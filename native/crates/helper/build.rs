fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let directory = std::env::var("DEP_CU_MACOS_BRIDGE_BRIDGE_DIR")
        .expect("macOS platform crate must provide the compiled Swift bridge directory");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{directory}");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
}
