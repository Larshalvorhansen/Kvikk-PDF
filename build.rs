fn main() {
    println!("cargo:rerun-if-changed=src/macos_bridge.m");

    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() == Some("macos") {
        cc::Build::new()
            .file("src/macos_bridge.m")
            .flag("-fobjc-arc")
            .compile("kvikk_macos_bridge");
        println!("cargo:rustc-link-lib=framework=AppKit");
    }
}
