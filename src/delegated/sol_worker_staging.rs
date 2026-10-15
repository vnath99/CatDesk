//! Dormant host-side staging intended for a verified, stopped Sol worker export.
//!
//! The source keeps a supplied-SID ACL audit internal to this module. Its
//! crate-visible stage path requires an opaque binding of the fixed local
//! worker account to an opened restricted process primary token. This checks
//! the opened root against that identity, but cannot prove that the process
//! is the intended Codex worker or excludes its children.
//! The caller must also prove the entire parent chain grants no worker
//! DELETE_CHILD or rename authority, ACL takeover rights, inherited writable
//! handles, or access through alternate groups or privileges.
//! It is not wired to Sol launch or repository mutation. It creates one fresh
//! child beneath the pinned root and leaves incomplete children inert for
//! later diagnosis.
//! The post-burn value is memory-only: a crash or staging failure after burn
//! leaves the nonce spent and requires operator attention, not an automatic
//! retry or a newly issued nonce.
//! Synced files and readback do not prove parent-directory metadata survives
//! sudden power loss.

use std::collections::{BTreeMap, BTreeSet};
#[cfg(windows)]
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::reviewed_source_snapshot::query_relative_directory_names_bounded;
use crate::windows_protected_fs::{
    ProtectedDirectoryGuard, read_relative_regular_unique, write_new_regular_in,
};

#[cfg(windows)]
use super::sol_worker_identity_preflight::BoundFixedLocalSolWorkerProcessV1;
use super::sol_worker_nonce::BurnedSolWorkerOutputV1;
use super::sol_worker_output::{ExpectedSolWorkerOutputV1, SolWorkerOutputFileV1};
use super::sol_worker_staging_security as security;

const LABEL: &str = "Sol worker staging";
const MAX_FILES: usize = 256;
const MAX_DEPTH: usize = 12;
const MAX_PATH_BYTES: usize = 512;
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const ROOT_MANIFEST: &str = "stage-manifest.json";
const PAYLOAD: &str = "payload";
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SolStagedOutputManifestV1 {
    pub schema_version: u32,
    pub purpose: String,
    pub stage_id: String,
    pub task_id: String,
    pub baseline_commit: String,
    pub native_turn_id: String,
    pub handoff_nonce: String,
    pub files: Vec<SolWorkerOutputFileV1>,
}

/// A single fresh stage object, pinned through its original parent. The
/// trusted host can use `readback` before applying any bytes to a repository.
struct PinnedSolWorkerStageV1 {
    acl_audited_root: ProtectedDirectoryGuard,
    guard: ProtectedDirectoryGuard,
    worker_token_user_sid: String,
    manifest: SolStagedOutputManifestV1,
}

impl PinnedSolWorkerStageV1 {
    fn manifest(&self) -> &SolStagedOutputManifestV1 {
        &self.manifest
    }

    fn readback(&self) -> Result<BTreeMap<String, Vec<u8>>, String> {
        security::verify_private_child(
            &self.acl_audited_root,
            &self.guard,
            &self.worker_token_user_sid,
        )?;
        let bytes = readback(&self.guard, &self.manifest)?;
        security::verify_private_child(
            &self.acl_audited_root,
            &self.guard,
            &self.worker_token_user_sid,
        )?;
        Ok(bytes)
    }
}

/// Readback retains the opaque fixed-account/process binding. A caller cannot
/// obtain this value from a supplied SID or an unbound token observation.
#[cfg(windows)]
pub(crate) struct FixedWorkerBoundPinnedSolWorkerStageV1 {
    stage: PinnedSolWorkerStageV1,
    bound_worker: Arc<BoundFixedLocalSolWorkerProcessV1>,
}

#[cfg(windows)]
impl FixedWorkerBoundPinnedSolWorkerStageV1 {
    pub(crate) fn manifest(&self) -> &SolStagedOutputManifestV1 {
        self.stage.manifest()
    }

    pub(crate) fn readback(&self) -> Result<BTreeMap<String, Vec<u8>>, String> {
        validate_bound_fixed_worker_for_stage_acl(
            &self.stage.acl_audited_root,
            &self.bound_worker,
        )?;
        let bytes = self.stage.readback()?;
        validate_bound_fixed_worker_for_stage_acl(
            &self.stage.acl_audited_root,
            &self.bound_worker,
        )?;
        Ok(bytes)
    }
}

/// An ACL audit of the exact opened staging root against a caller-supplied
/// SID. The root must have been opened for READ_CONTROL relative to a pinned
/// parent. This value does not prove that the SID identifies the real worker
/// or that the worker lost write authority through other SIDs or privileges.
struct AclAuditedSolStageRootForSuppliedSidV1 {
    guard: ProtectedDirectoryGuard,
    worker_token_user_sid: String,
}

