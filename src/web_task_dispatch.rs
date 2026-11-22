//! Explicit local task grants on the existing authenticated central MCP route.
//! Each invocation owns one bounded task turn; HTTP cancellation cannot abandon
//! its drain. There is no mutable global workspace, worker map or new registry.

use crate::{
    mcp::{JsonRpcRequest, JsonRpcResponse},
    project_mcp::{ProjectContext, Selection},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(crate) const TOOL: &str = "catdesk_web_task_tool";
pub(crate) const CACHED_COMMAND_PREFIX: &str = "CATDESK_WEB_TASK=";

/// A reserved task envelope must never fall through to the shell handler,
/// including malformed/incorrectly cased versions of the reserved namespace.
pub(crate) fn is_cached_call(request: &JsonRpcRequest) -> bool {
    request.method == "tools/call"
        && request.params["name"] == "run_command"
        && request.params["arguments"]["command"]
            .as_str()
            .is_some_and(|command| {
                command
                    .trim_start()
                    .get(.."CATDESK_WEB_TASK".len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("CATDESK_WEB_TASK"))
            })
}

pub(crate) fn cached_transport_metadata() -> Value {
    json!({"mcpTool":"run_command","commandPrefix":CACHED_COMMAND_PREFIX,
        "payload":"JSON_SERIALIZE_THE_SAME_SCOPED_TASK_ARGUMENTS",
        "requiredArguments":schema()["inputSchema"]["required"],
        "outerArguments":{"required":["command"],"optionalBoolean":["allow_without_plan"],
            "falseOnly":["dry_run","include_full_output"],"mustBeAbsent":["cwd","timeout"],
            "unknownFieldsAllowed":false},
        "shellExecuted":false,"cwdOverrideAllowed":false,"timeoutOverrideAllowed":false,
        "sameCallIdAndJournal":true,"centralPlanPolicyRetained":true})
}

/// Fixed categories only. Never return an argument value, unknown field name,
/// JSON parser excerpt, task envelope or execution key in an error receipt.
#[derive(Debug, PartialEq, Eq)]
struct CachedInvalid(&'static str);

fn cached_request(request: &JsonRpcRequest) -> Result<JsonRpcRequest, CachedInvalid> {
    let arguments = request.params["arguments"]
        .as_object()
        .ok_or(CachedInvalid("ARGUMENTS_NOT_OBJECT"))?;
    for (name, value) in arguments {
        match name.as_str() {
            "command" if value.is_string() => {}
            "allow_without_plan" if value.is_boolean() => {}
            "dry_run" | "include_full_output" if value == &Value::Bool(false) => {}
            "command" => return Err(CachedInvalid("COMMAND_NOT_STRING")),
            "allow_without_plan" => return Err(CachedInvalid("PLAN_FLAG_NOT_BOOLEAN")),
            "dry_run" => return Err(CachedInvalid("DRY_RUN_NOT_FALSE")),
            "include_full_output" => return Err(CachedInvalid("FULL_OUTPUT_NOT_FALSE")),
            "cwd" => return Err(CachedInvalid("CWD_OVERRIDE_FORBIDDEN")),
            "timeout" => return Err(CachedInvalid("TIMEOUT_OVERRIDE_FORBIDDEN")),
            _ => return Err(CachedInvalid("UNSUPPORTED_OUTER_FIELD")),
        }
    }
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .ok_or(CachedInvalid("COMMAND_NOT_STRING"))?;
    if command.len() > 2 * 1024 * 1024 {
        return Err(CachedInvalid("COMMAND_TOO_LARGE"));
    }
    let payload = command
        .trim_start()
        .strip_prefix(CACHED_COMMAND_PREFIX)
        .ok_or(CachedInvalid("RESERVED_PREFIX_INVALID"))?;
    let task: Value =
        serde_json::from_str(payload).map_err(|_| CachedInvalid("PAYLOAD_JSON_INVALID"))?;
    if !task.is_object() {
        return Err(CachedInvalid("PAYLOAD_NOT_OBJECT"));
    }
    Ok(JsonRpcRequest {
        jsonrpc: request.jsonrpc.clone(),
        id: request.id.clone(),
        method: "tools/call".into(),
        params: json!({"name":TOOL,"arguments":task}),
    })
}

/// Pure representation check for a frozen call. No grant, journal, task or
/// shell is accessed; only its canonical outer-envelope digest leaves here.
pub(crate) fn canonical_cached_envelope_sha256(task: &Value) -> Result<String, String> {
    let call: Call = serde_json::from_value(task.clone())
        .map_err(|_| "WEB_TASK_CACHED_ENVELOPE_UNREPRESENTABLE")?;
    if !call.arguments.is_object()
        || call.execution_key.len() != 64
        || !call
            .execution_key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("WEB_TASK_CACHED_ENVELOPE_UNREPRESENTABLE".into());
    }
    let arguments = json!({"command":format!("{CACHED_COMMAND_PREFIX}{task}")});
    let request = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!("representation-only")),
        method: "tools/call".into(),
        params: json!({"name":"run_command","arguments":arguments}),
    };
    let adapted =
        cached_request(&request).map_err(|_| "WEB_TASK_CACHED_ENVELOPE_UNREPRESENTABLE")?;
    if adapted.params["arguments"] != *task {
        return Err("WEB_TASK_CACHED_ENVELOPE_UNREPRESENTABLE".into());
    }
    Ok(catdesk_wake::protocol::digest(&arguments.to_string()))
}

/// The central HTTP layer must apply the same bearer/capability checks as TOOL.
/// Only this closed JSON adapter runs; the command text is never shell input.
pub(crate) async fn handle_cached(control: &str, request: &JsonRpcRequest) -> JsonRpcResponse {
    let adapted = match cached_request(request) {
        Ok(adapted) => adapted,
        Err(reason) => {
            let mut response = JsonRpcResponse::error(
                request.id.clone(),
                -32602,
                "WEB_TASK_CACHED_ARGUMENTS_INVALID".into(),
            );
            response.error.as_mut().expect("constructed error").data = Some(json!({
                "phase":"CACHED_ENVELOPE_VALIDATION", "invariant":reason.0,
                "nativeExecutionAttempted":false, "retryAuthorized":false
            }));
            return response;
        }
    };
    if let Err(response) = crate::mcp::enforce_cached_web_task_plan(request, control) {
        return response;
    }
    let mut response = handle(control, &adapted).await;
    if let Some(result) = response.result.as_mut() {
        result["_meta"]["catdeskTaskTransport"] = json!({"invokedTool":"run_command",
            "dispatcher":TOOL,"shellExecuted":false,"sameCallIdAndJournal":true});
    }
    response
}

#[cfg(all(test, windows))]
type TestGate = (
    std::sync::Arc<tokio::sync::Notify>,
    std::sync::Arc<tokio::sync::Notify>,
);
#[cfg(all(test, windows))]
static TEST_GATES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::BTreeMap<PathBuf, TestGate>>,
> = std::sync::OnceLock::new();
#[cfg(all(test, windows))]
async fn pause_started_test_turn(control: &Path) {
    let gate = TEST_GATES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(control)
        .cloned();
    if let Some((entered, release)) = gate {
        entered.notify_one();
        release.notified().await;
    }
}

pub(crate) fn schema() -> Value {
    json!({"name":TOOL,"description":"Execute one operator-granted isolated project task through the central connection. Use the exact project/work/task identity, current paired target, private project executionKey and stable callId. The registry and archived assignment choose the workspace. Recorded calls never replay. Ordinary tools use the isolated worktree; an additional frozen native-task grant permits normal native queue execution in the registered workspace under its existing ownership gates. This does not authenticate the provider conversation.",
    "inputSchema":{"type":"object","properties":{
        "projectId":{"type":"string"},"requestId":{"type":"string"},"sourceEventId":{"type":"string"},
        "assignmentSha256":{"type":"string","pattern":"^[a-f0-9]{64}$"},
        "expectedTargetGeneration":{"type":"integer","minimum":1},"expectedTargetSha256":{"type":"string","pattern":"^[a-f0-9]{64}$"},
        "executionKey":{"type":"string","pattern":"^[a-f0-9]{64}$","description":"Private key from this task's LOCAL execution-grant; never use a sibling project's key."},
        "taskId":{"type":"string"},"callId":{"type":"string"},
        "tool":{"type":"string","enum":["read","search","patch.preview","patch.apply","patch.compare","diff.actual","verify.command","git.checkpoint","git.sync","wake.manual_end_turn","native.codex.dispatch","native.codex.status"],"description":"git.checkpoint requires an explicit local Git checkpoint grant and reviewed assignment checkpointPaths; arguments are {verificationCallIds:[successful verify.command call IDs]}. git.sync additionally requires the sync grant; arguments are {checkpointCallId:completed checkpoint call ID}; an uncertain earlier push is observed, never resent. wake.manual_end_turn requires an explicit local manual-wake grant and records one queue-only intent. native.codex.dispatch requires an additional exact frozen native-queue task grant; arguments must be {}. Native CLI arguments and runtime bound come from local authority, never this request. native.codex.status reads only that grant's task/receipt, including while dispatch owns its project lease; it never replays."},"arguments":{"type":"object"}},
    "required":["projectId","requestId","sourceEventId","assignmentSha256","expectedTargetGeneration","expectedTargetSha256","executionKey","taskId","callId","tool","arguments"],"additionalProperties":false},
    "annotations":{"readOnlyHint":false,"destructiveHint":true,"openWorldHint":true}})
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Call {
    project_id: String,
    request_id: String,
    source_event_id: String,
    assignment_sha256: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
    execution_key: String,
    task_id: String,
    call_id: String,
    tool: String,
    arguments: Value,
}

impl Call {
    fn pins(&self) -> Value {
        json!({"projectId":self.project_id,"requestId":self.request_id,"sourceEventId":self.source_event_id,
        "assignmentSha256":self.assignment_sha256,"expectedTargetGeneration":self.expected_target_generation,"expectedTargetSha256":self.expected_target_sha256})
    }
    fn selection(&self) -> Selection {
        Selection {
            project_id: self.project_id.clone(),
            target_generation: self.expected_target_generation,
            target_sha256: self.expected_target_sha256.clone(),
        }
    }
    fn task_args(&self) -> Value {
        json!({"taskId":self.task_id,"callId":self.call_id,"tool":self.tool,"arguments":self.arguments})
    }
}

fn authorize(control: &Path, call: &Call) -> Result<PathBuf, String> {
    let lane_call = crate::project_mcp_task::tool_enters_web_execution_lane(&call.tool)
        .then_some((call.task_id.as_str(), call.call_id.as_str()));
    let admission = crate::web_work::authorized_assignment_for_tool(
        control,
        call.pins(),
        &call.execution_key,
        lane_call,
    )?;
    if admission["taskId"].as_str() != Some(&call.task_id) {
        return Err("WEB_TASK_GRANT_SCOPE_MISMATCH".into());
    }
    admission["assignmentFile"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "WEB_TASK_GRANT_UNAVAILABLE".into())
}

/// Called only after the central HTTP layer's existing bearer check. The
/// additional private key binds this call to one explicit local assignment.
pub(crate) async fn handle(control: &str, request: &JsonRpcRequest) -> JsonRpcResponse {
    let id = request.id.clone();
    if id.is_none() {
        return JsonRpcResponse::error(id, -32600, "WEB_TASK_REQUEST_ID_REQUIRED".into());
    }
    let call: Call = match serde_json::from_value(request.params["arguments"].clone()) {
        Ok(call) => call,
        Err(_) => return JsonRpcResponse::error(id, -32602, "WEB_TASK_ARGUMENTS_INVALID".into()),
    };
    if !call.arguments.is_object()
        || call.execution_key.len() != 64
        || !call
            .execution_key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return JsonRpcResponse::error(id, -32602, "WEB_TASK_ARGUMENTS_INVALID".into());
    }
    let control = PathBuf::from(control);
    if call.tool == "native.codex.status" {
        if call.arguments != json!({}) {
            return JsonRpcResponse::error(id, -32602, "WEB_TASK_NATIVE_ARGUMENTS_INVALID".into());
        }
        let response_id = id.clone();
        return match tokio::spawn(async move {
            let selection = call.selection();
            let observed = json!({"name":TOOL,"arguments":call.task_args()});
            let root = control.clone();
            let admission = tokio::task::spawn_blocking(move || crate::web_work::native_status(
                &root,call.pins(),&call.execution_key,&call.task_id)).await;
            match admission {
                Ok(Ok(admission)) => {
                    let traffic = crate::project_mcp::TrafficObservation::start_scoped(control.clone(),&selection,observed).await;
                    #[cfg(all(test,windows))]
                    pause_started_test_turn(&control.join(format!("native-status-{}",selection.project_id))).await;
                    let result = tokio::task::spawn_blocking(move || admission.snapshot()).await;
                    let mut response = match result {
                        Ok(Ok(value)) => JsonRpcResponse::success(response_id,json!({"content":[{"type":"text","text":"Native task snapshot; no execution or replay."}],"structuredContent":value,"isError":false})),
                        Ok(Err(reason)) => JsonRpcResponse::error(response_id,-32002,reason),
                        Err(_) => JsonRpcResponse::error(response_id,-32002,"WEB_TASK_NATIVE_STATUS_UNAVAILABLE".into()),
                    };
                    traffic.finish(&mut response).await;
                    response
                },
                Ok(Err(reason)) => JsonRpcResponse::error(response_id,-32002,reason),
                Err(_) => JsonRpcResponse::error(response_id,-32002,"WEB_TASK_NATIVE_STATUS_UNAVAILABLE".into()),
            }
        }).await { Ok(response)=>response, Err(_)=>JsonRpcResponse::error(id,-32002,"WEB_TASK_NATIVE_STATUS_UNAVAILABLE".into()) };
    }
    if call.tool == "native.codex.dispatch" {
        if call.arguments != json!({}) {
            return JsonRpcResponse::error(id, -32602, "WEB_TASK_NATIVE_ARGUMENTS_INVALID".into());
        }
        let response_id = id.clone();
        return match tokio::spawn(async move {
            let selection = call.selection();
            let observed = json!({"name":TOOL,"arguments":call.task_args()});
            let call = std::sync::Arc::new(call);
            let owned = call.clone(); let root = control.clone();
            match tokio::task::spawn_blocking(move || authorize(&root, &owned)).await {
                Ok(Ok(_)) => {},
                Ok(Err(reason)) => return JsonRpcResponse::error(response_id, -32002, reason),
                Err(_) => return JsonRpcResponse::error(response_id, -32002, "WEB_TASK_AUTHORIZATION_UNAVAILABLE".into()),
            }
            let traffic = crate::project_mcp::TrafficObservation::start_scoped(control.clone(), &selection, observed).await;
            let root = control.clone(); let owned = call.clone();
            let admission = tokio::task::spawn_blocking(move || crate::web_work::prepare_native_dispatch(
                &root, owned.pins(), &owned.execution_key, &owned.task_id, &owned.call_id)).await;
            let result = match admission {
                Ok(Ok(admission)) => {
                    #[cfg(all(test,windows))]
                    pause_started_test_turn(&control.join(format!("native-{}", call.project_id))).await;
                    let result = crate::operator_codex_cli::dispatch_native_queue_selection(
                        &admission.control, &admission.project_id, &admission.call.selection).await;
                    let recorded = result.clone();
                    match tokio::task::spawn_blocking(move || admission.finish(&recorded)).await {
                        Ok(Ok(())) => result,
                        _ => Err("PROJECT_TASK_RECOVERY_REQUIRED".into()),
                    }
                }
                Ok(Err(reason)) => Err(reason),
                Err(_) => Err("WEB_TASK_OUTCOME_UNKNOWN_NO_REPLAY".into()),
            };
            let mut response = match result {
                Ok(result) => {
                    let failed = result["status"] == "prelaunch_refused"
                        || result["job"]["state"].as_str().is_some_and(|state| state != "NATIVE_EXITED_ZERO");
                    let text = if failed { "Native task did not complete normally; inspect its exact result before any retry." }
                        else { "Native queue dispatch recorded; task verification remains separate." };
                    JsonRpcResponse::success(response_id,
                    json!({"content":[{"type":"text","text":text}],
                        "structuredContent":result,"isError":failed,
                        "_meta":{"catdeskTaskLifecycle":{"nativeSessionAction":"EXACT_NATIVE_QUEUE_DISPATCH_REQUEST",
                            "executionAuthority":"LOCAL_EXACT_ASSIGNMENT_KEY_AND_FROZEN_NATIVE_TASK",
                            "taskCompletionVerified":false,"conversationOriginAuthenticated":false,
                            "retryWithNewCallIdAllowed":false,"browserStarted":false,"wakeHostStarted":false}}})) },
                Err(reason) => JsonRpcResponse::error(response_id, -32002, reason),
            };
            traffic.finish(&mut response).await;
            response
        }).await {
            Ok(response) => response,
            Err(_) => JsonRpcResponse::error(id, -32002, "WEB_TASK_OUTCOME_UNKNOWN_NO_REPLAY".into()),
        };
    }
    if call.tool == "wake.manual_end_turn" {
        // Dropping the HTTP awaiter cannot cancel this bounded metadata turn;
        // the existing journal remains the no-replay authority on a lost reply.
        let response_id = id.clone();
        return match tokio::spawn(async move {
        let selection = call.selection();
        // Omit executionKey and the authentication envelope from observation.
        let observed_params = json!({"name":TOOL,"arguments":call.task_args()});
        let call = std::sync::Arc::new(call);
        let owned = call.clone();let root = control.clone();
        let admitted = tokio::task::spawn_blocking(move || authorize(&root, &owned)).await;
        match admitted {
            Ok(Ok(_)) => {},
            Ok(Err(reason)) => return JsonRpcResponse::error(response_id, -32002, reason),
            Err(_) => return JsonRpcResponse::error(response_id, -32002, "WEB_TASK_AUTHORIZATION_UNAVAILABLE".into()),
        }
        let traffic = crate::project_mcp::TrafficObservation::start_scoped(control.clone(), &selection, observed_params).await;
        let result = tokio::task::spawn_blocking(move || {
            crate::web_work::manual_wake(
                &control,
                call.pins(),
                &call.execution_key,
                &call.task_id,
                &call.call_id,
                call.arguments.clone(),
            )
        })
        .await;
        let mut response = match result {
            Ok(Ok(result)) => JsonRpcResponse::success(
                response_id,
                json!({"content":[{"type":"text","text":"Manual continuation intent recorded; delivery unverified."}],
                    "structuredContent":result,"isError":false,
                    "_meta":{"catdeskTaskLifecycle":{"status":"QUEUED_ONLY","nativeSessionAction":"NONE",
                        "isolatedWorktreeWriterClaimed":false,"browserStarted":false,"wakeHostStarted":false,
                        "executionAuthority":"LOCAL_EXACT_ASSIGNMENT_KEY_AND_MANUAL_WAKE_CAPABILITY",
                        "conversationOriginAuthenticated":false,"retryWithNewCallIdAllowed":false}}}),
            ),
            Ok(Err(reason)) => JsonRpcResponse::error(response_id, -32002, reason),
            Err(_) => {
                JsonRpcResponse::error(response_id, -32002, "WEB_TASK_OUTCOME_UNKNOWN_NO_REPLAY".into())
            }
        };
        traffic.finish(&mut response).await;
        response
        }).await {
            Ok(response) => response,
            Err(_) => JsonRpcResponse::error(id, -32002, "WEB_TASK_OUTCOME_UNKNOWN_NO_REPLAY".into()),
        };
    }

    let response_id = id.clone();
    // Detach only this existing bounded turn if its HTTP awaiter disappears.
    // The owned future still drains the task and releases its exact lease.
    match tokio::spawn(async move {
        let selection=call.selection();let args=call.task_args();
        let call=std::sync::Arc::new(call);let owned=call.clone();let root=control.clone();
        let archive=tokio::task::spawn_blocking(move||authorize(&root,&owned)).await
            .map_err(|_|"WEB_TASK_AUTHORIZATION_UNAVAILABLE".to_string())??;
        let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(2);
        let context=loop {
            match ProjectContext::bind_with_task(&control,selection.clone(),Some(&archive),180).await {
                Err(reason) if matches!(reason.as_str(),"PROJECT_MCP_TARGET_UNAVAILABLE"|"WEB_CONTEXT_REGISTRATION_BUSY"|"WEB_TARGET_WAKE_CONFIG_BUSY")
                    && tokio::time::Instant::now()<deadline=>tokio::time::sleep(std::time::Duration::from_millis(10)).await,
                result=>break result?,
            }
        };
        // Recheck permission after startup. Local revocation cannot overtake a
        // held project lease; a revocation that won the startup race refuses.
        let owned=call.clone();let root=control.clone();
        let permitted=tokio::task::spawn_blocking(move||{
            let scope=crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(&root,&owned.project_id)
                .map_err(|_|"WEB_TASK_PROJECT_UNAVAILABLE")?;
            let record=crate::web_work_state::WorkLedger::open(&root,&owned.project_id)?.record(&owned.request_id)?.ok_or("WEB_WORK_REQUEST_MISSING")?;
            crate::project_mcp_task::authorized_web_assignment(&scope,&record,&owned.execution_key).map(|_|())
        }).await.map_err(|_|"WEB_TASK_AUTHORIZATION_UNAVAILABLE".to_string()).and_then(|v|v);
        #[cfg(all(test,windows))]
        if permitted.is_ok(){pause_started_test_turn(&control).await;}
        let mut response=match permitted{
            Ok(())=>context.dispatch_task(response_id.clone(),&selection,args).await,
            Err(reason)=>JsonRpcResponse::error(response_id.clone(),-32002,reason),
        };
        let closed=context.close_task().await;
        if let Some(result)=response.result.as_mut(){
            let mut scope=context.metadata();
            if closed.is_ok(){scope["assignedTask"]["isolatedWorktreeWriterClaimed"]=json!(false);}
            result["_meta"]["catdeskProjectScope"]=scope;
            result["_meta"]["catdeskTaskLifecycle"]=json!({"status":if closed.is_ok(){"CLOSED"}else{"RECOVERY_REQUIRED"},
                "executionBudgetSeconds":180,"nativeSessionAction":"NONE","retryWithNewCallIdAllowed":false,
                "executionAuthority":"LOCAL_EXACT_ASSIGNMENT_KEY",
                "conversationOriginAuthenticated":false});
        }
        Ok::<_,String>(response)
    }).await {
        Ok(Ok(response))=>response,
        Ok(Err(reason))=>JsonRpcResponse::error(id,-32002,reason),
        Err(_)=>JsonRpcResponse::error(id,-32002,"WEB_TASK_OUTCOME_UNKNOWN_NO_REPLAY".into()),
    }
}

