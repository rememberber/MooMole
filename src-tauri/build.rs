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
    tauri_build::build()
}
