import importlib.util
import io
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "wake_development_login.py"
SPEC = importlib.util.spec_from_file_location("wake_development_login", SCRIPT)
login = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(login)


class FakeBrowser:
    def __init__(self, log, **kwargs):
        self.log = log
        self.log.append(("created", kwargs))

    def __enter__(self):
        self.log.append(("entered",))
        return self

    def activate_cdp_mode(self, url):
        self.log.append(("opened", url))

    def __exit__(self, *_):
        self.log.append(("closed",))


class DevelopmentLoginTests(unittest.TestCase):
    def test_exact_conversation_routes_only(self):
        for url in (
            "https://chatgpt.com/c/canary-1",
            "https://chat.openai.com/g/project-1/c/WEB:canary_2",
        ):
            self.assertEqual(login.validate_conversation_url(url), url)
        for url in (
            "https://chatgpt.com/",
            "https://chatgpt.com/c/a?x=1",
            "https://chatgpt.com/g/p/c/a/",
            "https://user@chatgpt.com/c/a",
            "https://chatgpt.com:443/c/a",
            "http://chatgpt.com/c/a",
            "https://other.example/c/a",
        ):
            with self.subTest(url=url), self.assertRaises(ValueError):
                login.validate_conversation_url(url)

    def test_start_handshake_and_enter_only_close_without_auth_claim(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            url = "https://chatgpt.com/g/project-1/c/canary-1"
            log = []
            output = io.StringIO()
            factory = lambda **kwargs: FakeBrowser(log, **kwargs)
            self.assertEqual(
                login.main([str(profile), url], io.StringIO("WRONG\n\n"), output, factory),
                2,
            )
            self.assertEqual(log, [])
            self.assertEqual(output.getvalue(), "")
            self.assertEqual(
                login.main([str(profile), url], io.StringIO("START\n\n"), output, factory),
                0,
            )
            self.assertEqual(log[2], ("opened", url))
            self.assertEqual(log[-1], ("closed",))
            self.assertEqual(
                output.getvalue(), "LOGIN_BROWSER_OPEN\nLOGIN_BROWSER_CLOSED\n"
            )
            self.assertNotIn("AUTHENTICATED", output.getvalue())

    def test_nonblank_acknowledgement_closes_browser_without_success(self):
        with tempfile.TemporaryDirectory() as directory:
            log = []
            output = io.StringIO()
            self.assertEqual(
                login.main(
                    [directory, "https://chatgpt.com/c/a"],
                    io.StringIO("START\nyes\n"),
                    output,
                    lambda **kwargs: FakeBrowser(log, **kwargs),
                ),
                2,
            )
            self.assertEqual(log[-1], ("closed",))
            self.assertEqual(
                output.getvalue(), "LOGIN_BROWSER_OPEN\nLOGIN_HELPER_ATTENTION\n"
            )


if __name__ == "__main__":
    unittest.main()
