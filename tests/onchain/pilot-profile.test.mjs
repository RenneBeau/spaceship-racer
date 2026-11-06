import test from 'node:test';
import assert from 'node:assert/strict';
import { TxRaw, TxBody, AuthInfo } from 'cosmjs-types/cosmos/tx/v1beta1/tx';
import { PubKey } from 'cosmjs-types/cosmos/crypto/secp256k1/keys';
import { extractTransactions } from '../../src/onchain/indexer.mjs';
import { Store } from '../../src/onchain/store.mjs';

test('shared callsigns are bound to the confirmed transaction signer and rewind with chain history', () => {
  const wallet = 'terra1x46rqay4d3cssq8gxxvqz8xt6nwlz4td20k38v';
  const memo = `Spaceship Racer: buy racer | pilot:${Buffer.from('Nova').toString('base64')}`;
  const pubkey = PubKey.encode({ key: Buffer.from('AjszqFJDRAYbEjZMuiD+ChqzbUSGq/RRu3zr0R6iJB5b', 'base64') }).finish();
  const raw = TxRaw.encode({ bodyBytes: TxBody.encode(TxBody.fromPartial({ memo })).finish(),
    authInfoBytes: AuthInfo.encode(AuthInfo.fromPartial({ signerInfos: [{ publicKey: { typeUrl: '/cosmos.crypto.secp256k1.PubKey', value: pubkey } }] })).finish(), signatures: [] }).finish();
  const block = { block: { data: { txs: [Buffer.from(raw).toString('base64')] } } };
  const results = { txs_results: [{ code: 0, events: [{ type: 'wasm', attributes: [{ key: '_contract_address', value: 'game' }, { key: 'action', value: 'buy_ship' }] }] }] };
  const records = extractTransactions(block, results, new Set(['game']));
  assert.deepEqual(records[0].profile, { wallet, nickname: 'Nova' });
  const store = new Store(':memory:', 'pilot');
  try {
    store.commit({ height: 1, hash: 'A', time: '2026-11-01T00:00:00Z' }, records); assert.equal(store.profileName(wallet), 'Nova');
    store.rewind(0); assert.equal(store.profileName(wallet), null);
    results.txs_results[0].code = 5; assert.deepEqual(extractTransactions(block, results, new Set(['game'])), []);
  } finally { store.close(); }
});