fn audit_sol_stage_root_acl_for_supplied_sid(
    root: &ProtectedDirectoryGuard,
    worker_token_user_sid: &str,
) -> Result<AclAuditedSolStageRootForSuppliedSidV1, String> {
    security::verify_private_root(root, worker_token_user_sid)?;
    Ok(AclAuditedSolStageRootForSuppliedSidV1 {
        guard: root.try_clone(LABEL)?,
        worker_token_user_sid: worker_token_user_sid.into(),
    })
}

#[cfg(windows)]
fn validate_bound_fixed_worker_for_stage_acl(
    root: &ProtectedDirectoryGuard,
    worker: &BoundFixedLocalSolWorkerProcessV1,
) -> Result<(), String> {
    worker.revalidate()?;
    let host_sid = security::current_host_sid()?;
    worker.reject_stage_acl_trustees(&[&host_sid, "S-1-5-18"])?;
    security::verify_private_root(root, worker.token_user_sid())?;
    worker.revalidate()
}

/// Binds the root ACL audit to a pinned process primary token whose TokenUser
/// was matched to the fixed local Sol worker account. The host must separately
/// prove this is the intended worker and that it and its descendants lost all
/// relevant write authority before repository use.
#[cfg(windows)]
pub(crate) struct AclAuditedSolStageRootForBoundFixedLocalWorkerV1 {
    supplied_sid_audit: AclAuditedSolStageRootForSuppliedSidV1,
    bound_worker: Arc<BoundFixedLocalSolWorkerProcessV1>,
}

#[cfg(windows)]
pub(crate) fn bind_sol_stage_root_acl_to_fixed_local_worker(
    root: &ProtectedDirectoryGuard,
    worker: BoundFixedLocalSolWorkerProcessV1,
) -> Result<AclAuditedSolStageRootForBoundFixedLocalWorkerV1, String> {
    validate_bound_fixed_worker_for_stage_acl(root, &worker)?;
    let supplied_sid_audit =
        audit_sol_stage_root_acl_for_supplied_sid(root, worker.token_user_sid())?;
    worker.revalidate()?;
    Ok(AclAuditedSolStageRootForBoundFixedLocalWorkerV1 {
        supplied_sid_audit,
        bound_worker: Arc::new(worker),
    })
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_component(component: &str) -> bool {
    if component.is_empty()
        || component.len() > 255
        || component.ends_with(['.', ' '])
        || !component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return false;
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !matches!(
            stem.as_str(),
            "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9"
        )
        && !matches!(
            stem.as_str(),
            "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
        )
}

fn valid_path(path: &str) -> bool {
    path.len() <= MAX_PATH_BYTES
        && path.split('/').count() <= MAX_DEPTH
        && path.split('/').all(valid_component)
}

fn valid_stage_id(stage_id: &str) -> bool {
    Uuid::parse_str(stage_id)
        .is_ok_and(|id| id.get_version_num() == 4 && id.simple().to_string() == stage_id)
}

fn stage_name(stage_id: &str) -> String {
    format!("sol-stage-{stage_id}")
}

fn manifest_bytes(manifest: &SolStagedOutputManifestV1) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(manifest).map_err(|_| format!("{LABEL} manifest is invalid"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(format!("{LABEL} manifest is oversized"));
    }
    Ok(bytes)
}

fn prepare_manifest(
    stage_id: &str,
    expected: &ExpectedSolWorkerOutputV1,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<SolStagedOutputManifestV1, String> {
    if !valid_stage_id(stage_id)
        || !valid_slug(&expected.task_id)
        || !valid_slug(&expected.native_turn_id)
        || !valid_slug(&expected.handoff_nonce)
        || expected.baseline_commit.len() != 40
        || !expected
            .baseline_commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || files.is_empty()
        || files.len() > MAX_FILES
        || expected.allowed_paths.len() > MAX_FILES
        || expected.allowed_paths.iter().any(|path| !valid_path(path))
    {
        return Err(format!("{LABEL} binding is invalid"));
    }
    let folded_allowed = expected
        .allowed_paths
        .iter()
        .map(|path| path.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if folded_allowed.len() != expected.allowed_paths.len() {
        return Err(format!("{LABEL} allowlist is ambiguous"));
    }
    let mut total = 0_usize;
    let mut folded_files = BTreeSet::new();
    let mut entries = Vec::with_capacity(files.len());
    for (path, bytes) in files {
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| format!("{LABEL} size overflow"))?;
        if !valid_path(path)
            || !expected.allowed_paths.contains(path)
            || !folded_files.insert(path.to_ascii_lowercase())
            || bytes.len() > MAX_FILE_BYTES
            || total > MAX_TOTAL_BYTES
        {
            return Err(format!("{LABEL} output is unsafe"));
        }
        entries.push(SolWorkerOutputFileV1 {
            path: path.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        });
    }
    let manifest = SolStagedOutputManifestV1 {
        schema_version: 1,
        purpose: "sol-worker-host-stage".into(),
        stage_id: stage_id.into(),
        task_id: expected.task_id.clone(),
        baseline_commit: expected.baseline_commit.clone(),
        native_turn_id: expected.native_turn_id.clone(),
        handoff_nonce: expected.handoff_nonce.clone(),
        files: entries,
    };
    manifest_bytes(&manifest)?;
    Ok(manifest)
}

fn expected_directories(files: &[SolWorkerOutputFileV1]) -> BTreeSet<String> {
    let mut directories = BTreeSet::new();
    for file in files {
        let mut prefix = String::new();
        let mut components = file.path.split('/').peekable();
        while let Some(component) = components.next() {
            if components.peek().is_none() {
                break;
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            directories.insert(prefix.clone());
        }
    }
    directories
}

fn visit_payload(
    parent: &ProtectedDirectoryGuard,
    prefix: &str,
    depth: usize,
    observed_files: &mut BTreeSet<String>,
    observed_dirs: &mut BTreeSet<String>,
) -> Result<(), String> {
    if depth >= MAX_DEPTH {
        return Err(format!("{LABEL} payload is too deep"));
    }
    parent.assert_stable(LABEL)?;
    for (name, attributes) in query_relative_directory_names_bounded(parent, MAX_FILES + 1)? {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if !valid_path(&path) || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!("{LABEL} payload is unsafe"));
        }
        if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            if !observed_dirs.insert(path.clone()) || observed_dirs.len() > MAX_FILES * MAX_DEPTH {
                return Err(format!("{LABEL} payload is oversized"));
            }
            let mut child = parent.try_clone(LABEL)?;
            child.descend_existing(&name, LABEL)?;
            visit_payload(&child, &path, depth + 1, observed_files, observed_dirs)?;
        } else if !observed_files.insert(path) || observed_files.len() > MAX_FILES {
            return Err(format!("{LABEL} payload is oversized"));
        }
    }
    parent.assert_stable(LABEL)
}

