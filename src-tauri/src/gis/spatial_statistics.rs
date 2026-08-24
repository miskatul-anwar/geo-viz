//! Spatial Statistics toolbox: measuring geographic distributions and
//! pattern analysis (ArcGIS Spatial Statistics parity, offline).
//!
//! All statistics operate on feature centroids (lon/lat) with optional
//! numeric weights/fields; distances use haversine meters.

use crate::gis::metrics::haversine_distance;
use geojson::{Feature, FeatureCollection, Value as GeoValue};
use serde_json::{json, Map, Value as JsonValue};

/// (lng, lat) centroid of a feature's geometry.
fn centroid_of(feature: &Feature) -> Option<(f64, f64)> {
    let geom = feature.geometry.as_ref()?;
    let mut pts = Vec::new();
    collect_points(&geom.value, &mut pts);
    if pts.is_empty() {
        return None;
    }
    let n = pts.len() as f64;
    Some((
        pts.iter().map(|p| p[0]).sum::<f64>() / n,
        pts.iter().map(|p| p[1]).sum::<f64>() / n,
    ))
}

fn collect_points(value: &GeoValue, out: &mut Vec<Vec<f64>>) {
    match value {
        GeoValue::Point(c) => out.push(c.clone()),
        GeoValue::MultiPoint(ps) => out.extend(ps.iter().cloned()),
        GeoValue::LineString(ls) => out.extend(ls.iter().cloned()),
        GeoValue::MultiLineString(lss) => out.extend(lss.iter().flatten().cloned()),
        GeoValue::Polygon(rings) => out.extend(rings.first().cloned().unwrap_or_default()),
        GeoValue::MultiPolygon(polys) => {
            for rings in polys {
                out.extend(rings.first().cloned().unwrap_or_default());
            }
        }
        GeoValue::GeometryCollection(gs) => gs.iter().for_each(|g| collect_points(&g.value, out)),
    }
}

fn numeric_field(feature: &Feature, field: &str) -> Option<f64> {
    feature.properties.as_ref()?.get(field)?.as_f64()
}

fn require_field(fc: &FeatureCollection, field: Option<&str>) -> Result<String, String> {
    field
        .map(str::to_string)
        .ok_or("a numeric attribute field is required".to_string())
        .and_then(|f| {
            if fc.features.iter().any(|ft| numeric_field(ft, &f).is_some()) {
                Ok(f)
            } else {
                Err(format!("field '{f}' not found or non-numeric"))
            }
        })
}

fn point_feature(lng: f64, lat: f64, props: Map<String, JsonValue>) -> Feature {
    Feature {
        bbox: None,
        geometry: Some(geojson::Geometry::new(GeoValue::Point(vec![lng, lat]))),
        id: None,
        properties: Some(props),
        foreign_members: None,
    }
}

/// Mean center: the average x/y of all feature centroids.
pub fn mean_center(fc: &FeatureCollection) -> Result<(FeatureCollection, JsonValue), String> {
    let centroids: Vec<_> = fc.features.iter().filter_map(centroid_of).collect();
    if centroids.is_empty() {
        return Err("no geometry to measure".into());
    }
    let n = centroids.len() as f64;
    let lng = centroids.iter().map(|c| c.0).sum::<f64>() / n;
    let lat = centroids.iter().map(|c| c.1).sum::<f64>() / n;

    let out = FeatureCollection {
        bbox: None,
        features: vec![point_feature(
            lng,
            lat,
            Map::from_iter([("type".into(), json!("Mean Center"))]),
        )],
        foreign_members: None,
    };
    Ok((
        out,
        json!({ "feature_count": fc.features.len(), "center_lng": lng, "center_lat": lat }),
    ))
}

