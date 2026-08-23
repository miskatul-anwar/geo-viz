use geojson::{Feature, FeatureCollection, Value as GeoValue};
use serde_json::Map;
use std::collections::HashMap;

const EARTH_RADIUS_M: f64 = 6378137.0;

pub fn calculate_metrics(
    fc: &FeatureCollection,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    let mut total_area_sqm = 0.0;
    let mut total_length_m = 0.0;
    let mut enriched_features = Vec::new();

    let mut numeric_stats: HashMap<String, Vec<f64>> = HashMap::new();

    for feature in &fc.features {
        let mut area_sqm = 0.0;
        let mut length_m = 0.0;

        if let Some(ref geom) = feature.geometry {
            match &geom.value {
                GeoValue::Polygon(rings) => {
                    area_sqm = polygon_spherical_area(rings);
                    if !rings.is_empty() {
                        length_m = linestring_geodesic_length(&rings[0]);
                    }
                }
                GeoValue::MultiPolygon(polys) => {
                    for rings in polys {
                        area_sqm += polygon_spherical_area(rings);
                        if !rings.is_empty() {
                            length_m += linestring_geodesic_length(&rings[0]);
                        }
                    }
                }
                GeoValue::LineString(coords) => {
                    length_m = linestring_geodesic_length(coords);
                }
                GeoValue::MultiLineString(lines) => {
                    for line in lines {
                        length_m += linestring_geodesic_length(line);
                    }
                }
                _ => {}
            }
        }

        total_area_sqm += area_sqm;
        total_length_m += length_m;

        let mut props = feature.properties.clone().unwrap_or_else(Map::new);
        if area_sqm > 0.0 {
            props.insert(
                "area_sqkm".to_string(),
                serde_json::json!((area_sqm / 1_000_000.0 * 1000.0).round() / 1000.0),
            );
            props.insert(
                "area_hectares".to_string(),
                serde_json::json!((area_sqm / 10_000.0 * 100.0).round() / 100.0),
            );
            props.insert(
                "perimeter_km".to_string(),
                serde_json::json!((length_m / 1000.0 * 1000.0).round() / 1000.0),
            );
        } else if length_m > 0.0 {
            props.insert(
                "length_km".to_string(),
                serde_json::json!((length_m / 1000.0 * 1000.0).round() / 1000.0),
            );
            props.insert(
                "length_miles".to_string(),
                serde_json::json!((length_m / 1609.344 * 1000.0).round() / 1000.0),
            );
        }

        // Track stats for numeric properties
        for (k, v) in &props {
            if let Some(n) = v.as_f64() {
                numeric_stats.entry(k.clone()).or_default().push(n);
            }
        }

        enriched_features.push(Feature {
            bbox: feature.bbox.clone(),
            geometry: feature.geometry.clone(),
            id: feature.id.clone(),
            properties: Some(props),
            foreign_members: feature.foreign_members.clone(),
        });
    }

    let mut stats_summary = Map::new();
    for (k, vals) in numeric_stats {
        if !vals.is_empty() {
            let count = vals.len();
            let sum: f64 = vals.iter().sum();
            let mean = sum / (count as f64);
            let min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

            stats_summary.insert(
                k,
                serde_json::json!({
                    "count": count,
                    "min": (min * 100.0).round() / 100.0,
                    "max": (max * 100.0).round() / 100.0,
                    "mean": (mean * 100.0).round() / 100.0,
                    "sum": (sum * 100.0).round() / 100.0
                }),
            );
        }
    }

    let summary = serde_json::json!({
        "feature_count": fc.features.len(),
        "total_area_sqm": (total_area_sqm * 100.0).round() / 100.0,
        "total_area_sqkm": (total_area_sqm / 1_000_000.0 * 1000.0).round() / 1000.0,
        "total_length_m": (total_length_m * 100.0).round() / 100.0,
        "total_length_km": (total_length_m / 1000.0 * 1000.0).round() / 1000.0,
        "attribute_statistics": stats_summary
    });

    let out_fc = FeatureCollection {
        bbox: fc.bbox.clone(),
        features: enriched_features,
        foreign_members: None,
    };

    Ok((out_fc, summary))
}

pub fn haversine_distance(p1: &[f64], p2: &[f64]) -> f64 {
    let lat1 = p1[1].to_radians();
    let lat2 = p2[1].to_radians();
    let dlat = (p2[1] - p1[1]).to_radians();
    let dlng = (p2[0] - p1[0]).to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c
}

pub fn linestring_geodesic_length(coords: &[Vec<f64>]) -> f64 {
    if coords.len() < 2 {
        return 0.0;
    }
    let mut len = 0.0;
    for i in 0..coords.len() - 1 {
        len += haversine_distance(&coords[i], &coords[i + 1]);
    }
    len
}

// Spherical polygon area based on Girard's theorem
pub fn polygon_spherical_area(rings: &[Vec<Vec<f64>>]) -> f64 {
    if rings.is_empty() {
        return 0.0;
    }

    let mut area = ring_spherical_area(&rings[0]);
    // Subtract holes
    for hole in &rings[1..] {
        area -= ring_spherical_area(hole);
    }
    area.max(0.0)
}

fn ring_spherical_area(ring: &[Vec<f64>]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }

    let mut total = 0.0;
    for i in 0..n {
        let p1 = &ring[i];
        let p2 = &ring[(i + 1) % n];

        let lambda1 = p1[0].to_radians();
        let phi1 = p1[1].to_radians();
        let lambda2 = p2[0].to_radians();
        let phi2 = p2[1].to_radians();

        let d_lambda = lambda2 - lambda1;
        total += d_lambda * (2.0 + phi1.sin() + phi2.sin());
    }

    total.abs() * EARTH_RADIUS_M * EARTH_RADIUS_M / 4.0
}
