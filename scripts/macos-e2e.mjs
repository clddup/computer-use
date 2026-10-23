import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile, rename } from 'node:fs/promises';
import { resolve, dirname, basename } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import { Computer } from '../typescript/packages/sdk/dist/index.js';
import { HelperTransport } from '../typescript/packages/runtime/dist/index.js';

if (process.platform !== 'darwin') throw new Error('This fixture requires a macOS interactive desktop.');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
await mkdir(resolve(root, '.local'), { recursive: true });
const run = await mkdtemp(resolve(root, '.local/macos-gui-'));
const helperPath = resolve(process.env.CU_HELPER ?? resolve(root, 'native/target/debug/cu-helper'));
const executable = resolve(run, 'Fixture');
const compile = spawnSync('xcrun', ['swiftc', resolve(root, 'tests/fixtures/macos/Fixture.swift'), '-o', executable], { encoding: 'utf8' });
assert.equal(compile.status, 0, `Fixture compilation failed: ${compile.stdout}\n${compile.stderr}`);
const oracle = resolve(run, 'oracle.json');
const title = `Computer Use ${basename(run)}`;
const outcomes = [];
const runtimes = [];
let fixture, mcp, fixtureWindowId;
let status = 'failed';
let failure;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function snapshot() {
  try { return JSON.parse(await readFile(oracle, 'utf8')); }
  catch (error) { if (error.code === 'ENOENT') return undefined; throw error; }
}
async function waitFor(test, label, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const data = await snapshot();
    if (data && test(data)) return data;
    if (fixture?.exitCode !== null && fixture?.exitCode !== undefined) throw new Error(`Fixture exited: ${fixture.exitCode}`);
    await sleep(25);
  }
  throw new Error(`Timed out waiting for ${label}; oracle=${JSON.stringify(await snapshot())}`);
}
function record(name) { outcomes.push({ name, passed: true }); console.log(`PASS ${name}`); }
function element(observation, name) {
  const matches = observation.elements.filter(item => item.name === name && item.bounds);
  const result = matches.find(item => item.editable) ?? matches.find(item => item.role !== 'AXStaticText') ?? matches[0];
  assert.ok(result, `Missing ${name}; nodes=${JSON.stringify(observation.elements.map(e => ({ role: e.role, name: e.name })))}`);
  return result;
}
function target(observation, name) { return { kind: 'element', element_id: element(observation, name).id }; }
function dispatched(outcome) {
  assert.equal(outcome.state, 'dispatched', outcome.message);
  assert.equal(outcome.mechanism, 'coregraphics-targeted');
}
async function policyFile(name, overrides = {}) {
  const path = resolve(run, name);
  await writeFile(path, JSON.stringify({ allowed_pids: [fixture.pid], allow_input: true, allow_screenshot: true, allow_accessibility: true, session_seconds: 180, ...overrides }));
  return path;
}
function computer(policyPath) {
  const runtime = new Computer({ helperPath, policyPath, onStderr: text => process.stderr.write(text) });
  runtimes.push(runtime);
  return runtime;
}
async function mainWindow(runtime) {
  const windows = await runtime.listWindows();
  assert.ok(windows.every(window => window.pid === fixture.pid));
  const state = await snapshot();
  const window = windows.find(window => window.id === state.window_id);
  assert.ok(window, `Missing fixture window: ${JSON.stringify(windows)}`);
  return window;
}
async function observe(runtime, screenshot = false) {
  return runtime.observe(await mainWindow(runtime), { screenshot, accessibility: true });
}
let commandId = 0;
async function control(kind) {
  const id = ++commandId;
  const temporary = resolve(run, 'command.tmp');
  await writeFile(temporary, JSON.stringify({ id, kind }));
  await rename(temporary, resolve(run, 'command.json'));
  await waitFor(value => value.command_id === id, `fixture control ${kind}`);
}
async function released(label) {
  return waitFor(value => !value.mouse_down && !value.pointer_down && !value.held_modifiers.some(name => ['command', 'shift', 'control', 'alt'].includes(name)), label);
}
try {
  const probe = computer();
  const caps = await probe.capabilities();
  await probe.stop();
  await writeFile(resolve(run, 'capabilities.json'), JSON.stringify(caps, null, 2));
  const permissionProbe = spawnSync(helperPath, ['--permissions'], { encoding: 'utf8' });
  assert.equal(permissionProbe.status, 0, permissionProbe.stderr);
  const permissions = JSON.parse(permissionProbe.stdout);
  await writeFile(resolve(run, 'permissions.json'), JSON.stringify(permissions, null, 2));
  if ((!caps.available || !permissions.accessibility || !permissions.screen_recording) && !process.argv.includes('--fixture-smoke')) {
    status = 'blocked';
    throw new Error(`GUI blocked: Accessibility=${permissions.accessibility}, Screen Recording=${permissions.screen_recording}. Request official macOS prompts with: "${helperPath}" --request-permissions, then enable the responsible host/helper in System Settings and rerun. No GUI input was attempted.`);
  }
  fixture = spawn(executable, [oracle, title], { stdio: ['ignore', 'ignore', 'pipe'] });
  let fixtureError;
  fixture.once('error', error => { fixtureError = error; });
  fixture.stderr.on('data', data => process.stderr.write(data));
  const initial = await waitFor(value => { if (fixtureError) throw fixtureError; return value.pid === fixture.pid && value.ready; }, 'fixture startup');
  fixtureWindowId = initial.window_id;
  await sleep(300);
  record('Native AppKit fixture starts and publishes an atomic synthetic-state oracle');
  if (process.argv.includes('--fixture-smoke')) {
    status = 'smoke-only';
    console.log('Fixture launch only; no capture or input was tested.');
  } else {
    const policyPath = await policyFile('policy.json');
    const runtime = computer(policyPath);
    await runtime.start();
    let observation;
    try { observation = await observe(runtime, true); }
    catch (error) {
      if (error.code === 'permission_denied') status = 'blocked';
      throw error;
    }
    assert.ok(observation.elements.length > 3);
    assert.ok(observation.elements.some(e => e.password), 'Fixture secure field must be identified');
    assert.ok(!JSON.stringify(observation.elements).includes('FIXTURE-PASSWORD-DO-NOT-EXPOSE'));
    assert.equal(observation.images.length, 1);
    const image = observation.images[0];
    assert.equal(image.capture_method, 'screencapturekit-stream');
    assert.equal(Buffer.from(image.data, 'base64').subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
    await writeFile(resolve(run, 'window.png'), Buffer.from(image.data, 'base64'));
    record('AX tree, password redaction, and real ScreenCaptureKit PNG');

    const click = { kind: 'click', button: 'left', target: target(observation, 'Fixture Increment') };
    const first = await runtime.act(observation.id, click, 'click-once'); dispatched(first);
    await waitFor(value => value.count === 1, 'element click');
    assert.deepEqual(await runtime.act(observation.id, click, 'click-once'), first);
    assert.deepEqual(await runtime.actionStatus('click-once'), first);
    await assert.rejects(runtime.act(observation.id, { kind: 'key', keys: ['Tab'] }, 'click-once'), error => error.code === 'action_id_conflict');
    await sleep(150); assert.equal((await snapshot()).count, 1);
    record('Element click, action journal, conflict rejection and no duplicate input');

    observation = await observe(runtime, true);
    const b = element(observation, 'Fixture Increment').bounds;
    const img = observation.images[0];
    const pixel = { kind: 'image', image_id: img.id, x: (b.x + b.width / 2 - img.screen_bounds.x) * img.width / img.screen_bounds.width, y: (b.y + b.height / 2 - img.screen_bounds.y) * img.height / img.screen_bounds.height };
    dispatched(await runtime.act(observation.id, { kind: 'click', button: 'left', target: pixel }));
    await waitFor(value => value.count === 2, 'pixel-coordinate click');
    record('Image pixel mapping hits the real AppKit button');

    for (const [name, text, field] of [
      ['Fixture Input', 'Hello 中文 👋', 'text'],
      ['Fixture Multiline Input', '第一行 Hello\n第二行 🌍', 'multiline_text'],
    ]) {
      observation = await observe(runtime);
      dispatched(await runtime.act(observation.id, { kind: 'click', button: 'left', target: target(observation, name) }));
      observation = await observe(runtime);
      dispatched(await runtime.act(observation.id, { kind: 'key', keys: ['Command', 'a'] }));
      observation = await observe(runtime);
      dispatched(await runtime.act(observation.id, { kind: 'type_text', text }));
      await waitFor(value => value[field] === text, `${name} Unicode input`);
      observation = await observe(runtime);
      assert.ok(observation.elements.some(e => e.value === text), 'AX re-observation must confirm input');
      record(`${name}: focus, Command+A, Unicode input and AX re-observation`);
    }
    for (const click_count of [2, 3]) {
      observation = await observe(runtime);
      dispatched(await runtime.act(observation.id, { kind: 'click', button: 'left', click_count, target: target(observation, 'Fixture Drag Region') }));
      await waitFor(value => value.click_count === click_count && !value.mouse_down, `native ${click_count}-click state`);
    }
    record('Native double/triple-click counts reach NSEvent');

    observation = await observe(runtime);
    const beforeScroll = (await snapshot()).scroll_y;
    dispatched(await runtime.act(observation.id, { kind: 'scroll', target: target(observation, 'Fixture Scroll Region'), dx: 0, dy: 3 }));
    await waitFor(value => value.scroll_y !== beforeScroll, 'scroll content change');
    record('Scroll changes actual AppKit content offset');

    observation = await observe(runtime);
    const dragTarget = target(observation, 'Fixture Drag Region');
    const beforeDrag = (await snapshot()).drag_count;
    dispatched(await runtime.act(observation.id, { kind: 'drag', from: dragTarget, to: dragTarget, duration_ms: 300 }));
    await waitFor(value => value.drag_count > beforeDrag && !value.mouse_down, 'completed drag');
    await released('completed drag release');
    record('Completed drag produces real events and releases input');

    observation = await observe(runtime);
    dispatched(await runtime.act(observation.id, { kind: 'key', keys: ['Tab'] }));
    assert.equal((await runtime.act(observation.id, { kind: 'key', keys: ['Tab'] })).state, 'rejected');
    const previous = await observe(runtime); observation = await observe(runtime);
    assert.equal((await runtime.act(previous.id, { kind: 'key', keys: ['Tab'] })).state, 'rejected');
    assert.equal((await runtime.act(observation.id, { kind: 'click', button: 'left', target: { kind: 'element', element_id: 'foreign' } })).state, 'rejected');
    record('Consumed/superseded observations and foreign references reject');

    const contender = computer(policyPath);
    await assert.rejects(contender.start(), error => error.code === 'desktop_busy'); await contender.stop();
    record('Second helper cannot acquire the active desktop lease');

    observation = await observe(runtime); await control('move');
    const stale = await runtime.act(observation.id, { kind: 'key', keys: ['Tab'] });
    assert.equal(stale.state, 'rejected'); assert.match(stale.message, /stale_window/);
    record('Moved fixture invalidates old geometry before input dispatch');
    await control('focus_other');
    const backgroundWindow = await mainWindow(runtime);
    await runtime.activateWindow(backgroundWindow);
    assert.deepEqual((await mainWindow(runtime)).bounds, backgroundWindow.bounds);
    record('Explicit activation preserves geometry');
    await control('focus_other');
    observation = await observe(runtime);
    const keyWindow = (await snapshot()).key_window_id;
    const wrongKeyboardWindow = await runtime.act(observation.id, { kind: 'key', keys: ['Tab'] });
    assert.notEqual(wrongKeyboardWindow.state, 'dispatched');
    assert.match(wrongKeyboardWindow.message, /focus_changed/);
    assert.equal((await snapshot()).key_window_id, keyWindow);
    assert.deepEqual((await mainWindow(runtime)).bounds, observation.window.bounds);
    record('Targeted keyboard input refuses another focused window rather than implicitly raising the selected one');
    await control('focus_main');

    observation = await observe(runtime);
    const pending = runtime.act(observation.id, { kind: 'drag', from: target(observation, 'Fixture Drag Region'), to: target(observation, 'Fixture Drag Region'), duration_ms: 3000 });
    const pendingResult = pending.then(value => ({ value }), error => ({ error }));
    await waitFor(value => value.mouse_down && value.pointer_down, 'actual held drag before stop');
    const stoppedAt = Date.now(); await runtime.stop();
    const stopped = await pendingResult; if (stopped.error) throw stopped.error;
    assert.equal(stopped.value.state, 'unknown'); await released('stop input release');
    assert.ok(Date.now() - stoppedAt < 5000);
    record('Stop interrupts a real held drag, reports unknown and releases inputs');
    const successor = computer(policyPath); await successor.start(); await successor.stop();
    record('Desktop lease becomes available after stopped input cleanup');

    const eof = new HelperTransport({ helperPath, policyPath });
    try {
      const { session_id } = await eof.request('start');
      const window = (await eof.request('list_windows', { session_id })).find(w => w.id === fixtureWindowId);
      const state = await eof.request('observe', { session_id, window, screenshot: false, accessibility: true });
      const endpoint = target(state, 'Fixture Drag Region');
      const action = eof.request('act', { session_id, observation_id: state.id, action_id: 'eof-drag', action: { kind: 'drag', from: endpoint, to: endpoint, duration_ms: 3000 } });
      const settled = action.then(value => ({ value }), error => ({ error }));
      await waitFor(value => value.mouse_down && value.pointer_down, 'EOF drag mouse-down');
      await eof.close(); const result = await settled; if (result.error) throw result.error;
      assert.equal(result.value.state, 'unknown'); await released('EOF release');
    } finally { await eof.close(); }
    record('Actual stdin EOF cancels a held drag and releases input');

    const ttl = computer(await policyFile('ttl-policy.json', { session_seconds: 5 }));
    const deadline = await ttl.start(); observation = await observe(ttl);
    await sleep(Math.max(0, deadline.expires_at_ms - Date.now() - 1500));
    const endpoint = target(observation, 'Fixture Drag Region');
    const ttlAction = ttl.act(observation.id, { kind: 'drag', from: endpoint, to: endpoint, duration_ms: 3000 });
    const ttlResult = ttlAction.then(value => ({ value }), error => ({ error }));
    await waitFor(value => value.mouse_down && value.pointer_down, 'TTL drag mouse-down');
    const expired = await ttlResult; if (expired.error) throw expired.error;
    assert.equal(expired.value.state, 'unknown');
    assert.ok(Date.now() < deadline.expires_at_ms + 1500); await released('TTL input release'); await ttl.stop();
    record('Session TTL interrupts input independently and releases held state');

    const denied = computer(await policyFile('denied-policy.json', { allowed_pids: [] })); await assert.rejects(denied.start(), error => ['permission_denied', 'permissions_failed'].includes(error.code)); await denied.stop();
    const restricted = computer(await policyFile('restricted-policy.json', { allow_input: false, allow_screenshot: false }));
    await restricted.start(); const restrictedWindow = await mainWindow(restricted);
    await assert.rejects(restricted.observe(restrictedWindow), error => error.code === 'permission_denied');
    const readOnly = await restricted.observe(restrictedWindow, { screenshot: false });
    assert.equal((await restricted.act(readOnly.id, { kind: 'key', keys: ['Tab'] })).state, 'rejected'); await restricted.stop();
    record('Default deny and separate screenshot/input policy permissions');

    const require = createRequire(resolve(root, 'typescript/package.json'));
    const { Client } = await import(pathToFileURL(require.resolve('@modelcontextprotocol/sdk/client/index.js')).href);
    const { StdioClientTransport } = await import(pathToFileURL(require.resolve('@modelcontextprotocol/sdk/client/stdio.js')).href);
    const transport = new StdioClientTransport({ command: process.execPath, args: [resolve(root, 'typescript/packages/mcp/dist/main.js'), '--helper', helperPath, '--policy', policyPath], stderr: 'pipe' });
    transport.stderr?.on('data', data => process.stderr.write(data));
    mcp = new Client({ name: 'macos-fixture-client', version: '0.1.0' }); await mcp.connect(transport);
    assert.equal((await mcp.listTools()).tools.length, 11);
    assert.equal((await mcp.callTool({ name: 'computer_start', arguments: { allowed_pids: [fixture.pid] } })).isError, true);
    const started = await mcp.callTool({ name: 'computer_start', arguments: {} }); assert.ok(!started.isError, JSON.stringify(started));
    const listed = await mcp.callTool({ name: 'computer_list_windows', arguments: {} });
    const window = listed.structuredContent.value.find(w => w.id === fixtureWindowId); assert.ok(window);
    const observed = await mcp.callTool({ name: 'computer_observe', arguments: { window, accessibility: true } });
    assert.ok(!observed.isError, JSON.stringify(observed)); assert.ok(observed.content.some(c => c.type === 'image'));
    const state = observed.structuredContent; const before = (await snapshot()).count;
    const acted = await mcp.callTool({ name: 'computer_act', arguments: { observation_id: state.id, action_id: 'mcp-click', action: { kind: 'click', button: 'left', target: target(state, 'Fixture Increment') } } });
    assert.equal(acted.structuredContent.state, 'dispatched', JSON.stringify(acted));
    await waitFor(value => value.count === before + 1, 'MCP button result');
    const final = await mcp.callTool({ name: 'computer_observe', arguments: { window, accessibility: true } });
    assert.ok(!final.isError, JSON.stringify(final));
    const finalImage = final.content.find(c => c.type === 'image'); assert.ok(finalImage);
    await writeFile(resolve(run, 'after-input.png'), Buffer.from(finalImage.data, 'base64'));
    assert.ok(!(await mcp.callTool({ name: 'computer_stop', arguments: {} })).isError);
    await mcp.close(); mcp = undefined;
    record('Official MCP client: discovery, authorization, image, real click and stop');
    status = 'passed';
  }
} catch (error) {
  failure = error.stack ?? String(error);
  console.error(failure);
  process.exitCode = status === 'blocked' ? 2 : 1;
} finally {
  if (mcp) await mcp.close().catch(() => {});
  for (const runtime of runtimes) await runtime.stop().catch(error => { console.error(`Cleanup failed: ${error}`); process.exitCode = 1; status = 'failed'; });
  if (fixture && fixture.exitCode === null) {
    fixture.kill('SIGTERM');
    await Promise.race([new Promise(resolve => fixture.once('exit', resolve)), sleep(2000)]);
    if (fixture.exitCode === null && fixture.signalCode === null) { fixture.kill('SIGKILL'); process.exitCode = 1; status = 'failed'; }
  }
  await writeFile(resolve(run, 'results.json'), JSON.stringify({ platform: process.platform, status, scope: 'Synthetic AppKit fixture only; no personal applications', helperPath, outcomes, failure }, null, 2));
  console.log(`macOS GUI evidence: ${run} (${status})`);
}
