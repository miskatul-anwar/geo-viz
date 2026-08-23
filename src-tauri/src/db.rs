use crate::error::{AppError, AppResult};
use crate::models::*;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Column, Pool, Row, Sqlite, TypeInfo,
};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

/// SQLite-backed application store. Cheap to clone; safe to share.
#[derive(Clone)]
pub struct AppDb {
    pool: Arc<Pool<Sqlite>>,
    db_path: PathBuf,
}

impl AppDb {
    pub async fn init(app_data_dir: Option<PathBuf>) -> Result<Self, String> {
        let db_dir = app_data_dir.unwrap_or_else(|| PathBuf::from("."));
        tokio::fs::create_dir_all(&db_dir)
            .await
            .map_err(|e| format!("Failed to create db dir: {e}"))?;

        let db_path = db_dir.join("geoviz.db");
        let conn_str = format!("sqlite://{}", db_path.to_string_lossy());

        // Connection-level pragmas must be applied through connect options so
        // that every pooled connection honors them (WAL itself is persistent,
        // but foreign_keys / synchronous / busy_timeout are per-connection).
        let options = SqliteConnectOptions::from_str(&conn_str)
            .map_err(|e| format!("Invalid SQLite connection string: {e}"))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| format!("Failed to connect to SQLite DB: {e}"))?;

        let app_db = Self {
            pool: Arc::new(pool),
            db_path,
        };

