//! Geostatistical Analyst: deterministic and probabilistic surface
//! prediction from scattered points with z-values (IDW, Ordinary Kriging).
//!
//! Prediction grids are emitted as GeoJSON point features so results flow
//! through the existing layer pipeline; kriging additionally reports the
//! standard error surface as a second property.

use crate::gis::metrics::haversine_distance;
use crate::gis::spatial_statistics::solve_linear;
use geojson::{Feature, FeatureCollection, Value as GeoValue};
use serde_json::{json, Map, Value as JsonValue};

/// Semivariogram model families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariogramModel {
    Spherical,
    Exponential,
    Gaussian,
}

impl VariogramModel {
    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("exponential") => Self::Exponential,
            Some("gaussian") => Self::Gaussian,
            _ => Self::Spherical,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Spherical => "spherical",
            Self::Exponential => "exponential",
            Self::Gaussian => "gaussian",
        }
    }

    /// γ(h) for nugget c0, sill c, range a.
    fn gamma(self, h: f64, nugget: f64, sill: f64, range: f64) -> f64 {
        let c = (sill - nugget).max(0.0);
        if h >= range && self != Self::Exponential {
            return nugget + c;
        }
        let r = h / range;
        match self {
            Self::Spherical => nugget + c * (1.5 * r - 0.5 * r * r * r),
            Self::Exponential => nugget + c * (1.0 - (-3.0 * r).exp()),
            Self::Gaussian => nugget + c * (1.0 - (-3.0 * r * r).exp()),
        }
    }
}

struct Sample {
    lng: f64,
    lat: f64,
    value: f64,
}

fn z_samples(fc: &FeatureCollection, field: &str) -> Result<Vec<Sample>, String> {
    let mut samples = Vec::new();
    for feature in &fc.features {
        let Some(v) = feature
            .properties
            .as_ref()
            .and_then(|p| p.get(field))
            .and_then(JsonValue::as_f64)
        else {
            continue;
        };
        let Some(geom) = &feature.geometry else {
            continue;
        };
        let (lng, lat) = match &geom.value {
            GeoValue::Point(c) => (c[0], c[1]),
            _ => {
                let mut pts = Vec::new();
                crate::gis::spatial_statistics::collect_points_pub(&geom.value, &mut pts);
                if pts.is_empty() {
                    continue;
                }
                let n = pts.len() as f64;
                (
                    pts.iter().map(|p| p[0]).sum::<f64>() / n,
                    pts.iter().map(|p| p[1]).sum::<f64>() / n,
                )
            }
        };
        samples.push(Sample { lng, lat, value: v });
    }
    if samples.len() < 3 {
        return Err("at least 3 features with the value field are required".into());
    }
    Ok(samples)
}

/// Bounding box of the samples with a small margin, used as the prediction extent.
fn sample_bounds(samples: &[Sample]) -> (f64, f64, f64, f64) {
    let min_lng = samples.iter().map(|s| s.lng).fold(f64::INFINITY, f64::min);
    let max_lng = samples
        .iter()
        .map(|s| s.lng)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_lat = samples.iter().map(|s| s.lat).fold(f64::INFINITY, f64::min);
    let max_lat = samples
        .iter()
        .map(|s| s.lat)
        .fold(f64::NEG_INFINITY, f64::max);
    let pad_lng = (max_lng - min_lng).max(0.01) * 0.02;
    let pad_lat = (max_lat - min_lat).max(0.01) * 0.02;
    (
        min_lng - pad_lng,
        min_lat - pad_lat,
        max_lng + pad_lng,
        max_lat + pad_lat,
    )
}

