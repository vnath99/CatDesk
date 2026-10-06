//! CatDesk-owned in-process registry for autonomous Codex controller ticks.
//!
//! The executable path is operator-local environment configuration. MCP can
//! name a persisted session but cannot select the executable, sandbox, or
//! authentication source.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tokio::time::{Duration, interval};

use crate::stable_wake_owner::{
    FixedScriptBrowserAdapter, OwnerOutcome, dispatch_if_rust_selected,
};
use crate::stable_wake_owner_mode::{WakeOwnerMode, selected_owner};

use super::autonomous_contract::AutonomousPolicyEngineV1;
use super::autonomous_controller::{
    AutonomousControllerOutcomeV1, AutonomousControllerV1, RestartTaskReconciliationV1,
    reconcile_restart_task_state,
};
use super::autonomy_projects::{
    AutonomousProjectRegistryStoreV1, AutonomousProjectV1, ProjectRegistrationEvidenceV1,
};
use super::autonomy_state::{
    AutonomousProviderRouteV1, AutonomousReviewInboxRecordV1, AutonomousSessionStateV1,
    AutonomousStateStoreV1, AutonomousWakeModeV1, CodexContinuityEvidenceV1,
};
use super::autonomy_verifier::ContractVerifierV1;
use super::codex_app_server::{
    CodexAppServerLaunchConfigV1, CodexAppServerReadClientV1, CodexAppServerTransportV1,
    CodexContinuityTurnV1, CodexThreadResolutionV1,
};
use super::codex_cli::{CodexCliConfigV1, CodexCliProviderV1, CodexCliSandboxV1};
use super::runtime::{FakeProvider, RuntimeError};
use super::worker_provider::{
    CodexQwenWorkerProviderV1, FakeWorkerProviderV1, OllamaWorkerProviderV1,
};

type LiveController = AutonomousControllerV1<
    CodexQwenWorkerProviderV1<CodexCliProviderV1, OllamaWorkerProviderV1>,
    ContractVerifierV1,
>;
type RuntimeRegistry = BTreeMap<String, LiveController>;

static RUNTIME_REGISTRY: OnceLock<Arc<Mutex<RuntimeRegistry>>> = OnceLock::new();
// Each live reviewer loop owns a generation.  A durable control-plane action
// can bump that generation before it ticks the controller; an older loop that
// was just about to exit for WAITING_FOR_CHATGPT then keeps ownership rather
// than clearing the wakeup registration underneath the re-arm.
static REVIEWER_WAKEUPS: OnceLock<Arc<Mutex<BTreeMap<String, u64>>>> = OnceLock::new();
static WAKE_DISPATCHES: OnceLock<StdMutex<BTreeSet<String>>> = OnceLock::new();
static ACTIVE_WAKE_DISPATCHES: OnceLock<StdMutex<BTreeSet<String>>> = OnceLock::new();
// Startup/reload wake owners are deliberately independent of reviewer-loop
// ownership. A durable WAITING_FOR_CHATGPT handoff can survive process exit,
// while REVIEWER_WAKEUPS is in-memory only. Keeping a tiny independent set
// lets startup schedule exactly one out-of-band redispatch without blocking a
// later accepted reply from installing the normal reviewer loop.
static STARTUP_WAKE_REHYDRATIONS: OnceLock<StdMutex<BTreeSet<String>>> = OnceLock::new();
const REVIEWER_WAKEUP_SECONDS: u64 = 30;
const STARTUP_WAKE_REHYDRATION_DELAY_SECONDS: u64 = 8;
const WAKE_SHORT_RETRY_LIMIT: usize = 3;
const WAKE_SHORT_RETRY_DELAY: Duration = Duration::from_secs(2);
const WAKE_CHAT_BUSY_RETRY_SECONDS: u64 = 300;
const WAKE_CHAT_BUSY_RETRY_LIMIT: u32 = 12;
const WAKE_RETRY_SCHEDULE_SCHEMA_VERSION: u64 = 1;
const WAKE_RETRY_SCHEDULE_MAX_BYTES: u64 = 4096;
pub const PREFERRED_CATDESK_THREAD_QUERY_V1: &str = "Integrate Codex MCP for ChatGPT";

/// Host-owned hourly deadman primitive for existing Codex Goals. The MCP
/// caller supplies only display titles; executable/auth/thread ids/prompts
/// remain host-owned. It fails closed on unavailable usage telemetry,
/// ambiguity, active writers, and non-paused terminal Goal states.
pub fn host_resume_codex_goals_by_title(
    workspace: &Path,
    titles: &[String],
    observed_at_unix: u64,
) -> Result<serde_json::Value, RuntimeError> {
    if titles.is_empty() || titles.len() > 8 {
        return Err(RuntimeError::Validation(
            "Codex Goal resume requires one to eight titles".into(),
        ));
    }
    let mut normalized = BTreeSet::new();
    for title in titles {
        let clean = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if clean.is_empty() || clean.len() > 512 {
            return Err(RuntimeError::Validation(
                "Codex Goal resume title is empty or oversized".into(),
            ));
        }
        if !normalized.insert(clean.to_ascii_lowercase()) {
            return Err(RuntimeError::Validation(
                "Codex Goal resume titles must be unique".into(),
            ));
        }
    }

    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("Codex Goal resume workspace canonicalization failed".into())
    })?;
    let mut transport = host_app_server_launch_config(&workspace)?.spawn_stdio_transport()?;
    transport.initialize()?;
    let telemetry =
        CodexAppServerReadClientV1::refresh_telemetry(&mut transport, observed_at_unix, None)?;
    if telemetry.source != "codex-app-server/account-rateLimits-read" {
        return Ok(serde_json::json!({
            "status": "USAGE_UNKNOWN",
            "codexEligibleAfterUnix": telemetry.codex_eligible_after_unix,
            "results": titles.iter().map(|title| serde_json::json!({
                "title": title,
                "outcome": "USAGE_UNKNOWN"
            })).collect::<Vec<_>>()
        }));
    }
    if telemetry.reached_limit {
        return Ok(serde_json::json!({
            "status": "USAGE_LIMITED",
            "codexEligibleAfterUnix": telemetry.codex_eligible_after_unix,
            "results": titles.iter().map(|title| serde_json::json!({
                "title": title,
                "outcome": "USAGE_LIMITED"
            })).collect::<Vec<_>>()
        }));
    }

    let mut results = Vec::with_capacity(titles.len());
    for title in titles {
        let matches =
            CodexAppServerReadClientV1::discover_exact_title_threads(&mut transport, title)?;
        let outcome = match matches.as_slice() {
            [] => "NOT_FOUND".to_string(),
            [_first, _second, ..] => "AMBIGUOUS".to_string(),
            [listed] if listed.concurrently_owned => "BUSY".to_string(),
            [listed] => {
                let reread = transport.request(
                    "thread/read",
                    serde_json::json!({"threadId": listed.thread_id}),
                )?;
                let reread = super::codex_app_server::parse_thread_metadata(&reread)?;
                if reread.thread_id != listed.thread_id || reread.concurrently_owned {
                    "BUSY".to_string()
                } else {
                    let resumed = CodexAppServerReadClientV1::resume_exact(
                        &mut transport,
                        &CodexThreadResolutionV1::Exact(reread),
                    )?;
                    let authoritative =
                        super::codex_app_server::parse_resumed_thread_metadata(&resumed)?;
                    if authoritative.thread_id != listed.thread_id
                        || authoritative.concurrently_owned
                    {
                        "BUSY".to_string()
                    } else {
                        match CodexAppServerReadClientV1::goal_get(
                            &mut transport,
                            &listed.thread_id,
                        )? {
                            None => "NO_GOAL".to_string(),
                            Some(goal) if goal.status == "active" => "ALREADY_ACTIVE".to_string(),
                            Some(goal) if goal.status == "paused" => {
                                let resumed_goal = CodexAppServerReadClientV1::goal_resume_paused(
                                    &mut transport,
                                    &listed.thread_id,
                                )?;
                                if resumed_goal.status != "active" {
                                    return Err(RuntimeError::Validation(
                                        "Codex Goal resume did not return active state".into(),
                                    ));
                                }
                                "RESUMED".to_string()
                            }
                            Some(goal) => format!(
                                "GOAL_{}",
                                goal.status
                                    .chars()
                                    .flat_map(char::to_uppercase)
                                    .collect::<String>()
                            ),
                        }
                    }
                }
            }
        };
        results.push(serde_json::json!({"title": title, "outcome": outcome}));
    }
    Ok(serde_json::json!({
        "status": "AVAILABLE",
        "codexEligibleAfterUnix": telemetry.codex_eligible_after_unix,
        "results": results
    }))
}

/// Bounded, non-secret metadata surfaced for operator selection. The raw
/// thread id remains inside the host/registry confirmation record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostThreadAdoptionCandidateV1 {
    pub title: Option<String>,
    pub evidence: ProjectRegistrationEvidenceV1,
}

/// Binds a persisted session only after the operator-provided app-server
/// transport returns one exact thread in this workspace. No transport is
/// discovered here: desktop authentication and connection ownership remain
/// operator-local. Ambiguity, an active writer, a different existing binding,
/// or an in-flight session fail closed.
pub fn bind_codex_app_server_thread<T: CodexAppServerTransportV1>(
    workspace: &Path,
    session_id: &str,
    transport: &mut T,
    title_or_preview: Option<&str>,
    explicit_thread_id: Option<&str>,
    resume: bool,
    observed_at_unix: u64,
) -> Result<String, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    let snapshot = store.load_session(session_id).map_err(RuntimeError::from)?;
    if matches!(
        snapshot.state,
        super::autonomy_state::AutonomousSessionStateV1::Running
            | super::autonomy_state::AutonomousSessionStateV1::Verifying
    ) {
        return Err(RuntimeError::Validation(
            "cannot bind a Codex thread while the autonomous session is in flight".into(),
        ));
    }
    let preparation = CodexAppServerReadClientV1::prepare_mutating_thread(
        transport,
        &workspace.to_string_lossy(),
        title_or_preview,
        explicit_thread_id,
        observed_at_unix,
    )?;
    if snapshot
        .provider_thread_id
        .as_deref()
        .is_some_and(|bound| bound != preparation.thread.thread_id)
    {
        return Err(RuntimeError::Validation(
            "autonomous session already has a different provider thread binding".into(),
        ));
    }
    if resume {
        CodexAppServerReadClientV1::resume_exact(
            transport,
            &CodexThreadResolutionV1::Exact(preparation.thread.clone()),
        )?;
    }
    store
        .record_codex_routing_telemetry(session_id, preparation.telemetry, 900)
        .map_err(RuntimeError::from)?;
    let mut updated = store.load_session(session_id).map_err(RuntimeError::from)?;
    updated.provider_thread_id = Some(preparation.thread.thread_id.clone());
    store.save_session(&updated).map_err(RuntimeError::from)?;
    store
        .append_event(
            session_id,
            "codex_thread_bound",
            "exact operator-local app-server thread bound",
        )
        .map_err(RuntimeError::from)?;
    Ok(preparation.thread.thread_id)
}

/// The CatDesk host owns this lifecycle. It starts the supported stdio
/// app-server in the normal current-user Codex context, initializes it, and
/// persists a canonical exact-CWD Terra/High binding before a worker may
/// mutate a repository. Workers never call this function or launch Codex.
pub fn host_prepare_codex_app_server_thread(
    workspace: &Path,
    session_id: &str,
    observed_at_unix: u64,
) -> Result<String, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    let snapshot = store.load_session(session_id).map_err(RuntimeError::from)?;
    let contract = store
        .load_contract(session_id)
        .map_err(RuntimeError::from)?;

    let project_store =
        AutonomousProjectRegistryStoreV1::open(workspace.join(".catdesk").join("projects"))?;
    project_store.initialize(1, 1)?;
    let registered = project_store.project_for_workspace(&contract.project_id, &workspace)?;
    let registry_thread = registered.codex_thread_id.clone();
    if snapshot
        .provider_thread_id
        .as_ref()
        .zip(registry_thread.as_ref())
        .is_some_and(|(session_thread, project_thread)| session_thread != project_thread)
    {
        return Err(RuntimeError::Validation(
            "session Codex thread conflicts with the canonical project binding".into(),
        ));
    }

    let mut transport = host_app_server_launch_config(&workspace)?.spawn_stdio_transport()?;
    transport.initialize()?;
    // The current session's provider thread is not bootstrap evidence by
    // itself: it may be stale or originate from an interrupted task. A
    // canonical thread must come from the durable project binding first, or
    // from a previously COMPLETED_VERIFIED session for this exact project and
    // workspace. `bind_codex_app_server_thread` below still rejects a current
    // session that disagrees with the selected canonical identity.
    let trusted_candidate = if registry_thread.is_none() {
        let trusted_thread_ids = store
            .list_sessions()
            .map_err(RuntimeError::from)?
            .into_iter()
            .filter(|candidate| {
                candidate.session_id != session_id
                    && candidate.state == AutonomousSessionStateV1::CompletedVerified
                    && candidate.provider_thread_id.is_some()
            })
            .filter_map(|candidate| {
                let candidate_contract = store.load_contract(&candidate.session_id).ok()?;
                let candidate_workspace = candidate_contract.workspace.canonicalize().ok()?;
                if candidate_contract.project_id == contract.project_id
                    && candidate_workspace == workspace
                    && candidate_contract.provider_policy.primary_model
                        == super::codex_app_server::CATDESK_REQUIRED_CODEX_MODEL_V1
                {
                    candidate.provider_thread_id
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        CodexAppServerReadClientV1::discover_recent_trusted_exec_thread(
            &mut transport,
            &workspace.to_string_lossy(),
            &trusted_thread_ids,
        )?
    } else {
        None
    };

    let candidate_thread = canonical_thread_candidate(registry_thread, trusted_candidate)
        .ok_or_else(|| {
            RuntimeError::Validation(
                "no trusted completed Codex thread is available for canonical project binding"
                    .into(),
            )
        })?;
    let bound_thread = bind_codex_app_server_thread(
        &workspace,
        session_id,
        &mut transport,
        None,
        Some(&candidate_thread),
        false,
        observed_at_unix,
    )?;

    let registry = project_store.load_registry()?;
    if !registry
        .projects
        .iter()
        .any(|project| project.project_id == contract.project_id)
    {
        project_store.register_project(AutonomousProjectV1 {
            project_id: contract.project_id.clone(),
            workspace: workspace.clone(),
            git_identity: contract.expected_origin.clone(),
            verification_profile: contract.verification_policy.profile.clone(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        })?;
    }
    project_store.bind_codex_thread(&contract.project_id, &bound_thread)?;
    let persisted = project_store
        .load_registry()?
        .projects
        .into_iter()
        .find(|project| project.project_id == contract.project_id)
        .ok_or_else(|| {
            RuntimeError::Validation("canonical project binding was not persisted".into())
        })?;
    if persisted.workspace != workspace
        || persisted.codex_thread_id.as_deref() != Some(bound_thread.as_str())
    {
        return Err(RuntimeError::Validation(
            "canonical project/thread binding verification failed".into(),
        ));
    }
    Ok(bound_thread)
}

/// Host-only observation/bootstrap for a never-registered external project.
/// The caller supplies no thread, prompt, executable, or authentication data.
/// An exact-CWD app-server-visible Terra/High idle thread is preferred first.
/// Zero candidates alone permits the fixed read-only bootstrap; ambiguity,
/// ownership, or metadata mismatch fails closed rather than creating a
/// parallel hidden thread.
pub fn host_observe_registration_thread(
    workspace: &Path,
    git_identity: &str,
    observed_at_unix: u64,
) -> Result<ProjectRegistrationEvidenceV1, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("registration workspace canonicalization failed".into())
    })?;
    let mut transport = host_app_server_launch_config(&workspace)?.spawn_stdio_transport()?;
    transport.initialize()?;
    let cwd = workspace.to_string_lossy().into_owned();
    let candidates = CodexAppServerReadClientV1::discover_adoptable_threads(
        &mut transport,
        &cwd,
        observed_at_unix,
    )?;
    let preparation = match candidates.as_slice() {
        [preparation] => preparation.clone(),
        [] => {
            let response = transport.request(
                "thread/start",
                serde_json::json!({
                    "cwd": cwd,
                    "model": super::codex_app_server::CATDESK_REQUIRED_CODEX_MODEL_V1,
                    "effort": super::codex_app_server::CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1,
                    "approvalPolicy": "never",
                    "sandboxPolicy": {"type":"readOnly", "networkAccess":false},
                    "input": [{"type":"text", "text":"CatDesk registration bootstrap. Do not modify files, run commands, use tools, or access the network. Reply only that this exact workspace thread is available."}]
                }),
            )?;
            let metadata = super::codex_app_server::parse_thread_metadata(&response)?;
            if PathBuf::from(&metadata.cwd).canonicalize().ok().as_deref() != Some(&workspace) {
                return Err(RuntimeError::Validation(
                    "registration bootstrap returned a different workspace".into(),
                ));
            }
            CodexAppServerReadClientV1::prepare_mutating_thread(
                &mut transport,
                &cwd,
                None,
                Some(&metadata.thread_id),
                observed_at_unix,
            )?
        }
        _ => {
            return Err(RuntimeError::Validation(
                "registration has multiple app-server-visible existing threads".into(),
            ));
        }
    };
    if PathBuf::from(&preparation.thread.cwd)
        .canonicalize()
        .ok()
        .as_deref()
        != Some(&workspace)
    {
        return Err(RuntimeError::Validation(
            "registration thread cwd drifted".into(),
        ));
    }
    Ok(ProjectRegistrationEvidenceV1 {
        workspace,
        git_identity: git_identity.into(),
        codex_thread_id: preparation.thread.thread_id,
        selected_model: preparation.thread.selected_model.unwrap_or_default(),
        reasoning_effort: preparation.thread.reasoning_effort.unwrap_or_default(),
    })
}

/// Reads only supported app-server metadata for exact-CWD existing threads.
/// This is deliberately separate from bootstrap: discovering no candidate
/// creates nothing, so an operator-selected existing thread is never silently
/// replaced with a hidden bootstrap thread.
pub fn host_discover_project_thread_adoption_candidates(
    workspace: &Path,
    git_identity: &str,
    observed_at_unix: u64,
) -> Result<Vec<HostThreadAdoptionCandidateV1>, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("adoption workspace canonicalization failed".into())
    })?;
    let cwd = workspace.to_string_lossy().into_owned();
    let mut transport = host_app_server_launch_config(&workspace)?.spawn_stdio_transport()?;
    transport.initialize()?;
    CodexAppServerReadClientV1::discover_adoptable_threads(&mut transport, &cwd, observed_at_unix)?
        .into_iter()
        .map(|preparation| {
            Ok(HostThreadAdoptionCandidateV1 {
                title: preparation.thread.title.clone(),
                evidence: ProjectRegistrationEvidenceV1 {
                    workspace: workspace.clone(),
                    git_identity: git_identity.into(),
                    codex_thread_id: preparation.thread.thread_id,
                    selected_model: preparation.thread.selected_model.unwrap_or_default(),
                    reasoning_effort: preparation.thread.reasoning_effort.unwrap_or_default(),
                },
            })
        })
        .collect()
}

/// Confirmation re-reads one raw id that was retained by a prior host-owned
/// preflight; no MCP caller provides this id.
pub fn host_confirm_project_thread_adoption(
    workspace: &Path,
    git_identity: &str,
    thread_id: &str,
    observed_at_unix: u64,
) -> Result<ProjectRegistrationEvidenceV1, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("adoption workspace canonicalization failed".into())
    })?;
    let cwd = workspace.to_string_lossy().into_owned();
    let mut transport = host_app_server_launch_config(&workspace)?.spawn_stdio_transport()?;
    transport.initialize()?;
    let preparation = CodexAppServerReadClientV1::prepare_mutating_thread(
        &mut transport,
        &cwd,
        None,
        Some(thread_id),
        observed_at_unix,
    )?;
    if PathBuf::from(&preparation.thread.cwd)
        .canonicalize()
        .ok()
        .as_deref()
        != Some(&workspace)
    {
        return Err(RuntimeError::Validation(
            "adopted thread cwd drifted".into(),
        ));
    }
    Ok(ProjectRegistrationEvidenceV1 {
        workspace,
        git_identity: git_identity.into(),
        codex_thread_id: preparation.thread.thread_id,
        selected_model: preparation.thread.selected_model.unwrap_or_default(),
        reasoning_effort: preparation.thread.reasoning_effort.unwrap_or_default(),
    })
}

