// Execute the actual inline widget in a deterministic host/DOM fixture.
// No network, browser profile, MCP tool execution or production state is used.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '../src/widget/catdesk_dashboard.html'), 'utf8');
const scripts = [...html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)];
assert.equal(scripts.length, 1, 'the fixture executes the complete existing inline script');
const source = scripts[0][1].replaceAll('__catdeskInitialTokenStatsLayoutPlaceholder__', 'disable');

function payload(detail = 'QUALIFIED_WIDGET_RESULT') {
  const usage = { inputTokens: 0, outputTokens: 0, totalTokens: 0 };
  return { schema: 'catdesk.review.v1', panelMode: 'tool_call', title: 'Fixture', state: 'done',
    toolName: 'fixture_read_only', tokenStatsLayout: 'disable', showDetailMode: 'expanded',
    changedFiles: [], hasChanges: false, turnTokenUsage: usage, toolCallCount: 0,
    historyTurnTokenUsage: usage, historyToolCallCount: 0, call: 'fixture-check', detail };
}
function metadata(detail) { return { 'catdesk/widgetPayload': payload(detail) }; }
function fixture(options = {}) {
  const listeners = new Map();
  const timers = new Map();
  const sent = [];
  let sequence = 0;
  const root = { innerHTML: '', height: 100, getBoundingClientRect() { return { height: this.height }; }, style: {}, querySelector: () => null, querySelectorAll: () => [],
    classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } } };
  const parent = { postMessage(message) { if (options.throwOnPost) throw Error('PRIVATE_HOST_ERROR'); sent.push(message); } };
  const window = { parent, openai: options.openai, getComputedStyle: () => ({ paddingTop: '12px', paddingBottom: '12px' }), localStorage: { getItem: () => null, setItem() {} },
    setTimeout(fn, delay) { const id = ++sequence; timers.set(id, { fn, delay }); return id; },
    clearTimeout(id) { timers.delete(id); }, requestAnimationFrame() { return ++sequence; }, cancelAnimationFrame() {},
    addEventListener(name, callback) { if (!listeners.has(name)) listeners.set(name, []); listeners.get(name).push(callback); } };
  const document = { body: {}, getElementById: id => id === 'root' ? root : null, querySelectorAll: () => [], addEventListener() {} };
  vm.runInNewContext(source, { window, document, console }, { timeout: 1000 });
  function emit(data, eventSource = parent) {
    for (const callback of listeners.get('message') || []) callback({ data, source: eventSource });
  }
  return { root, window, sent, emit,
    initialize(result = { protocolVersion: '2026-01-26' }) {
      emit({ jsonrpc: '2.0', id: 'catdesk-ui-initialize', result });
    },
    result(meta = metadata()) { emit({ jsonrpc: '2.0', method: 'ui/notifications/tool-result', params: { _meta: meta } }); },
    globals(meta) { for (const callback of listeners.get('openai:set_globals') || []) callback({ detail: { globals: { toolResponseMetadata: meta } } }); },
    expire() { for (const [id, timer] of [...timers]) if (timer.delay === 5000) { timers.delete(id); timer.fn(); } },
  };
}

