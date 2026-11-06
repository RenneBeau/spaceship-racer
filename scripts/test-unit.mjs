import { readdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
// These two suites require a generated deployment and a running local chain.
const integration = new Set(['manifest.test.mjs', 'http.test.mjs']);
const suites = readdirSync(new URL('../tests/onchain/', import.meta.url))
  .filter(name => name.endsWith('.test.mjs') && !integration.has(name))
  .sort().map(name => `tests/onchain/${name}`);
const result = spawnSync(process.execPath, ['--test', ...suites, 'contracts/local/keeper-plan.test.mjs'], { stdio: 'inherit' });
process.exit(result.status ?? 1);
