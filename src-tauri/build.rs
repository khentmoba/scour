use std::fs;
use std::path::PathBuf;

fn main() {
    tauri_build::build();

    // Keep the engine next to Scour's own executable in dev and release builds so
    // `current_exe().parent()` can find it without any Tauri packaging magic.
    let Ok(out_dir) = std::env::var("OUT_DIR") else { return };
    let Some(profile_dir) = PathBuf::from(out_dir)
        .ancestors()
        .nth(3)
        .map(|p| p.to_path_buf())
    else {
        return;
    };
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join("burrow-engine.exe");
    let dst = profile_dir.join("burrow-engine.exe");
    if src.is_file() {
        if let Err(e) = fs::copy(&src, &dst) {
            println!("cargo:warning=could not stage burrow-engine.exe: {e}");
        }
    }
    println!("cargo:rerun-if-changed=binaries/burrow-engine.exe");
}
