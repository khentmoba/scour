//! Native Windows disk + clutter scanner.
//!
//! The bundled Burrow engine only reports caches on Windows, so a "scan for
//! junk" through it finds almost nothing. This module does the Windows half of
//! the job in-process: it measures the user profile for the disk overview and
//! finds the same kinds of disposable build output, package caches, temps and
//! stale installers a disk utility should. Nothing here deletes anything —
//! every path is handed to the review-first Recycle Bin path in `cleanup.rs`.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Fixed order of the dithered disk overview, matching the UI palette.
pub const DISK_CATEGORIES: [&str; 9] = [
    "Apps",
    "Developer",
    "Photos",
    "Movies",
    "Documents",
    "Music",
    "System",
    "Caches",
    "Trash",
];

#[derive(Debug, Clone, Serialize)]
pub struct DiskSlice {
    pub name: String,
    pub bytes: u64,
    pub human: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanItem {
    pub path: String,
    pub label: String,
    pub category: String,
    pub bytes: u64,
    pub human: String,
    pub detail: String,
    /// False for things we only report (e.g. an app that is still installed).
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanPayload {
    pub disk: Vec<DiskSlice>,
    pub items: Vec<ScanItem>,
    pub volume_total: u64,
    pub volume_used: u64,
    pub total_bytes: u64,
    pub total_human: String,
    pub notes: Vec<String>,
    pub scanned_at: u64,
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "0 B".into();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from).filter(|p| p.is_dir())
}

fn home() -> Option<PathBuf> {
    env_path("USERPROFILE").or_else(|| env_path("HOME"))
}

fn index_of(name: &str) -> usize {
    DISK_CATEGORIES
        .iter()
        .position(|c| *c == name)
        .unwrap_or(DISK_CATEGORIES.len() - 1)
}

/// Directories that are never worth descending into while measuring a profile:
/// they are either huge, virtual, or handled as their own clutter category.
fn skip_dir(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        ".git" | ".svn" | ".hg" | "node_modules" | "target" | "dist" | "build"
            | ".next" | ".turbo" | ".nuxt" | ".svelte-kit" | ".parcel-cache" | ".angular"
            | ".astro" | ".output" | ".venv" | "venv" | "__pycache__" | ".pytest_cache"
            | ".mypy_cache" | ".ruff_cache" | ".tox" | ".nox" | ".dart_tool"
            | ".cache" | "cache" | "cachedata" | "code cache" | "gpucache" | "crashpad"
    )
}

/// Sum file sizes under `root`, pruning skipped directory names and capping
/// depth. Symlinks and junctions are ignored so a link cannot cross the disk.
fn dir_size(root: &Path, max_depth: usize) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if skip_dir(&name) {
                    continue;
                }
                stack.push((entry.path(), depth + 1));
            } else if ft.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            }
        }
    }
    total
}

/// Classify one file into a broad disk-overview bucket from its extension.
fn media_category(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "raw" | "tif" | "tiff"
        | "svg" | "cr2" | "nef" => "Photos",
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg" => {
            "Movies"
        }
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "aiff" => "Music",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "rtf"
        | "csv" | "odt" | "ods" | "epub" => "Documents",
        _ => "Documents",
    }
}

fn media_sizes(root: &Path, buckets: &mut [u64; 9]) {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 10 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if skip_dir(&name) {
                    continue;
                }
                stack.push((entry.path(), depth + 1));
            } else if ft.is_file() {
                if let Ok(meta) = entry.metadata() {
                    let cat = media_category(&entry.file_name().to_string_lossy());
                    buckets[index_of(cat)] += meta.len();
                }
            }
        }
    }
}

/// Used and total bytes on the system drive, from PowerShell. This is the
/// denominator for the bar and the "System data" residual, computed the same
/// way Beautiful Disk does.
fn volume() -> Option<(u64, u64)> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "$d = Get-PSDrive -Name $env:SystemDrive.TrimEnd(':'); \"$($d.Used) $($d.Free)\"",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    let used: u64 = parts.next()?.parse().ok()?;
    let free: u64 = parts.next()?.parse().ok()?;
    Some((used, used + free))
}

