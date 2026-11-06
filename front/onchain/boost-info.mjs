import { fleetHealth } from '../../src/onchain/leaderboards.mjs';
import { healingQuote } from '../../src/onchain/healing.mjs';

const SCALE = 1000n, FACTOR = 9000000n;
export function formatStat(value, decimals = 3, locale = 'en-US') {
  const raw = BigInt(value), scale = 10n ** BigInt(decimals);
  const fraction = (raw % scale).toString().padStart(decimals, '0').replace(/0+$/, '');
  return `${new Intl.NumberFormat(locale).format(raw / scale)}${fraction ? `${new Intl.NumberFormat(locale).formatToParts(1.1).find(part => part.type === 'decimal')?.value ?? '.'}${fraction}` : ''}`;
}
function damageFactor(player) {
  if (!player?.alive) return 0n;
  let loss = 0n;
  for (const [i, weight] of [[0, 5n], [1, 1n], [2, 3n]]) {
    const ship = player.ships[i], base = BigInt(ship.base_hp), hp = BigInt(ship.hp);
    if (base > 0n && hp < base) loss += (base - hp) * weight * 1000000n / base;
  }
  return FACTOR - (loss < FACTOR ? loss : FACTOR);
}
export function boostInfo(kind, quantity, parameters, player = null, healing = null, locale = 'en-US') {
  const count = BigInt(quantity), tick = BigInt(parameters.tick_seconds), factor = damageFactor(player);
  const base = BigInt(player?.safe_speed ?? 0), effective = speed => speed * factor / FACTOR;
  const format = value => formatStat(value, 3, locale), whole = value => formatStat(value, 0, locale);
  const stat = (label, value) => ({ label, value });
  if (kind === 'speed') {
    const gain = BigInt(parameters.speed_boost) * SCALE * count;
    const delta = player ? effective(base + gain) - effective(base) : null;
    return { summary: `+${format(BigInt(parameters.speed_boost) * SCALE)} /s`, stats: [
      stat('Base speed added', `+${format(gain)} /s`),
      stat('With current damage', delta === null ? 'Watch a fleet' : `+${format(delta)} /s`),
      stat(`Extra distance / ${tick}s`, delta === null ? 'Fleet-dependent' : `+${format(delta * tick)}`),
      stat('Duration', 'Rest of race'),
    ], note: 'Distance assumes the fleet keeps its current health for one full window. Further damage and the race ending change the result.' };
  }
  if (kind === 'acceleration') {
    const bonus = player?.commander?.bonus === 'acceleration' ? BigInt(player.commander.bonus_bps) : 0n;
    const step = n => BigInt(parameters.acceleration) * SCALE * n * tick * (10000n + bonus) / 10000n;
    const previous = BigInt(player?.acceleration_count ?? 0), before = step(previous), after = step(previous + count);
    let distance = 0n;
    for (let i = 0n; i < 5n; i++) distance += (effective(base + after * i) - effective(base + before * i)) * tick;
    return { summary: `+${format(BigInt(parameters.acceleration) * SCALE * (10000n + bonus) / 10000n)} /s²`, stats: [
      stat('Acceleration added', `+${format((after - before) / tick)} /s²`),
      stat(`Speed gain / ${tick}s`, player ? `+${format(effective(base + after) - effective(base + before))} /s` : 'Fleet-dependent'),
      stat(`Extra distance / ${tick * 5n}s`, player ? `+${format(distance)}` : 'Fleet-dependent'),
      stat('Commander bonus', `${formatStat(bonus, 2, locale)}%`),
    ], note: 'The distance example includes five full windows, starting with the activation window. Acceleration builds after movement; it assumes unchanged health, no other actions, and a race lasting that long.' };
  }
  if (kind === 'temporary') {
    const duration = BigInt(parameters.temporary_ticks) * tick;
    return { summary: `+${format(BigInt(parameters.temporary_distance) * SCALE)} distance`, stats: [
      stat('Temporary distance', `+${format(BigInt(parameters.temporary_distance) * SCALE * count)}`),
      stat('Duration', `${whole(duration)}s · ${parameters.temporary_ticks} windows`),
      stat('Movement speed', 'Unchanged'), stat('Affects', 'Win & target weight'),
    ], note: 'Adds temporary selection weight, not travelled distance. It increases both your winner weight and your weight as a rocket target. It expires after the duration or when the race ends.' };
  }
  if (kind === 'rocket') {
    const owned = player && BigInt(player.ships[2].base_hp ?? '0') > 0n;
    const damage = owned && player.ships[2].attack != null ? BigInt(player.ships[2].attack) * BigInt(parameters.rocket_damage_bps) / 10000n : null;
    const unavailable = owned ? 'Stats loading' : 'Fighter required';
    const chance = player?.commander?.bonus === 'critical' ? Math.min(10000, player.commander.bonus_bps) : 0;
    return { summary: damage === null ? unavailable : `${format(damage)} HP / rocket`, stats: [
      stat('Damage / rocket', damage === null ? unavailable : `${format(damage)} HP`),
      stat('Base salvo damage', damage === null ? 'Fleet-dependent' : `${format(damage * count)} HP`),
      stat('Critical chance', `${chance / 100}%${chance ? ' · 2× damage' : ''}`),
      stat('Hit order', 'Tank → fighter → racer'),
    ], note: 'Each rocket draws an eligible opponent by distance weight. Tank shields absorb damage first. Damage does not spill into a second ship on the same hit; rockets with no eligible target are refundable.' };
  }
  if (kind === 'heal') {
    const health = player ? fleetHealth(player) : null;
    const quote = player && healing ? healingQuote(player, healing) : null;
    const missing = field => health ? (BigInt(health[`max${field}`]) - BigInt(health[field.toLowerCase()])).toString() : null;
    return { summary: 'HP, shields & revival', stats: [
      stat('HP to restore', health ? format(missing('Hp')) : 'Watch a fleet'),
      stat('Shields to restore', health ? format(missing('Shield')) : 'Watch a fleet'),
      stat('Fallen ships', quote ? String(quote.fallen_ships) : '—'),
      stat('Tank level', player ? whole(player.tiers[1]) : '—'),
    ], note: `Cost grows with damage and falls with tank level. Each fallen ship adds ${(healing?.fallen_ship_bps ?? 0) / 100}% to the base repair price. The actual charge is settled during replay and cannot exceed your spend limit.` };
  }
  return { summary: '', stats: [], note: '' };
}
