import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { legacyParameters, commanderCatalogue, skinCatalogue } from './parameters.mjs';
const compose = fileURLToPath(new URL('./compose-alpha.yml', import.meta.url)), output = fileURLToPath(new URL('./alpha-deployment.json', import.meta.url));
assert.ok(!existsSync(output), 'Alpha is already deployed; refusing to overwrite its manifest');
const cli = args => JSON.parse(execFileSync('docker', ['compose', '-f', compose, 'exec', '-T', 'chain', 'terrad', ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }));
const query = (contract, value) => cli(['query', 'wasm', 'contract-state', 'smart', contract, JSON.stringify(value), '--output', 'json']).data;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
assert.equal((cli(['status']).node_info ?? cli(['status']).NodeInfo).network, 'spaceship-racer-alpha');
const owner = cli(['keys', 'show', 'alpha-operator', '--keyring-backend', 'test', '--output', 'json']).address;
const keeper = cli(['keys', 'show', 'alpha-keeper', '--keyring-backend', 'test', '--output', 'json']).address;
const faucet = cli(['keys', 'show', 'alpha-faucet', '--keyring-backend', 'test', '--output', 'json']).address;
const journal = { chain_id: 'spaceship-racer-alpha', owner, keeper_address: keeper, faucet_address: faucet, codes: {}, createdAt: new Date().toISOString() };
const checkpoint = () => writeFileSync(new URL('./alpha-deploy-progress.json', import.meta.url), JSON.stringify(journal, null, 2));
async function tx(args) {
  const sent = cli(['tx', 'wasm', ...args, '--from', 'alpha-operator', '--keyring-backend', 'test', '--chain-id', 'spaceship-racer-alpha', '--gas', 'auto', '--gas-adjustment', '1.5', '--gas-prices', '0.75uusd', '--broadcast-mode', 'sync', '-y', '--output', 'json']);
  assert.equal(Number(sent.code ?? 0), 0, sent.raw_log);
  for (let n = 0; n < 150; n++) { try { const receipt = cli(['query', 'tx', sent.txhash, '--output', 'json']); assert.equal(Number(receipt.code ?? 0), 0, receipt.raw_log); return receipt; } catch (error) { if (error.code === 'ERR_ASSERTION') throw error; } await sleep(300); }
  throw Error(`Alpha transaction timeout ${sent.txhash}`);
}
const attr = (receipt, name) => receipt.events.flatMap(event => event.attributes).find(a => a.key === name)?.value;
async function store(name, dex = false) { const code = Number(attr(await tx(['store', dex ? `/dex-artifacts/${name}.optimized.wasm` : `/artifacts/racer_${name}.wasm`]), 'code_id')); journal.codes[name] = code; checkpoint(); console.log(`Stored alpha ${name}: ${code}`); return code; }
async function instantiate(code, value, role) { const address = attr(await tx(['instantiate', String(code), JSON.stringify(value), '--label', `Spaceship alpha ${role}`, '--admin', owner]), '_contract_address'); journal[role] = address; checkpoint(); return address; }
const execute = (contract, value) => tx(['execute', contract, JSON.stringify(value)]);
const hook = value => Buffer.from(JSON.stringify(value)).toString('base64');
const code = {};
for (const name of ['local_token', 'rewards', 'fleet', 'collection', 'equipment', 'randomness', 'race', 'pause', 'marketplace', 'recruitment', 'reinvestment', 'treasury', 'clans', 'skins', 'beacon_verifier', 'local_lp']) code[name] = await store(name);
for (const name of ['cl8y_dex_pair', 'cl8y_dex_factory', 'cl8y_dex_router']) code[name] = await store(name, true);
const tokens = {};
for (const [name, symbol] of [['clunc', 'CLUNC'], ['custc', 'CUSTC'], ['ust1', 'UST'], ['spaceusd', 'USD'], ['srt', 'SRT']]) tokens[name] = await instantiate(code.local_token, { name: `Alpha ${name === 'spaceusd' ? 'SpaceUSD' : symbol} (test only)`, symbol, decimals: 6, initial_balances: [{ address: owner, amount: '1000000000000000' }], mint: { minter: faucet, cap: null }, marketing: null }, name);
const paymentTokens = [tokens.clunc, tokens.custc, tokens.ust1, tokens.spaceusd, tokens.srt], usd = tokens.spaceusd, srt = tokens.srt;
const pause = await instantiate(code.pause, { owner }, 'pause');
const backing = await instantiate(code.rewards, { owner, allocator: owner, spaceusd: usd, srt, annual_bps: 2500 }, 'backing_rewards');
const fleet = await instantiate(code.fleet, { owner, engine: owner, spaceusd: usd, treasury: owner, price_per_tier: '1000000' }, 'fleet');
const commanderCollection = await instantiate(code.collection, { owner, minter: owner, name: 'Alpha Commanders', symbol: 'ACMD' }, 'commanderCollection');
const skinCollection = await instantiate(code.collection, { owner, minter: owner, name: 'Alpha Ship Skins', symbol: 'ASKIN' }, 'skinCollection');
const equipment = await instantiate(code.equipment, { owner, engine: owner, collections: [commanderCollection, skinCollection] }, 'equipment');
const randomness = await instantiate(code.randomness, { owner, consumers: [], safety_seconds: 6 }, 'randomness');
const game = await instantiate(code.race, { owner, fleet, equipment, rewards: backing, randomness, srt, treasury: owner, parameters: legacyParameters(), resume_from: null }, 'game');
const factory = await instantiate(code.cl8y_dex_factory, { governance: owner, treasury: owner, default_fee_bps: 30, pair_code_id: code.cl8y_dex_pair, lp_token_code_id: code.local_lp, whitelisted_code_ids: [code.local_token], default_limit_batch_max_rungs: 20, pair_creation_fee_uluna: '0' }, 'dexFactory');
const tokenInfo = address => ({ token: { contract_addr: address } });
const pairs = [];
for (const token of [tokens.clunc, tokens.custc, tokens.ust1, srt]) {
  const asset_infos = [tokenInfo(token), tokenInfo(usd)]; await execute(factory, { create_pair: { asset_infos } });
  const pair = query(factory, { pair: { asset_infos } }).pair; pairs.push(pair.contract_addr, pair.liquidity_token);
  for (const asset of [token, usd]) await execute(asset, { increase_allowance: { spender: pair.contract_addr, amount: '1000000000000', expires: null } });
  await execute(pair.contract_addr, { provide_liquidity: { assets: [{ info: tokenInfo(token), amount: '1000000000000' }, { info: tokenInfo(usd), amount: '1000000000000' }], slippage_tolerance: '0.01', receiver: null, deadline: null } });
}
const router = await instantiate(code.cl8y_dex_router, { factory }, 'dexRouter');
const capitalTreasury = await instantiate(code.treasury, { owner, overlord: owner, router, spaceusd: usd, reward_pool: backing, payment_tokens: paymentTokens, max_spread: '0.02' }, 'capitalTreasury');
const gateway = await instantiate(code.reinvestment, { payments: { owner, fleet, spaceusd: usd, router, payment_tokens: paymentTokens, max_spread: '0.02' }, backing_rewards: backing, allocator: game }, 'reinvestment');
const clans = await instantiate(code.clans, { owner, engine: game, fleet, current_cycle: 0, restore_from: null }, 'clans');
const marketplace = await instantiate(code.marketplace, { owner, treasury: capitalTreasury, fee_bps: 500, collections: [commanderCollection, skinCollection], payment_tokens: paymentTokens }, 'marketplace');
const recruitment = await instantiate(code.recruitment, { owner, srt, collection: commanderCollection, randomness, treasury: capitalTreasury, pull_price: '500000000', catalogue: commanderCatalogue() }, 'recruitment');
const skins = await instantiate(code.skins, { owner, fleet, collection: skinCollection, randomness, milestone_step: 100, catalogue: skinCatalogue() }, 'skins');
const verifier = await instantiate(code.beacon_verifier, {}, 'tick_verifier');
await execute(fleet, { configure: { engine: game, treasury: capitalTreasury, price_per_tier: null, paused: null } });
await execute(fleet, { set_payment_adapter: { adapter: gateway } });
await execute(equipment, { configure: { engine: game, paused: null, collection: null, allowed: null } });
await execute(backing, { configure: { allocator: gateway, annual_bps: null, paused: null } });
await tx(['migrate', game, String(code.race), JSON.stringify({ reward_gateway: gateway })]);
await execute(game, { set_clans: { address: clans } }); await execute(game, { set_tick_verifier: { address: verifier } });
await execute(game, { configure_healing: { settings: { enabled: true, price_unit: '1000000', hp_per_unit: '1000', fallen_ship_bps: 2500 } } });
for (const contract of [game, recruitment, skins]) await execute(randomness, { configure: { paused: null, consumer: contract, allowed: true } });
for (const token of [tokens.clunc, tokens.custc, tokens.ust1, srt]) for (const contract of [gateway, capitalTreasury]) await execute(contract, { set_route: { token, path: [token, usd] } });
for (const [collection, minter] of [[commanderCollection, recruitment], [skinCollection, skins]]) {
  for (const custodian of [equipment, marketplace]) await execute(collection, { extension: { msg: { set_custodian: { address: custodian, allowed: true } } } });
  await execute(collection, { extension: { msg: { set_pause_controller: { controller: pause } } } });
  await execute(collection, { update_ownership: { transfer_ownership: { new_owner: minter, expiry: null } } }); await execute(minter, { accept_minter: {} });
}
for (const contract of [backing, fleet, equipment, randomness, game, gateway, capitalTreasury, clans, marketplace, recruitment, skins]) await execute(contract, { set_pause_controller: { controller: pause } });
await execute(usd, { send: { contract: gateway, amount: '1000000000000', msg: hook({ fund: {} }) } });
// Do not seed the SRT reward pool: it grows from actual alpha gameplay spending.
const manifest = { ...journal, alpha: true, owner, game, randomness, pause, recruitment, skins, skinCollection, fleet, equipment, rewards: gateway, clans, payments: gateway, capitalTreasury, marketplace, commanderCollection, paymentTokens, srt, spaceusd: usd, tokens,
  backing_rewards: backing, reinvestment: gateway, reward_pool: gateway, collapsar: owner, dex_contracts: [factory, router, ...pairs],
  indexer: { rpc: 'http://127.0.0.1:27659', lcd: 'http://127.0.0.1:1419' }, compose, key_name: 'alpha-operator', keeper_key: 'alpha-keeper', keeper_address: keeper, faucet_key: 'alpha-faucet', faucet_address: faucet, keeper_fee_denom: 'uluna', keeper_gas_price: '28.325', gas_price_uluna: '28.325', timing_version: 2, accounting_code_id: code.race, reinvestment_code_id: code.reinvestment, tick_verifier: verifier, previous_games: [], previous_clans: [] };
writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`); console.log(JSON.stringify({ alpha: true, chain: manifest.chain_id, game, contracts: Object.keys(journal).length, manifest: output, presaleEnabled: false }));