#[cfg(all(test, windows))]
pub(crate) mod tests {
    use super::*;
    use crate::delegated::autonomy_projects::{
        AutonomousProjectRegistryV1, resolve_registered_project_from_control_root,
    };
    use catdesk_wake::{
        protocol,
        store::{Phase, Receipt, Store},
    };
    use std::{fs, sync::Arc, time::Duration};

    fn sent(wake: &Store, event: &protocol::Event) {
        wake.publish(event).unwrap();
        let host = wake.host_lock().unwrap();
        let delivery = wake.claim(event, &host).unwrap();
        wake.transition(
            &event.event_id,
            &delivery.owner,
            Phase::Submitting,
            None,
            None,
        )
        .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: delivery.target.generation,
            target_digest: delivery.target.digest.clone(),
            message_digest: delivery.message_digest.clone(),
            sent_utc: catdesk_wake::store::now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        wake.transition(
            &event.event_id,
            &delivery.owner,
            Phase::Sent,
            Some(receipt),
            None,
        )
        .unwrap();
    }

    struct Fixture {
        control: PathBuf,
        pins: Value,
        key: String,
        task: PathBuf,
        native: PathBuf,
        keys: Vec<OwnedFixtureKey>,
        _native_queue: Option<crate::operator_codex_cli::NativeQueueFixture>,
    }

    struct OwnedFixtureKey {
        path: PathBuf,
        bytes: Vec<u8>,
    }

