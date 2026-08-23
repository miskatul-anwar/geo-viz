//! Geoprocessing execution engine.
//!
//! Centralizes dataset resolution, timing, history logging and result
//! assembly; individual tools only contribute a pure compute function.

use crate::db::AppDb;
use crate::error::{AppError, AppResult};
use crate::gis::{
    bbox::calculate_bounding_boxes, buffer::calculate_buffer, centroid::calculate_centroids,
    convex_hull::calculate_convex_hull, distance_matrix::calculate_nearest_neighbors,
    metrics::calculate_metrics, parser::parse_geojson_str, random_points::generate_random_points,
    simplify::simplify_geometries, spatial_binning::calculate_spatial_binning,
    spatial_query::execute_spatial_query,
};
use crate::models::{CalculationHistory, SpatialAnalysisResult};
use chrono::Utc;
use geojson::{FeatureCollection, GeoJson};
use serde::Deserialize;
use std::time::Instant;
use uuid::Uuid;

/// Supported geoprocessing tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Buffer,
    ConvexHull,
    Centroid,
    BoundingBox,
    Simplify,
    Metrics,
    SpatialQuery,
    SpatialBinning,
    DistanceMatrix,
    RandomPoints,
    Overlay,
    Dissolve,
    SpatialJoin,
}

impl ToolKind {
    fn title(self) -> &'static str {
        match self {
            ToolKind::Buffer => "Buffer Analysis",
            ToolKind::ConvexHull => "Convex Hull",
            ToolKind::Centroid => "Centroids / Centers of Mass",
            ToolKind::BoundingBox => "Bounding Box / Envelope",
            ToolKind::Simplify => "Douglas-Peucker Simplification",
            ToolKind::Metrics => "Area, Length & Attribute Metrics",
            ToolKind::SpatialQuery => "Spatial Query & Filter",
            ToolKind::SpatialBinning => "Spatial Binning & Density",
            ToolKind::DistanceMatrix => "Nearest Neighbor Distance",
            ToolKind::RandomPoints => "Random Point Generator",
            ToolKind::Overlay => "Overlay Analysis",
            ToolKind::Dissolve => "Dissolve & Union",
            ToolKind::SpatialJoin => "Spatial Join",
        }
    }

    /// Short key used by the frontend and calculation history log.
    pub fn key(self) -> &'static str {
        match self {
            ToolKind::Buffer => "buffer",
            ToolKind::ConvexHull => "convex_hull",
            ToolKind::Centroid => "centroid",
            ToolKind::BoundingBox => "bbox",
            ToolKind::Simplify => "simplify",
            ToolKind::Metrics => "metrics",
            ToolKind::SpatialQuery => "spatial_query",
            ToolKind::SpatialBinning => "spatial_binning",
            ToolKind::DistanceMatrix => "distance_matrix",
            ToolKind::RandomPoints => "random_points",
            ToolKind::Overlay => "overlay",
            ToolKind::Dissolve => "dissolve",
            ToolKind::SpatialJoin => "spatial_join",
        }
    }
}

/// User-supplied parameters for a tool run. Irrelevant fields are ignored.
#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolParams {
    // Buffer
    pub distance_meters: Option<f64>,
    pub steps: Option<usize>,
    // Convex hull / bounding box
    #[serde(default)]
    pub per_feature: bool,
    // Simplify
    pub tolerance_deg: Option<f64>,
    // Spatial binning
    pub grid_type: Option<String>,
    pub cell_size_km: Option<f64>,
    // Random points
    pub count: Option<usize>,
    #[serde(default)]
    pub restrict_to_polygons: bool,
    // Spatial query
    pub filter_dataset_id: Option<String>,
    pub filter_geojson: Option<String>,
    pub spatial_relation: Option<String>,
    pub attribute_field: Option<String>,
    pub attribute_op: Option<String>,
    pub attribute_val: Option<String>,
    // Overlay / dissolve / spatial join
    pub operation: Option<String>,
    pub group_field: Option<String>,
}

