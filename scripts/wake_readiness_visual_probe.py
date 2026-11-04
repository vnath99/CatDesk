from __future__ import annotations
import argparse
import hashlib
import importlib.util
import json
import os
import re
import sys
import time
from pathlib import Path
from urllib.parse import urlparse

PROFILE = Path(os.environ["LOCALAPPDATA"]) / "CatDeskWake" / "browser-profile"
EDITOR = "#prompt-textarea"
STOP = ("button[data-testid='stop-button']", "button[aria-label*='stop' i]")
LOGIN = ("a[href*='/auth/login']", "button[data-testid*='login' i]")
CAPTCHA = ("iframe[title*='captcha' i]", "[data-testid*='captcha' i]", "iframe[title*='security' i]", "[data-testid*='security' i]")
NETWORK_ERRORS = frozenset({
    "ERR_NETWORK_CHANGED", "ERR_INTERNET_DISCONNECTED", "ERR_NAME_NOT_RESOLVED",
    "ERR_CONNECTION_CLOSED", "ERR_CONNECTION_RESET", "ERR_CONNECTION_ABORTED",
    "ERR_CONNECTION_REFUSED", "ERR_CONNECTION_TIMED_OUT", "ERR_TIMED_OUT",
    "ERR_PROXY_CONNECTION_FAILED", "ERR_TUNNEL_CONNECTION_FAILED",
    "ERR_CERT_DATE_INVALID", "ERR_CERT_AUTHORITY_INVALID", "ERR_CERT_COMMON_NAME_INVALID",
    "ERR_BLOCKED_BY_CLIENT", "ERR_BLOCKED_BY_ADMINISTRATOR", "ERR_BLOCKED_BY_RESPONSE",
    "ERR_HTTP_RESPONSE_CODE_FAILURE", "ERR_TOO_MANY_RETRIES", "ERR_EMPTY_RESPONSE",
    "ERR_FAILED", "ERR_HTTP2_PROTOCOL_ERROR", "ERR_SSL_PROTOCOL_ERROR", "ERR_QUIC_PROTOCOL_ERROR",
})
HOME_CHECKS = frozenset({
    "EMPTY", "NOT_HOME", "BUSY", "DOCUMENT_LOADING", "EDITOR_MISSING",
    "EDITOR_AMBIGUOUS", "EDITOR_UNSUPPORTED", "DRAFT_PRESENT", "EDITOR_MEDIA_PRESENT",
    "ATTACHMENT_INDICATOR", "EVALUATION_UNKNOWN", "EVALUATION_FAILED",
})

