//! T0478 R3B4-B3A: stage exact bounded candidate Git-object bytes
//! into an already-owned, handle-pinned disposable fixture directory.
//!
//! This is *not* an MCP tool or a production release operation. The separate
//! trusted runner must create/own the temporary root, obtain a live GitHub
//! witness, and hold the root lease before this module can be activated.

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::Path;

#[cfg(windows)]
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sha2::Digest;
use uuid::Uuid;

use crate::isolated_fixture_manifest::{
    MAX_FILE_BYTES, MAX_TOTAL_BYTES, SnapshotManifest, validate_manifest, verify_file_bytes,
};
use crate::reviewed_source_snapshot::query_relative_directory_names;
use crate::windows_protected_fs::{
    PinnedDirectory, ProtectedDirectoryGuard, RetainedReviewedArtifact,
    read_optional_relative_regular, read_relative_regular_unique, retain_reviewed_regular_artifact,
    write_new_regular_in,
};
#[cfg(windows)]
use crate::windows_protected_fs::{dispose_opened_object, open_reviewed_regular_for_delete};

const LABEL: &str = "isolated fixture source snapshot";
#[cfg(windows)]
const MAX_OWNER_RECORD_BYTES: u64 = 4 * 1024;

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FixtureOwnerIntentV1 {
    schema_version: u32,
    snapshot_id: String,
    source_commit: String,
    manifest_sha256: String,
    root_name: String,
}

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FixtureOwnerRootV1 {
    schema_version: u32,
    snapshot_id: String,
    intent_sha256: String,
    root_object_identity: String,
}

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FixtureOwnerAttentionV1 {
    schema_version: u32,
    snapshot_id: String,
    intent_sha256: String,
    root_object_identity: Option<String>,
    code: String,
}

#[cfg(windows)]
fn record_fixture_owner_attention(
    journal: &ProtectedDirectoryGuard,
    intent: &FixtureOwnerIntentV1,
    intent_bytes: &[u8],
    root_binding: Option<&FixtureOwnerRootV1>,
    code: &'static str,
    sid: &str,
) -> Result<(), String> {
    if !matches!(
        code,
        "parent_unavailable"
            | "creation_indeterminate"
            | "root_binding_unavailable"
            | "partial_stage"
    ) || (code == "partial_stage") != root_binding.is_some()
    {
        return Err("fixture owner attention code invalid".into());
    }
    journal.verify_owner_only_fixture_journal_descriptor(sid, LABEL)?;
    if read_relative_regular_unique(journal, "intent.json", MAX_OWNER_RECORD_BYTES, LABEL)?
        != intent_bytes
    {
        return Err("fixture owner attention intent drifted".into());
    }
    if root_binding.is_some_and(|binding| {
        binding.snapshot_id != intent.snapshot_id
            || binding.intent_sha256 != format!("{:x}", sha2::Sha256::digest(intent_bytes))
    }) {
        return Err("fixture owner attention root binding drifted".into());
    }
    let attention = FixtureOwnerAttentionV1 {
        schema_version: 1,
        snapshot_id: intent.snapshot_id.clone(),
        intent_sha256: format!("{:x}", sha2::Sha256::digest(intent_bytes)),
        root_object_identity: root_binding.map(|binding| binding.root_object_identity.clone()),
        code: code.into(),
    };
    let bytes = serde_json::to_vec(&attention)
        .map_err(|_| "fixture owner attention encoding unavailable")?;
    if bytes.len() as u64 > MAX_OWNER_RECORD_BYTES {
        return Err("fixture owner attention too large".into());
    }
    write_new_regular_in(journal, "attention.json", &bytes, LABEL)?;
    if read_relative_regular_unique(journal, "attention.json", MAX_OWNER_RECORD_BYTES, LABEL)?
        != bytes
    {
        return Err("fixture owner attention readback drifted".into());
    }
    Ok(())
}

#[cfg(windows)]
fn read_fixture_owner_attention(
    journal: &ProtectedDirectoryGuard,
    intent: &FixtureOwnerIntentV1,
    intent_bytes: &[u8],
    binding: Option<&FixtureOwnerRootV1>,
    sid: &str,
) -> Result<Option<FixtureOwnerAttentionV1>, String> {
    journal.verify_owner_only_fixture_journal_descriptor(sid, LABEL)?;
    let uuid = Uuid::parse_str(&intent.snapshot_id)
        .map_err(|_| "fixture owner attention snapshot invalid")?;
    if journal.path().file_name().and_then(|name| name.to_str())
        != Some(format!("catdesk-fixture-journal-{}", uuid.simple()).as_str())
        || read_relative_regular_unique(journal, "intent.json", MAX_OWNER_RECORD_BYTES, LABEL)?
            != intent_bytes
    {
        return Err("fixture owner attention journal drifted".into());
    }
    if binding.is_some_and(|binding| {
        binding.snapshot_id != intent.snapshot_id
            || binding.intent_sha256 != format!("{:x}", sha2::Sha256::digest(intent_bytes))
    }) {
        return Err("fixture owner attention root binding drifted".into());
    }
    let Some(bytes) =
        read_optional_relative_regular(journal, "attention.json", MAX_OWNER_RECORD_BYTES, LABEL)?
    else {
        return Ok(None);
    };
    if bytes
        != read_relative_regular_unique(journal, "attention.json", MAX_OWNER_RECORD_BYTES, LABEL)?
    {
        return Err("fixture owner attention changed".into());
    }
    let attention: FixtureOwnerAttentionV1 =
        serde_json::from_slice(&bytes).map_err(|_| "fixture owner attention invalid")?;
    if bytes
        != serde_json::to_vec(&attention)
            .map_err(|_| "fixture owner attention encoding unavailable")?
        || attention.schema_version != 1
        || attention.snapshot_id != intent.snapshot_id
        || attention.intent_sha256 != format!("{:x}", sha2::Sha256::digest(intent_bytes))
        || attention.root_object_identity
            != binding.map(|binding| binding.root_object_identity.clone())
        || !matches!(
            attention.code.as_str(),
            "parent_unavailable"
                | "creation_indeterminate"
                | "root_binding_unavailable"
                | "partial_stage"
        )
        || (attention.code == "partial_stage") != binding.is_some()
    {
        return Err("fixture owner attention drifted".into());
    }
    Ok(Some(attention))
}

