import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile, rename, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { inflateSync } from 'node:zlib';
import { Computer, ComputerUseError } from '../typescript/packages/sdk/dist/index.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outcomes = [];
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const pass = (name, details = {}) => { outcomes.push({ name, status: 'passed', ...details }); console.log(`PASS ${name}`); };
const blocked = (name, reason) => { outcomes.push({ name, status: 'blocked', reason }); console.log(`BLOCKED ${name}: ${reason}`); };
const fail = (name, error) => { outcomes.push({ name, status: 'failed', error: String(error?.stack || error) }); console.error(`FAIL ${name}: ${error?.stack || error}`); };

function pngPixels(encoded) {
  const png = Buffer.from(encoded, 'base64');
  if (png.subarray(0, 8).toString('hex') !== '89504e470d0a1a0a') throw new Error('not a PNG');
  let offset = 8; let width; let height; let depth; let type; const idat = [];
  while (offset < png.length) {
    const length = png.readUInt32BE(offset); const kind = png.toString('ascii', offset + 4, offset + 8);
    const body = png.subarray(offset + 8, offset + 8 + length); offset += length + 12;
    if (kind === 'IHDR') { width = body.readUInt32BE(0); height = body.readUInt32BE(4); depth = body[8]; type = body[9]; }
    if (kind === 'IDAT') idat.push(body);
    if (kind === 'IEND') break;
  }
  if (depth !== 8 || type !== 6) throw new Error(`unsupported PNG format depth=${depth} type=${type}`);
  const raw = inflateSync(Buffer.concat(idat)); const stride = width * 4; const pixels = Buffer.alloc(height * stride);
  let source = 0;
  for (let y = 0; y < height; y++) {
    const filter = raw[source++]; const row = raw.subarray(source, source + stride); source += stride;
    const out = pixels.subarray(y * stride, (y + 1) * stride); const prior = y ? pixels.subarray((y - 1) * stride, y * stride) : null;
    for (let x = 0; x < stride; x++) {
      const left = x >= 4 ? out[x - 4] : 0; const up = prior ? prior[x] : 0; const upperLeft = prior && x >= 4 ? prior[x - 4] : 0;
      if (filter === 0) out[x] = row[x];
      else if (filter === 1) out[x] = (row[x] + left) & 255;
      else if (filter === 2) out[x] = (row[x] + up) & 255;
      else if (filter === 3) out[x] = (row[x] + Math.floor((left + up) / 2)) & 255;
      else if (filter === 4) { const p = left + up - upperLeft; const pa = Math.abs(p - left); const pb = Math.abs(p - up); const pc = Math.abs(p - upperLeft); out[x] = (row[x] + (pa <= pb && pa <= pc ? left : pb <= pc ? up : upperLeft)) & 255; }
      else throw new Error(`unsupported PNG filter ${filter}`);
    }
  }
  return { width, height, pixels };
}
function centerColor(image) {
  const decoded = pngPixels(image.data); const x = Math.floor(decoded.width / 2); const y = Math.floor(decoded.height / 2); const i = (y * decoded.width + x) * 4;
  return [decoded.pixels[i], decoded.pixels[i + 1], decoded.pixels[i + 2]];
}
function imageDigest(image) { return createHash('sha256').update(Buffer.from(image.data, 'base64')).digest('hex'); }
function colorNear(actual, expected) { return actual.every((value, i) => Math.abs(value - expected[i]) <= 35); }

async function waitFor(read, predicate, label, timeout = 10_000) {
  const deadline = Date.now() + timeout; let last;
  while (Date.now() < deadline) { last = await read(); if (predicate(last)) return last; await sleep(80); }
  throw new Error(`Timed out waiting for ${label}: ${JSON.stringify(last)}`);
}
async function command(path, action, target, extra = {}) {
  const sequence = Date.now() * 1000 + Math.floor(Math.random() * 1000);
  const payload = JSON.stringify({ action, target, sequence, ...extra });
  const temporary = `${path}.control.tmp`;
  const destination = `${path}.control`;
  await writeFile(temporary, payload, 'utf8');
  // The fixture retries malformed files, but publish atomically so it never
  // observes a partially written command. Windows rename cannot replace an
  // existing destination, so remove only the previous private command first.
  await rm(destination, { force: true });
  await rename(temporary, destination);
}

