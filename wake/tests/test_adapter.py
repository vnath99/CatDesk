import hashlib
import importlib.util
import io
import json
import shutil
import subprocess
import sys
import tempfile
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
    def send_keys(self, selector, text, timeout):
        self.keys.append((selector, text, timeout))
        self.draft = text
    def press_keys(self, selector, text, timeout):
        raise AssertionError("human-speed press_keys must not type a message")


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


class FakeProbeSink:
    def __init__(self, url, profile, ready, submit): self.url = url
    def observe_cooperative_response(self, cdp, message, expected_digest):
        cdp.timeline.append("observe")
        if cdp.get_current_url() != self.url:
            raise bridge.Attention("RESPONSE_TARGET_DRIFT")
        if bridge.digest(bridge.normalize_message(message)) != expected_digest:
            raise bridge.Attention("RESPONSE_SEQUENCE_DRIFT")
        if hasattr(cdp, "probe_stages"):
            return cdp.probe_stages.pop(0)
        return cdp.probe_stage


class AdapterTests(unittest.TestCase):
    def test_source_snapshot_waits_for_complete_matching_frames_without_writes(self):
        now = [0.0]
        pauses = []
        cdp = FakeCdp()
        def frame(text, complete=True):
            return {"messages": [
                {"role":"USER", "text":"bounded wake", "textComplete":True, "revisionComplete":True},
                {"role":"ASSISTANT", "text":text, "textComplete":complete, "revisionComplete":complete},
            ]}
        frames = [frame("mounting", False), frame("first render"), frame("final answer"), frame("final answer")]
        class Sink:
            def exact(self, url): return url == "https://chatgpt.com/c/exact"
            def visible_message_snapshot(self, cdp):
                cdp.timeline.append("snapshot")
                return frames.pop(0)
        def pause(seconds):
            pauses.append(seconds)
            now[0] += seconds
        snapshot = adapter.bounded_source_snapshot(bridge, Sink(), cdp, clock=lambda:now[0], sleep=pause)
        self.assertEqual(snapshot["messages"][-1]["text"], "final answer")
        self.assertEqual(pauses, [.5, .25, .25])
        self.assertEqual(len(cdp.timeline), 4)
        self.assertFalse(cdp.sent)
        self.assertEqual(cdp.keys, [])

    def test_source_snapshot_deadline_preserves_incomplete_and_drift_never_retries(self):
        now = [0.0]
        cdp = FakeCdp()
        partial = {"messages":[{"role":"ASSISTANT", "text":"partial", "textComplete":False, "revisionComplete":False}]}
        class Sink:
            def exact(self, url): return url == "https://chatgpt.com/c/exact"
            def visible_message_snapshot(self, cdp):
                cdp.timeline.append("snapshot")
                return partial
        def pause(seconds): now[0] += seconds
        self.assertIs(adapter.bounded_source_snapshot(
            bridge, Sink(), cdp, timeout=1, clock=lambda:now[0], sleep=pause), partial)
        self.assertEqual(now[0], 1)
        self.assertFalse(partial["messages"][-1]["textComplete"])
        class Drift(Sink):
            def visible_message_snapshot(self, cdp):
                cdp.url = "https://chatgpt.com/c/foreign"
                return partial
        with self.assertRaisesRegex(bridge.Attention, "SOURCE_SNAPSHOT_SOURCE_DRIFT"):
            adapter.bounded_source_snapshot(bridge, Drift(), cdp, clock=lambda:now[0], sleep=pause)
        self.assertEqual(now[0], 1)
        self.assertFalse(cdp.sent)

    def test_single_complete_source_frame_at_deadline_is_not_verified(self):
        cdp = FakeCdp()
        original = {"messages":[{"role":"ASSISTANT", "text":"new frame", "textComplete":True,
                                 "revisionComplete":True, "revisionHash":"prior-hash"}],
                    "coverage":{"textIncomplete":0, "revisionIncomplete":0}}
        class Sink:
            def exact(self, url): return url == cdp.url
            def visible_message_snapshot(self, cdp): return original
        result = adapter.bounded_source_snapshot(bridge, Sink(), cdp, timeout=0, clock=lambda:0,
                                                 sleep=lambda _:self.fail("expired probe must not sleep"))
        self.assertEqual(result["readinessReason"], "FINAL_FRAMES_NOT_STABLE")
        self.assertFalse(result["messages"][-1]["textComplete"])
        self.assertFalse(result["messages"][-1]["revisionComplete"])
        self.assertIsNone(result["messages"][-1]["revisionHash"])
        self.assertEqual(result["coverage"]["textIncomplete"], 1)
        self.assertTrue(original["messages"][-1]["textComplete"])
        self.assertEqual(original["coverage"]["textIncomplete"], 0)
        self.assertFalse(cdp.sent)

    def test_fresh_document_probe_waits_within_one_browser_and_requires_stability(self):
        now = [0.0]
        sleeps = []
        def pause(seconds):
            sleeps.append(seconds)
            now[0] += seconds
        cdp = FakeCdp()
        cdp.probe_stages = ["RESPONSE_PENDING", "RESPONSE_PENDING",
                            "RESPONSE_COMPLETE", "RESPONSE_COMPLETE"]
        result = adapter.bounded_response_observation(
            bridge, FakeProbeSink(cdp.url, None, 0, 0), cdp, "bounded wake",
            bridge.digest("bounded wake"), clock=lambda: now[0], sleep=pause,
        )
        self.assertEqual(result, "RESPONSE_COMPLETE")
        self.assertEqual(sleeps, [.5, .5, .25])
        self.assertEqual(len(cdp.timeline), 4)
        self.assertFalse(cdp.sent)
        self.assertEqual(cdp.keys, [])

    def test_pending_probe_has_fixed_budget_and_errors_are_not_retried(self):
        now = [0.0]
        cdp = FakeCdp()
        cdp.probe_stage = "RESPONSE_PENDING"
        sink = FakeProbeSink(cdp.url, None, 0, 0)
        def pause(seconds): now[0] += seconds
        self.assertEqual(adapter.bounded_response_observation(
            bridge, sink, cdp, "bounded wake", bridge.digest("bounded wake"),
            timeout=1, clock=lambda: now[0], sleep=pause,
        ), "RESPONSE_PENDING")
        self.assertEqual(now[0], 1)
        self.assertEqual(len(cdp.timeline), 3)
        cdp.url = "https://chatgpt.com/c/foreign"
        with self.assertRaisesRegex(bridge.Attention, "RESPONSE_TARGET_DRIFT"):
            adapter.bounded_response_observation(
                bridge, sink, cdp, "bounded wake", bridge.digest("bounded wake"),
                clock=lambda: now[0], sleep=pause,
            )
        self.assertEqual(len(cdp.timeline), 4)
        self.assertEqual(now[0], 1)

    def test_empty_user_mount_wait_is_bounded_and_requires_new_complete_proof(self):
        for frames, timeout, expected in (
            ([None, "RESPONSE_COMPLETE", "RESPONSE_COMPLETE"], 1, "RESPONSE_COMPLETE"),
            (["RESPONSE_COMPLETE", None, "RESPONSE_COMPLETE", "RESPONSE_COMPLETE"], 1, "RESPONSE_COMPLETE"),
            ([None, None, None], 1, "RESPONSE_USER_DOCUMENT_NOT_HYDRATED"),
            ([None], 0, "RESPONSE_USER_DOCUMENT_NOT_HYDRATED"),
        ):
            with self.subTest(frames=frames, timeout=timeout):
                now = [0.0]
                samples = list(frames)
                cdp = FakeCdp()
                class Sink:
                    def observe_cooperative_response(self, cdp, message, digest):
                        cdp.timeline.append("observe")
                        stage = samples.pop(0)
                        if stage is None:
                            raise bridge.Attention("RESPONSE_USER_DOCUMENT_NOT_HYDRATED")
                        return stage
                def pause(seconds): now[0] += seconds
                def observe():
                    return adapter.bounded_response_observation(
                        bridge, Sink(), cdp, "bounded wake", bridge.digest("bounded wake"),
                        timeout=timeout, clock=lambda: now[0], sleep=pause)
                if expected.startswith("RESPONSE_USER_"):
                    with self.assertRaisesRegex(bridge.Attention, expected):
                        observe()
                else:
                    self.assertEqual(observe(), expected)
                self.assertEqual(samples, [])
                self.assertLessEqual(now[0], timeout)
                self.assertFalse(cdp.sent)
                self.assertEqual(cdp.keys, [])

    def test_response_wait_never_retries_scope_or_nonempty_user_failures(self):
        for reason in ("RESPONSE_TARGET_DRIFT", "LOGIN_OR_PROFILE_REQUIRED",
                       "RESPONSE_EXACT_USER_MISSING_OR_AMBIGUOUS", "RESPONSE_SEQUENCE_DRIFT",
                       "RESPONSE_EXPECTED_USER_DUPLICATED", "RESPONSE_USER_REPRESENTATION_MISMATCH",
                       "RESPONSE_EXPECTED_USER_NOT_FOUND", "RESPONSE_USER_VIEW_CHANGED",
                       "RESPONSE_EXPECTED_USER_BODY_MISMATCH"):
            with self.subTest(reason=reason):
                sink = types.SimpleNamespace(observe_cooperative_response=lambda *args: None)
                with patch.object(sink, "observe_cooperative_response", side_effect=bridge.Attention(reason)) as sample:
                    with self.assertRaisesRegex(bridge.Attention, reason):
                        adapter.bounded_response_observation(
                            bridge, sink, FakeCdp(), "bounded wake", bridge.digest("bounded wake"),
                            clock=lambda: 0, sleep=lambda _: self.fail("refusal must not wait"))
                self.assertEqual(sample.call_count, 1)

        with self.assertRaisesRegex(bridge.Attention, "RESPONSE_PROBE_STATE_INVALID"):
            adapter.bounded_response_observation(
                bridge, types.SimpleNamespace(observe_cooperative_response=lambda *args: None),
                FakeCdp(), "bounded wake", bridge.digest("bounded wake"), clock=lambda: 0,
                sleep=lambda _: self.fail("malformed state must not wait"))

    def test_only_empty_user_and_empty_anchor_view_is_mount_pending(self):
        cdp = FakeCdp()
        expected = bridge.digest("bounded wake")
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.CdpSink(cdp.url, Path(profile), 1, 1)
            for users, anchor, reason in (
                ([], (0, 0), "RESPONSE_USER_DOCUMENT_NOT_HYDRATED"),
                ([], (1, 0), "RESPONSE_USER_REPRESENTATION_MISMATCH"),
                (["0" * 64], (0, 0), "RESPONSE_EXPECTED_USER_NOT_FOUND"),
                (["0" * 64], (1, 0), "RESPONSE_USER_REPRESENTATION_MISMATCH"),
                ([expected, expected], (2, 0), "RESPONSE_EXPECTED_USER_DUPLICATED"),
            ):
                with self.subTest(users=users, anchor=anchor), patch.object(
                    bridge.CdpSink, "post_submit_visible_any", return_value=False
                ), patch.object(bridge.CdpSink, "document_ready_state", return_value="complete"), patch.object(
                    bridge.CdpSink, "user_message_digests", return_value=users
                ), patch.object(bridge.CdpSink, "trusted_turn_anchor_state", return_value=anchor), \
                        self.assertRaisesRegex(bridge.Attention, reason):
                    sink.observe_cooperative_response(cdp, "bounded wake", expected)

    @staticmethod
    def probe_request(url="https://chatgpt.com/c/exact", event_id="review-1"):
        target = {"url": url, "digest": bridge.digest(url), "generation": 1}
        event = {"eventId": event_id, "message": "bounded wake", "targetGeneration": 1}
        receipt = {
            "schemaVersion": 1, "eventId": event_id, "targetGeneration": 1,
            "targetDigest": target["digest"],
            "messageDigest": bridge.digest(bridge.normalize_message(event["message"])),
            "sentUtc": 99, "evidence": "EXACT_USER_MESSAGE_APPENDED",
        }
        return {"observeResponse": True, "event": event, "target": target, "receipt": receipt}

    def test_missing_user_scope_hint_is_bounded_read_only_and_not_positive_evidence(self):
        header = "CatDesk bootstrap.\nProject: alpha\nIntent: owned-intent\nPredecessor: exact"
        message = header + "\n\nPRIVATE_HANDOFF_EXPECTED"
        for texts, reason in (
            ([header + " changed body"], "RESPONSE_EXPECTED_USER_BODY_MISMATCH"),
            ([header], "RESPONSE_EXPECTED_USER_BODY_MISMATCH"),
            ([header.replace("owned-intent", "other-intent") + " changed body"], "RESPONSE_EXPECTED_USER_NOT_FOUND"),
        ):
            with self.subTest(reason=reason), tempfile.TemporaryDirectory() as profile:
                cdp = FakeCdp()
                sink = bridge.CdpSink(cdp.url, Path(profile), 1, 1)
                observed = [bridge.digest(bridge.normalize_message(text)) for text in texts]
                with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
                    sink, "document_ready_state", return_value="complete"
                ), patch.object(sink, "user_message_digests", return_value=observed), patch.object(
                    sink, "trusted_turn_anchor_state", return_value=(0, 0)
                ), patch.object(sink, "_user_message_texts", return_value=texts) as reader, patch.object(
                    sink, "response_snapshot", side_effect=AssertionError("scope hint must not read/promote assistant")
                ), self.assertRaisesRegex(bridge.Attention, reason) as error:
                    adapter.bounded_response_observation(
                        bridge, sink, cdp, message, bridge.digest(bridge.normalize_message(message)),
                        clock=lambda: 0, sleep=lambda _: self.fail("mismatch must not wait"))
                self.assertEqual(reader.call_count, 1)
                self.assertNotIn("PRIVATE_HANDOFF", str(error.exception))
                self.assertFalse(cdp.sent)
                self.assertEqual(cdp.keys, [])

    def test_scope_hint_refuses_changed_view_drift_and_private_query_errors(self):
        message = "exact header\n\nPRIVATE_HANDOFF_EXPECTED"
        with tempfile.TemporaryDirectory() as profile:
            cdp = FakeCdp()
            sink = bridge.CdpSink(cdp.url, Path(profile), 1, 1)
            texts = ["exact header changed body"]
            with patch.object(sink, "_user_message_texts", return_value=texts):
                self.assertEqual(sink.missing_user_reason(cdp, message, ["0" * 64]), "RESPONSE_USER_VIEW_CHANGED")
            with patch.object(sink, "_user_message_texts", side_effect=RuntimeError("PRIVATE_HANDOFF error")):
                self.assertEqual(sink.missing_user_reason(cdp, message, []), "RESPONSE_SEQUENCE_QUERY_FAILED")
            def drift(_cdp):
                cdp.url = "https://chatgpt.com/c/foreign"
                return texts
            with patch.object(sink, "_user_message_texts", side_effect=drift):
                self.assertEqual(sink.missing_user_reason(cdp, message, []), "RESPONSE_TARGET_DRIFT")
            cdp.url = sink.url
            with patch.object(cdp, "get_current_url", side_effect=RuntimeError("PRIVATE_TARGET")):
                self.assertEqual(sink.missing_user_reason(cdp, message, []), "RESPONSE_TARGET_QUERY_FAILED")
            with patch.object(sink, "_user_message_texts", side_effect=AssertionError("single-line hint must not query")):
                self.assertEqual(sink.missing_user_reason(cdp, "one line", []), "RESPONSE_EXPECTED_USER_NOT_FOUND")

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

    def run_attempt(self, acknowledgement, *, cdp_type=FakeCdp, sink_type=FakeSink, cooperative=False, message="bounded wake"):
        cdp = cdp_type()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        target = {"url": cdp.url, "digest": bridge.digest(cdp.url), "generation": 1}
        event = {"eventId": "review-1", "message": message, "targetGeneration": 1}
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
            self.assertEqual(cdp.draft, "")
            self.assertEqual(cdp.keys, [])
            return acknowledgement(cdp)
        with patch.object(bridge, "CdpSink", sink_type), patch.object(adapter, "read_request", ack), patch.object(adapter, "emit", emitted.append):
            try: adapter.attempt(bridge, sb, event, target, cooperative=cooperative)
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

    def test_cooperative_delivery_releases_browser_after_durable_user_receipt(self):
        cdp, emitted, error = self.run_attempt(lambda _cdp: {"submit": True}, cooperative=True)
        self.assertIsNone(error)
        self.assertEqual(cdp.timeline, ["submit", "accepted", "round-trip", "confirm"])
        self.assertEqual([item["stage"] for item in emitted[-3:]],
                         ["SUBMISSION_ACCEPTED", "USER_MESSAGE_APPENDED", "SENT_PENDING_RESPONSE"])
        self.assertNotIn("RESPONSE_COMPLETED", [item["stage"] for item in emitted])
        self.assertEqual(emitted[-1]["receipt"], emitted[-2]["receipt"])

    def test_cooperative_delivery_keeps_uncertainty_when_fresh_receipt_fails(self):
        class UnprovenSink(FakeSink):
            def durable_receipt_round_trip(self, cdp):
                raise bridge.PostSubmitUnknown("SUBMIT_RECEIPT_ROUND_TRIP_FAILED")
        cdp, emitted, error = self.run_attempt(lambda _cdp: {"submit": True},
                                               sink_type=UnprovenSink, cooperative=True)
        self.assertEqual(error, "SUBMIT_RECEIPT_ROUND_TRIP_FAILED")
        self.assertEqual(cdp.timeline, ["submit", "accepted"])
        self.assertNotIn("SENT_PENDING_RESPONSE", [item["stage"] for item in emitted])

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
        self.assertEqual(error, "EXISTING_DRAFT")

    def test_alternate_editor_is_used_after_submission_authorization(self):
        alternate = bridge.EDITOR_SELECTORS[1]
        case = self

        class AlternateEditorCdp(FakeCdp):
            def send_keys(self, selector, text, timeout):
                if selector == bridge.EDITOR:
                    raise RuntimeError("legacy selector is unavailable")
                case.assertEqual(alternate, selector)
                super().send_keys(selector, text, timeout)

        class AlternateEditorSink(FakeSink):
            def editor_selector(self, cdp, *, prevalidated=False):
                case.assertTrue(prevalidated)
                return alternate

        cdp, _emitted, error = self.run_attempt(
            lambda _cdp: {"submit": True},
            cdp_type=AlternateEditorCdp,
            sink_type=AlternateEditorSink,
        )
        self.assertIsNone(error)
        self.assertTrue(cdp.sent)
        self.assertEqual([(alternate, "bounded wake", 30)], cdp.keys)

    def test_multiline_message_never_sends_enter_during_typing(self):
        class EnterSensitiveCdp(FakeCdp):
            def send_keys(self, selector, text, timeout):
                if any(char in text for char in "\r\n\t"):
                    self.sent = True
                    raise AssertionError("CDP text entry submitted before guarded click")
                super().send_keys(selector, text, timeout)

        message = 'Review JSON: {"note":"two  spaces"}\r\nNext\tstep'
        cdp, emitted, error = self.run_attempt(
            lambda _cdp: {"submit": True}, cdp_type=EnterSensitiveCdp, message=message
        )
        self.assertIsNone(error)
        self.assertTrue(cdp.sent)
        self.assertEqual(cdp.keys[0][1], 'Review JSON: {"note":"two  spaces"}  Next step')
        self.assertEqual(
            emitted[-1]["receipt"]["messageDigest"],
            bridge.digest(bridge.normalize_message(message)),
        )

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

    def test_cooperative_reconciliation_proves_existing_receipt_without_response_wait(self):
        cdp = FakeCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        target = {"url": cdp.url, "digest": bridge.digest(cdp.url), "generation": 1}
        event = {"eventId": "review-1", "message": "bounded wake", "targetGeneration": 1}
        emitted = []
        with patch.object(bridge, "CdpSink", FakeSink), patch.object(adapter, "emit", emitted.append):
            adapter.reconcile_submission(bridge, sb, event, target, cooperative=True)
        self.assertEqual(cdp.timeline, ["reconcile"])
        self.assertFalse(cdp.sent)
        self.assertEqual([item["stage"] for item in emitted[-2:]],
                         ["USER_MESSAGE_APPENDED", "SENT_PENDING_RESPONSE"])

    def test_read_only_probes_for_a_then_b_do_not_submit_or_block_one_another(self):
        emitted = []
        with patch.object(bridge, "CdpSink", FakeProbeSink), patch.object(
            adapter, "emit", emitted.append
        ), patch.object(adapter.time, "sleep"), patch.object(
            adapter.time, "monotonic", side_effect=range(100)
        ):
            for event_id, stage in (("project-a", "RESPONSE_PENDING"),
                                    ("project-b", "RESPONSE_COMPLETE")):
                cdp = FakeCdp()
                cdp.probe_stage = stage
                sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
                adapter.observe_response(bridge, sb, self.probe_request(event_id=event_id))
                self.assertFalse(cdp.sent)
                self.assertEqual(cdp.keys, [])
                self.assertTrue(all(item == "observe" for item in cdp.timeline))
                self.assertGreaterEqual(len(cdp.timeline), 2)
        terminal = [item for item in emitted if item.get("stage", "").startswith("RESPONSE_")]
        self.assertEqual([(item["eventId"], item["stage"]) for item in terminal],
                         [("project-a", "RESPONSE_PENDING"), ("project-b", "RESPONSE_COMPLETE")])
        self.assertTrue(all(set(item) == {"stage", "eventId", "targetGeneration",
                                              "targetDigest", "messageDigest"} for item in terminal))

    def test_probe_rejects_wrong_chat_and_mismatched_receipt_before_any_write(self):
        class StuckCdp(FakeCdp):
            def __init__(self):
                super().__init__()
                self.url = "https://chatgpt.com/c/other"
                self.probe_stage = "RESPONSE_COMPLETE"
            def get(self, url): pass
        cdp = StuckCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        with patch.object(bridge, "CdpSink", FakeProbeSink), patch.object(adapter, "emit", emitted.append):
            with self.assertRaisesRegex(bridge.Attention, "RESPONSE_TARGET_DRIFT"):
                adapter.observe_response(bridge, sb, self.probe_request())
            wrong = self.probe_request()
            wrong["receipt"]["messageDigest"] = "0" * 64
            with self.assertRaisesRegex(bridge.Attention, "RESPONSE_PROBE_RECEIPT_INVALID"):
                adapter.observe_response(bridge, sb, wrong)
        self.assertEqual(cdp.keys, [])
        self.assertFalse(cdp.sent)
        self.assertFalse(any(item.get("stage") == "RESPONSE_COMPLETE" for item in emitted))

    def test_probe_rejects_boolean_generation_and_schema_before_browser_open(self):
        class NoBrowser:
            @property
            def cdp(self):
                raise AssertionError("invalid receipt must not open browser")
        for field_path in (("event", "targetGeneration"),
                           ("receipt", "targetGeneration"),
                           ("receipt", "schemaVersion")):
            with self.subTest(field_path=field_path):
                request = self.probe_request()
                request[field_path[0]][field_path[1]] = True
                with self.assertRaisesRegex(bridge.Attention, "RESPONSE_PROBE_RECEIPT_INVALID"):
                    adapter.observe_response(bridge, NoBrowser(), request)

    def test_transient_completed_frame_becomes_pending_on_second_read(self):
        cdp = FakeCdp()
        cdp.probe_stages = ["RESPONSE_COMPLETE", "RESPONSE_PENDING"]
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        with patch.object(bridge, "CdpSink", FakeProbeSink), patch.object(
            adapter, "emit", emitted.append
        ), patch.object(adapter.time, "sleep") as pause:
            adapter.observe_response(bridge, sb, self.probe_request())
        pause.assert_called_once_with(.25)
        self.assertEqual(cdp.timeline, ["observe", "observe"])
        self.assertEqual(emitted[-1]["stage"], "RESPONSE_PENDING")
        self.assertFalse(cdp.sent)

    def test_probe_requires_latest_exact_user_and_usable_completed_assistant(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.CdpSink("https://chatgpt.com/c/exact", Path(profile), 1, 1)
            cdp = FakeCdp()
            expected = bridge.digest("bounded wake")
            with patch.object(bridge.CdpSink, "post_submit_visible_any", return_value=False), patch.object(
                bridge.CdpSink, "document_ready_state", return_value="complete"
            ), patch.object(bridge.CdpSink, "trusted_turn_anchor_state", return_value=(1, 0)), patch.object(
                bridge.CdpSink, "user_message_digests", return_value=[expected, bridge.digest("newer user")]
            ), patch.object(bridge.CdpSink, "editor_state", return_value=("ready", True)), patch.object(
                bridge.CdpSink, "response_snapshot", return_value=(0, 0, True, True)
            ):
                with self.assertRaisesRegex(bridge.Attention, "RESPONSE_SEQUENCE_DRIFT"):
                    sink.observe_cooperative_response(cdp, "bounded wake", expected)
            with patch.object(bridge.CdpSink, "post_submit_visible_any", return_value=False), patch.object(
                bridge.CdpSink, "document_ready_state", return_value="complete"
            ), patch.object(bridge.CdpSink, "trusted_turn_anchor_state", return_value=(1, 0)), patch.object(
                bridge.CdpSink, "user_message_digests", return_value=[expected]
            ), patch.object(bridge.CdpSink, "editor_state", return_value=("ready", True)), patch.object(
                bridge.CdpSink, "response_snapshot", return_value=(0, 0, True, False)
            ) as response:
                self.assertEqual(sink.observe_cooperative_response(cdp, "bounded wake", expected), "RESPONSE_PENDING")
                response.assert_called_once_with(cdp, "bounded wake", require_usable_body=True)
            with patch.object(bridge.CdpSink, "post_submit_visible_any", return_value=False), patch.object(
                bridge.CdpSink, "document_ready_state", return_value="complete"
            ), patch.object(bridge.CdpSink, "trusted_turn_anchor_state", return_value=(1, 0)), patch.object(
                bridge.CdpSink, "user_message_digests", return_value=[expected]
            ), patch.object(bridge.CdpSink, "editor_state", return_value=("ready", True)), patch.object(
                bridge.CdpSink, "response_snapshot", return_value=(0, 0, True, True)
            ):
                self.assertEqual(sink.observe_cooperative_response(cdp, "bounded wake", expected), "RESPONSE_COMPLETE")
            with patch.object(bridge.CdpSink, "post_submit_visible_any", return_value=False), patch.object(
                bridge.CdpSink, "document_ready_state", return_value="complete"
            ), patch.object(bridge.CdpSink, "trusted_turn_anchor_state", return_value=(1, 0)), patch.object(
                bridge.CdpSink, "user_message_digests", return_value=[expected]
            ), patch.object(bridge.CdpSink, "editor_state", return_value=("ready", True)), patch.object(
                bridge.CdpSink, "response_snapshot",
                side_effect=bridge.PostSubmitUnknown("RESPONSE_COMPLETION_UNPROVEN")
            ):
                with self.assertRaisesRegex(bridge.Attention, "RESPONSE_STATE_UNPROVEN"):
                    sink.observe_cooperative_response(cdp, "bounded wake", expected)

    def test_controls_only_assistant_is_not_usable_or_complete(self):
        node_exe = shutil.which("node")
        if node_exe is None:
            self.skipTest("Node.js is unavailable for the isolated DOM fixture")
        harness = r"""
            const fs = require('fs');
            const withBody = __WITH_BODY__;
            const withAssistantAction = __WITH_ASSISTANT_ACTION__;
            const body = {
                textContent: 'A useful answer',
                contains: () => false,
                getClientRects: () => [1],
                cloneNode: () => ({textContent: 'A useful answer', querySelectorAll: () => []})
            };
            let turn;
            const assistant = {
                innerText: withBody ? 'A useful answer Copy Regenerate' : 'Copy Regenerate',
                textContent: withBody ? 'A useful answer Copy Regenerate' : 'Copy Regenerate',
                parentElement: null,
                getClientRects: () => [1],
                getAttribute: name => name === 'data-message-author-role' ? 'assistant' : null,
                matches: () => false,
                closest: selector => selector === '[data-content-search-turn-key]' ? turn : null,
                querySelectorAll: selector => selector === "button[data-testid='copy-turn-action-button']" &&
                    withAssistantAction ? [{}] : [],
                querySelector: selector => selector.includes('data-message-content') && withBody ? body :
                    selector === '[data-chatgpt-selection-message-id]' && withBody ?
                        {getAttribute: () => null} : null
            };
            turn = {
                innerText: 'exact wake message ' + assistant.innerText,
                textContent: 'exact wake message ' + assistant.textContent,
                parentElement: null,
                getClientRects: () => [1],
                getAttribute: name => name === 'data-content-search-turn-key' ? 'turn-1' : null,
                matches: selector => selector === '[data-content-search-turn-key]',
                closest: selector => selector === '[data-content-search-turn-key]' ? turn : null,
                contains: element => element === assistant,
                compareDocumentPosition: element => element === assistant ? 4 : 0,
                querySelectorAll: selector => selector === '[data-content-search-unit-key]' ? [assistant] : [],
                querySelector: selector => selector === "button[data-testid='copy-turn-action-button']" ? {} : null
            };
            assistant.parentElement = turn;
            global.Node = {DOCUMENT_POSITION_FOLLOWING: 4};
            global.window = {getComputedStyle: () => ({visibility: 'visible', display: 'block'})};
            global.document = {querySelectorAll: selector => {
                if (selector === 'div, span, p') return [];
                if (selector.includes('[data-content-search-turn-key]') &&
                    selector.includes('[data-user-message-bubble]')) return [turn, assistant];
                if (selector === "[data-message-author-role='assistant'], article[data-turn='assistant']")
                    return [assistant];
                if (selector === "[data-message-author-role='user'], [data-message-author-role='assistant']")
                    return [assistant];
                return [];
            }};
            process.stdout.write(JSON.stringify(eval(fs.readFileSync(0, 'utf8'))));
        """
        class NodeCdp:
            def __init__(self, with_body, with_assistant_action=False):
                self.with_body = with_body
                self.with_assistant_action = with_assistant_action
            def evaluate(self, script):
                result = subprocess.run(
                    [node_exe, "-e", harness.replace("__WITH_BODY__", str(self.with_body).lower())
                     .replace("__WITH_ASSISTANT_ACTION__", str(self.with_assistant_action).lower())],
                    input=script, text=True, capture_output=True, timeout=10,
                )
                if result.returncode:
                    raise AssertionError(result.stderr)
                return json.loads(result.stdout)

        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink("https://chatgpt.com/c/exact", Path(profile), 1, 1)
            controls_only = NodeCdp(False)
            self.assertEqual(
                sink.response_snapshot(controls_only, "exact wake message", require_usable_body=True),
                (0, 0, True, False),
            )
            snapshot = sink.visible_message_snapshot(controls_only)
            self.assertEqual(snapshot["messages"][-1]["text"], "Copy Regenerate")
            self.assertFalse(snapshot["messages"][-1]["textComplete"])
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_ASSISTANT_UNAVAILABLE"):
                sink.require_usable_latest_assistant(controls_only, source=True)
            usable = NodeCdp(True)
            self.assertEqual(
                sink.response_snapshot(usable, "exact wake message", require_usable_body=True),
                (0, 0, True, False),
            )
            self.assertEqual(
                sink.response_snapshot(usable, "exact wake message", require_usable_body=False),
                (0, 0, True, True),
            )
            self.assertTrue(sink.visible_message_snapshot(usable)["messages"][-1]["textComplete"])
            completed = NodeCdp(True, with_assistant_action=True)
            self.assertEqual(
                sink.response_snapshot(completed, "exact wake message", require_usable_body=True),
                (0, 0, True, True),
            )

    def test_startup_reason_is_fixed_and_non_sensitive(self):
        session_not_created = type("SessionNotCreatedException", (Exception,), {})
        webdriver_error = type("WebDriverException", (Exception,), {})
        self.assertEqual(adapter.startup_reason(PermissionError("secret path")), "BROWSER_PROFILE_ACCESS_DENIED")
        self.assertEqual(adapter.startup_reason(FileNotFoundError("secret path")), "BROWSER_BINARY_OR_DRIVER_MISSING")
        self.assertEqual(adapter.startup_reason(session_not_created("secret browser detail")), "BROWSER_SESSION_NOT_CREATED")
        self.assertEqual(adapter.startup_reason(webdriver_error("secret browser detail")), "BROWSER_DRIVER_START_FAILED")
        self.assertEqual(adapter.startup_reason(RuntimeError("secret browser detail")), "BROWSER_SESSION_UNAVAILABLE")


if __name__ == "__main__": unittest.main()
