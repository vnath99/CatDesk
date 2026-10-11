//! T0478 R3B4-B3A: stage exact bounded candidate Git-object bytes
//! into an already-owned, handle-pinned disposable fixture directory.
//!
//! This is *not* an MCP tool or a production release operation. The separate
//! trusted runner must create/own the temporary root, obtain a live GitHub
//! witness, and hold the root lease before this module can be activated.

use std::path::Path;

use uuid::Uuid;

use crate::isolated_fixture_manifest::{
    MAX_FILE_BYTES, MAX_TOTAL_BYTES, SnapshotManifest, validate_manifest, verify_file_bytes,
};
use crate::windows_protected_fs::{
    ProtectedDirectoryGuard, read_relative_regular_unique, write_new_regular_in,
};

const LABEL: &str = "isolated fixture source snapshot";

fn verify_disposable_root(root: &ProtectedDirectoryGuard) -> Result<(), String> {
    root.assert_stable(LABEL)?;
    let temp = std::fs::canonicalize(std::env::temp_dir())
        .map_err(|_| "isolated fixture temp root unavailable")?;
    let path = root.path();
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return Err("isolated fixture root name invalid".into());
    };
    let Some(id) = name.strip_prefix("catdesk-dev-fixture-") else {
        return Err("isolated fixture root outside reviewed namespace".into());
    };
    if path.parent() != Some(temp.as_path())
        || id.len() != 32
        || Uuid::parse_str(id).is_err()
        || Uuid::parse_str(id)
            .map(|v| v.simple().to_string())
            .ok()
            .as_deref()
            != Some(id)
    {
        return Err("isolated fixture root outside reviewed scope".into());
    }
    Ok(())
}

/// Validate all content in memory *before* any file is created. Existing
/// files are never overwritten, and only fixed manifest leaf names are used.
fn preflight_candidates<'a>(
    manifest: &SnapshotManifest,
    candidates: &'a [(String, Vec<u8>)],
) -> Result<Vec<(&'a [u8], String)>, String> {
    validate_manifest(manifest)?;
    if candidates.len() != manifest.files.len() {
        return Err("isolated fixture snapshot candidate count mismatch".into());
    }
    let mut entries = Vec::with_capacity(candidates.len());
    let mut total_bytes = 0u64;
    for file in &manifest.files {
        let matching = candidates
            .iter()
            .filter(|(source_path, _)| *source_path == file.source_path)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err("isolated fixture snapshot source mismatch".into());
        }
        let bytes = matching[0].1.as_slice();
        verify_file_bytes(file, bytes)?;
        total_bytes = total_bytes
            .checked_add(bytes.len() as u64)
            .ok_or("isolated fixture snapshot aggregate overflow")?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err("isolated fixture snapshot aggregate too large".into());
        }
        entries.push((bytes, file.fixture_leaf.clone()));
    }
    Ok(entries)
}

/// Writes only the reviewed manifest leaves under one pinned *disposable*
/// root, then rereads each leaf through a single no-follow, unique-link handle
/// and recomputes SHA256. Caller retains ownership and cleanup responsibility
/// for the exact newly-created root on any error. Never call this on production
/// or a mutable real workspace. This module does not authorize caller blobs.
pub(crate) fn stage_candidate_into_owned_root(
    root: &ProtectedDirectoryGuard,
    manifest: &SnapshotManifest,
    candidates: &[(String, Vec<u8>)],
) -> Result<(), String> {
    verify_disposable_root(root)?;
    let prepared = preflight_candidates(manifest, candidates)?;
    for (bytes, leaf) in &prepared {
        root.assert_stable(LABEL)?;
        write_new_regular_in(root, leaf, bytes, LABEL)?;
        let read_back = read_relative_regular_unique(root, leaf, MAX_FILE_BYTES, LABEL)?;
        let expected = manifest
            .files
            .iter()
            .find(|file| file.fixture_leaf == *leaf)
            .ok_or("isolated fixture snapshot manifest entry disappeared")?;
        verify_file_bytes(expected, &read_back)?;
    }
    root.assert_stable(LABEL)
}

/// Result of a reviewed-source candidate staging operation. The pinned root
/// handle is retained by the caller. This is NOT runtime MCP authority.
#[allow(dead_code)]
pub(crate) struct StagedCandidateSnapshot {
    pub(crate) root: ProtectedDirectoryGuard,
    pub(crate) manifest: SnapshotManifest,
}

