"""Failure matrix for the existing post-refresh exact-exchange observer."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

SPEC = importlib.util.spec_from_file_location('persistence_bridge',
    Path(__file__).parents[1] / 'scripts' / 'wake_bridge.py')
bridge = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bridge
SPEC.loader.exec_module(bridge)


class Clock:
    now = 0.0
    def monotonic(self): return self.now
    def sleep(self, seconds): self.now += seconds


class ResponsePersistenceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.sink = bridge.CdpSink('https://chatgpt.com/c/exact-fixture', Path(self.directory.name), 1, 1)
        self.clock = Clock()

    def wait(self):
        self.sink.wait_for_persisted_response(object(), 'exact message',
            monotonic=self.clock.monotonic, sleeper=self.clock.sleep)

    def test_remounted_body_or_resumed_generation_waits_for_two_complete_reads(self):
        with patch.object(self.sink, 'observe_cooperative_response', side_effect=[
            'RESPONSE_PENDING', 'RESPONSE_COMPLETE', 'RESPONSE_PENDING',
            'RESPONSE_COMPLETE', 'RESPONSE_COMPLETE']) as read:
            self.wait()
        self.assertEqual(read.call_count, 5)
        for call in read.call_args_list:
            self.assertEqual(call.args[1:], ('exact message', bridge.digest('exact message')))

    def test_errors_fail_closed_and_do_not_poison_an_independent_observer(self):
        for reason in ('LOGIN_OR_PROFILE_REQUIRED', 'RESPONSE_NETWORK_ERROR', 'RESPONSE_TARGET_DRIFT',
                       'RESPONSE_EXPECTED_USER_DUPLICATED', 'RESPONSE_SEQUENCE_DRIFT',
                       'RESPONSE_EXPECTED_USER_NOT_FOUND', 'RESPONSE_LOAD_ERROR',
                       'RESPONSE_STATE_QUERY_FAILED'):
            with self.subTest(reason=reason), patch.object(self.sink, 'observe_cooperative_response',
                    side_effect=bridge.Attention(reason)) as read:
                with self.assertRaisesRegex(bridge.Attention, reason): self.wait()
                read.assert_called_once()
            with patch.object(self.sink, 'observe_cooperative_response', return_value='RESPONSE_COMPLETE'):
                self.wait()

    def test_missing_body_or_changed_dom_has_a_fixed_timeout_and_no_ui_actions(self):
        with patch.object(bridge, 'POST_SUBMIT_RECEIPT_WINDOW_SECONDS', 1), patch.object(
                self.sink, 'observe_cooperative_response', return_value='RESPONSE_PENDING'), patch.object(
                self.sink, 'click_response_timeout_retry') as retry, patch.object(
                self.sink, 'durable_receipt_round_trip') as reload:
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, 'RESPONSE_PERSISTENCE_UNPROVEN'): self.wait()
            self.assertEqual(self.clock.now, 1)
            retry.assert_not_called()
            reload.assert_not_called()

    def test_interruption_propagates_without_retry(self):
        with patch.object(self.sink, 'observe_cooperative_response', side_effect=KeyboardInterrupt) as read:
            with self.assertRaises(KeyboardInterrupt): self.wait()
        read.assert_called_once()

    def test_formatted_message_uses_the_original_normalized_user_digest_domain(self):
        message = 'Original task.\n  {"field": "two  spaces"}\n'
        with patch.object(self.sink, 'observe_cooperative_response', return_value='RESPONSE_COMPLETE') as read:
            self.sink.wait_for_persisted_response(object(), message,
                monotonic=self.clock.monotonic, sleeper=self.clock.sleep)
        self.assertEqual(read.call_args.args[1:], (message, bridge.digest(bridge.normalize_message(message))))
        self.assertNotEqual(bridge.digest(message), read.call_args.args[2])

    def test_empty_document_during_hydration_is_pending_but_foreign_messages_are_not(self):
        with patch.object(self.sink, 'observe_cooperative_response', side_effect=[
                bridge.Attention('RESPONSE_USER_DOCUMENT_NOT_HYDRATED'),
                'RESPONSE_COMPLETE', 'RESPONSE_COMPLETE']) as read:
            self.wait()
        self.assertEqual(read.call_count, 3)

    def test_post_completion_refresh_refuses_draft_missing_editor_or_resumed_generation(self):
        for status, empty, busy in (('ready', False, False), ('missing', False, False), ('ready', True, True)):
            with self.subTest(status=status, empty=empty, busy=busy):
                cdp = Mock()
                with patch.object(self.sink, 'require_response_document'), patch.object(
                        self.sink, 'editor_state', return_value=(status, empty)), patch.object(
                        self.sink, 'post_submit_visible_any', return_value=busy):
                    with self.assertRaises(bridge.PostSubmitUnknown):
                        self.sink.durable_receipt_round_trip(cdp, completed_response=True)
                    with self.assertRaises(bridge.PostSubmitUnknown):
                        self.sink.refresh_response_observer(cdp)
                cdp.evaluate.assert_not_called()
                cdp.reload.assert_not_called()


if __name__ == '__main__': unittest.main()