let run; let fixture; const runtimes = []; let reportPath;
try {
  if (process.platform !== 'win32') { blocked('all Windows shared-capture checks', 'interactive Windows desktop required'); process.exitCode = 0; }
  else {
    const helperPath = resolve(process.env.CU_HELPER ?? resolve(root, 'native/target/debug/cu-helper.exe'));
    const compiler = resolve(process.env.WINDIR ?? 'C:/Windows', 'Microsoft.NET/Framework64/v4.0.30319/csc.exe');
    if (!existsSync(helperPath)) blocked('all Windows shared-capture checks', `helper missing: ${helperPath}`);
    else if (!existsSync(compiler)) blocked('all Windows shared-capture checks', `C# compiler missing: ${compiler}`);
    else {
      run = await mkdtemp(resolve(root, '.local/windows-shared-capture-'));
      reportPath = resolve(run, 'report.json'); const oracle = resolve(run, 'oracle.json');
      const executable = resolve(run, 'SharedCaptureFixture.exe');
      const compile = spawnSync(compiler, ['/nologo', '/target:winexe', `/out:${executable}`, '/r:System.Windows.Forms.dll', '/r:System.Drawing.dll', '/r:System.Web.Extensions.dll', resolve(root, 'tests/fixtures/windows/SharedCaptureFixture.cs')], { encoding: 'utf8' });
      if (compile.status !== 0) throw new Error(`fixture compilation failed: ${compile.stdout}\n${compile.stderr}`);
      fixture = spawn(executable, [oracle, run.split(/[/\\]/).at(-1)], { stdio: 'ignore' });
      const snapshot = async () => { try { return JSON.parse(await readFile(oracle, 'utf8')); } catch { return undefined; } };
      await waitFor(snapshot, value => value?.pid === fixture.pid && value?.a_hwnd, 'fixture startup');
      const policyPath = resolve(run, 'policy.json');
      await writeFile(policyPath, JSON.stringify({ allowed_pids: [fixture.pid], allow_input: false, allow_screenshot: true, allow_accessibility: false, session_seconds: 120 }));
      const runtime = new Computer({ helperPath, policyPath, timeoutMs: 8_000 }); runtimes.push(runtime); await runtime.start();
      const windows = await runtime.listWindows();
      const a = windows.find(window => window.title.startsWith('Shared Capture A ')); const b = windows.find(window => window.title.startsWith('Shared Capture B '));
      if (!a || !b) throw new Error(`fixture windows not found: ${windows.map(window => window.title).join(', ')}`);
      const observe = window => runtime.observe(window, { screenshot: true, accessibility: false });

      const first = await observe(a); const firstImage = first.images[0]; const firstColor = centerColor(firstImage); const firstDigest = imageDigest(firstImage);
      if (!colorNear(firstColor, [220, 30, 30])) throw new Error(`initial red pixel mismatch: ${firstColor}`);
      pass('same-window initial real PNG color', { png: true, color: firstColor });
      const repeated = await observe(a); pass('same-window repeated capture', { first_digest: firstDigest, second_digest: imageDigest(repeated.images[0]), stream_reuse_proven: false });

      await command(oracle, 'color', 'a', { color: 'green' });
      const changed = await waitFor(async () => observe(a), image => colorNear(centerColor(image.images[0]), [30, 210, 40]), 'green pixels in a subsequent PNG');
      pass('same-window color change reaches later PNG', { color: centerColor(changed.images[0]), stream_reuse_proven: false });

      const switched = await observe(b); if (!colorNear(centerColor(switched.images[0]), [25, 55, 220])) throw new Error('target B did not produce blue PNG');
      pass('target switch produces B PNG without old-color assertion', { color: centerColor(switched.images[0]), stream_reuse_proven: false });

      const cancel = new AbortController(); const cancelled = new Computer({ helperPath, policyPath, timeoutMs: 8_000, signal: cancel.signal }); runtimes.push(cancelled); await cancelled.start();
      const cancelWindows = await cancelled.listWindows(); const pending = cancelled.observe(cancelWindows.find(window => window.title.startsWith('Shared Capture B ')), { screenshot: true }); cancel.abort();
      try { await pending; blocked('cancellation during capture', 'capture completed before abort; no in-flight cancellation was observable'); }
      catch (error) { pass('cancellation during capture', { code: error.code ?? error.name }); }
      await cancelled.stop();

      await command(oracle, 'close', 'a'); await waitFor(snapshot, value => value.a_closed, 'fixture A close');
      try { await observe(a); fail('closed target cleanup', new Error('observe unexpectedly succeeded after target close')); }
      catch (error) { pass('closed target cleanup', { code: error.code ?? error.name }); }
      await runtime.stop();
    }
  }
} catch (error) { fail('shared capture checks', error); }
finally {
  for (const runtime of runtimes.reverse()) { try { await runtime.stop(); } catch (error) { console.error(`cleanup runtime: ${error}`); } }
  if (fixture && !fixture.killed) fixture.kill();
  if (run) {
    try { await writeFile(reportPath, JSON.stringify({ generated_at: new Date().toISOString(), outcomes }, null, 2)); } catch (error) { console.error(`cleanup report: ${error}`); }
    try { await rm(run, { recursive: true, force: true }); } catch (error) { console.error(`cleanup directory: ${error}`); }
  }
}
if (outcomes.some(outcome => outcome.status === 'failed')) process.exitCode = 1;
console.log(JSON.stringify({ outcomes }, null, 2));
