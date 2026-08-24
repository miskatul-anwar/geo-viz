//! Geoprocessing execution engine.
//!
//! Centralizes dataset resolution, timing, history logging and result
//! assembly; individual tools only contribute a pure compute function.

use crate::db::AppDb;
use crate::error::{AppError, AppResult};
use crate::gis::raster::RasterGrid;
use crate::gis::{
    bbox::calculate_bounding_boxes, buffer::calculate_buffer, centroid::calculate_centroids,
    convex_hull::calculate_convex_hull, distance_matrix::calculate_nearest_neighbors,
    geostatistics, metrics::calculate_metrics, network, parser::parse_geojson_str,
    random_points::generate_random_points, raster, simplify::simplify_geometries,
    spatial_binning::calculate_spatial_binning, spatial_query::execute_spatial_query,
    spatial_statistics, table_join, topology,
};
use crate::models::{CalculationHistory, RasterSummary, SpatialAnalysisResult};
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
    MeanCenter,
    MedianCenter,
    DirectionalMean,
    MoransI,
    GetisOrd,
    OlsRegression,
    Idw,
    Kriging,
    ShortestPath,
    ServiceArea,
    OdMatrix,
    TopologyCheck,
    JoinCsv,
    Slope,
    Hillshade,
    RasterCalculator,
    D8Flow,
    ZonalStats,
    Viewshed,
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
            ToolKind::MeanCenter => "Mean Center",
            ToolKind::MedianCenter => "Median Center",
            ToolKind::DirectionalMean => "Linear Directional Mean",
            ToolKind::MoransI => "Global Moran's I",
            ToolKind::GetisOrd => "Hot Spot Analysis (Getis-Ord Gi*)",
            ToolKind::OlsRegression => "OLS Regression",
            ToolKind::Idw => "IDW Interpolation",
            ToolKind::Kriging => "Ordinary Kriging",
            ToolKind::ShortestPath => "Shortest Path (Dijkstra / A*)",
            ToolKind::ServiceArea => "Service Area",
            ToolKind::OdMatrix => "OD Cost Matrix",
            ToolKind::TopologyCheck => "Topology Validation",
            ToolKind::JoinCsv => "CSV Attribute Join",
            ToolKind::Slope => "Slope / Aspect / Hillshade",
            ToolKind::Hillshade => "Hillshade",
            ToolKind::RasterCalculator => "Raster Calculator",
            ToolKind::D8Flow => "D8 Flow Direction & Accumulation",
            ToolKind::ZonalStats => "Zonal Statistics",
            ToolKind::Viewshed => "Viewshed",
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
            ToolKind::MeanCenter => "mean_center",
            ToolKind::MedianCenter => "median_center",
            ToolKind::DirectionalMean => "directional_mean",
            ToolKind::MoransI => "morans_i",
            ToolKind::GetisOrd => "getis_ord",
            ToolKind::OlsRegression => "ols_regression",
            ToolKind::Idw => "idw",
            ToolKind::Kriging => "kriging",
            ToolKind::ShortestPath => "shortest_path",
            ToolKind::ServiceArea => "service_area",
            ToolKind::OdMatrix => "od_matrix",
            ToolKind::TopologyCheck => "topology_check",
            ToolKind::JoinCsv => "join_csv",
            ToolKind::Slope => "slope",
            ToolKind::Hillshade => "hillshade",
            ToolKind::RasterCalculator => "raster_calculator",
            ToolKind::D8Flow => "d8_flow",
            ToolKind::ZonalStats => "zonal_stats",
            ToolKind::Viewshed => "viewshed",
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
    // Spatial statistics
    pub statistic: Option<String>,
    pub explanatory_csv: Option<String>,
    pub band_meters: Option<f64>,
    // Geostatistics
    pub idw_power: Option<f64>,
    pub max_neighbors: Option<usize>,
    pub variogram_model: Option<String>,
    // Network
    pub start_lng: Option<f64>,
    pub start_lat: Option<f64>,
    pub end_lng: Option<f64>,
    pub end_lat: Option<f64>,
    pub algorithm: Option<String>,
    pub max_distance_m: Option<f64>,
    pub target_dataset_id: Option<String>,
    // Table join
    pub key_field: Option<String>,
    pub csv_key: Option<String>,
    pub csv_text: Option<String>,
    // Raster analysis
    pub raster_id: Option<String>,
    pub second_raster_id: Option<String>,
    pub expression: Option<String>,
    pub azimuth: Option<f64>,
    pub altitude: Option<f64>,
    pub observer_lng: Option<f64>,
    pub observer_lat: Option<f64>,
    pub observer_height_m: Option<f64>,
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

    // Raster tools resolve a stored grid instead of a vector dataset.
    if is_raster_kind(kind) {
        return run_raster_tool(db, kind, params, tab_id, started).await;
    }

    let fc = resolve_fc(db, dataset_id.as_deref(), raw_geojson.as_deref()).await?;
    let filter_fc = resolve_filter_fc(db, kind, &params).await?;
    // OD destinations are a third input; resolve them in async context.
    let destinations = resolve_destinations_fc(db, kind, &params).await?;
    let (out_fc, summary) = compute(kind, &fc, filter_fc, destinations, &params).await?;
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
/// masks, distance-matrix targets, overlay boundaries, join targets,
/// topology covers, OD origins and zonal polygons all use these fields.
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
            | ToolKind::TopologyCheck
            | ToolKind::OdMatrix
            | ToolKind::ZonalStats
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

