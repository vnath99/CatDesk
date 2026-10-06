import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).parents[1]


def load_module(name, relative_path):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


WEBDRIVER = load_module("wake_browser_smoke_test", "scripts/wake_browser_smoke_test.py")
CDP = load_module("wake_browser_smoke_test_cdp", "scripts/wake_browser_smoke_test_cdp.py")


class Clock:
    def __init__(self):
        self.now = 0.0
        self.sleeps = []

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += seconds


class EmptyEditor:
    def get_attribute(self, _name):
        return ""


class SmokeStopStateTests(unittest.TestCase):
    def test_pre_typing_stop_wait_then_idle_and_empty_recheck(self):
        clock = Clock()
        stop_states = iter([True, False])
        with patch.object(CDP, "network_error_page", return_value=False), patch.object(
            CDP, "stop_visible", side_effect=lambda _cdp: next(stop_states)
        ):
            CDP.wait_for_idle(object(), 2, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual([0.5], clock.sleeps)

        class Cdp:
            def get_current_url(self):
                return "https://chatgpt.com/c/exact"

            def evaluate(self, _script):
                return ["ready", True]

        with patch.object(CDP, "stop_visible", return_value=False):
            CDP.require_pre_typing_ready(Cdp(), "https://chatgpt.com/c/exact")

    def test_pre_typing_stop_timeout_fails_closed(self):
        clock = Clock()
        with patch.object(CDP, "network_error_page", return_value=False), patch.object(CDP, "stop_visible", return_value=True):
            with self.assertRaisesRegex(TimeoutError, "CHATGPT_STILL_GENERATING"):
                CDP.wait_for_idle(object(), 1, monotonic=clock.monotonic, sleeper=clock.sleep)

    def test_send_readiness_waits_for_enabled_unique_control(self):
        clock = Clock()
        states = iter([
            ("disabled", None),
            ("ready", "button#composer-submit-button"),
        ])
        with patch.object(CDP, "send_control_state", side_effect=lambda _cdp: next(states)):
            selector = CDP.find_send_selector(object(), 2, monotonic=clock.monotonic, sleeper=clock.sleep)
        self.assertEqual("button#composer-submit-button", selector)
        self.assertEqual([0.25], clock.sleeps)

    def test_disabled_send_timeout_and_ambiguous_send_fail_closed(self):
        clock = Clock()
        with patch.object(CDP, "send_control_state", return_value=("disabled", None)):
            with self.assertRaisesRegex(TimeoutError, "SEND_NOT_READY"):
                CDP.find_send_selector(object(), 1, monotonic=clock.monotonic, sleeper=clock.sleep)
        with patch.object(CDP, "send_control_state", return_value=("ambiguous", None)):
            with self.assertRaisesRegex(RuntimeError, "SEND_NOT_UNIQUE"):
                CDP.find_send_selector(object(), 1, monotonic=Clock().monotonic, sleeper=Clock().sleep)

    def test_native_enter_fallback_requires_a_bounded_absence_of_send_controls(self):
        clock = Clock()
        with patch.object(CDP, "send_control_state", return_value=("missing", None)):
            self.assertIsNone(CDP.find_send_selector(object(), 1, monotonic=clock.monotonic, sleeper=clock.sleep))
        self.assertGreaterEqual(sum(clock.sleeps), 1)

    def test_single_submit_has_no_post_boundary_reclick(self):
        class Cdp:
            def __init__(self):
                self.clicks = []
                self.keys = []

            def click(self, selector, timeout):
                self.clicks.append((selector, timeout))

            def press_keys(self, selector, text, timeout):
                self.keys.append((selector, text, timeout))

        class SB:
            def __init__(self):
                self.cdp = Cdp()

            def activate_cdp_mode(self, _url):
                pass

        sb = SB()
        with patch.object(CDP, "wait_for_ready"), patch.object(CDP, "wait_for_idle"), patch.object(
            CDP, "require_pre_typing_ready"
        ), patch.object(CDP, "find_send_selector", return_value="button#composer-submit-button"), patch.object(CDP, "observe_post_submit_state"):
            CDP.run_once(sb, "https://chatgpt.com/c/exact", "marker", 5, 0, 0)
        self.assertEqual([("button#composer-submit-button", 10)], sb.cdp.clicks)
        self.assertEqual([(CDP.EDITOR_SELECTOR, "marker", 5)], sb.cdp.keys)

    def test_post_submit_stop_observation_holds_browser_two_seconds(self):
        clock = Clock()
        stop_calls = 0

        def visible(_sb, selector):
            nonlocal stop_calls
            if selector == WEBDRIVER.STOP_SELECTOR:
                stop_calls += 1
                return [object()] if stop_calls == 1 else []
            return [EmptyEditor()]

        with patch.object(WEBDRIVER, "one_visible", side_effect=visible), patch.object(WEBDRIVER, "editor_text", return_value=""):
            self.assertTrue(WEBDRIVER.observe_post_submit_state(object(), 5, monotonic=clock.monotonic, sleeper=clock.sleep))
        self.assertGreaterEqual(sum(clock.sleeps), WEBDRIVER.POST_STOP_HOLD_SECONDS)

    def test_transient_or_missed_stop_does_not_false_fail_after_clear(self):
        clock = Clock()
        with patch.object(WEBDRIVER, "one_visible", side_effect=lambda _sb, selector: [] if selector == WEBDRIVER.STOP_SELECTOR else [EmptyEditor()]), patch.object(
            WEBDRIVER, "editor_text", return_value=""
        ):
            self.assertTrue(WEBDRIVER.observe_post_submit_state(object(), 1, monotonic=clock.monotonic, sleeper=clock.sleep))
        self.assertEqual([], clock.sleeps)

    def test_webdriver_disabled_send_never_becomes_enter_fallback(self):
        class DisabledButton:
            def is_enabled(self):
                return False

            def get_attribute(self, _name):
                return None

        clock = Clock()
        with patch.object(WEBDRIVER, "one_visible", return_value=[DisabledButton()]):
            with self.assertRaisesRegex(TimeoutError, "SEND_NOT_READY"):
                WEBDRIVER.wait_for_send(object(), 1, monotonic=clock.monotonic, sleeper=clock.sleep)

    def test_cdp_smoke_does_not_enable_exception_suppressing_test_mode(self):
        source = (ROOT / "scripts" / "wake_browser_smoke_test_cdp.py").read_text(encoding="utf-8")
        self.assertIn("with SB(uc=True, user_data_dir=str(profile))", source)
        self.assertNotIn("with SB(uc=True, test=True", source)


if __name__ == "__main__":
    unittest.main()
