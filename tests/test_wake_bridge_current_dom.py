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
  get innerText() { return (options.generatedLink && this.attrs['text-link-href'] ? '\n' : '') + this.text + this.children.map(c => c.innerText).join(''); }
  get nodeType() { return 1; }
  get childNodes() { return [{nodeType:3, nodeValue:this.text}, ...this.children]; }
  get disabled() { return this.attrs.disabled === true; }
  getAttribute(name) { return this.attrs[name] ?? null; }
  hasAttribute(name) { return Object.hasOwn(this.attrs, name); }
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
    for (const m of s.matchAll(/\[([^\s~^*=\]]+)(?:(~=|\^=|\*=|=)(?:"([^"]*)"|'([^']*)'|([^\]\s]+))(?:\s+(i))?)?\]/g)) {
      let value = this.getAttribute(m[1]);
      let expected = m[3] ?? m[4] ?? m[5];
      if (value === null) return false;
      if (m[6] === 'i') {value = value.toLowerCase(); expected = expected?.toLowerCase();}
      if ((m[2] === '=' && value !== expected) ||
          (m[2] === '~=' && !value.split(' ').includes(expected)) ||
          (m[2] === '^=' && !value.startsWith(expected)) ||
          (m[2] === '*=' && !value.includes(expected))) return false;
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
  click() { mutations++; }
}
const el = (tag, attrs = {}, text = '', children = []) => new Element(tag, attrs, text, children);
const append = (parent, child) => { child.parentElement = parent; parent.children.push(child); };
const button = label => el('button', {'aria-label':label}, label);
const user = el('div', {'data-content-search-unit-key':'fallback-turn-0:0:user'}, '', [
  el('div', options.userClassOnly ? {class:'group/user-message'} : {'data-user-message-bubble':'true'}, '', [
    el('div', options.userBodyClass ? {class:'whitespace-pre-wrap'} : {'data-markdown-text-tone':'user-message'}, options.userText ?? 'exact wake message'),
    el('div', {class:'turn-action-controls'}, '', [button(options.userCopy || 'Copy message')]),
  ]),
]);
if (options.userInlineLink) {
  const body = user.children[0].children[0];
  body.text = '';
  body.children = [el('p', {}, '', [
    el('span', {}, options.userPrefix || 'Review source at '),
    el('a', {href:options.userLink || 'https://example.test/compare'},
      options.userLink || 'https://example.test/compare'),
    el('span', {}, options.userSuffix || ' without editing it.'),
  ])];
  body.children[0].parentElement = body;
}
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
if (options.userControlsInsideBody)
  append(user.children[0].children[0], button('PRIVATE_CONTROL_LABEL'));
if (options.userParagraphs) {
  const body = user.children[0].children[0];
  body.text = ''; body.children = [el('p', {}, 'first paragraph'), el('p', {}, 'second paragraph')];
}
if (options.userBodyIsUnit) {
  user.text = user.children[0].children[0].text;
  user.attrs['data-markdown-text-tone'] = 'user-message';
  user.attrs['data-user-message-bubble'] = 'true';
  user.children = [user.children[0].children[1]];
  user.children[0].parentElement = user;
}
const answer = options.loadError ? "This response couldn’t load" : (options.answer ?? 'A useful answer');
const assistant = el('div', {'data-content-search-unit-key':'fallback-turn-0:2:assistant'}, '', [
  el('h4', {'data-conversation-role':'assistant'}, 'ChatGPT said:'),
  el('div', {'data-chatgpt-selection-message-id':'assistant-id'}, '', options.controlsOnly ? [] : [
    el('div', {'data-markdown-text-style':'assistant-message'}, answer, [
      el('div', {class:'turn-action-controls'}, '', [button(options.bodyCopyLabel || 'Copy')]),
    ]),
  ]),
]);
if (options.timeoutCard) append(assistant, el('div', {role:'alert'}, '', [
  el('p', {}, 'Message delivery timed out. Please try again.'),
  ...Array.from({length:options.retryOnlyOutside ? 0 : (options.retryCount ?? 1)}, () => button(options.retryLabel || 'Retry')),
]));
if (options.userTimeoutQuote) {
  append(user.children[0], el('p', {}, 'Message delivery timed out. Please try again.'));
  append(user.children[0], button('Retry'));
}
const children = options.userOnly ? [user] : [user, assistant];
for (let i = 0; i < (options.diagnosticExtraButtons || 0); i++)
  children.push(button(options.diagnosticButtonLabel || 'Unrelated control'));
if (options.auxiliaryUnit) {
  const auxiliary = el('div', {'data-content-search-unit-key':'tool-status'}, '', [
    el('div', {'data-markdown-text-style':'assistant-message'}, 'Tool status only'),
    el('div', {class:'turn-action-controls'}, '', [button(options.auxiliaryCopyLabel || 'Copy')]),
  ]);
  children.splice(options.auxiliaryUnit === 'before' ? 1 : children.length, 0, auxiliary);
}
if (options.duplicateUser) children.splice(1, 0, user.cloneNode());
for (const control of options.submitControls || []) children.push(el('button', {
  'aria-label':control.label, type:control.type, disabled:control.disabled || false
}));
if (options.extraAssistant) children.push(el('div', {'data-content-search-unit-key':'assistant-2'}, '', [
  el('h4', {'data-conversation-role':'assistant'}, 'ChatGPT said:'),
]));
if (options.toolbar !== false) children.push(el('div', {class:'turn-action-controls'}, '', [button(options.assistantCopyLabel || 'Copy')]));
if (options.duplicateToolbar) children.push(el('div', {class:'turn-action-controls'}, '', [button(options.assistantCopyLabel || 'Copy')]));
const paired = el('div', {'data-content-search-turn-key':'fallback-turn-0'}, '', children);
if (options.bareUser) {
  delete user.attrs['data-content-search-unit-key'];
  delete paired.attrs['data-content-search-turn-key'];
}
global.document = el('document', {}, '', [paired]);
if (options.legacyTurns) {
  delete user.attrs['data-content-search-unit-key'];
  delete assistant.attrs['data-content-search-unit-key'];
  user.attrs['data-message-author-role'] = 'user';
  assistant.attrs['data-message-author-role'] = 'assistant';
  document.children = [
    el('div', {'data-testid':'conversation-turn-0'}, '', [user]),
    el('div', {'data-testid':'conversation-turn-1'}, '', [assistant,
      el('div', {class:'turn-action-controls'}, '', [el('button', {'data-testid':'copy-turn-action-button','aria-label':'Copy'}, 'Copy')])]),
  ];
}
if (options.retryOnlyOutside) document.children.push(button('Retry'));
if (options.historicalLoadError) document.children.unshift(el('div', {'data-message-author-role':'assistant'}, 'This response couldn’t load'));
document.readyState = options.documentState || 'complete';
if (options.observationEditor) document.children.push(el('div', {id:'prompt-textarea'}, options.observationDraft || ''));
if (options.observationStop) document.children.push(el('button', {'data-testid':'stop-button'}));
if (options.observationLogin) document.children.push(el('a', {href:'/auth/login'}));
if (options.observationCaptcha) document.children.push(el('iframe', {title:'captcha'}));
if (options.observationNetwork) document.children.push(el('div', {id:'main-frame-error'}), el('div', {id:'error-code'}));
global.window = {getComputedStyle: () => ({visibility:'visible', display:'block'})};
global.location = {href:options.location || 'https://chatgpt.com/c/current-dom'};
if (options.composerCount !== undefined) {
  document.children = [];
  for (let i = 0; i < options.composerCount; i++) {
    const body = options.composerLink ? [
      el(options.generatedLink ? 'SPAN' : 'A', options.generatedLink ? {
        'text-link-href':options.linkHref || 'https://example.test/predecessor',
        'data-rich-text-generated-autolink':''
      } : {href:options.linkHref || 'https://example.test/predecessor'},
        options.linkText || 'https://example.test/predecessor'),
      el('span', {}, options.composerSuffix || '')
    ] : [];
    const editor = el('div', {id:'prompt-textarea', contenteditable:'true'},
      options.composerLink ? options.composerPrefix || '' : options.composerText || '', body);
    editor.isContentEditable = true;
    if (options.unknownMark) editor.children.push(el('strong'));
    editor.focus = () => {if (options.focusDrift) editor.text += ' changed';};
    editor.parentElement = document;
    document.children.push(editor);
  }
}
let mutations = 0;
const commands = [];
if (options.normalizationTrace) {
  let selected;
  document.createRange = () => ({selectNodeContents:editor => {selected = editor;}});
  window.getSelection = () => ({removeAllRanges:() => {}, addRange:() => {}, collapseToEnd:() => {}});
  document.execCommand = (command, ui, value) => {
    commands.push(command);
    if (!['unlink','insertText'].includes(command) || ui !== false || !selected) throw Error('unexpected edit');
    if (options.unlinkUnsupported) return false;
    if (command === 'insertText') {
      if (typeof value !== 'string') throw Error('plain text required');
      selected.text = value; selected.children = []; mutations++;
    } else {
      if (value !== null) throw Error('unlink has no text');
      for (const a of selected.querySelectorAll('a[href]')) {
        a.tagName = 'SPAN'; delete a.attrs.href; mutations++;
      }
    }
    if (options.unlinkTextDrift) selected.text += ' changed';
    return true;
  };
}
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
const editor = document.querySelector('#prompt-textarea');
process.stdout.write(JSON.stringify(options.normalizationTrace ? {
  value, mutations, commands, editorText:editor?.textContent || '', linked:Boolean(editor?.querySelector('a[href], [text-link-href]'))
} : (options.traceMutations || options.traceClicks) ? {value, mutations} : value));
"""


class CurrentDom:
    def __init__(self, **options):
        self.options = options

    def get_current_url(self):
        return self.options.get("metadataLocation", self.options.get("location", "https://chatgpt.com/c/current-dom"))

    def get_attribute(self, selector, attribute, timeout):
        return self.evaluate(f"document.querySelector({json.dumps(selector)})?.getAttribute({json.dumps(attribute)}) ?? null")

    def get_text(self, selector, timeout):
        return self.evaluate(f"document.querySelector({json.dumps(selector)})?.innerText || ''")

    def is_element_visible(self, selector):
        return self.evaluate(f"Boolean(document.querySelector({json.dumps(selector)}))")

    def evaluate(self, script):
        result = subprocess.run(
            [shutil.which("node"), "-e", DOM, json.dumps(self.options, ensure_ascii=False)],
            input=script, text=True, encoding="utf-8", capture_output=True, timeout=10,
        )
        if result.returncode:
            raise AssertionError(result.stderr)
        observed = json.loads(result.stdout)
        if self.options.get("normalizationTrace"):
            self.mutations = getattr(self, "mutations", 0) + observed["mutations"]
            self.commands = getattr(self, "commands", []) + observed["commands"]
            if not observed["linked"]:
                self.options.update(composerLink=False, composerText=observed["editorText"])
            return observed["value"]
        if self.options.get("traceMutations") or self.options.get("traceClicks"):
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

    def test_typing_confirmation_reads_text_data_and_native_attribute_signature(self):
        class NativeSignatureDom(CurrentDom):
            # Match the installed SeleniumBase signature: no timeout argument.
            def get_attribute(self, selector, attribute):
                return self.evaluate(f"document.querySelector({json.dumps(selector)})?.getAttribute({json.dumps(attribute)}) ?? null")
            def get_text(self, selector, timeout):
                # SeleniumBase's text_all separates adjacent DOM text nodes.
                return self.evaluate(f"Array.from(document.querySelector({json.dumps(selector)}).childNodes).map(n => n.nodeValue ?? n.textContent ?? '').join(' ')")
        expected = 'Review https://github.com/vnath99/CatDesk now'
        cdp = NativeSignatureDom(composerCount=1, composerLink=True,
            composerPrefix='Review https://github.com/vnath99/', linkText='CatDesk',
            composerSuffix=' now', normalizationTrace=True)
        legacy, available = self.sink.legacy_composer_observation(cdp)
        self.assertTrue(available)
        self.assertNotEqual(bridge.normalize_message(legacy), expected)
        self.assertEqual(self.sink.text(cdp), expected)
        self.assertEqual(cdp.mutations, 0)
        self.assertEqual(cdp.commands, [])

    def test_reduced_native_attribute_reader_and_ambiguous_dom_fail_closed(self):
        class Reduced:
            def is_element_visible(self, selector): return selector == bridge.EDITOR
            def get_attribute(self, selector, attribute): return 'native value'
            def get_text(self, selector, timeout): raise AssertionError('attribute already read')
        self.assertEqual(self.sink.text(Reduced()), 'native value')
        with self.assertRaises(bridge.Attention):
            self.sink.text(CurrentDom(composerCount=2, composerText='foreign draft'))

    def test_bootstrap_serialization_probe_reads_only_one_plain_home_composer(self):
        home = bridge.NEW_CONVERSATION_HOME
        self.sink.check_plain_home_draft(CurrentDom(
            location=home, composerCount=1,
            composerText='{"predecessor":"https://example.test/predecessor"}'))
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_RICH_DRAFT_UNVERIFIED"):
            self.sink.check_plain_home_draft(CurrentDom(
                location=home, composerCount=1, composerLink=True))
        for count in (0, 2):
            with self.subTest(count=count), self.assertRaisesRegex(
                    bridge.Attention, "BOOTSTRAP_DRAFT_SERIALIZATION_UNVERIFIED"):
                self.sink.check_plain_home_draft(CurrentDom(location=home, composerCount=count))
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_HOME_DRIFT"):
            self.sink.check_plain_home_draft(CurrentDom(composerCount=1))

    def test_owned_json_url_unlinks_without_changing_text_and_is_idempotent(self):
        expected = '{"predecessor":"https://example.test/predecessor"}'
        cdp = CurrentDom(location=bridge.NEW_CONVERSATION_HOME, composerCount=1,
                         composerLink=True, composerPrefix='{"predecessor":"',
                         composerSuffix='"}', normalizationTrace=True)
        self.assertTrue(self.sink.normalize_owned_home_draft(cdp, expected))
        self.assertEqual(cdp.options["composerText"], expected)
        self.assertEqual((cdp.commands, cdp.mutations), (["unlink"], 1))
        self.assertFalse(self.sink.normalize_owned_home_draft(cdp, expected))
        self.assertEqual((cdp.commands, cdp.mutations), (["unlink"], 1))

    def test_actual_generated_url_span_is_read_without_layout_and_flattened_natively(self):
        expected = '{"predecessor":"https://example.test/predecessor"}'
        cdp = CurrentDom(location=bridge.NEW_CONVERSATION_HOME, composerCount=1,
                         composerLink=True, generatedLink=True, composerPrefix='{"predecessor":"',
                         composerSuffix='"}', normalizationTrace=True)
        self.assertNotEqual(cdp.get_text('#prompt-textarea', 1), expected)
        self.assertEqual(self.sink.composer_plain_text(cdp), expected)
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_RICH_DRAFT_UNVERIFIED"):
            self.sink.check_plain_home_draft(cdp)
        self.assertTrue(self.sink.normalize_owned_home_draft(cdp, expected))
        self.assertEqual((cdp.commands, cdp.mutations), (["insertText"], 1))
        self.assertEqual(self.sink.composer_plain_text(cdp), expected)
        self.assertFalse(self.sink.normalize_owned_home_draft(cdp, expected))
        self.assertEqual(cdp.commands, ["insertText"])

    def test_owned_draft_unlink_refuses_foreign_text_concealed_links_and_focus_drift(self):
        expected = 'https://example.test/predecessor'
        for changed in ({"linkText":"foreign operator draft"},
                        {"linkHref":"https://example.test/other"},
                        {"focusDrift":True}, {"composerCount":2}, {"unknownMark":True}):
            with self.subTest(changed=changed):
                options = dict(location=bridge.NEW_CONVERSATION_HOME, composerCount=1,
                               composerLink=True, normalizationTrace=True)
                options.update(changed)
                cdp = CurrentDom(**options)
                with self.assertRaises(bridge.Attention):
                    self.sink.normalize_owned_home_draft(cdp, expected)
                self.assertEqual((getattr(cdp, "commands", []), getattr(cdp, "mutations", 0)), ([], 0))

    def test_unlink_failure_or_text_change_never_becomes_positive_normalization(self):
        expected = 'https://example.test/predecessor'
        for changed in ({"unlinkUnsupported":True}, {"unlinkTextDrift":True}):
            with self.subTest(changed=changed):
                options = dict(location=bridge.NEW_CONVERSATION_HOME, composerCount=1,
                               composerLink=True, normalizationTrace=True)
                options.update(changed)
                cdp = CurrentDom(**options)
                with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_DRAFT_NORMALIZATION_UNVERIFIED"):
                    self.sink.normalize_owned_home_draft(cdp, expected)
                self.assertEqual(cdp.commands, ["unlink"])

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

    def test_explicit_source_read_retains_review_larger_than_context_excerpt(self):
        answer = "R" * 4375
        cdp = CurrentDom(answer=answer)
        context = self.sink.visible_message_snapshot(cdp)
        self.assertFalse(context["messages"][-1]["textComplete"])
        observed = self.sink.visible_message_snapshot(cdp, max_message_bytes=8192)
        self.assertEqual(observed["messages"][-1]["text"], answer)
        self.assertTrue(observed["messages"][-1]["textComplete"])
        self.assertTrue(observed["messages"][-1]["revisionComplete"])
        self.assertEqual(observed["messages"][0]["text"], "exact wake message")
        payload = {"stage": "SOURCE_SNAPSHOT", "projectId": "alpha",
                   "sourceGeneration": 1, "sourceDigest": "a" * 64, "snapshot": observed}
        self.assertLessEqual(len(json.dumps(payload, separators=(",", ":")).encode()) + 1, 8192)

    def test_inline_user_link_is_read_as_one_complete_message(self):
        cdp = CurrentDom(userInlineLink=True, userLink="https://example.test/compare",
                         userPrefix="Review source at ", userSuffix=" without editing it.")
        expected = "Review source at https://example.test/compare without editing it."
        observed = self.sink.visible_message_snapshot(cdp, max_message_bytes=8192)
        self.assertEqual(observed["messages"][0]["text"], expected)
        self.assertTrue(observed["messages"][0]["textComplete"])
        self.assertEqual(self.sink.user_message_digests(cdp), [bridge.digest(expected)])

    def test_explicit_source_read_budget_stays_bounded(self):
        for limit in (0, 4095, 8193, True):
            with self.subTest(limit=limit), self.assertRaises(ValueError):
                self.sink.visible_message_snapshot(CurrentDom(), max_message_bytes=limit)
        observed = self.sink.visible_message_snapshot(CurrentDom(answer="R" * 8193), max_message_bytes=8192)
        self.assertFalse(observed["messages"][-1]["textComplete"])
        self.assertIsNone(observed["messages"][-1]["revisionHash"])
        unicode = self.sink.visible_message_snapshot(CurrentDom(answer="😀" * 2200), max_message_bytes=8192)
        self.assertFalse(unicode["messages"][-1]["textComplete"])
        self.assertLessEqual(len(unicode["messages"][-1]["text"].encode()), 8192)

    def test_current_user_receipt_and_following_assistant_anchor(self):
        self.assertEqual(self.sink.user_message_digests(CurrentDom()), [bridge.digest("exact wake message")])
        self.assertEqual(self.sink.trusted_turn_anchor_state(CurrentDom(), "exact wake message"), (1, 1))

    def test_turn_anchor_uses_canonical_user_body_not_control_or_container_text(self):
        cdp = CurrentDom(userParagraphs=True, userControlsInsideBody=True)
        message = "first paragraph\n\nsecond paragraph"
        self.assertEqual(self.sink.user_message_digests(cdp), [bridge.digest(bridge.normalize_message(message))])
        self.assertEqual(self.sink.trusted_turn_anchor_state(cdp, message), (1, 1))

    def test_turn_anchor_requires_whole_user_body_and_never_a_control_label(self):
        for message in ("wake message", "exact wake message Copy message", "Copy message"):
            with self.subTest(message=message):
                self.assertEqual(self.sink.trusted_turn_anchor_state(CurrentDom(), message), (0, 0))

    def test_sent_review_pair_preserves_user_beside_large_complete_answer(self):
        cdp = CurrentDom(answer="r" * 16000, historicalLoadError=True)
        observed = self.sink.visible_message_snapshot(cdp, max_message_bytes=32768, review_pair=True)
        self.assertEqual([item["role"] for item in observed["messages"]], ["USER", "ASSISTANT"])
        self.assertEqual(observed["messages"][0]["text"], "exact wake message")
        self.assertEqual(observed["messages"][1]["text"], "r" * 16000)
        self.assertTrue(all(item["textComplete"] for item in observed["messages"]))
        ordinary = self.sink.visible_message_snapshot(cdp, max_message_bytes=8192)
        self.assertLessEqual(sum(len(item["text"].encode()) for item in ordinary["messages"]), 8192)

    def test_sent_review_pair_caps_unicode_without_discarding_user(self):
        observed = self.sink.visible_message_snapshot(CurrentDom(answer="😀" * 10000),
            max_message_bytes=32768, review_pair=True)
        self.assertEqual([item["role"] for item in observed["messages"]], ["USER", "ASSISTANT"])
        self.assertTrue(observed["messages"][0]["textComplete"])
        self.assertFalse(observed["messages"][1]["textComplete"])
        self.assertLessEqual(len(observed["messages"][1]["text"].encode()), 32768)
        with self.assertRaises(ValueError):
            self.sink.visible_message_snapshot(CurrentDom(), max_message_bytes=32768)

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

    def test_user_body_readers_exclude_controls_even_when_unit_is_the_body(self):
        for options in [dict(userControlsInsideBody=True), dict(userBodyIsUnit=True)]:
            with self.subTest(options=options):
                cdp = CurrentDom(**options)
                self.assertEqual(self.sink.user_message_digests(cdp), [bridge.digest('exact wake message')])
                message = self.sink.visible_message_snapshot(cdp)['messages'][0]
                self.assertEqual(message['text'], 'exact wake message')
                self.assertNotIn('PRIVATE_CONTROL_LABEL', message['text'])
                self.assertEqual(self.sink.observe_cooperative_response(
                    CurrentDom(observationEditor=True, **options), 'exact wake message',
                    bridge.digest('exact wake message')), 'RESPONSE_COMPLETE')
        literal = CurrentDom(userText='Copy message', userControlsInsideBody=True)
        self.assertEqual(self.sink.user_message_digests(literal), [bridge.digest('Copy message')])
        with self.assertRaisesRegex(bridge.Attention, 'RESPONSE_EXPECTED_USER_NOT_FOUND'), \
                patch.object(self.sink, 'response_snapshot', side_effect=AssertionError('must refuse before assistant')):
            self.sink.observe_cooperative_response(CurrentDom(userText='unrelated',
                userControlsInsideBody=True, observationEditor=True), 'PRIVATE_CONTROL_LABEL',
                bridge.digest('PRIVATE_CONTROL_LABEL'))

    def test_user_paragraph_boundaries_match_receipts_and_private_snapshots(self):
        cdp = CurrentDom(userParagraphs=True)
        expected = bridge.digest(bridge.normalize_message('first paragraph\n\nsecond paragraph'))
        self.assertEqual(self.sink.user_message_digests(cdp), [expected])
        snapshot = self.sink.visible_message_snapshot(cdp)['messages'][0]
        self.assertEqual(bridge.digest(bridge.normalize_message(snapshot['text'])), expected)

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

    def test_assistant_copy_message_action_qualifies_only_exact_latest_assistant(self):
        for legacy in [False, True]:
            with self.subTest(legacy=legacy):
                cdp = CurrentDom(assistantCopyLabel='Copy message', legacyTurns=legacy)
                self.assertEqual(self.sink.response_snapshot(cdp, 'exact wake message',
                    require_usable_body=True), (0, 0, True, True))

    def test_copy_message_from_user_body_auxiliary_or_ambiguous_toolbar_cannot_complete(self):
        for options in [dict(toolbar=False, userCopy='Copy message'),
                dict(toolbar=False, bodyCopyLabel='Copy message'),
                dict(toolbar=False, auxiliaryUnit='after', auxiliaryCopyLabel='Copy message'),
                dict(assistantCopyLabel='Copy message', duplicateToolbar=True),
                dict(assistantCopyLabel='Copy message', extraAssistant=True),
                dict(assistantCopyLabel='Copy message elsewhere')]:
            with self.subTest(options=options):
                self.assertFalse(self.sink.response_snapshot(CurrentDom(**options),
                    'exact wake message', require_usable_body=True)[3])

    def test_copy_message_never_completes_controls_only_or_failed_assistant(self):
        self.assertFalse(self.sink.response_snapshot(CurrentDom(assistantCopyLabel='Copy message',
            controlsOnly=True), 'exact wake message', require_usable_body=True)[3])
        with self.assertRaisesRegex(bridge.PostSubmitUnknown, 'RESPONSE_LOAD_ERROR'):
            self.sink.response_snapshot(CurrentDom(assistantCopyLabel='Copy message', loadError=True),
                'exact wake message', require_usable_body=True)

    def test_passive_completion_diagnostics_distinguish_body_and_toolbar_without_text(self):
        for options, reason in [({}, "COMPLETE"),
                ({"controlsOnly": True}, "ASSISTANT_BODY_UNPROVEN"),
                ({"toolbar": False}, "ASSISTANT_COMPLETION_ACTION_UNPROVEN"),
                ({"duplicateToolbar": True}, "ASSISTANT_COMPLETION_ACTION_UNPROVEN")]:
            with self.subTest(options=options):
                cdp = CurrentDom(observationEditor=True, answer="PRIVATE_BODY_MARKER", **options)
                stage = self.sink.observe_cooperative_response(cdp, "exact wake message",
                    bridge.digest("exact wake message"))
                self.assertEqual(stage, "RESPONSE_COMPLETE" if reason == "COMPLETE" else "RESPONSE_PENDING")
                self.assertEqual(self.sink.response_observation_reason, reason)
                self.assertNotIn("PRIVATE_BODY_MARKER", reason)

    def test_passive_diagnostics_identify_pending_layer_and_reset_on_refusal(self):
        for options, reason in [({"documentState": "loading"}, "DOCUMENT_LOADING"),
                ({"observationStop": True}, "GENERATION_ACTIVE"),
                ({"observationEditor": False}, "EDITOR_NOT_READY")]:
            with self.subTest(options=options):
                cdp = CurrentDom(**({"observationEditor": True} | options))
                self.assertEqual(self.sink.observe_cooperative_response(cdp, "exact wake message",
                    bridge.digest("exact wake message")), "RESPONSE_PENDING")
                self.assertEqual(self.sink.response_observation_reason, reason)
        with self.assertRaises(bridge.Attention):
            self.sink.observe_cooperative_response(CurrentDom(loadError=True, observationEditor=True),
                "exact wake message", bridge.digest("exact wake message"))
        self.assertIsNone(self.sink.response_observation_reason)

    def test_response_diagnostic_refuses_arbitrary_dom_data(self):
        class Malformed:
            def evaluate(self, script): return ["ready", 0, 0, True, False, "PRIVATE_PAGE_TEXT"]
        with self.assertRaisesRegex(bridge.PostSubmitUnknown, "RESPONSE_STATE_QUERY_FAILED"):
            self.sink.response_snapshot(Malformed(), "exact wake message", require_usable_body=True,
                diagnostic={})

    def test_user_and_code_block_copy_cannot_complete_response(self):
        self.assertEqual(self.sink.response_snapshot(CurrentDom(toolbar=False, userCopy="Copy"),
                         "exact wake message", require_usable_body=True), (0, 0, True, False))

    def test_completion_control_shape_is_closed_content_free_and_advisory(self):
        for options, complete, shared in [({}, True, 1), ({"toolbar":False}, False, 0),
                ({"duplicateToolbar":True}, False, 2)]:
            with self.subTest(options=options):
                diagnostic = {}
                result = self.sink.response_snapshot(CurrentDom(answer="PRIVATE_ANSWER_MARKER", **options),
                    "exact wake message", require_usable_body=True, diagnostic=diagnostic)
                controls = diagnostic["controls"]
                self.assertTrue(bridge.valid_response_control_diagnostics(controls))
                self.assertEqual(controls["semanticAssistantUnits"], 1)
                self.assertEqual(controls["knownCopyInBody"], 1)
                self.assertEqual(controls["sharedCopyAfterAssistant"], shared)
                self.assertEqual(controls["assistantBodyPresent"], True)
                self.assertEqual(controls["completionActionFound"], complete)
                self.assertEqual(result[3], complete)
                self.assertNotIn("PRIVATE_", json.dumps(diagnostic))
                self.assertLess(len(json.dumps(diagnostic)), 1500)
        self.sink.observe_cooperative_response(CurrentDom(observationEditor=True),
            "exact wake message", bridge.digest("exact wake message"))
        self.assertIsNotNone(self.sink.response_observation_controls)
        self.sink.observe_cooperative_response(CurrentDom(observationEditor=True, observationStop=True),
            "exact wake message", bridge.digest("exact wake message"))
        self.assertIsNone(self.sink.response_observation_controls)
        diagnostic = {}
        self.sink.response_snapshot(CurrentDom(diagnosticExtraButtons=100,
            diagnosticButtonLabel='PRIVATE_CONTROL_LABEL'), 'exact wake message',
            require_usable_body=True, diagnostic=diagnostic)
        self.assertEqual(diagnostic['controls']['buttonsInTurn'], 64)
        self.assertNotIn('PRIVATE_CONTROL_LABEL', json.dumps(diagnostic))

    def test_completion_control_shape_rejects_text_unknown_keys_types_and_unbounded_counts(self):
        diagnostic = {}
        self.sink.response_snapshot(CurrentDom(), "exact wake message", require_usable_body=True,
            diagnostic=diagnostic)
        valid = diagnostic["controls"]
        for field, bad in [("copyTextInTurn", "PRIVATE_LABEL"), ("copyTextInTurn", True),
                ("copyTextInTurn", -1), ("copyTextInTurn", 65), ("pairedTurn", 1),
                ("extra", "PRIVATE_DRAFT")]:
            with self.subTest(field=field, bad=bad):
                self.assertFalse(bridge.valid_response_control_diagnostics(valid | {field:bad}))

    def test_auxiliary_search_unit_is_neither_assistant_nor_completion_authority(self):
        for where in ['before', 'after']:
            with self.subTest(where=where):
                cdp = CurrentDom(auxiliaryUnit=where)
                self.assertEqual(self.sink.response_snapshot(cdp, "exact wake message",
                    require_usable_body=True), (0, 0, True, True))
        cdp = CurrentDom(auxiliaryUnit='after', userOnly=True, toolbar=False)
        self.assertEqual(self.sink.response_snapshot(cdp, "exact wake message",
            require_usable_body=True), (0, 0, False, False))

    def test_ambiguous_toolbar_or_assistant_cannot_complete_response(self):
        for options in ({"duplicateToolbar": True}, {"extraAssistant": True}):
            with self.subTest(options=options):
                self.assertFalse(self.sink.response_snapshot(CurrentDom(**options), "exact wake message",
                                                            require_usable_body=True)[3])

    def test_error_placeholder_and_controls_only_are_not_usable(self):
        with self.assertRaisesRegex(bridge.PostSubmitUnknown, "RESPONSE_LOAD_ERROR"):
            self.sink.response_snapshot(CurrentDom(loadError=True), "exact wake message", require_usable_body=True)
        with self.assertRaisesRegex(bridge.Attention, "BOOTSTRAP_SOURCE_LOAD_ERROR_PLACEHOLDER"):
            self.sink.require_usable_latest_assistant(CurrentDom(loadError=True), source=True)
        observed = self.sink.visible_message_snapshot(CurrentDom(controlsOnly=True))
        self.assertFalse(observed["messages"][-1]["textComplete"])
        self.assertFalse(self.sink.response_snapshot(CurrentDom(controlsOnly=True), "exact wake message",
                                                   require_usable_body=True)[3])

    def test_readonly_surface_separates_usable_conversation_and_unattributed_load_error(self):
        result = self.sink.browser_surface_observation(CurrentDom(loadError=True, observationEditor=True))
        self.assertEqual(result, dict(route="EXACT", document="COMPLETE", history="PRESENT",
            editor="READY", composer="EMPTY", generation="IDLE", assistant="LOAD_ERROR",
            exchange="UNATTRIBUTED", recovery="OBSERVATION_ONLY"))

    def test_readonly_surface_exports_no_draft_or_assistant_text(self):
        result = self.sink.browser_surface_observation(CurrentDom(observationEditor=True,
            observationDraft="PRIVATE_DRAFT_MARKER", answer="PRIVATE_ANSWER_MARKER"))
        self.assertEqual(result["composer"], "DRAFT_PRESENT")
        self.assertEqual(result["assistant"], "PRESENT")
        self.assertNotIn("PRIVATE_", json.dumps(result))

    def test_readonly_surface_retains_exact_conversation_route_equivalence(self):
        for path in ("/c/test", "/g/project/c/test"):
            sink = bridge.CdpSink("https://chatgpt.com" + path, self.sink.profile, 30, 20)
            for host in ("chatgpt.com", "chat.openai.com"):
                for observed in ("/c/test", "/g/other-project/c/test"):
                    self.assertEqual(sink.browser_surface_observation(
                        CurrentDom(location="https://" + host + observed))["route"], "EXACT")
            for observed in ("/c/other", "/g/project/c/other", "/c/test/", "/c/test?query=1"):
                self.assertEqual(sink.browser_surface_observation(
                    CurrentDom(location="https://chatgpt.com" + observed))["route"], "TARGET_DRIFT")

    def test_readonly_surface_distinguishes_layers_without_recovery(self):
        for options, key, expected in [
            ({"observationStop": True}, "generation", "ACTIVE"),
            ({"observationLogin": True}, "route", "AUTH_REQUIRED"),
            ({"observationCaptcha": True}, "route", "SECURITY_CHECK"),
            ({"observationNetwork": True}, "route", "NETWORK_ERROR"),
            ({"location": "chrome-error://chromewebdata/"}, "route", "NETWORK_ERROR"),
            ({"location": "https://chatgpt.com/"}, "route", "HOME"),
            ({"location": "https://chatgpt.com/c/other"}, "route", "TARGET_DRIFT"),
            ({"documentState": "loading"}, "document", "LOADING"),
            ({}, "editor", "MISSING"),
        ]:
            with self.subTest(options=options):
                result = self.sink.browser_surface_observation(CurrentDom(**options))
                self.assertEqual(result[key], expected)
                self.assertEqual(result["recovery"], "OBSERVATION_ONLY")

    def test_historical_load_error_does_not_replace_current_completed_turn(self):
        cdp = CurrentDom(historicalLoadError=True)
        self.assertEqual(self.sink.browser_surface_observation(cdp)["assistant"], "PRESENT")
        self.assertTrue(self.sink.response_snapshot(cdp, "exact wake message", require_usable_body=True)[3])

    def test_answer_discussing_response_error_is_not_the_error_placeholder(self):
        cdp = CurrentDom(answer="The UI may say This response couldn’t load. Preserve its receipt before recovery.", observationEditor=True)
        self.assertEqual(self.sink.browser_surface_observation(cdp)["assistant"], "PRESENT")
        self.assertTrue(self.sink.response_snapshot(cdp, "exact wake message", require_usable_body=True)[3])
        self.sink.require_usable_latest_assistant(cdp, source=True)

    def test_surface_uses_document_route_not_stale_driver_metadata(self):
        cdp = CurrentDom(location="https://chatgpt.com/c/foreign", metadataLocation=self.sink.url,
                         observationEditor=True)
        self.assertEqual(self.sink.browser_surface_observation(cdp)["route"], "TARGET_DRIFT")

    def test_unique_assistant_timeout_retry_is_scoped_and_not_a_user_submit(self):
        for legacy in [False, True]:
            with self.subTest(legacy=legacy):
                cdp = CurrentDom(timeoutCard=True, observationEditor=True, traceClicks=True, legacyTurns=legacy)
                self.assertEqual(self.sink.response_snapshot(cdp, "exact wake message")[:2], (1, 1))
                self.sink.click_response_timeout_retry(cdp, "exact wake message")
                self.assertEqual(cdp.mutations, 1)

    def test_response_and_retry_require_whole_canonical_user_body(self):
        for legacy in [False, True]:
            for message in ["", "exact wake", "Copy message", "A useful answer"]:
                with self.subTest(legacy=legacy, message=message):
                    cdp = CurrentDom(timeoutCard=True, observationEditor=True,
                                     traceClicks=True, legacyTurns=legacy)
                    with self.assertRaises(bridge.PostSubmitUnknown):
                        self.sink.response_snapshot(cdp, message)
                    with self.assertRaisesRegex(bridge.PostSubmitUnknown,
                                                "RESPONSE_RETRY_CONTROL_AMBIGUOUS"):
                        self.sink.click_response_timeout_retry(cdp, message)
                    self.assertEqual(getattr(cdp, "mutations", 0), 0)

    def test_response_and_retry_share_paragraph_and_control_free_user_identity(self):
        for legacy in [False, True]:
            with self.subTest(legacy=legacy):
                cdp = CurrentDom(timeoutCard=True, observationEditor=True,
                                 traceClicks=True, legacyTurns=legacy, userParagraphs=True,
                                 userControlsInsideBody=True)
                message = "first paragraph\nsecond paragraph"
                self.assertEqual(self.sink.response_snapshot(cdp, message)[:2], (1, 1))
                self.sink.click_response_timeout_retry(cdp, message)
                self.assertEqual(cdp.mutations, 1)

    def test_duplicate_user_inside_one_exchange_never_authorizes_retry(self):
        cdp = CurrentDom(timeoutCard=True, observationEditor=True,
                         traceClicks=True, duplicateUser=True)
        with self.assertRaises(bridge.PostSubmitUnknown):
            self.sink.response_snapshot(cdp, "exact wake message")
        with self.assertRaisesRegex(bridge.PostSubmitUnknown,
                                    "RESPONSE_RETRY_CONTROL_AMBIGUOUS"):
            self.sink.click_response_timeout_retry(cdp, "exact wake message")
        self.assertEqual(getattr(cdp, "mutations", 0), 0)

    def test_current_receipt_attribution_distinguishes_failed_history_and_duplicates(self):
        for options, expected in [
            ({"historicalLoadError": True}, "RESPONSE_COMPLETE"),
            ({"loadError": True}, "RESPONSE_LOAD_ERROR"),
            ({"loadError": True, "observationStop": True}, "RESPONSE_PENDING"),
            ({"loadError": True, "duplicateUser": True}, "RESPONSE_EXPECTED_USER_DUPLICATED"),
        ]:
            with self.subTest(options=options):
                cdp = CurrentDom(observationEditor=True, **options)
                if expected in {"RESPONSE_COMPLETE", "RESPONSE_PENDING"}:
                    self.assertEqual(self.sink.observe_cooperative_response(cdp, "exact wake message",
                        bridge.digest("exact wake message")), expected)
                else:
                    with self.assertRaisesRegex(bridge.Attention, expected):
                        self.sink.observe_cooperative_response(cdp, "exact wake message",
                            bridge.digest("exact wake message"))

    def test_retry_refuses_user_quotes_foreign_controls_drafts_and_route_drift(self):
        for options, reason in [
            ({"userTimeoutQuote": True}, "RESPONSE_RETRY_CONTROL_AMBIGUOUS"),
            ({"timeoutCard": True, "retryOnlyOutside": True}, "RESPONSE_RETRY_CONTROL_AMBIGUOUS"),
            ({"timeoutCard": True, "retryCount": 2}, "RESPONSE_RETRY_CONTROL_AMBIGUOUS"),
            ({"timeoutCard": True, "retryLabel": "Regenerate"}, "RESPONSE_RETRY_CONTROL_AMBIGUOUS"),
            ({"timeoutCard": True, "observationDraft": "PRIVATE_DRAFT"}, "RESPONSE_RETRY_BLOCKED_DRAFT"),
            ({"timeoutCard": True, "observationStop": True}, "RESPONSE_RETRY_BLOCKED_GENERATION"),
            ({"timeoutCard": True, "location": "https://chatgpt.com/c/foreign", "metadataLocation": self.sink.url}, "RESPONSE_TARGET_DRIFT"),
        ]:
            with self.subTest(options=options):
                cdp = CurrentDom(observationEditor=True, traceClicks=True, **options)
                with self.assertRaisesRegex(bridge.PostSubmitUnknown, reason):
                    self.sink.click_response_timeout_retry(cdp, "exact wake message")
                self.assertEqual(getattr(cdp, "mutations", 0), 0)

    def test_user_timeout_quote_is_not_an_assistant_timeout_card(self):
        cdp = CurrentDom(userTimeoutQuote=True)
        self.assertEqual(self.sink.response_snapshot(cdp, "exact wake message")[:2], (0, 0))


if __name__ == "__main__":
    unittest.main()
