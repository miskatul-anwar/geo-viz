use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::Map;

pub fn calculate_buffer(
    fc: &FeatureCollection,
    distance_meters: f64,
    steps: usize,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    if distance_meters <= 0.0 {
        return Err("Buffer distance must be strictly positive (> 0)".to_string());
    }
    let steps = if steps < 8 { 16 } else { steps };
    let mut buffered_features = Vec::new();

    // 1 degree latitude is approx 111,320 meters
    let meters_per_deg_lat = 111320.0;

    for feature in &fc.features {
        if let Some(ref geom) = feature.geometry {
            let buffered_geom = match &geom.value {
                GeoValue::Point(coords) => {
                    let poly = buffer_point(
                        coords[0],
                        coords[1],
                        distance_meters,
                        steps,
                        meters_per_deg_lat,
                    );
                    Some(Geometry::new(GeoValue::Polygon(poly)))
                }
                GeoValue::MultiPoint(points) => {
                    let mut polys = Vec::new();
                    for pt in points {
                        polys.push(buffer_point(
                            pt[0],
                            pt[1],
                            distance_meters,
                            steps,
                            meters_per_deg_lat,
                        ));
                    }
                    Some(Geometry::new(GeoValue::MultiPolygon(polys)))
                }
                GeoValue::LineString(coords) => {
                    let poly =
                        buffer_linestring(coords, distance_meters, steps, meters_per_deg_lat);
                    Some(Geometry::new(GeoValue::Polygon(poly)))
                }
                GeoValue::MultiLineString(lines) => {
                    let mut polys = Vec::new();
                    for line in lines {
                        polys.push(buffer_linestring(
                            line,
                            distance_meters,
                            steps,
                            meters_per_deg_lat,
                        ));
                    }
                    Some(Geometry::new(GeoValue::MultiPolygon(polys)))
                }
                GeoValue::Polygon(rings) => {
                    let poly =
                        buffer_polygon_ring(rings, distance_meters, steps, meters_per_deg_lat);
                    Some(Geometry::new(GeoValue::Polygon(poly)))
                }
                GeoValue::MultiPolygon(polys) => {
                    let mut new_polys = Vec::new();
                    for poly in polys {
                        new_polys.push(buffer_polygon_ring(
                            poly,
                            distance_meters,
                            steps,
                            meters_per_deg_lat,
                        ));
                    }
                    Some(Geometry::new(GeoValue::MultiPolygon(new_polys)))
                }
                _ => None,
            };

            if let Some(bg) = buffered_geom {
                let mut props = feature.properties.clone().unwrap_or_else(Map::new);
                props.insert(
                    "buffer_distance_m".to_string(),
                    serde_json::json!(distance_meters),
                );
                props.insert(
                    "buffer_type".to_string(),
                    serde_json::json!("geodesic_buffer"),
                );

                buffered_features.push(Feature {
                    bbox: None,
                    geometry: Some(bg),
                    id: feature.id.clone(),
                    properties: Some(props),
                    foreign_members: None,
                });
            }
        }
    }

    let out_fc = FeatureCollection {
        bbox: None,
        features: buffered_features,
        foreign_members: None,
    };

    let output_area_sqkm = crate::gis::metrics::collection_spherical_area_sqkm(&out_fc);
    let summary = serde_json::json!({
        "input_features": fc.features.len(),
        "buffered_features": out_fc.features.len(),
        "buffer_radius_meters": distance_meters,
        "buffer_radius_km": distance_meters / 1000.0,
        "segments": steps,
        "total_buffered_area_sqkm": output_area_sqkm
    });

    Ok((out_fc, summary))
}

fn buffer_point(
    lng: f64,
    lat: f64,
    distance_meters: f64,
    steps: usize,
    meters_per_deg_lat: f64,
) -> Vec<Vec<Vec<f64>>> {
    let lat_rad = lat.to_radians();
    let meters_per_deg_lng = meters_per_deg_lat * lat_rad.cos().abs().max(0.001);

    let d_lat = distance_meters / meters_per_deg_lat;
    let d_lng = distance_meters / meters_per_deg_lng;

    let mut ring = Vec::with_capacity(steps + 1);
    for i in 0..steps {
        let angle = 2.0 * std::f64::consts::PI * (i as f64) / (steps as f64);
        let cur_lng = lng + d_lng * angle.cos();
        let cur_lat = (lat + d_lat * angle.sin()).clamp(-89.9, 89.9);
        ring.push(vec![cur_lng, cur_lat]);
    }
    // Close the ring
    if let Some(first) = ring.first().cloned() {
        ring.push(first);
    }

    vec![ring]
}

fn buffer_linestring(
    coords: &[Vec<f64>],
    distance_meters: f64,
    steps: usize,
    meters_per_deg_lat: f64,
) -> Vec<Vec<Vec<f64>>> {
    if coords.len() < 2 {
        if let Some(first) = coords.first() {
            return buffer_point(
                first[0],
                first[1],
                distance_meters,
                steps,
                meters_per_deg_lat,
            );
        }
        return vec![];
    }

    let mut left_side = Vec::new();
    let mut right_side = Vec::new();

    for i in 0..coords.len() {
        let (p_prev, p_curr, p_next) = if i == 0 {
            (&coords[0], &coords[0], &coords[1])
        } else if i == coords.len() - 1 {
            (&coords[i - 1], &coords[i], &coords[i])
        } else {
            (&coords[i - 1], &coords[i], &coords[i + 1])
        };

        let lat = p_curr[1];
        let lat_rad = lat.to_radians();
        let meters_per_deg_lng = meters_per_deg_lat * lat_rad.cos().abs().max(0.001);

        let dx = (p_next[0] - p_prev[0]) * meters_per_deg_lng;
        let dy = (p_next[1] - p_prev[1]) * meters_per_deg_lat;
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);

        // Normal vector (-dy, dx)
        let nx = -dy / len;
        let ny = dx / len;

        let off_lng = (nx * distance_meters) / meters_per_deg_lng;
        let off_lat = (ny * distance_meters) / meters_per_deg_lat;

        left_side.push(vec![
            p_curr[0] + off_lng,
            (p_curr[1] + off_lat).clamp(-89.9, 89.9),
        ]);
        right_side.push(vec![
            p_curr[0] - off_lng,
            (p_curr[1] - off_lat).clamp(-89.9, 89.9),
        ]);
    }

    // Build ring: left forward, right backwards, close
    right_side.reverse();
    let mut ring = left_side;
    ring.extend(right_side);
    if let Some(first) = ring.first().cloned() {
        ring.push(first);
    }

    vec![ring]
}

fn buffer_polygon_ring(
    rings: &[Vec<Vec<f64>>],
    distance_meters: f64,
    steps: usize,
    meters_per_deg_lat: f64,
) -> Vec<Vec<Vec<f64>>> {
    if rings.is_empty() {
        return vec![];
    }
    let outer_ring = &rings[0];
    buffer_linestring(outer_ring, distance_meters, steps, meters_per_deg_lat)
}
