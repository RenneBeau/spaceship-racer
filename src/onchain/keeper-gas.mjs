import { decodeTxRaw } from '@cosmjs/proto-signing';
export async function keeperGas(chain, address) {
  const rows = [];
  for (let page = 1; page <= 5; page++) {
    const result = await chain.rpc('tx_search', { query: `message.sender='${address}'`, page: String(page), per_page: '20', order_by: 'desc', prove: false });
    rows.push(...(result.txs ?? [])); if (!result.txs?.length || result.txs.length < 20) break;
  }
  if (!rows.length) return { transactions: 0, gasUsed: '0', gasWanted: '0', fees: {}, windowSeconds: 0 };
  let used = 0n, wanted = 0n, failures = 0; const fees = {};
  for (const row of rows) {
    used += BigInt(row.tx_result.gas_used ?? 0); wanted += BigInt(row.tx_result.gas_wanted ?? 0);
    if (Number(row.tx_result.code ?? 0)) failures++;
    for (const fee of decodeTxRaw(Buffer.from(row.tx, 'base64')).authInfo.fee?.amount ?? []) fees[fee.denom] = (BigInt(fees[fee.denom] ?? 0) + BigInt(fee.amount)).toString();
  }
  const [first, last] = await Promise.all([chain.rpc('block', { height: rows.at(-1).height }), chain.rpc('block', { height: rows[0].height })]);
  const windowSeconds = (Date.parse(last.block.header.time) - Date.parse(first.block.header.time)) / 1000;
  return { address, transactions: rows.length, failures, gasUsed: used.toString(), gasWanted: wanted.toString(), averageGasUsed: (used / BigInt(rows.length)).toString(),
    averageGasWanted: (wanted / BigInt(rows.length)).toString(), fees, windowSeconds,
    estimatedFeesPerHour: Object.fromEntries(Object.entries(fees).map(([denom, amount]) => [denom, windowSeconds > 0 ? (BigInt(amount) * 3600000n / BigInt(Math.round(windowSeconds * 1000))).toString() : '0'])),
    note: 'Paid fee amounts, not USD prices. LocalTerra coins are test assets; hourly rates extrapolate this sample.' };
}
