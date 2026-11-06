export function raceElapsedByHeight(game, height, parameters) {
  if (!(game.start_height > 0)) return 0;
  return Math.max(0, Math.min(parameters.duration_cdf.length, height - game.start_height)) * parameters.tick_seconds;
}

export function assessChainHealth(tip, validators = null, now = Date.now(), maximumAgeSeconds = 30) {
  const timestamp = Date.parse(tip.time), age = (now - timestamp) / 1000, reasons = [];
  if (!Number.isFinite(age)) reasons.push('invalid_block_time');
  else if (age > maximumAgeSeconds) reasons.push('blocks_stale');
  else if (age < -maximumAgeSeconds) reasons.push('block_time_in_future');
  if (validators && !validators.some(v => !v.jailed && v.status === 'BOND_STATUS_BONDED')) reasons.push('no_active_bonded_validator');
  return { healthy: reasons.length === 0, blockAgeSeconds: Number.isFinite(age) ? Math.max(0, age) : null,
    maximumBlockAgeSeconds: maximumAgeSeconds, reasons,
    ...(validators ? { bondedValidators: validators.length, activeBondedValidators: validators.filter(v => !v.jailed && v.status === 'BOND_STATUS_BONDED').length } : {}) };
}

export function assessGameHealth(game, { players = 0, paused = false, launchTime = null, lastProgressTime = null, keeperBalance = null, minimumKeeperBalance = '1000000000', now = Date.now() } = {}) {
  const reasons = [], active = players > 0 && !paused && (launchTime === null || now / 1000 >= launchTime);
  const age = lastProgressTime ? Math.max(0, (now - Date.parse(lastProgressTime)) / 1000) : 0;
  if (keeperBalance !== null && BigInt(keeperBalance) < BigInt(minimumKeeperBalance)) reasons.push('keeper_low_gas');
  if (active && age > 90) reasons.push('game_not_advancing');
  return { gameHealthy: reasons.length === 0, gameReasons: reasons, gameProgressAgeSeconds: age, gameExpectedToAdvance: active, race: game.race, phase: game.phase,
    ...(keeperBalance === null ? {} : { keeperBalance, keeperMinimumBalance: minimumKeeperBalance, keeperFeeDenom: 'uluna' }) };
}
