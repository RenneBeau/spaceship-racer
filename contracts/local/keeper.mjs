// Local-only bounded keeper. Uses isolated Docker test keys, never mainnet keys.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { launchSteps, planRace, raceStops, sealBatchLimit, skinCanFulfill, staleWork } from './keeper-plan.mjs';
import { CosmWasmClient, SigningCosmWasmClient } from '@cosmjs/cosmwasm-stargate';

const compose = process.env.RACER_COMPOSE ?? fileURLToPath(new URL('./compose.yml', import.meta.url));
const deployment = JSON.parse(readFileSync(process.env.RACER_MANIFEST ?? new URL('./deployment.json', import.meta.url), 'utf8'));
assert.ok(['spaceship-racer-local', 'spaceship-racer-alpha'].includes(deployment.chain_id), 'Refusing non-testnet manifest');
const reader = await CosmWasmClient.connect(deployment.indexer?.rpc ?? 'http://127.0.0.1:27657');
assert.equal(await reader.getChainId(), deployment.chain_id, 'Refusing non-local RPC');
const signingKey = process.env.RACER_KEEPER_KEY ?? deployment.keeper_key ?? deployment.key_name ?? 'test1';
const signingAccount = cli(['keys', 'show', signingKey, '--keyring-backend', 'test', '--output', 'json']);
const signingAddress = signingAccount.address;
const feeDenom = deployment.keeper_fee_denom ?? 'uluna';
const gasPrice = deployment.keeper_gas_price ?? deployment.gas_price_uluna ?? '28.325';
// Gas estimation uses only the public key. Actual signing stays in Docker's
// isolated test keyring, never in the indexer or a browser bundle.
const simulator = await SigningCosmWasmClient.connectWithSigner(deployment.indexer?.rpc ?? 'http://127.0.0.1:27657', {
  getAccounts: async () => [{ address: signingAddress, algo: 'secp256k1', pubkey: Buffer.from(JSON.parse(signingAccount.pubkey).key, 'base64') }],
  signDirect: async () => { throw new Error('Simulation client cannot sign'); },
});
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let fundingWait = null;
function cli(args, input) {
  return JSON.parse(execFileSync('docker', ['compose', '-f', compose, 'exec', '-T', 'chain', 'terrad', ...args], {
    encoding: 'utf8', input: input ? JSON.stringify(input) : undefined, stdio: [input ? 'pipe' : 'ignore', 'pipe', 'pipe'],
  }));
}
async function executeBatch(instructions, action, futureBlock = false, gasCap = null) {
  const current = status(); if (current.stale) return { action: 'waiting_for_fresh_blocks', blockAgeSeconds: current.blockAgeSeconds };
  let gas = 1000000 + instructions.length * 1000000;
  if (!futureBlock) {
    try {
      const used = await simulator.simulate(signingAddress, instructions.map(({ contract, msg }) => ({ typeUrl: '/cosmwasm.wasm.v1.MsgExecuteContract',
        value: { sender: signingAddress, contract, msg: Buffer.from(JSON.stringify(msg)), funds: [] } })), 'Local race tick');
      gas = Math.ceil(used * 1.3);
    } catch (error) {
      if (staleWork(error.message)) return { action: 'stale_work' };
      // An inclusion in the next block can be due before simulation at the current
      // committed timestamp. Keep the bounded fallback only for that case.
      if (!error.message.includes('beacon not due')) throw error;
    }
  }
  if (gasCap !== null && gas > gasCap) throw Object.assign(new Error('bounded batch gas cap'), { gasCap: true });
  const [whole, fraction = ''] = gasPrice.split('.');
  const scale = 10n ** BigInt(fraction.length);
  const fee = (BigInt(gas) * BigInt(whole + fraction) + scale - 1n) / scale;
  const balance = BigInt((await reader.getBalance(signingAddress, feeDenom)).amount);
  if (balance < fee) {
    fundingWait = { until: Date.now() + 30000, action: 'waiting_for_gas_funds', available: balance.toString(), required: fee.toString(), denom: feeDenom };
    return fundingWait;
  }
  const transaction = { body: { messages: instructions.map(({ contract, msg }) => ({
    '@type': '/cosmwasm.wasm.v1.MsgExecuteContract', sender: signingAddress, contract,
    msg, funds: [],
  })), memo: 'Local race tick', timeout_height: '0', extension_options: [], non_critical_extension_options: [] },
  auth_info: { signer_infos: [], fee: { amount: [{ denom: feeDenom, amount: fee.toString() }], gas_limit: String(gas), payer: '', granter: '' } }, signatures: [] };
  const signed = cli(['tx', 'sign', '/dev/stdin', '--from', signingKey, '--keyring-backend', 'test', '--chain-id', deployment.chain_id, '--output', 'json'], transaction);
  const submitted = cli(['tx', 'broadcast', '/dev/stdin', '--broadcast-mode', 'sync', '--output', 'json'], signed);
  assert.equal(Number(submitted.code ?? 0), 0, submitted.raw_log);
  for (let i = 0; i < 120; i++) {
    await sleep(500);
    let result; try { result = cli(['query', 'tx', submitted.txhash, '--output', 'json']); } catch { continue; }
    if (Number(result.code ?? 0) && staleWork(result.raw_log ?? '')) return { action: 'stale_work', txhash: submitted.txhash };
    assert.equal(Number(result.code ?? 0), 0, result.raw_log);
    return { action, txhash: submitted.txhash, gas_used: result.gas_used, messages: instructions.length };
  }
  throw new Error(`Confirmation timeout: ${submitted.txhash}`);
}
function status() {
  const status = cli(['status']);
  assert.equal((status.node_info ?? status.NodeInfo).network, deployment.chain_id, 'Refusing non-local chain');
  const sync = status.sync_info ?? status.SyncInfo;
  const height = Number(sync.latest_block_height);
  const time = Math.floor(Date.parse(sync.latest_block_time) / 1000);
  assert.ok(Number.isSafeInteger(height) && height > 0 && Number.isSafeInteger(time), 'Invalid local chain status');
  const blockAgeSeconds = Math.max(0, Date.now() / 1000 - time);
  return { height, time, blockAgeSeconds, stale: blockAgeSeconds > 30 };
}
const query = (contract, msg) => reader.queryContractSmart(contract, msg);
async function execute(contract, msg, action) {
  return executeBatch([{ contract, msg }], action);
}
const beacons = new Map();
const beaconRetry = new Map();
async function beaconProof(request, chainTime) {
  if (request.randomness) return { ready: true, seed: request.randomness };
  if (chainTime < request.available_at) return { ready: false };
  if (Date.now() / 1000 < request.available_at || Date.now() < (beaconRetry.get(request.round) ?? 0)) return { ready: false };
  let beacon = beacons.get(request.round);
  if (!beacon) {
    const controller = new AbortController();
    try {
      beacon = await Promise.any(['api.drand.sh', 'api2.drand.sh', 'api3.drand.sh'].map(async host => {
        const response = await fetch(`https://${host}/52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971/public/${request.round}`,
          { signal: AbortSignal.any([controller.signal, AbortSignal.timeout(2500)]) });
        if (!response.ok) throw new Error('Beacon not available');
        const value = await response.json();
        if (value.round !== request.round || !/^[0-9a-f]{96}$/i.test(value.signature)) throw new Error('Invalid beacon response');
        return value;
      }));
    } catch { beaconRetry.set(request.round, Date.now() + 1000); return { ready: false }; }
    finally { controller.abort(); }
    assert.equal(beacon.round, request.round, 'Wrong drand round');
    assert.match(beacon.signature, /^[0-9a-f]{96}$/i, 'Invalid quicknet signature');
    beacons.set(request.round, beacon);
    if (beacons.size > 64) beacons.delete(beacons.keys().next().value);
  }
  return { ready: true, signature: Buffer.from(beacon.signature, 'hex').toString('base64'), seed: Buffer.from(beacon.randomness, 'hex').toString('base64') };
}
async function randomness(consumer, id, adapter, chainTime, defer = false) {
  assert.ok(Number.isSafeInteger(id) && id > 0, 'Unsafe request ID');
  const request = await query(adapter, { request: { consumer, id } });
  const proof = await beaconProof(request, chainTime);
  if (!proof.ready || request.randomness) return proof;
  const instruction = { contract: adapter, msg: { submit: { consumer, id, signature: proof.signature } } };
  if (defer) return { ...proof, instruction };
  return { ready: false, transaction: await execute(adapter, instruction.msg, 'relay_randomness') };
}

