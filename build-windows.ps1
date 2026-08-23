# GeoViz Pro - Windows production installer build script
# Prerequisites: .NET 8 SDK, Rust, Tauri CLI (`cargo install tauri-cli --version "^2.0" --locked`)

Write-Host "========================================" -ForegroundColor Cyan
Write-Host " Building GeoViz Pro for Windows (.msi / .exe)" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# The frontend publish runs automatically via beforeBuildCommand in tauri.conf.json.
Write-Host "`n[1/2] Compiling Native Windows Executable and Installers (WiX MSI & NSIS Setup EXE)..." -ForegroundColor Yellow
cargo tauri build --bundles nsis,msi
if ($LASTEXITCODE -ne 0) {
    Write-Error "Tauri Windows bundle build failed!"
    exit 1
}

Write-Host "`n[2/2] Build Complete! Installers generated at:" -ForegroundColor Green
Get-ChildItem -Path "src-tauri\target\release\bundle\nsis\*.exe", "src-tauri\target\release\bundle\msi\*.msi" | ForEach-Object {
    Write-Host "  -> $($_.FullName) ($([math]::Round($_.Length / 1MB, 2)) MB)" -ForegroundColor White
}
