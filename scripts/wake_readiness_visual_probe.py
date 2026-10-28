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

def visible(cdp, selector):
    try: return bool(cdp.is_element_visible(selector))
    except Exception: return False

def classify(cdp, url):
    try: current = str(cdp.get_current_url())
    except Exception: return "URL_UNAVAILABLE"
    try:
        p = urlparse(current)
        expected = urlparse(url)
    except Exception:
        return "URL_INVALID"
    if current.startswith("chrome-error://"): return "CHROME_ERROR"
    if p.path.startswith("/auth/") or any(visible(cdp, s) for s in LOGIN): return "LOGIN_VISIBLE"
    if any(visible(cdp, s) for s in CAPTCHA): return "CAPTCHA_VISIBLE"
    exact = p.scheme == "https" and p.hostname == expected.hostname and p.path.rstrip("/") == expected.path.rstrip("/") and not p.query and not p.fragment
    if exact and visible(cdp, EDITOR):
        return "EXACT_READY_BUSY" if any(visible(cdp, s) for s in STOP) else "EXACT_READY_IDLE"
    if exact: return "EXACT_NO_EDITOR"
    if p.hostname in {"chatgpt.com","chat.openai.com"} and p.path in {"","/"}: return "HOME"
    return "TARGET_DRIFT"

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
        last = None
        deadline = time.monotonic() + args.seconds
        recover_after = time.monotonic() + min(5.0, args.seconds / 2)
        recovery_attempted = False
        while time.monotonic() < deadline:
            state = classify(cdp, url)
            if state != last:
                print("PROBE:" + state, flush=True); last = state
            if args.profile is not None and state == "HOME" and not recovery_attempted and time.monotonic() >= recover_after:
                recovery_attempted = True
                recover_empty_home(cdp, url)
            time.sleep(1)
        print("PROBE:FINAL:" + classify(cdp, url), flush=True)
    return 0
if __name__ == "__main__": raise SystemExit(main())