/// Create the owner-only write-ahead intent before a candidate root exists.
/// These records are forensic evidence only; they never authorize a path scan
/// or automatic cleanup after a crash.
#[cfg(windows)]
fn create_fixture_owner_intent(
    manifest: &SnapshotManifest,
    sid: &str,
) -> Result<(ProtectedDirectoryGuard, FixtureOwnerIntentV1, Vec<u8>), String> {
    validate_manifest(manifest)?;
    let uuid =
        Uuid::parse_str(&manifest.snapshot_id).map_err(|_| "fixture owner snapshot id invalid")?;
    let journal_name = format!("catdesk-fixture-journal-{}", uuid.simple());
    let root_name = format!("catdesk-dev-fixture-{}", uuid.simple());
    let manifest_bytes =
        serde_json::to_vec(manifest).map_err(|_| "fixture owner manifest unavailable")?;
    let intent = FixtureOwnerIntentV1 {
        schema_version: 1,
        snapshot_id: manifest.snapshot_id.clone(),
        source_commit: manifest.source_commit.clone(),
        manifest_sha256: format!("{:x}", sha2::Sha256::digest(&manifest_bytes)),
        root_name,
    };
    let bytes =
        serde_json::to_vec(&intent).map_err(|_| "fixture owner intent encoding unavailable")?;
    if bytes.len() as u64 > MAX_OWNER_RECORD_BYTES {
        return Err("fixture owner intent too large".into());
    }
    let temp = std::fs::canonicalize(std::env::temp_dir())
        .map_err(|_| "fixture owner temp unavailable")?;
    let mut journal = ProtectedDirectoryGuard::acquire(&temp, LABEL)?;
    journal.create_owner_only_fixture_journal_child(&journal_name, sid, LABEL)?;
    journal.verify_owner_only_fixture_journal_descriptor(sid, LABEL)?;
    write_new_regular_in(&journal, "intent.json", &bytes, LABEL)?;
    if read_relative_regular_unique(&journal, "intent.json", MAX_OWNER_RECORD_BYTES, LABEL)?
        != bytes
    {
        return Err("fixture owner intent readback drifted".into());
    }
    Ok((journal, intent, bytes))
}

#[cfg(windows)]
fn bind_fixture_owner_root(
    journal: &ProtectedDirectoryGuard,
    intent: &FixtureOwnerIntentV1,
    intent_bytes: &[u8],
    root: &ProtectedDirectoryGuard,
    sid: &str,
) -> Result<FixtureOwnerRootV1, String> {
    journal.verify_owner_only_fixture_journal_descriptor(sid, LABEL)?;
    root.verify_owner_only_fixture_descriptor(sid, LABEL)?;
    if root.path().file_name().and_then(|name| name.to_str()) != Some(intent.root_name.as_str())
        || read_relative_regular_unique(journal, "intent.json", MAX_OWNER_RECORD_BYTES, LABEL)?
            != intent_bytes
    {
        return Err("fixture owner intent or root drifted".into());
    }
    let information =
        crate::windows_protected_fs::handle_information(root.last_handle(LABEL)?, LABEL)?;
    let binding = FixtureOwnerRootV1 {
        schema_version: 1,
        snapshot_id: intent.snapshot_id.clone(),
        intent_sha256: format!("{:x}", sha2::Sha256::digest(intent_bytes)),
        root_object_identity: format!(
            "{:08x}:{:08x}:{:08x}",
            information.volume_serial_number,
            information.file_index_high,
            information.file_index_low
        ),
    };
    let bytes =
        serde_json::to_vec(&binding).map_err(|_| "fixture owner root encoding unavailable")?;
    if bytes.len() as u64 > MAX_OWNER_RECORD_BYTES {
        return Err("fixture owner root record too large".into());
    }
    write_new_regular_in(journal, "root.json", &bytes, LABEL)?;
    if read_relative_regular_unique(journal, "root.json", MAX_OWNER_RECORD_BYTES, LABEL)? != bytes {
        return Err("fixture owner root readback drifted".into());
    }
    Ok(binding)
}