/// Median center: the point minimizing total haversine distance to all
/// centroids (geometric median via Weiszfeld iteration).
pub fn median_center(fc: &FeatureCollection) -> Result<(FeatureCollection, JsonValue), String> {
    let centroids: Vec<_> = fc.features.iter().filter_map(centroid_of).collect();
    if centroids.is_empty() {
        return Err("no geometry to measure".into());
    }
    let (mut cx, mut cy) = (
        centroids.iter().map(|c| c.0).sum::<f64>() / centroids.len() as f64,
        centroids.iter().map(|c| c.1).sum::<f64>() / centroids.len() as f64,
    );
    for _ in 0..200 {
        let mut num_x = 0.0;
        let mut num_y = 0.0;
        let mut den = 0.0;
        for &(lng, lat) in &centroids {
            let d = (haversine_distance(&[cx, cy], &[lng, lat]) + 1e-6).recip();
            num_x += lng * d;
            num_y += lat * d;
            den += d;
        }
        let (nx, ny) = (num_x / den, num_y / den);
        if (nx - cx).abs() < 1e-9 && (ny - cy).abs() < 1e-9 {
            cx = nx;
            cy = ny;
            break;
        }
        cx = nx;
        cy = ny;
    }

    let out = FeatureCollection {
        bbox: None,
        features: vec![point_feature(
            cx,
            cy,
            Map::from_iter([("type".into(), json!("Median Center"))]),
        )],
        foreign_members: None,
    };
    Ok((
        out,
        json!({ "feature_count": fc.features.len(), "center_lng": cx, "center_lat": cy }),
    ))
}

/// Linear directional mean: the mean orientation of line features as a
/// vector (radians + compass bearing), weighted by line length.
pub fn linear_directional_mean(
    fc: &FeatureCollection,
) -> Result<(FeatureCollection, JsonValue), String> {
    let mut sin_sum = 0.0;
    let mut cos_sum = 0.0;
    let mut lines = 0usize;
    for feature in &fc.features {
        let Some(geom) = &feature.geometry else {
            continue;
        };
        let segments: Vec<&Vec<Vec<f64>>> = match &geom.value {
            GeoValue::LineString(ls) => vec![ls],
            GeoValue::MultiLineString(lss) => lss.iter().collect(),
            _ => continue,
        };
        for ls in segments {
            if ls.len() < 2 {
                continue;
            }
            let (x1, y1) = (ls[0][0].to_radians(), ls[0][1].to_radians());
            let (x2, y2) = (
                ls[ls.len() - 1][0].to_radians(),
                ls[ls.len() - 1][1].to_radians(),
            );
            sin_sum += y2 - y1;
            cos_sum += x2 - x1;
            lines += 1;
        }
    }
    if lines == 0 {
        return Err("no line geometry to measure".into());
    }
    let theta = cos_sum.atan2(sin_sum); // compass bearing: atan2(east, north)
    let bearing = (theta.to_degrees() + 360.0) % 360.0;

    let out = FeatureCollection {
        bbox: None,
        features: vec![Feature {
            bbox: None,
            geometry: Some(geojson::Geometry::new(GeoValue::LineString(vec![
                vec![0.0, 0.0],
                vec![theta.cos(), theta.sin()],
            ]))),
            id: None,
            properties: Some(Map::from_iter([
                ("type".into(), json!("Linear Directional Mean")),
                ("bearing_deg".into(), json!(bearing)),
            ])),
            foreign_members: None,
        }],
        foreign_members: None,
    };
    Ok((
        out,
        json!({ "line_count": lines, "direction_deg": (bearing * 10.0).round() / 10.0, "direction_rad": (theta * 1000.0).round() / 1000.0 }),
    ))
}

