<img src="assets/icon.svg" width="64" height="64" alt="">

# Scour

A small, review-first disk cleaner for Windows. It scans with the
[Burrow](https://github.com/caezium/Burrow) engine, shows you exactly what it found,
and deletes **only the items you tick** — to the Recycle Bin, by default.

Scour is a personal-use tool. It does not phone home, ask for elevation, or delete
anything without an explicit confirmation in the UI.

## How it works

```
Scour (Tauri app)  ──argv──▶  burrow-engine.exe  ──▶  disk
                   ◀──JSON──
```

The engine is launched as a separate process; Scour never links its code. Commands
map directly onto the engine's own CLI:

| UI action | Engine command |
|---|---|
| Scan for junk | `clean --dry-run` |
| Analyze a folder | `analyze <path>` |
| Preview maintenance | `optimize --dry-run` |
| Clean selected | write a plan file, then `clean --plan <file> --apply` |
| Status | `status` |

The plan file contains only the paths you selected, so a second scan cannot change
what gets deleted. Scour removes the plan file immediately after the run.

`status` and several maintenance tasks are macOS-only in the engine; on Windows the
app shows the engine's own "unavailable" reason instead of faking a result.

## Build and run

Prerequisites: Rust (MSVC toolchain), Node 18+, and the WebView2 runtime.

```powershell
npm install
npm run dev      # development window
npm run build    # release exe in src-tauri/target/release/
```

The engine binary lives in `src-tauri/binaries/` and is staged next to Scour's own
executable at build time. To point at a different engine, set `SCOUR_ENGINE` to its
full path. To rebuild it from source, see `scripts/build-engine.ps1`.

Tests (pure parsing, plan writing, and command construction — no deletions):

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

## Layout

- `dist/` — the frontend (plain HTML/CSS/JS, no bundler)
- `src-tauri/src/engine.rs` — engine process wrapper and JSON parsing
- `src-tauri/src/commands.rs` — Tauri commands exposed to the UI
- `src-tauri/binaries/` — bundled engine plus its license notices

## Licensing

Scour's own code is [MIT](LICENSE). The bundled `burrow-engine` is © 2026 caezium
and licensed under **FSL-1.1-ALv2** — see `src-tauri/binaries/LICENSE.md` and
`THIRD-PARTY-NOTICES.md`. Personal and internal use and redistribution are permitted
under that license; a commercial product that competes with Burrow is not. The
license converts to Apache 2.0 in 2028. The name "Burrow" is deliberately not used
for this project's branding.