/// Reopen only a caller-pinned journal and verify its canonical records against
/// the witnessed manifest and, when supplied, the already opened root object.
/// A valid record is forensic evidence, never a cleanup token.
#[cfg(windows)]
fn read_fixture_owner_records(
    journal: &ProtectedDirectoryGuard,
    manifest: &SnapshotManifest,
    root: Option<&ProtectedDirectoryGuard>,
    sid: &str,
) -> Result<(FixtureOwnerIntentV1, Option<FixtureOwnerRootV1>), String> {
    validate_manifest(manifest)?;
    journal.verify_owner_only_fixture_journal_descriptor(sid, LABEL)?;
    let uuid =
        Uuid::parse_str(&manifest.snapshot_id).map_err(|_| "fixture owner snapshot id invalid")?;
    let expected_journal = format!("catdesk-fixture-journal-{}", uuid.simple());
    if journal.path().file_name().and_then(|name| name.to_str()) != Some(expected_journal.as_str())
    {
        return Err("fixture owner journal identity drifted".into());
    }
    let intent_bytes =
        read_relative_regular_unique(journal, "intent.json", MAX_OWNER_RECORD_BYTES, LABEL)?;
    let intent: FixtureOwnerIntentV1 =
        serde_json::from_slice(&intent_bytes).map_err(|_| "fixture owner intent invalid")?;
    let canonical =
        serde_json::to_vec(&intent).map_err(|_| "fixture owner intent encoding unavailable")?;
    let manifest_bytes =
        serde_json::to_vec(manifest).map_err(|_| "fixture owner manifest unavailable")?;
    if intent_bytes != canonical
        || intent.schema_version != 1
        || intent.snapshot_id != manifest.snapshot_id
        || intent.source_commit != manifest.source_commit
        || intent.manifest_sha256 != format!("{:x}", sha2::Sha256::digest(&manifest_bytes))
        || intent.root_name != format!("catdesk-dev-fixture-{}", uuid.simple())
    {
        return Err("fixture owner intent identity drifted".into());
    }
    let Some(root_bytes) =
        read_optional_relative_regular(journal, "root.json", MAX_OWNER_RECORD_BYTES, LABEL)?
    else {
        if root.is_some() {
            return Err("fixture owner root binding missing".into());
        }
        return Ok((intent, None));
    };
    if root_bytes
        != read_relative_regular_unique(journal, "root.json", MAX_OWNER_RECORD_BYTES, LABEL)?
    {
        return Err("fixture owner root binding changed".into());
    }
    let binding: FixtureOwnerRootV1 =
        serde_json::from_slice(&root_bytes).map_err(|_| "fixture owner root binding invalid")?;
    if root_bytes
        != serde_json::to_vec(&binding).map_err(|_| "fixture owner root encoding unavailable")?
        || binding.schema_version != 1
        || binding.snapshot_id != manifest.snapshot_id
        || binding.intent_sha256 != format!("{:x}", sha2::Sha256::digest(&intent_bytes))
    {
        return Err("fixture owner root binding drifted".into());
    }
    if let Some(root) = root {
        root.verify_owner_only_fixture_descriptor(sid, LABEL)?;
        if root.path().file_name().and_then(|name| name.to_str()) != Some(intent.root_name.as_str())
        {
            return Err("fixture owner root name drifted".into());
        }
        let info =
            crate::windows_protected_fs::handle_information(root.last_handle(LABEL)?, LABEL)?;
        if binding.root_object_identity
            != format!(
                "{:08x}:{:08x}:{:08x}",
                info.volume_serial_number, info.file_index_high, info.file_index_low
            )
        {
            return Err("fixture owner root object drifted".into());
        }
    }
    Ok((intent, Some(binding)))
}

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

/// A stage that created an exact disposable root but could not finish. The
/// caller keeps its pinned owner handle and manifest; it must record attention
/// and may not infer deletion authority from the generated pathname.
#[allow(dead_code)]
pub(crate) struct OwnedCandidateStageFailure {
    pub(crate) root: ProtectedDirectoryGuard,
    pub(crate) manifest: SnapshotManifest,
    pub(crate) reason: String,
}

/// Create may have materialized a child before a later native/identity check
/// failed. The returned guard may pin only the parent or also the child;
/// the generated name is diagnostic only and grants no retirement authority.
#[allow(dead_code)]
pub(crate) struct IndeterminateCandidateCreation {
    pub(crate) guard: ProtectedDirectoryGuard,
    pub(crate) generated_name: String,
    pub(crate) manifest: SnapshotManifest,
    pub(crate) reason: String,
}

#[allow(dead_code)]
pub(crate) enum CandidateStageError {
    BeforeOwnership(String),
    CreationIndeterminate(Box<IndeterminateCandidateCreation>),
    Owned(Box<OwnedCandidateStageFailure>),
}

fn finish_owned_candidate_stage(
    root: ProtectedDirectoryGuard,
    manifest: SnapshotManifest,
    candidates: &[(String, Vec<u8>)],
) -> Result<StagedCandidateSnapshot, Box<OwnedCandidateStageFailure>> {
    if let Err(reason) = stage_candidate_into_owned_root(&root, &manifest, candidates) {
        return Err(Box::new(OwnedCandidateStageFailure {
            root,
            manifest,
            reason,
        }));
    }
    let leases = match retain_staged_artifacts(&root, &manifest) {
        Ok(leases) => leases,
        Err(reason) => {
            return Err(Box::new(OwnedCandidateStageFailure {
                root,
                manifest,
                reason,
            }));
        }
    };
    Ok(StagedCandidateSnapshot {
        root,
        manifest,
        _leases: leases,
    })
}

