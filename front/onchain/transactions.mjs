import { fromBech32, toBase64, toUtf8 } from '@cosmjs/encoding';

const MAX = (1n << 128n) - 1n;
export function terra(value) {
  const decoded = fromBech32(value);
  if (decoded.prefix !== 'terra' || ![20, 32].includes(decoded.data.length)) throw new Error('Invalid Terra address');
  return value;
}
export function units(value, decimals) {
  if (!Number.isInteger(decimals) || decimals < 0 || decimals > 18) throw new Error('Invalid token decimals');
  const text = String(value).trim();
  if (!/^\d+(?:\.\d+)?$/.test(text)) throw new Error('Enter a decimal amount without exponent notation');
  const [whole, fraction = ''] = text.split('.');
  if (fraction.length > decimals) throw new Error(`This token supports ${decimals} decimals`);
  const amount = BigInt(whole) * 10n ** BigInt(decimals) + BigInt(fraction.padEnd(decimals, '0') || '0');
  if (amount > MAX) throw new Error('Amount exceeds Uint128');
  return amount.toString();
}
export function display(value = '0', decimals = 6) {
  const raw = BigInt(value); const scale = 10n ** BigInt(decimals);
  const fraction = (raw % scale).toString().padStart(decimals, '0').replace(/0+$/, '');
  return `${raw / scale}${fraction ? `.${fraction}` : ''}`;
}
export function number(value, maximum = Number.MAX_SAFE_INTEGER) {
  if (!/^\d+$/.test(String(value))) throw new Error('Enter a positive integer');
  const result = Number(value);
  if (!Number.isSafeInteger(result) || result < 1 || result > maximum) throw new Error(`Integer must be between 1 and ${maximum}`);
  return result;
}
export function amount(value) {
  if (!/^\d+$/.test(String(value))) throw new Error('Invalid smallest-unit amount');
  const raw = BigInt(value); if (raw <= 0n || raw > MAX) throw new Error('Amount out of range'); return raw.toString();
}
export function instruction(contract, msg) { return { contractAddress: terra(contract), msg, funds: [] }; }
export function send(token, contract, paid, hook) {
  return instruction(token, { send: { contract: terra(contract), amount: amount(paid), msg: toBase64(toUtf8(JSON.stringify(hook))) } });
}
export function purchase(mode, ship, target, sponsor = '') {
  if (!['racer', 'tank', 'fighter'].includes(ship) || !['buy', 'upgrade'].includes(mode)) throw new Error('Invalid ship purchase');
  return { [mode]: { ship, target_tier: number(target, 1000), ...(mode === 'buy' ? { sponsor: sponsor ? terra(sponsor.trim()) : null } : {}) } };
}
export function shipPayment(config, token, wanted, quote, bps, deadline) {
  number(bps + 1, 5001); number(deadline);
  const direct = token === config.spaceusd;
  const ceiling = quote.maximum_price ?? quote.price;
  const offer = direct ? BigInt(ceiling) : (BigInt(quote.estimated_offer) * (10000n + BigInt(bps)) + 9999n) / 10000n;
  if (!Array.isArray(quote.route) || quote.route[0] !== token || quote.route.at(-1) !== config.spaceusd) throw new Error('Invalid onchain quote route');
  quote.route.forEach(terra);
  return send(token, config.contracts.payments, offer.toString(), { purchase: wanted, expected_price: amount(quote.price), minimum_receive: amount(ceiling),
    ...(quote.maximum_price ? { maximum_price: amount(quote.maximum_price) } : {}),
    max_spread: display(String(bps), 4), deadline: quote.valid_until ?? deadline, route: quote.route });
}
export function boost(config, kind, count, race, price) {
  if (!['speed', 'acceleration', 'temporary', 'rocket', 'heal'].includes(kind)) throw new Error('Invalid boost');
  const quantity = number(count, 0xffffffff);
  if (kind === 'heal' && quantity !== 1) throw new Error('One repair request per race window');
  return send(config.srt, config.contracts.game, amount((BigInt(amount(price)) * BigInt(quantity)).toString()), { race: number(race), kind, count: quantity });
}
export function bind(collection, tokenId, vault, kind) {
  const slots = { commander: 'commander', racer_skin: 'racer_skin', tank_skin: 'tank_skin', fighter_skin: 'fighter_skin' };
  if (!slots[kind] || !tokenId || tokenId.length > 256) throw new Error('Invalid equipment NFT');
  return instruction(collection, { send_nft: { contract: terra(vault), token_id: tokenId, msg: toBase64(toUtf8(JSON.stringify({ slot: slots[kind] }))) } });
}
export function list(collection, tokenId, marketplace, token, price) {
  if (!tokenId || tokenId.length > 256) throw new Error('Invalid NFT ID');
  return instruction(collection, { send_nft: { contract: terra(marketplace), token_id: tokenId, msg: toBase64(toUtf8(JSON.stringify({ payment_token: terra(token), price: amount(price) }))) } });
}
export function fusion(config, ids, height) {
  if (!Array.isArray(ids) || ids.length !== 5 || new Set(ids).size !== 5 || ids.some(id => typeof id !== 'string' || !id || id.length > 256)) throw new Error('Select exactly five distinct commanders');
  const expires = { at_height: number(height) + 100 };
  return [...ids.map(token_id => instruction(config.contracts.commanderCollection, { approve: { spender: config.contracts.recruitment, token_id, expires } })),
    instruction(config.contracts.recruitment, { fuse: { token_ids: ids } })];
}
export function clanAction(config, action, values = {}) {
  if (action === 'create') {
    const name = String(values.name ?? '').trim(); const code = String(values.code ?? '').trim().toUpperCase();
    if (!name || new TextEncoder().encode(name).length > 64 || !/^[A-Z0-9_-]{1,12}$/.test(code)) throw new Error('Use a name up to 64 bytes and a 1–12 character clan code');
    return instruction(config.contracts.clans, { create: { name, code } });
  }
  if (action === 'join') return instruction(config.contracts.clans, { join: { code: String(values.code).trim().toUpperCase() } });
  if (['leave', 'cancel_change'].includes(action)) return instruction(config.contracts.clans, { [action]: {} });
  if (['transfer_lead', 'kick', 'dismantle'].includes(action)) return instruction(config.contracts.clans, { [action]: { clan_id: number(values.id),
    ...(action !== 'dismantle' ? { wallet: terra(values.wallet.trim()) } : {}) } });
  throw new Error('Invalid clan action');
}
