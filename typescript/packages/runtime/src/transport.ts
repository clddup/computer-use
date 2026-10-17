import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { isAbsolute } from 'node:path';
import { VERSION, MAX_FRAME, assertWire, type Response } from '@independent-cu/protocol';

export class ComputerUseError extends Error {
  constructor(public readonly code: string, message: string, public readonly mayHaveExecuted = false) {
    super(message); this.name = 'ComputerUseError';
  }
}
export class UnknownExecutionError extends ComputerUseError {
  constructor(message: string, public readonly actionId?: string) {
    super('unknown_execution', message, true); this.name = 'UnknownExecutionError';
  }
}
export interface HelperOptions {
  helperPath: string;
  policyPath?: string;
  timeoutMs?: number;
  onStderr?: (message: string) => void;
  signal?: AbortSignal;
}
interface Pending {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
  actionId?: string;
  inputAction: boolean;
}
export class HelperTransport {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly pending = new Map<string, Pending>();
  private buffer: Buffer = Buffer.alloc(0);
  private ended = false;
  private closing = false;
  private exitPromise: Promise<void>;
  constructor(private readonly options: HelperOptions) {
    if (!isAbsolute(options.helperPath) || options.policyPath && !isAbsolute(options.policyPath)) {
      throw new ComputerUseError('invalid_config', 'Helper and policy paths must be absolute host configuration');
    }
    this.child = spawn(options.helperPath, options.policyPath ? ['--policy', options.policyPath] : [], {
      stdio: 'pipe', windowsHide: true,
    });
    this.exitPromise = new Promise(resolve => this.child.once('close', () => resolve()));
    this.child.stdout.on('data', (chunk: Buffer) => this.receive(chunk));
    this.child.stderr.on('data', (chunk: Buffer) => options.onStderr?.(chunk.toString('utf8')));
    this.child.on('error', error => this.fail(error));
    this.child.stdin.on('error', error => this.fail(error));
    this.child.once('close', () => this.fail(new ComputerUseError('helper_closed', 'Helper closed; unresolved input is unknown')));
  }
  request(method: string, params: unknown = {}, timeoutMs = this.options.timeoutMs ?? 45_000): Promise<unknown> {
    if (this.ended || this.closing) return Promise.reject(new ComputerUseError('helper_closed', 'Helper is not accepting requests'));
    if (this.pending.size >= 32) return Promise.reject(new ComputerUseError('queue_limit', 'Too many pending requests'));
    const id = randomUUID();
    const request = { version: VERSION, id, command: { method, params } };
    try { assertWire('Request', request); } catch (error) { return Promise.reject(error); }
    const bytes = Buffer.from(JSON.stringify(request) + '\n');
    if (bytes.length > MAX_FRAME) return Promise.reject(new ComputerUseError('frame_limit', 'Request exceeds frame limit'));
    const sideEffect = method === 'act' || method === 'launch_app' || method === 'activate_window';
    const actionId = method === 'act' && typeof params === 'object' && params !== null && 'action_id' in params
      ? String(params.action_id)
      : method === 'launch_app' && typeof params === 'object' && params !== null && 'app_id' in params
        ? String(params.app_id)
        : method === 'activate_window' && typeof params === 'object' && params !== null && 'window' in params
          ? String((params.window as { id?: unknown }).id ?? '')
          : undefined;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(sideEffect
          ? new UnknownExecutionError(`${method} response timed out. Do not replay; re-list and re-observe first.`, actionId)
          : new ComputerUseError('timeout', `Helper ${method} timed out`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer, actionId, inputAction: sideEffect });
      this.child.stdin.write(bytes, error => { if (error) this.fail(error); });
    });
  }
  private receive(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    let end: number;
    while ((end = this.buffer.indexOf(10)) !== -1) {
      if (end + 1 > MAX_FRAME) { this.protocolFailure('Oversized helper frame'); return; }
      const line = this.buffer.subarray(0, end); this.buffer = this.buffer.subarray(end + 1);
      try {
        const response = assertWire<Response>('Response', JSON.parse(line.toString('utf8')));
        if (response.version !== VERSION || (response.result === null) === (response.error === null)) throw new Error('Invalid response envelope');
        const pending = this.pending.get(response.id);
        if (!pending) continue; // Late responses never trigger a retry.
        clearTimeout(pending.timer); this.pending.delete(response.id);
        if (response.error) pending.reject(new ComputerUseError(response.error.code, response.error.message, response.error.may_have_executed));
        else pending.resolve(response.result);
      } catch { this.protocolFailure('Invalid helper protocol response'); return; }
    }
    if (this.buffer.length > MAX_FRAME) this.protocolFailure('Oversized partial helper frame');
  }
  private protocolFailure(message: string): void {
    this.fail(new ComputerUseError('protocol_error', message));
    this.child.stdin.end(); // EOF requests native cancellation and input cleanup.
  }
  private fail(error: Error): void {
    this.ended = true;
    for (const item of this.pending.values()) {
      clearTimeout(item.timer);
      item.reject(item.inputAction ? new UnknownExecutionError(`${error.message}; input may have been dispatched. Do not replay.`, item.actionId) : error);
    }
    this.pending.clear();
  }
  async close(): Promise<void> {
    if (this.closing) return this.exitPromise;
    this.closing = true; this.child.stdin.end();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const clean = await Promise.race([
      this.exitPromise.then(() => true),
      new Promise<false>(resolve => { timer = setTimeout(() => resolve(false), 6_000); }),
    ]);
    if (timer) clearTimeout(timer);
    if (!clean) {
      this.child.kill(); await this.exitPromise;
      throw new ComputerUseError('cleanup_unconfirmed', 'Helper did not stop gracefully; input cleanup could not be confirmed', true);
    }
  }
}