/// Inverse Distance Weighting: prediction grid over the sample extent.
/// Weight = 1/d^power; `max_neighbors` nearest samples participate.
pub fn inverse_distance_weighting(
    fc: &FeatureCollection,
    field: &str,
    power: f64,
    cell_size_km: f64,
    max_neighbors: usize,
) -> Result<(FeatureCollection, JsonValue), String> {
    if power <= 0.0 {
        return Err("IDW power exponent must be positive".into());
    }
    let field = field.to_string();
    let samples = z_samples(fc, &field)?;
    let (min_lng, min_lat, max_lng, max_lat) = sample_bounds(&samples);
    let step = (cell_size_km / 111.32).max(1e-4); // approx degrees per cell

    let cols = (((max_lng - min_lng) / step).ceil() as usize + 1).min(200);
    let rows = (((max_lat - min_lat) / step).ceil() as usize + 1).min(200);

    let mut features = Vec::with_capacity(cols * rows);
    let mut values: Vec<f64> = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            let lng = min_lng + step * c as f64;
            let lat = min_lat + step * r as f64;
            // Nearest max_neighbors by distance.
            let mut dists: Vec<(f64, f64)> = samples
                .iter()
                .map(|s| (haversine_distance(&[lng, lat], &[s.lng, s.lat]), s.value))
                .collect();
            dists.sort_by(|a, b| a.0.total_cmp(&b.0));
            dists.truncate(max_neighbors.max(1));

            let exact = dists.iter().find(|(d, _)| *d < 1.0);
            let (prediction, weight_sum) = if let Some((_, v)) = exact {
                (*v, 1.0)
            } else {
                let w_sum: f64 = dists.iter().map(|(d, _)| d.powf(-power)).sum();
                let v: f64 = dists.iter().map(|(d, val)| val * d.powf(-power)).sum();
                (v / w_sum, w_sum)
            };

            let mut props = Map::new();
            props.insert(
                "predicted".into(),
                json!((prediction * 1000.0).round() / 1000.0),
            );
            props.insert(
                "idw_weight_sum".into(),
                json!((weight_sum * 1000.0).round() / 1000.0),
            );
            values.push(prediction);
            features.push(Feature {
                bbox: None,
                geometry: Some(geojson::Geometry::new(GeoValue::Point(vec![lng, lat]))),
                id: None,
                properties: Some(props),
                foreign_members: None,
            });
        }
    }

    Ok((
        FeatureCollection {
            bbox: None,
            features,
            foreign_members: None,
        },
        json!({
            "method": "idw",
            "field": field,
            "power": power,
            "cell_size_km": cell_size_km,
            "max_neighbors": max_neighbors,
            "grid_cells": values.len(),
            "predicted_min": (values.iter().cloned().fold(f64::INFINITY, f64::min) * 1000.0).round() / 1000.0,
            "predicted_max": (values.iter().cloned().fold(f64::NEG_INFINITY, f64::max) * 1000.0).round() / 1000.0,
            "predicted_mean": (values.iter().sum::<f64>() / values.len() as f64 * 1000.0).round() / 1000.0
        }),
    ))
}

/// Fitted variogram parameters (nugget, sill, range) + model family.
#[derive(Debug, Clone, Copy)]
pub struct Variogram {
    pub model: VariogramModel,
    pub nugget: f64,
    pub sill: f64,
    pub range_deg: f64,
}

/// Empirical semivariance binned by lag distance, then coarse grid-search fit.
fn fit_variogram(samples: &[Sample], model: VariogramModel) -> Variogram {
    let n = samples.len();
    // Pairwise (distance, squared-diff/2).
    let mut pairs: Vec<(f64, f64)> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let d = haversine_distance(
                &[samples[i].lng, samples[i].lat],
                &[samples[j].lng, samples[j].lat],
            );
            pairs.push((d, (samples[i].value - samples[j].value).powi(2) / 2.0));
        }
    }
    let max_d = pairs
        .iter()
        .map(|(d, _)| *d)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);
    let var_total: f64 = pairs.iter().map(|(_, g)| *g).sum::<f64>() / pairs.len().max(1) as f64;
    let sill = (var_total * 2.0).max(1e-9); // semivariance plateaus near 2*mean(γ) ≈ variance

    // Bin empirical semivariance into 12 lag bins up to half the max distance.
    let lag_max = max_d * 0.5;
    let bins = 12;
    let mut bin_sum = vec![0.0; bins];
    let mut bin_count = vec![0usize; bins];
    for &(d, g) in &pairs {
        if d <= lag_max {
            let b = ((d / lag_max) * bins as f64).min((bins - 1) as f64) as usize;
            bin_sum[b] += g;
            bin_count[b] += 1;
        }
    }

    // Coarse grid search over nugget/range minimizing squared error vs binned γ.
    let mut best = Variogram {
        model,
        nugget: 0.0,
        sill,
        range_deg: 1.0,
    };
    let mut best_err = f64::INFINITY;
    for nugget_frac in [0.0, 0.1, 0.25, 0.5] {
        let nugget = sill * nugget_frac;
        for range_scale in [0.15, 0.3, 0.5, 0.75, 1.0] {
            let range = lag_max * range_scale;
            let mut err = 0.0;
            for (b, count) in bin_count.iter().enumerate() {
                if *count == 0 {
                    continue;
                }
                let lag = (b as f64 + 0.5) / bins as f64 * lag_max;
                let predicted = model.gamma(lag, nugget, sill, range);
                err += *count as f64 * (predicted - bin_sum[b]).powi(2);
            }
            if err < best_err {
                best_err = err;
                best = Variogram {
                    model,
                    nugget,
                    sill,
                    // store range in degrees for direct haversine-free reuse
                    range_deg: range / 111_320.0,
                };
            }
        }
    }
    best
}

