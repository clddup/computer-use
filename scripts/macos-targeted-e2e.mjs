import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile, appendFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';

if (process.platform !== 'darwin') throw new Error('This test requires a macOS interactive desktop.');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
await mkdir(resolve(root, '.local'), { recursive: true });
const run = await mkdtemp(resolve(root, '.local/macos-targeted-'));
const appPath = resolve(run, 'BackgroundFixture.app');
await mkdir(resolve(appPath, 'Contents/MacOS'), { recursive: true });
await mkdir(resolve(run, 'target'));
const executable = resolve(appPath, 'Contents/MacOS/Fixture');
const escapedOracle = resolve(run, 'target/oracle.json').replaceAll('&', '&amp;').replaceAll('<', '&lt;');
await writeFile(resolve(appPath, 'Contents/Info.plist'), `<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>org.independent-cu.background-fixture</string><key>CFBundleExecutable</key><string>Fixture</string><key>CFBundlePackageType</key><string>APPL</string><key>CUFixtureOracle</key><string>${escapedOracle}</string></dict></plist>`);
const helper = resolve(process.env.CU_HELPER ?? resolve(root, 'native/target/debug/cu-helper'));
const built = spawnSync('xcrun', ['swiftc', resolve(root, 'tests/fixtures/macos/Fixture.swift'), '-o', executable], { encoding: 'utf8' });
assert.equal(built.status, 0, built.stderr);
const require = createRequire(resolve(root, 'typescript/package.json'));
const { Client } = await import(pathToFileURL(require.resolve('@modelcontextprotocol/sdk/client/index.js')).href);
const { StdioClientTransport } = await import(pathToFileURL(require.resolve('@modelcontextprotocol/sdk/client/stdio.js')).href);
const outcomes = [];
const isolationSamples = [];
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let client;
let failed;
let blocked = false;
let targetPid;

async function snapshot(name) {
  try { return JSON.parse(await readFile(resolve(run, name, 'oracle.json'), 'utf8')); }
  catch (error) { if (error.code === 'ENOENT') return undefined; throw error; }
}
async function waitFor(name, predicate, label) {
  const deadline = Date.now() + 8000;
  while (Date.now() < deadline) {
    const value = await snapshot(name);
    if (value && predicate(value)) return value;
    await sleep(25);
  }
  throw new Error(`${label}: ${JSON.stringify(await snapshot(name))}`);
}
function record(name) { outcomes.push({ name, passed: true }); console.log(`PASS ${name}`); }
async function call(name, args = {}) {
  const result = await client.callTool({ name, arguments: args });
  if (result.isError) throw Object.assign(new Error(`${name}: ${JSON.stringify(result)}`), { code: result.structuredContent?.code });
  return result.structuredContent;
}
async function selectedWindow(id) {
  const result = await call('computer_list_windows');
  const window = result.value.find(window => window.id === id);
  assert.ok(window, `Window ${id} missing`);
  return window;
}
async function observe(id) {
  for (let attempt = 0; attempt < 5; attempt++) {
    const window = await selectedWindow(id);
    const result = await client.callTool({ name: 'computer_observe', arguments: { window, screenshot: false, accessibility: true } });
    if (!result.isError) return result.structuredContent;
    assert.equal(result.structuredContent?.code, 'stale_window', JSON.stringify(result));
    await appendFile(resolve(run, 'stale-windows.jsonl'), JSON.stringify({ before: window, after: await selectedWindow(id) }) + '\n');
    await sleep(100);
  }
  throw new Error(`Window ${id} did not settle; see stale-windows.jsonl. No input was retried.`);
}
function elementTarget(observation, name) {
  const element = observation.elements.find(element => element.name === name && element.bounds && element.role !== 'AXStaticText');
  assert.ok(element, `Element ${name} missing: ${JSON.stringify(observation.elements.map(e => [e.role, e.name]))}`);
  return { kind: 'element', element_id: element.id };
}
async function act(observation, action, actionId) {
  const result = await call('computer_act', { observation_id: observation.id, action, ...(actionId ? { action_id: actionId } : {}) });
  assert.equal(result.state, 'dispatched');
  assert.equal(result.mechanism, 'coregraphics-targeted');
  return result;
}
async function click(id, name, actionId) {
  const state = await observe(id);
  return act(state, { kind: 'click', button: 'left', target: elementTarget(state, name) }, actionId);
}
function recordIsolationSample(before, after) {
  const pointerUnchanged = after.pointer_x === before.pointer_x && after.pointer_y === before.pointer_y;
  isolationSamples.push({ pointerUnchanged, before: [before.pointer_x, before.pointer_y], after: [after.pointer_x, after.pointer_y], foregroundBefore: before.foreground_pid, foregroundAfter: after.foreground_pid, frontWindowBefore: before.front_window_id, frontWindowAfter: after.front_window_id });
  assert.notEqual(after.foreground_pid, after.pid, 'Background target unexpectedly became the real foreground application');
  assert.notEqual(after.front_window_id, after.window_id, 'Background main window was unexpectedly raised above the user window');
  if (!pointerUnchanged) console.log('NOTE User pointer movement overlapped this MCP sample; not counted as a hardware-isolation assertion. Synchronous native checks verify cursor preservation.');
}

