//! Versioned autonomous-development contract and fail-closed policy checks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::contracts::stable_hash;

pub const AUTONOMOUS_DEVELOPMENT_CONTRACT_SCHEMA_VERSION: u32 = 1;
pub const AUTONOMOUS_RUNTIME_CAPABILITY_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousDevelopmentContractV1 {
    pub schema_version: u32,
    pub contract_id: String,
    pub task_id: String,
    pub project_id: String,
    pub mode: String,
    pub objective: String,
    pub workspace: PathBuf,
    pub base_branch: String,
    pub feature_branch: String,
    pub base_commit: String,
    pub expected_origin: String,
    pub ordered_steps: Vec<String>,
    pub allowed_paths: Vec<PathBuf>,
    pub forbidden_paths: Vec<PathBuf>,
    pub allowed_command_profiles: Vec<AutonomousCommandProfileV1>,
    pub git_policy: AutonomousGitPolicyV1,
    pub provider_policy: AutonomousProviderPolicyV1,
    pub verification_policy: AutonomousVerificationPolicyV1,
    pub rate_limit_policy: AutonomousRateLimitPolicyV1,
    pub autonomy_lease: AutonomyLeaseV1,
    pub hard_stop_conditions: Vec<AutonomousHardStopV1>,
    /// Exact task-attributable outputs for the historical single-task form.
    /// Omission preserves the serialized/hash-compatible legacy contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completion_artifact_ids: Vec<String>,
    /// Optional approved execution DAG. Empty preserves the historical
    /// one-task contract bytes and behavior.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_graph: Vec<AutonomousTaskSpecV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousTaskSpecV1 {
    pub task_id: String,
    pub priority: u32,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_gate: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completion_artifact_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutonomousCommandProfileV1 {
    CargoFmt,
    CargoClippy,
    CargoTest,
    CargoBuildRelease,
    CargoBuildReleaseIsolated,
    GitStatus,
    GitDiff,
    ApprovedProjectTests,
}

impl AutonomousCommandProfileV1 {
    /// The one authoritative command-profile catalog. Capability discovery and
    /// policy matching both derive from this type rather than a second list.
    pub const fn catalog() -> &'static [Self] {
        &[
            Self::CargoFmt,
            Self::CargoClippy,
            Self::CargoTest,
            Self::CargoBuildRelease,
            Self::CargoBuildReleaseIsolated,
            Self::GitStatus,
            Self::GitDiff,
            Self::ApprovedProjectTests,
        ]
    }

    pub fn exact_argv(&self) -> &'static [&'static str] {
        match self {
            Self::CargoFmt => &["cargo", "fmt", "--check"],
            Self::CargoClippy => &[
                "cargo",
                "clippy",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
            Self::CargoTest => &["cargo", "test"],
            Self::CargoBuildRelease => &["cargo", "build", "--release"],
            // This workspace-relative directory is product-defined verification
            // output only. It is not contract input or deployment authority.
            Self::CargoBuildReleaseIsolated => &[
                "cargo",
                "build",
                "--release",
                "--locked",
                "--target-dir",
                ".catdesk/verification-targets/autonomy-release",
            ],
            Self::GitStatus => &["git", "status", "--short"],
            Self::GitDiff => &["git", "diff", "--check"],
            Self::ApprovedProjectTests => &[
                "powershell",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "scripts/test-start-catdesk-stack.ps1",
            ],
        }
    }
}

