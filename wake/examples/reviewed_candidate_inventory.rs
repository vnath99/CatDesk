use catdesk_wake::{VERSION, runtime};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn sha(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

fn main() -> Result<(), String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("REPO_ROOT_UNAVAILABLE")?
        .to_path_buf();
    let root = runtime::default_root()?;
    let versions = root.join("versions");

    let sources = BTreeMap::from([
        (
            "CatDeskWakeHost.exe",
            repo.join("wake/target/release/CatDeskWakeHost.exe"),
        ),
        (
            "CatDeskBinagotchy.exe",
            repo.join("wake/target/catdesk-gui/release/catdesk.exe"),
        ),
        ("adapter.py", repo.join("wake/adapter.py")),
        ("wake_bridge.py", repo.join("scripts/wake_bridge.py")),
    ]);
    let source_hashes: BTreeMap<_, _> = sources
        .iter()
        .map(|(name, path)| ((*name).to_string(), sha(path)))
        .collect();

    println!("COMPILED_VERSION|{VERSION}");
    for entry in fs::read_dir(&versions).map_err(|_| "VERSIONS_UNAVAILABLE")? {
        let entry = entry.map_err(|_| "VERSIONS_UNAVAILABLE")?;
        if !entry
            .file_type()
            .map_err(|_| "VERSIONS_UNAVAILABLE")?
            .is_dir()
        {
            continue;
        }
        let dir = entry.path();
        let manifest_path = dir.join("manifest.json");
        let Ok(bytes) = fs::read(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if manifest.get("version").and_then(Value::as_str) != Some(VERSION) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let staging = name.starts_with(".staging-");
        let artifacts = manifest.get("artifacts").and_then(Value::as_object);
        let mh = |artifact: &str| {
            artifacts
                .and_then(|m| m.get(artifact))
                .and_then(Value::as_str)
                .unwrap_or("-")
        };
        let host_m = mh("CatDeskWakeHost.exe");
        let gui_m = mh("CatDeskBinagotchy.exe");
        let canonical = if host_m.len() >= 12 && gui_m.len() >= 12 {
            format!("{VERSION}-{}-{}", &host_m[..12], &gui_m[..12])
        } else {
            "INVALID".into()
        };
        println!(
            "CANDIDATE|name={}|staging={}|canonical_name={}|name_matches={}",
            name,
            staging,
            canonical,
            name == canonical
        );
        for artifact in [
            "CatDeskWakeHost.exe",
            "CatDeskBinagotchy.exe",
            "adapter.py",
            "wake_bridge.py",
        ] {
            let actual = sha(&dir.join(artifact)).unwrap_or_else(|| "-".into());
            let manifest_hash = mh(artifact).to_string();
            let source_hash = source_hashes
                .get(artifact)
                .and_then(|v| v.clone())
                .unwrap_or_else(|| "-".into());
            println!(
                "ARTIFACT|candidate={}|name={}|manifest={}|actual={}|manifest_matches_actual={}|matches_source={}",
                name,
                artifact,
                manifest_hash,
                actual,
                manifest_hash.eq_ignore_ascii_case(&actual),
                actual.eq_ignore_ascii_case(&source_hash)
            );
        }
    }
    Ok(())
}