        app_db
            .run_migrations()
            .await
            .map_err(|e| format!("Migration failed: {e}"))?;
        Ok(app_db)
    }

    async fn run_migrations(&self) -> AppResult<()> {
        let queries = [
            r#"
            CREATE TABLE IF NOT EXISTS datasets (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                format TEXT NOT NULL,
                feature_count INTEGER NOT NULL,
                geom_types TEXT NOT NULL,
                bbox TEXT,
                properties_schema TEXT NOT NULL,
                geojson TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
            r#"
            CREATE TABLE IF NOT EXISTS layers (
                id TEXT PRIMARY KEY,
                dataset_id TEXT NOT NULL,
                name TEXT NOT NULL,
                is_visible INTEGER NOT NULL DEFAULT 1,
                opacity REAL NOT NULL DEFAULT 1.0,
                style TEXT NOT NULL,
                z_index INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                FOREIGN KEY (dataset_id) REFERENCES datasets(id) ON DELETE CASCADE
            );
            "#,
            r#"
            CREATE TABLE IF NOT EXISTS calculation_tabs (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                active_tool TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
            r#"
            CREATE TABLE IF NOT EXISTS calculations (
                id TEXT PRIMARY KEY,
                tab_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                parameters_json TEXT NOT NULL,
                result_summary_json TEXT NOT NULL,
                execution_time_ms INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            "#,
            r#"
            CREATE TABLE IF NOT EXISTS bookmarks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                center_lat REAL NOT NULL,
                center_lng REAL NOT NULL,
                zoom REAL NOT NULL,
                created_at TEXT NOT NULL
            );
            "#,
        ];

        for q in queries {
            sqlx::query(q).execute(&*self.pool).await?;
        }
        Ok(())
    }

    // Datasets

    pub async fn save_dataset(&self, dataset: &DatasetDetail) -> AppResult<()> {
        let geom_types_json = serde_json::to_string(&dataset.geom_types)?;
        let bbox_json = dataset
            .bbox
            .as_ref()
            .map(|b| serde_json::to_string(b).unwrap_or_default());
        let schema_json = serde_json::to_string(&dataset.properties_schema)?;

        sqlx::query(
            r#"
            INSERT INTO datasets (id, name, format, feature_count, geom_types, bbox, properties_schema, geojson, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                format = excluded.format,
                feature_count = excluded.feature_count,
                geom_types = excluded.geom_types,
                bbox = excluded.bbox,
                properties_schema = excluded.properties_schema,
                geojson = excluded.geojson,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&dataset.id)
        .bind(&dataset.name)
        .bind(&dataset.format)
        .bind(dataset.feature_count as i64)
        .bind(&geom_types_json)
        .bind(&bbox_json)
        .bind(&schema_json)
        .bind(&dataset.geojson)
        .bind(&dataset.created_at)
        .bind(&dataset.updated_at)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_datasets(&self) -> AppResult<Vec<DatasetSummary>> {
        let rows = sqlx::query(
            "SELECT id, name, format, feature_count, geom_types, bbox, properties_schema, created_at, updated_at FROM datasets ORDER BY created_at DESC",
        )
        .fetch_all(&*self.pool)
        .await?;

        rows.iter()
            .map(|r| {
                Ok(DatasetSummary {
                    id: r.get("id"),
                    name: r.get("name"),
                    format: r.get("format"),
                    feature_count: r.get::<i64, _>("feature_count") as usize,
                    geom_types: parse_json_or_default(&r.get::<String, _>("geom_types")),
                    bbox: r
                        .get::<Option<String>, _>("bbox")
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    properties_schema: parse_json_or_default(
                        &r.get::<String, _>("properties_schema"),
                    ),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                })
            })
            .collect()
    }

    pub async fn get_dataset_detail(&self, id: &str) -> AppResult<Option<DatasetDetail>> {
        let row = sqlx::query(
            "SELECT id, name, format, feature_count, geom_types, bbox, properties_schema, geojson, created_at, updated_at FROM datasets WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await?;

        row.map(|r| {
            Ok(DatasetDetail {
                id: r.get("id"),
                name: r.get("name"),
                format: r.get("format"),
                feature_count: r.get::<i64, _>("feature_count") as usize,
                geom_types: parse_json_or_default(&r.get::<String, _>("geom_types")),
                bbox: r
                    .get::<Option<String>, _>("bbox")
                    .and_then(|s| serde_json::from_str(&s).ok()),
                properties_schema: parse_json_or_default(&r.get::<String, _>("properties_schema")),
                geojson: r.get("geojson"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
        })
        .transpose()
    }

    /// Delete a dataset; layers cascade via foreign keys.
    pub async fn delete_dataset(&self, id: &str) -> AppResult<bool> {
        let res = sqlx::query("DELETE FROM datasets WHERE id = ?")
            .bind(id)
            .execute(&*self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    // Layers

    pub async fn save_layer(&self, layer: &Layer) -> AppResult<()> {
        let style_json = serde_json::to_string(&layer.style)?;

        sqlx::query(
            r#"
            INSERT INTO layers (id, dataset_id, name, is_visible, opacity, style, z_index, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                is_visible = excluded.is_visible,
                opacity = excluded.opacity,
                style = excluded.style,
                z_index = excluded.z_index
            "#,
        )
        .bind(&layer.id)
        .bind(&layer.dataset_id)
        .bind(&layer.name)
        .bind(if layer.is_visible { 1 } else { 0 })
        .bind(layer.opacity)
        .bind(&style_json)
        .bind(layer.z_index)
        .bind(&layer.created_at)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_layers(&self) -> AppResult<Vec<Layer>> {
        let rows =
            sqlx::query("SELECT id, dataset_id, name, is_visible, opacity, style, z_index, created_at FROM layers ORDER BY z_index ASC")
                .fetch_all(&*self.pool)
                .await?;

        rows.iter()
            .map(|r| {
                Ok(Layer {
                    id: r.get("id"),
                    dataset_id: r.get("dataset_id"),
                    name: r.get("name"),
                    is_visible: r.get::<i64, _>("is_visible") == 1,
                    opacity: r.get("opacity"),
                    style: serde_json::from_str(&r.get::<String, _>("style")).unwrap_or_default(),
                    z_index: r.get("z_index"),
                    created_at: r.get("created_at"),
                })
            })
            .collect()
    }

    pub async fn delete_layer(&self, id: &str) -> AppResult<bool> {
        let res = sqlx::query("DELETE FROM layers WHERE id = ?")
            .bind(id)
            .execute(&*self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    // Calculation tabs

    pub async fn save_tab(&self, tab: &CalculationTab) -> AppResult<()> {
        sqlx::query(
            r#"
            INSERT INTO calculation_tabs (id, title, active_tool, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                active_tool = excluded.active_tool,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&tab.id)
        .bind(&tab.title)
        .bind(&tab.active_tool)
        .bind(&tab.created_at)
        .bind(&tab.updated_at)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_tabs(&self) -> AppResult<Vec<CalculationTab>> {
        let rows =
            sqlx::query("SELECT id, title, active_tool, created_at, updated_at FROM calculation_tabs ORDER BY created_at ASC")
                .fetch_all(&*self.pool)
                .await?;

        Ok(rows
            .into_iter()
            .map(|r| CalculationTab {
                id: r.get("id"),
                title: r.get("title"),
                active_tool: r.get("active_tool"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    pub async fn delete_tab(&self, id: &str) -> AppResult<bool> {
        let res = sqlx::query("DELETE FROM calculation_tabs WHERE id = ?")
            .bind(id)
            .execute(&*self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    // Calculation history (write-only audit trail)

    pub async fn log_calculation(&self, history: &CalculationHistory) -> AppResult<()> {
        sqlx::query(
            r#"
            INSERT INTO calculations (id, tab_id, tool_name, parameters_json, result_summary_json, execution_time_ms, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&history.id)
        .bind(&history.tab_id)
        .bind(&history.tool_name)
        .bind(&history.parameters_json)
        .bind(&history.result_summary_json)
        .bind(history.execution_time_ms)
        .bind(&history.created_at)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    // Spatial bookmarks

    pub async fn save_bookmark(&self, bookmark: &MapBookmark) -> AppResult<()> {
        sqlx::query(
            r#"
            INSERT INTO bookmarks (id, name, center_lat, center_lng, zoom, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                center_lat = excluded.center_lat,
                center_lng = excluded.center_lng,
                zoom = excluded.zoom
            "#,
        )
        .bind(&bookmark.id)
        .bind(&bookmark.name)
        .bind(bookmark.center_lat)
        .bind(bookmark.center_lng)
        .bind(bookmark.zoom)
        .bind(&bookmark.created_at)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_bookmarks(&self) -> AppResult<Vec<MapBookmark>> {
        let rows = sqlx::query("SELECT id, name, center_lat, center_lng, zoom, created_at FROM bookmarks ORDER BY created_at DESC")
            .fetch_all(&*self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| MapBookmark {
                id: r.get("id"),
                name: r.get("name"),
                center_lat: r.get("center_lat"),
                center_lng: r.get("center_lng"),
                zoom: r.get("zoom"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn delete_bookmark(&self, id: &str) -> AppResult<bool> {
        let res = sqlx::query("DELETE FROM bookmarks WHERE id = ?")
            .bind(id)
            .execute(&*self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    // SQL console & stats

    pub async fn execute_sql_query(&self, sql: &str) -> AppResult<SqlQueryResult> {
        let start = std::time::Instant::now();
        let trimmed = sql.trim();
        let upper = trimmed.to_uppercase();
        if !(upper.starts_with("SELECT")
            || upper.starts_with("EXPLAIN")
            || upper.starts_with("PRAGMA"))
        {
            return Err(AppError::Parse(
                "only read-only statements (SELECT, EXPLAIN, PRAGMA) are allowed in the SQL console".into(),
            ));
        }

        let rows = sqlx::query(trimmed).fetch_all(&*self.pool).await?;
        let elapsed = start.elapsed().as_millis() as i64;

        if rows.is_empty() {
            return Ok(SqlQueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                row_count: 0,
                execution_time_ms: elapsed,
            });
        }

        let columns: Vec<String> = rows[0]
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();

        let result_rows: Vec<Vec<serde_json::Value>> = rows
            .iter()
            .map(|r| (0..columns.len()).map(|i| cell_to_json(r, i)).collect())
            .collect();

        Ok(SqlQueryResult {
            row_count: result_rows.len(),
            rows: result_rows,
            columns,
            execution_time_ms: elapsed,
        })
    }

    pub async fn get_stats(&self) -> AppResult<DatabaseStats> {
        let dataset_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM datasets")
            .fetch_one(&*self.pool)
            .await?;
        let layer_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM layers")
            .fetch_one(&*self.pool)
            .await?;
        let calculation_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calculations")
            .fetch_one(&*self.pool)
            .await?;
        let tab_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calculation_tabs")
            .fetch_one(&*self.pool)
            .await?;

        let db_size_bytes = tokio::fs::metadata(&self.db_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(DatabaseStats {
            dataset_count: dataset_count as usize,
            layer_count: layer_count as usize,
            calculation_count: calculation_count as usize,
            tab_count: tab_count as usize,
            db_size_bytes,
        })
    }
}

fn parse_json_or_default<T: serde::de::DeserializeOwned + Default>(json: &str) -> T {
    serde_json::from_str(json).unwrap_or_default()
}

fn cell_to_json(row: &sqlx::sqlite::SqliteRow, i: usize) -> serde_json::Value {
    match row.column(i).type_info().name() {
        "INTEGER" => row
            .try_get::<i64, _>(i)
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
        "REAL" => row
            .try_get::<f64, _>(i)
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
        "BOOLEAN" => row
            .try_get::<bool, _>(i)
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
        _ => row
            .try_get::<String, _>(i)
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
    }
}
