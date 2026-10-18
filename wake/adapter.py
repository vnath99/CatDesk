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
MAX_BOOTSTRAP_REQUEST_BYTES = 32 * 1024


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


def read_job_request():
    """Allow one bounded handoff only in the explicit bootstrap mode."""
    line = sys.stdin.buffer.readline(MAX_BOOTSTRAP_REQUEST_BYTES + 1)
    if not line or len(line) > MAX_BOOTSTRAP_REQUEST_BYTES or not line.endswith(b"\n"):
        raise ValueError("invalid job request")
    request = json.loads(line)
    if not isinstance(request, dict) or (request.get("bootstrap") is not True and len(line) > 8192):
        raise ValueError("invalid job request")
    return request


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


def bootstrap_attempt(bridge, sb, request):
    """Submit exactly one handoff from an empty native New Chat page."""
    if set(request) != {"bootstrap", "projectId", "intentId", "source", "handoff"} or request["bootstrap"] is not True:
        raise bridge.Attention("BOOTSTRAP_REQUEST_INVALID")
    source = request["source"]
    if not isinstance(source, dict) or set(source) != {"url", "digest", "generation"}:
        raise bridge.Attention("BOOTSTRAP_REQUEST_INVALID")
    source_url = bridge.canonical_conversation_url(source["url"])
    if (source_url != source["url"] or
            bridge.digest(source_url) != source["digest"] or
            not isinstance(source["generation"], int) or isinstance(source["generation"], bool) or
            not 0 <= source["generation"] < 2**63):
        raise bridge.Attention("BOOTSTRAP_SOURCE_INVALID")
    project_id, intent_id = request["projectId"], request["intentId"]
    message = bridge.compose_bootstrap_message(project_id, intent_id, source_url, request["handoff"])
    message_digest = bridge.digest(bridge.normalize_message(message))
    sink = bridge.NewConversationSink(source_url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, source_url)
    cdp = sink.wait_for_page_readiness(sb, cdp)
    sink.wait_for_idle(cdp, sink.ui_timeout)
    sink.pre_typing_ready(cdp)
    if not sink.exact(cdp.get_current_url()):
        raise bridge.Attention("BOOTSTRAP_SOURCE_DRIFT")
    emit({"stage": "BOOTSTRAP_SOURCE_READY"})
    sink.open_empty_home(cdp)
    emit({"stage": "BOOTSTRAP_HOME_READY"})
    sink.check_empty_home(cdp)
    editor_selector = sink.editor_selector(cdp, prevalidated=True)
    cdp.press_keys(editor_selector, message, timeout=sink.ui_timeout)
    sink.check_typed_home(cdp, message)
    send_selector = sink.wait_for_send_control(cdp, 20)
    if send_selector is None:
        raise bridge.Attention("BOOTSTRAP_SEND_CONTROL_MISSING")
    sink.check_typed_home(cdp, message)
    emit({
        "stage": "BEFORE_BOOTSTRAP_SUBMIT",
        "projectId": project_id,
        "intentId": intent_id,
        "sourceGeneration": source["generation"],
        "messageDigest": message_digest,
    })
    if read_request() != {"submit": True}:
        raise bridge.Attention("SUBMISSION_NOT_AUTHORIZED")
    sink.check_typed_home(cdp, message)
    send_selector = sink.wait_for_send_control(cdp, 5)
    if send_selector is None:
        raise bridge.Attention("BOOTSTRAP_SEND_CONTROL_MISSING")
    # From this point even a click exception may have submitted the USER turn.
    # Preserve unknown intent state; never send or create another chat here.
    try:
        sink.smoke_path_submit(cdp, send_selector, sink.ui_timeout, editor_selector)
        sink.latch_issued_url(cdp)
        accepted_at = sink.wait_for_submission_accepted(cdp, 0, message_digest, message)
        emit({"stage": "BOOTSTRAP_SUBMISSION_ACCEPTED"})
        sink.wait_for_response_completion(
            cdp, message, allow_retry=False,
            observer=lambda stage: emit({"stage": stage}),
        )
        sink.durable_receipt_round_trip(cdp)
        sink.latch_issued_url(cdp)
        receipt = sink.confirm_exact_receipt(
            cdp, 0, message_digest, intent_id, message,
            wall_time=lambda: accepted_at,
        )
        sink.prove_single_bootstrap_user(cdp, message)
        if receipt.message_sha256 != message_digest or receipt.target_sha256 != bridge.digest(sink.url):
            raise bridge.PostSubmitUnknown("BOOTSTRAP_RECEIPT_MISMATCH")
    except Exception as error:
        reason = "BOOTSTRAP_POST_SUBMIT_UNKNOWN"
        if isinstance(error, bridge.Attention):
            candidate = str(error)
            if candidate and len(candidate) <= 100 and all(c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789" for c in candidate):
                reason = candidate
        emit({"stage": "BOOTSTRAP_UNKNOWN", "reason": reason})
        return
    emit({
        "stage": "BOOTSTRAP_COMPLETE",
        "receipt": {
            "schemaVersion": 1,
            "projectId": project_id,
            "intentId": intent_id,
            "sourceGeneration": source["generation"],
            "sourceDigest": source["digest"],
            "destinationUrl": sink.url,
            "destinationDigest": bridge.digest(sink.url),
            "messageDigest": message_digest,
            "sentUtc": int(receipt.browser_sent_at_unix),
            "evidence": "EXACT_USER_MESSAGE_APPENDED",
            "completion": "RESPONSE_COMPLETED",
        },
    })


def source_snapshot(bridge, sb, request):
    """Read an exact idle source chat only on an explicit private request."""
    if set(request) != {"snapshot", "projectId", "source"} or request["snapshot"] is not True:
        raise bridge.Attention("SOURCE_SNAPSHOT_REQUEST_INVALID")
    if not isinstance(request["projectId"], str) or not bridge.SAFE_ID.fullmatch(request["projectId"]):
        raise bridge.Attention("SOURCE_SNAPSHOT_REQUEST_INVALID")
    source = request["source"]
    if not isinstance(source, dict) or set(source) != {"url", "digest", "generation"}:
        raise bridge.Attention("SOURCE_SNAPSHOT_REQUEST_INVALID")
    url = bridge.canonical_conversation_url(source["url"])
    if (url != source["url"] or bridge.digest(url) != source["digest"] or
            not isinstance(source["generation"], int) or isinstance(source["generation"], bool) or
            not 0 <= source["generation"] < 2**63):
        raise bridge.Attention("SOURCE_SNAPSHOT_REQUEST_INVALID")
    sink = bridge.NewConversationSink(url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, url)
    cdp = sink.wait_for_page_readiness(sb, cdp)
    sink.wait_for_idle(cdp, sink.ui_timeout)
    if not sink.exact(cdp.get_current_url()) or sink.readiness_reason(cdp) is not None:
        raise bridge.Attention("SOURCE_SNAPSHOT_SOURCE_DRIFT")
    snapshot = sink.visible_message_snapshot(cdp)
    payload = {
        "stage": "SOURCE_SNAPSHOT",
        "projectId": request["projectId"],
        "sourceGeneration": source["generation"],
        "sourceDigest": source["digest"],
        "snapshot": snapshot,
    }
    # The parent reader has an 8192-byte line cap. JSON escapes may be much
    # larger than the UTF-8 text budget enforced by the CDP snapshot helper.
    while len(json.dumps(payload, separators=(",", ":")).encode("utf-8")) + 1 > 8192:
        messages = snapshot["messages"]
        snapshot["truncated"] = True
        if len(messages) > 1:
            del messages[0]
        elif messages and messages[0]["text"]:
            text = messages[0]["text"]
            messages[0]["text"] = text[:len(text) // 2]
            messages[0]["textComplete"] = False
        else:
            raise bridge.Attention("SOURCE_SNAPSHOT_TOO_LARGE")
        coverage = snapshot["coverage"]
        coverage["returned"] = len(messages)
        coverage["omittedBeforeWindow"] = max(0, coverage["visibleRoleCount"] - len(messages))
        coverage["identityMissing"] = sum(item["identityBasis"] == "MISSING" for item in messages)
        coverage["identityUncertain"] = sum(item["identityBasis"] == "ROLE_TURN_ONLY" for item in messages)
        coverage["revisionIncomplete"] = sum(not item["revisionComplete"] for item in messages)
        coverage["textIncomplete"] = sum(not item["textComplete"] for item in messages)
    emit(payload)


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
                        request = read_job_request()
                        if request.get("snapshot") is True:
                            source_snapshot(bridge, sb, request)
                        elif request.get("bootstrap") is True:
                            bootstrap_attempt(bridge, sb, request)
                        elif request.get("reconcile") is True:
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
