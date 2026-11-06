import test from 'node:test';
import assert from 'node:assert/strict';
import { assessGameHealth } from '../../src/onchain/health.mjs';
test('fresh chain/indexer cannot hide exhausted keeper gas or stalled race processing', () => {
  const now = Date.parse('2026-11-04T12:00:00Z');
  const health = assessGameHealth({ race: 334, phase: 'waiting' }, { players: 3, keeperBalance: '16411817', lastProgressTime: '2026-11-04T11:00:00Z', now });
  assert.deepEqual(health.gameReasons, ['keeper_low_gas', 'game_not_advancing']);
  assert.equal(assessGameHealth({ race: 334, phase: 'waiting' }, { players: 3, keeperBalance: '100000000000', lastProgressTime: '2026-11-04T11:59:55Z', now }).gameHealthy, true);
});
test('paused, pre-launch and empty games are not classified as stalled', () => {
  const state = { race: 0, phase: 'boundary' }, now = Date.parse('2026-11-04T12:00:00Z'), lastProgressTime = '2026-11-03T12:00:00Z';
  for (const options of [{ players: 0 }, { players: 10, paused: true }, { players: 10, launchTime: now / 1000 + 86400 }]) assert.equal(assessGameHealth(state, { ...options, lastProgressTime, now }).gameHealthy, true);
});
