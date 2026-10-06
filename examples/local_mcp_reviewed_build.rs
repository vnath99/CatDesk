use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let action = args.next().ok_or("missing action")?;
    let second = args.next();
    let mut tool_args = serde_json::Map::new();
    tool_args.insert("action".into(), Value::String(action.clone()));
    match action.as_str() {
        "PREFLIGHT" => {
            tool_args.insert(
                "recordId".into(),
                Value::String(second.ok_or("missing record")?),
            );
        }
        "CONFIRM" => {
            tool_args.insert(
                "confirmationToken".into(),
                Value::String(second.ok_or("missing token")?),
            );
        }
        "RESULT" => {}
        _ => return Err("unsupported action".into()),
    }

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
        "jsonrpc":"2.0",
        "id":"reviewed-build-loopback",
        "method":"tools/call",
        "params":{
            "name":"catdesk_reviewed_build",
            "arguments": Value::Object(tool_args)
        }
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
    if let Some(error) = body.get("error") {
        println!("RPC_ERROR={}", serde_json::to_string(error)?);
    } else if let Some(result) = body.get("result") {
        println!("RPC_RESULT={}", serde_json::to_string(result)?);
    } else {
        println!("RPC_RESPONSE_UNEXPECTED");
    }
    Ok(())
}
