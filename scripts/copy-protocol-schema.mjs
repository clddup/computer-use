import { copyFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = resolve(root, 'typescript/packages/protocol/src/schema.json');
const destination = resolve(root, 'typescript/packages/protocol/dist/schema.json');
await copyFile(source, destination);
