use crate::cleanup;
use crate::engine::Engine;
use serde_json::Value;
use tauri::State;

/// The engine is resolved once at start-up. Keeping the `Result` in state means
/// a missing binary becomes a clear message in the UI instead of a crash.
pub struct EngineState(pub Result<Engine, String>);

impl EngineState {
    fn engine(&self) -> Result<&Engine, String> {
        self.0.as_ref().map_err(|e| e.clone())
    }
}

#[tauri::command]
pub fn engine_version(state: State<'_, EngineState>) -> Result<Value, String> {
    state.engine()?.version()
}

#[tauri::command]
pub fn system_status(state: State<'_, EngineState>) -> Result<Value, String> {
    state.engine()?.status()
}

#[tauri::command]
pub fn scan_clean(state: State<'_, EngineState>) -> Result<Value, String> {
    let data = state.engine()?.clean_preview()?;
    serde_json::to_value(data).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn analyze_dir(state: State<'_, EngineState>, path: String) -> Result<Value, String> {
    if path.trim().is_empty() {
        return Err("enter a folder path first".into());
    }
    let data = state.engine()?.analyze(&path)?;
    serde_json::to_value(data).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_optimize(state: State<'_, EngineState>) -> Result<Value, String> {
    state.engine()?.preview_optimize()
}

#[tauri::command]
pub fn apply_clean(paths: Vec<String>) -> Result<Vec<Value>, String> {
    cleanup::delete_reviewed(&paths)
}
