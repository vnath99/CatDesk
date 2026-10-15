//! Dormant host-side verifier for a stopped Sol worker's output directory.
//!
//! The directory is an export containing only changed regular files, not a
//! worker worktree. A future restricted-service launcher must prove that the
//! worker has stopped and cannot write this directory before calling here.
//! This module does not create that process/ACL proof or enable Sol launch.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use crate::reviewed_source_snapshot::query_relative_directory_names_bounded;
use crate::windows_protected_fs::{ProtectedDirectoryGuard, read_relative_regular_unique};

const LABEL: &str = "Sol worker output";
const MAX_FILES: usize = 256;
const MAX_DEPTH: usize = 12;
const MAX_PATH_BYTES: usize = 512;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// This expectation must come from the trusted host's task/native-turn state,
/// never from the worker-supplied manifest. `allowed_paths` is the exact
/// contract allowlist, not a worker-selected expansion of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpectedSolWorkerOutputV1 {
    pub task_id: String,
    pub baseline_commit: String,
    pub native_turn_id: String,
    pub handoff_nonce: String,
    pub allowed_paths: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SolWorkerOutputManifestV1 {
    pub schema_version: u32,
    pub task_id: String,
    pub baseline_commit: String,
    pub native_turn_id: String,
    pub handoff_nonce: String,
    pub files: Vec<SolWorkerOutputFileV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SolWorkerOutputFileV1 {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// Parse worker-controlled metadata only after a byte limit. The binding and
/// file-set checks below are still required after this syntax check.
pub(crate) fn parse_sol_worker_output_manifest(
    bytes: &[u8],
) -> Result<SolWorkerOutputManifestV1, String> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(format!("{LABEL} manifest is oversized"));
    }
    serde_json::from_slice(bytes).map_err(|_| format!("{LABEL} manifest is invalid"))
}

/// The only output of the verifier. File bytes and the exact host expectation
/// are owned by the host and consumed once by a later staging operation.
pub(crate) struct VerifiedSolWorkerOutputV1 {
    expected: ExpectedSolWorkerOutputV1,
    files: BTreeMap<String, Vec<u8>>,
}

impl VerifiedSolWorkerOutputV1 {
    pub(crate) fn into_files(self) -> BTreeMap<String, Vec<u8>> {
        self.files
    }

    /// Preserve the exact host authority used at verification. A later
    /// staging caller may not relabel verified bytes for a different task,
    /// baseline, native turn, nonce, or path allowlist.
    pub(crate) fn into_parts(self) -> (ExpectedSolWorkerOutputV1, BTreeMap<String, Vec<u8>>) {
        (self.expected, self.files)
    }
}

fn valid_slug(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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
    let stem = component.split('.').next().unwrap_or_default();
    let stem = stem.to_ascii_uppercase();
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

fn valid_relative_path(path: &str) -> bool {
    path.len() <= MAX_PATH_BYTES
        && path.split('/').count() <= MAX_DEPTH
        && path.split('/').all(valid_component)
}

fn validate_manifest(
    expected: &ExpectedSolWorkerOutputV1,
    manifest: &SolWorkerOutputManifestV1,
) -> Result<BTreeSet<String>, String> {
    if manifest.schema_version != 1
        || !valid_slug(&expected.task_id, 128)
        || !valid_commit(&expected.baseline_commit)
        || !valid_slug(&expected.native_turn_id, 128)
        || !valid_slug(&expected.handoff_nonce, 128)
        || manifest.task_id != expected.task_id
        || manifest.baseline_commit != expected.baseline_commit
        || manifest.native_turn_id != expected.native_turn_id
        || manifest.handoff_nonce != expected.handoff_nonce
        || manifest.files.len() > MAX_FILES
        || expected.allowed_paths.len() > MAX_FILES
        || expected
            .allowed_paths
            .iter()
            .any(|path| !valid_relative_path(path))
    {
        return Err(format!("{LABEL} binding is invalid"));
    }
    let mut paths = BTreeSet::new();
    let mut folded = BTreeSet::new();
    let mut total = 0_u64;
    for file in &manifest.files {
        if !valid_relative_path(&file.path)
            || !expected.allowed_paths.contains(&file.path)
            || !valid_digest(&file.sha256)
            || file.size > MAX_FILE_BYTES
        {
            return Err(format!("{LABEL} manifest is unsafe"));
        }
        total = total
            .checked_add(file.size)
            .ok_or_else(|| format!("{LABEL} size overflow"))?;
        if total > MAX_TOTAL_BYTES
            || !paths.insert(file.path.clone())
            || !folded.insert(file.path.to_ascii_lowercase())
        {
            return Err(format!("{LABEL} manifest is ambiguous or oversized"));
        }
    }
    if expected
        .allowed_paths
        .iter()
        .map(|path| path.to_ascii_lowercase())
        .collect::<BTreeSet<_>>()
        .len()
        != expected.allowed_paths.len()
    {
        return Err(format!("{LABEL} allowlist is ambiguous"));
    }
    Ok(paths)
}

fn inventory(
    root: &ProtectedDirectoryGuard,
    prefix: &str,
    depth: usize,
    files: &mut BTreeSet<String>,
) -> Result<(), String> {
    if depth >= MAX_DEPTH {
        return Err(format!("{LABEL} tree is too deep"));
    }
    root.assert_stable(LABEL)?;
    for (name, attributes) in query_relative_directory_names_bounded(root, MAX_FILES)? {
        if !valid_component(&name) || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!("{LABEL} tree is unsafe"));
        }
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if !valid_relative_path(&path) {
            return Err(format!("{LABEL} tree is unsafe"));
        }
        if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            let mut child = root.try_clone(LABEL)?;
            child.descend_existing(&name, LABEL)?;
            let before = files.len();
            inventory(&child, &path, depth + 1, files)?;
            if files.len() == before {
                return Err(format!("{LABEL} contains an unmanifested empty directory"));
            }
        } else if files.len() >= MAX_FILES || !files.insert(path) {
            return Err(format!("{LABEL} tree is ambiguous or oversized"));
        }
    }
    root.assert_stable(LABEL)
}

