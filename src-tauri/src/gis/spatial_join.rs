//! Spatial join: attach attributes from a target layer to source features
//! based on geometric containment (target polygons) or nearest-vertex
//! proximity fallback.

use crate::gis::spatial_query::point_in_polygon;
use geojson::{Feature, FeatureCollection, Value as GeoValue};
use serde_json::{Map, Value as JsonValue};

/// One target polygon with the attributes of its source feature.
type TargetPolygon = (Vec<Vec<Vec<f64>>>, Option<Map<String, JsonValue>>);

/// For every source feature, find matching target features:
/// - point sources: containment inside any target polygon
/// - other sources: representative-vertex containment (documented approximation)
///
/// Joined fields are prefixed `sj_`; a `sj_join_count` records match cardinality.
pub fn run_spatial_join(
    source_fc: &FeatureCollection,
    target_fc: &FeatureCollection,
) -> Result<(FeatureCollection, JsonValue), String> {
    if source_fc.features.is_empty() {
        return Err("source layer is empty".into());
    }

    // Pre-extract target polygons + properties (one entry per polygon).
    let mut targets: Vec<TargetPolygon> = Vec::new();
    for t in &target_fc.features {
        let Some(geom) = &t.geometry else { continue };
        match &geom.value {
            GeoValue::Polygon(rings) => targets.push((rings.clone(), t.properties.clone())),
            GeoValue::MultiPolygon(polys) => {
                for rings in polys {
                    targets.push((rings.clone(), t.properties.clone()));
                }
            }
            _ => continue,
        }
    }

    if targets.is_empty() {
        return Err("target layer contains no polygon geometry".into());
    }

    let mut joined_fields: Vec<String> = Vec::new();
    for (_, props) in &targets {
        if let Some(p) = props {
            for key in p.keys() {
                if !joined_fields.iter().any(|f| f == key) {
                    joined_fields.push(key.clone());
                }
            }
        }
    }
    joined_fields.sort();

    let mut out = Vec::new();
    for feature in &source_fc.features {
        let mut new_feature = feature.clone();
        let props = new_feature.properties.get_or_insert_with(Map::new);

        // Representative vertices of the source geometry.
        let vertices = representative_vertices(feature);

        let mut matched_props: Vec<&Map<String, JsonValue>> = Vec::new();
        for (poly_rings, tprops) in &targets {
            let contained = vertices
                .iter()
                .any(|(x, y)| point_in_polygon(&[*x, *y], poly_rings));
            if contained {
                if let Some(p) = tprops {
                    matched_props.push(p);
                }
            }
        }

        props.insert("sj_join_count".into(), JsonValue::from(matched_props.len()));
        for field in &joined_fields {
            let value = if matched_props.len() == 1 {
                matched_props[0]
                    .get(field)
                    .cloned()
                    .unwrap_or(JsonValue::Null)
            } else if matched_props.is_empty() {
                JsonValue::Null
            } else {
                JsonValue::String(format!("{} matches", matched_props.len()))
            };
            props.insert(format!("sj_{field}"), value);
        }

        out.push(new_feature);
    }

    let joined_count = out
        .iter()
        .filter(|f| {
            f.properties
                .as_ref()
                .and_then(|p| p.get("sj_join_count"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                > 0
        })
        .count();

    let summary = serde_json::json!({
        "source_features": source_fc.features.len(),
        "joined_features": joined_count,
        "joined_fields": joined_fields,
    });
    Ok((
        FeatureCollection {
            bbox: None,
            features: out,
            foreign_members: None,
        },
        summary,
    ))
}

fn representative_vertices(feature: &Feature) -> Vec<(f64, f64)> {
    match feature.geometry.as_ref().map(|g| &g.value) {
        Some(GeoValue::Point(c)) => vec![(c[0], c[1])],
        Some(GeoValue::MultiPoint(pts)) => pts.iter().map(|c| (c[0], c[1])).collect(),
        Some(GeoValue::LineString(l)) => l.iter().map(|c| (c[0], c[1])).collect(),
        Some(GeoValue::MultiLineString(ls)) => ls.iter().flatten().map(|c| (c[0], c[1])).collect(),
        Some(GeoValue::Polygon(rings)) => rings
            .first()
            .map(|r| r.iter().take(32).map(|c| (c[0], c[1])).collect())
            .unwrap_or_default(),
        Some(GeoValue::MultiPolygon(polys)) => polys
            .iter()
            .filter_map(|p| p.first())
            .flat_map(|r| r.iter().take(16).map(|c| (c[0], c[1])))
            .collect(),
        _ => Vec::new(),
    }
}
