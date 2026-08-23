//! Attribute-driven symbology: class breaks (equal interval / quantile)
//! with an interpolated sequential color ramp.

use serde::{Deserialize, Serialize};

/// Collect numeric values of `field` across all features of a collection.
pub fn numeric_values(fc: &geojson::FeatureCollection, field: &str) -> Vec<f64> {
    fc.features
        .iter()
        .filter_map(|f| f.properties.as_ref()?.get(field))
        .filter_map(|v| match v {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassBreak {
    pub min: f64,
    pub max: f64,
    /// Fill color for the class, `#rrggbb`.
    pub color: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationMethod {
    EqualInterval,
    Quantile,
}

impl ClassificationMethod {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "equal_interval" | "equalinterval" | "equal" => Ok(Self::EqualInterval),
            "quantile" => Ok(Self::Quantile),
            other => Err(format!("unknown classification method '{other}'")),
        }
    }
}

/// Compute class breaks for the numeric values of `field`.
pub fn compute_breaks(
    values: &[f64],
    method: ClassificationMethod,
    n_classes: usize,
) -> Result<Vec<ClassBreak>, String> {
    let n_classes = n_classes.clamp(2, 12);
    if values.is_empty() {
        return Err("no numeric values found for the selected field".into());
    }

    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];
    if (max - min).abs() < f64::EPSILON {
        // Degenerate distribution: single class covering the constant value.
        return Ok(vec![ClassBreak {
            min,
            max,
            color: ramp_color(0.0),
            label: format!("{min:.3}"),
        }]);
    }

    let edges: Vec<f64> = match method {
        ClassificationMethod::EqualInterval => (0..=n_classes)
            .map(|i| min + (max - min) * (i as f64) / (n_classes as f64))
            .collect(),
        ClassificationMethod::Quantile => {
            let mut e = Vec::with_capacity(n_classes + 1);
            for i in 0..=n_classes {
                let idx =
                    ((sorted.len() - 1) as f64 * (i as f64 / n_classes as f64)).round() as usize;
                let v = sorted[idx.min(sorted.len() - 1)];
                // Guarantee monotonic edges.
                match e.last() {
                    Some(&last) if v <= last && i > 0 && i < n_classes => {}
                    _ => e.push(v),
                }
            }
            if *e.last().unwrap() != max {
                *e.last_mut().unwrap() = max;
            }
            e
        }
    };

    let breaks = edges
        .windows(2)
        .enumerate()
        .map(|(i, w)| ClassBreak {
            min: w[0],
            max: w[1],
            color: ramp_color(i as f64 / (edges.len() - 2).max(1) as f64),
            label: format!("{:.2} – {:.2}", w[0], w[1]),
        })
        .collect();

    Ok(breaks)
}

/// Sequential blue→teal→yellow ramp (YlGnBu-inspired), interpolated in RGB.
fn ramp_color(t: f64) -> String {
    const STOPS: [(f64, [u8; 3]); 5] = [
        (0.00, [237, 248, 177]), // light yellow
        (0.25, [199, 233, 180]), // pale green
        (0.50, [127, 205, 187]), // teal
        (0.75, [65, 158, 182]),  // blue-teal
        (1.00, [34, 94, 168]),   // deep blue
    ];

    let t = t.clamp(0.0, 1.0);
    let seg = t * (STOPS.len() - 1) as f64;
    let i = (seg.floor() as usize).min(STOPS.len() - 2);
    let local = seg - i as f64;
    let a = STOPS[i].1;
    let b = STOPS[i + 1].1;

    let mix = |x: u8, y: u8| -> u8 { (x as f64 + (y as f64 - x as f64) * local).round() as u8 };
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2])
    )
}
