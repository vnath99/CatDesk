"""Persistent browser owner; installed beside an immutable reviewed primitives copy.

Rust owns config, events, claims and receipts. This adapter receives one exact
event at a time and cannot submit until Rust acknowledges its durable boundary.
No browser or authentication data is written to diagnostic output.
"""
import contextlib
import importlib.util
import json
import os
import sys
from pathlib import Path

CHANNEL = sys.stdout
ROOT = Path(sys.argv[1])


def emit(value):
    CHANNEL.write(json.dumps(value, separators=(",", ":")) + "\n")
    CHANNEL.flush()


def read_request():
    line = sys.stdin.buffer.readline(8193)
    if not line:
        raise EOFError()
    if len(line) > 8192 or not line.endswith(b"\n"):
        raise ValueError("invalid request")
    return json.loads(line)


def load_primitives():
    path = Path(__file__).resolve().parent / "wake_bridge.py"
    spec = importlib.util.spec_from_file_location("wake_primitives", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def prepare_cdp_target(bridge, sb, url):
    """Open the exact target through the proven CDP smoke path.

    A healthy already-active CDP page may navigate itself directly. A fresh
    SeleniumBase context enters CDP mode on the exact protected URL in one
    operation; do not add a UC warm-up first, because that double transition
    can leave ChatGPT HOME selected even when both calls return successfully.
    """
    cdp = getattr(sb, "cdp", None)
    direct_get = getattr(cdp, "get", None) if cdp is not None else None
    if callable(direct_get):
        emit({"stage": "CDP_REUSE_BEGIN"})
        try:
            direct_get(url)
            emit({"stage": "CDP_REUSE_DONE"})
            return cdp
        except Exception:
            pass

    # Match the independently proven CDP smoke path: a fresh
    # SeleniumBase context enters CDP mode directly on the protected target.
    # The previous UC-open -> CDP double transition could leave ChatGPT HOME
    # selected even though each operation individually succeeded.
    emit({"stage": "CDP_ACTIVATE_BEGIN"})
    try:
        sb.activate_cdp_mode(url)
    except Exception as error:
        raise bridge.Attention("BROWSER_SESSION_UNAVAILABLE") from error
    emit({"stage": "CDP_ACTIVATE_DONE"})
    cdp = getattr(sb, "cdp", None)
    if cdp is None:
        raise bridge.Attention("BROWSER_SESSION_UNAVAILABLE")
    return cdp


def attempt(bridge, sb, event, target):
    url = bridge.canonical_conversation_url(target["url"])
    if url != target["url"] or bridge.digest(url) != target["digest"]:
        raise bridge.Attention("TARGET_IDENTITY_MISMATCH")
    if event["targetGeneration"] != target["generation"]:
        raise bridge.Attention("TARGET_GENERATION_STALE")
    record_id, message = event["eventId"], event["message"]
    sink = bridge.CdpSink(url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, url)
    emit({"stage": "TARGET_OPEN"})
    def readiness_observer(route, reason):
        emit({"stage": "READINESS_OBSERVED", "route": route, "reason": reason})
    cdp = sink.wait_for_page_readiness(sb, cdp, observer=readiness_observer)
    emit({"stage": "PAGE_READY"})
    sink.wait_for_idle(cdp, sink.ui_timeout)
    emit({"stage": "CHAT_IDLE"})
    sink.pre_typing_ready(cdp)
    emit({"stage": "PREWRITE_READY"})
    message_digest = bridge.digest(bridge.normalize_message(message))
    prior = sink.pre_submit_receipt_snapshot(cdp, message)
    emit({"stage": "RECEIPT_BASELINE_READY"})
    sink.best_effort_present_browser(cdp)
    sink.pre_typing_ready(cdp)
    emit({"stage": "PREWRITE_CONFIRMED"})
    # ``pre_typing_ready`` has just proved one empty visible composer.  Carry
    # that resolved fixed selector through both typing and the native-Enter
    # fallback rather than reverting to the legacy first selector.  Current
    # ChatGPT surfaces can expose only the fixed ProseMirror or lexical
    # alternatives; using ``bridge.EDITOR`` here would then raise from CDP
    # before the explicit submit authorization boundary.
    editor_selector = sink.editor_selector(cdp, prevalidated=True)
    cdp.press_keys(editor_selector, message, timeout=sink.ui_timeout)
    if bridge.normalize_message(sink.text(cdp)) != bridge.normalize_message(message):
        raise bridge.Attention("TYPING_NOT_CONFIRMED")
    send_selector = sink.wait_for_send_control(cdp, 20)
    if not sink.exact(cdp.get_current_url()):
        raise bridge.Attention("TARGET_IDENTITY_MISMATCH")
    if bridge.normalize_message(sink.text(cdp)) != bridge.normalize_message(message):
        raise bridge.Attention("TYPING_NOT_CONFIRMED")
    emit({"stage": "BEFORE_SUBMIT"})
    if read_request() != {"submit": True}:
        raise bridge.Attention("SUBMISSION_NOT_AUTHORIZED")
    if not sink.exact(cdp.get_current_url()):
        raise bridge.Attention("TARGET_IDENTITY_MISMATCH")
    if bridge.normalize_message(sink.text(cdp)) != bridge.normalize_message(message):
        raise bridge.Attention("TYPING_NOT_CONFIRMED")
    send_selector = sink.wait_for_send_control(cdp, 5)
    sink.smoke_path_submit(cdp, send_selector, sink.ui_timeout, editor_selector)
    accepted_at = sink.wait_for_submission_accepted(cdp, prior, message_digest, message)
    # Accepted submission starts the passive response timer but is not a
    # durable receipt and cannot transition delivery to SENT.
    emit({"stage": "SUBMISSION_ACCEPTED"})
    sink.wait_for_response_completion(
        cdp, message, observer=lambda stage: emit({"stage": stage})
    )
    # Response observation keeps this browser/profile/conversation owned and
    # may periodically refresh the exact target while Stop/Pause remains active.
    # After deterministic completion, perform the separate persistence round-trip.
    sink.durable_receipt_round_trip(cdp)
    receipt = sink.confirm_exact_receipt(
        cdp,
        prior,
        message_digest,
        record_id,
        message,
        wall_time=lambda: accepted_at,
    )
    receipt_payload = {
        "schemaVersion": 1, "eventId": receipt.record_id,
        "targetGeneration": target["generation"], "targetDigest": receipt.target_sha256,
        "messageDigest": receipt.message_sha256, "sentUtc": int(receipt.browser_sent_at_unix),
        "evidence": "EXACT_USER_MESSAGE_APPENDED",
    }
    emit({"stage": "USER_MESSAGE_APPENDED", "receipt": receipt_payload})
    emit({"stage": "SENT", "receipt": receipt_payload})


def reconcile_submission(bridge, sb, event, target):
    """Reconcile a delivery already durably SUBMITTING without another USER submit.

    This path never types, presses Enter, or clicks the USER send control.
    It first proves that the expected wake digest is already durably present,
    then observes the resulting assistant turn. If ChatGPT exposes the exact
    bounded delivery-timeout card, only that assistant response's Retry control
    may be clicked; the original USER wake is never submitted again.
    """
    url = bridge.canonical_conversation_url(target["url"])
    if url != target["url"] or bridge.digest(url) != target["digest"]:
        raise bridge.Attention("TARGET_IDENTITY_MISMATCH")
    if event["targetGeneration"] != target["generation"]:
        raise bridge.Attention("TARGET_GENERATION_STALE")
    record_id, message = event["eventId"], event["message"]
    expected = bridge.digest(bridge.normalize_message(message))
    sink = bridge.CdpSink(url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, url)
    emit({"stage": "TARGET_OPEN"})
    # SUBMITTING reconciliation is post-submit and observe-only. This adapter
    # process already navigated a new same-profile browser document to the exact
    # protected target, so do not perform another reload while an assistant may
    # still be generating. The exact-message reconciliation below is the
    # persistence proof and never re-submits the USER wake.
    emit({"stage": "PAGE_READY"})
    receipt = sink.reconcile_persisted_receipt(
        cdp, expected, message, record_id, target["digest"]
    )
    receipt_payload = {
        "schemaVersion": 1, "eventId": receipt.record_id,
        "targetGeneration": target["generation"], "targetDigest": receipt.target_sha256,
        "messageDigest": receipt.message_sha256, "sentUtc": int(receipt.browser_sent_at_unix),
        "evidence": "EXACT_USER_MESSAGE_APPENDED",
    }
    # Reconciliation never re-submits the USER wake. It only proves the
    # existing receipt, restarts the passive timer idempotently, then observes
    # or retries the failed assistant response in the same conversation.
    emit({"stage": "USER_MESSAGE_APPENDED", "receipt": receipt_payload})
    sink.wait_for_response_completion(
        cdp, message, observer=lambda stage: emit({"stage": stage})
    )
    emit({"stage": "SENT", "receipt": receipt_payload})


def startup_reason(error):
    """Map browser-start failures to a fixed, non-sensitive vocabulary."""
    name = type(error).__name__
    return {
        "PermissionError": "BROWSER_PROFILE_ACCESS_DENIED",
        "FileNotFoundError": "BROWSER_BINARY_OR_DRIVER_MISSING",
        "SessionNotCreatedException": "BROWSER_SESSION_NOT_CREATED",
        "WebDriverException": "BROWSER_DRIVER_START_FAILED",
        "OSError": "BROWSER_PROCESS_START_FAILED",
    }.get(name, "BROWSER_SESSION_UNAVAILABLE")


def main():
    # No browser/library subprocess may start before the parent assigns this
    # process to its kill-on-close Windows job and acknowledges containment.
    if read_request() != {"initialize": True}:
        raise ValueError("owner initialization required")
    # Selenium/library output must never share the protocol channel or persist
    # raw page diagnostics. Only our fixed vocabulary reaches the owner.
    with open(os.devnull, "w") as quiet:
        with contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
            try:
                bridge = load_primitives()
            except Exception:
                emit({"stage": "ATTENTION", "reason": "BROWSER_PRIMITIVES_LOAD_FAILED"})
                return
            try:
                from seleniumbase import SB
            except Exception:
                emit({"stage": "ATTENTION", "reason": "SELENIUMBASE_IMPORT_FAILED"})
                return
            try:
                with SB(uc=True, user_data_dir=str(ROOT / "browser-profile")) as sb:
                    emit({"stage": "READY"})
                    try:
                        request = read_request()
                        if request.get("reconcile") is True:
                            reconcile_submission(
                                bridge, sb, request["event"], request["target"]
                            )
                        else:
                            attempt(bridge, sb, request["event"], request["target"])
                        # One adapter process owns exactly one browser attempt.
                        # Returning exits SeleniumBase's context immediately after
                        # SENT or successful observe-only reconciliation, so the
                        # session-owned Chrome closes deterministically instead of
                        # lingering for the next Wake event.
                        return
                    except EOFError:
                        return
                    except Exception as error:
                        reason = "BROWSER_ATTEMPT_FAILED"
                        if isinstance(error, bridge.Attention):
                            candidate = str(error)
                            if candidate and len(candidate) <= 100 and all(c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789" for c in candidate):
                                reason = candidate
                        else:
                            # Preserve a bounded, content-free exception class so an
                            # installed-path failure can be diagnosed without exposing
                            # browser text, URLs, credentials, or exception messages.
                            error_class = type(error).__name__.upper()
                            error_class = "".join(c if c.isalnum() else "_" for c in error_class)[:48]
                            if error_class:
                                reason = f"BROWSER_ATTEMPT_{error_class}"
                        emit({"stage": "ATTENTION", "reason": reason})
                        return
            except Exception as error:
                emit({"stage": "ATTENTION", "reason": startup_reason(error)})
                return


if __name__ == "__main__":
    try:
        main()
    except Exception:
        emit({"stage": "ATTENTION", "reason": "BROWSER_SESSION_UNAVAILABLE"})
