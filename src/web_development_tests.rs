//! A local development-root routing fixture. It never starts Wake or a browser.

use crate::{
    delegated::autonomy_projects::AutonomousProjectRegistryStoreV1,
    operator_facade, wake_protocol_client,
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
