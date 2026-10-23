#!/usr/bin/env node
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { ComputerRuntime } from '@independent-cu/runtime';
import { parseArgs } from 'node:util';
import { createComputerServer } from './server.js';
import { McpSession } from './session.js';
import { resolveHelperPath } from './helper-path.js';
const { values } = parseArgs({ options: { helper: { type: 'string' }, policy: { type: 'string' } } });
const helperPath = resolveHelperPath(import.meta.url, values.helper ?? process.env.CU_HELPER);
const session = new McpSession(() => new ComputerRuntime({ helperPath, policyPath: values.policy, onStderr: message => process.stderr.write(message) }));
const server = createComputerServer(session);
let shuttingDown: Promise<void> | undefined;
function shutdown(): Promise<void> {
  return shuttingDown ??= (async () => { try { await session.shutdown(); } finally { await server.close(); } })();
}
process.once('SIGINT', () => { void shutdown().catch(error => { console.error(error.message); process.exitCode = 1; }); });
process.once('SIGTERM', () => { void shutdown().catch(error => { console.error(error.message); process.exitCode = 1; }); });
process.stdin.once('end', () => { void shutdown().catch(error => { console.error(error.message); process.exitCode = 1; }); });
await server.connect(new StdioServerTransport());