async function racePass(chain) {
  const config = await query(deployment.game, { config: {} });
  if (config.paused) return { action: 'locally_paused' };
  for (const address of [config.fleet, config.equipment, config.rewards, config.randomness]) {
    const current = await query(address, address === config.rewards ? { state: {} } : { config: {} });
    if (current.paused) return { action: 'locally_paused' };
  }
  const state = await query(deployment.game, { state: {} });
  const fleetConfig = await query(config.fleet, { config: {} });
  if (fleetConfig.presale && (fleetConfig.presale.starts_at === null || chain.time < fleetConfig.presale.starts_at + fleetConfig.presale.duration_seconds)) return { action: 'waiting_for_presale_launch', presale: fleetConfig.presale };
  if (state.phase === 'boundary' && fleetConfig.players === 0) return { action: 'waiting_for_first_fleet' };
  if (state.phase === 'restore') return { action: 'restoration_in_progress' };
  const fast = Number(config.timing_version ?? 1) >= 2;
  const prefix = [];
  let nextReady = !fast;
  const prepare = () => ({ contract: deployment.game, msg: { prepare_next_multiplier: {} } });
  if (fast) {
    const commitment = await query(deployment.game, { multiplier: { race: state.race + 1 } });
    if (commitment.request_id === null) return execute(deployment.game, prepare().msg, 'commit_next_multiplier');
    nextReady = commitment.multiplier !== null;
    if (!nextReady) {
      const proof = await randomness(deployment.game, commitment.request_id, config.randomness,
        (config.accounting_version ?? 0) > 0 ? chain.time : chain.nextTime, true);
      nextReady = proof.ready;
      if (proof.instruction) prefix.push(proof.instruction);
    }
  }
  const advance = () => ({ contract: deployment.game, msg: { advance: { limit: 20 } } });
  if ((config.accounting_version ?? 0) >= 3 && ['snapshot', 'movement', 'combat', 'accrue'].includes(state.phase)) {
    for (const steps of [8, 4, 2, 1]) {
      try {
        return await executeBatch([...prefix, { contract: deployment.game, msg: { advance_batch: { limit: 20, steps } } }], `batch_${state.phase}`, false, 80000000);
      } catch (error) { if ((!error.gasCap && !error.message.includes('out of gas')) || steps === 1) throw error; }
    }
  }
  async function launch(phase = 'boundary') {
    if (process.argv.includes('--stop-at-boundary')) return [];
    if (!nextReady) return [];
    const fleet = await query(config.fleet, { config: {} });
    if (fleet.players < 1 || fleet.players >= 20) return [];
    const pending = await query(config.equipment, { pending_wallets: { start_after: null, limit: 20 } });
    if (pending.length >= 20) return [];
    return Array.from({ length: launchSteps(phase, fleet.players, pending.length) }, advance);
  }
  async function endCycle() {
    if ((config.accounting_version ?? 0) >= 3) return [];
    if (state.participants >= 20) return [];
    if (config.clans && (await query(config.clans, { clans: { start_after: null, limit: 20 } })).length >= 20) return [];
    return Array.from({ length: config.clans ? 4 : 3 }, advance);
  }
  if (['boundary', 'drain_equipment'].includes(state.phase)) {
    const steps = await launch(state.phase);
    if (steps.length) return executeBatch([...prefix, ...steps, ...(fast ? [prepare()] : [])], 'launch_race');
    if (fast && !nextReady) return { action: 'waiting_for_next_multiplier' };
  }
  if (state.phase === 'checkpoint') {
    const steps = await endCycle(), next = steps.length ? await launch() : [];
    if (steps.length) return executeBatch([...prefix, ...steps, ...next, ...(fast && next.length ? [prepare()] : [])], 'settle_cycle_and_launch');
  }
  const plan = planRace(state, chain.height);
  if (plan === 'wait') {
    if (state.tick === 1 && state.sealed === 0 && chain.height === state.start_height) {
      // This transaction can only enter height start+1 or later, when the first
      // action window is closed. Avoid waiting for yet another block to submit.
      return executeBatch([...prefix, { contract: deployment.game, msg: { seal: { limit: 1 } } }], 'seal_first_tick', true);
    }
    if (prefix.length) return executeBatch(prefix, 'reveal_next_multiplier');
    return { action: 'waiting_for_block' };
  }
  if (plan === 'seal') {
    const msg = { seal: { limit: sealBatchLimit(state, chain.height, config.parameters.duration_cdf.length) } };
    if (prefix.length) return executeBatch([...prefix, { contract: deployment.game, msg }], 'seal');
    return execute(deployment.game, msg, 'seal');
  }
  if (plan === 'randomness') {
    if (state.inline_oracle) {
      if (state.replay_until) return executeBatch([...prefix, advance()], 'advance_replay');
      const beacon = await query(deployment.game, { tick_beacon: { race: state.race, tick: state.tick } });
      assert.ok(beacon, 'Sealed inline tick has no beacon');
      const proof = await beaconProof(beacon, chain.time);
      if (!proof.ready) {
        if (prefix.length) return executeBatch(prefix, 'reveal_next_multiplier');
        return { action: 'waiting_for_beacon' };
      }
      return executeBatch([...prefix, { contract: deployment.game, msg: { check_tick: {
        race: state.race, tick: state.tick, signature: proof.signature ?? null,
        seal_limit: sealBatchLimit(state, chain.height, config.parameters.duration_cdf.length),
      } } }], 'check_and_seal_race_tick');
    }
    const id = await query(deployment.game, { seed_request: { race: state.race, tick: state.tick } });
    assert.ok(id !== null, 'Sealed tick has no request');
    const lazyAccounting = (config.accounting_version ?? 0) > 0;
    const batch = lazyAccounting || state.participants <= 20;
    // Lazy combat can contain large salvos. Use a committed timestamp for exact
    // simulation instead of charging an oversized guessed future-block limit.
    const cachedSeed = (config.accounting_version ?? 0) >= 3 ? await query(deployment.game, { tick_seed: { race: state.race, tick: state.tick } }) : null;
    const proof = cachedSeed ? { ready: true, seed: cachedSeed } : await randomness(deployment.game, id, config.randomness, lazyAccounting ? chain.time : chain.nextTime, batch);
    if (proof.transaction) return proof.transaction;
    if (!proof.ready) {
      if (prefix.length) return executeBatch(prefix, 'reveal_next_multiplier');
      return { action: 'waiting_for_beacon' };
    }
    if (batch) {
      // Proof + seed consumption + movement + combat commit atomically. A race
      // end occurs in the last message, so never advance into the next race.
      const closed = Math.min(chain.height - state.start_height, config.parameters.duration_cdf.length);
      const instructions = [...prefix, ...(closed > state.sealed ? [{ contract: deployment.game, msg: { seal: { limit: sealBatchLimit(state, chain.height, config.parameters.duration_cdf.length) } } }] : [])];
      let currentProof = proof;
      for (let tick = state.tick; tick <= Math.min(state.tick + (lazyAccounting ? 0 : 1), state.sealed); tick++) {
        if (tick !== state.tick) {
          const requestId = await query(deployment.game, { seed_request: { race: state.race, tick } });
          currentProof = await randomness(deployment.game, requestId, config.randomness, chain.nextTime, true);
          if (!currentProof.ready) break;
        }
        if (currentProof.instruction) instructions.push(currentProof.instruction);
        instructions.push(...Array.from({ length: (config.accounting_version ?? 0) >= 2 ? 1 : 3 }, () => ({ contract: deployment.game, msg: { advance: { limit: 20 } } })));
        if (raceStops(deployment.chain_id, deployment.game, state.race, tick, currentProof.seed, config.parameters.duration_cdf)) {
          if (lazyAccounting) break; // Combat may span pages; observe its confirmed phase before settlement/launch.
          const settlement = state.completed + 1 === config.parameters.races_per_cycle ? await endCycle() : [];
          if (state.completed + 1 !== config.parameters.races_per_cycle || settlement.length) {
            const next = await launch();
            instructions.push(...settlement, ...next, ...(fast && next.length ? [prepare()] : []));
          }
          break;
        }
      }
      return executeBatch(instructions, 'process_race_tick');
    }
  }
  if (prefix.length) return executeBatch([...prefix, advance()], 'advance');
  return execute(deployment.game, { advance: { limit: 20 } }, 'advance');
}

