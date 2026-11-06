import { defineConfig } from '@playwright/test';
const status = await (await fetch(`${process.env.RACER_RPC ?? 'http://127.0.0.1:27657'}/status`)).json();
const height = Number(status.result.sync_info.latest_block_height);
export default defineConfig({ testDir: '.', testMatch: '*.spec.mjs', workers: 1, timeout: 90000,
  use: { baseURL: 'http://127.0.0.1:8791', headless: true, launchOptions: { args: ['--no-sandbox'] }, screenshot: 'only-on-failure' },
  webServer: { command: 'npm run build:onchain && npm run start:onchain', url: 'http://127.0.0.1:8791/api/health', timeout: 60000,
    env: { RACER_PORT: '8791', RACER_DATABASE: ':memory:', RACER_START_HEIGHT: String(Math.max(1, height - 5)) }, reuseExistingServer: false } });
