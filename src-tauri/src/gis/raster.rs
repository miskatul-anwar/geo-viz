//! Spatial Analyst: offline raster engine (GeoTIFF ingestion, surface
//! analysis, map algebra, hydrology, zonal statistics, viewshed).
//!
//! Rasters are stored as f64 grids in SQLite (`rasters` table); analysis
//! outputs are decimated point grids in GeoJSON plus numeric summaries.

use serde_json::{json, Value as JsonValue};

/// A dense row-major raster grid in geographic cells.
#[derive(Debug, Clone)]
pub struct RasterGrid {
    pub width: usize,
    pub height: usize,
    /// Row-major values, north-to-south.
    pub data: Vec<f64>,
    pub nodata: Option<f64>,
    /// Geographic extent: (min_lng, min_lat, max_lng, max_lat).
    pub bbox: (f64, f64, f64, f64),
}

impl RasterGrid {
    pub fn cell_size(&self) -> (f64, f64) {
        (
            (self.bbox.2 - self.bbox.0) / self.width.max(1) as f64,
            (self.bbox.3 - self.bbox.1) / self.height.max(1) as f64,
        )
    }

    fn get(&self, x: usize, y: usize) -> Option<f64> {
        let v = *self.data.get(y * self.width + x)?;
        match self.nodata {
            Some(nd) if (v - nd).abs() < f64::EPSILON => None,
            _ => Some(v),
        }
    }
}

// ---------------------------------------------------------------------------
// Minimal GeoTIFF / baseline TIFF reader
// ---------------------------------------------------------------------------

struct TiffReader<'a> {
    buf: &'a [u8],
    little: bool,
}

impl<'a> TiffReader<'a> {
    fn u16(&self, off: usize) -> Result<u16, String> {
        let b = self.buf.get(off..off + 2).ok_or("TIFF truncated (u16)")?;
        Ok(if self.little {
            u16::from_le_bytes(b.try_into().unwrap())
        } else {
            u16::from_be_bytes(b.try_into().unwrap())
        })
    }

    fn u32(&self, off: usize) -> Result<u32, String> {
        let b = self.buf.get(off..off + 4).ok_or("TIFF truncated (u32)")?;
        Ok(if self.little {
            u32::from_le_bytes(b.try_into().unwrap())
        } else {
            u32::from_be_bytes(b.try_into().unwrap())
        })
    }

    /// Resolve an IFD value: inline when <= 4 bytes, else via offset.
    fn value_bytes(&self, entry_off: usize) -> Result<Vec<u8>, String> {
        let count = self.u32(entry_off + 4)? as usize;
        let typ = self.u16(entry_off + 2)? as usize;
        let type_size = match typ {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            other => return Err(format!("unsupported TIFF value type {other}")),
        };
        let total = count * type_size;
        if total <= 4 {
            Ok(self
                .buf
                .get(entry_off + 8..entry_off + 8 + total)
                .ok_or("TIFF truncated (inline value)")?
                .to_vec())
        } else {
            let off = self.u32(entry_off + 8)? as usize;
            self.buf
                .get(off..off + total)
                .ok_or_else(|| "TIFF truncated (value offset)".to_string())
                .map(<[u8]>::to_vec)
        }
    }
}

fn u16_from(v: &[u8], little: bool) -> u16 {
    if little {
        u16::from_le_bytes([v[0], v[1]])
    } else {
        u16::from_be_bytes([v[0], v[1]])
    }
}

fn u32_from(v: &[u8], little: bool) -> u32 {
    if little {
        u32::from_le_bytes([v[0], v[1], v[2], v[3]])
    } else {
        u32::from_be_bytes([v[0], v[1], v[2], v[3]])
    }
}

fn read_u32_array(r: &TiffReader, entry: usize) -> Result<Vec<u32>, String> {
    let raw = r.value_bytes(entry)?;
    Ok(raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32_from(c, r.little))
        .collect())
}

