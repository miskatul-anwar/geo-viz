//! KML (`.kml`) and KMZ (`.kmz`, zipped KML) ingestion.

use super::parser::ParsedGeoData;
use super::xml_tree::{parse_document, XmlElement};
use crate::gis::parser::parse_geojson_str;
use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::{Map, Value as JsonValue};

/// Parse a KML document (raw XML text) into a GeoJSON feature collection.
pub fn parse_kml_str(xml: &str) -> Result<ParsedGeoData, String> {
    let roots = parse_document(xml)?;
    let mut features = Vec::new();
    for root in &roots {
        collect_placemark_features(root, &mut features);
    }
    if features.is_empty() {
        return Err("no usable <Placemark> geometries found in KML".into());
    }
    build(features)
}

/// Parse a KMZ archive: locate the KML document and parse it.
pub fn parse_kmz_bytes(bytes: &[u8]) -> Result<ParsedGeoData, String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| format!("invalid KMZ archive: {e}"))?;

    // Prefer doc.kml; otherwise the first non-metadata *.kml entry.
    let mut chosen: Option<String> = None;
    for i in 0..zip.len() {
        let name = zip
            .by_index(i)
            .map_err(|e| format!("corrupt KMZ entry: {e}"))?
            .name()
            .to_string();
        let lower = name.to_lowercase();
        if !lower.ends_with(".kml") || lower.contains("metadata") {
            continue;
        }
        let is_doc_kml = lower == "doc.kml";
        if is_doc_kml || chosen.is_none() {
            chosen = Some(name);
            if is_doc_kml {
                break;
            }
        }
    }
    let kml_name = chosen.ok_or("KMZ archive contains no .kml document")?;

    let mut content = String::new();
    {
        let mut file = zip
            .by_name(&kml_name)
            .map_err(|e| format!("failed to read {kml_name}: {e}"))?;
        std::io::Read::read_to_string(&mut file, &mut content)
            .map_err(|e| format!("failed to decode {kml_name}: {e}"))?;
    }
    parse_kml_str(&content)
}

// ---------------------------------------------------------------------------
// Placemark extraction
// ---------------------------------------------------------------------------

fn collect_placemark_features(node: &XmlElement, out: &mut Vec<Feature>) {
    if node.name == "placemark" {
        for feature in placemark_to_features(node) {
            out.push(feature);
        }
        return; // do not recurse into nested placemarks from here
    }
    for child in &node.children {
        collect_placemark_features(child, out);
    }
}

/// One Placemark may hold a MultiGeometry with several parts; each part is
/// emitted as its own feature sharing the same properties (QGIS-style flatten).
fn placemark_to_features(placemark: &XmlElement) -> Vec<Feature> {
    let properties = extract_properties(placemark);

    let mut geom_elems: Vec<&XmlElement> = Vec::new();
    for name in ["point", "linestring", "polygon", "multigeometry"] {
        let mut found = Vec::new();
        placemark.find_all(name, &mut found);
        geom_elems.extend(found);
    }
    // Keep only outermost geometry elements (skip children already covered).
    let geoms: Vec<&XmlElement> = geom_elems
        .iter()
        .copied()
        .filter(|g| !geom_elems.iter().any(|other| is_descendant(other, g)))
        .collect();

    let parsed: Vec<Geometry> = geoms
        .iter()
        .filter_map(|g| element_to_geometry(g))
        .collect();
    if parsed.is_empty() {
        return Vec::new();
    }

    parsed
        .into_iter()
        .map(|geometry| Feature {
            bbox: None,
            geometry: Some(geometry),
            id: None,
            properties: Some(properties.clone()),
            foreign_members: None,
        })
        .collect()
}

fn is_descendant(candidate_parent: &XmlElement, target: &XmlElement) -> bool {
    candidate_parent
        .children
        .iter()
        .any(|c| std::ptr::eq(c, target) || is_descendant(c, target))
}

fn extract_properties(placemark: &XmlElement) -> Map<String, JsonValue> {
    let mut props = Map::new();

    for tag in ["name", "description"] {
        if let Some(el) = placemark.child(tag) {
            if !el.text.is_empty() {
                props.insert(tag.to_string(), JsonValue::String(el.text.clone()));
            }
        }
    }

    // ExtendedData/Data[name]/value and SchemaData/SimpleData[name]
    if let Some(extended) = placemark.find_first("extendeddata") {
        for data in extended.children.iter() {
            let key = data.attr("name").map(str::to_string);
            let value = match data.name.as_str() {
                "data" => data.child("value").map(|v| v.text.clone()),
                "simpledata" => Some(data.text.clone()),
                _ => None,
            };
            if let (Some(key), Some(value)) = (key, value) {
                props.insert(key, JsonValue::String(value));
            }
        }
    }

    props
}

// ---------------------------------------------------------------------------
// Geometry conversion
// ---------------------------------------------------------------------------

fn element_to_geometry(el: &XmlElement) -> Option<Geometry> {
    match el.name.as_str() {
        "point" => Some(Geometry::new(GeoValue::Point(ring_coords(el)?.pop()?))),
        "linestring" => Some(Geometry::new(GeoValue::LineString(coords_list(el)?))),
        "linearring" => Some(Geometry::new(GeoValue::LineString(coords_list(el)?))),
        "polygon" => polygon_geometry(el),
        "multigeometry" => {
            let mut parts = Vec::new();
            for child in &el.children {
                if let Some(g) = element_to_geometry(child) {
                    parts.push(g);
                }
            }
            (!parts.is_empty()).then(|| Geometry::new(GeoValue::GeometryCollection(parts)))
        }
        _ => None,
    }
}

fn polygon_geometry(el: &XmlElement) -> Option<Geometry> {
    let outer = el
        .children
        .iter()
        .find(|c| c.name == "outerboundaryis")
        .and_then(|o| o.find_first("linearring"))
        .and_then(coords_list)?;
    let mut rings = vec![outer];

    for inner in el.children.iter().filter(|c| c.name == "innerboundaryis") {
        if let Some(ring) = inner.find_first("linearring").and_then(coords_list) {
            rings.push(ring);
        }
    }

    Some(Geometry::new(GeoValue::Polygon(rings)))
}

/// `<coordinates>lon,lat[,alt] ...</coordinates>` -> list of [lon, lat].
fn coords_list(el: &XmlElement) -> Option<Vec<Vec<f64>>> {
    let coords_el = el.find_first("coordinates")?;
    Some(parse_coordinates_text(&coords_el.text))
}

fn ring_coords(el: &XmlElement) -> Option<Vec<Vec<f64>>> {
    coords_list(el)
}

pub(super) fn parse_coordinates_text(text: &str) -> Vec<Vec<f64>> {
    text.split_whitespace()
        .filter_map(parse_coord_token)
        .collect()
}

fn parse_coord_token(token: &str) -> Option<Vec<f64>> {
    let mut parts = token.split(',');
    let lon: f64 = parts.next()?.trim().parse().ok()?;
    let lat: f64 = parts.next()?.trim().parse().ok()?;
    // Altitude, when present, is intentionally dropped (2D dataset).
    let _ = parts.next();
    if !lon.is_finite() || !lat.is_finite() {
        return None;
    }
    Some(vec![lon, lat])
}

fn build(features: Vec<Feature>) -> Result<ParsedGeoData, String> {
    let fc = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    let json = serde_json::to_string(&fc).map_err(|e| e.to_string())?;
    parse_geojson_str(&json)
}
