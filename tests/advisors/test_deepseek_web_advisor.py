import dataclasses
import io
import json
import os
import subprocess
import sys
import tempfile
import threading
import time
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
            self.cancel_requested.set()

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


class BlockingAdapter(DeepSeekWebAdvisorAdapter):
    def __init__(self) -> None:
        super().__init__(selectors(), Path("blocking-profile"), headed=True)
        self.state = AdapterState.READY
        self.started = threading.Event()

    def refresh_state(self) -> AdapterState:
        return self.state

    def advise(self, request: dict[str, object]):  # type: ignore[override]
        from experimental.advisors.deepseek_web_advisor import (
            AdvisorStatus,
            normalize_advice_response,
            validate_advice_request,
        )

        validated = validate_advice_request(request)
        self.state = AdapterState.WAITING_FOR_RESPONSE
        self.started.set()
        while not self.cancel_requested.is_set():
            time.sleep(0.01)
        self.state = AdapterState.CANCELLED
        return normalize_advice_response(
            validated,
            self.advisor_id,
            AdvisorStatus.CANCELLED,
            "cancelled by protocol",
            confidence="LOW",
        )


class PromptFakeSb:
    def __init__(self, retained_value: str | None = "") -> None:
        self.retained_value = retained_value
        self.calls: list[str] = []

    def click(self, selector: str) -> None:
        self.calls.append(f"click:{selector}")

    def clear(self, selector: str) -> None:
        self.calls.append(f"clear:{selector}")
        if self.retained_value == "":
            self.retained_value = ""

    def get_attribute(self, selector: str, name: str) -> str | None:
        self.calls.append(f"get_attribute:{selector}:{name}")
        return self.retained_value

    def press_keys(self, selector: str, text: str) -> None:
        self.calls.append(f"press_keys:{selector}:{text[:8]}")


class PartialStartupAdapter(DeepSeekWebAdvisorAdapter):
    def __init__(self, context: object) -> None:
        super().__init__(selectors(), Path("partial-startup"), headed=True)
        self.context = context

    def _import_seleniumbase(self) -> object:
        context = self.context

        class Factory:
            def __call__(self, **_kwargs: object) -> object:
                return context

        return Factory()


class FailingBrowserAdapter(OfflineAdapter):
    def _type_prompt(self, selector: str, prompt: str) -> None:
        raise RuntimeError("synthetic browser failure")


class FailingStopAdapter(OfflineAdapter):
    def __init__(self) -> None:
        super().__init__([snapshot("baseline_before.html")])
        self.state = AdapterState.WAITING_FOR_RESPONSE

    def _click_first(self, selectors_: object) -> bool:
        raise RuntimeError("synthetic stop failure")


