export function movementSpeed(player) {
  if (!player?.alive) return 0;
  let loss = 0;
  for (const [index, weight] of [[0, 5], [1, 1], [2, 3]]) {
    const ship = player.ships[index];
    if (BigInt(ship.base_hp) > 0n) loss += Number(BigInt(ship.base_hp) - BigInt(ship.hp)) / Number(ship.base_hp) * weight;
  }
  return Number(player.safe_speed) / 1000 * (9 - Math.min(9, loss)) / 9;
}
export function distanceProjection({ player, processedTick, tickSeconds, raceElapsed, sinceSnapshot = 0, active = true, pending = [], parameters = {} }) {
  const confirmed = Number(player.distance) / 1000;
  const temporary = (player.temporary ?? []).reduce((sum, item) => sum + Number(item.distance) / 1000, 0);
  // The snapshot represents processed simulation time, not its arrival time.
  // Advancing from that common clock avoids both a six-second freeze and
  // resetting the animation whenever a new snapshot arrives.
  const maximum = parameters.duration_cdf?.length ? parameters.duration_cdf.length * tickSeconds : Infinity;
  const elapsed = Math.min(maximum, raceElapsed + Math.min(sinceSnapshot, 15));
  const ahead = active ? Math.max(0, elapsed - processedTick * tickSeconds) : 0;
  const liveTick = Math.floor(elapsed / tickSeconds) + 1;
  const speed = movementSpeed(player), factor = player.alive ? speed / (Number(player.safe_speed) / 1000 || 1) : 0;
  const activeTemporary = !active ? temporary : (player.temporary ?? []).filter(item => item.expires > liveTick).reduce((sum, item) => sum + Number(item.distance) / 1000, 0);
  let projected = confirmed + activeTemporary + speed * ahead;
  const accelerationBonus = player.commander?.bonus === 'acceleration' ? 1 + player.commander.bonus_bps / 10000 : 1;
  const acceleration = Number(parameters.acceleration ?? 0) * accelerationBonus * factor;
  const whole = Math.floor(ahead / tickSeconds), remainder = ahead % tickSeconds;
  projected += acceleration * (player.acceleration_count ?? 0) * (tickSeconds ** 2 * whole * (whole - 1) / 2 + tickSeconds * whole * remainder);
  if (active && player.alive) for (const entry of pending) {
    const duration = Math.max(0, elapsed - Math.max(processedTick * tickSeconds, (entry.tick - 1) * tickSeconds));
    projected += Number(parameters.speed_boost ?? 0) * entry.speed * factor * duration;
    const accelerated = Math.max(0, elapsed - entry.tick * tickSeconds);
    const complete = Math.floor(accelerated / tickSeconds), partial = accelerated % tickSeconds;
    projected += acceleration * entry.acceleration * (tickSeconds ** 2 * complete * (complete + 1) / 2 + tickSeconds * (complete + 1) * partial);
    if (liveTick < entry.tick + Number(parameters.temporary_ticks ?? 0)) projected += Number(parameters.temporary_distance ?? 0) * entry.temporary;
  }
  return { confirmed: confirmed + temporary, projected, speed, temporary: activeTemporary };
}
export function processedMovementTick(state, index = null) {
  if (state.phase === 'snapshot' || !state.start_height) return 0;
  if (state.phase === 'waiting') return Math.max(0, state.tick - 1);
  if (state.phase === 'movement') return index !== null && index <= state.cursor ? state.tick : Math.max(0, state.tick - 1);
  return state.tick;
}
