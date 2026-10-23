import { spawnSync } from 'node:child_process';
import { cp, lstat, mkdir, mkdtemp, readFile, readdir, readlink, realpath, rename, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import assert from 'node:assert/strict';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const artifact = 'app-use-macos';
const marker = 'manifest.json';

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, ...options });
  if (result.stderr) process.stderr.write(result.stderr);
  if (result.error || result.status !== 0) throw new Error(`${command} failed: ${result.error?.message ?? result.signal ?? result.status}\n${result.stdout ?? ''}`);
  return result.stdout;
}

export function releaseArtifacts(output) {
  const messages = output.trim().split('\n').map(line => JSON.parse(line));
  const helper = messages.find(message => message.reason === 'compiler-artifact' && message.target?.name === 'cu-helper' && message.executable);
  const bridge = messages.find(message => message.reason === 'build-script-executed' && message.linked_libs?.includes('dylib=cu_macos_bridge'));
  assert.ok(helper && helper.profile.opt_level !== '0' && !helper.profile.debug_assertions, 'Expected a release helper artifact');
  assert.ok(bridge?.out_dir, 'Cargo did not report the Swift bridge output directory');
  return { helper: helper.executable, bridge: join(bridge.out_dir, 'libcu_macos_bridge.dylib') };
}

export function rpaths(output) {
  return [...output.matchAll(/\bcmd LC_RPATH\s*\n\s*cmdsize \d+\s*\n\s*path (.+) \(offset \d+\)/g)].map(match => match[1]);
}

export function validateDestination(output) {
  const destination = resolve(output);
  assert.ok(destination !== root && !root.startsWith(destination + sep) && destination !== sep, 'Output must not replace the repository or its ancestors');
  return destination;
}

async function exists(path) {
  try { await lstat(path); return true; }
  catch (error) { if (error.code === 'ENOENT') return false; throw error; }
}

async function requirePackage(directory) {
  assert.ok((await lstat(directory)).isDirectory(), 'Package directory must not be a symlink');
  const manifest = JSON.parse(await readFile(join(directory, marker), 'utf8'));
  assert.equal(manifest.artifact, artifact, `Refusing to replace an unmanaged directory: ${directory}`);
}

export async function publishPackage(stage, output) {
  await requirePackage(stage);
  const previous = `${stage}.previous`;
  const replacing = await exists(output);
  if (replacing) { await requirePackage(output); await rename(output, previous); }
  try { await rename(stage, output); }
  catch (error) {
    if (replacing) await rename(previous, output);
    throw error;
  }
  if (replacing) await rm(previous, { recursive: true });
}

export async function verifyLinks(directory, packageRoot = directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isSymbolicLink()) {
      assert.ok(!isAbsolute(await readlink(path)), `Absolute package symlink: ${path}`);
      const target = relative(await realpath(packageRoot), await realpath(path));
      assert.ok(target !== '..' && !target.startsWith('..' + sep) && !isAbsolute(target), `Package symlink escapes its directory: ${path}`);
    } else if (entry.isDirectory()) await verifyLinks(path, packageRoot);
  }
}

function prepareNative(directory) {
  const helper = join(directory, 'cu-helper'), bridge = join(directory, 'libcu_macos_bridge.dylib');
  for (const path of [helper, bridge]) {
    for (const entry of rpaths(run('otool', ['-l', path]))) {
      if (!['@executable_path', '@loader_path', '/usr/lib/swift'].includes(entry)) run('install_name_tool', ['-delete_rpath', entry, path]);
    }
  }
  assert.ok(rpaths(run('otool', ['-l', helper])).includes('@executable_path'), 'Helper must load its sibling Swift bridge');
  run('codesign', ['--force', '--sign', '-', '--identifier', 'app-use.bridge', bridge]);
  run('codesign', ['--force', '--sign', '-', '--identifier', 'app-use.helper', helper]);
  for (const path of [bridge, helper]) run('codesign', ['--verify', '--strict', path]);
}