/// Execute a tool end-to-end: resolve inputs, compute, persist history entry
/// (non-fatal on failure) and return the analysis result.
pub async fn run_tool(
    db: &AppDb,
    kind: ToolKind,
    params: ToolParams,
    dataset_id: Option<String>,
    raw_geojson: Option<String>,
    tab_id: String,
) -> AppResult<SpatialAnalysisResult> {
    let started = Instant::now();

    let fc = resolve_fc(db, dataset_id.as_deref(), raw_geojson.as_deref()).await?;
    let filter_fc = resolve_filter_fc(db, kind, &params).await?;
    let (out_fc, summary) = compute(kind, &fc, filter_fc.as_ref(), &params)?;
    let elapsed = started.elapsed().as_millis() as i64;

    let layer_name = layer_name_for(kind, &params, out_fc.features.len());
    let result = SpatialAnalysisResult {
        tool_name: kind.title().to_string(),
        layer_name,
        output_geojson: serde_json::to_string(&out_fc)?,
        feature_count: out_fc.features.len(),
        execution_time_ms: elapsed,
        summary_metrics: summary.clone(),
    };

    // History logging is best-effort: never fail an already-successful run.
    let _ = db
        .log_calculation(&CalculationHistory {
            id: Uuid::new_v4().to_string(),
            tab_id,
            tool_name: kind.key().to_string(),
            parameters_json: serde_json::to_value(&params)
                .unwrap_or(serde_json::Value::Null)
                .to_string(),
            result_summary_json: summary.to_string(),
            execution_time_ms: elapsed,
            created_at: Utc::now().to_rfc3339(),
        })
        .await;

    Ok(result)
}

/// Load input data either from inline GeoJSON or from a persisted dataset.
async fn resolve_fc(
    db: &AppDb,
    dataset_id: Option<&str>,
    raw_geojson: Option<&str>,
) -> AppResult<FeatureCollection> {
    if let Some(raw) = raw_geojson {
        if !raw.trim().is_empty() {
            return parse_fc(raw);
        }
    }
    if let Some(id) = dataset_id {
        if !id.trim().is_empty() {
            if let Some(detail) = db.get_dataset_detail(id).await? {
                return parse_fc(&detail.geojson);
            }
            return Err(AppError::Parse(format!("dataset '{id}' was not found")));
        }
    }
    Err(AppError::Parse(
        "no dataset ID or GeoJSON payload provided".into(),
    ))
}

/// Secondary inputs are resolved eagerly whenever supplied: spatial-query
/// masks, distance-matrix targets, overlay boundaries and join targets all
/// use these fields.
async fn resolve_filter_fc(
    db: &AppDb,
    kind: ToolKind,
    params: &ToolParams,
) -> AppResult<Option<FeatureCollection>> {
    if !matches!(
        kind,
        ToolKind::SpatialQuery
            | ToolKind::DistanceMatrix
            | ToolKind::Overlay
            | ToolKind::SpatialJoin
    ) {
        return Ok(None);
    }
    if let Some(raw) = params.filter_geojson.as_deref() {
        if !raw.trim().is_empty() {
            return parse_fc(raw).map(Some);
        }
    }
    if let Some(id) = params.filter_dataset_id.as_deref() {
        if !id.trim().is_empty() {
            return match db.get_dataset_detail(id).await? {
                Some(d) => parse_fc(&d.geojson).map(Some),
                None => Err(AppError::Parse(format!(
                    "filter dataset '{id}' was not found"
                ))),
            };
        }
    }
    Ok(None)
}

fn parse_fc(raw: &str) -> AppResult<FeatureCollection> {
    match raw.parse::<GeoJson>() {
        Ok(GeoJson::FeatureCollection(fc)) => Ok(fc),
        Ok(_) => parse_geojson_str(raw)
            .map(|p| p.feature_collection)
            .map_err(|e| AppError::Parse(format!("invalid GeoJSON: {e}"))),
        Err(e) => Err(AppError::Parse(format!("invalid GeoJSON: {e}"))),
    }
}

