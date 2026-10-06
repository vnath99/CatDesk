//! Immutable reviewed-source byte snapshots.
//!
//! This is the single authority for the completion producer and the reviewed
//! promotion consumer.  A snapshot is never inferred from a mutable workspace
//! after completion: a committed directory is either exactly valid for the
//! requested completion evidence or it is rejected.

use std::collections::BTreeSet;
#[cfg(windows)]
use std::ffi::c_void;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle};

#[cfg(windows)]
use crate::windows_protected_fs::{
    CloseHandle, IO_STATUS_BLOCK, UNICODE_STRING, handle_information,
    nt_open_relative_with_error_domain,
};
use crate::windows_protected_fs::{ProtectedDirectoryGuard, validate_protected_component};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const REVIEWED_SOURCE_SNAPSHOT_SCHEMA_VERSION: u32 = 4;
pub const MAX_SNAPSHOT_PATH_BYTES: usize = 512;
pub const MAX_SNAPSHOT_ENTRIES: usize = 4096;
pub const MAX_SNAPSHOT_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_SNAPSHOT_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

const AUTHORITY_DOMAIN: &str = "CATDESK_REVIEWED_SOURCE_AUTHORITY_V1";
const MANIFEST_DOMAIN: &str = "CATDESK_REVIEWED_SOURCE_MANIFEST_V1";
const SNAPSHOT_ID_DOMAIN: &str = "CATDESK_REVIEWED_SOURCE_SNAPSHOT_ID_V1";

#[cfg(test)]
type CleanupDispositionHook = Box<dyn Fn(&Path, bool) + Send + Sync>;
#[cfg(test)]
static CLEANUP_DISPOSITION_HOOK: OnceLock<Mutex<Option<CleanupDispositionHook>>> = OnceLock::new();
#[cfg(test)]
static STAGING_RECLAIM_OPEN_HOOK: OnceLock<Mutex<Option<CleanupDispositionHook>>> = OnceLock::new();
#[cfg(test)]
static CLEANUP_HOOK_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
fn invoke_cleanup_disposition_hook(path: &Path, is_directory: bool) {
    let lock = CLEANUP_DISPOSITION_HOOK.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = lock.lock() {
        if let Some(hook) = guard.as_ref() {
            hook(path, is_directory);
        }
    }
}

#[cfg(test)]
fn replace_cleanup_disposition_hook(hook: Option<CleanupDispositionHook>) {
    let lock = CLEANUP_DISPOSITION_HOOK.get_or_init(|| Mutex::new(None));
    *lock.lock().expect("cleanup hook lock") = hook;
}

#[cfg(test)]
fn invoke_staging_reclaim_open_hook(path: &Path) {
    let lock = STAGING_RECLAIM_OPEN_HOOK.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = lock.lock() {
        if let Some(hook) = guard.as_ref() {
            hook(path, true);
        }
    }
}

#[cfg(test)]
fn replace_staging_reclaim_open_hook(hook: Option<CleanupDispositionHook>) {
    let lock = STAGING_RECLAIM_OPEN_HOOK.get_or_init(|| Mutex::new(None));
    *lock.lock().expect("reclaim hook lock") = hook;
}

#[cfg(test)]
fn cleanup_hook_test_lock() -> &'static Mutex<()> {
    CLEANUP_HOOK_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const DELETE: u32 = 0x0001_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const FILE_SHARE_DELETE: u32 = 0x0000_0004;
const FILE_OPEN: u32 = 1;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
const FILE_NON_DIRECTORY_FILE: u32 = 0x0000_0040;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_DIRECTORY_INFORMATION_CLASS: u32 = 1;
const FILE_DISPOSITION_INFORMATION_CLASS: u32 = 13;
const FILE_DISPOSITION_INFORMATION_EX_CLASS: u32 = 64;
const FILE_DISPOSITION_DELETE: u32 = 0x0000_0001;
#[cfg(windows)]
unsafe extern "system" {
    fn NtSetInformationFile(
        file_handle: *mut c_void,
        io_status_block: *mut IO_STATUS_BLOCK,
        file_information: *mut c_void,
        length: u32,
        file_information_class: u32,
    ) -> i32;
    fn NtQueryDirectoryFile(
        file_handle: *mut c_void,
        event: *mut c_void,
        apc_routine: *mut c_void,
        apc_context: *mut c_void,
        io_status_block: *mut IO_STATUS_BLOCK,
        file_information: *mut c_void,
        length: u32,
        file_information_class: u32,
        return_single_entry: i32,
        file_name: *mut UNICODE_STRING,
        restart_scan: i32,
    ) -> i32;
}

