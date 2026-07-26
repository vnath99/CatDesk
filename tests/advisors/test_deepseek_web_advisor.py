import dataclasses
import io
import json
import os
import random
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
    DEEPSEEK_GENERATION_INSTALL_SCRIPT,
    DEEPSEEK_GENERATION_STATE_SCRIPT,
    DeepSeekWebAdvisorAdapter,
    GenerationTracker,
    JsonLinesAdvisorProtocol,
    PageSnapshot,
    SelectorConfig,
    collect_virtual_list_baseline,
    newest_assistant_after_baseline,
    normalize_response_text,
    sha256_text,
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
        stable_sample_count=2,
        timeout_seconds=2.0,
        typing_min_interval_seconds=0.01,
        typing_max_interval_seconds=0.02,
        typing_newline_pause_min_seconds=0.05,
        typing_newline_pause_max_seconds=0.05,
        typing_timeout_seconds=5.0,
        submission_confirmation_timeout_seconds=0.5,
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
        super().__init__(
            selectors(),
            Path("offline-profile"),
            headed=True,
            rng=random.Random(7),
        )
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
        return self._click_first_enabled(selectors_)

    def _click_first_enabled(self, selectors_: object) -> bool:
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


class PromptScriptFakeSb(PromptFakeSb):
    def __init__(self) -> None:
        super().__init__(retained_value="")

    def execute_script(self, script: str) -> bool:
        self.calls.append(f"execute_script:{len(script)}")
        return True


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


class PromptClearedAdapter(OfflineAdapter):
    def _prompt_value(self, selector: str) -> str | None:
        return ""


class StateAdapter(DeepSeekWebAdvisorAdapter):
    def __init__(self, state: AdapterState) -> None:
        super().__init__(selectors(), Path("state-profile"), headed=True)
        self.state = state

    def refresh_state(self) -> AdapterState:
        return self.state


class DisconnectRecordingAdapter(DeepSeekWebAdvisorAdapter):
    def __init__(self) -> None:
        super().__init__(selectors(), Path("disconnect-profile"), headed=True)
        self.disconnected = False
        self.state = AdapterState.WAITING_FOR_RESPONSE
        self.active_generation = GenerationTracker(
            generation_id="gen-cancel",
            request_id="advice-cancel",
            submitted_prompt_hash=sha256_text("prompt"),
            started_at=0.0,
            baseline_turn_keys=set(),
            baseline_assistant_count=0,
            baseline_latest_assistant_key=None,
            baseline_latest_assistant_hash=None,
        )

    def _disconnect_generation_observers(self) -> None:
        self.disconnected = True


class EmptyChatBootstrapAdapter(DeepSeekWebAdvisorAdapter):
    def __init__(self, states: list[dict[str, object]]) -> None:
        super().__init__(selectors(), Path("empty-chat-profile"), headed=True, rng=random.Random(7))
        self._sb = PromptScriptFakeSb()
        self.state = AdapterState.READY
        self.clock = 0.0
        self.states = states
        self.typed_prompts: list[str] = []
        self.clicked = False
        self.click_count = 0
        self.enter_count = 0
        self.dom_value = ""
        self.send_enabled = True
        self.url_path = "/"
        self.installed_script = ""
        self.disconnected = False

    def refresh_state(self) -> AdapterState:
        self.state = AdapterState.READY
        return self.state

    def _snapshot(self) -> PageSnapshot:
        return inline_snapshot(
            """
            <html><body>
              <div id="root">
                <textarea placeholder="Message DeepSeek"></textarea>
                <div role="button" class="ds-button ds-button--primary ds-button--circle">Send</div>
              </div>
            </body></html>
            """
        )

    def _execute_browser_script(self, script: str) -> object:
        if "bootstrapObserver.observe(appRoot" in script:
            self.installed_script = script
            return {"ok": True, "observer": "bootstrap"}
        if "rootFound: false" in script and "validEmptyChat" in script:
            return {
                "rootFound": False,
                "validEmptyChat": True,
                "turnKeys": [],
                "assistantCount": 0,
                "latestAssistantKey": None,
                "latestAssistantText": "",
                "turns": [],
            }
        if "window.__catdeskDeepSeekGeneration" in script and "assistantText" in script:
            if self.states:
                return self._state_with_completion_defaults(self.states.pop(0))
            return self._state_with_completion_defaults({
                "ok": True,
                "rootFound": True,
                "lifecycle": "STABILIZING",
                "detectedUserTurnKey": "user-1",
                "detectedAssistantTurnKey": "assistant-1",
                "assistantText": "complete assistant answer",
                "mutationCount": 3,
                "sawUserTurn": True,
                "sawAssistantTurn": True,
                "sawAssistantTextChange": True,
                "sawGenerationActive": True,
                "controls": {"sendVisible": True, "sendEnabled": True, "stopVisible": False},
                "turnKeys": ["user-1", "assistant-1"],
                "assistantCount": 1,
                "assistantActionRowVisible": True,
            })
        if "delete window.__catdeskDeepSeekGeneration" in script:
            self.disconnected = True
            return True
        return True

    def _state_with_completion_defaults(self, state: dict[str, object]) -> dict[str, object]:
        output = dict(state)
        controls = dict(output.get("controls") or {})
        controls.setdefault("composerFound", True)
        controls.setdefault("composerVisible", True)
        controls.setdefault("composerEnabled", True)
        controls.setdefault("composerReadOnly", False)
        controls.setdefault("composerValueLength", 0)
        controls.setdefault("sendVisible", True)
        controls.setdefault("sendEnabled", True)
        controls.setdefault("stopVisible", False)
        output["controls"] = controls
        if output.get("sawAssistantTurn") and output.get("assistantText"):
            output.setdefault("assistantActionRowVisible", True)
        else:
            output.setdefault("assistantActionRowVisible", False)
        return output

    def _type_prompt(self, selector: str, prompt: str) -> None:
        self.typed_prompts.append(prompt)
        self.dom_value = prompt

    def _click_verified_composer_send(self) -> bool:
        self.click_count += 1
        self.clicked = True
        return self.send_enabled

    def _deepseek_composer_send_state(self) -> dict[str, object]:
        return {
            "composerFound": True,
            "value": self.dom_value,
            "valueLength": len(self.dom_value),
            "sendVisible": True,
            "sendEnabled": self.send_enabled,
            "sendDisabled": not self.send_enabled,
            "urlPath": self.url_path,
            "rootCount": 0,
            "userTurnCount": 0,
        }

    def _press_enter(self, selector: str) -> None:
        self.enter_count += 1

    def _now(self) -> float:
        return self.clock

    def _sleep(self, seconds: float) -> None:
        self.clock += seconds


