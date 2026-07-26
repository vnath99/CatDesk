import dataclasses
import io
import json
import tempfile
import unittest
from pathlib import Path

from experimental.advisors.deepseek_web_advisor import (
    AdapterState,
    CompletionDetector,
    DeepSeekWebAdvisorAdapter,
    JsonLinesAdvisorProtocol,
    PageSnapshot,
    SelectorConfig,
)


FIXTURE_ROOT = Path(__file__).resolve().parents[1] / "fixtures" / "deepseek_advisor"
SELECTOR_CONFIG = (
    Path(__file__).resolve().parents[2]
    / "experimental"
    / "advisors"
    / "deepseek_selectors.json"
)


def fixture(name: str) -> str:
    return (FIXTURE_ROOT / name).read_text(encoding="utf-8")


def selectors() -> SelectorConfig:
    return SelectorConfig.from_file(SELECTOR_CONFIG)


class DeepSeekWebAdvisorTests(unittest.TestCase):
    def test_completion_detector_combines_response_stop_send_stability_and_marker(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = PageSnapshot(
            html=fixture("ready_completed.html"),
            title="DeepSeek Chat",
            url="https://chat.deepseek.com/a/chat/synthetic",
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=100.0,
            last_text_change_at=105.0,
            now=108.0,
        )

        self.assertEqual(observation.state, AdapterState.COMPLETED)
        self.assertGreaterEqual(observation.response_count, 1)
        self.assertFalse(observation.stop_visible)
        self.assertTrue(observation.send_visible)
        self.assertTrue(observation.marker_seen)
        self.assertGreaterEqual(observation.stable_for_seconds, 2.0)

    def test_completion_detector_waits_when_stop_button_is_visible(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = PageSnapshot(
            html=fixture("responding.html"),
            title="DeepSeek Chat",
            url="https://chat.deepseek.com/a/chat/synthetic",
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=100.0,
            last_text_change_at=107.5,
            now=108.0,
        )

        self.assertEqual(observation.state, AdapterState.WAITING_FOR_RESPONSE)
        self.assertTrue(observation.stop_visible)

    def test_takeover_required_for_security_verification_fixture(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = PageSnapshot(
            html=fixture("security_challenge.html"),
            title="Security Check",
            url="https://chat.deepseek.com/signin",
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=1.0,
            last_text_change_at=1.0,
            now=2.0,
        )

        self.assertEqual(observation.state, AdapterState.TAKEOVER_REQUIRED)
        self.assertIn("turnstile", observation.diagnostics.failed_selector or "")

    def test_login_required_fixture_requires_manual_login(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = PageSnapshot(
            html=fixture("login_required.html"),
            title="DeepSeek Login",
            url="https://chat.deepseek.com/signin",
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=1.0,
            last_text_change_at=1.0,
            now=2.0,
        )

        self.assertEqual(observation.state, AdapterState.LOGIN_REQUIRED)
        self.assertIn("password", observation.diagnostics.failed_selector or "")

    def test_rate_limit_fixture(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = PageSnapshot(
            html=fixture("rate_limited.html"),
            title="DeepSeek Chat",
            url="https://chat.deepseek.com/a/chat/synthetic",
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=1.0,
            last_text_change_at=1.0,
            now=2.0,
        )

        self.assertEqual(observation.state, AdapterState.RATE_LIMITED)

    def test_timeout_fixture_times_out_without_completion(self) -> None:
        config = dataclasses.replace(selectors(), timeout_seconds=5.0)
        detector = CompletionDetector(config)
        snapshot = PageSnapshot(
            html=fixture("responding.html"),
            title="DeepSeek Chat",
            url="https://chat.deepseek.com/a/chat/synthetic",
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=1.0,
            last_text_change_at=2.0,
            now=7.0,
        )

        self.assertEqual(observation.state, AdapterState.TIMED_OUT)

    def test_redacted_diagnostics_omit_prompt_response_and_secret_values(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = PageSnapshot(
            html=fixture("ready_completed.html"),
            title="DeepSeek token=abc1234567890123",
            url="https://chat.deepseek.com/a/chat/synthetic?token=do-not-log",
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=100.0,
            last_text_change_at=105.0,
            now=108.0,
        )
        diagnostics = observation.diagnostics.to_dict()
        serialized = json.dumps(diagnostics)

        self.assertEqual(diagnostics["url_origin"], "https://chat.deepseek.com")
        self.assertNotIn("Here is the model response", serialized)
        self.assertNotIn("private user prompt", serialized)
        self.assertNotIn("abc1234567890123", serialized)
        self.assertNotIn("do-not-log", serialized)

    def test_jsonl_protocol_requires_auth_and_rejects_tool_authority(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            adapter = DeepSeekWebAdvisorAdapter(selectors(), Path(temp), headed=True)
            protocol = JsonLinesAdvisorProtocol(adapter, "secret-token")

            bad_auth = protocol.handle({"command": "hello", "auth_token": "wrong"})
            self.assertFalse(bad_auth["ok"])
            self.assertEqual(bad_auth["error"], "UNAUTHORIZED")

            prohibited = protocol.handle(
                {
                    "command": "advise",
                    "auth_token": "secret-token",
                    "request": {
                        "request_id": "advice-1",
                        "tool_definitions": [{"name": "run_command"}],
                    },
                }
            )
            self.assertFalse(prohibited["ok"])
            self.assertIn("PROHIBITED_AUTHORITY", prohibited["error"])

    def test_jsonl_protocol_returns_unavailable_advice_without_browser_tools(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            adapter = DeepSeekWebAdvisorAdapter(selectors(), Path(temp), headed=True)
            protocol = JsonLinesAdvisorProtocol(adapter, "secret-token")
            input_stream = io.StringIO(
                json.dumps(
                    {
                        "command": "advise",
                        "auth_token": "secret-token",
                        "request": {"request_id": "advice-42"},
                    }
                )
                + "\n"
            )
            output_stream = io.StringIO()

            protocol.serve(input_stream, output_stream)
            response = json.loads(output_stream.getvalue())

            self.assertTrue(response["ok"])
            self.assertEqual(response["response"]["request_id"], "advice-42")
            self.assertIn(
                response["response"]["status"],
                {"READY", "LOGIN_REQUIRED", "UNAVAILABLE"},
            )
            self.assertNotIn("tool_definitions", json.dumps(response))


if __name__ == "__main__":
    unittest.main()
