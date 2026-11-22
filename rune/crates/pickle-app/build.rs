fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // A dependency's rustc-link-arg does not propagate into the final executable/test.
        // The supported macOS installation provides the Swift concurrency runtime here.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=../../packaging/assets/Pickle.ico");
        println!("cargo:rerun-if-changed=../../packaging/windows/app.manifest");
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../packaging/assets/Pickle.ico");
        resource.set_manifest_file("../../packaging/windows/app.manifest");
        resource.set("ProductName", "Pickle Rune");
        resource.set("FileDescription", "Pickle reading assistant");
        resource
            .compile()
            .expect("Could not compile the Windows application icon/manifest");
    }
}
