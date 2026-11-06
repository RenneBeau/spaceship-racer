import test from 'node:test';
import assert from 'node:assert/strict';
import { distanceProjection, movementSpeed, processedMovementTick } from '../../front/onchain/telemetry.mjs';
const player = { alive: true, safe_speed: '1000000', distance: '60000000', temporary: [], ships: Array.from({ length: 3 }, () => ({ hp: '1000', base_hp: '1000' })) };

test('live distance continues beyond one tick while fresh snapshots arrive', () => {
  const at = elapsed => distanceProjection({ player, processedTick: 10, tickSeconds: 6, raceElapsed: elapsed }).projected;
  assert.equal(at(62), 62000); assert.equal(at(68), 68000); assert.equal(at(74), 74000);
  const confirmedNext = { ...player, distance: '72000000' };
  assert.equal(distanceProjection({ player: confirmedNext, processedTick: 12, tickSeconds: 6, raceElapsed: 74 }).projected, at(74));
});
test('warp bonuses and speed boosts affect telemetry; stale projections are bounded', () => {
  const boosted = { ...player, safe_speed: '2000000', temporary: [{ distance: '2000000000', expires: 60 }] };
  const result = distanceProjection({ player: boosted, processedTick: 10, tickSeconds: 6, raceElapsed: 60, sinceSnapshot: 4 });
  assert.equal(result.confirmed, 2060000); assert.equal(result.projected, 2068000); assert.equal(result.speed, 2000);
  const stale = distanceProjection({ player, processedTick: 10, tickSeconds: 6, raceElapsed: 60, sinceSnapshot: 1000 });
  assert.equal(stale.projected, 75000);
  assert.equal(distanceProjection({ player, processedTick: 10, tickSeconds: 6, raceElapsed: 90, active: false }).projected, 60000);
  assert.equal(movementSpeed({ ...player, alive: false }), 0);
});
test('processed movement ticks distinguish partial movement pages from completed combat', () => {
  const state = { start_height: 1, tick: 10, phase: 'waiting', cursor: 0 };
  assert.equal(processedMovementTick(state), 9);
  assert.equal(processedMovementTick({ ...state, phase: 'combat' }), 10);
  assert.equal(processedMovementTick({ ...state, phase: 'movement', cursor: 20 }, 20), 10);
  assert.equal(processedMovementTick({ ...state, phase: 'movement', cursor: 20 }, 21), 9);
  assert.equal(processedMovementTick({ ...state, phase: 'snapshot' }), 0);
});
test('live utility power contributes from its activation tick and reconciles without double counting', () => {
  const parameters = { speed_boost: '10000', acceleration: '100', temporary_distance: '2000000', temporary_ticks: 50 };
  const pending = [{ tick: 11, speed: 1, acceleration: 1, temporary: 1 }];
  const before = distanceProjection({ player, processedTick: 10, tickSeconds: 6, raceElapsed: 62, parameters, pending });
  assert.equal(before.confirmed, 60000); assert.equal(before.projected, 2082000);
  const ahead = distanceProjection({ player, processedTick: 10, tickSeconds: 6, raceElapsed: 68, parameters, pending });
  assert.equal(ahead.projected, 2149200);
  const resolved = { ...player, distance: '126000000', safe_speed: '11600000', acceleration_count: 1, temporary: [{ distance: '2000000000', expires: 61 }] };
  const after = distanceProjection({ player: resolved, processedTick: 11, tickSeconds: 6, raceElapsed: 68, parameters });
  assert.equal(after.projected, ahead.projected);
});