/// Commits the exact opened staging directory into the exact opened snapshot
/// root.  The destination name is evaluated under `RootDirectory`, not under
/// a mutable root pathname.
fn rename_pinned_staging(
    staging: &mut ProtectedDirectoryGuard,
    destination_root: &ProtectedDirectoryGuard,
    destination_name: &str,
) -> Result<(), String> {
    // Snapshot code owns commit/racing-winner policy; the shared protected-FS
    // layer owns only the exact-handle same-parent native rename authority.
    staging.rename_direct_child_within_parent(
        destination_root,
        destination_name,
        "reviewed source snapshot commit",
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewedSourceSnapshotExpectedV1 {
    pub session_id: String,
    pub project_id: String,
    pub approved_contract_hash: String,
    pub logical_task_id: String,
    pub completion_artifact_ids: Vec<String>,
    pub current_outputs: Vec<ReviewedSourceCurrentOutputV1>,
    pub baseline_observations: Vec<ReviewedSourceBaselineObservationV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedSourceCurrentOutputV1 {
    pub artifact_id: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedSourceBaselineObservationV1 {
    pub artifact_id: String,
    pub state: String,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedSourceEntryV1 {
    pub relative_path: String,
    pub byte_length: u64,
    pub sha256: String,
    pub content_object: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedSourceSnapshotV1 {
    pub schema_version: u32,
    pub authority_domain: String,
    pub manifest_domain: String,
    pub snapshot_id_domain: String,
    pub session_id: String,
    pub project_id: String,
    pub approved_contract_hash: String,
    pub logical_task_id: String,
    pub completion_artifact_ids: Vec<String>,
    pub current_outputs: Vec<ReviewedSourceCurrentOutputV1>,
    pub baseline_observations: Vec<ReviewedSourceBaselineObservationV1>,
    pub authority_digest: String,
    pub entries: Vec<ReviewedSourceEntryV1>,
    pub manifest_digest: String,
    pub snapshot_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedReviewedSourceSnapshotV1 {
    pub snapshot_id: String,
    pub authority_digest: String,
    pub manifest_digest: String,
    pub entries: Vec<ReviewedSourceEntryV1>,
    pub bytes_root: PathBuf,
}

/// Creates the exact byte snapshot only when no committed snapshot exists.
/// A valid committed replay does not inspect mutable workspace source files.
pub fn create_or_validate_reviewed_source_snapshot(
    workspace: &Path,
    expected: &ReviewedSourceSnapshotExpectedV1,
) -> Result<ValidatedReviewedSourceSnapshotV1, String> {
    let expected = canonical_expected(expected)?;
    let root = protected_snapshot_root(workspace, true)?;
    reclaim_stale_current_session_staging(&root, &expected.session_id)?;
    let final_dir = protected_snapshot_session_directory(root.path(), &expected.session_id)?;
    match fs::symlink_metadata(&final_dir) {
        Ok(_) => return validate_committed_snapshot(workspace, &expected),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err("reviewed source snapshot is unavailable".into()),
    }

    let inputs = collect_release_inputs(workspace)?;
    let staging_guard = create_owned_staging(&root, &expected.session_id)?;
    let bytes_guard = pin_create_relative_directories(&staging_guard, "bytes")?;

    let written = (|| {
        let mut entries = Vec::with_capacity(inputs.len());
        for relative in inputs {
            let source = safe_workspace_file(workspace, &relative)?;
            let metadata = fs::symlink_metadata(&source)
                .map_err(|_| "reviewed source snapshot is unavailable")?;
            let length = metadata.len();
            if length > MAX_SNAPSHOT_FILE_BYTES {
                return Err("reviewed source snapshot exceeds a file bound".into());
            }
            let bytes = fs::read(&source).map_err(|_| "reviewed source snapshot is unavailable")?;
            if bytes.len() as u64 != length {
                return Err("reviewed source snapshot source changed while reading".into());
            }
            let content_object = format!("bytes/{relative}");
            let relative_object = content_object
                .strip_prefix("bytes/")
                .ok_or_else(|| "reviewed source snapshot content is unsafe".to_string())?;
            let object_path = Path::new(relative_object);
            let object_name = object_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "reviewed source snapshot content is unsafe".to_string())?;
            let parent_relative = object_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(normalize_relative_path)
                .transpose()?
                .map(|parent| format!("bytes/{parent}"))
                .unwrap_or_else(|| "bytes".to_string());
            let parent_guard = pin_create_relative_directories(&staging_guard, &parent_relative)?;
            write_new_regular_in(
                &parent_guard,
                object_name,
                &bytes,
                "reviewed source snapshot content",
            )?;
            entries.push(ReviewedSourceEntryV1 {
                relative_path: relative,
                byte_length: length,
                sha256: sha256(&bytes),
                content_object,
            });
        }
        validate_entries(&entries)?;
        let manifest = build_manifest(&expected, entries)?;
        let manifest_bytes =
            serde_json::to_vec(&manifest).map_err(|_| "reviewed source snapshot is unavailable")?;
        staging_guard.assert_stable("reviewed source snapshot staging")?;
        write_new_regular_in(
            &staging_guard,
            "manifest.json",
            &manifest_bytes,
            "reviewed source snapshot manifest",
        )?;
        validate_snapshot_directory(&staging_guard, &expected)
    })();

    let validated = match written {
        Ok(value) => value,
        Err(error) => {
            let _ = remove_owned_staging(&root, &staging_guard, &expected.session_id);
            return Err(error);
        }
    };
    // Descendants are intentionally non-delete-share pinned while writing.
    // Release the bytes-root handle only after validation, so the unique
    // renameable staging identity can be committed under the pinned root.
    drop(bytes_guard);
    root.assert_stable("reviewed source snapshot root")?;
    staging_guard.assert_stable("reviewed source snapshot staging")?;
    let mut committed_guard = staging_guard;
    match rename_pinned_staging(&mut committed_guard, &root, &expected.session_id) {
        Ok(()) => validate_snapshot_directory(&committed_guard, &expected),
        Err(error) => {
            // A racing writer may only win when it committed the same exact
            // authority.  Staging itself never becomes authority.
            validate_committed_snapshot(workspace, &expected).map_err(|_| {
                let _ = validated;
                format!("reviewed source snapshot commit is ambiguous: {error}")
            })
        }
    }
}

/// Strictly validates an already committed snapshot.  Legacy `<session>.json`
/// files are deliberately ignored because only the directory manifest carries
/// byte-object authority.
pub fn validate_committed_snapshot(
    workspace: &Path,
    expected: &ReviewedSourceSnapshotExpectedV1,
) -> Result<ValidatedReviewedSourceSnapshotV1, String> {
    let expected = canonical_expected(expected)?;
    let mut root = protected_snapshot_root(workspace, false)?;
    root.descend_existing(&expected.session_id, "reviewed source snapshot session")?;
    root.assert_stable("reviewed source snapshot session")?;
    validate_snapshot_directory(&root, &expected)
}

fn protected_snapshot_session_directory(root: &Path, session_id: &str) -> Result<PathBuf, String> {
    if session_id.is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("reviewed source snapshot session is unsafe".into());
    }
    Ok(root.join(session_id))
}

fn canonical_expected(
    expected: &ReviewedSourceSnapshotExpectedV1,
) -> Result<ReviewedSourceSnapshotExpectedV1, String> {
    if expected.session_id.is_empty()
        || expected.project_id != "catdesk"
        || expected.approved_contract_hash.is_empty()
        || expected.logical_task_id.is_empty()
        || expected.completion_artifact_ids.is_empty()
    {
        return Err("reviewed source snapshot authority is unavailable".into());
    }
    let mut canonical = expected.clone();
    canonical.completion_artifact_ids.sort();
    if canonical
        .completion_artifact_ids
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err("reviewed source snapshot artifact authority is ambiguous".into());
    }
    for artifact in &canonical.completion_artifact_ids {
        validate_normalized_relative(artifact)?;
    }
    canonical
        .current_outputs
        .sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    canonical
        .baseline_observations
        .sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    let artifact_set: BTreeSet<_> = canonical.completion_artifact_ids.iter().cloned().collect();
    if canonical.current_outputs.len() != artifact_set.len()
        || canonical.baseline_observations.len() != artifact_set.len()
        || canonical
            .current_outputs
            .iter()
            .any(|value| !artifact_set.contains(&value.artifact_id) || !valid_sha256(&value.sha256))
        || canonical.baseline_observations.iter().any(|value| {
            !artifact_set.contains(&value.artifact_id)
                || !matches!(value.state.as_str(), "ABSENT" | "PRESENT_SHA256")
                || (value.state == "ABSENT" && value.sha256.is_some())
                || (value.state == "PRESENT_SHA256"
                    && !value.sha256.as_deref().is_some_and(valid_sha256))
        })
    {
        return Err("reviewed source snapshot output authority is unavailable".into());
    }
    if canonical
        .current_outputs
        .windows(2)
        .any(|pair| pair[0].artifact_id == pair[1].artifact_id)
        || canonical
            .baseline_observations
            .windows(2)
            .any(|pair| pair[0].artifact_id == pair[1].artifact_id)
    {
        return Err("reviewed source snapshot output authority is ambiguous".into());
    }
    Ok(canonical)
}

fn build_manifest(
    expected: &ReviewedSourceSnapshotExpectedV1,
    entries: Vec<ReviewedSourceEntryV1>,
) -> Result<ReviewedSourceSnapshotV1, String> {
    let authority_digest = authority_digest(expected)?;
    let mut manifest = ReviewedSourceSnapshotV1 {
        schema_version: REVIEWED_SOURCE_SNAPSHOT_SCHEMA_VERSION,
        authority_domain: AUTHORITY_DOMAIN.into(),
        manifest_domain: MANIFEST_DOMAIN.into(),
        snapshot_id_domain: SNAPSHOT_ID_DOMAIN.into(),
        session_id: expected.session_id.clone(),
        project_id: expected.project_id.clone(),
        approved_contract_hash: expected.approved_contract_hash.clone(),
        logical_task_id: expected.logical_task_id.clone(),
        completion_artifact_ids: expected.completion_artifact_ids.clone(),
        current_outputs: expected.current_outputs.clone(),
        baseline_observations: expected.baseline_observations.clone(),
        authority_digest,
        entries,
        manifest_digest: String::new(),
        snapshot_id: String::new(),
    };
    manifest.manifest_digest = manifest_digest(&manifest)?;
    manifest.snapshot_id = sha256(format!(
        "{SNAPSHOT_ID_DOMAIN}|{}|{}",
        manifest.authority_digest, manifest.manifest_digest
    ));
    Ok(manifest)
}

fn authority_digest(expected: &ReviewedSourceSnapshotExpectedV1) -> Result<String, String> {
    let payload = serde_json::to_vec(&(
        AUTHORITY_DOMAIN,
        REVIEWED_SOURCE_SNAPSHOT_SCHEMA_VERSION,
        &expected.session_id,
        &expected.project_id,
        &expected.approved_contract_hash,
        &expected.logical_task_id,
        &expected.completion_artifact_ids,
        &expected.current_outputs,
        &expected.baseline_observations,
    ))
    .map_err(|_| "reviewed source snapshot authority is unavailable")?;
    Ok(sha256(payload))
}

fn manifest_digest(manifest: &ReviewedSourceSnapshotV1) -> Result<String, String> {
    let payload = serde_json::to_vec(&(
        MANIFEST_DOMAIN,
        manifest.schema_version,
        &manifest.authority_digest,
        &manifest.entries,
    ))
    .map_err(|_| "reviewed source snapshot manifest is unavailable")?;
    Ok(sha256(payload))
}

fn validate_snapshot_directory(
    directory: &ProtectedDirectoryGuard,
    expected: &ReviewedSourceSnapshotExpectedV1,
) -> Result<ValidatedReviewedSourceSnapshotV1, String> {
    directory.assert_stable("reviewed source snapshot session")?;
    let bytes = read_relative_regular(
        directory,
        "manifest.json",
        MAX_SNAPSHOT_FILE_BYTES,
        "reviewed source snapshot manifest",
    )?;
    let manifest: ReviewedSourceSnapshotV1 =
        serde_json::from_slice(&bytes).map_err(|_| "reviewed source snapshot is malformed")?;
    let canonical = canonical_expected(expected)?;
    if manifest.schema_version != REVIEWED_SOURCE_SNAPSHOT_SCHEMA_VERSION
        || manifest.authority_domain != AUTHORITY_DOMAIN
        || manifest.manifest_domain != MANIFEST_DOMAIN
        || manifest.snapshot_id_domain != SNAPSHOT_ID_DOMAIN
        || manifest.session_id != canonical.session_id
        || manifest.project_id != canonical.project_id
        || manifest.approved_contract_hash != canonical.approved_contract_hash
        || manifest.logical_task_id != canonical.logical_task_id
        || manifest.completion_artifact_ids != canonical.completion_artifact_ids
        || manifest.current_outputs != canonical.current_outputs
        || manifest.baseline_observations != canonical.baseline_observations
        || manifest.authority_digest != authority_digest(&canonical)?
    {
        return Err("reviewed source snapshot authority mismatch".into());
    }
    validate_entries(&manifest.entries)?;
    if manifest.manifest_digest != manifest_digest(&manifest)?
        || manifest.snapshot_id
            != sha256(format!(
                "{SNAPSHOT_ID_DOMAIN}|{}|{}",
                manifest.authority_digest, manifest.manifest_digest
            ))
    {
        return Err("reviewed source snapshot digest mismatch".into());
    }
    let mut bytes_guard = directory.try_clone("reviewed source snapshot bytes")?;
    bytes_guard.descend_existing("bytes", "reviewed source snapshot bytes")?;
    validate_snapshot_bytes(&bytes_guard, &manifest.entries)?;
    let expected_objects: BTreeSet<_> = manifest
        .entries
        .iter()
        .map(|entry| entry.content_object.clone())
        .collect();
    let actual_objects = enumerate_regular_files(&bytes_guard, "bytes")?;
    if expected_objects != actual_objects {
        return Err("reviewed source snapshot content is incomplete".into());
    }
    Ok(ValidatedReviewedSourceSnapshotV1 {
        snapshot_id: manifest.snapshot_id,
        authority_digest: manifest.authority_digest,
        manifest_digest: manifest.manifest_digest,
        entries: manifest.entries,
        bytes_root: bytes_guard.path().to_path_buf(),
    })
}

fn validate_entries(entries: &[ReviewedSourceEntryV1]) -> Result<(), String> {
    if entries.is_empty() || entries.len() > MAX_SNAPSHOT_ENTRIES {
        return Err("reviewed source snapshot entry count is invalid".into());
    }
    let mut total = 0_u64;
    let mut normalized = BTreeSet::new();
    let mut case_folded = BTreeSet::new();
    let mut previous: Option<String> = None;
    for entry in entries {
        validate_normalized_relative(&entry.relative_path)?;
        if entry.content_object != format!("bytes/{}", entry.relative_path)
            || !valid_sha256(&entry.sha256)
            || entry.byte_length > MAX_SNAPSHOT_FILE_BYTES
        {
            return Err("reviewed source snapshot entry is unsafe".into());
        }
        if previous
            .as_ref()
            .is_some_and(|last| last >= &entry.relative_path)
        {
            return Err("reviewed source snapshot entries are not ordered".into());
        }
        previous = Some(entry.relative_path.clone());
        if !normalized.insert(entry.relative_path.clone())
            || !case_folded.insert(entry.relative_path.to_lowercase())
        {
            return Err("reviewed source snapshot path identity is ambiguous".into());
        }
        total = total
            .checked_add(entry.byte_length)
            .ok_or_else(|| "reviewed source snapshot exceeds its total bound".to_string())?;
        if total > MAX_SNAPSHOT_TOTAL_BYTES {
            return Err("reviewed source snapshot exceeds its total bound".into());
        }
    }
    Ok(())
}

fn validate_snapshot_bytes(
    root: &ProtectedDirectoryGuard,
    entries: &[ReviewedSourceEntryV1],
) -> Result<(), String> {
    root.assert_stable("reviewed source snapshot bytes")?;
    for entry in entries {
        let relative = entry
            .content_object
            .strip_prefix("bytes/")
            .ok_or_else(|| "reviewed source snapshot content is unsafe".to_string())?;
        let object = Path::new(relative);
        let name = object
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "reviewed source snapshot content is unsafe".to_string())?;
        let mut parent = root.try_clone("reviewed source snapshot content")?;
        if let Some(components) = object.parent() {
            for component in components.components() {
                let Component::Normal(component) = component else {
                    return Err("reviewed source snapshot content is unsafe".into());
                };
                parent.descend_existing(
                    component
                        .to_str()
                        .ok_or_else(|| "reviewed source snapshot content is unsafe".to_string())?,
                    "reviewed source snapshot content",
                )?;
            }
        }
        let bytes = read_relative_regular(
            &parent,
            name,
            entry.byte_length,
            "reviewed source snapshot content",
        )?;
        if bytes.len() as u64 != entry.byte_length {
            return Err("reviewed source snapshot content is unsafe".into());
        }
        if sha256(bytes) != entry.sha256 {
            return Err("reviewed source snapshot content hash mismatch".into());
        }
    }
    Ok(())
}

fn collect_release_inputs(workspace: &Path) -> Result<Vec<String>, String> {
    let mut input_set = BTreeSet::new();
    for required in ["Cargo.toml", "Cargo.lock", "wake/Cargo.toml"] {
        safe_workspace_file(workspace, required)?;
        input_set.insert(required.to_string());
    }
    for build_script in ["build.rs", "wake/build.rs"] {
        match fs::symlink_metadata(workspace.join(build_script)) {
            Ok(metadata) => {
                if unsafe_metadata(&metadata) || !metadata.file_type().is_file() {
                    return Err("reviewed source snapshot build script is unsafe".into());
                }
                input_set.insert(build_script.into());
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => {
                return Err("reviewed source snapshot build script is unavailable".into());
            }
        }
    }
    collect_tree(workspace, "src", &mut input_set)?;
    collect_tree(workspace, "wake/src", &mut input_set)?;
    let sources: Vec<_> = input_set
        .iter()
        .filter(|path| path.ends_with(".rs"))
        .cloned()
        .collect();
    for source in sources {
        let bytes = fs::read(safe_workspace_file(workspace, &source)?)
            .map_err(|_| "reviewed source snapshot coverage is unavailable")?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| "reviewed source snapshot coverage is ambiguous")?;
        let parent = Path::new(&source).parent().unwrap_or_else(|| Path::new(""));
        for literal_path in literal_include_paths(text)? {
            let joined = parent.join(literal_path);
            let normalized = normalize_relative_path(&joined)?;
            safe_workspace_file(workspace, &normalized)?;
            input_set.insert(normalized);
        }
    }
    if input_set.len() > MAX_SNAPSHOT_ENTRIES {
        return Err("reviewed source snapshot entry count is invalid".into());
    }
    Ok(input_set.into_iter().collect())
}

/// Extracts only syntactically literal `include_str!` and `include_bytes!`
/// invocations. Macro-shaped text in comments, ordinary strings, raw strings,
/// and this scanner's own source is not an invocation; a real non-literal or
/// malformed invocation remains ambiguous and therefore fails closed.
fn literal_include_paths(source: &str) -> Result<Vec<&str>, String> {
    if !source.contains("include_") {
        return Ok(Vec::new());
    }
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut paths = Vec::new();
    while cursor < bytes.len() {
        if bytes[cursor] == b'/' && bytes.get(cursor + 1) == Some(&b'/') {
            cursor = bytes[cursor + 2..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| cursor + 2 + offset + 1)
                .unwrap_or(bytes.len());
            continue;
        }
        if bytes[cursor] == b'/' && bytes.get(cursor + 1) == Some(&b'*') {
            cursor = skip_block_comment(bytes, cursor + 2)?;
            continue;
        }
        if let Some(next) = raw_string_end(bytes, cursor)? {
            cursor = next;
            continue;
        }
        if bytes[cursor] == b'\"' {
            cursor = skip_quoted_string(bytes, cursor)?;
            continue;
        }
        if bytes[cursor] == b'\'' {
            cursor = skip_character_literal(bytes, cursor);
            continue;
        }
        if !is_identifier_start(bytes[cursor]) {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        while cursor < bytes.len() && is_identifier_continue(bytes[cursor]) {
            cursor += 1;
        }
        let identifier = &source[start..cursor];
        if !matches!(identifier, "include_str" | "include_bytes") {
            continue;
        }
        let mut invocation = skip_ascii_whitespace(bytes, cursor);
        if bytes.get(invocation) != Some(&b'!') {
            continue;
        }
        invocation = skip_ascii_whitespace(bytes, invocation + 1);
        if bytes.get(invocation) != Some(&b'(') {
            return Err("reviewed source snapshot coverage is ambiguous".into());
        }
        invocation = skip_ascii_whitespace(bytes, invocation + 1);
        if bytes.get(invocation) != Some(&b'\"') {
            return Err("reviewed source snapshot coverage is ambiguous".into());
        }
        let literal_start = invocation + 1;
        let literal_end = literal_string_end(bytes, literal_start)?;
        if literal_end == literal_start || bytes[literal_start..literal_end].contains(&b'\\') {
            return Err("reviewed source snapshot coverage is ambiguous".into());
        }
        invocation = skip_ascii_whitespace(bytes, literal_end + 1);
        if bytes.get(invocation) != Some(&b')') {
            return Err("reviewed source snapshot coverage is ambiguous".into());
        }
        paths.push(&source[literal_start..literal_end]);
        cursor = invocation + 1;
    }
    Ok(paths)
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

fn skip_ascii_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
}

fn literal_string_end(bytes: &[u8], mut cursor: usize) -> Result<usize, String> {
    while let Some(byte) = bytes.get(cursor) {
        match byte {
            b'\"' => return Ok(cursor),
            b'\\' => return Err("reviewed source snapshot coverage is ambiguous".into()),
            _ => cursor += 1,
        }
    }
    Err("reviewed source snapshot coverage is ambiguous".into())
}

fn skip_quoted_string(bytes: &[u8], mut cursor: usize) -> Result<usize, String> {
    cursor += 1;
    while let Some(byte) = bytes.get(cursor) {
        match byte {
            b'\"' => return Ok(cursor + 1),
            b'\\' => {
                cursor = cursor
                    .checked_add(2)
                    .ok_or_else(|| "reviewed source snapshot coverage is ambiguous".to_string())?
            }
            _ => cursor += 1,
        }
    }
    Err("reviewed source snapshot coverage is ambiguous".into())
}

fn skip_character_literal(bytes: &[u8], cursor: usize) -> usize {
    let mut end = cursor + 1;
    if bytes.get(end) == Some(&b'\\') {
        end += 2;
    } else {
        end += 1;
    }
    if bytes.get(end) == Some(&b'\'') {
        end + 1
    } else {
        // This is a lifetime or malformed character literal. Treat only the
        // apostrophe as consumed so a following identifier is still scanned.
        cursor + 1
    }
}

fn raw_string_end(bytes: &[u8], cursor: usize) -> Result<Option<usize>, String> {
    let raw_start = match (bytes.get(cursor), bytes.get(cursor + 1)) {
        (Some(b'r'), _) => cursor,
        (Some(b'b'), Some(b'r')) => cursor + 1,
        _ => return Ok(None),
    };
    if cursor > 0 && is_identifier_continue(bytes[cursor - 1]) {
        return Ok(None);
    }
    let mut delimiter_end = raw_start + 1;
    while bytes.get(delimiter_end) == Some(&b'#') {
        delimiter_end += 1;
    }
    if bytes.get(delimiter_end) != Some(&b'\"') {
        return Ok(None);
    }
    let hashes = delimiter_end - raw_start - 1;
    let mut content = delimiter_end + 1;
    while let Some(quote_offset) = bytes[content..].iter().position(|byte| *byte == b'\"') {
        let quote = content + quote_offset;
        if bytes[quote + 1..].len() >= hashes
            && bytes[quote + 1..quote + hashes + 1]
                .iter()
                .all(|byte| *byte == b'#')
        {
            return Ok(Some(quote + hashes + 1));
        }
        content = quote + 1;
    }
    Err("reviewed source snapshot coverage is ambiguous".into())
}

fn skip_block_comment(bytes: &[u8], mut cursor: usize) -> Result<usize, String> {
    let mut depth = 1_u32;
    while cursor + 1 < bytes.len() {
        match (bytes[cursor], bytes[cursor + 1]) {
            (b'/', b'*') => {
                depth += 1;
                cursor += 2;
            }
            (b'*', b'/') => {
                depth -= 1;
                cursor += 2;
                if depth == 0 {
                    return Ok(cursor);
                }
            }
            _ => cursor += 1,
        }
    }
    Err("reviewed source snapshot coverage is ambiguous".into())
}

fn collect_tree(
    workspace: &Path,
    relative: &str,
    inputs: &mut BTreeSet<String>,
) -> Result<(), String> {
    let directory = safe_child_directory(workspace, relative)?;
    let mut children = fs::read_dir(&directory)
        .map_err(|_| "reviewed source snapshot source tree is unavailable")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "reviewed source snapshot source tree is unavailable")?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let name = child.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| "reviewed source snapshot path identity is invalid".to_string())?;
        let child_relative = format!("{relative}/{name}");
        let metadata = fs::symlink_metadata(workspace.join(&child_relative))
            .map_err(|_| "reviewed source snapshot source tree is unavailable")?;
        if unsafe_metadata(&metadata) {
            return Err("reviewed source snapshot source tree is unsafe".into());
        }
        if metadata.file_type().is_dir() {
            collect_tree(workspace, &child_relative, inputs)?;
        } else if metadata.file_type().is_file() {
            validate_normalized_relative(&child_relative)?;
            inputs.insert(child_relative);
        } else {
            return Err("reviewed source snapshot source tree is unsafe".into());
        }
    }
    Ok(())
}

fn enumerate_regular_files(
    directory: &ProtectedDirectoryGuard,
    prefix: &str,
) -> Result<BTreeSet<String>, String> {
    let mut result = BTreeSet::new();
    enumerate_regular_files_inner(directory, prefix, &mut result)?;
    Ok(result)
}

fn enumerate_regular_files_inner(
    directory: &ProtectedDirectoryGuard,
    prefix: &str,
    output: &mut BTreeSet<String>,
) -> Result<(), String> {
    directory.assert_stable("reviewed source snapshot content")?;
    for (name, attributes) in query_relative_directory_names(directory)? {
        let next = format!("{prefix}/{name}");
        if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("reviewed source snapshot content is unsafe".into());
        }
        if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            let mut child = directory.try_clone("reviewed source snapshot content")?;
            child.descend_existing(&name, "reviewed source snapshot content")?;
            enumerate_regular_files_inner(&child, &next, output)?;
        } else {
            // Opening the exact child under the pinned parent makes a
            // replacement between enumeration and classification fail closed.
            let _ =
                open_relative_regular_file(directory, &name, "reviewed source snapshot content")?;
            if !output.insert(next) {
                return Err("reviewed source snapshot content is ambiguous".into());
            }
        }
    }
    Ok(())
}

