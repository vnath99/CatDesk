//! Versioned autonomous-development contract and fail-closed policy checks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::contracts::stable_hash;

pub const AUTONOMOUS_DEVELOPMENT_CONTRACT_SCHEMA_VERSION: u32 = 1;

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
    pub autonomy_lease: AutonomyLeaseV1,
    pub hard_stop_conditions: Vec<AutonomousHardStopV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutonomousCommandProfileV1 {
    CargoFmt,
    CargoClippy,
    CargoTest,
    CargoBuildRelease,
    GitStatus,
    GitDiff,
    ApprovedProjectTests,
}

impl AutonomousCommandProfileV1 {
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
            Self::GitStatus => &["git", "status", "--short"],
            Self::GitDiff => &["git", "diff", "--check"],
            Self::ApprovedProjectTests => &[],
        }
    }
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
        if self.mode != "chatgpt_web_codex_autonomous"
            || self.objective.trim().is_empty()
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
        Ok(())
    }

    pub fn decision_hash(&self) -> Result<String, ContractPolicyError> {
        stable_hash(self).map_err(ContractPolicyError::Validation)
    }
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
                primary_model: "default".into(),
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
