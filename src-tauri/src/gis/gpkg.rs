//! GeoPackage (`.gpkg`) ingestion.
//!
//! GeoPackage is SQLite-based; we open the archive read-only through sqlx and
//! decode the standard GPKG binary geometry blobs (header + plain WKB).

use super::parser::ParsedGeoData;
use crate::gis::parser::parse_geojson_str;
use geojson::{Feature, Geometry, Value as GeoValue};
use serde_json::{Map, Value as JsonValue};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row, TypeInfo};
use std::str::FromStr;

const MAX_FEATURES: usize = 250_000;

pub async fn parse_gpkg_bytes(bytes: &[u8]) -> Result<ParsedGeoData, String> {
    // sqlx requires a file path; stage the payload in a temp directory.
    let temp_dir = std::env::temp_dir().join(format!("geoviz_gpkg_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    let db_path = temp_dir.join("data.gpkg");
    std::fs::write(&db_path, bytes).map_err(|e| e.to_string())?;

    let result = read_gpkg_feature_table(&db_path).await;

    // Best-effort cleanup regardless of outcome.
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

async fn read_gpkg_feature_table(db_path: &std::path::Path) -> Result<ParsedGeoData, String> {
    let options =
        SqliteConnectOptions::from_str(&format!("sqlite://{}?mode=ro", db_path.to_string_lossy()))
            .map_err(|e| format!("invalid gpkg connection string: {e}"))?
            .read_only(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| format!("cannot open GeoPackage (not valid SQLite?): {e}"))?;

    let tables: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT c.table_name AS t, g.column_name AS gcol
        FROM gpkg_contents c
        JOIN gpkg_geometry_columns g ON g.table_name = c.table_name
        WHERE c.data_type = 'features'
        ORDER BY c.table_name
        "#,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("not a valid GeoPackage (missing metadata tables): {e}"))?;

    let (table, geom_col) = tables
        .first()
        .cloned()
        .ok_or("GeoPackage contains no feature tables")?;

    let rows = sqlx::query(&format!(
        r#"SELECT * FROM "{}" LIMIT {}"#,
        table.replace('"', "\"\""),
        MAX_FEATURES
    ))
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("failed to read feature table '{table}': {e}"))?;

    let mut features = Vec::with_capacity(rows.len());
    for row in &rows {
        if let Some(f) = row_to_feature(row, &geom_col) {
            features.push(f);
        }
    }
    pool.close().await;

    if features.is_empty() {
        return Err("GeoPackage feature table contains no readable geometries".into());
    }
    finish(features)
}

fn row_to_feature(row: &SqliteRow, geom_col: &str) -> Option<Feature> {
    let mut properties = Map::new();
    let mut geometry: Option<Geometry> = None;

    for (i, col) in row.columns().iter().enumerate() {
        let name = col.name();
        if name == geom_col {
            let blob: Option<Vec<u8>> = row.try_get(i).ok();
            if let Some(blob) = blob {
                geometry = decode_gpkg_blob(&blob)
                    .map(Geometry::new)
                    .map_err(|e| e.to_string())
                    .ok();
            }
            continue;
        }
        match col.type_info().name() {
            "INTEGER" => {
                if let Ok(v) = row.try_get::<i64, _>(i) {
                    properties.insert(name.to_string(), JsonValue::from(v));
                }
            }
            "REAL" => {
                if let Ok(v) = row.try_get::<f64, _>(i) {
                    properties.insert(name.to_string(), JsonValue::from(v));
                }
            }
            "TEXT" => {
                if let Ok(v) = row.try_get::<String, _>(i) {
                    properties.insert(name.to_string(), JsonValue::String(v));
                }
            }
            _ => {}
        }
    }

    Some(Feature {
        bbox: None,
        geometry,
        id: None,
        properties: Some(properties),
        foreign_members: None,
    })
}

fn finish(features: Vec<Feature>) -> Result<ParsedGeoData, String> {
    let fc = geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    let json = serde_json::to_string(&fc).map_err(|e| e.to_string())?;
    parse_geojson_str(&json)
}

// ---------------------------------------------------------------------------
// GPKG binary geometry decoding
// ---------------------------------------------------------------------------

/// Decode a GeoPackage geometry blob: `"GP"` header + envelope + standard WKB.
pub fn decode_gpkg_blob(blob: &[u8]) -> Result<GeoValue, String> {
    if blob.len() < 8 || &blob[0..2] != b"GP" {
        return Err("geometry blob lacks GPKG magic header".into());
    }
    let version = blob[2];
    if version != 0 {
        return Err(format!("unsupported GPKG geometry version {version}"));
    }
    let flags = blob[3];
    if flags & 0x20 != 0 {
        return Err("extended GPKG geometry headers are not supported".into());
    }

    // Header byte-order only affects the srs_id/envelope fields, which are
    // skipped; WKB carries its own endianness marker.
    let envelope_code = (flags >> 1) & 0x07;
    let envelope_doubles = match envelope_code {
        0 => 0usize,
        1 => 4,
        2 | 3 => 6,
        4 => 8,
        other => return Err(format!("reserved envelope code {other}")),
    };
    let empty = flags & 0x10 != 0;

    let wkb_offset = 8 + envelope_doubles * 8;
    if blob.len() < wkb_offset {
        return Err("truncated GPKG geometry header".into());
    }
    if empty {
        return Err("empty geometry".into());
    }

    decode_wkb(&blob[wkb_offset..])
}

/// Decode one standard WKB geometry (ISO encoding; tolerates EWKB size flags).
pub fn decode_wkb(data: &[u8]) -> Result<GeoValue, String> {
    let mut pos = 0usize;
    let value = decode_wkb_inner(data, &mut pos)?;
    Ok(value)
}

fn decode_wkb_inner(data: &[u8], pos: &mut usize) -> Result<GeoValue, String> {
    if *pos + 5 > data.len() {
        return Err("truncated WKB header".into());
    }
    let little_endian = match data[*pos] {
        0 => false,
        1 => true,
        b => return Err(format!("invalid WKB byte-order byte {b}")),
    };
    *pos += 1;
    let rd_u32 = |data: &[u8], pos: &mut usize| -> Result<u64, String> {
        if *pos + 4 > data.len() {
            return Err("truncated WKB integer".into());
        }
        let bytes: [u8; 4] = data[*pos..*pos + 4].try_into().unwrap();
        *pos += 4;
        Ok(if little_endian {
            u32::from_le_bytes(bytes) as u64
        } else {
            u32::from_be_bytes(bytes) as u64
        })
    };

    let raw_type = rd_u32(data, pos)?;

    // Normalize ISO (+1000/+2000/+3000) and EWKB (high-bit) dimension flags.
    let mut has_z = false;
    let mut has_m = false;
    let mut base_type = raw_type & 0xFF;
    if (3000..4000).contains(&raw_type) {
        has_z = true;
        has_m = true;
        base_type = raw_type - 3000;
    } else if (2000..3000).contains(&raw_type) {
        has_m = true;
        base_type = raw_type - 2000;
    } else if (1000..2000).contains(&raw_type) {
        has_z = true;
        base_type = raw_type - 1000;
    } else {
        if raw_type & 0x8000_0000 != 0 {
            has_z = true;
        }
        if raw_type & 0x4000_0000 != 0 {
            has_m = true;
        }
        base_type &= 0xFF;
    }

    let dims = 2 + usize::from(has_z) + usize::from(has_m);

    fn coord(data: &[u8], pos: &mut usize, dims: usize, le: bool) -> Result<Vec<f64>, String> {
        if *pos + dims * 8 > data.len() {
            return Err("truncated WKB coordinate".into());
        }
        let mut out = Vec::with_capacity(2);
        for d in 0..dims {
            let bytes: [u8; 8] = data[*pos..*pos + 8].try_into().unwrap();
            *pos += 8;
            let v = if le {
                f64::from_le_bytes(bytes)
            } else {
                f64::from_be_bytes(bytes)
            };
            if d < 2 {
                out.push(v);
            }
        }
        Ok(out)
    }

    match base_type {
        1 => Ok(GeoValue::Point(coord(data, pos, dims, little_endian)?)),
        2 => {
            let n = rd_u32(data, pos)? as usize;
            let mut pts = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                pts.push(coord(data, pos, dims, little_endian)?);
            }
            Ok(GeoValue::LineString(pts))
        }
        3 => {
            let ring_count = rd_u32(data, pos)? as usize;
            let mut rings = Vec::with_capacity(ring_count.min(1024));
            for _ in 0..ring_count {
                let n = rd_u32(data, pos)? as usize;
                let mut ring = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    ring.push(coord(data, pos, dims, little_endian)?);
                }
                rings.push(ring);
            }
            Ok(GeoValue::Polygon(rings))
        }
        4..=7 => {
            let n = rd_u32(data, pos)? as usize;
            let mut parts = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                parts.push(decode_wkb_inner(data, pos)?);
            }
            match base_type {
                4 => Ok(GeoValue::MultiPoint(
                    parts
                        .into_iter()
                        .filter_map(|g| match g {
                            GeoValue::Point(c) => Some(c),
                            _ => None,
                        })
                        .collect(),
                )),
                5 => Ok(GeoValue::MultiLineString(
                    parts.into_iter().filter_map(to_lines).collect(),
                )),
                6 => Ok(GeoValue::MultiPolygon(
                    parts.into_iter().filter_map(to_polys).collect(),
                )),
                _ => Ok(GeoValue::GeometryCollection(
                    parts.into_iter().map(geojson::Geometry::new).collect(),
                )),
            }
        }
        other => Err(format!("unsupported WKB geometry type {other}")),
    }
}

fn to_lines(g: GeoValue) -> Option<Vec<Vec<f64>>> {
    match g {
        GeoValue::LineString(l) => Some(l),
        _ => None,
    }
}

fn to_polys(g: GeoValue) -> Option<Vec<Vec<Vec<f64>>>> {
    match g {
        GeoValue::Polygon(p) => Some(p),
        _ => None,
    }
}
