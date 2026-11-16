# Rebuild the bundled Burrow engine binary.
#
# Usage:
#   .\scripts\build-engine.ps1 -Source C:\path\to\Burrow
#
# The script expects a checkout of https://github.com/caezium/Burrow whose
# burrow-engine crate lives at <Source>\burrow-engine.

param(
  [Parameter(Mandatory = $true)]
  [string]$Source
)

$ErrorActionPreference = 'Stop'
$crate = Join-Path $Source 'burrow-engine'
if (-not (Test-Path (Join-Path $crate 'Cargo.toml'))) {
  throw "No burrow-engine crate found at $crate"
}

Push-Location $crate
try {
  cargo build --release
  $built = Join-Path $crate 'target\release\burrow-engine.exe'
  if (-not (Test-Path $built)) { throw "Build finished but $built is missing" }
  $dest = Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri\binaries'
  Copy-Item $built (Join-Path $dest 'burrow-engine.exe') -Force
  Write-Host "Installed engine into $dest"
} finally {
  Pop-Location
}
