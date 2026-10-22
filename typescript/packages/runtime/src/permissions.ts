import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { ComputerUseError, type HelperOptions } from './transport.js';

const execute = promisify(execFile);
interface PermissionStatus {
  accessibility: boolean;
  screen_recording: boolean;
  required: { accessibility: boolean; screen_recording: boolean };
}
async function probe(options: HelperOptions, request: boolean, signal: AbortSignal): Promise<PermissionStatus> {
  const args = [request ? '--request-permissions' : '--permissions', '--policy', options.policyPath!];
  let stdout: string;
  try {
    ({ stdout } = await execute(options.helperPath, args, { encoding: 'utf8', maxBuffer: 65_536, timeout: request ? 120_000 : 10_000, signal }));
  } catch {
    throw new ComputerUseError(signal.aborted ? 'stopped' : 'permissions_failed', 'macOS system permission helper failed; no desktop operation was dispatched');
  }
  try {
    const status = JSON.parse(stdout) as PermissionStatus;
    for (const key of ['accessibility', 'screen_recording'] as const) {
      if (typeof status[key] !== 'boolean' || typeof status.required?.[key] !== 'boolean') throw new Error('Invalid permissions');
    }
    return status;
  } catch { throw new ComputerUseError('protocol_error', 'Invalid macOS permission status; rebuild the helper'); }
}
function missing(status: PermissionStatus): string[] {
  return (['accessibility', 'screen_recording'] as const).filter(key => status.required[key] && !status[key]);
}
export async function ensureMacosPermissions(options: HelperOptions, signal: AbortSignal): Promise<void> {
  if (process.platform !== 'darwin' || !options.policyPath) return;
  if (!missing(await probe(options, false, signal)).length) return;
  await probe(options, true, signal);
  const remaining = missing(await probe(options, false, signal));
  if (remaining.length) throw new ComputerUseError('permissions_pending', `macOS permissions not granted: ${remaining.join(', ')}. Complete the official system prompt or enable the responsible host/helper in System Settings > Privacy & Security, then start again. No desktop operation was dispatched.`);
}
