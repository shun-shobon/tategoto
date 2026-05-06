use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(tategoto_stub_speech)");
    build_swift_bridge();
    tauri_build::build();
}

fn build_swift_bridge() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos")
        || env::var("TATEGOTO_SKIP_SWIFT_BRIDGE").as_deref() == Ok("1")
    {
        println!("cargo:rustc-cfg=tategoto_stub_speech");
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let source = manifest_dir
        .join("swift")
        .join("AppleSpeechTranscriber.swift");
    let library = out_dir.join("libtategoto_apple_speech.a");
    let module_cache = out_dir.join("swift-module-cache");

    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-parse-as-library",
            "-O",
            "-emit-library",
            "-static",
            "-module-name",
            "TategotoAppleSpeech",
            "-target",
            "arm64-apple-macosx26.0",
            "-module-cache-path",
        ])
        .arg(&module_cache)
        .args(["-o"])
        .arg(&library)
        .arg(&source)
        .args([
            "-framework",
            "AVFAudio",
            "-framework",
            "CoreMedia",
            "-framework",
            "Foundation",
            "-framework",
            "Speech",
        ])
        .status()
        .expect("failed to run swiftc for Apple Speech bridge");

    if !status.success() {
        panic!("swiftc failed to build Apple Speech bridge");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=tategoto_apple_speech");
    println!("cargo:rustc-link-lib=framework=AVFAudio");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=Speech");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rerun-if-changed={}", source.display());
}
