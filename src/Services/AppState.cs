using GeoViz.Models;

namespace GeoViz.Services;

/// <summary>
/// Central client-side state container. Owns all data and orchestration for
/// the app; components render from this class and delegate every mutation to
/// it. A single <see cref="Changed"/> event keeps the UI in sync.
/// </summary>
public sealed class AppState
{
    private readonly TauriService _tauri;
    private readonly Dictionary<string, DatasetDetail> _datasetCache = new();
    private readonly List<Layer> _layers = new();
    private readonly List<DatasetSummary> _datasets = new();
    private readonly List<CalculationTab> _tabs = new();
    private readonly List<MapBookmark> _bookmarks = new();
    private readonly List<CalculationRun> _recentRuns = new();
    private readonly List<RasterSummary> _rasters = new();

    public AppState(TauriService tauri)
    {
        _tauri = tauri;
    }

    /// <summary>Raised after any state mutation; components re-read properties.</summary>
    public event Action? Changed;

    public IReadOnlyList<Layer> Layers => _layers;
    public IReadOnlyList<DatasetSummary> Datasets => _datasets;
    public IReadOnlyList<CalculationTab> Tabs => _tabs;
    public IReadOnlyList<MapBookmark> Bookmarks => _bookmarks;
    public IReadOnlyList<CalculationRun> RecentRuns => _recentRuns;
    public IReadOnlyList<RasterSummary> Rasters => _rasters;

    public string? ActiveLayerId { get; private set; }
    public string? ActiveTabId { get; private set; }

    /// <summary>Index of the feature last clicked on the map (table sync).</summary>
    public int? ActiveFeatureIndex { get; private set; }

    public void SetActiveFeature(int? index)
    {
        ActiveFeatureIndex = index;
        Notify();
    }
    public DatabaseStats? Stats { get; private set; }
    public SpatialAnalysisResult? LastResult { get; private set; }

    /// <summary>Last operation error, surfaced once by the UI then dismissed.</summary>
    public string? Error { get; private set; }

    public Layer? ActiveLayer => _layers.FirstOrDefault(l => l.Id == ActiveLayerId);

    /// <summary>Cached detail (incl. GeoJSON) of the active layer's dataset.</summary>
    public DatasetDetail? ActiveDataset =>
        ActiveLayer is { } layer ? GetCachedDataset(layer.DatasetId) : null;

    public bool IsInitialized { get; private set; }

    // ------------------------------------------------------------------
    // Lifecycle
    // ------------------------------------------------------------------

    public async Task InitializeAsync()
    {
        if (IsInitialized) return;
        IsInitialized = true;
        try
        {
            await ReloadCoreAsync();
        }
        catch (Exception ex)
        {
            SetError($"Startup failed: {ex.Message}");
        }
        Notify();
    }

    private async Task ReloadCoreAsync()
    {
        _layers.Clear();
        _layers.AddRange(await _tauri.ListLayersAsync());

        var datasets = await _tauri.ListDatasetsAsync();
        _datasets.Clear();
        _datasets.AddRange(datasets);
        foreach (var ds in datasets)
            _datasetCache.Remove(ds.Id); // evict stale summaries; details reload on demand

        _tabs.Clear();
        _tabs.AddRange(await _tauri.ListCalculationTabsAsync());
        if (_tabs.Count == 0)
        {
            var tab = new CalculationTab { Title = "Buffer Analysis 1", ActiveTool = "buffer" };
            await _tauri.SaveCalculationTabAsync(tab);
            _tabs.Add(tab);
        }
        if (ActiveTabId is null || _tabs.All(t => t.Id != ActiveTabId))
            ActiveTabId = _tabs[0].Id;

        try
        {
            _bookmarks.Clear();
            _bookmarks.AddRange(await _tauri.ListBookmarksAsync());
        }
        catch
        {
            /* bookmarks are non-critical at startup */
        }

        try
        {
            _recentRuns.Clear();
            _recentRuns.AddRange(await _tauri.ListCalculationHistoryAsync());
        }
        catch
        {
            /* history is non-critical at startup */
        }

        try
        {
            _rasters.Clear();
            _rasters.AddRange(await _tauri.ListRastersAsync());
        }
        catch
        {
            /* rasters are non-critical at startup */
        }

        if (ActiveLayerId is not null && _layers.All(l => l.Id != ActiveLayerId))
            ActiveLayerId = null;
        if (ActiveLayerId is null && _layers.Count > 0)
            await SelectLayerAsync(_layers[0].Id);

        Stats = await _tauri.GetDatabaseStatsAsync();
    }

    public async Task RefreshStatsAsync()
    {
        try
        {
            Stats = await _tauri.GetDatabaseStatsAsync();
        }
        catch (Exception ex)
        {
            SetError(ex.Message);
        }
        Notify();
    }

