import json
import os
import pathlib
import queue
import subprocess
import threading
import time

WORKSPACE = pathlib.Path(__file__).resolve().parents[1]
TARGET_TITLE = "integrate codex mcp for chatgpt"
MAX_LINE = 64 * 1024

exe = os.environ.get("CATDESK_CODEX_CLI_EXECUTABLE")
if not exe:
    raise SystemExit("CATDESK_CODEX_CLI_EXECUTABLE is not present in the CatDesk host environment")

exe_path = pathlib.Path(exe)
if not exe_path.is_file() or exe_path.suffix.lower() in {".ps1", ".cmd", ".bat"}:
    raise SystemExit("CatDesk host Codex executable is not a direct native executable")

env = os.environ.copy()
for key in ("OPENAI_API_KEY", "CODEX_API_KEY", "CONTROL_PLANE_API_KEY"):
    env.pop(key, None)
# Deliberately do not set CODEX_HOME: probe the same normal current-user context
# as the T-0046B acceptance daemon.
env.pop("CODEX_HOME", None)

proc = subprocess.Popen(
    [str(exe_path), "app-server", "--stdio"],
    cwd=str(WORKSPACE),
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    text=False,
    env=env,
)

out_q = queue.Queue()

def reader():
    try:
        while True:
            line = proc.stdout.readline()
            if not line:
                out_q.put(None)
                return
            out_q.put(line)
    except BaseException as exc:
        out_q.put(exc)

threading.Thread(target=reader, daemon=True).start()

next_id = 1

def request(method, params, timeout=15):
    global next_id
    rid = next_id
    next_id += 1
    payload = json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params}, separators=(",", ":")).encode() + b"\n"
    proc.stdin.write(payload)
    proc.stdin.flush()
    deadline = time.time() + timeout
    while time.time() < deadline:
        remaining = max(0.1, deadline - time.time())
        try:
            item = out_q.get(timeout=remaining)
        except queue.Empty:
            break
        if item is None:
            raise RuntimeError("app-server closed stdout")
        if isinstance(item, BaseException):
            raise item
        if len(item) > MAX_LINE:
            raise RuntimeError("app-server response exceeded bounded line limit")
        msg = json.loads(item)
        if msg.get("id") != rid:
            if "method" in msg and "id" not in msg:
                continue
            raise RuntimeError("unexpected JSON-RPC response id")
        if "error" in msg:
            raise RuntimeError("bounded RPC error: " + json.dumps(msg["error"], separators=(",", ":"))[:512])
        return msg.get("result", msg)
    raise TimeoutError(f"timeout waiting for {method}")


def norm(p):
    return str(p).strip().replace("\\", "/").rstrip("/").lower()

try:
    init = request("initialize", {"clientInfo": {"name": "catdesk-host-probe", "version": "1"}, "capabilities": {}})
    listed = request("thread/list", {})
    raw_items = listed.get("threads") or listed.get("items") or listed.get("data") or []
    exact = []
    for item in raw_items[:128]:
        cwd = item.get("cwd")
        if not isinstance(cwd, str) or norm(cwd) != norm(WORKSPACE):
            continue
        title = item.get("title") or item.get("name")
        preview = item.get("preview")
        text = " ".join(x for x in (title, preview) if isinstance(x, str)).lower()
        exact.append({
            "id": str(item.get("id") or item.get("threadId") or "")[:256],
            "title": str(title)[:256] if title is not None else None,
            "matchesPreferredTitle": TARGET_TITLE in text,
            "canAcceptDirectInput": item.get("canAcceptDirectInput"),
            "concurrentlyOwned": bool(item.get("concurrentlyOwned") or item.get("activeWriter")) or (item.get("canAcceptDirectInput") is False),
            "listModel": item.get("model") or item.get("selectedModel"),
            "listReasoningEffort": item.get("reasoningEffort"),
        })

    # Read exact workspace candidates individually for authoritative bounded model/reasoning.
    for item in exact:
        if not item["id"]:
            continue
        try:
            detail = request("thread/read", {"threadId": item["id"]})
            item["readModel"] = detail.get("model") or detail.get("selectedModel")
            item["readReasoningEffort"] = detail.get("reasoningEffort")
            item["readCanAcceptDirectInput"] = detail.get("canAcceptDirectInput")
        except Exception as exc:
            item["threadReadError"] = str(exc)[:256]

    rates = request("account/rateLimits/read", {})
    safe_rates = {
        "planType": rates.get("planType"),
        "reachedLimit": rates.get("reachedLimit"),
        "rateLimitCount": len(rates.get("rateLimits") or rates.get("windows") or []),
    }

    result = {
        "initializeSucceeded": True,
        "threadListCount": len(raw_items),
        "nextCursorPresent": bool(listed.get("nextCursor")),
        "exactWorkspaceCandidateCount": len(exact),
        "preferredTitleCandidateCount": sum(1 for x in exact if x["matchesPreferredTitle"]),
        "candidates": exact,
        "account": safe_rates,
    }
    print(json.dumps(result, indent=2))
finally:
    try:
        proc.kill()
    except Exception:
        pass
    try:
        proc.wait(timeout=3)
    except Exception:
        pass
