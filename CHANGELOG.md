# Changelog

All notable changes to GeoViz are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [3.0.0] - 2026-08-24

Major release: **offline GIS toolbox parity** — the analytical toolboxes mandated by the specification sheet (`GIS Clone Specification Sheet.md`), with a full compliance matrix in `docs/SPEC-COMPLIANCE.md`. The tool catalog grows from 13 to 32 tools; backend test suite grows from 24 to 56.

### Added — Specification compliance release (offline GIS toolbox parity)

- **Spatial Statistics toolbox**: mean center, median center (Weiszfeld), linear directional mean, Global Moran's I (z/p significance), Getis-Ord Gi* hot spot analysis (adaptive distance band, 95%/99% classes), and OLS regression (≤6 regressors, R²/adj-R²/AIC, per-feature residual layer).
- **Geostatistical Analyst**: IDW interpolation and Ordinary Kriging with spherical/exponential/gaussian semivariogram autofit — kriging emits prediction **and standard-error** surfaces.
- **Network Analyst**: topological graph construction from line layers (endpoint snapping), Dijkstra and A* shortest paths, service-area isochrones (reachable edges + hull), and origin-destination cost matrices.
- **Topology engine**: `must_not_overlap`, `must_not_have_dangles` and `must_be_covered_by` rules producing a violations layer with machine-actionable `suggested_fix` attributes.
- **Spatial Analyst (raster)**: uncompressed single-band GeoTIFF ingestion (stored in a new `rasters` table), Horn slope & aspect, Lambertian hillshade, map-algebra raster calculator (`+ - * /`, `sqrt/log/abs/min/max`, two rasters), D8 flow direction + flow accumulation, polygon zonal statistics (min/max/mean/median/std/majority), and Bresenham viewshed. Import `.tif/.tiff` via Add Layer.
- **Attribute joins**: paste-a-CSV table join by primary key with `join_*` column prefixing.
- **Cartography**: all QGIS blending modes (multiply, screen, overlay, …, luminosity) executed per layer via dedicated compositor panes.
- **Table ↔ map bidirectional selection**: clicking a map feature highlights and reveals its row in the (already virtualized) attribute table; clicking a row still zooms to the feature.

