use geo::{ConvexHull, Coord, MultiPoint, Polygon};
use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::Map;

pub fn calculate_convex_hull(
    fc: &FeatureCollection,
    per_feature: bool,
) -> Result<(FeatureCollection, serde_json::Value), String> {
    if fc.features.is_empty() {
        return Err("No features in input dataset".to_string());
    }

    let mut output_features = Vec::new();

    if per_feature {
        for (i, f) in fc.features.iter().enumerate() {
            let coords = extract_coords_from_feature(f);
            if coords.len() < 3 {
                continue;
            }
            let geo_coords: Vec<Coord<f64>> = coords
                .into_iter()
                .map(|c| Coord { x: c[0], y: c[1] })
                .collect();
            let mp = MultiPoint::new(geo_coords.into_iter().map(geo::Point::from).collect());
            let hull: Polygon<f64> = mp.convex_hull();

            let ring: Vec<Vec<f64>> = hull.exterior().0.iter().map(|c| vec![c.x, c.y]).collect();
            let mut props = f.properties.clone().unwrap_or_else(Map::new);
            props.insert(
                "hull_type".to_string(),
                serde_json::json!("feature_convex_hull"),
            );
            props.insert("feature_index".to_string(), serde_json::json!(i));

            output_features.push(Feature {
                bbox: None,
                geometry: Some(Geometry::new(GeoValue::Polygon(vec![ring]))),
                id: f.id.clone(),
                properties: Some(props),
                foreign_members: None,
            });
        }
    } else {
        let mut all_coords = Vec::new();
        for f in &fc.features {
            all_coords.extend(extract_coords_from_feature(f));
        }

        if all_coords.len() < 3 {
            return Err(
                "At least 3 distinct coordinates are required to form a convex hull".to_string(),
            );
        }

        let geo_coords: Vec<Coord<f64>> = all_coords
            .into_iter()
            .map(|c| Coord { x: c[0], y: c[1] })
            .collect();
        let mp = MultiPoint::new(geo_coords.into_iter().map(geo::Point::from).collect());
        let hull: Polygon<f64> = mp.convex_hull();

        let ring: Vec<Vec<f64>> = hull.exterior().0.iter().map(|c| vec![c.x, c.y]).collect();
        let mut props = Map::new();
        props.insert("name".to_string(), serde_json::json!("Layer Convex Hull"));
        props.insert(
            "hull_type".to_string(),
            serde_json::json!("layer_convex_hull"),
        );
        props.insert(
            "source_features_count".to_string(),
            serde_json::json!(fc.features.len()),
        );
        props.insert(
            "hull_vertices_count".to_string(),
            serde_json::json!(ring.len()),
        );

        output_features.push(Feature {
            bbox: None,
            geometry: Some(Geometry::new(GeoValue::Polygon(vec![ring]))),
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
        "input_features": fc.features.len(),
        "output_hulls": out_fc.features.len(),
        "mode": if per_feature { "per_feature" } else { "unified_layer" }
    });

    Ok((out_fc, summary))
}

pub fn extract_coords_from_feature(f: &Feature) -> Vec<Vec<f64>> {
    let mut coords = Vec::new();
    if let Some(ref geom) = f.geometry {
        extract_coords_from_geom(geom, &mut coords);
    }
    coords
}

pub fn extract_coords_from_geom(geom: &Geometry, coords: &mut Vec<Vec<f64>>) {
    match &geom.value {
        GeoValue::Point(c) => coords.push(c.clone()),
        GeoValue::MultiPoint(pts) => coords.extend(pts.clone()),
        GeoValue::LineString(pts) => coords.extend(pts.clone()),
        GeoValue::MultiLineString(lines) => {
            for l in lines {
                coords.extend(l.clone());
            }
        }
        GeoValue::Polygon(rings) => {
            for r in rings {
                coords.extend(r.clone());
            }
        }
        GeoValue::MultiPolygon(polys) => {
            for p in polys {
                for r in p {
                    coords.extend(r.clone());
                }
            }
        }
        GeoValue::GeometryCollection(geoms) => {
            for g in geoms {
                extract_coords_from_geom(g, coords);
            }
        }
    }
}