/// Parse a baseline/GeoTIFF file (uncompressed, single-band, strip layout).
pub fn parse_geotiff(bytes: &[u8]) -> Result<RasterGrid, String> {
    if bytes.len() < 8 {
        return Err("file too small to be a TIFF".into());
    }
    let little = match &bytes[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err("not a TIFF (bad byte-order mark)".into()),
    };
    let r = TiffReader { buf: bytes, little };
    if r.u16(2)? != 42 {
        return Err("not a TIFF (bad magic)".into());
    }

    let ifd_off = r.u32(4)? as usize;
    let entry_count = r.u16(ifd_off)? as usize;
    let (mut width, mut height) = (0usize, 0usize);
    let (mut bits, mut spp, mut compression, mut sample_format) = (16u16, 1u16, 1u16, 1u16);
    let (mut strip_offsets, mut strip_counts) = (Vec::<u32>::new(), Vec::<u32>::new());

    for e in 0..entry_count {
        let entry = ifd_off + 2 + e * 12;
        let v = r.value_bytes(entry).unwrap_or_default();
        match r.u16(entry)? {
            256 => width = u16_from(&v, little) as usize,
            257 => height = u16_from(&v, little) as usize,
            258 => bits = u16_from(&v, little),
            259 => compression = u16_from(&v, little),
            273 => strip_offsets = read_u32_array(&r, entry)?,
            277 => spp = u16_from(&v, little),
            279 => strip_counts = read_u32_array(&r, entry)?,
            339 => sample_format = u16_from(&v, little),
            _ => {}
        }
    }

    if width == 0 || height == 0 {
        return Err("TIFF missing image dimensions (or LONG-typed tags unsupported)".into());
    }
    if compression != 1 {
        return Err(format!(
            "unsupported TIFF compression {compression}; only uncompressed (1) is supported offline"
        ));
    }
    if spp != 1 {
        return Err(format!(
            "only single-band rasters are supported; found {spp} samples per pixel"
        ));
    }
    if strip_offsets.is_empty() {
        return Err("TIFF has no strip data (tiled layouts are unsupported)".into());
    }
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err(format!("unsupported bit depth {bits}"));
    }

    let bps = bits as usize / 8;
    let mut data = Vec::with_capacity(width * height);
    for (s, &off) in strip_offsets.iter().enumerate() {
        let len = strip_counts.get(s).copied().unwrap_or(0) as usize;
        let slice = bytes
            .get(off as usize..off as usize + len)
            .ok_or("TIFF strip out of bounds")?;
        for chunk in slice.chunks_exact(bps) {
            let v = match (bits, sample_format) {
                (8, 1) => chunk[0] as f64,
                (8, 2) => chunk[0] as i8 as f64,
                (16, 1) => u16_from(chunk, little) as f64,
                (16, 2) => i16::from_le_bytes([chunk[0], chunk[1]]) as f64,
                (16, 3) => i16::from_le_bytes([chunk[0], chunk[1]]) as f64,
                (32, 3) => f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64,
                (32, _) => u32_from(chunk, little) as f64,
                (64, 3) => {
                    let mut a = [0u8; 8];
                    a.copy_from_slice(chunk);
                    f64::from_le_bytes(a)
                }
                _ => {
                    return Err(format!(
                        "unsupported sample format {sample_format} at {bits} bits"
                    ))
                }
            };
            data.push(v);
        }
    }
    if data.len() < width * height {
        return Err(format!(
            "TIFF pixel data incomplete: expected {} values, found {}",
            width * height,
            data.len()
        ));
    }

    // Geographic extent: prefer ModelTiepoint(33922)/ModelPixelScale(33550);
    // fall back to a unit square the UI can re-georeference.
    let mut bbox = (0.0, 0.0, 1.0, 1.0);
    let (mut scale, mut tiepoint): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    for e in 0..entry_count {
        let entry = ifd_off + 2 + e * 12;
        match r.u16(entry)? {
            33550 => scale = read_f64_array(&r, entry)?,
            33922 => tiepoint = read_f64_array(&r, entry)?,
            _ => {}
        }
    }
    if scale.len() >= 2 && tiepoint.len() >= 6 {
        let (ox, oy) = (tiepoint[3], tiepoint[4]);
        bbox = (
            ox,
            oy - scale[1] * height as f64,
            ox + scale[0] * width as f64,
            oy,
        );
    }

    Ok(RasterGrid {
        width,
        height,
        data,
        nodata: None,
        bbox,
    })
}