/// Build the disk-overview slices from the parts of the profile we can measure
/// quickly, plus the system-data residual.
fn scan_disk(notes: &mut Vec<String>) -> (Vec<DiskSlice>, (u64, u64)) {
    let mut buckets = [0u64; 9];
    let volume = volume().unwrap_or((0, 0));
    let Some(home) = home() else {
        notes.push("Could not locate the user profile; disk overview unavailable.".into());
        return (Vec::new(), volume);
    };

    for folder in ["Desktop", "Downloads", "Documents", "Pictures", "Videos", "Music"] {
        let root = home.join(folder);
        if root.is_dir() {
            media_sizes(&root, &mut buckets);
        }
    }

    let local = home.join("AppData").join("Local");
    let apps = local.join("Programs");
    if apps.is_dir() {
        buckets[index_of("Apps")] = dir_size(&apps, 5);
    }

    // Cache roots are counted once here so the bar and the clean list agree.
    let mut cache_roots: Vec<PathBuf> = vec![
        local.join("Temp"),
        local.join("npm-cache"),
        local.join("pip"),
        local.join("Yarn"),
        local.join("deno"),
    ];
    for name in [".bun", ".cargo", ".rustup", ".gradle", ".m2", "go", ".cache", ".npm", ".nuget", ".pub-cache"] {
        cache_roots.push(home.join(name));
    }
    for cache in cache_roots {
        if cache.is_dir() {
            buckets[index_of("Caches")] += dir_size(&cache, 7);
        }
    }

    let known: u64 = buckets.iter().sum();
    if volume.0 > known {
        buckets[index_of("System")] = volume.0 - known;
    }
    if volume.0 == 0 {
        notes.push("System data could not be measured; its slice is omitted.".into());
    }

    let mut slices: Vec<DiskSlice> = DISK_CATEGORIES
        .iter()
        .zip(buckets.iter())
        .filter(|(_, bytes)| **bytes > 0)
        .map(|(name, bytes)| DiskSlice {
            name: (*name).into(),
            bytes: *bytes,
            human: human(*bytes),
        })
        .collect();
    slices.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    if slices.is_empty() {
        notes.push("No measurable files found in the usual profile folders.".into());
    }
    (slices, volume)
}

/// Ignore cache crumbs. Beautiful Disk uses a similar threshold so the treemap
/// shows things worth a decision instead of hundreds of byte-sized tiles.
const MIN_BYTES: u64 = 20 * 1024 * 1024;

fn push_item(
    items: &mut Vec<ScanItem>,
    path: &Path,
    label: &str,
    category: &str,
    detail: &str,
    bytes: u64,
) {
    if bytes < MIN_BYTES {
        return;
    }
    items.push(ScanItem {
        path: path.to_string_lossy().into_owned(),
        label: label.into(),
        category: category.into(),
        bytes,
        human: human(bytes),
        detail: detail.into(),
        removable: true,
    });
}

/// Find named directories under `root`, without descending into a match.
fn find_named_dirs(root: &Path, wanted: &[&str], max_depth: usize, out: &mut Vec<PathBuf>) {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() || !ft.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if wanted.iter().any(|w| name.eq_ignore_ascii_case(w)) {
                out.push(entry.path());
                continue;
            }
            let lower = name.to_ascii_lowercase();
            // Tool/home folders are not projects; their caches are measured
            // separately. Skipping them keeps the discovery walk cheap.
            if lower.starts_with('.')
                || matches!(
                    lower.as_str(),
                    "appdata" | "windows" | "$recycle.bin" | "node_modules" | "target"
                        | "programdata" | "system volume information"
                )
            {
                continue;
            }
            stack.push((entry.path(), depth + 1));
        }
    }
}