const cursors = new Map();
async function jobPass(chain) {
  for (const [name, address] of [['recruitment', deployment.recruitment], ['skins', deployment.skins]]) {
    if (!address) continue;
    const config = await query(address, { config: {} });
    const controls = await query(config.collection, { extension: { msg: { controls: {} } } });
    const oracle = await query(config.randomness, { config: {} });
    if (controls.paused || oracle.paused) continue;
    const ids = await query(address, { pending: { start_after: cursors.get(address) ?? null, limit: 20 } });
    cursors.set(address, ids.length === 20 ? ids.at(-1) : null);
    for (const id of ids) {
      const proof = await randomness(address, id, config.randomness, chain.nextTime, true);
      if (!proof.ready) continue;
      if (name === 'skins' && !proof.instruction && !skinCanFulfill(await query(address, { preview: { id } }))) continue;
      if (proof.instruction && name === 'skins') return execute(config.randomness, proof.instruction.msg, 'relay_randomness');
      if (proof.instruction) return executeBatch([proof.instruction, { contract: address, msg: { fulfill: { id } } }], 'fulfill_commander');
      return execute(address, { fulfill: { id } }, name === 'skins' ? 'fulfill_skin' : 'fulfill_commander');
    }
  }
  if (deployment.reinvestment) {
    const wallets = await query(deployment.reinvestment, { auto_ready: { start_after: null, limit: 5 } });
    if (wallets.length) return executeBatch(wallets.map(wallet => ({ contract: deployment.reinvestment, msg: { process_auto: { wallet } } })), 'reinvest_cycle_rewards');
  }
  return { action: 'no_ready_jobs' };
}

