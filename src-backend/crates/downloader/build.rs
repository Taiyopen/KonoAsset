//! `Ctx` holds a `tauri::AppHandle`, so the unit-test executable links WebView2 / common controls
//! and needs the Common-Controls v6 manifest, otherwise it fails with `STATUS_ENTRYPOINT_NOT_FOUND`.
//! see https://github.com/tauri-apps/tauri/pull/4383#issuecomment-1212221864
fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV");
    if target_os == "windows" && Ok("msvc") == target_env.as_deref() {
        let manifest =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            manifest.to_str().unwrap()
        );
    }
}
