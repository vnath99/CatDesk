import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location(
    "wake_profile_login", Path(__file__).parents[1] / "scripts" / "wake_profile_login.py"
)
wake_profile_login = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = wake_profile_login
SPEC.loader.exec_module(wake_profile_login)


class WakeProfileLoginTests(unittest.TestCase):
    def test_mcp_readiness_requires_the_fixed_operator_confirmation(self):
        self.assertTrue(wake_profile_login.mcp_ready_confirmation("MCP_READY"))
        self.assertFalse(wake_profile_login.mcp_ready_confirmation("mcp_ready"))
        self.assertFalse(wake_profile_login.mcp_ready_confirmation("MCP_READY extra"))

    def test_conversation_url_remains_exact_and_credential_free(self):
        self.assertEqual(
            "https://chatgpt.com/c/exact-thread",
            wake_profile_login.validate_conversation_url("https://chatgpt.com/c/exact-thread"),
        )
        with self.assertRaises(ValueError):
            wake_profile_login.validate_conversation_url("https://chatgpt.com/c/exact-thread?token=secret")
        for invalid in (
            "https://chatgpt.com/c/exact-thread/",
            "https://user@chatgpt.com/c/exact-thread",
            "https://chatgpt.com:444/c/exact-thread",
            "https://chatgpt.com/share/exact-thread",
        ):
            with self.assertRaises(ValueError, msg=invalid):
                wake_profile_login.validate_conversation_url(invalid)

    def test_mcp_receipt_contains_only_target_binding_and_confirmation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            profile = root / "profile"
            profile.mkdir()
            receipt = root / "mcp-ready.json"
            wake_profile_login.write_mcp_ready_receipt(
                receipt, "https://chatgpt.com/c/exact-thread", profile, confirmed_at_unix=100
            )
            saved = json.loads(receipt.read_text(encoding="utf-8"))
        self.assertEqual({"schema_version", "conversation_url", "profile_dir", "confirmed_at_unix", "mcp_ready"}, set(saved))
        self.assertTrue(saved["mcp_ready"])
        self.assertNotIn("credential", json.dumps(saved).lower())


if __name__ == "__main__":
    unittest.main()
