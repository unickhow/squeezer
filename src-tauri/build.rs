use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=video-helper.swift");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
        let helper = out_dir.join("squeezer-video-helper");
        let module_cache = out_dir.join("swift-module-cache");
        fs::create_dir_all(&module_cache).expect("create Swift module cache");
        let status = Command::new("swiftc").env("CLANG_MODULE_CACHE_PATH", module_cache)
            .args(["-swift-version", "5", "-parse-as-library", "-O", "video-helper.swift", "-o"]).arg(&helper).status()
            .expect("Swift compiler is required to build the macOS video helper");
        assert!(status.success(), "failed to build native video helper");
        let resource_dir = PathBuf::from("resources");
        fs::create_dir_all(&resource_dir).expect("create bundled resources");
        let bundled_helper = resource_dir.join("squeezer-video-helper");
        let helper_contents = fs::read(&helper).expect("read compiled video helper");
        let needs_copy = fs::read(&bundled_helper).map(|current| current != helper_contents).unwrap_or(true);
        if needs_copy { fs::write(bundled_helper, helper_contents).expect("copy video helper for Tauri bundling"); }
    }
    tauri_build::build()
}
