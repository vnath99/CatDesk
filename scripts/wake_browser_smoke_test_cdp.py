#!/usr/bin/env python3
"""Standalone SeleniumBase UC + CDP smoke test for ChatGPT wake delivery.

This test intentionally does NOT use CatDesk MCP, review-inbox state,
MCP_READY receipts, wake accounting, or the CatDesk daemon.

It uses SeleniumBase's native CDP Mode API for navigation, element checks,
human-speed typing, clicking, reloads, and post-submit confirmation.
"""

from __future__ import annotations

import argparse
import time
from pathlib import Path
from urllib.parse import urlparse

DEFAULT_CONVERSATION_URL = "https://chatgpt.com/c/6a78d9f0-911c-83ea-b829-38a6cf623991"
DEFAULT_PROFILE_DIR = ".catdesk/wake-bridge/browser-profile"
DEFAULT_MESSAGE = "CATDESK_WAKE_SMOKE_TEST_CDP"

EDITOR_SELECTOR = "#prompt-textarea"
SEND_SELECTORS = (
    "button[data-testid='send-button']",
    "button[aria-label='Send prompt']",
    "button[aria-label='Send message']",
    "button#composer-submit-button",
)
STOP_SELECTORS = (
    "button[data-testid='stop-button']",
    "button[aria-label='Stop answering']",
    "button[aria-label='Stop generating']",
    "button[aria-label='Stop streaming']",
    "button[aria-label='Stop']",
)
LOGIN_SELECTORS = (
    "a[href*='/auth/login']",
    "button[data-testid*='login' i]",
)
NETWORK_ERROR_SELECTORS = (
    "#main-frame-error",
    "#error-code",
)
POST_STOP_HOLD_SECONDS = 2.0


def fail(stage: str, detail: str = "") -> int:
    suffix = f":{detail}" if detail else ""
    print(f"SMOKE_TEST_FAILED:{stage}{suffix}")
    return 1


def exact_target(current_url: str, configured_url: str) -> bool:
    current = urlparse(current_url)
    configured = urlparse(configured_url)
    return (
        current.scheme == "https"
        and current.hostname == configured.hostname
        and current.path.rstrip("/") == configured.path.rstrip("/")
        and not current.query
        and not current.fragment
    )


def visible(cdp, selector: str) -> bool:
    try:
        return bool(cdp.is_element_visible(selector))
    except Exception:
        return False


def network_error_page(cdp) -> bool:
    try:
        current = cdp.get_current_url()
        if isinstance(current, str) and current.startswith("chrome-error://"):
            return True
    except Exception:
        pass
    return all(visible(cdp, selector) for selector in NETWORK_ERROR_SELECTORS)


def login_visible(cdp) -> bool:
    return any(visible(cdp, selector) for selector in LOGIN_SELECTORS)


def stop_visible(cdp) -> bool:
    return any(visible(cdp, selector) for selector in STOP_SELECTORS)


def editor_text(cdp) -> str:
    # ChatGPT has used both contenteditable and textarea variants.
    try:
        value = cdp.get_attribute(EDITOR_SELECTOR, "value")
        if isinstance(value, str) and value.strip():
            return value.strip()
    except Exception:
        pass
    try:
        value = cdp.get_text(EDITOR_SELECTOR, timeout=2)
        return value.strip() if isinstance(value, str) else ""
    except Exception:
        return ""


def recover_network_error(cdp, url: str, reload_number: int, max_reloads: int) -> bool:
    if not network_error_page(cdp):
        return False
    if reload_number >= max_reloads:
        raise RuntimeError("NETWORK_RELOAD_LIMIT")
    print(f"BROWSER_ERROR_PAGE:RELOADING:{reload_number + 1}/{max_reloads}")
    cdp.reload(ignore_cache=False)
    time.sleep(2.0)
    # If Chrome's error document survived a normal reload, explicitly reopen
    # the same exact target through CDP before the next poll cycle.
    if network_error_page(cdp):
        cdp.get(url)
        time.sleep(2.0)
    return True


def wait_for_ready(cdp, url: str, timeout: float, max_network_reloads: int) -> None:
    deadline = time.monotonic() + timeout
    next_status = 0.0
    reloads = 0

    while time.monotonic() < deadline:
        if network_error_page(cdp):
            if recover_network_error(cdp, url, reloads, max_network_reloads):
                reloads += 1
                continue

        current = cdp.get_current_url()
        if isinstance(current, str) and urlparse(current).path.startswith("/auth/"):
            raise RuntimeError("LOGIN_REQUIRED")
        if login_visible(cdp):
            raise RuntimeError("LOGIN_REQUIRED")

        if exact_target(current, url) and visible(cdp, EDITOR_SELECTOR):
            print("TARGET_OK")
            print("EDITOR_FOUND")
            return

        now = time.monotonic()
        if now >= next_status:
            if exact_target(current, url):
                print("WAITING_FOR_CHATGPT_UI")
            else:
                print("WAITING_FOR_TARGET_NAVIGATION")
            next_status = now + 2.0
        time.sleep(0.25)

    raise TimeoutError("CHATGPT_UI_TIMEOUT")


