import test from 'node:test';
import assert from 'node:assert/strict';
import { boostKey, refundKey, groupRefundTicks } from '../../src/onchain/refunds.mjs';
import { Store } from '../../src/onchain/store.mjs';
import { Wallet } from '../../front/onchain/wallet.mjs';
import { Chain } from '../../src/onchain/chain.mjs';
import { QueryRawContractStateRequest, QueryRawContractStateResponse } from 'cosmjs-types/cosmwasm/wasm/v1/query';

test('refund discovery keys match cw-storage-plus tuple namespace encoding', () => {
  assert.equal(Buffer.from(refundKey('pilot')).toString('hex'), '0007726566756e647370696c6f74');
  assert.equal(Buffer.from(boostKey(258, 513, 'pilot')).toString('hex'), '0006626f6f7374730008000000000000010200040000020170696c6f74');
});
test('binary raw keys use pinned protobuf RPC rather than slash-sensitive LCD paths', async () => {
  const chain = new Chain({}); const key = new Uint8Array([255, 255, 63, 0, 1]);
  chain.rpc = async (method, params) => {
    assert.equal(method, 'abci_query'); assert.equal(params.height, '10');
    const request = QueryRawContractStateRequest.decode(Buffer.from(params.data, 'hex'));
    assert.equal(request.address, 'contract'); assert.deepEqual([...request.queryData], [...key]);
    return { response: { height: '10', code: 0, value: Buffer.from(QueryRawContractStateResponse.encode({ data: Buffer.from('{"paid":"1000000"}') }).finish()).toString('base64') } };
  };
  assert.deepEqual(await chain.raw('contract', key, 10), { paid: '1000000' });
});
test('all unused ticks are deduplicated and batched by race into twenty-tick messages', () => {
  const entries = Array.from({ length: 45 }, (_, i) => ({ race: 2, tick: i + 3 }));
  const groups = groupRefundTicks([...entries, entries[0], { race: 1, tick: 9 }]);
  assert.deepEqual(groups.map(g => [g.race, g.ticks.length]), [[1, 1], [2, 20], [2, 20], [2, 5]]);
  assert.deepEqual(groups.filter(g => g.race === 2).flatMap(g => g.ticks), entries.map(e => e.tick));
});
test('gameplay activity hides routine proofs but retains race results and boost discovery', () => {
  const store = new Store(':memory:', 'refund-test');
  try {
    const events = [
      { action: 'submit_randomness', attributes: [] },
      { action: 'execute', attributes: [] },
      { action: 'advance_race', attributes: [] },
      { action: 'advance_race', attributes: [{ key: 'race_ended', value: '1' }] },
      { action: 'queue_block_boost', attributes: Object.entries({ wallet: 'pilot', race: '1', tick: '8', paid: '1000000' }).map(([key, value]) => ({ key, value })) },
    ].map((e, index) => ({ ...e, index, contract: 'game' }));
    store.commit({ height: 1, hash: 'A', time: '2026-11-01T00:00:00Z' }, [{ hash: 'TX', index: 0, participants: ['pilot'], events }]);
    assert.equal(store.events().length, 5);
    assert.deepEqual(store.events({ gameplay: true }).map(e => e.action), ['queue_block_boost', 'advance_race']);
    assert.deepEqual(store.boostEntries('pilot', 'game'), [{ race: 1, tick: 8 }]);
    assert.deepEqual(store.boostEntries('someone-else', 'game'), []);
  } finally { store.close(); }
});

test('recover all verifies raw balances and submits all race chunks in one wallet transaction', async t => {
  const entries = [...Array.from({ length: 45 }, (_, i) => ({ race: 1, tick: i + 3 })), { race: 2, tick: 9 }];
  t.mock.method(globalThis, 'fetch', async () => Response.json({ claims: groupRefundTicks(entries) }));
  const game = 'terra1x46rqay4d3cssq8gxxvqz8xt6nwlz4td20k38v';
  const wallet = new Wallet({ contracts: { game } }); wallet.address = 'pilot';
  const raw = new Map(entries.map(e => [Buffer.from(boostKey(e.race, e.tick, 'pilot')).toString('hex'), { paid: '1000000' }]));
  raw.set(Buffer.from(refundKey('pilot')).toString('hex'), '7000000');
  wallet.reader = async () => ({ queryContractRaw: async (_contract, key) => {
    const value = raw.get(Buffer.from(key).toString('hex'));
    return value === undefined ? null : new TextEncoder().encode(JSON.stringify(value));
  } });
  wallet.query = async () => ({ ticks: 2 });
  let calls = 0;
  wallet.execute = async instructions => { calls++; return instructions; };
  const instructions = await wallet.recoverBoosts();
  assert.equal(calls, 1); assert.equal(instructions.length, 4);
  assert.ok(instructions.every(i => i.contractAddress === game));
  assert.deepEqual(instructions.map(i => i.msg.claim_refunds.ticks.length), [20, 20, 5, 1]);
});
test('legacy and new-engine refunds stay in one transaction and outside contracts are rejected', async t => {
  const game = 'terra1x46rqay4d3cssq8gxxvqz8xt6nwlz4td20k38v', legacy = 'terra1hcm3sjdzelk35z322thy2qvr429v80v0ktunk7';
  let claims = [{ contract: game, race: 2, ticks: [4] }, { contract: legacy, race: 1, ticks: [3] }];
  t.mock.method(globalThis, 'fetch', async () => Response.json({ claims }));
  const wallet = new Wallet({ contracts: { game }, previousGames: [legacy] }); wallet.address = 'pilot';
  wallet.reader = async () => ({ queryContractRaw: async (_contract, key) => {
    const value = Buffer.from(key).equals(Buffer.from(refundKey('pilot'))) ? '0' : { paid: '1000000' };
    return Buffer.from(JSON.stringify(value));
  } });
  wallet.query = async () => ({ ticks: 1 });
  let calls = 0; wallet.execute = async messages => { calls++; return messages; };
  const messages = await wallet.recoverBoosts();
  assert.equal(calls, 1); assert.deepEqual(messages.map(m => m.contractAddress), [game, legacy]);
  claims = [{ contract: 'outside', race: 1, ticks: [2] }];
  await assert.rejects(() => wallet.recoverBoosts(), /outside this deployment/);
});
