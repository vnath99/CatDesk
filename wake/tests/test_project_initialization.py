"""First project conversation: no fabricated source, same bounded browser path."""
import json
import io
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

from wake.tests.test_new_conversation import (
    AttemptCdp, AttemptSink, ISSUED, SOURCE, adapter, bridge,
)


class ProjectInitializationTests(unittest.TestCase):
    def test_wire_matches_rust_unicode_and_escaping_vector(self):
        body='path/src.rs\n中文 café 😀 "quote" \\ literal'
        message=bridge.compose_initialization_message("project-first","initialize-first","a"*64,
            bridge.APP_JSON_BOOTSTRAP_HANDOFF_PREFIX+'@CatDesk-Local-Tunnel-v5\n'+body)
        self.assertEqual(bridge.digest(bridge.normalize_message(message)),
            "8c99240359c76df6a7260e9c37901e6d62254046dca781e70da42fa2e1280aa4")

    def test_read_only_bootstrap_probe_retains_the_same_bounded_handoff_budget(self):
        request={"bootstrapProbe":True,"handoff":"x"*10000}
        wire=(json.dumps(request)+"\n").encode()
        with patch.object(adapter.sys,"stdin",types.SimpleNamespace(buffer=io.BytesIO(wire))):
            self.assertEqual(adapter.read_job_request(),request)
        for value in [{"bootstrapProbe":1,"handoff":"x"*10000},
                      {"bootstrapProbe":True,"handoff":"x"*adapter.MAX_BOOTSTRAP_REQUEST_BYTES}]:
            wire=(json.dumps(value)+"\n").encode()
            with patch.object(adapter.sys,"stdin",types.SimpleNamespace(buffer=io.BytesIO(wire))):
                with self.assertRaises(ValueError):
                    adapter.read_job_request()

    def request(self):
        return {
            "bootstrap": True, "cooperative": True, "bootstrapProtocol": 3,
            "resumeDraft": False, "resumeProgress": None,
            "projectId": "project-first", "intentId": "initialize-first",
            "source": None, "registrationDigest": "a" * 64,
            "handoff": bridge.APP_JSON_BOOTSTRAP_HANDOFF_PREFIX
            + '@CatDesk-Local-Tunnel-v5\n{"objective":"work only in the assigned project"}',
        }

    def run_case(self, *, request=None, draft="", users=0, deny=None, scenario="success"):
        request = request or self.request()
        cdp = AttemptCdp(scenario)
        cdp.draft, cdp.users = draft, users
        emitted, acknowledgments = [], []
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        responses = {"TEXT_ENTRY_AUTHORIZED": {"textEntry": True},
                     "DRAFT_READY": {"draftReady": True}, "SEND_ATTEMPTED": {"submit": True}}

        def acknowledge():
            stage = emitted[-1]["stage"]
            acknowledgments.append(stage)
            self.assertEqual(cdp.sent, 0)
            self.assertEqual(emitted[-1]["sourceGeneration"], 0)
            if stage != "TEXT_ENTRY_AUTHORIZED":
                self.assertEqual(bridge.normalize_message(cdp.draft), bridge.normalize_message(
                    bridge.compose_initialization_message(request["projectId"], request["intentId"],
                        request["registrationDigest"], request["handoff"])))
            return {} if stage == deny else responses[stage]

        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "read_request", acknowledge
        ), patch.object(adapter, "emit", emitted.append):
            try:
                adapter.bootstrap_attempt(bridge, sb, request)
                failure = None
            except bridge.Attention as error:
                failure = str(error)
        return cdp, emitted, acknowledgments, failure

    def test_initialization_envelope_has_null_predecessor_and_frozen_registration(self):
        r = self.request()
        text = bridge.compose_initialization_message(r["projectId"], r["intentId"],
            r["registrationDigest"], r["handoff"])
        self.assertTrue(text.startswith("@CatDesk-Local-Tunnel-v5\n"))
        payload = json.loads(text.split("\n\n", 1)[1])
        self.assertIsNone(payload["predecessor"])
        self.assertEqual(payload["purpose"], "INITIAL_PROJECT_CONVERSATION")
        self.assertEqual(payload["registrationDigest"], "a" * 64)
        self.assertNotIn(SOURCE, text)

    def test_first_conversation_uses_only_empty_home_and_one_send(self):
        cdp, emitted, acks, failure = self.run_case()
        self.assertIsNone(failure)
        self.assertEqual(cdp.sent, 1)
        self.assertNotIn(SOURCE, cdp.navigation)
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"])
        receipt = emitted[-1]["receipt"]
        self.assertEqual(receipt["schemaVersion"], 2)
        self.assertEqual(receipt["sourceGeneration"], 0)
        self.assertEqual(receipt["sourceDigest"], "a" * 64)
        self.assertEqual(receipt["destinationUrl"], ISSUED)
        self.assertEqual(receipt["completion"], "RESPONSE_PENDING")

    def test_unknown_draft_or_existing_history_is_never_modified(self):
        for draft, users in [("operator draft", 0), ("", 1)]:
            with self.subTest(draft=bool(draft), users=users):
                cdp, _events, acks, failure = self.run_case(draft=draft, users=users)
                self.assertIsNotNone(failure)
                self.assertEqual(cdp.sent, 0)
                self.assertEqual(cdp.draft, draft)
                self.assertEqual(acks, [])

    def test_initialization_refuses_borrowed_source_or_missing_registration(self):
        for change in [{"source": {"url": SOURCE, "digest": bridge.digest(SOURCE), "generation": 1}},
                       {"registrationDigest": "bad"}, {"bootstrapProtocol": 2}]:
            with self.subTest(change=change):
                request = {**self.request(), **change}
                cdp, _events, acks, failure = self.run_case(request=request)
                self.assertIsNotNone(failure)
                self.assertEqual(cdp.sent, 0)
                self.assertEqual(acks, [])

    def test_each_durable_ack_fences_typing_and_submission(self):
        for stage in ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"]:
            with self.subTest(stage=stage):
                cdp, _events, acks, failure = self.run_case(deny=stage)
                self.assertEqual(cdp.sent, 0)
                self.assertEqual(acks[-1], stage)
                self.assertTrue(failure or _events[-1]["stage"] == "BOOTSTRAP_UNKNOWN")

    def test_unknown_submission_retains_single_attempt_without_receipt(self):
        cdp, events, _acks, _failure = self.run_case(scenario="click_unknown")
        self.assertEqual(cdp.sent, 1)
        self.assertEqual(events[-1]["stage"], "BOOTSTRAP_UNKNOWN")
        self.assertNotIn("receipt", events[-1])

    def test_initial_sink_has_no_source_and_requires_durable_profile(self):
        with tempfile.TemporaryDirectory() as folder:
            sink = bridge.NewConversationSink(None, Path(folder), 2, 2)
            self.assertIsNone(sink.source_url)
            self.assertEqual(sink.url, bridge.NEW_CONVERSATION_HOME)
            with self.assertRaises(bridge.Attention):
                bridge.NewConversationSink(None, Path(folder) / "missing", 2, 2)

    def test_initialization_probe_only_reads_exact_receipt(self):
        request = self.request()
        _cdp, events, _acks, failure = self.run_case(request=request)
        self.assertIsNone(failure)
        pending = events[-1]["receipt"]

        class ProbeSink:
            def __init__(self, url, profile, ui_timeout, submit_timeout):
                self.url = url

            def observe_cooperative_response(self, cdp, message, message_digest):
                self.assertions(cdp, message, message_digest)
                return "RESPONSE_COMPLETE"

            def verify_initialization_ack(self, cdp, project, intent, registration, message):
                assert project == request["projectId"] and intent == request["intentId"]
                assert registration == request["registrationDigest"]
                self.assertions(cdp, message, bridge.digest(bridge.normalize_message(message)))

            @staticmethod
            def assertions(cdp, message, message_digest):
                assert cdp.url == ISSUED and cdp.sent == 0
                assert message_digest == bridge.digest(bridge.normalize_message(message))

        probe = {key: request[key] for key in ["projectId", "intentId", "source",
                 "registrationDigest", "handoff", "bootstrapProtocol"]}
        probe.update(bootstrapProbe=True, receipt=pending)
        cdp, output = AttemptCdp(), []
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        with patch.object(bridge, "NewConversationSink", ProbeSink), patch.object(
            adapter, "emit", output.append
        ), patch.object(adapter.time, "sleep", return_value=None):
            adapter.bootstrap_probe(bridge, sb, probe)
            self.assertEqual(output[-1]["stage"], "BOOTSTRAP_COMPLETE")
            self.assertEqual(cdp.sent, 0)
            for changed in [{"schemaVersion": 1}, {"sourceGeneration": False},
                            {"sourceDigest": "b" * 64}, {"destinationDigest": "b" * 64}]:
                with self.subTest(change=changed), self.assertRaises(bridge.Attention):
                    adapter.bootstrap_probe(bridge, sb,
                        {**probe, "receipt": {**pending, **changed}})

    def test_exact_project_ack_required_without_exporting_assistant_text(self):
        request = self.request()
        message = bridge.compose_initialization_message(request["projectId"], request["intentId"],
            request["registrationDigest"], request["handoff"])
        expected = f'CATDESK_INITIALIZED {request["projectId"]} {request["intentId"]} {request["registrationDigest"]}'
        with tempfile.TemporaryDirectory() as folder:
            sink = bridge.NewConversationSink(ISSUED, Path(folder), 2, 2)
            cdp = types.SimpleNamespace(get_current_url=lambda: ISSUED)
            for answer, complete in [(expected, True), (expected.replace("project-first", "other"), True),
                                     ("Acknowledged", True), (expected, False)]:
                rows = [{"role": "USER", "text": message, "textComplete": True, "revisionComplete": True},
                        {"role": "ASSISTANT", "text": answer, "textComplete": complete, "revisionComplete": True}]
                with patch.object(sink, "visible_message_snapshot", return_value={"messages": rows}), patch.object(
                    sink, "user_message_digests", return_value=[bridge.digest(bridge.normalize_message(message))]
                ):
                    if answer == expected and complete:
                        self.assertIsNone(sink.verify_initialization_ack(cdp, request["projectId"],
                            request["intentId"], request["registrationDigest"], message))
                    else:
                        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_INITIALIZATION_ACK_UNVERIFIED"):
                            sink.verify_initialization_ack(cdp, request["projectId"],
                                request["intentId"], request["registrationDigest"], message)

    def test_ack_uses_the_actual_qualified_snapshot_reader_budget(self):
        request=self.request()
        message=bridge.compose_initialization_message(request["projectId"],request["intentId"],request["registrationDigest"],request["handoff"])
        answer=f'CATDESK_INITIALIZED {request["projectId"]} {request["intentId"]} {request["registrationDigest"]}'
        # The real reader validates the selected private profile before CDP.
        # Keep it unmocked so an unsupported budget cannot pass a sink double.
        rows=[[role,text,True,len(text.encode()),f"turn-{role}",f"unit-{role}",f"message-{role}"]
              for role,text in [("user",message),("assistant",answer)]]
        cdp=types.SimpleNamespace(get_current_url=lambda:ISSUED,evaluate=lambda script:["SEMANTIC_UNIT",2,False,0,0,rows])
        with tempfile.TemporaryDirectory() as folder:
            sink=bridge.NewConversationSink(ISSUED,Path(folder),2,2)
            with patch.object(sink,"user_message_digests",return_value=[bridge.digest(bridge.normalize_message(message))]):
                self.assertIsNone(sink.verify_initialization_ack(cdp,request["projectId"],request["intentId"],request["registrationDigest"],message))

    def test_initialization_body_and_registration_bounds(self):
        for registration, body in [("A" * 64, "body"), ("a" * 63, "body"),
                                   ("a" * 64, "bad\x00body"), ("a" * 64, "/" * 20000)]:
            with self.subTest(registration=registration[:2], body_len=len(body)):
                with self.assertRaises(bridge.Attention):
                    bridge.compose_initialization_message("project-first", "initialize-first",
                        registration, body)


if __name__ == "__main__":
    unittest.main()
