import test from 'node:test';
import assert from 'node:assert/strict';
import { assessChainHealth, raceElapsedByHeight } from '../../src/onchain/health.mjs';
import { Chain } from '../../src/onchain/chain.mjs';

test('RPC reachability and indexer agreement cannot disguise stale blocks or a jailed sole validator', () => {
  const now = Date.parse('2026-11-03T12:00:00Z');
  const good = [{ jailed: false, status: 'BOND_STATUS_BONDED' }];
  assert.equal(assessChainHealth({ time: '2026-11-03T11:59:54Z' }, good, now).healthy, true);
  const stale = assessChainHealth({ time: '2026-11-03T05:28:08Z' }, good, now);
  assert.equal(stale.healthy, false); assert.ok(stale.reasons.includes('blocks_stale'));
  const jailed = assessChainHealth({ time: '2026-11-03T11:59:54Z' }, [{ jailed: true, status: 'BOND_STATUS_BONDED' }], now);
  assert.equal(jailed.healthy, false); assert.ok(jailed.reasons.includes('no_active_bonded_validator'));
  assert.equal(assessChainHealth({ time: 'invalid' }, good, now).healthy, false);
});

test('pinned reads use committed application height, excluding a failed finalization block', async () => {
  const chain = new Chain({ chainId: 'spaceship-racer-local' });
  chain.rpc = async (method, params) => {
    if (method === 'status') return { node_info: { network: 'spaceship-racer-local' }, sync_info: { latest_block_height: '100800', latest_block_time: '2026-11-03T05:28:08Z', latest_block_hash: 'uncommitted' } };
    if (method === 'abci_info') return { response: { last_block_height: '100799' } };
    assert.equal(method, 'block'); assert.equal(params.height, '100799');
    return { block_id: { hash: 'committed' }, block: { header: { time: '2026-11-03T05:28:02Z' } } };
  };
  assert.deepEqual(await chain.pinnedTip(), { height: 100799, time: '2026-11-03T05:28:02Z', hash: 'committed', consensusHeight: 100800, applicationHeight: 100799 });
});

test('a long wall-clock halt does not advance height-based race physics', () => {
  const p = { duration_cdf: Array(243).fill(1e9), tick_seconds: 6 };
  assert.equal(raceElapsedByHeight({ start_height: 100799 }, 100800, p), 6);
  assert.equal(raceElapsedByHeight({ start_height: 100799 }, 100799, p), 0);
  assert.equal(raceElapsedByHeight({ start_height: 0 }, 100800, p), 0);
  assert.equal(raceElapsedByHeight({ start_height: 100799 }, 101999, p), 1458);
});