/// Starts one of the two bounded host-owned continuity turns. It cannot
/// create a thread and never accepts caller-provided prompt, model, or
/// sandbox settings. Step two is blocked until step one's exact turn is
/// observed complete on the same canonical thread.
pub fn host_start_same_thread_continuity_turn(
    workspace: &Path,
    session_id: &str,
    sequence: u8,
    observed_at_unix: u64,
) -> Result<CodexContinuityTurnV1, RuntimeError> {
    if !(1..=2).contains(&sequence) {
        return Err(RuntimeError::Validation(
            "same-thread continuity sequence must be one or two".into(),
        ));
    }
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let canonical_thread_id =
        host_prepare_codex_app_server_thread(&workspace, session_id, observed_at_unix)?;
    let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    let snapshot = store.load_session(session_id).map_err(RuntimeError::from)?;
    let existing = snapshot.codex_continuity.clone();
    match (sequence, existing.as_ref()) {
        (1, None) => {}
        (1, Some(_)) => {
            return Err(RuntimeError::Validation(
                "first same-thread continuity turn was already started".into(),
            ));
        }
        (2, Some(evidence))
            if evidence.thread_id == canonical_thread_id
                && evidence.first_completed
                && evidence.second_turn_id.is_none() => {}
        (2, _) => {
            return Err(RuntimeError::Validation(
                "second continuity turn requires a completed first turn on the canonical thread"
                    .into(),
            ));
        }
        _ => unreachable!(),
    }

    let mut transport = host_app_server_launch_config(&workspace)?.spawn_stdio_transport()?;
    transport.initialize()?;
    let preparation = CodexAppServerReadClientV1::prepare_mutating_thread(
        &mut transport,
        &workspace.to_string_lossy(),
        None,
        Some(&canonical_thread_id),
        observed_at_unix,
    )?;
    if preparation.thread.thread_id != canonical_thread_id {
        return Err(RuntimeError::Validation(
            "continuity preflight did not preserve the canonical thread id".into(),
        ));
    }
    if let Some(evidence) = existing.as_ref() {
        if !CodexAppServerReadClientV1::continuity_turn_completed(
            &mut transport,
            &canonical_thread_id,
            &evidence.first_turn_id,
        )? {
            return Err(RuntimeError::Validation(
                "first continuity turn is not complete on the canonical thread".into(),
            ));
        }
    }
    let started = CodexAppServerReadClientV1::start_harmless_continuity_turn(
        &mut transport,
        &preparation,
        sequence,
    )?;
    if started.thread_id != canonical_thread_id {
        return Err(RuntimeError::Validation(
            "continuity turn start returned a different thread id".into(),
        ));
    }
    let mut updated = store.load_session(session_id).map_err(RuntimeError::from)?;
    updated.codex_continuity = Some(match sequence {
        1 => CodexContinuityEvidenceV1 {
            thread_id: canonical_thread_id,
            first_turn_id: started.turn_id.clone(),
            first_completed: false,
            second_turn_id: None,
            second_completed: false,
            observed_at_unix,
        },
        2 => {
            let mut evidence = updated.codex_continuity.ok_or_else(|| {
                RuntimeError::Validation(
                    "first continuity evidence disappeared before step two".into(),
                )
            })?;
            if evidence.thread_id != started.thread_id || !evidence.first_completed {
                return Err(RuntimeError::Validation(
                    "second continuity turn no longer shares completed canonical evidence".into(),
                ));
            }
            evidence.second_turn_id = Some(started.turn_id.clone());
            evidence.second_completed = false;
            evidence.observed_at_unix = observed_at_unix;
            evidence
        }
        _ => unreachable!(),
    });
    store.save_session(&updated).map_err(RuntimeError::from)?;
    store
        .append_event(
            session_id,
            "codex_same_thread_continuity_started",
            "host-owned bounded same-thread continuity turn started",
        )
        .map_err(RuntimeError::from)?;
    Ok(started)
}

/// Records completion only after `thread/turns/list` finds the exact started
/// turn on the same canonical thread. The second completion additionally
/// triggers the bounded post-turn accounting refresh when available.
pub fn host_record_same_thread_continuity_completion(
    workspace: &Path,
    session_id: &str,
    sequence: u8,
    observed_at_unix: u64,
) -> Result<CodexContinuityEvidenceV1, RuntimeError> {
    if !(1..=2).contains(&sequence) {
        return Err(RuntimeError::Validation(
            "same-thread continuity sequence must be one or two".into(),
        ));
    }
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let canonical_thread_id =
        host_prepare_codex_app_server_thread(&workspace, session_id, observed_at_unix)?;
    let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    let mut snapshot = store.load_session(session_id).map_err(RuntimeError::from)?;
    let mut evidence = snapshot.codex_continuity.clone().ok_or_else(|| {
        RuntimeError::Validation(
            "continuity completion requires a started host continuity turn".into(),
        )
    })?;
    if evidence.thread_id != canonical_thread_id {
        return Err(RuntimeError::Validation(
            "continuity evidence does not match the canonical thread binding".into(),
        ));
    }
    let turn_id = match sequence {
        1 => &evidence.first_turn_id,
        2 if evidence.first_completed => evidence.second_turn_id.as_deref().ok_or_else(|| {
            RuntimeError::Validation("second continuity turn was not started".into())
        })?,
        2 => {
            return Err(RuntimeError::Validation(
                "second continuity completion requires first completion".into(),
            ));
        }
        _ => unreachable!(),
    }
    .to_string();
    let mut transport = host_app_server_launch_config(&workspace)?.spawn_stdio_transport()?;
    transport.initialize()?;
    if !CodexAppServerReadClientV1::continuity_turn_completed(
        &mut transport,
        &canonical_thread_id,
        &turn_id,
    )? {
        return Err(RuntimeError::Validation(
            "continuity turn is not complete yet".into(),
        ));
    }
    if sequence == 1 {
        evidence.first_completed = true;
    } else {
        evidence.second_completed = true;
    }
    evidence.observed_at_unix = observed_at_unix;
    snapshot.codex_continuity = Some(evidence.clone());
    store.save_session(&snapshot).map_err(RuntimeError::from)?;
    store
        .append_event(
            session_id,
            "codex_same_thread_continuity_completed",
            "host-owned bounded same-thread continuity turn completed",
        )
        .map_err(RuntimeError::from)?;
    if sequence == 2 {
        host_refresh_codex_app_server_accounting(&workspace, session_id, observed_at_unix)?;
    }
    Ok(evidence)
}

/// Chooses only durable identity evidence. Deliberately accept no current
/// session thread argument here: an in-progress session cannot bootstrap a
/// canonical project binding.
fn canonical_thread_candidate(
    registry_thread: Option<String>,
    completed_verified_thread: Option<String>,
) -> Option<String> {
    registry_thread.or(completed_verified_thread)
}

/// Captures a bounded post-turn host snapshot. The accounting store computes
/// only comparable rate-limit percentage deltas; it never estimates credits.
pub fn host_refresh_codex_app_server_accounting(
    workspace: &Path,
    session_id: &str,
    observed_at_unix: u64,
) -> Result<(), RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    let accounting = store
        .execution_accounting_store()
        .map_err(RuntimeError::from)?;
    if !accounting.needs_post_codex_observability(session_id)? {
        return Ok(());
    }
    let mut transport = host_app_server_launch_config(&workspace)?.spawn_stdio_transport()?;
    transport.initialize()?;
    refresh_post_codex_accounting_with_transport(
        &workspace,
        &store,
        session_id,
        observed_at_unix,
        &mut transport,
    )?;
    Ok(())
}

/// Uses only the exact supported read surface and is generic solely so the
/// native boundary can be deterministic-tested.  The transport never receives
/// a caller-selected provider, account, route, or billing operation.
fn refresh_post_codex_accounting_with_transport<T: CodexAppServerTransportV1>(
    workspace: &Path,
    store: &AutonomousStateStoreV1,
    session_id: &str,
    observed_at_unix: u64,
    transport: &mut T,
) -> Result<bool, RuntimeError> {
    let snapshot = store.load_session(session_id).map_err(RuntimeError::from)?;
    let thread_id = snapshot.provider_thread_id.ok_or_else(|| {
        RuntimeError::Validation(
            "Codex accounting refresh requires a canonical thread binding".into(),
        )
    })?;
    let telemetry = CodexAppServerReadClientV1::refresh_telemetry(
        transport,
        observed_at_unix,
        Some(&thread_id),
    )?;
    let metadata = super::codex_app_server::CodexThreadMetadataV1 {
        thread_id,
        cwd: workspace.to_string_lossy().into_owned(),
        title: None,
        preview: None,
        selected_model: telemetry.selected_model.clone(),
        reasoning_effort: telemetry.reasoning_effort.clone(),
        token_usage: telemetry.thread_token_usage.clone(),
        concurrently_owned: false,
    };
    super::codex_app_server::enforce_terra_high(&metadata)?;
    let recorded = store
        .execution_accounting_store()
        .map_err(RuntimeError::from)?
        .record_post_codex_observability(
            session_id,
            telemetry.clone(),
            u128::from(observed_at_unix) * 1_000,
        )?;
    if recorded {
        store
            .record_codex_routing_telemetry(session_id, telemetry, 900)
            .map_err(RuntimeError::from)?;
    }
    Ok(recorded)
}

fn registry() -> &'static Arc<Mutex<RuntimeRegistry>> {
    RUNTIME_REGISTRY.get_or_init(|| Arc::new(Mutex::new(BTreeMap::new())))
}

fn reviewer_wakeups() -> &'static Arc<Mutex<BTreeMap<String, u64>>> {
    REVIEWER_WAKEUPS.get_or_init(|| Arc::new(Mutex::new(BTreeMap::new())))
}

/// The only exit transition for a reviewer wakeup owner.  Generations are
/// monotonic for one installed key: a re-arm may make the current owner adopt
/// a newer generation, but an old owner can retire only the exact generation
/// it observed.  `OwnershipLost` deliberately does not mutate the registry;
/// it covers a missing key or an impossible older generation without risking
/// removal of a registration now owned by somebody else.
fn project_wake_target(workspace: &Path, project_id: &str) -> Option<String> {
    if crate::wake_protocol_client::selected(workspace) {
        let root = catdesk_wake::runtime::default_root().ok()?;
        return catdesk_wake::store::Store::open(&root)
            .ok()?
            .config()
            .ok()?
            .targets
            .get(project_id)
            .map(|target| target.url.clone());
    }
    let store =
        AutonomousProjectRegistryStoreV1::open(workspace.join(".catdesk").join("projects")).ok()?;
    // Compatibility is a durable CatDesk-only registry migration, never a
    // runtime fallback. External projects and already-bound targets are left
    // untouched; malformed local configuration yields no dispatch target.
    if project_id == super::autonomy_projects::CATDESK_PROJECT_ID_V1 {
        if let Some(target) = validated_legacy_catdesk_wake_target(workspace) {
            let _ = store.migrate_catdesk_chat_target_if_unbound(workspace, &target);
        }
    }
    let project = store.project_for_workspace(project_id, workspace).ok()?;
    let target = project
        .chatgpt_target_url
        .as_deref()
        .and_then(canonical_wake_target)?;
    let digest = super::autonomy_projects::project_chat_target_digest(&target);
    (project.chatgpt_target_sha256.as_deref() == Some(digest.as_str())).then_some(target)
}

/// Persist one delegated completion handoff against the exact registered
/// project for this workspace. This deliberately does not create an autonomy
/// session: the delegated run id is only a compatibility identity for the
/// existing durable review inbox consumer.
pub fn emit_delegated_review_inbox(
    workspace: &Path,
    run_id: &str,
    final_review_sha256: &str,
    diff_hash: &str,
) -> Result<AutonomousReviewInboxRecordV1, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("delegated review workspace canonicalization failed".into())
    })?;
    let project_store =
        AutonomousProjectRegistryStoreV1::open(workspace.join(".catdesk").join("projects"))?;
    let registry = project_store.load_registry()?;
    let matches = registry
        .projects
        .iter()
        .filter(|project| project.workspace == workspace)
        .collect::<Vec<_>>();
    let [project] = matches.as_slice() else {
        return Err(RuntimeError::Validation(
            "delegated review requires exactly one registered project for its workspace".into(),
        ));
    };
    let target = project_wake_target(&workspace, &project.project_id).ok_or_else(|| {
        RuntimeError::Validation("delegated review project has no exact ChatGPT target".into())
    })?;
    if final_review_sha256.len() != 64
        || !final_review_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || diff_hash.is_empty()
        || diff_hash.len() > 128
    {
        return Err(RuntimeError::Validation(
            "delegated review evidence digest is invalid".into(),
        ));
    }
    let reference = delegated_review_reference(run_id, final_review_sha256, diff_hash, &target)?;
    let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    store
        .emit_delegated_review_inbox_record(run_id, &project.project_id, &reference, now_unix())
        .map_err(RuntimeError::from)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DelegatedReviewReferenceV1 {
    run_id: String,
    target_sha256: String,
}

fn delegated_review_reference(
    run_id: &str,
    final_review_sha256: &str,
    diff_hash: &str,
    target: &str,
) -> Result<String, RuntimeError> {
    if !valid_delegated_review_run_id(run_id)
        || final_review_sha256.len() != 64
        || !final_review_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || diff_hash.is_empty()
        || diff_hash.len() > 128
        || !diff_hash.is_ascii()
        || diff_hash.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(RuntimeError::Validation(
            "delegated review evidence digest is invalid".into(),
        ));
    }
    Ok(format!(
        "delegated-run={run_id};final-review-sha256={final_review_sha256};diff={diff_hash};target-sha256={}",
        wake_digest(target)
    ))
}

fn parse_delegated_review_reference(reference: &str) -> Option<DelegatedReviewReferenceV1> {
    let mut fields = reference.split(';');
    let run_id = fields.next()?.strip_prefix("delegated-run=")?;
    let final_review_sha256 = fields.next()?.strip_prefix("final-review-sha256=")?;
    let diff_hash = fields.next()?.strip_prefix("diff=")?;
    let target_sha256 = fields.next()?.strip_prefix("target-sha256=")?;
    if fields.next().is_some()
        || !valid_delegated_review_run_id(run_id)
        || final_review_sha256.len() != 64
        || !final_review_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || diff_hash.is_empty()
        || diff_hash.len() > 128
        || !diff_hash.is_ascii()
        || diff_hash.bytes().any(|byte| byte.is_ascii_control())
        || target_sha256.len() != 64
        || !target_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(DelegatedReviewReferenceV1 {
        run_id: run_id.into(),
        target_sha256: target_sha256.into(),
    })
}

fn valid_delegated_review_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= 120
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn validated_legacy_catdesk_wake_target(workspace: &Path) -> Option<String> {
    let wake_root = workspace.join(".catdesk").join("wake-bridge");
    let root = wake_root.canonicalize().ok()?;
    let config_path = wake_root.join("config.json");
    let metadata = fs::symlink_metadata(&config_path).ok()?;
    if !metadata.is_file()
        || path_is_reparse_or_symlink(&metadata)
        || metadata.len() > WAKE_BRIDGE_CONFIG_MAX_BYTES
    {
        return None;
    }
    let canonical_config = config_path.canonicalize().ok()?;
    if !canonical_config.starts_with(&root) {
        return None;
    }
    let config = bounded_json(&canonical_config, WAKE_BRIDGE_CONFIG_MAX_BYTES)?;
    let profile = config
        .get("profile_dir")
        .and_then(serde_json::Value::as_str)?;
    let supplied = Path::new(profile);
    if profile.is_empty()
        || profile.len() > 4_096
        || supplied
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let profile_path = if supplied.is_absolute() {
        supplied.into()
    } else {
        root.join(supplied)
    };
    let profile_metadata = fs::symlink_metadata(&profile_path).ok()?;
    if !profile_metadata.is_dir() || path_is_reparse_or_symlink(&profile_metadata) {
        return None;
    }
    let canonical_profile = profile_path.canonicalize().ok()?;
    if !canonical_profile.starts_with(&root) {
        return None;
    }
    config
        .get("conversation_url")
        .and_then(serde_json::Value::as_str)
        .and_then(canonical_wake_target)
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
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewerWakeupRetirement {
    Continue(u64),
    Retired,
    OwnershipLost,
}

fn install_reviewer_wakeup_if_absent(
    wakeups: &mut BTreeMap<String, u64>,
    key: &str,
) -> Option<u64> {
    if wakeups.contains_key(key) {
        None
    } else {
        wakeups.insert(key.to_owned(), 0);
        Some(0)
    }
}

fn request_reviewer_rearm_locked(wakeups: &mut BTreeMap<String, u64>, key: &str) {
    if let Some(generation) = wakeups.get_mut(key) {
        *generation = generation.saturating_add(1);
    }
}

/// Atomically retire this reviewer owner or adopt a re-arm that won the race.
/// Callers must exit only after this transition says `Retired` or ownership was
/// already lost; a `Continue` result is still owned by this same loop.
fn retire_or_adopt_reviewer_wakeup_locked(
    wakeups: &mut BTreeMap<String, u64>,
    key: &str,
    observed_generation: u64,
) -> ReviewerWakeupRetirement {
    match wakeups.get(key).copied() {
        Some(generation) if generation > observed_generation => {
            ReviewerWakeupRetirement::Continue(generation)
        }
        Some(generation) if generation == observed_generation => {
            wakeups.remove(key);
            ReviewerWakeupRetirement::Retired
        }
        // A lower generation violates the per-key monotonic invariant.  Do
        // not try to repair it here: this stale owner must never delete it.
        Some(_) | None => ReviewerWakeupRetirement::OwnershipLost,
    }
}

async fn retire_or_adopt_reviewer_wakeup(
    key: &str,
    observed_generation: u64,
) -> ReviewerWakeupRetirement {
    let mut wakeups = reviewer_wakeups().lock().await;
    retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, key, observed_generation)
}

fn wake_dispatches() -> &'static StdMutex<BTreeSet<String>> {
    WAKE_DISPATCHES.get_or_init(|| StdMutex::new(BTreeSet::new()))
}

fn active_wake_dispatches() -> &'static StdMutex<BTreeSet<String>> {
    ACTIVE_WAKE_DISPATCHES.get_or_init(|| StdMutex::new(BTreeSet::new()))
}

/// Read-only positive lifecycle evidence for the TUI. This exposes no bridge
/// target/configuration and is cleared immediately after the bounded dispatch.
pub fn wake_dispatch_active(workspace: &Path, session_id: &str) -> bool {
    let key = format!("{}:{session_id}", workspace.to_string_lossy());
    active_wake_dispatches()
        .lock()
        .is_ok_and(|dispatches| dispatches.contains(&key))
}

fn startup_wake_rehydrations() -> &'static StdMutex<BTreeSet<String>> {
    STARTUP_WAKE_REHYDRATIONS.get_or_init(|| StdMutex::new(BTreeSet::new()))
}

/// Read only durable handoffs that were already waiting for ChatGPT when this
/// process started. This never rehydrates a provider/controller turn: RUNNING,
/// VERIFYING, recovery, rate-limit, and queued states are intentionally
/// excluded. Stale unread records from inactive sessions are also ignored.
fn persisted_waiting_wake_candidates(workspace: &Path) -> Result<Vec<String>, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("wake rehydration workspace canonicalization failed".into())
    })?;
    let autonomy_root = workspace.join(".catdesk").join("autonomy");
    if !autonomy_root.is_dir() {
        return Ok(Vec::new());
    }
    let store = AutonomousStateStoreV1::open(&autonomy_root).map_err(RuntimeError::from)?;
    let mut candidates = Vec::new();
    for snapshot in store.list_sessions().map_err(RuntimeError::from)? {
        if !snapshot.active || snapshot.state != AutonomousSessionStateV1::WaitingForChatgpt {
            continue;
        }
        let Some(record) = store
            .latest_actionable_review_for_session(&snapshot.session_id)
            .map_err(RuntimeError::from)?
        else {
            continue;
        };
        if record.state != AutonomousSessionStateV1::WaitingForChatgpt
            || record.next_action != "chatgpt_decision_required"
            || record.session_id != snapshot.session_id
        {
            continue;
        }
        let Some(target) = project_wake_target(&workspace, &record.project_id) else {
            continue;
        };
        let wake_root = workspace.join(".catdesk").join("wake-bridge");
        match persisted_wake_receipt_state(&wake_root, &record.record_id, &target) {
            PersistedWakeReceiptState::Unsent => {
                // Validate any durable retry journal before startup installs an
                // owner. A malformed or impossible-future schedule fails
                // closed. A valid journal bound to a former project target is
                // reconciled by minting a distinct successor review: the old
                // exact record is never replayed into the new conversation.
                let now = now_unix();
                match load_wake_retry_schedule(&wake_root, &record.record_id, &target, now) {
                    Ok(Some(schedule)) if schedule.attempt >= WAKE_CHAT_BUSY_RETRY_LIMIT => {}
                    Ok(_) => candidates.push(snapshot.session_id),
                    Err(_) => {
                        let prior_schedule =
                            load_wake_retry_schedule_any_target(&wake_root, &record.record_id, now)
                                .map_err(|_| {
                                    RuntimeError::Validation(
                                "persisted wake retry schedule is unsafe for restart rehydration"
                                    .into(),
                            )
                                })?;
                        let current_target_sha256 = wake_digest(&target);
                        let Some(prior_schedule) = prior_schedule
                            .filter(|schedule| schedule.target_sha256 != current_target_sha256)
                        else {
                            return Err(RuntimeError::Validation(
                                "persisted wake retry schedule is unsafe for restart rehydration"
                                    .into(),
                            ));
                        };
                        store
                            .append_event(
                                &snapshot.session_id,
                                "wake_target_rebound_before_submit",
                                "pending pre-submit ChatGPT retry moved to the current registered conversation",
                            )
                            .map_err(RuntimeError::from)?;
                        let successor = store
                            .emit_review_inbox_record(
                                &snapshot.session_id,
                                &record.project_id,
                                AutonomousSessionStateV1::WaitingForChatgpt,
                                "chatgpt_decision_required",
                                &record.reference,
                                now,
                            )
                            .map_err(RuntimeError::from)?;
                        if successor.record_id == record.record_id {
                            return Err(RuntimeError::Validation(
                                "wake target rebind did not create a distinct successor review"
                                    .into(),
                            ));
                        }
                        store
                            .acknowledge_review_inbox(&record.record_id)
                            .map_err(RuntimeError::from)?;
                        clear_wake_retry_schedule(&wake_root, &prior_schedule.record_id);
                        candidates.push(snapshot.session_id);
                    }
                }
            }
            PersistedWakeReceiptState::SentCurrentTarget => {
                // This exact record already has a durable receipt for the
                // current target. Startup must not reopen the browser merely
                // because the review remains unread locally. Retry journals
                // are secondary evidence and may be safely retired here.
                clear_wake_retry_schedule(&wake_root, &record.record_id);
            }
            PersistedWakeReceiptState::SentDifferentTarget => {
                // A project conversation was explicitly rebound after this
                // exact review had already been delivered. Preserve the old
                // immutable SENT receipt, advance the session journal so the
                // successor receives a distinct record id, then retire only
                // the old inbox item. Exactly-once remains per record.
                store
                    .append_event(
                        &snapshot.session_id,
                        "wake_target_rebound",
                        "pending ChatGPT handoff moved to the current registered conversation",
                    )
                    .map_err(RuntimeError::from)?;
                let successor = store
                    .emit_review_inbox_record(
                        &snapshot.session_id,
                        &record.project_id,
                        AutonomousSessionStateV1::WaitingForChatgpt,
                        "chatgpt_decision_required",
                        &record.reference,
                        now_unix(),
                    )
                    .map_err(RuntimeError::from)?;
                if successor.record_id == record.record_id {
                    return Err(RuntimeError::Validation(
                        "wake target rebind did not create a distinct successor review".into(),
                    ));
                }
                store
                    .acknowledge_review_inbox(&record.record_id)
                    .map_err(RuntimeError::from)?;
                clear_wake_retry_schedule(&wake_root, &record.record_id);
                candidates.push(snapshot.session_id);
            }
            PersistedWakeReceiptState::Unsafe => {
                return Err(RuntimeError::Validation(
                    "persisted wake receipt is unsafe for restart rehydration".into(),
                ));
            }
        }
    }
    Ok(candidates)
}