    // ------------------------------------------------------------------
    // Dataset cache
    // ------------------------------------------------------------------

    public DatasetDetail? GetCachedDataset(string datasetId) =>
        _datasetCache.TryGetValue(datasetId, out var detail) ? detail : null;

    /// <summary>Returns cached dataset detail, fetching it once if needed.</summary>
    public async Task<DatasetDetail?> ResolveDatasetAsync(string datasetId)
    {
        if (_datasetCache.TryGetValue(datasetId, out var cached))
            return cached;
        try
        {
            var detail = await _tauri.GetDatasetAsync(datasetId);
            if (detail is not null)
                _datasetCache[datasetId] = detail;
            return detail;
        }
        catch (Exception ex)
        {
            SetError(ex.Message);
            return null;
        }
    }

    // ------------------------------------------------------------------
    // Layers
    // ------------------------------------------------------------------

    public async Task SelectLayerAsync(string layerId)
    {
        ActiveLayerId = layerId;
        if (ActiveLayer is { } layer)
            await ResolveDatasetAsync(layer.DatasetId);
        Notify();
    }

    public async Task ImportAsync(string name, string payload, string sourceFormat)
    {
        var outcome = await _tauri.ImportDatasetAsync(name, payload, sourceFormat);
        _datasetCache[outcome.Dataset.Id] = outcome.Dataset;
        _datasets.Insert(0, new DatasetSummary
        {
            Id = outcome.Dataset.Id,
            Name = outcome.Dataset.Name,
            Format = outcome.Dataset.Format,
            FeatureCount = outcome.Dataset.FeatureCount,
            GeomTypes = outcome.Dataset.GeomTypes,
            Bbox = outcome.Dataset.Bbox,
            PropertiesSchema = outcome.Dataset.PropertiesSchema,
            CreatedAt = outcome.Dataset.CreatedAt,
            UpdatedAt = outcome.Dataset.UpdatedAt
        });
        _layers.Add(outcome.Layer);
        ActiveLayerId = outcome.Layer.Id;
        await RefreshStatsInternalAsync();
        ClearError();
        Notify();
    }

    public async Task DeleteLayerAsync(string layerId)
    {
        await _tauri.DeleteLayerAsync(layerId);
        var layer = _layers.FirstOrDefault(l => l.Id == layerId);
        _layers.RemoveAll(l => l.Id == layerId);

        // Drop datasets that no longer have any layer referencing them.
        if (layer is not null && _layers.All(l => l.DatasetId != layer.DatasetId))
        {
            try
            {
                await _tauri.DeleteDatasetAsync(layer.DatasetId);
                _datasetCache.Remove(layer.DatasetId);
                var summary = _datasets.FirstOrDefault(d => d.Id == layer.DatasetId);
                if (summary is not null) _datasets.Remove(summary);
            }
            catch (Exception ex)
            {
                SetError($"Layer removed but dataset cleanup failed: {ex.Message}");
            }
        }

        if (ActiveLayerId == layerId)
        {
            ActiveLayerId = _layers.FirstOrDefault()?.Id;
            if (ActiveLayerId is not null && ActiveLayer is { } next)
                await ResolveDatasetAsync(next.DatasetId);
        }
        await RefreshStatsInternalAsync();
        Notify();
    }

    public async Task ToggleVisibilityAsync(Layer layer)
    {
        layer.IsVisible = !layer.IsVisible;
        await PersistLayerAsync(layer);
    }

    public async Task UpdateStyleAsync(Layer layer) => await PersistLayerAsync(layer);

    private async Task PersistLayerAsync(Layer layer)
    {
        try
        {
            await _tauri.SaveLayerAsync(layer);
            ClearError();
        }
        catch (Exception ex)
        {
            SetError(ex.Message);
        }
        Notify();
    }

    // ------------------------------------------------------------------
    // Calculation tabs & tools
    // ------------------------------------------------------------------

    public async Task CreateTabAsync()
    {
        var tab = new CalculationTab { Title = $"Analysis Workspace {_tabs.Count + 1}", ActiveTool = "buffer" };
        await _tauri.SaveCalculationTabAsync(tab);
        _tabs.Add(tab);
        ActiveTabId = tab.Id;
        Notify();
    }

    public void SwitchTab(string tabId)
    {
        ActiveTabId = tabId;
        Notify();
    }

    public async Task CloseTabAsync(string tabId)
    {
        await _tauri.DeleteCalculationTabAsync(tabId);
        _tabs.RemoveAll(t => t.Id == tabId);
        if (ActiveTabId == tabId)
            ActiveTabId = _tabs.FirstOrDefault()?.Id;
        Notify();
    }

