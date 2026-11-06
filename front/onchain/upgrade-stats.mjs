// Match race/src/math.rs: scale in milli-units, then round integer stats.
import { integerSqrt } from '../../src/onchain/healing.mjs';
export function upgradeStats(ship, level, healing = null, srtDecimals = 6) {
  if (!Number.isInteger(Number(level)) || Number(level) < 0 || Number(level) > 1000) return null;
  const bases = { racer: [55, 100, 500, 0, 0], tank: [5500, 10, 50000, 1000, 0], fighter: [550, 20, 5000, 100, 500] }[ship];
  if (!bases) return null;
  const tier = BigInt(level), factor = tier * (2000n + (tier > 0n ? tier - 1n : 0n));
  const scaled = bases.map((base, index) => {
    const milli = BigInt(base) * (index === 2 ? 1n : 1000n) * factor / 2000n;
    return index === 2 ? Number(milli) / 1000 : Number((milli + 500n) / 1000n);
  });
  const stats = { Level: Number(level), HP: scaled[0], Speed: scaled[1], Shield: scaled[3], Attack: scaled[4] };
  if (ship === 'tank' && healing?.enabled) {
    const unit = BigInt(healing.price_unit), denominator = BigInt(healing.hp_per_unit) * 1000n * (tier + 1n);
    const squared = (1000000n * unit * unit + denominator - 1n) / denominator;
    const root = integerSqrt(squared), rounded = root * root < squared ? root + 1n : root;
    stats['Repair 1,000 HP (SRT)'] = Number(rounded) / 10 ** srtDecimals;
  }
  return stats;
}
