fn main() {
    tauri_build::build();
    // tauri-build links its resource (including Common Controls v6) into app
    // binaries only. The library test executable also needs it when mocking Tauri.
    #[cfg(all(windows, target_env = "msvc"))]
    println!(
        "cargo:rustc-link-search=native={}",
        std::env::var("OUT_DIR").unwrap()
    );
}