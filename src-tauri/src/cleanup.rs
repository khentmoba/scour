//! The Windows half of a clean run.
//!
//! The Burrow engine scans portable paths, but its `--apply` is macOS only.
//! Scour therefore performs the deletion itself, and only for paths the user
//! reviewed, using the operating system's Recycle Bin so the action is
//! recoverable. Nothing here deletes permanently.

use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum CleanEvent {
    Removed {
        path: String,
        bytes: u64,
    },
    Failed {
        path: String,
        reason: String,
    },
    Done {
        dry_run: bool,
        ok: bool,
        freed_bytes: u64,
        freed_human: String,
        count: usize,
        failed_count: usize,
        total: usize,
    },
}

fn cached_home() -> &'static PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        normalize_lexical(Path::new(&home))
    })
}

fn cached_exe_dir() -> Option<&'static Path> {
    static EXE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    EXE_DIR
        .get_or_init(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        })
        .as_deref()
}

/// Delete exactly the reviewed paths. Uses `trash::delete_all` to batch the
/// Windows Recycle Bin shell operation into a single transaction, falling back
/// to per-path deletion only if the batch fails so individual failures are
/// still reported.
pub fn delete_reviewed(paths: &[String]) -> Result<Vec<CleanEvent>, String> {
    let (checked, sizes) = validate_borrowed(paths, true)?;
    let total = checked.len();
    let mut events = Vec::with_capacity(total + 1);
    let mut freed = 0u64;
    let mut removed = 0usize;

    delete_bisect(&checked, &sizes, &mut events, &mut freed, &mut removed);
    events.push(done_event(freed, removed, total));
    Ok(events)
}

/// Batch-delete via `trash::delete_all`, bisecting only if a batch fails so a
/// single locked file doesn't force per-file Recycle Bin transactions for the rest.
fn delete_bisect(
    paths: &[(&str, &Path)],
    sizes: &[u64],
    events: &mut Vec<CleanEvent>,
    freed: &mut u64,
    removed: &mut usize,
) {
    if paths.is_empty() {
        return;
    }
    if paths.len() <= 8 {
        for (&(raw, p), &bytes) in paths.iter().zip(sizes.iter()) {
            match trash::delete(p) {
                Ok(()) => {
                    events.push(CleanEvent::Removed {
                        path: raw.to_owned(),
                        bytes,
                    });
                    *freed += bytes;
                    *removed += 1;
                }
                Err(e) => {
                    events.push(CleanEvent::Failed {
                        path: raw.to_owned(),
                        reason: format!("could not move to Recycle Bin: {e}"),
                    });
                }
            }
        }
        return;
    }
    if trash::delete_all(paths.iter().map(|(_, p)| *p)).is_ok() {
        for (&(raw, _), &bytes) in paths.iter().zip(sizes.iter()) {
            events.push(CleanEvent::Removed {
                path: raw.to_owned(),
                bytes,
            });
            *freed += bytes;
        }
        *removed += paths.len();
        return;
    }
    // `trash::delete_all` may have moved earlier items before hitting a locked
    // file. Emit `Removed` for paths already gone from disk so bisected retries
    // don't fail on missing files.
    let mut remaining_paths = Vec::new();
    let mut remaining_sizes = Vec::new();
    for (&(raw, p), &sz) in paths.iter().zip(sizes.iter()) {
        if fs::symlink_metadata(p).is_err() {
            events.push(CleanEvent::Removed {
                path: raw.to_owned(),
                bytes: sz,
            });
            *freed += sz;
            *removed += 1;
        } else {
            remaining_paths.push((raw, p));
            remaining_sizes.push(sz);
        }
    }
    if remaining_paths.is_empty() {
        return;
    }
    if remaining_paths.len() < paths.len() {
        if remaining_paths.len() == 1 {
            delete_bisect(&remaining_paths, &remaining_sizes, events, freed, removed);
        } else {
            let mid = remaining_paths.len() / 2;
            delete_bisect(&remaining_paths[..mid], &remaining_sizes[..mid], events, freed, removed);
            delete_bisect(&remaining_paths[mid..], &remaining_sizes[mid..], events, freed, removed);
        }
        return;
    }
    let mid = paths.len() / 2;
    delete_bisect(&paths[..mid], &sizes[..mid], events, freed, removed);
    delete_bisect(&paths[mid..], &sizes[mid..], events, freed, removed);
}

