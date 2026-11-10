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
    def test_trace_report_uses_exact_delivery_and_masks_observer_exceptions(self):
        delivery=dict(messageDigest='a'*64,event=dict(message='PRIVATE_USER'))
        value=dict(schemaVersion=1,basis='TASK_BOUND_TRACE_UI_NOT_NATIVE_RECEIPT',state='CONTROL_UNSUPPORTED',
            openAttempted=False,openRequested=False,openOutcomeUnknown=False,controlType='UNKNOWN',controlCount=1,
            expandedState='CLOSED',panelKind='NONE',panelCount=0,knownCodes=[],cachedInvariants=[],mentionedTools=[],
            requestMatchCount=0,argumentTypes=None,unknownOuterFields=False,scanTruncated=False)
        sink=types.SimpleNamespace(inspect_response_trace=mock.Mock(return_value=value))
        cdp=object();output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_response_trace(sink,cdp,delivery)
        sink.inspect_response_trace.assert_called_once_with(cdp,'PRIVATE_USER','a'*64)
        self.assertEqual(json.loads(output.getvalue().split('PROBE:TRACE:')[1]),value)
        self.assertNotIn('PRIVATE_',output.getvalue())
        sink.inspect_response_trace.side_effect=RuntimeError('PRIVATE_ERROR')
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_response_trace(sink,cdp,delivery)
        self.assertEqual(output.getvalue(),'PROBE:TRACE_STATE:UNPROVEN\n')
        sink.trace_failure=lambda:dict(schemaVersion=1,phase='READ',boundary='SNAPSHOT',reason='OBSERVER_EXCEPTION')
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_response_trace(sink,cdp,delivery)
        lines=output.getvalue().splitlines()
        self.assertEqual(lines[0],'PROBE:TRACE_STATE:UNPROVEN')
        self.assertEqual(json.loads(lines[1].split('PROBE:TRACE_FAILURE:')[1]),sink.trace_failure())
        self.assertNotIn('PRIVATE_',output.getvalue())
        sink.inspect_historical_response_trace=mock.Mock(return_value=value)
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_response_trace(sink,cdp,delivery,historical=True)
        sink.inspect_historical_response_trace.assert_called_once_with(cdp,'PRIVATE_USER','a'*64)
        self.assertNotIn('PRIVATE_',output.getvalue())

    def test_menu_report_binds_only_existing_delivery_and_never_exports_private_errors(self):
        delivery=dict(messageDigest='a'*64,event=dict(message='PRIVATE_USER'))
        value=dict(schemaVersion=1,basis='TRANSIENT_MENU_ONLY_NOT_TOOL_CATALOG',state='UNPROVEN',
            openAttempted=False,openRequested=False,openOutcomeUnknown=False,closeAttempted=False,
            closedVerified=False,v5ItemCount=0,v5DisabledCount=0,appsItemCount=0,moreItemCount=0)
        sink=types.SimpleNamespace(inspect_composer_app_menu=mock.Mock(return_value=value))
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_app_menu(sink,object(),delivery)
        self.assertEqual(json.loads(output.getvalue().split('PROBE:APP_MENU:')[1]),value)
        self.assertNotIn('PRIVATE_',output.getvalue())
        self.assertEqual(sink.inspect_composer_app_menu.call_args.args[1:],('PRIVATE_USER','a'*64))
        sink.inspect_composer_app_menu.side_effect=RuntimeError('PRIVATE_CREDENTIAL')
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_app_menu(sink,object(),delivery)
        self.assertEqual(output.getvalue(),'PROBE:APP_MENU_STATE:UNPROVEN\n')

    def test_composer_app_report_keeps_private_failures_closed(self):
        value=dict(schemaVersion=1,scope='COMPOSER_FORM',editorCount=1,plusButtonCount=1,
            plusButtonType='BUTTON',plusButtonState='ENABLED',plusExpanded='CLOSED',
            v5MarkerCount=0,basis='PRESENTATION_ONLY_NOT_TOOL_CATALOG')
        sink=types.SimpleNamespace(composer_app_observation=mock.Mock(return_value=value))
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_composer_apps(sink,object())
        self.assertEqual(json.loads(output.getvalue().split('PROBE:APP:')[1]),value)
        sink.composer_app_observation.side_effect=RuntimeError('PRIVATE_DRAFT_OR_CREDENTIAL')
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_composer_apps(sink,object())
        self.assertEqual(output.getvalue(),'PROBE:APP_STATE:UNPROVEN\n')

    def test_pending_continuation_observation_preserves_phase_and_exact_scope(self):
        url = "https://chatgpt.com/c/fixture"
        identity = "wake-intent-" + "b" * 64
        message = "frozen task continuation"
        refs = ["catdesk.wake.v1:" + k + ":" + v for k, v in [
            ("purpose", "NEW_TASK"), ("source-event", "web-work-" + "a" * 64),
            ("source-turn", "initial-context-" + "c" * 64),
            ("priority", "0"), ("expires-utc", "100")]]
        target = dict(generation=1, url=url, digest=hashlib.sha256(url.encode()).hexdigest())
        value = dict(phase="SUBMITTING", receipt=None,
            messageDigest=hashlib.sha256(message.encode()).hexdigest(), target=target,
            event=dict(eventId=identity, projectId="alpha", eventType="engineering_attention",
                targetGeneration=1, message=message, auditReferences=refs))
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "deliveries" / (identity + ".json")
            path.parent.mkdir()
            def read(item, **kwargs):
                path.write_text(json.dumps(item), encoding="utf-8")
                checksum = hashlib.sha256(path.read_bytes()).hexdigest()
                return self.probe.load_delivery_message(path, checksum, url, **kwargs)
            for phase in ("CLAIMED", "SUBMITTING", "ATTENTION"):
                current = value | {"phase": phase}
                self.assertEqual(read(current), message)
                if phase == "SUBMITTING":
                    self.assertEqual(read(current, receipt_only=True), current)
                else:
                    with self.assertRaises(ValueError):
                        read(current, receipt_only=True)
                with self.assertRaises(ValueError):
                    read(current, response_only=True, sent_diagnostics=True)
            for changed in [value | {"phase": "SENT"}, value | {"receipt": {}},
                value | {"messageDigest": "0" * 64},
                value | {"target": target | {"url": url + "-foreign"}},
                value | {"event": value["event"] | {"targetGeneration": 2}},
                value | {"event": value["event"] | {"auditReferences": refs[:-1]}},
                value | {"event": value["event"] | {"auditReferences": refs + [refs[0]]}},
                *[value | {"event": value["event"] | {"auditReferences": [
                    ref.replace(old, new) for ref in refs]}}
                    for old, new in [("NEW_TASK", "ROLLOVER"), ("web-work-", "foreign-"),
                        ("initial-context-", "foreign-"), ("priority:0", "priority:256")]]]:
                with self.subTest(changed=changed), self.assertRaises((ValueError, KeyError)):
                    read(changed)
            self.assertEqual(read(value), message)
            self.assertEqual(json.loads(path.read_text()), value)

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

    def test_source_review_diagnostic_flag_cannot_select_ui_actions_or_incomplete_scope(self):
        with tempfile.TemporaryDirectory() as profile:
            required=['--read-only','--profile',profile,'--delivery-file','delivery.json',
                '--response-only','--task-diagnostics','--source-review-diagnostics']
            cases=[required+[flag] for flag in ['--receipt-only','--historical-response',
                '--inspect-app-menu','--inspect-response-trace','--inspect-historical-trace']]
            for remove in ['--read-only','--response-only','--task-diagnostics']:
                cases.append([item for item in required if item!=remove])
            for flags in cases:
                argv=['probe','--conversation-url','https://chatgpt.com/c/fixture',
                    '--primitives','wake_bridge.py','--primitives-sha256','a'*64]+flags
                with self.subTest(flags=flags),mock.patch.object(sys,'argv',argv), \
                    mock.patch.object(self.probe,'load_observation_sink',return_value=mock.Mock()), \
                    mock.patch.object(self.probe,'load_delivery_message') as loader:
                    self.assertEqual(self.probe.main(),2)
                    loader.assert_not_called()

    def test_existing_response_wait_flag_is_owner_scope_only_without_ui_actions(self):
        with tempfile.TemporaryDirectory() as profile:
            required=['--read-only','--profile',profile,'--delivery-file','delivery.json',
                '--response-only','--task-diagnostics','--await-existing-response']
            cases=[required+[flag] for flag in ['--receipt-only','--historical-response',
                '--source-review-diagnostics','--inspect-app-menu','--inspect-response-trace',
                '--inspect-historical-trace']]
            for remove in ['--read-only','--response-only','--task-diagnostics']:
                cases.append([item for item in required if item!=remove])
            for flags in cases:
                argv=['probe','--conversation-url','https://chatgpt.com/c/fixture',
                    '--primitives','wake_bridge.py','--primitives-sha256','a'*64]+flags
                with self.subTest(flags=flags),mock.patch.object(sys,'argv',argv), \
                    mock.patch.object(self.probe,'load_observation_sink',return_value=mock.Mock()), \
                    mock.patch.object(self.probe,'load_delivery_message') as loader:
                    self.assertEqual(self.probe.main(),2)
                    loader.assert_not_called()

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
    def test_receipt_reader_uses_existing_no_send_primitive_and_exports_no_content(self):
        delivery=dict(messageDigest="a"*64,event=dict(eventId="web-work-"+"b"*64,message="PRIVATE_MESSAGE"),
                      target=dict(generation=1,digest="c"*64))
        sink=mock.Mock()
        sink.reconcile_persisted_receipt.return_value=types.SimpleNamespace(record_id=delivery["event"]["eventId"],
            target_sha256="c"*64,message_sha256="a"*64,browser_sent_at_unix=10)
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_persisted_receipt(sink,object(),delivery)
        self.assertIn('PROBE:RECEIPT:',output.getvalue())
        self.assertNotIn('PRIVATE_MESSAGE',output.getvalue())
        self.assertEqual([call[0] for call in sink.method_calls],["reconcile_persisted_receipt"])
        for changed in ("message_sha256","target_sha256","record_id"):
            setattr(sink.reconcile_persisted_receipt.return_value,changed,"PRIVATE_MISMATCH")
            output=io.StringIO()
            with contextlib.redirect_stdout(output):self.probe.report_persisted_receipt(sink,object(),delivery)
            self.assertEqual(output.getvalue(),"PROBE:RECEIPT_STATE:UNPROVEN\n")
        sink.reconcile_persisted_receipt.side_effect=RuntimeError("PRIVATE_ERROR")
        output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_persisted_receipt(sink,object(),delivery)
        self.assertNotIn("PRIVATE_",output.getvalue())

    def test_receipt_report_forwards_existing_deadline_without_new_authority(self):
        delivery=dict(messageDigest="a"*64,event=dict(eventId="web-work-"+"b"*64,message="PRIVATE_MESSAGE"),
                      target=dict(generation=1,digest="c"*64))
        sink=mock.Mock()
        sink.reconcile_persisted_receipt.return_value=types.SimpleNamespace(record_id=delivery["event"]["eventId"],
            target_sha256="c"*64,message_sha256="a"*64,browser_sent_at_unix=10)
        cdp=object();output=io.StringIO()
        with contextlib.redirect_stdout(output):self.probe.report_persisted_receipt(sink,cdp,delivery,deadline=42.5)
        sink.reconcile_persisted_receipt.assert_called_once_with(cdp,"a"*64,"PRIVATE_MESSAGE",
            delivery["event"]["eventId"],"c"*64,observation_deadline=42.5)
        self.assertEqual([call[0] for call in sink.method_calls],["reconcile_persisted_receipt"])
        self.assertNotIn("PRIVATE_",output.getvalue())

    def test_receipt_probe_shares_startup_readiness_and_receipt_budget_without_reload(self):
        target="https://chatgpt.com/c/fixture"
        delivery=dict(messageDigest="a"*64,event=dict(eventId="web-work-"+"b"*64,message="PRIVATE_MESSAGE"),
                      target=dict(generation=1,digest="c"*64))
        for startup_seconds in (20.0,70.0):
            clock=[0.0]
            browser=mock.MagicMock()
            def start(**_):
                clock[0]+=startup_seconds
                return mock.MagicMock(__enter__=mock.Mock(return_value=browser),__exit__=mock.Mock(return_value=False))
            sink=mock.Mock()
            sink.browser_surface_observation.return_value=dict(route="EXACT",document="COMPLETE",editor="READY",
                composer="DRAFT_PRESENT",history="PRESENT",generation="IDLE",assistant="PRESENT",
                exchange="UNATTRIBUTED",recovery="OBSERVATION_ONLY")
            def receipt(*_,observation_deadline):
                self.assertEqual(observation_deadline,60.0)
                if clock[0]>=observation_deadline:raise RuntimeError("PRIVATE_EXPIRED_OBSERVATION")
                return types.SimpleNamespace(record_id=delivery["event"]["eventId"],target_sha256="c"*64,
                    message_sha256="a"*64,browser_sent_at_unix=10)
            sink.reconcile_persisted_receipt.side_effect=receipt
            with tempfile.TemporaryDirectory() as profile:
                args=["probe","--conversation-url",target,"--profile",profile,"--seconds","60","--read-only",
                    "--primitives","fixture.py","--primitives-sha256","d"*64,"--receipt-only",
                    "--delivery-file","fixture.json","--delivery-sha256","e"*64]
                output=io.StringIO()
                with mock.patch.dict(sys.modules,{"seleniumbase":types.SimpleNamespace(SB=start)}), mock.patch.object(sys,"argv",args), \
                        mock.patch.object(self.probe,"load_observation_sink",return_value=sink), \
                        mock.patch.object(self.probe,"load_delivery_message",return_value=delivery), \
                        mock.patch.object(self.probe,"select_registered_target_tab",return_value="TARGET_TAB_EXACT"), \
                        mock.patch.object(self.probe.time,"monotonic",side_effect=lambda:clock[0]), \
                        mock.patch.object(self.probe.time,"sleep") as sleep, contextlib.redirect_stdout(output):
                    self.assertEqual(self.probe.main(),0)
                sleep.assert_not_called()
            self.assertEqual(clock[0],startup_seconds)
            sink.reconcile_persisted_receipt.assert_called_once()
            browser.cdp.get.assert_not_called()
            browser.cdp.click.assert_not_called()
            browser.cdp.press_keys.assert_not_called()
            browser.cdp.send_keys.assert_not_called()
            self.assertNotIn("PRIVATE_",output.getvalue())
            self.assertIn("PROBE:RECEIPT:" if startup_seconds<60 else "PROBE:RECEIPT_STATE:UNPROVEN",output.getvalue())

    def test_event_probe_observes_only_exact_page_and_never_retries_home(self):
        target = "https://chatgpt.com/c/fixture"
        for route in ("EXACT", "HOME"):
            cdp = types.SimpleNamespace(get_tabs=lambda: [], get_current_url=lambda: target if route == "EXACT" else "https://chatgpt.com/")
            browser = mock.MagicMock(); browser.cdp = cdp
            factory = mock.Mock(return_value=mock.MagicMock(__enter__=mock.Mock(return_value=browser), __exit__=mock.Mock(return_value=False)))
            sink = mock.Mock()
            sink.browser_surface_observation.return_value = dict(route=route, document="COMPLETE", editor="READY", composer="EMPTY", history="PRESENT", generation="IDLE", assistant="ABSENT", exchange="UNATTRIBUTED", recovery="OBSERVATION_ONLY")
            sink.submission_diagnostic_sample.return_value = (dict(composerClass="EMPTY", expectedUserCount=0), "PRIVATE_COMPARISON")
            with tempfile.TemporaryDirectory() as profile:
                argv=["probe", "--await-start", "--read-only", "--profile", profile, "--primitives", "wake_bridge.py", "--primitives-sha256", "a"*64, "--delivery-file", "delivery.json", "--delivery-sha256", "b"*64, "--conversation-url", target, "--seconds", "5"]
                output=io.StringIO()
                with mock.patch.object(sys,"argv",argv), mock.patch.object(sys,"stdin",io.StringIO("START\n")), \
                        mock.patch.dict(sys.modules,{"seleniumbase":types.SimpleNamespace(SB=factory)}), \
                        mock.patch.object(self.probe,"load_observation_sink",return_value=sink), \
                        mock.patch.object(self.probe,"load_delivery_message",return_value="PRIVATE_EXPECTED_MESSAGE"), \
                        mock.patch.object(self.probe.time,"monotonic",side_effect=[0,0,10]), contextlib.redirect_stdout(output):
                    self.assertEqual(self.probe.main(),0)
                self.assertNotIn("PRIVATE_",output.getvalue())
                if route == "HOME":
                    sink.submission_diagnostic_sample.assert_not_called()
                    self.assertIn("PROBE:SUBMISSION_STATE:TARGET_UNAVAILABLE",output.getvalue())
                else:
                    self.assertEqual(sink.submission_diagnostic_sample.call_count,2)
                    self.assertIn("PROBE:SUBMISSION_STATE:OBSERVED",output.getvalue())
                self.assertNotIn("HOME_CHECK",output.getvalue())
                self.assertEqual(browser.activate_cdp_mode.call_count,1)

    def test_delivery_reader_requires_exact_persisted_identity_and_scope(self):
        url = "https://chatgpt.com/c/fixture"
        identity = "web-work-" + "a" * 64
        message = "frozen ordinary invitation"
        delivery = {"phase": "ATTENTION", "receipt": None,
            "messageDigest": hashlib.sha256(message.encode()).hexdigest(),
            "event": {"eventId": identity, "projectId": "alpha", "eventType": "engineering_attention",
                "auditReferences": [], "targetGeneration": 1, "message": message},
            "target": {"generation": 1, "url": url, "digest": hashlib.sha256(url.encode()).hexdigest()}}
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root) / "deliveries"; directory.mkdir()
            path = directory / (identity + ".json")
            def save(value):
                path.write_text(json.dumps(value), encoding="utf-8")
                return hashlib.sha256(path.read_bytes()).hexdigest()
            checksum = save(delivery)
            self.assertEqual(self.probe.load_delivery_message(path, checksum, url), message)
            with self.assertRaises(ValueError):self.probe.load_delivery_message(path,checksum,url,receipt_only=True)
            submitted=json.loads(json.dumps(delivery));submitted["phase"]="SUBMITTING"
            self.assertEqual(self.probe.load_delivery_message(path,save(submitted),url,receipt_only=True),submitted)
            sent=json.loads(json.dumps(delivery));sent["phase"]="SENT"
            with self.assertRaises(ValueError):self.probe.load_delivery_message(path,save(sent),url,response_only=True)
            sent["receipt"]=dict(schemaVersion=1,eventId=identity,targetGeneration=1,targetDigest=sent["target"]["digest"],messageDigest=sent["messageDigest"],sentUtc=1,evidence="EXACT_USER_MESSAGE_APPENDED")
            self.assertEqual(self.probe.load_delivery_message(path,save(sent),url,response_only=True),sent)
            sent["receipt"]["messageDigest"]="f"*64
            with self.assertRaises(ValueError):self.probe.load_delivery_message(path,save(sent),url,response_only=True)
            save(delivery)
            self.assertEqual(path.read_text(), json.dumps(delivery))
            with self.assertRaises(ValueError): self.probe.load_delivery_message(path, "0" * 64, url)
            for key, value in [("phase", "SENT"), ("receipt", {}), ("messageDigest", "0" * 64)]:
                changed = dict(delivery, **{key: value})
                with self.subTest(key=key), self.assertRaises(ValueError):
                    self.probe.load_delivery_message(path, save(changed), url)
            for key, value in [("eventType", "source_review"), ("targetGeneration", 2), ("auditReferences", ["private-review"]), ("eventId", "other")]:
                changed = dict(delivery, event=dict(delivery["event"], **{key: value}))
                with self.subTest(key=key), self.assertRaises(ValueError):
                    self.probe.load_delivery_message(path, save(changed), url)
            with self.assertRaises(ValueError): self.probe.load_delivery_message(path, save(delivery), "https://chatgpt.com/c/foreign")

    def test_submission_samples_export_only_comparisons_and_keep_duplicate_counts_advisory(self):
        value = dict(composerClass="OTHER", composerBytes=21, legacyAvailable=False,
            readersAgree=None, legacyMatchesExpected=None, expectedUserCount=2,
            lastUserMatches=True, trustedMatches=2, turnsAfter=1, stable=False, coverage="VISIBLE_BOUNDED_ONLY")
        sink = mock.Mock()
        sink.submission_diagnostic_sample.return_value = (value, ("PRIVATE_DRAFT_HASH", "PRIVATE_HISTORY_HASH"))
        output = io.StringIO()
        with contextlib.redirect_stdout(output): self.probe.report_submission(sink, object(), "PRIVATE_EXPECTED_MESSAGE", sleep=lambda _: None)
        rows = output.getvalue().splitlines()
        observed = json.loads(rows[0].removeprefix("PROBE:SUBMISSION:"))
        self.assertEqual(observed["expectedUserCount"], 2)
        self.assertTrue(observed["stable"])
        self.assertNotIn("PRIVATE_", output.getvalue())
        sink.submission_diagnostic_sample.side_effect = ValueError("PRIVATE_FAILURE")
        output = io.StringIO()
        with contextlib.redirect_stdout(output): self.probe.report_submission(sink, object(), "PRIVATE_EXPECTED_MESSAGE", sleep=lambda _: None)
        self.assertEqual(output.getvalue(), "PROBE:SUBMISSION_STATE:OBSERVATION_UNAVAILABLE\n")

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
            self.assertEqual([r for r in rows if r.startswith("PROBE:PHASE:")], [
                "PROBE:PHASE:IMPORT_BROWSER", "PROBE:PHASE:BROWSER_START",
                "PROBE:PHASE:CDP_NAVIGATION", "PROBE:PHASE:OBSERVING", "PROBE:PHASE:BROWSER_CLOSED"])
            rows = [r for r in rows if not r.startswith("PROBE:PHASE:")]
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
