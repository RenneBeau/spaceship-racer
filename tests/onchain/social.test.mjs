import test from 'node:test';
import assert from 'node:assert/strict';
import { Store } from '../../src/onchain/store.mjs';
test('profile history deduplicates victories and counts only reward transfers to the claiming wallet', () => {
  const store = new Store(':memory:', 'social');
  const attrs = values => Object.entries(values).map(([key, value]) => ({ key, value }));
  try {
    const events = [
      { contract: 'game', action: 'advance_race', attributes: attrs({ race_ended: '9', winner: 'pilot' }) },
      { contract: 'game', action: 'advance_race', attributes: attrs({ race_ended: '9', winner: 'pilot' }) },
      { contract: 'rewards', action: 'claim_rewards', attributes: attrs({ wallet: 'pilot' }) },
      { contract: 'srt', action: 'transfer', attributes: attrs({ from: 'rewards', to: 'pilot', amount: '12345678901234567890' }) },
      { contract: 'usd', action: 'transfer', attributes: attrs({ from: 'rewards', to: 'pilot', amount: '25' }) },
      { contract: 'usd', action: 'transfer', attributes: attrs({ from: 'rewards', to: 'other', amount: '1000' }) },
    ].map((event, index) => ({ ...event, index }));
    store.commit({ height: 1, hash: 'A', time: '2026-11-01T00:00:00Z' }, [{ hash: 'TX', index: 0, participants: ['pilot'], events }]);
    assert.equal(store.victories('pilot', 'game').length, 1);
    assert.deepEqual(store.victories('other', 'game'), []);
    const claims = store.rewardClaims('pilot', 'rewards', 'srt', 'usd');
    assert.equal(claims.length, 1); assert.equal(claims[0].srt, '12345678901234567890'); assert.equal(claims[0].spaceusd, '25');
  } finally { store.close(); }
});
