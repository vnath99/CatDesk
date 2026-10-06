//! Fail-closed, non-executing authority gate for future GitHub publication.
//!
//! This module deliberately has no process, network, credential, or GitHub
//! client dependency.  It binds a future executor to the immutable autonomous
//! contract, exact observed repository identity, feature-branch authority, an
//! unconsumed approval of the matching kind, and a durable/replayable journal
//! state.  Until an approved executor consumes this gate, CatDesk continues to
//! perform no remote publication.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::autonomous_contract::{
    AutonomousApprovalKindV1, AutonomousApprovalV1, AutonomousGitActionV1,
    AutonomousPolicyEngineV1, ContractPolicyError,
};
use super::contracts::stable_hash;

pub(crate) const GITHUB_PUBLICATION_SCHEMA_VERSION: u32 = 1;
pub(crate) const T0407_R12_MANIFEST_PATH: &str =
    "docs/orchestrator/T-0407_R12_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json";
pub(crate) const T0407_R12_DESCRIPTOR_PATH: &str =
    "docs/orchestrator/T-0407_R12_GITHUB_PUBLICATION_DESCRIPTOR.json";
pub(crate) const T0407_R12_REVIEW_PATH: &str =
    "docs/orchestrator/review_bundles/T-0407_R12_POST_R11_GITHUB_RECOVERY_SNAPSHOT.md";
pub(crate) const T0407_R12_DESCRIPTOR_PURPOSE: &str = "GITHUB_RECOVERY_RECONSTRUCTION_PUBLICATION";
pub(crate) const CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH: &str =
    "docs/orchestrator/CURRENT_GITHUB_PUBLICATION_AUTHORITY.json";
pub(crate) const CURRENT_GITHUB_PUBLICATION_AUTHORITY_PURPOSE: &str =
    "GITHUB_RECOVERY_RECONSTRUCTION_PUBLICATION_AUTHORITY";

