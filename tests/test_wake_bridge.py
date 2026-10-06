import importlib.util
import json
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "wake_bridge", Path(__file__).parents[1] / "scripts" / "wake_bridge.py"
)
wake_bridge = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = wake_bridge
SPEC.loader.exec_module(wake_bridge)


class FakeSink:
    def __init__(self, behavior, calls, target_url):
        self.behavior = behavior
        self.calls = calls
        self.url = target_url

    def wake(self, record_id, message, still_actionable, before_submit):
        self.calls.append(message)
        if self.behavior == "cancel":
            raise wake_bridge.Attention("WAKE_CANCELLED")
        if self.behavior == "attention":
            raise wake_bridge.Attention("LOGIN_OR_PROFILE_REQUIRED")
        if self.behavior == "not-idle":
            raise wake_bridge.Attention("CHATGPT_NOT_IDLE")
        if self.behavior == "typed-unsent":
            raise wake_bridge.Attention("SEND_SELECTOR")
        if self.behavior == "pre-submit-crash":
            raise RuntimeError("fixture pre-submit crash")
        if self.behavior == "malformed-receipt":
            return None
        self.assert_actionable(still_actionable)
        before_submit()
        if self.behavior == "malformed-receipt-after-boundary":
            return None
        if self.behavior == "click-unknown":
            raise wake_bridge.PostSubmitUnknown("SUBMIT_CLICK_UNKNOWN")
        if self.behavior == "post-submit-query-failure":
            raise wake_bridge.PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED")
        if self.behavior == "post-submit-crash":
            raise RuntimeError("fixture post-submit crash")
        if self.behavior == "empty-no-receipt":
            raise wake_bridge.PostSubmitUnknown("SUBMIT_RECEIPT_UNPROVEN")
        return wake_bridge.receipt_for(record_id, message, self.url, 123.0)

    @staticmethod
    def assert_actionable(still_actionable):
        if not still_actionable():
            raise AssertionError("fake sink expected the exact record to remain actionable")


class BusySingleton:
    def __init__(self, _path):
        pass

    def __enter__(self):
        raise wake_bridge.Busy()

    def __exit__(self, *_args):
        return False


class WakeBridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.record_id = "review-one"
        self.calls = []
        self.profile = self.root / ".catdesk" / "wake-bridge" / "profile"
        self.profile.mkdir(parents=True)
        self.config = self.root / ".catdesk" / "wake-bridge" / "config.json"
        self.config.write_text(
            json.dumps(
                {
                    "profile_dir": ".catdesk/wake-bridge/profile",
                    "conversation_url": "https://chatgpt.com/c/review-one",
                }
            ),
            encoding="utf-8",
        )
        self.bridge_path = self.root / "scripts" / "wake_bridge.py"
        self.bridge_path.parent.mkdir(parents=True)
        self.bridge_path.write_text(Path(wake_bridge.__file__).read_text(encoding="utf-8"), encoding="utf-8")
        self.bridge_sha256 = wake_bridge.bridge_sha256(self.bridge_path)
        self.inbox(
            {
                "recordId": self.record_id,
                "unread": True,
                "state": "WAITING_FOR_CHATGPT",
                "nextAction": "chatgpt_decision_required",
            }
        )

    def tearDown(self):
        self.temp.cleanup()

    @property
    def state_path(self):
        return self.root / ".catdesk" / "wake-bridge" / "state.json"

    def inbox(self, record):
        path = self.root / ".catdesk" / "autonomy" / "review-inbox.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps([record]), encoding="utf-8")

    def write_state(self, status, operator_attention=None, valid_receipt=False, delivery_attention=None):
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)
        receipt = wake_bridge.receipt_for(self.record_id, message, "https://chatgpt.com/c/review-one", 123.0) if valid_receipt else None
        state = wake_bridge.State(
            deliveries=[wake_bridge.Delivery(
                self.record_id,
                status,
                1.0,
                browser_sent_at_unix=receipt.browser_sent_at_unix if receipt else None,
                message_sha256=receipt.message_sha256 if receipt else None,
                target_sha256=receipt.target_sha256 if receipt else None,
                receipt_schema_version=receipt.receipt_schema_version if receipt else None,
                attention=delivery_attention,
            )],
            operator_attention=operator_attention,
        )
        wake_bridge.save(self.state_path, wake_bridge.asdict(state))

    def read_state(self):
        return json.loads(self.state_path.read_text(encoding="utf-8"))

    def run_main(self, behavior="success", singleton=None, bridge_path=None, bridge_sha256=None):
        def sink_factory(target_url, *_args, **_kwargs):
            return FakeSink(behavior, self.calls, target_url)

        argv = [
            "wake_bridge.py",
            "--workspace",
            str(self.root),
            "--config",
            str(self.config),
            "--record-id",
            self.record_id,
            "--bridge-sha256",
            bridge_sha256 or self.bridge_sha256,
        ]
        singleton_patch = (
            patch.object(wake_bridge, "Singleton", singleton)
            if singleton is not None
            else patch.object(wake_bridge, "Singleton", wake_bridge.Singleton)
        )
        with patch.object(wake_bridge, "CdpSink", side_effect=sink_factory), singleton_patch, patch.object(
            wake_bridge, "__file__", str(bridge_path or self.bridge_path)
        ), patch.object(sys, "argv", argv):
            return wake_bridge.main()

    @unittest.skipUnless(sys.platform == "win32", "Windows path normalization regression")
    def test_windows_extended_workspace_accepts_equivalent_normal_absolute_profile(self):
        normal_root = self.root.resolve()
        extended_root = Path("\\\\?\\" + str(normal_root))
        resolved = wake_bridge.workspace_path(extended_root, str(self.profile.resolve()))
        self.assertEqual(self.profile.resolve(), resolved)

    @unittest.skipUnless(sys.platform == "win32", "Windows path normalization regression")
    def test_windows_extended_workspace_still_rejects_escape(self):
        normal_root = self.root.resolve()
        extended_root = Path("\\\\?\\" + str(normal_root))
        outside = normal_root.parent / "outside-profile"
        with self.assertRaises(wake_bridge.Attention):
            wake_bridge.workspace_path(extended_root, str(outside))

    def test_exact_record_requires_unread_actionable_pair(self):
        row = {
            "recordId": self.record_id,
            "unread": True,
            "state": "WAITING_FOR_CHATGPT",
            "nextAction": "chatgpt_decision_required",
        }
        self.inbox(row)
        self.assertTrue(wake_bridge.actionable(self.root, self.record_id))
        row["unread"] = False
        self.inbox(row)
        self.assertFalse(wake_bridge.actionable(self.root, self.record_id))

    def test_rust_or_malformed_selector_disables_legacy_before_state_or_browser_access(self):
        selector = self.root / ".catdesk" / "wake-bridge" / "owner.json"
        selector.write_text(json.dumps({"schemaVersion": 1, "owner": "rust"}), encoding="utf-8")
        self.assertEqual(2, self.run_main())
        self.assertEqual([], self.calls)
        self.assertFalse(self.state_path.exists())
        selector.write_text('{"schemaVersion":1,"owner":"both"}', encoding="utf-8")
        self.assertEqual(2, self.run_main())
        self.assertEqual([], self.calls)

    def test_conversation_target_is_exact_credential_free_chatgpt_c_path(self):
        self.assertEqual(
            "https://chatgpt.com/c/exact-thread",
            wake_bridge.canonical_conversation_url("https://chatgpt.com/c/exact-thread"),
        )
        sink = wake_bridge.CdpSink(
            "https://chatgpt.com/c/exact-thread", self.profile, 1, 1
        )
        self.assertTrue(
            sink.exact(
                "https://chatgpt.com/g/g-p-project/c/exact-thread"
            )
        )
        self.assertFalse(
            sink.exact(
                "https://chatgpt.com/g/g-p-project/c/different-thread"
            )
        )
        for invalid in (
            "https://chatgpt.com/c/exact-thread/",
            "https://chatgpt.com/c/exact-thread?query=value",
            "https://user@chatgpt.com/c/exact-thread",
            "https://chatgpt.com:444/c/exact-thread",
            "https://chatgpt.com/share/exact-thread",
        ):
            with self.assertRaises(wake_bridge.Attention, msg=invalid):
                wake_bridge.canonical_conversation_url(invalid)

    def test_bounded_route_class_never_returns_identifiers(self):
        expected = "https://chatgpt.com/c/6aa98932-3a84-83e9-afa7-ad10a5c495a5"
        cases = {
            expected: "SAME_CONVERSATION",
            "https://chatgpt.com/g/project-one/c/6aa98932-3a84-83e9-afa7-ad10a5c495a5": "SAME_CONVERSATION",
            "https://chatgpt.com/c/different-thread": "DIFFERENT_CONVERSATION",
            "https://chatgpt.com/": "HOME",
            "https://chatgpt.com/auth/login": "AUTH",
            "chrome-error://chromewebdata/": "CHROME_ERROR",
            "https://example.com/c/6aa98932-3a84-83e9-afa7-ad10a5c495a5": "OTHER_HOST",
        }
        for value, label in cases.items():
            result = wake_bridge.bounded_route_class(value, expected)
            self.assertEqual(label, result)
            self.assertNotIn("6aa98932", result)
            self.assertNotIn("different-thread", result)

    def test_submit_target_drift_reason_is_structural_and_identifier_free(self):
        expected = "https://chatgpt.com/c/exact-thread"
        cases = {
            "https://chatgpt.com/c/exact-thread?model=auto": "SUBMIT_TARGET_DRIFT_SAME_QUERY",
            "https://chatgpt.com/c/exact-thread#anchor": "SUBMIT_TARGET_DRIFT_SAME_FRAGMENT",
            "https://chatgpt.com/c/exact-thread/": "SUBMIT_TARGET_DRIFT_SAME_TRAILING_SLASH",
            "https://chatgpt.com/g/project-one/c/exact-thread?model=auto": "SUBMIT_TARGET_DRIFT_SAME_QUERY",
            "https://chatgpt.com/c/different-thread": "SUBMIT_TARGET_DRIFT_DIFFERENT_CONVERSATION",
            "https://chatgpt.com/": "SUBMIT_TARGET_DRIFT_HOME",
            "https://chatgpt.com/auth/login": "SUBMIT_TARGET_DRIFT_AUTH",
            "https://example.com/c/exact-thread": "SUBMIT_TARGET_DRIFT_OTHER_HOST",
            "https://chatgpt.com/share/exact-thread": "SUBMIT_TARGET_DRIFT_OTHER_ROUTE",
            "chrome-error://chromewebdata/": "SUBMIT_TARGET_DRIFT_CHROME_ERROR",
        }
        for value, reason in cases.items():
            observed = wake_bridge.bounded_submit_target_drift_reason(value, expected)
            self.assertEqual(reason, observed)
            self.assertNotIn("exact-thread", observed)
            self.assertNotIn("different-thread", observed)
            self.assertNotIn("project-one", observed)
            self.assertLessEqual(len(observed), 100)

    def test_prompt_includes_record_and_acknowledgement_without_bundle(self):
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)
        self.assertIn(self.record_id, message)
        self.assertIn("Acknowledge or claim", message)
        self.assertNotIn("authoritative diff", message)

    def test_receipt_binds_exact_record_message_and_target_without_storing_message(self):
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)
        receipt = wake_bridge.receipt_for(self.record_id, message, "https://chatgpt.com/c/review-one", 123.0)
        delivery = wake_bridge.Delivery(
            self.record_id,
            "SENT",
            1.0,
            receipt.browser_sent_at_unix,
            receipt.message_sha256,
            receipt.target_sha256,
            receipt.receipt_schema_version,
        )
        self.assertTrue(wake_bridge.valid_receipt(delivery, self.record_id, message, "https://chatgpt.com/c/review-one"))
        self.assertEqual(4, wake_bridge.State().schema_version)
        self.assertEqual(1, receipt.receipt_schema_version)
        self.assertFalse(wake_bridge.valid_receipt(delivery, "other-record", message, "https://chatgpt.com/c/review-one"))
        self.assertFalse(wake_bridge.valid_receipt(delivery, self.record_id, message + " changed", "https://chatgpt.com/c/review-one"))
        self.assertFalse(wake_bridge.valid_receipt(delivery, self.record_id, message, "https://chatgpt.com/c/other-thread"))
        self.assertNotIn(message, json.dumps(wake_bridge.asdict(delivery)))

    def test_browser_user_message_receipt_requires_a_valid_bounded_result(self):
        class Cdp:
            def evaluate(self, _script):
                return ["prior", "exact submitted message"]
        digests = wake_bridge.CdpSink.user_message_digests(Cdp())
        self.assertEqual(wake_bridge.digest(wake_bridge.normalize_message("exact submitted message")), digests[-1])
        class InvalidCdp:
            def evaluate(self, _script):
                return {"unexpected": "shape"}
        with self.assertRaises(wake_bridge.Attention):
            wake_bridge.CdpSink.user_message_digests(InvalidCdp())

    def test_browser_user_message_receipt_invokes_and_accepts_bounded_cdp_result_shapes(self):
        accepted_shapes = (
            ["prior", "exact submitted message"],
            {"result": {"value": ["prior", "exact submitted message"]}},
            {"id": 1, "result": {"type": "object", "value": ["prior", "exact submitted message"]}},
            {"result": {"result": {"value": ["prior", "exact submitted message"]}}},
        )
        class NestedCdp:
            def __init__(self, result): self.result, self.script = result, ""
            def evaluate(self, script): self.script = script; return self.result
        for shape in accepted_shapes:
            cdp = NestedCdp(shape)
            digests = wake_bridge.CdpSink.user_message_digests(cdp)
            self.assertEqual(wake_bridge.digest(wake_bridge.normalize_message("exact submitted message")), digests[-1])
            self.assertIn("[data-testid^='conversation-turn-']", cdp.script)
            self.assertIn("article[id^='conversation-turn-']", cdp.script)
            self.assertIn("[data-turn='user']", cdp.script)
            self.assertIn("turn.matches", cdp.script)
            self.assertIn(".whitespace-pre-wrap", cdp.script)
            self.assertTrue(cdp.script.rstrip().endswith("})()"))

    def test_browser_user_message_receipt_rejects_malformed_or_oversized_cdp_shapes(self):
        invalid_shapes = (
            None,
            "not a list",
            {"unexpected": ["message"]},
            {"result": {"exceptionDetails": {}}},
            ["x" * (wake_bridge.MAX_USER_MESSAGE_CHARS + 1)],
            ["message"] * (wake_bridge.MAX_USER_MESSAGES + 1),
        )
        class Cdp:
            def __init__(self, result): self.result = result
            def evaluate(self, _script): return self.result
        for shape in invalid_shapes:
            with self.assertRaises(wake_bridge.Attention):
                wake_bridge.CdpSink.user_message_digests(Cdp(shape))

    def test_browser_receipt_uses_current_message_content_not_author_container_chrome(self):
        class CurrentChatGptCdp:
            def evaluate(self, script):
                if "data-message-content" not in script or "whitespace-pre-wrap" not in script:
                    raise AssertionError("receipt query omitted the fixed message-content selectors")
                # A current author container can also hold an Edit/Copy control.
                # The fixed content-node extraction must return only message text.
                return {"result": {"value": ["prior message", "exact submitted message"]}}
        cdp = CurrentChatGptCdp()
        digests = wake_bridge.CdpSink.user_message_digests(cdp)
        self.assertEqual(wake_bridge.digest("exact submitted message"), digests[-1])

    def test_receipt_accepts_a_fresh_unique_final_digest_without_history_anchor(self):
        prior = [
            wake_bridge.digest("oldest predecessor"),
            wake_bridge.digest("stable predecessor"),
            wake_bridge.digest("trailing predecessor"),
        ]
        expected = wake_bridge.digest("exact submitted message")
        self.assertTrue(wake_bridge.CdpSink.exact_appended_receipt(prior, prior + [expected], expected))
        # A complete remount may expose no predecessor shared with the bounded
        # pre-submit list. The unique fresh final digest is the receipt proof.
        self.assertTrue(
            wake_bridge.CdpSink.exact_appended_receipt(
                prior,
                [wake_bridge.digest("remounted history row"), expected],
                expected,
            )
        )
        self.assertTrue(
            wake_bridge.CdpSink.exact_appended_receipt(
                prior,
                [wake_bridge.digest("another remounted history row"), expected],
                expected,
            )
        )

    def test_receipt_rejects_nonfresh_duplicate_or_nonfinal_digest(self):
        oldest = wake_bridge.digest("oldest predecessor")
        stable = wake_bridge.digest("stable predecessor")
        trailing = wake_bridge.digest("trailing predecessor")
        prior = [oldest, stable, trailing]
        expected = wake_bridge.digest("exact submitted message")
        other = wake_bridge.digest("other")
        cases = (
            ("SUBMIT_CLEARED_NO_APPEND", prior),
            ("SUBMIT_CLEARED_NO_APPEND", prior + [other]),
            ("SUBMIT_RECEIPT_SEQUENCE_DRIFT", prior + [expected, expected]),
            ("SUBMIT_RECEIPT_SEQUENCE_DRIFT", [*prior, expected, other]),
        )
        for expected_stage, current in cases:
            self.assertFalse(wake_bridge.CdpSink.exact_appended_receipt(prior, current, expected))
            self.assertEqual(expected_stage, wake_bridge.CdpSink.receipt_stage(prior, current, expected))

        preexisting_prior = [oldest, stable, expected]
        self.assertFalse(
            wake_bridge.CdpSink.exact_appended_receipt(
                preexisting_prior, preexisting_prior, expected
            )
        )
        self.assertEqual(
            "SUBMIT_RECEIPT_SEQUENCE_DRIFT",
            wake_bridge.CdpSink.receipt_stage(
                preexisting_prior, preexisting_prior, expected
            ),
        )

    def test_pre_submit_receipt_snapshot_rejects_an_exact_wake_turn_already_visible(self):
        class Cdp:
            def evaluate(self, script):
                self.script = script
                return ["ready", 1, 0]
        cdp = Cdp()
        with self.assertRaisesRegex(wake_bridge.Attention, "PRE_SUBMIT_EXPECTED_DIGEST"):
            wake_bridge.CdpSink.pre_submit_receipt_snapshot(cdp, "exact submitted message")
        self.assertIn("conversation-turn-", cdp.script)
        self.assertIn("exact submitted message", cdp.script)

    def test_receipt_accepts_a_single_final_digest_regardless_of_remounted_history(self):
        expected = wake_bridge.digest("exact submitted message")
        other = wake_bridge.digest("other")
        self.assertTrue(wake_bridge.CdpSink.exact_appended_receipt([], [expected], expected))
        self.assertTrue(wake_bridge.CdpSink.exact_appended_receipt([], [other, expected], expected))
        self.assertFalse(wake_bridge.CdpSink.exact_appended_receipt([], [expected, other], expected))

    def test_smoke_path_uses_direct_cdp_click_on_selected_compatible_control(self):
        class ControlCdp:
            def __init__(self, selection=("ready", "button#composer-submit-button"), click_error=None):
                self.selection, self.click_error = selection, click_error
                self.clicked = []
                self.evaluations = []
            def evaluate(self, script):
                self.evaluations.append(script)
                return list(self.selection)
            def click(self, selector, timeout):
                self.clicked.append((selector, timeout))
                if self.click_error:
                    raise self.click_error
        cdp = ControlCdp()
        self.assertEqual(("ready", "button#composer-submit-button"), wake_bridge.CdpSink.select_send_control(cdp))
        wake_bridge.CdpSink.smoke_path_submit(cdp, "button#composer-submit-button", 7)
        self.assertEqual([("button#composer-submit-button", 7)], cdp.clicked)
        self.assertTrue(any("const unique" in script for script in cdp.evaluations))
        self.assertFalse(any("button.click()" in script for script in cdp.evaluations))

    def test_smoke_path_deduplicates_same_element_and_rejects_distinct_or_disabled_controls(self):
        class ControlCdp:
            def __init__(self, status): self.status = status
            def evaluate(self, _script): return self.status
        self.assertEqual(
            ("ready", "button[data-testid='send-button']"),
            wake_bridge.CdpSink.select_send_control(ControlCdp(["ready", "button[data-testid='send-button']"])),
        )
        for status in (["ambiguous", None], ["disabled", None], ["unexpected", None]):
            self.assertNotEqual("ready", wake_bridge.CdpSink.select_send_control(ControlCdp(status))[0])

    def test_click_or_native_enter_uncertainty_is_post_boundary_and_not_retried(self):
        class ControlCdp:
            def evaluate(self, _script): return ["ready", "button[data-testid='send-button']"]
            def click(self, _selector, timeout): raise RuntimeError("fixture click error")
        with self.assertRaises(wake_bridge.PostSubmitUnknown):
            wake_bridge.CdpSink.smoke_path_submit(ControlCdp(), "button[data-testid='send-button']", 1)
        class EnterFailureCdp:
            def press_keys(self, _selector, _text, timeout): raise RuntimeError("fixture enter error")
        with self.assertRaises(wake_bridge.PostSubmitUnknown) as error:
            wake_bridge.CdpSink.smoke_path_submit(EnterFailureCdp(), None, 1)
        self.assertEqual("SUBMIT_ENTER_UNKNOWN", str(error.exception))

    def test_native_enter_is_only_used_when_no_safe_button_exists(self):
        class Cdp:
            def __init__(self): self.keys = []
            def press_keys(self, selector, text, timeout): self.keys.append((selector, text, timeout))
        cdp = Cdp()
        wake_bridge.CdpSink.smoke_path_submit(cdp, None, 3)
        self.assertEqual([(wake_bridge.EDITOR, "\n", 3)], cdp.keys)

    def test_current_prosemirror_editor_fallback_is_bounded_and_used_for_text(self):
        fallback = "div.ProseMirror[contenteditable='true']"
        case = self

        class Cdp:
            def is_element_visible(self, selector):
                return selector == fallback

            def evaluate(self, script):
                case.assertIn("#prompt-textarea", script)
                case.assertIn("div.ProseMirror[contenteditable=\"true\"]", script)
                return ["ready", True]

            def get_attribute(self, selector, _name, timeout):
                case.assertEqual(fallback, selector)
                return "fallback text"

            def get_text(self, selector, timeout):
                case.assertEqual(fallback, selector)
                return "fallback text"

        cdp = Cdp()
        self.assertEqual(fallback, wake_bridge.CdpSink.editor_selector(cdp))
        self.assertEqual(("ready", True), wake_bridge.CdpSink.editor_state(cdp))
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        self.assertEqual("fallback text", sink.text(cdp))

    def test_prevalidated_selector_compatibility_is_only_for_reduced_adapter(self):
        class ReducedCdp:
            pass

        with self.assertRaisesRegex(wake_bridge.Attention, "EDITOR_SELECTOR"):
            wake_bridge.CdpSink.editor_selector(ReducedCdp())
        self.assertEqual(
            wake_bridge.EDITOR,
            wake_bridge.CdpSink.editor_selector(ReducedCdp(), prevalidated=True),
        )

        class BrokenEvaluateCdp:
            def evaluate(self, _script):
                raise RuntimeError("fixture evaluate failure")

        with self.assertRaisesRegex(wake_bridge.Attention, "EDITOR_SELECTOR"):
            wake_bridge.CdpSink.editor_selector(BrokenEvaluateCdp(), prevalidated=True)

    def test_post_submit_visibility_uses_cdp_dom_only(self):
        class Cdp:
            def is_element_visible(self, *_args, **_kwargs):
                raise AssertionError("post-submit path must not use Selenium visibility")

            def evaluate(self, script):
                self.script = script
                return False

        cdp = Cdp()
        self.assertFalse(wake_bridge.CdpSink.post_submit_visible_any(cdp, wake_bridge.STOP))
        self.assertIn("querySelectorAll", cdp.script)
        self.assertIn("getClientRects", cdp.script)

    def test_post_submit_missing_composer_is_empty_without_visibility_probe(self):
        class Cdp:
            def is_element_visible(self, *_args, **_kwargs):
                raise AssertionError("post-submit path must not use Selenium visibility")

            def evaluate(self, script):
                self.script = script
                return ["absent", ""]

        cdp = Cdp()
        self.assertEqual("", wake_bridge.CdpSink.post_submit_composer_text(cdp))
        self.assertIn("#prompt-textarea", cdp.script)
        self.assertIn("visible.length === 0", cdp.script)

    def test_production_waits_for_stop_to_clear_then_rechecks_exact_empty_unique_editor(self):
        case = self
        class Clock:
            def __init__(self): self.now, self.sleeps = 0.0, []
            def monotonic(self): return self.now
            def sleep(self, value): self.sleeps.append(value); self.now += value
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def evaluate(self, script):
                case.assertIn("#prompt-textarea", script)
                return ["ready", True]
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 2, 1)
        clock = Clock()
        with patch.object(sink, "error", return_value=False), patch.object(
            sink,
            "visible",
            side_effect=lambda _cdp, selector: (
                selector == wake_bridge.STOP[0] and clock.now < wake_bridge.READINESS_POLL_SECONDS
            ),
        ):
            sink.wait_for_idle(Cdp(), 2, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertGreaterEqual(
            clock.now,
            wake_bridge.READINESS_POLL_SECONDS + wake_bridge.POST_LOAD_STABLE_SECONDS,
        )
        with patch.object(sink, "visible", return_value=False):
            sink.pre_typing_ready(Cdp())

    def test_production_stop_timeout_and_nonunique_editor_fail_closed_before_typing(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        clock = Clock()
        with patch.object(sink, "error", return_value=False), patch.object(sink, "visible", return_value=True):
            with self.assertRaisesRegex(wake_bridge.Attention, "CHATGPT_NOT_IDLE"):
                sink.wait_for_idle(object(), 1, monotonic=clock.monotonic, sleeper=clock.sleep)
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def evaluate(self, _script): return ["count", 2]
        with patch.object(sink, "visible", return_value=False):
            with self.assertRaisesRegex(wake_bridge.Attention, "EDITOR_SELECTOR"):
                sink.pre_typing_ready(Cdp())

    def test_page_readiness_holds_slow_editor_stable_for_ten_seconds_before_accepting(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        class Cdp:
            reloads = 0
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def evaluate(self, script):
                if script == "document.readyState": return "loading" if clock.now <= 21 else "complete"
                if "#prompt-textarea" in script:
                    return ["count", 0] if clock.now <= 21 else ["ready", True]
                raise AssertionError("unexpected fixed CDP evaluation")
            def reload(self, ignore_cache): self.reloads += 1
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        cdp = Cdp()
        self.assertIs(cdp, sink.wait_for_page_readiness(object(), cdp, monotonic=clock.monotonic, sleeper=clock.sleep))
        self.assertGreaterEqual(clock.now, 21 + wake_bridge.POST_LOAD_STABLE_SECONDS)
        self.assertLess(clock.now, 21 + wake_bridge.POST_LOAD_STABLE_SECONDS + 1.0)
        self.assertEqual(0, cdp.reloads)

    def test_home_login_control_is_route_drift_not_auth_loss(self):
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/"
            def is_element_visible(self, selector):
                return selector == wake_bridge.LOGIN[0]
        sink = wake_bridge.CdpSink(
            "https://chatgpt.com/c/review-one", self.profile, 1, 1
        )
        cdp = Cdp()
        self.assertEqual("TARGET_DRIFT", sink.readiness_reason(cdp))
        class AuthCdp(Cdp):
            def get_current_url(self): return "https://chatgpt.com/auth/login"
        with self.assertRaisesRegex(
            wake_bridge.Attention, "LOGIN_OR_PROFILE_REQUIRED"
        ):
            sink.readiness_reason(AuthCdp())

    def test_page_readiness_allows_transient_on_target_login_control_to_hydrate_to_editor(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        class Cdp:
            def __init__(self): self.reloads = 0
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def is_element_visible(self, selector):
                return selector == wake_bridge.LOGIN[0] and clock.now < 2.0
            def evaluate(self, script):
                if "#prompt-textarea" in script:
                    return ["count", 0] if clock.now < 2.0 else ["ready", True]
                if script == "document.readyState":
                    return "complete"
                raise AssertionError("unexpected fixed CDP evaluation")
            def reload(self, ignore_cache): self.reloads += 1
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        cdp = Cdp()
        self.assertIs(
            cdp,
            sink.wait_for_page_readiness(
                object(), cdp, monotonic=clock.monotonic, sleeper=clock.sleep
            ),
        )
        self.assertGreaterEqual(clock.now, 2.0)
        self.assertEqual(0, cdp.reloads)

    def test_page_readiness_blank_shell_reloads_once_then_stabilizes(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        class Cdp:
            def __init__(self): self.reloads, self.reload_at = 0, []
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def evaluate(self, script):
                if script == "document.readyState": return "complete"
                if "#prompt-textarea" in script:
                    return ["ready", True] if self.reloads == 1 else ["count", 0]
                raise AssertionError("unexpected fixed CDP evaluation")
            def reload(self, ignore_cache): self.reloads += 1; self.reload_at.append(clock.now)
        class SB:
            def __init__(self, cdp): self.cdp = cdp
        clock, cdp = Clock(), Cdp()
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        self.assertIs(cdp, sink.wait_for_page_readiness(SB(cdp), cdp, monotonic=clock.monotonic, sleeper=clock.sleep))
        self.assertEqual(1, cdp.reloads)
        self.assertEqual(
            [wake_bridge.BLANK_SHELL_RELOAD_SECONDS],
            cdp.reload_at,
        )
        self.assertGreaterEqual(
            clock.now,
            wake_bridge.BLANK_SHELL_RELOAD_SECONDS + wake_bridge.POST_LOAD_STABLE_SECONDS,
        )
        self.assertLess(clock.now, wake_bridge.READINESS_WINDOW_SECONDS * 2)

    def test_persistent_blank_shell_never_falls_through_to_second_reload(self):
        class Clock:
            now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        class Cdp:
            reload_at = None
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def is_element_visible(self, selector): return False
            def evaluate(self, script):
                if script == "document.readyState": return "complete"
                if "#prompt-textarea" in script: return ["count", 0]
                raise AssertionError(script)
            def reload(self, ignore_cache):
                assert self.reload_at is None, "blank shell reloaded twice"
                assert ignore_cache is False
                self.reload_at = clock.now
        cdp = Cdp()
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with self.assertRaisesRegex(wake_bridge.Attention, "EDITOR_SELECTOR"):
            sink.wait_for_page_readiness(object(), cdp, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual(wake_bridge.BLANK_SHELL_RELOAD_SECONDS, cdp.reload_at)
        self.assertEqual(wake_bridge.BLANK_SHELL_RELOAD_SECONDS + wake_bridge.READINESS_WINDOW_SECONDS, clock.now)

    def test_readiness_resets_stability_after_editor_disappears(self):
        class Clock:
            now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def is_element_visible(self, selector): return False
            def evaluate(self, script):
                if script == "document.readyState": return "complete"
                if "#prompt-textarea" in script:
                    return ["count", 0] if 9 <= clock.now < 10 else ["ready", True]
                raise AssertionError(script)
            def reload(self, ignore_cache): raise AssertionError("transient shell must not reload")
        cdp = Cdp()
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        self.assertIs(cdp, sink.wait_for_page_readiness(object(), cdp, monotonic=clock.monotonic, sleeper=clock.sleep))
        self.assertEqual(10 + wake_bridge.POST_LOAD_STABLE_SECONDS, clock.now)

    def test_page_readiness_all_three_fail_keeps_specific_reason_and_only_retries_between_windows(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        class Cdp:
            def __init__(self): self.opens = []
            def get_current_url(self): return "chrome-error://chromewebdata/"
            def get(self, url): self.opens.append(url)
        clock, cdp = Clock(), Cdp()
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with self.assertRaisesRegex(wake_bridge.Attention, "BROWSER_NETWORK_ERROR"):
            sink.wait_for_page_readiness(object(), cdp, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual([sink.url, sink.url], cdp.opens)
        self.assertEqual(wake_bridge.READINESS_WINDOW_SECONDS, clock.now)

    def test_network_diagnostics_export_only_fixed_codes(self):
        class Cdp:
            def __init__(self, value): self.value = value
            def evaluate(self, script):
                self_script = "document.querySelector('#error-code')?.textContent?.trim() || ''"
                assert script == self_script
                return self.value
        for code in wake_bridge.NETWORK_ERROR_CODES:
            self.assertEqual(code, wake_bridge.CdpSink.network_error_code(Cdp(code)))
        for value in [None, {}, 42, "page content", "ERR_UNKNOWN_CUSTOM", "ERR_TIMED_OUT\nsecret"]:
            self.assertEqual("UNKNOWN", wake_bridge.CdpSink.network_error_code(Cdp(value)))
        self.assertEqual("UNKNOWN", wake_bridge.CdpSink.network_error_code(object()))

    def test_network_diagnostics_preserve_bounded_recovery_and_terminal_reason(self):
        class Clock:
            now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        class Cdp:
            opens = 0
            def get_current_url(self): return "chrome-error://chromewebdata/"
            def get(self, url): self.opens += 1
            def evaluate(self, script): return "ERR_CONNECTION_RESET"
        clock, cdp, observations = Clock(), Cdp(), []
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with self.assertRaisesRegex(wake_bridge.Attention, "BROWSER_NETWORK_ERROR"):
            sink.wait_for_page_readiness(object(), cdp, monotonic=clock.monotonic,
                sleeper=clock.sleep, observer=lambda route, reason: observations.append((route, reason)))
        self.assertIn(("CHROME_ERROR", "NETWORK_ERR_CONNECTION_RESET"), observations)
        self.assertEqual(2, cdp.opens)
        self.assertEqual(wake_bridge.READINESS_WINDOW_SECONDS, clock.now)

    def test_readiness_classifies_chrome_error_from_one_authoritative_url_snapshot(self):
        class Cdp:
            def __init__(self): self.url_reads = 0
            def get_current_url(self):
                self.url_reads += 1
                if self.url_reads == 1:
                    return "chrome-error://chromewebdata/"
                return "https://chatgpt.com/c/review-one"
            def is_element_visible(self, _selector): return False
        cdp = Cdp()
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        self.assertEqual("BROWSER_NETWORK_ERROR", sink.readiness_reason(cdp))
        self.assertEqual(1, cdp.url_reads)

    def test_page_readiness_reopens_exact_target_when_reload_leaves_chrome_error_document(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        class Cdp:
            def __init__(self, url):
                self.url = url
                self.reloads = 0
            def get_current_url(self): return self.url
            def reload(self, ignore_cache): self.reloads += 1
            def evaluate(self, script):
                if script == "document.readyState":
                    return "complete"
                if "#prompt-textarea" in script:
                    return ["ready", True]
                raise AssertionError("unexpected fixed CDP evaluation")
            def is_element_visible(self, _selector): return False
        class SB:
            def __init__(self, cdp):
                self.cdp = cdp
                self.activations = []
            def activate_cdp_mode(self, url):
                self.activations.append(url)
                self.cdp = Cdp(url)
        clock, cdp = Clock(), Cdp("chrome-error://chromewebdata/")
        sb = SB(cdp)
        target = "https://chatgpt.com/c/review-one"
        sink = wake_bridge.CdpSink(target, self.profile, 1, 1)
        result = sink.wait_for_page_readiness(
            sb,
            cdp,
            monotonic=clock.monotonic,
            sleeper=clock.sleep,
        )
        self.assertIs(sb.cdp, result)
        self.assertEqual(0, cdp.reloads)
        self.assertEqual([target], sb.activations)
        self.assertLess(clock.now, wake_bridge.READINESS_WINDOW_SECONDS)

    def test_page_readiness_reopens_exact_target_when_cdp_shell_falls_back_home(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        class Cdp:
            def __init__(self, url):
                self.url = url
                self.reloads = 0
            def get_current_url(self): return self.url
            def reload(self, ignore_cache): self.reloads += 1
            def evaluate(self, script):
                if script == "document.readyState":
                    return "complete"
                if "#prompt-textarea" in script:
                    return ["ready", True]
                raise AssertionError("unexpected fixed CDP evaluation")
            def is_element_visible(self, _selector): return False
        class SB:
            def __init__(self, cdp):
                self.cdp = cdp
                self.activations = []
            def activate_cdp_mode(self, url):
                self.activations.append(url)
                self.cdp = Cdp(url)
        clock, cdp = Clock(), Cdp("https://chatgpt.com/")
        sb = SB(cdp)
        target = "https://chatgpt.com/c/review-one"
        sink = wake_bridge.CdpSink(target, self.profile, 1, 1)
        result = sink.wait_for_page_readiness(
            sb,
            cdp,
            monotonic=clock.monotonic,
            sleeper=clock.sleep,
        )
        self.assertIs(sb.cdp, result)
        self.assertEqual(0, cdp.reloads)
        self.assertEqual([target], sb.activations)
        self.assertGreaterEqual(clock.now, wake_bridge.READINESS_WINDOW_SECONDS)
        self.assertLess(clock.now, wake_bridge.READINESS_WINDOW_SECONDS * 2)

    def test_page_readiness_allows_transient_home_to_stabilize_without_reopening_target(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        class Cdp:
            def __init__(self): self.opens = []
            def get_current_url(self):
                return "https://chatgpt.com/" if clock.now < 2.0 else "https://chatgpt.com/c/review-one"
            def get(self, url): self.opens.append(url)
            def is_element_visible(self, _selector): return False
            def evaluate(self, script):
                if script == "document.readyState": return "complete"
                if "#prompt-textarea" in script: return ["ready", True]
                raise AssertionError("unexpected fixed CDP evaluation")
        class SB:
            def activate_cdp_mode(self, _url): raise AssertionError("transient HOME must not trigger navigation")
        cdp = Cdp()
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        self.assertIs(cdp, sink.wait_for_page_readiness(SB(), cdp, monotonic=clock.monotonic, sleeper=clock.sleep))
        self.assertGreaterEqual(clock.now, 2.0)
        self.assertLess(clock.now, wake_bridge.READINESS_WINDOW_SECONDS)
        self.assertEqual([], cdp.opens)

    def test_page_readiness_persistent_home_exhausts_budget_then_target_drift(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        class Cdp:
            def __init__(self): self.opens = []
            def get_current_url(self): return "https://chatgpt.com/"
            def get(self, url): self.opens.append((url, clock.now))
            def is_element_visible(self, _selector): return False
        class SB:
            def activate_cdp_mode(self, _url): raise AssertionError("active CDP open is available")
        cdp = Cdp()
        target = "https://chatgpt.com/c/review-one"
        sink = wake_bridge.CdpSink(target, self.profile, 1, 1)
        with self.assertRaisesRegex(wake_bridge.Attention, "TARGET_DRIFT"):
            sink.wait_for_page_readiness(SB(), cdp, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual(
            [
                (target, wake_bridge.READINESS_WINDOW_SECONDS),
                (
                    target,
                    wake_bridge.READINESS_WINDOW_SECONDS * 2
                    + wake_bridge.HOME_RECOVERY_SETTLE_SECONDS,
                ),
            ],
            cdp.opens,
        )
        self.assertGreaterEqual(
            clock.now,
            wake_bridge.READINESS_WINDOW_SECONDS * 3
            + wake_bridge.HOME_RECOVERY_SETTLE_SECONDS * 2,
        )

    def test_retry_page_readiness_prefers_active_cdp_get_for_home_recovery(self):
        target = "https://chatgpt.com/c/review-one"
        class Cdp:
            def __init__(self):
                self.url = "https://chatgpt.com/"
                self.reloads = 0
                self.navigations = []
            def get_current_url(self): return self.url
            def reload(self, ignore_cache): self.reloads += 1
            def get(self, url):
                self.navigations.append(url)
                self.url = url
            def is_element_visible(self, _selector): return False
        class SB:
            def activate_cdp_mode(self, _url):
                raise AssertionError("active CDP open should be preferred")
        cdp = Cdp()
        sink = wake_bridge.CdpSink(target, self.profile, 1, 1)
        result = sink.retry_page_readiness(SB(), cdp)
        self.assertIs(cdp, result)
        self.assertEqual(0, cdp.reloads)
        self.assertEqual([target], cdp.navigations)
        self.assertTrue(sink.exact(cdp.get_current_url()))

    def test_retry_page_readiness_waits_for_delayed_home_to_exact_transition(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        target = "https://chatgpt.com/c/review-one"
        class Cdp:
            def __init__(self):
                self.navigation_at = None
                self.navigations = []
            def get_current_url(self):
                if self.navigation_at is None:
                    return "https://chatgpt.com/"
                if clock.now - self.navigation_at < 2.0:
                    return "https://chatgpt.com/"
                return target
            def get(self, url):
                self.navigations.append(url)
                self.navigation_at = clock.now
            def is_element_visible(self, _selector): return False
        class SB:
            def activate_cdp_mode(self, _url):
                raise AssertionError("active CDP get should be preferred")
        observed = []
        cdp = Cdp()
        sink = wake_bridge.CdpSink(target, self.profile, 1, 1)
        result = sink.retry_page_readiness(
            SB(),
            cdp,
            lambda route, reason: observed.append((route, reason)),
            monotonic=clock.monotonic,
            sleeper=clock.sleep,
        )
        self.assertIs(cdp, result)
        self.assertEqual([target], cdp.navigations)
        self.assertGreaterEqual(clock.now, 2.0)
        self.assertLess(clock.now, wake_bridge.HOME_RECOVERY_SETTLE_SECONDS)
        self.assertEqual(("SAME_CONVERSATION", "RECOVERY_RETURN"), observed[-1])

    def test_retry_page_readiness_persistent_home_waits_full_settle_window(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        target = "https://chatgpt.com/c/review-one"
        class Cdp:
            def __init__(self): self.navigations = []
            def get_current_url(self): return "https://chatgpt.com/"
            def get(self, url): self.navigations.append(url)
            def is_element_visible(self, _selector): return False
        class SB:
            def activate_cdp_mode(self, _url):
                raise AssertionError("active CDP get should be preferred")
        observed = []
        cdp = Cdp()
        sink = wake_bridge.CdpSink(target, self.profile, 1, 1)
        sink.retry_page_readiness(
            SB(),
            cdp,
            lambda route, reason: observed.append((route, reason)),
            monotonic=clock.monotonic,
            sleeper=clock.sleep,
        )
        self.assertEqual([target], cdp.navigations)
        self.assertGreaterEqual(clock.now, wake_bridge.HOME_RECOVERY_SETTLE_SECONDS)
        self.assertEqual(("HOME", "RECOVERY_RETURN"), observed[-1])

    def test_page_readiness_fails_closed_immediately_for_auth_path_or_captcha(self):
        class Cdp:
            def __init__(self, url, captcha=False): self.url, self.captcha, self.reloads = url, captcha, 0
            def get_current_url(self): return self.url
            def is_element_visible(self, selector): return self.captcha and selector == wake_bridge.CAPTCHA[0]
            def reload(self, ignore_cache): self.reloads += 1
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        cases = (("https://chatgpt.com/auth/login", False, "LOGIN_OR_PROFILE_REQUIRED"),
                 ("https://chatgpt.com/c/review-one", True, "CAPTCHA_OR_SECURITY_CHECK"))
        for url, captcha, expected in cases:
            cdp = Cdp(url, captcha)
            with self.assertRaisesRegex(wake_bridge.Attention, expected):
                sink.wait_for_page_readiness(object(), cdp, monotonic=lambda: 0, sleeper=lambda _seconds: None)
            self.assertEqual(0, cdp.reloads)

    def test_page_readiness_allows_transient_target_drift_to_settle_before_typing(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        class Cdp:
            def __init__(self): self.reloads = 0
            def get_current_url(self):
                return (
                    "https://chatgpt.com/c/other"
                    if clock.now < 2.0
                    else "https://chatgpt.com/c/review-one"
                )
            def is_element_visible(self, _selector): return False
            def evaluate(self, script):
                if "#prompt-textarea" in script:
                    return ["count", 0] if clock.now < 2.0 else ["ready", True]
                if script == "document.readyState":
                    return "complete"
                raise AssertionError("unexpected fixed CDP evaluation")
            def reload(self, ignore_cache): self.reloads += 1
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        cdp = Cdp()
        self.assertIs(
            cdp,
            sink.wait_for_page_readiness(
                object(), cdp, monotonic=clock.monotonic, sleeper=clock.sleep
            ),
        )
        self.assertGreaterEqual(clock.now, 2.0)
        self.assertEqual(0, cdp.reloads)

    def test_wake_never_types_when_page_readiness_has_not_succeeded(self):
        typed, contexts = [], []
        class Cdp:
            def press_keys(self, *_args, **_kwargs): typed.append("typed")
        class SB:
            def __init__(self, **_kwargs): self.cdp = Cdp()
            def __enter__(self): contexts.append("open"); return self
            def __exit__(self, *_args): contexts.append("close"); return False
            def activate_cdp_mode(self, _url): return None
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            sink, "wait_for_page_readiness", side_effect=wake_bridge.Attention("EDITOR_SELECTOR")
        ):
            with self.assertRaisesRegex(wake_bridge.Attention, "EDITOR_SELECTOR"):
                sink.wake(self.record_id, "message", lambda: True, lambda: self.fail("submission boundary"))
        self.assertEqual([], typed)
        self.assertEqual(["open", "close"], contexts)

    def test_outer_network_retry_reopens_only_before_first_browser_write(self):
        contexts, cdps = [], []
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)
        receipt = object()

        class Cdp:
            def __init__(self, index):
                self.index, self.writes = index, []
            def get_current_url(self):
                return "https://chatgpt.com/c/review-one"
            def press_keys(self, _selector, text, timeout):
                self.writes.append((text, timeout))

        class SB:
            def __init__(self, **_kwargs):
                self.cdp = Cdp(len(cdps))
                cdps.append(self.cdp)
            def __enter__(self):
                contexts.append(("open", self.cdp.index))
                return self
            def __exit__(self, *_args):
                contexts.append(("close", self.cdp.index))
                return False
            def activate_cdp_mode(self, _url):
                return None

        def readiness(_sb, cdp):
            if cdp.index == 0:
                raise wake_bridge.Attention("BROWSER_NETWORK_ERROR")
            return cdp

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            wake_bridge.time, "sleep"
        ), patch.object(sink, "wait_for_page_readiness", side_effect=readiness), patch.object(
            sink, "wait_for_idle"
        ), patch.object(sink, "pre_typing_ready"), patch.object(
            sink, "pre_submit_receipt_snapshot", return_value=0
        ), patch.object(sink, "best_effort_present_browser"), patch.object(
            sink, "text", return_value=message
        ), patch.object(sink, "wait_for_send_control", return_value=None), patch.object(
            sink, "smoke_path_submit"
        ), patch.object(
            sink, "wait_for_submission_accepted", return_value=99.0
        ), patch.object(sink, "durable_receipt_round_trip"), patch.object(
            sink, "confirm_exact_receipt", return_value=receipt
        ), patch.object(
            sink, "wait_for_response_completion", return_value=None
        ):
            self.assertIs(receipt, sink.wake(self.record_id, message, lambda: True, lambda: None))

        self.assertEqual(2, len(cdps))
        self.assertEqual([], cdps[0].writes)
        self.assertEqual([(message, 1)], cdps[1].writes)
        self.assertEqual([("open", 0), ("close", 0), ("open", 1), ("close", 1)], contexts)

    def test_outer_network_retry_stops_after_three_pre_write_sessions(self):
        contexts = []

        class Cdp:
            def press_keys(self, *_args, **_kwargs):
                self.fail("network readiness failure must never type")

        class SB:
            def __init__(self, **_kwargs):
                self.cdp = Cdp()
            def __enter__(self):
                contexts.append("open")
                return self
            def __exit__(self, *_args):
                contexts.append("close")
                return False
            def activate_cdp_mode(self, _url):
                return None

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            wake_bridge.time, "sleep"
        ), patch.object(
            sink, "wait_for_page_readiness", side_effect=wake_bridge.Attention("BROWSER_NETWORK_ERROR")
        ):
            with self.assertRaisesRegex(wake_bridge.Attention, "BROWSER_NETWORK_ERROR"):
                sink.wake(self.record_id, "message", lambda: True, lambda: self.fail("submission boundary"))

        self.assertEqual(["open", "close"] * wake_bridge.READINESS_ATTEMPTS, contexts)

    def test_outer_retry_never_relaunches_for_non_network_attention(self):
        contexts = []

        class SB:
            def __init__(self, **_kwargs):
                self.cdp = object()
            def __enter__(self):
                contexts.append("open")
                return self
            def __exit__(self, *_args):
                contexts.append("close")
                return False
            def activate_cdp_mode(self, _url):
                return None

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            sink, "wait_for_page_readiness", side_effect=wake_bridge.Attention("LOGIN_OR_PROFILE_REQUIRED")
        ):
            with self.assertRaisesRegex(wake_bridge.Attention, "LOGIN_OR_PROFILE_REQUIRED"):
                sink.wake(self.record_id, "message", lambda: True, lambda: self.fail("submission boundary"))

        self.assertEqual(["open", "close"], contexts)

    def test_outer_retry_never_relaunches_after_browser_write_starts(self):
        contexts, writes = [], []
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)

        class Cdp:
            def press_keys(self, _selector, text, timeout):
                writes.append((text, timeout))

        class SB:
            def __init__(self, **_kwargs):
                self.cdp = Cdp()
            def __enter__(self):
                contexts.append("open")
                return self
            def __exit__(self, *_args):
                contexts.append("close")
                return False
            def activate_cdp_mode(self, _url):
                return None

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            wake_bridge.time, "sleep"
        ), patch.object(sink, "wait_for_page_readiness", side_effect=lambda _sb, cdp: cdp), patch.object(
            sink, "wait_for_idle"
        ), patch.object(sink, "pre_typing_ready"), patch.object(
            sink, "pre_submit_receipt_snapshot", return_value=0
        ), patch.object(sink, "best_effort_present_browser"), patch.object(
            sink, "text", side_effect=wake_bridge.Attention("BROWSER_NETWORK_ERROR")
        ):
            with self.assertRaisesRegex(wake_bridge.Attention, "BROWSER_NETWORK_ERROR"):
                sink.wake(self.record_id, message, lambda: True, lambda: self.fail("submission boundary"))

        self.assertEqual([(message, 1)], writes)
        self.assertEqual(["open", "close"], contexts)

    def test_production_waits_for_safe_send_and_never_converts_disabled_to_enter(self):
        class Clock:
            def __init__(self): self.now, self.sleeps = 0.0, []
            def monotonic(self): return self.now
            def sleep(self, value): self.sleeps.append(value); self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 2, 1)
        clock = Clock()
        with patch.object(sink, "select_send_control", side_effect=[("disabled", None), ("ready", "button#composer-submit-button")]):
            self.assertEqual("button#composer-submit-button", sink.wait_for_send_control(object(), 2, monotonic=clock.monotonic, sleeper=clock.sleep))
        self.assertEqual([0.25], clock.sleeps)
        with patch.object(sink, "select_send_control", return_value=("disabled", None)):
            clock = Clock()
            with self.assertRaisesRegex(wake_bridge.Attention, "SEND_SELECTOR"):
                sink.wait_for_send_control(object(), 1, monotonic=clock.monotonic, sleeper=clock.sleep)

    def test_submission_acceptance_uses_same_document_stop_signal_without_reload(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value

        prior = 0
        expected = wake_bridge.digest("exact submitted message")
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def reload(self, *_args, **_kwargs):
                raise AssertionError("sender must not reload before response completion")
        clock = Clock()
        with patch.object(sink, "post_submit_composer_text", return_value=""), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 0)
        ), patch.object(
            sink,
            "post_submit_visible_any",
            side_effect=lambda _cdp, selectors: selectors == wake_bridge.STOP,
        ), patch.object(
            sink, "response_snapshot", side_effect=AssertionError("active Stop is sufficient")
        ):
            accepted = sink.wait_for_submission_accepted(
                Cdp(),
                prior,
                expected,
                "exact submitted message",
                monotonic=clock.monotonic,
                sleeper=clock.sleep,
                wall_time=lambda: 7.0,
            )
        self.assertEqual(7.0, accepted)
        self.assertEqual(0.0, clock.now)

    def test_submission_acceptance_recovers_chrome_error_without_resubmitting(self):
        prior = 0
        expected = wake_bridge.digest("exact submitted message")
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)

        class Cdp:
            def __init__(self):
                self.recovered = False
                self.opens = []
            def get_current_url(self):
                return (
                    "https://chatgpt.com/c/review-one"
                    if self.recovered
                    else "chrome-error://chromewebdata/"
                )
            def open(self, url):
                self.opens.append(url)
                self.recovered = True
            def press_keys(self, *_args, **_kwargs):
                raise AssertionError("post-submit recovery must never type or re-submit")

        cdp = Cdp()
        with patch.object(sink, "error", return_value=False), patch.object(
            sink, "document_ready_state", return_value="complete"
        ), patch.object(
            sink, "post_submit_composer_text", return_value=""
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 0)
        ), patch.object(
            sink,
            "post_submit_visible_any",
            side_effect=lambda _cdp, selectors: selectors == wake_bridge.STOP,
        ), patch.object(
            sink, "response_snapshot", side_effect=AssertionError("active Stop is sufficient")
        ):
            accepted = sink.wait_for_submission_accepted(
                cdp,
                prior,
                expected,
                "exact submitted message",
                wall_time=lambda: 9.0,
            )

        self.assertEqual(9.0, accepted)
        self.assertEqual(["https://chatgpt.com/c/review-one"], cdp.opens)

    def test_submission_acceptance_does_not_reopen_non_chrome_target_drift(self):
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)

        class Cdp:
            def get_current_url(self):
                return "https://chatgpt.com/c/different-thread"
            def open(self, _url):
                raise AssertionError("non-chrome target drift must fail closed")

        with self.assertRaisesRegex(
            wake_bridge.PostSubmitUnknown,
            "SUBMIT_TARGET_DRIFT_DIFFERENT_CONVERSATION",
        ):
            sink.wait_for_submission_accepted(
                Cdp(),
                0,
                wake_bridge.digest("exact submitted message"),
                "exact submitted message",
            )

    def test_submission_acceptance_allows_response_that_completed_before_stop_poll(self):
        prior = 0
        expected = wake_bridge.digest("exact submitted message")
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        with patch.object(sink, "post_submit_composer_text", return_value=""), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 0)
        ), patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "response_snapshot", return_value=(0, 0, True, True)
        ):
            accepted = sink.wait_for_submission_accepted(
                Cdp(), prior, expected, "exact submitted message", wall_time=lambda: 8.0
            )
        self.assertEqual(8.0, accepted)

    def test_submission_acceptance_refuses_same_document_append_without_server_turn_signal(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value

        prior = 0
        expected = wake_bridge.digest("exact submitted message")
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        clock = Clock()
        with patch.object(wake_bridge, "POST_SUBMIT_RECEIPT_WINDOW_SECONDS", .5), patch.object(
            sink, "post_submit_composer_text", return_value=""
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 0)
        ), patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "response_snapshot", return_value=(0, 0, False, False)
        ):
            with self.assertRaisesRegex(
                wake_bridge.PostSubmitUnknown, "SUBMIT_ACCEPTANCE_UNPROVEN"
            ):
                sink.wait_for_submission_accepted(
                    Cdp(),
                    prior,
                    expected,
                    "exact submitted message",
                    monotonic=clock.monotonic,
                    sleeper=clock.sleep,
                )

    def test_post_submit_receipt_requires_fresh_document_round_trip(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value

        class Cdp:
            def __init__(self, completes_reload=True):
                self.marker = False
                self.completes_reload = completes_reload
                self.reloads = 0
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def is_element_visible(self, _selector): return False
            def evaluate(self, script):
                if "__catdeskWakeReceiptPreReloadV1 = true" in script:
                    self.marker = True
                    return True
                if "typeof window.__catdeskWakeReceiptPreReloadV1" in script:
                    return not self.marker
                if script == "document.readyState":
                    return "complete"
                if "#prompt-textarea" in script:
                    raise AssertionError("post-submit receipt readiness must not query editor state")
                raise AssertionError("unexpected fixed CDP evaluation")
            def reload(self, ignore_cache):
                self.reloads += 1
                if self.completes_reload:
                    self.marker = False

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)
        clock = Clock()
        cdp = Cdp()
        with patch.object(sink, "post_submit_visible_any", return_value=False):
            sink.durable_receipt_round_trip(cdp, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual(1, cdp.reloads)

        clock = Clock()
        stale = Cdp(completes_reload=False)
        with patch.object(sink, "post_submit_visible_any", return_value=False):
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "SUBMIT_RECEIPT_ROUND_TRIP_FAILED"):
                sink.durable_receipt_round_trip(stale, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual(1, stale.reloads)

    def test_same_document_optimistic_append_cannot_become_definite_success(self):
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)
        boundary = []

        class Cdp:
            def __init__(self): self.composer, self.messages = "", ["prior message"]
            def is_element_visible(self, selector): return selector == wake_bridge.EDITOR
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def get_attribute(self, _selector, _name, timeout): return self.composer
            def get_text(self, _selector, timeout): return self.composer
            def press_keys(self, _selector, text, timeout):
                if text == "\n":
                    self.composer = ""
                    self.messages.append(message)
                else:
                    self.composer = text
            def evaluate(self, script):
                if script == "document.readyState": return "complete"
                if "#prompt-textarea" in script: return ["ready", self.composer == ""]
                if "data-message-author-role" in script: return list(self.messages)
                if "const unique" in script: return ["missing", None]
                if "__catdeskWakeReceiptPreReloadV1 = true" in script: return True
                raise AssertionError("unexpected fixed CDP evaluation")

        cdp = Cdp()
        class SB:
            def __init__(self, **_kwargs): self.cdp = cdp
            def __enter__(self): return self
            def __exit__(self, *_args): return False
            def activate_cdp_mode(self, _url): return None

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            sink, "wait_for_submission_accepted", return_value=99.0
        ), patch.object(
            sink, "post_submit_visible_any", return_value=False
        ), patch.object(
            sink, "post_submit_composer_text", return_value=""
        ):
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "SUBMIT_RECEIPT_ROUND_TRIP_FAILED"):
                sink.wake(self.record_id, message, lambda: True, lambda: boundary.append("submitted"))
        self.assertEqual(["submitted"], boundary)
        self.assertEqual(message, cdp.messages[-1])

    def test_post_submit_round_trip_target_drift_fails_closed(self):
        class Cdp:
            def __init__(self): self.marker = False; self.reloaded = False
            def get_current_url(self):
                return "https://chatgpt.com/c/other" if self.reloaded else "https://chatgpt.com/c/review-one"
            def evaluate(self, script):
                if "__catdeskWakeReceiptPreReloadV1 = true" in script:
                    self.marker = True
                    return True
                return True
            def reload(self, ignore_cache): self.reloaded = True; self.marker = False
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)
        with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "SUBMIT_TARGET_DRIFT"):
            sink.durable_receipt_round_trip(Cdp())

    def test_production_post_submit_requires_two_stable_receipt_polls_and_honors_stop_hold(self):
        class Clock:
            def __init__(self): self.now, self.sleeps = 0.0, []
            def monotonic(self): return self.now
            def sleep(self, value): self.sleeps.append(value); self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 3)
        prior, expected = 0, wake_bridge.digest("exact")
        clock, stop_seen = Clock(), False
        def visible(_cdp, _selector):
            nonlocal stop_seen
            if not stop_seen:
                stop_seen = True
                return True
            return False
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        with patch.object(
            sink,
            "post_submit_visible_any",
            side_effect=lambda _cdp, selectors: visible(_cdp, selectors[0]) if selectors == wake_bridge.STOP else False,
        ), patch.object(
            sink, "post_submit_composer_text", return_value=""
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 0)
        ):
            receipt = sink.confirm_exact_receipt(Cdp(), prior, expected, self.record_id, "exact", monotonic=clock.monotonic, sleeper=clock.sleep, wall_time=lambda: 7.0)
        self.assertEqual(7.0, receipt.browser_sent_at_unix)
        self.assertGreaterEqual(sum(clock.sleeps), wake_bridge.POST_STOP_HOLD_SECONDS)
        clock = Clock()
        remounted_one = wake_bridge.digest("remounted one")
        remounted_two = wake_bridge.digest("remounted two")
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "post_submit_composer_text", return_value=""
        ), patch.object(
            sink, "trusted_turn_anchor_state", side_effect=[(1, 1), (1, 1)]
        ):
            sink.confirm_exact_receipt(Cdp(), prior, expected, self.record_id, "exact", monotonic=clock.monotonic, sleeper=clock.sleep, wall_time=lambda: 8.0)
        self.assertEqual([0.25], clock.sleeps)


    def test_receipt_query_uses_role_attribute_without_div_tag_assumption(self):
        class Cdp:
            def evaluate(self, script):
                self.script = script
                return ["exact wake message"]

        cdp = Cdp()
        digests = wake_bridge.CdpSink.user_message_digests(cdp)
        self.assertEqual([wake_bridge.digest("exact wake message")], digests)
        self.assertIn("[data-message-author-role='user']", cdp.script)
        self.assertIn("article[data-turn='user']", cdp.script)
        self.assertIn("[data-testid^='conversation-turn-']", cdp.script)
        self.assertIn("article[id^='conversation-turn-']", cdp.script)
        self.assertIn("[data-turn='user']", cdp.script)
        self.assertIn("turn.matches", cdp.script)
        self.assertIn("[data-message-content]", cdp.script)
        self.assertIn(".whitespace-pre-wrap", cdp.script)
        self.assertNotIn("div[data-message-author-role='user']", cdp.script)

    def test_turn_anchor_and_response_queries_accept_role_only_turn_dom(self):
        class AnchorCdp:
            def evaluate(self, script):
                self.script = script
                return ["ready", 1, 1]

        anchor = AnchorCdp()
        self.assertEqual(
            (1, 1),
            wake_bridge.CdpSink.trusted_turn_anchor_state(anchor, "exact wake message"),
        )
        self.assertIn("[data-message-author-role='user']", anchor.script)
        self.assertIn("[data-message-author-role='assistant']", anchor.script)
        self.assertIn("[data-turn='user']", anchor.script)
        self.assertIn("[data-turn='assistant']", anchor.script)
        self.assertIn("[data-content-search-unit-key]", anchor.script)
        self.assertIn("[data-content-search-turn-key]", anchor.script)
        self.assertIn("[data-user-message-bubble]", anchor.script)
        self.assertIn("[class~='group/user-message']", anchor.script)
        self.assertIn("const classifyTurn = turn =>", anchor.script)
        self.assertIn("const selectorTiers = [", anchor.script)
        self.assertLess(
            anchor.script.index('"[data-content-search-unit-key]"'),
            anchor.script.index('"[data-content-search-turn-key]"'),
        )
        self.assertIn("for (const selector of selectorTiers)", anchor.script)
        self.assertIn("return isUserTurn !== isAssistantTurn;", anchor.script)
        self.assertIn("if (tierTurns.some(turn => classifyTurn(turn)[0]))", anchor.script)
        self.assertIn("turns = tierTurns;", anchor.script)
        self.assertNotIn("const rawTurns =", anchor.script)
        self.assertIn("isUserTurn && !isAssistantTurn && text.includes(expected)", anchor.script)
        self.assertNotIn("if (text.includes(expected)) matches.push(index);", anchor.script)

        class ResponseCdp:
            def evaluate(self, script):
                self.script = script
                return ["ready", 0, 0, True, True]

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        response = ResponseCdp()
        self.assertEqual((0, 0, True, True), sink.response_snapshot(response, "exact wake message"))
        self.assertIn("[data-message-author-role='user']", response.script)
        self.assertIn("[data-message-author-role='assistant']", response.script)
        self.assertIn("[data-turn='user']", response.script)
        self.assertIn("[data-turn='assistant']", response.script)
        self.assertIn("[data-content-search-turn-key]", response.script)
        self.assertIn("[data-content-search-unit-key]", response.script)
        self.assertIn("[data-user-message-bubble]", response.script)
        self.assertIn("pairedAssistantUnits", response.script)
        self.assertIn("pairedAssistantComplete", response.script)
        self.assertIn("[data-chatgpt-selection-message-id]", response.script)
        self.assertIn("pairedTurn ? pairedAssistantUnits : turnsAfterWake", response.script)
        self.assertIn("pairedCompletionAction", response.script)
        self.assertIn("inWakeExchange", response.script)

        class RetryCdp:
            def evaluate(self, script):
                self.script = script
                return "clicked"

        retry = RetryCdp()
        self.assertIsNone(sink.click_response_timeout_retry(retry, "exact wake message"))
        self.assertIn("[data-content-search-turn-key]", retry.script)
        self.assertIn("[data-user-message-bubble]", retry.script)
        self.assertIn("inWakeExchange", retry.script)

    def test_response_snapshot_accepts_hidden_but_present_completed_turn_action(self):
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        class Cdp:
            def evaluate(self, script):
                # Regression for diagnostic 008: ChatGPT may keep the completed-turn
                # copy action in the DOM while CSS hides it until hover. Completion
                # must depend on presence, not visible(completionAction).
                self.script = script
                return ["ready", 0, 0, True, True]
        cdp = Cdp()
        self.assertEqual((0, 0, True, True), sink.response_snapshot(cdp, "exact wake message"))
        self.assertIn("Boolean(completionAction)", cdp.script)
        self.assertNotIn("visible(completionAction)", cdp.script)
        self.assertIn("expectedWake", cdp.script)
        self.assertIn("[data-message-author-role='assistant']", cdp.script)
        self.assertIn("article[data-turn='assistant']", cdp.script)
        self.assertIn("[data-testid^='conversation-turn-']", cdp.script)
        self.assertIn("article[id^='conversation-turn-']", cdp.script)
        self.assertIn("wakeTurns", cdp.script)
        self.assertNotIn("div[data-message-author-role='user']", cdp.script)
        self.assertNotIn("div[data-message-author-role='assistant']", cdp.script)

    def test_response_completion_waits_for_generation_to_finish_before_success(self):
        class Clock:
            def __init__(self): self.now, self.sleeps = 0.0, []
            def monotonic(self): return self.now
            def sleep(self, value): self.sleeps.append(value); self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        expected = wake_bridge.digest("exact")
        clock, stages = Clock(), []
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        with patch.object(
            sink,
            "post_submit_visible_any",
            side_effect=lambda _cdp, selectors: selectors == wake_bridge.STOP and clock.now < 1.0,
        ), patch.object(
            sink, "editor_state", return_value=("ready", True)
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 1)
        ), patch.object(
            sink, "response_snapshot", side_effect=[(0, 0, True, False), (0, 0, True, True)]
        ):
            sink.wait_for_response_completion(
                Cdp(),
                expected,
                observer=stages.append,
                monotonic=clock.monotonic,
                sleeper=clock.sleep,
            )
        self.assertEqual(
            ["GENERATION_WAIT", "GENERATION_ACTIVE", "RESPONSE_COMPLETED"],
            stages,
        )
        self.assertGreaterEqual(clock.now, 1.5)

    def test_active_generation_periodically_refreshes_same_target_until_pause_clears(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        expected = wake_bridge.digest("exact")
        clock, stages = Clock(), []
        class Cdp:
            def __init__(self): self.reloads = 0
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def reload(self, ignore_cache=False):
                self.reloads += 1
        cdp = Cdp()
        with patch.object(sink, "error", return_value=False), patch.object(
            sink, "document_ready_state", return_value="complete"
        ), patch.object(
            sink,
            "post_submit_visible_any",
            side_effect=lambda _cdp, selectors: selectors == wake_bridge.STOP and cdp.reloads == 0,
        ), patch.object(
            sink, "editor_state", return_value=("ready", True)
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 1)
        ), patch.object(
            sink, "response_snapshot", return_value=(0, 0, True, True)
        ), patch.object(
            sink, "reopen_response_target"
        ) as reopen:
            sink.wait_for_response_completion(
                cdp,
                expected,
                observer=stages.append,
                monotonic=clock.monotonic,
                sleeper=clock.sleep,
            )
        self.assertEqual(1, cdp.reloads)
        self.assertGreaterEqual(clock.now, wake_bridge.RESPONSE_ACTIVE_REFRESH_SECONDS)
        self.assertEqual("RESPONSE_COMPLETED", stages[-1])
        reopen.assert_not_called()

    def test_missing_response_editor_obeys_generation_deadline(self):
        class Clock:
            now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        clock = Clock()
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(sink, "editor_state", return_value=("missing", False)), patch.object(
            wake_bridge, "RESPONSE_GENERATION_WINDOW_SECONDS", 3
        ), patch.object(wake_bridge, "RESPONSE_TOTAL_WINDOW_SECONDS", 10):
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "RESPONSE_COMPLETION_UNPROVEN"):
                sink.wait_for_response_completion(Cdp(), wake_bridge.digest("exact"), monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual(3, clock.now)

    def test_response_timeout_retries_existing_turn_in_place_before_any_reload(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        expected = wake_bridge.digest("exact")
        clock, stages, actions = Clock(), [], []
        snapshots = iter([
            (1, 1, False, False),
            (0, 0, False, False),
            (0, 0, True, True),
        ])
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "editor_state", return_value=("ready", True)
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 1)
        ), patch.object(
            sink, "response_snapshot", side_effect=lambda _cdp, _message: next(snapshots)
        ), patch.object(
            sink, "reopen_response_target"
        ) as reopen, patch.object(
            sink, "click_response_timeout_retry", side_effect=lambda *_args, **_kwargs: actions.append("retry")
        ):
            sink.wait_for_response_completion(
                Cdp(),
                expected,
                observer=stages.append,
                monotonic=clock.monotonic,
                sleeper=clock.sleep,
            )
        reopen.assert_not_called()
        self.assertEqual(["retry"], actions)
        self.assertEqual(
            [
                "GENERATION_WAIT",
                "RESPONSE_TIMEOUT_DETECTED",
                "RESPONSE_RETRYING",
                "RESPONSE_RETRY_STARTED",
                "RESPONSE_COMPLETED",
            ],
            stages,
        )

    def test_response_timeout_retry_does_not_reload_away_transient_retry_control(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        expected = wake_bridge.digest("exact")
        clock, actions = Clock(), []
        snapshots = iter([(1, 1, False, False), (0, 0, False, False), (0, 0, True, True)])
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "editor_state", return_value=("ready", True)
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 1)
        ), patch.object(
            sink, "response_snapshot", side_effect=lambda _cdp, _message: next(snapshots)
        ), patch.object(
            sink, "reopen_response_target"
        ) as reopen, patch.object(
            sink, "click_response_timeout_retry", side_effect=lambda *_args, **_kwargs: actions.append("retry")
        ):
            sink.wait_for_response_completion(
                Cdp(),
                expected,
                monotonic=clock.monotonic,
                sleeper=clock.sleep,
            )
        reopen.assert_not_called()
        self.assertEqual(["retry"], actions)

    def test_response_completion_refuses_sequence_drift_before_retry(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        expected = wake_bridge.digest("exact")
        other = wake_bridge.digest("other")
        clock = Clock()
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "editor_state", return_value=("ready", True)
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(0, 0)
        ), patch.object(
            sink, "click_response_timeout_retry"
        ) as retry:
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "RESPONSE_SEQUENCE_DRIFT"):
                sink.wait_for_response_completion(
                    Cdp(),
                    expected,
                    monotonic=clock.monotonic,
                    sleeper=clock.sleep,
                )
        retry.assert_not_called()

    def test_response_timeout_retry_control_is_fail_closed_when_ambiguous(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        expected = wake_bridge.digest("exact")
        clock = Clock()
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "editor_state", return_value=("ready", True)
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 1)
        ), patch.object(
            sink, "response_snapshot", return_value=(1, 2, False, False)
        ), patch.object(
            sink, "click_response_timeout_retry"
        ) as retry:
            with self.assertRaisesRegex(
                wake_bridge.PostSubmitUnknown, "RESPONSE_TIMEOUT_RETRY_AMBIGUOUS"
            ):
                sink.wait_for_response_completion(
                    Cdp(),
                    expected,
                    monotonic=clock.monotonic,
                    sleeper=clock.sleep,
                )
        retry.assert_not_called()

    def test_response_retry_start_cannot_extend_total_deadline(self):
        class Clock:
            now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        clock = Clock()
        expected = wake_bridge.digest("exact")
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(sink, "editor_state", return_value=("ready", True)), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 1)
        ), patch.object(sink, "response_snapshot", return_value=(1, 1, False, False)), patch.object(
            sink, "click_response_timeout_retry"
        ) as retry, patch.object(wake_bridge, "RESPONSE_TOTAL_WINDOW_SECONDS", 2), patch.object(
            wake_bridge, "RESPONSE_GENERATION_WINDOW_SECONDS", 10
        ):
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "RESPONSE_TOTAL_TIMEOUT"):
                sink.wait_for_response_completion(Cdp(), expected, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual(2, clock.now)
        retry.assert_called_once()

    def test_tool_pause_without_completion_controls_waits_for_resumed_generation(self):
        class Clock:
            now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds): self.now += seconds
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        clock, stages = Clock(), []
        expected = wake_bridge.digest("exact")
        with patch.object(
            sink,
            "post_submit_visible_any",
            side_effect=lambda _cdp, selectors: selectors == wake_bridge.STOP and 30 <= clock.now < 35,
        ), patch.object(sink, "editor_state", return_value=("ready", True)), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 1)
        ), patch.object(sink, "response_snapshot", side_effect=lambda _cdp, _message: (0, 0, True, clock.now >= 35)):
            sink.wait_for_response_completion(Cdp(), expected, observer=stages.append,
                monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertGreaterEqual(clock.now, 35)
        self.assertIn("GENERATION_ACTIVE", stages)
        self.assertEqual("RESPONSE_COMPLETED", stages[-1])
        self.assertEqual(1, stages.count("RESPONSE_COMPLETED"))

    def test_response_timeout_retry_budget_is_bounded(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        expected = wake_bridge.digest("exact")
        clock, clicks = Clock(), []
        snapshots = iter([
            (1, 1, False, False), (0, 0, False, False),
            (1, 1, False, False), (0, 0, False, False),
            (1, 1, False, False), (0, 0, False, False),
            (1, 1, False, False),
        ])
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "editor_state", return_value=("ready", True)
        ), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(1, 1)
        ), patch.object(
            sink, "response_snapshot", side_effect=lambda _cdp, _message: next(snapshots)
        ), patch.object(
            sink, "reopen_response_target"
        ) as reopen, patch.object(
            sink, "click_response_timeout_retry", side_effect=lambda _cdp, _message: clicks.append("retry")
        ):
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "RESPONSE_RETRY_EXHAUSTED"):
                sink.wait_for_response_completion(
                    Cdp(),
                    expected,
                    monotonic=clock.monotonic,
                    sleeper=clock.sleep,
                )
        reopen.assert_not_called()
        self.assertEqual(["retry", "retry", "retry"], clicks)

    def test_submitting_reconciliation_requires_two_exact_persisted_observations_and_never_submits(self):
        class Clock:
            def __init__(self): self.now, self.sleeps = 0.0, []
            def monotonic(self): return self.now
            def sleep(self, value): self.sleeps.append(value); self.now += value
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)
        expected = wake_bridge.digest("exact")
        clock = Clock()
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "trusted_turn_anchor_state", side_effect=[(1, 1), (1, 1)]
        ):
            receipt = sink.reconcile_persisted_receipt(
                Cdp(),
                expected,
                "exact",
                self.record_id,
                wake_bridge.digest("https://chatgpt.com/c/review-one"),
                monotonic=clock.monotonic,
                sleeper=clock.sleep,
                wall_time=lambda: 9.0,
            )
        self.assertEqual(expected, receipt.message_sha256)
        self.assertEqual(9.0, receipt.browser_sent_at_unix)
        self.assertEqual([0.25], clock.sleeps)

    def test_submitting_reconciliation_rejects_duplicate_or_target_drift(self):
        class Cdp:
            def __init__(self, url="https://chatgpt.com/c/review-one"): self.url = url
            def get_current_url(self): return self.url
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)
        expected = wake_bridge.digest("exact")
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "trusted_turn_anchor_state", return_value=(2, 0)
        ):
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "SUBMIT_RECEIPT_SEQUENCE_DRIFT"):
                sink.reconcile_persisted_receipt(
                    Cdp(), expected, "exact", self.record_id, wake_bridge.digest(sink.url)
                )
        with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "SUBMIT_TARGET_DRIFT"):
            sink.reconcile_persisted_receipt(
                Cdp("https://chatgpt.com/c/other"),
                expected,
                "exact",
                self.record_id,
                wake_bridge.digest(sink.url),
            )

    def test_production_post_submit_rejects_unstable_final_digest_target_or_composer(self):
        class Clock:
            def __init__(self): self.now = 0.0
            def monotonic(self): return self.now
            def sleep(self, value): self.now += value
        class Cdp:
            def __init__(self, url="https://chatgpt.com/c/review-one"): self.url = url
            def get_current_url(self): return self.url
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, .5)
        prior, expected = 0, wake_bridge.digest("exact")
        remounted = wake_bridge.digest("remounted")
        clock = Clock()
        with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
            sink, "post_submit_composer_text", return_value=""
        ), patch.object(
            sink, "trusted_turn_anchor_state", side_effect=lambda _cdp, _message: (
                (1, 1) if clock.now == 0 else (0, 0)
            )
        ):
            with self.assertRaises(wake_bridge.PostSubmitUnknown):
                sink.confirm_exact_receipt(Cdp(), prior, expected, self.record_id, "exact", monotonic=clock.monotonic, sleeper=clock.sleep)
        with patch.object(sink, "post_submit_visible_any", return_value=False):
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "SUBMIT_TARGET_DRIFT"):
                sink.confirm_exact_receipt(Cdp("https://chatgpt.com/c/other"), prior, expected, self.record_id, "exact")
        for composer, stage in (("exact", "SUBMIT_COMPOSER_RETAINED"), ("changed", "SUBMIT_COMPOSER_CHANGED")):
            clock = Clock()
            with patch.object(sink, "post_submit_visible_any", return_value=False), patch.object(
                sink, "post_submit_composer_text", return_value=composer
            ):
                with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, stage):
                    sink.confirm_exact_receipt(Cdp(), prior, expected, self.record_id, "exact", monotonic=clock.monotonic, sleeper=clock.sleep)

    def test_missing_receipt_after_stop_hold_keeps_polling_without_busy_loop(self):
        class Clock:
            now = 0.0
            def monotonic(self): return self.now
            def sleep(self, seconds):
                assert seconds > 0, "expired Stop hold must not busy-spin"
                self.now += seconds
        clock = Clock()
        class Cdp:
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.object(
            sink,
            "post_submit_visible_any",
            side_effect=lambda _cdp, selectors: selectors == wake_bridge.STOP and clock.now < .25,
        ), patch.object(
            sink, "post_submit_composer_text", return_value=""
        ), patch.object(sink, "trusted_turn_anchor_state", return_value=(0, 0)):
            with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "SUBMIT_CLEARED_NO_APPEND"):
                sink.confirm_exact_receipt(Cdp(), 0, wake_bridge.digest("exact"), self.record_id, "exact", monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual(wake_bridge.POST_SUBMIT_RECEIPT_WINDOW_SECONDS, clock.now)

    def test_post_submit_stage_taxonomy_is_non_secret_and_complete(self):
        self.assertEqual(
            {
                "SUBMIT_CLICK_UNKNOWN",
                "SUBMIT_ENTER_UNKNOWN",
                "SUBMIT_COMPOSER_RETAINED",
                "SUBMIT_COMPOSER_CHANGED",
                "SUBMIT_TARGET_DRIFT",
                "SUBMIT_TARGET_DRIFT_CHROME_ERROR",
                "SUBMIT_TARGET_DRIFT_OTHER_HOST",
                "SUBMIT_TARGET_DRIFT_USERINFO",
                "SUBMIT_TARGET_DRIFT_PORT",
                "SUBMIT_TARGET_DRIFT_AUTH",
                "SUBMIT_TARGET_DRIFT_HOME",
                "SUBMIT_TARGET_DRIFT_DIFFERENT_CONVERSATION",
                "SUBMIT_TARGET_DRIFT_SAME_QUERY_FRAGMENT",
                "SUBMIT_TARGET_DRIFT_SAME_QUERY",
                "SUBMIT_TARGET_DRIFT_SAME_FRAGMENT",
                "SUBMIT_TARGET_DRIFT_SAME_TRAILING_SLASH",
                "SUBMIT_TARGET_DRIFT_SAME_ROUTE_OTHER",
                "SUBMIT_TARGET_DRIFT_PROJECT_OTHER",
                "SUBMIT_TARGET_DRIFT_OTHER_ROUTE",
                "SUBMIT_TARGET_DRIFT_INVALID_ROUTE",
                "SUBMIT_CLEARED_NO_APPEND",
                "SUBMIT_APPEND_DIGEST_MISMATCH",
                "SUBMIT_RECEIPT_SEQUENCE_DRIFT",
                "SUBMIT_RECEIPT_QUERY_FAILED",
                "SUBMIT_RECEIPT_ROUND_TRIP_FAILED",
                "SUBMIT_RECEIPT_UNPROVEN",
                "RESPONSE_STATE_QUERY_FAILED",
                "RESPONSE_RETRY_CLICK_FAILED",
                "RESPONSE_RETRY_CONTROL_AMBIGUOUS",
                "RESPONSE_REOPEN_UNAVAILABLE",
                "RESPONSE_REOPEN_FAILED",
                "RESPONSE_SEQUENCE_QUERY_FAILED",
                "RESPONSE_SEQUENCE_DRIFT",
                "RESPONSE_TARGET_DRIFT",
                "RESPONSE_NETWORK_ERROR",
                "RESPONSE_GENERATION_TIMEOUT",
                "RESPONSE_COMPLETION_UNPROVEN",
                "RESPONSE_TIMEOUT_RETRY_AMBIGUOUS",
                "RESPONSE_RETRY_EXHAUSTED",
                "RESPONSE_TIMEOUT_STATE_LOST",
                "RESPONSE_RETRY_NOT_STARTED",
                "RESPONSE_TOTAL_TIMEOUT",
            },
            wake_bridge.POST_SUBMIT_DIAGNOSTICS,
        )

    def test_browser_presentation_is_one_shot_and_precedes_typing_and_submit_boundary(self):
        message, timeline = wake_bridge.MESSAGE.format(record_id=self.record_id), []

        class Browser: _process_pid = 4242
        class Driver: cdp_base = Browser()
        class Cdp:
            def __init__(self):
                self.composer, self.messages, self.driver = "", ["prior message"], Driver()
            def is_element_visible(self, selector): return selector == wake_bridge.EDITOR
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def get_attribute(self, _selector, _name, timeout): return self.composer
            def get_text(self, _selector, timeout): return self.composer
            def maximize(self): timeline.append("maximize")
            def bring_active_window_to_front(self): timeline.append("foreground")
            def press_keys(self, _selector, text, timeout):
                timeline.append("type" if text != "\n" else "submit")
                if text == "\n": self.composer = ""; self.messages.append(message)
                else: self.composer = text
            def evaluate(self, script):
                if "#prompt-textarea" in script: return ["ready", True]
                if "data-message-author-role" in script: return list(self.messages)
                if "const unique" in script: return ["missing", None]
                raise AssertionError("unexpected fixed CDP evaluation")

        cdp = Cdp()
        class SB:
            def __init__(self, **_kwargs): self.cdp = cdp
            def __enter__(self): return self
            def __exit__(self, *_args): return False
            def activate_cdp_mode(self, _url): return None

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            wake_bridge.CdpSink, "best_effort_windows_foreground", side_effect=lambda _pid: timeline.append("native")
        ), patch.object(
            sink, "wait_for_submission_accepted", return_value=99.0
        ), patch.object(
            sink, "durable_receipt_round_trip", side_effect=lambda _cdp: timeline.append("reload")
        ), patch.object(
            sink, "wait_for_response_completion", return_value=None
        ):
            sink.wake(self.record_id, message, lambda: True, lambda: timeline.append("boundary"))
        self.assertEqual(["maximize", "foreground", "native", "type", "boundary", "submit", "reload"], timeline)

    def test_windows_foreground_requires_exact_current_cdp_pid_and_one_suitable_window(self):
        class Browser: _process_pid = 4242
        class Driver: cdp_base = Browser()
        class Cdp: driver = Driver()
        class Win32:
            def __init__(self, windows, owners): self.windows, self.owners, self.calls = windows, owners, []
            def enum_windows(self, callback):
                for hwnd in self.windows: callback(hwnd)
            def is_suitable_top_level_window(self, _hwnd): return True
            def is_window_visible(self, _hwnd): return True
            def window_process_id(self, hwnd): return self.owners[hwnd]
            def show_maximized(self, hwnd): self.calls.append(("maximize", hwnd)); return True
            def bring_to_top(self, hwnd): self.calls.append(("top", hwnd)); return True
            def set_foreground(self, hwnd): self.calls.append(("foreground", hwnd)); return True

        self.assertEqual(4242, wake_bridge.CdpSink.selenium_browser_pid(Cdp()))
        unique = Win32([11], {11: 4242})
        wake_bridge.CdpSink.best_effort_windows_foreground(4242, platform_name="nt", win32=unique)
        self.assertEqual([("maximize", 11), ("top", 11), ("foreground", 11)], unique.calls)
        for windows, owners in (([], {}), ([11], {11: 7}), ([11, 12], {11: 4242, 12: 4242})):
            api = Win32(windows, owners)
            wake_bridge.CdpSink.best_effort_windows_foreground(4242, platform_name="nt", win32=api)
            self.assertEqual([], api.calls)
        self.assertIsNone(wake_bridge.CdpSink.selenium_browser_pid(object()))
        invalid = Win32([11], {11: 4242})
        wake_bridge.CdpSink.best_effort_windows_foreground(0, platform_name="nt", win32=invalid)
        self.assertEqual([], invalid.calls)

    def test_windows_foreground_is_nonfatal_for_false_errors_and_non_windows(self):
        class Win32:
            def __init__(self): self.calls = []
            def enum_windows(self, callback): callback(11)
            def is_suitable_top_level_window(self, _hwnd): return True
            def is_window_visible(self, _hwnd): return True
            def window_process_id(self, _hwnd): return 4242
            def show_maximized(self, hwnd): self.calls.append(("maximize", hwnd)); raise RuntimeError("policy")
            def bring_to_top(self, hwnd): self.calls.append(("top", hwnd)); return False
            def set_foreground(self, hwnd): self.calls.append(("foreground", hwnd)); return False
        api = Win32()
        wake_bridge.CdpSink.best_effort_windows_foreground(4242, platform_name="nt", win32=api)
        self.assertEqual([("maximize", 11), ("top", 11), ("foreground", 11)], api.calls)
        api = Win32()
        wake_bridge.CdpSink.best_effort_windows_foreground(4242, platform_name="posix", win32=api)
        self.assertEqual([], api.calls)

    def test_browser_presentation_is_nonfatal_but_presentation_drift_fails_before_typing(self):
        message, timeline = wake_bridge.MESSAGE.format(record_id=self.record_id), []

        class Cdp:
            def __init__(self, drift=False):
                self.composer, self.messages, self.url, self.drift = "", ["prior message"], "https://chatgpt.com/c/review-one", drift
            def is_element_visible(self, selector): return selector == wake_bridge.EDITOR
            def get_current_url(self): return self.url
            def get_attribute(self, _selector, _name, timeout): return self.composer
            def get_text(self, _selector, timeout): return self.composer
            def maximize(self):
                timeline.append("maximize")
                if self.drift: self.url = "https://chatgpt.com/c/other"
                raise RuntimeError("foreground policy")
            def bring_active_window_to_front(self): timeline.append("foreground"); return False
            def press_keys(self, _selector, text, timeout): timeline.append("type"); self.composer = text
            def evaluate(self, script):
                if "#prompt-textarea" in script: return ["ready", True]
                if "data-message-author-role" in script: return list(self.messages)
                if "const unique" in script: return ["missing", None]
                raise AssertionError("unexpected fixed CDP evaluation")

        def run(cdp):
            class SB:
                def __init__(self, **_kwargs): self.cdp = cdp
                def __enter__(self): return self
                def __exit__(self, *_args): return False
                def activate_cdp_mode(self, _url): return None
            sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
            with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
                sink, "durable_receipt_round_trip", side_effect=lambda _cdp: timeline.append("reload")
            ), patch.object(
                sink, "wait_for_response_completion", return_value=None
            ):
                return sink.wake(self.record_id, message, lambda: True, lambda: timeline.append("boundary"))

        with self.assertRaisesRegex(wake_bridge.Attention, "EDITOR_SELECTOR"):
            run(Cdp(drift=True))
        self.assertEqual(["maximize", "foreground"], timeline)
        timeline.clear()

        class DeliveredCdp(Cdp):
            def press_keys(self, _selector, text, timeout):
                timeline.append("type" if text != "\n" else "submit")
                if text == "\n": self.composer = ""; self.messages.append(message)
                else: self.composer = text
        run(DeliveredCdp())
        self.assertEqual(["maximize", "foreground", "type", "boundary", "submit", "reload"], timeline)

    def test_presentation_is_skipped_for_non_actionable_draft_or_existing_digest(self):
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        message, presented = wake_bridge.MESSAGE.format(record_id=self.record_id), []
        class Cdp:
            def __init__(self, composer="", messages=None):
                self.composer, self.messages = composer, messages or ["prior"]
                self.driver = types.SimpleNamespace(cdp_base=types.SimpleNamespace(_process_pid=4242))
            def is_element_visible(self, selector): return selector == wake_bridge.EDITOR
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def get_attribute(self, *_args, **_kwargs): return self.composer
            def get_text(self, *_args, **_kwargs): return self.composer
            def evaluate(self, script):
                if "#prompt-textarea" in script: return ["ready", self.composer == ""]
                if "data-message-author-role" in script: return list(self.messages)
                raise AssertionError("unexpected fixed CDP evaluation")
            def maximize(self): presented.append("maximize")
            def bring_active_window_to_front(self): presented.append("foreground")
        class SB:
            def __init__(self, cdp, **_kwargs): self.cdp = cdp
            def __enter__(self): return self
            def __exit__(self, *_args): return False
            def activate_cdp_mode(self, _url): return None
        for cdp, actionable in ((Cdp("draft"), lambda: True), (Cdp(), lambda: False), (Cdp(messages=[message]), lambda: True)):
            with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=lambda **kwargs: SB(cdp, **kwargs))}), patch.object(
                wake_bridge.CdpSink, "best_effort_windows_foreground", side_effect=lambda _pid: presented.append("native")
            ):
                with self.assertRaises(wake_bridge.Attention):
                    sink.wake(self.record_id, message, actionable, lambda: self.fail("submission boundary"))
        self.assertEqual([], presented)

    def test_login_captcha_or_unrecoverable_network_never_presents_browser(self):
        presented = []
        class Cdp:
            def __init__(self, mode): self.mode = mode
            def get_current_url(self):
                return {
                    "login": "https://chatgpt.com/auth/login",
                    "captcha": "https://chatgpt.com/c/review-one",
                    "network": "chrome-error://chromewebdata/",
                }[self.mode]
            def is_element_visible(self, selector):
                return self.mode == "captcha" and selector == wake_bridge.CAPTCHA[0]
            def reload(self, ignore_cache): pass
            def maximize(self): presented.append("maximize")
            def bring_active_window_to_front(self): presented.append("foreground")
        for mode in ("login", "captcha", "network"):
            cdp = Cdp(mode)
            class SB:
                def __init__(self, **_kwargs): self.cdp = cdp
                def __enter__(self): return self
                def __exit__(self, *_args): return False
                def activate_cdp_mode(self, _url): return None
            sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
            with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(wake_bridge.time, "sleep"):
                with self.assertRaises(wake_bridge.Attention):
                    sink.wake(self.record_id, wake_bridge.MESSAGE.format(record_id=self.record_id), lambda: True, lambda: self.fail("submission boundary"))
        self.assertEqual([], presented)

    def test_network_reload_does_not_repeat_presentation(self):
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        presentations = []
        with patch.object(sink, "pre_typing_ready"), patch.object(
            sink, "wait_for_page_readiness"
        ), patch.object(sink, "pre_submit_receipt_snapshot", return_value=0), patch.object(sink, "best_effort_present_browser", side_effect=lambda _cdp: presentations.append("present")), patch.object(
            sink, "wait_for_send_control", return_value=None
        ), patch.object(sink, "smoke_path_submit"), patch.object(
            sink, "wait_for_submission_accepted", return_value=99.0
        ), patch.object(
            sink, "durable_receipt_round_trip", return_value=None
        ), patch.object(sink, "confirm_exact_receipt", return_value=object()), patch.object(
            sink, "wait_for_response_completion", return_value=None
        ):
            class ReloadingCdp:
                def reload(self, ignore_cache): pass
                def get_current_url(self): return "https://chatgpt.com/c/review-one"
                def press_keys(self, *_args, **_kwargs): pass
            real_cdp = ReloadingCdp()
            class SB:
                def __init__(self, **_kwargs): self.cdp = real_cdp
                def __enter__(self): return self
                def __exit__(self, *_args): return False
                def activate_cdp_mode(self, _url): return None
            with patch.object(sink, "text", return_value=wake_bridge.MESSAGE.format(record_id=self.record_id)), patch.dict(
                sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}
            ):
                sink.wait_for_page_readiness.return_value = real_cdp
                sink.wake(self.record_id, wake_bridge.MESSAGE.format(record_id=self.record_id), lambda: True, lambda: None)
        self.assertEqual(["present"], presentations)

    def test_cdp_sink_valid_receipt_uses_smoke_compatible_direct_click(self):
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)
        events = []

        class Cdp:
            def __init__(self):
                self.composer = ""
                self.messages = ["prior message"]
                self.clicks = []
            def is_element_visible(self, selector):
                return selector == wake_bridge.EDITOR
            def get_current_url(self):
                return "https://chatgpt.com/c/review-one"
            def get_attribute(self, _selector, _name, timeout):
                return self.composer
            def get_text(self, _selector, timeout):
                return self.composer
            def press_keys(self, _selector, text, timeout):
                self.composer = text
            def evaluate(self, script):
                if "#prompt-textarea" in script:
                    return ["ready", True]
                if "const expected =" in script and "conversation-turn-" in script:
                    expected_text = wake_bridge.normalize_message(message)
                    matches = sum(
                        expected_text in wake_bridge.normalize_message(item)
                        for item in self.messages
                    )
                    return ["ready", matches, 0]
                if "const expected =" in script and "conversation-turn-" in script:
                    expected_text = wake_bridge.normalize_message(message)
                    matches = sum(
                        expected_text in wake_bridge.normalize_message(item)
                        for item in self.messages
                    )
                    return ["ready", matches, 0]
                if "data-message-author-role" in script:
                    return list(self.messages)
                if "const unique" in script:
                    return ["ready", "button#composer-submit-button"]
                raise AssertionError("unexpected fixed CDP evaluation")
            def click(self, selector, timeout):
                self.clicks.append((selector, timeout))
                self.composer = ""
                self.messages.append(message)

        cdp = Cdp()
        class SB:
            def __init__(self, **_kwargs): self.cdp = cdp
            def __enter__(self): return self
            def __exit__(self, *_args): return False
            def activate_cdp_mode(self, _url): return None

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            sink, "wait_for_submission_accepted", return_value=99.0
        ), patch.object(
            sink, "wait_for_response_completion", return_value=None
        ), patch.object(
            sink, "durable_receipt_round_trip", return_value=None
        ), patch.object(
            sink, "post_submit_visible_any", return_value=False
        ), patch.object(
            sink, "post_submit_composer_text", return_value=""
        ):
            receipt = sink.wake(self.record_id, message, lambda: True, lambda: events.append("boundary"))
        self.assertEqual(["boundary"], events)
        self.assertEqual([("button#composer-submit-button", 1)], cdp.clicks)
        self.assertEqual(wake_bridge.digest(wake_bridge.normalize_message(message)), receipt.message_sha256)

    def test_cdp_sink_propagates_pre_submit_attention_without_seleniumbase_test_mode(self):
        events, sb_kwargs = [], []
        class Cdp:
            def is_element_visible(self, selector): return selector == wake_bridge.EDITOR
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def get_attribute(self, *_args, **_kwargs): return ""
            def get_text(self, *_args, **_kwargs): return ""
            def evaluate(self, script):
                if "#prompt-textarea" in script:
                    return ["ready", True]
                return {"unexpected": "shape"}
        class SB:
            def __init__(self, **kwargs): self.cdp, self.kwargs = Cdp(), kwargs; sb_kwargs.append(kwargs)
            def __enter__(self): return self
            def __exit__(self, exc_type, *_args): return self.kwargs.get("test") is True and exc_type is not None
            def activate_cdp_mode(self, _url): return None
        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}):
            with self.assertRaisesRegex(wake_bridge.Attention, "USER_MESSAGE_RECEIPT"):
                sink.wake(self.record_id, message, lambda: True, lambda: events.append("boundary"))
        self.assertEqual([], events)
        self.assertEqual(1, len(sb_kwargs))
        self.assertNotIn("test", sb_kwargs[0])

    def test_cdp_sink_uses_native_enter_only_after_exact_typed_message_when_button_missing(self):
        message = wake_bridge.MESSAGE.format(record_id=self.record_id)
        events = []

        class Cdp:
            def __init__(self):
                self.composer = ""
                self.messages = ["prior message"]
                self.keys = []
            def is_element_visible(self, selector): return selector == wake_bridge.EDITOR
            def get_current_url(self): return "https://chatgpt.com/c/review-one"
            def get_attribute(self, _selector, _name, timeout): return self.composer
            def get_text(self, _selector, timeout): return self.composer
            def press_keys(self, selector, text, timeout):
                self.keys.append((selector, text, timeout))
                if text == "\n":
                    self.composer = ""
                    self.messages.append(message)
                else:
                    self.composer = text
            def evaluate(self, script):
                if "#prompt-textarea" in script: return ["ready", True]
                if "data-message-author-role" in script: return list(self.messages)
                if "const unique" in script: return ["missing", None]
                raise AssertionError("unexpected fixed CDP evaluation")

        cdp = Cdp()
        class SB:
            def __init__(self, **_kwargs): self.cdp = cdp
            def __enter__(self): return self
            def __exit__(self, *_args): return False
            def activate_cdp_mode(self, _url): return None

        sink = wake_bridge.CdpSink("https://chatgpt.com/c/review-one", self.profile, 1, 1)
        with patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}), patch.object(
            sink, "wait_for_submission_accepted", return_value=99.0
        ), patch.object(
            sink, "durable_receipt_round_trip", return_value=None
        ), patch.object(
            sink, "wait_for_response_completion", return_value=None
        ):
            receipt = sink.wake(self.record_id, message, lambda: True, lambda: events.append("boundary"))
        self.assertEqual(["boundary"], events)
        self.assertEqual([(wake_bridge.EDITOR, message, 1), (wake_bridge.EDITOR, "\n", 1)], cdp.keys)
        self.assertEqual(wake_bridge.digest(wake_bridge.normalize_message(message)), receipt.message_sha256)

    def test_bridge_send_selector_parity_with_proven_smoke_paths(self):
        cdp_smoke = (Path(__file__).parents[1] / "scripts" / "wake_browser_smoke_test_cdp.py").read_text(encoding="utf-8")
        webdriver_smoke = (Path(__file__).parents[1] / "scripts" / "wake_browser_smoke_test.py").read_text(encoding="utf-8")
        self.assertIn("button#composer-submit-button", wake_bridge.SEND)
        self.assertIn("button#composer-submit-button", cdp_smoke)
        self.assertIn("button#composer-submit-button", webdriver_smoke)
        self.assertIn("cdp.click(send_selector", cdp_smoke)
        self.assertIn("NO_SAFE_SEND_BUTTON:USING_NATIVE_ENTER", cdp_smoke)

    def test_copied_bridge_path_or_revision_is_rejected_before_sink_construction(self):
        copied = self.root / "copied-wake_bridge.py"
        copied.write_text(self.bridge_path.read_text(encoding="utf-8"), encoding="utf-8")
        self.assertEqual(2, self.run_main(bridge_path=copied))
        self.assertEqual([], self.calls)
        self.assertFalse(self.state_path.exists())
        self.assertEqual(2, self.run_main(bridge_sha256="0" * 64))
        self.assertEqual([], self.calls)

    def test_legacy_production_identifiers_are_absent(self):
        source = (Path(__file__).parents[1] / "scripts" / "wake_bridge.py").read_text(
            encoding="utf-8"
        )
        for forbidden in (
            "MCP_READY",
            "mcp-ready",
            "retired_record_ids",
            "retire_backlog",
            "debounce_seconds",
            "_raw_driver",
            "_prepare_composer",
            "_type_with_random_delay",
        ):
            self.assertNotIn(forbidden, source)

    def test_state_claim_is_explicit_and_bounded(self):
        state = wake_bridge.State(
            deliveries=[wake_bridge.Delivery(self.record_id, "CLAIMED", 1)]
        )
        self.assertEqual("CLAIMED", state.deliveries[0].status)
        self.assertEqual(4, state.schema_version)

    def test_sent_record_is_idempotent_and_never_calls_sink_again(self):
        self.write_state("SENT", valid_receipt=True)
        self.assertEqual(0, self.run_main())
        self.assertEqual([], self.calls)
        self.assertEqual("SENT", self.read_state()["deliveries"][0]["status"])

    def test_legacy_sent_without_receipt_is_unproven_and_never_calls_sink(self):
        self.write_state("SENT")
        self.assertEqual(2, self.run_main())
        self.assertEqual([], self.calls)
        self.assertEqual("OPERATOR_ATTENTION", self.read_state()["deliveries"][0]["status"])

    def test_typed_but_unsent_never_becomes_sent(self):
        self.assertEqual(2, self.run_main("typed-unsent"))
        self.assertEqual("OPERATOR_ATTENTION", self.read_state()["deliveries"][0]["status"])
        self.assertNotEqual("SENT", self.read_state()["deliveries"][0]["status"])

    def test_latest_idle_composer_without_receipt_keeps_submitting_and_never_blindly_retries(self):
        self.assertEqual(2, self.run_main("empty-no-receipt"))
        state = self.read_state()
        self.assertEqual("SUBMITTING", state["deliveries"][0]["status"])
        self.assertEqual("SUBMIT_RECEIPT_UNPROVEN", state["operator_attention"])
        self.assertIsNone(state["deliveries"][0]["browser_sent_at_unix"])
        self.assertIsNone(state["deliveries"][0]["message_sha256"])
        self.assertIsNone(state["deliveries"][0]["target_sha256"])
        self.assertEqual(2, self.run_main())
        self.assertEqual(1, len(self.calls))

    def test_pre_submit_missing_receipt_is_operator_attention_not_submitting(self):
        self.assertEqual(2, self.run_main("malformed-receipt"))
        state = self.read_state()
        self.assertEqual("OPERATOR_ATTENTION", state["deliveries"][0]["status"])
        self.assertEqual("PRE_SUBMIT_NO_RECEIPT", state["deliveries"][0]["attention"])

    def test_post_submit_missing_receipt_remains_submitting_without_duplicate_retry(self):
        self.assertEqual(2, self.run_main("malformed-receipt-after-boundary"))
        state = self.read_state()
        self.assertEqual("SUBMITTING", state["deliveries"][0]["status"])
        self.assertEqual("SUBMIT_RECEIPT_UNPROVEN", state["deliveries"][0]["attention"])
        self.assertEqual(2, self.run_main())
        self.assertEqual(1, len(self.calls))

    def test_post_submit_receipt_query_failure_remains_submitting(self):
        self.assertEqual(2, self.run_main("post-submit-query-failure"))
        state = self.read_state()
        self.assertEqual("SUBMITTING", state["deliveries"][0]["status"])
        self.assertEqual("SUBMIT_RECEIPT_QUERY_FAILED", state["deliveries"][0]["attention"])

    def test_pre_submit_post_submit_unknown_cannot_cross_submission_boundary(self):
        class PreSubmitUnknownSink(FakeSink):
            def wake(self, *_args):
                raise wake_bridge.PostSubmitUnknown("SUBMIT_RECEIPT_QUERY_FAILED")
        def sink_factory(target_url, *_args, **_kwargs):
            return PreSubmitUnknownSink("unused", self.calls, target_url)
        argv = ["wake_bridge.py", "--workspace", str(self.root), "--config", str(self.config), "--record-id", self.record_id, "--bridge-sha256", self.bridge_sha256]
        with patch.object(wake_bridge, "CdpSink", side_effect=sink_factory), patch.object(wake_bridge, "__file__", str(self.bridge_path)), patch.object(sys, "argv", argv):
            self.assertEqual(2, wake_bridge.main())
        state = self.read_state()
        self.assertEqual("OPERATOR_ATTENTION", state["deliveries"][0]["status"])
        self.assertEqual("SUBMIT_RECEIPT_QUERY_FAILED", state["deliveries"][0]["attention"])

    def test_claimed_submit_attention_is_reconciled_fail_closed_without_sink_call(self):
        self.write_state("CLAIMED", operator_attention="SUBMIT_RECEIPT_UNPROVEN", delivery_attention="SUBMIT_RECEIPT_UNPROVEN")
        self.assertEqual(2, self.run_main())
        state = self.read_state()
        self.assertEqual("OPERATOR_ATTENTION", state["deliveries"][0]["status"])
        self.assertEqual("INVALID_CLAIMED_POST_SUBMIT_STATE", state["deliveries"][0]["attention"])
        self.assertEqual([], self.calls)

    def test_click_failure_after_submission_boundary_keeps_submitting(self):
        self.assertEqual(2, self.run_main("click-unknown"))
        self.assertEqual("SUBMITTING", self.read_state()["deliveries"][0]["status"])

    def test_post_submit_crash_leaves_durable_unknown_and_never_becomes_sent(self):
        self.assertEqual(2, self.run_main("post-submit-crash"))
        self.assertEqual("SUBMITTING", self.read_state()["deliveries"][0]["status"])
        self.assertEqual("SUBMIT_RECEIPT_UNPROVEN", self.read_state()["deliveries"][0]["attention"])

    def test_unexpected_pre_submit_exception_is_operator_attention(self):
        self.assertEqual(2, self.run_main("pre-submit-crash"))
        state = self.read_state()
        self.assertEqual("OPERATOR_ATTENTION", state["deliveries"][0]["status"])
        self.assertEqual("PRE_SUBMIT_UNEXPECTED", state["deliveries"][0]["attention"])

    def test_stale_claimed_record_is_recovered_and_persisted_sent(self):
        self.write_state("CLAIMED")
        self.assertEqual(0, self.run_main())
        self.assertEqual(1, len(self.calls))
        state = self.read_state()
        self.assertEqual("SENT", state["deliveries"][0]["status"])
        self.assertEqual(123.0, state["deliveries"][0]["browser_sent_at_unix"])

    def test_stale_submitting_record_fails_closed_without_sink_call(self):
        self.write_state("SUBMITTING")
        self.assertEqual(2, self.run_main())
        self.assertEqual([], self.calls)
        self.assertEqual("SUBMITTING", self.read_state()["deliveries"][0]["status"])

    def test_pre_submit_cancellation_is_terminal_and_leaves_safe_claim(self):
        self.assertEqual(4, self.run_main("cancel"))
        self.assertEqual(1, len(self.calls))
        self.assertEqual("CLAIMED", self.read_state()["deliveries"][0]["status"])

    def test_operator_attention_is_persisted_then_cleared_after_successful_manual_retry(self):
        self.assertEqual(2, self.run_main("attention"))
        state = self.read_state()
        self.assertEqual("LOGIN_OR_PROFILE_REQUIRED", state["operator_attention"])
        self.assertEqual("OPERATOR_ATTENTION", state["deliveries"][0]["status"])

        self.assertEqual(0, self.run_main("success"))
        state = self.read_state()
        self.assertIsNone(state["operator_attention"])
        self.assertEqual("SENT", state["deliveries"][0]["status"])
        self.assertEqual(2, len(self.calls))

    def test_chatgpt_not_idle_is_retryable_and_keeps_clean_pre_submit_claim(self):
        with patch("builtins.print") as output:
            self.assertEqual(3, self.run_main("not-idle"))
        output.assert_any_call("CHATGPT_NOT_IDLE")
        state = self.read_state()
        self.assertIsNone(state["operator_attention"])
        self.assertEqual("CLAIMED", state["deliveries"][0]["status"])
        self.assertIsNone(state["deliveries"][0]["attention"])
        self.assertIsNone(state["deliveries"][0]["browser_sent_at_unix"])
        self.assertIsNone(state["deliveries"][0]["message_sha256"])
        self.assertIsNone(state["deliveries"][0]["target_sha256"])
        self.assertIsNone(state["deliveries"][0]["receipt_schema_version"])
        self.assertEqual(0, self.run_main("success"))
        self.assertEqual("SENT", self.read_state()["deliveries"][0]["status"])
        self.assertEqual(2, len(self.calls))

    def test_busy_singleton_is_retryable_without_sink_call(self):
        with patch("builtins.print") as output:
            self.assertEqual(3, self.run_main(singleton=BusySingleton))
        output.assert_any_call("WAKE_BRIDGE_BUSY")
        self.assertEqual([], self.calls)


