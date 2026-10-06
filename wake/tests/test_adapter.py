import hashlib
import importlib.util
import io
import json
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("independent_adapter", SOURCE / "adapter.py")
adapter = importlib.util.module_from_spec(spec)
with patch.object(sys, "argv", [str(SOURCE / "adapter.py"), str(SOURCE / "target")]):
    spec.loader.exec_module(adapter)
bridge_spec = importlib.util.spec_from_file_location("reviewed_primitives", SOURCE.parent / "scripts/wake_bridge.py")
bridge = importlib.util.module_from_spec(bridge_spec)
sys.modules[bridge_spec.name] = bridge
bridge_spec.loader.exec_module(bridge)


class FakeCdp:
    def __init__(self):
        self.url = "https://chatgpt.com/c/exact"
        self.draft = ""
        self.sent = False
        self.timeline = []
        self.keys = []
    def get_current_url(self): return self.url
    def get(self, url): self.url = url
    def press_keys(self, selector, text, timeout):
        self.keys.append((selector, text, timeout))
        self.draft = text


class FakeSink:
    def __init__(self, url, profile, ready, submit): self.url, self.ui_timeout = url, ready
    def wait_for_page_readiness(self, sb, cdp, observer=None):
        if observer is not None: observer("SAME_CONVERSATION", "READY")
        return cdp
    def wait_for_idle(self, cdp, timeout): pass
    def pre_typing_ready(self, cdp):
        if cdp.draft: raise bridge.Attention("EXISTING_DRAFT")
    def editor_selector(self, cdp, *, prevalidated=False):
        if not prevalidated:
            raise AssertionError("adapter must resolve only after pre-typing validation")
        return bridge.EDITOR
    def pre_submit_receipt_snapshot(self, cdp, digest): return []
    def best_effort_present_browser(self, cdp): pass
    def text(self, cdp): return cdp.draft
    def wait_for_send_control(self, cdp, timeout): return "send"
    def exact(self, url): return url == self.url
    def smoke_path_submit(self, cdp, selector, timeout, editor_selector):
        cdp.sent = True
        cdp.timeline.append("submit")
    def wait_for_submission_accepted(self, cdp, prior, digest, message):
        cdp.timeline.append("accepted")
        return 99.0
    def durable_receipt_round_trip(self, cdp):
        cdp.timeline.append("round-trip")
        return cdp
    def confirm_exact_receipt(self, cdp, prior, digest, record, message, wall_time=None):
        cdp.timeline.append("confirm")
        sent_at = wall_time() if wall_time is not None else 100
        return types.SimpleNamespace(record_id=record, target_sha256=bridge.digest(self.url), message_sha256=digest, browser_sent_at_unix=sent_at)
    def wait_for_response_completion(self, cdp, digest, observer=None):
        cdp.timeline.append("response")
        if observer is not None:
            observer("GENERATION_WAIT")
            observer("RESPONSE_COMPLETED")
    def reconcile_persisted_receipt(self, cdp, digest, message, record, target_sha256):
        cdp.timeline.append("reconcile")
        cdp.reconciled_message = message
        return types.SimpleNamespace(record_id=record, target_sha256=target_sha256, message_sha256=digest, browser_sent_at_unix=101)


