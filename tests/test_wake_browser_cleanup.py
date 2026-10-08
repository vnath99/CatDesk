import importlib.util
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "wake_bridge_cleanup_test_subject",
    Path(__file__).parents[1] / "scripts" / "wake_bridge.py",
)
wake_bridge = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = wake_bridge
SPEC.loader.exec_module(wake_bridge)


class WakeBrowserCleanupTests(unittest.TestCase):
    def setUp(self):
        self.profile = tempfile.TemporaryDirectory()
        self.addCleanup(self.profile.cleanup)

    def _run(self, *, receipt_error=None):
        timeline = []
        message = wake_bridge.MESSAGE.format(record_id="cleanup-record")
        receipt = wake_bridge.receipt_for(
            "cleanup-record",
            message,
            "https://chatgpt.com/c/review-one",
            1.0,
        )

        class Driver:
            def quit(self):
                timeline.append("quit")

        class Cdp:
            def press_keys(self, *_args, **_kwargs):
                return None

        cdp = Cdp()

        class SB:
            def __init__(self, **_kwargs):
                self.cdp = cdp
                self.driver = Driver()

            def __enter__(self):
                timeline.append("open")
                return self

            def __exit__(self, *_args):
                timeline.append("exit")
                return False

            def activate_cdp_mode(self, _url):
                return None

        sink = wake_bridge.CdpSink(
            "https://chatgpt.com/c/review-one",
            Path(self.profile.name),
            1,
            1,
        )

        def confirm(*_args, **_kwargs):
            timeline.append("receipt")
            if receipt_error is not None:
                raise receipt_error
            return receipt

        patches = (
            patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=SB)}),
            patch.object(sink, "wait_for_page_readiness", side_effect=lambda _sb, current: current),
            patch.object(sink, "wait_for_idle"),
            patch.object(sink, "pre_typing_ready"),
            patch.object(sink, "pre_submit_receipt_snapshot", return_value=[]),
            patch.object(sink, "best_effort_present_browser"),
            patch.object(sink, "text", return_value=message),
            patch.object(sink, "wait_for_send_control", return_value="send"),
            patch.object(sink, "smoke_path_submit"),
            patch.object(sink, "durable_receipt_round_trip"),
            patch.object(sink, "confirm_exact_receipt", side_effect=confirm),
            patch.object(sink, "wait_for_response_completion", side_effect=lambda *_args: timeline.append("complete")),
            patch.object(sink, "wait_for_submission_accepted", return_value=99.0),
        )
        with patches[0]:
            with patches[1], patches[2], patches[3], patches[4], patches[5], patches[6], patches[7], patches[8], patches[9], patches[10], patches[11], patches[12]:
                result = sink.wake(
                    "cleanup-record",
                    message,
                    lambda: True,
                    lambda: None,
                )
        return result, receipt, timeline

    def test_proven_success_explicitly_quits_owned_browser_before_context_exit(self):
        result, receipt, timeline = self._run()
        self.assertEqual(receipt, result)
        self.assertEqual(["open", "complete", "receipt", "quit", "exit"], timeline)

    def test_unproven_post_submit_failure_does_not_use_success_cleanup_path(self):
        with self.assertRaisesRegex(wake_bridge.PostSubmitUnknown, "UNPROVEN"):
            self._run(receipt_error=wake_bridge.PostSubmitUnknown("UNPROVEN"))


if __name__ == "__main__":
    unittest.main()