class MismatchedComposerAdapter(EmptyChatBootstrapAdapter):
    def _type_prompt(self, selector: str, prompt: str) -> None:
        self.typed_prompts.append(prompt)
        self.dom_value = "different prompt"


class CookieAdapter(OfflineAdapter):
    def __init__(self, page: PageSnapshot) -> None:
        super().__init__([page])

    def _snapshot(self) -> PageSnapshot:
        return self.snapshots[0]


class EnvLoginAdapter(OfflineAdapter):
    def __init__(self, page: PageSnapshot, *, allow_env_login: bool = True) -> None:
        DeepSeekWebAdvisorAdapter.__init__(
            self,
            selectors(),
            Path("env-login-profile"),
            headed=True,
            allow_env_login=allow_env_login,
        )
        self._sb = object()
        self.snapshots = [page]
        self.snapshot_index = 0
        self.typed_fields: list[tuple[str, str]] = []
        self.clicked_selectors: list[str] = []

    def _snapshot(self) -> PageSnapshot:
        return self.snapshots[0]

    def _type_login_field(self, selector: str, value: str) -> None:
        self.typed_fields.append((selector, value))

    def _click_first_enabled(self, selectors_: object) -> bool:
        selector_list = list(selectors_)  # type: ignore[arg-type]
        if selector_list:
            self.clicked_selectors.append(selector_list[0])
            return True
        return False


class FakeElement:
    def __init__(self, text: str) -> None:
        self.text = text
        self.clicked = False

    def is_displayed(self) -> bool:
        return True

    def click(self) -> None:
        self.clicked = True


class ChatAdapter(OfflineAdapter):
    def __init__(self, page: PageSnapshot, elements: dict[str, list[FakeElement]]) -> None:
        super().__init__([page])
        self.elements = elements

    def _snapshot(self) -> PageSnapshot:
        return self.snapshots[0]

    def _find_elements(self, selector: str) -> list[object]:
        return list(self.elements.get(selector, []))


class PacedTypingAdapter(DeepSeekWebAdvisorAdapter):
    def __init__(self) -> None:
        super().__init__(
            selectors(),
            Path("paced-profile"),
            headed=True,
            rng=random.Random(3),
        )
        self._sb = PromptFakeSb(retained_value="")
        self.sleeps: list[float] = []

    def _sleep(self, seconds: float) -> None:
        self.sleeps.append(seconds)


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


def inline_snapshot(html: str, url: str = "https://chat.deepseek.com/a/chat/synthetic") -> PageSnapshot:
    return PageSnapshot(html=html, title="DeepSeek Chat", url=url)


def completion_tracker() -> GenerationTracker:
    tracker = GenerationTracker(
        generation_id="gen-completion",
        request_id="advice-completion",
        submitted_prompt_hash=sha256_text("submitted prompt"),
        started_at=0.0,
        baseline_turn_keys=set(),
        baseline_assistant_count=0,
        baseline_latest_assistant_key=None,
        baseline_latest_assistant_hash=None,
    )
    tracker.saw_user_turn = True
    tracker.saw_assistant_turn = True
    tracker.saw_assistant_text_change = True
    return tracker


