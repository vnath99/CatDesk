#!/usr/bin/env python3
"""Standalone SeleniumBase smoke test for the ChatGPT wake path.

No CatDesk MCP, review inbox, wake state, MCP_READY receipt, or daemon is used.
This only launches the existing dedicated Chrome profile, opens one exact
ChatGPT conversation, waits for the composer to hydrate, waits for ChatGPT to
be idle, types one message, submits it, and confirms the composer cleared.
"""

from __future__ import annotations

import argparse
import random
import time
import traceback
from pathlib import Path
from urllib.parse import urlparse

DEFAULT_CONVERSATION_URL = "https://chatgpt.com/c/6a78d9f0-911c-83ea-b829-38a6cf623991"
DEFAULT_PROFILE_DIR = ".catdesk/wake-bridge/browser-profile"
DEFAULT_MESSAGE = "CATDESK_WAKE_SMOKE_TEST"

EDITOR_SELECTOR = "#prompt-textarea"
SEND_SELECTOR = (
    "button[data-testid='send-button'], "
    "button[aria-label='Send prompt'], "
    "button[aria-label='Send message'], "
    "button#composer-submit-button"
)
STOP_SELECTOR = (
    "button[data-testid='stop-button'], "
    "button[aria-label='Stop answering'], "
    "button[aria-label='Stop generating'], "
    "button[aria-label='Stop streaming'], "
    "button[aria-label='Stop']"
)
POST_STOP_HOLD_SECONDS = 2.0


class DriverLost(RuntimeError):
    pass


def fail(stage: str, detail: str = "") -> int:
    suffix = f": {detail}" if detail else ""
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


def browser_error_page(sb) -> bool:
    """Detect Chrome's own network-error document without scanning page text.

    ChatGPT conversation text can legitimately contain strings such as
    ``ERR_CONNECTION_RESET`` while we are debugging, so body-text matching is
    intentionally forbidden here. Chrome net-error pages expose distinctive
    internal DOM nodes such as ``#main-frame-error`` and ``#error-code``.
    """
    script = r"""
        const chromeUrl = location.href.startsWith("chrome-error://");
        const mainError = document.querySelector("#main-frame-error");
        const errorCode = document.querySelector("#error-code");
        const reloadButton = document.querySelector("#reload-button");
        const netErrorClass = document.documentElement.classList.contains("neterror") ||
                              document.body?.classList.contains("neterror");
        return chromeUrl ||
               (mainError && errorCode) ||
               (errorCode && reloadButton) ||
               netErrorClass;
    """
    try:
        return bool(with_driver_retry(sb, lambda driver: driver.execute_script(script), "browser_error_probe"))
    except Exception:
        return False


def type_with_random_delay(sb, element, text: str, minimum: float, maximum: float) -> None:
    """Type through SeleniumBase UC's GUI path when available.

    UC GUI writes use PyAutoGUI locally instead of one ChromeDriver HTTP request
    per character, which avoids mid-typing WebDriver socket resets.
    """
    gui_write = getattr(raw_driver(sb), "uc_gui_write", None)
    if callable(gui_write):
        try:
            element.click()
        except Exception:
            editors = one_visible(sb, EDITOR_SELECTOR)
            if len(editors) != 1:
                raise DriverLost(f"could not refocus editor; visible={len(editors)}")
            element = editors[0]
            with_driver_retry(sb, lambda _driver: element.click(), "focus_editor")

        print("TYPING_MODE:UC_GUI_WRITE")
        for character in text:
            gui_write(character)
            time.sleep(random.uniform(minimum, maximum))
        return

    print("TYPING_MODE:WEBDRIVER_FALLBACK")
    for character in text:
        element.send_keys(character)
        time.sleep(random.uniform(minimum, maximum))


def raw_driver(sb):
    driver = getattr(sb, "driver", None)
    if driver is None:
        raise DriverLost("WebDriver object is unavailable")
    return driver


def driver_connected(sb) -> bool:
    driver = raw_driver(sb)
    probe = getattr(driver, "is_connected", None)
    if callable(probe):
        try:
            return bool(probe())
        except Exception:
            return False
    return True


