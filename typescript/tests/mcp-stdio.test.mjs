import { expect, it } from 'vitest';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

// Protocol fixture only: no native APIs, desktop access, permission requests or actual input.
const helperSource = `#!${process.execPath}
import { createInterface } from 'node:readline';
import { appendFileSync } from 'node:fs';
const epoch = 'fixture-' + process.pid;
if (process.argv.includes('--permissions') || process.argv.includes('--request-permissions')) {
  console.log(JSON.stringify({ accessibility:true, screen_recording:true, required:{accessibility:true,screen_recording:false} }));
} else {
  for await (const line of createInterface({ input:process.stdin })) {
    const request = JSON.parse(line), method = request.command.method;
    appendFileSync(process.env.CU_TEST_COMMAND_LOG, JSON.stringify({ pid:process.pid, method }) + '\\n');
    let result;
    if (method === 'capabilities') result = { protocol_version:2, platform:'fixture', backend:'protocol-only', available:true, reason:null, actions:[], screenshot_methods:[], limitations:[] };
    else if (method === 'start') result = { session_id:epoch, helper_epoch:epoch, expires_at_ms:Date.now()+60000 };
    else if (method === 'list_apps') result = [];
    else if (method === 'stop') result = { stopped:true };
    else if (method === 'act') result = { action_id:request.command.params.action_id, helper_epoch:epoch, state:'rejected', mechanism:'none', message:'stale_observation: old epoch' };
    else throw new Error('Unexpected fixture request: ' + method);
    console.log(JSON.stringify({ version:request.version, id:request.id, result, error:null }));
  }
}
`;

it.skipIf(process.platform === 'win32')('restarts the helper on explicit start through real MCP stdio without replaying input (protocol fixture, not GUI)', async () => {
  const root = resolve(import.meta.dirname, '../..');
  await mkdir(resolve(root, '.local'), { recursive: true });
  const run = await mkdtemp(resolve(root, '.local/mcp-session-stdio-'));
  const helper = resolve(run, 'fixture-helper.mjs'), trace = resolve(run, 'commands.jsonl');
  await writeFile(helper, helperSource, { mode: 0o700 });
  const client = new Client({ name: 'stdio-session-regression', version: '1.0.0' });
  const transport = new StdioClientTransport({ command:process.execPath,
    args:[resolve(root, 'typescript/packages/mcp/dist/main.js'), '--helper', helper],
    env:{ ...process.env, CU_TEST_COMMAND_LOG:trace }, stderr:'pipe' });
  try {
    await client.connect(transport);
    const call = (name, args = {}) => client.callTool({ name, arguments:args });
    expect((await client.listTools()).tools).toHaveLength(11);
    const first = await call('computer_start'); expect(first.isError).toBe(false);
    expect((await call('computer_stop')).isError).toBe(false);
    const second = await call('computer_start'); expect(second.isError).toBe(false);
    expect(second.structuredContent.helper_epoch).not.toBe(first.structuredContent.helper_epoch);
    const rejected = await call('computer_act', { observation_id:'old-observation', action_id:'old-action', action:{kind:'key',keys:['Tab']} });
    expect(rejected.structuredContent).toMatchObject({ state:'rejected', mechanism:'none' });
    expect((await call('computer_stop')).isError).toBe(false);
    const commands = (await readFile(trace, 'utf8')).trim().split('\n').map(line => JSON.parse(line));
    expect(commands.filter(command => command.method === 'start')).toHaveLength(2);
    expect(commands.filter(command => command.method === 'stop')).toHaveLength(2);
    expect(commands.filter(command => command.method === 'act')).toHaveLength(1);
    expect(new Set(commands.map(command => command.pid)).size).toBe(2);
  } finally { await client.close(); }
}, 15000);