def load_observation_sink(path, expected_sha256, profile, url):
    """Execute only exact owner-verified primitive bytes, not an import search."""
    if (path.name != "wake_bridge.py" or not path.is_file() or path.is_symlink()
            or getattr(path, "is_junction", lambda: False)()
            or path.parent.is_symlink() or getattr(path.parent, "is_junction", lambda: False)()):
        raise ValueError("primitives unavailable")
    with path.open("rb") as stream:
        content = stream.read(4 * 1024 * 1024 + 1)
    if len(content) > 4 * 1024 * 1024 or hashlib.sha256(content).hexdigest() != expected_sha256:
        raise ValueError("primitives identity mismatch")
    spec = importlib.util.spec_from_file_location("catdesk_readiness_primitives", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    exec(compile(content, str(path), "exec"), module.__dict__)
    sink = module.CdpSink(url, profile, 30, 20)
    if not callable(getattr(sink, "browser_surface_observation", None)):
        raise ValueError("observation primitives unsupported")
    return sink


def surface_state(value):
    route = value["route"]
    if route != "EXACT":
        return {"HOME": "HOME", "AUTH_REQUIRED": "LOGIN_VISIBLE", "SECURITY_CHECK": "CAPTCHA_VISIBLE",
                "NETWORK_ERROR": "CHROME_ERROR", "TARGET_DRIFT": "TARGET_DRIFT"}.get(route, "OBSERVATION_UNAVAILABLE")
    if value["document"] == "LOADING":
        return "EXACT_DOCUMENT_LOADING"
    if value["document"] != "COMPLETE":
        return "OBSERVATION_UNAVAILABLE"
    if value["editor"] != "READY":
        return "EXACT_EDITOR_AMBIGUOUS" if value["editor"] == "AMBIGUOUS" else "EXACT_NO_EDITOR"
    if value["generation"] == "ACTIVE":
        return "EXACT_READY_BUSY"
    if value["generation"] != "IDLE":
        return "OBSERVATION_UNAVAILABLE"
    if value.get("composer") == "DRAFT_PRESENT":
        return "EXACT_DRAFT_PRESENT"
    if value.get("composer") != "EMPTY" or value["assistant"] == "UNKNOWN":
        return "OBSERVATION_UNAVAILABLE"
    if value["assistant"] == "DELIVERY_TIMEOUT":
        return "EXACT_READY_RESPONSE_TIMEOUT"
    return "EXACT_READY_RESPONSE_LOAD_ERROR" if value["assistant"] == "LOAD_ERROR" else "EXACT_READY_IDLE"

def visible(cdp, selector):
    try: return bool(cdp.is_element_visible(selector))
    except Exception: return False

def conversation_id(value):
    """Same strict route contract as the owner-verified bridge primitives."""
    if (not isinstance(value, str) or not value or len(value) > 512 or not value.isascii()
            or any(char.isspace() or ord(char) < 32 for char in value)):
        return None
    try:
        actual = urlparse(value)
        if (actual.scheme != "https" or actual.hostname not in {"chatgpt.com", "chat.openai.com"}
                or actual.username or actual.password or actual.port is not None or actual.query or actual.fragment):
            return None
        if not re.fullmatch(r"/(?:c/[A-Za-z0-9_-]{1,200}|g/[A-Za-z0-9_-]{1,200}/c/[A-Za-z0-9_-]{1,200})", actual.path):
            return None
        return actual.path.split("/")[-1]
    except Exception:
        return None


def exact_url(current, expected):
    identity = conversation_id(current)
    return identity is not None and identity == conversation_id(expected)


def value_shape(value):
    return "STRING" if isinstance(value, str) else ("WRAPPED" if isinstance(value, (dict, tuple, list)) else "OTHER")


def route_class(value, expected):
    if not isinstance(value, str):
        return "UNKNOWN"
    if value.startswith("chrome-error://"):
        return "NETWORK"
    if exact_url(value, expected):
        return "EXACT"
    try:
        parsed = urlparse(value)
        if (parsed.scheme != "https" or parsed.hostname not in {"chatgpt.com", "chat.openai.com"}
                or parsed.username or parsed.password or parsed.port is not None):
            return "OTHER"
        if parsed.path.startswith('/auth/'):
            return "AUTH"
        if parsed.path in {"", "/"} and not parsed.query and not parsed.fragment:
            return "HOME"
    except Exception:
        return "INVALID"
    return "OTHER"


def select_registered_target_tab(cdp, url, report=None):
    """CDP's controlled tab may differ from the human-visible tab after login.

    Select only a unique returned tab whose target metadata AND document URL
    match the registered destination. Do not navigate, close tabs, read draft
    contents, or export any other tab's URL. Uncertainty never permits selection.
    """
    facts = dict(schemaVersion=1, tabCount=-1, metadataMatches=0, unreadableMetadata=0,
                 candidateShape="UNOBSERVED", candidateRoute="UNKNOWN", candidateFrame="UNKNOWN",
                 controlledShape="UNOBSERVED", controlledRoute="UNKNOWN", alternateRoute=False,
                 stage="ENUMERATION", state="TARGET_TAB_UNAVAILABLE")
    def finish(state):
        facts["state"] = state
        if report is not None:
            report(dict(facts))
        return state
    def controlled():
        value = cdp.get_current_url()
        facts["controlledShape"] = value_shape(value)
        facts["controlledRoute"] = route_class(value, url)
        return value
    try:
        try:
            controlled()
        except Exception:
            facts["controlledShape"] = "OTHER"
        tabs = cdp.get_tabs()
        if not isinstance(tabs, (list, tuple)) or len(tabs) > 32:
            return finish("TARGET_TAB_UNAVAILABLE")
        facts["tabCount"] = len(tabs)
        facts["stage"] = "METADATA"
        matches = []
        for tab in tabs:
            try:
                metadata = tab.target.url
                if not isinstance(metadata, str):
                    facts["unreadableMetadata"] += 1
                elif exact_url(metadata, url):
                    matches.append(tab)
            except Exception:
                facts["unreadableMetadata"] += 1
        facts["metadataMatches"] = len(matches)
        if facts["unreadableMetadata"]:
            return finish("TARGET_TAB_UNAVAILABLE")
        if not matches:
            return finish("TARGET_TAB_MISSING")
        if len(matches) != 1:
            return finish("TARGET_TAB_AMBIGUOUS")
        tab = matches[0]
        facts["stage"] = "DOCUMENT"
        document = cdp.loop.run_until_complete(tab.get_current_url())
        facts["candidateShape"] = value_shape(document)
        facts["candidateRoute"] = route_class(document, url)
        try:
            frame = cdp.loop.run_until_complete(tab.evaluate("window === window.top"))
            facts["candidateFrame"] = "TOP" if frame is True else ("SUBFRAME" if frame is False else "UNKNOWN")
        except Exception:
            pass
        if not exact_url(document, url):
            return finish("TARGET_TAB_DRIFT")
        facts["alternateRoute"] = document != url
        facts["stage"] = "SWITCH"
        cdp.switch_to_tab(tab)
        if not exact_url(controlled(), url):
            return finish("TARGET_TAB_DRIFT")
        facts["stage"] = "VERIFIED"
        return finish("TARGET_TAB_EXACT")
    except Exception:
        return finish("TARGET_TAB_UNAVAILABLE")

def classify(cdp, url):
    try: current = str(cdp.get_current_url())
    except Exception: return "URL_UNAVAILABLE"
    try:
        p = urlparse(current)
    except Exception:
        return "URL_INVALID"
    if current.startswith("chrome-error://"): return "CHROME_ERROR"
    if p.path.startswith("/auth/") or any(visible(cdp, s) for s in LOGIN): return "LOGIN_VISIBLE"
    if any(visible(cdp, s) for s in CAPTCHA): return "CAPTCHA_VISIBLE"
    exact = exact_url(current, url)
    if exact and visible(cdp, EDITOR):
        return "EXACT_READY_BUSY" if any(visible(cdp, s) for s in STOP) else "EXACT_READY_IDLE"
    if exact: return "EXACT_NO_EDITOR"
    if p.hostname in {"chatgpt.com","chat.openai.com"} and p.path in {"","/"}: return "HOME"
    return "TARGET_DRIFT"

def chrome_error_detail(cdp):
    """Export only a fixed browser error label, never page text or a target URL.

    Ordinary ChatGPT documents, including unknown drafts, are not inspected.
    The browser's internal error document must still be selected at evaluation.
    """
    try:
        if not str(cdp.get_current_url()).startswith("chrome-error://"):
            return "UNKNOWN"
        code = cdp.evaluate("""(() => {
            if (location.protocol !== 'chrome-error:') return null;
            const nodes = document.querySelectorAll('#error-code, .error-code');
            if (nodes.length !== 1) return null;
            return nodes[0].textContent;
        })()""")
        if isinstance(code, str) and code.strip() in NETWORK_ERRORS:
            return code.strip()
    except Exception:
        pass
    return "UNKNOWN"

def home_recovery_check(cdp, url):
    """Explain the existing emptiness guard using fixed metadata, never text.

    The reason is not authentication or task authority. The document must still
    be HTTPS ChatGPT Home inside the evaluation before inspecting its composer.
    """
    if classify(cdp, url) != "HOME":
        return "NOT_HOME"
    if any(visible(cdp, s) for s in STOP):
        return "BUSY"
    try:
        reason = cdp.evaluate("""(() => {
            if (location.protocol !== 'https:' ||
                !['chatgpt.com', 'chat.openai.com'].includes(location.hostname) ||
                !['', '/'].includes(location.pathname) || location.port ||
                location.username || location.password || location.search || location.hash) return 'NOT_HOME';
            if (document.readyState !== 'complete') return 'DOCUMENT_LOADING';
            const editors = document.querySelectorAll('#prompt-textarea');
            if (editors.length === 0) return 'EDITOR_MISSING';
            if (editors.length !== 1) return 'EDITOR_AMBIGUOUS';
            const e = editors[0];
            if (!(e instanceof HTMLTextAreaElement) && !e.isContentEditable) return 'EDITOR_UNSUPPORTED';
            const text = e instanceof HTMLTextAreaElement ? e.value : e.textContent;
            if (text !== '') return 'DRAFT_PRESENT';
            if (e.querySelector('img, video, audio, input, [data-file-id]')) return 'EDITOR_MEDIA_PRESENT';
            if (document.querySelector('[data-testid*="attachment" i], [data-testid*="file-thumbnail" i]')) return 'ATTACHMENT_INDICATOR';
            return 'EMPTY';
        })()""")
        if isinstance(reason, str) and reason in HOME_CHECKS:
            return reason
    except Exception:
        return "EVALUATION_FAILED"
    return "EVALUATION_UNKNOWN"

def recover_empty_home(cdp, url):
    """One same-tab navigation after the same strict idle/empty checks.

    The caller consumes its sole attempt regardless of the fixed outcome. No
    unknown draft, attachment, wrapped value or evaluation failure permits it.
    """
    reason = home_recovery_check(cdp, url)
    if reason != "EMPTY":
        return reason
    if classify(cdp, url) != "HOME":
        return "NOT_HOME"
    if any(visible(cdp, s) for s in STOP):
        return "BUSY"
    try:
        # Existing bridge recovery also uses get(): open() can switch tabs.
        cdp.get(url)
        return "NAVIGATION_ATTEMPTED"
    except Exception:
        return "NAVIGATION_FAILED"

def main():
    parser = argparse.ArgumentParser(description="Read-only CatDesk Wake profile readiness probe")
    parser.add_argument("--conversation-url", required=True)
    parser.add_argument("--seconds", type=float, default=45.0)
    parser.add_argument("--await-start", action="store_true")
    # Internal development owner supplies only its independently validated
    # selected profile. The legacy production probe retains its fixed default.
    parser.add_argument("--profile", type=Path)
    parser.add_argument("--primitives", type=Path)
    parser.add_argument("--primitives-sha256")
    parser.add_argument("--read-only", action="store_true")
    args = parser.parse_args()
    if args.await_start and sys.stdin.readline() != "START\n":
        return 2
    url = args.conversation_url
    if conversation_id(url) is None:
        print("PROBE:BAD_URL", flush=True); return 2
    if not 5 <= args.seconds <= 120:
        print("PROBE:BAD_DURATION", flush=True); return 2
    profile = args.profile if args.profile is not None else PROFILE
    if not profile.is_dir():
        print("PROBE:PROFILE_NOT_FOUND", flush=True); return 2
    sink = None
    if args.primitives is not None or args.primitives_sha256 is not None:
        try:
            sink = load_observation_sink(args.primitives, args.primitives_sha256, profile, url)
        except Exception:
            print("PROBE:FINAL:OBSERVATION_UNAVAILABLE", flush=True)
            return 0
    def observation(cdp):
        if sink is None:
            return classify(cdp, url), None
        try:
            value = sink.browser_surface_observation(cdp)
            return surface_state(value), value
        except Exception:
            return "OBSERVATION_UNAVAILABLE", None
    from seleniumbase import SB
    def report_tab(facts):
        print("PROBE:TAB:" + json.dumps(facts, separators=(",", ":"), sort_keys=True), flush=True)
    with SB(uc=True, user_data_dir=str(profile)) as sb:
        sb.activate_cdp_mode(url)
        cdp = sb.cdp
        if args.profile is not None:
            print("PROBE:" + select_registered_target_tab(cdp, url, report_tab), flush=True)
        last = None
        last_surface = None
        deadline = time.monotonic() + args.seconds
        recover_after = time.monotonic() + min(5.0, args.seconds / 2)
        recovery_attempted = False
        # A returned tab can still be loading when the initial independent
        # document check runs. Reinspect existing tabs within the same budget;
        # select only the same unique, independently exact registered target.
        selection_attempts = 1
        select_after = recover_after
        while time.monotonic() < deadline:
            state, surface = observation(cdp)
            if (args.profile is not None and state in {"HOME", "TARGET_DRIFT"}
                    and selection_attempts < 4 and time.monotonic() >= select_after):
                selection_attempts += 1
                selected = select_registered_target_tab(cdp, url, report_tab)
                print("PROBE:" + selected, flush=True)
                state, surface = observation(cdp)
                select_after = time.monotonic() + 5.0
            if state != last:
                print("PROBE:" + state, flush=True); last = state
                if state == "CHROME_ERROR":
                    print("PROBE:NETWORK:" + chrome_error_detail(cdp), flush=True)
            if surface is not None and surface != last_surface:
                print("PROBE:SURFACE:" + json.dumps(surface, separators=(",", ":"), sort_keys=True), flush=True)
                last_surface = surface
            if not args.read_only and args.profile is not None and state == "HOME" and not recovery_attempted and time.monotonic() >= recover_after:
                recovery_attempted = True
                print("PROBE:HOME_CHECK:" + recover_empty_home(cdp, url), flush=True)
            time.sleep(1)
        final, surface = observation(cdp)
        if surface is not None:
            print("PROBE:SURFACE:" + json.dumps(surface, separators=(",", ":"), sort_keys=True), flush=True)
        if final == "CHROME_ERROR" and last != final:
            print("PROBE:NETWORK:" + chrome_error_detail(cdp), flush=True)
        print("PROBE:FINAL:" + final, flush=True)
    return 0
if __name__ == "__main__": raise SystemExit(main())
