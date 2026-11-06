import test from 'node:test';
import assert from 'node:assert/strict';
import { boostInfo, formatStat } from '../../front/onchain/boost-info.mjs';
const parameters = { tick_seconds: 6, speed_boost: '10000', acceleration: '100', temporary_distance: '2000000', temporary_ticks: 50, rocket_damage_bps: 10000 };
const player = { alive: true, safe_speed: '120000', acceleration_count: 0, tiers: [1, 1, 1], commander: null, ships: [
  { base_hp: '55000', hp: '55000', shield: '0', attack: '0' },
  { base_hp: '5500000', hp: '5500000', shield: '1000000', attack: '0' },
  { base_hp: '550000', hp: '550000', shield: '100000', attack: '500000' },
] };
test('boost detail respects discrete windows, damage and commander acceleration', () => {
  const speed = boostInfo('speed', 2, parameters, player);
  assert.equal(speed.stats[0].value, '+20,000 /s'); assert.equal(speed.stats[2].value, '+120,000');
  const damaged = { ...player, ships: player.ships.map((ship, i) => i === 0 ? { ...ship, hp: '0' } : ship) };
  assert.equal(boostInfo('speed', 1, parameters, damaged).stats[1].value, '+4,444.444 /s');
  assert.equal(boostInfo('acceleration', 1, parameters, player).stats[2].value, '+36,000');
  const commander = { ...player, commander: { bonus: 'acceleration', bonus_bps: 2500 } };
  assert.equal(boostInfo('acceleration', 1, parameters, commander).stats[2].value, '+45,000');
});
test('rocket and warp details distinguish damage, criticals and temporary weight', () => {
  const rocket = boostInfo('rocket', 3, parameters, { ...player, commander: { bonus: 'critical', bonus_bps: 1200 } });
  assert.equal(rocket.stats[0].value, '500 HP'); assert.equal(rocket.stats[1].value, '1,500 HP'); assert.equal(rocket.stats[2].value, '12% · 2× damage');
  const warp = boostInfo('temporary', 4, parameters, player);
  assert.equal(warp.stats[0].value, '+8,000,000'); assert.equal(warp.stats[1].value, '300s · 50 windows');
  assert.equal(boostInfo('rocket', 1, parameters).summary, 'Fighter required');
  assert.equal(formatStat(123456789012345678901n), '123,456,789,012,345,678.901');
});
test('repair stats show the missing HP and shields, not a promised fixed price', () => {
  const damaged = { ...player, ships: player.ships.map((ship, i) => i === 1 ? { ...ship, hp: '5000000', shield: '250000' } : ship) };
  const healing = { price_unit: '1000000', hp_per_unit: '1000', fallen_ship_bps: 2500 };
  const repair = boostInfo('heal', 1, parameters, damaged, healing);
  assert.equal(repair.stats[0].value, '500'); assert.equal(repair.stats[1].value, '750');
  assert.match(repair.note, /25%/);
});