class WakeBrowserCleanupContractTests(unittest.TestCase):
    def test_every_wake_attempt_closes_only_its_owned_browser_in_finally(self):
        source = (Path(__file__).parents[1] / "scripts" / "wake_bridge.py").read_text(encoding="utf-8")
        accepted = source.index("accepted_at = self.wait_for_submission_accepted")
        response = source.index("self.wait_for_response_completion", accepted)
        reload = source.index("self.durable_receipt_round_trip", response)
        returned = source.index("return self.confirm_exact_receipt", reload)
        cleanup_finally = source.index("finally:", returned)
        close = source.index("self.close_owned_browser(sb, cdp)", cleanup_finally)
        self.assertLess(accepted, response)
        self.assertLess(response, reload)
        self.assertLess(reload, returned)
        self.assertLess(returned, cleanup_finally)
        self.assertLess(cleanup_finally, close)

        calls = []
        cdp = types.SimpleNamespace(close_active_tab=lambda: calls.append("close-active-tab"))
        sb = types.SimpleNamespace(cdp=cdp)
        wake_bridge.CdpSink.close_owned_browser(sb, cdp)
        self.assertEqual(["close-active-tab"], calls)

    def test_owned_browser_cleanup_never_uses_browser_wide_quit_or_process_kill(self):
        calls = []
        driver = types.SimpleNamespace(quit=lambda: calls.append("driver-quit"))
        sb = types.SimpleNamespace(driver=driver, quit=lambda: calls.append("sb-quit"))
        cdp = types.SimpleNamespace(close_active_tab=lambda: calls.append("close-active-tab"))
        wake_bridge.CdpSink.close_owned_browser(sb, cdp)
        self.assertEqual(["close-active-tab"], calls)

    def test_owned_browser_cleanup_failure_cannot_change_delivery_authority(self):
        class Cdp:
            def close_active_tab(self):
                raise RuntimeError("synthetic cleanup failure")

        wake_bridge.CdpSink.close_owned_browser(types.SimpleNamespace(), Cdp())


if __name__ == "__main__":
    unittest.main()