/// Recover unread terminal autonomous reviews that reached durable completion
/// before their independent-Wake publication completed. The independent Wake
/// activation cutoff remains the historical-event boundary, and Store::produce
/// is idempotent for an already-published exact record.
fn persisted_completed_review_wake_candidates(
    workspace: &Path,
) -> Result<Vec<String>, RuntimeError> {
    persisted_completed_review_wake_candidates_with_cutoff(workspace, |project_id| {
        crate::wake_protocol_client::current_generation_accept_after(project_id)
    })
}

fn persisted_completed_review_wake_candidates_with_cutoff<F>(
    workspace: &Path,
    mut current_generation_accept_after: F,
) -> Result<Vec<String>, RuntimeError>
where
    F: FnMut(&str) -> Result<u64, String>,
{
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation(
            "completed-review wake rehydration workspace canonicalization failed".into(),
        )
    })?;
    if !crate::wake_protocol_client::selected(&workspace) {
        return Ok(Vec::new());
    }
    let autonomy_root = workspace.join(".catdesk").join("autonomy");
    if !autonomy_root.is_dir() {
        return Ok(Vec::new());
    }
    let store = AutonomousStateStoreV1::open(&autonomy_root).map_err(RuntimeError::from)?;
    let mut candidates = Vec::new();
    for record in store.all_review_inbox().map_err(RuntimeError::from)? {
        if !record.unread
            || record.state != AutonomousSessionStateV1::CompletedVerified
            || record.next_action != "independent_final_review"
            || record.reference != "artifacts/completion.json"
        {
            continue;
        }
        // Apply the same current-target-generation historical boundary as the
        // independent Wake producer *before* startup creates an async dispatch
        // owner. This prevents large stale backlogs from consuming daemon
        // startup capacity merely to be rejected later at publish time.
        let accept_after = current_generation_accept_after(&record.project_id)
            .map_err(RuntimeError::Validation)?;
        if record.created_at_unix < accept_after {
            continue;
        }
        let Ok(snapshot) = store.load_session(&record.session_id) else {
            continue;
        };
        if snapshot.active
            || snapshot.state != AutonomousSessionStateV1::CompletedVerified
            || snapshot.session_id != record.session_id
        {
            continue;
        }
        candidates.push(record.session_id);
    }
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

/// Read only the delegated final-review handoffs that were durably emitted
/// before a restart but have not yet obtained an exact-target receipt. Unlike
/// autonomous-session candidates, these records have no controller session to
/// rehydrate: the authenticated delegated run id in the reference is the only
/// compatibility identity accepted by the existing wake dispatcher.
fn persisted_delegated_review_wake_candidates(
    workspace: &Path,
) -> Result<Vec<String>, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation(
            "delegated wake rehydration workspace canonicalization failed".into(),
        )
    })?;
    let autonomy_root = workspace.join(".catdesk").join("autonomy");
    if !autonomy_root.is_dir() {
        return Ok(Vec::new());
    }
    let store = AutonomousStateStoreV1::open(&autonomy_root).map_err(RuntimeError::from)?;
    let wake_root = workspace.join(".catdesk").join("wake-bridge");
    let mut candidates = Vec::new();
    for record in store.all_review_inbox().map_err(RuntimeError::from)? {
        if !record.unread
            || record.state != AutonomousSessionStateV1::CompletedVerified
            || record.next_action != "independent_final_review"
        {
            continue;
        }
        let Some(reference) = parse_delegated_review_reference(&record.reference) else {
            continue;
        };
        let compatibility_session_id = reference.run_id.replace(['.', ':'], "_");
        if record.session_id != compatibility_session_id
            || record.record_id != format!("review-{compatibility_session_id}")
        {
            continue;
        }
        let Some(target) = project_wake_target(&workspace, &record.project_id) else {
            continue;
        };
        if wake_digest(&target) != reference.target_sha256 {
            // The durable project target is the handoff binding. A later target
            // change may not replay this delegated record into another chat.
            continue;
        }
        match persisted_wake_receipt_state(&wake_root, &record.record_id, &target) {
            PersistedWakeReceiptState::Unsent => {
                let now = now_unix();
                let retryable = wake_retry_rehydration_is_retryable(
                    &wake_root,
                    &record.record_id,
                    &target,
                    now,
                )
                .map_err(|_| {
                    RuntimeError::Validation(
                        "delegated wake retry schedule is unsafe for restart rehydration".into(),
                    )
                })?;
                if retryable {
                    candidates.push(reference.run_id);
                }
            }
            PersistedWakeReceiptState::SentCurrentTarget => {
                clear_wake_retry_schedule(&wake_root, &record.record_id);
            }
            PersistedWakeReceiptState::SentDifferentTarget | PersistedWakeReceiptState::Unsafe => {
                return Err(RuntimeError::Validation(
                    "delegated wake receipt is unsafe for restart rehydration".into(),
                ));
            }
        }
    }
    Ok(candidates)
}

/// Recreate only the out-of-band wake responsibility that is necessarily lost
/// across a daemon restart/reload. The delayed task runs independently of the
/// MCP request that caused a reload, so the target ChatGPT turn may finish and
/// become idle before the browser submits. The existing dispatcher remains the
/// sole submit path and retains durable receipt, policy, exact-target, and
/// bounded-retry semantics.
pub fn rehydrate_persisted_waiting_wakes(workspace: &Path) -> Result<usize, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("wake rehydration workspace canonicalization failed".into())
    })?;
    let candidates = persisted_waiting_wake_candidates(&workspace)?;
    let completed_candidates = persisted_completed_review_wake_candidates(&workspace)?;
    let delegated_candidates = persisted_delegated_review_wake_candidates(&workspace)?;
    let mut scheduled = 0usize;
    for session_id in candidates {
        let key = format!("{}:{session_id}", workspace.to_string_lossy());
        {
            let mut owners = startup_wake_rehydrations().lock().map_err(|_| {
                RuntimeError::Validation("wake rehydration ownership lock failed".into())
            })?;
            if !owners.insert(key.clone()) {
                continue;
            }
        }
        scheduled = scheduled.saturating_add(1);
        let wake_workspace = workspace.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(STARTUP_WAKE_REHYDRATION_DELAY_SECONDS)).await;
            let waiting_outcome = AutonomousControllerOutcomeV1 {
                state: AutonomousSessionStateV1::WaitingForChatgpt,
                provider_events: 0,
                verification: None,
                authoritative_diff: None,
                final_review: None,
            };
            let dispatch_workspace = wake_workspace.clone();
            let dispatch_session = session_id.clone();
            let dispatch_outcome = waiting_outcome.clone();
            let initial = tokio::task::spawn_blocking(move || {
                dispatch_actionable_wake(&dispatch_workspace, &dispatch_session, &dispatch_outcome)
            })
            .await
            .unwrap_or(WakeDispatchOutcome::RetrySoon);
            let _ = continue_wake_retries(
                wake_workspace.clone(),
                session_id.clone(),
                waiting_outcome,
                initial,
            )
            .await;
            if let Ok(mut owners) = startup_wake_rehydrations().lock() {
                owners.remove(&key);
            }
        });
    }
    for session_id in completed_candidates {
        let key = format!("{}:completed:{session_id}", workspace.to_string_lossy());
        {
            let mut owners = startup_wake_rehydrations().lock().map_err(|_| {
                RuntimeError::Validation("wake rehydration ownership lock failed".into())
            })?;
            if !owners.insert(key.clone()) {
                continue;
            }
        }
        scheduled = scheduled.saturating_add(1);
        let wake_workspace = workspace.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(STARTUP_WAKE_REHYDRATION_DELAY_SECONDS)).await;
            let completed_outcome = AutonomousControllerOutcomeV1 {
                state: AutonomousSessionStateV1::CompletedVerified,
                provider_events: 0,
                verification: None,
                authoritative_diff: None,
                final_review: None,
            };
            let dispatch_workspace = wake_workspace.clone();
            let dispatch_session = session_id.clone();
            let dispatch_outcome = completed_outcome.clone();
            let initial = tokio::task::spawn_blocking(move || {
                dispatch_actionable_wake(&dispatch_workspace, &dispatch_session, &dispatch_outcome)
            })
            .await
            .unwrap_or(WakeDispatchOutcome::RetrySoon);
            let _ = continue_wake_retries(
                wake_workspace.clone(),
                session_id.clone(),
                completed_outcome,
                initial,
            )
            .await;
            if let Ok(mut owners) = startup_wake_rehydrations().lock() {
                owners.remove(&key);
            }
        });
    }
    for run_id in delegated_candidates {
        let key = format!("{}:delegated:{run_id}", workspace.to_string_lossy());
        {
            let mut owners = startup_wake_rehydrations().lock().map_err(|_| {
                RuntimeError::Validation("wake rehydration ownership lock failed".into())
            })?;
            if !owners.insert(key.clone()) {
                continue;
            }
        }
        scheduled = scheduled.saturating_add(1);
        let wake_workspace = workspace.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(STARTUP_WAKE_REHYDRATION_DELAY_SECONDS)).await;
            dispatch_delegated_review_wake(wake_workspace, run_id).await;
            if let Ok(mut owners) = startup_wake_rehydrations().lock() {
                owners.remove(&key);
            }
        });
    }
    Ok(scheduled)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WakeDispatchOutcome {
    Confirmed,
    /// The bridge process never acquired its singleton, or the process could
    /// not be launched at all. No browser/page-readiness sequence was started,
    /// so a very short bounded coordination retry is safe.
    RetrySoon,
    /// The exact target was proven to still be generating before any typing or
    /// submit boundary. The durable retry schedule is authoritative; this
    /// duration is only the current in-memory wait until that schedule is due.
    RetryAfter(Duration),
    Terminal,
}

const WAKE_BRIDGE_STATE_MAX_BYTES: u64 = 128 * 1024;
const WAKE_BRIDGE_CONFIG_MAX_BYTES: u64 = 16 * 1024;
const WAKE_BRIDGE_SCRIPT_MAX_BYTES: u64 = 256 * 1024;
const WAKE_BRIDGE_OUTPUT_MAX_BYTES: usize = 256;
const WAKE_POLICY_RECONCILIATION_LIMIT: usize = 4;

fn classify_wake_exit(code: Option<i32>, diagnostic: &str) -> WakeDispatchOutcome {
    match (code, diagnostic) {
        // Distinguish a clean pre-submit ChatGPT-busy result from bridge
        // singleton/process coordination. Both use exit code 3, but only the
        // former is allowed to create the durable five-minute retry schedule.
        (Some(3), "CHATGPT_NOT_IDLE") => {
            WakeDispatchOutcome::RetryAfter(Duration::from_secs(WAKE_CHAT_BUSY_RETRY_SECONDS))
        }
        (Some(3), "WAKE_BRIDGE_BUSY") => WakeDispatchOutcome::RetrySoon,
        _ => WakeDispatchOutcome::Terminal,
    }
}

fn wake_message(record_id: &str) -> String {
    format!(
        "CatDesk review record {record_id}: This is an automated CatDesk wake event. Acknowledge or claim this triggering CatDesk review event before proceeding. Take stock of the current project status and where you last left off. Ensure the previous task and review bundle have been completed and reviewed. Continue working through the broad implementation phases of the project by dividing them into appropriately bounded tickets/tasks. For the next ticket, create a detailed technical design and implementation plan for Codex, send those instructions through CatDesk, and request a review bundle so the implementation can be verified. If the previous work is incomplete or issues remain, continue the fix/review cycle until major bugs are no longer an issue. Do not ask the user to relay prompts between agents. Involve the user only for genuine operator-only decisions."
    )
}

fn wake_digest(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

fn canonical_wake_target(value: &str) -> Option<String> {
    if value.is_empty()
        || value.len() > 512
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    let parsed = reqwest::Url::parse(value).ok()?;
    if parsed.scheme() != "https"
        || !matches!(parsed.host_str(), Some("chatgpt.com" | "chat.openai.com"))
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let segments = parsed.path_segments()?.collect::<Vec<_>>();
    let valid_id = |value: &str| {
        !value.is_empty()
            && value.len() <= 200
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    };
    let valid_conversation_id = |value: &str| {
        valid_id(value) || (value.len() <= 200 && value.strip_prefix("WEB:").is_some_and(valid_id))
    };
    let path = match segments.as_slice() {
        ["c", conversation] if valid_conversation_id(conversation) => {
            format!("/c/{conversation}")
        }
        ["g", project, "c", conversation]
            if valid_id(project) && valid_conversation_id(conversation) =>
        {
            format!("/g/{project}/c/{conversation}")
        }
        _ => return None,
    };
    Some(format!(
        "https://{}{}",
        parsed.host_str().expect("validated wake host"),
        path
    ))
}

fn bounded_bridge_diagnostic(stdout: &[u8], stderr: &[u8]) -> &'static str {
    let mut bounded = Vec::with_capacity(WAKE_BRIDGE_OUTPUT_MAX_BYTES);
    bounded.extend_from_slice(&stdout[..stdout.len().min(WAKE_BRIDGE_OUTPUT_MAX_BYTES)]);
    let remaining = WAKE_BRIDGE_OUTPUT_MAX_BYTES.saturating_sub(bounded.len());
    bounded.extend_from_slice(&stderr[..stderr.len().min(remaining)]);
    let Ok(text) = std::str::from_utf8(&bounded) else {
        return "NON_UTF8";
    };
    match text.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some("WOKE") => "WOKE",
        Some("ALREADY_SENT") => "ALREADY_SENT",
        Some("CHATGPT_NOT_IDLE") => "CHATGPT_NOT_IDLE",
        Some("WAKE_BRIDGE_BUSY") => "WAKE_BRIDGE_BUSY",
        Some("WAKE_CANCELLED") => "WAKE_CANCELLED",
        Some("OPERATOR_ATTENTION") => "OPERATOR_ATTENTION",
        Some(_) => "UNRECOGNIZED",
        None => "EMPTY",
    }
}

fn bounded_json(path: &Path, maximum_bytes: u64) -> Option<serde_json::Value> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > maximum_bytes {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WakeRetryScheduleV1 {
    record_id: String,
    target_sha256: String,
    not_before_unix: u64,
    attempt: u32,
}

fn valid_wake_retry_record_id(record_id: &str) -> bool {
    !record_id.is_empty()
        && record_id.len() <= 128
        && record_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn wake_retry_schedule_path(wake_root: &Path, record_id: &str, attempt: u32) -> PathBuf {
    wake_root.join(format!(
        "retry-{}-{attempt:02}.json",
        wake_digest(record_id)
    ))
}

fn parse_wake_retry_schedule(
    path: &Path,
    record_id: &str,
    expected_target_sha256: Option<&str>,
    expected_attempt: u32,
    now: u64,
) -> Result<WakeRetryScheduleV1, ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > WAKE_RETRY_SCHEDULE_MAX_BYTES
    {
        return Err(());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|_| ())?).map_err(|_| ())?;
    let object = value.as_object().ok_or(())?;
    let required = [
        "schema_version",
        "record_id",
        "target_sha256",
        "not_before_unix",
        "attempt",
        "reason",
    ];
    if object.len() != required.len() || required.iter().any(|name| !object.contains_key(*name)) {
        return Err(());
    }
    if object
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(WAKE_RETRY_SCHEDULE_SCHEMA_VERSION)
        || object.get("record_id").and_then(serde_json::Value::as_str) != Some(record_id)
        || object.get("reason").and_then(serde_json::Value::as_str) != Some("CHATGPT_NOT_IDLE")
        || object.get("attempt").and_then(serde_json::Value::as_u64)
            != Some(u64::from(expected_attempt))
    {
        return Err(());
    }
    let target_sha256 = object
        .get("target_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or(())?;
    if target_sha256.len() != 64
        || !target_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || expected_target_sha256.is_some_and(|expected| expected != target_sha256)
    {
        return Err(());
    }
    let not_before_unix = object
        .get("not_before_unix")
        .and_then(serde_json::Value::as_u64)
        .ok_or(())?;
    if not_before_unix == 0
        || not_before_unix > now.saturating_add(WAKE_CHAT_BUSY_RETRY_SECONDS.saturating_mul(2))
    {
        return Err(());
    }
    Ok(WakeRetryScheduleV1 {
        record_id: record_id.to_owned(),
        target_sha256: target_sha256.to_owned(),
        not_before_unix,
        attempt: expected_attempt,
    })
}

fn load_wake_retry_schedule_for_target_digest(
    wake_root: &Path,
    record_id: &str,
    expected_target_sha256: Option<&str>,
    now: u64,
) -> Result<Option<WakeRetryScheduleV1>, ()> {
    if !valid_wake_retry_record_id(record_id) {
        return Err(());
    }
    let root_metadata = fs::symlink_metadata(wake_root).map_err(|_| ())?;
    if !root_metadata.file_type().is_dir() || root_metadata.file_type().is_symlink() {
        return Err(());
    }
    let mut latest = None;
    let mut gap_seen = false;
    for attempt in 1..=WAKE_CHAT_BUSY_RETRY_LIMIT {
        let path = wake_retry_schedule_path(wake_root, record_id, attempt);
        if !path.exists() {
            gap_seen = true;
            continue;
        }
        if gap_seen {
            return Err(());
        }
        latest = Some(parse_wake_retry_schedule(
            &path,
            record_id,
            expected_target_sha256,
            attempt,
            now,
        )?);
    }
    Ok(latest)
}

fn load_wake_retry_schedule(
    wake_root: &Path,
    record_id: &str,
    target: &str,
    now: u64,
) -> Result<Option<WakeRetryScheduleV1>, ()> {
    let canonical_target = canonical_wake_target(target).ok_or(())?;
    let target_sha256 = wake_digest(&canonical_target);
    load_wake_retry_schedule_for_target_digest(wake_root, record_id, Some(&target_sha256), now)
}

fn load_wake_retry_schedule_any_target(
    wake_root: &Path,
    record_id: &str,
    now: u64,
) -> Result<Option<WakeRetryScheduleV1>, ()> {
    load_wake_retry_schedule_for_target_digest(wake_root, record_id, None, now)
}

/// A persisted busy schedule is retryable only until its fixed deferral budget
/// is consumed.  The terminal journal entry is deliberately retained as
/// durable, bounded evidence for the independent reviewer/deadman path; a
/// restart must not repeatedly schedule a browser-dispatch task that can only
/// fail terminally.
fn wake_retry_rehydration_is_retryable(
    wake_root: &Path,
    record_id: &str,
    target: &str,
    now: u64,
) -> Result<bool, ()> {
    Ok(load_wake_retry_schedule(wake_root, record_id, target, now)?
        .is_none_or(|schedule| schedule.attempt < WAKE_CHAT_BUSY_RETRY_LIMIT))
}

fn schedule_chat_busy_retry(
    wake_root: &Path,
    record_id: &str,
    target: &str,
    now: u64,
) -> Result<WakeRetryScheduleV1, ()> {
    let canonical_target = canonical_wake_target(target).ok_or(())?;
    let target_sha256 = wake_digest(&canonical_target);
    let existing = load_wake_retry_schedule(wake_root, record_id, &canonical_target, now)?;
    if let Some(existing) = existing.as_ref() {
        if existing.not_before_unix > now {
            return Ok(existing.clone());
        }
        if existing.attempt >= WAKE_CHAT_BUSY_RETRY_LIMIT {
            return Err(());
        }
    }
    let attempt = existing.as_ref().map_or(1, |schedule| schedule.attempt + 1);
    let schedule = WakeRetryScheduleV1 {
        record_id: record_id.to_owned(),
        target_sha256,
        not_before_unix: now.saturating_add(WAKE_CHAT_BUSY_RETRY_SECONDS),
        attempt,
    };
    let payload = serde_json::json!({
        "schema_version": WAKE_RETRY_SCHEDULE_SCHEMA_VERSION,
        "record_id": schedule.record_id,
        "target_sha256": schedule.target_sha256,
        "not_before_unix": schedule.not_before_unix,
        "attempt": schedule.attempt,
        "reason": "CHATGPT_NOT_IDLE",
    });
    let bytes = serde_json::to_vec(&payload).map_err(|_| ())?;
    if bytes.len() as u64 > WAKE_RETRY_SCHEDULE_MAX_BYTES {
        return Err(());
    }
    let path = wake_retry_schedule_path(wake_root, record_id, attempt);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| ())?;
    file.write_all(&bytes).map_err(|_| ())?;
    file.sync_all().map_err(|_| ())?;
    Ok(schedule)
}

fn wake_retry_remaining(schedule: &WakeRetryScheduleV1, now: u64) -> Duration {
    Duration::from_secs(schedule.not_before_unix.saturating_sub(now))
}

