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

GeoViz is a cross-platform **desktop GIS application** for ingesting, visualizing, analyzing and converting vector geodata. Heavy lifting — parsing, geoprocessing, classification, persistence — happens in a native Rust engine; the interface is a fast, sandboxed web view. No telemetry, no accounts, no cloud: your data never leaves your machine.

## ✨ Features

**Data ingestion**
| Format | Read | Notes |
|---|---|---|
| GeoJSON / JSON | ✅ | Text or paste |
| ESRI Shapefile (.shp / .zip) | ✅ | Binary + zipped archives |
| KML | ✅ | Placemarks, ExtendedData, MultiGeometry |
| KMZ (zipped KML) | ✅ | |
| GPX | ✅ | Waypoints, tracks, routes |
| GeoPackage (.gpkg) | ✅ | GPKG blob/WKB decoding built in |
| WKT / CSV | ✅ | Via the Format Converter |

**Map studio**
- Leaflet canvas with Dark / Light / OSM / Satellite / Topo basemaps
- Per-layer styling: fill/stroke colors, opacity, stroke width, point radius, shape rendering mode
- **Categorized & graduated symbology** — equal-interval or quantile class breaks computed by the Rust engine
- **Attribute labeling** rendered directly on the map
- Distance & area measurement tools
- **Spatial bookmarks** (persisted map views)
- Live coordinate readout, layer TOC with visibility toggles
- Attribute table: virtualized for large datasets, search, sorting, column statistics, CSV export

**Geoprocessing toolbox** (all executed natively in Rust)
- Buffer analysis · Convex hull · Centroids · Bounding boxes
- Douglas-Peucker simplification · Spherical area/perimeter metrics
- Spatial query (polygon containment + attribute filters) · Hexbin/square density binning
- Nearest-neighbor distance matrix · Random point sampling
- **Overlay analysis**: Intersection, Difference, Symmetric difference, Clip
- **Dissolve / Union** (optionally grouped by attribute)
- **Spatial join** (attach attributes by location)

**Utilities**
- SQL console over the embedded project database (read-only guardrails)
- Bidirectional format converter: GeoJSON ⇄ WKT ⇄ CSV
- Multi-tab calculation workspaces with run history logging

## 🆚 How it compares

| Capability | GeoViz | QGIS | ArcGIS Pro |
|---|---|---|---|
| License | MIT, free forever | GPL, free | Proprietary, paid |
| Footprint | ~15 MB installed | > 1 GB | > 5 GB |
| Vector formats in/out | Core set (see above) | Hundreds via GDAL | Hundreds via GDAL |
| Vector overlay/geoprocessing | ✅ | ✅ | ✅ |
| Raster analysis | ❌ planned | ✅ | ✅ |
| Coordinate reprojection | ❌ planned (EPSG:4326 today) | ✅ | ✅ |
| Offline / privacy-first | ✅ | ✅ | ⚠️ telemetry |

*GeoViz is intentionally small: it aims to cover the 20% of GIS operations that 80% of users need daily — not to replace full GDAL/OGR stacks.*

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
cargo test --manifest-path src-tauri/Cargo.toml   # 24 backend tests
dotnet test src/GeoViz.csproj                     # frontend builds validated in CI
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
│  db.rs: SQLite (WAL) — datasets, layers, tabs, history, bookmarks               │
└──────────────────────────────────────────────────────────────────────────────────┘
```

Design rules: high cohesion per module, low coupling across layers, all orchestration server-side (in-app), typed errors end-to-end, zero business logic in the UI.

See [ARCHITECTURE.md](ARCHITECTURE.md) for details.

## 🗺 Roadmap

- [ ] Field calculator (expression-driven attribute computation)
- [ ] GeoJSON/KML export from layers panel
- [ ] Web Mercator reprojection on import/export
- [ ] Print layout / map image export
- [ ] Raster tile overlays (XYZ)
- [ ] Plugin surface for community geoprocessing tools

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
