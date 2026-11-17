//! Scour backend micro-benchmarks. Run with:
//!   cargo run --release --manifest-path src-tauri/Cargo.toml --example bench
//!
//! Measures the same code paths the Tauri commands use: engine process spawn,
//! JSON envelope parsing, path validation, tree sizing, and real Recycle Bin
//! deletions of synthetic fixtures (restored afterwards by the runner).
//! Emits one JSON object per measurement on stdout.

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

#[path = "../src/cleanup.rs"]
mod cleanup;
#[path = "../src/engine.rs"]
mod engine;

fn emit(suite: &str, label: &str, samples: &mut Vec<f64>, extra: Value) {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = samples.len();
    let mut obj = json!({
        "suite": suite,
        "label": label,
        "iterations": n,
        "ms_min": round(samples[0]),
        "ms_median": round(samples[n / 2]),
        "ms_max": round(samples[n - 1]),
    });
    if let (Some(map), Some(extra_map)) = (obj.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_map {
            map.insert(k.clone(), v.clone());
        }
    }
    println!("{}", obj);
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn time_ms<F: FnMut()>(mut f: F, iterations: usize) -> Vec<f64> {
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    samples
}

fn make_files(root: &PathBuf, count: usize, size: usize) -> u64 {
    fs::create_dir_all(root).unwrap();
    let buf = vec![0x61u8; size];
    for i in 0..count {
        fs::write(root.join(format!("file-{i:05}.bin")), &buf).unwrap();
    }
    (count * size) as u64
}

fn main() {
    let quick = std::env::args().any(|a| a == "--quick");
    let iters = if quick { 20 } else { 100 };
    let bench_root = std::env::temp_dir().join(format!("scour-rust-bench-{}", std::process::id()));
    fs::create_dir_all(&bench_root).unwrap();

    // --- engine process overhead ---------------------------------------------
    let eng = engine::Engine::resolve().expect("engine must resolve for bench");
    let mut s = time_ms(|| { let _ = eng.version(); }, if quick { 3 } else { 5 });
    emit("rust", "engine version (process spawn + JSON)", &mut s, json!({}));

    // --- JSON envelope parsing (the hot path after every engine call) ---------
    let preview = br#"{"ok":true,"burrow_cli":"0.1.0","engine":"burrow-engine","command":"clean","data":{"dry_run":true,"total_bytes":100,"total_human":"100B","items":[{"path":"C:\\tmp\\junk","label":"Temp","size":100,"size_human":"100B"}],"text":"dry"}}"#;
    let big_items: Vec<Value> = (0..10_000)
        .map(|i| json!({"path": format!("C:\\tmp\\junk-{i}"), "label": "Temp", "size": 1024 * (i % 97), "size_human": "1KB"}))
        .collect();
    let big = json!({"ok": true, "data": {"dry_run": true, "total_bytes": 1, "total_human": "1B", "items": big_items, "text": ""}}).to_string();

    let out_small = std::process::Output {
        status: exit_ok(),
        stdout: preview.to_vec(),
        stderr: Vec::new(),
    };
    let out_big = std::process::Output {
        status: exit_ok(),
        stdout: big.as_bytes().to_vec(),
        stderr: Vec::new(),
    };
    let mut s = time_ms(|| {
        let _: engine::CleanData = engine::decode_envelope(&out_small).unwrap();
    }, iters * 10);
    emit("rust", "parse clean envelope (1 item)", &mut s, json!({"bytes": preview.len()}));
    let mut s = time_ms(|| {
        let _: Box<serde_json::value::RawValue> = engine::decode_raw_envelope(&out_big).unwrap();
    }, if quick { 10 } else { 30 });
    emit("rust", "parse clean envelope raw (10k items)", &mut s, json!({"bytes": big.len()}));
    let mut s = time_ms(|| {
        let _: engine::CleanData = engine::decode_envelope(&out_big).unwrap();
    }, if quick { 10 } else { 30 });
    emit("rust", "parse clean envelope (10k items)", &mut s, json!({"bytes": big.len()}));

    // --- path validation (runs before every delete batch) ---------------------
    let valid_dir = bench_root.join("validate-me");
    make_files(&valid_dir, 5, 1024);
    let valid_paths: Vec<String> = (0..5)
        .map(|i| valid_dir.join(format!("file-{i:05}.bin")).to_string_lossy().into_owned())
        .collect();
    let mut s = time_ms(|| { let _ = cleanup::validate(&valid_paths); }, iters);
    emit("rust", "validate 5 safe paths", &mut s, json!({}));

    let many_root = bench_root.join("validate-many");
    make_files(&many_root, 1000, 256);
    let many: Vec<String> = (0..1000)
        .map(|i| many_root.join(format!("file-{i:05}.bin")).to_string_lossy().into_owned())
        .collect();
    let mut s = time_ms(|| { let _ = cleanup::validate(&many); }, if quick { 5 } else { 20 });
    emit("rust", "validate 1000 safe paths", &mut s, json!({}));

    // --- tree sizing (runs per deleted path to report freed bytes) ------------
    let size_root = bench_root.join("tree-size");
    let bytes = make_files(&size_root, if quick { 500 } else { 2000 }, 4096);
    // tree_size is private; delete_reviewed_with runs it for every path before
    // calling the deleter, so a failing deleter measures sizing + event build
    // without removing anything.
    let tree_paths = vec![size_root.to_string_lossy().into_owned()];
    let files = if quick { 500 } else { 2000 };
    let mut s = time_ms(|| {
        let _ = cleanup::delete_reviewed_with(&tree_paths, |_| Err("bench no-op".into()));
    }, if quick { 5 } else { 20 });
    emit("rust", &format!("tree_size walk + event build ({files} files, 8MiB)"), &mut s, json!({"files": files, "bytes": bytes}));

    // --- real Recycle Bin deletes (synthetic files only) -----------------------
    // Small batch: per-call overhead of trash::delete.
    let small_root = bench_root.join("recycle-small");
    make_files(&small_root, 20, 1024);
    let small_paths: Vec<String> = (0..20)
        .map(|i| small_root.join(format!("file-{i:05}.bin")).to_string_lossy().into_owned())
        .collect();
    let t = Instant::now();
    let events = cleanup::delete_reviewed(&small_paths).expect("recycle small batch");
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let (removed, freed_bytes) = match events.last().unwrap() {
        cleanup::CleanEvent::Done { count, freed_bytes, .. } => (*count, *freed_bytes),
        _ => (0, 0),
    };
    println!("{}", json!({
        "suite": "rust", "label": "recycle bin delete 20 files (1KiB each)",
        "iterations": 1, "ms_min": round(ms), "ms_median": round(ms), "ms_max": round(ms),
        "removed": removed, "freed_bytes": freed_bytes,
        "recycle_paths": small_paths,
    }));

    // Larger batch: throughput on a realistic junk folder.
    let big_root = bench_root.join("recycle-large");
    let n = if quick { 200 } else { 1000 };
    make_files(&big_root, n, 8192);
    let big_paths: Vec<String> = (0..n)
        .map(|i| big_root.join(format!("file-{i:05}.bin")).to_string_lossy().into_owned())
        .collect();
    let t = Instant::now();
    let events = cleanup::delete_reviewed(&big_paths).expect("recycle large batch");
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let (removed, freed_bytes) = match events.last().unwrap() {
        cleanup::CleanEvent::Done { count, freed_bytes, .. } => (*count, *freed_bytes),
        _ => (0, 0),
    };
    println!("{}", json!({
        "suite": "rust", "label": format!("recycle bin delete {n} files (8KiB each)"),
        "iterations": 1, "ms_min": round(ms), "ms_median": round(ms), "ms_max": round(ms),
        "removed": removed, "freed_bytes": freed_bytes,
        "ms_per_file": round(ms / n as f64),
        "recycle_paths": big_paths,
    }));

    // Validation refusal must stay fast and must not delete.
    let t = Instant::now();
    let refused = cleanup::delete_reviewed(&["C:\\Windows\\Temp".into()]);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("{}", json!({
        "suite": "rust", "label": "refuse protected path (no deletion)",
        "iterations": 1, "ms_min": round(ms), "ms_median": round(ms), "ms_max": round(ms),
        "refused": refused.is_err(),
    }));

    // Leave the recycle bin contents for the runner to restore; remove the rest.
    for entry in fs::read_dir(&bench_root).unwrap().flatten() {
        let p = entry.path();
        if p.file_name().map(|n| n == "recycle-small" || n == "recycle-large").unwrap_or(false) {
            continue; // already emptied by the deletes; keep dir for restore mapping
        }
        let _ = fs::remove_dir_all(&p);
    }
    eprintln!("bench_root={}", bench_root.display());
}

fn exit_ok() -> std::process::ExitStatus {
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }
}