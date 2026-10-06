use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn hash(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|_| format!("READ_FAILED:{}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn main() -> Result<(), String> {
    let repo = std::env::current_dir().map_err(|_| "CWD_FAILED".to_string())?;
    let wake_root = catdesk_wake::runtime::default_root()?;
    let host = repo.join("wake/target/release/CatDeskWakeHost.exe");
    let gui = repo.join("wake/target/catdesk-gui/release/catdesk.exe");
    let adapter = repo.join("wake/adapter.py");
    let bridge = repo.join("scripts/wake_bridge.py");
    for p in [&host, &gui, &adapter, &bridge] {
        if !p.is_file() {
            return Err(format!("ARTIFACT_MISSING:{}", p.display()));
        }
    }
    let host_hash = hash(&host)?;
    let gui_hash = hash(&gui)?;
    let id = format!("1.0.0-dev.15-{}-{}", &host_hash[..12], &gui_hash[..12]);
    let dir = wake_root.join("versions").join(&id);
    let inputs: [(&str, PathBuf); 4] = [
        ("CatDeskWakeHost.exe", host),
        ("CatDeskBinagotchy.exe", gui),
        ("adapter.py", adapter),
        ("wake_bridge.py", bridge),
    ];
    if dir.exists() {
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(dir.join("manifest.json"))
                .map_err(|_| "EXISTING_MANIFEST_MISSING".to_string())?,
        )
        .map_err(|_| "EXISTING_MANIFEST_INVALID".to_string())?;
        if manifest.get("version").and_then(|v| v.as_str()) != Some("1.0.0-dev.15") {
            return Err("EXISTING_VERSION_MISMATCH".into());
        }
        for (name, source) in &inputs {
            let source_hash = hash(source)?;
            let installed_hash = hash(&dir.join(name))?;
            let manifest_hash = manifest
                .get("artifacts")
                .and_then(|a| a.get(name))
                .and_then(|v| v.as_str())
                .ok_or("EXISTING_MANIFEST_INVALID")?;
            if source_hash != installed_hash || installed_hash != manifest_hash {
                return Err(format!("EXISTING_HASH_MISMATCH:{name}"));
            }
        }
        println!("MATERIALIZED={}", dir.display());
        return Ok(());
    }
    fs::create_dir_all(&dir).map_err(|_| "CREATE_INSTALL_DIR_FAILED".to_string())?;
    let mut artifacts = serde_json::Map::new();
    for (name, source) in &inputs {
        let dest = dir.join(name);
        fs::copy(source, &dest).map_err(|_| format!("COPY_FAILED:{name}"))?;
        let source_hash = hash(source)?;
        let dest_hash = hash(&dest)?;
        if source_hash != dest_hash {
            return Err(format!("COPY_HASH_MISMATCH:{name}"));
        }
        artifacts.insert((*name).to_string(), serde_json::Value::String(dest_hash));
    }
    let manifest = serde_json::json!({
        "schemaVersion":1,
        "version":"1.0.0-dev.15",
        "protocolVersion":1,
        "installedUtc": catdesk_wake::store::now(),
        "artifacts": artifacts,
        "acceptance":"DEVELOPMENT_NOT_ACTIVATED"
    });
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|_| "MANIFEST_SERIALIZE_FAILED".to_string())?,
    )
    .map_err(|_| "MANIFEST_WRITE_FAILED".to_string())?;
    println!("MATERIALIZED={}", dir.display());
    Ok(())
}