def wait_for_idle(cdp, timeout: float, *, monotonic=time.monotonic, sleeper=time.sleep) -> None:
    deadline = monotonic() + timeout
    next_status = 0.0
    while monotonic() < deadline:
        if network_error_page(cdp):
            raise RuntimeError("NETWORK_ERROR_AFTER_READY")
        if not stop_visible(cdp):
            print("CHATGPT_IDLE")
            return
        now = monotonic()
        if now >= next_status:
            print("WAITING_FOR_CHATGPT_IDLE")
            next_status = now + 2.0
        sleeper(0.5)
    raise TimeoutError("CHATGPT_STILL_GENERATING")


def bounded_pair(value):
    if not isinstance(value, list) or len(value) != 2:
        raise RuntimeError("COMPOSER_STATE_INVALID")
    return value[0], value[1]


def editor_state(cdp) -> tuple[str, bool]:
    script = """(() => {
        const visible = Array.from(document.querySelectorAll('#prompt-textarea')).filter(element => {
            const style = window.getComputedStyle(element);
            return element.getClientRects().length > 0 && style.visibility !== 'hidden' && style.display !== 'none';
        });
        if (visible.length !== 1) return ['count', visible.length];
        const element = visible[0];
        const text = element.value || element.innerText || element.textContent || '';
        return ['ready', text.trim() === ''];
    })()"""
    status, detail = bounded_pair(cdp.evaluate(script))
    if status == "ready" and isinstance(detail, bool):
        return status, detail
    if status == "count" and isinstance(detail, int) and not isinstance(detail, bool) and 0 <= detail <= 8:
        return status, False
    raise RuntimeError("COMPOSER_STATE_INVALID")


def require_pre_typing_ready(cdp, url: str) -> None:
    current = cdp.get_current_url()
    if not exact_target(current, url):
        raise RuntimeError("TARGET_CHANGED_BEFORE_TYPING")
    if stop_visible(cdp):
        raise RuntimeError("CHATGPT_NOT_IDLE")
    state, empty = editor_state(cdp)
    if state != "ready":
        raise RuntimeError("EDITOR_NOT_UNIQUE")
    if not empty:
        raise RuntimeError("EXISTING_DRAFT")


def send_control_state(cdp) -> tuple[str, str | None]:
    script = f"""(() => {{
        const selectors = {list(SEND_SELECTORS)!r};
        const matches = selectors.flatMap((selector, index) => Array.from(document.querySelectorAll(selector))
            .filter(button => {{ const style = window.getComputedStyle(button); return button.getClientRects().length > 0 && style.visibility !== 'hidden' && style.display !== 'none'; }})
            .map(button => ({{button, index}})));
        const unique = [];
        for (const match of matches) if (!unique.some(item => item.button === match.button)) unique.push(match);
        if (unique.length === 0) return ['missing', null];
        if (unique.length !== 1) return ['ambiguous', null];
        const button = unique[0].button;
        if (button.disabled || button.getAttribute('aria-disabled') === 'true') return ['disabled', null];
        return ['ready', selectors[unique[0].index]];
    }})()"""
    status, selector = bounded_pair(cdp.evaluate(script))
    if status not in {"ready", "missing", "ambiguous", "disabled"}:
        raise RuntimeError("SEND_STATE_INVALID")
    if status == "ready" and selector in SEND_SELECTORS:
        return status, selector
    if status != "ready" and selector is None:
        return status, None
    raise RuntimeError("SEND_STATE_INVALID")


def find_send_selector(cdp, timeout: float, *, monotonic=time.monotonic, sleeper=time.sleep) -> str | None:
    deadline = monotonic() + timeout
    saw_send_control = False
    while monotonic() < deadline:
        state, selector = send_control_state(cdp)
        if state == "ready":
            return selector
        if state == "ambiguous":
            raise RuntimeError("SEND_NOT_UNIQUE")
        if state == "disabled":
            saw_send_control = True
        sleeper(0.25)
    if saw_send_control:
        raise TimeoutError("SEND_NOT_READY")
    return None


