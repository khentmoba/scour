mod cleanup;
mod commands;
mod engine;
mod finder;
mod scanner;
mod updater;

use commands::EngineState;
use engine::Engine;
use finder::FinderState;
use std::sync::OnceLock;
use std::time::Instant;
use tauri::Manager;

/// Startup timing for the benchmark suite (`scripts/bench/`). Enabled only when
/// `SCOUR_BENCH=1`; the process prints one JSON line per phase and exits after
/// the first frame is drawn, so no window stays open during a benchmark run.
pub fn bench_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SCOUR_BENCH").is_some_and(|v| v == "1"))
}

fn bench_mark(start: &Instant, phase: &str) {
    if bench_enabled() {
        println!(
            "{{\"phase\":\"{phase}\",\"elapsed_ms\":{:.2}}}",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}

fn main() {
    let start = Instant::now();
    // Anchor for the frontend's first-frame measurement (SCOUR_BENCH only).
    let epoch_ms = if bench_enabled() {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    } else {
        0.0
    };
    let engine = Engine::resolve().map(std::sync::Arc::new);
    if let Err(e) = &engine {
        eprintln!("Scour: {e}");
    }
    bench_mark(&start, "engine_resolved");
    commands::warm_version(engine.clone());

    tauri::Builder::default()
        .manage(EngineState(engine))
        .manage(FinderState::default())
        .invoke_handler(tauri::generate_handler![
            commands::engine_version,
            commands::system_status,
            commands::scan_clean,
            commands::analyze_dir,
            commands::preview_optimize,
            commands::apply_clean,
            commands::find_index,
            commands::find_search,
            commands::scan_all,
            commands::app_inventory,
            commands::bench_first_frame,
            updater::app_version,
            updater::check_update,
            updater::install_update,
        ])
        .setup(move |app| {
            bench_mark(&start, "setup");
            if bench_enabled() {
                if let Some(window) = app.get_webview_window("main") {
                    // After the page's first painted frame, report the elapsed
                    // time since process start and exit. No window stays open.
                    let js = format!(
                        "(function(){{const epoch={epoch_ms};const go=()=>requestAnimationFrame(()=>requestAnimationFrame(()=>{{window.__TAURI_INTERNALS__.invoke('bench_first_frame',{{startMs:Date.now()-epoch}}).catch(()=>{{}});}}));if(document.readyState==='complete')go();else window.addEventListener('load',go);}})();"
                    );
                    let _ = window.eval(&js);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Scour");
    bench_mark(&start, "run_returned");
}
