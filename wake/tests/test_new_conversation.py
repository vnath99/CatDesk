"""Offline CDP doubles for one-shot ChatGPT conversation bootstrap."""

import importlib.util
import json
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
adapter_spec = importlib.util.spec_from_file_location("bootstrap_adapter", ROOT / "wake/adapter.py")
adapter = importlib.util.module_from_spec(adapter_spec)
with patch.object(sys, "argv", [str(ROOT / "wake/adapter.py"), str(ROOT / "wake")]):
    adapter_spec.loader.exec_module(adapter)
bridge_spec = importlib.util.spec_from_file_location("bootstrap_bridge", ROOT / "scripts/wake_bridge.py")
bridge = importlib.util.module_from_spec(bridge_spec)
sys.modules[bridge_spec.name] = bridge
bridge_spec.loader.exec_module(bridge)
REAL_NEW_SINK = bridge.NewConversationSink

SOURCE = "https://chatgpt.com/c/source-123"
ISSUED = "https://chatgpt.com/c/issued-456"


class StructuralCdp:
    def __init__(self, url=bridge.NEW_CONVERSATION_HOME):
        self.url = url
        self.draft = ""
        self.users = 0
        self.assistants = 0
        self.turns = 0
        self.visible_selectors = set()
        self.snapshot = [False, []]

    def get_current_url(self):
        return self.url

    def is_element_visible(self, selector):
        return selector in self.visible_selectors

    def evaluate(self, script):
        if script == "document.readyState":
            return "complete"
        if "const count = selector" in script:
            return [self.users, self.assistants, self.turns]
        if "nodes.slice(-16)" in script:
            return self.snapshot
        if "if (visible.length !== 1) return ['count'" in script:
            return ["ready", not self.draft]
        raise AssertionError("unexpected CDP query")


class AttemptCdp:
    def __init__(self, scenario="success"):
        self.url = SOURCE
        self.scenario = scenario
        self.draft = ""
        self.sent = 0
        self.navigation = []
        self.timeline = []

    def get_current_url(self):
        return self.url

    def get(self, url):
        self.navigation.append(url)
        self.url = url

    def press_keys(self, selector, text, timeout):
        self.timeline.append("type")
        self.draft = text

    def click(self, selector, timeout):
        self.sent += 1
        self.timeline.append("submit")
        if self.scenario == "click_unknown":
            raise bridge.PostSubmitUnknown("SUBMIT_CLICK_UNKNOWN")
        self.draft = ""
        if self.scenario == "source_revisited":
            self.url = SOURCE
        elif self.scenario == "foreign_url":
            self.url = "https://example.invalid/c/issued-456"
        else:
            self.url = ISSUED


class AttemptSink:
    def __init__(self, source_url, profile, ui_timeout, submit_timeout):
        self.source_url = source_url
        self.url = source_url
        self.destination_url = None
        self.ui_timeout = ui_timeout
        self.submit_timeout = submit_timeout

    def exact(self, url):
        return url == self.url

    def wait_for_page_readiness(self, sb, cdp):
        if cdp.url != SOURCE:
            raise bridge.Attention("BOOTSTRAP_SOURCE_DRIFT")
        return cdp

    def wait_for_idle(self, cdp, timeout):
        pass

    def pre_typing_ready(self, cdp):
        if cdp.url != SOURCE or cdp.draft:
            raise bridge.Attention("BOOTSTRAP_SOURCE_DRIFT")

    def open_empty_home(self, cdp):
        cdp.get(bridge.NEW_CONVERSATION_HOME)
        if cdp.scenario == "wrong_home":
            cdp.url = SOURCE
        elif cdp.scenario == "existing_draft":
            cdp.draft = "operator draft"
        elif cdp.scenario == "existing_messages":
            cdp.users = 1

    def check_empty_home(self, cdp):
        if cdp.url != bridge.NEW_CONVERSATION_HOME:
            raise bridge.Attention("BOOTSTRAP_HOME_DRIFT")
        if cdp.draft:
            raise bridge.Attention("EXISTING_DRAFT")
        if getattr(cdp, "users", 0):
            raise bridge.Attention("BOOTSTRAP_HOME_HAS_TURNS")

    def editor_selector(self, cdp, *, prevalidated=False):
        if not prevalidated:
            raise AssertionError("composer must be prevalidated")
        return bridge.EDITOR

    def check_typed_home(self, cdp, message):
        if cdp.url != bridge.NEW_CONVERSATION_HOME:
            raise bridge.Attention("BOOTSTRAP_HOME_DRIFT")
        if bridge.normalize_message(cdp.draft) != bridge.normalize_message(message):
            raise bridge.Attention("BOOTSTRAP_DRAFT_DRIFT")

    def wait_for_send_control(self, cdp, timeout):
        return "send"

    def smoke_path_submit(self, cdp, selector, timeout, editor_selector):
        cdp.click(selector, timeout)

    def latch_issued_url(self, cdp):
        return REAL_NEW_SINK.latch_issued_url(self, cdp)

    def wait_for_submission_accepted(self, cdp, prior, message_digest, message):
        self._check_issued(cdp)
        cdp.timeline.append("accepted")
        return 99.0

    def wait_for_response_completion(self, cdp, message, *, allow_retry, observer):
        self._check_issued(cdp)
        assert allow_retry is False
        cdp.timeline.append("completion")
        observer("RESPONSE_COMPLETED")

    def durable_receipt_round_trip(self, cdp):
        self._check_issued(cdp)
        cdp.timeline.append("fresh-document")

    def confirm_exact_receipt(self, cdp, prior, message_digest, intent_id, message, *, wall_time):
        self._check_issued(cdp)
        cdp.timeline.append("exact-receipt")
        return bridge.receipt_for(intent_id, message, self.url, wall_time())

    def prove_single_bootstrap_user(self, cdp, message):
        self._check_issued(cdp)
        if cdp.sent != 1:
            raise bridge.PostSubmitUnknown("BOOTSTRAP_RECEIPT_SEQUENCE_DRIFT")

    def _check_issued(self, cdp):
        if cdp.url != ISSUED:
            raise AssertionError("destination not latched")

    def readiness_reason(self, cdp):
        return None

    def visible_message_snapshot(self, cdp):
        cdp.timeline.append("snapshot")
        if cdp.scenario == "large_snapshot":
            return {"messages": [{"role": "ASSISTANT", "text": "😀" * 4096}], "truncated": False}
        return {"messages": [{"role": "ASSISTANT", "text": "bounded private status"}], "truncated": False}