/// Global Moran's I: spatial autocorrelation of a numeric field using an
/// inverse-distance weights matrix. Returns I, E[I], z, and p (two-tailed).
pub fn morans_i(
    fc: &FeatureCollection,
    field: &str,
) -> Result<(FeatureCollection, JsonValue), String> {
    let field = require_field(fc, Some(field))?;
    let samples: Vec<(f64, f64, f64)> = fc
        .features
        .iter()
        .filter_map(|f| Some((centroid_of(f)?, numeric_field(f, &field)?)))
        .map(|((x, y), v)| (x, y, v))
        .collect();
    if samples.len() < 3 {
        return Err("Moran's I requires at least 3 features with the field".into());
    }

    let n = samples.len() as f64;
    let mean = samples.iter().map(|s| s.2).sum::<f64>() / n;
    let deviations: Vec<f64> = samples.iter().map(|s| s.2 - mean).collect();
    let denom: f64 = deviations.iter().map(|d| d * d).sum::<f64>();
    if denom == 0.0 {
        return Err("field has zero variance; Moran's I is undefined".into());
    }

    let mut num = 0.0;
    let mut w_sum = 0.0;
    for i in 0..samples.len() {
        for j in (i + 1)..samples.len() {
            let d =
                haversine_distance(&[samples[i].0, samples[i].1], &[samples[j].0, samples[j].1])
                    .max(1.0);
            let w = 1.0 / d;
            num += w * deviations[i] * deviations[j];
            w_sum += w;
            // Symmetric weights counted once; I's numerator convention uses wij+zizj.
            num += w * deviations[j] * deviations[i];
            w_sum += w;
        }
    }
    let s0 = w_sum;
    let i_stat = (n / s0) * (num / denom);
    let expected = -1.0 / (n - 1.0);
    // Variance under randomization hypothesis (simplified, S1=S2=2*S0 for symmetric weights).
    let var = ((n * n - 3.0 * n + 3.0) * s0 * s0)
        / ((n - 1.0) * (n - 1.0) * (n - 2.0) * (n - 3.0) + 1e-12)
        - (n * n + 3.0 * n - 6.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0) + 1e-12)
        - expected * expected;
    let z = if var > 0.0 {
        (i_stat - expected) / var.sqrt()
    } else {
        0.0
    };
    let p = 2.0 * (1.0 - normal_cdf(z.abs()));

    Ok((
        fc.clone(),
        json!({
            "morans_i": (i_stat * 10_000.0).round() / 10_000.0,
            "expected_i": (expected * 10_000.0).round() / 10_000.0,
            "z_score": (z * 1000.0).round() / 1000.0,
            "p_value": (p * 10_000.0).round() / 10_000.0,
            "pattern": if z > 1.96 { "clustered" } else if z < -1.96 { "dispersed" } else { "random" },
            "field": field,
            "feature_count": samples.len()
        }),
    ))
}

/// Getis-Ord Gi* hot spot analysis: per-feature z-scores flagging
/// statistically significant hot (high-high) and cold (low-low) spots.
/// Weights are binary within a distance band (self included); the band
/// defaults to 3x the mean nearest-neighbor distance, or `band_meters`.
pub fn getis_ord_gi(
    fc: &FeatureCollection,
    field: &str,
    band_meters: Option<f64>,
) -> Result<(FeatureCollection, JsonValue), String> {
    let field = require_field(fc, Some(field))?;
    let samples: Vec<(f64, f64, f64)> = fc
        .features
        .iter()
        .filter_map(|f| Some((centroid_of(f)?, numeric_field(f, &field)?)))
        .map(|((x, y), v)| (x, y, v))
        .collect();
    if samples.len() < 3 {
        return Err("Getis-Ord Gi* requires at least 3 features with the field".into());
    }

    let n = samples.len() as f64;
    let sum_all: f64 = samples.iter().map(|s| s.2).sum();
    let sq_all: f64 = samples.iter().map(|s| s.2 * s.2).sum();
    let mean = sum_all / n;
    let s = (sq_all / n - mean * mean).sqrt().max(1e-12);

    // Adaptive band: 3x mean nearest-neighbor distance.
    let band = band_meteres_or_adaptive(&samples, band_meters);

    // Inverse-distance weights, self-included (Gi*).
    let mut out_features = Vec::with_capacity(fc.features.len());
    let mut hot = 0usize;
    let mut cold = 0usize;
    for (idx, feature) in fc.features.iter().enumerate() {
        let Some((_, _, xi)) = samples.get(idx).copied() else {
            out_features.push(feature.clone());
            continue;
        };
        let mut sum_w = 0.0;
        let mut sum_wx = 0.0;
        let mut sum_w2 = 0.0;
        for (j, &(xj, yj, vj)) in samples.iter().enumerate() {
            let w = if j == idx
                || haversine_distance(&[samples[idx].0, samples[idx].1], &[xj, yj]) <= band
            {
                1.0
            } else {
                0.0
            };
            sum_w += w;
            sum_wx += w * vj;
            sum_w2 += w * w;
        }
        let num = sum_wx - mean * sum_w;
        let den = s * (((n * sum_w2 - sum_w * sum_w) / (n - 1.0)).max(1e-12)).sqrt();
        let z = num / den;
        let class = if z >= 2.576 {
            hot += 1;
            "hot_99pct"
        } else if z >= 1.96 {
            hot += 1;
            "hot_95pct"
        } else if z <= -2.576 {
            cold += 1;
            "cold_99pct"
        } else if z <= -1.96 {
            cold += 1;
            "cold_95pct"
        } else {
            "not_significant"
        };

        let mut props = feature.properties.clone().unwrap_or_default();
        props.insert("gi_z_score".into(), json!((z * 1000.0).round() / 1000.0));
        props.insert(
            "gi_p_value".into(),
            json!((p_value(z) * 10_000.0).round() / 10_000.0),
        );
        props.insert("gi_class".into(), json!(class));
        props.insert(format!("gi_source_{field}"), json!(xi));
        out_features.push(Feature {
            bbox: feature.bbox.clone(),
            geometry: feature.geometry.clone(),
            id: feature.id.clone(),
            properties: Some(props),
            foreign_members: feature.foreign_members.clone(),
        });
    }

    Ok((
        FeatureCollection {
            bbox: fc.bbox.clone(),
            features: out_features,
            foreign_members: None,
        },
        json!({
            "field": field,
            "feature_count": samples.len(),
            "hot_spots": hot,
            "cold_spots": cold,
            "confidence": "95%/99% (z ≥ ±1.96/±2.576)"
        }),
    ))
}

