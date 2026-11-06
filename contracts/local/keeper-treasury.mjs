// Local-only treasury keeper: funds the global SRT/SpaceUSD reward pools from
// accumulated treasury revenue. Uses isolated Docker test keys, never mainnet.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { planRewardFunding } from './keeper-plan.mjs';

const compose = fileURLToPath(new URL('./compose.yml', import.meta.url));
const deployment = JSON.parse(readFileSync(new URL('./deployment.json', import.meta.url), 'utf8'));
assert.equal(deployment.chain_id, 'spaceship-racer-local', 'Refusing non-local manifest');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
function cli(args) {
  return JSON.parse(execFileSync('docker', ['compose', '-f', compose, 'exec', '-T', 'chain', 'terrad', ...args], {
    encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
  }));
}
function status() {
  const value = cli(['status']);
  assert.equal((value.node_info ?? value.NodeInfo).network, deployment.chain_id, 'Refusing non-local chain');
  return Number((value.sync_info ?? value.SyncInfo).latest_block_height);
}
function query(contract, msg) {
  return cli(['query', 'wasm', 'contract-state', 'smart', contract, JSON.stringify(msg), '--output', 'json']).data;
}
async function execute(contract, msg, action) {
  status();
  const submitted = cli(['tx', 'wasm', 'execute', contract, JSON.stringify(msg), '--from', deployment.key_name ?? 'test1',
    '--keyring-backend', 'test', '--chain-id', deployment.chain_id, '--node', 'tcp://127.0.0.1:26657',
    '--gas', 'auto', '--gas-adjustment', '1.3', '--gas-prices', `${deployment.gas_price_uluna ?? '28.325'}uluna`,
    '--broadcast-mode', 'sync', '-y', '--output', 'json']);
  assert.equal(Number(submitted.code ?? 0), 0, submitted.raw_log);
  for (let i = 0; i < 100; i++) {
    await sleep(300);
    let result;
    try { result = cli(['query', 'tx', submitted.txhash, '--output', 'json']); } catch { continue; }
    assert.equal(Number(result.code ?? 0), 0, result.raw_log);
    return { action, txhash: submitted.txhash, gas_used: result.gas_used, gas_wanted: result.gas_wanted };
  }
  throw new Error(`Confirmation timeout: ${submitted.txhash}`);
}

export async function treasuryPass({ retain = process.env.TREASURY_RETAIN ?? '0' } = {}) {
  const treasury = deployment.capitalTreasury;
  const config = query(treasury, { config: {} });
  if (!config.reward_pool) return { action: 'reward_pool_not_configured' };
  const tokens = { SRT: deployment.srt, SpaceUSD: deployment.spaceusd };
  const balances = Object.fromEntries(Object.entries(tokens).map(([symbol, token]) => [symbol, query(treasury, { balance: { token } })]));
  const plan = planRewardFunding(balances, config.reward_pool, retain);
  if (!plan.length) return { action: 'nothing_to_fund', balances };
  for (const { token, amount } of plan) {
    const funded = await execute(treasury, { fund_reward_pool: { token: tokens[token], amount } }, `fund_${token.toLowerCase()}`);
    return { ...funded, treasury, reward_pool: config.reward_pool, token, amount, balances };
  }
  return { action: 'nothing_to_fund', balances };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const once = process.argv.includes('--once');
  const loop = async () => {
    try { console.log(JSON.stringify(await treasuryPass())); }
    catch (error) { console.error(JSON.stringify({ action: 'error', error: error.message })); process.exitCode = 1; if (once) return; }
    if (!once) setTimeout(loop, Number(process.env.TREASURY_INTERVAL_MS ?? 30000));
  };
  await loop();
}
