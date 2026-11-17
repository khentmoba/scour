# Scour full benchmark runner. Headless, no visible windows, no focus stealing.
#
#   powershell -ExecutionPolicy Bypass -File scripts/bench/run-bench.ps1 [-Quick]
#
# Runs: cargo tests, release build timing, app startup (SCOUR_BENCH=1),
# engine CLI timings, Rust backend micro-benchmarks, headless frontend timings.
# Synthetic delete fixtures are restored from the Recycle Bin afterwards.
# Results: scripts/bench/results/<timestamp>/ (JSON + raw logs).

param([switch]$Quick)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $repo
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$outDir = Join-Path $PSScriptRoot "results\$stamp"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$quickFlag = @()
if ($Quick) { $quickFlag = @('--quick') }

function Invoke-Native {
  param([scriptblock]$Cmd)
  $prev = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    & $Cmd
  } finally {
    $ErrorActionPreference = $prev
  }
}

function MachineInfo {
  $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
  $os = Get-CimInstance Win32_OperatingSystem
  $disk = Get-PhysicalDisk | Where-Object { $_.BusType -eq 'NVMe' -or $_.MediaType -eq 'SSD' } | Select-Object -First 1
  $drive = Get-PSDrive ($env:SystemDrive.TrimEnd(':'))
  [ordered]@{
    timestamp    = (Get-Date).ToString('o')
    cpu          = $cpu.Name.Trim()
    cores        = $cpu.NumberOfCores
    threads      = $cpu.NumberOfLogicalProcessors
    ram_gb       = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
    os           = "$($os.Caption) $($os.Version)"
    disk         = if ($disk) { "$($disk.FriendlyName) ($($disk.MediaType), $($disk.BusType))" } else { 'unknown' }
    system_drive_free_gb = [math]::Round($drive.Free / 1GB, 1)
    rustc        = (rustc --version)
    node         = (node --version)
    scour_exe_bytes = (Get-Item src-tauri\target\release\scour.exe -ErrorAction SilentlyContinue).Length
    engine_exe_bytes = (Get-Item src-tauri\binaries\burrow-engine.exe).Length
  }
}

$info = MachineInfo
$info | ConvertTo-Json | Set-Content (Join-Path $outDir 'machine.json')
Write-Host "[bench] machine: $($info.cpu), $($info.ram_gb) GB RAM, $($info.disk)"

# --- 1. correctness gate: unit tests must pass before we trust timings --------
Write-Host '[bench] cargo test (release)...'
$testLog = Join-Path $outDir 'cargo-test.log'
Invoke-Native { cargo test --release --manifest-path src-tauri/Cargo.toml *>&1 | Tee-Object -FilePath $testLog | Out-Null }
if ($LASTEXITCODE -ne 0) { throw "cargo test failed; see $testLog" }
Write-Host '[bench] tests passed'

# --- 2. build timing ------------------------------------------------------------
Write-Host '[bench] release build (incremental no-op)...'
$t = [Diagnostics.Stopwatch]::StartNew()
Invoke-Native { cargo build --release --manifest-path src-tauri/Cargo.toml *>&1 | Out-Null }
$t.Stop()
$buildNoop = [math]::Round($t.Elapsed.TotalSeconds, 2)
Write-Host "[bench] no-op release build: $buildNoop s"

Write-Host '[bench] release build (touch main.rs, recompile app crate)...'
(Get-Item src-tauri\src\main.rs).LastWriteTime = Get-Date
$t = [Diagnostics.Stopwatch]::StartNew()
Invoke-Native { cargo build --release --manifest-path src-tauri/Cargo.toml *>&1 | Out-Null }
$t.Stop()
$buildTouch = [math]::Round($t.Elapsed.TotalSeconds, 2)
Write-Host "[bench] app-crate rebuild: $buildTouch s"
@{ noop_build_s = $buildNoop; app_crate_rebuild_s = $buildTouch } |
  ConvertTo-Json | Set-Content (Join-Path $outDir 'build.json')

