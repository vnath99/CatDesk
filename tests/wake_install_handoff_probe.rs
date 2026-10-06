#[test]
fn read_wake_install_handoff_state() {
    let local = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA");
    let root = std::path::PathBuf::from(local).join("CatDeskWake");
    for name in [
        "reviewed-install-handoff.json",
        "current.json",
        "control.json",
        "previous.json",
    ] {
        let path = root.join(name);
        let value = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .unwrap_or(serde_json::Value::Null);
        let bounded = match name {
            "reviewed-install-handoff.json" => serde_json::json!({
                "schemaVersion": value.get("schemaVersion"),
                "candidateDirectory": value.get("candidateDirectory"),
                "priorDesired": value.get("priorDesired")
            }),
            "current.json" | "previous.json" => serde_json::json!({
                "schemaVersion": value.get("schemaVersion"),
                "directory": value.get("directory")
            }),
            "control.json" => serde_json::json!({
                "schemaVersion": value.get("schemaVersion"),
                "desired": value.get("desired")
            }),
            _ => serde_json::Value::Null,
        };
        println!("{name}={bounded}");
    }
    for dir in [
        "1.0.0-dev.81-1fa8104979d4-1c6bcdfcedf0",
        "1.0.0-dev.82-b780d33fea72-1c6bcdfcedf0",
    ] {
        let d = root.join("versions").join(dir);
        let manifest = std::fs::read(d.join("manifest.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .unwrap_or(serde_json::Value::Null);
        println!(
            "candidate={} exists={} version={:?} acceptance={:?}",
            dir,
            d.is_dir(),
            manifest.get("version"),
            manifest.get("acceptance")
        );
    }
}
