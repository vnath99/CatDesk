//! Native CatDesk daemon replacement handoff.
//!
//! This module intentionally does not invoke PowerShell, cmd.exe, a shell, or
//! the externally managed Secure MCP tunnel. A running CatDesk validates a
//! workspace-contained replacement, persists a short-lived confirmation, and
//! launches its own executable in a tiny helper mode. The helper waits for the
//! old daemon to relinquish the loopback MCP port, starts the replacement, and
//! rolls back to the prior CatDesk executable if readiness fails.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
#[cfg(not(target_os = "windows"))]
use std::net::Ipv6Addr;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::reviewed_build::validate_producer_attestation;
use crate::reviewed_source_snapshot::{
    ReviewedSourceSnapshotExpectedV1, validate_committed_snapshot,
};

#[path = "release_pair_stopped.rs"]
pub(crate) mod stopped;

pub const RELOAD_WORKER_FLAG: &str = "--catdesk-reload-worker";
pub const DAEMON_MODE_FLAG: &str = "--catdesk-daemon";
pub const LIFECYCLE_STOP_WORKER_FLAG: &str = "--catdesk-lifecycle-stop-worker";
pub const CANONICAL_RECOVERY_WORKER_FLAG: &str = "--catdesk-canonical-recovery-worker";
pub const REVIEWED_PROMOTION_WORKER_FLAG: &str = "--catdesk-reviewed-promotion-worker";
pub const RELEASE_PAIR_RECONCILE_WORKER_FLAG: &str = "--catdesk-release-pair-reconcile-worker";
const PREFLIGHT_TTL_SECONDS: u64 = 120;
const GRACEFUL_EXIT_TIMEOUT: Duration = Duration::from_secs(4);
const FORCED_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "windows")]
const PORT_RELEASE_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(not(target_os = "windows"))]
const PORT_RELEASE_TIMEOUT: Duration = Duration::from_secs(5);
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// A protected, one-shot authority written only by the Rust control plane.
/// PowerShell can validate this record but cannot issue it through MCP.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedPromotionAuthorizationV1 {
    pub schema_version: u8,
    pub generation: u64,
    pub authorization_id: String,
    pub expires_at_unix: u64,
    pub candidate_relative_path: String,
    pub candidate_sha256: String,
    pub prior_canonical_sha256: String,
    pub promotion_script_sha256: String,
    pub trusted_powershell_path: String,
    pub trusted_powershell_sha256: String,
    pub review_session_id: String,
    pub review_record_id: String,
    pub review_record_sha256: String,
    pub transaction_id: String,
    pub claim_id: String,
    pub build_attestation_sha256: String,
    pub build_attempt_id: String,
    pub snapshot_id: String,
    pub snapshot_authority_digest: String,
    pub snapshot_manifest_digest: String,
    pub build_policy_sha256: String,
    pub cargo_sha256: String,
    pub rustc_sha256: String,
    pub snapshot_expected: ReviewedSourceSnapshotExpectedV1,
    #[serde(default, skip_serializing_if = "is_false")]
    pub local_backend_only: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedPromotionClaimV1 {
    schema_version: u8,
    authorization_id: String,
    claim_id: String,
    transaction_id: String,
    candidate_relative_path: String,
    state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedPromotionPreflightV1 {
    pub schema_version: u8,
    pub confirmation_token: String,
    pub expires_at_unix: u64,
    pub authorization: ReviewedPromotionAuthorizationV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedPromotionResultV1 {
    pub schema_version: u8,
    pub state: String,
    pub authorization_generation: u64,
    pub candidate_sha256: String,
    pub transaction_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serving_state: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedPromotionConfirmationOutcomeV1 {
    pub state: &'static str,
    pub authorization_generation: u64,
    pub candidate_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedPromotionWorkerArgsV1 {
    pub workspace: PathBuf,
    pub candidate_relative_path: String,
    pub authorization_id: String,
    pub transaction_id: String,
}

/// This one-shot authority is deliberately separate from ordinary promotion:
/// the old canonical binary and sidecar are measured independently and neither
/// is accepted as a reviewed rollback image.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleasePairReconcileAuthorizationV1 {
    pub schema_version: u8,
    pub authorization_id: String,
    pub transaction_id: String,
    pub expires_at_unix: u64,
    pub reviewed_preflight_sha256: String,
    pub reviewed_authorization_id: String,
    pub candidate_relative_path: String,
    pub candidate_sha256: String,
    pub original_binary_sha256: String,
    pub original_sidecar_file_sha256: String,
    pub original_sidecar_value: String,
    pub slot_a_binary_sha256: String,
    pub slot_a_sidecar_file_sha256: String,
    pub slot_a_manifest_sha256: String,
    pub original_pointer_sha256: String,
    pub script_sha256: String,
    pub recovery_helper_sha256: String,
    pub powershell_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleasePairReconcilePreflightV1 {
    pub schema_version: u8,
    pub confirmation_token: String,
    pub expires_at_unix: u64,
    pub authorization: ReleasePairReconcileAuthorizationV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleasePairReconcileClaimV1 {
    schema_version: u8,
    authorization_id: String,
    transaction_id: String,
    state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleasePairReconcileResultV1 {
    pub schema_version: u8,
    pub state: String,
    pub transaction_id: String,
    pub candidate_sha256: String,
    pub owner_pid: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleasePairReconcileWorkerArgsV1 {
    pub workspace: PathBuf,
    pub authorization_id: String,
    pub transaction_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleasePairReconcileConfirmationOutcomeV1 {
    pub state: &'static str,
    pub candidate_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReloadReviewBindingV1 {
    pub schema_version: u8,
    pub review_record_id: String,
    pub review_session_id: String,
    pub request_sha256: String,
    pub authority_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadPreflightV1 {
    pub schema_version: u8,
    pub confirmation_token: String,
    pub created_at_unix: u64,
    pub expires_at_unix: u64,
    pub old_pid: u32,
    pub port: u16,
    pub workspace: PathBuf,
    pub replacement_path: PathBuf,
    pub rollback_path: PathBuf,
    pub expected_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_binding: Option<ReloadReviewBindingV1>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReloadCandidateMeasurementV1 {
    pub candidate_relative_path: String,
    pub candidate_sha256: String,
    pub candidate_length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReloadWorkerArgsV1 {
    pub old_pid: u32,
    pub port: u16,
    pub workspace: PathBuf,
    pub replacement_path: PathBuf,
    pub rollback_path: PathBuf,
    pub expected_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleStopWorkerArgsV1 {
    pub workspace: PathBuf,
    pub attempt_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleStopAuthorizationV1 {
    schema_version: u8,
    attempt_id: String,
    target_pid: u32,
    target_executable_sha256: String,
    expires_at_unix: u64,
}

/// A fixed-purpose, detached recovery worker carries no caller controlled
/// executable, script, command, path, tunnel, or credential input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRecoveryWorkerArgsV1 {
    pub workspace: PathBuf,
    pub attempt_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalRecoveryStateV1 {
    pub schema_version: u8,
    pub status: String,
    pub updated_at_unix: u64,
    #[serde(default)]
    pub generation: u64,
    #[serde(default)]
    pub attempt_id: String,
}

const RECOVERY_STATUS_SCHEDULED: &str = "RECOVERY_SCHEDULED";
const RECOVERY_STATUS_COMPLETED: &str = "RECOVERY_COMPLETED";
const RECOVERY_STATUS_AUTHORITY_REQUIRED: &str = "RECOVERY_AUTHORITY_REQUIRED";
const RECOVERY_STATUS_LKG_MISSING: &str = "LKG_AUTHORITY_MISSING";
const RECOVERY_STATUS_LKG_DAMAGED: &str = "LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED";
const RECOVERY_STATUS_TRANSPORT_FAILED: &str = "TRANSPORT_VERIFICATION_FAILED";

pub fn parse_lifecycle_stop_worker_args(
    args: &[String],
) -> Result<Option<LifecycleStopWorkerArgsV1>, String> {
    if !args.iter().any(|arg| arg == LIFECYCLE_STOP_WORKER_FLAG) {
        return Ok(None);
    }
    if args.len() != 5
        || args[0] != LIFECYCLE_STOP_WORKER_FLAG
        || args[1] != "--workspace"
        || args[3] != "--attempt"
    {
        return Err("CatDesk lifecycle stop helper arguments are invalid".into());
    }
    let workspace = args[2].trim();
    if workspace.is_empty() || workspace.starts_with('-') || workspace.len() > 4096 {
        return Err("CatDesk lifecycle stop helper workspace is invalid".into());
    }
    let attempt_id = args[4].trim();
    if Uuid::parse_str(attempt_id).is_err() {
        return Err("CatDesk lifecycle stop helper attempt is invalid".into());
    }
    Ok(Some(LifecycleStopWorkerArgsV1 {
        workspace: PathBuf::from(workspace),
        attempt_id: attempt_id.into(),
    }))
}

pub fn parse_canonical_recovery_worker_args(
    args: &[String],
) -> Result<Option<CanonicalRecoveryWorkerArgsV1>, String> {
    if !args.iter().any(|arg| arg == CANONICAL_RECOVERY_WORKER_FLAG) {
        return Ok(None);
    }
    if args.len() != 5
        || args[0] != CANONICAL_RECOVERY_WORKER_FLAG
        || args[1] != "--workspace"
        || args[3] != "--attempt"
    {
        return Err("CatDesk canonical recovery helper arguments are invalid".into());
    }
    let workspace = args[2].trim();
    if workspace.is_empty() || workspace.starts_with('-') || workspace.len() > 4096 {
        return Err("CatDesk canonical recovery helper workspace is invalid".into());
    }
    let attempt_id = args[4].trim();
    if Uuid::parse_str(attempt_id).is_err() {
        return Err("CatDesk canonical recovery helper attempt is invalid".into());
    }
    Ok(Some(CanonicalRecoveryWorkerArgsV1 {
        workspace: PathBuf::from(workspace),
        attempt_id: attempt_id.into(),
    }))
}

pub fn parse_reviewed_promotion_worker_args(
    args: &[String],
) -> Result<Option<ReviewedPromotionWorkerArgsV1>, String> {
    if !args.iter().any(|arg| arg == REVIEWED_PROMOTION_WORKER_FLAG) {
        return Ok(None);
    }
    if args.len() != 9
        || args[0] != REVIEWED_PROMOTION_WORKER_FLAG
        || args[1] != "--workspace"
        || args[3] != "--candidate"
        || args[5] != "--authorization"
        || args[7] != "--transaction"
    {
        return Err("CatDesk reviewed promotion helper arguments are invalid".into());
    }
    let workspace = args[2].trim();
    let candidate = args[4].trim();
    let authorization = args[6].trim();
    let transaction = args[8].trim();
    if workspace.is_empty()
        || workspace.starts_with('-')
        || candidate.is_empty()
        || candidate.len() > 4096
        || authorization.len() != 32
        || !authorization.bytes().all(|byte| byte.is_ascii_hexdigit())
        || transaction.len() != 32
        || !transaction.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("CatDesk reviewed promotion helper arguments are invalid".into());
    }
    Ok(Some(ReviewedPromotionWorkerArgsV1 {
        workspace: PathBuf::from(workspace),
        candidate_relative_path: candidate.into(),
        authorization_id: authorization.into(),
        transaction_id: transaction.into(),
    }))
}

pub fn parse_release_pair_reconcile_worker_args(
    args: &[String],
) -> Result<Option<ReleasePairReconcileWorkerArgsV1>, String> {
    if !args
        .iter()
        .any(|arg| arg == RELEASE_PAIR_RECONCILE_WORKER_FLAG)
    {
        return Ok(None);
    }
    if args.len() != 7
        || args[0] != RELEASE_PAIR_RECONCILE_WORKER_FLAG
        || args[1] != "--workspace"
        || args[3] != "--authorization"
        || args[5] != "--transaction"
        || args[2].is_empty()
        || args[2].len() > 4096
        || ![&args[4], &args[6]]
            .iter()
            .all(|value| value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err("CatDesk release-pair reconciliation worker arguments are invalid".into());
    }
    Ok(Some(ReleasePairReconcileWorkerArgsV1 {
        workspace: PathBuf::from(&args[2]),
        authorization_id: args[4].clone(),
        transaction_id: args[6].clone(),
    }))
}

pub fn prepare_reviewed_promotion(
    workspace: &Path,
    candidate: &Path,
    expected_sha256: &str,
    review_session_id: &str,
    review_record_id: &str,
    review_record_sha256: &str,
    snapshot_expected: &ReviewedSourceSnapshotExpectedV1,
) -> Result<ReviewedPromotionPreflightV1, String> {
    prepare_reviewed_promotion_with_profile(
        workspace,
        candidate,
        expected_sha256,
        review_session_id,
        review_record_id,
        review_record_sha256,
        snapshot_expected,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_reviewed_promotion_with_profile(
    workspace: &Path,
    candidate: &Path,
    expected_sha256: &str,
    review_session_id: &str,
    review_record_id: &str,
    review_record_sha256: &str,
    snapshot_expected: &ReviewedSourceSnapshotExpectedV1,
    local_backend_only: bool,
) -> Result<ReviewedPromotionPreflightV1, String> {
    let workspace = canonical_directory(workspace, "reviewed promotion workspace")?;
    let candidate =
        canonical_workspace_file(&workspace, candidate, "reviewed promotion candidate")?;
    let candidate_sha256 = sha256_file(&candidate)?;
    if candidate_sha256 != normalize_sha256(expected_sha256)? {
        return Err("reviewed promotion candidate hash did not match".into());
    }
    let canonical = canonical_workspace_file(
        &workspace,
        &workspace.join("target/release/catdesk.exe"),
        "canonical release",
    )?;
    let prior = sha256_file(&canonical)?;
    let script = canonical_workspace_script(
        &workspace,
        &workspace.join("scripts/promote-reviewed-catdesk-build.ps1"),
        "reviewed promotion script",
    )?;
    let script_sha = sha256_file(&script)?;
    let powershell = crate::operator_facade::trusted_windows_powershell(&workspace)
        .map_err(|_| "trusted promotion PowerShell is unavailable".to_string())?;
    let powershell_sha = sha256_file(&powershell)?;
    if review_session_id.is_empty()
        || review_session_id.len() > 128
        || review_record_id.is_empty()
        || review_record_id.len() > 128
        || review_record_sha256.len() != 64
        || !review_record_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("independent review authority is unavailable".into());
    }
    let candidate_relative_path = candidate
        .strip_prefix(&workspace)
        .map_err(|_| "reviewed promotion candidate is unsafe")?
        .to_string_lossy()
        .replace('\\', "/");
    if snapshot_expected.session_id != review_session_id {
        return Err("REVIEWED_SOURCE_SNAPSHOT_REQUIRED".into());
    }
    // The shared validator recomputes the completion/output/baseline binding
    // and every byte-object digest.  Legacy sibling `<session>.json` state is
    // intentionally never read.
    let snapshot = validate_committed_snapshot(&workspace, snapshot_expected)
        .map_err(|_| "REVIEWED_SOURCE_SNAPSHOT_REQUIRED".to_string())?;
    let attestation = validate_producer_attestation(
        &workspace,
        review_session_id,
        review_record_id,
        review_record_sha256,
        snapshot_expected,
        &candidate_relative_path,
        &candidate_sha256,
    )
    .map_err(|_| "reviewed build attestation is unavailable".to_string())?;
    if attestation.snapshot_id != snapshot.snapshot_id
        || attestation.snapshot_authority_digest != snapshot.authority_digest
        || attestation.snapshot_manifest_digest != snapshot.manifest_digest
    {
        return Err("reviewed build attestation is unavailable".into());
    }
    let authorization = ReviewedPromotionAuthorizationV1 {
        schema_version: 1,
        generation: now_unix().max(1),
        authorization_id: Uuid::new_v4().simple().to_string(),
        expires_at_unix: now_unix().saturating_add(PREFLIGHT_TTL_SECONDS),
        candidate_relative_path,
        candidate_sha256,
        prior_canonical_sha256: prior,
        promotion_script_sha256: script_sha,
        trusted_powershell_path: powershell.to_string_lossy().into_owned(),
        trusted_powershell_sha256: powershell_sha,
        review_session_id: review_session_id.into(),
        review_record_id: review_record_id.into(),
        review_record_sha256: review_record_sha256.to_ascii_lowercase(),
        transaction_id: Uuid::new_v4().simple().to_string(),
        claim_id: Uuid::new_v4().simple().to_string(),
        build_attestation_sha256: attestation.attestation_digest.clone(),
        build_attempt_id: attestation.build_attempt_id.clone(),
        snapshot_id: attestation.snapshot_id.clone(),
        snapshot_authority_digest: attestation.snapshot_authority_digest.clone(),
        snapshot_manifest_digest: attestation.snapshot_manifest_digest.clone(),
        build_policy_sha256: attestation.build_policy_sha256.clone(),
        cargo_sha256: attestation.cargo_sha256.clone(),
        rustc_sha256: attestation.rustc_sha256.clone(),
        snapshot_expected: snapshot_expected.clone(),
        local_backend_only,
    };
    let preflight = ReviewedPromotionPreflightV1 {
        schema_version: 1,
        confirmation_token: Uuid::new_v4().simple().to_string(),
        expires_at_unix: authorization.expires_at_unix,
        authorization,
    };
    if let Ok(existing) = read_bounded_json::<ReviewedPromotionPreflightV1>(
        &reviewed_promotion_preflight_path(&workspace),
        "reviewed promotion preflight",
    ) {
        if existing.schema_version == 1
            && existing.expires_at_unix >= now_unix()
            && same_reviewed_promotion_evidence(&existing.authorization, &preflight.authorization)
        {
            return Ok(existing);
        }
        if existing.authorization.review_record_id == preflight.authorization.review_record_id {
            return Err("reviewed promotion binding is already immutable".into());
        }
    }
    atomic_json_write(&reviewed_promotion_preflight_path(&workspace), &preflight)?;
    Ok(preflight)
}

pub fn confirm_reviewed_promotion(
    workspace: &Path,
    candidate: &Path,
    expected_sha256: &str,
    confirmation_token: &str,
) -> Result<ReviewedPromotionConfirmationOutcomeV1, String> {
    let workspace = canonical_directory(workspace, "reviewed promotion workspace")?;
    let preflight: ReviewedPromotionPreflightV1 = read_bounded_json(
        &reviewed_promotion_preflight_path(&workspace),
        "reviewed promotion preflight",
    )?;
    if preflight.schema_version != 1
        || preflight.expires_at_unix < now_unix()
        || preflight.confirmation_token != confirmation_token
    {
        return Err("reviewed promotion confirmation is unavailable".into());
    }
    let candidate =
        canonical_workspace_file(&workspace, candidate, "reviewed promotion candidate")?;
    let relative = candidate
        .strip_prefix(&workspace)
        .map_err(|_| "reviewed promotion candidate is unsafe")?
        .to_string_lossy()
        .replace('\\', "/");
    if relative != preflight.authorization.candidate_relative_path
        || sha256_file(&candidate)? != normalize_sha256(expected_sha256)?
        || sha256_file(&candidate)? != preflight.authorization.candidate_sha256
    {
        return Err("reviewed promotion evidence drifted".into());
    }
    let canonical = canonical_workspace_file(
        &workspace,
        &workspace.join("target/release/catdesk.exe"),
        "canonical release",
    )?;
    let script = canonical_workspace_script(
        &workspace,
        &workspace.join("scripts/promote-reviewed-catdesk-build.ps1"),
        "reviewed promotion script",
    )?;
    let _powershell = crate::operator_facade::trusted_windows_powershell(&workspace)
        .map_err(|_| "trusted promotion PowerShell is unavailable".to_string())?;
    let attestation = validate_producer_attestation(
        &workspace,
        &preflight.authorization.review_session_id,
        &preflight.authorization.review_record_id,
        &preflight.authorization.review_record_sha256,
        &preflight.authorization.snapshot_expected,
        &relative,
        &preflight.authorization.candidate_sha256,
    )
    .map_err(|_| "reviewed promotion evidence drifted".to_string())?;
    if attestation.attestation_digest != preflight.authorization.build_attestation_sha256
        || attestation.build_attempt_id != preflight.authorization.build_attempt_id
        || attestation.snapshot_id != preflight.authorization.snapshot_id
        || attestation.snapshot_authority_digest
            != preflight.authorization.snapshot_authority_digest
        || attestation.snapshot_manifest_digest != preflight.authorization.snapshot_manifest_digest
        || attestation.build_policy_sha256 != preflight.authorization.build_policy_sha256
        || attestation.cargo_sha256 != preflight.authorization.cargo_sha256
        || attestation.rustc_sha256 != preflight.authorization.rustc_sha256
    {
        return Err("reviewed promotion evidence drifted".into());
    }
    if sha256_file(&canonical)? != preflight.authorization.prior_canonical_sha256
        || sha256_file(&script)? != preflight.authorization.promotion_script_sha256
    {
        return Err("reviewed promotion evidence drifted".into());
    }
    atomic_json_write(
        &reviewed_promotion_authorization_path(&workspace),
        &preflight.authorization,
    )?;
    let claim = ReviewedPromotionClaimV1 {
        schema_version: 1,
        authorization_id: preflight.authorization.authorization_id.clone(),
        claim_id: preflight.authorization.claim_id.clone(),
        transaction_id: preflight.authorization.transaction_id.clone(),
        candidate_relative_path: relative.clone(),
        state: "CLAIMED_PENDING".into(),
    };
    let claim_path = reviewed_promotion_claim_path(&workspace);
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&claim_path)
    {
        Ok(mut file) => {
            let bytes = serde_json::to_vec(&claim)
                .map_err(|_| "reviewed promotion claim is unavailable".to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "reviewed promotion claim is unavailable".to_string())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing: ReviewedPromotionClaimV1 =
                read_bounded_json(&claim_path, "reviewed promotion claim")?;
            if existing != claim {
                return Err("reviewed promotion claim is unavailable".into());
            }
            return reviewed_promotion_claim_outcome(&workspace, &preflight.authorization);
        }
        Err(_) => return Err("reviewed promotion claim is unavailable".into()),
    }
    let current = std::env::current_exe()
        .map_err(|_| "reviewed promotion helper is unavailable".to_string())?;
    let mut command = Command::new(current);
    command
        .arg(REVIEWED_PROMOTION_WORKER_FLAG)
        .arg("--workspace")
        .arg(&workspace)
        .arg("--candidate")
        .arg(&relative)
        .arg("--authorization")
        .arg(&preflight.authorization.authorization_id)
        .arg("--transaction")
        .arg(&preflight.authorization.transaction_id)
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_detached_process_flags(&mut command);
    command
        .spawn()
        .map_err(|_| "reviewed promotion helper could not be scheduled".to_string())?;
    Ok(ReviewedPromotionConfirmationOutcomeV1 {
        state: "NEWLY_SCHEDULED",
        authorization_generation: preflight.authorization.generation,
        candidate_sha256: preflight.authorization.candidate_sha256.clone(),
    })
}

pub fn reviewed_promotion_preflight_authority(
    workspace: &Path,
) -> Result<ReviewedPromotionAuthorizationV1, String> {
    let workspace = canonical_directory(workspace, "reviewed promotion workspace")?;
    let preflight: ReviewedPromotionPreflightV1 = read_bounded_json(
        &reviewed_promotion_preflight_path(&workspace),
        "reviewed promotion preflight",
    )?;
    if preflight.schema_version != 1 || preflight.expires_at_unix < now_unix() {
        return Err("reviewed promotion preflight is unavailable".into());
    }
    Ok(preflight.authorization)
}

pub fn run_reviewed_promotion_worker(args: ReviewedPromotionWorkerArgsV1) -> Result<(), String> {
    let workspace = canonical_directory(&args.workspace, "reviewed promotion workspace")?;
    let authorization: ReviewedPromotionAuthorizationV1 = read_bounded_json(
        &reviewed_promotion_authorization_path(&workspace),
        "reviewed promotion authorization",
    )?;
    if authorization.schema_version != 1
        || authorization.authorization_id != args.authorization_id
        || authorization.expires_at_unix < now_unix()
        || authorization.candidate_relative_path != args.candidate_relative_path
        || authorization.transaction_id != args.transaction_id
    {
        return Err("reviewed promotion authorization is unavailable".into());
    }
    let claim: ReviewedPromotionClaimV1 = read_bounded_json(
        &reviewed_promotion_claim_path(&workspace),
        "reviewed promotion claim",
    )?;
    if claim.schema_version != 1
        || claim.state != "CLAIMED_PENDING"
        || claim.authorization_id != authorization.authorization_id
        || claim.claim_id != authorization.claim_id
        || claim.transaction_id != authorization.transaction_id
        || claim.candidate_relative_path != authorization.candidate_relative_path
    {
        return Err("reviewed promotion claim is unavailable".into());
    }
    let attestation = validate_producer_attestation(
        &workspace,
        &authorization.review_session_id,
        &authorization.review_record_id,
        &authorization.review_record_sha256,
        &authorization.snapshot_expected,
        &authorization.candidate_relative_path,
        &authorization.candidate_sha256,
    )
    .map_err(|_| "reviewed promotion authorization is unavailable".to_string())?;
    if attestation.attestation_digest != authorization.build_attestation_sha256
        || attestation.build_attempt_id != authorization.build_attempt_id
        || attestation.snapshot_id != authorization.snapshot_id
        || attestation.snapshot_authority_digest != authorization.snapshot_authority_digest
        || attestation.snapshot_manifest_digest != authorization.snapshot_manifest_digest
        || attestation.build_policy_sha256 != authorization.build_policy_sha256
        || attestation.cargo_sha256 != authorization.cargo_sha256
        || attestation.rustc_sha256 != authorization.rustc_sha256
    {
        return Err("reviewed promotion authorization is unavailable".into());
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(reviewed_promotion_worker_owner_path(&workspace))
    {
        Ok(mut owner) => owner
            .write_all(authorization.transaction_id.as_bytes())
            .and_then(|_| owner.sync_all())
            .map_err(|_| "reviewed promotion worker ownership is unavailable".to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err("reviewed promotion worker ownership is unavailable".into());
        }
        Err(_) => return Err("reviewed promotion worker ownership is unavailable".into()),
    }
    thread::sleep(Duration::from_millis(1500));
    let invocation = if authorization.local_backend_only {
        crate::operator_facade::reviewed_promotion_invocation_with_profile(
            &workspace,
            &workspace.join(&args.candidate_relative_path),
            &authorization.authorization_id,
            &authorization.transaction_id,
            true,
        )?
    } else {
        crate::operator_facade::reviewed_promotion_invocation(
            &workspace,
            &workspace.join(&args.candidate_relative_path),
            &authorization.authorization_id,
            &authorization.transaction_id,
        )?
    };
    let result = crate::operator_facade::run_fixed_powershell_invocation_blocking(invocation)?;
    let state = if result.success
        && result.stderr.trim().is_empty()
        && result.stdout.len() <= 4096
        && serde_json::from_str::<serde_json::Value>(&result.stdout)
            .ok()
            .and_then(|value| {
                value
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            == Some("PROMOTED_CANONICAL_READY")
    {
        "PROMOTION_COMPLETED"
    } else {
        "PROMOTION_FAILED_OR_AMBIGUOUS"
    };
    // Disk promotion remains completed even if a separate owner appears on
    // the port after the bounded helper Job closes. Never roll back through
    // an unknown live owner, and never equate the transient handoff with a
    // persistent service.
    let serving_state = if authorization.local_backend_only && state == "PROMOTION_COMPLETED" {
        Some(
            if bounded_promotion_owner_stopped() {
                "STOPPED_AFTER_BOUNDED_OWNER"
            } else {
                "UNVERIFIED"
            }
            .to_owned(),
        )
    } else {
        None
    };
    atomic_json_write(
        &reviewed_promotion_result_path(&workspace),
        &ReviewedPromotionResultV1 {
            schema_version: 1,
            state: state.into(),
            authorization_generation: authorization.generation,
            candidate_sha256: authorization.candidate_sha256,
            transaction_id: authorization.transaction_id,
            serving_state,
        },
    )?;
    if state == "PROMOTION_COMPLETED" {
        Ok(())
    } else {
        Err("reviewed promotion result was ambiguous".into())
    }
}

pub fn reviewed_promotion_result(
    workspace: &Path,
) -> Result<Option<ReviewedPromotionResultV1>, String> {
    let workspace = canonical_directory(workspace, "reviewed promotion workspace")?;
    let path = reviewed_promotion_result_path(&workspace);
    if !path.exists() {
        return Ok(None);
    }
    let result: ReviewedPromotionResultV1 = read_bounded_json(&path, "reviewed promotion result")?;
    if result.schema_version != 1
        || !matches!(
            result.state.as_str(),
            "PROMOTION_COMPLETED" | "PROMOTION_FAILED_OR_AMBIGUOUS"
        )
        || result.candidate_sha256.len() != 64
        || result.transaction_id.len() != 32
        || !result
            .transaction_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || result
            .serving_state
            .as_deref()
            .is_some_and(|state| !matches!(state, "STOPPED_AFTER_BOUNDED_OWNER" | "UNVERIFIED"))
        || (result.serving_state.is_some() && result.state != "PROMOTION_COMPLETED")
    {
        return Err("reviewed promotion result is malformed".into());
    }
    Ok(Some(result))
}

#[cfg(windows)]
fn bounded_promotion_owner_stopped() -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match windows_listener_owner_pids(3200) {
            Ok(owners) if owners.is_empty() => return true,
            Err(_) => return false,
            _ => {}
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(windows))]
fn bounded_promotion_owner_stopped() -> bool {
    false
}

fn release_pair_control_dir(workspace: &Path) -> PathBuf {
    workspace.join(".catdesk").join("release-pair-control")
}

fn release_pair_preflight_path(workspace: &Path) -> PathBuf {
    release_pair_control_dir(workspace).join("preflight.json")
}

fn release_pair_authorization_path(workspace: &Path) -> PathBuf {
    release_pair_control_dir(workspace).join("authorization.json")
}

fn release_pair_claim_path(workspace: &Path) -> PathBuf {
    release_pair_control_dir(workspace).join("claim.json")
}

fn release_pair_worker_owner_path(workspace: &Path) -> PathBuf {
    release_pair_control_dir(workspace).join("worker-owner")
}

fn release_pair_result_path(workspace: &Path) -> PathBuf {
    release_pair_control_dir(workspace).join("result.json")
}

fn release_pair_pending_path(workspace: &Path) -> PathBuf {
    release_pair_control_dir(workspace).join("serving-pending.json")
}

fn release_pair_released_path(workspace: &Path) -> PathBuf {
    release_pair_control_dir(workspace).join("job-release.json")
}

fn release_pair_quiescence_path(workspace: &Path) -> PathBuf {
    release_pair_control_dir(workspace).join("quiescence.json")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleasePairScriptResultV1 {
    state: String,
    transaction_id: String,
    #[serde(default)]
    owner_pid: Option<u32>,
    #[serde(default)]
    owner_started_at_utc: Option<String>,
    #[serde(default)]
    candidate_sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleasePairOwnerMarkerV1<'a> {
    schema_version: u8,
    state: &'a str,
    authorization_id: &'a str,
    transaction_id: &'a str,
    owner_pid: u32,
    owner_started_at_utc: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleasePairQuiescenceV1<'a> {
    schema_version: u8,
    state: &'a str,
    authorization_id: &'a str,
    transaction_id: &'a str,
}

fn bounded_release_file(workspace: &Path, relative: &str, max: u64) -> Result<Vec<u8>, String> {
    let path = workspace.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "release-pair evidence is unavailable".to_string())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > max
    {
        return Err("release-pair evidence is unsafe".into());
    }
    let canonical = canonical_workspace_regular_file(workspace, &path, "release-pair evidence")?;
    let bytes =
        fs::read(canonical).map_err(|_| "release-pair evidence is unreadable".to_string())?;
    if bytes.is_empty() || bytes.len() as u64 > max || bytes.len() as u64 != metadata.len() {
        return Err("release-pair evidence changed".into());
    }
    Ok(bytes)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn release_sidecar_value(bytes: &[u8]) -> Result<String, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| "release-pair sidecar is malformed".to_string())?;
    let value = text.trim();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("release-pair sidecar is malformed".into());
    }
    Ok(value.to_ascii_lowercase())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReleasePairObservedV1 {
    binary_sha256: String,
    sidecar_file_sha256: String,
    sidecar_value: String,
    slot_a_binary_sha256: String,
    slot_a_sidecar_file_sha256: String,
    slot_a_manifest_sha256: String,
    pointer_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperationalSlotManifestV1 {
    schema_version: u8,
    generation: u64,
    sha256: String,
    source: String,
    captured_at_utc: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperationalSlotPointerV1 {
    schema_version: u8,
    generation: u64,
    sha256: String,
    slot: String,
}

fn measure_divergent_release_pair(workspace: &Path) -> Result<ReleasePairObservedV1, String> {
    let canonical = canonical_workspace_file(
        workspace,
        &workspace.join("target/release/catdesk.exe"),
        "canonical release",
    )?;
    let binary_sha256 = sha256_file(&canonical)?;
    let sidecar = bounded_release_file(workspace, "target/release/catdesk.exe.sha256", 128)?;
    let sidecar_value = release_sidecar_value(&sidecar)?;
    if sidecar_value == binary_sha256 {
        return Err("release-pair reconciliation requires an actual divergent pair".into());
    }
    let slot_a_binary = canonical_workspace_file(
        workspace,
        &workspace.join(".catdesk/release-recovery/slot-a/catdesk.exe"),
        "existing operational slot-a",
    )?;
    let slot_a_binary_sha256 = sha256_file(&slot_a_binary)?;
    let slot_a_sidecar = bounded_release_file(
        workspace,
        ".catdesk/release-recovery/slot-a/catdesk.exe.sha256",
        128,
    )?;
    let slot_a_manifest = bounded_release_file(
        workspace,
        ".catdesk/release-recovery/slot-a/manifest.json",
        4096,
    )?;
    let pointer = bounded_release_file(workspace, ".catdesk/release-recovery/current.json", 1024)?;
    let manifest: OperationalSlotManifestV1 = serde_json::from_slice(&slot_a_manifest)
        .map_err(|_| "operational slot-a evidence is malformed".to_string())?;
    let current: OperationalSlotPointerV1 = serde_json::from_slice(&pointer)
        .map_err(|_| "operational slot-a pointer is malformed".to_string())?;
    if slot_a_binary_sha256 != sidecar_value
        || release_sidecar_value(&slot_a_sidecar)? != sidecar_value
        || manifest.schema_version != 1
        || manifest.generation != 1
        || manifest.source != "operational_verified"
        || manifest.sha256 != sidecar_value
        || manifest.captured_at_utc.is_empty()
        || current.schema_version != 1
        || current.generation != 1
        || current.slot != "slot-a"
        || current.sha256 != sidecar_value
    {
        return Err("operational slot-a evidence drifted".into());
    }
    if workspace.join(".catdesk/release-recovery/slot-b").exists()
        || workspace
            .join(".catdesk/promotion-recovery/promotion-transaction.json")
            .exists()
        || workspace
            .join(".catdesk/release-pair-control/transaction.json")
            .exists()
    {
        return Err("release-pair reconciliation requires an empty transaction boundary".into());
    }
    Ok(ReleasePairObservedV1 {
        binary_sha256,
        sidecar_file_sha256: sha256_bytes(&sidecar),
        sidecar_value,
        slot_a_binary_sha256,
        slot_a_sidecar_file_sha256: sha256_bytes(&slot_a_sidecar),
        slot_a_manifest_sha256: sha256_bytes(&slot_a_manifest),
        pointer_sha256: sha256_bytes(&pointer),
    })
}

fn validate_release_pair_review(
    workspace: &Path,
    authorization: &ReleasePairReconcileAuthorizationV1,
) -> Result<ReviewedPromotionAuthorizationV1, String> {
    validate_release_pair_review_freshness(workspace, authorization, true)
}

fn validate_release_pair_review_freshness(
    workspace: &Path,
    authorization: &ReleasePairReconcileAuthorizationV1,
    require_fresh: bool,
) -> Result<ReviewedPromotionAuthorizationV1, String> {
    let reviewed: ReviewedPromotionPreflightV1 = read_bounded_json(
        &reviewed_promotion_preflight_path(workspace),
        "independent reviewed promotion preflight",
    )?;
    if reviewed.schema_version != 1
        || (require_fresh && reviewed.expires_at_unix < now_unix())
        || sha256_file(&reviewed_promotion_preflight_path(workspace))?
            != authorization.reviewed_preflight_sha256
        || reviewed.authorization.authorization_id != authorization.reviewed_authorization_id
        || reviewed.authorization.candidate_relative_path != authorization.candidate_relative_path
        || reviewed.authorization.candidate_sha256 != authorization.candidate_sha256
        || reviewed.authorization.prior_canonical_sha256 != authorization.original_binary_sha256
    {
        return Err("release-pair independent review authority drifted".into());
    }
    crate::delegated::autonomy_supervisor::revalidate_release_pair_review_authority(
        workspace,
        &reviewed.authorization,
    )?;
    let attestation = validate_producer_attestation(
        workspace,
        &reviewed.authorization.review_session_id,
        &reviewed.authorization.review_record_id,
        &reviewed.authorization.review_record_sha256,
        &reviewed.authorization.snapshot_expected,
        &authorization.candidate_relative_path,
        &authorization.candidate_sha256,
    )
    .map_err(|_| "release-pair reviewed build attestation drifted".to_string())?;
    if attestation.attestation_digest != reviewed.authorization.build_attestation_sha256
        || attestation.build_attempt_id != reviewed.authorization.build_attempt_id
        || attestation.snapshot_id != reviewed.authorization.snapshot_id
        || attestation.snapshot_authority_digest != reviewed.authorization.snapshot_authority_digest
        || attestation.snapshot_manifest_digest != reviewed.authorization.snapshot_manifest_digest
        || attestation.build_policy_sha256 != reviewed.authorization.build_policy_sha256
        || attestation.cargo_sha256 != reviewed.authorization.cargo_sha256
        || attestation.rustc_sha256 != reviewed.authorization.rustc_sha256
    {
        return Err("release-pair reviewed build attestation drifted".into());
    }
    let candidate = canonical_workspace_file(
        workspace,
        &workspace.join(&authorization.candidate_relative_path),
        "release-pair candidate",
    )?;
    if sha256_file(&candidate)? != authorization.candidate_sha256 {
        return Err("release-pair candidate drifted".into());
    }
    Ok(reviewed.authorization)
}

fn build_release_pair_reconcile_preflight(
    workspace: &Path,
) -> Result<ReleasePairReconcilePreflightV1, String> {
    let workspace = canonical_directory(workspace, "release-pair workspace")?;
    let reviewed = reviewed_promotion_preflight_authority(&workspace)?;
    crate::delegated::autonomy_supervisor::revalidate_release_pair_review_authority(
        &workspace, &reviewed,
    )?;
    let observed = measure_divergent_release_pair(&workspace)?;
    if observed.binary_sha256 != reviewed.prior_canonical_sha256 {
        return Err("reviewed candidate prior binary binding drifted".into());
    }
    let script = canonical_workspace_script(
        &workspace,
        &workspace.join("scripts/reconcile-reviewed-catdesk-release-pair.ps1"),
        "fixed release-pair reconciliation script",
    )?;
    let recovery_helper = canonical_workspace_script(
        &workspace,
        &workspace.join("scripts/catdesk-release-recovery.ps1"),
        "fixed release-pair recovery helper",
    )?;
    let powershell = crate::operator_facade::trusted_windows_powershell(&workspace)?;
    let authorization = ReleasePairReconcileAuthorizationV1 {
        schema_version: 1,
        authorization_id: Uuid::new_v4().simple().to_string(),
        transaction_id: Uuid::new_v4().simple().to_string(),
        expires_at_unix: reviewed
            .expires_at_unix
            .min(now_unix().saturating_add(PREFLIGHT_TTL_SECONDS)),
        reviewed_preflight_sha256: sha256_file(&reviewed_promotion_preflight_path(&workspace))?,
        reviewed_authorization_id: reviewed.authorization_id,
        candidate_relative_path: reviewed.candidate_relative_path,
        candidate_sha256: reviewed.candidate_sha256,
        original_binary_sha256: observed.binary_sha256,
        original_sidecar_file_sha256: observed.sidecar_file_sha256,
        original_sidecar_value: observed.sidecar_value,
        slot_a_binary_sha256: observed.slot_a_binary_sha256,
        slot_a_sidecar_file_sha256: observed.slot_a_sidecar_file_sha256,
        slot_a_manifest_sha256: observed.slot_a_manifest_sha256,
        original_pointer_sha256: observed.pointer_sha256,
        script_sha256: sha256_file(&script)?,
        recovery_helper_sha256: sha256_file(&recovery_helper)?,
        powershell_sha256: sha256_file(&powershell)?,
    };
    validate_release_pair_review(&workspace, &authorization)?;
    let preflight = ReleasePairReconcilePreflightV1 {
        schema_version: 1,
        confirmation_token: Uuid::new_v4().simple().to_string(),
        expires_at_unix: authorization.expires_at_unix,
        authorization,
    };
    Ok(preflight)
}

pub fn prepare_release_pair_reconcile(
    workspace: &Path,
) -> Result<ReleasePairReconcilePreflightV1, String> {
    let workspace = canonical_directory(workspace, "release-pair workspace")?;
    let existing = if release_pair_preflight_path(&workspace).exists() {
        let existing: ReleasePairReconcilePreflightV1 = read_bounded_json(
            &release_pair_preflight_path(&workspace),
            "existing release-pair preflight",
        )
        .map_err(|_| "existing release-pair preflight belongs to a foreign or unknown mode")?;
        if existing.schema_version != 1 {
            return Err("existing release-pair preflight belongs to another schema".into());
        }
        Some(existing)
    } else {
        None
    };
    let preflight = build_release_pair_reconcile_preflight(&workspace)?;
    if release_pair_claim_path(&workspace).exists()
        || release_pair_authorization_path(&workspace).exists()
    {
        return Err("release-pair reconciliation is already claimed".into());
    }
    if let Some(existing) = existing {
        if existing.schema_version == 1
            && existing.expires_at_unix >= now_unix()
            && existing.authorization.reviewed_preflight_sha256
                == preflight.authorization.reviewed_preflight_sha256
            && existing.authorization.reviewed_authorization_id
                == preflight.authorization.reviewed_authorization_id
            && existing.authorization.candidate_relative_path
                == preflight.authorization.candidate_relative_path
            && existing.authorization.candidate_sha256 == preflight.authorization.candidate_sha256
            && existing.authorization.original_binary_sha256
                == preflight.authorization.original_binary_sha256
            && existing.authorization.original_sidecar_file_sha256
                == preflight.authorization.original_sidecar_file_sha256
            && existing.authorization.original_sidecar_value
                == preflight.authorization.original_sidecar_value
            && existing.authorization.slot_a_binary_sha256
                == preflight.authorization.slot_a_binary_sha256
            && existing.authorization.slot_a_sidecar_file_sha256
                == preflight.authorization.slot_a_sidecar_file_sha256
            && existing.authorization.slot_a_manifest_sha256
                == preflight.authorization.slot_a_manifest_sha256
            && existing.authorization.original_pointer_sha256
                == preflight.authorization.original_pointer_sha256
            && existing.authorization.script_sha256 == preflight.authorization.script_sha256
            && existing.authorization.recovery_helper_sha256
                == preflight.authorization.recovery_helper_sha256
            && existing.authorization.powershell_sha256 == preflight.authorization.powershell_sha256
        {
            return Ok(existing);
        }
    }
    atomic_json_write(&release_pair_preflight_path(&workspace), &preflight)?;
    Ok(preflight)
}

fn release_pair_observed_matches(
    observed: &ReleasePairObservedV1,
    authorized: &ReleasePairReconcileAuthorizationV1,
) -> bool {
    observed.binary_sha256 == authorized.original_binary_sha256
        && observed.sidecar_file_sha256 == authorized.original_sidecar_file_sha256
        && observed.sidecar_value == authorized.original_sidecar_value
        && observed.slot_a_binary_sha256 == authorized.slot_a_binary_sha256
        && observed.slot_a_sidecar_file_sha256 == authorized.slot_a_sidecar_file_sha256
        && observed.slot_a_manifest_sha256 == authorized.slot_a_manifest_sha256
        && observed.pointer_sha256 == authorized.original_pointer_sha256
}

fn validate_release_pair_fixed_inputs(
    workspace: &Path,
    authorized: &ReleasePairReconcileAuthorizationV1,
) -> Result<(), String> {
    if authorized.schema_version != 1
        || authorized.expires_at_unix < now_unix()
        || authorized.authorization_id.len() != 32
        || authorized.transaction_id.len() != 32
        || !authorized
            .authorization_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !authorized
            .transaction_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("release-pair authorization is unavailable".into());
    }
    validate_release_pair_review(workspace, authorized)?;
    let script = canonical_workspace_script(
        workspace,
        &workspace.join("scripts/reconcile-reviewed-catdesk-release-pair.ps1"),
        "fixed release-pair reconciliation script",
    )?;
    let recovery_helper = canonical_workspace_script(
        workspace,
        &workspace.join("scripts/catdesk-release-recovery.ps1"),
        "fixed release-pair recovery helper",
    )?;
    let powershell = crate::operator_facade::trusted_windows_powershell(workspace)?;
    if sha256_file(&script)? != authorized.script_sha256
        || sha256_file(&recovery_helper)? != authorized.recovery_helper_sha256
        || sha256_file(&powershell)? != authorized.powershell_sha256
    {
        return Err("release-pair fixed worker evidence drifted".into());
    }
    Ok(())
}

fn create_new_release_pair_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("release-pair control path is invalid")?;
    fs::create_dir_all(parent).map_err(|_| "release-pair control is unavailable")?;
    let bytes = serde_json::to_vec(value).map_err(|_| "release-pair control is malformed")?;
    if bytes.is_empty() || bytes.len() > 8192 {
        return Err("release-pair control is oversized".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "release-pair control is already claimed".to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "release-pair control write failed".to_string())
}

pub fn confirm_release_pair_reconcile(
    workspace: &Path,
    confirmation_token: &str,
) -> Result<ReleasePairReconcileConfirmationOutcomeV1, String> {
    let workspace = canonical_directory(workspace, "release-pair workspace")?;
    let preflight: ReleasePairReconcilePreflightV1 = read_bounded_json(
        &release_pair_preflight_path(&workspace),
        "release-pair preflight",
    )?;
    if preflight.schema_version != 1 || preflight.confirmation_token != confirmation_token {
        return Err("release-pair confirmation is unavailable".into());
    }
    let authorized = &preflight.authorization;
    let claim = ReleasePairReconcileClaimV1 {
        schema_version: 1,
        authorization_id: authorized.authorization_id.clone(),
        transaction_id: authorized.transaction_id.clone(),
        state: "CLAIMED_PENDING".into(),
    };
    let claim_path = release_pair_claim_path(&workspace);
    if claim_path.exists() {
        let prior: ReleasePairReconcileClaimV1 =
            read_bounded_json(&claim_path, "release-pair claim")?;
        if prior != claim {
            return Err("release-pair claim belongs to another attempt".into());
        }
        let result = release_pair_reconcile_result(&workspace)?;
        if result.as_ref().is_some_and(|result| {
            result.transaction_id != authorized.transaction_id
                || result.candidate_sha256 != authorized.candidate_sha256
        }) {
            return Err("release-pair result belongs to another transaction".into());
        }
        return Ok(ReleasePairReconcileConfirmationOutcomeV1 {
            state: match result.as_ref().map(|result| result.state.as_str()) {
                Some("RECONCILED_CANONICAL_READY") => "RECONCILED_CANONICAL_READY",
                Some("RESTORED_ORIGINAL_OPERATOR_ATTENTION") => {
                    "RESTORED_ORIGINAL_OPERATOR_ATTENTION"
                }
                Some(_) => "RECONCILE_UNPROVEN_OPERATOR_ATTENTION",
                None => "CLAIMED_OUTCOME_UNPROVEN_OPERATOR_ATTENTION",
            },
            candidate_sha256: authorized.candidate_sha256.clone(),
        });
    }
    if preflight.expires_at_unix < now_unix() {
        return Err("release-pair confirmation expired".into());
    }
    validate_release_pair_fixed_inputs(&workspace, authorized)?;
    if !release_pair_observed_matches(&measure_divergent_release_pair(&workspace)?, authorized) {
        return Err("divergent release-pair evidence drifted".into());
    }
    #[cfg(target_os = "windows")]
    crate::reviewed_candidate_owner::OwnedCandidateChild::require_uncontained_owner()?;
    let current = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|_| "release-pair worker image is unavailable".to_string())?;
    let canonical = canonical_workspace_file(
        &workspace,
        &workspace.join("target/release/catdesk.exe"),
        "canonical release",
    )?;
    if current == canonical {
        return Err("release-pair worker cannot replace its own mapped image".into());
    }
    let authorization_path = release_pair_authorization_path(&workspace);
    if authorization_path.exists() {
        let prior: ReleasePairReconcileAuthorizationV1 =
            read_bounded_json(&authorization_path, "release-pair authorization")?;
        if prior != *authorized {
            return Err("release-pair authorization belongs to another attempt".into());
        }
    } else {
        create_new_release_pair_json(&authorization_path, authorized)?;
    }
    create_new_release_pair_json(&claim_path, &claim)?;
    let mut command = Command::new(current);
    command
        .arg(RELEASE_PAIR_RECONCILE_WORKER_FLAG)
        .arg("--workspace")
        .arg(&workspace)
        .arg("--authorization")
        .arg(&authorized.authorization_id)
        .arg("--transaction")
        .arg(&authorized.transaction_id)
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_detached_process_flags(&mut command);
    if command.spawn().is_err() {
        let _ = atomic_json_write(
            &release_pair_result_path(&workspace),
            &ReleasePairReconcileResultV1 {
                schema_version: 1,
                state: "RECONCILE_UNPROVEN_OPERATOR_ATTENTION".into(),
                transaction_id: authorized.transaction_id.clone(),
                candidate_sha256: authorized.candidate_sha256.clone(),
                owner_pid: None,
            },
        );
        return Err("release-pair worker could not be scheduled".into());
    }
    Ok(ReleasePairReconcileConfirmationOutcomeV1 {
        state: "NEWLY_SCHEDULED",
        candidate_sha256: authorized.candidate_sha256.clone(),
    })
}

pub fn release_pair_reconcile_result(
    workspace: &Path,
) -> Result<Option<ReleasePairReconcileResultV1>, String> {
    let workspace = canonical_directory(workspace, "release-pair workspace")?;
    let path = release_pair_result_path(&workspace);
    if !path.exists() {
        return Ok(None);
    }
    let result: ReleasePairReconcileResultV1 = read_bounded_json(&path, "release-pair result")?;
    if result.schema_version != 1
        || !matches!(
            result.state.as_str(),
            "RECONCILED_CANONICAL_READY"
                | "RESTORED_ORIGINAL_OPERATOR_ATTENTION"
                | "RECONCILE_UNPROVEN_OPERATOR_ATTENTION"
        )
        || result.transaction_id.len() != 32
        || normalize_sha256(&result.candidate_sha256).is_err()
    {
        return Err("release-pair result is malformed".into());
    }
    let authorization: ReleasePairReconcileAuthorizationV1 = read_bounded_json(
        &release_pair_authorization_path(&workspace),
        "release-pair authorization",
    )?;
    let claim: ReleasePairReconcileClaimV1 =
        read_bounded_json(&release_pair_claim_path(&workspace), "release-pair claim")?;
    if authorization.schema_version != 1
        || claim.schema_version != 1
        || claim.state != "CLAIMED_PENDING"
        || claim.authorization_id != authorization.authorization_id
        || claim.transaction_id != authorization.transaction_id
        || result.transaction_id != authorization.transaction_id
        || result.candidate_sha256 != authorization.candidate_sha256
    {
        return Err("release-pair result authority drifted".into());
    }
    Ok(Some(result))
}

pub fn release_pair_reconcile_claimed(workspace: &Path) -> Result<bool, String> {
    let workspace = canonical_directory(workspace, "release-pair workspace")?;
    let claim_path = release_pair_claim_path(&workspace);
    if !claim_path.exists() {
        return Ok(false);
    }
    let authorization: ReleasePairReconcileAuthorizationV1 = read_bounded_json(
        &release_pair_authorization_path(&workspace),
        "release-pair authorization",
    )?;
    let claim: ReleasePairReconcileClaimV1 = read_bounded_json(&claim_path, "release-pair claim")?;
    if authorization.schema_version != 1
        || claim.schema_version != 1
        || claim.state != "CLAIMED_PENDING"
        || claim.authorization_id != authorization.authorization_id
        || claim.transaction_id != authorization.transaction_id
    {
        return Err("release-pair claim authority drifted".into());
    }
    Ok(true)
}

#[cfg(target_os = "windows")]
fn run_release_pair_phase(
    workspace: &Path,
    authorized: &ReleasePairReconcileAuthorizationV1,
    phase: crate::operator_facade::ReleasePairPhase,
    owner_pid: Option<u32>,
    owner_started_at_utc: Option<&str>,
    expected_state: &str,
) -> Result<ReleasePairScriptResultV1, String> {
    // The same exact script, transitive helper and interpreter bytes are
    // rechecked before *every* fixed invocation, including rollback.
    let script = canonical_workspace_script(
        workspace,
        &workspace.join("scripts/reconcile-reviewed-catdesk-release-pair.ps1"),
        "fixed release-pair reconciliation script",
    )?;
    let helper = canonical_workspace_script(
        workspace,
        &workspace.join("scripts/catdesk-release-recovery.ps1"),
        "fixed release-pair recovery helper",
    )?;
    let powershell = crate::operator_facade::trusted_windows_powershell(workspace)?;
    if sha256_file(&script)? != authorized.script_sha256
        || sha256_file(&helper)? != authorized.recovery_helper_sha256
        || sha256_file(&powershell)? != authorized.powershell_sha256
    {
        return Err("release-pair fixed source changed".into());
    }
    let invocation = crate::operator_facade::release_pair_invocation(
        workspace,
        phase,
        &authorized.authorization_id,
        &authorized.transaction_id,
        owner_pid,
        owner_started_at_utc,
    )?;
    let result = crate::operator_facade::run_fixed_powershell_invocation_blocking(invocation)?;
    if !result.success || !result.stderr.trim().is_empty() || result.stdout.len() > 4096 {
        return Err("release-pair fixed phase requires operator attention".into());
    }
    let parsed: ReleasePairScriptResultV1 = serde_json::from_str(result.stdout.trim())
        .map_err(|_| "release-pair fixed phase result is malformed".to_string())?;
    if parsed.state != expected_state
        || parsed.transaction_id != authorized.transaction_id
        || owner_pid.is_some_and(|pid| parsed.owner_pid != Some(pid))
        || parsed
            .candidate_sha256
            .as_ref()
            .is_some_and(|hash| hash != &authorized.candidate_sha256)
    {
        return Err("release-pair fixed phase result drifted".into());
    }
    Ok(parsed)
}

#[cfg(target_os = "windows")]
fn prove_release_pair_empty_listener() -> Result<(), String> {
    if !windows_listener_owner_pids(3200)?.is_empty() {
        return Err("canonical MCP listener is occupied".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn rollback_release_pair_after_quiescence(
    workspace: &Path,
    authorized: &ReleasePairReconcileAuthorizationV1,
) -> Result<(), String> {
    prove_release_pair_empty_listener()?;
    run_release_pair_phase(
        workspace,
        authorized,
        crate::operator_facade::ReleasePairPhase::Empty,
        None,
        None,
        "CANONICAL_OWNER_EMPTY",
    )?;
    create_new_release_pair_json(
        &release_pair_quiescence_path(workspace),
        &ReleasePairQuiescenceV1 {
            schema_version: 1,
            state: "OWNED_TREE_EXITED",
            authorization_id: &authorized.authorization_id,
            transaction_id: &authorized.transaction_id,
        },
    )?;
    run_release_pair_phase(
        workspace,
        authorized,
        crate::operator_facade::ReleasePairPhase::Rollback,
        None,
        None,
        "RESTORED_ORIGINAL_OPERATOR_ATTENTION",
    )?;
    Ok(())
}

/// This worker is scheduled only after an operator confirms a one-shot,
/// independently reviewed candidate bound to the two divergent canonical
/// files and the operational escrow. It owns every stage to final readback.
#[cfg(target_os = "windows")]
pub fn run_release_pair_reconcile_worker(
    args: ReleasePairReconcileWorkerArgsV1,
) -> Result<(), String> {
    use crate::operator_facade::ReleasePairPhase;
    use crate::reviewed_candidate_owner::OwnedCandidateChild;

    let workspace = canonical_directory(&args.workspace, "release-pair workspace")?;
    let authorized: ReleasePairReconcileAuthorizationV1 = read_bounded_json(
        &release_pair_authorization_path(&workspace),
        "release-pair authorization",
    )?;
    let claim: ReleasePairReconcileClaimV1 =
        read_bounded_json(&release_pair_claim_path(&workspace), "release-pair claim")?;
    if authorized.authorization_id != args.authorization_id
        || authorized.transaction_id != args.transaction_id
        || claim.schema_version != 1
        || claim.authorization_id != authorized.authorization_id
        || claim.transaction_id != authorized.transaction_id
        || claim.state != "CLAIMED_PENDING"
    {
        return Err("release-pair worker authority is unavailable".into());
    }
    let mut owner_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(release_pair_worker_owner_path(&workspace))
        .map_err(|_| "release-pair worker is already owned".to_string())?;
    if owner_file
        .write_all(authorized.transaction_id.as_bytes())
        .and_then(|_| owner_file.sync_all())
        .is_err()
    {
        let _ = atomic_json_write(
            &release_pair_result_path(&workspace),
            &ReleasePairReconcileResultV1 {
                schema_version: 1,
                state: "RECONCILE_UNPROVEN_OPERATOR_ATTENTION".into(),
                transaction_id: authorized.transaction_id,
                candidate_sha256: authorized.candidate_sha256,
                owner_pid: None,
            },
        );
        return Err("release-pair worker ownership is unavailable".into());
    }

    // The script's fixed runner owns a kill-on-close PowerShell Job. It must
    // finish before the daemon is spawned into this Rust worker's separate Job.
    let mut staged = false;
    let mut released = false;
    let mut owned: Option<OwnedCandidateChild> = None;
    let mut spawn_quiescence_proven = true;
    let mut owner_pid = None;
    let outcome = (|| -> Result<&'static str, String> {
        validate_release_pair_fixed_inputs(&workspace, &authorized)?;
        if !release_pair_observed_matches(&measure_divergent_release_pair(&workspace)?, &authorized)
        {
            return Err("divergent release-pair evidence drifted".into());
        }
        OwnedCandidateChild::require_uncontained_owner()?;
        prove_release_pair_empty_listener()?;
        run_release_pair_phase(
            &workspace,
            &authorized,
            ReleasePairPhase::Empty,
            None,
            None,
            "CANONICAL_OWNER_EMPTY",
        )?;
        // A failed Stage may have journaled and swapped only one of the two
        // files. Keep rollback eligible even when it never returned STAGED.
        staged = true;
        run_release_pair_phase(
            &workspace,
            &authorized,
            ReleasePairPhase::Stage,
            None,
            None,
            "STAGED",
        )?;
        let canonical = canonical_workspace_file(
            &workspace,
            &workspace.join("target/release/catdesk.exe"),
            "reconciled canonical image",
        )?;
        if sha256_file(&canonical)? != authorized.candidate_sha256 {
            return Err("reconciled candidate pair drifted".into());
        }
        let mut command = Command::new(&canonical);
        command
            .arg(DAEMON_MODE_FLAG)
            .current_dir(&workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let candidate = OwnedCandidateChild::spawn_classified(&mut command).map_err(|failure| {
            spawn_quiescence_proven = failure.quiescence_proven();
            failure.message()
        })?;
        owner_pid = Some(candidate.pid());
        owned = Some(candidate);
        let child = owned.as_mut().ok_or("candidate owner is unavailable")?;
        if !wait_child_ready(child.child_mut(), 3200, READY_TIMEOUT) {
            return Err("reconciled candidate did not become ready".into());
        }
        let proof = run_release_pair_phase(
            &workspace,
            &authorized,
            ReleasePairPhase::Probe,
            owner_pid,
            None,
            "SERVING_VERIFIED",
        )?;
        let started = proof
            .owner_started_at_utc
            .ok_or("reconciled candidate process instance is unproven")?;
        if started.is_empty()
            || started.len() > 64
            || proof.candidate_sha256.as_deref() != Some(authorized.candidate_sha256.as_str())
        {
            return Err("reconciled candidate process instance is unproven".into());
        }
        create_new_release_pair_json(
            &release_pair_pending_path(&workspace),
            &ReleasePairOwnerMarkerV1 {
                schema_version: 1,
                state: "SERVING_VERIFIED_PENDING_JOB_RELEASE",
                authorization_id: &authorized.authorization_id,
                transaction_id: &authorized.transaction_id,
                owner_pid: owner_pid.ok_or("candidate owner is unavailable")?,
                owner_started_at_utc: &started,
            },
        )?;
        child.disarm_live_retaining_job()?;
        let retained = run_release_pair_phase(
            &workspace,
            &authorized,
            ReleasePairPhase::Probe,
            owner_pid,
            Some(&started),
            "SERVING_VERIFIED",
        )?;
        if retained.owner_started_at_utc.as_deref() != Some(started.as_str()) {
            return Err("reconciled candidate instance drifted before Job release".into());
        }
        child.release_live()?;
        released = true;
        // A closing private Job is the persistence boundary. After it, any
        // ambiguous exit must stay in attention; old bytes may still be mapped.
        let post_close = run_release_pair_phase(
            &workspace,
            &authorized,
            ReleasePairPhase::Probe,
            owner_pid,
            Some(&started),
            "SERVING_VERIFIED",
        )?;
        if post_close.owner_started_at_utc.as_deref() != Some(started.as_str()) {
            return Err("reconciled candidate instance drifted after Job release".into());
        }
        create_new_release_pair_json(
            &release_pair_released_path(&workspace),
            &ReleasePairOwnerMarkerV1 {
                schema_version: 1,
                state: "JOB_RELEASED_PENDING_POSTPROOF",
                authorization_id: &authorized.authorization_id,
                transaction_id: &authorized.transaction_id,
                owner_pid: owner_pid.ok_or("candidate owner is unavailable")?,
                owner_started_at_utc: &started,
            },
        )?;
        run_release_pair_phase(
            &workspace,
            &authorized,
            ReleasePairPhase::Commit,
            owner_pid,
            Some(&started),
            "RECONCILED_CANONICAL_READY",
        )?;
        Ok("RECONCILED_CANONICAL_READY")
    })();

    let state = match outcome {
        Ok(state) => state,
        Err(_) if !released => {
            let quiescent = match owned.as_mut() {
                Some(child) => child.terminate_tree_and_confirm_quiescent().is_ok(),
                None if spawn_quiescence_proven => {
                    prove_release_pair_empty_listener().is_ok()
                        && run_release_pair_phase(
                            &workspace,
                            &authorized,
                            ReleasePairPhase::Empty,
                            None,
                            None,
                            "CANONICAL_OWNER_EMPTY",
                        )
                        .is_ok()
                }
                None => false,
            };
            if quiescent
                && staged
                && rollback_release_pair_after_quiescence(&workspace, &authorized).is_ok()
            {
                "RESTORED_ORIGINAL_OPERATOR_ATTENTION"
            } else {
                "RECONCILE_UNPROVEN_OPERATOR_ATTENTION"
            }
        }
        Err(_) => "RECONCILE_UNPROVEN_OPERATOR_ATTENTION",
    };
    atomic_json_write(
        &release_pair_result_path(&workspace),
        &ReleasePairReconcileResultV1 {
            schema_version: 1,
            state: state.into(),
            transaction_id: authorized.transaction_id,
            candidate_sha256: authorized.candidate_sha256,
            owner_pid,
        },
    )?;
    if state != "RECONCILED_CANONICAL_READY" {
        return Err("release-pair reconciliation requires operator attention".into());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn run_release_pair_reconcile_worker(
    _args: ReleasePairReconcileWorkerArgsV1,
) -> Result<(), String> {
    Err("release-pair reconciliation requires Windows".into())
}

/// Legacy daemon recovery cannot schedule a version-coupled script or runtime
/// replacement.  The only supported mutable supervisor lifecycle is the
/// closed `operator supervisor activate` composition, which owns the fixed
/// protected current/LKG transaction and startup authority.
pub fn schedule_canonical_recovery(_workspace: &Path) -> Result<CanonicalRecoveryStateV1, String> {
    Err("canonical recovery requires the closed supervisor lifecycle".into())
}

pub fn run_canonical_recovery_worker(args: CanonicalRecoveryWorkerArgsV1) -> Result<(), String> {
    let workspace = canonical_directory(&args.workspace, "canonical recovery workspace")?;
    let current = read_canonical_recovery_state(&workspace)?;
    if !matches!(current, Some(ref state) if state.status == RECOVERY_STATUS_SCHEDULED && state.attempt_id == args.attempt_id)
    {
        // An old helper must never overwrite a newer attempt or act after its
        // durable owner was replaced.
        return Ok(());
    }
    // A helper scheduled by an older binary may still reach this point after
    // upgrade.  It must retire its own durable attempt without executing the
    // old script/daemon/tunnel recovery chain; only the closed supervisor
    // lifecycle may perform future mutation.
    let status = RECOVERY_STATUS_AUTHORITY_REQUIRED;
    let state = CanonicalRecoveryStateV1 {
        schema_version: 2,
        status: status.into(),
        updated_at_unix: now_unix(),
        generation: current.expect("checked recovery owner").generation,
        attempt_id: args.attempt_id,
    };
    write_canonical_recovery_state(&workspace, &state)?;
    Err("canonical recovery requires the closed supervisor lifecycle".into())
}

pub fn run_lifecycle_stop_worker(args: LifecycleStopWorkerArgsV1) -> Result<(), String> {
    let workspace = canonical_directory(&args.workspace, "lifecycle stop workspace")?;
    let authorization_path = lifecycle_stop_authorization_path(&workspace, &args.attempt_id);
    let authorization: LifecycleStopAuthorizationV1 =
        read_bounded_json(&authorization_path, "lifecycle stop authorization")?;
    if authorization.schema_version != 1
        || authorization.attempt_id != args.attempt_id
        || authorization.target_pid == 0
        || authorization.target_pid == std::process::id()
        || authorization.expires_at_unix < now_unix()
        || authorization.target_executable_sha256.len() != 64
        || !authorization
            .target_executable_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("CatDesk lifecycle stop authorization is unavailable".into());
    }
    let current = std::env::current_exe()
        .map_err(|_| "CatDesk lifecycle stop helper is unavailable".to_string())?
        .canonicalize()
        .map_err(|_| "CatDesk lifecycle stop helper identity is unavailable".to_string())?;
    if !current.starts_with(&workspace)
        || sha256_file(&current)? != authorization.target_executable_sha256
    {
        return Err("CatDesk lifecycle stop helper identity did not match authorization".into());
    }

    // Consume the one-shot authority before any destructive action. A replayed
    // worker or manually constructed worker invocation therefore fails closed.
    fs::remove_file(&authorization_path)
        .map_err(|_| "CatDesk lifecycle stop authorization could not be consumed".to_string())?;

    // The MCP caller has already received STOP_ACKNOWLEDGED before this worker
    // acts. Termination is bound to the exact internally captured serving PID
    // and the exact executable image/hash that spawned this helper. No shell,
    // PowerShell script, tunnel process, or caller-supplied PID is involved.
    thread::sleep(Duration::from_millis(1500));
    #[cfg(target_os = "windows")]
    {
        windows_terminate_exact_process(authorization.target_pid, &current)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = authorization;
        Err("CatDesk lifecycle stop helper is supported only on Windows".into())
    }
}

pub fn spawn_lifecycle_stop_helper(workspace: &Path) -> Result<(), String> {
    let workspace = canonical_directory(workspace, "lifecycle stop workspace")?;
    let current = std::env::current_exe()
        .map_err(|_| "CatDesk lifecycle stop helper is unavailable".to_string())?
        .canonicalize()
        .map_err(|_| "CatDesk lifecycle stop helper identity is unavailable".to_string())?;
    if !current.starts_with(&workspace) {
        return Err("CatDesk lifecycle stop helper must run from the workspace".into());
    }
    let attempt_id = Uuid::new_v4().to_string();
    let authorization = LifecycleStopAuthorizationV1 {
        schema_version: 1,
        attempt_id: attempt_id.clone(),
        target_pid: std::process::id(),
        target_executable_sha256: sha256_file(&current)?,
        expires_at_unix: now_unix().saturating_add(PREFLIGHT_TTL_SECONDS),
    };
    let authorization_path = lifecycle_stop_authorization_path(&workspace, &attempt_id);
    atomic_json_write(&authorization_path, &authorization)?;

    let mut command = Command::new(&current);
    command
        .arg(LIFECYCLE_STOP_WORKER_FLAG)
        .arg("--workspace")
        .arg(&workspace)
        .arg("--attempt")
        .arg(&attempt_id)
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_detached_process_flags(&mut command);
    if command.spawn().is_err() {
        let _ = fs::remove_file(&authorization_path);
        return Err("CatDesk lifecycle stop helper could not be scheduled".into());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadHandoffStateV1 {
    pub schema_version: u8,
    pub status: String,
    pub completed_at_unix: u64,
    pub old_pid: u32,
    pub new_pid: Option<u32>,
    pub port: u16,
    pub replacement_sha256: String,
    pub stage: String,
    pub rollback_attempted: bool,
    pub rollback_ready: bool,
}

pub fn parse_reload_worker_args(args: &[String]) -> Result<Option<ReloadWorkerArgsV1>, String> {
    if !args.iter().any(|arg| arg == RELOAD_WORKER_FLAG) {
        return Ok(None);
    }
    if args.iter().filter(|arg| *arg == RELOAD_WORKER_FLAG).count() != 1 {
        return Err("CatDesk reload worker flag may only be supplied once".into());
    }
    let value = |name: &str| -> Result<String, String> {
        let position = args
            .iter()
            .position(|arg| arg == name)
            .ok_or_else(|| format!("CatDesk reload worker requires {name}"))?;
        args.get(position + 1)
            .filter(|value| !value.is_empty() && !value.starts_with("--"))
            .cloned()
            .ok_or_else(|| format!("CatDesk reload worker requires a value for {name}"))
    };
    let old_pid = value("--old-pid")?
        .parse::<u32>()
        .map_err(|_| "CatDesk reload worker old pid is invalid".to_string())?;
    let port = value("--mcp-port")?
        .parse::<u16>()
        .map_err(|_| "CatDesk reload worker MCP port is invalid".to_string())?;
    if port == 0 {
        return Err("CatDesk reload worker MCP port must be non-zero".into());
    }
    let expected_sha256 = normalize_sha256(&value("--expected-sha256")?)?;
    Ok(Some(ReloadWorkerArgsV1 {
        old_pid,
        port,
        workspace: PathBuf::from(value("--workspace")?),
        replacement_path: PathBuf::from(value("--replacement")?),
        rollback_path: PathBuf::from(value("--rollback")?),
        expected_sha256,
    }))
}

pub fn run_reload_worker(args: ReloadWorkerArgsV1) -> Result<(), String> {
    let workspace = canonical_directory(&args.workspace, "reload workspace")?;
    let replacement =
        match canonical_workspace_file(&workspace, &args.replacement_path, "replacement") {
            Ok(replacement) => replacement,
            Err(_) => {
                journal_worker_failure(
                    &workspace,
                    &args,
                    "FAILED_REPLACEMENT_VALIDATION",
                    "validate-replacement-path",
                    &args.expected_sha256,
                )?;
                return Err("replacement validation failed after reload preflight".into());
            }
        };
    let rollback = match canonical_workspace_file(&workspace, &args.rollback_path, "rollback") {
        Ok(rollback) => rollback,
        Err(_) => {
            journal_worker_failure(
                &workspace,
                &args,
                "FAILED_ROLLBACK_VALIDATION",
                "validate-rollback-path",
                &args.expected_sha256,
            )?;
            return Err("rollback validation failed after reload preflight".into());
        }
    };
    let actual_hash = match sha256_file(&replacement) {
        Ok(hash) => hash,
        Err(_) => {
            journal_worker_failure(
                &workspace,
                &args,
                "FAILED_REPLACEMENT_HASH",
                "hash-replacement",
                &args.expected_sha256,
            )?;
            return Err("replacement hashing failed after reload preflight".into());
        }
    };
    if actual_hash != args.expected_sha256 {
        journal_worker_failure(
            &workspace,
            &args,
            "FAILED_REPLACEMENT_HASH_MISMATCH",
            "validate-replacement-hash",
            &actual_hash,
        )?;
        return Err("replacement hash changed after reload preflight".into());
    }

    if wait_for_old_daemon_exit(args.old_pid, &rollback, &workspace, args.port, &actual_hash)
        .is_err()
    {
        journal_worker_failure(
            &workspace,
            &args,
            "FAILED_OLD_DAEMON_EXIT",
            "wait-old-daemon-exit",
            &actual_hash,
        )?;
        return Err("old daemon exit or port-release wait failed after reload preflight".into());
    }

    let mut replacement_child = match launch_catdesk(&replacement, &workspace) {
        Ok(child) => child,
        Err(_) => {
            journal_worker_failure(
                &workspace,
                &args,
                "FAILED_REPLACEMENT_LAUNCH",
                "replacement-launch",
                &actual_hash,
            )?;
            return Err("replacement CatDesk launch failed after reload preflight".into());
        }
    };
    if wait_child_ready(&mut replacement_child, args.port, READY_TIMEOUT) {
        write_handoff_state(
            &workspace,
            &ReloadHandoffStateV1 {
                schema_version: 1,
                status: "REPLACEMENT_READY_PENDING_TRANSPORT_RECONNECT".into(),
                completed_at_unix: now_unix(),
                old_pid: args.old_pid,
                new_pid: Some(replacement_child.id()),
                port: args.port,
                replacement_sha256: actual_hash,
                stage: "complete".into(),
                rollback_attempted: false,
                rollback_ready: false,
            },
        )?;
        return Ok(());
    }

    let _ = replacement_child.kill();
    let _ = replacement_child.wait();
    let rollback_hash = sha256_file(&rollback).unwrap_or_else(|_| "unavailable".into());
    let mut rollback_child = launch_catdesk(&rollback, &workspace).map_err(|error| {
        let _ = write_handoff_state(
            &workspace,
            &ReloadHandoffStateV1 {
                schema_version: 1,
                status: "FAILED_REPLACEMENT_AND_ROLLBACK_LAUNCH".into(),
                completed_at_unix: now_unix(),
                old_pid: args.old_pid,
                new_pid: None,
                port: args.port,
                replacement_sha256: actual_hash.clone(),
                stage: format!("rollback-launch:{rollback_hash}"),
                rollback_attempted: true,
                rollback_ready: false,
            },
        );
        format!("replacement failed readiness and rollback launch failed: {error}")
    })?;
    let rollback_ready = wait_child_ready(&mut rollback_child, args.port, READY_TIMEOUT);
    write_handoff_state(
        &workspace,
        &ReloadHandoffStateV1 {
            schema_version: 1,
            status: if rollback_ready {
                "ROLLED_BACK_READY_PENDING_TRANSPORT_RECONNECT"
            } else {
                "FAILED_REPLACEMENT_AND_ROLLBACK_READINESS"
            }
            .into(),
            completed_at_unix: now_unix(),
            old_pid: args.old_pid,
            new_pid: rollback_ready.then(|| rollback_child.id()),
            port: args.port,
            replacement_sha256: actual_hash,
            stage: "rollback-readiness".into(),
            rollback_attempted: true,
            rollback_ready,
        },
    )?;
    if rollback_ready {
        Err("replacement failed readiness; prior CatDesk daemon was restored".into())
    } else {
        let _ = rollback_child.kill();
        Err("replacement and rollback both failed readiness".into())
    }
}

pub fn measure_reload_candidate(
    workspace: &Path,
    replacement_path: &Path,
    expected_sha256: Option<&str>,
) -> Result<ReloadCandidateMeasurementV1, String> {
    let workspace = canonical_directory(workspace, "reload workspace")?;
    let replacement = canonical_workspace_file(&workspace, replacement_path, "replacement")?;
    let actual_sha256 = sha256_file(&replacement)?;
    let expected_sha256 = if let Some(expected) = expected_sha256 {
        let expected = normalize_sha256(expected)?;
        if actual_sha256 != expected {
            return Err("replacement CatDesk SHA-256 does not match expectedSha256".into());
        }
        expected
    } else {
        actual_sha256
    };
    let candidate_relative_path = replacement
        .strip_prefix(&workspace)
        .map_err(|_| "replacement CatDesk candidate is outside the reload workspace".to_string())?
        .to_string_lossy()
        .into_owned();
    if candidate_relative_path.is_empty() {
        return Err("replacement CatDesk candidate identity is invalid".into());
    }
    let candidate_length = fs::metadata(&replacement)
        .map_err(|_| "replacement CatDesk metadata is unavailable".to_string())?
        .len();
    if candidate_length == 0 {
        return Err("replacement CatDesk candidate is empty".into());
    }
    Ok(ReloadCandidateMeasurementV1 {
        candidate_relative_path,
        candidate_sha256: expected_sha256,
        candidate_length,
    })
}

pub fn prepare_reviewed_reload(
    workspace: &Path,
    replacement_path: &Path,
    expected_sha256: Option<&str>,
    port: u16,
    review_binding: ReloadReviewBindingV1,
) -> Result<ReloadPreflightV1, String> {
    validate_reload_review_binding(&review_binding)?;
    prepare_reload_with_review_binding(
        workspace,
        replacement_path,
        expected_sha256,
        port,
        Some(review_binding),
    )
}

fn prepare_reload_with_review_binding(
    workspace: &Path,
    replacement_path: &Path,
    expected_sha256: Option<&str>,
    port: u16,
    review_binding: Option<ReloadReviewBindingV1>,
) -> Result<ReloadPreflightV1, String> {
    if port == 0 {
        return Err("CatDesk self-reload requires a non-zero MCP port".into());
    }
    let workspace = canonical_directory(workspace, "reload workspace")?;
    let replacement = canonical_workspace_file(&workspace, replacement_path, "replacement")?;
    let rollback = std::env::current_exe()
        .map_err(|_| "current CatDesk executable could not be resolved".to_string())?;
    let rollback = canonical_workspace_file(&workspace, &rollback, "current CatDesk executable")?;
    if replacement == rollback {
        return Err("replacement CatDesk executable must differ from the running daemon".into());
    }
    let actual_sha256 = sha256_file(&replacement)?;
    let expected_sha256 = if let Some(expected) = expected_sha256 {
        let expected = normalize_sha256(expected)?;
        if actual_sha256 != expected {
            return Err("replacement CatDesk SHA-256 does not match expectedSha256".into());
        }
        expected
    } else {
        actual_sha256
    };
    let created_at_unix = now_unix();
    let expires_at_unix = created_at_unix.saturating_add(PREFLIGHT_TTL_SECONDS);
    let confirmation_token = confirmation_token(
        &workspace,
        &replacement,
        &expected_sha256,
        std::process::id(),
        expires_at_unix,
    );
    let preflight = ReloadPreflightV1 {
        schema_version: 1,
        confirmation_token,
        created_at_unix,
        expires_at_unix,
        old_pid: std::process::id(),
        port,
        workspace,
        replacement_path: replacement,
        rollback_path: rollback,
        expected_sha256,
        review_binding,
    };
    write_preflight(&preflight)?;
    Ok(preflight)
}

pub fn read_reload_preflight(workspace: &Path) -> Result<ReloadPreflightV1, String> {
    let workspace = canonical_directory(workspace, "reload workspace")?;
    read_preflight(&workspace)
}

pub fn execute_reviewed_reload(
    workspace: &Path,
    replacement_path: &Path,
    expected_sha256: &str,
    confirmation_token: &str,
    port: u16,
    review_binding: &ReloadReviewBindingV1,
) -> Result<ReloadPreflightV1, String> {
    validate_reload_review_binding(review_binding)?;
    execute_reload_with_review_binding(
        workspace,
        replacement_path,
        expected_sha256,
        confirmation_token,
        port,
        Some(review_binding),
    )
}

fn execute_reload_with_review_binding(
    workspace: &Path,
    replacement_path: &Path,
    expected_sha256: &str,
    confirmation_token: &str,
    port: u16,
    expected_review_binding: Option<&ReloadReviewBindingV1>,
) -> Result<ReloadPreflightV1, String> {
    let workspace = canonical_directory(workspace, "reload workspace")?;
    let replacement = canonical_workspace_file(&workspace, replacement_path, "replacement")?;
    let expected_sha256 = normalize_sha256(expected_sha256)?;
    let persisted = read_preflight(&workspace)?;
    if persisted.schema_version != 1
        || persisted.confirmation_token != confirmation_token
        || persisted.old_pid != std::process::id()
        || persisted.port != port
        || persisted.workspace != workspace
        || persisted.replacement_path != replacement
        || persisted.expected_sha256 != expected_sha256
        || persisted.review_binding.as_ref() != expected_review_binding
        || now_unix() > persisted.expires_at_unix
    {
        return Err(
            "CatDesk self-reload confirmation is expired or does not match this daemon/build/review"
                .into(),
        );
    }
    if sha256_file(&replacement)? != expected_sha256 {
        return Err("replacement CatDesk SHA-256 changed after preflight".into());
    }
    spawn_reload_helper(&persisted)?;
    Ok(persisted)
}

fn validate_reload_review_binding(binding: &ReloadReviewBindingV1) -> Result<(), String> {
    let request_sha256 = normalize_sha256(&binding.request_sha256)?;
    let authority_sha256 = normalize_sha256(&binding.authority_sha256)?;
    let valid_identity = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    };
    if binding.schema_version != 1
        || !valid_identity(&binding.review_record_id)
        || !valid_identity(&binding.review_session_id)
        || request_sha256 != binding.request_sha256
        || authority_sha256 != binding.authority_sha256
    {
        return Err("CatDesk self-reload review binding is invalid".into());
    }
    Ok(())
}

fn spawn_reload_helper(preflight: &ReloadPreflightV1) -> Result<(), String> {
    let current = std::env::current_exe()
        .map_err(|_| "current CatDesk executable could not be resolved".to_string())?;
    let mut command = Command::new(current);
    command
        .arg(RELOAD_WORKER_FLAG)
        .arg("--old-pid")
        .arg(preflight.old_pid.to_string())
        .arg("--mcp-port")
        .arg(preflight.port.to_string())
        .arg("--workspace")
        .arg(&preflight.workspace)
        .arg("--replacement")
        .arg(&preflight.replacement_path)
        .arg("--rollback")
        .arg(&preflight.rollback_path)
        .arg("--expected-sha256")
        .arg(&preflight.expected_sha256)
        .current_dir(&preflight.workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_detached_process_flags(&mut command);
    command
        .spawn()
        .map_err(|error| format!("failed to spawn native CatDesk reload helper: {error}"))?;
    Ok(())
}

pub fn schedule_server_exit_after_response() {
    thread::spawn(|| {
        // This must be invoked by the HTTP server process that owns the MCP
        // listener, not by a lower-level tool implementation. Give the
        // successful JSON-RPC response enough time to flush before releasing
        // the loopback listener. The detached helper waits for that listener
        // to disappear before launching the replacement.
        thread::sleep(Duration::from_millis(1500));
        std::process::exit(0);
    });
}

fn wait_for_old_daemon_exit(
    old_pid: u32,
    rollback: &Path,
    workspace: &Path,
    port: u16,
    replacement_sha256: &str,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let graceful_deadline = Instant::now() + GRACEFUL_EXIT_TIMEOUT;
        loop {
            if !windows_process_is_exact_and_alive(old_pid, rollback)? {
                break;
            }
            if Instant::now() >= graceful_deadline {
                windows_terminate_exact_process(old_pid, rollback)?;
                break;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let graceful_deadline = Instant::now() + GRACEFUL_EXIT_TIMEOUT;
        while loopback_port_accepts(port) && Instant::now() < graceful_deadline {
            thread::sleep(POLL_INTERVAL);
        }
    }

    let port_deadline = Instant::now() + PORT_RELEASE_TIMEOUT;
    loop {
        #[cfg(target_os = "windows")]
        {
            let owners = windows_listener_owner_pids(port)?;
            if owners.is_empty() {
                break;
            }
            if owners.iter().any(|pid| *pid != old_pid) {
                write_handoff_state(
                    workspace,
                    &ReloadHandoffStateV1 {
                        schema_version: 1,
                        status: "FAILED_PORT_RECLAIMED_BY_OTHER_PROCESS".into(),
                        completed_at_unix: now_unix(),
                        old_pid,
                        new_pid: None,
                        port,
                        replacement_sha256: replacement_sha256.to_string(),
                        stage: "wait-port-release-unexpected-owner".into(),
                        rollback_attempted: false,
                        rollback_ready: false,
                    },
                )?;
                return Err("MCP port was reclaimed by a different process during reload".into());
            }
        }
        #[cfg(not(target_os = "windows"))]
        if !loopback_port_accepts(port) {
            break;
        }

        if Instant::now() >= port_deadline {
            write_handoff_state(
                workspace,
                &ReloadHandoffStateV1 {
                    schema_version: 1,
                    status: "FAILED_PORT_STILL_LISTENING_AFTER_OLD_EXIT".into(),
                    completed_at_unix: now_unix(),
                    old_pid,
                    new_pid: None,
                    port,
                    replacement_sha256: replacement_sha256.to_string(),
                    stage: "wait-port-release-after-old-exit".into(),
                    rollback_attempted: false,
                    rollback_ready: false,
                },
            )?;
            return Err("MCP port remained owned after the old CatDesk process exited".into());
        }
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn make_listener_non_inheritable(listener: &tokio::net::TcpListener) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawSocket;

    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
        fn GetLastError() -> u32;
    }

    let handle = listener.as_raw_socket() as usize as *mut c_void;
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "failed to mark CatDesk MCP listener non-inheritable (win32={error})"
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn make_listener_non_inheritable(_listener: &tokio::net::TcpListener) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_listener_owner_pids(port: u16) -> Result<Vec<u32>, String> {
    use std::ffi::c_void;

    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TcpRowOwnerPidV4 {
        state: u32,
        local_addr: u32,
        local_port: u32,
        remote_addr: u32,
        remote_port: u32,
        owning_pid: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TcpRowOwnerPidV6 {
        local_addr: [u32; 4],
        local_scope_id: u32,
        local_port: u32,
        remote_addr: [u32; 4],
        remote_scope_id: u32,
        remote_port: u32,
        state: u32,
        owning_pid: u32,
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetExtendedTcpTable(
            tcp_table: *mut c_void,
            size: *mut u32,
            order: i32,
            af: u32,
            table_class: u32,
            reserved: u32,
        ) -> u32;
    }

    unsafe fn table_bytes(
        af: u32,
        get_table: unsafe extern "system" fn(*mut c_void, *mut u32, i32, u32, u32, u32) -> u32,
    ) -> Result<Vec<u8>, String> {
        let mut size = 0u32;
        let first = unsafe {
            get_table(
                std::ptr::null_mut(),
                &mut size,
                0,
                af,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if first != ERROR_INSUFFICIENT_BUFFER || size < 4 {
            return Err(format!(
                "Windows listener table size probe failed (af={af}, win32={first})"
            ));
        }
        let mut buffer = vec![0u8; size as usize];
        let status = unsafe {
            get_table(
                buffer.as_mut_ptr().cast(),
                &mut size,
                0,
                af,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status != 0 {
            return Err(format!(
                "Windows listener table query failed (af={af}, win32={status})"
            ));
        }
        Ok(buffer)
    }

    fn collect_rows<T: Copy>(
        buffer: &[u8],
        port_of: impl Fn(&T) -> u16,
        pid_of: impl Fn(&T) -> u32,
        port: u16,
        owners: &mut Vec<u32>,
    ) {
        if buffer.len() < 4 {
            return;
        }
        let count = u32::from_ne_bytes(buffer[0..4].try_into().unwrap()) as usize;
        let row_size = std::mem::size_of::<T>();
        for index in 0..count {
            let offset = 4 + index * row_size;
            if offset + row_size > buffer.len() {
                break;
            }
            let row = unsafe { std::ptr::read_unaligned(buffer.as_ptr().add(offset).cast::<T>()) };
            if port_of(&row) == port {
                owners.push(pid_of(&row));
            }
        }
    }

    let v4 = unsafe { table_bytes(AF_INET, GetExtendedTcpTable)? };
    let v6 = unsafe { table_bytes(AF_INET6, GetExtendedTcpTable)? };
    let mut owners = Vec::new();
    collect_rows(
        &v4,
        |row: &TcpRowOwnerPidV4| u16::from_be((row.local_port & 0xffff) as u16),
        |row| row.owning_pid,
        port,
        &mut owners,
    );
    collect_rows(
        &v6,
        |row: &TcpRowOwnerPidV6| u16::from_be((row.local_port & 0xffff) as u16),
        |row| row.owning_pid,
        port,
        &mut owners,
    );
    owners.sort_unstable();
    owners.dedup();
    Ok(owners)
}

#[cfg(target_os = "windows")]
fn windows_process_is_exact_and_alive(
    pid: u32,
    expected_executable: &Path,
) -> Result<bool, String> {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;

    type Handle = *mut c_void;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const ERROR_INVALID_PARAMETER: u32 = 87;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
        fn QueryFullProcessImageNameW(
            process: Handle,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    }

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let error = unsafe { GetLastError() };
        if error == ERROR_INVALID_PARAMETER {
            return Ok(false);
        }
        return Err(format!(
            "old CatDesk process could not be opened safely (win32={error})"
        ));
    }

    let wait = unsafe { WaitForSingleObject(handle, 0) };
    if wait == WAIT_OBJECT_0 {
        unsafe { CloseHandle(handle) };
        return Ok(false);
    }
    if wait != WAIT_TIMEOUT {
        unsafe { CloseHandle(handle) };
        return Err(format!(
            "old CatDesk process liveness check failed (wait={wait})"
        ));
    }

    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    let queried = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
    if queried == 0 {
        let wait_after_query = unsafe { WaitForSingleObject(handle, 0) };
        unsafe { CloseHandle(handle) };
        if wait_after_query == WAIT_OBJECT_0 {
            return Ok(false);
        }
        return Err("old CatDesk executable identity could not be queried".into());
    }
    unsafe { CloseHandle(handle) };

    let image = PathBuf::from(OsString::from_wide(&buffer[..size as usize]));
    let image = image
        .canonicalize()
        .map_err(|_| "old CatDesk executable identity could not be canonicalized".to_string())?;
    if windows_path_compare_key(&image) != windows_path_compare_key(expected_executable) {
        return Err("oldPid no longer resolves to the exact rollback CatDesk executable".into());
    }
    Ok(true)
}

#[cfg(target_os = "windows")]
fn windows_terminate_exact_process(pid: u32, expected_executable: &Path) -> Result<(), String> {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;

    type Handle = *mut c_void;
    const PROCESS_TERMINATE: u32 = 0x0001;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_OBJECT_0: u32 = 0;
    const ERROR_INVALID_PARAMETER: u32 = 87;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
        fn QueryFullProcessImageNameW(
            process: Handle,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn TerminateProcess(process: Handle, exit_code: u32) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    }

    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        let error = unsafe { GetLastError() };
        if error == ERROR_INVALID_PARAMETER {
            return Ok(());
        }
        return Err(format!(
            "old CatDesk process could not be opened for exact termination (win32={error})"
        ));
    }

    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) } == 0 {
        unsafe { CloseHandle(handle) };
        return Err(
            "old CatDesk executable identity could not be verified before termination".into(),
        );
    }
    let image = PathBuf::from(OsString::from_wide(&buffer[..size as usize]));
    let image = image.canonicalize().map_err(|_| {
        "old CatDesk executable identity could not be canonicalized before termination".to_string()
    })?;
    if windows_path_compare_key(&image) != windows_path_compare_key(expected_executable) {
        unsafe { CloseHandle(handle) };
        return Err("refused to terminate oldPid because executable identity changed".into());
    }

    if unsafe { TerminateProcess(handle, 0) } == 0 {
        let error = unsafe { GetLastError() };
        unsafe { CloseHandle(handle) };
        return Err(format!(
            "exact old CatDesk termination failed (win32={error})"
        ));
    }
    let wait = unsafe { WaitForSingleObject(handle, FORCED_EXIT_TIMEOUT.as_millis() as u32) };
    unsafe { CloseHandle(handle) };
    if wait != WAIT_OBJECT_0 {
        return Err(format!(
            "exact old CatDesk termination did not complete (wait={wait})"
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_path_compare_key(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let normalized = if let Some(rest) = raw.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = raw.strip_prefix("//?/") {
        rest.to_string()
    } else {
        raw
    };
    normalized.trim_end_matches('/').to_ascii_lowercase()
}

#[cfg(target_os = "windows")]
fn apply_detached_process_flags(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn apply_detached_process_flags(_command: &mut Command) {}

fn launch_catdesk(executable: &Path, workspace: &Path) -> Result<Child, String> {
    let mut command = Command::new(executable);
    command
        .arg(DAEMON_MODE_FLAG)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_detached_process_flags(&mut command);
    command
        .spawn()
        .map_err(|error| format!("failed to launch CatDesk daemon: {error}"))
}

fn wait_child_ready(child: &mut Child, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return false,
            Ok(None) => {}
        }
        if loopback_http_ready(port) {
            return true;
        }
        thread::sleep(POLL_INTERVAL);
    }
    false
}

#[cfg(not(target_os = "windows"))]
fn loopback_port_accepts(port: u16) -> bool {
    [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
    ]
    .into_iter()
    .any(|address| TcpStream::connect_timeout(&address, Duration::from_millis(150)).is_ok())
}

fn loopback_http_ready(port: u16) -> bool {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(750)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(750)));
    if stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buffer = [0u8; 256];
    let Ok(read) = stream.read(&mut buffer) else {
        return false;
    };
    read >= 12 && String::from_utf8_lossy(&buffer[..read]).starts_with("HTTP/1.1 200")
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|_| format!("{label} could not be canonicalized"))?;
    if !canonical.is_dir() {
        return Err(format!("{label} must be a directory"));
    }
    Ok(canonical)
}

fn canonical_workspace_regular_file(
    workspace: &Path,
    path: &Path,
    label: &str,
) -> Result<PathBuf, String> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|_| format!("{label} could not be canonicalized"))?;
    if !canonical.starts_with(workspace) || !canonical.is_file() {
        return Err(format!(
            "{label} must be an existing file inside the CatDesk workspace"
        ));
    }
    Ok(canonical)
}

fn canonical_workspace_script(
    workspace: &Path,
    path: &Path,
    label: &str,
) -> Result<PathBuf, String> {
    let canonical = canonical_workspace_regular_file(workspace, path, label)?;
    if canonical
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("ps1"))
    {
        return Err(format!("{label} must be a PowerShell .ps1 file"));
    }
    Ok(canonical)
}

fn canonical_workspace_file(workspace: &Path, path: &Path, label: &str) -> Result<PathBuf, String> {
    let canonical = canonical_workspace_regular_file(workspace, path, label)?;
    #[cfg(target_os = "windows")]
    if canonical
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("exe"))
    {
        return Err(format!("{label} must be a native .exe on Windows"));
    }
    Ok(canonical)
}

fn normalize_sha256(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("expectedSha256 must contain exactly 64 hexadecimal characters".into());
    }
    Ok(normalized)
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|_| "CatDesk executable could not be read for hashing".to_string())?;
    let mut digest = Sha256::new();
    digest.update(bytes);
    Ok(format!("{:x}", digest.finalize()))
}

fn confirmation_token(
    workspace: &Path,
    replacement: &Path,
    hash: &str,
    pid: u32,
    expires_at_unix: u64,
) -> String {
    let nonce = Uuid::new_v4();
    let payload = format!(
        "{}|{}|{}|{}|{}|{}",
        workspace.to_string_lossy(),
        replacement.to_string_lossy(),
        hash,
        pid,
        expires_at_unix,
        nonce
    );
    let mut digest = Sha256::new();
    digest.update(payload.as_bytes());
    format!("reload-{:x}", digest.finalize())
}

fn handoff_dir(workspace: &Path) -> PathBuf {
    workspace.join(".catdesk").join("restart-handoff")
}

fn lifecycle_stop_authorization_path(workspace: &Path, attempt_id: &str) -> PathBuf {
    workspace
        .join(".catdesk")
        .join("lifecycle-stop")
        .join(format!("{attempt_id}.json"))
}

fn reviewed_promotion_control_dir(workspace: &Path) -> PathBuf {
    workspace.join(".catdesk").join("promotion-control")
}

fn reviewed_promotion_preflight_path(workspace: &Path) -> PathBuf {
    reviewed_promotion_control_dir(workspace).join("preflight.json")
}

fn reviewed_promotion_authorization_path(workspace: &Path) -> PathBuf {
    reviewed_promotion_control_dir(workspace).join("authorization.json")
}

fn reviewed_promotion_claim_path(workspace: &Path) -> PathBuf {
    reviewed_promotion_control_dir(workspace).join("claim.json")
}

fn reviewed_promotion_worker_owner_path(workspace: &Path) -> PathBuf {
    reviewed_promotion_control_dir(workspace).join("worker-owner")
}

fn reviewed_promotion_claim_outcome(
    workspace: &Path,
    authorization: &ReviewedPromotionAuthorizationV1,
) -> Result<ReviewedPromotionConfirmationOutcomeV1, String> {
    let result_path = reviewed_promotion_result_path(workspace);
    if result_path.exists() {
        let metadata = fs::symlink_metadata(&result_path)
            .map_err(|_| "reviewed promotion result is unavailable".to_string())?;
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_file()
            || metadata.len() > 4096
        {
            return Err("reviewed promotion result is unavailable".into());
        }
        let result: ReviewedPromotionResultV1 =
            read_bounded_json(&result_path, "reviewed promotion result")?;
        if result.schema_version != 1
            || result.authorization_generation != authorization.generation
            || result.candidate_sha256 != authorization.candidate_sha256
            || result.transaction_id != authorization.transaction_id
            || (!authorization.local_backend_only && result.serving_state.is_some())
            || (authorization.local_backend_only
                && result.state == "PROMOTION_COMPLETED"
                && result.serving_state.is_none())
            || result
                .serving_state
                .as_deref()
                .is_some_and(|state| !matches!(state, "STOPPED_AFTER_BOUNDED_OWNER" | "UNVERIFIED"))
            || (result.serving_state.is_some() && result.state != "PROMOTION_COMPLETED")
            || !matches!(
                result.state.as_str(),
                "PROMOTION_COMPLETED" | "PROMOTION_FAILED_OR_AMBIGUOUS"
            )
        {
            return Err("reviewed promotion result is unavailable".into());
        }
        return Ok(ReviewedPromotionConfirmationOutcomeV1 {
            state: if result.state == "PROMOTION_COMPLETED" {
                "PROMOTION_COMPLETED"
            } else {
                "PROMOTION_FAILED_OR_AMBIGUOUS"
            },
            authorization_generation: authorization.generation,
            candidate_sha256: authorization.candidate_sha256.clone(),
        });
    }
    let owner_path = reviewed_promotion_worker_owner_path(workspace);
    if owner_path.exists() {
        let metadata = fs::symlink_metadata(&owner_path)
            .map_err(|_| "reviewed promotion worker ownership is unavailable".to_string())?;
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_file()
            || metadata.len() != 32
        {
            return Err("reviewed promotion worker ownership is unavailable".into());
        }
        let owner = fs::read_to_string(&owner_path)
            .map_err(|_| "reviewed promotion worker ownership is unavailable".to_string())?;
        if owner != authorization.transaction_id {
            return Err("reviewed promotion worker ownership is unavailable".into());
        }
        return Ok(ReviewedPromotionConfirmationOutcomeV1 {
            state: "WORKER_OWNED_PENDING",
            authorization_generation: authorization.generation,
            candidate_sha256: authorization.candidate_sha256.clone(),
        });
    }
    Ok(ReviewedPromotionConfirmationOutcomeV1 {
        state: "CLAIMED_PENDING_UNPROVEN",
        authorization_generation: authorization.generation,
        candidate_sha256: authorization.candidate_sha256.clone(),
    })
}

fn same_reviewed_promotion_evidence(
    left: &ReviewedPromotionAuthorizationV1,
    right: &ReviewedPromotionAuthorizationV1,
) -> bool {
    left.candidate_relative_path == right.candidate_relative_path
        && left.candidate_sha256 == right.candidate_sha256
        && left.prior_canonical_sha256 == right.prior_canonical_sha256
        && left.promotion_script_sha256 == right.promotion_script_sha256
        && left.trusted_powershell_path == right.trusted_powershell_path
        && left.trusted_powershell_sha256 == right.trusted_powershell_sha256
        && left.review_session_id == right.review_session_id
        && left.review_record_id == right.review_record_id
        && left.review_record_sha256 == right.review_record_sha256
        && left.build_attestation_sha256 == right.build_attestation_sha256
        && left.build_attempt_id == right.build_attempt_id
        && left.snapshot_id == right.snapshot_id
        && left.snapshot_authority_digest == right.snapshot_authority_digest
        && left.snapshot_manifest_digest == right.snapshot_manifest_digest
        && left.build_policy_sha256 == right.build_policy_sha256
        && left.cargo_sha256 == right.cargo_sha256
        && left.rustc_sha256 == right.rustc_sha256
        && left.snapshot_expected == right.snapshot_expected
        && left.local_backend_only == right.local_backend_only
}

fn reviewed_promotion_result_path(workspace: &Path) -> PathBuf {
    reviewed_promotion_control_dir(workspace).join("result.json")
}

fn read_bounded_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    label: &str,
) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|_| format!("{label} is unavailable"))?;
    if bytes.is_empty() || bytes.len() > 8192 {
        return Err(format!("{label} is malformed"));
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("{label} is malformed"))
}

fn canonical_recovery_state_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".catdesk")
        .join("canonical-recovery")
        .join("latest.json")
}

fn write_canonical_recovery_state(
    workspace: &Path,
    state: &CanonicalRecoveryStateV1,
) -> Result<(), String> {
    atomic_json_write(&canonical_recovery_state_path(workspace), state)
}

fn read_canonical_recovery_state(
    workspace: &Path,
) -> Result<Option<CanonicalRecoveryStateV1>, String> {
    let path = canonical_recovery_state_path(workspace);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("canonical recovery result is unavailable".into()),
    };
    if bytes.len() > 4096 {
        return Err("canonical recovery result is malformed".into());
    }
    let state: CanonicalRecoveryStateV1 = serde_json::from_slice(&bytes)
        .map_err(|_| "canonical recovery result is malformed".to_string())?;
    if !(state.schema_version == 1 || state.schema_version == 2)
        || !matches!(
            state.status.as_str(),
            RECOVERY_STATUS_SCHEDULED
                | RECOVERY_STATUS_COMPLETED
                | "RESTORED_KNOWN_GOOD"
                | "INTERRUPTED_TRANSACTION_COMPLETED"
                | RECOVERY_STATUS_LKG_MISSING
                | RECOVERY_STATUS_LKG_DAMAGED
                | RECOVERY_STATUS_TRANSPORT_FAILED
                | RECOVERY_STATUS_AUTHORITY_REQUIRED
        )
        || state.updated_at_unix == 0
        || (state.schema_version == 2
            && (state.generation == 0 || Uuid::parse_str(&state.attempt_id).is_err()))
    {
        return Err("canonical recovery result is malformed".into());
    }
    Ok(Some(state))
}

fn preflight_path(workspace: &Path) -> PathBuf {
    handoff_dir(workspace).join("preflight.json")
}

fn write_preflight(preflight: &ReloadPreflightV1) -> Result<(), String> {
    let target = preflight_path(&preflight.workspace);
    atomic_json_write(&target, preflight)
}

fn read_preflight(workspace: &Path) -> Result<ReloadPreflightV1, String> {
    let bytes = fs::read(preflight_path(workspace))
        .map_err(|_| "CatDesk self-reload preflight is unavailable".to_string())?;
    serde_json::from_slice(&bytes)
        .map_err(|_| "CatDesk self-reload preflight is malformed".to_string())
}

fn write_handoff_state(workspace: &Path, state: &ReloadHandoffStateV1) -> Result<(), String> {
    atomic_json_write(&handoff_dir(workspace).join("latest.json"), state)
}

/// Writes the bounded, non-sensitive terminal state for a worker failure that
/// occurs after a trusted workspace has been established. Callers deliberately
/// supply only fixed status/stage labels and a SHA-256 fingerprint; operating
/// system error strings, paths, command lines, endpoints, and inherited
/// environment values must never cross this handoff boundary.
fn journal_worker_failure(
    workspace: &Path,
    args: &ReloadWorkerArgsV1,
    status: &str,
    stage: &str,
    replacement_sha256: &str,
) -> Result<(), String> {
    write_handoff_state(
        workspace,
        &ReloadHandoffStateV1 {
            schema_version: 1,
            status: status.into(),
            completed_at_unix: now_unix(),
            old_pid: args.old_pid,
            new_pid: None,
            port: args.port,
            replacement_sha256: replacement_sha256.into(),
            stage: stage.into(),
            rollback_attempted: false,
            rollback_ready: false,
        },
    )
}

fn atomic_json_write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "CatDesk reload state path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|_| "CatDesk reload state directory could not be created".to_string())?;
    let temporary = parent.join(format!(".reload-{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| "CatDesk reload state serialization failed".to_string())?;
    fs::write(&temporary, bytes)
        .map_err(|_| "CatDesk reload temporary state could not be written".to_string())?;
    if path.exists() {
        fs::remove_file(path)
            .map_err(|_| "CatDesk reload prior state could not be replaced".to_string())?;
    }
    fs::rename(&temporary, path)
        .map_err(|_| "CatDesk reload state could not be atomically promoted".to_string())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("catdesk-reload-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("workspace");
        root
    }

    #[test]
    fn reviewed_promotion_serving_projection_preserves_legacy_and_refuses_false_health() {
        let root = temp_workspace("promotion-serving-projection");
        let control = root.join(".catdesk/promotion-control");
        fs::create_dir_all(&control).unwrap();
        let mut result = ReviewedPromotionResultV1 {
            schema_version: 1,
            state: "PROMOTION_COMPLETED".into(),
            authorization_generation: 1,
            candidate_sha256: "a".repeat(64),
            transaction_id: "b".repeat(32),
            serving_state: None,
        };
        assert!(
            serde_json::to_value(&result)
                .unwrap()
                .get("servingState")
                .is_none()
        );
        for state in [
            None,
            Some("STOPPED_AFTER_BOUNDED_OWNER"),
            Some("UNVERIFIED"),
        ] {
            result.serving_state = state.map(str::to_owned);
            fs::write(
                control.join("result.json"),
                serde_json::to_vec(&result).unwrap(),
            )
            .unwrap();
            assert_eq!(reviewed_promotion_result(&root).unwrap().unwrap(), result);
        }
        result.serving_state = Some("HEALTHY_PERSISTENT".into());
        fs::write(
            control.join("result.json"),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        assert!(reviewed_promotion_result(&root).is_err());
        result.serving_state = Some("STOPPED_AFTER_BOUNDED_OWNER".into());
        result.state = "PROMOTION_FAILED_OR_AMBIGUOUS".into();
        fs::write(
            control.join("result.json"),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        assert!(reviewed_promotion_result(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    fn sample_release_pair_authorization() -> ReleasePairReconcileAuthorizationV1 {
        ReleasePairReconcileAuthorizationV1 {
            schema_version: 1,
            authorization_id: "a".repeat(32),
            transaction_id: "b".repeat(32),
            expires_at_unix: now_unix().saturating_add(120),
            reviewed_preflight_sha256: "c".repeat(64),
            reviewed_authorization_id: "d".repeat(32),
            candidate_relative_path: "build/candidate.exe".into(),
            candidate_sha256: "e".repeat(64),
            original_binary_sha256: "f".repeat(64),
            original_sidecar_file_sha256: "1".repeat(64),
            original_sidecar_value: "2".repeat(64),
            slot_a_binary_sha256: "2".repeat(64),
            slot_a_sidecar_file_sha256: "3".repeat(64),
            slot_a_manifest_sha256: "4".repeat(64),
            original_pointer_sha256: "5".repeat(64),
            script_sha256: "6".repeat(64),
            recovery_helper_sha256: "7".repeat(64),
            powershell_sha256: "8".repeat(64),
        }
    }

    #[cfg(windows)]
    #[test]
    fn powershell_accepts_actual_rust_promotion_schema_and_rejects_stale_or_changed_proofs() {
        use crate::reviewed_source_snapshot::{
            ReviewedSourceBaselineObservationV1, ReviewedSourceCurrentOutputV1,
        };
        use base64::Engine as _;
        let root = temp_workspace("promotion-wire-schema");
        let expected = ReviewedSourceSnapshotExpectedV1 {
            session_id: "fixture-session".into(),
            project_id: "catdesk".into(),
            approved_contract_hash: "a".repeat(64),
            logical_task_id: "fixture-task".into(),
            completion_artifact_ids: vec!["review/report.md".into()],
            current_outputs: vec![ReviewedSourceCurrentOutputV1 {
                artifact_id: "review/report.md".into(),
                sha256: "b".repeat(64),
            }],
            baseline_observations: vec![ReviewedSourceBaselineObservationV1 {
                artifact_id: "review/report.md".into(),
                state: "ABSENT".into(),
                sha256: None,
            }],
        };
        let mut authorization = ReviewedPromotionAuthorizationV1 {
            schema_version: 1,
            generation: 1,
            authorization_id: "c".repeat(32),
            expires_at_unix: now_unix() + 120,
            candidate_relative_path: "build/candidate.exe".into(),
            candidate_sha256: "d".repeat(64),
            prior_canonical_sha256: "e".repeat(64),
            promotion_script_sha256: "f".repeat(64),
            trusted_powershell_path: "fixture-interpreter".into(),
            trusted_powershell_sha256: "1".repeat(64),
            review_session_id: "fixture-session".into(),
            review_record_id: "fixture-review".into(),
            review_record_sha256: "2".repeat(64),
            transaction_id: "3".repeat(32),
            claim_id: "4".repeat(32),
            build_attestation_sha256: "5".repeat(64),
            build_attempt_id: "6".repeat(32),
            snapshot_id: "7".repeat(64),
            snapshot_authority_digest: "8".repeat(64),
            snapshot_manifest_digest: "9".repeat(64),
            build_policy_sha256: "a".repeat(64),
            cargo_sha256: "b".repeat(64),
            rustc_sha256: "c".repeat(64),
            snapshot_expected: expected,
            local_backend_only: false,
        };
        let legacy_wire = serde_json::to_value(&authorization).unwrap();
        assert!(legacy_wire.get("localBackendOnly").is_none());
        let mut local_profile = authorization.clone();
        local_profile.local_backend_only = true;
        assert_eq!(
            serde_json::to_value(&local_profile).unwrap()["localBackendOnly"],
            true
        );
        assert!(!same_reviewed_promotion_evidence(
            &authorization,
            &local_profile
        ));
        let control = root.join(".catdesk/promotion-control");
        fs::create_dir_all(&control).unwrap();
        fs::create_dir(root.join("build")).unwrap();
        let candidate = root.join("build/candidate.exe");
        fs::write(&candidate, b"inert wire fixture candidate").unwrap();
        authorization.candidate_sha256 = sha256_file(&candidate).unwrap();
        let input = control.join("authorization.json");
        fs::write(
            root.join("recovery.ps1"),
            include_bytes!("../scripts/catdesk-release-recovery.ps1"),
        )
        .unwrap();
        let source = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
            "../scripts/promote-reviewed-catdesk-build.ps1"
        ));
        let driver = root.join("validate-fixture.ps1");
        fs::write(&driver, format!(r#"
param([string]$InputPath,[string]$Mode)
$ErrorActionPreference='Stop'
$library=Join-Path $PSScriptRoot 'consumer-library.ps1'
if($Mode -eq 'prepare-library'){{
 $source=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{source}'))
 $tokens=$null;$errors=$null
 $ast=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors)
 if($errors.Count){{throw 'source parse failed'}}
 $defs=@($ast.FindAll({{param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst]}},$true))
 $bodies=($defs|ForEach-Object{{$_.Extent.Text}}) -join "`n"
 [IO.File]::WriteAllText($library,$bodies,[Text.UTF8Encoding]::new($false))
 return
}}
. $library
. (Join-Path $PSScriptRoot 'recovery.ps1')
$script:PromotionSeams=@{{}}
function Get-FileHash {{ throw 'optional hashing cmdlet must not be required' }}
$record=Get-Content -LiteralPath $InputPath -Raw|ConvertFrom-Json
$candidate=Join-Path $PSScriptRoot 'build/candidate.exe'
$expectedHash=Get-PromotionHash $candidate
switch($Mode){{
 'local' {{ $script:BackendOnly=$true }}
 'flag-without-profile' {{ $script:BackendOnly=$true }}
 'malformed-current-hash' {{ $record.snapshotExpected.approved_contract_hash='fnv1a64:123' }}
 'malformed-profile' {{ $script:BackendOnly=$true; $record.localBackendOnly='true' }}
 'legacy' {{ foreach($name in @('buildAttestationSha256','buildAttemptId','snapshotId','snapshotAuthorityDigest','snapshotManifestDigest','buildPolicySha256','cargoSha256','rustcSha256','snapshotExpected')){{$record.PSObject.Properties.Remove($name)}} }}
 'bad-hash' {{ $record.cargoSha256='unknown' }}
 'wrong-session' {{ $record.snapshotExpected.session_id='other-session' }}
 'wrong-output' {{ $record.snapshotExpected.current_outputs[0].artifactId='other/report.md' }}
 'wrong-path' {{ $record.candidateRelativePath='other/candidate.exe' }}
 'extra' {{ $record|Add-Member -NotePropertyName callerOverride -NotePropertyValue $true }}
}}
$record|ConvertTo-Json -Depth 12 -Compress|Set-Content -LiteralPath $InputPath -Encoding utf8
$actualRelative=Get-WorkspaceRelativePath $PSScriptRoot $candidate 'wire fixture candidate'
Write-Output ('WIRE_NESTED_PATH_MATCH='+($record.candidateRelativePath -ceq $actualRelative))
$accepted=Get-ProtectedPromotionAuthorization $PSScriptRoot ('c'*32) ('3'*32) $candidate $expectedHash ('e'*64)
if($accepted.candidateRelativePath -cne 'build/candidate.exe'){{throw 'wrong wire candidate accepted'}}
'ACTUAL_RUST_AUTHORIZATION_CONSUMED'
"#)).unwrap();
        let powershell = crate::operator_facade::trusted_windows_powershell(&root).unwrap();
        let prepared = Command::new(&powershell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&driver)
            .arg(&input)
            .arg("prepare-library")
            .output()
            .unwrap();
        assert!(
            prepared.status.success(),
            "fixture library: {}",
            String::from_utf8_lossy(&prepared.stderr)
        );
        authorization.promotion_script_sha256 =
            sha256_file(&root.join("consumer-library.ps1")).unwrap();
        fs::write(
            control.join("claim.json"),
            serde_json::to_vec(&ReviewedPromotionClaimV1 {
                schema_version: 1,
                authorization_id: authorization.authorization_id.clone(),
                claim_id: authorization.claim_id.clone(),
                transaction_id: authorization.transaction_id.clone(),
                candidate_relative_path: authorization.candidate_relative_path.clone(),
                state: "CLAIMED_PENDING".into(),
            })
            .unwrap(),
        )
        .unwrap();
        for mode in [
            "valid",
            "current-hash",
            "malformed-current-hash",
            "local",
            "local-without-flag",
            "flag-without-profile",
            "malformed-profile",
            "legacy",
            "bad-hash",
            "wrong-session",
            "wrong-output",
            "wrong-path",
            "extra",
        ] {
            let mut wire = authorization.clone();
            if matches!(mode, "current-hash" | "malformed-current-hash") {
                wire.snapshot_expected.approved_contract_hash = "fnv1a64:0123456789abcdef".into();
            }
            if matches!(mode, "local" | "local-without-flag" | "malformed-profile") {
                wire.local_backend_only = true;
            }
            fs::write(&input, serde_json::to_vec(&wire).unwrap()).unwrap();
            let output = Command::new(&powershell)
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(&driver)
                .arg(&input)
                .arg(mode)
                .output()
                .unwrap();
            assert_eq!(
                output.status.success(),
                matches!(mode, "valid" | "current-hash" | "local"),
                "wire mode {mode}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let resolved = root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn release_pair_worker_arguments_are_closed() {
        let valid = vec![
            RELEASE_PAIR_RECONCILE_WORKER_FLAG.into(),
            "--workspace".into(),
            "C:/work/catdesk".into(),
            "--authorization".into(),
            "a".repeat(32),
            "--transaction".into(),
            "b".repeat(32),
        ];
        assert_eq!(
            parse_release_pair_reconcile_worker_args(&valid).unwrap(),
            Some(ReleasePairReconcileWorkerArgsV1 {
                workspace: PathBuf::from("C:/work/catdesk"),
                authorization_id: "a".repeat(32),
                transaction_id: "b".repeat(32),
            })
        );
        let mut extra = valid.clone();
        extra.push("--owner-pid".into());
        assert!(parse_release_pair_reconcile_worker_args(&extra).is_err());
    }

    #[test]
    fn release_pair_repeat_confirm_reads_exact_result_without_replaying_mutation() {
        let root = temp_workspace("release-pair-confirm-retry");
        let authorization = sample_release_pair_authorization();
        let preflight = ReleasePairReconcilePreflightV1 {
            schema_version: 1,
            confirmation_token: "token-for-exact-retry".into(),
            expires_at_unix: authorization.expires_at_unix,
            authorization: authorization.clone(),
        };
        let claim = ReleasePairReconcileClaimV1 {
            schema_version: 1,
            authorization_id: authorization.authorization_id.clone(),
            transaction_id: authorization.transaction_id.clone(),
            state: "CLAIMED_PENDING".into(),
        };
        atomic_json_write(&release_pair_preflight_path(&root), &preflight).unwrap();
        atomic_json_write(&release_pair_authorization_path(&root), &authorization).unwrap();
        atomic_json_write(&release_pair_claim_path(&root), &claim).unwrap();
        atomic_json_write(
            &release_pair_result_path(&root),
            &ReleasePairReconcileResultV1 {
                schema_version: 1,
                state: "RECONCILED_CANONICAL_READY".into(),
                transaction_id: authorization.transaction_id.clone(),
                candidate_sha256: authorization.candidate_sha256.clone(),
                owner_pid: Some(42),
            },
        )
        .unwrap();
        let result = confirm_release_pair_reconcile(&root, &preflight.confirmation_token).unwrap();
        assert_eq!(result.state, "RECONCILED_CANONICAL_READY");
        assert!(!release_pair_worker_owner_path(&root).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn release_pair_result_rejects_wrong_claimed_transaction() {
        let root = temp_workspace("release-pair-result-drift");
        let authorization = sample_release_pair_authorization();
        let claim = ReleasePairReconcileClaimV1 {
            schema_version: 1,
            authorization_id: authorization.authorization_id.clone(),
            transaction_id: authorization.transaction_id.clone(),
            state: "CLAIMED_PENDING".into(),
        };
        atomic_json_write(&release_pair_authorization_path(&root), &authorization).unwrap();
        atomic_json_write(&release_pair_claim_path(&root), &claim).unwrap();
        atomic_json_write(
            &release_pair_result_path(&root),
            &ReleasePairReconcileResultV1 {
                schema_version: 1,
                state: "RECONCILED_CANONICAL_READY".into(),
                transaction_id: "9".repeat(32),
                candidate_sha256: authorization.candidate_sha256.clone(),
                owner_pid: Some(42),
            },
        )
        .unwrap();
        assert!(release_pair_reconcile_result(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn release_pair_claim_without_result_is_explicitly_unproven() {
        let root = temp_workspace("release-pair-claimed-unknown");
        let authorization = sample_release_pair_authorization();
        let preflight = ReleasePairReconcilePreflightV1 {
            schema_version: 1,
            confirmation_token: "known-confirmation-token".into(),
            expires_at_unix: authorization.expires_at_unix,
            authorization: authorization.clone(),
        };
        let claim = ReleasePairReconcileClaimV1 {
            schema_version: 1,
            authorization_id: authorization.authorization_id.clone(),
            transaction_id: authorization.transaction_id.clone(),
            state: "CLAIMED_PENDING".into(),
        };
        atomic_json_write(&release_pair_preflight_path(&root), &preflight).unwrap();
        atomic_json_write(&release_pair_authorization_path(&root), &authorization).unwrap();
        atomic_json_write(&release_pair_claim_path(&root), &claim).unwrap();
        assert!(release_pair_reconcile_claimed(&root).unwrap());
        assert!(release_pair_reconcile_result(&root).unwrap().is_none());
        assert_eq!(
            confirm_release_pair_reconcile(&root, &preflight.confirmation_token)
                .unwrap()
                .state,
            "CLAIMED_OUTCOME_UNPROVEN_OPERATOR_ATTENTION"
        );
        assert!(!release_pair_worker_owner_path(&root).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn helper_args_are_explicit_and_bounded() {
        let args = vec![
            RELOAD_WORKER_FLAG.into(),
            "--old-pid".into(),
            "42".into(),
            "--mcp-port".into(),
            "3200".into(),
            "--workspace".into(),
            "C:/work/catdesk".into(),
            "--replacement".into(),
            "C:/work/catdesk/new.exe".into(),
            "--rollback".into(),
            "C:/work/catdesk/old.exe".into(),
            "--expected-sha256".into(),
            "a".repeat(64),
        ];
        let parsed = parse_reload_worker_args(&args)
            .expect("parse")
            .expect("worker args");
        assert_eq!(parsed.old_pid, 42);
        assert_eq!(parsed.port, 3200);
        assert_eq!(parsed.expected_sha256, "a".repeat(64));
        assert!(parse_reload_worker_args(&[RELOAD_WORKER_FLAG.into()]).is_err());
    }

    #[test]
    fn lifecycle_stop_helper_args_are_exact_and_do_not_accept_pid_or_extra_tokens() {
        let attempt_id = Uuid::new_v4().to_string();
        let args = vec![
            LIFECYCLE_STOP_WORKER_FLAG.into(),
            "--workspace".into(),
            "C:/work/catdesk".into(),
            "--attempt".into(),
            attempt_id.clone(),
        ];
        assert_eq!(
            parse_lifecycle_stop_worker_args(&args),
            Ok(Some(LifecycleStopWorkerArgsV1 {
                workspace: PathBuf::from("C:/work/catdesk"),
                attempt_id,
            }))
        );
        for invalid in [
            vec![LIFECYCLE_STOP_WORKER_FLAG.into()],
            vec![
                LIFECYCLE_STOP_WORKER_FLAG.into(),
                "--workspace".into(),
                "C:/work/catdesk".into(),
                "--attempt".into(),
                "not-a-uuid".into(),
            ],
            vec![
                LIFECYCLE_STOP_WORKER_FLAG.into(),
                "--workspace".into(),
                "C:/work/catdesk".into(),
                "--pid".into(),
                "42".into(),
            ],
            vec![
                LIFECYCLE_STOP_WORKER_FLAG.into(),
                "--workspace".into(),
                "C:/work/catdesk".into(),
                "--attempt".into(),
                Uuid::new_v4().to_string(),
                "extra".into(),
            ],
        ] {
            assert!(parse_lifecycle_stop_worker_args(&invalid).is_err());
        }
    }

    #[test]
    fn lifecycle_stop_worker_requires_one_shot_internal_authority() {
        let root = temp_workspace("lifecycle-stop-authority");
        let canonical_root = root.canonicalize().expect("root");
        let attempt_id = Uuid::new_v4().to_string();
        let args = LifecycleStopWorkerArgsV1 {
            workspace: canonical_root.clone(),
            attempt_id: attempt_id.clone(),
        };
        assert!(run_lifecycle_stop_worker(args).is_err());
        assert!(!lifecycle_stop_authorization_path(&canonical_root, &attempt_id).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lifecycle_stop_worker_source_has_no_shell_or_tunnel_execution_path() {
        let source = include_str!("daemon_reload.rs");
        let worker = source
            .split("pub fn run_lifecycle_stop_worker")
            .nth(1)
            .and_then(|tail| tail.split("pub fn spawn_lifecycle_stop_helper").next())
            .expect("lifecycle worker source");
        for forbidden in ["powershell.exe", "cmd.exe", "catdesk.ps1", "Command::new"] {
            assert!(
                !worker.to_ascii_lowercase().contains(forbidden),
                "lifecycle stop worker must not execute or mutate {forbidden}"
            );
        }
    }

    #[test]
    fn legacy_canonical_recovery_is_closed_before_it_can_schedule_or_execute() {
        let parsed = parse_canonical_recovery_worker_args(&[
            CANONICAL_RECOVERY_WORKER_FLAG.into(),
            "--workspace".into(),
            "C:\\workspace".into(),
            "--attempt".into(),
            Uuid::new_v4().to_string(),
        ])
        .expect("parse")
        .expect("worker");
        assert_eq!(parsed.workspace, PathBuf::from("C:\\workspace"));
        for invalid in [
            vec![CANONICAL_RECOVERY_WORKER_FLAG.into()],
            vec![
                CANONICAL_RECOVERY_WORKER_FLAG.into(),
                "--workspace".into(),
                "C:\\workspace".into(),
                "--command".into(),
                "whoami".into(),
            ],
            vec![
                CANONICAL_RECOVERY_WORKER_FLAG.into(),
                "--script".into(),
                "x.ps1".into(),
            ],
        ] {
            assert!(parse_canonical_recovery_worker_args(&invalid).is_err());
        }
        let root = temp_workspace("closed-legacy-recovery");
        assert!(schedule_canonical_recovery(&root).is_err());
        assert!(
            !canonical_recovery_state_path(&root).exists(),
            "a rejected legacy request may not create a recovery attempt"
        );

        // A detached worker that an older binary already scheduled is retired
        // without invoking the versioned script or touching a daemon/tunnel.
        let attempt_id = Uuid::new_v4().to_string();
        let scheduled = CanonicalRecoveryStateV1 {
            schema_version: 2,
            status: RECOVERY_STATUS_SCHEDULED.into(),
            updated_at_unix: 1,
            generation: 4,
            attempt_id: attempt_id.clone(),
        };
        write_canonical_recovery_state(&root, &scheduled).expect("scheduled legacy fixture");
        assert!(
            run_canonical_recovery_worker(CanonicalRecoveryWorkerArgsV1 {
                workspace: root.clone(),
                attempt_id: attempt_id.clone(),
            })
            .is_err()
        );
        assert_eq!(
            read_canonical_recovery_state(&root)
                .expect("read retired state")
                .expect("retired state")
                .status,
            RECOVERY_STATUS_AUTHORITY_REQUIRED
        );
        let source = include_str!("daemon_reload.rs");
        let legacy_worker = source
            .split("pub fn run_canonical_recovery_worker")
            .nth(1)
            .and_then(|tail| tail.split("pub fn run_lifecycle_stop_worker").next())
            .expect("legacy worker source");
        for forbidden in [
            "canonical_release_recovery_invocation",
            "run_fixed_powershell_invocation_blocking",
            "Command::new",
            "--catdesk-daemon",
        ] {
            assert!(
                !legacy_worker.contains(forbidden),
                "retired worker must not recover through legacy authority: {forbidden}"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_file_validation_rejects_escape_and_hash_mismatch() {
        let root = temp_workspace("containment");
        let inside = root.join("candidate.exe");
        let script = root.join("catdesk.ps1");
        fs::write(&inside, b"candidate").expect("candidate");
        fs::write(&script, b"Write-Output test").expect("script");
        let outside = std::env::temp_dir().join(format!("outside-{}.exe", Uuid::new_v4()));
        fs::write(&outside, b"outside").expect("outside");
        let canonical_root = root.canonicalize().expect("root");
        assert!(canonical_workspace_file(&canonical_root, &inside, "candidate").is_ok());
        assert!(canonical_workspace_file(&canonical_root, &outside, "candidate").is_err());
        assert!(canonical_workspace_script(&canonical_root, &script, "facade").is_ok());
        assert!(canonical_workspace_script(&canonical_root, &inside, "facade").is_err());
        #[cfg(target_os = "windows")]
        assert!(canonical_workspace_file(&canonical_root, &script, "candidate").is_err());
        let actual = sha256_file(&inside).expect("hash");
        assert_ne!(actual, "0".repeat(64));
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn sha256_validation_is_strict() {
        assert_eq!(normalize_sha256(&"A".repeat(64)), Ok("a".repeat(64)));
        assert!(normalize_sha256("abc").is_err());
        assert!(normalize_sha256(&"z".repeat(64)).is_err());
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn mcp_listener_handle_is_made_non_inheritable() {
        use std::ffi::c_void;
        use std::os::windows::io::AsRawSocket;

        const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind disposable listener");
        make_listener_non_inheritable(&listener).expect("clear listener inheritance");
        let mut flags = 0u32;
        let handle = listener.as_raw_socket() as usize as *mut c_void;
        assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn exact_pid_guard_recognizes_current_executable_and_rejects_mismatch() {
        let current = std::env::current_exe()
            .expect("current exe")
            .canonicalize()
            .expect("canonical current exe");
        assert_eq!(
            windows_process_is_exact_and_alive(std::process::id(), &current),
            Ok(true)
        );

        let root = temp_workspace("pid-identity");
        let mismatch = root.join("different.exe");
        fs::write(&mismatch, b"not the running executable").expect("mismatch file");
        let mismatch = mismatch.canonicalize().expect("canonical mismatch");
        let error = windows_process_is_exact_and_alive(std::process::id(), &mismatch)
            .expect_err("mismatched executable must fail closed");
        assert!(error.contains("exact rollback CatDesk executable"));
    }

    #[test]
    fn handoff_state_is_atomic_and_contains_no_endpoint_or_credentials() {
        let root = temp_workspace("state");
        let canonical_root = root.canonicalize().expect("root");
        let state = ReloadHandoffStateV1 {
            schema_version: 1,
            status: "TEST".into(),
            completed_at_unix: 1,
            old_pid: 1,
            new_pid: Some(2),
            port: 3200,
            replacement_sha256: "a".repeat(64),
            stage: "test".into(),
            rollback_attempted: false,
            rollback_ready: false,
        };
        write_handoff_state(&canonical_root, &state).expect("write");
        let text = fs::read_to_string(handoff_dir(&canonical_root).join("latest.json"))
            .expect("state text");
        assert!(text.contains("replacementSha256"));
        for forbidden in [
            "token",
            "password",
            "authorization",
            "tunnelUrl",
            "endpoint",
        ] {
            assert!(
                !text
                    .to_ascii_lowercase()
                    .contains(&forbidden.to_ascii_lowercase())
            );
        }
    }

    #[test]
    fn early_worker_validation_failure_always_advances_bounded_handoff_state() {
        let root = temp_workspace("early-handoff");
        let canonical_root = root.canonicalize().expect("root");
        let replacement = canonical_root.join("replacement.exe");
        let rollback = canonical_root.join("rollback.exe");
        let args = ReloadWorkerArgsV1 {
            old_pid: 42,
            port: 32123,
            workspace: canonical_root.clone(),
            replacement_path: replacement.clone(),
            rollback_path: rollback.clone(),
            expected_sha256: "a".repeat(64),
        };

        let read_state = || -> ReloadHandoffStateV1 {
            serde_json::from_slice(
                &fs::read(handoff_dir(&canonical_root).join("latest.json")).expect("handoff state"),
            )
            .expect("bounded handoff JSON")
        };

        assert!(run_reload_worker(args.clone()).is_err());
        let state = read_state();
        assert_eq!(state.status, "FAILED_REPLACEMENT_VALIDATION");
        assert_eq!(state.stage, "validate-replacement-path");

        fs::write(&replacement, b"replacement").expect("replacement fixture");
        assert!(run_reload_worker(args.clone()).is_err());
        let state = read_state();
        assert_eq!(state.status, "FAILED_ROLLBACK_VALIDATION");
        assert_eq!(state.stage, "validate-rollback-path");

        fs::write(&rollback, b"rollback").expect("rollback fixture");
        assert!(run_reload_worker(args).is_err());
        let state = read_state();
        assert_eq!(state.status, "FAILED_REPLACEMENT_HASH_MISMATCH");
        assert_eq!(state.stage, "validate-replacement-hash");
        assert_ne!(state.replacement_sha256, "a".repeat(64));
        assert!(!state.rollback_attempted);
        assert!(!state.rollback_ready);
    }

    #[test]
    fn reviewed_promotion_worker_arguments_are_closed() {
        let parsed = parse_reviewed_promotion_worker_args(&[
            REVIEWED_PROMOTION_WORKER_FLAG.into(),
            "--workspace".into(),
            "C:\\safe".into(),
            "--candidate".into(),
            "reviewed\\candidate.exe".into(),
            "--authorization".into(),
            "0123456789abcdef0123456789abcdef".into(),
            "--transaction".into(),
            "fedcba9876543210fedcba9876543210".into(),
        ])
        .expect("closed arguments")
        .expect("worker");
        assert_eq!(parsed.candidate_relative_path, "reviewed\\candidate.exe");
        assert_eq!(parsed.transaction_id, "fedcba9876543210fedcba9876543210");
        assert!(
            parse_reviewed_promotion_worker_args(&[
                REVIEWED_PROMOTION_WORKER_FLAG.into(),
                "--workspace".into(),
                "C:\\safe".into(),
                "--candidate".into(),
                "reviewed\\candidate.exe".into(),
                "--authorization".into(),
                "0123456789abcdef0123456789abcdef".into(),
                "--script".into(),
                "evil.ps1".into(),
            ])
            .is_err()
        );
    }

    #[test]
    fn reviewed_reload_preflight_binding_is_persisted_and_cannot_be_downgraded() {
        let root = temp_workspace("reviewed-binding");
        let canonical_root = root.canonicalize().expect("canonical workspace");
        let replacement = canonical_root.join("candidate.exe");
        fs::write(&replacement, b"reviewed-candidate").expect("candidate");
        let replacement = replacement.canonicalize().expect("canonical candidate");
        let expected_sha256 = sha256_file(&replacement).expect("candidate hash");
        let binding = ReloadReviewBindingV1 {
            schema_version: 1,
            review_record_id: "review-record".into(),
            review_session_id: "review-session".into(),
            request_sha256: "a".repeat(64),
            authority_sha256: "b".repeat(64),
        };
        let preflight = ReloadPreflightV1 {
            schema_version: 1,
            confirmation_token: "bounded-token".into(),
            created_at_unix: now_unix(),
            expires_at_unix: now_unix().saturating_add(600),
            old_pid: std::process::id(),
            port: 3200,
            workspace: canonical_root.clone(),
            replacement_path: replacement.clone(),
            rollback_path: replacement.clone(),
            expected_sha256: expected_sha256.clone(),
            review_binding: Some(binding.clone()),
        };
        write_preflight(&preflight).expect("persist reviewed preflight");
        assert_eq!(
            read_reload_preflight(&canonical_root)
                .expect("read reviewed preflight")
                .review_binding,
            Some(binding.clone())
        );

        let mut changed = binding.clone();
        changed.authority_sha256 = "c".repeat(64);
        assert!(
            execute_reload_with_review_binding(
                &canonical_root,
                &replacement,
                &expected_sha256,
                "bounded-token",
                3200,
                Some(&changed),
            )
            .expect_err("changed review authority must fail before helper launch")
            .contains("does not match this daemon/build/review")
        );
        assert!(
            execute_reload_with_review_binding(
                &canonical_root,
                &replacement,
                &expected_sha256,
                "bounded-token",
                3200,
                None,
            )
            .expect_err("reviewed preflight cannot be consumed unreviewed")
            .contains("does not match this daemon/build/review")
        );
        let _ = fs::remove_dir_all(root);
    }
}