export async function verifyPackage(directory) {
  await verifyLinks(directory);
  const helper = join(directory, 'native/cu-helper');
  const schema = JSON.parse(run(helper, ['--schema'], { cwd: directory }));
  assert.ok(schema.definitions, 'Packaged native helper did not emit its protocol schema');
  const require = createRequire(join(directory, 'mcp/package.json'));
  const { Client } = await import(pathToFileURL(require.resolve('@modelcontextprotocol/sdk/client/index.js')));
  const { StdioClientTransport } = await import(pathToFileURL(require.resolve('@modelcontextprotocol/sdk/client/stdio.js')));
  const client = new Client({ name: 'package-verification', version: '1.0.0' });
  const transport = new StdioClientTransport({ command: process.execPath, cwd: directory,
    args: [join(directory, 'mcp/dist/main.js')], env: Object.fromEntries(Object.entries(process.env).filter(([key]) => key !== 'CU_HELPER')), stderr: 'pipe' });
  transport.stderr?.on('data', data => process.stderr.write(data));
  try {
    await client.connect(transport);
    assert.equal((await client.listTools()).tools.length, 11);
    const capabilities = await client.callTool({ name: 'computer_capabilities', arguments: {} });
    assert.ok(!capabilities.isError, JSON.stringify(capabilities.structuredContent));
    assert.equal(capabilities.structuredContent.platform, 'macos');
    assert.ok(!(await client.callTool({ name: 'computer_stop', arguments: {} })).isError);
  } finally { await client.close(); }
}

async function verifyRelocation(stage) {
  const relocated = `${stage}-relocated`;
  await rename(stage, relocated);
  try { await verifyPackage(relocated); }
  finally { await rename(relocated, stage); }
}

async function buildPackage(stage, output) {
  console.log('Building TypeScript packages and the native release helper...');
  run('pnpm', ['--dir', join(root, 'typescript'), 'build']);
  const built = releaseArtifacts(run('cargo', ['build', '--manifest-path', join(root, 'native/Cargo.toml'), '--release', '--locked', '-p', 'cu-helper', '--message-format=json-render-diagnostics']));
  console.log('Deploying production dependencies and the sibling Swift bridge...');
  run('pnpm', ['--dir', join(root, 'typescript'), '--filter', '@independent-cu/mcp', 'deploy', '--prod', join(stage, 'mcp')]);
  await mkdir(join(stage, 'native'));
  await cp(built.helper, join(stage, 'native/cu-helper'));
  await cp(built.bridge, join(stage, 'native/libcu_macos_bridge.dylib'));
  prepareNative(join(stage, 'native'));
  await cp(join(root, 'LICENSE'), join(stage, 'LICENSE'));
  const manifest = { artifact, profile: 'release', platform: process.platform, arch: process.arch,
    commit: run('git', ['rev-parse', 'HEAD']).trim(), source_dirty: Boolean(run('git', ['status', '--porcelain']).trim()),
    signing: 'ad-hoc', node: '>=24.14 <25', built_at: new Date().toISOString() };
  await writeFile(join(stage, marker), JSON.stringify(manifest, null, 2) + '\n');
  const entry = { type: 'stdio', command: process.execPath,
    args: [join(output, 'mcp/dist/main.js'), '--helper', join(output, 'native/cu-helper')], env: {} };
  await writeFile(join(stage, 'mcp.json'), JSON.stringify({ mcpServers: { 'app-use': entry } }, null, 2) + '\n');
}

async function main() {
  const { values } = parseArgs({ options: { 'out-dir': { type: 'string' }, help: { type: 'boolean' } } });
  if (values.help) { console.log('Usage: node scripts/package-macos.mjs [--out-dir directory]\nDefault: dist/app-use. Requires macOS, Rust, Swift, pnpm and Node 24.14+.'); return; }
  assert.equal(process.platform, 'darwin', 'macOS packaging requires a native Mac');
  const [major, minor] = process.versions.node.split('.').map(Number);
  assert.ok(major === 24 && minor >= 14, 'Packaging requires Node >=24.14 <25');
  const output = validateDestination(resolve(root, values['out-dir'] ?? 'dist/app-use'));
  if (await exists(output)) await requirePackage(output);
  await mkdir(dirname(output), { recursive: true });
  const stage = await mkdtemp(join(dirname(output), `.${basename(output)}-stage-`));
  try {
    await buildPackage(stage, output);
    console.log('Verifying signatures, dependency links and relocated native MCP connectivity without requesting permissions...');
    await verifyRelocation(stage);
    await publishPackage(stage, output);
    console.log(`Release package verified: ${output}\nMCP configuration: ${join(output, 'mcp.json')}\nAd-hoc signing is not Developer ID signing and does not guarantee TCC permissions survive rebuilds.`);
  } finally { await rm(stage, { recursive: true, force: true }); }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main().catch(error => { console.error(error.stack ?? error); process.exitCode = 1; });
}
