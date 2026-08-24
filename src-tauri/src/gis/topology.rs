//! Topology rules engine: data-integrity validation with actionable
//! violations (ArcGIS topology parity, simplified to pairwise checks).
//!
//! Rules:
//! - `must_not_overlap` (polygon): intersection area > 0 flags a violation.
//! - `must_not_have_dangles` (line): endpoints not shared with any other
//!   endpoint are flagged (trim/extend candidates).
//! - `must_be_covered_by` (point/line in polygons): features whose
//!   representative vertices fall outside every polygon are flagged.

use geo::{
    BooleanOps, Contains, Coord, LineString as GeoLine, Point as GeoPoint, Polygon as GeoPolygon,
};
use geojson::{Feature, FeatureCollection, Value as GeoValue};
use serde_json::{json, Map, Value as JsonValue};

fn to_geo_polygon(rings: &[Vec<Vec<f64>>]) -> GeoPolygon<f64> {
    let outer = rings
        .first()
        .map(|r| {
            GeoLine::from(
                r.iter()
                    .map(|c| Coord { x: c[0], y: c[1] })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_else(|| GeoLine::from(Vec::<Coord<f64>>::new()));
    let interiors: Vec<GeoLine<f64>> = rings[1..]
        .iter()
        .map(|r| {
            GeoLine::from(
                r.iter()
                    .map(|c| Coord { x: c[0], y: c[1] })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    GeoPolygon::new(outer, interiors)
}

fn polygons_of(fc: &FeatureCollection) -> Vec<(usize, GeoPolygon<f64>)> {
    let mut out = Vec::new();
    for (idx, f) in fc.features.iter().enumerate() {
        let Some(g) = &f.geometry else { continue };
        match &g.value {
            GeoValue::Polygon(rings) => out.push((idx, to_geo_polygon(rings))),
            GeoValue::MultiPolygon(polys) => {
                for rings in polys {
                    out.push((idx, to_geo_polygon(rings)));
                }
            }
            _ => {}
        }
    }
    out
}

fn suggested_fix(kind: &str) -> &'static str {
    match kind {
        "must_not_overlap" => "merge intersecting polygons or subtract the overlap",
        "must_not_have_dangles" => {
            "trim the dangling segment or snap the endpoint to the nearest edge"
        }
        "must_be_covered_by" => {
            "move the feature inside the covering polygon or extend the boundary"
        }
        _ => "inspect manually",
    }
}

fn violation(
    kind: &str,
    feature_idx: usize,
    detail: JsonValue,
    geometry: Option<GeoValue>,
) -> Feature {
    let mut props = Map::new();
    props.insert("violation".into(), json!(kind));
    props.insert("feature_index".into(), json!(feature_idx));
    props.insert("detail".into(), detail);
    props.insert("suggested_fix".into(), json!(suggested_fix(kind)));
    Feature {
        bbox: None,
        geometry: geometry.map(geojson::Geometry::new),
        id: None,
        properties: Some(props),
        foreign_members: None,
    }
}

/// Validate a collection against a rule; `cover_fc` is the covering layer
/// for `must_be_covered_by`. Returns violation features + a summary.
pub fn validate_topology(
    fc: &FeatureCollection,
    rule: &str,
    cover_fc: Option<&FeatureCollection>,
) -> Result<(FeatureCollection, JsonValue), String> {
    match rule {
        "must_not_overlap" => check_overlaps(fc),
        "must_not_have_dangles" => check_dangles(fc),
        "must_be_covered_by" => {
            let cover = cover_fc.ok_or("must_be_covered_by requires a covering polygon layer")?;
            check_covered_by(fc, cover)
        }
        other => Err(format!("unknown topology rule '{other}'")),
    }
}

fn check_overlaps(fc: &FeatureCollection) -> Result<(FeatureCollection, JsonValue), String> {
    let polys = polygons_of(fc);
    if polys.len() < 2 {
        return Err("must_not_overlap needs at least two polygons".into());
    }
    let mut violations = Vec::new();
    for i in 0..polys.len() {
        for j in (i + 1)..polys.len() {
            let intersection = polys[i].1.intersection(&polys[j].1);
            let area_m2: f64 = intersection
                .iter()
                .map(|p| ring_area_deg2(p.exterior()) * 111_320.0 * 111_320.0)
                .sum();
            if area_m2 > 1.0 {
                violations.push(violation(
                    "must_not_overlap",
                    polys[i].0,
                    json!({
                        "with_feature_index": polys[j].0,
                        "overlap_area_m2": (area_m2 * 100.0).round() / 100.0
                    }),
                    rings_of(&intersection).into_iter().next(),
                ));
            }
        }
    }
    let count = violations.len();
    Ok((
        FeatureCollection {
            bbox: None,
            features: violations,
            foreign_members: None,
        },
        json!({ "rule": "must_not_overlap", "polygons_checked": polys.len(), "violations": count, "valid": count == 0 }),
    ))
}

/// Convert a geo MultiPolygon to GeoJSON polygon rings.
fn rings_of(mp: &geo::MultiPolygon<f64>) -> Vec<geojson::Value> {
    mp.iter()
        .map(|p| {
            let mut rings = vec![p
                .exterior()
                .0
                .iter()
                .map(|c| vec![c.x, c.y])
                .collect::<Vec<_>>()];
            for inner in p.interiors() {
                rings.push(inner.0.iter().map(|c| vec![c.x, c.y]).collect());
            }
            GeoValue::Polygon(rings)
        })
        .collect()
}

/// Shoelace area in square degrees (absolute).
fn ring_area_deg2(ring: &GeoLine<f64>) -> f64 {
    let pts: Vec<Coord<f64>> = ring.0.clone();
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = &pts[i];
        let b = &pts[(i + 1) % n];
        sum += a.x * b.y - b.x * a.y;
    }
    (sum / 2.0).abs()
}

fn check_dangles(fc: &FeatureCollection) -> Result<(FeatureCollection, JsonValue), String> {
    type EndpointKey = (i64, i64);
    type EndpointRecord = (EndpointKey, (f64, f64), usize);
    let mut endpoint_counts: std::collections::HashMap<EndpointKey, usize> =
        std::collections::HashMap::new();
    let mut endpoints: Vec<EndpointRecord> = Vec::new();
    for (idx, f) in fc.features.iter().enumerate() {
        let Some(g) = &f.geometry else { continue };
        let lines: Vec<&Vec<Vec<f64>>> = match &g.value {
            GeoValue::LineString(ls) => vec![ls],
            GeoValue::MultiLineString(lss) => lss.iter().collect(),
            _ => continue,
        };
        for ls in lines {
            if ls.len() < 2 {
                continue;
            }
            for end in [ls.first().unwrap(), ls.last().unwrap()] {
                let key = ((end[0] * 1e7).round() as i64, (end[1] * 1e7).round() as i64);
                *endpoint_counts.entry(key).or_insert(0) += 1;
                endpoints.push((key, (end[0], end[1]), idx));
            }
        }
    }
    if endpoints.is_empty() {
        return Err("no line geometry to inspect".into());
    }

    let mut violations = Vec::new();
    let mut reported: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (i, (key, (lng, lat), idx)) in endpoints.iter().enumerate() {
        if endpoint_counts[key] > 1 || !reported.insert(i) {
            continue;
        }
        violations.push(violation(
            "must_not_have_dangles",
            *idx,
            json!({ "endpoint": [lng, lat] }),
            Some(GeoValue::Point(vec![*lng, *lat])),
        ));
    }
    let count = violations.len();
    Ok((
        FeatureCollection {
            bbox: None,
            features: violations,
            foreign_members: None,
        },
        json!({ "rule": "must_not_have_dangles", "endpoints_checked": endpoints.len(), "violations": count, "valid": count == 0 }),
    ))
}

fn check_covered_by(
    fc: &FeatureCollection,
    cover: &FeatureCollection,
) -> Result<(FeatureCollection, JsonValue), String> {
    let cover_polys = polygons_of(cover);
    if cover_polys.is_empty() {
        return Err("covering layer contains no polygon geometry".into());
    }
    let mut violations = Vec::new();
    for (idx, f) in fc.features.iter().enumerate() {
        let Some(g) = &f.geometry else { continue };
        let mut pts: Vec<Vec<f64>> = Vec::new();
        crate::gis::spatial_statistics::collect_points_pub(&g.value, &mut pts);
        if pts.is_empty() {
            continue;
        }
        // Representative points: first vertex, midpoint, last vertex.
        let probes = [
            pts[0].clone(),
            pts[pts.len() / 2].clone(),
            pts[pts.len() - 1].clone(),
        ];
        let covered = probes.iter().any(|p| {
            let point = GeoPoint::new(p[0], p[1]);
            cover_polys.iter().any(|(_, poly)| poly.contains(&point))
        });
        if !covered {
            violations.push(violation(
                "must_be_covered_by",
                idx,
                json!({ "representative_point": probes[0] }),
                Some(GeoValue::Point(probes[0].clone())),
            ));
        }
    }
    let count = violations.len();
    Ok((
        FeatureCollection {
            bbox: None,
            features: violations,
            foreign_members: None,
        },
        json!({ "rule": "must_be_covered_by", "features_checked": fc.features.len(), "violations": count, "valid": count == 0 }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fc_from(geoms: Vec<GeoValue>) -> FeatureCollection {
        FeatureCollection {
            bbox: None,
            features: geoms
                .into_iter()
                .map(|g| Feature {
                    bbox: None,
                    geometry: Some(geojson::Geometry::new(g)),
                    id: None,
                    properties: None,
                    foreign_members: None,
                })
                .collect(),
            foreign_members: None,
        }
    }

    #[test]
    fn test_overlap_detection() {
        let a = GeoValue::Polygon(vec![vec![
            vec![0.0, 0.0],
            vec![2.0, 0.0],
            vec![2.0, 2.0],
            vec![0.0, 2.0],
            vec![0.0, 0.0],
        ]]);
        let b = GeoValue::Polygon(vec![vec![
            vec![1.0, 1.0],
            vec![3.0, 1.0],
            vec![3.0, 3.0],
            vec![1.0, 3.0],
            vec![1.0, 1.0],
        ]]);
        let fc = fc_from(vec![a, b]);
        let (out, summary) = validate_topology(&fc, "must_not_overlap", None).unwrap();
        assert_eq!(summary["violations"], 1);
        assert_eq!(
            out.features[0].properties.as_ref().unwrap()["detail"]["with_feature_index"],
            1
        );
    }

    #[test]
    fn test_no_overlap_when_adjacent() {
        let a = GeoValue::Polygon(vec![vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 0.0],
        ]]);
        let b = GeoValue::Polygon(vec![vec![
            vec![1.0, 0.0],
            vec![2.0, 0.0],
            vec![2.0, 1.0],
            vec![1.0, 1.0],
            vec![1.0, 0.0],
        ]]);
        let (_, summary) =
            validate_topology(&fc_from(vec![a, b]), "must_not_overlap", None).unwrap();
        assert_eq!(summary["violations"], 0);
        assert_eq!(summary["valid"], true);
    }

    #[test]
    fn test_dangle_detection() {
        // Two connected segments share (1,0); the third dangles at (3,3).
        let l1 = GeoValue::LineString(vec![vec![0.0, 0.0], vec![1.0, 0.0]]);
        let l2 = GeoValue::LineString(vec![vec![1.0, 0.0], vec![2.0, 0.0]]);
        let l3 = GeoValue::LineString(vec![vec![2.0, 0.0], vec![3.0, 3.0]]);
        let (out, summary) =
            validate_topology(&fc_from(vec![l1, l2, l3]), "must_not_have_dangles", None).unwrap();
        // Both free ends dangle: (0,0) on feature 0 and (3,3) on feature 2.
        assert_eq!(summary["violations"], 2);
        assert_eq!(
            out.features[1].properties.as_ref().unwrap()["feature_index"],
            2
        );
    }

    #[test]
    fn test_covered_by() {
        let cover = GeoValue::Polygon(vec![vec![
            vec![0.0, 0.0],
            vec![4.0, 0.0],
            vec![4.0, 4.0],
            vec![0.0, 4.0],
            vec![0.0, 0.0],
        ]]);
        let inside = GeoValue::Point(vec![1.0, 1.0]);
        let outside = GeoValue::Point(vec![9.0, 9.0]);
        let (out, summary) = validate_topology(
            &fc_from(vec![inside, outside]),
            "must_be_covered_by",
            Some(&fc_from(vec![cover])),
        )
        .unwrap();
        assert_eq!(summary["violations"], 1);
        assert_eq!(
            out.features[0].properties.as_ref().unwrap()["feature_index"],
            1
        );
        assert!(out.features[0].properties.as_ref().unwrap()["suggested_fix"].is_string());
    }
}
