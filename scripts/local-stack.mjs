import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const compose = resolve(root, 'contracts/local/compose.yml');
const manifest = resolve(root, 'contracts/local/deployment.json');
const deploy = process.argv.includes('--deploy');
const children = [];
const sleep = ms => new Promise(r => setTimeout(r, ms));

function run(command, args, env) {
  const child = spawn(command, args, { cwd: root, env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
  const prefix = env?.PREFIX ?? command;
  for (const stream of [child.stdout, child.stderr]) stream.on('data', chunk => process.stdout.write(`[${prefix}] ${chunk}`));
  children.push(child);
  return child;
}
async function rpcReady() {
  for (let i = 0; i < 120; i++) {
    try {
      const response = await fetch('http://127.0.0.1:27657/status', { signal: AbortSignal.timeout(2000) });
      const data = await response.json();
      if (data.result?.node_info?.network === 'spaceship-racer-local') return;
    } catch { /* Chain still starting. */ }
    await sleep(1000);
  }
  throw new Error('LocalTerra did not become ready');
}
function stop(code = 0) {
  for (const child of children) child.kill('SIGTERM');
  setTimeout(() => process.exit(code), 500);
}
process.on('SIGINT', () => stop(0)); process.on('SIGTERM', () => stop(0));

if (deploy) {
  const up = spawnSync('docker', ['compose', '-f', compose, 'up', '-d'], { cwd: root, stdio: 'inherit' });
  if (up.status !== 0) process.exit(up.status ?? 1);
}
await rpcReady();
if (deploy || !existsSync(manifest)) {
  console.log('[stack] deploying contracts via smoke test');
  const smoke = spawnSync(process.execPath, [resolve(root, 'contracts/local/smoke.mjs')], { cwd: resolve(root, 'contracts'), stdio: 'inherit' });
  if (smoke.status !== 0) process.exit(smoke.status ?? 1);
}
const stack = JSON.parse(readFileSync(manifest, 'utf8'));
const port = process.env.RACER_PORT ?? String(stack.indexer?.port ?? 8787);
run(process.execPath, ['src/onchain/server.mjs'], { PREFIX: 'indexer', RACER_PORT: port,
  RACER_MANIFEST: manifest, RACER_DATABASE: process.env.RACER_DATABASE ?? resolve(root, 'data', 'local-stack.sqlite') });
run(process.execPath, ['contracts/local/keeper.mjs'], { PREFIX: 'keeper' });
run(process.execPath, ['contracts/local/keeper-treasury.mjs'], { PREFIX: 'treasury-keeper', TREASURY_INTERVAL_MS: process.env.TREASURY_INTERVAL_MS ?? '30000' });
console.log(`[stack] indexer http://127.0.0.1:${port} · race keeper · treasury keeper · Ctrl-C stops`);