def ensure_connected(sb, timeout: float = 8.0) -> None:
    if driver_connected(sb):
        return

    print("WEBDRIVER_DISCONNECTED:RECONNECTING")
    reconnect = getattr(sb, "reconnect", None)
    if not callable(reconnect):
        reconnect = getattr(raw_driver(sb), "reconnect", None)
    if not callable(reconnect):
        raise DriverLost("UC reconnect method is unavailable")

    try:
        reconnect(timeout=min(2.0, timeout))
    except TypeError:
        reconnect(min(2.0, timeout))
    except Exception as error:
        raise DriverLost(f"reconnect failed: {type(error).__name__}") from error

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if driver_connected(sb):
            print("WEBDRIVER_RECONNECTED")
            return
        time.sleep(0.25)
    raise DriverLost("WebDriver did not reconnect")


def with_driver_retry(sb, operation, label: str):
    """Run one WebDriver operation, reconnecting once if UC detached."""
    ensure_connected(sb)
    try:
        return operation(raw_driver(sb))
    except Exception as first:
        print(f"WEBDRIVER_OPERATION_RETRY:{label}:{type(first).__name__}")
        try:
            ensure_connected(sb)
            reconnect = getattr(sb, "reconnect", None)
            if callable(reconnect):
                try:
                    reconnect(timeout=1.0)
                except TypeError:
                    reconnect(1.0)
            ensure_connected(sb)
            return operation(raw_driver(sb))
        except Exception as second:
            raise DriverLost(f"{label} failed after reconnect: {type(second).__name__}") from second


def current_url(sb) -> str:
    value = with_driver_retry(sb, lambda driver: driver.current_url, "current_url")
    return value if isinstance(value, str) else ""


def one_visible(sb, selector: str):
    from selenium.webdriver.common.by import By
    from selenium.common.exceptions import StaleElementReferenceException

    elements = with_driver_retry(
        sb,
        lambda driver: driver.find_elements(By.CSS_SELECTOR, selector),
        "find_elements",
    )
    visible = []
    for element in elements:
        try:
            if element.is_displayed():
                visible.append(element)
        except StaleElementReferenceException:
            continue
    return visible


def editor_text(element) -> str:
    tag = str(getattr(element, "tag_name", "") or "").lower()
    if element.get_attribute("id") != "prompt-textarea":
        raise RuntimeError("unexpected editor id")
    value = element.get_attribute("value" if tag == "textarea" else "textContent")
    return value.strip() if isinstance(value, str) else ""


def uc_open_target(sb, url: str) -> None:
    """Use SeleniumBase's UC-aware navigation, then restore WebDriver."""
    opener = getattr(sb, "uc_open_with_reconnect", None)
    if not callable(opener):
        opener = getattr(raw_driver(sb), "uc_open_with_reconnect", None)
    if not callable(opener):
        raise DriverLost("uc_open_with_reconnect is unavailable")

    try:
        opener(url, reconnect_time=3)
    except TypeError:
        opener(url, 3)
    ensure_connected(sb, timeout=12.0)


def navigate_to_target(sb, url: str, page_timeout: float) -> bool:
    max_error_reloads = 2
    error_reloads = 0

    try:
        uc_open_target(sb, url)
    except DriverLost:
        raise
    except Exception as error:
        print(f"NAVIGATION_OPEN_WARNING:{type(error).__name__}")
        ensure_connected(sb, timeout=12.0)

    deadline = time.monotonic() + min(max(page_timeout, 15.0), 60.0)
    next_status = time.monotonic()
    while time.monotonic() < deadline:
        here = current_url(sb)

        if browser_error_page(sb) or here.startswith("chrome-error://"):
            if error_reloads >= max_error_reloads:
                print("BROWSER_ERROR_PAGE:RELOAD_LIMIT_REACHED")
                return False
            error_reloads += 1
            print(f"BROWSER_ERROR_PAGE:RELOADING:{error_reloads}/{max_error_reloads}")
            time.sleep(1.0)
            uc_open_target(sb, url)
            time.sleep(1.0)
            continue

        if exact_target(here, url):
            print("TARGET_OK")
            return True
        if urlparse(here).path.startswith("/auth/"):
            raise RuntimeError("LOGIN_REQUIRED")
        now = time.monotonic()
        if now >= next_status:
            print("WAITING_FOR_TARGET_NAVIGATION")
            next_status = now + 2.0
        time.sleep(0.5)
    return False


