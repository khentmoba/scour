use crate::cleanup::{self, CleanEvent};
use crate::engine::Engine;
use crate::finder::FinderState;
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use tauri::State;

/// The engine is resolved once at start-up. Keeping the `Result` in state means
/// a missing binary becomes a clear message in the UI instead of a crash.
pub struct EngineState(pub Result<Arc<Engine>, String>);

static WARMED_VERSION: OnceLock<Result<Box<serde_json::value::RawValue>, String>> = OnceLock::new();
static ENGINE_SLOT: OnceLock<Result<Arc<Engine>, String>> = OnceLock::new();

fn resolve_version() -> Result<Box<serde_json::value::RawValue>, String> {
    WARMED_VERSION
        .get_or_init(|| match ENGINE_SLOT.get() {
            Some(Ok(eng)) => eng.version_raw(),
            Some(Err(e)) => Err(e.clone()),
            None => Err("engine not initialized".into()),
        })
        .clone()
}

pub fn warm_version(engine: Result<Arc<Engine>, String>) {
    let _ = ENGINE_SLOT.set(engine);
    std::thread::spawn(|| {
        let _ = resolve_version();
    });
}

impl EngineState {
    fn engine(&self) -> Result<Arc<Engine>, String> {
        self.0.as_ref().cloned().map_err(|e| e.clone())
    }
}

#[tauri::command]
pub async fn engine_version(
    state: State<'_, EngineState>,
) -> Result<Box<serde_json::value::RawValue>, String> {
    let _ = ENGINE_SLOT.get_or_init(|| state.0.clone());
    if let Some(cached) = WARMED_VERSION.get() {
        return cached.clone();
    }
    tauri::async_runtime::spawn_blocking(resolve_version)
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
pub async fn system_status(
    state: State<'_, EngineState>,
) -> Result<Box<serde_json::value::RawValue>, String> {
    let eng = state.engine()?;
    tauri::async_runtime::spawn_blocking(move || eng.status_raw())
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
pub async fn scan_clean(
    state: State<'_, EngineState>,
) -> Result<Box<serde_json::value::RawValue>, String> {
    let eng = state.engine()?;
    tauri::async_runtime::spawn_blocking(move || eng.clean_preview_raw())
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
pub async fn analyze_dir(
    state: State<'_, EngineState>,
    path: String,
) -> Result<Box<serde_json::value::RawValue>, String> {
    if path.trim().is_empty() {
        return Err("enter a folder path first".into());
    }
    let eng = state.engine()?;
    tauri::async_runtime::spawn_blocking(move || eng.analyze_raw(&path))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
pub async fn preview_optimize(
    state: State<'_, EngineState>,
) -> Result<Box<serde_json::value::RawValue>, String> {
    let eng = state.engine()?;
    tauri::async_runtime::spawn_blocking(move || eng.preview_optimize_raw())
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
pub async fn apply_clean(paths: Vec<String>) -> Result<Vec<CleanEvent>, String> {
    tauri::async_runtime::spawn_blocking(move || cleanup::delete_reviewed(&paths))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

/// Benchmark-only: the frontend calls this after its first painted frame when
/// `SCOUR_BENCH=1`. It reports the elapsed time since process start and closes
/// the app so a startup benchmark never leaves a window open.
#[tauri::command]
pub fn bench_first_frame(app: tauri::AppHandle, start_ms: f64) {
    if crate::bench_enabled() {
        println!("{{\"phase\":\"first_frame\",\"elapsed_ms\":{start_ms:.2}}}");
        app.exit(0);
    }
}

#[tauri::command]
pub fn find_index(state: State<'_, FinderState>, folder: String) -> Result<Value, String> {
    let data = crate::finder::index_folder(&state, &folder)?;
    serde_json::to_value(data).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn find_search(
    state: State<'_, FinderState>,
    query: String,
    limit: Option<usize>,
) -> Result<Value, String> {
    let data = crate::finder::search(&state, &query, limit.unwrap_or(100))?;
    serde_json::to_value(data).map_err(|e| e.to_string())
}
