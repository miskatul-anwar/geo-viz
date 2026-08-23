//! GPX (GPS Exchange) ingestion: waypoints, routes and tracks.

use super::parser::ParsedGeoData;
use super::xml_tree::{parse_document, XmlElement};
use crate::gis::parser::parse_geojson_str;
use geojson::{Feature, Geometry, Value as GeoValue};
use serde_json::{Map, Value as JsonValue};

pub fn parse_gpx_str(xml: &str) -> Result<ParsedGeoData, String> {
    let roots = parse_document(xml)?;
    let mut features = Vec::new();

    for root in &roots {
        collect(root, &mut features);
    }

    if features.is_empty() {
        return Err("no waypoints, tracks or routes found in GPX".into());
    }
    finish(features)
}

fn collect(node: &XmlElement, out: &mut Vec<Feature>) {
    match node.name.as_str() {
        "wpt" => {
            if let Some(f) = point_feature(node) {
                out.push(f);
            }
            return; // wpt has no nested features
        }
        "trk" => {
            // One LineString per <trkseg>.
            for seg in node.children_named("trkseg") {
                let pts = segment_points(seg);
                if pts.len() >= 2 {
                    out.push(line_feature(pts, track_properties(node)));
                }
            }
            return;
        }
        "rte" => {
            let pts = node
                .children_named("rtept")
                .filter_map(point_coord)
                .collect::<Vec<_>>();
            if pts.len() >= 2 {
                out.push(line_feature(pts, route_properties(node)));
            }
            return;
        }
        _ => {}
    }
    for child in &node.children {
        collect(child, out);
    }
}

fn point_feature(wpt: &XmlElement) -> Option<Feature> {
    let coord = point_coord(wpt)?;
    Some(Feature {
        bbox: None,
        geometry: Some(Geometry::new(GeoValue::Point(coord))),
        id: None,
        properties: Some(common_properties(wpt)),
        foreign_members: None,
    })
}

fn line_feature(coords: Vec<Vec<f64>>, mut props: Map<String, JsonValue>) -> Feature {
    props
        .entry("gpx_kind".to_string())
        .or_insert_with(|| JsonValue::String("track".into()));
    Feature {
        bbox: None,
        geometry: Some(Geometry::new(GeoValue::LineString(coords))),
        id: None,
        properties: Some(props),
        foreign_members: None,
    }
}

fn track_properties(trk: &XmlElement) -> Map<String, JsonValue> {
    let mut props = common_properties(trk);
    props.insert("gpx_kind".into(), JsonValue::String("track".into()));
    props
}

fn route_properties(rte: &XmlElement) -> Map<String, JsonValue> {
    let mut props = common_properties(rte);
    props.insert("gpx_kind".into(), JsonValue::String("route".into()));
    props
}

fn common_properties(el: &XmlElement) -> Map<String, JsonValue> {
    let mut props = Map::new();
    for tag in ["name", "desc", "sym", "type"] {
        if let Some(v) = el
            .child(tag)
            .map(|c| c.text.clone())
            .filter(|t| !t.is_empty())
        {
            props.insert(tag.to_string(), JsonValue::String(v));
        }
    }
    if let Some(time) = el
        .child("time")
        .map(|c| c.text.clone())
        .filter(|t| !t.is_empty())
    {
        props.insert("time".into(), JsonValue::String(time));
    }
    props
}

/// `[lon, lat]` from a `lat`/`lon` attributed point element.
fn point_coord(pt: &XmlElement) -> Option<Vec<f64>> {
    let lat: f64 = pt.attr("lat")?.parse().ok()?;
    let lon: f64 = pt.attr("lon")?.parse().ok()?;
    if !lat.is_finite() || !lon.is_finite() {
        return None;
    }
    Some(vec![lon, lat])
}

fn segment_points(seg: &XmlElement) -> Vec<Vec<f64>> {
    seg.children_named("trkpt")
        .filter_map(point_coord)
        .collect()
}

fn finish(features: Vec<Feature>) -> Result<ParsedGeoData, String> {
    let fc = geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    let json = serde_json::to_string(&fc).map_err(|e| e.to_string())?;
    parse_geojson_str(&json)
}
