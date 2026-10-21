#!/usr/bin/env python3
"""One-shot, exact-record CatDesk -> ChatGPT CDP wake adapter."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import threading
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urlparse

SAFE_ID = re.compile(r"^[A-Za-z0-9_-]{1,200}$")
CONVERSATION_ID = re.compile(r"^[A-Za-z0-9_-]{1,200}$")
SHA256_HEX = re.compile(r"^[0-9a-f]{64}$")
MAX_DELIVERIES = 128
MAX_BRIDGE_BYTES = 256 * 1024
MAX_OWNER_SELECTOR_BYTES = 1024
MAX_USER_MESSAGES = 512
MAX_USER_MESSAGE_CHARS = 16 * 1024
NEW_CONVERSATION_HOME = "https://chatgpt.com/"
EDITOR_SELECTORS = (
    "#prompt-textarea",
    "div.ProseMirror[contenteditable='true']",
    "[contenteditable='true'][data-lexical-editor='true']",
)
EDITOR = EDITOR_SELECTORS[0]
SEND = ("button[data-testid='send-button']", "button[aria-label='Send prompt']", "button[aria-label='Send message']", "button#composer-submit-button", "button[type='submit'][aria-label='Send']")
USER_MESSAGE = "[data-message-author-role='user'], article[data-turn='user']"
USER_MESSAGE_CONTENT = "[data-message-content], .whitespace-pre-wrap"
STOP = ("button[data-testid='stop-button']", "button[aria-label='Stop answering']", "button[aria-label='Stop generating']", "button[aria-label='Stop streaming']", "button[aria-label='Stop']")
LOGIN = ("a[href*='/auth/login']", "button[data-testid*='login' i]")
CAPTCHA = (
    "iframe[title*='captcha' i]", "[data-testid*='captcha' i]",
    "iframe[title*='security' i]", "[data-testid*='security' i]",
    "[aria-label*='security verification' i]",
)
NETWORK = ("#main-frame-error", "#error-code")
NETWORK_ERROR_CODES = frozenset({
    "ERR_NETWORK_CHANGED", "ERR_INTERNET_DISCONNECTED", "ERR_NAME_NOT_RESOLVED",
    "ERR_CONNECTION_RESET", "ERR_CONNECTION_CLOSED", "ERR_CONNECTION_REFUSED",
    "ERR_CONNECTION_TIMED_OUT", "ERR_TIMED_OUT", "ERR_TUNNEL_CONNECTION_FAILED",
    "ERR_PROXY_CONNECTION_FAILED", "ERR_HTTP2_PROTOCOL_ERROR", "ERR_QUIC_PROTOCOL_ERROR",
    "ERR_SSL_PROTOCOL_ERROR", "ERR_CERT_AUTHORITY_INVALID", "ERR_BLOCKED_BY_CLIENT",
})
READINESS_ATTEMPTS = 3
READINESS_WINDOW_SECONDS = 30.0
READINESS_POLL_SECONDS = .25
POST_LOAD_STABLE_SECONDS = 10.0
HOME_RECOVERY_SETTLE_SECONDS = 10.0
BLANK_SHELL_RELOAD_SECONDS = 10.0
POST_STOP_HOLD_SECONDS = 2.0
POST_SUBMIT_RECEIPT_WINDOW_SECONDS = 60.0
RESPONSE_TIMEOUT_TEXT = "Message delivery timed out. Please try again."
RESPONSE_RETRY_LABEL = "Retry"
RESPONSE_POLL_SECONDS = 0.5
RESPONSE_HEARTBEAT_SECONDS = 10.0
RESPONSE_ACTIVE_REFRESH_SECONDS = 2.0 * 60.0
RESPONSE_STABLE_POLLS = 2
RESPONSE_RETRY_START_WINDOW_SECONDS = 15.0
RESPONSE_GENERATION_WINDOW_SECONDS = 35.0 * 60.0
RESPONSE_TOTAL_WINDOW_SECONDS = 90.0 * 60.0
RESPONSE_MAX_RETRIES = 3
POST_SUBMIT_DIAGNOSTICS = frozenset({
    "SUBMIT_CLICK_UNKNOWN",
    "SUBMIT_ENTER_UNKNOWN",
    "SUBMIT_COMPOSER_RETAINED",
    "SUBMIT_COMPOSER_CHANGED",
    "SUBMIT_ACCEPTANCE_UNPROVEN",
    "SUBMIT_TARGET_DRIFT",
    "SUBMIT_TARGET_DRIFT_CHROME_ERROR",
    "SUBMIT_TARGET_DRIFT_OTHER_HOST",
    "SUBMIT_TARGET_DRIFT_USERINFO",
    "SUBMIT_TARGET_DRIFT_PORT",
    "SUBMIT_TARGET_DRIFT_AUTH",
    "SUBMIT_TARGET_DRIFT_HOME",
    "SUBMIT_TARGET_DRIFT_DIFFERENT_CONVERSATION",
    "SUBMIT_TARGET_DRIFT_SAME_QUERY_FRAGMENT",
    "SUBMIT_TARGET_DRIFT_SAME_QUERY",
    "SUBMIT_TARGET_DRIFT_SAME_FRAGMENT",
    "SUBMIT_TARGET_DRIFT_SAME_TRAILING_SLASH",
    "SUBMIT_TARGET_DRIFT_SAME_ROUTE_OTHER",
    "SUBMIT_TARGET_DRIFT_PROJECT_OTHER",
    "SUBMIT_TARGET_DRIFT_OTHER_ROUTE",
    "SUBMIT_TARGET_DRIFT_INVALID_ROUTE",
    "SUBMIT_CLEARED_NO_APPEND",
    "SUBMIT_APPEND_DIGEST_MISMATCH",
    "SUBMIT_RECEIPT_SEQUENCE_DRIFT",
    "SUBMIT_RECEIPT_QUERY_FAILED",
    "SUBMIT_RECEIPT_ROUND_TRIP_FAILED",
    "SUBMIT_RECEIPT_UNPROVEN",
    "RESPONSE_STATE_QUERY_FAILED",
    "RESPONSE_RETRY_CLICK_FAILED",
    "RESPONSE_RETRY_CONTROL_AMBIGUOUS",
    "RESPONSE_REOPEN_UNAVAILABLE",
    "RESPONSE_REOPEN_FAILED",
    "RESPONSE_SEQUENCE_QUERY_FAILED",
    "RESPONSE_SEQUENCE_DRIFT",
    "RESPONSE_TARGET_DRIFT",
    "RESPONSE_NETWORK_ERROR",
    "RESPONSE_GENERATION_TIMEOUT",
    "RESPONSE_COMPLETION_UNPROVEN",
    "RESPONSE_TIMEOUT_RETRY_AMBIGUOUS",
    "RESPONSE_RETRY_EXHAUSTED",
    "RESPONSE_TIMEOUT_STATE_LOST",
    "RESPONSE_RETRY_NOT_STARTED",
    "RESPONSE_TOTAL_TIMEOUT",
})
MESSAGE = ("CatDesk review record {record_id}: This is an automated CatDesk wake event. "
           "Acknowledge or claim this triggering CatDesk review event before proceeding. Take stock of the current project status and where you last left off. Ensure the previous task and review bundle have been completed and reviewed. Continue working through the broad implementation phases of the project by dividing them into appropriately bounded tickets/tasks. For the next ticket, create a detailed technical design and implementation plan for Codex, send those instructions through CatDesk, and request a review bundle so the implementation can be verified. If the previous work is incomplete or issues remain, continue the fix/review cycle until major bugs are no longer an issue. Do not ask the user to relay prompts between agents. Involve the user only for genuine operator-only decisions.")
_guard = threading.Lock(); _held: set[str] = set()

class Attention(RuntimeError): pass
class PostSubmitUnknown(Attention): pass
class Busy(RuntimeError): pass

@dataclass
class Delivery:
    record_id: str; status: str; claimed_at_unix: float; browser_sent_at_unix: float | None = None; message_sha256: str | None = None; target_sha256: str | None = None; receipt_schema_version: int | None = None; attention: str | None = None
@dataclass
class State:
    schema_version: int = 4; deliveries: list[Delivery] = field(default_factory=list); operator_attention: str | None = None
@dataclass(frozen=True)
class DeliveryReceipt:
    record_id: str; browser_sent_at_unix: float; message_sha256: str; target_sha256: str; receipt_schema_version: int = 1

def normalize_message(value: str) -> str:
    return " ".join(value.split())

def keyboard_safe_message(value: str) -> str:
    """Keep message text semantically identical without CDP Enter/Tab key events.

    CDP text entry treats physical newlines as Enter on ChatGPT's composer.
    Preserve ordinary spaces (including JSON string content) while replacing
    only control whitespace with spaces. Receipt normalization must agree.
    """
    if not isinstance(value, str) or not value:
        raise Attention("MESSAGE_CONTROL_UNSUPPORTED")
    safe = "".join(" " if char in "\t\n\v\f\r\x85\u2028\u2029" else char for char in value)
    if (any(ord(char) < 32 or ord(char) == 127 or 0xE000 <= ord(char) <= 0xE05D
            for char in safe) or
            normalize_message(safe) != normalize_message(value)):
        raise Attention("MESSAGE_CONTROL_UNSUPPORTED")
    return safe
def digest(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()

def compose_bootstrap_message(project_id: str, intent_id: str, source_url: str, handoff: str) -> str:
    """Build the exact message Rust must hash before authorizing one submit."""
    if (not all(isinstance(value, str) and SAFE_ID.fullmatch(value)
                for value in (project_id, intent_id)) or
            not isinstance(handoff, str) or not handoff.strip()):
        raise Attention("BOOTSTRAP_REQUEST_INVALID")
    canonical = canonical_conversation_url(source_url)
    if canonical != source_url:
        raise Attention("BOOTSTRAP_SOURCE_INVALID")
    message = (
        "CatDesk project rollover bootstrap.\n"
        f"Project: {project_id}\n"
        f"Intent: {intent_id}\n"
        f"Predecessor: {canonical}\n\n"
        f"{handoff}"
    )
    if len(message) > MAX_USER_MESSAGE_CHARS or len(message.encode("utf-8")) > MAX_USER_MESSAGE_CHARS:
        raise Attention("BOOTSTRAP_MESSAGE_TOO_LONG")
    return message

def bridge_sha256(path: Path) -> str:
    try:
        if not path.is_file() or path.stat().st_size > MAX_BRIDGE_BYTES:
            raise OSError("bridge is not a bounded regular file")
        return hashlib.sha256(path.read_bytes()).hexdigest()
    except OSError as error:
        raise Attention("UNTRUSTED_BRIDGE_REVISION") from error

def canonical_conversation_url(value: str) -> str:
    if not isinstance(value, str) or not value or len(value) > 512 or not value.isascii() or any(char.isspace() or ord(char) < 32 for char in value):
        raise Attention("INVALID_DURABLE_STATE")
    try:
        parsed = urlparse(value)
        port = parsed.port
    except ValueError as error:
        raise Attention("INVALID_DURABLE_STATE") from error
    if parsed.scheme != "https" or parsed.hostname not in {"chatgpt.com", "chat.openai.com"} or parsed.username or parsed.password or port is not None or parsed.query or parsed.fragment:
        raise Attention("INVALID_DURABLE_STATE")
    parts = parsed.path.split("/")[1:]
    if len(parts) == 2 and parts[0] == "c" and CONVERSATION_ID.fullmatch(parts[1]):
        return f"https://{parsed.hostname}/c/{parts[1]}"
    if len(parts) == 4 and parts[0] == "g" and parts[2] == "c" and CONVERSATION_ID.fullmatch(parts[1]) and CONVERSATION_ID.fullmatch(parts[3]):
        return f"https://{parsed.hostname}/g/{parts[1]}/c/{parts[3]}"
    raise Attention("INVALID_DURABLE_STATE")
def conversation_identity(value: str) -> str:
    """Return the exact conversation ID from one already-valid ChatGPT route."""
    canonical = canonical_conversation_url(value)
    parts = urlparse(canonical).path.split("/")[1:]
    return parts[-1]

def bounded_route_class(value: str, expected_url: str) -> str:
    """Classify one browser URL without returning any identifier or page content."""
    try:
        if isinstance(value, str) and value.startswith("chrome-error://"):
            return "CHROME_ERROR"
        parsed = urlparse(value)
        if parsed.scheme != "https" or parsed.hostname not in {"chatgpt.com", "chat.openai.com"}:
            return "OTHER_HOST"
        if parsed.path.startswith("/auth/"):
            return "AUTH"
        if parsed.path in {"", "/"}:
            return "HOME"
        try:
            return (
                "SAME_CONVERSATION"
                if conversation_identity(value) == conversation_identity(expected_url)
                else "DIFFERENT_CONVERSATION"
            )
        except Attention:
            parts = parsed.path.split("/")[1:]
            if len(parts) >= 2 and parts[0] == "g":
                return "PROJECT_OTHER"
            return "OTHER_ROUTE"
    except Exception:
        return "INVALID_ROUTE"

def bounded_submit_target_drift_reason(value: str, expected_url: str) -> str:
    """Return a fixed structural post-submit drift reason without identifiers."""
    prefix = "SUBMIT_TARGET_DRIFT_"
    try:
        if isinstance(value, str) and value.startswith("chrome-error://"):
            return prefix + "CHROME_ERROR"
        parsed = urlparse(value)
        if parsed.scheme != "https" or parsed.hostname not in {"chatgpt.com", "chat.openai.com"}:
            return prefix + "OTHER_HOST"
        if parsed.username or parsed.password:
            return prefix + "USERINFO"
        try:
            if parsed.port is not None:
                return prefix + "PORT"
        except ValueError:
            return prefix + "PORT"
        if parsed.path.startswith("/auth/"):
            return prefix + "AUTH"
        if parsed.path in {"", "/"}:
            return prefix + "HOME"

        expected_id = conversation_identity(expected_url)
        parts = [part for part in parsed.path.split("/") if part]
        route_id = None
        if len(parts) == 2 and parts[0] == "c" and CONVERSATION_ID.fullmatch(parts[1]):
            route_id = parts[1]
        elif (
            len(parts) == 4
            and parts[0] == "g"
            and parts[2] == "c"
            and CONVERSATION_ID.fullmatch(parts[1])
            and CONVERSATION_ID.fullmatch(parts[3])
        ):
            route_id = parts[3]

        if route_id is not None and route_id != expected_id:
            return prefix + "DIFFERENT_CONVERSATION"
        if route_id == expected_id:
            if parsed.query and parsed.fragment:
                return prefix + "SAME_QUERY_FRAGMENT"
            if parsed.query:
                return prefix + "SAME_QUERY"
            if parsed.fragment:
                return prefix + "SAME_FRAGMENT"
            if parsed.path.endswith("/"):
                return prefix + "SAME_TRAILING_SLASH"
            return prefix + "SAME_ROUTE_OTHER"

        if parts and parts[0] == "g":
            return prefix + "PROJECT_OTHER"
        return prefix + "OTHER_ROUTE"
    except Exception:
        return prefix + "INVALID_ROUTE"

def receipt_for(record_id: str, message: str, target_url: str, sent_at_unix: float) -> DeliveryReceipt:
    return DeliveryReceipt(record_id, sent_at_unix, digest(normalize_message(message)), digest(target_url), 1)
def valid_receipt(delivery: Delivery, record_id: str, message: str, target_url: str) -> bool:
    return (
        delivery.record_id == record_id and delivery.status == "SENT" and
        isinstance(delivery.browser_sent_at_unix, (int, float)) and delivery.browser_sent_at_unix > 0 and
        delivery.receipt_schema_version == 1 and
        delivery.message_sha256 == digest(normalize_message(message)) and
        delivery.target_sha256 == digest(target_url)
    )

def load(path: Path, default: Any) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8")) if path.exists() else default
    except (OSError, json.JSONDecodeError): return default
def save(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True); tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"); os.replace(tmp, path)
def _comparison_path(path: Path) -> str:
    value = str(path.resolve())
    if os.name == "nt":
        if value.startswith("\\\\?\\UNC\\"):
            value = "\\\\" + value[8:]
        elif value.startswith("\\\\?\\"):
            value = value[4:]
        value = os.path.normcase(os.path.normpath(value))
    return value

def workspace_path(root: Path, value: str) -> Path:
    path = (root / value).resolve() if not Path(value).is_absolute() else Path(value).resolve()
    try:
        if os.name == "nt":
            root_cmp = _comparison_path(root)
            path_cmp = _comparison_path(path)
            if os.path.commonpath([root_cmp, path_cmp]) != root_cmp:
                raise ValueError("path escapes workspace")
        else:
            path.relative_to(root)
    except (OSError, ValueError) as error:
        raise Attention("INVALID_DURABLE_STATE") from error
    return path

def validate_bridge_identity(root: Path, expected_sha256: str) -> None:
    """Accept only the project-local bridge bytes selected by the Rust host."""
    if not SHA256_HEX.fullmatch(expected_sha256):
        raise Attention("UNTRUSTED_BRIDGE_REVISION")
    expected_path = workspace_path(root, "scripts/wake_bridge.py")
    source_path = Path(__file__).resolve()
    if _comparison_path(source_path) != _comparison_path(expected_path):
        raise Attention("UNTRUSTED_BRIDGE_PATH")
    if bridge_sha256(source_path) != expected_sha256:
        raise Attention("UNTRUSTED_BRIDGE_REVISION")

def legacy_owner_selected(root: Path) -> bool:
    """Fail closed unless the fixed selector leaves legacy Python eligible."""
    selector = root / ".catdesk" / "wake-bridge" / "owner.json"
    try:
        if not selector.exists():
            return True
        metadata = selector.lstat()
        if selector.is_symlink() or not selector.is_file() or metadata.st_size > MAX_OWNER_SELECTOR_BYTES:
            return False
        value = json.loads(selector.read_text(encoding="utf-8"))
        return (
            isinstance(value, dict)
            and set(value) == {"schemaVersion", "owner"}
            and value["schemaVersion"] == 1
            and value["owner"] == "legacy_python"
        )
    except (OSError, UnicodeError, json.JSONDecodeError, TypeError):
        return False
def actionable(root: Path, record_id: str) -> bool:
    if not SAFE_ID.fullmatch(record_id): return False
    rows = load(root / ".catdesk" / "autonomy" / "review-inbox.json", [])
    matches = [row for row in rows if isinstance(row, dict) and row.get("recordId") == record_id] if isinstance(rows, list) else []
    return len(matches) == 1 and matches[0].get("unread") is True and (matches[0].get("state"), matches[0].get("nextAction")) in {("COMPLETED_VERIFIED", "independent_final_review"), ("WAITING_FOR_CHATGPT", "chatgpt_decision_required")}

class Singleton:
    def __init__(self, path: Path): self.path, self.key, self.handle = path, str(path.resolve()), None
    def __enter__(self):
        with _guard:
            if self.key in _held: raise Busy()
            self.path.parent.mkdir(parents=True, exist_ok=True); self.handle = self.path.open("a+b")
            try:
                if os.name == "nt":
                    import msvcrt
                    self.handle.seek(0); self.handle.write(b"0"); self.handle.flush(); self.handle.seek(0); msvcrt.locking(self.handle.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    import fcntl
                    fcntl.flock(self.handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except (OSError, ImportError): self.handle.close(); raise Busy()
            _held.add(self.key); return self
    def __exit__(self, *_):
        try:
            if os.name == "nt":
                import msvcrt; self.handle.seek(0); msvcrt.locking(self.handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl; fcntl.flock(self.handle.fileno(), fcntl.LOCK_UN)
        finally:
            self.handle.close()
            with _guard: _held.discard(self.key)

class _WindowsUser32:
    """Small stdlib-only adapter; never searches by title or process metadata."""
    GW_OWNER, GWL_EXSTYLE, WS_EX_TOOLWINDOW, GA_ROOT, SW_MAXIMIZE = 4, -20, 0x80, 2, 3
    def __init__(self):
        import ctypes
        from ctypes import wintypes
        self.ctypes, self.wintypes = ctypes, wintypes
        self.user32 = ctypes.WinDLL("user32", use_last_error=True)
        self._enum_callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        self.user32.EnumWindows.argtypes = [self._enum_callback_type, wintypes.LPARAM]
        self.user32.EnumWindows.restype = wintypes.BOOL
        self.user32.IsWindow.argtypes = [wintypes.HWND]
        self.user32.IsWindow.restype = wintypes.BOOL
        self.user32.IsWindowVisible.argtypes = [wintypes.HWND]
        self.user32.IsWindowVisible.restype = wintypes.BOOL
        self.user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
        self.user32.GetWindowThreadProcessId.restype = wintypes.DWORD
        self.user32.GetWindow.argtypes = [wintypes.HWND, wintypes.UINT]
        self.user32.GetWindow.restype = wintypes.HWND
        self.user32.GetAncestor.argtypes = [wintypes.HWND, wintypes.UINT]
        self.user32.GetAncestor.restype = wintypes.HWND
        self.user32.GetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int]
        self.user32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
        self.user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
        self.user32.ShowWindow.restype = wintypes.BOOL
        self.user32.BringWindowToTop.argtypes = [wintypes.HWND]
        self.user32.BringWindowToTop.restype = wintypes.BOOL
        self.user32.SetForegroundWindow.argtypes = [wintypes.HWND]
        self.user32.SetForegroundWindow.restype = wintypes.BOOL
    def enum_windows(self, callback: Callable[[int], None]) -> None:
        def visit(hwnd: Any, _lparam: Any) -> bool:
            callback(self._handle_value(hwnd)); return True
        if not self.user32.EnumWindows(self._enum_callback_type(visit), 0):
            raise OSError("EnumWindows failed")
    @staticmethod
    def _handle_value(value: Any) -> int:
        raw = value.value if hasattr(value, "value") else value
        return int(raw or 0)
    def is_suitable_top_level_window(self, hwnd: int) -> bool:
        return bool(self.user32.IsWindow(hwnd)) and self._handle_value(self.user32.GetAncestor(hwnd, self.GA_ROOT)) == hwnd and self._handle_value(self.user32.GetWindow(hwnd, self.GW_OWNER)) == 0 and not (int(self.user32.GetWindowLongPtrW(hwnd, self.GWL_EXSTYLE)) & self.WS_EX_TOOLWINDOW)
    def is_window_visible(self, hwnd: int) -> bool:
        return bool(self.user32.IsWindowVisible(hwnd))
    def window_process_id(self, hwnd: int) -> int | None:
        process_id = self.wintypes.DWORD()
        return int(process_id.value) if self.user32.GetWindowThreadProcessId(hwnd, self.ctypes.byref(process_id)) else None
    def show_maximized(self, hwnd: int) -> bool:
        return bool(self.user32.ShowWindow(hwnd, self.SW_MAXIMIZE))
    def bring_to_top(self, hwnd: int) -> bool:
        return bool(self.user32.BringWindowToTop(hwnd))
    def set_foreground(self, hwnd: int) -> bool:
        return bool(self.user32.SetForegroundWindow(hwnd))

class CdpSink:
    def __init__(self, url: str, profile: Path, ui_timeout: float, submit_timeout: float):
        if not profile.is_dir(): raise Attention("INVALID_DURABLE_STATE")
        self.url, self.profile, self.ui_timeout, self.submit_timeout = canonical_conversation_url(url), profile, ui_timeout, submit_timeout
    @staticmethod
    def visible(cdp: Any, selector: str) -> bool:
        try: return bool(cdp.is_element_visible(selector))
        except Exception: return False
    def error(self, cdp: Any, current: str | None = None) -> bool:
        if current is None:
            try:
                current = str(cdp.get_current_url())
            except Exception:
                current = None
        if isinstance(current, str) and current.startswith("chrome-error://"):
            return True
        return all(self.visible(cdp, s) for s in NETWORK)
    @staticmethod
    def document_ready_state(cdp: Any) -> str | None:
        """Return only a bounded, non-content document readiness signal.

        Some SeleniumBase/CDP versions do not expose a reliable evaluation
        result while a navigation is in progress.  Its absence is not treated
        as content evidence: the exact target and unique visible editor still
        provide the authoritative fallback.  A valid reported ``loading``
        state, however, is transient and cannot be ready yet.
        """
        try:
            value = CdpSink.cdp_value(cdp.evaluate("document.readyState"))
        except Exception:
            return None
        return value if isinstance(value, str) and value in {"loading", "interactive", "complete"} else None
    @staticmethod
    def editor_selector(cdp: Any, *, prevalidated: bool = False) -> str:
        """Return the first fixed selector that resolves to the unique visible composer."""
        for selector in EDITOR_SELECTORS:
            try:
                if bool(cdp.is_element_visible(selector)):
                    return selector
            except Exception:
                pass
        # Deliberately reduced test adapters may not expose either the
        # visibility helper or evaluate(). Production can reach prevalidated=True
        # only after pre_typing_ready() has already proved one empty visible
        # composer through editor_state()/evaluate().
        if not callable(getattr(cdp, "evaluate", None)):
            if prevalidated:
                return EDITOR
            raise Attention("EDITOR_SELECTOR")

        # Older real CDP adapters may lack visibility but still expose evaluate().
        # Keep the bounded DOM uniqueness check fail-closed in that case.
        status, _empty = CdpSink.editor_state(cdp)
        if status == "ready":
            return EDITOR
        raise Attention("EDITOR_SELECTOR")

    def text(self, cdp: Any) -> str:
        selector = self.editor_selector(cdp)
        for method, args in (("get_attribute", (selector, "value")), ("get_text", (selector,))):
            try:
                value = getattr(cdp, method)(*args, timeout=2)
                if isinstance(value, str) and value.strip(): return value.strip()
            except Exception: pass
        return ""
    @staticmethod
    def post_submit_visible_any(cdp: Any, selectors: tuple[str, ...]) -> bool:
        """Check fixed post-submit selectors through one bounded DOM evaluation."""
        selector_json = json.dumps(list(selectors))
        script = f"""(() => {{
            const selectors = {selector_json};
            return selectors.some(selector =>
                Array.from(document.querySelectorAll(selector)).some(element => {{
                    const style = window.getComputedStyle(element);
                    return element.getClientRects().length > 0 &&
                        style.visibility !== 'hidden' && style.display !== 'none';
                }})
            );
        }})()"""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("POST_SUBMIT_DOM_QUERY_FAILED") from error
        if not isinstance(value, bool):
            raise Attention("POST_SUBMIT_DOM_QUERY_FAILED")
        return value

    @staticmethod
    def post_submit_composer_text(cdp: Any) -> str:
        """Read the bounded visible draft without Selenium visibility waits."""
        script = """(() => {
            const visible = Array.from(document.querySelectorAll(
                '#prompt-textarea, div.ProseMirror[contenteditable="true"], [contenteditable="true"][data-lexical-editor="true"]'
            )).filter(element => {
                const style = window.getComputedStyle(element);
                return element.getClientRects().length > 0 &&
                    style.visibility !== 'hidden' && style.display !== 'none';
            });
            if (visible.length === 0) return ['absent', ''];
            if (visible.length !== 1) return ['ambiguous', ''];
            const element = visible[0];
            const text = element.value || element.innerText || element.textContent || '';
            return ['present', text];
        })()"""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("POST_SUBMIT_COMPOSER_QUERY_FAILED") from error
        if (
            not isinstance(value, list)
            or len(value) != 2
            or value[0] not in {"absent", "present", "ambiguous"}
            or not isinstance(value[1], str)
            or len(value[1]) > MAX_USER_MESSAGE_CHARS
        ):
            raise Attention("POST_SUBMIT_COMPOSER_QUERY_FAILED")
        if value[0] == "ambiguous":
            raise Attention("POST_SUBMIT_COMPOSER_QUERY_FAILED")
        return value[1].strip() if value[0] == "present" else ""

    @staticmethod
    def cdp_value(value: Any) -> Any:
        """Unwrap only the small Runtime.evaluate envelopes used by CDP."""
        for _ in range(4):
            if not isinstance(value, dict):
                break
            keys = set(value)
            if "result" in value and keys <= {"id", "result"}:
                value = value["result"]
            elif "value" in value and keys <= {
                "type", "subtype", "className", "description", "objectId", "value",
            }:
                value = value["value"]
            else:
                break
        return value
    @staticmethod
    def bounded_cdp_strings(value: Any) -> list[str]:
        """Accept a bounded list of strings, never the evaluated text itself."""
        value = CdpSink.cdp_value(value)
        if (not isinstance(value, list) or len(value) > MAX_USER_MESSAGES or
                not all(isinstance(item, str) and len(item) <= MAX_USER_MESSAGE_CHARS for item in value)):
            raise ValueError("invalid bounded string list")
        return value
    @staticmethod
    def editor_state(cdp: Any) -> tuple[str, bool]:
        """Require exactly one visible composer and only its bounded empty state."""
        script = """(() => {
            const visible = Array.from(document.querySelectorAll(
                '#prompt-textarea, div.ProseMirror[contenteditable="true"], [contenteditable="true"][data-lexical-editor="true"]'
            )).filter(element => {
                const style = window.getComputedStyle(element);
                return element.getClientRects().length > 0 && style.visibility !== 'hidden' && style.display !== 'none';
            });
            if (visible.length !== 1) return ['count', visible.length];
            const element = visible[0];
            const text = element.value || element.innerText || element.textContent || '';
            return ['ready', text.trim() === ''];
        })()"""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("EDITOR_SELECTOR") from error
        if not isinstance(value, list) or len(value) != 2:
            raise Attention("EDITOR_SELECTOR")
        if value[0] == "ready" and isinstance(value[1], bool):
            return "ready", value[1]
        if value[0] == "count" and isinstance(value[1], int) and not isinstance(value[1], bool) and 0 <= value[1] <= 8:
            return "count", False
        raise Attention("EDITOR_SELECTOR")
    @staticmethod
    def user_message_digests(cdp: Any) -> list[str]:
        # Query only dedicated USER message containers, never the whole page.
        # ChatGPT puts message controls beside/inside the author container on
        # some current surfaces; hash the dedicated message-content node when
        # present so those controls cannot change the receipt digest.
        script = """(() => {
            const roleUsers = Array.from(document.querySelectorAll(\"[data-message-author-role='user'], article[data-turn='user']:not(:has([data-message-author-role='user']))\"));
            const bubbles = Array.from(document.querySelectorAll('[data-user-message-bubble]'));
            const turnContainers = Array.from(new Set([
                ...document.querySelectorAll(\"[data-testid^='conversation-turn-']\"),
                ...document.querySelectorAll(\"article[id^='conversation-turn-']\"),
                ...document.querySelectorAll(\"[data-turn='user']\")
            ]));
            const fallbackUsers = turnContainers.filter(turn =>
                Boolean(
                    turn.matches(\"[data-turn=\'user\']\") ||
                    turn.querySelector(\"[data-message-author-role=\'user\'], [data-message-content], .whitespace-pre-wrap\")
                )
            );
            const containers = roleUsers.length ? roleUsers : bubbles.length ? bubbles : fallbackUsers;
            return containers.map(container => {
            const content = container.querySelector(\"[data-message-content], [data-markdown-text-tone='user-message'], .whitespace-pre-wrap\") || container;
            return content.innerText || content.textContent || '';
            });
        })()"""
        try:
            value = CdpSink.bounded_cdp_strings(cdp.evaluate(script))
            return [digest(normalize_message(item)) for item in value]
        except Exception as error:
            raise Attention("USER_MESSAGE_RECEIPT") from error
    @staticmethod
    def trusted_turn_anchor_state(cdp: Any, message: str) -> tuple[int, int]:
        """Return exact wake-turn match count and trusted turns after it."""
        expected = normalize_message(message)
        script = f"""(() => {{
            const expected = {json.dumps(expected)};
            const normalize = value => String(value || '').replace(/\\s+/g, ' ').trim();
            const classifyTurn = turn => {{
                const isUserTurn = Boolean(
                    turn.matches(
                        "[data-turn='user'], [data-message-author-role='user'], " +
                        "[data-user-message-bubble], [class~='group/user-message']"
                    ) ||
                    turn.querySelector(
                        "[data-turn='user'], [data-message-author-role='user'], " +
                        "[data-user-message-bubble], [class~='group/user-message']"
                    )
                );
                const isAssistantTurn = Boolean(
                    turn.matches("[data-turn='assistant'], [data-message-author-role='assistant']") ||
                    turn.querySelector("[data-turn='assistant'], [data-message-author-role='assistant'], h4[data-conversation-role='assistant']")
                );
                return [isUserTurn, isAssistantTurn];
            }};
            // Pick exactly one DOM representation tier. Mixing a semantic USER
            // unit with its broader paired wrapper can double-count the same
            // message before the assistant mounts, then erase it after the
            // wrapper gains the assistant role.
            const selectorTiers = [
                "[data-content-search-unit-key]",
                "[data-message-author-role='user'], [data-message-author-role='assistant']",
                "[data-turn='user'], [data-turn='assistant']",
                "[data-user-message-bubble], [class~='group/user-message']",
                "[data-testid^='conversation-turn-'], article[id^='conversation-turn-']",
                "[data-content-search-turn-key]"
            ];
            let turns = [];
            for (const selector of selectorTiers) {{
                const tierTurns = Array.from(document.querySelectorAll(selector)).filter(turn => {{
                    const [isUserTurn, isAssistantTurn] = classifyTurn(turn);
                    return isUserTurn !== isAssistantTurn;
                }});
                if (tierTurns.some(turn => classifyTurn(turn)[0])) {{
                    turns = tierTurns;
                    break;
                }}
            }}
            if (turns.length > 128) return ['invalid', 0, 0];
            const matches = [];
            turns.forEach((turn, index) => {{
                const [isUserTurn, isAssistantTurn] = classifyTurn(turn);
                const text = normalize(turn.innerText || turn.textContent);
                if (isUserTurn && !isAssistantTurn && text.includes(expected)) matches.push(index);
            }});
            if (matches.length > 8) return ['invalid', 0, 0];
            if (matches.length !== 1) return ['ready', matches.length, 0];
            return ['ready', 1, turns.length - matches[0] - 1];
        }})()"""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("USER_MESSAGE_RECEIPT") from error
        if (
            not isinstance(value, list)
            or len(value) != 3
            or value[0] not in {"ready", "invalid"}
            or not isinstance(value[1], int) or isinstance(value[1], bool)
            or not isinstance(value[2], int) or isinstance(value[2], bool)
            or not 0 <= value[1] <= 8
            or not 0 <= value[2] <= 128
            or value[0] != "ready"
        ):
            raise Attention("USER_MESSAGE_RECEIPT")
        return value[1], value[2]

    @staticmethod
    def exact_appended_receipt(prior: list[str], current: list[str], expected: str) -> bool:
        return CdpSink.receipt_stage(prior, current, expected) == "RECEIPT_OK"
    @staticmethod
    def receipt_stage(prior: list[str], current: list[str], expected: str) -> str:
        # The pre-submit snapshot proves freshness only.  Current ChatGPT
        # views may fully remount the prior author-message DOM after submit,
        # so predecessor identities cannot safely be used as a receipt anchor.
        # Delivery is instead proven by this fresh digest being unique and
        # final in two bounded, separated post-submit observations.
        if expected in prior:
            return "SUBMIT_RECEIPT_SEQUENCE_DRIFT"
        expected_count = current.count(expected)
        if expected_count == 1 and current and current[-1] == expected:
            return "RECEIPT_OK"
        if expected_count == 0:
            return "SUBMIT_CLEARED_NO_APPEND"
        return "SUBMIT_RECEIPT_SEQUENCE_DRIFT"

    @staticmethod
    def turn_anchor_receipt_stage(prior_count: int, current_count: int, turns_after: int) -> str:
        if prior_count != 0:
            return "SUBMIT_RECEIPT_SEQUENCE_DRIFT"
        if current_count == 1 and turns_after <= 1:
            return "RECEIPT_OK"
        if current_count == 0:
            return "SUBMIT_CLEARED_NO_APPEND"
        return "SUBMIT_RECEIPT_SEQUENCE_DRIFT"

    @staticmethod
    def pre_submit_receipt_snapshot(cdp: Any, message: str) -> int:
        """Prove the exact wake string is absent from trusted turns before submit."""
        prior_count, _turns_after = CdpSink.trusted_turn_anchor_state(cdp, message)
        if prior_count != 0:
            raise Attention("PRE_SUBMIT_EXPECTED_DIGEST")
        return prior_count

    def wait_for_submission_accepted(
        self,
        cdp: Any,
        prior: int,
        expected: str,
        message: str,
        *,
        monotonic=time.monotonic,
        sleeper=time.sleep,
        wall_time=time.time,
    ) -> float:
        """Prove the submitted turn was accepted without re-submitting.

        This is deliberately not a durable delivery receipt. Normally it uses
        the current sender document. If that document transiently becomes a
        chrome-error page after Send, it may reopen only the same fixed
        canonical conversation and continue proof there without any USER
        action. It still requires the exact USER digest to be uniquely latest,
        the submitted draft to have cleared, and a server-backed turn signal:
        either ChatGPT's active Stop/Pause control or an assistant/timeout state
        following that USER.  The caller may start the response timer from this
        boundary, but final SENT still requires a later fresh-document receipt.
        """
        deadline = monotonic() + max(self.submit_timeout, POST_SUBMIT_RECEIPT_WINDOW_SECONDS)
        expected_message = normalize_message(message)
        last_stage = "SUBMIT_ACCEPTANCE_UNPROVEN"
        while monotonic() < deadline:
            try:
                current_url = str(cdp.get_current_url())
            except Exception as error:
                raise PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED") from error
            if not self.exact(current_url):
                drift_reason = bounded_submit_target_drift_reason(current_url, self.url)
                if (
                    drift_reason == "SUBMIT_TARGET_DRIFT_CHROME_ERROR"
                    and self.reopen_submit_receipt_target(
                        cdp,
                        deadline=deadline,
                        monotonic=monotonic,
                        sleeper=sleeper,
                    )
                ):
                    last_stage = drift_reason
                    continue
                raise PostSubmitUnknown(drift_reason)
            try:
                network_error = self.post_submit_visible_any(cdp, NETWORK)
                composer = normalize_message(self.post_submit_composer_text(cdp))
            except Attention as error:
                raise PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED") from error
            if network_error:
                raise PostSubmitUnknown("RESPONSE_NETWORK_ERROR")
            if composer:
                last_stage = (
                    "SUBMIT_COMPOSER_RETAINED"
                    if composer == expected_message
                    else "SUBMIT_COMPOSER_CHANGED"
                )
                sleeper(READINESS_POLL_SECONDS)
                continue

            try:
                current_count, turns_after = self.trusted_turn_anchor_state(cdp, message)
            except Attention as error:
                raise PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED") from error
            last_stage = self.turn_anchor_receipt_stage(prior, current_count, turns_after)
            if last_stage != "RECEIPT_OK":
                sleeper(READINESS_POLL_SECONDS)
                continue

            try:
                stop_active = self.post_submit_visible_any(cdp, STOP)
            except Attention as error:
                raise PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED") from error
            if stop_active:
                return float(wall_time())

            try:
                timeout_count, _retry_count, assistant_after_user, _completion = (
                    self.response_snapshot(cdp, message)
                )
            except PostSubmitUnknown:
                sleeper(READINESS_POLL_SECONDS)
                continue
            if timeout_count > 0 or assistant_after_user:
                return float(wall_time())

            last_stage = "SUBMIT_ACCEPTANCE_UNPROVEN"
            sleeper(READINESS_POLL_SECONDS)
        raise PostSubmitUnknown(last_stage)

    def durable_receipt_round_trip(self, cdp: Any, *, monotonic=time.monotonic, sleeper=time.sleep) -> None:
        """Require a fresh document load before trusting post-submit DOM evidence.

        ChatGPT can optimistically append a submitted user message to the current
        client-side tree before the conversation is durably persisted.  Two
        observations from that same document therefore are not sufficient as a
        delivery receipt.  Mark the current JS execution context, perform one
        same-profile reload, and require the marker to disappear on the exact
        canonical target before receipt polling is allowed to continue.

        This is a post-submit boundary.  The composer/editor is deliberately
        not part of readiness here: while the assistant is actively generating,
        current ChatGPT surfaces may hide or replace the normal editor.  Requiring
        pre-submit editor readiness would delay the durable USER receipt until
        after the assistant turn ended, defeating both the live turn timer and
        response-completion observer.
        """
        marker = "__catdeskWakeReceiptPreReloadV1"
        try:
            cdp.evaluate(f"window.{marker} = true; true")
            reload_page = getattr(cdp, "reload", None)
            if not callable(reload_page):
                raise RuntimeError("CDP reload unavailable")
            reload_page(ignore_cache=False)
        except Exception as error:
            raise PostSubmitUnknown("SUBMIT_RECEIPT_ROUND_TRIP_FAILED") from error

        deadline = monotonic() + self.submit_timeout
        while monotonic() < deadline:
            try:
                current = str(cdp.get_current_url())
            except Exception:
                sleeper(.25)
                continue
            if not self.exact(current):
                drift_reason = bounded_submit_target_drift_reason(current, self.url)
                if (
                    drift_reason == "SUBMIT_TARGET_DRIFT_CHROME_ERROR"
                    and self.reopen_submit_receipt_target(
                        cdp,
                        deadline=deadline,
                        monotonic=monotonic,
                        sleeper=sleeper,
                    )
                ):
                    # Same-CDP fixed canonical reload only; no USER resubmit.
                    # Still require fresh-document marker proof below.
                    continue
                raise PostSubmitUnknown(drift_reason)
            try:
                if self.post_submit_visible_any(cdp, NETWORK):
                    sleeper(.25)
                    continue
            except Attention:
                sleeper(.25)
                continue
            try:
                fresh_document = self.cdp_value(
                    cdp.evaluate(f"typeof window.{marker} === 'undefined'")
                ) is True
            except Exception:
                sleeper(.25)
                continue
            if not fresh_document or self.document_ready_state(cdp) == "loading":
                sleeper(.25)
                continue
            # Post-submit receipt proof is intentionally editor-independent.
            # Exact target + a fresh loaded document is sufficient to begin the
            # bounded exact USER-message observations below; no USER action is
            # possible on this side of the submit boundary.
            return
        raise PostSubmitUnknown("SUBMIT_RECEIPT_ROUND_TRIP_FAILED")

    def confirm_exact_receipt(self, cdp: Any, prior: int, expected: str, record_id: str, message: str, *, monotonic=time.monotonic, sleeper=time.sleep, wall_time=time.time) -> DeliveryReceipt:
        """Require two separated exact-final observations without another submit."""
        deadline = monotonic() + max(self.submit_timeout, POST_SUBMIT_RECEIPT_WINDOW_SECONDS)
        stop_seen_at = None
        stable_observations = 0
        first_stable_at = None
        last_stage = "SUBMIT_RECEIPT_UNPROVEN"
        while monotonic() < deadline or (stop_seen_at is not None and monotonic() < stop_seen_at + POST_STOP_HOLD_SECONDS):
            now = monotonic()
            try:
                current_url = str(cdp.get_current_url())
            except Exception as error:
                raise PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED") from error
            if not self.exact(current_url):
                drift_reason = bounded_submit_target_drift_reason(current_url, self.url)
                if (
                    drift_reason == "SUBMIT_TARGET_DRIFT_CHROME_ERROR"
                    and self.reopen_submit_receipt_target(
                        cdp, deadline=deadline, monotonic=monotonic, sleeper=sleeper
                    )
                ):
                    # An error-page transition invalidates previous same-DOM
                    # observations. Require two fresh exact-turn polls again.
                    stable_observations, first_stable_at, stop_seen_at = 0, None, None
                    continue
                raise PostSubmitUnknown(drift_reason)
            try:
                stop_active = self.post_submit_visible_any(cdp, STOP)
                composer = normalize_message(self.post_submit_composer_text(cdp))
            except Attention as error:
                raise PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED") from error
            if stop_active and stop_seen_at is None:
                stop_seen_at = now
            if not composer:
                try:
                    current_count, turns_after = self.trusted_turn_anchor_state(cdp, message)
                except Attention as error:
                    raise PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED") from error
                last_stage = self.turn_anchor_receipt_stage(prior, current_count, turns_after)
                if last_stage == "RECEIPT_OK":
                    if stable_observations == 0:
                        stable_observations, first_stable_at = 1, now
                    elif first_stable_at is not None and now > first_stable_at:
                        stable_observations = 2
                    if stable_observations >= 2 and (stop_seen_at is None or now >= stop_seen_at + POST_STOP_HOLD_SECONDS):
                        return receipt_for(record_id, message, self.url, wall_time())
                else:
                    stable_observations, first_stable_at = 0, None
            elif composer == normalize_message(message):
                last_stage = "SUBMIT_COMPOSER_RETAINED"
                stable_observations, first_stable_at = 0, None
            else:
                last_stage = "SUBMIT_COMPOSER_CHANGED"
                stable_observations, first_stable_at = 0, None
            if stop_seen_at is not None and now < stop_seen_at + POST_STOP_HOLD_SECONDS:
                sleeper(min(.25, max(0.0, stop_seen_at + POST_STOP_HOLD_SECONDS - now)))
            else:
                sleeper(.25)
        raise PostSubmitUnknown(last_stage)

    def response_snapshot(self, cdp: Any, message: str, *, require_usable_body: bool = False) -> tuple[int, int, bool, bool]:
        """Return only bounded structural signals for the latest user turn.

        The timeout detector is intentionally exact-text and scoped strictly
        after the latest user-authored message.  It never returns page text.
        Retry is considered actionable only when exactly one visible Retry
        control can be associated with the exact timeout card.
        """
        script = f"""(() => {{
            const timeoutText = {json.dumps("Message delivery timed out. Please try again.")};
            const retryLabel = {json.dumps("Retry")};
            const expectedWake = {json.dumps(normalize_message(message))};
            const requireUsableBody = {str(require_usable_body).lower()};
            const normalize = value => String(value || '').replace(/\\s+/g, ' ').trim();
            const visible = element => {{
                if (!element) return false;
                const style = window.getComputedStyle(element);
                return element.getClientRects().length > 0 &&
                    style.visibility !== 'hidden' && style.display !== 'none';
            }};
            const turnCandidates = Array.from(document.querySelectorAll(
                "[data-content-search-turn-key], " +
                "[data-testid^='conversation-turn-'], article[id^='conversation-turn-'], " +
                "[data-turn='user'], [data-turn='assistant'], " +
                "[data-message-author-role='user'], [data-message-author-role='assistant'], " +
                "[data-user-message-bubble], [class~='group/user-message']"
            ));
            const turnContainers = [];
            turnCandidates.forEach(candidate => {{
                const currentTurn = candidate.closest("[data-content-search-turn-key]");
                const container = currentTurn || candidate.closest(
                    "[data-testid^='conversation-turn-'], article[id^='conversation-turn-'], " +
                    "[data-turn='user'], [data-turn='assistant'], " +
                    "[class~='group/user-message']"
                ) || candidate;
                if (!turnContainers.includes(container)) turnContainers.push(container);
            }});
            const wakeTurns = turnContainers.filter(turn =>
                normalize(turn.innerText || turn.textContent).includes(expectedWake)
            );
            if (wakeTurns.length !== 1)
                return [wakeTurns.length === 0 ? 'missing-user' : 'invalid', 0, 0, false, false];
            const latestUser = wakeTurns[0];
            const followsLatestUser = element =>
                Boolean(latestUser.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING);
            const pairedTurn = latestUser.matches("[data-content-search-turn-key]");
            const inWakeExchange = element =>
                Boolean(element && (latestUser.contains(element) || followsLatestUser(element)));
            const turnsAfterWake = turnContainers.filter(turn => followsLatestUser(turn));
            if (turnsAfterWake.length > 1)
                return ['invalid', 0, 0, false, false];
            const rawTimeouts = Array.from(document.querySelectorAll('div, span, p'))
                .filter(element => visible(element) && inWakeExchange(element) &&
                    normalize(element.innerText || element.textContent) === timeoutText);
            const timeouts = rawTimeouts.filter(element =>
                !Array.from(element.children).some(child =>
                    visible(child) && normalize(child.innerText || child.textContent) === timeoutText));
            const retryButtons = [];
            for (const timeout of timeouts) {{
                let scope = timeout;
                for (let depth = 0; depth < 7 && scope; depth += 1, scope = scope.parentElement) {{
                    const candidates = Array.from(scope.querySelectorAll('button')).filter(button =>
                        visible(button) &&
                        (normalize(button.innerText || button.textContent) === retryLabel ||
                         normalize(button.getAttribute('aria-label')) === retryLabel));
                    if (candidates.length) {{
                        for (const button of candidates) {{
                            if (!retryButtons.includes(button)) retryButtons.push(button);
                        }}
                        break;
                    }}
                }}
            }}
            const roleAssistants = Array.from(document.querySelectorAll(
                "[data-message-author-role='assistant'], article[data-turn='assistant']"
            )).filter(element => inWakeExchange(element));
            const pairedAssistantUnits = pairedTurn
                ? Array.from(latestUser.querySelectorAll("[data-content-search-unit-key]")).filter(unit =>
                    !unit.matches("[data-user-message-bubble], [class~='group/user-message']") &&
                    !unit.querySelector("[data-user-message-bubble], [class~='group/user-message']"))
                : [];
            const pairedAssistantComplete = pairedAssistantUnits.some(unit =>
                normalize(unit.innerText || unit.textContent).length > 0 &&
                Boolean(unit.querySelector("[data-chatgpt-selection-message-id]")));
            // Current ChatGPT turn-keys are one user+assistant exchange. Never
            // borrow a later turn as assistant evidence for a paired current-DOM
            // wake; that would let an unrelated subsequent user turn satisfy
            // response completion. Legacy surfaces keep the following-turn path.
            const fallbackAssistants = pairedTurn ? pairedAssistantUnits : turnsAfterWake;
            const assistants = roleAssistants.length ? roleAssistants : fallbackAssistants;
            const latestAssistant = assistants.length ? assistants[assistants.length - 1] : null;
            const assistantRawText = latestAssistant
                ? normalize(latestAssistant.innerText || latestAssistant.textContent)
                    .replace(/[‘’]/g, "'")
                : '';
            // A mounted assistant shell may contain only Copy/Regenerate
            // controls. Read a dedicated answer body and remove controls from
            // that body before treating it as a usable response. If the current
            // UI offers no known body node, cooperative observation stays
            // pending instead of inferring an answer from button labels.
            const contentSelector = '[data-message-content], .markdown, [class~="prose"], [data-markdown-text-style="assistant-message"]';
            const bodyNode = latestAssistant
                ? (latestAssistant.matches(contentSelector) ? latestAssistant :
                   latestAssistant.querySelector(contentSelector))
                : null;
            let assistantBodyText = '';
            if (bodyNode && visible(bodyNode)) {{
                const bodyCopy = bodyNode.cloneNode(true);
                bodyCopy.querySelectorAll('button, [role="button"], [role="toolbar"], [aria-hidden="true"]')
                    .forEach(control => control.remove());
                assistantBodyText = normalize(bodyCopy.textContent);
            }}
            // ChatGPT can render an old assistant as a load-error placeholder
            // while retaining Copy/Regenerate controls. Those controls alone
            // are not evidence of a usable completed response.
            if (assistantRawText.toLowerCase().includes("this response couldn't load"))
                return ['load-error', 0, 0, true, false];
            const latestAssistantTurn = latestAssistant
                ? (latestAssistant.closest("[data-content-search-turn-key]") ||
                   latestAssistant.closest("[data-testid^='conversation-turn-']") ||
                   latestAssistant.closest("article[id^='conversation-turn-']") ||
                   latestAssistant.closest("article[data-turn='assistant']") ||
                   latestAssistant.parentElement)
                : null;
            const pairedCompletionAction = pairedTurn
                ? latestUser.querySelector("button[data-testid='copy-turn-action-button']")
                : null;
            const completionAction = pairedCompletionAction || (latestAssistantTurn
                ? latestAssistantTurn.querySelector("button[data-testid='copy-turn-action-button']")
                : null);
            // The paired turn can also hold a USER Copy button. For passive
            // cooperative completion, climb only inside an assistant-only
            // subtree; never treat a whole-turn control or a mounted message
            // selection id as proof that the assistant finished.
            const userNodeSelector = "[data-message-author-role='user'], article[data-turn='user'], " +
                "[data-user-message-bubble], [class~='group/user-message']";
            let assistantCompletionAction = null;
            for (let scope = latestAssistant; scope && scope !== latestUser; scope = scope.parentElement) {{
                if (scope.querySelector(userNodeSelector)) break;
                const actions = Array.from(new Set([
                    ...scope.querySelectorAll("button[data-testid='copy-turn-action-button']"),
                    ...scope.querySelectorAll(".turn-action-controls button[aria-label='Copy']"),
                ])).filter(button => !bodyNode || !bodyNode.contains(button));
                if (actions.length > 1) break;
                if (actions.length === 1) {{
                    assistantCompletionAction = actions[0];
                    break;
                }}
            }}
            // The current paired-turn UI puts its ASSISTANT toolbar beside
            // both message units. Qualify that toolbar by its distinct Copy
            // label, position after the sole ASSISTANT unit and exclusion from
            // every USER subtree. A USER Copy message or code-block Copy is
            // not a completed-answer control.
            if (!assistantCompletionAction && pairedTurn && latestAssistant &&
                    pairedAssistantUnits.length === 1 &&
                    (pairedAssistantUnits[0] === latestAssistant ||
                     pairedAssistantUnits[0].contains(latestAssistant)) &&
                    pairedAssistantUnits[0].querySelector('h4[data-conversation-role="assistant"]')) {{
                const actions = Array.from(latestUser.querySelectorAll(
                    ".turn-action-controls button[aria-label='Copy']"
                )).filter(button => !button.closest(userNodeSelector) &&
                    !pairedAssistantUnits[0].contains(button) &&
                    Boolean(pairedAssistantUnits[0].compareDocumentPosition(button) & Node.DOCUMENT_POSITION_FOLLOWING));
                if (actions.length === 1) assistantCompletionAction = actions[0];
            }}
            // ChatGPT may keep completed-turn actions in the DOM but visually hide
            // them until hover. Presence on the latest assistant turn/exchange is the
            // durable completion signal; requiring visibility caused clean turns
            // to age into RESPONSE_COMPLETION_UNPROVEN.
            const completionActionReady = requireUsableBody
                ? Boolean(assistantCompletionAction) && assistantBodyText.length > 0
                : (Boolean(completionAction) || pairedAssistantComplete);
            if (timeouts.length > 8 || retryButtons.length > 8 || assistants.length > 64)
                return ['invalid', 0, 0, false, false];
            return [
                'ready',
                timeouts.length,
                retryButtons.length,
                assistants.length > 0,
                completionActionReady,
            ];
        }})()"""
        try:
            value = self.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise PostSubmitUnknown("RESPONSE_STATE_QUERY_FAILED") from error
        if (not isinstance(value, list) or len(value) != 5 or
                value[0] not in {"ready", "missing-user", "invalid", "load-error"} or
                not isinstance(value[1], int) or isinstance(value[1], bool) or
                not isinstance(value[2], int) or isinstance(value[2], bool) or
                not isinstance(value[3], bool) or
                not isinstance(value[4], bool)):
            raise PostSubmitUnknown("RESPONSE_STATE_QUERY_FAILED")
        if value[0] == "load-error":
            raise PostSubmitUnknown("RESPONSE_COMPLETION_UNPROVEN")
        if value[0] != "ready":
            raise PostSubmitUnknown("RESPONSE_STATE_QUERY_FAILED")
        return value[1], value[2], value[3], value[4]

    def observe_cooperative_response(self, cdp: Any, message: str, expected_digest: str) -> str:
        """One read-only check of a durably sent USER turn on a new document.

        The caller opens the exact target before this check. In particular,
        this helper never retries the assistant or re-submits a USER message.
        """
        try:
            current = str(cdp.get_current_url())
        except Exception as error:
            raise Attention("RESPONSE_TARGET_QUERY_FAILED") from error
        if not self.exact(current):
            raise Attention("RESPONSE_TARGET_DRIFT")
        if self.post_submit_visible_any(cdp, NETWORK):
            raise Attention("RESPONSE_NETWORK_ERROR")
        if (self.post_submit_visible_any(cdp, LOGIN) or
                self.post_submit_visible_any(cdp, CAPTCHA)):
            raise Attention("LOGIN_OR_PROFILE_REQUIRED")
        if self.document_ready_state(cdp) != "complete":
            return "RESPONSE_PENDING"
        try:
            users = self.user_message_digests(cdp)
            count, turns_after = self.trusted_turn_anchor_state(cdp, message)
        except Attention as error:
            raise Attention("RESPONSE_SEQUENCE_QUERY_FAILED") from error
        # readyState can precede React's conversation mount. Only a wholly
        # empty USER/anchor view is eligible for the caller's bounded wait.
        # A nonempty wrong/duplicate view must still fail immediately.
        if not users and count == 0:
            raise Attention("RESPONSE_USER_DOCUMENT_NOT_HYDRATED")
        if users.count(expected_digest) != 1 or not users:
            raise Attention("RESPONSE_EXACT_USER_MISSING_OR_AMBIGUOUS")
        if users[-1] != expected_digest or count != 1 or turns_after > 1:
            raise Attention("RESPONSE_SEQUENCE_DRIFT")
        if self.post_submit_visible_any(cdp, STOP):
            return "RESPONSE_PENDING"
        try:
            editor_status, _editor_empty = self.editor_state(cdp)
        except Attention as error:
            raise Attention("RESPONSE_STATE_QUERY_FAILED") from error
        if editor_status != "ready":
            return "RESPONSE_PENDING"
        try:
            timeout_count, _retry_count, assistant_after_user, completed = self.response_snapshot(
                cdp, message, require_usable_body=True
            )
        except PostSubmitUnknown as error:
            raise Attention("RESPONSE_STATE_UNPROVEN") from error
        if timeout_count:
            raise Attention("RESPONSE_TIMEOUT_RETRY_REQUIRED")
        return "RESPONSE_COMPLETE" if assistant_after_user and completed else "RESPONSE_PENDING"

    def click_response_timeout_retry(self, cdp: Any, message: str) -> None:
        """Click exactly one Retry control tied to the exact Wake turn."""
        script = f"""(() => {{
            const timeoutText = {json.dumps("Message delivery timed out. Please try again.")};
            const retryLabel = {json.dumps("Retry")};
            const expectedWake = {json.dumps(normalize_message(message))};
            const normalize = value => String(value || '').replace(/\\s+/g, ' ').trim();
            const visible = element => {{
                if (!element) return false;
                const style = window.getComputedStyle(element);
                return element.getClientRects().length > 0 &&
                    style.visibility !== 'hidden' && style.display !== 'none';
            }};
            const turnCandidates = Array.from(document.querySelectorAll(
                "[data-content-search-turn-key], " +
                "[data-testid^='conversation-turn-'], article[id^='conversation-turn-'], " +
                "[data-turn='user'], [data-turn='assistant'], " +
                "[data-message-author-role='user'], [data-message-author-role='assistant'], " +
                "[data-user-message-bubble], [class~='group/user-message']"
            ));
            const turnContainers = [];
            turnCandidates.forEach(candidate => {{
                const currentTurn = candidate.closest("[data-content-search-turn-key]");
                const container = currentTurn || candidate.closest(
                    "[data-testid^='conversation-turn-'], article[id^='conversation-turn-'], " +
                    "[data-turn='user'], [data-turn='assistant'], " +
                    "[class~='group/user-message']"
                ) || candidate;
                if (!turnContainers.includes(container)) turnContainers.push(container);
            }});
            const wakeTurns = turnContainers.filter(turn =>
                normalize(turn.innerText || turn.textContent).includes(expectedWake)
            );
            if (wakeTurns.length !== 1) return wakeTurns.length ? 'ambiguous-user' : 'missing-user';
            const latestUser = wakeTurns[0];
            const followsLatestUser = element =>
                Boolean(latestUser.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING);
            const inWakeExchange = element =>
                Boolean(element && (latestUser.contains(element) || followsLatestUser(element)));
            if (turnContainers.filter(turn => followsLatestUser(turn)).length > 1) return 'ambiguous-user';
            const rawTimeouts = Array.from(document.querySelectorAll('div, span, p'))
                .filter(element => visible(element) && inWakeExchange(element) &&
                    normalize(element.innerText || element.textContent) === timeoutText);
            const timeouts = rawTimeouts.filter(element =>
                !Array.from(element.children).some(child =>
                    visible(child) && normalize(child.innerText || child.textContent) === timeoutText));
            if (timeouts.length !== 1) return timeouts.length ? 'ambiguous-timeout' : 'missing-timeout';
            const retryButtons = [];
            let scope = timeouts[0];
            for (let depth = 0; depth < 7 && scope; depth += 1, scope = scope.parentElement) {{
                const candidates = Array.from(scope.querySelectorAll('button')).filter(button =>
                    visible(button) &&
                    (normalize(button.innerText || button.textContent) === retryLabel ||
                     normalize(button.getAttribute('aria-label')) === retryLabel));
                if (candidates.length) {{
                    for (const button of candidates) {{
                        if (!retryButtons.includes(button)) retryButtons.push(button);
                    }}
                    break;
                }}
            }}
            if (retryButtons.length !== 1)
                return retryButtons.length ? 'ambiguous-retry' : 'missing-retry';
            retryButtons[0].click();
            return 'clicked';
        }})()"""
        try:
            value = self.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise PostSubmitUnknown("RESPONSE_RETRY_CLICK_FAILED") from error
        if value != "clicked":
            raise PostSubmitUnknown("RESPONSE_RETRY_CONTROL_AMBIGUOUS")

    def reopen_submit_receipt_target(
        self,
        cdp: Any,
        *,
        deadline: float,
        monotonic=time.monotonic,
        sleeper=time.sleep,
    ) -> bool:
        """Recover only from post-submit chrome-error drift without re-submitting.

        The USER action has already occurred at this boundary, so recovery may
        navigate in-place in the current CDP tab to the fixed canonical conversation. It never queries
        editor readiness, types, or clicks Send. Success means only that a
        fresh exact-target document is loaded; normal receipt proof still has
        to observe the exact submitted USER turn and a server-backed signal.
        """
        navigate = getattr(cdp, "get", None)
        if not callable(navigate):
            return False
        recovery_deadline = min(
            deadline,
            monotonic() + max(self.ui_timeout, READINESS_WINDOW_SECONDS),
        )
        # Only retry *navigation* after Chrome's own error document. Never
        # retype the wake, click Send, switch tabs, or navigate away from a
        # different host/chat. A thrown CDP navigation can be transient.
        attempts = 0
        next_navigation_at = monotonic()
        while monotonic() < recovery_deadline:
            try:
                current = str(cdp.get_current_url())
            except Exception:
                # No target identity evidence: observe rather than navigate.
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            if self.exact(current):
                if not self.error(cdp, current) and self.document_ready_state(cdp) != "loading":
                    return True
            elif bounded_submit_target_drift_reason(current, self.url) != "SUBMIT_TARGET_DRIFT_CHROME_ERROR":
                # A different conversation or origin is not a network retry.
                return False
            elif attempts < 3 and monotonic() >= next_navigation_at:
                attempts += 1
                try:
                    navigate(self.url)
                except Exception:
                    # Reinspect the actual route before any bounded retry.
                    pass
                # Inspect the result immediately. A verified same-tab return
                # can complete inside a short remaining receipt deadline; a
                # blind half-second sleep would discard that recovery.
                try:
                    navigated_url = str(cdp.get_current_url())
                except Exception:
                    navigated_url = None
                if navigated_url is not None:
                    if self.exact(navigated_url):
                        if (not self.error(cdp, navigated_url)
                                and self.document_ready_state(cdp) != "loading"):
                            return True
                    elif bounded_submit_target_drift_reason(navigated_url, self.url) != "SUBMIT_TARGET_DRIFT_CHROME_ERROR":
                        return False
                next_navigation_at = monotonic() + min(float(2 ** (attempts - 1)), 4.0)
            sleeper(RESPONSE_POLL_SECONDS)
        return False

    def reopen_response_target(
        self,
        cdp: Any,
        *,
        monotonic=time.monotonic,
        sleeper=time.sleep,
    ) -> None:
        """Re-open only the already-authorized exact conversation after timeout."""
        navigate = getattr(cdp, "get", None)
        if not callable(navigate):
            raise PostSubmitUnknown("RESPONSE_REOPEN_UNAVAILABLE")
        try:
            navigate(self.url)
        except Exception as error:
            raise PostSubmitUnknown("RESPONSE_REOPEN_FAILED") from error
        deadline = monotonic() + max(self.ui_timeout, READINESS_WINDOW_SECONDS)
        while monotonic() < deadline:
            try:
                current = str(cdp.get_current_url())
            except Exception:
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            if not self.exact(current):
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            if self.error(cdp, current):
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            if self.document_ready_state(cdp) == "loading":
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            try:
                editor_status, _ = self.editor_state(cdp)
            except Attention:
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            if editor_status == "ready":
                return
            sleeper(RESPONSE_POLL_SECONDS)
        raise PostSubmitUnknown("RESPONSE_REOPEN_FAILED")

    def refresh_response_observer(
        self,
        cdp: Any,
        *,
        monotonic=time.monotonic,
        sleeper=time.sleep,
    ) -> None:
        """Refresh the current exact target without changing USER-turn ownership.

        This is used only after the USER wake is already accepted and while the
        active Stop/Pause control remains visible.  Refreshing periodically lets
        the observer discard a stale client-side generation control while keeping
        the same browser/profile/conversation.  It never types, clicks Send, or
        opens a caller-selected target.
        """
        reload_page = getattr(cdp, "reload", None)
        if not callable(reload_page):
            raise PostSubmitUnknown("RESPONSE_REFRESH_UNAVAILABLE")
        try:
            reload_page(ignore_cache=False)
        except TypeError:
            try:
                reload_page()
            except Exception as error:
                raise PostSubmitUnknown("RESPONSE_REFRESH_FAILED") from error
        except Exception as error:
            raise PostSubmitUnknown("RESPONSE_REFRESH_FAILED") from error

        deadline = monotonic() + max(self.ui_timeout, READINESS_WINDOW_SECONDS)
        while monotonic() < deadline:
            try:
                current = str(cdp.get_current_url())
            except Exception:
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            if not self.exact(current):
                raise PostSubmitUnknown("RESPONSE_TARGET_DRIFT")
            if self.error(cdp, current):
                raise PostSubmitUnknown("RESPONSE_NETWORK_ERROR")
            if self.document_ready_state(cdp) == "loading":
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            return
        raise PostSubmitUnknown("RESPONSE_REFRESH_FAILED")

    def wait_for_response_completion(
        self,
        cdp: Any,
        expected_message: str,
        *,
        observer: Callable[[str], None] | None = None,
        allow_retry: bool = True,
        monotonic=time.monotonic,
        sleeper=time.sleep,
    ) -> None:
        """Keep the owned browser alive until the wake-triggered turn terminates.

        The same-tab submission-accepted boundary is required before this
        method is called; durable fresh-document receipt proof intentionally
        happens only after this method returns. It never types or submits
        another user message. While Stop/Pause remains active, the same browser
        periodically refreshes the exact authorized conversation and revalidates
        the latest USER digest so a stale client-side generation control cannot
        hold the browser forever. If the exact latest turn ends with ChatGPT's
        bounded delivery-timeout card, it retries only that failed assistant
        turn before any periodic refresh can destroy the transient Retry control.
        """
        total_deadline = monotonic() + RESPONSE_TOTAL_WINDOW_SECONDS
        generation_deadline = monotonic() + RESPONSE_GENERATION_WINDOW_SECONDS
        next_heartbeat = monotonic()
        next_active_refresh = None
        stable_idle_polls = 0
        retries = 0

        def emit_stage(stage: str) -> None:
            if observer is not None:
                observer(stage)

        def require_expected_turn_sequence() -> None:
            try:
                count, turns_after = self.trusted_turn_anchor_state(cdp, expected_message)
            except Attention as error:
                raise PostSubmitUnknown("RESPONSE_SEQUENCE_QUERY_FAILED") from error
            if count != 1 or turns_after > 1:
                raise PostSubmitUnknown("RESPONSE_SEQUENCE_DRIFT")

        def wait_for_expected_turn_after_refresh() -> None:
            deadline = monotonic() + max(self.ui_timeout, READINESS_WINDOW_SECONDS)
            while monotonic() < deadline:
                try:
                    count, turns_after = self.trusted_turn_anchor_state(cdp, expected_message)
                except Attention:
                    sleeper(RESPONSE_POLL_SECONDS)
                    continue
                if count > 1 or turns_after > 1:
                    raise PostSubmitUnknown("RESPONSE_SEQUENCE_DRIFT")
                if count == 1:
                    return
                sleeper(RESPONSE_POLL_SECONDS)
            raise PostSubmitUnknown("RESPONSE_SEQUENCE_DRIFT")

        emit_stage("GENERATION_WAIT")
        while monotonic() < total_deadline:
            now = monotonic()
            try:
                current_url = str(cdp.get_current_url())
            except Exception as error:
                raise PostSubmitUnknown("RESPONSE_STATE_QUERY_FAILED") from error
            if not self.exact(current_url):
                raise PostSubmitUnknown("RESPONSE_TARGET_DRIFT")
            try:
                network_error = self.post_submit_visible_any(cdp, NETWORK)
                stop_active = self.post_submit_visible_any(cdp, STOP)
            except Attention as error:
                raise PostSubmitUnknown("RESPONSE_STATE_QUERY_FAILED") from error
            if network_error:
                raise PostSubmitUnknown("RESPONSE_NETWORK_ERROR")
            # The per-generation bound also applies while the composer is
            # missing or remounting. Otherwise that branch can wait for the
            # entire 90-minute total budget without honoring the 35-minute cap.
            if now >= generation_deadline:
                raise PostSubmitUnknown(
                    "RESPONSE_GENERATION_TIMEOUT" if stop_active else "RESPONSE_COMPLETION_UNPROVEN"
                )
            if stop_active:
                stable_idle_polls = 0
                if now >= generation_deadline:
                    raise PostSubmitUnknown("RESPONSE_GENERATION_TIMEOUT")
                if next_active_refresh is None:
                    next_active_refresh = now + RESPONSE_ACTIVE_REFRESH_SECONDS
                if now >= next_active_refresh:
                    self.refresh_response_observer(
                        cdp, monotonic=monotonic, sleeper=sleeper
                    )
                    wait_for_expected_turn_after_refresh()
                    next_active_refresh = monotonic() + RESPONSE_ACTIVE_REFRESH_SECONDS
                    next_heartbeat = monotonic()
                    continue
                if now >= next_heartbeat:
                    emit_stage("GENERATION_ACTIVE")
                    next_heartbeat = now + RESPONSE_HEARTBEAT_SECONDS
                sleeper(RESPONSE_POLL_SECONDS)
                continue

            next_active_refresh = None
            try:
                editor_status, _editor_empty = self.editor_state(cdp)
            except Attention as error:
                raise PostSubmitUnknown("RESPONSE_STATE_QUERY_FAILED") from error
            if editor_status != "ready":
                stable_idle_polls = 0
                sleeper(RESPONSE_POLL_SECONDS)
                continue
            stable_idle_polls += 1
            if stable_idle_polls < RESPONSE_STABLE_POLLS:
                sleeper(RESPONSE_POLL_SECONDS)
                continue

            require_expected_turn_sequence()
            timeout_count, retry_count, assistant_after_user, completion_action_ready = self.response_snapshot(cdp, expected_message)
            if timeout_count == 0:
                if assistant_after_user and completion_action_ready:
                    emit_stage("RESPONSE_COMPLETED")
                    return
                if now >= generation_deadline:
                    raise PostSubmitUnknown("RESPONSE_COMPLETION_UNPROVEN")
                if now >= next_heartbeat:
                    emit_stage("GENERATION_WAIT")
                    next_heartbeat = now + RESPONSE_HEARTBEAT_SECONDS
                sleeper(RESPONSE_POLL_SECONDS)
                continue

            if timeout_count != 1 or retry_count != 1:
                raise PostSubmitUnknown("RESPONSE_TIMEOUT_RETRY_AMBIGUOUS")
            if not allow_retry:
                raise PostSubmitUnknown("BOOTSTRAP_RESPONSE_RETRY_REQUIRED")
            if retries >= RESPONSE_MAX_RETRIES:
                raise PostSubmitUnknown("RESPONSE_RETRY_EXHAUSTED")

            emit_stage("RESPONSE_TIMEOUT_DETECTED")
            # The timeout card is transient UI. Reloading before Retry can
            # destroy the only actionable control while leaving the durable
            # USER wake intact. Re-validate the exact latest USER message and
            # click the unique assistant Retry in the current document first.
            # We never type or re-submit the USER wake on this path.
            require_expected_turn_sequence()
            emit_stage("RESPONSE_RETRYING")
            self.click_response_timeout_retry(cdp, expected_message)
            retries += 1
            retry_start_deadline = min(total_deadline, monotonic() + RESPONSE_RETRY_START_WINDOW_SECONDS)
            retry_started = False
            while monotonic() < retry_start_deadline:
                try:
                    current_url = str(cdp.get_current_url())
                except Exception as error:
                    raise PostSubmitUnknown("RESPONSE_STATE_QUERY_FAILED") from error
                if not self.exact(current_url):
                    raise PostSubmitUnknown("RESPONSE_TARGET_DRIFT")
                try:
                    network_error = self.post_submit_visible_any(cdp, NETWORK)
                    stop_active = self.post_submit_visible_any(cdp, STOP)
                except Attention as error:
                    raise PostSubmitUnknown("RESPONSE_STATE_QUERY_FAILED") from error
                if network_error:
                    raise PostSubmitUnknown("RESPONSE_NETWORK_ERROR")
                if stop_active:
                    retry_started = True
                    break
                timeout_count, _retry_count, _assistant_after_user, _completion_action_ready = self.response_snapshot(cdp, expected_message)
                if timeout_count == 0:
                    retry_started = True
                    break
                sleeper(RESPONSE_POLL_SECONDS)
            if monotonic() >= total_deadline:
                raise PostSubmitUnknown("RESPONSE_TOTAL_TIMEOUT")
            if not retry_started:
                raise PostSubmitUnknown("RESPONSE_RETRY_NOT_STARTED")
            emit_stage("RESPONSE_RETRY_STARTED")
            generation_deadline = monotonic() + RESPONSE_GENERATION_WINDOW_SECONDS
            next_heartbeat = monotonic()
            next_active_refresh = None
            stable_idle_polls = 0

        raise PostSubmitUnknown("RESPONSE_TOTAL_TIMEOUT")

    def reconcile_persisted_receipt(
        self,
        cdp: Any,
        expected: str,
        message: str,
        record_id: str,
        target_sha256: str,
        *,
        monotonic=time.monotonic,
        sleeper=time.sleep,
        wall_time=time.time,
    ) -> DeliveryReceipt:
        """Prove an already-SUBMITTING event without typing or submitting again.

        SUBMITTING can only exist after the normal pre-submit freshness check
        established that the expected digest was absent. A later fresh browser
        document may therefore reconcile the delivery only when that same
        digest is now present exactly once and is the final user message in two
        separated observations on the exact stored conversation.
        """
        deadline = monotonic() + POST_SUBMIT_RECEIPT_WINDOW_SECONDS
        stable_observations = 0
        first_stable_at = None
        last_stage = "SUBMIT_RECEIPT_UNPROVEN"
        while monotonic() < deadline:
            try:
                current_url = str(cdp.get_current_url())
            except Exception as error:
                raise PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED") from error
            if not self.exact(current_url):
                raise PostSubmitUnknown(bounded_submit_target_drift_reason(current_url, self.url))
            try:
                if self.post_submit_visible_any(cdp, NETWORK):
                    sleeper(.25)
                    continue
            except Attention:
                sleeper(.25)
                continue
            try:
                current_count, turns_after = self.trusted_turn_anchor_state(cdp, message)
            except Attention:
                sleeper(.25)
                continue
            last_stage = self.turn_anchor_receipt_stage(0, current_count, turns_after)
            if last_stage == "RECEIPT_OK":
                now = monotonic()
                if stable_observations == 0:
                    stable_observations, first_stable_at = 1, now
                elif first_stable_at is not None and now > first_stable_at:
                    stable_observations = 2
                if stable_observations >= 2:
                    return DeliveryReceipt(
                        record_id=record_id,
                        browser_sent_at_unix=wall_time(),
                        message_sha256=expected,
                        target_sha256=target_sha256,
                    )
            elif last_stage == "SUBMIT_RECEIPT_SEQUENCE_DRIFT":
                raise PostSubmitUnknown(last_stage)
            else:
                stable_observations, first_stable_at = 0, None
            sleeper(.25)
        raise PostSubmitUnknown(last_stage)

    @staticmethod
    def select_send_control(cdp: Any) -> tuple[str, str | None]:
        # This follows the successful standalone CDP smoke path: use the fixed
        # current selectors directly with SeleniumBase's CDP click primitive.
        # De-duplicate DOM identities before considering ambiguity because one
        # current button legitimately exposes multiple supported attributes.
        script = f"""(() => {{
            const selectors = {json.dumps(SEND)};
            const matches = selectors.flatMap((selector, index) => Array.from(document.querySelectorAll(selector))
                .filter(button => {{
                const style = window.getComputedStyle(button);
                return button.getClientRects().length > 0 && style.visibility !== 'hidden' && style.display !== 'none';
                }})
                .map(button => ({{button, index}})));
            const unique = [];
            for (const match of matches) if (!unique.some(item => item.button === match.button)) unique.push(match);
            if (unique.length === 0) return ['missing', null];
            if (unique.length !== 1) return ['ambiguous', null];
            const button = unique[0].button;
            if (button.disabled || button.getAttribute('aria-disabled') === 'true') return ['disabled', null];
            return ['ready', selectors[unique[0].index]];
        }})()"""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("SEND_SELECTOR") from error
        if not isinstance(value, list) or len(value) != 2 or value[0] not in {"ready", "missing", "ambiguous", "disabled"}:
            return "ambiguous", None
        selector = value[1] if value[0] == "ready" and value[1] in SEND else None
        return value[0], selector
    def readiness_reason(self, cdp: Any, current: str | None = None) -> str | None:
        """Classify only pre-submit page state; terminal safety signals raise."""
        if current is None:
            try:
                current = str(cdp.get_current_url())
            except Exception:
                return "EDITOR_SELECTOR"
        if self.error(cdp, current):
            return "BROWSER_NETWORK_ERROR"
        if urlparse(current).path.startswith("/auth/"):
            raise Attention("LOGIN_OR_PROFILE_REQUIRED")
        if any(self.visible(cdp, s) for s in CAPTCHA):
            raise Attention("CAPTCHA_OR_SECURITY_CHECK")
        if not self.exact(current):
            # HOME can expose login controls while an authenticated shell is
            # hydrating. Route identity is authoritative here: before typing,
            # any non-target route is drift, not proof that authentication was
            # lost. The exact target is still re-checked fail-closed later.
            return "TARGET_DRIFT"
        if any(self.visible(cdp, s) for s in LOGIN):
            # On the exact protected conversation, a transient login control
            # may still settle during the bounded readiness window. A control
            # that persists is rejected again immediately before first write.
            return "LOGIN_OR_PROFILE_REQUIRED"
        if self.document_ready_state(cdp) == "loading":
            return "DOCUMENT_LOADING"
        try:
            editor_status, _editor_empty = self.editor_state(cdp)
        except Attention:
            return "EDITOR_SELECTOR"
        return None if editor_status == "ready" else "EDITOR_SELECTOR"

    @staticmethod
    def network_error_code(cdp: Any) -> str:
        """Read only Chrome's rendered error code; never export arbitrary text."""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(
                "document.querySelector('#error-code')?.textContent?.trim() || ''"
            ))
            return value if isinstance(value, str) and value in NETWORK_ERROR_CODES else "UNKNOWN"
        except Exception:
            return "UNKNOWN"

    @staticmethod
    def more_specific_readiness_reason(current: str, candidate: str) -> str:
        """Keep the most useful final transient diagnostic without page text."""
        priority = {"DOCUMENT_LOADING": 1, "EDITOR_SELECTOR": 2, "BROWSER_NETWORK_ERROR": 3, "LOGIN_OR_PROFILE_REQUIRED": 4, "TARGET_DRIFT": 5}
        return candidate if priority.get(candidate, 0) >= priority.get(current, 0) else current

    def retry_page_readiness(
        self,
        sb: Any,
        cdp: Any,
        observer: Callable[[str, str], None] | None = None,
        *,
        monotonic=time.monotonic,
        sleeper=time.sleep,
    ) -> Any:
        """Perform one bounded pre-submit recovery at a readiness boundary.

        A normal reload is preferred. Chrome can retain its internal
        chrome-error:// document after that reload, so only while the page is
        still positively classified as a network error may recovery reopen the
        same canonical target. No caller-selected URL or second browser
        context is introduced.
        """
        try:
            current = str(cdp.get_current_url())
        except Exception:
            current = None
        try:
            route = bounded_route_class(current, self.url) if current else "INVALID_ROUTE"
            if self.error(cdp, current) or route == "HOME":
                if observer is not None:
                    observer(route, "RECOVERY_BEGIN")
                # Navigate the already-active CDP tab in place. The proven
                # smoke-test path uses cdp.get(url); cdp.open(url) can create
                # or switch tab state, and production dev.62/dev.63 evidence
                # showed it returning to ChatGPT HOME instead of the exact
                # conversation. Never create a second context merely to recover.
                navigate = getattr(cdp, "get", None)
                if callable(navigate):
                    navigate(self.url)
                else:
                    sb.activate_cdp_mode(self.url)
                    replacement = getattr(sb, "cdp", None)
                    if replacement is not None:
                        cdp = replacement

                # SeleniumBase/CDP navigation can return before ChatGPT's
                # router settles. Live dev.65 evidence showed an immediate
                # HOME read even though recovery had just targeted the exact
                # conversation. Give the same active tab a bounded settle
                # window and report RECOVERY_RETURN only from the verified
                # post-navigation route.
                settle_deadline = monotonic() + HOME_RECOVERY_SETTLE_SECONDS
                recovered_url = ""
                recovered_route = "INVALID_ROUTE"
                while monotonic() < settle_deadline:
                    try:
                        recovered_url = str(cdp.get_current_url())
                    except Exception:
                        recovered_url = ""
                    recovered_route = (
                        bounded_route_class(recovered_url, self.url)
                        if recovered_url
                        else "INVALID_ROUTE"
                    )
                    if recovered_route != "HOME":
                        break
                    sleeper(
                        min(
                            READINESS_POLL_SECONDS,
                            max(0.0, settle_deadline - monotonic()),
                        )
                    )
                if observer is not None:
                    observer(recovered_route, "RECOVERY_RETURN")
                return cdp
        except Exception:
            # The next readiness window retains the bounded network/HOME
            # diagnostic; recovery never converts an exception into ready
            # without re-checking the exact target and editor.
            return cdp

        reload_page = getattr(cdp, "reload", None)
        if callable(reload_page):
            try:
                reload_page(ignore_cache=False)
            except Exception:
                pass
            return cdp
        try:
            sb.activate_cdp_mode(self.url)
            return sb.cdp
        except Exception:
            return cdp

    def wait_for_page_readiness(self, sb: Any, cdp: Any, *, monotonic=time.monotonic, sleeper=time.sleep, observer: Callable[[str, str], None] | None = None) -> Any:
        """Require a stable interactive ChatGPT page before the first browser write.

        Each attempt retains the existing 30-second readiness budget, but an
        exact target that becomes ready must remain continuously ready for an
        additional 10 seconds before Wake may type.  This absorbs late ChatGPT
        hydration/stream-control changes (for example a transient Stop button)
        instead of racing the page as soon as the composer first appears.

        An exact target whose document is complete but exposes no editor, send
        control, or generation control for 10 continuous seconds is treated as
        a blank shell and receives one normal reload at the bounded pre-submit
        recovery boundary.  No reload/relaunch is allowed after browser write.
        """
        final_reason = "EDITOR_SELECTOR"
        last_observation = None
        blank_shell_recovered = False
        for attempt in range(READINESS_ATTEMPTS):
            deadline = monotonic() + READINESS_WINDOW_SECONDS
            ready_since = None
            blank_shell_since = None
            recovered_early = False
            while monotonic() < deadline or ready_since is not None:
                now = monotonic()
                try:
                    current = str(cdp.get_current_url())
                except Exception:
                    current = ""
                reason = self.readiness_reason(cdp, current) if current else "EDITOR_SELECTOR"
                route = bounded_route_class(current, self.url) if current else "INVALID_ROUTE"
                observation = (route, "READY" if reason is None else reason)
                if observer is not None and observation != last_observation:
                    observer(*observation)
                    if reason == "BROWSER_NETWORK_ERROR":
                        observer(route, "NETWORK_" + self.network_error_code(cdp))
                    last_observation = observation

                if reason is None:
                    blank_shell_since = None
                    if ready_since is None:
                        ready_since = now
                    if now - ready_since >= POST_LOAD_STABLE_SECONDS:
                        return cdp
                    sleeper(READINESS_POLL_SECONDS)
                    continue

                ready_since = None
                final_reason = self.more_specific_readiness_reason(final_reason, reason)

                blank_shell = (
                    route == "SAME_CONVERSATION"
                    and reason == "EDITOR_SELECTOR"
                    and self.document_ready_state(cdp) == "complete"
                    and not any(self.visible(cdp, selector) for selector in STOP)
                    and not any(self.visible(cdp, selector) for selector in SEND)
                )
                if blank_shell:
                    if blank_shell_since is None:
                        blank_shell_since = now
                    elif (
                        now - blank_shell_since >= BLANK_SHELL_RELOAD_SECONDS
                        and not blank_shell_recovered
                        and attempt + 1 < READINESS_ATTEMPTS
                    ):
                        if observer is not None:
                            observer(route, "BLANK_SHELL_RECOVERY")
                        cdp = self.retry_page_readiness(sb, cdp, observer, monotonic=monotonic, sleeper=sleeper)
                        blank_shell_recovered = True
                        recovered_early = True
                        break
                else:
                    blank_shell_since = None

                # A true Chrome network-error document cannot hydrate into the
                # target and remains safe to recover immediately before write.
                if route == "CHROME_ERROR" and attempt + 1 < READINESS_ATTEMPTS:
                    cdp = self.retry_page_readiness(sb, cdp, observer, monotonic=monotonic, sleeper=sleeper)
                    recovered_early = True
                    break

                if now >= deadline:
                    break
                sleeper(min(READINESS_POLL_SECONDS, max(0.0, deadline - monotonic())))

            if recovered_early:
                continue
            # A persistent blank shell gets one recovery, then a full bounded
            # observation window. Do not fall through to the generic reload
            # budget and silently repeat the blank-shell recovery.
            if blank_shell_recovered:
                break
            if attempt + 1 < READINESS_ATTEMPTS:
                cdp = self.retry_page_readiness(sb, cdp, observer, monotonic=monotonic, sleeper=sleeper)
        raise Attention(final_reason)

    def wait_for_idle(self, cdp: Any, timeout: float, *, monotonic=time.monotonic, sleeper=time.sleep) -> None:
        """Require a continuous quiet window after any generation control disappears."""
        deadline = monotonic() + timeout
        idle_since = None
        while monotonic() < deadline or idle_since is not None:
            now = monotonic()
            if self.error(cdp):
                raise Attention("BROWSER_NETWORK_ERROR")
            if any(self.visible(cdp, selector) for selector in STOP):
                idle_since = None
            else:
                if idle_since is None:
                    idle_since = now
                if now - idle_since >= POST_LOAD_STABLE_SECONDS:
                    return
            if now >= deadline and idle_since is None:
                break
            sleeper(READINESS_POLL_SECONDS)
        raise Attention("CHATGPT_NOT_IDLE")
    def pre_typing_ready(self, cdp: Any) -> None:
        """Re-check exact target, idle state, and one empty editor before typing."""
        try:
            current = str(cdp.get_current_url())
        except Exception as error:
            raise Attention("EDITOR_SELECTOR") from error
        if self.error(cdp, current):
            raise Attention("BROWSER_NETWORK_ERROR")
        if urlparse(current).path.startswith("/auth/") or any(self.visible(cdp, s) for s in LOGIN):
            raise Attention("LOGIN_OR_PROFILE_REQUIRED")
        if any(self.visible(cdp, s) for s in CAPTCHA):
            raise Attention("CAPTCHA_OR_SECURITY_CHECK")
        if not self.exact(current):
            raise Attention("EDITOR_SELECTOR")
        if any(self.visible(cdp, selector) for selector in STOP):
            raise Attention("CHATGPT_NOT_IDLE")
        editor_status, editor_empty = self.editor_state(cdp)
        if editor_status != "ready":
            raise Attention("EDITOR_SELECTOR")
        if not editor_empty:
            raise Attention("EXISTING_DRAFT")

    @staticmethod
    def best_effort_present_browser(cdp: Any) -> None:
        """Present only the current SeleniumBase CDP target, once per wake.

        SeleniumBase 4.51.5 exposes both methods on its active CDP page.  They
        are intentionally best effort: Windows foreground-lock policy may
        reject the front request and neither failure may affect the W13 submit
        boundary or durable wake state.  There is no native window lookup, so
        this cannot select a browser window outside the current CDP target.
        """
        for method_name in ("maximize", "bring_active_window_to_front"):
            try:
                method = getattr(cdp, method_name, None)
                if callable(method):
                    method()
            except Exception:
                pass
        browser_pid = CdpSink.selenium_browser_pid(cdp)
        if browser_pid is not None:
            CdpSink.best_effort_windows_foreground(browser_pid)

    @staticmethod
    def selenium_browser_pid(cdp: Any) -> int | None:
        """Read only the current Selenium-owned browser PID, never discover one."""
        try:
            driver = getattr(cdp, "driver", None)
            browser = getattr(driver, "cdp_base", None) or driver
            pid = getattr(browser, "_process_pid", None)
            if pid is None:
                pid = getattr(browser, "browser_pid", None)
            if isinstance(pid, int) and not isinstance(pid, bool) and pid > 0:
                return pid
        except Exception:
            pass
        return None

    @staticmethod
    def best_effort_windows_foreground(browser_pid: int, *, platform_name: str | None = None, win32: Any = None) -> None:
        """Make one best-effort foreground request for one exact owned HWND.

        The optional seams keep this native-only path deterministic in tests.
        No PID, HWND, title, or failure detail leaves this helper.
        """
        if (not isinstance(browser_pid, int) or isinstance(browser_pid, bool) or browser_pid <= 0 or
                (platform_name if platform_name is not None else os.name) != "nt"):
            return
        try:
            api = win32 if win32 is not None else _WindowsUser32()
            matches: list[int] = []
            def candidate(hwnd: int) -> None:
                try:
                    hwnd = int(hwnd)
                    if (hwnd > 0 and api.is_suitable_top_level_window(hwnd) and
                            api.is_window_visible(hwnd) and
                            api.window_process_id(hwnd) == browser_pid):
                        matches.append(hwnd)
                except Exception:
                    pass
            api.enum_windows(candidate)
            if len(matches) != 1:
                return
            hwnd = matches[0]
            for operation in (api.show_maximized, api.bring_to_top, api.set_foreground):
                try:
                    operation(hwnd)
                except Exception:
                    pass
        except Exception:
            pass
    def wait_for_send_control(self, cdp: Any, timeout: float, *, monotonic=time.monotonic, sleeper=time.sleep) -> str | None:
        """Wait only for a single safe Send control; Enter needs full absence."""
        deadline = monotonic() + timeout
        saw_disabled = False
        while monotonic() < deadline:
            status, selector = self.select_send_control(cdp)
            if status == "ready":
                return selector
            if status == "ambiguous":
                raise Attention("SEND_SELECTOR")
            if status == "disabled":
                saw_disabled = True
            sleeper(.25)
        if saw_disabled:
            raise Attention("SEND_SELECTOR")
        return None
    @staticmethod
    def smoke_path_submit(
        cdp: Any,
        selector: str | None,
        timeout: float,
        editor_selector: str = EDITOR,
    ) -> None:
        # Native Enter is the standalone smoke fallback, but only if there was
        # no safe send button at all and the caller just verified exact text.
        try:
            if selector is None:
                cdp.press_keys(editor_selector, "\n", timeout=timeout)
            else:
                cdp.click(selector, timeout=timeout)
        except Exception as error:
            raise PostSubmitUnknown("SUBMIT_ENTER_UNKNOWN" if selector is None else "SUBMIT_CLICK_UNKNOWN") from error
    def exact(self, current: str) -> bool:
        try:
            # ChatGPT may canonicalize a plain /c/<id> URL into its
            # /g/<project>/c/<id> form (or vice versa) for the same chat.
            # The globally unique conversation ID is the protected identity;
            # every accepted current URL must still pass the strict ChatGPT
            # route/host/query validation above.
            return conversation_identity(current) == conversation_identity(self.url)
        except Attention:
            return False

    @staticmethod
    def close_owned_browser(sb: Any, cdp: Any = None) -> None:
        """Best-effort close only the active tab owned by this Wake attempt.

        SeleniumBase UC/CDP may share the ordinary Chrome process, so process
        termination or browser-wide quit is unsafe. CDP exposes an exact
        active-tab close operation for the page this attempt controls. Closing
        that tab on every success/attention path prevents orphan New Tab
        windows without touching unrelated user tabs or browser processes.
        Cleanup never changes delivery authority or permits replay.
        """
        owned_cdp = cdp if cdp is not None else getattr(sb, "cdp", None)
        close_tab = getattr(owned_cdp, "close_active_tab", None)
        if not callable(close_tab):
            return
        try:
            close_tab()
        except Exception:
            pass

    def wake(self, record_id: str, message: str, still_actionable: Callable[[], bool], before_submit: Callable[[], None]) -> DeliveryReceipt:
        from seleniumbase import SB
        typed_message = keyboard_safe_message(message)
        # Match the proven smoke-test resilience boundary: an intermittent
        # Chrome/network failure may discard and recreate the browser session
        # with the SAME durable profile and exact protected target. This outer
        # retry exists only before the durable submission boundary. Every other safety
        # classification (login, CAPTCHA, target drift, draft, ambiguity) and
        # every failure after typing remains single-attempt/fail-closed.
        for session_attempt in range(READINESS_ATTEMPTS):
            browser_write_started = False
            try:
                # ``test=True`` swallows body exceptions in SeleniumBase's SB
                # context manager, so production deliberately leaves it off.
                with SB(uc=True, user_data_dir=str(self.profile)) as sb:
                    cdp = None
                    try:
                        sb.activate_cdp_mode(self.url)
                        cdp = self.wait_for_page_readiness(sb, sb.cdp)
                        # This is a fresh, later safety timer; it intentionally cannot use
                        # any attempt-local page readiness deadline.
                        self.wait_for_idle(cdp, self.ui_timeout)
                        self.pre_typing_ready(cdp)
                        if not still_actionable(): raise Attention("WAKE_CANCELLED")
                        expected_digest = digest(normalize_message(message))
                        prior_message_digests = self.pre_submit_receipt_snapshot(cdp, expected_digest)
                        self.best_effort_present_browser(cdp)
                        # Presentation is allowed to fail, but may never weaken the exact
                        # target/editor contract before the first browser write.
                        self.pre_typing_ready(cdp)
                        # CDP text entry can synthesize submit keys. Persist the
                        # caller's uncertain-submission boundary before any write.
                        editor_selector = self.editor_selector(cdp, prevalidated=True)
                        browser_write_started = True
                        before_submit()
                        if not self.exact(cdp.get_current_url()): raise Attention("TARGET_IDENTITY_MISMATCH")
                        self.pre_typing_ready(cdp)
                        editor_selector = self.editor_selector(cdp, prevalidated=True)
                        cdp.send_keys(editor_selector, typed_message, timeout=self.ui_timeout)
                        if normalize_message(self.text(cdp)) != normalize_message(message): raise Attention("TYPING_NOT_CONFIRMED")
                        send_selector = self.wait_for_send_control(cdp, min(self.ui_timeout, 20))
                        if normalize_message(self.text(cdp)) != normalize_message(message): raise Attention("TYPING_NOT_CONFIRMED")
                        self.smoke_path_submit(cdp, send_selector, self.ui_timeout, editor_selector)
                        accepted_at = self.wait_for_submission_accepted(
                            cdp, prior_message_digests, expected_digest, message
                        )
                        # Preserve the same owned browser/profile/conversation
                        # throughout the assistant lifecycle. Timeout Retry stays in
                        # place, while a long-lived active Stop/Pause state may receive
                        # bounded same-target refreshes so stale client UI cannot wedge
                        # completion. No fresh browser or USER re-submit is permitted.
                        self.wait_for_response_completion(cdp, expected_digest)
                        # Only after deterministic assistant completion is it safe
                        # to replace this document.  The fresh load proves the USER
                        # turn persisted independently of the sender's optimistic DOM.
                        self.durable_receipt_round_trip(cdp)
                        return self.confirm_exact_receipt(
                            cdp,
                            prior_message_digests,
                            expected_digest,
                            record_id,
                            message,
                            wall_time=lambda: accepted_at,
                        )
                    finally:
                        # Every attempt owns its browser session regardless of
                        # success or attention. Cleanup is not delivery authority.
                        self.close_owned_browser(sb, cdp)
            except Attention as error:
                if (browser_write_started or str(error) != "BROWSER_NETWORK_ERROR" or
                        session_attempt + 1 >= READINESS_ATTEMPTS):
                    raise
                time.sleep(READINESS_POLL_SECONDS)
        raise Attention("BROWSER_NETWORK_ERROR")

class NewConversationSink(CdpSink):
    """One owned, empty ChatGPT home -> one issued conversation; never binds Wake."""

    def __init__(self, source_url: str, profile: Path, ui_timeout: float, submit_timeout: float):
        super().__init__(source_url, profile, ui_timeout, submit_timeout)
        self.source_url = self.url
        self.destination_url: str | None = None

    def pre_typing_ready(self, cdp: Any) -> None:
        super().pre_typing_ready(cdp)
        self.require_usable_latest_assistant(cdp, source=True)

    @staticmethod
    def visible_role_counts(cdp: Any) -> tuple[int, int, int]:
        """Read bounded structural counts only; no transcript text leaves CDP."""
        script = """(() => {
            const visible = element => {
                const style = window.getComputedStyle(element);
                return element.getClientRects().length > 0 &&
                    style.visibility !== 'hidden' && style.display !== 'none';
            };
            const count = selector => Array.from(new Set(
                document.querySelectorAll(selector)
            )).filter(visible).length;
            return [
                count("[data-message-author-role='user'], article[data-turn='user'], [data-user-message-bubble]"),
                count("[data-message-author-role='assistant'], article[data-turn='assistant'], [data-content-search-unit-key]:has(h4[data-conversation-role='assistant'])"),
                count("[data-content-search-turn-key], [data-testid^='conversation-turn-'], article[id^='conversation-turn-'], [data-content-search-unit-key]"),
            ];
        })()"""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("BOOTSTRAP_TURN_QUERY_FAILED") from error
        if (not isinstance(value, list) or len(value) != 3 or
                any(not isinstance(count, int) or isinstance(count, bool) or count < 0 or count > MAX_USER_MESSAGES
                    for count in value)):
            raise Attention("BOOTSTRAP_TURN_QUERY_FAILED")
        return value[0], value[1], value[2]

    def message_body_diagnostics(self, cdp: Any) -> dict[str, Any]:
        """Bounded private DOM shape only; never promotes incomplete text."""
        if not self.exact(str(cdp.get_current_url())):
            raise Attention("SOURCE_SNAPSHOT_SOURCE_DRIFT")
        script = """(() => {
            const visible = node => {
                const style = window.getComputedStyle(node);
                return node.getClientRects().length > 0 &&
                    style.visibility !== 'hidden' && style.display !== 'none';
            };
            const shape = node => {
                const style = window.getComputedStyle(node);
                return [String(node.tagName || '').slice(0, 16),
                    String(node.getAttribute('class') || '').slice(0, 96),
                    String(node.getAttribute('data-markdown-text-style') || '').slice(0, 64),
                    String(style.display || '').slice(0, 32),
                    String(style.visibility || '').slice(0, 16),
                    Math.min(node.getClientRects().length, 32)];
            };
            let tier = 'AUTHOR_ROLE';
            let nodes = Array.from(document.querySelectorAll(
                '[data-message-author-role="assistant"]')).filter(visible);
            if (!nodes.length) {
                tier = 'ARTICLE_ROLE';
                nodes = Array.from(document.querySelectorAll('article[data-turn="assistant"]')).filter(visible);
            }
            if (!nodes.length) {
                tier = 'SEMANTIC_UNIT';
                nodes = Array.from(document.querySelectorAll('[data-content-search-unit-key]'))
                    .filter(node => visible(node) && node.querySelector('h4[data-conversation-role="assistant"]') &&
                        !node.querySelector('[data-user-message-bubble]'));
            }
            const node = nodes[nodes.length - 1];
            if (!node) return null;
            const selector = '[data-message-content], .markdown, [class~="prose"], [data-markdown-text-style="assistant-message"]';
            const bodies = (node.matches(selector) ? [node] : [])
                .concat(Array.from(node.querySelectorAll(selector)));
            const ancestors = [];
            for (let parent = node.parentElement; parent && ancestors.length < 2; parent = parent.parentElement)
                ancestors.push(shape(parent));
            return {selectorTier:tier, roleNodeCount:Math.min(nodes.length, 1024),
                selected:shape(node), bodyCount:Math.min(bodies.length, 1024),
                visibleBodyCount:Math.min(bodies.filter(visible).length, 1024),
                bodies:bodies.slice(0, 2).map(shape), ancestors,
                descendants:Array.from(node.querySelectorAll('*')).slice(0, 4).map(shape)};
        })()"""
        try:
            value = self.cdp_value(cdp.evaluate(script))
            if (not isinstance(value, dict) or set(value) != {
                    "selectorTier", "roleNodeCount", "selected", "bodyCount",
                    "visibleBodyCount", "bodies", "ancestors", "descendants"} or
                    value["selectorTier"] not in {"AUTHOR_ROLE", "ARTICLE_ROLE", "SEMANTIC_UNIT"} or
                    any(type(value[key]) is not int or not 0 <= value[key] <= 1024
                        for key in ("roleNodeCount", "bodyCount", "visibleBodyCount"))):
                return {"status": "UNAVAILABLE"}
            for key, maximum in (("bodies", 2), ("ancestors", 2), ("descendants", 4)):
                if not isinstance(value[key], list) or len(value[key]) > maximum:
                    return {"status": "UNAVAILABLE"}
            for shape in [value["selected"], *value["bodies"], *value["ancestors"], *value["descendants"]]:
                if (not isinstance(shape, list) or len(shape) != 6 or
                        any(not isinstance(part, str) or len(part) > limit
                            for part, limit in zip(shape[:5], (16, 96, 64, 32, 16))) or
                        type(shape[5]) is not int or not 0 <= shape[5] <= 32):
                    return {"status": "UNAVAILABLE"}
            if len(json.dumps(value).encode("utf-8")) > 3072:
                return {"status": "UNAVAILABLE"}
        except Exception:
            return {"status": "UNAVAILABLE"}
        if not self.exact(str(cdp.get_current_url())):
            raise Attention("SOURCE_SNAPSHOT_SOURCE_DRIFT")
        return {"status": "OBSERVED", **value}

    def visible_message_snapshot(self, cdp: Any) -> dict[str, Any]:
        """Return bounded private text and qualified identities for visible role units.

        DOM keys stay inside this process. A hash is an observation aid, not a
        claim that ChatGPT will preserve that key across a future document.
        """
        script = """(() => {
            const visible = element => {
                const style = window.getComputedStyle(element);
                return element.getClientRects().length > 0 &&
                    style.visibility !== 'hidden' && style.display !== 'none';
            };
            const role = node => {
                const explicit = node.getAttribute('data-message-author-role') || node.getAttribute('data-turn');
                if (explicit) return explicit;
                // The current Web UI identifies each message by a search unit,
                // with the USER bubble or ASSISTANT heading inside it. A paired
                // wrapper containing both roles must never become one message.
                const user = Boolean(node.querySelector('[data-user-message-bubble]'));
                const assistant = Boolean(node.querySelector('h4[data-conversation-role="assistant"]'));
                return user !== assistant ? (user ? 'user' : 'assistant') : null;
            };
            const anchor = (node, name) => {
                const selector = '[' + name + ']';
                const holder = node.closest(selector) || node.querySelector(selector);
                const value = holder && holder.getAttribute(name);
                return value && value.length <= 256 ? value : null;
            };
            const roles = Array.from(document.querySelectorAll(
                "[data-message-author-role='user'], [data-message-author-role='assistant']"
            )).filter(visible);
            const articles = Array.from(document.querySelectorAll(
                "article[data-turn='user'], article[data-turn='assistant']"
            )).filter(visible);
            const units = Array.from(document.querySelectorAll('[data-content-search-unit-key]'))
                .filter(node => visible(node) && role(node));
            const tier = roles.length ? 'AUTHOR_ROLE' : articles.length ? 'ARTICLE_ROLE' :
                units.length ? 'SEMANTIC_UNIT' : 'NONE';
            const candidates = roles.length ? roles : articles.length ? articles : units;
            const candidateSet = new Set(candidates);
            const nested = new Set();
            for (const node of candidates) {
                let parent = node.parentElement;
                while (parent) {
                    if (candidateSet.has(parent) && role(parent) === role(node)) nested.add(parent);
                    parent = parent.parentElement;
                }
            }
            const innermost = candidates.filter(node => !nested.has(node));
            const keyed = new Map();
            const rows = [];
            let keyedCollapsed = 0;
            for (const node of innermost) {
                const unitKey = anchor(node, 'data-content-search-unit-key');
                const messageId = anchor(node, 'data-chatgpt-selection-message-id');
                const key = unitKey ? 'unit:' + role(node) + ':' + unitKey :
                    messageId ? 'message:' + role(node) + ':' + messageId : null;
                // Match the dedicated USER body used by exact delivery receipts.
                // Its surrounding bubble can append an ellipsis/Show more UI.
                const contentSelector = role(node) === 'user' ?
                    '[data-message-content], [data-markdown-text-tone="user-message"], .whitespace-pre-wrap' :
                    '[data-message-content], .markdown, [class~="prose"], [data-markdown-text-style="assistant-message"]';
                const content = node.matches(contentSelector) ? node : node.querySelector(contentSelector);
                const completeBody = Boolean(content && visible(content));
                let text = '';
                if (completeBody) {
                    const bodyCopy = content.cloneNode(true);
                    bodyCopy.querySelectorAll('button, [role="button"], [role="toolbar"], [aria-hidden="true"]')
                        .forEach(control => control.remove());
                    text = String(bodyCopy.textContent || '');
                } else {
                    // Retain only a bounded diagnostic excerpt for a missing
                    // body. It must never be counted as complete answer text.
                    text = String(node.innerText || node.textContent || '');
                }
                const bodyProven = completeBody;
                let excerpt = text.slice(-4096);
                // JavaScript slices UTF-16 units. A tail can begin with half
                // of an emoji; omit that half before Python's UTF-8 encoding.
                const firstUnit = excerpt.charCodeAt(0);
                if (firstUnit >= 0xDC00 && firstUnit <= 0xDFFF) excerpt = excerpt.slice(1);
                const row = [role(node), excerpt, bodyProven,
                    Math.min(text.length, 1000000), anchor(node, 'data-content-search-turn-key'),
                    unitKey, messageId];
                if (key && keyed.has(key)) {
                    rows[keyed.get(key)] = null;
                    keyedCollapsed++;
                }
                if (key) keyed.set(key, rows.length);
                rows.push(row);
            }
            const unique = rows.filter(row => row !== null);
            return [tier, Math.min(unique.length, 1024), unique.length > 1024,
                Math.min(nested.size, 1024), Math.min(keyedCollapsed, 1024), unique.slice(-16)];
        })()"""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("SOURCE_SNAPSHOT_QUERY_FAILED") from error
        if (not isinstance(value, list) or len(value) != 6 or
                value[0] not in {"AUTHOR_ROLE", "ARTICLE_ROLE", "SEMANTIC_UNIT", "NONE"} or
                any(not isinstance(count, int) or isinstance(count, bool) or count < 0 or count > 1024
                    for count in (value[1], value[3], value[4])) or
                not isinstance(value[2], bool) or not isinstance(value[5], list) or
                len(value[5]) > 16 or value[1] < len(value[5])):
            raise Attention("SOURCE_SNAPSHOT_QUERY_FAILED")
        truncated = value[2] or value[1] > len(value[5])
        remaining = 8192
        messages = []
        source_salt = digest(self.url)
        for item in reversed(value[5]):
            if (not isinstance(item, list) or len(item) != 7 or
                    item[0] not in {"user", "assistant"} or
                    not isinstance(item[1], str) or len(item[1]) > 4096 or
                    not isinstance(item[2], bool) or
                    not isinstance(item[3], int) or isinstance(item[3], bool) or not 0 <= item[3] <= 1000000 or
                    any(key is not None and (not isinstance(key, str) or not 0 < len(key) <= 256)
                        for key in item[4:])):
                raise Attention("SOURCE_SNAPSHOT_QUERY_FAILED")
            text_bytes = item[1].encode("utf-8")
            clipped = text_bytes[:min(4096, remaining)].decode("utf-8", errors="ignore")
            complete_body = item[2] and item[3] <= 4096
            text_complete = complete_body and len(text_bytes) == len(clipped.encode("utf-8"))
            if not text_complete:
                truncated = True
            if not clipped and text_bytes:
                break
            remaining -= len(clipped.encode("utf-8"))
            role_name = item[0].upper()
            turn_key, unit_key, message_id = item[4:]
            if unit_key:
                identity_basis = "UNIT_KEY"
                message_key = ("unit", unit_key)
            elif message_id:
                identity_basis = "MESSAGE_ID"
                message_key = ("message", message_id)
            elif turn_key:
                identity_basis = "ROLE_TURN_ONLY"
                message_key = None
            else:
                identity_basis = "MISSING"
                message_key = None
            messages.append({
                "role": role_name,
                "text": clipped,
                "visibleUnits": item[3],
                "textComplete": text_complete,
                "bodyTextObserved": item[2],
                "turnHash": digest(f"catdesk-snapshot-turn-v1\0{source_salt}\0{turn_key}") if turn_key else None,
                "roleTurnHash": digest(f"catdesk-snapshot-role-turn-v1\0{source_salt}\0{role_name}\0{turn_key}")
                    if turn_key else None,
                "messageHash": digest(f"catdesk-snapshot-message-v1\0{source_salt}\0{role_name}\0{message_key[0]}\0{message_key[1]}")
                    if message_key else None,
                "identityBasis": identity_basis,
                "revisionHash": digest(f"catdesk-snapshot-revision-v1\0{source_salt}\0{item[1]}")
                    if complete_body else None,
                "revisionComplete": complete_body,
            })
            if remaining == 0:
                truncated = True
                break
        messages.reverse()
        omitted = max(0, value[1] - len(messages))
        coverage = {
            "selectorTier": value[0],
            "visibleRoleCount": value[1],
            "countOverflow": value[2],
            "nestedCollapsed": value[3],
            "keyedCollapsed": value[4],
            "omittedBeforeWindow": omitted,
            "returned": len(messages),
            "identityMissing": sum(item["identityBasis"] == "MISSING" for item in messages),
            "identityUncertain": sum(item["identityBasis"] == "ROLE_TURN_ONLY" for item in messages),
            "revisionIncomplete": sum(not item["revisionComplete"] for item in messages),
            "textIncomplete": sum(not item["textComplete"] for item in messages),
            "anchorStability": "UNVERIFIED",
        }
        return {"messages": messages, "truncated": truncated or omitted > 0, "coverage": coverage}

    def require_usable_latest_assistant(self, cdp: Any, *, source: bool) -> None:
        """Fail closed if a source or freshly reloaded destination lost its answer."""
        unavailable = "BOOTSTRAP_SOURCE_ASSISTANT_UNAVAILABLE" if source else "BOOTSTRAP_ASSISTANT_UNAVAILABLE"
        load_error = "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER" if source else "BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER"
        error_type = Attention if source else PostSubmitUnknown
        try:
            messages = self.visible_message_snapshot(cdp)["messages"]
        except Attention as error:
            raise error_type(unavailable) from error
        if not messages or messages[-1]["role"] != "ASSISTANT":
            raise error_type(unavailable)
        body = normalize_message(messages[-1]["text"]).replace("’", "'").replace("‘", "'")
        if "this response couldn't load" in body.lower():
            raise error_type(load_error)
        if not body or not messages[-1]["textComplete"] or not messages[-1]["revisionComplete"]:
            raise error_type(unavailable)

    def check_empty_home(self, cdp: Any) -> None:
        try:
            current = str(cdp.get_current_url())
        except Exception as error:
            raise Attention("BOOTSTRAP_HOME_DRIFT") from error
        if current != NEW_CONVERSATION_HOME:
            raise Attention("BOOTSTRAP_HOME_DRIFT")
        if self.error(cdp, current):
            raise Attention("BROWSER_NETWORK_ERROR")
        if any(self.visible(cdp, selector) for selector in LOGIN):
            raise Attention("LOGIN_OR_PROFILE_REQUIRED")
        if any(self.visible(cdp, selector) for selector in CAPTCHA):
            raise Attention("CAPTCHA_OR_SECURITY_CHECK")
        if any(self.visible(cdp, selector) for selector in STOP):
            raise Attention("CHATGPT_NOT_IDLE")
        if self.document_ready_state(cdp) == "loading":
            raise Attention("BOOTSTRAP_HOME_NOT_READY")
        editor_status, editor_empty = self.editor_state(cdp)
        if editor_status != "ready":
            raise Attention("BOOTSTRAP_HOME_NOT_READY")
        if not editor_empty:
            raise Attention("EXISTING_DRAFT")
        if any(self.visible_role_counts(cdp)):
            raise Attention("BOOTSTRAP_HOME_HAS_TURNS")

    def observe_home_draft(self, cdp: Any, expected_message: str) -> dict[str, Any]:
        """Classify one owned home composer without writing or returning its text."""
        try:
            current = str(cdp.get_current_url())
        except Exception as error:
            raise Attention("BOOTSTRAP_HOME_DRIFT") from error
        if current != NEW_CONVERSATION_HOME:
            raise Attention("BOOTSTRAP_HOME_DRIFT")
        if self.error(cdp, current):
            raise Attention("BROWSER_NETWORK_ERROR")
        if any(self.visible(cdp, selector) for selector in LOGIN):
            raise Attention("LOGIN_OR_PROFILE_REQUIRED")
        if any(self.visible(cdp, selector) for selector in CAPTCHA):
            raise Attention("CAPTCHA_OR_SECURITY_CHECK")
        if any(self.visible(cdp, selector) for selector in STOP):
            raise Attention("CHATGPT_NOT_IDLE")
        if self.document_ready_state(cdp) == "loading":
            raise Attention("BOOTSTRAP_HOME_NOT_READY")
        if any(self.visible_role_counts(cdp)):
            raise Attention("BOOTSTRAP_HOME_HAS_TURNS")
        status, empty = self.editor_state(cdp)
        if status != "ready":
            raise Attention("BOOTSTRAP_HOME_NOT_READY")
        if empty:
            return {"classification": "EMPTY_HOME", "actualDigest": None, "actualBytes": 0}
        actual = self.text(cdp)
        if not actual:
            raise Attention("BOOTSTRAP_DRAFT_UNREADABLE")
        actual_bytes = actual.encode("utf-8")
        if len(actual_bytes) > MAX_USER_MESSAGE_CHARS:
            return {"classification": "MISMATCH", "actualDigest": None,
                    "actualBytes": MAX_USER_MESSAGE_CHARS}
        normalized = normalize_message(actual)
        return {
            "classification": "EXACT_OWNED" if normalized == normalize_message(expected_message) else "MISMATCH",
            "actualDigest": digest(normalized), "actualBytes": len(actual_bytes),
        }

    def wait_home_draft(self, cdp: Any, expected_message: str, *,
                        monotonic=time.monotonic, sleeper=time.sleep) -> dict[str, Any]:
        """Wait only for readiness; never wait through a foreign draft or turn."""
        deadline = monotonic() + self.ui_timeout
        empty_since = None
        while True:
            try:
                observed = self.observe_home_draft(cdp, expected_message)
            except Attention as error:
                if str(error) != "BOOTSTRAP_HOME_NOT_READY" or monotonic() >= deadline:
                    raise
                sleeper(READINESS_POLL_SECONDS)
                continue
            if observed["classification"] != "EMPTY_HOME":
                return observed
            now = monotonic()
            if empty_since is None:
                empty_since = now
            # A fresh browser may show an empty editor before a saved draft
            # hydrates. Require a short stable empty observation before any
            # caller treats it as absent; the caller still cannot type on a
            # resumed intent merely because the home is empty.
            if now - empty_since >= 0.5:
                return observed
            if now >= deadline:
                raise Attention("BOOTSTRAP_HOME_NOT_READY")
            sleeper(min(READINESS_POLL_SECONDS, deadline - now))

    def raw_editor_text(self, cdp: Any) -> str:
        """Read the actual DOM text, preserving spaces and line breaks."""
        script = """(() => {
            const visible = Array.from(document.querySelectorAll(
                '#prompt-textarea, div.ProseMirror[contenteditable="true"], [contenteditable="true"][data-lexical-editor="true"]'
            )).filter(element => {
                const style = window.getComputedStyle(element);
                return element.getClientRects().length > 0 && style.visibility !== 'hidden' && style.display !== 'none';
            });
            if (visible.length !== 1) return ['count'];
            const element = visible[0];
            const text = element.value ?? element.innerText ?? element.textContent ?? '';
            if (new TextEncoder().encode(text).length > 16384) return ['oversize'];
            return ['ready', text];
        })()"""
        try:
            value = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("BOOTSTRAP_DRAFT_UNREADABLE") from error
        if value == ["oversize"]:
            raise Attention("BOOTSTRAP_ARCHIVE_TEXT_TOO_LARGE")
        if (not isinstance(value, list) or len(value) != 2 or value[0] != "ready" or
                not isinstance(value[1], str) or not value[1].strip() or
                len(value[1].encode("utf-8")) > MAX_USER_MESSAGE_CHARS):
            raise Attention("BOOTSTRAP_DRAFT_UNREADABLE")
        return value[1]

    def clear_archived_home_draft(self, cdp: Any, expected_message: str, actual_text: str) -> None:
        """Clear only the exact owned composer after the durable archive ACK."""
        self.check_typed_home(cdp, expected_message)
        if (not isinstance(actual_text, str) or
                normalize_message(actual_text) != normalize_message(expected_message)):
            raise Attention("BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED")
        # The pinned CDP clear_input() only sets `.value`, which does not clear
        # ChatGPT's contenteditable composer. Select its contents, then issue
        # the browser's native edit command. The identity, URL, turn and text
        # recheck is in the same DOM evaluation as the edit.
        script = f"""(() => {{
            if (location.href !== {json.dumps(NEW_CONVERSATION_HOME)}) return 'UNAVAILABLE';
            if (document.querySelector(
                '[data-message-author-role="user"], [data-message-author-role="assistant"], '
                + 'article[data-turn="user"], article[data-turn="assistant"], '
                + '[data-user-message-bubble], h4[data-conversation-role="assistant"]'
            )) return 'UNAVAILABLE';
            const visible = Array.from(document.querySelectorAll(
                '#prompt-textarea, div.ProseMirror[contenteditable="true"], [contenteditable="true"][data-lexical-editor="true"]'
            )).filter(element => {{
                const style = window.getComputedStyle(element);
                return element.getClientRects().length > 0 && style.visibility !== 'hidden' && style.display !== 'none';
            }});
            if (visible.length !== 1) return 'UNAVAILABLE';
            const editor = visible[0];
            const current = editor.value ?? editor.innerText ?? editor.textContent ?? '';
            if (current !== {json.dumps(actual_text)}) return 'UNAVAILABLE';
            editor.focus();
            if (editor.isContentEditable) {{
                const range = document.createRange();
                range.selectNodeContents(editor);
                const selection = window.getSelection();
                selection.removeAllRanges();
                selection.addRange(range);
                return document.execCommand('delete', false, null) ? 'CLEARED' : 'UNAVAILABLE';
            }}
            if (editor instanceof HTMLTextAreaElement) {{
                editor.setRangeText('', 0, editor.value.length, 'start');
                editor.dispatchEvent(new InputEvent('input', {{bubbles: true, inputType: 'deleteContent'}}));
                return 'CLEARED';
            }}
            return 'UNAVAILABLE';
        }})()"""
        try:
            result = CdpSink.cdp_value(cdp.evaluate(script))
        except Exception as error:
            raise Attention("BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED") from error
        if result != "CLEARED":
            raise Attention("BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED")
        self.check_empty_home(cdp)
        try:
            cdp.get(NEW_CONVERSATION_HOME)
            reloaded = self.wait_home_draft(cdp, expected_message)
        except Exception as error:
            raise Attention("BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED") from error
        if reloaded["classification"] != "EMPTY_HOME":
            raise Attention("BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED")

    def open_empty_home(self, cdp: Any, *, monotonic=time.monotonic, sleeper=time.sleep) -> None:
        try:
            cdp.get(NEW_CONVERSATION_HOME)
        except Exception as error:
            raise Attention("BOOTSTRAP_HOME_NAVIGATION_FAILED") from error
        deadline = monotonic() + self.ui_timeout
        while monotonic() < deadline:
            try:
                self.check_empty_home(cdp)
                return
            except Attention as error:
                if str(error) != "BOOTSTRAP_HOME_NOT_READY":
                    raise
            sleeper(READINESS_POLL_SECONDS)
        raise Attention("BOOTSTRAP_HOME_NOT_READY")

    def check_typed_home(self, cdp: Any, message: str) -> None:
        """Repeat home and exact-draft guards before the intentional send."""
        try:
            current = str(cdp.get_current_url())
        except Exception as error:
            raise Attention("BOOTSTRAP_HOME_DRIFT") from error
        if current != NEW_CONVERSATION_HOME:
            raise Attention("BOOTSTRAP_HOME_DRIFT")
        if self.error(cdp, current):
            raise Attention("BROWSER_NETWORK_ERROR")
        if any(self.visible(cdp, selector) for selector in LOGIN):
            raise Attention("LOGIN_OR_PROFILE_REQUIRED")
        if any(self.visible(cdp, selector) for selector in CAPTCHA):
            raise Attention("CAPTCHA_OR_SECURITY_CHECK")
        if any(self.visible(cdp, selector) for selector in STOP):
            raise Attention("CHATGPT_NOT_IDLE")
        if any(self.visible_role_counts(cdp)):
            raise Attention("BOOTSTRAP_HOME_HAS_TURNS")
        status, empty = self.editor_state(cdp)
        if status != "ready" or empty:
            raise Attention("BOOTSTRAP_EDITOR_DRIFT")
        if normalize_message(self.text(cdp)) != normalize_message(message):
            raise Attention("BOOTSTRAP_DRAFT_DRIFT")

    def latch_issued_url(self, cdp: Any, *, monotonic=time.monotonic, sleeper=time.sleep) -> str:
        """Latch only a browser-issued, canonical conversation ID unlike source."""
        deadline = monotonic() + max(self.submit_timeout, POST_SUBMIT_RECEIPT_WINDOW_SECONDS)
        while monotonic() < deadline:
            try:
                current = str(cdp.get_current_url())
            except Exception as error:
                raise PostSubmitUnknown("BOOTSTRAP_URL_QUERY_FAILED") from error
            if current == NEW_CONVERSATION_HOME:
                sleeper(READINESS_POLL_SECONDS)
                continue
            try:
                canonical = canonical_conversation_url(current)
            except Attention as error:
                raise PostSubmitUnknown("BOOTSTRAP_URL_INVALID") from error
            if canonical != current:
                raise PostSubmitUnknown("BOOTSTRAP_URL_INVALID")
            identity = conversation_identity(canonical)
            if identity == conversation_identity(self.source_url):
                raise PostSubmitUnknown("BOOTSTRAP_SOURCE_REVISITED")
            if self.destination_url is not None and identity != conversation_identity(self.destination_url):
                raise PostSubmitUnknown("BOOTSTRAP_DESTINATION_DRIFT")
            self.destination_url = canonical
            self.url = canonical
            return canonical
        raise PostSubmitUnknown("BOOTSTRAP_URL_UNISSUED")

    def prove_single_bootstrap_user(self, cdp: Any, message: str) -> None:
        """After a fresh document, require exactly one full USER digest."""
        expected = digest(normalize_message(message))
        try:
            user_digests = self.user_message_digests(cdp)
            users, assistants, _turns = self.visible_role_counts(cdp)
        except Attention as error:
            raise PostSubmitUnknown("BOOTSTRAP_RECEIPT_QUERY_FAILED") from error
        if user_digests != [expected] or users > 1 or assistants > 1:
            raise PostSubmitUnknown("BOOTSTRAP_RECEIPT_SEQUENCE_DRIFT")
        self.require_usable_latest_assistant(cdp, source=False)

def delivery_from_raw(value: Any) -> Delivery | None:
    if not isinstance(value, dict): return None
    try:
        if not isinstance(value.get("record_id"), str) or not isinstance(value.get("status"), str) or not isinstance(value.get("claimed_at_unix"), (int, float)): return None
        return Delivery(
            value["record_id"], value["status"], float(value["claimed_at_unix"]), value.get("browser_sent_at_unix"),
            value.get("message_sha256"), value.get("target_sha256"), value.get("receipt_schema_version"), value.get("attention"),
        )
    except (TypeError, ValueError): return None

def invalid_claimed_post_submit_state(delivery: Delivery) -> bool:
    """A claim is pre-submit only and cannot carry receipt/submit attention."""
    return delivery.status == "CLAIMED" and (
        delivery.attention is not None or delivery.browser_sent_at_unix is not None or
        delivery.message_sha256 is not None or delivery.target_sha256 is not None or
        delivery.receipt_schema_version is not None
    )

def main() -> int:
    parser = argparse.ArgumentParser(); parser.add_argument("--workspace", default="."); parser.add_argument("--config"); parser.add_argument("--record-id", required=True); parser.add_argument("--bridge-sha256", required=True); parser.add_argument("--conversation-url"); args = parser.parse_args()
    root = Path(args.workspace).resolve(); state_path = root / ".catdesk" / "wake-bridge" / "state.json"
    try: validate_bridge_identity(root, args.bridge_sha256)
    except Attention: print("OPERATOR_ATTENTION"); return 2
    if not legacy_owner_selected(root): print("WAKE_OWNER_RUST_SELECTED"); return 2
    if not actionable(root, args.record_id): print("WAKE_CANCELLED"); return 4
    config = load(workspace_path(root, args.config) if args.config else root / ".catdesk" / "wake-bridge" / "config.json", {})
    try:
        profile = workspace_path(root, config["profile_dir"]); sink = CdpSink(args.conversation_url or config["conversation_url"], profile, float(config.get("ui_ready_timeout_seconds", 20)), float(config.get("send_confirmation_timeout_seconds", 15)))
    except (KeyError, TypeError, ValueError, Attention): print("OPERATOR_ATTENTION"); return 2
    try:
        with Singleton(root / ".catdesk" / "wake-bridge" / "wake-bridge.lock"):
            if not legacy_owner_selected(root): print("WAKE_OWNER_RUST_SELECTED"); return 2
            raw = load(state_path, {})
            raw_deliveries = raw.get("deliveries", []) if isinstance(raw, dict) and isinstance(raw.get("deliveries", []), list) else []
            state = State(deliveries=[d for d in (delivery_from_raw(item) for item in raw_deliveries) if d is not None])
            message = MESSAGE.format(record_id=args.record_id)
            invalid_claim = next((d for d in state.deliveries if d.record_id == args.record_id and invalid_claimed_post_submit_state(d)), None)
            if invalid_claim:
                invalid_claim.status = "OPERATOR_ATTENTION"; invalid_claim.attention = "INVALID_CLAIMED_POST_SUBMIT_STATE"
                state.operator_attention = "INVALID_CLAIMED_POST_SUBMIT_STATE"; save(state_path, asdict(state)); print("OPERATOR_ATTENTION"); return 2
            prior_sent = next((d for d in state.deliveries if d.record_id == args.record_id and d.status == "SENT"), None)
            if prior_sent:
                if valid_receipt(prior_sent, args.record_id, message, sink.url): print("ALREADY_SENT"); return 0
                prior_sent.status = "OPERATOR_ATTENTION"; prior_sent.attention = "UNPROVEN_SENT_RECEIPT"; state.operator_attention = "UNPROVEN_SENT_RECEIPT"; save(state_path, asdict(state)); print("OPERATOR_ATTENTION"); return 2
            if any(d.record_id == args.record_id and d.status == "SUBMITTING" for d in state.deliveries):
                print("OPERATOR_ATTENTION"); return 2
            # CLAIMED is the safe pre-submit phase and may be retried.
            # Drop that stale exact-record entry while retaining other bounded history.
            state.deliveries = [d for d in state.deliveries if d.record_id != args.record_id]
            state.deliveries = (state.deliveries + [Delivery(args.record_id, "CLAIMED", time.time())])[-MAX_DELIVERIES:]; save(state_path, asdict(state))
            submission_started = False
            def before_submit() -> None:
                nonlocal submission_started
                if not legacy_owner_selected(root): raise Attention("WAKE_OWNER_CHANGED")
                if submission_started:
                    raise Attention("SUBMISSION_BOUNDARY_REENTERED")
                state.deliveries[-1].status = "SUBMITTING"; save(state_path, asdict(state)); submission_started = True
            try: receipt = sink.wake(args.record_id, message, lambda: actionable(root, args.record_id), before_submit)
            except PostSubmitUnknown as error:
                state.operator_attention = str(error); state.deliveries[-1].attention = str(error)
                if not submission_started: state.deliveries[-1].status = "OPERATOR_ATTENTION"
                save(state_path, asdict(state)); print("OPERATOR_ATTENTION"); return 2
            except Attention as error:
                if str(error) == "WAKE_CANCELLED" and not submission_started: print("WAKE_CANCELLED"); return 4
                if str(error) == "CHATGPT_NOT_IDLE" and not submission_started:
                    # The exact target is still generating. No typing or submit
                    # boundary was crossed, so this is retry-safe. Keep a clean
                    # CLAIMED record (no receipt/attention fields) and let the
                    # CatDesk host retry out of band after ChatGPT becomes idle.
                    state.operator_attention = None; state.deliveries[-1].status = "CLAIMED"; state.deliveries[-1].attention = None
                    save(state_path, asdict(state)); print("CHATGPT_NOT_IDLE"); return 3
                state.operator_attention = str(error); state.deliveries[-1].attention = str(error)
                if not submission_started: state.deliveries[-1].status = "OPERATOR_ATTENTION"
                save(state_path, asdict(state)); print("OPERATOR_ATTENTION"); return 2
            except Exception:
                # Never leak browser exception text. The durable callback still
                # distinguishes retry-safe pre-submit failures from ambiguity.
                attention = "SUBMIT_RECEIPT_UNPROVEN" if submission_started else "PRE_SUBMIT_UNEXPECTED"
                if not submission_started: state.deliveries[-1].status = "OPERATOR_ATTENTION"
                state.operator_attention = attention; state.deliveries[-1].attention = attention; save(state_path, asdict(state)); print("OPERATOR_ATTENTION"); return 2
            if not isinstance(receipt, DeliveryReceipt) or receipt.record_id != args.record_id or receipt_for(args.record_id, message, sink.url, receipt.browser_sent_at_unix) != receipt:
                # Only the durable callback establishes the submission boundary.
                # A swallowed pre-submit exception may otherwise look like None.
                attention = "SUBMIT_RECEIPT_UNPROVEN" if submission_started else "PRE_SUBMIT_NO_RECEIPT"
                if not submission_started: state.deliveries[-1].status = "OPERATOR_ATTENTION"
                state.operator_attention = attention; state.deliveries[-1].attention = attention; save(state_path, asdict(state)); print("OPERATOR_ATTENTION"); return 2
            state.deliveries[-1].status = "SENT"; state.deliveries[-1].browser_sent_at_unix = receipt.browser_sent_at_unix; state.deliveries[-1].message_sha256 = receipt.message_sha256; state.deliveries[-1].target_sha256 = receipt.target_sha256; state.deliveries[-1].receipt_schema_version = receipt.receipt_schema_version; state.operator_attention = None; save(state_path, asdict(state)); print("WOKE"); return 0
    except Busy: print("WAKE_BRIDGE_BUSY"); return 3
if __name__ == "__main__": raise SystemExit(main())
