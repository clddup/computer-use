fn main() {
    println!("cargo:rerun-if-changed=bridge.swift");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    if std::env::var("HOST").unwrap() != std::env::var("TARGET").unwrap() {
        panic!("Build the macOS Swift bridge on a native macOS host");
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let status = std::process::Command::new("xcrun")
        .args([
            "swiftc",
            "-parse-as-library",
            "-emit-library",
            "-O",
            "-target",
        ])
        .arg(format!(
            "{}-apple-macosx14.0",
            std::env::var("CARGO_CFG_TARGET_ARCH")
                .unwrap()
                .replace("aarch64", "arm64")
        ))
        .args(["bridge.swift", "-module-name", "CuMacosBridge", "-o"])
        .arg(out.join("libcu_macos_bridge.dylib"))
        .args([
            "-Xlinker",
            "-install_name",
            "-Xlinker",
            "@rpath/libcu_macos_bridge.dylib",
        ])
        .status()
        .expect("Xcode command line tools and Swift are required");
    assert!(status.success(), "Swift bridge compilation failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=dylib=cu_macos_bridge");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", out.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
    // Helper build.rs applies the same rpath for dev builds; packaging copies the dylib beside the helper.
    println!("cargo:bridge_dir={}", out.display());
}
