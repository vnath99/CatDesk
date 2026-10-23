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
from unittest.mock import patch

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
    const has = s.match(/^([^:]+):has\(([^()]+)\)$/);
    if (has) return this.matches(has[1]) && Boolean(this.querySelector(has[2]));
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
  el('div', options.userClassOnly ? {class:'group/user-message'} : {'data-user-message-bubble':'true'}, '', [
    el('div', options.userBodyClass ? {class:'whitespace-pre-wrap'} : {'data-markdown-text-tone':'user-message'}, options.userText ?? 'exact wake message'),
    el('div', {class:'turn-action-controls'}, '', [button(options.userCopy || 'Copy message')]),
  ]),
]);
if (options.userMultipleMarkers) {
  user.attrs['data-message-author-role'] = 'user';
  user.children[0].attrs.class = 'group/user-message';
}
if (options.nestedUserMarkers) {
  user.attrs.class = 'group/user-message';
  const inner = user.children[0];
  const wrapper = el('div', {class:'group/user-message'}, '', [inner]);
  wrapper.parentElement = user;
  user.children = [wrapper];
  if (options.conflictingNestedUserIds) {
    user.attrs['data-chatgpt-selection-message-id'] = 'outer-message';
    wrapper.attrs['data-chatgpt-selection-message-id'] = 'inner-message';
  }
}
if (options.userShowMore) user.children[0].children.push(el('span', {}, '…'), button('Show more'));
const answer = options.loadError ? "This response couldn’t load" : (options.answer ?? 'A useful answer');
const assistant = el('div', {'data-content-search-unit-key':'fallback-turn-0:2:assistant'}, '', [
  el('h4', {'data-conversation-role':'assistant'}, 'ChatGPT said:'),
  el('div', {'data-chatgpt-selection-message-id':'assistant-id'}, '', options.controlsOnly ? [] : [
    el('div', {'data-markdown-text-style':'assistant-message'}, answer, [
      el('div', {class:'turn-action-controls'}, '', [button('Copy')]),
    ]),
  ]),
]);
const children = options.userOnly ? [user] : [user, assistant];
if (options.duplicateUser) children.splice(1, 0, user.cloneNode());
for (const control of options.submitControls || []) children.push(el('button', {
  'aria-label':control.label, type:control.type, disabled:control.disabled || false
}));
if (options.extraAssistant) children.push(el('div', {'data-content-search-unit-key':'assistant-2'}, '', [
  el('h4', {'data-conversation-role':'assistant'}, 'ChatGPT said:'),
]));
if (options.toolbar !== false) children.push(el('div', {class:'turn-action-controls'}, '', [button('Copy')]));
if (options.duplicateToolbar) children.push(el('div', {class:'turn-action-controls'}, '', [button('Copy')]));
const paired = el('div', {'data-content-search-turn-key':'fallback-turn-0'}, '', children);
if (options.bareUser) {
  delete user.attrs['data-content-search-unit-key'];
  delete paired.attrs['data-content-search-turn-key'];
}
global.document = el('document', {}, '', [paired]);
global.window = {getComputedStyle: () => ({visibility:'visible', display:'block'})};
global.location = {href:options.location || 'https://chatgpt.com/c/current-dom'};
let mutations = 0;
if (options.traceMutations) {
  const editor = el('div', {id:'prompt-textarea', contenteditable:'true'}, 'owned draft');
  editor.isContentEditable = true;
  editor.focus = () => {mutations++;};
  document.children.push(editor);
  document.createRange = () => ({selectNodeContents:() => {}});
  window.getSelection = () => ({removeAllRanges:() => {}, addRange:() => {}});
  document.execCommand = () => {mutations++; return true;};
}
global.Node = {DOCUMENT_POSITION_FOLLOWING:4};
const value = eval(fs.readFileSync(0, 'utf8'));
process.stdout.write(JSON.stringify(options.traceMutations ? {value, mutations} : value));
"""


class CurrentDom:
    def __init__(self, **options):
        self.options = options

    def get_current_url(self):
        return "https://chatgpt.com/c/current-dom"

    def evaluate(self, script):
        result = subprocess.run(
            [shutil.which("node"), "-e", DOM, json.dumps(self.options, ensure_ascii=False)],
            input=script, text=True, encoding="utf-8", capture_output=True, timeout=10,
        )
        if result.returncode:
            raise AssertionError(result.stderr)
        observed = json.loads(result.stdout)
        if self.options.get("traceMutations"):
            self.mutations = observed["mutations"]
            return observed["value"]
        return observed


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
        self.assertTrue(all(m["bodyTextObserved"] for m in observed["messages"]))

    def test_long_body_excerpt_is_identified_without_claiming_full_revision(self):
        for answer in ("Actual body text. " * 600, "😀" * 3000, "😀" * 3000 + "x"):
            with self.subTest(unicode=answer.startswith("😀")):
                snapshot = self.sink.visible_message_snapshot(CurrentDom(answer=answer))
                message = snapshot["messages"][-1]
                self.assertTrue(message["bodyTextObserved"])
                self.assertFalse(message["textComplete"])
                self.assertFalse(message["revisionComplete"])
                self.assertIsNone(message["revisionHash"])
                self.assertIsNotNone(message["messageHash"])
                self.assertTrue(snapshot["truncated"])
                self.assertGreater(message["visibleUnits"], 4096)
                self.assertLessEqual(len(message["text"].encode("utf-8")), 4096)
                self.assertTrue(message["text"])
                self.assertNotIn("Copy", message["text"])
        diagnostic = self.sink.visible_message_snapshot(CurrentDom(controlsOnly=True))["messages"][-1]
        self.assertFalse(diagnostic["bodyTextObserved"])

    def test_current_user_receipt_and_following_assistant_anchor(self):
        self.assertEqual(self.sink.user_message_digests(CurrentDom()), [bridge.digest("exact wake message")])
        self.assertEqual(self.sink.trusted_turn_anchor_state(CurrentDom(), "exact wake message"), (1, 1))

    def test_class_only_user_receipt_agrees_with_snapshot_and_turn_anchor(self):
        cdp = CurrentDom(userClassOnly=True, answer="exact wake message")
        observed = self.sink.visible_message_snapshot(cdp)
        self.assertEqual(observed["messages"][0]["role"], "USER")
        self.assertEqual(observed["messages"][0]["text"], "exact wake message")
        self.assertEqual(self.sink.trusted_turn_anchor_state(cdp, "exact wake message"), (1, 1))
        # An assistant echo of the message must not become a second USER.
        self.assertEqual(self.sink.user_message_digests(cdp), [bridge.digest("exact wake message")])

    def test_user_representations_do_not_duplicate_but_distinct_turns_do(self):
        expected = bridge.digest("exact wake message")
        self.assertEqual(self.sink.user_message_digests(CurrentDom(userMultipleMarkers=True)), [expected])
        cdp = CurrentDom(userClassOnly=True, duplicateUser=True)
        self.assertEqual(self.sink.user_message_digests(cdp), [expected, expected])
        self.assertEqual(self.sink.trusted_turn_anchor_state(cdp, "exact wake message"), (2, 0))

    def test_nested_current_user_markers_count_one_logical_turn(self):
        cdp = CurrentDom(nestedUserMarkers=True)
        self.assertEqual(self.sink.visible_role_counts(cdp)[:2], (1, 1))
        self.assertEqual(self.sink.user_message_digests(cdp), [bridge.digest("exact wake message")])
        self.sink.prove_single_bootstrap_user(cdp, "exact wake message")

    def test_distinct_identical_users_are_not_collapsed_by_text_or_key(self):
        cdp = CurrentDom(nestedUserMarkers=True, duplicateUser=True)
        self.assertEqual(self.sink.visible_role_counts(cdp)[0], 2)
        with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_RECEIPT_SEQUENCE_DRIFT"):
            self.sink.prove_single_bootstrap_user(cdp, "exact wake message")

    def test_empty_or_whitespace_assistant_body_is_incomplete_context(self):
        for answer in ("", " \n\t"):
            with self.subTest(answer=repr(answer)):
                message = self.sink.visible_message_snapshot(CurrentDom(answer=answer))["messages"][-1]
                self.assertFalse(message["bodyTextObserved"])
                self.assertFalse(message["textComplete"])
                self.assertFalse(message["revisionComplete"])
                self.assertIsNone(message["revisionHash"])
                with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_ASSISTANT_UNAVAILABLE"):
                    self.sink.prove_single_bootstrap_user(CurrentDom(answer=answer), "exact wake message")

    def test_nested_conflicting_message_ids_remain_separate(self):
        cdp = CurrentDom(nestedUserMarkers=True, conflictingNestedUserIds=True)
        self.assertEqual(self.sink.visible_role_counts(cdp)[0], 2)
        with self.assertRaisesRegex(bridge.PostSubmitUnknown, "BOOTSTRAP_RECEIPT_SEQUENCE_DRIFT"):
            self.sink.prove_single_bootstrap_user(cdp, "exact wake message")

    def test_class_only_user_blocks_home_clear_even_after_prior_checks(self):
        cdp = CurrentDom(userClassOnly=True, userOnly=True, bareUser=True,
                         location=bridge.NEW_CONVERSATION_HOME, traceMutations=True)
        self.assertEqual(self.sink.visible_role_counts(cdp), (1, 0, 0))
        # Simulate the prior checks passing before a USER mounts. The atomic
        # clear expression must independently refuse before focusing/deleting.
        with patch.object(self.sink, "check_typed_home"), self.assertRaisesRegex(
                bridge.Attention, "BOOTSTRAP_ARCHIVE_CLEAR_UNVERIFIED"):
            self.sink.clear_archived_home_draft(cdp, "owned draft", "owned draft")
        self.assertEqual(cdp.mutations, 0)

    def test_observer_classifies_actual_dom_mismatch_without_positive_evidence(self):
        header = "CatDesk bootstrap.\nProject: alpha\nIntent: owned\nPredecessor: exact"
        message = header + "\n\nPRIVATE_EXPECTED_HANDOFF"
        for text, duplicate, reason in (
            (header + "\n\nchanged body", False, "RESPONSE_EXPECTED_USER_BODY_MISMATCH"),
            (header.replace("Intent: owned", "Intent: other") + "\n\nchanged body", False,
             "RESPONSE_EXPECTED_USER_NOT_FOUND"),
            (message, True, "RESPONSE_EXPECTED_USER_DUPLICATED"),
        ):
            cdp = CurrentDom(userClassOnly=True, userText=text, duplicateUser=duplicate)
            with self.subTest(reason=reason), patch.object(
                self.sink, "post_submit_visible_any", return_value=False
            ), patch.object(self.sink, "document_ready_state", return_value="complete"), \
                    patch.object(self.sink, "response_snapshot", side_effect=AssertionError("must refuse before assistant")), \
                    self.assertRaisesRegex(bridge.Attention, reason) as error:
                self.sink.observe_cooperative_response(cdp, message, bridge.digest(bridge.normalize_message(message)))
            self.assertNotIn("PRIVATE_EXPECTED_HANDOFF", str(error.exception))

    def test_exact_dom_response_completion_does_not_enter_mismatch_diagnostics(self):
        with patch.object(self.sink, "post_submit_visible_any", return_value=False), patch.object(
            self.sink, "document_ready_state", return_value="complete"
        ), patch.object(self.sink, "editor_state", return_value=("ready", True)), patch.object(
            self.sink, "missing_user_reason", side_effect=AssertionError("exact message must not enter diagnostic")
        ):
            self.assertEqual(self.sink.observe_cooperative_response(
                CurrentDom(userClassOnly=True), "exact wake message", bridge.digest("exact wake message")),
                "RESPONSE_COMPLETE")

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