/// A deliberately narrow summary of the only currently reviewed recovery
/// snapshot. The executor receives literal entries from this parser; it never
/// accepts a caller-selected glob, directory, or path list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrozenPublicationManifestV1 {
    pub includes: Vec<FrozenPublicationManifestEntryV1>,
    pub archival_binary_exclusions: usize,
    pub runtime_generated_exclusions: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrozenPublicationManifestEntryV1 {
    pub path: String,
    pub sha256: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GithubPublicationDescriptorV1 {
    pub manifest_path: String,
    pub manifest_sha256: String,
    pub include_count: usize,
    pub archival_exclusion_count: usize,
    pub runtime_generated_exclusion_count: usize,
    pub supplemental_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GithubPublicationAuthorityPointerV1 {
    pub descriptor_path: String,
    pub descriptor_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrozenPublicationManifestErrorV1 {
    Schema,
    Classification,
    Path,
    Digest,
}

impl FrozenPublicationManifestErrorV1 {
    pub(crate) const fn reason_code(self) -> &'static str {
        match self {
            Self::Schema => "GITHUB_PUBLICATION_MANIFEST_SCHEMA_INVALID",
            Self::Classification => "GITHUB_PUBLICATION_MANIFEST_CLASSIFICATION_INVALID",
            Self::Path => "GITHUB_PUBLICATION_MANIFEST_PATH_INVALID",
            Self::Digest => "GITHUB_PUBLICATION_MANIFEST_DIGEST_MISMATCH",
        }
    }
}

pub(crate) fn parse_t0407_reconstruction_manifest(
    manifest_bytes: &[u8],
) -> Result<FrozenPublicationManifestV1, FrozenPublicationManifestErrorV1> {
    let root: Value = serde_json::from_slice(manifest_bytes)
        .map_err(|_| FrozenPublicationManifestErrorV1::Schema)?;
    if root.get("schemaVersion").and_then(Value::as_u64) != Some(2) {
        return Err(FrozenPublicationManifestErrorV1::Schema);
    }
    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(FrozenPublicationManifestErrorV1::Schema)?;
    if entries.is_empty() || entries.len() > 4_096 {
        return Err(FrozenPublicationManifestErrorV1::Classification);
    }
    let mut includes = Vec::new();
    let mut archival = 0usize;
    let mut runtime = 0usize;
    for entry in entries {
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or(FrozenPublicationManifestErrorV1::Schema)?;
        if !valid_manifest_path(path) {
            return Err(FrozenPublicationManifestErrorV1::Path);
        }
        if path == CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH {
            return Err(FrozenPublicationManifestErrorV1::Classification);
        }
        match entry.get("classification").and_then(Value::as_str) {
            Some("INCLUDE_RECONSTRUCTION") => {
                let sha256 = entry
                    .get("sha256")
                    .and_then(Value::as_str)
                    .filter(|value| {
                        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                    .ok_or(FrozenPublicationManifestErrorV1::Schema)?;
                let byte_length = entry
                    .get("byteLength")
                    .and_then(Value::as_u64)
                    .ok_or(FrozenPublicationManifestErrorV1::Schema)?;
                includes.push(FrozenPublicationManifestEntryV1 {
                    path: path.to_owned(),
                    sha256: sha256.to_ascii_lowercase(),
                    byte_length,
                });
            }
            Some("EXCLUDE_ARCHIVAL_BINARY") if path.ends_with(".zip") => archival += 1,
            Some("EXCLUDE_RUNTIME_GENERATED")
                if path == "src/daemon-reload-approval-request-v1.json" =>
            {
                runtime += 1;
            }
            _ => return Err(FrozenPublicationManifestErrorV1::Classification),
        }
    }
    includes.sort_by(|left, right| left.path.cmp(&right.path));
    if includes.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(FrozenPublicationManifestErrorV1::Classification);
    }
    if includes.is_empty() || archival > 128 || runtime > 16 {
        return Err(FrozenPublicationManifestErrorV1::Classification);
    }
    Ok(FrozenPublicationManifestV1 {
        includes,
        archival_binary_exclusions: archival,
        runtime_generated_exclusions: runtime,
    })
}

pub(crate) fn parse_current_github_publication_authority(
    bytes: &[u8],
) -> Result<Option<GithubPublicationAuthorityPointerV1>, FrozenPublicationManifestErrorV1> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FrozenPublicationManifestErrorV1::Schema)?;
    let object = root
        .as_object()
        .ok_or(FrozenPublicationManifestErrorV1::Schema)?;
    if root.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || root.get("purpose").and_then(Value::as_str)
            != Some(CURRENT_GITHUB_PUBLICATION_AUTHORITY_PURPOSE)
        || root.get("projectId").and_then(Value::as_str) != Some("catdesk")
    {
        return Err(FrozenPublicationManifestErrorV1::Schema);
    }
    match root.get("state").and_then(Value::as_str) {
        Some("UNBOUND") if object.len() == 4 => Ok(None),
        Some("BOUND") if object.len() == 6 => {
            let descriptor_path = root
                .get("descriptorPath")
                .and_then(Value::as_str)
                .filter(|path| valid_publication_descriptor_path(path))
                .ok_or(FrozenPublicationManifestErrorV1::Path)?;
            let descriptor_sha256 = root
                .get("descriptorSha256")
                .and_then(Value::as_str)
                .filter(|value| {
                    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .ok_or(FrozenPublicationManifestErrorV1::Digest)?;
            Ok(Some(GithubPublicationAuthorityPointerV1 {
                descriptor_path: descriptor_path.to_owned(),
                descriptor_sha256: descriptor_sha256.to_ascii_lowercase(),
            }))
        }
        _ => Err(FrozenPublicationManifestErrorV1::Schema),
    }
}

pub(crate) fn parse_github_publication_descriptor(
    bytes: &[u8],
    descriptor_path: &str,
) -> Result<GithubPublicationDescriptorV1, FrozenPublicationManifestErrorV1> {
    if !valid_publication_descriptor_path(descriptor_path) {
        return Err(FrozenPublicationManifestErrorV1::Path);
    }
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FrozenPublicationManifestErrorV1::Schema)?;
    if root.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || root.get("purpose").and_then(Value::as_str) != Some(T0407_R12_DESCRIPTOR_PURPOSE)
        || root.get("projectId").and_then(Value::as_str) != Some("catdesk")
    {
        return Err(FrozenPublicationManifestErrorV1::Schema);
    }
    let manifest_path = root
        .get("manifestPath")
        .and_then(Value::as_str)
        .filter(|path| valid_publication_manifest_path(path))
        .ok_or(FrozenPublicationManifestErrorV1::Path)?;
    let manifest_sha256 = root
        .get("manifestSha256")
        .and_then(Value::as_str)
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or(FrozenPublicationManifestErrorV1::Schema)?;
    let include_count = root
        .get("includeCount")
        .and_then(Value::as_u64)
        .ok_or(FrozenPublicationManifestErrorV1::Schema)? as usize;
    let archival_exclusion_count =
        root.get("archivalExclusionCount")
            .and_then(Value::as_u64)
            .ok_or(FrozenPublicationManifestErrorV1::Schema)? as usize;
    let runtime_generated_exclusion_count =
        root.get("runtimeGeneratedExclusionCount")
            .and_then(Value::as_u64)
            .ok_or(FrozenPublicationManifestErrorV1::Schema)? as usize;
    let supplemental_paths = root
        .get("supplementalReviewedArtifactPaths")
        .and_then(Value::as_array)
        .ok_or(FrozenPublicationManifestErrorV1::Schema)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|path| valid_manifest_path(path))
                .map(str::to_owned)
                .ok_or(FrozenPublicationManifestErrorV1::Path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !(supplemental_paths.len() == 2 || supplemental_paths.len() == 3)
        || supplemental_paths[0] != descriptor_path
        || !valid_publication_review_path(&supplemental_paths[1])
        || (supplemental_paths.len() == 3
            && supplemental_paths[2] != CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH)
        || include_count == 0
        || archival_exclusion_count != 12
        || runtime_generated_exclusion_count != 1
    {
        return Err(FrozenPublicationManifestErrorV1::Classification);
    }
    Ok(GithubPublicationDescriptorV1 {
        manifest_path: manifest_path.to_owned(),
        manifest_sha256: manifest_sha256.to_ascii_lowercase(),
        include_count,
        archival_exclusion_count,
        runtime_generated_exclusion_count,
        supplemental_paths,
    })
}

pub(crate) fn parse_t0407_r12_publication_descriptor(
    bytes: &[u8],
) -> Result<GithubPublicationDescriptorV1, FrozenPublicationManifestErrorV1> {
    let descriptor = parse_github_publication_descriptor(bytes, T0407_R12_DESCRIPTOR_PATH)?;
    let expected: Vec<String> = vec![
        T0407_R12_DESCRIPTOR_PATH.into(),
        T0407_R12_REVIEW_PATH.into(),
    ];
    if descriptor.manifest_path != T0407_R12_MANIFEST_PATH
        || descriptor.supplemental_paths != expected
    {
        return Err(FrozenPublicationManifestErrorV1::Classification);
    }
    Ok(descriptor)
}

fn valid_publication_manifest_path(path: &str) -> bool {
    valid_manifest_path(path)
        && path.starts_with("docs/orchestrator/")
        && path.ends_with("_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json")
}

fn valid_publication_descriptor_path(path: &str) -> bool {
    valid_manifest_path(path)
        && path.starts_with("docs/orchestrator/")
        && path.ends_with("_GITHUB_PUBLICATION_DESCRIPTOR.json")
}

fn valid_publication_review_path(path: &str) -> bool {
    valid_manifest_path(path)
        && path.starts_with("docs/orchestrator/review_bundles/")
        && path.ends_with(".md")
}

fn valid_manifest_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && !path.starts_with('-')
        && !path.contains('\\')
        && !path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

/// The only remote operations which a future, separately approved executor
/// may ask this gate to authorize.  Force-push and direct protected-branch
/// mutation are intentionally not representable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum GithubPublicationOperationV1 {
    PushFeatureBranch,
    OpenPullRequest,
    MergeApprovedPullRequest,
}

/// Exact, non-secret repository evidence obtained by the future trusted Git
/// probe.  It is compared to the already-approved contract; it is never a
/// repository selector.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubRepositoryEvidenceV1 {
    pub origin: String,
    pub current_branch: String,
    pub head_commit: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubPublicationRequestV1 {
    pub operation: GithubPublicationOperationV1,
    pub repository: GithubRepositoryEvidenceV1,
    pub approval: AutonomousApprovalV1,
}

/// Persist this value alongside the autonomous session before a future
/// executor makes a remote call.  Its fields are deliberately private so a
/// caller cannot forge a follow-on transition from an arbitrary key/hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GithubPublicationPermitV1 {
    idempotency_key: String,
    intent_hash: String,
}

impl GithubPublicationPermitV1 {
    #[cfg(test)]
    pub(crate) fn for_test(idempotency_key: &str, intent_hash: &str) -> Self {
        Self {
            idempotency_key: idempotency_key.into(),
            intent_hash: intent_hash.into(),
        }
    }