#[cfg(windows)]
fn create_owned_candidate_root(
    mut guard: ProtectedDirectoryGuard,
    child_name: String,
    sid: &str,
    manifest: SnapshotManifest,
) -> Result<(ProtectedDirectoryGuard, SnapshotManifest), CandidateStageError> {
    if let Err(reason) = guard.create_owner_only_fixture_child(&child_name, sid, LABEL) {
        return Err(CandidateStageError::CreationIndeterminate(Box::new(
            IndeterminateCandidateCreation {
                guard,
                generated_name: child_name,
                manifest,
                reason,
            },
        )));
    }
    if let Err(reason) = guard.verify_owner_only_fixture_descriptor(sid, LABEL) {
        return Err(CandidateStageError::Owned(Box::new(
            OwnedCandidateStageFailure {
                root: guard,
                manifest,
                reason,
            },
        )));
    }
    Ok((guard, manifest))
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
        let directory = query_relative_directory_names(&self.root)
            .map_err(|_| "disposable snapshot inventory unavailable")?;
        let mut observed = std::collections::BTreeSet::new();
        for (name, attributes) in directory {
            if !self
                .manifest
                .files
                .iter()
                .any(|file| file.fixture_leaf == name)
                || !observed.insert(name)
            {
                return Err("disposable snapshot contains unexpected leaf".into());
            }
            if attributes & (0x10 | 0x400) != 0 {
                return Err("disposable snapshot contains nonregular or reparse leaf".into());
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

    /// Consume one producer-owned snapshot and retire only its exact
    /// manifest files. This remains dormant until the fixture owner lifecycle
    /// records durable failure/attention and owns the entire operation.
    #[cfg(windows)]
    pub(crate) fn retire_owned_root(self) -> Result<(), String> {
        let evidence = self.review_retirement_object_identities()?;
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .map_err(|_| "disposable snapshot owner identity unavailable")?;
        self.root
            .verify_owner_only_fixture_descriptor(&sid, LABEL)?;
        let StagedCandidateSnapshot {
            root,
            manifest,
            _leases,
        } = self;
        let expected = _leases
            .iter()
            .map(|lease| lease.identity.clone())
            .collect::<Vec<_>>();
        drop(_leases);

        // Open and validate every DELETE-capable child handle before the
        // first disposition. An absent, changed, aliased or leased child
        // refuses the whole operation while all manifest files remain.
        let mut opened = Vec::with_capacity(manifest.files.len());
        for (entry, identity) in manifest.files.iter().zip(&expected) {
            opened.push(open_reviewed_regular_for_delete(
                &root,
                &entry.fixture_leaf,
                identity,
                MAX_FILE_BYTES,
                LABEL,
            )?);
        }
        if query_relative_directory_names(&root)?
            .iter()
            .map(|(name, attributes)| (name.as_str(), *attributes & (0x10 | 0x400)))
            .collect::<Vec<_>>()
            != manifest
                .files
                .iter()
                .map(|entry| (entry.fixture_leaf.as_str(), 0))
                .collect::<Vec<_>>()
        {
            return Err("disposable snapshot inventory drifted before retirement".into());
        }
        root.verify_owner_only_fixture_descriptor(&sid, LABEL)?;
        if evidence.snapshot_id != manifest.snapshot_id || evidence.files.len() != opened.len() {
            return Err("disposable snapshot retirement evidence drifted".into());
        }

        for file in opened {
            dispose_opened_object(file.as_raw_handle().cast(), LABEL)?;
            drop(file);
        }
        if !query_relative_directory_names(&root)?.is_empty() {
            return Err("disposable snapshot root not empty after file disposition".into());
        }
        root.verify_owner_only_fixture_descriptor(&sid, LABEL)?;
        let root = root.reopen_direct_child_for_delete(LABEL)?;
        root.verify_owner_only_fixture_descriptor(&sid, LABEL)?;
        if !query_relative_directory_names(&root)?.is_empty() {
            return Err("disposable snapshot root changed before disposition".into());
        }
        let mut parent = root.direct_parent_guard(LABEL)?;
        let name = root
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("disposable snapshot root name unavailable")?
            .to_owned();
        dispose_opened_object(root.last_handle(LABEL)?, LABEL)?;
        drop(root);
        if parent.descend_optional_existing(&name, LABEL)? {
            return Err("disposable snapshot root identity drifted after disposition".into());
        }
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
) -> Result<StagedCandidateSnapshot, CandidateStageError> {
    let witness = crate::isolated_fixture_github_authority::fetch_live_witness(expected_commit)
        .await
        .map_err(CandidateStageError::BeforeOwnership)?;
    let snapshot_id = Uuid::new_v4().to_string();
    let (manifest, candidates) =
        crate::isolated_fixture_git_objects::read_witnessed_candidate_git_objects(
            workspace,
            &witness,
            &snapshot_id,
        )
        .await
        .map_err(CandidateStageError::BeforeOwnership)?;
    if manifest.source_commit != witness.commit_sha() {
        return Err(CandidateStageError::BeforeOwnership(
            "isolated fixture source witness mismatch".into(),
        ));
    }
    let temp = std::fs::canonicalize(std::env::temp_dir()).map_err(|_| {
        CandidateStageError::BeforeOwnership("isolated fixture temp unavailable".into())
    })?;
    #[cfg(windows)]
    {
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .map_err(|_| {
                    CandidateStageError::BeforeOwnership(
                        "isolated fixture interactive owner identity unavailable".into(),
                    )
                })?;
        // The independent owner-only intent is durably written before the
        // candidate root can be created. A crash at any later point leaves
        // read-only forensic evidence, never automatic delete authority.
        let (journal, intent, intent_bytes) = create_fixture_owner_intent(&manifest, &sid)
            .map_err(|reason| {
                CandidateStageError::BeforeOwnership(format!(
                    "fixture owner intent {} unavailable: {reason}",
                    manifest.snapshot_id
                ))
            })?;
        let root = ProtectedDirectoryGuard::acquire(&temp, LABEL).map_err(|reason| {
            let attention = record_fixture_owner_attention(
                &journal, &intent, &intent_bytes, None, "parent_unavailable", &sid,
            );
            CandidateStageError::BeforeOwnership(format!(
                "fixture owner intent {} persisted; candidate parent unavailable: {reason}; attention: {attention:?}",
                manifest.snapshot_id
            ))
        })?;
        let (root, manifest) = create_owned_candidate_root(
            root,
            intent.root_name.clone(),
            &sid,
            manifest,
        )
        .map_err(|mut failure| {
            let code = match &failure {
                CandidateStageError::CreationIndeterminate(_) => "creation_indeterminate",
                CandidateStageError::Owned(_) => "root_binding_unavailable",
                CandidateStageError::BeforeOwnership(_) => "parent_unavailable",
            };
            if let Err(attention_error) =
                record_fixture_owner_attention(&journal, &intent, &intent_bytes, None, code, &sid)
            {
                match &mut failure {
                    CandidateStageError::CreationIndeterminate(failure) => {
                        failure.reason.push_str(&format!(
                            "; attention record unavailable: {attention_error}"
                        ));
                    }
                    CandidateStageError::Owned(failure) => {
                        failure.reason.push_str(&format!(
                            "; attention record unavailable: {attention_error}"
                        ));
                    }
                    CandidateStageError::BeforeOwnership(reason) => {
                        reason.push_str(&format!(
                            "; attention record unavailable: {attention_error}"
                        ));
                    }
                }
            }
            failure
        })?;
        let binding = match bind_fixture_owner_root(&journal, &intent, &intent_bytes, &root, &sid) {
            Ok(binding) => binding,
            Err(reason) => {
                let attention = record_fixture_owner_attention(
                    &journal,
                    &intent,
                    &intent_bytes,
                    None,
                    "root_binding_unavailable",
                    &sid,
                );
                return Err(CandidateStageError::Owned(Box::new(
                    OwnedCandidateStageFailure {
                        root,
                        manifest,
                        reason: format!("{reason}; attention: {attention:?}"),
                    },
                )));
            }
        };
        finish_owned_candidate_stage(root, manifest, &candidates).map_err(|mut failure| {
            if let Err(attention_error) = record_fixture_owner_attention(
                &journal,
                &intent,
                &intent_bytes,
                Some(&binding),
                "partial_stage",
                &sid,
            ) {
                failure.reason.push_str(&format!(
                    "; attention record unavailable: {attention_error}"
                ));
            }
            CandidateStageError::Owned(failure)
        })
    }
    #[cfg(not(windows))]
    return Err(CandidateStageError::BeforeOwnership(
        "isolated fixture owner-only creation is Windows-only".into(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isolated_fixture_manifest::{SOURCE_REPOSITORY, SnapshotFile};
    use sha2::{Digest, Sha256};

    #[tokio::test]
    async fn staged_bytes_roundtrip_only_inside_test_owned_root() {
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
        let call = |tool: &str, leaf: &str| crate::mcp::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".into(),
            params: serde_json::json!({
                "name": tool,
                "arguments": {"path": leaf, "pattern": "tracked"}
            }),
        };
        for tool in ["read", "search"] {
            let allowed = crate::mcp::handle_isolated_fixture_request_with_snapshot(
                &call(tool, "Cargo.toml"),
                &staged,
            )
            .await
            .expect("fixture tool response");
            assert_eq!(
                allowed.result.as_ref().unwrap()["isError"],
                serde_json::Value::Null
            );
            let denied = crate::mcp::handle_isolated_fixture_request_with_snapshot(
                &call(tool, "secret.txt"),
                &staged,
            )
            .await
            .expect("fixture denied response");
            assert_eq!(denied.result.as_ref().unwrap()["isError"], true);
        }
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

    #[tokio::test]
    async fn manifest_bound_mcp_read_and_search_reject_changed_and_new_leaves() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = temp.join(format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).expect("create own fixture");
        let expected = b"reviewed source";
        std::fs::write(dir.join("Cargo.toml"), expected).expect("write own leaf");
        let root = ProtectedDirectoryGuard::acquire(&dir, LABEL).expect("pin own fixture");
        let staged = StagedCandidateSnapshot {
            root,
            manifest: SnapshotManifest {
                schema_version: 1,
                source_repository: SOURCE_REPOSITORY.into(),
                source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
                snapshot_id: Uuid::new_v4().to_string(),
                files: vec![SnapshotFile {
                    fixture_leaf: "Cargo.toml".into(),
                    source_path: "Cargo.toml".into(),
                    size_bytes: expected.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(expected)),
                }],
            },
            // Deliberately omit read leases in this adversarial test so a
            // same-user writer can change bytes between tool calls.
            _leases: Vec::new(),
        };
        let call = |tool: &str, leaf: &str| crate::mcp::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".into(),
            params: serde_json::json!({
                "name": tool,
                "arguments": {"path": leaf, "pattern": "reviewed"}
            }),
        };
        for tool in ["read", "search"] {
            let result = crate::mcp::handle_isolated_fixture_request_with_snapshot(
                &call(tool, "Cargo.toml"),
                &staged,
            )
            .await
            .unwrap();
            assert!(result.result.unwrap()["isError"].is_null());
        }
        std::fs::write(dir.join("Cargo.toml"), b"changed source").expect("mutate own leaf");
        std::fs::write(dir.join("new.txt"), b"reviewed source").expect("add foreign leaf");
        for tool in ["read", "search"] {
            for leaf in ["Cargo.toml", "new.txt"] {
                let result = crate::mcp::handle_isolated_fixture_request_with_snapshot(
                    &call(tool, leaf),
                    &staged,
                )
                .await
                .unwrap();
                assert_eq!(result.result.unwrap()["isError"], true);
            }
        }
        drop(staged);
        std::fs::remove_file(dir.join("new.txt")).expect("remove own injected leaf");
        std::fs::remove_file(dir.join("Cargo.toml")).expect("remove own source leaf");
        std::fs::remove_dir(dir).expect("remove empty own fixture");
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
    fn owned_fixture_retirement_deletes_only_exact_manifest_files_and_empty_root() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp");
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .expect("ordinary interactive test owner SID required");
        let name = format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple());
        let mut root = ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp");
        root.create_owner_only_fixture_child(&name, &sid, LABEL)
            .expect("create owner-only disposable root");
        let entries = [
            (
                "Cargo.toml",
                "Cargo.toml",
                b"[package]\nname='fixture'\n".as_slice(),
            ),
            ("main.rs", "src/main.rs", b"fn main() {}\n".as_slice()),
        ];
        let manifest = SnapshotManifest {
            schema_version: 1,
            source_repository: SOURCE_REPOSITORY.into(),
            source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            snapshot_id: Uuid::new_v4().to_string(),
            files: entries
                .iter()
                .map(|(leaf, source, bytes)| SnapshotFile {
                    fixture_leaf: (*leaf).into(),
                    source_path: (*source).into(),
                    size_bytes: bytes.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                })
                .collect(),
        };
        let candidates = entries
            .iter()
            .map(|(_, source, bytes)| ((*source).into(), bytes.to_vec()))
            .collect::<Vec<_>>();
        let staged = finish_owned_candidate_stage(root, manifest, &candidates)
            .unwrap_or_else(|failure| panic!("owned stage failed: {}", failure.reason));
        staged
            .retire_owned_root()
            .expect("retire only exact owned files and root");
        assert!(!temp.join(name).exists(), "owned root must be absent");
    }

    #[cfg(windows)]
    #[test]
    fn owner_journal_records_pre_root_attention_without_delete_authority() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp");
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .expect("ordinary interactive test owner SID required");
        let manifest = SnapshotManifest {
            schema_version: 1,
            source_repository: SOURCE_REPOSITORY.into(),
            source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            snapshot_id: Uuid::new_v4().to_string(),
            files: vec![SnapshotFile {
                fixture_leaf: "Cargo.toml".into(),
                source_path: "Cargo.toml".into(),
                size_bytes: 0,
                sha256: format!("{:x}", Sha256::digest([])),
            }],
        };
        let journal_name = format!(
            "catdesk-fixture-journal-{}",
            Uuid::parse_str(&manifest.snapshot_id).unwrap().simple()
        );
        let (journal, intent, intent_bytes) =
            create_fixture_owner_intent(&manifest, &sid).expect("persist intent");
        record_fixture_owner_attention(
            &journal,
            &intent,
            &intent_bytes,
            None,
            "creation_indeterminate",
            &sid,
        )
        .expect("persist attention before root binding");
        drop(journal);

        let mut reopened = ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp");
        reopened
            .descend_existing_for_security(&journal_name, LABEL)
            .expect("reopen journal after crash");
        let (_, root_binding) = read_fixture_owner_records(&reopened, &manifest, None, &sid)
            .expect("read pre-root intent");
        assert!(root_binding.is_none());
        let attention = read_fixture_owner_attention(&reopened, &intent, &intent_bytes, None, &sid)
            .expect("read attention")
            .expect("attention present");
        assert_eq!(attention.code, "creation_indeterminate");
        assert!(attention.root_object_identity.is_none());
        assert!(!temp.join(&intent.root_name).exists());
        drop(reopened);
        std::fs::remove_file(temp.join(&journal_name).join("attention.json"))
            .expect("remove own attention");
        std::fs::remove_file(temp.join(&journal_name).join("intent.json"))
            .expect("remove own intent");
        std::fs::remove_dir(temp.join(&journal_name)).expect("remove empty own journal");
    }

    #[cfg(windows)]
    #[test]
    fn owner_journal_survives_pre_root_and_bound_root_crash_windows() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp");
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .expect("ordinary interactive test owner SID required");
        let manifest = SnapshotManifest {
            schema_version: 1,
            source_repository: SOURCE_REPOSITORY.into(),
            source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            snapshot_id: Uuid::new_v4().to_string(),
            files: vec![SnapshotFile {
                fixture_leaf: "Cargo.toml".into(),
                source_path: "Cargo.toml".into(),
                size_bytes: 0,
                sha256: format!("{:x}", Sha256::digest([])),
            }],
        };
        let uuid = Uuid::parse_str(&manifest.snapshot_id).unwrap();
        let journal_name = format!("catdesk-fixture-journal-{}", uuid.simple());
        let (journal, intent, intent_bytes) =
            create_fixture_owner_intent(&manifest, &sid).expect("persist pre-root intent");
        assert!(!temp.join(&intent.root_name).exists());
        drop(journal);

        // Simulate a crash before root creation: the owner-only intent must
        // survive, while the absence of a root binding grants no deletion.
        let mut reopened_journal =
            ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp after crash");
        reopened_journal
            .descend_existing_for_security(&journal_name, LABEL)
            .expect("reopen exact journal");
        let (read_intent, pre_root) =
            read_fixture_owner_records(&reopened_journal, &manifest, None, &sid)
                .expect("read pre-root forensic intent");
        assert_eq!(read_intent, intent);
        assert!(pre_root.is_none());

        let root_parent = ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp");
        let (root, manifest) =
            create_owned_candidate_root(root_parent, intent.root_name.clone(), &sid, manifest)
                .unwrap_or_else(|_| panic!("create exact test root"));
        let binding =
            bind_fixture_owner_root(&reopened_journal, &intent, &intent_bytes, &root, &sid)
                .expect("persist opened root identity");
        drop(root);
        drop(reopened_journal);

        // Simulate a later crash: both records must reread and bind only the
        // exact opened root object, with no cleanup side effect.
        let mut reopened_journal =
            ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp after crash");
        reopened_journal
            .descend_existing_for_security(&journal_name, LABEL)
            .expect("reopen exact journal");
        let mut reopened_root =
            ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp after crash");
        reopened_root
            .descend_existing_for_security(&intent.root_name, LABEL)
            .expect("reopen exact root");
        let (_, read_binding) =
            read_fixture_owner_records(&reopened_journal, &manifest, Some(&reopened_root), &sid)
                .expect("verify exact bound root");
        assert_eq!(read_binding, Some(binding.clone()));

        record_fixture_owner_attention(
            &reopened_journal,
            &intent,
            &intent_bytes,
            Some(&binding),
            "partial_stage",
            &sid,
        )
        .expect("persist bounded attention");
        let attention = read_fixture_owner_attention(
            &reopened_journal,
            &intent,
            &intent_bytes,
            Some(&binding),
            &sid,
        )
        .expect("read bounded attention")
        .expect("attention present");
        assert_eq!(attention.code, "partial_stage");
        assert!(
            record_fixture_owner_attention(
                &reopened_journal,
                &intent,
                &intent_bytes,
                Some(&binding),
                "partial_stage",
                &sid,
            )
            .is_err(),
            "a second writer may not overwrite attention"
        );
        let mut altered_attention = attention;
        altered_attention.root_object_identity = None;
        std::fs::write(
            temp.join(&journal_name).join("attention.json"),
            serde_json::to_vec(&altered_attention).unwrap(),
        )
        .expect("tamper only test-owned attention record");
        assert!(
            read_fixture_owner_attention(
                &reopened_journal,
                &intent,
                &intent_bytes,
                Some(&binding),
                &sid
            )
            .is_err(),
            "attention for another root identity must not verify"
        );

        let mut altered = binding;
        altered.root_object_identity = "00000000:00000000:00000000".into();
        std::fs::write(
            temp.join(&journal_name).join("root.json"),
            serde_json::to_vec(&altered).unwrap(),
        )
        .expect("tamper only test-owned journal record");
        assert!(
            read_fixture_owner_records(&reopened_journal, &manifest, Some(&reopened_root), &sid)
                .is_err(),
            "a changed root object ID must not verify"
        );
        assert!(temp.join(&intent.root_name).is_dir());
        drop(reopened_root);
        drop(reopened_journal);
        std::fs::remove_file(temp.join(&journal_name).join("attention.json"))
            .expect("remove test attention record");
        std::fs::remove_file(temp.join(&journal_name).join("root.json"))
            .expect("remove test root record");
        std::fs::remove_file(temp.join(&journal_name).join("intent.json"))
            .expect("remove test intent");
        std::fs::remove_dir(temp.join(&journal_name)).expect("remove empty test journal");
        std::fs::remove_dir(temp.join(&intent.root_name)).expect("remove empty test root");
    }

    #[cfg(windows)]
    #[test]
    fn owned_root_create_collision_is_indeterminate_without_delete_authority() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp");
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .expect("ordinary interactive test owner SID required");
        let name = format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple());
        let dir = temp.join(&name);
        std::fs::create_dir(&dir).expect("create test-owned collision root");
        std::fs::write(dir.join("foreign.txt"), b"do not touch")
            .expect("create test-owned foreign bytes");
        let manifest = SnapshotManifest {
            schema_version: 1,
            source_repository: SOURCE_REPOSITORY.into(),
            source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            snapshot_id: Uuid::new_v4().to_string(),
            files: vec![SnapshotFile {
                fixture_leaf: "Cargo.toml".into(),
                source_path: "Cargo.toml".into(),
                size_bytes: 0,
                sha256: format!("{:x}", Sha256::digest([])),
            }],
        };
        let expected_id = manifest.snapshot_id.clone();
        let parent = ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp");
        let failure = match create_owned_candidate_root(parent, name.clone(), &sid, manifest) {
            Err(CandidateStageError::CreationIndeterminate(failure)) => failure,
            _ => panic!("existing name must be indeterminate, not an owned root"),
        };
        assert_eq!(failure.generated_name, name);
        assert_eq!(failure.manifest.snapshot_id, expected_id);
        assert!(!failure.reason.is_empty());
        failure
            .guard
            .assert_stable(LABEL)
            .expect("parent stays pinned");
        assert_eq!(
            std::fs::read(dir.join("foreign.txt")).unwrap(),
            b"do not touch"
        );
        drop(failure);
        std::fs::remove_file(dir.join("foreign.txt")).expect("remove test file");
        std::fs::remove_dir(dir).expect("remove empty test root");
    }

    #[cfg(windows)]
    #[test]
    fn partial_owned_stage_returns_pinned_root_for_attention() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp");
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .expect("ordinary interactive test owner SID required");
        let name = format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple());
        let dir = temp.join(&name);
        let mut root = ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp");
        root.create_owner_only_fixture_child(&name, &sid, LABEL)
            .expect("create owner-only disposable root");
        // A preexisting second leaf causes a partial create-only stage after
        // the first reviewed leaf was written. It must never be overwritten.
        std::fs::write(dir.join("main.rs"), b"preexisting foreign bytes")
            .expect("create test collision");
        let cargo = b"[package]\nname='partial'\n".to_vec();
        let main = b"fn main() {}\n".to_vec();
        let manifest = SnapshotManifest {
            schema_version: 1,
            source_repository: SOURCE_REPOSITORY.into(),
            source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            snapshot_id: Uuid::new_v4().to_string(),
            files: vec![
                SnapshotFile {
                    fixture_leaf: "Cargo.toml".into(),
                    source_path: "Cargo.toml".into(),
                    size_bytes: cargo.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(&cargo)),
                },
                SnapshotFile {
                    fixture_leaf: "main.rs".into(),
                    source_path: "src/main.rs".into(),
                    size_bytes: main.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(&main)),
                },
            ],
        };
        let snapshot_id = manifest.snapshot_id.clone();
        let failure = match finish_owned_candidate_stage(
            root,
            manifest,
            &[
                ("Cargo.toml".into(), cargo.clone()),
                ("src/main.rs".into(), main),
            ],
        ) {
            Ok(_) => panic!("collision must fail the owned stage"),
            Err(failure) => failure,
        };
        assert_eq!(failure.manifest.snapshot_id, snapshot_id);
        failure
            .root
            .assert_stable(LABEL)
            .expect("owner handle retained");
        assert!(!failure.reason.is_empty());
        assert_eq!(std::fs::read(dir.join("Cargo.toml")).unwrap(), cargo);
        assert_eq!(
            std::fs::read(dir.join("main.rs")).unwrap(),
            b"preexisting foreign bytes"
        );
        drop(failure);
        std::fs::remove_file(dir.join("Cargo.toml")).expect("remove test-created first file");
        std::fs::remove_file(dir.join("main.rs")).expect("remove test-owned collision file");
        std::fs::remove_dir(dir).expect("remove empty test root");
    }

    #[cfg(windows)]
    #[test]
    fn owned_fixture_retirement_refuses_foreign_dacl_before_any_disposition() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp");
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .expect("ordinary interactive test owner SID required");
        let name = format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple());
        let mut root = ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp");
        let wrong_dacl =
            format!("O:{sid}G:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BU)");
        root.create_child_with_security_descriptor(&name, &wrong_dacl, LABEL)
            .expect("create only test-owned malformed ACL root");
        assert!(
            root.verify_owner_only_fixture_descriptor(&sid, LABEL)
                .is_err()
        );
        let bytes = b"reviewed bytes".to_vec();
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
        stage_candidate_into_owned_root(&root, &manifest, &[("Cargo.toml".into(), bytes.clone())])
            .expect("stage exact test bytes");
        let leases = retain_staged_artifacts(&root, &manifest).expect("retain test file");
        let dir = temp.join(&name);
        let staged = StagedCandidateSnapshot {
            root,
            manifest,
            _leases: leases,
        };
        assert!(staged.retire_owned_root().is_err());
        assert_eq!(std::fs::read(dir.join("Cargo.toml")).unwrap(), bytes);
        std::fs::remove_file(dir.join("Cargo.toml")).expect("remove test file");
        std::fs::remove_dir(dir).expect("remove empty test root");
    }

    #[cfg(windows)]
    #[test]
    fn owned_fixture_retirement_refuses_replaced_root_after_release() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp");
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .expect("ordinary interactive test owner SID required");
        let name = format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple());
        let dir = temp.join(&name);
        let retired = temp.join(format!("catdesk-retired-test-{}", Uuid::new_v4().simple()));
        let mut root = ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp");
        root.create_owner_only_fixture_child(&name, &sid, LABEL)
            .expect("create owner-only disposable root");
        let bytes = b"reviewed bytes".to_vec();
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
        stage_candidate_into_owned_root(&root, &manifest, &[("Cargo.toml".into(), bytes)])
            .expect("stage exact source");
        let leases = retain_staged_artifacts(&root, &manifest).expect("retain reviewed file");
        let move_from = dir.clone();
        let move_to = retired.clone();
        crate::windows_protected_fs::replace_reopen_delete_test_hook(Some(Box::new(move || {
            std::fs::rename(&move_from, &move_to).expect("move empty owned test root");
            std::fs::create_dir(&move_from).expect("create replacement test root");
        })));
        let staged = StagedCandidateSnapshot {
            root,
            manifest,
            _leases: leases,
        };
        assert!(
            staged.retire_owned_root().is_err(),
            "a replacement at the same name must not gain delete authority"
        );
        assert!(dir.is_dir(), "replacement must remain");
        assert!(retired.is_dir(), "original empty root must remain");
        std::fs::remove_dir(&dir).expect("remove empty test replacement");
        std::fs::remove_dir(&retired).expect("remove empty retired test root");
    }

    #[cfg(windows)]
    #[test]
    fn owned_fixture_retirement_refuses_foreign_leaf_before_any_disposition() {
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp");
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .expect("ordinary interactive test owner SID required");
        let name = format!("catdesk-dev-fixture-{}", Uuid::new_v4().simple());
        let mut root = ProtectedDirectoryGuard::acquire(&temp, LABEL).expect("pin temp");
        root.create_owner_only_fixture_child(&name, &sid, LABEL)
            .expect("create owner-only disposable root");
        let bytes = b"reviewed bytes".to_vec();
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
        stage_candidate_into_owned_root(&root, &manifest, &[("Cargo.toml".into(), bytes.clone())])
            .expect("stage exact source");
        let leases = retain_staged_artifacts(&root, &manifest).expect("retain reviewed file");
        let dir = temp.join(&name);
        std::fs::write(dir.join("foreign.txt"), b"outside manifest")
            .expect("inject test-owned foreign leaf");
        let staged = StagedCandidateSnapshot {
            root,
            manifest,
            _leases: leases,
        };
        assert!(staged.retire_owned_root().is_err());
        assert_eq!(std::fs::read(dir.join("Cargo.toml")).unwrap(), bytes);
        assert_eq!(
            std::fs::read(dir.join("foreign.txt")).unwrap(),
            b"outside manifest"
        );
        std::fs::remove_file(dir.join("foreign.txt")).expect("remove test foreign file");
        std::fs::remove_file(dir.join("Cargo.toml")).expect("remove test source file");
        std::fs::remove_dir(dir).expect("remove empty test root");
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