fn read_f64_array(r: &TiffReader, entry: usize) -> Result<Vec<f64>, String> {
    let raw = r.value_bytes(entry)?;
    Ok(raw
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| {
            let mut a = [0u8; 8];
            a.copy_from_slice(c);
            if r.little {
                f64::from_le_bytes(a)
            } else {
                f64::from_be_bytes(a)
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Surface analysis (Horn's method) & rendering helpers
// ---------------------------------------------------------------------------

const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;

fn cell_meters(grid: &RasterGrid) -> f64 {
    let (w, h) = grid.cell_size();
    let lat_center = (grid.bbox.1 + grid.bbox.3) / 2.0;
    (w * 111_320.0 * lat_center.cos().max(0.01))
        .max(h * 110_540.0)
        .max(1e-6)
}

/// Horn slope in degrees.
pub fn slope_degrees(grid: &RasterGrid) -> RasterGrid {
    let cell = cell_meters(grid);
    let mut out = grid.clone();
    for y in 0..grid.height {
        for x in 0..grid.width {
            let zm1 = grid.get(x.saturating_sub(1), (y + 1).min(grid.height - 1));
            let zp1 = grid.get((x + 1).min(grid.width - 1), y.saturating_sub(1));
            let dzdx = match (
                grid.get(x.saturating_sub(1), y),
                grid.get((x + 1).min(grid.width - 1), y),
            ) {
                (Some(a), Some(b)) => (b - a) / (2.0 * cell),
                _ => 0.0,
            };
            let dzdy = match (
                grid.get(x, y.saturating_sub(1)),
                grid.get(x, (y + 1).min(grid.height - 1)),
            ) {
                (Some(a), Some(b)) => (b - a) / (2.0 * cell),
                _ => 0.0,
            };
            let _ = (zm1, zp1);
            out.data[y * grid.width + x] = (dzdx * dzdx + dzdy * dzdy).sqrt().atan().to_degrees();
        }
    }
    out
}

/// Horn aspect in degrees (0-360, clockwise from north).
pub fn aspect_degrees(grid: &RasterGrid) -> RasterGrid {
    let cell = cell_meters(grid);
    let mut out = grid.clone();
    for y in 0..grid.height {
        for x in 0..grid.width {
            let dzdx = match (
                grid.get(x.saturating_sub(1), y),
                grid.get((x + 1).min(grid.width - 1), y),
            ) {
                (Some(a), Some(b)) => (b - a) / (2.0 * cell),
                _ => 0.0,
            };
            let dzdy = match (
                grid.get(x, y.saturating_sub(1)),
                grid.get(x, (y + 1).min(grid.height - 1)),
            ) {
                (Some(a), Some(b)) => (b - a) / (2.0 * cell),
                _ => 0.0,
            };
            let mut aspect = (dzdx).atan2(dzdy).to_degrees();
            aspect = (180.0 - aspect + 360.0) % 360.0;
            out.data[y * grid.width + x] = aspect;
        }
    }
    out
}

/// Hillshade via Lambertian reflectance with a point light source.
pub fn hillshade(grid: &RasterGrid, azimuth_deg: f64, altitude_deg: f64) -> RasterGrid {
    let slope = slope_degrees(grid);
    let aspect = aspect_degrees(grid);
    let zenith = (90.0 - altitude_deg) * DEG_TO_RAD;
    let az = azimuth_deg * DEG_TO_RAD;
    let mut out = grid.clone();
    for i in 0..out.data.len() {
        let s = slope.data[i].to_radians();
        let a = aspect.data[i].to_radians();
        let shade = zenith.cos() * s.cos() + zenith.sin() * s.sin() * (az - a).cos();
        // Normalize 0..1 (clamp small negatives from rounding).
        out.data[i] = shade.clamp(0.0, 1.0);
    }
    out
}

// ---------------------------------------------------------------------------
// Map algebra (raster calculator)
// ---------------------------------------------------------------------------

/// Evaluate a map-algebra expression over one or two rasters.
/// Variables: `a`/`b` (cell values), numbers, + - * /, parentheses,
/// functions sqrt/log/abs/min/max. Division by zero -> nodata.
pub fn raster_calculator(
    expr: &str,
    a: &RasterGrid,
    b: Option<&RasterGrid>,
) -> Result<RasterGrid, String> {
    let tokens = tokenize(expr)?;
    let mut parser = ExprParser { tokens, pos: 0 };
    let mut out = a.clone();
    for i in 0..out.data.len() {
        parser.pos = 0;
        let va = a.data[i];
        let vb = b.map(|r| r.data.get(i).copied());
        if a.nodata.is_some() && Some(va) == a.nodata {
            continue;
        }
        match parser.parse_expr(va, vb.flatten()) {
            Ok(v) if v.is_finite() => out.data[i] = v,
            Ok(_) => out.data[i] = f64::NAN,
            Err(e) => return Err(e),
        }
    }
    out.nodata = Some(f64::NAN);
    Ok(out)
}

fn tokenize(expr: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => {
                chars.next();
            }
            '(' | ')' | '+' | '-' | '*' | '/' | ',' => {
                tokens.push(c.to_string());
                chars.next();
            }
            '0'..='9' | '.' => {
                let mut num = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2.is_ascii_digit() || c2 == '.' {
                        num.push(c2);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(num);
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut id = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2.is_ascii_alphanumeric() || c2 == '_' {
                        id.push(c2);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(id.to_lowercase());
            }
            other => return Err(format!("unexpected character '{other}' in expression")),
        }
    }
    if tokens.is_empty() {
        return Err("empty expression".into());
    }
    Ok(tokens)
}

struct ExprParser {
    tokens: Vec<String>,
    pos: usize,
}

impl ExprParser {
    fn peek(&self) -> Option<&String> {
        self.tokens.get(self.pos)
    }

    fn parse_expr(&mut self, va: f64, vb: Option<f64>) -> Result<f64, String> {
        let mut left = self.parse_term(va, vb)?;
        while matches!(self.peek().map(String::as_str), Some("+") | Some("-")) {
            let op = self.tokens[self.pos].clone();
            self.pos += 1;
            let right = self.parse_term(va, vb)?;
            left = if op == "+" {
                left + right
            } else {
                left - right
            };
        }
        Ok(left)
    }

    fn parse_term(&mut self, va: f64, vb: Option<f64>) -> Result<f64, String> {
        let mut left = self.parse_factor(va, vb)?;
        while matches!(self.peek().map(String::as_str), Some("*") | Some("/")) {
            let op = self.tokens[self.pos].clone();
            self.pos += 1;
            let right = self.parse_factor(va, vb)?;
            left = if op == "*" {
                left * right
            } else if right.abs() < 1e-15 {
                return Ok(f64::NAN);
            } else {
                left / right
            };
        }
        Ok(left)
    }

    fn parse_factor(&mut self, va: f64, vb: Option<f64>) -> Result<f64, String> {
        let token = self
            .peek()
            .cloned()
            .ok_or_else(|| "unexpected end of expression".to_string())?;
        self.pos += 1;
        match token.as_str() {
            "(" => {
                let v = self.parse_expr(va, vb)?;
                if self.peek().map(String::as_str) != Some(")") {
                    return Err("missing closing parenthesis".into());
                }
                self.pos += 1;
                Ok(v)
            }
            "-" => {
                let v = self.parse_factor(va, vb)?;
                Ok(-v)
            }
            "a" => Ok(va),
            "b" => vb.ok_or("expression uses 'b' but no second raster was provided".to_string()),
            "sqrt" => Ok(self.parse_factor(va, vb)?.sqrt()),
            "abs" => Ok(self.parse_factor(va, vb)?.abs()),
            "log" => {
                let v = self.parse_factor(va, vb)?;
                Ok(if v > 0.0 { v.ln() } else { f64::NAN })
            }
            "min" => {
                let l = self.parse_factor(va, vb)?;
                let r = self.parse_factor(va, vb)?;
                Ok(l.min(r))
            }
            "max" => {
                let l = self.parse_factor(va, vb)?;
                let r = self.parse_factor(va, vb)?;
                Ok(l.max(r))
            }
            num => num
                .parse::<f64>()
                .map_err(|_| format!("unknown token '{num}' in expression")),
        }
    }
}

// ---------------------------------------------------------------------------
// Hydrology: D8 flow direction & accumulation
// ---------------------------------------------------------------------------

const D8_DIRS: [(i32, i32, f64); 8] = [
    (0, -1, 1.0),    // N
    (1, -1, 2.0),    // NE
    (1, 0, 4.0),     // E
    (1, 1, 8.0),     // SE
    (0, 1, 16.0),    // S
    (-1, 1, 32.0),   // SW
    (-1, 0, 64.0),   // W
    (-1, -1, 128.0), // NW
];

/// D8 flow direction grid (ESRI powers-of-two codes; 0 = sink/flat).
pub fn d8_flow_direction(grid: &RasterGrid) -> RasterGrid {
    let mut out = grid.clone();
    for y in 0..grid.height {
        for x in 0..grid.width {
            let Some(z) = grid.get(x, y) else {
                out.data[y * grid.width + x] = 0.0;
                continue;
            };
            let mut best_drop = 0.0;
            let mut best_code = 0.0;
            for (dx, dy, code) in D8_DIRS {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= grid.width as i32 || ny >= grid.height as i32 {
                    continue;
                }
                if let Some(nz) = grid.get(nx as usize, ny as usize) {
                    let dist = if dx != 0 && dy != 0 {
                        std::f64::consts::SQRT_2
                    } else {
                        1.0
                    };
                    let drop = (z - nz) / dist;
                    if drop > best_drop {
                        best_drop = drop;
                        best_code = code;
                    }
                }
            }
            out.data[y * grid.width + x] = best_code;
        }
    }
    out
}

/// Flow accumulation: number of upstream cells draining through each cell,
/// computed by routing in descending-elevation order.
pub fn flow_accumulation(grid: &RasterGrid) -> Result<RasterGrid, String> {
    let dirs = d8_flow_direction(grid);
    let n = grid.width * grid.height;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| {
        grid.data
            .get(j)
            .copied()
            .unwrap_or(f64::NEG_INFINITY)
            .total_cmp(&grid.data.get(i).copied().unwrap_or(f64::NEG_INFINITY))
    });

    let mut receiver = vec![usize::MAX; n];
    for (i, &code) in dirs.data.iter().enumerate() {
        if code == 0.0 {
            continue;
        }
        let (x, y) = (i % grid.width, i / grid.width);
        for (dx, dy, expected) in D8_DIRS {
            if expected == code {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < grid.width && (ny as usize) < grid.height {
                    receiver[i] = ny as usize * grid.width + nx as usize;
                }
            }
        }
    }

    let mut acc = vec![1.0f64; n];
    for &i in &order {
        let r = receiver[i];
        if r != usize::MAX {
            acc[r] += acc[i];
        }
    }
    let mut out = grid.clone();
    out.data = acc;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Zonal statistics & viewshed
// ---------------------------------------------------------------------------

/// Per-polygon descriptive statistics of the raster cells inside it.
pub fn zonal_statistics(
    grid: &RasterGrid,
    polygons: &geojson::FeatureCollection,
) -> Result<(geojson::FeatureCollection, JsonValue), String> {
    use geo::{Contains, Coord, LineString as GeoLine, Polygon as GeoPolygon};

    let (cell_w, cell_h) = grid.cell_size();
    let mut out_features = Vec::new();
    let mut zone_count = 0usize;

    for (idx, feature) in polygons.features.iter().enumerate() {
        let rings = match feature.geometry.as_ref().map(|g| &g.value) {
            Some(geojson::Value::Polygon(r)) => r,
            _ => continue,
        };
        let outer: GeoLine = GeoLine::from(
            rings[0]
                .iter()
                .map(|c| Coord { x: c[0], y: c[1] })
                .collect::<Vec<_>>(),
        );
        let interiors: Vec<GeoLine> = rings[1..]
            .iter()
            .map(|r| {
                GeoLine::from(
                    r.iter()
                        .map(|c| Coord { x: c[0], y: c[1] })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let poly = GeoPolygon::new(outer, interiors);

        // Iterate only cells within the polygon bbox.
        let min_x = rings[0].iter().map(|c| c[0]).fold(f64::INFINITY, f64::min);
        let max_x = rings[0]
            .iter()
            .map(|c| c[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = rings[0].iter().map(|c| c[1]).fold(f64::INFINITY, f64::min);
        let max_y = rings[0]
            .iter()
            .map(|c| c[1])
            .fold(f64::NEG_INFINITY, f64::max);
        let xs: Vec<usize> = (0..grid.width)
            .filter(|&x| {
                let lng = grid.bbox.0 + cell_w * (x as f64 + 0.5);
                lng >= min_x && lng <= max_x
            })
            .collect();
        let ys: Vec<usize> = (0..grid.height)
            .filter(|&y| {
                let lat = grid.bbox.3 - cell_h * (y as f64 + 0.5);
                lat >= min_y && lat <= max_y
            })
            .collect();

        let mut values = Vec::new();
        for &y in &ys {
            for &x in &xs {
                let lng = grid.bbox.0 + cell_w * (x as f64 + 0.5);
                let lat = grid.bbox.3 - cell_h * (y as f64 + 0.5);
                if poly.contains(&geo::Point::new(lng, lat)) {
                    if let Some(v) = grid.get(x, y) {
                        values.push(v);
                    }
                }
            }
        }
        if values.is_empty() {
            continue;
        }
        values.sort_by(|a, b| a.total_cmp(b));
        let n = values.len();
        let mean = values.iter().sum::<f64>() / n as f64;
        let median = if n % 2 == 1 {
            values[n / 2]
        } else {
            (values[n / 2 - 1] + values[n / 2]) / 2.0
        };
        let std = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64).sqrt();

        let mut props = feature.properties.clone().unwrap_or_default();
        props.insert("zone_id".into(), json!(idx));
        props.insert("cell_count".into(), json!(n));
        props.insert(
            "zonal_min".into(),
            json!((values[0] * 1000.0).round() / 1000.0),
        );
        props.insert(
            "zonal_max".into(),
            json!((values[n - 1] * 1000.0).round() / 1000.0),
        );
        props.insert("zonal_mean".into(), json!((mean * 1000.0).round() / 1000.0));
        props.insert(
            "zonal_median".into(),
            json!((median * 1000.0).round() / 1000.0),
        );
        props.insert("zonal_std".into(), json!((std * 1000.0).round() / 1000.0));
        // Majority: most frequent rounded value.
        let mut freq: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        for v in &values {
            *freq.entry((*v * 100.0).round() as i64).or_insert(0) += 1;
        }
        if let Some((maj, _)) = freq.iter().max_by_key(|(_, c)| **c) {
            props.insert("zonal_majority".into(), json!(*maj as f64 / 100.0));
        }
        zone_count += 1;
        out_features.push(geojson::Feature {
            bbox: feature.bbox.clone(),
            geometry: feature.geometry.clone(),
            id: feature.id.clone(),
            properties: Some(props),
            foreign_members: feature.foreign_members.clone(),
        });
    }

    if zone_count == 0 {
        return Err("no polygon overlapped the raster extent".into());
    }
    Ok((
        geojson::FeatureCollection {
            bbox: None,
            features: out_features,
            foreign_members: None,
        },
        json!({ "zones": zone_count, "stats": ["min", "max", "mean", "median", "std", "majority", "count"] }),
    ))
}

/// Viewshed: binary visibility grid from an observer over the terrain
/// using line-of-sight rays to every cell (Bresenham walk).
pub fn viewshed(grid: &RasterGrid, lng: f64, lat: f64, observer_height_m: f64) -> RasterGrid {
    let (cell_w, cell_h) = grid.cell_size();
    let ox = ((lng - grid.bbox.0) / cell_w)
        .floor()
        .max(0.0)
        .min((grid.width - 1) as f64) as usize;
    let oy = ((grid.bbox.3 - lat) / cell_h)
        .floor()
        .max(0.0)
        .min((grid.height - 1) as f64) as usize;
    let observer_z = grid.get(ox, oy).unwrap_or(0.0) + observer_height_m;
    let meters_per_deg = 111_320.0;

    let mut out = grid.clone();
    for ty in 0..grid.height {
        for tx in 0..grid.width {
            let idx = ty * grid.width + tx;
            out.data[idx] = if (tx, ty) == (ox, oy)
                || los_visible(grid, ox, oy, observer_z, tx, ty, meters_per_deg)
            {
                1.0
            } else {
                0.0
            };
        }
    }
    out.nodata = None;
    out
}

fn los_visible(
    grid: &RasterGrid,
    ox: usize,
    oy: usize,
    observer_z: f64,
    tx: usize,
    ty: usize,
    meters_per_deg: f64,
) -> bool {
    let (cell_w, cell_h) = grid.cell_size();
    let target_z = grid.get(tx, ty).unwrap_or(0.0);
    let dx_cells = (tx as f64 - ox as f64) * cell_w * meters_per_deg;
    let dy_cells = (ty as f64 - oy as f64) * cell_h * meters_per_deg;
    let dist = (dx_cells * dx_cells + dy_cells * dy_cells).sqrt().max(1e-6);
    let slope_to_target = (target_z - observer_z) / dist;

    // Walk the line with Bresenham; any intermediate terrain above the
    // sight line blocks visibility.
    let (mut x0, mut y0) = (ox as i64, oy as i64);
    let (x1, y1) = (tx as i64, ty as i64);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        if (x0, y0) != (ox as i64, oy as i64) && (x0, y0) != (x1, y1) {
            if let Some(z) = grid.get(x0 as usize, y0 as usize) {
                let ix = (x0 as f64 - ox as f64) * cell_w * meters_per_deg;
                let iy = (y0 as f64 - oy as f64) * cell_h * meters_per_deg;
                let id = (ix * ix + iy * iy).sqrt().max(1e-6);
                let sight_z = observer_z + slope_to_target * id;
                if z > sight_z {
                    return false;
                }
            }
        }
        if (x0, y0) == (x1, y1) {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    true
}

/// Convert a result grid into a decimated GeoJSON point grid for map display.
pub fn grid_to_points(
    grid: &RasterGrid,
    value_name: &str,
    max_points: usize,
) -> geojson::FeatureCollection {
    let (cell_w, cell_h) = grid.cell_size();
    let step = ((grid.width * grid.height) as f64 / max_points as f64)
        .sqrt()
        .ceil()
        .max(1.0) as usize;
    let mut features = Vec::new();
    let mut y = 0usize;
    while y < grid.height {
        let mut x = 0usize;
        while x < grid.width {
            if let Some(v) = grid.get(x, y) {
                if v.is_finite() {
                    let lng = grid.bbox.0 + cell_w * (x as f64 + 0.5);
                    let lat = grid.bbox.3 - cell_h * (y as f64 + 0.5);
                    features.push(geojson::Feature {
                        bbox: None,
                        geometry: Some(geojson::Geometry::new(geojson::Value::Point(vec![
                            lng, lat,
                        ]))),
                        id: None,
                        properties: Some(serde_json::Map::from_iter([(
                            value_name.to_string(),
                            json!((v * 1000.0).round() / 1000.0),
                        )])),
                        foreign_members: None,
                    });
                }
            }
            x += step;
        }
        y += step;
    }
    geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    }
}

/// Summary stats of a grid (used across raster tools).
pub fn grid_summary(grid: &RasterGrid) -> (usize, f64, f64, f64) {
    let vals: Vec<f64> = grid
        .data
        .iter()
        .copied()
        .filter(|v| {
            v.is_finite()
                && grid
                    .nodata
                    .map(|nd| (v - nd).abs() > f64::EPSILON)
                    .unwrap_or(true)
        })
        .collect();
    let count = vals.len();
    if count == 0 {
        return (0, 0.0, 0.0, 0.0);
    }
    let min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (count, min, max, vals.iter().sum::<f64>() / count as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_grid() -> RasterGrid {
        // 8x8 cone: value = 10 - distance from center (peak in middle).
        let (w, h) = (8usize, 8usize);
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f64 - 3.5).powi(2) + (y as f64 - 3.5).powi(2)).sqrt();
                data.push((10.0 - d).max(0.0));
            }
        }
        RasterGrid {
            width: w,
            height: h,
            data,
            nodata: None,
            bbox: (0.0, 0.0, 0.08, 0.08),
        }
    }

    fn tiny_tiff(width: u32, height: u32, values: &[u16]) -> Vec<u8> {
        // Build a minimal little-endian uncompressed 16-bit TIFF.
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&42u16.to_le_bytes());
        buf.extend_from_slice(&8u32.to_le_bytes()); // IFD offset
        let entries: Vec<(u16, u16, u32, Vec<u8>)> = vec![
            (256, 3, 1, (width as u16).to_le_bytes().to_vec()),
            (257, 3, 1, (height as u16).to_le_bytes().to_vec()),
            (258, 3, 1, 16u16.to_le_bytes().to_vec()),
            (259, 3, 1, 1u16.to_le_bytes().to_vec()),
            (273, 4, 1, {
                let strip_off = 8 + 2 + 7 * 12 + 4;
                (strip_off as u32).to_le_bytes().to_vec()
            }),
            (277, 3, 1, 1u16.to_le_bytes().to_vec()),
            (
                279,
                4,
                1,
                ((values.len() * 2) as u32).to_le_bytes().to_vec(),
            ),
        ];
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, typ, count, value) in &entries {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&typ.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
            let mut v = value.clone();
            v.resize(4, 0);
            buf.extend_from_slice(&v);
        }
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0
        for v in values {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf
    }

    #[test]
    fn test_parse_geotiff_roundtrip() {
        let values: Vec<u16> = (0..64).map(|i| (i * 7) as u16).collect();
        let bytes = tiny_tiff(8, 8, &values);
        let grid = parse_geotiff(&bytes).unwrap();
        assert_eq!(grid.width, 8);
        assert_eq!(grid.height, 8);
        assert_eq!(grid.data[5], 35.0);
    }

    #[test]
    fn test_rejects_compression() {
        let mut bytes = tiny_tiff(2, 2, &[1, 2, 3, 4]);
        // Patch compression tag value (entry index 3, offset 8+2+3*12+8).
        let comp_off = 8 + 2 + 3 * 12 + 8;
        bytes[comp_off] = 5; // LZW
        assert!(parse_geotiff(&bytes).is_err());
    }

    #[test]
    fn test_slope_flat_grid_is_zero() {
        let mut grid = synthetic_grid();
        grid.data = vec![5.0; 64];
        let slope = slope_degrees(&grid);
        assert!(slope.data.iter().all(|&s| s.abs() < 1e-9));
    }

    #[test]
    fn test_hillshade_range() {
        let shade = hillshade(&synthetic_grid(), 315.0, 45.0);
        assert!(shade.data.iter().all(|&v| (0.0..=1.0).contains(&v)));
    }

    #[test]
    fn test_calculator_arithmetic() {
        let grid = synthetic_grid();
        let doubled = raster_calculator("a * 2 + 1", &grid, None).unwrap();
        assert!((doubled.data[0] - (grid.data[0] * 2.0 + 1.0)).abs() < 1e-9);
        let b = raster_calculator("sqrt(a) ", &grid, None).unwrap();
        assert!((b.data[10] - grid.data[10].sqrt()).abs() < 1e-9);
    }

    #[test]
    fn test_calculator_two_rasters() {
        let a = synthetic_grid();
        let mut b = a.clone();
        b.data = vec![2.0; 64];
        let out = raster_calculator("a / b", &a, Some(&b)).unwrap();
        assert!((out.data[3] - a.data[3] / 2.0).abs() < 1e-9);
        // Division by zero -> NaN (nodata).
        let z = raster_calculator("a / (b - b)", &a, Some(&b)).unwrap();
        assert!(z.data[0].is_nan());
    }

    #[test]
    fn test_flow_accumulation_pit_collects_everything() {
        // Inverted cone: the center is the only pit, so all 64 cells drain to it.
        let mut grid = synthetic_grid();
        for y in 0..grid.height {
            for x in 0..grid.width {
                let i = y * grid.width + x;
                let d = ((x as f64 - 3.5).powi(2) + (y as f64 - 3.5).powi(2)).sqrt();
                // Subtle tilt breaks elevation ties so (3,3) is the unique pit.
                grid.data[i] = d + x as f64 * 1e-3 + y as f64 * 1e-4;
            }
        }
        let acc = flow_accumulation(&grid).unwrap();
        let center = 3 * 8 + 3;
        let max = acc.data.iter().cloned().fold(0.0, f64::max);
        assert!((acc.data[center] - max).abs() < 1e-9);
        assert!(acc.data[center] >= 60.0);
    }

    #[test]
    fn test_zonal_statistics() {
        let grid = synthetic_grid();
        let poly = geojson::FeatureCollection {
            bbox: None,
            features: vec![geojson::Feature {
                bbox: None,
                geometry: Some(geojson::Geometry::new(geojson::Value::Polygon(vec![vec![
                    vec![0.01, 0.01],
                    vec![0.07, 0.01],
                    vec![0.07, 0.07],
                    vec![0.01, 0.07],
                    vec![0.01, 0.01],
                ]]))),
                id: None,
                properties: None,
                foreign_members: None,
            }],
            foreign_members: None,
        };
        let (out, summary) = zonal_statistics(&grid, &poly).unwrap();
        assert_eq!(summary["zones"], 1);
        let props = out.features[0].properties.as_ref().unwrap();
        assert!(props["cell_count"].as_u64().unwrap() > 4);
        assert!(props["zonal_max"].as_f64().unwrap() > props["zonal_min"].as_f64().unwrap());
    }

    #[test]
    fn test_viewshed_observer_sees_flat_ground() {
        let mut grid = synthetic_grid();
        grid.data = vec![1.0; 64]; // flat terrain
        let vs = viewshed(&grid, 0.04, 0.04, 5.0);
        assert_eq!(vs.data.iter().filter(|&&v| v == 1.0).count(), 64);
    }
}
