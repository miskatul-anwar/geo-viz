# Contributing to GeoViz

Thanks for your interest in improving GeoViz! This document explains how to set up, work and submit changes.

## Development setup

| Tool | Version | Notes |
|---|---|---|
| .NET SDK | 8.0+ | Blazor WebAssembly frontend |
| Rust | 1.80+ (stable) | Native engine |
| Tauri CLI | 2.x | `cargo install tauri-cli --locked` |
| Linux system deps | — | `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf libsoup-3.0-dev` |

```bash
git clone https://github.com/miskatul-anwar/geo-viz && cd geo-viz
cargo tauri dev          # full dev loop with hot reload
```

## Project layout & ground rules

```
src-tauri/src/
  gis/        Pure geospatial algorithms (no IO, no framework code)
  services/   Orchestration (import pipeline, tool runner) — owns workflows
  commands.rs Thin IPC adapters ONLY (no logic)
  db.rs       All persistence; typed AppResult everywhere
src/
  Services/AppState.cs    Single client-side state container
  Components/             Render from AppState; delegate every mutation to it
  wwwroot/js/map_bridge.js  The ONLY place touching Leaflet directly
```

Non-negotiables:
1. **Backend-heavy**: new capabilities go into the Rust engine first; the UI stays a thin renderer.
2. **Typed errors**: return `AppResult<T>` / surface messages through `AppState.Error`; never swallow exceptions.
3. **Tests**: every new `gis/` algorithm or service path needs `tests.rs` coverage. PRs that lower coverage will be asked to add tests.
4. **IPC contract**: commands must use `#[tauri::command(rename_all = "snake_case")]` and call sites must pass snake_case keys matching Rust parameters (see ARCHITECTURE.md → IPC naming convention).
5. **No comments unless they explain *why***; keep code self-describing.
6. Run before pushing:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets   # zero warnings required
cargo test  --manifest-path src-tauri/Cargo.toml                 # all tests green
dotnet build src/GeoViz.csproj -c Release                        # zero warnings required
node --check src/wwwroot/js/map_bridge.js                        # if you touched bridges
```

CI enforces exactly this on every pull request.

## Submitting changes

1. Fork → branch (`feat/my-feature` or `fix/my-fix`) off `main`.
2. Keep commits atomic; use [Conventional Commits](https://www.conventionalcommits.org/) style (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).
3. Fill in the PR template; link related issues (`Closes #123`).
4. For UI changes include before/after screenshots.
5. New user-facing features should update the README feature table and `CHANGELOG.md` under *Unreleased*.

## Reporting bugs

Open a [bug report](https://github.com/miskatul-anwar/geo-viz/issues/new?template=bug_report.yml) with steps to reproduce, expected vs actual behavior, OS + version, and the dataset description (never attach private data).

## Feature proposals

Check the [roadmap](README.md#-roadmap) and open discussions first; large features need an accepted issue before implementation to avoid wasted work.
