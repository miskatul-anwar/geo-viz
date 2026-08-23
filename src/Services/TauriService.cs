using GeoViz.Models;
using Microsoft.JSInterop;

namespace GeoViz.Services;

/// <summary>
/// Thin typed wrapper around the Tauri IPC bridge. Contains no business logic:
/// orchestration lives in <see cref="AppState"/> on the frontend and in the Rust
/// service layer on the backend.
/// </summary>
public sealed class TauriService
{
    private readonly IJSRuntime _js;

    public TauriService(IJSRuntime js)
    {
        _js = js;
    }

    internal async Task<T> InvokeAsync<T>(string cmd, object? args = null)
    {
        return args is null
            ? await _js.InvokeAsync<T>("tauriInvoke", cmd)
            : await _js.InvokeAsync<T>("tauriInvoke", cmd, args);
    }

    internal async Task InvokeVoidAsync(string cmd, object? args = null)
    {
        if (args is null)
            await _js.InvokeVoidAsync("tauriInvoke", cmd);
        else
            await _js.InvokeVoidAsync("tauriInvoke", cmd, args);
    }

    // Format conversion

    public Task<string> ConvertFormatAsync(string inputData, string fromFormat, string toFormat) =>
        InvokeAsync<string>("convert_format", new { input_data = inputData, from_format = fromFormat, to_format = toFormat });

    // Ingestion (backend-owned pipeline)

    public Task<ImportOutcome> ImportDatasetAsync(string name, string payload, string sourceFormat) =>
        InvokeAsync<ImportOutcome>("import_dataset", new { name, payload, source_format = sourceFormat });

    public Task<Layer> AddResultLayerAsync(SpatialAnalysisResult result) =>
        InvokeAsync<Layer>("add_result_layer", new { result });

    // Symbology

    public Task<List<ClassBreak>> ComputeClassBreaksAsync(string datasetId, string field, string method, int nClasses) =>
        InvokeAsync<List<ClassBreak>>("compute_class_breaks", new { dataset_id = datasetId, field, method, n_classes = nClasses });

    // Bookmarks

    public Task SaveBookmarkAsync(MapBookmark bookmark) =>
        InvokeVoidAsync("save_bookmark", new { bookmark });

    public Task<List<MapBookmark>> ListBookmarksAsync() =>
        InvokeAsync<List<MapBookmark>>("list_bookmarks");

    public Task<bool> DeleteBookmarkAsync(string id) =>
        InvokeAsync<bool>("delete_bookmark", new { id });

    // Datasets

    public Task<List<DatasetSummary>> ListDatasetsAsync() =>
        InvokeAsync<List<DatasetSummary>>("list_datasets");

    public Task<DatasetDetail?> GetDatasetAsync(string id) =>
        InvokeAsync<DatasetDetail?>("get_dataset", new { id });

    public Task<bool> DeleteDatasetAsync(string id) =>
        InvokeAsync<bool>("delete_dataset", new { id });

    // Layers

    public Task SaveLayerAsync(Layer layer) =>
        InvokeVoidAsync("save_layer", new { layer });

    public Task<List<Layer>> ListLayersAsync() =>
        InvokeAsync<List<Layer>>("list_layers");

    public Task<bool> DeleteLayerAsync(string id) =>
        InvokeAsync<bool>("delete_layer", new { id });

    // Calculation tabs

    public Task SaveCalculationTabAsync(CalculationTab tab) =>
        InvokeVoidAsync("save_calculation_tab", new { tab });

    public Task<List<CalculationTab>> ListCalculationTabsAsync() =>
        InvokeAsync<List<CalculationTab>>("list_calculation_tabs");

    public Task<bool> DeleteCalculationTabAsync(string id) =>
        InvokeAsync<bool>("delete_calculation_tab", new { id });

    // SQL console & stats

    public Task<SqlQueryResult> ExecuteSqlQueryAsync(string sql) =>
        InvokeAsync<SqlQueryResult>("execute_sql_query", new { sql });

    public Task<DatabaseStats> GetDatabaseStatsAsync() =>
        InvokeAsync<DatabaseStats>("get_database_stats");

    // Geoprocessing tools: one generic dispatch point; argument construction
    // and tool selection live in AppState.RunToolAsync.

    public Task<SpatialAnalysisResult> RunToolCommandAsync(string command, Dictionary<string, object?> args) =>
        InvokeAsync<SpatialAnalysisResult>(command, args);
}
