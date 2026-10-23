fn main() {
    #[cfg(target_os = "macos")]
    compile_afm_helper();
    tauri_build::build();
}

#[cfg(target_os = "macos")]
fn compile_afm_helper() {
    println!("cargo:rerun-if-changed=afm/main.swift");
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest.join("afm/main.swift");
    let out_dir = manifest.join("binaries");
    std::fs::create_dir_all(&out_dir).expect("binaries");
    let out = out_dir.join("vrclibman-afm-aarch64-apple-darwin");
    let status = std::process::Command::new("swiftc")
        .arg("-O")
        .arg("-o")
        .arg(&out)
        .arg(&source)
        .status()
        .unwrap_or_else(|err| panic!("swiftc を起動できません: {err}"));
    if !status.success() {
        panic!("Apple のオンデバイスモデル補助プログラムをビルドできません");
    }
}
