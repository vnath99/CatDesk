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
const inputFrame = JSON.parse(fs.readFileSync(0, 'utf8'));
const options = inputFrame.options;
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
  getClientRects() { for(let node=this;node;node=node.parentElement) if(node.attrs.hidden) return []; return [1]; }
  matches(selector) { return selector.split(',').some(raw => {
    const s = raw.trim();
    if(s===':popover-open') return this.attrs.popoverOpen===true;
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
if (options.userNoDedicatedBody) {
  user.children[0].children[0].attrs = {};
  if (options.userPlainSuffix) append(user.children[0], el('span', {}, options.userPlainSuffix));
}
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
for (const body of options.additionalAnswerBodies || [])
  append(assistant, el('div', {class:'markdown'}, body));
if (options.timeoutCard) append(assistant, el('div', {role:'alert'}, '', [
  el('p', {}, 'Message delivery timed out. Please try again.'),
  ...Array.from({length:options.retryOnlyOutside ? 0 : (options.retryCount ?? 1)}, () => button(options.retryLabel || 'Retry')),
]));
if (options.userTimeoutQuote) {
  append(user.children[0], el('p', {}, 'Message delivery timed out. Please try again.'));
  append(user.children[0], button('Retry'));
}
const children = options.userOnly ? [user] : [user, assistant];
for (const [index, trace] of (options.traceControls || []).entries()) {
  const id='PRIVATE_TRACE_PANEL_'+index;
  const control=el(trace.summary ? 'summary' : 'button', trace.summary ? {} : {
    ...(options.traceInteractive ? {type:trace.type || 'button'} : {}),
    ...(trace.expanded===undefined ? {} : {'aria-expanded':String(trace.expanded)}),
    ...(trace.linked ? {'aria-controls':id} : {})
  }, trace.label || 'Worked for 46s');
  const panel=el('div',{id,hidden:trace.expanded!==true},'PRIVATE_TRACE_ARGUMENTS');
  if(options.traceInteractive) {
    panel.text=options.tracePanelText || 'run_command INVALID_ARGUMENT WEB_TASK_CACHED_ARGUMENTS_INVALID CWD_OVERRIDE_FORBIDDEN PRIVATE_TRACE_VALUE';
    if(options.traceJson)append(panel,el('pre',{},options.traceJson));
  }
  let node=trace.summary ? el('details',trace.expanded ? {open:''} : {},'', [control,panel]) : control;
  if(options.traceInteractive && !trace.summary) {
    const wrapper=el('section',{},'', options.traceRolePanel ? [control] : [control,panel]);node=wrapper;
    control.click=()=>{
      mutations++;control.attrs['aria-expanded']='true';panel.attrs.hidden=false;
      if(options.traceLoading) {
        const finalText=panel.text;panel.text='Loading details…';panel.attrs['aria-busy']='true';
        if(options.traceLoading==='delayed')setTimeout(()=>{panel.text=finalText;panel.attrs['aria-busy']='false';},20);
      }
      if(options.traceRolePanel){panel.attrs.role='complementary';append(document,panel);}
      if(options.traceClickThrows)throw Error('test click outcome unknown');
      if(options.traceChangedControl)append(wrapper,el('button',{type:'button','aria-expanded':'true'},'Worked for 20s'));
      if(options.traceAmbiguousPanels)append(wrapper,el('div',{},'PRIVATE_SECOND_PANEL'));
    };
  }
  if(options.traceInteractive && trace.summary)control.click=()=>{mutations++;node.attrs.open='';panel.attrs.hidden=false;};
  if(trace.placement==='user')append(user.children[0],node);
  else if(trace.placement==='code')append(assistant.children[1].children[0],el('pre',{},'',[node]));
  else children.splice(1,0,node);
  if(!trace.summary && trace.linked && !options.traceInteractive)children.splice(1,0,panel);
}
if (options.unmountedLoadError) {
  for (let i = 0; i < (options.unmountedErrorCount || 1); i++) {
    const card = el('div', {}, '', [el('p', {}, 'This response couldn’t load')]);
    if (options.unmountedPlacement === 'user') append(user.children[0], card);
    else if (options.unmountedPlacement === 'code') children.push(el('pre', {}, '', [card]));
    else if (options.unmountedPlacement === 'tool') children.push(el('div', {'data-testid':'mcp-tool-result'}, '', [card]));
    else if (options.unmountedPlacement === 'unit') children.push(el('div', {'data-content-search-unit-key':'unknown-tool'}, '', [card]));
    else if (options.unmountedPlacement === 'quote') children.push(el('blockquote', {}, '', [card]));
    else if (options.unmountedPlacement === 'control') children.push(el('button', {}, '', [card]));
    else if (options.unmountedPlacement === 'before') children.unshift(card);
    else children.push(card);
  }
}
for (let i = 0; i < (options.toolMarkerCount || 0); i++) {
  const card = el('div', {'data-testid':'mcp-tool-call', 'data-tool-call-id':'PRIVATE_CALL_ID',
    'aria-busy':options.toolBusy ? 'true' : 'false', hidden:options.toolHidden || false},
    'PRIVATE_TOOL_ARGUMENTS', options.toolError ? [el('div', {role:'alert'}, 'PRIVATE_POLICY_TEXT')] : []);
  if (options.toolPlacement === 'user') append(user.children[0], card);
  else if (options.toolPlacement === 'code') append(assistant.children[1].children[0], el('pre', {}, '', [card]));
  else children.splice(1,0,card);
}
if (options.toolResult) children.splice(1,0,el('div', {'data-testid':'mcp-tool-result'}, 'PRIVATE_TOOL_RESULT'));
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
if(options.laterExchange) {
  const later=paired.cloneNode(true);
  later.attrs['data-content-search-turn-key']='fallback-turn-1';
  const laterUser=later.querySelector('[data-user-message-bubble]');
  const laterBody=laterUser.querySelector('[data-message-content], [data-markdown-text-tone="user-message"], .whitespace-pre-wrap');
  (laterBody||laterUser).text=options.laterSameUser?'exact wake message':'foreign later request';
  if(!options.laterSameUser)(laterBody||laterUser).children=[];
  const laterAssistant=later.querySelector('.markdown');
  if(laterAssistant)laterAssistant.text='PRIVATE_LATER_RESPONSE HTTP 429 WEB_TASK_NATIVE_ARGUMENTS_INVALID';
  append(document,later);
}
if (options.retryOnlyOutside) document.children.push(button('Retry'));
if (options.historicalLoadError) document.children.unshift(el('div', {'data-message-author-role':'assistant'}, 'This response couldn’t load'));
if (options.historicalToolMarker) document.children.unshift(el('div', {'data-testid':'mcp-tool-call'}, 'PRIVATE_OLD_TOOL'));
document.readyState = options.documentState || 'complete';
if (options.observationEditor) document.children.push(el('div', {id:'prompt-textarea'}, options.observationDraft || ''));
if (options.observationStop) document.children.push(el('button', {'data-testid':'stop-button'}));
if (options.observationLogin) document.children.push(el('a', {href:'/auth/login'}));
if (options.observationCaptcha) document.children.push(el('iframe', {title:'captcha'}));
if (options.observationNetwork) document.children.push(el('div', {id:'main-frame-error'}), el('div', {id:'error-code'}));
let appMenuControl=null,appPanel=null;
if (options.appForm) {
  const editor=el('div',{id:'prompt-textarea',contenteditable:'true'},options.appDraft || '');
  editor.isContentEditable=true;
  const controls=Array.from({length:options.appPlusCount ?? 1},()=>el('button',{
    'data-testid':'composer-plus-btn',type:options.appPlusType || 'button',
    'aria-expanded':options.appExpanded || 'false',disabled:options.appDisabled || false},'PRIVATE_CONTROL'));
  for(const control of controls){
    if(options.appPopupMetadata!==undefined) control.attrs['aria-haspopup']=options.appPopupMetadata;
    if(options.appControlsMetadata!==undefined) control.attrs['aria-controls']=options.appControlsMetadata;
  }
  if(options.appMenuTrace){
    const menu=el('div',{role:options.appPopupRole || 'menu',id:'owned-app-menu',hidden:options.appExpanded!=='true'},'',[
      el('div',{role:'menuitem','aria-disabled':options.v5Disabled?'true':'false'},'CatDesk-Local-Tunnel-v5'),
      el('div',{role:'menuitem'},'PRIVATE_OTHER_APP'),el('div',{role:'menuitem'},'Apps'),
    ]);
    appPanel=menu;
    if(options.appHtmlPopover){menu.attrs.popover='auto';menu.attrs.popoverOpen=options.appExpanded==='true';}
    if(options.appKnownPortal) menu.attrs['data-radix-popper-content-wrapper']='';
    if(options.appPanelIsHistory) menu.attrs['data-message-author-role']='assistant';
    if(!options.appInlinePanel) append(document,menu);
    document.getElementById=id=>document.querySelectorAll('[id]').find(node=>node.getAttribute('id')===id)||null;
    for(const control of controls){
      control.attrs['aria-haspopup']=options.appPopup || 'menu';
      control.attrs['aria-controls']=options.menuLinkMismatch?'foreign-menu':'owned-app-menu';
      if(options.unlinkedMenuControl){delete control.attrs['aria-haspopup'];delete control.attrs['aria-controls'];}
      control.click=()=>{mutations++;control.attrs['aria-expanded']=control.attrs['aria-expanded']==='true'?'false':'true';menu.attrs.hidden=control.attrs['aria-expanded']!=='true';};
    }
    appMenuControl=controls[0]||null;
    if(options.priorMenuOpen) append(document,el('div',{role:'menu',id:'foreign-menu'},'PRIVATE_MENU'));
    if(options.priorDialogOpen) append(document,el('div',{role:'dialog'},'PRIVATE_DIALOG'));
    if(options.ambiguousMenuAfterOpen&&options.appExpanded==='true') append(document,el('div',{role:'menu'},'PRIVATE_MENU'));
    if(options.ambiguousPortalAfterOpen&&options.appExpanded==='true') append(document,el('div',{'data-floating-ui-portal':''},'',[el('button',{},'PRIVATE_OTHER_PANEL')]));
  }
  if(options.appSemanticLabel) for(const control of controls){delete control.attrs['data-testid'];control.attrs['aria-label']=options.appSemanticLabel;}
  if(options.v5Chip) controls.push(el('button',{'data-testid':'app-chip','aria-label':'CatDesk-Local-Tunnel-v5'}));
  if(options.foreignChip) controls.push(el('button',{'data-testid':'app-chip','aria-label':'PRIVATE_OTHER_APP'}));
  const form=el('form',{},'',[editor,...(options.appControlOutsideForm?[]:controls)]);
  if(options.appInlinePanel&&appPanel) append(form,appPanel);
  if(options.appFileInput){
    const input=el('input',{type:'file'});
    input.files=options.appUnknownFileList?null:{length:options.appSelectedFile?1:0};
    input.value=options.appSelectedFile?'PRIVATE_FILENAME':'';
    append(form,input);
  }
  if(options.appHiddenInput) append(form,el('input',{type:'hidden'}));
  if(options.appUnknownInput) append(form,el('input',{type:'text'}));
  if(options.appButtonIcon) append(controls[0],el('img'));
  if(options.appUnownedImage) append(form,el('img'));
  if(options.appAttachment) append(form,el('div',{'data-testid':'attachment-preview'},'PRIVATE_FILE'));
  const container=options.appControlOutsideForm?el('div',{},'',[form,...controls]):form;
  if(options.appHistoryContainer) append(container,el('div',{'data-message-author-role':'user'},'PRIVATE_HISTORY'));
  append(document,container);
  if(options.duplicateAppEditor) append(document,el('div',{id:'prompt-textarea'},'PRIVATE_DRAFT'));
  if(options.outsideV5) append(document,el('button',{'data-testid':'app-chip','aria-label':'CatDesk-Local-Tunnel-v5'}));
}
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
const value = eval(inputFrame.script);
const editor = document.querySelector('#prompt-textarea');
process.stdout.write(JSON.stringify(options.appMenuTrace ? {value,mutations,expanded:appMenuControl?.getAttribute('aria-expanded')||'false'} : options.normalizationTrace ? {
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
            [shutil.which("node"), "-e", DOM],
            input=json.dumps({"options":self.options,"script":script},ensure_ascii=False),
            text=True, encoding="utf-8", capture_output=True, timeout=10,
        )
        if result.returncode:
            raise AssertionError(result.stderr)
        observed = json.loads(result.stdout)
        if self.options.get('appMenuTrace'):
            self.options['appExpanded']=observed['expanded']
            self.mutations=getattr(self,'mutations',0)+observed['mutations']
            return observed['value']
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


class PersistentCurrentDom(CurrentDom):
    """Run the actual probes against one DOM across transaction steps."""
    def __init__(self,**options):
        super().__init__(**options)
        prefix=DOM.split('const value = eval(inputFrame.script);')[0]
        prefix=prefix.replace("const inputFrame = JSON.parse(fs.readFileSync(0, 'utf8'));",
            'const inputFrame = '+json.dumps({'options':options,'script':''})+';')
        server=prefix+'''
const reader=require('readline').createInterface({input:process.stdin});
reader.on('line', line=>{try {const value=eval(JSON.parse(line));
 process.stdout.write(JSON.stringify({value,mutations})+'\\n');
}catch(error){process.stdout.write(JSON.stringify({error:String(error)})+'\\n');}});
'''
        self.fixture_directory=tempfile.TemporaryDirectory(prefix='catdesk-trace-dom-')
        directory=Path(self.fixture_directory.name).resolve()
        assert directory.parent==Path(tempfile.gettempdir()).resolve()
        program=directory/'persistent-dom.js';program.write_text(server,encoding='utf-8')
        try:
            self.process=subprocess.Popen([shutil.which('node'),str(program)],stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,encoding='utf-8')
        except BaseException:self.fixture_directory.cleanup();raise

    def evaluate(self,script):
        self.process.stdin.write(json.dumps(script)+'\n');self.process.stdin.flush()
        line=self.process.stdout.readline()
        if not line:raise AssertionError('persistent DOM exited')
        observed=json.loads(line)
        if 'error' in observed:raise AssertionError(observed['error'])
        self.mutations=observed['mutations'];return observed['value']

    def close(self):
        self.process.stdin.close()
        try:self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:self.process.kill();self.process.wait(timeout=3)
        self.process.stdout.close();self.process.stderr.close()
        self.fixture_directory.cleanup()


@unittest.skipUnless(shutil.which("node"), "Node.js required for offline DOM probes")
class CurrentDomTests(unittest.TestCase):
    def test_historical_response_is_exact_bounded_and_cannot_change_latest_or_retry_rules(self):
        for legacy in [False,True]:
            with self.subTest(legacy=legacy):
                cdp=CurrentDom(observationEditor=True,laterExchange=True,legacyTurns=legacy,traceClicks=True,
                    answer='native.codex.dispatch returned WEB_TASK_CACHED_ARGUMENTS_INVALID.')
                with self.assertRaises(bridge.Attention):
                    self.sink.observe_cooperative_response(cdp,'exact wake message',bridge.digest('exact wake message'))
                self.assertEqual(self.sink.observe_cooperative_response(cdp,'exact wake message',bridge.digest('exact wake message'),historical=True),'RESPONSE_COMPLETE')
                diagnostic,tools,hints={},{},{}
                self.sink.response_snapshot(cdp,'exact wake message',require_usable_body=True,diagnostic=diagnostic,
                    tool_diagnostic=tools,execution_diagnostic=hints,historical=True)
                self.assertEqual(hints['knownCodes'],['WEB_TASK_CACHED_ARGUMENTS_INVALID'])
                self.assertEqual(hints['mentionedTools'],['NATIVE_DISPATCH'])
                self.assertNotIn('PRIVATE_',json.dumps(hints))
                with self.assertRaises(bridge.PostSubmitUnknown):self.sink.click_response_timeout_retry(cdp,'exact wake message')
                with self.assertRaises(ValueError):self.sink.response_snapshot(cdp,'exact wake message',require_usable_body=True,
                    diagnostic={},tool_diagnostic={},execution_diagnostic={},historical=True,trace_action=('OPEN','a'*32),trace_observation={})
                self.assertEqual(cdp.mutations,0)

    def test_historical_response_refuses_wrong_duplicate_expired_or_unfinished_exchange(self):
        for options,expected in [({'laterSameUser':True},'RESPONSE_EXPECTED_USER_DUPLICATED'),
                ({'userText':'foreign original'},None),({'observationLogin':True},'LOGIN_OR_PROFILE_REQUIRED'),
                ({'observationNetwork':True},'RESPONSE_NETWORK_ERROR')]:
            with self.subTest(options=options):
                cdp=CurrentDom(observationEditor=True,laterExchange=True,**options)
                with self.assertRaises(bridge.Attention) as failure:
                    self.sink.observe_cooperative_response(cdp,'exact wake message',bridge.digest('exact wake message'),historical=True)
                if expected:self.assertEqual(str(failure.exception),expected)
        for options in [dict(observationStop=True),dict(answer=''),dict(toolbar=False)]:
            self.assertEqual(self.sink.observe_cooperative_response(CurrentDom(observationEditor=True,laterExchange=True,**options),
                'exact wake message',bridge.digest('exact wake message'),historical=True),'RESPONSE_PENDING')

    def test_passive_composer_popup_metadata_is_closed_and_never_returns_control_ids(self):
        for popup,expected in [(None,'ABSENT'),('menu','MENU'),('dialog','DIALOG'),('true','TRUE'),('false','FALSE'),('PRIVATE_VALUE','UNKNOWN')]:
            with self.subTest(popup=popup):
                options={} if popup is None else dict(appPopupMetadata=popup)
                cdp=CurrentDom(appForm=True,appControlsMetadata='PRIVATE_CONTROL_ID',traceClicks=True,**options)
                value=self.sink.composer_app_observation(cdp)
                self.assertEqual(value['schemaVersion'],2)
                self.assertEqual(value['plusPopup'],expected)
                self.assertEqual(value['plusControlReference'],'PRESENT')
                self.assertNotIn('PRIVATE_',json.dumps(value))
                self.assertEqual(getattr(cdp,'mutations',0),0)
        self.assertEqual(self.sink.composer_app_observation(CurrentDom(appForm=True))['plusControlReference'],'ABSENT')
        self.assertEqual(self.sink.composer_app_observation(CurrentDom(appForm=True,appControlsMetadata='unsafe id'))['plusControlReference'],'INVALID')

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.sink = bridge.NewConversationSink(
            "https://chatgpt.com/c/current-dom", Path(self.directory.name), 1, 1
        )

    def test_composer_app_metadata_never_reads_draft_or_claims_catalog_authority(self):
        cdp=CurrentDom(appForm=True,v5Chip=True,foreignChip=True,appDraft='PRIVATE_DRAFT',traceClicks=True)
        value=self.sink.composer_app_observation(cdp)
        self.assertEqual(value['scope'],'COMPOSER_FORM')
        self.assertEqual(value['v5MarkerCount'],1)
        self.assertEqual(value['plusButtonType'],'BUTTON')
        self.assertEqual(value['plusButtonState'],'ENABLED')
        self.assertEqual(value['plusExpanded'],'CLOSED')
        self.assertEqual(value['basis'],'PRESENTATION_ONLY_NOT_TOOL_CATALOG')
        self.assertNotIn('PRIVATE_',json.dumps(value))
        self.assertEqual(cdp.mutations,0)
        outside=self.sink.composer_app_observation(CurrentDom(appForm=True,outsideV5=True))
        self.assertEqual(outside['v5MarkerCount'],0)
        ambiguous=self.sink.composer_app_observation(CurrentDom(appForm=True,duplicateAppEditor=True))
        self.assertEqual(ambiguous['scope'],'UNPROVEN')
        self.assertEqual(ambiguous['plusButtonCount'],0)
        duplicate=self.sink.composer_app_observation(CurrentDom(appForm=True,appPlusCount=2))
        self.assertEqual(duplicate['plusButtonCount'],2)
        self.assertEqual(duplicate['plusButtonType'],'UNKNOWN')

    def test_composer_app_metadata_refuses_foreign_and_unknown_documents(self):
        with self.assertRaises(bridge.Attention):
            self.sink.composer_app_observation(CurrentDom(appForm=True,location='https://chatgpt.com/c/foreign'))
        class Failure(CurrentDom):
            def evaluate(self,script):raise RuntimeError('PRIVATE_PAGE')
        with self.assertRaises(RuntimeError):
            self.sink.composer_app_observation(Failure(appForm=True))

    def test_composer_app_controls_allow_known_labels_but_never_cross_into_history(self):
        for outside in (False, True):
            value=self.sink.composer_app_observation(CurrentDom(appForm=True,appSemanticLabel='Add files and more',
                appControlOutsideForm=outside))
            self.assertEqual(value['scope'],'COMPOSER_CONTAINER' if outside else 'COMPOSER_FORM')
            self.assertEqual(value['plusButtonCount'],1)
            self.assertEqual(value['plusButtonType'],'BUTTON')
        value=self.sink.composer_app_observation(CurrentDom(appForm=True,appControlOutsideForm=True,appHistoryContainer=True))
        self.assertEqual(value['plusButtonCount'],0)
        for label in ('Send','PRIVATE_UNKNOWN_CONTROL'):
            value=self.sink.composer_app_observation(CurrentDom(appForm=True,appSemanticLabel=label))
            self.assertEqual(value['plusButtonCount'],0)

    def test_receipt_bound_app_menu_reads_fixed_labels_and_closes_without_selecting(self):
        cdp=CurrentDom(appForm=True,appMenuTrace=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'OBSERVED')
        self.assertTrue(value['openRequested'] and value['closeAttempted'] and value['closedVerified'])
        self.assertEqual(value['v5ItemCount'],1)
        self.assertEqual(value['appsItemCount'],1)
        self.assertEqual(cdp.mutations,2)
        self.assertEqual(cdp.options['appExpanded'],'false')
        self.assertNotIn('PRIVATE_',json.dumps(value))

    def test_exact_unlinked_composer_requires_witnessed_fresh_menu_and_proved_close(self):
        cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'OBSERVED')
        self.assertEqual(value['schemaVersion'],4)
        self.assertEqual(value['ownershipEvidence'],'FRESH_MENU_TRANSITION')
        self.assertTrue(value['closedVerified'])
        self.assertEqual(cdp.mutations,2)
        for options in [dict(priorMenuOpen=True),dict(priorDialogOpen=True),dict(appExpanded='true')]:
            with self.subTest(options=options):
                cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,**options)
                value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                self.assertFalse(value['openAttempted'] or value['openRequested'])
                self.assertEqual(getattr(cdp,'mutations',0),0)

    def test_unlinked_semantic_control_is_witnessed_and_changed_identity_never_clicks(self):
        cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,appSemanticLabel='Add files and more')
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'OBSERVED')
        self.assertEqual(value['ownershipEvidence'],'FRESH_MENU_TRANSITION')
        self.assertTrue(value['closedVerified'])
        self.assertEqual(cdp.mutations,2)
        self.assertNotIn('controlKind',value)
        class ChangedControl(CurrentDom):
            def evaluate(self,script):
                value=super().evaluate(script)
                if 'const action="PREPARE"' in script:self.options['appSemanticLabel']='Apps'
                return value
        cdp=ChangedControl(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,appSemanticLabel='Add files and more')
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'CONTROL_UNPROVEN')
        self.assertFalse(value['openRequested'])
        self.assertEqual(cdp.mutations,0)

    def test_unlinked_menu_lost_open_is_not_repeated_and_ambiguity_never_closes_foreign_popup(self):
        class LostOpen(CurrentDom):
            def evaluate(self,script):
                value=super().evaluate(script)
                if 'const action="OPEN"' in script:raise RuntimeError('PRIVATE_UNKNOWN_OPEN')
                return value
        cdp=LostOpen(appForm=True,appMenuTrace=True,unlinkedMenuControl=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'UNPROVEN')
        self.assertTrue(value['openOutcomeUnknown'] and value['closedVerified'])
        self.assertEqual(cdp.mutations,2)
        cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,ambiguousMenuAfterOpen=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'MENU_AMBIGUOUS')
        self.assertFalse(value['closeAttempted'] or value['closedVerified'])
        self.assertEqual(cdp.mutations,1)

    def test_stable_popup_shape_failure_preserves_closed_counts_without_selecting_or_retry(self):
        for role,dialogs,other in [('dialog',1,0),('listbox',0,1),('presentation',0,0)]:
            with self.subTest(role=role):
                cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,appPopupRole=role)
                value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                self.assertEqual(value['state'],'MENU_ABSENT')
                self.assertEqual((value['menuCount'],value['dialogCount'],value['otherPopupCount']),(0,dialogs,other))
                self.assertEqual(value['expandedState'],'OPEN')
                self.assertFalse(value['closeAttempted'] or value['closedVerified'])
                self.assertEqual(cdp.mutations,1)
                self.assertNotIn('PRIVATE_',json.dumps(value))

    def test_non_aria_panels_require_fresh_qualified_structure_and_close_without_selection(self):
        for option,kind in [('appHtmlPopover','HTML_POPOVER'),('appKnownPortal','KNOWN_PORTAL'),('appInlinePanel','INLINE_COMPOSER')]:
            with self.subTest(option=option):
                cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,appSemanticLabel='Add files and more',appPopupRole='presentation',**{option:True})
                value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                self.assertEqual(value['state'],'OBSERVED')
                self.assertEqual(value['panelKind'],kind)
                self.assertEqual(value['panelCount'],1)
                self.assertEqual(value['menuCount'],0)
                self.assertTrue(value['closedVerified'])
                self.assertEqual(value['v5ItemCount'],1)
                self.assertEqual(cdp.mutations,2)
                self.assertNotIn('PRIVATE_',json.dumps(value))
                self.assertNotIn('inlineBaseline',value)

    def test_non_aria_panel_ambiguity_or_unchanged_existing_chip_does_not_authorize_close(self):
        cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,appPopupRole='presentation',appKnownPortal=True,ambiguousPortalAfterOpen=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'PANEL_AMBIGUOUS')
        self.assertEqual(value['panelCount'],2)
        self.assertFalse(value['closeAttempted'])
        self.assertEqual(cdp.mutations,1)
        cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,appPopupRole='presentation',appKnownPortal=True,appPanelIsHistory=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        # The extra assistant unit invalidates the exact-response witness
        # before any UI action or narrower panel classification is permitted.
        self.assertEqual(value['state'],'UNPROVEN')
        self.assertEqual(value['panelCount'],0)
        self.assertFalse(value['closeAttempted'])
        self.assertFalse(value['openAttempted'] or value['openRequested'])
        self.assertEqual(cdp.mutations,0)
        # An existing selected chip must not become evidence of a fresh panel.
        cdp=CurrentDom(appForm=True,appMenuTrace=True,unlinkedMenuControl=True,appPopupRole='presentation',v5Chip=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'MENU_ABSENT')
        self.assertEqual(value['panelCount'],0)
        self.assertFalse(value['closeAttempted'])
        self.assertEqual(cdp.mutations,1)

    def test_app_menu_refuses_drafts_generation_foreign_turn_and_unsafe_controls(self):
        for options in [dict(appDraft='PRIVATE_DRAFT'),dict(observationStop=True),dict(appPlusCount=2),
                dict(appPlusType='submit'),dict(appDisabled=True),dict(appPopup='dialog'),
                dict(priorMenuOpen=True),dict(userText='different wake'),dict(duplicateUser=True),
                dict(location='https://chatgpt.com/c/foreign')]:
            with self.subTest(options=options):
                cdp=CurrentDom(appForm=True,appMenuTrace=True,**options)
                value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                self.assertNotEqual(value['state'],'OBSERVED')
                self.assertFalse(value['openRequested'])
                self.assertEqual(getattr(cdp,'mutations',0),0)
                self.assertNotIn('PRIVATE_',json.dumps(value))

    def test_app_menu_distinguishes_empty_upload_controls_from_unsent_attachments(self):
        for options in [dict(appFileInput=True),dict(appHiddenInput=True),dict(appButtonIcon=True)]:
            with self.subTest(options=options):
                cdp=CurrentDom(appForm=True,appMenuTrace=True,**options)
                value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                self.assertEqual(value['state'],'OBSERVED')
                self.assertTrue(value['closedVerified'])
                self.assertEqual(cdp.mutations,2)
        for options,state in [(dict(appFileInput=True,appSelectedFile=True),'DRAFT_PRESENT'),
                (dict(appFileInput=True,appUnknownFileList=True),'CONTROL_UNPROVEN'),
                (dict(appUnknownInput=True),'CONTROL_UNPROVEN'),
                (dict(appUnownedImage=True),'DRAFT_PRESENT'),(dict(appAttachment=True),'DRAFT_PRESENT')]:
            with self.subTest(options=options):
                cdp=CurrentDom(appForm=True,appMenuTrace=True,**options)
                value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                self.assertEqual(value['state'],state)
                self.assertEqual(getattr(cdp,'mutations',0),0)
                self.assertNotIn('PRIVATE_',json.dumps(value))

    def test_unknown_menu_open_result_never_reopens_and_only_closes_proven_owned_menu(self):
        class LostOpen(CurrentDom):
            lost=False
            def evaluate(self,script):
                value=super().evaluate(script)
                if 'const action="OPEN"' in script and not self.lost:
                    self.lost=True
                    raise RuntimeError('PRIVATE_UNKNOWN_OUTCOME')
                return value
        cdp=LostOpen(appForm=True,appMenuTrace=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'UNPROVEN')
        self.assertTrue(value['openOutcomeUnknown'] and value['closedVerified'])
        self.assertEqual(cdp.mutations,2)
        mismatch=CurrentDom(appForm=True,appMenuTrace=True,menuLinkMismatch=True)
        value=self.sink.inspect_composer_app_menu(mismatch,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'CONTROL_LINK_UNPROVEN')
        self.assertFalse(value['closedVerified'] or value['closeAttempted'])
        self.assertEqual(mismatch.mutations,1)
        # A held/failed menu has no shared state that blocks an independent owner.
        independent=CurrentDom(appForm=True,appMenuTrace=True)
        self.assertEqual(self.sink.inspect_composer_app_menu(independent,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)['state'],'OBSERVED')

    def test_interrupted_menu_close_or_target_drift_never_retries_ui_actions(self):
        class LostClose(CurrentDom):
            def evaluate(self,script):
                value=super().evaluate(script)
                if 'const action="CLOSE"' in script:raise RuntimeError('PRIVATE_CLOSE_UNKNOWN')
                return value
        cdp=LostClose(appForm=True,appMenuTrace=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'MENU_UNPROVEN')
        self.assertEqual(cdp.mutations,2)
        class DriftAfterOpen(CurrentDom):
            def evaluate(self,script):
                value=super().evaluate(script)
                if 'const action="OPEN"' in script:self.options['location']='https://chatgpt.com/c/foreign'
                return value
        cdp=DriftAfterOpen(appForm=True,appMenuTrace=True)
        value=self.sink.inspect_composer_app_menu(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(value['state'],'UNPROVEN')
        self.assertFalse(value['closeAttempted'])
        self.assertEqual(cdp.mutations,1)

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

    def test_missing_dedicated_user_body_readers_agree_without_mutation(self):
        for text in ("exact wake message", "Authored Copy message and Show more are retained"):
            with self.subTest(text=text):
                cdp = CurrentDom(userNoDedicatedBody=True, userText=text, traceMutations=True)
                self.assertEqual(self.sink.trusted_turn_anchor_state(cdp, text), (1, 1))
                self.assertEqual(self.sink.user_message_digests(cdp), [bridge.digest(text)])
                self.assertEqual(cdp.mutations, 0)

    def test_missing_dedicated_user_body_preserves_unknown_text_and_duplicates(self):
        text = "exact wake message"
        cdp = CurrentDom(userNoDedicatedBody=True, userPlainSuffix=" FOREIGN_TEXT", traceMutations=True)
        self.assertNotIn(bridge.digest(text), self.sink.user_message_digests(cdp))
        self.assertEqual(self.sink.trusted_turn_anchor_state(cdp, text), (0, 0))
        self.assertEqual(cdp.mutations, 0)
        duplicate = CurrentDom(userNoDedicatedBody=True, duplicateUser=True, traceMutations=True)
        self.assertEqual(self.sink.user_message_digests(duplicate), [bridge.digest(text)] * 2)
        self.assertEqual(self.sink.trusted_turn_anchor_state(duplicate, text), (2, 0))
        self.assertEqual(duplicate.mutations, 0)

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

    def test_exact_exchange_tool_marker_counts_are_private_capped_and_advisory(self):
        controls, tools = {}, {}
        result = self.sink.response_snapshot(CurrentDom(toolMarkerCount=2, toolError=True,
            toolBusy=True, toolResult=True, historicalToolMarker=True),
            "exact wake message", diagnostic=controls, tool_diagnostic=tools)
        self.assertEqual(result, (0, 0, True, True))
        self.assertTrue(bridge.valid_tool_marker_diagnostics(tools))
        self.assertEqual([tools[key] for key in bridge.TOOL_MARKER_COUNTS], [2, 1, 2, 2])
        self.assertFalse(tools['countOverflow'])
        self.assertIn('NOT_EXECUTION_RECEIPT', tools['basis'])
        self.assertNotIn('PRIVATE_', json.dumps(tools))
        self.assertEqual(controls['reason'], 'COMPLETE')
        self.sink.response_snapshot(CurrentDom(toolMarkerCount=65), "exact wake message",
            diagnostic={}, tool_diagnostic=tools)
        self.assertEqual(tools['callMarkers'],64)
        self.assertTrue(tools['countOverflow'])

    def test_tool_marker_diagnostics_exclude_user_code_hidden_and_history(self):
        for options in ({}, {'toolPlacement':'user'}, {'toolPlacement':'code'}, {'toolHidden':True}):
            tools = {}
            self.sink.response_snapshot(CurrentDom(toolMarkerCount=0 if not options else 1,
                historicalToolMarker=True, **options), "exact wake message", diagnostic={}, tool_diagnostic=tools)
            self.assertEqual([tools[key] for key in bridge.TOOL_MARKER_COUNTS], [0,0,0,0])

    def test_tool_marker_diagnostics_reset_on_wrong_or_duplicate_exchange(self):
        for options in ({'duplicateUser':True},{'userText':'different request'}):
            tools = {'callMarkers':999}
            with self.assertRaises(bridge.PostSubmitUnknown):
                self.sink.response_snapshot(CurrentDom(**options), "exact wake message",
                    diagnostic={}, tool_diagnostic=tools)
            self.assertEqual(tools,{})

    def test_exact_assistant_task_hints_are_closed_and_not_execution_authority(self):
        hints, tools = {}, {}
        answer = "PRIVATE_KEY_MARKER. The run_command tool is not exposed. Private context is missing. Please approve. native.codex.dispatch HTTP 429 WEB_ROLLOVER_PROJECT_BUSY UNKNOWN_PRIVATE_CODE"
        result = self.sink.response_snapshot(CurrentDom(answer=answer), "exact wake message",
            require_usable_body=True, diagnostic={}, tool_diagnostic=tools, execution_diagnostic=hints)
        self.assertEqual(result, (0,0,True,True))
        self.assertTrue(bridge.valid_task_response_hints(hints))
        self.assertEqual([hints[key] for key in ("toolUnavailable","contextUnavailable","approvalRequested","nativeDispatchMentioned")],[True]*4)
        self.assertEqual(hints['knownCodes'],['HTTP_429','WEB_ROLLOVER_PROJECT_BUSY'])
        self.assertNotIn('PRIVATE_KEY_MARKER',json.dumps(hints))
        self.assertEqual(hints['unavailableSubjects'], ['RUN_COMMAND', 'PRIVATE_CONTEXT'])
        for key,value in [('knownCodes',['UNKNOWN_PRIVATE_CODE']),('knownCodes',['HTTP_429','HTTP_429']),
                          ('toolUnavailable',1),('schemaVersion',True),('text',answer)]:
            self.assertFalse(bridge.valid_task_response_hints(hints | {key:value}))
        self.sink.response_snapshot(CurrentDom(answer="A usable answer."), "exact wake message",
            diagnostic={}, tool_diagnostic=tools, execution_diagnostic=hints)
        self.assertFalse(any(hints[key] for key in ('toolUnavailable','contextUnavailable','approvalRequested','nativeDispatchMentioned')))
        self.assertEqual(hints['knownCodes'],[])

    def test_task_hints_refuse_foreign_or_duplicate_exchange_and_reset_previous_hints(self):
        for options in ({'duplicateUser':True},{'userText':'different request'}):
            hints={'PRIVATE_STALE_HINT':True}
            with self.assertRaises(bridge.PostSubmitUnknown):
                self.sink.response_snapshot(CurrentDom(**options),"exact wake message",
                    diagnostic={},tool_diagnostic={},execution_diagnostic=hints)
            self.assertEqual(hints,{})

    def test_cached_tool_rejection_is_classified_without_exporting_request_or_keys(self):
        hints = {}
        body = ('native.codex.dispatch returned INVALID_ARGUMENT: McpServerError: '
                'WEB_TASK_CACHED_ARGUMENTS_INVALID. Dispatch rejected. '
                'PRIVATE_CONTEXT_MARKER executionKey=' + 'f' * 64)
        self.sink.response_snapshot(CurrentDom(answer=body), 'exact wake message',
            require_usable_body=True, diagnostic={}, tool_diagnostic={}, execution_diagnostic=hints)
        self.assertTrue(bridge.valid_task_response_hints(hints))
        self.assertEqual(hints['schemaVersion'], 4)
        self.assertEqual(hints['knownCodes'], ['WEB_TASK_CACHED_ARGUMENTS_INVALID', 'INVALID_ARGUMENT'])
        self.assertEqual(hints['refusalCategories'], ['DISPATCH_REJECTED'])
        self.assertNotIn('PRIVATE_CONTEXT_MARKER', json.dumps(hints))
        self.assertNotIn('f' * 64, json.dumps(hints))
        self.assertNotIn('executionKey', json.dumps(hints))
        self.assertFalse(hints['scanTruncated'])

    def test_missing_call_does_not_claim_an_available_tool_is_missing(self):
        for answer, expected in [
            ("run_command is available, but nativeCall is not visible.", ['NATIVE_CALL']),
            ("I don't have nativeCall. run_command is available.", ['NATIVE_CALL']),
            ("The run_command tool is not exposed. The executionKey is missing.", ['RUN_COMMAND', 'EXECUTION_KEY']),
            ("I have run_command and nativeCall is not available.", ['NATIVE_CALL']),
            ("CatDesk-Local-Tunnel-v5 is unavailable. Frozen capsule is not visible.", ['PRIVATE_CONTEXT','CATDESK_APP']),
            ("All tools and the original context are available.", []),
        ]:
            with self.subTest(answer=answer):
                hints = {}
                self.sink.response_snapshot(CurrentDom(answer=answer), 'exact wake message',
                    require_usable_body=True, diagnostic={}, tool_diagnostic={}, execution_diagnostic=hints)
                self.assertEqual(hints['unavailableSubjects'], expected)
                self.assertTrue(bridge.valid_task_response_hints(hints))
                self.assertNotIn(answer, json.dumps(hints))
                for bad in [['PRIVATE_KEY_MARKER'], ['NATIVE_CALL','NATIVE_CALL'],
                            ['EXECUTION_KEY','RUN_COMMAND'], 'NATIVE_CALL', None]:
                    self.assertFalse(bridge.valid_task_response_hints(hints | {'unavailableSubjects':bad}))
                for bad in [{}, [], True, 5]:
                    self.assertFalse(bridge.valid_task_response_hints(hints | {'schemaVersion':bad}))

    def test_bounded_classifier_reports_incomplete_coverage_without_exporting_long_body(self):
        hints = {}
        self.sink.response_snapshot(CurrentDom(answer='safe text ' * 5000 + ' WEB_TASK_CACHED_ARGUMENTS_INVALID'),
            'exact wake message', require_usable_body=True, diagnostic={}, tool_diagnostic={}, execution_diagnostic=hints)
        self.assertTrue(bridge.valid_task_response_hints(hints))
        self.assertTrue(hints['scanTruncated'])
        self.assertEqual(hints['knownCodes'], [])
        self.assertLess(len(json.dumps(hints)), 1100)

    def test_argument_hints_report_only_closed_tools_causes_and_invariants(self):
        hints = {}
        body = ('run_command returned INVALID_ARGUMENT: invalid JSON, missing required field. '
                'native.codex.dispatch was not attempted. CWD_OVERRIDE_FORBIDDEN '
                'PAYLOAD_JSON_INVALID PRIVATE_FIELD=PRIVATE_VALUE executionKey=' + 'e'*64)
        self.sink.response_snapshot(CurrentDom(answer=body), 'exact wake message',
            diagnostic={}, tool_diagnostic={}, execution_diagnostic=hints)
        self.assertTrue(bridge.valid_task_response_hints(hints))
        self.assertEqual(hints['mentionedTools'], ['RUN_COMMAND','NATIVE_DISPATCH'])
        self.assertEqual(hints['argumentIssues'], ['JSON_SYNTAX','MISSING_REQUIRED_FIELD'])
        self.assertEqual(hints['cachedInvariants'], ['CWD_OVERRIDE_FORBIDDEN','PAYLOAD_JSON_INVALID'])
        for private in ['PRIVATE_FIELD','PRIVATE_VALUE','executionKey','e'*64]:
            self.assertNotIn(private,json.dumps(hints))
        for field in ['mentionedTools','argumentIssues','cachedInvariants']:
            for bad in [['PRIVATE_VALUE'], hints[field]*2, True, {}, None]:
                self.assertFalse(bridge.valid_task_response_hints(hints | {field:bad}))

    def test_argument_hints_preserve_uncertainty_and_exact_code_boundaries(self):
        hints = {}
        self.sink.response_snapshot(CurrentDom(answer='INVALID_ARGUMENT. '
            'NOT_CWD_OVERRIDE_FORBIDDEN_OTHER PRIVATE_TOOL_NAME.'), 'exact wake message',
            diagnostic={}, tool_diagnostic={}, execution_diagnostic=hints)
        self.assertEqual(hints['knownCodes'], ['INVALID_ARGUMENT'])
        for field in ['mentionedTools','argumentIssues','cachedInvariants']:
            self.assertEqual(hints[field],[])
        self.assertNotIn('retryAuthorized',hints)
        for version in [1,2,3]:
            legacy = {k:v for k,v in hints.items() if k not in ['mentionedTools','argumentIssues','cachedInvariants','trace']}
            legacy['schemaVersion']=version
            if version<3:legacy.pop('unavailableSubjects')
            if version==1:
                legacy.pop('refusalCategories');legacy.pop('scanTruncated')
            self.assertTrue(bridge.valid_task_response_hints(legacy))

    def test_actual_dispatch_schema_and_admission_error_names_are_recognized_without_values(self):
        hints={}
        self.sink.response_snapshot(CurrentDom(answer='INVALID_ARGUMENT: WEB_TASK_ARGUMENTS_INVALID '
            'WEB_TASK_NATIVE_ARGUMENTS_INVALID WEB_TASK_REQUEST_ID_REQUIRED '
            'WEB_TASK_CENTRAL_MODE_REQUIRED WEB_TASK_CENTRAL_AUTH_REQUIRED PLAN_REQUIRED '
            'PRIVATE_CONTEXT='+ 'b'*64), 'exact wake message',
            diagnostic={},tool_diagnostic={},execution_diagnostic=hints)
        self.assertEqual(hints['knownCodes'],['INVALID_ARGUMENT','WEB_TASK_ARGUMENTS_INVALID',
            'WEB_TASK_NATIVE_ARGUMENTS_INVALID','WEB_TASK_REQUEST_ID_REQUIRED',
            'WEB_TASK_CENTRAL_MODE_REQUIRED','WEB_TASK_CENTRAL_AUTH_REQUIRED','PLAN_REQUIRED'])
        self.assertTrue(bridge.valid_task_response_hints(hints))
        self.assertNotIn('PRIVATE_CONTEXT',json.dumps(hints));self.assertNotIn('b'*64,json.dumps(hints))

    def test_trace_metadata_is_task_bound_passive_and_exports_no_labels_or_panels(self):
        for options, expected in [
            ([{'expanded':False,'linked':True}],(1,0,1,0,1,0)),
            ([{'expanded':True,'linked':True}],(1,1,0,0,1,1)),
            ([{}],(1,0,0,1,0,0)),
            ([{'summary':True,'expanded':False}],(1,0,1,0,1,0)),
            ([{'summary':True,'expanded':True}],(1,1,0,0,1,1)),
            ([{'placement':'user'},{'placement':'code'},{'label':'PRIVATE_UNKNOWN_LABEL'}],(0,0,0,0,0,0)),
        ]:
            with self.subTest(options=options):
                hints={}
                cdp=CurrentDom(traceControls=options,traceClicks=True)
                self.sink.response_snapshot(cdp,'exact wake message',diagnostic={},tool_diagnostic={},execution_diagnostic=hints)
                trace=hints['trace']
                self.assertTrue(bridge.valid_task_response_hints(hints))
                self.assertEqual(tuple(trace[k] for k in bridge.TASK_TRACE_COUNTS),expected)
                self.assertEqual(cdp.mutations,0)
                for private in ['PRIVATE_TRACE_PANEL','PRIVATE_TRACE_ARGUMENTS','PRIVATE_UNKNOWN_LABEL','Worked for']:
                    self.assertNotIn(private,json.dumps(hints))
                for field, bad in [('knownControls',True),('knownControls',65),('knownControls',-1),
                        ('knownControls',2),('scope','OTHER_EXCHANGE'),('rawText','PRIVATE_CONTEXT')]:
                    self.assertFalse(bridge.valid_task_trace_metadata(trace | {field:bad}))

    def test_trace_metadata_cannot_escape_foreign_or_duplicate_exchange(self):
        for options in [{'duplicateUser':True},{'userText':'another request'}]:
            hints={'trace':'PRIVATE_STALE_TRACE'}
            with self.assertRaises(bridge.PostSubmitUnknown):
                self.sink.response_snapshot(CurrentDom(traceControls=[{'expanded':True,'linked':True}],**options),
                    'exact wake message',diagnostic={},tool_diagnostic={},execution_diagnostic=hints)
            self.assertEqual(hints,{})

    def test_receipt_bound_trace_opens_once_and_returns_error_names_and_argument_types_only(self):
        cdp=PersistentCurrentDom(observationEditor=True,traceInteractive=True,
            traceControls=[{'expanded':False}],traceJson=json.dumps({'command':'CATDESK_WEB_TASK={}',
                'cwd':None,'timeout':None,'PRIVATE_FIELD':'PRIVATE_KEY_VALUE'}))
        self.addCleanup(cdp.close)
        result=self.sink.inspect_response_trace(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(result['state'],'OBSERVED')
        self.assertEqual(result['panelKind'],'FRESH_CHILD')
        self.assertEqual(result['knownCodes'],['WEB_TASK_CACHED_ARGUMENTS_INVALID','INVALID_ARGUMENT'])
        self.assertEqual(result['cachedInvariants'],['CWD_OVERRIDE_FORBIDDEN'])
        self.assertEqual(result['mentionedTools'],['RUN_COMMAND'])
        self.assertEqual(result['argumentTypes']['cwd'],'NULL')
        self.assertEqual(result['argumentTypes']['timeout'],'NULL')
        self.assertTrue(result['unknownOuterFields'])
        self.assertEqual(cdp.mutations,1)
        self.assertTrue(bridge.valid_trace_inspection(result))
        for secret in ['PRIVATE_','executionKey','CATDESK_WEB_TASK={}']:
            self.assertNotIn(secret,json.dumps(result))
        for field,bad in [('panelCount',2),('openRequested',False),('knownCodes',['PRIVATE_VALUE']),
                ('argumentTypes',{'command':'PRIVATE_COMMAND'}),('rawText','PRIVATE_CONTEXT')]:
            self.assertFalse(bridge.valid_trace_inspection(result|{field:bad}))

    def test_trace_action_refuses_unsafe_controls_drafts_generation_and_ambiguous_exchange(self):
        for options in [
            {'traceControls':[{'expanded':False,'type':'submit'}]},
            {'traceControls':[{'expanded':False},{'expanded':False}]},
            {'observationDraft':'PRIVATE_DRAFT'}, {'observationStop':True},
            {'observationLogin':True}, {'location':'chrome-error://chromewebdata/'},
            {'duplicateUser':True}, {'userText':'foreign request'},
        ]:
            with self.subTest(options=options):
                cdp=PersistentCurrentDom(**({'observationEditor':True,'traceInteractive':True,'traceControls':[{'expanded':False}]}|options))
                try:
                    try:result=self.sink.inspect_response_trace(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                    except (bridge.Attention,bridge.PostSubmitUnknown):result=None
                    if result is not None:self.assertNotEqual(result['state'],'OBSERVED');self.assertFalse(result['openAttempted'])
                    self.assertEqual(getattr(cdp,'mutations',0),0)
                finally:cdp.close()

    def test_trace_unknown_or_changed_outcome_never_clicks_again_or_exports_partial_content(self):
        for options,expected in [({'traceClickThrows':True},'OPEN_OUTCOME_UNKNOWN'),
                ({'traceChangedControl':True},'CONTROL_AMBIGUOUS'),({'traceAmbiguousPanels':True},'PANEL_AMBIGUOUS')]:
            with self.subTest(options=options):
                cdp=PersistentCurrentDom(observationEditor=True,traceInteractive=True,traceControls=[{'expanded':False}],**options)
                try:
                    result=self.sink.inspect_response_trace(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                    self.assertEqual(result['state'],expected);self.assertEqual(cdp.mutations,1)
                    self.assertTrue(result['openAttempted']);self.assertEqual(result['knownCodes'],[])
                    self.assertTrue(bridge.valid_trace_inspection(result));self.assertNotIn('PRIVATE_',json.dumps(result))
                finally:cdp.close()

    def test_trace_interrupted_transport_after_click_remains_unknown_without_a_second_click(self):
        class Interrupted(PersistentCurrentDom):
            def evaluate(self,script):
                value=super().evaluate(script)
                if self.mutations and not getattr(self,'interrupted',False):
                    self.interrupted=True
                    raise RuntimeError('PRIVATE_TRANSPORT_DIAGNOSTIC')
                return value
        cdp=Interrupted(observationEditor=True,traceInteractive=True,traceControls=[{'expanded':False}])
        self.addCleanup(cdp.close)
        result=self.sink.inspect_response_trace(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(result['state'],'OPEN_OUTCOME_UNKNOWN');self.assertTrue(result['openOutcomeUnknown'])
        self.assertTrue(result['openAttempted']);self.assertEqual(cdp.mutations,1)
        self.assertEqual(result['knownCodes'],[]);self.assertNotIn('PRIVATE_',json.dumps(result))
        self.assertEqual(self.sink.trace_failure(),dict(schemaVersion=1,phase='OPEN',boundary='SNAPSHOT',reason='RESPONSE_STATE_QUERY_FAILED'))

    def test_trace_loading_waits_without_reclick_and_never_classifies_an_unfinished_panel(self):
        import time
        for loading,expected in [('delayed','OBSERVED'),('stuck','OPEN_OUTCOME_UNKNOWN')]:
            with self.subTest(loading=loading):
                cdp=PersistentCurrentDom(observationEditor=True,traceInteractive=True,
                    traceControls=[{'expanded':False}],traceLoading=loading)
                try:
                    result=self.sink.inspect_response_trace(cdp,'exact wake message',
                        bridge.digest('exact wake message'),sleeper=lambda _:time.sleep(.01))
                    self.assertEqual(result['state'],expected);self.assertEqual(cdp.mutations,1)
                    self.assertTrue(bridge.valid_trace_inspection(result))
                    if loading=='stuck':
                        self.assertEqual(result['knownCodes'],[])
                        self.assertEqual(self.sink.trace_failure(),dict(schemaVersion=1,phase='READ',
                            boundary='PANEL_STABILITY',reason='TRACE_READ_WINDOW_EXPIRED'))
                    else:self.assertIsNone(self.sink.trace_failure())
                    self.assertNotIn('PRIVATE_',json.dumps(result))
                finally:cdp.close()

    def test_trace_failure_classifies_boundary_without_private_exception_or_stale_cause(self):
        with patch.object(self.sink,'observe_cooperative_response',side_effect=bridge.Attention('RESPONSE_TARGET_DRIFT')):
            with self.assertRaises(bridge.Attention):
                self.sink.inspect_response_trace(object(),'exact wake message',bridge.digest('exact wake message'))
        value=self.sink.trace_failure()
        self.assertEqual(value,dict(schemaVersion=1,phase='PREPARE',boundary='COOPERATIVE_RESPONSE',reason='RESPONSE_TARGET_DRIFT'))
        for key,bad in [('reason','PRIVATE_ERROR'),('phase','PRIVATE_PHASE'),('boundary','PRIVATE_PATH'),('rawText','PRIVATE_CONTEXT')]:
            self.assertFalse(bridge.valid_trace_failure(value|{key:bad}))
        self.sink.trace_failure_diagnostic=value|{'reason':'PRIVATE_ERROR'}
        self.assertIsNone(self.sink.trace_failure())
        cdp=PersistentCurrentDom(observationEditor=True,traceInteractive=True,traceControls=[{'expanded':False,'type':'submit'}])
        self.addCleanup(cdp.close)
        result=self.sink.inspect_response_trace(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
        self.assertEqual(result['state'],'CONTROL_UNSUPPORTED');self.assertIsNone(self.sink.trace_failure())

    def test_trace_reads_only_linked_details_or_fresh_role_panels_and_reports_truncated_coverage(self):
        for control,options,kind in [({'expanded':False,'linked':True},{},'CONTROL_LINK'),
                ({'summary':True,'expanded':False},{},'DETAILS'),
                ({'expanded':False},{'traceRolePanel':True},'FRESH_ROLE'),
                ({'expanded':False},{'tracePanelText':'safe '*7000+' WEB_TASK_ARGUMENTS_INVALID'},'FRESH_CHILD')]:
            with self.subTest(kind=kind):
                cdp=PersistentCurrentDom(observationEditor=True,traceInteractive=True,traceControls=[control],**options)
                try:
                    result=self.sink.inspect_response_trace(cdp,'exact wake message',bridge.digest('exact wake message'),sleeper=lambda _:None)
                    self.assertEqual(result['state'],'OBSERVED');self.assertEqual(result['panelKind'],kind)
                    self.assertEqual(cdp.mutations,1);self.assertTrue(bridge.valid_trace_inspection(result))
                    if 'tracePanelText' in options:
                        self.assertTrue(result['scanTruncated']);self.assertEqual(result['knownCodes'],[])
                    self.assertNotIn('PRIVATE_',json.dumps(result))
                finally:cdp.close()

    def test_identity_refusal_and_exact_code_boundaries_are_closed(self):
        hints = {}
        self.sink.response_snapshot(CurrentDom(answer='Dispatch blocked: private-context SHA-256 mismatch. '
            'NOT_WEB_TASK_CACHED_ARGUMENTS_INVALID_OTHER PRIVATE_UNKNOWN_CODE'), 'exact wake message',
            require_usable_body=True, diagnostic={}, tool_diagnostic={}, execution_diagnostic=hints)
        self.assertEqual(hints['knownCodes'], [])
        self.assertEqual(hints['refusalCategories'], ['IDENTITY_MISMATCH', 'DISPATCH_REJECTED'])
        for replacement in (['PRIVATE_UNKNOWN_CODE'], ['IDENTITY_MISMATCH', 'IDENTITY_MISMATCH'], 'IDENTITY_MISMATCH'):
            self.assertFalse(bridge.valid_task_response_hints(hints | {'refusalCategories':replacement}))

    def test_rejection_in_later_answer_block_is_not_silently_discarded(self):
        hints = {}
        self.sink.response_snapshot(CurrentDom(answer='native.codex.dispatch returned INVALID_ARGUMENT.',
            additionalAnswerBodies=['McpServerError: WEB_TASK_CACHED_ARGUMENTS_INVALID.', 'PRIVATE_KEY_MARKER']),
            'exact wake message', require_usable_body=True, diagnostic={}, tool_diagnostic={}, execution_diagnostic=hints)
        self.assertEqual(hints['knownCodes'], ['WEB_TASK_CACHED_ARGUMENTS_INVALID','INVALID_ARGUMENT'])
        self.assertEqual(hints['refusalCategories'], ['DISPATCH_REJECTED'])
        self.assertNotIn('PRIVATE_KEY_MARKER', json.dumps(hints))

    def test_tool_marker_diagnostics_schema_rejects_content_or_unbounded_values(self):
        tools={}
        self.sink.response_snapshot(CurrentDom(), "exact wake message", diagnostic={}, tool_diagnostic=tools)
        for key, value in [('callMarkers',True),('callMarkers',65),('countOverflow',0),
                ('schemaVersion',True),('scope','OTHER_CHAT'),('arguments','PRIVATE_KEY')]:
            with self.subTest(key=key,value=value):
                self.assertFalse(bridge.valid_tool_marker_diagnostics(tools | {key:value}))

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

    def test_unmounted_current_error_card_is_diagnosed_without_any_mutation(self):
        cdp = CurrentDom(userOnly=True, unmountedLoadError=True,
                         observationEditor=True, traceClicks=True)
        self.assertEqual(self.sink.trusted_turn_anchor_state(cdp, 'exact wake message'), (1, 0))
        with self.assertRaisesRegex(bridge.Attention, '^RESPONSE_LOAD_ERROR$'):
            self.sink.observe_cooperative_response(cdp, 'exact wake message', bridge.digest('exact wake message'))
        self.assertEqual(cdp.mutations, 0)

    def test_unmounted_error_detection_excludes_quotes_tools_history_and_ambiguity(self):
        for options in [
            *[{'unmountedLoadError': True, 'unmountedPlacement': where}
              for where in ['user', 'code', 'tool', 'unit', 'quote', 'control', 'before']],
            {'unmountedLoadError': True, 'unmountedErrorCount': 2},
            {'historicalLoadError': True}, {},
        ]:
            with self.subTest(options=options):
                cdp = CurrentDom(userOnly=True, observationEditor=True, traceClicks=True, **options)
                self.assertEqual(self.sink.observe_cooperative_response(
                    cdp, 'exact wake message', bridge.digest('exact wake message')), 'RESPONSE_PENDING')
                self.assertEqual(self.sink.response_observation_reason, 'ASSISTANT_NOT_MOUNTED')
                self.assertEqual(cdp.mutations, 0)

    def test_unmounted_error_does_not_override_active_generation_or_duplicate_user(self):
        cdp = CurrentDom(userOnly=True, unmountedLoadError=True,
                         observationEditor=True, observationStop=True)
        self.assertEqual(self.sink.observe_cooperative_response(
            cdp, 'exact wake message', bridge.digest('exact wake message')), 'RESPONSE_PENDING')
        self.assertEqual(self.sink.response_observation_reason, 'GENERATION_ACTIVE')
        with self.assertRaisesRegex(bridge.Attention, '^RESPONSE_EXPECTED_USER_DUPLICATED$'):
            self.sink.observe_cooperative_response(
                CurrentDom(userOnly=True, unmountedLoadError=True, duplicateUser=True, observationEditor=True),
                'exact wake message', bridge.digest('exact wake message'))

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

    def test_completion_uses_actual_document_even_if_driver_metadata_is_stale(self):
        cdp = CurrentDom(metadataLocation='https://chatgpt.com/', observationEditor=True)
        self.assertEqual(self.sink.observe_cooperative_response(
            cdp, 'exact wake message', bridge.digest('exact wake message')), 'RESPONSE_COMPLETE')

    def test_completion_refuses_foreign_document_despite_expected_driver_metadata(self):
        cdp = CurrentDom(location='https://chatgpt.com/c/foreign', metadataLocation=self.sink.url,
                         observationEditor=True, traceClicks=True)
        with self.assertRaisesRegex(bridge.Attention, '^RESPONSE_TARGET_DRIFT$'):
            self.sink.observe_cooperative_response(cdp, 'exact wake message', bridge.digest('exact wake message'))
        self.assertEqual(getattr(cdp, 'mutations', 0), 0)

    def test_completion_rechecks_document_after_reading_exact_response(self):
        cdp = CurrentDom(observationEditor=True)
        original = self.sink.response_snapshot
        def drift(*args, **kwargs):
            result = original(*args, **kwargs)
            cdp.options['location'] = 'https://chatgpt.com/c/foreign'
            return result
        with patch.object(self.sink, 'response_snapshot', side_effect=drift), \
                self.assertRaisesRegex(bridge.Attention, '^RESPONSE_TARGET_DRIFT$'):
            self.sink.observe_cooperative_response(cdp, 'exact wake message', bridge.digest('exact wake message'))

    def test_completion_requires_readable_document_route_without_private_failure_text(self):
        for value in [None, {}, 17]:
            class InvalidDocument:
                def evaluate(self, _): return value
                def get_current_url(self): raise AssertionError('driver metadata is not document authority')
            with self.assertRaisesRegex(bridge.Attention, '^RESPONSE_TARGET_DRIFT$'):
                self.sink.require_response_document(InvalidDocument())
        class Unavailable:
            def evaluate(self, _): raise RuntimeError('PRIVATE_BROWSER_DETAILS')
        with self.assertRaisesRegex(bridge.Attention, '^RESPONSE_TARGET_QUERY_FAILED$'):
            self.sink.require_response_document(Unavailable())

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
