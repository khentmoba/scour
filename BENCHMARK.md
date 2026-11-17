# Scour Performance Benchmark Baseline

Reproducible performance baseline for **Scour** (`scour.exe` + `burrow-engine.exe` + `dist/` frontend).
Use this file to detect regressions and track speedups across releases.

> **How to run the suite:**
> ```powershell
> powershell -ExecutionPolicy Bypass -File scripts/bench/run-bench.ps1
> # Quick smoke run (~15s):
> powershell -ExecutionPolicy Bypass -File scripts/bench/run-bench.ps1 -Quick
> # Include full user-profile + whole-C:\ drive analysis (~11 min):
> node scripts/bench/engine-bench.mjs --whole-pc
> ```
> All runs are headless, restore/clean synthetic Recycle Bin fixtures, and write raw JSON to `scripts/bench/results/<timestamp>/`.

---

## Baseline Environment (2026-11-17, Round 10 — Final)

| Spec | Value |
|---|---|
| **CPU** | AMD Ryzen 7 5700X 8-Core Processor (8C / 16T) |
| **RAM** | 31.9 GB |
| **OS** | Microsoft Windows 11 Pro 10.0.26300 |
| **Disk** | Samsung SSD 980 500GB (SSD, NVMe) |
| **Rust** | `rustc 1.93.0 (254b59607 2026-01-19)` |
| **Node** | `v24.22.2` |
| **Artifacts** | `scour.exe` = 3,839,488 bytes (~3.66 MiB) · `burrow-engine.exe` = 2,124,288 bytes (~2.0 MiB) |
| **Frontend Assets** | `index.html` = 8,143 B · `main.js` = 25,522 B · `styles.css` = 7,504 B (41,169 B total) |

---

## 1. Build & App Startup (`scour.exe` with `SCOUR_BENCH=1`)

| Metric | Min | Median | Max | Notes |
|---|---:|---:|---:|---|
| Release build (incremental no-op) | — | **80.16 s** | — | Parallel ThinLTO + `opt-level = "z"` |
| Release build (app-crate rebuild) | — | **16.44 s** | — | Touch `src-tauri/src/main.rs` |
| Startup: `engine_resolved` | 0.07 ms | **0.07 ms** | 0.10 ms | n=5 |
| Startup: `setup` (WebView2 init) | 289.32 ms | **295.89 ms** | 562.87 ms | n=5 |
| Startup: `first_frame` | 379.53 ms | **397.17 ms** | 641.98 ms | n=5 |
| Startup: process wall time | 456.2 ms | **465.5 ms** | 829.4 ms | n=5 |

---

## 2. Engine CLI (`burrow-engine.exe` sidecar)

