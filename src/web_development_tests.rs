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
    assert!(!names.contains(&"run_command"));

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
