fn main() {
    #[cfg(target_os = "macos")]
    compile_afm_helper();
    tauri_build::build();
    // tauri-build links its resource (including Common Controls v6) into app
    // binaries only. The library test executable also needs it when mocking Tauri.
    #[cfg(all(windows, target_env = "msvc"))]
    println!(
        "cargo:rustc-link-search=native={}",
        std::env::var("OUT_DIR").unwrap()
    );
}

#[cfg(target_os = "macos")]
fn compile_afm_helper() {
    println!("cargo:rerun-if-changed=afm/main.swift");
    println!("cargo:rerun-if-changed=afm/unavailable-stub.c");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "aarch64".into());
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = manifest.join("binaries");
    std::fs::create_dir_all(&out_dir).expect("binaries");
    let out = out_dir.join(format!("vrclibman-afm-{arch}-apple-darwin"));
    if arch != "aarch64" {
        compile_unavailable_stub(&out, &manifest, &arch);
        return;
    }
    let source = manifest.join("afm/main.swift");
    // AFM 3 Core Advanced の日本展開は macOS 27.2 以降。アプリ本体の最小 OS は 14 のまま。
    let status = std::process::Command::new("swiftc")
        .arg("-O")
        .arg("-parse-as-library")
        .arg("-target")
        .arg("arm64-apple-macos27.2")
        .arg("-o")
        .arg(&out)
        .arg(&source)
        .status()
        .unwrap_or_else(|err| panic!("swiftc を起動できません: {err}"));
    if !status.success() {
        panic!("Apple のオンデバイスモデル補助プログラムをビルドできません");
    }
}

#[cfg(target_os = "macos")]
fn compile_unavailable_stub(out: &std::path::Path, manifest: &std::path::Path, arch: &str) {
    let source = manifest.join("afm/unavailable-stub.c");
    let target = match arch {
        "x86_64" => "x86_64-apple-macos14.0",
        "aarch64" => "arm64-apple-macos14.0",
        other => panic!("Apple の補助プログラムのスタブを {other} 向けに作れません"),
    };
    let status = std::process::Command::new("clang")
        .arg("-O2")
        .arg("-target")
        .arg(target)
        .arg("-o")
        .arg(out)
        .arg(&source)
        .status()
        .unwrap_or_else(|err| panic!("clang を起動できません: {err}"));
    if !status.success() {
        panic!("Apple のオンデバイスモデルを使わないスタブをビルドできません");
    }
}