fn clear_wake_retry_schedule(wake_root: &Path, record_id: &str) {
    if !valid_wake_retry_record_id(record_id) {
        return;
    }
    for attempt in 1..=WAKE_CHAT_BUSY_RETRY_LIMIT {
        let path = wake_retry_schedule_path(wake_root, record_id, attempt);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
}

fn authoritative_bridge_sha256(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > WAKE_BRIDGE_SCRIPT_MAX_BYTES {
        return None;
    }
    Some(format!("{:x}", Sha256::digest(fs::read(path).ok()?)))
}

fn valid_wake_receipt(wake_root: &Path, record_id: &str, target: &str) -> bool {
    let Some(target) = canonical_wake_target(target) else {
        return false;
    };
    let Some(state) = bounded_json(&wake_root.join("state.json"), WAKE_BRIDGE_STATE_MAX_BYTES)
    else {
        return false;
    };
    if state
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(4)
    {
        return false;
    }
    let Some(deliveries) = state
        .get("deliveries")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    let matches: Vec<_> = deliveries
        .iter()
        .filter(|delivery| {
            delivery
                .get("record_id")
                .and_then(serde_json::Value::as_str)
                == Some(record_id)
        })
        .collect();
    if matches.len() != 1 {
        return false;
    }
    let delivery = matches[0];
    let expected_message_digest = wake_digest(&wake_message(record_id));
    let expected_target_digest = wake_digest(&target);
    delivery.get("status").and_then(serde_json::Value::as_str) == Some("SENT")
        && delivery
            .get("browser_sent_at_unix")
            .and_then(serde_json::Value::as_f64)
            .is_some_and(|value| value.is_finite() && value > 0.0)
        && delivery
            .get("receipt_schema_version")
            .and_then(serde_json::Value::as_u64)
            == Some(1)
        && delivery
            .get("message_sha256")
            .and_then(serde_json::Value::as_str)
            == Some(expected_message_digest.as_str())
        && delivery
            .get("target_sha256")
            .and_then(serde_json::Value::as_str)
            == Some(expected_target_digest.as_str())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PersistedWakeReceiptState {
    Unsent,
    SentCurrentTarget,
    SentDifferentTarget,
    Unsafe,
}

/// Classify durable bridge state without weakening the one-submit boundary.
/// A fully proven SENT receipt for a different canonical target is not
/// reusable; callers may issue a *new* review record for an explicit project
/// target rebind while preserving this immutable delivery as audit evidence.
fn persisted_wake_receipt_state(
    wake_root: &Path,
    record_id: &str,
    target: &str,
) -> PersistedWakeReceiptState {
    let state_path = wake_root.join("state.json");
    if !state_path.exists() {
        return PersistedWakeReceiptState::Unsent;
    }
    let Some(state) = bounded_json(&state_path, WAKE_BRIDGE_STATE_MAX_BYTES) else {
        return PersistedWakeReceiptState::Unsafe;
    };
    if state
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(4)
    {
        return PersistedWakeReceiptState::Unsafe;
    }
    let Some(deliveries) = state
        .get("deliveries")
        .and_then(serde_json::Value::as_array)
    else {
        return PersistedWakeReceiptState::Unsafe;
    };
    let matching = deliveries
        .iter()
        .filter(|delivery| {
            delivery
                .get("record_id")
                .and_then(serde_json::Value::as_str)
                == Some(record_id)
        })
        .collect::<Vec<_>>();
    let delivery = match matching.as_slice() {
        [] => return PersistedWakeReceiptState::Unsent,
        [delivery] => *delivery,
        _ => return PersistedWakeReceiptState::Unsafe,
    };
    let pre_submit_receipt_fields_empty = [
        "browser_sent_at_unix",
        "message_sha256",
        "target_sha256",
        "receipt_schema_version",
    ]
    .into_iter()
    .all(|field| delivery.get(field).is_none_or(serde_json::Value::is_null));
    match delivery.get("status").and_then(serde_json::Value::as_str) {
        Some("SENT") => {
            let expected_message_digest = wake_digest(&wake_message(record_id));
            let Some(target_digest) = delivery
                .get("target_sha256")
                .and_then(serde_json::Value::as_str)
            else {
                return PersistedWakeReceiptState::Unsafe;
            };
            let receipt_proven = delivery
                .get("browser_sent_at_unix")
                .and_then(serde_json::Value::as_f64)
                .is_some_and(|value| value.is_finite() && value > 0.0)
                && delivery
                    .get("receipt_schema_version")
                    .and_then(serde_json::Value::as_u64)
                    == Some(1)
                && delivery
                    .get("message_sha256")
                    .and_then(serde_json::Value::as_str)
                    == Some(expected_message_digest.as_str())
                && target_digest.len() == 64
                && target_digest.bytes().all(|byte| byte.is_ascii_hexdigit());
            if !receipt_proven {
                return PersistedWakeReceiptState::Unsafe;
            }
            if canonical_wake_target(target)
                .is_some_and(|target| wake_digest(&target) == target_digest)
            {
                PersistedWakeReceiptState::SentCurrentTarget
            } else {
                PersistedWakeReceiptState::SentDifferentTarget
            }
        }
        Some("CLAIMED")
            if pre_submit_receipt_fields_empty
                && delivery
                    .get("attention")
                    .is_none_or(serde_json::Value::is_null) =>
        {
            PersistedWakeReceiptState::Unsent
        }
        // Compatibility for the one historical pre-submit encoding produced
        // before CHATGPT_NOT_IDLE became a clean CLAIMED/exit-3 result. Other
        // operator-attention reasons (login, CAPTCHA, security, target drift,
        // selector ambiguity, etc.) remain terminal and must never rehydrate.
        Some("OPERATOR_ATTENTION")
            if pre_submit_receipt_fields_empty
                && delivery
                    .get("attention")
                    .and_then(serde_json::Value::as_str)
                    == Some("CHATGPT_NOT_IDLE") =>
        {
            PersistedWakeReceiptState::Unsent
        }
        Some("SUBMITTING" | "OPERATOR_ATTENTION") | Some(_) | None => {
            PersistedWakeReceiptState::Unsafe
        }
    }
}

/// Non-launching readiness used only when an operator enables automatic wake.
/// It validates the same fixed project-local bridge/config identity that the
/// dispatcher will use, without opening the browser or reading profile data.
pub fn wake_policy_enablement_ready(workspace: &Path) -> bool {
    let wake_root = workspace.join(".catdesk").join("wake-bridge");
    let config = bounded_json(&wake_root.join("config.json"), WAKE_BRIDGE_CONFIG_MAX_BYTES);
    // The dedicated profile location is configuration, not authentication
    // material. Validate only that it resolves below the fixed project-local
    // wake root; never inspect its browser/profile contents.
    let profile_ok = config
        .as_ref()
        .and_then(|value| value.get("profile_dir"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 4_096)
        .map(PathBuf::from)
        .map(|candidate| {
            if candidate.is_absolute() {
                candidate
            } else {
                wake_root.join(candidate)
            }
        })
        .and_then(|path| path.canonicalize().ok())
        .is_some_and(|path| {
            wake_root
                .canonicalize()
                .is_ok_and(|root| path.starts_with(root))
        });
    profile_ok
        && wake_root
            .join("venv")
            .join("Scripts")
            .join("python.exe")
            .is_file()
        && authoritative_bridge_sha256(&workspace.join("scripts").join("wake_bridge.py")).is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WakePolicyGateDecision {
    Allow,
    Suppress,
    Stop(&'static str),
}

/// Resolve the small window between reading a stop-condition policy and
/// committing its MANUAL_OFF transition. A failed CAS is never treated as a
/// successful stop: the next iteration reads the durable policy again and
/// evaluates that fresh generation. Unbounded policy churn fails closed.
fn reconcile_wake_policy<FLoad, FEvaluate, FStop>(
    mut load: FLoad,
    mut evaluate: FEvaluate,
    mut stop: FStop,
) -> bool
where
    FLoad: FnMut() -> Result<super::autonomy_state::AutonomousWakePolicyV1, ()>,
    FEvaluate:
        FnMut(&super::autonomy_state::AutonomousWakePolicyV1) -> Result<WakePolicyGateDecision, ()>,
    FStop: FnMut(u64, &'static str) -> Result<(), ()>,
{
    for _ in 0..WAKE_POLICY_RECONCILIATION_LIMIT {
        let Ok(policy) = load() else {
            return false;
        };
        match evaluate(&policy) {
            Ok(WakePolicyGateDecision::Allow) => return true,
            Ok(WakePolicyGateDecision::Suppress) => return false,
            Ok(WakePolicyGateDecision::Stop(reason)) => {
                // A successful atomic write is authoritative for the exact
                // generation just evaluated. A stale/error result requires a
                // fresh policy read; it must not suppress this pending wake.
                if stop(policy.generation, reason).is_ok() {
                    return false;
                }
            }
            Err(()) => return false,
        }
    }
    false
}

fn wake_policy_decision(
    policy: &super::autonomy_state::AutonomousWakePolicyV1,
    task_id: Option<&str>,
    outcome_state: AutonomousSessionStateV1,
    provider_state: Option<(AutonomousSessionStateV1, AutonomousProviderRouteV1)>,
) -> Result<WakePolicyGateDecision, ()> {
    match &policy.mode {
        AutonomousWakeModeV1::ManualOff => Ok(WakePolicyGateDecision::Suppress),
        AutonomousWakeModeV1::Indefinite => Ok(WakePolicyGateDecision::Allow),
        AutonomousWakeModeV1::ThroughTask => {
            if policy.terminal_task_id.as_deref() != task_id {
                return Ok(WakePolicyGateDecision::Allow);
            }
            if outcome_state != AutonomousSessionStateV1::CompletedVerified {
                return Ok(WakePolicyGateDecision::Allow);
            }
            // The final completed review remains unread because the caller
            // stops policy before the bridge claims the actionable record.
            Ok(WakePolicyGateDecision::Stop("through_task_completed"))
        }
        AutonomousWakeModeV1::UntilProviderExhausted => {
            let Some((session_state, provider_route)) = provider_state else {
                return Err(());
            };
            let terminal = session_state == AutonomousSessionStateV1::CreditBudgetExhausted
                || provider_route == AutonomousProviderRouteV1::QwenUnavailable;
            if terminal {
                Ok(WakePolicyGateDecision::Stop("provider_chain_exhausted"))
            } else {
                // Transient Codex rate limits remain eligible for retry and do
                // not authorize an early local-provider transition.
                Ok(WakePolicyGateDecision::Allow)
            }
        }
    }
}

fn wake_policy_allows_dispatch(
    store: &AutonomousStateStoreV1,
    session_id: &str,
    outcome: &AutonomousControllerOutcomeV1,
) -> bool {
    reconcile_wake_policy(
        || store.load_wake_policy().map_err(|_| ()),
        |policy| match &policy.mode {
            AutonomousWakeModeV1::ThroughTask => {
                let contract = store.load_contract(session_id).map_err(|_| ())?;
                wake_policy_decision(policy, Some(&contract.task_id), outcome.state.clone(), None)
            }
            AutonomousWakeModeV1::UntilProviderExhausted => {
                let snapshot = store.load_session(session_id).map_err(|_| ())?;
                wake_policy_decision(
                    policy,
                    None,
                    outcome.state.clone(),
                    Some((snapshot.state, snapshot.provider_route)),
                )
            }
            _ => wake_policy_decision(policy, None, outcome.state.clone(), None),
        },
        |generation, reason| {
            store
                .stop_wake_policy(generation, reason, now_unix())
                .map(|_| ())
                .map_err(|_| ())
        },
    )
}

fn delegated_wake_policy_allows_dispatch(store: &AutonomousStateStoreV1, run_id: &str) -> bool {
    reconcile_wake_policy(
        || store.load_wake_policy().map_err(|_| ()),
        |policy| match &policy.mode {
            AutonomousWakeModeV1::UntilProviderExhausted => Err(()),
            _ => wake_policy_decision(
                policy,
                Some(run_id),
                AutonomousSessionStateV1::CompletedVerified,
                None,
            ),
        },
        |generation, reason| {
            store
                .stop_wake_policy(generation, reason, now_unix())
                .map(|_| ())
                .map_err(|_| ())
        },
    )
}

fn confirm_wake_bridge_result(
    exit_code: Option<i32>,
    diagnostic: &str,
    receipt_valid: bool,
) -> WakeDispatchOutcome {
    if exit_code == Some(0) {
        if matches!(diagnostic, "WOKE" | "ALREADY_SENT") && receipt_valid {
            WakeDispatchOutcome::Confirmed
        } else {
            // A zero without this exact durable receipt is an unsafe bridge
            // outcome, not a reason to launch a second browser/page-readiness
            // sequence.  It remains visible for operator reconciliation.
            WakeDispatchOutcome::Terminal
        }
    } else {
        classify_wake_exit(exit_code, diagnostic)
    }
}

fn dispatch_actionable_wake(
    workspace: &Path,
    session_id: &str,
    outcome: &AutonomousControllerOutcomeV1,
) -> WakeDispatchOutcome {
    dispatch_actionable_wake_with_policy(workspace, session_id, outcome, None)
}

fn dispatch_actionable_wake_with_policy(
    workspace: &Path,
    session_id: &str,
    outcome: &AutonomousControllerOutcomeV1,
    delegated_run_id: Option<&str>,
) -> WakeDispatchOutcome {
    let Ok(store) = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
    else {
        return WakeDispatchOutcome::Terminal;
    };
    let policy_allows = if let Some(run_id) = delegated_run_id {
        delegated_wake_policy_allows_dispatch(&store, run_id)
    } else {
        wake_policy_allows_dispatch(&store, session_id, outcome)
    };
    if !policy_allows {
        return WakeDispatchOutcome::Terminal;
    }
    if !matches!(
        outcome.state,
        AutonomousSessionStateV1::CompletedVerified | AutonomousSessionStateV1::WaitingForChatgpt
    ) {
        return WakeDispatchOutcome::Terminal;
    }
    let Ok(Some(record)) = store.latest_actionable_review_for_session(session_id) else {
        return WakeDispatchOutcome::Terminal;
    };
    if crate::wake_protocol_client::selected(workspace) {
        // Queue acknowledgement is deliberately not reported as browser delivery.
        // WakeHost owns retries and exact receipts after this durable handoff.
        return match crate::wake_protocol_client::publish(
            &record.project_id,
            &record.record_id,
            record.created_at_unix,
            record.state == AutonomousSessionStateV1::CompletedVerified,
        ) {
            Ok(()) => WakeDispatchOutcome::Terminal,
            Err(reason) if reason == "WAKE_HISTORICAL_EVENT_RETAINED" => {
                WakeDispatchOutcome::Terminal
            }
            Err(_) => WakeDispatchOutcome::RetrySoon,
        };
    }
    let Some(target) = project_wake_target(workspace, &record.project_id) else {
        return WakeDispatchOutcome::Terminal;
    };
    let dispatch_key = format!("{}:{}", workspace.to_string_lossy(), record.record_id);
    let Ok(dispatched) = wake_dispatches().lock() else {
        return WakeDispatchOutcome::Terminal;
    };
    if dispatched.contains(&dispatch_key) {
        return WakeDispatchOutcome::Confirmed;
    }
    // The selector is the sole production owner seam.  Do this before any
    // legacy receipt/retry/bridge handling so a Rust selection cannot leave a
    // reachable legacy Python writer.  The Rust entrypoint rechecks it before
    // claiming, which also closes a selection change in this short interval.
    match selected_owner(workspace) {
        Ok(WakeOwnerMode::LegacyPython) => {}
        Ok(WakeOwnerMode::Rust) => {
            drop(dispatched);
            let adapter = match FixedScriptBrowserAdapter::open(workspace) {
                Ok(adapter) => adapter,
                Err(_) => return WakeDispatchOutcome::Terminal,
            };
            let owner_outcome = dispatch_if_rust_selected(
                workspace,
                &record.record_id,
                now_unix() as f64,
                &adapter,
            );
            return match owner_outcome {
                Ok(OwnerOutcome::Sent)
                | Ok(OwnerOutcome::AlreadyOwned(
                    crate::stable_wake_delivery::DeliveryClassification::AlreadySent,
                )) => {
                    if let Ok(mut dispatched) = wake_dispatches().lock() {
                        dispatched.insert(dispatch_key);
                    }
                    WakeDispatchOutcome::Confirmed
                }
                Ok(OwnerOutcome::PreSubmitRetryable) => WakeDispatchOutcome::RetrySoon,
                Ok(OwnerOutcome::AlreadyOwned(_))
                | Ok(OwnerOutcome::OperatorAttention)
                | Err(_) => WakeDispatchOutcome::Terminal,
            };
        }
        Err(_) => return WakeDispatchOutcome::Terminal,
    }
    let wake_root = workspace.join(".catdesk").join("wake-bridge");
    match persisted_wake_receipt_state(&wake_root, &record.record_id, &target) {
        PersistedWakeReceiptState::SentCurrentTarget => {
            clear_wake_retry_schedule(&wake_root, &record.record_id);
            if let Ok(mut dispatched) = wake_dispatches().lock() {
                dispatched.insert(dispatch_key);
            }
            return WakeDispatchOutcome::Confirmed;
        }
        PersistedWakeReceiptState::SentDifferentTarget | PersistedWakeReceiptState::Unsafe => {
            clear_wake_retry_schedule(&wake_root, &record.record_id);
            return WakeDispatchOutcome::Terminal;
        }
        PersistedWakeReceiptState::Unsent => {}
    }
    let now = now_unix();
    match load_wake_retry_schedule(&wake_root, &record.record_id, &target, now) {
        Ok(Some(schedule)) if schedule.not_before_unix > now => {
            return WakeDispatchOutcome::RetryAfter(wake_retry_remaining(&schedule, now));
        }
        Ok(Some(schedule)) if schedule.attempt >= WAKE_CHAT_BUSY_RETRY_LIMIT => {
            // Twelve proven pre-submit busy deferrals cover roughly one hour.
            // Do not launch a thirteenth browser attempt; the independent
            // hourly deadman/reviewer path can surface the stalled handoff.
            return WakeDispatchOutcome::Terminal;
        }
        Ok(_) => {}
        Err(_) => return WakeDispatchOutcome::Terminal,
    }
    let python = wake_root.join("venv").join("Scripts").join("python.exe");
    let config = wake_root.join("config.json");
    let bridge = workspace.join("scripts").join("wake_bridge.py");
    if !python.is_file() || !config.is_file() || !bridge.is_file() {
        return WakeDispatchOutcome::Terminal;
    }
    let Some(bridge_sha256) = authoritative_bridge_sha256(&bridge) else {
        return WakeDispatchOutcome::Terminal;
    };
    drop(dispatched);
    let active_key = format!("{}:{session_id}", workspace.to_string_lossy());
    let Ok(mut active) = active_wake_dispatches().lock() else {
        return WakeDispatchOutcome::Terminal;
    };
    if !active.insert(active_key.clone()) {
        return WakeDispatchOutcome::RetrySoon;
    }
    drop(active);
    let bridge_outcome = Command::new(&python)
        .arg(&bridge)
        .arg("--workspace")
        .arg(workspace)
        .arg("--config")
        .arg(&config)
        .arg("--record-id")
        .arg(&record.record_id)
        .arg("--bridge-sha256")
        .arg(&bridge_sha256)
        .arg("--conversation-url")
        .arg(&target)
        .current_dir(workspace)
        .output()
        .map(|output| {
            let diagnostic = bounded_bridge_diagnostic(&output.stdout, &output.stderr);
            let receipt_valid = valid_wake_receipt(&wake_root, &record.record_id, &target);
            confirm_wake_bridge_result(output.status.code(), diagnostic, receipt_valid)
        })
        .unwrap_or(WakeDispatchOutcome::RetrySoon);
    if let Ok(mut active) = active_wake_dispatches().lock() {
        active.remove(&active_key);
    }
    match bridge_outcome {
        WakeDispatchOutcome::Confirmed => {
            clear_wake_retry_schedule(&wake_root, &record.record_id);
            if let Ok(mut dispatched) = wake_dispatches().lock() {
                dispatched.insert(dispatch_key);
            }
            WakeDispatchOutcome::Confirmed
        }
        WakeDispatchOutcome::RetryAfter(_) => {
            match schedule_chat_busy_retry(&wake_root, &record.record_id, &target, now_unix()) {
                Ok(schedule) => {
                    WakeDispatchOutcome::RetryAfter(wake_retry_remaining(&schedule, now_unix()))
                }
                Err(_) => WakeDispatchOutcome::Terminal,
            }
        }
        WakeDispatchOutcome::RetrySoon => WakeDispatchOutcome::RetrySoon,
        WakeDispatchOutcome::Terminal => {
            clear_wake_retry_schedule(&wake_root, &record.record_id);
            WakeDispatchOutcome::Terminal
        }
    }
}

/// Dispatch an already-persisted delegated completion review through the
/// normal CatDesk wake runtime. This reuses the exact-target receipt and retry
/// machinery while applying a delegated-specific wake-policy gate.
pub async fn dispatch_delegated_review_wake(workspace: PathBuf, run_id: String) {
    let outcome = AutonomousControllerOutcomeV1 {
        state: AutonomousSessionStateV1::CompletedVerified,
        provider_events: 0,
        verification: None,
        authoritative_diff: None,
        final_review: None,
    };
    let compatibility_session_id = run_id.replace(['.', ':'], "_");
    let dispatch_workspace = workspace.clone();
    let dispatch_session = compatibility_session_id.clone();
    let dispatch_run = run_id.clone();
    let dispatch_outcome = outcome.clone();
    let initial = tokio::task::spawn_blocking(move || {
        dispatch_actionable_wake_with_policy(
            &dispatch_workspace,
            &dispatch_session,
            &dispatch_outcome,
            Some(&dispatch_run),
        )
    })
    .await
    .unwrap_or(WakeDispatchOutcome::RetrySoon);
    if matches!(
        initial,
        WakeDispatchOutcome::RetrySoon | WakeDispatchOutcome::RetryAfter(_)
    ) {
        let mut current = initial;
        let mut consecutive_short_attempts = if current == WakeDispatchOutcome::RetrySoon {
            1usize
        } else {
            0usize
        };
        loop {
            match current {
                WakeDispatchOutcome::Confirmed | WakeDispatchOutcome::Terminal => break,
                WakeDispatchOutcome::RetrySoon => {
                    if consecutive_short_attempts >= WAKE_SHORT_RETRY_LIMIT {
                        break;
                    }
                    tokio::time::sleep(WAKE_SHORT_RETRY_DELAY).await;
                }
                WakeDispatchOutcome::RetryAfter(delay) => tokio::time::sleep(delay).await,
            }
            let dispatch_workspace = workspace.clone();
            let dispatch_session = compatibility_session_id.clone();
            let dispatch_run = run_id.clone();
            let dispatch_outcome = outcome.clone();
            current = tokio::task::spawn_blocking(move || {
                dispatch_actionable_wake_with_policy(
                    &dispatch_workspace,
                    &dispatch_session,
                    &dispatch_outcome,
                    Some(&dispatch_run),
                )
            })
            .await
            .unwrap_or(WakeDispatchOutcome::RetrySoon);
            consecutive_short_attempts = if current == WakeDispatchOutcome::RetrySoon {
                consecutive_short_attempts.saturating_add(1).max(1)
            } else {
                0
            };
        }
    }
}

async fn continue_wake_retries(
    workspace: PathBuf,
    session_id: String,
    outcome: AutonomousControllerOutcomeV1,
    initial: WakeDispatchOutcome,
) -> WakeDispatchOutcome {
    let mut current = initial;
    let mut consecutive_short_attempts = if current == WakeDispatchOutcome::RetrySoon {
        1usize
    } else {
        0usize
    };
    loop {
        match current {
            WakeDispatchOutcome::Confirmed | WakeDispatchOutcome::Terminal => return current,
            WakeDispatchOutcome::RetrySoon => {
                if consecutive_short_attempts >= WAKE_SHORT_RETRY_LIMIT {
                    return WakeDispatchOutcome::RetrySoon;
                }
                tokio::time::sleep(WAKE_SHORT_RETRY_DELAY).await;
            }
            WakeDispatchOutcome::RetryAfter(delay) => {
                tokio::time::sleep(delay).await;
            }
        }
        let dispatch_workspace = workspace.clone();
        let dispatch_session = session_id.clone();
        let dispatch_outcome = outcome.clone();
        current = tokio::task::spawn_blocking(move || {
            dispatch_actionable_wake(&dispatch_workspace, &dispatch_session, &dispatch_outcome)
        })
        .await
        .unwrap_or(WakeDispatchOutcome::RetrySoon);
        consecutive_short_attempts = if current == WakeDispatchOutcome::RetrySoon {
            consecutive_short_attempts.saturating_add(1).max(1)
        } else {
            0
        };
    }
}

/// Claim one approved dependency-ready task for direct ChatGPT execution
/// without launching, resuming, or probing Codex/Qwen. This persists the
/// task-output baseline and execution ownership before direct edits begin.
pub async fn claim_direct_chatgpt_work(
    workspace: &Path,
    session_id: &str,
) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let key = format!("{}:{session_id}", workspace.to_string_lossy());
    let mut registry = registry().lock().await;
    if let Some(controller) = registry.get_mut(&key) {
        return controller.claim_direct_chatgpt_work(now_unix());
    }
    drop(registry);

    let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    let contract = store
        .load_contract(session_id)
        .map_err(RuntimeError::from)?;
    let policy = AutonomousPolicyEngineV1::new(contract).map_err(|error| {
        RuntimeError::Validation(format!("persisted autonomous contract rejected: {error:?}"))
    })?;
    let verifier = ContractVerifierV1::new(policy.clone())?;
    let provider = FakeWorkerProviderV1::new(FakeProvider::new(Vec::new()));
    let mut controller =
        AutonomousControllerV1::new(policy, store, provider, verifier, session_id.into());
    controller.claim_direct_chatgpt_work(now_unix())
}

/// Verify and finalize work performed directly by ChatGPT after an existing
/// autonomous task escalated to WAITING_FOR_CHATGPT. This path never discovers,
/// launches, resumes, or probes Codex/Qwen. If a live controller is already
/// resident we reuse it; after a restart a finalization-only fake provider is
/// used solely to satisfy the controller's generic type and is never invoked.
pub async fn finalize_direct_chatgpt_work(
    workspace: &Path,
    session_id: &str,
) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let key = format!("{}:{session_id}", workspace.to_string_lossy());
    let mut registry = registry().lock().await;
    let resident_outcome = if let Some(controller) = registry.get_mut(&key) {
        Some(controller.finalize_direct_chatgpt_work(now_unix())?)
    } else {
        None
    };
    drop(registry);

    let outcome = if let Some(outcome) = resident_outcome {
        outcome
    } else {
        let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
            .map_err(RuntimeError::from)?;
        let contract = store
            .load_contract(session_id)
            .map_err(RuntimeError::from)?;
        let policy = AutonomousPolicyEngineV1::new(contract).map_err(|error| {
            RuntimeError::Validation(format!("persisted autonomous contract rejected: {error:?}"))
        })?;
        let verifier = ContractVerifierV1::new(policy.clone())?;
        let provider = FakeWorkerProviderV1::new(FakeProvider::new(Vec::new()));
        let mut controller =
            AutonomousControllerV1::new(policy, store, provider, verifier, session_id.into());
        controller.finalize_direct_chatgpt_work(now_unix())?
    };

    let immediate_wake = matches!(
        outcome.state,
        AutonomousSessionStateV1::CompletedVerified | AutonomousSessionStateV1::WaitingForChatgpt
    )
    .then(|| dispatch_actionable_wake(&workspace, session_id, &outcome));
    ensure_reviewer_wakeup(
        key,
        workspace,
        session_id.into(),
        immediate_wake,
        outcome.clone(),
    );
    Ok(outcome)
}

pub async fn start_or_tick(
    workspace: &Path,
    session_id: &str,
) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let key = format!("{}:{session_id}", workspace.to_string_lossy());
    let preflight_store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    let preflight_snapshot = preflight_store
        .load_session(session_id)
        .map_err(RuntimeError::from)?;
    let post_codex_snapshot_pending = preflight_store
        .execution_accounting_store()
        .map_err(RuntimeError::from)?
        .needs_post_codex_observability(session_id)?;
    // Use the same durable ownership classifier as the controller before any
    // host Codex preflight. A queued stale WORKER_RUNNING task otherwise has
    // no dependency-ready work and used to fail before controller recovery
    // could inspect the interrupted provider span.
    if reconcile_restart_task_state(&preflight_store, session_id)?
        == RestartTaskReconciliationV1::Pending
    {
        return Err(RuntimeError::Validation(
            "restart task reconciliation is pending authoritative ownership evidence".into(),
        ));
    }
    // A fresh host preflight is required at every Codex task boundary. Do not
    // open an app-server while a worker owns a live turn, and do not request
    // Codex metadata while the approved route is already local Qwen.
    if matches!(
        preflight_snapshot.state,
        super::autonomy_state::AutonomousSessionStateV1::Queued
            | super::autonomy_state::AutonomousSessionStateV1::RateLimited
    ) && preflight_snapshot.provider_route
        != super::autonomy_state::AutonomousProviderRouteV1::QwenFallbackActive
    {
        host_prepare_codex_app_server_thread(&workspace, session_id, now_unix())?;
    }
    let mut registry = registry().lock().await;
    if !registry.contains_key(&key) {
        let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
            .map_err(RuntimeError::from)?;
        // This process owns no previous child process. Mark any durable
        // in-flight state for continuity validation before rehydrating it.
        let recovered = store
            .recover_interrupted_sessions()
            .map_err(RuntimeError::from)?;
        let accounting = store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?;
        for snapshot in recovered {
            accounting.interrupt_open_spans(&snapshot.session_id)?;
        }
        let contract = store
            .load_contract(session_id)
            .map_err(RuntimeError::from)?;
        let policy = AutonomousPolicyEngineV1::new(contract).map_err(|error| {
            RuntimeError::Validation(format!("persisted autonomous contract rejected: {error:?}"))
        })?;
        let codex = CodexCliProviderV1::discover(operator_config(
            &workspace,
            policy.contract().provider_policy.primary_model.clone(),
        )?)
        .await?;
        let qwen = OllamaWorkerProviderV1::new(&local_ollama_url()?, Some("5m".into()))?;
        let provider = CodexQwenWorkerProviderV1::new(codex, qwen);
        let verifier = ContractVerifierV1::new(policy.clone())?;
        registry.insert(
            key.clone(),
            AutonomousControllerV1::new(policy, store, provider, verifier, session_id.into()),
        );
    }
    let outcome = registry
        .get_mut(&key)
        .expect("controller was inserted")
        .run_once(now_unix())
        .await?;
    drop(registry);
    // A successful Codex provider turn has reached verification before this
    // point.  Capture exactly one read-only post-turn account snapshot only
    // after that boundary; failed/retried turns and local-Qwen turns neither
    // probe Codex nor manufacture a post-turn measurement.
    if should_refresh_post_codex_accounting(
        preflight_snapshot.provider_route,
        post_codex_snapshot_pending,
        &outcome,
    ) {
        // Accounting telemetry is authoritative only when this read succeeds.
        // Its absence is already represented by UNKNOWN_NOT_CAPTURED in the
        // durable ledger and must not alter provider routing or a successful
        // controller outcome.
        let _ = host_refresh_codex_app_server_accounting(&workspace, session_id, now_unix());
    }
    let immediate_wake = matches!(
        outcome.state,
        AutonomousSessionStateV1::CompletedVerified | AutonomousSessionStateV1::WaitingForChatgpt
    )
    .then(|| dispatch_actionable_wake(&workspace, session_id, &outcome));
    ensure_reviewer_wakeup(
        key,
        workspace,
        session_id.into(),
        immediate_wake,
        outcome.clone(),
    );
    Ok(outcome)
}

fn should_refresh_post_codex_accounting(
    route_before_turn: AutonomousProviderRouteV1,
    post_snapshot_pending_before_tick: bool,
    outcome: &AutonomousControllerOutcomeV1,
) -> bool {
    route_before_turn != AutonomousProviderRouteV1::QwenFallbackActive
        && (outcome.verification.is_some()
            || (post_snapshot_pending_before_tick
                && outcome.state == AutonomousSessionStateV1::CompletedVerified))
}

/// Re-arms a durably accepted QUEUED session using only CatDesk's local
/// operator configuration.  This is deliberately separate from the durable
/// supervisor mutation: callers invoke it only after the reply/resume record
/// is committed, so a rejected or failed control-plane mutation can never
/// start a provider.
///
/// The registry serializes controller ticks.  Bumping the reviewer generation
/// additionally closes the narrow race where the former reviewer loop had
/// observed WAITING_FOR_CHATGPT and was about to remove its wakeup key.
pub async fn rearm_accepted_queued_session(
    workspace: &Path,
    session_id: &str,
) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
        .map_err(RuntimeError::from)?;
    let snapshot = store.load_session(session_id).map_err(RuntimeError::from)?;
    if !matches!(
        snapshot.state,
        AutonomousSessionStateV1::Queued
            | AutonomousSessionStateV1::RateLimited
            | AutonomousSessionStateV1::Running
    ) {
        return Err(RuntimeError::Validation(
            "accepted continuation requires a queued, rate-limited, or live autonomous session"
                .into(),
        ));
    }
    let key = format!("{}:{session_id}", workspace.to_string_lossy());
    request_reviewer_rearm(&key).await;
    start_or_tick(&workspace, session_id).await
}

async fn request_reviewer_rearm(key: &str) {
    let mut wakeups = reviewer_wakeups().lock().await;
    request_reviewer_rearm_locked(&mut wakeups, key);
}

fn ensure_reviewer_wakeup(
    key: String,
    workspace: PathBuf,
    session_id: String,
    initial_wake: Option<WakeDispatchOutcome>,
    initial_outcome: AutonomousControllerOutcomeV1,
) {
    tokio::spawn(async move {
        let mut observed_generation = {
            let mut wakeups = reviewer_wakeups().lock().await;
            let Some(generation) = install_reviewer_wakeup_if_absent(&mut wakeups, &key) else {
                return;
            };
            generation
        };

        if let Some(initial_wake) = initial_wake {
            if matches!(
                initial_wake,
                WakeDispatchOutcome::RetrySoon | WakeDispatchOutcome::RetryAfter(_)
            ) {
                let _ = continue_wake_retries(
                    workspace.clone(),
                    session_id.clone(),
                    initial_outcome.clone(),
                    initial_wake,
                )
                .await;
            }
            match retire_or_adopt_reviewer_wakeup(&key, observed_generation).await {
                // The re-arm arrived while this initial wake was completing.
                // Keep the one existing owner and enter its normal ticker.
                ReviewerWakeupRetirement::Continue(generation) => observed_generation = generation,
                ReviewerWakeupRetirement::Retired | ReviewerWakeupRetirement::OwnershipLost => {
                    return;
                }
            }
        }

        let mut ticker = interval(Duration::from_secs(REVIEWER_WAKEUP_SECONDS));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let outcome = {
                let mut registry = registry().lock().await;
                match registry.get_mut(&key) {
                    Some(controller) => Some(controller.run_once(now_unix()).await),
                    None => None,
                }
            };
            let Some(outcome) = outcome else {
                match retire_or_adopt_reviewer_wakeup(&key, observed_generation).await {
                    ReviewerWakeupRetirement::Continue(generation) => {
                        observed_generation = generation;
                        continue;
                    }
                    ReviewerWakeupRetirement::Retired | ReviewerWakeupRetirement::OwnershipLost => {
                        return;
                    }
                }
            };
            match outcome {
                Ok(outcome)
                    if outcome.state.is_terminal()
                        || matches!(
                            outcome.state,
                            super::autonomy_state::AutonomousSessionStateV1::WaitingForChatgpt
                                | super::autonomy_state::AutonomousSessionStateV1::WaitingForUser
                        ) =>
                {
                    let dispatch_workspace = workspace.clone();
                    let dispatch_session = session_id.clone();
                    let dispatch_outcome = outcome.clone();
                    let initial = tokio::task::spawn_blocking(move || {
                        dispatch_actionable_wake(
                            &dispatch_workspace,
                            &dispatch_session,
                            &dispatch_outcome,
                        )
                    })
                    .await
                    .unwrap_or(WakeDispatchOutcome::RetrySoon);
                    if matches!(
                        initial,
                        WakeDispatchOutcome::RetrySoon | WakeDispatchOutcome::RetryAfter(_)
                    ) {
                        let _ = continue_wake_retries(
                            workspace.clone(),
                            session_id.clone(),
                            outcome.clone(),
                            initial,
                        )
                        .await;
                    }
                    match retire_or_adopt_reviewer_wakeup(&key, observed_generation).await {
                        ReviewerWakeupRetirement::Continue(generation) => {
                            observed_generation = generation;
                            continue;
                        }
                        ReviewerWakeupRetirement::Retired
                        | ReviewerWakeupRetirement::OwnershipLost => return,
                    }
                }
                Ok(_) => {}
                Err(_) => match retire_or_adopt_reviewer_wakeup(&key, observed_generation).await {
                    ReviewerWakeupRetirement::Continue(generation) => {
                        observed_generation = generation;
                    }
                    ReviewerWakeupRetirement::Retired | ReviewerWakeupRetirement::OwnershipLost => {
                        return;
                    }
                },
            }
        }
    });
}

pub async fn cancel_owned_turn(workspace: &Path, session_id: &str) -> Result<bool, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let key = format!("{}:{session_id}", workspace.to_string_lossy());
    let mut registry = registry().lock().await;
    if let Some(controller) = registry.get_mut(&key) {
        controller.cancel().await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn operator_configuration_available(
    workspace: &Path,
    model_id: String,
) -> Result<(), RuntimeError> {
    operator_config(workspace, model_id).map(|_| ())
}

fn operator_config(workspace: &Path, model_id: String) -> Result<CodexCliConfigV1, RuntimeError> {
    if model_id != super::codex_app_server::CATDESK_REQUIRED_CODEX_MODEL_V1 {
        return Err(RuntimeError::Validation(
            "Codex implementation policy requires gpt-5.6-terra".into(),
        ));
    }
    let executable = resolve_codex_executable()?;
    let mut config = CodexCliConfigV1::new(executable, workspace.into())?;
    config.operator_codex_home = operator_codex_home()?;
    config.model_id = Some(model_id);
    config.sandbox = CodexCliSandboxV1::WorkspaceWrite;
    config.diagnostic_root = Some(
        workspace
            .join(".catdesk")
            .join("autonomy")
            .join("diagnostics"),
    );
    Ok(config)
}

fn host_app_server_launch_config(
    workspace: &Path,
) -> Result<CodexAppServerLaunchConfigV1, RuntimeError> {
    CodexAppServerLaunchConfigV1::new(
        resolve_codex_executable()?,
        workspace.to_path_buf(),
        operator_codex_home()?,
    )
}

/// Normal operation inherits the current user's Codex identity/config/history
/// without inspecting it. Advanced recovery may explicitly override only the
/// executable path; the override is validated as a direct executable.
fn resolve_codex_executable() -> Result<PathBuf, RuntimeError> {
    let override_path = std::env::var_os("CATDESK_CODEX_CLI_EXECUTABLE").map(PathBuf::from);
    let path = std::env::var_os("PATH").ok_or_else(|| {
        RuntimeError::Validation("current-user PATH is unavailable for Codex discovery".into())
    })?;
    resolve_codex_executable_from_paths(override_path, &path)
}

/// Resolves a directly-launchable Codex binary from the ordinary current-user
/// PATH. Windows npm commonly provides `codex.cmd`; CatDesk never executes or
/// parses that shell launcher. Instead it considers only the bounded native
/// binary locations directly below that launcher's known package root.
fn resolve_codex_executable_from_paths(
    override_path: Option<PathBuf>,
    path: &OsStr,
) -> Result<PathBuf, RuntimeError> {
    if let Some(override_path) = override_path {
        if is_direct_codex_executable(&override_path) {
            return Ok(override_path);
        }
        return Err(RuntimeError::Validation(
            "CATDESK_CODEX_CLI_EXECUTABLE must identify an existing direct executable, not a shell launcher".into(),
        ));
    }
    for directory in std::env::split_paths(&path) {
        for executable in ["codex.exe", "codex"] {
            let candidate = directory.join(executable);
            if is_direct_codex_executable(&candidate) {
                return Ok(candidate);
            }
        }
        for launcher in ["codex.cmd", "codex.ps1", "codex.bat"] {
            if let Some(native) = resolve_npm_codex_launcher(&directory.join(launcher)) {
                return Ok(native);
            }
        }
    }
    Err(RuntimeError::Validation(
        "current-user Codex executable was not found; CATDESK_CODEX_CLI_EXECUTABLE is an optional recovery override".into(),
    ))
}

fn is_direct_codex_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "cmd" | "bat" | "ps1") {
        return false;
    }
    // npm installs an extensionless `codex` companion beside its Windows
    // `.cmd` launcher. It is a script shim, not a native image. Do not let it
    // preempt the bounded native resolution below merely because it lacks an
    // extension; a normal extensionless executable remains valid elsewhere.
    !(extension.is_empty()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("codex"))
        && ["cmd", "bat", "ps1"]
            .iter()
            .any(|launcher_extension| path.with_extension(launcher_extension).is_file()))
}

