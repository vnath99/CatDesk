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
import time
from pathlib import Path

CHANNEL = sys.stdout
ROOT = Path(sys.argv[1])
MAX_BOOTSTRAP_REQUEST_BYTES = 32 * 1024
SOURCE_SNAPSHOT_RESPONSE_BYTES = 64 * 1024


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
    if not isinstance(request, dict) or (not any(request.get(kind) is True for kind in
            ("bootstrap", "bootstrapDraftInspect", "bootstrapDraftArchive",
             "bootstrapDraftRepair", "bootstrapQuarantineInspect")) and len(line) > 8192):
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


def attempt(bridge, sb, event, target, *, cooperative=False):
    url = bridge.canonical_conversation_url(target["url"])
    if url != target["url"] or bridge.digest(url) != target["digest"]:
        raise bridge.Attention("TARGET_IDENTITY_MISMATCH")
    if event["targetGeneration"] != target["generation"]:
        raise bridge.Attention("TARGET_GENERATION_STALE")
    record_id, message = event["eventId"], event["message"]
    typed_message = bridge.keyboard_safe_message(message)
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
    # CDP text entry itself can synthesize Enter. The parent must persist its
    # uncertain-submission boundary before the first possible browser write.
    emit({"stage": "BEFORE_SUBMIT"})
    if read_request() != {"submit": True}:
        raise bridge.Attention("SUBMISSION_NOT_AUTHORIZED")
    if not sink.exact(cdp.get_current_url()):
        raise bridge.Attention("TARGET_IDENTITY_MISMATCH")
    sink.pre_typing_ready(cdp)
    # Carry the exact validated selector through typing and the native-Enter
    # fallback; alternate ChatGPT composers need not expose bridge.EDITOR.
    editor_selector = sink.editor_selector(cdp, prevalidated=True)
    sink.insert_empty_conversation_draft(cdp, typed_message)
    if bridge.normalize_message(sink.text(cdp)) != bridge.normalize_message(message):
        raise bridge.Attention("TYPING_NOT_CONFIRMED")
    send_selector = sink.wait_for_send_control(cdp, 20)
    if not sink.exact(cdp.get_current_url()):
        raise bridge.Attention("TARGET_IDENTITY_MISMATCH")
    if bridge.normalize_message(sink.text(cdp)) != bridge.normalize_message(message):
        raise bridge.Attention("TYPING_NOT_CONFIRMED")
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
    if not cooperative:
        sink.wait_for_response_completion(
            cdp, message, observer=lambda stage: emit({"stage": stage})
        )
    # A fresh-document round-trip is mandatory in both modes. Cooperative
    # delivery may close this owner while the assistant is still generating,
    # but never before independent USER persistence has been proven.
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
    emit({"stage": "SENT_PENDING_RESPONSE" if cooperative else "SENT", "receipt": receipt_payload})