    public CalculationTab? ActiveTab => _tabs.FirstOrDefault(t => t.Id == ActiveTabId);

    /// <summary>Persists mutable tab state (e.g. the selected tool).</summary>
    public async Task UpdateTabAsync(CalculationTab tab)
    {
        tab.UpdatedAt = DateTime.UtcNow.ToString("o");
        await _tauri.SaveCalculationTabAsync(tab);
        Notify();
    }

    /// <summary>
    /// Executes a geoprocessing tool on the backend. <paramref name="parameters"/>
    /// holds tool-specific snake_case arguments; common inputs are attached here.
    /// </summary>
    public async Task RunToolAsync(string toolKey, string datasetId, Dictionary<string, object?> parameters)
    {
        var args = new Dictionary<string, object?>(parameters)
        {
            ["dataset_id"] = datasetId,
            ["tab_id"] = ActiveTabId ?? string.Empty
        };

        SpatialAnalysisResult result;
        switch (toolKey)
        {
            case "buffer":
                result = await _tauri.RunToolCommandAsync("run_buffer_tool", args);
                break;
            case "convex_hull":
                result = await _tauri.RunToolCommandAsync("run_convex_hull_tool", args);
                break;
            case "centroid":
                result = await _tauri.RunToolCommandAsync("run_centroid_tool", args);
                break;
            case "bbox":
                result = await _tauri.RunToolCommandAsync("run_bounding_box_tool", args);
                break;
            case "simplify":
                result = await _tauri.RunToolCommandAsync("run_simplify_tool", args);
                break;
            case "metrics":
                result = await _tauri.RunToolCommandAsync("run_metrics_tool", args);
                break;
            case "spatial_binning":
                result = await _tauri.RunToolCommandAsync("run_spatial_binning_tool", args);
                break;
            case "distance_matrix":
                args["source_dataset_id"] = args.TryGetValue("dataset_id", out var src) ? src : datasetId;
                args.Remove("dataset_id");
                result = await _tauri.RunToolCommandAsync("run_distance_matrix_tool", args);
                break;
            case "spatial_query":
                args["source_dataset_id"] = args.TryGetValue("dataset_id", out var src2) ? src2 : datasetId;
                args.Remove("dataset_id");
                result = await _tauri.RunToolCommandAsync("run_spatial_query_tool", args);
                break;
            case "random_points":
                result = await _tauri.RunToolCommandAsync("run_random_points_tool", args);
                break;
            case "overlay":
                args["overlay_dataset_id"] = args.TryGetValue("filter_dataset_id", out var overlayId) ? overlayId : null;
                result = await _tauri.RunToolCommandAsync("run_overlay_tool", args);
                break;
            case "dissolve":
                result = await _tauri.RunToolCommandAsync("run_dissolve_tool", args);
                break;
            case "spatial_join":
                args["join_dataset_id"] = args.TryGetValue("filter_dataset_id", out var joinId) ? joinId : null;
                result = await _tauri.RunToolCommandAsync("run_spatial_join_tool", args);
                break;
            // --- Spatial statistics ---
            case "mean_center":
                result = await _tauri.RunToolCommandAsync("run_mean_center_tool", args);
                break;
            case "median_center":
                result = await _tauri.RunToolCommandAsync("run_median_center_tool", args);
                break;
            case "directional_mean":
                result = await _tauri.RunToolCommandAsync("run_directional_mean_tool", args);
                break;
            case "morans_i":
                result = await _tauri.RunToolCommandAsync("run_morans_i_tool", args);
                break;
            case "getis_ord":
                result = await _tauri.RunToolCommandAsync("run_getis_ord_tool", args);
                break;
            case "ols_regression":
                result = await _tauri.RunToolCommandAsync("run_ols_tool", args);
                break;
            // --- Geostatistics ---
            case "idw":
                result = await _tauri.RunToolCommandAsync("run_idw_tool", args);
                break;
            case "kriging":
                result = await _tauri.RunToolCommandAsync("run_kriging_tool", args);
                break;
            // --- Network ---
            case "shortest_path":
                result = await _tauri.RunToolCommandAsync("run_shortest_path_tool", args);
                break;
            case "service_area":
                result = await _tauri.RunToolCommandAsync("run_service_area_tool", args);
                break;
            case "od_matrix":
                result = await _tauri.RunToolCommandAsync("run_od_matrix_tool", args);
                break;
            // --- Topology & joins ---
            case "topology_check":
                result = await _tauri.RunToolCommandAsync("run_topology_tool", args);
                break;
            case "join_csv":
                result = await _tauri.RunToolCommandAsync("run_join_csv_tool", args);
                break;
            // --- Raster (Spatial Analyst) ---
            case "slope":
                result = await _tauri.RunToolCommandAsync("run_slope_tool", args);
                break;
            case "hillshade":
                result = await _tauri.RunToolCommandAsync("run_hillshade_tool", args);
                break;
            case "raster_calculator":
                result = await _tauri.RunToolCommandAsync("run_raster_calculator_tool", args);
                break;
            case "d8_flow":
                result = await _tauri.RunToolCommandAsync("run_d8_tool", args);
                break;
            case "zonal_stats":
                result = await _tauri.RunToolCommandAsync("run_zonal_stats_tool", args);
                break;
            case "viewshed":
                result = await _tauri.RunToolCommandAsync("run_viewshed_tool", args);
                break;
            default:
                throw new InvalidOperationException($"Unknown tool '{toolKey}'.");
        }

        LastResult = result;
        _recentRuns.Insert(0, new CalculationRun
        {
            Id = Guid.NewGuid().ToString(),
            TabId = ActiveTabId ?? string.Empty,
            ToolName = toolKey,
            ParametersJson = System.Text.Json.JsonSerializer.Serialize(parameters),
            ResultSummaryJson = result.SummaryMetrics.ValueKind == System.Text.Json.JsonValueKind.Object
                ? result.SummaryMetrics.GetRawText()
                : "{}",
            ExecutionTimeMs = result.ExecutionTimeMs,
            CreatedAt = DateTime.UtcNow.ToString("o")
        });
        if (_recentRuns.Count > 20) _recentRuns.RemoveAt(_recentRuns.Count - 1);
        await RefreshStatsInternalAsync();
        ClearError();
        Notify();
    }