/// A deleter is injected so tests can prove the selection reaches it without
/// ever touching real files.
pub fn delete_reviewed_with<F>(paths: &[String], deleter: F) -> Result<Vec<CleanEvent>, String>
where
    F: Fn(&Path) -> Result<(), String>,
{
    let (checked, sizes) = validate_borrowed(paths, true)?;
    emit_with_deleter(&checked, &sizes, deleter)
}

fn emit_with_deleter<F>(checked: &[(&str, &Path)], sizes: &[u64], deleter: F) -> Result<Vec<CleanEvent>, String>
where
    F: Fn(&Path) -> Result<(), String>,
{
    let total = checked.len();
    let mut events = Vec::with_capacity(total + 1);
    let mut freed = 0u64;
    let mut removed = 0usize;

    for (&(raw, path), &bytes) in checked.iter().zip(sizes.iter()) {
        match deleter(path) {
            Ok(()) => {
                events.push(CleanEvent::Removed {
                    path: raw.to_owned(),
                    bytes,
                });
                freed += bytes;
                removed += 1;
            }
            Err(reason) => {
                events.push(CleanEvent::Failed {
                    path: raw.to_owned(),
                    reason,
                });
            }
        }
    }

    events.push(done_event(freed, removed, total));
    Ok(events)
}

fn done_event(freed: u64, removed: usize, total: usize) -> CleanEvent {
    CleanEvent::Done {
        dry_run: false,
        ok: removed == total,
        freed_bytes: freed,
        freed_human: humansize(freed),
        count: removed,
        failed_count: total - removed,
        total,
    }
}

/// Refuse anything that is not an existing, absolute, ordinary path outside the
/// protected system and profile roots. The engine's own target rules decide
/// what is offered for review in the first place; this is the last guard.
pub fn validate(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    validate_borrowed(paths, false)
        .map(|(checked, _)| checked.into_iter().map(|(_, p)| p.to_path_buf()).collect())
}

#[inline]
fn is_blocked_part(s: &str) -> bool {
    let b = match s.as_bytes().first() {
        Some(&b) if s.len() >= 7 => b,
        _ => return false,
    };
    if !matches!(b, b'w' | b'W' | b'p' | b'P' | b's' | b'S' | b'$') {
        return false;
    }
    BLOCKED_PARTS.iter().any(|part| s.eq_ignore_ascii_case(part))
}