# --- 3. app startup (real exe, SCOUR_BENCH=1, exits after first frame) ---------
Write-Host '[bench] app startup runs...'
$startupRuns = @()
$runs = if ($Quick) { 3 } else { 5 }
for ($i = 0; $i -lt $runs; $i++) {
  $env:SCOUR_BENCH = '1'
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $p = Start-Process -FilePath 'src-tauri\target\release\scour.exe' -PassThru -WindowStyle Hidden `
       -RedirectStandardOutput (Join-Path $outDir "startup-$i.log") -RedirectStandardError (Join-Path $outDir "startup-$i.err.log")
  if (-not $p.WaitForExit(30000)) { $p.Kill(); throw "startup run $i did not exit within 30s" }
  $sw.Stop()
  $phases = @{}
  Get-Content (Join-Path $outDir "startup-$i.log") | ForEach-Object {
    if ($_ -match '^\{') {
      $j = $_ | ConvertFrom-Json
      $phases[$j.phase] = $j.elapsed_ms
    }
  }
  $startupRuns += [ordered]@{
    run = $i
    wall_ms = [math]::Round($sw.Elapsed.TotalMilliseconds, 1)
    exit_code = $p.ExitCode
    phases = $phases
  }
  Remove-Item Env:SCOUR_BENCH -ErrorAction SilentlyContinue
}
$startupRuns | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $outDir 'startup.json')
$ff = $startupRuns | ForEach-Object { $_.phases.first_frame } | Where-Object { $_ -ne $null }
if ($ff) {
  $median = ($ff | Sort-Object)[[int]($ff.Count / 2)]
  Write-Host "[bench] startup first-frame median: $median ms"
} else {
  Write-Host '[bench] WARNING: no first_frame phase captured (WebView2 may be unavailable)'
}

# --- 4. engine CLI benchmarks ----------------------------------------------------
Write-Host '[bench] engine CLI benchmarks...'
Invoke-Native { node scripts/bench/engine-bench.mjs @quickFlag --out (Join-Path $outDir 'engine.json') }
if ($LASTEXITCODE -ne 0) { throw 'engine benchmark failed' }

# --- 5. Rust backend micro-benchmarks --------------------------------------------
Write-Host '[bench] Rust backend benchmarks...'
$rustLog = Join-Path $outDir 'rust.jsonl'
Invoke-Native { cargo run --release --manifest-path src-tauri/Cargo.toml --example bench -- @quickFlag 2> (Join-Path $outDir 'rust.err.log') | Set-Content $rustLog }
if ($LASTEXITCODE -ne 0) { throw 'rust benchmark failed' }
$rustResults = Get-Content $rustLog | Where-Object { $_ -match '^\{' } | ForEach-Object { $_ | ConvertFrom-Json }

# --- 6. restore recycled fixtures -------------------------------------------------
Write-Host '[bench] restoring recycled fixtures from Recycle Bin...'
$recycled = @($rustResults | Where-Object { $_.recycle_paths } | ForEach-Object { $_.recycle_paths } | ForEach-Object { $_ })
$restored = 0; $restoreFailed = @()
if ($recycled.Count -gt 0) {
  $shell = New-Object -ComObject Shell.Application
  $bin = $shell.Namespace(0xA)
  $items = @($bin.Items())
  $byName = @{}
  foreach ($item in $items) {
    $orig = $bin.GetDetailsOf($item, 1)   # original location column
    $full = Join-Path $orig $item.Name
    $byName[$full.ToLower()] = $item
  }
  foreach ($p in $recycled) {
    $key = $p.ToLower()
    if ($byName.ContainsKey($key)) {
      $item = $byName[$key]
      $verb = $item.Verbs() | Where-Object { ($_.Name -replace '&','') -eq 'Restore' } | Select-Object -First 1
      if ($verb) {
        $verb.DoIt()
        $restored++
      } else { $restoreFailed += $p }
    } else { $restoreFailed += $p }
  }
  Start-Sleep -Seconds 2
  $stillMissing = @($recycled | Where-Object { -not (Test-Path $_) })
  Write-Host "[bench] recycle restore: $restored invoked, $($stillMissing.Count) still missing"
  if ($stillMissing.Count -gt 0) {
    $stillMissing | Set-Content (Join-Path $outDir 'restore-missing.txt')
  }
}
# sweep any leftover bench fixture roots in temp
Get-ChildItem $env:TEMP -Directory -Filter 'scour-rust-bench-*' | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem $env:TEMP -Directory -Filter 'scour-engine-bench-*' | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# --- 7. frontend benchmarks --------------------------------------------------------
Write-Host '[bench] frontend benchmarks (headless Edge)...'
Invoke-Native { node scripts/bench/frontend-bench.mjs @quickFlag --out (Join-Path $outDir 'frontend.json') }
if ($LASTEXITCODE -ne 0) { throw 'frontend benchmark failed' }

# --- 8. combined summary -------------------------------------------------------------
$engine = Get-Content (Join-Path $outDir 'engine.json') -Raw | ConvertFrom-Json
$frontend = Get-Content (Join-Path $outDir 'frontend.json') -Raw | ConvertFrom-Json
$summary = [ordered]@{
  machine = $info
  quick = [bool]$Quick
  build = @{ noop_build_s = $buildNoop; app_crate_rebuild_s = $buildTouch }
  startup = $startupRuns
  engine = $engine.results
  rust = $rustResults
  frontend = $frontend.results
  restore = @{ invoked = $restored; failed = $restoreFailed.Count }
}
$summary | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $outDir 'summary.json')
Write-Host "[bench] done -> $outDir"