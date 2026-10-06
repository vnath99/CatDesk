from __future__ import annotations
import time
import os
from pathlib import Path
from urllib.parse import urlparse

URL = "https://chatgpt.com/c/6aac7d9d-9298-83e9-9769-c69df5437ceb"
PROFILE = Path(os.environ["LOCALAPPDATA"]) / "CatDeskWake" / "browser-profile"
EDITOR = "#prompt-textarea"
NETWORK = ("#main-frame-error", "#error-code")
LOGIN = ("a[href*='/auth/login']", "button[data-testid*='login' i]")
CAPTCHA = (
    "iframe[title*='captcha' i]",
    "[data-testid*='captcha' i]",
    "iframe[title*='security' i]",
    "[data-testid*='security' i]",
)

def visible(cdp, selector: str) -> bool:
    try:
        return bool(cdp.is_element_visible(selector))
    except Exception:
        return False

def exact(current: str) -> bool:
    try:
        a, b = urlparse(current), urlparse(URL)
        return (
            a.scheme == b.scheme == "https"
            and a.hostname == b.hostname
            and a.path.rstrip("/") == b.path.rstrip("/")
            and not a.query and not a.fragment
        )
    except Exception:
        return False

def classify(cdp) -> str:
    try:
        current = str(cdp.get_current_url())
    except Exception:
        return "URL_UNAVAILABLE"
    if current.startswith("chrome-error://"):
        return "CHROME_ERROR_URL"
    if all(visible(cdp, s) for s in NETWORK):
        return "CHROME_ERROR_DOM"
    if urlparse(current).path.startswith("/auth/") or any(visible(cdp, s) for s in LOGIN):
        return "LOGIN_VISIBLE"
    if any(visible(cdp, s) for s in CAPTCHA):
        return "CAPTCHA_OR_SECURITY_VISIBLE"
    if exact(current) and visible(cdp, EDITOR):
        return "EXACT_TARGET_EDITOR_READY"
    if exact(current):
        return "EXACT_TARGET_EDITOR_NOT_READY"
    return "TARGET_DRIFT"

def main() -> int:
    if not PROFILE.is_dir():
        print("PROBE:PROFILE_NOT_FOUND", flush=True)
        return 2
    from seleniumbase import SB
    print("PROBE:OPENING_EXACT_CHAT27", flush=True)
    with SB(uc=True, user_data_dir=str(PROFILE)) as sb:
        sb.activate_cdp_mode(URL)
        cdp = sb.cdp
        for method in ("maximize", "bring_active_window_to_front"):
            try:
                fn = getattr(cdp, method, None)
                if callable(fn):
                    fn()
            except Exception:
                pass
        last = None
        end = time.monotonic() + 75
        while time.monotonic() < end:
            state = classify(cdp)
            if state != last:
                print(f"PROBE:{state}", flush=True)
                last = state
            time.sleep(1)
        print(f"PROBE:FINAL:{classify(cdp)}", flush=True)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