async function pass(jobs) {
  if (fundingWait && Date.now() < fundingWait.until) return fundingWait;
  fundingWait = null;
  const chain = status();
  if (chain.stale) return { action: 'waiting_for_fresh_blocks', blockAgeSeconds: chain.blockAgeSeconds };
  // Existing future-round signatures can be included in the next block once
  // its conservative expected timestamp reaches the request deadline. Waiting
  // for a further committed block needlessly adds another six seconds.
  const prior = await reader.getBlock(Math.max(1, chain.height - 1));
  const interval = Math.max(0, Math.min(6, chain.time - Date.parse(prior.header.time) / 1000) - 0.2);
  chain.nextTime = chain.time + interval;
  if ((await query(deployment.pause, { state: {} })).paused) return { action: 'globally_paused' };
  return jobs ? jobPass(chain) : racePass(chain);
}

if (process.argv.includes('--once')) {
  console.log(JSON.stringify(await pass(process.argv.includes('--jobs')), null, 2));
  reader.disconnect();
  simulator.disconnect();
} else {
  let stopping = false;
  process.on('SIGINT', () => { stopping = true; });
  process.on('SIGTERM', () => { stopping = true; });
  let nextJobs = 0;
  while (!stopping) {
    // Cheap RPC reads let recruitment share the next block instead of waiting
    // through a separate 30-second job timer.
    // Unexpected broadcast failures terminate rather than spending fees forever.
    const jobs = Date.now() >= nextJobs;
    if (jobs) nextJobs = Date.now() + 3000;
    console.log(JSON.stringify(await pass(jobs)));
    if (!stopping) await sleep(250);
  }
  reader.disconnect();
  simulator.disconnect();
}