    impl OwnedFixtureKey {
        fn retain(expected: PathBuf, granted: &Value) -> Self {
            assert_eq!(granted["executionKeyFile"].as_str(), expected.to_str());
            let bytes = fs::read(&expected).unwrap();
            let key: String = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(key.len(), 64);
            assert!(
                key.bytes()
                    .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase())
            );
            Self {
                path: expected,
                bytes,
            }
        }
    }

    impl Drop for OwnedFixtureKey {
        fn drop(&mut self) {
            // Only this exact path, proved absent before its fixture grant,
            // may be removed. Never sweep the shared user key directory.
            let metadata = fs::symlink_metadata(&self.path).unwrap();
            use std::os::windows::fs::MetadataExt;
            assert!(
                metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.file_attributes() & 0x400 == 0
            );
            let parent = fs::symlink_metadata(self.path.parent().unwrap()).unwrap();
            assert!(
                parent.is_dir()
                    && !parent.file_type().is_symlink()
                    && parent.file_attributes() & 0x400 == 0
            );
            assert_eq!(fs::read(&self.path).unwrap(), self.bytes);
            fs::remove_file(&self.path).unwrap();
        }
    }

    fn fresh_fixture_key_path(control: &Path, project: &str) -> PathBuf {
        let record = crate::web_work_state::WorkLedger::open(control, project)
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let identity = protocol::digest(
            &serde_json::to_string(&(control, project, record.task_assignment.as_ref().unwrap()))
                .unwrap(),
        );
        let path = crate::delegated::autonomy_runtime::fixed_current_user_catdesk_state_root()
            .unwrap()
            .join("WebTaskKeys")
            .join(format!("{identity}.json"));
        assert!(
            !path.try_exists().unwrap(),
            "fixture cannot take ownership of an existing key"
        );
        path
    }
    fn fixture(id: &str) -> Fixture {
        fixture_with_manual_wake(id, false)
    }
    fn fixture_with_manual_wake(id: &str, allow_manual_wake: bool) -> Fixture {
        fixture_capabilities(id, allow_manual_wake, false)
    }
    fn native_fixture(id: &str) -> Fixture {
        fixture_capabilities(id, false, true)
    }

    pub(crate) fn with_native_context_fixture<T>(read: impl FnOnce(&Path, &Value) -> T) -> T {
        let fixture = native_fixture("alpha");
        read(&fixture.control, &fixture.pins)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn native_dispatch_slow_inspection_releases_shared_locks_and_peer_can_patch() {
        let (alpha, beta) = paired_fixtures(true);
        let entered = Arc::new(tokio::sync::Notify::new());
        let (release, released) = std::sync::mpsc::channel();
        let root = alpha.control.clone();
        let pins = alpha.pins.clone();
        let key = alpha.key.clone();
        let notified = entered.clone();
        let job = alpha._native_queue.as_ref().unwrap().job_file("alpha");
        let original_job = fs::read(&job).unwrap();
        let pending = tokio::task::spawn_blocking(move || {
            crate::web_work::prepare_native_dispatch_with_inspection(
                &root,
                pins,
                &key,
                "task-fixture",
                "slow-native-preparation",
                |scope, record, key, task| {
                    let inspected = crate::project_mcp_task::inspect_native_task_call(
                        scope, record, key, task,
                    )?;
                    notified.notify_one();
                    released
                        .recv_timeout(Duration::from_secs(45))
                        .map_err(|_| "CONTROLLED_INSPECTION_TIMEOUT".to_owned())?;
                    Ok(inspected)
                },
            )
        });
        tokio::time::timeout(Duration::from_secs(20), entered.notified())
            .await
            .unwrap();
        let project_lease_held =
            crate::operator_facade::acquire_web_work_lease(&alpha.control, "alpha").is_err();
        let root = alpha.control.clone();
        let shared_locks_free = tokio::task::spawn_blocking(move || {
            let registry = crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1::open_read_only(root.join(".catdesk/projects")).unwrap();
            let wake = Store::open(&crate::wake_protocol_client::wake_root_for_workspace(&root).unwrap()).unwrap();
            registry.with_registration_lock(|| wake.with_config_read_only(|_| Ok(()))).is_ok()
        }).await.unwrap();
        let journal = alpha
            .control
            .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal");
        let no_early_effect = !journal.exists() && fs::read(&job).unwrap() == original_job;
        let peer = tokio::time::timeout(Duration::from_secs(20), async {
            let preview = call(&beta, request(&beta, "peer-preview", "patch.preview", json!({"patchId":"peer-preparation-patch","operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 58 }"}]}))).await;
            if preview.get("error").is_some() { return preview; }
            call(&beta, request(&beta, "peer-apply", "patch.apply", json!({"patchId":"peer-preparation-patch"}))).await
        }).await;
        release.send(()).unwrap();
        let admission = tokio::time::timeout(Duration::from_secs(20), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        // This test stops at admission. Close its journal explicitly with a
        // known no-launch result; never leave an EXECUTING effect behind.
        admission.finish(&Ok(json!({"status":"prelaunch_refused","nativeTurnStarted":false,"reason":"CONTROLLED_PREPARATION_ONLY"}))).unwrap();
        assert!(project_lease_held && shared_locks_free && no_early_effect);
        let peer = peer.expect("a paused native preparation cannot stall another project");
        assert!(peer.get("error").is_none(), "{peer}");
        assert!(
            fs::read_to_string(beta.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 58 }")
        );
        for path in [
            alpha.task.join("src/lib.rs"),
            alpha.native.join("src/lib.rs"),
            beta.native.join("src/lib.rs"),
        ] {
            assert!(fs::read_to_string(path).unwrap().contains("{ 1 }"));
        }
        assert_eq!(fs::read(&job).unwrap(), original_job);
        assert!(!native_proof(&alpha).exists() && !native_proof(&beta).exists());
        drop(crate::operator_facade::acquire_web_work_lease(&alpha.control, "alpha").unwrap());
    }

    #[test]
    fn native_dispatch_rechecks_authority_after_inspection_before_any_queue_or_journal_effect() {
        for changed in ["alpha", "catdesk", "assignment", "target"] {
            let f = native_fixture("alpha");
            let job = f._native_queue.as_ref().unwrap().job_file("alpha");
            let original_job = fs::read(&job).unwrap();
            let error = crate::web_work::prepare_native_dispatch_with_inspection(
                &f.control,
                f.pins.clone(),
                &f.key,
                "task-fixture",
                "changed-native-preparation",
                |scope, record, key, task| {
                    let inspected = crate::project_mcp_task::inspect_native_task_call(
                        scope, record, key, task,
                    )?;
                    match changed {
                        "assignment" => {
                            let archive = f.control.join(
                                ".catdesk/project-mcp-tasks/alpha/task-fixture/assignment.json",
                            );
                            let mut bytes = fs::read(&archive).unwrap();
                            bytes.push(b'\n');
                            fs::write(archive, bytes).unwrap();
                        }
                        "target" => {
                            // Manual binding intentionally does not acquire the
                            // per-project Web lease. Its paired CAS can win
                            // during preparation; final admission must refuse.
                            crate::mcp::operator_bind_project_web_target(
                                &f.control,
                                "alpha",
                                "https://chatgpt.com/c/manual-preparation-successor",
                                f.pins["expectedTargetSha256"].as_str(),
                                f.pins["expectedTargetGeneration"].as_u64().unwrap(),
                            )
                            .unwrap();
                        }
                        project => {
                            let path = f.control.join(".catdesk/projects/projects.json");
                            let mut registry: AutonomousProjectRegistryV1 =
                                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                            registry
                                .projects
                                .iter_mut()
                                .find(|p| p.project_id == project)
                                .unwrap()
                                .chatgpt_target_history
                                .push(format!(
                                    "https://chatgpt.com/c/preparation-history-{project}"
                                ));
                            fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
                        }
                    }
                    Ok(inspected)
                },
            )
            .err()
            .expect("changed preparation authority must refuse");
            assert_eq!(
                error,
                match changed {
                    "assignment" => "WEB_TASK_GRANT_SCOPE_MISMATCH",
                    "target" => "WEB_CONTEXT_TARGET_CHANGED",
                    _ => "WEB_WORK_PROJECT_REGISTRATION_CHANGED",
                }
            );
            assert_eq!(fs::read(&job).unwrap(), original_job);
            assert!(
                !f.control
                    .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal")
                    .exists()
            );
            assert!(!native_proof(&f).exists());
            drop(crate::operator_facade::acquire_web_work_lease(&f.control, "alpha").unwrap());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn native_status_slow_lookup_releases_shared_locks_and_peer_can_patch() {
        let (alpha, beta) = paired_fixtures(true);
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let gate_key = alpha.control.join("native-status-alpha");
        TEST_GATES
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(gate_key.clone(), (entered.clone(), release.clone()));
        let control = alpha.control.clone();
        let body = request(&alpha, "slow-status", "native.codex.status", json!({}));
        let slow = tokio::spawn(async move {
            let req: JsonRpcRequest = serde_json::from_value(body).unwrap();
            serde_json::to_value(handle(control.to_str().unwrap(), &req).await).unwrap()
        });
        tokio::time::timeout(Duration::from_secs(15), entered.notified())
            .await
            .unwrap();
        let root = alpha.control.clone();
        let shared_locks_free=tokio::task::spawn_blocking(move || {
            let registry=crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1::open_read_only(root.join(".catdesk/projects")).unwrap();
            let wake=Store::open(&crate::wake_protocol_client::wake_root_for_workspace(&root).unwrap()).unwrap();
            registry.with_registration_lock(||wake.with_config_read_only(|_|Ok(()))).is_ok()
        }).await.unwrap();
        let peer=tokio::time::timeout(Duration::from_secs(20),async {
            let preview=call(&beta,request(&beta,"peer-preview","patch.preview",json!({"patchId":"peer-status-patch","operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 57 }"}]}))).await;
            if preview.get("error").is_some(){return preview;}
            call(&beta,request(&beta,"peer-apply","patch.apply",json!({"patchId":"peer-status-patch"}))).await
        }).await;
        release.notify_one();
        let status = tokio::time::timeout(Duration::from_secs(20), slow)
            .await
            .unwrap()
            .unwrap();
        TEST_GATES.get().unwrap().lock().unwrap().remove(&gate_key);
        assert!(
            shared_locks_free,
            "queue status lookup must not retain registry/Wake locks"
        );
        let peer = peer.expect("a paused peer status lookup cannot stall independent work");
        assert!(peer.get("error").is_none(), "{peer}");
        assert!(status.get("error").is_none(), "{status}");
        assert_eq!(
            status["result"]["structuredContent"]["journalChanged"],
            false
        );
        assert!(
            fs::read_to_string(beta.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 57 }")
        );
        for path in [
            alpha.task.join("src/lib.rs"),
            alpha.native.join("src/lib.rs"),
            beta.native.join("src/lib.rs"),
        ] {
            assert!(fs::read_to_string(path).unwrap().contains("{ 1 }"));
        }
        assert!(
            !alpha
                .control
                .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal")
                .exists()
        );
        assert!(!native_proof(&alpha).exists() && !native_proof(&beta).exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn native_status_counts_authorized_failed_reads_without_foreign_or_revoked_traffic() {
        fn pressure(f: &Fixture) -> crate::web_conversation_context::PressureStatus {
            let store = crate::web_conversation_context::WebContextStore::open_read_only(
                f.control.join(".catdesk/web-context"),
            )
            .unwrap();
            let key = store
                .active_key(f.pins["projectId"].as_str().unwrap())
                .unwrap()
                .unwrap();
            store.pressure(&key).unwrap()
        }
        let (alpha, beta) = paired_fixtures(true);
        for f in [&alpha, &beta] {
            let ok = call(
                f,
                request(f, "initial-status", "native.codex.status", json!({})),
            )
            .await;
            assert!(ok.get("error").is_none(), "{ok}");
            assert_eq!(pressure(f).recorded_events, 2);
        }
        let peer = pressure(&beta);
        let job = alpha._native_queue.as_ref().unwrap().job_file("alpha");
        let held = job.with_extension("held-test-job");
        fs::rename(&job, &held).unwrap();
        let failed = call(
            &alpha,
            request(
                &alpha,
                "missing-queue-status",
                "native.codex.status",
                json!({}),
            ),
        )
        .await;
        fs::rename(&held, &job).unwrap();
        assert_eq!(
            failed["error"]["message"],
            "WEB_TASK_NATIVE_TASK_UNAVAILABLE"
        );
        assert_eq!(pressure(&alpha).recorded_events, 4);
        assert_eq!(pressure(&beta), peer);
        let before = pressure(&alpha);
        let mut foreign = request(&alpha, "foreign-status", "native.codex.status", json!({}));
        foreign["params"]["arguments"]["executionKey"] = json!(beta.key);
        assert!(call(&alpha, foreign).await.get("error").is_some());
        assert_eq!(pressure(&alpha), before);
        assert_eq!(pressure(&beta), peer);
        crate::web_work::execution_permission(&alpha.control, alpha.pins.clone(), true).unwrap();
        let revoked = call(
            &alpha,
            request(&alpha, "revoked-status", "native.codex.status", json!({})),
        )
        .await;
        assert_eq!(revoked["error"]["message"], "WEB_TASK_GRANT_REVOKED");
        assert_eq!(pressure(&alpha), before);
        assert_eq!(pressure(&beta), peer);
        assert!(
            !alpha
                .control
                .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal")
                .exists()
        );
        assert!(!native_proof(&alpha).exists() && !native_proof(&beta).exists());
    }

    #[test]
    fn native_status_revalidates_prepared_project_and_controller_before_key_admission() {
        for changed in ["alpha", "catdesk"] {
            let f = native_fixture("alpha");
            let error = crate::web_work::native_status_with_preparation(
                &f.control,
                f.pins.clone(),
                &f.key,
                "task-fixture",
                |root, id| {
                    let prepared =
                        crate::delegated::autonomy_projects::PreparedProjectScopeV1::resolve(
                            root, id,
                        )
                        .unwrap();
                    let path = root.join(".catdesk/projects/projects.json");
                    let mut registry: AutonomousProjectRegistryV1 =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    // Keep Git/paired-target identity valid so the prepared
                    // record check, rather than the earlier root check, refuses.
                    registry
                        .projects
                        .iter_mut()
                        .find(|p| p.project_id == changed)
                        .unwrap()
                        .chatgpt_target_history
                        .push(format!("https://chatgpt.com/c/prepared-history-{changed}"));
                    fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
                    Ok(prepared)
                },
            )
            .err()
            .expect("changed registration must refuse");
            assert_eq!(error, "WEB_WORK_PROJECT_REGISTRATION_CHANGED");
            assert!(
                !f.control
                    .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal")
                    .exists()
            );
            assert!(!native_proof(&f).exists());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn native_status_rechecks_revocation_and_target_after_unlocked_lookup() {
        for rollover in [false, true] {
            let mut f = native_fixture("alpha");
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let gate = f.control.join("native-status-alpha");
            TEST_GATES
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .insert(gate.clone(), (entered.clone(), release.clone()));
            let root = f.control.clone();
            let body = request(&f, "delayed-status", "native.codex.status", json!({}));
            let pending = tokio::spawn(async move {
                let request: JsonRpcRequest = serde_json::from_value(body).unwrap();
                serde_json::to_value(handle(root.to_str().unwrap(), &request).await).unwrap()
            });
            tokio::time::timeout(Duration::from_secs(15), entered.notified())
                .await
                .unwrap();
            if rollover {
                native_rollover(&mut f);
            } else {
                crate::web_work::execution_permission(&f.control, f.pins.clone(), true).unwrap();
            }
            release.notify_one();
            let response = tokio::time::timeout(Duration::from_secs(20), pending)
                .await
                .unwrap()
                .unwrap();
            TEST_GATES.get().unwrap().lock().unwrap().remove(&gate);
            assert!(
                response.get("result").is_none(),
                "stale authority must not release its snapshot"
            );
            assert_eq!(
                response["error"]["message"],
                if rollover {
                    "WEB_CONTEXT_TARGET_CHANGED"
                } else {
                    "WEB_TASK_GRANT_REVOKED"
                }
            );
            if rollover {
                let fresh = call(
                    &f,
                    request(
                        &f,
                        "current-target-status",
                        "native.codex.status",
                        json!({}),
                    ),
                )
                .await;
                assert!(fresh.get("error").is_none(), "{fresh}");
                assert_eq!(
                    fresh["result"]["structuredContent"]["job"]["state"],
                    "READY"
                );
            }
            assert!(
                !f.control
                    .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal")
                    .exists()
            );
            assert!(!native_proof(&f).exists());
        }
    }
    fn fixture_capabilities(id: &str, allow_manual_wake: bool, enable_native: bool) -> Fixture {
        let (control, assignment, wake, pins, event) =
            crate::web_work::tests::assignment_fixture_for(id);
        let native_queue = if enable_native {
            let path = control.join(".catdesk/projects/projects.json");
            let mut registry: AutonomousProjectRegistryV1 =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            registry.maximum_codex_workers = 2;
            registry
                .projects
                .iter_mut()
                .find(|p| p.project_id == id)
                .unwrap()
                .codex_thread_id = Some(uuid::Uuid::new_v4().to_string());
            fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
            Some(crate::operator_codex_cli::native_queue_fixture(
                &control, id,
            ))
        } else {
            None
        };
        wake.publish(&event).unwrap();
        crate::web_work::associate(&control, pins.clone(), &assignment).unwrap();
        let task_data: Value = serde_json::from_slice(&fs::read(&assignment).unwrap()).unwrap();
        let task = PathBuf::from(task_data["contract"]["workspace"].as_str().unwrap());
        let native = resolve_registered_project_from_control_root(&control, id)
            .unwrap()
            .project
            .workspace;
        // A prepared association itself does not authorize central execution.
        assert!(
            crate::web_work::authorized_assignment(&control, pins.clone(), &"a".repeat(64))
                .is_err()
        );
        sent(&wake, &event);
        let expected_key = fresh_fixture_key_path(&control, id);
        let granted = crate::web_work::execution_permission_with_capabilities(
            &control,
            pins.clone(),
            false,
            allow_manual_wake,
            enable_native.then_some("native-fixture"),
        )
        .unwrap();
        let owned_key = OwnedFixtureKey::retain(expected_key, &granted);
        assert!(!Path::new(granted["executionKeyFile"].as_str().unwrap()).starts_with(&control));
        let key: String = serde_json::from_slice(
            &fs::read(granted["executionKeyFile"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let again = crate::web_work::execution_permission_with_capabilities(
            &control,
            pins.clone(),
            false,
            allow_manual_wake,
            enable_native.then_some("native-fixture"),
        )
        .unwrap();
        assert_eq!(granted, again);
        assert!(!granted.to_string().contains(&key));
        Fixture {
            control,
            pins,
            key,
            task,
            native,
            keys: vec![owned_key],
            _native_queue: native_queue,
        }
    }
    fn request(f: &Fixture, id: &str, tool: &str, args: Value) -> Value {
        let mut pins = f.pins.clone();
        let o = pins.as_object_mut().unwrap();
        o.insert("executionKey".into(), json!(f.key));
        o.insert("taskId".into(), json!("task-fixture"));
        o.insert("callId".into(), json!(id));
        o.insert("tool".into(), json!(tool));
        o.insert("arguments".into(), args);
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":TOOL,"arguments":pins}})
    }
    async fn call(f: &Fixture, body: Value) -> Value {
        let req = serde_json::from_value(body).unwrap();
        serde_json::to_value(handle(f.control.to_str().unwrap(), &req).await).unwrap()
    }

    fn ready_manual_wake(f: &Fixture) -> Store {
        let root = crate::wake_protocol_client::wake_root_for_workspace(&f.control).unwrap();
        // Controlled fixture times establish a past activation/target cutoff;
        // no clock sleep or production metadata is involved.
        let mut activation: catdesk_wake::runtime::Activation =
            catdesk_wake::store::read(&root.join("activation.json")).unwrap();
        activation.accept_after_utc = catdesk_wake::store::now() - 5;
        fs::write(
            root.join("activation.json"),
            serde_json::to_vec(&activation).unwrap(),
        )
        .unwrap();
        let mut config: catdesk_wake::store::Config =
            catdesk_wake::store::read(&root.join("config.json")).unwrap();
        for change in &mut config.history {
            change.changed_utc = activation.accept_after_utc;
        }
        fs::write(
            root.join("config.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        Store::open(&root).unwrap()
    }

    fn manual_request(f: &Fixture, call_id: &str, turn: &str) -> Value {
        request(
            f,
            call_id,
            "wake.manual_end_turn",
            json!({"sourceTurnId":turn,"priority":10,"ttlSeconds":300}),
        )
    }

    #[test]
    fn manual_wake_fixture_cleanup_is_exact_and_preserves_peer_key() {
        let alpha = fixture("alpha");
        let beta = fixture_with_manual_wake("beta", true);
        let alpha_path = alpha.keys[0].path.clone();
        let beta_path = beta.keys[0].path.clone();
        let beta_bytes = fs::read(&beta_path).unwrap();
        drop(alpha);
        assert!(!alpha_path.try_exists().unwrap());
        assert_eq!(fs::read(&beta_path).unwrap(), beta_bytes);
        drop(beta);
        assert!(!beta_path.try_exists().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manual_traffic_is_project_bound_counts_retries_and_refuses_foreign_keys() {
        use crate::web_conversation_context::WebContextStore;
        fn pressure(f: &Fixture) -> crate::web_conversation_context::PressureStatus {
            let store =
                WebContextStore::open_read_only(f.control.join(".catdesk/web-context")).unwrap();
            let key = store
                .active_key(f.pins["projectId"].as_str().unwrap())
                .unwrap()
                .unwrap();
            store.pressure(&key).unwrap()
        }
        let alpha = fixture_with_manual_wake("alpha", true);
        let beta = fixture_with_manual_wake("beta", true);
        ready_manual_wake(&alpha);
        ready_manual_wake(&beta);
        let a = manual_request(&alpha, "traffic-alpha", "turn-alpha");
        let b = manual_request(&beta, "traffic-beta", "turn-beta");
        let (ra, rb) = tokio::join!(call(&alpha, a.clone()), call(&beta, b));
        for response in [&ra, &rb] {
            assert!(response.get("error").is_none(), "{response}");
            let observed = &response["result"]["_meta"]["catdeskContextObservation"];
            assert_eq!(observed["arguments"], "RECORDED");
            assert_eq!(observed["result"], "RECORDED");
            assert_eq!(observed["originAuthenticated"], false);
            assert_eq!(observed["rawTextPersisted"], false);
        }
        let before_a = pressure(&alpha);
        let before_b = pressure(&beta);
        assert_eq!(before_a.recorded_events, 2);
        assert_eq!(before_b.recorded_events, 2);
        assert!(before_a.observed_tool_argument_tokens > 0);
        assert!(before_a.observed_tool_result_tokens > 0);
        assert!(!before_a.coverage_known);
        // A second actual invocation consumes context even when its effect is
        // suppressed by the journal. It cannot increment Beta's estimate.
        assert_eq!(
            call(&alpha, a.clone()).await["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        assert_eq!(pressure(&alpha).recorded_events, 4);
        assert_eq!(pressure(&beta), before_b);
        let before_denied = pressure(&alpha);
        let mut foreign = a;
        foreign["params"]["arguments"]["executionKey"] = json!(beta.key);
        assert!(call(&alpha, foreign).await.get("error").is_some());
        assert_eq!(pressure(&alpha), before_denied);
        assert_eq!(pressure(&beta), before_b);
    }

    #[test]
    fn manual_wake_grant_is_explicit_immutable_and_revocable() {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        #[allow(dead_code)]
        struct LegacyGrant {
            schema_version: u32,
            project_id: String,
            request_id: String,
            source_event_id: String,
            assignment: Value,
            key_sha256: String,
        }
        let legacy = fixture("alpha");
        let grant_path = legacy
            .control
            .join(".catdesk/project-mcp-tasks/alpha/task-fixture/execution-grant.json");
        let saved = fs::read(&grant_path).unwrap();
        let grant: Value = serde_json::from_slice(&saved).unwrap();
        assert!(grant.get("allowManualWake").is_none());
        assert!(serde_json::from_slice::<LegacyGrant>(&saved).is_ok());
        let scope = resolve_registered_project_from_control_root(&legacy.control, "alpha").unwrap();
        let record = crate::web_work_state::WorkLedger::open(&legacy.control, "alpha")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let context =
            crate::project_mcp_task::granted_handoff_context(&scope, &record, &record.target)
                .unwrap()
                .unwrap();
        assert!(context.get("manualWake").is_none());
        assert_eq!(
            crate::web_work::execution_permission_with_manual_wake(
                &legacy.control,
                legacy.pins.clone(),
                false,
                true
            )
            .unwrap_err(),
            "PROJECT_TASK_ARCHIVE_CONFLICT"
        );
        assert_eq!(fs::read(&grant_path).unwrap(), saved);
        assert!(
            crate::web_work::authorized_assignment(
                &legacy.control,
                legacy.pins.clone(),
                &legacy.key
            )
            .is_ok()
        );
        let explicit = fixture_with_manual_wake("beta", true);
        let grant: Value = serde_json::from_slice(
            &fs::read(
                explicit
                    .control
                    .join(".catdesk/project-mcp-tasks/beta/task-fixture/execution-grant.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(grant["allowManualWake"], true);
        assert!(
            serde_json::from_value::<LegacyGrant>(grant).is_err(),
            "old runtime must refuse new capability rather than silently downgrade"
        );
        assert!(
            crate::web_work::authorized_assignment(
                &explicit.control,
                explicit.pins.clone(),
                &explicit.key
            )
            .is_ok()
        );
        let scope =
            resolve_registered_project_from_control_root(&explicit.control, "beta").unwrap();
        let record = crate::web_work_state::WorkLedger::open(&explicit.control, "beta")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let context =
            crate::project_mcp_task::granted_handoff_context(&scope, &record, &record.target)
                .unwrap()
                .unwrap();
        assert_eq!(context["manualWake"]["allowed"], true);
        crate::web_work::execution_permission(&explicit.control, explicit.pins.clone(), true)
            .unwrap();
        assert_eq!(
            crate::web_work::authorized_assignment(
                &explicit.control,
                explicit.pins.clone(),
                &explicit.key
            )
            .unwrap_err(),
            "WEB_TASK_GRANT_REVOKED"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manual_wake_checks_exact_capability_identity_and_never_claims_native_writer() {
        let legacy = fixture("alpha");
        ready_manual_wake(&legacy);
        assert_eq!(
            call(&legacy, manual_request(&legacy, "legacy-manual", "turn-1")).await["error"]["message"],
            "WEB_TASK_MANUAL_WAKE_NOT_GRANTED"
        );
        assert!(
            !legacy
                .control
                .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal")
                .exists()
        );
        let f = fixture_with_manual_wake("beta", true);
        let wake = ready_manual_wake(&f);
        let body = manual_request(&f, "manual-first", "turn-1");
        for (field, value) in [
            ("executionKey", json!("b".repeat(64))),
            ("taskId", json!("foreign-task")),
            ("sourceEventId", json!("foreign-source")),
            ("assignmentSha256", json!("b".repeat(64))),
            ("expectedTargetGeneration", json!(999)),
        ] {
            let mut invalid = body.clone();
            invalid["params"]["arguments"][field] = value;
            assert!(call(&f, invalid).await.get("error").is_some());
        }
        for arguments in [
            json!({"sourceTurnId":"turn-1","priority":10,"ttlSeconds":0}),
            json!({"sourceTurnId":"turn-1","priority":256,"ttlSeconds":300}),
            json!({"sourceTurnId":"turn-1","priority":10,"ttlSeconds":86401}),
            json!({"sourceTurnId":"turn-1","priority":10,"ttlSeconds":300,"message":"arbitrary text"}),
        ] {
            let mut invalid = body.clone();
            invalid["params"]["arguments"]["arguments"] = arguments;
            assert_eq!(
                call(&f, invalid).await["error"]["message"],
                "WEB_TASK_MANUAL_WAKE_ARGUMENTS_INVALID"
            );
        }
        let native_before = fs::read(f.native.join("src/lib.rs")).unwrap();
        let writer =
            crate::delegated::autonomy_runtime::acquire_project_writer_lock(&f.task).unwrap();
        let result = call(&f, body).await;
        assert!(result.get("error").is_none(), "{result}");
        assert_eq!(
            result["result"]["_meta"]["catdeskTaskLifecycle"]["isolatedWorktreeWriterClaimed"],
            false
        );
        assert_eq!(
            result["result"]["structuredContent"]["deliveryProved"],
            false
        );
        let id = result["result"]["structuredContent"]["eventId"]
            .as_str()
            .unwrap();
        let event = wake
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_id == id)
            .unwrap();
        assert_eq!(event.project_id, "beta");
        assert_eq!(
            event.wake_metadata().unwrap().unwrap().purpose,
            protocol::WakePurpose::ManualEndTurn
        );
        assert!(!event.message.contains(&f.key));
        assert!(!event.message.contains(f.native.to_str().unwrap()));
        assert!(event.message.contains("Project: beta"));
        assert_eq!(
            fs::read(f.native.join("src/lib.rs")).unwrap(),
            native_before
        );
        drop(writer);
        let ordinary = call(
            &f,
            request(
                &f,
                "after-manual-read",
                "read",
                json!({"path":"src/lib.rs"}),
            ),
        )
        .await;
        assert!(
            ordinary.get("error").is_none(),
            "manual journal stranded normal task: {ordinary}"
        );
        crate::web_work::execution_permission(&f.control, f.pins.clone(), true).unwrap();
        assert_eq!(
            call(&f, manual_request(&f, "revoked-manual", "turn-2")).await["error"]["message"],
            "WEB_TASK_GRANT_REVOKED"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manual_wake_dedupes_restart_and_preserves_material_conflicts_and_unknown_delivery() {
        let f = fixture_with_manual_wake("alpha", true);
        let wake = ready_manual_wake(&f);
        let first = call(&f, manual_request(&f, "manual-1", "turn-1")).await;
        assert!(first.get("error").is_none(), "{first}");
        let result = &first["result"]["structuredContent"];
        let id = result["eventId"].as_str().unwrap();
        let queued = wake.root().join("queue").join(format!("{id}.json"));
        let saved = fs::read(&queued).unwrap();
        assert_eq!(
            call(&f, manual_request(&f, "manual-1", "turn-1")).await["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        let cold = call(&f, manual_request(&f, "manual-2", "turn-1")).await;
        assert_eq!(cold["result"]["structuredContent"], *result);
        assert_eq!(fs::read(&queued).unwrap(), saved);
        let mut conflict = manual_request(&f, "manual-conflict", "turn-1");
        conflict["params"]["arguments"]["arguments"]["priority"] = json!(11);
        assert_eq!(
            call(&f, conflict).await["error"]["message"],
            "QUEUE_EVENT_ID_CONFLICT"
        );
        let distinct = call(&f, manual_request(&f, "manual-next", "turn-2")).await;
        assert!(distinct.get("error").is_none(), "{distinct}");
        assert_ne!(
            distinct["result"]["structuredContent"]["eventId"],
            result["eventId"]
        );
        let event: protocol::Event = serde_json::from_slice(&saved).unwrap();
        let host = wake.host_lock().unwrap();
        let delivery = wake.claim(&event, &host).unwrap();
        wake.transition(id, &delivery.owner, Phase::Submitting, None, None)
            .unwrap();
        drop(host);
        let delivery_path = wake.root().join("deliveries").join(format!("{id}.json"));
        let unknown = fs::read(&delivery_path).unwrap();
        let retry = call(&f, manual_request(&f, "manual-known-identity", "turn-1")).await;
        assert_eq!(retry["result"]["structuredContent"], *result);
        assert_eq!(fs::read(&delivery_path).unwrap(), unknown);
        assert_eq!(fs::read(&queued).unwrap(), saved);
        assert_eq!(
            wake.events()
                .unwrap()
                .iter()
                .filter(|event| event.wake_metadata().unwrap().is_some())
                .count(),
            2
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manual_wake_unknown_journal_fences_new_identities_and_budget_is_shared() {
        use crate::delegated::{
            contracts::{ExecutionContractV1, RunId, ToolCallId},
            journal::{DelegatedJournal, ToolCallRecordV1, ToolCallStatus, ToolMutationKind},
        };
        let f = fixture_with_manual_wake("alpha", true);
        let wake = ready_manual_wake(&f);
        let first = call(&f, manual_request(&f, "manual-initial", "turn-1")).await;
        assert!(first.get("error").is_none(), "{first}");
        let journal = DelegatedJournal::open(
            f.control
                .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal"),
        )
        .unwrap();
        let run = RunId::new("task-fixture").unwrap();
        let add = |id: &str, completed: bool| {
            let call = ToolCallId::new(id).unwrap();
            journal
                .record_tool_call_requested(ToolCallRecordV1 {
                    schema_version: 1,
                    run_id: run.clone(),
                    tool_call_id: call.clone(),
                    tool_name: "read".into(),
                    request_hash: "fixture".into(),
                    arguments_hash: "fixture".into(),
                    mutation_kind: ToolMutationKind::ReadOnly,
                    status: ToolCallStatus::Requested,
                    result_hash: None,
                    outcome_summary: None,
                })
                .unwrap();
            for phase in [
                ToolCallStatus::PolicyAllowed,
                ToolCallStatus::Executing,
                if completed {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::OutcomeUnknown
                },
            ] {
                journal
                    .transition_tool_call(&run, &call, phase, None, None)
                    .unwrap();
            }
        };
        let contract: ExecutionContractV1 = journal.load_contract(&run).unwrap();
        for index in 1..contract.max_tool_calls {
            add(&format!("budget-{index}"), true);
        }
        let queue_before = wake.events().unwrap();
        assert_eq!(
            call(&f, manual_request(&f, "over-budget", "new-turn")).await["error"]["message"],
            "PROJECT_TASK_TOOL_BUDGET_EXCEEDED"
        );
        assert_eq!(wake.events().unwrap(), queue_before);
        // A separate task with budget headroom still fences a new identity
        // when its earlier call has an ambiguous journal outcome.
        let g = fixture_with_manual_wake("beta", true);
        let beta = ready_manual_wake(&g);
        let initial = call(&g, manual_request(&g, "manual-initial", "turn-1")).await;
        assert!(initial.get("error").is_none(), "{initial}");
        let beta_journal = DelegatedJournal::open(
            g.control
                .join(".catdesk/project-mcp-tasks/beta/task-fixture/journal"),
        )
        .unwrap();
        beta_journal
            .record_tool_call_requested(ToolCallRecordV1 {
                schema_version: 1,
                run_id: run.clone(),
                tool_call_id: ToolCallId::new("ambiguous-prior").unwrap(),
                tool_name: "wake.manual_end_turn".into(),
                request_hash: "fixture".into(),
                arguments_hash: "fixture".into(),
                mutation_kind: ToolMutationKind::Mutating,
                status: ToolCallStatus::Requested,
                result_hash: None,
                outcome_summary: None,
            })
            .unwrap();
        let before = beta.events().unwrap();
        assert_eq!(
            call(
                &g,
                manual_request(&g, "new-call-to-evade", "new-turn-to-evade")
            )
            .await["error"]["message"],
            "PROJECT_TASK_RECOVERY_REQUIRED"
        );
        assert_eq!(beta.events().unwrap(), before);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn exact_grant_survives_cold_calls_refuses_wrong_identity_and_revokes() {
        let f = fixture("alpha");
        let fresh = request(&f, "first-read", "read", json!({"path":"src/lib.rs"}));
        let task_dir = f
            .control
            .join(".catdesk/project-mcp-tasks/alpha/task-fixture");
        for (field, value) in [
            ("executionKey", json!("b".repeat(64))),
            ("taskId", json!("other-task")),
            ("sourceEventId", json!("wrong-event")),
            ("assignmentSha256", json!("b".repeat(64))),
            ("expectedTargetGeneration", json!(999)),
            ("workspace", json!(f.native)),
        ] {
            let mut invalid = fresh.clone();
            invalid["params"]["arguments"][field] = value;
            assert!(call(&f, invalid).await.get("error").is_some());
            assert!(
                !task_dir.join("journal").exists(),
                "wrong identity opened task writer"
            );
        }
        let result = call(&f, fresh.clone()).await;
        assert!(result.get("error").is_none(), "{result}");
        assert_eq!(
            result["result"]["_meta"]["catdeskTaskLifecycle"]["status"],
            "CLOSED"
        );
        assert_eq!(
            result["result"]["_meta"]["catdeskProjectScope"]["projectId"],
            "alpha"
        );
        assert_eq!(
            result["result"]["_meta"]["catdeskProjectScope"]["assignedTask"]["isolatedWorktreeWriterClaimed"],
            false
        );
        assert_eq!(
            call(&f, fresh).await["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        let read_again = call(
            &f,
            request(&f, "second-read", "read", json!({"path":"src/lib.rs"})),
        )
        .await;
        assert!(read_again.get("error").is_none(), "{read_again}");
        crate::web_work::execution_permission(&f.control, f.pins.clone(), true).unwrap();
        assert_eq!(
            call(
                &f,
                request(&f, "revoked-read", "read", json!({"path":"src/lib.rs"}))
            )
            .await["error"]["message"],
            "WEB_TASK_GRANT_REVOKED"
        );
        assert_eq!(
            crate::web_work::execution_permission(&f.control, f.pins.clone(), false).unwrap_err(),
            "WEB_TASK_GRANT_REVOKED"
        );
        drop(crate::delegated::autonomy_runtime::acquire_project_writer_lock(&f.task).unwrap());
        drop(crate::operator_facade::acquire_web_work_lease(&f.control, "alpha").unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_awaiter_still_drains_exact_patch_and_refuses_replay() {
        let f = fixture("alpha");
        let preview=call(&f,request(&f,"cancel-preview","patch.preview",json!({"patchId":"cancel-patch","operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 49 }"}]}))).await;
        assert!(preview.get("error").is_none(), "{preview}");
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        TEST_GATES
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(f.control.clone(), (entered.clone(), release.clone()));
        let apply = request(
            &f,
            "cancel-apply",
            "patch.apply",
            json!({"patchId":"cancel-patch"}),
        );
        let req = serde_json::from_value(apply.clone()).unwrap();
        let root = f.control.clone();
        let awaiter = tokio::spawn(async move { handle(root.to_str().unwrap(), &req).await });
        tokio::time::timeout(Duration::from_secs(10), entered.notified())
            .await
            .unwrap();
        assert!(crate::delegated::autonomy_runtime::acquire_project_writer_lock(&f.task).is_err());
        awaiter.abort();
        assert!(matches!(awaiter.await,Err(error) if error.is_cancelled()));
        release.notify_one();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(lease) = crate::operator_facade::acquire_web_work_lease(&f.control, "alpha") {
                drop(lease);
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "cancelled turn did not release its project lease"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        TEST_GATES.get().unwrap().lock().unwrap().remove(&f.control);
        assert!(
            fs::read_to_string(f.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 49 }")
        );
        assert!(
            fs::read_to_string(f.native.join("src/lib.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
        assert_eq!(
            call(&f, apply).await["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        drop(crate::delegated::autonomy_runtime::acquire_project_writer_lock(&f.task).unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn one_central_http_route_patches_two_projects_and_busy_alpha_does_not_stall_beta() {
        central_http_route_fixture(false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cached_schema_route_uses_same_scoped_dispatcher_and_no_replay_journal() {
        central_http_route_fixture(true).await;
    }

    async fn start_frozen_cycle_backend(
        image: &Path,
        control: &Path,
        route: &str,
        token: &str,
    ) -> (std::net::SocketAddr, String, tokio::process::Child) {
        use sha2::{Digest, Sha256};
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

        assert!(image.is_absolute() && fs::metadata(image).unwrap().len() <= 128 * 1024 * 1024);
        let digest = format!("{:x}", Sha256::digest(fs::read(image).unwrap()));
        let expected = std::env::var("CATDESK_FROZEN_BACKEND_TEST_SHA256").unwrap();
        assert_eq!(digest, expected);
        let config = control.join(".catdesk/web-development/catdesk.toml");
        let mut command = tokio::process::Command::new(image);
        command
            .current_dir(control)
            .args([
                "--headless-mcp",
                "--host",
                "127.0.0.1",
                "--port",
                "0",
                "--workspace",
            ])
            .arg(control)
            .arg("--config-path")
            .arg(&config)
            .args([
                "--mcp-path",
                route,
                "--mode",
                "computer",
                "--tool-mode",
                "supervisor-only",
                "--no-ngrok",
                "--no-web-auto",
                "--max-runtime-seconds",
                "60",
            ])
            .env("CATDESK_MCP_AUTH_TOKEN", token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).take(8193);
        let mut ready = Vec::new();
        let read = tokio::time::timeout(
            Duration::from_secs(12),
            output.read_until(b'\n', &mut ready),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            (1..=8192).contains(&read),
            "frozen backend did not publish bounded readiness"
        );
        let descriptor: Value = serde_json::from_slice(&ready).unwrap();
        assert_eq!(descriptor["status"], "ready");
        assert_eq!(descriptor["web_auto_scheduler_enabled"], false);
        assert_eq!(
            Path::new(descriptor["workspace"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            control.canonicalize().unwrap()
        );
        let url = descriptor["mcp_url"].as_str().unwrap().to_owned();
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert_eq!(parsed.scheme(), "http");
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert_eq!(parsed.path(), route);
        assert!(parsed.username().is_empty() && parsed.password().is_none());
        assert!(parsed.query().is_none() && parsed.fragment().is_none());
        let port = parsed.port().filter(|port| *port != 3200).unwrap();
        (([127, 0, 0, 1], port).into(), url, child)
    }

    async fn qualify_frozen_manual_wake_admission(
        image: &Path,
        f: &Fixture,
        wake: &Store,
        source: &protocol::Event,
        event_id: &str,
    ) {
        let wake_root = catdesk_wake::development_runtime::root_for_workspace(&f.control)
            .unwrap()
            .unwrap();
        assert_eq!(wake.root(), wake_root);
        // The final refusal is a real Windows image-load failure. The fixture
        // interpreter is intentionally inert, so no browser can be started.
        assert_eq!(
            fs::read(wake.root().join("runtime/python.exe")).unwrap(),
            b"inert fixture Python"
        );
        let queue_path = wake.root().join("queue").join(format!("{event_id}.json"));
        let submission_start = wake
            .root()
            .join("logs")
            .join(format!("submission-start-{event_id}.json"));
        let queued = fs::read(&queue_path).unwrap();
        let event: protocol::Event = serde_json::from_slice(&queued).unwrap();
        let target = wake.config().unwrap().targets[&source.project_id].clone();
        let source_delivery = wake.delivery(&source.event_id).unwrap().unwrap();
        let invoke = |call_id| async move {
            let output = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::process::Command::new(image)
                    .current_dir(&f.control)
                    .args([
                        "operator",
                        "web",
                        "work",
                        "manual-wake-deliver",
                        "--project-id",
                        &source.project_id,
                        "--request-id",
                        "request-a",
                        "--call-id",
                        call_id,
                        "--event-id",
                        event_id,
                    ])
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .expect("frozen manual Wake operator exceeded its fixture bound")
            .unwrap();
            assert_eq!(output.status.code(), Some(2));
            serde_json::from_slice::<Value>(&output.stdout).unwrap()
        };

        assert_eq!(
            invoke("wrong-call").await["errorCode"],
            "WEB_TASK_MANUAL_WAKE_CALL_UNPROVEN"
        );
        assert_eq!(
            invoke("end-turn").await["errorCode"],
            "SCOPED_REVIEW_PROJECT_BUSY"
        );
        assert!(wake.delivery(event_id).unwrap().is_none());
        assert!(!submission_start.try_exists().unwrap());
        assert_eq!(fs::read(&queue_path).unwrap(), queued);

        // The source receipt was synthetic. In live use only an attributed
        // completed browser response permits this timer transition.
        wake.complete_timer(&source.event_id).unwrap();
        assert_eq!(
            invoke("end-turn").await["errorCode"],
            "BROWSER_ADAPTER_START_FAILED"
        );
        let delivery = wake.delivery(event_id).unwrap().unwrap();
        assert_eq!(delivery.phase, Phase::Attention);
        assert_eq!(delivery.event, event);
        assert_eq!(delivery.target, target);
        assert_eq!(
            delivery.reason.as_deref(),
            Some("BROWSER_ADAPTER_START_FAILED")
        );
        assert!(delivery.receipt.is_none());
        assert_eq!(
            invoke("end-turn").await["errorCode"],
            "SCOPED_REVIEW_REPLAY_REFUSED"
        );
        assert_eq!(wake.delivery(event_id).unwrap(), Some(delivery));
        assert!(!submission_start.try_exists().unwrap());
        assert!(wake.timer(event_id).unwrap().is_none());
        assert_eq!(fs::read(queue_path).unwrap(), queued);
        assert_eq!(
            wake.delivery(&source.event_id).unwrap(),
            Some(source_delivery)
        );
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "explicit bounded qualification with the installed native Windows command sandbox"]
    async fn cached_http_checkpoint_precedes_manual_wake() {
        let project = "cached-http-cycle";
        let (control, assignment, wake, mut pins, event) =
            crate::web_work::tests::assignment_fixture_for(project);
        let mut envelope: Value = serde_json::from_slice(&fs::read(&assignment).unwrap()).unwrap();
        let task = PathBuf::from(envelope["contract"]["workspace"].as_str().unwrap());
        let findstr =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/findstr.exe");
        envelope["contract"]["allowedCommandProfiles"] = json!(["check-content"]);
        envelope["commandProfiles"] = json!([{
            "id":"check-content", "argv":[findstr, "/L", "/C:{ 42 }", r"src\lib.rs"], "timeoutSeconds":25
        }]);
        envelope["checkpointPaths"] = json!(["src/lib.rs"]);
        let content = serde_json::to_string_pretty(&envelope).unwrap();
        fs::write(&assignment, &content).unwrap();
        pins["assignmentSha256"] = json!(protocol::digest(&content));
        wake.publish(&event).unwrap();
        crate::web_work::associate(&control, pins.clone(), &assignment).unwrap();
        sent(&wake, &event);
        let key_path = fresh_fixture_key_path(&control, project);
        let grant = crate::web_work::execution_permission_with_git_capabilities(
            &control,
            pins.clone(),
            false,
            true,
            None,
            true,
            false,
        )
        .unwrap();
        let owned_key = OwnedFixtureKey::retain(key_path, &grant);
        let native = resolve_registered_project_from_control_root(&control, project)
            .unwrap()
            .project
            .workspace;
        let key = serde_json::from_slice(&owned_key.bytes).unwrap();
        let f = Fixture {
            control,
            pins,
            key,
            task,
            native,
            keys: vec![owned_key],
            _native_queue: None,
        };
        let wake = ready_manual_wake(&f);
        let original = fs::read(f.native.join("src/lib.rs")).unwrap();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let path = "/cached-cycle/mcp";
        let frozen_image = std::env::var_os("CATDESK_FROZEN_BACKEND_TEST_IMAGE");
        let (addr, url, mut child, local_server) = if let Some(image) = frozen_image.as_deref() {
            let (addr, url, child) =
                start_frozen_cycle_backend(Path::new(image), &f.control, path, &token).await;
            (addr, url, Some(child), None)
        } else {
            let mut app = crate::state::AppState::new_isolated_fixture(
                0,
                f.control.to_str().unwrap().into(),
                f.control.join("inert-config.toml"),
            )
            .unwrap();
            app.mode = crate::state::Mode::Computer;
            app.tool_mode = crate::state::ToolMode::SupervisorOnly;
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let state = Arc::new(tokio::sync::Mutex::new(app));
            crate::configure_native_mcp_auth(&state, Ok(token.clone()))
                .await
                .unwrap();
            let router = crate::native_mcp_router(state, None, path.into(), tx).await;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (stop, stopped) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await
                    .unwrap();
            });
            (
                addr,
                format!("http://{addr}{path}"),
                None,
                Some((stop, server)),
            )
        };
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let transport = |mut body: Value| {
            let command = format!("{}{}", CACHED_COMMAND_PREFIX, body["params"]["arguments"]);
            body["params"] = json!({"name":"run_command","arguments":{"command":command}});
            body
        };
        let rpc = |body| {
            let request = client.post(&url).bearer_auth(&token).json(&transport(body));
            async move {
                request
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap()
                    .json::<Value>()
                    .await
                    .unwrap()
            }
        };
        let expect_success = |answer: &Value| {
            assert!(answer.get("error").is_none());
            assert_eq!(answer["result"]["isError"], false);
            assert_eq!(
                answer["result"]["_meta"]["catdeskTaskTransport"]["shellExecuted"],
                false
            );
        };

        let early = manual_request(&f, "too-early", "fixture-turn");
        let unauthorized = client
            .post(&url)
            .json(&transport(early.clone()))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(
            rpc(early).await["error"]["message"],
            "PROJECT_TASK_END_TURN_CHECKPOINT_REQUIRED"
        );
        let preview = request(
            &f,
            "preview",
            "patch.preview",
            json!({
                "patchId":"fixture-patch",
                "operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 42 }"}]
            }),
        );
        expect_success(&rpc(preview).await);
        let apply = request(
            &f,
            "apply",
            "patch.apply",
            json!({"patchId":"fixture-patch"}),
        );
        expect_success(&rpc(apply).await);
        let verify = request(
            &f,
            "verify",
            "verify.command",
            json!({"profile":"check-content"}),
        );
        let result = rpc(verify).await;
        expect_success(&result);
        let verified = &result["result"]["structuredContent"]["payload"];
        assert_eq!(verified["success"], true);
        assert_eq!(verified["modelCalls"], 0);
        assert_eq!(verified["cleanupProved"], true);
        let checkpoint = request(
            &f,
            "checkpoint",
            "git.checkpoint",
            json!({"verificationCallIds":["verify"]}),
        );
        let saved = rpc(checkpoint.clone()).await;
        expect_success(&saved);
        assert_eq!(
            saved["result"]["structuredContent"]["payload"]["status"],
            "CHECKPOINTED"
        );
        let git = std::process::Command::new("git")
            .arg("-C")
            .arg(&f.task)
            .args(["show", "HEAD:src/lib.rs"])
            .output()
            .unwrap();
        assert!(git.status.success());
        assert!(String::from_utf8(git.stdout).unwrap().contains("{ 42 }"));
        assert_eq!(
            rpc(checkpoint).await["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        let intent = manual_request(&f, "end-turn", "fixture-turn");
        let queued = rpc(intent.clone()).await;
        expect_success(&queued);
        let event_id = queued["result"]["structuredContent"]["eventId"]
            .as_str()
            .unwrap();
        assert!(wake.delivery(event_id).unwrap().is_none());
        assert!(wake.timer(event_id).unwrap().is_none());
        assert_eq!(
            rpc(intent).await["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        assert_eq!(
            wake.events()
                .unwrap()
                .iter()
                .filter(|event| event.wake_metadata().unwrap().is_some())
                .count(),
            1
        );
        assert_eq!(fs::read(f.native.join("src/lib.rs")).unwrap(), original);
        if let Some(image) = frozen_image.as_deref() {
            qualify_frozen_manual_wake_admission(Path::new(image), &f, &wake, &event, event_id)
                .await;
        } else {
            let retry = rpc(manual_request(&f, "end-turn-retry", "fixture-turn")).await;
            expect_success(&retry);
            assert_eq!(
                retry["result"]["structuredContent"],
                queued["result"]["structuredContent"]
            );
            assert!(wake.delivery(event_id).unwrap().is_none());
            assert_eq!(
                wake.events()
                    .unwrap()
                    .iter()
                    .filter(|event| event.wake_metadata().unwrap().is_some())
                    .count(),
                1
            );
            // Keep transport, verification and Git real. Browser effects
            // are simulated only after the existing guarded submission fence.
            assert_eq!(
                fs::read(wake.root().join("runtime/python.exe")).unwrap(),
                b"inert fixture Python"
            );
            wake.complete_timer(&event.event_id).unwrap();
            let queue_path = wake.root().join("queue").join(format!("{event_id}.json"));
            let before = fs::read(&queue_path).unwrap();
            let assigned_file = f.task.join("src/lib.rs");
            let verified_bytes = fs::read(&assigned_file).unwrap();
            let mut changed = verified_bytes.clone();
            changed.extend_from_slice(b"\n// Unverified change after the queued Wake\n");
            fs::write(&assigned_file, changed).unwrap();
            assert_eq!(
                crate::web_work::deliver_next_manual_wake(&f.control, project).unwrap_err(),
                "PROJECT_TASK_END_TURN_CHECKPOINT_REQUIRED"
            );
            assert_eq!(
                crate::web_work::deliver_manual_wake(
                    &f.control,
                    project,
                    "request-a",
                    "end-turn",
                    event_id
                )
                .unwrap_err(),
                "PROJECT_TASK_END_TURN_CHECKPOINT_REQUIRED"
            );
            assert_eq!(fs::read(queue_path).unwrap(), before);
            assert!(wake.delivery(event_id).unwrap().is_none());
            assert!(wake.timer(event_id).unwrap().is_none());
            assert!(
                !wake
                    .root()
                    .join("logs")
                    .join(format!("submission-start-{event_id}.json"))
                    .exists()
            );
            fs::write(&assigned_file, &verified_bytes).unwrap();
            let interrupted =
                crate::web_work::fixture_manual_submission_fence(&f.control, project).unwrap();
            assert_eq!(interrupted["phase"], "SUBMITTING");
            assert_eq!(interrupted["eventId"], event_id);
            assert_eq!(interrupted["requestId"], "request-a");
            assert_eq!(interrupted["taskCallId"], "end-turn");
            let prior = wake.delivery(event_id).unwrap().unwrap();
            assert!(prior.receipt.is_none());
            assert!(wake.timer(event_id).unwrap().is_none());
            let scope = resolve_registered_project_from_control_root(&f.control, project).unwrap();
            let ledger =
                crate::web_work_state::WorkLedger::open_read_only(&f.control, project).unwrap();
            let record = ledger.record("request-a").unwrap().unwrap();
            crate::project_mcp_task::require_journaled_manual_wake(&scope, &record, &prior)
                .unwrap();
            let receipt = Receipt {
                schema_version: 1,
                event_id: event_id.into(),
                target_generation: prior.target.generation,
                target_digest: prior.target.digest.clone(),
                message_digest: prior.message_digest.clone(),
                sent_utc: catdesk_wake::store::now(),
                evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
            };
            wake.reconcile_task_continuation_sent_expected(event_id, receipt.clone(), &prior)
                .unwrap();
            let resolution = json!({"projectId":project,"requestId":"request-a",
                "expectedTargetGeneration":prior.target.generation,"expectedTargetSha256":prior.target.digest,
                "outcome":"completed","summary":"Fixture continuation completed"});
            assert_eq!(
                crate::web_work::resolve(&f.control, resolution.clone()).unwrap_err(),
                "WEB_WORK_CONTINUATION_PENDING"
            );

            // This is a fresh authenticated MCP call under the original grant,
            // with the checkpoint at the exact current HEAD on the new turn.
            let next_read = request(&f, "after-self-wake", "read", json!({"path":"src/lib.rs"}));
            let answer = rpc(next_read.clone()).await;
            expect_success(&answer);
            assert!(
                answer["result"]["structuredContent"]["payload"]
                    .to_string()
                    .contains("{ 42 }")
            );
            assert_eq!(
                rpc(next_read).await["error"]["message"],
                "PROJECT_TASK_CALL_ALREADY_COMPLETED"
            );
            let run = crate::delegated::contracts::RunId::new("task-fixture").unwrap();
            let journal = crate::delegated::journal::DelegatedJournal::open(
                &f.control
                    .join(".catdesk/project-mcp-tasks")
                    .join(project)
                    .join("task-fixture/journal"),
            )
            .unwrap();
            let calls = journal.load_tool_calls(&run).unwrap();
            assert_eq!(
                calls["end-turn"].result_hash,
                calls["end-turn-retry"].result_hash
            );
            assert_eq!(
                calls["after-self-wake"].status,
                crate::delegated::journal::ToolCallStatus::Completed
            );
            assert_eq!(calls["after-self-wake"].tool_name, "read");
            assert_eq!(fs::read(&assigned_file).unwrap(), verified_bytes);
            assert_eq!(fs::read(f.native.join("src/lib.rs")).unwrap(), original);
            assert_eq!(
                crate::web_work::deliver_next_manual_wake(&f.control, project).unwrap_err(),
                "WEB_TASK_MANUAL_WAKE_NO_PENDING_INTENT"
            );
            // The fixture's assistant response completes only this bound timer.
            wake.complete_timer(event_id).unwrap();
            assert_eq!(
                wake.delivery(event_id).unwrap().unwrap().receipt,
                Some(receipt)
            );
            let closed = crate::web_work::resolve(&f.control, resolution).unwrap();
            assert_eq!(closed["phase"], "CLOSED");
            assert_eq!(closed["taskCompletionVerified"], false);
        }
        drop(client);
        if let Some((stop, server)) = local_server {
            stop.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), server)
                .await
                .unwrap()
                .unwrap();
        }
        if let Some(child) = &mut child {
            assert!(
                tokio::time::timeout(Duration::from_secs(62), child.wait())
                    .await
                    .unwrap()
                    .unwrap()
                    .success()
            );
        }
        assert!(tokio::net::TcpStream::connect(addr).await.is_err());

        // Only the UUID-owned temporary root returned by the fixture is
        // eligible for cleanup. Drop the exact fixture key first.
        let fixture_root = assignment.parent().unwrap().canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(fixture_root.parent(), Some(temp.as_path()));
        let leaf = fixture_root.file_name().unwrap().to_str().unwrap();
        assert!(uuid::Uuid::parse_str(leaf.strip_prefix("project-task-").unwrap()).is_ok());
        drop((wake, f));
        fs::remove_dir_all(&fixture_root).unwrap();
        assert!(!fixture_root.exists());
        if child.is_some() {
            println!(
                "frozen backend: exact image, authenticated edit, sandbox verification, Git checkpoint, exact manual Wake admission and refusal, natural exit and fixture cleanup"
            );
        } else {
            println!(
                "development backend: authenticated edit, sandbox verification, Git checkpoint, guarded Wake fence, simulated receipt and response, authenticated same-task next read, listener and fixture cleanup"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn native_http_uses_scoped_grant_and_drains_after_disconnect() {
        native_http_fixture(false).await;
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn native_cached_http_uses_same_grant_and_journal() {
        native_http_fixture(true).await;
    }

    fn native_request(f: &Fixture, call_id: &str) -> Value {
        request(f, call_id, "native.codex.dispatch", json!({}))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manual_wake_slow_preparation_does_not_hold_shared_registration() {
        let (alpha, beta) = paired_fixtures(false);
        let wake = ready_manual_wake(&alpha);
        let control = alpha.control.clone();
        let pins = alpha.pins.clone();
        let key = alpha.key.clone();
        let (entered, waiting) = std::sync::mpsc::sync_channel(1);
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let owner = tokio::task::spawn_blocking(move || {
            crate::web_work::manual_wake_with_preparation(
                &control,
                pins,
                &key,
                "task-fixture",
                "prepared-alpha",
                json!({"sourceTurnId":"prepared-alpha-turn","priority":10,"ttlSeconds":300}),
                |root, project| {
                    // Stand in for a slow external Git identity probe. No
                    // registry/Wake/project lock may span this preparation.
                    entered.send(()).unwrap();
                    released
                        .recv_timeout(Duration::from_secs(30))
                        .expect("bounded preparation release");
                    crate::delegated::autonomy_projects::PreparedProjectScopeV1::resolve(
                        root, project,
                    )
                    .map_err(|_| "TEST_PREPARATION_FAILED".into())
                },
            )
        });
        tokio::task::spawn_blocking(move || waiting.recv_timeout(Duration::from_secs(10)))
            .await
            .unwrap()
            .unwrap();
        let registry =
            crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1::open_read_only(
                alpha.control.join(".catdesk/projects"),
            )
            .unwrap();
        registry.with_registration_lock(|| Ok(())).unwrap();
        let peer = call(
            &beta,
            manual_request(&beta, "prepared-peer", "prepared-peer-turn"),
        )
        .await;
        release.send(()).unwrap();
        let admitted = owner.await.unwrap().unwrap();
        assert_eq!(
            peer["result"]["structuredContent"]["projectId"], "beta",
            "{peer}"
        );
        assert_eq!(admitted["projectId"], "alpha");
        let a = admitted["eventId"].as_str().unwrap();
        let b = peer["result"]["structuredContent"]["eventId"]
            .as_str()
            .unwrap();
        assert_ne!(a, b);
        assert!(wake.delivery(a).unwrap().is_none());
        assert!(wake.delivery(b).unwrap().is_none());
        for f in [&alpha, &beta] {
            assert!(
                fs::read_to_string(f.native.join("src/lib.rs"))
                    .unwrap()
                    .contains("{ 1 }")
            );
        }
    }

    #[test]
    fn manual_wake_prepared_scope_refuses_changed_project_and_controller() {
        for changed_id in ["alpha", "catdesk"] {
            let f = fixture_with_manual_wake("alpha", true);
            let wake = ready_manual_wake(&f);
            let before = wake.events().unwrap();
            let registry_path = f.control.join(".catdesk/projects/projects.json");
            let original = fs::read(&registry_path).unwrap();
            let result = crate::web_work::manual_wake_with_preparation(
                &f.control,
                f.pins.clone(),
                &f.key,
                "task-fixture",
                "prepared-drift",
                json!({"sourceTurnId":"prepared-drift-turn","priority":10,"ttlSeconds":300}),
                |root, project| {
                    let prepared =
                        crate::delegated::autonomy_projects::PreparedProjectScopeV1::resolve(
                            root, project,
                        )
                        .unwrap();
                    let mut registry: AutonomousProjectRegistryV1 =
                        serde_json::from_slice(&original).unwrap();
                    registry
                        .projects
                        .iter_mut()
                        .find(|p| p.project_id == changed_id)
                        .unwrap()
                        .verification_profile = "web_only_v1".into();
                    fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
                    Ok(prepared)
                },
            );
            fs::write(&registry_path, original).unwrap();
            assert_eq!(result.unwrap_err(), "WEB_WORK_PROJECT_REGISTRATION_CHANGED");
            assert_eq!(wake.events().unwrap(), before);
            assert!(
                !f.control
                    .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal")
                    .exists()
            );
        }
    }
    fn native_proof(f: &Fixture) -> PathBuf {
        let p = resolve_registered_project_from_control_root(
            &f.control,
            f.pins["projectId"].as_str().unwrap(),
        )
        .unwrap();
        f.native.join(format!(
            "queue-proof-{}.txt",
            p.project.codex_thread_id.unwrap()
        ))
    }

    async fn native_http_fixture(cached: bool) {
        let (alpha, beta) = paired_fixtures(true);
        let mut app = crate::state::AppState::new_isolated_fixture(
            0,
            alpha.control.to_str().unwrap().into(),
            alpha.control.join("inert-config.toml"),
        )
        .unwrap();
        app.tool_mode = crate::state::ToolMode::SupervisorOnly;
        let state = Arc::new(tokio::sync::Mutex::new(app));
        let token = "native-http-fixture-bearer";
        crate::configure_native_mcp_auth(&state, Ok(token.into()))
            .await
            .unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let router = crate::native_mcp_router(state, None, "/central/mcp".into(), tx).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let url = format!("http://{address}/central/mcp");
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let transport = |mut body: Value| {
            if cached {
                body["params"] = json!({"name":"run_command","arguments":{"command":format!("CATDESK_WEB_TASK={}", body["params"]["arguments"])}});
            }
            body
        };
        let send = |body: Value| {
            client
                .post(&url)
                .bearer_auth(token)
                .json(&transport(body))
                .send()
        };
        assert_eq!(
            client
                .post(&url)
                .json(&transport(native_request(&alpha, "unauth")))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        let mut cross = native_request(&beta, "wrong-key");
        cross["params"]["arguments"]["executionKey"] = json!(alpha.key);
        let denied: Value = send(cross).await.unwrap().json().await.unwrap();
        assert_eq!(denied["error"]["message"], "WEB_TASK_GRANT_SCOPE_MISMATCH");
        let mut cross_status = request(&beta, "status-wrong-key", "native.codex.status", json!({}));
        cross_status["params"]["arguments"]["executionKey"] = json!(alpha.key);
        let denied: Value = send(cross_status).await.unwrap().json().await.unwrap();
        assert_eq!(denied["error"]["message"], "WEB_TASK_GRANT_SCOPE_MISMATCH");
        let mut override_call = native_request(&alpha, "override");
        override_call["params"]["arguments"]["arguments"] = json!({"workspace":beta.native});
        let denied: Value = send(override_call).await.unwrap().json().await.unwrap();
        assert_eq!(
            denied["error"]["message"],
            "WEB_TASK_NATIVE_ARGUMENTS_INVALID"
        );
        assert!(!native_proof(&alpha).exists() && !native_proof(&beta).exists());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let gate_key = alpha.control.join("native-alpha");
        TEST_GATES
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(gate_key.clone(), (entered.clone(), release.clone()));
        let body = transport(native_request(&alpha, "native-once"));
        let c = client.clone();
        let endpoint = url.clone();
        let awaiter = tokio::spawn(async move {
            c.post(endpoint)
                .bearer_auth(token)
                .json(&body)
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        });
        tokio::time::timeout(Duration::from_secs(15), entered.notified())
            .await
            .unwrap();
        assert!(crate::operator_facade::acquire_web_work_lease(&alpha.control, "alpha").is_err());
        let status: Value = send(request(
            &alpha,
            "status-during-dispatch",
            "native.codex.status",
            json!({}),
        ))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(
            status["result"]["structuredContent"]["job"]["state"], "READY",
            "{status}"
        );
        assert_eq!(
            status["result"]["structuredContent"]["journalChanged"],
            false
        );
        // Alpha's project lease/journal is held, but shared registry/Wake locks
        // and beta's normal native queue remain available through this server.
        let beta_result: Value = send(native_request(&beta, "native-once"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(beta_result.get("error").is_none(), "{beta_result}");
        assert_eq!(
            beta_result["result"]["structuredContent"]["job"]["state"],
            "NATIVE_EXITED_ZERO"
        );
        assert_eq!(
            beta_result["result"]["structuredContent"]["receipt"]["treeExitProven"],
            true
        );
        assert!(beta_result["result"]["structuredContent"]["receipt"]["actualProfile"].is_null());
        let grant_path = beta
            .control
            .join(".catdesk/project-mcp-tasks/beta/task-fixture/execution-grant.json");
        let frozen = fs::read(&grant_path).unwrap();
        crate::web_work::execution_permission_with_capabilities(
            &beta.control,
            beta.pins.clone(),
            false,
            false,
            Some("native-fixture"),
        )
        .unwrap();
        assert_eq!(fs::read(grant_path).unwrap(), frozen);
        let scope = resolve_registered_project_from_control_root(&beta.control, "beta").unwrap();
        let record = crate::web_work_state::WorkLedger::open(&beta.control, "beta")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let handoff =
            crate::project_mcp_task::granted_handoff_context(&scope, &record, &record.target)
                .unwrap()
                .unwrap();
        assert_eq!(
            handoff["nativeQueue"]["snapshot"]["job"]["state"],
            "NATIVE_EXITED_ZERO"
        );
        assert_eq!(handoff["nativeQueue"]["tool"], "native.codex.dispatch");
        assert_eq!(handoff["nativeQueue"]["selection"]["maximumSeconds"], 60);
        assert!(!beta_result.to_string().contains(&beta.key));
        assert_eq!(
            fs::read_to_string(native_proof(&beta)).unwrap(),
            "native fixture task"
        );
        assert!(!native_proof(&alpha).exists());
        awaiter.abort();
        assert!(awaiter.await.unwrap_err().is_cancelled());
        release.notify_one();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(lease) =
                crate::operator_facade::acquire_web_work_lease(&alpha.control, "alpha")
            {
                drop(lease);
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "owned native turn did not drain"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        TEST_GATES.get().unwrap().lock().unwrap().remove(&gate_key);
        assert_eq!(
            fs::read_to_string(native_proof(&alpha)).unwrap(),
            "native fixture task"
        );
        let recovered: Value = send(request(
            &alpha,
            "status-after-lost-reply",
            "native.codex.status",
            json!({}),
        ))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(
            recovered["result"]["structuredContent"]["job"]["state"], "NATIVE_EXITED_ZERO",
            "{recovered}"
        );
        assert_eq!(
            recovered["result"]["structuredContent"]["receipt"]["treeExitProven"],
            true
        );
        assert_eq!(
            recovered["result"]["structuredContent"]["taskCompletionVerified"],
            false
        );
        let duplicate: Value = send(native_request(&alpha, "native-once"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            duplicate["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        for f in [&alpha, &beta] {
            assert!(
                fs::read_to_string(f.task.join("src/lib.rs"))
                    .unwrap()
                    .contains("{ 1 }")
            );
            let journal = crate::delegated::journal::DelegatedJournal::open(
                f.control
                    .join(".catdesk/project-mcp-tasks")
                    .join(f.pins["projectId"].as_str().unwrap())
                    .join("task-fixture/journal"),
            )
            .unwrap();
            let calls = journal
                .load_tool_calls(&crate::delegated::contracts::RunId::new("task-fixture").unwrap())
                .unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(
                calls["native-once"].status,
                crate::delegated::journal::ToolCallStatus::Completed
            );
        }
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .unwrap()
            .unwrap();
        let closed_by = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::net::TcpStream::connect(address).await.is_err() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < closed_by,
                "test endpoint still accepts connections after bounded shutdown"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn native_capability_is_explicit_immutable_and_revocable() {
        let legacy = fixture("alpha");
        let denied = call(&legacy, native_request(&legacy, "not-granted")).await;
        assert_eq!(denied["error"]["message"], "WEB_TASK_NATIVE_NOT_GRANTED");
        let f = native_fixture("alpha");
        let path = f
            .control
            .join(".catdesk/project-mcp-tasks/alpha/task-fixture/execution-grant.json");
        let original = fs::read(&path).unwrap();
        assert!(
            crate::web_work::execution_permission(&f.control, f.pins.clone(), false).is_err(),
            "immutable grant cannot silently lose capabilities"
        );
        let mut altered: Value = serde_json::from_slice(&original).unwrap();
        altered["nativeQueue"]["requestSha256"] = json!("0".repeat(64));
        fs::write(&path, serde_json::to_vec(&altered).unwrap()).unwrap();
        let denied = call(&f, native_request(&f, "changed-payload")).await;
        assert_eq!(
            denied["error"]["message"], "WEB_TASK_NATIVE_REQUEST_CHANGED",
            "{denied}"
        );
        assert!(!native_proof(&f).exists());
        fs::write(&path, &original).unwrap();
        crate::web_work::execution_permission(&f.control, f.pins.clone(), true).unwrap();
        let denied = call(&f, native_request(&f, "revoked")).await;
        assert_eq!(denied["error"]["message"], "WEB_TASK_GRANT_REVOKED");
        let denied = call(
            &f,
            request(&f, "revoked-status", "native.codex.status", json!({})),
        )
        .await;
        assert_eq!(denied["error"]["message"], "WEB_TASK_GRANT_REVOKED");
        assert!(!native_proof(&f).exists());
    }

    #[tokio::test]
    async fn native_preflight_refusal_is_not_an_unknown_task_effect() {
        let f = native_fixture("alpha");
        let image = f._native_queue.as_ref().unwrap().image();
        let retained = image.with_extension("retained-exe");
        fs::rename(&image, &retained).unwrap();
        let refused = call(&f, native_request(&f, "unavailable-cli")).await;
        fs::rename(retained, &image).unwrap();
        assert_eq!(
            refused["result"]["structuredContent"]["status"], "prelaunch_refused",
            "{refused}"
        );
        assert_eq!(
            refused["result"]["structuredContent"]["nativeTurnStarted"],
            false
        );
        assert_eq!(refused["result"]["isError"], true);
        assert!(!native_proof(&f).exists());
        let restored = call(&f, native_request(&f, "restored-cli")).await;
        assert_eq!(
            restored["result"]["structuredContent"]["job"]["state"], "NATIVE_EXITED_ZERO",
            "{restored}"
        );
        let journal = crate::delegated::journal::DelegatedJournal::open(
            f.control
                .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal"),
        )
        .unwrap();
        let calls = journal
            .load_tool_calls(&crate::delegated::contracts::RunId::new("task-fixture").unwrap())
            .unwrap();
        assert_eq!(
            calls["unavailable-cli"].status,
            crate::delegated::journal::ToolCallStatus::Failed
        );
        assert_eq!(
            calls["restored-cli"].status,
            crate::delegated::journal::ToolCallStatus::Completed
        );
    }

    #[tokio::test]
    async fn ambiguous_native_admission_keeps_existing_journal_fence() {
        let (alpha, beta) = paired_fixtures(true);
        let admission = crate::web_work::prepare_native_dispatch(
            &alpha.control,
            alpha.pins.clone(),
            &alpha.key,
            "task-fixture",
            "interrupted-native",
        )
        .unwrap();
        admission
            .finish(&Err("CONTROLLED_UNKNOWN_NATIVE_OUTCOME".into()))
            .unwrap();
        let replay = call(&alpha, native_request(&alpha, "interrupted-native")).await;
        assert_eq!(
            replay["error"]["message"],
            "PROJECT_TASK_CALL_RECORDED_NO_REPLAY"
        );
        let changed_id = call(&alpha, native_request(&alpha, "new-native-id")).await;
        assert_eq!(
            changed_id["error"]["message"],
            "PROJECT_TASK_RECOVERY_REQUIRED"
        );
        let peer = call(&beta, native_request(&beta, "peer-native")).await;
        assert_eq!(
            peer["result"]["structuredContent"]["job"]["state"], "NATIVE_EXITED_ZERO",
            "{peer}"
        );
        assert!(!native_proof(&alpha).exists());
    }

    fn native_rollover(f: &mut Fixture) {
        let event = f.pins["sourceEventId"].as_str().unwrap();
        let root = crate::wake_protocol_client::wake_root_for_workspace(&f.control).unwrap();
        let wake = catdesk_wake::store::Store::open(&root).unwrap();
        let (_, current) = crate::operator_facade::native_continuation_fixture(
            &f.control,
            &wake,
            f.pins["projectId"].as_str().unwrap(),
            event,
        );
        f.pins["expectedTargetGeneration"] = json!(current.generation);
        f.pins["expectedTargetSha256"] = json!(current.digest);
    }

    #[tokio::test]
    async fn native_rollover_keeps_grant_receipts_and_completed_call_fence() {
        let (mut alpha, beta) = paired_fixtures(true);
        let path = alpha
            .control
            .join(".catdesk/project-mcp-tasks/alpha/task-fixture/execution-grant.json");
        let original = fs::read(&path).unwrap();
        native_rollover(&mut alpha);
        crate::web_work::execution_permission_with_capabilities(
            &alpha.control,
            alpha.pins.clone(),
            false,
            false,
            Some("native-fixture"),
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        let scope = resolve_registered_project_from_control_root(&alpha.control, "alpha").unwrap();
        let record = crate::web_work_state::WorkLedger::open(&alpha.control, "alpha")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let current = catdesk_wake::protocol::Target::new(
            alpha.pins["expectedTargetGeneration"].as_u64().unwrap(),
            scope.project.chatgpt_target_url.clone().unwrap(),
        )
        .unwrap();
        let handed = crate::project_mcp_task::granted_handoff_context(&scope, &record, &current)
            .unwrap()
            .unwrap();
        assert_eq!(
            handed["nativeQueue"]["snapshot"]["job"]["taskId"],
            "native-fixture"
        );
        let result = call(&alpha, native_request(&alpha, "rollover-native-once")).await;
        let result = &result["result"]["structuredContent"];
        assert_eq!(result["job"]["state"], "NATIVE_EXITED_ZERO", "{result}");
        let child = result["job"]["taskId"].as_str().unwrap().to_string();
        assert!(child.starts_with("native-next-"));
        assert_eq!(
            fs::read(&path).unwrap(),
            original,
            "rollover cannot rewrite the grant"
        );
        let status = call(
            &alpha,
            request(&alpha, "rollover-status", "native.codex.status", json!({})),
        )
        .await;
        let status = &status["result"]["structuredContent"];
        assert_eq!(status["originTaskId"], "native-fixture");
        assert_eq!(status["lineage"], json!(["native-fixture", child]));
        assert_eq!(status["receipt"]["treeExitProven"], true);
        assert_eq!(
            fs::read_to_string(native_proof(&alpha)).unwrap(),
            "native fixture task"
        );
        let replay = call(&alpha, native_request(&alpha, "rollover-native-once")).await;
        assert_eq!(
            replay["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        // A second rollover must preserve the already completed native job,
        // not create another task under a newer conversation identity.
        native_rollover(&mut alpha);
        let again = call(&alpha, native_request(&alpha, "rollover-native-once")).await;
        assert_eq!(
            again["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        let separate = call(&alpha, native_request(&alpha, "already-ended-new-call")).await;
        assert_eq!(
            separate["result"]["structuredContent"]["status"], "no_ready_tasks",
            "{separate}"
        );
        let status = call(
            &alpha,
            request(&alpha, "second-status", "native.codex.status", json!({})),
        )
        .await;
        assert_eq!(
            status["result"]["structuredContent"]["lineage"],
            json!(["native-fixture", child])
        );
        let peer = call(&beta, native_request(&beta, "independent-peer")).await;
        assert_eq!(
            peer["result"]["structuredContent"]["job"]["state"], "NATIVE_EXITED_ZERO",
            "{peer}"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[tokio::test]
    async fn native_rollover_cannot_route_around_ambiguous_call_or_block_peer() {
        let (mut alpha, beta) = paired_fixtures(true);
        let admission = crate::web_work::prepare_native_dispatch(
            &alpha.control,
            alpha.pins.clone(),
            &alpha.key,
            "task-fixture",
            "unknown-before-rollover",
        )
        .unwrap();
        admission
            .finish(&Err("CONTROLLED_UNKNOWN_NATIVE_OUTCOME".into()))
            .unwrap();
        native_rollover(&mut alpha);
        let blocked = call(&alpha, native_request(&alpha, "new-id-after-rollover")).await;
        assert_eq!(
            blocked["error"]["message"],
            "PROJECT_TASK_RECOVERY_REQUIRED"
        );
        let status = call(
            &alpha,
            request(&alpha, "unknown-status", "native.codex.status", json!({})),
        )
        .await;
        assert_eq!(
            status["result"]["structuredContent"]["lineage"],
            json!(["native-fixture"])
        );
        assert!(!native_proof(&alpha).exists());
        let peer = call(&beta, native_request(&beta, "peer-after-rollover-fence")).await;
        assert_eq!(
            peer["result"]["structuredContent"]["job"]["state"], "NATIVE_EXITED_ZERO",
            "{peer}"
        );
    }

    #[test]
    fn cached_envelope_is_closed_bounded_and_never_accepts_shell_options() {
        let wrap = |command: &str| {
            serde_json::from_value::<JsonRpcRequest>(json!({
                "jsonrpc":"2.0","id":7,"method":"tools/call",
                "params":{"name":"run_command","arguments":{"command":command}}
            }))
            .unwrap()
        };
        for command in [
            "CATDESK_WEB_TASK",
            "CATDESK_WEB_TASK ={}",
            "catdesk_web_task={}",
            "CATDESK_WEB_TASK_EXTRA={}",
            "CATDESK_WEB_TASK=[]",
            "CATDESK_WEB_TASK={}; echo should-not-run",
            "CATDESK_WEB_TASK={invalid",
        ] {
            let request = wrap(command);
            assert!(is_cached_call(&request));
            assert!(cached_request(&request).is_err());
        }
        assert!(!is_cached_call(&wrap("git status --short")));
        let request = wrap("  CATDESK_WEB_TASK={\"callId\":\"stable-call\"}");
        let adapted = cached_request(&request).unwrap();
        assert_eq!(adapted.id, Some(json!(7)));
        assert_eq!(adapted.params["name"], TOOL);
        assert_eq!(adapted.params["arguments"]["callId"], "stable-call");
        for (name, value) in [
            ("cwd", json!(".")),
            ("timeout", json!(1000)),
            ("dry_run", json!(true)),
            ("include_full_output", json!(true)),
            ("allow_without_plan", json!("true")),
            ("other", Value::Null),
        ] {
            let mut invalid = wrap("CATDESK_WEB_TASK={}");
            invalid.params["arguments"][name] = value;
            assert!(cached_request(&invalid).is_err());
        }
        let mut accepted = wrap("CATDESK_WEB_TASK={}");
        accepted.params["arguments"]["dry_run"] = json!(false);
        accepted.params["arguments"]["include_full_output"] = json!(false);
        accepted.params["arguments"]["allow_without_plan"] = json!(true);
        assert!(cached_request(&accepted).is_ok());
        let too_large = format!(
            "CATDESK_WEB_TASK={{\"text\":\"{}\"}}",
            "x".repeat(2 * 1024 * 1024)
        );
        assert!(cached_request(&wrap(&too_large)).is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cached_error_receipt_explains_invariant_without_exporting_values() {
        let request: JsonRpcRequest = serde_json::from_value(json!({"jsonrpc":"2.0","id":7,
            "method":"tools/call","params":{"name":"run_command","arguments":{
                "command":"CATDESK_WEB_TASK={\"executionKey\":\"PRIVATE_SENTINEL\"}",
                "PRIVATE_FIELD_NAME":"PRIVATE_VALUE"}}}))
        .unwrap();
        let response =
            serde_json::to_value(handle_cached("not-an-admitted-workspace", &request).await)
                .unwrap();
        assert_eq!(
            response["error"]["message"],
            "WEB_TASK_CACHED_ARGUMENTS_INVALID"
        );
        assert_eq!(
            response["error"]["data"]["invariant"],
            "UNSUPPORTED_OUTER_FIELD"
        );
        assert_eq!(response["error"]["data"]["nativeExecutionAttempted"], false);
        assert_eq!(response["error"]["data"]["retryAuthorized"], false);
        assert!(!response.to_string().contains("PRIVATE_"));
        assert!(!response.to_string().contains("executionKey"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cached_empty_probe_is_refused_by_inner_schema_before_task_or_native_admission() {
        let f = fixture("alpha");
        let request:JsonRpcRequest=serde_json::from_value(json!({"jsonrpc":"2.0","id":"no-effect-probe",
            "method":"tools/call","params":{"name":"run_command","arguments":{"command":"CATDESK_WEB_TASK={}"}}})).unwrap();
        assert!(is_cached_call(&request));
        let adapted = cached_request(&request).unwrap();
        assert_eq!(adapted.params["arguments"], json!({}));
        let journal = f
            .control
            .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal");
        assert!(!journal.exists());
        let response = handle_cached(f.control.to_str().unwrap(), &request).await;
        let error = response.error.unwrap();
        assert_eq!(error.code, -32602);
        assert_eq!(error.message, "WEB_TASK_ARGUMENTS_INVALID");
        assert!(!journal.exists());
    }

    #[test]
    fn cached_canonical_envelope_checks_representation_without_native_admission() {
        let call = json!({"projectId":"alpha","requestId":"fixture","sourceEventId":"source",
            "assignmentSha256":"a".repeat(64),"expectedTargetGeneration":1,"expectedTargetSha256":"b".repeat(64),
            "taskId":"task","callId":"fixed-call","tool":"read","executionKey":"c".repeat(64),
            "arguments":{"text":"quotes \" and newline\n"}});
        let hash = canonical_cached_envelope_sha256(&call).unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, canonical_cached_envelope_sha256(&call).unwrap());
        assert!(!hash.contains("PRIVATE_"));
        for invalid in [
            Value::Null,
            json!([]),
            json!("shell"),
            json!({}),
            call.as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| *key != "projectId")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<serde_json::Map<_, _>>()
                .into(),
        ] {
            assert!(canonical_cached_envelope_sha256(&invalid).is_err());
        }
        let mut changed = call;
        changed["arguments"] = json!({"different":true});
        assert_ne!(hash, canonical_cached_envelope_sha256(&changed).unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cached_http_retains_auth_mode_plan_and_non_disclosure_guards() {
        use crate::state::{Mode, ToolMode};
        let f = fixture("alpha");
        let mut app = crate::state::AppState::new_isolated_fixture(
            0,
            f.control.to_str().unwrap().into(),
            f.control.join("inert-config.toml"),
        )
        .unwrap();
        app.mode = Mode::Computer;
        app.tool_mode = ToolMode::SupervisorOnly;
        let state = Arc::new(tokio::sync::Mutex::new(app));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let token = "c92-cached-http-fixture-token-000000";
        let router = crate::server::router(
            state.clone(),
            None,
            "/cached/mcp".into(),
            tx,
            Some(token.into()),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/cached/mcp", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let direct = request(&f, "guarded-read", "read", json!({"path":"src/lib.rs"}));
        let cached = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"run_command","arguments":{"command":format!("CATDESK_WEB_TASK={}", direct["params"]["arguments"])}}});
        let rpc = |body: Value| client.post(&url).bearer_auth(token).json(&body).send();
        let unauthorized = client.post(&url).json(&cached).send().await.unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        let journal = f
            .control
            .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal");
        for (name, value) in [
            ("cwd", json!(f.native)),
            ("timeout", json!(1)),
            ("dry_run", json!(true)),
            ("include_full_output", json!(true)),
        ] {
            let mut invalid = cached.clone();
            invalid["params"]["arguments"][name] = value;
            let denied: Value = rpc(invalid).await.unwrap().json().await.unwrap();
            assert_eq!(
                denied["error"]["message"],
                "WEB_TASK_CACHED_ARGUMENTS_INVALID"
            );
            assert_eq!(
                denied["error"]["data"]["phase"],
                "CACHED_ENVELOPE_VALIDATION"
            );
            assert_eq!(denied["error"]["data"]["nativeExecutionAttempted"], false);
            assert_eq!(denied["error"]["data"]["retryAuthorized"], false);
            let invariant = match name {
                "cwd" => "CWD_OVERRIDE_FORBIDDEN",
                "timeout" => "TIMEOUT_OVERRIDE_FORBIDDEN",
                "dry_run" => "DRY_RUN_NOT_FALSE",
                _ => "FULL_OUTPUT_NOT_FALSE",
            };
            assert_eq!(denied["error"]["data"]["invariant"], invariant);
            assert!(!denied.to_string().contains(&f.key));
            assert!(!journal.exists());
        }
        for (mode, tools) in [
            (Mode::Computer, ToolMode::ReadOnly),
            (Mode::Browser, ToolMode::MultiTools),
            (Mode::Both, ToolMode::ReadOnly),
        ] {
            {
                let mut app = state.lock().await;
                app.mode = mode;
                app.tool_mode = tools;
            }
            let catalog: Value = rpc(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert!(
                !catalog["result"]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["name"] == TOOL)
            );
            for body in [&direct, &cached] {
                let denied: Value = rpc(body.clone()).await.unwrap().json().await.unwrap();
                assert_eq!(denied["error"]["message"], "WEB_TASK_CENTRAL_MODE_REQUIRED");
                assert!(!denied.to_string().contains(&f.key));
                assert!(!journal.exists());
            }
        }
        {
            let mut app = state.lock().await;
            app.mode = Mode::Computer;
            app.tool_mode = ToolMode::SupervisorOnly;
        }
        fs::write(
            f.control.join(".catdesk/current_plan.md"),
            "# Current Plan\n\nplan_required: true\n\n## Plan\n\n",
        )
        .unwrap();
        let policy = crate::planning::policy_status(f.control.to_str().unwrap()).unwrap();
        assert!(policy.plan_required && !policy.has_plan);
        let denied: Value = rpc(cached.clone()).await.unwrap().json().await.unwrap();
        assert_eq!(denied["result"]["isError"], true);
        assert!(denied.to_string().contains("PLAN_REQUIRED"));
        assert!(!denied.to_string().contains(&f.key));
        assert!(!journal.exists());
        let mut allowed = cached;
        allowed["params"]["arguments"]["allow_without_plan"] = json!(true);
        let result: Value = rpc(allowed).await.unwrap().json().await.unwrap();
        assert!(result.get("error").is_none(), "{result}");
        assert_eq!(
            result["result"]["_meta"]["catdeskTaskLifecycle"]["status"],
            "CLOSED"
        );
        assert_eq!(
            result["result"]["_meta"]["catdeskTaskTransport"]["shellExecuted"],
            false
        );
        assert!(!result.to_string().contains(&f.key));
        // The reserved task adapter grants no ordinary shell command permission.
        let ordinary: Value = rpc(json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"run_command","arguments":{"command":"echo should-not-run","allow_without_plan":true}}})).await.unwrap().json().await.unwrap();
        assert_eq!(ordinary["result"]["isError"], true);
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        drop(crate::delegated::autonomy_runtime::acquire_project_writer_lock(&f.task).unwrap());
        drop(crate::delegated::autonomy_runtime::acquire_project_writer_lock(&f.native).unwrap());
    }

    fn paired_fixtures(native: bool) -> (Fixture, Fixture) {
        let alpha = if native {
            native_fixture("alpha")
        } else {
            fixture_with_manual_wake("alpha", true)
        };
        let mut beta = if native {
            native_fixture("beta")
        } else {
            fixture_with_manual_wake("beta", true)
        };
        let path = alpha.control.join(".catdesk/projects/projects.json");
        let mut registry: AutonomousProjectRegistryV1 =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let beta_scope =
            resolve_registered_project_from_control_root(&beta.control, "beta").unwrap();
        registry.projects.push(beta_scope.project.clone());
        fs::write(&path, serde_json::to_vec(&registry).unwrap()).unwrap();
        // Prepare the second project's existing exact source in this one
        // control root. No native checkout/worktree is shared between projects.
        let source_root = beta.control.clone();
        let source_wake = Store::open(
            &crate::wake_protocol_client::wake_root_for_workspace(&source_root).unwrap(),
        )
        .unwrap();
        let source_record = crate::web_work_state::WorkLedger::open(&source_root, "beta")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let event = source_wake
            .delivery(&source_record.event_id)
            .unwrap()
            .unwrap()
            .event;
        let wake = Store::open(
            &crate::wake_protocol_client::wake_root_for_workspace(&alpha.control).unwrap(),
        )
        .unwrap();
        let target = wake
            .set_target("beta", 0, &source_record.target.url)
            .unwrap();
        crate::web_work_state::WorkLedger::open(&alpha.control, "beta")
            .unwrap()
            .prepare(
                "request-a",
                &event.event_id,
                &source_record.message_sha256,
                &target,
            )
            .unwrap();
        wake.publish(&event).unwrap();
        let assignment =
            source_root.join(".catdesk/project-mcp-tasks/beta/task-fixture/assignment.json");
        crate::web_work::associate(&alpha.control, beta.pins.clone(), &assignment).unwrap();
        sent(&wake, &event);
        beta.control = alpha.control.clone();
        if native {
            beta._native_queue = Some(crate::operator_codex_cli::native_queue_fixture(
                &beta.control,
                "beta",
            ));
        }
        let expected_key = fresh_fixture_key_path(&beta.control, "beta");
        let grant = crate::web_work::execution_permission_with_capabilities(
            &beta.control,
            beta.pins.clone(),
            false,
            !native,
            native.then_some("native-fixture"),
        )
        .unwrap();
        beta.keys
            .push(OwnedFixtureKey::retain(expected_key, &grant));
        beta.key =
            serde_json::from_slice(&fs::read(grant["executionKeyFile"].as_str().unwrap()).unwrap())
                .unwrap();
        (alpha, beta)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn serial_web_lane_preserves_owner_through_wake_and_allows_safe_yield() {
        use crate::web_work_state::{WorkLedger, execution_lane};
        let (alpha, beta) = paired_fixtures(false);
        let wake = ready_manual_wake(&alpha);
        let root = &alpha.control;
        assert_eq!(
            execution_lane::status(root, "alpha")["status"],
            "NOT_ENABLED"
        );
        let transition = |project: &str, select| {
            use execution_lane::Transition;
            crate::web_work::transition_execution_lane(
                root,
                project,
                "request-a",
                if select {
                    Transition::Select
                } else {
                    Transition::Yield
                },
            )
        };
        let select_alpha = [
            "operator",
            "web",
            "work",
            "lane-select",
            "--project-id",
            "alpha",
            "--request-id",
            "request-a",
        ]
        .map(str::to_string);
        let old_journal = root.join(".catdesk/project-mcp-tasks/beta/task-fixture/journal");
        fs::create_dir(&old_journal).unwrap();
        assert_eq!(
            transition("alpha", true).unwrap_err(),
            "WEB_WORK_LANE_EXISTING_TASK_ACTIVITY"
        );
        assert_eq!(
            execution_lane::status(root, "alpha")["status"],
            "NOT_ENABLED"
        );
        fs::remove_dir(&old_journal).unwrap();
        let action = crate::operator_facade::parse_operator_action(select_alpha)
            .unwrap()
            .unwrap();
        let selected = crate::operator_facade::execute_operator_action(action, root)
            .await
            .unwrap();
        assert_eq!(selected["status"], "ACTIVE");
        assert_eq!(execution_lane::status(root, "beta")["status"], "QUEUED");
        assert_eq!(
            transition("beta", true).unwrap_err(),
            "WEB_WORK_LANE_ALREADY_OWNED"
        );

        let preview = |f: &Fixture| {
            request(
                f,
                "serial-preview",
                "patch.preview",
                json!({"patchId":"serial-patch","operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 52 }"}]}),
            )
        };
        let lane_file = root.join(".catdesk/web-work/.execution-lane.json");
        let before_wait = fs::read(&lane_file).unwrap();
        let mut wrong_task = preview(&beta);
        wrong_task["params"]["arguments"]["taskId"] = json!("another-task");
        assert_eq!(
            call(&beta, wrong_task).await["error"]["message"],
            "WEB_TASK_GRANT_SCOPE_MISMATCH"
        );
        assert_eq!(fs::read(&lane_file).unwrap(), before_wait);
        assert_eq!(
            call(&beta, preview(&beta)).await["error"]["message"],
            "WEB_WORK_EXECUTION_QUEUED"
        );
        let waiting = fs::read(&lane_file).unwrap();
        assert_ne!(waiting, before_wait);
        assert_eq!(
            call(&beta, preview(&beta)).await["error"]["message"],
            "WEB_WORK_EXECUTION_QUEUED"
        );
        assert_eq!(fs::read(&lane_file).unwrap(), waiting);
        assert!(
            !root
                .join(".catdesk/project-mcp-tasks/beta/task-fixture/journal")
                .exists()
        );
        let alpha_preview = call(&alpha, preview(&alpha)).await;
        assert!(alpha_preview.get("error").is_none(), "{alpha_preview}");
        let beta_read = call(
            &beta,
            request(&beta, "queued-read", "read", json!({"path":"src/lib.rs"})),
        )
        .await;
        assert!(beta_read.get("error").is_none(), "{beta_read}");
        let queued = WorkLedger::open_read_only(root, "beta")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        assert_eq!(queued.phase, "OPEN");
        assert_eq!(
            call(&beta, manual_request(&beta, "not-owner", "beta-turn")).await["error"]["message"],
            "WEB_WORK_EXECUTION_QUEUED"
        );
        let alpha_wake = call(&alpha, manual_request(&alpha, "alpha-wake", "alpha-turn")).await;
        assert!(alpha_wake.get("error").is_none(), "{alpha_wake}");
        let event_id = alpha_wake["result"]["structuredContent"]["eventId"]
            .as_str()
            .unwrap();
        assert_eq!(
            transition("alpha", false).unwrap_err(),
            "WEB_WORK_CONTINUATION_PENDING"
        );
        let event = wake
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_id == event_id)
            .unwrap();
        // Simulate the exact browser receipt without opening a browser or
        // sending any message. A delivered USER alone cannot release ownership.
        sent(&wake, &event);
        assert_eq!(
            transition("alpha", false).unwrap_err(),
            "WEB_WORK_CONTINUATION_PENDING"
        );
        assert_eq!(execution_lane::status(root, "alpha")["status"], "ACTIVE");
        wake.complete_timer(event_id).unwrap();
        assert_eq!(transition("alpha", false).unwrap()["status"], "YIELDED");
        assert_eq!(
            execution_lane::status(root, "beta")["status"],
            "NEXT_IN_QUEUE"
        );
        let after_yield = fs::read(&lane_file).unwrap();
        assert_eq!(
            call(&alpha, preview(&alpha)).await["error"]["message"],
            "PROJECT_TASK_CALL_RECORDED_NO_REPLAY"
        );
        assert_eq!(fs::read(&lane_file).unwrap(), after_yield);
        let alpha_apply = request(
            &alpha,
            "released-owner-next",
            "patch.apply",
            json!({"patchId":"serial-patch"}),
        );
        assert_eq!(
            call(&alpha, alpha_apply.clone()).await["error"]["message"],
            "WEB_WORK_EXECUTION_QUEUED"
        );
        let beta_preview = call(&beta, preview(&beta)).await;
        assert!(beta_preview.get("error").is_none(), "{beta_preview}");
        assert_eq!(execution_lane::status(root, "beta")["status"], "ACTIVE");
        let beta_apply = call(
            &beta,
            request(
                &beta,
                "serial-apply",
                "patch.apply",
                json!({"patchId":"serial-patch"}),
            ),
        )
        .await;
        assert!(beta_apply.get("error").is_none(), "{beta_apply}");
        assert_eq!(
            call(&alpha, alpha_apply.clone()).await["error"]["message"],
            "WEB_WORK_EXECUTION_QUEUED"
        );
        assert!(
            fs::read_to_string(beta.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 52 }")
        );
        assert!(
            fs::read_to_string(alpha.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
        let reopened = WorkLedger::open_read_only(root, "beta")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        assert_eq!(reopened, queued);
        assert_eq!(wake.delivery(event_id).unwrap().unwrap().phase, Phase::Sent);
        wake.complete_timer(&queued.event_id).unwrap();
        let resolved = crate::web_work::resolve(
            root,
            json!({
                "projectId":"beta","requestId":queued.request_id,
                "expectedTargetGeneration":queued.target.generation,
                "expectedTargetSha256":queued.target.digest,
                "outcome":"completed","summary":"Fixture Web work settled",
            }),
        )
        .unwrap();
        assert_eq!(resolved["phase"], "CLOSED");
        assert_eq!(
            execution_lane::status(root, "alpha")["status"],
            "NEXT_IN_QUEUE"
        );
        let alpha_after_close = call(&alpha, alpha_apply).await;
        assert!(
            alpha_after_close.get("error").is_none(),
            "{alpha_after_close}"
        );
        assert_eq!(execution_lane::status(root, "alpha")["status"], "ACTIVE");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn serial_web_lane_pause_and_revoked_waiters_preserve_original_tasks() {
        use crate::web_work_state::{WorkLedger, execution_lane};
        let (alpha, beta) = paired_fixtures(false);
        let wake = ready_manual_wake(&alpha);
        let root = &alpha.control;
        let change = |project: &'static str, verb: &'static str| async move {
            let args = [
                "operator",
                "web",
                "work",
                verb,
                "--project-id",
                project,
                "--request-id",
                "request-a",
            ]
            .map(str::to_string);
            let action = crate::operator_facade::parse_operator_action(args)
                .unwrap()
                .unwrap();
            crate::operator_facade::execute_operator_action(action, root).await
        };
        let preview = |f: &Fixture, id| {
            request(
                f,
                id,
                "patch.preview",
                json!({"patchId":id,"operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 53 }"}]}),
            )
        };
        assert_eq!(
            change("alpha", "lane-select").await.unwrap()["status"],
            "ACTIVE"
        );
        assert_eq!(
            call(&beta, preview(&beta, "beta-waiting")).await["error"]["message"],
            "WEB_WORK_EXECUTION_QUEUED"
        );
        let wake_queued = call(
            &alpha,
            manual_request(&alpha, "alpha-pause-wake", "alpha-turn"),
        )
        .await;
        assert!(wake_queued.get("error").is_none(), "{wake_queued}");
        let event_id = wake_queued["result"]["structuredContent"]["eventId"]
            .as_str()
            .unwrap();
        let event = wake
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_id == event_id)
            .unwrap();
        let host = wake.host_lock().unwrap();
        wake.claim(&event, &host).unwrap();
        drop(host);
        assert_eq!(
            change("alpha", "lane-pause").await.unwrap_err(),
            "WEB_WORK_CONTINUATION_PENDING"
        );
        assert_eq!(execution_lane::status(root, "alpha")["status"], "ACTIVE");
        // Only fixture receipt/response data. Production and the browser are outside the test.
        sent(&wake, &event);
        wake.complete_timer(event_id).unwrap();
        assert_eq!(
            change("alpha", "lane-pause").await.unwrap()["status"],
            "PAUSED"
        );
        assert_eq!(execution_lane::status(root, "alpha")["status"], "PAUSED");
        assert_eq!(
            call(&alpha, preview(&alpha, "alpha-next")).await["error"]["message"],
            "WEB_WORK_LANE_PAUSED"
        );
        let read = call(
            &alpha,
            request(&alpha, "paused-read", "read", json!({"path":"src/lib.rs"})),
        )
        .await;
        assert!(read.get("error").is_none(), "{read}");
        let selected_beta = call(&beta, preview(&beta, "beta-waiting")).await;
        assert!(selected_beta.get("error").is_none(), "{selected_beta}");
        assert_eq!(execution_lane::status(root, "beta")["status"], "ACTIVE");
        assert_eq!(
            change("alpha", "lane-resume").await.unwrap()["status"],
            "RESUMED"
        );
        assert_eq!(
            call(&alpha, preview(&alpha, "alpha-next")).await["error"]["message"],
            "WEB_WORK_EXECUTION_QUEUED"
        );
        let ledger = WorkLedger::open_read_only(root, "alpha").unwrap();
        let before = ledger.record("request-a").unwrap().unwrap();
        let grant_path =
            root.join(".catdesk/project-mcp-tasks/alpha/task-fixture/execution-grant.json");
        let grant = fs::read(&grant_path).unwrap();
        let revocation =
            crate::web_work::execution_permission(root, alpha.pins.clone(), true).unwrap();
        assert_eq!(revocation["status"], "EXECUTION_REVOKED");
        assert_eq!(
            execution_lane::status(root, "alpha")["status"],
            "EXECUTION_REVOKED"
        );
        assert_eq!(execution_lane::status(root, "beta")["status"], "ACTIVE");
        assert_eq!(
            change("beta", "lane-yield").await.unwrap()["status"],
            "YIELDED"
        );
        assert_eq!(
            execution_lane::status(root, "beta")["status"],
            "AVAILABLE_FOR_SELECTION"
        );
        let continued_beta = call(
            &beta,
            request(
                &beta,
                "beta-apply",
                "patch.apply",
                json!({"patchId":"beta-waiting"}),
            ),
        )
        .await;
        assert!(continued_beta.get("error").is_none(), "{continued_beta}");
        assert_eq!(
            call(&alpha, preview(&alpha, "alpha-next")).await["error"]["message"],
            "WEB_TASK_GRANT_REVOKED"
        );
        assert_eq!(ledger.record("request-a").unwrap(), Some(before));
        assert_eq!(fs::read(grant_path).unwrap(), grant);
        assert_eq!(wake.delivery(event_id).unwrap().unwrap().phase, Phase::Sent);
        assert!(
            fs::read_to_string(beta.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 53 }")
        );
        assert!(
            fs::read_to_string(alpha.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn serial_web_lane_parks_untouched_wake_and_resumes_its_original_event() {
        use crate::web_work_state::execution_lane::{self, Transition};
        let (alpha, beta) = paired_fixtures(false);
        let wake = ready_manual_wake(&alpha);
        let root = &alpha.control;
        let action = |project, transition| {
            crate::web_work::transition_execution_lane(root, project, "request-a", transition)
        };
        action("alpha", Transition::Select).unwrap();
        wake.complete_timer(alpha.pins["sourceEventId"].as_str().unwrap())
            .unwrap();
        let queued = call(
            &alpha,
            manual_request(&alpha, "park-original", "alpha-turn"),
        )
        .await;
        assert!(queued.get("error").is_none(), "{queued}");
        let id = queued["result"]["structuredContent"]["eventId"]
            .as_str()
            .unwrap();
        let envelope = wake.root().join("queue").join(format!("{id}.json"));
        let original_bytes = fs::read(&envelope).unwrap();
        let witness = wake
            .root()
            .join("logs")
            .join(format!("submission-start-{id}.json"));
        fs::write(&witness, b"incomplete fixture witness").unwrap();
        assert_eq!(
            action("alpha", Transition::Pause).unwrap_err(),
            "WEB_WORK_CONTINUATION_PENDING"
        );
        assert_eq!(fs::read(&witness).unwrap(), b"incomplete fixture witness");
        fs::remove_file(&witness).unwrap();
        assert_eq!(
            action("alpha", Transition::Yield).unwrap_err(),
            "WEB_WORK_CONTINUATION_PENDING"
        );
        assert_eq!(
            action("alpha", Transition::Pause).unwrap()["status"],
            "PAUSED"
        );
        assert_eq!(execution_lane::status(root, "alpha")["status"], "PAUSED");
        assert_eq!(
            crate::web_work::fixture_manual_submission_fence(root, "alpha").unwrap_err(),
            "WEB_WORK_LANE_PAUSED"
        );
        assert!(wake.delivery(id).unwrap().is_none());
        assert!(wake.timer(id).unwrap().is_none());
        assert_eq!(fs::read(&envelope).unwrap(), original_bytes);

        let preview = call(&beta, request(
            &beta, "peer-preview", "patch.preview",
            json!({"patchId":"peer-patch","operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 65 }"}]}),
        )).await;
        assert!(preview.get("error").is_none(), "{preview}");
        let edited = call(
            &beta,
            request(
                &beta,
                "peer-edit",
                "patch.apply",
                json!({"patchId":"peer-patch"}),
            ),
        )
        .await;
        assert!(edited.get("error").is_none(), "{edited}");
        assert!(
            fs::read_to_string(beta.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 65 }")
        );
        assert!(
            fs::read_to_string(alpha.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
        action("beta", Transition::Yield).unwrap();
        action("alpha", Transition::Resume).unwrap();
        action("alpha", Transition::Select).unwrap();
        assert_eq!(fs::read(&envelope).unwrap(), original_bytes);
        let fenced = crate::web_work::fixture_manual_submission_fence(root, "alpha").unwrap();
        assert_eq!(fenced["eventId"], id);
        assert_eq!(fenced["taskCallId"], "park-original");
        assert_eq!(fenced["phase"], "SUBMITTING");
        let receiptless = wake.delivery(id).unwrap().unwrap();
        assert!(receiptless.receipt.is_none());
        assert_eq!(
            action("alpha", Transition::Pause).unwrap_err(),
            "WEB_WORK_CONTINUATION_PENDING"
        );
        assert_eq!(wake.delivery(id).unwrap(), Some(receiptless));
        assert_eq!(fs::read(&envelope).unwrap(), original_bytes);
    }

    async fn central_http_route_fixture(cached: bool) {
        let (alpha, beta) = paired_fixtures(false);
        let shared_wake = ready_manual_wake(&alpha);

        let mut app = crate::state::AppState::new_isolated_fixture(
            0,
            alpha.control.to_str().unwrap().into(),
            alpha.control.join("inert-config.toml"),
        )
        .unwrap();
        app.tool_mode = crate::state::ToolMode::SupervisorOnly;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let token = "c47-central-http-fixture-token-000000";
        let state = Arc::new(tokio::sync::Mutex::new(app));
        crate::configure_native_mcp_auth(&state, Ok(token.into()))
            .await
            .unwrap();
        let router = crate::native_mcp_router(state, None, "/central/mcp".into(), tx).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/central/mcp", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let transport = |mut body: Value| {
            if cached && body["params"]["name"] == TOOL {
                let command = format!("CATDESK_WEB_TASK={}", body["params"]["arguments"]);
                body["params"] = json!({"name":"run_command","arguments":{"command":command}});
            }
            body
        };
        let rpc = |body: Value| {
            client
                .post(&url)
                .bearer_auth(token)
                .json(&transport(body))
                .send()
        };
        let unauthorized = client
            .post(&url)
            .json(&transport(request(
                &alpha,
                "unauthorized",
                "read",
                json!({"path":"src/lib.rs"}),
            )))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        let wrong = client
            .post(&url)
            .bearer_auth("wrong-fixture-token")
            .json(&transport(request(
                &alpha,
                "wrong-auth",
                "read",
                json!({"path":"src/lib.rs"}),
            )))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), reqwest::StatusCode::UNAUTHORIZED);
        let manual_unauthorized = client
            .post(&url)
            .json(&transport(manual_request(
                &alpha,
                "manual-no-auth",
                "manual-turn",
            )))
            .send()
            .await
            .unwrap();
        assert_eq!(
            manual_unauthorized.status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        assert!(
            !alpha
                .control
                .join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal")
                .exists()
        );
        let catalog: Value = rpc(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            catalog["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|t| t["name"] == TOOL)
                .count(),
            1
        );
        let mut mixed = request(&beta, "cross-key", "read", json!({"path":"src/lib.rs"}));
        mixed["params"]["arguments"]["executionKey"] = json!(alpha.key);
        let denied: Value = rpc(mixed).await.unwrap().json().await.unwrap();
        assert_eq!(
            denied["error"]["message"], "WEB_TASK_GRANT_SCOPE_MISMATCH",
            "cached task route requires exact grant admission"
        );
        assert!(
            !beta
                .control
                .join(".catdesk/project-mcp-tasks/beta/task-fixture/journal")
                .exists()
        );
        let preview = |f: &Fixture| {
            request(
                f,
                "preview",
                "patch.preview",
                json!({"patchId":"central-patch","operations":[{"path":"src/lib.rs","old":"{ 1 }","new":if f.pins["projectId"]=="alpha"{"{ 47 }"}else{"{ 48 }"}}]}),
            )
        };
        let (pa, pb) = tokio::join!(rpc(preview(&alpha)), rpc(preview(&beta)));
        for response in [pa, pb] {
            let v: Value = response.unwrap().json().await.unwrap();
            assert!(v.get("error").is_none(), "{v}");
        }
        let apply = |f: &Fixture| {
            request(
                f,
                "apply",
                "patch.apply",
                json!({"patchId":"central-patch"}),
            )
        };
        let (aa, ab) = tokio::join!(rpc(apply(&alpha)), rpc(apply(&beta)));
        for response in [aa, ab] {
            let v: Value = response.unwrap().json().await.unwrap();
            assert!(v.get("error").is_none(), "{v}");
            assert_eq!(
                v["result"]["_meta"]["catdeskTaskLifecycle"]["status"],
                "CLOSED"
            );
        }
        assert!(
            fs::read_to_string(alpha.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 47 }")
        );
        assert!(
            fs::read_to_string(beta.task.join("src/lib.rs"))
                .unwrap()
                .contains("{ 48 }")
        );
        for f in [&alpha, &beta] {
            assert!(
                fs::read_to_string(f.native.join("src/lib.rs"))
                    .unwrap()
                    .contains("{ 1 }")
            );
        }
        let wake_a = manual_request(&alpha, "manual-concurrent", "manual-turn");
        let wake_b = manual_request(&beta, "manual-concurrent", "manual-turn");
        let (wa, wb) = tokio::join!(rpc(wake_a.clone()), rpc(wake_b));
        let mut events = std::collections::BTreeSet::new();
        for (expected, response) in [("alpha", wa), ("beta", wb)] {
            let value: Value = response.unwrap().json().await.unwrap();
            assert!(value.get("error").is_none(), "{value}");
            assert_eq!(value["result"]["structuredContent"]["projectId"], expected);
            assert_eq!(
                value["result"]["_meta"]["catdeskTaskLifecycle"]["isolatedWorktreeWriterClaimed"],
                false
            );
            events.insert(
                value["result"]["structuredContent"]["eventId"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        assert_eq!(events.len(), 2);
        let duplicate: Value = rpc(wake_a).await.unwrap().json().await.unwrap();
        assert_eq!(
            duplicate["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        assert_eq!(
            shared_wake
                .events()
                .unwrap()
                .iter()
                .filter(|event| event.wake_metadata().unwrap().is_some())
                .count(),
            2
        );
        let held = crate::operator_facade::acquire_web_work_lease(&alpha.control, "alpha").unwrap();
        let (blocked, useful) = tokio::join!(
            rpc(request(
                &alpha,
                "blocked",
                "read",
                json!({"path":"src/lib.rs"})
            )),
            rpc(request(
                &beta,
                "useful",
                "read",
                json!({"path":"src/lib.rs"})
            ))
        );
        let blocked: Value = blocked.unwrap().json().await.unwrap();
        let useful: Value = useful.unwrap().json().await.unwrap();
        assert_eq!(blocked["error"]["message"], "WEB_ROLLOVER_PROJECT_BUSY");
        assert!(useful.get("error").is_none(), "{useful}");
        let (blocked, useful) = tokio::join!(
            rpc(manual_request(&alpha, "held-manual", "manual-held-turn")),
            rpc(manual_request(&beta, "useful-manual", "manual-next-turn"))
        );
        let blocked: Value = blocked.unwrap().json().await.unwrap();
        let useful: Value = useful.unwrap().json().await.unwrap();
        assert_eq!(blocked["error"]["message"], "WEB_ROLLOVER_PROJECT_BUSY");
        assert!(useful.get("error").is_none(), "{useful}");
        drop(held);
        let replay: Value = rpc(apply(&beta)).await.unwrap().json().await.unwrap();
        assert_eq!(
            replay["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        if cached {
            let direct: Value = client
                .post(&url)
                .bearer_auth(token)
                .json(&apply(&beta))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(
                direct["error"]["message"],
                "PROJECT_TASK_CALL_ALREADY_COMPLETED"
            );
        }
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        for f in [&alpha, &beta] {
            drop(crate::delegated::autonomy_runtime::acquire_project_writer_lock(&f.task).unwrap());
        }
        fn no_keys(root: &Path, keys: &[&str]) -> usize {
            let mut files = 0;
            for entry in fs::read_dir(root).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    files += no_keys(&entry.path(), keys);
                } else if entry.path().extension().is_some_and(|e| e == "json") {
                    let raw = fs::read_to_string(entry.path()).unwrap();
                    for key in keys {
                        assert!(!raw.contains(key));
                    }
                    files += 1;
                }
            }
            files
        }
        assert!(
            no_keys(
                &alpha.control.join(".catdesk/web-context"),
                &[&alpha.key, &beta.key]
            ) >= 2
        );
    }
}
