use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn user_home() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| "user home is unavailable".to_string())
}

fn local_route() -> Result<String, String> {
    let config_path = user_home()?.join(".catdesk").join("config.toml");
    let text =
        fs::read_to_string(config_path).map_err(|_| "CatDesk config is unavailable".to_string())?;
    let parsed: toml::Value =
        toml::from_str(&text).map_err(|_| "CatDesk config is invalid".to_string())?;
    let route = parsed
        .get("mcp")
        .and_then(|mcp| mcp.get("route_id"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "CatDesk local MCP route is unavailable".to_string())?;
    if route.is_empty() || route.chars().any(|ch| matches!(ch, '/' | '\\' | '?' | '#')) {
        return Err("CatDesk local MCP route is invalid".into());
    }
    Ok(route.to_string())
}

fn post_json(path: &str, body: &Value) -> Result<Value, String> {
    let encoded =
        serde_json::to_vec(body).map_err(|_| "request serialization failed".to_string())?;
    let mut stream = TcpStream::connect(("127.0.0.1", 3200))
        .map_err(|_| "local CatDesk MCP is unavailable".to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|_| "could not set MCP read timeout".to_string())?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:3200\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        encoded.len()
    )
    .map_err(|_| "MCP request header write failed".to_string())?;
    stream
        .write_all(&encoded)
        .map_err(|_| "MCP request body write failed".to_string())?;
    stream
        .flush()
        .map_err(|_| "MCP request flush failed".to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|_| "MCP response read failed".to_string())?;
    let text = String::from_utf8(response).map_err(|_| "MCP response was not UTF-8".to_string())?;
    let (_, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "MCP response was malformed".to_string())?;
    serde_json::from_str(body).map_err(|_| "MCP response JSON was malformed".to_string())
}

fn call(path: &str, id: &str, args: Value) -> Result<Value, String> {
    let response = post_json(
        path,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": "catdesk_daemon_reload", "arguments": args}
        }),
    )?;
    if response.get("error").is_some() {
        return Err("MCP reload tool returned a JSON-RPC error".into());
    }
    let result = response
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| "MCP reload tool returned no result".to_string())?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err("MCP reload tool reported a bounded error".into());
    }
    if let Some(structured) = result.get("structuredContent") {
        return Ok(structured.clone());
    }
    Err("MCP reload tool returned no structured content".into())
}

fn write_safe_result(workspace: &Path, value: Value) {
    let target = workspace
        .join(".catdesk")
        .join("t0048-native-reload-acceptance.json");
    if let Some(parent) = target.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(
        target,
        serde_json::to_vec_pretty(&value).unwrap_or_default(),
    );
}

fn main() -> Result<(), String> {
    let workspace = std::env::current_dir()
        .map_err(|_| "workspace is unavailable".to_string())?
        .canonicalize()
        .map_err(|_| "workspace canonicalization failed".to_string())?;
    let build = workspace
        .join(".catdesk")
        .join("t0048-native-reload-proof")
        .join("debug")
        .join("catdesk.exe");
    if !build.is_file() {
        return Err("proof build is unavailable".into());
    }
    let relative = build
        .strip_prefix(&workspace)
        .map_err(|_| "proof build escaped workspace".to_string())?
        .to_string_lossy()
        .into_owned();
    let route = local_route()?;
    let mcp_path = format!("/{route}/mcp");

    let dry = call(
        &mcp_path,
        "t0048-native-dry-run",
        json!({"buildPath": relative, "dryRun": true}),
    )?;
    let sha = dry
        .get("expectedSha256")
        .and_then(Value::as_str)
        .filter(|value| value.len() == 64)
        .ok_or_else(|| "dry-run did not return a SHA-256".to_string())?
        .to_string();
    let token = dry
        .get("confirmationToken")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "dry-run did not return a confirmation token".to_string())?
        .to_string();

    let execute = call(
        &mcp_path,
        "t0048-native-execute",
        json!({
            "buildPath": relative,
            "expectedSha256": sha,
            "dryRun": false,
            "confirmationToken": token
        }),
    )?;
    let accepted = execute.get("accepted").and_then(Value::as_bool) == Some(true)
        && execute.get("handoff").and_then(Value::as_str) == Some("native-detached-helper-started");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    write_safe_result(
        &workspace,
        json!({
            "schemaVersion": 1,
            "dryRunAccepted": dry.get("dryRun").and_then(Value::as_bool) == Some(true),
            "executeAccepted": accepted,
            "handoff": execute.get("handoff").and_then(Value::as_str),
            "tunnelAction": execute.get("tunnelAction").and_then(Value::as_str),
            "mcpPort": execute.get("mcpPort").and_then(Value::as_u64),
            "recordedAtUnix": now
        }),
    );
    if !accepted {
        return Err("native reload execute was not accepted".into());
    }
    println!("T-0048_NATIVE_RELOAD_EXECUTE_ACCEPTED");
    Ok(())
}
