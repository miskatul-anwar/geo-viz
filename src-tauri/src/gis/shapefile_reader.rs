use crate::gis::parser::ParsedGeoData;
use crate::models::FieldSchema;
use geojson::{Feature, FeatureCollection, Geometry, Value};
use serde_json::{json, Map};
use shapefile::dbase;
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

pub fn parse_shapefile_bytes(
    shp_or_zip_bytes: &[u8],
    _layer_name: Option<String>,
) -> Result<ParsedGeoData, String> {
    if shp_or_zip_bytes.is_empty() {
        return Err("Shapefile buffer is empty".to_string());
    }

    // Check if zip archive (Magic bytes PK\x03\x04 or PK\x05\x06 or PK\x07\x08)
    if shp_or_zip_bytes.len() >= 4
        && (shp_or_zip_bytes[0..4] == [0x50, 0x4B, 0x03, 0x04]
            || shp_or_zip_bytes[0..4] == [0x50, 0x4B, 0x05, 0x06]
            || shp_or_zip_bytes[0..4] == [0x50, 0x4B, 0x07, 0x08])
    {
        parse_shapefile_zip(shp_or_zip_bytes)
    } else {
        parse_raw_shp_bytes(shp_or_zip_bytes, None)
    }
}

pub fn parse_shapefile_zip(zip_bytes: &[u8]) -> Result<ParsedGeoData, String> {
    let cursor = Cursor::new(zip_bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid ZIP archive: {}", e))?;

    let mut shp_bytes: Option<Vec<u8>> = None;
    let mut dbf_bytes: Option<Vec<u8>> = None;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();
        let lower = name.to_lowercase();

        // Skip macOS metadata files and hidden files
        if lower.contains("__macosx") || lower.starts_with('.') || lower.contains("/.") {
            continue;
        }

        if lower.ends_with(".shp") {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            shp_bytes = Some(buf);
        } else if lower.ends_with(".dbf") {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            dbf_bytes = Some(buf);
        }
    }

    let shp_data =
        shp_bytes.ok_or_else(|| "ZIP archive does not contain a valid .shp file".to_string())?;
    parse_raw_shp_bytes(&shp_data, dbf_bytes.as_deref())
}

