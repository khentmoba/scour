<img src="assets/icon.svg" width="64" height="64" alt="">

# Scour

A small, review-first disk cleaner for Windows. The overview and clutter scan are
implemented natively in Rust (inspired by the dithered disk bar and treemap of
[Beautiful Disk](https://github.com/vkpdeveloper/beautifuldisk)); the bundled
[Burrow](https://github.com/caezium/Burrow) engine is still used for its own CLI
commands. Scour shows you exactly what it found and deletes **only the items you
tick** — to the Recycle Bin, by default.

The Beautiful Disk look and feature set were reimplemented from scratch in
Scour's own Tauri/HTML stack. No source code was copied from that repository.

Scour is a personal-use tool. It does not phone home, ask for elevation, or delete
anything without an explicit confirmation in the UI.

## How it works

```
Scour (Tauri app)  ──argv──▶  burrow-engine.exe   (scan / analyze / optimize)
                   ◀──JSON──
        │
        └── OS Recycle Bin  (the reviewed deletions)
```

The engine is launched as a separate process; Scour never links its code. Commands
map directly onto the engine's own CLI:

| UI action | What runs |
|---|---|
| Scan for junk | Native Windows scan (caches, build output, old installers, clean worktrees) |
| Analyze a folder | `analyze <path>` |
| Preview maintenance | `optimize --dry-run` |
| Clean selected | Scour moves exactly the ticked paths to the Recycle Bin |
| Status | `status` |

The engine's `--apply` is macOS-only, so on Windows Scour performs the deletion
itself using the OS Recycle Bin. It deletes only the paths you ticked — a second
scan cannot change the list — and refuses absolute-root, system, profile-root and
symlink paths before anything is removed. Deletions are recoverable; nothing is
permanently erased.

`status` and several maintenance tasks are macOS-only in the engine; on Windows the
app shows the engine's own "unavailable" reason instead of faking a result.

### Find (experimental)

The **Find** view searches one folder you pick with
[fff-search](https://github.com/dmtrKovalenko/fff) (MIT), linked into Scour as a
Rust crate — no extra process. The folder is indexed once (a drive root is
refused); after that every keystroke searches warm memory with typo-tolerant
fuzzy matching and filters like `*.log` or `setup !keep/`. An empty query lists
the largest files first. Ticked results go through the same reviewed-deletion
guard and Recycle Bin path as the Clean view. The index lives only in memory
for the current session; nothing is persisted and no content is read.

## Install and update

Tagged releases (`v*`) are published by [`.github/workflows/release.yml`](.github/workflows/release.yml)
with two Windows installers:

- `Scour_<version>_x64-setup.exe` — NSIS installer (per-user, no elevation needed).
- `Scour_<version>_x64.msi` — WiX installer, for machines managed with MSI.

Pick either one. Once installed, open **Updates** in Scour and click **Check for
updates**. When a newer release exists, **Update & restart** downloads that
release's NSIS setup and runs it with the update flag. The installer replaces the
running copy and relaunches Scour on the new version. Nothing is checked or
downloaded unless you press the button.

The check reads the newest GitHub release over HTTPS. No signing key, secret, or
`latest.json` is involved. Updates trust the HTTPS connection to GitHub rather
than a detached signature, so only install from releases published in this
repository. Development builds are not a supported update target because there
is no installer for them to replace.

## Build and run

Prerequisites: Rust (MSVC toolchain), Node 18+, WebView2, and — for installers
only — NSIS and WiX Toolset 3.

```powershell
npm install
npm run dev      # development window
npm run build    # installers in src-tauri/target/release/bundle/
```

To build just the raw executable without NSIS/WiX, run
`cargo build --release --manifest-path src-tauri/Cargo.toml`.

The engine binary lives in `src-tauri/binaries/` and is staged next to Scour's own
executable at build time. To point at a different engine, set `SCOUR_ENGINE` to its
full path. To rebuild it from source, see `scripts/build-engine.ps1`.

Tests (pure parsing, plan writing, and command construction — no deletions):

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

## Layout

- `dist/` — the frontend (plain HTML/CSS/JS, no bundler; dithered bar + treemap)
- `assets/logo.svg` — the Scour mark, also used as the app icon and sidebar logo
- `src-tauri/src/scanner.rs` — native Windows disk overview, clutter scan and app inventory
- `src-tauri/src/engine.rs` — engine process wrapper and JSON parsing
- `src-tauri/src/finder.rs` — fff-search index + fuzzy search for the Find view
- `src-tauri/src/commands.rs` — Tauri commands exposed to the UI
- `src-tauri/src/updater.rs` — in-app update checks, download progress and install
- `src-tauri/binaries/` — bundled engine plus its license notices
- `.github/workflows/release.yml` — builds installers on a tag

## Releasing

1. Bump the version in **all three** places: `package.json`,
   `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (then refresh
   `src-tauri/Cargo.lock` with `cargo check`).
2. Commit, tag `vX.Y.Z` and push the tag. The release workflow builds both
   installers and publishes them as release assets. The tag supplies the version
   that the in-app check compares against.

No signing key or repository secret is needed beyond the GitHub token the
workflow already receives.

## Licensing

Scour's own code is [MIT](LICENSE). It also links the MIT-licensed
[fff-search](https://crates.io/crates/fff-search) crate (© 2026 Dmitriy Kovalenko)
for the experimental Find view. The bundled `burrow-engine` is © 2026 caezium
and licensed under **FSL-1.1-ALv2** — see `src-tauri/binaries/LICENSE.md` and
`THIRD-PARTY-NOTICES.md`. Personal and internal use and redistribution are permitted
under that license; a commercial product that competes with Burrow is not. The
license converts to Apache 2.0 in 2028. The name "Burrow" is deliberately not used
for this project's branding.
