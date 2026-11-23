fn main() {
    println!("cargo:rerun-if-changed=../../native/macos/PickleBridge.swift");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        other => panic!("Unsupported macOS architecture: {other:?}"),
    };
    let target = format!("{arch}-apple-macosx14.0");
    let status = std::process::Command::new("xcrun")
        .args([
            "swiftc",
            "-parse-as-library",
            "-emit-library",
            "-static",
            "-O",
            "-target",
            &target,
        ])
        .arg("../../native/macos/PickleBridge.swift")
        .arg("../../native/macos/PickleBrowserTarget.swift")
        .arg("../../native/macos/PickleBrowserPage.swift")
        .arg("../../native/macos/PickleLibrarySearch.swift")
        .arg("../../native/macos/PickleAudio.swift")
        .arg("../../native/macos/PickleInvocation.swift")
        .arg("../../native/macos/PickleWindow.swift")
        .arg("../../native/macos/PickleLaunch.swift")
        .arg("-o")
        .arg(out.join("libPickleBridge.a"))
        .status()
        .expect("Apple Swift compiler is required for the macOS bridge");
    assert!(status.success(), "Could not build the macOS bridge");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=PickleBridge");
    // Swift's toolchain runtime directory is needed by the native linker. The OS supplies
    // the supported Swift runtime at execution; verify deployment in packaged-app tests.
    let info = std::process::Command::new("xcrun")
        .args(["swiftc", "-print-target-info", "-target", &target])
        .output()
        .expect("Swift target info");
    let text = String::from_utf8_lossy(&info.stdout);
    // Avoid a JSON build dependency: the SDK's compiler path gives the corresponding lib dir.
    let compiler = std::process::Command::new("xcrun")
        .args(["--find", "swiftc"])
        .output()
        .unwrap();
    let path = std::path::PathBuf::from(String::from_utf8_lossy(&compiler.stdout).trim());
    if let Some(bin) = path.parent() {
        println!(
            "cargo:rustc-link-search=native={}",
            bin.join("../lib/swift/macosx").display()
        );
    }
    let _ = text;
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rerun-if-changed=../../native/macos/PickleAudio.swift");
    println!("cargo:rerun-if-changed=../../native/macos/PickleBrowserTarget.swift");
    println!("cargo:rerun-if-changed=../../native/macos/PickleBrowserPage.swift");
    println!("cargo:rerun-if-changed=../../native/macos/PickleLibrarySearch.swift");
    println!("cargo:rerun-if-changed=../../native/macos/PickleInvocation.swift");
    println!("cargo:rerun-if-changed=../../native/macos/PickleWindow.swift");
    println!("cargo:rerun-if-changed=../../native/macos/PickleLaunch.swift");
    for framework in [
        "AppKit",
        "ApplicationServices",
        "Security",
        "LocalAuthentication",
        "ScreenCaptureKit",
        "Vision",
        "Speech",
        "AVFoundation",
        "Carbon",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
