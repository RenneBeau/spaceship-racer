import express from 'express';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { loadConfig, address, integer, ROOT } from './config.mjs';
import { Chain } from './chain.mjs';
import { Store } from './store.mjs';
import { Indexer } from './indexer.mjs';
import { boostKey, refundKey, groupRefundTicks } from './refunds.mjs';
import { processedMovementTick } from '../../front/onchain/telemetry.mjs';
import { buildLeaderboards } from './leaderboards.mjs';
import { ClanRewardHistory } from './clan-rewards.mjs';
import { keeperGas } from './keeper-gas.mjs';
import { createRacePreview } from './race-preview.mjs';
import { assessChainHealth, assessGameHealth, raceElapsedByHeight } from './health.mjs';
import { cycleEstimate } from './cycle-estimate.mjs';
import { rewardAllocation, clanCycleKey } from './reward-allocation.mjs';

const SHIPS = ['racer', 'tank', 'fighter'];
const SLOTS = ['commander', 'racer_skin', 'tank_skin', 'fighter_skin'];
const asyncRoute = fn => (req, res, next) => Promise.resolve(fn(req, res)).catch(next);

export async function createIndexer(config = loadConfig()) {
  const chain = new Chain(config);
  const tip = await chain.pinnedTip(); const height = tip.height; const c = config.contracts;
  const [gameConfig, fleetConfig, rewardConfig, recruitConfig, skinConfig, paymentConfig] = await Promise.all([
    chain.smart(c.game, { config: {} }, height), chain.smart(c.fleet, { config: {} }, height), chain.smart(c.rewards, { state: {} }, height),
    chain.smart(c.recruitment, { config: {} }, height), chain.smart(c.skins, { config: {} }, height), chain.smart(c.payments, { config: {} }, height),
  ]);
  for (const [actual, expected] of [[gameConfig.fleet, c.fleet], [gameConfig.equipment, c.equipment], [gameConfig.rewards, c.rewards],
    [gameConfig.randomness, c.randomness], [fleetConfig.engine, c.game], [rewardConfig.allocator, c.game], [skinConfig.fleet, c.fleet],
    [skinConfig.collection, c.skinCollection], [paymentConfig.fleet, c.fleet], [paymentConfig.spaceusd, rewardConfig.spaceusd], [recruitConfig.srt, gameConfig.srt],
    [fleetConfig.payment_adapter, c.payments]]) {
    if (actual !== expected) throw new Error('Deployment wiring does not match chain contracts');
  }
  const contracts = { ...c, commanderCollection: address(recruitConfig.collection) };
  const watched = new Set([...Object.values(contracts), ...config.tokens]);
  const infos = await Promise.all([...watched].map(async addr => {
    const response = await chain.lcd(`/cosmwasm/wasm/v1/contract/${addr}`, height);
    return Number(response.contract_info.created.block_height);
  }));
  if (infos.some(h => !Number.isSafeInteger(h) || h < 1)) throw new Error('Invalid contract creation height');
  const startHeight = config.startHeight ?? Math.min(...infos);
  const genesis = config.chainId.startsWith('spaceship-racer-') ? (await chain.rpc('block', { height: '1' })).block_id.hash : config.chainId;
  const store = new Store(config.database, `${config.fingerprint}:${genesis}`);
  if (store.meta('start_height') && Number(store.meta('start_height')) !== startHeight) { store.close(); throw new Error('Start height changed; use a new index database'); }
  store.setMeta('start_height', startHeight);
  const indexer = new Indexer(chain, store, watched, startHeight);
  const previews = createRacePreview(chain, store, c.game);
  const rewardHistory = new ClanRewardHistory(chain, store, { ...config, historyStartHeight: Math.min(...infos) });
  const tokens = await Promise.all(config.tokens.map(async token => {
    const info = await chain.smart(token, { token_info: {} }, height);
    if (!Number.isInteger(info.decimals) || info.decimals < 0 || info.decimals > 18) throw new Error('Unsupported token decimals');
    return { address: token, name: info.name, symbol: token === gameConfig.srt ? 'SRT' : token === rewardConfig.spaceusd ? 'SpaceUSD' : info.symbol, decimals: info.decimals };
  }));
  const publicConfig = { chainId: config.chainId, alpha: config.alpha, chainName: config.alpha ? 'Spaceship Racer Alpha' : config.chainId === 'spaceship-racer-local' ? 'Spaceship Racer LocalTerra' : 'Terra Classic',
    rpc: config.publicRpc, lcd: config.publicLcd, prefix: 'terra', coinType: 330, feeDenom: 'uluna', feeDecimals: 6, gasPrice: config.gasPrice,
    contracts, tokens, srt: gameConfig.srt, spaceusd: rewardConfig.spaceusd, timingVersion: gameConfig.timing_version ?? 1,
    previousGames: config.previousGames ?? [], deployment: config.fingerprint };
  const gameContracts = [c.game, ...(config.previousGames ?? [])];
  async function resultFor(s, race) {
    for (const contract of gameContracts) {
      const result = await s.smart(contract, { result: { race } }, true);
      if (result) return result;
    }
    return null;
  }

  async function snapshot() {
    const at = await chain.pinnedTip();
    return { at, smart: (contract, message, optional = false) => chain.smart(contract, message, at.height, optional) };
  }
  let boardCache = null;
  async function boards(s) {
    if (boardCache?.height === s.at.height) return boardCache.promise;
    const promise = (async () => {
      const state = await s.smart(c.game, { state: {} }), wallets = []; let after = null;
      const forecast = await previews.get(s, state, await s.smart(c.game, { config: {} }));
      for (let page = 0; page < 51; page++) {
        const items = await s.smart(c.fleet, { players: { start_after: after, limit: 100 } });
        wallets.push(...items); if (!items.length) break; after = items.at(-1);
      }
      const players = await Promise.all(wallets.map(async wallet => {
        let player; try { player = forecast?.players.get(wallet) ?? await s.smart(c.game, { player: { wallet } }, true); }
        catch (error) { if (!/player no longer/.test(error.message)) throw error; player = null; }
        const [score, fleet] = await Promise.all([s.smart(c.game, { cycle_player: { cycle: state.cycle, wallet } }), s.smart(c.fleet, { fleet: { wallet } })]);
        return { wallet, nickname: store.profileName(wallet), player, score, fleet };
      }));
      const ids = [...new Set(players.map(p => p.score.clan).filter(id => id !== null))];
      const clans = await Promise.all(ids.map(async id => {
        const clan = await s.smart(c.clans, { clan: { id } });
        const snapshot = await chain.raw(c.game, clanCycleKey(state.cycle, id), s.at.height);
        return { ...clan, leader: snapshot?.leader ?? clan.leader };
      }));
      return { state, players, clans, preview: !!forecast, previewTick: forecast?.throughTick ?? null, rankings: buildLeaderboards(state, players, clans) };
    })();
    boardCache = { height: s.at.height, promise };
    try { return await promise; } catch (error) { if (boardCache?.promise === promise) boardCache = null; throw error; }
  }
  async function inventory(wallet, collection, startAfter, limit, s) {
    const role = collection === 'commanders' ? 'commanderCollection' : collection === 'skins' ? 'skinCollection' : null;
    if (!role) throw Object.assign(new Error('Unknown NFT collection'), { status: 400 });
    if (startAfter && (typeof startAfter !== 'string' || startAfter.length > 256)) throw Object.assign(new Error('Invalid NFT cursor'), { status: 400 });
    const contract = contracts[role];
    const result = await s.smart(contract, { tokens: { owner: wallet, start_after: startAfter ?? null, limit } });
    const items = await Promise.all(result.tokens.map(async tokenId => ({ tokenId, collection: contract,
      ...(await s.smart(contract, { nft_info: { token_id: tokenId } })) })));
    return { items, next: result.tokens.length === limit ? result.tokens.at(-1) : null };
  }

  const app = express(); app.disable('x-powered-by');
  const origins = [config.publicRpc, config.publicLcd].map(url => {
    const parsed = new URL(url);
    if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password) throw new Error('Public chain endpoints cannot contain credentials');
    return parsed.origin;
  });
  let activeApi = 0;
  app.use((req, res, next) => {
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Referrer-Policy', 'no-referrer');
    res.setHeader('Content-Security-Policy', `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self' ${origins.join(' ')} https://api.drand.sh; base-uri 'none'; object-src 'none'; frame-ancestors 'none'`);
    if (req.path.startsWith('/api/')) res.setHeader('Cache-Control', 'no-store');
    if (!['GET', 'HEAD'].includes(req.method)) return res.status(405).json({ error: 'Read-only indexer: sign transactions with your wallet directly' });
    if (req.path.startsWith('/api/')) {
      if (activeApi >= 32) return res.status(429).json({ error: 'Indexer busy; retry shortly' });
      activeApi++; let done = false;
      const finish = () => { if (!done) { done = true; activeApi--; } };
      res.once('finish', finish); res.once('close', finish);
    }
    next();
  });
  app.get('/api/config', (_req, res) => res.json(publicConfig));
  app.get('/api/leaderboards', asyncRoute(async (req, res) => {
    const scope = req.query.scope ?? 'race', group = req.query.group ?? 'racers';
    if (!['race', 'cycle'].includes(scope) || !['racers', 'teams'].includes(group)) return res.status(400).json({ error: 'Invalid leaderboard scope or group' });
    const wallet = req.query.wallet ? address(req.query.wallet) : null;
    const s = await snapshot(), frame = await boards(s), rows = frame.rankings[scope][group];
    const limit = integer(req.query.limit, 50, 100), offset = req.query.offset === undefined || req.query.offset === '0' ? 0 : integer(req.query.offset, 0, 10000);
    const racers = frame.rankings.race.racers, total = racers.reduce((sum, p) => sum + BigInt(p.distance), 0n), own = racers.find(p => p.wallet === wallet);
    const eligible = total - BigInt(own?.distance ?? '0');
    const items = rows.slice(offset, offset + limit).map(row => ({ ...row,
      winChanceBps: total ? Number(BigInt(row.distance) * 10000n / total) : 0,
      rocketChanceBps: group === 'racers' && wallet && row.wallet !== wallet && eligible > 0n ? Number(BigInt(row.distance) * 10000n / eligible) : 0 }));
    const self = wallet ? rows.find(row => row.wallet === wallet || row.members?.includes(wallet)) ?? null : null;
    res.json({ ...s.at, scope, group, race: frame.state.race, cycle: frame.state.cycle, items, self, total: rows.length,
      next: offset + limit < rows.length ? offset + limit : null, resolvedDistance: !frame.preview, preview: frame.preview, previewTick: frame.previewTick,
      finalizing: !!frame.state.replay_until, processing: !!frame.state.replay_until || ['snapshot', 'movement', 'combat'].includes(frame.state.phase) });
  }));
  app.get('/api/combat', asyncRoute(async (req, res) => {
    const wallet = req.query.wallet ? address(req.query.wallet) : null;
    const after = req.query.after === undefined ? null : req.query.after === '0' ? 0 : integer(req.query.after, null, Number.MAX_SAFE_INTEGER);
    const items = store.rocketHits(gameContracts, wallet, after, integer(req.query.limit, 30, 100)).map(hit => ({ ...hit, attackerNickname: store.profileName(hit.attacker), targetNickname: store.profileName(hit.target) }));
    const s = await snapshot(), state = await s.smart(c.game, { state: {} });
    const forecast = await previews.get(s, state, await s.smart(c.game, { config: {} }));
    const previewItems = (forecast?.hits ?? []).filter(hit => !wallet || hit.attacker === wallet || hit.target === wallet).slice(-100)
      .map(hit => ({ ...hit, attackerNickname: store.profileName(hit.attacker), targetNickname: store.profileName(hit.target) }));
    res.json({ items, previewItems, next: items.at(-1)?.id ?? after ?? 0, index: indexer.status() });
  }));
  let gasCache = null;
  app.get('/api/keeper/gas', asyncRoute(async (_req, res) => {
    if (!config.keeperAddress) return res.status(503).json({ error: 'Keeper address not configured' });
    if (!gasCache || Date.now() - gasCache.at > 60000) gasCache = { at: Date.now(), value: keeperGas(chain, config.keeperAddress) };
    try { res.json(await gasCache.value); } catch (error) { gasCache = null; throw error; }
  }));
  app.get('/api/recruitment', asyncRoute(async (_req, res) => {
    const s = await snapshot(); const config = await s.smart(c.recruitment, { config: {} });
    res.json({ ...s.at, pullPrice: config.pull_price, catalogue: config.catalogue, collection: config.collection });
  }));
  app.get('/api/wallet/:wallet/recruitment/:id', asyncRoute(async (req, res) => {
    const wallet = address(req.params.wallet), id = integer(req.params.id, null, Number.MAX_SAFE_INTEGER), s = await snapshot();
    const job = await s.smart(c.recruitment, { job: { id } });
    if (job.wallet !== wallet) return res.status(404).json({ error: 'Draw does not belong to this wallet' });
    const request = await s.smart(c.randomness, { request: { consumer: c.recruitment, id } });
    const items = job.completed ? await Promise.all(job.minted.map(async tokenId => {
      const info = await s.smart(contracts.commanderCollection, { nft_info: { token_id: tokenId } }, true);
      return { tokenId, collection: contracts.commanderCollection, ...info, unavailable: !info };
    })) : [];
    res.json({ ...s.at, job, items, stage: job.completed ? 'complete' : request.randomness ? 'minting' : 'randomness', availableAt: request.available_at });
  }));
  app.get('/api/wallet/:wallet/recruitment', asyncRoute(async (req, res) => {
    const wallet = address(req.params.wallet), s = await snapshot();
    const limit = integer(req.query.limit, 20, 50);
    const items = await s.smart(c.recruitment, { jobs: { wallet, start_after: req.query.start_after ? integer(req.query.start_after, null, Number.MAX_SAFE_INTEGER) : null, limit } });
    res.json({ ...s.at, items, next: items.length === limit ? items.at(-1).id : null });
  }));
  app.get('/api/health', asyncRoute(async (_req, res) => {
    const health = await chain.health(), index = indexer.status(), indexLag = Math.max(0, health.height - index.indexed_height);
    if (indexLag > 5) { health.healthy = false; health.reasons.push('indexer_behind'); }
    const s = { at: health, smart: (contract, message) => chain.smart(contract, message, health.height) };
    const [game, fleet, controls, cfg] = await Promise.all([s.smart(c.game, { state: {} }), s.smart(c.fleet, { config: {} }), s.smart(c.pause, { state: {} }), s.smart(c.game, { config: {} })]);
    const last = store.db.prepare("SELECT b.time FROM events e JOIN blocks b ON b.height=e.height WHERE e.contract=? AND e.action IN ('check_race_tick','advance_race','seal_race_blocks') ORDER BY e.id DESC LIMIT 1").get(c.game);
    const balance = config.keeperAddress ? (await chain.lcd(`/cosmos/bank/v1beta1/balances/${config.keeperAddress}/by_denom?denom=uluna`, health.height)).balance?.amount ?? '0' : null;
    const launchTime = fleet.presale ? fleet.presale.starts_at === null ? Number.MAX_SAFE_INTEGER : fleet.presale.starts_at + fleet.presale.duration_seconds : null;
    const gameHealth = assessGameHealth(game, { players: fleet.players, paused: controls.paused || cfg.paused, launchTime, lastProgressTime: last?.time, keeperBalance: balance });
    if (!gameHealth.gameHealthy) { health.healthy = false; health.reasons.push(...gameHealth.gameReasons); }
    res.status(health.healthy ? 200 : 503).json({ chainId: config.chainId, ...health, ...gameHealth, indexLag, index });
  }));
  const raceStarts = new Map();
  app.get('/api/state', asyncRoute(async (_req, res) => {
    const s = await snapshot();
    const [game, configuration, pause, pools] = await Promise.all([s.smart(c.game, { state: {} }), s.smart(c.game, { config: {} }), s.smart(c.pause, { state: {} }), s.smart(c.rewards, { state: {} })]);
    let raceStartedAt = null;
    if (game.phase !== 'snapshot' && game.start_height > 0 && game.start_height <= s.at.height) {
      if (!raceStarts.has(game.start_height)) {
        const block = await chain.rpc('block', { height: String(game.start_height) });
        raceStarts.set(game.start_height, block.block.header.time);
        if (raceStarts.size > 64) raceStarts.delete(raceStarts.keys().next().value);
      }
      raceStartedAt = raceStarts.get(game.start_height);
    }
    const raceElapsedSeconds = raceElapsedByHeight(game, s.at.height, configuration.parameters);
    res.json({ ...s.at, deployment: config.fingerprint, raceStartedAt, raceClockMode: 'height', raceElapsedSeconds,
      chainHealth: assessChainHealth(s.at), game, configuration, pause, pools,
      rewardEstimate: cycleEstimate(game, configuration.parameters, pools, s.at),
      previewMode: (configuration.accounting_version ?? 0) >= 2, finalizing: !!game.replay_until, index: indexer.status() });
  }));
  app.get('/api/wallet/:wallet', asyncRoute(async (req, res) => {
    const wallet = address(req.params.wallet); const s = await snapshot();
    const [fleet, canonicalPlayer, credit, membership, skinStatus, positions, balances, native, jobs, listings] = await Promise.all([
      s.smart(c.fleet, { fleet: { wallet } }), s.smart(c.game, { player: { wallet } }, true), s.smart(c.rewards, { credit: { wallet } }),
      s.smart(c.clans, { member: { wallet } }), Promise.all(SHIPS.map(ship => s.smart(c.skins, { status: { wallet, ship } }))),
      Promise.all(SLOTS.map(slot => s.smart(c.equipment, { position: { wallet, slot } }))),
      Promise.all(tokens.map(async token => ({ ...token, ...(await s.smart(token.address, { balance: { address: wallet } })) }))),
      chain.lcd(`/cosmos/bank/v1beta1/balances/${wallet}/by_denom?denom=uluna`, s.at.height),
      s.smart(c.recruitment, { jobs: { wallet, start_after: null, limit: 10 } }), s.smart(c.marketplace, { seller: { wallet, start_after: null, limit: 20 } }),
    ]);
    const legacyCredit = c.backingRewards ? await s.smart(c.backingRewards, { credit: { wallet } }) : { spaceusd: '0', srt: '0' };
    const totalCredit = { spaceusd: (BigInt(credit.spaceusd) + BigInt(legacyCredit.spaceusd)).toString(), srt: (BigInt(credit.srt) + BigInt(legacyCredit.srt)).toString() };
    const reinvestment = c.reinvestment ? { ...(await s.smart(c.reinvestment, { reinvestment: { wallet } })), pricePerLevel: (await s.smart(c.fleet, { config: {} })).price_per_tier } : null;
    const raceState = await s.smart(c.game, { state: {} });
    const forecast = await previews.get(s, raceState, await s.smart(c.game, { config: {} }));
    const player = forecast?.players.get(wallet) ?? canonicalPlayer;
    const cycle = raceState.cycle;
    let playerIndex = null;
    if (raceState.phase === 'movement' && raceState.cursor > 0) {
      const prefix = Buffer.from('index'); const size = Buffer.alloc(2); size.writeUInt16BE(prefix.length);
      const index = await chain.raw(c.game, Buffer.concat([size, prefix, Buffer.from(wallet)]), s.at.height);
      if (index && (index[0] === raceState.race || raceState.registry_enabled) && index[1] <= raceState.participants) playerIndex = index[1];
    }
    const playerTick = forecast?.players.has(wallet) ? forecast.throughTick : player?.race === raceState.race ? processedMovementTick(raceState, playerIndex) : 0;
    const score = await s.smart(c.game, { cycle_player: { cycle, wallet } });
    const configuration = await s.smart(c.game, { config: {} }), pools = await s.smart(c.rewards, { state: {} }), frame = await boards(s);
    const estimate = cycleEstimate(raceState, configuration.parameters, pools, s.at);
    const allocation = { cycle: raceState.cycle, ...rewardAllocation(wallet, raceState, configuration.parameters, estimate.available, frame.players, frame.clans) };
    const live = !forecast && publicConfig.timingVersion >= 2 && player ? await s.smart(c.game, { live_player: { wallet } }, true) : null;
    const pending = new Map();
    if (forecast && !raceState.replay_until) for (const a of forecast.actions.filter(a => a.wallet === wallet && a.tick > forecast.throughTick && ['speed', 'acceleration', 'temporary'].includes(a.kind))) {
      const entry = pending.get(a.tick) ?? { tick: a.tick, speed: 0, acceleration: 0, temporary: 0 };
      entry[a.kind] += Number(a.count); pending.set(a.tick, entry);
    }
    res.json({ ...s.at, wallet, race: raceState.race, nickname: store.profileName(wallet), fleet, player, playerTick, preview: !!forecast?.players.has(wallet),
      livePlayer: live?.player ?? null, livePending: forecast ? [...pending.values()] : live?.pending ?? [], credit: totalCredit, legacyCredit, reinvestment, rewardAllocation: allocation, score, membership, balances, native: native.balance ?? { denom: 'uluna', amount: '0' },
      skins: Object.fromEntries(SHIPS.map((ship, i) => [ship, skinStatus[i]])), equipment: Object.fromEntries(SLOTS.map((slot, i) => [slot, positions[i]])), jobs, listings });
  }));
  app.get('/api/wallet/:wallet/referrals', asyncRoute(async (req, res) => {
    const wallet = address(req.params.wallet); const s = await snapshot();
    const limit = integer(req.query.limit, 20, 50);
    const startAfter = req.query.start_after ? address(req.query.start_after) : null;
    const referred = await s.smart(c.fleet, { referrals: { sponsor: wallet, start_after: startAfter, limit } });
    res.json({ ...s.at, sponsor: wallet, referred, next: referred.length === limit ? referred.at(-1) : null,
      earnings: store.referralEarnings(wallet, c.fleet) });
  }));
  app.get('/api/wallet/:wallet/refunds', asyncRoute(async (req, res) => {
    const wallet = address(req.params.wallet), s = await snapshot();
    let total = 0n, creditsTotal = 0n, boostCount = 0; const claims = [];
    for (const contract of gameContracts) {
      const credits = BigInt(await chain.raw(contract, refundKey(wallet), s.at.height) ?? '0');
      total += credits; creditsTotal += credits;
      const results = new Map(), refundable = [];
      for (const entry of store.boostEntries(wallet, contract)) {
        if (!results.has(entry.race)) results.set(entry.race, await s.smart(contract, { result: { race: entry.race } }, true));
        const result = results.get(entry.race); if (!result || entry.tick <= result.ticks) continue;
        const boosts = await chain.raw(contract, boostKey(entry.race, entry.tick, wallet), s.at.height);
        if (boosts && BigInt(boosts.paid) > 0n) { total += BigInt(boosts.paid); boostCount += boosts.speed + boosts.acceleration + boosts.temporary + boosts.rockets + (entry.heals ?? 0); refundable.push(entry); }
      }
      const groups = groupRefundTicks(refundable);
      if (!groups.length && credits > 0n) groups.push({ race: 1, ticks: [] });
      claims.push(...groups.map(group => ({ ...group, contract })));
    }
    res.json({ ...s.at, wallet, amount: total.toString(), boostCount, refundCredits: creditsTotal.toString(), claims, index: indexer.status() });
  }));
  app.get('/api/wallet/:wallet/boosts', asyncRoute(async (req, res) => {
    const wallet = address(req.params.wallet), s = await snapshot();
    const current = await s.smart(c.game, { state: {} }); const results = new Map();
    const items = [];
    for (const entry of store.boostHistory(wallet, gameContracts)) {
      const key = boostKey(entry.race, entry.tick, wallet);
      const queued = await chain.raw(entry.contract, key, s.at.height);
      let status = 'queued';
      if (entry.race <= current.race) {
        if (!results.has(entry.race)) results.set(entry.race, await s.smart(entry.contract, { result: { race: entry.race } }, true));
        const result = results.get(entry.race);
        if (result) status = entry.tick > result.ticks ? (queued ? 'refundable' : 'recovered') : 'processed';
        else if (entry.race === current.race && !queued && entry.tick <= processedMovementTick(current)) status = 'processed';
        else if (entry.liveActivated && entry.contract === c.game && entry.race === current.race) status = 'active';
      }
      const counts = queued ? Object.fromEntries(['speed', 'acceleration', 'temporary', 'rockets'].filter(kind => queued[kind] > 0).map(kind => [kind, queued[kind]])) : {};
      if (['speed', 'acceleration', 'temporary', 'rocket', 'heal'].includes(entry.kind) && entry.count > 0 && (!queued || entry.kind === 'heal')) counts[entry.kind === 'rocket' ? 'rockets' : entry.kind] = entry.count;
      items.push({ ...entry, status, counts });
    }
    res.json({ ...s.at, wallet, items });
  }));
  app.get('/api/wallet/:wallet/history', asyncRoute(async (req, res) => {
    const wallet = address(req.params.wallet), s = await snapshot(); const wins = store.victories(wallet, gameContracts);
    const victories = await Promise.all(wins.slice(0, 12).map(async win => ({ ...win, result: await resultFor(s, win.race) })));
    res.json({ ...s.at, wins: wins.length, victories, claims: store.rewardClaims(wallet, [c.rewards, ...(c.backingRewards ? [c.backingRewards] : [])], gameConfig.srt, rewardConfig.spaceusd), index: indexer.status() });
  }));
  app.get('/api/treasury', asyncRoute(async (_req, res) => {
    const s = await snapshot();
    const [config, rewards] = await Promise.all([s.smart(c.capitalTreasury, { config: {} }), s.smart(c.rewards, { state: {} })]);
    const balances = await Promise.all(tokens.map(async token => ({ ...token,
      treasury: (await s.smart(c.capitalTreasury, { balance: { token: token.address } }).catch(() => null))?.balance ?? '0',
      rewardPool: (await Promise.all([...new Set([config.reward_pool ?? c.rewards, c.rewards, ...(c.backingRewards ? [c.backingRewards] : [])])].map(async address => BigInt((await s.smart(token.address, { balance: { address } })).balance)))).reduce((a, b) => a + b, 0n).toString() })));
    const routes = await Promise.all(tokens.map(async token => ({ token: token.address,
      route: await s.smart(c.capitalTreasury, { route: { token: token.address } }).catch(() => null) })));
    res.json({ ...s.at, treasury: c.capitalTreasury, collapsar: config.overlord, rewardPool: c.rewards, backingRewardPool: c.backingRewards ?? config.reward_pool ?? c.rewards, config, rewards, balances, routes });
  }));
  app.get('/api/wallet/:wallet/nfts', asyncRoute(async (req, res) => {
    const s = await snapshot();
    res.json({ ...s.at, ...(await inventory(address(req.params.wallet), req.query.collection ?? 'commanders', req.query.start_after, integer(req.query.limit, 12, 24), s)) });
  }));
  app.get('/api/marketplace', asyncRoute(async (req, res) => {
    const s = await snapshot(); const after = req.query.start_after ? integer(req.query.start_after, null, Number.MAX_SAFE_INTEGER) : null;
    const listings = await s.smart(c.marketplace, { active: { start_after: after, limit: integer(req.query.limit, 30, 50) } });
    const items = await Promise.all(listings.map(async listing => ({ ...listing, nft: await s.smart(listing.collection, { nft_info: { token_id: listing.token_id } }) })));
    res.json({ ...s.at, items, next: listings.length ? listings.at(-1).id : null });
  }));
  app.get('/api/clans', asyncRoute(async (req, res) => {
    const s = await snapshot(); const limit = integer(req.query.limit, 20, 50);
    const [configuration, items] = await Promise.all([s.smart(c.clans, { config: {} }), s.smart(c.clans, { clans: { start_after: req.query.start_after ? integer(req.query.start_after, null, Number.MAX_SAFE_INTEGER) : null, limit } })]);
    res.json({ ...s.at, configuration, items, next: items.length === limit ? items.at(-1).id : null });
  }));
  app.get('/api/clans/:id', asyncRoute(async (req, res) => {
    const s = await snapshot(), id = integer(req.params.id, null, Number.MAX_SAFE_INTEGER);
    const [clan, configuration, game] = await Promise.all([s.smart(c.clans, { clan: { id } }), s.smart(c.clans, { config: {} }), s.smart(c.game, { state: {} })]);
    const wallets = [...new Set([...clan.members, ...clan.next_members])];
    const members = await Promise.all(wallets.map(async wallet => {
      const [fleet, score] = await Promise.all([s.smart(c.fleet, { fleet: { wallet } }), s.smart(c.game, { cycle_player: { cycle: game.cycle, wallet } })]);
      return { wallet, fleet, score, active: clan.members.includes(wallet), next: clan.next_members.includes(wallet), leader: wallet === clan.leader };
    }));
    res.json({ ...s.at, clan, leaderNickname: store.profileName(clan.leader), configuration, cycle: game.cycle, members: members.map(member => ({ ...member, nickname: store.profileName(member.wallet) })) });
  }));
  app.get('/api/clans/:id/rewards', (req, res, next) => {
    try { res.json({ clanId: integer(req.params.id, null, Number.MAX_SAFE_INTEGER), ...store.clanRewardHistory(Number(req.params.id)), sync: rewardHistory.status(),
      basis: 'Net member cycle rewards plus leader premiums; referral commissions excluded.' }); } catch (error) { next(error); }
  });
  app.get('/api/activity', (req, res, next) => {
    try {
      const wallet = req.query.wallet ? address(req.query.wallet) : undefined;
      const contract = req.query.contract ? address(req.query.contract) : undefined;
      if (contract && !watched.has(contract)) throw Object.assign(new Error('Contract outside deployment'), { status: 400 });
      const items = store.events({ wallet, contract, gameplay: req.query.view === 'gameplay', before: req.query.before ? integer(req.query.before, null, Number.MAX_SAFE_INTEGER) : undefined, limit: integer(req.query.limit, 30, 100) });
      res.json({ items, next: items.length ? items.at(-1).id : null, index: indexer.status() });
    } catch (error) { next(error); }
  });
  app.get('/api/races/:id', asyncRoute(async (req, res) => {
    const race = integer(req.params.id, null, Number.MAX_SAFE_INTEGER), s = await snapshot(), result = await resultFor(s, race);
    if (!result) return res.status(404).json({ error: 'Race result not available' });
    let winner = null;
    if (result.winner) {
      const score = await s.smart(c.game, { cycle_player: { cycle: result.cycle, wallet: result.winner } });
      const clan = score.clan ? await s.smart(c.clans, { clan: { id: score.clan } }, true) : null;
      const end = store.raceEnd(c.game, race);
      let fleet = null, equipment = null;
      if (end?.height > 1) {
        try {
          const height = end.height - 1;
          const atRace = await chain.smart(c.game, { state: {} }, height);
          if (atRace.race === race) {
            fleet = await chain.smart(c.fleet, { fleet: { wallet: result.winner } }, height);
            equipment = Object.fromEntries(await Promise.all(SLOTS.map(async slot => [slot, await chain.smart(c.equipment, { position: { wallet: result.winner, slot } }, height)])));
          }
        } catch { /* Historical appearance is optional; never substitute a later upgraded fleet. */ }
      }
      winner = { wallet: result.winner, nickname: store.profileName(result.winner), clan, fleet, equipment, historicalFleet: !!fleet };
    }
    res.json({ ...s.at, result, winner });
  }));
  app.get('/architecture', (_req, res) => res.sendFile(resolve(ROOT, 'front/onchain/architecture.html')));
  app.get('/architecture/style.css', (_req, res) => res.sendFile(resolve(ROOT, 'front/onchain/architecture.css')));
  app.get('/architecture/source', (_req, res) => res.type('text/plain').sendFile(resolve(ROOT, 'docs/ARCHITECTURE.md')));
  app.use('/assets/ship-skins', express.static(resolve(ROOT, 'front/ship-skins'), { dotfiles: 'deny', index: false }));
  app.use('/assets/ui', express.static(resolve(ROOT, 'front/ui-assets'), { dotfiles: 'deny', index: false }));
  app.use('/assets/commander-skins', express.static(resolve(ROOT, 'front/commander-skins'), { dotfiles: 'deny', index: false }));
  app.use('/assets', express.static(resolve(ROOT, 'front/onchain/dist'), { dotfiles: 'deny', index: false }));
  for (const path of ['/', '/game', '/shop', '/hangar', '/inventory', '/commander-recruitment', '/marketplace', '/clans', '/clans/:id', '/profile', '/treasury', '/activity']) app.get(path, (_req, res) => res.sendFile(resolve(ROOT, 'front/onchain/index.html')));
  app.use((_req, res) => res.status(404).json({ error: 'Not found' }));
  app.use((error, _req, res, _next) => res.status(error.status ?? 503).json({ error: String(error.message).slice(0, 500) }));
  return { app, chain, store, indexer, rewardHistory, publicConfig, async close() { await rewardHistory.stop(); await indexer.stop(); store.close(); } };
}

export async function startServer() {
  const config = loadConfig(); const service = await createIndexer(config);
  service.indexer.start();
  service.rewardHistory.start();
  const server = service.app.listen(config.port, config.host, () => console.log(`Onchain indexer: http://${config.host}:${config.port}`));
  const stop = () => server.close(async () => { await service.close(); process.exit(0); });
  process.on('SIGINT', stop); process.on('SIGTERM', stop);
  return { ...service, server };
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await startServer();
