//! Keyless in-app updates.
//!
//! The app asks GitHub which release is newest, downloads that release's NSIS
//! installer over HTTPS, and runs it with NSIS `/S` (silent) and `/R`
//! (relaunch) flags. Those make the installer replace the running copy and
//! relaunch Scour, so one click is enough.
//!
//! There is no signing key to generate or configure. The tradeoff is that the
//! installer is trusted because of the HTTPS connection to GitHub, not because
//! of a detached signature.

use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use tauri::Emitter;

const RELEASES_API: &str = "https://api.github.com/repos/khentmoba/scour/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/khentmoba/scour/releases/latest";

/// Event emitted while an update is downloading, listened to by `dist/main.js`.
pub const UPDATE_PROGRESS_EVENT: &str = "scour://update-progress";

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug)]
struct ReleaseInfo {
    version: String,
    notes: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Clone)]
struct DownloadTarget {
    name: String,
    url: String,
}

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

fn is_newer(current: &str, remote: &str) -> Result<bool, String> {
    let current = semver::Version::parse(current)
        .map_err(|e| format!("invalid installed version {current}: {e}"))?;
    let remote = semver::Version::parse(remote)
        .map_err(|e| format!("invalid release version {remote}: {e}"))?;
    Ok(remote > current)
}

/// Only the NSIS setup can update an installed Scour in place. Our release
/// workflow always builds one; an MSI-only release is reported rather than
/// silently installing a second copy.
fn pick_installer(assets: &[GithubAsset]) -> Option<DownloadTarget> {
    let nsis = assets
        .iter()
        .find(|a| a.name.ends_with("-setup.exe") && a.name.contains("x64"))
        .or_else(|| assets.iter().find(|a| a.name.ends_with("-setup.exe")))?;
    Some(DownloadTarget {
        name: nsis.name.clone(),
        url: nsis.browser_download_url.clone(),
    })
}

async fn latest_release(client: &Client) -> Result<GithubRelease, String> {
    let response = client
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "scour")
        .send()
        .await
        .map_err(|e| format!("could not reach GitHub Releases: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("GitHub Releases returned {status}"));
    }
    let body = response
        .text()
        .await
        .map_err(|e| format!("could not read the release response: {e}"))?;
    serde_json::from_str(&body)
        .map_err(|e| format!("release response has an unexpected shape: {e}"))
}

fn release_version(release: &GithubRelease) -> &str {
    release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name)
}

fn release_status(current: &str, release: &GithubRelease) -> Result<Value, String> {
    let version = release_version(release).to_string();
    let found = if is_newer(current, &version)? {
        Some(ReleaseInfo {
            version,
            notes: Some(release.body.trim().to_string()).filter(|s| !s.is_empty()),
            date: release.published_at.clone(),
        })
    } else {
        None
    };
    Ok(status_payload(current, found))
}

#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Read the newest release from GitHub. Nothing is downloaded here.
#[tauri::command]
pub async fn check_update() -> Result<Value, String> {
    let current = env!("CARGO_PKG_VERSION");
    let client = Client::new();
    let release = latest_release(&client).await?;
    release_status(current, &release)
}

