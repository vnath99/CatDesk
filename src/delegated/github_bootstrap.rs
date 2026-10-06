//! Fixed-policy, fail-closed GitHub bootstrap control.
//!
//! This module is deliberately not a general Git or GitHub wrapper.  It owns
//! only the two explicitly approved local project mappings and emits only
//! bounded, non-secret evidence.  The supervisor exposes it through closed
//! cached-schema decision forms.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_WORKSPACE_LEN: usize = 4_096;
const MAX_COMMAND_OUTPUT: usize = 4_096;
const MAX_BRANCH_LEN: usize = 128;
const PREFLIGHT_TTL_SECONDS: u64 = 300;
const OWNER: &str = "vnath99";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GithubBootstrapPolicy {
    pub project_id: &'static str,
    pub workspace: &'static str,
    pub repository: &'static str,
    pub origin: &'static str,
}

const POLICIES: [GithubBootstrapPolicy; 2] = [
    GithubBootstrapPolicy {
        project_id: "BYOVD_DRIVER_PIPELINE",
        workspace: r"%USERPROFILE%\OneDrive\Desktop\Projects\BYOVD_DRIVER_PIPELINE",
        repository: "BYOVD_DRIVER_PIPELINE",
        origin: "https://github.com/vnath99/BYOVD_DRIVER_PIPELINE.git",
    },
    GithubBootstrapPolicy {
        project_id: "BUG_BOUNTY_RECON_PLATFORM",
        workspace: r"%USERPROFILE%\OneDrive\Desktop\Projects\BUG_BOUNTY_RECON_PLATFORM",
        repository: "BUG_BOUNTY_RECON_PLATFORM",
        origin: "https://github.com/vnath99/BUG_BOUNTY_RECON_PLATFORM.git",
    },
];

