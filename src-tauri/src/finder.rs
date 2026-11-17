//! Interactive "find files to clean" search, powered by the fff-search crate.
//!
//! Unlike the Burrow engine (one process per scan), the fff picker lives in
//! Scour's own process: index a user-chosen folder once, then every keystroke
//! searches warm memory. Only the folder the user picks is indexed — never a
//! drive root — and deletions still go through `cleanup::delete_reviewed`.

use fff_search::{
    FFFMode, FilePicker, FilePickerOptions, FileSearchConfig, FuzzySearchOptions, GitRecencyConfig,
    PaginationArgs, QueryParser, SharedFilePicker, SharedFrecency,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// One indexed folder. Re-indexing replaces the session; dropping the old
/// picker cancels its background scan and watcher threads.
pub struct FinderSession {
    picker: SharedFilePicker,
    base: PathBuf,
}

#[derive(Default)]
pub struct FinderState(pub Mutex<Option<FinderSession>>);

#[derive(Debug, Serialize)]
pub struct FindHit {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
    pub score: i32,
}

#[derive(Debug, Serialize)]
pub struct FindResults {
    pub base: String,
    pub total_matched: usize,
    pub total_files: usize,
    pub items: Vec<FindHit>,
}

/// Refuse anything that is not an existing, absolute, ordinary directory.
/// Drive roots are refused outright: indexing `C:\` is never the intent here.
fn validate_folder(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("enter a folder path first".into());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(format!("folder path must be absolute: {trimmed}"));
    }
    let meta = std::fs::metadata(&path)
        .map_err(|e| format!("cannot inspect {trimmed}: {e}"))?;
    if !meta.is_dir() {
        return Err(format!("not a folder: {trimmed}"));
    }
    if path.parent().is_none() {
        return Err(format!("refusing to index a drive root: {trimmed}"));
    }
    Ok(path)
}

