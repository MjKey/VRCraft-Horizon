//! Post-build step of the `vivecraft` bundle (`[[post_build]]` in `horizon-mixin.toml`):
//! copies the OpenXR loader the bundle ships in its `data/` folder into the build's
//! version folder, where the game finds it beside its executable
//! (`native-data/vivecraft/`, what `horizon_client::native_data::built` reads).
//!
//! The step is run by the launcher after the game compiles, with:
//! - `HORIZON_NATIVE_BUNDLE_DATA_DIR` — the bundle's staged `data/`
//!   (`native/stage/native-data/vivecraft/`): read it, don't write it;
//! - `HORIZON_NATIVE_DATA_DIR` — `<version folder>/native-data/vivecraft/`,
//!   created by the launcher: what the game reads at run time.
//!
//! The bundle ships the loader by placing `openxr_loader.dll` at
//! `mixins/vivecraft/data/openxr_loader.dll` (the `.hnmod` reader allows any file
//! under the bundle's `data/`; 512 files / 64 MiB limit). The dll is NOT in this
//! repository: a source-only bundle has none, this step then exits successfully
//! and the game falls back to a loader found on PATH.
use std::{env, fs, path::PathBuf};

/// The loader library names `xr.rs` tries, in the same order per platform.
fn loader_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["openxr_loader.dll"]
    }
    #[cfg(target_os = "macos")]
    {
        &["libopenxr_loader.dylib"]
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        &["libopenxr_loader.so.1", "libopenxr_loader.so"]
    }
}

fn main() {
    let bundle = env::var_os("HORIZON_NATIVE_BUNDLE_DATA_DIR").map(PathBuf::from);
    let data = env::var_os("HORIZON_NATIVE_DATA_DIR").map(PathBuf::from);
    let (Some(bundle), Some(data)) = (bundle, data) else {
        // Only the launcher runs post-build steps; a manual run has no folders to fill.
        eprintln!(
            "[vivecraft] post-build: HORIZON_NATIVE_BUNDLE_DATA_DIR / \
             HORIZON_NATIVE_DATA_DIR not set; nothing to copy"
        );
        return;
    };
    if let Err(e) = fs::create_dir_all(&data) {
        eprintln!("[vivecraft] post-build: {}: {e}", data.display());
        std::process::exit(1);
    }
    let mut copied = 0;
    for name in loader_names() {
        let src = bundle.join(name);
        if !src.is_file() {
            continue;
        }
        let dst = data.join(name);
        match fs::copy(&src, &dst) {
            Ok(bytes) => {
                eprintln!(
                    "[vivecraft] post-build: copied {name} ({bytes} bytes) to {}",
                    dst.display()
                );
                copied += 1;
            }
            // A copy that fails must fail the build, not leave a half-staged version folder.
            Err(e) => {
                eprintln!(
                    "[vivecraft] post-build: copying {} to {}: {e}",
                    src.display(),
                    dst.display()
                );
                std::process::exit(1);
            }
        }
    }
    if copied == 0 {
        eprintln!(
            "[vivecraft] post-build: no OpenXR loader in {} (ship one as \
             mixins/vivecraft/data/openxr_loader.dll, e.g. from the SteamVR install); \
             the game will only find a loader on PATH",
            bundle.display()
        );
    }
}