pub fn approved_policy(project_id: &str) -> Option<GithubBootstrapPolicy> {
    POLICIES
        .iter()
        .copied()
        .find(|policy| policy.project_id == project_id)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GithubBootstrapFailure {
    InvalidRequest,
    ProjectNotApproved,
    WorkspaceUnavailable,
    WorkspaceNotAbsolute,
    WorkspaceReparseOrSymlink,
    WorkspacePolicyMismatch,
    WorkspaceNotGitRoot,
    WorkspaceNestedGitRoot,
    OriginAlreadyPresent,
    OriginUnexpected,
    HeadUnavailable,
    HeadInvalid,
    BranchUnavailable,
    BranchInvalid,
    DirtyStatusInvalid,
    GitUnavailable,
    GitUnsafeExecutable,
    GitInsideWorkspace,
    GitSelectionDrift,
    GitCommandFailed,
    GitOutputInvalid,
    GhUnavailable,
    GhUnsafeExecutable,
    GhAmbiguous,
    GhReparseOrSymlink,
    GhInsideWorkspace,
    GhFingerprintDrift,
    GhCommandFailed,
    GhOutputInvalid,
    GithubAuthUnavailable,
    GithubAuthWrongAccount,
    GhProcessFailure,
    AccountGhProcessFailure,
    RepositoryGhProcessFailure,
    GhHttpAuthenticationFailed,
    GhRepoScopeUnproven,
    GhHttpEnvelopeInvalid,
    TargetRepositoryAlreadyExists,
    TargetRepositoryProbeFailed,
    TokenUnavailable,
    TokenExpired,
    TokenProjectMismatch,
    EvidenceDrift,
    TransactionConflict,
    TransactionPersistenceFailed,
    RepositoryCreateFailed,
    OriginAddFailed,
    PushFailed,
}

impl GithubBootstrapFailure {
    pub const fn reason_code(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "GITHUB_BOOTSTRAP_REQUEST_INVALID",
            Self::ProjectNotApproved => "GITHUB_BOOTSTRAP_PROJECT_NOT_APPROVED",
            Self::WorkspaceUnavailable => "GITHUB_BOOTSTRAP_WORKSPACE_UNAVAILABLE",
            Self::WorkspaceNotAbsolute => "GITHUB_BOOTSTRAP_WORKSPACE_NOT_ABSOLUTE",
            Self::WorkspaceReparseOrSymlink => "GITHUB_BOOTSTRAP_WORKSPACE_REPARSE_OR_SYMLINK",
            Self::WorkspacePolicyMismatch => "GITHUB_BOOTSTRAP_WORKSPACE_POLICY_MISMATCH",
            Self::WorkspaceNotGitRoot => "GITHUB_BOOTSTRAP_NOT_GIT_ROOT",
            Self::WorkspaceNestedGitRoot => "GITHUB_BOOTSTRAP_NESTED_GIT_ROOT",
            Self::OriginAlreadyPresent => "GITHUB_BOOTSTRAP_ORIGIN_ALREADY_PRESENT",
            Self::OriginUnexpected => "GITHUB_BOOTSTRAP_ORIGIN_UNEXPECTED",
            Self::HeadUnavailable => "GITHUB_BOOTSTRAP_HEAD_UNAVAILABLE",
            Self::HeadInvalid => "GITHUB_BOOTSTRAP_HEAD_INVALID",
            Self::BranchUnavailable => "GITHUB_BOOTSTRAP_BRANCH_UNAVAILABLE",
            Self::BranchInvalid => "GITHUB_BOOTSTRAP_BRANCH_INVALID",
            Self::DirtyStatusInvalid => "GITHUB_BOOTSTRAP_DIRTY_STATUS_INVALID",
            Self::GitUnavailable => "GITHUB_BOOTSTRAP_GIT_UNAVAILABLE",
            Self::GitUnsafeExecutable => "GITHUB_BOOTSTRAP_GIT_UNSAFE_EXECUTABLE",
            Self::GitInsideWorkspace => "GITHUB_BOOTSTRAP_GIT_INSIDE_WORKSPACE",
            Self::GitSelectionDrift => "GITHUB_BOOTSTRAP_GIT_SELECTION_DRIFT",
            Self::GitCommandFailed => "GITHUB_BOOTSTRAP_GIT_COMMAND_FAILED",
            Self::GitOutputInvalid => "GITHUB_BOOTSTRAP_GIT_OUTPUT_INVALID",
            Self::GhUnavailable => "GITHUB_BOOTSTRAP_GH_UNAVAILABLE",
            Self::GhUnsafeExecutable => "GITHUB_BOOTSTRAP_GH_UNSAFE_EXECUTABLE",
            Self::GhAmbiguous => "GITHUB_BOOTSTRAP_GH_AMBIGUOUS",
            Self::GhReparseOrSymlink => "GITHUB_BOOTSTRAP_GH_REPARSE_OR_SYMLINK",
            Self::GhInsideWorkspace => "GITHUB_BOOTSTRAP_GH_INSIDE_WORKSPACE",
            Self::GhFingerprintDrift => "GITHUB_BOOTSTRAP_GH_FINGERPRINT_DRIFT",
            Self::GhCommandFailed => "GITHUB_BOOTSTRAP_GH_COMMAND_FAILED",
            Self::GhOutputInvalid => "GITHUB_BOOTSTRAP_GH_OUTPUT_INVALID",
            Self::GithubAuthUnavailable => "GITHUB_BOOTSTRAP_AUTH_UNAVAILABLE",
            Self::GithubAuthWrongAccount => "GITHUB_BOOTSTRAP_AUTH_WRONG_ACCOUNT",
            Self::GhProcessFailure => "GITHUB_BOOTSTRAP_GH_PROCESS_FAILURE",
            Self::AccountGhProcessFailure => "GITHUB_BOOTSTRAP_ACCOUNT_GH_PROCESS_FAILURE",
            Self::RepositoryGhProcessFailure => "GITHUB_BOOTSTRAP_REPOSITORY_GH_PROCESS_FAILURE",
            Self::GhHttpAuthenticationFailed => "GITHUB_BOOTSTRAP_GH_HTTP_AUTH_FAILURE",
            Self::GhRepoScopeUnproven => "GITHUB_BOOTSTRAP_GH_REPO_SCOPE_UNPROVEN",
            Self::GhHttpEnvelopeInvalid => "GITHUB_BOOTSTRAP_GH_HTTP_ENVELOPE_INVALID",
            Self::TargetRepositoryAlreadyExists => "GITHUB_BOOTSTRAP_TARGET_REPOSITORY_EXISTS",
            Self::TargetRepositoryProbeFailed => "GITHUB_BOOTSTRAP_TARGET_REPOSITORY_PROBE_FAILED",
            Self::TokenUnavailable => "GITHUB_BOOTSTRAP_TOKEN_UNAVAILABLE",
            Self::TokenExpired => "GITHUB_BOOTSTRAP_TOKEN_EXPIRED",
            Self::TokenProjectMismatch => "GITHUB_BOOTSTRAP_TOKEN_PROJECT_MISMATCH",
            Self::EvidenceDrift => "GITHUB_BOOTSTRAP_EVIDENCE_DRIFT",
            Self::TransactionConflict => "GITHUB_BOOTSTRAP_TRANSACTION_CONFLICT",
            Self::TransactionPersistenceFailed => "GITHUB_BOOTSTRAP_TRANSACTION_PERSISTENCE_FAILED",
            Self::RepositoryCreateFailed => "GITHUB_BOOTSTRAP_REPOSITORY_CREATE_FAILED",
            Self::OriginAddFailed => "GITHUB_BOOTSTRAP_ORIGIN_ADD_FAILED",
            Self::PushFailed => "GITHUB_BOOTSTRAP_PUSH_FAILED",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommandResult {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait GithubBootstrapRunner {
    fn git(&self, workspace: &Path, args: &[&str])
    -> Result<CommandResult, GithubBootstrapFailure>;
    fn gh(&self, executable: &Path, args: &[&str])
    -> Result<CommandResult, GithubBootstrapFailure>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemGithubBootstrapRunner;

impl GithubBootstrapRunner for SystemGithubBootstrapRunner {
    fn git(
        &self,
        workspace: &Path,
        args: &[&str],
    ) -> Result<CommandResult, GithubBootstrapFailure> {
        let executable = resolve_trusted_tool(
            TrustedToolKind::Git,
            &default_trusted_git_candidates(),
            workspace,
        )?;
        let output = Command::new(executable.canonical)
            .arg("-C")
            .arg(workspace)
            .args(args)
            .output()
            .map_err(|_| GithubBootstrapFailure::GitUnavailable)?;
        if output.stdout.len() > MAX_COMMAND_OUTPUT || output.stderr.len() > MAX_COMMAND_OUTPUT {
            return Err(GithubBootstrapFailure::GitOutputInvalid);
        }
        Ok(CommandResult {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn gh(
        &self,
        executable: &Path,
        args: &[&str],
    ) -> Result<CommandResult, GithubBootstrapFailure> {
        let output = Command::new(executable)
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .args(args)
            .output()
            .map_err(|_| GithubBootstrapFailure::GhUnavailable)?;
        if output.stdout.len() > MAX_COMMAND_OUTPUT || output.stderr.len() > MAX_COMMAND_OUTPUT {
            return Err(GithubBootstrapFailure::GhOutputInvalid);
        }
        Ok(CommandResult {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GithubBootstrapPreflightV1 {
    pub schema_version: u32,
    pub confirmation_token: String,
    pub expires_at_unix: u64,
    pub project_id: String,
    pub workspace: String,
    pub workspace_identity: String,
    pub head: String,
    #[serde(default)]
    pub head_state: String,
    pub branch: String,
    pub working_tree_dirty: bool,
    pub working_tree_change_count: u32,
    pub gh_executable_fingerprint: String,
    #[serde(default)]
    pub gh_selected_slot: u8,
    #[serde(default)]
    pub gh_selected_identity: String,
    pub git_executable_fingerprint: String,
    #[serde(default)]
    pub git_selected_slot: u8,
    #[serde(default)]
    pub git_selected_identity: String,
    pub policy_origin: String,
    pub evidence_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GitHeadState {
    head: Option<String>,
    unborn: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct GithubBootstrapTransactionV1 {
    schema_version: u32,
    confirmation_token: String,
    project_id: String,
    evidence_fingerprint: String,
    stages: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubBootstrapOutcome {
    pub project_id: String,
    pub owner: &'static str,
    pub repository: &'static str,
    pub origin: &'static str,
    pub branch: String,
    pub head: String,
    pub working_tree_dirty: bool,
    pub working_tree_change_count: u32,
    pub stages: Vec<String>,
}

pub struct GithubBootstrapStore {
    root: PathBuf,
    trusted_git_candidates: Vec<PathBuf>,
    trusted_gh_candidates: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrustedToolKind {
    Git,
    Gh,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrustedToolIdentity {
    slot: u8,
    canonical: PathBuf,
    fingerprint: String,
}

/// Shared closed-world Git executable identity. Callers receive only the
/// canonical trusted program selected from the reviewed fixed slots and its
/// content fingerprint; PATH is deliberately not consulted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrustedGitIdentityV1 {
    pub canonical: PathBuf,
    pub fingerprint: String,
    pub slot: u8,
}

pub(crate) fn resolve_trusted_git_identity(
    workspace: &Path,
) -> Result<TrustedGitIdentityV1, GithubBootstrapFailure> {
    let identity = resolve_trusted_tool(
        TrustedToolKind::Git,
        &default_trusted_git_candidates(),
        workspace,
    )?;
    Ok(TrustedGitIdentityV1 {
        canonical: identity.canonical,
        fingerprint: identity.fingerprint,
        slot: identity.slot,
    })
}

impl GithubBootstrapStore {
    pub fn open(root: PathBuf) -> Self {
        Self {
            root,
            trusted_git_candidates: default_trusted_git_candidates(),
            trusted_gh_candidates: default_trusted_gh_candidates(),
        }
    }

    #[cfg(test)]
    fn with_candidates(
        root: PathBuf,
        git_candidates: Vec<PathBuf>,
        gh_candidates: Vec<PathBuf>,
    ) -> Self {
        Self {
            root,
            trusted_git_candidates: git_candidates,
            trusted_gh_candidates: gh_candidates,
        }
    }

    pub fn preflight(
        &self,
        project_id: &str,
        requested_workspace: &str,
        now_unix: u64,
        runner: &impl GithubBootstrapRunner,
    ) -> Result<GithubBootstrapPreflightV1, GithubBootstrapFailure> {
        let policy =
            approved_policy(project_id).ok_or(GithubBootstrapFailure::ProjectNotApproved)?;
        self.preflight_with_policy(policy, requested_workspace, now_unix, runner)
    }

    #[cfg(test)]
    fn preflight_with_test_policy(
        &self,
        policy: GithubBootstrapPolicy,
        requested_workspace: &str,
        now_unix: u64,
        runner: &impl GithubBootstrapRunner,
    ) -> Result<GithubBootstrapPreflightV1, GithubBootstrapFailure> {
        self.preflight_with_policy(policy, requested_workspace, now_unix, runner)
    }

    fn preflight_with_policy(
        &self,
        policy: GithubBootstrapPolicy,
        requested_workspace: &str,
        now_unix: u64,
        runner: &impl GithubBootstrapRunner,
    ) -> Result<GithubBootstrapPreflightV1, GithubBootstrapFailure> {
        let (workspace, workspace_identity) =
            validated_workspace(policy, requested_workspace, runner, None)?;
        let git = resolve_trusted_tool(
            TrustedToolKind::Git,
            &self.trusted_git_candidates,
            &workspace,
        )?;
        let gh = self.resolve_trusted_gh(&workspace)?;
        let (head_state, branch, dirty, dirty_count) = git_state(runner, &workspace)?;
        prove_repository_state(runner, &gh.canonical, policy, RepositoryState::Absent)?;
        let token = Uuid::new_v4().simple().to_string();
        let mut preflight = GithubBootstrapPreflightV1 {
            schema_version: 1,
            confirmation_token: token,
            expires_at_unix: now_unix.saturating_add(PREFLIGHT_TTL_SECONDS),
            project_id: policy.project_id.into(),
            workspace: workspace.to_string_lossy().into_owned(),
            workspace_identity,
            head: head_state.head.clone().unwrap_or_default(),
            head_state: if head_state.unborn {
                "UNBORN_HEAD".into()
            } else {
                "COMMITTED_HEAD".into()
            },
            branch,
            working_tree_dirty: dirty,
            working_tree_change_count: dirty_count,
            gh_executable_fingerprint: gh.fingerprint.clone(),
            gh_selected_slot: gh.slot,
            gh_selected_identity: gh.canonical.to_string_lossy().into_owned(),
            git_executable_fingerprint: git.fingerprint.clone(),
            git_selected_slot: git.slot,
            git_selected_identity: git.canonical.to_string_lossy().into_owned(),
            policy_origin: policy.origin.into(),
            evidence_fingerprint: String::new(),
        };
        preflight.evidence_fingerprint = preflight_fingerprint(&preflight);
        self.write_preflight(&preflight)?;
        Ok(preflight)
    }

    pub fn confirm(
        &self,
        project_id: &str,
        confirmation_token: &str,
        now_unix: u64,
        runner: &impl GithubBootstrapRunner,
    ) -> Result<GithubBootstrapOutcome, GithubBootstrapFailure> {
        let policy =
            approved_policy(project_id).ok_or(GithubBootstrapFailure::ProjectNotApproved)?;
        self.confirm_with_policy(
            policy,
            project_id,
            confirmation_token,
            now_unix,
            runner,
            false,
        )
    }

    pub fn recover(
        &self,
        project_id: &str,
        confirmation_token: &str,
        now_unix: u64,
        runner: &impl GithubBootstrapRunner,
    ) -> Result<GithubBootstrapOutcome, GithubBootstrapFailure> {
        let policy =
            approved_policy(project_id).ok_or(GithubBootstrapFailure::ProjectNotApproved)?;
        self.confirm_with_policy(
            policy,
            project_id,
            confirmation_token,
            now_unix,
            runner,
            true,
        )
    }

    #[cfg(test)]
    fn confirm_with_test_policy(
        &self,
        policy: GithubBootstrapPolicy,
        project_id: &str,
        confirmation_token: &str,
        now_unix: u64,
        runner: &impl GithubBootstrapRunner,
    ) -> Result<GithubBootstrapOutcome, GithubBootstrapFailure> {
        self.confirm_with_policy(
            policy,
            project_id,
            confirmation_token,
            now_unix,
            runner,
            false,
        )
    }

    #[cfg(test)]
    fn recover_with_test_policy(
        &self,
        policy: GithubBootstrapPolicy,
        project_id: &str,
        confirmation_token: &str,
        now_unix: u64,
        runner: &impl GithubBootstrapRunner,
    ) -> Result<GithubBootstrapOutcome, GithubBootstrapFailure> {
        self.confirm_with_policy(
            policy,
            project_id,
            confirmation_token,
            now_unix,
            runner,
            true,
        )
    }

    fn confirm_with_policy(
        &self,
        policy: GithubBootstrapPolicy,
        project_id: &str,
        confirmation_token: &str,
        now_unix: u64,
        runner: &impl GithubBootstrapRunner,
        recovery: bool,
    ) -> Result<GithubBootstrapOutcome, GithubBootstrapFailure> {
        if confirmation_token.len() != 32
            || !confirmation_token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(GithubBootstrapFailure::TokenUnavailable);
        }
        let preflight = self.read_preflight(confirmation_token)?;
        if preflight.project_id != project_id {
            return Err(GithubBootstrapFailure::TokenProjectMismatch);
        }
        if preflight.evidence_fingerprint != preflight_fingerprint(&preflight) {
            return Err(GithubBootstrapFailure::EvidenceDrift);
        }
        let workspace_text = preflight.workspace.clone();
        let mut transaction = self.read_transaction(confirmation_token)?;
        if let Some(existing) = &transaction {
            if existing.project_id != project_id
                || existing.evidence_fingerprint != preflight.evidence_fingerprint
            {
                return Err(GithubBootstrapFailure::TransactionConflict);
            }
        }
        let started = transaction
            .as_ref()
            .is_some_and(|value| value.stages.contains("REPOSITORY_CREATED"));
        if now_unix > preflight.expires_at_unix && (!recovery || !started) {
            return Err(GithubBootstrapFailure::TokenExpired);
        }
        if recovery && !started {
            return Err(GithubBootstrapFailure::TransactionConflict);
        }
        let expected_existing_origin = transaction
            .as_ref()
            .filter(|value| value.stages.contains("ORIGIN_ADDED"))
            .map(|_| policy.origin);
        let (workspace, workspace_identity) =
            validated_workspace(policy, &workspace_text, runner, expected_existing_origin)?;
        let gh = self.resolve_trusted_gh(&workspace)?;
        if gh.slot != preflight.gh_selected_slot
            || gh.canonical.to_string_lossy() != preflight.gh_selected_identity
            || gh.fingerprint != preflight.gh_executable_fingerprint
        {
            return Err(GithubBootstrapFailure::GhFingerprintDrift);
        }
        let git = resolve_trusted_tool(
            TrustedToolKind::Git,
            &self.trusted_git_candidates,
            &workspace,
        )?;
        if git.slot != preflight.git_selected_slot
            || git.canonical.to_string_lossy() != preflight.git_selected_identity
            || git.fingerprint != preflight.git_executable_fingerprint
        {
            return Err(GithubBootstrapFailure::GitSelectionDrift);
        }
        let (head_state, branch, dirty, dirty_count) = git_state(runner, &workspace)?;
        let head = head_state.head.clone().unwrap_or_default();
        if workspace_identity != preflight.workspace_identity
            || head != preflight.head
            || (if head_state.unborn {
                "UNBORN_HEAD"
            } else {
                "COMMITTED_HEAD"
            }) != preflight.head_state
            || branch != preflight.branch
            || dirty != preflight.working_tree_dirty
            || dirty_count != preflight.working_tree_change_count
        {
            return Err(GithubBootstrapFailure::EvidenceDrift);
        }
        if transaction.is_none() {
            prove_repository_state(runner, &gh.canonical, policy, RepositoryState::Absent)?;
            let created = GithubBootstrapTransactionV1 {
                schema_version: 1,
                confirmation_token: confirmation_token.into(),
                project_id: project_id.into(),
                evidence_fingerprint: preflight.evidence_fingerprint.clone(),
                stages: BTreeSet::from(["JOURNALED".into()]),
            };
            self.write_transaction(&created)?;
            transaction = Some(created);
        }
        let mut transaction = transaction.expect("transaction was created");

        if transaction.stages.contains("REPOSITORY_CREATED") {
            prove_repository_state(runner, &gh.canonical, policy, RepositoryState::Exists)?;
        } else {
            prove_repository_state(runner, &gh.canonical, policy, RepositoryState::Absent)?;
        }

        if !transaction.stages.contains("REPOSITORY_CREATED") {
            let result = runner.gh(
                &gh.canonical,
                &[
                    "repo",
                    "create",
                    &format!("{OWNER}/{}", policy.repository),
                    "--private",
                ],
            )?;
            if !result.success {
                return Err(GithubBootstrapFailure::RepositoryCreateFailed);
            }
            transaction.stages.insert("REPOSITORY_CREATED".into());
            self.write_transaction(&transaction)?;
        }
        if !transaction.stages.contains("ORIGIN_ADDED") {
            let origin = runner.git(&workspace, &["config", "--get", "remote.origin.url"])?;
            if origin.success {
                return Err(GithubBootstrapFailure::OriginAlreadyPresent);
            }
            let result = runner.git(&workspace, &["remote", "add", "origin", policy.origin])?;
            if !result.success {
                return Err(GithubBootstrapFailure::OriginAddFailed);
            }
            transaction.stages.insert("ORIGIN_ADDED".into());
            self.write_transaction(&transaction)?;
        }
        if head_state.unborn {
            verify_post_mutation_authority(runner, &workspace, &gh.canonical, policy)?;
            if !transaction
                .stages
                .contains("PUSH_SKIPPED_NO_COMMITTED_HISTORY")
            {
                transaction
                    .stages
                    .insert("PUSH_SKIPPED_NO_COMMITTED_HISTORY".into());
                self.write_transaction(&transaction)?;
            }
        } else if !head_state.unborn && !transaction.stages.contains("PUSHED") {
            let origin = runner.git(&workspace, &["config", "--get", "remote.origin.url"])?;
            if !origin.success || bounded_text(&origin.stdout).as_deref() != Some(policy.origin) {
                return Err(GithubBootstrapFailure::OriginUnexpected);
            }
            let result = runner.git(&workspace, &["push", "-u", "origin", &branch])?;
            if !result.success {
                return Err(GithubBootstrapFailure::PushFailed);
            }
            transaction.stages.insert("PUSHED".into());
            self.write_transaction(&transaction)?;
        }
        Ok(GithubBootstrapOutcome {
            project_id: project_id.into(),
            owner: OWNER,
            repository: policy.repository,
            origin: policy.origin,
            branch,
            head,
            working_tree_dirty: dirty,
            working_tree_change_count: dirty_count,
            stages: transaction.stages.into_iter().collect(),
        })
    }

    fn resolve_trusted_gh(
        &self,
        workspace: &Path,
    ) -> Result<TrustedToolIdentity, GithubBootstrapFailure> {
        resolve_trusted_tool(TrustedToolKind::Gh, &self.trusted_gh_candidates, workspace)
    }

    fn preflight_path(&self, token: &str) -> PathBuf {
        self.root.join("preflights").join(format!("{token}.json"))
    }
    fn transaction_path(&self, token: &str) -> PathBuf {
        self.root.join("transactions").join(format!("{token}.json"))
    }
    fn write_preflight(
        &self,
        preflight: &GithubBootstrapPreflightV1,
    ) -> Result<(), GithubBootstrapFailure> {
        self.write_json(
            &self.preflight_path(&preflight.confirmation_token),
            preflight,
        )
    }
    fn read_preflight(
        &self,
        token: &str,
    ) -> Result<GithubBootstrapPreflightV1, GithubBootstrapFailure> {
        self.read_json(&self.preflight_path(token))
            .ok_or(GithubBootstrapFailure::TokenUnavailable)
    }
    fn read_transaction(
        &self,
        token: &str,
    ) -> Result<Option<GithubBootstrapTransactionV1>, GithubBootstrapFailure> {
        Ok(self.read_json(&self.transaction_path(token)))
    }
    fn write_transaction(
        &self,
        transaction: &GithubBootstrapTransactionV1,
    ) -> Result<(), GithubBootstrapFailure> {
        self.write_json(
            &self.transaction_path(&transaction.confirmation_token),
            transaction,
        )
    }
    fn write_json<T: Serialize>(
        &self,
        path: &Path,
        value: &T,
    ) -> Result<(), GithubBootstrapFailure> {
        let parent = path
            .parent()
            .ok_or(GithubBootstrapFailure::TransactionPersistenceFailed)?;
        fs::create_dir_all(parent)
            .map_err(|_| GithubBootstrapFailure::TransactionPersistenceFailed)?;
        let bytes = serde_json::to_vec(value)
            .map_err(|_| GithubBootstrapFailure::TransactionPersistenceFailed)?;
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, bytes)
            .map_err(|_| GithubBootstrapFailure::TransactionPersistenceFailed)?;
        fs::rename(temporary, path)
            .map_err(|_| GithubBootstrapFailure::TransactionPersistenceFailed)
    }
    fn read_json<T: for<'a> Deserialize<'a>>(&self, path: &Path) -> Option<T> {
        let bytes = fs::read(path).ok()?;
        if bytes.len() > 32_768 {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    }
}

fn verify_post_mutation_authority(
    runner: &impl GithubBootstrapRunner,
    workspace: &Path,
    gh: &Path,
    policy: GithubBootstrapPolicy,
) -> Result<(), GithubBootstrapFailure> {
    let origin = runner.git(workspace, &["config", "--get", "remote.origin.url"])?;
    if !origin.success || bounded_text(&origin.stdout).as_deref() != Some(policy.origin) {
        return Err(GithubBootstrapFailure::OriginUnexpected);
    }
    prove_repository_state(runner, gh, policy, RepositoryState::Exists)
}

fn validated_workspace(
    policy: GithubBootstrapPolicy,
    requested_workspace: &str,
    runner: &impl GithubBootstrapRunner,
    expected_existing_origin: Option<&str>,
) -> Result<(PathBuf, String), GithubBootstrapFailure> {
    if requested_workspace.is_empty()
        || requested_workspace.len() > MAX_WORKSPACE_LEN
        || requested_workspace.trim() != requested_workspace
    {
        return Err(GithubBootstrapFailure::InvalidRequest);
    }
    let raw = PathBuf::from(requested_workspace);
    if !raw.is_absolute() {
        return Err(GithubBootstrapFailure::WorkspaceNotAbsolute);
    }
    let metadata =
        fs::symlink_metadata(&raw).map_err(|_| GithubBootstrapFailure::WorkspaceUnavailable)?;
    if !metadata.is_dir() {
        return Err(GithubBootstrapFailure::WorkspaceUnavailable);
    }
    if path_is_reparse_or_symlink(&metadata) {
        return Err(GithubBootstrapFailure::WorkspaceReparseOrSymlink);
    }
    let workspace = raw
        .canonicalize()
        .map_err(|_| GithubBootstrapFailure::WorkspaceUnavailable)?;
    let expected_raw = policy_workspace_path(policy)?;
    if !expected_raw.is_absolute() {
        return Err(GithubBootstrapFailure::WorkspacePolicyMismatch);
    }
    let expected_metadata = fs::symlink_metadata(&expected_raw)
        .map_err(|_| GithubBootstrapFailure::WorkspacePolicyMismatch)?;
    if !expected_metadata.is_dir() || path_is_reparse_or_symlink(&expected_metadata) {
        return Err(GithubBootstrapFailure::WorkspacePolicyMismatch);
    }
    let expected = expected_raw
        .canonicalize()
        .map_err(|_| GithubBootstrapFailure::WorkspacePolicyMismatch)?;
    if workspace != expected {
        return Err(GithubBootstrapFailure::WorkspacePolicyMismatch);
    }
    let root = runner.git(&workspace, &["rev-parse", "--show-toplevel"])?;
    if !root.success {
        return Err(GithubBootstrapFailure::WorkspaceNotGitRoot);
    }
    let root = bounded_text(&root.stdout).ok_or(GithubBootstrapFailure::GitOutputInvalid)?;
    let canonical_root = PathBuf::from(root)
        .canonicalize()
        .map_err(|_| GithubBootstrapFailure::WorkspaceNotGitRoot)?;
    if canonical_root != workspace {
        return Err(GithubBootstrapFailure::WorkspaceNestedGitRoot);
    }
    let origin = runner.git(&workspace, &["config", "--get", "remote.origin.url"])?;
    if origin.success {
        if bounded_text(&origin.stdout).as_deref() != expected_existing_origin {
            return Err(GithubBootstrapFailure::OriginAlreadyPresent);
        }
    } else if expected_existing_origin.is_some() {
        return Err(GithubBootstrapFailure::EvidenceDrift);
    }
    Ok((workspace.clone(), directory_identity(&workspace, &metadata)))
}

fn policy_workspace_path(policy: GithubBootstrapPolicy) -> Result<PathBuf, GithubBootstrapFailure> {
    const USERPROFILE_PREFIX: &str = "%USERPROFILE%\\";
    let Some(relative) = policy.workspace.strip_prefix(USERPROFILE_PREFIX) else {
        return Ok(PathBuf::from(policy.workspace));
    };
    let user_profile = env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .ok_or(GithubBootstrapFailure::WorkspacePolicyMismatch)?;
    Ok(user_profile.join(relative))
}

fn git_state(
    runner: &impl GithubBootstrapRunner,
    workspace: &Path,
) -> Result<(GitHeadState, String, bool, u32), GithubBootstrapFailure> {
    let head = runner.git(workspace, &["rev-parse", "--verify", "HEAD"])?;
    let mut unborn = false;
    let head = if head.success {
        let value = bounded_text(&head.stdout).ok_or(GithubBootstrapFailure::GitOutputInvalid)?;
        if !(value.len() == 40 || value.len() == 64)
            || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(GithubBootstrapFailure::HeadInvalid);
        }
        Some(value)
    } else {
        let symbolic = runner.git(workspace, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        if !symbolic.success {
            return Err(GithubBootstrapFailure::HeadUnavailable);
        }
        let count = runner.git(workspace, &["rev-list", "--count", "--all"])?;
        if !count.success || bounded_text(&count.stdout).as_deref() != Some("0") {
            return Err(GithubBootstrapFailure::HeadUnavailable);
        }
        unborn = true;
        None
    };
    let branch = if unborn {
        runner.git(workspace, &["symbolic-ref", "--quiet", "--short", "HEAD"])?
    } else {
        runner.git(workspace, &["branch", "--show-current"])?
    };
    if !branch.success {
        return Err(GithubBootstrapFailure::BranchUnavailable);
    }
    let branch = bounded_text(&branch.stdout).ok_or(GithubBootstrapFailure::GitOutputInvalid)?;
    if !safe_branch(&branch) {
        return Err(GithubBootstrapFailure::BranchInvalid);
    }
    let status = runner.git(workspace, &["status", "--porcelain=v1", "-uno"])?;
    if !status.success {
        return Err(GithubBootstrapFailure::GitCommandFailed);
    }
    if status.stdout.len() > MAX_COMMAND_OUTPUT {
        return Err(GithubBootstrapFailure::DirtyStatusInvalid);
    }
    let status =
        String::from_utf8(status.stdout).map_err(|_| GithubBootstrapFailure::DirtyStatusInvalid)?;
    let count = status.lines().count();
    if count > 1_000 {
        return Err(GithubBootstrapFailure::DirtyStatusInvalid);
    }
    Ok((
        GitHeadState { head, unborn },
        branch,
        count > 0,
        count as u32,
    ))
}

fn verify_github_account(
    runner: &impl GithubBootstrapRunner,
    gh: &Path,
) -> Result<(), GithubBootstrapFailure> {
    let response = runner.gh(gh, &["api", "user", "--jq", ".login"])?;
    if !response.success {
        return Err(GithubBootstrapFailure::GithubAuthUnavailable);
    }
    match bounded_text(&response.stdout).as_deref() {
        Some(OWNER) => Ok(()),
        Some(_) => Err(GithubBootstrapFailure::GithubAuthWrongAccount),
        None => Err(GithubBootstrapFailure::GhOutputInvalid),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepositoryState {
    Absent,
    Exists,
}

const GITHUB_ACCEPT: &str = "Accept: application/vnd.github+json";
const GITHUB_VERSION: &str = "X-GitHub-Api-Version: 2022-11-28";

fn repository_state_argv(policy: GithubBootstrapPolicy) -> [String; 9] {
    [
        "api".into(),
        format!("repos/{OWNER}/{}", policy.repository),
        "--include".into(),
        "-H".into(),
        GITHUB_ACCEPT.into(),
        "-H".into(),
        GITHUB_VERSION.into(),
        "--method".into(),
        "GET".into(),
    ]
}

fn account_scope_argv() -> [&'static str; 9] {
    [
        "api",
        "user",
        "--include",
        "-H",
        GITHUB_ACCEPT,
        "-H",
        GITHUB_VERSION,
        "--method",
        "GET",
    ]
}

/// The only repository lookup uses fixed included-header REST and accepts a
/// result only under the scope-qualified bounded HTTP envelope contract.
/// A transport, auth, API, rate-limit, or schema failure is never inferred as
/// target absence.
fn prove_repository_state(
    runner: &impl GithubBootstrapRunner,
    gh: &Path,
    policy: GithubBootstrapPolicy,
    expected: RepositoryState,
) -> Result<(), GithubBootstrapFailure> {
    prove_account_scope(runner, gh)?;
    let argv = repository_state_argv(policy);
    let refs = argv.iter().map(String::as_str).collect::<Vec<_>>();
    let response = runner.gh(gh, &refs)?;
    let (status, _headers, body) = parse_http_response(&response.stdout)?;
    if status == 404 {
        validate_non_authoritative_diagnostic(&response.stderr)?;
        return match expected {
            RepositoryState::Absent => Ok(()),
            RepositoryState::Exists => Err(GithubBootstrapFailure::TransactionConflict),
        };
    }
    if !response.success || !response.stderr.is_empty() {
        return Err(GithubBootstrapFailure::RepositoryGhProcessFailure);
    }
    if status != 200 {
        return Err(GithubBootstrapFailure::TargetRepositoryProbeFailed);
    }
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| GithubBootstrapFailure::TargetRepositoryProbeFailed)?;
    let Some(root) = value.as_object() else {
        return Err(GithubBootstrapFailure::TargetRepositoryProbeFailed);
    };
    let observed = match root {
        repository
            if repository
                .get("owner")
                .and_then(|v| v.get("login"))
                .and_then(Value::as_str)
                == Some(OWNER)
                && repository.get("name").and_then(Value::as_str) == Some(policy.repository) =>
        {
            let numeric_id = repository
                .get("id")
                .and_then(Value::as_u64)
                .filter(|id| *id > 0);
            let node_id = repository
                .get("node_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() <= 256);
            if numeric_id.is_some() && node_id.is_some() {
                RepositoryState::Exists
            } else {
                return Err(GithubBootstrapFailure::TargetRepositoryProbeFailed);
            }
        }
        _ => return Err(GithubBootstrapFailure::TargetRepositoryProbeFailed),
    };
    if observed != expected {
        return Err(match observed {
            RepositoryState::Exists => GithubBootstrapFailure::TargetRepositoryAlreadyExists,
            RepositoryState::Absent => GithubBootstrapFailure::TransactionConflict,
        });
    }
    Ok(())
}

/// A 404 diagnostic is never authority, but accepting it requires it to be a
/// bounded, ordinary UTF-8 diagnostic rather than opaque/binary data.
fn validate_non_authoritative_diagnostic(bytes: &[u8]) -> Result<(), GithubBootstrapFailure> {
    if bytes.len() > MAX_COMMAND_OUTPUT || std::str::from_utf8(bytes).is_err() || bytes.contains(&0)
    {
        return Err(GithubBootstrapFailure::GhHttpEnvelopeInvalid);
    }
    Ok(())
}

fn prove_account_scope(
    runner: &impl GithubBootstrapRunner,
    gh: &Path,
) -> Result<(), GithubBootstrapFailure> {
    let response = runner.gh(gh, &account_scope_argv())?;
    if !response.success || !response.stderr.is_empty() {
        return Err(GithubBootstrapFailure::AccountGhProcessFailure);
    }
    let (status, headers, body) = parse_http_response(&response.stdout)?;
    if status != 200 {
        return Err(GithubBootstrapFailure::GhHttpAuthenticationFailed);
    }
    let scopes = headers
        .get("x-oauth-scopes")
        .ok_or(GithubBootstrapFailure::GhRepoScopeUnproven)?;
    if !scopes
        .split(',')
        .map(str::trim)
        .any(|scope| scope == "repo")
    {
        return Err(GithubBootstrapFailure::GhRepoScopeUnproven);
    }
    let body: Value =
        serde_json::from_slice(body).map_err(|_| GithubBootstrapFailure::GhHttpEnvelopeInvalid)?;
    if body.get("login").and_then(Value::as_str) != Some(OWNER) {
        return Err(GithubBootstrapFailure::GithubAuthWrongAccount);
    }
    Ok(())
}

fn parse_http_response(
    bytes: &[u8],
) -> Result<(u16, std::collections::BTreeMap<String, String>, &[u8]), GithubBootstrapFailure> {
    if bytes.len() > MAX_COMMAND_OUTPUT {
        return Err(GithubBootstrapFailure::GhHttpEnvelopeInvalid);
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| GithubBootstrapFailure::GhHttpEnvelopeInvalid)?;
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .ok_or(GithubBootstrapFailure::GhHttpEnvelopeInvalid)?;
    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or(GithubBootstrapFailure::GhHttpEnvelopeInvalid)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .ok_or(GithubBootstrapFailure::TargetRepositoryProbeFailed)?;
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        let (key, value) = line
            .split_once(':')
            .ok_or(GithubBootstrapFailure::GhHttpEnvelopeInvalid)?;
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() || headers.insert(key, value.trim().to_owned()).is_some() {
            return Err(GithubBootstrapFailure::GhHttpEnvelopeInvalid);
        }
    }
    Ok((status, headers, body.as_bytes()))
}

fn bounded_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_COMMAND_OUTPUT {
        return None;
    }
    let value = std::str::from_utf8(bytes).ok()?.trim();
    if value.is_empty() || value.len() > MAX_COMMAND_OUTPUT || value.contains('\0') {
        return None;
    }
    Some(value.into())
}
fn safe_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= MAX_BRANCH_LEN
        && !branch.starts_with('-')
        && !branch.contains("..")
        && branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
}
fn path_is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}
fn directory_identity(path: &Path, metadata: &fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |value| value.as_nanos());
    sha256_text(&format!(
        "{}|{}|{}",
        path.to_string_lossy(),
        metadata.len(),
        modified
    ))
}
fn sha256_file(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|bytes| sha256_bytes(&bytes))
}
fn sha256_text(value: &str) -> String {
    sha256_bytes(value.as_bytes())
}
fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn preflight_fingerprint(preflight: &GithubBootstrapPreflightV1) -> String {
    sha256_text(&format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        preflight.project_id,
        preflight.workspace,
        preflight.workspace_identity,
        preflight.head,
        preflight.head_state,
        preflight.branch,
        preflight.working_tree_dirty,
        preflight.working_tree_change_count,
        preflight.gh_executable_fingerprint,
        preflight.gh_selected_slot,
        preflight.gh_selected_identity,
        preflight.git_executable_fingerprint,
        preflight.git_selected_slot,
        preflight.git_selected_identity,
        preflight.policy_origin,
        preflight.expires_at_unix
    ))
}
fn default_trusted_gh_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from(r"C:\Program Files\GitHub CLI\gh.exe"),
        PathBuf::from(r"C:\Program Files (x86)\GitHub CLI\gh.exe"),
    ];
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local_app_data).join("Programs\\GitHub CLI\\gh.exe"));
    }
    candidates
}

fn default_trusted_git_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\Program Files\Git\cmd\git.exe"),
        PathBuf::from(r"C:\Program Files\Git\bin\git.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\cmd\git.exe"),
    ]
}

fn resolve_trusted_tool(
    kind: TrustedToolKind,
    raw_candidates: &[PathBuf],
    workspace: &Path,
) -> Result<TrustedToolIdentity, GithubBootstrapFailure> {
    let workspace = workspace.canonicalize().map_err(|_| match kind {
        TrustedToolKind::Git => GithubBootstrapFailure::GitUnavailable,
        TrustedToolKind::Gh => GithubBootstrapFailure::GhUnavailable,
    })?;
    for (index, raw) in raw_candidates.iter().enumerate() {
        let metadata = match fs::symlink_metadata(raw) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if path_is_reparse_or_symlink(&metadata) || !metadata.is_file() {
            return Err(match kind {
                TrustedToolKind::Git => GithubBootstrapFailure::GitUnsafeExecutable,
                TrustedToolKind::Gh => GithubBootstrapFailure::GhUnsafeExecutable,
            });
        }
        let canonical = raw.canonicalize().map_err(|_| match kind {
            TrustedToolKind::Git => GithubBootstrapFailure::GitUnavailable,
            TrustedToolKind::Gh => GithubBootstrapFailure::GhUnavailable,
        })?;
        if canonical.starts_with(&workspace) {
            return Err(match kind {
                TrustedToolKind::Git => GithubBootstrapFailure::GitInsideWorkspace,
                TrustedToolKind::Gh => GithubBootstrapFailure::GhInsideWorkspace,
            });
        }
        let fingerprint = sha256_file(&canonical).ok_or(match kind {
            TrustedToolKind::Git => GithubBootstrapFailure::GitUnavailable,
            TrustedToolKind::Gh => GithubBootstrapFailure::GhUnavailable,
        })?;
        return Ok(TrustedToolIdentity {
            slot: index as u8,
            canonical,
            fingerprint,
        });
    }
    Err(match kind {
        TrustedToolKind::Git => GithubBootstrapFailure::GitUnavailable,
        TrustedToolKind::Gh => GithubBootstrapFailure::GhUnavailable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeRunner {
        git: Mutex<BTreeMap<String, CommandResult>>,
        gh: Mutex<BTreeMap<String, CommandResult>>,
        git_sequence: Mutex<BTreeMap<String, VecDeque<CommandResult>>>,
        gh_sequence: Mutex<BTreeMap<String, VecDeque<CommandResult>>>,
        calls: Mutex<Vec<String>>,
    }
    impl FakeRunner {
        fn key(args: &[&str]) -> String {
            args.join(" ")
        }
        fn git_ok(&self, args: &[&str], out: &str) {
            self.git.lock().unwrap().insert(
                Self::key(args),
                CommandResult {
                    success: true,
                    stdout: out.as_bytes().to_vec(),
                    stderr: vec![],
                },
            );
        }
        fn git_fail(&self, args: &[&str]) {
            self.git.lock().unwrap().insert(
                Self::key(args),
                CommandResult {
                    success: false,
                    stdout: vec![],
                    stderr: vec![],
                },
            );
        }
        fn gh_ok(&self, args: &[&str], out: &str) {
            self.gh.lock().unwrap().insert(
                Self::key(args),
                CommandResult {
                    success: true,
                    stdout: out.as_bytes().to_vec(),
                    stderr: vec![],
                },
            );
        }
        fn gh_fail(&self, args: &[&str]) {
            self.gh.lock().unwrap().insert(
                Self::key(args),
                CommandResult {
                    success: false,
                    stdout: vec![],
                    stderr: vec![],
                },
            );
        }
        fn gh_result(&self, args: &[&str], success: bool, stdout: &[u8], stderr: &[u8]) {
            self.gh.lock().unwrap().insert(
                Self::key(args),
                CommandResult {
                    success,
                    stdout: stdout.into(),
                    stderr: stderr.into(),
                },
            );
        }
        fn git_sequence(&self, args: &[&str], results: Vec<CommandResult>) {
            self.git_sequence
                .lock()
                .unwrap()
                .insert(Self::key(args), results.into());
        }
        fn gh_sequence(&self, args: &[&str], results: Vec<CommandResult>) {
            self.gh_sequence
                .lock()
                .unwrap()
                .insert(Self::key(args), results.into());
        }
    }
    impl GithubBootstrapRunner for FakeRunner {
        fn git(&self, _: &Path, args: &[&str]) -> Result<CommandResult, GithubBootstrapFailure> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("git {}", Self::key(args)));
            if let Some(result) = self
                .git_sequence
                .lock()
                .unwrap()
                .get_mut(&Self::key(args))
                .and_then(VecDeque::pop_front)
            {
                return Ok(result);
            }
            self.git
                .lock()
                .unwrap()
                .get(&Self::key(args))
                .cloned()
                .ok_or(GithubBootstrapFailure::GitCommandFailed)
        }
        fn gh(&self, _: &Path, args: &[&str]) -> Result<CommandResult, GithubBootstrapFailure> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("gh {}", Self::key(args)));
            if let Some(result) = self
                .gh_sequence
                .lock()
                .unwrap()
                .get_mut(&Self::key(args))
                .and_then(VecDeque::pop_front)
            {
                return Ok(result);
            }
            self.gh
                .lock()
                .unwrap()
                .get(&Self::key(args))
                .cloned()
                .ok_or(GithubBootstrapFailure::GhCommandFailed)
        }
    }

    #[test]
    fn policy_is_exact_and_never_includes_catdesk() {
        assert!(approved_policy("catdesk").is_none());
        let policy = approved_policy("BYOVD_DRIVER_PIPELINE").unwrap();
        assert_eq!(policy.repository, "BYOVD_DRIVER_PIPELINE");
        assert!(!policy.workspace.contains("\\\\"));
    }

    #[test]
    fn account_scope_request_includes_body_and_never_silences_it() {
        let argv = account_scope_argv();
        assert_eq!(argv[0], "api");
        assert_eq!(argv[1], "user");
        assert!(argv.contains(&"--include"));
        assert!(argv.contains(&GITHUB_ACCEPT));
        assert!(argv.contains(&GITHUB_VERSION));
        assert!(!argv.contains(&"--silent"));
    }

    #[test]
    fn accepted_404_diagnostic_requires_bounded_utf8_without_trusting_text() {
        assert!(validate_non_authoritative_diagnostic(b"gh: HTTP 404\n").is_ok());
        assert!(validate_non_authoritative_diagnostic(&[0xff]).is_err());
        assert!(validate_non_authoritative_diagnostic(b"diagnostic\0").is_err());
        assert!(
            validate_non_authoritative_diagnostic(&vec![b'x'; MAX_COMMAND_OUTPUT + 1]).is_err()
        );
    }

    #[test]
    fn repository_state_accepts_real_404_shape_only_after_scope_proof() {
        let runner = FakeRunner::default();
        let executable = PathBuf::from("C:\\trusted\\gh.exe");
        let policy = approved_policy("BYOVD_DRIVER_PIPELINE").expect("policy");
        let account = account_scope_argv();
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_result(
            &account,
            true,
            b"HTTP/1.1 200 OK\r\nX-OAuth-Scopes: repo\r\n\r\n{\"login\":\"vnath99\"}",
            b"",
        );
        runner.gh_result(
            &repo_refs,
            false,
            b"HTTP/1.1 404 Not Found\r\n\r\n{}",
            b"gh: Not Found (HTTP 404)\n",
        );
        assert!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent).is_ok()
        );
        runner.gh_result(
            &repo_refs,
            false,
            b"HTTP/1.1 404 Not Found\r\n\r\n{}",
            b"different bounded diagnostic\n",
        );
        assert!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent).is_ok()
        );
        runner.gh_result(
            &repo_refs,
            false,
            b"HTTP/1.1 404 Not Found\r\n\r\n{}",
            &[0xff],
        );
        assert!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent).is_err()
        );
        runner.gh_result(
            &repo_refs,
            false,
            b"HTTP/1.1 404 Not Found\r\n\r\n{}",
            b"bad\0diagnostic",
        );
        assert!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent).is_err()
        );
        runner.gh_result(&repo_refs, true, b"HTTP/1.1 200 OK\r\n\r\n{\"id\":\"opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}", b"diagnostic");
        assert!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Exists).is_err()
        );
    }

    #[test]
    fn unborn_post_mutation_authority_requires_exact_origin_and_exists_without_push() {
        let runner = FakeRunner::default();
        let workspace = PathBuf::from("C:\\fixture\\workspace");
        let executable = PathBuf::from("C:\\trusted\\gh.exe");
        let policy = approved_policy("BUG_BOUNTY_RECON_PLATFORM").expect("policy");
        runner.git_ok(&["config", "--get", "remote.origin.url"], policy.origin);
        let account = account_scope_argv();
        runner.gh_ok(
            &account,
            "HTTP/1.1 200 OK\r\nX-OAuth-Scopes: repo\r\n\r\n{\"login\":\"vnath99\"}",
        );
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_ok(&repo_refs, "HTTP/1.1 200 OK\r\n\r\n{\"id\":1,\"node_id\":\"R_opaque\",\"name\":\"BUG_BOUNTY_RECON_PLATFORM\",\"owner\":{\"login\":\"vnath99\"}}");
        assert!(verify_post_mutation_authority(&runner, &workspace, &executable, policy).is_ok());
        let calls = runner.calls.lock().expect("calls");
        assert!(
            calls.iter().all(|call| {
                ![
                    "push",
                    " add ",
                    "commit",
                    "checkout",
                    "branch",
                    "init",
                    "README",
                    "license",
                    ".gitignore",
                ]
                .iter()
                .any(|forbidden| call.contains(forbidden))
            }),
            "unexpected unborn mutation trace: {calls:?}"
        );
    }

    #[test]
    fn unborn_post_mutation_authority_rejects_origin_or_repository_drift() {
        let runner = FakeRunner::default();
        let workspace = PathBuf::from("C:\\fixture\\workspace");
        let executable = PathBuf::from("C:\\trusted\\gh.exe");
        let policy = approved_policy("BUG_BOUNTY_RECON_PLATFORM").expect("policy");
        runner.git_ok(
            &["config", "--get", "remote.origin.url"],
            "https://example.invalid/wrong.git",
        );
        assert!(verify_post_mutation_authority(&runner, &workspace, &executable, policy).is_err());
    }

    fn command_result(success: bool, stdout: &[u8], stderr: &[u8]) -> CommandResult {
        CommandResult {
            success,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn confirm_fixture() -> (
        GithubBootstrapStore,
        GithubBootstrapPolicy,
        PathBuf,
        FakeRunner,
    ) {
        let root = std::env::temp_dir().join(format!("catdesk-r27-{}", Uuid::new_v4()));
        let workspace = root.join("workspace");
        let tools = root.join("tools");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::create_dir_all(&tools).expect("tools");
        let git = tools.join("git.exe");
        let gh = tools.join("gh.exe");
        fs::write(&git, b"test-git").expect("git fixture");
        fs::write(&gh, b"test-gh").expect("gh fixture");
        let workspace_policy: &'static str =
            Box::leak(workspace.to_string_lossy().into_owned().into_boxed_str());
        (
            GithubBootstrapStore::with_candidates(root.join("state"), vec![git], vec![gh]),
            GithubBootstrapPolicy {
                project_id: "R27_TEST_PROJECT",
                workspace: workspace_policy,
                repository: "R27_TEST_REPOSITORY",
                origin: "https://github.com/vnath99/R27_TEST_REPOSITORY.git",
            },
            workspace,
            FakeRunner::default(),
        )
    }

    fn configure_git_state(runner: &FakeRunner, workspace: &Path, unborn: bool) {
        runner.git_ok(
            &["rev-parse", "--show-toplevel"],
            &workspace.to_string_lossy(),
        );
        if unborn {
            runner.git_fail(&["rev-parse", "--verify", "HEAD"]);
            runner.git_ok(&["symbolic-ref", "--quiet", "--short", "HEAD"], "main");
            runner.git_ok(&["rev-list", "--count", "--all"], "0");
        } else {
            runner.git_ok(
                &["rev-parse", "--verify", "HEAD"],
                "0123456789012345678901234567890123456789",
            );
            runner.git_ok(&["branch", "--show-current"], "main");
        }
        runner.git_ok(&["status", "--porcelain=v1", "-uno"], "");
    }

    fn account_http() -> CommandResult {
        command_result(
            true,
            b"HTTP/1.1 200 OK\r\nX-OAuth-Scopes: repo\r\n\r\n{\"login\":\"vnath99\"}",
            b"",
        )
    }

    fn absent_http() -> CommandResult {
        command_result(
            false,
            b"HTTP/1.1 404 Not Found\r\n\r\n{}",
            b"gh: Not Found\n",
        )
    }

    fn exists_http(policy: GithubBootstrapPolicy) -> CommandResult {
        command_result(
            true,
            format!(
                "HTTP/1.1 200 OK\r\n\r\n{{\"id\":1,\"node_id\":\"R_opaque\",\"name\":\"{}\",\"owner\":{{\"login\":\"vnath99\"}}}}",
                policy.repository
            )
            .as_bytes(),
            b"",
        )
    }

    fn assert_unborn_mutation_free(runner: &FakeRunner) {
        let calls = runner.calls.lock().expect("calls");
        assert!(
            calls.iter().all(|call| {
                ![
                    "git push ",
                    "git add ",
                    "git commit ",
                    "git checkout ",
                    "git switch ",
                    "git init",
                    "git branch -m",
                    "git branch --set-upstream-to",
                    "README",
                    "license",
                    ".gitignore",
                ]
                .iter()
                .any(|forbidden| call.contains(forbidden))
            }),
            "unexpected unborn mutation trace: {calls:?}"
        );
    }

    #[test]
    fn confirm_unborn_persists_terminal_only_after_fresh_origin_and_exists_proofs() {
        let (store, policy, workspace, runner) = confirm_fixture();
        configure_git_state(&runner, &workspace, true);
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_sequence(
            &repo_refs,
            vec![
                absent_http(),
                absent_http(),
                absent_http(),
                exists_http(policy),
            ],
        );
        runner.git_sequence(
            &["config", "--get", "remote.origin.url"],
            vec![
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(true, policy.origin.as_bytes(), b""),
            ],
        );
        runner.git_ok(&["remote", "add", "origin", policy.origin], "");
        runner.gh_ok(
            &["repo", "create", "vnath99/R27_TEST_REPOSITORY", "--private"],
            "",
        );

        let preflight = store
            .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
            .expect("unborn preflight");
        let outcome = store
            .confirm_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                101,
                &runner,
            )
            .expect("unborn confirmation");
        assert!(
            outcome
                .stages
                .contains(&"PUSH_SKIPPED_NO_COMMITTED_HISTORY".into())
        );
        let persisted = store
            .read_transaction(&preflight.confirmation_token)
            .expect("transaction")
            .expect("persisted transaction");
        assert!(
            persisted
                .stages
                .contains("PUSH_SKIPPED_NO_COMMITTED_HISTORY")
        );
        assert_unborn_mutation_free(&runner);
    }

    #[test]
    fn confirm_unborn_postcondition_failures_leave_terminal_stage_absent() {
        let (store, policy, workspace, runner) = confirm_fixture();
        configure_git_state(&runner, &workspace, true);
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_sequence(
            &repo_refs,
            vec![absent_http(), absent_http(), absent_http(), absent_http()],
        );
        runner.git_sequence(
            &["config", "--get", "remote.origin.url"],
            vec![
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(true, b"https://example.invalid/wrong.git", b""),
            ],
        );
        runner.git_ok(&["remote", "add", "origin", policy.origin], "");
        runner.gh_ok(
            &["repo", "create", "vnath99/R27_TEST_REPOSITORY", "--private"],
            "",
        );
        let preflight = store
            .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
            .expect("preflight");
        assert_eq!(
            store.confirm_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                101,
                &runner,
            ),
            Err(GithubBootstrapFailure::OriginUnexpected)
        );
        let persisted = store
            .read_transaction(&preflight.confirmation_token)
            .expect("transaction")
            .expect("persisted transaction");
        assert!(
            !persisted
                .stages
                .contains("PUSH_SKIPPED_NO_COMMITTED_HISTORY")
        );
        assert_unborn_mutation_free(&runner);
    }

    #[test]
    fn confirm_unborn_postcreate_repository_404_leaves_terminal_stage_absent() {
        let (store, policy, workspace, runner) = confirm_fixture();
        configure_git_state(&runner, &workspace, true);
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_sequence(
            &repo_refs,
            vec![absent_http(), absent_http(), absent_http(), absent_http()],
        );
        runner.git_sequence(
            &["config", "--get", "remote.origin.url"],
            vec![
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(true, policy.origin.as_bytes(), b""),
            ],
        );
        runner.git_ok(&["remote", "add", "origin", policy.origin], "");
        runner.gh_ok(
            &["repo", "create", "vnath99/R27_TEST_REPOSITORY", "--private"],
            "",
        );
        let preflight = store
            .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
            .expect("preflight");
        assert_eq!(
            store.confirm_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                101,
                &runner,
            ),
            Err(GithubBootstrapFailure::TransactionConflict)
        );
        let persisted = store
            .read_transaction(&preflight.confirmation_token)
            .expect("transaction")
            .expect("persisted transaction");
        assert!(
            !persisted
                .stages
                .contains("PUSH_SKIPPED_NO_COMMITTED_HISTORY")
        );
        assert_unborn_mutation_free(&runner);
    }

    #[test]
    fn confirm_unborn_postcreate_authority_failure_matrix_is_fail_closed() {
        struct MatrixCase {
            name: &'static str,
            post_account: CommandResult,
            post_repository: Option<CommandResult>,
            expected: GithubBootstrapFailure,
        }

        let valid_account = account_http();
        let cases = vec![
            MatrixCase {
                name: "wrong repository owner",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 200 OK\r\n\r\n{\"id\":\"opaque\",\"name\":\"R27_TEST_REPOSITORY\",\"owner\":{\"login\":\"wrong\"}}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "wrong repository name",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 200 OK\r\n\r\n{\"id\":\"opaque\",\"name\":\"wrong\",\"owner\":{\"login\":\"vnath99\"}}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "missing opaque repository id",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 200 OK\r\n\r\n{\"name\":\"R27_TEST_REPOSITORY\",\"owner\":{\"login\":\"vnath99\"}}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "empty opaque repository id",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 200 OK\r\n\r\n{\"id\":\"\",\"name\":\"R27_TEST_REPOSITORY\",\"owner\":{\"login\":\"vnath99\"}}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "non-string opaque repository id",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 200 OK\r\n\r\n{\"id\":1,\"name\":\"R27_TEST_REPOSITORY\",\"owner\":{\"login\":\"vnath99\"}}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "account wrong login",
                post_account: command_result(
                    true,
                    b"HTTP/1.1 200 OK\r\nX-OAuth-Scopes: repo\r\n\r\n{\"login\":\"wrong\"}",
                    b"",
                ),
                post_repository: None,
                expected: GithubBootstrapFailure::GithubAuthWrongAccount,
            },
            MatrixCase {
                name: "account missing repo scope",
                post_account: command_result(
                    true,
                    b"HTTP/1.1 200 OK\r\nX-OAuth-Scopes: read:org\r\n\r\n{\"login\":\"vnath99\"}",
                    b"",
                ),
                post_repository: None,
                expected: GithubBootstrapFailure::GhRepoScopeUnproven,
            },
            MatrixCase {
                name: "account process failure",
                post_account: command_result(
                    false,
                    b"HTTP/1.1 200 OK\r\nX-OAuth-Scopes: repo\r\n\r\n{\"login\":\"vnath99\"}",
                    b"",
                ),
                post_repository: None,
                expected: GithubBootstrapFailure::AccountGhProcessFailure,
            },
            MatrixCase {
                name: "repository process failure",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    false,
                    b"HTTP/1.1 200 OK\r\n\r\n{\"id\":\"opaque\",\"name\":\"R27_TEST_REPOSITORY\",\"owner\":{\"login\":\"vnath99\"}}",
                    b"",
                )),
                expected: GithubBootstrapFailure::RepositoryGhProcessFailure,
            },
            MatrixCase {
                name: "malformed repository envelope",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(true, b"not an HTTP envelope", b"")),
                expected: GithubBootstrapFailure::GhHttpEnvelopeInvalid,
            },
            MatrixCase {
                name: "multiple repository envelopes",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 200 OK\r\n\r\nHTTP/1.1 200 OK\r\n\r\n{}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "repository HTTP 401",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 401 Unauthorized\r\n\r\n{}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "repository HTTP 403",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(true, b"HTTP/1.1 403 Forbidden\r\n\r\n{}", b"")),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "repository HTTP 429",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 429 Too Many Requests\r\n\r\n{}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
            MatrixCase {
                name: "repository HTTP 500",
                post_account: valid_account.clone(),
                post_repository: Some(command_result(
                    true,
                    b"HTTP/1.1 500 Internal Server Error\r\n\r\n{}",
                    b"",
                )),
                expected: GithubBootstrapFailure::TargetRepositoryProbeFailed,
            },
        ];

        for case in cases {
            let (store, policy, workspace, runner) = confirm_fixture();
            configure_git_state(&runner, &workspace, true);
            let account = account_scope_argv();
            runner.gh_sequence(
                &account,
                vec![
                    valid_account.clone(),
                    valid_account.clone(),
                    valid_account.clone(),
                    case.post_account,
                ],
            );
            let repo = repository_state_argv(policy);
            let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
            let mut repository_results = vec![absent_http(), absent_http(), absent_http()];
            if let Some(post_repository) = case.post_repository {
                repository_results.push(post_repository);
            }
            runner.gh_sequence(&repo_refs, repository_results);
            runner.git_sequence(
                &["config", "--get", "remote.origin.url"],
                vec![
                    command_result(false, b"", b""),
                    command_result(false, b"", b""),
                    command_result(false, b"", b""),
                    command_result(true, policy.origin.as_bytes(), b""),
                ],
            );
            runner.git_ok(&["remote", "add", "origin", policy.origin], "");
            runner.gh_ok(
                &["repo", "create", "vnath99/R27_TEST_REPOSITORY", "--private"],
                "",
            );
            let preflight = store
                .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
                .unwrap_or_else(|failure| panic!("{} preflight: {failure:?}", case.name));
            assert_eq!(
                store.confirm_with_test_policy(
                    policy,
                    policy.project_id,
                    &preflight.confirmation_token,
                    101,
                    &runner,
                ),
                Err(case.expected),
                "{}",
                case.name
            );
            let persisted = store
                .read_transaction(&preflight.confirmation_token)
                .expect("transaction")
                .expect("persisted transaction");
            assert!(persisted.stages.contains("JOURNALED"), "{}", case.name);
            assert!(
                persisted.stages.contains("REPOSITORY_CREATED"),
                "{}",
                case.name
            );
            assert!(persisted.stages.contains("ORIGIN_ADDED"), "{}", case.name);
            assert!(
                !persisted
                    .stages
                    .contains("PUSH_SKIPPED_NO_COMMITTED_HISTORY"),
                "{}",
                case.name
            );
            assert_unborn_mutation_free(&runner);
        }
    }

    #[test]
    fn confirm_unborn_recovery_revalidates_journal_and_terminal_replay() {
        let (store, policy, workspace, runner) = confirm_fixture();
        configure_git_state(&runner, &workspace, true);
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_sequence(
            &repo_refs,
            vec![absent_http(), exists_http(policy), exists_http(policy)],
        );
        runner.gh_result(
            &repo_refs,
            true,
            &exists_http(policy).stdout,
            &exists_http(policy).stderr,
        );
        runner.git_sequence(
            &["config", "--get", "remote.origin.url"],
            vec![command_result(false, b"", b"")],
        );
        runner.git_ok(&["config", "--get", "remote.origin.url"], policy.origin);
        let preflight = store
            .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
            .expect("preflight");
        store
            .write_transaction(&GithubBootstrapTransactionV1 {
                schema_version: 1,
                confirmation_token: preflight.confirmation_token.clone(),
                project_id: policy.project_id.into(),
                evidence_fingerprint: preflight.evidence_fingerprint.clone(),
                stages: BTreeSet::from([
                    "JOURNALED".into(),
                    "REPOSITORY_CREATED".into(),
                    "ORIGIN_ADDED".into(),
                ]),
            })
            .expect("seed transaction");
        let first = store
            .confirm_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                101,
                &runner,
            )
            .expect("recovery confirmation");
        assert!(
            first
                .stages
                .contains(&"PUSH_SKIPPED_NO_COMMITTED_HISTORY".into())
        );
        let calls_after_first = runner.calls.lock().expect("calls").len();
        let second = store
            .confirm_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                102,
                &runner,
            )
            .expect("terminal replay");
        assert_eq!(first.stages, second.stages);
        assert!(runner.calls.lock().expect("calls").len() > calls_after_first);
        assert_unborn_mutation_free(&runner);
    }

    #[test]
    fn expired_started_unborn_transaction_recovers_without_a_second_create() {
        let (store, policy, workspace, runner) = confirm_fixture();
        configure_git_state(&runner, &workspace, true);
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_sequence(
            &repo_refs,
            vec![absent_http(), exists_http(policy), exists_http(policy)],
        );
        runner.git_sequence(
            &["config", "--get", "remote.origin.url"],
            vec![
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(true, policy.origin.as_bytes(), b""),
            ],
        );
        runner.git_ok(&["remote", "add", "origin", policy.origin], "");
        let preflight = store
            .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
            .expect("preflight");
        store
            .write_transaction(&GithubBootstrapTransactionV1 {
                schema_version: 1,
                confirmation_token: preflight.confirmation_token.clone(),
                project_id: policy.project_id.into(),
                evidence_fingerprint: preflight.evidence_fingerprint.clone(),
                stages: BTreeSet::from(["JOURNALED".into(), "REPOSITORY_CREATED".into()]),
            })
            .expect("seed started transaction");
        let outcome = store
            .recover_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                preflight.expires_at_unix + 1,
                &runner,
            )
            .expect("expired recovery");
        assert!(outcome.stages.contains(&"ORIGIN_ADDED".into()));
        assert!(
            outcome
                .stages
                .contains(&"PUSH_SKIPPED_NO_COMMITTED_HISTORY".into())
        );
        assert!(
            runner
                .calls
                .lock()
                .expect("calls")
                .iter()
                .all(|call| !call.starts_with("gh repo create "))
        );
        assert_unborn_mutation_free(&runner);
    }

    #[test]
    fn expired_unstarted_recovery_is_rejected_without_mutation() {
        let (store, policy, workspace, runner) = confirm_fixture();
        configure_git_state(&runner, &workspace, true);
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_result(
            &repo_refs,
            false,
            &absent_http().stdout,
            &absent_http().stderr,
        );
        runner.git_fail(&["config", "--get", "remote.origin.url"]);
        let preflight = store
            .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
            .expect("preflight");
        let calls_before = runner.calls.lock().expect("calls").len();
        assert_eq!(
            store.recover_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                preflight.expires_at_unix + 1,
                &runner,
            ),
            Err(GithubBootstrapFailure::TokenExpired)
        );
        assert_eq!(runner.calls.lock().expect("calls").len(), calls_before);
    }

    #[test]
    fn expired_journaled_only_recovery_is_rejected_without_mutation() {
        let (store, policy, workspace, runner) = confirm_fixture();
        configure_git_state(&runner, &workspace, true);
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_result(
            &repo_refs,
            false,
            &absent_http().stdout,
            &absent_http().stderr,
        );
        runner.git_fail(&["config", "--get", "remote.origin.url"]);
        let preflight = store
            .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
            .expect("preflight");
        store
            .write_transaction(&GithubBootstrapTransactionV1 {
                schema_version: 1,
                confirmation_token: preflight.confirmation_token.clone(),
                project_id: policy.project_id.into(),
                evidence_fingerprint: preflight.evidence_fingerprint.clone(),
                stages: BTreeSet::from(["JOURNALED".into()]),
            })
            .expect("journal-only transaction");
        let calls_before = runner.calls.lock().expect("calls").len();
        assert_eq!(
            store.recover_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                preflight.expires_at_unix + 1,
                &runner,
            ),
            Err(GithubBootstrapFailure::TokenExpired)
        );
        assert_eq!(runner.calls.lock().expect("calls").len(), calls_before);
        let transaction = store
            .read_transaction(&preflight.confirmation_token)
            .expect("read")
            .expect("transaction");
        assert_eq!(transaction.stages, BTreeSet::from(["JOURNALED".into()]));
    }

    #[test]
    fn numeric_repository_identity_requires_positive_id_and_node_id() {
        let runner = FakeRunner::default();
        let executable = PathBuf::from("C:\\trusted\\gh.exe");
        let policy = approved_policy("BYOVD_DRIVER_PIPELINE").expect("policy");
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_result(
            &repo_refs,
            true,
            b"HTTP/1.1 200 OK\r\n\r\n{\"id\":1,\"node_id\":\"R_opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}",
            b"",
        );
        assert!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Exists).is_ok()
        );
        let oversized_node = "x".repeat(257);
        let invalid = vec![
            "{\"id\":0,\"node_id\":\"R_opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}".into(),
            "{\"id\":-1,\"node_id\":\"R_opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}".into(),
            "{\"id\":1.5,\"node_id\":\"R_opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}".into(),
            "{\"id\":\"1\",\"node_id\":\"R_opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}".into(),
            "{\"node_id\":\"R_opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}".into(),
            "{\"id\":1,\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}".into(),
            "{\"id\":1,\"node_id\":\"\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}".into(),
            format!("{{\"id\":1,\"node_id\":\"{oversized_node}\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{{\"login\":\"vnath99\"}}}}"),
            "{\"id\":1,\"node_id\":1,\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}".into(),
            "{\"id\":1,\"node_id\":\"R_opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"wrong\"}}".into(),
            "{\"id\":1,\"node_id\":\"R_opaque\",\"name\":\"wrong\",\"owner\":{\"login\":\"vnath99\"}}".into(),
        ];
        for body in invalid {
            runner.gh_result(
                &repo_refs,
                true,
                &[b"HTTP/1.1 200 OK\r\n\r\n", body.as_bytes()].concat(),
                b"",
            );
            assert_eq!(
                prove_repository_state(&runner, &executable, policy, RepositoryState::Exists),
                Err(GithubBootstrapFailure::TargetRepositoryProbeFailed)
            );
        }
    }

    #[test]
    fn confirm_committed_head_still_pushes_the_current_branch() {
        let (store, policy, workspace, runner) = confirm_fixture();
        configure_git_state(&runner, &workspace, false);
        let account = account_scope_argv();
        runner.gh_result(&account, true, &account_http().stdout, b"");
        let repo = repository_state_argv(policy);
        let repo_refs = repo.iter().map(String::as_str).collect::<Vec<_>>();
        runner.gh_sequence(
            &repo_refs,
            vec![absent_http(), absent_http(), absent_http()],
        );
        runner.git_sequence(
            &["config", "--get", "remote.origin.url"],
            vec![
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(false, b"", b""),
                command_result(true, policy.origin.as_bytes(), b""),
            ],
        );
        runner.git_ok(&["remote", "add", "origin", policy.origin], "");
        runner.git_ok(&["push", "-u", "origin", "main"], "");
        runner.gh_ok(
            &["repo", "create", "vnath99/R27_TEST_REPOSITORY", "--private"],
            "",
        );
        let preflight = store
            .preflight_with_test_policy(policy, policy.workspace, 100, &runner)
            .expect("committed preflight");
        let outcome = store
            .confirm_with_test_policy(
                policy,
                policy.project_id,
                &preflight.confirmation_token,
                101,
                &runner,
            )
            .expect("committed confirmation");
        assert!(outcome.stages.contains(&"PUSHED".into()));
        assert!(
            runner
                .calls
                .lock()
                .expect("calls")
                .iter()
                .any(|call| call == "git push -u origin main")
        );
    }

    #[test]
    fn branch_validation_rejects_unsafe_forms() {
        assert!(safe_branch("main"));
        assert!(!safe_branch(" main"));
        assert!(!safe_branch("main..other"));
        assert!(!safe_branch("-main"));
    }

    #[test]
    fn fixed_auth_and_repository_probes_do_not_accept_wrong_or_existing_identity() {
        let runner = FakeRunner::default();
        let executable = PathBuf::from("C:\\trusted\\gh.exe");
        runner.gh_ok(&["api", "user", "--jq", ".login"], "vnath99\n");
        assert!(verify_github_account(&runner, &executable).is_ok());
        runner.gh_ok(&["api", "user", "--jq", ".login"], "other-user\n");
        assert_eq!(
            verify_github_account(&runner, &executable),
            Err(GithubBootstrapFailure::GithubAuthWrongAccount)
        );

        let policy = approved_policy("BYOVD_DRIVER_PIPELINE").expect("fixed policy");
        let argv = repository_state_argv(policy);
        let query = argv.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(query[1], "repos/vnath99/BYOVD_DRIVER_PIPELINE");
        assert_eq!(query[2], "--include");
        let account = account_scope_argv();
        runner.gh_ok(
            &account,
            "HTTP/1.1 200 OK\r\nX-OAuth-Scopes: repo\r\n\r\n{\"login\":\"vnath99\"}",
        );
        runner.gh_ok(&query, "HTTP/1.1 404 Not Found\r\n\r\n{}");
        assert!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent).is_ok()
        );
        runner.gh_ok(&query, "HTTP/1.1 200 OK\r\n\r\n{\"id\":1,\"node_id\":\"R_opaque\",\"name\":\"BYOVD_DRIVER_PIPELINE\",\"owner\":{\"login\":\"vnath99\"}}");
        assert_eq!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent),
            Err(GithubBootstrapFailure::TargetRepositoryAlreadyExists)
        );
        assert!(
            runner
                .calls
                .lock()
                .expect("calls")
                .iter()
                .all(|call| !call.contains("--public") && !call.contains("push --force"))
        );
    }

    #[test]
    fn repository_probe_rejects_failed_or_ambiguous_rest_without_inferring_absence() {
        let runner = FakeRunner::default();
        let executable = PathBuf::from("C:\\trusted\\gh.exe");
        let policy = approved_policy("BUG_BOUNTY_RECON_PLATFORM").expect("fixed policy");
        let argv = repository_state_argv(policy);
        let query = argv.iter().map(String::as_str).collect::<Vec<_>>();
        let account = account_scope_argv();
        runner.gh_ok(
            &account,
            "HTTP/1.1 200 OK\r\nX-OAuth-Scopes: repo\r\n\r\n{\"login\":\"vnath99\"}",
        );
        runner.gh_fail(&query);
        assert_eq!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent),
            Err(GithubBootstrapFailure::GhHttpEnvelopeInvalid)
        );
        runner.gh_ok(&query, r#"{"data":{"repository":null},"errors":[]}"#);
        assert_eq!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent),
            Err(GithubBootstrapFailure::GhHttpEnvelopeInvalid)
        );
        runner.gh_ok(&query, r#"{"data":{"repository":{"id":null}}}"#);
        assert_eq!(
            prove_repository_state(&runner, &executable, policy, RepositoryState::Absent),
            Err(GithubBootstrapFailure::GhHttpEnvelopeInvalid)
        );
    }

    #[test]
    fn resolver_uses_fixed_priority_without_false_ambiguity() {
        let root = std::env::temp_dir().join(format!("catdesk-tool-slots-{}", Uuid::new_v4()));
        let workspace = root.join("workspace");
        let tools = root.join("tools");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::create_dir_all(&tools).expect("tools");
        let preferred = tools.join("git-cmd.exe");
        let fallback = tools.join("git-bin.exe");
        fs::write(&preferred, b"preferred").expect("preferred");
        fs::write(&fallback, b"fallback").expect("fallback");
        let selected = resolve_trusted_tool(
            TrustedToolKind::Git,
            &[preferred.clone(), fallback.clone()],
            &workspace,
        )
        .expect("preferred wins");
        assert_eq!(selected.slot, 0);
        fs::remove_file(&preferred).expect("remove preferred");
        let selected =
            resolve_trusted_tool(TrustedToolKind::Git, &[preferred, fallback], &workspace)
                .expect("fallback after missing preferred");
        assert_eq!(selected.slot, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolver_rejects_selected_workspace_or_unsafe_tool() {
        let root = std::env::temp_dir().join(format!("catdesk-tool-unsafe-{}", Uuid::new_v4()));
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let inside = workspace.join("git.exe");
        fs::write(&inside, b"hijack").expect("inside");
        assert_eq!(
            resolve_trusted_tool(TrustedToolKind::Git, &[inside], &workspace),
            Err(GithubBootstrapFailure::GitInsideWorkspace)
        );
        assert_eq!(
            resolve_trusted_tool(TrustedToolKind::Gh, &[workspace], &root),
            Err(GithubBootstrapFailure::GhUnsafeExecutable)
        );
        let _ = fs::remove_dir_all(root);
    }
}
