import { describe, it, expect } from 'vitest';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { validator } from '../packages/protocol/dist/index.js';

const helper = process.env.CU_HELPER ?? resolve('../native/target/debug', process.platform === 'win32' ? 'cu-helper.exe' : 'cu-helper');
const window = { id: '1', pid: 1, process_identity: 'process-start', title: 'Fixture', bounds: { x: -100, y: 20, width: 400, height: 200 } };
const request = (method, params = {}) => ({ version: 1, id: 'request', command: { method, params } });
const samples = [
  request('capabilities'), request('start'), request('list_windows', { session_id: 's' }),
  request('observe', { session_id: 's', window, screenshot: true, accessibility: false }),
  request('act', { session_id: 's', observation_id: 'o', action_id: 'a', action: { kind: 'type_text', text: '中文' } }),
  request('action_status', { session_id: 's', action_id: 'a' }), request('stop', { session_id: 's' }),
];
const invalid = [
  {}, { ...request('start'), extra: true }, request('start', { allowed_pids: [1] }),
  request('shell', { command: 'anything' }), request('list_windows'),
  request('observe', { session_id: 's', window: { ...window, pid: -1 }, screenshot: true, accessibility: true }),
  request('observe', { session_id: 's', window: { ...window, pid: 4294967296 }, screenshot: true, accessibility: true }),
  request('act', { session_id: 's', observation_id: 'o', action_id: 'a', action: { kind: 'key', keys: [1] } }),
  request('act', { session_id: 's', observation_id: 'o', action_id: 'a', action: { kind: 'scroll', target: { kind: 'image', image_id: 'i', x: 0, y: 0 }, dx: 2147483648, dy: 0 } }),
];
describe('Generated TypeScript/Rust wire compatibility', () => {
  it('emits the protocol schema asset alongside the built package', () => {
    expect(existsSync(new URL('../packages/protocol/dist/schema.json', import.meta.url))).toBe(true);
  });
  it('agrees on valid messages and rejects malformed/host-authorization parameters', () => {
    const values = [...samples, ...invalid];
    const native = spawnSync(helper, ['--validate-protocol'], { input: values.map(v => JSON.stringify(v)).join('\n') + '\n', encoding: 'utf8' });
    expect(native.status, native.stderr || native.error?.message).toBe(0);
    const actual = native.stdout.trim().split('\n').map(line => JSON.parse(line).valid);
    const expected = values.map((_, index) => index < samples.length);
    expect(actual).toEqual(expected);
    expect(values.map(v => Boolean(validator('Request')(v)))).toEqual(expected);
  });
  it('keeps request correlation IDs distinct from action deduplication IDs', () => {
    expect(validator('Request')(request('start'))).toBe(true);
    expect(validator('Request')({ version: 1, id: 'a', method: 'start', params: {} })).toBe(false);
  });
  it('rejects truncated and over-limit native NDJSON frames', () => {
    for (const input of [JSON.stringify(request('start')), ' '.repeat(16 * 1024 * 1024) + '\n']) {
      const result = spawnSync(helper, ['--validate-protocol'], { input, encoding: 'utf8', maxBuffer: 1024 * 1024 });
      expect(result.status).toBe(1); expect(result.stderr).toContain('invalid_frame');
    }
  });
  it('enforces safe integer timestamps and uint32/finite coordinate formats', () => {
    expect(validator('Window')({ ...window, pid: 1.5 })).toBe(false);
    expect(validator('Window')({ ...window, bounds: { ...window.bounds, x: Infinity } })).toBe(false);
    const observation = { id: 'o', helper_epoch: 'e', window, captured_at_ms: 1, expires_at_ms: Number.MAX_SAFE_INTEGER + 1, elements: [], images: [], warnings: [] };
    expect(validator('Observation')(observation)).toBe(false);
  });
});
