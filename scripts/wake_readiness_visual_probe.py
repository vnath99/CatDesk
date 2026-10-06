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

def main():
    parser = argparse.ArgumentParser(description="Read-only CatDesk Wake profile readiness probe")
    parser.add_argument("--conversation-url", required=True)
    parser.add_argument("--seconds", type=float, default=45.0)
    parser.add_argument("--await-start", action="store_true")
    args = parser.parse_args()
    if args.await_start and sys.stdin.readline() != "START\n":
        return 2
    url = args.conversation_url
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.hostname not in {"chatgpt.com", "chat.openai.com"} or not parsed.path.startswith("/c/") or parsed.query or parsed.fragment:
        print("PROBE:BAD_URL", flush=True); return 2
    if not 5 <= args.seconds <= 120:
        print("PROBE:BAD_DURATION", flush=True); return 2
    if not PROFILE.is_dir():
        print("PROBE:PROFILE_NOT_FOUND", flush=True); return 2
    from seleniumbase import SB
    with SB(uc=True, user_data_dir=str(PROFILE)) as sb:
        sb.activate_cdp_mode(url)
        cdp = sb.cdp
        last = None
        deadline = time.monotonic() + args.seconds
        while time.monotonic() < deadline:
            state = classify(cdp, url)
            if state != last:
                print("PROBE:" + state, flush=True); last = state
            time.sleep(1)
        print("PROBE:FINAL:" + classify(cdp, url), flush=True)
    return 0
if __name__ == "__main__": raise SystemExit(main())