pub fn parse_raw_shp_bytes(
    shp_bytes: &[u8],
    dbf_bytes: Option<&[u8]>,
) -> Result<ParsedGeoData, String> {
    let mut features: Vec<Feature> = Vec::new();
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut geom_types_set = std::collections::HashSet::new();
    let mut prop_type_map: BTreeMap<String, String> = BTreeMap::new();

    // Try reading with DBF attributes if DBF is provided
    let mut read_success = false;
    if let Some(dbf_data) = dbf_bytes {
        let shp_cursor = Cursor::new(shp_bytes);
        let dbf_cursor = Cursor::new(dbf_data);

        if let (Ok(shape_reader), Ok(dbase_reader)) = (
            shapefile::ShapeReader::new(shp_cursor),
            dbase::Reader::new(dbf_cursor),
        ) {
            let mut reader = shapefile::Reader::new(shape_reader, dbase_reader);
            if let Ok(shape_records) = reader.read() {
                for (shape, record) in shape_records {
                    if let Ok((geojson_geom, type_name, bounds)) = shape_to_geojson_geometry(shape)
                    {
                        geom_types_set.insert(type_name);

                        if let Some((b_min_x, b_min_y, b_max_x, b_max_y)) = bounds {
                            min_x = min_x.min(b_min_x);
                            min_y = min_y.min(b_min_y);
                            max_x = max_x.max(b_max_x);
                            max_y = max_y.max(b_max_y);
                        }

                        let mut props_map = Map::new();
                        for (key, field_value) in record {
                            let val = match field_value {
                                dbase::FieldValue::Character(Some(s)) => {
                                    prop_type_map
                                        .entry(key.clone())
                                        .or_insert_with(|| "string".to_string());
                                    json!(s.trim())
                                }
                                dbase::FieldValue::Numeric(Some(n)) => {
                                    prop_type_map
                                        .entry(key.clone())
                                        .or_insert_with(|| "number".to_string());
                                    json!(n)
                                }
                                dbase::FieldValue::Float(Some(f)) => {
                                    prop_type_map
                                        .entry(key.clone())
                                        .or_insert_with(|| "number".to_string());
                                    json!(f)
                                }
                                dbase::FieldValue::Double(d) => {
                                    prop_type_map
                                        .entry(key.clone())
                                        .or_insert_with(|| "number".to_string());
                                    json!(d)
                                }
                                dbase::FieldValue::Logical(Some(b)) => {
                                    prop_type_map
                                        .entry(key.clone())
                                        .or_insert_with(|| "boolean".to_string());
                                    json!(b)
                                }
                                dbase::FieldValue::Date(Some(d)) => {
                                    prop_type_map
                                        .entry(key.clone())
                                        .or_insert_with(|| "date".to_string());
                                    json!(format!(
                                        "{:04}-{:02}-{:02}",
                                        d.year(),
                                        d.month(),
                                        d.day()
                                    ))
                                }
                                _ => json!(null),
                            };
                            props_map.insert(key, val);
                        }

                        let feature = Feature {
                            bbox: None,
                            geometry: Some(geojson_geom),
                            id: None,
                            properties: Some(props_map),
                            foreign_members: None,
                        };
                        features.push(feature);
                    }
                }
                read_success = !features.is_empty();
            }
        }
    }

    // Fallback: Read shapes directly if DBF wasn't present or failed
    if !read_success {
        let shp_cursor = Cursor::new(shp_bytes);
        let mut shape_reader = shapefile::ShapeReader::new(shp_cursor)
            .map_err(|e| format!("Failed to read Shapefile: {}", e))?;

        for shape in shape_reader.iter_shapes().flatten() {
            if let Ok((geojson_geom, type_name, bounds)) = shape_to_geojson_geometry(shape) {
                geom_types_set.insert(type_name);

                if let Some((b_min_x, b_min_y, b_max_x, b_max_y)) = bounds {
                    min_x = min_x.min(b_min_x);
                    min_y = min_y.min(b_min_y);
                    max_x = max_x.max(b_max_x);
                    max_y = max_y.max(b_max_y);
                }

                let feature = Feature {
                    bbox: None,
                    geometry: Some(geojson_geom),
                    id: None,
                    properties: Some(Map::new()),
                    foreign_members: None,
                };
                features.push(feature);
            }
        }
    }

    if features.is_empty() {
        return Err("Shapefile contains no readable features or valid geometry".to_string());
    }

    let bbox = if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some([min_x, min_y, max_x, max_y])
    } else {
        None
    };

    let properties_schema: Vec<FieldSchema> = prop_type_map
        .into_iter()
        .map(|(key, inferred_type)| FieldSchema {
            name: key,
            field_type: inferred_type,
            sample_value: None,
        })
        .collect();

    let feature_collection = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    let feature_count = feature_collection.features.len();

    Ok(ParsedGeoData {
        feature_collection,
        feature_count,
        geom_types: geom_types_set.into_iter().collect(),
        bbox,
        properties_schema,
    })
}

/// A converted shape: `(GeoJSON geometry, type name, (min_x, min_y, max_x, max_y))`.
type ConvertedShape = (Geometry, String, Option<(f64, f64, f64, f64)>);

