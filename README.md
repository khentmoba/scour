<img src="assets/icon.svg" width="64" height="64" alt="">

# Scour

A small, review-first disk cleaner for Windows. It scans with the
[Burrow](https://github.com/caezium/Burrow) engine, shows you exactly what it found,
and deletes **only the items you tick** — to the Recycle Bin, by default.

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
| Scan for junk | `clean --dry-run` |
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

## Install and update

Tagged releases (`v*`) are published by [`.github/workflows/release.yml`](.github/workflows/release.yml)
with two Windows installers:

- `Scour_<version>_x64-setup.exe` — NSIS installer (per-user, no elevation needed).
- `Scour_<version>_x64.msi` — WiX installer, for machines managed with MSI.

Pick either one. Once installed, open **Updates** in Scour and click **Check for
updates**. When a newer release exists, **Update & restart** downloads the signed
installer, verifies it against the public key in `src-tauri/tauri.conf.json`,
installs it and relaunches Scour on the new version — no second click. Nothing is
checked or downloaded unless you press the button.

The check reads `latest.json` from the newest GitHub release. Development builds
are not a supported update target: there is no installer for them to replace, so
don't use **Update & restart** from `npm run dev`.

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

- `dist/` — the frontend (plain HTML/CSS/JS, no bundler)
- `src-tauri/src/engine.rs` — engine process wrapper and JSON parsing
- `src-tauri/src/commands.rs` — Tauri commands exposed to the UI
- `src-tauri/src/updater.rs` — in-app update checks, download progress and install
- `src-tauri/binaries/` — bundled engine plus its license notices
- `.github/workflows/release.yml` — builds signed installers and `latest.json` on a tag

## Releasing

1. Bump the version in **all three** places: `package.json`,
   `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (then refresh
   `src-tauri/Cargo.lock` with `cargo check`).
2. Commit, tag `vX.Y.Z` and push the tag. The release workflow builds both
   installers, signs them, publishes the release assets and writes `latest.json`.

Signing needs a private key that is deliberately **not** in the repository. It was
generated locally with `cargo tauri signer generate` and is stored at
`signing/tauri-private.key` (gitignored). Before the first release, add its
contents as the repository secret `TAURI_SIGNING_PRIVATE_KEY`. The key has no
password, so `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` can be left empty. The matching
public key is already embedded in `src-tauri/tauri.conf.json`.

If the private key is lost, generate a new pair and update **both** the secret and
the `pubkey` in `tauri.conf.json` in the same release — an existing install will
only accept updates signed by the key it shipped with.

## Licensing

Scour's own code is [MIT](LICENSE). The bundled `burrow-engine` is © 2026 caezium
and licensed under **FSL-1.1-ALv2** — see `src-tauri/binaries/LICENSE.md` and
`THIRD-PARTY-NOTICES.md`. Personal and internal use and redistribution are permitted
under that license; a commercial product that competes with Burrow is not. The
license converts to Apache 2.0 in 2028. The name "Burrow" is deliberately not used
for this project's branding.
