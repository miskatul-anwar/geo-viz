use geojson::{feature::Id, Feature, FeatureCollection, Geometry, Value as GeoValue};
use serde_json::{Map, Value as JsonValue};

// 1. GeoJSON to CSV
pub fn geojson_to_csv(fc: &FeatureCollection) -> Result<String, String> {
    if fc.features.is_empty() {
        return Ok("feature_id,geometry_type,centroid_lng,centroid_lat".to_string());
    }

    let mut header_keys = Vec::new();
    for f in &fc.features {
        if let Some(ref props) = f.properties {
            for k in props.keys() {
                if !header_keys.contains(k) {
                    header_keys.push(k.clone());
                }
            }
        }
    }

    let mut lines = Vec::new();
    let mut header_line = vec![
        "feature_id".to_string(),
        "geometry_type".to_string(),
        "centroid_lng".to_string(),
        "centroid_lat".to_string(),
    ];
    header_line.extend(header_keys.clone());
    lines.push(header_line.join(","));

    for (idx, f) in fc.features.iter().enumerate() {
        let feat_id =
            f.id.as_ref()
                .map(|id| match id {
                    Id::String(s) => s.clone(),
                    Id::Number(n) => n.to_string(),
                })
                .unwrap_or_else(|| (idx + 1).to_string());

        let geom_type = f
            .geometry
            .as_ref()
            .map(|g| match &g.value {
                GeoValue::Point(_) => "Point",
                GeoValue::MultiPoint(_) => "MultiPoint",
                GeoValue::LineString(_) => "LineString",
                GeoValue::MultiLineString(_) => "MultiLineString",
                GeoValue::Polygon(_) => "Polygon",
                GeoValue::MultiPolygon(_) => "MultiPolygon",
                GeoValue::GeometryCollection(_) => "GeometryCollection",
            })
            .unwrap_or("None");

        let mut coords = Vec::new();
        if let Some(ref geom) = f.geometry {
            super::convex_hull::extract_coords_from_geom(geom, &mut coords);
        }
        let (c_lng, c_lat) = if let Some(first) = coords.first() {
            (first[0].to_string(), first[1].to_string())
        } else {
            ("".to_string(), "".to_string())
        };

        let mut row = vec![escape_csv(&feat_id), geom_type.to_string(), c_lng, c_lat];

        let props = f.properties.as_ref();
        for k in &header_keys {
            let val_str = props
                .and_then(|p| p.get(k))
                .map(|v| match v {
                    JsonValue::String(s) => s.clone(),
                    JsonValue::Number(n) => n.to_string(),
                    JsonValue::Bool(b) => b.to_string(),
                    _ => v.to_string(),
                })
                .unwrap_or_default();

            row.push(escape_csv(&val_str));
        }

        lines.push(row.join(","));
    }

    Ok(lines.join("\n"))
}

// 2. GeoJSON to WKT
pub fn geojson_to_wkt(fc: &FeatureCollection) -> Result<String, String> {
    let mut wkt_lines = Vec::new();
    for f in &fc.features {
        if let Some(ref geom) = f.geometry {
            let wkt_str = geom_to_wkt_str(&geom.value);
            if !wkt_str.is_empty() {
                wkt_lines.push(wkt_str);
            }
        }
    }
    if wkt_lines.is_empty() {
        return Err("No valid vector geometries found in GeoJSON".to_string());
    }
    Ok(wkt_lines.join("\n"))
}

