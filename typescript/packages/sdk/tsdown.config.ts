import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['src/index.ts'],
  root: 'src',
  outDir: 'dist',
  format: 'esm',
  platform: 'node',
  target: 'es2023',
  unbundle: true,
  dts: true,
  clean: true,
  fixedExtension: false,
  deps: {
    neverBundle: [/^@independent-cu\//],
  },
});
