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
        "shellExecuted":false,"cwdOverrideAllowed":false,"timeoutOverrideAllowed":false,
        "sameCallIdAndJournal":true,"centralPlanPolicyRetained":true})
}

fn cached_request(request: &JsonRpcRequest) -> Result<JsonRpcRequest, String> {
    let invalid = || "WEB_TASK_CACHED_ARGUMENTS_INVALID".to_owned();
    let arguments = request.params["arguments"]
        .as_object()
        .ok_or_else(invalid)?;
    for (name, value) in arguments {
        match name.as_str() {
            "command" if value.is_string() => {}
            "allow_without_plan" if value.is_boolean() => {}
            "dry_run" | "include_full_output" if value == &Value::Bool(false) => {}
            _ => return Err(invalid()),
        }
    }
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if command.len() > 2 * 1024 * 1024 {
        return Err(invalid());
    }
    let payload = command
        .trim_start()
        .strip_prefix(CACHED_COMMAND_PREFIX)
        .ok_or_else(invalid)?;
    let task: Value = serde_json::from_str(payload).map_err(|_| invalid())?;
    if !task.is_object() {
        return Err(invalid());
    }
    Ok(JsonRpcRequest {
        jsonrpc: request.jsonrpc.clone(),
        id: request.id.clone(),
        method: "tools/call".into(),
        params: json!({"name":TOOL,"arguments":task}),
    })
}

/// The central HTTP layer must apply the same bearer/capability checks as TOOL.
/// Only this closed JSON adapter runs; the command text is never shell input.
pub(crate) async fn handle_cached(control: &str, request: &JsonRpcRequest) -> JsonRpcResponse {
    let adapted = match cached_request(request) {
        Ok(adapted) => adapted,
        Err(reason) => return JsonRpcResponse::error(request.id.clone(), -32602, reason),
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
    json!({"name":TOOL,"description":"Execute one operator-granted isolated project task through the central connection. Use the exact project/work/task identity, current paired target, private project executionKey and stable callId. The registry and archived assignment choose the workspace. Recorded calls never replay. This does not identify the provider conversation or take over a native checkout.",
    "inputSchema":{"type":"object","properties":{
        "projectId":{"type":"string"},"requestId":{"type":"string"},"sourceEventId":{"type":"string"},
        "assignmentSha256":{"type":"string","pattern":"^[a-f0-9]{64}$"},
        "expectedTargetGeneration":{"type":"integer","minimum":1},"expectedTargetSha256":{"type":"string","pattern":"^[a-f0-9]{64}$"},
        "executionKey":{"type":"string","pattern":"^[a-f0-9]{64}$","description":"Private key from this task's LOCAL execution-grant; never use a sibling project's key."},
        "taskId":{"type":"string"},"callId":{"type":"string"},
        "tool":{"type":"string","enum":["read","search","patch.preview","patch.apply","patch.compare","diff.actual","verify.command"]},"arguments":{"type":"object"}},
    "required":["projectId","requestId","sourceEventId","assignmentSha256","expectedTargetGeneration","expectedTargetSha256","executionKey","taskId","callId","tool","arguments"],"additionalProperties":false},
    "annotations":{"readOnlyHint":false,"destructiveHint":true,"openWorldHint":false}})
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
    let admission =
        crate::web_work::authorized_assignment(control, call.pins(), &call.execution_key)?;
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
mod tests {
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
    }
    fn fixture(id: &str) -> Fixture {
        let (control, assignment, wake, pins, event) =
            crate::web_work::tests::assignment_fixture_for(id);
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
        let granted = crate::web_work::execution_permission(&control, pins.clone(), false).unwrap();
        assert!(!Path::new(granted["executionKeyFile"].as_str().unwrap()).starts_with(&control));
        let key: String = serde_json::from_slice(
            &fs::read(granted["executionKeyFile"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let again = crate::web_work::execution_permission(&control, pins.clone(), false).unwrap();
        assert_eq!(granted, again);
        assert!(!granted.to_string().contains(&key));
        Fixture {
            control,
            pins,
            key,
            task,
            native,
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

    async fn central_http_route_fixture(cached: bool) {
        let alpha = fixture("alpha");
        let mut beta = fixture("beta");
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
        let grant =
            crate::web_work::execution_permission(&beta.control, beta.pins.clone(), false).unwrap();
        beta.key =
            serde_json::from_slice(&fs::read(grant["executionKeyFile"].as_str().unwrap()).unwrap())
                .unwrap();

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