fn exact_inventory(
    stage: &ProtectedDirectoryGuard,
    manifest: &SolStagedOutputManifestV1,
) -> Result<ProtectedDirectoryGuard, String> {
    stage.assert_stable(LABEL)?;
    let root_entries = query_relative_directory_names_bounded(stage, 2)?;
    if root_entries.len() != 2
        || !root_entries.iter().any(|(name, attributes)| {
            name == ROOT_MANIFEST
                && attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0
        })
        || !root_entries.iter().any(|(name, attributes)| {
            name == PAYLOAD
                && attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
                    == FILE_ATTRIBUTE_DIRECTORY
        })
    {
        return Err(format!("{LABEL} root inventory differs from manifest"));
    }
    let mut payload = stage.try_clone(LABEL)?;
    payload.descend_existing(PAYLOAD, LABEL)?;
    let mut observed_files = BTreeSet::new();
    let mut observed_dirs = BTreeSet::new();
    visit_payload(&payload, "", 0, &mut observed_files, &mut observed_dirs)?;
    let listed_files = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    if observed_files != listed_files || observed_dirs != expected_directories(&manifest.files) {
        return Err(format!("{LABEL} payload inventory differs from manifest"));
    }
    stage.assert_stable(LABEL)?;
    Ok(payload)
}

fn readback(
    stage: &ProtectedDirectoryGuard,
    manifest: &SolStagedOutputManifestV1,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let expected_manifest = manifest_bytes(manifest)?;
    let on_disk_manifest =
        read_relative_regular_unique(stage, ROOT_MANIFEST, MAX_MANIFEST_BYTES as u64, LABEL)?;
    if on_disk_manifest != expected_manifest {
        return Err(format!("{LABEL} manifest changed"));
    }
    let payload = exact_inventory(stage, manifest)?;
    let mut result = BTreeMap::new();
    for file in &manifest.files {
        let mut parent = payload.try_clone(LABEL)?;
        let mut components = file.path.split('/').collect::<Vec<_>>();
        let leaf = components
            .pop()
            .ok_or_else(|| format!("{LABEL} path is invalid"))?;
        for component in components {
            parent.descend_existing(component, LABEL)?;
        }
        let bytes = read_relative_regular_unique(&parent, leaf, file.size, LABEL)?;
        if bytes.len() as u64 != file.size || format!("{:x}", Sha256::digest(&bytes)) != file.sha256
        {
            return Err(format!("{LABEL} bytes changed"));
        }
        result.insert(file.path.clone(), bytes);
    }
    exact_inventory(stage, manifest)?;
    if read_relative_regular_unique(stage, ROOT_MANIFEST, MAX_MANIFEST_BYTES as u64, LABEL)?
        != expected_manifest
    {
        return Err(format!("{LABEL} manifest changed"));
    }
    Ok(result)
}

