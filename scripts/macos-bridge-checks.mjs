import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

if (process.platform !== 'darwin') throw new Error('Swift bridge checks require macOS 14+.');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
await mkdir(resolve(root, '.local'), { recursive: true });
const run = await mkdtemp(resolve(root, '.local/macos-bridge-'));
const executable = resolve(run, 'BridgeChecks');
const target = process.arch === 'arm64' ? 'arm64-apple-macosx14.0' : 'x86_64-apple-macosx14.0';
const built = spawnSync('xcrun', ['swiftc', '-parse-as-library', '-target', target,
  resolve(root, 'native/crates/platform-macos/bridge.swift'),
  resolve(root, 'native/crates/platform-macos/input.swift'),
  resolve(root, 'native/crates/platform-macos/presentation.swift'),
  resolve(root, 'native/crates/platform-macos/capture.swift'),
  resolve(root, 'tests/fixtures/macos/BridgeChecks.swift'), '-o', executable], { encoding: 'utf8' });
assert.equal(built.status, 0, built.stdout + built.stderr);
const tested = spawnSync(executable, [], { encoding: 'utf8' });
process.stdout.write(tested.stdout);
process.stderr.write(tested.stderr);
assert.equal(tested.status, 0, `Bridge checks failed: ${tested.signal ?? tested.status}`);
console.log('No screen capture, input posting, permission requests or personal windows were used.');