fn validate_borrowed<'a>(
    paths: &'a [String],
    collect_sizes: bool,
) -> Result<(Vec<(&'a str, &'a Path)>, Vec<u64>), String> {
    let resolved_home = cached_home();
    let exe_dir = cached_exe_dir();
    let mut safe_ancestors: HashSet<PathBuf> = HashSet::new();
    let mut last_safe_parent: Option<&'a Path> = None;
    let mut checked = Vec::with_capacity(paths.len());
    let mut sizes = if collect_sizes { Vec::with_capacity(paths.len()) } else { Vec::new() };

    for raw in paths {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let path = Path::new(line);
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(format!("refusing non-absolute or root path: {line}"));
        }
        // Lexical / protected-path check before any filesystem metadata syscalls.
        let mut has_dots = false;
        let mut blocked_hit = false;
        for comp in path.components() {
            match comp {
                Component::CurDir | Component::ParentDir => {
                    has_dots = true;
                    break;
                }
                Component::Normal(part) => {
                    if let Some(s) = part.to_str() {
                        if is_blocked_part(s) {
                            blocked_hit = true;
                            break;
                        }
                    } else {
                        let s = part.to_string_lossy();
                        if is_blocked_part(&s) {
                            blocked_hit = true;
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        let is_forbidden = if blocked_hit {
            true
        } else if has_dots {
            let resolved = normalize_lexical(path);
            forbidden(&resolved, resolved_home, exe_dir)
        } else {
            forbidden_roots(path, resolved_home, exe_dir)
        };
        if is_forbidden {
            return Err(format!("refusing a protected path: {line}"));
        }
        let meta = fs::symlink_metadata(path)
            .map_err(|e| format!("cannot inspect {line}: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err(format!("refusing a link in path: {line}"));
        }
        let parent = path.parent();
        if !(parent.is_some() && parent == last_safe_parent) {
            if !parent.is_some_and(|p| safe_ancestors.contains(p))
                && has_link_in_ancestors(path, &mut safe_ancestors)
            {
                return Err(format!("refusing a link in path: {line}"));
            }
            last_safe_parent = parent;
        }
        if collect_sizes {
            let sz = if meta.is_dir() {
                dir_tree_size(path)
            } else {
                meta.len()
            };
            sizes.push(sz);
        }
        checked.push((line, path));
    }

    if checked.is_empty() {
        return Err("no paths selected for cleaning".into());
    }
    Ok((checked, sizes))
}

const BLOCKED_PARTS: [&str; 8] = [
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "system32",
    "$recycle.bin",
    "$windows.~bt",
    "$windows.~ws",
];

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

fn forbidden_roots(path: &Path, home: &Path, exe_dir: Option<&Path>) -> bool {
    let path_len = path.as_os_str().len();
    if !home.as_os_str().is_empty()
        && path_len <= home.as_os_str().len()
        && (path == home || home.starts_with(path))
    {
        return true;
    }
    if let Some(dir) = exe_dir {
        if path_len <= dir.as_os_str().len() && (path == dir || dir.starts_with(path)) {
            return true;
        }
    }
    false
}

fn forbidden(path: &Path, home: &Path, exe_dir: Option<&Path>) -> bool {
    if forbidden_roots(path, home, exe_dir) {
        return true;
    }
    for comp in path.components() {
        if let Component::Normal(part) = comp {
            if let Some(s) = part.to_str() {
                if is_blocked_part(s) {
                    return true;
                }
            } else {
                let s = part.to_string_lossy();
                if is_blocked_part(&s) {
                    return true;
                }
            }
        }
    }
    false
}

/// Check every ancestor component, caching verified link-free directories so
/// batches sharing parent folders don't re-stat `C:\Users\...\Temp` repeatedly.
fn has_link_in_ancestors(path: &Path, safe_ancestors: &mut HashSet<PathBuf>) -> bool {
    for anc in path.ancestors().skip(1) {
        if safe_ancestors.contains(anc) {
            break;
        }
        if let Ok(meta) = fs::symlink_metadata(anc) {
            if meta.file_type().is_symlink() {
                return true;
            }
            safe_ancestors.insert(anc.to_path_buf());
        }
    }
    false
}

fn dir_tree_size(root: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else if let Ok(m) = entry.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn humansize(bytes: u64) -> String {
    if bytes == 0 {
        return "0.0B".into();
    }
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
        assert!(matches!(events[0], CleanEvent::Removed { .. }));
        assert!(matches!(events[1], CleanEvent::Removed { .. }));
        assert!(matches!(
            events[2],
            CleanEvent::Done {
                count: 2,
                freed_bytes: 8,
                ..
            }
        ));
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

        assert!(matches!(events[0], CleanEvent::Failed { .. }));
        assert!(matches!(
            events[1],
            CleanEvent::Done {
                ok: false,
                count: 0,
                failed_count: 1,
                total: 1,
                ..
            }
        ));
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
