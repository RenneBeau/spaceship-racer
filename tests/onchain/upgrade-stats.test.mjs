import test from 'node:test';
import assert from 'node:assert/strict';
import { upgradeStats } from '../../front/onchain/upgrade-stats.mjs';

test('hangar preview matches contract base stats and integer rounding', () => {
  assert.deepEqual(upgradeStats('racer', 1), { Level: 1, HP: 55, Speed: 100, Shield: 0, Attack: 0 });
  assert.equal(upgradeStats('fighter', 100).Attack, 52475);
  assert.equal(upgradeStats('tank', 2).Shield, 2001);
  assert.equal(upgradeStats('tank', 0).HP, 0);
  assert.equal(upgradeStats('racer', 1001), null);
  const healing = { enabled: true, price_unit: '1000000', hp_per_unit: '1000' };
  assert.equal(upgradeStats('tank', 0, healing)['Repair 1,000 HP (SRT)'], 1);
  assert.equal(upgradeStats('tank', 3, healing)['Repair 1,000 HP (SRT)'], 0.5);
});
