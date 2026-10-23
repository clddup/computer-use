import { type ComputerRuntime, ComputerUseError } from '@independent-cu/runtime';

const terminalCodes = new Set([
  'cancelled', 'session_expired', 'helper_closed', 'stopped', 'user_interruption',
  'capture_stream_stopped', 'capture_timeout', 'presentation_timeout', 'desktop_unavailable',
]);

export class McpSession {
  private runtime?: ComputerRuntime;
  private failure?: ComputerUseError;
  private closing?: Promise<void>;
  private starting = false;
  private generation = 0;
  private shutDown = false;

  constructor(private readonly createRuntime: () => ComputerRuntime) {}

  current(): ComputerRuntime {
    if (this.shutDown) throw new ComputerUseError('stopped', 'MCP server is shutting down');
    if (this.closing) throw new ComputerUseError('busy', 'Session cleanup is in progress');
    if (this.failure) throw this.failure;
    return this.runtime ??= this.createRuntime();
  }

  failed(error: unknown, source?: ComputerRuntime): void {
    if (source && source !== this.runtime) return;
    if (error instanceof ComputerUseError && terminalCodes.has(error.code)) {
      this.failure ??= new ComputerUseError(error.code, `${error.message}. Call computer_start explicitly to create a new session; re-list and re-observe. Never replay an old action.`);
    }
  }

  async start(): ReturnType<ComputerRuntime['start']> {
    if (this.starting || this.closing) throw new ComputerUseError('busy', 'Session transition is in progress');
    this.starting = true;
    const generation = this.generation;
    try {
      if (this.failure) {
        await this.dispose();
        this.failure = undefined;
      }
      if (generation !== this.generation || this.shutDown) throw new ComputerUseError('stopped', 'Start was cancelled by stop');
      const result = await this.current().start();
      if (generation !== this.generation || this.shutDown) throw new ComputerUseError('stopped', 'Start was cancelled by stop');
      return result;
    } finally { this.starting = false; }
  }

  stop(): Promise<void> {
    this.generation++;
    return this.dispose();
  }

  shutdown(): Promise<void> {
    this.shutDown = true;
    return this.stop();
  }

  private dispose(): Promise<void> {
    if (this.closing) return this.closing;
    const runtime = this.runtime;
    if (!runtime) return Promise.resolve();
    const closing = runtime.stop().then(() => {
      this.runtime = undefined;
      this.failure = undefined;
    });
    this.closing = closing.finally(() => { this.closing = undefined; });
    return this.closing;
  }
}