/// Enumerates entries from the pinned directory handle.  No directory
/// pathname is re-opened after its guard has been acquired.
fn query_relative_directory_names(
    directory: &ProtectedDirectoryGuard,
) -> Result<Vec<(String, u32)>, String> {
    directory.assert_stable("reviewed source snapshot content")?;
    #[cfg(windows)]
    {
        let handle = directory.last_handle("reviewed source snapshot content")?;
        let mut result = Vec::new();
        let mut restart = 1;
        loop {
            let mut buffer = vec![0_u8; 64 * 1024];
            let mut status = IO_STATUS_BLOCK {
                status: 0,
                information: 0,
            };
            let code = unsafe {
                NtQueryDirectoryFile(
                    handle,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut status,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    FILE_DIRECTORY_INFORMATION_CLASS,
                    0,
                    std::ptr::null_mut(),
                    restart,
                )
            };
            restart = 0;
            if code == STATUS_NO_MORE_FILES {
                break;
            }
            if code < 0 || status.information < 64 || status.information > buffer.len() {
                return Err("reviewed source snapshot content is unavailable".into());
            }
            let mut offset = 0_usize;
            while offset < status.information {
                if status.information - offset < 64 {
                    return Err("reviewed source snapshot content is malformed".into());
                }
                let next =
                    u32::from_ne_bytes(buffer[offset..offset + 4].try_into().unwrap()) as usize;
                let attributes =
                    u32::from_ne_bytes(buffer[offset + 56..offset + 60].try_into().unwrap());
                let name_length =
                    u32::from_ne_bytes(buffer[offset + 60..offset + 64].try_into().unwrap())
                        as usize;
                if name_length == 0
                    || !name_length.is_multiple_of(2)
                    || offset + 64 + name_length > status.information
                {
                    return Err("reviewed source snapshot content is malformed".into());
                }
                let units = buffer[offset + 64..offset + 64 + name_length]
                    .chunks_exact(2)
                    .map(|value| u16::from_ne_bytes([value[0], value[1]]))
                    .collect::<Vec<_>>();
                let name = String::from_utf16(&units)
                    .map_err(|_| "reviewed source snapshot path identity is invalid")?;
                if name != "." && name != ".." {
                    validate_protected_component(&name, "reviewed source snapshot content")?;
                    result.push((name, attributes));
                }
                if next == 0 {
                    break;
                }
                if next < 64 || offset.checked_add(next).is_none() {
                    return Err("reviewed source snapshot content is malformed".into());
                }
                offset += next;
            }
        }
        result.sort_by(|left, right| left.0.cmp(&right.0));
        if result.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err("reviewed source snapshot content is ambiguous".into());
        }
        directory.assert_stable("reviewed source snapshot content")?;
        Ok(result)
    }
    #[cfg(not(windows))]
    {
        let _ = directory;
        Err("reviewed source snapshot content stable identity is unavailable".into())
    }
}

