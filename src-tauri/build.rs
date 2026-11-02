fn main() {
    for path in ["../package.json", "tauri.conf.json", "Cargo.toml"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let version = env!("CARGO_PKG_VERSION");
    for path in ["../package.json", "tauri.conf.json"] {
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("无法读取版本来源"))
                .expect("版本来源不是有效 JSON");
        assert_eq!(
            json["version"].as_str(),
            Some(version),
            "应用版本不一致：{path} 与 Cargo.toml；请运行 npm run version:sync"
        );
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=native/process_state.c");
        cc::Build::new()
            .file("native/process_state.c")
            .compile("moomole_process_state");
        println!("cargo:rerun-if-changed=native/live_photo.m");
        cc::Build::new()
            .file("native/live_photo.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("moomole_live_photo");
        println!("cargo:rerun-if-changed=native/storage_notifications.m");
        cc::Build::new()
            .file("native/storage_notifications.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("moomole_storage_notifications");
        println!("cargo:rerun-if-changed=native/archive_quarantine.m");
        cc::Build::new()
            .file("native/archive_quarantine.m")
            .flag("-fobjc-arc")
            .compile("moomole_archive_quarantine");
        for framework in [
            "CoreServices",
            "UserNotifications",
            "Photos",
            "AppKit",
            "Foundation",
            "CoreFoundation",
            "CoreGraphics",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    tauri_build::build()
}
