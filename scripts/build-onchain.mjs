import { build } from 'esbuild';
await build({ entryPoints: ['front/onchain/app.mjs'], bundle: true, platform: 'browser', format: 'iife', target: 'es2022',
  outfile: 'front/onchain/dist/app.js', minify: true, sourcemap: false, external: ['/assets/*'], define: { global: 'globalThis' } });