/// Resolves only bounded package-local native layouts emitted by the
/// current-user npm Codex launcher. This is intentionally not a general
/// launcher interpreter: no `.cmd`, PowerShell, JavaScript, package metadata,
/// or user configuration/auth content is opened.
fn resolve_npm_codex_launcher(launcher: &Path) -> Option<PathBuf> {
    let extension = launcher
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "cmd" | "bat" | "ps1") || !launcher.is_file() {
        return None;
    }
    let launcher_name = launcher.file_stem()?.to_str()?;
    if !launcher_name.eq_ignore_ascii_case("codex") {
        return None;
    }
    let package_root = launcher
        .parent()?
        .join("node_modules")
        .join("@openai")
        .join("codex");
    let native_candidates = [
        package_root
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe"),
        package_root
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-arm64")
            .join("vendor")
            .join("aarch64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe"),
        // Retain the earlier package-local layout for supported installations
        // that have not yet adopted platform packages.
        package_root
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("codex")
            .join("codex.exe"),
        package_root
            .join("vendor")
            .join("aarch64-pc-windows-msvc")
            .join("codex")
            .join("codex.exe"),
    ];
    native_candidates
        .into_iter()
        .find(|candidate| is_direct_codex_executable(candidate))
}

/// Accepts only an operator-provided directory path for the existing Codex
/// context. CatDesk does not inspect or copy auth/config files: the path is
/// passed to its directly launched Codex children as `CODEX_HOME`.
fn operator_codex_home() -> Result<Option<PathBuf>, RuntimeError> {
    let Some(value) = std::env::var_os("CATDESK_CODEX_HOME") else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if !path.is_dir() {
        return Err(RuntimeError::Validation(
            "operator-local CATDESK_CODEX_HOME must identify an existing directory".into(),
        ));
    }
    Ok(Some(path))
}

