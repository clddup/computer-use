import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { resolve } from 'node:path';
const { spawn } = vi.hoisted(() => ({ spawn: vi.fn() }));
vi.mock('node:child_process', () => ({ spawn }));
import { HelperTransport, UnknownExecutionError } from '../packages/runtime/dist/transport.js';

let child, transport, requests;
beforeEach(() => {
  requests = [];
  spawn.mockImplementation(() => {
    const process = Object.assign(new EventEmitter(), { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: vi.fn() });
    process.stdin.on('data', chunk => requests.push(JSON.parse(chunk.toString())));
    process.stdin.on('finish', () => process.emit('close', 0));
    child = process;
    return process;
  });
  transport = new HelperTransport({ helperPath: resolve('test-helper'), timeoutMs: 500 });
});
afterEach(async () => { await transport.close(); vi.clearAllMocks(); });
const respond = (index, result, extras = {}) => child.stdout.write(JSON.stringify({ version: 1, id: requests[index].id, result, error: null, ...extras }) + '\n');
const act = () => transport.request('act', { session_id: 's', observation_id: 'o', action_id: 'a', action: { kind: 'type_text', text: 'fixture' } }, 20);
describe('Helper transport failure boundaries (isolated protocol tests, not GUI)', () => {
  it('spawns without a shell and sends the versioned nested command', async () => {
    const pending = transport.request('capabilities'); respond(0, { available: true });
    expect(await pending).toEqual({ available: true });
    expect(requests[0].command).toEqual({ method: 'capabilities', params: {} });
    expect(spawn.mock.calls[0][2].shell).toBeUndefined();
  });
  it('assembles a response split across chunks', async () => {
    const pending = transport.request('capabilities');
    const line = JSON.stringify({ version: 1, id: requests[0].id, result: {}, error: null }) + '\n';
    child.stdout.write(line.slice(0, 11)); child.stdout.write(line.slice(11));
    expect(await pending).toEqual({});
  });
  it('returns unknown on lost act response and never automatically replays', async () => {
    await expect(act()).rejects.toMatchObject({ code: 'unknown_execution', mayHaveExecuted: true, actionId: 'a' });
    expect(requests).toHaveLength(1); respond(0, { state: 'dispatched' });
    const pending = transport.request('capabilities'); respond(1, {}); expect(await pending).toEqual({});
    expect(requests.filter(r => r.command.method === 'act')).toHaveLength(1);
  });
  it('treats helper disconnect during input as unknown', async () => {
    const pending = act(); child.emit('close', 1);
    await expect(pending).rejects.toBeInstanceOf(UnknownExecutionError);
  });
  it('rejects malformed JSON, invalid envelopes and incompatible versions', async () => {
    for (const invalid of ['not-json\n', JSON.stringify({ version: 2, id: 'x', result: {}, error: null }) + '\n', JSON.stringify({ version: 1, id: 'x', result: {}, error: { code: 'x', message: 'x', may_have_executed: false } }) + '\n']) {
      const isolated = new HelperTransport({ helperPath: resolve('test-helper') });
      const pending = isolated.request('capabilities'); child.stdout.write(invalid);
      await expect(pending).rejects.toMatchObject({ code: 'protocol_error' }); await isolated.close();
    }
  });
  it('rejects oversized output and cancels via EOF', async () => {
    const pending = act(); child.stdout.write(Buffer.alloc(16 * 1024 * 1024 + 1, 32));
    await expect(pending).rejects.toBeInstanceOf(UnknownExecutionError); expect(child.stdin.writableEnded).toBe(true);
  });
  it('rejects unknown input fields before sending them to a helper', async () => {
    await expect(transport.request('start', { allowed_pids: [1] })).rejects.toThrow('Invalid Request'); expect(requests).toHaveLength(0);
  });
  it('bounds the request queue', async () => {
    const pending = Array.from({ length: 32 }, () => transport.request('capabilities').catch(e => e));
    await expect(transport.request('capabilities')).rejects.toMatchObject({ code: 'queue_limit' });
    child.emit('close', 1); await Promise.all(pending);
  });
});