/// OD destinations are a third input resolved ahead of the sync compute.
async fn resolve_destinations_fc(
    db: &AppDb,
    kind: ToolKind,
    p: &ToolParams,
) -> AppResult<Option<FeatureCollection>> {
    if kind != ToolKind::OdMatrix {
        return Ok(None);
    }
    let Some(id) = p
        .target_dataset_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    else {
        return Ok(None);
    };
    match db.get_dataset_detail(id).await? {
        Some(d) => parse_fc(&d.geojson).map(Some),
        None => Err(AppError::Parse(format!(
            "destination dataset '{id}' was not found"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Raster (Spatial Analyst) tool execution
// ---------------------------------------------------------------------------

fn is_raster_kind(kind: ToolKind) -> bool {
    matches!(
        kind,
        ToolKind::Slope
            | ToolKind::Hillshade
            | ToolKind::RasterCalculator
            | ToolKind::D8Flow
            | ToolKind::ZonalStats
            | ToolKind::Viewshed
    )
}

fn grid_from_payload(payload: crate::models::RasterPayload) -> RasterGrid {
    RasterGrid {
        width: payload.summary.width,
        height: payload.summary.height,
        data: payload.data,
        nodata: None,
        bbox: (
            payload.summary.bbox.first().copied().unwrap_or(0.0),
            payload.summary.bbox.get(1).copied().unwrap_or(0.0),
            payload.summary.bbox.get(2).copied().unwrap_or(1.0),
            payload.summary.bbox.get(3).copied().unwrap_or(1.0),
        ),
    }
}

async fn load_raster(db: &AppDb, id: Option<&str>, label: &str) -> AppResult<RasterGrid> {
    let id = id.ok_or_else(|| {
        AppError::Parse(format!("{label} requires a raster (import a .tif first)"))
    })?;
    db.get_raster(id)
        .await?
        .map(grid_from_payload)
        .ok_or_else(|| AppError::Parse(format!("raster '{id}' was not found")))
}

async fn run_raster_tool(
    db: &AppDb,
    kind: ToolKind,
    p: ToolParams,
    tab_id: String,
    started: Instant,
) -> AppResult<SpatialAnalysisResult> {
    let (out_fc, summary, layer_name) = match kind {
        ToolKind::Slope | ToolKind::Hillshade => {
            let grid = load_raster(db, p.raster_id.as_deref(), "surface analysis").await?;
            let mut result_grid = match kind {
                ToolKind::Slope => raster::slope_degrees(&grid),
                _ => raster::hillshade(
                    &grid,
                    p.azimuth.unwrap_or(315.0),
                    p.altitude.unwrap_or(45.0),
                ),
            };
            result_grid.bbox = grid.bbox;
            let (count, min, max, mean) = raster::grid_summary(&result_grid);
            let name = if kind == ToolKind::Slope {
                "Slope (deg)"
            } else {
                "Hillshade"
            };
            let out = raster::grid_to_points(&result_grid, "value", 20_000);
            let summary = serde_json::json!({
                "product": name,
                "raster": p.raster_id,
                "cells": grid.width * grid.height,
                "min": (min * 1000.0).round() / 1000.0,
                "max": (max * 1000.0).round() / 1000.0,
                "mean": (mean * 1000.0).round() / 1000.0,
                "displayed_points": out.features.len(),
                "sampled_of": count,
                "azimuth": if kind == ToolKind::Hillshade { serde_json::json!(p.azimuth.unwrap_or(315.0)) } else { serde_json::Value::Null },
                "altitude": if kind == ToolKind::Hillshade { serde_json::json!(p.altitude.unwrap_or(45.0)) } else { serde_json::Value::Null }
            });
            (out, summary, name.to_string())
        }
        ToolKind::RasterCalculator => {
            let a = load_raster(db, p.raster_id.as_deref(), "raster calculator").await?;
            let b = match p.second_raster_id.as_deref().filter(|s| !s.is_empty()) {
                Some(_) => {
                    Some(load_raster(db, p.second_raster_id.as_deref(), "raster calculator").await?)
                }
                None => None,
            };
            let expr = p.expression.clone().unwrap_or_else(|| "a * 1.0".into());
            let mut result_grid =
                raster::raster_calculator(&expr, &a, b.as_ref()).map_err(AppError::Analysis)?;
            result_grid.bbox = a.bbox;
            let (_, min, max, mean) = raster::grid_summary(&result_grid);
            let out = raster::grid_to_points(&result_grid, "value", 20_000);
            let summary = serde_json::json!({
                "expression": expr,
                "cells": a.width * a.height,
                "min": (min * 1000.0).round() / 1000.0,
                "max": (max * 1000.0).round() / 1000.0,
                "mean": (mean * 1000.0).round() / 1000.0,
                "displayed_points": out.features.len()
            });
            (out, summary, format!("Map Algebra: {expr}"))
        }
        ToolKind::D8Flow => {
            let grid = load_raster(db, p.raster_id.as_deref(), "D8 flow").await?;
            let dirs = raster::d8_flow_direction(&grid);
            let acc = raster::flow_accumulation(&grid).map_err(AppError::Analysis)?;
            let mut dirs_out = dirs.clone();
            dirs_out.bbox = grid.bbox;
            let mut acc_out = acc.clone();
            acc_out.bbox = grid.bbox;
            let (_, acc_max, _, acc_mean) = raster::grid_summary(&acc);
            let mut out = raster::grid_to_points(&dirs_out, "flow_dir_code", 10_000);
            let acc_points = raster::grid_to_points(&acc_out, "accumulation", 10_000);
            out.features.extend(acc_points.features);
            let summary = serde_json::json!({
                "cells": grid.width * grid.height,
                "max_accumulation_cells": (acc_max * 10.0).round() / 10.0,
                "mean_accumulation_cells": (acc_mean * 10.0).round() / 10.0,
                "displayed_points": out.features.len()
            });
            (out, summary, "D8 Flow & Accumulation".to_string())
        }
        ToolKind::ZonalStats => {
            let grid = load_raster(db, p.raster_id.as_deref(), "zonal statistics").await?;
            let polygons = resolve_filter_fc(db, ToolKind::ZonalStats, &p)
                .await?
                .ok_or_else(|| {
                    AppError::Parse("zonal statistics requires a polygon layer".into())
                })?;
            let (out, summary) =
                raster::zonal_statistics(&grid, &polygons).map_err(AppError::Analysis)?;
            (out, summary, "Zonal Statistics".to_string())
        }
        ToolKind::Viewshed => {
            let grid = load_raster(db, p.raster_id.as_deref(), "viewshed").await?;
            let result_grid = raster::viewshed(
                &grid,
                p.observer_lng
                    .ok_or_else(|| AppError::Parse("observer_lng is required".into()))?,
                p.observer_lat
                    .ok_or_else(|| AppError::Parse("observer_lat is required".into()))?,
                p.observer_height_m.unwrap_or(10.0),
            );
            let visible = result_grid.data.iter().filter(|&&v| v == 1.0).count();
            let mut vs_out = result_grid.clone();
            vs_out.bbox = grid.bbox;
            let out = raster::grid_to_points(&vs_out, "visible", 20_000);
            let summary = serde_json::json!({
                "observer": [p.observer_lng, p.observer_lat],
                "observer_height_m": p.observer_height_m.unwrap_or(10.0),
                "visible_cells": visible,
                "total_cells": grid.width * grid.height,
                "visible_percent": ((visible as f64 / (grid.width * grid.height) as f64) * 1000.0).round() / 10.0,
                "displayed_points": out.features.len()
            });
            (out, summary, "Viewshed".to_string())
        }
        _ => return Err(AppError::Analysis("unsupported raster tool".into())),
    };

    let elapsed = started.elapsed().as_millis() as i64;
    let result = SpatialAnalysisResult {
        tool_name: kind.title().to_string(),
        layer_name,
        output_geojson: serde_json::to_string(&out_fc)?,
        feature_count: out_fc.features.len(),
        execution_time_ms: elapsed,
        summary_metrics: summary.clone(),
    };
    let _ = db
        .log_calculation(&CalculationHistory {
            id: Uuid::new_v4().to_string(),
            tab_id,
            tool_name: kind.key().to_string(),
            parameters_json: serde_json::to_value(&p)
                .unwrap_or(serde_json::Value::Null)
                .to_string(),
            result_summary_json: summary.to_string(),
            execution_time_ms: elapsed,
            created_at: Utc::now().to_rfc3339(),
        })
        .await;
    Ok(result)
}

/// Import a GeoTIFF into the raster store for Spatial Analyst tools.
pub async fn import_raster(
    db: &AppDb,
    name: Option<String>,
    tiff_bytes: &[u8],
) -> AppResult<RasterSummary> {
    let grid = raster::parse_geotiff(tiff_bytes).map_err(AppError::Analysis)?;
    let (cell_w, cell_h) = grid.cell_size();
    let lat_center = (grid.bbox.1 + grid.bbox.3) / 2.0;
    let cell_size_m = (cell_w * 111_320.0 * lat_center.cos().max(0.01)).max(cell_h * 110_540.0);
    let summary = RasterSummary {
        id: Uuid::new_v4().to_string(),
        name: name.unwrap_or_else(|| "DEM.tif".to_string()),
        width: grid.width,
        height: grid.height,
        cell_size_m: (cell_size_m * 100.0).round() / 100.0,
        bbox: vec![grid.bbox.0, grid.bbox.1, grid.bbox.2, grid.bbox.3],
        created_at: Utc::now().to_rfc3339(),
    };
    db.save_raster(&summary, &grid.data).await?;
    Ok(summary)
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

async fn compute(
    kind: ToolKind,
    fc: &FeatureCollection,
    filter_fc: Option<FeatureCollection>,
    destinations: Option<FeatureCollection>,
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
                filter_fc.as_ref(),
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
            ToolKind::DistanceMatrix => calculate_nearest_neighbors(fc, filter_fc.as_ref()),
            ToolKind::RandomPoints => {
                generate_random_points(fc, p.count.unwrap_or(100), p.restrict_to_polygons)
            }
            ToolKind::Overlay => {
                let mask = filter_fc
                    .as_ref()
                    .ok_or_else(|| "overlay requires a boundary dataset".to_string())?;
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
                    .as_ref()
                    .ok_or_else(|| "spatial join requires a target dataset".to_string())?;
                crate::gis::spatial_join::run_spatial_join(fc, target)
            }
            // --- Spatial Statistics ---
            ToolKind::MeanCenter => spatial_statistics::mean_center(fc),
            ToolKind::MedianCenter => spatial_statistics::median_center(fc),
            ToolKind::DirectionalMean => spatial_statistics::linear_directional_mean(fc),
            ToolKind::MoransI => spatial_statistics::morans_i(
                fc,
                p.attribute_field
                    .as_deref()
                    .ok_or("a numeric attribute field is required")?,
            ),
            ToolKind::GetisOrd => spatial_statistics::getis_ord_gi(
                fc,
                p.attribute_field
                    .as_deref()
                    .ok_or("a numeric attribute field is required")?,
                p.band_meters,
            ),
            ToolKind::OlsRegression => spatial_statistics::ols_regression(
                fc,
                p.attribute_field
                    .as_deref()
                    .ok_or("a dependent field is required")?,
                &p.explanatory_csv
                    .as_deref()
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect::<Vec<_>>(),
            ),
            // --- Geostatistics ---
            ToolKind::Idw => geostatistics::inverse_distance_weighting(
                fc,
                p.attribute_field
                    .as_deref()
                    .ok_or("a value field is required")?,
                p.idw_power.unwrap_or(2.0),
                p.cell_size_km.unwrap_or(50.0),
                p.max_neighbors.unwrap_or(12),
            ),
            ToolKind::Kriging => geostatistics::ordinary_kriging(
                fc,
                p.attribute_field
                    .as_deref()
                    .ok_or("a value field is required")?,
                p.variogram_model.as_deref(),
                p.cell_size_km.unwrap_or(50.0),
                p.max_neighbors.unwrap_or(12),
            ),
            // --- Network ---
            ToolKind::ShortestPath => network::shortest_path(
                fc,
                p.start_lng.ok_or("start_lng is required")?,
                p.start_lat.ok_or("start_lat is required")?,
                p.end_lng.ok_or("end_lng is required")?,
                p.end_lat.ok_or("end_lat is required")?,
                p.algorithm.as_deref().unwrap_or("dijkstra"),
            ),
            ToolKind::ServiceArea => network::service_area(
                fc,
                p.start_lng.ok_or("start_lng is required")?,
                p.start_lat.ok_or("start_lat is required")?,
                p.max_distance_m.unwrap_or(10_000.0),
            ),
            ToolKind::OdMatrix => {
                let origins =
                    filter_fc.ok_or_else(|| "an origins layer is required".to_string())?;
                let dest =
                    destinations.ok_or_else(|| "a destinations layer is required".to_string())?;
                network::od_cost_matrix(fc, &origins, &dest)
            }
            // --- Topology & joins ---
            ToolKind::TopologyCheck => topology::validate_topology(
                fc,
                p.operation.as_deref().unwrap_or("must_not_overlap"),
                filter_fc.as_ref(),
            ),
            ToolKind::JoinCsv => table_join::join_csv(
                fc,
                p.key_field.as_deref().ok_or("key_field is required")?,
                p.csv_text.as_deref().ok_or("csv_text is required")?,
                p.csv_key
                    .as_deref()
                    .unwrap_or(p.key_field.as_deref().unwrap_or("id")),
            ),
            _ => Err("unsupported tool".into()),
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
        ToolKind::MeanCenter => "Mean Center".into(),
        ToolKind::MedianCenter => "Median Center".into(),
        ToolKind::DirectionalMean => "Linear Directional Mean".into(),
        ToolKind::MoransI => "Moran's I Input Layer".into(),
        ToolKind::GetisOrd => "Hot Spot Analysis (Gi*)".into(),
        ToolKind::OlsRegression => "OLS Regression Residuals".into(),
        ToolKind::Idw => "IDW Prediction Surface".into(),
        ToolKind::Kriging => "Kriging Prediction Surface".into(),
        ToolKind::ShortestPath => "Shortest Path Route".into(),
        ToolKind::ServiceArea => "Service Area".into(),
        ToolKind::OdMatrix => "OD Cost Matrix Links".into(),
        ToolKind::TopologyCheck => "Topology Violations".into(),
        ToolKind::JoinCsv => "CSV-Joined Layer".into(),
        ToolKind::Slope => "Slope Surface".into(),
        ToolKind::Hillshade => "Hillshade Surface".into(),
        ToolKind::RasterCalculator => "Map Algebra Result".into(),
        ToolKind::D8Flow => "D8 Flow Grid".into(),
        ToolKind::ZonalStats => "Zonal Statistics".into(),
        ToolKind::Viewshed => "Viewshed".into(),
    }
}