fn shape_to_geojson_geometry(shape: shapefile::Shape) -> Result<ConvertedShape, String> {
    match shape {
        shapefile::Shape::Point(pt) => {
            let bounds = Some((pt.x, pt.y, pt.x, pt.y));
            let geom = Geometry::new(Value::Point(vec![pt.x, pt.y]));
            Ok((geom, "Point".to_string(), bounds))
        }
        shapefile::Shape::PointM(pt) => {
            let bounds = Some((pt.x, pt.y, pt.x, pt.y));
            let geom = Geometry::new(Value::Point(vec![pt.x, pt.y]));
            Ok((geom, "Point".to_string(), bounds))
        }
        shapefile::Shape::PointZ(pt) => {
            let bounds = Some((pt.x, pt.y, pt.x, pt.y));
            let geom = Geometry::new(Value::Point(vec![pt.x, pt.y, pt.z]));
            Ok((geom, "Point".to_string(), bounds))
        }
        shapefile::Shape::Polyline(poly) => {
            let bbox = poly.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let parts = poly.parts();
            let coords: Vec<Vec<Vec<f64>>> = parts
                .iter()
                .map(|part| part.iter().map(|p| vec![p.x, p.y]).collect())
                .collect();

            if coords.len() == 1 {
                let geom = Geometry::new(Value::LineString(coords[0].clone()));
                Ok((geom, "LineString".to_string(), bounds))
            } else {
                let geom = Geometry::new(Value::MultiLineString(coords));
                Ok((geom, "MultiLineString".to_string(), bounds))
            }
        }
        shapefile::Shape::PolylineM(poly) => {
            let bbox = poly.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let parts = poly.parts();
            let coords: Vec<Vec<Vec<f64>>> = parts
                .iter()
                .map(|part| part.iter().map(|p| vec![p.x, p.y]).collect())
                .collect();

            if coords.len() == 1 {
                let geom = Geometry::new(Value::LineString(coords[0].clone()));
                Ok((geom, "LineString".to_string(), bounds))
            } else {
                let geom = Geometry::new(Value::MultiLineString(coords));
                Ok((geom, "MultiLineString".to_string(), bounds))
            }
        }
        shapefile::Shape::PolylineZ(poly) => {
            let bbox = poly.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let parts = poly.parts();
            let coords: Vec<Vec<Vec<f64>>> = parts
                .iter()
                .map(|part| part.iter().map(|p| vec![p.x, p.y, p.z]).collect())
                .collect();

            if coords.len() == 1 {
                let geom = Geometry::new(Value::LineString(coords[0].clone()));
                Ok((geom, "LineString".to_string(), bounds))
            } else {
                let geom = Geometry::new(Value::MultiLineString(coords));
                Ok((geom, "MultiLineString".to_string(), bounds))
            }
        }
        shapefile::Shape::Polygon(poly) => {
            let bbox = poly.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let rings = poly.rings();
            let coords: Vec<Vec<Vec<f64>>> = rings
                .iter()
                .map(|ring| {
                    let mut pts: Vec<Vec<f64>> =
                        ring.points().iter().map(|p| vec![p.x, p.y]).collect();
                    if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                        if first != last {
                            pts.push(first.clone());
                        }
                    }
                    pts
                })
                .collect();

            let geom = Geometry::new(Value::Polygon(coords));
            Ok((geom, "Polygon".to_string(), bounds))
        }
        shapefile::Shape::PolygonM(poly) => {
            let bbox = poly.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let rings = poly.rings();
            let coords: Vec<Vec<Vec<f64>>> = rings
                .iter()
                .map(|ring| {
                    let mut pts: Vec<Vec<f64>> =
                        ring.points().iter().map(|p| vec![p.x, p.y]).collect();
                    if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                        if first != last {
                            pts.push(first.clone());
                        }
                    }
                    pts
                })
                .collect();

            let geom = Geometry::new(Value::Polygon(coords));
            Ok((geom, "Polygon".to_string(), bounds))
        }
        shapefile::Shape::PolygonZ(poly) => {
            let bbox = poly.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let rings = poly.rings();
            let coords: Vec<Vec<Vec<f64>>> = rings
                .iter()
                .map(|ring| {
                    let mut pts: Vec<Vec<f64>> =
                        ring.points().iter().map(|p| vec![p.x, p.y, p.z]).collect();
                    if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                        if first != last {
                            pts.push(first.clone());
                        }
                    }
                    pts
                })
                .collect();

            let geom = Geometry::new(Value::Polygon(coords));
            Ok((geom, "Polygon".to_string(), bounds))
        }
        shapefile::Shape::Multipoint(mp) => {
            let bbox = mp.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let pts: Vec<Vec<f64>> = mp.points().iter().map(|p| vec![p.x, p.y]).collect();
            let geom = Geometry::new(Value::MultiPoint(pts));
            Ok((geom, "MultiPoint".to_string(), bounds))
        }
        shapefile::Shape::MultipointM(mp) => {
            let bbox = mp.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let pts: Vec<Vec<f64>> = mp.points().iter().map(|p| vec![p.x, p.y]).collect();
            let geom = Geometry::new(Value::MultiPoint(pts));
            Ok((geom, "MultiPoint".to_string(), bounds))
        }
        shapefile::Shape::MultipointZ(mp) => {
            let bbox = mp.bbox();
            let bounds = Some((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y));
            let pts: Vec<Vec<f64>> = mp.points().iter().map(|p| vec![p.x, p.y, p.z]).collect();
            let geom = Geometry::new(Value::MultiPoint(pts));
            Ok((geom, "MultiPoint".to_string(), bounds))
        }
        shapefile::Shape::NullShape => {
            let geom = Geometry::new(Value::GeometryCollection(vec![]));
            Ok((geom, "NullShape".to_string(), None))
        }
        _ => Err("Unsupported Shapefile geometry variant".to_string()),
    }
}
