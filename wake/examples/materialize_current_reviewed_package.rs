use catdesk_wake::{PROTOCOL_VERSION, VERSION, runtime};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn sha(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|_| "SOURCE_READ_FAILED")?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("REPO_ROOT_FAILED")?
        .to_path_buf();
    let wake = repo.join("wake");
    let host = wake
        .join("target")
        .join("release")
        .join("CatDeskWakeHost.exe");
    let gui = wake
        .join("target")
        .join("catdesk-gui")
        .join("release")
        .join("catdesk.exe");
    let adapter = wake.join("adapter.py");
    let bridge = repo.join("scripts").join("wake_bridge.py");
    for path in [&host, &gui, &adapter, &bridge] {
        if !path.is_file() {
            return Err("PACKAGE_SOURCE_MISSING".into());
        }
    }
    let host_hash = sha(&host)?;
    let gui_hash = sha(&gui)?;
    let id = format!("{}-{}-{}", VERSION, &host_hash[..12], &gui_hash[..12]);
    let dir = root.join("versions").join(&id);
    if dir.exists() {
        return Err("IMMUTABLE_VERSION_ALREADY_EXISTS".into());
    }
    fs::create_dir_all(&dir).map_err(|_| "PACKAGE_DIRECTORY_CREATE_FAILED")?;
    let sources = [
        ("CatDeskWakeHost.exe", host),
        ("CatDeskBinagotchy.exe", gui),
        ("adapter.py", adapter),
        ("wake_bridge.py", bridge),
    ];
    let mut hashes = BTreeMap::new();
    for (name, source) in sources {
        let destination = dir.join(name);
        fs::copy(&source, &destination).map_err(|_| "PACKAGE_COPY_FAILED")?;
        let expected = sha(&source)?;
        let actual = sha(&destination)?;
        if expected != actual {
            return Err("PACKAGE_READBACK_MISMATCH".into());
        }
        hashes.insert(name.to_string(), actual);
    }
    let manifest = json!({
        "schemaVersion": 1,
        "version": VERSION,
        "protocolVersion": PROTOCOL_VERSION,
        "artifacts": hashes,
        "acceptance": "DEVELOPMENT_NOT_ACTIVATED"
    });
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|_| "MANIFEST_SERIALIZATION_FAILED")?,
    )
    .map_err(|_| "MANIFEST_WRITE_FAILED")?;
    println!(
        "{}",
        json!({"directory": id, "version": VERSION, "hostSha256": host_hash, "guiSha256": gui_hash})
    );
    Ok(())
}
