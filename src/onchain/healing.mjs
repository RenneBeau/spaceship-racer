import { fleetHealth } from './leaderboards.mjs';
const ceil = (n, d) => (n + d - 1n) / d;
export function integerSqrt(n) {
  if (n < 2n) return n;
  let x = n, next = (x + 1n) / 2n;
  while (next < x) { x = next; next = (x + n / x) / 2n; }
  return x;
}
export function healingQuote(player, settings) {
  const health = fleetHealth(player); let missing = 0n, fallen = 0;
  for (const ship of health.ships) {
    missing += BigInt(ship.maxHp) > BigInt(ship.hp) ? BigInt(ship.maxHp) - BigInt(ship.hp) : 0n;
    missing += BigInt(ship.maxShield) > BigInt(ship.shield) ? BigInt(ship.maxShield) - BigInt(ship.shield) : 0n;
    if (BigInt(ship.maxHp) > 0n && BigInt(ship.hp) === 0n) fallen++;
  }
  if (missing === 0n) return { missing: '0', fallen_ships: 0, price: '0' };
  const unit = BigInt(settings.price_unit), denominator = BigInt(settings.hp_per_unit) * 1000n * (BigInt(player.tiers[1]) + 1n);
  const squared = ceil(missing * unit * unit, denominator), root = integerSqrt(squared);
  const rounded = root * root < squared ? root + 1n : root;
  return { missing: missing.toString(), fallen_ships: fallen, price: ceil(rounded * (10000n + BigInt(settings.fallen_ship_bps) * BigInt(fallen)), 10000n).toString() };
}
