import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import { Computer, UnknownExecutionError } from '../typescript/packages/sdk/dist/index.js';
import { HelperTransport } from '../typescript/packages/runtime/dist/index.js';

if (process.platform !== 'win32') throw new Error('This fixture requires a Windows interactive desktop.');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
await mkdir(resolve(root, '.local'), { recursive: true });
const run = await mkdtemp(resolve(root, '.local/windows-gui-'));
const helperPath = resolve(process.env.CU_HELPER ?? resolve(root, 'native/target/debug/cu-helper.exe'));
const executable = resolve(run, 'Fixture.exe');
const compiler = resolve(process.env.WINDIR ?? 'C:/Windows', 'Microsoft.NET/Framework64/v4.0.30319/csc.exe');
const compile = spawnSync(compiler, ['/nologo', '/target:winexe', `/out:${executable}`, '/r:System.Windows.Forms.dll', '/r:System.Drawing.dll', '/r:System.Web.Extensions.dll', resolve(root, 'tests/fixtures/windows/Fixture.cs')], { encoding: 'utf8' });
if (compile.status !== 0) throw new Error(`Fixture compilation failed: ${compile.stdout}\n${compile.stderr}`);
const oracle = resolve(run, 'oracle.json');
const fixture = spawn(executable, [oracle, run.split(/[/\\]/).at(-1)], { stdio: 'ignore' });
const runtimes = [];
let mcp;
const outcomes = [];
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function snapshot() {
  for (let i = 0; i < 10; i++) {
    try { return JSON.parse(await readFile(oracle, 'utf8')); }
    catch { await sleep(10); }
  }
  return undefined;
}
async function waitFor(test, label, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { const data = await snapshot(); if (data && test(data)) return data; await sleep(50); }
  throw new Error(`Timed out waiting for ${label}; oracle=${JSON.stringify(await snapshot())}`);
}
function record(name) { outcomes.push({ name, passed: true }); console.log(`PASS ${name}`); }
try {
  await waitFor(value => value.pid === fixture.pid, 'fixture startup');
  const policyPath = resolve(run, 'policy.json');
  await writeFile(policyPath, JSON.stringify({ allowed_pids: [fixture.pid], allow_input: true, allow_screenshot: true, allow_accessibility: true, session_seconds: 180 }));
  const runtime = new Computer({ helperPath, policyPath, onStderr: text => process.stderr.write(text) }); runtimes.push(runtime);
  await runtime.start();
  async function observe(screenshot = false) {
    const windows = await runtime.listWindows();
    assert.equal(windows.length, 1, 'Only the approved fixture window is visible to the runtime');
    return runtime.observe(windows[0], { screenshot, accessibility: true });
  }
  function element(observation, name) {
    const result = observation.elements.find(item => item.name === name);
    assert.ok(result, `Missing ${name}; nodes=${JSON.stringify(observation.elements.map(e => ({ role: e.role, name: e.name })))}`);
    return result;
  }
  let observation = await observe(true);
  assert.ok(observation.elements.length > 3);
  assert.equal(observation.images.length, 1);
  const image = observation.images[0];
  const png = Buffer.from(image.data, 'base64'); assert.equal(png.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
  assert.equal(image.capture_method, 'windows-graphics-capture');
  assert.ok(!JSON.stringify(observation.elements).includes('FIXTURE-PASSWORD-DO-NOT-EXPOSE'));
  await writeFile(resolve(run, 'window.png'), png);
  record('UIA tree, password redaction, and actual WGC PNG');

  const button = element(observation, 'Fixture Increment');
  const click = { kind: 'click', button: 'left', target: { kind: 'element', element_id: button.id } };
  const actionId = 'fixture-click-once';
  const first = await runtime.act(observation.id, click, actionId); assert.equal(first.state, 'dispatched', first.message);
  await waitFor(value => value.count === 1, 'element click');
  const replay = await runtime.act(observation.id, click, actionId); assert.deepEqual(replay, first);
  await sleep(200); assert.equal((await snapshot()).count, 1);
  assert.deepEqual(await runtime.actionStatus(actionId), first);
  await assert.rejects(runtime.act(observation.id, { kind: 'key', keys: ['Enter'] }, actionId), error => error.code === 'action_id_conflict');
  record('Element click and action-ID deduplication without repeated input');

  observation = await observe(true);
  const b = element(observation, 'Fixture Increment').bounds;
  const img = observation.images[0];
  const target = { kind: 'image', image_id: img.id, x: (b.x + b.width / 2 - img.screen_bounds.x) * img.width / img.screen_bounds.width, y: (b.y + b.height / 2 - img.screen_bounds.y) * img.height / img.screen_bounds.height };
  const coordinate = await runtime.act(observation.id, { kind: 'click', button: 'left', target }); assert.equal(coordinate.state, 'dispatched', coordinate.message);
  await waitFor(value => value.count === 2, 'image-coordinate click'); record('Image coordinate mapping and real button result');

  observation = await observe();
  let outcome = await runtime.act(observation.id, { kind: 'click', button: 'left', target: { kind: 'element', element_id: element(observation, 'Fixture Input').id } }); assert.equal(outcome.state, 'dispatched', outcome.message);
  observation = await observe(); outcome = await runtime.act(observation.id, { kind: 'key', keys: ['Ctrl', 'a'] }); assert.equal(outcome.state, 'dispatched', outcome.message);
  observation = await observe(); outcome = await runtime.act(observation.id, { kind: 'type_text', text: 'Hello 中文' }); assert.equal(outcome.state, 'dispatched', outcome.message);
  await waitFor(value => value.text === 'Hello 中文', 'Unicode input');
  observation = await observe(); assert.ok(observation.elements.some(e => e.value === 'Hello 中文'));
  record('Focus, key chord, Chinese/English input, and re-observation');

  const consumed = await runtime.act(observation.id, { kind: 'key', keys: ['Tab'] });
  assert.equal(consumed.state, 'dispatched', consumed.message);
  assert.equal((await runtime.act(observation.id, { kind: 'key', keys: ['Tab'] })).state, 'rejected');
  const previous = observation; observation = await observe();
  assert.equal((await runtime.act(previous.id, { kind: 'key', keys: ['Tab'] })).state, 'rejected');
  const badRef = await runtime.act(observation.id, { kind: 'click', button: 'left', target: { kind: 'element', element_id: 'not-in-this-observation' } });
  assert.equal(badRef.state, 'rejected');
  record('Consumed observations, superseded observations and foreign element references are rejected');

  observation = await observe();
  const scrolling = await runtime.act(observation.id, { kind: 'scroll', target: { kind: 'element', element_id: element(observation, 'Fixture Scroll Region').id }, dx: 0, dy: 3 });
  assert.equal(scrolling.state, 'dispatched', scrolling.message);
  await waitFor(value => value.scroll_y > 0, 'actual panel scrolling');
  observation = await observe(true);
  await writeFile(resolve(run, 'after-input.png'), Buffer.from(observation.images[0].data, 'base64'));
  record('Real scroll operation changes the fixture scroll position');

  const completedTarget = { kind: 'element', element_id: element(observation, 'Fixture Increment').id };
  const completedDrag = await runtime.act(observation.id, { kind: 'drag', from: completedTarget, to: completedTarget, duration_ms: 100 });
  assert.equal(completedDrag.state, 'dispatched', completedDrag.message);
  await waitFor(value => !value.mouse_down && !value.modifiers_down, 'completed drag input release');
  record('Uninterrupted drag completes and releases held inputs');
  observation = await observe();

  const contender = new Computer({ helperPath, policyPath }); runtimes.push(contender);
  await assert.rejects(contender.start(), error => error.code === 'desktop_busy'); await contender.stop();
  record('Cross-process control lease rejects a second helper');

  const old = observation;
  const beforeMove = await snapshot();
  await writeFile(oracle + '.control', 'move');
  await waitFor(value => value.x === beforeMove.x + 40 && value.y === beforeMove.y + 30, 'fixture geometry change');
  outcome = await runtime.act(old.id, { kind: 'key', keys: ['Tab'] }); assert.equal(outcome.state, 'rejected'); assert.match(outcome.message, /stale_window/);
  record('Moved window rejects a stale observation before dispatch');

  observation = await observe();
  const endpoint = { kind: 'element', element_id: element(observation, 'Fixture Increment').id };
  const pending = runtime.act(observation.id, { kind: 'drag', from: endpoint, to: endpoint, duration_ms: 3000 });
  await waitFor(value => value.mouse_down, 'actual injected mouse-down during drag');
  const stoppedAt = Date.now(); await runtime.stop(); outcome = await pending;
  assert.equal(outcome.state, 'unknown', 'Interrupted input must not be advertised as safe to retry');
  await waitFor(value => !value.mouse_down && !value.modifiers_down, 'stop input release');
  assert.ok(Date.now() - stoppedAt < 5000);
  record('Stop preempts a held drag, reports unknown, and releases inputs');

  const successor = new Computer({ helperPath, policyPath }); runtimes.push(successor); await successor.start(); await successor.stop();
  record('Lease is available only after the old executor stops');

  const eof = new HelperTransport({ helperPath, policyPath });
  try {
    const started = await eof.request('start'); const session_id = started.session_id;
    const windows = await eof.request('list_windows', { session_id });
    const state = await eof.request('observe', { session_id, window: windows[0], screenshot: false, accessibility: true });
    const target = { kind: 'element', element_id: element(state, 'Fixture Increment').id };
    const pending = eof.request('act', { session_id, observation_id: state.id, action_id: 'eof-drag', action: { kind: 'drag', from: target, to: target, duration_ms: 3000 } });
    await waitFor(value => value.mouse_down, 'EOF drag mouse-down');
    await eof.close(); const result = await pending; assert.equal(result.state, 'unknown');
    await waitFor(value => !value.mouse_down && !value.modifiers_down, 'EOF input release');
  } finally { await eof.close(); }
  record('Actual stdin EOF cancels a held drag and releases mouse/modifiers');

  const ttlPolicy = resolve(run, 'ttl-policy.json');
  const policy = JSON.parse(await readFile(policyPath, 'utf8'));
  await writeFile(ttlPolicy, JSON.stringify({ ...policy, session_seconds: 3 }));
  const ttl = new Computer({ helperPath, policyPath: ttlPolicy }); runtimes.push(ttl);
  const deadline = await ttl.start(); const ttlWindows = await ttl.listWindows();
  const ttlState = await ttl.observe(ttlWindows[0], { screenshot: false, accessibility: true });
  await sleep(Math.max(0, deadline.expires_at_ms - Date.now() - 1200));
  const ttlTarget = { kind: 'element', element_id: element(ttlState, 'Fixture Increment').id };
  const ttlPending = ttl.act(ttlState.id, { kind: 'drag', from: ttlTarget, to: ttlTarget, duration_ms: 3000 });
  await waitFor(value => value.mouse_down, 'TTL drag mouse-down');
  assert.equal((await ttlPending).state, 'unknown');
  assert.ok(Date.now() < deadline.expires_at_ms + 1500, 'Expiry cancels during input, not after the full drag');
  await waitFor(value => !value.mouse_down && !value.modifiers_down, 'TTL input release'); await ttl.stop();
  record('Session TTL independently preempts input and releases held state');

  const proxyPath = resolve(run, 'LostResponseProxy.exe');
  const proxyCompile = spawnSync(compiler, ['/nologo', '/target:exe', `/out:${proxyPath}`, '/r:System.Web.Extensions.dll', resolve(root, 'tests/fixtures/windows/LostResponseProxy.cs')], { encoding: 'utf8' });
  assert.equal(proxyCompile.status, 0, proxyCompile.stdout + proxyCompile.stderr);
  process.env.CU_TEST_REAL_HELPER = helperPath;
  let lossy;
  try { lossy = new Computer({ helperPath: proxyPath, policyPath, timeoutMs: 2500, onStderr: text => process.stderr.write(text) }); }
  finally { delete process.env.CU_TEST_REAL_HELPER; }
  runtimes.push(lossy); await lossy.start();
  const lossWindows = await lossy.listWindows(); const lossState = await lossy.observe(lossWindows[0], { screenshot: false, accessibility: true });
  const beforeLoss = (await snapshot()).count;
  await assert.rejects(lossy.act(lossState.id, { kind: 'click', button: 'left', target: { kind: 'element', element_id: element(lossState, 'Fixture Increment').id } }, 'lost-response'), error => error instanceof UnknownExecutionError);
  await waitFor(value => value.count === beforeLoss + 1, 'real click despite lost response');
  assert.equal((await lossy.actionStatus('lost-response')).state, 'dispatched');
  await sleep(300); assert.equal((await snapshot()).count, beforeLoss + 1); await lossy.stop();
  record('Lost real action response produces unknown, without automatically repeating input');

  const deniedPolicy = resolve(run, 'denied-policy.json');
  await writeFile(deniedPolicy, JSON.stringify({ ...policy, allowed_pids: [], allowed_apps: [] }));
  const deniedRuntime = new Computer({ helperPath, policyPath: deniedPolicy }); runtimes.push(deniedRuntime);
  await assert.rejects(deniedRuntime.start(), error => error.code === 'permission_denied'); await deniedRuntime.stop();
  const restrictedPolicy = resolve(run, 'restricted-policy.json');
  await writeFile(restrictedPolicy, JSON.stringify({ ...policy, allow_input: false, allow_screenshot: false }));
  const restricted = new Computer({ helperPath, policyPath: restrictedPolicy }); runtimes.push(restricted); await restricted.start();
  const restrictedWindows = await restricted.listWindows();
  await assert.rejects(restricted.observe(restrictedWindows[0]), error => error.code === 'permission_denied');
  const readOnly = await restricted.observe(restrictedWindows[0], { screenshot: false });
  const countBeforeDenied = (await snapshot()).count;
  assert.equal((await restricted.act(readOnly.id, { kind: 'key', keys: ['Enter'] })).state, 'rejected');
  assert.equal((await snapshot()).count, countBeforeDenied); await restricted.stop();
  record('Default-deny host policy and separate capture/input permission checks');

  const require = createRequire(resolve(root, 'typescript/package.json'));
  const { Client } = await import(pathToFileURL(require.resolve('@modelcontextprotocol/sdk/client/index.js')).href);
  const { StdioClientTransport } = await import(pathToFileURL(require.resolve('@modelcontextprotocol/sdk/client/stdio.js')).href);
  const transport = new StdioClientTransport({ command: process.execPath, args: [resolve(root, 'typescript/packages/mcp/dist/main.js'), '--helper', helperPath, '--policy', policyPath], stderr: 'pipe' });
  transport.stderr?.on('data', chunk => process.stderr.write(chunk));
  mcp = new Client({ name: 'fixture-client', version: '0.1.0' }); await mcp.connect(transport);
  const tools = await mcp.listTools(); assert.equal(tools.tools.length, 11);
  const denied = await mcp.callTool({ name: 'computer_start', arguments: { allowed_pids: [fixture.pid] } }); assert.equal(denied.isError, true);
  const start = await mcp.callTool({ name: 'computer_start', arguments: {} }); assert.ok(!start.isError);
  const listed = await mcp.callTool({ name: 'computer_list_windows', arguments: {} }); const windows = listed.structuredContent.value;
  const observed = await mcp.callTool({ name: 'computer_observe', arguments: { window: windows[0], accessibility: true } }); assert.ok(!observed.isError, JSON.stringify(observed));
  assert.ok(observed.content.some(c => c.type === 'image' && Buffer.from(c.data, 'base64').subarray(0, 8).toString('hex') === '89504e470d0a1a0a'));
  const state = observed.structuredContent; const checkbox = state.elements.find(e => e.name === 'Fixture Option'); assert.ok(checkbox);
  const acted = await mcp.callTool({ name: 'computer_act', arguments: { observation_id: state.id, action_id: 'mcp-check-option', action: { kind: 'click', button: 'left', target: { kind: 'element', element_id: checkbox.id } } } });
  assert.equal(acted.structuredContent.state, 'dispatched', JSON.stringify(acted)); await waitFor(value => value.option, 'MCP checkbox input');
  const mcpSemanticState = (await mcp.callTool({ name: 'computer_observe', arguments: { window: windows[0], accessibility: true, screenshot: false } })).structuredContent;
  const mcpBefore = await snapshot();
  const mcpSemantic = await mcp.callTool({ name: 'computer_act', arguments: { observation_id: mcpSemanticState.id, action_id: 'mcp-semantic-invoke', action: { kind: 'invoke', element_id: element(mcpSemanticState, 'Fixture Increment').id } } });
  assert.equal(mcpSemantic.structuredContent.mechanism, 'uia-invoke', JSON.stringify(mcpSemantic));
  await waitFor(a => a.count === mcpBefore.count + 1, 'MCP semantic invoke');
  const stop = await mcp.callTool({ name: 'computer_stop', arguments: {} }); assert.ok(!stop.isError);
  await mcp.close(); mcp = undefined; record('MCP discovery, host-only authorization, real image content, input and stop');
  const semantic = new Computer({ helperPath, policyPath }); runtimes.push(semantic);
  await semantic.start();
  const semanticObserve = async () => semantic.observe((await semantic.listWindows())[0], { screenshot: false, accessibility: true });
  const unchangedPointer = (before, after, requireForeground = true) => {
    assert.equal(after.cursor_x, before.cursor_x); assert.equal(after.cursor_y, before.cursor_y);
    assert.equal(after.mouse_messages, before.mouse_messages, 'Semantic action must not post mouse input');
    if (requireForeground) assert.equal(after.foreground_hwnd, before.foreground_hwnd, 'Foreground changed during foreground-fixture semantic action');
  };
  for (const [kind, name, mechanism, changed] of [
    ['invoke', 'Fixture Increment', 'uia-invoke', (a, b) => a.count === b.count + 1],
    ['toggle', 'Fixture Option', 'uia-toggle', (a, b) => a.option !== b.option],
    ['select', 'Choice B', 'uia-selection', a => a.selected === 1],
    ['set_value', 'Fixture Input', 'uia-value-pattern', a => a.text === 'UIA replacement 中文'],
  ]) {
    const state = await semanticObserve(); const node = element(state, name);
    assert.ok(node.capabilities?.includes(kind), `${name} lacks ${kind}: ${JSON.stringify(node)}`);
    await sleep(150); const before = await snapshot();
    const action = { kind, element_id: node.id, ...(kind === 'set_value' ? { value: 'UIA replacement 中文' } : {}) };
    const id = `semantic-${kind}`;
    const result = await semantic.act(state.id, action, id);
    assert.equal(result.state, 'dispatched', result.message); assert.equal(result.mechanism, mechanism);
    const after = await waitFor(a => changed(a, before), kind);
    unchangedPointer(before, after);
    if (kind === 'set_value') assert.equal(after.key_messages, before.key_messages);
    assert.deepEqual(await semantic.act(state.id, action, id), result);
    await sleep(150); unchangedPointer(before, await snapshot());
    record(`UIA ${kind}: real state, mechanism, no pointer movement or mouse messages, dedupe`);
  }
  let semanticState = await semanticObserve();
  const surface = element(semanticState, 'Fixture Physical Surface');
  assert.ok(!surface.capabilities?.includes('invoke'));
  const beforeSurface = await snapshot();
  const rejected = await semantic.act(semanticState.id, { kind: 'invoke', element_id: surface.id });
  assert.equal(rejected.state, 'rejected'); await sleep(150); unchangedPointer(beforeSurface, await snapshot());
  semanticState = await semanticObserve();
  const physical = await semantic.act(semanticState.id, { kind: 'click', button: 'left', target: { kind: 'element', element_id: element(semanticState, 'Fixture Physical Surface').id } });
  assert.equal(physical.state, 'dispatched', physical.message); assert.equal(physical.mechanism, 'sendinput');
  const afterSurface = await waitFor(a => a.fallback_count === beforeSurface.fallback_count + 1, 'explicit physical surface click');
  assert.equal(afterSurface.mouse_messages, beforeSurface.mouse_messages + 1);
  record('Unsupported semantic action rejects without input; explicit new physical click dispatches once');
  const semanticImage = await semantic.observe((await semantic.listWindows())[0], { screenshot: true, accessibility: true });
  await writeFile(resolve(run, 'after-semantic.png'), Buffer.from(semanticImage.images[0].data, 'base64'));
  await writeFile(oracle + '.control', 'background');
  await waitFor(a => a.sentinel_hwnd !== 0 && a.foreground_hwnd === a.sentinel_hwnd, 'foreground sentinel');
  const backgroundState = await semantic.observe((await semantic.listWindows()).find(w => w.title.startsWith('Computer Use Fixture ')), { screenshot: false, accessibility: true });
  const beforeBackground = await snapshot();
  const backgroundInvoke = await semantic.act(backgroundState.id, { kind: 'invoke', element_id: element(backgroundState, 'Fixture Increment').id });
  assert.equal(backgroundInvoke.state, 'dispatched', backgroundInvoke.message);
  const afterBackground = await waitFor(a => a.count === beforeBackground.count + 1, 'background UIA invoke');
  unchangedPointer(beforeBackground, afterBackground, false);
  const foregroundChanged = beforeBackground.foreground_hwnd !== afterBackground.foreground_hwnd;
  outcomes.push({ name: 'Background UIA provider activation observation', passed: true, foreground_changed: foregroundChanged, before: beforeBackground.foreground_hwnd, after: afterBackground.foreground_hwnd });
  record(`Background UIA invoke preserves pointer; provider foreground_changed=${foregroundChanged} (not input isolation)`);
  await semantic.stop();
  process.env.CU_TEST_REAL_HELPER = helperPath;
  let semanticLoss;
  try { semanticLoss = new Computer({ helperPath: proxyPath, policyPath, timeoutMs: 2500 }); }
  finally { delete process.env.CU_TEST_REAL_HELPER; }
  runtimes.push(semanticLoss); await semanticLoss.start();
  const semanticLossState = await semanticLoss.observe((await semanticLoss.listWindows()).find(w => w.title.startsWith('Computer Use Fixture ')), { screenshot: false, accessibility: true });
  const beforeSemanticLoss = await snapshot();
  await assert.rejects(semanticLoss.act(semanticLossState.id, { kind: 'invoke', element_id: element(semanticLossState, 'Fixture Increment').id }, 'lost-semantic-response'), error => error instanceof UnknownExecutionError);
  await waitFor(a => a.count === beforeSemanticLoss.count + 1, 'semantic invoke despite lost response');
  assert.equal((await semanticLoss.actionStatus('lost-semantic-response')).mechanism, 'uia-invoke');
  await sleep(300); assert.equal((await snapshot()).count, beforeSemanticLoss.count + 1);
  assert.equal((await snapshot()).mouse_messages, beforeSemanticLoss.mouse_messages);
  await semanticLoss.stop();
  record('Lost real UIA response is unknown to caller, no mouse fallback and no replay');
  const legacy = new HelperTransport({ helperPath, policyPath });
  try {
    const caps = await legacy.request('capabilities'); assert.equal(caps.protocol_version, 2); assert.ok(!caps.actions.includes('invoke'));
    const { session_id } = await legacy.request('start');
    const window = (await legacy.request('list_windows', { session_id })).find(w => w.title.startsWith('Computer Use Fixture '));
    const state = await legacy.request('observe', { session_id, window, screenshot: false, accessibility: true });
    assert.ok(state.elements.every(e => !Object.hasOwn(e, 'capabilities')));
    await assert.rejects(legacy.request('act', { session_id, observation_id: state.id, action_id: 'v1-semantic-denied', action: { kind: 'invoke', element_id: element(state, 'Fixture Increment').id } }), error => error.code === 'unsupported_action');
    await legacy.request('stop', { session_id });
  } finally { await legacy.close(); }
  record('Legacy profile 1 omits new fields/actions and rejects semantic dispatch');
  const activation = new Computer({ helperPath, policyPath }); runtimes.push(activation);
  await activation.start();
  for (const maximized of [true, false]) {
    await writeFile(oracle + '.control', maximized ? 'maximize-background' : 'normal-background');
    await waitFor(a => a.maximized === maximized && a.foreground_hwnd === a.sentinel_hwnd, 'background fixture geometry');
    const window = (await activation.listWindows()).find(w => w.title.startsWith('Computer Use Fixture '));
    await activation.activateWindow(window);
    assert.deepEqual((await activation.listWindows()).find(w => w.id === window.id).bounds, window.bounds);
    await writeFile(oracle + '.control', maximized ? 'maximize-background' : 'normal-background');
    await waitFor(a => a.maximized === maximized && a.foreground_hwnd === a.sentinel_hwnd, 'background fixture before physical input');
    const state = await activation.observe(window, { screenshot: false, accessibility: true });
    const result = await activation.act(state.id, { kind: 'key', keys: ['Tab'] });
    assert.equal(result.state, 'dispatched', result.message);
    assert.deepEqual((await activation.listWindows()).find(w => w.id === window.id).bounds, window.bounds);
    await waitFor(a => a.maximized === maximized && a.foreground_hwnd === Number(window.id), 'activated fixture geometry');
  }
  record('Explicit activation and physical input preserve maximized and normal window geometry');
  await writeFile(oracle + '.control', 'untitled');
  let untitled;
  for (let i = 0; i < 50; i++) {
    const sentinelId = String((await snapshot()).sentinel_hwnd);
    untitled = (await activation.listWindows()).find(w => w.id === sentinelId && w.title === '');
    if (untitled) break;
    await sleep(100);
  }
  assert.ok(untitled, 'Approved untitled popup must remain discoverable');
  const popup = await activation.observe(untitled, { screenshot: true, accessibility: true });
  assert.equal(popup.window.id, untitled.id);
  await writeFile(resolve(run, 'untitled-popup.png'), Buffer.from(popup.images[0].data, 'base64'));
  record('Untitled approved windows are listed and observable');
  await activation.stop();
  await writeFile(resolve(run, 'results.json'), JSON.stringify({ platform: process.platform, scope: 'Windows interactive fixture only', outcomes, screenshot: resolve(run, 'window.png') }, null, 2));
  console.log(`GUI evidence: ${run}`);
} finally {
  if (mcp) await mcp.close().catch(() => {});
  for (const runtime of runtimes) await runtime.stop().catch(() => {});
  await writeFile(oracle + '.control', 'close').catch(() => {});
  if (fixture.exitCode === null) { await Promise.race([new Promise(resolve => fixture.once('exit', resolve)), sleep(2000)]); if (fixture.exitCode === null) fixture.kill(); }
}