class PartialContext:
    def __init__(self) -> None:
        self.exited = False

    def __enter__(self) -> object:
        return self

    def __exit__(self, *_args: object) -> None:
        self.exited = True

    def activate_cdp_mode(self, _url: str) -> None:
        raise RuntimeError("navigation failed")


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
            lines = [json.loads(line) for line in output_stream.getvalue().splitlines()]
            response = lines[0]

            self.assertTrue(response["ok"])
            self.assertTrue(response["accepted"])
            self.assertEqual(response["request_id"], "advice-42")
            self.assertEqual(lines[1]["event"], "advice_completed")
            self.assertEqual(lines[1]["response"]["status"], "UNAVAILABLE")
            self.assertNotIn("tool_definitions", json.dumps(lines))

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

    def test_disclosure_denied_before_typing(self) -> None:
        adapter = OfflineAdapter([snapshot("ready_prompt.html")])
        with self.assertRaises(ValueError):
            adapter.advise(valid_request(disclosureClassification="LOCAL_ONLY"))

        self.assertEqual(adapter.typed_prompts, [])

        with tempfile.TemporaryDirectory() as temp:
            protocol = JsonLinesAdvisorProtocol(
                DeepSeekWebAdvisorAdapter(selectors(), Path(temp), headed=True),
                "secret-token",
            )
            denied = protocol.handle(
                {
                    "command": "advise",
                    "auth_token": "secret-token",
                    "request": valid_request(disclosureClassification="LOCAL_ONLY"),
                }
            )
            self.assertFalse(denied["ok"])
            self.assertEqual(denied["error"], "DISCLOSURE_DENIED")

    def test_refresh_state_ready_and_origin_rejection(self) -> None:
        ready = OfflineAdapter([snapshot("ready_prompt.html")])
        self.assertEqual(ready.refresh_state(), AdapterState.READY)

        wrong_origin = OfflineAdapter(
            [snapshot("ready_prompt.html", url="https://example.invalid/chat")]
        )
        self.assertEqual(wrong_origin.refresh_state(), AdapterState.DEGRADED)

    def test_malicious_selector_start_url_is_rejected(self) -> None:
        bad_selectors = dataclasses.replace(
            selectors(),
            start_url="https://example.invalid/chat",
        )
        with tempfile.TemporaryDirectory() as temp:
            adapter = DeepSeekWebAdvisorAdapter(bad_selectors, Path(temp), headed=True)
            self.assertEqual(adapter.start(), AdapterState.DEGRADED)

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

    def test_partial_startup_failure_cleans_browser_context(self) -> None:
        context = PartialContext()
        adapter = PartialStartupAdapter(context)

        self.assertEqual(adapter.start(), AdapterState.DEGRADED)
        self.assertTrue(context.exited)
        self.assertIsNone(adapter._sb)
        self.assertIsNone(adapter._sb_context)

    def test_prompt_is_cleared_before_typing(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("prompt-profile"), headed=True)
        fake = PromptFakeSb(retained_value="")
        adapter._sb = fake

        adapter._type_prompt("textarea", "synthetic prompt")

        clear_index = fake.calls.index("clear:textarea")
        press_index = next(index for index, call in enumerate(fake.calls) if call.startswith("press_keys"))
        self.assertLess(clear_index, press_index)
        self.assertTrue(any(call.startswith("press_keys:textarea:synthet") for call in fake.calls))

        retained = PromptFakeSb(retained_value="retained draft")
        adapter._sb = retained
        with self.assertRaises(RuntimeError):
            adapter._type_prompt("textarea", "synthetic prompt")

    def test_browser_exception_returns_bounded_failure(self) -> None:
        adapter = FailingBrowserAdapter(
            [snapshot("baseline_before.html"), snapshot("baseline_before.html")]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "FAILED")
        self.assertEqual(adapter.state, AdapterState.DEGRADED)
        self.assertNotIn("synthetic browser failure", response.diagnosis)

    def test_protocol_cancel_status_second_request_and_completion_event(self) -> None:
        adapter = BlockingAdapter()
        protocol = JsonLinesAdvisorProtocol(adapter, "secret-token")
        events: list[dict[str, object]] = []

        accepted = protocol.handle(
            {
                "command": "advise",
                "auth_token": "secret-token",
                "request": valid_request(requestId="advice-block"),
            },
            events.append,
        )
        self.assertTrue(accepted["accepted"])
        self.assertTrue(adapter.started.wait(1.0))

        status = protocol.handle({"command": "status", "auth_token": "secret-token"}, events.append)
        self.assertTrue(status["active"])
        self.assertEqual(status["request_id"], "advice-block")

        second = protocol.handle(
            {
                "command": "advise",
                "auth_token": "secret-token",
                "request": valid_request(requestId="advice-second"),
            },
            events.append,
        )
        self.assertFalse(second["ok"])
        self.assertEqual(second["error"], "ADVICE_ALREADY_ACTIVE")

        cancelled = protocol.handle(
            {"command": "cancel", "auth_token": "secret-token"},
            events.append,
        )
        self.assertTrue(cancelled["ok"])
        protocol._join_active(1.0)
        self.assertEqual(events[-1]["event"], "advice_completed")
        self.assertEqual(events[-1]["request_id"], "advice-block")
        self.assertEqual(events[-1]["response"]["status"], "CANCELLED")

    def test_protocol_cancel_handles_stop_button_failure(self) -> None:
        adapter = FailingStopAdapter()
        protocol = JsonLinesAdvisorProtocol(adapter, "secret-token")

        cancelled = protocol.handle(
            {"command": "cancel", "auth_token": "secret-token"},
            None,
        )

        self.assertTrue(cancelled["ok"])
        self.assertEqual(cancelled["response"]["status"], "CANCELLED")
        self.assertEqual(adapter.state, AdapterState.CANCELLED)

    def test_protocol_shutdown_during_advice_cancels_and_joins(self) -> None:
        adapter = BlockingAdapter()
        protocol = JsonLinesAdvisorProtocol(adapter, "secret-token")
        events: list[dict[str, object]] = []

        protocol.handle(
            {
                "command": "advise",
                "auth_token": "secret-token",
                "request": valid_request(requestId="advice-shutdown"),
            },
            events.append,
        )
        self.assertTrue(adapter.started.wait(1.0))

        shutdown = protocol.handle(
            {"command": "shutdown", "auth_token": "secret-token"},
            events.append,
        )

        self.assertTrue(shutdown["ok"])
        self.assertEqual(adapter.state, AdapterState.STOPPED)
        self.assertFalse(protocol._thread_is_active_locked())

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
