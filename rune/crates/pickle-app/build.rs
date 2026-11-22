fn main() {
    build_metadata();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // A dependency's rustc-link-arg does not propagate into the final executable/test.
        // The supported macOS installation provides the Swift concurrency runtime here.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=../../packaging/assets/Pickle.ico");
        println!("cargo:rerun-if-changed=../../packaging/windows/app.manifest");
        println!("cargo:rerun-if-env-changed=PICKLE_WINDOWS_APPLICATION_MANIFEST");
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../packaging/assets/Pickle.ico");
        let manifest = std::env::var_os("PICKLE_WINDOWS_APPLICATION_MANIFEST")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "../../packaging/windows/app.manifest".into());
        println!("cargo:rerun-if-changed={}", manifest.display());
        resource.set_manifest_file(
            manifest
                .to_str()
                .expect("Windows application manifest path must be Unicode"),
        );
        resource.set("ProductName", "Pickle Rune");
        resource.set("FileDescription", "Pickle reading assistant");
        resource
            .compile()
            .expect("Could not compile the Windows application icon/manifest");
    }
}

fn build_metadata() {
    println!("cargo:rerun-if-env-changed=PICKLE_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    let target = std::env::var("TARGET").expect("Cargo build target");
    assert!(
        target.len() <= 80
            && target
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    println!("cargo:rustc-env=PICKLE_BUILD_TARGET={target}");
    let workspace = std::fs::read_to_string("../../Cargo.toml").expect("workspace manifest");
    let revision = workspace
        .lines()
        .find(|line| line.trim_start().starts_with("rune = "))
        .and_then(|line| line.split("rev = \"").nth(1))
        .and_then(|revision| revision.split('"').next())
        .filter(|revision| revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()))
        .expect("pinned Rune revision");
    println!("cargo:rustc-env=PICKLE_RUNE_REVISION={revision}");
    // A release pipeline may provide an exact source commit. Do not guess that
    // a developer's HEAD identifies uncommitted app code or expose other env data.
    let commit = std::env::var("PICKLE_BUILD_COMMIT")
        .or_else(|_| std::env::var("GITHUB_SHA"))
        .ok()
        .filter(|value| {
            matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
        });
    println!(
        "cargo:rustc-env=PICKLE_BUILD_COMMIT={}",
        commit.unwrap_or_default()
    );
}