/// Index `folder` and wait (bounded) for the initial scan. Replaces any
/// previous session.
pub fn index_folder(state: &FinderState, folder: &str) -> Result<FindResults, String> {
    let base = validate_folder(folder)?;

    let picker = SharedFilePicker::default();
    let frecency = SharedFrecency::default();
    FilePicker::new_with_shared_state(
        picker.clone(),
        frecency,
        FilePickerOptions {
            base_path: base.to_string_lossy().into_owned(),
            mode: FFFMode::Ai,
            // Scour only needs names, sizes and dates — no content index, no
            // mmap warmup. Keeps the resident footprint small.
            enable_content_indexing: false,
            enable_mmap_cache: false,
            watch: true,
            follow_symlinks: false,
            enable_fs_root_scanning: false,
            enable_home_dir_scanning: true,
            git_recency: GitRecencyConfig {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not start indexing: {e}"))?;

    if !picker.wait_for_scan(Duration::from_secs(60)) {
        return Err("indexing timed out — try a smaller folder".into());
    }

    let total_files = {
        let guard = picker.read().map_err(|e| e.to_string())?;
        guard.as_ref().map(|p| p.live_file_count()).unwrap_or(0)
    };

    let results = FindResults {
        base: base.to_string_lossy().into_owned(),
        total_matched: total_files,
        total_files,
        items: Vec::new(),
    };
    *state.0.lock().map_err(|e| e.to_string())? = Some(FinderSession { picker, base });
    Ok(results)
}

/// Search the indexed folder. An empty query lists the largest files first —
/// the most useful starting point for a cleanup review.
pub fn search(state: &FinderState, query: &str, limit: usize) -> Result<FindResults, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or("index a folder first")?;
    let picker_guard = session.picker.read().map_err(|e| e.to_string())?;
    let picker = picker_guard.as_ref().ok_or("index is not ready")?;
    let limit = limit.clamp(1, 500);

    let mut hits: Vec<FindHit> = Vec::new();
    let (total_matched, total_files) = if query.trim().is_empty() {
        let mut by_size: Vec<&fff_search::FileItem> = picker
            .get_files()
            .iter()
            .filter(|item| !item.is_deleted())
            .collect();
        by_size.sort_by(|a, b| b.size.cmp(&a.size));
        for item in by_size.iter().take(limit) {
            hits.push(hit_of(picker, &session.base, item, 0));
        }
        let live = by_size.len();
        (live, live)
    } else {
        let parser = QueryParser::new(FileSearchConfig);
        let parsed = parser.parse(query);
        let result = picker.fuzzy_search(
            &parsed,
            None,
            FuzzySearchOptions {
                // The UI debounces keystrokes; one interactive query at a time
                // doesn't need a rayon fan-out (thread setup dominated latency
                // on small indexes in testing).
                max_threads: 1,
                pagination: PaginationArgs { offset: 0, limit },
                ..Default::default()
            },
        );
        for (item, score) in result.items.iter().zip(result.scores.iter()) {
            hits.push(hit_of(picker, &session.base, item, score.total));
        }
        (result.total_matched, result.total_files)
    };

    Ok(FindResults {
        base: session.base.to_string_lossy().into_owned(),
        total_matched,
        total_files,
        items: hits,
    })
}

fn hit_of(picker: &FilePicker, base: &Path, item: &fff_search::FileItem, score: i32) -> FindHit {
    let relative = item.relative_path(picker);
    let absolute = base.join(&relative);
    FindHit {
        name: item.file_name(picker),
        path: absolute.to_string_lossy().into_owned(),
        size: item.size,
        modified: item.modified,
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn staging(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scour-find-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("setup-installer.exe"), vec![0u8; 4096]).unwrap();
        fs::write(dir.join("nested").join("old_backup.log"), vec![0u8; 2048]).unwrap();
        fs::write(dir.join("notes.txt"), b"keep me").unwrap();
        dir
    }

    #[test]
    fn refuses_roots_and_missing_folders() {
        assert!(validate_folder("").is_err());
        assert!(validate_folder("relative/dir").is_err());
        assert!(validate_folder("C:\\").is_err());
        assert!(
            validate_folder(
                &std::env::temp_dir()
                    .join("scour-find-does-not-exist")
                    .to_string_lossy()
            )
            .is_err()
        );
    }

    #[test]
    fn indexes_and_finds_with_a_typo() {
        let dir = staging("typo");
        let state = FinderState::default();
        let indexed = index_folder(&state, &dir.to_string_lossy()).unwrap();
        assert_eq!(indexed.total_files, 3);

        // "instalr" is a typo of "installer" — fff's fuzzy matching must survive it.
        let found = search(&state, "instalr", 50).unwrap();
        assert!(
            found.items.iter().any(|h| h.name == "setup-installer.exe"),
            "typo query should still find the installer: {:?}",
            found.items
        );

        // Extension constraint from the fff query language.
        let logs = search(&state, "*.log", 50).unwrap();
        assert_eq!(logs.items.len(), 1);
        assert_eq!(logs.items[0].name, "old_backup.log");
        assert_eq!(logs.items[0].size, 2048);

        // Empty query ranks by size, largest first.
        let biggest = search(&state, "", 50).unwrap();
        assert_eq!(biggest.items[0].name, "setup-installer.exe");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_before_indexing_is_a_clear_error() {
        let state = FinderState::default();
        let err = search(&state, "anything", 10).unwrap_err();
        assert!(err.contains("index a folder first"), "{err}");
    }

    #[test]
    fn search_results_flow_into_the_deletion_guard() {
        // End to end: index → fuzzy search → the exact returned paths reach
        // cleanup's validator and deleter. Injected deleter: no real deletion.
        let dir = staging("e2e");
        let state = FinderState::default();
        index_folder(&state, &dir.to_string_lossy()).unwrap();
        let found = search(&state, "backp", 50).unwrap();
        let target = found
            .items
            .iter()
            .find(|h| h.name == "old_backup.log")
            .expect("fuzzy search finds the backup log");

        let seen = std::sync::Mutex::new(Vec::new());
        let events = crate::cleanup::delete_reviewed_with(&[target.path.clone()], |p| {
            seen.lock().unwrap().push(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            seen.into_inner().unwrap(),
            vec![PathBuf::from(&target.path)]
        );
        assert!(matches!(
            events[0],
            crate::cleanup::CleanEvent::Removed { bytes: 2048, .. }
        ));
        assert!(matches!(
            events[1],
            crate::cleanup::CleanEvent::Done { ok: true, .. }
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_query_skips_tombstoned_files() {
        let dir = staging("tombstone");
        let state = FinderState::default();
        let indexed = index_folder(&state, &dir.to_string_lossy()).unwrap();
        assert_eq!(indexed.total_files, 3);

        let victim = dir.join("setup-installer.exe");
        {
            let guard = state.0.lock().unwrap();
            let session = guard.as_ref().unwrap();
            let mut picker = session.picker.write().unwrap();
            assert!(picker.as_mut().unwrap().remove_file_by_path(&victim));
        }

        let res = search(&state, "", 50).unwrap();
        assert_eq!(res.total_matched, 2);
        assert_eq!(res.total_files, 2);
        assert!(
            !res.items.iter().any(|h| h.name == "setup-installer.exe"),
            "tombstoned file must not reappear in empty-query results: {:?}",
            res.items
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// True end to end with the REAL Recycle Bin deleter, on a disposable
    /// staged file. Run explicitly: `cargo test real_recycle_bin -- --ignored`
    #[test]
    #[ignore]
    fn real_recycle_bin_e2e() {
        let dir = staging("recycle");
        let state = FinderState::default();
        index_folder(&state, &dir.to_string_lossy()).unwrap();
        let found = search(&state, "instalr", 50).unwrap();
        let target = found
            .items
            .iter()
            .find(|h| h.name == "setup-installer.exe")
            .expect("typo search finds the installer");

        let events = crate::cleanup::delete_reviewed(&[target.path.clone()]).unwrap();
        assert!(matches!(
            events[0],
            crate::cleanup::CleanEvent::Removed { .. }
        ));
        assert!(matches!(
            events[1],
            crate::cleanup::CleanEvent::Done { ok: true, .. }
        ));
        assert!(
            !Path::new(&target.path).exists(),
            "file must be gone from disk (now in the Recycle Bin)"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Warm-query latency proof. Run explicitly:
    /// `cargo test finder_perf -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn finder_perf_warm_queries() {
        let dir = std::env::temp_dir().join(format!("scour-find-perf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for i in 0..40 {
            let sub = dir.join(format!("folder-{i:03}"));
            fs::create_dir_all(&sub).unwrap();
            for j in 0..50 {
                fs::write(sub.join(format!("document-{i:03}-{j:03}.log")), b"x").unwrap();
            }
        }
        let state = FinderState::default();
        let t0 = std::time::Instant::now();
        let indexed = index_folder(&state, &dir.to_string_lossy()).unwrap();
        let scan_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let queries = ["docment", "*.log", "document-017", "instalr", "folder-023 doc"];
        let t1 = std::time::Instant::now();
        for _ in 0..20 {
            for q in queries {
                let r = search(&state, q, 100).unwrap();
                assert!(r.total_files == indexed.total_files);
            }
        }
        let per_query_us = t1.elapsed().as_micros() as f64 / 100.0;
        eprintln!(
            "PERF: {} files indexed in {scan_ms:.1} ms; 100 warm queries avg {per_query_us:.0} µs",
            indexed.total_files
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reindex_replaces_the_session() {
        let a = staging("reindex-a");
        let b = staging("reindex-b");
        fs::remove_file(b.join("notes.txt")).unwrap();
        fs::write(b.join("unique-marker.dat"), b"x").unwrap();
        let state = FinderState::default();
        index_folder(&state, &a.to_string_lossy()).unwrap();
        let second = index_folder(&state, &b.to_string_lossy()).unwrap();
        assert_eq!(second.total_files, 3);

        // A file that exists only in the first folder must be unreachable now.
        // Full filename: a bare "notes" fuzzy-matches other names (typo tolerance).
        let stale = search(&state, "notes.txt", 50).unwrap();
        assert!(
            !stale.items.iter().any(|h| h.name == "notes.txt"),
            "old index must be gone: {:?}",
            stale.items
        );
        // A file that exists only in the second folder must be findable.
        let fresh = search(&state, "unique-marker", 50).unwrap();
        assert!(fresh.items.iter().any(|h| h.name == "unique-marker.dat"));
        fs::remove_dir_all(&a).unwrap();
        fs::remove_dir_all(&b).unwrap();
    }
}