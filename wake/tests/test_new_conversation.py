"""Offline CDP doubles for one-shot ChatGPT conversation bootstrap."""

import importlib.util
import io
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


def observed_rows(*rows, nested=0, keyed=0, count=None):
    """Synthetic CDP output; raw DOM keys must never escape the sink."""
    units = [[role, text, len(text.encode("utf-16-le")) // 2 <= 4096,
              min(len(text.encode("utf-16-le")) // 2, 1000000), turn, unit, message]
             for role, text, turn, unit, message in rows]
    return ["AUTHOR_ROLE", len(units) if count is None else count, False, nested, keyed, units]


class StructuralCdp:
    def __init__(self, url=bridge.NEW_CONVERSATION_HOME):
        self.url = url
        self.draft = ""
        self.navigation = []
        self.users = 0
        self.assistants = 0
        self.turns = 0
        self.visible_selectors = set()
        self.snapshot = observed_rows()

    def get_current_url(self):
        return self.url

    def get(self, url):
        self.navigation.append(url)
        self.url = url

    def is_element_visible(self, selector):
        return selector in self.visible_selectors

    def get_attribute(self, selector, attribute, timeout):
        return None

    def get_text(self, selector, timeout):
        return self.draft

    def evaluate(self, script):
        if script == "document.readyState":
            return "complete"
        if "new TextEncoder().encode(text).length" in script:
            return ["ready", self.draft]
        if "const count = selector" in script:
            return [self.users, self.assistants, self.turns]
        if "unique.slice(-16)" in script:
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

    def send_keys(self, selector, text, timeout):
        self.timeline.append("type")
        self.draft = text
        self.typed_message = text
    def press_keys(self, selector, text, timeout):
        raise AssertionError("human-speed press_keys must not type a handoff")

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
        if cdp.scenario == "source_load_error":
            raise bridge.Attention("BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER")

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

    def observe_home_draft(self, cdp, message):
        if cdp.url != bridge.NEW_CONVERSATION_HOME or getattr(cdp, "users", 0):
            raise bridge.Attention("BOOTSTRAP_HOME_HAS_TURNS")
        if not cdp.draft:
            return {"classification": "EMPTY_HOME", "actualDigest": None, "actualBytes": 0}
        normalized = bridge.normalize_message(cdp.draft)
        return {"classification": "EXACT_OWNED" if normalized == bridge.normalize_message(message)
                else "MISMATCH", "actualDigest": bridge.digest(normalized),
                "actualBytes": len(cdp.draft.encode("utf-8"))}

    def wait_home_draft(self, cdp, message):
        return self.observe_home_draft(cdp, message)

    def raw_editor_text(self, cdp):
        return cdp.draft

    def clear_archived_home_draft(self, cdp, message, actual_text):
        self.check_typed_home(cdp, message)
        if cdp.draft != actual_text:
            raise bridge.Attention("BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED")
        cdp.timeline.append("clear")
        cdp.draft = ""
        self.check_empty_home(cdp)

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
        if cdp.scenario == "load_error_after_reload":
            raise bridge.PostSubmitUnknown("BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER")
        if cdp.sent != 1:
            raise bridge.PostSubmitUnknown("BOOTSTRAP_RECEIPT_SEQUENCE_DRIFT")

    def user_message_digests(self, cdp):
        self._check_issued(cdp)
        return [bridge.digest(bridge.normalize_message(cdp.typed_message))]

    def _check_issued(self, cdp):
        if cdp.url != ISSUED:
            raise AssertionError("destination not latched")

    def readiness_reason(self, cdp):
        return None

    def visible_message_snapshot(self, cdp):
        cdp.timeline.append("snapshot")
        coverage = {
            "selectorTier": "AUTHOR_ROLE", "visibleRoleCount": 1, "countOverflow": False,
            "nestedCollapsed": 0, "keyedCollapsed": 0, "omittedBeforeWindow": 0,
            "returned": 1, "identityMissing": 1, "identityUncertain": 0,
            "revisionIncomplete": 0, "textIncomplete": 0,
            "anchorStability": "UNVERIFIED",
        }
        message = {
            "role": "ASSISTANT", "text": "bounded private status", "visibleUnits": 22,
            "textComplete": True, "turnHash": None, "roleTurnHash": None,
            "messageHash": None, "identityBasis": "MISSING",
            "revisionHash": bridge.digest("bounded private status"), "revisionComplete": True,
        }
        if cdp.scenario == "large_snapshot":
            message["text"] = "😀" * 4096
            message["visibleUnits"] = 4096
        if cdp.scenario == "many_snapshot":
            messages = []
            for index in range(12):
                item = dict(message)
                item["text"] = "x" * 800
                item["visibleUnits"] = 800
                item["identityBasis"] = "UNIT_KEY"
                item["messageHash"] = bridge.digest(f"message-{index}")
                messages.append(item)
            coverage.update(visibleRoleCount=12, returned=12, identityMissing=0)
            return {"messages": messages, "truncated": False, "coverage": coverage}
        return {"messages": [message], "truncated": False, "coverage": coverage}


class NewConversationTests(unittest.TestCase):
    def request(self):
        return {
            "bootstrap": True,
            "projectId": "project-one",
            "intentId": "rollover-1",
            "source": {"url": SOURCE, "digest": bridge.digest(SOURCE), "generation": 7},
            "handoff": "Continue the bounded task with the preserved evidence.",
        }

    def run_attempt(self, scenario="success", *, ack=None, request=None, cdp_type=AttemptCdp):
        cdp = cdp_type(scenario)
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        acknowledgements = []

        def read_ack():
            acknowledgements.append(1)
            self.assertEqual(cdp.sent, 0)
            self.assertEqual(cdp.draft, "")
            self.assertNotIn("type", cdp.timeline)
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

    def v2_request(self, *, resume_progress=None):
        request = self.request()
        request.update({"cooperative": True, "bootstrapProtocol": 2,
                        "resumeDraft": resume_progress is not None})
        if resume_progress is not None:
            request["resumeProgress"] = resume_progress
        return request

    def run_v2_attempt(self, *, request=None, draft="", users=0, deny_stage=None):
        request = request or self.v2_request()
        cdp = AttemptCdp()
        cdp.draft = draft
        cdp.users = users
        if draft:
            cdp.typed_message = draft
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        acknowledgements = []
        responses = {
            "TEXT_ENTRY_AUTHORIZED": {"textEntry": True},
            "DRAFT_READY": {"draftReady": True},
            "SEND_ATTEMPTED": {"submit": True},
        }

        def read_ack():
            stage = emitted[-1]["stage"]
            acknowledgements.append(stage)
            self.assertEqual(cdp.sent, 0, "a browser click cannot precede SEND_ATTEMPTED ACK")
            if stage == "TEXT_ENTRY_AUTHORIZED":
                self.assertNotIn("type", cdp.timeline)
            if stage in {"DRAFT_READY", "SEND_ATTEMPTED"}:
                self.assertEqual(
                    bridge.normalize_message(cdp.draft),
                    bridge.normalize_message(bridge.compose_bootstrap_message(
                        request["projectId"], request["intentId"], SOURCE, request["handoff"])),
                )
            return {} if stage == deny_stage else responses[stage]

        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "read_request", read_ack
        ), patch.object(adapter, "emit", emitted.append):
            try:
                adapter.bootstrap_attempt(bridge, sb, request)
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

    def test_real_sink_classifies_exact_home_draft_without_exposing_text(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp()
            expected = bridge.compose_bootstrap_message(
                "project-one", "rollover-1", SOURCE, "Keep the exact handoff.")
            self.assertEqual(sink.observe_home_draft(cdp, expected)["classification"], "EMPTY_HOME")
            cdp.draft = bridge.keyboard_safe_message(expected)
            exact = sink.observe_home_draft(cdp, expected)
            self.assertEqual(exact["classification"], "EXACT_OWNED")
            self.assertEqual(exact["actualDigest"], bridge.digest(bridge.normalize_message(expected)))
            self.assertNotIn("text", exact)
            cdp.draft = "Project: someone-else"
            self.assertEqual(sink.observe_home_draft(cdp, expected)["classification"], "MISMATCH")
            cdp.users = 1
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_HOME_HAS_TURNS"):
                sink.observe_home_draft(cdp, expected)
            cdp.users = 0
            cdp.visible_selectors.add(bridge.LOGIN[0])
            with self.assertRaisesRegex(bridge.Attention, "LOGIN_OR_PROFILE_REQUIRED"):
                sink.observe_home_draft(cdp, expected)

    def test_real_sink_archived_clear_reloads_home_without_submit(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            expected = bridge.compose_bootstrap_message(
                "project-one", "rollover-1", SOURCE, "Preserve exact journal text.")

            class ClearCdp(StructuralCdp):
                def evaluate(self, script):
                    if "document.execCommand('delete'" in script:
                        self.assert_safe_script = script
                        if self.draft != expected:
                            return "UNAVAILABLE"
                        self.draft = ""
                        return "CLEARED"
                    return super().evaluate(script)

            cdp = ClearCdp()
            cdp.draft = "  " + expected + "  "
            self.assertEqual(sink.raw_editor_text(cdp), "  " + expected + "  ")
            cdp.draft = expected
            self.assertEqual(sink.raw_editor_text(cdp), expected)
            with patch.object(sink, "wait_home_draft", side_effect=lambda cdp, message:
                              sink.observe_home_draft(cdp, message)):
                sink.clear_archived_home_draft(cdp, expected, expected)
            self.assertEqual(cdp.draft, "")
            self.assertEqual(cdp.navigation, [bridge.NEW_CONVERSATION_HOME])
            self.assertNotIn("send", cdp.assert_safe_script.lower())
            self.assertIn("location.href !==", cdp.assert_safe_script)
            self.assertIn("current !==", cdp.assert_safe_script)
            self.assertIn(json.dumps(expected), cdp.assert_safe_script)

            cdp.draft = expected
            original_check = sink.check_typed_home
            def drift_after_python_check(cdp, message):
                original_check(cdp, message)
                cdp.draft = "foreign late edit"
            with patch.object(sink, "check_typed_home", side_effect=drift_after_python_check):
                with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED"):
                    sink.clear_archived_home_draft(cdp, expected, expected)
            self.assertEqual(cdp.draft, "foreign late edit")

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

    def test_load_error_placeholder_is_neither_usable_source_nor_fresh_answer(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = observed_rows(("user", "handoff", None, None, None),
                                         ("assistant", "This response couldn’t load", None, None, None))
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER"):
                sink.require_usable_latest_assistant(cdp, source=True)
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER"):
                sink.require_usable_latest_assistant(cdp, source=False)
            cdp.snapshot = observed_rows(("user", "handoff", None, None, None),
                                         ("assistant", "Usable code and next step.", None, None, None))
            sink.require_usable_latest_assistant(cdp, source=True)
            sink.require_usable_latest_assistant(cdp, source=False)
            cdp.snapshot = observed_rows(("user", "handoff", None, None, None))
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_ASSISTANT_UNAVAILABLE"):
                sink.require_usable_latest_assistant(cdp, source=True)

    def test_actual_source_and_fresh_receipt_gates_reject_placeholder(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            message = "Exact bootstrap handoff"
            cdp.snapshot = observed_rows(("user", message, None, None, None),
                                         ("assistant", "This response couldn’t load", None, None, None))
            with patch.object(bridge.CdpSink, "pre_typing_ready", return_value=None):
                with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER"):
                    sink.pre_typing_ready(cdp)
            with patch.object(sink, "user_message_digests", return_value=[bridge.digest(bridge.normalize_message(message))]), patch.object(
                sink, "visible_role_counts", return_value=(1, 1, 1)
            ):
                with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER"):
                    sink.prove_single_bootstrap_user(cdp, message)

    def test_response_snapshot_rejects_load_error_even_if_copy_action_exists(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = types.SimpleNamespace(evaluate=lambda script: ["load-error", 0, 0, True, False])
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "RESPONSE_COMPLETION_UNPROVEN"):
                sink.response_snapshot(cdp, "expected user turn")

    def test_private_source_snapshot_bounds_text_without_browser_write(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = observed_rows(("user", "old", None, None, None),
                                         ("assistant", "😀" * 4096, None, None, None))
            snapshot = sink.visible_message_snapshot(cdp)
            self.assertTrue(snapshot["truncated"])
            self.assertEqual((snapshot["messages"][0]["role"], snapshot["messages"][0]["text"]),
                             ("USER", "old"))
            self.assertEqual(len(snapshot["messages"][1]["text"].encode("utf-8")), 4096)
            self.assertLessEqual(
                sum(len(item["text"].encode("utf-8")) for item in snapshot["messages"]),
                8192,
            )

    def test_snapshot_identity_stays_stable_while_complete_body_revision_changes(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            raw_turn = "private-turn-key"
            raw_unit = "private-unit-key"
            cdp.snapshot = observed_rows(("assistant", "Working", raw_turn, raw_unit, None))
            first = sink.visible_message_snapshot(cdp)
            cdp.snapshot = observed_rows(("assistant", "Working step two", raw_turn, raw_unit, None))
            second = sink.visible_message_snapshot(cdp)
            prior, current = first["messages"][0], second["messages"][0]
            for field in ("turnHash", "roleTurnHash", "messageHash"):
                self.assertEqual(prior[field], current[field])
                self.assertEqual(len(prior[field]), 64)
            self.assertNotEqual(prior["revisionHash"], current["revisionHash"])
            self.assertEqual(current["identityBasis"], "UNIT_KEY")
            self.assertEqual(second["coverage"]["anchorStability"], "UNVERIFIED")
            self.assertNotIn(raw_turn, json.dumps(second))
            self.assertNotIn(raw_unit, json.dumps(second))
            other_source = bridge.NewConversationSink(ISSUED, Path(profile), 1, 1)
            self.assertNotEqual(other_source.visible_message_snapshot(cdp)["messages"][0]["messageHash"],
                                current["messageHash"])

    def test_snapshot_reports_uncertain_or_missing_identity_and_incomplete_revision(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = observed_rows(
                ("user", "one", "turn-only-key", None, None),
                ("assistant", "tail" * 1024, None, None, None),
                nested=2, keyed=1, count=20,
            )
            cdp.snapshot[5][1][2] = False
            cdp.snapshot[5][1][3] = 9000
            result = sink.visible_message_snapshot(cdp)
            uncertain, missing = result["messages"]
            self.assertEqual(uncertain["identityBasis"], "ROLE_TURN_ONLY")
            self.assertIsNotNone(uncertain["roleTurnHash"])
            self.assertIsNone(uncertain["messageHash"])
            self.assertEqual(missing["identityBasis"], "MISSING")
            self.assertIsNone(missing["turnHash"])
            self.assertIsNone(missing["revisionHash"])
            self.assertFalse(missing["revisionComplete"])
            self.assertEqual(missing["visibleUnits"], 9000)
            self.assertTrue(result["truncated"])
            self.assertEqual(result["coverage"]["omittedBeforeWindow"], 18)
            self.assertEqual(result["coverage"]["nestedCollapsed"], 2)
            self.assertEqual(result["coverage"]["keyedCollapsed"], 1)
            self.assertEqual(result["coverage"]["identityMissing"], 1)
            self.assertEqual(result["coverage"]["identityUncertain"], 1)
            self.assertEqual(result["coverage"]["revisionIncomplete"], 1)

    def test_snapshot_message_id_can_identify_unit_without_turn_key(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = observed_rows(("assistant", "answer", None, None, "private-message-id"))
            item = sink.visible_message_snapshot(cdp)["messages"][0]
            self.assertEqual(item["identityBasis"], "MESSAGE_ID")
            self.assertIsNotNone(item["messageHash"])
            self.assertIsNone(item["turnHash"])

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
        self.assertFalse(emitted[-1]["snapshot"]["messages"][0]["textComplete"])
        self.assertEqual(emitted[-1]["snapshot"]["coverage"]["textIncomplete"], 1)
        self.assertLessEqual(
            len(json.dumps(emitted[-1], separators=(",", ":")).encode("utf-8")) + 1,
            8192,
        )

    def test_snapshot_frame_trim_reports_omitted_older_units(self):
        cdp = AttemptCdp("many_snapshot")
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        request = {"snapshot": True, "projectId": "project-one", "source": self.request()["source"]}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ):
            adapter.source_snapshot(bridge, sb, request)
        snapshot = emitted[-1]["snapshot"]
        self.assertTrue(snapshot["truncated"])
        self.assertLess(len(snapshot["messages"]), 12)
        self.assertEqual(snapshot["coverage"]["returned"], len(snapshot["messages"]))
        self.assertEqual(snapshot["coverage"]["omittedBeforeWindow"], 12 - len(snapshot["messages"]))
        self.assertEqual(snapshot["coverage"]["identityMissing"], 0)
        self.assertLessEqual(len(json.dumps(emitted[-1], separators=(",", ":")).encode("utf-8")) + 1, 8192)

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

    def test_source_load_error_never_navigates_to_new_chat(self):
        cdp, emitted, acknowledgements, error = self.run_attempt("source_load_error")
        self.assertEqual(error, "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER")
        self.assertEqual((cdp.navigation, cdp.sent, acknowledgements), ([SOURCE], 0, []))
        self.assertNotIn("BOOTSTRAP_HOME_READY", [item["stage"] for item in emitted])

    def test_parent_denial_never_submits(self):
        cdp, emitted, acknowledgements, error = self.run_attempt(ack={"submit": False})
        self.assertEqual(error, "SUBMISSION_NOT_AUTHORIZED")
        self.assertEqual((cdp.sent, len(acknowledgements)), (0, 1))
        self.assertEqual(cdp.timeline, [])
        self.assertEqual([x["stage"] for x in emitted].count("BEFORE_BOOTSTRAP_SUBMIT"), 1)

    def test_v2_persists_three_checkpoints_before_text_or_click(self):
        cdp, emitted, acks, error = self.run_v2_attempt()
        self.assertIsNone(error)
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"])
        self.assertEqual((cdp.timeline.count("type"), cdp.timeline.count("submit")), (1, 1))
        self.assertEqual(cdp.sent, 1)
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_SUBMITTED")
        for stage in acks:
            frame = next(item for item in emitted if item["stage"] == stage)
            self.assertEqual(set(frame), {"stage", "projectId", "intentId",
                                          "sourceGeneration", "messageDigest"})
            self.assertEqual(frame["messageDigest"], emitted[-1]["receipt"]["messageDigest"])

    def test_v2_denials_never_click_before_final_submit_ack(self):
        for denied, expected_types in (("TEXT_ENTRY_AUTHORIZED", 0), ("DRAFT_READY", 1),
                                       ("SEND_ATTEMPTED", 1)):
            with self.subTest(denied=denied):
                cdp, emitted, acks, error = self.run_v2_attempt(deny_stage=denied)
                self.assertIsNotNone(error) if denied == "TEXT_ENTRY_AUTHORIZED" else self.assertIsNone(error)
                self.assertEqual(cdp.timeline.count("type"), expected_types)
                self.assertEqual(cdp.sent, 0)
                self.assertEqual(acks[-1], denied)
                self.assertNotIn("BOOTSTRAP_SUBMITTED", [item["stage"] for item in emitted])

    def test_v2_pre_send_resume_reuses_exact_draft_or_safe_empty_home(self):
        request = self.v2_request(resume_progress="TEXT_ENTRY_AUTHORIZED")
        expected = bridge.compose_bootstrap_message(
            request["projectId"], request["intentId"], SOURCE, request["handoff"])
        full = bridge.keyboard_safe_message(expected)
        cdp, emitted, acks, error = self.run_v2_attempt(request=request, draft=full)
        self.assertIsNone(error)
        self.assertEqual(cdp.navigation, [bridge.NEW_CONVERSATION_HOME])
        self.assertEqual((cdp.timeline.count("type"), cdp.sent), (0, 1))
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"])
        cdp, emitted, acks, error = self.run_v2_attempt(request=request)
        self.assertEqual(error, "BOOTSTRAP_DRAFT_MISSING")
        self.assertEqual((cdp.timeline.count("type"), cdp.sent, acks), (0, 0, []))
        ready = self.v2_request(resume_progress="DRAFT_READY")
        cdp, emitted, acks, error = self.run_v2_attempt(request=ready, draft=full)
        self.assertIsNone(error)
        self.assertEqual((cdp.timeline.count("type"), cdp.sent), (0, 1))
        for draft, users, expected_error in (("", 0, "BOOTSTRAP_DRAFT_MISSING"),
                                             ("partial handoff", 0, "BOOTSTRAP_DRAFT_MISMATCH"),
                                             (full, 1, "BOOTSTRAP_HOME_HAS_TURNS")):
            with self.subTest(draft=draft[:12], users=users):
                cdp, emitted, acks, error = self.run_v2_attempt(
                    request=ready, draft=draft, users=users)
                self.assertEqual(error, expected_error)
                self.assertEqual((cdp.sent, acks), (0, []))
        cdp, emitted, acks, error = self.run_v2_attempt(
            request=request, draft="partial handoff")
        self.assertEqual(error, "BOOTSTRAP_DRAFT_MISMATCH")
        self.assertEqual((cdp.sent, acks), (0, []))

    def test_ready_draft_survives_send_control_failure_and_resumes_without_retyping(self):
        with patch.object(AttemptSink, "wait_for_send_control", return_value=None):
            cdp, emitted, acks, error = self.run_v2_attempt()
        self.assertIsNone(error)
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY"])
        self.assertEqual(cdp.sent, 0)
        self.assertEqual(emitted[-1], {"stage":"BOOTSTRAP_UNKNOWN",
                                      "reason":"BOOTSTRAP_SEND_CONTROL_MISSING"})
        saved_draft = cdp.draft
        request = self.v2_request(resume_progress="DRAFT_READY")
        resumed, emitted, acks, error = self.run_v2_attempt(request=request, draft=saved_draft)
        self.assertIsNone(error)
        self.assertEqual((resumed.timeline.count("type"), resumed.sent), (0, 1))
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_SUBMITTED")

    def test_large_inspector_request_uses_bootstrap_bound_without_exposing_text(self):
        request = {"bootstrapDraftInspect": True, "bootstrapProtocol": 2,
                   "projectId": "project-one", "intentId": "rollover-1",
                   "source": self.request()["source"], "handoff": "x" * 9000}
        wire = (json.dumps(request, separators=(",", ":")) + "\n").encode()
        self.assertGreater(len(wire), 8192)
        self.assertLess(len(wire), adapter.MAX_BOOTSTRAP_REQUEST_BYTES)
        with patch.object(adapter.sys, "stdin", types.SimpleNamespace(buffer=io.BytesIO(wire))):
            self.assertEqual(adapter.read_job_request(), request)
        wire = (json.dumps({"unrelated": "x" * 9000}) + "\n").encode()
        with patch.object(adapter.sys, "stdin", types.SimpleNamespace(buffer=io.BytesIO(wire))):
            with self.assertRaisesRegex(ValueError, "invalid job request"):
                adapter.read_job_request()

    def test_v2_rejects_malformed_protocol_and_resume_stage_before_navigation(self):
        for mutation in ({"bootstrapProtocol": 2.0}, {"resumeDraft": "yes"},
                         {"resumeDraft": True},
                         {"resumeDraft": True, "resumeProgress": "SEND_ATTEMPTED"}):
            with self.subTest(mutation=mutation):
                request = self.v2_request()
                request.update(mutation)
                cdp, emitted, acks, error = self.run_v2_attempt(request=request)
                self.assertEqual(error, "BOOTSTRAP_REQUEST_INVALID")
                self.assertEqual((cdp.navigation, cdp.sent, emitted, acks), ([], 0, [], []))

    def test_v2_ambiguous_text_entry_never_emits_send_authorization(self):
        class AmbiguousCdp(AttemptCdp):
            def send_keys(self, selector, text, timeout):
                self.timeline.append("type")
                self.url = ISSUED
                raise RuntimeError("ambiguous browser input")

        request = self.v2_request()
        cdp = AmbiguousCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        acks = []
        def read_ack():
            acks.append(emitted[-1]["stage"])
            return {"textEntry": True}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ), patch.object(adapter, "read_request", read_ack):
            adapter.bootstrap_attempt(bridge, sb, request)
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED"])
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_UNKNOWN")
        self.assertEqual(emitted[-1]["reason"], "BOOTSTRAP_PRE_SEND_RUNTIMEERROR")
        self.assertNotIn("SEND_ATTEMPTED", [item["stage"] for item in emitted])

    def test_v2_draft_inspector_is_observe_only_and_bounded(self):
        request = self.v2_request()
        inspect = {"bootstrapDraftInspect": True, "bootstrapProtocol": 2,
                   "projectId": request["projectId"], "intentId": request["intentId"],
                   "source": request["source"], "handoff": request["handoff"]}
        expected = bridge.compose_bootstrap_message(
            inspect["projectId"], inspect["intentId"], SOURCE, inspect["handoff"])
        for draft, users, classification in (("", 0, "EMPTY_HOME"),
                                             (bridge.keyboard_safe_message(expected), 0, "EXACT_OWNED"),
                                             ("unrelated private draft", 0, "MISMATCH"),
                                             ("", 1, "UNAVAILABLE")):
            with self.subTest(classification=classification):
                cdp = AttemptCdp()
                cdp.draft = draft
                cdp.users = users
                sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
                emitted = []
                with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
                    adapter, "emit", emitted.append
                ):
                    adapter.bootstrap_draft_inspect(bridge, sb, inspect)
                frame = emitted[-1]
                self.assertEqual(frame["stage"], "BOOTSTRAP_DRAFT_INSPECTED")
                self.assertEqual(frame["classification"], classification)
                self.assertEqual(frame["sourceDigest"], bridge.digest(SOURCE))
                self.assertEqual(frame["expectedDigest"], bridge.digest(bridge.normalize_message(expected)))
                self.assertLessEqual(frame["actualBytes"], bridge.MAX_USER_MESSAGE_CHARS)
                self.assertEqual((cdp.timeline, cdp.sent, cdp.navigation),
                                 ([], 0, [bridge.NEW_CONVERSATION_HOME]))
                self.assertNotIn("unrelated private draft", json.dumps(frame))

    def test_archive_requires_exact_owned_draft_and_durable_ack_before_clear(self):
        request = self.v2_request()
        archive = {"bootstrapDraftArchive": True, "bootstrapProtocol": 2,
                   "archiveRecorded": False, "projectId": request["projectId"],
                   "intentId": request["intentId"], "source": request["source"],
                   "handoff": request["handoff"]}
        expected = bridge.compose_bootstrap_message(
            archive["projectId"], archive["intentId"], SOURCE, archive["handoff"])

        def run(draft, *, recorded=False, ack=True, after_ready=None):
            cdp = AttemptCdp()
            cdp.draft = draft
            sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
            emitted = []
            read_count = []
            scoped = dict(archive, archiveRecorded=recorded)
            def read_ack():
                read_count.append(True)
                self.assertEqual(cdp.timeline, [], "clear must follow durable archive ACK")
                self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_DRAFT_ARCHIVE_READY")
                if after_ready:
                    after_ready(cdp)
                return {"archiveDraft": True} if ack else {}
            with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
                adapter, "read_request", read_ack
            ), patch.object(adapter, "emit", emitted.append):
                try:
                    adapter.bootstrap_draft_archive(bridge, sb, scoped)
                    error = None
                except bridge.Attention as failure:
                    error = str(failure)
            return cdp, emitted, len(read_count), error

        cdp, emitted, reads, error = run(expected)
        self.assertIsNone(error)
        self.assertEqual((reads, cdp.timeline, cdp.draft), (1, ["clear"], ""))
        self.assertEqual([item["stage"] for item in emitted if item["stage"].startswith("BOOTSTRAP_DRAFT_")],
                         ["BOOTSTRAP_DRAFT_ARCHIVE_READY", "BOOTSTRAP_DRAFT_ARCHIVED"])
        self.assertEqual(emitted[-2]["actualText"], expected)
        self.assertEqual(emitted[-2]["actualBytes"], len(expected.encode("utf-8")))
        self.assertNotIn("actualText", emitted[-1])
        for draft, recorded, reason in (("", False, "BOOTSTRAP_DRAFT_MISSING"),
                                        ("unrelated", False, "BOOTSTRAP_DRAFT_MISMATCH"),
                                        ("partial", True, "BOOTSTRAP_DRAFT_MISMATCH")):
            cdp, emitted, reads, error = run(draft, recorded=recorded)
            self.assertEqual((error, reads, cdp.timeline), (reason, 0, []))
        cdp, emitted, reads, error = run("", recorded=True)
        self.assertIsNone(error)
        self.assertEqual((reads, cdp.timeline, emitted[-1]["actualBytes"]), (0, [], 0))
        cdp, emitted, reads, error = run(expected, ack=False)
        self.assertEqual((error, reads, cdp.timeline), ("BOOTSTRAP_DRAFT_ARCHIVE_NOT_AUTHORIZED", 1, []))
        cdp, emitted, reads, error = run(expected, after_ready=lambda cdp: setattr(cdp, "draft", "other"))
        self.assertEqual((error, reads, cdp.timeline), ("BOOTSTRAP_DRAFT_DRIFT", 1, []))

    def test_archive_refuses_oversized_private_ready_frame_before_ack(self):
        request = self.request()
        request["handoff"] = "é" * 3500
        archive = {"bootstrapDraftArchive": True, "bootstrapProtocol": 2,
                   "archiveRecorded": False, "projectId": request["projectId"],
                   "intentId": request["intentId"], "source": request["source"],
                   "handoff": request["handoff"]}
        draft = bridge.compose_bootstrap_message(
            archive["projectId"], archive["intentId"], SOURCE, archive["handoff"])
        cdp = AttemptCdp()
        cdp.draft = draft
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ), patch.object(adapter, "read_request", side_effect=AssertionError("no ACK expected")):
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_ARCHIVE_FRAME_TOO_LARGE"):
                adapter.bootstrap_draft_archive(bridge, sb, archive)
        self.assertEqual((cdp.timeline, cdp.sent), ([], 0))
        self.assertNotIn("BOOTSTRAP_DRAFT_ARCHIVE_READY", [item["stage"] for item in emitted])

    def test_multiline_handoff_is_one_keyboard_safe_user_turn_after_ack(self):
        class EnterSensitiveCdp(AttemptCdp):
            def send_keys(self, selector, text, timeout):
                if any(char in text for char in "\r\n\t"):
                    self.sent += 1
                    self.url = ISSUED
                    raise AssertionError("CDP text entry submitted before guarded click")
                super().send_keys(selector, text, timeout)

        request = self.request()
        request["handoff"] = 'Keep JSON {"note":"two  spaces"}\r\nThen\tcontinue.'
        cdp, emitted, acknowledgements, error = self.run_attempt(
            request=request, cdp_type=EnterSensitiveCdp
        )
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
        self.assertEqual(cdp.timeline.count("type"), 1)
        self.assertEqual(cdp.timeline.count("submit"), 1)
        self.assertNotIn("\n", cdp.typed_message)
        self.assertNotIn("\r", cdp.typed_message)
        self.assertNotIn("\t", cdp.typed_message)
        self.assertIn('"note":"two  spaces"', cdp.typed_message)
        expected = bridge.compose_bootstrap_message("project-one", "rollover-1", SOURCE, request["handoff"])
        self.assertEqual(bridge.normalize_message(cdp.typed_message), bridge.normalize_message(expected))
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_COMPLETE")

    def test_uncertain_typing_after_ack_is_not_pre_submit_attention(self):
        class AmbiguousCdp(AttemptCdp):
            def send_keys(self, selector, text, timeout):
                self.sent += 1
                self.url = ISSUED
                raise RuntimeError("ambiguous CDP keyboard outcome")

        cdp, emitted, acknowledgements, error = self.run_attempt(cdp_type=AmbiguousCdp)
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_UNKNOWN")
        self.assertNotIn("BOOTSTRAP_COMPLETE", [item["stage"] for item in emitted])

    def test_url_drift_and_ambiguous_click_are_single_send_unknown(self):
        for scenario in ("source_revisited", "foreign_url", "click_unknown", "load_error_after_reload"):
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

    def test_cooperative_bootstrap_saves_pending_receipt_without_response_wait(self):
        request = self.request()
        request["cooperative"] = True
        cdp, emitted, acknowledgements, error = self.run_attempt(request=request)
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
        self.assertNotIn("completion", cdp.timeline)
        self.assertEqual(cdp.timeline[-2:], ["fresh-document", "exact-receipt"])
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_SUBMITTED")
        receipt = emitted[-1]["receipt"]
        self.assertEqual(receipt["destinationUrl"], ISSUED)
        self.assertEqual(receipt["destinationDigest"], bridge.digest(ISSUED))
        self.assertEqual(receipt["completion"], "RESPONSE_PENDING")

    def test_pending_bootstrap_probe_is_read_only_and_rejects_wrong_receipt(self):
        request = self.request()
        request["cooperative"] = True
        _original, emitted, _ack, error = self.run_attempt(request=request)
        self.assertIsNone(error)
        receipt = emitted[-1]["receipt"]

        class ProbeSink:
            stages = ["RESPONSE_PENDING", "RESPONSE_COMPLETE", "RESPONSE_COMPLETE"]

            def __init__(self, url, profile, ui_timeout, submit_timeout):
                self.url = url

            def observe_cooperative_response(self, cdp, message, message_digest):
                self.assert_probe(cdp, message, message_digest)
                return self.stages.pop(0)

            @staticmethod
            def assert_probe(cdp, message, message_digest):
                assert cdp.url == ISSUED and cdp.sent == 0
                assert message_digest == bridge.digest(bridge.normalize_message(message))

        def run_probe(probe_receipt):
            cdp = AttemptCdp()
            sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
            output = []
            probe_request = {
                "bootstrapProbe": True,
                "projectId": "project-one",
                "intentId": "rollover-1",
                "source": self.request()["source"],
                "handoff": self.request()["handoff"],
                "receipt": probe_receipt,
            }
            with patch.object(bridge, "CdpSink", ProbeSink), patch.object(
                adapter, "emit", output.append
            ), patch.object(adapter.time, "sleep", return_value=None):
                adapter.bootstrap_probe(bridge, sb, probe_request)
            return cdp, output

        first_cdp, first_output = run_probe(receipt)
        self.assertEqual(first_cdp.sent, 0)
        self.assertEqual(first_output[-1]["stage"], "BOOTSTRAP_RESPONSE_PENDING")
        second_cdp, second_output = run_probe(receipt)
        self.assertEqual(second_cdp.sent, 0)
        self.assertEqual(second_output[-1]["stage"], "BOOTSTRAP_COMPLETE")
        self.assertEqual(second_output[-1]["receipt"]["completion"], "RESPONSE_COMPLETED")
        self.assertEqual(second_output[-1]["receipt"]["destinationUrl"], ISSUED)

        wrong = dict(receipt, destinationDigest="0" * 64)
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_PROBE_RECEIPT_INVALID"):
            run_probe(wrong)
        wrong_source = self.request()["source"].copy()
        wrong_source["generation"] = True
        cdp = AttemptCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_PROBE_SOURCE_INVALID"):
            with patch.object(bridge, "CdpSink", ProbeSink):
                adapter.bootstrap_probe(bridge, sb, {
                    "bootstrapProbe": True, "projectId": "project-one",
                    "intentId": "rollover-1", "source": wrong_source,
                    "handoff": self.request()["handoff"], "receipt": receipt,
                })
        self.assertEqual(cdp.navigation, [])


if __name__ == "__main__":
    unittest.main()
