import { afterEach, describe, expect, it } from 'vitest';
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { publishPackage, releaseArtifacts, rpaths, validateDestination, verifyLinks } from '../../scripts/package-macos.mjs';
import { resolveHelperPath } from '../packages/mcp/dist/helper-path.js';

const root = resolve(import.meta.dirname, '../..');
const runs = [];
async function directory() {
  await mkdir(join(root, '.local'), { recursive: true });
  const path = await mkdtemp(join(root, '.local/package-test-'));
  runs.push(path); return path;
}
async function managed(path, version) {
  await mkdir(path);
  await writeFile(join(path, 'manifest.json'), JSON.stringify({ artifact: 'app-use-macos', version }));
}
afterEach(async () => { for (const path of runs.splice(0)) await rm(path, { recursive: true, force: true }); });

describe('macOS release packaging without GUI or permission requests', () => {
  it('uses the exact Cargo release artifact and reported Swift bridge, not a globbed build cache', () => {
    const messages = [
      { reason: 'build-script-executed', linked_libs: ['dylib=cu_macos_bridge'], out_dir: '/build with spaces/out' },
      { reason: 'compiler-artifact', target: { name: 'cu-helper' }, executable: '/release/cu-helper', profile: { opt_level: '3', debug_assertions: false } },
    ];
    expect(releaseArtifacts(messages.map(JSON.stringify).join('\n'))).toEqual({ helper: '/release/cu-helper', bridge: join('/build with spaces/out', 'libcu_macos_bridge.dylib') });
    messages[1].profile.debug_assertions = true;
    expect(() => releaseArtifacts(messages.map(JSON.stringify).join('\n'))).toThrow('release helper');
    expect(() => releaseArtifacts(JSON.stringify(messages[1]))).toThrow();
  });

  it('parses Mach-O runtime search paths including paths with spaces', () => {
    expect(rpaths(' cmd LC_RPATH\n cmdsize 112\n path /build with spaces/out (offset 12)\n cmd LC_RPATH\n cmdsize 32\n path @executable_path (offset 12)\n')).toEqual(['/build with spaces/out', '@executable_path']);
    expect(rpaths(' cmd LC_LOAD_DYLIB\n name /usr/lib/libSystem.B.dylib')).toEqual([]);
    expect(() => validateDestination(root)).toThrow();
    expect(() => validateDestination(resolve(root, '..'))).toThrow();
    expect(validateDestination(join(root, 'dist/app-use'))).toBe(join(root, 'dist/app-use'));
  });

  it('replaces only a managed package and removes the previous package after success', async () => {
    const base = await directory(), stage = join(base, 'stage'), output = join(base, 'output');
    await managed(stage, 2); await managed(output, 1);
    await writeFile(join(output, 'old-file'), 'old');
    await publishPackage(stage, output);
    expect(JSON.parse(await readFile(join(output, 'manifest.json'), 'utf8')).version).toBe(2);
    expect(await readdir(base)).toEqual(['output']);
    expect(await readdir(output)).toEqual(['manifest.json']);
  });

  it('never replaces an unrelated directory or damages it when staging is missing', async () => {
    const base = await directory(), stage = join(base, 'stage'), output = join(base, 'output');
    await managed(stage, 2); await mkdir(output); await writeFile(join(output, 'keep'), 'user data');
    await expect(publishPackage(stage, output)).rejects.toThrow();
    await expect(publishPackage(join(base, 'missing'), output)).rejects.toThrow();
    expect(await readFile(join(output, 'keep'), 'utf8')).toBe('user data');
  });

  it.skipIf(process.platform === 'win32')('accepts internal relative dependency links and rejects absolute or escaping links', async () => {
    const base = await directory(), output = join(base, 'package');
    await mkdir(output); await writeFile(join(output, 'dependency'), 'fixture');
    await symlink('dependency', join(output, 'link')); await verifyLinks(output);
    await rm(join(output, 'link'));
    await symlink(join(output, 'dependency'), join(output, 'link'));
    await expect(verifyLinks(output)).rejects.toThrow('Absolute package symlink');
    await rm(join(output, 'link')); await writeFile(join(base, 'outside'), 'fixture');
    await symlink('../outside', join(output, 'link'));
    await expect(verifyLinks(output)).rejects.toThrow('escapes');
  });

  it('resolves a packaged helper without falling back to debug when the packaged binary is missing', async () => {
    const base = await directory(), output = join(base, 'package');
    await managed(output, 1);
    const entrypoint = pathToFileURL(join(output, 'mcp/dist/main.js')).href;
    const binary = process.platform === 'win32' ? 'cu-helper.exe' : 'cu-helper';
    expect(resolveHelperPath(entrypoint)).toBe(join(output, 'native', binary));
    expect(resolveHelperPath(entrypoint, '/explicit/helper')).toBe('/explicit/helper');
    expect(resolveHelperPath(pathToFileURL(join(root, 'typescript/packages/mcp/dist/main.js')).href)).toBe(join(root, 'native/target/debug', binary));
  });
});
