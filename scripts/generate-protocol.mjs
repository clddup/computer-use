import { spawnSync } from 'node:child_process';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve, dirname } from 'node:path';
import { createRequire } from 'node:module';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(resolve(root, 'typescript/package.json'));
const { compile } = require('json-schema-to-typescript');
const binary = process.env.CU_HELPER ?? resolve(root, 'native/target/debug', process.platform === 'win32' ? 'cu-helper.exe' : 'cu-helper');
const result = spawnSync(binary, ['--schema'], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
if (result.error || result.status !== 0) throw new Error(`Build native helper first: ${result.error ?? result.stderr}`);
const schema = JSON.parse(result.stdout);
const json = JSON.stringify(schema, null, 2) + '\n';
const types = await compile(schema, 'ProtocolTypes', { bannerComment: '// Generated from the Rust wire schema. Run protocol:generate; do not edit.', additionalProperties: false });
const files = new Map([
  ['protocol/schema.json', json],
  ['typescript/packages/protocol/src/schema.json', json],
  ['typescript/packages/protocol/src/generated.ts', types],
]);
for (const [name, content] of files) {
  const path = resolve(root, name);
  if (process.argv.includes('--check')) {
    const actual = await readFile(path, 'utf8');
    const matches = name.endsWith('.json')
      ? JSON.stringify(JSON.parse(actual)) === JSON.stringify(JSON.parse(content))
      : actual === content;
    if (!matches) throw new Error(`Protocol drift: ${name}`);
  } else {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
  }
}
console.log(process.argv.includes('--check') ? 'Protocol schema and generated TS agree.' : 'Generated shared schema and TypeScript types.');
