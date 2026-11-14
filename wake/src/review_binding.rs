//! Independent, read-only validation at Wake's completed-review boundary.
//!
//! Wake deliberately has no dependency on the CatDesk crate. These are the
//! bounded wire fields required to recheck a managed Sol review before it can
//! become a `review_ready` event. The committed source bytes, rather than the
//! mutable workspace, are the source authority.

use crate::Result;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{ErrorKind, Read},
    path::{Component, Path},
};

const SOL: &str = "gpt-6.1-sol";
const TERRA: &str = "gpt-5.6-terra";
const AUTHORITY_DOMAIN: &str = "CATDESK_REVIEWED_SOURCE_AUTHORITY_V1";
const MANIFEST_DOMAIN: &str = "CATDESK_REVIEWED_SOURCE_MANIFEST_V1";
const SNAPSHOT_ID_DOMAIN: &str = "CATDESK_REVIEWED_SOURCE_SNAPSHOT_ID_V1";
const SNAPSHOT_SCHEMA: u32 = 4;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Session {
    schema_version: u32,
    session_id: String,
    state: String,
    current_task_id: Option<String>,
    provider_thread_id: Option<String>,
    #[serde(default)]
    provider_turn_count: u32,
    #[serde(default)]
    codex_turn_profile_violation: bool,
    #[serde(default)]
    codex_turn_profile_tracking_started: bool,
    approved_contract_hash: Option<String>,
    last_event_sequence: u64,
    active: bool,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Verification {
    status: String,
    command: String,
    summary: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeTurn {
    codex_thread_id: String,
    codex_turn_id: String,
    model_id: String,
    reasoning_effort: String,
    evidence_sha256: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ExpectedSource {
    session_id: String,
    project_id: String,
    approved_contract_hash: String,
    logical_task_id: String,
    completion_artifact_ids: Vec<String>,
    current_outputs: Vec<CurrentOutput>,
    baseline_observations: Vec<BaselineObservation>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CurrentOutput {
    artifact_id: String,
    sha256: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BaselineObservation {
    artifact_id: String,
    state: String,
    sha256: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Checkpoint {
    schema_version: u32,
    session_id: String,
    project_id: String,
    approved_contract_hash: String,
    logical_task_id: String,
    provider_id: String,
    provider_turn_count: u32,
    verification_profile: String,
    verification: Verification,
    authoritative_diff: String,
    authoritative_diff_sha256: String,
    attribution_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    codex_turn_profile: Option<NativeTurn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reviewed_source_expectation: Option<ExpectedSource>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Completion {
    schema_version: u32,
    session_id: String,
    verification: Verification,
    authoritative_diff: String,
    final_review: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    schema_version: u32,
    record_id: String,
    created_at_unix: u64,
    session_id: String,
    project_id: String,
    reference: String,
    approved_contract_hash: String,
    logical_task_id: String,
    provider_turn_count: u32,
    codex_turn_profile: NativeTurn,
    checkpoint_sha256: String,
    completion_sha256: String,
    authoritative_diff_sha256: String,
    attribution_digest: String,
    reviewed_source_expectation_sha256: String,
    reviewed_source_snapshot_id: String,
    reviewed_source_manifest_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotEntry {
    relative_path: String,
    byte_length: u64,
    sha256: String,
    content_object: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    authority_domain: String,
    manifest_domain: String,
    snapshot_id_domain: String,
    session_id: String,
    project_id: String,
    approved_contract_hash: String,
    logical_task_id: String,
    completion_artifact_ids: Vec<String>,
    current_outputs: Vec<CurrentOutput>,
    baseline_observations: Vec<BaselineObservation>,
    authority_digest: String,
    entries: Vec<SnapshotEntry>,
    manifest_digest: String,
    snapshot_id: String,
}

fn sha(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn json_sha<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_vec(value)
        .map(sha)
        .map_err(|_| "WAKE_SOL_REVIEW_JSON_INVALID".into())
}

fn hex_sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.contains(['\\', ':', '\0'])
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn review_sequence(session_id: &str, record_id: &str) -> Option<u64> {
    record_id
        .strip_prefix(&format!("review-{session_id}-"))?
        .strip_suffix("-independent_final_review")?
        .parse::<u64>()
        .ok()
}

fn safe_file(workspace: &Path, path_relative: &str, max: u64) -> Result<Vec<u8>> {
    if !relative(path_relative) {
        return Err("WAKE_SOL_REVIEW_PATH_INVALID".into());
    }
    let mut path = workspace.to_path_buf();
    for component in path_relative.split('/') {
        path.push(component);
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| "WAKE_SOL_REVIEW_EVIDENCE_MISSING")?;
        if metadata.file_type().is_symlink() {
            return Err("WAKE_SOL_REVIEW_EVIDENCE_LINK".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x0400 != 0 {
                return Err("WAKE_SOL_REVIEW_EVIDENCE_LINK".into());
            }
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|_| "WAKE_SOL_REVIEW_EVIDENCE_MISSING")?;
    if !metadata.file_type().is_file() || metadata.len() > max {
        return Err("WAKE_SOL_REVIEW_EVIDENCE_INVALID".into());
    }
    let file = fs::File::open(path).map_err(|_| "WAKE_SOL_REVIEW_EVIDENCE_MISSING")?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_SOL_REVIEW_EVIDENCE_MISSING")?;
    if bytes.len() as u64 > max || bytes.len() as u64 != metadata.len() {
        return Err("WAKE_SOL_REVIEW_EVIDENCE_CHANGED".into());
    }
    Ok(bytes)
}

fn read_json<T: DeserializeOwned>(workspace: &Path, relative: &str, max: u64) -> Result<T> {
    serde_json::from_slice(&safe_file(workspace, relative, max)?)
        .map_err(|_| "WAKE_SOL_REVIEW_EVIDENCE_INVALID".into())
}

// CatDesk's contract decision hash is FNV-1a over compact serde JSON. Files
// are pretty-printed, so remove insignificant whitespace without changing
// object key order or string bytes. Reordered fields fail closed.
fn compact_json(bytes: &[u8]) -> Result<Vec<u8>> {
    let _: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "WAKE_SOL_REVIEW_CONTRACT_INVALID")?;
    let mut compact = Vec::with_capacity(bytes.len());
    let mut quoted = false;
    let mut escaped = false;
    for &byte in bytes {
        if quoted {
            compact.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else if byte == b'"' {
            quoted = true;
            compact.push(byte);
        } else if !byte.is_ascii_whitespace() {
            compact.push(byte);
        }
    }
    Ok(compact)
}

fn contract_hash(bytes: &[u8]) -> Result<String> {
    let compact = compact_json(bytes)?;
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in compact {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("fnv1a64:{hash:016x}"))
}

fn approved_outputs(contract: &serde_json::Value, task_id: &str) -> Option<Vec<String>> {
    let graph = contract.get("taskGraph").and_then(|value| value.as_array());
    let task = if graph.is_some_and(|graph| !graph.is_empty()) {
        graph?
            .iter()
            .find(|task| task.get("taskId").and_then(|value| value.as_str()) == Some(task_id))?
    } else {
        if contract.get("taskId")?.as_str() != Some(task_id) {
            return None;
        }
        contract
    };
    let mut outputs = task
        .get("completionArtifactIds")?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    outputs.sort();
    if outputs.is_empty() || outputs.windows(2).any(|pair| pair[0] == pair[1]) {
        return None;
    }
    Some(outputs)
}

fn snapshot(
    workspace: &Path,
    project_id: &str,
    expected: &ExpectedSource,
) -> Result<(String, String)> {
    if expected.session_id.is_empty()
        || expected.project_id != project_id
        || expected.completion_artifact_ids.is_empty()
        || expected.completion_artifact_ids.len() > 64
        || expected.current_outputs.len() != expected.completion_artifact_ids.len()
        || expected.baseline_observations.len() != expected.completion_artifact_ids.len()
    {
        return Err("WAKE_SOL_REVIEW_SNAPSHOT_INVALID".into());
    }
    let mut canonical = expected.clone();
    canonical.completion_artifact_ids.sort();
    canonical
        .current_outputs
        .sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    canonical
        .baseline_observations
        .sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    if canonical
        .completion_artifact_ids
        .windows(2)
        .any(|pair| pair[0] == pair[1])
        || canonical
            .current_outputs
            .iter()
            .zip(&canonical.completion_artifact_ids)
            .any(|(output, id)| output.artifact_id != *id || !hex_sha(&output.sha256))
        || canonical
            .baseline_observations
            .iter()
            .zip(&canonical.completion_artifact_ids)
            .any(|(baseline, id)| {
                baseline.artifact_id != *id
                    || !matches!(baseline.state.as_str(), "ABSENT" | "PRESENT_SHA256")
                    || (baseline.state == "ABSENT" && baseline.sha256.is_some())
                    || (baseline.state == "PRESENT_SHA256"
                        && !baseline.sha256.as_deref().is_some_and(hex_sha))
            })
        || canonical
            .completion_artifact_ids
            .iter()
            .any(|id| id.len() > 512 || !relative(id))
    {
        return Err("WAKE_SOL_REVIEW_SNAPSHOT_INVALID".into());
    }
    let root = format!(
        ".catdesk/reviewed-source-snapshots/{}",
        canonical.session_id
    );
    let manifest: Manifest = read_json(
        workspace,
        &format!("{root}/manifest.json"),
        16 * 1024 * 1024,
    )?;
    let authority = json_sha(&(
        AUTHORITY_DOMAIN,
        SNAPSHOT_SCHEMA,
        &canonical.session_id,
        &canonical.project_id,
        &canonical.approved_contract_hash,
        &canonical.logical_task_id,
        &canonical.completion_artifact_ids,
        &canonical.current_outputs,
        &canonical.baseline_observations,
    ))?;
    if manifest.schema_version != SNAPSHOT_SCHEMA
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
        || manifest.authority_digest != authority
        || manifest.entries.is_empty()
        || manifest.entries.len() > 4096
    {
        return Err("WAKE_SOL_REVIEW_SNAPSHOT_INVALID".into());
    }
    let manifest_digest = json_sha(&(
        MANIFEST_DOMAIN,
        manifest.schema_version,
        &manifest.authority_digest,
        &manifest.entries,
    ))?;
    let snapshot_id = sha(format!(
        "{SNAPSHOT_ID_DOMAIN}|{}|{}",
        manifest.authority_digest, manifest_digest
    ));
    if manifest.manifest_digest != manifest_digest || manifest.snapshot_id != snapshot_id {
        return Err("WAKE_SOL_REVIEW_SNAPSHOT_DIGEST".into());
    }
    let mut last = None::<&str>;
    let mut paths = BTreeSet::new();
    let mut objects = BTreeSet::new();
    let mut total = 0u64;
    for entry in &manifest.entries {
        if entry.relative_path.len() > 512
            || !relative(&entry.relative_path)
            || entry.content_object != format!("bytes/{}", entry.relative_path)
            || !hex_sha(&entry.sha256)
            || entry.byte_length > 16 * 1024 * 1024
            || last.is_some_and(|previous| previous >= entry.relative_path.as_str())
        {
            return Err("WAKE_SOL_REVIEW_SNAPSHOT_INVALID".into());
        }
        last = Some(&entry.relative_path);
        if !paths.insert(entry.relative_path.to_lowercase()) {
            return Err("WAKE_SOL_REVIEW_SNAPSHOT_INVALID".into());
        }
        objects.insert(entry.content_object.clone());
        total = total
            .checked_add(entry.byte_length)
            .ok_or("WAKE_SOL_REVIEW_SNAPSHOT_INVALID")?;
        if total > 64 * 1024 * 1024 {
            return Err("WAKE_SOL_REVIEW_SNAPSHOT_INVALID".into());
        }
        let bytes = safe_file(
            workspace,
            &format!("{root}/{}", entry.content_object),
            entry.byte_length,
        )?;
        if bytes.len() as u64 != entry.byte_length || sha(bytes) != entry.sha256 {
            return Err("WAKE_SOL_REVIEW_SNAPSHOT_BYTES".into());
        }
    }
    for output in &canonical.current_outputs {
        if !manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == output.artifact_id && entry.sha256 == output.sha256)
        {
            return Err("WAKE_SOL_REVIEW_SNAPSHOT_OUTPUT".into());
        }
    }
    if ["Cargo.toml", "Cargo.lock", "wake/Cargo.toml"]
        .into_iter()
        .any(|required| {
            !manifest
                .entries
                .iter()
                .any(|entry| entry.relative_path == required)
        })
        || !manifest.entries.iter().any(|entry| {
            entry.relative_path.starts_with("src/") && entry.relative_path.ends_with(".rs")
        })
        || !manifest.entries.iter().any(|entry| {
            entry.relative_path.starts_with("wake/src/") && entry.relative_path.ends_with(".rs")
        })
        || enumerate_snapshot_objects(workspace, &format!("{root}/bytes"))? != objects
    {
        return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
    }
    Ok((snapshot_id, manifest_digest))
}

fn enumerate_snapshot_objects(workspace: &Path, bytes_root: &str) -> Result<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    let mut pending = vec![(bytes_root.to_string(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        if !relative(&directory) {
            return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
        }
        let mut parent = workspace.to_path_buf();
        for component in directory.split('/') {
            parent.push(component);
            let metadata =
                fs::symlink_metadata(&parent).map_err(|_| "WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE")?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x0400 != 0 {
                    return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
                }
            }
        }
        for item in fs::read_dir(&parent).map_err(|_| "WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE")? {
            let item = item.map_err(|_| "WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE")?;
            let name = item
                .file_name()
                .into_string()
                .map_err(|_| "WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE")?;
            let child = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{}", name)
            };
            if !relative(&child) {
                return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
            }
            let metadata = fs::symlink_metadata(item.path())
                .map_err(|_| "WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE")?;
            if metadata.file_type().is_symlink() {
                return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x0400 != 0 {
                    return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
                }
            }
            if metadata.is_dir() {
                pending.push((
                    format!(
                        "{directory}/{}",
                        child.rsplit('/').next().unwrap_or_default()
                    ),
                    child,
                ));
            } else if metadata.is_file() {
                result.insert(format!("bytes/{child}"));
                if result.len() > 4096 {
                    return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
                }
            } else {
                return Err("WAKE_SOL_REVIEW_SNAPSHOT_COVERAGE".into());
            }
        }
    }
    Ok(result)
}

fn object_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(_) => Err("WAKE_SOL_REVIEW_EVIDENCE_UNAVAILABLE".into()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewProjectRegistry {
    schema_version: u32,
    projects: Vec<ReviewProjectTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewProjectTarget {
    #[serde(default)]
    project_id: serde_json::Value,
    #[serde(default)]
    chatgpt_target_url: serde_json::Value,
    #[serde(default)]
    chatgpt_target_sha256: serde_json::Value,
}

/// Read only target metadata from the fixed control registry. Call again
/// under Wake's queue lock before publishing, so a guarded pair update cannot
/// interleave with the final registry/config comparison and event write.
pub(crate) fn validate_registered_review_target(
    workspace: &Path,
    project_id: &str,
    expected: &crate::protocol::Target,
) -> Result<()> {
    expected
        .validate()
        .map_err(|_| "REVIEW_PROJECT_TARGET_INVALID")?;
    let registry: ReviewProjectRegistry =
        read_json(workspace, ".catdesk/projects/projects.json", 1024 * 1024)
            .map_err(|_| "REVIEW_PROJECT_REGISTRY_INVALID")?;
    if registry.schema_version != 1 || registry.projects.len() > 1024 {
        return Err("REVIEW_PROJECT_REGISTRY_INVALID".into());
    }
    let matches: Vec<_> = registry
        .projects
        .iter()
        .filter(|project| project.project_id.as_str() == Some(project_id))
        .collect();
    if matches.is_empty() {
        return Err("REVIEW_PROJECT_NOT_REGISTERED".into());
    }
    if matches.len() != 1 {
        return Err("REVIEW_PROJECT_REGISTRATION_AMBIGUOUS".into());
    }
    if matches[0].chatgpt_target_url.as_str() != Some(expected.url.as_str())
        || matches[0].chatgpt_target_sha256.as_str() != Some(expected.digest.as_str())
    {
        return Err("REVIEW_PROJECT_TARGET_MISMATCH".into());
    }
    Ok(())
}

/// A missing contract is accepted only for an actual pre-contract session
/// whose persisted state has no approved contract hash. Terra keeps its
/// historical inbox contract. Every managed Sol review must pass all checks.
pub fn validate_completed_review(
    workspace: &Path,
    project_id: &str,
    session_id: &str,
    record_id: &str,
    reference: &str,
    created_at_unix: u64,
) -> Result<()> {
    let canonical_workspace = workspace
        .canonicalize()
        .map_err(|_| "WAKE_SOL_REVIEW_WORKSPACE_INVALID")?;
    let workspace = canonical_workspace.as_path();
    if !slug(project_id) || !slug(session_id) || !slug(record_id) || created_at_unix == 0 {
        return Err("WAKE_SOL_REVIEW_RECORD_INVALID".into());
    }
    let session_root = format!(".catdesk/autonomy/{session_id}");
    let state: Session = read_json(
        workspace,
        &format!("{session_root}/state.json"),
        2 * 1024 * 1024,
    )?;
    if state.schema_version != 1
        || state.session_id != session_id
        || state.state != "COMPLETED_VERIFIED"
        || state.active
    {
        return Err("WAKE_SOL_REVIEW_STATE_INVALID".into());
    }
    let binding_file = format!("{session_root}/artifacts/review-bindings/{record_id}.json");
    let contract_file = format!("{session_root}/contract.json");
    let contract_path = workspace.join(&contract_file);
    if !object_exists(&contract_path)? {
        if project_id != "catdesk" {
            return Err("WAKE_SOL_REVIEW_PROJECT_BINDING_REQUIRED".into());
        }
        return if state.approved_contract_hash.is_none()
            && !object_exists(&workspace.join(&binding_file))?
        {
            Ok(())
        } else {
            Err("WAKE_SOL_REVIEW_CONTRACT_MISSING".into())
        };
    }
    let contract_bytes = safe_file(workspace, &contract_file, 2 * 1024 * 1024)?;
    let contract: serde_json::Value =
        serde_json::from_slice(&contract_bytes).map_err(|_| "WAKE_SOL_REVIEW_CONTRACT_INVALID")?;
    let model = contract
        .pointer("/providerPolicy/primaryModel")
        .and_then(|value| value.as_str())
        .ok_or("WAKE_SOL_REVIEW_CONTRACT_INVALID")?;
    let hash = contract_hash(&contract_bytes)?;
    if contract
        .pointer("/providerPolicy/primaryProvider")
        .and_then(|value| value.as_str())
        != Some("codex-cli")
        || contract.get("projectId").and_then(|value| value.as_str()) != Some(project_id)
        || contract
            .get("workspace")
            .and_then(|value| value.as_str())
            .and_then(|path| Path::new(path).canonicalize().ok())
            .as_deref()
            != Some(workspace)
    {
        return Err("WAKE_SOL_REVIEW_CONTRACT_INVALID".into());
    }
    if model == TERRA {
        return if state.approved_contract_hash.as_deref() == Some(hash.as_str())
            && !object_exists(&workspace.join(&binding_file))?
        {
            Ok(())
        } else {
            Err("WAKE_SOL_REVIEW_LEGACY_BINDING".into())
        };
    }
    if model != SOL {
        return Err("WAKE_SOL_REVIEW_CONTRACT_INVALID".into());
    }
    if state.approved_contract_hash.as_deref() != Some(hash.as_str())
        || state.current_task_id.as_deref().is_none()
        || state.provider_turn_count == 0
        || state.codex_turn_profile_violation
        || !state.codex_turn_profile_tracking_started
        // The producer writes the inbox before its terminal transition;
        // that transition appends another state event. The immutable binding
        // owns the exact review sequence and the state may advance afterward.
        || !review_sequence(session_id, record_id)
            .is_some_and(|sequence| sequence <= state.last_event_sequence)
        || reference != "artifacts/completion.json"
    {
        return Err("WAKE_SOL_REVIEW_STATE_INVALID".into());
    }
    let checkpoint: Checkpoint = read_json(
        workspace,
        &format!("{session_root}/verification/passed-finalization.json"),
        2 * 1024 * 1024,
    )?;
    let completion: Completion = read_json(
        workspace,
        &format!("{session_root}/artifacts/completion.json"),
        2 * 1024 * 1024,
    )?;
    let profile = checkpoint
        .codex_turn_profile
        .as_ref()
        .ok_or("WAKE_SOL_REVIEW_NATIVE_TURN_MISSING")?;
    let expected = checkpoint
        .reviewed_source_expectation
        .as_ref()
        .ok_or("WAKE_SOL_REVIEW_SNAPSHOT_MISSING")?;
    if checkpoint.schema_version != 1
        || checkpoint.session_id != session_id
        || checkpoint.project_id != project_id
        || checkpoint.approved_contract_hash != hash
        || Some(checkpoint.logical_task_id.as_str()) != state.current_task_id.as_deref()
        || checkpoint.provider_id != "codex-cli"
        || checkpoint.provider_turn_count != state.provider_turn_count
        || !slug(&checkpoint.logical_task_id)
        || checkpoint.verification_profile.is_empty()
        || checkpoint.verification_profile.len() > 128
        || checkpoint.verification.status != "PASSED"
        || checkpoint.verification.command.is_empty()
        || checkpoint.verification.command.len() > 2_048
        || checkpoint.verification.summary.is_empty()
        || checkpoint.verification.summary.len() > 8 * 1024
        || checkpoint.authoritative_diff.trim().is_empty()
        || checkpoint.authoritative_diff.len() > 512 * 1024
        || sha(checkpoint.authoritative_diff.as_bytes()) != checkpoint.authoritative_diff_sha256
        || profile.codex_thread_id != state.provider_thread_id.as_deref().unwrap_or_default()
        || !slug(&profile.codex_turn_id)
        || profile.model_id != SOL
        || profile.reasoning_effort != "medium"
        || !hex_sha(&profile.evidence_sha256)
        || expected.session_id != session_id
        || expected.project_id != project_id
        || expected.approved_contract_hash != hash
        || expected.logical_task_id != checkpoint.logical_task_id
        || approved_outputs(&contract, &checkpoint.logical_task_id).as_deref()
            != Some(expected.completion_artifact_ids.as_slice())
        || completion.schema_version != 1
        || completion.session_id != session_id
        || completion.verification != checkpoint.verification
        || completion.authoritative_diff != checkpoint.authoritative_diff
        || completion.final_review.trim().is_empty()
        || completion.final_review.len() > 24 * 1024
    {
        return Err("WAKE_SOL_REVIEW_CHECKPOINT_INVALID".into());
    }
    let attribution = json_sha(&(
        hash.as_str(),
        checkpoint.logical_task_id.as_str(),
        Some(expected),
    ))?;
    if checkpoint.attribution_digest != attribution {
        return Err("WAKE_SOL_REVIEW_ATTRIBUTION_INVALID".into());
    }
    let (snapshot_id, manifest_digest) = snapshot(workspace, project_id, expected)?;
    let binding: Binding = read_json(workspace, &binding_file, 16 * 1024)?;
    let actual = Binding {
        schema_version: 1,
        record_id: record_id.into(),
        created_at_unix,
        session_id: session_id.into(),
        project_id: project_id.into(),
        reference: reference.into(),
        approved_contract_hash: hash,
        logical_task_id: checkpoint.logical_task_id.clone(),
        provider_turn_count: checkpoint.provider_turn_count,
        codex_turn_profile: NativeTurn {
            codex_thread_id: profile.codex_thread_id.clone(),
            codex_turn_id: profile.codex_turn_id.clone(),
            model_id: profile.model_id.clone(),
            reasoning_effort: profile.reasoning_effort.clone(),
            evidence_sha256: profile.evidence_sha256.clone(),
        },
        checkpoint_sha256: json_sha(&checkpoint)?,
        completion_sha256: json_sha(&completion)?,
        authoritative_diff_sha256: checkpoint.authoritative_diff_sha256.clone(),
        attribution_digest: checkpoint.attribution_digest.clone(),
        reviewed_source_expectation_sha256: json_sha(expected)?,
        reviewed_source_snapshot_id: snapshot_id,
        reviewed_source_manifest_digest: manifest_digest,
    };
    if binding != actual {
        return Err("WAKE_SOL_REVIEW_BINDING_MISMATCH".into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use serde_json::json;

    fn write_bytes(workspace: &Path, relative: &str, bytes: &[u8]) {
        let path = workspace.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("create fixture parent");
        fs::write(path, bytes).expect("write fixture bytes");
    }

    fn write_json<T: Serialize>(workspace: &Path, relative: &str, value: &T) {
        write_bytes(
            workspace,
            relative,
            &serde_json::to_vec_pretty(value).expect("fixture JSON"),
        );
    }

    /// Construct the producer's current wire shapes without linking Wake to
    /// CatDesk code. Runtime discovery tests exercise the complete boundary.
    pub(crate) fn completed_sol(
        workspace: &Path,
        session_id: &str,
        created_at_unix: u64,
    ) -> String {
        completed_sol_for_project(workspace, "catdesk", session_id, created_at_unix)
    }

    pub(crate) fn completed_sol_for_project(
        workspace: &Path,
        project_id: &str,
        session_id: &str,
        created_at_unix: u64,
    ) -> String {
        let record_id = format!("review-{session_id}-0-independent_final_review");
        let contract = json!({
            "schemaVersion":1,
            "taskId":"task-one",
            "projectId":project_id,
            "completionArtifactIds":["src/output.rs"],
            "workspace":workspace.to_string_lossy(),
            "providerPolicy":{"primaryProvider":"codex-cli","primaryModel":SOL}
        });
        let contract_bytes = serde_json::to_vec_pretty(&contract).expect("contract JSON");
        let hash = contract_hash(&contract_bytes).expect("contract hash");
        let root = format!(".catdesk/autonomy/{session_id}");
        write_bytes(workspace, &format!("{root}/contract.json"), &contract_bytes);
        write_json(
            workspace,
            &format!("{root}/state.json"),
            &json!({
                "schemaVersion":1,
                "sessionId":session_id,
                "state":"COMPLETED_VERIFIED",
                "currentTaskId":"task-one",
                "providerThreadId":"thread-one",
                "providerTurnCount":1,
                "codexTurnProfileViolation":false,
                "codexTurnProfileTrackingStarted":true,
                "approvedContractHash":hash,
                "lastEventSequence":1,
                "active":false
            }),
        );

        let files = [
            ("Cargo.lock", b"version = 4\n".as_slice()),
            ("Cargo.toml", b"[package]\nname='fixture'\n".as_slice()),
            ("src/output.rs", b"pub const OUTPUT: u8 = 1;\n".as_slice()),
            (
                "wake/Cargo.toml",
                b"[package]\nname='fixture-wake'\n".as_slice(),
            ),
            ("wake/src/lib.rs", b"pub const WAKE: u8 = 1;\n".as_slice()),
        ];
        let entries = files
            .iter()
            .map(|(path, bytes)| {
                write_bytes(
                    workspace,
                    &format!(".catdesk/reviewed-source-snapshots/{session_id}/bytes/{path}"),
                    bytes,
                );
                SnapshotEntry {
                    relative_path: (*path).into(),
                    byte_length: bytes.len() as u64,
                    sha256: sha(bytes),
                    content_object: format!("bytes/{path}"),
                }
            })
            .collect::<Vec<_>>();
        let expected = ExpectedSource {
            session_id: session_id.into(),
            project_id: project_id.into(),
            approved_contract_hash: hash.clone(),
            logical_task_id: "task-one".into(),
            completion_artifact_ids: vec!["src/output.rs".into()],
            current_outputs: vec![CurrentOutput {
                artifact_id: "src/output.rs".into(),
                sha256: sha(files[2].1),
            }],
            baseline_observations: vec![BaselineObservation {
                artifact_id: "src/output.rs".into(),
                state: "ABSENT".into(),
                sha256: None,
            }],
        };
        let authority = json_sha(&(
            AUTHORITY_DOMAIN,
            SNAPSHOT_SCHEMA,
            &expected.session_id,
            &expected.project_id,
            &expected.approved_contract_hash,
            &expected.logical_task_id,
            &expected.completion_artifact_ids,
            &expected.current_outputs,
            &expected.baseline_observations,
        ))
        .expect("authority digest");
        let manifest_digest = json_sha(&(MANIFEST_DOMAIN, SNAPSHOT_SCHEMA, &authority, &entries))
            .expect("manifest digest");
        let snapshot_id = sha(format!(
            "{SNAPSHOT_ID_DOMAIN}|{authority}|{manifest_digest}"
        ));
        let manifest = Manifest {
            schema_version: SNAPSHOT_SCHEMA,
            authority_domain: AUTHORITY_DOMAIN.into(),
            manifest_domain: MANIFEST_DOMAIN.into(),
            snapshot_id_domain: SNAPSHOT_ID_DOMAIN.into(),
            session_id: session_id.into(),
            project_id: project_id.into(),
            approved_contract_hash: hash.clone(),
            logical_task_id: "task-one".into(),
            completion_artifact_ids: expected.completion_artifact_ids.clone(),
            current_outputs: expected.current_outputs.clone(),
            baseline_observations: expected.baseline_observations.clone(),
            authority_digest: authority,
            entries,
            manifest_digest: manifest_digest.clone(),
            snapshot_id: snapshot_id.clone(),
        };
        write_json(
            workspace,
            &format!(".catdesk/reviewed-source-snapshots/{session_id}/manifest.json"),
            &manifest,
        );
        let diff = "diff --git a/src/output.rs b/src/output.rs";
        let checkpoint = Checkpoint {
            schema_version: 1,
            session_id: session_id.into(),
            project_id: project_id.into(),
            approved_contract_hash: hash.clone(),
            logical_task_id: "task-one".into(),
            provider_id: "codex-cli".into(),
            provider_turn_count: 1,
            verification_profile: "rust_full".into(),
            verification: Verification {
                status: "PASSED".into(),
                command: "cargo test".into(),
                summary: "passed".into(),
            },
            authoritative_diff: diff.into(),
            authoritative_diff_sha256: sha(diff.as_bytes()),
            attribution_digest: json_sha(&(hash.as_str(), "task-one", Some(&expected)))
                .expect("attribution"),
            codex_turn_profile: Some(NativeTurn {
                codex_thread_id: "thread-one".into(),
                codex_turn_id: "turn-one".into(),
                model_id: SOL.into(),
                reasoning_effort: "medium".into(),
                evidence_sha256: "a".repeat(64),
            }),
            reviewed_source_expectation: Some(expected),
        };
        write_json(
            workspace,
            &format!("{root}/verification/passed-finalization.json"),
            &checkpoint,
        );
        let completion = Completion {
            schema_version: 1,
            session_id: session_id.into(),
            verification: checkpoint.verification.clone(),
            authoritative_diff: diff.into(),
            final_review: "review passed".into(),
        };
        write_json(
            workspace,
            &format!("{root}/artifacts/completion.json"),
            &completion,
        );
        let binding = Binding {
            schema_version: 1,
            record_id: record_id.clone(),
            created_at_unix,
            session_id: session_id.into(),
            project_id: project_id.into(),
            reference: "artifacts/completion.json".into(),
            approved_contract_hash: hash,
            logical_task_id: "task-one".into(),
            provider_turn_count: 1,
            codex_turn_profile: checkpoint
                .codex_turn_profile
                .as_ref()
                .expect("profile")
                .clone(),
            checkpoint_sha256: json_sha(&checkpoint).expect("checkpoint digest"),
            completion_sha256: json_sha(&completion).expect("completion digest"),
            authoritative_diff_sha256: checkpoint.authoritative_diff_sha256.clone(),
            attribution_digest: checkpoint.attribution_digest.clone(),
            reviewed_source_expectation_sha256: json_sha(
                checkpoint
                    .reviewed_source_expectation
                    .as_ref()
                    .expect("expectation"),
            )
            .expect("expectation digest"),
            reviewed_source_snapshot_id: snapshot_id,
            reviewed_source_manifest_digest: manifest_digest,
        };
        write_json(
            workspace,
            &format!("{root}/artifacts/review-bindings/{record_id}.json"),
            &binding,
        );
        record_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn bound_sol_review_accepts_a_registered_non_catdesk_project_identity() {
        let workspace = std::env::temp_dir().join(format!(
            "wake-sol-project-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&workspace).expect("isolated workspace");
        let workspace = workspace.canonicalize().expect("canonical workspace");
        let record =
            test_support::completed_sol_for_project(&workspace, "binagotchy", "sol-project", 42);
        validate_completed_review(
            &workspace,
            "binagotchy",
            "sol-project",
            &record,
            "artifacts/completion.json",
            42,
        )
        .expect("bound non-CatDesk project");
        assert!(
            validate_completed_review(
                &workspace,
                "catdesk",
                "sol-project",
                &record,
                "artifacts/completion.json",
                42,
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(workspace);
    }
}
