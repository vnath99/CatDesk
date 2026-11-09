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
    def test_passive_response_modes_reject_incomplete_scope_and_action_flags(self):
        with tempfile.TemporaryDirectory() as profile:
            base=['probe','--conversation-url','https://chatgpt.com/c/fixture','--profile',profile,
                '--read-only','--response-only','--task-diagnostics','--delivery-file','delivery.json',
                '--delivery-sha256','b'*64,'--primitives','wake_bridge.py','--primitives-sha256','a'*64]
            cases=[base+['--await-existing-response','--historical-response'],
                base+['--await-existing-response','--source-review-diagnostics'],
                base+['--source-review-diagnostics','--historical-response']]
            for missing in ['--read-only','--response-only','--task-diagnostics']:
                cases.append([v for v in base if v!=missing]+['--await-existing-response'])
            for argv in cases:
                with self.subTest(argv=argv),mock.patch.object(sys,'argv',argv), \
                        mock.patch.object(self.probe,'load_observation_sink',return_value=mock.Mock()), \
                        mock.patch.object(self.probe,'load_delivery_message') as loader:
                    self.assertEqual(self.probe.main(),2)
                    loader.assert_not_called()
            for flag in ['--receipt-only','--inspect-app-menu','--inspect-response-trace','--inspect-historical-trace']:
                with self.subTest(flag=flag),mock.patch.object(sys,'argv',base+[flag]), \
                        contextlib.redirect_stderr(io.StringIO()),self.assertRaises(SystemExit) as failure:
                    self.probe.main()
                self.assertEqual(failure.exception.code,2)

    def test_sent_response_reader_refuses_hash_route_phase_and_receipt_drift(self):
        url='https://chatgpt.com/c/fixture';event_id='web-work-'+'a'*64;message='PRIVATE_FROZEN_USER'
        message_digest=hashlib.sha256(message.encode()).hexdigest();target_digest=hashlib.sha256(url.encode()).hexdigest()
        delivery=dict(phase='SENT',messageDigest=message_digest,
            event=dict(eventId=event_id,projectId='alpha',eventType='engineering_attention',
                targetGeneration=1,message=message,auditReferences=[]),
            target=dict(generation=1,url=url,digest=target_digest),
            receipt=dict(schemaVersion=1,eventId=event_id,targetGeneration=1,targetDigest=target_digest,
                messageDigest=message_digest,sentUtc=1,evidence='EXACT_USER_MESSAGE_APPENDED'))
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'deliveries'/(event_id+'.json');path.parent.mkdir()
            def read(value,checksum=None,target=url):
                path.write_text(json.dumps(value),encoding='utf-8')
                digest=hashlib.sha256(path.read_bytes()).hexdigest()
                return self.probe.load_delivery_message(path,checksum or digest,target,response_only=True)
            self.assertEqual(read(delivery),delivery)
            for value,kwargs in [(delivery,dict(checksum='0'*64)),(delivery,dict(target=url+'-foreign')),
                    (delivery | {'phase':'SUBMITTING'},{}),(delivery | {'receipt':None},{}),
                    (delivery | {'receipt':delivery['receipt'] | {'messageDigest':'0'*64}},{}),
                    (delivery | {'target':delivery['target'] | {'generation':2}}, {})]:
                with self.subTest(kwargs=kwargs),self.assertRaises((ValueError,KeyError)):
                    read(value,**kwargs)

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
            self.assertEqual([row for row in rows if row.startswith("PROBE:PHASE:")], [
                "PROBE:PHASE:IMPORT_BROWSER", "PROBE:PHASE:BROWSER_START", "PROBE:PHASE:CDP_NAVIGATION",
                "PROBE:PHASE:OBSERVING", "PROBE:PHASE:BROWSER_CLOSED"])
            observations = [row for row in rows if not row.startswith("PROBE:PHASE:")]
            self.assertTrue(observations[0].startswith("PROBE:TAB:"))
            self.assertEqual(observations[1:], ["PROBE:TARGET_TAB_UNAVAILABLE", "PROBE:FINAL:EXACT_READY_IDLE"])
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


    def test_sent_continuation_diagnostics_cannot_admit_unknown_or_unreceived_delivery(self):
        url="https://chatgpt.com/c/fixture"
        event_id="wake-intent-"+"b"*64
        message="frozen continuation"
        refs=['catdesk.wake.v1:'+k+':'+v for k,v in [('purpose','NEW_TASK'),('source-event','web-work-'+'a'*64),
            ('source-turn','initial-context-'+'c'*64),('priority','0'),('expires-utc','100')]]
        target=dict(generation=1,url=url,digest=hashlib.sha256(url.encode()).hexdigest())
        digest=hashlib.sha256(message.encode()).hexdigest()
        value=dict(phase='SENT',messageDigest=digest,target=target,event=dict(eventId=event_id,projectId='alpha',
            eventType='engineering_attention',targetGeneration=1,message=message,auditReferences=refs),
            receipt=dict(schemaVersion=1,eventId=event_id,targetGeneration=1,targetDigest=target['digest'],
                messageDigest=digest,sentUtc=10,evidence='EXACT_USER_MESSAGE_APPENDED'))
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'deliveries'/(event_id+'.json');path.parent.mkdir()
            def read(item,**kw):
                path.write_text(json.dumps(item));checksum=hashlib.sha256(path.read_bytes()).hexdigest()
                return self.probe.load_delivery_message(path,checksum,url,response_only=True,**kw)
            with self.assertRaises(ValueError):read(value)
            self.assertEqual(read(value,sent_diagnostics=True),value)
            for changed in [value | {'phase':'SUBMITTING'},value | {'receipt':None},
                value | {'event':value['event'] | {'auditReferences':refs[:-1]}},
                value | {'event':value['event'] | {'auditReferences':[v.replace('NEW_TASK','ROLLOVER') for v in refs]}}]:
                with self.assertRaises((ValueError,KeyError)):read(changed,sent_diagnostics=True)


    def test_source_review_delivery_is_passive_diagnostics_only_and_exact_receipt_bound(self):
        url='https://chatgpt.com/c/fixture'
        identity='web-work-'+'a'*64
        message='frozen public source review'
        digest=hashlib.sha256(message.encode()).hexdigest()
        target=dict(url=url,digest=hashlib.sha256(url.encode()).hexdigest(),generation=1)
        value=dict(phase='SENT',messageDigest=digest,target=target,
            event=dict(eventId=identity,projectId='alpha',eventType='source_review',targetGeneration=1,
                message=message,auditReferences=['catdesk-review-v1:original-frozen-grant']),
            receipt=dict(schemaVersion=1,eventId=identity,targetGeneration=1,targetDigest=target['digest'],
                messageDigest=digest,sentUtc=10,evidence='EXACT_USER_MESSAGE_APPENDED'))
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'deliveries'/(identity+'.json');path.parent.mkdir()
            def read(item,**flags):
                path.write_text(json.dumps(item));checksum=hashlib.sha256(path.read_bytes()).hexdigest()
                result=self.probe.load_delivery_message(path,checksum,url,**flags)
                self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),checksum)
                return result
            allowed=dict(response_only=True,sent_diagnostics=True,source_review_diagnostics=True)
            self.assertEqual(read(value,**allowed),value)
            for flags in [dict(),dict(response_only=True),dict(response_only=True,sent_diagnostics=True),
                dict(response_only=True,source_review_diagnostics=True),dict(source_review_diagnostics=True),
                allowed | dict(receipt_only=True)]:
                with self.subTest(flags=flags),self.assertRaises((ValueError,KeyError)):
                    read(value,**flags)
            for changed in [value | dict(phase='SUBMITTING'),value | dict(receipt=None),
                value | dict(event=value['event'] | dict(auditReferences=[])),
                value | dict(event=value['event'] | dict(auditReferences=['catdesk-review-v1:a','catdesk-review-v1:b'])),
                value | dict(event=value['event'] | dict(auditReferences=['catdesk-review-v1:'])),
                value | dict(event=value['event'] | dict(auditReferences=['catdesk-review-v1:bad/grant'])),
                value | dict(event=value['event'] | dict(auditReferences=['catdesk-review-v1:'+'a'*129])),
                value | dict(event=value['event'] | dict(eventId='wake-intent-'+'a'*64)),
                value | dict(event=value['event'] | dict(targetGeneration=2)),
                value | dict(receipt=value['receipt'] | dict(evidence='UNVERIFIED')),
                value | dict(receipt=value['receipt'] | dict(messageDigest='b'*64))]:
                with self.subTest(changed=changed),self.assertRaises((ValueError,KeyError)):
                    read(changed,**allowed)


    def awaited_response(self, states, deadline=5, late_complete=False):
        clock=[0.0];sleeps=[];observed=[]
        delivery=dict(messageDigest='a'*64,event=dict(eventId='web-work-'+'b'*64,message='PRIVATE_USER'),
            target=dict(generation=1,digest='c'*64))
        sink=types.SimpleNamespace(response_observation_controls=None)
        sequence=iter(states)
        def observe(cdp,message,digest):
            self.assertIs(cdp,controlled)
            self.assertEqual((message,digest),('PRIVATE_USER','a'*64))
            item=next(sequence);observed.append(item)
            if isinstance(item,Exception):raise item
            if isinstance(item,tuple):
                state,reason=item[:2]
                sink.response_observation_reason=reason
                sink.response_observation_controls=dict(item[2]) if len(item)>2 else None
            else:
                state=item
                sink.response_observation_reason='GENERATION_ACTIVE' if state=='RESPONSE_PENDING' else 'COMPLETE'
                sink.response_observation_controls=None
            if late_complete and state=='RESPONSE_COMPLETE':clock[0]=deadline+1
            return state
        sink.observe_cooperative_response=observe
        def sleep(seconds):
            self.assertGreater(seconds,0);self.assertLessEqual(seconds,1)
            sleeps.append(seconds);clock[0]+=seconds
        controlled=object();output=io.StringIO()
        with contextlib.redirect_stdout(output):
            completed=self.probe.report_sent_response(sink,controlled,delivery,sleep=sleep,
                await_deadline=deadline,monotonic=lambda:clock[0])
        self.assertNotIn('PRIVATE_',output.getvalue())
        return completed,output.getvalue(),clock[0],sleeps,observed


    def test_existing_response_wait_keeps_owner_until_two_fresh_completed_samples(self):
        result,out,elapsed,sleeps,observed=self.awaited_response([
            'RESPONSE_PENDING','RESPONSE_PENDING','RESPONSE_COMPLETE','RESPONSE_COMPLETE'])
        self.assertTrue(result)
        self.assertEqual(observed[-2:],['RESPONSE_COMPLETE','RESPONSE_COMPLETE'])
        self.assertEqual(observed.count('RESPONSE_PENDING'),2)
        self.assertEqual(elapsed,2.25)
        self.assertEqual(json.loads(out.split('PROBE:RESPONSE:')[1])['stage'],'RESPONSE_COMPLETE')
        # If generation resumes during the stability spacing, start a fresh pair.
        result,_,_,_,observed=self.awaited_response([
            'RESPONSE_PENDING','RESPONSE_COMPLETE','RESPONSE_PENDING','RESPONSE_COMPLETE','RESPONSE_COMPLETE'])
        self.assertTrue(result)
        self.assertEqual(observed[-2:],['RESPONSE_COMPLETE','RESPONSE_COMPLETE'])


    def test_existing_response_wait_expires_without_renewing_budget_or_completing(self):
        result,out,elapsed,sleeps,observed=self.awaited_response(['RESPONSE_PENDING']*3,deadline=2.5)
        self.assertFalse(result)
        self.assertEqual(elapsed,2.5)
        self.assertEqual(sleeps,[1,1,.5])
        self.assertEqual(len(observed),3)
        value=json.loads(out.split('PROBE:RESPONSE:')[1])
        self.assertEqual((value['stage'],value['diagnosticReason']),('RESPONSE_PENDING','GENERATION_ACTIVE'))
        result,out,_,_,_=self.awaited_response(['RESPONSE_PENDING','RESPONSE_COMPLETE'],late_complete=True)
        self.assertFalse(result)
        self.assertEqual(out,'PROBE:RESPONSE_STATE:UNSTABLE\n')


    def test_existing_response_wait_rechecks_scope_and_fails_closed_on_interruption(self):
        for reason,expected in [('RESPONSE_TARGET_DRIFT','RESPONSE_TARGET_DRIFT'),
                ('LOGIN_OR_PROFILE_REQUIRED','LOGIN_OR_PROFILE_REQUIRED'),
                ('RESPONSE_NETWORK_ERROR','RESPONSE_NETWORK_ERROR'),
                ('RESPONSE_EXPECTED_USER_DUPLICATED','RESPONSE_EXPECTED_USER_DUPLICATED'),
                ('RESPONSE_SEQUENCE_DRIFT','RESPONSE_SEQUENCE_DRIFT'),('PRIVATE_CANCELLATION','UNPROVEN')]:
            with self.subTest(reason=reason):
                result,out,elapsed,sleeps,observed=self.awaited_response([
                    'RESPONSE_PENDING',RuntimeError(reason)])
                self.assertFalse(result)
                self.assertEqual(out,'PROBE:RESPONSE_STATE:'+expected+'\n')
                self.assertEqual((elapsed,sleeps,len(observed)),(1,[1],2))


    def test_existing_response_wait_does_not_report_old_generation_after_unstable_completion(self):
        result,out,elapsed,_,observed=self.awaited_response([
            'RESPONSE_PENDING',
            ('RESPONSE_COMPLETE','COMPLETE',self.completed_controls(1)),
            ('RESPONSE_COMPLETE','COMPLETE',self.completed_controls(2)),
        ],deadline=1.5)
        self.assertFalse(result)
        self.assertEqual(elapsed,1.5)
        self.assertEqual(len(observed),3)
        self.assertEqual(out,'PROBE:RESPONSE_STATE:UNSTABLE\n')


    def test_existing_response_wait_settles_generation_mount_body_and_completion_controls(self):
        states=[('RESPONSE_PENDING',reason) for reason in ['GENERATION_ACTIVE',
            'ASSISTANT_NOT_MOUNTED','ASSISTANT_BODY_UNPROVEN','ASSISTANT_COMPLETION_ACTION_UNPROVEN']]
        states+=['RESPONSE_COMPLETE','RESPONSE_COMPLETE']
        result,out,elapsed,_,observed=self.awaited_response(states,deadline=6)
        self.assertTrue(result)
        self.assertEqual(observed,states)
        self.assertEqual(elapsed,4.25)
        self.assertEqual(json.loads(out.split('PROBE:RESPONSE:')[1])['stage'],'RESPONSE_COMPLETE')


    @staticmethod
    def completed_controls(buttons):
        value={key:0 for key in ['semanticAssistantUnits','roleAssistantNodes','copyTestIdInTurn',
            'copyLabelInTurn','copyResponseLabelInTurn','copyMessageLabelInTurn','copyTextInTurn',
            'knownCopyInAssistant','knownCopyInBody','sharedCopyAfterAssistant','toolbarContainersInTurn',
            'buttonsInTurn','copyTestIdInParent','copyLabelInParent','copyResponseLabelInParent',
            'copyMessageLabelInParent']}
        value.update(pairedTurn=True,assistantBodyPresent=True,completionActionFound=True,
            assistantInsideSemanticUnit=True,buttonsInTurn=buttons)
        return value


    def test_existing_response_wait_restarts_pair_after_successful_completion_metadata_change(self):
        a=('RESPONSE_COMPLETE','COMPLETE',self.completed_controls(1))
        b=('RESPONSE_COMPLETE','COMPLETE',self.completed_controls(2))
        result,out,elapsed,_,observed=self.awaited_response([a,b,b])
        self.assertTrue(result)
        self.assertEqual(observed,[a,b,b])
        self.assertEqual(elapsed,.5)
        value=json.loads(out.split('PROBE:RESPONSE:')[1])
        self.assertEqual(value['completionControls']['buttonsInTurn'],2)


    def test_existing_response_wait_keeps_stable_non_generation_pending_until_deadline(self):
        for reason in ['ASSISTANT_NOT_MOUNTED','ASSISTANT_BODY_UNPROVEN',
                'ASSISTANT_COMPLETION_ACTION_UNPROVEN']:
            states=[('RESPONSE_PENDING',reason)]*3
            with self.subTest(reason=reason):
                result,out,elapsed,sleeps,observed=self.awaited_response(states,deadline=2.5)
                self.assertFalse(result)
                self.assertEqual((elapsed,sleeps,len(observed)),(2.5,[1,1,.5],3))
                value=json.loads(out.split('PROBE:RESPONSE:')[1])
                self.assertEqual((value['stage'],value['diagnosticReason']),('RESPONSE_PENDING',reason))


    def test_existing_response_wait_never_promotes_unstable_completed_samples_at_deadline(self):
        states=[('RESPONSE_COMPLETE','COMPLETE',self.completed_controls(i)) for i in range(4)]
        result,out,elapsed,sleeps,observed=self.awaited_response(states,deadline=1)
        self.assertFalse(result)
        self.assertEqual(observed,states)
        self.assertEqual((elapsed,sleeps),(1,[.25]*4))
        self.assertEqual(out,'PROBE:RESPONSE_STATE:UNSTABLE\n')


    def test_task_diagnostic_read_requires_two_identical_completed_samples_without_text(self):
        delivery=dict(messageDigest='a'*64,event=dict(eventId='wake-intent-'+'b'*64,message='PRIVATE_USER'),target=dict(generation=1,digest='c'*64))
        hints=dict(schemaVersion=1,basis='ASSISTANT_TEXT_HINTS_ONLY_NOT_VERIFIED_EXECUTION',toolUnavailable=True,
            contextUnavailable=False,approvalRequested=False,nativeDispatchMentioned=False,knownCodes=[])
        tools=dict(schemaVersion=1,basis='VISIBLE_UI_MARKERS_ONLY_NOT_EXECUTION_RECEIPT',scope='EXACT_USER_EXCHANGE',
            selectorCoverage='KNOWN_MARKERS_ONLY',callMarkers=0,resultMarkers=0,errorMarkers=0,busyMarkers=0,countOverflow=False)
        sink=mock.Mock();sink.response_observation_reason='COMPLETE';sink.response_observation_controls=None
        sink.observe_cooperative_response.return_value='RESPONSE_COMPLETE'
        def snapshot(*args,**kw):
            kw['diagnostic'].update(reason='COMPLETE');kw['tool_diagnostic'].update(tools);kw['execution_diagnostic'].update(hints)
            return (0,0,True,True)
        sink.response_snapshot.side_effect=snapshot
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_sent_response(sink,object(),delivery,sleep=lambda _:None,task_diagnostics=True)
        self.assertIn('PROBE:TASK:',output.getvalue());self.assertNotIn('PRIVATE_',output.getvalue())
        self.assertEqual(sink.response_snapshot.call_count,2)
        sink.response_snapshot.side_effect=lambda *a,**k:(0,0,True,False)
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_sent_response(sink,object(),delivery,sleep=lambda _:None,task_diagnostics=True)
        self.assertEqual(output.getvalue(),'PROBE:RESPONSE_STATE:UNPROVEN\n')


    def test_response_observer_is_receipt_bound_stable_and_never_retries(self):
        delivery=dict(messageDigest="a"*64,event=dict(eventId="web-work-"+"b"*64,message="PRIVATE_USER"),target=dict(generation=1,digest="c"*64))
        sink=types.SimpleNamespace(observe_cooperative_response=mock.Mock(return_value="RESPONSE_COMPLETE"),response_observation_reason="COMPLETE",response_observation_controls=None)
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_sent_response(sink,object(),delivery,sleep=lambda _:None)
        self.assertEqual(sink.observe_cooperative_response.call_count,2)
        value=json.loads(output.getvalue().split("PROBE:RESPONSE:")[1]);self.assertEqual(value["stage"],"RESPONSE_COMPLETE")
        self.assertNotIn("PRIVATE_USER",output.getvalue())
        output=io.StringIO();sink.observe_cooperative_response.reset_mock()
        with contextlib.redirect_stdout(output):self.probe.report_sent_response(sink,object(),delivery,sleep=lambda _:None,historical=True)
        self.assertIn('PROBE:HISTORY_RESPONSE:',output.getvalue());self.assertNotIn('PROBE:RESPONSE:',output.getvalue())
        self.assertTrue(all(call.kwargs=={'historical':True} for call in sink.observe_cooperative_response.call_args_list))
        sink.observe_cooperative_response.side_effect=["RESPONSE_PENDING","RESPONSE_COMPLETE"]
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_sent_response(sink,object(),delivery,sleep=lambda _:None)
        self.assertEqual(output.getvalue(),"PROBE:RESPONSE_STATE:UNSTABLE\n")
        for reason,expected in [("RESPONSE_LOAD_ERROR","RESPONSE_LOAD_ERROR"),
                ("RESPONSE_EXPECTED_USER_NOT_FOUND","RESPONSE_EXPECTED_USER_NOT_FOUND"),
                ("RESPONSE_USER_REPRESENTATION_MISMATCH","RESPONSE_USER_REPRESENTATION_MISMATCH"),
                ("RESPONSE_SEQUENCE_QUERY_FAILED","RESPONSE_SEQUENCE_QUERY_FAILED"),("PRIVATE_ERROR_BODY","UNPROVEN")]:
            sink.observe_cooperative_response.side_effect=RuntimeError(reason)
            output=io.StringIO()
            with contextlib.redirect_stdout(output):self.probe.report_sent_response(sink,object(),delivery,sleep=lambda _:None)
            self.assertEqual(output.getvalue(),"PROBE:RESPONSE_STATE:"+expected+"\n")


if __name__ == "__main__":
    unittest.main()
