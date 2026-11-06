import test from 'node:test';
import assert from 'node:assert/strict';
import { rocketPrice } from '../../front/onchain/rocket-price.mjs';

test('rocket payment uses the deployed quote and only falls back for an older query schema', async () => {
  assert.equal(await rocketPrice(async () => '200000000', 'game', 'pilot'), '200000000');
  const query = async (_address, msg) => {
    if (msg.rocket_price) throw new Error('unknown variant `rocket_price`');
    if (msg.player) return { ships: [{}, {}, { attack: '500000' }] };
    return { parameters: { rocket_cost_bps: 4000 } };
  };
  assert.equal(await rocketPrice(query, 'game', 'pilot'), '200');
  await assert.rejects(rocketPrice(async () => { throw new Error('RPC unavailable'); }, 'game', 'pilot'), /RPC unavailable/);
});
