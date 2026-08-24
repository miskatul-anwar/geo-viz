//! Tauri IPC command layer.
//!
//! Commands are deliberately thin adapters: all orchestration lives in the
//! service modules, all persistence in `db::AppDb`.

use crate::db::AppDb;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::services::{
    dataset_service,
    tool_service::{self, ToolKind, ToolParams},
};
use tauri::State;

// ---------------------------------------------------------------------------
// Ingestion & provisioning (backend-owned pipeline)
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "snake_case")]
pub async fn import_dataset(
    state: State<'_, AppDb>,
    name: Option<String>,
    payload: String,
    source_format: String,
) -> AppResult<dataset_service::ImportOutcome> {
    dataset_service::import_dataset(&state, name, &payload, &source_format).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn add_result_layer(
    state: State<'_, AppDb>,
    result: SpatialAnalysisResult,
) -> AppResult<Layer> {
    let outcome =
        dataset_service::add_result_layer(&state, result.layer_name, &result.output_geojson)
            .await?;
    Ok(outcome.layer)
}

// ---------------------------------------------------------------------------
// Datasets
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "snake_case")]
pub async fn save_dataset(state: State<'_, AppDb>, dataset: DatasetDetail) -> AppResult<()> {
    state.save_dataset(&dataset).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_datasets(state: State<'_, AppDb>) -> AppResult<Vec<DatasetSummary>> {
    state.list_datasets().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_dataset(state: State<'_, AppDb>, id: String) -> AppResult<Option<DatasetDetail>> {
    state.get_dataset_detail(&id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_dataset(state: State<'_, AppDb>, id: String) -> AppResult<bool> {
    state.delete_dataset(&id).await
}

// ---------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "snake_case")]
pub async fn save_layer(state: State<'_, AppDb>, layer: Layer) -> AppResult<()> {
    state.save_layer(&layer).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_layers(state: State<'_, AppDb>) -> AppResult<Vec<Layer>> {
    state.list_layers().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_layer(state: State<'_, AppDb>, id: String) -> AppResult<bool> {
    state.delete_layer(&id).await
}

// ---------------------------------------------------------------------------
// Calculation tabs
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "snake_case")]
pub async fn save_calculation_tab(state: State<'_, AppDb>, tab: CalculationTab) -> AppResult<()> {
    state.save_tab(&tab).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_calculation_tabs(state: State<'_, AppDb>) -> AppResult<Vec<CalculationTab>> {
    state.list_tabs().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_calculation_tab(state: State<'_, AppDb>, id: String) -> AppResult<bool> {
    state.delete_tab(&id).await
}

// ---------------------------------------------------------------------------
// SQL console & stats
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "snake_case")]
pub async fn execute_sql_query(state: State<'_, AppDb>, sql: String) -> AppResult<SqlQueryResult> {
    state.execute_sql_query(&sql).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_database_stats(state: State<'_, AppDb>) -> AppResult<DatabaseStats> {
    state.get_stats().await
}

// ---------------------------------------------------------------------------
// Geoprocessing tools (thin wrappers over `tool_service::run_tool`)
// ---------------------------------------------------------------------------

macro_rules! tool_command {
    ($name:ident { $($arg:ident : $ty:ty),* } => $kind:expr, $params:expr) => {
        #[tauri::command(rename_all = "snake_case")]
        pub async fn $name(
            state: State<'_, AppDb>,
            dataset_id: Option<String>,
            raw_geojson: Option<String>,
            tab_id: String,
            $($arg: $ty,)*
        ) -> AppResult<SpatialAnalysisResult> {
            tool_service::run_tool(&state, $kind, $params, dataset_id, raw_geojson, tab_id).await
        }
    };
}

tool_command!(run_buffer_tool { distance_meters: f64, steps: usize } => ToolKind::Buffer,
    ToolParams { distance_meters: Some(distance_meters), steps: Some(steps), ..Default::default() });

tool_command!(run_convex_hull_tool { per_feature: bool } => ToolKind::ConvexHull,
    ToolParams { per_feature, ..Default::default() });

tool_command!(run_centroid_tool { } => ToolKind::Centroid, ToolParams::default());

tool_command!(run_bounding_box_tool { per_feature: bool } => ToolKind::BoundingBox,
    ToolParams { per_feature, ..Default::default() });

tool_command!(run_simplify_tool { tolerance_deg: f64 } => ToolKind::Simplify,
    ToolParams { tolerance_deg: Some(tolerance_deg), ..Default::default() });

tool_command!(run_metrics_tool { } => ToolKind::Metrics, ToolParams::default());

tool_command!(run_spatial_binning_tool { grid_type: String, cell_size_km: f64 } => ToolKind::SpatialBinning,
    ToolParams { grid_type: Some(grid_type), cell_size_km: Some(cell_size_km), ..Default::default() });

tool_command!(run_random_points_tool { count: usize, restrict_to_polygons: bool } => ToolKind::RandomPoints,
    ToolParams { count: Some(count), restrict_to_polygons, ..Default::default() });

/// Spatial query keeps its richer signature because of its secondary mask input.
#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn run_spatial_query_tool(
    state: State<'_, AppDb>,
    source_dataset_id: Option<String>,
    source_geojson: Option<String>,
    filter_dataset_id: Option<String>,
    filter_geojson: Option<String>,
    spatial_relation: String,
    attribute_field: Option<String>,
    attribute_op: Option<String>,
    attribute_val: Option<String>,
    tab_id: String,
) -> AppResult<SpatialAnalysisResult> {
    tool_service::run_tool(
        &state,
        ToolKind::SpatialQuery,
        ToolParams {
            filter_dataset_id,
            filter_geojson,
            spatial_relation: Some(spatial_relation),
            attribute_field,
            attribute_op,
            attribute_val,
            ..Default::default()
        },
        source_dataset_id,
        source_geojson,
        tab_id,
    )
    .await
}

/// Distance matrix treats its secondary input as the optional target set.
#[tauri::command(rename_all = "snake_case")]
pub async fn run_distance_matrix_tool(
    state: State<'_, AppDb>,
    source_dataset_id: Option<String>,
    source_geojson: Option<String>,
    target_dataset_id: Option<String>,
    target_geojson: Option<String>,
    tab_id: String,
) -> AppResult<SpatialAnalysisResult> {
    tool_service::run_tool(
        &state,
        ToolKind::DistanceMatrix,
        ToolParams {
            filter_dataset_id: target_dataset_id,
            filter_geojson: target_geojson,
            ..Default::default()
        },
        source_dataset_id,
        source_geojson,
        tab_id,
    )
    .await
}

/// Overlay analysis: intersection / difference / symmetric difference / clip
/// against a boundary dataset supplied as the secondary input.
#[tauri::command(rename_all = "snake_case")]
pub async fn run_overlay_tool(
    state: State<'_, AppDb>,
    dataset_id: Option<String>,
    raw_geojson: Option<String>,
    overlay_dataset_id: Option<String>,
    operation: String,
    tab_id: String,
) -> AppResult<SpatialAnalysisResult> {
    tool_service::run_tool(
        &state,
        ToolKind::Overlay,
        ToolParams {
            filter_dataset_id: overlay_dataset_id,
            operation: Some(operation),
            ..Default::default()
        },
        dataset_id,
        raw_geojson,
        tab_id,
    )
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn run_dissolve_tool(
    state: State<'_, AppDb>,
    dataset_id: Option<String>,
    raw_geojson: Option<String>,
    group_field: Option<String>,
    tab_id: String,
) -> AppResult<SpatialAnalysisResult> {
    tool_service::run_tool(
        &state,
        ToolKind::Dissolve,
        ToolParams {
            group_field,
            ..Default::default()
        },
        dataset_id,
        raw_geojson,
        tab_id,
    )
    .await
}

/// Spatial join attaches attributes of the polygon target layer onto the source.
#[tauri::command(rename_all = "snake_case")]
pub async fn run_spatial_join_tool(
    state: State<'_, AppDb>,
    dataset_id: Option<String>,
    raw_geojson: Option<String>,
    join_dataset_id: Option<String>,
    tab_id: String,
) -> AppResult<SpatialAnalysisResult> {
    tool_service::run_tool(
        &state,
        ToolKind::SpatialJoin,
        ToolParams {
            filter_dataset_id: join_dataset_id,
            ..Default::default()
        },
        dataset_id,
        raw_geojson,
        tab_id,
    )
    .await
}

// ---------------------------------------------------------------------------
// Symbology: attribute classification for categorized/graduated rendering
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "snake_case")]
pub async fn compute_class_breaks(
    state: State<'_, AppDb>,
    dataset_id: String,
    field: String,
    method: String,
    n_classes: usize,
) -> AppResult<Vec<crate::gis::classification::ClassBreak>> {
    let detail = state
        .get_dataset_detail(&dataset_id)
        .await?
        .ok_or_else(|| AppError::Parse(format!("dataset '{dataset_id}' was not found")))?;

    let fc = crate::gis::parser::parse_geojson_str(&detail.geojson)
        .map_err(|e| AppError::Parse(format!("stored dataset is invalid: {e}")))?;

    let values = crate::gis::classification::numeric_values(&fc.feature_collection, &field);
    let parsed_method = crate::gis::classification::ClassificationMethod::parse(&method)
        .map_err(AppError::Parse)?;
    crate::gis::classification::compute_breaks(&values, parsed_method, n_classes)
        .map_err(AppError::Analysis)
}

// ---------------------------------------------------------------------------
// Spatial bookmarks
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "snake_case")]
pub async fn save_bookmark(state: State<'_, AppDb>, bookmark: MapBookmark) -> AppResult<()> {
    state.save_bookmark(&bookmark).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_bookmarks(state: State<'_, AppDb>) -> AppResult<Vec<MapBookmark>> {
    state.list_bookmarks().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_bookmark(state: State<'_, AppDb>, id: String) -> AppResult<bool> {
    state.delete_bookmark(&id).await
}
