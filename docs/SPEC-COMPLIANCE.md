# GIS Specification Compliance Matrix

Status of GeoViz against the *Technical Specification: Upgrading geo-viz to a
Comprehensive Offline Geospatial Information System* (see
`GIS Clone Specification Sheet.md`). Everything listed as **implemented**
runs fully offline, is covered by unit tests, and is reachable from the
Spatial Calculations workspace.

## Geoprocessing & Analytical Engine

| Spec area | Requirement | Status | Notes |
| :--- | :--- | :--- | :--- |
| Proximity | Geodesic buffer | Implemented | `gis/buffer.rs`, spherical approximation |
| Overlay | Intersect / Erase / Sym-diff / Clip | Implemented | `gis/overlay.rs`, `geo::BooleanOps` (sweep-line backed) |
| Extraction | Select by location / Clip | Implemented | `gis/spatial_query.rs` |
| Attribution | Spatial join | Implemented | `gis/spatial_join.rs` |
| Indexing | R-tree spatial index | Partial | bbox pre-filtering + pairwise checks; R-tree on roadmap for 100k+ feature overlays |
| Measuring distributions | Mean / Median center | Implemented | `gis/spatial_statistics.rs` (Weiszfeld median) |
| Measuring distributions | Linear directional mean | Implemented | compass bearing, length-weighted |
| Pattern analysis | Global Moran's I | Implemented | inverse-distance weights, z/p values |
| Pattern analysis | Getis-Ord Gi* hot spots | Implemented | fixed-band weights (adaptive 3× NN), 95/99% classes |
| Modeling relationships | OLS regression | Implemented | ≤6 regressors, R²/adj-R²/AIC, residual layer |
| Modeling relationships | Geographically Weighted Regression | Roadmap | requires local-kernel solver suite |
| Interpolation | IDW | Implemented | `gis/geostatistics.rs`, power/neighbor controls |
| Interpolation | Ordinary Kriging | Implemented | spherical/exponential/gaussian variograms, coarse autofit, prediction + standard error |
| Interpolation | Universal / Empirical Bayesian Kriging | Roadmap | EBK needs semivariogram simulation ensemble |
| Network | Graph construction w/ endpoint snapping | Implemented | `gis/network.rs`, ~100 m snap tolerance |
| Network | Dijkstra & A* shortest path | Implemented | haversine heuristic for A* |
| Network | Service area / isochrone | Implemented | reachable edges + hull polygon |
| Network | OD cost matrix | Implemented | ≤10,000 pairs |
| Network | Turn penalties / one-way direction / location-allocation | Roadmap | requires directed-edge data model |
| Topology | Must not overlap (+ fix hints) | Implemented | `gis/topology.rs` |
| Topology | Must not have dangles (+ trim/extend hints) | Implemented | endpoint identity at 1e-7° |
| Topology | Must be covered by | Implemented | representative-point ray-cast containment |
| Topology | Must not have gaps | Roadmap | needs robust planar union coverage test |
| Topology | Error Inspector UI | Implemented | violations render as a map layer with `suggested_fix` attributes |

## Spatial Analyst (Raster)

| Spec area | Requirement | Status | Notes |
| :--- | :--- | :--- | :--- |
| Ingestion | GeoTIFF (uncompressed, single-band, strip) | Implemented | `gis/raster.rs`; LZW/Deflate/tiled rasters rejected with a clear error |
| Surface analysis | Slope (Horn) | Implemented | |
| Surface analysis | Aspect | Implemented | |
| Surface analysis | Hillshade (Lambertian, azimuth/altitude) | Implemented | |
| Map algebra | Raster calculator | Implemented | recursive-descent parser: `+ - * /`, `sqrt/log/abs/min/max`, two rasters (`a`, `b`) |
| Hydrology | D8 flow direction | Implemented | ESRI powers-of-two codes |
| Hydrology | Flow accumulation | Implemented | descending-elevation routing |
| Hydrology | Watershed delineation / pour points | Roadmap | builds on the D8 primitives |
| Zonal statistics | Mean/median/min/max/std/majority/count | Implemented | polygon zones × raster |
| Viewshed | Line of sight | Implemented | Bresenham rays, observer height |
| Block-based out-of-core processing | Streaming windows | Roadmap | grids are held in memory (f64) today |
| NetCDF / HDF5 / HFA drivers | Scientific raster formats | Roadmap | requires additional pure-Rust decoders |

