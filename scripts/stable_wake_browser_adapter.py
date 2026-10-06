#!/usr/bin/env python3
"""Fixed stateless one-attempt browser adapter for the Rust wake owner.

This is intentionally not the legacy bridge: it never reads or writes
delivery state, canonical inbox records, receipts, locks, or acknowledgements.
It derives its workspace and config paths from this fixed project-owned script
location and emits only a small fixed-vocabulary JSON result.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import re
import stat
import sys
import time
from pathlib import Path
from typing import Any

SAFE_ID = re.compile(r"^[A-Za-z0-9_-]{1,128}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
MAX_CONFIG_BYTES = 16 * 1024
SCHEMA_VERSION = 1
FIXED_PROFILE = ".catdesk/wake-bridge/browser-profile"
MAX_PROFILE_BYTES = 1024

# The reviewed durable runtime carries this script and its exact browser
# primitives beside the reviewed interpreter.  Resolving a sibling is
# intentional: do not fall back to a repository script, PATH, `py`, or a
# developer installation when the durable runtime is absent or damaged.
RUNTIME_ROOT = Path(__file__).resolve().parent
LEGACY_SOURCE = RUNTIME_ROOT / "wake_bridge.py"


def emit(outcome: str, sent_at_unix: float | None = None) -> int:
    value: dict[str, Any] = {"schemaVersion": SCHEMA_VERSION, "outcome": outcome}
    if sent_at_unix is not None:
        value["sentAtUnix"] = sent_at_unix
    encoded = json.dumps(value, separators=(",", ":"), sort_keys=True)
    if len(encoded.encode("utf-8")) > 256:
        return 2
    print(encoded)
    return 0


def load_bridge() -> Any:
    spec = importlib.util.spec_from_file_location("catdesk_legacy_browser_primitives", LEGACY_SOURCE)
    if spec is None or spec.loader is None:
        raise RuntimeError("bridge unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _assert_directory_component_is_safe(
    path: Path, metadata: os.stat_result, *, windows: bool | None = None
) -> None:
    """Reject an unsafe directory component without following it.

    Windows junctions and other reparse points are not reported by
    ``os.path.islink``.  They have to be rejected before any canonicalization
    can turn a caller-selected path into authority outside the fixed profile.
    The explicit ``windows`` seam makes the Windows metadata rule testable on
    non-Windows CI; production always uses the host platform.
    """
    if not stat.S_ISDIR(metadata.st_mode) or os.path.islink(path):
        raise ValueError("unsafe directory identity")
    if windows is None:
        windows = os.name == "nt"
    if not windows:
        return
    try:
        attributes = metadata.st_file_attributes
        reparse_flag = stat.FILE_ATTRIBUTE_REPARSE_POINT
        if not isinstance(attributes, int) or not isinstance(reparse_flag, int):
            raise ValueError("directory metadata unavailable")
        if os.path.isjunction(path) or attributes & reparse_flag:
            raise ValueError("unsafe directory identity")
    except (AttributeError, OSError) as error:
        raise ValueError("directory metadata unavailable") from error


def _safe_directory_identity(path: Path) -> Path:
    """Resolve a pre-existing directory only after rejecting every link hop."""
    absolute = path.absolute()
    current = Path(absolute.anchor)
    try:
        _assert_directory_component_is_safe(current, os.lstat(current))
        for component in absolute.parts[1:]:
            current = current / component
            _assert_directory_component_is_safe(current, os.lstat(current))
        resolved = absolute.resolve(strict=True)
        _assert_directory_component_is_safe(resolved, os.lstat(resolved))
    except OSError as error:
        raise ValueError("directory identity unavailable") from error
    return resolved


def _fixed_profile_identity(root: Path) -> Path:
    root_identity = _safe_directory_identity(root)
    return _safe_directory_identity(root_identity / ".catdesk" / "wake-bridge" / "browser-profile")


def validate_exact_profile(profile_dir: Any, root: Path | None = None) -> Path:
    """Authorize exactly one protected profile directory, never containment.

    The relative representation is deliberately one normalized spelling.  An
    absolute representation must be a normal local path and have the same
    canonical filesystem identity as the fixed profile; prefix containment is
    never an authorization check.
    """
    if root is None:
        root = _safe_directory_identity(Path.cwd())
    if not isinstance(profile_dir, str) or not profile_dir or len(profile_dir.encode("utf-8")) > MAX_PROFILE_BYTES:
        raise ValueError("profile invalid")
    if "\x00" in profile_dir or any(ord(char) < 32 for char in profile_dir):
        raise ValueError("profile invalid")
    expected = _fixed_profile_identity(root)
    normalized = profile_dir.replace("\\", "/")
    if normalized == FIXED_PROFILE:
        # No dot, parent, rooted, prefixed, or alternate relative spelling can
        # reach this branch; exact spelling is the sole relative authority.
        return expected
    if normalized.startswith(("//", "\\\\?\\", "\\\\.\\")):
        raise ValueError("profile invalid")
    candidate = Path(profile_dir)
    if not candidate.is_absolute():
        raise ValueError("profile invalid")
    if any(component in {".", ".."} for component in candidate.parts):
        raise ValueError("profile invalid")
    # Windows device/verbatim/UNC alternatives are intentionally refused.  A
    # normal absolute local path is accepted only after same-file identity.
    if profile_dir.startswith(("\\\\", "//", "\\\\?\\", "\\\\.\\")):
        raise ValueError("profile invalid")
    candidate_identity = _safe_directory_identity(candidate)
    try:
        if not os.path.samefile(candidate_identity, expected):
            raise ValueError("profile identity mismatch")
    except OSError as error:
        raise ValueError("profile identity unavailable") from error
    return expected


def load_exact_config(
    bridge: Any, expected_target_sha256: str, root: Path | None = None
) -> tuple[str, Path, float, float]:
    # `FixedScriptBrowserAdapter` fixes cwd to its canonical workspace.  It is
    # not an adapter argument and therefore cannot select a second target,
    # profile, inbox, state, or runtime authority.
    if root is None:
        root = _safe_directory_identity(Path.cwd())
    path = root / ".catdesk" / "wake-bridge" / "config.json"
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_CONFIG_BYTES:
        raise ValueError("config unavailable")
    pairs: list[tuple[str, Any]] = []
    config = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=lambda values: pairs.extend(values) or dict(values))
    if not isinstance(config, dict) or len({key for key, _ in pairs}) != len(pairs):
        raise ValueError("config invalid")
    target = bridge.canonical_conversation_url(config.get("conversation_url"))
    if hashlib.sha256(target.encode("utf-8")).hexdigest() != expected_target_sha256:
        raise ValueError("target drift")
    profile = validate_exact_profile(config.get("profile_dir"), root)
    ready = float(config.get("ui_ready_timeout_seconds", 20))
    submit = float(config.get("send_confirmation_timeout_seconds", 15))
    if not (math.isfinite(ready) and math.isfinite(submit) and 1 <= ready <= 60 and 1 <= submit <= 60):
        raise ValueError("timeout invalid")
    return target, profile, ready, submit


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--record-id", required=True)
    parser.add_argument("--message-sha256", required=True)
    parser.add_argument("--target-sha256", required=True)
    args = parser.parse_args()
    if not SAFE_ID.fullmatch(args.record_id) or not SHA256.fullmatch(args.message_sha256) or not SHA256.fullmatch(args.target_sha256):
        return emit("AMBIGUOUS_POST_SUBMIT")
    bridge: Any | None = None
    try:
        bridge = load_bridge()
        target, profile, ready, submit = load_exact_config(bridge, args.target_sha256)
        message = bridge.MESSAGE.format(record_id=args.record_id)
        if bridge.digest(bridge.normalize_message(message)) != args.message_sha256:
            return emit("AMBIGUOUS_POST_SUBMIT")
        sink = bridge.CdpSink(target, profile, ready, submit)
        receipt = sink.wake(args.record_id, message, lambda: True, lambda: None)
        if (
            not isinstance(receipt, bridge.DeliveryReceipt)
            or receipt.record_id != args.record_id
            or receipt.message_sha256 != args.message_sha256
            or receipt.target_sha256 != args.target_sha256
            or receipt.receipt_schema_version != 1
            or not isinstance(receipt.browser_sent_at_unix, (int, float))
            or not math.isfinite(receipt.browser_sent_at_unix)
            or receipt.browser_sent_at_unix <= 0
        ):
            return emit("AMBIGUOUS_POST_SUBMIT")
        return emit("DEFINITE_SUCCESS", float(receipt.browser_sent_at_unix))
    except (OSError, ValueError, json.JSONDecodeError):
        return emit("TARGET_NOT_READY")
    except Exception as error:
        # Only the bridge's known idle condition is demonstrably before typing
        # or submission. Every other browser exception remains ambiguous.
        if bridge is not None and isinstance(error, bridge.Attention) and str(error) == "CHATGPT_NOT_IDLE":
            return emit("DEFINITE_PRE_SUBMIT_FAILURE")
        return emit("AMBIGUOUS_POST_SUBMIT")


if __name__ == "__main__":
    raise SystemExit(main())
