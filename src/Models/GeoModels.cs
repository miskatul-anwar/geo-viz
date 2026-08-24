using System.Text.Json;
using System.Text.Json.Serialization;

namespace GeoViz.Models;

public class DatasetSummary
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("format")]
    public string Format { get; set; } = "geojson";

    [JsonPropertyName("feature_count")]
    public int FeatureCount { get; set; }

    [JsonPropertyName("geom_types")]
    public List<string> GeomTypes { get; set; } = new();

    [JsonPropertyName("bbox")]
    public double[]? Bbox { get; set; }

    [JsonPropertyName("properties_schema")]
    public List<FieldSchema> PropertiesSchema { get; set; } = new();

    [JsonPropertyName("created_at")]
    public string CreatedAt { get; set; } = string.Empty;

    [JsonPropertyName("updated_at")]
    public string UpdatedAt { get; set; } = string.Empty;
}

public class DatasetDetail : DatasetSummary
{
    [JsonPropertyName("geojson")]
    public string Geojson { get; set; } = string.Empty;
}

/// Result of the backend-owned import pipeline: persisted dataset + provisioned layer.
public class ImportOutcome
{
    [JsonPropertyName("dataset")]
    public DatasetDetail Dataset { get; set; } = new();

    [JsonPropertyName("layer")]
    public Layer Layer { get; set; } = new();
}

public class FieldSchema
{
    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("field_type")]
    public string FieldType { get; set; } = "string";

    [JsonPropertyName("sample_value")]
    public string? SampleValue { get; set; }
}

public class LayerStyle
{
    [JsonPropertyName("fill_color")]
    public string FillColor { get; set; } = "#3b82f6";

    [JsonPropertyName("fill_opacity")]
    public double FillOpacity { get; set; } = 0.35;

    [JsonPropertyName("stroke_color")]
    public string StrokeColor { get; set; } = "#2563eb";

    [JsonPropertyName("stroke_width")]
    public double StrokeWidth { get; set; } = 2.0;

    [JsonPropertyName("stroke_opacity")]
    public double StrokeOpacity { get; set; } = 0.9;

    [JsonPropertyName("point_radius")]
    public double PointRadius { get; set; } = 6.0;

    [JsonPropertyName("dash_array")]
    public string? DashArray { get; set; }

    [JsonPropertyName("shape_type")]
    public string ShapeType { get; set; } = "point";

    /// <summary>Attribute-driven class breaks (categorized/graduated symbology).</summary>
    [JsonPropertyName("classification")]
    public Classification? Classification { get; set; }

    /// <summary>Optional attribute rendered as a map label.</summary>
    [JsonPropertyName("label_field")]
    public string? LabelField { get; set; }

    [JsonPropertyName("blend_mode")]
    public string? BlendMode { get; set; }
}

public class ClassBreak
{
    [JsonPropertyName("min")] public double Min { get; set; }
    [JsonPropertyName("max")] public double Max { get; set; }
    [JsonPropertyName("color")] public string Color { get; set; } = "#7fcd";
    [JsonPropertyName("label")] public string Label { get; set; } = "";
}

public class Classification
{
    [JsonPropertyName("field")]
    public string Field { get; set; } = "";

    [JsonPropertyName("method")]
    public string Method { get; set; } = "equal_interval";

    [JsonPropertyName("breaks")]
    public List<ClassBreak> Breaks { get; set; } = new();
}

public class MapBookmark
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = Guid.NewGuid().ToString();

    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    [JsonPropertyName("center_lat")]
    public double CenterLat { get; set; }

    [JsonPropertyName("center_lng")]
    public double CenterLng { get; set; }

    [JsonPropertyName("zoom")]
    public double Zoom { get; set; } = 4;

    [JsonPropertyName("created_at")]
    public string CreatedAt { get; set; } = DateTime.UtcNow.ToString("o");
}

public class Layer
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = Guid.NewGuid().ToString();

    [JsonPropertyName("dataset_id")]
    public string DatasetId { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("is_visible")]
    public bool IsVisible { get; set; } = true;

    [JsonPropertyName("opacity")]
    public double Opacity { get; set; } = 1.0;

    [JsonPropertyName("style")]
    public LayerStyle Style { get; set; } = new();

    [JsonPropertyName("z_index")]
    public int ZIndex { get; set; } = 0;

    [JsonPropertyName("created_at")]
    public string CreatedAt { get; set; } = DateTime.UtcNow.ToString("o");
}

public class CalculationTab
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = Guid.NewGuid().ToString();

    [JsonPropertyName("title")]
    public string Title { get; set; } = "New Calculation";

    [JsonPropertyName("active_tool")]
    public string ActiveTool { get; set; } = "buffer";

    [JsonPropertyName("created_at")]
    public string CreatedAt { get; set; } = DateTime.UtcNow.ToString("o");

    [JsonPropertyName("updated_at")]
    public string UpdatedAt { get; set; } = DateTime.UtcNow.ToString("o");
}

public class SpatialAnalysisResult
{
    [JsonPropertyName("tool_name")]
    public string ToolName { get; set; } = string.Empty;

    [JsonPropertyName("output_geojson")]
    public string OutputGeojson { get; set; } = string.Empty;

    [JsonPropertyName("feature_count")]
    public int FeatureCount { get; set; }

    [JsonPropertyName("execution_time_ms")]
    public long ExecutionTimeMs { get; set; }

    [JsonPropertyName("summary_metrics")]
    public JsonElement SummaryMetrics { get; set; }

    [JsonPropertyName("layer_name")]
    public string LayerName { get; set; } = string.Empty;
}

public class CalculationRun
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = string.Empty;

    [JsonPropertyName("tab_id")]
    public string TabId { get; set; } = string.Empty;

    [JsonPropertyName("tool_name")]
    public string ToolName { get; set; } = string.Empty;

    [JsonPropertyName("parameters_json")]
    public string ParametersJson { get; set; } = string.Empty;

    [JsonPropertyName("result_summary_json")]
    public string ResultSummaryJson { get; set; } = string.Empty;

    [JsonPropertyName("execution_time_ms")]
    public long ExecutionTimeMs { get; set; }

    [JsonPropertyName("created_at")]
    public string CreatedAt { get; set; } = string.Empty;
}

public class DatabaseStats
{
    [JsonPropertyName("dataset_count")]
    public int DatasetCount { get; set; }

    [JsonPropertyName("layer_count")]
    public int LayerCount { get; set; }

    [JsonPropertyName("calculation_count")]
    public int CalculationCount { get; set; }

    [JsonPropertyName("tab_count")]
    public int TabCount { get; set; }

    [JsonPropertyName("db_size_bytes")]
    public ulong DbSizeBytes { get; set; }
}

public class SqlQueryResult
{
    [JsonPropertyName("columns")]
    public List<string> Columns { get; set; } = new();

    [JsonPropertyName("rows")]
    public List<List<JsonElement>> Rows { get; set; } = new();

    [JsonPropertyName("row_count")]
    public int RowCount { get; set; }

    [JsonPropertyName("execution_time_ms")]
    public long ExecutionTimeMs { get; set; }
}

public class RasterSummary
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("width")]
    public int Width { get; set; }

    [JsonPropertyName("height")]
    public int Height { get; set; }

    [JsonPropertyName("cell_size_m")]
    public double CellSizeM { get; set; }

    [JsonPropertyName("bbox")]
    public List<double> Bbox { get; set; } = new();

    [JsonPropertyName("created_at")]
    public string CreatedAt { get; set; } = string.Empty;
}
