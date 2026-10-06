use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};

#[tokio::test]
#[ignore = "live acceptance: requires inherited MCP credentials and restarts installed WakeHost"]
async fn bounded_local_control_probe() {
    let token = env::var("CATDESK_MCP_AUTH_TOKEN").expect("inherited MCP token");
    assert!(token.len() >= 24 && !token.chars().any(char::is_whitespace));

    let config_path = PathBuf::from(env::var("USERPROFILE").expect("USERPROFILE"))
        .join(".catdesk")
        .join("config.toml");
    let config_text = fs::read_to_string(config_path).expect("config");
    let config: toml::Value = toml::from_str(&config_text).expect("toml");
    let mcp = config
        .get("mcp")
        .and_then(toml::Value::as_table)
        .expect("[mcp]");
    let port = mcp
        .get("port")
        .and_then(toml::Value::as_integer)
        .expect("mcp.port");
    let route = mcp
        .get("route_id")
        .and_then(toml::Value::as_str)
        .expect("mcp.route_id");
    assert!((1..=65535).contains(&port));
    assert!(
        !route.is_empty()
            && route
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    );

    let request = json!({
        "jsonrpc": "2.0",
        "id": "wake-control-test",
        "method": "tools/call",
        "params": {
            "name": "catdesk_wake_restart_installed",
            "arguments": { "confirm": true }
        }
    });
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/{route}/mcp"))
        .bearer_auth(token)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .expect("local MCP response");
    assert!(response.status().is_success(), "HTTP {}", response.status());
    let body: Value = response.json().await.expect("json");
    assert!(body.get("error").is_none(), "MCP error: {body}");
    let text = body
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        text.contains("restarted") || text.contains("WakeHost"),
        "unexpected MCP result: {body}"
    );
}
