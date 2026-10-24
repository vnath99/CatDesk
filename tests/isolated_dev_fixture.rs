// Windows-only source-current disposable CatDesk fixture canary.
// This must never point at the installed daemon, production workspace,
// official Secure MCP tunnel, independent WakeHost or protected image.
#![cfg(windows)]

use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::timeout,
};

#[tokio::test]
async fn fixture_headless_deadline_exits_without_external_stop() {
    let root = std::env::temp_dir().join(format!(
        "catdesk-dev-fixture-deadline-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&root).unwrap();
    let config = root.join("config.toml");
    let child = Command::new(env!("CARGO_BIN_EXE_catdesk"))
        .args([
            "--headless-mcp",
            "--isolated-dev-fixture",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--workspace",
            root.to_str().unwrap(),
            "--config-path",
            config.to_str().unwrap(),
            "--mcp-path",
            "/dev/mcp",
            "--mode",
            "computer",
            "--tool-mode",
            "read-only",
            "--auth-token",
            "catdesk-test-deadline-token-123456",
            "--max-runtime-seconds",
            "1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let output = timeout(Duration::from_secs(15), child.wait_with_output())
        .await
        .expect("internal deadline must terminate this owner")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ready: Value = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value["status"] == "ready")
        .expect("ready before deadline");
    assert_eq!(ready["maximum_runtime_seconds"], 1);
    assert!(!config.exists());
    assert!(
        reqwest::Client::new()
            .get(ready["mcp_url"].as_str().unwrap())
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_err(),
        "listener must be gone"
    );
    std::fs::remove_dir(&root).unwrap();
}

#[tokio::test]
async fn fixture_headless_mcp_has_only_authenticated_local_read_and_search() {
    let root = std::env::temp_dir().join(format!(
        "catdesk-dev-fixture-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).expect("fixture root");
    std::fs::write(root.join("fixture.txt"), b"fixture-boundary-evidence").expect("fixture file");
    let config = root.join("config.toml");
    let binary = Path::new(env!("CARGO_BIN_EXE_catdesk"));
    let token = "catdesk-ephemeral-fixture-token-123456";
    let mut child = Command::new(binary)
        .args([
            "--headless-mcp",
            "--isolated-dev-fixture",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--workspace",
            root.to_str().expect("fixture path"),
            "--config-path",
            config.to_str().expect("config path"),
            "--mcp-path",
            "/dev/mcp",
            "--mode",
            "computer",
            "--tool-mode",
            "read-only",
            "--auth-token",
            token,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("launch isolated fixture");
    let stdout = child.stdout.take().expect("fixture stdout");
    let mut reader = BufReader::new(stdout).lines();
    let ready = timeout(Duration::from_secs(20), async {
        for _ in 0..16 {
            let line = reader
                .next_line()
                .await
                .expect("read fixture line")
                .expect("fixture exited before readiness");
            if let Ok(value) = serde_json::from_str::<Value>(&line)
                && value["status"] == "ready"
            {
                return value;
            }
        }
        panic!("fixture failed bounded readiness");
    })
    .await
    .expect("fixture readiness timeout");
    let mcp_url = ready["mcp_url"].as_str().expect("MCP URL");
    assert!(mcp_url.starts_with("http://127.0.0.1:"));
    assert!(mcp_url.ends_with("/dev/mcp"));
    assert!(
        !mcp_url.contains(":3200/"),
        "must never use production port"
    );
    assert!(
        !ready.to_string().contains(token),
        "token must not be logged"
    );
    assert_eq!(ready["tool_mode"], "read-only");
    // The fixture must never initialize a pathname-backed configuration.
    assert!(!config.exists(), "fixture must use in-memory config only");

    // Running fixture pins its accepted directory without delete sharing.
    // Prove that a second same-user process cannot rename that directory
    // and replace the path served by later file-tool requests.
    let moved_root = root.with_file_name(format!(
        "catdesk-dev-fixture-moved-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let attempted_move = std::fs::rename(&root, &moved_root);
    if attempted_move.is_ok() {
        // Restore before failing, to avoid leaving test-owned fixture paths.
        std::fs::rename(&moved_root, &root).expect("restore fixture after unexpected rename");
    }
    assert!(
        attempted_move.is_err(),
        "running fixture root must resist rename"
    );

    let client = reqwest::Client::new();
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let mut response = None;
    for _ in 0..8 {
        if let Ok(r) = client
            .post(mcp_url)
            .bearer_auth(token)
            .json(&list)
            .send()
            .await
        {
            response = Some(r);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let response = response.expect("bound fixture responds");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let list_result: Value = response.json().await.expect("tool listing");
    let tools = list_result["result"]["tools"]
        .as_array()
        .expect("tools array");
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["read", "search"]);

    let denied = json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"catdesk_binagotchy_command","arguments":{"command":"test"}}});
    let result: Value = client
        .post(mcp_url)
        .bearer_auth(token)
        .json(&denied)
        .send()
        .await
        .expect("denial response")
        .json()
        .await
        .expect("denial json");
    assert_eq!(result["error"]["code"], -32602);
    let read = json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"read","arguments":{"path":"fixture.txt"}}});
    let result: Value = client
        .post(mcp_url)
        .bearer_auth(token)
        .json(&read)
        .send()
        .await
        .expect("read response")
        .json()
        .await
        .expect("read json");
    assert!(
        result["result"]["structuredContent"]["text"]
            .as_str()
            .is_some_and(|s| s.contains("fixture-boundary-evidence"))
    );

    let memory_search = json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
        "params":{"name":"search","arguments":{"path":"fixture.txt","pattern":"fixture-boundary"}}});
    let searched: Value = client
        .post(mcp_url)
        .bearer_auth(token)
        .json(&memory_search)
        .send()
        .await
        .expect("search response")
        .json()
        .await
        .expect("search JSON");
    assert_eq!(
        searched["result"]["structuredContent"]["searchBackend"],
        "isolated-memory"
    );
    assert_eq!(searched["result"]["structuredContent"]["matchCount"], 1);
    let config_read = json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
        "params":{"name":"read","arguments":{"path":"config.toml"}}});
    let config_denied: Value = client
        .post(mcp_url)
        .bearer_auth(token)
        .json(&config_read)
        .send()
        .await
        .expect("config denied")
        .json()
        .await
        .expect("config refusal");
    assert_eq!(config_denied["result"]["isError"], true);

    let outside = root.with_file_name(format!("fixture-outside-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&outside, "outside proof").expect("outside data");
    std::fs::hard_link(&outside, root.join("linked.txt")).expect("hardlink fixture");
    for name in ["read", "search"] {
        let request = json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
            "params":{"name":name,"arguments":{"path":"linked.txt","pattern":"outside"}}});
        let denied: Value = client
            .post(mcp_url)
            .bearer_auth(token)
            .json(&request)
            .send()
            .await
            .expect("hardlink denial")
            .json()
            .await
            .expect("refusal JSON");
        assert_eq!(denied["result"]["isError"], true, "{name}");
    }
    std::fs::remove_file(outside).expect("remove only own external test file");

    let unauthenticated = client
        .post(mcp_url)
        .json(&list)
        .send()
        .await
        .expect("401 response");
    assert_eq!(unauthenticated.status(), reqwest::StatusCode::UNAUTHORIZED);
    let origin = mcp_url.strip_suffix("/dev/mcp").expect("fixed suffix");
    let normal_route = client
        .post(format!("{origin}/agents/path-mode"))
        .bearer_auth(token)
        .json(&json!({}))
        .send()
        .await
        .expect("normal route probe");
    assert_eq!(normal_route.status(), reqwest::StatusCode::NOT_FOUND);

    child.kill().await.expect("stop own fixture process");
    let _ = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("bounded exit");
    assert!(!config.exists(), "fixture never persisted configuration");
    // A stopped child must relinquish its root lease so only this test-owned
    // fixture can be moved and cleaned up. Never touch another process.
    let after_stop = root.with_file_name(format!(
        "catdesk-dev-fixture-released-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::rename(&root, &after_stop).expect("fixture root lease released on child exit");
    std::fs::remove_dir_all(after_stop).expect("clean up own fixture");
}