/// Internal staging primitive below a pinned root whose ACL was audited
/// against a SID. Its caller-visible wrapper requires the fixed-account
/// process binding and nonce burn result before this can create any stage.
/// It accepts no worker-supplied path or stage ID, retries no collision,
/// removes no crash residue, and grants no repository or Sol launch authority.
fn stage_verified_sol_worker_output(
    acl_audited_root: &AclAuditedSolStageRootForSuppliedSidV1,
    expected: &ExpectedSolWorkerOutputV1,
    burned: BurnedSolWorkerOutputV1,
) -> Result<PinnedSolWorkerStageV1, String> {
    let stage_id = Uuid::new_v4().simple().to_string();
    stage_verified_sol_worker_output_with_id(acl_audited_root, expected, burned, &stage_id)
}

/// The only crate-visible path from a burned Sol output to a host stage. The
/// fixed local worker/process binding is revalidated around staging and held
/// for readback; this does not prove worker or descendant exit, nor ancestor
/// and inherited-handle write exclusion.
#[cfg(windows)]
pub(crate) fn stage_verified_sol_worker_output_for_bound_fixed_local_worker(
    bound_root: &AclAuditedSolStageRootForBoundFixedLocalWorkerV1,
    expected: &ExpectedSolWorkerOutputV1,
    burned: BurnedSolWorkerOutputV1,
) -> Result<FixedWorkerBoundPinnedSolWorkerStageV1, String> {
    validate_bound_fixed_worker_for_stage_acl(
        &bound_root.supplied_sid_audit.guard,
        &bound_root.bound_worker,
    )?;
    let stage = stage_verified_sol_worker_output(&bound_root.supplied_sid_audit, expected, burned)?;
    let staged = FixedWorkerBoundPinnedSolWorkerStageV1 {
        stage,
        bound_worker: Arc::clone(&bound_root.bound_worker),
    };
    staged.readback()?;
    Ok(staged)
}

