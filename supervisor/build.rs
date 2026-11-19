fn main() {
    // option_env! 不被 cargo 自动跟踪: 不写这一行, 改了 CHAOS_BUILD_TIME 也不会重编,
    // 关于页会一直显示第一次编译进去的那个时间。
    println!("cargo:rerun-if-env-changed=CHAOS_BUILD_TIME");
    println!("cargo:rerun-if-env-changed=CHAOS_VERSION_TEXT");
    println!("cargo:rerun-if-env-changed=CHAOS_FIRMWARE");
    println!("cargo:rerun-if-env-changed=PYTHON");
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent().unwrap().to_path_buf();
    let target = std::env::var("CHAOS_FIRMWARE").unwrap_or_else(|_| "10p-3.101.043".into());
    let script = root.join("tools/firmware_symbols.py");
    println!("cargo:rerun-if-changed={}", script.display());
    let status = std::process::Command::new(std::env::var("PYTHON").unwrap_or_else(|_| "python3".into()))
        .arg(&script).arg("--repo").arg(&root).arg("check").arg("--target").arg(&target)
        .status().expect("cannot run firmware symbol validation");
    assert!(status.success(), "firmware symbols/ABI are not ready; refusing to compile");
    let symbols = root.join("symbol").join(&target);
    for name in ["target.json", "reviewed.json", "manifest.json", "symbols.json", "firmware.rs", "firmware.h", "firmware.ld"] {
        println!("cargo:rerun-if-changed={}", symbols.join(name).display());
    }
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::copy(symbols.join("firmware.rs"), out_dir.join("firmware.rs"))
        .expect("cannot generate selected firmware bindings");
    // 版本串由构建脚本注入(build.py 的 CHAOS_VERSION_TEXT, 形如 1.2.7-d);
    // 直接用 cargo build 时退回 Cargo.toml 里的版本, 不会编译失败。
    let version = std::env::var("CHAOS_VERSION_TEXT")
        .or_else(|_| std::env::var("CARGO_PKG_VERSION"))
        .unwrap_or_else(|_| "0.0.0".into());
    std::fs::write(out_dir.join("version.rs"),
        format!("pub const VERSION_TEXT: &str = {:?};\n", version))
        .expect("cannot generate version.rs");
}
