import { randomUUID } from 'node:crypto';
import { assertWire, type Action, type ActionOutcome, type Application, type Capabilities, type LaunchResult, type Observation, type Window } from '@independent-cu/protocol';
import { ComputerUseError, UnknownExecutionError, HelperTransport, type HelperOptions } from './transport.js';
export { ComputerUseError, UnknownExecutionError, HelperTransport } from './transport.js';
export type { HelperOptions } from './transport.js';

export class ComputerRuntime {
  private readonly transport: HelperTransport;
  private sessionId?: string;
  private busy = false;
  private stopped = false;
  private stopping?: Promise<void>;
  constructor(options: HelperOptions) {
    this.transport = new HelperTransport(options);
    if (options.signal) {
      if (options.signal.aborted) void this.stop();
      else options.signal.addEventListener('abort', () => void this.stop(), { once: true });
    }
  }
  async capabilities(): Promise<Capabilities> {
    return assertWire('Capabilities', await this.transport.request('capabilities'));
  }
  private async exclusive<T>(operation: () => Promise<T>): Promise<T> {
    if (this.stopped) throw new ComputerUseError('stopped', 'Create a new runtime after stopping');
    if (this.busy) throw new ComputerUseError('busy', 'Only one observe/action operation may run at a time; stop is always available');
    this.busy = true;
    try { return await operation(); } finally { this.busy = false; }
  }
  private session(): string {
    if (!this.sessionId) throw new ComputerUseError('invalid_session', 'Start a host-authorized session first');
    return this.sessionId;
  }
  async start(): Promise<{ session_id: string; helper_epoch: string; expires_at_ms: number }> {
    return this.exclusive(async () => {
      if (this.sessionId) throw new ComputerUseError('session_exists', 'Session already started');
      const value = await this.transport.request('start') as { session_id: string; helper_epoch: string; expires_at_ms: number };
      if (!value || typeof value.session_id !== 'string' || typeof value.helper_epoch !== 'string' || typeof value.expires_at_ms !== 'number') throw new ComputerUseError('protocol_error', 'Invalid start result');
      if (this.stopped) throw new ComputerUseError('stopped', 'Start was cancelled by stop');
      this.sessionId = value.session_id; return value;
    });
  }
  async listApps(): Promise<Application[]> {
    return this.exclusive(async () => {
      const result = await this.transport.request('list_apps', { session_id: this.session() });
      if (!Array.isArray(result)) throw new ComputerUseError('protocol_error', 'Expected application array');
      return result.map(value => assertWire<Application>('Application', value));
    });
  }
  async launchApp(appId: string): Promise<LaunchResult> {
    return this.exclusive(async () => {
      const result = await this.transport.request('launch_app', { session_id: this.session(), app_id: appId });
      try { return assertWire<LaunchResult>('LaunchResult', result); }
      catch { throw new UnknownExecutionError('Invalid launch result; application may have started. Re-list applications before retrying.', appId); }
    });
  }
  async getWindow(window: Window): Promise<Window> {
    return this.exclusive(async () => assertWire('Window', await this.transport.request('get_window', { session_id: this.session(), window })));
  }
  async activateWindow(window: Window): Promise<{ activated: boolean }> {
    return this.exclusive(async () => {
      const result = await this.transport.request('activate_window', { session_id: this.session(), window });
      if (!result || typeof result !== 'object' || (result as { activated?: unknown }).activated !== true) {
        throw new UnknownExecutionError('Invalid activation result; window activation may have occurred.', window.id);
      }
      return result as { activated: true };
    });
  }
  async listWindows(): Promise<Window[]> {
    return this.exclusive(async () => {
      const result = await this.transport.request('list_windows', { session_id: this.session() });
      if (!Array.isArray(result)) throw new ComputerUseError('protocol_error', 'Expected window array');
      return result.map(value => assertWire<Window>('Window', value));
    });
  }
  async observe(window: Window, options: { screenshot?: boolean; accessibility?: boolean } = {}): Promise<Observation> {
    return this.exclusive(async () => assertWire('Observation', await this.transport.request('observe', {
      session_id: this.session(), window, screenshot: options.screenshot ?? true, accessibility: options.accessibility ?? false,
    })));
  }
  async act(observationId: string, action: Action, actionId: string = randomUUID()): Promise<ActionOutcome> {
    return this.exclusive(async () => {
      const outcome = await this.transport.request('act', {
        session_id: this.session(), action_id: actionId, observation_id: observationId, action,
      });
      try { return assertWire<ActionOutcome>('ActionOutcome', outcome); }
      catch { throw new UnknownExecutionError('Invalid action outcome; input may have executed. Do not replay.', actionId); }
    });
  }
  async actionStatus(actionId: string): Promise<ActionOutcome> {
    return assertWire('ActionOutcome', await this.transport.request('action_status', { session_id: this.session(), action_id: actionId }));
  }
  stop(): Promise<void> {
    if (this.stopping) return this.stopping;
    this.stopped = true;
    this.stopping = (async () => {
      try {
        if (this.sessionId) await this.transport.request('stop', { session_id: this.sessionId }, 5_000);
      } finally { this.sessionId = undefined; await this.transport.close(); }
    })();
    return this.stopping;
  }
  async [Symbol.asyncDispose](): Promise<void> { await this.stop(); }
}