/// Download the newest NSIS installer and let it replace and relaunch Scour.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<Value, String> {
    let client = Client::new();
    let release = latest_release(&client).await?;
    let version = release_version(&release).to_string();
    if !is_newer(env!("CARGO_PKG_VERSION"), &version)? {
        return Err("already running the latest version".into());
    }

    let target = pick_installer(&release.assets)
        .ok_or_else(|| format!("no Windows setup.exe in the latest release, see {RELEASES_PAGE}"))?;

    let path = std::env::temp_dir().join(&target.name);
    let mut response = client
        .get(&target.url)
        .header("User-Agent", "scour")
        .send()
        .await
        .map_err(|e| format!("could not start the download: {e}"))?
        .error_for_status()
        .map_err(|e| format!("the download was rejected: {e}"))?;
    let total = response.content_length();
    let mut file = File::create(&path)
        .map_err(|e| format!("could not create {}: {e}", path.display()))?;
    let mut downloaded = 0u64;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("download failed: {e}"))?
    {
        file.write_all(&chunk)
            .map_err(|e| format!("could not write the installer: {e}"))?;
        downloaded += chunk.len() as u64;
        let _ = app.emit(UPDATE_PROGRESS_EVENT, progress_payload(downloaded, total));
    }
    drop(file);
    let _ = app.emit(
        UPDATE_PROGRESS_EVENT,
        json!({ "phase": "downloaded", "percent": 100.0 }),
    );

    spawn_installer(&path)?;

    // Windows exits here so the installer can replace the running executable.
    Ok(json!({ "installed": true, "version": version, "restart_required": cfg!(not(windows)) }))
}

#[cfg(windows)]
fn spawn_installer(path: &PathBuf) -> Result<(), String> {
    Command::new(path)
        .args(["/S", "/R"])
        .spawn()
        .map_err(|e| format!("could not start the installer: {e}"))?;
    std::process::exit(0);
}

#[cfg(not(windows))]
fn spawn_installer(_path: &PathBuf) -> Result<(), String> {
    Err("automatic install is only supported on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GithubAsset {
        GithubAsset {
            name: name.into(),
            browser_download_url: format!("https://example.test/{name}"),
        }
    }

    #[test]
    fn reports_when_no_new_version_exists() {
        let payload = status_payload("0.1.0", None);
        assert_eq!(payload["available"], false);
        assert_eq!(payload["version"], "0.1.0");
    }

    #[test]
    fn reports_the_remote_version_when_available() {
        let payload = status_payload(
            "0.1.0",
            Some(ReleaseInfo {
                version: "0.2.0".into(),
                notes: Some("faster scans".into()),
                date: None,
            }),
        );
        assert_eq!(payload["available"], true);
        assert_eq!(payload["version"], "0.2.0");
        assert_eq!(payload["notes"], "faster scans");
    }

    #[test]
    fn maps_a_github_release_to_the_expected_status() {
        let release: GithubRelease = serde_json::from_str(
            r#"{"tag_name":"v0.2.0","body":"  faster scans  ",
                "published_at":"2026-11-17T00:00:00Z","assets":[]}"#,
        )
        .unwrap();
        let status = release_status("0.1.0", &release).unwrap();
        assert_eq!(status["available"], true);
        assert_eq!(status["version"], "0.2.0");
        assert_eq!(status["notes"], "faster scans");
        assert_eq!(status["date"], "2026-11-17T00:00:00Z");

        let current = release_status("0.2.0", &release).unwrap();
        assert_eq!(current["available"], false);
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("0.1.0", "0.2.0").unwrap());
        assert!(!is_newer("0.2.0", "0.1.0").unwrap());
        assert!(!is_newer("0.1.0", "0.1.0").unwrap());
        assert!(is_newer("v0.1.0", "0.1.1").is_err());
    }

    #[test]
    fn picks_the_64_bit_nsis_installer() {
        let assets = vec![
            asset("Scour_0.2.0_x64_en-US.msi"),
            asset("Scour_0.2.0_x64-setup.exe"),
            asset("Scour_0.2.0_x64-setup.exe.sig"),
        ];
        let target = pick_installer(&assets).unwrap();
        assert_eq!(target.name, "Scour_0.2.0_x64-setup.exe");
    }

    #[test]
    fn progress_accumulates_chunks_and_computes_a_percentage() {
        let first = progress_payload(25, Some(100));
        assert_eq!(first["downloaded"], 25);
        assert_eq!(first["percent"], 25.0);

        let last = progress_payload(100, Some(100));
        assert_eq!(last["percent"], 100.0);

        let unknown = progress_payload(42, None);
        assert!(unknown["percent"].is_null());
    }
}