#[cfg(windows)]
const STATUS_NO_MORE_FILES: i32 = 0x8000_0006_u32 as i32;

fn safe_workspace_file(workspace: &Path, relative: &str) -> Result<PathBuf, String> {
    safe_child_file(workspace, relative)
}

fn safe_child_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = safe_child(root, relative)?;
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| "reviewed source snapshot path is unavailable")?;
    if unsafe_metadata(&metadata) || !metadata.file_type().is_file() {
        return Err("reviewed source snapshot path is unsafe".into());
    }
    Ok(path)
}

fn safe_child_directory(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = safe_child(root, relative)?;
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| "reviewed source snapshot path is unavailable")?;
    if unsafe_metadata(&metadata) || !metadata.file_type().is_dir() {
        return Err("reviewed source snapshot path is unsafe".into());
    }
    Ok(path)
}

fn safe_child(root: &Path, relative: &str) -> Result<PathBuf, String> {
    validate_normalized_relative(relative)?;
    let root_metadata =
        fs::symlink_metadata(root).map_err(|_| "reviewed source snapshot root is unavailable")?;
    if unsafe_metadata(&root_metadata) || !root_metadata.file_type().is_dir() {
        return Err("reviewed source snapshot root is unsafe".into());
    }
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| "reviewed source snapshot path is unavailable")?;
        if unsafe_metadata(&metadata) {
            return Err("reviewed source snapshot path is unsafe".into());
        }
    }
    Ok(path)
}

