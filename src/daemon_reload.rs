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

pub const RELOAD_WORKER_FLAG: &str = "--catdesk-reload-worker";
pub const DAEMON_MODE_FLAG: &str = "--catdesk-daemon";
pub const LIFECYCLE_STOP_WORKER_FLAG: &str = "--catdesk-lifecycle-stop-worker";
pub const CANONICAL_RECOVERY_WORKER_FLAG: &str = "--catdesk-canonical-recovery-worker";
pub const REVIEWED_PROMOTION_WORKER_FLAG: &str = "--catdesk-reviewed-promotion-worker";
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

pub fn prepare_reviewed_promotion(
    workspace: &Path,
    candidate: &Path,
    expected_sha256: &str,
    review_session_id: &str,
    review_record_id: &str,
    review_record_sha256: &str,
    snapshot_expected: &ReviewedSourceSnapshotExpectedV1,
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
    let invocation = crate::operator_facade::reviewed_promotion_invocation(
        &workspace,
        &workspace.join(&args.candidate_relative_path),
        &authorization.authorization_id,
        &authorization.transaction_id,
    )?;
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
    atomic_json_write(
        &reviewed_promotion_result_path(&workspace),
        &ReviewedPromotionResultV1 {
            schema_version: 1,
            state: state.into(),
            authorization_generation: authorization.generation,
            candidate_sha256: authorization.candidate_sha256,
            transaction_id: authorization.transaction_id,
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
    {
        return Err("reviewed promotion result is malformed".into());
    }
    Ok(Some(result))
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
