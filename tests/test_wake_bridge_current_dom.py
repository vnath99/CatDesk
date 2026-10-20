"""Execute the bridge's actual DOM probes against the October Web structure.

No browser/network is used. The tiny DOM below implements selectors over a tree,
instead of returning canned probe outcomes. Nodes mirror a sanitized live read.
"""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("current_dom_bridge", ROOT / "scripts/wake_bridge.py")
bridge = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bridge
SPEC.loader.exec_module(bridge)

DOM = r"""
const fs = require('fs');
const options = JSON.parse(process.argv[1]);
class Element {
  constructor(tag, attrs = {}, text = '', children = []) {
    this.tagName = tag; this.attrs = attrs; this.text = text;
    this.children = children; this.parentElement = null;
    for (const child of children) child.parentElement = this;
  }
  get textContent() { return this.text + this.children.map(c => c.textContent).join(''); }
  get innerText() { return this.textContent; }
  get disabled() { return this.attrs.disabled === true; }
  getAttribute(name) { return this.attrs[name] ?? null; }
  getClientRects() { return [1]; }
  matches(selector) { return selector.split(',').some(raw => {
    const s = raw.trim();
    if (s.includes(':')) return false;
    const parts = s.split(/\s+(?![^\[]*\])/);
    if (parts.length > 1) {
      if (!this.matches(parts.pop())) return false;
      return !!this.parentElement?.closest(parts.join(' '));
    }
    const tag = s.match(/^[a-z][a-z0-9-]*/i);
    if (tag && tag[0].toLowerCase() !== this.tagName.toLowerCase()) return false;
    const id = s.match(/#([a-zA-Z0-9_-]+)/);
    if (id && this.getAttribute('id') !== id[1]) return false;
    const classes = [...s.matchAll(/\.([a-zA-Z0-9_-]+)/g)].map(m => m[1]);
    if (classes.some(c => !(this.attrs.class || '').split(' ').includes(c))) return false;
    for (const m of s.matchAll(/\[([^\s~^=\]]+)(?:(~=|\^=|=)["']?([^\]"']*)["']?)?\]/g)) {
      const value = this.getAttribute(m[1]);
      if (value === null || (m[2] === '=' && value !== m[3]) ||
          (m[2] === '~=' && !value.split(' ').includes(m[3])) ||
          (m[2] === '^=' && !value.startsWith(m[3]))) return false;
    }
    return true;
  }); }
  querySelectorAll(selector) {
    return this.children.flatMap(c => [c, ...c.querySelectorAll('*')]).filter(c => c.matches(selector));
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
  closest(selector) { return this.matches(selector) ? this : this.parentElement?.closest(selector) || null; }
  contains(node) { return node === this || this.children.some(c => c.contains(node)); }
  compareDocumentPosition(node) {
    const all = document.querySelectorAll('*');
    return all.indexOf(node) > all.indexOf(this) ? 4 : 0;
  }
  cloneNode() { return new Element(this.tagName, {...this.attrs}, this.text, this.children.map(c => c.cloneNode())); }
  remove() { this.parentElement.children = this.parentElement.children.filter(c => c !== this); }
}
const el = (tag, attrs = {}, text = '', children = []) => new Element(tag, attrs, text, children);
const button = label => el('button', {'aria-label':label}, label);
const user = el('div', {'data-content-search-unit-key':'fallback-turn-0:0:user'}, '', [
  el('div', {'data-user-message-bubble':'true'}, '', [
    el('div', options.userBodyClass ? {class:'whitespace-pre-wrap'} : {'data-markdown-text-tone':'user-message'}, 'exact wake message'),
    el('div', {class:'turn-action-controls'}, '', [button(options.userCopy || 'Copy message')]),
  ]),
]);
if (options.userShowMore) user.children[0].children.push(el('span', {}, '…'), button('Show more'));
const answer = options.loadError ? "This response couldn’t load" : 'A useful answer';
const assistant = el('div', {'data-content-search-unit-key':'fallback-turn-0:2:assistant'}, '', [
  el('h4', {'data-conversation-role':'assistant'}, 'ChatGPT said:'),
  el('div', {'data-chatgpt-selection-message-id':'assistant-id'}, '', options.controlsOnly ? [] : [
    el('div', {'data-markdown-text-style':'assistant-message'}, answer, [
      el('div', {class:'turn-action-controls'}, '', [button('Copy')]),
    ]),
  ]),
]);
const children = [user, assistant];
for (const control of options.submitControls || []) children.push(el('button', {
  'aria-label':control.label, type:control.type, disabled:control.disabled || false
}));
if (options.extraAssistant) children.push(el('div', {'data-content-search-unit-key':'assistant-2'}, '', [
  el('h4', {'data-conversation-role':'assistant'}, 'ChatGPT said:'),
]));
if (options.toolbar !== false) children.push(el('div', {class:'turn-action-controls'}, '', [button('Copy')]));
if (options.duplicateToolbar) children.push(el('div', {class:'turn-action-controls'}, '', [button('Copy')]));
const paired = el('div', {'data-content-search-turn-key':'fallback-turn-0'}, '', children);
global.document = el('document', {}, '', [paired]);
global.window = {getComputedStyle: () => ({visibility:'visible', display:'block'})};
global.Node = {DOCUMENT_POSITION_FOLLOWING:4};
process.stdout.write(JSON.stringify(eval(fs.readFileSync(0, 'utf8'))));
"""


class CurrentDom:
    def __init__(self, **options):
        self.options = options

    def get_current_url(self):
        return "https://chatgpt.com/c/current-dom"

    def evaluate(self, script):
        result = subprocess.run(
            [shutil.which("node"), "-e", DOM, json.dumps(self.options)],
            input=script, text=True, encoding="utf-8", capture_output=True, timeout=10,
        )
        if result.returncode:
            raise AssertionError(result.stderr)
        return json.loads(result.stdout)