def wait_for_editor(sb, url: str, timeout: float):
    deadline = time.monotonic() + timeout
    next_status = time.monotonic()
    network_reloads = 0
    max_network_reloads = 2
    while time.monotonic() < deadline:
        here = current_url(sb)
        if browser_error_page(sb) or here.startswith("chrome-error://"):
            if network_reloads >= max_network_reloads:
                raise RuntimeError("BROWSER_ERROR_PAGE")
            network_reloads += 1
            print(f"BROWSER_ERROR_PAGE:RELOADING_DURING_UI_WAIT:{network_reloads}/{max_network_reloads}")
            time.sleep(1.0)
            uc_open_target(sb, url)
            next_status = time.monotonic()
            continue
        editors = one_visible(sb, EDITOR_SELECTOR)
        if len(editors) == 1:
            return editors[0]
        if len(editors) > 1:
            raise RuntimeError("multiple visible #prompt-textarea elements")
        now = time.monotonic()
        if now >= next_status:
            print("WAITING_FOR_CHATGPT_UI")
            next_status = now + 2.0
        time.sleep(0.25)
    raise TimeoutError("#prompt-textarea did not become uniquely visible")


def wait_for_idle(sb, timeout: float, *, monotonic=time.monotonic, sleeper=time.sleep) -> bool:
    deadline = monotonic() + timeout
    next_status = monotonic()
    while monotonic() < deadline:
        if not one_visible(sb, STOP_SELECTOR):
            return True
        now = monotonic()
        if now >= next_status:
            print("WAITING_FOR_CHATGPT_IDLE")
            next_status = now + 2.0
        sleeper(0.5)
    return False


def wait_for_send(sb, timeout: float, *, monotonic=time.monotonic, sleeper=time.sleep):
    deadline = monotonic() + timeout
    saw_send_control = False
    while monotonic() < deadline:
        buttons = one_visible(sb, SEND_SELECTOR)
        if len(buttons) > 1:
            raise RuntimeError("multiple visible send buttons")
        if len(buttons) == 1:
            saw_send_control = True
            button = buttons[0]
            if (
                button.is_enabled()
                and button.get_attribute("disabled") is None
                and button.get_attribute("aria-disabled") != "true"
            ):
                return button
        sleeper(0.25)
    if saw_send_control:
        raise TimeoutError("SEND_NOT_READY")
    return None


def pre_typing_ready(sb, url: str):
    """Re-check the exact target and idle, empty, unique editor before typing."""
    if not exact_target(current_url(sb), url):
        raise RuntimeError("TARGET_CHANGED_BEFORE_TYPING")
    if one_visible(sb, STOP_SELECTOR):
        raise RuntimeError("CHATGPT_NOT_IDLE")
    editors = one_visible(sb, EDITOR_SELECTOR)
    if len(editors) != 1:
        raise RuntimeError(f"EDITOR_CHANGED_BEFORE_TYPING visible={len(editors)}")
    if editor_text(editors[0]):
        raise RuntimeError("EXISTING_DRAFT")
    return editors[0]


def wait_for_clear(sb, timeout: float) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        editors = one_visible(sb, EDITOR_SELECTOR)
        if len(editors) == 1:
            try:
                if not editor_text(editors[0]):
                    return True
            except Exception:
                pass
        time.sleep(0.25)
    return False


def observe_post_submit_state(sb, timeout: float, *, monotonic=time.monotonic, sleeper=time.sleep) -> bool:
    """Confirm one submission without ever issuing a second submit.

    A Stop state proves the composer transitioned into generating. It can be
    transient, so a confirmed clear remains sufficient if it is not observed.
    Once seen, retain the browser for two seconds from that observation.
    """
    deadline = monotonic() + timeout
    stop_seen_at = None
    while monotonic() < deadline:
        now = monotonic()
        if one_visible(sb, STOP_SELECTOR) and stop_seen_at is None:
            stop_seen_at = now
            print("STOP_STATE_OBSERVED")
        editors = one_visible(sb, EDITOR_SELECTOR)
        cleared = len(editors) == 1 and not editor_text(editors[0])
        if cleared and (stop_seen_at is None or now >= stop_seen_at + POST_STOP_HOLD_SECONDS):
            print("SMOKE_TEST_SENT")
            return True
        if stop_seen_at is not None:
            sleeper(min(0.25, max(0.0, stop_seen_at + POST_STOP_HOLD_SECONDS - now)))
        else:
            sleeper(0.25)
    return False


