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
    PinnedDirectory, ProtectedDirectoryGuard, RetainedReviewedArtifact,
    read_relative_regular_unique, retain_reviewed_regular_artifact, write_new_regular_in,
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

/// Bind all freshly staged files to independently opened SHA-256 checked,
/// no-write/no-delete Windows read leases. On error the caller must retain
/// ownership of the root until its reviewed cleanup phase; nothing is served.
fn retain_staged_artifacts(
    root: &ProtectedDirectoryGuard,
    manifest: &SnapshotManifest,
) -> Result<Vec<RetainedReviewedArtifact>, String> {
    verify_disposable_root(root)?;
    validate_manifest(manifest)?;
    // The accepted root is already held open with no delete sharing. Opening
    // its same pathname again cannot redirect to a replacement while pinned.
    let pinned = PinnedDirectory::acquire(root.path(), LABEL)?;
    let mut leases = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let retained = retain_reviewed_regular_artifact(
            &pinned,
            &file.fixture_leaf,
            &file.sha256,
            MAX_FILE_BYTES,
            LABEL,
        )?;
        if retained.identity.byte_length != file.size_bytes {
            return Err("isolated fixture retained size mismatch".into());
        }
        leases.push(retained);
    }
    root.assert_stable(LABEL)?;
    Ok(leases)
}

/// Result of a reviewed-source candidate staging operation. The pinned root
/// handle is retained by the caller. This is NOT runtime MCP authority.
#[allow(dead_code)]
pub(crate) struct StagedCandidateSnapshot {
    pub(crate) root: ProtectedDirectoryGuard,
    pub(crate) manifest: SnapshotManifest,
    // Read-only, no-write/no-delete file handles retained alongside the
    // root; the fixture source must not change before the owner retires it.
    _leases: Vec<RetainedReviewedArtifact>,
}

/// Read-only evidence used by a future owner/handle-bound cleanup engine.
// Not a delete token. The in-process owner must separately prove the opened
// root ACL, exact file DELETE handles and single-generation authority.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct CandidateRetirementInventory {
    pub(crate) snapshot_id: String,
    pub(crate) root_object_identity: String,
    pub(crate) files: Vec<(String, String)>,
}

/// Dormant exact manifest-bound read gate for a producer-owned staged root.
/// Ordinary fixture MCP routing must NOT call this by manufacturing a manifest
/// from a request: only the future owner of StagedCandidateSnapshot may use it.
impl StagedCandidateSnapshot {
    pub(crate) fn read_manifest_leaf(&self, leaf: &str) -> Result<Vec<u8>, String> {
        verify_disposable_root(&self.root)?;
        validate_manifest(&self.manifest)?;
        let entry = self
            .manifest
            .files
            .iter()
            .find(|entry| entry.fixture_leaf == leaf)
            .ok_or("isolated fixture leaf is not in witnessed manifest")?;
        let bytes = read_relative_regular_unique(&self.root, leaf, MAX_FILE_BYTES, LABEL)?;
        verify_file_bytes(entry, &bytes)?;
        Ok(bytes)
    }

    /// R3B4-B3B2A: bind a *candidate* retirement audit to exact live file
    /// handles rather than names or digests alone. This never deletes,
    /// releases leases or attests the directory's owner-only ACL.
    #[cfg(windows)]
    pub(crate) fn review_retirement_object_identities(
        &self,
    ) -> Result<CandidateRetirementInventory, String> {
        self.review_retirement_inventory()?;
        if self._leases.len() != self.manifest.files.len() {
            return Err("disposable snapshot retained file count mismatch".into());
        }
        let root =
            crate::windows_protected_fs::handle_information(self.root.last_handle(LABEL)?, LABEL)?;
        let root_object_identity = format!(
            "{:08x}:{:08x}:{:08x}",
            root.volume_serial_number, root.file_index_high, root.file_index_low,
        );
        let mut names = std::collections::BTreeSet::new();
        let mut identities = std::collections::BTreeSet::new();
        let mut files = Vec::with_capacity(self.manifest.files.len());
        for (entry, retained) in self.manifest.files.iter().zip(&self._leases) {
            if retained.identity.sha256 != entry.sha256
                || retained.identity.byte_length != entry.size_bytes
                || !names.insert(&entry.fixture_leaf)
                || !identities.insert(&retained.identity.object_identity)
            {
                return Err("disposable snapshot retained file identity mismatch".into());
            }
            files.push((
                entry.fixture_leaf.clone(),
                retained.identity.object_identity.clone(),
            ));
        }
        self.root.assert_stable(LABEL)?;
        Ok(CandidateRetirementInventory {
            snapshot_id: self.manifest.snapshot_id.clone(),
            root_object_identity,
            files,
        })
    }