pub fn geom_to_wkt_str(val: &GeoValue) -> String {
    match val {
        GeoValue::Point(c) => format!("POINT({} {})", c[0], c[1]),
        GeoValue::MultiPoint(pts) => {
            let pts_str = pts
                .iter()
                .map(|c| format!("{} {}", c[0], c[1]))
                .collect::<Vec<_>>()
                .join(", ");
            format!("MULTIPOINT({})", pts_str)
        }
        GeoValue::LineString(pts) => {
            let pts_str = pts
                .iter()
                .map(|c| format!("{} {}", c[0], c[1]))
                .collect::<Vec<_>>()
                .join(", ");
            format!("LINESTRING({})", pts_str)
        }
        GeoValue::MultiLineString(lines) => {
            let lines_str = lines
                .iter()
                .map(|l| {
                    let pts_str = l
                        .iter()
                        .map(|c| format!("{} {}", c[0], c[1]))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("({})", pts_str)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("MULTILINESTRING({})", lines_str)
        }
        GeoValue::Polygon(rings) => {
            let rings_str = rings
                .iter()
                .map(|r| {
                    let coords_str = r
                        .iter()
                        .map(|c| format!("{} {}", c[0], c[1]))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("({})", coords_str)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("POLYGON({})", rings_str)
        }
        GeoValue::MultiPolygon(polys) => {
            let polys_str = polys
                .iter()
                .map(|p| {
                    let rings_str = p
                        .iter()
                        .map(|r| {
                            let coords_str = r
                                .iter()
                                .map(|c| format!("{} {}", c[0], c[1]))
                                .collect::<Vec<_>>()
                                .join(", ");
                            format!("({})", coords_str)
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("({})", rings_str)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("MULTIPOLYGON({})", polys_str)
        }
        _ => String::new(),
    }
}

// 3. CSV to GeoJSON
pub fn csv_to_geojson(csv_text: &str) -> Result<FeatureCollection, String> {
    let mut lines = csv_text.lines();
    let header_line = lines.next().ok_or("CSV is empty")?;
    let headers: Vec<&str> = header_line
        .split(',')
        .map(|s| s.trim().trim_matches('"'))
        .collect();

    let mut lat_idx = None;
    let mut lng_idx = None;

    for (i, h) in headers.iter().enumerate() {
        let h_lower = h.to_lowercase();
        if h_lower == "lat" || h_lower == "latitude" || h_lower == "y" || h_lower == "centroid_lat"
        {
            lat_idx = Some(i);
        } else if h_lower == "lng"
            || h_lower == "lon"
            || h_lower == "longitude"
            || h_lower == "x"
            || h_lower == "centroid_lng"
        {
            lng_idx = Some(i);
        }
    }

    let lat_idx = lat_idx.ok_or("Could not find Latitude/Lat/Y column in CSV")?;
    let lng_idx = lng_idx.ok_or("Could not find Longitude/Lng/Lon/X column in CSV")?;

    let mut features = Vec::new();
    for (line_num, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line
            .split(',')
            .map(|s| s.trim().trim_matches('"'))
            .collect();
        if cols.len() <= lat_idx || cols.len() <= lng_idx {
            continue;
        }

        let lat: f64 = match cols[lat_idx].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let lng: f64 = match cols[lng_idx].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let mut props = Map::new();
        for (i, h) in headers.iter().enumerate() {
            if i < cols.len() {
                let val_str = cols[i];
                if let Ok(num) = val_str.parse::<f64>() {
                    props.insert(h.to_string(), serde_json::json!(num));
                } else {
                    props.insert(h.to_string(), serde_json::json!(val_str));
                }
            }
        }

        features.push(Feature {
            bbox: None,
            geometry: Some(Geometry::new(GeoValue::Point(vec![lng, lat]))),
            id: Some(Id::Number(serde_json::Number::from(line_num + 1))),
            properties: Some(props),
            foreign_members: None,
        });
    }

    if features.is_empty() {
        return Err("No valid coordinate rows found in CSV".to_string());
    }

    Ok(FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    })
}

// 4. CSV to WKT
pub fn csv_to_wkt(csv_text: &str) -> Result<String, String> {
    let fc = csv_to_geojson(csv_text)?;
    geojson_to_wkt(&fc)
}

// 5. WKT to GeoJSON
pub fn wkt_to_geojson(wkt_text: &str) -> Result<FeatureCollection, String> {
    let mut features = Vec::new();
    let mut feat_idx = 1;

    for line in wkt_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let geom_val = parse_single_wkt_geom(trimmed)?;
        let mut props = Map::new();
        props.insert("feature_id".to_string(), serde_json::json!(feat_idx));
        props.insert("wkt_source".to_string(), serde_json::json!(trimmed));

        features.push(Feature {
            bbox: None,
            geometry: Some(Geometry::new(geom_val)),
            id: Some(Id::Number(serde_json::Number::from(feat_idx))),
            properties: Some(props),
            foreign_members: None,
        });

        feat_idx += 1;
    }

    if features.is_empty() {
        return Err("No valid WKT geometries could be parsed".to_string());
    }

    Ok(FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    })
}

// 6. WKT to CSV
pub fn wkt_to_csv(wkt_text: &str) -> Result<String, String> {
    let fc = wkt_to_geojson(wkt_text)?;
    geojson_to_csv(&fc)
}

// Helper WKT Parsing Functions
fn parse_single_wkt_geom(wkt: &str) -> Result<GeoValue, String> {
    let s = wkt.trim();
    let upper = s.to_uppercase();

    if upper.starts_with("POINT") {
        let inner = extract_parentheses(s)?;
        let pair = parse_coord_pair(&inner)?;
        Ok(GeoValue::Point(pair))
    } else if upper.starts_with("MULTIPOINT") {
        let inner = extract_parentheses(s)?;
        let cleaned = inner.replace(['(', ')'], "");
        let pts = parse_coord_list(&cleaned)?;
        Ok(GeoValue::MultiPoint(pts))
    } else if upper.starts_with("LINESTRING") {
        let inner = extract_parentheses(s)?;
        let pts = parse_coord_list(&inner)?;
        Ok(GeoValue::LineString(pts))
    } else if upper.starts_with("MULTILINESTRING") {
        let inner = extract_parentheses(s)?;
        let rings = extract_nested_groups(&inner)?;
        let mut lines = Vec::new();
        for r in rings {
            lines.push(parse_coord_list(&r)?);
        }
        Ok(GeoValue::MultiLineString(lines))
    } else if upper.starts_with("POLYGON") {
        let inner = extract_parentheses(s)?;
        let rings = extract_nested_groups(&inner)?;
        let mut poly = Vec::new();
        if rings.is_empty() {
            poly.push(parse_coord_list(&inner)?);
        } else {
            for r in rings {
                poly.push(parse_coord_list(&r)?);
            }
        }
        Ok(GeoValue::Polygon(poly))
    } else if upper.starts_with("MULTIPOLYGON") {
        let inner = extract_parentheses(s)?;
        let poly_groups = extract_nested_groups(&inner)?;
        let mut multipoly = Vec::new();
        for p in poly_groups {
            let rings = extract_nested_groups(&p)?;
            let mut poly = Vec::new();
            if rings.is_empty() {
                poly.push(parse_coord_list(&p)?);
            } else {
                for r in rings {
                    poly.push(parse_coord_list(&r)?);
                }
            }
            multipoly.push(poly);
        }
        Ok(GeoValue::MultiPolygon(multipoly))
    } else {
        Err(format!("Unrecognized WKT geometry format: {}", s))
    }
}

fn extract_parentheses(s: &str) -> Result<String, String> {
    let start = s
        .find('(')
        .ok_or("Missing opening parenthesis '(' in WKT")?;
    let end = s
        .rfind(')')
        .ok_or("Missing closing parenthesis ')' in WKT")?;
    if start >= end {
        return Err("Malformed parentheses in WKT".to_string());
    }
    Ok(s[start + 1..end].trim().to_string())
}

fn extract_nested_groups(s: &str) -> Result<Vec<String>, String> {
    let mut groups = Vec::new();
    let mut depth = 0;
    let mut start = 0;

    // char_indices yields byte offsets, keeping slicing UTF-8 safe.
    for (i, c) in s.char_indices() {
        if c == '(' {
            if depth == 0 {
                start = i + 1;
            }
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                groups.push(s[start..i].trim().to_string());
            }
        }
    }

    Ok(groups)
}

fn parse_coord_pair(pair_str: &str) -> Result<Vec<f64>, String> {
    let parts: Vec<&str> = pair_str.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(format!("Invalid coordinate pair: '{}'", pair_str));
    }
    let x: f64 = parts[0]
        .parse()
        .map_err(|e| format!("Failed to parse X/Lng '{}': {}", parts[0], e))?;
    let y: f64 = parts[1]
        .parse()
        .map_err(|e| format!("Failed to parse Y/Lat '{}': {}", parts[1], e))?;
    Ok(vec![x, y])
}

fn parse_coord_list(list_str: &str) -> Result<Vec<Vec<f64>>, String> {
    let mut coords = Vec::new();
    for pair in list_str.split(',') {
        let trimmed = pair.trim();
        if !trimmed.is_empty() {
            coords.push(parse_coord_pair(trimmed)?);
        }
    }
    if coords.is_empty() {
        return Err("Coordinate list is empty".to_string());
    }
    Ok(coords)
}

fn escape_csv(val: &str) -> String {
    if val.contains(',') || val.contains('"') || val.contains('\n') {
        format!("\"{}\"", val.replace('"', "\"\""))
    } else {
        val.to_string()
    }
}