@unittest.skipUnless(shutil.which("node"), "Node.js required for offline DOM probes")
class CurrentDomTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.sink = bridge.NewConversationSink(
            "https://chatgpt.com/c/current-dom", Path(self.directory.name), 1, 1
        )

    def test_snapshot_uses_real_semantic_units_without_legacy_role_nodes(self):
        observed = self.sink.visible_message_snapshot(CurrentDom())
        self.assertEqual(observed["coverage"]["selectorTier"], "SEMANTIC_UNIT")
        self.assertEqual([m["role"] for m in observed["messages"]], ["USER", "ASSISTANT"])
        self.assertEqual([m["text"] for m in observed["messages"]], ["exact wake message", "A useful answer"])
        self.assertTrue(all(m["textComplete"] and m["revisionComplete"] for m in observed["messages"]))
        self.assertEqual(observed["coverage"]["anchorStability"], "UNVERIFIED")

    def test_current_user_receipt_and_following_assistant_anchor(self):
        self.assertEqual(self.sink.user_message_digests(CurrentDom()), [bridge.digest("exact wake message")])
        self.assertEqual(self.sink.trusted_turn_anchor_state(CurrentDom(), "exact wake message"), (1, 1))

    def test_private_body_shape_diagnostics_contain_no_message_text(self):
        for options, count in (({}, 1), ({"controlsOnly": True}, 0)):
            with self.subTest(options=options):
                shape = self.sink.message_body_diagnostics(CurrentDom(**options))
                self.assertEqual(shape["status"], "OBSERVED")
                self.assertEqual(shape["selectorTier"], "SEMANTIC_UNIT")
                self.assertEqual(shape["bodyCount"], count)
                self.assertLessEqual(len(json.dumps(shape).encode("utf-8")), 3072)
                self.assertNotIn("A useful answer", json.dumps(shape))
                self.assertNotIn("assistant-id", json.dumps(shape))
        self.assertFalse(self.sink.visible_message_snapshot(CurrentDom(controlsOnly=True))["messages"][-1]["textComplete"])

    def test_body_shape_refuses_wrong_target_and_sanitizes_failures(self):
        class Wrong:
            def get_current_url(self): return "https://chatgpt.com/c/foreign"
            def evaluate(self, script): raise AssertionError("must not evaluate")
        with self.assertRaisesRegex(bridge.Attention, "SOURCE_SNAPSHOT_SOURCE_DRIFT"):
            self.sink.message_body_diagnostics(Wrong())
        class Failure(CurrentDom):
            def evaluate(self, script): raise RuntimeError("private page details")
        self.assertEqual(self.sink.message_body_diagnostics(Failure()), {"status":"UNAVAILABLE"})

    def test_collapsible_user_controls_do_not_change_message_digest(self):
        cdp = CurrentDom(userBodyClass=True, userShowMore=True)
        observed = self.sink.visible_message_snapshot(cdp)["messages"][0]
        self.assertEqual(observed["text"], "exact wake message")
        self.assertTrue(observed["textComplete"])
        self.assertEqual(bridge.digest(bridge.normalize_message(observed["text"])),
                         self.sink.user_message_digests(cdp)[0])

    def test_new_chat_plain_send_requires_submit_type_and_unique_enabled_control(self):
        send = {"label":"Send", "type":"submit"}
        selector = "button[type='submit'][aria-label='Send']"
        self.assertEqual(self.sink.select_send_control(CurrentDom(submitControls=[send])),
                         ("ready", selector))
        for controls, status in (([send, send], "ambiguous"),
                                 ([{**send, "disabled":True}], "disabled"),
                                 ([{"label":"Send", "type":"button"}], "missing"),
                                 ([{"label":"Dictate", "type":"submit"}], "missing")):
            with self.subTest(controls=controls):
                self.assertEqual(self.sink.select_send_control(CurrentDom(submitControls=controls))[0], status)

    def test_adjacent_assistant_toolbar_proves_usable_response(self):
        self.assertEqual(self.sink.response_snapshot(CurrentDom(), "exact wake message", require_usable_body=True),
                         (0, 0, True, True))

    def test_user_and_code_block_copy_cannot_complete_response(self):
        self.assertEqual(self.sink.response_snapshot(CurrentDom(toolbar=False, userCopy="Copy"),
                         "exact wake message", require_usable_body=True), (0, 0, True, False))

    def test_ambiguous_toolbar_or_assistant_cannot_complete_response(self):
        for options in ({"duplicateToolbar": True}, {"extraAssistant": True}):
            with self.subTest(options=options):
                self.assertFalse(self.sink.response_snapshot(CurrentDom(**options), "exact wake message",
                                                            require_usable_body=True)[3])

    def test_error_placeholder_and_controls_only_are_not_usable(self):
        with self.assertRaisesRegex(bridge.PostSubmitUnknown, "RESPONSE_COMPLETION_UNPROVEN"):
            self.sink.response_snapshot(CurrentDom(loadError=True), "exact wake message", require_usable_body=True)
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER"):
            self.sink.require_usable_latest_assistant(CurrentDom(loadError=True), source=True)
        observed = self.sink.visible_message_snapshot(CurrentDom(controlsOnly=True))
        self.assertFalse(observed["messages"][-1]["textComplete"])
        self.assertFalse(self.sink.response_snapshot(CurrentDom(controlsOnly=True), "exact wake message",
                                                   require_usable_body=True)[3])


if __name__ == "__main__":
    unittest.main()
