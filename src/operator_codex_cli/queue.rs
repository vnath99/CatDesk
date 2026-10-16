//! Explicit current-user operator queue for registered Codex projects.
//!
//! Task payloads and receipts live outside every Codex workspace. The
//! existing control registry owns the fair cursor and exact workspace lease;
//! this is same-user operator coordination, not hostile-worker isolation.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use uuid::Uuid;

use crate::delegated::autonomy_projects::{
    AutonomousProjectRegistryStoreV1, AutonomousProjectV1, AutonomousWorkspaceLeaseV1,
    GlobalScheduleDecisionV1, ScheduledProviderV1, resolve_registered_project_from_control_root,
};
use crate::delegated::autonomy_runtime::{
    acquire_project_writer_lock, fixed_current_user_catdesk_state_root,
    fixed_current_user_catdesk_state_root_path, inspect_project_writer_pending_marker,
    queue_writer_marker_sha256,
};
use crate::delegated::codex_cli::{
    CodexCliNativeStdoutEvidenceV1, prepare_resumed_native_turn_readback,
};
use crate::delegated::codex_model_policy::CodexCliTurnProfileEvidenceV1;
use crate::delegated::runtime::RuntimeError;

use super::{
    CodexScope, OperatorCodexAction, bounded_probe, codex_command,
    ensure_no_interrupted_managed_writer, ensure_same_project_binding, exact_thread,
    invocation_args, project_binding_sha256, registered_project, resolve_native_cli, valid_effort,
    valid_model,
};

mod advisory;
mod operator_review;
mod review_invite;
mod review_reader;

pub(crate) use review_reader::read_for_mcp as read_review_package_for_mcp;

const QUEUE_SCHEMA_VERSION: u32 = 1;
const MAX_TASK_BYTES: usize = 24 * 1024;
const MAX_JOB_BYTES: u64 = 64 * 1024;
const MAX_JOBS: usize = 512;
const MAX_DIR_ENTRIES: usize = 1024;
const MAX_RECEIPT_BYTES: u64 = 16 * 1024;
const MAX_REVIEW_BYTES: u64 = 64 * 1024;
const MAX_VERIFICATION_BYTES: u64 = 72 * 1024 * 1024;
const MAX_REVIEW_PACKAGE_BYTES: u64 = 76 * 1024 * 1024;
const MAX_VERIFY_STREAM_BYTES: usize = 16 * 1024 * 1024;
const VERIFY_COMMAND_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const NATIVE_STDOUT_EOF_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_NATIVE_STDOUT_LINE_BYTES: usize = 64 * 1024;
const MAX_GIT_STATUS_BYTES: usize = 1024 * 1024;
const MAX_GIT_DIFF_BYTES: usize = 16 * 1024 * 1024;
const MAX_GIT_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_GIT_FILES_BYTES: u64 = 16 * 1024 * 1024;
const MAX_GIT_PATHS: usize = 256;
const GIT_WITNESS_TIMEOUT: Duration = Duration::from_secs(30);
const PROFILE_INPUTS_TAGGED: &[u8] = b"H pyproject.toml\nH uv.lock\n";
const MAX_NATIVE_EXE_BYTES: u64 = 512 * 1024 * 1024;
// Expiry is an operator recovery signal only; the scheduler never reclaims
// this lease based on time. The window exceeds the maximum native turn bound.
const LEASE_SECONDS: u64 = 24 * 60 * 60;
const PROMPT_TIMEOUT: Duration = Duration::from_secs(30);
const KILL_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QueueAction {
    Preflight {
        scope: CodexScope,
    },
    Add {
        scope: CodexScope,
        task_id: String,
        task: String,
        model: Option<String>,
        effort: Option<String>,
    },
    Status,
    Receipt {
        receipt_id: String,
    },
    Review {
        receipt_id: String,
    },
    Verify {
        receipt_id: String,
    },
    ReviewPackage {
        receipt_id: String,
    },
    ReviewInvitePrepare {
        scope: CodexScope,
        receipt_id: String,
        expected_package_sha256: String,
    },
    AdvisoryReview {
        receipt_id: String,
    },
    OperatorReview {
        scope: CodexScope,
        receipt_id: String,
        expected_package_sha256: String,
        decision: String,
    },
    OperatorReviewReceipt {
        receipt_id: String,
    },
    ReviewDisclosureGrant {
        scope: CodexScope,
        receipt_id: String,
        expected_package_sha256: String,
    },
    ReviewDisclosureRenew {
        scope: CodexScope,
        receipt_id: String,
        expected_package_sha256: String,
    },
    ReviewDisclosureRevoke {
        project_id: String,
        receipt_id: String,
        expected_package_sha256: String,
    },
    ProfileBind {
        scope: CodexScope,
        expected_profile: String,
        new_profile: String,
        expected_binding_sha256: String,
        confirmed_quiescent_thread: String,
    },
    Dispatch,
    Pause {
        scope: CodexScope,
        task_id: String,
    },
    Resume {
        scope: CodexScope,
        task_id: String,
    },
}

pub(crate) fn parse_action(args: &[String]) -> Result<QueueAction, String> {
    let verb = args.get(3).map(String::as_str).ok_or_else(|| {
        "CODEX_QUEUE_USAGE: expected preflight, add, status, receipt, review, verify, review-package, review-invite-prepare, advisory-review, operator-review, operator-review-receipt, review-disclosure-grant, review-disclosure-renew, review-disclosure-revoke, profile-bind, dispatch, pause, or resume"
            .to_string()
    })?;
    if !matches!(
        verb,
        "preflight"
            | "add"
            | "status"
            | "receipt"
            | "review"
            | "verify"
            | "review-package"
            | "review-invite-prepare"
            | "advisory-review"
            | "operator-review"
            | "operator-review-receipt"
            | "review-disclosure-grant"
            | "review-disclosure-renew"
            | "review-disclosure-revoke"
            | "profile-bind"
            | "dispatch"
            | "pause"
            | "resume"
    ) {
        return Err("CODEX_QUEUE_USAGE: unsupported queue action".into());
    }
    let mut values = BTreeMap::new();
    let mut index = 4;
    while index < args.len() {
        let flag = args[index].as_str();
        if !matches!(
            flag,
            "--project-id"
                | "--workspace"
                | "--task-id"
                | "--task"
                | "--model"
                | "--effort"
                | "--receipt-id"
                | "--expected-package-sha256"
                | "--decision"
                | "--expected-profile"
                | "--new-profile"
                | "--expected-binding-sha256"
                | "--confirm-chat-quiescent"
        ) {
            return Err("CODEX_QUEUE_USAGE: unsupported option".into());
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| "CODEX_QUEUE_USAGE: an option requires a value".to_string())?;
        if value.is_empty()
            || value.len() > MAX_TASK_BYTES
            || values.insert(flag, value.clone()).is_some()
        {
            return Err("CODEX_QUEUE_USAGE: an option is empty, oversized, or repeated".into());
        }
        index += 2;
    }
    if matches!(verb, "status" | "dispatch") {
        if !values.is_empty() {
            return Err("CODEX_QUEUE_USAGE: status and dispatch take no options".into());
        }
        return Ok(if verb == "status" {
            QueueAction::Status
        } else {
            QueueAction::Dispatch
        });
    }
    if matches!(
        verb,
        "receipt"
            | "review"
            | "verify"
            | "review-package"
            | "advisory-review"
            | "operator-review-receipt"
    ) {
        let receipt_id = values
            .remove("--receipt-id")
            .ok_or("CODEX_QUEUE_USAGE: receipt read actions require --receipt-id")?;
        if !values.is_empty()
            || !Uuid::parse_str(&receipt_id).is_ok_and(|id| id.to_string() == receipt_id)
        {
            return Err(
                "CODEX_QUEUE_USAGE: receipt read actions require one canonical receipt UUID".into(),
            );
        }
        return Ok(match verb {
            "receipt" => QueueAction::Receipt { receipt_id },
            "review" => QueueAction::Review { receipt_id },
            "review-package" => QueueAction::ReviewPackage { receipt_id },
            "advisory-review" => QueueAction::AdvisoryReview { receipt_id },
            "operator-review-receipt" => QueueAction::OperatorReviewReceipt { receipt_id },
            _ => QueueAction::Verify { receipt_id },
        });
    }
    let project_id = values
        .remove("--project-id")
        .ok_or("CODEX_QUEUE_USAGE: --project-id is required")?;
    valid_slug(&project_id)?;
    let package_binding = if matches!(
        verb,
        "review-disclosure-grant"
            | "review-disclosure-renew"
            | "review-disclosure-revoke"
            | "review-invite-prepare"
            | "operator-review"
    ) {
        let receipt_id = values
            .remove("--receipt-id")
            .ok_or("CODEX_QUEUE_USAGE: package-bound action requires --receipt-id")?;
        let expected_package_sha256 = values
            .remove("--expected-package-sha256")
            .ok_or("CODEX_QUEUE_USAGE: package-bound action requires --expected-package-sha256")?;
        if !Uuid::parse_str(&receipt_id).is_ok_and(|id| id.to_string() == receipt_id)
            || !valid_sha256_hex(&expected_package_sha256)
            || expected_package_sha256 != expected_package_sha256.to_ascii_lowercase()
        {
            return Err("CODEX_QUEUE_USAGE: package-bound action requires canonical receipt UUID and lowercase package SHA-256".into());
        }
        Some((receipt_id, expected_package_sha256))
    } else {
        None
    };
    if verb == "review-disclosure-revoke" {
        if !values.is_empty() {
            return Err(
                "CODEX_QUEUE_USAGE: disclosure revoke takes no workspace or extra options".into(),
            );
        }
        let (receipt_id, expected_package_sha256) = package_binding
            .ok_or("CODEX_QUEUE_USAGE: disclosure revoke identity is unavailable")?;
        return Ok(QueueAction::ReviewDisclosureRevoke {
            project_id,
            receipt_id,
            expected_package_sha256,
        });
    }
    let workspace = PathBuf::from(
        values
            .remove("--workspace")
            .ok_or("CODEX_QUEUE_USAGE: --workspace is required")?,
    );
    if !workspace.is_absolute() {
        return Err("CODEX_SCOPE: --workspace must be absolute".into());
    }
    let scope = CodexScope {
        project_id,
        workspace,
    };
    if verb == "preflight" {
        if !values.is_empty() {
            return Err(
                "CODEX_QUEUE_USAGE: preflight takes only --project-id and --workspace".into(),
            );
        }
        return Ok(QueueAction::Preflight { scope });
    }
    if let Some((receipt_id, expected_package_sha256)) = package_binding {
        if verb == "operator-review" {
            let decision = values
                .remove("--decision")
                .ok_or("CODEX_QUEUE_USAGE: operator-review requires --decision")?;
            if !matches!(decision.as_str(), "ACCEPT" | "REJECT" | "INCONCLUSIVE")
                || !values.is_empty()
            {
                return Err("CODEX_QUEUE_USAGE: operator-review requires ACCEPT, REJECT, or INCONCLUSIVE and no extra options".into());
            }
            return Ok(QueueAction::OperatorReview {
                scope,
                receipt_id,
                expected_package_sha256,
                decision,
            });
        }
        if verb == "review-invite-prepare" {
            if !values.is_empty() {
                return Err(
                    "CODEX_QUEUE_USAGE: review invitation preparation takes no extra options"
                        .into(),
                );
            }
            return Ok(QueueAction::ReviewInvitePrepare {
                scope,
                receipt_id,
                expected_package_sha256,
            });
        }
        if !values.is_empty() {
            return Err("CODEX_QUEUE_USAGE: disclosure grant/renew takes no extra options".into());
        }
        return Ok(if verb == "review-disclosure-renew" {
            QueueAction::ReviewDisclosureRenew {
                scope,
                receipt_id,
                expected_package_sha256,
            }
        } else {
            QueueAction::ReviewDisclosureGrant {
                scope,
                receipt_id,
                expected_package_sha256,
            }
        });
    }
    if verb == "profile-bind" {
        let expected_profile = values
            .remove("--expected-profile")
            .ok_or("CODEX_QUEUE_USAGE: profile-bind requires --expected-profile")?;
        let new_profile = values
            .remove("--new-profile")
            .ok_or("CODEX_QUEUE_USAGE: profile-bind requires --new-profile")?;
        let expected_binding_sha256 = values
            .remove("--expected-binding-sha256")
            .ok_or("CODEX_QUEUE_USAGE: profile-bind requires --expected-binding-sha256")?;
        let confirmed_quiescent_thread = values
            .remove("--confirm-chat-quiescent")
            .ok_or("CODEX_QUEUE_USAGE: profile-bind requires --confirm-chat-quiescent with the exact bound thread ID")?;
        if !values.is_empty()
            || expected_profile != "rust_full"
            || !matches!(
                new_profile.as_str(),
                "python_uv_group_dev_v1" | "python_uv_extras_dev_lab_v1"
            )
            || !valid_sha256_hex(&expected_binding_sha256)
            || expected_binding_sha256 != expected_binding_sha256.to_ascii_lowercase()
            || !Uuid::parse_str(&confirmed_quiescent_thread)
                .is_ok_and(|id| id.to_string() == confirmed_quiescent_thread)
        {
            return Err("CODEX_QUEUE_USAGE: profile-bind requires an old rust_full binding hash, one fixed Python profile, and the exact bound thread assertion".into());
        }
        return Ok(QueueAction::ProfileBind {
            scope,
            expected_profile,
            new_profile,
            expected_binding_sha256,
            confirmed_quiescent_thread,
        });
    }
    let task_id = values
        .remove("--task-id")
        .ok_or("CODEX_QUEUE_USAGE: --task-id is required")?;
    valid_slug(&task_id)?;
    if verb == "pause" || verb == "resume" {
        if !values.is_empty() {
            return Err("CODEX_QUEUE_USAGE: pause and resume only accept scope and task ID".into());
        }
        return Ok(if verb == "pause" {
            QueueAction::Pause { scope, task_id }
        } else {
            QueueAction::Resume { scope, task_id }
        });
    }
    let task = values
        .remove("--task")
        .ok_or("CODEX_QUEUE_USAGE: add requires --task")?;
    if task.trim().is_empty() || task.contains('\0') || task.len() > MAX_TASK_BYTES {
        return Err("CODEX_QUEUE_USAGE: task is empty or invalid".into());
    }
    let model = values.remove("--model");
    let effort = values.remove("--effort");
    if model.as_deref().is_some_and(|value| !valid_model(value))
        || effort.as_deref().is_some_and(|value| !valid_effort(value))
        || !values.is_empty()
    {
        return Err("CODEX_QUEUE_USAGE: invalid model, effort, or option".into());
    }
    Ok(QueueAction::Add {
        scope,
        task_id,
        task,
        model,
        effort,
    })
}