def completion_controls(**overrides: object) -> dict[str, object]:
    controls: dict[str, object] = {
        "composerFound": True,
        "composerVisible": True,
        "composerEnabled": True,
        "composerReadOnly": False,
        "composerValueLength": 0,
        "sendVisible": True,
        "sendEnabled": False,
        "stopVisible": False,
    }
    controls.update(overrides)
    return controls


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

    def test_env_login_uses_explicit_selectors_and_stops_for_security_verification(self) -> None:
        login_page = inline_snapshot(
            """
            <main data-testid="login-form">
              <input type="email" aria-label="Email">
              <input type="password" aria-label="Password">
              <button type="submit">Sign in</button>
            </main>
            """,
            url="https://chat.deepseek.com/signin",
        )
        adapter = EnvLoginAdapter(login_page)
        old_email = os.environ.get("CATDESK_ADVISOR_DEEPSEEK_EMAIL")
        old_password = os.environ.get("CATDESK_ADVISOR_DEEPSEEK_PASSWORD")
        os.environ["CATDESK_ADVISOR_DEEPSEEK_EMAIL"] = "synthetic@example.invalid"
        os.environ["CATDESK_ADVISOR_DEEPSEEK_PASSWORD"] = "synthetic-password"
        try:
            self.assertTrue(adapter._attempt_env_login())
            self.assertNotIn("CATDESK_ADVISOR_DEEPSEEK_EMAIL", os.environ)
            self.assertNotIn("CATDESK_ADVISOR_DEEPSEEK_PASSWORD", os.environ)
        finally:
            if old_email is None:
                os.environ.pop("CATDESK_ADVISOR_DEEPSEEK_EMAIL", None)
            else:
                os.environ["CATDESK_ADVISOR_DEEPSEEK_EMAIL"] = old_email
            if old_password is None:
                os.environ.pop("CATDESK_ADVISOR_DEEPSEEK_PASSWORD", None)
            else:
                os.environ["CATDESK_ADVISOR_DEEPSEEK_PASSWORD"] = old_password

        self.assertEqual(
            adapter.typed_fields,
            [
                ("input[type=\"email\"]", "synthetic@example.invalid"),
                ("input[type=\"password\"]", "synthetic-password"),
            ],
        )
        self.assertEqual(adapter.clicked_selectors, ["button[type=\"submit\"]"])

        os.environ["CATDESK_ADVISOR_DEEPSEEK_EMAIL"] = "synthetic@example.invalid"
        os.environ["CATDESK_ADVISOR_DEEPSEEK_PASSWORD"] = "synthetic-password"
        try:
            security_adapter = EnvLoginAdapter(snapshot("security_challenge.html"))
            self.assertFalse(security_adapter._attempt_env_login())
            self.assertEqual(security_adapter.typed_fields, [])
            self.assertEqual(security_adapter.state, AdapterState.TAKEOVER_REQUIRED)
        finally:
            if old_email is None:
                os.environ.pop("CATDESK_ADVISOR_DEEPSEEK_EMAIL", None)
            else:
                os.environ["CATDESK_ADVISOR_DEEPSEEK_EMAIL"] = old_email
            if old_password is None:
                os.environ.pop("CATDESK_ADVISOR_DEEPSEEK_PASSWORD", None)
            else:
                os.environ["CATDESK_ADVISOR_DEEPSEEK_PASSWORD"] = old_password

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

    def test_rate_limit_requires_visible_provider_selector(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = inline_snapshot(
            """
            <main>
              <section data-ds-role="assistant-response">
                The words rate limit are only ordinary text. Copy
              </section>
              <button aria-label="Send">Send</button>
            </main>
            """
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=1.0,
            last_text_change_at=1.0,
            now=2.0,
        )

        self.assertEqual(observation.state, AdapterState.COMPLETED)

    def test_magic_words_are_not_required_for_completion(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = inline_snapshot(
            """
            <main>
              <section data-ds-role="assistant-response">
                A stable assistant answer without provider toolbar words.
              </section>
              <button aria-label="Send">Send</button>
            </main>
            """
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=1.0,
            last_text_change_at=1.0,
            now=2.0,
        )

        self.assertEqual(observation.state, AdapterState.COMPLETED)

    def test_hidden_send_control_does_not_complete(self) -> None:
        detector = CompletionDetector(selectors())
        snapshot = inline_snapshot(
            """
            <main>
              <section data-ds-role="assistant-response">new advice Copy</section>
              <button aria-label="Send" hidden>Send</button>
            </main>
            """
        )

        observation = detector.observe(
            snapshot,
            first_seen_at=1.0,
            last_text_change_at=1.0,
            now=2.0,
        )

        self.assertEqual(observation.state, AdapterState.WAITING_FOR_RESPONSE)

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

    def test_jsonl_protocol_rejects_advice_without_ready_browser(self) -> None:
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

            self.assertFalse(response["ok"])
            self.assertEqual(response["error"], "ADVISOR_NOT_READY:STOPPED")
            self.assertEqual(len(lines), 1)
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

        empty_prompt_disabled_send = OfflineAdapter(
            [
                inline_snapshot(
                    """
                    <main>
                      <textarea aria-label="Message DeepSeek"></textarea>
                      <button aria-label="Send" disabled>Send</button>
                    </main>
                    """
                )
            ]
        )
        self.assertEqual(empty_prompt_disabled_send.refresh_state(), AdapterState.READY)

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

    def test_chat_selection_uses_exact_configured_chat_name(self) -> None:
        page = snapshot("ready_prompt.html")
        target = FakeElement("CatDesk advisor smoke")
        other = FakeElement("Other chat")
        adapter = ChatAdapter(
            page,
            {
                "button[data-testid=\"chat-menu\"]": [FakeElement("menu")],
                "[data-testid=\"chat-history-item\"]": [other, target],
            },
        )

        self.assertTrue(adapter._ensure_chat_selected("CatDesk advisor smoke"))

        self.assertFalse(other.clicked)
        self.assertTrue(target.clicked)

    def test_virtual_list_baseline_collects_direct_turn_keys(self) -> None:
        page = snapshot("virtual_list_completed.html")

        baseline = collect_virtual_list_baseline(page)

        self.assertTrue(baseline.root_found)
        self.assertEqual(baseline.turn_keys, ["old-assistant", "user-new", "assistant-new"])
        self.assertEqual(baseline.assistant_count, 2)
        self.assertEqual(baseline.latest_assistant_key, "assistant-new")
        self.assertTrue(baseline.latest_assistant_hash)

    def test_virtual_list_identifies_new_user_and_assistant_turns(self) -> None:
        before = inline_snapshot(
            """
            <main><div class="ds-virtual-list-visible-items">
              <div data-virtual-list-item-key="old"><div class="ds-markdown ds-assistant-message-main-content">old</div></div>
            </div></main>
            """
        )
        after = snapshot("virtual_list_completed.html")

        baseline = collect_virtual_list_baseline(before)
        current = collect_virtual_list_baseline(after)
        newest = newest_assistant_after_baseline(baseline, current)

        self.assertIsNotNone(newest)
        self.assertEqual(newest.key, "assistant-new")
        self.assertTrue(any(turn.role == "user" and turn.key == "user-new" for turn in current.turns))

    def test_virtual_list_ignores_reasoning_and_action_rows(self) -> None:
        newest = newest_assistant_after_baseline(
            collect_virtual_list_baseline(
                inline_snapshot('<main><div class="ds-virtual-list-visible-items"></div></main>')
            ),
            collect_virtual_list_baseline(snapshot("virtual_list_completed.html")),
        )

        self.assertIsNotNone(newest)
        self.assertIn("First paragraph", newest.assistant_text)
        self.assertNotIn("private reasoning", newest.assistant_text)
        self.assertNotIn("Copy", newest.assistant_text)
        self.assertNotIn("Share", newest.assistant_text)

    def test_virtual_list_extracts_complete_container_not_last_span(self) -> None:
        page = inline_snapshot(
            """
            <main><div class="ds-virtual-list-visible-items">
              <div data-virtual-list-item-key="assistant-new">
                <div class="ds-markdown ds-assistant-message-main-content">
                  <p>First paragraph.</p>
                  <p>Second paragraph with <span>last span only</span>.</p>
                  <pre><code>line one
line two</code></pre>
                </div>
              </div>
            </div></main>
            """
        )

        turn = collect_virtual_list_baseline(page).turns[0]

        self.assertIn("First paragraph.", turn.assistant_text)
        self.assertIn("Second paragraph", turn.assistant_text)
        self.assertIn("line one", turn.assistant_text)
        self.assertNotEqual(turn.assistant_text, "last span only")

    def test_virtual_list_rejects_unchanged_baseline_assistant(self) -> None:
        baseline = collect_virtual_list_baseline(snapshot("virtual_list_completed.html"))
        current = collect_virtual_list_baseline(snapshot("virtual_list_completed.html"))

        self.assertIsNone(newest_assistant_after_baseline(baseline, current))

    def test_empty_chat_bootstrap_script_targets_root_only(self) -> None:
        self.assertIn("document.querySelector('#root')", DEEPSEEK_GENERATION_INSTALL_SCRIPT)
        self.assertIn("bootstrapObserver.observe(appRoot, { subtree: true, childList: true })", DEEPSEEK_GENERATION_INSTALL_SCRIPT)
        self.assertIn("WAITING_FOR_CONVERSATION_ROOT", DEEPSEEK_GENERATION_STATE_SCRIPT)

    def test_blank_chat_valid_empty_baseline_completes_after_root_creation(self) -> None:
        adapter = EmptyChatBootstrapAdapter(
            [
                {
                    "ok": True,
                    "rootFound": True,
                    "lifecycle": "WAITING_FOR_USER_TURN",
                    "detectedUserTurnKey": "user-1",
                    "detectedAssistantTurnKey": None,
                    "assistantText": "",
                    "mutationCount": 1,
                    "sawUserTurn": True,
                    "sawAssistantTurn": False,
                    "sawAssistantTextChange": False,
                    "sawGenerationActive": True,
                    "controls": {"sendVisible": False, "sendEnabled": False, "stopVisible": True},
                    "turnKeys": ["user-1"],
                    "assistantCount": 0,
                },
                {
                    "ok": True,
                    "rootFound": True,
                    "lifecycle": "STABILIZING",
                    "detectedUserTurnKey": "user-1",
                    "detectedAssistantTurnKey": "assistant-1",
                    "assistantText": "complete assistant answer",
                    "mutationCount": 3,
                    "sawUserTurn": True,
                    "sawAssistantTurn": True,
                    "sawAssistantTextChange": True,
                    "sawGenerationActive": True,
                    "controls": {"sendVisible": True, "sendEnabled": True, "stopVisible": False},
                    "turnKeys": ["user-1", "assistant-1"],
                    "assistantCount": 1,
                },
                {
                    "ok": True,
                    "rootFound": True,
                    "lifecycle": "STABILIZING",
                    "detectedUserTurnKey": "user-1",
                    "detectedAssistantTurnKey": "assistant-1",
                    "assistantText": "complete assistant answer",
                    "mutationCount": 3,
                    "sawUserTurn": True,
                    "sawAssistantTurn": True,
                    "sawAssistantTextChange": True,
                    "sawGenerationActive": True,
                    "controls": {"sendVisible": True, "sendEnabled": True, "stopVisible": False},
                    "turnKeys": ["user-1", "assistant-1"],
                    "assistantCount": 1,
                },
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "COMPLETED")
        self.assertIn("complete assistant answer", response.diagnosis)
        self.assertEqual(len(adapter.typed_prompts), 1)
        self.assertTrue(adapter.clicked)
        self.assertIn("bootstrapObserver.observe(appRoot", adapter.installed_script)
        self.assertTrue(adapter.disconnected)

    def test_disabled_composer_send_is_not_classified_as_stop(self) -> None:
        adapter = EmptyChatBootstrapAdapter([])
        adapter.send_enabled = False

        page = adapter._snapshot()

        self.assertTrue(adapter._selector_is_visible(page, "deepseek:composer-send"))
        self.assertFalse(adapter._selector_is_enabled(page, "deepseek:composer-send"))
        self.assertFalse(adapter._selector_is_visible(page, "deepseek:composer-stop"))
        self.assertFalse(adapter._selector_is_enabled(page, "deepseek:composer-stop"))

    def test_send_must_be_enabled_after_typing(self) -> None:
        adapter = EmptyChatBootstrapAdapter([])
        adapter.send_enabled = False

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "DEGRADED")
        self.assertFalse(adapter.clicked)
        self.assertEqual(adapter.enter_count, 0)
        self.assertFalse(adapter.last_submission_diagnostics["send_enabled"])

    def test_mismatched_composer_value_fails_before_click(self) -> None:
        adapter = MismatchedComposerAdapter([])

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "DEGRADED")
        self.assertFalse(adapter.clicked)
        self.assertEqual(adapter.click_count, 0)
        self.assertFalse(adapter.last_submission_diagnostics["prompt_value_matches"])

    def test_exact_composer_send_click_occurs_once_without_enter_fallback(self) -> None:
        adapter = EmptyChatBootstrapAdapter(
            [
                {
                    "ok": True,
                    "rootFound": True,
                    "lifecycle": "WAITING_FOR_USER_TURN",
                    "detectedUserTurnKey": "user-once",
                    "detectedAssistantTurnKey": None,
                    "assistantText": "",
                    "mutationCount": 1,
                    "sawUserTurn": True,
                    "sawAssistantTurn": False,
                    "sawAssistantTextChange": False,
                    "sawGenerationActive": True,
                    "controls": {"sendVisible": True, "sendEnabled": True, "stopVisible": False},
                    "turnKeys": ["user-once"],
                    "assistantCount": 0,
                    "urlPath": "/a/chat/s/synthetic",
                    "rootCount": 1,
                    "userTurnCount": 1,
                },
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "COMPLETED")
        self.assertEqual(adapter.click_count, 1)
        self.assertEqual(adapter.enter_count, 0)

    def test_disabled_send_does_not_confirm_submission(self) -> None:
        adapter = EmptyChatBootstrapAdapter(
            [
                {
                    "ok": True,
                    "rootFound": False,
                    "lifecycle": "WAITING_FOR_CONVERSATION_ROOT",
                    "assistantText": "",
                    "mutationCount": 1,
                    "sawUserTurn": False,
                    "sawAssistantTurn": False,
                    "sawAssistantTextChange": False,
                    "sawGenerationActive": False,
                    "controls": {"sendVisible": True, "sendEnabled": False, "sendDisabled": True, "stopVisible": False},
                    "turnKeys": [],
                    "assistantCount": 0,
                    "urlPath": "/",
                    "rootCount": 0,
                    "userTurnCount": 0,
                }
                for _ in range(4)
            ]
        )
        tracker = GenerationTracker(
            generation_id="gen-disabled",
            request_id="advice-disabled",
            submitted_prompt_hash=sha256_text("prompt"),
            started_at=0.0,
            baseline_turn_keys=set(),
            baseline_assistant_count=0,
            baseline_latest_assistant_key=None,
            baseline_latest_assistant_hash=None,
        )
        adapter.selectors = dataclasses.replace(
            adapter.selectors,
            submission_confirmation_timeout_seconds=0.5,
        )

        self.assertFalse(adapter._confirm_generation_submission(tracker, "textarea", "/"))
        self.assertFalse(adapter.last_submission_diagnostics["confirmed"])

    def test_route_change_without_root_or_user_turn_is_insufficient(self) -> None:
        adapter = EmptyChatBootstrapAdapter(
            [
                {
                    "ok": True,
                    "rootFound": False,
                    "lifecycle": "WAITING_FOR_CONVERSATION_ROOT",
                    "assistantText": "",
                    "mutationCount": 1,
                    "sawUserTurn": False,
                    "sawAssistantTurn": False,
                    "sawAssistantTextChange": False,
                    "sawGenerationActive": False,
                    "controls": {"sendVisible": True, "sendEnabled": True, "stopVisible": False},
                    "turnKeys": [],
                    "assistantCount": 0,
                    "urlPath": "/a/chat/s/created",
                    "rootCount": 0,
                    "userTurnCount": 0,
                }
                for _ in range(4)
            ]
        )
        tracker = GenerationTracker(
            generation_id="gen-route",
            request_id="advice-route",
            submitted_prompt_hash=sha256_text("prompt"),
            started_at=0.0,
            baseline_turn_keys=set(),
            baseline_assistant_count=0,
            baseline_latest_assistant_key=None,
            baseline_latest_assistant_hash=None,
        )
        adapter.selectors = dataclasses.replace(
            adapter.selectors,
            submission_confirmation_timeout_seconds=0.5,
        )

        self.assertFalse(adapter._confirm_generation_submission(tracker, "textarea", "/"))
        self.assertFalse(adapter.last_submission_diagnostics["confirmed"])

    def test_bounded_exact_root_poll_recovers_observer_race(self) -> None:
        adapter = EmptyChatBootstrapAdapter(
            [
                {
                    "ok": True,
                    "rootFound": False,
                    "lifecycle": "WAITING_FOR_CONVERSATION_ROOT",
                    "assistantText": "",
                    "mutationCount": 0,
                    "sawUserTurn": False,
                    "sawAssistantTurn": False,
                    "sawAssistantTextChange": False,
                    "sawGenerationActive": False,
                    "controls": {"sendVisible": True, "sendEnabled": True, "stopVisible": False},
                    "turnKeys": [],
                    "assistantCount": 0,
                },
                {
                    "ok": True,
                    "rootFound": True,
                    "lifecycle": "WAITING_FOR_USER_TURN",
                    "detectedUserTurnKey": "user-race",
                    "detectedAssistantTurnKey": None,
                    "assistantText": "",
                    "mutationCount": 1,
                    "sawUserTurn": True,
                    "sawAssistantTurn": False,
                    "sawAssistantTextChange": False,
                    "sawGenerationActive": True,
                    "controls": {"sendVisible": False, "sendEnabled": False, "stopVisible": True},
                    "turnKeys": ["user-race"],
                    "assistantCount": 0,
                },
            ]
        )
        tracker = GenerationTracker(
            generation_id="gen-race",
            request_id="advice-race",
            submitted_prompt_hash=sha256_text("prompt"),
            started_at=0.0,
            baseline_turn_keys=set(),
            baseline_assistant_count=0,
            baseline_latest_assistant_key=None,
            baseline_latest_assistant_hash=None,
        )

        self.assertTrue(adapter._install_generation_observer(tracker))
        self.assertTrue(adapter._confirm_generation_submission(tracker, "textarea"))
        self.assertTrue(tracker.saw_user_turn)

    def test_cancellation_before_root_creation_cleans_bootstrap_observer(self) -> None:
        adapter = EmptyChatBootstrapAdapter([])
        tracker = GenerationTracker(
            generation_id="gen-cancel-root",
            request_id="advice-cancel-root",
            submitted_prompt_hash=sha256_text("prompt"),
            started_at=0.0,
            baseline_turn_keys=set(),
            baseline_assistant_count=0,
            baseline_latest_assistant_key=None,
            baseline_latest_assistant_hash=None,
        )
        adapter.active_generation = tracker
        adapter._install_generation_observer(tracker)

        response = adapter.cancel("advice-cancel-root")

        self.assertEqual(response.status, "CANCELLED")
        self.assertTrue(adapter.disconnected)
        self.assertEqual(tracker.terminal_state, "CANCELLED")

    def test_no_definitive_submission_signal_degrades_before_response_timeout(self) -> None:
        config = dataclasses.replace(
            selectors(),
            submission_confirmation_timeout_seconds=0.75,
            timeout_seconds=30.0,
        )
        adapter = EmptyChatBootstrapAdapter(
            [
                {
                    "ok": True,
                    "rootFound": True,
                    "lifecycle": "WAITING_FOR_USER_TURN",
                    "assistantText": "",
                    "mutationCount": 1,
                    "sawUserTurn": False,
                    "sawAssistantTurn": False,
                    "sawAssistantTextChange": False,
                    "sawGenerationActive": False,
                    "controls": {"sendVisible": True, "sendEnabled": True, "stopVisible": False},
                    "turnKeys": [],
                    "assistantCount": 0,
                }
                for _ in range(8)
            ]
        )
        adapter.selectors = config

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "DEGRADED")
        self.assertLess(adapter.clock, config.timeout_seconds)
        self.assertTrue(adapter.disconnected)

    def test_generation_tracker_hash_updates_for_character_and_child_mutations(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("tracker-profile"), headed=True)
        tracker = GenerationTracker(
            generation_id="gen-1",
            request_id="advice-1",
            submitted_prompt_hash=sha256_text("prompt"),
            started_at=0.0,
            baseline_turn_keys={"old"},
            baseline_assistant_count=1,
            baseline_latest_assistant_key="old",
            baseline_latest_assistant_hash=sha256_text("old"),
        )

        adapter._update_tracker_from_browser_state(
            tracker,
            {
                "ok": True,
                "detectedUserTurnKey": "user-1",
                "detectedAssistantTurnKey": "assistant-1",
                "assistantText": "first paragraph",
                "mutationCount": 1,
                "sawUserTurn": True,
                "sawAssistantTurn": True,
                "sawGenerationActive": True,
            },
        )
        first_hash = tracker.assistant_text_hash
        adapter._update_tracker_from_browser_state(
            tracker,
            {
                "ok": True,
                "detectedUserTurnKey": "user-1",
                "detectedAssistantTurnKey": "assistant-1",
                "assistantText": "first paragraph\n\nnew code block",
                "mutationCount": 2,
                "sawUserTurn": True,
                "sawAssistantTurn": True,
                "sawGenerationActive": True,
            },
        )

        self.assertNotEqual(first_hash, tracker.assistant_text_hash)
        self.assertEqual(tracker.mutation_count, 2)
        self.assertTrue(tracker.saw_assistant_text_change)

    def test_generation_completion_requires_ready_controls_stability_and_hashes(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("tracker-profile"), headed=True)
        tracker = GenerationTracker(
            generation_id="gen-2",
            request_id="advice-2",
            submitted_prompt_hash=sha256_text("prompt"),
            started_at=0.0,
            baseline_turn_keys={"old"},
            baseline_assistant_count=1,
            baseline_latest_assistant_key="old",
            baseline_latest_assistant_hash=sha256_text("old"),
        )
        request = type("Request", (), {"specific_question": "question"})()

        tracker.detected_assistant_turn_key = "assistant-2"
        tracker.saw_assistant_turn = True
        tracker.saw_assistant_text_change = True
        tracker.assistant_text = "complete answer"
        tracker.assistant_text_hash = sha256_text("complete answer")
        tracker.last_text_change_at = 0.0

        self.assertFalse(adapter._response_is_invalid_for_generation("complete answer", request, tracker))
        self.assertTrue(adapter._response_is_invalid_for_generation("question", request, tracker))
        self.assertTrue(adapter._response_is_invalid_for_generation("old", request, tracker))

    def test_completed_stable_response_allows_empty_composer_with_disabled_send(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("completion-profile"), headed=True)
        tracker = completion_tracker()
        tracker.generation_inactive_sample_count = 3
        request = type("Request", (), {"specific_question": "different question"})()
        controls = completion_controls(sendEnabled=False, composerValueLength=0)

        self.assertTrue(adapter._composer_usable_for_completion(controls))
        conditions = adapter._generation_completion_conditions(
            request,
            tracker,
            "finished answer",
            5.0,
            3,
            False,
            adapter._composer_usable_for_completion(controls),
            False,
        )

        self.assertTrue(all(conditions.values()))

    def test_disabled_send_does_not_automatically_mean_generation_active(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("completion-profile"), headed=True)
        controls = completion_controls(sendEnabled=False, composerValueLength=0)

        self.assertTrue(adapter._composer_usable_for_completion(controls))

    def test_current_assistant_action_row_confirms_generation_completion(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("completion-profile"), headed=True)
        tracker = completion_tracker()
        request = type("Request", (), {"specific_question": "different question"})()

        conditions = adapter._generation_completion_conditions(
            request,
            tracker,
            "finished answer",
            5.0,
            3,
            False,
            False,
            True,
        )

        self.assertTrue(conditions["generation_inactive_evidence"])

    def test_old_or_external_action_row_does_not_confirm_current_response(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("completion-profile"), headed=True)
        tracker = completion_tracker()
        request = type("Request", (), {"specific_question": "different question"})()

        conditions = adapter._generation_completion_conditions(
            request,
            tracker,
            "finished answer",
            5.0,
            3,
            False,
            False,
            False,
        )

        self.assertFalse(conditions["generation_inactive_evidence"])
        self.assertIn("turn.querySelectorAll('button,[role=\"button\"]')", DEEPSEEK_GENERATION_STATE_SCRIPT)
        self.assertIn("compareDocumentPosition", DEEPSEEK_GENERATION_STATE_SCRIPT)

    def test_temporarily_stable_streaming_text_without_inactive_evidence_does_not_complete(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("completion-profile"), headed=True)
        tracker = completion_tracker()
        request = type("Request", (), {"specific_question": "different question"})()

        conditions = adapter._generation_completion_conditions(
            request,
            tracker,
            "draft answer",
            5.0,
            3,
            False,
            False,
            False,
        )

        self.assertFalse(all(conditions.values()))
        self.assertFalse(conditions["generation_inactive_evidence"])

    def test_three_inactive_composer_samples_complete_without_action_row(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("completion-profile"), headed=True)
        tracker = completion_tracker()
        tracker.generation_inactive_sample_count = 3
        request = type("Request", (), {"specific_question": "different question"})()

        conditions = adapter._generation_completion_conditions(
            request,
            tracker,
            "finished answer",
            5.0,
            3,
            False,
            True,
            False,
        )

        self.assertTrue(conditions["generation_inactive_evidence"])
        self.assertTrue(all(conditions.values()))

    def test_completion_requires_five_seconds_and_three_stable_hash_samples(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("completion-profile"), headed=True)
        adapter.selectors = dataclasses.replace(
            adapter.selectors,
            text_stability_seconds=5.0,
            stable_sample_count=3,
        )
        tracker = completion_tracker()
        tracker.generation_inactive_sample_count = 3
        request = type("Request", (), {"specific_question": "different question"})()

        too_early = adapter._generation_completion_conditions(
            request,
            tracker,
            "finished answer",
            4.9,
            3,
            False,
            True,
            False,
        )
        too_few_samples = adapter._generation_completion_conditions(
            request,
            tracker,
            "finished answer",
            5.0,
            2,
            False,
            True,
            False,
        )

        self.assertFalse(too_early["stable_for_required_seconds"])
        self.assertFalse(too_few_samples["stable_hash_sample_count"])

    def test_current_live_timeout_diagnostic_shape_reaches_completed_when_inactive(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("completion-profile"), headed=True)
        tracker = completion_tracker()
        tracker.generation_inactive_sample_count = 3
        tracker.assistant_text = "x" * 68
        tracker.assistant_text_hash = sha256_text(tracker.assistant_text)
        tracker.mutation_count = 1075
        request = type("Request", (), {"specific_question": "different question"})()

        conditions = adapter._generation_completion_conditions(
            request,
            tracker,
            tracker.assistant_text,
            86.0,
            172,
            False,
            True,
            False,
        )

        self.assertTrue(all(conditions.values()))

    def test_timeout_diagnostics_identify_blocked_completion_condition(self) -> None:
        adapter = EmptyChatBootstrapAdapter(
            [
                {
                    "ok": True,
                    "rootFound": True,
                    "lifecycle": "STABILIZING",
                    "detectedUserTurnKey": "user-timeout",
                    "detectedAssistantTurnKey": "assistant-timeout",
                    "assistantText": "stable but still blocked",
                    "mutationCount": 10,
                    "sawUserTurn": True,
                    "sawAssistantTurn": True,
                    "sawAssistantTextChange": True,
                    "sawGenerationActive": True,
                    "controls": completion_controls(composerVisible=False),
                    "turnKeys": ["user-timeout", "assistant-timeout"],
                    "assistantCount": 1,
                    "assistantActionRowVisible": False,
                }
                for _ in range(8)
            ]
        )
        adapter.selectors = dataclasses.replace(
            adapter.selectors,
            text_stability_seconds=5.0,
            stable_sample_count=3,
            timeout_seconds=1.5,
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "TIMED_OUT")
        diagnostics = json.loads(response.assumptions_or_questions[0])
        self.assertIn("completion_blockers", diagnostics)
        self.assertFalse(diagnostics["completion_blockers"]["generation_inactive_evidence"])
        self.assertIn("composer_visible", diagnostics)

    def test_normalize_response_text_preserves_structure(self) -> None:
        text = normalize_response_text("  Heading\n\n\n- item one\n- item two\n\n```x```  ")

        self.assertEqual(text, "Heading\n\n- item one\n- item two\n\n```x```")

    def test_rate_limit_text_elsewhere_is_ignored_but_visible_toast_is_honored(self) -> None:
        harmless_text = inline_snapshot(
            """
            <main>
              <section data-ds-role="assistant-response">The words rate limit are documentation only.</section>
              <button aria-label="Send">Send</button>
            </main>
            """
        )
        toast = inline_snapshot(
            """
            <main>
              <div data-testid="rate-limit-toast">provider notice</div>
              <button aria-label="Send">Send</button>
            </main>
            """
        )

        self.assertFalse(harmless_text.any_selector(selectors().rate_limit_selectors)[0])
        self.assertTrue(toast.any_selector(selectors().rate_limit_selectors)[0])

    def test_cancellation_disconnects_generation_observers_once(self) -> None:
        adapter = DisconnectRecordingAdapter()

        response = adapter.cancel("advice-cancel")

        self.assertEqual(response.status, "CANCELLED")
        self.assertTrue(adapter.disconnected)
        self.assertTrue(adapter.active_generation.cancellation_event.is_set())
        self.assertEqual(adapter.active_generation.terminal_state, "CANCELLED")

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

    def test_submission_not_confirmed_returns_degraded(self) -> None:
        adapter = OfflineAdapter(
            [
                snapshot("ready_prompt.html"),
                snapshot("ready_prompt.html"),
                snapshot("ready_prompt.html"),
                snapshot("ready_prompt.html"),
                snapshot("ready_prompt.html"),
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "DEGRADED")
        self.assertEqual(adapter.state, AdapterState.DEGRADED)
        self.assertIn("not confirmed", response.diagnosis)

    def test_input_clearing_alone_does_not_confirm_submission(self) -> None:
        adapter = PromptClearedAdapter(
            [
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "DEGRADED")
        self.assertNotIn("prompt_empty", adapter.last_submission_diagnostics)

    def test_new_user_message_confirms_submission_but_is_not_returned(self) -> None:
        user_only = inline_snapshot(
            """
            <main>
              <section data-testid="user-message">submitted prompt should not return</section>
              <textarea aria-label="Message DeepSeek"></textarea>
              <button aria-label="Send">Send</button>
            </main>
            """
        )
        adapter = OfflineAdapter(
            [
                snapshot("ready_prompt.html"),
                snapshot("ready_prompt.html"),
                user_only,
                user_only,
                user_only,
                user_only,
                user_only,
            ]
        )

        response = adapter.advise(valid_request())

        self.assertNotEqual(response.status, "COMPLETED")
        self.assertNotIn("submitted prompt", response.diagnosis)

    def test_existing_response_flow_does_not_require_stop_signal(self) -> None:
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
        self.assertNotIn("stop_visible", adapter.last_submission_diagnostics)

    def test_advisory_turn_rejects_empty_new_response(self) -> None:
        adapter = PromptClearedAdapter(
            [
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
                snapshot("baseline_streaming.html"),
                snapshot("empty_completed.html"),
                snapshot("empty_completed.html"),
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "FAILED")

    def test_advisory_turn_rejects_stale_baseline_response(self) -> None:
        stale = inline_snapshot(
            """
            <main>
              <section data-ds-role="assistant-response">old response should stay out</section>
              <section data-ds-role="assistant-response">old response should stay out</section>
              <textarea aria-label="Message DeepSeek"></textarea>
              <button aria-label="Send">Send</button>
            </main>
            """
        )
        adapter = OfflineAdapter(
            [
                snapshot("baseline_before.html"),
                snapshot("baseline_before.html"),
                stale,
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "FAILED")

    def test_nested_whole_conversation_container_is_rejected(self) -> None:
        nested = inline_snapshot(
            """
            <main>
              <section data-testid="user-message">submitted prompt should not return</section>
              <section data-testid="assistant-message">
                submitted prompt should not return
                nested answer must not be taken from a parent container
              </section>
              <textarea aria-label="Message DeepSeek"></textarea>
              <button aria-label="Send">Send</button>
            </main>
            """
        )
        adapter = PromptClearedAdapter(
            [
                snapshot("ready_prompt.html"),
                snapshot("ready_prompt.html"),
                nested,
                nested,
                nested,
            ]
        )

        response = adapter.advise(valid_request())

        self.assertNotEqual(response.status, "COMPLETED")
        self.assertNotIn("submitted prompt", response.diagnosis)

    def test_newest_assistant_element_is_returned(self) -> None:
        newest = inline_snapshot(
            """
            <main>
              <section data-testid="assistant-message">old assistant response</section>
              <section data-testid="user-message">synthetic user prompt</section>
              <section data-testid="assistant-message">first new assistant draft</section>
              <section data-testid="assistant-message">newest final assistant answer</section>
              <textarea aria-label="Message DeepSeek"></textarea>
              <button aria-label="Send">Send</button>
            </main>
            """
        )
        baseline = inline_snapshot(
            """
            <main>
              <section data-testid="assistant-message">old assistant response</section>
              <textarea aria-label="Message DeepSeek"></textarea>
              <button aria-label="Send">Send</button>
            </main>
            """
        )
        adapter = PromptClearedAdapter(
            [
                baseline,
                baseline,
                newest,
                newest,
                newest,
                newest,
            ]
        )

        response = adapter.advise(valid_request())

        self.assertEqual(response.status, "COMPLETED")
        self.assertIn("newest final assistant answer", response.diagnosis)
        self.assertNotIn("first new assistant draft", response.diagnosis)
        self.assertNotIn("synthetic user prompt", response.diagnosis)

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
        typed = "".join(
            call.split(":", 2)[2]
            for call in fake.calls
            if call.startswith("press_keys:textarea:")
        )
        self.assertEqual(typed, "synthetic prompt")

        retained = PromptFakeSb(retained_value="retained draft")
        adapter._sb = retained
        with self.assertRaises(RuntimeError):
            adapter._type_prompt("textarea", "synthetic prompt")

    def test_paced_typing_prefers_native_keyboard_input(self) -> None:
        adapter = DeepSeekWebAdvisorAdapter(selectors(), Path("prompt-profile"), headed=True)
        fake = PromptScriptFakeSb()
        adapter._sb = fake

        adapter._type_prompt("textarea", "abc")

        self.assertFalse(any(call.startswith("execute_script:") for call in fake.calls))
        self.assertTrue(any(call.startswith("press_keys:") for call in fake.calls))

    def test_cookie_banner_prefers_exact_reject_selector_on_trusted_origin(self) -> None:
        page = inline_snapshot(
            """
            <main>
              <div id="onetrust-banner-sdk">
                <button id="onetrust-reject-all-handler">Reject</button>
                <button id="onetrust-accept-btn-handler">Accept</button>
              </div>
            </main>
            """
        )
        adapter = CookieAdapter(page)

        self.assertTrue(adapter._handle_cookie_banner(page))

        self.assertEqual(adapter.clicked_selectors, ["#onetrust-reject-all-handler"])

    def test_cookie_banner_does_not_click_on_untrusted_origin(self) -> None:
        page = inline_snapshot(
            """
            <main>
              <div id="onetrust-banner-sdk">
                <button id="onetrust-reject-all-handler">Reject</button>
              </div>
            </main>
            """,
            url="https://example.invalid",
        )
        adapter = CookieAdapter(page)

        self.assertFalse(adapter._handle_cookie_banner(page))

        self.assertEqual(adapter.clicked_selectors, [])

    def test_paced_typing_uses_deterministic_delays_and_checks_cancellation(self) -> None:
        adapter = PacedTypingAdapter()

        adapter._type_text_paced("textarea", "ab\nc")

        self.assertEqual(len(adapter.sleeps), 4)
        self.assertEqual(adapter.sleeps[2], selectors().typing_newline_pause_min_seconds)
        self.assertTrue(
            all(
                selectors().typing_min_interval_seconds <= delay <= selectors().typing_max_interval_seconds
                for index, delay in enumerate(adapter.sleeps)
                if index != 2
            )
        )

        adapter.cancel_requested.set()
        before = len(adapter.sleeps)
        adapter._type_text_paced("textarea", "will-not-type")
        self.assertEqual(len(adapter.sleeps), before)

    def test_paced_typing_default_range_is_slower(self) -> None:
        config = SelectorConfig.from_file(SELECTOR_CONFIG)

        self.assertGreaterEqual(config.typing_min_interval_seconds, 0.03)
        self.assertLessEqual(config.typing_max_interval_seconds, 0.09)
        self.assertGreaterEqual(config.typing_newline_pause_min_seconds, 0.15)
        self.assertLessEqual(config.typing_newline_pause_max_seconds, 0.35)

    def test_paced_typing_enforces_total_timeout(self) -> None:
        config = dataclasses.replace(selectors(), typing_timeout_seconds=0.015)
        adapter = DeepSeekWebAdvisorAdapter(config, Path("timeout-profile"), headed=True)
        adapter._sb = PromptFakeSb(retained_value="")
        adapter._sleep = lambda seconds: setattr(adapter, "_forced_time", getattr(adapter, "_forced_time", 0.0) + seconds)  # type: ignore[method-assign]
        adapter._now = lambda: getattr(adapter, "_forced_time", 0.0)  # type: ignore[method-assign]

        with self.assertRaises(TimeoutError):
            adapter._type_text_paced("textarea", "abcdef")

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
        self.assertEqual(cancelled["request_id"], "advice-block")
        self.assertEqual(cancelled["response"]["request_id"], "advice-block")
        protocol._join_active(1.0)
        self.assertEqual(events, [])

    def test_protocol_rejects_non_ready_states_before_typing(self) -> None:
        for state in [
            AdapterState.RATE_LIMITED,
            AdapterState.CANCELLED,
            AdapterState.DEGRADED,
            AdapterState.TAKEOVER_REQUIRED,
            AdapterState.LOGIN_REQUIRED,
        ]:
            with self.subTest(state=state):
                protocol = JsonLinesAdvisorProtocol(StateAdapter(state), "secret-token")
                rejected = protocol.handle(
                    {
                        "command": "advise",
                        "auth_token": "secret-token",
                        "request": valid_request(requestId=f"advice-{state.value.lower()}"),
                    }
                )
                self.assertFalse(rejected["ok"])
                self.assertEqual(rejected["error"], f"ADVISOR_NOT_READY:{state.value}")

    def test_protocol_cancel_handles_stop_button_failure(self) -> None:
        adapter = FailingStopAdapter()

        cancelled = adapter.cancel("advice-stop-failure")

        self.assertEqual(cancelled.status, "CANCELLED")
        self.assertEqual(cancelled.request_id, "advice-stop-failure")
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
