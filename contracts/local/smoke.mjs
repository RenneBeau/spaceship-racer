// Compiled-Wasm smoke test against this project's isolated LocalTerra only.
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { statSync, writeFileSync } from 'node:fs';
import { commanderCatalogue, legacyParameters, skinCatalogue } from './parameters.mjs';

const compose = fileURLToPath(new URL('./compose.yml', import.meta.url));
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
function cli(args, json = true) {
  const output = execFileSync('docker', ['compose', '-f', compose, 'exec', '-T', 'chain',
    'terrad', ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  return json ? JSON.parse(output) : output.trim();
}
async function tx(args, key = 'test1', module = 'wasm') {
  const submitted = cli(['tx', module, ...args, '--from', key, '--keyring-backend', 'test',
    '--chain-id', 'spaceship-racer-local', '--node', 'tcp://127.0.0.1:26657',
    '--gas', '15000000', '--fees', module === 'bank' ? '5000000000uluna' : '500000000uluna', '--broadcast-mode', 'sync', '-y', '--output', 'json']);
  assert.equal(Number(submitted.code ?? 0), 0, submitted.raw_log);
  for (let i = 0; i < 100; i++) {
    await sleep(300);
    let result;
    try { result = cli(['query', 'tx', submitted.txhash, '--output', 'json']); }
    catch { continue; }
    assert.equal(Number(result.code ?? 0), 0, result.raw_log);
    return result;
  }
  throw new Error(`Transaction confirmation timeout: ${submitted.txhash}`);
}
function attribute(result, key) {
  for (const event of result.events ?? []) {
    for (const attr of event.attributes ?? []) {
      if (attr.key === key) return attr.value;
    }
  }
  throw new Error(`Missing event attribute ${key}: ${JSON.stringify(result)}`);
}
async function store(name) {
  return attribute(await tx(['store', `/artifacts/${name}.wasm`]), 'code_id');
}
async function storeDex(name) {
  return attribute(await tx(['store', `/dex-artifacts/${name}.optimized.wasm`]), 'code_id');
}
async function instantiate(code, msg, label) {
  return attribute(await tx(['instantiate', code, JSON.stringify(msg), '--label', label, '--no-admin']), '_contract_address');
}
const execute = (contract, msg, key = 'test1') => tx(['execute', contract, JSON.stringify(msg)], key);
const query = (contract, msg) => cli(['query', 'wasm', 'contract-state', 'smart', contract,
  JSON.stringify(msg), '--output', 'json']).data;

for (let i = 0; ; i++) {
  try {
    const status = cli(['status']);
    const network = (status.node_info ?? status.NodeInfo).network;
    assert.equal(network, 'spaceship-racer-local', 'Refusing non-project chain');
    if (Number((status.sync_info ?? status.SyncInfo).latest_block_height) > 1) break;
  } catch (error) { if (i >= 100) throw error; }
  await sleep(500);
}
const owner = cli(['keys', 'show', 'test1', '-a', '--keyring-backend', 'test'], false);
function fixtureKey(name) {
  const existing = cli(['keys', 'list', '--keyring-backend', 'test', '--output', 'json']).find(k => k.name === name);
  if (existing) return existing.address;
  // Fixture keys live only inside this chain's Docker volume. Never print key material.
  return cli(['keys', 'add', name, '--keyring-backend', 'test', '--output', 'json']).address;
}
const buyer = fixtureKey('racer-buyer');
const treasuryWallet = fixtureKey('racer-treasury');
await tx(['send', owner, buyer, '50000000000uluna'], 'test1', 'bank');
const tokenCode = await store('racer_local_token');
const rewardCode = await store('racer_rewards');
const fleetCode = await store('racer_fleet');
const collectionCode = await store('racer_collection');
const equipmentCode = await store('racer_equipment');
const randomnessCode = await store('racer_randomness');
const raceCode = await store('racer_race');
const pauseCode = await store('racer_pause');
const marketplaceCode = await store('racer_marketplace');
const recruitmentCode = await store('racer_recruitment');
const paymentsCode = await store('racer_payments');
const treasuryCode = await store('racer_treasury');
const clansCode = await store('racer_clans');
const skinsCode = await store('racer_skins');
const tokenMsg = symbol => ({ name: `Local ${symbol}`, symbol, decimals: 6,
  initial_balances: [{ address: owner, amount: '1000000000000' }], mint: null, marketing: null });
const usd = await instantiate(tokenCode, tokenMsg('USD'), 'Local SpaceUSD');
const srt = await instantiate(tokenCode, tokenMsg('SRT'), 'Local SRT');
const rewards = await instantiate(rewardCode, { owner, allocator: owner, spaceusd: usd, srt,
  annual_bps: 2500 }, 'Racer rewards smoke');
const fleet = await instantiate(fleetCode, { owner, engine: owner, spaceusd: usd, treasury: owner,
  price_per_tier: '1000000' }, 'Racer fleet smoke');
const collection = await instantiate(collectionCode, { owner, minter: owner, name: 'Local commanders',
  symbol: 'CMD' }, 'Racer commander collection smoke');
const equipment = await instantiate(equipmentCode, { owner, engine: owner, collections: [collection] }, 'Racer equipment smoke');
const randomness = await instantiate(randomnessCode, { owner, consumers: [owner], safety_seconds: 6 }, 'Racer quicknet smoke');
const pause = await instantiate(pauseCode, { owner }, 'Racer global pause');
async function wirePause(addresses, collections = []) {
  for (const address of addresses) await execute(address, { set_pause_controller: { controller: pause } });
  for (const address of collections) await execute(address, { extension: { msg: { set_pause_controller: { controller: pause } } } });
}
await wirePause([rewards, fleet, equipment, randomness], [collection]);
await execute(randomness, { request: { id: 1 } });
async function relay(randomness, consumer, id) {
const request = query(randomness, { request: { consumer, id } });
let beacon;
for (let i = 0; i < 30; i++) {
  try {
    const response = await fetch(`https://api.drand.sh/52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971/public/${request.round}`,
      { signal: AbortSignal.timeout(10000) });
    if (response.ok) { beacon = await response.json(); break; }
  } catch { /* Retry transient network failures; the exact round never changes. */ }
  await sleep(1000);
}
assert.ok(beacon, 'Requested drand round unavailable');
// The chain can trail wall-clock time by a block. Submit only once the committed
// block timestamp reaches the round's release time, otherwise the contract
// correctly rejects the proof as not yet due.
const due = Number(query(randomness, { request: { consumer, id } }).available_at);
for (let i = 0; i < 90; i++) {
  const sync = cli(['status']);
  const chainTime = Date.parse((sync.sync_info ?? sync.SyncInfo).latest_block_time) / 1000;
  if (chainTime >= due + 1) break;
  await sleep(1000);
}
const proofTx = await execute(randomness, { submit: { consumer, id,
  signature: Buffer.from(beacon.signature, 'hex').toString('base64') } });
assert.equal(Buffer.from(query(randomness, { request: { consumer, id } }).randomness, 'base64').toString('hex'), beacon.randomness);
return proofTx;
}
const proofTx = await relay(randomness, owner, 1);
await execute(collection, { mint: { token_id: 'nova', owner, token_uri: null, extension: {
  code: 'cadet_nova', kind: 'commander', rarity: 'common', bonus: 'speed', bonus_bps: 300 } } });
await execute(collection, { send_nft: { contract: equipment, token_id: 'nova',
  msg: Buffer.from(JSON.stringify({ slot: 'commander' })).toString('base64') } });
assert.equal(query(collection, { owner_of: { token_id: 'nova' } }).owner, equipment);
assert.equal(query(equipment, { position: { wallet: owner, slot: 'commander' } }).active, null);
await execute(equipment, { begin_boundary: {} });
await execute(equipment, { process: { wallets: [owner] } });
await execute(equipment, { end_boundary: {} });
assert.equal(query(equipment, { position: { wallet: owner, slot: 'commander' } }).active.token_id, 'nova');
await execute(equipment, { unbind: { slot: 'commander' } });
assert.equal(query(collection, { owner_of: { token_id: 'nova' } }).owner, equipment);
await execute(equipment, { begin_boundary: {} });
await execute(equipment, { process: { wallets: [owner] } });
await execute(equipment, { end_boundary: {} });
assert.equal(query(collection, { owner_of: { token_id: 'nova' } }).owner, owner);
await execute(usd, { send: { contract: fleet, amount: '1000000', msg:
  Buffer.from(JSON.stringify({ buy: { ship: 'racer', target_tier: 1, sponsor: null } })).toString('base64') } });
assert.deepEqual(query(fleet, { fleet: { wallet: owner } }).active, [0, 0, 0]);
await execute(fleet, { advance_race: { race: 1 } });
assert.deepEqual(query(fleet, { fleet: { wallet: owner } }).active, [1, 0, 0]);
const fund = Buffer.from(JSON.stringify({ fund: {} })).toString('base64');
await execute(usd, { send: { contract: rewards, amount: '100000000', msg: fund } });
await execute(srt, { send: { contract: rewards, amount: '10000', msg: fund } });
await execute(rewards, { allocate: { recipients: [{ wallet: owner, spaceusd: '0', srt: '4000' }] } });
await execute(rewards, { allocate: { recipients: [{ wallet: owner, spaceusd: '0', srt: '6000' }] } });
assert.equal(query(rewards, { credit: { wallet: owner } }).srt, '10000');
await execute(rewards, { claim: {} });
assert.equal(query(rewards, { credit: { wallet: owner } }).srt, '0');
assert.equal(query(srt, { balance: { address: owner } }).balance, '1000000000000');
const state = query(rewards, { state: {} });
assert.equal(state.liabilities.srt, '0');
assert.equal(BigInt(state.reserve) + BigInt(state.available.spaceusd), 100000000n);

// Fresh interconnected contracts: only the game engine may advance fleet races,
// activate equipment or allocate prizes. This local fixture forces an early end
// to test refunds quickly on the image's accelerated 200ms blocks.
const gameFleet = await instantiate(fleetCode, { owner, engine: owner, spaceusd: usd, treasury: owner,
  price_per_tier: '1000000' }, 'Game fleet');
const gameRewards = await instantiate(rewardCode, { owner, allocator: owner, spaceusd: usd, srt, annual_bps: 2500 }, 'Game rewards');
const gameEquipment = await instantiate(equipmentCode, { owner, engine: owner, collections: [collection] }, 'Game equipment');
const parameters = { ...legacyParameters(), races_per_cycle: 1, duration_cdf: Array(128).fill(1_000_000_000) };
const game = await instantiate(raceCode, { owner, fleet: gameFleet, equipment: gameEquipment, rewards: gameRewards,
  randomness, srt, treasury: owner, parameters }, 'Game engine');
await wirePause([game, gameFleet, gameRewards, gameEquipment]);
await execute(gameFleet, { configure: { engine: game, treasury: null, price_per_tier: null, paused: null } });
await execute(gameEquipment, { configure: { engine: game, paused: null, collection: null, allowed: null } });
await execute(gameRewards, { configure: { allocator: game, annual_bps: null, paused: null } });
await execute(randomness, { configure: { paused: null, consumer: game, allowed: true } });
await execute(usd, { send: { contract: gameFleet, amount: '1000000', msg:
  Buffer.from(JSON.stringify({ buy: { ship: 'racer', target_tier: 1, sponsor: null } })).toString('base64') } });
await execute(collection, { send_nft: { contract: gameEquipment, token_id: 'nova',
  msg: Buffer.from(JSON.stringify({ slot: 'commander' })).toString('base64') } });
await execute(usd, { send: { contract: gameRewards, amount: '100000000', msg: fund } });
await execute(srt, { send: { contract: gameRewards, amount: '10000', msg: fund } });
let peakAdvanceGas = 0n;
async function advanceGame() {
  const state = query(game, { state: {} });
  if (state.phase === 'boundary') {
    let commitment = query(game, { multiplier: { race: state.race + 1 } });
    if (commitment.request_id === null) { await execute(game, { prepare_next_multiplier: {} }); commitment = query(game, { multiplier: { race: state.race + 1 } }); }
    if (commitment.multiplier === null) await relay(randomness, game, commitment.request_id);
  }
  const result = await execute(game, { advance: { limit: 20 } });
  if (BigInt(result.gas_used) > peakAdvanceGas) peakAdvanceGas = BigInt(result.gas_used);
}
for (let i = 0; query(game, { state: {} }).phase !== 'waiting'; i++) {
  assert.ok(i < 20, 'Game snapshot stalled');
  await advanceGame();
}
assert.equal(query(game, { player: { wallet: owner } }).commander.code, 'cadet_nova');
const boostTx = await execute(srt, { send: { contract: game, amount: '1000000', msg:
  Buffer.from(JSON.stringify({ race: 1, kind: 'speed', count: 1 })).toString('base64') } });
const boostTick = Number(attribute(boostTx, 'tick'));
await execute(game, { seal: { limit: 1 } });
const seedId = query(game, { seed_request: { race: 1, tick: 1 } });
await relay(randomness, game, seedId);
for (let i = 0; query(game, { state: {} }).cycle === 1; i++) {
  assert.ok(i < 20, 'Game settlement stalled');
  await advanceGame();
}
const raceResult = query(game, { result: { race: 1 } });
assert.equal(raceResult.winner, owner);
assert.equal(raceResult.ticks, 1);
await execute(gameRewards, { claim: {} });
if (boostTick > 1) await execute(game, { claim_refunds: { race: 1, ticks: [boostTick] } });
assert.equal(query(game, { state: {} }).escrow_srt, '0');
const backed = query(gameRewards, { state: {} });
assert.equal(BigInt(query(usd, { balance: { address: gameRewards } }).balance),
  BigInt(backed.reserve) + BigInt(backed.available.spaceusd) + BigInt(backed.liabilities.spaceusd));

// All five supported payment interfaces use local CW20 fixtures; the UST1
// fixture displays UST because cw20-base excludes digits in fixture symbols.
const clunc = await instantiate(tokenCode, tokenMsg('CLUNC'), 'Local cLUNC');
const custc = await instantiate(tokenCode, tokenMsg('CUSTC'), 'Local cUSTC');
const ust1 = await instantiate(tokenCode, tokenMsg('UST'), 'Local UST1');
const paymentTokens = [clunc, custc, ust1, usd, srt];
const marketplace = await instantiate(marketplaceCode, { owner, treasury: treasuryWallet, fee_bps: 500,
  collections: [collection], payment_tokens: paymentTokens }, 'Racer marketplace');
const recruitment = await instantiate(recruitmentCode, { owner, srt, collection, randomness, treasury: treasuryWallet,
  pull_price: '500000000', catalogue: commanderCatalogue() }, 'Racer recruitment');
await wirePause([marketplace, recruitment]);
for (let i = 0; i < 5; i++) await execute(collection, { mint: { token_id: `fusion${i}`, owner, token_uri: null,
  extension: commanderCatalogue()[0].attributes } });
await execute(collection, { update_ownership: { transfer_ownership: { new_owner: recruitment, expiry: null } } });
await execute(recruitment, { accept_minter: {} });
await execute(randomness, { configure: { paused: null, consumer: recruitment, allowed: true } });
await execute(srt, { send: { contract: recruitment, amount: '5000000000',
  msg: Buffer.from(JSON.stringify({ count: 10 })).toString('base64') } });
await relay(randomness, recruitment, 1);
const recruitTx = await execute(recruitment, { fulfill: { id: 1 } });
const draw = query(recruitment, { job: { id: 1 } });
assert.equal(draw.minted.length, 10);
assert.ok(['epic', 'legendary'].includes(query(collection, { nft_info: { token_id: draw.minted[0] } }).extension.rarity));
assert.equal(query(recruitment, { config: {} }).escrow_srt, '0');
for (let i = 0; i < 5; i++) {
  const token = paymentTokens[i];
  await execute(token, { transfer: { recipient: buyer, amount: '10000' } });
  const feesBefore = BigInt(query(token, { balance: { address: treasuryWallet } }).balance);
  await execute(collection, { send_nft: { contract: marketplace, token_id: draw.minted[i],
    msg: Buffer.from(JSON.stringify({ payment_token: token, price: '1000' })).toString('base64') } });
  await execute(token, { send: { contract: marketplace, amount: '1000',
    msg: Buffer.from(JSON.stringify({ listing_id: i + 1 })).toString('base64') } }, 'racer-buyer');
  assert.equal(query(collection, { owner_of: { token_id: draw.minted[i] } }).owner, buyer);
  assert.equal(BigInt(query(token, { balance: { address: treasuryWallet } }).balance), feesBefore + 50n);
}
await execute(collection, { approve_all: { operator: recruitment, expires: null } });
await execute(recruitment, { fuse: { token_ids: Array.from({ length: 5 }, (_, i) => `fusion${i}`) } });
for (let i = 0; i < 5; i++) assert.equal(query(collection, { owner_of: { token_id: `fusion${i}` } }).owner, recruitment);
await execute(collection, { send_nft: { contract: marketplace, token_id: draw.minted[6],
  msg: Buffer.from(JSON.stringify({ payment_token: usd, price: '1000' })).toString('base64') } });
const gameBeforePause = query(game, { state: {} });
await execute(pause, { set_paused: { paused: true } });
const pausedFailure = operation => assert.rejects(operation, /globally paused/);
await pausedFailure(() => execute(collection, { transfer_nft: { recipient: buyer, token_id: draw.minted[7] } }));
await pausedFailure(() => execute(usd, { send: { contract: marketplace, amount: '1000',
  msg: Buffer.from(JSON.stringify({ listing_id: 6 })).toString('base64') } }, 'racer-buyer'));
await pausedFailure(() => execute(recruitment, { fulfill: { id: 2 } }));
await pausedFailure(() => execute(gameEquipment, { unbind: { slot: 'commander' } }));
await pausedFailure(() => execute(gameRewards, { claim: {} }));
await pausedFailure(() => execute(game, { advance: { limit: 20 } }));
await pausedFailure(() => execute(randomness, { request: { id: 999 } }));
assert.deepEqual(query(game, { state: {} }), gameBeforePause);
await execute(pause, { set_paused: { paused: false } });
await execute(usd, { send: { contract: marketplace, amount: '1000',
  msg: Buffer.from(JSON.stringify({ listing_id: 6 })).toString('base64') } }, 'racer-buyer');
await relay(randomness, recruitment, 2);
const fusionTx = await execute(recruitment, { fulfill: { id: 2 } });
const fusion = query(recruitment, { job: { id: 2 } });
assert.equal(fusion.minted.length, 1);
assert.equal(query(collection, { owner_of: { token_id: fusion.minted[0] } }).owner, owner);
assert.equal(query(collection, { nft_info: { token_id: fusion.minted[0] } }).extension.rarity, 'rare');
for (let i = 0; i < 5; i++) assert.throws(() => query(collection, { owner_of: { token_id: `fusion${i}` } }));

// Deploy actual upstream CL8Y factory/pair/router/LP Wasm into this same isolated
// chain. None of these tests use a fake exchange or live mainnet liquidity.
const dexPairCode = await storeDex('cl8y_dex_pair');
const dexFactoryCode = await storeDex('cl8y_dex_factory');
const dexRouterCode = await storeDex('cl8y_dex_router');
const dexLpCode = await store('racer_local_lp');
const dexFactory = await instantiate(dexFactoryCode, { governance: owner, treasury: treasuryWallet, default_fee_bps: 30,
  pair_code_id: Number(dexPairCode), lp_token_code_id: Number(dexLpCode), whitelisted_code_ids: [Number(tokenCode)],
  default_limit_batch_max_rungs: 20, pair_creation_fee_uluna: '0' }, 'Local CL8Y factory');
const tokenInfo = address => ({ token: { contract_addr: address } });
for (const token of [clunc, custc, ust1, srt]) {
  const asset_infos = [tokenInfo(token), tokenInfo(usd)];
  await execute(dexFactory, { create_pair: { asset_infos } });
  const pair = query(dexFactory, { pair: { asset_infos } }).pair;
  // Fixtures hold 1e12 micro-units; keep deep but affordable 1e10/unit pools.
  for (const asset of [token, usd]) await execute(asset, { increase_allowance: { spender: pair.contract_addr, amount: '10000000000', expires: null } });
  await execute(pair.contract_addr, { provide_liquidity: { assets: [
    { info: tokenInfo(token), amount: '10000000000' }, { info: tokenInfo(usd), amount: '10000000000' },
  ], slippage_tolerance: '0.01', receiver: null, deadline: null } });
}
const dexRouter = await instantiate(dexRouterCode, { factory: dexFactory }, 'Local CL8Y router');
const capitalTreasury = await instantiate(treasuryCode, { owner, overlord: treasuryWallet, router: dexRouter, spaceusd: usd,
  reward_pool: gameRewards, payment_tokens: paymentTokens, max_spread: '0.02' }, 'Collapsar capital treasury');
const payments = await instantiate(paymentsCode, { owner, fleet: gameFleet, spaceusd: usd, router: dexRouter,
  payment_tokens: paymentTokens, max_spread: '0.02' }, 'CL8Y ship payments');
await wirePause([payments, capitalTreasury]);
await execute(gameFleet, { set_payment_adapter: { adapter: payments } });
await execute(gameFleet, { configure: { engine: null, treasury: capitalTreasury, price_per_tier: null, paused: null } });
for (const token of [clunc, custc, ust1, srt]) {
  const path = [token, usd];
  await execute(payments, { set_route: { token, path } });
  await execute(capitalTreasury, { set_route: { token, path } });
}
let peakPaymentGas = 0n;
for (let i = 0; i < paymentTokens.length; i++) {
  const token = paymentTokens[i];
  const purchase = { upgrade: { ship: 'racer', target_tier: i + 2 } };
  const quote = query(payments, { quote: { wallet: owner, token, purchase } });
  const deadline = Math.floor(Date.now() / 1000) + 300;
  const paid = await execute(token, { send: { contract: payments, amount: '1100000', msg:
    Buffer.from(JSON.stringify({ purchase, expected_price: quote.price, minimum_receive: quote.price,
      max_spread: '0.02', deadline, route: quote.route })).toString('base64') } });
  peakPaymentGas = BigInt(paid.gas_used) > peakPaymentGas ? BigInt(paid.gas_used) : peakPaymentGas;
  assert.deepEqual(query(gameFleet, { fleet: { wallet: owner } }).next, [i + 2, 0, 0]);
  assert.deepEqual(query(gameFleet, { fleet: { wallet: payments } }).next, [0, 0, 0]);
  assert.equal(query(usd, { balance: { address: payments } }).balance, '0');
}
assert.equal(query(usd, { balance: { address: capitalTreasury } }).balance, '5000000');
await execute(srt, { transfer: { recipient: capitalTreasury, amount: '2000000' } });
const convertTx = await execute(capitalTreasury, { convert: { token: srt, amount: '2000000', minimum_receive: '1900000', max_spread: '0.02',
  deadline: Math.floor(Date.now() / 1000) + 300, route: [srt, usd] } });
const overlordBefore = BigInt(query(usd, { balance: { address: treasuryWallet } }).balance);
await execute(capitalTreasury, { send_capital: { amount: '1000000' } });
assert.equal(BigInt(query(usd, { balance: { address: treasuryWallet } }).balance), overlordBefore + 1000000n);
assert.equal(query(usd, { balance: { address: gameRewards } }).balance, (BigInt(backed.reserve) + BigInt(backed.available.spaceusd) + BigInt(backed.liabilities.spaceusd)).toString());
assert.equal(query(capitalTreasury, { config: {} }).reward_pool, gameRewards);
// Treasury funds both global reward pools directly. Collapsar/capital dispatch
// stays separate from prize funding.
const rewardsBeforeFunding = query(gameRewards, { state: {} });
await execute(srt, { transfer: { recipient: capitalTreasury, amount: '1500000' } });
await execute(usd, { transfer: { recipient: capitalTreasury, amount: '2000000' } });
assert.equal(query(capitalTreasury, { balance: { token: srt } }), '1500000');
await execute(capitalTreasury, { fund_reward_pool: { token: srt, amount: '1000000' } });
await execute(capitalTreasury, { fund_reward_pool: { token: usd, amount: '1000000' } });
const rewardsAfterFunding = query(gameRewards, { state: {} });
const usdBacking = state => BigInt(state.reserve) + BigInt(state.available.spaceusd) + BigInt(state.liabilities.spaceusd);
assert.equal(usdBacking(rewardsAfterFunding), usdBacking(rewardsBeforeFunding) + 1000000n);
assert.equal(BigInt(rewardsAfterFunding.available.srt), BigInt(rewardsBeforeFunding.available.srt) + 1000000n);
assert.equal(query(capitalTreasury, { balance: { token: srt } }), '500000');

// Attach clans before the next cycle: membership and leadership changes are
// pending until the engine activates that cycle, not applied by the website.
await execute(usd, { send: { contract: gameFleet, amount: '94000000', msg:
  Buffer.from(JSON.stringify({ upgrade: { ship: 'racer', target_tier: 100 } })).toString('base64') } });
await execute(usd, { transfer: { recipient: buyer, amount: '2000000' } });
const buyerPurchase = await execute(usd, { send: { contract: gameFleet, amount: '950000', msg:
  Buffer.from(JSON.stringify({ buy: { ship: 'racer', target_tier: 1, sponsor: owner } })).toString('base64') } }, 'racer-buyer');
assert.equal(attribute(buyerPurchase, 'sponsor'), owner);
assert.equal(attribute(buyerPurchase, 'referral'), '47500');
assert.equal(query(gameFleet, { fleet: { wallet: buyer } }).sponsor, owner);
assert.deepEqual(query(gameFleet, { referrals: { sponsor: owner, start_after: null, limit: 20 } }), [buyer]);
const clans = await instantiate(clansCode, { owner, engine: game, fleet: gameFleet, current_cycle: 1 }, 'Cycle-delayed clans');
await wirePause([clans]);
await execute(clans, { create: { name: 'Local pilots', code: 'PILOTS' } });
await execute(clans, { join: { code: 'PILOTS' } }, 'racer-buyer');
assert.equal(query(clans, { member: { wallet: buyer } }).active, null);
await execute(game, { set_clans: { address: clans } });
// Deliberate manual seed, separate from all purchase/fee revenue.
await execute(usd, { send: { contract: gameRewards, amount: '10000000000', msg: fund } });
await advanceGame();
assert.equal(query(clans, { member: { wallet: buyer } }).active, 1);
await execute(clans, { leave: {} }, 'racer-buyer');
assert.equal(query(clans, { member: { wallet: buyer } }).active, 1);
for (let i = 0; query(game, { state: {} }).phase !== 'waiting'; i++) {
  assert.ok(i < 20, 'Clan race snapshot stalled');
  await advanceGame();
}
await execute(game, { seal: { limit: 1 } });
await relay(randomness, game, query(game, { seed_request: { race: 2, tick: 1 } }));
for (let i = 0; query(game, { state: {} }).cycle === 2; i++) {
  assert.ok(i < 20, 'Clan settlement stalled');
  await advanceGame();
}
const clanBudget = BigInt(query(game, { state: {} }).budget_usd);
const premium = clanBudget * 500n / 10000n;
const leaderSettlement = query(game, { settlement: { cycle: 2, wallet: owner } });
const memberSettlement = query(game, { settlement: { cycle: 2, wallet: buyer } });
assert.equal(BigInt(leaderSettlement.gross_usd), (clanBudget - premium) * 100n / 101n);
assert.equal(BigInt(memberSettlement.gross_usd), (clanBudget - premium) / 101n);
const clanBacking = query(gameRewards, { state: {} });
assert.equal(BigInt(query(usd, { balance: { address: gameRewards } }).balance),
  BigInt(clanBacking.reserve) + BigInt(clanBacking.available.spaceusd) + BigInt(clanBacking.liabilities.spaceusd));
await advanceGame(); // Begin cycle three; the queued leave finally activates.
assert.equal(query(clans, { member: { wallet: buyer } }).active, null);

const skinCollection = await instantiate(collectionCode, { owner, minter: owner, name: 'Local ship skins', symbol: 'SKIN' }, 'Ship skin collection');
const skins = await instantiate(skinsCode, { owner, fleet: gameFleet, collection: skinCollection, randomness, milestone_step: 100,
  catalogue: skinCatalogue() }, 'Milestone skins');
await wirePause([skins], [skinCollection]);
for (const address of [gameEquipment, marketplace]) await execute(skinCollection, { extension: { msg: { set_custodian: { address, allowed: true } } } });
await execute(gameEquipment, { configure: { engine: null, paused: null, collection: skinCollection, allowed: true } });
await execute(marketplace, { allow_collection: { collection: skinCollection, allowed: true } });
await execute(marketplace, { configure: { treasury: capitalTreasury, fee_bps: null } });
await execute(skinCollection, { update_ownership: { transfer_ownership: { new_owner: skins, expiry: null } } });
await execute(skins, { accept_minter: {} });
await execute(randomness, { configure: { paused: null, consumer: skins, allowed: true } });
await execute(usd, { send: { contract: gameFleet, amount: '900000000', msg:
  Buffer.from(JSON.stringify({ upgrade: { ship: 'racer', target_tier: 1000 } })).toString('base64') } });
assert.equal(query(skins, { status: { wallet: owner, ship: 'racer' } }).earned, 10);
let peakSkinGas = 0n;
const skinIds = [];
for (let id = 1; id <= 3; id++) {
  await execute(skins, { request: { ship: 'racer' } });
  if (id === 1) {
    await execute(pause, { set_paused: { paused: true } });
    await pausedFailure(() => execute(skins, { fulfill: { id } }));
    await execute(pause, { set_paused: { paused: false } });
  }
  await relay(randomness, skins, id);
  assert.ok(query(skins, { preview: { id } }).ready);
  const result = await execute(skins, { fulfill: { id } });
  peakSkinGas = BigInt(result.gas_used) > peakSkinGas ? BigInt(result.gas_used) : peakSkinGas;
  const claim = query(skins, { claim: { id } });
  skinIds.push(claim.token_id);
  assert.equal(query(skinCollection, { owner_of: { token_id: claim.token_id } }).owner, owner);
}
const codes = skinIds.map(token_id => query(skinCollection, { nft_info: { token_id } }).extension.code);
assert.equal(new Set(codes).size, 3);
await advanceGame(); // End cycle-three boundary so new bind requests can queue.
await execute(skinCollection, { send_nft: { contract: gameEquipment, token_id: skinIds[0],
  msg: Buffer.from(JSON.stringify({ slot: 'racer_skin' })).toString('base64') } });
await execute(skinCollection, { send_nft: { contract: marketplace, token_id: skinIds[1],
  msg: Buffer.from(JSON.stringify({ payment_token: usd, price: '1000' })).toString('base64') } });
const counts = query(skinCollection, { extension: { msg: { skin_counts: { wallet: owner, kind: 'racer_skin', codes } } } });
assert.ok(counts.counts.every(item => item.count === 1));
assert.equal(query(skins, { status: { wallet: owner, ship: 'racer' } }).available, 7);
assert.deepEqual(query(skins, { status: { wallet: owner, ship: 'racer' } }).eligible_codes, []);
await assert.rejects(() => execute(skins, { request: { ship: 'racer' } }), /all available skins owned/);
// Six commander listings preceded this new skin listing.
await execute(usd, { send: { contract: marketplace, amount: '1000',
  msg: Buffer.from(JSON.stringify({ listing_id: 7 })).toString('base64') } }, 'racer-buyer');
assert.deepEqual(query(skins, { status: { wallet: owner, ship: 'racer' } }).eligible_codes, [codes[1]]);
await execute(skins, { request: { ship: 'racer' } });
await relay(randomness, skins, 4);
await execute(skins, { fulfill: { id: 4 } });
const fourth = query(skins, { claim: { id: 4 } });
assert.equal(query(skinCollection, { nft_info: { token_id: fourth.token_id } }).extension.code, codes[1]);
assert.equal(query(skins, { status: { wallet: owner, ship: 'racer' } }).available, 6);
assert.deepEqual(query(skins, { pending: { start_after: null, limit: 20 } }), []);
const deployment = { chain_id: 'spaceship-racer-local', owner, game, randomness, pause, recruitment, skins, skinCollection,
  fleet: gameFleet, equipment: gameEquipment, rewards: gameRewards, clans, payments, capitalTreasury, marketplace, paymentTokens,
  srt, spaceusd: usd, reward_pool: gameRewards, collapsar: treasuryWallet,
  tokens: { clunc, custc, ust1, srt, spaceusd: usd },
  indexer: { host: '127.0.0.1', port: 8787, rpc: 'http://127.0.0.1:27657', lcd: 'http://127.0.0.1:1417' },
  key_name: 'test1', gas_price_uluna: '28.325' };
writeFileSync(new URL('./deployment.json', import.meta.url), `${JSON.stringify(deployment, null, 2)}\n`);
// Exercise the actual keeper executable, including automatic gas simulation,
// a future proof relay and permissionless skin fulfillment.
await execute(usd, { send: { contract: gameFleet, amount: '100000000', msg:
  Buffer.from(JSON.stringify({ buy: { ship: 'tank', target_tier: 100, sponsor: null } })).toString('base64') } });
await execute(skins, { request: { ship: 'tank' } });
const keeperFile = fileURLToPath(new URL('./keeper.mjs', import.meta.url));
const keeperOnce = jobs => JSON.parse(execFileSync(process.execPath, [keeperFile, '--once', ...(jobs ? ['--jobs'] : [])], {
  encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
}));
await execute(pause, { set_paused: { paused: true } });
assert.equal(keeperOnce(true).action, 'globally_paused');
assert.equal(keeperOnce(false).action, 'globally_paused');
await execute(pause, { set_paused: { paused: false } });
let keeperProof;
for (let i = 0; i < 30; i++) {
  keeperProof = keeperOnce(true);
  if (keeperProof.action === 'relay_randomness') break;
  await sleep(500);
}
assert.equal(keeperProof.action, 'relay_randomness');
const keeperSkin = keeperOnce(true);
assert.equal(keeperSkin.action, 'fulfill_skin');
assert.equal(query(skins, { status: { wallet: owner, ship: 'tank' } }).progress.fulfilled, 1);
const keeperRace = keeperOnce(false);
assert.ok(['advance', 'launch_race', 'process_race_tick', 'settle_cycle_and_launch', 'commit_next_multiplier', 'reveal_next_multiplier'].includes(keeperRace.action), `Unexpected keeper action: ${keeperRace.action}`);
// Smoke assertions use one-tick races. Never leave those fixtures as the
// interactive deployment: restore real durations and 100-race cycles.
execFileSync(process.execPath, [fileURLToPath(new URL('./play.mjs', import.meta.url))], { stdio: 'inherit' });
const artifactBytes = Object.fromEntries(['racer_race', 'racer_rewards', 'racer_fleet', 'racer_equipment', 'racer_collection', 'racer_randomness', 'racer_marketplace', 'racer_recruitment', 'racer_pause', 'racer_payments', 'racer_treasury', 'racer_clans', 'racer_skins']
  .map(name => [name, statSync(fileURLToPath(new URL(`../target/wasm32-unknown-unknown/release/${name}.wasm`, import.meta.url))).size]));
console.log(JSON.stringify({ chain: 'spaceship-racer-local', owner, usd, srt, rewards, fleet, collection, equipment,
  randomness, game, gameFleet, gameEquipment, gameRewards, proofGasUsed: proofTx.gas_used,
  marketplace, recruitment, pause, buyer, treasuryWallet, paymentTokens,
  dexFactory, dexRouter, payments, capitalTreasury, peakPaymentGas: peakPaymentGas.toString(), treasuryConversionGas: convertTx.gas_used,
  clans,
  skins, skinCollection, peakSkinGas: peakSkinGas.toString(),
  keeperProof, keeperSkin, keeperRace,
  peakAdvanceGas: peakAdvanceGas.toString(), tenPullGas: recruitTx.gas_used, fusionGas: fusionTx.gas_used, artifactBytes,
  result: 'PASS: compiled Wasm, live proofs, race/assets/pause, CL8Y/clans, milestone skins, custody-aware duplicate avoidance and executable local keeper' }, null, 2));