/// Ordinary Least Squares regression of a dependent field on explanatory
/// fields via normal equations (Gaussian elimination, ≤6 regressors).
/// Residuals are attached to output features as `ols_residual`.
pub fn ols_regression(
    fc: &FeatureCollection,
    dependent: &str,
    explanatory: &[String],
) -> Result<(FeatureCollection, JsonValue), String> {
    let dep = require_field(fc, Some(dependent))?;
    if explanatory.is_empty() {
        return Err("at least one explanatory field is required".into());
    }
    if explanatory.len() > 6 {
        return Err("at most 6 explanatory fields are supported".into());
    }
    let mut expl = Vec::with_capacity(explanatory.len());
    for name in explanatory {
        expl.push(require_field(fc, Some(name))?);
    }

    // Rows: [1, x1..xk, y]
    let mut rows: Vec<Vec<f64>> = Vec::new();
    for feature in &fc.features {
        let Some(y) = numeric_field(feature, &dep) else {
            continue;
        };
        let mut xs = Vec::with_capacity(expl.len());
        for name in &expl {
            match numeric_field(feature, name) {
                Some(v) => xs.push(v),
                None => {
                    rows.clear();
                    break;
                }
            }
        }
        if xs.len() == expl.len() {
            rows.push(xs);
            rows.last_mut().unwrap().push(y);
        }
    }
    let k = expl.len() + 1;
    if rows.len() < k + 1 {
        return Err(format!(
            "need at least {} complete rows for {} regressor(s); found {}",
            k + 1,
            expl.len(),
            rows.len()
        ));
    }

    // Normal equations X'X b = X'y with intercept column.
    let mut ata = vec![vec![0.0; k]; k];
    let mut atb = vec![0.0; k];
    for row in &rows {
        let mut x = vec![1.0];
        x.extend_from_slice(&row[..k - 1]);
        let y = row[k - 1];
        for i in 0..k {
            for j in 0..k {
                ata[i][j] += x[i] * x[j];
            }
            atb[i] += x[i] * y;
        }
    }
    let beta =
        solve_linear(&ata, &atb).ok_or("regressors are singular (check for duplicated fields)")?;

    let n = rows.len() as f64;
    let mean_y = rows.iter().map(|r| r[k - 1]).sum::<f64>() / n;
    let (mut sst, mut sse) = (0.0, 0.0);
    for row in &rows {
        let mut x = vec![1.0];
        x.extend_from_slice(&row[..k - 1]);
        let y = row[k - 1];
        let pred: f64 = beta.iter().zip(&x).map(|(b, v)| b * v).sum();
        sse += (y - pred).powi(2);
        sst += (y - mean_y).powi(2);
    }
    let r2 = if sst > 0.0 { 1.0 - sse / sst } else { 0.0 };
    let adj_r2 = 1.0 - (1.0 - r2) * (n - 1.0) / (n - k as f64);
    let mut aic = 0.0;
    if sse > 0.0 {
        aic = n * (sse / n).ln() + 2.0 * k as f64;
    }

    // Attach residuals to output features.
    let mut out_features = Vec::with_capacity(fc.features.len());
    for feature in &fc.features {
        let mut props = feature.properties.clone().unwrap_or_default();
        if let Some(y) = numeric_field(feature, &dep) {
            let mut pred = beta[0];
            let mut complete = true;
            for (i, name) in expl.iter().enumerate() {
                match numeric_field(feature, name) {
                    Some(v) => pred += beta[i + 1] * v,
                    None => {
                        complete = false;
                        break;
                    }
                }
            }
            if complete {
                props.insert("ols_fitted".into(), json!((pred * 1000.0).round() / 1000.0));
                props.insert(
                    "ols_residual".into(),
                    json!(((y - pred) * 1000.0).round() / 1000.0),
                );
            }
        }
        out_features.push(Feature {
            bbox: feature.bbox.clone(),
            geometry: feature.geometry.clone(),
            id: feature.id.clone(),
            properties: Some(props),
            foreign_members: feature.foreign_members.clone(),
        });
    }

    let mut coeffs = Map::new();
    coeffs.insert(
        "intercept".into(),
        json!((beta[0] * 1000.0).round() / 1000.0),
    );
    for (i, name) in expl.iter().enumerate() {
        coeffs.insert(name.clone(), json!((beta[i + 1] * 1000.0).round() / 1000.0));
    }

    Ok((
        FeatureCollection {
            bbox: fc.bbox.clone(),
            features: out_features,
            foreign_members: None,
        },
        json!({
            "dependent": dep,
            "explanatory": expl,
            "coefficients": coeffs,
            "r_squared": (r2 * 10_000.0).round() / 10_000.0,
            "adjusted_r_squared": (adj_r2 * 10_000.0).round() / 10_000.0,
            "aic": (aic * 10.0).round() / 10.0,
            "observations": rows.len()
        }),
    ))
}