fn valid_slug(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err("CODEX_QUEUE_USAGE: project and task IDs must be conservative slugs".into())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueBinding {
    project_id: String,
    workspace: PathBuf,
    git_identity: String,
    verification_profile: String,
    thread_id: String,
    target_sha256: Option<String>,
}

impl QueueBinding {
    fn from_project(project: &AutonomousProjectV1) -> Result<Self, String> {
        Ok(Self {
            project_id: project.project_id.clone(),
            workspace: project.workspace.clone(),
            git_identity: project.git_identity.clone(),
            verification_profile: project.verification_profile.clone(),
            thread_id: exact_thread(project)?.into(),
            target_sha256: project.chatgpt_target_sha256.clone(),
        })
    }
    fn matches(&self, project: &AutonomousProjectV1) -> bool {
        Self::from_project(project).as_ref() == Ok(self)
    }
    fn scope(&self) -> CodexScope {
        CodexScope {
            project_id: self.project_id.clone(),
            workspace: self.workspace.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum QueueState {
    Ready,
    Paused,
    Claimed,
    Unknown,
    NativeExitedZero,
    Failed,
    Attention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueAttempt {
    attempt_id: String,
    lease: AutonomousWorkspaceLeaseV1,
    started_at_unix: u64,
    // Older attempts remain readable, but have no queue marker binding that
    // future recovery code may use as evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    writer_marker_sha256: Option<String>,
}

impl QueueAttempt {
    fn marker_binding_is_valid(&self) -> bool {
        queue_writer_marker_sha256(&self.attempt_id)
            .ok()
            .is_some_and(|digest| self.writer_marker_sha256.as_deref() == Some(digest.as_str()))
    }

    fn matches_writer_marker(&self, observed_sha256: Option<&str>) -> bool {
        self.marker_binding_is_valid()
            && observed_sha256
                .is_some_and(|observed| self.writer_marker_sha256.as_deref() == Some(observed))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueJob {
    schema_version: u32,
    task_id: String,
    binding: QueueBinding,
    prompt: String,
    prompt_sha256: String,
    model: Option<String>,
    effort: Option<String>,
    state: QueueState,
    created_at_unix: u64,
    attempt: Option<QueueAttempt>,
    receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_candidate_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verification_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_package_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    advisory_outcome_sha256: Option<String>,
}

impl QueueJob {
    fn new(
        task_id: String,
        project: &AutonomousProjectV1,
        prompt: String,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<Self, String> {
        let prompt_sha256 = format!("{:x}", Sha256::digest(prompt.as_bytes()));
        Ok(Self {
            schema_version: QUEUE_SCHEMA_VERSION,
            task_id,
            binding: QueueBinding::from_project(project)?,
            prompt,
            prompt_sha256,
            model,
            effort,
            state: QueueState::Ready,
            created_at_unix: unix_now()?,
            attempt: None,
            receipt_id: None,
            review_candidate_sha256: None,
            verification_sha256: None,
            review_package_sha256: None,
            advisory_outcome_sha256: None,
        })
    }
    fn same_request(&self, other: &Self) -> bool {
        self.task_id == other.task_id
            && self.binding == other.binding
            && self.prompt == other.prompt
            && self.model == other.model
            && self.effort == other.effort
    }
    fn summary(&self) -> Value {
        json!({
            "projectId": self.binding.project_id, "taskId": self.task_id,
            "workspace": self.binding.workspace, "threadId": self.binding.thread_id,
            "targetSha256": self.binding.target_sha256,
            "gitIdentitySha256": format!("{:x}", Sha256::digest(self.binding.git_identity.as_bytes())),
            "verificationProfile": self.binding.verification_profile,
            "promptSha256": self.prompt_sha256, "state": self.state,
            "createdAtUnix": self.created_at_unix,
            "attemptId": self.attempt.as_ref().map(|attempt| &attempt.attempt_id),
            "receiptId": self.receipt_id,
            "reviewCandidateSha256": self.review_candidate_sha256,
            "verificationSha256": self.verification_sha256,
            "reviewPackageSha256": self.review_package_sha256,
            "advisoryOutcomeSha256": self.advisory_outcome_sha256,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueReceipt {
    schema_version: u32,
    receipt_id: String,
    attempt_id: String,
    project_id: String,
    task_id: String,
    workspace: PathBuf,
    thread_id: String,
    target_sha256: Option<String>,
    prompt_sha256: String,
    requested_model: Option<String>,
    requested_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    actual_profile: Option<CodexCliTurnProfileEvidenceV1>,
    native_executable: Option<PathBuf>,
    native_version: Option<String>,
    native_executable_sha256: Option<String>,
    evidence_level: String,
    exit_code: Option<i32>,
    child_exit_observed: bool,
    binding_still_matches: bool,
    outcome: String,
    recorded_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitChangedPath {
    path: String,
    kind: String,
    sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitReviewWitness {
    head_commit: Option<String>,
    status_sha256: String,
    tracked_diff_sha256: String,
    #[serde(default)]
    index_diff_sha256: String,
    changed_paths: Vec<GitChangedPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueReviewCandidate {
    schema_version: u32,
    receipt_id: String,
    receipt_sha256: String,
    project_id: String,
    task_id: String,
    workspace: PathBuf,
    thread_id: String,
    prompt_sha256: String,
    requested_model: Option<String>,
    requested_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    actual_profile: Option<CodexCliTurnProfileEvidenceV1>,
    before: GitReviewWitness,
    after: GitReviewWitness,
    baseline_dirty: bool,
    head_changed: bool,
    changed_during_turn: Vec<String>,
    review_status: String,
    verification_status: String,
    wake_ready: bool,
}

impl QueueReviewCandidate {
    fn from_observed_turn(
        job: &QueueJob,
        receipt: &QueueReceipt,
        receipt_sha256: String,
        before: GitReviewWitness,
        after: GitReviewWitness,
    ) -> Self {
        let (baseline_dirty, head_changed, changed_during_turn) =
            derived_review_fields(&before, &after);
        Self {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: receipt.receipt_id.clone(),
            receipt_sha256,
            project_id: job.binding.project_id.clone(),
            task_id: job.task_id.clone(),
            workspace: job.binding.workspace.clone(),
            thread_id: job.binding.thread_id.clone(),
            prompt_sha256: job.prompt_sha256.clone(),
            requested_model: job.model.clone(),
            requested_effort: job.effort.clone(),
            actual_profile: receipt.actual_profile.clone(),
            baseline_dirty,
            head_changed,
            changed_during_turn,
            before,
            after,
            review_status: "UNREVIEWED".into(),
            verification_status: "NOT_VERIFIED_BY_CATDESK".into(),
            wake_ready: false,
        }
    }
}

fn derived_review_fields(
    before: &GitReviewWitness,
    after: &GitReviewWitness,
) -> (bool, bool, Vec<String>) {
    let before_paths = before
        .changed_paths
        .iter()
        .map(|item| (item.path.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let after_paths = after
        .changed_paths
        .iter()
        .map(|item| (item.path.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let changed_during_turn = before_paths
        .keys()
        .chain(after_paths.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| before_paths.get(path) != after_paths.get(path))
        .map(str::to_owned)
        .collect();
    (
        before.head_commit.is_none() || !before.changed_paths.is_empty(),
        before.head_commit != after.head_commit,
        changed_during_turn,
    )
}

/// One create-once, same-user local verification artifact. A source snapshot
/// and its command outcomes share one file, so a torn write can never expose
/// a passing manifest with missing patch or file bytes. This is not an
/// independent review or an authorization to publish a Wake event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueVerificationFile {
    path: String,
    kind: String,
    sha256: Option<String>,
    content_base64: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueVerificationCommand {
    argv: Vec<String>,
    executable: PathBuf,
    executable_sha256: String,
    environment_sha256: String,
    exit_code: Option<i32>,
    elapsed_ms: u64,
    stdout_sha256: String,
    stderr_sha256: String,
    stdout_bytes: u64,
    stderr_bytes: u64,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueVerification {
    schema_version: u32,
    receipt_id: String,
    receipt_sha256: String,
    review_candidate_sha256: String,
    project_id: String,
    task_id: String,
    workspace: PathBuf,
    thread_id: String,
    verification_profile: String,
    actual_profile: Option<CodexCliTurnProfileEvidenceV1>,
    git_witness: GitReviewWitness,
    patch_sha256: String,
    patch_base64: String,
    index_patch_sha256: String,
    index_patch_base64: String,
    files: Vec<QueueVerificationFile>,
    commands: Vec<QueueVerificationCommand>,
    status: String,
    recorded_at_unix: u64,
    independent_review: String,
    wake_ready: bool,
}

impl QueueVerification {
    fn summary(&self, digest: &str) -> Value {
        let process_evidence = match verification_profile(&self.verification_profile) {
            Ok(profile) if profile.tool == VerificationTool::Uv => "DIRECT_UV_PARENT_EXIT_ONLY",
            _ => "DIRECT_CARGO_PARENT_EXIT_ONLY",
        };
        json!({
            "receiptId":self.receipt_id, "projectId":self.project_id,
            "taskId":self.task_id, "workspace":self.workspace,
            "threadId":self.thread_id, "verificationProfile":self.verification_profile,
            "actualProfile":self.actual_profile,
            "status":self.status, "processEvidence":process_evidence,
            "independentReview":self.independent_review,
            "wakeReady":self.wake_ready, "artifactSha256":digest,
            "gitWitness":self.git_witness, "patchSha256":self.patch_sha256,
            "indexPatchSha256":self.index_patch_sha256,
            "files":self.files.iter().map(|file| json!({
                "path":file.path,"kind":file.kind,"sha256":file.sha256,
            })).collect::<Vec<_>>(),
            "commands":self.commands,
            "recordedAtUnix":self.recorded_at_unix,
        })
    }
}

/// A self-contained, create-once copy of the evidence a separate reviewer
/// may inspect later. The source bytes and command results are copied from
/// the already validated verification artifact. Packaging does not perform
/// review or grant Wake authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueReviewPackage {
    schema_version: u32,
    receipt_id: String,
    receipt_sha256: String,
    review_candidate_sha256: String,
    verification_sha256: String,
    prompt: String,
    receipt: QueueReceipt,
    candidate: QueueReviewCandidate,
    verification: QueueVerification,
    recorded_at_unix: u64,
    independent_review: String,
    wake_ready: bool,
}

impl QueueReviewPackage {
    fn summary(&self, digest: &str) -> Value {
        json!({
            "receiptId":self.receipt_id,
            "receiptSha256":self.receipt_sha256,
            "reviewCandidateSha256":self.review_candidate_sha256,
            "verificationSha256":self.verification_sha256,
            "artifactSha256":digest,
            "projectId":self.receipt.project_id,
            "taskId":self.receipt.task_id,
            "workspace":self.receipt.workspace,
            "threadId":self.receipt.thread_id,
            "actualProfile":self.receipt.actual_profile,
            "verificationProfile":self.verification.verification_profile,
            "verificationStatus":self.verification.status,
            "gitWitness":self.verification.git_witness,
            "patchSha256":self.verification.patch_sha256,
            "indexPatchSha256":self.verification.index_patch_sha256,
            "files":self.verification.files.iter().map(|file| json!({
                "path":file.path,"kind":file.kind,"sha256":file.sha256,
            })).collect::<Vec<_>>(),
            "commands":self.verification.commands,
            "recordedAtUnix":self.recorded_at_unix,
            "independentReview":self.independent_review,
            "wakeReady":self.wake_ready,
        })
    }
}

struct QueueStore {
    root: PathBuf,
}

impl QueueStore {
    fn namespaced_path(
        control: &AutonomousProjectV1,
        project_workspace: &Path,
    ) -> Result<PathBuf, String> {
        let workspace = project_workspace
            .canonicalize()
            .map_err(|_| "CODEX_QUEUE: project workspace is unavailable".to_string())?;
        let planned_root = fixed_current_user_catdesk_state_root_path()
            .map_err(|_| "CODEX_QUEUE: fixed current-user state root is unavailable".to_string())?;
        reject_overlap(&planned_root, &workspace)?;
        reject_overlap(&planned_root, &control.workspace)?;
        let identity = format!("{}\0{}", control.workspace.display(), control.git_identity);
        let namespace = format!("{:x}", Sha256::digest(identity.as_bytes()));
        Ok(planned_root.join("OperatorCodexQueue").join(namespace))
    }
    fn open_for_operator(
        control: &AutonomousProjectV1,
        project_workspace: &Path,
    ) -> Result<Self, String> {
        let planned = Self::namespaced_path(control, project_workspace)?;
        let workspace = project_workspace
            .canonicalize()
            .map_err(|_| "CODEX_QUEUE: project workspace is unavailable".to_string())?;
        let registry = scheduler(&control.workspace)?
            .load_registry()
            .map_err(|_| "CODEX_QUEUE: control registry is invalid".to_string())?;
        let state_root = planned
            .parent()
            .and_then(Path::parent)
            .ok_or("CODEX_QUEUE: queue namespace is invalid")?;
        for registered in &registry.projects {
            reject_overlap(state_root, &registered.workspace)?;
        }
        let user_root = fixed_current_user_catdesk_state_root()
            .map_err(|_| "CODEX_QUEUE: fixed current-user state root is unavailable".to_string())?;
        reject_overlap(&user_root, &workspace)?;
        reject_overlap(&user_root, &control.workspace)?;
        let queue_parent = user_root.join("OperatorCodexQueue");
        checked_dir(&queue_parent)?;
        let namespace = planned
            .file_name()
            .ok_or("CODEX_QUEUE: queue namespace is invalid")?;
        Self::open_at(queue_parent.join(namespace))
    }
    fn open_read_only(control: &AutonomousProjectV1) -> Result<Option<Self>, String> {
        let planned = Self::namespaced_path(control, &control.workspace)?;
        let user_root = planned
            .parent()
            .and_then(Path::parent)
            .ok_or("CODEX_QUEUE: queue namespace is invalid")?;
        let queue_parent = planned
            .parent()
            .ok_or("CODEX_QUEUE: queue namespace is invalid")?;
        for path in [user_root, queue_parent, planned.as_path()] {
            let metadata = match fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err("CODEX_QUEUE: queue state is unreadable".into()),
            };
            if !metadata.is_dir() || redirected(&metadata) {
                return Err("CODEX_QUEUE: queue state is redirected or unsafe".into());
            }
        }
        for child in ["jobs", "receipts"] {
            let metadata = fs::symlink_metadata(planned.join(child))
                .map_err(|_| "CODEX_QUEUE: queue state is incomplete".to_string())?;
            if !metadata.is_dir() || redirected(&metadata) {
                return Err("CODEX_QUEUE: queue state is redirected or unsafe".into());
            }
        }
        Ok(Some(Self { root: planned }))
    }
    fn open_at(root: PathBuf) -> Result<Self, String> {
        checked_dir(&root)?;
        checked_dir(&root.join("jobs"))?;
        checked_dir(&root.join("receipts"))?;
        Ok(Self { root })
    }
    fn lock(&self) -> Result<File, String> {
        let path = self.root.join("queue.lock");
        checked_file_if_present(&path, 0)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        nofollow(&mut options);
        let file = options
            .open(&path)
            .map_err(|_| "CODEX_QUEUE: queue lock is unavailable")?;
        let opened = file
            .metadata()
            .map_err(|_| "CODEX_QUEUE: queue lock is unreadable")?;
        let observed =
            fs::symlink_metadata(&path).map_err(|_| "CODEX_QUEUE: queue lock is unreadable")?;
        if !opened.is_file()
            || opened.len() != 0
            || redirected(&opened)
            || !observed.is_file()
            || redirected(&observed)
            || !same_open_file_identity(&file, &path, &opened, &observed)
        {
            return Err("CODEX_QUEUE: queue lock is redirected or changed".into());
        }
        file.try_lock()
            .map_err(|_| "CODEX_QUEUE: another queue action is changing state".to_string())?;
        Ok(file)
    }
    fn job_path(&self, project_id: &str, task_id: &str) -> PathBuf {
        let digest = Sha256::digest(format!("{project_id}\0{task_id}").as_bytes());
        self.root.join("jobs").join(format!("{digest:x}.json"))
    }
    fn load_job(&self, project_id: &str, task_id: &str) -> Result<Option<QueueJob>, String> {
        let path = self.job_path(project_id, task_id);
        let Some(bytes) = read_regular_bounded(&path, MAX_JOB_BYTES)? else {
            return Ok(None);
        };
        let job: QueueJob = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE: a job record is corrupt".to_string())?;
        validate_job(&job)?;
        if job.binding.project_id != project_id || job.task_id != task_id {
            return Err("CODEX_QUEUE: job path and identity disagree".into());
        }
        Ok(Some(job))
    }
    fn jobs(&self) -> Result<Vec<QueueJob>, String> {
        let mut jobs = Vec::new();
        let mut identities = BTreeSet::new();
        let mut entries = 0;
        for entry in fs::read_dir(self.root.join("jobs"))
            .map_err(|_| "CODEX_QUEUE: jobs directory is unavailable")?
        {
            entries += 1;
            if entries > MAX_DIR_ENTRIES {
                return Err("CODEX_QUEUE: jobs directory entry limit reached".into());
            }
            let entry = entry.map_err(|_| "CODEX_QUEUE: jobs directory is unreadable")?;
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "tmp") {
                continue;
            }
            if jobs.len() >= MAX_JOBS {
                return Err("CODEX_QUEUE: job limit reached".into());
            }
            let Some(bytes) = read_regular_bounded(&path, MAX_JOB_BYTES)? else {
                continue;
            };
            let job: QueueJob = serde_json::from_slice(&bytes)
                .map_err(|_| "CODEX_QUEUE: a job record is corrupt".to_string())?;
            validate_job(&job)?;
            if path != self.job_path(&job.binding.project_id, &job.task_id)
                || !identities.insert((job.binding.project_id.clone(), job.task_id.clone()))
            {
                return Err("CODEX_QUEUE: job filename or identity is inconsistent".into());
            }
            jobs.push(job);
        }
        jobs.sort_by(|left, right| {
            (
                left.created_at_unix,
                &left.binding.project_id,
                &left.task_id,
            )
                .cmp(&(
                    right.created_at_unix,
                    &right.binding.project_id,
                    &right.task_id,
                ))
        });
        Ok(jobs)
    }
    fn save_job(&self, job: &QueueJob) -> Result<(), String> {
        validate_job(job)?;
        let path = self.job_path(&job.binding.project_id, &job.task_id);
        checked_file_if_present(&path, MAX_JOB_BYTES)?;
        let bytes =
            serde_json::to_vec_pretty(job).map_err(|_| "CODEX_QUEUE: job could not be encoded")?;
        if bytes.len().saturating_add(1) as u64 > MAX_JOB_BYTES {
            return Err("CODEX_QUEUE: job record is oversized".into());
        }
        atomic_write(&path, &bytes)
    }
    fn write_receipt(&self, receipt: &QueueReceipt) -> Result<(), String> {
        let path = self
            .root
            .join("receipts")
            .join(format!("{}.json", receipt.receipt_id));
        let bytes = serde_json::to_vec_pretty(receipt)
            .map_err(|_| "CODEX_QUEUE: receipt could not be encoded")?;
        if bytes.len().saturating_add(1) as u64 > MAX_RECEIPT_BYTES {
            return Err("CODEX_QUEUE: receipt record is oversized".into());
        }
        create_new_record(&path, &bytes)
    }
    fn persisted_receipt_sha256(&self, receipt: &QueueReceipt) -> Result<String, String> {
        let bytes = read_regular_bounded(
            &self
                .root
                .join("receipts")
                .join(format!("{}.json", receipt.receipt_id)),
            MAX_RECEIPT_BYTES,
        )?
        .ok_or("CODEX_QUEUE_REVIEW: process receipt is unavailable")?;
        let observed: QueueReceipt = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE_REVIEW: process receipt is corrupt")?;
        if &observed != receipt {
            return Err("CODEX_QUEUE_REVIEW: process receipt changed before review capture".into());
        }
        Ok(format!("{:x}", Sha256::digest(&bytes)))
    }
    fn review_path(&self, receipt_id: &str) -> PathBuf {
        self.root
            .join("receipts")
            .join(format!("{receipt_id}.review.json"))
    }
    fn verification_lock(&self, receipt_id: &str) -> Result<File, String> {
        let path = self
            .root
            .join("receipts")
            .join(format!("{receipt_id}.verification.lock"));
        checked_file_if_present(&path, 0)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        nofollow(&mut options);
        let file = options
            .open(&path)
            .map_err(|_| "CODEX_QUEUE_VERIFY: receipt lock is unavailable")?;
        let opened = file
            .metadata()
            .map_err(|_| "CODEX_QUEUE_VERIFY: receipt lock is unreadable")?;
        let observed = fs::symlink_metadata(&path)
            .map_err(|_| "CODEX_QUEUE_VERIFY: receipt lock is unreadable")?;
        if !opened.is_file()
            || opened.len() != 0
            || redirected(&opened)
            || !observed.is_file()
            || redirected(&observed)
            || !same_open_file_identity(&file, &path, &opened, &observed)
        {
            return Err("CODEX_QUEUE_VERIFY: receipt lock changed or is unsafe".into());
        }
        file.try_lock()
            .map_err(|_| "CODEX_QUEUE_VERIFY: this receipt is already being verified")?;
        Ok(file)
    }
    fn write_review_candidate(&self, candidate: &QueueReviewCandidate) -> Result<String, String> {
        let bytes = serde_json::to_vec_pretty(candidate)
            .map_err(|_| "CODEX_QUEUE_REVIEW: candidate could not be encoded")?;
        if bytes.len().saturating_add(1) as u64 > MAX_REVIEW_BYTES {
            return Err("CODEX_QUEUE_REVIEW: candidate is oversized".into());
        }
        let mut persisted = bytes.clone();
        persisted.push(b'\n');
        let digest = format!("{:x}", Sha256::digest(&persisted));
        create_new_record(&self.review_path(&candidate.receipt_id), &bytes)?;
        Ok(digest)
    }
    fn read_review_candidate(
        &self,
        receipt: &QueueReceipt,
    ) -> Result<QueueReviewCandidate, String> {
        let receipt_bytes = read_regular_bounded(
            &self
                .root
                .join("receipts")
                .join(format!("{}.json", receipt.receipt_id)),
            MAX_RECEIPT_BYTES,
        )?
        .ok_or("CODEX_QUEUE_REVIEW: process receipt is unavailable")?;
        let bytes = read_regular_bounded(&self.review_path(&receipt.receipt_id), MAX_REVIEW_BYTES)?
            .ok_or("CODEX_QUEUE_REVIEW: bound candidate is unavailable")?;
        let candidate: QueueReviewCandidate = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE_REVIEW: candidate is corrupt".to_string())?;
        let job = self
            .load_job(&receipt.project_id, &receipt.task_id)?
            .ok_or("CODEX_QUEUE_REVIEW: bound job is unavailable")?;
        let observed_sha = format!("{:x}", Sha256::digest(&bytes));
        let (baseline_dirty, head_changed, changed_during_turn) =
            derived_review_fields(&candidate.before, &candidate.after);
        if candidate.schema_version != QUEUE_SCHEMA_VERSION
            || job.review_candidate_sha256.as_deref() != Some(observed_sha.as_str())
            || candidate.receipt_id != receipt.receipt_id
            || candidate.receipt_sha256 != format!("{:x}", Sha256::digest(&receipt_bytes))
            || candidate.project_id != receipt.project_id
            || candidate.task_id != receipt.task_id
            || candidate.workspace != receipt.workspace
            || candidate.thread_id != receipt.thread_id
            || candidate.prompt_sha256 != receipt.prompt_sha256
            || candidate.requested_model != receipt.requested_model
            || candidate.requested_effort != receipt.requested_effort
            || candidate.actual_profile != receipt.actual_profile
            || candidate.baseline_dirty != baseline_dirty
            || candidate.head_changed != head_changed
            || candidate.changed_during_turn != changed_during_turn
            || candidate.review_status != "UNREVIEWED"
            || candidate.verification_status != "NOT_VERIFIED_BY_CATDESK"
            || candidate.wake_ready
            || receipt.outcome != "NATIVE_EXIT_ZERO"
            || !receipt.child_exit_observed
            || !receipt.binding_still_matches
        {
            return Err("CODEX_QUEUE_REVIEW: candidate and process receipt disagree".into());
        }
        Ok(candidate)
    }
    fn verification_path(&self, receipt_id: &str) -> PathBuf {
        self.root
            .join("receipts")
            .join(format!("{receipt_id}.verification.json"))
    }
    fn write_verification(&self, verification: &QueueVerification) -> Result<String, String> {
        let bytes = serde_json::to_vec_pretty(verification)
            .map_err(|_| "CODEX_QUEUE_VERIFY: artifact could not be encoded")?;
        if bytes.len().saturating_add(1) as u64 > MAX_VERIFICATION_BYTES {
            return Err("CODEX_QUEUE_VERIFY: artifact is oversized".into());
        }
        let mut persisted = bytes.clone();
        persisted.push(b'\n');
        let digest = format!("{:x}", Sha256::digest(&persisted));
        create_new_record(&self.verification_path(&verification.receipt_id), &bytes)?;
        Ok(digest)
    }
    fn read_verification(
        &self,
        receipt: &QueueReceipt,
        candidate: &QueueReviewCandidate,
    ) -> Result<Option<(QueueVerification, String)>, String> {
        let path = self.verification_path(&receipt.receipt_id);
        let Some(bytes) = read_regular_bounded(&path, MAX_VERIFICATION_BYTES)? else {
            let job = self
                .load_job(&receipt.project_id, &receipt.task_id)?
                .ok_or("CODEX_QUEUE_VERIFY: bound job is unavailable")?;
            return if job.verification_sha256.is_none() {
                Ok(None)
            } else {
                Err("CODEX_QUEUE_VERIFY: bound artifact is missing".into())
            };
        };
        let verification: QueueVerification = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE_VERIFY: artifact is corrupt")?;
        let job = self
            .load_job(&receipt.project_id, &receipt.task_id)?
            .ok_or("CODEX_QUEUE_VERIFY: bound job is unavailable")?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let patch = BASE64
            .decode(verification.patch_base64.as_bytes())
            .map_err(|_| "CODEX_QUEUE_VERIFY: patch encoding is invalid")?;
        let index_patch = BASE64
            .decode(verification.index_patch_base64.as_bytes())
            .map_err(|_| "CODEX_QUEUE_VERIFY: staged patch encoding is invalid")?;
        let mut total_file_bytes = 0u64;
        let files_valid = verification
            .files
            .iter()
            .zip(candidate.after.changed_paths.iter())
            .all(|(file, changed)| {
                changed.path == file.path
                    && changed.kind == file.kind
                    && changed.sha256 == file.sha256
                    && match (&file.content_base64, &file.sha256) {
                        (Some(content), Some(sha)) => {
                            BASE64.decode(content.as_bytes()).ok().is_some_and(|bytes| {
                                total_file_bytes =
                                    total_file_bytes.saturating_add(bytes.len() as u64);
                                bytes.len() as u64 <= MAX_GIT_FILE_BYTES
                                    && total_file_bytes <= MAX_GIT_FILES_BYTES
                                    && format!("{:x}", Sha256::digest(bytes)) == *sha
                            })
                        }
                        (None, None) => true,
                        _ => false,
                    }
            });
        let expected_commands = verification_profile(&verification.verification_profile)?.commands;
        let command_status_valid = verification.commands.len() == expected_commands.len()
            && verification
                .commands
                .iter()
                .zip(expected_commands.iter())
                .all(|(observed, expected)| {
                    observed
                        .argv
                        .iter()
                        .map(String::as_str)
                        .eq(expected.iter().copied())
                })
            && verification.commands.iter().all(|command| {
                valid_sha256_hex(&command.executable_sha256)
                    && valid_sha256_hex(&command.environment_sha256)
                    && valid_sha256_hex(&command.stdout_sha256)
                    && valid_sha256_hex(&command.stderr_sha256)
                    && command.stdout_bytes <= MAX_VERIFY_STREAM_BYTES as u64
                    && command.stderr_bytes <= MAX_VERIFY_STREAM_BYTES as u64
                    && ((command.status == "EXIT_ZERO_PARENT_OBSERVED"
                        && command.exit_code == Some(0))
                        || (command.status == "EXIT_NONZERO_PARENT_OBSERVED"
                            && command.exit_code.is_some_and(|code| code != 0))
                        || (command.status == "EXIT_SIGNALLED_PARENT_OBSERVED"
                            && command.exit_code.is_none()))
            })
            && ((verification.status == "LOCAL_COMMANDS_EXITED_ZERO_SOURCE_RECHECKED"
                && verification
                    .commands
                    .iter()
                    .all(|command| command.status == "EXIT_ZERO_PARENT_OBSERVED"))
                || (verification.status == "LOCAL_COMMAND_FAILURE"
                    && verification
                        .commands
                        .iter()
                        .any(|command| command.status != "EXIT_ZERO_PARENT_OBSERVED")));
        if verification.schema_version != QUEUE_SCHEMA_VERSION
            || job.verification_sha256.as_deref() != Some(digest.as_str())
            || job.state != QueueState::NativeExitedZero
            || job.receipt_id.as_deref() != Some(receipt.receipt_id.as_str())
            || candidate.baseline_dirty
            || candidate.head_changed
            || candidate.before.head_commit.is_none()
            || candidate.changed_during_turn.is_empty()
            || candidate.changed_during_turn.len() != candidate.after.changed_paths.len()
            || !actual_profile_matches_receipt(receipt)
            || verification.receipt_id != receipt.receipt_id
            || verification.receipt_sha256 != self.persisted_receipt_sha256(receipt)?
            || verification.review_candidate_sha256
                != job.review_candidate_sha256.as_deref().unwrap_or_default()
            || verification.project_id != receipt.project_id
            || verification.task_id != receipt.task_id
            || verification.workspace != receipt.workspace
            || verification.thread_id != receipt.thread_id
            || verification.verification_profile != job.binding.verification_profile
            || verification.actual_profile != receipt.actual_profile
            || verification.git_witness != candidate.after
            || patch.len() > MAX_GIT_DIFF_BYTES
            || index_patch.len() > MAX_GIT_DIFF_BYTES
            || format!("{:x}", Sha256::digest(&patch)) != verification.patch_sha256
            || verification.patch_sha256 != candidate.after.tracked_diff_sha256
            || format!("{:x}", Sha256::digest(&index_patch)) != verification.index_patch_sha256
            || verification.index_patch_sha256 != candidate.after.index_diff_sha256
            || verification.files.len() != candidate.after.changed_paths.len()
            || !files_valid
            || !command_status_valid
            || verification.independent_review != "PENDING"
            || verification.wake_ready
            || verification.recorded_at_unix == 0
        {
            return Err("CODEX_QUEUE_VERIFY: artifact and bound source disagree".into());
        }
        Ok(Some((verification, digest)))
    }
    fn review_package_path(&self, receipt_id: &str) -> PathBuf {
        self.root
            .join("receipts")
            .join(format!("{receipt_id}.review-package.json"))
    }
    fn write_review_package(&self, package: &QueueReviewPackage) -> Result<String, String> {
        let bytes = serde_json::to_vec_pretty(package)
            .map_err(|_| "CODEX_QUEUE_REVIEW_PACKAGE: package could not be encoded")?;
        if bytes.len().saturating_add(1) as u64 > MAX_REVIEW_PACKAGE_BYTES {
            return Err("CODEX_QUEUE_REVIEW_PACKAGE: package is oversized".into());
        }
        let mut persisted = bytes.clone();
        persisted.push(b'\n');
        let digest = format!("{:x}", Sha256::digest(&persisted));
        create_new_record(&self.review_package_path(&package.receipt_id), &bytes)?;
        Ok(digest)
    }
    fn read_review_package(
        &self,
        receipt: &QueueReceipt,
        candidate: &QueueReviewCandidate,
        verification: &QueueVerification,
        verification_sha256: &str,
    ) -> Result<Option<(QueueReviewPackage, String)>, String> {
        let job = self
            .load_job(&receipt.project_id, &receipt.task_id)?
            .ok_or("CODEX_QUEUE_REVIEW_PACKAGE: bound job is unavailable")?;
        let bytes = read_regular_bounded(
            &self.review_package_path(&receipt.receipt_id),
            MAX_REVIEW_PACKAGE_BYTES,
        )?;
        let Some(bytes) = bytes else {
            return if job.review_package_sha256.is_none() {
                Ok(None)
            } else {
                Err("CODEX_QUEUE_REVIEW_PACKAGE: bound package is missing".into())
            };
        };
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let package: QueueReviewPackage = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE_REVIEW_PACKAGE: package is corrupt")?;
        if package.schema_version != QUEUE_SCHEMA_VERSION
            || job.review_package_sha256.as_deref() != Some(digest.as_str())
            || job.state != QueueState::NativeExitedZero
            || job.receipt_id.as_deref() != Some(receipt.receipt_id.as_str())
            || job.review_candidate_sha256.as_deref()
                != Some(package.review_candidate_sha256.as_str())
            || job.verification_sha256.as_deref() != Some(verification_sha256)
            || package.receipt_id != receipt.receipt_id
            || package.receipt_sha256 != self.persisted_receipt_sha256(receipt)?
            || package.verification_sha256 != verification_sha256
            || package.prompt != job.prompt
            || package.prompt.is_empty()
            || package.prompt.len() > MAX_TASK_BYTES
            || format!("{:x}", Sha256::digest(package.prompt.as_bytes())) != receipt.prompt_sha256
            || package.candidate.prompt_sha256 != receipt.prompt_sha256
            || &package.receipt != receipt
            || &package.candidate != candidate
            || &package.verification != verification
            || verification.status != "LOCAL_COMMANDS_EXITED_ZERO_SOURCE_RECHECKED"
            || package.independent_review != "PENDING"
            || package.wake_ready
            || package.recorded_at_unix == 0
        {
            return Err("CODEX_QUEUE_REVIEW_PACKAGE: package and bound evidence disagree".into());
        }
        Ok(Some((package, digest)))
    }
    fn read_receipt(&self, receipt_id: &str) -> Result<QueueReceipt, String> {
        let path = self
            .root
            .join("receipts")
            .join(format!("{receipt_id}.json"));
        let bytes = read_regular_bounded(&path, MAX_RECEIPT_BYTES)?
            .ok_or("CODEX_QUEUE: receipt is unavailable")?;
        let receipt: QueueReceipt = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE: receipt is corrupt".to_string())?;
        if receipt.schema_version != QUEUE_SCHEMA_VERSION
            || receipt.receipt_id != receipt_id
            || !Uuid::parse_str(&receipt.attempt_id)
                .is_ok_and(|id| id.to_string() == receipt.attempt_id)
        {
            return Err("CODEX_QUEUE: receipt identity is invalid".into());
        }
        let job = self
            .load_job(&receipt.project_id, &receipt.task_id)?
            .ok_or("CODEX_QUEUE: receipt task is unavailable")?;
        if job.binding.workspace != receipt.workspace
            || job.binding.thread_id != receipt.thread_id
            || job.binding.target_sha256 != receipt.target_sha256
            || job.prompt_sha256 != receipt.prompt_sha256
        {
            return Err("CODEX_QUEUE: receipt task binding changed".into());
        }
        Ok(receipt)
    }
}

fn reject_overlap(state_root: &Path, project_workspace: &Path) -> Result<(), String> {
    if state_root.starts_with(project_workspace) || project_workspace.starts_with(state_root) {
        Err("CODEX_QUEUE: user state overlaps a project workspace".into())
    } else {
        Ok(())
    }
}

fn unix_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "CODEX_QUEUE: system clock is invalid".into())
}

fn redirected(metadata: &fs::Metadata) -> bool {
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

fn nofollow(options: &mut OpenOptions) {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
}

fn same_open_file_identity(
    file: &File,
    path: &Path,
    opened: &fs::Metadata,
    observed: &fs::Metadata,
) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let mut options = OpenOptions::new();
        options.read(true);
        nofollow(&mut options);
        let Ok(second) = options.open(path) else {
            return false;
        };
        let Ok(second_metadata) = second.metadata() else {
            return false;
        };
        if !second_metadata.is_file() || redirected(&second_metadata) {
            return false;
        }
        let Ok(first_info) = crate::windows_protected_fs::handle_information(
            file.as_raw_handle(),
            "operator queue record",
        ) else {
            return false;
        };
        let Ok(second_info) = crate::windows_protected_fs::handle_information(
            second.as_raw_handle(),
            "operator queue record",
        ) else {
            return false;
        };
        let _ = (opened, observed);
        first_info.file_attributes & 0x400 == 0
            && second_info.file_attributes & 0x400 == 0
            && first_info.volume_serial_number == second_info.volume_serial_number
            && first_info.file_index_high == second_info.file_index_high
            && first_info.file_index_low == second_info.file_index_low
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = (file, path);
        opened.dev() == observed.dev() && opened.ino() == observed.ino()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (file, path, opened, observed);
        false
    }
}

fn checked_dir(path: &Path) -> Result<(), String> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("CODEX_QUEUE: state directory is unavailable".into()),
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "CODEX_QUEUE: state directory is unavailable")?;
    if !metadata.is_dir() || redirected(&metadata) {
        return Err("CODEX_QUEUE: state directory is redirected or unsafe".into());
    }
    Ok(())
}

fn checked_file_if_present(path: &Path, max_bytes: u64) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("CODEX_QUEUE: state file is unreadable".into()),
    };
    if !metadata.is_file() || redirected(&metadata) || metadata.len() > max_bytes {
        return Err("CODEX_QUEUE: state file is redirected or oversized".into());
    }
    Ok(())
}

fn read_regular_bounded(path: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("CODEX_QUEUE: state file is unreadable".into()),
    };
    if !metadata.is_file() || redirected(&metadata) || metadata.len() > max_bytes {
        return Err("CODEX_QUEUE: state file is redirected or oversized".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    nofollow(&mut options);
    let file = options
        .open(path)
        .map_err(|_| "CODEX_QUEUE: state file is unreadable")?;
    let opened = file
        .metadata()
        .map_err(|_| "CODEX_QUEUE: state file is unreadable")?;
    let observed =
        fs::symlink_metadata(path).map_err(|_| "CODEX_QUEUE: state file is unreadable")?;
    if !opened.is_file()
        || redirected(&opened)
        || opened.len() > max_bytes
        || !observed.is_file()
        || redirected(&observed)
        || !same_open_file_identity(&file, path, &opened, &observed)
    {
        return Err("CODEX_QUEUE: state file changed or is redirected".into());
    }
    let mut bytes = Vec::new();
    (&file)
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "CODEX_QUEUE: state file is unreadable")?;
    if bytes.len() as u64 > max_bytes {
        return Err("CODEX_QUEUE: state file is oversized".into());
    }
    let observed_after = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_QUEUE: state file changed during read".to_string())?;
    if !same_open_file_identity(&file, path, &opened, &observed_after)
        || redirected(&observed_after)
    {
        return Err("CODEX_QUEUE: state file changed during read".into());
    }
    Ok(Some(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("CODEX_QUEUE: state path has no parent")?;
    let draft = parent.join(format!(".queue-{}.tmp", Uuid::new_v4()));
    create_new_record(&draft, bytes)?;
    fs::rename(&draft, path).map_err(|_| "CODEX_QUEUE: atomic state publication failed".to_string())
}

fn create_new_record(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "CODEX_QUEUE: record already exists or cannot be created")?;
    file.write_all(bytes)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|_| "CODEX_QUEUE: record could not be persisted".to_string())
}

fn validate_job(job: &QueueJob) -> Result<(), String> {
    valid_slug(&job.task_id)?;
    valid_slug(&job.binding.project_id)?;
    if job.schema_version != QUEUE_SCHEMA_VERSION
        || job.prompt.is_empty()
        || job.prompt.len() > MAX_TASK_BYTES
        || job.prompt.contains('\0')
        || job.prompt_sha256 != format!("{:x}", Sha256::digest(job.prompt.as_bytes()))
        || !Uuid::parse_str(&job.binding.thread_id)
            .is_ok_and(|id| id.to_string() == job.binding.thread_id)
        || job
            .model
            .as_deref()
            .is_some_and(|value| !valid_model(value))
        || job
            .effort
            .as_deref()
            .is_some_and(|value| !valid_effort(value))
        || job.binding.workspace.as_os_str().is_empty()
        || job
            .review_candidate_sha256
            .as_deref()
            .is_some_and(|sha| !valid_sha256_hex(sha))
        || job
            .verification_sha256
            .as_deref()
            .is_some_and(|sha| !valid_sha256_hex(sha))
        || job
            .review_package_sha256
            .as_deref()
            .is_some_and(|sha| !valid_sha256_hex(sha))
        || job
            .advisory_outcome_sha256
            .as_deref()
            .is_some_and(|sha| !valid_sha256_hex(sha))
    {
        return Err("CODEX_QUEUE: job record is invalid".into());
    }
    if job.state == QueueState::Claimed && job.attempt.is_none() {
        return Err("CODEX_QUEUE: claimed job has no attempt".into());
    }
    if job.attempt.as_ref().is_some_and(|attempt| {
        queue_writer_marker_sha256(&attempt.attempt_id).is_err()
            || attempt.writer_marker_sha256.is_some() && !attempt.marker_binding_is_valid()
    }) {
        return Err("CODEX_QUEUE: attempt writer marker binding is invalid".into());
    }
    if job.review_package_sha256.is_some()
        && (job.state != QueueState::NativeExitedZero
            || job.receipt_id.is_none()
            || job.review_candidate_sha256.is_none()
            || job.verification_sha256.is_none())
    {
        return Err("CODEX_QUEUE: review package lacks its completed evidence chain".into());
    }
    if job.advisory_outcome_sha256.is_some()
        && (job.state != QueueState::NativeExitedZero || job.review_package_sha256.is_none())
    {
        return Err("CODEX_QUEUE: advisory outcome lacks its completed package chain".into());
    }
    Ok(())
}

/// Observe the same registered root, managed-accounting gate and Git baseline
/// used by a queued turn without creating a queue namespace, lease or writer
/// marker. This is a diagnostic snapshot: a later dispatch must recheck all
/// conditions under its real locks, and the native CLI is not launched here.
async fn preflight_project(control_workspace: &Path, scope: &CodexScope) -> Result<Value, String> {
    let project = registered_project(control_workspace, scope)?;
    let mut blockers = Vec::new();
    if exact_thread(&project).is_err() {
        blockers.push("THREAD_UNBOUND");
    }

    let recorded_gate = super::recorded_turn_gate_summary(&project);
    if recorded_gate["recordedState"] != "NO_RECORDED_BLOCKER_OBSERVED" {
        blockers.push("MANAGED_OWNERSHIP_OR_ACCOUNTING_UNRESOLVED");
    }

    let profile_result = verification_profile(&project.verification_profile).and_then(|profile| {
        if profile.tool == VerificationTool::Uv {
            profile_binding_inputs(&project.workspace, &project.verification_profile).map(|_| ())
        } else {
            for name in profile.inputs {
                let bytes =
                    read_regular_bounded(&project.workspace.join(name), MAX_GIT_FILES_BYTES)?
                        .ok_or("CODEX_QUEUE_PREFLIGHT: required Rust profile input is absent")?;
                if *name == "Cargo.toml" {
                    let text = std::str::from_utf8(&bytes)
                        .map_err(|_| "CODEX_QUEUE_PREFLIGHT: Cargo manifest is not UTF-8")?;
                    toml::from_str::<toml::Value>(text)
                        .map_err(|_| "CODEX_QUEUE_PREFLIGHT: Cargo manifest is invalid TOML")?;
                }
            }
            Ok(())
        }
    });
    if profile_result.is_err() {
        blockers.push("VERIFICATION_PROFILE_INPUTS_UNAVAILABLE");
    }

    let (git_result, tracked_result) =
        match resolve_trusted_git(&project.workspace, control_workspace) {
            Ok(git) => (
                capture_git_review_witness(&project.workspace, &git).await,
                preflight_profile_inputs_tracked(&project, &git).await,
            ),
            Err(error) => (
                Err(error),
                Err(
                    "CODEX_QUEUE_PREFLIGHT: trusted Git is unavailable for profile tracking".into(),
                ),
            ),
        };
    if tracked_result.is_err() {
        blockers.push("VERIFICATION_PROFILE_INPUTS_UNTRACKED_OR_UNSAFE");
    }
    let git = match git_result {
        Ok(witness) => {
            if witness.head_commit.is_none() || !witness.changed_paths.is_empty() {
                blockers.push("REVIEW_BASELINE_DIRTY_OR_UNBORN");
            }
            json!({
                "headCommit":witness.head_commit,
                "changedPathCount":witness.changed_paths.len(),
                "statusSha256":witness.status_sha256,
                "clean":witness.head_commit.is_some() && witness.changed_paths.is_empty(),
            })
        }
        Err(ref error) => {
            blockers.push("REVIEW_BASELINE_UNAVAILABLE");
            json!({"clean":false, "error":error})
        }
    };
    Ok(json!({
        "action":"codex_queue_preflight", "projectId":project.project_id,
        "workspace":project.workspace, "threadId":project.codex_thread_id,
        "verificationProfile":project.verification_profile,
        "bindingSha256":project_binding_sha256(&project)?,
        "recordedTurnGate":recorded_gate,
        "profileInputsValid":profile_result.is_ok(),
        "profileError":profile_result.err(),
        "profileInputsTracked":tracked_result.is_ok(),
        "profileTrackingError":tracked_result.err(),
        "git":git,
        "status":if blockers.is_empty() { "CANDIDATE_ONLY" } else { "BLOCKED" },
        "blockers":blockers,
        "readOnly":true, "transactional":false, "launchAuthorized":false,
        "writerLockChecked":false, "nativeCliChecked":false,
        "operatorQueueAndLeaseChecked":false, "ownerHandoffChecked":false,
        "ignoredFilesChecked":false,
        "meaning":"Recorded prerequisites at observation time only. A later dispatch rechecks exact binding, writer ownership, queue and scheduler capacity, CLI identity and Git; this report grants no launch, review or Wake authority.",
    }))
}

async fn preflight_profile_inputs_tracked(
    project: &AutonomousProjectV1,
    git: &TrustedGit,
) -> Result<(), String> {
    let profile = verification_profile(&project.verification_profile)?;
    if profile.inputs.len() != 2 {
        return Err(
            "CODEX_QUEUE_PREFLIGHT: verification profile input shape is unsupported".into(),
        );
    }
    let (ok, tags) = git_witness_output(
        git,
        &project.workspace,
        &[
            "ls-files",
            "-v",
            "--error-unmatch",
            "--",
            profile.inputs[0],
            profile.inputs[1],
        ],
        4096,
    )
    .await?;
    let mut expected = profile
        .inputs
        .iter()
        .map(|name| format!("H {name}\n"))
        .collect::<Vec<_>>();
    expected.sort();
    if !ok || tags != expected.concat().as_bytes() {
        return Err("CODEX_QUEUE_PREFLIGHT: profile inputs are untracked or have Git flags".into());
    }
    Ok(())
}

pub(crate) async fn execute_action(
    action: QueueAction,
    control_workspace: &Path,
) -> Result<Value, String> {
    // A queue verb never accepts a caller-selected control plane. All state
    // below is anchored to the registered CatDesk Git root.
    let control = resolve_registered_project_from_control_root(control_workspace, "catdesk")
        .map_err(|_| {
            "CODEX_QUEUE_SCOPE: CatDesk control Git identity is unavailable".to_string()
        })?;
    let pause_action = matches!(&action, QueueAction::Pause { .. });
    match action {
        QueueAction::Preflight { scope } => {
            preflight_project(&control.control_workspace, &scope).await
        }
        QueueAction::Add {
            scope,
            task_id,
            task,
            model,
            effort,
        } => {
            let project = registered_project(&control.control_workspace, &scope)?;
            let store = QueueStore::open_for_operator(&control.project, &project.workspace)?;
            let _lock = store.lock()?;
            let proposed = QueueJob::new(task_id, &project, task, model, effort)?;
            if let Some(existing) =
                store.load_job(&proposed.binding.project_id, &proposed.task_id)?
            {
                if !existing.same_request(&proposed) {
                    return Err("CODEX_QUEUE_IDEMPOTENCY: task ID already has a different scope, prompt, or profile".into());
                }
                return Ok(
                    json!({ "action":"codex_queue_add", "idempotent":true, "job":existing.summary() }),
                );
            }
            if store.jobs()?.len() >= MAX_JOBS {
                return Err("CODEX_QUEUE: job limit reached".into());
            }
            store.save_job(&proposed)?;
            Ok(json!({ "action":"codex_queue_add", "idempotent":false, "job":proposed.summary() }))
        }
        QueueAction::Status => {
            let jobs = match QueueStore::open_read_only(&control.project)? {
                Some(store) => store
                    .jobs()?
                    .iter()
                    .map(QueueJob::summary)
                    .collect::<Vec<_>>(),
                None => Vec::new(),
            };
            Ok(json!({
                "action":"codex_queue_status", "jobs":jobs,
                "claimRecoveryPolicy":"CLAIMED and UNKNOWN jobs are never replayed automatically",
                "execution":"explicit_dispatch_only", "authority":"same_user_operator_coordination",
            }))
        }
        QueueAction::Receipt { receipt_id } => {
            let store = QueueStore::open_read_only(&control.project)?
                .ok_or("CODEX_QUEUE: queue state is unavailable")?;
            Ok(
                json!({ "action":"codex_queue_receipt", "receipt":store.read_receipt(&receipt_id)? }),
            )
        }
        QueueAction::Review { receipt_id } => {
            let store = QueueStore::open_read_only(&control.project)?
                .ok_or("CODEX_QUEUE: queue state is unavailable")?;
            let receipt = store.read_receipt(&receipt_id)?;
            let candidate = store.read_review_candidate(&receipt)?;
            let job = store
                .load_job(&receipt.project_id, &receipt.task_id)?
                .ok_or("CODEX_QUEUE_REVIEW: bound job is unavailable")?;
            let binding_matches = registered_project(
                &control.control_workspace,
                &CodexScope {
                    project_id: receipt.project_id.clone(),
                    workspace: receipt.workspace.clone(),
                },
            )
            .as_ref()
            .is_ok_and(|project| job.binding.matches(project));
            let current_matches =
                match resolve_trusted_git(&receipt.workspace, &control.control_workspace) {
                    Ok(git) => capture_git_review_witness(&receipt.workspace, &git)
                        .await
                        .ok()
                        .map(|current| current == candidate.after),
                    Err(_) => None,
                };
            let verification =
                store
                    .read_verification(&receipt, &candidate)?
                    .map(|(artifact, sha)| {
                        json!({
                            "summary":artifact.summary(&sha),
                            "artifactPath":store.verification_path(&receipt_id),
                        })
                    });
            Ok(json!({
                "action":"codex_queue_review", "candidate":candidate,
                "jobState":job.state,
                "scopeBindingMatchesNow":binding_matches,
                "worktreeMatchesCapturedAfter":current_matches,
                "verificationEligibility":if candidate.after.index_diff_sha256.is_empty() {
                    "LEGACY_CANDIDATE_UNVERIFIABLE"
                } else {
                    "REQUIRES_EFFECTIVE_PROFILE_AND_SOURCE_CHECK"
                },
                "verification":verification,
                "meaning":"Same-user Git observations and optional fixed-profile local verification. Independent review and Wake remain pending. Ignored files and submodule contents are excluded.",
            }))
        }
        QueueAction::Verify { receipt_id } => verify_receipt(&control.project, &receipt_id).await,
        QueueAction::ReviewPackage { receipt_id } => {
            package_verified_receipt(&control.project, &receipt_id)
        }
        QueueAction::ReviewInvitePrepare {
            scope,
            receipt_id,
            expected_package_sha256,
        } => {
            let project = registered_project(&control.control_workspace, &scope)?;
            review_invite::prepare(
                &control.control_workspace,
                &control.project,
                &project,
                &receipt_id,
                &expected_package_sha256,
            )
        }
        QueueAction::AdvisoryReview { receipt_id } => {
            advisory::review_package_advisory(&control.project, &receipt_id).await
        }
        QueueAction::OperatorReview {
            scope,
            receipt_id,
            expected_package_sha256,
            decision,
        } => {
            let project = registered_project(&control.control_workspace, &scope)?;
            operator_review::record_local_decision(
                &control.control_workspace,
                &control.project,
                &project,
                &receipt_id,
                &expected_package_sha256,
                &decision,
            )
        }
        QueueAction::OperatorReviewReceipt { receipt_id } => operator_review::read_local_decision(
            &control.control_workspace,
            &control.project,
            &receipt_id,
        ),
        QueueAction::ReviewDisclosureGrant {
            scope,
            receipt_id,
            expected_package_sha256,
        } => {
            let project = registered_project(&control.control_workspace, &scope)?;
            review_reader::grant_local_disclosure(
                &control.control_workspace,
                &project,
                &receipt_id,
                &expected_package_sha256,
            )
        }
        QueueAction::ReviewDisclosureRenew {
            scope,
            receipt_id,
            expected_package_sha256,
        } => {
            let project = registered_project(&control.control_workspace, &scope)?;
            review_reader::renew_local_disclosure(
                &control.control_workspace,
                &project,
                &receipt_id,
                &expected_package_sha256,
            )
        }
        QueueAction::ReviewDisclosureRevoke {
            project_id,
            receipt_id,
            expected_package_sha256,
        } => review_reader::revoke_local_disclosure(
            &control.control_workspace,
            &project_id,
            &receipt_id,
            &expected_package_sha256,
        ),
        QueueAction::ProfileBind {
            scope,
            expected_profile,
            new_profile,
            expected_binding_sha256,
            confirmed_quiescent_thread,
        } => {
            let project = registered_project(&control.control_workspace, &scope)?;
            let store = QueueStore::open_for_operator(&control.project, &project.workspace)?;
            bind_profile_in_store(
                &control.control_workspace,
                &store,
                &project,
                &expected_profile,
                &new_profile,
                &expected_binding_sha256,
                &confirmed_quiescent_thread,
            )
            .await
        }
        QueueAction::Pause { scope, task_id } | QueueAction::Resume { scope, task_id } => {
            let pause = pause_action;
            let project = registered_project(&control.control_workspace, &scope)?;
            let store = QueueStore::open_for_operator(&control.project, &project.workspace)?;
            let _lock = store.lock()?;
            let mut job = store
                .load_job(&project.project_id, &task_id)?
                .ok_or("CODEX_QUEUE: task ID is not queued for this project")?;
            if !job.binding.matches(&project) {
                return Err("CODEX_QUEUE_SCOPE: queued task binding drifted".into());
            }
            let expected = if pause {
                QueueState::Ready
            } else {
                QueueState::Paused
            };
            if job.state != expected {
                return Err("CODEX_QUEUE_STATE: only a quiescent ready task may pause, or a paused task resume".into());
            }
            job.state = if pause {
                QueueState::Paused
            } else {
                QueueState::Ready
            };
            store.save_job(&job)?;
            Ok(
                json!({ "action":if pause { "codex_queue_pause" } else { "codex_queue_resume" }, "job":job.summary() }),
            )
        }
        QueueAction::Dispatch => dispatch_one(&control.project).await,
    }
}

fn profile_binding_inputs(
    workspace: &Path,
    profile_name: &str,
) -> Result<Vec<(String, String)>, String> {
    let profile = verification_profile(profile_name)?;
    if profile.tool != VerificationTool::Uv || profile.inputs.len() != 2 {
        return Err("CODEX_QUEUE_PROFILE: only the two fixed Python profiles can be bound".into());
    }
    validate_python_profile_manifest(workspace, profile_name)?;
    let mut inputs = Vec::new();
    for name in profile.inputs {
        let bytes = read_regular_bounded(&workspace.join(name), MAX_GIT_FILES_BYTES)?
            .ok_or("CODEX_QUEUE_PROFILE: tracked manifest or lock is unavailable")?;
        if *name == "uv.lock" {
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| "CODEX_QUEUE_PROFILE: uv.lock is not UTF-8")?;
            let lock: toml::Value =
                toml::from_str(text).map_err(|_| "CODEX_QUEUE_PROFILE: uv.lock is invalid TOML")?;
            let root_name = profile
                .package_name
                .ok_or("CODEX_QUEUE_PROFILE: Python package name is unavailable")?;
            let root_locked = lock
                .get("package")
                .and_then(toml::Value::as_array)
                .is_some_and(|packages| {
                    packages.iter().any(|package| {
                        package.get("name").and_then(toml::Value::as_str) == Some(root_name)
                    })
                });
            if lock
                .get("version")
                .and_then(toml::Value::as_integer)
                .is_none()
                || !root_locked
            {
                return Err(
                    "CODEX_QUEUE_PROFILE: uv.lock does not bind the selected package".into(),
                );
            }
        }
        inputs.push(((*name).into(), format!("{:x}", Sha256::digest(&bytes))));
    }
    Ok(inputs)
}

async fn bind_profile_in_store(
    control_workspace: &Path,
    queue: &QueueStore,
    selected: &AutonomousProjectV1,
    expected_profile: &str,
    new_profile: &str,
    expected_binding_sha256: &str,
    confirmed_quiescent_thread: &str,
) -> Result<Value, String> {
    if expected_profile != "rust_full"
        || !matches!(
            new_profile,
            "python_uv_group_dev_v1" | "python_uv_extras_dev_lab_v1"
        )
        || !matches!(
            selected.verification_profile.as_str(),
            "rust_full" | "python_uv_group_dev_v1" | "python_uv_extras_dev_lab_v1"
        )
    {
        return Err("CODEX_QUEUE_PROFILE: requested profile transition is unsupported".into());
    }
    let mut expected_old = selected.clone();
    expected_old.verification_profile = expected_profile.into();
    if project_binding_sha256(&expected_old)? != expected_binding_sha256 {
        return Err("CODEX_QUEUE_PROFILE: expected full project binding hash differs".into());
    }
    if exact_thread(selected)? != confirmed_quiescent_thread {
        return Err(
            "CODEX_QUEUE_PROFILE: quiescence assertion names a different bound thread".into(),
        );
    }
    if selected.verification_profile != expected_profile
        && selected.verification_profile != new_profile
    {
        return Err(
            "CODEX_QUEUE_PROFILE: current profile differs from the expected transition".into(),
        );
    }
    let _queue_lock = queue.lock()?;
    let _writer = acquire_project_writer_lock(&selected.workspace)
        .map_err(|_| "CODEX_QUEUE_PROFILE: project writer or pending marker is present")?;
    let current = registered_project(
        control_workspace,
        &CodexScope {
            project_id: selected.project_id.clone(),
            workspace: selected.workspace.clone(),
        },
    )?;
    ensure_same_project_binding(selected, &current)?;
    ensure_no_interrupted_managed_writer(selected)?;
    if queue
        .jobs()?
        .iter()
        .any(|job| job.binding.project_id == selected.project_id)
    {
        return Err("CODEX_QUEUE_PROFILE: project has queued or historical jobs".into());
    }
    let inputs = profile_binding_inputs(&selected.workspace, new_profile)?;
    let git = resolve_trusted_git(&selected.workspace, control_workspace)?;
    let (tracked, input_tags) = git_witness_output(
        &git,
        &selected.workspace,
        &[
            "ls-files",
            "-v",
            "--error-unmatch",
            "--",
            "pyproject.toml",
            "uv.lock",
        ],
        4096,
    )
    .await?;
    if !tracked || input_tags != PROFILE_INPUTS_TAGGED {
        return Err("CODEX_QUEUE_PROFILE: pyproject.toml and uv.lock must be tracked without assume-unchanged or skip-worktree bits".into());
    }
    let (status_ok, dirty_inputs) = git_witness_output(
        &git,
        &selected.workspace,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            "pyproject.toml",
            "uv.lock",
        ],
        4096,
    )
    .await?;
    if !status_ok || !dirty_inputs.is_empty() {
        return Err(
            "CODEX_QUEUE_PROFILE: pyproject.toml and uv.lock must have clean index and worktree states"
                .into(),
        );
    }
    let registry = AutonomousProjectRegistryStoreV1::open_read_only(
        control_workspace.join(".catdesk/projects"),
    )
    .map_err(|_| "CODEX_QUEUE_PROFILE: project registry is unavailable")?;
    let (updated, idempotent) = registry
        .bind_verification_profile_after(selected, expected_profile, new_profile, || {
            if queue
                .jobs()
                .map_err(RuntimeError::Validation)?
                .iter()
                .any(|job| job.binding.project_id == selected.project_id)
            {
                return Err(RuntimeError::Validation(
                    "project jobs appeared during profile binding".into(),
                ));
            }
            let observed_inputs = profile_binding_inputs(&selected.workspace, new_profile)
                .map_err(RuntimeError::Validation)?;
            if observed_inputs != inputs {
                return Err(RuntimeError::Validation(
                    "Python profile inputs changed during binding".into(),
                ));
            }
            profile_git_inputs_clean_under_lock(&git, &selected.workspace)?;
            Ok(())
        })
        .map_err(|_| "CODEX_QUEUE_PROFILE: binding, lease, jobs, or profile inputs changed")?;
    Ok(json!({
        "action":"codex_queue_profile_bind", "idempotent":idempotent,
        "projectId":updated.project_id, "workspace":updated.workspace,
        "verificationProfile":updated.verification_profile,
        "bindingSha256":project_binding_sha256(&updated)?,
        "threadId":updated.codex_thread_id,
        "targetSha256":updated.chatgpt_target_sha256,
        "meaning":"Fixed verification profile metadata only; no native turn, queue job, reviewer, or Wake event was launched.",
        "chatQuiescence":"OPERATOR_ASSERTED_UNVERIFIED",
    }))
}

/// The registry callback is synchronous while the scheduler and registration
/// locks are held. Recheck both index flags and porcelain there so an
/// index-only edit after preflight cannot be published.
fn profile_git_inputs_clean_under_lock(
    git: &TrustedGit,
    workspace: &Path,
) -> Result<(), RuntimeError> {
    let tagged = profile_git_output_under_lock(
        git,
        workspace,
        &[
            "ls-files",
            "-v",
            "--error-unmatch",
            "--",
            "pyproject.toml",
            "uv.lock",
        ],
    )?;
    let status = profile_git_output_under_lock(
        git,
        workspace,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            "pyproject.toml",
            "uv.lock",
        ],
    )?;
    if tagged != PROFILE_INPUTS_TAGGED || !status.is_empty() {
        return Err(RuntimeError::Validation(
            "Python profile inputs have Git index or worktree drift".into(),
        ));
    }
    Ok(())
}

fn profile_git_output_under_lock(
    git: &TrustedGit,
    workspace: &Path,
    args: &[&str],
) -> Result<Vec<u8>, RuntimeError> {
    let refusal = || RuntimeError::Validation("Python profile inputs have Git drift".into());
    if native_executable_sha256(&git.executable).ok().as_deref() != Some(&git.sha256) {
        return Err(refusal());
    }
    let mut command = std::process::Command::new(&git.executable);
    command
        .args([
            "--no-optional-locks",
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(name);
    }
    let mut child = command.spawn().map_err(|_| refusal())?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < GIT_WITNESS_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(refusal());
            }
        }
    };
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(refusal)?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| refusal())?;
    if !status.success() || bytes.len() > 4096 {
        return Err(refusal());
    }
    Ok(bytes)
}

fn package_verified_receipt(
    control: &AutonomousProjectV1,
    receipt_id: &str,
) -> Result<Value, String> {
    let store = QueueStore::open_for_operator(control, &control.workspace)?;
    package_verified_receipt_in_store(&store, receipt_id)
}

fn package_verified_receipt_in_store(
    store: &QueueStore,
    receipt_id: &str,
) -> Result<Value, String> {
    // This is a historical evidence copy. It never reads or writes the
    // registered source worktree and never launches a reviewer process.
    let _receipt_lock = store.verification_lock(receipt_id)?;
    let _queue_lock = store.lock()?;
    let receipt = store.read_receipt(receipt_id)?;
    let candidate = store.read_review_candidate(&receipt)?;
    let (verification, verification_sha256) = store
        .read_verification(&receipt, &candidate)?
        .ok_or("CODEX_QUEUE_REVIEW_PACKAGE: completed local verification is unavailable")?;
    let job = store
        .load_job(&receipt.project_id, &receipt.task_id)?
        .ok_or("CODEX_QUEUE_REVIEW_PACKAGE: bound job is unavailable")?;
    if job.state != QueueState::NativeExitedZero
        || job.receipt_id.as_deref() != Some(receipt_id)
        || verification.status != "LOCAL_COMMANDS_EXITED_ZERO_SOURCE_RECHECKED"
    {
        return Err(
            "CODEX_QUEUE_REVIEW_PACKAGE: completed source-bound local verification is required"
                .into(),
        );
    }
    let package_path = store.review_package_path(receipt_id);
    let package_exists = match fs::symlink_metadata(&package_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err("CODEX_QUEUE_REVIEW_PACKAGE: package path is unreadable".into()),
    };
    if job.review_package_sha256.is_none() && package_exists {
        // A crash between create-once record publication and the job digest
        // update needs owner inspection. Replaying would adopt unknown bytes.
        let mut attention = job;
        attention.state = QueueState::Attention;
        store.save_job(&attention)?;
        return Err("CODEX_QUEUE_REVIEW_PACKAGE_ATTENTION: unbound package survived before job publication; owner recovery is required".into());
    }
    if let Some((package, digest)) =
        store.read_review_package(&receipt, &candidate, &verification, &verification_sha256)?
    {
        return Ok(json!({
            "action":"codex_queue_review_package", "idempotent":true,
            "package":package.summary(&digest),
            "artifactPath":store.review_package_path(receipt_id),
            "meaning":"Historical source and local command evidence copied for later independent review. Independent review and Wake remain pending.",
        }));
    }
    let package = QueueReviewPackage {
        schema_version: QUEUE_SCHEMA_VERSION,
        receipt_id: receipt_id.into(),
        receipt_sha256: store.persisted_receipt_sha256(&receipt)?,
        review_candidate_sha256: job
            .review_candidate_sha256
            .as_ref()
            .ok_or("CODEX_QUEUE_REVIEW_PACKAGE: candidate digest is absent")?
            .clone(),
        verification_sha256,
        prompt: job.prompt.clone(),
        receipt,
        candidate,
        verification,
        recorded_at_unix: unix_now()?,
        independent_review: "PENDING".into(),
        wake_ready: false,
    };
    let digest = store.write_review_package(&package)?;
    let mut updated = job;
    updated.review_package_sha256 = Some(digest.clone());
    store.save_job(&updated)?;
    Ok(json!({
        "action":"codex_queue_review_package", "idempotent":false,
        "package":package.summary(&digest),
        "artifactPath":store.review_package_path(receipt_id),
        "meaning":"Historical source and local command evidence copied for later independent review. Independent review and Wake remain pending.",
    }))
}

async fn verify_receipt(control: &AutonomousProjectV1, receipt_id: &str) -> Result<Value, String> {
    let control_workspace = &control.workspace;
    let store = QueueStore::open_for_operator(control, control_workspace)?;
    verify_receipt_in_store(control, &store, receipt_id).await
}

async fn verify_receipt_in_store(
    control: &AutonomousProjectV1,
    store: &QueueStore,
    receipt_id: &str,
) -> Result<Value, String> {
    verify_receipt_in_store_with_executable(control, store, receipt_id, None).await
}

async fn verify_receipt_in_store_with_executable(
    control: &AutonomousProjectV1,
    store: &QueueStore,
    receipt_id: &str,
    #[cfg_attr(not(test), allow(unused_variables))] fixture_executable: Option<
        &TrustedVerificationExecutable,
    >,
) -> Result<Value, String> {
    let control_workspace = &control.workspace;
    let _receipt_lock = store.verification_lock(receipt_id)?;
    let (receipt, candidate, job) = {
        let _queue_lock = store.lock()?;
        let receipt = store.read_receipt(receipt_id)?;
        let candidate = store.read_review_candidate(&receipt)?;
        let job = store
            .load_job(&receipt.project_id, &receipt.task_id)?
            .ok_or("CODEX_QUEUE_VERIFY: bound job is unavailable")?;
        if job.state != QueueState::NativeExitedZero
            || job.receipt_id.as_deref() != Some(receipt_id)
            || job
                .attempt
                .as_ref()
                .map(|attempt| attempt.attempt_id.as_str())
                != Some(receipt.attempt_id.as_str())
            || receipt.exit_code != Some(0)
            || receipt.outcome != "NATIVE_EXIT_ZERO"
            || !receipt.child_exit_observed
            || !receipt.binding_still_matches
            || !actual_profile_matches_receipt(&receipt)
        {
            return Err("CODEX_QUEUE_VERIFY: process receipt lacks a completed bound turn with matching effective model and effort".into());
        }
        if candidate.after.index_diff_sha256.is_empty() {
            return Err("CODEX_QUEUE_VERIFY_LEGACY_CANDIDATE_UNVERIFIABLE: staged index witness is absent; historical review remains readable".into());
        }
        if job.verification_sha256.is_none()
            && read_regular_bounded(&store.verification_path(receipt_id), MAX_VERIFICATION_BYTES)?
                .is_some()
        {
            // The artifact write and job update are separate durable writes.
            // A crash in that gap is preserved for owner inspection rather
            // than adopting unanchored bytes or replaying tests automatically.
            let mut attention = job.clone();
            attention.state = QueueState::Attention;
            store.save_job(&attention)?;
            return Err("CODEX_QUEUE_VERIFY_ATTENTION: unbound verification artifact survived before job publication; owner recovery is required".into());
        }
        if let Some((artifact, digest)) = store.read_verification(&receipt, &candidate)? {
            return Ok(json!({
                "action":"codex_queue_verify", "idempotent":true,
                "verification":artifact.summary(&digest),
                "artifactPath":store.verification_path(receipt_id),
                "historicalReadback":true,
                "meaning":"Persisted local command exits on the captured source; current worktree, descendants, independent review, and Wake are not proven by this readback.",
            }));
        }
        (receipt, candidate, job)
    };
    let project = registered_project(control_workspace, &job.binding.scope())?;
    if !job.binding.matches(&project) {
        return Err("CODEX_QUEUE_VERIFY: project registration changed after native turn".into());
    }
    let profile = verification_profile(&job.binding.verification_profile)?;
    for name in profile.inputs {
        let metadata = fs::symlink_metadata(project.workspace.join(name)).map_err(|_| {
            format!("CODEX_QUEUE_VERIFY: {} requires a regular {name}; registered profile may not match this project", job.binding.verification_profile)
        })?;
        if !metadata.is_file() || redirected(&metadata) {
            return Err(format!(
                "CODEX_QUEUE_VERIFY: {} requires a regular {name}",
                job.binding.verification_profile
            ));
        }
    }
    let mut writer = acquire_project_writer_lock(&project.workspace)
        .map_err(|_| "CODEX_QUEUE_VERIFY: project writer is active or unresolved".to_string())?;
    let observed = registered_project(control_workspace, &job.binding.scope())?;
    if !job.binding.matches(&observed)
        || ensure_same_project_binding(&project, &observed).is_err()
        || ensure_no_interrupted_managed_writer(&project).is_err()
    {
        return Err("CODEX_QUEUE_VERIFY: project writer or registration changed".into());
    }
    let git = resolve_trusted_git(&project.workspace, control_workspace)?;
    for name in profile.inputs {
        let path = project.workspace.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(
            |_| "CODEX_QUEUE_VERIFY: profile manifest or lock vanished under writer lock",
        )?;
        if !metadata.is_file() || redirected(&metadata) {
            return Err("CODEX_QUEUE_VERIFY: profile manifest or lock is redirected".into());
        }
    }
    if profile.tool == VerificationTool::Uv {
        validate_python_profile_manifest(&project.workspace, &job.binding.verification_profile)?;
    }
    let (tracked_inputs, _) = git_witness_output(
        &git,
        &project.workspace,
        &[
            "ls-files",
            "--error-unmatch",
            "--",
            profile.inputs[0],
            profile.inputs[1],
        ],
        4096,
    )
    .await?;
    if !tracked_inputs {
        return Err("CODEX_QUEUE_VERIFY: profile requires tracked manifest and lock files".into());
    }
    if capture_git_review_witness(&project.workspace, &git).await? != candidate.after {
        return Err(
            "CODEX_QUEUE_VERIFY: current source differs from the native post-turn witness".into(),
        );
    }
    let (patch, index_patch, files) =
        snapshot_verified_source(&project.workspace, &git, &candidate).await?;
    let executable = if cfg!(test) {
        fixture_executable
            .map(|fixture| TrustedVerificationExecutable {
                executable: fixture.executable.clone(),
                sha256: fixture.sha256.clone(),
            })
            .map(Ok)
            .unwrap_or_else(|| {
                resolve_trusted_verifier(&project.workspace, control_workspace, profile.tool)
            })?
    } else {
        resolve_trusted_verifier(&project.workspace, control_workspace, profile.tool)?
    };
    writer
        .begin_writer_turn()
        .map_err(|_| "CODEX_QUEUE_VERIFY: writer marker could not be persisted".to_string())?;
    let mut command_results = Vec::new();
    for argv in profile.commands {
        match execute_fixed_verification_command(
            &project.workspace,
            &executable,
            profile.tool,
            argv,
        )
        .await
        {
            Ok(result) => command_results.push(result),
            Err(VerifyCommandError::Safe(reason)) => {
                writer.finish_writer_turn().map_err(|_| {
                    "CODEX_QUEUE_VERIFY: command did not start or exceeded output bound; writer marker remains unresolved"
                        .to_string()
                })?;
                return Err(reason);
            }
            Err(VerifyCommandError::Uncertain(reason)) => return Err(reason),
        }
    }
    writer.finish_writer_turn().map_err(|_| {
        "CODEX_QUEUE_VERIFY: verification children exited but writer marker could not be retired"
            .to_string()
    })?;
    if capture_git_review_witness(&project.workspace, &git).await? != candidate.after {
        return Err("CODEX_QUEUE_VERIFY: source changed during fixed-profile verification".into());
    }
    let status = if command_results
        .iter()
        .all(|result| result.status == "EXIT_ZERO_PARENT_OBSERVED")
    {
        "LOCAL_COMMANDS_EXITED_ZERO_SOURCE_RECHECKED"
    } else {
        "LOCAL_COMMAND_FAILURE"
    };
    let artifact = QueueVerification {
        schema_version: QUEUE_SCHEMA_VERSION,
        receipt_id: receipt_id.into(),
        receipt_sha256: store.persisted_receipt_sha256(&receipt)?,
        review_candidate_sha256: job
            .review_candidate_sha256
            .clone()
            .ok_or("CODEX_QUEUE_VERIFY: candidate digest is absent")?,
        project_id: receipt.project_id.clone(),
        task_id: receipt.task_id.clone(),
        workspace: receipt.workspace.clone(),
        thread_id: receipt.thread_id.clone(),
        verification_profile: job.binding.verification_profile.clone(),
        actual_profile: receipt.actual_profile.clone(),
        git_witness: candidate.after.clone(),
        patch_sha256: format!("{:x}", Sha256::digest(&patch)),
        patch_base64: BASE64.encode(&patch),
        index_patch_sha256: format!("{:x}", Sha256::digest(&index_patch)),
        index_patch_base64: BASE64.encode(&index_patch),
        files,
        commands: command_results,
        status: status.into(),
        recorded_at_unix: unix_now()?,
        independent_review: "PENDING".into(),
        wake_ready: false,
    };
    let digest = {
        let _queue_lock = store.lock()?;
        let current = store
            .load_job(&receipt.project_id, &receipt.task_id)?
            .ok_or("CODEX_QUEUE_VERIFY: bound job vanished before publication")?;
        if current != job {
            return Err("CODEX_QUEUE_VERIFY: bound job changed before publication".into());
        }
        let digest = store.write_verification(&artifact)?;
        let mut updated = current;
        updated.verification_sha256 = Some(digest.clone());
        store.save_job(&updated)?;
        digest
    };
    Ok(json!({
        "action":"codex_queue_verify", "idempotent":false,
        "verification":artifact.summary(&digest),
        "artifactPath":store.verification_path(receipt_id),
        "historicalReadback":false,
        "meaning":"Fixed local command exits and source recheck at recording time only; descendants, independent review, and Wake are not proven.",
    }))
}

fn scheduler(control_workspace: &Path) -> Result<AutonomousProjectRegistryStoreV1, String> {
    AutonomousProjectRegistryStoreV1::open_read_only(control_workspace.join(".catdesk/projects"))
        .map_err(|_| "CODEX_QUEUE: control registry is unavailable".to_string())
}

fn claim_one(control_workspace: &Path, queue: &QueueStore) -> Result<Option<QueueJob>, String> {
    let _lock = queue.lock()?;
    let read_only_scheduler = scheduler(control_workspace)?;
    let registry = read_only_scheduler
        .load_registry()
        .map_err(|_| "CODEX_QUEUE: control registry is invalid".to_string())?;
    // The fixed user-state root must stay outside every registered project.
    for project in &registry.projects {
        if queue.root.starts_with(&project.workspace) || project.workspace.starts_with(&queue.root)
        {
            return Err("CODEX_QUEUE: queue state overlaps a registered workspace".into());
        }
    }
    let mut ready = BTreeMap::<String, QueueJob>::new();
    for mut job in queue.jobs()? {
        if job.state != QueueState::Ready {
            continue;
        }
        let observed = registered_project(control_workspace, &job.binding.scope());
        if !observed
            .as_ref()
            .is_ok_and(|project| job.binding.matches(project))
        {
            // No launch occurred. Leave the original authority snapshot and
            // prompt intact for a human to reconcile; do not migrate it to a
            // new root, target, or thread.
            job.state = QueueState::Attention;
            queue.save_job(&job)?;
            continue;
        }
        ready.entry(job.binding.project_id.clone()).or_insert(job);
    }
    let ready_ids = registry
        .projects
        .iter()
        .filter(|project| ready.contains_key(&project.project_id))
        .map(|project| project.project_id.as_str())
        .collect::<Vec<_>>();
    if ready_ids.is_empty() {
        return Ok(None);
    }
    // Dispatch is the first queue operation allowed to initialize scheduler
    // children. Validate the existing registry and every ready binding before
    // opening the mutating store, then reject any registry change observed
    // across that transition. `open` checks existing children before use and
    // creates the absent leases directory required by scheduler claims.
    let scheduler =
        AutonomousProjectRegistryStoreV1::open(control_workspace.join(".catdesk/projects"))
            .map_err(|_| "CODEX_QUEUE: scheduler state is unavailable or unsafe".to_string())?;
    if scheduler
        .load_registry()
        .map_err(|_| "CODEX_QUEUE: control registry is invalid".to_string())?
        != registry
    {
        return Err("CODEX_QUEUE_SCOPE: registry changed before scheduler claim".into());
    }
    let now = unix_now()?;
    // Prepare the complete attempt binding before taking a scheduler lease.
    let attempt_id = Uuid::new_v4().to_string();
    let writer_marker_sha256 = queue_writer_marker_sha256(&attempt_id)
        .map_err(|_| "CODEX_QUEUE: new attempt marker binding is invalid".to_string())?;
    let owner = format!("queue-{}", Uuid::new_v4());
    let decision = scheduler
        .schedule_next_ready_worker(
            &ready_ids,
            &owner,
            ScheduledProviderV1::Codex,
            now,
            now.saturating_add(LEASE_SECONDS),
        )
        .map_err(|_| "CODEX_QUEUE: scheduler state is unavailable or unsafe".to_string())?;
    let lease = match decision {
        GlobalScheduleDecisionV1::Granted(lease) => lease,
        GlobalScheduleDecisionV1::Rejected { reason } => {
            return Err(format!("CODEX_QUEUE_CAPACITY: {reason}"));
        }
    };
    let mut job = ready
        .remove(&lease.project_id)
        .ok_or("CODEX_QUEUE: scheduler selected a project without a ready job")?;
    if lease.project_id != job.binding.project_id
        || lease.workspace != job.binding.workspace
        || lease.owner_id != owner
        || lease.provider != ScheduledProviderV1::Codex
        || !registered_project(control_workspace, &job.binding.scope())
            .as_ref()
            .is_ok_and(|project| job.binding.matches(project))
    {
        scheduler.release_workspace_lease(&lease).map_err(|_| {
            "CODEX_QUEUE: drifted scheduler grant could not be released".to_string()
        })?;
        job.state = QueueState::Attention;
        queue.save_job(&job)?;
        return Err("CODEX_QUEUE_SCOPE: selected registry binding changed during claim".into());
    }
    job.state = QueueState::Claimed;
    job.receipt_id = None;
    job.review_candidate_sha256 = None;
    job.attempt = Some(QueueAttempt {
        attempt_id,
        lease: lease.clone(),
        started_at_unix: now,
        writer_marker_sha256: Some(writer_marker_sha256),
    });
    if let Err(error) = queue.save_job(&job) {
        scheduler.release_workspace_lease(&lease).map_err(|_| {
            "CODEX_QUEUE: claim failed and scheduler lease could not be released".to_string()
        })?;
        return Err(error);
    }
    Ok(Some(job))
}

#[derive(Debug)]
struct TurnResult {
    outcome: &'static str,
    exit_code: Option<i32>,
    child_exit_observed: bool,
    child_absent_proven: bool,
    marker_retired: bool,
    native_executable: Option<PathBuf>,
    native_version: Option<String>,
    native_executable_sha256: Option<String>,
    actual_profile: Option<CodexCliTurnProfileEvidenceV1>,
    review_before: Option<GitReviewWitness>,
    review_after: Option<GitReviewWitness>,
}

impl TurnResult {
    fn before_spawn(outcome: &'static str) -> Self {
        Self {
            outcome,
            exit_code: None,
            child_exit_observed: false,
            child_absent_proven: true,
            marker_retired: true,
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
            actual_profile: None,
            review_before: None,
            review_after: None,
        }
    }
    fn unknown(outcome: &'static str) -> Self {
        Self {
            outcome,
            exit_code: None,
            child_exit_observed: false,
            child_absent_proven: false,
            marker_retired: false,
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
            actual_profile: None,
            review_before: None,
            review_after: None,
        }
    }
    fn with_cli(mut self, cli: &NativeCliPreflight) -> Self {
        self.native_executable = Some(cli.executable.clone());
        self.native_version = Some(cli.version.clone());
        self.native_executable_sha256 = Some(cli.sha256.clone());
        self
    }
}

struct NativeCliPreflight {
    executable: PathBuf,
    codex_home: Option<PathBuf>,
    version: String,
    sha256: String,
}

fn native_executable_sha256(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_QUEUE_CLI: native executable is unavailable".to_string())?;
    if !metadata.is_file() || redirected(&metadata) || metadata.len() > MAX_NATIVE_EXE_BYTES {
        return Err("CODEX_QUEUE_CLI: native executable is redirected or oversized".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    nofollow(&mut options);
    let mut file = options
        .open(path)
        .map_err(|_| "CODEX_QUEUE_CLI: native executable cannot be opened".to_string())?;
    let opened = file
        .metadata()
        .map_err(|_| "CODEX_QUEUE_CLI: native executable cannot be inspected")?;
    let observed = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_QUEUE_CLI: native executable changed".to_string())?;
    if !opened.is_file()
        || redirected(&opened)
        || opened.len() > MAX_NATIVE_EXE_BYTES
        || !same_open_file_identity(&file, path, &opened, &observed)
    {
        return Err("CODEX_QUEUE_CLI: native executable changed".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "CODEX_QUEUE_CLI: native executable cannot be hashed".to_string())?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > MAX_NATIVE_EXE_BYTES {
            return Err("CODEX_QUEUE_CLI: native executable is oversized".into());
        }
        digest.update(&buffer[..count]);
    }
    let observed_after = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_QUEUE_CLI: native executable changed".to_string())?;
    if !same_open_file_identity(&file, path, &opened, &observed_after) {
        return Err("CODEX_QUEUE_CLI: native executable changed".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Bounded, local Git observations. These are review inputs, not proof that
/// the native child authored every byte: another same-user writer can edit a
/// workspace outside CatDesk's cooperative lock.
struct TrustedGit {
    executable: PathBuf,
    sha256: String,
}

struct TrustedVerificationExecutable {
    executable: PathBuf,
    sha256: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VerificationTool {
    Cargo,
    Uv,
}

struct VerificationProfile {
    tool: VerificationTool,
    commands: &'static [&'static [&'static str]],
    inputs: &'static [&'static str],
    package_name: Option<&'static str>,
}

fn verification_profile(profile: &str) -> Result<VerificationProfile, String> {
    const RUST_COMMANDS: &[&[&str]] = &[
        &["cargo", "fmt", "--all", "--", "--check"],
        &[
            "cargo",
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        &[
            "cargo",
            "test",
            "--locked",
            "--workspace",
            "--all-features",
            "--quiet",
        ],
        &[
            "cargo",
            "build",
            "--locked",
            "--workspace",
            "--all-features",
        ],
    ];
    const PYTHON_GROUP_DEV: &[&[&str]] = &[
        &[
            "uv",
            "run",
            "--locked",
            "--offline",
            "--no-python-downloads",
            "--no-env-file",
            "--no-config",
            "--group",
            "dev",
            "ruff",
            "--isolated",
            "format",
            "--check",
            ".",
        ],
        &[
            "uv",
            "run",
            "--locked",
            "--offline",
            "--no-python-downloads",
            "--no-env-file",
            "--no-config",
            "--group",
            "dev",
            "ruff",
            "--isolated",
            "check",
            ".",
        ],
        &[
            "uv",
            "run",
            "--locked",
            "--offline",
            "--no-python-downloads",
            "--no-env-file",
            "--no-config",
            "--group",
            "dev",
            "pytest",
            "-q",
        ],
    ];
    const PYTHON_EXTRAS_DEV_LAB: &[&[&str]] = &[
        &[
            "uv",
            "run",
            "--locked",
            "--offline",
            "--no-python-downloads",
            "--no-env-file",
            "--no-config",
            "--no-default-groups",
            "--extra",
            "dev",
            "--extra",
            "lab",
            "ruff",
            "--config",
            "pyproject.toml",
            "format",
            "--check",
            ".",
        ],
        &[
            "uv",
            "run",
            "--locked",
            "--offline",
            "--no-python-downloads",
            "--no-env-file",
            "--no-config",
            "--no-default-groups",
            "--extra",
            "dev",
            "--extra",
            "lab",
            "ruff",
            "--config",
            "pyproject.toml",
            "check",
            ".",
        ],
        &[
            "uv",
            "run",
            "--locked",
            "--offline",
            "--no-python-downloads",
            "--no-env-file",
            "--no-config",
            "--no-default-groups",
            "--extra",
            "dev",
            "--extra",
            "lab",
            "pytest",
            "-q",
        ],
    ];
    match profile {
        "rust_full" => Ok(VerificationProfile {
            tool: VerificationTool::Cargo,
            commands: RUST_COMMANDS,
            inputs: &["Cargo.toml", "Cargo.lock"],
            package_name: None,
        }),
        "python_uv_group_dev_v1" => Ok(VerificationProfile {
            tool: VerificationTool::Uv,
            commands: PYTHON_GROUP_DEV,
            inputs: &["pyproject.toml", "uv.lock"],
            package_name: Some("byovd-driver-pipeline"),
        }),
        "python_uv_extras_dev_lab_v1" => Ok(VerificationProfile {
            tool: VerificationTool::Uv,
            commands: PYTHON_EXTRAS_DEV_LAB,
            inputs: &["pyproject.toml", "uv.lock"],
            package_name: Some("bug-bounty-security-research-platform"),
        }),
        _ => Err("CODEX_QUEUE_VERIFY: registered verification profile is unsupported".into()),
    }
}

fn validate_python_profile_manifest(workspace: &Path, profile: &str) -> Result<(), String> {
    let path = workspace.join("pyproject.toml");
    let bytes = read_regular_bounded(&path, 1024 * 1024)?
        .ok_or("CODEX_QUEUE_VERIFY: Python profile requires a regular pyproject.toml")?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "CODEX_QUEUE_VERIFY: Python pyproject.toml is not UTF-8")?;
    let manifest: toml::Value = toml::from_str(&text)
        .map_err(|_| "CODEX_QUEUE_VERIFY: Python pyproject.toml is invalid TOML")?;
    let specification = verification_profile(profile)?;
    let name = manifest
        .get("project")
        .and_then(|project| project.get("name"))
        .and_then(toml::Value::as_str);
    if name != specification.package_name {
        return Err(
            "CODEX_QUEUE_VERIFY: Python verification profile and package name disagree".into(),
        );
    }
    let development_dependencies = match profile {
        "python_uv_group_dev_v1" => manifest
            .get("dependency-groups")
            .and_then(|groups| groups.get("dev"))
            .and_then(toml::Value::as_array),
        "python_uv_extras_dev_lab_v1" => {
            let extras = manifest
                .get("project")
                .and_then(|project| project.get("optional-dependencies"));
            if extras
                .and_then(|extras| extras.get("lab"))
                .and_then(toml::Value::as_array)
                .is_none()
            {
                return Err("CODEX_QUEUE_VERIFY: Python lab extra is absent".into());
            }
            extras
                .and_then(|extras| extras.get("dev"))
                .and_then(toml::Value::as_array)
        }
        _ => None,
    }
    .ok_or("CODEX_QUEUE_VERIFY: Python development dependencies are absent")?;
    let has_dependency = |package: &str| {
        development_dependencies.iter().any(|dependency| {
            dependency.as_str().is_some_and(|value| {
                let lower = value.to_ascii_lowercase();
                lower.strip_prefix(package).is_some_and(|suffix| {
                    suffix.is_empty()
                        || suffix.starts_with(['<', '>', '=', '!', '~', ';', '[', ' '])
                })
            })
        })
    };
    if !has_dependency("pytest") || !has_dependency("ruff") {
        return Err("CODEX_QUEUE_VERIFY: Python profile requires pytest and Ruff".into());
    }
    Ok(())
}

fn actual_profile_matches_receipt(receipt: &QueueReceipt) -> bool {
    let Some(profile) = &receipt.actual_profile else {
        return false;
    };
    profile.codex_thread_id == receipt.thread_id
        && !profile.codex_turn_id.is_empty()
        && profile.codex_turn_id.len() <= 128
        && profile
            .codex_turn_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && valid_model(&profile.model_id)
        && valid_effort(&profile.reasoning_effort)
        && profile.evidence_sha256.len() == 64
        && profile
            .evidence_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        && receipt
            .requested_model
            .as_deref()
            .is_none_or(|model| model == profile.model_id)
        && receipt
            .requested_effort
            .as_deref()
            .is_none_or(|effort| effort == profile.reasoning_effort)
}

fn valid_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Pin the inherited environment passed to each fixed verification command.
/// Exclude variables that can silently replace the profile's explicit Cargo
/// or uv/Python/Ruff/pytest behavior. This digest is audit evidence only; it
/// does not prove interpreters, toolchains, or process descendants.
fn verification_command_environment(
    tool: VerificationTool,
) -> Result<(Vec<(OsString, OsString)>, String), String> {
    const MAX_ENV_BYTES: usize = 256 * 1024;
    const MAX_ENV_ENTRIES: usize = 512;
    let mut variables = std::env::vars_os()
        .filter(|(key, _)| {
            let upper = key.to_string_lossy().to_ascii_uppercase();
            match tool {
                VerificationTool::Cargo => {
                    if matches!(upper.as_str(), "CARGO_HOME" | "RUSTUP_HOME") {
                        return true;
                    }
                    !upper.starts_with("CARGO_")
                        && !upper.starts_with("RUSTC")
                        && !upper.starts_with("RUSTDOC")
                        && !upper.starts_with("RUSTFLAGS")
                        && !upper.starts_with("RUSTUP_")
                        && !upper.starts_with("CLIPPY_")
                }
                VerificationTool::Uv => {
                    !upper.starts_with("UV_")
                        && !upper.starts_with("PYTHON")
                        && !upper.starts_with("PYTEST_")
                        && !upper.starts_with("PIP_")
                        && !upper.starts_with("RUFF_")
                        && !upper.starts_with("CONDA_")
                        && !upper.starts_with("POETRY_")
                        && !upper.starts_with("PDM_")
                        && !upper.starts_with("RYE_")
                        && upper != "VIRTUAL_ENV"
                }
            }
        })
        .collect::<Vec<_>>();
    if variables.len() > MAX_ENV_ENTRIES {
        return Err("CODEX_QUEUE_VERIFY: command environment has too many entries".into());
    }
    variables.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    let mut byte_count = 0usize;
    for (key, value) in &variables {
        let encoded = format!("{key:?}\0{value:?}\n");
        byte_count = byte_count.saturating_add(encoded.len());
        if byte_count > MAX_ENV_BYTES {
            return Err("CODEX_QUEUE_VERIFY: command environment is oversized".into());
        }
        hasher.update(encoded.as_bytes());
    }
    Ok((variables, format!("{:x}", hasher.finalize())))
}

fn resolve_trusted_verifier(
    workspace: &Path,
    control_workspace: &Path,
    tool: VerificationTool,
) -> Result<TrustedVerificationExecutable, String> {
    let registered_roots = scheduler(control_workspace)?
        .load_registry()
        .map_err(|_| "CODEX_QUEUE_VERIFY: registered project roots are unavailable")?
        .projects
        .into_iter()
        .map(|project| project.workspace)
        .collect::<Vec<_>>();
    if !registered_roots.contains(&workspace.to_path_buf()) {
        return Err("CODEX_QUEUE_VERIFY: selected workspace is not registered".into());
    }
    let path =
        std::env::var_os("PATH").ok_or("CODEX_QUEUE_VERIFY: verifier PATH is unavailable")?;
    let basename = match (tool, cfg!(windows)) {
        (VerificationTool::Cargo, true) => "cargo.exe",
        (VerificationTool::Cargo, false) => "cargo",
        (VerificationTool::Uv, true) => "uv.exe",
        (VerificationTool::Uv, false) => "uv",
    };
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(basename);
        let Ok(metadata) = fs::symlink_metadata(&candidate) else {
            continue;
        };
        if !metadata.is_file() || redirected(&metadata) {
            continue;
        }
        let Ok(executable) = candidate.canonicalize() else {
            continue;
        };
        if registered_roots
            .iter()
            .any(|project_root| executable.starts_with(project_root))
        {
            continue;
        }
        let sha256 = native_executable_sha256(&executable)
            .map_err(|_| "CODEX_QUEUE_VERIFY: verifier executable cannot be pinned")?;
        return Ok(TrustedVerificationExecutable { executable, sha256 });
    }
    Err("CODEX_QUEUE_VERIFY: no trusted verifier executable outside projects".into())
}

#[derive(Debug)]
enum VerifyCommandError {
    Safe(String),
    Uncertain(String),
}

async fn digest_verification_stream<R: tokio::io::AsyncRead + Unpin>(
    mut stream: R,
) -> Result<(String, u64), ()> {
    let mut hasher = Sha256::new();
    let mut count = 0usize;
    let mut buf = [0u8; 8192];
    loop {
        let read = stream.read(&mut buf).await.map_err(|_| ())?;
        if read == 0 {
            break;
        }
        count = count.checked_add(read).ok_or(())?;
        if count > MAX_VERIFY_STREAM_BYTES {
            return Err(());
        }
        hasher.update(&buf[..read]);
    }
    Ok((format!("{:x}", hasher.finalize()), count as u64))
}

async fn execute_fixed_verification_command(
    workspace: &Path,
    executable: &TrustedVerificationExecutable,
    tool: VerificationTool,
    argv: &[&str],
) -> Result<QueueVerificationCommand, VerifyCommandError> {
    if native_executable_sha256(&executable.executable)
        .ok()
        .as_deref()
        != Some(&executable.sha256)
    {
        return Err(VerifyCommandError::Safe(
            "CODEX_QUEUE_VERIFY: pinned verifier executable changed".into(),
        ));
    }
    let (environment, environment_sha256) =
        verification_command_environment(tool).map_err(VerifyCommandError::Safe)?;
    let mut command = Command::new(&executable.executable);
    command
        .args(&argv[1..])
        .env_clear()
        .envs(environment)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| {
        VerifyCommandError::Safe("CODEX_QUEUE_VERIFY: approved command could not start".into())
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        VerifyCommandError::Uncertain("CODEX_QUEUE_VERIFY: stdout ownership is unknown".into())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        VerifyCommandError::Uncertain("CODEX_QUEUE_VERIFY: stderr ownership is unknown".into())
    })?;
    let started = Instant::now();
    let result = tokio::time::timeout(VERIFY_COMMAND_TIMEOUT, async {
        tokio::join!(
            digest_verification_stream(stdout),
            digest_verification_stream(stderr),
            child.wait()
        )
    })
    .await;
    let (stdout, stderr, status) = match result {
        Ok(parts) => parts,
        Err(_) => {
            let _ = tokio::time::timeout(KILL_TIMEOUT, child.kill()).await;
            return Err(VerifyCommandError::Uncertain(
                "CODEX_QUEUE_VERIFY: approved command timed out; child descendants are unproven"
                    .into(),
            ));
        }
    };
    let status = status.map_err(|_| {
        VerifyCommandError::Uncertain("CODEX_QUEUE_VERIFY: command exit is unobserved".into())
    })?;
    let (stdout_sha256, stdout_bytes) = stdout.map_err(|_| {
        VerifyCommandError::Safe("CODEX_QUEUE_VERIFY: command stdout exceeded its bound".into())
    })?;
    let (stderr_sha256, stderr_bytes) = stderr.map_err(|_| {
        VerifyCommandError::Safe("CODEX_QUEUE_VERIFY: command stderr exceeded its bound".into())
    })?;
    Ok(QueueVerificationCommand {
        argv: argv.iter().map(|part| (*part).into()).collect(),
        executable: executable.executable.clone(),
        executable_sha256: executable.sha256.clone(),
        environment_sha256,
        exit_code: status.code(),
        elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        stdout_sha256,
        stderr_sha256,
        stdout_bytes,
        stderr_bytes,
        status: if status.success() {
            "EXIT_ZERO_PARENT_OBSERVED"
        } else if status.code().is_some() {
            "EXIT_NONZERO_PARENT_OBSERVED"
        } else {
            "EXIT_SIGNALLED_PARENT_OBSERVED"
        }
        .into(),
    })
}

fn resolve_trusted_git(workspace: &Path, control_workspace: &Path) -> Result<TrustedGit, String> {
    let registered_roots = scheduler(control_workspace)?
        .load_registry()
        .map_err(|_| "CODEX_QUEUE_REVIEW: registered project roots are unavailable")?
        .projects
        .into_iter()
        .map(|project| project.workspace)
        .collect::<Vec<_>>();
    if !registered_roots.contains(&workspace.to_path_buf()) {
        return Err("CODEX_QUEUE_REVIEW: selected workspace is not registered".into());
    }
    let path = std::env::var_os("PATH").ok_or("CODEX_QUEUE_REVIEW: Git PATH is unavailable")?;
    let basename = if cfg!(windows) { "git.exe" } else { "git" };
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(basename);
        let Ok(metadata) = fs::symlink_metadata(&candidate) else {
            continue;
        };
        if !metadata.is_file() || redirected(&metadata) {
            continue;
        }
        let Ok(executable) = candidate.canonicalize() else {
            continue;
        };
        if registered_roots
            .iter()
            .any(|project_root| executable.starts_with(project_root))
        {
            continue;
        }
        let sha256 = native_executable_sha256(&executable)
            .map_err(|_| "CODEX_QUEUE_REVIEW: Git executable cannot be pinned")?;
        return Ok(TrustedGit { executable, sha256 });
    }
    Err("CODEX_QUEUE_REVIEW: no trusted Git executable outside the project".into())
}

async fn git_witness_output(
    git: &TrustedGit,
    workspace: &Path,
    args: &[&str],
    limit: usize,
) -> Result<(bool, Vec<u8>), String> {
    if native_executable_sha256(&git.executable).ok().as_deref() != Some(&git.sha256) {
        return Err("CODEX_QUEUE_REVIEW: pinned Git executable changed".into());
    }
    let mut command = Command::new(&git.executable);
    command
        .args([
            "--no-optional-locks",
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(name);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "CODEX_QUEUE_REVIEW: Git could not start".to_string())?;
    tokio::time::timeout(GIT_WITNESS_TIMEOUT, async {
        let stdout = child
            .stdout
            .take()
            .ok_or("CODEX_QUEUE_REVIEW: Git output is unavailable")?;
        let mut bytes = Vec::new();
        stdout
            .take(limit.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "CODEX_QUEUE_REVIEW: Git output could not be read")?;
        if bytes.len() > limit {
            return Err("CODEX_QUEUE_REVIEW: Git output exceeds the review bound".into());
        }
        let status = child
            .wait()
            .await
            .map_err(|_| "CODEX_QUEUE_REVIEW: Git exit is unavailable")?;
        Ok((status.success(), bytes))
    })
    .await
    .map_err(|_| "CODEX_QUEUE_REVIEW: Git witness timed out".to_string())?
}

fn checked_git_path(workspace: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || relative.len() > 1024
        || relative.contains('\\')
        || relative.contains(':')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("CODEX_QUEUE_REVIEW: Git reported an unsafe path".into());
    }
    let mut path = workspace.to_path_buf();
    let parts = relative.split('/').collect::<Vec<_>>();
    let mut parent_missing = false;
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        if index + 1 == parts.len() || parent_missing {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !redirected(&metadata) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => parent_missing = true,
            _ => return Err("CODEX_QUEUE_REVIEW: Git path has a redirected ancestor".into()),
        }
    }
    Ok(path)
}

fn changed_paths_from_status(
    workspace: &Path,
    status: &[u8],
) -> Result<Vec<GitChangedPath>, String> {
    let records = status
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut changed = Vec::new();
    let mut total_file_bytes = 0u64;
    let mut index = 0;
    while index < records.len() {
        let record = std::str::from_utf8(records[index])
            .map_err(|_| "CODEX_QUEUE_REVIEW: Git path is not UTF-8")?;
        let prefix = record.as_bytes();
        if prefix.len() < 4 || prefix[2] != b' ' {
            return Err("CODEX_QUEUE_REVIEW: Git status format is invalid".into());
        }
        let relative = &record[3..];
        let path = checked_git_path(workspace, relative)?;
        if changed.len() >= MAX_GIT_PATHS {
            return Err("CODEX_QUEUE_REVIEW: too many changed paths".into());
        }
        let observation = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !redirected(&metadata) => {
                if !path
                    .canonicalize()
                    .is_ok_and(|physical| physical.starts_with(workspace))
                {
                    return Err("CODEX_QUEUE_REVIEW: changed path left its project root".into());
                }
                total_file_bytes = total_file_bytes
                    .checked_add(metadata.len())
                    .ok_or("CODEX_QUEUE_REVIEW: changed files are oversized")?;
                if metadata.len() > MAX_GIT_FILE_BYTES || total_file_bytes > MAX_GIT_FILES_BYTES {
                    return Err("CODEX_QUEUE_REVIEW: changed files exceed the review bound".into());
                }
                let bytes = read_regular_bounded(&path, MAX_GIT_FILE_BYTES)?
                    .ok_or("CODEX_QUEUE_REVIEW: changed file disappeared")?;
                GitChangedPath {
                    path: relative.into(),
                    kind: "file".into(),
                    sha256: Some(format!("{:x}", Sha256::digest(&bytes))),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => GitChangedPath {
                path: relative.into(),
                kind: "absent".into(),
                sha256: None,
            },
            _ => {
                return Err("CODEX_QUEUE_REVIEW: changed path is redirected or unsupported".into());
            }
        };
        changed.push(observation);
        index += 1;
        if matches!(prefix[0], b'R' | b'C') || matches!(prefix[1], b'R' | b'C') {
            let old = records
                .get(index)
                .ok_or("CODEX_QUEUE_REVIEW: Git rename record is incomplete")?;
            let old = std::str::from_utf8(old)
                .map_err(|_| "CODEX_QUEUE_REVIEW: Git rename path is not UTF-8")?;
            checked_git_path(workspace, old)?;
            index += 1;
        }
    }
    changed.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(changed)
}

async fn capture_git_review_witness(
    workspace: &Path,
    git: &TrustedGit,
) -> Result<GitReviewWitness, String> {
    let (ok, top) =
        git_witness_output(git, workspace, &["rev-parse", "--show-toplevel"], 4096).await?;
    let top = std::str::from_utf8(&top)
        .map_err(|_| "CODEX_QUEUE_REVIEW: Git root is not UTF-8")?
        .trim();
    if !ok || Path::new(top).canonicalize().ok().as_deref() != Some(workspace) {
        return Err("CODEX_QUEUE_REVIEW: Git top level is not the registered workspace".into());
    }
    let (has_head, head) =
        git_witness_output(git, workspace, &["rev-parse", "--verify", "HEAD"], 128).await?;
    let head_commit = if has_head {
        let value = std::str::from_utf8(&head)
            .map_err(|_| "CODEX_QUEUE_REVIEW: Git HEAD is invalid")?
            .trim();
        if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("CODEX_QUEUE_REVIEW: Git HEAD is invalid".into());
        }
        Some(value.to_owned())
    } else {
        None
    };
    let (ok, status) = git_witness_output(
        git,
        workspace,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
        MAX_GIT_STATUS_BYTES,
    )
    .await?;
    if !ok {
        return Err("CODEX_QUEUE_REVIEW: Git status failed".into());
    }
    let changed_paths = changed_paths_from_status(workspace, &status)?;
    let diff_args: &[&str] = if head_commit.is_some() {
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
        ]
    } else {
        &["diff", "--binary", "--no-ext-diff", "--no-textconv", "--"]
    };
    let (ok, diff) = git_witness_output(git, workspace, diff_args, MAX_GIT_DIFF_BYTES).await?;
    if !ok {
        return Err("CODEX_QUEUE_REVIEW: Git diff failed".into());
    }
    let index_args: &[&str] = if head_commit.is_some() {
        &[
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
        ]
    } else {
        &[
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--",
        ]
    };
    let (ok, staged) = git_witness_output(git, workspace, index_args, MAX_GIT_DIFF_BYTES).await?;
    if !ok {
        return Err("CODEX_QUEUE_REVIEW: staged Git diff failed".into());
    }
    Ok(GitReviewWitness {
        head_commit,
        status_sha256: format!("{:x}", Sha256::digest(&status)),
        tracked_diff_sha256: format!("{:x}", Sha256::digest(&diff)),
        index_diff_sha256: format!("{:x}", Sha256::digest(&staged)),
        changed_paths,
    })
}

async fn snapshot_verified_source(
    workspace: &Path,
    git: &TrustedGit,
    candidate: &QueueReviewCandidate,
) -> Result<(Vec<u8>, Vec<u8>, Vec<QueueVerificationFile>), String> {
    // The native turn can modify tracked files, stage changes, or add files.
    // A dirty initial tree or a commit during this first slice cannot be
    // attributed with the retained before/after witnesses, so refuse rather
    // than treating a clean post-commit status as an empty deliverable.
    if candidate.baseline_dirty
        || candidate.head_changed
        || candidate.before.head_commit.is_none()
        || candidate.changed_during_turn.is_empty()
        || candidate.changed_during_turn.len() != candidate.after.changed_paths.len()
    {
        return Err(
            "CODEX_QUEUE_VERIFY: source attribution requires a clean initial tree, unchanged HEAD, and a changed file"
                .into(),
        );
    }
    let (ok, patch) = git_witness_output(
        git,
        workspace,
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
        ],
        MAX_GIT_DIFF_BYTES,
    )
    .await?;
    if !ok || format!("{:x}", Sha256::digest(&patch)) != candidate.after.tracked_diff_sha256 {
        return Err(
            "CODEX_QUEUE_VERIFY: tracked patch differs from the native post-turn witness".into(),
        );
    }
    let (ok, index_patch) = git_witness_output(
        git,
        workspace,
        &[
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
        ],
        MAX_GIT_DIFF_BYTES,
    )
    .await?;
    if !ok || format!("{:x}", Sha256::digest(&index_patch)) != candidate.after.index_diff_sha256 {
        return Err(
            "CODEX_QUEUE_VERIFY: staged patch differs from the native post-turn witness".into(),
        );
    }
    let mut files = Vec::new();
    let mut total_file_bytes = 0u64;
    for changed in &candidate.after.changed_paths {
        let path = checked_git_path(workspace, &changed.path)?;
        let content = if changed.kind == "file" {
            let bytes = read_regular_bounded(&path, MAX_GIT_FILE_BYTES)?
                .ok_or("CODEX_QUEUE_VERIFY: changed source file disappeared")?;
            total_file_bytes = total_file_bytes
                .checked_add(bytes.len() as u64)
                .ok_or("CODEX_QUEUE_VERIFY: changed source files are oversized")?;
            if total_file_bytes > MAX_GIT_FILES_BYTES
                || format!("{:x}", Sha256::digest(&bytes))
                    != changed.sha256.as_deref().unwrap_or_default()
            {
                return Err(
                    "CODEX_QUEUE_VERIFY: changed source bytes exceed the bound or drifted".into(),
                );
            }
            Some(BASE64.encode(bytes))
        } else if changed.kind == "absent"
            && changed.sha256.is_none()
            && fs::symlink_metadata(&path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            None
        } else {
            return Err("CODEX_QUEUE_VERIFY: changed source path is unsupported or drifted".into());
        };
        files.push(QueueVerificationFile {
            path: changed.path.clone(),
            kind: changed.kind.clone(),
            sha256: changed.sha256.clone(),
            content_base64: content,
        });
    }
    if capture_git_review_witness(workspace, git).await? != candidate.after {
        return Err("CODEX_QUEUE_VERIFY: source changed during snapshot capture".into());
    }
    Ok((patch, index_patch, files))
}

async fn preflight_native_cli(workspace: &Path) -> Result<NativeCliPreflight, String> {
    let (selected, codex_home) = resolve_native_cli()?;
    preflight_selected_cli(selected, codex_home, workspace).await
}

async fn preflight_selected_cli(
    selected: PathBuf,
    codex_home: Option<PathBuf>,
    workspace: &Path,
) -> Result<NativeCliPreflight, String> {
    let selected_metadata = fs::symlink_metadata(&selected)
        .map_err(|_| "CODEX_QUEUE_CLI: selected executable is unavailable".to_string())?;
    if !selected_metadata.is_file() || redirected(&selected_metadata) {
        return Err("CODEX_QUEUE_CLI: selected executable is redirected or unsafe".into());
    }
    // Pin an absolute executable before switching the child's current_dir to
    // a different registered project. A relative override would otherwise
    // resolve against that project's files at spawn time.
    let executable = selected
        .canonicalize()
        .map_err(|_| "CODEX_QUEUE_CLI: selected executable cannot be resolved".to_string())?;
    let sha256 = native_executable_sha256(&executable)?;
    let (version_ok, version) = bounded_probe(
        codex_command(&executable, workspace, codex_home.as_deref()),
        &["--version"],
    )
    .await?;
    if !version_ok || !version.starts_with("codex-cli ") {
        return Err("CODEX_QUEUE_CLI: executable did not report Codex CLI version".into());
    }
    let (authenticated, _) = bounded_probe(
        codex_command(&executable, workspace, codex_home.as_deref()),
        &["login", "status"],
    )
    .await?;
    if !authenticated {
        return Err(
            "CODEX_QUEUE_AUTH: native Codex login is unavailable; queued task remains ready".into(),
        );
    }
    if native_executable_sha256(&executable)? != sha256 {
        return Err("CODEX_QUEUE_CLI: native executable changed during preflight".into());
    }
    Ok(NativeCliPreflight {
        executable,
        codex_home,
        version,
        sha256,
    })
}

fn turn_timeout() -> Result<Duration, String> {
    match std::env::var("CATDESK_CODEX_QUEUE_TURN_TIMEOUT_SECS") {
        Ok(value) => {
            let seconds: u64 = value
                .parse()
                .map_err(|_| "CODEX_QUEUE_TIMEOUT: invalid timeout")?;
            if !(60..=43_200).contains(&seconds) {
                return Err("CODEX_QUEUE_TIMEOUT: timeout must be 60 to 43200 seconds".into());
            }
            Ok(Duration::from_secs(seconds))
        }
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_TURN_TIMEOUT),
        Err(_) => Err("CODEX_QUEUE_TIMEOUT: invalid timeout".into()),
    }
}

async fn drain_native_jsonl_stdout(
    mut stdout: tokio::process::ChildStdout,
    mut evidence: CodexCliNativeStdoutEvidenceV1,
) -> (CodexCliNativeStdoutEvidenceV1, bool) {
    let mut buffer = [0u8; 8192];
    let mut line = Vec::new();
    let mut oversized_line = false;
    loop {
        let count = match stdout.read(&mut buffer).await {
            Ok(count) => count,
            Err(_) => {
                evidence.invalidate();
                return (evidence, false);
            }
        };
        if count == 0 {
            if !line.is_empty() || oversized_line {
                evidence.invalidate();
            }
            evidence.mark_eof();
            return (evidence, true);
        }
        for byte in &buffer[..count] {
            if *byte == b'\n' {
                if oversized_line {
                    evidence.invalidate();
                } else {
                    evidence.observe_line(&line);
                }
                line.clear();
                oversized_line = false;
            } else if !oversized_line {
                if line.len() < MAX_NATIVE_STDOUT_LINE_BYTES {
                    line.push(*byte);
                } else {
                    evidence.invalidate();
                    line.clear();
                    oversized_line = true;
                }
            }
        }
    }
}

async fn write_queued_prompt(
    child: &mut tokio::process::Child,
    prompt: &str,
    limit: Duration,
) -> bool {
    let Some(mut stdin) = child.stdin.take() else {
        return false;
    };
    let accepted = tokio::time::timeout(limit, async {
        stdin.write_all(prompt.as_bytes()).await?;
        stdin.flush().await
    })
    .await
    .is_ok_and(|write| write.is_ok());
    // The native `exec -` reader needs the pipe handle closed to observe
    // EOF. AsyncWriteExt::shutdown alone does not close ChildStdin on Windows.
    drop(stdin);
    accepted
}

async fn run_queued_turn(
    job: &QueueJob,
    control_workspace: &Path,
    cli: &NativeCliPreflight,
    turn_limit: Duration,
) -> TurnResult {
    let scope = job.binding.scope();
    let project = match registered_project(control_workspace, &scope) {
        Ok(project) if job.binding.matches(&project) => project,
        _ => return TurnResult::before_spawn("SCOPE_DRIFT_BEFORE_SPAWN"),
    };
    let executable = &cli.executable;
    let codex_home = cli.codex_home.as_deref();
    if native_executable_sha256(executable).ok().as_deref() != Some(&cli.sha256) {
        return TurnResult::before_spawn("NATIVE_BINARY_DRIFT_BEFORE_SPAWN");
    }
    let mut writer = match acquire_project_writer_lock(&project.workspace) {
        Ok(writer) => writer,
        Err(_) => return TurnResult::before_spawn("PROJECT_WRITER_UNAVAILABLE"),
    };
    let observed = registered_project(control_workspace, &scope);
    if !observed
        .as_ref()
        .is_ok_and(|selected| job.binding.matches(selected))
        || ensure_same_project_binding(&project, observed.as_ref().unwrap_or(&project)).is_err()
    {
        return TurnResult::before_spawn("SCOPE_DRIFT_BEFORE_SPAWN");
    }
    if ensure_no_interrupted_managed_writer(&project).is_err() {
        return TurnResult::before_spawn("MANAGED_WRITER_UNRESOLVED");
    }
    let git = match resolve_trusted_git(&project.workspace, control_workspace) {
        Ok(git) => git,
        Err(_) => return TurnResult::before_spawn("REVIEW_BASELINE_UNAVAILABLE"),
    };
    let review_before = match capture_git_review_witness(&project.workspace, &git).await {
        Ok(witness) => witness,
        Err(_) => return TurnResult::before_spawn("REVIEW_BASELINE_UNAVAILABLE"),
    };
    let action = OperatorCodexAction::Run {
        scope,
        task: job.prompt.clone(),
        model: job.model.clone(),
        effort: job.effort.clone(),
    };
    let mut args = match invocation_args(&action, &project) {
        Ok(args) => args,
        Err(_) => return TurnResult::before_spawn("NATIVE_INVOCATION_INVALID"),
    };
    let Some(resume_index) = args.iter().position(|arg| arg == "resume") else {
        return TurnResult::before_spawn("NATIVE_INVOCATION_INVALID");
    };
    args.insert(resume_index + 1, "--json".into());
    let profile_readback = prepare_resumed_native_turn_readback(codex_home, &job.binding.thread_id);
    let Some(stdout_evidence) = CodexCliNativeStdoutEvidenceV1::new(&job.binding.thread_id) else {
        return TurnResult::before_spawn("NATIVE_THREAD_INVALID");
    };
    let mut command: Command = codex_command(executable, &project.workspace, codex_home);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let Some(attempt) = job.attempt.as_ref() else {
        return TurnResult {
            marker_retired: false,
            ..TurnResult::before_spawn("QUEUE_ATTEMPT_MARKER_UNBOUND")
        };
    };
    if !attempt.marker_binding_is_valid() {
        return TurnResult {
            marker_retired: false,
            ..TurnResult::before_spawn("QUEUE_ATTEMPT_MARKER_UNBOUND")
        };
    }
    // This queue promises an attributable, locally verifiable deliverable.
    // A dirty or unborn baseline cannot satisfy that contract even if Codex
    // exits successfully. Refuse after checking legacy attempt identity but
    // before writing a marker or launching the native child.
    if review_before.head_commit.is_none() || !review_before.changed_paths.is_empty() {
        return TurnResult::before_spawn("REVIEW_BASELINE_DIRTY_BEFORE_SPAWN");
    }
    if writer.begin_queue_writer_turn(&attempt.attempt_id).is_err() {
        return TurnResult {
            marker_retired: false,
            native_executable: Some(executable.clone()),
            ..TurnResult::before_spawn("WRITER_MARKER_UNAVAILABLE")
        };
    }
    let observed_marker = inspect_project_writer_pending_marker(&project.workspace);
    if !observed_marker
        .as_ref()
        .is_ok_and(|observed| attempt.matches_writer_marker(observed.as_deref()))
    {
        return TurnResult {
            marker_retired: false,
            native_executable: Some(executable.clone()),
            ..TurnResult::before_spawn("WRITER_MARKER_BINDING_MISMATCH")
        };
    }
    let launched_at = SystemTime::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let retired = writer.finish_writer_turn().is_ok();
            return TurnResult {
                marker_retired: retired,
                native_executable: Some(executable.clone()),
                ..TurnResult::before_spawn("NATIVE_SPAWN_FAILED")
            };
        }
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = tokio::time::timeout(KILL_TIMEOUT, child.kill()).await;
        return TurnResult::unknown("NATIVE_STDOUT_UNAVAILABLE");
    };
    let stdout_reader = tokio::spawn(drain_native_jsonl_stdout(stdout, stdout_evidence));
    let prompt_accepted = write_queued_prompt(&mut child, &job.prompt, PROMPT_TIMEOUT).await;
    if !prompt_accepted {
        let _ = tokio::time::timeout(KILL_TIMEOUT, child.kill()).await;
        // The native parent may have descendants. Keep the pending writer
        // marker and scheduler lease until an explicit proof-based recovery.
        return TurnResult::unknown("PROMPT_PIPE_DESCENDANTS_UNPROVEN");
    }
    let status = match tokio::time::timeout(turn_limit, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => {
            let _ = tokio::time::timeout(KILL_TIMEOUT, child.kill()).await;
            return TurnResult::unknown("NATIVE_EXIT_UNOBSERVED");
        }
        Err(_) => {
            let _ = tokio::time::timeout(KILL_TIMEOUT, child.kill()).await;
            return TurnResult::unknown("NATIVE_TIMEOUT_DESCENDANTS_UNPROVEN");
        }
    };
    let exit_observed_at = SystemTime::now();
    let stdout_evidence = match tokio::time::timeout(NATIVE_STDOUT_EOF_TIMEOUT, stdout_reader).await
    {
        Ok(Ok((evidence, true))) => evidence,
        _ => {
            return TurnResult {
                outcome: "NATIVE_STDOUT_EOF_UNOBSERVED",
                exit_code: status.code(),
                child_exit_observed: true,
                child_absent_proven: false,
                marker_retired: false,
                native_executable: Some(executable.clone()),
                native_version: None,
                native_executable_sha256: None,
                actual_profile: None,
                review_before: Some(review_before),
                review_after: None,
            };
        }
    };
    let actual_profile = if status.success() {
        profile_readback.as_ref().and_then(|readback| {
            readback.read_completed(launched_at, exit_observed_at, &stdout_evidence)
        })
    } else {
        None
    };
    let review_after = if status.success() {
        capture_git_review_witness(&project.workspace, &git)
            .await
            .ok()
    } else {
        None
    };
    let retired = writer.finish_writer_turn().is_ok();
    TurnResult {
        outcome: if status.success() && review_after.is_none() {
            "REVIEW_POST_SNAPSHOT_UNAVAILABLE"
        } else if status.success() {
            "NATIVE_EXIT_ZERO"
        } else {
            "NATIVE_EXIT_NONZERO"
        },
        exit_code: status.code(),
        child_exit_observed: true,
        child_absent_proven: false,
        marker_retired: retired,
        native_executable: Some(executable.clone()),
        native_version: None,
        native_executable_sha256: None,
        actual_profile,
        review_before: Some(review_before),
        review_after,
    }
}

fn complete_attempt(
    control_workspace: &Path,
    queue: &QueueStore,
    claimed: &QueueJob,
    turn: TurnResult,
) -> Result<Value, String> {
    let _lock = queue.lock()?;
    let mut job = queue
        .load_job(&claimed.binding.project_id, &claimed.task_id)?
        .ok_or("CODEX_QUEUE: claimed job disappeared before receipt")?;
    if job.state != QueueState::Claimed || job.attempt != claimed.attempt {
        return Err("CODEX_QUEUE: claimed job ownership changed before receipt".into());
    }
    let attempt = job
        .attempt
        .as_ref()
        .ok_or("CODEX_QUEUE: claimed attempt is absent")?;
    let binding_still_matches = registered_project(control_workspace, &job.binding.scope())
        .as_ref()
        .is_ok_and(|project| job.binding.matches(project));
    // The attempt ID is printed at CLAIMED publication. A crash between
    // receipt publication and job update therefore leaves a discoverable
    // receipt at the same ID, without replaying the native turn.
    let receipt_id = attempt.attempt_id.clone();
    let receipt = QueueReceipt {
        schema_version: QUEUE_SCHEMA_VERSION,
        receipt_id: receipt_id.clone(),
        attempt_id: attempt.attempt_id.clone(),
        project_id: job.binding.project_id.clone(),
        task_id: job.task_id.clone(),
        workspace: job.binding.workspace.clone(),
        thread_id: job.binding.thread_id.clone(),
        target_sha256: job.binding.target_sha256.clone(),
        prompt_sha256: job.prompt_sha256.clone(),
        requested_model: job.model.clone(),
        requested_effort: job.effort.clone(),
        actual_profile: turn.actual_profile.clone(),
        native_executable: turn.native_executable.clone(),
        native_version: turn.native_version.clone(),
        native_executable_sha256: turn.native_executable_sha256.clone(),
        evidence_level: if turn.child_exit_observed {
            "NATIVE_EXIT_OBSERVED"
        } else if turn.child_absent_proven {
            "CHILD_ABSENT_BEFORE_SPAWN"
        } else {
            "PROCESS_STATE_UNKNOWN"
        }
        .into(),
        exit_code: turn.exit_code,
        child_exit_observed: turn.child_exit_observed,
        binding_still_matches,
        outcome: turn.outcome.into(),
        recorded_at_unix: unix_now()?,
    };
    queue.write_receipt(&receipt)?;
    job.receipt_id = Some(receipt_id);
    // Preserve the process receipt even if evidence capture or sidecar
    // publication fails. A review candidate is never a completion verdict.
    job.review_candidate_sha256 =
        if turn.outcome == "NATIVE_EXIT_ZERO" && binding_still_matches && turn.marker_retired {
            turn.review_before
                .clone()
                .zip(turn.review_after.clone())
                .and_then(|(before, after)| {
                    let receipt_sha = queue.persisted_receipt_sha256(&receipt).ok()?;
                    let candidate = QueueReviewCandidate::from_observed_turn(
                        &job,
                        &receipt,
                        receipt_sha,
                        before,
                        after,
                    );
                    queue.write_review_candidate(&candidate).ok()
                })
        } else {
            None
        };
    let review_missing =
        turn.outcome == "NATIVE_EXIT_ZERO" && job.review_candidate_sha256.is_none();
    let final_state = if !turn.child_exit_observed && !turn.child_absent_proven {
        QueueState::Unknown
    } else if !binding_still_matches
        || !turn.marker_retired
        || turn.outcome.contains("DRIFT")
        || turn.outcome.starts_with("REVIEW_")
        || review_missing
    {
        QueueState::Attention
    } else if turn.outcome == "NATIVE_EXIT_ZERO" {
        QueueState::NativeExitedZero
    } else if turn.child_absent_proven {
        QueueState::Ready
    } else {
        QueueState::Failed
    };
    let release_safe = turn.child_exit_observed || turn.child_absent_proven;
    // Stage attention while the lease is still held. A failed release or a
    // crash between receipt and release cannot masquerade as a completed job.
    job.state = if release_safe {
        QueueState::Attention
    } else {
        QueueState::Unknown
    };
    queue.save_job(&job)?;
    let lease_released = if release_safe {
        scheduler(control_workspace)?
            .release_workspace_lease(&attempt.lease)
            .map_err(|_| {
                "CODEX_QUEUE: observed attempt receipt saved; exact lease release failed and task needs attention"
                    .to_string()
            })?;
        job.state = final_state;
        queue.save_job(&job)?;
        true
    } else {
        false
    };
    Ok(json!({
        "action":"codex_queue_dispatch", "job":job.summary(), "receipt":receipt,
        "leaseReleased":lease_released,
        "authority":"same_user_operator_coordination",
    }))
}

async fn dispatch_one(control: &AutonomousProjectV1) -> Result<Value, String> {
    let control_workspace = &control.workspace;
    let queue = QueueStore::open_for_operator(control, control_workspace)?;
    if !queue
        .jobs()?
        .iter()
        .any(|job| job.state == QueueState::Ready)
    {
        return Ok(json!({ "action":"codex_queue_dispatch", "status":"no_ready_tasks" }));
    }
    let turn_limit = turn_timeout()?;
    let cli = preflight_native_cli(control_workspace).await?;
    let Some(job) = claim_one(control_workspace, &queue)? else {
        return Ok(json!({ "action":"codex_queue_dispatch", "status":"no_ready_tasks" }));
    };
    let turn = run_queued_turn(&job, control_workspace, &cli, turn_limit)
        .await
        .with_cli(&cli);
    complete_attempt(control_workspace, &queue, &job, turn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::autonomy_projects::{
        AutonomousProjectRegistryV1, project_chat_target_digest,
    };
    use crate::delegated::autonomy_state::{
        AutonomousQueueTaskStateV1, AutonomousQueueTaskV1, AutonomousQueueV1,
        AutonomousSessionStateV1, AutonomousStateStoreV1,
    };
    use std::io::IsTerminal;
    use std::process::Command as StdCommand;

    fn parse(items: &[&str]) -> Result<QueueAction, String> {
        parse_action(
            &items
                .iter()
                .map(|item| (*item).to_string())
                .collect::<Vec<_>>(),
        )
    }

    fn mock_git_witness() -> GitReviewWitness {
        GitReviewWitness {
            head_commit: None,
            status_sha256: format!("{:x}", Sha256::digest([])),
            tracked_diff_sha256: format!("{:x}", Sha256::digest([])),
            index_diff_sha256: format!("{:x}", Sha256::digest([])),
            changed_paths: Vec::new(),
        }
    }

    fn fixture_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("catdesk-operator-queue-{}", Uuid::new_v4()));
        fs::create_dir(&root).expect("fixture root");
        root
    }

    pub(super) fn cleanup(root: PathBuf) {
        let root = root.canonicalize().expect("fixture root canonical");
        let temp = std::env::temp_dir().canonicalize().expect("temp canonical");
        assert!(root != temp && root.starts_with(&temp));
        fs::remove_dir_all(root).expect("fixture cleanup");
    }

    fn fixture_git(root: &Path, origin: &str) {
        fs::create_dir(root).expect("Git root");
        for args in [vec!["init", "-q"], vec!["remote", "add", "origin", origin]] {
            assert!(
                StdCommand::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(args)
                    .status()
                    .expect("Git available")
                    .success()
            );
        }
    }

    fn fake_native_cli(root: &Path) -> PathBuf {
        let source = root.join("fake-codex.rs");
        let executable = root.join(if cfg!(windows) {
            "fake-codex.exe"
        } else {
            "fake-codex"
        });
        fs::write(&source, r#"
            use std::io::Read;
            fn main() {
                let args = std::env::args().skip(1).collect::<Vec<_>>();
                if args.iter().any(|arg| arg == "--version") { println!("codex-cli fake-fixture"); return; }
                if args.windows(2).any(|pair| pair == ["login", "status"]) { println!("logged in"); return; }
                let mut prompt = String::new();
                std::io::stdin().read_to_string(&mut prompt).expect("prompt");
                let thread = args.get(args.len().saturating_sub(2)).expect("thread");
                std::fs::write(format!("queue-proof-{thread}.txt"), prompt).expect("proof");
                println!("fixture native turn complete");
            }
        "#).unwrap();
        assert!(
            StdCommand::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .expect("rustc available")
                .success()
        );
        executable
    }

    #[tokio::test]
    async fn queued_prompt_closes_native_pipe_and_times_out_a_blocked_write() {
        let root = fixture_root();
        let source = root.join("prompt-child.rs");
        let executable = root.join(if cfg!(windows) {
            "prompt-child.exe"
        } else {
            "prompt-child"
        });
        fs::write(
            &source,
            r#"
                use std::io::Read;
                use std::time::Duration;
                fn main() {
                    match std::env::args().nth(1).as_deref() {
                        Some("eof") => {
                            let mut bytes = Vec::new();
                            std::io::stdin().read_to_end(&mut bytes).unwrap();
                            if bytes != b"queued prompt through EOF" {
                                std::process::exit(42);
                            }
                            std::fs::write("prompt-proof.txt", bytes).unwrap();
                        }
                        Some("blocked") => std::thread::sleep(Duration::from_secs(10)),
                        _ => std::process::exit(43),
                    }
                }
            "#,
        )
        .expect("write prompt child source");
        assert!(
            StdCommand::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .expect("rustc available")
                .success()
        );

        let mut eof_child = Command::new(&executable)
            .arg("eof")
            .current_dir(&root)
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("EOF reader starts");
        assert!(
            write_queued_prompt(
                &mut eof_child,
                "queued prompt through EOF",
                Duration::from_secs(5),
            )
            .await
        );
        assert!(eof_child.stdin.is_none(), "parent stdin handle must close");
        let status = tokio::time::timeout(Duration::from_secs(5), eof_child.wait())
            .await
            .expect("EOF reader exits promptly")
            .expect("EOF reader status");
        assert!(status.success());
        assert_eq!(
            fs::read(root.join("prompt-proof.txt")).expect("exact prompt proof"),
            b"queued prompt through EOF"
        );

        let mut blocked_child = Command::new(&executable)
            .arg("blocked")
            .current_dir(&root)
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("blocked reader starts");
        let oversized_prompt = "P".repeat(4 * 1024 * 1024);
        assert!(
            !write_queued_prompt(
                &mut blocked_child,
                &oversized_prompt,
                Duration::from_millis(100),
            )
            .await,
            "blocked prompt must time out without an EOF claim"
        );
        assert!(
            blocked_child.stdin.is_none(),
            "failed prompt still closes the parent handle"
        );
        tokio::time::timeout(Duration::from_secs(5), blocked_child.kill())
            .await
            .expect("blocked child stop is bounded")
            .expect("blocked child stopped");
        cleanup(root);
    }

    fn fixture_project(id: &str, root: &Path) -> AutonomousProjectV1 {
        let target = format!("https://chatgpt.com/c/{}-fixture", id);
        AutonomousProjectV1 {
            project_id: id.into(),
            workspace: root.canonicalize().expect("project canonical"),
            git_identity: format!("https://example.test/{id}.git"),
            verification_profile: "rust_full".into(),
            codex_thread_id: Some(Uuid::new_v4().to_string()),
            chatgpt_target_sha256: Some(project_chat_target_digest(&target)),
            chatgpt_target_url: Some(target),
        }
    }

    fn three_project_fixture(
        max_codex_workers: u32,
    ) -> (PathBuf, Vec<AutonomousProjectV1>, QueueStore) {
        three_project_fixture_with_alpha_profile(max_codex_workers, "rust_full")
    }

    fn three_project_fixture_with_alpha_profile(
        max_codex_workers: u32,
        alpha_profile: &str,
    ) -> (PathBuf, Vec<AutonomousProjectV1>, QueueStore) {
        let fixture = fixture_root();
        let control = fixture.join("catdesk");
        let alpha = fixture.join("alpha");
        let beta = fixture.join("beta");
        fixture_git(&control, "https://example.test/catdesk.git");
        fixture_git(&alpha, "https://example.test/alpha.git");
        fixture_git(&beta, "https://example.test/beta.git");
        let registry = AutonomousProjectRegistryStoreV1::open(control.join(".catdesk/projects"))
            .expect("registry store");
        registry
            .initialize(max_codex_workers, 1)
            .expect("registry initialized");
        let projects = [(&"catdesk", &control), (&"alpha", &alpha), (&"beta", &beta)]
            .into_iter()
            .map(|(id, root)| {
                let mut project = fixture_project(id, root);
                if *id == "alpha" {
                    project.verification_profile = alpha_profile.into();
                }
                project
            })
            .collect::<Vec<_>>();
        for project in &projects {
            registry
                .register_project(project.clone())
                .expect("registered");
        }
        let queue = QueueStore::open_at(fixture.join("user-state/OperatorCodexQueue"))
            .or_else(|_| {
                fs::create_dir(fixture.join("user-state")).expect("user state parent");
                QueueStore::open_at(fixture.join("user-state/OperatorCodexQueue"))
            })
            .expect("queue");
        (fixture, projects, queue)
    }

    fn fixture_git_command(root: &Path, args: &[&str]) {
        assert!(
            StdCommand::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .status()
                .expect("fixture Git")
                .success(),
            "fixture Git command failed: {args:?}"
        );
    }

    fn fixture_commit(root: &Path) {
        fixture_git_command(root, &["add", "--all"]);
        fixture_git_command(
            root,
            &[
                "-c",
                "user.name=CatDesk Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "-qm",
                "fixture baseline",
            ],
        );
    }

    fn snapshot_non_git_tree(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
        fn visit(root: &Path, path: &Path, rows: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
            let relative = path.strip_prefix(root).expect("fixture relative path");
            if relative.components().any(|part| part.as_os_str() == ".git") {
                return;
            }
            let metadata = fs::symlink_metadata(path).expect("fixture metadata");
            if metadata.is_dir() {
                rows.push((relative.to_path_buf(), None));
                let mut children = fs::read_dir(path)
                    .expect("fixture directory")
                    .map(|entry| entry.expect("fixture entry").path())
                    .collect::<Vec<_>>();
                children.sort();
                for child in children {
                    visit(root, &child, rows);
                }
            } else {
                rows.push((
                    relative.to_path_buf(),
                    Some(fs::read(path).expect("fixture file")),
                ));
            }
        }
        let mut rows = Vec::new();
        visit(root, root, &mut rows);
        rows
    }

    #[tokio::test]
    async fn queue_preflight_is_candidate_only_and_never_initializes_missing_scheduler() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0].workspace;
        let alpha = &projects[1];
        fs::write(
            alpha.workspace.join("Cargo.toml"),
            "[package]\nname = \"preflight_fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(alpha.workspace.join("Cargo.lock"), "version = 3\n").unwrap();
        fixture_commit(&alpha.workspace);
        let command = [
            "catdesk".to_string(),
            "codex".into(),
            "queue".into(),
            "preflight".into(),
            "--project-id".into(),
            "alpha".into(),
            "--workspace".into(),
            alpha.workspace.to_string_lossy().into_owned(),
        ];
        assert!(matches!(
            parse_action(&command),
            Ok(QueueAction::Preflight { .. })
        ));
        let mut unsupported = command.to_vec();
        unsupported.extend(["--task-id".into(), "unapproved".into()]);
        assert!(parse_action(&unsupported).is_err());
        let leases = control.join(".catdesk/projects/leases");
        fs::remove_dir(&leases).expect("empty fixture leases removed");
        let before = snapshot_non_git_tree(&fixture);
        let report = execute_action(
            QueueAction::Preflight {
                scope: CodexScope {
                    project_id: "alpha".into(),
                    workspace: alpha.workspace.clone(),
                },
            },
            control,
        )
        .await
        .expect("read-only preflight");
        assert_eq!(report["status"], "CANDIDATE_ONLY");
        assert_eq!(report["blockers"], json!([]));
        assert_eq!(report["profileInputsValid"], true);
        assert_eq!(report["git"]["clean"], true);
        assert_eq!(report["readOnly"], true);
        assert_eq!(report["launchAuthorized"], false);
        assert_eq!(report["writerLockChecked"], false);
        assert_eq!(report["operatorQueueAndLeaseChecked"], false);
        assert!(!leases.exists());
        assert_eq!(snapshot_non_git_tree(&fixture), before);
        assert!(queue.jobs().unwrap().is_empty());

        fs::write(alpha.workspace.join("untracked.txt"), b"owner work").unwrap();
        let before_dirty = snapshot_non_git_tree(&fixture);
        let dirty = preflight_project(
            control,
            &CodexScope {
                project_id: "alpha".into(),
                workspace: alpha.workspace.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(dirty["status"], "BLOCKED");
        assert!(
            dirty["blockers"]
                .as_array()
                .unwrap()
                .contains(&json!("REVIEW_BASELINE_DIRTY_OR_UNBORN"))
        );
        assert_eq!(snapshot_non_git_tree(&fixture), before_dirty);
        cleanup(fixture);
    }

    #[tokio::test]
    async fn queue_preflight_reports_profile_and_managed_claim_without_mutation() {
        let (fixture, projects, queue) =
            three_project_fixture_with_alpha_profile(1, "python_uv_group_dev_v1");
        let control = &projects[0].workspace;
        let alpha = &projects[1];
        fs::write(
            alpha.workspace.join("pyproject.toml"),
            "[project]\nname = \"wrong-package\"\nversion = \"0.1.0\"\n[dependency-groups]\ndev = [\"pytest>=8\", \"ruff>=0.8\"]\n",
        )
        .unwrap();
        fs::write(
            alpha.workspace.join("uv.lock"),
            "version = 1\n\n[[package]]\nname = \"wrong-package\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fixture_commit(&alpha.workspace);
        let managed = AutonomousStateStoreV1::open(alpha.workspace.join(".catdesk/autonomy"))
            .expect("managed state fixture");
        managed
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WorkerRunning,
                }])
                .expect("managed queue"),
            )
            .expect("managed session");
        let mut snapshot = managed.load_session("session_one").unwrap();
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        managed.save_session(&snapshot).unwrap();
        let before = snapshot_non_git_tree(&fixture);
        let report = preflight_project(
            control,
            &CodexScope {
                project_id: "alpha".into(),
                workspace: alpha.workspace.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(report["status"], "BLOCKED");
        assert_eq!(report["profileInputsValid"], false);
        assert!(
            report["blockers"]
                .as_array()
                .unwrap()
                .contains(&json!("VERIFICATION_PROFILE_INPUTS_UNAVAILABLE"))
        );
        assert!(
            report["blockers"]
                .as_array()
                .unwrap()
                .contains(&json!("MANAGED_OWNERSHIP_OR_ACCOUNTING_UNRESOLVED"))
        );
        assert_eq!(report["recordedTurnGate"]["queueBlockerCount"], 1);
        assert_eq!(snapshot_non_git_tree(&fixture), before);
        assert!(queue.jobs().unwrap().is_empty());
        cleanup(fixture);
    }

    #[tokio::test]
    async fn queue_preflight_rejects_ignored_profile_input_on_clean_git() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0].workspace;
        let alpha = &projects[1];
        fs::write(alpha.workspace.join(".gitignore"), "Cargo.lock\n").unwrap();
        fs::write(
            alpha.workspace.join("Cargo.toml"),
            "[package]\nname = \"preflight_fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fixture_commit(&alpha.workspace);
        fs::write(alpha.workspace.join("Cargo.lock"), "version = 3\n").unwrap();
        let before = snapshot_non_git_tree(&fixture);
        let report = preflight_project(
            control,
            &CodexScope {
                project_id: "alpha".into(),
                workspace: alpha.workspace.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(report["git"]["clean"], true);
        assert_eq!(report["profileInputsValid"], true);
        assert_eq!(report["profileInputsTracked"], false);
        assert_eq!(report["status"], "BLOCKED");
        assert!(
            report["blockers"]
                .as_array()
                .unwrap()
                .contains(&json!("VERIFICATION_PROFILE_INPUTS_UNTRACKED_OR_UNSAFE"))
        );
        assert_eq!(snapshot_non_git_tree(&fixture), before);
        assert!(queue.jobs().unwrap().is_empty());
        cleanup(fixture);
    }

    fn fixture_python_profile_inputs(project: &AutonomousProjectV1, profile: &str) {
        let (name, manifest) = match profile {
            "python_uv_group_dev_v1" => (
                "byovd-driver-pipeline",
                "[project]\nname = \"byovd-driver-pipeline\"\nversion = \"0.1.0\"\n[dependency-groups]\ndev = [\"pytest>=8\", \"ruff>=0.8\"]\n",
            ),
            "python_uv_extras_dev_lab_v1" => (
                "bug-bounty-security-research-platform",
                "[project]\nname = \"bug-bounty-security-research-platform\"\nversion = \"0.1.0\"\n[project.optional-dependencies]\ndev = [\"pytest>=8\", \"ruff>=0.9\"]\nlab = [\"flask>=3\"]\n",
            ),
            _ => panic!("unsupported fixture profile"),
        };
        fs::write(project.workspace.join("pyproject.toml"), manifest).expect("fixture manifest");
        fs::write(
            project.workspace.join("uv.lock"),
            format!("version = 1\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        )
        .expect("fixture lock");
    }

    async fn fixture_profile_bind(
        control_workspace: &Path,
        queue: &QueueStore,
        selected: &AutonomousProjectV1,
        profile: &str,
        hash: &str,
    ) -> Result<Value, String> {
        bind_profile_in_store(
            control_workspace,
            queue,
            selected,
            "rust_full",
            profile,
            hash,
            exact_thread(selected)?,
        )
        .await
    }

    fn fixture_candidate(
        project: &AutonomousProjectV1,
        before: GitReviewWitness,
        after: GitReviewWitness,
    ) -> QueueReviewCandidate {
        let changed_during_turn = after
            .changed_paths
            .iter()
            .map(|changed| changed.path.clone())
            .collect();
        QueueReviewCandidate {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: Uuid::new_v4().to_string(),
            receipt_sha256: "0".repeat(64),
            project_id: project.project_id.clone(),
            task_id: "fixture".into(),
            workspace: project.workspace.clone(),
            thread_id: project.codex_thread_id.clone().unwrap(),
            prompt_sha256: "0".repeat(64),
            requested_model: None,
            requested_effort: None,
            actual_profile: None,
            baseline_dirty: before.head_commit.is_none() || !before.changed_paths.is_empty(),
            head_changed: before.head_commit != after.head_commit,
            changed_during_turn,
            before,
            after,
            review_status: "UNREVIEWED".into(),
            verification_status: "NOT_VERIFIED_BY_CATDESK".into(),
            wake_ready: false,
        }
    }

    fn fixture_cargo(root: &Path, fail_clippy: bool) -> TrustedVerificationExecutable {
        let source = root.join("fake-cargo.rs");
        let executable = root.join(if cfg!(windows) {
            "fake-cargo.exe"
        } else {
            "fake-cargo"
        });
        let program = r#"
            fn main() {
                let args = std::env::args().skip(1).collect::<Vec<_>>();
                let subcommand = args.first().map(String::as_str).unwrap_or("");
                if !matches!(subcommand, "fmt" | "clippy" | "test" | "build") {
                    std::process::exit(41);
                }
                if subcommand != "fmt" && !args.iter().any(|arg| arg == "--locked") {
                    std::process::exit(42);
                }
                println!("fixture cargo {subcommand}");
                eprintln!("fixture stderr {subcommand}");
                if FAIL_CLIPPY && subcommand == "clippy" {
                    std::process::exit(7);
                }
            }
        "#;
        fs::write(
            &source,
            program.replace("FAIL_CLIPPY", if fail_clippy { "true" } else { "false" }),
        )
        .unwrap();
        assert!(
            StdCommand::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .unwrap()
                .success()
        );
        let executable = executable.canonicalize().unwrap();
        let sha256 = native_executable_sha256(&executable).unwrap();
        TrustedVerificationExecutable { executable, sha256 }
    }

    fn fixture_uv(
        root: &Path,
        profile: &str,
        fail_ruff_check: bool,
    ) -> TrustedVerificationExecutable {
        let source = root.join("fake-uv.rs");
        let executable = root.join(if cfg!(windows) {
            "fake-uv.exe"
        } else {
            "fake-uv"
        });
        let program = r#"
            fn main() {
                let args = std::env::args().skip(1).collect::<Vec<_>>();
                if args.first().map(String::as_str) != Some("run") {
                    std::process::exit(41);
                }
                for flag in ["--locked", "--offline", "--no-python-downloads", "--no-env-file", "--no-config"] {
                    if !args.iter().any(|arg| arg == flag) { std::process::exit(42); }
                }
                if PROFILE == "python_uv_group_dev_v1" {
                    if !args.windows(2).any(|pair| pair == ["--group", "dev"]) {
                        std::process::exit(43);
                    }
                } else if !args.windows(2).any(|pair| pair == ["--extra", "dev"])
                    || !args.windows(2).any(|pair| pair == ["--extra", "lab"])
                    || !args.iter().any(|arg| arg == "--no-default-groups") {
                    std::process::exit(44);
                }
                let ruff_format = args.windows(2).any(|pair| pair == ["format", "--check"]);
                let ruff_check = args.windows(2).any(|pair| pair == ["check", "."]);
                let pytest = args.windows(2).any(|pair| pair == ["pytest", "-q"]);
                if !ruff_format && !ruff_check && !pytest {
                    std::process::exit(45);
                }
                if (ruff_format || ruff_check) && PROFILE == "python_uv_group_dev_v1"
                    && !args.windows(2).any(|pair| pair == ["ruff", "--isolated"]) {
                    std::process::exit(46);
                }
                if (ruff_format || ruff_check) && PROFILE == "python_uv_extras_dev_lab_v1"
                    && !args.windows(3).any(|triple| triple == ["ruff", "--config", "pyproject.toml"]) {
                    std::process::exit(47);
                }
                println!("fixture uv {}", args.join(" "));
                eprintln!("fixture stderr");
                if FAIL_RUFF_CHECK && ruff_check {
                    std::process::exit(7);
                }
            }
        "#;
        fs::write(
            &source,
            program.replace("PROFILE", &format!("{profile:?}")).replace(
                "FAIL_RUFF_CHECK",
                if fail_ruff_check { "true" } else { "false" },
            ),
        )
        .unwrap();
        assert!(
            StdCommand::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .expect("rustc available")
                .success()
        );
        let executable = executable.canonicalize().unwrap();
        let sha256 = native_executable_sha256(&executable).unwrap();
        TrustedVerificationExecutable { executable, sha256 }
    }

    pub(super) async fn completed_verified_fixture(
        fail_clippy: bool,
    ) -> (
        PathBuf,
        Vec<AutonomousProjectV1>,
        QueueStore,
        String,
        TrustedVerificationExecutable,
    ) {
        completed_verified_fixture_for_profile("rust_full", fail_clippy).await
    }

    async fn completed_verified_fixture_for_profile(
        profile: &str,
        fail_command: bool,
    ) -> (
        PathBuf,
        Vec<AutonomousProjectV1>,
        QueueStore,
        String,
        TrustedVerificationExecutable,
    ) {
        completed_verified_fixture_for_profile_with_source(profile, fail_command, b"after\n").await
    }

    async fn completed_verified_fixture_for_profile_with_source(
        profile: &str,
        fail_command: bool,
        source_after: &[u8],
    ) -> (
        PathBuf,
        Vec<AutonomousProjectV1>,
        QueueStore,
        String,
        TrustedVerificationExecutable,
    ) {
        let (fixture, projects, queue) = three_project_fixture_with_alpha_profile(1, profile);
        let workspace = &projects[1].workspace;
        match profile {
            "rust_full" => {
                fs::write(
                    workspace.join("Cargo.toml"),
                    b"[package]\nname = \"queue-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ).unwrap();
                fs::write(workspace.join("Cargo.lock"), b"# fixture lock\n").unwrap();
            }
            "python_uv_group_dev_v1" => {
                fs::write(workspace.join("pyproject.toml"), b"[project]\nname = \"byovd-driver-pipeline\"\nversion = \"0.1.0\"\n[dependency-groups]\ndev = [\"pytest>=8\", \"ruff>=0.8\"]\n").unwrap();
                fs::write(workspace.join("uv.lock"), b"version = 1\n").unwrap();
            }
            "python_uv_extras_dev_lab_v1" => {
                fs::write(workspace.join("pyproject.toml"), b"[project]\nname = \"bug-bounty-security-research-platform\"\nversion = \"0.1.0\"\n[project.optional-dependencies]\ndev = [\"pytest>=8\", \"ruff>=0.9\"]\nlab = [\"flask>=3\"]\n").unwrap();
                fs::write(workspace.join("uv.lock"), b"version = 1\n").unwrap();
            }
            _ => panic!("unknown test profile"),
        }
        fs::write(workspace.join("source.txt"), b"before\n").unwrap();
        fixture_commit(workspace);
        let git = resolve_trusted_git(workspace, &projects[0].workspace).unwrap();
        let before = capture_git_review_witness(workspace, &git).await.unwrap();
        let job = QueueJob::new(
            "verified_fixture".into(),
            &projects[1],
            "fixture edit".into(),
            Some("gpt-6.1-sol".into()),
            Some("medium".into()),
        )
        .unwrap();
        queue.save_job(&job).unwrap();
        let mut claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let attempt = claimed.attempt.as_ref().unwrap();
        let receipt_id = attempt.attempt_id.clone();
        fs::write(workspace.join("source.txt"), source_after).unwrap();
        fs::write(workspace.join("new.txt"), b"new\n").unwrap();
        let after = capture_git_review_witness(workspace, &git).await.unwrap();
        let receipt = QueueReceipt {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: receipt_id.clone(),
            attempt_id: receipt_id.clone(),
            project_id: claimed.binding.project_id.clone(),
            task_id: claimed.task_id.clone(),
            workspace: claimed.binding.workspace.clone(),
            thread_id: claimed.binding.thread_id.clone(),
            target_sha256: claimed.binding.target_sha256.clone(),
            prompt_sha256: claimed.prompt_sha256.clone(),
            requested_model: claimed.model.clone(),
            requested_effort: claimed.effort.clone(),
            actual_profile: Some(CodexCliTurnProfileEvidenceV1 {
                codex_thread_id: claimed.binding.thread_id.clone(),
                codex_turn_id: Uuid::new_v4().to_string(),
                model_id: "gpt-6.1-sol".into(),
                reasoning_effort: "medium".into(),
                evidence_sha256: "b".repeat(64),
            }),
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
            evidence_level: "NATIVE_EXIT_OBSERVED".into(),
            exit_code: Some(0),
            child_exit_observed: true,
            binding_still_matches: true,
            outcome: "NATIVE_EXIT_ZERO".into(),
            recorded_at_unix: unix_now().unwrap(),
        };
        queue.write_receipt(&receipt).unwrap();
        let candidate = QueueReviewCandidate::from_observed_turn(
            &claimed,
            &receipt,
            queue.persisted_receipt_sha256(&receipt).unwrap(),
            before,
            after,
        );
        let candidate_sha = queue.write_review_candidate(&candidate).unwrap();
        claimed.state = QueueState::NativeExitedZero;
        claimed.receipt_id = Some(receipt_id.clone());
        claimed.review_candidate_sha256 = Some(candidate_sha);
        queue.save_job(&claimed).unwrap();
        let executable = if profile == "rust_full" {
            fixture_cargo(&fixture, fail_command)
        } else {
            fixture_uv(&fixture, profile, fail_command)
        };
        (fixture, projects, queue, receipt_id, executable)
    }

    #[tokio::test]
    async fn review_package_copies_bound_source_and_is_idempotent_after_workspace_drift() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let first = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        assert_eq!(first["idempotent"], false);
        assert_eq!(first["package"]["independentReview"], "PENDING");
        assert_eq!(first["package"]["wakeReady"], false);
        assert_eq!(
            first["package"]["verificationStatus"],
            "LOCAL_COMMANDS_EXITED_ZERO_SOURCE_RECHECKED"
        );
        let bound_job = queue
            .load_job("alpha", "verified_fixture")
            .unwrap()
            .unwrap();
        let mut incomplete = bound_job.clone();
        incomplete.verification_sha256 = None;
        assert!(validate_job(&incomplete).is_err());
        incomplete = bound_job;
        incomplete.state = QueueState::Attention;
        assert!(validate_job(&incomplete).is_err());
        let original_digest = first["package"]["artifactSha256"].as_str().unwrap();
        let receipt = queue.read_receipt(&receipt_id).unwrap();
        let candidate = queue.read_review_candidate(&receipt).unwrap();
        let (verification, verification_sha256) = queue
            .read_verification(&receipt, &candidate)
            .unwrap()
            .unwrap();
        let (package, readback_digest) = queue
            .read_review_package(&receipt, &candidate, &verification, &verification_sha256)
            .unwrap()
            .unwrap();
        assert_eq!(readback_digest, original_digest);
        assert_eq!(package.verification.patch_base64, verification.patch_base64);
        assert_eq!(
            package.verification.index_patch_base64,
            verification.index_patch_base64
        );
        assert_eq!(package.verification.files, verification.files);
        assert_eq!(package.verification.commands, verification.commands);
        assert_eq!(package.receipt.actual_profile, verification.actual_profile);
        assert_eq!(package.prompt, "fixture edit");
        assert!(first["package"].get("prompt").is_none());
        fs::write(projects[1].workspace.join("source.txt"), b"changed later\n").unwrap();
        let second = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        assert_eq!(second["idempotent"], true);
        assert_eq!(second["package"]["artifactSha256"], original_digest);
        cleanup(fixture);
    }

    #[tokio::test]
    async fn review_invitation_preparation_requires_consent_and_carries_no_review_authority() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let packaged = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = packaged["package"]["artifactSha256"].as_str().unwrap();
        let package_before = fs::read(queue.review_package_path(&receipt_id)).unwrap();
        let job_before = fs::read(queue.job_path("alpha", "verified_fixture")).unwrap();
        assert!(
            review_invite::prepare_in_store_for_test(&queue, &projects[1], &receipt_id, sha)
                .unwrap_err()
                .contains("disclosure grant is required")
        );
        let grant = review_reader::grant_in_store(&queue, &projects[1], &receipt_id, sha).unwrap();
        let grant_created_at_unix = grant["createdAtUnix"].as_u64().unwrap();
        let grant_expires_at_unix = grant["expiresAtUnix"].as_u64().unwrap();
        let invitation_path = queue
            .root
            .join("receipts")
            .join(format!("{receipt_id}.review-invitation.json"));
        assert!(grant_created_at_unix > 0);
        for outside_grant in [grant_created_at_unix - 1, grant_expires_at_unix] {
            assert!(
                review_invite::prepare_in_store_at_for_test(
                    &queue,
                    &projects[1],
                    &receipt_id,
                    sha,
                    outside_grant,
                )
                .unwrap_err()
                .contains("outside its lifetime")
            );
            assert!(!invitation_path.exists());
        }
        let first =
            review_invite::prepare_in_store_for_test(&queue, &projects[1], &receipt_id, sha)
                .unwrap();
        assert_eq!(
            first["disclosureCreatedAtUnixObserved"],
            grant_created_at_unix
        );
        assert_eq!(
            first["disclosureExpiresAtUnixObserved"],
            grant_expires_at_unix
        );
        let invitation_before = fs::read(&invitation_path).unwrap();
        for outside_grant in [grant_created_at_unix - 1, grant_expires_at_unix] {
            assert!(
                review_invite::prepare_in_store_at_for_test(
                    &queue,
                    &projects[1],
                    &receipt_id,
                    sha,
                    outside_grant,
                )
                .unwrap_err()
                .contains("outside its lifetime")
            );
            assert_eq!(fs::read(&invitation_path).unwrap(), invitation_before);
        }
        let second =
            review_invite::prepare_in_store_for_test(&queue, &projects[1], &receipt_id, sha)
                .unwrap();
        assert_eq!(first["invitationSha256"], second["invitationSha256"]);
        assert_eq!(first["idempotent"], false);
        assert_eq!(second["idempotent"], true);
        assert_eq!(first["invitation"]["wakePublication"], "NOT_PUBLISHED");
        assert_eq!(first["invitation"]["independentReview"], "PENDING");
        assert_eq!(first["invitation"]["wakeReady"], false);
        assert_eq!(first["invitation"]["projectId"], "alpha");
        assert_eq!(first["invitation"]["receiptId"], receipt_id);
        assert_eq!(first["invitation"]["packageSha256"], sha);
        assert_eq!(
            catdesk_wake::protocol::digest(projects[1].chatgpt_target_url.as_deref().unwrap()),
            project_chat_target_digest(projects[1].chatgpt_target_url.as_deref().unwrap())
        );
        assert_eq!(
            first["invitation"]["targetSha256"],
            projects[1].chatgpt_target_sha256.as_deref().unwrap()
        );
        assert!(
            first["invitation"]["message"]
                .as_str()
                .unwrap()
                .contains("not an independent review decision or approval")
        );
        assert!(first["invitation"].get("targetUrl").is_none());
        assert!(first["invitation"].get("prompt").is_none());
        assert_eq!(
            fs::read(queue.review_package_path(&receipt_id)).unwrap(),
            package_before
        );
        assert_eq!(
            fs::read(queue.job_path("alpha", "verified_fixture")).unwrap(),
            job_before
        );
        assert_eq!(fs::read(&invitation_path).unwrap(), invitation_before);

        assert!(
            review_invite::prepare_in_store_for_test(&queue, &projects[2], &receipt_id, sha)
                .is_err()
        );
        assert!(
            review_invite::prepare_in_store_for_test(
                &queue,
                &projects[1],
                &receipt_id,
                &"0".repeat(64)
            )
            .is_err()
        );
        let mut unsupported_target = projects[1].clone();
        unsupported_target.chatgpt_target_url = Some("https://chatgpt.com/g/alpha/c/exact".into());
        unsupported_target.chatgpt_target_sha256 = Some(project_chat_target_digest(
            unsupported_target.chatgpt_target_url.as_deref().unwrap(),
        ));
        assert!(review_invite::registered_wake_target_sha256(&unsupported_target).is_err());
        let mut rebound = projects[1].clone();
        rebound.chatgpt_target_url = Some("https://chatgpt.com/c/alpha-rebound".into());
        rebound.chatgpt_target_sha256 = Some(project_chat_target_digest(
            rebound.chatgpt_target_url.as_deref().unwrap(),
        ));
        assert!(
            review_invite::prepare_in_store_for_test(&queue, &rebound, &receipt_id, sha).is_err(),
            "the original invitation cannot authorize a rebound target"
        );
        let mut altered: Value = serde_json::from_slice(&invitation_before).unwrap();
        altered["message"] = json!("approval granted");
        fs::write(&invitation_path, serde_json::to_vec(&altered).unwrap()).unwrap();
        assert!(
            review_invite::prepare_in_store_for_test(&queue, &projects[1], &receipt_id, sha)
                .unwrap_err()
                .contains("binding changed")
        );
        fs::write(&invitation_path, &invitation_before).unwrap();
        let mut whitespace_tamper = invitation_before.clone();
        whitespace_tamper.push(b' ');
        fs::write(&invitation_path, whitespace_tamper).unwrap();
        assert!(
            review_invite::prepare_in_store_for_test(&queue, &projects[1], &receipt_id, sha)
                .unwrap_err()
                .contains("bytes changed")
        );
        fs::write(&invitation_path, &invitation_before).unwrap();
        let original_text = String::from_utf8(invitation_before.clone()).unwrap();
        let original_record: Value = serde_json::from_str(&original_text).unwrap();
        let prepared_at_unix = original_record["preparedAtUnix"].as_u64().unwrap();
        let old_checksum = original_record["checksumSha256"].as_str().unwrap();
        let restamp = |new_time: u64| {
            let without_checksum = original_text
                .trim_end_matches('\n')
                .replacen(
                    &format!("\"preparedAtUnix\":{prepared_at_unix}"),
                    &format!("\"preparedAtUnix\":{new_time}"),
                    1,
                )
                .replacen(
                    &format!("\"checksumSha256\":\"{old_checksum}\""),
                    "\"checksumSha256\":\"\"",
                    1,
                );
            let mut hasher = Sha256::new();
            hasher.update(b"CATDESK_QUEUE_REVIEW_INVITATION_V1\0");
            hasher.update(without_checksum.as_bytes());
            let checksum = format!("{:x}", hasher.finalize());
            format!(
                "{}\n",
                without_checksum.replacen(
                    "\"checksumSha256\":\"\"",
                    &format!("\"checksumSha256\":\"{checksum}\""),
                    1,
                )
            )
            .into_bytes()
        };
        assert_eq!(restamp(prepared_at_unix), invitation_before);
        for impossible_time in [1, u64::MAX] {
            fs::write(&invitation_path, restamp(impossible_time)).unwrap();
            assert!(
                review_invite::prepare_in_store_for_test(&queue, &projects[1], &receipt_id, sha)
                    .unwrap_err()
                    .contains("invalid preparation chronology")
            );
        }
        fs::write(&invitation_path, &invitation_before).unwrap();
        let grant_path = review_reader::grant_path(&queue, &receipt_id);
        let grant_before = fs::read(&grant_path).unwrap();
        let mut expired: Value = serde_json::from_slice(&grant_before).unwrap();
        expired["createdAtUnix"] = json!(1);
        expired["expiresAtUnix"] = json!(3601);
        fs::write(&grant_path, serde_json::to_vec(&expired).unwrap()).unwrap();
        assert!(
            review_invite::prepare_in_store_for_test(&queue, &projects[1], &receipt_id, sha)
                .unwrap_err()
                .contains("outside its lifetime")
        );
        assert_eq!(fs::read(&invitation_path).unwrap(), invitation_before);
        fs::write(&grant_path, &grant_before).unwrap();
        review_reader::revoke_in_store(&queue, "alpha", &receipt_id, sha).unwrap();
        assert!(
            review_invite::prepare_in_store_for_test(&queue, &projects[1], &receipt_id, sha)
                .unwrap_err()
                .contains("revoked")
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn operator_review_is_create_once_bound_and_never_promotes_wake() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        assert!(
            operator_review::test_decide_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                &"a".repeat(64),
                "ACCEPT",
                true
            )
            .is_err()
        );
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let packaged = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = packaged["package"]["artifactSha256"].as_str().unwrap();
        let package_path = queue.review_package_path(&receipt_id);
        let job_path = queue.job_path("alpha", "verified_fixture");
        let package_before = fs::read(&package_path).unwrap();
        let job_before = fs::read(&job_path).unwrap();
        let mut drifted = projects[1].clone();
        drifted.chatgpt_target_sha256 = Some("a".repeat(64));
        assert!(
            operator_review::test_decide_in_store(
                &queue,
                &drifted,
                &receipt_id,
                sha,
                "ACCEPT",
                true
            )
            .is_err()
        );
        assert!(
            operator_review::test_decide_in_store(
                &queue,
                &projects[2],
                &receipt_id,
                sha,
                "ACCEPT",
                true
            )
            .is_err()
        );
        fs::write(&package_path, b"{\n").unwrap();
        assert!(
            operator_review::test_decide_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "ACCEPT",
                true
            )
            .is_err()
        );
        fs::write(&package_path, &package_before).unwrap();
        fs::write(
            projects[1].workspace.join("source.txt"),
            b"changed since capture\n",
        )
        .unwrap();
        assert!(
            operator_review::test_decide_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "ACCEPT",
                false
            )
            .unwrap()
            .is_none()
        );
        let first = operator_review::test_decide_in_store(
            &queue,
            &projects[1],
            &receipt_id,
            sha,
            "ACCEPT",
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(first["idempotent"], false);
        assert_eq!(first["authority"], "OPERATOR_REVIEW_ONLY");
        assert_eq!(first["decision"], "ACCEPT");
        assert_eq!(first["independentReview"], "PENDING");
        assert_eq!(first["wakeReady"], false);
        let saved =
            operator_review::test_read_in_store(&queue, &projects[1], &receipt_id, sha).unwrap();
        assert_eq!(saved["recordSha256"], first["recordSha256"]);
        assert_eq!(saved["idempotent"], true);
        fs::write(projects[1].workspace.join("source.txt"), b"later edit\n").unwrap();
        let historical =
            operator_review::test_read_in_store(&queue, &projects[1], &receipt_id, sha).unwrap();
        assert_eq!(historical["recordSha256"], first["recordSha256"]);
        fs::write(&package_path, b"{\n").unwrap();
        assert!(
            operator_review::test_read_in_store(&queue, &projects[1], &receipt_id, sha).is_err()
        );
        fs::write(&package_path, &package_before).unwrap();
        let again = operator_review::test_decide_in_store(
            &queue,
            &projects[1],
            &receipt_id,
            sha,
            "ACCEPT",
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(again["recordSha256"], first["recordSha256"]);
        assert_eq!(again["idempotent"], true);
        assert!(
            operator_review::test_decide_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "REJECT",
                true
            )
            .unwrap_err()
            .contains("different decision")
        );
        assert!(
            operator_review::test_decide_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                &"f".repeat(64),
                "ACCEPT",
                true
            )
            .is_err()
        );
        assert!(operator_review::test_read_in_store(&queue, &drifted, &receipt_id, sha).is_err());
        assert!(
            operator_review::test_read_in_store(&queue, &projects[2], &receipt_id, sha).is_err()
        );
        assert_eq!(fs::read(&package_path).unwrap(), package_before);
        assert_eq!(fs::read(&job_path).unwrap(), job_before);
        cleanup(fixture);
    }

    #[tokio::test]
    async fn operator_review_action_requires_a_local_terminal() {
        assert!(
            !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal(),
            "this test requires noninteractive stdio"
        );
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let packaged = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = packaged["package"]["artifactSha256"].as_str().unwrap();
        let error = operator_review::test_record_local_decision_in_store(
            &queue,
            &projects[0].workspace,
            &projects[0],
            &projects[1],
            &receipt_id,
            sha,
            "ACCEPT",
        )
        .unwrap_err();
        assert!(
            error.contains("interactive local terminal is required"),
            "{error}"
        );
        assert!(
            !queue
                .root
                .join("receipts")
                .join(format!("{receipt_id}.operator-review.json"))
                .exists()
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn operator_review_registration_lock_rejects_stale_target_on_create_and_readback() {
        for create_before_drift in [false, true] {
            let (fixture, projects, queue, receipt_id, cargo) =
                completed_verified_fixture(false).await;
            verify_receipt_in_store_with_executable(
                &projects[0],
                &queue,
                &receipt_id,
                Some(&cargo),
            )
            .await
            .unwrap();
            let packaged = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
            let sha = packaged["package"]["artifactSha256"].as_str().unwrap();
            let record_path = queue
                .root
                .join("receipts")
                .join(format!("{receipt_id}.operator-review.json"));
            let registry = AutonomousProjectRegistryStoreV1::open_read_only(
                projects[0].workspace.join(".catdesk/projects"),
            )
            .unwrap();
            registry
                .with_registration_lock(|| {
                    assert!(
                        operator_review::test_decide_with_registration_in_store(
                            &queue,
                            &projects[0].workspace,
                            &projects[0],
                            &projects[1],
                            &receipt_id,
                            sha,
                            "ACCEPT",
                            true,
                        )
                        .is_err()
                    );
                    Ok(())
                })
                .unwrap();
            assert!(!record_path.exists());
            if create_before_drift {
                operator_review::test_decide_with_registration_in_store(
                    &queue,
                    &projects[0].workspace,
                    &projects[0],
                    &projects[1],
                    &receipt_id,
                    sha,
                    "ACCEPT",
                    true,
                )
                .unwrap()
                .unwrap();
                assert!(record_path.exists());
            }
            registry
                .bind_project_chat_target(
                    "alpha",
                    "https://chatgpt.com/c/alpha-after-capture",
                    projects[1].chatgpt_target_sha256.as_deref(),
                )
                .unwrap();
            let error = operator_review::test_decide_with_registration_in_store(
                &queue,
                &projects[0].workspace,
                &projects[0],
                &projects[1],
                &receipt_id,
                sha,
                "ACCEPT",
                true,
            )
            .unwrap_err();
            assert!(error.contains("registration changed"), "{error}");
            assert_eq!(record_path.exists(), create_before_drift);
            cleanup(fixture);
        }
    }

    #[tokio::test]
    async fn operator_review_rejects_modified_or_partial_decision_records() {
        for decision in ["REJECT", "INCONCLUSIVE"] {
            let (fixture, projects, queue, receipt_id, cargo) =
                completed_verified_fixture(false).await;
            verify_receipt_in_store_with_executable(
                &projects[0],
                &queue,
                &receipt_id,
                Some(&cargo),
            )
            .await
            .unwrap();
            let packaged = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
            let sha = packaged["package"]["artifactSha256"].as_str().unwrap();
            let created = operator_review::test_decide_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                decision,
                true,
            )
            .unwrap()
            .unwrap();
            assert_eq!(created["decision"], decision);
            let path = queue
                .root
                .join("receipts")
                .join(format!("{receipt_id}.operator-review.json"));
            let original = fs::read(&path).unwrap();
            let modified = String::from_utf8(original.clone())
                .unwrap()
                .replace(decision, "ACCEPT");
            fs::write(&path, modified).unwrap();
            assert!(
                operator_review::test_read_in_store(&queue, &projects[1], &receipt_id, sha)
                    .is_err()
            );
            fs::write(&path, b"{\n").unwrap();
            assert!(
                operator_review::test_decide_in_store(
                    &queue,
                    &projects[1],
                    &receipt_id,
                    sha,
                    decision,
                    true
                )
                .is_err()
            );
            fs::write(&path, original).unwrap();
            cleanup(fixture);
        }
    }

    #[tokio::test]
    async fn granted_review_reader_requires_exact_chain_and_never_mutates_queue() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let created = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = created["package"]["artifactSha256"].as_str().unwrap();
        let package_path = queue.review_package_path(&receipt_id);
        let job_path = queue.job_path("alpha", "verified_fixture");
        let grant_path = review_reader::grant_path(&queue, &receipt_id);
        let package_before = fs::read(&package_path).unwrap();
        let job_before = fs::read(&job_path).unwrap();
        let receipt_count_before = fs::read_dir(queue.root.join("receipts")).unwrap().count();

        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .unwrap_err()
            .contains("grant is required")
        );
        assert!(!grant_path.exists());
        assert_eq!(
            fs::read_dir(queue.root.join("receipts")).unwrap().count(),
            receipt_count_before
        );
        assert_eq!(fs::read(&package_path).unwrap(), package_before);
        assert_eq!(fs::read(&job_path).unwrap(), job_before);

        let grant = review_reader::grant_in_store(&queue, &projects[1], &receipt_id, sha).unwrap();
        assert_eq!(grant["independentReview"], "PENDING");
        assert_eq!(grant["wakeReady"], false);
        assert_eq!(
            grant["expiresAtUnix"].as_u64().unwrap(),
            grant["createdAtUnix"].as_u64().unwrap() + 3600
        );
        let grant_before = fs::read(&grant_path).unwrap();
        let metadata = review_reader::read_in_store(
            &queue,
            &projects[1],
            &receipt_id,
            sha,
            "metadata",
            0,
            None,
        )
        .unwrap();
        assert_eq!(metadata["independentReview"], "PENDING");
        assert_eq!(metadata["wakeReady"], false);
        assert!(metadata["fileCount"].as_u64().unwrap() > 0);
        assert!(serde_json::to_vec(&metadata).unwrap().len() <= 16 * 1024);
        assert_eq!(metadata["receipt"]["childExitObserved"], true);
        assert_eq!(metadata["receipt"]["outcome"], "NATIVE_EXIT_ZERO");
        assert!(metadata.get("prompt").is_none());
        assert!(metadata.get("pageBase64").is_none());
        let file_metadata = review_reader::read_in_store(
            &queue,
            &projects[1],
            &receipt_id,
            sha,
            "metadata",
            0,
            Some(0),
        )
        .unwrap();
        assert_eq!(file_metadata["file"]["fileIndex"], 0);
        assert!(file_metadata["file"]["pathPreview"].as_str().is_some());
        assert!(file_metadata["file"]["pathSha256"].as_str().is_some());
        let prompt =
            review_reader::read_in_store(&queue, &projects[1], &receipt_id, sha, "prompt", 0, None)
                .unwrap();
        assert_eq!(prompt["pageTextUtf8"], "fixture edit");
        assert_eq!(prompt["totalBytes"], 12);
        assert_eq!(prompt["nextOffset"], Value::Null);
        assert_eq!(prompt["independentReview"], "PENDING");
        assert_eq!(prompt["wakeReady"], false);
        let registered_page = review_reader::read_with_store_opener(
            &projects[0].workspace,
            "alpha",
            &receipt_id,
            sha,
            "prompt",
            (0, None),
            |control| {
                assert_eq!(control, &projects[0]);
                Ok(QueueStore {
                    root: queue.root.clone(),
                })
            },
        )
        .unwrap();
        assert_eq!(registered_page["pageBase64"], prompt["pageBase64"]);
        assert_eq!(fs::read(&grant_path).unwrap(), grant_before);
        assert_eq!(fs::read(&package_path).unwrap(), package_before);
        assert_eq!(fs::read(&job_path).unwrap(), job_before);

        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[2],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                &"0".repeat(64),
                "metadata",
                0,
                None
            )
            .is_err()
        );
        let mut rotated = projects[1].clone();
        rotated.codex_thread_id = Some(Uuid::new_v4().to_string());
        assert!(
            review_reader::read_in_store(&queue, &rotated, &receipt_id, sha, "metadata", 0, None)
                .is_err()
        );
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                1,
                None
            )
            .is_err()
        );
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "prompt",
                13,
                None
            )
            .is_err()
        );
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "file",
                0,
                Some(255)
            )
            .is_err()
        );
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "prompt",
                0,
                Some(0)
            )
            .is_err()
        );

        let now = unix_now().unwrap();
        let mut expired: Value = serde_json::from_slice(&grant_before).unwrap();
        expired["createdAtUnix"] = json!(now - 7200);
        expired["expiresAtUnix"] = json!(now - 3600);
        fs::write(&grant_path, serde_json::to_vec(&expired).unwrap()).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        assert!(review_reader::grant_in_store(&queue, &projects[1], &receipt_id, sha).is_err());
        let mut future: Value = serde_json::from_slice(&grant_before).unwrap();
        future["createdAtUnix"] = json!(now + 3600);
        future["expiresAtUnix"] = json!(now + 7200);
        fs::write(&grant_path, serde_json::to_vec(&future).unwrap()).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        fs::write(&grant_path, &grant_before).unwrap();
        let mut tampered = grant_before.clone();
        tampered[5] ^= 1;
        fs::write(&grant_path, tampered).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        fs::write(&grant_path, grant_before).unwrap();
        let mut tampered = package_before.clone();
        tampered[5] ^= 1;
        fs::write(&package_path, tampered).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        let revoked = review_reader::revoke_in_store(&queue, "alpha", &receipt_id, sha).unwrap();
        assert_eq!(revoked["idempotent"], false);
        assert_eq!(revoked["independentReview"], "PENDING");
        assert_eq!(revoked["wakeReady"], false);
        fs::write(&package_path, &package_before).unwrap();
        assert_eq!(
            review_reader::revoke_in_store(&queue, "alpha", &receipt_id, sha).unwrap()["idempotent"],
            true
        );
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        assert!(review_reader::grant_in_store(&queue, &projects[1], &receipt_id, sha).is_err());
        let tombstone_path = review_reader::revocation_path(&queue, &receipt_id);
        let mut tampered_tombstone = fs::read(&tombstone_path).unwrap();
        tampered_tombstone[5] ^= 1;
        fs::write(&tombstone_path, tampered_tombstone).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn disclosure_renewal_restores_expired_exact_package_and_preserves_final_revocation() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let packaged = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = packaged["package"]["artifactSha256"].as_str().unwrap();
        let package_path = queue.review_package_path(&receipt_id);
        let package_before = fs::read(&package_path).unwrap();
        let job_path = queue.job_path("alpha", "verified_fixture");
        let job_before = fs::read(&job_path).unwrap();
        let receipt_entries = fs::read_dir(queue.root.join("receipts")).unwrap().count();
        assert!(
            review_reader::test_renewal_sequences_with_cap(&queue, &receipt_id, receipt_entries)
                .is_ok()
        );
        assert!(
            review_reader::test_renewal_sequences_with_cap(
                &queue,
                &receipt_id,
                receipt_entries - 1
            )
            .unwrap_err()
            .contains("directory limit")
        );
        review_reader::publish_after_confirmation(
            &projects[0].workspace,
            &projects[0],
            &projects[1],
            &queue,
            &receipt_id,
            sha,
            false,
        )
        .unwrap();
        assert!(
            review_reader::renew_in_store(&queue, &projects[1], &receipt_id, sha)
                .unwrap_err()
                .contains("last five minutes")
        );

        // Move only the disposable fixture clock window into the past. The
        // real grant is never edited by this test.
        let grant_path = review_reader::grant_path(&queue, &receipt_id);
        let mut expired: Value = serde_json::from_slice(&fs::read(&grant_path).unwrap()).unwrap();
        let now = unix_now().unwrap();
        expired["createdAtUnix"] = json!(now - 7200);
        expired["expiresAtUnix"] = json!(now - 3600);
        fs::write(&grant_path, serde_json::to_vec(&expired).unwrap()).unwrap();
        let expired_bytes = fs::read(&grant_path).unwrap();
        assert!(
            review_reader::read_in_store(&queue, &projects[1], &receipt_id, sha, "prompt", 0, None)
                .is_err()
        );
        let mut stale_target = projects[1].clone();
        stale_target.chatgpt_target_sha256 = Some("0".repeat(64));
        assert!(
            review_reader::publish_after_confirmation(
                &projects[0].workspace,
                &projects[0],
                &stale_target,
                &queue,
                &receipt_id,
                sha,
                true,
            )
            .unwrap_err()
            .contains("registration changed")
        );
        let mut changed_package = package_before.clone();
        changed_package[5] ^= 1;
        fs::write(&package_path, &changed_package).unwrap();
        assert!(
            review_reader::publish_after_confirmation(
                &projects[0].workspace,
                &projects[0],
                &projects[1],
                &queue,
                &receipt_id,
                sha,
                true,
            )
            .is_err()
        );
        fs::write(&package_path, &package_before).unwrap();
        let renewal = review_reader::publish_after_confirmation(
            &projects[0].workspace,
            &projects[0],
            &projects[1],
            &queue,
            &receipt_id,
            sha,
            true,
        )
        .expect("fresh local consent may renew exact expired package");
        assert_eq!(renewal["renewalSequence"], 1);
        assert_eq!(renewal["independentReview"], "PENDING");
        assert_eq!(renewal["wakeReady"], false);
        assert_eq!(fs::read(&grant_path).unwrap(), expired_bytes);
        let page =
            review_reader::read_in_store(&queue, &projects[1], &receipt_id, sha, "prompt", 0, None)
                .unwrap();
        assert_eq!(page["pageTextUtf8"], "fixture edit");
        assert_eq!(page["independentReview"], "PENDING");
        assert_eq!(page["wakeReady"], false);
        assert!(
            review_reader::renew_in_store(&queue, &projects[1], &receipt_id, sha)
                .unwrap_err()
                .contains("last five minutes")
        );

        let renewal_path = queue.root.join("receipts").join(format!(
            "{receipt_id}.web-review-disclosure-renewal-0001.json"
        ));
        let renewal_bytes = fs::read(&renewal_path).unwrap();
        let mut early: Value = serde_json::from_slice(&renewal_bytes).unwrap();
        early["createdAtUnix"] = json!(now - 6900);
        early["expiresAtUnix"] = json!(now - 3300);
        fs::write(&renewal_path, serde_json::to_vec(&early).unwrap()).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .unwrap_err()
            .contains("binding changed")
        );
        fs::write(&renewal_path, &renewal_bytes).unwrap();

        let mut bad_hash: Value = serde_json::from_slice(&renewal_bytes).unwrap();
        bad_hash["previousRecordSha256"] = json!("0".repeat(64));
        fs::write(&renewal_path, serde_json::to_vec(&bad_hash).unwrap()).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        fs::write(&renewal_path, &renewal_bytes).unwrap();

        let gap_path = queue.root.join("receipts").join(format!(
            "{receipt_id}.web-review-disclosure-renewal-0002.json"
        ));
        fs::rename(&renewal_path, &gap_path).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .unwrap_err()
            .contains("gap")
        );
        fs::rename(&gap_path, &renewal_path).unwrap();

        let revoked = review_reader::revoke_after_confirmation(
            &projects[0].workspace,
            &projects[0],
            &queue,
            "alpha",
            &receipt_id,
            sha,
        )
        .unwrap();
        assert_eq!(revoked["wakeReady"], false);
        assert!(
            review_reader::renew_in_store(&queue, &projects[1], &receipt_id, sha)
                .unwrap_err()
                .contains("revoked")
        );
        assert!(
            review_reader::read_in_store(&queue, &projects[1], &receipt_id, sha, "prompt", 0, None)
                .is_err()
        );
        assert_eq!(fs::read(&package_path).unwrap(), package_before);
        assert_eq!(fs::read(&job_path).unwrap(), job_before);
        cleanup(fixture);
    }

    async fn disposable_public_review_call(
        control_workspace: &Path,
        queue_root: &Path,
        arguments: Value,
    ) -> crate::mcp::JsonRpcResponse {
        let request = crate::mcp::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!("disposable-review-call")),
            method: "tools/call".into(),
            params: json!({
                "name":"catdesk_codex_queue_review_package_read",
                "arguments":arguments,
            }),
        };
        let queue_root = queue_root.to_path_buf();
        crate::mcp::test_queue_review_tool_call(
            &request,
            control_workspace.to_str().expect("fixture path UTF-8"),
            true,
            move |workspace, project_id, receipt_id, sha256, part, offset, file_index| {
                review_reader::read_with_store_opener(
                    workspace,
                    project_id,
                    receipt_id,
                    sha256,
                    part,
                    (offset, file_index),
                    |_| Ok(QueueStore { root: queue_root }),
                )
            },
        )
        .await
    }

    #[tokio::test]
    async fn disposable_public_mcp_review_call_requires_grant_then_revocation_denies() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let created = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = created["package"]["artifactSha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let control = &projects[0].workspace;
        let args = json!({
            "projectId":"alpha",
            "receiptId":receipt_id,
            "expectedPackageSha256":sha,
            "part":"prompt",
            "offset":0,
        });
        let job_path = queue.job_path("alpha", "verified_fixture");
        let package_path = queue.review_package_path(&receipt_id);
        let job_before = fs::read(&job_path).unwrap();
        let package_before = fs::read(&package_path).unwrap();

        let denied = disposable_public_review_call(control, &queue.root, args.clone()).await;
        assert_eq!(denied.result.as_ref().unwrap()["isError"], true);
        let denied_text = serde_json::to_string(&denied.result).unwrap();
        assert!(denied_text.contains("unavailable or local disclosure is not authorized"));
        assert!(!denied_text.contains("fixture edit"));
        assert!(!denied_text.contains(&sha));

        // The same public call is invisible outside the SupervisorOnly mode.
        // The panic closure proves the queue reader was never entered.
        let denied_mode = crate::mcp::test_queue_review_tool_call(
            &crate::mcp::JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!("disposable-review-mode-denied")),
                method: "tools/call".into(),
                params: json!({
                    "name":"catdesk_codex_queue_review_package_read",
                    "arguments":args,
                }),
            },
            control.to_str().expect("fixture path UTF-8"),
            false,
            |_, _, _, _, _, _, _| panic!("reader entered in denied mode"),
        )
        .await;
        assert_eq!(denied_mode.result.as_ref().unwrap()["isError"], true);
        assert!(
            serde_json::to_string(&denied_mode.result)
                .unwrap()
                .contains("Unknown tool")
        );

        // Simulate the already confirmed local terminal action in the fixture;
        // this does not grant disclosure in the current user's real queue.
        let grant = review_reader::grant_in_store(&queue, &projects[1], &receipt_id, &sha).unwrap();
        assert_eq!(grant["independentReview"], "PENDING");
        assert_eq!(grant["wakeReady"], false);
        let page = disposable_public_review_call(control, &queue.root, args.clone()).await;
        assert_ne!(page.result.as_ref().unwrap()["isError"], true);
        let structured = page
            .result
            .as_ref()
            .unwrap()
            .get("structuredContent")
            .expect("MCP structured result");
        assert_eq!(structured["part"], "prompt");
        assert_eq!(structured["pageTextUtf8"], "fixture edit");
        assert_eq!(structured["packageSha256"], sha);
        assert_eq!(structured["independentReview"], "PENDING");
        assert_eq!(structured["wakeReady"], false);
        assert_eq!(structured["readOnly"], true);

        let metadata = disposable_public_review_call(
            control,
            &queue.root,
            json!({
                "projectId":"alpha",
                "receiptId":receipt_id,
                "expectedPackageSha256":sha,
            }),
        )
        .await;
        let metadata = &metadata.result.as_ref().unwrap()["structuredContent"];
        assert_eq!(metadata["part"], "metadata");
        assert_eq!(metadata["receipt"]["childExitObserved"], true);
        assert!(metadata.get("pageBase64").is_none());
        assert!(metadata.get("prompt").is_none());

        let revoked = review_reader::revoke_in_store(&queue, "alpha", &receipt_id, &sha).unwrap();
        assert_eq!(revoked["independentReview"], "PENDING");
        assert_eq!(revoked["wakeReady"], false);
        let denied_after_revoke = disposable_public_review_call(
            control,
            &queue.root,
            json!({
                "projectId":"alpha",
                "receiptId":receipt_id,
                "expectedPackageSha256":sha,
                "part":"prompt",
            }),
        )
        .await;
        assert_eq!(
            denied_after_revoke.result.as_ref().unwrap()["isError"],
            true
        );
        assert!(
            serde_json::to_string(&denied_after_revoke.result)
                .unwrap()
                .contains("unavailable or local disclosure is not authorized")
        );
        let metadata_after_revoke = disposable_public_review_call(
            control,
            &queue.root,
            json!({
                "projectId":"alpha",
                "receiptId":receipt_id,
                "expectedPackageSha256":sha,
            }),
        )
        .await;
        assert_eq!(
            metadata_after_revoke.result.as_ref().unwrap()["isError"],
            true
        );
        assert!(review_reader::grant_in_store(&queue, &projects[1], &receipt_id, &sha).is_err());
        assert_eq!(fs::read(&job_path).unwrap(), job_before);
        assert_eq!(fs::read(&package_path).unwrap(), package_before);
        cleanup(fixture);
    }

    #[tokio::test]
    async fn review_reader_revocation_survives_project_removal_and_reregistration() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let created = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = created["package"]["artifactSha256"].as_str().unwrap();
        review_reader::grant_in_store(&queue, &projects[1], &receipt_id, sha).unwrap();
        assert!(
            review_reader::read_in_store(&queue, &projects[1], &receipt_id, sha, "prompt", 0, None)
                .is_ok()
        );
        let control_workspace = &projects[0].workspace;
        let registry =
            AutonomousProjectRegistryStoreV1::open(control_workspace.join(".catdesk/projects"))
                .unwrap();
        let mut absent = registry.load_registry().unwrap();
        absent
            .projects
            .retain(|project| project.project_id != "alpha");
        fs::write(
            control_workspace.join(".catdesk/projects/projects.json"),
            serde_json::to_vec(&absent).unwrap(),
        )
        .unwrap();
        assert!(resolve_registered_project_from_control_root(control_workspace, "alpha").is_err());
        assert!(
            review_reader::revoke_after_confirmation(
                control_workspace,
                &projects[0],
                &queue,
                "beta",
                &receipt_id,
                sha,
            )
            .is_err()
        );
        assert!(
            review_reader::revoke_after_confirmation(
                control_workspace,
                &projects[0],
                &queue,
                "alpha",
                &receipt_id,
                &"0".repeat(64),
            )
            .is_err()
        );
        let revoked = review_reader::revoke_after_confirmation(
            control_workspace,
            &projects[0],
            &queue,
            "alpha",
            &receipt_id,
            sha,
        )
        .unwrap();
        assert_eq!(revoked["idempotent"], false);
        assert_eq!(revoked["independentReview"], "PENDING");
        assert_eq!(revoked["wakeReady"], false);
        registry.register_project(projects[1].clone()).unwrap();
        assert_eq!(
            resolve_registered_project_from_control_root(control_workspace, "alpha")
                .unwrap()
                .project,
            projects[1]
        );
        assert!(
            review_reader::read_in_store(&queue, &projects[1], &receipt_id, sha, "prompt", 0, None)
                .is_err()
        );
        assert!(
            review_reader::read_with_store_opener(
                control_workspace,
                "alpha",
                &receipt_id,
                sha,
                "prompt",
                (0, None),
                |_| Ok(QueueStore {
                    root: queue.root.clone(),
                }),
            )
            .is_err()
        );
        assert!(review_reader::grant_in_store(&queue, &projects[1], &receipt_id, sha).is_err());
        assert_eq!(
            review_reader::revoke_after_confirmation(
                control_workspace,
                &projects[0],
                &queue,
                "alpha",
                &receipt_id,
                sha,
            )
            .unwrap()["idempotent"],
            true
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn review_reader_rejects_redirected_package_or_grant() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let created = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = created["package"]["artifactSha256"].as_str().unwrap();
        review_reader::grant_in_store(&queue, &projects[1], &receipt_id, sha).unwrap();
        let grant_path = review_reader::grant_path(&queue, &receipt_id);
        let saved_grant = fixture.join("saved-grant.json");
        fs::rename(&grant_path, &saved_grant).unwrap();
        let linked = {
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(&saved_grant, &grant_path).is_ok()
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&saved_grant, &grant_path).is_ok()
            }
        };
        if linked {
            assert!(
                review_reader::read_in_store(
                    &queue,
                    &projects[1],
                    &receipt_id,
                    sha,
                    "metadata",
                    0,
                    None
                )
                .is_err()
            );
            fs::remove_file(&grant_path).unwrap();
        }
        fs::rename(&saved_grant, &grant_path).unwrap();

        let package_path = queue.review_package_path(&receipt_id);
        let saved_package = fixture.join("saved-package.json");
        fs::rename(&package_path, &saved_package).unwrap();
        let linked = {
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(&saved_package, &package_path).is_ok()
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&saved_package, &package_path).is_ok()
            }
        };
        if linked {
            assert!(
                review_reader::read_in_store(
                    &queue,
                    &projects[1],
                    &receipt_id,
                    sha,
                    "metadata",
                    0,
                    None
                )
                .is_err()
            );
            fs::remove_file(&package_path).unwrap();
        }
        fs::create_dir(&package_path).unwrap();
        assert!(
            review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "metadata",
                0,
                None
            )
            .is_err()
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn review_reader_reassembles_bounded_multibyte_file_pages() {
        let source_after = format!("{}\n", "界".repeat(5000)).into_bytes();
        let (fixture, projects, queue, receipt_id, cargo) =
            completed_verified_fixture_for_profile_with_source("rust_full", false, &source_after)
                .await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        let created = package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let sha = created["package"]["artifactSha256"].as_str().unwrap();
        review_reader::grant_in_store(&queue, &projects[1], &receipt_id, sha).unwrap();
        let grant_path = review_reader::grant_path(&queue, &receipt_id);
        let package_path = queue.review_package_path(&receipt_id);
        let job_path = queue.job_path("alpha", "verified_fixture");
        let before = [
            fs::read(&grant_path).unwrap(),
            fs::read(&package_path).unwrap(),
            fs::read(&job_path).unwrap(),
        ];
        let receipt = queue.read_receipt(&receipt_id).unwrap();
        let candidate = queue.read_review_candidate(&receipt).unwrap();
        let (verification, _) = queue
            .read_verification(&receipt, &candidate)
            .unwrap()
            .unwrap();
        let file_index = verification
            .files
            .iter()
            .position(|file| file.path == "source.txt")
            .unwrap();
        let expected_sha = format!("{:x}", Sha256::digest(&source_after));
        let mut assembled = Vec::new();
        let mut offset = 0u64;
        loop {
            let page = review_reader::read_in_store(
                &queue,
                &projects[1],
                &receipt_id,
                sha,
                "file",
                offset,
                Some(file_index),
            )
            .unwrap();
            let bytes = BASE64.decode(page["pageBase64"].as_str().unwrap()).unwrap();
            assert!(bytes.len() <= 8192);
            assert_eq!(page["offset"].as_u64(), Some(offset));
            assert_eq!(page["totalBytes"].as_u64(), Some(source_after.len() as u64));
            assert_eq!(page["contentSha256"], expected_sha);
            assert_eq!(page["pageSha256"], format!("{:x}", Sha256::digest(&bytes)));
            assert!(serde_json::to_vec(&page).unwrap().len() <= 32 * 1024);
            assembled.extend_from_slice(&bytes);
            match page["nextOffset"].as_u64() {
                Some(next) => {
                    assert_eq!(next, offset + bytes.len() as u64);
                    offset = next;
                }
                None => break,
            }
        }
        assert_eq!(assembled, source_after);
        assert_eq!(
            before,
            [
                fs::read(&grant_path).unwrap(),
                fs::read(&package_path).unwrap(),
                fs::read(&job_path).unwrap(),
            ]
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn review_package_refuses_rebound_tampering_and_orphaned_write() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&cargo))
            .await
            .unwrap();
        package_verified_receipt_in_store(&queue, &receipt_id).unwrap();
        let path = queue.review_package_path(&receipt_id);
        let original = fs::read(&path).unwrap();
        let mut package: QueueReviewPackage = serde_json::from_slice(&original).unwrap();
        package.verification.files[0].content_base64 = Some(BASE64.encode(b"forged"));
        let mut tampered = serde_json::to_vec_pretty(&package).unwrap();
        tampered.push(b'\n');
        fs::write(&path, &tampered).unwrap();
        let mut job = queue
            .load_job("alpha", "verified_fixture")
            .unwrap()
            .unwrap();
        job.review_package_sha256 = Some(format!("{:x}", Sha256::digest(&tampered)));
        queue.save_job(&job).unwrap();
        assert!(package_verified_receipt_in_store(&queue, &receipt_id).is_err());

        let mut wrong_intent: QueueReviewPackage = serde_json::from_slice(&original).unwrap();
        wrong_intent.prompt = "a different task".into();
        let mut tampered = serde_json::to_vec_pretty(&wrong_intent).unwrap();
        tampered.push(b'\n');
        fs::write(&path, &tampered).unwrap();
        job.review_package_sha256 = Some(format!("{:x}", Sha256::digest(&tampered)));
        queue.save_job(&job).unwrap();
        assert!(package_verified_receipt_in_store(&queue, &receipt_id).is_err());

        // A persisted package with no durable job digest is never adopted.
        fs::write(&path, original).unwrap();
        job.review_package_sha256 = None;
        queue.save_job(&job).unwrap();
        let error = package_verified_receipt_in_store(&queue, &receipt_id).unwrap_err();
        assert!(error.contains("ATTENTION"), "{error}");
        assert_eq!(
            queue
                .load_job("alpha", "verified_fixture")
                .unwrap()
                .unwrap()
                .state,
            QueueState::Attention
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn failed_local_verification_cannot_create_review_package() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(true).await;
        let result = verify_receipt_in_store_with_executable(
            &projects[0],
            &queue,
            &receipt_id,
            Some(&cargo),
        )
        .await
        .unwrap();
        assert_eq!(result["verification"]["status"], "LOCAL_COMMAND_FAILURE");
        assert!(package_verified_receipt_in_store(&queue, &receipt_id).is_err());
        assert!(!queue.review_package_path(&receipt_id).exists());
        cleanup(fixture);
    }

    #[tokio::test]
    async fn fixed_profile_persists_source_and_four_local_command_exits() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(false).await;
        let result = verify_receipt_in_store_with_executable(
            &projects[0],
            &queue,
            &receipt_id,
            Some(&cargo),
        )
        .await
        .unwrap();
        assert_eq!(result["idempotent"], false);
        assert_eq!(
            result["verification"]["status"],
            "LOCAL_COMMANDS_EXITED_ZERO_SOURCE_RECHECKED"
        );
        assert_eq!(result["verification"]["wakeReady"], false);
        assert_eq!(result["verification"]["independentReview"], "PENDING");
        let receipt = queue.read_receipt(&receipt_id).unwrap();
        let candidate = queue.read_review_candidate(&receipt).unwrap();
        let (artifact, _) = queue
            .read_verification(&receipt, &candidate)
            .unwrap()
            .unwrap();
        assert_eq!(artifact.commands.len(), 4);
        assert!(
            artifact
                .commands
                .iter()
                .all(|command| command.status == "EXIT_ZERO_PARENT_OBSERVED"
                    && valid_sha256_hex(&command.environment_sha256))
        );
        assert_eq!(artifact.files.len(), 2);
        let source = artifact
            .files
            .iter()
            .find(|file| file.path == "source.txt")
            .unwrap();
        assert_eq!(
            BASE64
                .decode(source.content_base64.as_ref().unwrap())
                .unwrap(),
            b"after\n"
        );
        let reread = verify_receipt_in_store_with_executable(
            &projects[0],
            &queue,
            &receipt_id,
            Some(&cargo),
        )
        .await
        .unwrap();
        assert_eq!(reread["idempotent"], true);
        assert_eq!(reread["historicalReadback"], true);
        let path = queue.verification_path(&receipt_id);
        let mut tampered: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        tampered["patchBase64"] = Value::String("dGFtcGVy".into());
        fs::write(&path, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();
        assert!(queue.read_verification(&receipt, &candidate).is_err());
        cleanup(fixture);
    }

    #[tokio::test]
    async fn nonzero_fixed_command_keeps_artifact_unreviewed_and_not_wake_ready() {
        let (fixture, projects, queue, receipt_id, cargo) = completed_verified_fixture(true).await;
        let result = verify_receipt_in_store_with_executable(
            &projects[0],
            &queue,
            &receipt_id,
            Some(&cargo),
        )
        .await
        .unwrap();
        assert_eq!(result["verification"]["status"], "LOCAL_COMMAND_FAILURE");
        assert_eq!(result["verification"]["independentReview"], "PENDING");
        assert_eq!(result["verification"]["wakeReady"], false);
        let receipt = queue.read_receipt(&receipt_id).unwrap();
        let candidate = queue.read_review_candidate(&receipt).unwrap();
        let (artifact, _) = queue
            .read_verification(&receipt, &candidate)
            .unwrap()
            .unwrap();
        assert_eq!(artifact.commands.len(), 4);
        assert_eq!(artifact.commands[1].argv[1], "clippy");
        assert_eq!(artifact.commands[1].exit_code, Some(7));
        assert_eq!(artifact.commands[1].status, "EXIT_NONZERO_PARENT_OBSERVED");
        // Rebind a deliberately malformed artifact digest to exercise the
        // field validation even when the failed command status is legitimate.
        let mut tampered = artifact;
        tampered.commands[1].environment_sha256 = "invalid".into();
        let mut bytes = serde_json::to_vec_pretty(&tampered).unwrap();
        bytes.push(b'\n');
        fs::write(queue.verification_path(&receipt_id), &bytes).unwrap();
        let mut job = queue
            .load_job("alpha", "verified_fixture")
            .unwrap()
            .unwrap();
        job.verification_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
        queue.save_job(&job).unwrap();
        assert!(queue.read_verification(&receipt, &candidate).is_err());
        cleanup(fixture);
    }

    #[tokio::test]
    async fn fixed_python_profiles_capture_source_command_exits_and_refuse_rebound_argv() {
        for profile in ["python_uv_group_dev_v1", "python_uv_extras_dev_lab_v1"] {
            let (fixture, projects, queue, receipt_id, uv) =
                completed_verified_fixture_for_profile(profile, false).await;
            let result = verify_receipt_in_store_with_executable(
                &projects[0],
                &queue,
                &receipt_id,
                Some(&uv),
            )
            .await
            .unwrap();
            assert_eq!(
                result["verification"]["status"],
                "LOCAL_COMMANDS_EXITED_ZERO_SOURCE_RECHECKED"
            );
            assert_eq!(
                result["verification"]["processEvidence"],
                "DIRECT_UV_PARENT_EXIT_ONLY"
            );
            assert_eq!(result["verification"]["independentReview"], "PENDING");
            assert_eq!(result["verification"]["wakeReady"], false);
            let receipt = queue.read_receipt(&receipt_id).unwrap();
            let candidate = queue.read_review_candidate(&receipt).unwrap();
            let (artifact, _) = queue
                .read_verification(&receipt, &candidate)
                .unwrap()
                .unwrap();
            assert_eq!(artifact.commands.len(), 3);
            assert!(artifact.commands.iter().all(|command| {
                command.status == "EXIT_ZERO_PARENT_OBSERVED"
                    && command.argv.iter().any(|arg| arg == "--locked")
                    && command.argv.iter().any(|arg| arg == "--offline")
                    && valid_sha256_hex(&command.environment_sha256)
                    && command.executable_sha256 == uv.sha256
            }));
            for command in &artifact.commands[..2] {
                if profile == "python_uv_group_dev_v1" {
                    assert!(
                        command
                            .argv
                            .windows(2)
                            .any(|args| args == ["ruff", "--isolated"])
                    );
                } else {
                    assert!(
                        command
                            .argv
                            .windows(3)
                            .any(|args| args == ["ruff", "--config", "pyproject.toml"])
                    );
                }
            }
            assert_eq!(artifact.files.len(), 2);
            assert_eq!(
                verify_receipt_in_store_with_executable(
                    &projects[0],
                    &queue,
                    &receipt_id,
                    Some(&uv),
                )
                .await
                .unwrap()["idempotent"],
                true
            );

            // Even a matching job digest does not turn a changed command into
            // a valid fixed-profile verification artifact.
            let mut tampered = artifact;
            tampered.commands[0].argv[1] = "shell".into();
            let mut bytes = serde_json::to_vec_pretty(&tampered).unwrap();
            bytes.push(b'\n');
            fs::write(queue.verification_path(&receipt_id), &bytes).unwrap();
            let mut job = queue
                .load_job("alpha", "verified_fixture")
                .unwrap()
                .unwrap();
            job.verification_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
            queue.save_job(&job).unwrap();
            assert!(queue.read_verification(&receipt, &candidate).is_err());
            cleanup(fixture);
        }
    }

    #[tokio::test]
    async fn nonzero_python_profile_stays_pending_and_not_wake_ready() {
        for profile in ["python_uv_group_dev_v1", "python_uv_extras_dev_lab_v1"] {
            let (fixture, projects, queue, receipt_id, uv) =
                completed_verified_fixture_for_profile(profile, true).await;
            let result = verify_receipt_in_store_with_executable(
                &projects[0],
                &queue,
                &receipt_id,
                Some(&uv),
            )
            .await
            .unwrap();
            assert_eq!(result["verification"]["status"], "LOCAL_COMMAND_FAILURE");
            assert_eq!(result["verification"]["independentReview"], "PENDING");
            assert_eq!(result["verification"]["wakeReady"], false);
            let receipt = queue.read_receipt(&receipt_id).unwrap();
            let candidate = queue.read_review_candidate(&receipt).unwrap();
            let (artifact, _) = queue
                .read_verification(&receipt, &candidate)
                .unwrap()
                .unwrap();
            assert_eq!(artifact.commands[1].exit_code, Some(7));
            assert_eq!(artifact.commands[1].status, "EXIT_NONZERO_PARENT_OBSERVED");
            cleanup(fixture);
        }
    }

    #[tokio::test]
    async fn python_profile_package_mismatch_refuses_commands() {
        let (fixture, projects, queue, receipt_id, uv) =
            completed_verified_fixture_for_profile("python_uv_group_dev_v1", false).await;
        let workspace = &projects[1].workspace;
        fs::write(workspace.join("pyproject.toml"), b"[project]\nname = \"different-package\"\nversion = \"0.1.0\"\n[dependency-groups]\ndev = [\"pytest>=8\", \"ruff>=0.8\"]\n").unwrap();
        let error =
            verify_receipt_in_store_with_executable(&projects[0], &queue, &receipt_id, Some(&uv))
                .await
                .expect_err("mismatched Python profile must refuse verification");
        assert!(error.contains("package name disagree"), "{error}");
        assert!(!queue.verification_path(&receipt_id).exists());
        cleanup(fixture);
    }

    #[tokio::test]
    async fn python_profiles_require_their_declared_dependency_selection() {
        for (profile, removed, expected_error) in [
            (
                "python_uv_group_dev_v1",
                "dev = [\"pytest>=8\", \"ruff>=0.8\"]",
                "development dependencies are absent",
            ),
            (
                "python_uv_extras_dev_lab_v1",
                "lab = [\"flask>=3\"]",
                "lab extra is absent",
            ),
        ] {
            let (fixture, projects, queue, receipt_id, uv) =
                completed_verified_fixture_for_profile(profile, false).await;
            let path = projects[1].workspace.join("pyproject.toml");
            let manifest = fs::read_to_string(&path).unwrap();
            assert!(manifest.contains(removed));
            fs::write(&path, manifest.replace(removed, "")).unwrap();
            let error = verify_receipt_in_store_with_executable(
                &projects[0],
                &queue,
                &receipt_id,
                Some(&uv),
            )
            .await
            .expect_err("missing selected dependencies must refuse verification");
            assert!(error.contains(expected_error), "{error}");
            assert!(!queue.verification_path(&receipt_id).exists());
            cleanup(fixture);
        }
    }

    #[test]
    fn python_manifest_reader_rejects_oversized_and_non_utf8_input() {
        let fixture = fixture_root();
        let manifest = fixture.join("pyproject.toml");
        fs::write(&manifest, vec![b'x'; 1024 * 1024 + 1]).unwrap();
        assert!(
            validate_python_profile_manifest(&fixture, "python_uv_group_dev_v1")
                .expect_err("oversized manifest must fail")
                .contains("oversized")
        );
        fs::write(&manifest, [0xff, 0xfe]).unwrap();
        assert!(
            validate_python_profile_manifest(&fixture, "python_uv_group_dev_v1")
                .expect_err("non-UTF-8 manifest must fail")
                .contains("UTF-8")
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn rebound_candidate_digest_cannot_hide_dirty_baseline_or_false_changed_list() {
        let (fixture, _, queue, receipt_id, _) = completed_verified_fixture(false).await;
        let receipt = queue.read_receipt(&receipt_id).unwrap();
        let candidate = queue.read_review_candidate(&receipt).unwrap();
        let mut job = queue
            .load_job("alpha", "verified_fixture")
            .unwrap()
            .unwrap();
        for (index, mut tampered) in [candidate.clone(), candidate].into_iter().enumerate() {
            if index == 0 {
                tampered.before.changed_paths.push(GitChangedPath {
                    path: "preexisting.txt".into(),
                    kind: "file".into(),
                    sha256: Some("c".repeat(64)),
                });
            } else {
                tampered.changed_during_turn[0] = "invented.txt".into();
            }
            let mut bytes = serde_json::to_vec_pretty(&tampered).unwrap();
            bytes.push(b'\n');
            fs::write(queue.review_path(&receipt_id), &bytes).unwrap();
            job.review_candidate_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
            queue.save_job(&job).unwrap();
            assert!(queue.read_review_candidate(&receipt).is_err());
        }
        cleanup(fixture);
    }

    #[tokio::test]
    async fn staged_index_drift_is_distinct_from_unchanged_worktree_bytes() {
        let (fixture, projects, _) = three_project_fixture(1);
        let control = &projects[0].workspace;
        let workspace = &projects[1].workspace;
        fs::write(workspace.join("source.txt"), b"A\n").unwrap();
        fixture_commit(workspace);
        let git = resolve_trusted_git(workspace, control).unwrap();
        let before = capture_git_review_witness(workspace, &git).await.unwrap();
        fs::write(workspace.join("source.txt"), b"C\n").unwrap();
        fixture_git_command(workspace, &["add", "source.txt"]);
        fs::write(workspace.join("source.txt"), b"B\n").unwrap();
        fs::write(workspace.join("new.txt"), b"new file\n").unwrap();
        fixture_git_command(workspace, &["add", "new.txt"]);
        let after = capture_git_review_witness(workspace, &git).await.unwrap();
        let candidate = fixture_candidate(&projects[1], before, after.clone());
        let (patch, staged_patch, files) = snapshot_verified_source(workspace, &git, &candidate)
            .await
            .unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&patch)),
            after.tracked_diff_sha256
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(&staged_patch)),
            after.index_diff_sha256
        );
        assert_eq!(files.len(), 2);
        assert_eq!(
            BASE64
                .decode(
                    files
                        .iter()
                        .find(|file| file.path == "source.txt")
                        .unwrap()
                        .content_base64
                        .as_ref()
                        .unwrap()
                )
                .unwrap(),
            b"B\n"
        );
        assert_eq!(
            BASE64
                .decode(
                    files
                        .iter()
                        .find(|file| file.path == "new.txt")
                        .unwrap()
                        .content_base64
                        .as_ref()
                        .unwrap()
                )
                .unwrap(),
            b"new file\n"
        );

        fs::write(workspace.join("source.txt"), b"D\n").unwrap();
        fixture_git_command(workspace, &["add", "source.txt"]);
        fs::write(workspace.join("source.txt"), b"B\n").unwrap();
        let index_drift = capture_git_review_witness(workspace, &git).await.unwrap();
        assert_eq!(after.status_sha256, index_drift.status_sha256);
        assert_eq!(after.tracked_diff_sha256, index_drift.tracked_diff_sha256);
        assert_eq!(after.changed_paths, index_drift.changed_paths);
        assert_ne!(after.index_diff_sha256, index_drift.index_diff_sha256);
        assert!(
            snapshot_verified_source(workspace, &git, &candidate)
                .await
                .is_err()
        );

        let mut legacy_json = serde_json::to_value(&candidate).unwrap();
        legacy_json["after"]
            .as_object_mut()
            .unwrap()
            .remove("indexDiffSha256");
        let legacy: QueueReviewCandidate = serde_json::from_value(legacy_json).unwrap();
        assert_eq!(legacy.after.index_diff_sha256, "");
        assert!(
            snapshot_verified_source(workspace, &git, &legacy)
                .await
                .is_err()
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn unbound_artifact_after_crash_requires_attention_without_replaying_commands() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let job = QueueJob::new(
            "orphan_artifact".into(),
            &projects[1],
            "fixture edit".into(),
            Some("gpt-6.1-sol".into()),
            Some("medium".into()),
        )
        .unwrap();
        queue.save_job(&job).unwrap();
        let mut claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let attempt = claimed.attempt.as_ref().unwrap();
        let receipt_id = attempt.attempt_id.clone();
        let actual_profile = CodexCliTurnProfileEvidenceV1 {
            codex_thread_id: claimed.binding.thread_id.clone(),
            codex_turn_id: Uuid::new_v4().to_string(),
            model_id: "gpt-6.1-sol".into(),
            reasoning_effort: "medium".into(),
            evidence_sha256: "a".repeat(64),
        };
        let receipt = QueueReceipt {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: receipt_id.clone(),
            attempt_id: receipt_id.clone(),
            project_id: claimed.binding.project_id.clone(),
            task_id: claimed.task_id.clone(),
            workspace: claimed.binding.workspace.clone(),
            thread_id: claimed.binding.thread_id.clone(),
            target_sha256: claimed.binding.target_sha256.clone(),
            prompt_sha256: claimed.prompt_sha256.clone(),
            requested_model: claimed.model.clone(),
            requested_effort: claimed.effort.clone(),
            actual_profile: Some(actual_profile),
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
            evidence_level: "NATIVE_EXIT_OBSERVED".into(),
            exit_code: Some(0),
            child_exit_observed: true,
            binding_still_matches: true,
            outcome: "NATIVE_EXIT_ZERO".into(),
            recorded_at_unix: unix_now().unwrap(),
        };
        queue.write_receipt(&receipt).unwrap();
        let candidate = QueueReviewCandidate::from_observed_turn(
            &claimed,
            &receipt,
            queue.persisted_receipt_sha256(&receipt).unwrap(),
            mock_git_witness(),
            mock_git_witness(),
        );
        let candidate_sha = queue.write_review_candidate(&candidate).unwrap();
        claimed.state = QueueState::NativeExitedZero;
        claimed.receipt_id = Some(receipt_id.clone());
        claimed.review_candidate_sha256 = Some(candidate_sha);
        queue.save_job(&claimed).unwrap();
        // Reproduce a power loss after the create-once artifact write and
        // before the job digest publication. No command should be launched.
        let artifact = QueueVerification {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: receipt_id.clone(),
            receipt_sha256: queue.persisted_receipt_sha256(&receipt).unwrap(),
            review_candidate_sha256: claimed.review_candidate_sha256.clone().unwrap(),
            project_id: receipt.project_id.clone(),
            task_id: receipt.task_id.clone(),
            workspace: receipt.workspace.clone(),
            thread_id: receipt.thread_id.clone(),
            verification_profile: claimed.binding.verification_profile.clone(),
            actual_profile: receipt.actual_profile.clone(),
            git_witness: candidate.after.clone(),
            patch_sha256: format!("{:x}", Sha256::digest([])),
            patch_base64: String::new(),
            index_patch_sha256: format!("{:x}", Sha256::digest([])),
            index_patch_base64: String::new(),
            files: Vec::new(),
            commands: Vec::new(),
            status: "LOCAL_COMMAND_FAILURE".into(),
            recorded_at_unix: unix_now().unwrap(),
            independent_review: "PENDING".into(),
            wake_ready: false,
        };
        queue.write_verification(&artifact).unwrap();
        let error = verify_receipt_in_store(&projects[0], &queue, &receipt_id)
            .await
            .expect_err("unbound artifact must require attention");
        assert!(
            error.contains("UNBOUND") || error.contains("unbound"),
            "{error}"
        );
        assert_eq!(
            queue
                .load_job("alpha", "orphan_artifact")
                .unwrap()
                .unwrap()
                .state,
            QueueState::Attention
        );
        assert!(queue.verification_path(&receipt_id).exists());
        cleanup(fixture);
    }

    #[test]
    fn parser_rejects_scope_and_profile_bypasses() {
        assert!(matches!(
            parse(&["operator", "codex", "queue", "status"]),
            Ok(QueueAction::Status)
        ));
        assert!(matches!(
            parse(&["operator", "codex", "queue", "dispatch"]),
            Ok(QueueAction::Dispatch)
        ));
        let receipt_id = Uuid::new_v4().to_string();
        assert!(matches!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "receipt",
                "--receipt-id",
                &receipt_id
            ]),
            Ok(QueueAction::Receipt { .. })
        ));
        assert!(matches!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "review",
                "--receipt-id",
                &receipt_id
            ]),
            Ok(QueueAction::Review { .. })
        ));
        assert!(matches!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "review-package",
                "--receipt-id",
                &receipt_id
            ]),
            Ok(QueueAction::ReviewPackage { .. })
        ));
        assert!(matches!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "advisory-review",
                "--receipt-id",
                &receipt_id
            ]),
            Ok(QueueAction::AdvisoryReview { .. })
        ));
        assert!(matches!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "operator-review-receipt",
                "--receipt-id",
                &receipt_id
            ]),
            Ok(QueueAction::OperatorReviewReceipt { .. })
        ));
        let operator_review = [
            "operator",
            "codex",
            "queue",
            "operator-review",
            "--project-id",
            "alpha",
            "--workspace",
            "C:/alpha",
            "--receipt-id",
            &receipt_id,
            "--expected-package-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--decision",
            "ACCEPT",
        ];
        assert!(matches!(
            parse(&operator_review),
            Ok(QueueAction::OperatorReview { .. })
        ));
        assert!(parse(&[&operator_review[..], &["--task", "unexpected"]].concat()).is_err());
        let mut wrong_decision = operator_review.to_vec();
        *wrong_decision.last_mut().unwrap() = "APPROVE_WAKE";
        assert!(parse(&wrong_decision).is_err());
        let mut wrong_package = operator_review.to_vec();
        wrong_package[11] = "A";
        assert!(parse(&wrong_package).is_err());
        let grant = [
            "operator",
            "codex",
            "queue",
            "review-disclosure-grant",
            "--project-id",
            "alpha",
            "--workspace",
            "C:/alpha",
            "--receipt-id",
            &receipt_id,
            "--expected-package-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ];
        assert!(matches!(
            parse(&grant),
            Ok(QueueAction::ReviewDisclosureGrant { .. })
        ));
        let mut invitation = grant.to_vec();
        invitation[3] = "review-invite-prepare";
        assert!(matches!(
            parse(&invitation),
            Ok(QueueAction::ReviewInvitePrepare { .. })
        ));
        assert!(parse(&[&invitation[..], &["--task", "unexpected"]].concat()).is_err());
        let mut renew = grant.to_vec();
        renew[3] = "review-disclosure-renew";
        assert!(matches!(
            parse(&renew),
            Ok(QueueAction::ReviewDisclosureRenew { .. })
        ));
        let revoke = [
            "operator",
            "codex",
            "queue",
            "review-disclosure-revoke",
            "--project-id",
            "alpha",
            "--receipt-id",
            &receipt_id,
            "--expected-package-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ];
        assert!(matches!(
            parse(&revoke),
            Ok(QueueAction::ReviewDisclosureRevoke { .. })
        ));
        assert!(parse(&[&revoke[..], &["--workspace", "C:/alpha"]].concat()).is_err());
        for disclosure in [&grant[..], &renew[..], &revoke[..]] {
            assert!(parse(&[disclosure, &["--task", "unexpected"]].concat()).is_err());
            let mut wrong_sha = disclosure.to_vec();
            let sha_index = wrong_sha
                .iter()
                .position(|part| *part == "--expected-package-sha256")
                .unwrap()
                + 1;
            wrong_sha[sha_index] = "A";
            assert!(parse(&wrong_sha).is_err());
        }
        assert!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "review-package",
                "--receipt-id",
                "../../other"
            ])
            .is_err()
        );
        assert!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "receipt",
                "--receipt-id",
                "../../other"
            ])
            .is_err()
        );
        let add = [
            "operator",
            "codex",
            "queue",
            "add",
            "--project-id",
            "alpha",
            "--workspace",
            "C:/alpha",
            "--task-id",
            "safe_one",
            "--task",
            "edit a test",
        ];
        assert!(matches!(parse(&add), Ok(QueueAction::Add { .. })));
        assert!(parse(&[&add[..], &["--sandbox", "danger-full-access"]].concat()).is_err());
        assert!(parse(&[&add[..], &["--task", "second"]].concat()).is_err());
        assert!(parse(&[&add[..], &["--model", "gpt;evil"]].concat()).is_err());
        assert!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "add",
                "--project-id",
                "alpha",
                "--workspace",
                "relative",
                "--task-id",
                "safe",
                "--task",
                "edit"
            ])
            .is_err()
        );
        assert!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "resume",
                "--project-id",
                "alpha",
                "--workspace",
                "C:/alpha",
                "--task-id",
                "safe",
                "--task",
                "replay"
            ])
            .is_err()
        );
        let profile_bind = [
            "operator",
            "codex",
            "queue",
            "profile-bind",
            "--project-id",
            "alpha",
            "--workspace",
            "C:/alpha",
            "--expected-profile",
            "rust_full",
            "--new-profile",
            "python_uv_group_dev_v1",
            "--expected-binding-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--confirm-chat-quiescent",
            "00000000-0000-0000-0000-000000000001",
        ];
        assert!(matches!(
            parse(&profile_bind),
            Ok(QueueAction::ProfileBind { .. })
        ));
        assert!(parse(&[&profile_bind[..], &["--task", "unexpected"]].concat()).is_err());
        assert!(parse(&profile_bind[..profile_bind.len() - 2]).is_err());
        let mut invalid = profile_bind;
        invalid[9] = "python_freeform";
        assert!(parse(&invalid).is_err());
        let mut wrong_thread = profile_bind;
        wrong_thread[15] = "not-a-thread";
        assert!(parse(&wrong_thread).is_err());
    }

    #[tokio::test]
    async fn profile_bind_changes_only_exact_quiescent_python_projects_and_retries_idempotently() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0];
        let registry = AutonomousProjectRegistryStoreV1::open_read_only(
            control.workspace.join(".catdesk/projects"),
        )
        .expect("registry");
        for (project, profile) in [
            (&projects[1], "python_uv_group_dev_v1"),
            (&projects[2], "python_uv_extras_dev_lab_v1"),
        ] {
            fixture_python_profile_inputs(project, profile);
            fixture_commit(&project.workspace);
            let expected_hash = project_binding_sha256(project).expect("old binding hash");
            let before = registry.load_registry().expect("before registry");
            let first = bind_profile_in_store(
                &control.workspace,
                &queue,
                project,
                "rust_full",
                profile,
                &expected_hash,
                exact_thread(project).expect("bound fixture thread"),
            )
            .await
            .expect("first binding");
            assert_eq!(first["idempotent"], false);
            assert_eq!(first["verificationProfile"], profile);
            let after = registry.load_registry().expect("after registry");
            assert_eq!(before.projects.len(), after.projects.len());
            for unchanged in before
                .projects
                .iter()
                .filter(|entry| entry.project_id != project.project_id)
            {
                assert!(
                    after.projects.contains(unchanged),
                    "sibling binding changed"
                );
            }
            let rebound = after
                .projects
                .iter()
                .find(|entry| entry.project_id == project.project_id)
                .expect("rebound project");
            let mut expected_rebound = project.clone();
            expected_rebound.verification_profile = profile.into();
            assert_eq!(rebound, &expected_rebound);
            let registry_bytes =
                fs::read(control.workspace.join(".catdesk/projects/projects.json"))
                    .expect("registry bytes");
            let retry = bind_profile_in_store(
                &control.workspace,
                &queue,
                rebound,
                "rust_full",
                profile,
                &expected_hash,
                exact_thread(rebound).expect("bound fixture thread"),
            )
            .await
            .expect("exact retry");
            assert_eq!(retry["idempotent"], true);
            assert_eq!(
                fs::read(control.workspace.join(".catdesk/projects/projects.json"))
                    .expect("registry after retry"),
                registry_bytes
            );
        }
        cleanup(fixture);
    }

    #[tokio::test]
    async fn profile_bind_refuses_binding_manifest_and_job_drift_without_registry_change() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0];
        let project = &projects[1];
        let profile = "python_uv_group_dev_v1";
        fixture_python_profile_inputs(project, profile);
        fixture_commit(&project.workspace);
        let expected_hash = project_binding_sha256(project).expect("binding hash");
        let registry_path = control.workspace.join(".catdesk/projects/projects.json");
        let original = fs::read(&registry_path).expect("registry");
        let git = resolve_trusted_git(&project.workspace, &control.workspace).expect("fixture Git");
        profile_git_inputs_clean_under_lock(&git, &project.workspace).expect("clean inputs");
        assert!(
            fixture_profile_bind(
                &control.workspace,
                &queue,
                project,
                profile,
                &"0".repeat(64)
            )
            .await
            .is_err()
        );
        assert!(
            bind_profile_in_store(
                &control.workspace,
                &queue,
                project,
                "rust_full",
                profile,
                &expected_hash,
                "00000000-0000-0000-0000-000000000001",
            )
            .await
            .is_err()
        );
        let mut drifted = project.clone();
        drifted.chatgpt_target_sha256 = Some("a".repeat(64));
        let drift_hash = project_binding_sha256(&drifted).expect("drift hash");
        assert!(
            fixture_profile_bind(&control.workspace, &queue, &drifted, profile, &drift_hash)
                .await
                .is_err()
        );
        for name in ["pyproject.toml", "uv.lock"] {
            for staged in [false, true] {
                let path = project.workspace.join(name);
                let mut modified = fs::read(&path).expect("profile input");
                modified.extend_from_slice(b"\n# valid but dirty profile input\n");
                fs::write(&path, modified).expect("dirty input");
                if staged {
                    fixture_git_command(&project.workspace, &["add", "--", name]);
                }
                assert!(profile_git_inputs_clean_under_lock(&git, &project.workspace).is_err());
                assert!(
                    fixture_profile_bind(
                        &control.workspace,
                        &queue,
                        project,
                        profile,
                        &expected_hash
                    )
                    .await
                    .is_err()
                );
                fixture_git_command(
                    &project.workspace,
                    &["restore", "--staged", "--worktree", "--", name],
                );
                profile_git_inputs_clean_under_lock(&git, &project.workspace)
                    .expect("restored clean inputs");
            }
        }
        for (name, set_bit, clear_bit) in [
            (
                "pyproject.toml",
                "--assume-unchanged",
                "--no-assume-unchanged",
            ),
            ("uv.lock", "--skip-worktree", "--no-skip-worktree"),
        ] {
            fixture_git_command(&project.workspace, &["update-index", set_bit, "--", name]);
            let path = project.workspace.join(name);
            let mut modified = fs::read(&path).expect("profile input");
            modified.extend_from_slice(b"\n# valid hidden edit\n");
            fs::write(&path, modified).expect("hidden edit");
            profile_binding_inputs(&project.workspace, profile).expect("valid hidden input");
            assert!(profile_git_inputs_clean_under_lock(&git, &project.workspace).is_err());
            assert!(
                fixture_profile_bind(&control.workspace, &queue, project, profile, &expected_hash)
                    .await
                    .is_err()
            );
            fixture_git_command(&project.workspace, &["update-index", clear_bit, "--", name]);
            fixture_git_command(
                &project.workspace,
                &["restore", "--staged", "--worktree", "--", name],
            );
            profile_git_inputs_clean_under_lock(&git, &project.workspace)
                .expect("restored clean inputs");
        }
        fs::write(
            project.workspace.join("pyproject.toml"),
            b"[project]\nname = \"foreign\"\n",
        )
        .expect("mismatched manifest");
        assert!(
            fixture_profile_bind(&control.workspace, &queue, project, profile, &expected_hash)
                .await
                .is_err()
        );
        fixture_python_profile_inputs(project, profile);
        fs::write(project.workspace.join("uv.lock"), b"version = 1\n").expect("mismatched lock");
        assert!(
            fixture_profile_bind(&control.workspace, &queue, project, profile, &expected_hash)
                .await
                .is_err()
        );
        fixture_python_profile_inputs(project, profile);
        let job = QueueJob::new(
            "pending".into(),
            project,
            "approved task".into(),
            None,
            None,
        )
        .expect("job");
        queue.save_job(&job).expect("queued job");
        assert!(
            fixture_profile_bind(&control.workspace, &queue, project, profile, &expected_hash)
                .await
                .is_err()
        );
        assert_eq!(
            fs::read(&registry_path).expect("registry after refusals"),
            original
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn profile_bind_refuses_held_writer_lease_and_stale_registry_cas() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0];
        let project = &projects[1];
        let profile = "python_uv_group_dev_v1";
        fixture_python_profile_inputs(project, profile);
        fixture_commit(&project.workspace);
        let expected_hash = project_binding_sha256(project).expect("binding hash");
        let registry_path = control.workspace.join(".catdesk/projects/projects.json");
        let registry = AutonomousProjectRegistryStoreV1::open_read_only(
            control.workspace.join(".catdesk/projects"),
        )
        .expect("registry");
        let original = fs::read(&registry_path).expect("registry before refusals");

        let writer = acquire_project_writer_lock(&project.workspace).expect("held fixture writer");
        assert!(
            fixture_profile_bind(&control.workspace, &queue, project, profile, &expected_hash)
                .await
                .is_err()
        );
        assert_eq!(
            fs::read(&registry_path).expect("registry with writer"),
            original
        );
        drop(writer);

        let lease = match registry
            .schedule_mutating_worker("alpha", "fixture-owner", ScheduledProviderV1::Codex, 1, 2)
            .expect("fixture lease")
        {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            other => panic!("expected lease, got {other:?}"),
        };
        assert!(
            fixture_profile_bind(&control.workspace, &queue, project, profile, &expected_hash)
                .await
                .is_err()
        );
        assert_eq!(
            fs::read(&registry_path).expect("registry with lease"),
            original
        );
        registry
            .release_workspace_lease(&lease)
            .expect("release fixture lease");

        // Simulate a registry update after the operator read the old binding.
        let (updated, idempotent) = registry
            .bind_verification_profile_after(project, "rust_full", profile, || Ok(()))
            .expect("fixture registry change");
        assert!(!idempotent);
        assert_eq!(updated.verification_profile, profile);
        let changed = fs::read(&registry_path).expect("changed registry");
        assert!(
            fixture_profile_bind(&control.workspace, &queue, project, profile, &expected_hash)
                .await
                .is_err()
        );
        assert_eq!(
            fs::read(&registry_path).expect("registry after stale request"),
            changed
        );
        cleanup(fixture);
    }

    #[test]
    fn records_are_idempotent_and_status_omits_prompt() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let job = QueueJob::new(
            "task_one".into(),
            &projects[1],
            "private prompt text".into(),
            Some("gpt-6.1-sol".into()),
            Some("medium".into()),
        )
        .expect("job");
        queue.save_job(&job).expect("persist job");
        let same = QueueJob::new(
            "task_one".into(),
            &projects[1],
            "private prompt text".into(),
            Some("gpt-6.1-sol".into()),
            Some("medium".into()),
        )
        .expect("same job");
        assert!(
            queue
                .load_job("alpha", "task_one")
                .expect("load")
                .unwrap()
                .same_request(&same)
        );
        let changed = QueueJob::new(
            "task_one".into(),
            &projects[1],
            "different prompt".into(),
            None,
            None,
        )
        .expect("changed");
        assert!(!job.same_request(&changed));
        let summary = job.summary();
        assert_eq!(
            summary["verificationProfile"],
            projects[1].verification_profile
        );
        assert_eq!(
            summary["gitIdentitySha256"],
            format!("{:x}", Sha256::digest(projects[1].git_identity.as_bytes()))
        );
        let summary_text = summary.to_string();
        assert!(!summary_text.contains("private prompt text"));
        assert!(!summary_text.contains(&projects[1].git_identity));
        let stored = fs::read_to_string(queue.job_path("alpha", "task_one")).unwrap();
        assert!(!stored.contains("https://chatgpt.com/c/"));
        assert_eq!(queue.jobs().expect("enumerate").len(), 1);
        cleanup(fixture);
    }

    #[test]
    fn fresh_attempt_persists_exact_marker_binding_and_legacy_remains_readable() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let job = QueueJob::new(
            "bound_task".into(),
            &projects[1],
            "fixture edit".into(),
            None,
            None,
        )
        .expect("job");
        queue.save_job(&job).expect("persist ready job");
        let claimed = claim_one(&projects[0].workspace, &queue)
            .expect("claim")
            .expect("selected job");
        let attempt = claimed.attempt.as_ref().expect("attempt");
        let expected = queue_writer_marker_sha256(&attempt.attempt_id).expect("marker digest");
        assert_eq!(
            attempt.writer_marker_sha256.as_deref(),
            Some(expected.as_str())
        );
        assert!(attempt.matches_writer_marker(Some(&expected)));
        assert!(!attempt.matches_writer_marker(None));
        assert!(!attempt.matches_writer_marker(Some(&"0".repeat(64))));
        assert_eq!(
            queue
                .load_job(&claimed.binding.project_id, &claimed.task_id)
                .expect("loaded job")
                .expect("stored job")
                .attempt,
            claimed.attempt
        );

        let mut legacy = claimed.clone();
        legacy.attempt.as_mut().unwrap().writer_marker_sha256 = None;
        queue
            .save_job(&legacy)
            .expect("legacy attempt stays readable");
        let loaded = queue
            .load_job(&legacy.binding.project_id, &legacy.task_id)
            .expect("legacy loaded")
            .expect("legacy job");
        assert!(
            !loaded
                .attempt
                .unwrap()
                .matches_writer_marker(Some(&expected))
        );

        let mut tampered = claimed.clone();
        tampered.attempt.as_mut().unwrap().writer_marker_sha256 = Some("0".repeat(64));
        assert!(queue.save_job(&tampered).is_err());
        let mut changed_attempt = claimed.clone();
        changed_attempt.attempt.as_mut().unwrap().attempt_id = Uuid::new_v4().to_string();
        assert!(queue.save_job(&changed_attempt).is_err());
        fs::write(
            queue.job_path(&claimed.binding.project_id, &claimed.task_id),
            serde_json::to_vec(&tampered).unwrap(),
        )
        .expect("tamper fixture record");
        assert!(
            queue
                .load_job(&claimed.binding.project_id, &claimed.task_id)
                .is_err()
        );
        cleanup(fixture);
    }

    #[test]
    fn fair_claim_and_observed_failure_release_exact_lease() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in &projects {
            let job = QueueJob::new(
                format!("job_{}", project.project_id),
                project,
                "fixture edit".into(),
                None,
                None,
            )
            .expect("job");
            queue.save_job(&job).expect("persist job");
        }
        let control = &projects[0].workspace;
        let mut selected = Vec::new();
        for iteration in 0..3 {
            let claimed = claim_one(control, &queue).expect("claim").expect("job");
            selected.push(claimed.binding.project_id.clone());
            let result = TurnResult {
                outcome: if iteration == 0 {
                    "NATIVE_EXIT_NONZERO"
                } else {
                    "NATIVE_EXIT_ZERO"
                },
                exit_code: Some(if iteration == 0 { 1 } else { 0 }),
                child_exit_observed: true,
                child_absent_proven: false,
                marker_retired: true,
                native_executable: None,
                native_version: None,
                native_executable_sha256: None,
                actual_profile: None,
                review_before: Some(mock_git_witness()),
                review_after: Some(mock_git_witness()),
            };
            let receipt = complete_attempt(control, &queue, &claimed, result).expect("receipt");
            assert_eq!(receipt["leaseReleased"], true);
            assert_eq!(receipt["receipt"]["threadId"], claimed.binding.thread_id);
            assert_eq!(receipt["receipt"]["evidenceLevel"], "NATIVE_EXIT_OBSERVED");
            assert_eq!(
                receipt["receipt"]["receiptId"],
                claimed.attempt.as_ref().unwrap().attempt_id
            );
            let reread = queue
                .read_receipt(&claimed.attempt.as_ref().unwrap().attempt_id)
                .unwrap();
            assert_eq!(reread.thread_id, claimed.binding.thread_id);
        }
        assert_eq!(selected, ["alpha", "beta", "catdesk"]);
        assert_eq!(
            queue
                .load_job("catdesk", "job_catdesk")
                .unwrap()
                .unwrap()
                .state,
            QueueState::NativeExitedZero
        );
        assert_eq!(
            queue.load_job("alpha", "job_alpha").unwrap().unwrap().state,
            QueueState::Failed
        );
        assert_eq!(
            fs::read_dir(control.join(".catdesk/projects/leases"))
                .unwrap()
                .count(),
            0
        );
        assert!(!projects[1].workspace.join(".catdesk").exists());
        assert!(!projects[2].workspace.join(".catdesk").exists());
        cleanup(fixture);
    }

    #[test]
    fn valid_registry_without_scheduler_children_initializes_on_claim() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0].workspace;
        let registry_root = control.join(".catdesk/projects");
        for child in ["leases", "registration-confirmations", "thread-adoptions"] {
            fs::remove_dir(registry_root.join(child)).expect("remove empty fixture child");
        }
        queue
            .save_job(
                &QueueJob::new(
                    "missing_leases".into(),
                    &projects[1],
                    "fixture task".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let registered = scheduler(control)
            .unwrap()
            .load_registry()
            .unwrap()
            .projects;
        assert_eq!(registered.len(), projects.len());
        for project in &projects {
            assert!(registered.contains(project));
        }
        assert!(!registry_root.join("leases").exists());
        assert!(!registry_root.join("scheduler.lock").exists());

        let claimed = claim_one(control, &queue)
            .expect("claim")
            .expect("ready job");
        assert_eq!(claimed.binding.project_id, "alpha");
        assert_eq!(claimed.state, QueueState::Claimed);
        assert!(registry_root.join("leases").is_dir());
        assert!(registry_root.join("scheduler.lock").is_file());
        scheduler(control)
            .unwrap()
            .release_workspace_lease(&claimed.attempt.unwrap().lease)
            .expect("release exact fixture lease");
        cleanup(fixture);
    }

    #[test]
    fn corrupt_scheduler_child_refuses_claim_without_consuming_job() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0].workspace;
        let registry_root = control.join(".catdesk/projects");
        fs::remove_dir(registry_root.join("leases")).expect("remove empty fixture leases");
        fs::write(registry_root.join("leases"), b"not a directory")
            .expect("corrupt fixture scheduler child");
        queue
            .save_job(
                &QueueJob::new(
                    "corrupt_leases".into(),
                    &projects[1],
                    "fixture task".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();

        assert!(
            claim_one(control, &queue)
                .expect_err("unsafe scheduler child")
                .contains("scheduler state is unavailable or unsafe")
        );
        let job = queue.load_job("alpha", "corrupt_leases").unwrap().unwrap();
        assert_eq!(job.state, QueueState::Ready);
        assert!(job.attempt.is_none());
        assert!(!registry_root.join("scheduler.lock").exists());
        cleanup(fixture);
    }

    #[test]
    fn origin_drift_never_reroutes_ready_job() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        assert!(
            StdCommand::new("git")
                .arg("-C")
                .arg(&projects[1].workspace)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/changed.git"
                ])
                .status()
                .expect("Git drift")
                .success()
        );
        let claimed = claim_one(&projects[0].workspace, &queue)
            .expect("claim")
            .expect("other project");
        assert_eq!(claimed.binding.project_id, "beta");
        assert_eq!(
            queue.load_job("alpha", "job_alpha").unwrap().unwrap().state,
            QueueState::Attention
        );
        assert!(!projects[1].workspace.join(".catdesk").exists());
        // The fixture owns exactly this lease; no child was launched.
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&claimed.attempt.unwrap().lease)
            .unwrap();
        cleanup(fixture);
    }

    #[test]
    fn profile_drift_never_claims_ready_job() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .expect("job"),
                )
                .expect("persist job");
        }
        let registry_path = projects[0]
            .workspace
            .join(".catdesk/projects/projects.json");
        let mut registry: AutonomousProjectRegistryV1 =
            serde_json::from_slice(&fs::read(&registry_path).expect("registry"))
                .expect("registry JSON");
        registry
            .projects
            .iter_mut()
            .find(|project| project.project_id == "alpha")
            .expect("alpha registration")
            .verification_profile = "python_uv_group_dev_v1".into();
        fs::write(
            &registry_path,
            serde_json::to_vec(&registry).expect("registry JSON"),
        )
        .expect("profile drift fixture");

        let claimed = claim_one(&projects[0].workspace, &queue)
            .expect("claim")
            .expect("other project remains ready");
        assert_eq!(claimed.binding.project_id, "beta");
        assert_eq!(
            queue
                .load_job("alpha", "job_alpha")
                .expect("alpha job")
                .expect("persisted alpha job")
                .state,
            QueueState::Attention
        );
        assert!(!projects[1].workspace.join(".catdesk").exists());
        scheduler(&projects[0].workspace)
            .expect("fixture scheduler")
            .release_workspace_lease(&claimed.attempt.expect("exact attempt").lease)
            .expect("release fixture lease");
        cleanup(fixture);
    }

    #[test]
    fn claimed_attempt_remains_durable_and_is_never_reselected() {
        let (fixture, projects, queue) = three_project_fixture(2);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let first = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        assert_eq!(first.binding.project_id, "alpha");
        let second = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        assert_eq!(second.binding.project_id, "beta");
        assert_eq!(
            queue.load_job("alpha", "job_alpha").unwrap().unwrap().state,
            QueueState::Claimed
        );
        assert!(claim_one(&projects[0].workspace, &queue).unwrap().is_none());
        for claimed in [first, second] {
            scheduler(&projects[0].workspace)
                .unwrap()
                .release_workspace_lease(&claimed.attempt.unwrap().lease)
                .unwrap();
        }
        cleanup(fixture);
    }

    #[test]
    fn prelaunch_writer_refusal_keeps_task_ready_and_releases_lease() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let first = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        assert_eq!(first.binding.project_id, "alpha");
        let outcome = complete_attempt(
            &projects[0].workspace,
            &queue,
            &first,
            TurnResult::before_spawn("PROJECT_WRITER_UNAVAILABLE"),
        )
        .unwrap();
        assert_eq!(outcome["leaseReleased"], true);
        assert_eq!(outcome["job"]["state"], "READY");
        assert_eq!(outcome["receipt"]["childExitObserved"], false);
        assert_eq!(
            outcome["receipt"]["evidenceLevel"],
            "CHILD_ABSENT_BEFORE_SPAWN"
        );
        assert_eq!(
            queue
                .read_receipt(&first.attempt.as_ref().unwrap().attempt_id)
                .unwrap()
                .outcome,
            "PROJECT_WRITER_UNAVAILABLE"
        );
        let next = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        assert_eq!(next.binding.project_id, "beta");
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&next.attempt.unwrap().lease)
            .unwrap();
        cleanup(fixture);
    }

    #[tokio::test]
    async fn unresolved_accounting_refuses_queued_native_before_spawn() {
        let (fixture, projects, _queue) = three_project_fixture(1);
        let executable = fake_native_cli(&fixture);
        let cli = preflight_selected_cli(executable, None, &projects[0].workspace)
            .await
            .expect("fixture native preflight");
        let alpha = &projects[1];
        let store = AutonomousStateStoreV1::open(alpha.workspace.join(".catdesk/autonomy"))
            .expect("managed state fixture");
        store
            .create_session(
                "historical-task",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "work".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::Ready,
                }])
                .expect("managed queue"),
            )
            .expect("managed session");
        let mut snapshot = store.load_session("historical-task").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        store.save_session(&snapshot).expect("terminal snapshot");
        store
            .execution_accounting_store()
            .expect("accounting store")
            .record_task_started(
                "alpha",
                "work",
                "historical-task",
                Some("thread"),
                "codex-cli",
                None,
                None,
                100,
            )
            .expect("unended accounting record");
        let job = QueueJob::new(
            "queued_work".into(),
            alpha,
            "fixture edit".into(),
            None,
            None,
        )
        .expect("queue job");
        let turn =
            run_queued_turn(&job, &projects[0].workspace, &cli, Duration::from_secs(30)).await;
        assert_eq!(turn.outcome, "MANAGED_WRITER_UNRESOLVED");
        assert!(turn.child_absent_proven);
        assert!(
            !alpha
                .workspace
                .join(format!("queue-proof-{}.txt", job.binding.thread_id))
                .exists()
        );
        cleanup(fixture);
    }

    #[test]
    fn unobserved_child_blocks_replay_and_retains_exact_lease() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let first = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let outcome = complete_attempt(
            &projects[0].workspace,
            &queue,
            &first,
            TurnResult::unknown("NATIVE_EXIT_UNOBSERVED"),
        )
        .unwrap();
        assert_eq!(outcome["leaseReleased"], false);
        assert_eq!(outcome["job"]["state"], "UNKNOWN");
        assert_eq!(outcome["receipt"]["evidenceLevel"], "PROCESS_STATE_UNKNOWN");
        assert_eq!(
            queue.load_job("alpha", "job_alpha").unwrap().unwrap().state,
            QueueState::Unknown
        );
        let error = claim_one(&projects[0].workspace, &queue).expect_err("provider capacity held");
        assert!(error.contains("CAPACITY"), "{error}");
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&first.attempt.unwrap().lease)
            .unwrap();
        cleanup(fixture);
    }

    #[test]
    fn uncertain_prompt_keeps_writer_marker_and_lease_for_owner_recovery() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let first = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let attempt = first.attempt.as_ref().expect("claimed attempt");
        let mut writer = acquire_project_writer_lock(&projects[1].workspace)
            .expect("claimed project writer is available");
        writer
            .begin_queue_writer_turn(&attempt.attempt_id)
            .expect("exact attempt marker");
        let marker = inspect_project_writer_pending_marker(&projects[1].workspace)
            .expect("writer marker readback")
            .expect("writer marker exists");
        assert!(attempt.matches_writer_marker(Some(&marker)));

        let outcome = complete_attempt(
            &projects[0].workspace,
            &queue,
            &first,
            TurnResult::unknown("PROMPT_PIPE_DESCENDANTS_UNPROVEN"),
        )
        .expect("uncertain prompt receipt");
        assert_eq!(outcome["leaseReleased"], false);
        assert_eq!(outcome["job"]["state"], "UNKNOWN");
        assert_eq!(
            outcome["receipt"]["outcome"],
            "PROMPT_PIPE_DESCENDANTS_UNPROVEN"
        );
        assert_eq!(outcome["receipt"]["evidenceLevel"], "PROCESS_STATE_UNKNOWN");
        assert_eq!(
            inspect_project_writer_pending_marker(&projects[1].workspace)
                .expect("marker after uncertain prompt")
                .as_deref(),
            Some(marker.as_str())
        );
        let error = claim_one(&projects[0].workspace, &queue).expect_err("lease blocks replay");
        assert!(error.contains("CAPACITY"), "{error}");

        // Fixture teardown is an explicit owner action; the runtime path did
        // not retire this marker or release the lease on ambiguous delivery.
        writer.finish_writer_turn().expect("retire fixture marker");
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&attempt.lease)
            .unwrap();
        cleanup(fixture);
    }

    #[test]
    fn unsafe_or_oversized_queue_entries_fail_closed() {
        let fixture = fixture_root();
        let parent = fixture.join("user-state");
        fs::create_dir(&parent).unwrap();
        let workspace = fixture.join("project");
        fs::create_dir(&workspace).unwrap();
        assert!(reject_overlap(&workspace.join("CatDesk"), &workspace).is_err());
        assert!(!workspace.join("CatDesk").exists());
        let queue = QueueStore::open_at(parent.join("OperatorCodexQueue")).unwrap();
        fs::write(
            queue.root.join("jobs/invalid.json"),
            vec![b'x'; MAX_JOB_BYTES as usize + 1],
        )
        .unwrap();
        assert!(queue.jobs().is_err());
        fs::remove_file(queue.root.join("jobs/invalid.json")).unwrap();
        for index in 0..=MAX_DIR_ENTRIES {
            fs::write(
                queue.root.join("jobs").join(format!(".draft-{index}.tmp")),
                b"",
            )
            .unwrap();
        }
        assert!(queue.jobs().is_err());
        cleanup(fixture);
    }

    #[test]
    fn separate_verified_controls_use_distinct_queue_namespaces() {
        let (fixture_a, projects_a, _) = three_project_fixture(1);
        let (fixture_b, projects_b, _) = three_project_fixture(1);
        let path_a = QueueStore::namespaced_path(&projects_a[0], &projects_a[0].workspace).unwrap();
        let path_b = QueueStore::namespaced_path(&projects_b[0], &projects_b[0].workspace).unwrap();
        assert_ne!(path_a, path_b);
        assert!(
            QueueStore::open_read_only(&projects_a[0])
                .unwrap()
                .is_none()
        );
        assert!(!path_a.exists());
        let root = fixture_root();
        let parent = root.join("state");
        fs::create_dir(&parent).unwrap();
        let queue_a = QueueStore::open_at(parent.join(path_a.file_name().unwrap())).unwrap();
        let queue_b = QueueStore::open_at(parent.join(path_b.file_name().unwrap())).unwrap();
        queue_a
            .save_job(
                &QueueJob::new(
                    "only_a".into(),
                    &projects_a[1],
                    "fixture".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(queue_a.jobs().unwrap().len(), 1);
        assert!(queue_b.jobs().unwrap().is_empty());
        assert!(
            claim_one(&projects_b[0].workspace, &queue_b)
                .unwrap()
                .is_none()
        );
        assert_eq!(queue_a.jobs().unwrap().len(), 1);
        cleanup(root);
        cleanup(fixture_a);
        cleanup(fixture_b);
    }

    #[test]
    fn attempt_id_finds_receipt_after_publication_before_job_update() {
        let (fixture, projects, queue) = three_project_fixture(1);
        queue
            .save_job(
                &QueueJob::new(
                    "crash_window".into(),
                    &projects[1],
                    "fixture".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let attempt = claimed.attempt.as_ref().unwrap();
        let receipt = QueueReceipt {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: attempt.attempt_id.clone(),
            attempt_id: attempt.attempt_id.clone(),
            project_id: claimed.binding.project_id.clone(),
            task_id: claimed.task_id.clone(),
            workspace: claimed.binding.workspace.clone(),
            thread_id: claimed.binding.thread_id.clone(),
            target_sha256: claimed.binding.target_sha256.clone(),
            prompt_sha256: claimed.prompt_sha256.clone(),
            requested_model: None,
            requested_effort: None,
            actual_profile: None,
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
            evidence_level: "NATIVE_EXIT_OBSERVED".into(),
            exit_code: Some(0),
            child_exit_observed: true,
            binding_still_matches: true,
            outcome: "NATIVE_EXIT_ZERO".into(),
            recorded_at_unix: unix_now().unwrap(),
        };
        queue.write_receipt(&receipt).unwrap();
        assert_eq!(
            queue
                .load_job("alpha", "crash_window")
                .unwrap()
                .unwrap()
                .state,
            QueueState::Claimed
        );
        assert_eq!(queue.read_receipt(&attempt.attempt_id).unwrap(), receipt);
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&attempt.lease)
            .unwrap();
        cleanup(fixture);
    }

    #[test]
    fn exact_lease_release_failure_leaves_attention_and_readable_receipt() {
        let (fixture, projects, queue) = three_project_fixture(1);
        queue
            .save_job(
                &QueueJob::new(
                    "release_fault".into(),
                    &projects[1],
                    "fixture".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let attempt = claimed.attempt.as_ref().unwrap();
        assert!(attempt.lease.expires_at_unix - attempt.started_at_unix >= 12 * 60 * 60);
        // Inject an exact lease disappearance before completion; the receipt
        // remains discoverable, and no job is reported as completed.
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&attempt.lease)
            .unwrap();
        let error = complete_attempt(
            &projects[0].workspace,
            &queue,
            &claimed,
            TurnResult {
                outcome: "NATIVE_EXIT_ZERO",
                exit_code: Some(0),
                child_exit_observed: true,
                child_absent_proven: false,
                marker_retired: true,
                native_executable: None,
                native_version: None,
                native_executable_sha256: None,
                actual_profile: None,
                review_before: Some(mock_git_witness()),
                review_after: Some(mock_git_witness()),
            },
        )
        .expect_err("exact release missing");
        assert!(error.contains("needs attention"), "{error}");
        assert_eq!(
            queue
                .load_job("alpha", "release_fault")
                .unwrap()
                .unwrap()
                .state,
            QueueState::Attention
        );
        assert_eq!(
            queue
                .read_receipt(&attempt.attempt_id)
                .unwrap()
                .evidence_level,
            "NATIVE_EXIT_OBSERVED"
        );
        cleanup(fixture);
    }

    #[test]
    fn oversized_receipt_is_rejected_before_publication() {
        let fixture = fixture_root();
        let parent = fixture.join("state");
        fs::create_dir(&parent).unwrap();
        let queue = QueueStore::open_at(parent.join("queue")).unwrap();
        let receipt_id = Uuid::new_v4().to_string();
        let receipt = QueueReceipt {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: receipt_id.clone(),
            attempt_id: receipt_id.clone(),
            project_id: "fixture".into(),
            task_id: "large".into(),
            workspace: PathBuf::from("x".repeat(MAX_RECEIPT_BYTES as usize)),
            thread_id: Uuid::new_v4().to_string(),
            target_sha256: None,
            prompt_sha256: "0".repeat(64),
            requested_model: None,
            requested_effort: None,
            actual_profile: None,
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
            evidence_level: "CHILD_ABSENT_BEFORE_SPAWN".into(),
            exit_code: None,
            child_exit_observed: false,
            binding_still_matches: false,
            outcome: "FIXTURE".into(),
            recorded_at_unix: unix_now().unwrap(),
        };
        assert!(
            queue
                .write_receipt(&receipt)
                .unwrap_err()
                .contains("oversized")
        );
        assert!(
            !queue
                .root
                .join("receipts")
                .join(format!("{receipt_id}.json"))
                .exists()
        );
        cleanup(fixture);
    }

    #[test]
    fn review_snapshot_failures_keep_process_receipts_and_need_attention() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for (task_id, turn) in [
            (
                "no_baseline",
                TurnResult::before_spawn("REVIEW_BASELINE_UNAVAILABLE"),
            ),
            (
                "no_post_snapshot",
                TurnResult {
                    outcome: "REVIEW_POST_SNAPSHOT_UNAVAILABLE",
                    exit_code: Some(0),
                    child_exit_observed: true,
                    child_absent_proven: false,
                    marker_retired: true,
                    native_executable: None,
                    native_version: None,
                    native_executable_sha256: None,
                    actual_profile: None,
                    review_before: Some(mock_git_witness()),
                    review_after: None,
                },
            ),
        ] {
            queue
                .save_job(
                    &QueueJob::new(task_id.into(), &projects[1], "fixture".into(), None, None)
                        .unwrap(),
                )
                .unwrap();
            let claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
            let result = complete_attempt(&projects[0].workspace, &queue, &claimed, turn).unwrap();
            assert_eq!(result["job"]["state"], "ATTENTION");
            assert_eq!(result["leaseReleased"], true);
            let receipt_id = result["receipt"]["receiptId"].as_str().unwrap();
            let receipt = queue
                .read_receipt(receipt_id)
                .expect("process receipt survives");
            assert!(queue.read_review_candidate(&receipt).is_err());
        }
        cleanup(fixture);
    }

    #[test]
    fn zero_exit_with_drift_or_unretired_marker_never_publishes_review_candidate() {
        for drift in [false, true] {
            let (fixture, projects, queue) = three_project_fixture(1);
            queue
                .save_job(
                    &QueueJob::new(
                        "review_hold".into(),
                        &projects[1],
                        "fixture".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
            let claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
            if drift {
                assert!(
                    StdCommand::new("git")
                        .arg("-C")
                        .arg(&projects[1].workspace)
                        .args([
                            "remote",
                            "set-url",
                            "origin",
                            "https://example.test/drift.git"
                        ])
                        .status()
                        .unwrap()
                        .success()
                );
            }
            let result = complete_attempt(
                &projects[0].workspace,
                &queue,
                &claimed,
                TurnResult {
                    outcome: "NATIVE_EXIT_ZERO",
                    exit_code: Some(0),
                    child_exit_observed: true,
                    child_absent_proven: false,
                    marker_retired: drift,
                    native_executable: None,
                    native_version: None,
                    native_executable_sha256: None,
                    actual_profile: None,
                    review_before: Some(mock_git_witness()),
                    review_after: Some(mock_git_witness()),
                },
            )
            .unwrap();
            assert_eq!(result["job"]["state"], "ATTENTION");
            assert_eq!(result["job"]["reviewCandidateSha256"], Value::Null);
            let receipt_id = result["receipt"]["receiptId"].as_str().unwrap();
            let receipt = queue.read_receipt(receipt_id).unwrap();
            assert!(queue.read_review_candidate(&receipt).is_err());
            cleanup(fixture);
        }
    }

    #[tokio::test]
    async fn legacy_claim_without_marker_binding_never_spawns_native_child() {
        for baseline in ["unborn", "untracked"] {
            let (fixture, projects, queue) = three_project_fixture(1);
            if baseline == "untracked" {
                fs::write(projects[1].workspace.join("baseline.txt"), "clean").unwrap();
                fixture_commit(&projects[1].workspace);
                fs::write(projects[1].workspace.join("other-work.txt"), "untouched").unwrap();
            }
            let executable = fake_native_cli(&fixture);
            let cli = preflight_selected_cli(executable, None, &projects[0].workspace)
                .await
                .expect("fake native preflight");
            queue
                .save_job(
                    &QueueJob::new(
                        "legacy_claim".into(),
                        &projects[1],
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .expect("job"),
                )
                .expect("ready job");
            let mut claimed = claim_one(&projects[0].workspace, &queue)
                .expect("claim")
                .expect("selected job");
            claimed.attempt.as_mut().unwrap().writer_marker_sha256 = None;
            queue
                .save_job(&claimed)
                .expect("legacy record stays readable");
            let turn = run_queued_turn(
                &claimed,
                &projects[0].workspace,
                &cli,
                Duration::from_secs(30),
            )
            .await;
            assert_eq!(turn.outcome, "QUEUE_ATTEMPT_MARKER_UNBOUND");
            assert!(turn.child_absent_proven);
            assert!(!turn.marker_retired);
            assert!(
                !projects[1]
                    .workspace
                    .join(format!("queue-proof-{}.txt", claimed.binding.thread_id))
                    .exists()
            );
            let result = complete_attempt(&projects[0].workspace, &queue, &claimed, turn)
                .expect("legacy refusal receipt");
            assert_eq!(result["job"]["state"], "ATTENTION");
            assert_eq!(result["leaseReleased"], true);
            assert_eq!(
                result["receipt"]["evidenceLevel"],
                "CHILD_ABSENT_BEFORE_SPAWN"
            );
            assert_eq!(result["receipt"]["outcome"], "QUEUE_ATTEMPT_MARKER_UNBOUND");
            cleanup(fixture);
        }
    }

    #[tokio::test]
    async fn fake_native_child_produces_project_scoped_receipts_and_proofs() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            fs::write(project.workspace.join("baseline.txt"), "before").expect("baseline fixture");
            fixture_commit(&project.workspace);
        }
        let executable = fake_native_cli(&fixture);
        let cli = preflight_selected_cli(executable, None, &projects[0].workspace)
            .await
            .expect("fake direct executable preflight");
        assert_eq!(cli.version, "codex-cli fake-fixture");
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        format!("edit only {}", project.project_id),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        for expected in ["alpha", "beta"] {
            let claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
            assert_eq!(claimed.binding.project_id, expected);
            let turn = run_queued_turn(
                &claimed,
                &projects[0].workspace,
                &cli,
                Duration::from_secs(30),
            )
            .await
            .with_cli(&cli);
            assert!(turn.child_exit_observed && turn.marker_retired, "{turn:?}");
            let result = complete_attempt(&projects[0].workspace, &queue, &claimed, turn).unwrap();
            assert_eq!(result["job"]["state"], "NATIVE_EXITED_ZERO");
            assert_eq!(result["receipt"]["nativeVersion"], "codex-cli fake-fixture");
            assert_eq!(result["receipt"]["nativeExecutableSha256"], cli.sha256);
            assert_eq!(result["receipt"]["evidenceLevel"], "NATIVE_EXIT_OBSERVED");
            let receipt_id = result["receipt"]["receiptId"].as_str().unwrap();
            let receipt = queue
                .read_receipt(receipt_id)
                .expect("durable process receipt");
            let candidate = queue
                .read_review_candidate(&receipt)
                .expect("bound unreviewed candidate");
            assert_eq!(candidate.review_status, "UNREVIEWED");
            assert_eq!(candidate.verification_status, "NOT_VERIFIED_BY_CATDESK");
            assert!(!candidate.wake_ready);
            assert!(
                candidate
                    .changed_during_turn
                    .contains(&format!("queue-proof-{}.txt", claimed.binding.thread_id))
            );
            assert!(!candidate.baseline_dirty);
            assert!(candidate.before.changed_paths.is_empty());
            assert_eq!(candidate.before.head_commit, candidate.after.head_commit);
            let path = queue.review_path(receipt_id);
            let mut tampered: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            tampered["reviewStatus"] = "REVIEWED".into();
            fs::write(&path, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();
            assert!(queue.read_review_candidate(&receipt).is_err());
            let project = projects
                .iter()
                .find(|project| project.project_id == expected)
                .unwrap();
            let proof = project
                .workspace
                .join(format!("queue-proof-{}.txt", claimed.binding.thread_id));
            assert_eq!(
                fs::read_to_string(proof).unwrap(),
                format!("edit only {expected}")
            );
            fs::write(
                project
                    .workspace
                    .join(format!("queue-proof-{}.txt", claimed.binding.thread_id)),
                "later edit",
            )
            .unwrap();
            let git = resolve_trusted_git(&project.workspace, &projects[0].workspace).unwrap();
            assert_ne!(
                capture_git_review_witness(&project.workspace, &git)
                    .await
                    .unwrap(),
                candidate.after,
                "a later same-path edit must fail the current-worktree comparison"
            );
            for other in projects
                .iter()
                .filter(|project| project.project_id != expected)
            {
                assert!(
                    !other
                        .workspace
                        .join(format!("queue-proof-{}.txt", claimed.binding.thread_id))
                        .exists()
                );
            }
        }
        cleanup(fixture);
    }

    #[tokio::test]
    async fn unreviewable_project_refuses_child_and_does_not_starve_next_project() {
        for baseline in ["unborn", "staged", "untracked"] {
            let (fixture, projects, queue) = three_project_fixture(1);
            let control = &projects[0].workspace;
            let alpha = &projects[1];
            let beta = &projects[2];
            fs::write(beta.workspace.join("baseline.txt"), "clean").unwrap();
            fixture_commit(&beta.workspace);
            if baseline != "unborn" {
                fs::write(alpha.workspace.join("baseline.txt"), "clean").unwrap();
                fixture_commit(&alpha.workspace);
                match baseline {
                    "staged" => {
                        fs::write(alpha.workspace.join("baseline.txt"), "staged change").unwrap();
                        fixture_git_command(&alpha.workspace, &["add", "baseline.txt"]);
                    }
                    "untracked" => {
                        fs::write(alpha.workspace.join("preexisting.txt"), "other work").unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            let executable = fake_native_cli(&fixture);
            let cli = preflight_selected_cli(executable, None, control)
                .await
                .expect("fixture native preflight");
            let git = resolve_trusted_git(&alpha.workspace, control).unwrap();
            let before = capture_git_review_witness(&alpha.workspace, &git)
                .await
                .unwrap();
            match baseline {
                "unborn" => assert!(before.head_commit.is_none()),
                "staged" => assert!(
                    before
                        .changed_paths
                        .iter()
                        .any(|path| path.path == "baseline.txt")
                ),
                "untracked" => assert!(
                    before
                        .changed_paths
                        .iter()
                        .any(|path| path.path == "preexisting.txt")
                ),
                _ => unreachable!(),
            }
            for project in [alpha, beta] {
                queue
                    .save_job(
                        &QueueJob::new(
                            format!("job_{}", project.project_id),
                            project,
                            format!("edit only {}", project.project_id),
                            None,
                            None,
                        )
                        .unwrap(),
                    )
                    .unwrap();
            }

            let refused = claim_one(control, &queue).unwrap().unwrap();
            assert_eq!(refused.binding.project_id, "alpha");
            let turn = run_queued_turn(&refused, control, &cli, Duration::from_secs(30)).await;
            assert_eq!(turn.outcome, "REVIEW_BASELINE_DIRTY_BEFORE_SPAWN");
            assert!(turn.child_absent_proven);
            let result = complete_attempt(control, &queue, &refused, turn).unwrap();
            assert_eq!(result["job"]["state"], "ATTENTION");
            assert_eq!(result["leaseReleased"], true);
            assert_eq!(
                result["receipt"]["evidenceLevel"],
                "CHILD_ABSENT_BEFORE_SPAWN"
            );
            assert!(
                !alpha
                    .workspace
                    .join(format!("queue-proof-{}.txt", refused.binding.thread_id))
                    .exists()
            );
            assert_eq!(
                queue
                    .read_receipt(&refused.attempt.as_ref().unwrap().attempt_id)
                    .unwrap()
                    .outcome,
                "REVIEW_BASELINE_DIRTY_BEFORE_SPAWN"
            );

            let next = claim_one(control, &queue).unwrap().unwrap();
            assert_eq!(next.binding.project_id, "beta");
            let turn = run_queued_turn(&next, control, &cli, Duration::from_secs(30)).await;
            assert!(turn.child_exit_observed, "{turn:?}");
            let result = complete_attempt(control, &queue, &next, turn).unwrap();
            assert_eq!(result["job"]["state"], "NATIVE_EXITED_ZERO");
            assert_eq!(result["leaseReleased"], true);
            assert_eq!(
                fs::read_to_string(
                    beta.workspace
                        .join(format!("queue-proof-{}.txt", next.binding.thread_id))
                )
                .unwrap(),
                "edit only beta"
            );
            cleanup(fixture);
        }
    }
}
