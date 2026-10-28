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
                    mock.patch.object(self.probe.time, "monotonic", side_effect=[0, 10]), \
                    contextlib.redirect_stdout(output):
                self.assertEqual(self.probe.main(), 0)
            self.assertEqual(calls, [{"uc": True, "user_data_dir": profile}, target, "closed"])
            self.assertEqual(output.getvalue(), "PROBE:FINAL:EXACT_READY_IDLE\n")
        self.assertEqual(self.probe.PROFILE, default)


if __name__ == "__main__":
    unittest.main()
