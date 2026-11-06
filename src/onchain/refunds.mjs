// cw-storage-plus keys for the existing deployed race contract. Keep these
// scoped to this contract version; no execute/admin privileges are involved.
const text = value => new TextEncoder().encode(value);
function namespace(bytes) { return [bytes.length >> 8, bytes.length & 255, ...bytes]; }
export function refundKey(wallet) { return new Uint8Array([...namespace(text('refunds')), ...text(wallet)]); }
export function boostKey(race, tick, wallet) {
  const a = new Uint8Array(8); new DataView(a.buffer).setBigUint64(0, BigInt(race));
  const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, tick);
  return new Uint8Array([...namespace(text('boosts')), ...namespace(a), ...namespace(b), ...text(wallet)]);
}
export function groupRefundTicks(entries) {
  const races = new Map();
  for (const { race, tick } of entries) {
    if (!races.has(race)) races.set(race, new Set());
    races.get(race).add(tick);
  }
  return [...races].sort(([a], [b]) => a - b).flatMap(([race, ticks]) => {
    const sorted = [...ticks].sort((a, b) => a - b); const chunks = [];
    for (let i = 0; i < sorted.length; i += 20) chunks.push({ race, ticks: sorted.slice(i, i + 20) });
    return chunks;
  });
}
