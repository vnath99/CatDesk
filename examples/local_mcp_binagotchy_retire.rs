use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("CATDESK_MCP_AUTH_TOKEN")?;
    if token.len() < 24 || token.chars().any(char::is_whitespace) {
        return Err("invalid inherited MCP auth token".into());
    }
    let config_path = PathBuf::from(env::var("USERPROFILE")?)
        .join(".catdesk")
        .join("config.toml");
    let config_text = fs::read_to_string(config_path)?;
    let config: toml::Value = toml::from_str(&config_text)?;
    let mcp = config
        .get("mcp")
        .and_then(toml::Value::as_table)
        .ok_or("missing [mcp]")?;
    let port = mcp
        .get("port")
        .and_then(toml::Value::as_integer)
        .ok_or("missing port")?;
    let route = mcp
        .get("route_id")
        .and_then(toml::Value::as_str)
        .ok_or("missing route")?;
    let request = json!({
        "jsonrpc":"2.0","id":"binagotchy-retire","method":"tools/call",
        "params":{"name":"catdesk_binagotchy_command","arguments":{
            "command":"retire",
            "eventId":"review-adc-t0396-r1-isolated-bootstrap-rebuild-20260921-6-independent_final_review"
        }}
    });
    let body: Value = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/{route}/mcp"))
        .bearer_auth(token)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await?
        .json()
        .await?;
    if let Some(result) = body.get("result") {
        println!("{}", serde_json::to_string(result)?);
    } else if let Some(error) = body.get("error") {
        println!("{}", serde_json::to_string(error)?);
    }
    Ok(())
}
