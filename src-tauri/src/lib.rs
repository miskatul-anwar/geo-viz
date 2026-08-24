pub mod commands;
pub mod db;
pub mod error;
pub mod gis;
pub mod models;
pub mod services;

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests;

use db::AppDb;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir().ok();
            let db = tauri::async_runtime::block_on(async {
                AppDb::init(app_data_dir)
                    .await
                    .expect("Failed to initialize SQLite database")
            });
            app.manage(db);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Ingestion & provisioning
            commands::import_dataset,
            commands::add_result_layer,
            commands::import_raster,
            commands::list_rasters,
            commands::delete_raster,
            // Datasets
            commands::save_dataset,
            commands::list_datasets,
            commands::get_dataset,
            commands::delete_dataset,
            // Layers
            commands::save_layer,
            commands::list_layers,
            commands::delete_layer,
            // Calculation tabs
            commands::save_calculation_tab,
            commands::list_calculation_tabs,
            commands::delete_calculation_tab,
            // SQL console & stats
            commands::execute_sql_query,
            commands::get_database_stats,
            // Geoprocessing tools
            commands::run_buffer_tool,
            commands::run_convex_hull_tool,
            commands::run_centroid_tool,
            commands::run_bounding_box_tool,
            commands::run_simplify_tool,
            commands::run_metrics_tool,
            commands::run_spatial_query_tool,
            commands::run_spatial_binning_tool,
            commands::run_distance_matrix_tool,
            commands::run_random_points_tool,
            commands::run_overlay_tool,
            commands::run_dissolve_tool,
            commands::run_spatial_join_tool,
            // Spatial statistics
            commands::run_mean_center_tool,
            commands::run_median_center_tool,
            commands::run_directional_mean_tool,
            commands::run_morans_i_tool,
            commands::run_getis_ord_tool,
            commands::run_ols_tool,
            // Geostatistics
            commands::run_idw_tool,
            commands::run_kriging_tool,
            // Network
            commands::run_shortest_path_tool,
            commands::run_service_area_tool,
            commands::run_od_matrix_tool,
            // Topology & joins
            commands::run_topology_tool,
            commands::run_join_csv_tool,
            // Raster (Spatial Analyst)
            commands::run_slope_tool,
            commands::run_hillshade_tool,
            commands::run_raster_calculator_tool,
            commands::run_d8_tool,
            commands::run_zonal_stats_tool,
            commands::run_viewshed_tool,
            // Calculation history
            commands::list_calculation_history,
            // Symbology
            commands::compute_class_breaks,
            // Bookmarks
            commands::save_bookmark,
            commands::list_bookmarks,
            commands::delete_bookmark
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
