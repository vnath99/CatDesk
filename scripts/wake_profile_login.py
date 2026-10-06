#!/usr/bin/env python3
"""Open CatDesk's dedicated ChatGPT wake profile for operator-only authentication.

This helper intentionally does not inspect cookies, browser storage, credentials,
page source/text, tokens, or profile files. It only opens the configured
conversation in the dedicated SeleniumBase profile and waits for the operator.
"""
from __future__ import annotations

import argparse
import json
import os
import time
import re
from pathlib import Path
from urllib.parse import urlparse


def validate_conversation_url(value: str) -> str:
    if not isinstance(value, str) or not value or len(value) > 512 or not value.isascii() or any(char.isspace() or ord(char) < 32 for char in value):
        raise ValueError("conversation URL must be a bounded exact ChatGPT conversation URL")
    try:
        parsed = urlparse(value)
        port = parsed.port
    except ValueError as error:
        raise ValueError("conversation URL must be a bounded exact ChatGPT conversation URL") from error
    if parsed.scheme != "https" or parsed.hostname not in {"chatgpt.com", "chat.openai.com"}:
        raise ValueError("conversation URL must use a supported ChatGPT HTTPS host")
    if parsed.username or parsed.password or port is not None or parsed.query or parsed.fragment:
        raise ValueError("conversation URL must not contain credentials, ports, query parameters, or fragments")
    conversation_id = parsed.path.removeprefix("/c/")
    if parsed.path != f"/c/{conversation_id}" or not re.fullmatch(r"[A-Za-z0-9_-]{1,200}", conversation_id):
        raise ValueError("conversation URL must identify one exact ChatGPT /c/<conversation-id> path")
    return f"https://{parsed.hostname}/c/{conversation_id}"


def mcp_ready_confirmation(value: str) -> bool:
    """Accept only the fixed operator acknowledgement, never UI/page data."""
    return value.strip() == "MCP_READY"


def write_mcp_ready_receipt(path: Path, conversation_url: str, profile_dir: Path, confirmed_at_unix: float | None = None) -> None:
    """Persist only the non-secret operator acknowledgement bound to this target."""
    confirmed_at_unix = time.time() if confirmed_at_unix is None else confirmed_at_unix
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("w", encoding="utf-8", newline="\n") as handle:
        json.dump(
            {
                "schema_version": 1,
                "conversation_url": conversation_url,
                "profile_dir": str(profile_dir),
                "confirmed_at_unix": confirmed_at_unix,
                "mcp_ready": True,
            },
            handle,
            indent=2,
            sort_keys=True,
        )
        handle.write("\n")
    os.replace(temporary, path)


def main() -> int:
    parser = argparse.ArgumentParser(description="Open the dedicated CatDesk wake profile for manual ChatGPT authentication")
    parser.add_argument("--conversation-url", required=True)
    parser.add_argument("--profile-dir", required=True)
    parser.add_argument("--mcp-ready-receipt", help="local receipt path written only after MCP_READY")
    parser.add_argument(
        "--require-mcp-ready",
        action="store_true",
        help="require an operator-only confirmation that CatDesk MCP is enabled in this exact conversation",
    )
    args = parser.parse_args()

    conversation_url = validate_conversation_url(args.conversation_url)
    profile_dir = Path(args.profile_dir).resolve()
    if not profile_dir.is_dir():
        raise SystemExit("Dedicated wake profile directory does not exist.")

    try:
        from seleniumbase import SB
    except ImportError as error:
        raise SystemExit("SeleniumBase is not installed in this wake-bridge environment.") from error

    print("Opening the dedicated CatDesk ChatGPT profile.")
    print("Manually sign in to ChatGPT if needed and make sure this exact conversation is open.")
    if args.require_mcp_ready:
        print("In this same conversation, manually enable the CatDesk MCP connector before returning here.")
    print("Do not paste credentials into this PowerShell window. Complete authentication only in the browser.")
    with SB(uc=True, user_data_dir=str(profile_dir)) as browser:
        browser.open(conversation_url)
        if args.require_mcp_ready:
            confirmation = input(
                "After the exact conversation is open and CatDesk MCP is manually enabled, type MCP_READY to close: "
            )
            if not mcp_ready_confirmation(confirmation):
                print("OPERATOR_ATTENTION")
                return 2
            if not args.mcp_ready_receipt:
                print("OPERATOR_ATTENTION")
                return 2
            write_mcp_ready_receipt(Path(args.mcp_ready_receipt).resolve(), conversation_url, profile_dir)
        elif args.mcp_ready_receipt:
            print("OPERATOR_ATTENTION")
            return 2
        else:
            input("When the exact conversation is open and authenticated, return here and press Enter to close the setup browser: ")
    print("Dedicated wake profile authentication and MCP-readiness step completed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
