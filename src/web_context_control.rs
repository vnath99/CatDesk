//! Explicit, advisory context controls for the current paired Web target.
//! A project scope supplied in a request does not identify its ChatGPT caller.

use crate::{
    mcp,
    web_conversation_context::{
        self, ContextPressurePolicy, ConversationKey, Observation, VisibleSourceKind,
        WebContextStore,
    },
};
use catdesk_wake::{protocol::valid_id, store::now};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CaptureRequest {
    pub(crate) project_id: String,
    pub(crate) expected_target_generation: u64,
    pub(crate) expected_target_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewRequest {
    project_id: String,
    event_id: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
}

/// Explicit response reads share context capture's bounded worker and browser
/// admission. Diagnostic snapshots stay confined to the local operator reader.
pub(crate) async fn read_review(workspace: &Path, args: Value) -> Result<Value, String> {
    let request: ReviewRequest =
        serde_json::from_value(args).map_err(|_| "WEB_REVIEW_ARGUMENTS_INVALID")?;
    validate_scope(
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
    )
    .map_err(|_| "WEB_REVIEW_ARGUMENTS_INVALID")?;
    if request.event_id.len() > 128 || !valid_id(&request.event_id) {
        return Err("WEB_REVIEW_ARGUMENTS_INVALID".into());
    }
    let capture_args = json!({"projectId":request.project_id,
        "expectedTargetGeneration":request.expected_target_generation,
        "expectedTargetSha256":request.expected_target_sha256});
    capture_with(
        workspace,
        capture_args,
        move |control, scope, keep_running| {
            let response = crate::operator_facade::read_web_review_expected_guarded(
                control,
                &scope.project_id,
                &request.event_id,
                (
                    scope.expected_target_generation,
                    &scope.expected_target_sha256,
                ),
                keep_running,
            )?;
            verified_review_response(&request, response)
        },
    )
    .await
    .map_err(|error| {
        let prefix = if error.starts_with("WEB_CONTEXT_") {
            "WEB_CONTEXT_"
        } else {
            "WEB_REVIEW_"
        };
        match error.strip_prefix(prefix).filter(|code| {
            !code.is_empty()
                && code.len() <= 80
                && code
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        }) {
            Some(code) => format!("WEB_REVIEW_{code}"),
            None => "WEB_REVIEW_UNAVAILABLE".into(),
        }
    })
}

fn verified_review_response(request: &ReviewRequest, response: Value) -> Result<Value, String> {
    let text = response["response"]
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= 8192);
    if response["action"] != "web_review_read"
        || response["eventVerified"] != true
        || response["readOnly"] != true
        || response["sendAttempted"] != false
        || response["projectId"] != request.project_id
        || response["eventId"] != request.event_id
        || response["target"]["generation"] != request.expected_target_generation
        || response["target"]["digest"] != request.expected_target_sha256
        || text.is_none()
    {
        return Err("WEB_REVIEW_EVENT_NOT_VERIFIED".into());
    }
    // Build a closed response rather than forwarding local diagnostic fields.
    let text = text.expect("checked review text");
    Ok(
        json!({"action":"web_review_read","projectId":request.project_id,
        "eventId":request.event_id,"target":response["target"],"response":text,
        "responseSha256":catdesk_wake::protocol::digest(text),
        "readOnly":true,"eventVerified":true,"sendAttempted":false,
        "taskCompletionVerified":false}),
    )
}

// Cover the existing 30s profile-lock wait and 90s adapter initialization,
// with remaining time for the bounded page/snapshot read. An outer 60s cap
// cancelled valid startup before its own diagnostic deadline could report.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_CAPTURES: usize = 4;
type CaptureScope = (PathBuf, String);
static CAPTURES: OnceLock<Mutex<BTreeSet<CaptureScope>>> = OnceLock::new();

struct CaptureLease(CaptureScope);

impl CaptureLease {
    fn acquire(control: &Path, project: &str) -> Result<Self, String> {
        let scope = (control.to_path_buf(), project.to_owned());
        let mut captures = CAPTURES
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "WEB_CONTEXT_CAPTURE_UNAVAILABLE")?;
        if captures.contains(&scope) {
            return Err("WEB_CONTEXT_CAPTURE_BUSY".into());
        }
        if captures.len() >= MAX_CAPTURES {
            return Err("WEB_CONTEXT_CAPTURE_LIMIT".into());
        }
        captures.insert(scope.clone());
        Ok(Self(scope))
    }
}