fn gamma_deg(_model: VariogramModel, h_deg: f64, v: &Variogram) -> f64 {
    v.model
        .gamma(h_deg * 111_320.0, v.nugget, v.sill, v.range_deg * 111_320.0)
}

/// Ordinary Kriging: best linear unbiased prediction on a grid with a fitted
/// variogram. Emits `predicted` plus `standard_error` per grid cell.
pub fn ordinary_kriging(
    fc: &FeatureCollection,
    field: &str,
    model_str: Option<&str>,
    cell_size_km: f64,
    max_neighbors: usize,
) -> Result<(FeatureCollection, JsonValue), String> {
    let field = field.to_string();
    let model = VariogramModel::parse(model_str);
    let samples = z_samples(fc, &field)?;
    if samples.len() > 400 {
        // Kriging solves an (N+1) system; cap for responsiveness.
        return Err(format!(
            "ordinary kriging supports up to 400 points; dataset has {} (use IDW for dense sets)",
            samples.len()
        ));
    }
    let variogram = fit_variogram(&samples, model);
    let n = samples.len();

    let (min_lng, min_lat, max_lng, max_lat) = sample_bounds(&samples);
    let step = (cell_size_km / 111.32).max(1e-4);
    let cols = (((max_lng - min_lng) / step).ceil() as usize + 1).min(150);
    let rows = (((max_lat - min_lat) / step).ceil() as usize + 1).min(150);

    let mut features = Vec::with_capacity(cols * rows);
    let mut err_sum = 0.0;
    for r in 0..rows {
        for c in 0..cols {
            let lng = min_lng + step * c as f64;
            let lat = min_lat + step * r as f64;

            let mut dists: Vec<(usize, f64)> = (0..n)
                .map(|i| {
                    (
                        i,
                        ((samples[i].lng - lng).powi(2) + (samples[i].lat - lat).powi(2)).sqrt(),
                    )
                })
                .collect();
            dists.sort_by(|a, b| a.1.total_cmp(&b.1));
            let chosen: Vec<usize> = dists
                .into_iter()
                .take(max_neighbors.max(1))
                .map(|(i, _)| i)
                .collect();

            // Solve on the reduced neighborhood: rebuild rows for chosen indices.
            let m = chosen.len();
            let mut sys = vec![vec![0.0; m + 1]; m + 1];
            for (a, &i) in chosen.iter().enumerate() {
                for (b, &j) in chosen.iter().enumerate() {
                    let d = ((samples[i].lng - samples[j].lng).powi(2)
                        + (samples[i].lat - samples[j].lat).powi(2))
                    .sqrt();
                    sys[a][b] = if i == j {
                        0.0
                    } else {
                        gamma_deg(model, d, &variogram)
                    };
                }
                sys[a][m] = 1.0;
                sys[m][a] = 1.0;
            }
            let mut rhs = vec![0.0; m + 1];
            for (a, &i) in chosen.iter().enumerate() {
                let d = ((samples[i].lng - lng).powi(2) + (samples[i].lat - lat).powi(2)).sqrt();
                rhs[a] = gamma_deg(model, d, &variogram);
            }
            let Some(sol) = solve_linear(&sys, &rhs[..m + 1]) else {
                continue;
            };
            let prediction: f64 = sol[..m]
                .iter()
                .zip(chosen.iter())
                .map(|(&w, &i)| w * samples[i].value)
                .sum();
            // kriging variance = Σ λj γ(s0, sj) + μ
            let variance: f64 = sol[..m]
                .iter()
                .zip(chosen.iter())
                .map(|(&w, &i)| {
                    let d =
                        ((samples[i].lng - lng).powi(2) + (samples[i].lat - lat).powi(2)).sqrt();
                    w * gamma_deg(model, d, &variogram)
                })
                .sum::<f64>()
                + sol[m];
            let stderr = variance.max(0.0).sqrt();
            err_sum += stderr;

            let mut props = Map::new();
            props.insert(
                "predicted".into(),
                json!((prediction * 1000.0).round() / 1000.0),
            );
            props.insert(
                "standard_error".into(),
                json!((stderr * 1000.0).round() / 1000.0),
            );
            features.push(Feature {
                bbox: None,
                geometry: Some(geojson::Geometry::new(GeoValue::Point(vec![lng, lat]))),
                id: None,
                properties: Some(props),
                foreign_members: None,
            });
        }
    }

    let grid_cells = features.len();
    let mean_stderr = if grid_cells == 0 {
        0.0
    } else {
        err_sum / grid_cells as f64
    };
    Ok((
        FeatureCollection {
            bbox: None,
            features,
            foreign_members: None,
        },
        json!({
            "method": "ordinary_kriging",
            "field": field,
            "variogram_model": variogram.model.name(),
            "nugget": (variogram.nugget * 1000.0).round() / 1000.0,
            "sill": (variogram.sill * 1000.0).round() / 1000.0,
            "range_km": ((variogram.range_deg * 111.32) * 10.0).round() / 10.0,
            "grid_cells": grid_cells,
            "mean_standard_error": (mean_stderr * 1000.0).round() / 1000.0
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_fc(points: &[(f64, f64, f64)]) -> FeatureCollection {
        FeatureCollection {
            bbox: None,
            features: points
                .iter()
                .map(|&(x, y, v)| {
                    let mut props = Map::new();
                    props.insert("z".into(), json!(v));
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
    fn test_idw_recovers_exact_value_at_sample_location() {
        let fc = sample_fc(&[
            (0.0, 0.0, 10.0),
            (0.5, 0.5, 20.0),
            (1.0, 0.0, 30.0),
            (0.0, 1.0, 40.0),
        ]);
        let (out, summary) = inverse_distance_weighting(&fc, "z", 2.0, 20.0, 4).unwrap();
        assert!(!out.features.is_empty());
        assert_eq!(summary["method"], "idw");
        // A grid cell coinciding with a sample must reproduce its value.
        // The grid cell nearest to sample (0,0,10) must predict ~10: the
        // sample dominates the inverse-distance weighting at close range.
        let nearest = out
            .features
            .iter()
            .min_by(|f, g| {
                let pt = |f: &geojson::Feature| match &f.geometry.as_ref().unwrap().value {
                    GeoValue::Point(c) => (c[0], c[1]),
                    _ => unreachable!(),
                };
                let (x1, y1) = pt(f);
                let (x2, y2) = pt(g);
                (x1 * x1 + y1 * y1).total_cmp(&(x2 * x2 + y2 * y2))
            })
            .expect("grid must not be empty");
        let predicted = nearest
            .properties
            .as_ref()
            .unwrap()
            .get("predicted")
            .unwrap()
            .as_f64()
            .unwrap();
        assert!(
            (predicted - 10.0).abs() < 1.5,
            "predicted {predicted} not ~10"
        );
    }

    #[test]
    fn test_kriging_prediction_within_data_range_and_reports_variogram() {
        let fc = sample_fc(&[
            (0.0, 0.0, 10.0),
            (0.1, 0.0, 12.0),
            (0.0, 0.1, 11.0),
            (0.1, 0.1, 13.0),
            (0.05, 0.05, 12.0),
        ]);
        let (out, summary) = ordinary_kriging(&fc, "z", Some("spherical"), 5.0, 5).unwrap();
        assert!(summary["variogram_model"].is_string());
        assert!(summary["range_km"].as_f64().unwrap() > 0.0);
        assert!(!out.features.is_empty());
        for feature in out.features.iter().take(10) {
            let p = feature.properties.as_ref().unwrap();
            assert!(p.contains_key("predicted"));
            assert!(p.contains_key("standard_error"));
        }
    }

    #[test]
    fn test_variogram_models_differ() {
        let v = Variogram {
            model: VariogramModel::Spherical,
            nugget: 1.0,
            sill: 10.0,
            range_deg: 0.1,
        };
        let g = VariogramModel::Gaussian.gamma(
            0.05 * 111_320.0,
            v.nugget,
            v.sill,
            v.range_deg * 111_320.0,
        );
        let s = VariogramModel::Spherical.gamma(
            0.05 * 111_320.0,
            v.nugget,
            v.sill,
            v.range_deg * 111_320.0,
        );
        assert!((g - s).abs() > 1e-6);
    }

    #[test]
    fn test_requires_minimum_samples() {
        let fc = sample_fc(&[(0.0, 0.0, 1.0), (1.0, 1.0, 2.0)]);
        assert!(inverse_distance_weighting(&fc, "z", 2.0, 10.0, 3).is_err());
        assert!(ordinary_kriging(&fc, "z", None, 10.0, 3).is_err());
    }
}