    /// R3B4-B3B1 diagnostic only. Before a future handle-authorized retire
    /// operation, reject foreign, renamed, reparse or altered contents. This
    /// NEVER deletes anything and is NOT permission to delete via pathname.
    pub(crate) fn review_retirement_inventory(&self) -> Result<(), String> {
        verify_disposable_root(&self.root)?;
        validate_manifest(&self.manifest)?;
        let directory = std::fs::read_dir(self.root.path())
            .map_err(|_| "disposable snapshot inventory unavailable")?;
        let mut observed = std::collections::BTreeSet::new();
        for item in directory {
            let entry = item.map_err(|_| "disposable snapshot inventory unavailable")?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "disposable snapshot foreign leaf name")?;
            if !self
                .manifest
                .files
                .iter()
                .any(|file| file.fixture_leaf == name)
                || !observed.insert(name)
            {
                return Err("disposable snapshot contains unexpected leaf".into());
            }
            let file_type = entry
                .file_type()
                .map_err(|_| "disposable snapshot inventory changed")?;
            if !file_type.is_file() || file_type.is_symlink() {
                return Err("disposable snapshot contains nonregular leaf".into());
            }
            // Never follow an unreviewed symlink simply to classify it.
            let metadata = std::fs::symlink_metadata(entry.path())
                .map_err(|_| "disposable snapshot inventory changed")?;
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                return Err("disposable snapshot contains nonregular leaf".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("disposable snapshot contains reparse leaf".into());
                }
            }
        }
        if observed.len() != self.manifest.files.len() {
            return Err("disposable snapshot missing expected leaf".into());
        }
        for file in &self.manifest.files {
            self.read_manifest_leaf(&file.fixture_leaf)?;
        }
        self.root.assert_stable(LABEL)?;
        Ok(())
    }
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
    #[cfg(windows)]
    {
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .map_err(|_| "isolated fixture interactive owner identity unavailable")?;
        root.create_owner_only_fixture_child(&child_name, &sid, LABEL)?;
        root.verify_owner_only_fixture_descriptor(&sid, LABEL)?;
    }
    #[cfg(not(windows))]
    return Err("isolated fixture owner-only creation is Windows-only".into());

    stage_candidate_into_owned_root(&root, &manifest, &candidates)?;
    let leases = retain_staged_artifacts(&root, &manifest)?;
    Ok(StagedCandidateSnapshot {
        root,
        manifest,
        _leases: leases,
    })
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
        let leases = retain_staged_artifacts(&root, &manifest).expect("retain reviewed files");
        let staged = StagedCandidateSnapshot {
            root,
            manifest,
            _leases: leases,
        };
        assert_eq!(
            staged.read_manifest_leaf("Cargo.toml").unwrap(),
            b"source tracked bytes"
        );
        staged
            .review_retirement_inventory()
            .expect("intact test-owned fixture has expected inventory");
        #[cfg(windows)]
        {
            let audit = staged
                .review_retirement_object_identities()
                .expect("retained file and root handles bind a candidate inventory");
            assert_eq!(audit.snapshot_id, staged.manifest.snapshot_id);
            assert_eq!(audit.files.len(), 1);
            assert_eq!(audit.files[0].0, "Cargo.toml");
            assert!(!audit.root_object_identity.is_empty());
        }
        std::fs::write(dir.join("unlisted.txt"), b"foreign")
            .expect("inject only into test-owned fixture");
        assert!(
            staged.review_retirement_inventory().is_err(),
            "unexpected new content must block future retirement"
        );
        #[cfg(windows)]
        assert!(
            staged.review_retirement_object_identities().is_err(),
            "unexpected file also blocks retained-identity retirement audit"
        );
        std::fs::create_dir(dir.join("foreign-directory"))
            .expect("inject only into test-owned fixture");
        assert!(
            staged.review_retirement_inventory().is_err(),
            "unexpected nested directory must also block future retirement"
        );
        for disallowed in ["secret.txt", "../Cargo.toml", "main.rs", "config.toml"] {
            assert!(
                staged.read_manifest_leaf(disallowed).is_err(),
                "unlisted source leaf should fail closed"
            );
        }
        assert!(
            std::fs::write(dir.join("Cargo.toml"), b"modified").is_err(),
            "retained source file handle must prevent write"
        );
        drop(staged);
        std::fs::write(dir.join("Cargo.toml"), b"modified")
            .expect("source write allowed only after retired file lease");
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
        // Even a manifest-matching leaf name is not an owned cleanup target
        // when it aliases an outside file through a second hardlink.
        let staged = StagedCandidateSnapshot {
            root,
            manifest,
            _leases: Vec::new(),
        };
        assert!(
            staged.review_retirement_inventory().is_err(),
            "hardlinked external content must block retirement review"
        );
        assert_eq!(std::fs::read(&external).unwrap(), b"external unchanged");
        drop(staged);
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_file(external).unwrap();
    }

    #[test]
    fn retirement_inventory_refuses_incomplete_test_owned_stage() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = temp.join(format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).expect("create own incomplete fixture");
        let snapshot = StagedCandidateSnapshot {
            root: ProtectedDirectoryGuard::acquire(&dir, LABEL).unwrap(),
            manifest: SnapshotManifest {
                schema_version: 1,
                source_repository: SOURCE_REPOSITORY.into(),
                source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
                snapshot_id: Uuid::new_v4().to_string(),
                files: vec![SnapshotFile {
                    fixture_leaf: "Cargo.toml".into(),
                    source_path: "Cargo.toml".into(),
                    size_bytes: 4,
                    sha256: format!("{:x}", Sha256::digest(b"data")),
                }],
            },
            _leases: Vec::new(),
        };
        assert!(
            snapshot.review_retirement_inventory().is_err(),
            "incomplete fixture is not ready for handle-based retirement"
        );
        drop(snapshot);
        std::fs::remove_dir_all(dir).expect("cleanup own empty test-only fixture");
    }

    #[cfg(windows)]
    #[test]
    fn generated_fixture_owner_acl_is_fixed_at_creation() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let mut parent = ProtectedDirectoryGuard::acquire(&temp, LABEL).unwrap();
        let invalid_source = "S-1-5-21-111-222-333-1001";
        assert!(
            parent
                .create_owner_only_fixture_child("other-root", invalid_source, LABEL)
                .is_err()
        );
        assert!(
            parent
                .create_owner_only_fixture_child("catdesk-dev-fixture-bad", invalid_source, LABEL)
                .is_err()
        );
        let name = format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple());
        let interactive =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid();
        let Ok(sid) = interactive else {
            assert!(
                !temp.join(&name).exists(),
                "service/CI identities may not stage"
            );
            return;
        };
        parent
            .create_owner_only_fixture_child(&name, &sid, LABEL)
            .expect("create fresh fixed ACL child for ordinary interactive SID");
        parent.assert_stable(LABEL).unwrap();
        verify_disposable_root(&parent).unwrap();
        parent
            .verify_owner_only_fixture_descriptor(&sid, LABEL)
            .expect("exact opened directory owner and protected DACL");
        assert!(
            parent
                .verify_owner_only_fixture_descriptor("S-1-5-21-123-999", LABEL)
                .is_err(),
            "wrong SID must not match even if the directory name is correct"
        );
        let mut second = ProtectedDirectoryGuard::acquire(&temp, LABEL).unwrap();
        assert!(
            second
                .create_owner_only_fixture_child(&name, &sid, LABEL)
                .is_err(),
            "existing fixture root must never be adopted or overwritten"
        );
        let foreign = temp.join(format!("fixture-not-owned-{}", Uuid::new_v4().simple()));
        std::fs::create_dir(&foreign).unwrap();
        let mut nested = ProtectedDirectoryGuard::acquire(&foreign, LABEL).unwrap();
        assert!(
            nested
                .create_owner_only_fixture_child(
                    &format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple()),
                    &sid,
                    LABEL
                )
                .is_err(),
            "foreign non-temp parent is not authorized"
        );
        drop(nested);
        drop(second);
        drop(parent);
        std::fs::remove_dir(foreign).unwrap();
        std::fs::remove_dir(temp.join(name)).expect("remove only test-owned empty root");
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
