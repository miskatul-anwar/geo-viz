use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::Map;

pub fn simplify_geometries(
    fc: &FeatureCollection,
    tolerance_deg: f64,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    let tolerance = if tolerance_deg <= 0.0 {
        0.001
    } else {
        tolerance_deg
    };
    let mut simplified_features = Vec::new();
    let mut initial_vertices = 0;
    let mut final_vertices = 0;

    for feature in &fc.features {
        if let Some(ref geom) = feature.geometry {
            let (new_geom, in_v, out_v) = simplify_geom(geom, tolerance);
            initial_vertices += in_v;
            final_vertices += out_v;

            let mut props = feature.properties.clone().unwrap_or_else(Map::new);
            props.insert(
                "simplified_tolerance".to_string(),
                serde_json::json!(tolerance),
            );
            props.insert("original_vertices".to_string(), serde_json::json!(in_v));
            props.insert("simplified_vertices".to_string(), serde_json::json!(out_v));

            simplified_features.push(Feature {
                bbox: None,
                geometry: Some(new_geom),
                id: feature.id.clone(),
                properties: Some(props),
                foreign_members: None,
            });
        }
    }

    let out_fc = FeatureCollection {
        bbox: None,
        features: simplified_features,
        foreign_members: None,
    };

    let reduction_pct = if initial_vertices > 0 {
        100.0 * (1.0 - (final_vertices as f64 / initial_vertices as f64))
    } else {
        0.0
    };

    let summary = serde_json::json!({
        "input_features": fc.features.len(),
        "tolerance": tolerance,
        "initial_vertices": initial_vertices,
        "final_vertices": final_vertices,
        "vertex_reduction_percent": (reduction_pct * 10.0).round() / 10.0
    });

    Ok((out_fc, summary))
}

fn simplify_geom(geom: &Geometry, tolerance: f64) -> (Geometry, usize, usize) {
    match &geom.value {
        GeoValue::LineString(coords) => {
            let in_count = coords.len();
            let simplified = douglas_peucker(coords, tolerance);
            let out_count = simplified.len();
            (
                Geometry::new(GeoValue::LineString(simplified)),
                in_count,
                out_count,
            )
        }
        GeoValue::MultiLineString(lines) => {
            let mut new_lines = Vec::new();
            let mut in_count = 0;
            let mut out_count = 0;
            for l in lines {
                in_count += l.len();
                let simp = douglas_peucker(l, tolerance);
                out_count += simp.len();
                new_lines.push(simp);
            }
            (
                Geometry::new(GeoValue::MultiLineString(new_lines)),
                in_count,
                out_count,
            )
        }
        GeoValue::Polygon(rings) => {
            let mut new_rings = Vec::new();
            let mut in_count = 0;
            let mut out_count = 0;
            for r in rings {
                in_count += r.len();
                let mut simp = douglas_peucker(r, tolerance);
                if simp.len() >= 3 {
                    if let Some(first) = simp.first().cloned() {
                        if simp.last() != Some(&first) {
                            simp.push(first);
                        }
                    }
                }
                out_count += simp.len();
                new_rings.push(simp);
            }
            (
                Geometry::new(GeoValue::Polygon(new_rings)),
                in_count,
                out_count,
            )
        }
        GeoValue::MultiPolygon(polys) => {
            let mut new_polys = Vec::new();
            let mut in_count = 0;
            let mut out_count = 0;
            for p in polys {
                let mut new_rings = Vec::new();
                for r in p {
                    in_count += r.len();
                    let mut simp = douglas_peucker(r, tolerance);
                    if simp.len() >= 3 {
                        if let Some(first) = simp.first().cloned() {
                            if simp.last() != Some(&first) {
                                simp.push(first);
                            }
                        }
                    }
                    out_count += simp.len();
                    new_rings.push(simp);
                }
                new_polys.push(new_rings);
            }
            (
                Geometry::new(GeoValue::MultiPolygon(new_polys)),
                in_count,
                out_count,
            )
        }
        _ => (geom.clone(), 1, 1),
    }
}

fn douglas_peucker(points: &[Vec<f64>], tolerance: f64) -> Vec<Vec<f64>> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut dmax = 0.0;
    let mut index = 0;
    let end = points.len() - 1;

    for i in 1..end {
        let d = perpendicular_distance(&points[i], &points[0], &points[end]);
        if d > dmax {
            index = i;
            dmax = d;
        }
    }

    if dmax > tolerance {
        let left = douglas_peucker(&points[0..=index], tolerance);
        let right = douglas_peucker(&points[index..=end], tolerance);

        let mut res = left[0..left.len() - 1].to_vec();
        res.extend(right);
        res
    } else {
        vec![points[0].clone(), points[end].clone()]
    }
}

fn perpendicular_distance(pt: &[f64], start: &[f64], end: &[f64]) -> f64 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let len_sq = dx * dx + dy * dy;

    if len_sq < 1e-12 {
        let px = pt[0] - start[0];
        let py = pt[1] - start[1];
        return (px * px + py * py).sqrt();
    }

    let numerator = ((end[1] - start[1]) * pt[0] - (end[0] - start[0]) * pt[1] + end[0] * start[1]
        - end[1] * start[0])
        .abs();
    numerator / len_sq.sqrt()
}
