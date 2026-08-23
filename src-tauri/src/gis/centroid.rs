use geo::{Centroid, Coord, LineString, Polygon};
use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::Map;

pub fn calculate_centroids(
    fc: &FeatureCollection,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    if fc.features.is_empty() {
        return Err("No features to compute centroids for".to_string());
    }

    let mut output_features = Vec::new();

    for (i, feature) in fc.features.iter().enumerate() {
        if let Some(ref geom) = feature.geometry {
            if let Some(centroid_pt) = compute_geom_centroid(geom) {
                let mut props = feature.properties.clone().unwrap_or_else(Map::new);
                props.insert(
                    "centroid_lng".to_string(),
                    serde_json::json!(centroid_pt[0]),
                );
                props.insert(
                    "centroid_lat".to_string(),
                    serde_json::json!(centroid_pt[1]),
                );
                props.insert("source_feature_index".to_string(), serde_json::json!(i));

                output_features.push(Feature {
                    bbox: None,
                    geometry: Some(Geometry::new(GeoValue::Point(centroid_pt))),
                    id: feature.id.clone(),
                    properties: Some(props),
                    foreign_members: None,
                });
            }
        }
    }

    let out_fc = FeatureCollection {
        bbox: None,
        features: output_features,
        foreign_members: None,
    };

    let summary = serde_json::json!({
        "input_features": fc.features.len(),
        "computed_centroids": out_fc.features.len()
    });

    Ok((out_fc, summary))
}

fn compute_geom_centroid(geom: &Geometry) -> Option<Vec<f64>> {
    match &geom.value {
        GeoValue::Point(p) => {
            if p.len() >= 2 && p[0].is_finite() && p[1].is_finite() {
                Some(vec![p[0], p[1]])
            } else {
                None
            }
        }
        GeoValue::MultiPoint(pts) => {
            if pts.is_empty() {
                return None;
            }
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut count = 0.0;
            for p in pts {
                if p.len() >= 2 && p[0].is_finite() && p[1].is_finite() {
                    sum_x += p[0];
                    sum_y += p[1];
                    count += 1.0;
                }
            }
            if count > 0.0 {
                Some(vec![sum_x / count, sum_y / count])
            } else {
                None
            }
        }
        GeoValue::Polygon(rings) => {
            if rings.is_empty() || rings[0].is_empty() {
                return None;
            }
            let exterior_coords: Vec<Coord<f64>> = rings[0]
                .iter()
                .filter(|c| c.len() >= 2 && c[0].is_finite() && c[1].is_finite())
                .map(|c| Coord { x: c[0], y: c[1] })
                .collect();
            if exterior_coords.len() < 3 {
                return None;
            }
            let poly = Polygon::new(LineString::new(exterior_coords), vec![]);
            poly.centroid().map(|p| vec![p.x(), p.y()])
        }
        GeoValue::MultiPolygon(polys) => {
            let mut all_centroids = Vec::new();
            for rings in polys {
                if rings.is_empty() || rings[0].is_empty() {
                    continue;
                }
                let coords: Vec<Coord<f64>> = rings[0]
                    .iter()
                    .filter(|c| c.len() >= 2 && c[0].is_finite() && c[1].is_finite())
                    .map(|c| Coord { x: c[0], y: c[1] })
                    .collect();
                if coords.len() >= 3 {
                    let poly = Polygon::new(LineString::new(coords), vec![]);
                    if let Some(c) = poly.centroid() {
                        all_centroids.push(c);
                    }
                }
            }
            if all_centroids.is_empty() {
                return None;
            }
            let count = all_centroids.len() as f64;
            let avg_x: f64 = all_centroids.iter().map(|p| p.x()).sum::<f64>() / count;
            let avg_y: f64 = all_centroids.iter().map(|p| p.y()).sum::<f64>() / count;
            Some(vec![avg_x, avg_y])
        }
        _ => {
            let mut all_coords = Vec::new();
            super::convex_hull::extract_coords_from_geom(geom, &mut all_coords);
            if all_coords.is_empty() {
                return None;
            }
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut count = 0.0;
            for c in &all_coords {
                if c.len() >= 2 && c[0].is_finite() && c[1].is_finite() {
                    sum_x += c[0];
                    sum_y += c[1];
                    count += 1.0;
                }
            }
            if count > 0.0 {
                Some(vec![sum_x / count, sum_y / count])
            } else {
                None
            }
        }
    }
}
