use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use rand::Rng;
use serde_json::Map;

pub fn generate_random_points(
    fc: &FeatureCollection,
    count: usize,
    restrict_to_polygons: bool,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    let count = count.clamp(1, 10000);
    let mut all_coords = Vec::new();
    let mut polygons = Vec::new();

    for f in &fc.features {
        if let Some(ref geom) = f.geometry {
            super::convex_hull::extract_coords_from_geom(geom, &mut all_coords);
            match &geom.value {
                GeoValue::Polygon(rings) => polygons.push(rings.clone()),
                GeoValue::MultiPolygon(polys) => {
                    for p in polys {
                        polygons.push(p.clone());
                    }
                }
                _ => {}
            }
        }
    }

    if all_coords.is_empty() {
        return Err("Cannot determine bounds from empty dataset".to_string());
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for c in &all_coords {
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

    let mut rng = rand::thread_rng();
    let mut generated_features = Vec::new();
    let max_attempts = count * 50;
    let mut attempts = 0;

    while generated_features.len() < count && attempts < max_attempts {
        attempts += 1;
        let rx = rng.gen_range(min_x..=max_x);
        let ry = rng.gen_range(min_y..=max_y);
        let pt = vec![rx, ry];

        if restrict_to_polygons && !polygons.is_empty() {
            let mut inside = false;
            for poly in &polygons {
                if super::spatial_query::point_in_polygon(&pt, poly) {
                    inside = true;
                    break;
                }
            }
            if !inside {
                continue;
            }
        }

        let mut props = Map::new();
        props.insert(
            "point_id".to_string(),
            serde_json::json!(generated_features.len() + 1),
        );
        props.insert(
            "lng".to_string(),
            serde_json::json!((rx * 100000.0).round() / 100000.0),
        );
        props.insert(
            "lat".to_string(),
            serde_json::json!((ry * 100000.0).round() / 100000.0),
        );
        let weight: f64 = rng.gen_range(10.0..100.0);
        props.insert(
            "sample_weight".to_string(),
            serde_json::json!((weight * 10.0).round() / 10.0),
        );

        generated_features.push(Feature {
            bbox: None,
            geometry: Some(Geometry::new(GeoValue::Point(pt))),
            id: None,
            properties: Some(props),
            foreign_members: None,
        });
    }

    let out_fc = FeatureCollection {
        bbox: None,
        features: generated_features,
        foreign_members: None,
    };

    let summary = serde_json::json!({
        "requested_count": count,
        "generated_count": out_fc.features.len(),
        "restricted_to_polygons": restrict_to_polygons,
        "attempts": attempts
    });

    Ok((out_fc, summary))
}
