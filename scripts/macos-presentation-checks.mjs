import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

if (process.platform !== 'darwin') throw new Error('Presentation checks require macOS 14.4+.');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
await mkdir(resolve(root, '.local'), { recursive: true });
const run = await mkdtemp(resolve(root, '.local/macos-presentation-'));
const executable = resolve(run, 'PresentationChecks');
const target = process.arch === 'arm64' ? 'arm64-apple-macosx14.0' : 'x86_64-apple-macosx14.0';
const sources = ['bridge.swift', 'input.swift', 'presentation.swift', 'capture.swift'].map(name => resolve(root, 'native/crates/platform-macos', name));
const built = spawnSync('xcrun', ['swiftc', '-parse-as-library', '-target', target, ...sources,
  resolve(root, 'tests/fixtures/macos/PresentationChecks.swift'), '-o', executable], { encoding: 'utf8' });
assert.equal(built.status, 0, built.stdout + built.stderr);
const tested = spawnSync(executable, [run], { encoding: 'utf8', timeout: 30_000 });
process.stdout.write(tested.stdout ?? '');
process.stderr.write(tested.stderr ?? '');
assert.equal(tested.status, 0, `Presentation checks failed: ${tested.error ?? tested.signal ?? tested.status}`);
assert.equal(JSON.parse(await readFile(resolve(run, 'results.json'), 'utf8')).passed, true);
console.log(`Synthetic self-window presentation evidence: ${run}`);
