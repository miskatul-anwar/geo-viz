# Changelog

All notable changes to GeoViz are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
