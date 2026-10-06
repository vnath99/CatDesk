use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("CATDESK_MCP_AUTH_TOKEN")?;
    if token.len() < 24 || token.chars().any(char::is_whitespace) {
        return Err("invalid inherited MCP auth token".into());
    }

    let user_profile = env::var("USERPROFILE")?;
    let config_path = PathBuf::from(user_profile)
        .join(".catdesk")
        .join("config.toml");
    let config_text = fs::read_to_string(config_path)?;
    let config: toml::Value = toml::from_str(&config_text)?;
    let mcp = config
        .get("mcp")
        .and_then(toml::Value::as_table)
        .ok_or("missing [mcp] config")?;
    let port = mcp
        .get("port")
        .and_then(toml::Value::as_integer)
        .ok_or("missing mcp.port")?;
    let route = mcp
        .get("route_id")
        .and_then(toml::Value::as_str)
        .ok_or("missing mcp.route_id")?;
    if !(1..=65535).contains(&port)
        || route.is_empty()
        || !route
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("invalid local MCP endpoint metadata".into());
    }

    let url = format!("http://127.0.0.1:{port}/{route}/mcp");
    let request = json!({
        "jsonrpc": "2.0",
        "id": "wake-control",
        "method": "tools/call",
        "params": {
            "name": "catdesk_wake_restart_installed",
            "arguments": { "confirm": true }
        }
    });
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await?;
    println!("HTTP={status}");
    if let Some(result) = body.get("result") {
        println!("{}", serde_json::to_string(result)?);
    } else if let Some(error) = body.get("error") {
        println!("{}", serde_json::to_string(error)?);
    } else {
        println!("UNEXPECTED_RESPONSE");
    }
    Ok(())
}
