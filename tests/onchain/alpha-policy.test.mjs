import test from 'node:test';
import assert from 'node:assert/strict';
import { DirectSecp256k1HdWallet } from '@cosmjs/proto-signing';
import { toBech32 } from '@cosmjs/encoding';
import { PubKey } from 'cosmjs-types/cosmos/crypto/secp256k1/keys';
import { TxBody, AuthInfo, TxRaw } from 'cosmjs-types/cosmos/tx/v1beta1/tx';
import { SimulateRequest } from 'cosmjs-types/cosmos/tx/v1beta1/service';
import { MsgExecuteContract } from 'cosmjs-types/cosmwasm/wasm/v1/tx';
import { validateAlphaTransaction, validateRpcRequest } from '../../src/alpha/transactions.mjs';
import { createFaucet } from '../../src/alpha/faucet.mjs';
import { rewriteHtml, rewriteCss } from '../../src/alpha/server.mjs';
const wallet = await DirectSecp256k1HdWallet.fromMnemonic('notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius', { prefix: 'terra' });
const [account] = await wallet.getAccounts(), contract = toBech32('terra', new Uint8Array(32).fill(7));
const policy = { contracts: new Set([contract]), protectedAddresses: new Set() };
function encoded(messages, overrides = {}) {
  const auth = { signerInfos: [{ publicKey: { typeUrl: '/cosmos.crypto.secp256k1.PubKey', value: PubKey.encode({ key: account.pubkey }).finish() }, modeInfo: { single: { mode: 1 } }, sequence: 0n }], fee: { amount: [{ denom: 'uluna', amount: '100' }], gasLimit: 500000n, payer: '', granter: '' }, ...overrides };
  return TxRaw.encode({ bodyBytes: TxBody.encode({ messages, memo: '', timeoutHeight: 0n, extensionOptions: [], nonCriticalExtensionOptions: [] }).finish(), authInfoBytes: AuthInfo.encode(auth).finish(), signatures: [new Uint8Array(64)] }).finish();
}
const action = sender => ({ typeUrl: '/cosmwasm.wasm.v1.MsgExecuteContract', value: MsgExecuteContract.encode({ sender, contract, msg: Buffer.from('{"claim":{}}'), funds: [] }).finish() });
test('public alpha accepts game transactions while protecting operators and blocking code upload/migration/authz', () => {
  assert.equal(validateAlphaTransaction(encoded([action(account.address)]), policy), account.address);
  assert.throws(() => validateAlphaTransaction(encoded([action(account.address)]), { ...policy, protectedAddresses: new Set([account.address]) }), /private infrastructure/);
  assert.throws(() => validateAlphaTransaction(encoded([action(toBech32('terra', new Uint8Array(20).fill(1)))]), policy), /Only alpha/);
  for (const typeUrl of ['/cosmwasm.wasm.v1.MsgStoreCode', '/cosmwasm.wasm.v1.MsgInstantiateContract', '/cosmwasm.wasm.v1.MsgMigrateContract', '/cosmos.authz.v1beta1.MsgExec']) assert.throws(() => validateAlphaTransaction(encoded([{ typeUrl, value: new Uint8Array() }]), policy), /not available/);
});
test('simulation cannot bypass transaction policy and RPC methods are restricted', () => {
  const txBytes = encoded([{ typeUrl: '/cosmwasm.wasm.v1.MsgStoreCode', value: new Uint8Array() }]);
  const data = Buffer.from(SimulateRequest.encode({ txBytes }).finish()).toString('hex');
  assert.throws(() => validateRpcRequest({ jsonrpc: '2.0', id: 1, method: 'abci_query', params: { path: '/cosmos.tx.v1beta1.Service/Simulate', data } }, policy), /not available/);
  assert.throws(() => validateRpcRequest({ jsonrpc: '2.0', id: 1, method: 'abci_query', params: { path: '/cosmos.tx.v1beta1.Service/BroadcastTx', data } }, policy), /validated broadcast/);
  assert.throws(() => validateRpcRequest({ jsonrpc: '2.0', id: 1, method: 'unsafe_flush_mempool', params: {} }, policy), /not exposed/);
  assert.doesNotThrow(() => validateRpcRequest({ jsonrpc: '2.0', id: 1, method: 'status' }, policy));
});
test('faucet funds arbitrary valid player wallets once per day and serializes signing', async () => {
  let active = 0, peak = 0, calls = 0;
  const faucet = createFaucet({ manifest: { chain_id: 'spaceship-racer-alpha', paymentTokens: ['usd', 'srt'], srt: 'srt' }, filename: ':memory:', protectedAddresses: new Set(), submit: async () => { active++; peak = Math.max(peak, active); await new Promise(resolve => setTimeout(resolve, 5)); active--; return { txhash: `HASH${++calls}` }; } });
  try {
    const a = toBech32('terra', new Uint8Array(20).fill(1)), b = toBech32('terra', new Uint8Array(20).fill(2));
    const results = await Promise.all([faucet.claim(a, '1.2.3.4'), faucet.claim(b, '1.2.3.4')]);
    assert.equal(results.length, 2); assert.equal(peak, 1); assert.equal(calls, 2);
    await assert.rejects(faucet.claim(a, '5.6.7.8'), /already received/);
  } finally { faucet.close(); }
});
test('alpha rewrites only site-root URLs and leaves external links intact', () => {
  assert.equal(rewriteHtml('<a href="/game"><img src="/assets/a.png"></a><a href="https://example.com">'), '<a href="/spaceship-racer/game"><img src="/spaceship-racer/assets/a.png"></a><a href="https://example.com">');
  assert.equal(rewriteCss('url(/assets/ui/a.webp)'), 'url(/spaceship-racer/assets/ui/a.webp)');
});
test('first-login funding is once per wallet even after a day, and concurrent logins cannot duplicate a grant', async () => {
  let calls = 0, clock = Date.now(), release;
  const wait = new Promise(resolve => { release = resolve; });
  const faucet = createFaucet({ manifest: { chain_id: 'spaceship-racer-alpha', paymentTokens: ['usd', 'srt'], srt: 'srt' }, filename: ':memory:', protectedAddresses: new Set(), now: () => clock,
    submit: async () => { calls++; await wait; return { txhash: 'STARTER' }; } });
  try {
    const address = toBech32('terra', new Uint8Array(20).fill(8));
    const first = faucet.claim(address, '1.2.3.4', { firstLogin: true });
    await assert.rejects(faucet.claim(address, '1.2.3.4', { firstLogin: true }), /pending confirmation/);
    release(); await first;
    clock += 2 * 86400000;
    const again = await faucet.claim(address, '5.6.7.8', { firstLogin: true });
    assert.equal(again.alreadyFunded, true); assert.equal(again.txhash, 'STARTER'); assert.equal(calls, 1);
  } finally { faucet.close(); }
});
