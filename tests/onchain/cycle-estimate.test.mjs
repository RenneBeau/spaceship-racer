import test from 'node:test';
import assert from 'node:assert/strict';
import { cycleEstimate } from '../../src/onchain/cycle-estimate.mjs';
const game = { race: 3, cycle: 2, completed: 1, total_score: '10000', start_height: 10, phase: 'waiting', budget_usd: '999999999', budget_srt: '88888888' };
const p = { races_per_cycle: 2, tick_seconds: 6, duration_cdf: [500000000, 1000000000] };
const pools = { reserve: '31557600000000', annual_bps: 10000, checkpoint_time: 1000, accrual_remainder: '0', available: { spaceusd: '1000000', srt: '4000000' } };
test('cycle estimates exclude stale prior-cycle budgets and claim liabilities', () => {
  const estimate = cycleEstimate(game, p, { ...pools, liabilities: { spaceusd: '999999999999', srt: '999999999999' } }, { height: 10, time: new Date(1000000).toISOString() });
  assert.equal(estimate.available.spaceusd, '1000000'); assert.equal(estimate.available.srt, '4000000');
  assert.equal(estimate.remainingSeconds, 9); assert.equal(estimate.projected.spaceusd, '10000000');
  assert.equal(estimate.perPoint.spaceusd, '1000000'); assert.equal(estimate.perPoint.srt, '4000000');
  assert.equal(estimate.projectedPerPoint.spaceusd, '5000000'); assert.equal(estimate.projectedPerPoint.srt, '2000000');
});

test('current SRT share uses earned points, not projected future points', () => {
  const estimate = cycleEstimate({ ...game, completed: 27, total_score: '1078000' }, { ...p, races_per_cycle: 100 }, { ...pools, available: { spaceusd: '0', srt: '612000000' } }, { height: 10, time: new Date(1000000).toISOString() });
  const share = BigInt(estimate.available.srt) * 418000n / BigInt(estimate.currentScore);
  assert.equal(share.toString(), '237306122');
  assert.ok(BigInt(estimate.projectedScore) > BigInt(estimate.currentScore));
});
test('settlement uses the locked budget, and a no-winner cycle has no point value', () => {
  const estimate = cycleEstimate({ ...game, phase: 'settlement', completed: 2, total_score: '0' }, p, pools, { height: 20, time: new Date(1000000).toISOString() });
  assert.equal(estimate.locked, true); assert.equal(estimate.projected.spaceusd, game.budget_usd); assert.equal(estimate.perPoint.spaceusd, null);
});
