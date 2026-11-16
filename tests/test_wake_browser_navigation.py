"""Execute the actual metadata-only navigation diagnostic without a browser."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import types
import unittest
from unittest.mock import Mock

from test_wake_bridge import wake_bridge as bridge


DOM = r"""
const fs = require('fs');
const {script, options:o} = JSON.parse(fs.readFileSync(0,'utf8'));
global.location = {href:o.location || 'https://chatgpt.com/'};
global.document = {URL:o.documentUrl || location.href, title:o.title || '',
  querySelector:() => o.editor || o.turns ? {} : null};
if (!o.noBody) document.body = {childNodes:o.emptyBody ? [] : [1],
  get innerText() {if(o.forbidBodyRead) throw Error('PRIVATE_BODY_MUST_NOT_BE_READ'); return o.text || '';}};
global.performance = {getEntriesByType:() => o.noTiming ? [] : [{responseStatus:o.status}]};
process.stdout.write(JSON.stringify(eval(script)));
"""


@unittest.skipUnless(shutil.which('node'), 'Node.js required for actual diagnostic JavaScript')
class BrowserNavigationTests(unittest.TestCase):
    def setUp(self):
        self.profile = tempfile.TemporaryDirectory()
        self.addCleanup(self.profile.cleanup)
        self.sink = bridge.CdpSink('https://chatgpt.com/c/fixture', Path(self.profile.name), 30, 20)

    def observe(self, *, history=None, **options):
        def evaluate(script):
            completed = subprocess.run([shutil.which('node'), '-e', DOM],
                input=json.dumps(dict(script=script,options=options)), text=True,
                capture_output=True, timeout=5, check=True)
            return json.loads(completed.stdout)
        cdp = types.SimpleNamespace(evaluate=evaluate,
            get_navigation_history=Mock(return_value=history if history is not None else
                (0, [types.SimpleNamespace(url=self.sink.url)])))
        return self.sink.browser_navigation_observation(cdp)

    def test_home_runtime_and_browser_history_remain_independent_not_receipt_authority(self):
        result = self.observe(title='ChatGPT',status=200,text='PRIVATE_HOME_TEXT')
        self.assertEqual(result, dict(schemaVersion=1, documentRoute='HOME', historyRoute='SAME_CONVERSATION',
            title='CHATGPT',body='NONEMPTY',notice='NONE',responseStatus=200))
        self.assertNotIn('PRIVATE',json.dumps(result))
        self.assertNotIn('chatgpt.com',json.dumps(result))

    def test_closed_notice_categories_and_status_preserve_no_body_or_title(self):
        for text,notice in [('Too many requests PRIVATE','RATE_LIMITED'),
                ('Verify you are human PRIVATE','SECURITY_CHECK'),
                ('Sorry, you have been blocked PRIVATE','ACCESS_DENIED'),
                ('Please sign in PRIVATE','LOGIN_REQUIRED'),
                ('Network connection lost PRIVATE','NETWORK_ERROR')]:
            with self.subTest(notice=notice):
                result = self.observe(title='PRIVATE_TITLE',text=text,status=403)
                self.assertEqual((result['notice'],result['title'],result['responseStatus']), (notice,'OTHER',403))
                self.assertNotIn('PRIVATE',json.dumps(result))

    def test_conversation_editor_foreign_and_ambiguous_home_never_scan_body(self):
        for options in [dict(location=self.sink.url),dict(editor=True),dict(turns=True),
                dict(location='https://foreign.test/'),dict(location='https://chatgpt.com/?q=private')]:
            with self.subTest(options=options):
                result=self.observe(**options,forbidBodyRead=True)
                self.assertEqual(result['notice'],'NOT_INSPECTED')

    def test_title_and_empty_document_are_distinct_from_authentication(self):
        self.assertEqual(self.observe(title='Just a moment...',noBody=True)['title'],'SECURITY_CHECK')
        result=self.observe(emptyBody=True,noTiming=True)
        self.assertEqual((result['body'],result['title'],result['notice'],result['responseStatus']),('EMPTY','EMPTY','NONE',None))

    def test_missing_or_invalid_history_is_unavailable_without_guessing(self):
        for history in [(True,[]),(3,[]),(0,[object()]),[],(0,[types.SimpleNamespace(url=None)])]:
            with self.subTest(history=history):
                self.assertEqual(self.observe(history=history)['historyRoute'],'UNAVAILABLE')

    def test_unknown_status_is_not_http_success(self):
        for status in [None,True,600,-1,'200']:
            with self.subTest(status=status):
                self.assertIsNone(self.observe(status=status)['responseStatus'])

    def test_unexpected_fields_and_private_values_are_rejected(self):
        valid=dict(documentUrl=self.sink.url,title='OTHER',body='NONEMPTY',notice='NOT_INSPECTED',responseStatus=None)
        for change in [dict(rawBody='PRIVATE'),dict(title='PRIVATE'),dict(notice='PRIVATE'),dict(responseStatus=True)]:
            with self.subTest(change=change), self.assertRaises(bridge.Attention):
                self.sink.browser_navigation_observation(types.SimpleNamespace(evaluate=lambda _:dict(valid,**change)))


if __name__ == '__main__':
    unittest.main()
