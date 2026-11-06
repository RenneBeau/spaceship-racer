import express from 'express';
import { readFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isIP } from 'node:net';
import { loadConfig, ROOT } from '../onchain/config.mjs';
import { createIndexer } from '../onchain/server.mjs';
import { createFaucet } from './faucet.mjs';
import { validateRpcRequest, validateAlphaTransaction, READ_RPC_METHODS, AlphaPolicyError } from './transactions.mjs';

export const BASE = '/spaceship-racer';
const ORIGIN = 'https://colapsar.space';
export function rewriteHtml(html) { return html.replace(/\b(href|src)="\/(?!\/)/g, `$1="${BASE}/`); }
export function rewriteCss(css) { return css.replaceAll('/assets/', `${BASE}/assets/`); }
async function relay(url, options, res) {
  const response = await fetch(url, { ...options, signal: AbortSignal.timeout(20000) });
  const reader = response.body.getReader(), chunks = []; let size = 0;
  for (;;) { const part = await reader.read(); if (part.done) break; size += part.value.length; if (size > 8 * 1024 * 1024) { await reader.cancel(); throw Error('Alpha response limit'); } chunks.push(part.value); }
  res.status(response.status).type(response.headers.get('content-type') ?? 'application/json').send(Buffer.concat(chunks));
}
function clientIp(req) {
  const candidate = req.get('x-alpha-client-ip') ?? req.socket.remoteAddress ?? '';
  if (!isIP(candidate)) return 'unknown';
  if (!candidate.includes(':')) return candidate;
  if (candidate.startsWith('::ffff:') && isIP(candidate.slice(7)) === 4) return candidate.slice(7);
  const [left = '', right = ''] = candidate.split('::');
  const first = left.split(':').filter(Boolean), last = right.split(':').filter(Boolean);
  return [...first, ...Array(Math.max(0, 8 - first.length - last.length)).fill('0'), ...last].slice(0, 4).map(part => Number.parseInt(part, 16).toString(16)).join(':');
}
export async function createAlphaServer({ manifest, config, faucet = null }) {
  if (manifest.chain_id !== 'spaceship-racer-alpha' || !manifest.alpha) throw Error('Public gateway requires an alpha deployment');
  const service = await createIndexer(config);
  Object.assign(service.publicConfig, { alpha: true, chainName: 'Spaceship Racer Alpha', basePath: BASE, faucet: `${BASE}/faucet`, portalWalletAccess: process.env.RACER_PORTAL_WALLET_ACCESS === 'true' });
  const policy = { protectedAddresses: new Set([manifest.owner, manifest.keeper_address, manifest.faucet_address]), contracts: new Set([...Object.values(service.publicConfig.contracts), ...manifest.paymentTokens, ...(manifest.dex_contracts ?? [])]) };
  const app = express(), rates = new Map(); let active = 0;
  app.disable('x-powered-by');
  app.enable('strict routing');
  app.use((req, res, next) => {
    res.set('X-Content-Type-Options', 'nosniff'); res.set('Referrer-Policy', 'no-referrer');
    res.set('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self' https://api.drand.sh; font-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'");
    res.set('X-Robots-Tag', 'noindex, nofollow');
    // A connected dashboard makes several reads per refresh; static artwork
    // must not consume the same budget as RPC and game actions.
    if (req.method === 'GET' && req.path.startsWith(`${BASE}/assets/`)) return next();
    const now = Date.now(), ip = clientIp(req), entry = rates.get(ip);
    if (!entry || now - entry.time > 60000) rates.set(ip, { time: now, count: 1 }); else entry.count++;
    if (rates.size > 10000) for (const [key, value] of rates) if (now - value.time > 60000) rates.delete(key);
    if ((rates.get(ip)?.count ?? 0) > 1200 || active >= 48) { res.set('Retry-After', '10'); return res.status(429).json({ error: 'Alpha gateway busy; retry shortly' }); }
    active++; let done = false; const finish = () => { if (!done) { active--; done = true; } }; res.once('finish', finish); res.once('close', finish); next();
  });
  app.use(express.json({ limit: '512kb' }));
  const handler = fn => (req, res, next) => Promise.resolve(fn(req, res)).catch(next);
  app.options([`${BASE}/rpc`, `${BASE}/lcd/{*path}`], (_req, res) => { res.set('Access-Control-Allow-Origin', '*'); res.set('Access-Control-Allow-Methods', 'GET, POST, OPTIONS'); res.set('Access-Control-Allow-Headers', 'Content-Type'); res.sendStatus(204); });
  app.post(`${BASE}/rpc`, handler(async (req, res) => {
    res.set('Access-Control-Allow-Origin', '*');
    const requests = Array.isArray(req.body) ? req.body : [req.body];
    if (!requests.length || requests.length > 20) throw new AlphaPolicyError('RPC batch exceeds alpha limit');
    requests.forEach(request => validateRpcRequest(request, policy));
    await relay(config.rpc, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(req.body) }, res);
  }));
  app.get(`${BASE}/rpc/:method`, handler(async (req, res) => {
    const method = req.params.method;
    if (!READ_RPC_METHODS.has(method)) throw new AlphaPolicyError('RPC method is not publicly exposed');
    res.set('Access-Control-Allow-Origin', '*'); await relay(`${config.rpc}/${method}?${new URLSearchParams(req.query).toString()}`, {}, res);
  }));
  app.use(`${BASE}/lcd`, handler(async (req, res) => {
    res.set('Access-Control-Allow-Origin', '*');
    if (!/^\/(cosmos|cosmwasm|terra|ibc)\//.test(req.path) || req.url.includes('..')) throw new AlphaPolicyError('Unsupported LCD route');
    if (req.method === 'GET') { await relay(`${config.lcd}${req.url}`, {}, res); return; }
    if (req.method !== 'POST') throw new AlphaPolicyError('Unsupported LCD method');
    if (req.path === '/cosmos/tx/v1beta1/txs') {
      validateAlphaTransaction(Buffer.from(req.body?.tx_bytes ?? '', 'base64'), policy);
      if (!['BROADCAST_MODE_SYNC', 'BROADCAST_MODE_ASYNC'].includes(req.body.mode)) throw new AlphaPolicyError('Use synchronous transaction broadcasting');
    } else if (req.path === '/cosmos/tx/v1beta1/simulate') {
      if (req.body?.tx_bytes) validateAlphaTransaction(Buffer.from(req.body.tx_bytes, 'base64'), policy, true);
      else throw new AlphaPolicyError('Base64 simulation transaction required');
    } else throw new AlphaPolicyError('LCD write endpoint is not available');
    await relay(`${config.lcd}${req.url}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(req.body) }, res);
  }));
  app.get(`${BASE}/faucet`, (_req, res) => res.json({ enabled: !!faucet, chainId: manifest.chain_id, kit: faucet?.kit, notice: 'Test assets have no monetary value. This is not the release presale.' }));
  app.post(`${BASE}/faucet/first-login`, handler(async (req, res) => {
    if (req.get('origin') !== ORIGIN || !faucet) throw new AlphaPolicyError('Use the alpha page to request test funds');
    if (Object.keys(req.body ?? {}).join(',') !== 'address') throw new AlphaPolicyError('Only a recipient address is accepted');
    res.set('Cache-Control', 'no-store'); res.json(await faucet.claim(req.body.address, clientIp(req), { firstLogin: true }));
  }));
  app.post(`${BASE}/faucet`, handler(async (req, res) => {
    if (req.get('origin') !== ORIGIN || !faucet) throw new AlphaPolicyError('Use the alpha page to request test funds');
    if (Object.keys(req.body ?? {}).join(',') !== 'address') throw new AlphaPolicyError('Only a recipient address is accepted');
    res.set('Cache-Control', 'no-store'); res.json(await faucet.claim(req.body.address, clientIp(req)));
  }));
  app.get(`${BASE}/assets/app.css`, (_req, res) => res.type('text/css').send(rewriteCss(readFileSync(resolve(ROOT, 'front/onchain/dist/app.css'), 'utf8'))));
  app.get(`${BASE}/architecture`, (_req, res) => res.redirect(302, `${BASE}/about`));
  app.get(`${BASE}/architecture/source`, (_req, res) => res.type('text/plain').send(`# Spaceship Racer funded alpha\n\nChain: ${manifest.chain_id}\nAll assets are test assets with no monetary value. The release presale is not active.\n\n${Object.entries(service.publicConfig.contracts).map(([role, address]) => `${role}: ${address}`).join('\n')}`));
  app.get(`${BASE}/about`, (_req, res) => res.type('html').send(`<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Spaceship Racer Alpha</title><link rel="stylesheet" href="${BASE}/assets/app.css"></head><body><main class="shell"><h1>Spaceship Racer Alpha</h1><p>A funded playtest on a dedicated local Terra testnet. Test assets have no monetary value and are not redeemable.</p><p>Connect Keplr or Leap, request a test kit, then buy ships in the hangar. Your fleet joins the next race automatically.</p><p>Ship upgrades, boosts, combat, clans, rewards, commander draws, fusion, skins and trading are available for testing.</p><p>The release presale is separate from alpha and has not started.</p><p><a href="${BASE}/">Return to the game</a> · <a href="${BASE}/architecture/source">Alpha contract addresses</a></p></main></body></html>`));
  const pages = ['/', '/game', '/shop', '/hangar', '/inventory', '/commander-recruitment', '/marketplace', '/clans', '/profile', '/treasury', '/activity'];
  app.get(BASE, (_req, res) => res.redirect(302, `${BASE}/`));
  app.use(BASE, (req, res, next) => {
    if (req.method === 'GET' && (pages.includes(req.path) || /^\/clans\/\d+$/.test(req.path))) return res.type('html').send(rewriteHtml(readFileSync(resolve(ROOT, 'front/onchain/index.html'), 'utf8')));
    next();
  });
  app.use(BASE, service.app);
  app.use((error, _req, res, _next) => res.status(error.status ?? 503).json({ error: error.status ? error.message : 'Alpha service temporarily unavailable' }));
  return { app, service, policy };
}
export async function startAlphaServer() {
  const filename = process.env.RACER_MANIFEST ?? resolve(ROOT, 'contracts/local/alpha-deployment.json');
  const manifest = JSON.parse(readFileSync(filename));
  const config = loadConfig({ ...process.env, RACER_MANIFEST: filename, RACER_PUBLIC_RPC: `${ORIGIN}${BASE}/rpc`, RACER_PUBLIC_LCD: `${ORIGIN}${BASE}/lcd` });
  mkdirSync(resolve(ROOT, 'data'), { recursive: true });
  const faucet = createFaucet({ manifest, filename: resolve(ROOT, 'data/alpha-faucet.sqlite'), protectedAddresses: new Set([manifest.owner, manifest.keeper_address, manifest.faucet_address]) });
  const alpha = await createAlphaServer({ manifest, config, faucet });
  alpha.service.indexer.start(); alpha.service.rewardHistory.start();
  const server = alpha.app.listen(5110, '127.0.0.1', () => console.log(`Spaceship Racer alpha: http://127.0.0.1:5110${BASE}/`));
  const stop = () => server.close(async () => { await alpha.service.close(); faucet.close(); process.exit(0); });
  process.on('SIGTERM', stop); process.on('SIGINT', stop);
  return alpha;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await startAlphaServer();
