// Only the offchain configuration builder uses floating point. The contract
// stores integer CDF thresholds and never samples or publishes an end in advance.
export function legacyParameters(tickSeconds = 6) {
  const maximumSeconds = 1450;
  const ticks = Math.ceil(maximumSeconds / tickSeconds);
  return {
    tick_seconds: tickSeconds,
    races_per_cycle: 100,
    duration_cdf: Array.from({ length: ticks }, (_, i) =>
      i === ticks - 1 ? 1_000_000_000 : Math.floor(
        Math.pow(Math.min((i + 1) * tickSeconds / maximumSeconds, 1), 1 / 1.5849625007) * 1_000_000_000)),
    boost_price: '1000000', speed_boost: '10000', acceleration: '100',
    temporary_distance: '2000000', temporary_ticks: Math.ceil(300 / tickSeconds),
    rocket_cost_bps: 4000, rocket_damage_bps: 10000, boost_fee_bps: 500,
    maintenance_referral_bps: 500, max_actions_per_tick: 0, max_players: 1000,
  };
}

export function commanderCatalogue() {
  return [
    ['cadet_nova', 'common', 'speed', 300, 400],
    ['bolt_jax', 'common', 'acceleration', 400, 300],
    ['shield_iris', 'rare', 'shield', 800, 180],
    ['scrap_milo', 'rare', 'repair', 500, 90],
    ['profit_zen', 'epic', 'reward', 1000, 25],
    ['orion_hex', 'legendary', 'critical', 1200, 5],
  ].map(([code, rarity, bonus, bonus_bps, weight]) => ({
    attributes: { code, kind: 'commander', rarity, bonus, bonus_bps }, weight, token_uri: null,
  }));
}

// Local fixtures use existing image codes. Public-launch rarity/weight tuning
// remains operator configuration, rather than a hidden hardcoded probability.
export function skinCatalogue() {
  return ['racer', 'tank', 'fighter'].flatMap(ship => [1, 2, 3].map(number => ({
    attributes: { code: `${ship}${number}`, kind: `${ship}_skin`, rarity: 'common', bonus: 'cosmetic', bonus_bps: 0 },
    weight: 1, token_uri: null,
  })));
}