class AdapterTests(unittest.TestCase):
    def test_initial_navigation_enters_cdp_directly_on_same_exact_url(self):
        cdp = FakeCdp()
        calls = []
        class SB:
            def __init__(self):
                self.cdp = None
            def uc_open_with_reconnect(self, *_args, **_kwargs):
                raise AssertionError("fresh CDP navigation must not UC-warm first")
            def activate_cdp_mode(self, *args, **kwargs):
                calls.append(("cdp", args, kwargs))
                self.cdp = cdp
        sb = SB()
        result = adapter.prepare_cdp_target(
            bridge, sb, "https://chatgpt.com/c/exact"
        )
        self.assertIs(cdp, result)
        self.assertEqual(
            [("cdp", ("https://chatgpt.com/c/exact",), {})],
            calls,
        )

    def run_attempt(self, acknowledgement, *, cdp_type=FakeCdp, sink_type=FakeSink):
        cdp = cdp_type()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        target = {"url": cdp.url, "digest": bridge.digest(cdp.url), "generation": 1}
        event = {"eventId": "review-1", "message": "bounded wake", "targetGeneration": 1}
        emitted = []
        def ack():
            self.assertEqual(
                [item["stage"] for item in emitted],
                [
                    "CDP_REUSE_BEGIN",
                    "CDP_REUSE_DONE",
                    "TARGET_OPEN",
                    "READINESS_OBSERVED",
                    "PAGE_READY",
                    "CHAT_IDLE",
                    "PREWRITE_READY",
                    "RECEIPT_BASELINE_READY",
                    "PREWRITE_CONFIRMED",
                    "BEFORE_SUBMIT",
                ],
            )
            self.assertFalse(cdp.sent)
            return acknowledgement(cdp)
        with patch.object(bridge, "CdpSink", sink_type), patch.object(adapter, "read_request", ack), patch.object(adapter, "emit", emitted.append):
            try: adapter.attempt(bridge, sb, event, target)
            except bridge.Attention as error: return cdp, emitted, str(error)
        return cdp, emitted, None

    def test_success_preserves_sender_until_response_then_proves_durable_receipt(self):
        cdp, emitted, error = self.run_attempt(lambda cdp: {"submit": True})
        self.assertIsNone(error)
        self.assertTrue(cdp.sent)
        self.assertEqual(
            cdp.timeline,
            ["submit", "accepted", "response", "round-trip", "confirm"],
        )
        self.assertEqual(
            [item["stage"] for item in emitted[-5:]],
            [
                "SUBMISSION_ACCEPTED",
                "GENERATION_WAIT",
                "RESPONSE_COMPLETED",
                "USER_MESSAGE_APPENDED",
                "SENT",
            ],
        )
        self.assertEqual(
            emitted[-2]["receipt"]["evidence"],
            "EXACT_USER_MESSAGE_APPENDED",
        )
        self.assertEqual(emitted[-2]["receipt"]["sentUtc"], 99)
        self.assertEqual(emitted[-1]["receipt"], emitted[-2]["receipt"])

    def test_no_permission_never_submits(self):
        cdp, _, error = self.run_attempt(lambda cdp: {"submit": False})
        self.assertFalse(cdp.sent)
        self.assertEqual(error, "SUBMISSION_NOT_AUTHORIZED")

    def test_navigation_during_authorization_wait_never_submits(self):
        def navigate(cdp): cdp.url = "https://chatgpt.com/c/wrong"; return {"submit": True}
        cdp, _, error = self.run_attempt(navigate)
        self.assertFalse(cdp.sent)
        self.assertEqual(error, "TARGET_IDENTITY_MISMATCH")

    def test_draft_change_during_authorization_wait_never_submits(self):
        def edit(cdp): cdp.draft = "operator draft"; return {"submit": True}
        cdp, _, error = self.run_attempt(edit)
        self.assertFalse(cdp.sent)
        self.assertEqual(error, "TYPING_NOT_CONFIRMED")

    def test_alternate_editor_is_used_before_submission_authorization(self):
        alternate = bridge.EDITOR_SELECTORS[1]
        case = self

        class AlternateEditorCdp(FakeCdp):
            def press_keys(self, selector, text, timeout):
                if selector == bridge.EDITOR:
                    raise RuntimeError("legacy selector is unavailable")
                case.assertEqual(alternate, selector)
                super().press_keys(selector, text, timeout)

        class AlternateEditorSink(FakeSink):
            def editor_selector(self, cdp, *, prevalidated=False):
                case.assertTrue(prevalidated)
                return alternate

        cdp, _emitted, error = self.run_attempt(
            lambda _cdp: {"submit": False},
            cdp_type=AlternateEditorCdp,
            sink_type=AlternateEditorSink,
        )
        self.assertEqual("SUBMISSION_NOT_AUTHORIZED", error)
        self.assertFalse(cdp.sent)
        self.assertEqual([(alternate, "bounded wake", 30)], cdp.keys)

    def test_reconciliation_observes_only_and_never_submits(self):
        cdp = FakeCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        target = {"url": cdp.url, "digest": bridge.digest(cdp.url), "generation": 1}
        event = {"eventId": "review-1", "message": "bounded wake", "targetGeneration": 1}
        emitted = []
        with patch.object(bridge, "CdpSink", FakeSink), patch.object(adapter, "emit", emitted.append):
            adapter.reconcile_submission(bridge, sb, event, target)
        self.assertFalse(cdp.sent)
        self.assertEqual(cdp.draft, "")
        # Reconciliation proves the existing USER receipt, then observes the
        # resulting assistant response without typing or re-submitting the wake.
        self.assertEqual(cdp.timeline, ["reconcile", "response"])
        self.assertEqual(cdp.reconciled_message, "bounded wake")
        self.assertEqual(
            [item["stage"] for item in emitted],
            [
                "CDP_REUSE_BEGIN",
                "CDP_REUSE_DONE",
                "TARGET_OPEN",
                "PAGE_READY",
                "USER_MESSAGE_APPENDED",
                "GENERATION_WAIT",
                "RESPONSE_COMPLETED",
                "SENT",
            ],
        )
        self.assertEqual(
            emitted[-4]["receipt"]["evidence"],
            "EXACT_USER_MESSAGE_APPENDED",
        )
        self.assertEqual(emitted[-1]["receipt"], emitted[-4]["receipt"])

    def test_startup_reason_is_fixed_and_non_sensitive(self):
        session_not_created = type("SessionNotCreatedException", (Exception,), {})
        webdriver_error = type("WebDriverException", (Exception,), {})
        self.assertEqual(adapter.startup_reason(PermissionError("secret path")), "BROWSER_PROFILE_ACCESS_DENIED")
        self.assertEqual(adapter.startup_reason(FileNotFoundError("secret path")), "BROWSER_BINARY_OR_DRIVER_MISSING")
        self.assertEqual(adapter.startup_reason(session_not_created("secret browser detail")), "BROWSER_SESSION_NOT_CREATED")
        self.assertEqual(adapter.startup_reason(webdriver_error("secret browser detail")), "BROWSER_DRIVER_START_FAILED")
        self.assertEqual(adapter.startup_reason(RuntimeError("secret browser detail")), "BROWSER_SESSION_UNAVAILABLE")


if __name__ == "__main__": unittest.main()