/// Verify the complete export against host-held run binding and preserve the
/// exact bytes read through no-follow, unique-link handles. The caller must
/// first prove that the separate worker has exited and lost write authority;
/// that OS proof and durable nonce replay ledger are not implemented here.
pub(crate) fn verify_exact_sol_worker_output(
    output_root: &ProtectedDirectoryGuard,
    expected: &ExpectedSolWorkerOutputV1,
    manifest: &SolWorkerOutputManifestV1,
) -> Result<VerifiedSolWorkerOutputV1, String> {
    let manifest_paths = validate_manifest(expected, manifest)?;
    // The service/host keeps this exact directory object pinned across the
    // worker run. Reopening an untrusted path at handoff would lose that bind.
    let root = output_root.try_clone(LABEL)?;
    let mut observed = BTreeSet::new();
    inventory(&root, "", 0, &mut observed)?;
    if observed != manifest_paths {
        return Err(format!("{LABEL} inventory differs from manifest"));
    }
    let mut files = BTreeMap::new();
    for entry in &manifest.files {
        let mut parent = root.try_clone(LABEL)?;
        let mut components = entry.path.split('/').collect::<Vec<_>>();
        let leaf = components
            .pop()
            .ok_or_else(|| format!("{LABEL} path is invalid"))?;
        for component in components {
            parent.descend_existing(component, LABEL)?;
        }
        let bytes = read_relative_regular_unique(&parent, leaf, entry.size, LABEL)?;
        if bytes.len() as u64 != entry.size
            || format!("{:x}", Sha256::digest(&bytes)) != entry.sha256.to_ascii_lowercase()
        {
            return Err(format!("{LABEL} content differs from manifest"));
        }
        files.insert(entry.path.clone(), bytes);
    }
    observed.clear();
    inventory(&root, "", 0, &mut observed)?;
    if observed != manifest_paths {
        return Err(format!("{LABEL} inventory changed while verifying"));
    }
    Ok(VerifiedSolWorkerOutputV1 {
        expected: expected.clone(),
        files,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use uuid::Uuid;

    fn verify(
        root: &Path,
        expected: &ExpectedSolWorkerOutputV1,
        manifest: &SolWorkerOutputManifestV1,
    ) -> Result<VerifiedSolWorkerOutputV1, String> {
        let pinned = ProtectedDirectoryGuard::acquire(root, LABEL)?;
        verify_exact_sol_worker_output(&pinned, expected, manifest)
    }

    fn fixture() -> (
        std::path::PathBuf,
        ExpectedSolWorkerOutputV1,
        SolWorkerOutputManifestV1,
    ) {
        let root = std::env::temp_dir().join(format!("catdesk-sol-output-{}", Uuid::new_v4()));
        fs::create_dir(&root).expect("isolated root");
        fs::create_dir(root.join("src")).expect("nested output");
        fs::write(root.join("src").join("lib.rs"), b"changed\n").expect("output");
        let expected = ExpectedSolWorkerOutputV1 {
            task_id: "T-0483".into(),
            baseline_commit: "a".repeat(40),
            native_turn_id: "turn-123".into(),
            handoff_nonce: Uuid::new_v4().to_string(),
            allowed_paths: BTreeSet::from(["src/lib.rs".into()]),
        };
        let manifest = SolWorkerOutputManifestV1 {
            schema_version: 1,
            task_id: expected.task_id.clone(),
            baseline_commit: expected.baseline_commit.clone(),
            native_turn_id: expected.native_turn_id.clone(),
            handoff_nonce: expected.handoff_nonce.clone(),
            files: vec![SolWorkerOutputFileV1 {
                path: "src/lib.rs".into(),
                size: 8,
                sha256: format!("{:x}", Sha256::digest(b"changed\n")),
            }],
        };
        (root, expected, manifest)
    }

    fn remove_base_fixture(root: &Path) {
        fs::remove_file(root.join("src").join("lib.rs")).expect("remove fixture output");
        fs::remove_dir(root.join("src")).expect("remove fixture child directory");
        fs::remove_dir(root).expect("remove fixture root");
    }

    #[test]
    fn exact_nested_output_returns_host_owned_bytes() {
        let (root, expected, manifest) = fixture();
        let output = verify(&root, &expected, &manifest).expect("verify");
        fs::write(root.join("src").join("lib.rs"), b"changed after capture")
            .expect("worker path can change after capture");
        assert_eq!(output.into_files()["src/lib.rs"], b"changed\n");
        remove_base_fixture(&root);
    }

    #[test]
    fn binding_manifest_and_inventory_must_match_exactly() {
        let (root, expected, manifest) = fixture();
        let mut wrong = manifest.clone();
        wrong.handoff_nonce = "different".into();
        assert!(verify(&root, &expected, &wrong).is_err());
        wrong = manifest.clone();
        wrong.baseline_commit = "b".repeat(40);
        assert!(verify(&root, &expected, &wrong).is_err());
        wrong = manifest.clone();
        wrong.native_turn_id = "other-turn".into();
        assert!(verify(&root, &expected, &wrong).is_err());
        wrong = manifest.clone();
        wrong.files[0].sha256 = "0".repeat(64);
        assert!(verify(&root, &expected, &wrong).is_err());
        fs::write(root.join("extra.rs"), b"unexpected").expect("extra output");
        assert!(verify(&root, &expected, &manifest).is_err());
        fs::remove_file(root.join("extra.rs")).expect("remove extra output");
        remove_base_fixture(&root);
    }

    #[test]
    fn aliases_hardlinks_and_unlisted_directories_fail_closed() {
        let (root, expected, manifest) = fixture();
        let mut wrong = manifest.clone();
        wrong.files[0].path = "src/../lib.rs".into();
        assert!(verify(&root, &expected, &wrong).is_err());
        wrong = manifest.clone();
        wrong.files.push(SolWorkerOutputFileV1 {
            path: "src/LIB.rs".into(),
            ..wrong.files[0].clone()
        });
        let mut alias_allowed = expected.clone();
        alias_allowed.allowed_paths.insert("src/LIB.rs".into());
        assert!(verify(&root, &alias_allowed, &wrong).is_err());
        let alias = root.with_extension("hardlink");
        fs::hard_link(root.join("src").join("lib.rs"), &alias).expect("hardlink fixture");
        assert!(verify(&root, &expected, &manifest).is_err());
        fs::remove_file(alias).expect("remove alias");
        fs::create_dir(root.join("empty")).expect("empty unlisted directory");
        assert!(verify(&root, &expected, &manifest).is_err());
        fs::remove_dir(root.join("empty")).expect("remove empty directory");
        remove_base_fixture(&root);
    }

    #[test]
    fn worker_manifest_parser_is_bounded_and_strict() {
        let (root, _expected, manifest) = fixture();
        let bytes = serde_json::to_vec(&manifest).expect("manifest JSON");
        assert_eq!(
            parse_sol_worker_output_manifest(&bytes).expect("parse"),
            manifest
        );
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON value");
        value["workerSelectedRoot"] = serde_json::json!("C:\\authority");
        let unknown = serde_json::to_vec(&value).expect("unknown field JSON");
        assert!(parse_sol_worker_output_manifest(&unknown).is_err());
        assert!(parse_sol_worker_output_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1]).is_err());
        remove_base_fixture(&root);
    }

    #[test]
    fn overfull_output_directory_is_rejected_before_unbounded_inventory() {
        let (root, expected, manifest) = fixture();
        for number in 0..=MAX_FILES {
            fs::write(root.join(format!("extra-{number}.txt")), b"x").expect("extra output");
        }
        let pinned = ProtectedDirectoryGuard::acquire(&root, LABEL).expect("pinned output");
        assert!(query_relative_directory_names_bounded(&pinned, MAX_FILES).is_err());
        assert!(verify_exact_sol_worker_output(&pinned, &expected, &manifest).is_err());
        drop(pinned);
        for number in 0..=MAX_FILES {
            fs::remove_file(root.join(format!("extra-{number}.txt"))).expect("remove extra output");
        }
        remove_base_fixture(&root);
    }
}
