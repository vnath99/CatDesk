use catdesk_wake::runtime;
use std::fs;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

fn safe_metadata(path: &Path) -> Result<fs::Metadata, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "PROFILE_METADATA_FAILED")?;
    if meta.file_type().is_symlink() {
        return Err("PROFILE_REPARSE_REFUSED".into());
    }
    #[cfg(windows)]
    if meta.file_attributes() & 0x400 != 0 {
        return Err("PROFILE_REPARSE_REFUSED".into());
    }
    Ok(meta)
}

fn copy_tree(src: &Path, dst: &Path) -> Result<u64, String> {
    let meta = safe_metadata(src)?;
    if !meta.is_dir() {
        return Err("PROFILE_SOURCE_NOT_DIRECTORY".into());
    }
    fs::create_dir(dst).map_err(|_| "PROFILE_DEST_CREATE_FAILED")?;
    let mut files = 0u64;
    for entry in fs::read_dir(src).map_err(|_| "PROFILE_ENUM_FAILED")? {
        let entry = entry.map_err(|_| "PROFILE_ENUM_FAILED")?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if name_text.starts_with("Singleton") {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        let child = safe_metadata(&from)?;
        if child.is_dir() {
            files += copy_tree(&from, &to)?;
        } else if child.is_file() {
            fs::copy(&from, &to).map_err(|_| "PROFILE_COPY_FAILED")?;
            files += 1;
        } else {
            return Err("PROFILE_SPECIAL_FILE_REFUSED".into());
        }
    }
    Ok(files)
}

fn main() -> Result<(), String> {
    let repo = std::env::current_dir().map_err(|_| "CWD_FAILED")?;
    let root = runtime::default_root()?;
    let legacy = repo
        .join(".catdesk")
        .join("wake-bridge")
        .join("browser-profile");
    let current = root.join("browser-profile");
    let backup: PathBuf = root.join("browser-profile.pre-auth-migration-20260917");
    safe_metadata(&legacy)?;
    if backup.exists() {
        return Err("PROFILE_BACKUP_ALREADY_EXISTS".into());
    }
    if current.exists() {
        safe_metadata(&current)?;
        fs::rename(&current, &backup).map_err(|e| {
            format!(
                "PROFILE_BACKUP_RENAME_FAILED kind={:?} os={:?}",
                e.kind(),
                e.raw_os_error()
            )
        })?;
    }
    let temp = root.join("browser-profile.migrating-20260917");
    if temp.exists() {
        return Err("PROFILE_MIGRATION_TEMP_EXISTS".into());
    }
    let files = match copy_tree(&legacy, &temp) {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_dir_all(&temp);
            if backup.exists() && !current.exists() {
                let _ = fs::rename(&backup, &current);
            }
            return Err(error);
        }
    };
    if !temp.join("Local State").is_file() || !temp.join("Default").is_dir() {
        let _ = fs::remove_dir_all(&temp);
        if backup.exists() && !current.exists() {
            let _ = fs::rename(&backup, &current);
        }
        return Err("PROFILE_MIGRATION_READBACK_FAILED".into());
    }
    fs::rename(&temp, &current).map_err(|_| "PROFILE_COMMIT_FAILED")?;
    println!(
        "{{\"migrated\":true,\"files\":{files},\"backupPreserved\":{}}}",
        backup.exists()
    );
    Ok(())
}
