import json
import os
import sys
import time
import tomllib
import urllib.request
from pathlib import Path

WORKSPACE = Path.cwd().resolve()
CONFIG = Path(os.environ.get("USERPROFILE", str(Path.home()))) / ".catdesk" / "config.toml"
BUILD = WORKSPACE / ".catdesk" / "t0048-native-reload-proof" / "debug" / "catdesk.exe"
RESULT = WORKSPACE / ".catdesk" / "t0048-native-reload-acceptance.json"


def rpc(url: str, request_id: str, name: str, arguments: dict) -> dict:
    body = json.dumps({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments},
    }).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(req, timeout=10) as response:
        return json.loads(response.read().decode("utf-8"))


def structured_result(response: dict) -> dict:
    result = response.get("result")
    if not isinstance(result, dict):
        raise RuntimeError("MCP response did not contain a structured result")
    if result.get("isError"):
        raise RuntimeError("MCP tool reported an error")
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        return structured
    content = result.get("content")
    if isinstance(content, list):
        for item in content:
            if isinstance(item, dict) and item.get("type") == "text":
                try:
                    parsed = json.loads(item.get("text", ""))
                except json.JSONDecodeError:
                    continue
                if isinstance(parsed, dict):
                    return parsed
    raise RuntimeError("MCP tool result did not expose structured content")


def main() -> int:
    if not CONFIG.is_file():
        raise RuntimeError("CatDesk config is unavailable")
    if not BUILD.is_file():
        raise RuntimeError("proof build is unavailable")
    with CONFIG.open("rb") as handle:
        config = tomllib.load(handle)
    route = config.get("mcp", {}).get("route_id")
    if not isinstance(route, str) or not route or any(ch in route for ch in "/\\?#"):
        raise RuntimeError("CatDesk local MCP route is unavailable or invalid")
    url = f"http://127.0.0.1:3200/{route}/mcp"
    relative_build = str(BUILD.relative_to(WORKSPACE))

    dry = structured_result(rpc(url, "t0048-dry-run", "catdesk_daemon_reload", {
        "buildPath": relative_build,
        "dryRun": True,
    }))
    sha = dry.get("expectedSha256")
    token = dry.get("confirmationToken")
    if dry.get("dryRun") is not True or not isinstance(sha, str) or len(sha) != 64 or not isinstance(token, str) or not token:
        raise RuntimeError("reload dry-run did not return the required bounded confirmation")

    execute = structured_result(rpc(url, "t0048-execute", "catdesk_daemon_reload", {
        "buildPath": relative_build,
        "expectedSha256": sha,
        "dryRun": False,
        "confirmationToken": token,
    }))
    accepted = execute.get("accepted") is True and execute.get("handoff") == "native-detached-helper-started"
    safe_record = {
        "schemaVersion": 1,
        "dryRunAccepted": dry.get("dryRun") is True,
        "executeAccepted": accepted,
        "handoff": execute.get("handoff"),
        "tunnelAction": execute.get("tunnelAction"),
        "mcpPort": execute.get("mcpPort"),
        "recordedAtUnix": int(time.time()),
    }
    RESULT.write_text(json.dumps(safe_record, indent=2), encoding="utf-8")
    print("T-0048_NATIVE_RELOAD_EXECUTE_ACCEPTED" if accepted else "T-0048_NATIVE_RELOAD_EXECUTE_NOT_ACCEPTED")
    return 0 if accepted else 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        RESULT.parent.mkdir(parents=True, exist_ok=True)
        RESULT.write_text(json.dumps({"schemaVersion": 1, "executeAccepted": False, "error": str(exc)[:240]}, indent=2), encoding="utf-8")
        print("T-0048_NATIVE_RELOAD_CLIENT_FAILED")
        raise