/// Descends from a positively classified workspace directory. Missing
/// components are created one at a time only after their parent has been
/// classified; `create_dir_all` is intentionally never used for authority.
fn protected_snapshot_root(
    workspace: &Path,
    create_missing: bool,
) -> Result<ProtectedDirectoryGuard, String> {
    let mut guard = ProtectedDirectoryGuard::acquire(workspace, "reviewed source workspace")?;
    for component in [".catdesk", "reviewed-source-snapshots"] {
        let child = guard.path().join(component);
        match fs::symlink_metadata(&child) {
            Ok(_) => guard.descend_existing(component, "reviewed source snapshot root")?,
            Err(error) if error.kind() == ErrorKind::NotFound && create_missing => {
                guard.create_child(component, "reviewed source snapshot root")?
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err("reviewed source snapshot root is unavailable".into());
            }
            Err(_) => return Err("reviewed source snapshot root is unavailable".into()),
        }
    }
    guard.assert_stable("reviewed source snapshot root")?;
    Ok(guard)
}

/// Pins every component used as a parent for a snapshot child. Each component
/// is atomically opened or created beneath its already pinned parent, then
/// positively validated as the exact non-reparse directory before it becomes
/// authority for the next component. This accepts only the benign
/// same-parent create collision; it deliberately has no path-only fallback.
fn pin_create_relative_directories(
    root: &ProtectedDirectoryGuard,
    relative: &str,
) -> Result<ProtectedDirectoryGuard, String> {
    validate_normalized_relative(relative)?;
    root.assert_stable("reviewed source snapshot content")?;
    let mut guard = root.try_clone("reviewed source snapshot content")?;
    for component in relative.split('/') {
        validate_protected_component(component, "reviewed source snapshot content")?;
        guard.descend_or_create(component, "reviewed source snapshot content")?;
    }
    guard.assert_stable("reviewed source snapshot content")?;
    Ok(guard)
}

// The shared protected-filesystem façade is the only public route for these
// bounded child operations. The underlying implementation remains the
// reviewed RootDirectory/NT-handle machinery above; no pathname fallback is
// introduced by the extraction.
pub(crate) fn write_new_regular_in(
    parent: &ProtectedDirectoryGuard,
    name: &str,
    bytes: &[u8],
    label: &str,
) -> Result<(), String> {
    crate::windows_protected_fs::write_new_regular_in(parent, name, bytes, label)
}

pub(crate) fn read_relative_regular(
    parent: &ProtectedDirectoryGuard,
    name: &str,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    crate::windows_protected_fs::read_relative_regular(parent, name, limit, label)
}

pub(crate) fn create_relative_regular_file(
    parent: &ProtectedDirectoryGuard,
    name: &str,
    label: &str,
) -> Result<fs::File, String> {
    crate::windows_protected_fs::create_relative_regular_file(parent, name, label)
}

pub(crate) fn open_relative_regular_file(
    parent: &ProtectedDirectoryGuard,
    name: &str,
    label: &str,
) -> Result<fs::File, String> {
    crate::windows_protected_fs::open_relative_regular_file(parent, name, label)
}

fn open_relative_regular_file_for_delete(
    parent: &ProtectedDirectoryGuard,
    name: &str,
    label: &str,
) -> Result<fs::File, String> {
    #[cfg(windows)]
    {
        validate_protected_component(name, label)?;
        parent.assert_stable(label)?;
        let parent_handle = parent.last_handle(label)?;
        let handle = nt_open_relative_with_error_domain(
            parent_handle,
            name,
            DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            "reviewed source snapshot",
        )?;
        let information = handle_information(handle, label)?;
        if information.file_attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)
            != 0
        {
            unsafe { CloseHandle(handle) };
            return Err(format!("{label} is unsafe"));
        }
        Ok(unsafe { fs::File::from_raw_handle(handle) })
    }
    #[cfg(not(windows))]
    {
        let _ = (parent, name);
        Err(format!("{label} stable identity is unavailable"))
    }
}

fn dispose_opened_object(handle: *mut c_void, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut status = IO_STATUS_BLOCK {
            status: 0,
            information: 0,
        };
        let mut flags = FILE_DISPOSITION_DELETE;
        let extended = unsafe {
            NtSetInformationFile(
                handle,
                &mut status,
                (&mut flags as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
                FILE_DISPOSITION_INFORMATION_EX_CLASS,
            )
        };
        if extended >= 0 {
            return Ok(());
        }
        // Older supported Windows releases may not recognize the Ex class.
        // The exact same already-opened DELETE handle is used for the native
        // one-byte FileDispositionInformation fallback.
        if extended != STATUS_INVALID_INFO_CLASS && extended != STATUS_INVALID_PARAMETER {
            return Err(format!("{label} deletion is unavailable"));
        }
        let mut delete = 1_u8;
        let fallback = unsafe {
            NtSetInformationFile(
                handle,
                &mut status,
                (&mut delete as *mut u8).cast(),
                1,
                FILE_DISPOSITION_INFORMATION_CLASS,
            )
        };
        if fallback < 0 {
            return Err(format!("{label} deletion is unavailable"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        Err(format!("{label} stable identity is unavailable"))
    }
}

fn cleanup_staging_tree(directory: &ProtectedDirectoryGuard) -> Result<(), String> {
    directory.assert_stable("reviewed source snapshot staging")?;
    for (name, attributes) in query_relative_directory_names(directory)? {
        if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("reviewed source snapshot staging is unsafe".into());
        }
        if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            let mut child = directory.try_clone("reviewed source snapshot staging")?;
            child.descend_for_delete(&name, "reviewed source snapshot staging")?;
            cleanup_staging_tree(&child)?;
            child.assert_stable("reviewed source snapshot staging")?;
            #[cfg(windows)]
            #[cfg(test)]
            invoke_cleanup_disposition_hook(child.path(), true);
            #[cfg(windows)]
            dispose_opened_object(
                child.last_handle("reviewed source snapshot staging")?,
                "reviewed source snapshot staging",
            )?;
        } else {
            let file = open_relative_regular_file_for_delete(
                directory,
                &name,
                "reviewed source snapshot staging",
            )?;
            #[cfg(windows)]
            #[cfg(test)]
            invoke_cleanup_disposition_hook(&directory.path().join(&name), false);
            #[cfg(windows)]
            dispose_opened_object(
                file.as_raw_handle().cast(),
                "reviewed source snapshot staging",
            )?;
            #[cfg(not(windows))]
            {
                let _ = file;
                return Err(
                    "reviewed source snapshot staging stable identity is unavailable".into(),
                );
            }
        }
    }
    directory.assert_stable("reviewed source snapshot staging")?;
    Ok(())
}

#[cfg(windows)]
const STATUS_INVALID_INFO_CLASS: i32 = 0xc000_0003_u32 as i32;
#[cfg(windows)]
const STATUS_INVALID_PARAMETER: i32 = 0xc000_000d_u32 as i32;

fn create_owned_staging(
    root: &ProtectedDirectoryGuard,
    session_id: &str,
) -> Result<ProtectedDirectoryGuard, String> {
    root.assert_stable("reviewed source snapshot root")?;
    let name = format!(".{session_id}-{}.staging", Uuid::new_v4());
    if name.len() > 192 {
        return Err("reviewed source snapshot staging identity is unsafe".into());
    }
    let mut guard = root.try_clone("reviewed source snapshot root")?;
    // The staging object remains identity-pinned, while only its own handle
    // permits the final same-parent rename.  Its protected parent does not.
    guard.create_renameable_child(&name, "reviewed source snapshot staging")?;
    guard.assert_stable("reviewed source snapshot staging")?;
    Ok(guard)
}

fn owned_staging_name_for_session(name: &str, session_id: &str) -> Result<bool, String> {
    let prefix = format!(".{session_id}-");
    let Some(uuid) = name.strip_prefix(&prefix) else {
        return Ok(false);
    };
    let Some(uuid) = uuid.strip_suffix(".staging") else {
        return Err("reviewed source snapshot staging is unsafe".into());
    };
    let parsed = Uuid::parse_str(uuid)
        .map_err(|_| "reviewed source snapshot staging is unsafe".to_string())?;
    if parsed.hyphenated().to_string() != uuid {
        return Err("reviewed source snapshot staging is unsafe".into());
    }
    Ok(true)
}

/// Reclaims only this session's CatDesk-owned crash residue.  Every name is
/// enumerated and opened under the pinned root; another session's staging and
/// committed session directories are intentionally not cleanup candidates.
fn reclaim_stale_current_session_staging(
    root: &ProtectedDirectoryGuard,
    session_id: &str,
) -> Result<(), String> {
    root.assert_stable("reviewed source snapshot root")?;
    for (name, attributes) in query_relative_directory_names(root)? {
        let is_current_staging = owned_staging_name_for_session(&name, session_id)?;
        if !is_current_staging {
            continue;
        }
        if attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)
            != FILE_ATTRIBUTE_DIRECTORY
        {
            return Err("reviewed source snapshot staging is unsafe".into());
        }
        let mut staging = root.try_clone("reviewed source snapshot root")?;
        #[cfg(test)]
        invoke_staging_reclaim_open_hook(&root.path().join(&name));
        staging.descend_for_delete(&name, "reviewed source snapshot staging")?;
        if !staging.is_direct_child_of(root) {
            return Err("reviewed source snapshot staging is unsafe".into());
        }
        remove_owned_staging(root, &staging, session_id)?;
    }
    root.assert_stable("reviewed source snapshot root")?;
    Ok(())
}

