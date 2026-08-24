use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::Value as JsonValue;

pub fn execute_spatial_query(
    source_fc: &FeatureCollection,
    filter_fc: Option<&FeatureCollection>,
    spatial_relation: &str, // "within_polygons", "none"
    attribute_field: Option<&str>,
    attribute_op: Option<&str>, // "=", "!=", ">", "<", ">=", "<=", "contains"
    attribute_val: Option<&str>,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    let mut matched_features = Vec::new();

    // Extract filter polygons if provided (each polygon is Vec<Vec<Vec<f64>>>: list of rings)
    let mut filter_polys: Vec<Vec<Vec<Vec<f64>>>> = Vec::new();
    if let Some(ffc) = filter_fc {
        for f in &ffc.features {
            if let Some(ref geom) = f.geometry {
                match &geom.value {
                    GeoValue::Polygon(rings) => filter_polys.push(rings.clone()),
                    GeoValue::MultiPolygon(polys) => {
                        for p in polys {
                            filter_polys.push(p.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    for feature in &source_fc.features {
        // 1. Check attribute filter
        let attr_match = if let (Some(field), Some(op), Some(val_str)) =
            (attribute_field, attribute_op, attribute_val)
        {
            if field.trim().is_empty() {
                true
            } else {
                check_attribute_match(feature, field, op, val_str)
            }
        } else {
            true
        };

        if !attr_match {
            continue;
        }

        // 2. Check spatial relation filter (applies whenever a mask is supplied)
        let spatial_match = if !filter_polys.is_empty() {
            if let Some(ref geom) = feature.geometry {
                geom_within_any_polygon(geom, &filter_polys)
            } else {
                false
            }
        } else {
            true
        };

        if spatial_match {
            matched_features.push(feature.clone());
        }
    }

    let out_fc = FeatureCollection {
        bbox: None,
        features: matched_features,
        foreign_members: None,
    };

    let input_count = source_fc.features.len();
    let matched_count = out_fc.features.len();
    let match_rate = if input_count > 0 {
        (matched_count as f64 / input_count as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    };
    let summary = serde_json::json!({
        "input_features": input_count,
        "matched_features": matched_count,
        "match_rate_percent": match_rate,
        "spatial_relation": spatial_relation,
        "attribute_condition": format!("{} {} {}", attribute_field.unwrap_or(""), attribute_op.unwrap_or(""), attribute_val.unwrap_or(""))
    });

    Ok((out_fc, summary))
}

fn check_attribute_match(f: &Feature, field: &str, op: &str, target_val: &str) -> bool {
    let props = match &f.properties {
        Some(p) => p,
        None => return false,
    };

    let actual_val = match props.get(field) {
        Some(v) => v,
        None => return false,
    };

    match actual_val {
        JsonValue::Number(n) => {
            if let (Some(num_actual), Ok(num_target)) = (n.as_f64(), target_val.parse::<f64>()) {
                match op {
                    "=" | "==" => (num_actual - num_target).abs() < 1e-9,
                    "!=" => (num_actual - num_target).abs() >= 1e-9,
                    ">" => num_actual > num_target,
                    ">=" => num_actual >= num_target,
                    "<" => num_actual < num_target,
                    "<=" => num_actual <= num_target,
                    _ => false,
                }
            } else {
                false
            }
        }
        JsonValue::String(s) => {
            let s_lower = s.to_lowercase();
            let t_lower = target_val.to_lowercase();
            match op {
                "=" | "==" => s_lower == t_lower,
                "!=" => s_lower != t_lower,
                "contains" => s_lower.contains(&t_lower),
                ">" => s_lower > t_lower,
                "<" => s_lower < t_lower,
                _ => false,
            }
        }
        JsonValue::Bool(b) => {
            let t_bool = target_val.eq_ignore_ascii_case("true") || target_val == "1";
            match op {
                "=" | "==" => *b == t_bool,
                "!=" => *b != t_bool,
                _ => false,
            }
        }
        _ => false,
    }
}

pub fn point_in_polygon_ring(pt: &[f64], ring: &[Vec<f64>]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let (x, y) = (pt[0], pt[1]);
    let mut inside = false;

    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i][0], ring[i][1]);
        let (xj, yj) = (ring[j][0], ring[j][1]);

        let intersect = ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi);
        if intersect {
            inside = !inside;
        }
        j = i;
    }

    inside
}

pub fn point_in_polygon(pt: &[f64], rings: &[Vec<Vec<f64>>]) -> bool {
    if rings.is_empty() {
        return false;
    }
    // Check outer ring
    if !point_in_polygon_ring(pt, &rings[0]) {
        return false;
    }
    // Check inner holes
    for hole in &rings[1..] {
        if point_in_polygon_ring(pt, hole) {
            return false; // Point is inside hole
        }
    }
    true
}

fn geom_within_any_polygon(geom: &Geometry, filter_polys: &[Vec<Vec<Vec<f64>>>]) -> bool {
    match &geom.value {
        GeoValue::Point(coords) => {
            for poly in filter_polys {
                if point_in_polygon(coords, poly) {
                    return true;
                }
            }
            false
        }
        GeoValue::MultiPoint(points) => {
            for pt in points {
                for poly in filter_polys {
                    if point_in_polygon(pt, poly) {
                        return true;
                    }
                }
            }
            false
        }
        GeoValue::LineString(coords) => {
            for pt in coords {
                for poly in filter_polys {
                    if point_in_polygon(pt, poly) {
                        return true;
                    }
                }
            }
            false
        }
        _ => true,
    }
}
