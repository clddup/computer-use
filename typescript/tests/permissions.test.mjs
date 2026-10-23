import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
const { execute } = vi.hoisted(() => ({ execute: vi.fn() }));
vi.mock('node:child_process', async () => {
  const { promisify } = await import('node:util');
  return { execFile: Object.assign(() => {}, { [promisify.custom]: execute }) };
});
import { ensureMacosPermissions } from '../packages/runtime/dist/permissions.js';
const options = { helperPath: '/fixture', policyPath: '/policy.json' };
const status = (accessibility, screen_recording, screenRequired = true) => ({ stdout: JSON.stringify({
  accessibility, screen_recording, required: { accessibility: true, screen_recording: screenRequired },
}) });
const signal = () => new AbortController().signal;
beforeEach(() => { execute.mockReset(); vi.stubGlobal('process', { ...process, platform: 'darwin' }); });
afterEach(() => vi.unstubAllGlobals());
describe('macOS official system-permission flow (mocked; no application approval UI)', () => {
  it('does not request permissions that are already granted', async () => {
    execute.mockResolvedValue(status(true, true)); await ensureMacosPermissions(options, signal());
    expect(execute).toHaveBeenCalledOnce(); expect(execute.mock.calls[0][1]).toEqual(['--permissions', '--policy', '/policy.json']);
  });
  it('requests official system permissions directly when missing, then independently rechecks', async () => {
    execute.mockResolvedValueOnce(status(true, false)).mockResolvedValueOnce(status(true, false)).mockResolvedValueOnce(status(true, true));
    await ensureMacosPermissions(options, signal());
    expect(execute.mock.calls.map(call => call[1][0])).toEqual(['--permissions', '--request-permissions', '--permissions']);
  });
  it('does not request screen recording for a policy that disables screenshots', async () => {
    execute.mockResolvedValue(status(true, false, false)); await ensureMacosPermissions(options, signal());
    expect(execute).toHaveBeenCalledOnce();
  });
  it('reports missing access instead of treating a permission request as a grant', async () => {
    execute.mockResolvedValue(status(true, false));
    await expect(ensureMacosPermissions(options, signal())).rejects.toMatchObject({ code: 'permissions_pending', mayHaveExecuted: false });
    expect(execute).toHaveBeenCalledTimes(3);
  });
  it('fails explicitly on malformed status or a failed subprocess', async () => {
    execute.mockResolvedValueOnce({ stdout: '{"accessibility":true}' });
    await expect(ensureMacosPermissions(options, signal())).rejects.toMatchObject({ code: 'protocol_error' });
    execute.mockRejectedValueOnce(new Error('Child exited'));
    await expect(ensureMacosPermissions(options, signal())).rejects.toMatchObject({ code: 'permissions_failed' });
  });
  it('passes the cancellation signal to the official permission request subprocess', async () => {
    const controller = new AbortController(); execute.mockImplementation(async (_path, _args, options) => {
      expect(options.signal).toBe(controller.signal); controller.abort(); throw new Error('Cancelled');
    });
    await expect(ensureMacosPermissions(options, controller.signal)).rejects.toMatchObject({ code: 'stopped' });
  });
  it('checks system permissions without requiring a host policy file', async () => {
    execute.mockResolvedValue(status(true, true));
    await ensureMacosPermissions({ helperPath: '/fixture' }, signal());
    expect(execute.mock.calls[0][1]).toEqual(['--permissions']);
  });
  it('does not request macOS permissions on another platform', async () => {
    vi.stubGlobal('process', { ...process, platform: 'linux' }); await ensureMacosPermissions(options, signal());
    expect(execute).not.toHaveBeenCalled();
  });
});
