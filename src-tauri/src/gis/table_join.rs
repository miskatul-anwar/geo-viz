//! Attribute table joins: attach columns from a standalone CSV to a layer
//! by matching a primary key (ArcGIS "Add Join" parity).

use geojson::{Feature, FeatureCollection};
use serde_json::{json, Value as JsonValue};

/// Parse CSV text (header row + records; quoted fields supported).
pub fn parse_csv(text: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let mut rows = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => in_quotes = true,
            ',' => {
                record.push(field.clone());
                field.clear();
            }
            '\r' => {}
            '\n' => {
                record.push(field.clone());
                field.clear();
                if !(record.len() == 1 && record[0].trim().is_empty()) {
                    rows.push(record.clone());
                }
                record.clear();
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        if !(record.len() == 1 && record[0].trim().is_empty()) {
            rows.push(record);
        }
    }

    let mut iter = rows.into_iter();
    let header: Vec<String> = iter
        .next()
        .ok_or("CSV is empty (missing header row)")?
        .into_iter()
        .map(|h| h.trim().to_string())
        .collect();
    let records: Vec<Vec<String>> = iter.collect();
    for (i, r) in records.iter().enumerate() {
        if r.len() != header.len() {
            return Err(format!(
                "CSV row {} has {} fields; header has {}",
                i + 2,
                r.len(),
                header.len()
            ));
        }
    }
    Ok((header, records))
}

/// Join CSV columns onto `fc` features where `fc[key_field] == csv[csv_key]`.
/// Joined columns are prefixed `join_` to avoid collisions.
pub fn join_csv(
    fc: &FeatureCollection,
    key_field: &str,
    csv_text: &str,
    csv_key: &str,
) -> Result<(FeatureCollection, JsonValue), String> {
    let (header, records) = parse_csv(csv_text)?;
    let csv_idx = header
        .iter()
        .position(|h| h.eq_ignore_ascii_case(csv_key))
        .ok_or_else(|| format!("CSV key column '{csv_key}' not found in header {header:?}"))?;
    if !header.iter().any(|h| h.eq_ignore_ascii_case(key_field)) {
        return Err(format!(
            "layer key field '{key_field}' not found in CSV header {header:?}"
        ));
    }

    // Build lookup: key value -> row (first occurrence wins).
    let mut lookup: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, row) in records.iter().enumerate() {
        lookup.entry(row[csv_idx].trim().to_string()).or_insert(i);
    }

    let join_columns: Vec<&String> = header
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != csv_idx)
        .map(|(_, h)| h)
        .collect();

    let mut matched = 0usize;
    let mut out_features = Vec::with_capacity(fc.features.len());
    for feature in &fc.features {
        let mut props = feature.properties.clone().unwrap_or_default();
        let key_value = props
            .get(key_field)
            .map(|v| match v {
                JsonValue::String(s) => s.clone(),
                JsonValue::Number(n) => n.to_string(),
                other => other.to_string(),
            })
            .or(None);
        if let Some(key) = key_value {
            if let Some(&row_idx) = lookup.get(key.trim()) {
                matched += 1;
                for col_name in join_columns.iter() {
                    let header_pos = header.iter().position(|h| h == *col_name).unwrap();
                    let raw = &records[row_idx][header_pos];
                    let value: JsonValue = raw
                        .parse::<f64>()
                        .map(JsonValue::from)
                        .unwrap_or_else(|_| json!(raw));
                    props.insert(format!("join_{col_name}"), value);
                }
            }
        }
        out_features.push(Feature {
            bbox: feature.bbox.clone(),
            geometry: feature.geometry.clone(),
            id: feature.id.clone(),
            properties: Some(props),
            foreign_members: feature.foreign_members.clone(),
        });
    }

    let unmatched = fc.features.len() - matched;
    Ok((
        FeatureCollection {
            bbox: fc.bbox.clone(),
            features: out_features,
            foreign_members: None,
        },
        json!({
            "key_field": key_field,
            "csv_key": csv_key,
            "joined_columns": join_columns.iter().map(|c| format!("join_{c}")).collect::<Vec<_>>(),
            "csv_rows": records.len(),
            "matched_features": matched,
            "unmatched_features": unmatched
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Map};

    fn fc_with_keys() -> FeatureCollection {
        FeatureCollection {
            bbox: None,
            features: (0..3)
                .map(|i| {
                    let mut props = Map::new();
                    props.insert("fid".into(), json!(format!("id-{i}")));
                    Feature {
                        bbox: None,
                        geometry: Some(geojson::Geometry::new(geojson::Value::Point(vec![
                            0.0, i as f64,
                        ]))),
                        id: None,
                        properties: Some(props),
                        foreign_members: None,
                    }
                })
                .collect(),
            foreign_members: None,
        }
    }

    #[test]
    fn test_join_matches_and_prefixes() {
        let csv = "fid,population,label\nid-0,100,alpha\nid-1,200,beta\nid-9,999,ghost\n";
        let (out, summary) = join_csv(&fc_with_keys(), "fid", csv, "fid").unwrap();
        assert_eq!(summary["matched_features"], 2);
        assert_eq!(summary["unmatched_features"], 1);
        let p0 = out.features[0].properties.as_ref().unwrap();
        assert_eq!(p0["join_population"].as_f64().unwrap(), 100.0);
        assert_eq!(p0["join_label"], "alpha");
        assert!(out.features[2]
            .properties
            .as_ref()
            .unwrap()
            .get("join_population")
            .is_none());
    }

    #[test]
    fn test_csv_quoted_fields() {
        let (header, records) = parse_csv("a,b\n\"x,1\",2\n\"say \"\"hi\"\"\",3\n").unwrap();
        assert_eq!(header, vec!["a", "b"]);
        assert_eq!(records[0][0], "x,1");
        assert_eq!(records[1][0], "say \"hi\"");
    }

    #[test]
    fn test_missing_key_errors() {
        let csv = "other,val\nx,1\n";
        assert!(join_csv(&fc_with_keys(), "fid", csv, "fid").is_err());
    }
}
