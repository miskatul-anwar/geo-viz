# Architecture

GeoViz follows a **backend-heavy, frontend-thin** split: all geospatial computation, orchestration and persistence live in Rust; the web layer only renders state and forwards user intent.

## Layer map

```
src-tauri/src/
├── commands.rs        IPC adapters (1-line bodies; zero logic)
├── services/
│   ├── dataset_service.rs   Import pipeline: parse → persist → provision layer
│   └── tool_service.rs      Unified tool runner: resolve inputs, compute, log history
├── gis/               Pure algorithms — no framework code, fully unit-tested
│   ├── parser.rs            GeoJSON normalization + schema/bbox inference
│   ├── shapefile_reader.rs  SHP / ZIP archives
│   ├── kml.rs / gpx.rs      XML formats via gis/xml_tree.rs
│   ├── gpkg.rs              GeoPackage (SQLite metadata + GPKG/WKB blobs)
│   ├── overlay.rs           Intersection/Difference/Xor/Clip/Dissolve (geo::BooleanOps)
│   ├── spatial_join.rs      Attribute transfer by containment
│   ├── classification.rs    Equal-interval & quantile breaks + color ramp
│   ├── buffer|centroid|convex_hull|bbox|simplify|metrics|
│   │   spatial_binning|spatial_query|distance_matrix|random_points
│   └── format_convert.rs    GeoJSON ⇄ WKT ⇄ CSV
├── db.rs              SQLite (WAL, per-connection pragmas) — datasets/layers/tabs/history/bookmarks
├── models.rs          Wire types shared over IPC
└── error.rs           AppError enum; serializes to human-readable strings for the UI

src/ (Blazor WASM)
├── Services/AppState.cs     Single source of truth; every mutation funnels through it
├── Services/TauriService.cs Typed wrapper over window.__TAURI__.invoke
├── Components/              Render AppState + emit intents (no direct data logic)
└── wwwroot/js/map_bridge.js The only module that touches Leaflet
```

## Key decisions

1. **One IPC call per user action.** Importing a file persists the dataset *and* provisions a styled layer server-side (`ImportOutcome`), instead of the UI choreographing four calls.
2. **Tool execution is centralized** in `tool_service::run_tool`: dataset resolution → optional secondary-input resolution → pure compute → timing → best-effort history logging. Adding a tool means: one `gis/` function, one `ToolKind` arm, one thin command, one test.
3. **Signature-diffed map sync.** `MapCanvas` hashes each layer's geometry length + style (+ classification + label field) and rebuilds only layers whose signature changed — no full-map refetch storms.
4. **Typed errors everywhere.** `AppError` converts `sqlx`/`serde_json`/IO/base64 errors at the boundary and serializes as a presentable string across IPC; the UI surfaces it in one banner.
5. **Per-connection SQLite pragmas** via `SqliteConnectOptions` (WAL persistence is database-level, but `foreign_keys`/`busy_timeout` are connection-level — a common sqlx pitfall).
6. **Offline-first assets**: Leaflet vendored under `wwwroot/lib`, no CDN scripts or fonts; strict CSP allow-lists tile hosts for `img-src`.
7. **IPC naming convention**: Tauri expects *camelCase* argument keys by default, but this codebase standardizes on **snake_case** end-to-end. Every command therefore MUST be declared `#[tauri::command(rename_all = "snake_case")]` and every JS/C# call site must send snake_case keys (matching Rust parameter names exactly). A missing rename attribute surfaces at runtime as `invalid args … missing required key <camelCaseName>`. Extra keys in an args object are ignored, and `Option<T>` parameters may be omitted entirely.

## Testing strategy

- `gis/` modules: pure-function unit tests including fixture files built in-test (KML/GPX strings, synthesized GeoPackage).
- Services: end-to-end async tests against temp SQLite databases.
- Regression tests accompany every bug fix (e.g., spatial-query mask gating).
- CI runs fmt/clippy(-D warnings)/cargo test/dotnet build on all three platforms; release workflow produces signed bundles on tags.

## Adding a feature checklist

- [ ] Algorithm in `gis/` with unit tests
- [ ] Service/command wiring + registration in `lib.rs`
- [ ] C# model + `TauriService` method + `AppState` operation
- [ ] Component UI + `CHANGELOG.md` entry
- [ ] `cargo clippy --all-targets` clean, all tests green
