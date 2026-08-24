<div align="center">

# GeoViz

**Free, open-source desktop GIS for spatial analysis — a lightweight alternative to ArcGIS Pro and QGIS**

[![CI](https://img.shields.io/github/actions/workflow/status/miskatul-anwar/geo-viz/build-all.yml?branch=main&label=build)](https://github.com/miskatul-anwar/geo-viz/actions/workflows/build-all.yml)
[![Tests](https://img.shields.io/github/actions/workflow/status/miskatul-anwar/geo-viz/tests.yml?branch=main&label=tests)](https://github.com/miskatul-anwar/geo-viz/actions/workflows/tests.yml)
[![Release](https://img.shields.io/github/v/release/miskatul-anwar/geo-viz)](https://github.com/miskatul-anwar/geo-viz/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)](#install)

*Rust engine · Blazor WebAssembly UI · Tauri shell · SQLite persistence · Fully offline-capable*

</div>

---

GeoViz is a cross-platform **desktop GIS application** for ingesting, visualizing, analyzing and converting geodata — vector **and** raster. Heavy lifting — parsing, geoprocessing, spatial statistics, interpolation, network routing, raster algebra — happens in a native Rust engine; the interface is a fast, sandboxed web view. No telemetry, no accounts, no cloud: your data never leaves your machine.

## ✨ Features

**Data ingestion**
| Format | Kind | Notes |
|---|---|---|
| GeoJSON / JSON | Vector | Text or paste |
| ESRI Shapefile (.shp / .zip) | Vector | Binary + zipped archives |
| KML | Vector | Placemarks, ExtendedData, MultiGeometry |
| KMZ (zipped KML) | Vector | |
| GPX | Vector | Waypoints, tracks, routes |
| GeoPackage (.gpkg) | Vector | GPKG blob/WKB decoding built in |
| GeoTIFF (.tif / .tiff) | Raster | Uncompressed single-band grids for the Spatial Analyst tools |
| WKT / CSV | Vector | Via the Format Converter |

**Map studio**
- Leaflet canvas with Dark / Light / OSM / Satellite / Topo basemaps
- Per-layer styling: fill/stroke colors, opacity, stroke width, point radius, shape rendering mode
- **Categorized & graduated symbology** — equal-interval or quantile class breaks computed by the Rust engine
- **Attribute labeling** rendered directly on the map
- **16 blending modes** (multiply, screen, overlay, difference, …) — QGIS-style layer compositing
- Distance & area measurement tools
- **Spatial bookmarks** (persisted map views)
- Live coordinate readout, layer TOC with visibility toggles
- Attribute table: virtualized for large datasets, search, sorting, column statistics, CSV export, and **bidirectional map↔table selection** (click a feature → its row highlights; click a row → zoom to the feature)

**Geoprocessing toolbox** (32 tools, all executed natively in Rust)

*Fundamental analysis*
- Buffer analysis · Convex hull · Centroids · Bounding boxes
- Douglas-Peucker simplification · Spherical area/perimeter metrics
- Spatial query (polygon containment + attribute filters) · Hexbin/square density binning
- Nearest-neighbor distance matrix · Random point sampling
- **Overlay analysis**: Intersection, Difference, Symmetric difference, Clip
- **Dissolve / Union** (optionally grouped by attribute)
- **Spatial join** (attach attributes by location) · **CSV attribute join** (attach columns by key)

*Spatial statistics*
- Mean & median center (Weiszfeld iteration) · Linear directional mean
- **Global Moran's I** spatial autocorrelation (z-score + p-value)
- **Getis-Ord Gi\* hot spot analysis** (adaptive distance band, 95%/99% significance classes)
- **OLS regression** (up to 6 explanatory fields, R²/adj-R²/AIC, per-feature residuals)

*Geostatistics*
- **IDW interpolation** (power + neighbor controls)
- **Ordinary Kriging** with spherical/exponential/gaussian semivariogram autofit — prediction **and** standard-error surfaces

*Network analysis*
- **Shortest path** via Dijkstra or A* (haversine heuristic) over endpoint-snapped line graphs
- **Service areas** (network isochrones with hull) · **OD cost matrices**

*Data integrity*
- **Topology validation**: `must_not_overlap`, `must_not_have_dangles`, `must_be_covered_by` — violations render as a layer with suggested fixes

**Spatial Analyst (raster)**
- Horn **slope** & **aspect** · Lambertian **hillshade** (configurable azimuth/altitude)
- **Raster calculator** (map algebra: `+ - * /`, `sqrt/log/abs/min/max`, two rasters)
- **D8 flow direction & accumulation** · **Viewshed** (line-of-sight visibility)
- **Zonal statistics** (min/max/mean/median/std/majority per polygon)

**Utilities**
- SQL console over the embedded project database (read-only guardrails)
- Bidirectional format converter: GeoJSON ⇄ WKT ⇄ CSV
- Multi-tab calculation workspaces with run history logging

## 🆚 How it compares

| Capability | GeoViz | QGIS | ArcGIS Pro |
|---|---|---|---|
| License | MIT, free forever | GPL, free | Proprietary, paid |
| Footprint | ~20 MB installed | > 1 GB | > 5 GB |
| Vector formats in/out | Core set (see above) | Hundreds via GDAL | Hundreds via GDAL |
| Vector overlay/geoprocessing | ✅ | ✅ | ✅ |
| Spatial statistics (Moran's I, Gi\*, OLS) | ✅ | ✅ | ✅ |
| Interpolation (IDW, Kriging) | ✅ | ✅ | ✅ |
| Network routing (shortest path, service areas) | ✅ | ✅ | ✅ |
| Raster analysis (slope, hillshade, algebra, D8) | ✅ | ✅ | ✅ |
| Layer blending modes | ✅ (16) | ✅ (13) | ⚠️ subset |
| Coordinate reprojection | ❌ planned (EPSG:4326 today) | ✅ | ✅ |
| Offline / privacy-first | ✅ | ✅ | ⚠️ telemetry |

*GeoViz aims to cover the analytical core of the big GIS platforms as a single small, offline binary. See [docs/SPEC-COMPLIANCE.md](docs/SPEC-COMPLIANCE.md) for the detailed capability matrix, implementation limits and roadmap.*

## 📦 Install

Grab an installer from [**Releases**](https://github.com/miskatul-anwar/geo-viz/releases/latest):

| Platform | Artifact |
|---|---|
| Debian / Ubuntu | `geo-viz_*_amd64.deb` → `sudo apt install ./geo-viz_*.deb` |
| Fedora / RHEL | `geo-viz-*-1.x86_64.rpm` → `sudo rpm -i geo-viz-*.rpm` |
| Any Linux | `geo-viz_*_amd64.AppImage` → `chmod +x && run` |
| Windows | `GeoViz_*_x64-setup.exe` (NSIS) or `*.msi` |
| macOS (Intel + Apple Silicon) | `GeoViz_*.dmg` |

## 🛠 Build from source

Prerequisites: [.NET 8 SDK](https://dotnet.microsoft.com/download), [Rust](https://rustup.rs), [Tauri CLI](https://tauri.app), platform webkit deps (Linux).

```bash
# Linux (deb + rpm + AppImage)
./build-linux.sh

# macOS (.app + .dmg, universal when both targets installed)
./build-macos.sh

# Windows (NSIS + MSI)
.\build-windows.ps1

# Dev loop
cargo tauri dev
```

Run the test suites:

```bash
cargo test --manifest-path src-tauri/Cargo.toml   # 56 backend tests (algorithms + services)
dotnet build src/GeoViz.csproj                    # frontend build validated in CI
```

## 🏗 Architecture

```
┌──────────────────────────── Browser-grade UI (thin) ───────────────────────────┐
│  Blazor WASM components ── AppState (single source of truth)                   │
│        │                                  │                                    │
│        ▼                                  ▼                                    │
│  map_bridge.js (Leaflet)         TauriService (typed IPC wrapper)              │
└────────────────┬──────────────────────────────┬─────────────────────────────────┘
                 │ IPC                          │ invoke()
┌────────────────▼──────────────────────────────▼─────────────────────────────────┐
│                       Native engine (Rust — heavy layer)                        │
│  commands.rs (thin adapters) → services/ (import, tools) → gis/ (pure algos)    │
│  gis/: vector geoprocessing · spatial statistics · geostatistics · network ·    │
│         topology · raster (GeoTIFF) · joins · classification                    │
│  db.rs: SQLite (WAL) — datasets, layers, rasters, tabs, history, bookmarks      │
└──────────────────────────────────────────────────────────────────────────────────┘
```

Design rules: high cohesion per module, low coupling across layers, all orchestration server-side (in-app), typed errors end-to-end, zero business logic in the UI.

See [ARCHITECTURE.md](ARCHITECTURE.md) for details and [docs/SPEC-COMPLIANCE.md](docs/SPEC-COMPLIANCE.md) for the feature-compliance matrix.

## 🗺 Roadmap

- [ ] Compressed/tiled GeoTIFF (LZW, Deflate) + NetCDF/HDF5 drivers
- [ ] Geographically Weighted Regression & Empirical Bayesian Kriging
- [ ] Watershed delineation on the D8 primitives
- [ ] Field calculator (expression-driven attribute computation)
- [ ] Web Mercator / UTM reprojection on import/export
- [ ] Print layout / map image export
- [ ] Directed networks: one-way streets & turn penalties
- [ ] Python automation surface over the IPC commands

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

## 🤝 Contributing

Issues and pull requests are welcome! Good first issues are labeled [`good first issue`](https://github.com/miskatul-anwar/geo-viz/labels/good%20first%20issue). Please read the [contributing guide](CONTRIBUTING.md) and our [code of conduct](CODE_OF_CONDUCT.md).

## 🔒 Security

Found a vulnerability? See [SECURITY.md](SECURITY.md) — please do not open public issues for security reports.

## 📄 License

[MIT](LICENSE) © [Miskatul Anwar](https://github.com/miskatul-anwar) and contributors.

---

<div align="center">
<sub>If GeoViz saves you time, consider starring ⭐ the repo — it helps others discover it.</sub>
</div>
