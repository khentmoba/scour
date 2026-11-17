//! Thin, testable wrapper around the `burrow-engine` command line.
//!
//! The engine speaks argv in, JSON out. Scour never links its code: it launches
//! the executable as a separate process, which keeps the FSL-1.1 boundary clean
//! and lets us swap the binary via `SCOUR_ENGINE` for tests.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Override the engine executable (used by tests and by custom installs).
pub const ENGINE_ENV: &str = "SCOUR_ENGINE";

#[derive(Debug, Clone)]
pub struct Engine {
    exe: PathBuf,
}

impl Engine {
    pub fn new(exe: PathBuf) -> Self {
        Self { exe }
    }

    /// Find the engine: explicit override first, then the folders a dev or
    /// packaged build actually uses.
    pub fn resolve() -> Result<Self, String> {
        if let Ok(p) = std::env::var(ENGINE_ENV) {
            if !p.trim().is_empty() {
                let pb = PathBuf::from(p);
                if pb.is_file() {
                    return Ok(Self::new(pb));
                }
            }
        }

        let bin_name = if cfg!(windows) {
            "burrow-engine.exe"
        } else {
            "burrow-engine"
        };
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let direct = parent.join(bin_name);
                if direct.is_file() {
                    return Ok(Self::new(direct));
                }
                let dirs = [parent.join("binaries"), parent.join("..")];
                for dir in &dirs {
                    let candidate = dir.join(bin_name);
                    if candidate.is_file() {
                        return Ok(Self::new(candidate));
                    }
                }
            }
        }
        let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(bin_name);
        if candidate.is_file() {
            return Ok(Self::new(candidate));
        }
        Err(format!(
            "burrow-engine not found. Set {ENGINE_ENV} to its full path or place it in src-tauri/binaries/."
        ))
    }

    /// Run one command through an injected runner. Tests use a fake runner;
    /// production uses [`spawn`].
    pub fn run_with<F>(&self, args: &[&str], runner: F) -> Result<Output, String>
    where
        F: FnOnce(&Path, &[&str]) -> Result<Output, String>,
    {
        if args.is_empty() {
            return Err("no engine command given".into());
        }
        runner(&self.exe, args)
    }

    pub fn run(&self, args: &[&str]) -> Result<Output, String> {
        self.run_with(args, spawn)
    }

    pub fn version(&self) -> Result<Value, String> {
        let out = self.run(&["version"])?;
        decode_data(&out)
    }

    pub fn version_raw(&self) -> Result<Box<serde_json::value::RawValue>, String> {
        let out = self.run(&["version"])?;
        decode_raw_envelope(&out)
    }

    pub fn status_raw(&self) -> Result<Box<serde_json::value::RawValue>, String> {
        let out = self.run(&["status"])?;
        if out.stdout.iter().any(|b| !b.is_ascii_whitespace()) {
            return serde_json::from_slice(&out.stdout)
                .map_err(|e| format!("engine returned invalid JSON: {e}"));
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("engine exited with status {}", out.status)
        } else {
            stderr
        })
    }

    pub fn clean_preview(&self) -> Result<CleanData, String> {
        let out = self.run(&["clean", "--dry-run"])?;
        decode_envelope(&out)
    }

    pub fn clean_preview_raw(&self) -> Result<Box<serde_json::value::RawValue>, String> {
        let out = self.run(&["clean", "--dry-run"])?;
        decode_raw_envelope(&out)
    }

    pub fn analyze(&self, path: &str) -> Result<AnalyzeData, String> {
        let out = self.run(&["analyze", path])?;
        decode_envelope(&out)
    }

    pub fn analyze_raw(&self, path: &str) -> Result<Box<serde_json::value::RawValue>, String> {
        let out = self.run(&["analyze", path])?;
        decode_raw_envelope(&out)
    }

    pub fn preview_optimize(&self) -> Result<Value, String> {
        let out = self.run(&["optimize", "--dry-run"])?;
        decode_data(&out)
    }

    pub fn preview_optimize_raw(&self) -> Result<Box<serde_json::value::RawValue>, String> {
        let out = self.run(&["optimize", "--dry-run"])?;
        decode_raw_envelope(&out)
    }

}

/// The one real side effect: start the engine as a child process.
pub fn spawn(exe: &Path, args: &[&str]) -> Result<Output, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output()
        .map_err(|e| format!("could not start {}: {e}", exe.display()))
}

fn decode_data(out: &Output) -> Result<Value, String> {
    decode_envelope::<Value>(out)
}

pub fn decode_envelope<T: for<'de> Deserialize<'de>>(out: &Output) -> Result<T, String> {
    let env: Envelope<T> = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("engine returned an unexpected envelope: {e}"))?;
    if let Some(err) = env.error {
        return Err(format!("{}: {}", err.kind, err.message));
    }
    if !env.ok {
        return Err("engine reported failure".into());
    }
    env.data.ok_or_else(|| "engine returned no data".into())
}

pub fn decode_raw_envelope(out: &Output) -> Result<Box<serde_json::value::RawValue>, String> {
    let raw: Box<serde_json::value::RawValue> = decode_envelope(out)?;
    Ok(raw)
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    #[serde(default)]
    error: Option<EngineError>,
}

#[derive(Debug, Deserialize)]
struct EngineError {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CleanData {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub total_human: String,
    #[serde(default)]
    pub items: Vec<CleanItem>,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CleanItem {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub size_human: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AnalyzeData {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub overview: bool,
    #[serde(default)]
    pub entries: Vec<AnalyzeEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AnalyzeEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub is_dir: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::ExitStatus;

    fn exit_status() -> ExitStatus {
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::process::ExitStatusExt;
            ExitStatus::from_raw(0)
        }
    }

    fn output(stdout: &str) -> Output {
        Output {
            status: exit_status(),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    const PREVIEW: &str = r#"{"ok":true,"burrow_cli":"0.1.0","engine":"burrow-engine","command":"clean","data":{"dry_run":true,"total_bytes":100,"total_human":"100B","items":[{"path":"C:\\tmp\\junk","label":"Temp","size":100,"size_human":"100B"}],"text":"dry"}}"#;

    #[test]
    fn parses_real_clean_envelope() {
        let engine = Engine::new(PathBuf::from("engine"));
        let seen = std::sync::Mutex::new(Vec::new());
        let data = engine
            .run_with(&["clean", "--dry-run"], |exe, args| {
                let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
                seen.lock().unwrap().push((exe.to_path_buf(), owned));
                Ok(output(PREVIEW))
            })
            .and_then(|out| decode_envelope::<CleanData>(&out))
            .unwrap();
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].label, "Temp");
        assert_eq!(data.total_bytes, 100);
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen[0].1, vec!["clean", "--dry-run"]);
    }

    #[test]
    fn surfaces_engine_errors() {
        let out = output(r#"{"ok":false,"command":"status","error":{"kind":"unsupported","message":"no probes"}}"#);
        let err = decode_data(&out).unwrap_err();
        assert!(err.contains("unsupported"), "{err}");
        assert!(err.contains("no probes"), "{err}");
    }

    /// Wiring check against the real staged binary. Safe: `--dry-run` deletes nothing.
    #[test]
    fn real_engine_dry_run() {
        let engine = match Engine::resolve() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skipping real-engine check: {e}");
                return;
            }
        };
        let version = engine.version().expect("version command");
        assert_eq!(version["engine"], "burrow-engine");
        let preview = engine.clean_preview().expect("clean --dry-run command");
        assert!(preview.dry_run);
        // The shape must be stable even when the machine has nothing to clean.
        let _ = preview.items;
    }
}