impl Drop for CaptureLease {
    fn drop(&mut self) {
        if let Some(captures) = CAPTURES.get()
            && let Ok(mut captures) = captures.lock()
        {
            captures.remove(&self.0);
        }
    }
}

struct CancelCapture(Arc<AtomicBool>);

impl Drop for CancelCapture {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) async fn capture(workspace: &Path, args: Value) -> Result<Value, String> {
    capture_with(workspace, args, read_snapshot)
        .await
        .map_err(capture_error)
}

fn read_snapshot(
    control: &Path,
    request: &CaptureRequest,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    crate::operator_facade::capture_web_context_expected_guarded(
        control,
        &request.project_id,
        (
            request.expected_target_generation,
            &request.expected_target_sha256,
        ),
        keep_running,
    )
}

fn capture_error(error: String) -> String {
    match error.as_str() {
        "BROWSER_SESSION_BUSY" | "COOPERATIVE_HOST_PROTOCOL_REQUIRED" => {
            "WEB_CONTEXT_CAPTURE_OWNER_BUSY".into()
        }
        "SOURCE_SNAPSHOT_INSTALLED_PACKAGE_UNSUPPORTED" => {
            "WEB_CONTEXT_CAPTURE_PACKAGE_UNSUPPORTED".into()
        }
        "SOURCE_SNAPSHOT_TIMEOUT" => "WEB_CONTEXT_CAPTURE_TIMEOUT".into(),
        _ => error,
    }
}

/// The local operator selects the currently paired target without accepting a
/// URL override. Resolve it once, then use the same witnessed bounded reader.
pub(crate) async fn capture_current(workspace: &Path, project: &str) -> Result<Value, String> {
    capture_current_with(workspace, project, read_snapshot)
        .await
        .map_err(capture_error)
}

pub(crate) async fn capture_current_with<F>(
    workspace: &Path,
    project: &str,
    snapshot: F,
) -> Result<Value, String>
where
    F: FnOnce(&Path, &CaptureRequest, &dyn Fn() -> Result<(), String>) -> Result<Value, String>
        + Send
        + 'static,
{
    if project.len() > 128 || !valid_id(project) {
        return Err("WEB_CONTEXT_ARGUMENTS_INVALID".into());
    }
    let target = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|_| "WEB_CONTEXT_PROJECT_UNAVAILABLE")?
        .ok_or("WEB_CONTEXT_PAIRED_TARGET_REQUIRED")?;
    let mut result = capture_with(
        workspace,
        json!({"projectId":project,
        "expectedTargetGeneration":target.generation,"expectedTargetSha256":target.sha256}),
        snapshot,
    )
    .await?;
    result["invocationSource"] = json!("STANDALONE_OPERATOR");
    Ok(result)
}

pub(crate) async fn capture_with<F>(
    workspace: &Path,
    args: Value,
    snapshot: F,
) -> Result<Value, String>
where
    F: FnOnce(&Path, &CaptureRequest, &dyn Fn() -> Result<(), String>) -> Result<Value, String>
        + Send
        + 'static,
{
    capture_with_timeout(workspace, args, CAPTURE_TIMEOUT, snapshot).await
}

