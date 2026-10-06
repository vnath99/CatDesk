use catdesk_wake::runtime;
use std::fs;

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let versions = root.join("versions");
    for entry in fs::read_dir(&versions).map_err(|_| "VERSIONS_UNAVAILABLE")? {
        let entry = entry.map_err(|_| "VERSIONS_UNAVAILABLE")?;
        if !entry
            .file_type()
            .map_err(|_| "VERSIONS_UNAVAILABLE")?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.contains("1.0.0-dev.19") {
            continue;
        }
        let dir = entry.path();
        let manifest = dir.join("manifest.json");
        println!("DIR|{}|manifest={}", name, manifest.is_file());
        for artifact in [
            "CatDeskWakeHost.exe",
            "CatDeskBinagotchy.exe",
            "adapter.py",
            "wake_bridge.py",
        ] {
            let path = dir.join(artifact);
            match fs::metadata(&path) {
                Ok(meta) if meta.is_file() => {
                    println!("FILE|{}|{}|len={}", name, artifact, meta.len())
                }
                _ => println!("FILE|{}|{}|missing", name, artifact),
            }
        }
    }
    Ok(())
}
