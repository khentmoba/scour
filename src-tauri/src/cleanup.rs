//! The Windows half of a clean run.
//!
//! The Burrow engine scans portable paths, but its `--apply` is macOS only.
//! Scour therefore performs the deletion itself, and only for paths the user
//! reviewed, using the operating system's Recycle Bin so the action is
//! recoverable. Nothing here deletes permanently.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn real_deleter(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| format!("could not move to Recycle Bin: {e}"))
}

/// Delete exactly the reviewed paths. Each outcome is emitted as an NDJSON-style
/// event so the UI can show the same log shape for every run.
pub fn delete_reviewed(paths: &[String]) -> Result<Vec<Value>, String> {
    delete_reviewed_with(paths, real_deleter)
}

/// A deleter is injected so tests can prove the selection reaches it without
/// ever touching real files.
pub fn delete_reviewed_with<F>(paths: &[String], deleter: F) -> Result<Vec<Value>, String>
where
    F: Fn(&Path) -> Result<(), String>,
{
    let checked = validate(paths)?;
    let mut events = Vec::new();
    let mut freed = 0u64;
    let mut removed = 0usize;
    let total = checked.len();

    for path in checked {
        let bytes = tree_size(&path);
        match deleter(&path) {
            Ok(()) => {
                events.push(json!({
                    "event": "removed",
                    "path": path.to_string_lossy(),
                    "bytes": bytes,
                }));
                freed += bytes;
                removed += 1;
            }
            Err(reason) => {
                events.push(json!({
                    "event": "failed",
                    "path": path.to_string_lossy(),
                    "reason": reason,
                }));
            }
        }
    }

    events.push(json!({
        "event": "done",
        "dry_run": false,
        "ok": removed == total,
        "freed_bytes": freed,
        "freed_human": humansize(freed),
        "count": removed,
        "failed_count": total - removed,
        "total": total,
    }));
    Ok(events)
}

/// Refuse anything that is not an existing, absolute, ordinary path outside the
/// protected system and profile roots. The engine's own target rules decide
/// what is offered for review in the first place; this is the last guard.
pub fn validate(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let home = PathBuf::from(home);
    let resolved_home = fs::canonicalize(&home).unwrap_or_else(|_| home.clone());
    let mut checked = Vec::new();

    for raw in paths {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let path = PathBuf::from(line);
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(format!("refusing non-absolute or root path: {line}"));
        }
        fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot inspect {line}: {e}"))?;
        if has_link_in_path(&path) {
            return Err(format!("refusing a link in path: {line}"));
        }
        // Junctions and `..` can make the literal path point somewhere else.
        // Check the target the OS will actually delete, not just the spelling.
        let resolved = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if forbidden(&resolved, &resolved_home) {
            return Err(format!("refusing a protected path: {line}"));
        }
        checked.push(path);
    }

    if checked.is_empty() {
        return Err("no paths selected for cleaning".into());
    }
    Ok(checked)
}

fn forbidden(path: &Path, home: &Path) -> bool {
    if !home.as_os_str().is_empty() && (path == home || home.starts_with(path)) {
        return true;
    }
    let lower = path.to_string_lossy().to_lowercase();
    let blocked = [
        "windows",
        "program files",
        "program files (x86)",
        "programdata",
        "system32",
        "$recycle.bin",
        "$windows.~bt",
        "$windows.~ws",
    ];
    if blocked.iter().any(|part| {
        lower
            .split(['\\', '/'])
            .any(|c| c.eq_ignore_ascii_case(part))
    }) {
        return true;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if path == dir || dir.starts_with(path) {
                return true;
            }
        }
    }
    false
}

/// Check every component, not just the leaf: a junction under the profile can
/// redirect an otherwise ordinary-looking path to a protected location.
fn has_link_in_path(path: &Path) -> bool {
    path.ancestors()
        .filter_map(|part| fs::symlink_metadata(part).ok())
        .any(|meta| meta.file_type().is_symlink())
}

fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = fs::metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else { return 0 };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        total += if ft.is_dir() {
            tree_size(&entry.path())
        } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };
    }
    total
}

fn humansize(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let i = (bytes as f64).log(1024.0).floor() as usize;
    let i = i.min(UNITS.len() - 1);
    format!("{:.1}{}", bytes as f64 / 1024f64.powi(i as i32), UNITS[i])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn emits_removed_events_for_exactly_the_reviewed_paths() {
        let staging = std::env::temp_dir().join(format!("scour-test-{}", std::process::id()));
        let a = staging.join("a.txt");
        let b = staging.join("b.txt");
        fs::create_dir_all(&staging).unwrap();
        fs::write(&a, b"12345").unwrap();
        fs::write(&b, b"123").unwrap();

        let seen = Mutex::new(Vec::new());
        let events = delete_reviewed_with(
            &[
                a.to_string_lossy().into_owned(),
                b.to_string_lossy().into_owned(),
            ],
            |p| {
                seen.lock().unwrap().push(p.to_path_buf());
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(seen.into_inner().unwrap(), vec![a.clone(), b.clone()]);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["event"], "removed");
        assert_eq!(events[1]["event"], "removed");
        assert_eq!(events[2]["event"], "done");
        assert_eq!(events[2]["count"], 2);
        assert_eq!(events[2]["freed_bytes"], 8);
        fs::remove_dir_all(&staging).unwrap();
    }

    #[test]
    fn failed_deletions_are_flagged_in_the_done_event() {
        let staging = std::env::temp_dir().join(format!("scour-fail-{}", std::process::id()));
        fs::create_dir_all(&staging).unwrap();
        let p = staging.join("locked.txt");
        fs::write(&p, b"data").unwrap();

        let events = delete_reviewed_with(&[p.to_string_lossy().into_owned()], |_| {
            Err("file is locked".into())
        })
        .unwrap();

        assert_eq!(events[0]["event"], "failed");
        assert_eq!(events[1]["event"], "done");
        assert_eq!(events[1]["ok"], false);
        assert_eq!(events[1]["count"], 0);
        assert_eq!(events[1]["failed_count"], 1);
        assert_eq!(events[1]["total"], 1);
        fs::remove_dir_all(&staging).unwrap();
    }

    #[test]
    fn refuses_unsafe_paths_before_any_deleter_runs() {
        let cases = [
            "C:\\Windows\\Temp".to_string(),
            "relative/path".to_string(),
            std::env::temp_dir().join("does-not-exist-scour").to_string_lossy().into_owned(),
        ];
        for bad in cases {
            assert!(validate(&[bad]).is_err(), "should have refused");
        }
        let seen = Mutex::new(0);
        let result = delete_reviewed_with(&["C:\\Windows\\Temp".into()], |_| {
            *seen.lock().unwrap() += 1;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(*seen.lock().unwrap(), 0);
    }
}