test('standard host initializes once, then renders canonical hidden metadata without legacy globals', () => {
  const f = fixture();
  assert.equal(f.sent.length, 1);
  assert.equal(f.sent[0].method, 'ui/initialize');
  assert.equal(f.sent[0].params.protocolVersion, '2026-01-26');
  f.initialize();
  assert.equal(f.sent[1].method, 'ui/notifications/initialized');
  f.result();
  assert.match(f.root.innerHTML, /QUALIFIED_WIDGET_RESULT/);
  assert.deepEqual(f.sent.map(x => x.method), ['ui/initialize', 'ui/notifications/initialized', 'ui/notifications/size-changed'], 'rendering sends only lifecycle and deduplicated size metadata');
});
test('wrong window, request id and premature tool results are ignored', () => {
  const f = fixture();
  f.emit({ jsonrpc: '2.0', id: 'catdesk-ui-initialize', result: { protocolVersion: '2026-01-26' } }, {});
  f.emit({ jsonrpc: '2.0', id: 'other', result: { protocolVersion: '2026-01-26' } });
  f.result();
  assert.doesNotMatch(f.root.innerHTML, /QUALIFIED_WIDGET_RESULT/);
  assert.equal(f.sent.length, 1);
  f.initialize();
  f.result();
  assert.match(f.root.innerHTML, /QUALIFIED_WIDGET_RESULT/);
});
test('duplicate initialization response cannot initialize or submit work twice', () => {
  const f = fixture();
  f.initialize(); f.initialize(); f.result();
  assert.deepEqual(f.sent.map(x => x.method), ['ui/initialize', 'ui/notifications/initialized', 'ui/notifications/size-changed']);
});
test('height notifications are numeric, deduplicated and contain no tool content', () => {
  const f = fixture(); f.initialize(); f.result(); f.result();
  let sizes = f.sent.filter(x => x.method === 'ui/notifications/size-changed');
  assert.equal(sizes.length, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(sizes[0].params)), { height: 124 });
  f.root.height = 300; f.result();
  sizes = f.sent.filter(x => x.method === 'ui/notifications/size-changed');
  assert.equal(sizes.length, 2);
  assert.equal(sizes[1].params.height, 324);
});
test('unsupported or malformed initialization fails without leaking host error text', () => {
  for (const result of [null, [], { protocolVersion: 'unknown' }]) {
    const f = fixture(); f.initialize(result); f.result();
    assert.match(f.root.innerHTML, /UI Host Unavailable/);
    assert.doesNotMatch(f.root.innerHTML, /QUALIFIED_WIDGET_RESULT/);
    assert.equal(f.sent.length, 1);
  }
  const f = fixture();
  f.emit({ jsonrpc: '2.0', id: 'catdesk-ui-initialize', error: { message: 'PRIVATE_HOST_ERROR' } });
  assert.doesNotMatch(f.root.innerHTML, /PRIVATE_HOST_ERROR/);
});
test('initialization timeout is bounded, does not retry and refuses late replies', () => {
  const f = fixture(); f.expire(); f.initialize(); f.result();
  assert.match(f.root.innerHTML, /UI Host Unavailable/);
  assert.equal(f.sent.length, 1);
});
test('legacy direct and canonical globals remain usable when standard bridge is unavailable', () => {
  for (const value of [metadata(), { _meta: metadata() }]) {
    const f = fixture({ openai: { toolResponseMetadata: value } });
    assert.match(f.root.innerHTML, /QUALIFIED_WIDGET_RESULT/);
    f.expire();
    assert.match(f.root.innerHTML, /QUALIFIED_WIDGET_RESULT/);
    assert.doesNotMatch(f.root.innerHTML, /UI Host Unavailable/);
  }
});
test('legacy set_globals canonical envelope still updates the existing renderer', () => {
  const f = fixture(); f.globals({ _meta: metadata('LEGACY_UPDATED') });
  assert.match(f.root.innerHTML, /LEGACY_UPDATED/);
});
test('missing or malformed payloads use fixed diagnostics rather than arbitrary tool text', () => {
  const f = fixture(); f.initialize();
  f.emit({ jsonrpc: '2.0', method: 'ui/notifications/tool-result', params: { content: [{ text: 'PRIVATE_UNRELATED_CONTENT' }] } });
  assert.match(f.root.innerHTML, /Widget Payload Missing/);
  assert.doesNotMatch(f.root.innerHTML, /PRIVATE_UNRELATED_CONTENT/);
  f.result({ 'catdesk/widgetPayload': { schema: 'catdesk.review.v1', privateField: 'PRIVATE_FIELD_VALUE' } });
  assert.match(f.root.innerHTML, /Invalid Widget Payload/);
  assert.doesNotMatch(f.root.innerHTML, /PRIVATE_FIELD_VALUE/);
});
test('separate widget instances retain independent results and initialization state', () => {
  const a = fixture(); const b = fixture();
  a.initialize(); a.result(metadata('INSTANCE_A'));
  b.expire();
  assert.match(a.root.innerHTML, /INSTANCE_A/);
  assert.doesNotMatch(b.root.innerHTML, /INSTANCE_A/);
  assert.match(b.root.innerHTML, /UI Host Unavailable/);
});
test('postMessage failure produces a bounded redacted diagnostic', () => {
  const f = fixture({ throwOnPost: true });
  assert.match(f.root.innerHTML, /UI Host Unavailable/);
  assert.doesNotMatch(f.root.innerHTML, /PRIVATE_HOST_ERROR/);
});
test('host health ping receives only an empty correlated result', () => {
  const f = fixture(); f.initialize();
  f.emit({ jsonrpc: '2.0', id: 42, method: 'ping' });
  assert.deepEqual(JSON.parse(JSON.stringify(f.sent.at(-1))), { jsonrpc: '2.0', id: 42, result: {} });
});
test('unsupported host requests are refused without echoing their arguments', () => {
  const f = fixture(); f.initialize();
  f.emit({ jsonrpc: '2.0', id: 'unsupported', method: 'tools/call', params: { private: 'PRIVATE_REQUEST_ARGUMENT' } });
  const reply = JSON.parse(JSON.stringify(f.sent.at(-1)));
  assert.equal(reply.error.code, -32601);
  assert.doesNotMatch(JSON.stringify(reply), /PRIVATE_REQUEST_ARGUMENT/);
});
test('teardown acknowledges cleanup and ignores late tool results and legacy globals', () => {
  const f = fixture(); f.initialize(); f.result();
  const before = f.root.innerHTML;
  f.emit({ jsonrpc: '2.0', id: 'teardown', method: 'ui/resource-teardown', params: { reason: 'fixture' } });
  assert.deepEqual(JSON.parse(JSON.stringify(f.sent.at(-1))), { jsonrpc: '2.0', id: 'teardown', result: {} });
  f.result(metadata('LATE_RESULT')); f.globals(metadata('LATE_GLOBALS'));
  assert.equal(f.root.innerHTML, before);
});