/// Dormant reviewed-source path. It uses only the opaque live HTTPS GitHub
/// witness, the existing fixed Git object reader and protected relative writer.
/// It does NOT expose data to the MCP router, start processes or auto-clean
/// anything that the caller may not own. Failure leaves an inactive
/// random-named disposable root for a later *reviewed owner* cleanup operation.
#[allow(dead_code)]
pub(crate) async fn stage_witnessed_candidate_snapshot(
    workspace: &Path,
    expected_commit: &str,
) -> Result<StagedCandidateSnapshot, String> {
    let witness =
        crate::isolated_fixture_github_authority::fetch_live_witness(expected_commit).await?;
    let snapshot_id = Uuid::new_v4().to_string();
    let (manifest, candidates) =
        crate::isolated_fixture_git_objects::read_witnessed_candidate_git_objects(
            workspace,
            &witness,
            &snapshot_id,
        )
        .await?;
    if manifest.source_commit != witness.commit_sha() {
        return Err("isolated fixture source witness mismatch".into());
    }
    let temp = std::fs::canonicalize(std::env::temp_dir())
        .map_err(|_| "isolated fixture temp unavailable")?;
    // Atomically create exactly one private, generated temp root relative to
    // an already opened parent. No existing directory is adopted.
    let mut root = ProtectedDirectoryGuard::acquire(&temp, LABEL)?;
    let child_name = format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple());
    root.create_child(&child_name, LABEL)?;
    stage_candidate_into_owned_root(&root, &manifest, &candidates)?;
    Ok(StagedCandidateSnapshot { root, manifest })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isolated_fixture_manifest::{SOURCE_REPOSITORY, SnapshotFile};
    use sha2::{Digest, Sha256};

    #[test]
    fn staged_bytes_roundtrip_only_inside_test_owned_root() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = temp.join(format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).expect("create only test-owned temp fixture root");
        let root = ProtectedDirectoryGuard::acquire(&dir, LABEL).expect("pin own temp root");
        let blob = b"source tracked bytes".to_vec();
        let manifest = SnapshotManifest {
            schema_version: 1,
            source_repository: SOURCE_REPOSITORY.into(),
            source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            snapshot_id: Uuid::new_v4().to_string(),
            files: vec![SnapshotFile {
                fixture_leaf: "Cargo.toml".into(),
                source_path: "Cargo.toml".into(),
                size_bytes: blob.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&blob)),
            }],
        };
        let candidates = vec![("Cargo.toml".into(), blob.clone())];
        stage_candidate_into_owned_root(&root, &manifest, &candidates)
            .expect("stage fixed candidate file");
        assert_eq!(
            read_relative_regular_unique(&root, "Cargo.toml", MAX_FILE_BYTES, LABEL).unwrap(),
            blob
        );
        // Existing destinations must refuse overwrite rather than mutate the file.
        assert!(stage_candidate_into_owned_root(&root, &manifest, &candidates).is_err());
        let changed = vec![("Cargo.toml".into(), b"tampered bytes".to_vec())];
        assert!(stage_candidate_into_owned_root(&root, &manifest, &changed).is_err());
        assert_eq!(
            read_relative_regular_unique(&root, "Cargo.toml", MAX_FILE_BYTES, LABEL).unwrap(),
            blob
        );
        let extra = vec![
            ("Cargo.toml".into(), blob),
            ("secret.txt".into(), b"secret".to_vec()),
        ];
        assert!(stage_candidate_into_owned_root(&root, &manifest, &extra).is_err());
        assert!(!dir.join("secret.txt").exists());
        drop(root);
        std::fs::remove_dir_all(dir).expect("cleanup only own released fixture root");
    }

    #[test]
    fn preexisting_hardlink_cannot_replace_external_test_file() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = temp.join(format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).unwrap();
        let external = temp.join(format!("catdesk-external-blob-{}", Uuid::new_v4().simple()));
        std::fs::write(&external, b"external unchanged").unwrap();
        std::fs::hard_link(&external, dir.join("Cargo.toml")).unwrap();

        let bytes = b"reviewed content".to_vec();
        let manifest = SnapshotManifest {
            schema_version: 1,
            source_repository: SOURCE_REPOSITORY.into(),
            source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            snapshot_id: Uuid::new_v4().to_string(),
            files: vec![SnapshotFile {
                fixture_leaf: "Cargo.toml".into(),
                source_path: "Cargo.toml".into(),
                size_bytes: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            }],
        };
        let root = ProtectedDirectoryGuard::acquire(&dir, LABEL).unwrap();
        assert!(
            stage_candidate_into_owned_root(&root, &manifest, &[("Cargo.toml".into(), bytes)])
                .is_err(),
            "an existing hardlink must never be overwritten"
        );
        assert_eq!(std::fs::read(&external).unwrap(), b"external unchanged");
        drop(root);
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_file(external).unwrap();
    }

    #[test]
    fn refuses_ordinary_temp_dirs_and_unapproved_names() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let root = ProtectedDirectoryGuard::acquire(&temp, LABEL).unwrap();
        assert!(verify_disposable_root(&root).is_err());
        let dir = temp.join(format!("fixture-not-owned-{}", Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).unwrap();
        let other = ProtectedDirectoryGuard::acquire(&dir, LABEL).unwrap();
        assert!(verify_disposable_root(&other).is_err());
        drop(other);
        drop(root);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