fn local_ollama_url() -> Result<String, RuntimeError> {
    let url = std::env::var("CATDESK_OLLAMA_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434".into());
    let parsed = reqwest::Url::parse(&url).map_err(|_| {
        RuntimeError::Validation("CATDESK_OLLAMA_BASE_URL is not a valid URL".into())
    })?;
    if parsed.scheme() != "http"
        || !matches!(
            parsed.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("::1")
        )
    {
        return Err(RuntimeError::Validation(
            "Ollama fallback must use a loopback http endpoint".into(),
        ));
    }
    Ok(url)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::ffi::OsString;

    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::*;
    use crate::delegated::autonomy_state::{
        AutonomousQueueTaskStateV1, AutonomousQueueTaskV1, AutonomousQueueV1,
    };

    struct FakeTransport {
        replies: VecDeque<Value>,
        methods: Vec<String>,
    }
    impl CodexAppServerTransportV1 for FakeTransport {
        fn request(&mut self, method: &str, _params: Value) -> Result<Value, RuntimeError> {
            self.methods.push(method.into());
            self.replies
                .pop_front()
                .ok_or_else(|| RuntimeError::Validation("missing fake response".into()))
        }
    }

    #[test]
    fn wake_exit_diagnostics_split_short_coordination_from_chat_busy_delay() {
        assert_eq!(
            classify_wake_exit(Some(3), "WAKE_BRIDGE_BUSY"),
            WakeDispatchOutcome::RetrySoon
        );
        assert_eq!(
            classify_wake_exit(Some(3), "CHATGPT_NOT_IDLE"),
            WakeDispatchOutcome::RetryAfter(Duration::from_secs(300))
        );
        for code in [Some(0), Some(2), Some(4), Some(5), Some(17), None] {
            assert_eq!(
                classify_wake_exit(code, "UNRECOGNIZED"),
                WakeDispatchOutcome::Terminal
            );
        }
        assert_eq!(
            classify_wake_exit(Some(3), "UNRECOGNIZED"),
            WakeDispatchOutcome::Terminal,
            "exit code 3 alone is never enough to authorize a retry class"
        );
    }

    #[test]
    fn automatic_wake_dispatch_selects_one_owner_before_legacy_bridge_invocation() {
        let source = include_str!("autonomy_runtime.rs");
        let dispatch = source
            .split("fn dispatch_actionable_wake_with_policy")
            .nth(1)
            .expect("automatic wake dispatcher exists")
            .split("/// Dispatch an already-persisted delegated completion review")
            .next()
            .expect("dispatcher ends before retry worker");
        let selector = dispatch
            .find("match selected_owner(workspace)")
            .expect("selector gate exists");
        let legacy_bridge = dispatch
            .find("Command::new(&python)")
            .expect("legacy bridge invocation exists");

        assert!(
            selector < legacy_bridge,
            "the selector must be evaluated before the legacy bridge can run"
        );
        assert!(dispatch.contains("WakeOwnerMode::LegacyPython"));
        assert!(dispatch.contains("WakeOwnerMode::Rust"));
        assert!(dispatch.contains("dispatch_if_rust_selected"));
        assert!(
            dispatch.contains("Err(_) => return WakeDispatchOutcome::Terminal"),
            "malformed selector authority must fail closed rather than use legacy"
        );
    }

    #[tokio::test]
    async fn accepted_rearm_latches_a_live_reviewer_loop_generation() {
        let key = format!("reviewer-rearm-test-{}", Uuid::new_v4());
        reviewer_wakeups().lock().await.insert(key.clone(), 7);
        request_reviewer_rearm(&key).await;
        assert_eq!(reviewer_wakeups().lock().await.get(&key), Some(&8));
        // A replay is intentionally another harmless wake signal; controller
        // ticks remain serialized by the runtime registry.
        request_reviewer_rearm(&key).await;
        assert_eq!(reviewer_wakeups().lock().await.get(&key), Some(&9));
        reviewer_wakeups().lock().await.remove(&key);
    }

    #[test]
    fn unchanged_generation_atomically_retires_and_removes_the_owner() {
        let mut wakeups = BTreeMap::from([(String::from("reviewer"), 4)]);

        assert_eq!(
            retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, "reviewer", 4),
            ReviewerWakeupRetirement::Retired
        );
        assert!(wakeups.is_empty());
    }

    #[test]
    fn newer_generation_atomically_adopts_and_continues() {
        let mut wakeups = BTreeMap::from([(String::from("reviewer"), 5)]);

        assert_eq!(
            retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, "reviewer", 4),
            ReviewerWakeupRetirement::Continue(5)
        );
        assert_eq!(wakeups.get("reviewer"), Some(&5));
    }

    #[test]
    fn initial_wake_rearm_before_retirement_adopts_the_new_generation() {
        let mut wakeups = BTreeMap::from([(String::from("reviewer"), 0)]);
        // This is the deterministic interleave: initial wake finishes its
        // dispatch only after reply-side re-arm has bumped the generation.
        request_reviewer_rearm_locked(&mut wakeups, "reviewer");

        assert_eq!(
            retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, "reviewer", 0),
            ReviewerWakeupRetirement::Continue(1)
        );
        assert_eq!(wakeups.get("reviewer"), Some(&1));
    }

    #[test]
    fn normal_terminal_rearm_before_retirement_adopts_the_new_generation() {
        let mut wakeups = BTreeMap::from([(String::from("reviewer"), 12)]);
        // A terminal/WAITING tick has completed its dispatch while a durable
        // reply/resume re-arm wins before the owner retirement transition.
        request_reviewer_rearm_locked(&mut wakeups, "reviewer");

        assert_eq!(
            retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, "reviewer", 12),
            ReviewerWakeupRetirement::Continue(13)
        );
        assert_eq!(wakeups.get("reviewer"), Some(&13));
    }

    #[test]
    fn rearm_after_atomic_retirement_can_register_a_fresh_loop() {
        let mut wakeups = BTreeMap::from([(String::from("reviewer"), 9)]);
        assert_eq!(
            retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, "reviewer", 9),
            ReviewerWakeupRetirement::Retired
        );
        // A subsequent re-arm sees no live key.  Its start_or_tick call can
        // therefore be the sole installer of the new reviewer loop.
        request_reviewer_rearm_locked(&mut wakeups, "reviewer");
        assert_eq!(
            install_reviewer_wakeup_if_absent(&mut wakeups, "reviewer"),
            Some(0)
        );
        assert_eq!(
            install_reviewer_wakeup_if_absent(&mut wakeups, "reviewer"),
            None
        );
    }

    #[test]
    fn replayed_rearms_keep_one_logical_owner_and_never_install_a_second_loop() {
        let mut wakeups = BTreeMap::new();
        assert_eq!(
            install_reviewer_wakeup_if_absent(&mut wakeups, "reviewer"),
            Some(0)
        );
        request_reviewer_rearm_locked(&mut wakeups, "reviewer");
        request_reviewer_rearm_locked(&mut wakeups, "reviewer");

        assert_eq!(
            install_reviewer_wakeup_if_absent(&mut wakeups, "reviewer"),
            None
        );
        assert_eq!(
            retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, "reviewer", 0),
            ReviewerWakeupRetirement::Continue(2)
        );
        // Provider turns remain serialized by the independent runtime
        // registry; wake replays only advance this one logical owner.
        assert_eq!(wakeups.len(), 1);
    }

    #[test]
    fn stale_or_missing_ownership_fails_safe_without_deleting_a_registration() {
        let mut wakeups = BTreeMap::from([(String::from("reviewer"), 3)]);
        assert_eq!(
            retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, "reviewer", 4),
            ReviewerWakeupRetirement::OwnershipLost
        );
        assert_eq!(wakeups.get("reviewer"), Some(&3));
        assert_eq!(
            retire_or_adopt_reviewer_wakeup_locked(&mut wakeups, "missing", 0),
            ReviewerWakeupRetirement::OwnershipLost
        );
        assert_eq!(wakeups.get("reviewer"), Some(&3));
    }

    #[test]
    fn automatic_dispatch_binds_a_bounded_exact_bridge_revision() {
        let root = wake_receipt_root("bridge-revision");
        let bridge = root.join("scripts").join("wake_bridge.py");
        fs::create_dir_all(bridge.parent().expect("bridge parent")).expect("bridge parent");
        fs::write(&bridge, b"print('authoritative bridge')\n").expect("bridge write");
        let first = authoritative_bridge_sha256(&bridge).expect("bounded bridge hash");
        fs::write(&bridge, b"print('different bridge')\n").expect("bridge rewrite");
        let second = authoritative_bridge_sha256(&bridge).expect("changed bridge hash");
        assert_ne!(first, second);
        fs::write(
            &bridge,
            vec![b'x'; (WAKE_BRIDGE_SCRIPT_MAX_BYTES + 1) as usize],
        )
        .expect("oversized bridge write");
        assert!(authoritative_bridge_sha256(&bridge).is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wake_receipt_validation_uses_the_same_exact_chatgpt_target_shape() {
        assert_eq!(
            canonical_wake_target("https://chatgpt.com/c/exact-thread").as_deref(),
            Some("https://chatgpt.com/c/exact-thread")
        );
        assert_eq!(
            canonical_wake_target("https://chatgpt.com/g/project-1/c/exact-thread").as_deref(),
            Some("https://chatgpt.com/g/project-1/c/exact-thread")
        );
        let web_target = "https://chatgpt.com/c/WEB:1229263e-88d1-41a1-8ce2-b4aa97fbcb0f";
        assert_eq!(
            canonical_wake_target(web_target).as_deref(),
            Some(web_target)
        );
        for invalid in [
            "https://chatgpt.com/c/WEB:",
            "https://chatgpt.com/c/web:exact-thread",
            "https://chatgpt.com/c/WEB:exact:thread",
            "https://chatgpt.com/g/WEB:project/c/exact-thread",
            "https://chatgpt.com/c/exact-thread/",
            "https://chatgpt.com/c/exact-thread?query=value",
            "https://user@chatgpt.com/c/exact-thread",
            "https://chatgpt.com:444/c/exact-thread",
            "https://chatgpt.com/share/exact-thread",
            "https://chatgpt.com/g/project/c/exact-thread/extra",
        ] {
            assert!(canonical_wake_target(invalid).is_none(), "{invalid}");
        }
    }

    fn wake_receipt_root(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("catdesk-wake-receipt-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join(".catdesk").join("wake-bridge")).expect("wake root");
        root
    }

    fn register_wake_project(root: &Path, id: &str, target: Option<&str>) {
        let store = AutonomousProjectRegistryStoreV1::open(root.join(".catdesk").join("projects"))
            .expect("store");
        store.initialize(1, 1).expect("init");
        let mut project = AutonomousProjectV1 {
            project_id: id.into(),
            workspace: root.canonicalize().expect("workspace"),
            git_identity: format!("fixture-{id}"),
            verification_profile: "rust_full".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        if let Some(target) = target {
            let target = super::super::autonomy_projects::canonical_project_chat_target(target)
                .expect("target");
            project.chatgpt_target_sha256 = Some(
                super::super::autonomy_projects::project_chat_target_digest(&target),
            );
            project.chatgpt_target_url = Some(target);
        }
        store.register_project(project).expect("project");
    }

    #[test]
    fn workspace_bound_wake_lookup_cannot_select_a_registered_sibling_target() {
        let root = wake_receipt_root("sibling-target-isolation");
        let sibling = root
            .parent()
            .expect("temp parent")
            .join(format!("catdesk-wake-sibling-{}", Uuid::new_v4()));
        fs::create_dir_all(&sibling).expect("sibling workspace");
        register_wake_project(&root, "alpha", Some("https://chatgpt.com/c/alpha-target"));
        let store = AutonomousProjectRegistryStoreV1::open(root.join(".catdesk").join("projects"))
            .expect("registry");
        let sibling_target = "https://chatgpt.com/c/beta-target";
        let canonical_target =
            super::super::autonomy_projects::canonical_project_chat_target(sibling_target)
                .expect("canonical sibling target");
        store
            .register_project(AutonomousProjectV1 {
                project_id: "beta".into(),
                workspace: sibling.canonicalize().expect("canonical sibling"),
                git_identity: "fixture-beta".into(),
                verification_profile: "rust_full".into(),
                codex_thread_id: Some("beta-thread".into()),
                chatgpt_target_sha256: Some(
                    super::super::autonomy_projects::project_chat_target_digest(&canonical_target),
                ),
                chatgpt_target_url: Some(canonical_target),
            })
            .expect("register sibling");

        assert_eq!(
            project_wake_target(&root, "alpha").as_deref(),
            Some("https://chatgpt.com/c/alpha-target")
        );
        assert!(
            project_wake_target(&root, "beta").is_none(),
            "a root-bound delegated operation cannot route through sibling beta's target"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(sibling);
    }

    fn create_restart_waiting_review(
        root: &Path,
        session_id: &str,
        project_id: &str,
        active_waiting: bool,
    ) -> String {
        let store = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("autonomy store");
        store
            .create_session(
                session_id,
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: format!("task-{session_id}"),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WaitingForChatgpt,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session(session_id).expect("snapshot");
        if active_waiting {
            snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        } else {
            snapshot.state = AutonomousSessionStateV1::CompletedVerified;
            snapshot.active = false;
        }
        store.save_session(&snapshot).expect("persist state");
        let (review_state, next_action) = if active_waiting {
            (
                AutonomousSessionStateV1::WaitingForChatgpt,
                "chatgpt_decision_required",
            )
        } else {
            (
                AutonomousSessionStateV1::CompletedVerified,
                "independent_final_review",
            )
        };
        store
            .emit_review_inbox_record(
                session_id,
                project_id,
                review_state,
                next_action,
                "artifacts/restart-wake-review.json",
                10,
            )
            .expect("review record")
            .record_id
    }

    #[test]
    fn delegated_terminal_review_handoff_is_project_bound_and_restart_idempotent() {
        let root = wake_receipt_root("delegated-review-restart");
        let target = "https://chatgpt.com/c/delegated-review-target";
        register_wake_project(&root, "catdesk", Some(target));
        let project_store =
            AutonomousProjectRegistryStoreV1::open(root.join(".catdesk").join("projects"))
                .expect("project store");
        let before = project_store.load_registry().expect("project registry");
        let final_review_sha256 = "a".repeat(64);

        let first = emit_delegated_review_inbox(
            &root,
            "delegated-run.terminal:one",
            &final_review_sha256,
            "sha256:delegated-diff",
        )
        .expect("first durable handoff");
        let replay = emit_delegated_review_inbox(
            &root,
            "delegated-run.terminal:one",
            &final_review_sha256,
            "sha256:delegated-diff",
        )
        .expect("restart replay");

        assert_eq!(first, replay, "same final evidence reuses one record");
        assert_eq!(first.project_id, "catdesk");
        assert_eq!(first.state, AutonomousSessionStateV1::CompletedVerified);
        assert_eq!(first.next_action, "independent_final_review");
        assert!(
            first
                .reference
                .contains("delegated-run=delegated-run.terminal:one")
        );
        assert!(first.reference.contains(&final_review_sha256));
        assert!(first.reference.contains("diff=sha256:delegated-diff"));
        assert!(first.reference.contains(&wake_digest(target)));

        let reopened = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("reopened state store");
        assert_eq!(
            reopened
                .all_review_inbox()
                .expect("reopened inbox")
                .iter()
                .filter(|record| record.record_id == first.record_id)
                .count(),
            1,
            "restart/replay cannot duplicate the delegated handoff"
        );
        let autonomous_record =
            create_restart_waiting_review(&root, "autonomous-session-review", "catdesk", true);
        assert_ne!(autonomous_record, first.record_id);
        assert_eq!(
            reopened
                .latest_actionable_review_for_session("autonomous-session-review")
                .expect("autonomous review")
                .expect("active autonomous review")
                .state,
            AutonomousSessionStateV1::WaitingForChatgpt,
            "delegated compatibility identity must not alter autonomous review semantics"
        );
        let after = project_store
            .load_registry()
            .expect("project registry after handoff");
        assert_eq!(
            before, after,
            "handoff reads but never retargets the project"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delegated_terminal_review_restart_candidate_is_exact_and_does_not_adopt_autonomous_review() {
        let root = wake_receipt_root("delegated-review-restart-candidate");
        register_wake_project(
            &root,
            "catdesk",
            Some("https://chatgpt.com/c/delegated-restart-target"),
        );
        create_restart_waiting_review(&root, "autonomous-final-review", "catdesk", false);
        emit_delegated_review_inbox(
            &root,
            "delegated-run.restart:one",
            &"d".repeat(64),
            "sha256:delegated-diff",
        )
        .expect("delegated handoff");

        assert_eq!(
            persisted_delegated_review_wake_candidates(&root).expect("delegated candidates"),
            vec!["delegated-run.restart:one".to_string()],
            "only an unread exact delegated review is restart-rehydrated"
        );
        assert!(
            persisted_waiting_wake_candidates(&root)
                .expect("autonomous candidates")
                .is_empty(),
            "a completed autonomous final review keeps its existing non-waiting behavior"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delegated_retry_exhaustion_survives_restart_without_duplicate_handoff_or_dispatch() {
        let root = wake_receipt_root("delegated-retry-exhausted");
        let target = "https://chatgpt.com/c/delegated-retry-exhausted";
        register_wake_project(&root, "catdesk", Some(target));
        let handoff = emit_delegated_review_inbox(
            &root,
            "delegated-run.retry:exhausted",
            &"f".repeat(64),
            "sha256:delegated-retry-diff",
        )
        .expect("delegated handoff");
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let mut now = now_unix().saturating_sub(
            WAKE_CHAT_BUSY_RETRY_SECONDS.saturating_mul(u64::from(WAKE_CHAT_BUSY_RETRY_LIMIT)),
        );
        for _ in 1..=WAKE_CHAT_BUSY_RETRY_LIMIT {
            let schedule = schedule_chat_busy_retry(&wake_root, &handoff.record_id, target, now)
                .expect("bounded pre-submit retry");
            now = schedule.not_before_unix;
        }

        assert!(
            persisted_delegated_review_wake_candidates(&root)
                .expect("durable restart classification")
                .is_empty(),
            "an exhausted pre-submit journal must not repeatedly schedule a terminal bridge task"
        );
        let store = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("state store");
        assert_eq!(
            store
                .all_review_inbox()
                .expect("review inbox")
                .into_iter()
                .filter(|record| record.record_id == handoff.record_id && record.unread)
                .count(),
            1,
            "exhaustion retains the one immutable delegated review for deadman/reviewer handling"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delegated_review_reference_rejects_ambiguous_or_malformed_identity() {
        let valid = delegated_review_reference(
            "delegated-run.identity:one",
            &"e".repeat(64),
            "fnv1a64:diff",
            "https://chatgpt.com/c/delegated-identity",
        )
        .expect("valid reference");
        assert_eq!(
            parse_delegated_review_reference(&valid)
                .expect("parse valid reference")
                .run_id,
            "delegated-run.identity:one"
        );
        for invalid in [
            "delegated-run=run;final-review-sha256=bad;diff=fnv1a64:diff;target-sha256=bad",
            "delegated-run=run;final-review-sha256=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee;diff=fnv1a64:diff;target-sha256=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee;extra=value",
            "delegated-run=run;final-review-sha256=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee;diff=bad\nvalue;target-sha256=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        ] {
            assert!(
                parse_delegated_review_reference(invalid).is_none(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn delegated_terminal_review_handoff_refuses_missing_target_and_conflicting_replay() {
        let missing = wake_receipt_root("delegated-review-no-target");
        register_wake_project(&missing, "catdesk", None);
        assert!(
            emit_delegated_review_inbox(
                &missing,
                "delegated-run-missing",
                &"b".repeat(64),
                "sha256:delegated-diff",
            )
            .is_err()
        );
        let missing_store = AutonomousStateStoreV1::open(missing.join(".catdesk").join("autonomy"))
            .expect("missing-target state store");
        assert!(
            missing_store
                .all_review_inbox()
                .expect("missing-target inbox")
                .is_empty(),
            "missing durable target cannot create an unbound review record"
        );

        let conflicting = wake_receipt_root("delegated-review-conflicting-replay");
        register_wake_project(
            &conflicting,
            "catdesk",
            Some("https://chatgpt.com/c/conflicting-replay-target"),
        );
        emit_delegated_review_inbox(
            &conflicting,
            "delegated-run-conflicting",
            &"c".repeat(64),
            "sha256:delegated-diff",
        )
        .expect("first conflicting fixture handoff");
        assert!(
            emit_delegated_review_inbox(
                &conflicting,
                "delegated-run-conflicting",
                &"d".repeat(64),
                "sha256:delegated-diff",
            )
            .is_err(),
            "a run id cannot be rebound to different final-review evidence"
        );
        let conflicting_store =
            AutonomousStateStoreV1::open(conflicting.join(".catdesk").join("autonomy"))
                .expect("conflicting state store");
        assert_eq!(
            conflicting_store
                .all_review_inbox()
                .expect("conflicting inbox")
                .len(),
            1,
            "conflicting replay must preserve the first durable handoff"
        );
        let _ = fs::remove_dir_all(missing);
        let _ = fs::remove_dir_all(conflicting);
    }

    #[test]
    fn restart_wake_rehydration_selects_only_active_waiting_exact_project_handoffs() {
        let root = wake_receipt_root("startup-rehydrate-selection");
        register_wake_project(
            &root,
            "catdesk",
            Some("https://chatgpt.com/c/restart-target"),
        );
        create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        create_restart_waiting_review(&root, "session-stale", "catdesk", false);

        assert_eq!(
            persisted_waiting_wake_candidates(&root).expect("restart candidates"),
            vec!["session-waiting".to_string()],
            "inactive historical review backlog must not be rehydrated"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completed_autonomous_review_rehydrates_only_for_independent_wake_owner() {
        let root = wake_receipt_root("completed-autonomous-independent-wake");
        let target = "https://chatgpt.com/c/completed-review-target";
        register_wake_project(&root, "catdesk", Some(target));
        let independent_wake_root = root.join(".catdesk").join("independent-wake-fixture");
        let independent_store =
            catdesk_wake::store::Store::open_scoped_for_test(&independent_wake_root, &root)
                .expect("independent Wake fixture store");
        independent_store
            .initialize()
            .expect("independent Wake fixture init");
        independent_store
            .set_target("catdesk", 0, target)
            .expect("independent Wake fixture target");
        fs::write(
            independent_wake_root.join("activation.json"),
            serde_json::to_vec(&catdesk_wake::runtime::Activation {
                schema_version: 1,
                owner: "CatDeskWake".into(),
                migration_audit: "fixture.json".into(),
                accept_after_utc: 1,
            })
            .expect("activation json"),
        )
        .expect("activation write");
        let store = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("autonomy store");
        let session_id = "completed-autonomous-review";
        store
            .create_session(
                session_id,
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "completed-task".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::CompletedVerified,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session(session_id).expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::CompletedVerified;
        snapshot.active = false;
        store.save_session(&snapshot).expect("persist state");
        store
            .emit_review_inbox_record(
                session_id,
                "catdesk",
                AutonomousSessionStateV1::CompletedVerified,
                "independent_final_review",
                "artifacts/completion.json",
                now_unix(),
            )
            .expect("review record");

        assert!(
            persisted_completed_review_wake_candidates_with_cutoff(&root, |project_id| {
                crate::wake_protocol_client::current_generation_accept_after_at_for_test(
                    &independent_wake_root,
                    &root,
                    project_id,
                )
            })
            .expect("legacy owner candidates")
            .is_empty(),
            "completed autonomous review must not cross into independent Wake without explicit owner selection"
        );

        let owner_path = root.join(".catdesk").join("wake-bridge").join("owner.json");
        fs::create_dir_all(owner_path.parent().expect("owner parent")).expect("owner dir");
        fs::write(
            &owner_path,
            serde_json::to_vec(&json!({"schemaVersion": 1, "owner": "independent_v1"}))
                .expect("owner json"),
        )
        .expect("owner write");

        assert_eq!(
            persisted_completed_review_wake_candidates_with_cutoff(&root, |project_id| {
                crate::wake_protocol_client::current_generation_accept_after_at_for_test(
                    &independent_wake_root,
                    &root,
                    project_id,
                )
            })
            .expect("independent owner candidates"),
            vec![session_id.to_string()],
            "one unread terminal autonomous completion is restart-recoverable for independent Wake"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_finalization_routes_terminal_outcome_through_actionable_wake_dispatch() {
        let source = include_str!("autonomy_runtime.rs");
        let section = source
            .split("pub async fn finalize_direct_chatgpt_work")
            .nth(1)
            .expect("direct finalizer")
            .split("pub async fn start_or_tick")
            .next()
            .expect("direct finalizer section");
        assert!(section.contains("dispatch_actionable_wake"));
        assert!(section.contains("ensure_reviewer_wakeup"));
        assert!(section.contains("AutonomousSessionStateV1::CompletedVerified"));
    }

    #[test]
    fn restart_wake_rehydration_fails_closed_without_exact_project_target() {
        let root = wake_receipt_root("startup-rehydrate-no-target");
        register_wake_project(&root, "catdesk", None);
        create_restart_waiting_review(&root, "session-waiting", "catdesk", true);

        assert!(
            persisted_waiting_wake_candidates(&root)
                .expect("restart candidates")
                .is_empty(),
            "a durable waiting review without an exact target must not launch a browser"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_wake_rebind_creates_successor_without_reusing_sent_record() {
        let root = wake_receipt_root("startup-rehydrate-rebind");
        register_wake_project(&root, "catdesk", Some("https://chatgpt.com/c/new-target"));
        let old_record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        write_wake_receipt(&root, &old_record, true);
        let wake_root = root.join(".catdesk").join("wake-bridge");
        assert_eq!(
            persisted_wake_receipt_state(
                &wake_root,
                &old_record,
                "https://chatgpt.com/c/new-target"
            ),
            PersistedWakeReceiptState::SentDifferentTarget
        );

        assert_eq!(
            persisted_waiting_wake_candidates(&root).expect("restart candidates"),
            vec!["session-waiting".to_string()]
        );

        let store = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("autonomy store");
        let successor = store
            .latest_actionable_review_for_session("session-waiting")
            .expect("latest review")
            .expect("successor review");
        assert_ne!(successor.record_id, old_record);
        assert_eq!(successor.project_id, "catdesk");
        assert_eq!(successor.state, AutonomousSessionStateV1::WaitingForChatgpt);
        assert_eq!(successor.next_action, "chatgpt_decision_required");
        assert!(successor.unread);
        let old = store
            .all_review_inbox()
            .expect("all reviews")
            .into_iter()
            .find(|record| record.record_id == old_record)
            .expect("old review retained");
        assert!(!old.unread, "old inbox item is retired, not redelivered");
        assert_eq!(
            persisted_wake_receipt_state(
                &wake_root,
                &old_record,
                "https://chatgpt.com/c/exact-thread"
            ),
            PersistedWakeReceiptState::SentCurrentTarget,
            "old immutable bridge receipt remains valid for its original target"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_wake_rehydration_skips_already_sent_current_target() {
        let root = wake_receipt_root("startup-rehydrate-already-sent");
        register_wake_project(&root, "catdesk", Some("https://chatgpt.com/c/exact-thread"));
        let record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        write_wake_receipt(&root, &record, true);
        assert_eq!(
            persisted_wake_receipt_state(
                &root.join(".catdesk").join("wake-bridge"),
                &record,
                "https://chatgpt.com/c/exact-thread"
            ),
            PersistedWakeReceiptState::SentCurrentTarget
        );
        assert!(
            persisted_waiting_wake_candidates(&root)
                .expect("restart candidates")
                .is_empty(),
            "a proven exact-current-target delivery must not reopen the browser"
        );
        let store = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("autonomy store");
        assert_eq!(
            store
                .all_review_inbox()
                .expect("reviews")
                .into_iter()
                .filter(|review| review.session_id == "session-waiting")
                .count(),
            1,
            "no successor is invented when the current target already received the review"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_wake_rehydration_treats_explicit_null_pre_submit_attention_as_retryable() {
        let root = wake_receipt_root("startup-rehydrate-null-attention");
        register_wake_project(&root, "catdesk", Some("https://chatgpt.com/c/exact-thread"));
        let record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        let wake_root = root.join(".catdesk").join("wake-bridge");
        fs::write(
            wake_root.join("state.json"),
            serde_json::to_vec(&json!({
                "schema_version": 4,
                "deliveries": [{
                    "record_id": record,
                    "status": "OPERATOR_ATTENTION",
                    "claimed_at_unix": 1.0,
                    "browser_sent_at_unix": null,
                    "message_sha256": null,
                    "target_sha256": null,
                    "receipt_schema_version": null,
                    "attention": "CHATGPT_NOT_IDLE"
                }],
                "operator_attention": "CHATGPT_NOT_IDLE"
            }))
            .expect("state json"),
        )
        .expect("state write");
        assert_eq!(
            persisted_wake_receipt_state(&wake_root, &record, "https://chatgpt.com/c/exact-thread"),
            PersistedWakeReceiptState::Unsent
        );
        assert_eq!(
            persisted_waiting_wake_candidates(&root).expect("restart candidates"),
            vec!["session-waiting".to_string()]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn chat_busy_retry_schedule_preserves_future_deadline_and_advances_only_when_due() {
        let root = wake_receipt_root("retry-schedule-deadline");
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let record = "review-retry-deadline";
        let target = "https://chatgpt.com/c/exact-thread";
        let first = schedule_chat_busy_retry(&wake_root, record, target, 1_000)
            .expect("first retry schedule");
        assert_eq!(first.attempt, 1);
        assert_eq!(first.not_before_unix, 1_300);
        assert_eq!(
            wake_retry_remaining(&first, 1_100),
            Duration::from_secs(200)
        );

        let duplicate = schedule_chat_busy_retry(&wake_root, record, target, 1_100)
            .expect("future schedule is reused");
        assert_eq!(
            duplicate, first,
            "duplicate owners must not push the deadline forward"
        );
        assert!(!wake_retry_schedule_path(&wake_root, record, 2).exists());

        let second = schedule_chat_busy_retry(&wake_root, record, target, 1_300)
            .expect("due schedule advances one generation");
        assert_eq!(second.attempt, 2);
        assert_eq!(second.not_before_unix, 1_600);
        assert!(wake_retry_schedule_path(&wake_root, record, 1).is_file());
        assert!(wake_retry_schedule_path(&wake_root, record, 2).is_file());
        let loaded = load_wake_retry_schedule(&wake_root, record, target, 1_350)
            .expect("valid retry journal")
            .expect("latest schedule");
        assert_eq!(loaded, second);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn chat_busy_retry_schedule_is_bounded_to_one_hour_of_deferrals() {
        let root = wake_receipt_root("retry-schedule-exhaustion");
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let record = "review-retry-exhaustion";
        let target = "https://chatgpt.com/c/exact-thread";
        let mut now = 2_000;
        for expected_attempt in 1..=WAKE_CHAT_BUSY_RETRY_LIMIT {
            let schedule = schedule_chat_busy_retry(&wake_root, record, target, now)
                .expect("bounded retry schedule");
            assert_eq!(schedule.attempt, expected_attempt);
            assert_eq!(schedule.not_before_unix, now + WAKE_CHAT_BUSY_RETRY_SECONDS);
            now = schedule.not_before_unix;
        }
        assert!(
            schedule_chat_busy_retry(&wake_root, record, target, now).is_err(),
            "the thirteenth busy deferral is refused"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wake_retry_schedule_target_mismatch_gap_and_extra_fields_fail_closed() {
        let root = wake_receipt_root("retry-schedule-validation");
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let record = "review-retry-validation";
        let target = "https://chatgpt.com/c/exact-thread";
        let first =
            schedule_chat_busy_retry(&wake_root, record, target, 3_000).expect("first schedule");
        assert!(
            load_wake_retry_schedule(
                &wake_root,
                record,
                "https://chatgpt.com/c/different-thread",
                3_010,
            )
            .is_err(),
            "a persisted retry is bound to the exact target digest"
        );

        let first_path = wake_retry_schedule_path(&wake_root, record, 1);
        let mut malformed: Value =
            serde_json::from_slice(&fs::read(&first_path).expect("schedule bytes"))
                .expect("schedule json");
        malformed
            .as_object_mut()
            .expect("schedule object")
            .insert("unexpected".into(), json!(true));
        fs::write(&first_path, serde_json::to_vec(&malformed).expect("json"))
            .expect("malformed rewrite");
        assert!(
            load_wake_retry_schedule(&wake_root, record, target, 3_010).is_err(),
            "unknown fields fail closed"
        );

        fs::write(
            &first_path,
            serde_json::to_vec(&json!({
                "schema_version": WAKE_RETRY_SCHEDULE_SCHEMA_VERSION,
                "record_id": record,
                "target_sha256": wake_digest(target),
                "not_before_unix": first.not_before_unix,
                "attempt": 1,
                "reason": "CHATGPT_NOT_IDLE"
            }))
            .expect("restored json"),
        )
        .expect("restore schedule");
        let second = schedule_chat_busy_retry(&wake_root, record, target, first.not_before_unix)
            .expect("second schedule");
        assert_eq!(second.attempt, 2);
        fs::remove_file(&first_path).expect("create journal gap");
        assert!(
            load_wake_retry_schedule(&wake_root, record, target, second.not_before_unix).is_err(),
            "append-only retry journals may not contain generation gaps"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_rehydrates_future_retry_and_sent_receipt_retires_its_journal() {
        let root = wake_receipt_root("retry-schedule-restart");
        register_wake_project(&root, "catdesk", Some("https://chatgpt.com/c/exact-thread"));
        let record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let now = now_unix();
        let schedule = schedule_chat_busy_retry(
            &wake_root,
            &record,
            "https://chatgpt.com/c/exact-thread",
            now,
        )
        .expect("durable busy retry");
        assert_eq!(
            persisted_waiting_wake_candidates(&root).expect("restart candidates"),
            vec!["session-waiting".to_string()],
            "restart retains the waiting handoff while the durable deadline remains authoritative"
        );
        assert_eq!(
            load_wake_retry_schedule(
                &wake_root,
                &record,
                "https://chatgpt.com/c/exact-thread",
                now,
            )
            .expect("schedule load"),
            Some(schedule)
        );

        write_wake_receipt(&root, &record, true);
        assert!(
            persisted_waiting_wake_candidates(&root)
                .expect("already-sent restart candidates")
                .is_empty()
        );
        assert_eq!(
            load_wake_retry_schedule(
                &wake_root,
                &record,
                "https://chatgpt.com/c/exact-thread",
                now,
            )
            .expect("cleared schedule"),
            None,
            "a proven current-target SENT receipt retires stale retry evidence"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exhausted_retry_journal_is_not_rehydrated_but_preserves_the_exact_review() {
        let root = wake_receipt_root("retry-schedule-exhausted-restart");
        let target = "https://chatgpt.com/c/exact-thread";
        register_wake_project(&root, "catdesk", Some(target));
        let record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let mut now = now_unix().saturating_sub(
            WAKE_CHAT_BUSY_RETRY_SECONDS.saturating_mul(u64::from(WAKE_CHAT_BUSY_RETRY_LIMIT)),
        );
        for _ in 1..=WAKE_CHAT_BUSY_RETRY_LIMIT {
            let schedule = schedule_chat_busy_retry(&wake_root, &record, target, now)
                .expect("bounded pre-submit retry");
            now = schedule.not_before_unix;
        }

        assert!(
            persisted_waiting_wake_candidates(&root)
                .expect("durable restart classification")
                .is_empty(),
            "restart must not recreate a worker for a terminally exhausted retry journal"
        );
        let store = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("state store");
        assert_eq!(
            store
                .latest_actionable_review_for_session("session-waiting")
                .expect("review")
                .expect("unread review")
                .record_id,
            record,
            "retry exhaustion never deletes, acknowledges, or replaces the original review"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dispatcher_honors_future_retry_before_browser_runtime_checks() {
        let root = wake_receipt_root("retry-dispatch-future");
        register_wake_project(&root, "catdesk", Some("https://chatgpt.com/c/exact-thread"));
        let record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let now = now_unix();
        let schedule = schedule_chat_busy_retry(
            &wake_root,
            &record,
            "https://chatgpt.com/c/exact-thread",
            now,
        )
        .expect("future retry schedule");
        let outcome = AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::WaitingForChatgpt,
            provider_events: 0,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        };
        let result = dispatch_actionable_wake(&root, "session-waiting", &outcome);
        match result {
            WakeDispatchOutcome::RetryAfter(delay) => {
                assert!(delay > Duration::ZERO);
                assert!(delay <= Duration::from_secs(WAKE_CHAT_BUSY_RETRY_SECONDS));
                assert_eq!(
                    wake_retry_remaining(&schedule, now),
                    Duration::from_secs(WAKE_CHAT_BUSY_RETRY_SECONDS)
                );
            }
            other => {
                panic!("future durable retry was not honored before runtime checks: {other:?}")
            }
        }
        assert!(
            !wake_root
                .join("venv")
                .join("Scripts")
                .join("python.exe")
                .exists(),
            "fixture intentionally proves no browser runtime was needed to honor the timer"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pending_retry_target_rebind_mints_successor_instead_of_replaying_old_record() {
        let root = wake_receipt_root("retry-target-rebind");
        register_wake_project(&root, "catdesk", Some("https://chatgpt.com/c/new-target"));
        let old_record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let now = now_unix();
        schedule_chat_busy_retry(
            &wake_root,
            &old_record,
            "https://chatgpt.com/c/old-target",
            now,
        )
        .expect("old-target retry schedule");

        assert_eq!(
            persisted_waiting_wake_candidates(&root).expect("rebound restart candidates"),
            vec!["session-waiting".to_string()]
        );
        let store = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("autonomy store");
        let successor = store
            .latest_actionable_review_for_session("session-waiting")
            .expect("latest review")
            .expect("successor review");
        assert_ne!(successor.record_id, old_record);
        assert!(successor.unread);
        let old = store
            .all_review_inbox()
            .expect("all reviews")
            .into_iter()
            .find(|review| review.record_id == old_record)
            .expect("old review retained");
        assert!(!old.unread);
        assert_eq!(
            load_wake_retry_schedule_any_target(&wake_root, &old_record, now)
                .expect("old retry journal cleared"),
            None,
            "old-target retry evidence cannot wake the rebound conversation"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn login_captcha_and_submitting_states_never_rehydrate_as_unsent() {
        let root = wake_receipt_root("retry-terminal-attention");
        register_wake_project(&root, "catdesk", Some("https://chatgpt.com/c/exact-thread"));
        let record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        let wake_root = root.join(".catdesk").join("wake-bridge");
        for (status, attention) in [
            ("OPERATOR_ATTENTION", Some("LOGIN_REQUIRED")),
            ("OPERATOR_ATTENTION", Some("CAPTCHA_OR_SECURITY_CHALLENGE")),
            ("SUBMITTING", None),
        ] {
            fs::write(
                wake_root.join("state.json"),
                serde_json::to_vec(&json!({
                    "schema_version": 4,
                    "deliveries": [{
                        "record_id": record,
                        "status": status,
                        "claimed_at_unix": 1.0,
                        "browser_sent_at_unix": null,
                        "message_sha256": null,
                        "target_sha256": null,
                        "receipt_schema_version": null,
                        "attention": attention
                    }]
                }))
                .expect("state json"),
            )
            .expect("state write");
            assert_eq!(
                persisted_wake_receipt_state(
                    &wake_root,
                    &record,
                    "https://chatgpt.com/c/exact-thread"
                ),
                PersistedWakeReceiptState::Unsafe,
                "{status}/{attention:?} must fail closed"
            );
            assert!(persisted_waiting_wake_candidates(&root).is_err());
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_wake_rehydration_rejects_unproven_sent_receipt() {
        let root = wake_receipt_root("startup-rehydrate-unsafe-sent");
        register_wake_project(&root, "catdesk", Some("https://chatgpt.com/c/exact-thread"));
        let record = create_restart_waiting_review(&root, "session-waiting", "catdesk", true);
        write_wake_receipt(&root, &record, false);
        assert_eq!(
            persisted_wake_receipt_state(
                &root.join(".catdesk").join("wake-bridge"),
                &record,
                "https://chatgpt.com/c/exact-thread"
            ),
            PersistedWakeReceiptState::Unsafe
        );
        assert!(persisted_waiting_wake_candidates(&root).is_err());
        let store = AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy"))
            .expect("autonomy store");
        let review = store
            .latest_actionable_review_for_session("session-waiting")
            .expect("latest")
            .expect("original review");
        assert_eq!(
            review.record_id, record,
            "unsafe state creates no successor"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catdesk_local_wake_target_migrates_once_without_global_fallback() {
        let root = wake_receipt_root("target-migration");
        fs::create_dir(root.join(".catdesk/wake-bridge/browser-profile")).expect("profile");
        fs::write(root.join(".catdesk/wake-bridge/config.json"), serde_json::to_vec(&json!({
            "conversation_url":"https://chatgpt.com/c/legacy-catdesk", "profile_dir":"browser-profile"
        })).expect("config")).expect("config write");
        register_wake_project(&root, "catdesk", None);
        assert_eq!(
            project_wake_target(&root, "catdesk").as_deref(),
            Some("https://chatgpt.com/c/legacy-catdesk")
        );
        let store =
            AutonomousProjectRegistryStoreV1::open(root.join(".catdesk/projects")).expect("store");
        let first = store.load_registry().expect("registry").projects[0].clone();
        assert_eq!(
            first.chatgpt_target_sha256.as_deref(),
            Some(
                super::super::autonomy_projects::project_chat_target_digest(
                    "https://chatgpt.com/c/legacy-catdesk"
                )
                .as_str()
            )
        );
        fs::write(root.join(".catdesk/wake-bridge/config.json"), serde_json::to_vec(&json!({
            "conversation_url":"https://chatgpt.com/c/changed-local", "profile_dir":"browser-profile"
        })).expect("config")).expect("rewrite");
        assert_eq!(
            project_wake_target(&root, "catdesk").as_deref(),
            Some("https://chatgpt.com/c/legacy-catdesk")
        );
        assert!(project_wake_target(&root, "external").is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wake_target_migration_profile_path_accepts_in_root_absolute_and_relative_only() {
        let root = wake_receipt_root("profile-shapes");
        let wake_root = root.join(".catdesk/wake-bridge");
        let relative = wake_root.join("relative-profile");
        let absolute = wake_root.join("absolute-profile");
        let outside = root.join("outside-profile");
        fs::create_dir(&relative).expect("relative");
        fs::create_dir(&absolute).expect("absolute");
        fs::create_dir(&outside).expect("outside");
        let config = |profile: &str| json!({"conversation_url":"https://chatgpt.com/c/profile-target","profile_dir":profile});
        fs::write(
            wake_root.join("config.json"),
            serde_json::to_vec(&config("relative-profile")).expect("config"),
        )
        .expect("relative config");
        assert_eq!(
            validated_legacy_catdesk_wake_target(&root).as_deref(),
            Some("https://chatgpt.com/c/profile-target")
        );
        fs::write(
            wake_root.join("config.json"),
            serde_json::to_vec(&config(&absolute.to_string_lossy())).expect("config"),
        )
        .expect("absolute config");
        assert_eq!(
            validated_legacy_catdesk_wake_target(&root).as_deref(),
            Some("https://chatgpt.com/c/profile-target")
        );
        fs::write(
            wake_root.join("config.json"),
            serde_json::to_vec(&config(&outside.to_string_lossy())).expect("config"),
        )
        .expect("outside config");
        assert_eq!(validated_legacy_catdesk_wake_target(&root), None);
        fs::write(
            wake_root.join("config.json"),
            serde_json::to_vec(&config("../outside-profile")).expect("config"),
        )
        .expect("traversal config");
        assert_eq!(validated_legacy_catdesk_wake_target(&root), None);
        fs::write(wake_root.join("config.json"), b"not-json").expect("malformed");
        assert_eq!(validated_legacy_catdesk_wake_target(&root), None);
        fs::write(
            wake_root.join("config.json"),
            vec![b'x'; (WAKE_BRIDGE_CONFIG_MAX_BYTES + 1) as usize],
        )
        .expect("oversized");
        assert_eq!(validated_legacy_catdesk_wake_target(&root), None);
        #[cfg(not(windows))]
        {
            let link = wake_root.join("linked-profile");
            std::os::unix::fs::symlink(&outside, &link).expect("profile symlink");
            fs::write(
                wake_root.join("config.json"),
                serde_json::to_vec(&config("linked-profile")).expect("config"),
            )
            .expect("symlink config");
            assert_eq!(validated_legacy_catdesk_wake_target(&root), None);
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catdesk_wake_target_migration_rejects_unsafe_config_and_preserves_existing_target() {
        let root = wake_receipt_root("target-migration-invalid");
        register_wake_project(
            &root,
            "catdesk",
            Some("https://chatgpt.com/c/durable-target"),
        );
        fs::write(root.join(".catdesk/wake-bridge/config.json"), b"not-json").expect("bad config");
        assert_eq!(
            project_wake_target(&root, "catdesk").as_deref(),
            Some("https://chatgpt.com/c/durable-target")
        );
        fs::write(
            root.join(".catdesk/wake-bridge/config.json"),
            serde_json::to_vec(&json!({
                "conversation_url":"https://chatgpt.com/c/unsafe", "profile_dir":"../escape"
            }))
            .expect("config"),
        )
        .expect("unsafe config");
        assert_eq!(validated_legacy_catdesk_wake_target(&root), None);
        let _ = fs::remove_dir_all(root);
    }

    fn write_wake_receipt(root: &Path, record_id: &str, valid: bool) {
        let wake_root = root.join(".catdesk").join("wake-bridge");
        let target = "https://chatgpt.com/c/exact-thread";
        fs::write(
            wake_root.join("config.json"),
            serde_json::to_vec(&json!({"conversation_url": target})).expect("config json"),
        )
        .expect("config write");
        let message_digest = if valid {
            wake_digest(&wake_message(record_id))
        } else {
            "0".repeat(64)
        };
        let state = json!({
            "schema_version": 4,
            "deliveries": [{
                "record_id": record_id,
                "status": "SENT",
                "claimed_at_unix": 1.0,
                "browser_sent_at_unix": if valid { 2.0 } else { 0.0 },
                "message_sha256": message_digest,
                "target_sha256": wake_digest(target),
                "receipt_schema_version": 1
            }]
        });
        fs::write(
            wake_root.join("state.json"),
            serde_json::to_vec(&state).expect("state json"),
        )
        .expect("state write");
    }

    #[test]
    fn invalid_or_legacy_receipt_never_relaunches_page_readiness() {
        let root = wake_receipt_root("invalid");
        let record = "review-receipt-invalid";
        write_wake_receipt(&root, record, false);
        assert!(!valid_wake_receipt(
            &root.join(".catdesk").join("wake-bridge"),
            record,
            "https://chatgpt.com/c/exact-thread"
        ));
        assert_eq!(
            confirm_wake_bridge_result(Some(0), "WOKE", false),
            WakeDispatchOutcome::Terminal
        );
        let legacy = json!({"schema_version": 3, "deliveries": [{"record_id": record, "status": "SENT", "claimed_at_unix": 1.0, "browser_sent_at_unix": null}]});
        fs::write(
            root.join(".catdesk").join("wake-bridge").join("state.json"),
            serde_json::to_vec(&legacy).expect("legacy json"),
        )
        .expect("legacy write");
        assert!(!valid_wake_receipt(
            &root.join(".catdesk").join("wake-bridge"),
            record,
            "https://chatgpt.com/c/exact-thread"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wake_valid_exact_receipt_confirms_and_binds_current_record() {
        let root = wake_receipt_root("valid");
        let record = "review-receipt-valid";
        let wake_root = root.join(".catdesk").join("wake-bridge");
        write_wake_receipt(&root, record, true);
        assert!(valid_wake_receipt(
            &wake_root,
            record,
            "https://chatgpt.com/c/exact-thread"
        ));
        assert!(!valid_wake_receipt(
            &wake_root,
            "different-review-record",
            "https://chatgpt.com/c/exact-thread"
        ));
        assert_eq!(
            confirm_wake_bridge_result(Some(0), "ALREADY_SENT", true),
            WakeDispatchOutcome::Confirmed
        );
        assert_eq!(
            confirm_wake_bridge_result(Some(0), "UNRECOGNIZED", true),
            WakeDispatchOutcome::Terminal
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn short_coordination_and_chat_busy_retry_budgets_remain_separate() {
        assert_eq!(WAKE_SHORT_RETRY_LIMIT, 3);
        assert_eq!(WAKE_SHORT_RETRY_DELAY, Duration::from_secs(2));
        assert_eq!(WAKE_CHAT_BUSY_RETRY_SECONDS, 300);
        assert_eq!(WAKE_CHAT_BUSY_RETRY_LIMIT, 12);
        assert_eq!(
            confirm_wake_bridge_result(Some(0), "WOKE", false),
            WakeDispatchOutcome::Terminal,
            "a completed bridge invocation without its exact receipt never creates another browser retry"
        );
    }

    fn wake_policy(
        generation: u64,
        mode: AutonomousWakeModeV1,
        terminal_task_id: Option<&str>,
    ) -> super::super::autonomy_state::AutonomousWakePolicyV1 {
        super::super::autonomy_state::AutonomousWakePolicyV1 {
            schema_version: super::super::autonomy_state::AUTONOMY_WAKE_POLICY_SCHEMA_VERSION,
            generation,
            mode,
            terminal_task_id: terminal_task_id.map(str::to_owned),
            set_at_unix: 1,
            stopped_reason: None,
        }
    }

    #[test]
    fn through_task_stale_stop_reconciles_fresh_user_policy() {
        let policies = RefCell::new(VecDeque::from([
            wake_policy(4, AutonomousWakeModeV1::ThroughTask, Some("task-a")),
            wake_policy(5, AutonomousWakeModeV1::Indefinite, None),
        ]));
        let stops = RefCell::new(Vec::new());
        let allowed = reconcile_wake_policy(
            || policies.borrow_mut().pop_front().ok_or(()),
            |policy| {
                wake_policy_decision(
                    policy,
                    Some("task-a"),
                    AutonomousSessionStateV1::CompletedVerified,
                    None,
                )
            },
            |generation, reason| {
                stops.borrow_mut().push((generation, reason));
                Err(())
            },
        );
        assert!(
            allowed,
            "a newer INDEFINITE policy permits the pending wake"
        );
        assert_eq!(*stops.borrow(), vec![(4, "through_task_completed")]);
    }

    #[test]
    fn through_task_stale_stop_honors_off_and_changed_terminal_task() {
        let off_policies = RefCell::new(VecDeque::from([
            wake_policy(7, AutonomousWakeModeV1::ThroughTask, Some("task-a")),
            wake_policy(8, AutonomousWakeModeV1::ManualOff, None),
        ]));
        let off_stops = RefCell::new(0usize);
        let suppressed = reconcile_wake_policy(
            || off_policies.borrow_mut().pop_front().ok_or(()),
            |policy| {
                wake_policy_decision(
                    policy,
                    Some("task-a"),
                    AutonomousSessionStateV1::CompletedVerified,
                    None,
                )
            },
            |_, _| {
                *off_stops.borrow_mut() += 1;
                Err(())
            },
        );
        assert!(!suppressed, "newer MANUAL_OFF leaves the review unread");
        assert_eq!(*off_stops.borrow(), 1);

        let changed_policies = RefCell::new(VecDeque::from([
            wake_policy(9, AutonomousWakeModeV1::ThroughTask, Some("task-a")),
            wake_policy(10, AutonomousWakeModeV1::ThroughTask, Some("task-b")),
        ]));
        let allowed = reconcile_wake_policy(
            || changed_policies.borrow_mut().pop_front().ok_or(()),
            |policy| {
                wake_policy_decision(
                    policy,
                    Some("task-a"),
                    AutonomousSessionStateV1::CompletedVerified,
                    None,
                )
            },
            |_, _| Err(()),
        );
        assert!(
            allowed,
            "a changed exact terminal task is re-evaluated, not guessed"
        );
    }

    #[test]
    fn provider_exhaustion_stale_stop_reconciles_and_bounded_churn_fails_closed() {
        let policies = RefCell::new(VecDeque::from([
            wake_policy(12, AutonomousWakeModeV1::UntilProviderExhausted, None),
            wake_policy(13, AutonomousWakeModeV1::Indefinite, None),
        ]));
        let allowed = reconcile_wake_policy(
            || policies.borrow_mut().pop_front().ok_or(()),
            |policy| {
                wake_policy_decision(
                    policy,
                    None,
                    AutonomousSessionStateV1::WaitingForChatgpt,
                    Some((
                        AutonomousSessionStateV1::CreditBudgetExhausted,
                        AutonomousProviderRouteV1::QwenUnavailable,
                    )),
                )
            },
            |_, _| Err(()),
        );
        assert!(
            allowed,
            "newer INDEFINITE policy permits a not-yet-launched wake"
        );

        let refreshed_policies = RefCell::new(VecDeque::from([
            wake_policy(14, AutonomousWakeModeV1::UntilProviderExhausted, None),
            wake_policy(15, AutonomousWakeModeV1::UntilProviderExhausted, None),
        ]));
        let refreshed_provider = RefCell::new(VecDeque::from([
            (
                AutonomousSessionStateV1::CreditBudgetExhausted,
                AutonomousProviderRouteV1::QwenUnavailable,
            ),
            (
                AutonomousSessionStateV1::WaitingForChatgpt,
                AutonomousProviderRouteV1::CodexPreferred,
            ),
        ]));
        let allowed_after_fresh_provider_read = reconcile_wake_policy(
            || refreshed_policies.borrow_mut().pop_front().ok_or(()),
            |policy| {
                let provider_state = refreshed_provider.borrow_mut().pop_front().ok_or(())?;
                wake_policy_decision(
                    policy,
                    None,
                    AutonomousSessionStateV1::WaitingForChatgpt,
                    Some(provider_state),
                )
            },
            |_, _| Err(()),
        );
        assert!(
            allowed_after_fresh_provider_read,
            "a fresh provider state is re-evaluated after a stale exhaustion stop"
        );

        let churn = RefCell::new(VecDeque::from_iter(
            (0..WAKE_POLICY_RECONCILIATION_LIMIT).map(|generation| {
                wake_policy(
                    generation as u64 + 20,
                    AutonomousWakeModeV1::UntilProviderExhausted,
                    None,
                )
            }),
        ));
        let stop_attempts = RefCell::new(0usize);
        let allowed_after_churn = reconcile_wake_policy(
            || churn.borrow_mut().pop_front().ok_or(()),
            |policy| {
                wake_policy_decision(
                    policy,
                    None,
                    AutonomousSessionStateV1::WaitingForChatgpt,
                    Some((
                        AutonomousSessionStateV1::CreditBudgetExhausted,
                        AutonomousProviderRouteV1::QwenUnavailable,
                    )),
                )
            },
            |_, _| {
                *stop_attempts.borrow_mut() += 1;
                Err(())
            },
        );
        assert!(
            !allowed_after_churn,
            "unsettled policy generations fail closed"
        );
        assert_eq!(*stop_attempts.borrow(), WAKE_POLICY_RECONCILIATION_LIMIT);
    }

    #[test]
    fn canonical_bootstrap_uses_only_project_or_completed_session_evidence() {
        assert_eq!(
            canonical_thread_candidate(
                Some("project-thread".into()),
                Some("completed-thread".into()),
            )
            .as_deref(),
            Some("project-thread")
        );
        assert_eq!(
            canonical_thread_candidate(None, Some("completed-thread".into())).as_deref(),
            Some("completed-thread")
        );
        assert!(canonical_thread_candidate(None, None).is_none());
    }

    fn resolver_root(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("catdesk-codex-resolver-{label}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("resolver root");
        root
    }

    fn path_of(paths: &[PathBuf]) -> OsString {
        std::env::join_paths(paths).expect("PATH fixture")
    }

    #[test]
    fn override_free_windows_npm_launcher_resolves_native_package_without_reading_shims() {
        let root = resolver_root("npm-launcher");
        let npm_bin = root.join("npm");
        let launcher = npm_bin.join("codex.cmd");
        let extensionless_shim = npm_bin.join("codex");
        let native = npm_bin
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe");
        std::fs::create_dir_all(native.parent().expect("native parent")).expect("package layout");
        std::fs::write(&launcher, "untrusted launcher contents").expect("launcher fixture");
        std::fs::write(&extensionless_shim, "untrusted extensionless shim").expect("shim fixture");
        std::fs::write(&native, []).expect("native fixture");

        assert_eq!(
            resolve_codex_executable_from_paths(None, &path_of(&[npm_bin])),
            Ok(native)
        );
    }

    #[test]
    fn direct_recovery_override_has_precedence_over_current_user_path() {
        let root = resolver_root("override");
        let override_path = root.join("override.exe");
        let path_directory = root.join("path");
        let path_binary = path_directory.join("codex.exe");
        std::fs::create_dir_all(&path_directory).expect("PATH fixture");
        std::fs::write(&override_path, []).expect("override fixture");
        std::fs::write(&path_binary, []).expect("PATH binary fixture");

        assert_eq!(
            resolve_codex_executable_from_paths(
                Some(override_path.clone()),
                &path_of(&[path_directory])
            ),
            Ok(override_path)
        );
    }

    #[test]
    fn shell_launcher_override_is_rejected_without_falling_back_or_reading_config() {
        let root = resolver_root("shell-override");
        let override_path = root.join("codex.cmd");
        let path_directory = root.join("path");
        std::fs::create_dir_all(&path_directory).expect("PATH fixture");
        std::fs::write(&override_path, "do not execute").expect("launcher fixture");
        std::fs::write(path_directory.join("codex.exe"), []).expect("PATH binary fixture");
        // The resolver consults only direct executable metadata and the
        // bounded npm-native location; an unrelated auth-shaped file must not
        // affect discovery.
        std::fs::write(root.join("auth.json"), "not consulted").expect("auth-shaped fixture");

        let error =
            resolve_codex_executable_from_paths(Some(override_path), &path_of(&[path_directory]))
                .expect_err("shell launcher override must fail closed");
        assert!(matches!(
            error,
            RuntimeError::Validation(message) if message.contains("direct executable")
        ));
    }

    #[test]
    fn app_server_binding_requires_exact_workspace_thread_and_persists_bounded_telemetry() {
        let root = std::env::temp_dir().join(format!("catdesk-runtime-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        store
            .create_session(
                "session-one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task-one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::Ready,
                }])
                .expect("queue"),
            )
            .expect("session");
        let cwd = root
            .canonicalize()
            .expect("cwd")
            .to_string_lossy()
            .to_string();
        let mut transport = FakeTransport {
            replies: VecDeque::from([
                json!({"thread":{"id":"thread-one","cwd":cwd,"canAcceptDirectInput":null}}),
                json!({"rateLimits":[{"name":"weekly","reachedLimit":true,"resetsAt":99}]}),
                json!({"thread":{"id":"thread-one","cwd":root.canonicalize().expect("cwd").to_string_lossy(),"canAcceptDirectInput":true,"status":{"type":"idle"}},"cwd":root.canonicalize().expect("cwd").to_string_lossy(),"model":"gpt-5.6-terra","reasoningEffort":"high"}),
            ]),
            methods: Vec::new(),
        };
        assert_eq!(
            bind_codex_app_server_thread(
                &root,
                "session-one",
                &mut transport,
                None,
                Some("thread-one"),
                false,
                10
            )
            .expect("bind"),
            "thread-one"
        );
        let snapshot = store.load_session("session-one").expect("snapshot");
        assert_eq!(snapshot.provider_thread_id.as_deref(), Some("thread-one"));
        assert_eq!(snapshot.codex_eligible_after_unix, Some(99));
        assert_eq!(
            transport.methods,
            ["thread/read", "account/rateLimits/read", "thread/resume"]
        );
    }

    #[test]
    fn successful_codex_turn_refreshes_one_comparable_post_snapshot_without_replay() {
        let root = std::env::temp_dir().join(format!("catdesk-runtime-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        store
            .create_session(
                "session-one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task-one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::Ready,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("session-one").expect("snapshot");
        snapshot.provider_thread_id = Some("thread-one".into());
        store.save_session(&snapshot).expect("thread binding");
        let before = super::super::codex_app_server::CodexRoutingTelemetryV1 {
            observed_at_unix: 10,
            source: "codex-app-server/account-rateLimits-read".into(),
            rate_limit_windows: vec![super::super::codex_app_server::CodexRateLimitWindowV1 {
                limit_name: "weekly".into(),
                used_percent: Some(10),
                resets_at_unix: Some(100),
                reached_limit: false,
            }],
            ..Default::default()
        };
        store
            .execution_accounting_store()
            .expect("accounting")
            .record_task_started(
                "catdesk",
                "task-one",
                "session-one",
                Some("thread-one"),
                "codex-cli",
                Some("gpt-5.6-terra"),
                Some(before),
                10_000,
            )
            .expect("pre-turn snapshot");
        let cwd = root
            .canonicalize()
            .expect("cwd")
            .to_string_lossy()
            .to_string();
        let mut transport = FakeTransport {
            replies: VecDeque::from([
                json!({"rateLimits":[{"name":"weekly","usedPercent":30,"resetsAt":100}]}),
                json!({"thread":{"id":"thread-one","cwd":cwd,"canAcceptDirectInput":null,"status":{"type":"notLoaded"}}}),
                json!({"thread":{"id":"thread-one","cwd":root.canonicalize().expect("cwd").to_string_lossy(),"canAcceptDirectInput":true,"status":{"type":"idle"}},"cwd":root.canonicalize().expect("cwd").to_string_lossy(),"model":"gpt-5.6-terra","reasoningEffort":"high"}),
            ]),
            methods: Vec::new(),
        };
        assert!(
            refresh_post_codex_accounting_with_transport(
                &root,
                &store,
                "session-one",
                11,
                &mut transport,
            )
            .expect("post-turn refresh")
        );
        assert_eq!(
            transport.methods,
            ["account/rateLimits/read", "thread/read", "thread/resume"]
        );
        let record = store
            .execution_accounting_store()
            .expect("accounting")
            .list(Some("session-one"))
            .expect("records")
            .pop()
            .expect("record");
        assert_eq!(record.usage_delta.status, "AVAILABLE");
        assert_eq!(
            record.usage_delta.rate_limit_used_percent_deltas[0].used_percent_delta,
            20
        );
        assert!(record.post_codex_usage_snapshot.is_some());
        assert!(
            !store
                .execution_accounting_store()
                .expect("accounting")
                .needs_post_codex_observability("session-one")
                .expect("replay gate")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_or_retried_turns_and_qwen_fallback_do_not_probe_post_turn_codex_accounting() {
        let incomplete = AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::Running,
            provider_events: 1,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        };
        let successful = AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::CompletedVerified,
            provider_events: 1,
            verification: Some(crate::delegated::coordinator::VerificationSummaryV1 {
                status: crate::delegated::coordinator::VerificationStatusV1::Passed,
                command: "fixture".into(),
                summary: "passed".into(),
            }),
            authoritative_diff: Some("diff".into()),
            final_review: Some("review".into()),
        };
        let failed = AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::WaitingForChatgpt,
            provider_events: 1,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        };
        let recovered_terminal = AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::CompletedVerified,
            provider_events: 0,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        };
        assert!(!should_refresh_post_codex_accounting(
            AutonomousProviderRouteV1::CodexPreferred,
            false,
            &incomplete,
        ));
        assert!(!should_refresh_post_codex_accounting(
            AutonomousProviderRouteV1::CodexPreferred,
            false,
            &failed,
        ));
        assert!(should_refresh_post_codex_accounting(
            AutonomousProviderRouteV1::CodexPreferred,
            false,
            &successful,
        ));
        assert!(should_refresh_post_codex_accounting(
            AutonomousProviderRouteV1::CodexPreferred,
            true,
            &recovered_terminal,
        ));
        assert!(!should_refresh_post_codex_accounting(
            AutonomousProviderRouteV1::CodexPreferred,
            false,
            &recovered_terminal,
        ));
        assert!(!should_refresh_post_codex_accounting(
            AutonomousProviderRouteV1::QwenFallbackActive,
            true,
            &successful,
        ));
    }

    #[test]
    fn missing_post_capture_authority_preserves_unknown_and_retries_without_mutation() {
        let root = std::env::temp_dir().join(format!("catdesk-runtime-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        store
            .create_session(
                "session-one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task-one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::Ready,
                }])
                .expect("queue"),
            )
            .expect("session");
        let before = super::super::codex_app_server::CodexRoutingTelemetryV1 {
            observed_at_unix: 10,
            source: "codex-app-server/account-rateLimits-read".into(),
            rate_limit_windows: vec![super::super::codex_app_server::CodexRateLimitWindowV1 {
                limit_name: "weekly".into(),
                used_percent: Some(10),
                resets_at_unix: Some(100),
                reached_limit: false,
            }],
            ..Default::default()
        };
        let accounting = store.execution_accounting_store().expect("accounting");
        accounting
            .record_task_started(
                "catdesk",
                "task-one",
                "session-one",
                Some("thread-one"),
                "codex-cli",
                Some("gpt-5.6-terra"),
                Some(before),
                10_000,
            )
            .expect("pre-turn snapshot");
        let mut transport = FakeTransport {
            replies: VecDeque::new(),
            methods: Vec::new(),
        };
        assert!(
            refresh_post_codex_accounting_with_transport(
                &root,
                &store,
                "session-one",
                11,
                &mut transport,
            )
            .is_err()
        );
        assert!(transport.methods.is_empty());
        let record = accounting
            .list(Some("session-one"))
            .expect("records")
            .pop()
            .expect("record");
        assert!(record.post_codex_usage_snapshot.is_none());
        assert_eq!(record.usage_delta.status, "UNKNOWN_NOT_CAPTURED");
        assert!(
            accounting
                .needs_post_codex_observability("session-one")
                .expect("retry remains pending")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "operator-local read-only override-free Codex acceptance probe"]
    fn live_override_free_current_user_codex_probe() {
        for name in [
            "CATDESK_CODEX_HOME",
            "CATDESK_CODEX_CLI_EXECUTABLE",
            "CODEX_HOME",
            "CODEX_API_KEY",
            "OPENAI_API_KEY",
            "CONTROL_PLANE_API_KEY",
        ] {
            unsafe { std::env::remove_var(name) };
        }

        let workspace = std::env::current_dir()
            .expect("current workspace")
            .canonicalize()
            .expect("canonical workspace");
        let project_store =
            AutonomousProjectRegistryStoreV1::open(workspace.join(".catdesk").join("projects"))
                .expect("project registry");
        let registry = project_store.load_registry().expect("registry read");
        let thread_id = registry
            .projects
            .iter()
            .find(|project| project.project_id == "catdesk")
            .and_then(|project| project.codex_thread_id.clone())
            .expect("canonical CatDesk thread binding");

        let config = host_app_server_launch_config(&workspace)
            .expect("override-free current-user Codex discovery");
        let mut transport = config
            .spawn_stdio_transport()
            .expect("override-free app-server launch");
        transport
            .initialize()
            .expect("override-free app-server initialize");
        let preparation = CodexAppServerReadClientV1::prepare_mutating_thread(
            &mut transport,
            &workspace.to_string_lossy(),
            None,
            Some(&thread_id),
            now_unix(),
        )
        .expect("override-free canonical Terra/High metadata");
        assert_eq!(preparation.thread.thread_id, thread_id);
        assert_eq!(
            preparation.thread.selected_model.as_deref(),
            Some("gpt-5.6-terra")
        );
        assert_eq!(preparation.thread.reasoning_effort.as_deref(), Some("high"));
        println!("OVERRIDE_FREE_CURRENT_USER_CODEX_OK");
    }
}
