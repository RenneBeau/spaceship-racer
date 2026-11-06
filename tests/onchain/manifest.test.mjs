import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { loadConfig, address } from '../../src/onchain/config.mjs';

const manifest = JSON.parse(readFileSync(new URL('../../contracts/local/deployment.json', import.meta.url), 'utf8'));
test('local deployment manifest is internally consistent and indexer-ready', () => {
  assert.equal(manifest.chain_id, 'spaceship-racer-local');
  for (const key of ['game', 'fleet', 'equipment', 'rewards', 'clans', 'payments', 'capitalTreasury', 'marketplace', 'recruitment', 'skins', 'skinCollection', 'randomness', 'pause', 'srt', 'spaceusd', 'reward_pool']) address(manifest[key]);
  assert.equal(manifest.reward_pool, manifest.rewards);
  address(manifest.collapsar);
  assert.equal(manifest.paymentTokens.length, 5);
  assert.equal(new Set(manifest.paymentTokens).size, 5);
  assert.ok(manifest.paymentTokens.includes(manifest.srt));
  assert.ok(manifest.paymentTokens.includes(manifest.spaceusd));
  assert.deepEqual(Object.values(manifest.tokens).sort(), [...manifest.paymentTokens].sort());
  for (const value of [manifest.srt, manifest.spaceusd, ...manifest.paymentTokens]) address(value);
  assert.equal(manifest.indexer.rpc, 'http://127.0.0.1:27657');
  assert.equal(manifest.indexer.lcd, 'http://127.0.0.1:1417');
  assert.equal(manifest.indexer.port, 8787);
  const config = loadConfig({ RACER_MANIFEST: new URL('../../contracts/local/deployment.json', import.meta.url).pathname, RACER_PORT: '8799' });
  assert.equal(config.contracts.rewards, manifest.reward_pool);
  assert.equal(config.contracts.capitalTreasury, manifest.capitalTreasury);
  assert.equal(config.chainId, manifest.chain_id);
});
