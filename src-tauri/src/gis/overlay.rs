//! Overlay geoprocessing: intersection, union, difference, clip and dissolve.
//!
//! Polygon boolean operations are delegated to `geo::BooleanOps`; features
//! keep their source attributes so downstream workflows stay lossless.

use geo::{BooleanOps, Coord, LineString as GeoLineString, MultiPolygon, Polygon as GeoPolygon};
use geojson::{Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::{Map, Value as JsonValue};

pub const OPERATIONS: [&str; 3] = ["intersection", "difference", "symmetric_difference"];

/// Pairwise overlay of two collections: every input feature is combined with
/// the unary union of the overlay layer's polygons.
pub fn run_overlay(
    fc: &FeatureCollection,
    overlay_fc: &FeatureCollection,
    operation: &str,
) -> Result<(FeatureCollection, JsonValue), String> {
    let op = match operation {
        "intersection" => Op::Intersection,
        "difference" => Op::Difference,
        "symmetric_difference" => Op::SymmetricDifference,
        other => return Err(format!("unknown overlay operation '{other}'")),
    };

    let mask = collect_polygons(overlay_fc)?;
    if mask.0.is_empty() {
        return Err("overlay layer contains no polygon geometry".into());
    }

    let mut out = Vec::new();
    for feature in &fc.features {
        let Some(geometry) = &feature.geometry else {
            continue;
        };
        let Some(subject) = to_multipolygon(geometry) else {
            continue;
        };

        let result = match op {
            Op::Intersection => subject.intersection(&mask),
            Op::Difference => subject.difference(&mask),
            Op::SymmetricDifference => subject
                .union(&mask)
                .difference(&subject.intersection(&mask)),
        };

        if result.0.is_empty() {
            continue;
        }

        let mut props = feature.properties.clone().unwrap_or_default();
        props.insert(
            "overlay_op".into(),
            JsonValue::String(operation.to_string()),
        );
        out.push(feature_with_multipolygon(&result, props));
    }

    let summary = serde_json::json!({
        "operation": operation,
        "input_features": fc.features.len(),
        "output_features": out.len(),
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

/// Clip: keep the parts of the source inside the (multi)polygon mask,
/// preserving source attributes.
pub fn run_clip(
    fc: &FeatureCollection,
    mask_fc: &FeatureCollection,
) -> Result<(FeatureCollection, JsonValue), String> {
    let mask = collect_polygons(mask_fc)?;
    if mask.0.is_empty() {
        return Err("clip boundary contains no polygon geometry".into());
    }

    let mut out = Vec::new();
    for feature in &fc.features {
        let Some(geometry) = &feature.geometry else {
            continue;
        };

        match geometry.value {
            // Polygons: true boolean intersection.
            GeoValue::Polygon(_) | GeoValue::MultiPolygon(_) => {
                let Some(subject) = to_multipolygon(geometry) else {
                    continue;
                };
                let clipped = subject.intersection(&mask);
                if clipped.0.is_empty() {
                    continue;
                }
                out.push(feature_with_multipolygon(
                    &clipped,
                    feature.properties.clone().unwrap_or_default(),
                ));
            }
            // Lines/points: containment filter on representative vertices.
            _ => {
                if geometry_intersects_mask(geometry, &mask) {
                    out.push(feature.clone());
                }
            }
        }
    }

    let summary = serde_json::json!({
        "operation": "clip",
        "input_features": fc.features.len(),
        "clipped_features": out.len(),
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

/// Union/Dissolve: merge all polygons into their minimal coverage. When a
/// group field is supplied, polygons sharing an attribute value are merged.
pub fn run_dissolve(
    fc: &FeatureCollection,
    group_field: Option<&str>,
) -> Result<(FeatureCollection, JsonValue), String> {
    let mut groups: Vec<(Option<JsonValue>, Vec<GeoPolygon<f64>>)> = Vec::new();

    for feature in &fc.features {
        let Some(geometry) = &feature.geometry else {
            continue;
        };
        let Some(mp) = to_multipolygon(geometry) else {
            continue;
        };

        let key = group_field.and_then(|field| {
            feature
                .properties
                .as_ref()
                .and_then(|p| p.get(field))
                .cloned()
        });

        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, polys)) => polys.extend(mp),
            None => groups.push((key, mp.0)),
        }
    }

    let mut out = Vec::new();
    for (key, polys) in groups {
        let mut acc = MultiPolygon(polys);
        if acc.0.is_empty() {
            continue;
        }
        // Fold pairwise unions until stable (n is typically small per group).
        let dissolved = fold_union(acc);
        acc = dissolved;

        let mut props = Map::new();
        if let (Some(field), Some(value)) = (group_field, key) {
            props.insert(field.to_string(), value);
        }
        props.insert(
            "dissolved_parts".into(),
            JsonValue::from(folded_part_count(&acc)),
        );
        out.push(feature_with_multipolygon(&acc, props));
    }

    if out.is_empty() {
        return Err("no polygon geometry available to dissolve".into());
    }

    let summary = serde_json::json!({
        "group_field": group_field,
        "input_features": fc.features.len(),
        "output_groups": out.len(),
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

enum Op {
    Intersection,
    Difference,
    SymmetricDifference,
}

fn fold_union(mut mp: MultiPolygon<f64>) -> MultiPolygon<f64> {
    loop {
        let before = mp.0.len();
        let mut changed = false;
        'outer: for i in 0..mp.0.len() {
            for j in (i + 1)..mp.0.len() {
                let a = GeoPolygon::new(mp[i].exterior().clone(), mp[i].interiors().to_vec());
                let b = GeoPolygon::new(mp[j].exterior().clone(), mp[j].interiors().to_vec());
                if bbox_overlaps_polygon(&a, &b) {
                    let merged = MultiPolygon::new(vec![a]).union(&MultiPolygon::new(vec![b]));
                    let mut next: Vec<GeoPolygon<f64>> = (0..mp.0.len())
                        .filter(|k| *k != i && *k != j)
                        .map(|k| mp[k].clone())
                        .collect();
                    next.extend(merged);
                    mp = MultiPolygon::new(next);
                    changed = true;
                    break 'outer;
                }
            }
        }
        if !changed || mp.0.len() >= before {
            break;
        }
    }
    mp
}

fn folded_part_count(mp: &MultiPolygon<f64>) -> usize {
    mp.0.len()
}

fn bbox_overlaps_polygon(a: &GeoPolygon<f64>, b: &GeoPolygon<f64>) -> bool {
    fn extent(p: &GeoPolygon<f64>) -> Option<(f64, f64, f64, f64)> {
        let coords = p.exterior().0.as_slice();
        coords
            .iter()
            .fold(None, |acc: Option<(f64, f64, f64, f64)>, c: &Coord<f64>| {
                Some(match acc {
                    None => (c.x, c.y, c.x, c.y),
                    Some((minx, miny, maxx, maxy)) => {
                        (minx.min(c.x), miny.min(c.y), maxx.max(c.x), maxy.max(c.y))
                    }
                })
            })
    }
    match (extent(a), extent(b)) {
        (Some((ax0, ay0, ax1, ay1)), Some((bx0, by0, bx1, by1))) => {
            !(ax1 < bx0 || bx1 < ax0 || ay1 < by0 || by1 < ay0)
        }
        _ => false,
    }
}

/// Collect all polygons of a collection into one MultiPolygon (no merging).
fn collect_polygons(fc: &FeatureCollection) -> Result<MultiPolygon<f64>, String> {
    let mut polys = Vec::new();
    for feature in &fc.features {
        if let Some(g) = &feature.geometry {
            if let Some(mp) = to_multipolygon(g) {
                polys.extend(mp);
            }
        }
    }
    Ok(MultiPolygon::new(polys))
}

fn to_multipolygon(g: &Geometry) -> Option<MultiPolygon<f64>> {
    match &g.value {
        GeoValue::Polygon(rings) => Some(MultiPolygon::new(vec![rings_to_geo(rings)])),
        GeoValue::MultiPolygon(all) => Some(MultiPolygon::new(
            all.iter()
                .filter_map(|rings| {
                    rings.first().map(|outer| {
                        GeoPolygon::new(
                            GeoLineString::from(
                                outer
                                    .iter()
                                    .map(|c| Coord { x: c[0], y: c[1] })
                                    .collect::<Vec<_>>(),
                            ),
                            interior_lines(&rings[1..]),
                        )
                    })
                })
                .collect::<Vec<_>>(),
        )),
        _ => None,
    }
}

fn rings_to_geo(rings: &[Vec<Vec<f64>>]) -> GeoPolygon<f64> {
    let outer = rings
        .first()
        .map(|ring| {
            GeoLineString::from(
                ring.iter()
                    .map(|c| Coord { x: c[0], y: c[1] })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_else(|| GeoLineString::from(Vec::<Coord<f64>>::new()));
    let interiors = if rings.len() > 1 {
        interior_lines(&rings[1..])
    } else {
        Vec::new()
    };
    GeoPolygon::new(outer, interiors)
}

fn interior_lines(rings: &[Vec<Vec<f64>>]) -> Vec<GeoLineString<f64>> {
    rings
        .iter()
        .map(|ring| {
            GeoLineString::from(
                ring.iter()
                    .map(|c| Coord { x: c[0], y: c[1] })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn feature_with_multipolygon(mp: &MultiPolygon<f64>, props: Map<String, JsonValue>) -> Feature {
    let rings: Vec<Vec<Vec<Vec<f64>>>> = mp
        .iter()
        .map(|p| {
            let mut r = vec![p
                .exterior()
                .0
                .iter()
                .map(|c| vec![c.x, c.y])
                .collect::<Vec<_>>()];
            for inner in p.interiors() {
                r.push(inner.0.iter().map(|c| vec![c.x, c.y]).collect());
            }
            r
        })
        .collect();

    Feature {
        bbox: None,
        geometry: Some(Geometry::new(GeoValue::MultiPolygon(rings))),
        id: None,
        properties: Some(props),
        foreign_members: None,
    }
}

/// Approximate containment test for non-polygon geometries: any representative
/// vertex inside any mask polygon counts as intersecting.
fn geometry_intersects_mask(g: &Geometry, mask: &MultiPolygon<f64>) -> bool {
    fn point_in(polygon: &GeoPolygon<f64>, x: f64, y: f64) -> bool {
        use geo::{Contains, Point as GP};
        polygon.contains(&GP::new(x, y))
    }

    let vertices: Vec<(f64, f64)> = match &g.value {
        GeoValue::Point(c) => vec![(c[0], c[1])],
        GeoValue::MultiPoint(pts) => pts.iter().map(|c| (c[0], c[1])).collect(),
        GeoValue::LineString(l) => l.iter().map(|c| (c[0], c[1])).collect(),
        GeoValue::MultiLineString(ls) => ls.iter().flatten().map(|c| (c[0], c[1])).collect(),
        GeoValue::GeometryCollection(gs) => {
            return gs.iter().any(|inner| geometry_intersects_mask(inner, mask))
        }
        _ => return false,
    };

    vertices
        .iter()
        .any(|(x, y)| mask.iter().any(|poly| point_in(poly, *x, *y)))
}