def observe_post_submit_state(cdp, timeout: float, *, monotonic=time.monotonic, sleeper=time.sleep) -> None:
    deadline = time.monotonic() + timeout
    stop_seen_at = None
    while monotonic() < deadline:
        now = monotonic()
        if stop_visible(cdp) and stop_seen_at is None:
            stop_seen_at = now
            print("STOP_STATE_OBSERVED")
        if not editor_text(cdp) and (stop_seen_at is None or now >= stop_seen_at + POST_STOP_HOLD_SECONDS):
            print("SMOKE_TEST_SENT")
            return
        if stop_seen_at is not None:
            sleeper(min(0.25, max(0.0, stop_seen_at + POST_STOP_HOLD_SECONDS - now)))
        else:
            sleeper(0.25)
    raise TimeoutError("SUBMIT_NOT_CONFIRMED")


def run_once(sb, url: str, message: str, timeout: float, max_network_reloads: int, keep_open: float) -> None:
    print("ACTIVATING_CDP_MODE")
    sb.activate_cdp_mode(url)
    cdp = sb.cdp

    wait_for_ready(cdp, url, timeout, max_network_reloads)
    wait_for_idle(cdp, timeout)
    require_pre_typing_ready(cdp, url)

    print("TYPING_MODE:CDP_PRESS_KEYS")
    print("TYPING_SPEED:SELENIUMBASE_NATIVE_HUMAN_SPEED")
    cdp.press_keys(EDITOR_SELECTOR, message, timeout=timeout)
    print("MESSAGE_TYPED")

    send_selector = find_send_selector(cdp, min(timeout, 20.0))
    if send_selector:
        print(f"SEND_BUTTON_FOUND:{send_selector}")
        cdp.click(send_selector, timeout=10)
        print("SEND_CLICKED")
    else:
        print("NO_SAFE_SEND_BUTTON:USING_NATIVE_ENTER")
        cdp.press_keys(EDITOR_SELECTOR, "\n", timeout=10)
        print("ENTER_SENT")

    observe_post_submit_state(cdp, min(timeout, 30.0))

    if keep_open > 0:
        time.sleep(min(keep_open, 60.0))


def main() -> int:
    parser = argparse.ArgumentParser(description="Standalone SeleniumBase UC + CDP ChatGPT smoke test")
    parser.add_argument("--conversation-url", default=DEFAULT_CONVERSATION_URL)
    parser.add_argument("--profile-dir", default=DEFAULT_PROFILE_DIR)
    parser.add_argument("--message", default=DEFAULT_MESSAGE)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--navigation-attempts", type=int, default=3)
    parser.add_argument("--network-reloads", type=int, default=2)
    parser.add_argument("--keep-open-seconds", type=float, default=8.0)
    args = parser.parse_args()

    parsed = urlparse(args.conversation_url)
    if parsed.scheme != "https" or parsed.hostname not in {"chatgpt.com", "chat.openai.com"}:
        return fail("BAD_URL")
    if parsed.username or parsed.password or parsed.query or parsed.fragment or not parsed.path:
        return fail("BAD_URL")
    if not args.message.strip():
        return fail("EMPTY_MESSAGE")
    if not 5 <= args.timeout <= 180:
        return fail("BAD_TIMEOUT")
    if not 1 <= args.navigation_attempts <= 5:
        return fail("BAD_NAVIGATION_ATTEMPTS")
    if not 0 <= args.network_reloads <= 5:
        return fail("BAD_NETWORK_RELOAD_LIMIT")

    profile = Path(args.profile_dir).resolve()
    if not profile.is_dir():
        return fail("PROFILE_NOT_FOUND", str(profile))

    try:
        from seleniumbase import SB
    except ImportError as error:
        return fail("SELENIUMBASE_IMPORT", type(error).__name__)

    print(f"OPENING:{args.conversation_url}")
    print(f"MESSAGE:{args.message}")

    for attempt in range(1, args.navigation_attempts + 1):
        print(f"NAVIGATION_ATTEMPT:{attempt}")
        try:
            # Test mode may suppress context-body exceptions. Smoke failures
            # must reach this bounded retry/fail-closed loop unchanged.
            with SB(uc=True, user_data_dir=str(profile)) as sb:
                run_once(
                    sb,
                    args.conversation_url,
                    args.message,
                    args.timeout,
                    args.network_reloads,
                    args.keep_open_seconds,
                )
                return 0
        except KeyboardInterrupt:
            return fail("CANCELLED")
        except RuntimeError as error:
            name = str(error)
            if name in {"LOGIN_REQUIRED", "EXISTING_DRAFT"}:
                return fail(name)
            print(f"ATTEMPT_FAILED:{name}")
        except TimeoutError as error:
            print(f"ATTEMPT_TIMEOUT:{error}")
        except Exception as error:
            print(f"ATTEMPT_EXCEPTION:{type(error).__name__}:{error}")

        if attempt < args.navigation_attempts:
            print("RELAUNCHING_BROWSER_SESSION")
            time.sleep(2.0)

    return fail("ALL_ATTEMPTS_FAILED")


if __name__ == "__main__":
    raise SystemExit(main())
