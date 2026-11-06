import { createHash } from 'node:crypto';

// Reproduce only the already-revealed tick's deterministic stop draw so a
// multi-tick catch-up transaction ends exactly at a race boundary. No future
// rounds are guessed or replaced.
export function raceStops(chainId, contract, race, tick, seed, cdf) {
  const prior = tick === 1 ? 0 : cdf[tick - 2];
  const bound = BigInt(1000000000 - prior), threshold = BigInt(cdf[tick - 1] - prior);
  const max = (1n << 256n) - 1n, limit = max - max % bound;
  for (let attempt = 0; attempt < 16; attempt++) {
    const hash = createHash('sha256');
    for (const part of [Buffer.from(chainId), Buffer.from(contract), Buffer.from('race-stop'), Buffer.from(seed, 'base64')]) {
      const length = Buffer.alloc(8); length.writeBigUInt64BE(BigInt(part.length)); hash.update(length); hash.update(part);
    }
    const suffix = Buffer.alloc(17); suffix.writeBigUInt64BE(BigInt(race)); suffix.writeUInt32BE(tick, 8); suffix[16] = attempt;
    hash.update(suffix);
    const value = BigInt(`0x${hash.digest('hex')}`);
    if (value < limit) return value % bound < threshold;
  }
  throw new Error('Random rejection limit');
}

export function planRace(state, height) {
  if (state.phase === 'waiting') {
    if (state.sealed < state.tick) {
      return height >= state.start_height + state.tick ? 'seal' : 'wait';
    }
    // Do not keep sealing a growing backlog before resolving the current tick.
    return 'randomness';
  }
  if (['boundary', 'drain_equipment', 'snapshot', 'movement', 'combat', 'checkpoint', 'accrue', 'budget', 'clan_premiums', 'settlement'].includes(state.phase)) {
    return 'advance';
  }
  throw new Error(`Unknown race phase: ${state.phase}`);
}
export function launchSteps(phase, players, pendingWallets) {
  if (players < 1 || players >= 20 || pendingWallets >= 20) return 0;
  return (phase === 'boundary' ? 1 : 0) + (pendingWallets > 0 ? 1 : 0) + 2;
}
export function sealBatchLimit(state, height, maximumTicks) {
  const closed = Math.min(Math.max(0, height - state.start_height), maximumTicks);
  return Math.max(0, Math.min(20, closed - state.sealed));
}

export function planRewardFunding(balances, rewardPool, retain = '0') {
  if (!rewardPool) return [];
  const keep = BigInt(retain);
  if (keep < 0n) throw new Error('Retain amount must not be negative');
  return Object.entries(balances).flatMap(([token, balance]) => {
    if (!/^\d+$/.test(String(balance))) throw new Error(`Invalid treasury balance for ${token}`);
    const amount = BigInt(balance) > keep ? BigInt(balance) - keep : 0n;
    return amount > 0n ? [{ token, amount: amount.toString() }] : [];
  });
}

export function skinCanFulfill(preview) {
  return preview !== null && typeof preview === 'object' && typeof preview.ready?.code === 'string';
}

export function staleWork(message) {
  return ['already fulfilled', 'job already fulfilled', 'skin claim already fulfilled',
    'no closed unsealed blocks', 'cannot seal here', 'tick not sealed', 'randomness pending', 'stale or unsupported tick check',
    'selected skin currently owned; same claim must wait', 'game globally paused']
    .some(reason => message.includes(`Generic error: ${reason}`));
}