async fn capture_with_timeout<F>(
    workspace: &Path,
    args: Value,
    timeout: Duration,
    snapshot: F,
) -> Result<Value, String>
where
    F: FnOnce(&Path, &CaptureRequest, &dyn Fn() -> Result<(), String>) -> Result<Value, String>
        + Send
        + 'static,
{
    let request: CaptureRequest =
        serde_json::from_value(args).map_err(|_| "WEB_CONTEXT_ARGUMENTS_INVALID")?;
    validate_scope(
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
    )?;
    let workspace = workspace.to_path_buf();
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancel_on_drop = CancelCapture(cancelled.clone());
    let started = Instant::now();
    // The worker, not the awaiting request, owns the lease. Dropping the MCP
    // future cancels cooperatively without admitting a second browser reader.
    tokio::task::spawn_blocking(move || {
        let keep_running = || {
            if cancelled.load(Ordering::Acquire) {
                Err("WEB_CONTEXT_CAPTURE_CANCELLED".into())
            } else if started.elapsed() >= timeout {
                Err("WEB_CONTEXT_CAPTURE_TIMEOUT".into())
            } else {
                Ok(())
            }
        };
        keep_running()?;
        let control =
            std::fs::canonicalize(&workspace).map_err(|_| "WEB_CONTEXT_CAPTURE_UNAVAILABLE")?;
        // This short preflight releases all target/registration locks before
        // browser work. The reader repeats the witness before and after read.
        mcp::with_current_project_web_target(
            &control,
            &request.project_id,
            request.expected_target_generation,
            &request.expected_target_sha256,
            |_, _| Ok(()),
        )?;
        keep_running()?;
        let _lease = CaptureLease::acquire(&control, &request.project_id)?;
        let mut result = snapshot(&control, &request, &keep_running)?;
        keep_running()?;
        result["observationSource"] = json!("BOUND_BROWSER_SNAPSHOT");
        result["hostStarted"] = json!(false);
        Ok(result)
    })
    .await
    .map_err(|_| "WEB_CONTEXT_CAPTURE_WORKER_FAILED".to_owned())?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigureRequest {
    project_id: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
    policy: PolicyInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyInput {
    configured_budget_tokens: u64,
    safety_headroom_tokens: u64,
    unknown_source_reserve_tokens: u64,
    #[serde(default)]
    warning_threshold_tokens: Option<u64>,
    automatic_rollover_enabled: bool,
    automatic_observation_enabled: bool,
    incomplete_history_rollover: bool,
}

impl PolicyInput {
    fn into_policy(self) -> ContextPressurePolicy {
        ContextPressurePolicy {
            configured_budget_tokens: self.configured_budget_tokens,
            safety_headroom_tokens: self.safety_headroom_tokens,
            unknown_source_reserve_tokens: self.unknown_source_reserve_tokens,
            warning_threshold_tokens: self.warning_threshold_tokens,
            automatic_rollover_enabled: self.automatic_rollover_enabled,
            automatic_observation_enabled: self.automatic_observation_enabled,
            incomplete_history_rollover: self.incomplete_history_rollover,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObserveRequest {
    project_id: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
    event: ContextEvent,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextEvent {
    event_id: String,
    revision: u64,
    dedup_key: String,
    source_kind: VisibleSourceKind,
    visible_text: Option<String>,
}

impl ContextEvent {
    fn observation(&self, observed_at: u64) -> Observation<'_> {
        Observation {
            event_id: &self.event_id,
            revision: self.revision,
            dedup_key: &self.dedup_key,
            source_kind: self.source_kind,
            visible_text: self.visible_text.as_deref(),
            observed_at_unix: observed_at,
        }
    }
}

fn validate_scope(project: &str, generation: u64, digest: &str) -> Result<(), String> {
    if project.len() > 128
        || !valid_id(project)
        || generation == 0
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("WEB_CONTEXT_ARGUMENTS_INVALID".into());
    }
    Ok(())
}

fn key(target: &mcp::ProjectWebTargetV1) -> ConversationKey {
    ConversationKey {
        project_id: target.project_id.clone(),
        conversation_url: target.url.clone(),
        target_generation: target.generation,
    }
}

pub(crate) fn configure(workspace: &Path, args: Value) -> Result<Value, String> {
    let request: ConfigureRequest =
        serde_json::from_value(args).map_err(|_| "WEB_CONTEXT_ARGUMENTS_INVALID")?;
    validate_scope(
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
    )?;
    let policy = request.policy.into_policy();
    policy.validate()?;
    mcp::with_current_project_web_target(
        workspace,
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
        |control, target| {
            let store = WebContextStore::open_nonblocking(control.join(".catdesk/web-context"))?;
            let key = key(target);
            let previous = store.active_key(&request.project_id)?;
            store.bind_observed_target(key.clone(), previous.as_ref())?;
            store.configure_policy(&request.project_id, policy)?;
            Ok(
                json!({"action":"web_context_configure","projectId":request.project_id,
            "pressure":store.pressure(&key)?,"advisory":true,"originAuthenticated":false,
            "pairedTargetVerified":true,"browserAction":"NONE","hostStarted":false}),
            )
        },
    )
}

pub(crate) fn observe(workspace: &Path, args: Value) -> Result<Value, String> {
    let request: ObserveRequest =
        serde_json::from_value(args).map_err(|_| "WEB_CONTEXT_ARGUMENTS_INVALID")?;
    validate_scope(
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
    )?;
    if request
        .event
        .visible_text
        .as_ref()
        .is_some_and(|text| text.len() > 64 * 1024)
    {
        return Err("WEB_CONTEXT_ARGUMENTS_INVALID".into());
    }
    let observed_at = now();
    // Validate before opening state, including existing event/revision rules.
    web_conversation_context::validate_observation(request.event.observation(observed_at))?;
    mcp::with_current_project_web_target(
        workspace,
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
        |control, target| {
            let store = WebContextStore::open_nonblocking(control.join(".catdesk/web-context"))?;
            let key = key(target);
            let previous = store.active_key(&request.project_id)?;
            store.bind_observed_target(key.clone(), previous.as_ref())?;
            let pressure =
                store.record_observation(&key, request.event.observation(observed_at))?;
            Ok(
                json!({"action":"web_context_observe","projectId":request.project_id,
            "eventId":request.event.event_id,"pressure":pressure,"advisory":true,
            "observationSource":"CALLER_REPORTED","originAuthenticated":false,
            "pairedTargetVerified":true,"rawTextPersisted":false,"browserAction":"NONE","hostStarted":false}),
            )
        },
    )
}

pub(crate) fn tool_schemas() -> [(&'static str, &'static str, &'static str, Value); 3] {
    [
        (
            "catdesk_web_context_capture",
            "Capture one project's visible context",
            "Perform one bounded read through the existing private browser snapshot owner for the exact paired project generation and URL digest. Store hashes and estimated token counts only. No message is sent, no target changes, and no recurring worker or WakeHost starts. Browser reads can refuse when another owner is active. Caller identity is not authenticated by the explicit project scope.",
            json!({"type":"object","additionalProperties":false,"properties":{
                "projectId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"},
                "expectedTargetGeneration":{"type":"integer","minimum":1},
                "expectedTargetSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"}},
                "required":["projectId","expectedTargetGeneration","expectedTargetSha256"]}),
        ),
        (
            "catdesk_web_context_configure",
            "Configure one project's context policy",
            "Set the existing advisory context policy for the exact paired project generation and URL digest from registry Web status. Automatic observation and rollover are explicit opt-ins affecting later work by an already authorized running worker. The operation starts no browser or host and does not identify the calling ChatGPT conversation.",
            json!({"type":"object","additionalProperties":false,"properties":{
            "projectId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"},
            "expectedTargetGeneration":{"type":"integer","minimum":1},
            "expectedTargetSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "policy":{"type":"object","additionalProperties":false,"properties":{
                "configuredBudgetTokens":{"type":"integer","minimum":1,"maximum":2000000},
                "safetyHeadroomTokens":{"type":"integer","minimum":0,"description":"Must be smaller than the configured budget"},
                "unknownSourceReserveTokens":{"type":"integer","minimum":0,"description":"Must not exceed the configured budget"},
                "warningThresholdTokens":{"type":["integer","null"],"minimum":1,"maximum":2000000,"description":"Optional observed-token warning at or below budget minus safety headroom; omitted or null uses 80% of rollover trigger. Does not authorize rollover or imply complete history."},
                "automaticRolloverEnabled":{"type":"boolean"},
                "automaticObservationEnabled":{"type":"boolean"},
                "incompleteHistoryRollover":{"type":"boolean"}},
                "required":["configuredBudgetTokens","safetyHeadroomTokens","unknownSourceReserveTokens","automaticRolloverEnabled","automaticObservationEnabled","incompleteHistoryRollover"]}},
            "required":["projectId","expectedTargetGeneration","expectedTargetSha256","policy"]}),
        ),
        (
            "catdesk_web_context_observe",
            "Record one advisory context observation",
            "Record bounded caller-reported model-visible text, including retained tool arguments and worker messages, or unknown-attachment metadata for the exact paired project conversation. Use the same dedup key for multiple representations of one retained message and stable revisions for streaming updates. Only hashes and token counts persist. Scope is explicit caller-reported data, not authenticated ChatGPT origin; no global traffic attribution, browser capture, or delivery authority is created.",
            json!({"type":"object","additionalProperties":false,"properties":{
            "projectId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"},
            "expectedTargetGeneration":{"type":"integer","minimum":1},
            "expectedTargetSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "event":{"type":"object","additionalProperties":false,"properties":{
                "eventId":{"type":"string","pattern":"^[A-Za-z0-9_.:-]{1,200}$"},
                "revision":{"type":"integer","minimum":1},
                "dedupKey":{"type":"string","pattern":"^[A-Za-z0-9_.:-]{1,200}$"},
                "sourceKind":{"type":"string","enum":["USER_MESSAGE","ASSISTANT_MESSAGE","TOOL_RESULT","TOOL_ARGUMENTS","WORKER_MESSAGE","HANDOFF","UNKNOWN_ATTACHMENT"]},
                "visibleText":{"type":["string","null"],"maxLength":65536,"description":"UTF-8 bytes also limited to 65536; null only for UNKNOWN_ATTACHMENT"}},
                "required":["eventId","revision","dedupKey","sourceKind","visibleText"]}},
            "required":["projectId","expectedTargetGeneration","expectedTargetSha256","event"]}),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn review_reads_reject_scope_overrides_before_state_creation() {
        let root =
            std::env::temp_dir().join(format!("web-review-invalid-{}", uuid::Uuid::new_v4()));
        let valid = json!({"projectId":"alpha","eventId":"review-one",
            "expectedTargetGeneration":1,"expectedTargetSha256":"a".repeat(64)});
        for (field, value) in [
            ("workspace", json!("private-secret")),
            ("conversationUrl", json!("https://chatgpt.com/c/override")),
            ("projectId", json!("../other")),
            ("eventId", json!("../other")),
            ("eventId", json!("x".repeat(129))),
            ("expectedTargetGeneration", json!(0)),
            ("expectedTargetSha256", json!("A".repeat(64))),
        ] {
            let mut args = valid.clone();
            args[field] = value;
            assert_eq!(
                read_review(&root, args).await.unwrap_err(),
                "WEB_REVIEW_ARGUMENTS_INVALID"
            );
            assert!(!root.exists());
        }
    }

    #[test]
    fn review_response_never_forwards_unverified_or_unrelated_text() {
        let request: ReviewRequest = serde_json::from_value(json!({
            "projectId":"alpha","eventId":"review-one",
            "expectedTargetGeneration":1,"expectedTargetSha256":"a".repeat(64)
        }))
        .unwrap();
        let valid = json!({"action":"web_review_read","projectId":"alpha",
            "eventId":"review-one","eventVerified":true,"readOnly":true,"sendAttempted":false,
            "target":{"generation":1,"digest":"a".repeat(64),"url":"https://chatgpt.com/c/alpha"},
            "response":"A bounded review finding.","snapshot":{"private":"diagnostic-secret"}});
        let result = verified_review_response(&request, valid.clone()).unwrap();
        assert_eq!(result["taskCompletionVerified"], false);
        assert_eq!(
            result["responseSha256"],
            catdesk_wake::protocol::digest("A bounded review finding.")
        );
        assert!(result.get("snapshot").is_none());
        assert!(!result.to_string().contains("diagnostic-secret"));
        for (field, value) in [
            ("projectId", json!("beta")),
            ("eventId", json!("review-two")),
            ("eventVerified", json!(false)),
            ("eventVerified", json!("true")),
            ("readOnly", json!(false)),
            ("sendAttempted", json!(true)),
            ("response", json!("")),
            ("response", json!("x".repeat(8193))),
            ("target", json!({"generation":2,"digest":"a".repeat(64)})),
            ("target", json!({"generation":1,"digest":"b".repeat(64)})),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert_eq!(
                verified_review_response(&request, invalid).unwrap_err(),
                "WEB_REVIEW_EVENT_NOT_VERIFIED"
            );
        }
    }

    #[test]
    fn context_controls_reject_invalid_payloads_before_state_creation() {
        let root =
            std::env::temp_dir().join(format!("web-context-invalid-{}", uuid::Uuid::new_v4()));
        let valid = json!({"projectId":"alpha","expectedTargetGeneration":1,
            "expectedTargetSha256":"a".repeat(64),"policy":ContextPressurePolicy::default()});
        for (field, value) in [
            ("workspace", json!("C:/private")),
            ("conversationUrl", json!("https://chatgpt.com/c/other")),
            ("projectId", json!("../other")),
            ("expectedTargetGeneration", json!(0)),
            ("expectedTargetSha256", json!("A".repeat(64))),
        ] {
            let mut args = valid.clone();
            args[field] = value;
            assert!(configure(&root, args).is_err());
            assert!(!root.exists());
        }
        let mut invalid_policy = valid.clone();
        invalid_policy["policy"]["safetyHeadroomTokens"] = json!(96000);
        assert!(configure(&root, invalid_policy).is_err());
        for warning in [json!(0), json!(72001), json!("early")] {
            let mut invalid = valid.clone();
            invalid["policy"]["warningThresholdTokens"] = warning;
            assert!(configure(&root, invalid).is_err());
            assert!(!root.exists());
        }
        let mut unknown_policy = valid.clone();
        unknown_policy["policy"]["conversationUrl"] = json!("C:/private-secret");
        assert!(configure(&root, unknown_policy).is_err());
        let event = json!({"eventId":"tool-1","revision":1,"dedupKey":"tool-1","sourceKind":"TOOL_RESULT","visibleText":"private text"});
        for (field, value) in [
            ("eventId", json!("../other")),
            ("revision", json!(0)),
            ("sourceKind", json!("OTHER")),
            ("visibleText", json!(null)),
            ("visibleText", json!("é".repeat(32769))),
            ("observedAtUnix", json!(100)),
        ] {
            let mut args = json!({"projectId":"alpha","expectedTargetGeneration":1,"expectedTargetSha256":"a".repeat(64),"event":event.clone()});
            args["event"][field] = value;
            assert!(observe(&root, args).is_err());
            assert!(!root.exists());
        }
    }

    #[test]
    fn warning_policy_control_and_schema_retain_legacy_omission() {
        let legacy = serde_json::to_value(ContextPressurePolicy::default()).unwrap();
        let parsed: PolicyInput = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(parsed.into_policy(), ContextPressurePolicy::default());
        let mut custom = legacy;
        custom["warningThresholdTokens"] = json!(12345);
        let parsed: PolicyInput = serde_json::from_value(custom).unwrap();
        let policy = parsed.into_policy();
        policy.validate().unwrap();
        assert_eq!(policy.warning_tokens(), 12345);
        assert!(!policy.automatic_rollover_enabled && !policy.automatic_observation_enabled);
        let (_, _, _, schema) = tool_schemas()
            .into_iter()
            .find(|(name, _, _, _)| *name == "catdesk_web_context_configure")
            .unwrap();
        assert_eq!(
            schema["properties"]["policy"]["properties"]["warningThresholdTokens"]["type"],
            json!(["integer", "null"])
        );
        assert!(
            !schema["properties"]["policy"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("warningThresholdTokens"))
        );
    }

    #[tokio::test]
    async fn capture_validates_before_state_and_deadline_before_reader() {
        let root =
            std::env::temp_dir().join(format!("web-capture-invalid-{}", uuid::Uuid::new_v4()));
        let valid = json!({"projectId":"alpha","expectedTargetGeneration":1,
            "expectedTargetSha256":"a".repeat(64)});
        for (field, value) in [
            ("workspace", json!("C:/private")),
            ("conversationUrl", json!("https://chatgpt.com/c/other")),
            ("timeout", json!(1800)),
            ("projectId", json!("../other")),
            ("expectedTargetGeneration", json!(0)),
            ("expectedTargetSha256", json!("A".repeat(64))),
        ] {
            let mut args = valid.clone();
            args[field] = value;
            assert_eq!(
                capture_with(&root, args, |_, _, _| panic!("reader called"))
                    .await
                    .unwrap_err(),
                "WEB_CONTEXT_ARGUMENTS_INVALID"
            );
            assert!(!root.exists());
        }
        assert_eq!(
            capture_with_timeout(&root, valid, Duration::ZERO, |_, _, _| panic!(
                "reader called"
            ))
            .await
            .unwrap_err(),
            "WEB_CONTEXT_CAPTURE_TIMEOUT"
        );
        assert!(!root.exists());
    }
}
