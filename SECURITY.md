# Security Policy

## Supported versions

| Version | Supported |
|---|---|
| latest release | ✅ |
| older releases | ❌ (upgrade) |

## Reporting a vulnerability

**Do not open a public issue for security problems.**

Use GitHub's private vulnerability reporting: *Security → Report a vulnerability* on this repository, or contact [@miskatul-anwar](https://github.com/miskatul-anwar) directly.

Include: affected version/commit, reproduction steps or PoC, and impact assessment. You will receive an acknowledgement within **72 hours**, and a fix timeline within **7 days** for confirmed issues. Credit is given in the release notes unless you prefer otherwise.

## Scope notes

GeoViz is a desktop application that stores project data in a local SQLite database and renders web content in a sandboxed Tauri webview with a strict Content-Security-Policy. Areas of particular interest to reviewers:

- The Tauri IPC surface (`src-tauri/src/commands.rs`) — input validation on every command
- SQL console guardrails (`db.rs::execute_sql_query`) — read-only enforcement
- GeoPackage/KML/GPX parsers — fuzzed inputs, memory safety
- `map_bridge.js` — HTML escaping of feature properties in popups

Third-party assets are vendored (Leaflet) and pinned; no runtime CDN dependencies.
