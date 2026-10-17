#!/usr/bin/env node
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { ComputerRuntime } from '@independent-cu/runtime';
import { parseArgs } from 'node:util';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createComputerServer } from './server.js';
const { values } = parseArgs({ options: { helper: { type: 'string' }, policy: { type: 'string' } } });
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../');
const helperPath = values.helper ?? process.env.CU_HELPER ?? resolve(root, 'native/target/debug', process.platform === 'win32' ? 'cu-helper.exe' : 'cu-helper');
const runtime = new ComputerRuntime({ helperPath, policyPath: values.policy, onStderr: message => process.stderr.write(message) });
const server = createComputerServer(runtime);
let shuttingDown: Promise<void> | undefined;
function shutdown(): Promise<void> {
  return shuttingDown ??= (async () => { try { await runtime.stop(); } finally { await server.close(); } })();
}
process.once('SIGINT', () => { void shutdown().catch(error => { console.error(error.message); process.exitCode = 1; }); });
process.once('SIGTERM', () => { void shutdown().catch(error => { console.error(error.message); process.exitCode = 1; }); });
process.stdin.once('end', () => { void shutdown().catch(error => { console.error(error.message); process.exitCode = 1; }); });
await server.connect(new StdioServerTransport());
