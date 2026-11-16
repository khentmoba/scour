//! In-app updates, built on Tauri's updater plugin.
//!
//! The frontend calls two small commands instead of talking to the plugin's JS
//! API directly: Scour ships without a bundler, so keeping the updater on the
//! Rust side avoids vendoring another npm package into `dist/`.
//!
//! On Windows the installer is launched by the plugin after a verified download
//! and the app exits. The NSIS installer is told to relaunch Scour when it
//! finishes, so the user comes back in the new version without doing anything.

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// Event emitted while an update is downloading, listened to by `dist/main.js`.
pub const UPDATE_PROGRESS_EVENT: &str = "scour://update-progress";

/// The parts of a remote release that are worth showing in the UI.
#[derive(Debug)]
struct ReleaseInfo {
    version: String,
    notes: Option<String>,
    date: Option<String>,
}

/// Shape of the result of `check_update`. Kept separate from the command so the
/// "available" and "up to date" shapes can be tested without a network request.
fn status_payload(current_version: &str, remote: Option<ReleaseInfo>) -> Value {
    match remote {
        Some(release) => json!({
            "available": true,
            "current_version": current_version,
            "version": release.version,
            "notes": release.notes,
            "date": release.date,
        }),
        None => json!({
            "available": false,
            "current_version": current_version,
            "version": current_version,
        }),
    }
}

/// Progress event body. `downloaded` is cumulative; the updater reports the size
/// of each chunk, so the command adds them up before calling this.
fn progress_payload(downloaded: u64, total: Option<u64>) -> Value {
    let percent = total.filter(|&t| t > 0).map(|t| {
        let value = downloaded as f64 / t as f64 * 100.0;
        value.clamp(0.0, 100.0)
    });
    json!({
        "phase": "download",
        "downloaded": downloaded,
        "total": total,
        "percent": percent,
    })
}

#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Ask the configured endpoint what the newest release is. No download happens
/// here, and nothing is checked unless the user clicks the button.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Value, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let current = env!("CARGO_PKG_VERSION");

    let found = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .map(|update| ReleaseInfo {
            version: update.version,
            notes: update.body,
            date: update.date.map(|d| d.to_string()),
        });

    Ok(status_payload(current, found))
}

/// Download, verify and install the update in one step. The plugin verifies the
/// signature against the public key in `tauri.conf.json` before installing.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<Value, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "already running the latest version".to_string())?
        .restart_after_install(true);

    let version = update.version.clone();
    let emitter = app.clone();
    let mut downloaded = 0u64;

    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = emitter.emit(UPDATE_PROGRESS_EVENT, progress_payload(downloaded, total));
            },
            || {
                let _ = emitter.emit(
                    UPDATE_PROGRESS_EVENT,
                    json!({ "phase": "downloaded", "percent": 100.0 }),
                );
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    // Windows exits the process before this point. Other platforms get a clear
    // result so the UI can ask the user to relaunch.
    Ok(json!({ "installed": true, "version": version, "restart_required": cfg!(not(windows)) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_when_no_new_version_exists() {
        let payload = status_payload("0.1.0", None);
        assert_eq!(payload["available"], false);
        assert_eq!(payload["current_version"], "0.1.0");
        assert_eq!(payload["version"], "0.1.0");
    }

    #[test]
    fn reports_the_remote_version_and_notes_when_available() {
        let payload = status_payload(
            "0.1.0",
            Some(ReleaseInfo {
                version: "0.2.0".into(),
                notes: Some("faster scans".into()),
                date: None,
            }),
        );
        assert_eq!(payload["available"], true);
        assert_eq!(payload["current_version"], "0.1.0");
        assert_eq!(payload["version"], "0.2.0");
        assert_eq!(payload["notes"], "faster scans");
    }

    #[test]
    fn progress_accumulates_chunks_and_computes_a_percentage() {
        let first = progress_payload(25, Some(100));
        assert_eq!(first["downloaded"], 25);
        assert_eq!(first["percent"], 25.0);

        let last = progress_payload(100, Some(100));
        assert_eq!(last["percent"], 100.0);
    }

    #[test]
    fn progress_without_a_total_has_no_percentage() {
        let payload = progress_payload(42, None);
        assert_eq!(payload["downloaded"], 42);
        assert!(payload["percent"].is_null());
    }
}