fn explicit_caches(home: &Path, items: &mut Vec<ScanItem>) {
    let local = home.join("AppData").join("Local");
    let roaming = home.join("AppData").join("Roaming");
    let mut roots: Vec<(&str, PathBuf, &str, &str)> = vec![
        ("Temporary files", local.join("Temp"), "Caches", "Files apps left behind."),
        ("npm cache", home.join(".npm").join("_cacache"), "Caches", "Downloaded again when needed."),
        ("npm logs", home.join(".npm").join("_logs"), "Caches", "Old command logs."),
        ("Yarn cache", local.join("Yarn").join("Cache"), "Caches", "Downloaded again when needed."),
        ("pnpm store", local.join("pnpm").join("store"), "Caches", "Downloaded again when needed."),
        ("bun cache", home.join(".bun").join("install").join("cache"), "Caches", "Downloaded again when needed."),
        ("Cargo registry", home.join(".cargo").join("registry"), "Developer", "Downloaded again when needed."),
        ("Cargo git cache", home.join(".cargo").join("git"), "Developer", "Downloaded again when needed."),
        ("Rustup downloads", home.join(".rustup").join("downloads"), "Developer", "Downloaded again when needed."),
        ("Gradle cache", home.join(".gradle").join("caches"), "Developer", "Rebuilt on the next build."),
        ("Maven repository", home.join(".m2").join("repository"), "Developer", "Downloaded again when needed."),
        ("pip cache", local.join("pip").join("Cache"), "Developer", "Downloaded again when needed."),
        ("uv cache", home.join(".cache").join("uv"), "Developer", "Downloaded again when needed."),
        ("Go module cache", home.join("go").join("pkg").join("mod"), "Developer", "Downloaded again when needed."),
        ("Deno cache", local.join("deno"), "Developer", "Downloaded again when needed."),
        ("Composer cache", roaming.join("Composer").join("cache"), "Developer", "Downloaded again when needed."),
        ("Playwright browsers", home.join(".cache").join("ms-playwright"), "Developer", "Reinstalled when needed."),
        ("Cypress cache", home.join(".cache").join("Cypress"), "Developer", "Reinstalled when needed."),
        ("Puppeteer cache", home.join(".cache").join("puppeteer"), "Developer", "Reinstalled when needed."),
        ("Hugging Face models", home.join(".cache").join("huggingface"), "Developer", "Downloaded again when needed."),
        ("Ollama models", local.join("Ollama"), "Developer", "Downloaded again when needed."),
        ("VS Code cached data", roaming.join("Code").join("CachedData"), "Developer", "Rebuilt on the next launch."),
    ];

    // One tile per actual cache folder. The path must be the cache itself — a
    // cleaning decision removes exactly that path, never the browser profile.
    for (browser, base) in [
        ("Chrome", local.join("Google").join("Chrome").join("User Data")),
        ("Edge", local.join("Microsoft").join("Edge").join("User Data")),
        ("Brave", local.join("BraveSoftware").join("Brave-Browser").join("User Data")),
    ] {
        if base.is_dir() {
            let mut found = Vec::new();
            find_named_dirs(&base, &["Cache", "Code Cache", "GPUCache", "DawnCache"], 5, &mut found);
            for p in found {
                roots.push((browser, p, "Caches", "Regenerated automatically."));
            }
        }
    }
    let firefox = local.join("Mozilla").join("Firefox").join("Profiles");
    if firefox.is_dir() {
        let mut found = Vec::new();
        find_named_dirs(&firefox, &["cache2"], 4, &mut found);
        for p in found {
            roots.push(("Firefox", p, "Caches", "Regenerated automatically."));
        }
    }

    for (label, path, cat, detail) in roots {
        if path.as_os_str().is_empty() || !path.is_dir() {
            continue;
        }
        let bytes = dir_size(&path, 8);
        push_item(items, &path, label, cat, detail, bytes);
    }
}

fn build_outputs(home: &Path, items: &mut Vec<ScanItem>) {
    let mut found = Vec::new();
    find_named_dirs(
        home,
        &[
            "node_modules", "target", ".venv", "venv", ".next", ".turbo", ".nuxt",
            ".svelte-kit", ".parcel-cache", ".astro", "__pycache__", ".pytest_cache",
            ".mypy_cache", ".ruff_cache", ".tox", ".nox", ".dart_tool",
        ],
        5,
        &mut found,
    );
    for path in found {
        if path.starts_with(home.join("AppData")) {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let (label, detail) = match name.as_str() {
            "node_modules" => ("Node modules", "Installed again with npm install."),
            "target" => ("Rust build output", "Rebuilt on the next cargo build."),
            ".venv" | "venv" => ("Python environment", "Recreate with python -m venv."),
            ".next" => ("Next.js build", "Rebuilt on the next build."),
            ".turbo" => ("Turborepo cache", "Rebuilt on the next task run."),
            "__pycache__" => ("Python byte-code", "Regenerated automatically."),
            ".pytest_cache" | ".mypy_cache" | ".ruff_cache" => ("Python cache", "Regenerated automatically."),
            _ => ("Build output", "Regenerated on the next build."),
        };
        let bytes = dir_size(&path, 6);
        push_item(items, &path, label, "Developer", detail, bytes);
    }
}

fn stale_installers(home: &Path, items: &mut Vec<ScanItem>) {
    let downloads = home.join("Downloads");
    if !downloads.is_dir() {
        return;
    }
    let cutoff = now_secs().saturating_sub(30 * 24 * 3600);
    let Ok(entries) = fs::read_dir(&downloads) else { return };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_file() || ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if ![
            ".exe", ".msi", ".msix", ".appx", ".msixbundle", ".iso", ".cab", ".dmg",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext))
        {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if modified > cutoff {
            continue;
        }
        push_item(
            items,
            &entry.path(),
            &name,
            "Installers",
            "Installer older than 30 days in Downloads.",
            meta.len(),
        );
    }
}

