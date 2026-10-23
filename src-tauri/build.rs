fn main() {
    #[cfg(target_os = "macos")]
    compile_afm_helper();
    tauri_build::build();
}

#[cfg(target_os = "macos")]
fn compile_afm_helper() {
    println!("cargo:rerun-if-changed=afm/main.swift");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "aarch64".into());
    if arch != "aarch64" {
        println!(
            "cargo:warning=Apple のオンデバイスモデル補助プログラムは {arch} では作りません"
        );
        return;
    }
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest.join("afm/main.swift");
    let out_dir = manifest.join("binaries");
    std::fs::create_dir_all(&out_dir).expect("binaries");
    let out = out_dir.join(format!("vrclibman-afm-{arch}-apple-darwin"));
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