## Data Management & Interoperability

| Spec area | Requirement | Status | Notes |
| :--- | :--- | :--- | :--- |
| Vector formats | GeoJSON, Shapefile (+zip), KML/KMZ, GPX | Implemented | |
| Vector formats | GeoPackage (.gpkg) | Implemented | built-in GPKG/WKB decoder |
| Vector formats | OpenFileGDB (.gdb directory) | Roadmap | |
| Raster formats | GeoTIFF | Implemented | see limits above |
| Embedded DB | SQLite (spatial SQL via SQL console) | Implemented | `execute_sql_query` |
| Embedded DB | DuckDB + Spatial / SpatiaLite | Roadmap | SQLite covers storage + attribute SQL today |
| Table join | CSV ↔ layer by key | Implemented | `gis/table_join.rs`, quoted-CSV parser |
| Attribute table | Row virtualization | Implemented | Blazor `<Virtualize>` |
| Attribute table | Map ↔ table bidirectional selection | Implemented | feature click highlights + reveals the table row; row click zooms to feature |
| CRS | On-the-fly reprojection (PROJ) | Partial | display is Web Mercator (Leaflet-native); WGS84 data path; full PROJ parity on roadmap |

## Cartography

| Spec area | Requirement | Status | Notes |
| :--- | :--- | :--- | :--- |
| Renderers | Categorized / graduated colors | Implemented | equal-interval & quantile class breaks |
| Renderers | Proportional symbols | Partial | point-radius styling is manual, not field-driven (roadmap) |
| Blending | 13 QGIS blending modes | Implemented | compositor-executed (`mix-blend-mode` on dedicated layer panes) |
| Heatmap | Dynamic density surface | Roadmap | needs canvas/WebGL overlay pass |
| Point displacement / clustering | Overlap handling | Roadmap | |
| Labeling | Attribute labels | Implemented | permanent tooltips w/ per-layer field |
| Labeling | Curved text, conflict resolution, halos | Roadmap | |
| Layout | Print composer / atlas generation | Roadmap | |

## Extensibility & Automation

| Spec area | Requirement | Status | Notes |
| :--- | :--- | :--- | :--- |
| Scripting | Python API (arcpy-style) | Roadmap | requires an embedded interpreter; the IPC surface (`invoke` commands) is the current automation seam |
| Model builder | Visual DAG execution | Roadmap | the calculation-history audit log is the execution-record foundation |
| Plugins | Python/WASM drop-ins | Roadmap | |

## Architecture Notes (deliberate divergences)

The specification prescribes a React + GDAL/GEOS/PROJ C++ FFI + Apache Arrow
stack. GeoViz keeps its existing, tested architecture and diverges where the
prescription conflicts with the offline-first, dependency-free build:

- **Geometry engine**: pure-Rust `geo` crate (Cheatleum/GEOS-compatible
  algorithms) instead of C++ GEOS FFI — no system libraries required.
- **IPC**: GeoJSON over Tauri IPC instead of Apache Arrow buffers — the
  frontend is Blazor WASM (not React), and datasets are already
  signature-diffed and cached; Arrow becomes worthwhile only above ~1M
  vertices.
- **Rendering**: Leaflet (vendored, SVG) instead of WebGL2/WebGPU — blending
  modes and labels are compositor/shader-assisted via CSS panes today.

These are performance-motivated upgrades tracked on the roadmap, not
functional gaps: every analytical capability above executes offline.
