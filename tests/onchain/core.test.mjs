import test from 'node:test';
import assert from 'node:assert/strict';
import { toBech32, fromBase64, fromUtf8 } from '@cosmjs/encoding';
import { Store } from '../../src/onchain/store.mjs';
import { Indexer, extractTransactions } from '../../src/onchain/indexer.mjs';
import * as tx from '../../front/onchain/transactions.mjs';

const wallet = toBech32('terra', new Uint8Array(20).fill(1));
const contract = toBech32('terra', new Uint8Array(32).fill(2));
const token = toBech32('terra', new Uint8Array(32).fill(3));
const other = toBech32('terra', new Uint8Array(20).fill(4));
const hash = 'A'.repeat(64);
function block(height, hash = String(height), parent = String(height - 1)) { return { block_id: { hash }, block: { header: { height: String(height), chain_id: 'test', time: '2026-01-01T00:00:00Z', last_block_id: { hash: parent } }, data: { txs: [Buffer.from(`tx${height}`).toString('base64')] } } }; }
const raw = [{ type: 'message', attributes: [{ key: 'sender', value: wallet }] }, { type: 'wasm', attributes: [{ key: '_contract_address', value: contract }, { key: 'action', value: 'queue_bind' }, { key: 'wallet', value: wallet }] }];

test('plain and legacy base64 events index identically; failed transactions do not', () => {
  const a = extractTransactions(block(1), { txs_results: [{ code: 0, events: raw }] }, new Set([contract]));
  const encoded = raw.map(e => ({ ...e, attributes: e.attributes.map(a => ({ key: Buffer.from(a.key).toString('base64'), value: Buffer.from(a.value).toString('base64') })) }));
  const b = extractTransactions(block(1), { txs_results: [{ code: 0, events: encoded }] }, new Set([contract]));
  assert.deepEqual(a, b); assert.deepEqual(a[0].participants, [wallet]); assert.equal(a[0].events[0].action, 'queue_bind');
  assert.deepEqual(extractTransactions(block(1), { txs_results: [{ code: 5, events: raw }] }, new Set([contract])), []);
  assert.deepEqual(extractTransactions(block(1), { txs_results: [{ code: 0, events: raw }] }, new Set([token])), []);
});
test('event persistence is atomic and rewind removes transaction/address history', () => {
  const store = new Store(':memory:', 'test');
  const transactions = extractTransactions(block(1), { txs_results: [{ code: 0, events: raw }] }, new Set([contract]));
  store.commit({ height: 1, hash, time: 'now' }, transactions);
  assert.equal(store.events({ wallet }).length, 1); assert.equal(store.events({ wallet: other }).length, 0);
  assert.equal(store.referralEarnings(other, contract), '0');
  assert.throws(() => store.commit({ height: 2, hash, time: 'later' }, transactions));
  assert.equal(store.head().height, 1); assert.equal(store.count(), 1);
  store.rewind(0); assert.equal(store.head(), null); assert.equal(store.events({ wallet }).length, 0); store.close();
});
test('sponsor is a participant and referral payouts are aggregated per sponsor', () => {
  const store = new Store(':memory:', 'test');
  const events = [{ type: 'wasm', attributes: [
    { key: '_contract_address', value: contract }, { key: 'action', value: 'buy_ship' },
    { key: 'wallet', value: wallet }, { key: 'sponsor', value: other }, { key: 'referral', value: '47500' },
  ] }];
  const transactions = extractTransactions(block(1), { txs_results: [{ code: 0, events }] }, new Set([contract]));
  assert.ok(transactions[0].participants.includes(other));
  store.commit({ height: 1, hash, time: 'now' }, transactions);
  assert.equal(store.referralEarnings(other, contract), '47500');
  assert.equal(store.referralEarnings(wallet, contract), '0');
  store.close();
});
test('indexer rewinds a changed canonical head before replaying it', async () => {
  let fork = false;
  const chain = { config: { chainId: 'test' }, cache: new Map(), tip: async () => ({ height: 2 }), rpc: async (method, { height }) =>
    method === 'block' ? block(Number(height), Number(height) === 2 && fork ? 'fork2' : String(height)) : { height, txs_results: [] } };
  const store = new Store(':memory:', 'test'); const indexer = new Indexer(chain, store, new Set(), 1);
  await indexer.syncOnce(); assert.equal(store.head().hash, '2');
  fork = true; await indexer.syncOnce(); assert.equal(store.head().height, 1);
  await indexer.syncOnce(); assert.equal(store.head().hash, 'fork2'); store.close();
});
test('amount conversion preserves Uint128 precision and rejects rounding/overflow', () => {
  const max = ((1n << 128n) - 1n).toString();
  assert.equal(tx.units(tx.display(max, 18), 18), max);
  assert.equal(tx.units('0.0002', 6), '200');
  for (const value of ['1e6', '-1', 'NaN', '0.0000001']) assert.throws(() => tx.units(value, 6));
  assert.throws(() => tx.units(((1n << 128n)).toString(), 0));
});
test('ship conversion binds exact invoice and directs surplus through the contract', () => {
  const config = { spaceusd: token, contracts: { payments: contract } };
  const wanted = tx.purchase('buy', 'racer', '1');
  const instruction = tx.shipPayment(config, other, wanted, { price: '1000000', estimated_offer: '2000000', route: [other, token] }, 200, 1800000000);
  assert.equal(instruction.msg.send.amount, '2040000'); assert.equal(instruction.msg.send.contract, contract);
  assert.equal(instruction.contractAddress, other);
  const hook = JSON.parse(fromUtf8(fromBase64(instruction.msg.send.msg)));
  assert.equal(hook.expected_price, '1000000'); assert.equal(hook.minimum_receive, '1000000'); assert.equal(hook.max_spread, '0.02');
  assert.equal(hook.wallet, undefined);
  assert.equal(tx.shipPayment(config, token, wanted, { price: '1000000', estimated_offer: '1000000', route: [token] }, 200, 1800000000).msg.send.amount, '1000000');
});
test('fusion uses five scoped approvals and one atomic fuse message', () => {
  const config = { contracts: { commanderCollection: contract, recruitment: token } };
  const ids = ['a', 'b', 'c', 'd', 'e']; const messages = tx.fusion(config, ids, 100);
  assert.equal(messages.length, 6); assert.deepEqual(messages[0].msg.approve, { spender: token, token_id: 'a', expires: { at_height: 200 } });
  assert.deepEqual(messages[5].msg, { fuse: { token_ids: ids } });
  assert.throws(() => tx.fusion(config, ['a', 'a', 'b', 'c', 'd'], 100));
  assert.throws(() => tx.fusion(config, ids.slice(1), 100));
});
test('large boost purchases preserve quantity and exact SRT payment without a ten-action cap', () => {
  const config = { srt: token, contracts: { game: contract } };
  for (const kind of ['speed', 'acceleration', 'temporary', 'rocket']) {
    const message = tx.boost(config, kind, 1000, 9, '20990000').msg.send;
    assert.equal(message.amount, '20990000000');
    assert.deepEqual(JSON.parse(fromUtf8(fromBase64(message.msg))), { race: 9, kind, count: 1000 });
  }
  assert.throws(() => tx.boost(config, 'rocket', 0, 9, '20990000'));
  assert.throws(() => tx.boost(config, 'rocket', 0x100000000, 9, '20990000'));
});
