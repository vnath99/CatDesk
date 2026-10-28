import contextlib
import importlib.util
import io
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest import mock


class ReadinessProbeTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location(
            "readiness_probe", Path(__file__).resolve().parents[1] / "scripts/wake_readiness_visual_probe.py")
        self.probe = importlib.util.module_from_spec(spec)
        with mock.patch.dict(os.environ, {"LOCALAPPDATA": tempfile.gettempdir()}):
            spec.loader.exec_module(self.probe)

    def test_inspection_reports_fixed_readiness_classes(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        for current, visible, expected in [
            (target, {"#prompt-textarea"}, "EXACT_READY_IDLE"),
            (target, {"#prompt-textarea", "button[data-testid='stop-button']"}, "EXACT_READY_BUSY"),
            (target, set(), "EXACT_NO_EDITOR"),
            ("https://chatgpt.com/auth/login", set(), "LOGIN_VISIBLE"),
            (target, {"iframe[title*='captcha' i]"}, "CAPTCHA_VISIBLE"),
            ("https://chatgpt.com/", set(), "HOME"),
            ("https://chatgpt.com/c/another", set(), "TARGET_DRIFT"),
            ("chrome-error://chromewebdata/", set(), "CHROME_ERROR"),
        ]:
            with self.subTest(expected=expected):
                cdp = types.SimpleNamespace(get_current_url=lambda: current,
                                            is_element_visible=lambda selector: selector in visible)
                self.assertEqual(self.probe.classify(cdp, target), expected)

    def test_handshake_precedes_runtime_import_and_browser_creation(self):
        argv = ["probe", "--await-start", "--conversation-url", "https://chatgpt.com/c/readiness-fixture"]
        with mock.patch.object(sys, "argv", argv), mock.patch.object(sys, "stdin", io.StringIO("NO\n")):
            self.assertEqual(self.probe.main(), 2)

    def test_development_owner_supplies_existing_profile_without_changing_default(self):
        default = self.probe.PROFILE
        target = "https://chatgpt.com/c/readiness-fixture"
        calls = []

        class Browser:
            def __enter__(self):
                self.cdp = types.SimpleNamespace(get_current_url=lambda: target,
                    is_element_visible=lambda selector: selector == "#prompt-textarea")
                return self
            def __exit__(self, *args):
                calls.append("closed")
            def activate_cdp_mode(self, url):
                calls.append(url)

        def factory(**kwargs):
            calls.append(kwargs)
            return Browser()

        with tempfile.TemporaryDirectory() as profile:
            argv = ["probe", "--await-start", "--profile", profile,
                    "--conversation-url", target, "--seconds", "5"]
            output = io.StringIO()
            with mock.patch.object(sys, "argv", argv), mock.patch.object(sys, "stdin", io.StringIO("START\n")), \
                    mock.patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=factory)}), \
                    mock.patch.object(self.probe.time, "monotonic", side_effect=[0, 0, 10]), \
                    contextlib.redirect_stdout(output):
                self.assertEqual(self.probe.main(), 0)
            self.assertEqual(calls, [{"uc": True, "user_data_dir": profile}, target, "closed"])
            self.assertEqual(output.getvalue(), "PROBE:FINAL:EXACT_READY_IDLE\n")
        self.assertEqual(self.probe.PROFILE, default)

    def test_home_recovery_requires_verified_empty_composer(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        for empty in [True, False, None, "true", {"value": True}]:
            with self.subTest(empty=empty):
                cdp = mock.Mock()
                cdp.get_current_url.return_value = "https://chatgpt.com/"
                cdp.is_element_visible.return_value = False
                cdp.evaluate.return_value = empty
                self.probe.recover_empty_home(cdp, target)
                self.assertEqual(cdp.get.call_args_list, [mock.call(target)] if empty is True else [])

    def test_recovery_refuses_login_security_drift_busy_and_unknown_state(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        for current, visible in [
                ("https://chatgpt.com/auth/login", set()),
                ("https://chatgpt.com/", {"iframe[title*='captcha' i]"}),
                ("https://chatgpt.com/", {"button[data-testid='stop-button']"}),
                ("https://chatgpt.com/c/other", set()),
                (target, {"#prompt-textarea"})]:
            with self.subTest(current=current, visible=visible):
                cdp = mock.Mock()
                cdp.get_current_url.return_value = current
                cdp.is_element_visible.side_effect = lambda selector: selector in visible
                cdp.evaluate.return_value = True
                self.probe.recover_empty_home(cdp, target)
                cdp.get.assert_not_called()
        cdp = mock.Mock()
        cdp.get_current_url.return_value = "https://chatgpt.com/"
        cdp.is_element_visible.return_value = False
        cdp.evaluate.side_effect = RuntimeError("unavailable")
        self.probe.recover_empty_home(cdp, target)
        cdp.get.assert_not_called()

    def test_home_navigation_is_attempted_only_once_within_original_deadline(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        cdp = mock.Mock()
        cdp.get_current_url.return_value = "https://chatgpt.com/"
        cdp.is_element_visible.return_value = False
        cdp.evaluate.return_value = True
        browser = mock.MagicMock()
        browser.__enter__.return_value = browser
        browser.cdp = cdp
        clock = [0]

        def sleep(seconds):
            clock[0] += seconds

        with tempfile.TemporaryDirectory() as profile:
            argv = ["probe", "--profile", profile, "--conversation-url", target, "--seconds", "5"]
            with mock.patch.object(sys, "argv", argv), \
                    mock.patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=lambda **kwargs: browser)}), \
                    mock.patch.object(self.probe.time, "monotonic", side_effect=lambda: clock[0]), \
                    mock.patch.object(self.probe.time, "sleep", side_effect=sleep), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(self.probe.main(), 0)
        cdp.get.assert_called_once_with(target)
        self.assertEqual(clock[0], 5)


if __name__ == "__main__":
    unittest.main()
