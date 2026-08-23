use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetSummary {
    pub id: String,
    pub name: String,
    pub format: String,
    pub feature_count: usize,
    pub geom_types: Vec<String>,
    pub bbox: Option<[f64; 4]>, // [min_lng, min_lat, max_lng, max_lat]
    pub properties_schema: Vec<FieldSchema>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetDetail {
    pub id: String,
    pub name: String,
    pub format: String,
    pub feature_count: usize,
    pub geom_types: Vec<String>,
    pub bbox: Option<[f64; 4]>,
    pub properties_schema: Vec<FieldSchema>,
    pub geojson: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub field_type: String, // "string", "number", "boolean", "null"
    pub sample_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerStyle {
    pub fill_color: String,
    pub fill_opacity: f64,
    pub stroke_color: String,
    pub stroke_width: f64,
    pub stroke_opacity: f64,
    pub point_radius: f64,
    pub dash_array: Option<String>,
    #[serde(default = "default_shape_type")]
    pub shape_type: String, // "point" | "line" | "polygon"
    /// Attribute-driven class breaks (categorized/graduated symbology).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    /// Optional attribute used to render feature labels on the map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_field: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    pub field: String,
    pub method: String,
    pub breaks: Vec<crate::gis::classification::ClassBreak>,
}

fn default_shape_type() -> String {
    "point".to_string()
}

impl Default for LayerStyle {
    fn default() -> Self {
        Self {
            fill_color: "#3b82f6".to_string(),
            fill_opacity: 0.35,
            stroke_color: "#2563eb".to_string(),
            stroke_width: 2.0,
            stroke_opacity: 0.9,
            point_radius: 6.0,
            dash_array: None,
            shape_type: "point".to_string(),
            classification: None,
            label_field: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapBookmark {
    pub id: String,
    pub name: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub zoom: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: String,
    pub dataset_id: String,
    pub name: String,
    pub is_visible: bool,
    pub opacity: f64,
    pub style: LayerStyle,
    pub z_index: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationTab {
    pub id: String,
    pub title: String,
    pub active_tool: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationHistory {
    pub id: String,
    pub tab_id: String,
    pub tool_name: String,
    pub parameters_json: String,
    pub result_summary_json: String,
    pub execution_time_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialAnalysisResult {
    pub tool_name: String,
    pub output_geojson: String,
    pub feature_count: usize,
    pub execution_time_ms: i64,
    pub summary_metrics: serde_json::Value,
    pub layer_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseStats {
    pub dataset_count: usize,
    pub layer_count: usize,
    pub calculation_count: usize,
    pub tab_count: usize,
    pub db_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub execution_time_ms: i64,
}
