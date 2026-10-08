import hashlib
import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "stable_wake_browser_adapter",
    # Unit-test the version-controlled source. Deployed runtime identity and
    # presence are separate release/installation acceptance gates.
    Path(__file__).parents[1] / "scripts" / "stable_wake_browser_adapter.py",
)
adapter = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = adapter
SPEC.loader.exec_module(adapter)


class FakeBridge:
    @staticmethod
    def canonical_conversation_url(value):
        if value != "https://chatgpt.com/c/exact-thread":
            raise ValueError("target invalid")
        return value


class StableWakeAdapterProfileTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.profile = self.root / ".catdesk" / "wake-bridge" / "browser-profile"
        self.profile.mkdir(parents=True)
        self.config = self.root / ".catdesk" / "wake-bridge" / "config.json"
        self.target = "https://chatgpt.com/c/exact-thread"
        self.target_hash = hashlib.sha256(self.target.encode("utf-8")).hexdigest()

    def tearDown(self):
        self.temp.cleanup()

    def write_config(self, profile):
        self.config.write_text(
            json.dumps(
                {
                    "conversation_url": self.target,
                    "profile_dir": profile,
                    "ui_ready_timeout_seconds": 20,
                    "send_confirmation_timeout_seconds": 15,
                }
            ),
            encoding="utf-8",
        )

    def test_exact_relative_and_deployed_absolute_profile_are_the_only_successes(self):
        self.assertEqual(
            adapter.validate_exact_profile(".catdesk/wake-bridge/browser-profile", self.root),
            self.profile.resolve(),
        )
        self.assertEqual(
            adapter.validate_exact_profile(str(self.profile.resolve()), self.root),
            self.profile.resolve(),
        )
        self.write_config(str(self.profile.resolve()))
        before = self.config.read_bytes()
        target, profile, ready, submit = adapter.load_exact_config(FakeBridge, self.target_hash, self.root)
        self.assertEqual((target, profile, ready, submit), (self.target, self.profile.resolve(), 20.0, 15.0))
        self.assertEqual(self.config.read_bytes(), before)
        self.assertNotIn("seleniumbase", sys.modules)

    def test_arbitrary_sibling_outside_traversal_and_prefix_forms_fail_closed(self):
        sibling = self.root / ".catdesk" / "wake-bridge" / "other-profile"
        sibling.mkdir()
        outside = self.root.parent
        invalid = [
            ".catdesk/wake-bridge/./browser-profile",
            ".catdesk/wake-bridge/../wake-bridge/browser-profile",
            ".catdesk/wake-bridge/other-profile",
            str(sibling),
            str(outside),
            "",
            "\\\\server\\share\\browser-profile",
            "\\\\?\\C:\\browser-profile",
            "\\\\.\\C:\\browser-profile",
            "C:\\outside\\browser-profile",
        ]
        for value in invalid:
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    adapter.validate_exact_profile(value, self.root)

    def test_oversize_wrong_type_target_and_target_drift_fail_without_browser(self):
        for value in ["x" * 1025, 12, self.profile / "nested"]:
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    adapter.validate_exact_profile(value, self.root)
        wrong_type = self.root / ".catdesk" / "wake-bridge" / "wrong-type"
        wrong_type.write_text("not a profile", encoding="utf-8")
        with self.assertRaises(ValueError):
            adapter.validate_exact_profile(str(wrong_type), self.root)
        self.write_config(str(self.profile.resolve()))
        with self.assertRaises(ValueError):
            adapter.load_exact_config(FakeBridge, "0" * 64, self.root)
        self.config.write_text(
            json.dumps({"conversation_url": "https://chatgpt.com/c/drifted", "profile_dir": str(self.profile.resolve())}),
            encoding="utf-8",
        )
        with self.assertRaises(ValueError):
            adapter.load_exact_config(FakeBridge, self.target_hash, self.root)

    def test_link_escape_is_refused_when_the_platform_allows_fixture_links(self):
        escaped = self.root / ".catdesk" / "wake-bridge" / "browser-profile"
        outside = self.root / "outside-profile"
        outside.mkdir()
        escaped.rmdir()
        try:
            escaped.symlink_to(outside, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("profile symlink creation unavailable")
        with self.assertRaises(ValueError):
            adapter.validate_exact_profile(str(escaped), self.root)

    def test_windows_junction_and_reparse_metadata_are_fail_closed(self):
        """Exercise Windows-only checks deterministically without a live junction."""
        safe = SimpleNamespace(st_mode=0o040755, st_file_attributes=0)
        reparse = SimpleNamespace(
            st_mode=0o040755,
            st_file_attributes=getattr(adapter.stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400),
        )
        path = self.profile
        with mock.patch.object(adapter.stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400, create=True), mock.patch.object(
            adapter.os.path, "islink", return_value=False
        ), mock.patch.object(adapter.os.path, "isjunction", return_value=False, create=True):
            adapter._assert_directory_component_is_safe(path, safe, windows=True)
            with self.assertRaises(ValueError):
                adapter._assert_directory_component_is_safe(path, reparse, windows=True)
        with mock.patch.object(adapter.stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400, create=True), mock.patch.object(
            adapter.os.path, "islink", return_value=False
        ), mock.patch.object(adapter.os.path, "isjunction", return_value=True, create=True):
            with self.assertRaises(ValueError):
                adapter._assert_directory_component_is_safe(path, safe, windows=True)

    def test_windows_missing_or_inconsistent_directory_metadata_fails_closed(self):
        path = self.profile
        missing = SimpleNamespace(st_mode=0o040755)
        not_directory = SimpleNamespace(st_mode=0o100644, st_file_attributes=0)
        with mock.patch.object(adapter.stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400, create=True), mock.patch.object(
            adapter.os.path, "islink", return_value=False
        ), mock.patch.object(adapter.os.path, "isjunction", return_value=False, create=True):
            with self.assertRaises(ValueError):
                adapter._assert_directory_component_is_safe(path, missing, windows=True)
            with self.assertRaises(ValueError):
                adapter._assert_directory_component_is_safe(path, not_directory, windows=True)


if __name__ == "__main__":
    unittest.main()
