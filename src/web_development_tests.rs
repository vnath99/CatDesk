//! A local development-root routing fixture. It never starts Wake or a browser.

use crate::{
    delegated::autonomy_projects::AutonomousProjectRegistryStoreV1,
    operator_facade,
    state::{AppState, Mode, ToolMode},
    wake_protocol_client,
    web_conversation_context::{
        ContextPressurePolicy, ConversationKey, Observation, VisibleSourceKind, WebContextStore,
    },
};
use catdesk_wake::{development_runtime, runtime, store::Store};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn fixture_parent() -> PathBuf {
    let parent = std::env::temp_dir().join(format!(
        "catdesk-web-development-integration-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&parent).unwrap();
    parent
}

fn git_root(path: &Path, origin: &str) {
    fs::create_dir_all(path).unwrap();
    for args in [
        &["init", "-q"][..],
        &["remote", "add", "origin", origin][..],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
}

fn package_and_python(parent: &Path) -> (PathBuf, PathBuf) {
    let package = parent.join("package");
    let python = parent.join("python");
    fs::create_dir(&package).unwrap();
    fs::create_dir(&python).unwrap();
    fs::write(
        package.join("CatDeskWakeHost.exe"),
        b"fixture host; never execute",
    )
    .unwrap();
    fs::write(
        package.join("CatDeskBinagotchy.exe"),
        b"fixture gui; never execute",
    )
    .unwrap();
    fs::write(
        package.join("adapter.py"),
        include_bytes!("../wake/adapter.py"),
    )
    .unwrap();
    fs::write(
        package.join("wake_bridge.py"),
        include_bytes!("../scripts/wake_bridge.py"),
    )
    .unwrap();
    fs::write(python.join("python.exe"), b"fixture Python; never execute").unwrap();
    fs::write(python.join("__init__.py"), b"").unwrap();
    (package, python)
}

fn production_snapshot() -> Vec<(PathBuf, Option<String>)> {
    let direct = runtime::default_root().unwrap();
    let historical = PathBuf::from(std::env::var_os("USERPROFILE").unwrap())
        .join(r"AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\CatDeskWake");
    let mut roots = vec![direct];
    if !roots.contains(&historical) {
        roots.push(historical);
    }
    let mut snapshot = Vec::new();
    for root in roots {
        for leaf in [
            "activation.json",
            "config.json",
            "control.json",
            "current.json",
        ] {
            let path = root.join(leaf);
            let digest = match fs::read(&path) {
                Ok(bytes) => Some(format!("{:x}", Sha256::digest(bytes))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("production snapshot {}: {error}", path.display()),
            };
            snapshot.push((path, digest));
        }
    }
    snapshot
}

async fn web_action(control: &Path, args: &[&str]) -> serde_json::Value {
    let action = operator_facade::parse_operator_action(args.iter().map(|value| (*value).into()))
        .unwrap()
        .unwrap();
    operator_facade::execute_operator_action(action, control)
        .await
        .unwrap()
}

fn cleanup(parent: &Path) {
    let canonical = parent.canonicalize().unwrap();
    assert_eq!(
        canonical.parent(),
        Some(std::env::temp_dir().canonicalize().unwrap().as_path())
    );
    assert!(
        canonical
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("catdesk-web-development-integration-")
    );
    fs::remove_dir_all(canonical).unwrap();
}

async fn web_mcp_request(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    request: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let request = client.post(url).json(&request);
    let request = match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    };
    let response = request.send().await.unwrap();
    let status = response.status();
    (status, response.json().await.unwrap())
}

async fn web_mcp_tool(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let (status, response) = web_mcp_request(
        client,
        url,
        Some(token),
        serde_json::json!({
            "jsonrpc":"2.0", "id":name, "method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK);
    response
}

#[tokio::test(flavor = "current_thread")]
async fn three_projects_route_only_to_prepared_development_wake() {
    let production_before = production_snapshot();
    let parent = fixture_parent();
    let control = parent.join("control");
    fs::create_dir(&control).unwrap();
    let (package, python) = package_and_python(&parent);
    assert!(
        development_runtime::root_for_workspace(&control)
            .unwrap()
            .is_none()
    );
    development_runtime::prepare(&control, &package, &python).unwrap();
    let dev_root = development_runtime::root_for_workspace(&control)
        .unwrap()
        .unwrap();
    assert_eq!(
        wake_protocol_client::wake_root_for_workspace(&control).unwrap(),
        dev_root
    );
    assert_eq!(
        wake_protocol_client::wake_runtime_mode_for_workspace(&control),
        Ok("DEVELOPMENT_UNPROTECTED")
    );

    let projects = [
        ("catdesk", control.clone()),
        ("alpha", parent.join("alpha")),
        ("beta", parent.join("beta")),
    ];
    for (id, workspace) in &projects {
        let origin = format!("https://example.test/{id}.git");
        git_root(workspace, &origin);
        let registered = web_action(
            &control,
            &[
                "operator",
                "web",
                "project",
                "register",
                "--project-id",
                id,
                "--workspace",
                workspace.to_str().unwrap(),
                "--git-identity",
                &origin,
            ],
        )
        .await;
        assert_eq!(registered["status"], "REGISTERED_WEB_ONLY");
        assert_eq!(registered["nativeThreadId"], serde_json::Value::Null);
        assert_eq!(registered["runtimeMode"], "DEVELOPMENT_UNPROTECTED");
    }
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(control.join(".catdesk/projects"))
            .unwrap();
    assert_eq!(registry.load_registry().unwrap().projects.len(), 3);

    let meter = WebContextStore::open(control.join(".catdesk/web-context")).unwrap();
    for (index, (id, _)) in projects.iter().enumerate() {
        let url = format!("https://chatgpt.com/c/{}", uuid::Uuid::new_v4());
        let bound = web_action(
            &control,
            &[
                "operator",
                "web",
                "target",
                "bind",
                "--project-id",
                id,
                "--conversation-url",
                &url,
                "--expected-current-target-sha256",
                "none",
                "--expected-wake-generation",
                "0",
            ],
        )
        .await;
        assert_eq!(bound["status"], "PAIRED");
        assert_eq!(bound["runtimeMode"], "DEVELOPMENT_UNPROTECTED");
        assert_eq!(bound["target"]["url"], url);
        let read = web_action(
            &control,
            &["operator", "web", "target", "read", "--project-id", id],
        )
        .await;
        assert_eq!(read["runtimeMode"], "DEVELOPMENT_UNPROTECTED");
        assert_eq!(read["wake"]["target"]["url"], url);

        if index == 0 {
            let private_file = control.join(".catdesk/private-fixture.txt");
            fs::write(&private_file, b"do not read private state").unwrap();
            let private_path = private_file.to_string_lossy().into_owned();
            assert_eq!(
                web_action_result(
                    &control,
                    &[
                        "operator",
                        "web-dev-runtime",
                        "enqueue",
                        "--project-id",
                        id,
                        "--event-id",
                        "dev-private-refused",
                        "--message-file",
                        &private_path,
                    ],
                )
                .await
                .unwrap_err(),
                "DEV_MESSAGE_PRIVATE_STATE_REFUSED"
            );
        }

        let event_id = format!("dev-{id}");
        let message_file = control.join(format!("fixture-message-{id}.txt"));
        fs::write(
            &message_file,
            format!("Work only on the {id} fixture task."),
        )
        .unwrap();
        let message_path = message_file.to_string_lossy().into_owned();
        let enqueue_args = [
            "operator",
            "web-dev-runtime",
            "enqueue",
            "--project-id",
            id,
            "--event-id",
            &event_id,
            "--message-file",
            &message_path,
        ];
        let queued = web_action(&control, &enqueue_args).await;
        assert_eq!(queued["status"], "QUEUED");
        assert_eq!(queued["mode"], "DEVELOPMENT_UNPROTECTED");
        assert_eq!(queued["deliveryProved"], false);
        assert_eq!(queued["naturalReviewProved"], false);
        assert_eq!(web_action(&control, &enqueue_args).await, queued);
        fs::write(
            &message_file,
            format!("Changed body for the {id} fixture task."),
        )
        .unwrap();
        assert_eq!(
            web_action_result(&control, &enqueue_args)
                .await
                .unwrap_err(),
            "QUEUE_EVENT_ID_CONFLICT"
        );
        let key = ConversationKey {
            project_id: (*id).into(),
            conversation_url: url,
            target_generation: 1,
        };
        meter.bind_observed_target(key.clone(), None).unwrap();
        meter
            .configure_policy(
                id,
                ContextPressurePolicy {
                    configured_budget_tokens: 96_000 + (index as u64) * 16_000,
                    ..Default::default()
                },
            )
            .unwrap();
        let visible = format!("Fixture context for {id}; no browser observation is claimed.");
        meter
            .record_observation(
                &key,
                Observation {
                    event_id: &format!("fixture-message-{id}"),
                    revision: 1,
                    dedup_key: &format!("fixture-dedup-{id}"),
                    source_kind: VisibleSourceKind::UserMessage,
                    visible_text: Some(&visible),
                    observed_at_unix: catdesk_wake::store::now(),
                },
            )
            .unwrap();
        assert_eq!(
            meter.pressure(&key).unwrap().configured_budget_tokens,
            96_000 + (index as u64) * 16_000
        );
    }
    let wake = Store::open(&dev_root).unwrap();
    let events = wake.events().unwrap();
    assert_eq!(events.len(), 3);
    for (id, _) in &projects {
        let target = wake.config().unwrap().targets.get(*id).unwrap().clone();
        assert!(events.iter().any(|event| event.project_id == *id
            && event.target_generation == target.generation
            && event.event_id == format!("dev-{id}")));
    }
    let status = web_action(&control, &["operator", "web", "status"]).await;
    assert_eq!(status["runtimeMode"], "DEVELOPMENT_UNPROTECTED");
    assert_eq!(status["projects"].as_array().unwrap().len(), 3);
    let central = &status["webProjects"];
    assert_eq!(central["readOnly"], true);
    assert_eq!(central["status"], "SOURCE_STATUS_ONLY");
    assert_eq!(central["installedAcceptance"], "UNVERIFIED");
    assert_eq!(central["projects"].as_array().unwrap().len(), 3);
    for project in central["projects"].as_array().unwrap() {
        assert_eq!(project["wake"]["status"], "PAIRED", "{project}");
        assert_eq!(project["context"]["status"], "CURRENT_ADVISORY");
        assert_eq!(project["rollover"]["status"], "UNOBSERVED");
    }
    for project in status["projects"].as_array().unwrap() {
        assert_eq!(project["wake"]["status"], "PAIRED");
        assert_eq!(project["wake"]["runtimeMode"], "DEVELOPMENT_UNPROTECTED");
        assert_eq!(project["context"]["status"], "OBSERVED");
    }
    assert_eq!(production_snapshot(), production_before);

    fs::write(
        control.join(".catdesk/web-development/authority.json"),
        b"{}",
    )
    .unwrap();
    assert!(wake_protocol_client::wake_root_for_workspace(&control).is_err());
    assert!(
        web_action_result(&control, &["operator", "web", "status"])
            .await
            .is_err()
    );
    fs::remove_file(control.join(".catdesk/web-development/authority.json")).unwrap();
    assert!(wake_protocol_client::wake_root_for_workspace(&control).is_err());
    assert_eq!(production_snapshot(), production_before);
    cleanup(&parent);
}

async fn web_action_result(control: &Path, args: &[&str]) -> Result<serde_json::Value, String> {
    let action = operator_facade::parse_operator_action(args.iter().map(|value| (*value).into()))?
        .ok_or("FIXTURE_ACTION_MISSING")?;
    operator_facade::execute_operator_action(action, control).await
}

#[tokio::test(flavor = "current_thread")]
async fn central_web_work_requests_are_independent_durable_and_generation_bound() {
    use serde_json::json;
    let production_before = production_snapshot();
    let parent = fixture_parent();
    let control = parent.join("control");
    fs::create_dir(&control).unwrap();
    let (package, python) = package_and_python(&parent);
    development_runtime::prepare(&control, &package, &python).unwrap();
    let root = development_runtime::root_for_workspace(&control)
        .unwrap()
        .unwrap();
    let wake = Store::open(&root).unwrap();
    let projects = [
        ("catdesk", control.clone()),
        ("alpha", parent.join("alpha")),
        ("beta", parent.join("beta")),
    ];
    for (project, workspace) in &projects {
        let origin = format!("https://example.test/{project}.git");
        git_root(workspace, &origin);
        web_action(
            &control,
            &[
                "operator",
                "web",
                "project",
                "register",
                "--project-id",
                project,
                "--workspace",
                workspace.to_str().unwrap(),
                "--git-identity",
                &origin,
            ],
        )
        .await;
        let url = format!("https://chatgpt.com/c/{}", uuid::Uuid::new_v4());
        web_action(
            &control,
            &[
                "operator",
                "web",
                "target",
                "bind",
                "--project-id",
                project,
                "--conversation-url",
                &url,
                "--expected-current-target-sha256",
                "none",
                "--expected-wake-generation",
                "0",
            ],
        )
        .await;
    }
    // Ordinary publication deliberately refuses the target-change second.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let bindings = wake.config().unwrap().targets;
    let arguments = |project: &str, request: &str| {
        json!({
            "projectId":project, "requestId":request, "expectedTargetGeneration":1,
            "expectedTargetSha256":bindings[project].digest,
            "taskText":"Review this project's current work; preserve the existing Codex task owner."
        })
    };

    // Use the same disposable development control as the real HTTP MCP route.
    // The listener is loopback-only, needs a bearer token, and has no tunnel.
    let mut app = AppState::new_for_test(
        0,
        control.to_string_lossy().into_owned(),
        control.join(".catdesk/web-development/catdesk.toml"),
    )
    .unwrap();
    app.mode = Mode::Computer;
    app.tool_mode = ToolMode::SupervisorOnly;
    app.mcp_slug = "web-work-fixture".into();
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let mcp_path = "/web-work-fixture/mcp";
    let token = "catdesk-web-work-fixture-token-1234567890";
    let router = crate::server::router(
        std::sync::Arc::new(tokio::sync::Mutex::new(app)),
        None,
        mcp_path.into(),
        ui_tx,
        Some(token.into()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}{mcp_path}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();
    let (denied_status, _) = web_mcp_request(
        &client,
        &url,
        None,
        json!({"jsonrpc":"2.0","id":"denied","method":"tools/call",
            "params":{"name":"catdesk_web_work_request",
                "arguments":arguments("beta", "same-request-id")}}),
    )
    .await;
    assert_eq!(denied_status, reqwest::StatusCode::UNAUTHORIZED);
    assert!(wake.events().unwrap().is_empty());
    assert_eq!(
        crate::web_work_state::WorkLedger::status(&control, "beta")["status"],
        "UNOBSERVED"
    );
    let (list_status, listed) = web_mcp_request(
        &client,
        &url,
        Some(token),
        json!({"jsonrpc":"2.0","id":"list","method":"tools/list","params":{}}),
    )
    .await;
    assert_eq!(list_status, reqwest::StatusCode::OK);
    let names = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"catdesk_web_work_request"));
    assert!(names.contains(&"catdesk_web_work_resolve"));
    assert!(names.contains(&"catdesk_web_context_configure"));
    assert!(names.contains(&"catdesk_web_context_observe"));
    assert!(!names.contains(&"run_command"));

    let context_config = |project: &str| {
        json!({"projectId":project,"expectedTargetGeneration":1,
        "expectedTargetSha256":bindings[project].digest,"policy":{
            "configuredBudgetTokens":match project {"catdesk"=>4000,"alpha"=>6000,_=>8000},
            "safetyHeadroomTokens":1000,"unknownSourceReserveTokens":500,
            "automaticRolloverEnabled":false,"automaticObservationEnabled":false,"incompleteHistoryRollover":false}})
    };
    let context_event = |project: &str| {
        json!({"projectId":project,"expectedTargetGeneration":1,
        "expectedTargetSha256":bindings[project].digest,"event":{"eventId":"same-event",
            "revision":1,"dedupKey":"same-output","sourceKind":"TOOL_RESULT",
            "visibleText":format!("PRIVATE_CONTEXT_{project} {}", "context ".repeat(10))}})
    };
    let (denied, _) = web_mcp_request(&client, &url, None, json!({"jsonrpc":"2.0","id":"denied-context",
        "method":"tools/call","params":{"name":"catdesk_web_context_configure","arguments":context_config("catdesk")}})).await;
    assert_eq!(denied, reqwest::StatusCode::UNAUTHORIZED);
    assert!(!control.join(".catdesk/web-context").exists());
    for (project, _) in &projects {
        let configured = web_mcp_tool(
            &client,
            &url,
            token,
            "catdesk_web_context_configure",
            context_config(project),
        )
        .await;
        let result = &configured["result"]["structuredContent"];
        assert_eq!(
            result["pressure"]["configuredBudgetTokens"],
            context_config(project)["policy"]["configuredBudgetTokens"]
        );
        assert_eq!(result["originAuthenticated"], false);
        assert_eq!(result["hostStarted"], false);
        let observed = web_mcp_tool(
            &client,
            &url,
            token,
            "catdesk_web_context_observe",
            context_event(project),
        )
        .await;
        let result = &observed["result"]["structuredContent"];
        assert_eq!(result["pressure"]["key"]["projectId"], *project);
        assert_eq!(result["pressure"]["recordedEvents"], 1);
        assert!(
            result["pressure"]["observedToolResultTokens"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(result["observationSource"], "CALLER_REPORTED");
        assert_eq!(result["rawTextPersisted"], false);
        assert_eq!(result["browserAction"], "NONE");
        assert!(!observed.to_string().contains("PRIVATE_CONTEXT_"));
        let replay = web_mcp_tool(
            &client,
            &url,
            token,
            "catdesk_web_context_observe",
            context_event(project),
        )
        .await;
        assert_eq!(
            replay["result"]["structuredContent"]["pressure"],
            result["pressure"]
        );
    }
    // A contended metadata lock must refuse immediately instead of freezing
    // the HTTP reactor. The separate thread bounds the old blocking behavior
    // so this regression cannot hang the suite before the fix.
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(control.join(".catdesk/web-context/ledger.lock"))
        .unwrap();
    held.lock().unwrap();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release = std::thread::spawn(move || {
        let _ = release_rx.recv_timeout(std::time::Duration::from_secs(30));
        drop(held);
    });
    let busy = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_context_configure",
        context_config("alpha"),
    )
    .await;
    let busy_observe = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_context_observe",
        context_event("beta"),
    )
    .await;
    let sibling_status = web_mcp_tool(
        &client,
        &url,
        token,
        "autonomy_project_registry_read",
        json!({}),
    )
    .await;
    let released_before_timeout = release_tx.send(()).is_ok();
    release.join().unwrap();
    assert!(
        released_before_timeout,
        "context operations waited for fixture release timeout"
    );
    assert!(
        busy.to_string().contains("WEB_CONTEXT_BUSY"),
        "context mutation waited for held lock: {busy}"
    );
    assert!(busy_observe.to_string().contains("WEB_CONTEXT_BUSY"));
    assert_ne!(sibling_status["result"]["isError"], true);
    let busy_projects = sibling_status["result"]["structuredContent"]["webProjects"]["projects"]
        .as_array()
        .unwrap();
    assert_eq!(busy_projects.len(), 3);
    assert!(
        busy_projects
            .iter()
            .all(|project| project["wake"]["status"] == "PAIRED")
    );

    let independent = crate::mcp::with_current_project_web_target(
        &control,
        "alpha",
        1,
        &bindings["alpha"].digest,
        |workspace, _| {
            let sibling = crate::mcp::operator_read_project_web_target_status(workspace, "beta")
                .map_err(|_| "WEB_CONTEXT_PARALLEL_READ_UNAVAILABLE")?
                .ok_or("WEB_CONTEXT_PARALLEL_READ_UNAVAILABLE")?;
            assert_eq!(sibling.sha256, bindings["beta"].digest);
            Ok(())
        },
    );
    assert!(
        independent.is_ok(),
        "sibling status blocked during context operation: {independent:?}"
    );
    let meter = WebContextStore::open_read_only(control.join(".catdesk/web-context")).unwrap();
    let before = meter.project_history("alpha").unwrap();
    let mut wrong_scope = context_config("alpha");
    wrong_scope["expectedTargetSha256"] = json!(bindings["beta"].digest);
    wrong_scope["policy"]["configuredBudgetTokens"] = json!(9000);
    let refused = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_context_configure",
        wrong_scope,
    )
    .await;
    assert!(refused.to_string().contains("WEB_CONTEXT_TARGET_CHANGED"));
    let mut conflict = context_event("alpha");
    conflict["event"]["visibleText"] = json!("different private content");
    let refused = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_context_observe",
        conflict,
    )
    .await;
    assert!(
        refused
            .to_string()
            .contains("WEB_CONTEXT_EVENT_REVISION_CONFLICT")
    );
    assert_eq!(meter.project_history("alpha").unwrap(), before);
    let mut revision = context_event("beta");
    revision["event"]["revision"] = json!(2);
    revision["event"]["visibleText"] = json!("revised visible context ".repeat(30));
    let revised = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_context_observe",
        revision,
    )
    .await;
    assert_eq!(
        revised["result"]["structuredContent"]["pressure"]["recordedEvents"],
        1
    );
    let mut arguments_event = context_event("alpha");
    arguments_event["event"]["eventId"] = json!("tool-arguments");
    arguments_event["event"]["dedupKey"] = json!("tool-arguments");
    arguments_event["event"]["sourceKind"] = json!("TOOL_ARGUMENTS");
    let traffic = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_context_observe",
        arguments_event,
    )
    .await;
    assert!(
        traffic["result"]["structuredContent"]["pressure"]["observedToolArgumentTokens"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        traffic["result"]["structuredContent"]["pressure"]["visibleUniqueTokens"],
        json!(before[0].visible_unique_tokens)
    );
    let mut unknown = context_event("beta");
    unknown["event"]["eventId"] = json!("unknown-attachment");
    unknown["event"]["dedupKey"] = json!("unknown-attachment");
    unknown["event"]["sourceKind"] = json!("UNKNOWN_ATTACHMENT");
    unknown["event"]["visibleText"] = json!(null);
    let reserved = web_mcp_tool(&client, &url, token, "catdesk_web_context_observe", unknown).await;
    assert_eq!(
        reserved["result"]["structuredContent"]["pressure"]["unknownSourceCount"],
        1
    );
    assert!(
        wake.events().unwrap().is_empty(),
        "context controls publish no Wake events"
    );
    for entry in fs::read_dir(control.join(".catdesk/web-context/conversations")).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !String::from_utf8(bytes)
                .unwrap()
                .contains("PRIVATE_CONTEXT_")
        );
    }

    let mut event_ids = std::collections::BTreeSet::new();
    for (project, _) in &projects {
        let input = arguments(project, "same-request-id");
        let recorded = if *project == "beta" {
            let response = web_mcp_tool(
                &client,
                &url,
                token,
                "catdesk_web_work_request",
                input.clone(),
            )
            .await;
            assert_ne!(response["result"]["isError"], true);
            response["result"]["structuredContent"].clone()
        } else {
            crate::web_work::request(&control, input.clone()).unwrap()
        };
        assert_eq!(recorded["status"], "REQUEST_RECORDED");
        assert_eq!(recorded["nativeSessionAction"], "NONE");
        assert_eq!(recorded["deliveryProved"], false);
        assert_eq!(recorded["taskCompletionVerified"], false);
        let retried = if *project == "beta" {
            let response = web_mcp_tool(
                &client,
                &url,
                token,
                "catdesk_web_work_request",
                input.clone(),
            )
            .await;
            assert_ne!(response["result"]["isError"], true);
            response["result"]["structuredContent"].clone()
        } else {
            crate::web_work::request(&control, input.clone()).unwrap()
        };
        assert_eq!(retried, recorded);
        event_ids.insert(recorded["eventId"].as_str().unwrap().to_owned());
        let mut conflict = input;
        conflict["taskText"] = json!("A different task under the same request ID.");
        if *project == "beta" {
            let response =
                web_mcp_tool(&client, &url, token, "catdesk_web_work_request", conflict).await;
            assert_eq!(response["result"]["isError"], true);
            assert!(response.to_string().contains("WEB_WORK_REQUEST_CONFLICT"));
            let response = web_mcp_tool(
                &client,
                &url,
                token,
                "catdesk_web_work_request",
                arguments(project, "another-task"),
            )
            .await;
            assert_eq!(response["result"]["isError"], true);
            assert!(response.to_string().contains("WEB_WORK_ACTIVE_REQUEST"));
        } else {
            assert_eq!(
                crate::web_work::request(&control, conflict).unwrap_err(),
                "WEB_WORK_REQUEST_CONFLICT"
            );
            assert_eq!(
                crate::web_work::request(&control, arguments(project, "another-task")).unwrap_err(),
                "WEB_WORK_ACTIVE_REQUEST"
            );
        }
    }
    assert_eq!(event_ids.len(), 3);
    let events = wake.events().unwrap();
    assert_eq!(events.len(), 3);
    assert!(
        events
            .iter()
            .all(|event| event.event_type == "engineering_attention")
    );
    for project in ["catdesk", "alpha", "beta"] {
        assert_eq!(
            events
                .iter()
                .filter(|event| event.project_id == project)
                .count(),
            1
        );
    }
    let status = web_mcp_tool(
        &client,
        &url,
        token,
        "autonomy_project_registry_read",
        json!({}),
    )
    .await;
    assert_ne!(status["result"]["isError"], true);
    let reported_projects = status["result"]["structuredContent"]["webProjects"]["projects"]
        .as_array()
        .unwrap();
    assert_eq!(reported_projects.len(), 3);
    for project in ["catdesk", "alpha", "beta"] {
        let reported = reported_projects
            .iter()
            .find(|reported| reported["projectId"] == project)
            .unwrap();
        assert_eq!(
            reported["workIntent"]["active"]["requestId"],
            "same-request-id"
        );
        assert_eq!(reported["queue"]["workRequests"]["recordCount"], 1);
        assert_eq!(
            reported["queue"]["workRequests"]["taskCompletionVerified"],
            false
        );
    }
    let resolution = |project: &str, outcome: &str| {
        json!({"projectId":project,"requestId":"same-request-id",
            "expectedTargetGeneration":1,"expectedTargetSha256":bindings[project].digest,
            "outcome":outcome,"summary":"Coordinator reports the controlled review is closed."})
    };
    assert_eq!(
        crate::web_work::resolve(&control, resolution("catdesk", "completed")).unwrap_err(),
        "WEB_WORK_DELIVERY_REQUIRED"
    );
    // A queued request can be retired without claiming it was delivered. Its
    // archived transport and closed intent make an exact retry harmless.
    let retired = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_work_resolve",
        resolution("beta", "retired"),
    )
    .await;
    assert_ne!(retired["result"]["isError"], true);
    let closed = retired["result"]["structuredContent"].clone();
    assert_eq!(closed["taskCompletionVerified"], false);
    let retired_retry = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_work_resolve",
        resolution("beta", "retired"),
    )
    .await;
    assert_eq!(retired_retry["result"]["structuredContent"], closed);
    let already_closed = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_work_request",
        arguments("beta", "same-request-id"),
    )
    .await;
    assert_eq!(
        already_closed["result"]["structuredContent"]["status"],
        "REQUEST_ALREADY_CLOSED"
    );
    let remaining_events = wake.events().unwrap();
    assert_eq!(remaining_events.len(), 2);
    assert_eq!(
        remaining_events
            .iter()
            .map(|event| (&event.project_id, &event.event_id))
            .collect::<std::collections::BTreeSet<_>>(),
        events
            .iter()
            .filter(|event| event.project_id != "beta")
            .map(|event| (&event.project_id, &event.event_id))
            .collect::<std::collections::BTreeSet<_>>()
    );
    let status = web_mcp_tool(
        &client,
        &url,
        token,
        "autonomy_project_registry_read",
        json!({}),
    )
    .await;
    let reported_projects = status["result"]["structuredContent"]["webProjects"]["projects"]
        .as_array()
        .unwrap();
    for project in ["catdesk", "alpha"] {
        let reported = reported_projects
            .iter()
            .find(|reported| reported["projectId"] == project)
            .unwrap();
        assert_eq!(
            reported["workIntent"]["active"]["requestId"],
            "same-request-id"
        );
    }
    let beta = reported_projects
        .iter()
        .find(|reported| reported["projectId"] == "beta")
        .unwrap();
    assert!(beta["workIntent"]["active"].is_null());
    assert_eq!(beta["workIntent"]["recordCount"], 1);

    // Synthetic receipt/timer evidence is confined to this disposable Store.
    // The coordinator cannot close delivered work while its response is active.
    use catdesk_wake::{
        protocol::digest,
        store::{Phase, Receipt},
    };
    let event = events
        .iter()
        .find(|event| event.project_id == "catdesk")
        .unwrap();
    let lease = wake.host_lock().unwrap();
    let delivery = wake.claim(event, &lease).unwrap();
    wake.transition(
        &event.event_id,
        lease.owner(),
        Phase::Submitting,
        None,
        None,
    )
    .unwrap();
    let receipt = Receipt {
        schema_version: 1,
        event_id: event.event_id.clone(),
        target_generation: 1,
        target_digest: bindings["catdesk"].digest.clone(),
        message_digest: digest(&event.message),
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
    assert_eq!(
        crate::web_work::resolve(&control, resolution("catdesk", "completed")).unwrap_err(),
        "WEB_WORK_COMPLETED_RESPONSE_REQUIRED"
    );
    assert_eq!(
        crate::web_work::resolve(&control, resolution("catdesk", "retired")).unwrap_err(),
        "WEB_WORK_COMPLETED_RESPONSE_REQUIRED"
    );
    wake.complete_timer(&event.event_id).unwrap();
    drop(lease);
    let replacement_url = format!("https://chatgpt.com/c/{}", uuid::Uuid::new_v4());
    web_action(
        &control,
        &[
            "operator",
            "web",
            "target",
            "bind",
            "--project-id",
            "catdesk",
            "--conversation-url",
            &replacement_url,
            "--expected-current-target-sha256",
            &bindings["catdesk"].digest,
            "--expected-wake-generation",
            "1",
        ],
    )
    .await;
    // Closing an existing intent uses its original witness after a target
    // changes. The preserved receipt cannot be relabelled as the new target.
    let old_context = meter.project_history("catdesk").unwrap();
    for (tool, input) in [
        ("catdesk_web_context_configure", context_config("catdesk")),
        ("catdesk_web_context_observe", context_event("catdesk")),
        (
            "catdesk_web_context_capture",
            json!({"projectId":"catdesk","expectedTargetGeneration":1,
            "expectedTargetSha256":bindings["catdesk"].digest}),
        ),
    ] {
        let stale = web_mcp_tool(&client, &url, token, tool, input).await;
        assert!(stale.to_string().contains("WEB_CONTEXT_TARGET_CHANGED"));
    }
    assert_eq!(meter.project_history("catdesk").unwrap(), old_context);
    let mut replacement_event = context_event("catdesk");
    replacement_event["expectedTargetGeneration"] = json!(2);
    replacement_event["expectedTargetSha256"] = json!(digest(&replacement_url));
    let recorded = web_mcp_tool(
        &client,
        &url,
        token,
        "catdesk_web_context_observe",
        replacement_event,
    )
    .await;
    assert_eq!(
        recorded["result"]["structuredContent"]["pressure"]["key"]["targetGeneration"],
        2
    );
    let context_history = meter.project_history("catdesk").unwrap();
    assert_eq!(context_history.len(), 2);
    assert_eq!(
        context_history[0].recorded_events,
        old_context[0].recorded_events
    );
    assert_eq!(meter.project_history("alpha").unwrap().len(), 1);
    assert_eq!(meter.project_history("beta").unwrap().len(), 1);
    let mut new_witness = resolution("catdesk", "completed");
    new_witness["expectedTargetGeneration"] = json!(2);
    new_witness["expectedTargetSha256"] = json!(digest(&replacement_url));
    assert_eq!(
        crate::web_work::resolve(&control, new_witness).unwrap_err(),
        "WEB_WORK_TARGET_CHANGED"
    );
    let closed = crate::web_work::resolve(&control, resolution("catdesk", "completed")).unwrap();
    assert_eq!(closed["taskCompletionVerified"], false);
    assert_eq!(
        crate::web_work::resolve(&control, resolution("catdesk", "completed")).unwrap(),
        closed
    );
    let mut wrong_resolution = resolution("alpha", "retired");
    wrong_resolution["expectedTargetSha256"] = json!(bindings["beta"].digest);
    assert_eq!(
        crate::web_work::resolve(&control, wrong_resolution).unwrap_err(),
        "WEB_WORK_TARGET_CHANGED"
    );
    // Crash boundary: the intent was persisted, but publication never happened.
    // The exact intent can be retired so the project is not permanently blocked.
    let ledger = crate::web_work_state::WorkLedger::open(&control, "beta").unwrap();
    let unpublished_id = format!("web-work-{}", digest("beta\0unpublished"));
    ledger
        .prepare(
            "unpublished",
            &unpublished_id,
            &digest("unpublished fixture"),
            &bindings["beta"],
        )
        .unwrap();
    let mut unpublished = resolution("beta", "retired");
    unpublished["requestId"] = json!("unpublished");
    assert_eq!(
        crate::web_work::resolve(&control, unpublished).unwrap()["phase"],
        "CLOSED"
    );
    let mut stale = arguments("alpha", "wrong-generation");
    stale["expectedTargetGeneration"] = json!(2);
    assert_eq!(
        crate::web_work::request(&control, stale).unwrap_err(),
        "WEB_WORK_TARGET_CHANGED"
    );
    let mut stale = arguments("alpha", "wrong-digest");
    stale["expectedTargetSha256"] = json!("b".repeat(64));
    assert_eq!(
        crate::web_work::request(&control, stale).unwrap_err(),
        "WEB_WORK_TARGET_CHANGED"
    );
    // One project's origin drift refuses its request and leaves the other
    // project's independently registered request usable.
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&projects[1].1)
            .args([
                "remote",
                "set-url",
                "origin",
                "https://example.test/changed.git"
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(crate::web_work::request(&control, arguments("alpha", "after-drift")).is_err());
    assert_eq!(
        crate::web_work::request(&control, arguments("beta", "after-drift")).unwrap()["status"],
        "REQUEST_RECORDED"
    );
    assert_eq!(wake.events().unwrap().len(), 2);
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(control.join(".catdesk/projects"))
            .unwrap();
    assert!(
        registry
            .load_registry()
            .unwrap()
            .projects
            .iter()
            .all(|project| project.codex_thread_id.is_none())
    );
    for (_, workspace) in &projects[1..] {
        assert!(
            !workspace.join(".catdesk").exists(),
            "publication must not write the project checkout"
        );
    }
    assert_eq!(production_snapshot(), production_before);
    server.abort();
    let _ = server.await;
    cleanup(&parent);
}

// Exercise the real scope checks, capture lifecycle and ingestion with a fake
// snapshot provider. No runtime reader, browser, native session or Wake starts.
#[tokio::test(flavor = "current_thread")]
async fn bounded_context_capture_is_independent_cancelled_and_generation_bound() {
    use crate::web_context_control::capture_with;
    use catdesk_wake::protocol::digest;
    use serde_json::{Value, json};
    use std::time::Duration;

    fn snapshot(target: &catdesk_wake::protocol::Target) -> Value {
        let text = "private captured assistant response";
        json!({"messages":[{"role":"ASSISTANT","text":text,"visibleUnits":text.len(),
            "textComplete":true,"turnHash":null,"roleTurnHash":null,
            "messageHash":digest("capture-fixture-message"),"identityBasis":"MESSAGE_ID",
            "revisionHash":digest(&format!("catdesk-snapshot-revision-v1\0{}\0{}",target.digest,text)),
            "revisionComplete":true}],"truncated":false,
            "coverage":{"selectorTier":"AUTHOR_ROLE","visibleRoleCount":1,"countOverflow":false,
                "nestedCollapsed":0,"keyedCollapsed":0,"omittedBeforeWindow":0,"returned":1,
                "identityMissing":0,"identityUncertain":0,"revisionIncomplete":0,"textIncomplete":0,
                "anchorStability":"UNVERIFIED"}})
    }
    fn read(
        control: &Path,
        request: &crate::web_context_control::CaptureRequest,
        keep_running: &dyn Fn() -> Result<(), String>,
    ) -> Result<Value, String> {
        operator_facade::capture_web_context_with_snapshot(
            control,
            &request.project_id,
            (
                request.expected_target_generation,
                &request.expected_target_sha256,
            ),
            keep_running,
            |_, project, target, guard| {
                guard()?;
                assert_eq!(project, request.project_id);
                assert_eq!(target.generation, request.expected_target_generation);
                assert_eq!(target.digest, request.expected_target_sha256);
                Ok(snapshot(target))
            },
        )
    }

    let production_before = production_snapshot();
    let parent = fixture_parent();
    let control = parent.join("control");
    fs::create_dir(&control).unwrap();
    let (package, python) = package_and_python(&parent);
    development_runtime::prepare(&control, &package, &python).unwrap();
    let wake = Store::open(
        &development_runtime::root_for_workspace(&control)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let projects = [
        ("catdesk", control.clone()),
        ("alpha", parent.join("alpha")),
        ("beta", parent.join("beta")),
    ];
    for (project, workspace) in &projects {
        let origin = format!("https://example.test/{project}.git");
        git_root(workspace, &origin);
        web_action(
            &control,
            &[
                "operator",
                "web",
                "project",
                "register",
                "--project-id",
                project,
                "--workspace",
                workspace.to_str().unwrap(),
                "--git-identity",
                &origin,
            ],
        )
        .await;
        let url = format!("https://chatgpt.com/c/{}", uuid::Uuid::new_v4());
        crate::mcp::operator_bind_project_web_target(&control, project, &url, None, 0).unwrap();
    }
    let bindings = wake.config().unwrap().targets;
    let args = |project: &str| {
        json!({"projectId":project,"expectedTargetGeneration":1,
        "expectedTargetSha256":bindings[project].digest})
    };

    // Cancel an awaiting MCP operation while its blocking reader still owns the
    // slot. Other projects remain runnable; the cancelled project cannot overlap.
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let owned_control = control.clone();
    let input = args("alpha");
    let pending = tokio::spawn(async move {
        capture_with(&owned_control, input, move |control, _, guard| {
            assert!(
                crate::mcp::operator_read_project_web_target(control, "beta")
                    .unwrap()
                    .is_some()
            );
            let _ = started_tx.send(());
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let reason = loop {
                if let Err(reason) = guard() {
                    break reason;
                }
                if std::time::Instant::now() >= deadline {
                    panic!("cancellation not observed");
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            let _ = cancel_tx.send(reason.clone());
            let _ = release_rx.recv_timeout(Duration::from_secs(30));
            Err(reason)
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(15), started_rx)
        .await
        .unwrap()
        .unwrap();
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), cancel_rx)
            .await
            .unwrap()
            .unwrap(),
        "WEB_CONTEXT_CAPTURE_CANCELLED"
    );
    assert_eq!(
        capture_with(&control, args("alpha"), |_, _, _| panic!(
            "duplicate reader"
        ))
        .await
        .unwrap_err(),
        "WEB_CONTEXT_CAPTURE_BUSY"
    );
    for project in ["catdesk", "beta"] {
        let captured = capture_with(&control, args(project), read).await.unwrap();
        assert_eq!(captured["recordedMessages"], 1);
        assert_eq!(captured["observationSource"], "BOUND_BROWSER_SNAPSHOT");
        assert_eq!(captured["browserBindingVerified"], true);
        assert_eq!(captured["originAuthenticated"], false);
        assert_eq!(captured["userMessageSent"], false);
        assert_eq!(captured["hostStarted"], false);
        assert!(!captured.to_string().contains("private captured"));
    }
    release_tx.send(()).unwrap();
    // This includes the full registration/Git witness and first alpha ledger
    // write, which may take seconds while the full suite runs in parallel.
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match capture_with(&control, args("alpha"), read).await {
                Ok(_) => break,
                Err(reason) if reason == "WEB_CONTEXT_CAPTURE_BUSY" => {
                    tokio::time::sleep(Duration::from_millis(5)).await
                }
                Err(reason) => panic!("{reason}"),
            }
        }
    })
    .await
    .unwrap();

    let meter = WebContextStore::open(control.join(".catdesk/web-context")).unwrap();
    let mut before = meter.project_history("alpha").unwrap();
    let replay = capture_with(&control, args("alpha"), read).await.unwrap();
    assert_eq!(replay["recordedMessages"], 1);
    let refreshed = meter.project_history("alpha").unwrap();
    assert!(
        refreshed[0]
            .browser_coverage
            .last_observed_at_unix
            .is_some()
    );
    assert!(
        refreshed[0].browser_coverage.last_observed_at_unix
            >= before[0].browser_coverage.last_observed_at_unix
    );
    before[0].browser_coverage.last_observed_at_unix =
        refreshed[0].browser_coverage.last_observed_at_unix;
    assert_eq!(refreshed, before);
    // A completed fake browser read must not strand its capture lease while
    // waiting for a metadata writer. Its exact retry remains deduplicated.
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(control.join(".catdesk/web-context/ledger.lock"))
        .unwrap();
    held.lock().unwrap();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release = std::thread::spawn(move || {
        let _ = release_rx.recv_timeout(Duration::from_secs(30));
        drop(held);
    });
    let busy_capture = capture_with(&control, args("alpha"), read).await;
    let released_before_timeout = release_tx.send(()).is_ok();
    release.join().unwrap();
    assert!(
        released_before_timeout,
        "capture waited for metadata release timeout"
    );
    assert_eq!(busy_capture.unwrap_err(), "WEB_CONTEXT_BUSY");
    assert_eq!(meter.project_history("alpha").unwrap(), before);
    capture_with(&control, args("alpha"), read).await.unwrap();
    let refreshed = meter.project_history("alpha").unwrap();
    assert!(
        refreshed[0].browser_coverage.last_observed_at_unix
            >= before[0].browser_coverage.last_observed_at_unix
    );
    before[0].browser_coverage.last_observed_at_unix =
        refreshed[0].browser_coverage.last_observed_at_unix;
    assert_eq!(refreshed, before);
    let mut stale = args("alpha");
    stale["expectedTargetGeneration"] = json!(2);
    assert_eq!(
        capture_with(&control, stale, |_, _, _| panic!("stale reader called"))
            .await
            .unwrap_err(),
        "WEB_CONTEXT_TARGET_CHANGED"
    );

    // Rebind during a read must discard the old snapshot before ingestion.
    let beta_before = meter.project_history("beta").unwrap();
    assert_eq!(
        capture_with(&control, args("beta"), |control, request, guard| {
            operator_facade::capture_web_context_with_snapshot(
                control,
                &request.project_id,
                (
                    request.expected_target_generation,
                    &request.expected_target_sha256,
                ),
                guard,
                |_, project, target, _| {
                    crate::mcp::operator_bind_project_web_target(
                        control,
                        project,
                        &format!("https://chatgpt.com/c/{}", uuid::Uuid::new_v4()),
                        Some(&target.digest),
                        1,
                    )
                    .map_err(|_| "fixture rebind failed")?;
                    Ok(snapshot(target))
                },
            )
        })
        .await
        .unwrap_err(),
        "WEB_CONTEXT_TARGET_CHANGED"
    );
    assert_eq!(meter.project_history("beta").unwrap(), beta_before);
    assert_eq!(meter.project_history("alpha").unwrap(), before);

    // A rebind after MCP preflight but before the reader's own check must not
    // silently capture the new target under the caller's old witness.
    let catdesk_before = meter.project_history("catdesk").unwrap();
    assert_eq!(
        capture_with(&control, args("catdesk"), |control, request, guard| {
            crate::mcp::operator_bind_project_web_target(
                control,
                "catdesk",
                &format!("https://chatgpt.com/c/{}", uuid::Uuid::new_v4()),
                Some(&request.expected_target_sha256),
                1,
            )
            .map_err(|_| "fixture rebind failed")?;
            operator_facade::capture_web_context_with_snapshot(
                control,
                &request.project_id,
                (
                    request.expected_target_generation,
                    &request.expected_target_sha256,
                ),
                guard,
                |_, _, _, _| panic!("reader crossed target change"),
            )
        })
        .await
        .unwrap_err(),
        "WEB_CONTEXT_TARGET_CHANGED"
    );
    assert_eq!(meter.project_history("catdesk").unwrap(), catdesk_before);

    // Standalone selection resolves the actual replacement and passes its
    // exact witness to the shared bounded reader, preserving history.
    let current = crate::web_context_control::capture_current_with(&control, "catdesk", read)
        .await
        .unwrap();
    assert_eq!(current["invocationSource"], "STANDALONE_OPERATOR");
    assert_eq!(current["observationSource"], "BOUND_BROWSER_SNAPSHOT");
    assert_eq!(current["pressure"]["key"]["targetGeneration"], 2);
    assert_eq!(meter.project_history("catdesk").unwrap().len(), 2);
    let mut predecessor = catdesk_before[0].clone();
    predecessor.is_current = false;
    assert_eq!(meter.project_history("catdesk").unwrap()[0], predecessor);
    assert_eq!(
        crate::web_context_control::capture_current_with(&control, "../alpha", |_, _, _| panic!(
            "invalid current reader"
        ))
        .await
        .unwrap_err(),
        "WEB_CONTEXT_ARGUMENTS_INVALID"
    );
    assert_eq!(
        crate::web_context_control::capture_current_with(
            &control,
            "unregistered",
            |_, _, _| panic!("unregistered current reader")
        )
        .await
        .unwrap_err(),
        "WEB_CONTEXT_PROJECT_UNAVAILABLE"
    );

    // Failure/unwind also releases the worker's ownership without poisoning it.
    assert_eq!(
        capture_with(&control, args("alpha"), |_, _, _| panic!(
            "fixture worker unwind"
        ))
        .await
        .unwrap_err(),
        "WEB_CONTEXT_CAPTURE_WORKER_FAILED"
    );
    capture_with(&control, args("alpha"), read).await.unwrap();
    assert!(wake.events().unwrap().is_empty());
    let control_state: Value =
        serde_json::from_slice(&fs::read(wake.root().join("control.json")).unwrap()).unwrap();
    assert_eq!(control_state["desired"], "STOPPED");
    for (_, workspace) in &projects[1..] {
        assert!(!workspace.join(".catdesk").exists());
    }
    assert!(
        AutonomousProjectRegistryStoreV1::open_read_only(control.join(".catdesk/projects"))
            .unwrap()
            .load_registry()
            .unwrap()
            .projects
            .iter()
            .all(|p| p.codex_thread_id.is_none())
    );
    assert_eq!(production_snapshot(), production_before);
    cleanup(&parent);
}