fn remove_owned_staging(
    root: &ProtectedDirectoryGuard,
    staging: &ProtectedDirectoryGuard,
    session_id: &str,
) -> Result<(), String> {
    root.assert_stable("reviewed source snapshot root")?;
    staging.assert_stable("reviewed source snapshot staging")?;
    let name = staging
        .path()
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "reviewed source snapshot staging is unsafe".to_string())?;
    if !owned_staging_name_for_session(name, session_id)? {
        return Err("reviewed source snapshot staging is unsafe".into());
    }
    if !staging.is_direct_child_of(root) {
        return Err("reviewed source snapshot staging is unsafe".into());
    }
    cleanup_staging_tree(staging)?;
    #[cfg(windows)]
    #[cfg(test)]
    invoke_cleanup_disposition_hook(staging.path(), true);
    #[cfg(windows)]
    dispose_opened_object(
        staging.last_handle("reviewed source snapshot staging")?,
        "reviewed source snapshot staging",
    )?;
    #[cfg(not(windows))]
    return Err("reviewed source snapshot staging stable identity is unavailable".into());
    Ok(())
}

fn normalize_relative_path(path: &Path) -> Result<String, String> {
    if path.is_absolute() {
        return Err("reviewed source snapshot path is unsafe".into());
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    "reviewed source snapshot path identity is invalid".to_string()
                })?;
                if value.is_empty() || value == "." || value == ".." {
                    return Err("reviewed source snapshot path is unsafe".into());
                }
                parts.push(value);
            }
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err("reviewed source snapshot path is unsafe".into());
                }
            }
            Component::CurDir => return Err("reviewed source snapshot path is unsafe".into()),
            _ => return Err("reviewed source snapshot path is unsafe".into()),
        }
    }
    let normalized = parts.join("/");
    validate_normalized_relative(&normalized)?;
    Ok(normalized)
}

fn validate_normalized_relative(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_SNAPSHOT_PATH_BYTES
        || value.contains('\\')
        || value.starts_with('/')
        || value.contains('\0')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("reviewed source snapshot path is unsafe".into());
    }
    if [".git", ".catdesk", "target"]
        .iter()
        .any(|root| value == *root || value.starts_with(&format!("{root}/")))
    {
        return Err("reviewed source snapshot path is excluded".into());
    }
    Ok(())
}

