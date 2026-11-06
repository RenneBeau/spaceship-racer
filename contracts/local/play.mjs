// Switch a smoke deployment to normal gameplay without replacing wallets,
// ships, balances, reward pools, or historical race/cycle records.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { legacyParameters } from './parameters.mjs';

const deployment = JSON.parse(readFileSync(new URL('./deployment.json', import.meta.url)));
const compose = fileURLToPath(new URL('./compose.yml', import.meta.url));
const cli = args => JSON.parse(execFileSync('docker', ['compose', '-f', compose, 'exec', '-T', 'chain', 'terrad', ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }));
const query = msg => cli(['query', 'wasm', 'contract-state', 'smart', deployment.game, JSON.stringify(msg), '--output', 'json']).data;
const status = cli(['status']);
assert.equal((status.node_info ?? status.NodeInfo).network, 'spaceship-racer-local');
const parameters = legacyParameters();
if (JSON.stringify(query({ config: {} }).parameters) === JSON.stringify(parameters)) {
  console.log('Normal gameplay parameters already configured.');
} else {
  let ready = false;
  for (let i = 0; i < 200; i++) {
    const state = query({ state: {} });
    if (state.phase === 'boundary' && state.completed === 0) { ready = true; break; }
    console.log(execFileSync(process.execPath, [fileURLToPath(new URL('./keeper.mjs', import.meta.url)), '--once', '--stop-at-boundary'], { encoding: 'utf8' }).trim());
    await new Promise(resolve => setTimeout(resolve, 500));
  }
  assert.ok(ready, 'Could not reach a cycle boundary; leave the keeper stopped and inspect chain state');
  const submitted = cli(['tx', 'wasm', 'execute', deployment.game, JSON.stringify({ configure: { paused: null, parameters } }),
    '--from', deployment.key_name ?? 'test1', '--keyring-backend', 'test', '--chain-id', deployment.chain_id,
    '--gas', 'auto', '--gas-adjustment', '1.3', '--gas-prices', `${deployment.gas_price_uluna}uluna`, '--broadcast-mode', 'sync', '-y', '--output', 'json']);
  assert.equal(Number(submitted.code ?? 0), 0, submitted.raw_log);
  let confirmed = false;
  for (let i = 0; i < 100; i++) {
    await new Promise(resolve => setTimeout(resolve, 500));
    let result; try { result = cli(['query', 'tx', submitted.txhash, '--output', 'json']); } catch { continue; }
    assert.equal(Number(result.code ?? 0), 0, result.raw_log); confirmed = true; break;
  }
  assert.ok(confirmed, 'Gameplay configuration confirmation timed out');
  assert.equal(query({ config: {} }).parameters.races_per_cycle, 100);
  console.log(JSON.stringify({ action: 'normal_gameplay_configured', races_per_cycle: 100, duration_ticks: parameters.duration_cdf.length, txhash: submitted.txhash }));
}