def bootstrap_attempt(bridge, sb, request):
    """Submit exactly one handoff from an empty native New Chat page."""
    cooperative = request.get("cooperative") is True
    protocol_v2 = type(request.get("bootstrapProtocol")) is int and request["bootstrapProtocol"] == 2
    resume_draft = request.get("resumeDraft") if protocol_v2 else False
    resume_progress = request.get("resumeProgress") if protocol_v2 else None
    expected_fields = {"bootstrap", "projectId", "intentId", "source", "handoff"}
    if protocol_v2:
        expected_fields.update({"cooperative", "bootstrapProtocol", "resumeDraft"})
        if "resumeProgress" in request:
            expected_fields.add("resumeProgress")
        if (not cooperative or type(resume_draft) is not bool or
                (resume_draft and (not isinstance(resume_progress, str) or
                                   resume_progress not in {"TEXT_ENTRY_AUTHORIZED", "DRAFT_READY"})) or
                (not resume_draft and resume_progress is not None)):
            raise bridge.Attention("BOOTSTRAP_REQUEST_INVALID")
    elif cooperative:
        expected_fields.add("cooperative")
    if set(request) != expected_fields or request["bootstrap"] is not True:
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
    typed_message = bridge.keyboard_safe_message(message)
    message_digest = bridge.digest(bridge.normalize_message(message))
    sink = bridge.NewConversationSink(source_url, ROOT / "browser-profile", 30, 20)
    existing_draft = False
    if protocol_v2 and resume_draft:
        # A new adapter process opens the fixed home once to load this profile's
        # saved composer. Never navigate to the source first or clear the draft.
        cdp = prepare_cdp_target(bridge, sb, bridge.NEW_CONVERSATION_HOME)
        observed = sink.wait_home_draft(cdp, message)
        if observed["classification"] == "EMPTY_HOME":
            raise bridge.Attention("BOOTSTRAP_DRAFT_MISSING")
        if observed["classification"] != "EXACT_OWNED":
            raise bridge.Attention("BOOTSTRAP_DRAFT_MISMATCH")
        existing_draft = observed["classification"] == "EXACT_OWNED"
        emit({"stage": "BOOTSTRAP_HOME_READY"})
    else:
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
    if protocol_v2:
        emit({"stage": "TEXT_ENTRY_AUTHORIZED", "projectId": project_id,
              "intentId": intent_id, "sourceGeneration": source["generation"],
              "messageDigest": message_digest})
        if read_request() != {"textEntry": True}:
            raise bridge.Attention("TEXT_ENTRY_NOT_AUTHORIZED")
    else:
        # Legacy requests retain their old unknown-outcome behavior. They may
        # never use the v2 draft-resume path after a prior submit checkpoint.
        emit({
            "stage": "BEFORE_BOOTSTRAP_SUBMIT",
            "projectId": project_id,
            "intentId": intent_id,
            "sourceGeneration": source["generation"],
            "messageDigest": message_digest,
        })
        if read_request() != {"submit": True}:
            raise bridge.Attention("SUBMISSION_NOT_AUTHORIZED")
    # After the checkpoint, even a typing exception may have submitted a USER
    # turn. Preserve unknown intent state and reconcile without another send.
    send_authorized = not protocol_v2
    try:
        if not existing_draft:
            sink.check_empty_home(cdp)
        editor_selector = sink.editor_selector(cdp, prevalidated=True)
        if not existing_draft:
            if protocol_v2:
                sink.insert_empty_home_draft(cdp, message)
            else:
                cdp.send_keys(editor_selector, typed_message, timeout=sink.ui_timeout)
        sink.check_typed_home(cdp, message)
        if protocol_v2:
            # Only after the parent's durable text-entry ACK. This native
            # formatting repair preserves text data, is idempotent, and uses
            # no keyboard or send event.
            sink.normalize_owned_home_draft(cdp, message)
        sink.check_plain_home_draft(cdp)
        if protocol_v2:
            emit({"stage": "DRAFT_READY", "projectId": project_id,
                  "intentId": intent_id, "sourceGeneration": source["generation"],
                  "messageDigest": message_digest})
            if read_request() != {"draftReady": True}:
                raise bridge.Attention("DRAFT_NOT_AUTHORIZED")
            sink.check_typed_home(cdp, message)
        send_selector = sink.wait_for_send_control(cdp, 20)
        if send_selector is None:
            raise bridge.Attention("BOOTSTRAP_SEND_CONTROL_MISSING")
        sink.check_typed_home(cdp, message)
        sink.check_plain_home_draft(cdp)
        send_selector = sink.wait_for_send_control(cdp, 5)
        if send_selector is None:
            raise bridge.Attention("BOOTSTRAP_SEND_CONTROL_MISSING")
        if protocol_v2:
            emit({"stage": "SEND_ATTEMPTED", "projectId": project_id,
                  "intentId": intent_id, "sourceGeneration": source["generation"],
                  "messageDigest": message_digest})
            if read_request() != {"submit": True}:
                raise bridge.Attention("SUBMISSION_NOT_AUTHORIZED")
            send_authorized = True
            sink.check_typed_home(cdp, message)
            sink.check_plain_home_draft(cdp)
        sink.smoke_path_submit(cdp, send_selector, sink.ui_timeout, editor_selector)
        sink.latch_issued_url(cdp)
        accepted_at = sink.wait_for_submission_accepted(cdp, 0, message_digest, message)
        emit({"stage": "BOOTSTRAP_SUBMISSION_ACCEPTED"})
        if not cooperative:
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
        if cooperative:
            # The fresh document must contain exactly the submitted USER
            # turn before the adapter releases the shared browser profile.
            if sink.user_message_digests(cdp) != [message_digest]:
                raise bridge.PostSubmitUnknown("BOOTSTRAP_RECEIPT_SEQUENCE_DRIFT")
        else:
            sink.prove_single_bootstrap_user(cdp, message)
        if receipt.message_sha256 != message_digest or receipt.target_sha256 != bridge.digest(sink.url):
            raise bridge.PostSubmitUnknown("BOOTSTRAP_RECEIPT_MISMATCH")
    except Exception as error:
        error_class = "".join(c for c in type(error).__name__.upper() if c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789")[:40]
        reason = ("BOOTSTRAP_POST_SUBMIT_" if send_authorized else "BOOTSTRAP_PRE_SEND_") + (error_class or "UNKNOWN")
        if isinstance(error, bridge.Attention):
            candidate = str(error)
            if candidate and len(candidate) <= 100 and all(c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789" for c in candidate):
                reason = candidate
        emit({"stage": "BOOTSTRAP_UNKNOWN", "reason": reason})
        return
    emit({
        "stage": "BOOTSTRAP_SUBMITTED" if cooperative else "BOOTSTRAP_COMPLETE",
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
            "completion": "RESPONSE_PENDING" if cooperative else "RESPONSE_COMPLETED",
        },
    })


def bootstrap_draft_inspect(bridge, sb, request):
    """Classify one saved home draft without typing, clicking, or clearing it."""
    if (set(request) != {"bootstrapDraftInspect", "bootstrapProtocol", "projectId", "intentId",
                         "source", "handoff"} or
            request["bootstrapDraftInspect"] is not True or
            type(request["bootstrapProtocol"]) is not int or request["bootstrapProtocol"] != 2):
        raise bridge.Attention("BOOTSTRAP_DRAFT_INSPECT_REQUEST_INVALID")
    source = request["source"]
    if not isinstance(source, dict) or set(source) != {"url", "digest", "generation"}:
        raise bridge.Attention("BOOTSTRAP_DRAFT_INSPECT_REQUEST_INVALID")
    source_url = bridge.canonical_conversation_url(source["url"])
    if (source_url != source["url"] or bridge.digest(source_url) != source["digest"] or
            type(source["generation"]) is not int or not 0 <= source["generation"] < 2**63):
        raise bridge.Attention("BOOTSTRAP_DRAFT_INSPECT_SOURCE_INVALID")
    project_id, intent_id = request["projectId"], request["intentId"]
    message = bridge.compose_bootstrap_message(project_id, intent_id, source_url, request["handoff"])
    expected_digest = bridge.digest(bridge.normalize_message(message))
    sink = bridge.NewConversationSink(source_url, ROOT / "browser-profile", 30, 20)
    controls = []
    try:
        # A fresh SeleniumBase process must activate the fixed home once to
        # load this profile's saved draft. Never open the source or clear text.
        cdp = prepare_cdp_target(bridge, sb, bridge.NEW_CONVERSATION_HOME)
        observed = sink.wait_home_draft(cdp, message)
        if observed["classification"] == "EXACT_OWNED":
            try:
                controls = bridge.CdpSink.cdp_value(cdp.evaluate("""(() => {
                    const editor = document.querySelector('#prompt-textarea, div.ProseMirror[contenteditable="true"]');
                    if (!editor) return [];
                    const scope = editor.closest('form') || editor.parentElement?.parentElement?.parentElement || editor;
                    return Array.from(scope.querySelectorAll('button, [role="button"], input[type="submit"]'))
                        .filter(node => node.getClientRects().length && getComputedStyle(node).visibility !== 'hidden' && getComputedStyle(node).display !== 'none')
                        .slice(-12).map(node => [node.tagName, node.id, node.getAttribute('data-testid') || '',
                            node.getAttribute('aria-label') || '', node.getAttribute('type') || '',
                            Boolean(node.disabled || node.getAttribute('aria-disabled') === 'true')])
                        .map(row => row.map(value => typeof value === 'string' ? Array.from(value).slice(0,48).join('') : value));
                })()"""))
                if not isinstance(controls, list):
                    controls = []
                controls = [row for row in controls[:12] if isinstance(row, list) and len(row) == 6
                            and all(isinstance(item, str) and len(item.encode('utf-8')) <= 192 for item in row[:5])
                            and type(row[5]) is bool]
            except Exception:
                controls = []
    except Exception:
        observed = {"classification": "UNAVAILABLE", "actualDigest": None, "actualBytes": 0}
    payload = {
        "stage": "BOOTSTRAP_DRAFT_INSPECTED",
        "projectId": project_id, "intentId": intent_id,
        "sourceGeneration": source["generation"], "sourceDigest": source["digest"],
        "expectedDigest": expected_digest,
        "actualDigest": observed["actualDigest"], "actualBytes": observed["actualBytes"],
        "classification": observed["classification"],
        "controls": controls,
    }
    while controls and len((json.dumps(payload, separators=(",", ":")) + "\n").encode("utf-8")) > 8192:
        controls.pop()
    emit(payload)


def bootstrap_draft_archive(bridge, sb, request):
    """Archive then clear only a fully owned legacy bootstrap draft."""
    expected_fields = {"bootstrapDraftArchive", "bootstrapProtocol", "archiveRecorded",
                       "projectId", "intentId", "source", "handoff"}
    if (set(request) != expected_fields or request["bootstrapDraftArchive"] is not True or
            type(request["bootstrapProtocol"]) is not int or request["bootstrapProtocol"] != 2 or
            type(request["archiveRecorded"]) is not bool):
        raise bridge.Attention("BOOTSTRAP_DRAFT_ARCHIVE_REQUEST_INVALID")
    source = request["source"]
    if not isinstance(source, dict) or set(source) != {"url", "digest", "generation"}:
        raise bridge.Attention("BOOTSTRAP_DRAFT_ARCHIVE_REQUEST_INVALID")
    source_url = bridge.canonical_conversation_url(source["url"])
    if (source_url != source["url"] or bridge.digest(source_url) != source["digest"] or
            type(source["generation"]) is not int or not 0 <= source["generation"] < 2**63):
        raise bridge.Attention("BOOTSTRAP_DRAFT_ARCHIVE_SOURCE_INVALID")
    project_id, intent_id = request["projectId"], request["intentId"]
    message = bridge.compose_bootstrap_message(project_id, intent_id, source_url, request["handoff"])
    message_digest = bridge.digest(bridge.normalize_message(message))
    scope = {"projectId": project_id, "intentId": intent_id,
             "sourceGeneration": source["generation"], "sourceDigest": source["digest"],
             "messageDigest": message_digest}
    sink = bridge.NewConversationSink(source_url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, bridge.NEW_CONVERSATION_HOME)
    observed = sink.wait_home_draft(cdp, message)
    if observed["classification"] == "EMPTY_HOME":
        if not request["archiveRecorded"]:
            raise bridge.Attention("BOOTSTRAP_DRAFT_MISSING")
        emit({"stage": "BOOTSTRAP_DRAFT_ARCHIVED", **scope, "actualBytes": 0})
        return
    if observed["classification"] != "EXACT_OWNED":
        raise bridge.Attention("BOOTSTRAP_DRAFT_MISMATCH")
    actual_text = sink.raw_editor_text(cdp)
    actual_bytes = len(actual_text.encode("utf-8"))
    if (actual_bytes > bridge.MAX_USER_MESSAGE_CHARS or
            bridge.normalize_message(actual_text) != bridge.normalize_message(message)):
        raise bridge.Attention("BOOTSTRAP_DRAFT_MISMATCH")
    sink.check_typed_home(cdp, message)
    ready = {"stage": "BOOTSTRAP_DRAFT_ARCHIVE_READY", **scope,
             "actualBytes": actual_bytes, "actualText": actual_text}
    # The standard adapter protocol reader accepts at most 8 KiB per frame.
    # Refuse a larger private witness before the parent could ACK or clear it.
    if len((json.dumps(ready, separators=(",", ":")) + "\n").encode("utf-8")) > 8192:
        raise bridge.Attention("BOOTSTRAP_ARCHIVE_FRAME_TOO_LARGE")
    emit(ready)
    if read_request() != {"archiveDraft": True}:
        raise bridge.Attention("BOOTSTRAP_DRAFT_ARCHIVE_NOT_AUTHORIZED")
    sink.check_typed_home(cdp, message)
    if sink.raw_editor_text(cdp) != actual_text:
        raise bridge.Attention("BOOTSTRAP_DRAFT_CHANGED")
    sink.clear_archived_home_draft(cdp, message, actual_text)
    emit({"stage": "BOOTSTRAP_DRAFT_ARCHIVED", **scope, "actualBytes": 0})


def bootstrap_draft_repair(bridge, sb, request):
    """Repair only a journal-authorized partial draft; never send or clear it."""
    fields = {"bootstrapDraftRepair", "bootstrapProtocol", "projectId", "intentId",
              "source", "handoff", "expectedRawDigest", "attemptRecorded"}
    if (set(request) != fields or request["bootstrapDraftRepair"] is not True or
            type(request["bootstrapProtocol"]) is not int or request["bootstrapProtocol"] != 2 or
            type(request["attemptRecorded"]) is not bool or
            not isinstance(request["expectedRawDigest"], str) or
            not bridge.SHA256_HEX.fullmatch(request["expectedRawDigest"])):
        raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_REQUEST_INVALID")
    source = request["source"]
    if not isinstance(source, dict) or set(source) != {"url", "digest", "generation"}:
        raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_REQUEST_INVALID")
    source_url = bridge.canonical_conversation_url(source["url"])
    if (source_url != source["url"] or bridge.digest(source_url) != source["digest"] or
            type(source["generation"]) is not int or not 0 <= source["generation"] < 2**63):
        raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_SOURCE_INVALID")
    project_id, intent_id = request["projectId"], request["intentId"]
    message = bridge.compose_bootstrap_message(project_id, intent_id, source_url, request["handoff"])
    scope = {"projectId": project_id, "intentId": intent_id,
             "sourceGeneration": source["generation"], "sourceDigest": source["digest"],
             "messageDigest": bridge.digest(bridge.normalize_message(message))}
    sink = bridge.NewConversationSink(source_url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, bridge.NEW_CONVERSATION_HOME)
    observed = sink.wait_home_draft(cdp, message)
    if request["attemptRecorded"]:
        # A saved write attempt can only be reconciled by a complete readback.
        # Never retry replacement, even when the partial draft still matches.
        if observed["classification"] != "EXACT_OWNED":
            raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_RECONCILIATION_UNVERIFIED")
        sink.check_typed_home(cdp, message)
        sink.check_plain_home_draft(cdp)
        emit({"stage": "BOOTSTRAP_DRAFT_REPAIRED", **scope, "reconciled": True})
        return
    if observed["classification"] != "MISMATCH":
        raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_PREFIX_INVALID")
    actual = sink.raw_editor_text(cdp)
    raw_digest = bridge.digest(actual)
    partial, full = bridge.normalize_message(actual), bridge.normalize_message(message)
    if raw_digest != request["expectedRawDigest"]:
        raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_CAS_CHANGED")
    if not partial or partial == full or not full.startswith(partial):
        raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_PREFIX_INVALID")
    sink.check_typed_home(cdp, actual)
    ready = {"stage": "BOOTSTRAP_DRAFT_REPAIR_READY", **scope,
             "actualBytes": len(actual.encode("utf-8")), "actualText": actual,
             "actualRawDigest": raw_digest}
    if len((json.dumps(ready, separators=(",", ":")) + "\n").encode("utf-8")) > 8192:
        raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_FRAME_TOO_LARGE")
    emit(ready)
    acknowledgement = read_request()
    if (not isinstance(acknowledgement, dict) or set(acknowledgement) != {"repairDraft"} or
            acknowledgement["repairDraft"] is not True):
        raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_NOT_AUTHORIZED")
    sink.repair_home_draft(cdp, message, actual)
    sink.check_typed_home(cdp, message)
    sink.check_plain_home_draft(cdp)
    emit({"stage": "BOOTSTRAP_DRAFT_REPAIRED", **scope, "reconciled": False})


def bounded_response_observation(bridge, sink, cdp, message, message_digest,
                                 *, timeout=10, clock=None, sleep=None):
    """Allow fresh-document hydration without opening another browser or sending.

    Every sample retains the exact receipt/target/body checks. A completed
    frame still needs a separated second proof; a remount returns pending.
    """
    clock = clock or time.monotonic
    sleep = sleep or time.sleep
    deadline = clock() + timeout
    unmounted = object()
    unmounted_reason = "RESPONSE_USER_DOCUMENT_NOT_HYDRATED"

    def sample():
        nonlocal unmounted_reason
        try:
            return sink.observe_cooperative_response(cdp, message, message_digest)
        except bridge.Attention as error:
            reason = str(error)
            if reason == "RESPONSE_TARGET_DRIFT":
                current = cdp.get_current_url()
                if not isinstance(current, str) or current not in {
                        "about:blank", "https://chatgpt.com/", "https://chat.openai.com/"}:
                    raise
                # CDP activation can return before navigation/hydration. Only
                # wait: never select, navigate, reload or inspect a foreign chat.
            elif reason != "RESPONSE_USER_DOCUMENT_NOT_HYDRATED":
                raise
            unmounted_reason = reason
            # This is not positive pending/receipt evidence. If the document
            # never mounts, preserve the refusal at the original deadline.
            if clock() >= deadline:
                raise
            return unmounted

    while True:
        stage = sample()
        if stage not in {unmounted, "RESPONSE_PENDING", "RESPONSE_COMPLETE"}:
            raise bridge.Attention("RESPONSE_PROBE_STATE_INVALID")
        if stage == "RESPONSE_COMPLETE":
            sleep(.25)
            stage = sample()
            if stage not in {unmounted, "RESPONSE_PENDING", "RESPONSE_COMPLETE"}:
                raise bridge.Attention("RESPONSE_PROBE_STATE_INVALID")
            if stage is not unmounted:
                return stage
        remaining = deadline - clock()
        if remaining <= 0:
            if stage is unmounted:
                raise bridge.Attention(unmounted_reason)
            return stage
        sleep(min(.5, remaining))


def bootstrap_probe(bridge, sb, request):
    """Observe a persisted bootstrap receipt without sending another USER turn."""
    if set(request) != {"bootstrapProbe", "projectId", "intentId", "source", "handoff", "receipt"} or request["bootstrapProbe"] is not True:
        raise bridge.Attention("BOOTSTRAP_PROBE_REQUEST_INVALID")
    source, receipt = request["source"], request["receipt"]
    if not isinstance(source, dict) or set(source) != {"url", "digest", "generation"} or not isinstance(receipt, dict):
        raise bridge.Attention("BOOTSTRAP_PROBE_REQUEST_INVALID")
    project_id, intent_id = request["projectId"], request["intentId"]
    source_url = bridge.canonical_conversation_url(source["url"])
    if (source_url != source["url"] or bridge.digest(source_url) != source["digest"] or
            not isinstance(source["generation"], int) or isinstance(source["generation"], bool) or
            not 0 <= source["generation"] < 2**63):
        raise bridge.Attention("BOOTSTRAP_PROBE_SOURCE_INVALID")
    message = bridge.compose_bootstrap_message(project_id, intent_id, source_url, request["handoff"])
    message_digest = bridge.digest(bridge.normalize_message(message))
    destination = bridge.canonical_conversation_url(receipt.get("destinationUrl"))
    if (destination != receipt.get("destinationUrl") or destination == source_url or
            receipt.get("schemaVersion") != 1 or receipt.get("projectId") != project_id or
            receipt.get("intentId") != intent_id or receipt.get("sourceGeneration") != source["generation"] or
            receipt.get("sourceDigest") != source["digest"] or
            receipt.get("destinationDigest") != bridge.digest(destination) or
            receipt.get("messageDigest") != message_digest or
            receipt.get("evidence") != "EXACT_USER_MESSAGE_APPENDED" or
            receipt.get("completion") != "RESPONSE_PENDING" or
            not isinstance(receipt.get("sentUtc"), int) or isinstance(receipt.get("sentUtc"), bool) or
            receipt["sentUtc"] <= 0):
        raise bridge.Attention("BOOTSTRAP_PROBE_RECEIPT_INVALID")
    sink = bridge.CdpSink(destination, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, destination)
    stage = bounded_response_observation(bridge, sink, cdp, message, message_digest)
    if stage == "RESPONSE_PENDING":
        emit({"stage": "BOOTSTRAP_RESPONSE_PENDING", "projectId": project_id,
              "intentId": intent_id, "destinationDigest": receipt["destinationDigest"]})
    elif stage == "RESPONSE_COMPLETE":
        completed = dict(receipt)
        completed["completion"] = "RESPONSE_COMPLETED"
        emit({"stage": "BOOTSTRAP_COMPLETE", "receipt": completed})
    else:
        raise bridge.Attention("BOOTSTRAP_PROBE_STATE_INVALID")


def bootstrap_quarantine_inspect(bridge, sb, request):
    """Read a saved orphan candidate without creating a receipt or any write authority."""
    if (set(request) != {"bootstrapQuarantineInspect", "projectId", "intentId",
                         "source", "handoff", "candidateUrl"} or
            request["bootstrapQuarantineInspect"] is not True):
        raise bridge.Attention("BOOTSTRAP_QUARANTINE_REQUEST_INVALID")
    source = request["source"]
    if not isinstance(source, dict) or set(source) != {"url", "digest", "generation"}:
        raise bridge.Attention("BOOTSTRAP_QUARANTINE_REQUEST_INVALID")
    source_url = bridge.canonical_conversation_url(source["url"])
    candidate = bridge.canonical_conversation_url(request["candidateUrl"])
    if (source_url != source["url"] or bridge.digest(source_url) != source["digest"] or
            type(source["generation"]) is not int or not 0 < source["generation"] < 2**63 or
            candidate != request["candidateUrl"] or candidate == source_url):
        raise bridge.Attention("BOOTSTRAP_QUARANTINE_SCOPE_INVALID")
    message = bridge.compose_bootstrap_message(
        request["projectId"], request["intentId"], source_url, request["handoff"])
    expected = bridge.digest(bridge.normalize_message(message))
    sink = bridge.CdpSink(candidate, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, candidate)
    stage = bounded_response_observation(bridge, sink, cdp, message, expected)
    # Pending can mean the document never hydrated. A historical probe has a
    # receipt already, but this diagnostic must independently see the USER now.
    if not sink.exact(cdp.get_current_url()) or sink.document_ready_state(cdp) != "complete":
        raise bridge.Attention("BOOTSTRAP_QUARANTINE_USER_UNPROVEN")
    users = sink.user_message_digests(cdp)
    count, turns_after = sink.trusted_turn_anchor_state(cdp, message)
    if (users.count(expected) != 1 or not users or users[-1] != expected or
            count != 1 or turns_after > 1 or not sink.exact(cdp.get_current_url())):
        raise bridge.Attention("BOOTSTRAP_QUARANTINE_USER_UNPROVEN")
    emit({"stage": "BOOTSTRAP_QUARANTINE_INSPECTED", "projectId": request["projectId"],
          "intentId": request["intentId"], "sourceGeneration": source["generation"],
          "sourceDigest": source["digest"], "candidateDigest": bridge.digest(candidate),
          "messageDigest": expected, "existingUserMessageVerified": True,
          "responseState": stage, "observationOnly": True})


def bounded_source_snapshot(bridge, sink, cdp, *, timeout=10, clock=None, sleep=None,
                            max_message_bytes=None, review_pair=False):
    """Wait in one document for matching complete final message frames.

    A still-incomplete snapshot remains explicitly incomplete at the deadline;
    no reload, retry control, editor operation or submission is permitted.
    """
    clock = clock or time.monotonic
    sleep = sleep or time.sleep
    deadline = clock() + timeout
    previous = None
    while True:
        if not sink.exact(cdp.get_current_url()):
            raise bridge.Attention("SOURCE_SNAPSHOT_SOURCE_DRIFT")
        options = {} if max_message_bytes is None else {"max_message_bytes": max_message_bytes}
        if review_pair:
            options["review_pair"] = True
        snapshot = sink.visible_message_snapshot(cdp, **options)
        if not sink.exact(cdp.get_current_url()):
            raise bridge.Attention("SOURCE_SNAPSHOT_SOURCE_DRIFT")
        messages = snapshot["messages"]
        ready = (messages and messages[-1]["role"] == "ASSISTANT" and
                 all(item["textComplete"] and item["revisionComplete"] for item in messages[-2:]))
        final = [(item["role"], item["text"]) for item in messages[-2:]] if ready else None
        if final is not None and final == previous:
            return snapshot
        previous = final
        remaining = deadline - clock()
        if remaining <= 0:
            if ready:
                # One complete-looking final frame at the deadline has not
                # earned the separated stability proof. Retain its text only
                # as an explicitly incomplete private observation.
                snapshot = dict(snapshot, messages=[dict(item) for item in messages],
                                truncated=True, readinessReason="FINAL_FRAMES_NOT_STABLE")
                for item in snapshot["messages"][-2:]:
                    item.update(textComplete=False, revisionComplete=False, revisionHash=None)
                if "coverage" in snapshot:
                    snapshot["coverage"] = dict(snapshot["coverage"],
                        textIncomplete=sum(not item["textComplete"] for item in snapshot["messages"]),
                        revisionIncomplete=sum(not item["revisionComplete"] for item in snapshot["messages"]))
            return snapshot
        sleep(min(.25 if ready else .5, remaining))


def source_snapshot(bridge, sb, request):
    """Read an exact idle source chat only on an explicit private request."""
    review = request.get("reviewEvent")
    keys = {"snapshot", "projectId", "source"} | ({"reviewEvent"} if review is not None else set())
    if set(request) != keys or request["snapshot"] is not True:
        raise bridge.Attention("SOURCE_SNAPSHOT_REQUEST_INVALID")
    if review is not None and (
            not isinstance(review, dict) or set(review) != {"eventId", "message", "messageDigest"} or
            not isinstance(review["eventId"], str) or not bridge.SAFE_ID.fullmatch(review["eventId"]) or
            not isinstance(review["message"], str) or not 0 < len(review["message"].encode("utf-8")) <= 2048 or
            bridge.normalize_message(review["message"]) != review["message"] or
            review["messageDigest"] != bridge.digest(review["message"])):
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
    def require_review_turn():
        if review is not None and sink.trusted_turn_anchor_state(cdp, review["message"]) != (1, 1):
            raise bridge.Attention("SOURCE_REVIEW_SEQUENCE_UNPROVEN")
    require_review_turn()
    snapshot = bounded_source_snapshot(bridge, sink, cdp,
        max_message_bytes=32768 if review is not None else 8192, review_pair=review is not None)
    if review is not None:
        messages = snapshot["messages"]
        if (len(messages) != 2 or [item["role"] for item in messages] != ["USER", "ASSISTANT"] or
                not messages[0]["textComplete"] or
                bridge.normalize_message(messages[0]["text"]) != review["message"]):
            raise bridge.Attention("SOURCE_REVIEW_USER_WITNESS_REQUIRED")
        require_review_turn()
    if snapshot["messages"] and snapshot["messages"][-1]["role"] == "ASSISTANT":
        last = snapshot["messages"][-1]
        normalized = bridge.normalize_message(last["text"]).replace("’", "'").replace("‘", "'").lower()
        if not last["textComplete"] or "this response couldn't load" in normalized:
            snapshot["bodyDiagnostics"] = sink.message_body_diagnostics(cdp)
    if not sink.exact(cdp.get_current_url()):
        raise bridge.Attention("SOURCE_SNAPSHOT_SOURCE_DRIFT")
    payload = {
        "stage": "SOURCE_SNAPSHOT",
        "projectId": request["projectId"],
        "sourceGeneration": source["generation"],
        "sourceDigest": source["digest"],
        "snapshot": snapshot,
    }
    # The private snapshot reader has a separate wire allowance: JSON escaping
    # and identity metadata must not consume the 8 KiB raw-text budget and
    # discard a complete USER witness preceding a complete assistant answer.
    # Ordinary frames and generic context limits stay unchanged; receipt-bound
    # review pair text limits were applied above.
    while (len(json.dumps(payload, separators=(",", ":")).encode("utf-8")) + 1 >
           SOURCE_SNAPSHOT_RESPONSE_BYTES):
        messages = snapshot["messages"]
        snapshot["truncated"] = True
        if review is not None:
            # Escaping/metadata may exhaust the unchanged64KiB wire cap.
            # Retain the exact USER and honestly mark the answer incomplete.
            if len(messages) != 2 or not messages[-1]["text"]:
                raise bridge.Attention("SOURCE_SNAPSHOT_TOO_LARGE")
            answer = messages[-1]
            answer["text"] = answer["text"][:len(answer["text"]) // 2]
            answer.update(textComplete=False, revisionComplete=False, revisionHash=None)
        elif len(messages) > 1:
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


def submission_diagnostic_sample(bridge, sink, cdp, message):
    """Return bounded comparisons, never draft text or unrelated draft hashes."""
    if not sink.exact(cdp.get_current_url()):
        raise bridge.Attention("SUBMISSION_DIAGNOSE_TARGET_CHANGED")
    expected = bridge.normalize_message(message)
    observation = {
        "composerClass": "UNAVAILABLE", "composerBytes": 0,
        "legacyAvailable": False, "readersAgree": None,
        "legacyMatchesExpected": None, "expectedUserCount": None,
        "lastUserMatches": None, "trustedMatches": None, "turnsAfter": None,
        "stable": False, "coverage": "VISIBLE_BOUNDED_ONLY",
    }
    private_tokens = []
    if sink.document_ready_state(cdp) == "complete":
        try:
            actual = sink.composer_plain_text(cdp)
            normalized = bridge.normalize_message(actual)
            observation["composerBytes"] = len(actual.encode("utf-8"))
            observation["composerClass"] = ("EMPTY" if not normalized else
                "EXACT" if normalized == expected else
                "PREFIX" if expected.startswith(normalized) else "OTHER")
            private_tokens.append(bridge.digest(normalized))
            try:
                legacy, available = sink.legacy_composer_observation(cdp)
                if not available:
                    raise ValueError("legacy read unavailable")
                if len(legacy.encode("utf-8")) > bridge.MAX_USER_MESSAGE_CHARS:
                    raise ValueError("bounded legacy read required")
                legacy = bridge.normalize_message(legacy)
                observation.update(legacyAvailable=True, readersAgree=legacy == normalized,
                                   legacyMatchesExpected=legacy == expected)
                private_tokens.append(bridge.digest(legacy))
            except Exception:
                pass
        except Exception:
            pass
        try:
            users = sink.user_message_digests(cdp)
            count, after = sink.trusted_turn_anchor_state(cdp, message)
            if (len(users) > bridge.MAX_USER_MESSAGES or type(count) is not int or
                    type(after) is not int or not 0 <= count <= bridge.MAX_USER_MESSAGES or
                    not 0 <= after <= 1024):
                raise ValueError("bounded turn read required")
            expected_digest = bridge.digest(expected)
            observation.update(expectedUserCount=users.count(expected_digest),
                lastUserMatches=bool(users) and users[-1] == expected_digest,
                trustedMatches=count, turnsAfter=after)
            private_tokens.extend(users)
        except Exception:
            pass
    if not sink.exact(cdp.get_current_url()):
        raise bridge.Attention("SUBMISSION_DIAGNOSE_TARGET_CHANGED")
    # Comparison tokens are private working values and never enter the frame.
    return observation, (json.dumps(observation, sort_keys=True), tuple(private_tokens))


def diagnose_submission(bridge, sb, request, *, clock=None, sleep=None):
    """Inspect one ambiguous source review without typing or changing its state."""
    if (set(request) != {"diagnoseSubmission", "event", "target"} or
            request["diagnoseSubmission"] is not True):
        raise bridge.Attention("SUBMISSION_DIAGNOSE_REQUEST_INVALID")
    event, target = request["event"], request["target"]
    if (not isinstance(event, dict) or not isinstance(target, dict) or
            set(target) != {"url", "digest", "generation"} or
            not isinstance(event.get("message"), str) or
            not isinstance(event.get("projectId"), str) or
            not isinstance(event.get("eventId"), str) or
            not bridge.SAFE_ID.fullmatch(event["projectId"]) or
            not bridge.SAFE_ID.fullmatch(event["eventId"]) or
            event.get("eventType") != "source_review" or
            type(target["generation"]) is not int or target["generation"] <= 0 or
            event.get("targetGeneration") != target["generation"] or
            not event["message"] or len(event["message"].encode("utf-8")) > 2048 or
            bridge.normalize_message(event["message"]) != event["message"]):
        raise bridge.Attention("SUBMISSION_DIAGNOSE_REQUEST_INVALID")
    url = bridge.canonical_conversation_url(target["url"])
    if url != target["url"] or bridge.digest(url) != target["digest"]:
        raise bridge.Attention("SUBMISSION_DIAGNOSE_TARGET_CHANGED")
    sink = bridge.CdpSink(url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, url)
    clock, sleep = clock or time.monotonic, sleep or time.sleep
    deadline = clock() + 10
    previous = None
    while True:
        observation, comparison = submission_diagnostic_sample(bridge, sink, cdp, event["message"])
        available = observation["composerClass"] != "UNAVAILABLE" or observation["expectedUserCount"] is not None
        if previous == comparison and available:
            observation["stable"] = True
            break
        previous = comparison
        remaining = deadline - clock()
        if remaining <= 0:
            break
        sleep(min(.25, remaining))
    emit({"stage": "SUBMISSION_DIAGNOSED", "projectId": event["projectId"],
          "eventId": event["eventId"], "targetGeneration": target["generation"],
          "targetDigest": target["digest"], "messageDigest": bridge.digest(event["message"]),
          "observation": observation})


def reconcile_submission(bridge, sb, event, target, *, cooperative=False):
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
    # Reconciliation never re-submits the USER wake. Cooperative mode leaves
    # the independently scheduled response probe to observe the assistant.
    emit({"stage": "USER_MESSAGE_APPENDED", "receipt": receipt_payload})
    if not cooperative:
        sink.wait_for_response_completion(
            cdp, message, observer=lambda stage: emit({"stage": stage})
        )
    emit({"stage": "SENT_PENDING_RESPONSE" if cooperative else "SENT", "receipt": receipt_payload})


def observe_response(bridge, sb, request):
    """Read one exact sent turn on a newly opened target; never interact with it."""
    if set(request) != {"observeResponse", "event", "target", "receipt"} or request["observeResponse"] is not True:
        raise bridge.Attention("RESPONSE_PROBE_REQUEST_INVALID")
    event, target, receipt = request["event"], request["target"], request["receipt"]
    if not all(isinstance(value, dict) for value in (event, target, receipt)):
        raise bridge.Attention("RESPONSE_PROBE_REQUEST_INVALID")
    if set(target) != {"url", "digest", "generation"} or set(receipt) != {
        "schemaVersion", "eventId", "targetGeneration", "targetDigest",
        "messageDigest", "sentUtc", "evidence",
    }:
        raise bridge.Attention("RESPONSE_PROBE_REQUEST_INVALID")
    url = bridge.canonical_conversation_url(target["url"])
    if url != target["url"] or bridge.digest(url) != target["digest"]:
        raise bridge.Attention("RESPONSE_PROBE_TARGET_INVALID")
    generation = target["generation"]
    if not isinstance(generation, int) or isinstance(generation, bool) or not 0 < generation < 2**63:
        raise bridge.Attention("RESPONSE_PROBE_TARGET_INVALID")
    message = event.get("message")
    event_id = event.get("eventId")
    expected = bridge.digest(bridge.normalize_message(message)) if isinstance(message, str) else None
    if (not isinstance(event_id, str) or not bridge.SAFE_ID.fullmatch(event_id) or
            not isinstance(message, str) or not message or len(message) > 2048 or
            type(event.get("targetGeneration")) is not int or
            event["targetGeneration"] != generation or
            type(receipt["schemaVersion"]) is not int or receipt["schemaVersion"] != 1 or
            receipt["eventId"] != event_id or
            type(receipt["targetGeneration"]) is not int or
            receipt["targetGeneration"] != generation or receipt["targetDigest"] != target["digest"] or
            receipt["messageDigest"] != expected or
            not isinstance(receipt["sentUtc"], int) or isinstance(receipt["sentUtc"], bool) or
            receipt["sentUtc"] <= 0 or receipt["evidence"] != "EXACT_USER_MESSAGE_APPENDED"):
        raise bridge.Attention("RESPONSE_PROBE_RECEIPT_INVALID")
    sink = bridge.CdpSink(url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, url)
    stage = bounded_response_observation(bridge, sink, cdp, message, expected)
    if stage not in {"RESPONSE_PENDING", "RESPONSE_COMPLETE"}:
        raise bridge.Attention("RESPONSE_PROBE_STATE_INVALID")
    frame = {"stage": stage, "eventId": event_id, "targetGeneration": generation,
             "targetDigest": target["digest"], "messageDigest": expected}
    reason = getattr(sink, "response_observation_reason", None)
    if reason is not None:
        pending = {"DOCUMENT_LOADING", "GENERATION_ACTIVE", "EDITOR_NOT_READY",
                   "ASSISTANT_NOT_MOUNTED", "ASSISTANT_BODY_UNPROVEN",
                   "ASSISTANT_COMPLETION_ACTION_UNPROVEN"}
        if not isinstance(reason, str) or (reason not in pending if stage == "RESPONSE_PENDING" else reason != "COMPLETE"):
            raise bridge.Attention("RESPONSE_PROBE_DIAGNOSTIC_INVALID")
        frame["diagnosticReason"] = reason
    controls = getattr(sink, "response_observation_controls", None)
    if controls is not None:
        if (reason is None or not bridge.valid_response_control_diagnostics(controls) or
                (stage == "RESPONSE_COMPLETE" and
                 not (controls["assistantBodyPresent"] and controls["completionActionFound"]))):
            raise bridge.Attention("RESPONSE_PROBE_DIAGNOSTIC_INVALID")
        frame["completionControls"] = controls
    emit(frame)


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
                        if request.get("bootstrapQuarantineInspect") is True:
                            bootstrap_quarantine_inspect(bridge, sb, request)
                        elif request.get("bootstrapDraftRepair") is True:
                            bootstrap_draft_repair(bridge, sb, request)
                        elif request.get("bootstrapDraftArchive") is True:
                            bootstrap_draft_archive(bridge, sb, request)
                        elif request.get("bootstrapDraftInspect") is True:
                            bootstrap_draft_inspect(bridge, sb, request)
                        elif request.get("bootstrapProbe") is True:
                            bootstrap_probe(bridge, sb, request)
                        elif request.get("observeResponse") is True:
                            observe_response(bridge, sb, request)
                        elif request.get("snapshot") is True:
                            source_snapshot(bridge, sb, request)
                        elif request.get("bootstrap") is True:
                            bootstrap_attempt(bridge, sb, request)
                        elif request.get("diagnoseSubmission") is True:
                            diagnose_submission(bridge, sb, request)
                        elif request.get("reconcile") is True:
                            reconcile_submission(
                                bridge, sb, request["event"], request["target"],
                                cooperative=request.get("cooperative") is True,
                            )
                        elif set(request) <= {"event", "target", "cooperative"}:
                            attempt(bridge, sb, request["event"], request["target"],
                                    cooperative=request.get("cooperative") is True)
                        else:
                            raise bridge.Attention("BROWSER_REQUEST_UNSUPPORTED")
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
