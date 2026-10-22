param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$harness = Join-Path $root '.build/quality-tests'
New-Item -ItemType Directory -Path (Join-Path $harness 'src') -Force | Out-Null
@'
[package]
name = "ov-quality-module-tests"
version = "0.0.0"
edition = "2021"
[workspace]
[dependencies]
ov-core = { path = "../../crates/ov-core" }
claxon = "0.4"
hound = "3.5"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
'@ | Set-Content -LiteralPath (Join-Path $harness 'Cargo.toml')
$modules = @('corpus', 'wer', 'latency_report', 'stats') | ForEach-Object {
    $source = (Join-Path $root "crates/ov-cli/src/$_.rs").Replace('\', '/')
    "#[path = `"$source`"] pub mod $_;"
}
$modules | Set-Content -LiteralPath (Join-Path $harness 'src/lib.rs')
Write-Host 'Testing actual portable quality modules; this does not load native ASR or measure real speech.'
$cargoArgs = @('test', '--manifest-path', (Join-Path $harness 'Cargo.toml'))
if ($Offline) { $cargoArgs += '--offline' }
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