/// Mean nearest-neighbor distance; the Gi* band defaults to 3x this value.
fn mean_nn_distance(samples: &[(f64, f64, f64)]) -> f64 {
    let mut total = 0.0;
    for (i, &(x, y, _)) in samples.iter().enumerate() {
        let nn = samples
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, &(x2, y2, _))| haversine_distance(&[x, y], &[x2, y2]))
            .fold(f64::INFINITY, f64::min);
        total += nn;
    }
    (total / samples.len() as f64).max(1.0)
}

fn band_meteres_or_adaptive(samples: &[(f64, f64, f64)], band_meters: Option<f64>) -> f64 {
    band_meters.unwrap_or_else(|| mean_nn_distance(samples) * 3.0)
}

/// Standard normal CDF (Abramowitz & Stegun 7.1.26 erf approximation).
fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();
    sign * y
}

/// Two-tailed p-value from a z-score.
fn p_value(z: f64) -> f64 {
    2.0 * (1.0 - normal_cdf(z.abs()))
}

/// Gaussian elimination with partial pivoting; None if singular.
pub(crate) fn solve_linear(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = a.to_vec();
    for i in 0..n {
        m[i].push(b[i]);
    }
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(pivot, col);
        let pivot_row = m[col][col..=n].to_vec();
        let pivot_pivot = m[col][col];
        for row in m.iter_mut().take(n).skip(col + 1) {
            let factor = row[col] / pivot_pivot;
            for (c, val) in row.iter_mut().enumerate().skip(col) {
                *val -= factor * pivot_row[c - col];
            }
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = m[i][n];
        for j in (i + 1)..n {
            s -= m[i][j] * x[j];
        }
        x[i] = s / m[i][i];
    }
    Some(x)
}

/// Centroids exposed for sibling modules (network/geostatistics reuse).
pub(crate) fn feature_centroids(fc: &FeatureCollection) -> Vec<(f64, f64)> {
    fc.features.iter().filter_map(centroid_of).collect()
}

