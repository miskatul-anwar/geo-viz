//! Dataset ingestion and layer provisioning.
//!
//! Owns the full import pipeline (parse -> persist dataset -> provision styled
//! layer) so clients perform one atomic call instead of orchestrating steps.

use crate::db::AppDb;
use crate::error::{AppError, AppResult};
use crate::gis::parser::{parse_geojson_str, ParsedGeoData};
use crate::gis::shapefile_reader::parse_shapefile_bytes;
use crate::models::{DatasetDetail, Layer, LayerStyle};
use chrono::Utc;
use uuid::Uuid;

/// Ingestion formats accepted by the backend pipeline.
pub const SUPPORTED_FORMATS: [&str; 6] = ["geojson", "shapefile", "kml", "kmz", "gpx", "gpkg"];

/// Visual presets applied to freshly provisioned layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StylePreset {
    /// User-imported source data.
    Input,
    /// Output of a geoprocessing tool.
    Result,
}

impl StylePreset {
    pub fn style(self) -> LayerStyle {
        match self {
            StylePreset::Input => LayerStyle {
                fill_color: "#38bdf8".into(),
                fill_opacity: 0.35,
                stroke_color: "#0ea5e9".into(),
                stroke_width: 2.0,
                stroke_opacity: 0.9,
                point_radius: 6.0,
                dash_array: None,
                shape_type: "point".into(),
                classification: None,
                label_field: None,
                blend_mode: None,
            },
            StylePreset::Result => LayerStyle {
                fill_color: "#34d399".into(),
                fill_opacity: 0.4,
                stroke_color: "#059669".into(),
                stroke_width: 2.0,
                stroke_opacity: 0.9,
                point_radius: 6.0,
                dash_array: None,
                shape_type: "point".into(),
                classification: None,
                label_field: None,
                blend_mode: None,
            },
        }
    }
}

/// A persisted dataset together with the layer created for it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportOutcome {
    pub dataset: DatasetDetail,
    pub layer: Layer,
}

/// Ingest a spatial dataset, persist it and create a map layer.
///
/// `payload` is raw text for text formats (geojson/kml/gpx) and base64 for
/// binary formats (shapefile/kmz/gpkg).
pub async fn import_dataset(
    db: &AppDb,
    name: Option<String>,
    payload: &str,
    source_format: &str,
) -> AppResult<ImportOutcome> {
    let format = source_format.trim().to_ascii_lowercase();
    let parsed = match format.as_str() {
        "geojson" | "json" => parse_geojson_str(payload)
            .map_err(|e| AppError::Parse(format!("invalid GeoJSON: {e}")))?,
        "kml" => crate::gis::kml::parse_kml_str(payload).map_err(AppError::Parse)?,
        "gpx" => crate::gis::gpx::parse_gpx_str(payload).map_err(AppError::Parse)?,
        "shapefile" => {
            let bytes = decode_base64(payload)?;
            parse_shapefile_bytes(&bytes, name.clone())
                .map_err(|e| AppError::Parse(format!("shapefile parsing failed: {e}")))?
        }
        "kmz" => {
            let bytes = decode_base64(payload)?;
            crate::gis::kml::parse_kmz_bytes(&bytes).map_err(AppError::Parse)?
        }
        "gpkg" => {
            let bytes = decode_base64(payload)?;
            crate::gis::gpkg::parse_gpkg_bytes(&bytes)
                .await
                .map_err(AppError::Parse)?
        }
        other => {
            return Err(AppError::Parse(format!(
                "unsupported source format '{other}' (supported: {})",
                SUPPORTED_FORMATS.join(", ")
            )))
        }
    };

    let dataset = persist_parsed(db, parsed, name, &format).await?;
    let layer = provision_layer(db, &dataset, StylePreset::Input).await?;
    Ok(ImportOutcome { dataset, layer })
}

/// Persist a geoprocessing output as a first-class dataset + result layer.
pub async fn add_result_layer(
    db: &AppDb,
    layer_name: String,
    output_geojson: &str,
) -> AppResult<ImportOutcome> {
    let parsed = parse_geojson_str(output_geojson)
        .map_err(|e| AppError::Analysis(format!("tool output rejected: {e}")))?;
    let dataset = persist_parsed(db, parsed, Some(layer_name), "geojson").await?;
    let layer = provision_layer(db, &dataset, StylePreset::Result).await?;
    Ok(ImportOutcome { dataset, layer })
}

async fn persist_parsed(
    db: &AppDb,
    parsed: ParsedGeoData,
    name: Option<String>,
    format: &str,
) -> AppResult<DatasetDetail> {
    if parsed.feature_count == 0 {
        return Err(AppError::Parse("dataset contains no features".into()));
    }

    let id = Uuid::new_v4().to_string();
    let display_name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            format!(
                "{} {}",
                if format == "shapefile" {
                    "Shapefile"
                } else {
                    "Dataset"
                },
                &id[..6]
            )
        });

    let now = Utc::now().to_rfc3339();
    let dataset = DatasetDetail {
        id,
        name: display_name,
        format: format.to_string(),
        feature_count: parsed.feature_count,
        geom_types: parsed.geom_types,
        bbox: parsed.bbox,
        properties_schema: parsed.properties_schema,
        geojson: serde_json::to_string(&parsed.feature_collection)?,
        created_at: now.clone(),
        updated_at: now,
    };
    db.save_dataset(&dataset).await?;
    Ok(dataset)
}

async fn provision_layer(
    db: &AppDb,
    dataset: &DatasetDetail,
    preset: StylePreset,
) -> AppResult<Layer> {
    let layer = Layer {
        id: Uuid::new_v4().to_string(),
        dataset_id: dataset.id.clone(),
        name: dataset.name.clone(),
        is_visible: true,
        opacity: 1.0,
        style: preset.style(),
        z_index: 0,
        created_at: Utc::now().to_rfc3339(),
    };
    db.save_layer(&layer).await?;
    Ok(layer)
}

fn decode_base64(payload: &str) -> AppResult<Vec<u8>> {
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.decode(payload.trim())?)
}
