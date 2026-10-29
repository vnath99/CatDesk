from __future__ import annotations
import argparse
import os
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

def visible(cdp, selector):
    try: return bool(cdp.is_element_visible(selector))
    except Exception: return False

def exact_url(current, expected):
    try:
        actual, target = urlparse(current), urlparse(expected)
        return (actual.scheme == "https" and actual.hostname == target.hostname
                and actual.username is None and actual.password is None and actual.port is None
                and actual.path.rstrip("/") == target.path.rstrip("/")
                and not actual.query and not actual.fragment)
    except Exception:
        return False

def select_registered_target_tab(cdp, url):
    """CDP's controlled tab may differ from the human-visible tab after login.

    Select only a unique returned tab whose target metadata AND document URL
    match the registered destination. Do not navigate, close tabs, read draft
    contents, or export any other tab's URL. Uncertainty never permits selection.
    """
    try:
        tabs = cdp.get_tabs()
        if not isinstance(tabs, (list, tuple)) or len(tabs) > 32:
            return "TARGET_TAB_UNAVAILABLE"
        matches = [tab for tab in tabs if exact_url(str(tab.target.url), url)]
        if not matches:
            return "TARGET_TAB_MISSING"
        if len(matches) != 1:
            return "TARGET_TAB_AMBIGUOUS"
        tab = matches[0]
        if not exact_url(str(cdp.loop.run_until_complete(tab.get_current_url())), url):
            return "TARGET_TAB_DRIFT"
        cdp.switch_to_tab(tab)
        return "TARGET_TAB_EXACT" if exact_url(str(cdp.get_current_url()), url) else "TARGET_TAB_DRIFT"
    except Exception:
        return "TARGET_TAB_UNAVAILABLE"

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

def recover_empty_home(cdp, url):
    """One same-tab navigation, only from a positively empty, idle Home.

    HOME alone says nothing about authentication. Never inspect/export draft
    contents or leave an unknown composer. The caller consumes its sole recovery
    attempt even if this check or navigation fails.
    """
    if classify(cdp, url) != "HOME":
        return
    try:
        empty = cdp.evaluate("""(() => {
            if (document.readyState !== 'complete') return false;
            const editors = document.querySelectorAll('#prompt-textarea');
            if (editors.length !== 1) return false;
            const e = editors[0];
            if (!(e instanceof HTMLTextAreaElement) && !e.isContentEditable) return false;
            const text = e instanceof HTMLTextAreaElement ? e.value : e.textContent;
            if (text !== '' || e.querySelector('img, video, audio, input, [data-file-id]')) return false;
            if (document.querySelector('[data-testid*="attachment" i], [data-testid*="file-thumbnail" i]')) return false;
            return true;
        })()""")
        # Require an actual boolean, never truthy error strings or wrapped values.
        if empty is True and not any(visible(cdp, s) for s in STOP):
            # Existing bridge recovery also uses get(): open() can switch tabs.
            cdp.get(url)
    except Exception:
        pass

def main():
    parser = argparse.ArgumentParser(description="Read-only CatDesk Wake profile readiness probe")
    parser.add_argument("--conversation-url", required=True)
    parser.add_argument("--seconds", type=float, default=45.0)
    parser.add_argument("--await-start", action="store_true")
    # Internal development owner supplies only its independently validated
    # selected profile. The legacy production probe retains its fixed default.
    parser.add_argument("--profile", type=Path)
    args = parser.parse_args()
    if args.await_start and sys.stdin.readline() != "START\n":
        return 2
    url = args.conversation_url
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.hostname not in {"chatgpt.com", "chat.openai.com"} or not parsed.path.startswith("/c/") or parsed.query or parsed.fragment:
        print("PROBE:BAD_URL", flush=True); return 2
    if not 5 <= args.seconds <= 120:
        print("PROBE:BAD_DURATION", flush=True); return 2
    profile = args.profile if args.profile is not None else PROFILE
    if not profile.is_dir():
        print("PROBE:PROFILE_NOT_FOUND", flush=True); return 2
    from seleniumbase import SB
    with SB(uc=True, user_data_dir=str(profile)) as sb:
        sb.activate_cdp_mode(url)
        cdp = sb.cdp
        if args.profile is not None:
            print("PROBE:" + select_registered_target_tab(cdp, url), flush=True)
        last = None
        deadline = time.monotonic() + args.seconds
        recover_after = time.monotonic() + min(5.0, args.seconds / 2)
        recovery_attempted = False
        # A returned tab can still be loading when the initial independent
        # document check runs. Reinspect existing tabs within the same budget;
        # select only the same unique, independently exact registered target.
        selection_attempts = 1
        select_after = recover_after
        while time.monotonic() < deadline:
            state = classify(cdp, url)
            if (args.profile is not None and state in {"HOME", "TARGET_DRIFT"}
                    and selection_attempts < 4 and time.monotonic() >= select_after):
                selection_attempts += 1
                selected = select_registered_target_tab(cdp, url)
                print("PROBE:" + selected, flush=True)
                state = classify(cdp, url)
                select_after = time.monotonic() + 5.0
            if state != last:
                print("PROBE:" + state, flush=True); last = state
                if state == "CHROME_ERROR":
                    print("PROBE:NETWORK:" + chrome_error_detail(cdp), flush=True)
            if args.profile is not None and state == "HOME" and not recovery_attempted and time.monotonic() >= recover_after:
                recovery_attempted = True
                recover_empty_home(cdp, url)
            time.sleep(1)
        final = classify(cdp, url)
        if final == "CHROME_ERROR" and last != final:
            print("PROBE:NETWORK:" + chrome_error_detail(cdp), flush=True)
        print("PROBE:FINAL:" + final, flush=True)
    return 0
if __name__ == "__main__": raise SystemExit(main())
