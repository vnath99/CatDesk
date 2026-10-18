"""Open one prepared Wake development profile for manual ChatGPT sign-in.

The operator supplies credentials only in the visible browser. This helper
neither reads browser state nor asserts that authentication succeeded.
"""
from __future__ import annotations

import contextlib
import os
import re
import sys
from pathlib import Path


_URL = re.compile(
    r"https://(?:chatgpt\.com|chat\.openai\.com)/"
    r"(?:c/(?:WEB:)?[A-Za-z0-9_-]{1,200}|"
    r"g/[A-Za-z0-9_-]{1,200}/c/(?:WEB:)?[A-Za-z0-9_-]{1,200})\Z"
)


def validate_conversation_url(value: str) -> str:
    if not isinstance(value, str) or len(value) > 512 or not _URL.fullmatch(value):
        raise ValueError("exact ChatGPT conversation URL required")
    return value


def main(argv=None, input_stream=None, output_stream=None, browser_factory=None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    input_stream = sys.stdin if input_stream is None else input_stream
    output_stream = sys.stdout if output_stream is None else output_stream
    if len(argv) != 2:
        return 2
    try:
        profile = Path(argv[0])
        url = validate_conversation_url(argv[1])
        if not profile.is_absolute() or not profile.is_dir() or profile.is_symlink():
            return 2
        # Rust assigns the still-idle helper to its private kill-on-close Job
        # before this START token can permit SeleniumBase to launch Chrome.
        if input_stream.readline() != "START\n":
            return 2
        with open(os.devnull, "w", encoding="utf-8") as quiet:
            with contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
                if browser_factory is None:
                    from seleniumbase import SB

                    browser_factory = SB
                with browser_factory(uc=True, headless=False, user_data_dir=str(profile)) as browser:
                    browser.activate_cdp_mode(url)
                    print("LOGIN_BROWSER_OPEN", file=output_stream, flush=True)
                    acknowledged = input_stream.readline() in {"\n", "\r\n"}
        if not acknowledged:
            print("LOGIN_HELPER_ATTENTION", file=output_stream, flush=True)
            return 2
        print("LOGIN_BROWSER_CLOSED", file=output_stream, flush=True)
        return 0
    except Exception:
        # Never print profile contents, browser state, URLs, or Python tracebacks.
        print("LOGIN_HELPER_ATTENTION", file=output_stream, flush=True)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