    pub(crate) fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    pub(crate) fn intent_hash(&self) -> &str {
        &self.intent_hash
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum GithubPublicationRecordStateV1 {
    /// A permit was minted but there is no evidence that a remote request was
    /// dispatched.  Automatic replay is unsafe and therefore refused.
    Prepared,
    /// A remote request may have reached GitHub.  A trusted reconciliation is
    /// required before any retry.
    RemoteOutcomeUnknown,
    /// A trusted executor observed the exact requested remote outcome.
    Confirmed,
    /// A trusted reconciliation proved the exact remote operation did not
    /// happen.  One subsequent authorization of the same intent is allowed.
    ReconciledNotApplied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubPublicationRecordV1 {
    pub schema_version: u32,
    pub intent_hash: String,
    pub state: GithubPublicationRecordStateV1,
}

/// Evidence retained between the closed PREPARE and CONFIRM operations.  It
/// contains no credentials, command output, or caller-selected paths.
fn default_publication_manifest_path() -> String {
    T0407_R12_MANIFEST_PATH.into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubPublicationPreparedV1 {
    pub schema_version: u32,
    pub idempotency_key: String,
    pub intent_hash: String,
    pub review_record_id: String,
    pub review_digest: String,
    #[serde(default = "default_publication_manifest_path")]
    pub manifest_path: String,
    pub manifest_sha256: String,
    #[serde(default)]
    pub manifest_include_count: usize,
    #[serde(default)]
    pub archival_exclusion_count: usize,
    #[serde(default)]
    pub runtime_generated_exclusion_count: usize,
    #[serde(default)]
    pub supplemental_paths: Vec<String>,
    pub evidence_fingerprint: String,
    pub expires_at_unix: u64,
    pub repository: GithubRepositoryEvidenceV1,
    #[serde(default)]
    pub git_executable_fingerprint: String,
    #[serde(default)]
    pub git_selected_slot: u8,
    /// Canonical executable identity selected from the fixed trusted-candidate
    /// list. This is never caller supplied and is compared again before every
    /// subsequent Git operation.
    #[serde(default)]
    pub git_selected_identity: String,
}

/// Durable state is intentionally a small serializable value: the autonomous
/// session store owns its persistence and must write it atomically with the
/// dispatch intent.  This type itself has no filesystem authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubPublicationJournalV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub records: BTreeMap<String, GithubPublicationRecordV1>,
    #[serde(default)]
    pub prepared: BTreeMap<String, GithubPublicationPreparedV1>,
}

impl Default for GithubPublicationJournalV1 {
    fn default() -> Self {
        Self {
            schema_version: GITHUB_PUBLICATION_SCHEMA_VERSION,
            records: BTreeMap::new(),
            prepared: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GithubPublicationGateErrorV1 {
    JournalSchemaInvalid,
    ContractPolicy,
    RepositoryIdentityMismatch,
    BranchAuthorityMismatch,
    HeadEvidenceInvalid,
    ApprovalKindMismatch,
    ApprovalInvalid,
    IdempotencyConflict,
    RecoveryRequired,
    AlreadyCompleted,
    TransitionInvalid,
}

impl GithubPublicationGateErrorV1 {
    pub(crate) const fn reason_code(&self) -> &'static str {
        match self {
            Self::JournalSchemaInvalid => "GITHUB_PUBLICATION_JOURNAL_INVALID",
            Self::ContractPolicy => "GITHUB_PUBLICATION_CONTRACT_DENIED",
            Self::RepositoryIdentityMismatch => "GITHUB_PUBLICATION_REPOSITORY_MISMATCH",
            Self::BranchAuthorityMismatch => "GITHUB_PUBLICATION_BRANCH_DENIED",
            Self::HeadEvidenceInvalid => "GITHUB_PUBLICATION_HEAD_INVALID",
            Self::ApprovalKindMismatch => "GITHUB_PUBLICATION_APPROVAL_KIND_DENIED",
            Self::ApprovalInvalid => "GITHUB_PUBLICATION_APPROVAL_INVALID",
            Self::IdempotencyConflict => "GITHUB_PUBLICATION_IDEMPOTENCY_CONFLICT",
            Self::RecoveryRequired => "GITHUB_PUBLICATION_RECOVERY_REQUIRED",
            Self::AlreadyCompleted => "GITHUB_PUBLICATION_ALREADY_COMPLETED",
            Self::TransitionInvalid => "GITHUB_PUBLICATION_TRANSITION_INVALID",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BoundPublicationIntent<'a> {
    schema_version: u32,
    contract_hash: &'a str,
    project_id: &'a str,
    base_branch: &'a str,
    feature_branch: &'a str,
    operation: &'a GithubPublicationOperationV1,
    repository: &'a GithubRepositoryEvidenceV1,
    approval_id: &'a str,
    approval_kind: &'a AutonomousApprovalKindV1,
}

impl GithubPublicationJournalV1 {
    pub(crate) fn validate(&self) -> Result<(), GithubPublicationGateErrorV1> {
        if self.schema_version != GITHUB_PUBLICATION_SCHEMA_VERSION
            || self.records.len() > 256
            || self.prepared.len() > 256
        {
            return Err(GithubPublicationGateErrorV1::JournalSchemaInvalid);
        }
        for (key, record) in &self.records {
            if !valid_idempotency_key(key)
                || record.schema_version != GITHUB_PUBLICATION_SCHEMA_VERSION
                || record.intent_hash.len() != "fnv1a64:".len() + 16
                || !record.intent_hash.starts_with("fnv1a64:")
                || !record.intent_hash["fnv1a64:".len()..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(GithubPublicationGateErrorV1::JournalSchemaInvalid);
            }
        }
        for (token, prepared) in &self.prepared {
            if !valid_confirmation_token(token)
                || !valid_idempotency_key(&prepared.idempotency_key)
                || prepared.schema_version != GITHUB_PUBLICATION_SCHEMA_VERSION
                || prepared.intent_hash.len() != "fnv1a64:".len() + 16
                || !prepared.intent_hash.starts_with("fnv1a64:")
                || !prepared.intent_hash["fnv1a64:".len()..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || prepared.review_record_id.is_empty()
                || prepared.review_record_id.len() > 128
                || prepared.review_digest.len() != 64
                || !prepared
                    .review_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || !valid_publication_manifest_path(&prepared.manifest_path)
                || prepared.manifest_sha256.len() != 64
                || !prepared
                    .manifest_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || prepared.manifest_include_count == 0
                || prepared.archival_exclusion_count != 12
                || prepared.runtime_generated_exclusion_count != 1
                || !(prepared.supplemental_paths.len() == 2
                    || prepared.supplemental_paths.len() == 3)
                || prepared
                    .supplemental_paths
                    .iter()
                    .any(|path| !valid_manifest_path(path))
                || (prepared.supplemental_paths.len() == 3
                    && prepared.supplemental_paths[2] != CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH)
                || prepared.evidence_fingerprint.len() != 64
                || !prepared
                    .evidence_fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || !valid_commit_id(&prepared.repository.head_commit)
                || prepared.repository.origin.is_empty()
                || prepared.repository.origin.len() > 1024
                || prepared.repository.current_branch.is_empty()
                || prepared.repository.current_branch.len() > 256
                || prepared.git_executable_fingerprint.len() != 64
                || !prepared
                    .git_executable_fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || prepared.git_selected_identity.is_empty()
                || prepared.git_selected_identity.len() > 1_024
                || prepared.git_selected_identity.contains('\r')
                || prepared.git_selected_identity.contains('\n')
                || !self.records.contains_key(&prepared.idempotency_key)
            {
                return Err(GithubPublicationGateErrorV1::JournalSchemaInvalid);
            }
        }
        Ok(())
    }

    /// Validates all authority before creating the one durable dispatch
    /// record.  It does not invoke Git, `gh`, a network client, or a shell.
    pub(crate) fn authorize(
        &mut self,
        policy: &AutonomousPolicyEngineV1,
        request: &GithubPublicationRequestV1,
        now_unix: u64,
    ) -> Result<GithubPublicationPermitV1, GithubPublicationGateErrorV1> {
        self.validate()?;
        let action = match request.operation {
            GithubPublicationOperationV1::PushFeatureBranch => {
                AutonomousGitActionV1::PushFeatureBranch
            }
            GithubPublicationOperationV1::OpenPullRequest => AutonomousGitActionV1::OpenPullRequest,
            GithubPublicationOperationV1::MergeApprovedPullRequest => AutonomousGitActionV1::Merge,
        };
        policy
            .permit_git_action(action)
            .map_err(|_| GithubPublicationGateErrorV1::ContractPolicy)?;
        if request.repository.origin != policy.contract().expected_origin {
            return Err(GithubPublicationGateErrorV1::RepositoryIdentityMismatch);
        }
        // A remote operation may only originate from the contract's feature
        // branch.  `modify_main` is separately prohibited by contract
        // validation and has no representation in this gate.
        if request.repository.current_branch != policy.contract().feature_branch
            || request.repository.current_branch == policy.contract().base_branch
        {
            return Err(GithubPublicationGateErrorV1::BranchAuthorityMismatch);
        }
        if !valid_commit_id(&request.repository.head_commit) {
            return Err(GithubPublicationGateErrorV1::HeadEvidenceInvalid);
        }
        policy
            .validate_approval(&request.approval, now_unix)
            .map_err(map_approval_error)?;
        let expected_kind = match request.operation {
            GithubPublicationOperationV1::PushFeatureBranch
            | GithubPublicationOperationV1::OpenPullRequest => AutonomousApprovalKindV1::GitPush,
            GithubPublicationOperationV1::MergeApprovedPullRequest => {
                AutonomousApprovalKindV1::Merge
            }
        };
        if request.approval.kind != expected_kind {
            return Err(GithubPublicationGateErrorV1::ApprovalKindMismatch);
        }
        let intent_hash = stable_hash(&BoundPublicationIntent {
            schema_version: GITHUB_PUBLICATION_SCHEMA_VERSION,
            contract_hash: policy.contract_hash(),
            project_id: &policy.contract().project_id,
            base_branch: &policy.contract().base_branch,
            feature_branch: &policy.contract().feature_branch,
            operation: &request.operation,
            repository: &request.repository,
            approval_id: &request.approval.approval_id,
            approval_kind: &request.approval.kind,
        })
        .map_err(|_| GithubPublicationGateErrorV1::ApprovalInvalid)?;
        let key = &request.approval.idempotency_key;
        if let Some(record) = self.records.get_mut(key) {
            if record.intent_hash != intent_hash {
                return Err(GithubPublicationGateErrorV1::IdempotencyConflict);
            }
            match record.state {
                GithubPublicationRecordStateV1::Confirmed => {
                    return Err(GithubPublicationGateErrorV1::AlreadyCompleted);
                }
                GithubPublicationRecordStateV1::Prepared
                | GithubPublicationRecordStateV1::RemoteOutcomeUnknown => {
                    return Err(GithubPublicationGateErrorV1::RecoveryRequired);
                }
                GithubPublicationRecordStateV1::ReconciledNotApplied => {
                    record.state = GithubPublicationRecordStateV1::Prepared;
                }
            }
        } else {
            self.records.insert(
                key.clone(),
                GithubPublicationRecordV1 {
                    schema_version: GITHUB_PUBLICATION_SCHEMA_VERSION,
                    intent_hash: intent_hash.clone(),
                    state: GithubPublicationRecordStateV1::Prepared,
                },
            );
        }
        Ok(GithubPublicationPermitV1 {
            idempotency_key: key.clone(),
            intent_hash,
        })
    }

    /// Persist this transition immediately before a future executor dispatches
    /// remotely.  Recovery cannot retry until it has independently reconciled
    /// the exact remote effect.
    pub(crate) fn mark_remote_dispatch_started(
        &mut self,
        permit: &GithubPublicationPermitV1,
    ) -> Result<(), GithubPublicationGateErrorV1> {
        self.transition(
            permit,
            GithubPublicationRecordStateV1::Prepared,
            GithubPublicationRecordStateV1::RemoteOutcomeUnknown,
        )
    }

    pub(crate) fn mark_remote_confirmed(
        &mut self,
        permit: &GithubPublicationPermitV1,
    ) -> Result<(), GithubPublicationGateErrorV1> {
        self.transition(
            permit,
            GithubPublicationRecordStateV1::RemoteOutcomeUnknown,
            GithubPublicationRecordStateV1::Confirmed,
        )
    }

    /// This transition is permitted only after a future trusted reconciler
    /// proves the exact operation was not applied.  It never deletes history.
    pub(crate) fn mark_remote_not_applied(
        &mut self,
        permit: &GithubPublicationPermitV1,
    ) -> Result<(), GithubPublicationGateErrorV1> {
        self.transition(
            permit,
            GithubPublicationRecordStateV1::RemoteOutcomeUnknown,
            GithubPublicationRecordStateV1::ReconciledNotApplied,
        )
    }

    pub(crate) fn bind_prepared(
        &mut self,
        token: String,
        permit: &GithubPublicationPermitV1,
        prepared: GithubPublicationPreparedV1,
    ) -> Result<(), GithubPublicationGateErrorV1> {
        if prepared.idempotency_key != permit.idempotency_key
            || prepared.intent_hash != permit.intent_hash
            || self.prepared.contains_key(&token)
            || !matches!(
                self.records.get(&permit.idempotency_key),
                Some(record) if record.intent_hash == permit.intent_hash
                    && record.state == GithubPublicationRecordStateV1::Prepared
            )
        {
            return Err(GithubPublicationGateErrorV1::TransitionInvalid);
        }
        self.prepared.insert(token, prepared);
        self.validate()
    }

    pub(crate) fn prepared(
        &self,
        token: &str,
    ) -> Result<
        (&GithubPublicationPreparedV1, GithubPublicationPermitV1),
        GithubPublicationGateErrorV1,
    > {
        let prepared = self
            .prepared
            .get(token)
            .ok_or(GithubPublicationGateErrorV1::TransitionInvalid)?;
        let record = self
            .records
            .get(&prepared.idempotency_key)
            .ok_or(GithubPublicationGateErrorV1::TransitionInvalid)?;
        if record.intent_hash != prepared.intent_hash {
            return Err(GithubPublicationGateErrorV1::TransitionInvalid);
        }
        Ok((
            prepared,
            GithubPublicationPermitV1 {
                idempotency_key: prepared.idempotency_key.clone(),
                intent_hash: prepared.intent_hash.clone(),
            },
        ))
    }

    pub(crate) fn state_for(
        &self,
        permit: &GithubPublicationPermitV1,
    ) -> Result<GithubPublicationRecordStateV1, GithubPublicationGateErrorV1> {
        let record = self
            .records
            .get(permit.idempotency_key())
            .ok_or(GithubPublicationGateErrorV1::TransitionInvalid)?;
        if record.intent_hash != permit.intent_hash() {
            return Err(GithubPublicationGateErrorV1::TransitionInvalid);
        }
        Ok(record.state.clone())
    }

    fn transition(
        &mut self,
        permit: &GithubPublicationPermitV1,
        expected: GithubPublicationRecordStateV1,
        next: GithubPublicationRecordStateV1,
    ) -> Result<(), GithubPublicationGateErrorV1> {
        let Some(record) = self.records.get_mut(&permit.idempotency_key) else {
            return Err(GithubPublicationGateErrorV1::TransitionInvalid);
        };
        if record.intent_hash != permit.intent_hash || record.state != expected {
            return Err(GithubPublicationGateErrorV1::TransitionInvalid);
        }
        record.state = next;
        Ok(())
    }
}

fn map_approval_error(error: ContractPolicyError) -> GithubPublicationGateErrorV1 {
    match error {
        ContractPolicyError::Approval(_) => GithubPublicationGateErrorV1::ApprovalInvalid,
        _ => GithubPublicationGateErrorV1::ContractPolicy,
    }
}

fn valid_commit_id(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_idempotency_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_confirmation_token(value: &str) -> bool {
    value.len() == "gpub-".len() + 64
        && value.starts_with("gpub-")
        && value["gpub-".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::autonomous_contract::{
        AutonomousCommandProfileV1, AutonomousDevelopmentContractV1, AutonomousGitPolicyV1,
        AutonomousHardStopV1, AutonomousProviderPolicyV1, AutonomousRateLimitPolicyV1,
        AutonomousVerificationPolicyV1, AutonomyLeaseV1,
    };
    use crate::delegated::autonomy_state::{AutonomousQueueV1, AutonomousStateStoreV1};
    use uuid::Uuid;

    fn policy() -> AutonomousPolicyEngineV1 {
        let root = std::env::temp_dir().join(format!("catdesk-publication-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        AutonomousPolicyEngineV1::new(AutonomousDevelopmentContractV1 {
            schema_version: 1,
            contract_id: "publication-contract".into(),
            task_id: "publication-task".into(),
            project_id: "catdesk".into(),
            mode: "chatgpt_web_codex_autonomous".into(),
            objective: "test publication authority".into(),
            workspace: root.clone(),
            base_branch: "main".into(),
            feature_branch: "feature-publication".into(),
            base_commit: "0123456".into(),
            expected_origin: "https://github.example.invalid/catdesk.git".into(),
            ordered_steps: vec!["verify".into()],
            allowed_paths: vec![root.join("src")],
            forbidden_paths: Vec::new(),
            allowed_command_profiles: vec![AutonomousCommandProfileV1::GitStatus],
            git_policy: AutonomousGitPolicyV1 {
                create_branch: false,
                create_worktree: false,
                local_commits: false,
                push_feature_branch: true,
                open_pull_request: true,
                merge: true,
                force_push: false,
                modify_main: false,
            },
            provider_policy: AutonomousProviderPolicyV1 {
                primary_provider: "codex-cli".into(),
                primary_model: "gpt-5.6-terra".into(),
                routine_provider: "ollama".into(),
                routine_model: "qwen3.8:27b".into(),
                allow_paid_fallback: false,
                allow_cloud_fallback: false,
            },
            verification_policy: AutonomousVerificationPolicyV1 {
                profile: "rust_full".into(),
                required_commands: vec![AutonomousCommandProfileV1::GitStatus],
                max_repair_cycles: 1,
                require_authoritative_diff: true,
                require_final_review: true,
            },
            rate_limit_policy: AutonomousRateLimitPolicyV1 {
                automatic_pause: true,
                automatic_resume: true,
                initial_backoff_seconds: 1,
                maximum_backoff_seconds: 2,
                maximum_rate_limited_seconds: 3,
                honor_provider_retry_after: true,
            },
            autonomy_lease: AutonomyLeaseV1 {
                start_approval_required: true,
                renewal_allowed: true,
                issued_at_unix: 1,
                expires_at_unix: 100,
                maximum_total_elapsed_seconds: 99,
                maximum_provider_turns: 1,
                maximum_tool_calls: 1,
                maximum_consecutive_failures: 1,
                maximum_repair_cycles: 1,
            },
            hard_stop_conditions: vec![AutonomousHardStopV1::CancellationRequested],
            completion_artifact_ids: Vec::new(),
            task_graph: Vec::new(),
        })
        .expect("policy")
    }

    fn request(
        policy: &AutonomousPolicyEngineV1,
        operation: GithubPublicationOperationV1,
    ) -> GithubPublicationRequestV1 {
        let kind = match operation {
            GithubPublicationOperationV1::MergeApprovedPullRequest => {
                AutonomousApprovalKindV1::Merge
            }
            GithubPublicationOperationV1::PushFeatureBranch
            | GithubPublicationOperationV1::OpenPullRequest => AutonomousApprovalKindV1::GitPush,
        };
        GithubPublicationRequestV1 {
            operation,
            repository: GithubRepositoryEvidenceV1 {
                origin: policy.contract().expected_origin.clone(),
                current_branch: policy.contract().feature_branch.clone(),
                head_commit: "abcdef0123456789".into(),
            },
            approval: AutonomousApprovalV1 {
                schema_version: 1,
                approval_id: "publication-approval".into(),
                contract_hash: policy.contract_hash().into(),
                kind,
                idempotency_key: "publication-action".into(),
                expires_at_unix: 50,
                consumed: false,
            },
        }
    }

    #[test]
    fn authorization_binds_contract_repo_branch_and_typed_approval() {
        let policy = policy();
        let mut journal = GithubPublicationJournalV1::default();
        let request = request(&policy, GithubPublicationOperationV1::PushFeatureBranch);
        assert!(journal.authorize(&policy, &request, 10).is_ok());

        let mut bad_origin = request.clone();
        bad_origin.repository.origin = "https://github.example.invalid/other.git".into();
        assert_eq!(
            GithubPublicationJournalV1::default()
                .authorize(&policy, &bad_origin, 10)
                .expect_err("origin denied")
                .reason_code(),
            "GITHUB_PUBLICATION_REPOSITORY_MISMATCH"
        );
        let mut bad_branch = request.clone();
        bad_branch.repository.current_branch = "main".into();
        assert_eq!(
            GithubPublicationJournalV1::default()
                .authorize(&policy, &bad_branch, 10)
                .expect_err("branch denied")
                .reason_code(),
            "GITHUB_PUBLICATION_BRANCH_DENIED"
        );
        let mut wrong_kind = request;
        wrong_kind.approval.kind = AutonomousApprovalKindV1::Merge;
        assert_eq!(
            GithubPublicationJournalV1::default()
                .authorize(&policy, &wrong_kind, 10)
                .expect_err("kind denied")
                .reason_code(),
            "GITHUB_PUBLICATION_APPROVAL_KIND_DENIED"
        );
    }

    #[test]
    fn disabled_contract_and_protected_branch_actions_fail_closed() {
        let mut contract = policy().contract().clone();
        contract.git_policy.push_feature_branch = false;
        let disabled = AutonomousPolicyEngineV1::new(contract).expect("disabled policy");
        assert_eq!(
            GithubPublicationJournalV1::default()
                .authorize(
                    &disabled,
                    &request(&disabled, GithubPublicationOperationV1::PushFeatureBranch),
                    10,
                )
                .expect_err("no publication policy")
                .reason_code(),
            "GITHUB_PUBLICATION_CONTRACT_DENIED"
        );
        assert!(!valid_commit_id("not-a-commit"));
        assert!(!valid_commit_id("123456"));
    }

    #[test]
    fn remote_partial_failure_requires_reconciliation_and_survives_restart() {
        let policy = policy();
        let request = request(&policy, GithubPublicationOperationV1::OpenPullRequest);
        let mut journal = GithubPublicationJournalV1::default();
        let permit = journal.authorize(&policy, &request, 10).expect("permit");
        journal
            .mark_remote_dispatch_started(&permit)
            .expect("dispatch journaled");
        let encoded = serde_json::to_vec(&journal).expect("serialize durable state");
        let mut restored: GithubPublicationJournalV1 =
            serde_json::from_slice(&encoded).expect("restore durable state");
        assert_eq!(
            restored
                .authorize(&policy, &request, 10)
                .expect_err("do not blindly replay")
                .reason_code(),
            "GITHUB_PUBLICATION_RECOVERY_REQUIRED"
        );
        restored
            .mark_remote_not_applied(&permit)
            .expect("trusted not-applied reconciliation");
        let retry = restored
            .authorize(&policy, &request, 10)
            .expect("one retry after proof");
        restored
            .mark_remote_dispatch_started(&retry)
            .expect("retry dispatch journaled");
        restored.mark_remote_confirmed(&retry).expect("confirmed");
        assert_eq!(
            restored
                .authorize(&policy, &request, 10)
                .expect_err("idempotent completion")
                .reason_code(),
            "GITHUB_PUBLICATION_ALREADY_COMPLETED"
        );
    }

    #[test]
    fn conflicting_replay_and_invalid_transitions_are_refused() {
        let policy = policy();
        let request = request(
            &policy,
            GithubPublicationOperationV1::MergeApprovedPullRequest,
        );
        let mut journal = GithubPublicationJournalV1::default();
        let permit = journal.authorize(&policy, &request, 10).expect("permit");
        assert_eq!(
            journal
                .mark_remote_confirmed(&permit)
                .expect_err("must record dispatch first")
                .reason_code(),
            "GITHUB_PUBLICATION_TRANSITION_INVALID"
        );
        let mut conflict = request.clone();
        conflict.repository.head_commit = "1234567abcdef0".into();
        assert_eq!(
            journal
                .authorize(&policy, &conflict, 10)
                .expect_err("same idempotency key different intent")
                .reason_code(),
            "GITHUB_PUBLICATION_IDEMPOTENCY_CONFLICT"
        );
    }

    #[test]
    fn session_state_persists_the_journal_and_rejects_corruption_before_replay() {
        let policy = policy();
        let root =
            std::env::temp_dir().join(format!("catdesk-publication-state-{}", Uuid::new_v4()));
        let store = AutonomousStateStoreV1::open(&root).expect("state store");
        store
            .create_session(
                "publication-session",
                AutonomousQueueV1 {
                    schema_version: 1,
                    tasks: Vec::new(),
                },
            )
            .expect("session");
        let request = request(&policy, GithubPublicationOperationV1::PushFeatureBranch);
        let mut snapshot = store.load_session("publication-session").expect("load");
        snapshot
            .github_publication_journal
            .authorize(&policy, &request, 10)
            .expect("journal authorization");
        store.save_session(&snapshot).expect("atomic state save");
        let mut reloaded = store
            .load_session("publication-session")
            .expect("restart load");
        assert_eq!(
            reloaded
                .github_publication_journal
                .authorize(&policy, &request, 10)
                .expect_err("prepared journal cannot be replayed")
                .reason_code(),
            "GITHUB_PUBLICATION_RECOVERY_REQUIRED"
        );
        let mut corrupt = reloaded;
        corrupt.github_publication_journal.schema_version = 99;
        assert!(store.save_session(&corrupt).is_err());
    }

    #[test]
    fn reconstruction_manifest_rejects_the_stable_authority_pointer_from_its_hash_graph() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "entries": [{
                "path": CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH,
                "sha256": "a".repeat(64),
                "byteLength": 1,
                "classification": "INCLUDE_RECONSTRUCTION",
                "rationale": "must remain supplemental",
            }],
        });
        assert_eq!(
            parse_t0407_reconstruction_manifest(
                &serde_json::to_vec(&manifest).expect("manifest json")
            )
            .expect_err("stable authority pointer must stay outside the manifest hash graph"),
            FrozenPublicationManifestErrorV1::Classification
        );
    }

    #[test]
    fn legacy_prepared_publication_state_defaults_to_the_r12_manifest_path() {
        let legacy = serde_json::json!({
            "schemaVersion": 1,
            "idempotencyKey": "approval-1",
            "intentHash": "fnv1a64:0123456789abcdef",
            "reviewRecordId": "review-1",
            "reviewDigest": "a".repeat(64),
            "manifestSha256": "b".repeat(64),
            "manifestIncludeCount": 1,
            "archivalExclusionCount": 12,
            "runtimeGeneratedExclusionCount": 1,
            "supplementalPaths": [
                T0407_R12_DESCRIPTOR_PATH,
                T0407_R12_REVIEW_PATH
            ],
            "evidenceFingerprint": "c".repeat(64),
            "expiresAtUnix": 20,
            "repository": {
                "origin": "https://github.example.invalid/catdesk.git",
                "currentBranch": "orchestrator/chatgpt-codex-autonomous-loop",
                "headCommit": "abcdef0123456789"
            },
            "gitExecutableFingerprint": "d".repeat(64),
            "gitSelectedSlot": 0,
            "gitSelectedIdentity": "C:/trusted/git.exe"
        });
        let prepared: GithubPublicationPreparedV1 =
            serde_json::from_value(legacy).expect("legacy prepared publication");
        assert_eq!(prepared.manifest_path, T0407_R12_MANIFEST_PATH);
    }

    #[test]
    fn current_publication_authority_pointer_is_closed_and_fail_closed_when_unbound() {
        let unbound = serde_json::json!({
            "schemaVersion": 1,
            "purpose": CURRENT_GITHUB_PUBLICATION_AUTHORITY_PURPOSE,
            "projectId": "catdesk",
            "state": "UNBOUND",
        });
        assert_eq!(
            parse_current_github_publication_authority(
                &serde_json::to_vec(&unbound).expect("unbound json")
            )
            .expect("valid unbound pointer"),
            None
        );

        let mut overbroad = unbound.clone();
        overbroad["descriptorPath"] =
            serde_json::json!("docs/orchestrator/T-0407_R15_GITHUB_PUBLICATION_DESCRIPTOR.json");
        assert!(
            parse_current_github_publication_authority(
                &serde_json::to_vec(&overbroad).expect("overbroad json")
            )
            .is_err(),
            "UNBOUND authority must not carry a latent descriptor"
        );
    }

    #[test]
    fn current_publication_authority_pointer_binds_one_safe_descriptor_hash() {
        let descriptor_path = "docs/orchestrator/T-0407_R15_GITHUB_PUBLICATION_DESCRIPTOR.json";
        let bound = serde_json::json!({
            "schemaVersion": 1,
            "purpose": CURRENT_GITHUB_PUBLICATION_AUTHORITY_PURPOSE,
            "projectId": "catdesk",
            "state": "BOUND",
            "descriptorPath": descriptor_path,
            "descriptorSha256": "a".repeat(64),
        });
        let parsed = parse_current_github_publication_authority(
            &serde_json::to_vec(&bound).expect("bound json"),
        )
        .expect("valid bound pointer")
        .expect("bound pointer");
        assert_eq!(parsed.descriptor_path, descriptor_path);
        assert_eq!(parsed.descriptor_sha256, "a".repeat(64));

        for (field, value) in [
            ("descriptorPath", serde_json::json!("../escape.json")),
            (
                "descriptorPath",
                serde_json::json!("docs/orchestrator/not-a-descriptor.json"),
            ),
            ("descriptorSha256", serde_json::json!("abc")),
        ] {
            let mut tampered = bound.clone();
            tampered[field] = value;
            assert!(
                parse_current_github_publication_authority(
                    &serde_json::to_vec(&tampered).expect("tampered pointer json")
                )
                .is_err(),
                "pointer field {field} must remain closed"
            );
        }
    }

    #[test]
    fn generic_publication_descriptor_supports_a_reviewed_future_snapshot_without_code_rollover() {
        let descriptor_path = "docs/orchestrator/T-0407_R15_GITHUB_PUBLICATION_DESCRIPTOR.json";
        let review_path = "docs/orchestrator/review_bundles/T-0407_R15_GITHUB_RECOVERY_SNAPSHOT.md";
        let manifest_path = "docs/orchestrator/T-0407_R15_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json";
        let descriptor = serde_json::json!({
            "schemaVersion": 1,
            "purpose": T0407_R12_DESCRIPTOR_PURPOSE,
            "projectId": "catdesk",
            "manifestPath": manifest_path,
            "manifestSha256": "b".repeat(64),
            "includeCount": 1,
            "archivalExclusionCount": 12,
            "runtimeGeneratedExclusionCount": 1,
            "gitIdentity": {
                "originSha256": "c".repeat(64),
                "currentBranch": "orchestrator/chatgpt-codex-autonomous-loop",
                "headCommit": "abcdef0123456789"
            },
            "supplementalReviewedArtifactPaths": [
                descriptor_path,
                review_path,
                CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH,
            ],
        });
        let parsed = parse_github_publication_descriptor(
            &serde_json::to_vec(&descriptor).expect("future descriptor"),
            descriptor_path,
        )
        .expect("future reviewed descriptor");
        assert_eq!(parsed.manifest_path, manifest_path);
        assert_eq!(parsed.supplemental_paths.len(), 3);
        assert_eq!(
            parsed.supplemental_paths[2],
            CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH
        );

        let mut wrong_descriptor = descriptor.clone();
        wrong_descriptor["supplementalReviewedArtifactPaths"][0] =
            serde_json::json!("docs/orchestrator/T-0407_R14_GITHUB_PUBLICATION_DESCRIPTOR.json");
        assert!(
            parse_github_publication_descriptor(
                &serde_json::to_vec(&wrong_descriptor).expect("wrong descriptor json"),
                descriptor_path,
            )
            .is_err(),
            "descriptor must self-bind its reviewed path"
        );

        let mut missing_pointer = descriptor;
        missing_pointer["supplementalReviewedArtifactPaths"] =
            serde_json::json!([descriptor_path, review_path,]);
        let parsed_without_pointer = parse_github_publication_descriptor(
            &serde_json::to_vec(&missing_pointer).expect("legacy descriptor json"),
            descriptor_path,
        )
        .expect("generic parser may read legacy two-supplement descriptors");
        assert_eq!(parsed_without_pointer.supplemental_paths.len(), 2);
    }

    #[test]
    fn r12_reconstruction_manifest_retains_exact_entry_provenance_and_classifications() {
        let bytes = include_bytes!(
            "../../docs/orchestrator/T-0407_R12_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json"
        );
        let parsed = parse_t0407_reconstruction_manifest(bytes).expect("reviewed R12 snapshot");
        assert_eq!(parsed.includes.len(), 949);
        assert!(parsed.includes.iter().all(|entry| entry.sha256.len() == 64));
        assert_eq!(parsed.archival_binary_exclusions, 12);
        assert_eq!(parsed.runtime_generated_exclusions, 1);
    }

    #[test]
    fn r12_descriptor_is_closed_to_the_exact_manifest_and_supplements() {
        let descriptor = serde_json::json!({
            "schemaVersion": 1,
            "purpose": T0407_R12_DESCRIPTOR_PURPOSE,
            "projectId": "catdesk",
            "manifestPath": T0407_R12_MANIFEST_PATH,
            "manifestSha256": "a".repeat(64),
            "includeCount": 1,
            "archivalExclusionCount": 12,
            "runtimeGeneratedExclusionCount": 1,
            "supplementalReviewedArtifactPaths": [
                T0407_R12_DESCRIPTOR_PATH,
                T0407_R12_REVIEW_PATH,
            ],
        });
        let bytes = serde_json::to_vec(&descriptor).expect("descriptor json");
        assert!(parse_t0407_r12_publication_descriptor(&bytes).is_ok());
        for (field, value) in [
            ("manifestPath", serde_json::json!("docs/other.json")),
            ("archivalExclusionCount", serde_json::json!(11)),
            (
                "supplementalReviewedArtifactPaths",
                serde_json::json!([T0407_R12_REVIEW_PATH]),
            ),
        ] {
            let mut tampered = descriptor.clone();
            tampered[field] = value;
            assert!(
                parse_t0407_r12_publication_descriptor(
                    &serde_json::to_vec(&tampered).expect("tampered json")
                )
                .is_err(),
                "descriptor field {field} must be closed"
            );
        }
    }

    #[test]
    fn r12_authority_rejects_stale_r10_and_r7_descriptors() {
        let stale_r10 =
            include_bytes!("../../docs/orchestrator/T-0407_R10_GITHUB_PUBLICATION_DESCRIPTOR.json");
        let stale_r7 =
            include_bytes!("../../docs/orchestrator/T-0407_R7_GITHUB_PUBLICATION_DESCRIPTOR.json");
        assert!(parse_t0407_r12_publication_descriptor(stale_r10).is_err());
        assert!(parse_t0407_r12_publication_descriptor(stale_r7).is_err());
    }

    #[test]
    fn r12_authority_rejects_stale_r10_and_r7_manifest_paths() {
        for stale_manifest_path in [
            "docs/orchestrator/T-0407_R10_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json",
            "docs/orchestrator/T-0407_R7_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json",
        ] {
            let descriptor = serde_json::json!({
                "schemaVersion": 1,
                "purpose": T0407_R12_DESCRIPTOR_PURPOSE,
                "projectId": "catdesk",
                "manifestPath": stale_manifest_path,
                "manifestSha256": "a".repeat(64),
                "includeCount": 949,
                "archivalExclusionCount": 12,
                "runtimeGeneratedExclusionCount": 1,
                "supplementalReviewedArtifactPaths": [
                    T0407_R12_DESCRIPTOR_PATH,
                    T0407_R12_REVIEW_PATH,
                ],
            });
            assert!(
                parse_t0407_r12_publication_descriptor(
                    &serde_json::to_vec(&descriptor).expect("stale descriptor json")
                )
                .is_err(),
                "stale manifest authority must be refused"
            );
        }
    }

    #[test]
    fn r12_descriptor_and_manifest_define_the_exact_952_path_publication_set() {
        let descriptor = parse_t0407_r12_publication_descriptor(include_bytes!(
            "../../docs/orchestrator/T-0407_R12_GITHUB_PUBLICATION_DESCRIPTOR.json"
        ))
        .expect("reviewed R12 descriptor");
        let manifest = parse_t0407_reconstruction_manifest(include_bytes!(
            "../../docs/orchestrator/T-0407_R12_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json"
        ))
        .expect("reviewed R12 manifest");
        assert_eq!(descriptor.include_count, manifest.includes.len());
        let mut paths = manifest
            .includes
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        paths.push(descriptor.manifest_path);
        paths.extend(descriptor.supplemental_paths);
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 952);
        assert!(paths.contains(&T0407_R12_MANIFEST_PATH.into()));
        assert!(paths.contains(&T0407_R12_DESCRIPTOR_PATH.into()));
        assert!(paths.contains(&T0407_R12_REVIEW_PATH.into()));
    }

    #[test]
    fn manifest_paths_reject_escapes_and_generic_staging_shapes() {
        for path in [
            "",
            "/absolute",
            "../escape",
            "src\\backslash.rs",
            "src//empty.rs",
        ] {
            assert!(!valid_manifest_path(path), "unsafe path accepted: {path}");
        }
        assert!(valid_manifest_path("src/delegated/github_publication.rs"));
    }

    #[test]
    fn source_has_no_remote_executor_or_generic_git_authority() {
        let source = include_str!("github_publication.rs");
        let forbidden = [
            ["Command", "::new"].concat(),
            ["git", " push"].concat(),
            ["gh", " pr"].concat(),
            ["req", "west"].concat(),
            ["Tcp", "Stream"].concat(),
        ];
        for forbidden in forbidden {
            assert!(
                !source.contains(&forbidden),
                "forbidden executor authority: {forbidden}"
            );
        }
        assert!(source.contains("GithubPublicationOperationV1"));
        assert!(source.contains("GITHUB_PUBLICATION_RECOVERY_REQUIRED"));
    }
}
