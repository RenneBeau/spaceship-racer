const YEAR = 31557600n, POINT = 10000n;
const min = (a, b) => a < b ? a : b;
export function cycleEstimate(game, parameters, pools, at) {
  const remainingRaces = Math.max(0, parameters.races_per_cycle - game.completed);
  const cdf = parameters.duration_cdf, tick = parameters.tick_seconds;
  const meanSeconds = cdf.reduce((sum, _p, i) => sum + (1 - (cdf[i - 1] ?? 0) / 1e9) * tick, 0);
  const elapsed = Math.max(0, Math.min(cdf.length, at.height - game.start_height));
  const survival = 1 - (cdf[elapsed - 1] ?? 0) / 1e9;
  const tailSeconds = survival > 0 ? cdf.slice(elapsed).reduce((sum, _p, j) => sum + (1 - (cdf[elapsed + j - 1] ?? 0) / 1e9) * tick, 0) / survival : 0;
  const running = ['waiting', 'movement', 'combat'].includes(game.phase);
  const remainingSeconds = Math.ceil(Math.max(0, remainingRaces * meanSeconds - (running && remainingRaces ? meanSeconds - tailSeconds : 0)));
  const locked = ['clan_premiums', 'settlement'].includes(game.phase);
  const reserve = BigInt(pools.reserve), denominator = YEAR * 10000n;
  const seconds = Math.max(0, Math.floor(Date.parse(at.time) / 1000) - pools.checkpoint_time);
  const accrued = delay => min(reserve, (reserve * BigInt(seconds + delay) * BigInt(pools.annual_bps) + BigInt(pools.accrual_remainder ?? '0')) / denominator);
  const spaceusdNow = locked ? BigInt(game.budget_usd) : BigInt(pools.available.spaceusd) + accrued(0);
  const srt = locked ? BigInt(game.budget_srt) : BigInt(pools.available.srt);
  const projectedSpaceusd = locked ? spaceusdNow : BigInt(pools.available.spaceusd) + accrued(remainingSeconds);
  const score = BigInt(game.total_score);
  // Fixed multiplier distribution; once races finish, use the observed score rate.
  const expectedPerRace = 55380n;
  const projectedScore = score + (game.completed > 0 && score > 0n ? score / BigInt(game.completed) : expectedPerRace) * BigInt(remainingRaces);
  const perPoint = amount => score ? (amount * POINT / score).toString() : null;
  const projectedPerPoint = amount => projectedScore ? (amount * POINT / projectedScore).toString() : null;
  return { cycle: game.cycle, locked, remainingRaces, remainingSeconds,
    available: { spaceusd: spaceusdNow.toString(), srt: srt.toString() },
    projected: { spaceusd: projectedSpaceusd.toString(), srt: srt.toString() },
    currentScore: score.toString(), projectedScore: projectedScore.toString(),
    perPoint: { spaceusd: perPoint(spaceusdNow), srt: perPoint(srt) },
    projectedPerPoint: { spaceusd: projectedPerPoint(projectedSpaceusd), srt: projectedPerPoint(srt) },
    note: 'Estimate before maintenance and clan redistribution. Future points, race durations and boost spending can change it. No future SRT funding or boost sales are assumed.' };
}
