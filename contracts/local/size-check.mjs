import { statSync } from 'node:fs';
import assert from 'node:assert/strict';
const localLimit = 819200; // Verified by the pinned LocalTerra StoreCode rejection.
const contracts = ['race', 'reinvestment', 'beacon_verifier', 'skins', 'clans', 'payments', 'treasury', 'rewards', 'fleet', 'equipment', 'collection', 'randomness', 'marketplace', 'recruitment', 'pause'];
const sizes = process.argv.includes('--dex-only') ? {} : Object.fromEntries(contracts.map(name => {
  const bytes = statSync(new URL(`../target/wasm32-unknown-unknown/release/racer_${name}.wasm`, import.meta.url)).size;
  assert.ok(bytes <= localLimit, `${name}: ${bytes} bytes exceeds verified local limit ${localLimit}`);
  return [name, bytes];
}));
const dexBytes = {};
if (process.argv.includes('--dex-only') || process.argv.includes('--with-dex')) {
  for (const name of ['factory', 'router', 'pair']) {
    const base = `../../vendor/cl8y-dex-terraclassic/smartcontracts/target/wasm32-unknown-unknown/release/cl8y_dex_${name}`;
    const raw = statSync(new URL(`${base}.wasm`, import.meta.url)).size;
    const optimized = statSync(new URL(`${base}.optimized.wasm`, import.meta.url)).size;
    assert.ok(optimized <= localLimit, `CL8Y ${name}: ${optimized} exceeds local limit ${localLimit}`);
    dexBytes[name] = { raw, optimized };
  }
}
console.log(JSON.stringify({ verifiedLocalWasmLimit: localLimit, artifactBytes: sizes, dexBytes }, null, 2));