    /// <summary>Persists the last analysis output as a new map layer.</summary>
    public async Task<Layer?> AddResultAsLayerAsync()
    {
        if (LastResult is not { } result) return null;
        var layer = await _tauri.AddResultLayerAsync(result);
        _layers.Add(layer);
        ActiveLayerId = layer.Id;
        await RefreshStatsInternalAsync();
        Notify();
        return layer;
    }

    // ------------------------------------------------------------------
    // Symbology & bookmarks
    // ------------------------------------------------------------------

    /// <summary>Computes class breaks on the backend and applies them to the layer.</summary>
    public async Task ClassifyLayerAsync(Layer layer, string field, string method, int nClasses)
    {
        var breaks = await _tauri.ComputeClassBreaksAsync(layer.DatasetId, field, method, nClasses);
        layer.Style.Classification = new Classification
        {
            Field = field,
            Method = method,
            Breaks = breaks
        };
        // The user's base fill color is preserved; classified features are
        // colored per-class by the renderer, unmatched features keep the base.
        await PersistLayerAsync(layer);
    }

    public async Task ClearClassificationAsync(Layer layer)
    {
        layer.Style.Classification = null;
        await PersistLayerAsync(layer);
    }

    public async Task SetLabelFieldAsync(Layer layer, string? field)
    {
        layer.Style.LabelField = string.IsNullOrWhiteSpace(field) ? null : field;
        await PersistLayerAsync(layer);
    }

    public async Task AddBookmarkAsync(string name, double lat, double lng, double zoom)
    {
        var bookmark = new MapBookmark { Name = name, CenterLat = lat, CenterLng = lng, Zoom = zoom };
        await _tauri.SaveBookmarkAsync(bookmark);
        _bookmarks.Insert(0, bookmark);
        Notify();
    }

    public async Task DeleteBookmarkAsync(string id)
    {
        await _tauri.DeleteBookmarkAsync(id);
        _bookmarks.RemoveAll(b => b.Id == id);
        Notify();
    }

    // ------------------------------------------------------------------
    // Rasters (Spatial Analyst)
    // ------------------------------------------------------------------

    public async Task<RasterSummary> ImportRasterAsync(string? name, string tiffBase64)
    {
        var summary = await _tauri.ImportRasterAsync(name, tiffBase64);
        _rasters.Insert(0, summary);
        await RefreshStatsInternalAsync();
        ClearError();
        Notify();
        return summary;
    }

    public async Task DeleteRasterAsync(string id)
    {
        await _tauri.DeleteRasterAsync(id);
        _rasters.RemoveAll(r => r.Id == id);
        Notify();
    }

    // ------------------------------------------------------------------
    // Error handling
    // ------------------------------------------------------------------

    public void DismissError()
    {
        ClearError();
        Notify();
    }

    private void SetError(string message) => Error = message;
    private void ClearError() => Error = null;

    private async Task RefreshStatsInternalAsync()
    {
        try
        {
            Stats = await _tauri.GetDatabaseStatsAsync();
        }
        catch
        {
            /* stats are non-critical */
        }
    }

    public void Notify() => Changed?.Invoke();
}
