import { beforeEach, describe, expect, it, vi } from 'vitest';
const { request, close, permissions } = vi.hoisted(() => ({ request: vi.fn(), close: vi.fn(), permissions: vi.fn() }));
vi.mock('../packages/runtime/dist/transport.js', async original => {
  const actual = await original();
  return { ...actual, HelperTransport: class { request = request; close = close; async negotiate() {} } };
});
vi.mock('../packages/runtime/dist/permissions.js', () => ({ ensureMacosPermissions: permissions }));
import { ComputerRuntime } from '../packages/runtime/dist/index.js';
beforeEach(() => {
  request.mockReset(); close.mockReset(); close.mockResolvedValue(undefined);
  permissions.mockReset(); permissions.mockResolvedValue(undefined);
  request.mockImplementation(async method => method === 'start' ? { session_id: 's', helper_epoch: 'e', expires_at_ms: Date.now() + 30000 } : {});
});
describe('Runtime state and conservative outcome validation', () => {
  it('does not start a native control session while system permission is missing', async () => {
    const runtime = new ComputerRuntime({ helperPath: '/fixture' });
    permissions.mockRejectedValueOnce(Object.assign(new Error('Screen Recording is missing'), { code: 'permissions_pending' }));
    await expect(runtime.start()).rejects.toMatchObject({ code: 'permissions_pending' });
    expect(request).not.toHaveBeenCalled(); await runtime.stop();
  });
  it('stop cancels system permission setup and late completion cannot start a session', async () => {
    let release;
    permissions.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    const runtime = new ComputerRuntime({ helperPath: '/fixture' });
    const start = runtime.start(); await runtime.stop();
    expect(permissions.mock.calls[0][1].aborted).toBe(true);
    release(); await expect(start).rejects.toMatchObject({ code: 'stopped' });
    expect(request).not.toHaveBeenCalled();
  });
  it('converts an invalid act result to unknown rather than a safe-to-retry validation error', async () => {
    const runtime = new ComputerRuntime({ helperPath: '/fixture' }); await runtime.start();
    await expect(runtime.act('o', { kind: 'key', keys: ['Tab'] }, 'a')).rejects.toMatchObject({ code: 'unknown_execution', mayHaveExecuted: true, actionId: 'a' });
    expect(request.mock.calls.filter(([method]) => method === 'act')).toHaveLength(1); await runtime.stop();
  });
  it('rejects concurrent work but allows stop to overtake it', async () => {
    const runtime = new ComputerRuntime({ helperPath: '/fixture' }); await runtime.start();
    let release;
    request.mockImplementation(method => method === 'list_windows' ? new Promise(resolve => { release = resolve; }) : Promise.resolve({ stopped: true }));
    const listing = runtime.listWindows();
    await expect(runtime.listWindows()).rejects.toMatchObject({ code: 'busy' });
    await runtime.stop(); expect(close).toHaveBeenCalledOnce(); release([]); await listing;
    await expect(runtime.listWindows()).rejects.toMatchObject({ code: 'stopped' });
  });
  it('exposes the host-approved application lifecycle without accepting arbitrary launch data', async () => {
    const runtime = new ComputerRuntime({ helperPath: '/fixture' });
    request.mockImplementation(async method => {
      if (method === 'start') return { session_id: 's', helper_epoch: 'e', expires_at_ms: Date.now() + 30000 };
      if (method === 'list_apps') return [{ id: 'fixture', display_name: 'Fixture', is_running: false, windows: [] }];
      if (method === 'launch_app') return { app_id: 'fixture', state: 'ready', windows: [], message: 'ready' };
      if (method === 'get_window') return { id: '1', pid: 1, process_identity: '1:1:1', title: 'Fixture', bounds: { x: 0, y: 0, width: 10, height: 10 } };
      if (method === 'activate_window') return { activated: true };
      return {};
    });
    await runtime.start();
    await expect(runtime.listApps()).resolves.toHaveLength(1);
    await expect(runtime.launchApp('fixture')).resolves.toMatchObject({ app_id: 'fixture', state: 'ready' });
    const window = { id: '1', pid: 1, process_identity: '1:1:1', title: 'Fixture', bounds: { x: 0, y: 0, width: 10, height: 10 } };
    await expect(runtime.getWindow(window)).resolves.toMatchObject({ id: '1' });
    await expect(runtime.activateWindow(window)).resolves.toEqual({ activated: true });
    expect(request.mock.calls.map(([method]) => method)).toEqual(['start', 'list_apps', 'launch_app', 'get_window', 'activate_window']);
    await runtime.stop();
  });
  it('is idempotent when stopping and requires a new runtime afterward', async () => {
    const runtime = new ComputerRuntime({ helperPath: '/fixture' }); await runtime.start();
    const first = runtime.stop(); expect(runtime.stop()).toBe(first); await first;
    expect(close).toHaveBeenCalledOnce(); await expect(runtime.start()).rejects.toMatchObject({ code: 'stopped' });
  });
});