class NewConversationTests(unittest.TestCase):
    def request(self):
        return {
            "bootstrap": True,
            "projectId": "project-one",
            "intentId": "rollover-1",
            "source": {"url": SOURCE, "digest": bridge.digest(SOURCE), "generation": 7},
            "handoff": "Continue the bounded task with the preserved evidence.",
        }

    def run_attempt(self, scenario="success", *, ack=None, request=None):
        cdp = AttemptCdp(scenario)
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        acknowledgements = []

        def read_ack():
            acknowledgements.append(1)
            self.assertEqual(cdp.sent, 0)
            self.assertEqual(emitted[-1]["stage"], "BEFORE_BOOTSTRAP_SUBMIT")
            return {"submit": True} if ack is None else ack

        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "read_request", read_ack
        ), patch.object(adapter, "emit", emitted.append):
            try:
                adapter.bootstrap_attempt(bridge, sb, request or self.request())
                error = None
            except bridge.Attention as failure:
                error = str(failure)
        return cdp, emitted, acknowledgements, error

    def test_deterministic_identity_prefix_and_bound(self):
        message = bridge.compose_bootstrap_message("project-one", "rollover-1", SOURCE, "body\nline")
        self.assertEqual(
            message,
            "CatDesk project rollover bootstrap.\nProject: project-one\n"
            "Intent: rollover-1\nPredecessor: https://chatgpt.com/c/source-123\n\nbody\nline",
        )
        self.assertEqual(
            bridge.digest(bridge.normalize_message(message)),
            bridge.digest(" ".join(message.split())),
        )
        with self.assertRaises(bridge.Attention):
            bridge.compose_bootstrap_message("project-one", "rollover-1", SOURCE, "é" * 9000)

    def test_real_sink_rejects_wrong_home_draft_messages_and_login(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_HOME_DRIFT"):
                sink.check_empty_home(cdp)
            cdp.url = bridge.NEW_CONVERSATION_HOME
            cdp.draft = "operator draft"
            with self.assertRaisesRegex(bridge.Attention, "EXISTING_DRAFT"):
                sink.check_empty_home(cdp)
            cdp.draft = ""
            cdp.users = 1
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_HOME_HAS_TURNS"):
                sink.check_empty_home(cdp)
            cdp.users = 0
            cdp.visible_selectors.add(bridge.LOGIN[0])
            with self.assertRaisesRegex(bridge.Attention, "LOGIN_OR_PROFILE_REQUIRED"):
                sink.check_empty_home(cdp)

    def test_real_sink_latches_only_new_canonical_identity(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_SOURCE_REVISITED"):
                sink.latch_issued_url(cdp)
            cdp.url = ISSUED
            self.assertEqual(sink.latch_issued_url(cdp), ISSUED)
            cdp.url = "https://chatgpt.com/c/other-789"
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_DESTINATION_DRIFT"):
                sink.latch_issued_url(cdp)

    def test_private_source_snapshot_bounds_text_without_browser_write(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = [False, [["user", "old"], ["assistant", "😀" * 4096]]]
            snapshot = sink.visible_message_snapshot(cdp)
            self.assertTrue(snapshot["truncated"])
            self.assertEqual(snapshot["messages"][0], {"role": "USER", "text": "old"})
            self.assertEqual(len(snapshot["messages"][1]["text"].encode("utf-8")), 4096)
            self.assertLessEqual(
                sum(len(item["text"].encode("utf-8")) for item in snapshot["messages"]),
                8192,
            )

    def test_explicit_snapshot_mode_never_types_or_submits(self):
        cdp = AttemptCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        request = {"snapshot": True, "projectId": "project-one", "source": self.request()["source"]}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ):
            adapter.source_snapshot(bridge, sb, request)
        self.assertEqual((cdp.sent, cdp.draft, cdp.navigation), (0, "", [SOURCE]))
        self.assertEqual(cdp.timeline, ["snapshot"])
        self.assertEqual(emitted[-1]["stage"], "SOURCE_SNAPSHOT")
        self.assertEqual(emitted[-1]["sourceGeneration"], 7)

    def test_snapshot_wire_frame_stays_within_parent_reader_bound(self):
        cdp = AttemptCdp("large_snapshot")
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        request = {"snapshot": True, "projectId": "project-one", "source": self.request()["source"]}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ):
            adapter.source_snapshot(bridge, sb, request)
        self.assertEqual(cdp.sent, 0)
        self.assertTrue(emitted[-1]["snapshot"]["truncated"])
        self.assertTrue(emitted[-1]["snapshot"]["messages"][0]["text"])
        self.assertLessEqual(
            len(json.dumps(emitted[-1], separators=(",", ":")).encode("utf-8")) + 1,
            8192,
        )

    def test_wrong_source_fails_before_browser_navigation(self):
        request = self.request()
        request["source"]["digest"] = "0" * 64
        cdp, emitted, acknowledgements, error = self.run_attempt(request=request)
        self.assertEqual(error, "BOOTSTRAP_SOURCE_INVALID")
        self.assertEqual((cdp.navigation, cdp.sent, emitted, acknowledgements), ([], 0, [], []))

    def test_home_draft_and_messages_never_request_submit(self):
        for scenario, reason in (
            ("wrong_home", "BOOTSTRAP_HOME_DRIFT"),
            ("existing_draft", "EXISTING_DRAFT"),
            ("existing_messages", "BOOTSTRAP_HOME_HAS_TURNS"),
        ):
            with self.subTest(scenario=scenario):
                cdp, emitted, acknowledgements, error = self.run_attempt(scenario)
                self.assertEqual(error, reason)
                self.assertEqual(cdp.sent, 0)
                self.assertEqual(acknowledgements, [])
                self.assertNotIn("BEFORE_BOOTSTRAP_SUBMIT", [item["stage"] for item in emitted])

    def test_parent_denial_never_submits(self):
        cdp, emitted, acknowledgements, error = self.run_attempt(ack={"submit": False})
        self.assertEqual(error, "SUBMISSION_NOT_AUTHORIZED")
        self.assertEqual((cdp.sent, len(acknowledgements)), (0, 1))
        self.assertEqual([x["stage"] for x in emitted].count("BEFORE_BOOTSTRAP_SUBMIT"), 1)

    def test_url_drift_and_ambiguous_click_are_single_send_unknown(self):
        for scenario in ("source_revisited", "foreign_url", "click_unknown"):
            with self.subTest(scenario=scenario):
                cdp, emitted, acknowledgements, error = self.run_attempt(scenario)
                self.assertIsNone(error)
                self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
                self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_UNKNOWN")
                self.assertNotIn("BOOTSTRAP_COMPLETE", [item["stage"] for item in emitted])

    def test_success_uses_issued_url_and_fresh_exact_receipt(self):
        cdp, emitted, acknowledgements, error = self.run_attempt()
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
        self.assertEqual(cdp.navigation, [SOURCE, bridge.NEW_CONVERSATION_HOME])
        self.assertEqual(
            cdp.timeline[-5:],
            ["submit", "accepted", "completion", "fresh-document", "exact-receipt"],
        )
        before = next(item for item in emitted if item["stage"] == "BEFORE_BOOTSTRAP_SUBMIT")
        receipt = emitted[-1]["receipt"]
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_COMPLETE")
        self.assertEqual(receipt["destinationUrl"], ISSUED)
        self.assertEqual(receipt["destinationDigest"], bridge.digest(ISSUED))
        self.assertEqual(receipt["messageDigest"], before["messageDigest"])
        self.assertEqual(receipt["sourceGeneration"], 7)
        self.assertEqual(receipt["sentUtc"], 99)
        self.assertEqual(receipt["evidence"], "EXACT_USER_MESSAGE_APPENDED")
        self.assertEqual(receipt["completion"], "RESPONSE_COMPLETED")
        self.assertNotIn("handoff", json.dumps(emitted))


if __name__ == "__main__":
    unittest.main()
