use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::Map;

pub fn calculate_nearest_neighbors(
    source_fc: &FeatureCollection,
    target_fc: Option<&FeatureCollection>,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    let source_points = extract_points_with_props(source_fc);
    if source_points.is_empty() {
        return Err("Source layer contains no valid point coordinates".to_string());
    }

    let target_points = if let Some(tfc) = target_fc {
        extract_points_with_props(tfc)
    } else {
        source_points.clone()
    };

    if target_points.is_empty() {
        return Err("Target layer contains no valid point coordinates".to_string());
    }

    let is_self = target_fc.is_none();
    let mut connection_lines = Vec::new();
    let mut distances = Vec::new();

    for (s_idx, (s_lng, s_lat, s_props)) in source_points.iter().enumerate() {
        let mut min_dist = f64::INFINITY;
        let mut nearest_pt = None;
        let mut nearest_props = None;

        for (t_idx, (t_lng, t_lat, t_props)) in target_points.iter().enumerate() {
            if is_self && s_idx == t_idx {
                continue;
            }

            let dist = super::metrics::haversine_distance(&[*s_lng, *s_lat], &[*t_lng, *t_lat]);
            if dist < min_dist {
                min_dist = dist;
                nearest_pt = Some((*t_lng, *t_lat));
                nearest_props = Some(t_props.clone());
            }
        }

        if let (Some((t_lng, t_lat)), Some(t_props)) = (nearest_pt, nearest_props) {
            distances.push(min_dist);

            let mut props = Map::new();
            props.insert(
                "distance_meters".to_string(),
                serde_json::json!((min_dist * 10.0).round() / 10.0),
            );
            props.insert(
                "distance_km".to_string(),
                serde_json::json!((min_dist / 1000.0 * 100.0).round() / 100.0),
            );
            props.insert("source_index".to_string(), serde_json::json!(s_idx));

            if let Some(sp) = s_props {
                for (k, v) in sp {
                    props.insert(format!("src_{}", k), v.clone());
                }
            }
            if let Some(tp) = t_props {
                for (k, v) in tp {
                    props.insert(format!("tgt_{}", k), v.clone());
                }
            }

            let line = vec![vec![*s_lng, *s_lat], vec![t_lng, t_lat]];

            connection_lines.push(Feature {
                bbox: None,
                geometry: Some(Geometry::new(GeoValue::LineString(line))),
                id: None,
                properties: Some(props),
                foreign_members: None,
            });
        }
    }

    let out_fc = FeatureCollection {
        bbox: None,
        features: connection_lines,
        foreign_members: None,
    };

    let avg_dist = if !distances.is_empty() {
        distances.iter().sum::<f64>() / (distances.len() as f64)
    } else {
        0.0
    };
    let min_dist = distances.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_dist = distances.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let summary = serde_json::json!({
        "connections_count": out_fc.features.len(),
        "avg_distance_km": (avg_dist / 1000.0 * 100.0).round() / 100.0,
        "min_distance_km": (min_dist / 1000.0 * 100.0).round() / 100.0,
        "max_distance_km": (max_dist / 1000.0 * 100.0).round() / 100.0
    });

    Ok((out_fc, summary))
}

/// A point with its original attribute payload: `(lng, lat, properties)`.
type PointWithProps = (f64, f64, Option<Map<String, serde_json::Value>>);

fn extract_points_with_props(fc: &FeatureCollection) -> Vec<PointWithProps> {
    let mut list = Vec::new();
    for f in &fc.features {
        if let Some(ref geom) = f.geometry {
            match &geom.value {
                GeoValue::Point(c) => list.push((c[0], c[1], f.properties.clone())),
                GeoValue::MultiPoint(pts) => {
                    for p in pts {
                        list.push((p[0], p[1], f.properties.clone()));
                    }
                }
                _ => {
                    let mut coords = Vec::new();
                    super::convex_hull::extract_coords_from_geom(geom, &mut coords);
                    if let Some(first) = coords.first() {
                        list.push((first[0], first[1], f.properties.clone()));
                    }
                }
            }
        }
    }
    list
}
