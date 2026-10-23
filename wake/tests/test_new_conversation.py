"""Offline CDP doubles for one-shot ChatGPT conversation bootstrap."""

import importlib.util
import io
import json
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
adapter_spec = importlib.util.spec_from_file_location("bootstrap_adapter", ROOT / "wake/adapter.py")
adapter = importlib.util.module_from_spec(adapter_spec)
with patch.object(sys, "argv", [str(ROOT / "wake/adapter.py"), str(ROOT / "wake")]):
    adapter_spec.loader.exec_module(adapter)
bridge_spec = importlib.util.spec_from_file_location("bootstrap_bridge", ROOT / "scripts/wake_bridge.py")
bridge = importlib.util.module_from_spec(bridge_spec)
sys.modules[bridge_spec.name] = bridge
bridge_spec.loader.exec_module(bridge)
REAL_NEW_SINK = bridge.NewConversationSink

SOURCE = "https://chatgpt.com/c/source-123"
ISSUED = "https://chatgpt.com/c/issued-456"


def observed_rows(*rows, nested=0, keyed=0, count=None):
    """Synthetic CDP output; raw DOM keys must never escape the sink."""
    units = [[role, text, len(text.encode("utf-16-le")) // 2 <= 4096,
              min(len(text.encode("utf-16-le")) // 2, 1000000), turn, unit, message]
             for role, text, turn, unit, message in rows]
    return ["AUTHOR_ROLE", len(units) if count is None else count, False, nested, keyed, units]


class StructuralCdp:
    def __init__(self, url=bridge.NEW_CONVERSATION_HOME):
        self.url = url
        self.draft = ""
        self.navigation = []
        self.users = 0
        self.assistants = 0
        self.turns = 0
        self.visible_selectors = set()
        self.snapshot = observed_rows()

    def get_current_url(self):
        return self.url

    def get(self, url):
        self.navigation.append(url)
        self.url = url

    def is_element_visible(self, selector):
        return selector in self.visible_selectors

    def get_attribute(self, selector, attribute, timeout):
        return None

    def get_text(self, selector, timeout):
        return self.draft

    def evaluate(self, script):
        if script == "document.readyState":
            return "complete"
        if "return composerText(visible[0])" in script:
            return getattr(self, "plain_draft", self.draft)
        if "new TextEncoder().encode(text).length" in script:
            return ["ready", self.draft]
        if "const count = selector" in script:
            return [self.users, self.assistants, self.turns]
        if "unique.slice(-16)" in script:
            return self.snapshot
        if "if (visible.length !== 1) return ['count'" in script:
            return ["ready", not self.draft]
        raise AssertionError("unexpected CDP query")


class AttemptCdp:
    def __init__(self, scenario="success"):
        self.url = SOURCE
        self.scenario = scenario
        self.draft = ""
        self.sent = 0
        self.navigation = []
        self.timeline = []

    def get_current_url(self):
        return self.url

    def get(self, url):
        self.navigation.append(url)
        self.url = url

    def send_keys(self, selector, text, timeout):
        self.timeline.append("type")
        self.draft = text
        self.typed_message = text

    def insert_text(self, text):
        self.timeline.append("insert")
        self.draft = text
        self.typed_message = text

    def press_keys(self, selector, text, timeout):
        raise AssertionError("human-speed press_keys must not type a handoff")

    def click(self, selector, timeout):
        self.sent += 1
        self.timeline.append("submit")
        if self.scenario == "click_unknown":
            raise bridge.PostSubmitUnknown("SUBMIT_CLICK_UNKNOWN")
        self.draft = ""
        if self.scenario == "source_revisited":
            self.url = SOURCE
        elif self.scenario == "foreign_url":
            self.url = "https://example.invalid/c/issued-456"
        else:
            self.url = ISSUED


class AttemptSink:
    def __init__(self, source_url, profile, ui_timeout, submit_timeout):
        self.source_url = source_url
        self.url = source_url
        self.destination_url = None
        self.ui_timeout = ui_timeout
        self.submit_timeout = submit_timeout

    def exact(self, url):
        return url == self.url

    def wait_for_page_readiness(self, sb, cdp):
        if cdp.url != SOURCE:
            raise bridge.Attention("BOOTSTRAP_SOURCE_DRIFT")
        return cdp

    def wait_for_idle(self, cdp, timeout):
        pass

    def pre_typing_ready(self, cdp):
        if cdp.url != SOURCE or cdp.draft:
            raise bridge.Attention("BOOTSTRAP_SOURCE_DRIFT")
        if cdp.scenario == "source_load_error":
            raise bridge.Attention("BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER")

    def open_empty_home(self, cdp):
        cdp.get(bridge.NEW_CONVERSATION_HOME)
        if cdp.scenario == "wrong_home":
            cdp.url = SOURCE
        elif cdp.scenario == "existing_draft":
            cdp.draft = "operator draft"
        elif cdp.scenario == "existing_messages":
            cdp.users = 1

    def check_empty_home(self, cdp):
        if cdp.url != bridge.NEW_CONVERSATION_HOME:
            raise bridge.Attention("BOOTSTRAP_HOME_DRIFT")
        if cdp.draft:
            raise bridge.Attention("EXISTING_DRAFT")
        if getattr(cdp, "users", 0):
            raise bridge.Attention("BOOTSTRAP_HOME_HAS_TURNS")

    def observe_home_draft(self, cdp, message):
        if cdp.url != bridge.NEW_CONVERSATION_HOME or getattr(cdp, "users", 0):
            raise bridge.Attention("BOOTSTRAP_HOME_HAS_TURNS")
        if not cdp.draft:
            return {"classification": "EMPTY_HOME", "actualDigest": None, "actualBytes": 0}
        normalized = bridge.normalize_message(cdp.draft)
        return {"classification": "EXACT_OWNED" if normalized == bridge.normalize_message(message)
                else "MISMATCH", "actualDigest": bridge.digest(normalized),
                "actualBytes": len(cdp.draft.encode("utf-8"))}

    def wait_home_draft(self, cdp, message):
        return self.observe_home_draft(cdp, message)

    def raw_editor_text(self, cdp):
        return cdp.draft

    def clear_archived_home_draft(self, cdp, message, actual_text):
        self.check_typed_home(cdp, message)
        if cdp.draft != actual_text:
            raise bridge.Attention("BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED")
        cdp.timeline.append("clear")
        cdp.draft = ""
        self.check_empty_home(cdp)

    def editor_selector(self, cdp, *, prevalidated=False):
        if not prevalidated:
            raise AssertionError("composer must be prevalidated")
        return bridge.EDITOR

    def check_typed_home(self, cdp, message):
        if cdp.url != bridge.NEW_CONVERSATION_HOME:
            raise bridge.Attention("BOOTSTRAP_HOME_DRIFT")
        if bridge.normalize_message(cdp.draft) != bridge.normalize_message(message):
            raise bridge.Attention("BOOTSTRAP_DRAFT_DRIFT")

    def insert_empty_home_draft(self, cdp, message):
        self.check_empty_home(cdp)
        cdp.insert_text(bridge.keyboard_safe_message(message))
        self.check_typed_home(cdp, message)

    def repair_home_draft(self, cdp, message, actual_text):
        self.check_typed_home(cdp, actual_text)
        if cdp.draft != actual_text:
            raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_UNVERIFIED")
        cdp.insert_text(bridge.keyboard_safe_message(message))
        self.check_typed_home(cdp, message)
        self.check_plain_home_draft(cdp)

    def check_plain_home_draft(self, cdp):
        if getattr(cdp, "rich_link", False):
            raise bridge.Attention("BOOTSTRAP_RICH_DRAFT_UNVERIFIED")

    def normalize_owned_home_draft(self, cdp, message):
        self.check_typed_home(cdp, message)
        if getattr(cdp, "rich_link", False):
            cdp.timeline.append("unlink")
            cdp.rich_link = False
            return True
        return False

    def wait_for_send_control(self, cdp, timeout):
        return "send"

    def smoke_path_submit(self, cdp, selector, timeout, editor_selector):
        cdp.click(selector, timeout)

    def latch_issued_url(self, cdp):
        return REAL_NEW_SINK.latch_issued_url(self, cdp)

    def wait_for_submission_accepted(self, cdp, prior, message_digest, message):
        self._check_issued(cdp)
        cdp.timeline.append("accepted")
        return 99.0

    def wait_for_response_completion(self, cdp, message, *, allow_retry, observer):
        self._check_issued(cdp)
        assert allow_retry is False
        cdp.timeline.append("completion")
        observer("RESPONSE_COMPLETED")

    def durable_receipt_round_trip(self, cdp):
        self._check_issued(cdp)
        cdp.timeline.append("fresh-document")

    def confirm_exact_receipt(self, cdp, prior, message_digest, intent_id, message, *, wall_time):
        self._check_issued(cdp)
        cdp.timeline.append("exact-receipt")
        return bridge.receipt_for(intent_id, message, self.url, wall_time())

    def prove_single_bootstrap_user(self, cdp, message):
        self._check_issued(cdp)
        if cdp.scenario == "load_error_after_reload":
            raise bridge.PostSubmitUnknown("BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER")
        if cdp.sent != 1:
            raise bridge.PostSubmitUnknown("BOOTSTRAP_RECEIPT_SEQUENCE_DRIFT")

    def user_message_digests(self, cdp):
        self._check_issued(cdp)
        return [bridge.digest(bridge.normalize_message(cdp.typed_message))]

    def _check_issued(self, cdp):
        if cdp.url != ISSUED:
            raise AssertionError("destination not latched")

    def readiness_reason(self, cdp):
        return None

    def visible_message_snapshot(self, cdp):
        cdp.timeline.append("snapshot")
        coverage = {
            "selectorTier": "AUTHOR_ROLE", "visibleRoleCount": 1, "countOverflow": False,
            "nestedCollapsed": 0, "keyedCollapsed": 0, "omittedBeforeWindow": 0,
            "returned": 1, "identityMissing": 1, "identityUncertain": 0,
            "revisionIncomplete": 0, "textIncomplete": 0,
            "anchorStability": "UNVERIFIED",
        }
        message = {
            "role": "ASSISTANT", "text": "bounded private status", "visibleUnits": 22,
            "textComplete": True, "turnHash": None, "roleTurnHash": None,
            "messageHash": None, "identityBasis": "MISSING",
            "revisionHash": bridge.digest("bounded private status"), "revisionComplete": True,
        }
        if cdp.scenario == "incomplete_body":
            message["textComplete"] = False
            coverage["textIncomplete"] = 1
        if cdp.scenario == "large_snapshot":
            message["text"] = "😀" * 4096
            message["visibleUnits"] = 4096
        if cdp.scenario == "many_snapshot":
            messages = []
            for index in range(12):
                item = dict(message)
                item["text"] = "x" * 800
                item["visibleUnits"] = 800
                item["identityBasis"] = "UNIT_KEY"
                item["messageHash"] = bridge.digest(f"message-{index}")
                messages.append(item)
            coverage.update(visibleRoleCount=12, returned=12, identityMissing=0)
            return {"messages": messages, "truncated": False, "coverage": coverage}
        return {"messages": [message], "truncated": False, "coverage": coverage}

    def message_body_diagnostics(self, cdp):
        cdp.timeline.append("body-diagnostics")
        return {"status": "UNAVAILABLE"}


class NativeBootstrapInsertTests(unittest.TestCase):
    # Execute the actual generated browser guard, not a Python reimplementation.
    # This offline fixture does not create a browser or perform network I/O.
    DOM_FIXTURE = r"""
const fs = require('fs');
const cfg = JSON.parse(fs.readFileSync(0, 'utf8'));
let inserts = 0;
let command = null;
let liveEditors;
global.location = {href: cfg.url || 'https://chatgpt.com/'};
const makeEditor = () => ({
    nodeType: 1, tagName: 'DIV', childNodes: [], isConnected: true,
    isContentEditable: !cfg.textarea,
    get innerText() { return this.childNodes.map(flatText).join(''); },
    getAttribute: () => null, hasAttribute: () => false,
    getClientRects: () => cfg.hidden ? [] : [1],
    contains(node) { return node === this || this.childNodes.includes(node); },
    focus() {
        document.activeElement = cfg.focus === 'elsewhere' ? {} : this;
        if (cfg.focus === 'draft') setText(this, 'operator draft');
        if (cfg.focus === 'url') location.href = 'https://chatgpt.com/c/other';
        if (cfg.focus === 'turn') cfg.turns = true;
        if (cfg.focus === 'replace') liveEditors = [makeEditor()];
        if (cfg.richAfterFocus) decorateEditor(cfg.richAfterFocus);
    },
    setSelectionRange(start, end) { this.selectionStart = start; this.selectionEnd = end; },
});
const setText = (element, text) => {
    if (cfg.textarea) element.value = text;
    else element.childNodes = [{nodeType: 3, nodeValue: text}];
};
const flatText = node => node.nodeType === 3 ? node.nodeValue : node.childNodes.map(flatText).join('');
const getText = element => cfg.textarea ? element.value : element.childNodes.map(flatText).join('');
global.HTMLTextAreaElement = class {};
const editor = makeEditor();
if (cfg.textarea) Object.setPrototypeOf(editor, HTMLTextAreaElement.prototype);
setText(editor, cfg.draft || '');
const elementNode = (tagName, attrs = {}, children = []) => ({
    nodeType: 1, tagName, childNodes: children,
    attributes: Object.entries(attrs).map(([name, value]) => ({name, value})),
    getAttribute: name => attrs[name] ?? null,
    hasAttribute: name => Object.hasOwn(attrs, name),
});
const decorateEditor = kind => {
    if (kind === 'link') editor.childNodes = [elementNode('SPAN', {'text-link-href':'https://example.test/secret'}, editor.childNodes)];
    if (kind === 'anchor') editor.childNodes = [elementNode('A', {href:'https://example.test/secret'}, editor.childNodes)];
    if (kind === 'chip') editor.childNodes = [elementNode('SPAN', {contenteditable:'false'}, editor.childNodes)];
    if (kind === 'image') editor.childNodes.push(elementNode('IMG', {src:'https://example.test/image'}));
    if (kind === 'hidden') {
        const node = elementNode('SPAN', {class:'hidden'}, []);
        node.testStyle = {display:'none'};
        editor.childNodes.push(node);
    }
};
if (cfg.rich) decorateEditor(cfg.rich);
if (cfg.plainParagraph) editor.childNodes = [elementNode('P', {class:'paragraph', 'data-placeholder':'Ask anything'},
    cfg.draft ? editor.childNodes : [elementNode('BR', {class:'ProseMirror-trailingBreak'})])];
liveEditors = cfg.multiple ? [editor, makeEditor()] : [editor];
const selection = {
    rangeCount: 0, anchorNode: null, focusNode: null,
    removeAllRanges() { this.rangeCount = 0; },
    addRange(range) {
        this.rangeCount = 1;
        this.range = range;
        if (cfg.partialSelection) this.range.endOffset = 0;
        this.anchorNode = cfg.selectionDrift ? {} : range.node;
        this.focusNode = this.anchorNode;
        if (cfg.selectionFocusShift) document.activeElement = {};
        if (cfg.richAfterSelection) decorateEditor(cfg.richAfterSelection);
    },
    getRangeAt() { return this.range; },
};
global.window = {getComputedStyle: node => ({visibility:'visible', display:'block', ...node.testStyle}),
    getSelection: () => cfg.noSelection ? null : selection};
global.document = {
    readyState: cfg.loading ? 'loading' : 'complete',
    querySelector: () => cfg.turns ? {} : null,
    querySelectorAll: () => liveEditors,
    createRange: () => ({selectNodeContents(node) {
        this.node = node; this.startContainer = node; this.endContainer = node;
        this.startOffset = 0; this.endOffset = node.childNodes.length;
    }}),
    execCommand(name, _ui, text) {
        inserts++;
        command = name;
        if (cfg.outcome === 'refused') return false;
        setText(editor, cfg.outcome === 'partial' ? text.slice(0, 30) : text);
        if (cfg.outcome === 'changed') setText(editor, getText(editor) + '!');
        if (cfg.outcome === 'throw') throw Error('lost acknowledgement');
        return cfg.outcome !== 'partial';
    },
};
let result;
try { result = eval(cfg.script); } catch (error) { result = 'THREW'; }
process.stdout.write(JSON.stringify({result, inserts, command, text:getText(editor)}));
"""

    def insertion_script(self, message, repair_raw=None):
        scripts = []
        cdp = types.SimpleNamespace(evaluate=lambda script: scripts.append(script) or "INSERTED")
        with tempfile.TemporaryDirectory() as profile:
            sink = REAL_NEW_SINK(SOURCE, Path(profile), 1, 1)
            with patch.object(sink, "check_empty_home"), patch.object(sink, "check_typed_home"), \
                    patch.object(sink, "check_plain_home_draft"):
                if repair_raw is None:
                    sink.insert_empty_home_draft(cdp, message)
                else:
                    sink.repair_home_draft(cdp, message, repair_raw)
        self.assertEqual(len(scripts), 1)
        return scripts[0]

    def run_script(self, message, *, repair_raw=None, **state):
        node = shutil.which("node")
        if node is None:
            self.skipTest("Node is required for the offline native editor DOM fixture")
        state["script"] = self.insertion_script(message, repair_raw)
        result = subprocess.run([node, "-e", self.DOM_FIXTURE], input=json.dumps(state),
                                capture_output=True, text=True, encoding="utf-8", timeout=5, check=True)
        return json.loads(result.stdout)

    def test_native_insert_handles_long_unicode_contenteditable_and_textarea_once(self):
        message = 'Long JSON {"path":"https://example.test/a", "note":"two  spaces"}\n' + '😀 café 漢字 ' * 700
        for textarea in (False, True):
            with self.subTest(textarea=textarea):
                result = self.run_script(message, textarea=textarea)
                self.assertEqual((result["result"], result["inserts"], result["command"]),
                                 ("INSERTED", 1, "insertText"))
                self.assertEqual(result["text"], bridge.keyboard_safe_message(message))

    def test_native_guard_rejects_changed_authority_without_inserting(self):
        cases = ({"url": ISSUED}, {"turns": True}, {"draft": "unknown"},
                 {"multiple": True}, {"hidden": True}, {"loading": True},
                 {"noSelection": True}, {"focus": "draft"}, {"focus": "url"},
                 {"focus": "turn"}, {"focus": "replace"}, {"focus": "elsewhere"},
                 {"selectionDrift": True}, {"selectionFocusShift": True})
        for state in cases:
            with self.subTest(state=state):
                result = self.run_script("expected handoff", **state)
                self.assertEqual((result["result"], result["inserts"]), ("UNPROVEN", 0))

    def test_native_failure_is_not_repeated_and_never_reports_success(self):
        for outcome in ("refused", "partial", "changed", "throw"):
            with self.subTest(outcome=outcome):
                result = self.run_script("a full long handoff " * 40, outcome=outcome)
                self.assertEqual(result["inserts"], 1)
                self.assertNotEqual(result["result"], "INSERTED")

    def test_uncertain_evaluation_is_one_attempt_and_stops_before_body_checks(self):
        for error in (None, TimeoutError("lost result")):
            with self.subTest(error=error), tempfile.TemporaryDirectory() as profile:
                sink = REAL_NEW_SINK(SOURCE, Path(profile), 1, 1)
                calls = []
                def evaluate(script):
                    calls.append(script)
                    if error is not None:
                        raise error
                    return "UNPROVEN"
                cdp = types.SimpleNamespace(evaluate=evaluate)
                with patch.object(sink, "check_empty_home"), \
                        patch.object(sink, "check_typed_home") as body, \
                        patch.object(sink, "check_plain_home_draft") as plain:
                    with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_DRAFT_INSERT_UNVERIFIED"):
                        sink.insert_empty_home_draft(cdp, "handoff")
                self.assertEqual(len(calls), 1)
                body.assert_not_called()
                plain.assert_not_called()

    def test_partial_repair_native_cas_replaces_once_without_clearing(self):
        expected = 'Prefix with  spacing and café 😀. ' + 'remaining JSON {"x":1} ' * 60
        partial = expected[:28]
        for textarea in (False, True):
            with self.subTest(textarea=textarea):
                result = self.run_script(expected, repair_raw=partial, draft=partial, textarea=textarea)
                self.assertEqual((result["result"], result["inserts"], result["command"]),
                                 ("INSERTED", 1, "insertText"))
                self.assertEqual(result["text"], expected)

    def test_partial_repair_native_cas_rejects_raw_focus_selection_and_scope_drift(self):
        expected = "Prefix with  spacing. Continue the exact task."
        partial = "Prefix with  spacing."
        cases = ({"draft": "Prefix with spacing."}, {"url": ISSUED}, {"turns": True},
                 {"multiple": True}, {"focus": "draft"}, {"focus": "elsewhere"},
                 {"selectionDrift": True}, {"selectionFocusShift": True}, {"partialSelection": True})
        for state in cases:
            with self.subTest(state=state):
                result = self.run_script(expected, repair_raw=partial, **dict({"draft": partial}, **state))
                self.assertEqual((result["result"], result["inserts"]), ("UNPROVEN", 0))

    def test_partial_repair_uncertain_native_result_is_not_repeated(self):
        expected = "Original prefix with remaining handoff"
        for outcome in ("refused", "partial", "changed", "throw"):
            with self.subTest(outcome=outcome):
                result = self.run_script(expected, repair_raw="Original prefix", draft="Original prefix",
                                         outcome=outcome)
                self.assertEqual(result["inserts"], 1)
                self.assertNotEqual(result["result"], "INSERTED")

    def test_native_insert_and_repair_preserve_same_text_rich_or_hidden_nodes(self):
        message, partial = "Known prefix and remaining handoff", "Known prefix"
        for repairing in (False, True):
            for kind in ("link", "anchor", "chip", "image", "hidden"):
                for moment in ("rich", "richAfterFocus", "richAfterSelection"):
                    with self.subTest(repairing=repairing, kind=kind, moment=moment):
                        draft = partial if repairing else ""
                        result = self.run_script(message, repair_raw=partial if repairing else None,
                                                 draft=draft, **{moment: kind})
                        self.assertEqual((result["result"], result["inserts"]), ("UNPROVEN", 0))
                        self.assertEqual(result["text"], draft)

    def test_native_plain_paragraph_and_trailing_break_remain_supported(self):
        message, partial = "Known prefix and remaining handoff", "Known prefix"
        for repairing in (False, True):
            with self.subTest(repairing=repairing):
                result = self.run_script(message, repair_raw=partial if repairing else None,
                                         draft=partial if repairing else "", plainParagraph=True)
                self.assertEqual((result["result"], result["inserts"], result["text"]),
                                 ("INSERTED", 1, message))


class NewConversationTests(unittest.TestCase):
    def request(self):
        return {
            "bootstrap": True,
            "projectId": "project-one",
            "intentId": "rollover-1",
            "source": {"url": SOURCE, "digest": bridge.digest(SOURCE), "generation": 7},
            "handoff": "Continue the bounded task with the preserved evidence.",
        }

    def run_attempt(self, scenario="success", *, ack=None, request=None, cdp_type=AttemptCdp):
        cdp = cdp_type(scenario)
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        acknowledgements = []

        def read_ack():
            acknowledgements.append(1)
            self.assertEqual(cdp.sent, 0)
            self.assertEqual(cdp.draft, "")
            self.assertNotIn("type", cdp.timeline)
            self.assertEqual(emitted[-1]["stage"], "BEFORE_BOOTSTRAP_SUBMIT")
            return {"submit": True} if ack is None else ack

        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "read_request", read_ack
        ), patch.object(adapter, "emit", emitted.append):
            try:
                adapter.bootstrap_attempt(bridge, sb, request or self.request())
                error = None
            except bridge.Attention as failure:
                error = str(failure)
        return cdp, emitted, acknowledgements, error

    def v2_request(self, *, resume_progress=None):
        request = self.request()
        request.update({"cooperative": True, "bootstrapProtocol": 2,
                        "resumeDraft": resume_progress is not None})
        if resume_progress is not None:
            request["resumeProgress"] = resume_progress
        return request

    def run_v2_attempt(self, *, request=None, draft="", users=0, deny_stage=None, rich_link=False,
                       cdp_type=AttemptCdp, on_ack=None):
        request = request or self.v2_request()
        cdp = cdp_type()
        cdp.draft = draft
        cdp.users = users
        cdp.rich_link = rich_link
        if draft:
            cdp.typed_message = draft
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        acknowledgements = []
        responses = {
            "TEXT_ENTRY_AUTHORIZED": {"textEntry": True},
            "DRAFT_READY": {"draftReady": True},
            "SEND_ATTEMPTED": {"submit": True},
        }

        def read_ack():
            stage = emitted[-1]["stage"]
            acknowledgements.append(stage)
            self.assertEqual(cdp.sent, 0, "a browser click cannot precede SEND_ATTEMPTED ACK")
            if stage == "TEXT_ENTRY_AUTHORIZED":
                self.assertNotIn("type", cdp.timeline)
                self.assertNotIn("insert", cdp.timeline)
            if stage in {"DRAFT_READY", "SEND_ATTEMPTED"}:
                self.assertEqual(
                    bridge.normalize_message(cdp.draft),
                    bridge.normalize_message(bridge.compose_bootstrap_message(
                        request["projectId"], request["intentId"], SOURCE, request["handoff"])),
                )
            if on_ack is not None:
                on_ack(stage, cdp)
            return {} if stage == deny_stage else responses[stage]

        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "read_request", read_ack
        ), patch.object(adapter, "emit", emitted.append):
            try:
                adapter.bootstrap_attempt(bridge, sb, request)
                error = None
            except bridge.Attention as failure:
                error = str(failure)
        return cdp, emitted, acknowledgements, error

    def test_deterministic_identity_prefix_and_bound(self):
        message = bridge.compose_bootstrap_message("project-one", "rollover-1", SOURCE, "body\nline")
        self.assertEqual(
            message,
            "CatDesk project rollover bootstrap.\nProject: project-one\n"
            "Intent: rollover-1\nPredecessor: https://chatgpt.com/c/source-123\n\nbody\nline",
        )
        self.assertEqual(
            bridge.digest(bridge.normalize_message(message)),
            bridge.digest(" ".join(message.split())),
        )
        with self.assertRaises(bridge.Attention):
            bridge.compose_bootstrap_message("project-one", "rollover-1", SOURCE, "é" * 9000)

    def test_json_bootstrap_envelope_round_trips_urls_and_matches_rust_digest(self):
        content = 'Keep https://example.test/a/b, "quotes", caf\u00e9, and \\path.\nNext.'
        message = bridge.compose_bootstrap_message(
            "project-one", "rollover-1", SOURCE, bridge.JSON_BOOTSTRAP_HANDOFF_PREFIX + content)
        self.assertTrue(message.startswith(bridge.JSON_BOOTSTRAP_MESSAGE_PREFIX))
        self.assertNotIn("/", message)
        envelope = json.loads(message[len(bridge.JSON_BOOTSTRAP_MESSAGE_PREFIX):])
        self.assertEqual(envelope, {"schemaVersion":1, "projectId":"project-one",
                                   "intentId":"rollover-1", "predecessor":SOURCE, "handoff":content})
        self.assertEqual(bridge.digest(bridge.normalize_message(message)),
                         "f83dd687c0a2470e5a583845926fd8e5c9ce6d858397c2d9e6a4b8feee698c0f")
        legacy = bridge.compose_bootstrap_message("project-one", "rollover-1", SOURCE, "legacy\nline")
        self.assertEqual(legacy, "CatDesk project rollover bootstrap.\nProject: project-one\n"
                                "Intent: rollover-1\nPredecessor: " + SOURCE + "\n\nlegacy\nline")
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_MESSAGE_TOO_LONG"):
            bridge.compose_bootstrap_message("project-one", "rollover-1", SOURCE,
                bridge.JSON_BOOTSTRAP_HANDOFF_PREFIX + "/" * 3000)

    def test_real_sink_rejects_wrong_home_draft_messages_and_login(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_HOME_DRIFT"):
                sink.check_empty_home(cdp)
            cdp.url = bridge.NEW_CONVERSATION_HOME
            cdp.draft = "operator draft"
            with self.assertRaisesRegex(bridge.Attention, "EXISTING_DRAFT"):
                sink.check_empty_home(cdp)
            cdp.draft = ""
            cdp.users = 1
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_HOME_HAS_TURNS"):
                sink.check_empty_home(cdp)
            cdp.users = 0
            cdp.visible_selectors.add(bridge.LOGIN[0])
            with self.assertRaisesRegex(bridge.Attention, "LOGIN_OR_PROFILE_REQUIRED"):
                sink.check_empty_home(cdp)

    def test_real_sink_classifies_exact_home_draft_without_exposing_text(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp()
            expected = bridge.compose_bootstrap_message(
                "project-one", "rollover-1", SOURCE, "Keep the exact handoff.")
            self.assertEqual(sink.observe_home_draft(cdp, expected)["classification"], "EMPTY_HOME")
            cdp.draft = bridge.keyboard_safe_message(expected)
            exact = sink.observe_home_draft(cdp, expected)
            self.assertEqual(exact["classification"], "EXACT_OWNED")
            self.assertEqual(exact["actualDigest"], bridge.digest(bridge.normalize_message(expected)))
            self.assertNotIn("text", exact)
            cdp.draft = "Project: someone-else"
            self.assertEqual(sink.observe_home_draft(cdp, expected)["classification"], "MISMATCH")
            cdp.users = 1
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_HOME_HAS_TURNS"):
                sink.observe_home_draft(cdp, expected)
            cdp.users = 0
            cdp.visible_selectors.add(bridge.LOGIN[0])
            with self.assertRaisesRegex(bridge.Attention, "LOGIN_OR_PROFILE_REQUIRED"):
                sink.observe_home_draft(cdp, expected)

    def test_real_sink_archived_clear_reloads_home_without_submit(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            expected = bridge.compose_bootstrap_message(
                "project-one", "rollover-1", SOURCE, "Preserve exact journal text.")

            class ClearCdp(StructuralCdp):
                def evaluate(self, script):
                    if "document.execCommand('delete'" in script:
                        self.assert_safe_script = script
                        if self.draft != getattr(self, "expected_raw", expected):
                            return "UNAVAILABLE"
                        self.draft = ""
                        return "CLEARED"
                    return super().evaluate(script)

            cdp = ClearCdp()
            cdp.draft = "  " + expected + "  "
            self.assertEqual(sink.raw_editor_text(cdp), "  " + expected + "  ")
            cdp.draft = expected
            self.assertEqual(sink.raw_editor_text(cdp), expected)
            with patch.object(sink, "wait_home_draft", side_effect=lambda cdp, message:
                              sink.observe_home_draft(cdp, message)):
                sink.clear_archived_home_draft(cdp, expected, expected)
            self.assertEqual(cdp.draft, "")
            self.assertEqual(cdp.navigation, [bridge.NEW_CONVERSATION_HOME])
            self.assertNotIn("send", cdp.assert_safe_script.lower())
            self.assertIn("location.href !==", cdp.assert_safe_script)
            self.assertIn("current !==", cdp.assert_safe_script)
            self.assertIn(json.dumps(expected), cdp.assert_safe_script)

            cdp.draft = expected
            original_check = sink.check_typed_home
            def drift_after_python_check(cdp, message):
                original_check(cdp, message)
                cdp.draft = "foreign late edit"
            with patch.object(sink, "check_typed_home", side_effect=drift_after_python_check):
                with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED"):
                    sink.clear_archived_home_draft(cdp, expected, expected)
            self.assertEqual(cdp.draft, "foreign late edit")

            logical = '{"url":"https://example.test/predecessor"}'
            raw = logical.replace('"https:', '"\nhttps:')
            cdp = ClearCdp()
            cdp.draft, cdp.plain_draft, cdp.expected_raw = raw, logical, raw
            with patch.object(sink, "wait_home_draft", return_value={"classification":"EMPTY_HOME"}):
                sink.clear_archived_home_draft(cdp, logical, raw)
            self.assertEqual(cdp.draft, "")
            self.assertIn(json.dumps(raw), cdp.assert_safe_script)

    def test_real_sink_latches_only_new_canonical_identity(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_SOURCE_REVISITED"):
                sink.latch_issued_url(cdp)
            cdp.url = ISSUED
            self.assertEqual(sink.latch_issued_url(cdp), ISSUED)
            cdp.url = "https://chatgpt.com/c/other-789"
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_DESTINATION_DRIFT"):
                sink.latch_issued_url(cdp)

    def test_provisional_route_waits_without_latching_or_submitting(self):
        provisional = "https://chatgpt.com/c/local-chatgpt%3A" + "-".join(
            ["a" * 8, "b" * 4, "c" * 4, "d" * 4, "e" * 12])
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(provisional)
            ticks = [0.0]
            def sleep(seconds):
                self.assertIsNone(sink.destination_url)
                self.assertEqual(sink.url, SOURCE)
                ticks[0] += seconds
                if ticks[0] >= 1:
                    cdp.url = ISSUED
            self.assertEqual(sink.latch_issued_url(
                cdp, monotonic=lambda: ticks[0], sleeper=sleep), ISSUED)
            self.assertEqual(cdp.navigation, [])
            self.assertEqual(sink.destination_url, ISSUED)
            cdp.url = provisional
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_URL_INVALID"):
                sink.latch_issued_url(cdp)

    def test_provisional_route_timeout_never_grants_destination(self):
        provisional = "https://chatgpt.com/c/local-chatgpt%3A" + "-".join(
            ["a" * 8, "b" * 4, "c" * 4, "d" * 4, "e" * 12])
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(provisional)
            ticks = [0.0]
            def sleep(seconds):
                ticks[0] += seconds
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_URL_UNISSUED"):
                sink.latch_issued_url(cdp, monotonic=lambda: ticks[0], sleeper=sleep)
            self.assertIsNone(sink.destination_url)
            self.assertEqual(sink.url, SOURCE)
            self.assertEqual(cdp.navigation, [])
            self.assertLessEqual(ticks[0], max(1, bridge.POST_SUBMIT_RECEIPT_WINDOW_SECONDS))
            with self.assertRaises(bridge.Attention):
                bridge.canonical_conversation_url(provisional)

    def test_provisional_lookalikes_still_refuse_immediately(self):
        provisional = "https://chatgpt.com/c/local-chatgpt%3A" + "-".join(
            ["a" * 8, "b" * 4, "c" * 4, "d" * 4, "e" * 12])
        for invalid in (provisional + "?q=x", provisional + "#x",
                        provisional.replace("chatgpt.com", "example.invalid"),
                        provisional.replace("https://", "http://"),
                        provisional.replace("chatgpt.com/", "chatgpt.com:443/"),
                        provisional.replace("%3A", "%3a")):
            with self.subTest(invalid=invalid), tempfile.TemporaryDirectory() as profile:
                sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
                with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_URL_INVALID"):
                    sink.latch_issued_url(StructuralCdp(invalid), sleeper=lambda _s: self.fail("must refuse immediately"))
                self.assertIsNone(sink.destination_url)

    def test_load_error_placeholder_is_neither_usable_source_nor_fresh_answer(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = observed_rows(("user", "handoff", None, None, None),
                                         ("assistant", "This response couldn’t load", None, None, None))
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER"):
                sink.require_usable_latest_assistant(cdp, source=True)
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER"):
                sink.require_usable_latest_assistant(cdp, source=False)
            cdp.snapshot = observed_rows(("user", "handoff", None, None, None),
                                         ("assistant", "Usable code and next step.", None, None, None))
            sink.require_usable_latest_assistant(cdp, source=True)
            sink.require_usable_latest_assistant(cdp, source=False)
            cdp.snapshot = observed_rows(("user", "handoff", None, None, None))
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_ASSISTANT_UNAVAILABLE"):
                sink.require_usable_latest_assistant(cdp, source=True)

    def test_actual_source_and_fresh_receipt_gates_reject_placeholder(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            message = "Exact bootstrap handoff"
            cdp.snapshot = observed_rows(("user", message, None, None, None),
                                         ("assistant", "This response couldn’t load", None, None, None))
            with patch.object(bridge.CdpSink, "pre_typing_ready", return_value=None):
                with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER"):
                    sink.pre_typing_ready(cdp)
            with patch.object(sink, "user_message_digests", return_value=[bridge.digest(bridge.normalize_message(message))]), patch.object(
                sink, "visible_role_counts", return_value=(1, 1, 1)
            ):
                with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER"):
                    sink.prove_single_bootstrap_user(cdp, message)

    def test_response_snapshot_rejects_load_error_even_if_copy_action_exists(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = types.SimpleNamespace(evaluate=lambda script: ["load-error", 0, 0, True, False])
            with self.assertRaisesRegex(bridge.PostSubmitUnknown, "RESPONSE_COMPLETION_UNPROVEN"):
                sink.response_snapshot(cdp, "expected user turn")

    def test_private_source_snapshot_bounds_text_without_browser_write(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = observed_rows(("user", "old", None, None, None),
                                         ("assistant", "😀" * 4096, None, None, None))
            snapshot = sink.visible_message_snapshot(cdp)
            self.assertTrue(snapshot["truncated"])
            self.assertEqual((snapshot["messages"][0]["role"], snapshot["messages"][0]["text"]),
                             ("USER", "old"))
            self.assertEqual(len(snapshot["messages"][1]["text"].encode("utf-8")), 4096)
            self.assertLessEqual(
                sum(len(item["text"].encode("utf-8")) for item in snapshot["messages"]),
                8192,
            )

    def test_snapshot_identity_stays_stable_while_complete_body_revision_changes(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            raw_turn = "private-turn-key"
            raw_unit = "private-unit-key"
            cdp.snapshot = observed_rows(("assistant", "Working", raw_turn, raw_unit, None))
            first = sink.visible_message_snapshot(cdp)
            cdp.snapshot = observed_rows(("assistant", "Working step two", raw_turn, raw_unit, None))
            second = sink.visible_message_snapshot(cdp)
            prior, current = first["messages"][0], second["messages"][0]
            for field in ("turnHash", "roleTurnHash", "messageHash"):
                self.assertEqual(prior[field], current[field])
                self.assertEqual(len(prior[field]), 64)
            self.assertNotEqual(prior["revisionHash"], current["revisionHash"])
            self.assertEqual(current["identityBasis"], "UNIT_KEY")
            self.assertEqual(second["coverage"]["anchorStability"], "UNVERIFIED")
            self.assertNotIn(raw_turn, json.dumps(second))
            self.assertNotIn(raw_unit, json.dumps(second))
            other_source = bridge.NewConversationSink(ISSUED, Path(profile), 1, 1)
            self.assertNotEqual(other_source.visible_message_snapshot(cdp)["messages"][0]["messageHash"],
                                current["messageHash"])

    def test_snapshot_reports_uncertain_or_missing_identity_and_incomplete_revision(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = observed_rows(
                ("user", "one", "turn-only-key", None, None),
                ("assistant", "tail" * 1024, None, None, None),
                nested=2, keyed=1, count=20,
            )
            cdp.snapshot[5][1][2] = False
            cdp.snapshot[5][1][3] = 9000
            result = sink.visible_message_snapshot(cdp)
            uncertain, missing = result["messages"]
            self.assertEqual(uncertain["identityBasis"], "ROLE_TURN_ONLY")
            self.assertIsNotNone(uncertain["roleTurnHash"])
            self.assertIsNone(uncertain["messageHash"])
            self.assertEqual(missing["identityBasis"], "MISSING")
            self.assertIsNone(missing["turnHash"])
            self.assertIsNone(missing["revisionHash"])
            self.assertFalse(missing["revisionComplete"])
            self.assertEqual(missing["visibleUnits"], 9000)
            self.assertTrue(result["truncated"])
            self.assertEqual(result["coverage"]["omittedBeforeWindow"], 18)
            self.assertEqual(result["coverage"]["nestedCollapsed"], 2)
            self.assertEqual(result["coverage"]["keyedCollapsed"], 1)
            self.assertEqual(result["coverage"]["identityMissing"], 1)
            self.assertEqual(result["coverage"]["identityUncertain"], 1)
            self.assertEqual(result["coverage"]["revisionIncomplete"], 1)

    def test_snapshot_message_id_can_identify_unit_without_turn_key(self):
        with tempfile.TemporaryDirectory() as profile:
            sink = bridge.NewConversationSink(SOURCE, Path(profile), 1, 1)
            cdp = StructuralCdp(SOURCE)
            cdp.snapshot = observed_rows(("assistant", "answer", None, None, "private-message-id"))
            item = sink.visible_message_snapshot(cdp)["messages"][0]
            self.assertEqual(item["identityBasis"], "MESSAGE_ID")
            self.assertIsNotNone(item["messageHash"])
            self.assertIsNone(item["turnHash"])

    def test_explicit_snapshot_mode_never_types_or_submits(self):
        cdp = AttemptCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        request = {"snapshot": True, "projectId": "project-one", "source": self.request()["source"]}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ):
            adapter.source_snapshot(bridge, sb, request)
        self.assertEqual((cdp.sent, cdp.draft, cdp.navigation), (0, "", [SOURCE]))
        self.assertEqual(cdp.timeline, ["snapshot", "snapshot"])
        self.assertEqual(emitted[-1]["stage"], "SOURCE_SNAPSHOT")
        self.assertEqual(emitted[-1]["sourceGeneration"], 7)

    def test_snapshot_wire_frame_stays_within_parent_reader_bound(self):
        cdp = AttemptCdp("large_snapshot")
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        request = {"snapshot": True, "projectId": "project-one", "source": self.request()["source"]}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ):
            adapter.source_snapshot(bridge, sb, request)
        self.assertEqual(cdp.sent, 0)
        self.assertTrue(emitted[-1]["snapshot"]["truncated"])
        self.assertTrue(emitted[-1]["snapshot"]["messages"][0]["text"])
        self.assertFalse(emitted[-1]["snapshot"]["messages"][0]["textComplete"])
        self.assertEqual(emitted[-1]["snapshot"]["coverage"]["textIncomplete"], 1)
        self.assertLessEqual(
            len(json.dumps(emitted[-1], separators=(",", ":")).encode("utf-8")) + 1,
            8192,
        )

    def test_incomplete_snapshot_adds_private_shape_without_promoting_text(self):
        cdp = AttemptCdp("incomplete_body")
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        request = {"snapshot": True, "projectId": "project-one", "source": self.request()["source"]}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(adapter, "emit", emitted.append), patch.object(
            adapter.time, "monotonic", side_effect=range(100)
        ), patch.object(adapter.time, "sleep"):
            adapter.source_snapshot(bridge, sb, request)
        snapshot = emitted[-1]["snapshot"]
        self.assertEqual(snapshot["bodyDiagnostics"], {"status":"UNAVAILABLE"})
        self.assertFalse(snapshot["messages"][-1]["textComplete"])
        self.assertGreater(cdp.timeline.count("snapshot"), 1)
        self.assertEqual(cdp.timeline[-1], "body-diagnostics")
        self.assertEqual(cdp.sent, 0)

    def test_snapshot_frame_trim_reports_omitted_older_units(self):
        cdp = AttemptCdp("many_snapshot")
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        request = {"snapshot": True, "projectId": "project-one", "source": self.request()["source"]}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ):
            adapter.source_snapshot(bridge, sb, request)
        snapshot = emitted[-1]["snapshot"]
        self.assertTrue(snapshot["truncated"])
        self.assertLess(len(snapshot["messages"]), 12)
        self.assertEqual(snapshot["coverage"]["returned"], len(snapshot["messages"]))
        self.assertEqual(snapshot["coverage"]["omittedBeforeWindow"], 12 - len(snapshot["messages"]))
        self.assertEqual(snapshot["coverage"]["identityMissing"], 0)
        self.assertLessEqual(len(json.dumps(emitted[-1], separators=(",", ":")).encode("utf-8")) + 1, 8192)

    def test_wrong_source_fails_before_browser_navigation(self):
        request = self.request()
        request["source"]["digest"] = "0" * 64
        cdp, emitted, acknowledgements, error = self.run_attempt(request=request)
        self.assertEqual(error, "BOOTSTRAP_SOURCE_INVALID")
        self.assertEqual((cdp.navigation, cdp.sent, emitted, acknowledgements), ([], 0, [], []))

    def test_home_draft_and_messages_never_request_submit(self):
        for scenario, reason in (
            ("wrong_home", "BOOTSTRAP_HOME_DRIFT"),
            ("existing_draft", "EXISTING_DRAFT"),
            ("existing_messages", "BOOTSTRAP_HOME_HAS_TURNS"),
        ):
            with self.subTest(scenario=scenario):
                cdp, emitted, acknowledgements, error = self.run_attempt(scenario)
                self.assertEqual(error, reason)
                self.assertEqual(cdp.sent, 0)
                self.assertEqual(acknowledgements, [])
                self.assertNotIn("BEFORE_BOOTSTRAP_SUBMIT", [item["stage"] for item in emitted])

    def test_source_load_error_never_navigates_to_new_chat(self):
        cdp, emitted, acknowledgements, error = self.run_attempt("source_load_error")
        self.assertEqual(error, "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER")
        self.assertEqual((cdp.navigation, cdp.sent, acknowledgements), ([SOURCE], 0, []))
        self.assertNotIn("BOOTSTRAP_HOME_READY", [item["stage"] for item in emitted])

    def test_parent_denial_never_submits(self):
        cdp, emitted, acknowledgements, error = self.run_attempt(ack={"submit": False})
        self.assertEqual(error, "SUBMISSION_NOT_AUTHORIZED")
        self.assertEqual((cdp.sent, len(acknowledgements)), (0, 1))
        self.assertEqual(cdp.timeline, [])
        self.assertEqual([x["stage"] for x in emitted].count("BEFORE_BOOTSTRAP_SUBMIT"), 1)

    def test_v2_persists_three_checkpoints_before_text_or_click(self):
        cdp, emitted, acks, error = self.run_v2_attempt()
        self.assertIsNone(error)
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"])
        self.assertEqual((cdp.timeline.count("insert"), cdp.timeline.count("submit")), (1, 1))
        self.assertNotIn("type", cdp.timeline)
        self.assertEqual(cdp.sent, 1)
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_SUBMITTED")
        for stage in acks:
            frame = next(item for item in emitted if item["stage"] == stage)
            self.assertEqual(set(frame), {"stage", "projectId", "intentId",
                                          "sourceGeneration", "messageDigest"})
            self.assertEqual(frame["messageDigest"], emitted[-1]["receipt"]["messageDigest"])

    def test_v2_denials_never_click_before_final_submit_ack(self):
        for denied, expected_types in (("TEXT_ENTRY_AUTHORIZED", 0), ("DRAFT_READY", 1),
                                       ("SEND_ATTEMPTED", 1)):
            with self.subTest(denied=denied):
                cdp, emitted, acks, error = self.run_v2_attempt(deny_stage=denied)
                self.assertIsNotNone(error) if denied == "TEXT_ENTRY_AUTHORIZED" else self.assertIsNone(error)
                self.assertEqual(cdp.timeline.count("insert"), expected_types)
                self.assertNotIn("type", cdp.timeline)
                self.assertEqual(cdp.sent, 0)
                self.assertEqual(acks[-1], denied)
                self.assertNotIn("BOOTSTRAP_SUBMITTED", [item["stage"] for item in emitted])

    def test_v2_pre_send_resume_reuses_exact_draft_or_safe_empty_home(self):
        request = self.v2_request(resume_progress="TEXT_ENTRY_AUTHORIZED")
        expected = bridge.compose_bootstrap_message(
            request["projectId"], request["intentId"], SOURCE, request["handoff"])
        full = bridge.keyboard_safe_message(expected)
        cdp, emitted, acks, error = self.run_v2_attempt(request=request, draft=full)
        self.assertIsNone(error)
        self.assertEqual(cdp.navigation, [bridge.NEW_CONVERSATION_HOME])
        self.assertEqual((cdp.timeline.count("type"), cdp.sent), (0, 1))
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"])
        cdp, emitted, acks, error = self.run_v2_attempt(request=request)
        self.assertEqual(error, "BOOTSTRAP_DRAFT_MISSING")
        self.assertEqual((cdp.timeline.count("type"), cdp.sent, acks), (0, 0, []))
        ready = self.v2_request(resume_progress="DRAFT_READY")
        cdp, emitted, acks, error = self.run_v2_attempt(request=ready, draft=full)
        self.assertIsNone(error)
        self.assertEqual((cdp.timeline.count("type"), cdp.sent), (0, 1))
        for draft, users, expected_error in (("", 0, "BOOTSTRAP_DRAFT_MISSING"),
                                             ("partial handoff", 0, "BOOTSTRAP_DRAFT_MISMATCH"),
                                             (full, 1, "BOOTSTRAP_HOME_HAS_TURNS")):
            with self.subTest(draft=draft[:12], users=users):
                cdp, emitted, acks, error = self.run_v2_attempt(
                    request=ready, draft=draft, users=users)
                self.assertEqual(error, expected_error)
                self.assertEqual((cdp.sent, acks), (0, []))
        cdp, emitted, acks, error = self.run_v2_attempt(
            request=request, draft="partial handoff")
        self.assertEqual(error, "BOOTSTRAP_DRAFT_MISMATCH")
        self.assertEqual((cdp.sent, acks), (0, []))

    def test_ready_draft_survives_send_control_failure_and_resumes_without_retyping(self):
        with patch.object(AttemptSink, "wait_for_send_control", return_value=None):
            cdp, emitted, acks, error = self.run_v2_attempt()
        self.assertIsNone(error)
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY"])
        self.assertEqual(cdp.sent, 0)
        self.assertEqual(emitted[-1], {"stage":"BOOTSTRAP_UNKNOWN",
                                      "reason":"BOOTSTRAP_SEND_CONTROL_MISSING"})
        saved_draft = cdp.draft
        request = self.v2_request(resume_progress="DRAFT_READY")
        resumed, emitted, acks, error = self.run_v2_attempt(request=request, draft=saved_draft)
        self.assertIsNone(error)
        self.assertEqual((resumed.timeline.count("type"), resumed.sent), (0, 1))
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_SUBMITTED")

    def test_unverified_rich_draft_normalization_never_receives_send_authorization(self):
        request = self.v2_request(resume_progress="DRAFT_READY")
        expected = bridge.compose_bootstrap_message(
            request["projectId"], request["intentId"], SOURCE, request["handoff"])
        with patch.object(AttemptSink, "normalize_owned_home_draft", side_effect=
                          bridge.Attention("BOOTSTRAP_DRAFT_NORMALIZATION_UNVERIFIED")):
            cdp, emitted, acks, error = self.run_v2_attempt(request=request, draft=expected)
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, cdp.timeline, acks), (0, [], ["TEXT_ENTRY_AUTHORIZED"]))
        self.assertEqual(cdp.draft, expected)
        self.assertEqual(emitted[-1], {"stage": "BOOTSTRAP_UNKNOWN",
                                      "reason": "BOOTSTRAP_DRAFT_NORMALIZATION_UNVERIFIED"})
        self.assertNotIn("BOOTSTRAP_SUBMITTED", [frame["stage"] for frame in emitted])

    def test_exact_restored_rich_draft_unlinks_after_ack_without_retyping(self):
        request = self.v2_request(resume_progress="DRAFT_READY")
        expected = bridge.compose_bootstrap_message(
            request["projectId"], request["intentId"], SOURCE, request["handoff"])
        cdp, emitted, acks, error = self.run_v2_attempt(
            request=request, draft=expected, rich_link=True)
        self.assertIsNone(error)
        self.assertEqual(cdp.timeline, ["unlink", "submit", "accepted", "fresh-document", "exact-receipt"])
        self.assertEqual(cdp.sent, 1)
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"])
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_SUBMITTED")
        cdp, emitted, acks, error = self.run_v2_attempt(
            request=request, draft="foreign operator draft", rich_link=True)
        self.assertEqual(error, "BOOTSTRAP_DRAFT_MISMATCH")
        self.assertEqual((cdp.sent, cdp.timeline, acks), (0, [], []))
        self.assertEqual(cdp.draft, "foreign operator draft")

    def test_rich_link_appearing_at_later_guards_preserves_draft_without_click(self):
        for failed_check, expected_acks in (
                (1, ["TEXT_ENTRY_AUTHORIZED"]),
                (2, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY"]),
                (3, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"])):
            calls = []
            def check(_self, cdp):
                calls.append(True)
                if len(calls) == failed_check:
                    raise bridge.Attention("BOOTSTRAP_RICH_DRAFT_UNVERIFIED")
            with self.subTest(failed_check=failed_check), patch.object(
                    AttemptSink, "check_plain_home_draft", check):
                cdp, emitted, acks, error = self.run_v2_attempt()
                self.assertIsNone(error)
                self.assertEqual(cdp.sent, 0)
                self.assertEqual(cdp.timeline, ["insert"])
                self.assertEqual(acks, expected_acks)
                self.assertTrue(cdp.draft)
                self.assertEqual(emitted[-1], {"stage": "BOOTSTRAP_UNKNOWN",
                                              "reason": "BOOTSTRAP_RICH_DRAFT_UNVERIFIED"})

    def test_large_inspector_request_uses_bootstrap_bound_without_exposing_text(self):
        request = {"bootstrapDraftInspect": True, "bootstrapProtocol": 2,
                   "projectId": "project-one", "intentId": "rollover-1",
                   "source": self.request()["source"], "handoff": "x" * 9000}
        wire = (json.dumps(request, separators=(",", ":")) + "\n").encode()
        self.assertGreater(len(wire), 8192)
        self.assertLess(len(wire), adapter.MAX_BOOTSTRAP_REQUEST_BYTES)
        with patch.object(adapter.sys, "stdin", types.SimpleNamespace(buffer=io.BytesIO(wire))):
            self.assertEqual(adapter.read_job_request(), request)
        wire = (json.dumps({"unrelated": "x" * 9000}) + "\n").encode()
        with patch.object(adapter.sys, "stdin", types.SimpleNamespace(buffer=io.BytesIO(wire))):
            with self.assertRaisesRegex(ValueError, "invalid job request"):
                adapter.read_job_request()

    def test_v2_rejects_malformed_protocol_and_resume_stage_before_navigation(self):
        for mutation in ({"bootstrapProtocol": 2.0}, {"resumeDraft": "yes"},
                         {"resumeDraft": True},
                         {"resumeDraft": True, "resumeProgress": "SEND_ATTEMPTED"}):
            with self.subTest(mutation=mutation):
                request = self.v2_request()
                request.update(mutation)
                cdp, emitted, acks, error = self.run_v2_attempt(request=request)
                self.assertEqual(error, "BOOTSTRAP_REQUEST_INVALID")
                self.assertEqual((cdp.navigation, cdp.sent, emitted, acks), ([], 0, [], []))

    def test_v2_ambiguous_text_entry_never_emits_send_authorization(self):
        class AmbiguousCdp(AttemptCdp):
            def insert_text(self, text):
                self.timeline.append("insert")
                self.url = ISSUED
                raise RuntimeError("ambiguous browser input")

        request = self.v2_request()
        cdp = AmbiguousCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        acks = []
        def read_ack():
            acks.append(emitted[-1]["stage"])
            return {"textEntry": True}
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ), patch.object(adapter, "read_request", read_ack):
            adapter.bootstrap_attempt(bridge, sb, request)
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED"])
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_UNKNOWN")
        self.assertEqual(emitted[-1]["reason"], "BOOTSTRAP_PRE_SEND_RUNTIMEERROR")
        self.assertNotIn("SEND_ATTEMPTED", [item["stage"] for item in emitted])

    def test_v2_long_unicode_entry_uses_one_insert_and_no_keyboard_input(self):
        request = self.v2_request()
        request["handoff"] = bridge.JSON_BOOTSTRAP_HANDOFF_PREFIX + (
            'Keep {"note":"two  spaces"}; café 😀 漢字 / path.\n' * 100)
        cdp, emitted, acks, error = self.run_v2_attempt(request=request)
        self.assertIsNone(error)
        self.assertEqual((cdp.timeline.count("insert"), cdp.timeline.count("type"), cdp.sent), (1, 0, 1))
        expected = bridge.compose_bootstrap_message(
            request["projectId"], request["intentId"], SOURCE, request["handoff"])
        self.assertGreater(len(expected.encode("utf-8")), 4000)
        self.assertEqual(cdp.typed_message, bridge.keyboard_safe_message(expected))
        self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"])
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_SUBMITTED")

    def test_v2_authority_change_after_ack_prevents_insertion(self):
        for change in (lambda cdp: setattr(cdp, "url", ISSUED),
                       lambda cdp: setattr(cdp, "draft", "operator text"),
                       lambda cdp: setattr(cdp, "users", 1)):
            with self.subTest(change=change):
                def on_ack(stage, cdp):
                    if stage == "TEXT_ENTRY_AUTHORIZED":
                        change(cdp)
                cdp, emitted, acks, error = self.run_v2_attempt(on_ack=on_ack)
                self.assertIsNone(error)
                self.assertEqual((cdp.timeline, cdp.sent), ([], 0))
                self.assertEqual(acks, ["TEXT_ENTRY_AUTHORIZED"])
                self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_UNKNOWN")

    def test_v2_interrupted_full_insert_recovers_exact_draft_without_reinsertion(self):
        class LostInsertAck(AttemptCdp):
            def insert_text(self, text):
                super().insert_text(text)
                raise TimeoutError("native insert completed but response was lost")
        cdp, emitted, acks, error = self.run_v2_attempt(cdp_type=LostInsertAck)
        self.assertIsNone(error)
        self.assertEqual((cdp.timeline, cdp.sent, acks), (["insert"], 0, ["TEXT_ENTRY_AUTHORIZED"]))
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_UNKNOWN")
        recovered, emitted, acks, error = self.run_v2_attempt(
            request=self.v2_request(resume_progress="TEXT_ENTRY_AUTHORIZED"), draft=cdp.draft)
        self.assertIsNone(error)
        self.assertNotIn("insert", recovered.timeline)
        self.assertNotIn("type", recovered.timeline)
        self.assertEqual(recovered.sent, 1)

    def test_v2_partial_insert_is_preserved_and_not_repaired_by_resume(self):
        class PartialInsert(AttemptCdp):
            def insert_text(self, text):
                super().insert_text(text[:len(text) // 2])
                raise TimeoutError("partial native editor result")
        cdp, emitted, acks, error = self.run_v2_attempt(cdp_type=PartialInsert)
        self.assertIsNone(error)
        self.assertEqual((cdp.timeline, cdp.sent, acks), (["insert"], 0, ["TEXT_ENTRY_AUTHORIZED"]))
        self.assertNotIn("DRAFT_READY", [item["stage"] for item in emitted])
        recovered, emitted, acks, error = self.run_v2_attempt(
            request=self.v2_request(resume_progress="TEXT_ENTRY_AUTHORIZED"), draft=cdp.draft)
        self.assertEqual(error, "BOOTSTRAP_DRAFT_MISMATCH")
        self.assertEqual((recovered.draft, recovered.timeline, recovered.sent, acks), (cdp.draft, [], 0, []))

    def test_v2_draft_inspector_is_observe_only_and_bounded(self):
        request = self.v2_request()
        inspect = {"bootstrapDraftInspect": True, "bootstrapProtocol": 2,
                   "projectId": request["projectId"], "intentId": request["intentId"],
                   "source": request["source"], "handoff": request["handoff"]}
        expected = bridge.compose_bootstrap_message(
            inspect["projectId"], inspect["intentId"], SOURCE, inspect["handoff"])
        for draft, users, classification in (("", 0, "EMPTY_HOME"),
                                             (bridge.keyboard_safe_message(expected), 0, "EXACT_OWNED"),
                                             ("unrelated private draft", 0, "MISMATCH"),
                                             ("", 1, "UNAVAILABLE")):
            with self.subTest(classification=classification):
                cdp = AttemptCdp()
                cdp.draft = draft
                cdp.users = users
                sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
                emitted = []
                with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
                    adapter, "emit", emitted.append
                ):
                    adapter.bootstrap_draft_inspect(bridge, sb, inspect)
                frame = emitted[-1]
                self.assertEqual(frame["stage"], "BOOTSTRAP_DRAFT_INSPECTED")
                self.assertEqual(frame["classification"], classification)
                self.assertEqual(frame["sourceDigest"], bridge.digest(SOURCE))
                self.assertEqual(frame["expectedDigest"], bridge.digest(bridge.normalize_message(expected)))
                self.assertLessEqual(frame["actualBytes"], bridge.MAX_USER_MESSAGE_CHARS)
                self.assertEqual((cdp.timeline, cdp.sent, cdp.navigation),
                                 ([], 0, [bridge.NEW_CONVERSATION_HOME]))
                self.assertNotIn("unrelated private draft", json.dumps(frame))

    def repair_request(self, handoff=None):
        source = self.request()
        if handoff is not None:
            source["handoff"] = handoff
        request = {"bootstrapDraftRepair": True, "bootstrapProtocol": 2,
                   "projectId": source["projectId"], "intentId": source["intentId"],
                   "source": source["source"], "handoff": source["handoff"],
                   "expectedRawDigest": "0" * 64, "attemptRecorded": False}
        expected = bridge.compose_bootstrap_message(
            request["projectId"], request["intentId"], SOURCE, request["handoff"])
        return request, expected

    def run_repair(self, request, draft, *, acknowledgement=None, after_ready=None,
                   cdp_type=AttemptCdp, users=0, rich_link=False):
        cdp = cdp_type()
        cdp.draft, cdp.users, cdp.rich_link = draft, users, rich_link
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted, acknowledgements = [], []
        def read_ack():
            acknowledgements.append(True)
            self.assertEqual(cdp.timeline, [], "repair must follow durable witness ACK")
            self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_DRAFT_REPAIR_READY")
            if after_ready is not None:
                after_ready(cdp)
            return {"repairDraft": True} if acknowledgement is None else acknowledgement
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
                adapter, "emit", emitted.append), patch.object(adapter, "read_request", read_ack):
            try:
                adapter.bootstrap_draft_repair(bridge, sb, request)
                error = None
            except bridge.Attention as failure:
                error = str(failure)
        return cdp, emitted, len(acknowledgements), error

    def test_partial_repair_protocol_archives_exact_raw_before_one_no_send_replacement(self):
        request, expected = self.repair_request()
        partial = bridge.keyboard_safe_message(expected)[:-25]
        request["expectedRawDigest"] = bridge.digest(partial)
        cdp, frames, acknowledgements, error = self.run_repair(request, partial)
        self.assertIsNone(error)
        self.assertEqual((cdp.timeline, cdp.sent, acknowledgements), (["insert"], 0, 1))
        self.assertEqual(cdp.draft, bridge.keyboard_safe_message(expected))
        ready, done = [frame for frame in frames if frame["stage"].startswith("BOOTSTRAP_DRAFT_")]
        self.assertEqual(set(ready), {"stage", "projectId", "intentId", "sourceGeneration",
                                     "sourceDigest", "messageDigest", "actualBytes", "actualText", "actualRawDigest"})
        self.assertEqual((ready["actualText"], ready["actualBytes"], ready["actualRawDigest"]),
                         (partial, len(partial.encode("utf-8")), request["expectedRawDigest"]))
        self.assertEqual(set(done), {"stage", "projectId", "intentId", "sourceGeneration",
                                    "sourceDigest", "messageDigest", "reconciled"})
        self.assertFalse(done["reconciled"])
        self.assertEqual(done["messageDigest"], bridge.digest(bridge.normalize_message(expected)))
        self.assertEqual(cdp.navigation, [bridge.NEW_CONVERSATION_HOME])

    def test_partial_repair_requires_exact_typed_ack_and_raw_cas(self):
        request, expected = self.repair_request()
        partial = bridge.keyboard_safe_message(expected)[:-25]
        request["expectedRawDigest"] = bridge.digest(partial)
        for ack in ({}, {"repairDraft": False}, {"repairDraft": 1}, {"repairDraft": True, "extra": 1}):
            with self.subTest(ack=ack):
                cdp, frames, count, error = self.run_repair(request, partial, acknowledgement=ack)
                self.assertEqual(error, "BOOTSTRAP_DRAFT_REPAIR_NOT_AUTHORIZED")
                self.assertEqual((cdp.draft, cdp.timeline, cdp.sent, count), (partial, [], 0, 1))
        # Same normalized text is insufficient: the raw witness is the CAS key.
        changed = partial.replace("CatDesk project", "CatDesk  project", 1)
        cdp, frames, count, error = self.run_repair(
            request, partial, after_ready=lambda cdp: setattr(cdp, "draft", changed))
        self.assertEqual(error, "BOOTSTRAP_DRAFT_REPAIR_UNVERIFIED")
        self.assertEqual((cdp.draft, cdp.timeline, cdp.sent), (changed, [], 0))

    def test_partial_repair_refuses_wrong_digest_foreign_full_empty_and_existing_turn(self):
        request, expected = self.repair_request()
        partial = bridge.keyboard_safe_message(expected)[:-25]
        for draft, users, digest, reason in (
                (partial, 0, "a" * 64, "BOOTSTRAP_DRAFT_REPAIR_CAS_CHANGED"),
                ("foreign draft", 0, bridge.digest("foreign draft"), "BOOTSTRAP_DRAFT_REPAIR_PREFIX_INVALID"),
                (expected, 0, bridge.digest(expected), "BOOTSTRAP_DRAFT_REPAIR_PREFIX_INVALID"),
                ("", 0, bridge.digest(""), "BOOTSTRAP_DRAFT_REPAIR_PREFIX_INVALID"),
                (partial, 1, bridge.digest(partial), "BOOTSTRAP_HOME_HAS_TURNS")):
            with self.subTest(reason=reason):
                cdp, frames, count, error = self.run_repair(dict(request, expectedRawDigest=digest), draft, users=users)
                self.assertEqual(error, reason)
                self.assertEqual((cdp.draft, cdp.timeline, cdp.sent, count), (draft, [], 0, 0))

    def test_partial_repair_recorded_attempt_only_reconciles_full_plain_draft(self):
        request, expected = self.repair_request()
        request["attemptRecorded"] = True
        for draft, rich_link, wanted in (
                (expected, False, None), (expected[:-25], False, "BOOTSTRAP_DRAFT_REPAIR_RECONCILIATION_UNVERIFIED"),
                ("foreign", False, "BOOTSTRAP_DRAFT_REPAIR_RECONCILIATION_UNVERIFIED"),
                ("", False, "BOOTSTRAP_DRAFT_REPAIR_RECONCILIATION_UNVERIFIED"),
                (expected, True, "BOOTSTRAP_RICH_DRAFT_UNVERIFIED")):
            with self.subTest(draft=draft[:12], rich_link=rich_link):
                cdp, frames, count, error = self.run_repair(request, draft, rich_link=rich_link)
                self.assertEqual(error, wanted)
                self.assertEqual((cdp.draft, cdp.timeline, cdp.sent, count), (draft, [], 0, 0))
                if wanted is None:
                    self.assertTrue(frames[-1]["reconciled"])
                    self.assertNotIn("BOOTSTRAP_DRAFT_REPAIR_READY", [frame["stage"] for frame in frames])

    def test_partial_repair_lost_result_never_repeats_replacement(self):
        class LostRepairResult(AttemptCdp):
            def insert_text(self, text):
                super().insert_text(text)
                raise bridge.Attention("BOOTSTRAP_DRAFT_REPAIR_UNVERIFIED")
        request, expected = self.repair_request()
        partial = bridge.keyboard_safe_message(expected)[:-25]
        request["expectedRawDigest"] = bridge.digest(partial)
        cdp, frames, count, error = self.run_repair(request, partial, cdp_type=LostRepairResult)
        self.assertEqual(error, "BOOTSTRAP_DRAFT_REPAIR_UNVERIFIED")
        self.assertEqual((cdp.timeline, cdp.sent, count), (["insert"], 0, 1))
        self.assertNotIn("BOOTSTRAP_DRAFT_REPAIRED", [frame["stage"] for frame in frames])
        recovered, frames, count, error = self.run_repair(dict(request, attemptRecorded=True), cdp.draft)
        self.assertIsNone(error)
        self.assertEqual((recovered.timeline, recovered.sent, count), ([], 0, 0))
        self.assertTrue(frames[-1]["reconciled"])

    def test_partial_repair_large_witness_refuses_before_ack(self):
        request, expected = self.repair_request("😀" * 800 + " retained suffix")
        partial = bridge.keyboard_safe_message(expected)[:-15]
        request["expectedRawDigest"] = bridge.digest(partial)
        cdp, frames, count, error = self.run_repair(request, partial)
        self.assertEqual(error, "BOOTSTRAP_DRAFT_REPAIR_FRAME_TOO_LARGE")
        self.assertEqual((cdp.draft, cdp.timeline, cdp.sent, count), (partial, [], 0, 0))

    def test_partial_repair_invalid_request_fails_before_browser_navigation(self):
        request, expected = self.repair_request()
        for changes in ({"bootstrapProtocol": True}, {"attemptRecorded": 1},
                        {"expectedRawDigest": "A" * 64}, {"extra": 1},
                        {"source": dict(request["source"], generation=True)}):
            with self.subTest(changes=changes):
                cdp, frames, count, error = self.run_repair(dict(request, **changes), expected[:-20])
                self.assertIsNotNone(error)
                self.assertEqual((cdp.navigation, cdp.timeline, cdp.sent, count), ([], [], 0, 0))

    def test_large_repair_request_uses_explicit_bootstrap_request_bound(self):
        request, _expected = self.repair_request("x" * 9000)
        wire = (json.dumps(request, separators=(",", ":")) + "\n").encode()
        self.assertGreater(len(wire), 8192)
        with patch.object(adapter.sys, "stdin", types.SimpleNamespace(buffer=io.BytesIO(wire))):
            self.assertEqual(adapter.read_job_request(), request)

    def test_archive_requires_exact_owned_draft_and_durable_ack_before_clear(self):
        request = self.v2_request()
        archive = {"bootstrapDraftArchive": True, "bootstrapProtocol": 2,
                   "archiveRecorded": False, "projectId": request["projectId"],
                   "intentId": request["intentId"], "source": request["source"],
                   "handoff": request["handoff"]}
        expected = bridge.compose_bootstrap_message(
            archive["projectId"], archive["intentId"], SOURCE, archive["handoff"])

        def run(draft, *, recorded=False, ack=True, after_ready=None):
            cdp = AttemptCdp()
            cdp.draft = draft
            sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
            emitted = []
            read_count = []
            scoped = dict(archive, archiveRecorded=recorded)
            def read_ack():
                read_count.append(True)
                self.assertEqual(cdp.timeline, [], "clear must follow durable archive ACK")
                self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_DRAFT_ARCHIVE_READY")
                if after_ready:
                    after_ready(cdp)
                return {"archiveDraft": True} if ack else {}
            with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
                adapter, "read_request", read_ack
            ), patch.object(adapter, "emit", emitted.append):
                try:
                    adapter.bootstrap_draft_archive(bridge, sb, scoped)
                    error = None
                except bridge.Attention as failure:
                    error = str(failure)
            return cdp, emitted, len(read_count), error

        cdp, emitted, reads, error = run(expected)
        self.assertIsNone(error)
        self.assertEqual((reads, cdp.timeline, cdp.draft), (1, ["clear"], ""))
        self.assertEqual([item["stage"] for item in emitted if item["stage"].startswith("BOOTSTRAP_DRAFT_")],
                         ["BOOTSTRAP_DRAFT_ARCHIVE_READY", "BOOTSTRAP_DRAFT_ARCHIVED"])
        self.assertEqual(emitted[-2]["actualText"], expected)
        self.assertEqual(emitted[-2]["actualBytes"], len(expected.encode("utf-8")))
        self.assertNotIn("actualText", emitted[-1])
        for draft, recorded, reason in (("", False, "BOOTSTRAP_DRAFT_MISSING"),
                                        ("unrelated", False, "BOOTSTRAP_DRAFT_MISMATCH"),
                                        ("partial", True, "BOOTSTRAP_DRAFT_MISMATCH")):
            cdp, emitted, reads, error = run(draft, recorded=recorded)
            self.assertEqual((error, reads, cdp.timeline), (reason, 0, []))
        cdp, emitted, reads, error = run("", recorded=True)
        self.assertIsNone(error)
        self.assertEqual((reads, cdp.timeline, emitted[-1]["actualBytes"]), (0, [], 0))
        cdp, emitted, reads, error = run(expected, ack=False)
        self.assertEqual((error, reads, cdp.timeline), ("BOOTSTRAP_DRAFT_ARCHIVE_NOT_AUTHORIZED", 1, []))
        cdp, emitted, reads, error = run(expected, after_ready=lambda cdp: setattr(cdp, "draft", "other"))
        self.assertEqual((error, reads, cdp.timeline), ("BOOTSTRAP_DRAFT_DRIFT", 1, []))

    def test_archive_refuses_oversized_private_ready_frame_before_ack(self):
        request = self.request()
        request["handoff"] = "é" * 3500
        archive = {"bootstrapDraftArchive": True, "bootstrapProtocol": 2,
                   "archiveRecorded": False, "projectId": request["projectId"],
                   "intentId": request["intentId"], "source": request["source"],
                   "handoff": request["handoff"]}
        draft = bridge.compose_bootstrap_message(
            archive["projectId"], archive["intentId"], SOURCE, archive["handoff"])
        cdp = AttemptCdp()
        cdp.draft = draft
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        emitted = []
        with patch.object(bridge, "NewConversationSink", AttemptSink), patch.object(
            adapter, "emit", emitted.append
        ), patch.object(adapter, "read_request", side_effect=AssertionError("no ACK expected")):
            with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_ARCHIVE_FRAME_TOO_LARGE"):
                adapter.bootstrap_draft_archive(bridge, sb, archive)
        self.assertEqual((cdp.timeline, cdp.sent), ([], 0))
        self.assertNotIn("BOOTSTRAP_DRAFT_ARCHIVE_READY", [item["stage"] for item in emitted])

    def test_multiline_handoff_is_one_keyboard_safe_user_turn_after_ack(self):
        class EnterSensitiveCdp(AttemptCdp):
            def send_keys(self, selector, text, timeout):
                if any(char in text for char in "\r\n\t"):
                    self.sent += 1
                    self.url = ISSUED
                    raise AssertionError("CDP text entry submitted before guarded click")
                super().send_keys(selector, text, timeout)

        request = self.request()
        request["handoff"] = 'Keep JSON {"note":"two  spaces"}\r\nThen\tcontinue.'
        cdp, emitted, acknowledgements, error = self.run_attempt(
            request=request, cdp_type=EnterSensitiveCdp
        )
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
        self.assertEqual(cdp.timeline.count("type"), 1)
        self.assertEqual(cdp.timeline.count("submit"), 1)
        self.assertNotIn("\n", cdp.typed_message)
        self.assertNotIn("\r", cdp.typed_message)
        self.assertNotIn("\t", cdp.typed_message)
        self.assertIn('"note":"two  spaces"', cdp.typed_message)
        expected = bridge.compose_bootstrap_message("project-one", "rollover-1", SOURCE, request["handoff"])
        self.assertEqual(bridge.normalize_message(cdp.typed_message), bridge.normalize_message(expected))
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_COMPLETE")

    def test_uncertain_typing_after_ack_is_not_pre_submit_attention(self):
        class AmbiguousCdp(AttemptCdp):
            def send_keys(self, selector, text, timeout):
                self.sent += 1
                self.url = ISSUED
                raise RuntimeError("ambiguous CDP keyboard outcome")

        cdp, emitted, acknowledgements, error = self.run_attempt(cdp_type=AmbiguousCdp)
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_UNKNOWN")
        self.assertNotIn("BOOTSTRAP_COMPLETE", [item["stage"] for item in emitted])

    def test_url_drift_and_ambiguous_click_are_single_send_unknown(self):
        for scenario in ("source_revisited", "foreign_url", "click_unknown", "load_error_after_reload"):
            with self.subTest(scenario=scenario):
                cdp, emitted, acknowledgements, error = self.run_attempt(scenario)
                self.assertIsNone(error)
                self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
                self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_UNKNOWN")
                self.assertNotIn("BOOTSTRAP_COMPLETE", [item["stage"] for item in emitted])

    def test_success_uses_issued_url_and_fresh_exact_receipt(self):
        cdp, emitted, acknowledgements, error = self.run_attempt()
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
        self.assertEqual(cdp.navigation, [SOURCE, bridge.NEW_CONVERSATION_HOME])
        self.assertEqual(
            cdp.timeline[-5:],
            ["submit", "accepted", "completion", "fresh-document", "exact-receipt"],
        )
        before = next(item for item in emitted if item["stage"] == "BEFORE_BOOTSTRAP_SUBMIT")
        receipt = emitted[-1]["receipt"]
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_COMPLETE")
        self.assertEqual(receipt["destinationUrl"], ISSUED)
        self.assertEqual(receipt["destinationDigest"], bridge.digest(ISSUED))
        self.assertEqual(receipt["messageDigest"], before["messageDigest"])
        self.assertEqual(receipt["sourceGeneration"], 7)
        self.assertEqual(receipt["sentUtc"], 99)
        self.assertEqual(receipt["evidence"], "EXACT_USER_MESSAGE_APPENDED")
        self.assertEqual(receipt["completion"], "RESPONSE_COMPLETED")
        self.assertNotIn("handoff", json.dumps(emitted))

    def test_cooperative_bootstrap_saves_pending_receipt_without_response_wait(self):
        request = self.request()
        request["cooperative"] = True
        cdp, emitted, acknowledgements, error = self.run_attempt(request=request)
        self.assertIsNone(error)
        self.assertEqual((cdp.sent, len(acknowledgements)), (1, 1))
        self.assertNotIn("completion", cdp.timeline)
        self.assertEqual(cdp.timeline[-2:], ["fresh-document", "exact-receipt"])
        self.assertEqual(emitted[-1]["stage"], "BOOTSTRAP_SUBMITTED")
        receipt = emitted[-1]["receipt"]
        self.assertEqual(receipt["destinationUrl"], ISSUED)
        self.assertEqual(receipt["destinationDigest"], bridge.digest(ISSUED))
        self.assertEqual(receipt["completion"], "RESPONSE_PENDING")

    def test_pending_bootstrap_probe_is_read_only_and_rejects_wrong_receipt(self):
        request = self.request()
        request["cooperative"] = True
        _original, emitted, _ack, error = self.run_attempt(request=request)
        self.assertIsNone(error)
        receipt = emitted[-1]["receipt"]

        class ProbeSink:
            stages = ["RESPONSE_PENDING"]

            def __init__(self, url, profile, ui_timeout, submit_timeout):
                self.url = url

            def observe_cooperative_response(self, cdp, message, message_digest):
                self.assert_probe(cdp, message, message_digest)
                return self.stages.pop(0) if len(self.stages) > 1 else self.stages[0]

            @staticmethod
            def assert_probe(cdp, message, message_digest):
                assert cdp.url == ISSUED and cdp.sent == 0
                assert message_digest == bridge.digest(bridge.normalize_message(message))

        def run_probe(probe_receipt):
            cdp = AttemptCdp()
            sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
            output = []
            probe_request = {
                "bootstrapProbe": True,
                "projectId": "project-one",
                "intentId": "rollover-1",
                "source": self.request()["source"],
                "handoff": self.request()["handoff"],
                "receipt": probe_receipt,
            }
            with patch.object(bridge, "CdpSink", ProbeSink), patch.object(
                adapter, "emit", output.append
            ), patch.object(adapter.time, "sleep", return_value=None), patch.object(
                adapter.time, "monotonic", side_effect=range(100)
            ):
                adapter.bootstrap_probe(bridge, sb, probe_request)
            return cdp, output

        first_cdp, first_output = run_probe(receipt)
        self.assertEqual(first_cdp.sent, 0)
        self.assertEqual(first_output[-1]["stage"], "BOOTSTRAP_RESPONSE_PENDING")
        ProbeSink.stages = ["RESPONSE_PENDING", "RESPONSE_COMPLETE", "RESPONSE_COMPLETE"]
        second_cdp, second_output = run_probe(receipt)
        self.assertEqual(second_cdp.sent, 0)
        self.assertEqual(second_output[-1]["stage"], "BOOTSTRAP_COMPLETE")
        self.assertEqual(second_output[-1]["receipt"]["completion"], "RESPONSE_COMPLETED")
        self.assertEqual(second_output[-1]["receipt"]["destinationUrl"], ISSUED)

        wrong = dict(receipt, destinationDigest="0" * 64)
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_PROBE_RECEIPT_INVALID"):
            run_probe(wrong)
        wrong_source = self.request()["source"].copy()
        wrong_source["generation"] = True
        cdp = AttemptCdp()
        sb = types.SimpleNamespace(cdp=cdp, activate_cdp_mode=lambda url: None)
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_PROBE_SOURCE_INVALID"):
            with patch.object(bridge, "CdpSink", ProbeSink):
                adapter.bootstrap_probe(bridge, sb, {
                    "bootstrapProbe": True, "projectId": "project-one",
                    "intentId": "rollover-1", "source": wrong_source,
                    "handoff": self.request()["handoff"], "receipt": receipt,
                })
        self.assertEqual(cdp.navigation, [])


    def test_quarantined_candidate_inspection_reads_without_receipt_or_submit(self):
        original = self.request()
        request = {"bootstrapQuarantineInspect": True, "projectId": original["projectId"],
                   "intentId": original["intentId"], "source": original["source"],
                   "handoff": original["handoff"], "candidateUrl": ISSUED}
        expected = bridge.compose_bootstrap_message(
            request["projectId"], request["intentId"], SOURCE, request["handoff"])

        class ReadSink:
            response = "RESPONSE_COMPLETE"
            ready = "complete"

            def __init__(self, url, profile, ui_timeout, submit_timeout):
                assert url == ISSUED

            def observe_cooperative_response(self, cdp, message, message_digest):
                assert cdp.url == ISSUED and cdp.sent == 0
                assert message == expected
                assert message_digest == bridge.digest(bridge.normalize_message(expected))
                return self.response

            def exact(self, url):
                return url == ISSUED

            def document_ready_state(self, cdp):
                return self.ready

            def user_message_digests(self, cdp):
                return [bridge.digest(bridge.normalize_message(expected))]

            def trusted_turn_anchor_state(self, cdp, message):
                assert message == expected
                return 1, 1

        for response in ("RESPONSE_PENDING", "RESPONSE_COMPLETE"):
            with self.subTest(response=response):
                ReadSink.response = response
                cdp = AttemptCdp()
                output = []
                sb = types.SimpleNamespace(cdp=cdp)
                with patch.object(bridge, "CdpSink", ReadSink), patch.object(adapter, "emit", output.append), \
                        patch.object(adapter.time, "sleep"), patch.object(adapter.time, "monotonic", side_effect=range(100)):
                    adapter.bootstrap_quarantine_inspect(bridge, sb, request)
                result = output[-1]
                self.assertEqual(result["stage"], "BOOTSTRAP_QUARANTINE_INSPECTED")
                self.assertEqual(result["responseState"], response)
                self.assertTrue(result["existingUserMessageVerified"])
                self.assertTrue(result["observationOnly"])
                self.assertEqual(cdp.sent, 0)
                self.assertNotIn("receipt", result)
                self.assertNotIn("sentUtc", result)
                self.assertNotIn(ISSUED, json.dumps(result))
                self.assertNotIn(request["handoff"], json.dumps(result))

        for changed in (
            dict(request, candidateUrl=SOURCE),
            dict(request, source=dict(request["source"], generation=True)),
            dict(request, source=dict(request["source"], digest="0" * 64)),
            dict(request, receipt={"evidence": "EXACT_USER_MESSAGE_APPENDED"}),
        ):
            cdp = AttemptCdp()
            with self.assertRaises(bridge.Attention), patch.object(bridge, "CdpSink", ReadSink):
                adapter.bootstrap_quarantine_inspect(bridge, types.SimpleNamespace(cdp=cdp), changed)
            self.assertEqual(cdp.navigation, [])
            self.assertEqual(cdp.sent, 0)

        class MismatchedUser(ReadSink):
            def observe_cooperative_response(self, cdp, message, message_digest):
                raise bridge.Attention("RECEIPT_USER_MISMATCH")

        output = []
        cdp = AttemptCdp()
        with patch.object(bridge, "CdpSink", MismatchedUser), patch.object(adapter, "emit", output.append), \
                self.assertRaisesRegex(bridge.Attention, "RECEIPT_USER_MISMATCH"):
            adapter.bootstrap_quarantine_inspect(bridge, types.SimpleNamespace(cdp=cdp), request)
        self.assertEqual(cdp.sent, 0)
        self.assertNotIn("BOOTSTRAP_QUARANTINE_INSPECTED", [item["stage"] for item in output])

        class Unhydrated(ReadSink):
            response = "RESPONSE_PENDING"
            ready = "loading"

        class WrongUser(ReadSink):
            def user_message_digests(self, cdp):
                return ["0" * 64]

        for sink_type in (Unhydrated, WrongUser):
            cdp = AttemptCdp()
            output = []
            with patch.object(bridge, "CdpSink", sink_type), patch.object(adapter, "emit", output.append), \
                    patch.object(adapter.time, "sleep"), patch.object(adapter.time, "monotonic", side_effect=range(100)), \
                    self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_QUARANTINE_USER_UNPROVEN"):
                adapter.bootstrap_quarantine_inspect(bridge, types.SimpleNamespace(cdp=cdp), request)
            self.assertEqual(cdp.sent, 0)
            self.assertNotIn("BOOTSTRAP_QUARANTINE_INSPECTED", [item["stage"] for item in output])

    def test_quarantine_inspection_request_uses_existing_bounded_handoff_reader(self):
        request = {"bootstrapQuarantineInspect": True, "handoff": "x" * 9000}
        raw = json.dumps(request).encode() + b"\n"
        self.assertGreater(len(raw), 8192)
        with patch.object(adapter.sys, "stdin", types.SimpleNamespace(buffer=io.BytesIO(raw))):
            self.assertEqual(adapter.read_job_request(), request)
        raw = json.dumps(dict(request, handoff="x" * (32 * 1024))).encode() + b"\n"
        with patch.object(adapter.sys, "stdin", types.SimpleNamespace(buffer=io.BytesIO(raw))), self.assertRaises(ValueError):
            adapter.read_job_request()


if __name__ == "__main__":
    unittest.main()
