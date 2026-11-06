import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { fromBech32 } from '@cosmjs/encoding';

export const ROOT = fileURLToPath(new URL('../../', import.meta.url));
export function address(value) {
  if (typeof value !== 'string' || value.length > 90) throw Object.assign(new Error('Invalid Terra address'), { status: 400 });
  try {
    const decoded = fromBech32(value);
    if (decoded.prefix !== 'terra' || ![20, 32].includes(decoded.data.length)) throw new Error();
  } catch { throw Object.assign(new Error('Invalid Terra address'), { status: 400 }); }
  return value;
}
export function integer(value, fallback, maximum = 100) {
  if (value === undefined) return fallback;
  if (!/^\d+$/.test(String(value))) throw Object.assign(new Error('Invalid integer'), { status: 400 });
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 1 || number > maximum) throw Object.assign(new Error('Integer out of range'), { status: 400 });
  return number;
}
export function loadConfig(env = process.env) {
  const manifest = JSON.parse(readFileSync(env.RACER_MANIFEST ?? new URL('../../contracts/local/deployment.json', import.meta.url), 'utf8'));
  const contracts = Object.fromEntries(['game', 'fleet', 'equipment', 'rewards', 'clans', 'payments', 'capitalTreasury', 'marketplace', 'recruitment', 'skins', 'skinCollection', 'randomness', 'pause']
    .map(key => [key, address(manifest[key])]));
  const previousGames = (manifest.previous_games ?? []).map(address);
  const previousClans = (manifest.previous_clans ?? []).map(address);
  previousGames.forEach((value, index) => { contracts[`legacyGame${index}`] = value; });
  previousClans.forEach((value, index) => { contracts[`legacyClans${index}`] = value; });
  if (manifest.reinvestment) contracts.reinvestment = address(manifest.reinvestment);
  if (manifest.backing_rewards) contracts.backingRewards = address(manifest.backing_rewards);
  if (manifest.previous_payments) contracts.legacyPayments = address(manifest.previous_payments);
  const tokens = manifest.paymentTokens.map(address);
  if (tokens.length !== 5 || new Set(tokens).size !== 5) throw new Error('Expected five distinct payment tokens');
  const identity = JSON.stringify({ chainId: manifest.chain_id, contracts, tokens });
  const fingerprint = createHash('sha256').update(identity).digest('hex');
  const rpc = env.RACER_RPC ?? manifest.indexer?.rpc ?? 'http://127.0.0.1:27657';
  const lcd = env.RACER_LCD ?? manifest.indexer?.lcd ?? 'http://127.0.0.1:1417';
  for (const url of [rpc, lcd]) if (!['http:', 'https:'].includes(new URL(url).protocol)) throw new Error('Invalid chain endpoint');
  return { chainId: manifest.chain_id, alpha: manifest.alpha === true, contracts, previousGames, tokens, fingerprint, rpc, lcd,
    keeperAddress: manifest.keeper_address ? address(manifest.keeper_address) : null,
    publicRpc: env.RACER_PUBLIC_RPC ?? rpc, publicLcd: env.RACER_PUBLIC_LCD ?? lcd,
    gasPrice: manifest.gas_price_uluna ?? '28.325',
    database: env.RACER_DATABASE ?? `${ROOT}data/onchain-${fingerprint.slice(0, 12)}.sqlite`,
    host: env.RACER_HOST ?? '127.0.0.1', port: integer(env.RACER_PORT, 8787, 65535),
    startHeight: env.RACER_START_HEIGHT ? integer(env.RACER_START_HEIGHT, 1, Number.MAX_SAFE_INTEGER) : null };
}
