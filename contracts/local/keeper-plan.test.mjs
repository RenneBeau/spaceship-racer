import test from 'node:test';
import assert from 'node:assert/strict';
import { launchSteps, planRace, planRewardFunding, raceStops, sealBatchLimit, skinCanFulfill, staleWork } from './keeper-plan.mjs';

test('only closed block windows are sealed', () => {
  const state = { phase: 'waiting', start_height: 100, tick: 1, sealed: 0 };
  assert.equal(planRace(state, 100), 'wait');
  assert.equal(planRace(state, 101), 'seal');
});
test('seal gas work cannot expand when another block closes before inclusion', () => {
  const state = { start_height: 100, sealed: 0 };
  const limit = sealBatchLimit(state, 101, 242);
  assert.equal(limit, 1);
  assert.equal(Math.min(2, state.sealed + limit), 1);
  assert.equal(sealBatchLimit(state, 10000, 242), 20);
  assert.equal(sealBatchLimit({ start_height: 100, sealed: 242 }, 10000, 242), 0);
});
test('small-fleet launch batching includes equipment work and stops before an unsealed tick', () => {
  assert.equal(launchSteps('boundary', 3, 0), 3);
  assert.equal(launchSteps('boundary', 3, 2), 4);
  assert.equal(launchSteps('drain_equipment', 3, 0), 2);
  assert.equal(launchSteps('boundary', 20, 0), 0);
  assert.equal(launchSteps('boundary', 3, 20), 0);
  assert.equal(launchSteps('boundary', 0, 0), 0);
});

test('an already sealed current tick takes priority over a growing backlog', () => {
  assert.equal(planRace({ phase: 'waiting', start_height: 100, tick: 3, sealed: 20 }, 10000), 'randomness');
  assert.equal(planRace({ phase: 'combat' }, 10000), 'advance');
  assert.throws(() => planRace({ phase: 'unknown-version' }, 10000), /Unknown race phase/);
});

test('blocked or completed skin jobs are never scheduled for paid fulfillment', () => {
  assert.equal(skinCanFulfill('waiting_for_randomness'), false);
  assert.equal(skinCanFulfill({ duplicate_owned: { code: 'racer1' } }), false);
  assert.equal(skinCanFulfill({ fulfilled: { token_id: 'skin:1' } }), false);
  assert.equal(skinCanFulfill({ ready: { code: 'racer1' } }), true);
});

test('treasury funding keeps the configured reserve and needs a reward pool', () => {
  assert.deepEqual(planRewardFunding({ SRT: '1500000', SpaceUSD: '2500000' }, 'rewards', '500000'),
    [{ token: 'SRT', amount: '1000000' }, { token: 'SpaceUSD', amount: '2000000' }]);
  assert.deepEqual(planRewardFunding({ SRT: '100' }, 'rewards', '500'), []);
  assert.deepEqual(planRewardFunding({ SRT: '100' }, null), []);
  assert.throws(() => planRewardFunding({ SRT: '-1' }, 'rewards'), /Invalid treasury balance/);
  assert.throws(() => planRewardFunding({ SRT: '1' }, 'rewards', '-1'), /negative/);
});

test('normal permissionless races retry but authorization and invalid proofs fail', () => {
  assert.equal(staleWork('Generic error: job already fulfilled: execute wasm contract failed'), true);
  assert.equal(staleWork('Generic error: selected skin currently owned; same claim must wait'), true);
  assert.equal(staleWork('Generic error: unauthorized'), false);
  assert.equal(staleWork('Generic error: invalid beacon proof'), false);
  assert.equal(staleWork('already fulfilled appears in unrelated text'), false);
});
test('catch-up stops only at the revealed tick CDF boundary', () => {
  const seed = Buffer.alloc(32, 7).toString('base64');
  assert.equal(raceStops('local', 'race', 1, 1, seed, [0, 1000000000]), false);
  assert.equal(raceStops('local', 'race', 1, 2, seed, [0, 1000000000]), true);
});
