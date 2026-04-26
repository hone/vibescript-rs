use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_dir = manifest_dir.parent().unwrap();
    let profile = env::var("PROFILE").expect("PROFILE env var not set");

    let wasm_target_dir = out_dir.join("wasm-build");

    // Build the vibescript-core crate for wasm32-wasip2, matching the host's profile
    let mut args = vec![
        "build",
        "-p",
        "vibescript-core",
        "--target",
        "wasm32-wasip2",
        "--target-dir",
        wasm_target_dir.to_str().unwrap(),
    ];

    if profile == "release" {
        args.push("--release");
    }

    let status = Command::new("cargo")
        .args(&args)
        .current_dir(workspace_dir)
        .status()
        .expect("Failed to run cargo build for vibescript-core");

    if !status.success() {
        panic!(
            "Compilation of vibescript-core failed. Make sure you have the wasm32-wasip2 target installed: rustup target add wasm32-wasip2"
        );
    }

    let wasm_path = wasm_target_dir
        .join("wasm32-wasip2")
        .join(&profile)
        .join("vibescript_core.wasm");

    let dest_path = out_dir.join("vibescript_core.wasm");
    fs::copy(&wasm_path, &dest_path).expect("Failed to copy wasm file to OUT_DIR");

    // Watch for changes in the core crate
    println!("cargo:rerun-if-changed=../vibescript-core/src");
    println!("cargo:rerun-if-changed=../vibescript-core/wit");
    println!("cargo:rerun-if-changed=../vibescript-core/Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
}