fn worktrees(items: &mut Vec<ScanItem>, notes: &mut Vec<String>) {
    let Some(home) = home() else { return };
    let mut roots = vec![
        home.join(".t3").join("worktrees"),
        home.join(".codex").join("worktrees"),
        home.join(".cursor").join("worktrees"),
        home.join(".capy").join("worktrees"),
        home.join(".verdent").join("worktrees"),
        home.join("conductor").join("workspaces"),
    ];
    for project in ["Developer", "Projects", "source", "Documents", "Desktop", "Downloads"] {
        roots.push(home.join(project).join(".claude").join("worktrees"));
    }
    let mut git_missing = false;
    for base in roots {
        if !base.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&base) else { continue };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            let output = std::process::Command::new("git")
                .args(["-C", &path.to_string_lossy(), "status", "--porcelain"])
                .output();
            let clean = match output {
                Ok(o) => o.status.success() && o.stdout.is_empty(),
                Err(_) => {
                    git_missing = true;
                    continue;
                }
            };
            if clean {
                let bytes = dir_size(&path, 8);
                let label = format!("Clean worktree — {}", entry.file_name().to_string_lossy());
                push_item(items, &path, &label, "Developer", "No local changes; git worktree remove is safe.", bytes);
            }
        }
    }
    if git_missing {
        notes.push("git was unavailable, so some worktrees were not checked for changes.".into());
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppEntry {
    pub name: String,
    pub bytes: u64,
    pub human: String,
    pub location: String,
    pub id: String,
}

/// Read the Windows uninstall registry to answer "what is installed and how
/// big is it". This is deliberately read-only: removing apps needs their own
/// uninstaller, never the Recycle Bin.
pub fn scan_apps() -> Result<Vec<AppEntry>, String> {
    const PS: &str = r#"
$keys = @(
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
$apps = Get-ItemProperty $keys -ErrorAction SilentlyContinue |
  Where-Object { $_.DisplayName -and $_.EstimatedSize } |
  ForEach-Object { [pscustomobject]@{
    name = [string]$_.DisplayName
    kb = [int64]$_.EstimatedSize
    location = [string]$_.InstallLocation
    id = [string]$_.PSChildName
  } }
$apps | ConvertTo-Json -Compress -Depth 2
"#;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", PS])
        .output()
        .map_err(|e| format!("could not read installed apps: {e}"))?;
    if !out.status.success() {
        return Err("could not read installed apps".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("unexpected app inventory: {e}"))?;
    let list = value.as_array().cloned().unwrap_or_else(|| vec![value]);
    let mut entries: Vec<AppEntry> = list
        .iter()
        .filter_map(|v| {
            let name = v.get("name")?.as_str()?.to_owned();
            let kb = v.get("kb")?.as_i64().unwrap_or(0).max(0) as u64;
            if name.is_empty() || kb == 0 {
                return None;
            }
            let bytes = kb * 1024;
            Some(AppEntry {
                name,
                bytes,
                human: human(bytes),
                location: v.get("location").and_then(|x| x.as_str()).unwrap_or("").to_owned(),
                id: v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_owned(),
            })
        })
        .collect();
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    entries.truncate(100);
    Ok(entries)
}

/// Run the whole native scan. Cheap enough to call on demand from the UI.
pub fn scan_all() -> ScanPayload {
    let home = home();
    let mut notes = Vec::new();
    if home.is_none() {
        notes.push("Could not locate the user profile; clutter scan unavailable.".into());
    }

    // The phases touch disjoint folders, so run them on a scoped thread pool.
    // On a spinning disk this keeps the whole scan close to its slowest part.
    let (disk, vol, caches, outputs, installers, trees, tree_notes) =
        std::thread::scope(|scope| {
            let h = home.clone();
            let disk_job = scope.spawn(move || {
                let mut local_notes = Vec::new();
                let (slices, vol) = scan_disk(&mut local_notes);
                (slices, vol, local_notes)
            });
            let h = home.clone();
            let caches_job = scope.spawn(move || {
                let mut v = Vec::new();
                if let Some(h) = &h {
                    explicit_caches(h, &mut v);
                }
                v
            });
            let h = home.clone();
            let outputs_job = scope.spawn(move || {
                let mut v = Vec::new();
                if let Some(h) = &h {
                    build_outputs(h, &mut v);
                }
                v
            });
            let h = home.clone();
            let installers_job = scope.spawn(move || {
                let mut v = Vec::new();
                if let Some(h) = &h {
                    stale_installers(h, &mut v);
                }
                v
            });
            let trees_job = scope.spawn(move || {
                let mut v = Vec::new();
                let mut n = Vec::new();
                worktrees(&mut v, &mut n);
                (v, n)
            });
            let (disk, vol, disk_notes) = disk_job.join().unwrap_or_default();
            let (tree_items, tree_notes) = trees_job.join().unwrap_or_default();
            notes.extend(disk_notes);
            (
                disk,
                vol,
                caches_job.join().unwrap_or_default(),
                outputs_job.join().unwrap_or_default(),
                installers_job.join().unwrap_or_default(),
                tree_items,
                tree_notes,
            )
        });
    notes.extend(tree_notes);

    let mut items = Vec::new();
    items.extend(caches);
    items.extend(outputs);
    items.extend(installers);
    items.extend(trees);

    items.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    let total_bytes = items.iter().filter(|i| i.removable).map(|i| i.bytes).sum();
    ScanPayload {
        disk,
        items,
        volume_total: vol.1,
        volume_used: vol.0,
        total_bytes,
        total_human: human(total_bytes),
        notes,
        scanned_at: now_secs(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_reads() {
        assert_eq!(human(0), "0 B");
        assert_eq!(human(1024), "1.0 KB");
        assert_eq!(human(1536), "1.5 KB");
        assert_eq!(human(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn categorises_media() {
        assert_eq!(media_category("photo.JPG"), "Photos");
        assert_eq!(media_category("clip.mp4"), "Movies");
        assert_eq!(media_category("song.flac"), "Music");
        assert_eq!(media_category("notes.docx"), "Documents");
        assert_eq!(media_category("app.exe"), "Documents");
    }

    /// A cleaning decision removes exactly the path shown. A scan must never
    /// offer a whole Downloads folder, a browser profile, or the home root.
    #[test]
    fn scan_items_are_specific_paths() {
        let home = home().expect("a user profile is required for this test");
        let payload = scan_all();
        assert!(!payload.items.is_empty(), "scanner should find real clutter");
        for item in &payload.items {
            let path = Path::new(&item.path);
            assert!(path.starts_with(&home), "outside profile: {}", item.path);
            assert_ne!(path, home.as_path(), "profile root offered as a candidate");
            assert_ne!(
                path,
                home.join("Downloads").as_path(),
                "whole Downloads folder offered as a candidate"
            );
            assert!(
                fs::symlink_metadata(path)
                    .map(|m| !m.file_type().is_symlink())
                    .unwrap_or(false),
                "symlink offered as a candidate: {}",
                item.path
            );
        }
    }

    #[test]
    fn reads_installed_apps() {
        // Windows only: other platforms return an error we surface in the UI.
        if !cfg!(windows) {
            return;
        }
        let apps = scan_apps().expect("app inventory");
        assert!(!apps.is_empty(), "expected at least one installed app");
        assert!(apps.iter().all(|a| !a.name.is_empty() && a.bytes > 0));
        assert!(apps.windows(2).all(|w| w[0].bytes >= w[1].bytes), "apps sorted by size");
    }

    #[test]
    fn scan_returns_shape() {
        // Runs against the real machine, but only measures and never deletes.
        let payload = scan_all();
        assert!(payload.disk.iter().all(|s| !s.name.is_empty() && s.bytes > 0));
        assert!(payload.items.iter().all(|i| !i.path.is_empty() && i.bytes > 0));
        assert_eq!(
            payload.total_bytes,
            payload.items.iter().filter(|i| i.removable).map(|i| i.bytes).sum::<u64>()
        );
        assert!(!payload.total_human.is_empty());
    }
}
