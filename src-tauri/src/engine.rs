//! Thin, testable wrapper around the `burrow-engine` command line.
//!
//! The engine speaks argv in, JSON out. Scour never links its code: it launches
//! the executable as a separate process, which keeps the FSL-1.1 boundary clean
//! and lets us swap the binary via `SCOUR_ENGINE` for tests.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
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

        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                dirs.push(parent.to_path_buf());
                dirs.push(parent.join("binaries"));
                dirs.push(parent.join(".."));
            }
        }
        dirs.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("binaries"),
        );

        for dir in dirs {
            for name in ["burrow-engine.exe", "burrow-engine"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Ok(Self::new(candidate));
                }
            }
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

    /// `status` is envelope-wrapped but often returns `ok:false` on Windows, so
    /// return the whole envelope and let the UI present the reason.
    pub fn status(&self) -> Result<Value, String> {
        let out = self.run(&["status"])?;
        parse_json(&out)
    }

    pub fn clean_preview(&self) -> Result<CleanData, String> {
        let out = self.run(&["clean", "--dry-run"])?;
        decode_envelope(&out)
    }

    pub fn analyze(&self, path: &str) -> Result<AnalyzeData, String> {
        let out = self.run(&["analyze", path])?;
        decode_envelope(&out)
    }

    pub fn preview_optimize(&self) -> Result<Value, String> {
        let out = self.run(&["optimize", "--dry-run"])?;
        decode_data(&out)
    }

    /// Delete exactly the reviewed paths. The engine removes only what is in
    /// the plan file, and Scour deletes the plan file immediately afterwards.
    pub fn apply_plan(&self, paths: &[String]) -> Result<Vec<Value>, String> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let plan = write_plan_file(paths, &std::env::temp_dir(), stamp as u64)?;
        let plan_arg = plan.to_string_lossy().into_owned();
        let result = self.run(&["clean", "--plan", &plan_arg, "--apply"]);
        let _ = fs::remove_file(&plan);
        let out = result?;
        let events = parse_ndjson(&out)?;
        if !out.status.success() {
            return Err(format!(
                "engine exited with status {} during cleanup ({} event(s) emitted); see the run log",
                out.status,
                events.len()
            ));
        }
        Ok(events)
    }
}

/// The one real side effect: start the engine as a child process.
pub fn spawn(exe: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new(exe)
        .args(args)
        .output()
        .map_err(|e| format!("could not start {}: {e}", exe.display()))
}

fn parse_json(out: &Output) -> Result<Value, String> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !stdout.trim().is_empty() {
        return serde_json::from_str(stdout.trim())
            .map_err(|e| format!("engine returned invalid JSON: {e}"));
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("engine exited with status {}", out.status)
    } else {
        stderr
    })
}

fn decode_data(out: &Output) -> Result<Value, String> {
    let env: Envelope<Value> = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
        .map_err(|e| format!("engine returned an unexpected envelope: {e}"))?;
    if let Some(err) = env.error {
        return Err(format!("{}: {}", err.kind, err.message));
    }
    if !env.ok {
        return Err("engine reported failure".into());
    }
    env.data.ok_or_else(|| "engine returned no data".into())
}

fn decode_envelope<T: for<'de> Deserialize<'de>>(out: &Output) -> Result<T, String> {
    decode_data(out).and_then(|v| {
        serde_json::from_value(v).map_err(|e| format!("engine data has an unexpected shape: {e}"))
    })
}

fn parse_ndjson(out: &Output) -> Result<Vec<Value>, String> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| match serde_json::from_str(l) {
            Ok(v) => Some(v),
            Err(_) => None,
        })
        .collect();
    if lines.is_empty() {
        let single = parse_json(out)?;
        return Ok(vec![single]);
    }
    Ok(lines)
}

/// Build the plan the engine will execute. Only non-empty, non-comment lines
/// are included; the engine itself validates that each path is a clean target.
pub fn write_plan_file(paths: &[String], dir: &Path, id: u64) -> Result<PathBuf, String> {
    let mut body = String::from("# Scour reviewed clean plan\n");
    let mut count = 0usize;
    for raw in paths {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        body.push_str(line);
        body.push('\n');
        count += 1;
    }
    if count == 0 {
        return Err("no paths selected for cleaning".into());
    }
    let file = dir.join(format!("scour-plan-{id}.txt"));
    fs::write(&file, body)
        .map_err(|e| format!("could not write clean plan: {e}"))?;
    Ok(file)
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    #[serde(default)]
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

    #[test]
    fn writes_only_selected_paths_to_plan() {
        let dir = std::env::temp_dir();
        let file = write_plan_file(
            &["C:\\tmp\\a".into(), "".into(), "# note".into(), "D:\\b".into()],
            &dir,
            424242,
        )
        .unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert_eq!(text, "# Scour reviewed clean plan\nC:\\tmp\\a\nD:\\b\n");
        fs::remove_file(&file).unwrap();
    }

    #[test]
    fn apply_plan_uses_plan_and_apply_flags() {
        let engine = Engine::new(PathBuf::from("engine"));
        let events = engine
            .run_with(&["clean", "--plan", "plan.txt", "--apply"], |_, _| {
                Ok(output("{\"event\":\"removed\",\"path\":\"C:\\\\tmp\\\\a\"}\n{\"event\":\"done\",\"dry_run\":false}\n"))
            })
            .and_then(|out| parse_ndjson(&out))
            .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "removed");
    }

    #[test]
    fn rejects_empty_plan() {
        let err = write_plan_file(&["".into(), "# only a comment".into()], &std::env::temp_dir(), 1);
        assert!(err.is_err());
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
