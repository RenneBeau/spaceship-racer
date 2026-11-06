import test from 'node:test';
import assert from 'node:assert/strict';
import { buildLeaderboards, fleetHealth } from '../../src/onchain/leaderboards.mjs';
import { Store } from '../../src/onchain/store.mjs';
import { ClanRewardHistory } from '../../src/onchain/clan-rewards.mjs';

const player = (wallet, distance, alive = true) => ({ wallet, race: 5, distance, alive, temporary: [], tiers: [1, 1, 1], commander: null,
  ships: Array.from({ length: 3 }, () => ({ hp: alive ? '1000' : '0', base_hp: '1000', shield: '0' })) });
test('race and cycle standings rank racers, clans, and named solo competitors independently', () => {
  const rows = [
    { wallet: 'a', player: player('a', '3000'), score: { score: '10000', clan: 7 } },
    { wallet: 'b', nickname: 'Nova', player: player('b', '6000'), score: { score: '30000', clan: null } },
    { wallet: 'c', player: player('c', '9000', false), score: { score: '40000', clan: 7 } },
  ];
  const board = buildLeaderboards({ race: 5, cycle: 3 }, rows, [{ id: 7, name: 'Seven', code: 'SEVEN', disband_at: null }]);
  assert.deepEqual(board.race.racers.map(r => r.wallet), ['b', 'a', 'c']);
  assert.deepEqual(board.cycle.racers.map(r => r.wallet), ['c', 'b', 'a']);
  assert.equal(board.race.teams[0].solo, true); assert.equal(board.race.teams[0].name, 'Nova');
  assert.equal(board.cycle.teams[0].clanId, 7); assert.equal(board.cycle.teams[0].points, '50000');
  assert.equal(board.race.racers[2].distance, '0'); assert.equal(board.race.racers[2].health.hp, '0');
});
test('fleet health keeps exact HP and derives the initial tank shield with commander bonuses', () => {
  const p = player('a', '0'); p.tiers = [110, 10, 310]; p.commander = { bonus: 'shield', bonus_bps: 800 };
  p.ships[1].shield = '5000000'; const result = fleetHealth(p);
  assert.equal(result.hp, '3000'); assert.equal(result.maxHp, '3000');
  assert.equal(result.maxCombatShield, '10848600'); assert.equal(result.combatShield, '5000000');
});
test('combat impacts are indexed atomically, cursor-addressable, and removed by fork rewind', () => {
  const store = new Store(':memory:', 'combat');
  try {
    const hit = { race: 1, tick: 2, attacker: 'pilot', target: 'victim', hp_before: '1000', hp_after: '0', hp_lost: '1000', destroyed: true };
    store.commit({ height: 1, hash: 'A', time: '2026-11-01T00:00:00Z' }, [{ hash: 'TX', index: 0, participants: ['pilot'], events: [
      { index: 0, contract: 'game', action: 'advance_race', attributes: [{ key: 'rocket_hit', value: JSON.stringify(hit) }] },
    ] }]);
    const rows = store.rocketHits(['game'], 'pilot'); assert.equal(rows.length, 1); assert.equal(rows[0].target, 'victim'); assert.equal(rows[0].hp_lost, '1000');
    assert.deepEqual(store.rocketHits(['game'], 'pilot', rows[0].id), []); assert.equal(store.rocketHits(['game'], 'victim').length, 1);
    store.rewind(0); assert.deepEqual(store.rocketHits(['game'], 'pilot'), []);
  } finally { store.close(); }
});
test('clan history uses historical premium rules and net settlements, without counting referrals twice', async () => {
  const store = new Store(':memory:', 'clan-rewards');
  const attrs = values => Object.entries(values).map(([key, value]) => ({ key, value }));
  try {
    store.commit({ height: 1, hash: 'B1', time: '2026-11-01T00:00:00Z' }, [{ hash: 'START', index: 0, participants: [], events: [{ index: 0, contract: 'clans', action: 'begin_clan_cycle', attributes: attrs({ cycle: '1' }) }] }]);
    store.commit({ height: 2, hash: 'B2', time: '2026-11-01T00:00:06Z' }, [{ hash: 'END', index: 0, participants: [], events: [{ index: 0, contract: 'game', action: 'advance_race', attributes: attrs({ settled_cycle: '1' }) }] }]);
    const scores = { a: { clan: 7, score: '60000', tier_sum: '100' }, b: { clan: 7, score: '0', tier_sum: '100' }, solo: { clan: null, score: '40000', tier_sum: '100' }, later: { clan: null, score: '0', tier_sum: '0' } };
    const settlement = { a: { gross_usd: '285', maintenance: '10', sponsor_usd: '5', srt: '60' }, b: { gross_usd: '285', maintenance: '20', sponsor_usd: '0', srt: '0' }, solo: { gross_usd: '400', maintenance: '5', sponsor_usd: '0', srt: '40' } };
    let requests = 0;
    const chain = { pinnedTip: async () => ({ height: 3 }), rpc: async () => ({ block_id: { hash: 'B2' } }), smart: async (contract, message, height) => {
      requests++;
      if (contract === 'fleet') return message.players.start_after ? [] : ['a', 'b', 'solo', 'later'];
      if (contract === 'clans') return { engine: 'game' };
      if (message.state) return height === 1 ? { leader_bps: 500 } : { total_score: '0', leader_bps: 9999, budget_usd: '1000', budget_srt: '100' };
      if (message.cycle_player) return scores[message.cycle_player.wallet];
      if (message.settlement) return settlement[message.settlement.wallet];
      throw new Error('Unexpected query');
    } };
    const history = new ClanRewardHistory(chain, store, { contracts: { fleet: 'fleet', game: 'game', clans: 'clans' } });
    await history.tick(); assert.equal(history.lastError, null);
    const result = store.clanRewardHistory(7);
    assert.deepEqual(result.totals, { spaceusd: '570', srt: '60', premium: '30', gross: '570', maintenance: '30' });
    const before = requests; await history.tick(); assert.equal(requests, before); // Immutable cycles are not recomputed.
    store.rewind(0); assert.equal(store.clanRewardHistory(7).cycles.length, 0);
  } finally { store.close(); }
});
