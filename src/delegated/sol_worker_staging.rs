//! Dormant host-side staging for a verified, stopped Sol worker export.
//!
//! The caller must provide a pinned root whose Windows ACL was independently
//! proved to exclude the restricted worker identity and every worker child.
//! This module cannot establish that process/token/ACL proof; it is not wired
//! to Sol launch or repository mutation. It creates one fresh child beneath
//! the pinned root and leaves incomplete children inert for later diagnosis.
//! The post-burn value is memory-only: a crash or staging failure after burn
//! leaves the nonce spent and requires operator attention, not an automatic
//! retry or a newly issued nonce.
//! Synced files and readback do not prove parent-directory metadata survives
//! sudden power loss.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::reviewed_source_snapshot::query_relative_directory_names_bounded;
use crate::windows_protected_fs::{
    ProtectedDirectoryGuard, read_relative_regular_unique, write_new_regular_in,
};

use super::sol_worker_nonce::BurnedSolWorkerOutputV1;
use super::sol_worker_output::{ExpectedSolWorkerOutputV1, SolWorkerOutputFileV1};

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
pub(crate) struct PinnedSolWorkerStageV1 {
    guard: ProtectedDirectoryGuard,
    manifest: SolStagedOutputManifestV1,
}

impl PinnedSolWorkerStageV1 {
    pub(crate) fn manifest(&self) -> &SolStagedOutputManifestV1 {
        &self.manifest
    }

    pub(crate) fn readback(&self) -> Result<BTreeMap<String, Vec<u8>>, String> {
        readback(&self.guard, &self.manifest)
    }
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

/// Stage host-owned bytes only after the nonce burn path returns its distinct
/// output type, below an independently proved host-private parent. This
/// function deliberately accepts no worker-supplied path or stage ID. It
/// never opens an existing stage, retries a collision, removes crash residue,
/// mutates the repository, or grants Sol launch authority.
pub(crate) fn stage_verified_sol_worker_output(
    host_private_root: &ProtectedDirectoryGuard,
    expected: &ExpectedSolWorkerOutputV1,
    burned: BurnedSolWorkerOutputV1,
) -> Result<PinnedSolWorkerStageV1, String> {
    let stage_id = Uuid::new_v4().simple().to_string();
    stage_verified_sol_worker_output_with_id(host_private_root, expected, burned, &stage_id)
}

fn stage_verified_sol_worker_output_with_id(
    host_private_root: &ProtectedDirectoryGuard,
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
    host_private_root.assert_stable(LABEL)?;
    let mut stage = host_private_root.try_clone(LABEL)?;
    stage.create_child(&stage_name(stage_id), LABEL)?;
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
        guard: stage,
        manifest,
    };
    if staged.readback()? != files {
        return Err(format!("{LABEL} readback differs from verified output"));
    }
    host_private_root.assert_stable(LABEL)?;
    Ok(staged)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    use super::super::codex_model_policy::{CodexCliTurnProfileEvidenceV1, SOL_MEDIUM_MODEL};
    use super::super::sol_worker_nonce::{
        SolWorkerNonceAuthorityV1, bind_sol_worker_native_turn, issue_sol_worker_nonce,
        verify_and_burn_sol_worker_output,
    };
    use super::super::sol_worker_output::SolWorkerOutputManifestV1;

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
            fs::create_dir(&staging).expect("fresh staging root");
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
            let guard =
                ProtectedDirectoryGuard::acquire(&self.staging, LABEL).expect("pin staging");
            stage_verified_sol_worker_output(&guard, &self.expected, self.burned())
                .expect("stage exact export")
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
        let guard = ProtectedDirectoryGuard::acquire(&fixture.staging, LABEL).expect("pin root");
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
        let guard = ProtectedDirectoryGuard::acquire(&fixture.staging, LABEL).expect("pin root");
        let mut changed = fixture.expected.clone();
        changed.allowed_paths.remove("README.md");
        assert!(stage_verified_sol_worker_output(&guard, &changed, fixture.burned()).is_err());
        assert!(
            query_relative_directory_names_bounded(&guard, 0)
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
            let guard =
                ProtectedDirectoryGuard::acquire(&fixture.staging, LABEL).expect("pin root");
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
                query_relative_directory_names_bounded(&guard, 0)
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
}