fn unsafe_metadata(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    if metadata.file_attributes() & 0x0400 != 0 {
        return true;
    }
    false
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha256(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[cfg(unix)]
    use std::os::unix::fs::symlink as symlink_dir;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_dir;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_file;

    fn workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("catdesk-snapshot-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::create_dir_all(root.join("wake/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            b"[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.lock"), b"version = 4\n").unwrap();
        fs::write(
            root.join("wake/Cargo.toml"),
            b"[package]\nname='catdesk-wake'\nversion='1.0.0'\nedition='2024'\n",
        )
        .unwrap();
        fs::write(root.join("wake/src/lib.rs"), b"pub const WAKE: u8 = 1;\n").unwrap();
        fs::write(root.join("src/main.rs"), b"mod nested;\n").unwrap();
        fs::write(root.join("src/nested/mod.rs"), b"pub const X: u8 = 1;\n").unwrap();
        root
    }

    fn expected() -> ReviewedSourceSnapshotExpectedV1 {
        ReviewedSourceSnapshotExpectedV1 {
            session_id: "session-1".into(),
            project_id: "catdesk".into(),
            approved_contract_hash: "contract-hash".into(),
            logical_task_id: "task-1".into(),
            completion_artifact_ids: vec!["src/out.txt".into()],
            current_outputs: vec![ReviewedSourceCurrentOutputV1 {
                artifact_id: "src/out.txt".into(),
                sha256: "a".repeat(64),
            }],
            baseline_observations: vec![ReviewedSourceBaselineObservationV1 {
                artifact_id: "src/out.txt".into(),
                state: "ABSENT".into(),
                sha256: None,
            }],
        }
    }

    #[test]
    fn creates_nested_binary_snapshot_and_replays_without_live_source_reads() {
        let root = workspace();
        fs::write(root.join("src/nested/data.bin"), [0_u8, 255, 7]).unwrap();
        let first = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        let original = fs::read(first.bytes_root.join("src/nested/data.bin")).unwrap();
        fs::write(root.join("src/nested/data.bin"), [9_u8, 9, 9]).unwrap();
        let replay = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        assert_eq!(first.snapshot_id, replay.snapshot_id);
        assert_eq!(
            fs::read(replay.bytes_root.join("src/nested/data.bin")).unwrap(),
            original
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_manifest_tamper_and_authority_drift() {
        let root = workspace();
        create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        let manifest = root.join(".catdesk/reviewed-source-snapshots/session-1/manifest.json");
        fs::write(&manifest, b"{}").unwrap();
        assert!(validate_committed_snapshot(&root, &expected()).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_dynamic_include_coverage() {
        let root = workspace();
        fs::write(root.join("src/main.rs"), b"include_str!(path!());\n").unwrap();
        assert!(create_or_validate_reviewed_source_snapshot(&root, &expected()).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn captures_literal_include_assets_as_exact_bytes() {
        let root = workspace();
        fs::write(
            root.join("src/main.rs"),
            b"const DATA: &[u8] = include_bytes!(\"../asset.bin\");\n",
        )
        .unwrap();
        fs::write(root.join("asset.bin"), [1_u8, 0, 2, 255]).unwrap();
        let snapshot = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        assert_eq!(
            fs::read(snapshot.bytes_root.join("asset.bin")).unwrap(),
            [1_u8, 0, 2, 255]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_macro_like_text_but_captures_real_literal_include() {
        let root = workspace();
        fs::write(
            root.join("src/main.rs"),
            br#"
                const TEXT: &str = "include_str!(\"not-an-input.txt\")";
                // include_bytes!("also-not-an-input.bin")
                /* include_str!("still-not-an-input.txt") */
                const DATA: &str = include_str!("../asset.txt");
            "#,
        )
        .unwrap();
        fs::write(root.join("asset.txt"), b"reviewed asset").unwrap();

        let snapshot = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        assert_eq!(
            fs::read(snapshot.bytes_root.join("asset.txt")).unwrap(),
            b"reviewed asset"
        );
        assert!(
            !snapshot
                .entries
                .iter()
                .any(|entry| entry.relative_path.contains("not-an-input"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_macro_like_text_in_byte_strings() {
        assert_eq!(
            literal_include_paths("let _ = b\"include_str!(path!());\\n\";"),
            Ok(Vec::new())
        );
    }

    #[test]
    fn release_inputs_require_the_local_wake_crate_manifest() {
        let root = workspace();
        fs::remove_file(root.join("wake/Cargo.toml")).unwrap();
        assert!(collect_release_inputs(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn current_workspace_source_inputs_are_collectable() {
        let inputs = collect_release_inputs(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert!(inputs.contains(&"src/reviewed_source_snapshot.rs".to_string()));
        assert!(inputs.contains(&"wake/Cargo.toml".to_string()));
        assert!(inputs.contains(&"wake/src/lib.rs".to_string()));
        assert!(inputs.contains(&"wake/src/bin/CatDeskWakeHost.rs".to_string()));
        assert!(inputs.contains(&"scripts/start-local-orchestrator.ps1".to_string()));
        assert!(
            inputs.contains(&"tests/fixtures/delegated/execution_contract_v1.json".to_string())
        );
    }

    #[test]
    fn rejects_extra_content_objects_and_legacy_sibling_state() {
        let root = workspace();
        create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        let directory = root.join(".catdesk/reviewed-source-snapshots/session-1");
        fs::write(directory.join("bytes/extra.bin"), b"not reviewed").unwrap();
        assert!(validate_committed_snapshot(&root, &expected()).is_err());
        let other = workspace();
        fs::create_dir_all(other.join(".catdesk/reviewed-source-snapshots")).unwrap();
        fs::write(
            other.join(".catdesk/reviewed-source-snapshots/session-1.json"),
            b"{}",
        )
        .unwrap();
        assert!(validate_committed_snapshot(&other, &expected()).is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(other);
    }

    #[test]
    fn rejects_changed_completion_output_authority_without_overwrite() {
        let root = workspace();
        let original = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        let mut changed = expected();
        changed.current_outputs[0].sha256 = "b".repeat(64);
        assert!(create_or_validate_reviewed_source_snapshot(&root, &changed).is_err());
        assert_eq!(
            validate_committed_snapshot(&root, &expected())
                .unwrap()
                .snapshot_id,
            original.snapshot_id
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn creates_missing_protected_root_components_one_at_a_time() {
        let root = workspace();
        let snapshot = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        let catdesk = root.join(".catdesk");
        let snapshot_root = catdesk.join("reviewed-source-snapshots");
        assert!(fs::symlink_metadata(&catdesk).unwrap().file_type().is_dir());
        assert!(
            fs::symlink_metadata(&snapshot_root)
                .unwrap()
                .file_type()
                .is_dir()
        );
        assert_eq!(
            validate_committed_snapshot(&root, &expected())
                .unwrap()
                .snapshot_id,
            snapshot.snapshot_id
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_file_as_protected_intermediate_without_creating_snapshot_state() {
        let root = workspace();
        fs::write(root.join(".catdesk"), b"not a directory").unwrap();
        assert!(create_or_validate_reviewed_source_snapshot(&root, &expected()).is_err());
        assert!(fs::symlink_metadata(root.join(".catdesk/reviewed-source-snapshots")).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_intermediate_link_redirection_without_outside_mutation_when_supported() {
        let root = workspace();
        let outside = std::env::temp_dir().join(format!("catdesk-outside-{}", Uuid::new_v4()));
        fs::create_dir(&outside).unwrap();
        if symlink_dir(&outside, root.join(".catdesk")).is_err() {
            // Some Windows test hosts do not grant link creation. The
            // production classification still rejects reparse metadata.
            let _ = fs::remove_dir_all(root);
            let _ = fs::remove_dir_all(outside);
            return;
        }
        assert!(create_or_validate_reviewed_source_snapshot(&root, &expected()).is_err());
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        let _ = fs::remove_file(root.join(".catdesk"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn stale_staging_does_not_block_exact_committed_replay_or_become_authority() {
        // The cleanup fault seams are process-global test instrumentation.
        // Serialize every test that reclaims staging so another test's
        // deliberate pre-disposition mutation cannot perturb this ordinary
        // replay assertion.
        let _serial = cleanup_hook_test_lock().lock().unwrap();
        let root = workspace();
        let committed = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        let snapshot_root = root.join(".catdesk/reviewed-source-snapshots");
        let stale_name = ".session-1-00000000-0000-4000-8000-000000000001.staging";
        fs::create_dir(snapshot_root.join(stale_name)).unwrap();
        fs::write(
            snapshot_root.join(stale_name).join("manifest.json"),
            b"not authority",
        )
        .unwrap();
        assert_eq!(
            create_or_validate_reviewed_source_snapshot(&root, &expected())
                .unwrap()
                .snapshot_id,
            committed.snapshot_id
        );
        let other = workspace();
        fs::create_dir_all(
            other.join(".catdesk/reviewed-source-snapshots/.session-1-00000000-0000-4000-8000-000000000001.staging"),
        )
        .unwrap();
        assert!(validate_committed_snapshot(&other, &expected()).is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(other);
    }

    #[test]
    fn pinned_snapshot_directories_create_and_reopen_existing_bytes_scripts() {
        let root = workspace();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();

        // The first call exercises fresh creation. The second is the
        // deterministic same-parent collision equivalent: FILE_OPEN_IF must
        // reopen and validate the existing exact directories, never trust the
        // pathname observation that preceded an earlier create.
        let created = pin_create_relative_directories(&staging, "bytes/scripts").unwrap();
        let created_path = created.path().to_path_buf();
        created
            .assert_stable("reviewed source snapshot content")
            .unwrap();
        drop(created);
        let reopened = pin_create_relative_directories(&staging, "bytes/scripts").unwrap();
        assert_eq!(reopened.path(), created_path.as_path());
        reopened
            .assert_stable("reviewed source snapshot content")
            .unwrap();

        drop(reopened);
        drop(staging);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pinned_snapshot_directories_refuse_existing_file_collision() {
        let root = workspace();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();
        fs::write(staging.path().join("bytes"), b"not-a-directory").unwrap();

        assert!(pin_create_relative_directories(&staging, "bytes/scripts").is_err());
        assert!(!staging.path().join("bytes/scripts").exists());

        drop(staging);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pinned_child_parent_rejects_link_redirection_before_content_write_when_supported() {
        let root = workspace();
        let outside =
            std::env::temp_dir().join(format!("catdesk-snapshot-outside-{}", Uuid::new_v4()));
        fs::create_dir(&outside).unwrap();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();
        let bytes = pin_create_relative_directories(&staging, "bytes").unwrap();
        if symlink_dir(&outside, bytes.path().join("redirect")).is_err() {
            let _ = fs::remove_dir_all(root);
            let _ = fs::remove_dir_all(outside);
            return;
        }
        assert!(pin_create_relative_directories(&staging, "bytes/redirect/nested").is_err());
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        drop(bytes);
        drop(staging);
        let _ = fs::remove_file(
            root.join(".catdesk/reviewed-source-snapshots")
                .join(".session-1"),
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn create_new_content_file_never_overwrites_an_existing_child() {
        let root = workspace();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();
        let bytes = pin_create_relative_directories(&staging, "bytes").unwrap();
        fs::write(bytes.path().join("manifest-piece"), b"original").unwrap();
        assert!(
            write_new_regular_in(
                &bytes,
                "manifest-piece",
                b"replacement",
                "reviewed source snapshot content"
            )
            .is_err()
        );
        assert_eq!(
            fs::read(bytes.path().join("manifest-piece")).unwrap(),
            b"original"
        );
        drop(bytes);
        drop(staging);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn committed_handle_validation_rejects_replaced_content_object() {
        let root = workspace();
        let snapshot = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        fs::write(snapshot.bytes_root.join("src/main.rs"), b"substituted").unwrap();
        assert!(validate_committed_snapshot(&root, &expected()).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn handle_bound_cleanup_removes_only_the_owned_nested_staging_tree() {
        let _serial = cleanup_hook_test_lock().lock().unwrap();
        let root = workspace();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();
        let staging_path = staging.path().to_path_buf();
        let nested = pin_create_relative_directories(&staging, "bytes/nested").unwrap();
        write_new_regular_in(
            &nested,
            "payload.bin",
            b"staging-only",
            "reviewed source snapshot staging",
        )
        .unwrap();
        drop(nested);
        remove_owned_staging(&protected, &staging, "session-1").unwrap();
        drop(staging);
        assert!(fs::symlink_metadata(&staging_path).is_err());
        assert!(fs::symlink_metadata(root.join("Cargo.toml")).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_reclaims_only_this_sessions_exact_uuid_staging() {
        let _serial = cleanup_hook_test_lock().lock().unwrap();
        let root = workspace();
        let snapshot_root = root.join(".catdesk/reviewed-source-snapshots");
        fs::create_dir_all(&snapshot_root).unwrap();
        let current = ".session-1-00000000-0000-4000-8000-000000000001.staging";
        let other = ".session-2-00000000-0000-4000-8000-000000000002.staging";
        fs::create_dir_all(snapshot_root.join(current).join("bytes")).unwrap();
        fs::write(
            snapshot_root.join(current).join("bytes/crash.bin"),
            b"crash",
        )
        .unwrap();
        fs::create_dir(snapshot_root.join(other)).unwrap();
        let snapshot = create_or_validate_reviewed_source_snapshot(&root, &expected()).unwrap();
        assert!(fs::symlink_metadata(snapshot_root.join(current)).is_err());
        assert!(fs::symlink_metadata(snapshot_root.join(other)).is_ok());
        assert_eq!(
            create_or_validate_reviewed_source_snapshot(&root, &expected())
                .unwrap()
                .snapshot_id,
            snapshot.snapshot_id
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pre_disposition_hook_cannot_redirect_opened_file_deletion() {
        let _serial = cleanup_hook_test_lock().lock().unwrap();
        let root = workspace();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();
        let staging_path = staging.path().to_path_buf();
        write_new_regular_in(
            &staging,
            "payload.bin",
            b"before-hook",
            "reviewed source snapshot staging",
        )
        .unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        replace_cleanup_disposition_hook(Some(Box::new(move |path, is_directory| {
            observed.lock().unwrap().push(is_directory);
            if !is_directory {
                let _ = fs::write(path, b"replacement-at-boundary");
            }
        })));
        let result = remove_owned_staging(&protected, &staging, "session-1");
        replace_cleanup_disposition_hook(None);
        result.unwrap();
        drop(staging);
        assert!(fs::symlink_metadata(&staging_path).is_err());
        assert!(calls.lock().unwrap().iter().any(|directory| !directory));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn final_child_disposition_replacement_preserves_replacement_or_is_denied() {
        let _serial = cleanup_hook_test_lock().lock().unwrap();
        let root = workspace();
        let outside =
            std::env::temp_dir().join(format!("catdesk-cleanup-outside-{}", Uuid::new_v4()));
        fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("sentinel.txt");
        fs::write(&sentinel, b"outside").unwrap();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();
        let staging_path = staging.path().to_path_buf();
        let payload = staging_path.join("payload.bin");
        write_new_regular_in(
            &staging,
            "payload.bin",
            b"original",
            "reviewed source snapshot staging",
        )
        .unwrap();
        let outcome = Arc::new(Mutex::new(None));
        let observed = Arc::clone(&outcome);
        replace_cleanup_disposition_hook(Some(Box::new(move |path, is_directory| {
            if is_directory
                || path.file_name().and_then(|name| name.to_str()) != Some("payload.bin")
            {
                return;
            }
            let moved = path.with_extension("opened-original");
            let replacement =
                fs::rename(path, &moved).and_then(|_| fs::write(path, b"replacement"));
            *observed.lock().unwrap() = Some(replacement.is_ok());
        })));
        let result = remove_owned_staging(&protected, &staging, "session-1");
        replace_cleanup_disposition_hook(None);
        let replaced = outcome.lock().unwrap().unwrap_or(false);
        if replaced {
            assert!(result.is_err());
            assert_eq!(fs::read(&payload).unwrap(), b"replacement");
        } else {
            result.unwrap();
        }
        assert_eq!(fs::read(&sentinel).unwrap(), b"outside");
        drop(staging);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn final_nested_directory_replacement_preserves_substituted_tree_or_is_denied() {
        let _serial = cleanup_hook_test_lock().lock().unwrap();
        let root = workspace();
        let outside =
            std::env::temp_dir().join(format!("catdesk-cleanup-dir-outside-{}", Uuid::new_v4()));
        fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("sentinel.txt");
        fs::write(&sentinel, b"outside").unwrap();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();
        let staging_path = staging.path().to_path_buf();
        let nested = pin_create_relative_directories(&staging, "bytes/nested").unwrap();
        write_new_regular_in(
            &nested,
            "payload.bin",
            b"original",
            "reviewed source snapshot staging",
        )
        .unwrap();
        drop(nested);
        let outcome = Arc::new(Mutex::new(None));
        let observed = Arc::clone(&outcome);
        replace_cleanup_disposition_hook(Some(Box::new(move |path, is_directory| {
            if !is_directory || path.file_name().and_then(|name| name.to_str()) != Some("nested") {
                return;
            }
            let moved = path.with_extension("opened-original");
            let replacement = fs::rename(path, &moved).and_then(|_| {
                fs::create_dir(path)?;
                fs::write(path.join("replacement.txt"), b"replacement")
            });
            *observed.lock().unwrap() = Some(replacement.is_ok());
        })));
        let result = remove_owned_staging(&protected, &staging, "session-1");
        replace_cleanup_disposition_hook(None);
        let nested_path = staging_path.join("bytes/nested/replacement.txt");
        if outcome.lock().unwrap().unwrap_or(false) {
            assert!(result.is_err());
            assert_eq!(fs::read(&nested_path).unwrap(), b"replacement");
        } else {
            result.unwrap();
        }
        assert_eq!(fs::read(&sentinel).unwrap(), b"outside");
        drop(staging);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn stale_staging_enumeration_to_open_replacement_fails_closed_or_is_denied() {
        let _serial = cleanup_hook_test_lock().lock().unwrap();
        let root = workspace();
        let snapshot_root = root.join(".catdesk/reviewed-source-snapshots");
        fs::create_dir_all(&snapshot_root).unwrap();
        let name = ".session-1-00000000-0000-4000-8000-000000000003.staging";
        let candidate = snapshot_root.join(name);
        fs::create_dir(&candidate).unwrap();
        let outside =
            std::env::temp_dir().join(format!("catdesk-reclaim-outside-{}", Uuid::new_v4()));
        fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("sentinel.txt");
        fs::write(&sentinel, b"outside").unwrap();
        let outcome = Arc::new(Mutex::new(None));
        let observed = Arc::clone(&outcome);
        replace_staging_reclaim_open_hook(Some(Box::new(move |path, _| {
            let moved = path.with_extension("enumerated-original");
            let replacement =
                fs::rename(path, &moved).and_then(|_| fs::write(path, b"not a directory"));
            *observed.lock().unwrap() = Some(replacement.is_ok());
        })));
        let result = create_or_validate_reviewed_source_snapshot(&root, &expected());
        replace_staging_reclaim_open_hook(None);
        if outcome.lock().unwrap().unwrap_or(false) {
            assert!(result.is_err());
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(fs::read(&sentinel).unwrap(), b"outside");
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn reparse_replacement_at_final_child_boundary_never_touches_outside_when_supported() {
        let _serial = cleanup_hook_test_lock().lock().unwrap();
        let root = workspace();
        let outside =
            std::env::temp_dir().join(format!("catdesk-reparse-outside-{}", Uuid::new_v4()));
        fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("sentinel.txt");
        fs::write(&sentinel, b"outside").unwrap();
        let protected = protected_snapshot_root(&root, true).unwrap();
        let staging = create_owned_staging(&protected, "session-1").unwrap();
        write_new_regular_in(
            &staging,
            "payload.bin",
            b"original",
            "reviewed source snapshot staging",
        )
        .unwrap();
        let installed = Arc::new(Mutex::new(false));
        let observed = Arc::clone(&installed);
        let outside_target = outside.join("sentinel.txt");
        replace_cleanup_disposition_hook(Some(Box::new(move |path, is_directory| {
            if is_directory
                || path.file_name().and_then(|name| name.to_str()) != Some("payload.bin")
            {
                return;
            }
            let moved = path.with_extension("opened-original");
            if fs::rename(path, &moved).is_ok() && symlink_file(&outside_target, path).is_ok() {
                *observed.lock().unwrap() = true;
            }
        })));
        let _ = remove_owned_staging(&protected, &staging, "session-1");
        replace_cleanup_disposition_hook(None);
        assert_eq!(fs::read(&sentinel).unwrap(), b"outside");
        drop(staging);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
