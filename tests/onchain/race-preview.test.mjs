import test from 'node:test';
import assert from 'node:assert/strict';
import { floorSum, replayRace, createRacePreview } from '../../src/onchain/race-preview.mjs';
import { Store } from '../../src/onchain/store.mjs';
const params = { tick_seconds: 6, acceleration: '100', speed_boost: '10000', temporary_distance: '2000000', temporary_ticks: 50,
  boost_price: '1000000', rocket_damage_bps: 10000 };
const player = wallet => ({ race: 1, wallet, tiers: [1, 0, 1], commander: null, safe_speed: '120000', distance: '0', temporary: [],
  acceleration_count: 0, alive: true, salvo: 0, utility_spent: '0', materialized_tick: 0,
  ships: [{ base_hp: '55000', hp: '55000', shield: '0', regen: '500', speed: '100000', attack: '0' },
    { base_hp: '0', hp: '0', shield: '0', regen: '0', speed: '0', attack: '0' },
    { base_hp: '550000', hp: '50000', shield: '100000', regen: '5000', speed: '20000', attack: '500000' }] });

test('closed-form movement preserves every tick floor and never heals an injured fleet', () => {
  for (let seed = 1n; seed <= 100n; seed++) {
    const n = seed * 3n, m = seed * 71n + 1n, a = seed * 19n, b = seed * 37n;
    let brute = 0n; for (let i = 0n; i < n; i++) brute += (a * i + b) / m;
    assert.equal(floorSum(n, m, a, b), brute);
  }
  const result = replayRace({ chainId: 'test', game: 'game', race: 1, snapshots: [[1, player('a')]], actions: [], seeds: new Map(), throughTick: 240, parameters: params });
  assert.equal(result.players.get('a').ships[2].hp, '50000');
  assert.equal(result.players.get('a').ships[1].hp, '0');
});

test('preview uses indexed actions and verified seeds, and rewinds forked actions', async () => {
  const store = new Store(':memory:', 'preview-test');
  const commit = (height, hash, kind = null) => store.commit({ height, hash, time: '2026-11-02T00:00:00Z' }, kind ? [{
    hash, index: 0, participants: ['a'], events: [{ index: 0, contract: 'game', action: 'queue_block_boost', attributes:
      Object.entries({ race: '1', tick: '2', wallet: 'a', kind, count: kind === 'rocket' ? '1' : '1000' }).map(([key, value]) => ({ key, value })) }],
  }] : []);
  commit(10, 'start'); commit(11, 'old-action', 'rocket'); commit(12, 'old-head');
  const seed = Buffer.alloc(32, 7).toString('base64');
  const chain = { config: { chainId: 'test' }, smart: async (_contract, msg, height) => {
    if (msg.players) { assert.equal(height, 10); return [[1, player('a')], [2, player('b')]]; }
    if (msg.seed_request) return 2;
    if (msg.request) return { randomness: seed };
    throw Error('Unexpected preview read');
  } };
  const previews = createRacePreview(chain, store, 'game'), state = { race: 1, start_height: 10, tick: 3, replay_until: 0, phase: 'waiting', participants: 2 };
  const configuration = { accounting_version: 2, randomness: 'oracle', parameters: params };
  const snapshot = () => ({ at: { height: 12 }, smart: (c, q) => chain.smart(c, q, 12) });
  const first = await previews.get(snapshot(), state, configuration);
  assert.equal(first.hits.length, 1); assert.equal(first.hits[0].target, 'b'); assert.equal(first.preview, true);
  store.rewind(10); commit(11, 'new-action', 'speed'); commit(12, 'new-head');
  const second = await previews.get(snapshot(), state, configuration);
  assert.equal(second.hits.length, 0);
  assert.ok(BigInt(second.players.get('a').safe_speed) > BigInt(first.players.get('a').safe_speed));
  assert.equal(await previews.get(snapshot(), state, { accounting_version: 1 }), null);
  const cachedChain = { ...chain, smart: async (contract, msg, height) => {
    if (msg.tick_seed) return seed;
    if (msg.request || msg.seed_request) throw Error('v3 preview must use the authenticated game cache');
    return chain.smart(contract, msg, height);
  } };
  store.rewind(10); commit(11, 'cached-action', 'rocket'); commit(12, 'cached-head');
  const cachedPreview = createRacePreview(cachedChain, store, 'game');
  const cachedSnapshot = { at: { height: 12 }, smart: (c, q) => cachedChain.smart(c, q, 12) };
  assert.equal((await cachedPreview.get(cachedSnapshot, state, { ...configuration, accounting_version: 3 })).hits.length, 1);
  store.close();
});

test('rollback snapshot fallback accepts only an untouched next-block opening state', async () => {
  const store = new Store(':memory:', 'recovered-preview');
  store.commit({ height: 12, hash: 'head', time: '2026-11-03T00:00:00Z' }, []);
  const state = { race: 1, start_height: 10, tick: 1, replay_until: 0, phase: 'waiting', participants: 1 };
  let next = { ...state };
  const chain = { config: { chainId: 'test' }, smart: async (_c, q, h) => {
    if (h === 10) throw Error('codespace undefined code 111222: panic');
    assert.equal(h, 11);
    return q.state ? next : [[1, player('a')]];
  } };
  const snapshot = { at: { height: 12 } }, config = { accounting_version: 3, parameters: params };
  try {
    assert.equal((await createRacePreview(chain, store, 'game').get(snapshot, state, config)).players.size, 1);
    next = { ...state, tick: 2 };
    await assert.rejects(createRacePreview(chain, store, 'game').get(snapshot, state, config), /111222/);
  } finally { store.close(); }
});
