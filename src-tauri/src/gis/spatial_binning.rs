use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::Map;
use std::collections::HashMap;

/// Accumulated state per grid cell: point count and collected attributes.
type BinEntry = (usize, Vec<Map<String, serde_json::Value>>);

pub fn calculate_spatial_binning(
    fc: &FeatureCollection,
    grid_type: &str, // "square", "hex"
    cell_size_km: f64,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    let cell_size_km = if cell_size_km <= 0.0 {
        10.0
    } else {
        cell_size_km
    };

    // 1. Extract all points from dataset
    let mut points = Vec::new();
    for f in &fc.features {
        if let Some(ref geom) = f.geometry {
            match &geom.value {
                GeoValue::Point(c) => points.push((c[0], c[1], f.properties.clone())),
                GeoValue::MultiPoint(pts) => {
                    for p in pts {
                        points.push((p[0], p[1], f.properties.clone()));
                    }
                }
                _ => {
                    let mut coords = Vec::new();
                    super::convex_hull::extract_coords_from_geom(geom, &mut coords);
                    if let Some(first) = coords.first() {
                        points.push((first[0], first[1], f.properties.clone()));
                    }
                }
            }
        }
    }

    if points.is_empty() {
        return Err("No points available for binning".to_string());
    }

    // 2. Find bounding extent
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for (x, y, _) in &points {
        if *x < min_x {
            min_x = *x;
        }
        if *y < min_y {
            min_y = *y;
        }
        if *x > max_x {
            max_x = *x;
        }
        if *y > max_y {
            max_y = *y;
        }
    }

    let avg_lat = (min_y + max_y) / 2.0;
    let lat_rad = avg_lat.to_radians();
    let deg_per_km_lat = 1.0 / 111.32;
    let deg_per_km_lng = 1.0 / (111.32 * lat_rad.cos().abs().max(0.01));

    let dx = cell_size_km * deg_per_km_lng;
    let dy = cell_size_km * deg_per_km_lat;

    min_x -= dx;
    min_y -= dy;

    // Bin points into grid cells
    let mut grid_counts: HashMap<(i64, i64), BinEntry> = HashMap::new();

    for (x, y, props) in points {
        let ix = ((x - min_x) / dx).floor() as i64;
        let iy = ((y - min_y) / dy).floor() as i64;
        let entry = grid_counts
            .entry((ix, iy))
            .or_insert_with(|| (0, Vec::new()));
        entry.0 += 1;
        if let Some(p) = props {
            entry.1.push(p);
        }
    }

    let mut output_features = Vec::new();
    let mut max_count = 0;

    for ((ix, iy), (count, _)) in &grid_counts {
        if *count > max_count {
            max_count = *count;
        }

        let cell_min_x = min_x + (*ix as f64) * dx;
        let cell_min_y = min_y + (*iy as f64) * dy;
        let cell_max_x = cell_min_x + dx;
        let cell_max_y = cell_min_y + dy;

        let polygon_coords = if grid_type == "hex" {
            generate_hexagon(
                cell_min_x + dx * 0.5,
                cell_min_y + dy * 0.5,
                dx * 0.55,
                dy * 0.55,
            )
        } else {
            vec![
                vec![cell_min_x, cell_min_y],
                vec![cell_max_x, cell_min_y],
                vec![cell_max_x, cell_max_y],
                vec![cell_min_x, cell_max_y],
                vec![cell_min_x, cell_min_y],
            ]
        };

        let cell_area_sqkm = cell_size_km * cell_size_km;
        let density = (*count as f64) / cell_area_sqkm;

        let mut props = Map::new();
        props.insert("point_count".to_string(), serde_json::json!(count));
        props.insert(
            "density_per_sqkm".to_string(),
            serde_json::json!((density * 100.0).round() / 100.0),
        );
        props.insert("grid_x".to_string(), serde_json::json!(ix));
        props.insert("grid_y".to_string(), serde_json::json!(iy));
        props.insert("cell_size_km".to_string(), serde_json::json!(cell_size_km));

        output_features.push(Feature {
            bbox: Some(vec![cell_min_x, cell_min_y, cell_max_x, cell_max_y]),
            geometry: Some(Geometry::new(GeoValue::Polygon(vec![polygon_coords]))),
            id: None,
            properties: Some(props),
            foreign_members: None,
        });
    }

    let out_fc = FeatureCollection {
        bbox: None,
        features: output_features,
        foreign_members: None,
    };

    let summary = serde_json::json!({
        "grid_type": grid_type,
        "cell_size_km": cell_size_km,
        "total_bins_generated": out_fc.features.len(),
        "max_points_in_single_bin": max_count
    });

    Ok((out_fc, summary))
}

fn generate_hexagon(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<Vec<f64>> {
    let mut ring = Vec::with_capacity(7);
    for i in 0..6 {
        let angle = std::f64::consts::PI / 3.0 * (i as f64) + std::f64::consts::PI / 6.0;
        ring.push(vec![cx + rx * angle.cos(), cy + ry * angle.sin()]);
    }
    if let Some(first) = ring.first().cloned() {
        ring.push(first);
    }
    ring
}