/// Public point-collector for sibling modules.
pub(crate) fn collect_points_pub(value: &GeoValue, out: &mut Vec<Vec<f64>>) {
    collect_points(value, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geojson::Feature;

    fn point_fc(points: &[(f64, f64, f64)]) -> FeatureCollection {
        FeatureCollection {
            bbox: None,
            features: points
                .iter()
                .map(|&(x, y, v)| {
                    let mut props = Map::new();
                    props.insert("value".into(), json!(v));
                    Feature {
                        bbox: None,
                        geometry: Some(geojson::Geometry::new(GeoValue::Point(vec![x, y]))),
                        id: None,
                        properties: Some(props),
                        foreign_members: None,
                    }
                })
                .collect(),
            foreign_members: None,
        }
    }

    #[test]
    fn test_mean_and_median_center() {
        let fc = point_fc(&[(0.0, 0.0, 1.0), (10.0, 10.0, 2.0), (20.0, 0.0, 3.0)]);
        let (out, summary) = mean_center(&fc).unwrap();
        assert_eq!(out.features.len(), 1);
        assert_eq!(summary["center_lng"], 10.0);
        let (_, med) = median_center(&fc).unwrap();
        assert!(med["center_lng"].as_f64().unwrap().abs() < 15.0);
    }

    #[test]
    fn test_morans_i_positive_for_clustered_values() {
        // Two tight clusters with distinct values: within-cluster pairs
        // dominate the inverse-distance weights -> positive autocorrelation.
        let fc = point_fc(&[
            (0.0, 0.0, 10.0),
            (0.01, 0.0, 10.5),
            (0.02, 0.0, 11.0),
            (5.0, 5.0, 50.0),
            (5.01, 5.0, 50.5),
        ]);
        let (_, summary) = morans_i(&fc, "value").unwrap();
        assert!(summary["morans_i"].as_f64().unwrap() > 0.5);
    }

    #[test]
    fn test_morans_i_zero_variance_is_error() {
        let fc = point_fc(&[(0.0, 0.0, 5.0), (1.0, 1.0, 5.0), (2.0, 2.0, 5.0)]);
        assert!(morans_i(&fc, "value").is_err());
    }

    #[test]
    fn test_getis_ord_flags_hot_spot() {
        let fc = point_fc(&[
            (0.0, 0.0, 900.0),
            (0.01, 0.0, 950.0),
            (0.02, 0.0, 980.0),
            (5.0, 5.0, 1.0),
            (5.01, 5.0, 2.0),
            (5.02, 5.0, 3.0),
        ]);
        let (out, summary) = getis_ord_gi(&fc, "value", None).unwrap();
        assert_eq!(out.features.len(), 6);
        assert!(summary["hot_spots"].as_u64().unwrap() >= 2);
        assert!(out.features[0]
            .properties
            .as_ref()
            .unwrap()
            .contains_key("gi_z_score"));
    }

    #[test]
    fn test_ols_recovers_exact_linear_relationship() {
        // y = 2*x + 1 exactly.
        let fc = FeatureCollection {
            bbox: None,
            features: (0..4)
                .map(|i| {
                    let mut props = Map::new();
                    props.insert("y".into(), json!((2.0 * i as f64 + 1.0)));
                    props.insert("x".into(), json!(i as f64));
                    Feature {
                        bbox: None,
                        geometry: Some(geojson::Geometry::new(GeoValue::Point(vec![
                            0.0, i as f64,
                        ]))),
                        id: None,
                        properties: Some(props),
                        foreign_members: None,
                    }
                })
                .collect(),
            foreign_members: None,
        };
        let (out, summary) = ols_regression(&fc, "y", &["x".to_string()]).unwrap();
        assert_eq!(out.features.len(), 4);
        assert_eq!(summary["coefficients"]["intercept"], 1.0);
        assert_eq!(summary["coefficients"]["x"], 2.0);
        assert!(summary["r_squared"].as_f64().unwrap() > 0.999);
        let residual: f64 = out.features[0]
            .properties
            .as_ref()
            .unwrap()
            .get("ols_residual")
            .unwrap()
            .as_f64()
            .unwrap();
        assert!(residual.abs() < 0.01);
    }

    #[test]
    fn test_directional_mean() {
        let fc = FeatureCollection {
            bbox: None,
            features: vec![Feature {
                bbox: None,
                geometry: Some(geojson::Geometry::new(GeoValue::LineString(vec![
                    vec![0.0, 0.0],
                    vec![1.0, 0.0],
                ]))),
                id: None,
                properties: None,
                foreign_members: None,
            }],
            foreign_members: None,
        };
        let (_, summary) = linear_directional_mean(&fc).unwrap();
        assert_eq!(summary["direction_deg"], 90.0); // due east
    }
}
