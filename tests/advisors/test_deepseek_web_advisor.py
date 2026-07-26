import dataclasses
import io
import json
import os
import subprocess
import sys
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
    return dataclasses.replace(
        SelectorConfig.from_file(SELECTOR_CONFIG),
        text_stability_seconds=0.5,
        timeout_seconds=2.0,
    )


def valid_request(**overrides: object) -> dict[str, object]:
    request: dict[str, object] = {
        "schemaVersion": 1,
        "requestId": "advice-42",
        "runId": "run-42",
        "objective": "Explain a harmless synthetic failure.",
        "currentStep": "offline adapter test",
        "specificQuestion": "What should be checked next?",
        "constraints": ["No tools are available to the advisor."],
        "latestFailure": "assertion failed in synthetic fixture",
        "boundedSourceExcerpts": [
            {
                "source": "synthetic.py",
                "summary": "tiny fake sample",
                "content": "def answer(): return 41",
            }
        ],
        "boundedPatchOrDiffSummary": "No real repository diff.",
        "verificationSummary": "Synthetic verification failed.",
        "disclosureClassification": "REMOTE_ALLOWED",
        "maximumResponseLength": 512,
    }
    request.update(overrides)
    return request


class OfflineAdapter(DeepSeekWebAdvisorAdapter):
    def __init__(self, snapshots: list[PageSnapshot]) -> None:
        super().__init__(selectors(), Path("offline-profile"), headed=True)
        self._sb = object()
        self.snapshots = snapshots
        self.snapshot_index = 0
        self.typed_prompts: list[str] = []
        self.clicked_selectors: list[str] = []
        self.clock = 0.0
        self.cancel_after_type = False

    def _snapshot(self) -> PageSnapshot:
        index = min(self.snapshot_index, len(self.snapshots) - 1)
        self.snapshot_index += 1
        return self.snapshots[index]

    def _type_prompt(self, selector: str, prompt: str) -> None:
        self.typed_prompts.append(prompt)
        if self.cancel_after_type:
            self.cancel_requested = True

    def _click_first(self, selectors_: object) -> bool:
        selector_list = list(selectors_)  # type: ignore[arg-type]
        if selector_list:
            self.clicked_selectors.append(selector_list[0])
        return bool(selector_list)

    def _press_enter(self, selector: str) -> None:
        self.clicked_selectors.append(f"enter:{selector}")

    def _now(self) -> float:
        return self.clock

    def _sleep(self, seconds: float) -> None:
        self.clock += seconds


class StartupFailureAdapter(DeepSeekWebAdvisorAdapter):
    def _import_seleniumbase(self) -> object:
        raise RuntimeError("synthetic startup failure")


def snapshot(name: str, url: str = "https://chat.deepseek.com/a/chat/synthetic") -> PageSnapshot:
    return PageSnapshot(html=fixture(name), title="DeepSeek Chat", url=url)


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
            last_text_change_at=100.0,
            now=101.0,
        )

        self.assertEqual(observation.state, AdapterState.COMPLETED)
        self.assertGreaterEqual(observation.response_count, 1)
        self.assertFalse(observation.stop_visible)
        self.assertTrue(observation.send_visible)
        self.assertTrue(observation.marker_seen)
        self.assertGreaterEqual(observation.stable_for_seconds, 0.5)

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
            last_text_change_at=100.5,
            now=101.0,
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
                        "schemaVersion": 1,
                        "requestId": "advice-1",
                        "objective": "Synthetic",
                        "specificQuestion": "Synthetic?",
                        "tool-definitions": [{"name": "run_command"}],
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
                        "request": valid_request(),
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
                {"UNAVAILABLE"},
            )
            self.assertNotIn("tool_definitions", json.dumps(response))

    def test_request_validation_rejects_missing_schema_and_extra_instruction(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            adapter = DeepSeekWebAdvisorAdapter(selectors(), Path(temp), headed=True)
            protocol = JsonLinesAdvisorProtocol(adapter, "secret-token")

            missing_schema = protocol.handle(
                {
                    "command": "advise",
                    "auth_token": "secret-token",
                    "request": {"requestId": "advice-1", "objective": "x", "specificQuestion": "y"},
                }
            )
            self.assertFalse(missing_schema["ok"])
            self.assertEqual(missing_schema["error"], "UNSUPPORTED_SCHEMA_VERSION")

            extra_instruction = protocol.handle(
                {
                    "command": "advise",
                    "auth_token": "secret-token",
                    "request": valid_request(operationalInstruction="click everything"),
                }
            )
            self.assertFalse(extra_instruction["ok"])
            self.assertIn("UNSUPPORTED_REQUEST_KEYS", extra_instruction["error"])

    def test_refresh_state_ready_and_origin_rejection(self) -> None:
        ready = OfflineAdapter([snapshot("ready_prompt.html")])
        self.assertEqual(ready.refresh_state(), AdapterState.READY)

        wrong_origin = OfflineAdapter(
            [snapshot("ready_prompt.html", url="https://example.invalid/chat")]
        )
        self.assertEqual(wrong_origin.refresh_state(), AdapterState.DEGRADED)

    def test_advisory_turn_submits_once_and_extracts_newest_response(self) -> None:
        adapter = OfflineAdapter(
            [
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
                snapshot("baseline_streaming.html"),
                snapshot("baseline_completed.html"),
                snapshot("baseline_completed.html"),
                snapshot("baseline_completed.html"),
                snapshot("baseline_completed.html"),
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "COMPLETED")
        self.assertIn("new bounded advice", response.diagnosis)
        self.assertNotIn("old response", response.diagnosis)
        self.assertEqual(len(adapter.typed_prompts), 1)
        self.assertEqual(len([item for item in adapter.clicked_selectors if "Send" in item]), 1)

    def test_advisory_turn_rejects_empty_new_response(self) -> None:
        adapter = OfflineAdapter(
            [
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
                snapshot("empty_completed.html"),
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "FAILED")

    def test_cancel_during_generation_returns_cancelled_without_shutdown(self) -> None:
        adapter = OfflineAdapter(
            [
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
                snapshot("baseline_streaming.html"),
            ]
        )
        adapter.cancel_after_type = True

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "CANCELLED")
        self.assertEqual(adapter.state, AdapterState.CANCELLED)
        self.assertTrue(adapter._sb)

    def test_startup_failure_leaves_starting_state(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            adapter = StartupFailureAdapter(selectors(), Path(temp), headed=True)
            self.assertEqual(adapter.start(), AdapterState.DEGRADED)
            self.assertNotEqual(adapter.state, AdapterState.STARTING)

    def test_jsonl_subprocess_stdout_is_protocol_only(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            env = os.environ.copy()
            env["CATDESK_ADVISOR_AUTH_TOKEN"] = "secret-token"
            proc = subprocess.run(
                [
                    sys.executable,
                    str(
                        Path(__file__).resolve().parents[2]
                        / "experimental"
                        / "advisors"
                        / "deepseek_web_advisor.py"
                    ),
                    "--profile-dir",
                    temp,
                ],
                input=json.dumps({"command": "hello", "auth_token": "secret-token"}) + "\n",
                capture_output=True,
                text=True,
                env=env,
                timeout=10,
                check=False,
            )

        self.assertEqual(proc.returncode, 0)
        lines = [line for line in proc.stdout.splitlines() if line.strip()]
        self.assertEqual(len(lines), 1)
        self.assertTrue(json.loads(lines[0])["ok"])


if __name__ == "__main__":
    unittest.main()
