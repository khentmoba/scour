mod cleanup;
mod commands;
mod engine;
mod updater;

use commands::EngineState;
use engine::Engine;
use tauri::Manager;

fn main() {
    let engine = Engine::resolve();
    if let Err(e) = &engine {
        eprintln!("Scour: {e}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(EngineState(engine))
        .invoke_handler(tauri::generate_handler![
            commands::engine_version,
            commands::system_status,
            commands::scan_clean,
            commands::analyze_dir,
            commands::preview_optimize,
            commands::apply_clean,
            updater::app_version,
            updater::check_update,
            updater::install_update,
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("Scour — disk hygiene");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Scour");
}