fn compute(
    kind: ToolKind,
    fc: &FeatureCollection,
    filter_fc: Option<&FeatureCollection>,
    p: &ToolParams,
) -> AppResult<(FeatureCollection, serde_json::Value)> {
    let run = || -> Result<_, String> {
        match kind {
            ToolKind::Buffer => {
                let distance = p.distance_meters.unwrap_or(50_000.0);
                let steps = p.steps.unwrap_or(16);
                calculate_buffer(fc, distance, steps)
            }
            ToolKind::ConvexHull => calculate_convex_hull(fc, p.per_feature),
            ToolKind::Centroid => calculate_centroids(fc),
            ToolKind::BoundingBox => calculate_bounding_boxes(fc, p.per_feature),
            ToolKind::Simplify => simplify_geometries(fc, p.tolerance_deg.unwrap_or(0.05)),
            ToolKind::Metrics => calculate_metrics(fc),
            ToolKind::SpatialQuery => execute_spatial_query(
                fc,
                filter_fc,
                p.spatial_relation.as_deref().unwrap_or("intersects"),
                p.attribute_field.as_deref(),
                p.attribute_op.as_deref(),
                p.attribute_val.as_deref(),
            ),
            ToolKind::SpatialBinning => calculate_spatial_binning(
                fc,
                p.grid_type.as_deref().unwrap_or("hexbin"),
                p.cell_size_km.unwrap_or(100.0),
            ),
            ToolKind::DistanceMatrix => calculate_nearest_neighbors(fc, filter_fc),
            ToolKind::RandomPoints => {
                generate_random_points(fc, p.count.unwrap_or(100), p.restrict_to_polygons)
            }
            ToolKind::Overlay => {
                let mask =
                    filter_fc.ok_or_else(|| "overlay requires a boundary dataset".to_string())?;
                let op = p.operation.as_deref().unwrap_or("intersection");
                if op == "clip" {
                    crate::gis::overlay::run_clip(fc, mask)
                } else {
                    crate::gis::overlay::run_overlay(fc, mask, op)
                }
            }
            ToolKind::Dissolve => crate::gis::overlay::run_dissolve(fc, p.group_field.as_deref()),
            ToolKind::SpatialJoin => {
                let target = filter_fc
                    .ok_or_else(|| "spatial join requires a target dataset".to_string())?;
                crate::gis::spatial_join::run_spatial_join(fc, target)
            }
        }
    };
    run().map_err(AppError::Analysis)
}

fn layer_name_for(kind: ToolKind, p: &ToolParams, count: usize) -> String {
    match kind {
        ToolKind::Buffer => format!("Buffer ({}m)", p.distance_meters.unwrap_or(50_000.0)),
        ToolKind::ConvexHull => {
            if p.per_feature {
                "Feature Convex Hulls".into()
            } else {
                "Layer Convex Hull".into()
            }
        }
        ToolKind::Centroid => "Feature Centroids".into(),
        ToolKind::BoundingBox => {
            if p.per_feature {
                "Feature Envelopes".into()
            } else {
                "Layer Envelope".into()
            }
        }
        ToolKind::Simplify => format!("Simplified (tol: {})", p.tolerance_deg.unwrap_or(0.05)),
        ToolKind::Metrics => "Enriched Metrics Layer".into(),
        ToolKind::SpatialQuery => "Query Result Layer".into(),
        ToolKind::SpatialBinning => format!(
            "{} Bins ({}km)",
            if p.grid_type.as_deref() == Some("square") {
                "Square"
            } else {
                "Hexagonal"
            },
            p.cell_size_km.unwrap_or(100.0)
        ),
        ToolKind::DistanceMatrix => "Nearest Distance Vectors".into(),
        ToolKind::RandomPoints => format!("Random Points ({count})"),
        ToolKind::Overlay => format!(
            "{} Result",
            match p.operation.as_deref() {
                Some("difference") => "Difference",
                Some("symmetric_difference") => "Symmetric Difference",
                Some("clip") => "Clipped",
                _ => "Intersection",
            }
        ),
        ToolKind::Dissolve => "Dissolved Boundaries".into(),
        ToolKind::SpatialJoin => "Spatially Joined Layer".into(),
    }
}
