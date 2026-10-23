import { afterEach, describe, expect, it, vi } from 'vitest';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { ComputerUseError } from '@independent-cu/runtime';
import { McpSession } from '../packages/mcp/dist/session.js';
import { createComputerServer } from '../packages/mcp/dist/server.js';

function fixture() {
  const runtimes = [];
  const create = vi.fn(() => {
    const index = runtimes.length;
    const runtime = {
      start: vi.fn(async () => ({ session_id: `s${index}`, helper_epoch: `e${index}`, expires_at_ms: Date.now() + 30000 })),
      stop: vi.fn(async () => {}),
      listApps: vi.fn(async () => []),
      act: vi.fn(async () => { throw new ComputerUseError('unknown_execution', 'Do not replay', true); }),
    };
    runtimes.push(runtime);
    return runtime;
  });
  return { session: new McpSession(create), create, runtimes };
}

describe('MCP session recovery without input replay', () => {
  it('creates a new runtime only after an explicit start following stop', async () => {
    const { session, create, runtimes } = fixture();
    const first = await session.start();
    await session.stop();
    expect(create).toHaveBeenCalledTimes(1);
    const second = await session.start();
    expect(second.helper_epoch).not.toBe(first.helper_epoch);
    expect(runtimes[0].stop).toHaveBeenCalledOnce();
    expect(create).toHaveBeenCalledTimes(2);
    await session.shutdown();
  });

  it.each(['cancelled', 'capture_stream_stopped', 'capture_timeout', 'presentation_timeout', 'user_interruption', 'helper_closed', 'session_expired', 'desktop_unavailable'])(
    'keeps %s visible until explicit start and cleans the old helper first', async code => {
      const { session, runtimes, create } = fixture();
      await session.start();
      session.failed(new ComputerUseError(code, 'original cause -3805'));
      expect(() => session.current()).toThrow('original cause -3805');
      expect(create).toHaveBeenCalledTimes(1);
      await session.start();
      expect(runtimes[0].stop).toHaveBeenCalledOnce();
      expect(create).toHaveBeenCalledTimes(2);
      expect(runtimes.every(runtime => runtime.act.mock.calls.length === 0)).toBe(true);
      await session.shutdown();
    },
  );

  it('does not replace a running session for stale observations or uncertain actions', async () => {
    const { session, create } = fixture();
    await session.start();
    const original = session.current();
    session.failed(new ComputerUseError('stale_observation', 'Observe again'));
    session.failed(new ComputerUseError('unknown_execution', 'Do not replay', true));
    expect(session.current()).toBe(original);
    expect(create).toHaveBeenCalledOnce();
    await session.shutdown();
  });

  it('does not let a late error from an old helper poison the replacement session', async () => {
    const { session } = fixture();
    await session.start(); const old = session.current();
    await session.stop(); await session.start();
    const replacement = session.current();
    session.failed(new ComputerUseError('cancelled', 'Old request completed late'), old);
    expect(session.current()).toBe(replacement);
    await session.shutdown();
  });

  it('does not replace a helper when graceful cleanup failed', async () => {
    const { session, create, runtimes } = fixture();
    await session.start();
    runtimes[0].stop.mockRejectedValue(new ComputerUseError('cleanup_unconfirmed', 'Input cleanup was not confirmed', true));
    session.failed(new ComputerUseError('cancelled', 'Stopped'));
    await expect(session.start()).rejects.toMatchObject({ code: 'cleanup_unconfirmed' });
    expect(create).toHaveBeenCalledOnce();
    runtimes[0].stop.mockResolvedValue(undefined);
    await session.shutdown();
  });

  it('stop overtakes an in-progress recovery without starting a replacement', async () => {
    const { session, create, runtimes } = fixture();
    await session.start();
    let release;
    runtimes[0].stop.mockImplementation(() => new Promise(resolve => { release = resolve; }));
    session.failed(new ComputerUseError('cancelled', 'Stopped'));
    const restarting = session.start();
    const stopped = session.stop();
    release();
    await expect(restarting).rejects.toMatchObject({ code: 'stopped' });
    await stopped;
    expect(create).toHaveBeenCalledOnce();
    await session.shutdown();
  });

  it('rejects concurrent starts and prevents reopening during server shutdown', async () => {
    const { session, runtimes, create } = fixture();
    await session.start();
    let release;
    runtimes[0].start.mockImplementation(() => new Promise(resolve => { release = resolve; }));
    const starting = session.start();
    await expect(session.start()).rejects.toMatchObject({ code: 'busy' });
    release({ session_id: 's0', helper_epoch: 'e0', expires_at_ms: 1 });
    await starting;
    await session.shutdown();
    await expect(session.start()).rejects.toMatchObject({ code: 'stopped' });
    expect(create).toHaveBeenCalledOnce();
  });
});

describe('Public MCP lifecycle', () => {
  let client, server, session;
  afterEach(async () => { await session?.shutdown(); await client?.close(); await server?.close(); });

  it('returns the interruption cause then accepts start again on the same MCP connection', async () => {
    const state = fixture(); session = state.session;
    server = createComputerServer(session);
    client = new Client({ name: 'session-regression', version: '1.0.0' });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    await server.connect(serverTransport); await client.connect(clientTransport);
    const call = (name, args = {}) => client.callTool({ name, arguments: args });
    expect((await client.listTools()).tools).toHaveLength(11);
    const first = await call('computer_start');
    state.runtimes[0].listApps.mockRejectedValueOnce(new ComputerUseError('capture_stream_stopped', 'SCStreamErrorDomain code -3805'));
    const failure = await call('computer_list_apps');
    expect(failure.isError).toBe(true);
    expect(failure.structuredContent).toMatchObject({ code: 'capture_stream_stopped', may_have_executed: false });
    const second = await call('computer_start');
    expect(second.isError).toBe(false);
    expect(second.structuredContent.helper_epoch).not.toBe(first.structuredContent.helper_epoch);
    expect((await call('computer_stop')).isError).toBe(false);
    expect((await call('computer_start')).isError).toBe(false);
    expect(state.create).toHaveBeenCalledTimes(3);
    expect(state.runtimes.every(runtime => runtime.act.mock.calls.length === 0)).toBe(true);
  });
});
