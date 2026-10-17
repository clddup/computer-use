import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { Computer } from '../typescript/packages/sdk/dist/index.js';

if (process.platform !== 'win32') throw new Error('This fixture requires a Windows interactive desktop.');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
await mkdir(resolve(root, '.local'), { recursive: true });
const run = await mkdtemp(resolve(root, '.local/windows-cold-start-'));
const helperPath = resolve(process.env.CU_HELPER ?? resolve(root, 'native/target/debug/cu-helper.exe'));
const executable = resolve(run, 'Fixture.exe');
const compiler = resolve(process.env.WINDIR ?? 'C:/Windows', 'Microsoft.NET/Framework64/v4.0.30319/csc.exe');
const compile = spawnSync(compiler, ['/nologo', '/target:winexe', `/out:${executable}`, '/r:System.Windows.Forms.dll', '/r:System.Drawing.dll', '/r:System.Web.Extensions.dll', resolve(root, 'tests/fixtures/windows/Fixture.cs')], { encoding: 'utf8' });
if (compile.status !== 0) throw new Error(`Fixture compilation failed: ${compile.stdout}\n${compile.stderr}`);
const oracle = resolve(run, 'oracle.json');
const policyPath = resolve(run, 'policy.json');
await writeFile(policyPath, JSON.stringify({
  allowed_pids: [],
  allowed_apps: [{ id: 'fixture', display_name: 'Independent Computer-use Fixture', executable, args: [oracle, run.split(/[/\\]/).at(-1)] }],
  allow_input: true,
  allow_screenshot: true,
  allow_accessibility: true,
  session_seconds: 120,
}));
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function snapshot() { try { return JSON.parse(await readFile(oracle, 'utf8')); } catch { return undefined; } }
async function waitFor(test, label, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { const value = await snapshot(); if (value && test(value)) return value; await sleep(50); }
  throw new Error(`Timed out waiting for ${label}; oracle=${JSON.stringify(await snapshot())}`);
}
const runtime = new Computer({ helperPath, policyPath, onStderr: text => process.stderr.write(text) });
try {
  await runtime.start();
  const apps = await runtime.listApps();
  const app = apps.find(value => value.id === 'fixture');
  assert.ok(app, `Approved app missing: ${JSON.stringify(apps)}`);
  assert.equal(app.is_running, false);
  const launch = await runtime.launchApp('fixture');
  assert.equal(launch.app_id, 'fixture');
  assert.equal(launch.state, 'ready', launch.message);
  const started = await waitFor(value => Number.isInteger(value.pid), 'helper-launched fixture');
  assert.equal(started.pid > 0, true);
  const windows = await runtime.listWindows();
  assert.equal(windows.length, 1, `Expected one launched window: ${JSON.stringify(windows)}`);
  const window = await runtime.getWindow(windows[0]);
  await runtime.activateWindow(window);
  let observation = await runtime.observe(window, { screenshot: true, accessibility: true });
  const button = observation.elements.find(value => value.name === 'Fixture Increment');
  assert.ok(button, 'Fixture increment control was not observed');
  const outcome = await runtime.act(observation.id, { kind: 'click', button: 'left', target: { kind: 'element', element_id: button.id } }, 'cold-start-click');
  assert.equal(outcome.state, 'dispatched', outcome.message);
  await waitFor(value => value.count === 1, 'launched fixture click');
  observation = await runtime.observe(window, { screenshot: false, accessibility: true });
  assert.ok(observation.elements.length > 3);
  console.log(`Cold-start GUI evidence: ${run}`);
} finally {
  await writeFile(oracle + '.control', 'close').catch(() => {});
  await Promise.race([runtime.stop().catch(() => {}), sleep(5_000)]);
  await sleep(500);
}