def run_attempt(
    SB,
    Keys,
    profile: Path,
    url: str,
    message: str,
    timeout: float,
    attempt: int,
    keep_open: float,
    typing_delay_min: float,
    typing_delay_max: float,
) -> bool:
    print(f"NAVIGATION_ATTEMPT:{attempt}")
    with SB(uc=True, user_data_dir=str(profile)) as sb:
        if not navigate_to_target(sb, url, timeout):
            return False

        editor = wait_for_editor(sb, url, timeout)
        print("EDITOR_FOUND")

        if not wait_for_idle(sb, timeout):
            raise RuntimeError("CHATGPT_STILL_GENERATING")
        print("CHATGPT_IDLE")

        editor = pre_typing_ready(sb, url)

        print(f"TYPING_DELAY_RANGE:{typing_delay_min:.2f}-{typing_delay_max:.2f}s")
        type_with_random_delay(sb, editor, message, typing_delay_min, typing_delay_max)
        print("MESSAGE_TYPED")

        send = wait_for_send(sb, min(timeout, 20.0))
        if send is not None:
            print("SEND_BUTTON_FOUND")
            send.click()
            print("SEND_CLICKED")
        else:
            print("NO_SAFE_SEND_BUTTON:USING_ENTER")
            editor.send_keys(Keys.ENTER)
            print("ENTER_SENT")

        if not observe_post_submit_state(sb, min(timeout, 30.0)):
            raise RuntimeError("SUBMIT_NOT_CONFIRMED")
        if keep_open > 0:
            time.sleep(min(keep_open, 60.0))
        return True


def main() -> int:
    parser = argparse.ArgumentParser(description="Standalone ChatGPT wake browser smoke test")
    parser.add_argument("--conversation-url", default=DEFAULT_CONVERSATION_URL)
    parser.add_argument("--profile-dir", default=DEFAULT_PROFILE_DIR)
    parser.add_argument("--message", default=DEFAULT_MESSAGE)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--navigation-attempts", type=int, default=3)
    parser.add_argument("--keep-open-seconds", type=float, default=8.0)
    parser.add_argument("--typing-delay-min", type=float, default=0.02)
    parser.add_argument("--typing-delay-max", type=float, default=0.20)
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
    if not (0.0 <= args.typing_delay_min <= args.typing_delay_max <= 1.0):
        return fail("BAD_TYPING_DELAY_RANGE")

    profile = Path(args.profile_dir).resolve()
    if not profile.is_dir():
        return fail("PROFILE_NOT_FOUND", str(profile))

    try:
        from seleniumbase import SB
        from selenium.webdriver.common.keys import Keys
    except ImportError as error:
        return fail("SELENIUM_IMPORT_FAILED", str(error))

    print(f"OPENING:{args.conversation_url}")
    print(f"MESSAGE:{args.message}")

    for attempt in range(1, args.navigation_attempts + 1):
        try:
            if run_attempt(
                SB,
                Keys,
                profile,
                args.conversation_url,
                args.message,
                args.timeout,
                attempt,
                args.keep_open_seconds,
                args.typing_delay_min,
                args.typing_delay_max,
            ):
                return 0
        except KeyboardInterrupt:
            return fail("CANCELLED")
        except DriverLost as error:
            print(f"DRIVER_SESSION_LOST:RELAUNCHING:{error}")
        except TimeoutError as error:
            print(f"CHATGPT_UI_TIMEOUT:RELAUNCHING:{error}")
        except RuntimeError as error:
            if str(error) == "LOGIN_REQUIRED":
                return fail("LOGIN_REQUIRED")
            if str(error) in {"CHATGPT_STILL_GENERATING", "EXISTING_DRAFT", "SUBMIT_NOT_CONFIRMED"} or str(error).startswith("EDITOR_CHANGED_AFTER_IDLE"):
                return fail(str(error).split()[0])
            if str(error) == "BROWSER_ERROR_PAGE":
                print("BROWSER_ERROR_PAGE:RELAUNCHING")
            else:
                traceback.print_exc(limit=4)
                print(f"ATTEMPT_FAILED:RELAUNCHING:{type(error).__name__}:{error}")
        except Exception as error:
            traceback.print_exc(limit=4)
            print(f"ATTEMPT_FAILED:RELAUNCHING:{type(error).__name__}:{error}")

        if attempt < args.navigation_attempts:
            time.sleep(2.0)

    return fail("ALL_NAVIGATION_ATTEMPTS_FAILED")


if __name__ == "__main__":
    raise SystemExit(main())