/// Fixed, bounded, non-secret runtime capability data. It is derived directly
/// from the command-profile catalog and grants no contract/session authority.
pub fn runtime_capability_manifest() -> Value {
    let command_profiles = AutonomousCommandProfileV1::catalog()
        .iter()
        .map(|profile| {
            serde_json::to_value(profile)
                .expect("AutonomousCommandProfileV1 serialization is infallible")
        })
        .collect::<Vec<_>>();
    json!({
        "schemaVersion": AUTONOMOUS_RUNTIME_CAPABILITY_SCHEMA_VERSION,
        "product": "CatDesk",
        "contractSchemaVersion": AUTONOMOUS_DEVELOPMENT_CONTRACT_SCHEMA_VERSION,
        "commandProfiles": command_profiles,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousGitPolicyV1 {
    pub create_branch: bool,
    pub create_worktree: bool,
    pub local_commits: bool,
    pub push_feature_branch: bool,
    pub open_pull_request: bool,
    pub merge: bool,
    pub force_push: bool,
    pub modify_main: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutonomousGitActionV1 {
    CreateBranch,
    CreateWorktree,
    LocalCommit,
    PushFeatureBranch,
    OpenPullRequest,
    Merge,
    ForcePush,
    ModifyMain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousProviderPolicyV1 {
    pub primary_provider: String,
    pub primary_model: String,
    pub routine_provider: String,
    pub routine_model: String,
    pub allow_paid_fallback: bool,
    pub allow_cloud_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousVerificationPolicyV1 {
    pub profile: String,
    pub required_commands: Vec<AutonomousCommandProfileV1>,
    pub max_repair_cycles: u32,
    pub require_authoritative_diff: bool,
    pub require_final_review: bool,
}

/// Bounded, persisted behavior for a provider capacity pause. This is a
/// control-plane delay, never a provider fallback or a new worker session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousRateLimitPolicyV1 {
    pub automatic_pause: bool,
    pub automatic_resume: bool,
    pub initial_backoff_seconds: u64,
    pub maximum_backoff_seconds: u64,
    pub maximum_rate_limited_seconds: u64,
    pub honor_provider_retry_after: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomyLeaseV1 {
    pub start_approval_required: bool,
    pub renewal_allowed: bool,
    pub issued_at_unix: u64,
    pub expires_at_unix: u64,
    pub maximum_total_elapsed_seconds: u64,
    pub maximum_provider_turns: u32,
    pub maximum_tool_calls: u32,
    pub maximum_consecutive_failures: u32,
    pub maximum_repair_cycles: u32,
}

impl AutonomyLeaseV1 {
    pub fn valid_at(&self, now_unix: u64) -> bool {
        now_unix >= self.issued_at_unix && now_unix < self.expires_at_unix
    }

    pub fn renew(
        &mut self,
        now_unix: u64,
        new_expiry_unix: u64,
    ) -> Result<(), ContractPolicyError> {
        if !self.renewal_allowed || now_unix > self.expires_at_unix || new_expiry_unix <= now_unix {
            return Err(ContractPolicyError::Lease(
                "autonomy lease renewal is not permitted".into(),
            ));
        }
        self.expires_at_unix = new_expiry_unix;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutonomousHardStopV1 {
    SecretExposure,
    WorkspaceContainmentFailure,
    ProtectedBranchModification,
    ForcePushOrMergeAttempt,
    RollbackCheckpointUnavailable,
    LeaseExpired,
    CancellationRequested,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutonomousApprovalKindV1 {
    RunStart,
    GitPush,
    Merge,
    UnrestrictedShell,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousApprovalV1 {
    pub schema_version: u32,
    pub approval_id: String,
    pub contract_hash: String,
    pub kind: AutonomousApprovalKindV1,
    pub idempotency_key: String,
    pub expires_at_unix: u64,
    pub consumed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct AutonomousIdempotencyRegistryV1 {
    action_hashes: BTreeMap<String, String>,
}

impl AutonomousIdempotencyRegistryV1 {
    pub fn register(&mut self, key: &str, action_hash: &str) -> Result<bool, ContractPolicyError> {
        validate_slug(key, "idempotency key")?;
        if action_hash.trim().is_empty() {
            return Err(ContractPolicyError::Validation(
                "idempotency action hash is empty".into(),
            ));
        }
        match self.action_hashes.get(key) {
            Some(existing) if existing == action_hash => Ok(false),
            Some(_) => Err(ContractPolicyError::Idempotency(
                "idempotency key was already used for a different action".into(),
            )),
            None => {
                self.action_hashes.insert(key.into(), action_hash.into());
                Ok(true)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct AutonomousPolicyEngineV1 {
    contract: AutonomousDevelopmentContractV1,
    contract_hash: String,
    workspace: PathBuf,
    allowed_paths: Vec<PathBuf>,
    forbidden_paths: Vec<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ContractPolicyError {
    Validation(String),
    Denied(String),
    Lease(String),
    Approval(String),
    Idempotency(String),
}

impl AutonomousDevelopmentContractV1 {
    pub fn validate(&self) -> Result<(), ContractPolicyError> {
        if self.schema_version != AUTONOMOUS_DEVELOPMENT_CONTRACT_SCHEMA_VERSION {
            return Err(ContractPolicyError::Validation(
                "unsupported autonomous contract schema version".into(),
            ));
        }
        for (label, value) in [
            ("contract id", &self.contract_id),
            ("task id", &self.task_id),
            ("project id", &self.project_id),
        ] {
            validate_slug(value, label)?;
        }
        if !matches!(
            self.mode.as_str(),
            "chatgpt_web_codex_autonomous" | "chatgpt_web_qwen_autonomous"
        ) || self.objective.trim().is_empty()
            || self.objective.len() > 8_000
        {
            return Err(ContractPolicyError::Validation(
                "autonomous contract mode or objective is invalid".into(),
            ));
        }
        if !self.workspace.is_dir()
            || self.allowed_paths.is_empty()
            || self.ordered_steps.is_empty()
        {
            return Err(ContractPolicyError::Validation(
                "workspace, allowed paths, and ordered steps are required".into(),
            ));
        }
        if self.base_branch.trim().is_empty()
            || self.feature_branch.trim().is_empty()
            || self.base_commit.len() < 7
            || self.expected_origin.trim().is_empty()
        {
            return Err(ContractPolicyError::Validation(
                "autonomous Git identity is incomplete".into(),
            ));
        }
        if self.git_policy.force_push
            || self.git_policy.modify_main
            || self.provider_policy.allow_paid_fallback
            || self.provider_policy.allow_cloud_fallback
        {
            return Err(ContractPolicyError::Validation(
                "autonomous contract enables a prohibited operation or fallback".into(),
            ));
        }
        if self.provider_policy.primary_provider != "codex-cli"
            || self.provider_policy.routine_provider != "ollama"
        {
            return Err(ContractPolicyError::Validation(
                "autonomous contract provider selection is unsupported".into(),
            ));
        }
        if self.provider_policy.primary_model != "gpt-5.6-terra" {
            return Err(ContractPolicyError::Validation(
                "autonomous Codex implementation work requires gpt-5.6-terra".into(),
            ));
        }
        if self.autonomy_lease.expires_at_unix <= self.autonomy_lease.issued_at_unix
            || self.autonomy_lease.maximum_total_elapsed_seconds == 0
            || self.autonomy_lease.maximum_provider_turns == 0
            || self.autonomy_lease.maximum_tool_calls == 0
            || self.autonomy_lease.maximum_repair_cycles == 0
        {
            return Err(ContractPolicyError::Validation(
                "autonomy lease budgets are invalid".into(),
            ));
        }
        if !self.rate_limit_policy.automatic_pause
            || !self.rate_limit_policy.automatic_resume
            || self.rate_limit_policy.initial_backoff_seconds == 0
            || self.rate_limit_policy.maximum_backoff_seconds
                < self.rate_limit_policy.initial_backoff_seconds
            || self.rate_limit_policy.maximum_rate_limited_seconds
                < self.rate_limit_policy.initial_backoff_seconds
        {
            return Err(ContractPolicyError::Validation(
                "autonomous rate-limit policy is invalid".into(),
            ));
        }
        let profiles = self
            .allowed_command_profiles
            .iter()
            .collect::<BTreeSet<_>>();
        if profiles.len() != self.allowed_command_profiles.len()
            || self.allowed_command_profiles.is_empty()
        {
            return Err(ContractPolicyError::Validation(
                "autonomous command profiles must be non-empty and unique".into(),
            ));
        }
        validate_task_graph(&self.task_graph)?;
        validate_completion_artifact_ids(
            &self.workspace,
            &self.allowed_paths,
            &self.completion_artifact_ids,
        )?;
        for task in &self.task_graph {
            validate_completion_artifact_ids(
                &self.workspace,
                &self.allowed_paths,
                &task.completion_artifact_ids,
            )?;
        }
        Ok(())
    }

    pub fn starts_on_routine_provider(&self) -> bool {
        self.mode == "chatgpt_web_qwen_autonomous"
    }

    pub fn decision_hash(&self) -> Result<String, ContractPolicyError> {
        stable_hash(self).map_err(ContractPolicyError::Validation)
    }
}

/// Completion artifacts are authority-bearing exact workspace-relative file
/// IDs. Existing files are inspected only for safe file identity; absence is
/// intentionally allowed because a provider may be required to create one.
fn validate_completion_artifact_ids(
    workspace: &Path,
    allowed_paths: &[PathBuf],
    artifact_ids: &[String],
) -> Result<(), ContractPolicyError> {
    if artifact_ids.len() > 64 {
        return Err(ContractPolicyError::Validation(
            "completion artifact list is oversized".into(),
        ));
    }
    let mut normalized = BTreeSet::new();
    for artifact_id in artifact_ids {
        if artifact_id.is_empty()
            || artifact_id.len() > 512
            || artifact_id.contains('\0')
            || artifact_id.to_ascii_lowercase().contains("authorization:")
        {
            return Err(ContractPolicyError::Validation(
                "completion artifact id is unsafe".into(),
            ));
        }
        let relative = Path::new(artifact_id);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(ContractPolicyError::Validation(
                "completion artifact must be a normalized workspace-relative file path".into(),
            ));
        }
        let normalized_id = relative.to_string_lossy().replace('\\', "/");
        if !normalized.insert(normalized_id) {
            return Err(ContractPolicyError::Validation(
                "completion artifact ids must be unique after normalization".into(),
            ));
        }
        let candidate = workspace.join(relative);
        if !candidate.starts_with(workspace)
            || !allowed_paths
                .iter()
                .any(|allowed| candidate.starts_with(allowed))
        {
            return Err(ContractPolicyError::Validation(
                "completion artifact is outside approved paths".into(),
            ));
        }
        let mut cursor = workspace.to_path_buf();
        for component in relative.components() {
            let Component::Normal(part) = component else {
                unreachable!()
            };
            cursor.push(part);
            if cursor.exists() {
                let metadata = std::fs::symlink_metadata(&cursor).map_err(|_| {
                    ContractPolicyError::Validation(
                        "completion artifact metadata is unavailable".into(),
                    )
                })?;
                if has_unsafe_link_or_reparse(&metadata)
                    || (cursor == candidate && !metadata.file_type().is_file())
                {
                    return Err(ContractPolicyError::Validation(
                        "completion artifact has an unsafe filesystem identity".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn has_unsafe_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink() || {
        #[cfg(windows)]
        {
            metadata.file_attributes() & 0x400 != 0
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

fn validate_task_graph(graph: &[AutonomousTaskSpecV1]) -> Result<(), ContractPolicyError> {
    if graph.is_empty() {
        return Ok(());
    }
    if graph.len() > 128 {
        return Err(ContractPolicyError::Validation(
            "autonomous task graph is oversized".into(),
        ));
    }
    let ids = graph
        .iter()
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    if ids.len() != graph.len() {
        return Err(ContractPolicyError::Validation(
            "autonomous task graph ids must be unique".into(),
        ));
    }
    for task in graph {
        validate_slug(&task.task_id, "autonomous graph task id")?;
        if task.priority == 0
            || task.acceptance_criteria.is_empty()
            || task.acceptance_criteria.len() > 32
            || task.depends_on.len() > 64
        {
            return Err(ContractPolicyError::Validation(
                "autonomous task graph task shape is invalid".into(),
            ));
        }
        let dependencies = task
            .depends_on
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if dependencies.len() != task.depends_on.len()
            || dependencies.contains(task.task_id.as_str())
            || !dependencies
                .iter()
                .all(|dependency| ids.contains(dependency))
        {
            return Err(ContractPolicyError::Validation(
                "autonomous task graph dependencies are invalid".into(),
            ));
        }
        for dependency in &task.depends_on {
            validate_slug(dependency, "autonomous graph dependency")?;
        }
        if task
            .acceptance_criteria
            .iter()
            .any(|value| !safe_task_text(value))
            || task
                .planner_gate
                .as_deref()
                .is_some_and(|value| !safe_task_text(value))
        {
            return Err(ContractPolicyError::Validation(
                "autonomous task graph text is unsafe".into(),
            ));
        }
    }
    let mut remaining = graph
        .iter()
        .map(|task| {
            (
                task.task_id.as_str(),
                task.depends_on
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut complete = BTreeSet::new();
    loop {
        let ready = remaining
            .iter()
            .filter(|(_, dependencies)| {
                dependencies
                    .iter()
                    .all(|dependency| complete.contains(dependency))
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if ready.is_empty() {
            break;
        }
        for id in ready {
            remaining.remove(id);
            complete.insert(id);
        }
    }
    if !remaining.is_empty() {
        return Err(ContractPolicyError::Validation(
            "autonomous task graph must be acyclic".into(),
        ));
    }
    Ok(())
}

fn safe_task_text(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    !value.trim().is_empty()
        && value.len() <= 1024
        && !lowered.contains("api_key")
        && !lowered.contains("authorization:")
        && !value.contains("-----BEGIN")
}

impl AutonomousPolicyEngineV1 {
    pub fn new(contract: AutonomousDevelopmentContractV1) -> Result<Self, ContractPolicyError> {
        contract.validate()?;
        let workspace = canonical_directory(&contract.workspace, "workspace")?;
        let allowed_paths = contract
            .allowed_paths
            .iter()
            .map(|path| canonical_directory(path, "allowed path"))
            .collect::<Result<Vec<_>, _>>()?;
        let forbidden_paths = contract
            .forbidden_paths
            .iter()
            .map(|path| canonical_existing_path(path, "forbidden path"))
            .collect::<Result<Vec<_>, _>>()?;
        if !allowed_paths
            .iter()
            .all(|path| path.starts_with(&workspace))
        {
            return Err(ContractPolicyError::Validation(
                "allowed path escapes workspace".into(),
            ));
        }
        Ok(Self {
            contract_hash: contract.decision_hash()?,
            contract,
            workspace,
            allowed_paths,
            forbidden_paths,
        })
    }

    pub fn contract_hash(&self) -> &str {
        &self.contract_hash
    }
    pub fn contract(&self) -> &AutonomousDevelopmentContractV1 {
        &self.contract
    }

    pub fn permit_path(&self, target: &Path) -> Result<(), ContractPolicyError> {
        let target = canonical_existing_path(target, "operation target")?;
        self.permit_canonical_path(&target)
    }

    pub fn permit_new_path(&self, target: &Path) -> Result<(), ContractPolicyError> {
        let parent = target.parent().ok_or_else(|| {
            ContractPolicyError::Denied("new path has no parent directory".into())
        })?;
        if target.file_name().is_none() {
            return Err(ContractPolicyError::Denied(
                "new path must name a file or directory".into(),
            ));
        }
        let parent = canonical_directory(parent, "new operation target parent")?;
        self.permit_canonical_path(&parent)
    }

    fn permit_canonical_path(&self, target: &Path) -> Result<(), ContractPolicyError> {
        if !target.starts_with(&self.workspace)
            || self
                .forbidden_paths
                .iter()
                .any(|path| target.starts_with(path))
            || !self
                .allowed_paths
                .iter()
                .any(|path| target.starts_with(path))
        {
            return Err(ContractPolicyError::Denied(
                "path is outside the autonomous contract allowlist".into(),
            ));
        }
        Ok(())
    }

    pub fn permit_command(
        &self,
        argv: &[String],
    ) -> Result<AutonomousCommandProfileV1, ContractPolicyError> {
        self.contract
            .allowed_command_profiles
            .iter()
            .find(|profile| {
                !profile.exact_argv().is_empty()
                    && profile
                        .exact_argv()
                        .iter()
                        .map(|part| part.to_string())
                        .eq(argv.iter().cloned())
            })
            .cloned()
            .ok_or_else(|| {
                ContractPolicyError::Denied("command is not an exact approved profile".into())
            })
    }

    pub fn permit_git_action(
        &self,
        action: AutonomousGitActionV1,
    ) -> Result<(), ContractPolicyError> {
        let allowed = match action {
            AutonomousGitActionV1::CreateBranch => self.contract.git_policy.create_branch,
            AutonomousGitActionV1::CreateWorktree => self.contract.git_policy.create_worktree,
            AutonomousGitActionV1::LocalCommit => self.contract.git_policy.local_commits,
            AutonomousGitActionV1::PushFeatureBranch => {
                self.contract.git_policy.push_feature_branch
            }
            AutonomousGitActionV1::OpenPullRequest => self.contract.git_policy.open_pull_request,
            AutonomousGitActionV1::Merge => self.contract.git_policy.merge,
            AutonomousGitActionV1::ForcePush | AutonomousGitActionV1::ModifyMain => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ContractPolicyError::Denied(
                "Git action is not allowed by autonomous contract".into(),
            ))
        }
    }

    pub fn validate_approval(
        &self,
        approval: &AutonomousApprovalV1,
        now_unix: u64,
    ) -> Result<(), ContractPolicyError> {
        if approval.schema_version != AUTONOMOUS_DEVELOPMENT_CONTRACT_SCHEMA_VERSION
            || approval.contract_hash != self.contract_hash
            || approval.consumed
            || approval.expires_at_unix <= now_unix
        {
            return Err(ContractPolicyError::Approval(
                "autonomous approval is stale, consumed, or for a different contract".into(),
            ));
        }
        validate_slug(&approval.approval_id, "approval id")?;
        validate_slug(&approval.idempotency_key, "approval idempotency key")
    }

    pub fn requires_hard_stop(&self, condition: AutonomousHardStopV1) -> bool {
        self.contract.hard_stop_conditions.contains(&condition)
    }
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, ContractPolicyError> {
    if !path.is_dir() {
        return Err(ContractPolicyError::Validation(format!(
            "{label} must be an existing directory"
        )));
    }
    path.canonicalize()
        .map_err(|_| ContractPolicyError::Validation(format!("{label} could not be canonicalized")))
}

fn canonical_existing_path(path: &Path, label: &str) -> Result<PathBuf, ContractPolicyError> {
    if !path.exists() {
        return Err(ContractPolicyError::Validation(format!(
            "{label} must exist"
        )));
    }
    path.canonicalize()
        .map_err(|_| ContractPolicyError::Validation(format!("{label} could not be canonicalized")))
}

fn validate_slug(value: &str, label: &str) -> Result<(), ContractPolicyError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ContractPolicyError::Validation(format!(
            "{label} must be a conservative slug"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn contract() -> AutonomousDevelopmentContractV1 {
        let root = std::env::temp_dir().join(format!("catdesk-adc-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        AutonomousDevelopmentContractV1 {
            schema_version: 1,
            contract_id: "adc-1".into(),
            task_id: "t-28".into(),
            project_id: "catdesk".into(),
            mode: "chatgpt_web_codex_autonomous".into(),
            objective: "bounded test objective".into(),
            workspace: root.clone(),
            base_branch: "main".into(),
            feature_branch: "feature-adc".into(),
            base_commit: "1234567".into(),
            expected_origin: "https://example.invalid/catdesk.git".into(),
            ordered_steps: vec!["inspect".into()],
            allowed_paths: vec![root.join("src")],
            forbidden_paths: Vec::new(),
            allowed_command_profiles: vec![
                AutonomousCommandProfileV1::CargoTest,
                AutonomousCommandProfileV1::GitStatus,
            ],
            git_policy: AutonomousGitPolicyV1 {
                create_branch: true,
                create_worktree: true,
                local_commits: true,
                push_feature_branch: false,
                open_pull_request: false,
                merge: false,
                force_push: false,
                modify_main: false,
            },
            provider_policy: AutonomousProviderPolicyV1 {
                primary_provider: "codex-cli".into(),
                primary_model: "gpt-5.6-terra".into(),
                routine_provider: "ollama".into(),
                routine_model: "qwen3.6:35b-a3b".into(),
                allow_paid_fallback: false,
                allow_cloud_fallback: false,
            },
            verification_policy: AutonomousVerificationPolicyV1 {
                profile: "rust_full".into(),
                required_commands: vec![AutonomousCommandProfileV1::CargoTest],
                max_repair_cycles: 1,
                require_authoritative_diff: true,
                require_final_review: true,
            },
            rate_limit_policy: AutonomousRateLimitPolicyV1 {
                automatic_pause: true,
                automatic_resume: true,
                initial_backoff_seconds: 60,
                maximum_backoff_seconds: 300,
                maximum_rate_limited_seconds: 600,
                honor_provider_retry_after: true,
            },
            autonomy_lease: AutonomyLeaseV1 {
                start_approval_required: true,
                renewal_allowed: true,
                issued_at_unix: 10,
                expires_at_unix: 100,
                maximum_total_elapsed_seconds: 90,
                maximum_provider_turns: 2,
                maximum_tool_calls: 3,
                maximum_consecutive_failures: 1,
                maximum_repair_cycles: 1,
            },
            hard_stop_conditions: vec![AutonomousHardStopV1::CancellationRequested],
            completion_artifact_ids: Vec::new(),
            task_graph: Vec::new(),
        }
    }

    #[test]
    fn policy_is_deterministic_and_rejects_prohibited_fallbacks() {
        let contract = contract();
        assert_eq!(
            contract.decision_hash().expect("hash"),
            contract.decision_hash().expect("hash")
        );
        let mut prohibited = contract.clone();
        prohibited.provider_policy.allow_cloud_fallback = true;
        assert!(prohibited.validate().is_err());
    }

    #[test]
    fn provider_start_mode_is_explicit_backward_compatible_and_fail_closed() {
        let codex = contract();
        assert_eq!(codex.mode, "chatgpt_web_codex_autonomous");
        assert!(!codex.starts_on_routine_provider());
        codex
            .validate()
            .expect("historical Codex mode remains valid");

        let mut qwen = codex.clone();
        qwen.mode = "chatgpt_web_qwen_autonomous".into();
        qwen.validate()
            .expect("explicit local routine-provider mode is valid");
        assert!(qwen.starts_on_routine_provider());

        let mut unknown = codex;
        unknown.mode = "chatgpt_web_unknown_autonomous".into();
        assert!(unknown.validate().is_err());
    }

    #[test]
    fn empty_task_graph_preserves_legacy_contract_serialization() {
        let contract = contract();
        let json = serde_json::to_value(&contract).expect("serialize");
        assert!(json.get("taskGraph").is_none());
        let restored: AutonomousDevelopmentContractV1 =
            serde_json::from_value(json).expect("legacy deserialize");
        assert!(restored.task_graph.is_empty());
        assert_eq!(
            contract.decision_hash().expect("hash"),
            restored.decision_hash().expect("hash")
        );
    }

    #[test]
    fn completion_artifact_ids_are_optional_but_path_safe_and_exact() {
        let mut contract = contract();
        contract.completion_artifact_ids = vec!["src/output.txt".into()];
        assert!(contract.validate().is_ok());
        for invalid in [
            "/absolute.txt",
            "../escape.txt",
            "src/../output.txt",
            "outside.txt",
        ] {
            let mut invalid_contract = contract.clone();
            invalid_contract.completion_artifact_ids = vec![invalid.into()];
            assert!(invalid_contract.validate().is_err(), "{invalid}");
        }
        let mut duplicate = contract.clone();
        duplicate.completion_artifact_ids = vec!["src/output.txt".into(), "src\\output.txt".into()];
        assert!(duplicate.validate().is_err());
        let existing_directory = contract.workspace.join("src/existing-dir");
        std::fs::create_dir_all(&existing_directory).expect("directory");
        let mut unsafe_type = contract;
        unsafe_type.completion_artifact_ids = vec!["src/existing-dir".into()];
        assert!(unsafe_type.validate().is_err());
    }

    #[test]
    fn task_graph_rejects_duplicate_missing_self_and_cycles() {
        let mut contract = contract();
        contract.task_graph = vec![
            AutonomousTaskSpecV1 {
                task_id: "a".into(),
                priority: 1,
                depends_on: vec![],
                acceptance_criteria: vec!["bounded A".into()],
                planner_gate: None,
                completion_artifact_ids: Vec::new(),
            },
            AutonomousTaskSpecV1 {
                task_id: "b".into(),
                priority: 1,
                depends_on: vec!["a".into()],
                acceptance_criteria: vec!["bounded B".into()],
                planner_gate: Some("choose architecture".into()),
                completion_artifact_ids: Vec::new(),
            },
        ];
        assert!(contract.validate().is_ok());
        contract.task_graph[1].depends_on = vec!["missing".into()];
        assert!(contract.validate().is_err());
        contract.task_graph[1].depends_on = vec!["b".into()];
        assert!(contract.validate().is_err());
        contract.task_graph[1].depends_on = vec!["a".into()];
        contract.task_graph[0].depends_on = vec!["b".into()];
        assert!(contract.validate().is_err());
    }
    #[test]
    fn paths_commands_and_git_are_fail_closed() {
        let engine = AutonomousPolicyEngineV1::new(contract()).expect("engine");
        assert!(
            engine
                .permit_path(&engine.contract.workspace.join("src"))
                .is_ok()
        );
        assert!(engine.permit_path(&engine.contract.workspace).is_err());
        assert!(
            engine
                .permit_new_path(&engine.contract.workspace.join("src").join("new.rs"))
                .is_ok()
        );
        assert!(
            engine
                .permit_command(&["cargo".into(), "test".into()])
                .is_ok()
        );
        assert!(
            engine
                .permit_command(&["cargo".into(), "test".into(), "--all".into()])
                .is_err()
        );
        assert!(
            engine
                .permit_git_action(AutonomousGitActionV1::ForcePush)
                .is_err()
        );
    }

    #[test]
    fn isolated_release_profile_is_closed_world_and_legacy_release_is_unchanged() {
        let isolated = AutonomousCommandProfileV1::CargoBuildReleaseIsolated;
        let legacy = AutonomousCommandProfileV1::CargoBuildRelease;
        assert_eq!(
            legacy.exact_argv(),
            &["cargo", "build", "--release"],
            "CARGO_BUILD_RELEASE must remain backwards compatible"
        );
        assert_eq!(
            serde_json::to_string(&legacy).expect("serialize legacy profile"),
            "\"CARGO_BUILD_RELEASE\""
        );
        assert_eq!(
            isolated.exact_argv(),
            &[
                "cargo",
                "build",
                "--release",
                "--locked",
                "--target-dir",
                ".catdesk/verification-targets/autonomy-release",
            ]
        );
        assert_ne!(isolated.exact_argv(), legacy.exact_argv());
        let target = std::path::Path::new(isolated.exact_argv()[5]);
        assert!(target.is_relative());
        assert!(
            !target
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        );
        assert_ne!(target, std::path::Path::new("target/release"));
        assert_eq!(
            serde_json::to_string(&isolated).expect("serialize profile"),
            "\"CARGO_BUILD_RELEASE_ISOLATED\""
        );
        assert_eq!(
            serde_json::from_str::<AutonomousCommandProfileV1>("\"CARGO_BUILD_RELEASE_ISOLATED\"")
                .expect("deserialize isolated profile"),
            isolated
        );
        assert!(
            serde_json::from_str::<AutonomousCommandProfileV1>(
                "\"CARGO_BUILD_RELEASE_ISOLATED:/caller/path\""
            )
            .is_err()
        );

        let mut selected = contract();
        selected.allowed_command_profiles = vec![isolated.clone()];
        selected.verification_policy.required_commands = vec![isolated.clone()];
        let engine = AutonomousPolicyEngineV1::new(selected).expect("isolated policy");
        let exact = isolated
            .exact_argv()
            .iter()
            .map(|part| (*part).to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            engine.permit_command(&exact).expect("exact isolated argv"),
            isolated
        );
        let mut caller_selected = exact;
        caller_selected[5] = ".catdesk/verification-targets/caller-selected".into();
        assert!(engine.permit_command(&caller_selected).is_err());
    }

    #[test]
    fn runtime_capability_manifest_is_catalog_derived_bounded_and_stable() {
        let manifest = runtime_capability_manifest();
        assert_eq!(
            manifest.get("schemaVersion").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            manifest.get("product").and_then(Value::as_str),
            Some("CatDesk")
        );
        assert_eq!(
            manifest
                .get("contractSchemaVersion")
                .and_then(Value::as_u64),
            Some(AUTONOMOUS_DEVELOPMENT_CONTRACT_SCHEMA_VERSION as u64)
        );
        let profiles = manifest
            .get("commandProfiles")
            .and_then(Value::as_array)
            .expect("bounded profile catalog")
            .iter()
            .map(|value| value.as_str().expect("serialized profile").to_string())
            .collect::<Vec<_>>();
        let expected = AutonomousCommandProfileV1::catalog()
            .iter()
            .map(|profile| {
                serde_json::to_value(profile)
                    .expect("profile serialization")
                    .as_str()
                    .expect("profile name")
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(profiles, expected);
        assert_eq!(
            profiles,
            vec![
                "CARGO_FMT",
                "CARGO_CLIPPY",
                "CARGO_TEST",
                "CARGO_BUILD_RELEASE",
                "CARGO_BUILD_RELEASE_ISOLATED",
                "GIT_STATUS",
                "GIT_DIFF",
                "APPROVED_PROJECT_TESTS",
            ]
        );
        assert_eq!(
            profiles.iter().collect::<BTreeSet<_>>().len(),
            profiles.len(),
            "catalog must not contain duplicate advertised profiles"
        );
        assert!(
            serde_json::to_string(&manifest)
                .expect("manifest serialization")
                .len()
                <= 2_048
        );
    }
    #[test]
    fn approval_lease_and_idempotency_are_bound_to_contract() {
        let engine = AutonomousPolicyEngineV1::new(contract()).expect("engine");
        let approval = AutonomousApprovalV1 {
            schema_version: 1,
            approval_id: "approval-1".into(),
            contract_hash: engine.contract_hash().into(),
            kind: AutonomousApprovalKindV1::RunStart,
            idempotency_key: "start-1".into(),
            expires_at_unix: 50,
            consumed: false,
        };
        engine.validate_approval(&approval, 20).expect("approval");
        let mut registry = AutonomousIdempotencyRegistryV1::default();
        assert!(registry.register("start-1", "hash-a").expect("first"));
        assert!(!registry.register("start-1", "hash-a").expect("repeat"));
        assert!(registry.register("start-1", "hash-b").is_err());
        let mut lease = engine.contract.autonomy_lease.clone();
        assert!(lease.valid_at(20));
        lease.renew(20, 120).expect("renew");
        assert!(lease.valid_at(110));
    }
}
