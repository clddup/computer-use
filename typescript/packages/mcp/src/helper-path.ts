import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function resolveHelperPath(entrypoint: string, override?: string): string {
  if (override !== undefined) return override;
  const directory = dirname(fileURLToPath(entrypoint));
  const binary = process.platform === 'win32' ? 'cu-helper.exe' : 'cu-helper';
  const packageRoot = resolve(directory, '../..');
  if (existsSync(resolve(packageRoot, 'manifest.json'))) return resolve(packageRoot, 'native', binary);
  return resolve(directory, '../../../../native/target/debug', binary);
}