fn stage_verified_sol_worker_output_with_id(
    acl_audited_root: &AclAuditedSolStageRootForSuppliedSidV1,
    expected: &ExpectedSolWorkerOutputV1,
    burned: BurnedSolWorkerOutputV1,
    stage_id: &str,
) -> Result<PinnedSolWorkerStageV1, String> {
    let (verified_expected, files) = burned.into_parts();
    if verified_expected != *expected {
        return Err(format!(
            "{LABEL} verified binding differs from staging authority"
        ));
    }
    let manifest = prepare_manifest(stage_id, expected, &files)?;
    security::verify_private_root(
        &acl_audited_root.guard,
        &acl_audited_root.worker_token_user_sid,
    )?;
    let mut stage = acl_audited_root.guard.try_clone(LABEL)?;
    stage.create_child(&stage_name(stage_id), LABEL)?;
    let mut security_stage = acl_audited_root.guard.try_clone(LABEL)?;
    security_stage.descend_existing_for_security(&stage_name(stage_id), LABEL)?;
    security::same_opened_directory(&stage, &security_stage)?;
    security::verify_private_child(
        &acl_audited_root.guard,
        &security_stage,
        &acl_audited_root.worker_token_user_sid,
    )?;
    drop(stage);
    let stage = security_stage;
    let mut payload = stage.try_clone(LABEL)?;
    payload.create_child(PAYLOAD, LABEL)?;
    for path in expected_directories(&manifest.files) {
        let mut components = path.split('/').collect::<Vec<_>>();
        let leaf = components
            .pop()
            .ok_or_else(|| format!("{LABEL} path is invalid"))?;
        let mut parent = payload.try_clone(LABEL)?;
        for component in components {
            parent.descend_existing(component, LABEL)?;
        }
        parent.create_child(leaf, LABEL)?;
    }
    for (path, bytes) in &files {
        let mut components = path.split('/').collect::<Vec<_>>();
        let leaf = components
            .pop()
            .ok_or_else(|| format!("{LABEL} path is invalid"))?;
        let mut parent = payload.try_clone(LABEL)?;
        for component in components {
            parent.descend_existing(component, LABEL)?;
        }
        write_new_regular_in(&parent, leaf, bytes, LABEL)?;
    }
    let serialized = manifest_bytes(&manifest)?;
    write_new_regular_in(&stage, ROOT_MANIFEST, &serialized, LABEL)?;
    let staged = PinnedSolWorkerStageV1 {
        acl_audited_root: acl_audited_root.guard.try_clone(LABEL)?,
        guard: stage,
        worker_token_user_sid: acl_audited_root.worker_token_user_sid.clone(),
        manifest,
    };
    if staged.readback()? != files {
        return Err(format!("{LABEL} readback differs from verified output"));
    }
    security::verify_private_root(
        &acl_audited_root.guard,
        &acl_audited_root.worker_token_user_sid,
    )?;
    Ok(staged)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::ffi::c_void;
    use std::fs;
    use std::os::windows::io::BorrowedHandle;
    use std::path::PathBuf;

    use super::super::codex_model_policy::{CodexCliTurnProfileEvidenceV1, SOL_MEDIUM_MODEL};
    use super::super::sol_worker_identity_preflight::bind_fixed_local_sol_worker_to_opened_process;
    use super::super::sol_worker_nonce::{
        SolWorkerNonceAuthorityV1, bind_sol_worker_native_turn, issue_sol_worker_nonce,
        verify_and_burn_sol_worker_output,
    };
    use super::super::sol_worker_output::SolWorkerOutputManifestV1;

    const TEST_WORKER_SID: &str = "S-1-5-21-111-222-333-1009";

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }

    #[test]
    fn exposed_stage_boundary_requires_fixed_account_process_binding() {
        let _: fn(
            &ProtectedDirectoryGuard,
            BoundFixedLocalSolWorkerProcessV1,
        ) -> Result<AclAuditedSolStageRootForBoundFixedLocalWorkerV1, String> =
            bind_sol_stage_root_acl_to_fixed_local_worker;
        let _: fn(
            &AclAuditedSolStageRootForBoundFixedLocalWorkerV1,
            &ExpectedSolWorkerOutputV1,
            BurnedSolWorkerOutputV1,
        ) -> Result<FixedWorkerBoundPinnedSolWorkerStageV1, String> =
            stage_verified_sol_worker_output_for_bound_fixed_local_worker;
        let _: fn(
            &FixedWorkerBoundPinnedSolWorkerStageV1,
        ) -> Result<BTreeMap<String, Vec<u8>>, String> =
            FixedWorkerBoundPinnedSolWorkerStageV1::readback;
    }

    #[test]
    fn current_host_process_cannot_satisfy_fixed_worker_stage_boundary() {
        let process = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) };
        assert!(bind_fixed_local_sol_worker_to_opened_process(process).is_err());
    }

    struct Fixture {
        root: PathBuf,
        ledger: PathBuf,
        export: PathBuf,
        staging: PathBuf,
        authority: SolWorkerNonceAuthorityV1,
        expected: ExpectedSolWorkerOutputV1,
        worker_manifest: SolWorkerOutputManifestV1,
    }

    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("catdesk-sol-stage-test-{}", Uuid::new_v4()));
            let ledger = root.join("ledger");
            let export = root.join("export");
            let staging = root.join("staging");
            fs::create_dir(&root).expect("fresh test root");
            fs::create_dir(&ledger).expect("fresh nonce ledger");
            fs::create_dir(&export).expect("fresh export");
            let host_sid = security::current_host_sid().expect("host token SID");
            let stage_sddl = if host_sid == "S-1-5-18" {
                "O:SYG:SYD:P(A;OICI;FA;;;SY)".into()
            } else {
                format!("O:{host_sid}G:{host_sid}D:P(A;OICI;FA;;;{host_sid})(A;OICI;FA;;;SY)")
            };
            let root_guard = ProtectedDirectoryGuard::acquire(&root, LABEL).expect("pin test root");
            let mut stage_guard = root_guard.try_clone(LABEL).expect("clone test root");
            stage_guard
                .create_child_with_security_descriptor("staging", &stage_sddl, LABEL)
                .expect("create protected staging root");
            drop(stage_guard);
            drop(root_guard);
            fs::create_dir(export.join("src")).expect("export source directory");
            fs::write(export.join("src/lib.rs"), b"changed\n").expect("export source file");
            fs::write(export.join("README.md"), b"review\n").expect("export readme");
            let authority = SolWorkerNonceAuthorityV1 {
                session_id: "session-1".into(),
                task_id: "T-0483".into(),
                host_turn_id: "host-turn-123".into(),
                contract_hash: "b".repeat(64),
                baseline_commit: "a".repeat(40),
                allowed_paths: BTreeSet::from(["README.md".into(), "src/lib.rs".into()]),
            };
            let ledger_guard =
                ProtectedDirectoryGuard::acquire(&ledger, LABEL).expect("pin ledger");
            let nonce = issue_sol_worker_nonce(&ledger_guard, &authority).expect("issue nonce");
            let expected = ExpectedSolWorkerOutputV1 {
                task_id: authority.task_id.clone(),
                baseline_commit: authority.baseline_commit.clone(),
                native_turn_id: "turn-123".into(),
                handoff_nonce: nonce,
                allowed_paths: authority.allowed_paths.clone(),
            };
            let profile = CodexCliTurnProfileEvidenceV1 {
                codex_thread_id: "thread-abc".into(),
                codex_turn_id: expected.native_turn_id.clone(),
                model_id: SOL_MEDIUM_MODEL.into(),
                reasoning_effort: "medium".into(),
                evidence_sha256: "c".repeat(64),
            };
            bind_sol_worker_native_turn(
                &ledger_guard,
                &authority,
                &expected.handoff_nonce,
                "thread-abc",
                &profile,
            )
            .expect("bind native Sol turn");
            drop(ledger_guard);
            let worker_manifest = SolWorkerOutputManifestV1 {
                schema_version: 1,
                task_id: expected.task_id.clone(),
                baseline_commit: expected.baseline_commit.clone(),
                native_turn_id: expected.native_turn_id.clone(),
                handoff_nonce: expected.handoff_nonce.clone(),
                files: vec![
                    SolWorkerOutputFileV1 {
                        path: "README.md".into(),
                        size: 7,
                        sha256: format!("{:x}", Sha256::digest(b"review\n")),
                    },
                    SolWorkerOutputFileV1 {
                        path: "src/lib.rs".into(),
                        size: 8,
                        sha256: format!("{:x}", Sha256::digest(b"changed\n")),
                    },
                ],
            };
            Self {
                root,
                ledger,
                export,
                staging,
                authority,
                expected,
                worker_manifest,
            }
        }

        fn try_burn(&self) -> Result<BurnedSolWorkerOutputV1, String> {
            let ledger = ProtectedDirectoryGuard::acquire(&self.ledger, LABEL).expect("pin ledger");
            let export = ProtectedDirectoryGuard::acquire(&self.export, LABEL).expect("pin export");
            verify_and_burn_sol_worker_output(
                &ledger,
                &export,
                &self.authority,
                &self.expected,
                &self.worker_manifest,
            )
        }

        fn burned(&self) -> BurnedSolWorkerOutputV1 {
            self.try_burn().expect("verify and burn output")
        }

        fn stage(&self) -> PinnedSolWorkerStageV1 {
            let guard = self.private_root();
            stage_verified_sol_worker_output(&guard, &self.expected, self.burned())
                .expect("stage exact export")
        }

        fn private_root(&self) -> AclAuditedSolStageRootForSuppliedSidV1 {
            let base = ProtectedDirectoryGuard::acquire(&self.root, LABEL).expect("pin test root");
            let mut stage = base.try_clone(LABEL).expect("clone test root");
            stage
                .descend_existing_for_security("staging", LABEL)
                .expect("open staging root for READ_CONTROL");
            audit_sol_stage_root_acl_for_supplied_sid(&stage, TEST_WORKER_SID)
                .expect("audit test root ACL")
        }

        fn remove(self, stage_id: Option<&str>) {
            if let Some(stage_id) = stage_id {
                let stage = self.staging.join(stage_name(stage_id));
                fs::remove_file(stage.join("payload/README.md")).expect("remove staged readme");
                fs::remove_file(stage.join("payload/src/lib.rs")).expect("remove staged source");
                fs::remove_dir(stage.join("payload/src")).expect("remove staged source directory");
                fs::remove_dir(stage.join("payload")).expect("remove staged payload");
                fs::remove_file(stage.join(ROOT_MANIFEST)).expect("remove staged manifest");
                fs::remove_dir(stage).expect("remove exact staged directory");
            }
            fs::remove_file(self.export.join("src/lib.rs")).expect("remove export source");
            fs::remove_file(self.export.join("README.md")).expect("remove export readme");
            fs::remove_dir(self.export.join("src")).expect("remove export source directory");
            fs::remove_dir(&self.export).expect("remove exact export");
            let mut digest = Sha256::new();
            digest.update((self.authority.session_id.len() as u64).to_le_bytes());
            digest.update(self.authority.session_id.as_bytes());
            digest.update((self.authority.host_turn_id.len() as u64).to_le_bytes());
            digest.update(self.authority.host_turn_id.as_bytes());
            let active = format!("active-{:x}.json", digest.finalize());
            for name in [
                active,
                format!("issued-{}.json", self.expected.handoff_nonce),
                format!("native-turn-{}.json", self.expected.handoff_nonce),
                format!("burned-{}.json", self.expected.handoff_nonce),
            ] {
                let file = self.ledger.join(name);
                if file.exists() {
                    fs::remove_file(file).expect("remove exact test nonce record");
                }
            }
            fs::remove_dir(&self.ledger).expect("remove exact nonce ledger");
            fs::remove_dir(&self.staging).expect("remove exact staging root");
            fs::remove_dir(&self.root).expect("remove exact test root");
        }
    }

    #[test]
    fn staged_bytes_and_manifest_are_exact_after_worker_export_changes() {
        let fixture = Fixture::new();
        let staged = fixture.stage();
        let stage_id = staged.manifest().stage_id.clone();
        assert!(
            fixture.try_burn().is_err(),
            "spent nonce cannot mint a second stage input"
        );
        fs::write(
            fixture.export.join("src/lib.rs"),
            b"untrusted after handoff",
        )
        .expect("change worker export");
        let files = staged.readback().expect("read back exact stage");
        assert_eq!(files["README.md"], b"review\n");
        assert_eq!(files["src/lib.rs"], b"changed\n");
        assert_eq!(staged.manifest().purpose, "sol-worker-host-stage");
        assert_eq!(staged.manifest().task_id, fixture.expected.task_id);
        assert_eq!(
            staged.manifest().native_turn_id,
            fixture.expected.native_turn_id
        );
        assert_eq!(
            staged.manifest().handoff_nonce,
            fixture.expected.handoff_nonce
        );
        assert_eq!(
            staged
                .manifest()
                .files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            ["README.md", "src/lib.rs"]
        );
        drop(staged);
        fixture.remove(Some(&stage_id));
    }

    #[test]
    fn collision_fails_closed_and_does_not_replace_existing_stage() {
        let fixture = Fixture::new();
        let guard = fixture.private_root();
        let stage_id = Uuid::new_v4().simple().to_string();
        let collided = fixture.staging.join(stage_name(&stage_id));
        fs::create_dir(&collided).expect("preexisting stage identity");
        fs::write(collided.join("sentinel.txt"), b"untouched").expect("preexisting sentinel");
        assert!(
            stage_verified_sol_worker_output_with_id(
                &guard,
                &fixture.expected,
                fixture.burned(),
                &stage_id,
            )
            .is_err()
        );
        assert_eq!(
            fs::read(collided.join("sentinel.txt")).expect("unchanged sentinel"),
            b"untouched"
        );
        assert!(!collided.join(ROOT_MANIFEST).exists());
        drop(guard);
        fs::remove_file(collided.join("sentinel.txt")).expect("remove exact sentinel");
        fs::remove_dir(collided).expect("remove exact collision directory");
        fixture.remove(None);
    }

    #[test]
    fn readback_rejects_changed_bytes_manifest_and_extra_entries() {
        let fixture = Fixture::new();
        let staged = fixture.stage();
        let stage_id = staged.manifest().stage_id.clone();
        let root = fixture.staging.join(stage_name(&stage_id));
        fs::write(root.join("payload/src/lib.rs"), b"changed!").expect("tamper payload");
        assert!(staged.readback().is_err());
        fs::write(root.join("payload/src/lib.rs"), b"changed\n").expect("restore payload");
        fs::write(root.join("payload/extra.txt"), b"extra").expect("add extra file");
        assert!(staged.readback().is_err());
        fs::remove_file(root.join("payload/extra.txt")).expect("remove extra file");
        fs::create_dir(root.join("payload/empty")).expect("add empty directory");
        assert!(staged.readback().is_err());
        fs::remove_dir(root.join("payload/empty")).expect("remove empty directory");
        fs::write(root.join("unexpected.txt"), b"extra").expect("add extra root entry");
        assert!(staged.readback().is_err());
        fs::remove_file(root.join("unexpected.txt")).expect("remove extra root entry");
        let original = fs::read(root.join(ROOT_MANIFEST)).expect("read manifest");
        fs::write(root.join(ROOT_MANIFEST), b"{}").expect("tamper manifest");
        assert!(staged.readback().is_err());
        fs::write(root.join(ROOT_MANIFEST), original).expect("restore manifest");
        assert!(staged.readback().is_ok());
        drop(staged);
        fixture.remove(Some(&stage_id));
    }

    #[test]
    fn readback_rejects_hardlinks_and_reparse_payload_entries() {
        let fixture = Fixture::new();
        let staged = fixture.stage();
        let stage_id = staged.manifest().stage_id.clone();
        let root = fixture.staging.join(stage_name(&stage_id));
        let alias = fixture.root.join("external-hardlink.txt");
        fs::hard_link(root.join("payload/src/lib.rs"), &alias).expect("create hardlink alias");
        assert!(staged.readback().is_err());
        fs::remove_file(alias).expect("remove hardlink alias");
        let redirect = root.join("payload/redirect");
        if std::os::windows::fs::symlink_dir(&fixture.export, &redirect).is_ok() {
            assert!(staged.readback().is_err());
            fs::remove_dir(&redirect).expect("remove directory symlink");
        }
        assert!(staged.readback().is_ok());
        drop(staged);
        fixture.remove(Some(&stage_id));
    }

    #[test]
    fn changed_host_allowlist_fails_before_creating_a_stage() {
        let fixture = Fixture::new();
        let guard = fixture.private_root();
        let mut changed = fixture.expected.clone();
        changed.allowed_paths.remove("README.md");
        assert!(stage_verified_sol_worker_output(&guard, &changed, fixture.burned()).is_err());
        assert!(
            query_relative_directory_names_bounded(&guard.guard, 0)
                .expect("empty staging root")
                .is_empty()
        );
        drop(guard);
        fixture.remove(None);
    }

    #[test]
    fn verified_bytes_cannot_be_rebound_to_another_host_task_or_turn() {
        for changed_field in ["task", "baseline", "turn", "nonce"] {
            let fixture = Fixture::new();
            let guard = fixture.private_root();
            let mut changed = fixture.expected.clone();
            match changed_field {
                "task" => changed.task_id = "T-other".into(),
                "baseline" => changed.baseline_commit = "b".repeat(40),
                "turn" => changed.native_turn_id = "different-turn".into(),
                "nonce" => changed.handoff_nonce = Uuid::new_v4().to_string(),
                _ => unreachable!(),
            }
            assert!(stage_verified_sol_worker_output(&guard, &changed, fixture.burned()).is_err());
            assert!(
                query_relative_directory_names_bounded(&guard.guard, 0)
                    .expect("no created stages")
                    .is_empty()
            );
            drop(guard);
            fixture.remove(None);
        }
    }

    #[test]
    fn stage_identifier_is_internal_canonical_v4_only() {
        for invalid in ["", "../outside", "0".repeat(32).as_str(), "not-a-uuid"] {
            assert!(!valid_stage_id(invalid));
        }
        let id = Uuid::new_v4().simple().to_string();
        assert!(valid_stage_id(&id));
        assert!(stage_name(&id).starts_with("sol-stage-"));
    }

    #[test]
    fn root_acl_rejects_broad_and_same_worker_identity() {
        let fixture = Fixture::new();
        let host_sid = security::current_host_sid().expect("host SID");
        let private = fixture.private_root();
        assert!(audit_sol_stage_root_acl_for_supplied_sid(&private.guard, &host_sid).is_err());
        assert!(audit_sol_stage_root_acl_for_supplied_sid(&private.guard, "not-a-sid").is_err());
        let plain = ProtectedDirectoryGuard::acquire(&fixture.staging, LABEL)
            .expect("ordinary directory handle without READ_CONTROL");
        assert!(audit_sol_stage_root_acl_for_supplied_sid(&plain, TEST_WORKER_SID).is_err());
        drop(plain);
        drop(private);

        let base = ProtectedDirectoryGuard::acquire(&fixture.root, LABEL).expect("pin base");
        let host_ace = format!("(A;OICI;FA;;;{host_sid})");
        let system_ace = if host_sid == "S-1-5-18" {
            String::new()
        } else {
            "(A;OICI;FA;;;SY)".into()
        };
        let broad = format!("O:{host_sid}G:{host_sid}D:P{host_ace}{system_ace}(A;OICI;FA;;;WD)");
        let name = "broad-root";
        let mut created = base.try_clone(LABEL).expect("clone base");
        created
            .create_child_with_security_descriptor(name, &broad, LABEL)
            .expect("create hostile test root");
        let mut security_open = base.try_clone(LABEL).expect("clone base");
        security_open
            .descend_existing_for_security(name, LABEL)
            .expect("security reopen hostile root");
        security::same_opened_directory(&created, &security_open).expect("same opened object");
        assert!(
            audit_sol_stage_root_acl_for_supplied_sid(&security_open, TEST_WORKER_SID).is_err()
        );
        drop(security_open);
        drop(created);
        fs::remove_dir(fixture.root.join(name)).expect("remove exact hostile test root");
        drop(base);
        fixture.remove(None);
    }

    #[test]
    fn child_acl_rejects_worker_or_inherited_broad_ace_and_wrong_object() {
        let fixture = Fixture::new();
        let private = fixture.private_root();
        let host_sid = security::current_host_sid().expect("host SID");
        let host_ace = format!("(A;OICI;FA;;;{host_sid})");
        let system_ace = if host_sid == "S-1-5-18" {
            String::new()
        } else {
            "(A;OICI;FA;;;SY)".into()
        };
        for (name, extra_ace) in [
            ("worker-child", format!("(A;OICI;FA;;;{TEST_WORKER_SID})")),
            ("inherited-broad-child", "(A;OICIID;FA;;;WD)".into()),
        ] {
            let sddl = format!("O:{host_sid}G:{host_sid}D:P{host_ace}{system_ace}{extra_ace}");
            let mut created = private.guard.try_clone(LABEL).expect("clone parent");
            created
                .create_child_with_security_descriptor(name, &sddl, LABEL)
                .expect("create hostile test child");
            let mut security_open = private.guard.try_clone(LABEL).expect("clone parent");
            security_open
                .descend_existing_for_security(name, LABEL)
                .expect("security reopen hostile child");
            security::same_opened_directory(&created, &security_open).expect("same opened object");
            assert!(
                security::verify_private_child(&private.guard, &security_open, TEST_WORKER_SID,)
                    .is_err()
            );
            drop(security_open);
            drop(created);
            fs::remove_dir(fixture.staging.join(name)).expect("remove exact hostile child");
        }

        let mut first = private.guard.try_clone(LABEL).expect("clone parent");
        first
            .create_child("first-child", LABEL)
            .expect("create first child");
        let mut second = private.guard.try_clone(LABEL).expect("clone parent");
        second
            .create_child("second-child", LABEL)
            .expect("create second child");
        let mut security_open = private.guard.try_clone(LABEL).expect("clone parent");
        security_open
            .descend_existing_for_security("second-child", LABEL)
            .expect("reopen second child");
        assert!(security::same_opened_directory(&first, &security_open).is_err());
        drop(security_open);
        drop(second);
        drop(first);
        drop(private);
        fs::remove_dir(fixture.staging.join("first-child")).expect("remove first child");
        fs::remove_dir(fixture.staging.join("second-child")).expect("remove second child");
        fixture.remove(None);
    }
}
