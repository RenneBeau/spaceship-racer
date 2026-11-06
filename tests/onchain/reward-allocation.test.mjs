import test from 'node:test';
import assert from 'node:assert/strict';
import { rewardAllocation } from '../../src/onchain/reward-allocation.mjs';
const rows = [
  { wallet: 'leader', score: { score: '40000', tier_sum: '100', weighted_sum: '1000', clan: 1, sponsor: null } },
  { wallet: 'member', score: { score: '60000', tier_sum: '300', weighted_sum: '3600', clan: 1, sponsor: null } },
  { wallet: 'solo', score: { score: '100000', tier_sum: '100', weighted_sum: '1000', clan: null, sponsor: 'leader' } },
];
const game = { total_score: '200000', leader_bps: 500 }, params = { maintenance_referral_bps: 500 }, pool = { spaceusd: '100000000', srt: '612000000' }, clans = [{ id: 1, leader: 'leader' }];
test('SpaceUSD uses clan points then participating levels, with maintenance, premium and referral', () => {
  const member = rewardAllocation('member', game, params, pool, rows, clans);
  assert.equal(member.grossSpaceusd, '35625000'); assert.equal(member.maintenance, '7200000'); assert.equal(member.spaceusd, '28425000'); assert.equal(member.srt, '183600000');
  const leader = rewardAllocation('leader', game, params, pool, rows, clans);
  assert.equal(leader.leaderPremium, '2500000'); assert.equal(leader.referral, '100000'); assert.equal(leader.spaceusd, '12475000');
  const solo = rewardAllocation('solo', game, params, pool, rows, clans);
  assert.equal(solo.spaceusd, '48000000'); assert.equal(solo.srt, '306000000');
});
test('a clan member can earn SpaceUSD from level participation without winning SRT points', () => {
  const players = rows.map(row => row.wallet === 'member' ? { ...row, score: { ...row.score, score: '0' } } : row);
  const result = rewardAllocation('member', { ...game, total_score: '140000' }, params, pool, players, clans);
  assert.ok(BigInt(result.spaceusd) > 0n); assert.equal(result.srt, '0');
});