try {
  const policy = resolve(run, 'policy.json');
  await writeFile(policy, JSON.stringify({ desktop: true, allowed_pids: [], allow_input: true, allow_screenshot: false, allow_accessibility: true, session_seconds: 90 }));
  const transport = new StdioClientTransport({ command: process.execPath, args: [resolve(root, 'typescript/packages/mcp/dist/main.js'), '--helper', helper, '--policy', policy], stderr: 'pipe' });
  transport.stderr?.on('data', data => process.stderr.write(data));
  client = new Client({ name: 'targeted-input-acceptance', version: '0.1.0' });
  await client.connect(transport);
  const firstSession = await call('computer_start');
  record('Real MCP connection and AX-only synthetic session without Screen Recording request');
  const launched = await call('computer_launch_app', { app_id: appPath });
  const initial = await waitFor('target', value => value.ready, 'single background fixture ready');
  targetPid = initial.pid;
  assert.ok(launched.windows.some(window => window.pid === targetPid));
  assert.notEqual(initial.foreground_pid, targetPid, 'Application launch activated the background fixture');
  assert.notEqual(initial.front_window_id, initial.window_id, 'Application launch raised the background fixture');
  const main = initial.window_id;
  record('One application opens through MCP in the background without preconfigured PID or foreground sentinel');

  const beforeInvalid = await observe(main);
  const invalid = await client.callTool({ name: 'computer_act', arguments: {
    observation_id: beforeInvalid.id, action_id: 'unsupported-key', action: { kind: 'key', keys: ['not-a-key'] },
  } });
  assert.equal(invalid.isError, true);
  assert.equal(invalid.structuredContent.state, 'rejected');
  assert.equal(invalid.structuredContent.mechanism, 'none');
  assert.match(invalid.structuredContent.message, /unsupported_key/);
  assert.equal((await snapshot('target')).application_active, initial.application_active);
  record('Unsupported key is rejected before synthetic focus or input dispatch, not reported as unknown');

  const before = await snapshot('target');
  await click(main, 'Fixture Increment', 'background-click');
  const after = await waitFor('target', value => value.count === before.count + 1, 'background button result');
  assert.equal(after.application_active, true, 'Target did not receive synthetic AppKit activation');
  recordIsolationSample(before, after);
  record('Background window button changes oracle without activating or raising the target main window');

  await click(main, 'Fixture Input');
  await act(await observe(main), { kind: 'key', keys: ['cmd', 'a'] });
  await act(await observe(main), { kind: 'key', keys: ['Backspace'] });
  await act(await observe(main), { kind: 'type_text', text: '后台中文 123' });
  const typed = await waitFor('target', value => value.text === '后台中文 123', 'background Unicode input');
  assert.notEqual(typed.foreground_pid, targetPid);
  record('Targeted Unicode input changes the background target without activating it');

  const dragState = await observe(main);
  const region = dragState.elements.find(element => element.name === 'Fixture Drag Region');
  assert.ok(region?.bounds);
  const dragBefore = await snapshot('target');
  await act(dragState, { kind: 'click', button: 'left', target: { kind: 'element', element_id: region.id }, click_count: 2 });
  const double = await waitFor('target', value => value.down_count > dragBefore.down_count && value.click_count === 2, 'targeted double click');
  assert.ok(!double.mouse_down);
  recordIsolationSample(dragBefore, double);
  record('Targeted double click metadata and mouse-up cleanup');

  const dragObservation = await observe(main);
  const downBefore = await snapshot('target');
  await act(dragObservation, { kind: 'drag', from: elementTarget(dragObservation, 'Fixture Drag Region'), to: elementTarget(dragObservation, 'Fixture Input'), duration_ms: 300 });
  const dragged = await waitFor('target', value => value.drag_count > downBefore.drag_count && !value.mouse_down, 'background drag completion');
  recordIsolationSample(downBefore, dragged);
  record('Background drag follows the target window and releases its mouse state');

  const scrollObservation = await observe(main);
  const scrollBefore = await snapshot('target');
  await act(scrollObservation, { kind: 'scroll', target: elementTarget(scrollObservation, 'Fixture Scroll Region'), dx: 0, dy: 3 });
  const scrolled = await waitFor('target', value => value.scroll_events > scrollBefore.scroll_events && value.scroll_y !== scrollBefore.scroll_y, 'targeted scroll content change');
  assert.notEqual(scrolled.foreground_pid, targetPid);
  record('Background scroll delivers real target events and changes content offset');

  if (process.argv.includes('--popup')) {
    await click(main, 'Fixture Open Popup');
    const popupState = await waitFor('target', value => value.popup_visible, 'popup opens');
    const popup = await selectedWindow(popupState.popup_window_id);
    assert.equal(popup.pid, targetPid);
    const parent = await observe(main);
    assert.ok(parent.warnings.some(warning => warning.includes(popup.id)));
    const popupObserved = await observe(popup.id);
    const popupBefore = await snapshot('target');
    await act(popupObserved, { kind: 'click', button: 'left', target: elementTarget(popupObserved, 'Popup Increment') });
    const popupAfter = await waitFor('target', value => value.popup_count === popupBefore.popup_count + 1, 'popup increment');
    assert.ok(popupAfter.popup_visible, 'Observing/input must not dismiss the popup by raising the parent');
    recordIsolationSample(popupBefore, popupAfter);
    record('Layer-24 popup is enumerated, observed and clicked through public MCP without dismissing it');
  }

  const previousObservation = await observe(main);
  await call('computer_stop');
  await waitFor('target', value => !value.application_active, 'synthetic activation released after stop');
  const nextSession = await call('computer_start');
  assert.notEqual(nextSession.helper_epoch, firstSession.helper_epoch);
  record('Explicit start after stop creates a new native helper epoch on the same MCP connection');
  const rejected = await client.callTool({ name: 'computer_act', arguments: {
    observation_id: previousObservation.id, action_id: 'old-epoch-action',
    action: { kind: 'click', button: 'left', target: elementTarget(previousObservation, 'Fixture Increment') },
  } });
  assert.equal(rejected.isError, true);
  assert.equal(rejected.structuredContent.state, 'rejected');
  assert.equal(rejected.structuredContent.mechanism, 'none');
  assert.equal((await snapshot('target')).count, after.count);
  await observe(main);
  record('Old-epoch observations cannot dispatch input; the replacement session can observe fresh state');
  await call('computer_stop');
  await client.close(); client = undefined;
  record('Explicit stop terminates the helper presentation and input session');
} catch (error) {
  blocked = ['desktop_unavailable', 'unsupported_environment', 'permissions_pending'].includes(error.code);
  failed = error.stack ?? String(error);
  console.error(failed); process.exitCode = blocked ? 2 : 1;
} finally {
  if (client) await client.close().catch(error => { console.error(`MCP cleanup failed: ${error}`); process.exitCode = 1; });
  targetPid ??= (await snapshot('target'))?.pid;
  if (targetPid) {
    const check = spawnSync('ps', ['-p', String(targetPid), '-o', 'comm='], { encoding: 'utf8' });
    if (check.stdout.trim() === executable) {
      try { process.kill(targetPid, 'SIGTERM'); }
      catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
  }
  await writeFile(resolve(run, 'results.json'), JSON.stringify({ status: failed ? blocked ? 'blocked' : 'failed' : 'passed', scope: 'One self-created background application; real MCP, no configured PID, screen capture or permission requests; asynchronous cursor samples may include human movement', popupTested: process.argv.includes('--popup'), outcomes, isolationSamples, failure: failed }, null, 2));
  console.log(`Evidence: ${run}/results.json`);
}
