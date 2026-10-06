use catdesk_wake::{PROTOCOL_VERSION, VERSION, runtime, store::Store};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn sha(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|_| "SOURCE_READ_FAILED")?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("REPO_ROOT_FAILED")?
        .to_path_buf();
    let wake = repo.join("wake");
    let sources = [
        (
            "CatDeskWakeHost.exe",
            wake.join("target/release/CatDeskWakeHost.exe"),
        ),
        (
            "CatDeskBinagotchy.exe",
            wake.join("target/catdesk-gui/release/catdesk.exe"),
        ),
        ("adapter.py", wake.join("adapter.py")),
        ("wake_bridge.py", repo.join("scripts/wake_bridge.py")),
    ];
    for (_, path) in &sources {
        if !path.is_file() {
            return Err("PACKAGE_SOURCE_MISSING".into());
        }
    }

    let host_hash = sha(&sources[0].1)?;
    let gui_hash = sha(&sources[1].1)?;
    let id = format!("{}-{}-{}", VERSION, &host_hash[..12], &gui_hash[..12]);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "CLOCK_INVALID")?
        .as_nanos();
    let staging_name = format!(".staging-{}-{}-{}", id, std::process::id(), nonce);
    let staging = root.join("versions").join(&staging_name);
    fs::create_dir_all(&staging).map_err(|_| "PACKAGE_DIRECTORY_CREATE_FAILED")?;

    let result = (|| {
        let mut hashes = BTreeMap::new();
        for (name, source) in &sources {
            let destination = staging.join(name);
            fs::copy(source, &destination).map_err(|_| "PACKAGE_COPY_FAILED")?;
            let expected = sha(source)?;
            let actual = sha(&destination)?;
            if actual != expected {
                return Err("PACKAGE_READBACK_MISMATCH".into());
            }
            hashes.insert((*name).to_string(), actual);
        }
        let manifest = json!({
            "schemaVersion": 1,
            "version": VERSION,
            "protocolVersion": PROTOCOL_VERSION,
            "artifacts": hashes,
            "acceptance": "DEVELOPMENT_NOT_ACTIVATED"
        });
        fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|_| "MANIFEST_SERIALIZATION_FAILED")?,
        )
        .map_err(|_| "MANIFEST_WRITE_FAILED")?;

        let publication = runtime::publish_reviewed_install(&store, &staging_name, &id)?;
        let handoff = runtime::activate_reviewed_install(&store)?;
        println!(
            "{}",
            serde_json::to_string(&json!({
                "version": VERSION,
                "directory": id,
                "hostSha256": host_hash,
                "guiSha256": gui_hash,
                "publication": publication,
                "activation": handoff
            }))
            .map_err(|_| "OUTPUT_SERIALIZATION_FAILED")?
        );
        Ok(())
    })();

    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}