### Fixed
- **Layer visibility toggle**: unchecking and re-checking a layer left it permanently invisible — the map-sync signature was not recorded when hiding, so the identical signature on re-check skipped the rebuild. Hidden state is now recorded, restoring the toggle cycle.
- **Layer symbology inputs ignored user choices**: the Classes field/method/count were a single shared draft across all layers, so each layer's panel pre-showed another layer's selection (or the default option when the field was absent from that layer's schema) and Apply could silently use stale values. Drafts are now per-layer.
- **Classification overwrote the user's fill color** with a hardcoded neutral; the base color is now preserved (classified features are still colored per-class).
- **Label/class dropdowns could render with only the default option** when the dataset schema wasn't loaded yet; expanding symbology now resolves the schema on demand.

### Removed
- **Layer Matrix view**: redundant with the layer manager and calculator input selectors; the cross-view intent queue (`QueueCalculationFor`/`ConsumePendingCalculation`) it existed to serve was removed with it.

## [2.0.0] - 2026-08-24

Major release focused on **UI/UX quality and structural discipline**.

### Added
- **Fluid, viewport-aware layout**: rem-based spacing tokens, 4K scale-up (wider panels, 18px root type at ≥2000px), slim side rails at ≤1024px, stacked single-column workspaces at ≤860px, and icon-only toolbars at ≤640px — no horizontal scrolling or overlapping chrome from 4K down to the smallest window.
- **Functional density pass**: reclaimed decorative whitespace across the matrix view, results panel, tool cards and config headers without shrinking controls.

### Changed
- **Stylesheet decomposition**: the 1,550-line `app.css` monolith is split into seven single-responsibility sheets (`tokens` → `base` → `layout` → `map` → `calculations` → `data` → `responsive`), loaded in strict cascade order so responsive overrides always win.
- **Larger, tactile controls**: buttons, nav tabs, basemap switcher, bookmark panel and matrix row actions enlarged (13px/12px labels, taller hit areas).
- **Recent runs feed**: calculation history surfaced via the new `list_calculation_history` IPC command; the telemetry panel lists recent tool runs with duration.
- **Result provenance**: results now name the tool and input layer that produced them.

### Fixed
- Metric cards rendered nested payloads as "…"; nested objects/arrays are now flattened into readable rows (e.g. "Population Mean").
- Actionable summaries across tools: buffer reports total buffered area, hull/bbox report enclosed area, overlay/clip report output area, dissolve reports merged area + feature reduction %, spatial query reports match rate %, spatial join reports unmatched count.

### Verified
- **Architecture audit**: no IPC outside the typed `TauriService` wrapper; no business logic or persistence in components (backend-heavy/frontend-thin boundary intact); layer sync remains signature-diffed and cache-backed (no over-fetching).
- Headless-browser walkthrough at 2560/1280/920/620 px: zero page errors, zero horizontal scroll, satellite basemap loads 24/24 tiles under the production CSP.

## [1.0.0] - 2026-08-24

First stable release. The IPC surface, persistence schema and tool catalogue are considered dependable for daily use.

### Added
- **Layer Matrix view** — a new top-level workspace listing every layer with format, feature count, geometry types and extent; one-click *Operate* (queues the layer into the calculator) or *Map* (select + jump).
- **Layer-driven analytics**: every calculator input selector now lists the layers you actually added (with feature counts), polygon-only selectors filter appropriately, and the active map layer is preselected automatically.
- **Actionable results**: tool summaries now render as a dynamic metric grid straight from the backend payload (areas, counts, densities — humanized), plus a scrollable result attribute preview (up to 200 rows) so outputs are inspectable without leaving the view.
- **Add to Map** now zooms to the result layer and switches to Map Studio, closing the analysis loop.
- Cross-view intent queue (`QueueCalculationFor`) decouples matrix/calculator/map navigation through `AppState`.

### Fixed
- **Satellite basemap**: Google tile scraping was dead (HTTP failures); replaced with Esri World Imagery and allow-listed in the CSP.

### Changed
- UI density pass: larger, more tactile buttons across toolbars/tabs/icon actions while tightening chrome spacing; functional-density styling for matrix and result views.

## [0.2.1] - 2026-08-24

### Fixed
- **Critical**: importing datasets (GeoJSON/Shapefile/KML/KMZ/GPX/GeoPackage) and running geoprocessing tools failed with `invalid args … missing required key <camelCase>`. Tauri expects camelCase argument keys by default while GeoViz sends snake_case; every command now declares `#[tauri::command(rename_all = "snake_case")]` so the IPC contract matches the codebase-wide snake_case convention. Documented in ARCHITECTURE.md and enforced via the contributing checklist.


## [0.2.0] - 2026-08-24

### Added
- **New ingestion formats**: KML, KMZ (zipped KML), GPX (waypoints/tracks/routes), and GeoPackage (`.gpkg`) with a built-in GPKG/WKB decoder.
- **Overlay geoprocessing**: Intersection, Difference, Symmetric Difference, and Clip against any polygon layer — implemented with native boolean operations in the Rust engine.
- **Dissolve / Union** tool with optional attribute-based grouping; pairwise union folding merges adjacent polygons.
- **Spatial Join** tool: attaches target-layer attributes (`sj_*` prefix) plus match counts onto source features by containment.
- **Categorized & graduated symbology**: equal-interval and quantile class breaks computed backend-side, rendered per-feature on the map with an interpolated sequential color ramp.
- **Attribute labeling**: render any schema field as permanent on-map labels.
- **Spatial bookmarks**: save/jump/delete persisted map views from the map toolbar.
- Map empty-state hint when no layers are visible.
- Keyboard focus outlines, disabled-button affordances, themed scrollbars.

### Changed
- Import pipeline is fully backend-owned: one IPC call persists dataset + provisions a styled layer (was 4+ calls orchestrated in the UI).
- Layer synchronization uses signature diffing — style/geometry changes rebuild only affected layers (fixes redundant full-map refetch storms).
- `AppState` introduced as the single client-side state container; pages/components are now thin renderers.
- Tool execution unified behind one Rust service with centralized timing, history logging and typed errors.

### Fixed
- Fatal `layerStyle is not defined` crash in the map bridge that broke all layer rendering.
- Polygon masks were ignored by spatial query unless the relation string was exactly `within_polygons`; containment now applies whenever a mask is supplied.
- SQLite pragmas (`foreign_keys`, WAL) now applied to every pooled connection via connect options instead of one connection.
- UTF-8 panic in the WKT nested-group parser on multi-byte characters.
- FormatConverter file-picker interop signature mismatch (runtime failure).
- Popup property values are HTML-escaped (injection-safe rendering).
- Mousemove telemetry throttled to 10 Hz (constant re-renders removed).
- Invalid short hex color assigned after classification.

### Removed
- Dead code: bookmarks/history stubs without UI, unused dependencies (`thiserror`, `tauri-plugin-opener`), CDN scripts (Motion, Google Fonts) — Leaflet is now vendored locally for full offline capability.

## [0.1.0] - initial internal build

- GeoJSON / Shapefile ingestion, Leaflet map studio with styling & measurement
- Ten geoprocessing tools, SQL console, format converter (GeoJSON ⇄ WKT ⇄ CSV)
- Tauri packaging for Windows (NSIS/MSI), macOS (DMG/universal) and Linux (deb/rpm/AppImage)