| Command / Target | Iterations | Min | Median | Max | Details |
|---|---:|---:|---:|---:|---|
| `version` | 5 | 18.45 ms | **19.03 ms** | 22.83 ms | 147 B JSON |
| `status` | 3 | — | **62.53 ms** | — | 558 B JSON (`ok=false`, exit 1) |
| `clean --dry-run` (whole-PC junk scan) | 3 | — | **150.60 ms** | — | 1 item, 31.1 MiB |
| `optimize --dry-run` | 3 | — | **16.41 ms** | — | 3 tasks |
| `analyze` synthetic small (100 files, 4 dirs) | 3 | — | **21.67 ms** | — | 407 KB |
| `analyze` synthetic medium (2,000 files, 20 dirs) | 3 | — | **74.10 ms** | — | 15.5 MiB |
| `analyze` synthetic large (10,000 files, 50 dirs) | 3 | — | **286.60 ms** | — | 155.0 MiB |
| `analyze` synthetic deep (40 levels, 400 files) | 3 | — | **34.11 ms** | — | 1.66 MiB |
| `analyze ~/Downloads` (real) | 1 | — | **35.80 ms** | — | 35 entries |
| `analyze %USERPROFILE%` (real, whole profile) | 1 | — | **358,975.31 ms** | — | ~5m 59s (baseline) |
| `analyze C:\` (real, whole system drive) | 1 | — | **289,145.22 ms** | — | ~4m 49s, 7,234 B JSON (baseline) |

---

## 3. Rust Backend (`src-tauri` micro-benchmarks)

| Operation | Iterations | Min | Median | Max | Notes |
|---|---:|---:|---:|---:|---|
| `engine.version()` (spawn + JSON) | 5 | 18.45 ms | **18.59 ms** | 22.83 ms | Includes `CREATE_NO_WINDOW` |
| Parse `clean` envelope (1 item, 232 B) | 1,000 | 0.00 ms | **0.00 ms** | 0.02 ms | Direct `from_slice` |
| Parse `clean` envelope raw (10k items, 767 KB) | 30 | 0.77 ms | **0.77 ms** | 0.89 ms | Zero-alloc `RawValue` passthrough |
| Parse `clean` envelope typed (10k items, 767 KB) | 30 | 2.71 ms | **2.79 ms** | 3.04 ms | Direct `from_slice` |
| `cleanup::validate` (5 safe paths) | 100 | 0.29 ms | **0.30 ms** | 0.44 ms | `last_safe_parent` + lexical-first |
| `cleanup::validate` (1,000 safe paths) | 20 | 22.47 ms | **22.87 ms** | 24.05 ms | `last_safe_parent` + lexical-first |
| `tree_size` walk + event build (2k files, 8 MiB) | 20 | 1.05 ms | **1.06 ms** | 1.16 ms | Typed `CleanEvent` enum |
| Recycle Bin delete (20 files × 1 KiB) | 1 | — | **182.82 ms** | — | Borrowed `&str` + `trash::delete_all` |
| Recycle Bin delete (1,000 files × 8 KiB) | 1 | — | **4,558.98 ms** | — | **4.56 ms/file** |
| Refuse protected path (`C:\Windows\Temp`) | 1 | — | **0.00 ms** | — | Lexical-first zero-syscall refusal |

---

## 4. Frontend UI (Headless Edge via CDP, `dist/`)

| Metric | Iterations | Min | Median | Max | Details |
|---|---:|---:|---:|---:|---|
| Boot: `DOMContentLoaded` | 4 | — | **9.8 ms** | — | — |
| Boot: `load` event | 4 | — | **11.7 ms** | — | — |
| Boot: `first-paint` | 4 | — | **28.0 ms** | — | — |
| `renderClean` 100 rows (click → painted) | 3 | — | **31.9 ms** | — | 233 DOM nodes |
| `select-all` toggle ×2 on 100 rows | 3 | — | **0.0 ms** | — | Cached DOM refs |
| `renderClean` 1,000 rows (click → painted) | 3 | — | **30.9 ms** | — | 233 DOM nodes |
| `select-all` toggle ×2 on 1,000 rows | 3 | — | **0.0 ms** | — | `Uint8Array` mask |
| `renderClean` 10,000 rows (click → painted) | 3 | — | **29.4 ms** | — | **233 DOM nodes** |
| `select-all` toggle ×2 on 10,000 rows | 3 | — | **0.0 ms** | — | `Uint8Array` mask |
| `analyze` render 2,000 entries (click → painted) | 3 | — | **31.7 ms** | — | Pooled virtualizer |
| Idle page metrics (post-bench) | 1 | — | — | — | **311 DOM nodes** · 7.50 MB heap |

---

## Optimization Log

| Round | Change | Key Impact |
|---|---|---|
| Baseline | Initial measurement (`20261117-174546` + whole-drive baseline) | Established full-stack baseline |
| Round 1 | `trash::delete_all` batching; ancestor cache + lexical norm in `cleanup::validate`; single-pass `serde_json::from_slice` + `CREATE_NO_WINDOW` + `spawn_blocking`; `[profile.release]` LTO/strip/abort; `<template>` + `DocumentFragment` + `content-visibility: auto` + delegated listener + parallel `init()`; `BufWriter` + throttled update emits | Recycle delete 1k: **16.86s → 5.26s** (3.2x); `validate` 1k: **337ms → 30.4ms** (11.1x); JSON 10k: **17.6ms → 3.5ms** (5.1x); `renderClean` 10k: **537ms → 173ms** (3.1x); `scour.exe`: **12.0 MiB → 7.0 MiB** (-41.6%); startup `first_frame`: **641ms → 497ms** |
| Round 2 | Virtualized `#clean-list` & `#analyze-list`; `Box<RawValue>` zero-copy IPC in `scan_clean`/`analyze_dir`; reused `meta.len()` + iterative `dir_tree_size` + fast-path `normalize_lexical` in `cleanup.rs`; O(1) `select-all`, `Set` retry filter, `scaleX` bars, deferred `listen`, `DocumentFragment` in status/optimize; `from_slice` in `updater.rs`; gated `SystemTime::now()` | `renderClean` 10k: **173ms → 29.7ms** (5.8x, **18.1x** vs baseline); 10k DOM nodes: **80,143 → 233** (-99.7%); `select-all` 10k: **10.5ms → 0.3ms** (35x); JSON raw 10k: **0.87ms**; `validate` 1k: **23.8ms**; Recycle 1k: **4.71s**; startup `first_frame`: **383ms** |
| Round 3 | Typed `CleanEvent` enum; pre-warmed `engine_version` on background thread; `OnceLock` home/exe/bench flags + `Path::new` zero-alloc in `cleanup.rs`; pooled row-recycling virtualizer + `Uint8Array` selection + cached `viewH`; capped `#result-log`; `contain: layout style paint`; smart `build.rs` copy; fixed `refresh-status` | `select-all` 1k/10k: **0.0 ms**; `validate` 5 paths: **0.29 ms**; `tree_size`: **0.89 ms**; `DOMContentLoaded`: **9.8 ms**; `load`: **11.7 ms**; `engine_version` pre-warmed during WebView2 init |
| Round 4 | Parallel ThinLTO (removed `codegen-units = 1`); binary bisection fallback on `trash::delete_all`; `to_str()` + length guards in `cleanup.rs`; `OnceLock::get_or_init` dedup for `engine_version`; removed dead `parse_json`; `teardownVirtualList`, `ResizeObserver`, `_idx` skip, pool iteration, pre-formatted `_sizeStr`, active nav/view tracking in `main.js` | App-crate rebuild: **78.9s → 26.2s** (3.0x faster); JSON raw 10k: **0.77 ms**; refuse path: **0.12 ms**; `renderClean` 10k: **30.8 ms**; O(1) nav/select/scroll |
| Round 5 | `reqwest` `native-tls` (SChannel) + `[profile.release.package."*"] opt-level = "s"`; already-deleted filter in `delete_bisect`; `Engine::resolve` fast-path; cached `check_update` target + typed `UpdateProgress`; `<script defer>`, cached `viewMap`, `box._idx`, skipped `removedSet` on success, `#optimize-list` template | `scour.exe`: **7.0 MiB → 5.09 MiB** (-27.5%, **-57.7%** vs baseline); `renderClean` 10k: **29.4 ms**; Recycle 20: **184.8 ms** |
| Round 6 | Borrowed `(&str, &Path)` in `cleanup.rs` (zero `PathBuf`→`String` round-trips) + first-byte `BLOCKED_PARTS` pre-filter; slot-based O(delta) virtual row recycling + `contain: strict` + cached selection DOM refs; tightened `update-progress` throttle | Recycle 1k: **4.75s → 4.63s** (**3.64x** vs baseline); Recycle 20: **182.1 ms**; `select-all` 100/1k/10k: **0.0 ms**; no-op build: **25.7s** |
| Round 7 | Lexical-first protected-path check + `last_safe_parent` cache + direct bisection in `cleanup.rs`; removed duplicate `Engine::resolve` check; `connect_timeout` + cached `elapsed` in `updater.rs`; zero-alloc `activeSlots`/`free` in virtualizer; lazy `_sizeStr` + hoisted `LOG_1024`; `<template>` in `refresh-status` | Refuse protected path: **0.12 ms → 0.00 ms**; `validate` 1k: **24.0 ms → 23.22 ms**; Recycle 20: **180.81 ms**; `renderClean` 1k: **30.9 ms** |
| Round 8 | Scoped absolute positioning to `.list-viewport > .row` (fixing `#optimize-list` stacking + GPU layer bloat); `visibility: hidden` for pooled rows; `safe_ancestors.contains` pre-check + `is_blocked_part` in `forbidden()`; single `is_newer` parse + typed `DownloadedProgress`/`InstallResult` in `updater.rs` | `engine.version()`: **17.91 ms**; JSON raw 10k: **0.77 ms**; `tree_size`: **0.89 ms**; `select-all` 1k: **0.0 ms**; fixed `#optimize-list` layout |
| Round 9 | `opt-level = "s"` on root `scour` crate; direct single-path `trash::delete` in `delete_bisect`; removed `uncached` `Vec` in `has_link_in_ancestors`; deduplicated `Engine::resolve` fallback; lazy `app_version` IPC on Updates tab | `scour.exe`: **5.09 MiB → 4.04 MiB** (-20.6%, **-66.4%** vs baseline); Recycle 1k: **4,575.86 ms** (**4.58 ms/file**); `validate` 1k: **22.95 ms**; rebuild: **18.50 s** |
| Round 10 | `normalize_lexical` in `cached_home()` (fixes `\\?\` prefix mismatch + removes syscall); `<= 8` threshold in `delete_bisect`; `bytes == 0` fast-path in `humansize`; `opt-level = "z"` in `[profile.release]`; `targets = ["nsis"]`; eliminated forced reflow & detached-row `appendChild` in `mountVirtualList` | `scour.exe`: **4.04 MiB → 3.66 MiB** (**-69.6%** vs initial `12.0 MiB`); Recycle 1k: **4,558.98 ms** (**4.56 ms/file**, **3.70x** vs initial); `validate` 1k: **22.87 ms** (**14.7x** vs initial); rebuild: **16.44 s** |
