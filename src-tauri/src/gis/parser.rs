use crate::models::FieldSchema;
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value as GeoValue};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};

pub struct ParsedGeoData {
    pub feature_count: usize,
    pub geom_types: Vec<String>,
    pub bbox: Option<[f64; 4]>, // [min_lng, min_lat, max_lng, max_lat]
    pub properties_schema: Vec<FieldSchema>,
    pub feature_collection: FeatureCollection,
}

pub fn parse_geojson_str(raw: &str) -> Result<ParsedGeoData, String> {
    let geojson = raw
        .parse::<GeoJson>()
        .map_err(|e| format!("Invalid GeoJSON: {}", e))?;

    let fc = match geojson {
        GeoJson::FeatureCollection(fc) => fc,
        GeoJson::Feature(feature) => FeatureCollection {
            bbox: None,
            features: vec![feature],
            foreign_members: None,
        },
        GeoJson::Geometry(geometry) => FeatureCollection {
            bbox: None,
            features: vec![Feature {
                bbox: None,
                geometry: Some(geometry),
                id: None,
                properties: None,
                foreign_members: None,
            }],
            foreign_members: None,
        },
    };

    let mut min_lng = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lng = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    let mut has_coords = false;

    let mut geom_type_set = HashSet::new();
    let mut property_field_types: HashMap<String, (String, Option<String>)> = HashMap::new();

    for feature in &fc.features {
        if let Some(ref geom) = feature.geometry {
            let type_name = match &geom.value {
                GeoValue::Point(_) => "Point",
                GeoValue::MultiPoint(_) => "MultiPoint",
                GeoValue::LineString(_) => "LineString",
                GeoValue::MultiLineString(_) => "MultiLineString",
                GeoValue::Polygon(_) => "Polygon",
                GeoValue::MultiPolygon(_) => "MultiPolygon",
                GeoValue::GeometryCollection(_) => "GeometryCollection",
            };
            geom_type_set.insert(type_name.to_string());

            update_bounds_from_geom(
                geom,
                &mut min_lng,
                &mut min_lat,
                &mut max_lng,
                &mut max_lat,
                &mut has_coords,
            );
        }

        if let Some(ref props) = feature.properties {
            for (key, val) in props {
                let (ftype, sample) = match val {
                    JsonValue::Number(n) => ("number".to_string(), Some(n.to_string())),
                    JsonValue::String(s) => ("string".to_string(), Some(s.clone())),
                    JsonValue::Bool(b) => ("boolean".to_string(), Some(b.to_string())),
                    JsonValue::Array(_) => ("array".to_string(), Some("[Array]".to_string())),
                    JsonValue::Object(_) => ("object".to_string(), Some("[Object]".to_string())),
                    JsonValue::Null => ("null".to_string(), None),
                };

                property_field_types
                    .entry(key.clone())
                    .or_insert((ftype, sample));
            }
        }
    }

    let bbox = if has_coords {
        Some([min_lng, min_lat, max_lng, max_lat])
    } else {
        None
    };

    let mut properties_schema: Vec<FieldSchema> = property_field_types
        .into_iter()
        .map(|(name, (field_type, sample_value))| FieldSchema {
            name,
            field_type,
            sample_value,
        })
        .collect();
    properties_schema.sort_by(|a, b| a.name.cmp(&b.name));

    let mut geom_types: Vec<String> = geom_type_set.into_iter().collect();
    geom_types.sort();

    let feature_count = fc.features.len();

    Ok(ParsedGeoData {
        feature_count,
        geom_types,
        bbox,
        properties_schema,
        feature_collection: fc,
    })
}

fn update_bounds_from_geom(
    geom: &Geometry,
    min_lng: &mut f64,
    min_lat: &mut f64,
    max_lng: &mut f64,
    max_lat: &mut f64,
    has_coords: &mut bool,
) {
    match &geom.value {
        GeoValue::Point(coords) => {
            expand_bounds(
                coords[0], coords[1], min_lng, min_lat, max_lng, max_lat, has_coords,
            );
        }
        GeoValue::MultiPoint(coords_list) => {
            for c in coords_list {
                expand_bounds(c[0], c[1], min_lng, min_lat, max_lng, max_lat, has_coords);
            }
        }
        GeoValue::LineString(coords_list) => {
            for c in coords_list {
                expand_bounds(c[0], c[1], min_lng, min_lat, max_lng, max_lat, has_coords);
            }
        }
        GeoValue::MultiLineString(lines) => {
            for line in lines {
                for c in line {
                    expand_bounds(c[0], c[1], min_lng, min_lat, max_lng, max_lat, has_coords);
                }
            }
        }
        GeoValue::Polygon(rings) => {
            for ring in rings {
                for c in ring {
                    expand_bounds(c[0], c[1], min_lng, min_lat, max_lng, max_lat, has_coords);
                }
            }
        }
        GeoValue::MultiPolygon(polys) => {
            for poly in polys {
                for ring in poly {
                    for c in ring {
                        expand_bounds(c[0], c[1], min_lng, min_lat, max_lng, max_lat, has_coords);
                    }
                }
            }
        }
        GeoValue::GeometryCollection(geoms) => {
            for g in geoms {
                update_bounds_from_geom(g, min_lng, min_lat, max_lng, max_lat, has_coords);
            }
        }
    }
}

fn expand_bounds(
    lng: f64,
    lat: f64,
    min_lng: &mut f64,
    min_lat: &mut f64,
    max_lng: &mut f64,
    max_lat: &mut f64,
    has_coords: &mut bool,
) {
    if lng.is_finite() && lat.is_finite() {
        *has_coords = true;
        if lng < *min_lng {
            *min_lng = lng;
        }
        if lat < *min_lat {
            *min_lat = lat;
        }
        if lng > *max_lng {
            *max_lng = lng;
        }
        if lat > *max_lat {
            *max_lat = lat;
        }
    }
}
