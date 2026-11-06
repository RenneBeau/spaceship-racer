import test from 'node:test';
import assert from 'node:assert/strict';
import { TxRaw, TxBody, AuthInfo } from 'cosmjs-types/cosmos/tx/v1beta1/tx';
import { keeperGas } from '../../src/onchain/keeper-gas.mjs';
function transaction(fee) {
  return Buffer.from(TxRaw.encode({ bodyBytes: TxBody.encode(TxBody.fromPartial({ memo: 'keeper' })).finish(),
    authInfoBytes: AuthInfo.encode(AuthInfo.fromPartial({ signerInfos: [], fee: { amount: [{ denom: 'uusd', amount: fee }], gasLimit: 2000000n } })).finish(), signatures: [] }).finish()).toString('base64');
}
test('keeper report distinguishes actual execution gas, gas limits, and paid fees including failures', async () => {
  const chain = { rpc: async (method, params) => {
    if (method === 'tx_search') return { txs: [
      { height: '11', tx: transaction('1000000'), tx_result: { gas_used: '1000000', gas_wanted: '2000000', code: 0 } },
      { height: '10', tx: transaction('2000000'), tx_result: { gas_used: '3000000', gas_wanted: '2000000', code: 11 } },
    ] };
    return { block: { header: { time: params.height === '10' ? '2026-11-01T00:00:00Z' : '2026-11-01T00:10:00Z' } } };
  } };
  const report = await keeperGas(chain, 'keeper');
  assert.equal(report.transactions, 2); assert.equal(report.failures, 1);
  assert.equal(report.gasUsed, '4000000'); assert.equal(report.gasWanted, '4000000'); assert.equal(report.averageGasUsed, '2000000');
  assert.equal(report.fees.uusd, '3000000'); assert.equal(report.windowSeconds, 600); assert.equal(report.estimatedFeesPerHour.uusd, '18000000');
});
