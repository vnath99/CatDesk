import contextlib
import hashlib
import asyncio
import importlib.util
import io
import json
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

    def test_verified_primitives_are_shared_and_rejected_before_execution_on_hash_drift(self):
        path = Path(__file__).resolve().parents[1] / "scripts/wake_bridge.py"
        content = path.read_bytes()
        with tempfile.TemporaryDirectory() as profile:
            sink = self.probe.load_observation_sink(path, hashlib.sha256(content).hexdigest(),
                                                    Path(profile), "https://chatgpt.com/c/fixture")
            self.assertTrue(callable(sink.browser_surface_observation))
            with mock.patch("builtins.compile", side_effect=AssertionError("must not execute")):
                with self.assertRaises(ValueError):
                    self.probe.load_observation_sink(path, "0" * 64, Path(profile), "https://chatgpt.com/c/fixture")

    def test_surface_error_is_not_home_or_missing_editor(self):
        value = dict(route="EXACT", document="COMPLETE", editor="READY", composer="EMPTY", generation="IDLE", assistant="LOAD_ERROR")
        self.assertEqual(self.probe.surface_state(value), "EXACT_READY_RESPONSE_LOAD_ERROR")
        self.assertEqual(self.probe.surface_state(dict(value, generation="ACTIVE")), "EXACT_READY_BUSY")
        self.assertEqual(self.probe.surface_state(dict(value, route="HOME")), "HOME")
        self.assertEqual(self.probe.surface_state(dict(value, editor="MISSING")), "EXACT_NO_EDITOR")
        self.assertEqual(self.probe.surface_state(dict(value, composer="DRAFT_PRESENT")), "EXACT_DRAFT_PRESENT")
        self.assertEqual(self.probe.surface_state(dict(value, assistant="DELIVERY_TIMEOUT")), "EXACT_READY_RESPONSE_TIMEOUT")

    def test_surface_unknown_state_cannot_be_reported_as_ready_idle(self):
        value = dict(route="EXACT", document="COMPLETE", editor="READY", composer="EMPTY", generation="IDLE", assistant="PRESENT")
        for key in ("document", "generation", "composer", "assistant"):
            with self.subTest(key=key):
                self.assertEqual(self.probe.surface_state(dict(value, **{key: "UNKNOWN"})), "OBSERVATION_UNAVAILABLE")

    def test_readonly_inspection_never_recovers_home_or_exports_content(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        cdp = mock.Mock()
        cdp.get_current_url.return_value = "https://chatgpt.com/"
        cdp.get_tabs.return_value = []
        cdp.is_element_visible.return_value = False
        browser = mock.MagicMock()
        browser.cdp = cdp
        factory = mock.Mock(return_value=mock.MagicMock(__enter__=mock.Mock(return_value=browser), __exit__=mock.Mock(return_value=False)))
        elapsed = [0.0]
        def sleep(seconds): elapsed[0] += seconds
        with tempfile.TemporaryDirectory() as profile:
            argv = ["probe", "--read-only", "--profile", profile, "--conversation-url", target, "--seconds", "5"]
            output = io.StringIO()
            with mock.patch.object(sys, "argv", argv), mock.patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=factory)}), \
                    mock.patch.object(self.probe.time, "monotonic", side_effect=lambda: elapsed[0]), \
                    mock.patch.object(self.probe.time, "sleep", side_effect=sleep), contextlib.redirect_stdout(output):
                self.assertEqual(self.probe.main(), 0)
            cdp.get.assert_not_called()
            cdp.evaluate.assert_not_called()
            self.assertNotIn("HOME_CHECK", output.getvalue())
            self.assertIn("PROBE:FINAL:HOME", output.getvalue())

    def test_handshake_precedes_runtime_import_and_browser_creation(self):
        argv = ["probe", "--await-start", "--conversation-url", "https://chatgpt.com/c/readiness-fixture"]
        with mock.patch.object(sys, "argv", argv), mock.patch.object(sys, "stdin", io.StringIO("NO\n")):
            self.assertEqual(self.probe.main(), 2)

    def test_network_detail_reads_only_an_internal_error_document(self):
        cdp = mock.Mock()
        for current in ["https://chatgpt.com/c/readiness-fixture", "https://chatgpt.com/",
                        "https://other.example/", "about:blank"]:
            cdp.get_current_url.return_value = current
            self.assertEqual(self.probe.chrome_error_detail(cdp), "UNKNOWN")
        cdp.evaluate.assert_not_called()
        cdp.get.assert_not_called()

    def test_network_detail_exports_only_fixed_codes(self):
        cdp = mock.Mock()
        cdp.get_current_url.return_value = "chrome-error://chromewebdata/"
        for code in self.probe.NETWORK_ERRORS:
            cdp.evaluate.return_value = " \n" + code + "\n"
            self.assertEqual(self.probe.chrome_error_detail(cdp), code)
        for value in [None, True, {"value": "ERR_CONNECTION_RESET"}, "ERR_PRIVATE_CONTENT",
                      "ERR_CONNECTION_RESET private-page-text", "https://chatgpt.com/c/private"]:
            cdp.evaluate.return_value = value
            self.assertEqual(self.probe.chrome_error_detail(cdp), "UNKNOWN")
        cdp.evaluate.side_effect = RuntimeError("private failure")
        self.assertEqual(self.probe.chrome_error_detail(cdp), "UNKNOWN")
        cdp.get.assert_not_called()

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
            rows = output.getvalue().splitlines()
            self.assertTrue(rows[0].startswith("PROBE:TAB:"))
            self.assertEqual(rows[1:], ["PROBE:TARGET_TAB_UNAVAILABLE", "PROBE:FINAL:EXACT_READY_IDLE"])
        self.assertEqual(self.probe.PROFILE, default)

    def test_home_recovery_requires_verified_empty_composer(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        for empty in ["EMPTY", True, False, None, "true", {"value": "EMPTY"}, "EMPTY private"]:
            with self.subTest(empty=empty):
                cdp = mock.Mock()
                cdp.get_current_url.return_value = "https://chatgpt.com/"
                cdp.is_element_visible.return_value = False
                cdp.evaluate.return_value = empty
                self.probe.recover_empty_home(cdp, target)
                self.assertEqual(cdp.get.call_args_list, [mock.call(target)] if empty == "EMPTY" else [])

    def test_home_refusal_reasons_are_fixed_and_do_not_navigate(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        cdp = mock.Mock()
        cdp.get_current_url.return_value = "https://chatgpt.com/"
        cdp.is_element_visible.return_value = False
        for reason in self.probe.HOME_CHECKS - {"EMPTY"}:
            cdp.evaluate.return_value = reason
            self.assertEqual(self.probe.recover_empty_home(cdp, target), reason)
        for value in [True, None, "private draft", "EMPTY\n", {"value": "EMPTY"}]:
            cdp.evaluate.return_value = value
            self.assertEqual(self.probe.recover_empty_home(cdp, target), "EVALUATION_UNKNOWN")
        cdp.evaluate.side_effect = RuntimeError("private failure")
        self.assertEqual(self.probe.recover_empty_home(cdp, target), "EVALUATION_FAILED")
        cdp.get.assert_not_called()

    def test_home_route_change_or_busy_state_refuses_navigation_after_empty_check(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        cdp = mock.Mock()
        cdp.evaluate.return_value = "EMPTY"
        cdp.is_element_visible.return_value = False
        cdp.get_current_url.side_effect = ["https://chatgpt.com/", target]
        self.assertEqual(self.probe.recover_empty_home(cdp, target), "NOT_HOME")
        cdp.get.assert_not_called()
        cdp.get_current_url.side_effect = None
        cdp.get_current_url.return_value = "https://chatgpt.com/"
        def evaluation(_):
            cdp.is_element_visible.side_effect = lambda selector: selector in self.probe.STOP
            return "EMPTY"
        cdp.evaluate.side_effect = evaluation
        self.assertEqual(self.probe.recover_empty_home(cdp, target), "BUSY")
        cdp.get.assert_not_called()

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
                cdp.evaluate.return_value = "EMPTY"
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
        cdp.evaluate.return_value = "EMPTY"
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

    def target_tabs(self, metadata_urls, document_urls, selected_url=None):
        tabs = []
        for metadata, document in zip(metadata_urls, document_urls):
            async def read_url(value=document):
                return value
            tabs.append(types.SimpleNamespace(target=types.SimpleNamespace(url=metadata),
                                             get_current_url=read_url))
        cdp = mock.Mock()
        cdp.get_tabs.return_value = tabs
        cdp.get_current_url.return_value = selected_url
        cdp.loop = asyncio.new_event_loop()
        self.addCleanup(cdp.loop.close)
        return cdp, tabs

    def test_target_aliases_match_bridge_identity_without_relaxing_route_guards(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        alternatives = [target, "https://chatgpt.com/g/project/c/readiness-fixture",
                        "https://chat.openai.com/c/readiness-fixture"]
        for metadata in alternatives:
            for document in alternatives:
                with self.subTest(metadata=metadata, document=document):
                    cdp, tabs = self.target_tabs([metadata], [document], selected_url=document)
                    facts = []
                    self.assertEqual(self.probe.select_registered_target_tab(cdp, target, facts.append), "TARGET_TAB_EXACT")
                    cdp.switch_to_tab.assert_called_once_with(tabs[0])
                    self.assertEqual(facts[0]["stage"], "VERIFIED")
                    self.assertEqual(facts[0]["alternateRoute"], document != target)
        for invalid in [target + "/", target + "?query=private", target + "#fragment",
                        "https://user@chatgpt.com/c/readiness-fixture", "https://chatgpt.com:443/c/readiness-fixture",
                        "https://foreign.test/c/readiness-fixture", "https://chatgpt.com/c/other",
                        "https://chatgpt.com/c/readiness%2Dfixture", "https://chatgpt.com/c/with space"]:
            with self.subTest(invalid=invalid):
                self.assertFalse(self.probe.exact_url(invalid, target))
                cdp, _ = self.target_tabs([target], [invalid], selected_url=target)
                self.assertEqual(self.probe.select_registered_target_tab(cdp, target), "TARGET_TAB_DRIFT")
                cdp.switch_to_tab.assert_not_called()

    def test_route_contract_matches_owner_verified_bridge(self):
        path = Path(__file__).resolve().parents[1] / "scripts/wake_bridge.py"
        expected = "https://chatgpt.com/c/fixture"
        with tempfile.TemporaryDirectory() as profile:
            sink = self.probe.load_observation_sink(path, hashlib.sha256(path.read_bytes()).hexdigest(), Path(profile), expected)
            for value in [expected, "https://chat.openai.com/g/project/c/fixture",
                          expected + "/", expected + "?query=1", expected + "#fragment",
                          "https://user@chatgpt.com/c/fixture", "https://chatgpt.com:443/c/fixture",
                          "http://chatgpt.com/c/fixture", "https://foreign.test/c/fixture",
                          "https://chatgpt.com/g/project/c/other", "https://chatgpt.com/c/fixture extra", None, {}]:
                with self.subTest(value=value):
                    self.assertEqual(self.probe.exact_url(value, expected), sink.exact(value))

    def test_main_accepts_registered_project_route_and_rejects_invalid_route_before_browser(self):
        target = "https://chatgpt.com/g/project/c/fixture"
        cdp = mock.Mock()
        cdp.get_current_url.return_value = target
        cdp.get_tabs.return_value = []
        cdp.is_element_visible.side_effect = lambda selector: selector == "#prompt-textarea"
        browser = mock.MagicMock();browser.__enter__.return_value = browser;browser.cdp = cdp
        factory = mock.Mock(return_value=browser)
        with tempfile.TemporaryDirectory() as profile:
            args = ["probe", "--profile", profile, "--read-only", "--conversation-url", target, "--seconds", "5"]
            output = io.StringIO()
            with mock.patch.object(sys, "argv", args), mock.patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=factory)}), \
                    mock.patch.object(self.probe.time, "monotonic", side_effect=[0, 0, 10]), contextlib.redirect_stdout(output):
                self.assertEqual(self.probe.main(), 0)
            self.assertTrue(output.getvalue().endswith("PROBE:FINAL:EXACT_READY_IDLE\n"))
            factory.reset_mock()
            for invalid in [target + "/", target + "?query=1", "https://user@chatgpt.com/c/fixture", "https://chatgpt.com:443/c/fixture"]:
                args[5] = invalid
                with mock.patch.object(sys, "argv", args), mock.patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=factory)}), contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(self.probe.main(), 2)
            factory.assert_not_called()

    def test_metadata_match_with_home_document_is_precise_and_does_not_switch(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        cdp, _ = self.target_tabs([target], ["https://chatgpt.com/"], selected_url="https://chatgpt.com/")
        facts = []
        self.assertEqual(self.probe.select_registered_target_tab(cdp, target, facts.append), "TARGET_TAB_DRIFT")
        self.assertEqual((facts[0]["metadataMatches"], facts[0]["candidateRoute"], facts[0]["stage"]), (1, "HOME", "DOCUMENT"))
        cdp.switch_to_tab.assert_not_called()
        cdp.get.assert_not_called()

    def test_wrapped_document_and_metadata_remain_unproven_without_export(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        for metadata, document, state, key in [
                (target, {"value": "PRIVATE_ROUTE"}, "TARGET_TAB_DRIFT", "candidateShape"),
                ({"value": "PRIVATE_ROUTE"}, target, "TARGET_TAB_UNAVAILABLE", "controlledShape")]:
            cdp, _ = self.target_tabs([metadata], [document], selected_url=None)
            facts = []
            self.assertEqual(self.probe.select_registered_target_tab(cdp, target, facts.append), state)
            if key == "candidateShape":self.assertEqual(facts[0][key], "WRAPPED")
            else:self.assertEqual(facts[0]["unreadableMetadata"], 1)
            self.assertNotIn("PRIVATE_ROUTE", json.dumps(facts))
            cdp.switch_to_tab.assert_not_called()

    def test_post_selection_drift_is_distinct_from_candidate_document_drift(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        cdp, tabs = self.target_tabs([target], [target], selected_url="https://chatgpt.com/")
        facts = []
        self.assertEqual(self.probe.select_registered_target_tab(cdp, target, facts.append), "TARGET_TAB_DRIFT")
        self.assertEqual((facts[0]["candidateRoute"], facts[0]["controlledRoute"], facts[0]["stage"]), ("EXACT", "HOME", "SWITCH"))
        cdp.switch_to_tab.assert_called_once_with(tabs[0])
        cdp.get.assert_not_called()

    def test_duplicate_aliases_and_unreadable_sibling_cannot_select(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        for metadata, documents, expected in [
                ([target, "https://chatgpt.com/g/project/c/readiness-fixture"], [target, target], "TARGET_TAB_AMBIGUOUS"),
                ([target, None], [target, target], "TARGET_TAB_UNAVAILABLE")]:
            cdp, _ = self.target_tabs(metadata, documents, selected_url=target)
            self.assertEqual(self.probe.select_registered_target_tab(cdp, target), expected)
            cdp.switch_to_tab.assert_not_called()

    def test_metadata_diagnostics_never_export_private_urls_and_errors(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        cdp, tabs = self.target_tabs([target], [target], selected_url=target)
        async def frame(_):return False
        tabs[0].evaluate = frame
        facts = []
        self.assertEqual(self.probe.select_registered_target_tab(cdp, target, facts.append), "TARGET_TAB_EXACT")
        self.assertEqual(facts[0]["candidateFrame"], "SUBFRAME")
        self.assertNotIn("chatgpt.com", json.dumps(facts))
        cdp.get_tabs.side_effect = RuntimeError("PRIVATE_ERROR")
        self.assertEqual(self.probe.select_registered_target_tab(cdp, target, facts.append), "TARGET_TAB_UNAVAILABLE")
        self.assertNotIn("PRIVATE_ERROR", json.dumps(facts))

    def test_loading_registered_tab_is_reselected_without_home_navigation(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        home = "https://chatgpt.com/"
        clock = [0]
        selected_url = [home]
        cdp = mock.Mock()
        cdp.get_current_url.side_effect = lambda: selected_url[0]
        cdp.is_element_visible.side_effect = lambda selector: selector == "#prompt-textarea"
        # An unknown Home composer must remain untouched. Selecting a separate
        # verified existing tab does not require reading or changing that draft.
        cdp.evaluate.return_value = False
        async def document_url():
            return target if clock[0] >= 3 else home
        tab = types.SimpleNamespace(target=types.SimpleNamespace(url=target), get_current_url=document_url)
        cdp.get_tabs.return_value = [tab]
        cdp.loop = asyncio.new_event_loop()
        self.addCleanup(cdp.loop.close)
        cdp.switch_to_tab.side_effect = lambda _: selected_url.__setitem__(0, target)
        browser = mock.MagicMock()
        browser.__enter__.return_value = browser
        browser.cdp = cdp
        def sleep(seconds):
            clock[0] += seconds
        with tempfile.TemporaryDirectory() as profile:
            argv = ["probe", "--profile", profile, "--conversation-url", target, "--seconds", "5"]
            output = io.StringIO()
            with mock.patch.object(sys, "argv", argv), \
                    mock.patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=lambda **kwargs: browser)}), \
                    mock.patch.object(self.probe.time, "monotonic", side_effect=lambda: clock[0]), \
                    mock.patch.object(self.probe.time, "sleep", side_effect=sleep), \
                    contextlib.redirect_stdout(output):
                self.assertEqual(self.probe.main(), 0)
        self.assertIn("PROBE:TARGET_TAB_DRIFT\n", output.getvalue())
        self.assertIn("PROBE:TARGET_TAB_EXACT\n", output.getvalue())
        self.assertTrue(output.getvalue().endswith("PROBE:FINAL:EXACT_READY_IDLE\n"))
        self.assertEqual(clock[0], 5)
        cdp.switch_to_tab.assert_called_once_with(tab)
        cdp.get.assert_not_called()

    def test_tab_reinspection_is_bounded_when_target_remains_missing(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        clock = [0]
        cdp = mock.Mock()
        cdp.get_current_url.return_value = "https://chatgpt.com/"
        cdp.get_tabs.return_value = []
        cdp.is_element_visible.return_value = False
        cdp.evaluate.return_value = False
        browser = mock.MagicMock()
        browser.__enter__.return_value = browser
        browser.cdp = cdp
        def sleep(seconds):
            clock[0] += seconds
        with tempfile.TemporaryDirectory() as profile:
            argv = ["probe", "--profile", profile, "--conversation-url", target, "--seconds", "30"]
            with mock.patch.object(sys, "argv", argv), \
                    mock.patch.dict(sys.modules, {"seleniumbase": types.SimpleNamespace(SB=lambda **kwargs: browser)}), \
                    mock.patch.object(self.probe.time, "monotonic", side_effect=lambda: clock[0]), \
                    mock.patch.object(self.probe.time, "sleep", side_effect=sleep), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(self.probe.main(), 0)
        self.assertEqual(cdp.get_tabs.call_count, 4)
        self.assertEqual(clock[0], 30)
        cdp.switch_to_tab.assert_not_called()
        cdp.get.assert_not_called()

    def test_unique_tab_requires_independent_metadata_and_document_agreement(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        home = "https://chatgpt.com/"
        cdp, tabs = self.target_tabs([home, target], [home, target], target)
        self.assertEqual(self.probe.select_registered_target_tab(cdp, target), "TARGET_TAB_EXACT")
        cdp.switch_to_tab.assert_called_once_with(tabs[1])
        cdp.get.assert_not_called()

    def test_missing_or_ambiguous_target_does_not_select_a_tab(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        for urls, expected in [(["https://chatgpt.com/"], "TARGET_TAB_MISSING"),
                               ([target, target], "TARGET_TAB_AMBIGUOUS")]:
            cdp, _ = self.target_tabs(urls, urls)
            self.assertEqual(self.probe.select_registered_target_tab(cdp, target), expected)
            cdp.switch_to_tab.assert_not_called()

    def test_stale_metadata_and_changed_selected_document_fail_closed(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        home = "https://chatgpt.com/"
        cdp, _ = self.target_tabs([target], [home], target)
        self.assertEqual(self.probe.select_registered_target_tab(cdp, target), "TARGET_TAB_DRIFT")
        cdp.switch_to_tab.assert_not_called()
        cdp, _ = self.target_tabs([target], [target], home)
        self.assertEqual(self.probe.select_registered_target_tab(cdp, target), "TARGET_TAB_DRIFT")

    def test_unavailable_or_excessive_tab_inventory_never_selects(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        cdp = mock.Mock()
        for inventory in [None, [], [None] * 33]:
            cdp.get_tabs.return_value = inventory
            expected = "TARGET_TAB_MISSING" if inventory == [] else "TARGET_TAB_UNAVAILABLE"
            self.assertEqual(self.probe.select_registered_target_tab(cdp, target), expected)
        cdp.get_tabs.side_effect = RuntimeError("unavailable")
        self.assertEqual(self.probe.select_registered_target_tab(cdp, target), "TARGET_TAB_UNAVAILABLE")
        cdp.switch_to_tab.assert_not_called()

    def test_target_comparison_refuses_credentials_ports_and_other_projects(self):
        target = "https://chatgpt.com/c/readiness-fixture"
        for current in ["http://chatgpt.com/c/readiness-fixture", target + "?query=1",
                        target + "#fragment", "https://user@chatgpt.com/c/readiness-fixture",
                        "https://chatgpt.com:443/c/readiness-fixture",
                        "https://other.example/c/readiness-fixture",
                        "https://chatgpt.com/c/other"]:
            self.assertFalse(self.probe.exact_url(current, target))


if __name__ == "__main__":
    unittest.main()
