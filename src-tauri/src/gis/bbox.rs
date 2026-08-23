use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::Map;

pub fn calculate_bounding_boxes(
    fc: &FeatureCollection,
    per_feature: bool,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    if fc.features.is_empty() {
        return Err("Dataset contains no features".to_string());
    }

    let mut output_features = Vec::new();

    if per_feature {
        for (i, feature) in fc.features.iter().enumerate() {
            if let Some(ref geom) = feature.geometry {
                let mut coords = Vec::new();
                super::convex_hull::extract_coords_from_geom(geom, &mut coords);

                if let Some(bbox) = get_bounds(&coords) {
                    let poly = bbox_to_polygon(&bbox);
                    let mut props = feature.properties.clone().unwrap_or_else(Map::new);
                    props.insert("bbox_min_lng".to_string(), serde_json::json!(bbox[0]));
                    props.insert("bbox_min_lat".to_string(), serde_json::json!(bbox[1]));
                    props.insert("bbox_max_lng".to_string(), serde_json::json!(bbox[2]));
                    props.insert("bbox_max_lat".to_string(), serde_json::json!(bbox[3]));
                    props.insert("source_feature_idx".to_string(), serde_json::json!(i));

                    output_features.push(Feature {
                        bbox: Some(vec![bbox[0], bbox[1], bbox[2], bbox[3]]),
                        geometry: Some(Geometry::new(GeoValue::Polygon(poly))),
                        id: feature.id.clone(),
                        properties: Some(props),
                        foreign_members: None,
                    });
                }
            }
        }
    } else {
        let mut all_coords = Vec::new();
        for feature in &fc.features {
            if let Some(ref geom) = feature.geometry {
                super::convex_hull::extract_coords_from_geom(geom, &mut all_coords);
            }
        }

        if let Some(bbox) = get_bounds(&all_coords) {
            let poly = bbox_to_polygon(&bbox);
            let mut props = Map::new();
            props.insert(
                "name".to_string(),
                serde_json::json!("Layer Extent Bounding Box"),
            );
            props.insert("min_lng".to_string(), serde_json::json!(bbox[0]));
            props.insert("min_lat".to_string(), serde_json::json!(bbox[1]));
            props.insert("max_lng".to_string(), serde_json::json!(bbox[2]));
            props.insert("max_lat".to_string(), serde_json::json!(bbox[3]));
            props.insert(
                "width_deg".to_string(),
                serde_json::json!(bbox[2] - bbox[0]),
            );
            props.insert(
                "height_deg".to_string(),
                serde_json::json!(bbox[3] - bbox[1]),
            );

            output_features.push(Feature {
                bbox: Some(vec![bbox[0], bbox[1], bbox[2], bbox[3]]),
                geometry: Some(Geometry::new(GeoValue::Polygon(poly))),
                id: None,
                properties: Some(props),
                foreign_members: None,
            });
        } else {
            return Err("Failed to calculate layer bounds".to_string());
        }
    }

    let out_fc = FeatureCollection {
        bbox: None,
        features: output_features,
        foreign_members: None,
    };

    let summary = serde_json::json!({
        "input_features": fc.features.len(),
        "output_bboxes": out_fc.features.len(),
        "mode": if per_feature { "per_feature" } else { "unified_layer" }
    });

    Ok((out_fc, summary))
}

fn get_bounds(coords: &[Vec<f64>]) -> Option<[f64; 4]> {
    if coords.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for c in coords {
        if c.len() >= 2 && c[0].is_finite() && c[1].is_finite() {
            if c[0] < min_x {
                min_x = c[0];
            }
            if c[1] < min_y {
                min_y = c[1];
            }
            if c[0] > max_x {
                max_x = c[0];
            }
            if c[1] > max_y {
                max_y = c[1];
            }
        }
    }

    if min_x.is_finite() && max_x.is_finite() {
        Some([min_x, min_y, max_x, max_y])
    } else {
        None
    }
}

fn bbox_to_polygon(bbox: &[f64; 4]) -> Vec<Vec<Vec<f64>>> {
    let (min_x, min_y, max_x, max_y) = (bbox[0], bbox[1], bbox[2], bbox[3]);
    let ring = vec![
        vec![min_x, min_y],
        vec![max_x, min_y],
        vec![max_x, max_y],
        vec![min_x, max_y],
        vec![min_x, min_y],
    ];
    vec![ring]
}
