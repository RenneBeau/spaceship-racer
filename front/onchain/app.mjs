import './style.css';
import './cockpit.css';
import './game.css';
import './stakes.css';
import './expansion.css';
import './competition.css';
import './guidance.css';
import './race-experience.css';
import './interface.css';
import './scene.css';
import { setupInterface, interfaceIcon, showSceneAction } from './interface.mjs';
import { boostInfo, formatStat } from './boost-info.mjs';
import { createRaceExperience } from './race-experience.mjs';
import { createReinvestment } from './reinvestment.mjs';
import { healingQuote } from '../../src/onchain/healing.mjs';
import { rocketPrice } from './rocket-price.mjs';
import { Wallet } from './wallet.mjs';
import { upgradeStats } from './upgrade-stats.mjs';
import { appPath, routePath } from './paths.mjs';
import { createAlphaOnboarding } from './alpha.mjs';
import * as tx from './transactions.mjs';
import { distanceProjection, movementSpeed } from './telemetry.mjs';
import { createRecruitment } from './recruitment.mjs';
import { createSocial } from './social.mjs';
import { createCompetition } from './competition.mjs';
import { fleetHealth } from '../../src/onchain/leaderboards.mjs';
import { healthBar } from './battle-ui.mjs';

const $ = id => document.getElementById(id);
const selected = new Set();
let config, wallet, state, profile, treasury = null, referral = null, watched = '', busy = false, refreshing = false, tab = 'game';
let nftPages = { commanders: { items: [], next: null }, skins: { items: [], next: null } };
let marketPage = { items: [], next: null }, clanPage = { items: [], next: null }, activityPage = { items: [], next: null };
let inventoryKey = '';
let recruitment, social, competition, raceExperience, reinvestment, alpha;
let raceClock = null, distanceReading = null, distanceFrame = null;
let boostHistory = [], boostHistoryKey = '', lastBoostHistory = 0;
const localBoostReceipts = new Map();
let rocketQuote = null;
function renderGuidance() {
  const own = connectedProfile;
  $('guide-wallet').textContent = wallet.address ? `Connected ${short(wallet.address)}. Keep native LUNC available for network gas.` : 'Connect Keplr or Leap to own ships and sign actions. You can watch without connecting.';
  $('guide-connect').hidden = !!wallet.address;
  $('guide-fleet').textContent = own?.fleet.purchased ? 'Your fleet is permanent. Add ships or pay only for the levels you add.' : 'Start with a racer in the hangar. Ship invoices settle in SpaceUSD; native LUNC pays network gas.';
  const participating = own?.player?.race === state.game.race;
  $('guide-race').textContent = participating ? `Your fleet is participating in race #${state.game.race}. Boosts are optional; upgrades wait for the next race.` : own?.fleet.purchased ? `Your purchase is queued for race #${own.fleet.effective_race}. No separate entry transaction is needed.` : 'Your first purchase joins next race automatically. Current race results cannot be changed by an upgrade.';
  [...$('flight-guide').querySelectorAll('li')].forEach((item, i) => item.classList.toggle('complete', [!!wallet.address, !!own?.fleet.purchased, participating][i]));
}
function renderBoostGuide() {
  if (!state) return;
  const form = $('boost-form'), kind = form.elements.kind.value, countText = form.elements.count.value;
  form.dataset.kind = kind;
  const valid = /^\d+$/.test(countText) && BigInt(countText) > 0n && BigInt(countText) <= 0xffffffffn;
  const params = state.configuration.parameters;
  const effects = {
    speed: 'Adds speed for the rest of this race. Damage can reduce effective speed.',
    acceleration: 'Builds speed over time. Earlier activation gives it more time to contribute.',
    temporary: `Adds temporary winner-selection distance for up to ${params.temporary_ticks * params.tick_seconds} seconds, or until this race ends.`,
    rocket: 'Attacks another eligible fleet, selected by distance weight. Requires a surviving fighter; no target is chosen by you.',
    heal: 'Restores HP and shields, including fallen ships. A fallen-ship surcharge applies. Full-fleet revival restores its saved distance and reconstructs speed from race ticks.',
  };
  $('boost-effect').textContent = effects[kind];
  const fee = Number(params.boost_fee_bps) / 100;
  $('boost-rules').textContent = `${100 - fee}% prize allocation · ${fee}% service fee on executed spending. ${config.timingVersion >= 2 ? 'Utility power activates with confirmed payment; combat and final outcomes are verified later.' : 'Actions apply when their tick is processed.'} If an earlier ending makes your action unused, recover its SRT.`;
  const player = profile?.player;
  const participating = player?.race === state.game.race;
  const effectPlayer = participating ? profile.livePlayer ?? player : null;
  const preview = boostInfo(kind, valid ? countText : '1', params, effectPlayer, state.configuration.healing, navigator.language);
  const names = { speed: 'Overdrive', acceleration: 'Afterburner', temporary: 'Warp surge', rocket: 'Rocket strike', heal: 'Repair' };
  $('boost-selection').textContent = `${names[kind]} · ${kind === 'heal' ? 'one repair request' : valid ? `${formatStat(BigInt(countText), 0, navigator.language)} selected` : 'per-boost preview'}`;
  for (const id of ['boost-stat-grid', 'boost-calculation-stats']) clear(id, preview.stats.map(({ label, value }) => { const item = node('div'); item.append(node('dt', label), node('dd', value)); return item; }));
  $('boost-stat-note').textContent = preview.note;
  for (const card of document.querySelectorAll('[data-boost]')) {
    let summary = card.querySelector('.boost-card-stat');
    if (!summary) { summary = node('span', undefined, 'boost-card-stat'); card.append(summary); }
    const text = boostInfo(card.dataset.boost, 1, params, effectPlayer, state.configuration.healing, navigator.language).summary;
    if (summary.textContent !== text) summary.textContent = text;
  }
  const fighter = participating && player.ships?.[2]?.hp !== undefined && BigInt(player.ships[2].hp) > 0n;
  const key = `${state.height}:${watched}`;
  if (kind === 'rocket' && fighter && rocketQuote?.key !== key) {
    rocketQuote = { key, price: null, error: false };
    void rocketPrice((address, message) => wallet.query(address, message), config.contracts.game, watched).then(price => {
      if (rocketQuote?.key !== key) return;
      rocketQuote.price = price; updateButtons();
    }).catch(() => { if (rocketQuote?.key === key) { rocketQuote.error = true; updateButtons(); } });
  }
  const isHeal = kind === 'heal';
  const healOption = form.elements.kind.querySelector('option[value="heal"]');
  if (healOption) { healOption.hidden = !state.configuration.healing?.enabled; healOption.disabled = !state.configuration.healing?.enabled; }
  const repairCard = document.querySelector('[data-boost="heal"]'); if (repairCard) repairCard.hidden = !state.configuration.healing?.enabled;
  let healMaximum = null;
  if (isHeal) { try { healMaximum = tx.amount(tx.units(form.elements.heal_max.value, token(config.srt)?.decimals ?? 6)); } catch {} }
  const unit = isHeal ? healMaximum : kind === 'rocket' ? (rocketQuote?.key === key ? rocketQuote.price : null) : params.boost_price;
  const total = valid && unit !== null ? BigInt(unit) * BigInt(countText) : null;
  $('boost-cost').textContent = !valid ? 'Enter a whole quantity from 1 to 4,294,967,295.' : total !== null ? `Total: ${money(total.toString(), config.srt)} · ${money(unit, config.srt)} each` : !fighter ? 'Rocket price needs an active fighter.' : rocketQuote?.error ? 'Rocket quote unavailable. Retrying with fresh chain data.' : 'Checking on-chain rocket price…';
  if (isHeal) {
    const quote = participating && state.configuration.healing?.enabled ? healingQuote(player, state.configuration.healing) : null;
    $('boost-cost').textContent = quote ? `Repair estimate: ${money(quote.price, config.srt)} · spend limit: ${healMaximum ? money(healMaximum, config.srt) : 'enter an amount'}` : 'Repair estimate needs a participating fleet.';
    $('boost-effect').textContent += ' Cost is settled during replay. If your limit is too low, no repair is applied; unused reserved SRT is refundable.';
  }
  $('boost-calculation-effect').textContent = $('boost-effect').textContent;
  let reason = '';
  if (!wallet.address) reason = 'Connect your wallet to activate a boost.';
  else if (watched !== wallet.address) reason = 'You are watching another pilot. Switch to your wallet to boost your fleet.';
  else if (state.chainHealth?.healthy === false) reason = 'Waiting for fresh chain data.';
  else if (state.pause.paused) reason = 'The game is paused.';
  else if (!participating) reason = 'Your fleet joins next race. Boosts unlock once it is participating.';
  else if (!player.alive && !isHeal) reason = 'Your fleet was destroyed this race. Repair it or wait for next race.';
  else if (state.game.replay_until || !['waiting', 'movement', 'combat'].includes(state.game.phase) || state.height < state.game.start_height || state.height - state.game.start_height >= params.duration_cdf.length) reason = 'This race is resolving. Boosts reopen for the next race.';
  else if (kind === 'rocket' && !fighter) reason = 'You need a surviving fighter this race to fire rockets.';
  else if (isHeal && !state.configuration.healing?.enabled) reason = 'Repair boosts are not enabled for this race.';
  else if (!valid) reason = 'Choose a valid whole quantity.';
  else if (total === null) reason = 'Wait for the on-chain price before activating.';
  else {
    const balance = profile.balances.find(asset => asset.address === config.srt)?.balance;
    if (balance !== undefined && BigInt(balance) < total) reason = 'Not enough SRT for this quantity. Lower the count or fund your wallet.';
  }
  $('boost-availability').textContent = reason || 'Ready for this race. Your wallet will show the final payment and network gas.';
  $('boost-availability').classList.toggle('ready', !reason);
  if (reason) form.querySelector('button').disabled = true;
}
function renderRaceTimer() {
  const target = $('race-elapsed');
  if (!target || !raceClock) return;
  const seconds = Math.max(0, Math.floor(raceClock.elapsed + (raceClock.started ? (performance.now() - raceClock.received) / 1000 : 0)));
  const hours = Math.floor(seconds / 3600);
  target.textContent = `${hours ? `${hours}:` : ''}${String(Math.floor(seconds / 60) % 60).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}
function renderDistance(player) {
  cancelAnimationFrame(distanceFrame);
  const target = $('pilot-distance'); if (!target) return;
  if (!player || player.race !== state.game.race) { target.textContent = 'Joining next race'; distanceReading = null; return; }
  const tickSeconds = state.configuration.parameters.tick_seconds;
  const processedTick = profile.playerTick ?? Math.max(0, state.game.tick - 1);
  const snapshotTime = Date.parse(profile.time ?? state.time);
  const raceElapsed = state.raceClockMode === 'height' ? Math.max(0, Math.min(state.configuration.parameters.duration_cdf.length, profile.height - state.game.start_height)) * tickSeconds
    : state.raceStartedAt ? Math.max(0, (snapshotTime - Date.parse(state.raceStartedAt)) / 1000) : processedTick * tickSeconds;
  const value = Number(player.distance) / 1000;
  const key = `${watched}:${player.race}`;
  const previous = distanceReading?.key === key ? distanceReading : null;
  const now = performance.now();
  const live = { pending: profile.livePending ?? [], parameters: state.configuration.parameters };
  const initial = distanceProjection({ player, processedTick, tickSeconds, raceElapsed, active: !state.game.replay_until && state.chainHealth?.healthy !== false, ...live });
  distanceReading = { key, value, height: profile.height, received: previous?.height === profile.height ? previous.received : now, shown: previous?.shown ?? initial.projected, lastFrame: now };
  target.title = `${profile.preview ? 'Preview · pending final validation' : 'Live estimate · confirmed distance including warp'}: ${initial.confirmed.toLocaleString()}`;
  const animate = now => {
    const reading = distanceReading;
    if (!reading || reading.key !== key) return;
    const active = !state.game.replay_until && state.chainHealth?.healthy !== false && ['waiting', 'movement', 'combat'].includes(state.game.phase) && !state.pause.paused;
    const seconds = Math.max(0, (now - reading.received) / 1000);
    target.dataset.stale = String(seconds > 15);
    if (seconds > 15) target.title = 'Waiting for fresh chain data · visual estimate paused';
    const { projected } = distanceProjection({ player, processedTick, tickSeconds, raceElapsed, sinceSnapshot: seconds, active, ...live });
    reading.shown += (projected - reading.shown) * Math.min(1, (now - reading.lastFrame) / 180);
    reading.lastFrame = now;
    target.textContent = reading.shown.toLocaleString(undefined, { maximumFractionDigits: 1 });
    distanceFrame = requestAnimationFrame(animate);
  };
  // Text telemetry continues in reduced-motion mode too; it is not decorative
  // animation. Limit visual projection if chain snapshots stop arriving.
  distanceFrame = requestAnimationFrame(animate);
}
function renderBoostHistory() {
  const labels = { speed: 'Overdrive', acceleration: 'Afterburner', temporary: 'Warp surge', rockets: 'Rocket strike', heal: 'Repair' };
  if (!watched) { clear('boost-feedback', [node('p', 'Connect your wallet to track your boosts.', 'muted')]); return; }
  clear('boost-feedback', boostHistory.length ? boostHistory.slice(0, 6).map(entry => {
    const row = node('div', undefined, `boost-receipt ${entry.status}`);
    const kinds = Object.entries(entry.counts ?? {}).map(([kind, count]) => `${labels[kind]} ×${count}`).join(' · ');
    row.append(node('strong', kinds || money(entry.paid, config.srt)));
    const status = entry.kind === 'heal' && entry.status === 'processed' ? 'Repair request resolved · unused reserve is refundable' : { queued: 'Queued · awaiting race processing', active: 'Active now · final race outcome pending', processed: 'Processed onchain', refundable: 'Race ended earlier · recover this SRT', recovered: 'Recovered to wallet' }[entry.status];
    row.append(node('span', `${status} · Race ${entry.race}, tick ${entry.tick}`));
    return row;
  }) : [node('p', 'Your boost receipts will appear here: queued, processed, or refundable.', 'muted')]);
}
let connectedProfile = null, purchasePreview = null, quoteVersion = 0;
const confirmedUpgrades = new Map();
function applyConfirmedUpgrades(data) {
  if (!data) return;
  for (const [key, upgrade] of confirmedUpgrades) {
    if (upgrade.wallet !== data.wallet) continue;
    if (data.fleet.next[upgrade.index] >= upgrade.level) { confirmedUpgrades.delete(key); continue; }
    data.fleet.next[upgrade.index] = upgrade.level;
    data.fleet.purchased = true;
    data.fleet.effective_race = Math.max(data.fleet.effective_race, upgrade.effectiveRace);
  }
}
const ships = ['racer', 'tank', 'fighter'];
const shipRoles = { racer: ['RACER', 'Speed', 'Adds speed to your fleet.'], tank: ['TANK', 'Defense', 'Absorbs rocket damage with HP and shields.'], fighter: ['FIGHTER', 'Attack', 'Lets your fleet fire rockets.'] };
function pilotKey() { return `racer.pilot.${config.chainId}.${wallet.address}`; }
function pilotIdentity() { try { return JSON.parse(localStorage.getItem(pilotKey()) ?? 'null'); } catch { return null; } }
function restorePilotIdentity() {
  if (wallet.address && connectedProfile?.nickname && !pilotIdentity()) {
    localStorage.setItem(pilotKey(), JSON.stringify({ nickname: connectedProfile.nickname, sponsor: connectedProfile.fleet.sponsor ?? '' })); renderResources();
  }
}
function pilotName(address, nickname = null) {
  let saved; try { saved = JSON.parse(localStorage.getItem(`racer.pilot.${config.chainId}.${address}`) ?? 'null')?.nickname; } catch {}
  return address === wallet.address ? saved || nickname || short(address) : nickname || saved || short(address);
}
function purchaseSponsor() {
  if (profile?.fleet.purchased) return '';
  return profile?.fleet.sponsor ?? pilotIdentity()?.sponsor ?? '';
}
function renderResources() {
  document.querySelector('.resource-dock').classList.toggle('is-empty', !connectedProfile || !wallet.address);
  $('open-profile').disabled = !wallet.address;
  const identity = wallet.address ? pilotIdentity() : null;
  $('open-profile').textContent = `◎ ${identity?.nickname || 'Pilot profile'}`;
  $('open-profile').prepend(interfaceIcon('profile'));
  $('open-profile').setAttribute('aria-label', identity?.nickname ? `Profile: ${identity.nickname}` : 'Pilot profile');
  $('open-profile').title = identity?.nickname || 'Pilot profile';
  if (!connectedProfile || !wallet.address) {
    clear('resource-header', [node('span', 'Connect your wallet to load your resources', 'resource-empty')]); return;
  }
  const resources = [...connectedProfile.balances, { symbol: 'LUNC', balance: connectedProfile.native.amount, decimals: 6 }];
  clear('resource-header', resources.map((asset, index) => {
    const item = node('div', undefined, 'resource-token');
    const raw = tx.display(asset.balance, asset.decimals);
    item.title = `${raw} ${asset.symbol}${asset.symbol === 'LUNC' ? ' · network gas' : ''}`;
    const symbol = asset.symbol === 'UST' ? 'UST1' : asset.symbol;
    item.append(node('span', symbol === 'SpaceUSD' ? '$' : symbol.slice(0, 1), 'resource-icon'), node('strong', Number(raw).toLocaleString(undefined, { maximumFractionDigits: 2 })), node('small', symbol));
    return item;
  }));
}
function openPilot(first = false) {
  if (!wallet.address) return;
  const identity = pilotIdentity(); const fleet = connectedProfile?.fleet;
  $('pilot-dialog-title').textContent = first ? 'Create your profile' : 'Your profile';
  $('pilot-form').elements.nickname.value = identity?.nickname ?? '';
  const sponsor = fleet?.sponsor || identity?.sponsor || '';
  const input = $('pilot-form').elements.sponsor;
  input.value = sponsor || new URLSearchParams(location.search).get('sponsor') || '';
  input.disabled = !!fleet?.purchased || !!sponsor;
  $('sponsor-help').textContent = fleet?.purchased ? `Sponsor registration closed after your first purchase.${fleet.sponsor ? ` Sponsor: ${short(fleet.sponsor)}` : ' No sponsor registered.'}` : sponsor ? 'Sponsor saved. This choice cannot be changed.' : 'Invited by someone? Add their wallet now for a first-purchase discount. Once saved, it cannot be changed; registration closes after your first purchase.';
  $('save-pilot').textContent = first ? 'Save & continue' : 'Save profile';
  $('close-profile').hidden = first;
  $('pilot-error').textContent = '';
  if (!$('pilot-dialog').open) $('pilot-dialog').showModal();
}
let fleetRenderKey = '';
function renderFleet() {
  const health = profile?.player?.race === state?.game.race ? fleetHealth(profile.player) : null;
  const key = JSON.stringify({ wallet: profile?.wallet, fleet: profile?.fleet, equipment: profile?.equipment, progress: profile?.reinvestment, health });
  if (key === fleetRenderKey) return;
  fleetRenderKey = key;
  clear('fleet-cards', ships.map((ship, i) => {
    const fleet = profile?.fleet; const level = fleet?.next[i] ?? 0; const active = fleet?.active[i] ?? 0;
    const skin = profile?.equipment[`${ship}_skin`].active?.attributes;
    const card = node('article', undefined, `card fleet-ship ${ship}${level ? ' owned' : ' unowned'}`);
    const head = node('div', undefined, 'ship-card-head'); head.append(node('span', shipRoles[ship][0]), node('b', level ? `LVL ${level}` : 'NOT OWNED', 'owned-level'));
    const art = node('div', undefined, 'ship-art'); art.append(image(skin ?? { code: `${ship}1`, kind: `${ship}_skin` }));
    const body = node('div', undefined, 'ship-card-body'); body.append(node('h2', title(ship)), node('strong', shipRoles[ship][1]), node('p', shipRoles[ship][2]));
    const vital = health?.ships[i];
    if (vital && BigInt(vital.maxHp) > 0n) {
      const bars = node('div', undefined, 'fleet-vitals'); bars.append(healthBar('HP', vital.hp, vital.maxHp), healthBar(i === 1 ? 'Combat shield' : 'Shield stat', vital.shield, vital.maxShield, true));
      body.append(bars); if (BigInt(vital.hp) === 0n) { card.classList.add('ship-destroyed'); body.append(node('small', 'Destroyed this race', 'destroyed-label')); }
      if (i !== 1 && BigInt(vital.shield) > 0n) bars.title = 'Tank shields absorb rocket hits. Other shield stats follow the legacy cosmetic-shield rule.';
    } else body.append(node('small', level ? 'Race health available at next launch.' : 'Get this ship to join your fleet.', 'muted'));
    const progress = node('div', undefined, 'ship-level-track'); const fill = node('span'); fill.style.width = `${Math.min(level / 10, 100)}%`; progress.append(fill);
    const levels = node('div', undefined, 'ship-level-summary');
    for (const [label, value, cls] of [['Active fleet', active, 'active-level'], [level > active ? `Queued · race #${fleet.effective_race}` : 'Owned level', level, 'queued-level']]) {
      const box = node('div', undefined, cls); box.append(node('small', label), node('strong', value ? `Level ${value}` : 'Not owned')); levels.append(box);
    }
    body.append(levels);
    const saved = profile?.reinvestment?.progress.spaceusd[i];
    if (saved && BigInt(saved) > 0n) {
      const percent = Number(BigInt(saved) * 10000n / BigInt(profile.reinvestment.pricePerLevel)) / 100;
      const bar = node('progress'); bar.max = 100; bar.value = Math.min(percent, 100); bar.setAttribute('aria-label', `${ship} next-level progress`);
      body.append(node('small', `${percent}% saved toward level ${level + 1}`, 'level-progress-note'), bar);
    }
    body.append(progress, node('small', level > active ? `Owned: level ${level} · race power: level ${active}. Level ${level} activates in race ${fleet.effective_race}.` : level ? `Level ${level} · active in races` : 'Buy once. Keep your ship. Upgrade anytime.', 'ship-activation'));
    const actions = node('div', undefined, 'ship-upgrade-actions');
    for (const increment of level ? [1, 10, 100] : [1]) {
      const target = Math.min(1000, level + increment);
      const action = node('button', level ? `+${target - level} level${target - level === 1 ? '' : 's'}` : `Get ${title(ship)} ↗`, increment > 1 ? 'secondary' : '');
      action.type = 'button'; action.dataset.walletAction = '';
      if (level >= 1000) { action.textContent = 'Max level'; action.dataset.blocked = ''; }
      action.addEventListener('click', () => { selectShip(ship, target); }); actions.append(action);
    }
    body.append(actions); card.append(head, art, body); return card;
  }));
}
function upgradeComparison(ship, target) {
  const healing = state?.configuration.healing;
  const decimals = config.tokens.find(token => token.address === config.srt)?.decimals ?? 6;
  const current = upgradeStats(ship, profile?.fleet.next[ships.indexOf(ship)] ?? 0, healing, decimals);
  const upgraded = upgradeStats(ship, target, healing, decimals);
  const box = node('div', undefined, 'upgrade-comparison');
  if (!current || !upgraded) { box.append(node('p', 'Choose a target level from 1 to 1000.')); return box; }
  const table = node('table'); table.setAttribute('aria-label', `${title(ship)} current and upgraded base stats`);
  const head = node('tr'); for (const label of ['Base stat', 'Current owned', 'After upgrade']) head.append(node('th', label));
  const thead = node('thead'); thead.append(head); const body = node('tbody');
  for (const [label, value] of Object.entries(current)) {
    const row = node('tr'); row.append(node('th', label), node('td', value.toLocaleString()), node('td', upgraded[label].toLocaleString())); body.append(row);
  }
  table.append(thead, body); box.append(table, node('p', 'Base stats at full health, before commander bonuses and boosts. Upgraded power activates next race. No passive regeneration.', 'muted'));
  if (ship === 'tank' && healing?.enabled) box.append(node('p', 'Repair example: 1,000 missing HP, no fallen-ship surcharge. Higher tank levels reduce fleet repair cost; the actual quote depends on missing HP/shields and fallen ships.', 'muted'));
  return box;
}
async function previewShip() {
  const version = ++quoteVersion; purchasePreview = null;
  const values = Object.fromEntries(new FormData($('ship-form')));
  let comparison = $('ship-upgrade-preview');
  if (!comparison) { comparison = node('div'); comparison.id = 'ship-upgrade-preview'; $('ship-quote').after(comparison); }
  comparison.replaceChildren(upgradeComparison(values.ship, Number(values.tier)));
  if (!wallet.address || watched !== wallet.address) { $('ship-quote').textContent = 'Connect your wallet, then choose a ship to get started.'; return; }
  const mode = profile?.fleet.next[ships.indexOf(values.ship)] ? 'upgrade' : 'buy';
  $('ship-quote').textContent = 'Calculating your upgrade…';
  try {
    const sponsor = purchaseSponsor();
    const quote = await wallet.quoteShip(mode, values.ship, values.tier, values.token, sponsor);
    if (version !== quoteVersion) return;
    purchasePreview = { ...values, mode, sponsor, quote };
    $('ship-quote').textContent = `Invoice: ${money(quote.price, config.spaceusd)}${values.token !== config.spaceusd ? ` · estimated ${money(quote.estimated_offer, values.token)}` : ''}`;
  } catch (error) { if (version === quoteVersion) $('ship-quote').textContent = error.message; }
}
function selectShip(ship, target) {
  $('ship-form').elements.ship.value = ship; $('ship-form').elements.tier.value = target;
  $('workshop-title').textContent = `${title(ship)} → level ${target}`;
  $('ship-checkout').classList.add('selected-workshop'); $('ship-checkout').scrollIntoView({ behavior: 'smooth', block: 'center' });
  void previewShip();
}
function node(tag, text, className) { const e = document.createElement(tag); if (text !== undefined) e.textContent = text; if (className) e.className = className; return e; }
function clear(id, children = []) { $(id).replaceChildren(...children); }
function short(address) { return address ? `${address.slice(0, 12)}…${address.slice(-6)}` : '—'; }
function title(value) { return String(value).replaceAll('_', ' ').replace(/^./, c => c.toUpperCase()); }
function notice(text, error = false) {
  $('notice').textContent = text; $('notice').classList.toggle('error', error);
  $('notice').dataset.quiet = String(!error && text.startsWith('Flight deck ready'));
}
async function api(path) {
  const response = await fetch(appPath(`/api/${path}`)); const data = await response.json();
  if (!response.ok) throw new Error(data.error ?? 'Indexer request failed'); return data;
}
function token(address) { return config.tokens.find(t => t.address === address); }
function money(value, address) { const asset = token(address); return `${tx.display(value, asset?.decimals ?? 6)} ${asset?.symbol ?? short(address)}`; }
function button(text, fn, secondary = false) {
  const b = node('button', text, secondary ? 'secondary' : ''); b.type = 'button'; b.dataset.walletAction = '';
  b.addEventListener('click', () => run(text, fn)); return b;
}
function image(attributes) {
  const img = document.createElement('img'); const code = attributes?.code ?? '';
  img.alt = title(code || 'Spaceship');
  if (/^[a-z0-9_-]{1,64}$/.test(code)) img.src = appPath(`/assets/${attributes.kind === 'commander' ? 'commander' : 'ship'}-skins/${code}.png`);
  else img.hidden = true;
  img.addEventListener('error', () => { img.hidden = true; }); return img;
}
function tokenSelect(select) { select.replaceChildren(...config.tokens.map(t => { const option = node('option', t.symbol); option.value = t.address; return option; })); select.value = config.spaceusd; }
function updateButtons() {
  const can = !!wallet?.address && watched === wallet.address && !state?.pause.paused && state?.chainHealth?.healthy !== false && !busy;
  document.querySelectorAll('[data-wallet-action]').forEach(b => { b.disabled = !can; });
  $('fuse').disabled ||= selected.size !== 5;
  if (state?.game.replay_until) $('boost-form').querySelector('button').disabled = true;
  const rarity = nftPages.commanders.items.find(item => selected.has(item.tokenId))?.extension.rarity;
  const nextRarity = { common: 'Rare', rare: 'Epic', epic: 'Legendary' }[rarity];
  $('fuse').textContent = selected.size && nextRarity ? `Fuse ${title(rarity)} (${selected.size}/5) → 1 ${nextRarity}` : 'Fuse five selected commanders';
  document.querySelectorAll('[data-blocked]').forEach(b => { b.disabled = true; });
  if (state && (state.height < state.game.start_height || state.height - state.game.start_height >= state.configuration.parameters.duration_cdf.length || !['waiting', 'movement', 'combat'].includes(state.game.phase))) $('boost-form').querySelector('button').disabled = true;
  for (const id of ['claim-rewards', 'profile-claim']) {
    if (!connectedProfile || (BigInt(connectedProfile.credit.srt) === 0n && BigInt(connectedProfile.credit.spaceusd) === 0n)) $(id).disabled = true;
  }
  $('connect').disabled = busy; $('leap').disabled = busy;
  $('disconnect').hidden = !wallet?.address;
  $('connect').textContent = wallet?.address ? short(wallet.address) : 'Connect Keplr';
  $('signer-status').textContent = wallet?.address ? `Signing wallet ${short(wallet.address)}` : 'Spectator mode · wallet signatures authorize every action';
  const operator = wallet?.address && wallet.address === state?.pause.owner;
  $('admin').hidden = !operator;
  $('pause-game').disabled = busy || !operator || !!state?.pause.paused;
  $('resume-game').disabled = busy || !operator || !state?.pause.paused;
  renderBoostGuide();
}
async function run(label, fn) {
  if (busy) return;
  busy = true; updateButtons(); notice(`Preparing ${label.toLowerCase()} — review the transaction in your wallet.`);
  try {
    const result = await fn();
    notice(`Confirmed at block ${result.height}: ${result.transactionHash}`);
    inventoryKey = ''; await refresh(true).catch(() => {});
    return result;
  } catch (error) { notice(error.message.slice(0, 500), true); return null; }
  finally { busy = false; updateButtons(); }
}
function renderState() {
  const s = state.game; const pools = state.pools;
  const ended = (state.configuration.accounting_version ?? 0) >= 2 && (s.replay_until || !['waiting', 'movement', 'combat'].includes(s.phase));
  const elapsed = ended ? (s.replay_until || s.tick) * state.configuration.parameters.tick_seconds : state.raceElapsedSeconds ?? (state.raceStartedAt ? Math.max(0, (Date.parse(state.time) - Date.parse(state.raceStartedAt)) / 1000) : 0);
  raceClock = { race: s.race, elapsed, received: performance.now(), started: !!state.raceStartedAt && !ended && state.chainHealth?.healthy !== false };
  clear('race-stats', [['Race', `#${s.race}`], ['Cycle', `#${s.cycle}`], ['Race time', '00:00'], ['Pilots', String(s.participants)]]
    .map(([label, value]) => { const box = node('div', undefined, 'stat'); const strong = node('strong', value); if (label === 'Race time') strong.id = 'race-elapsed'; box.append(node('span', label), strong); if (label === 'Cycle') box.append(node('small', `${Math.min(s.completed + 1, state.configuration.parameters.races_per_cycle)} / ${state.configuration.parameters.races_per_cycle} races`)); return box; }));
  renderRaceTimer();
  $('pause-banner').hidden = !state.pause.paused && state.chainHealth?.healthy !== false;
  $('pause-banner').textContent = state.chainHealth?.healthy === false ? 'Chain is not producing fresh blocks. Transactions and live projections are paused.' : 'Emergency pause active. Game transactions and NFT transfers are paused.';
  const indexed = state.index.indexed_height;
  $('chain-status').textContent = `Chain block ${state.height.toLocaleString()} · indexed ${indexed.toLocaleString()}${state.index.error ? ' · history retrying' : ''}`;
  $('footer-index').textContent = `${state.index.events.toLocaleString()} public events indexed`;
  const multiplier = Number(s.multiplier);
  $('race-multiplier').textContent = multiplier ? `×${multiplier}` : 'Drawing…';
  document.querySelector('.race-stakes').classList.toggle('high-stakes', multiplier >= 25);
  $('multiplier-title').textContent = multiplier ? `Winner earns ${multiplier} cycle points` : 'Waiting for the race multiplier';
  $('race-detail').textContent = `SpaceUSD funding reserve: ${money(pools.reserve, config.spaceusd)}. Funds accrue into the cycle pool over time. Distance is estimated between confirmations.`;
  clear('credits', [node('h2', profile ? money(profile.credit.spaceusd, config.spaceusd) : '—'), node('p', profile ? money(profile.credit.srt, config.srt) : 'Watch a wallet to see claimable rewards', 'muted')]);
}
function renderProfile() {
  if (!profile) {
    clear('balances', [node('p', 'Connect or watch a wallet.', 'muted')]); clear('pilot-stats'); renderFleet(); return;
  }
  clear('balances', [{ symbol: 'LUNC · gas', balance: profile.native.amount, decimals: 6 }, ...profile.balances].map(t => {
    const row = node('div'); row.append(node('span', t.symbol), node('strong', tx.display(t.balance, t.decimals))); return row;
  }));
  const player = profile.player;
  if (!$('pilot-distance')) clear('pilot-stats', [['Pilot', 'pilot-name'], ['Distance', 'pilot-distance'], ['Points', 'pilot-score'], ['Speed', 'pilot-speed'], ['Warp', 'pilot-warp'], ['Accel', 'pilot-acceleration']]
    .map(([label, id]) => { const box = node('div'); const strong = node('strong', '—'); strong.id = id; box.append(node('span', label), strong); return box; }));
  $('pilot-name').textContent = watched === wallet.address ? pilotIdentity()?.nickname || short(watched) : short(watched);
  $('pilot-score').textContent = tx.display(profile.score.score, 4);
  const activePlayer = player?.race === state.game.race ? profile.livePlayer ?? player : null;
  const concise = value => value >= 1000 ? new Intl.NumberFormat(navigator.language, { notation: 'compact', maximumFractionDigits: 1 }).format(value) : value.toLocaleString(undefined, { maximumFractionDigits: 1 });
  const speed = activePlayer ? movementSpeed(activePlayer) : null;
  const warp = activePlayer ? (activePlayer.temporary ?? []).reduce((sum, item) => sum + Number(item.distance) / 1000, 0) : null;
  $('pilot-speed').textContent = speed === null ? '—' : `${concise(speed)}/s`;
  $('pilot-speed').title = speed === null ? 'No active fleet' : `${speed.toLocaleString(undefined, { maximumFractionDigits: 3 })} distance units per second`;
  $('pilot-warp').textContent = warp === null ? '—' : `+${concise(warp)}`;
  $('pilot-warp').title = warp === null ? 'No active fleet' : `${warp.toLocaleString()} temporary distance`;
  $('pilot-acceleration').textContent = activePlayer?.acceleration_count ?? '—';
  renderDistance(player);
  const activeSkin = profile.equipment.racer_skin.active?.attributes?.code;
  const heroSrc = appPath(`/assets/ship-skins/${activeSkin && /^[a-z0-9_-]+$/.test(activeSkin) ? activeSkin : 'racer1'}.png`);
  if ($('hero-ship').getAttribute('src') !== heroSrc) $('hero-ship').src = heroSrc;
  renderFleet();
  clear('equipment', Object.entries(profile.equipment).map(([slot, position]) => {
    const card = node('div', undefined, 'card'); card.append(node('h3', title(slot)));
    if (position.active) card.append(image(position.active.attributes), node('p', title(position.active.attributes.code)));
    else card.append(node('p', 'Empty', 'muted'));
    if (position.pending) card.append(node('p', 'Change queued for race boundary', 'badge'));
    if (position.active && !position.pending) card.append(button('Queue unbind', () => wallet.execute([tx.instruction(config.contracts.equipment, { unbind: { slot } })], 'unbind NFT'), true));
    return card;
  }));
  clear('skin-jobs', ['racer', 'tank', 'fighter'].map(ship => {
    const status = profile.skins[ship]; const box = node('div', undefined, 'job');
    box.append(node('strong', `${title(ship)} · ${status.available} available / ${status.earned} earned`), node('p', `${status.eligible_codes.length} currently eligible skin codes`, 'muted'));
    const actions = node('div', undefined, 'actions');
    const request = button('Request earned skin', () => wallet.execute([tx.instruction(config.contracts.skins, { request: { ship } })], 'request skin'));
    if (!status.available || !status.eligible_codes.length || status.progress.pending) request.dataset.blocked = '';
    actions.append(request);
    if (status.progress.pending) actions.append(node('span', 'Automatic delivery in progress…', 'badge'));
    box.append(actions); return box;
  }));
  clear('my-listings', profile.listings.map(listing => { const row = node('div', undefined, 'job'); row.append(node('span', `${listing.token_id} · ${money(listing.price, listing.payment_token)}`),
    button('Cancel listing', () => wallet.execute([tx.instruction(config.contracts.marketplace, { cancel: { listing_id: listing.id } })], 'cancel listing'), true)); return row; }));
  const m = profile.membership;
  clear('membership', [node('h2', `Current clan: ${m.active ?? 'Solo'}`), node('p', `Next clan: ${m.next ?? 'Solo'} · effective cycle ${m.effective_cycle}${m.forced ? ' · forced change' : ''}`, 'muted')]);
}
function renderReferrals() {
  if (!referral) { clear('referral', [node('p', 'Watch a wallet to see its referral link and earnings.', 'muted')]); return; }
  const link = `${location.origin}${appPath('/')}?sponsor=${referral.sponsor}`;
  const boxes = [['Sponsor wallet', short(referral.sponsor)], ['Referred pilots', String(referral.referred.length)], ['Sponsor earnings', money(referral.earnings, config.spaceusd)]]
    .map(([label, value]) => { const box = node('div'); box.append(node('span', label), node('strong', value)); return box; });
  const row = node('div', undefined, 'watch'); const input = document.createElement('input'); input.value = link; input.readOnly = true; input.setAttribute('aria-label', 'Referral link');
  const copy = node('button', 'Copy link', 'secondary'); copy.type = 'button'; copy.addEventListener('click', async () => {
    try { await navigator.clipboard.writeText(link); copy.textContent = 'Copied'; } catch { input.select(); document.execCommand?.('copy'); }
  });
  row.append(input, copy); clear('referral', [...boxes, row]);
}
function renderTreasury() {
  if (!treasury) { clear('treasury-summary', [node('p', 'Loading treasury…', 'muted')]); clear('treasury-balances'); return; }
  clear('treasury-summary', [['Treasury', short(treasury.treasury)], ['Collapsar / capital', short(treasury.collapsar)], ['Reward pool', short(treasury.rewardPool)], ['SpaceUSD reserve', money(treasury.rewards.reserve, config.spaceusd)]]
    .map(([label, value]) => { const box = node('div'); box.append(node('span', label), node('strong', value)); return box; }));
  clear('treasury-balances', treasury.balances.map((token, i) => {
    const row = node('div'); const route = treasury.routes[i]?.route;
    row.append(node('span', token.symbol), node('strong', `${tx.display(token.treasury, token.decimals)} treasury · ${tx.display(token.rewardPool, token.decimals)} reward pool`));
    if (route) row.append(node('small', route.map(a => a === token.address ? token.symbol : short(a)).join(' → '), 'muted'));
    return row;
  }));
}
function renderInventory() {
  const key = JSON.stringify({ watched, commanders: nftPages.commanders.items, skins: nftPages.skins.items });
  if (key === inventoryKey) return;
  inventoryKey = key;
  for (const id of selected) if (!nftPages.commanders.items.some(item => item.tokenId === id)) selected.delete(id);
  const selectedRarity = nftPages.commanders.items.find(item => selected.has(item.tokenId))?.extension.rarity;
  for (const role of ['commanders', 'skins']) {
    clear(role, nftPages[role].items.map(item => {
      const a = item.extension; const card = node('article', undefined, 'card');
      card.append(image(a), node('span', a.rarity, 'rarity'), node('h3', title(a.code)), node('code', item.tokenId));
      if (role === 'commanders') {
        const label = node('label', 'Select for fusion'); const check = document.createElement('input'); check.type = 'checkbox'; check.checked = selected.has(item.tokenId);
        check.disabled = !check.checked && (a.rarity === 'legendary' || selected.size >= 5 || (selectedRarity && a.rarity !== selectedRarity));
        label.title = a.rarity === 'legendary' ? 'Legendary is the highest tier.' : selectedRarity && a.rarity !== selectedRarity ? `Select only ${selectedRarity} commanders, or clear your selection to switch tiers.` : 'Five commanders of this tier fuse into one commander of the next tier.';
        check.addEventListener('change', () => {
          if (check.checked) {
            const rarity = nftPages.commanders.items.find(entry => selected.has(entry.tokenId))?.extension.rarity;
            if (a.rarity === 'legendary' || selected.size >= 5 || (rarity && a.rarity !== rarity)) { check.checked = false; return; }
            selected.add(item.tokenId);
          } else selected.delete(item.tokenId);
          inventoryKey = ''; renderInventory();
        }); label.prepend(check); card.append(label);
      }
      const actions = node('div', undefined, 'actions'); actions.append(button('Bind next race', () => wallet.bind(item.collection, item.tokenId))); card.append(actions);
      const form = node('form', undefined, 'listing-form'); const select = document.createElement('select'); tokenSelect(select);
      const input = document.createElement('input'); input.placeholder = 'Sale price'; input.inputMode = 'decimal'; input.required = true; input.setAttribute('aria-label', 'Sale price');
      const submit = node('button', 'List NFT', 'secondary'); submit.dataset.walletAction = ''; form.append(select, input, submit);
      form.addEventListener('submit', event => { event.preventDefault(); void run('List NFT', () => wallet.list(item.collection, item.tokenId, select.value, input.value)); });
      card.append(form); return card;
    }));
    if (!nftPages[role].items.length) $(role).append(node('p', role === 'commanders' ? 'No commanders in this wallet yet. Draw your first commander above.' : 'Earn skin credits by upgrading your ships, then claim a milestone skin.', 'muted'));
    $(`more-${role}`).hidden = !nftPages[role].next;
  }
  updateButtons();
}
function renderMarket() {
  clear('market', marketPage.items.map(listing => {
    const card = node('article', undefined, 'card'); card.append(image(listing.nft.extension), node('span', listing.nft.extension.rarity, 'rarity'), node('h3', title(listing.nft.extension.code)),
      node('p', money(listing.price, listing.payment_token)), node('code', `Seller ${listing.seller}`));
    const buy = button(`Buy listing #${listing.id}`, () => wallet.buyListing(listing.id)); if (listing.seller === wallet.address) buy.dataset.blocked = ''; card.append(buy); return card;
  })); $('more-market').hidden = !marketPage.next; updateButtons();
}
function renderClans() {
  social.clans(clanPage);
  const own = clanPage.items.find(clan => clan.id === profile?.membership.active && clan.leader === wallet.address && !clan.disband_at);
  $('clan-admin').hidden = !own; $('leadership-form').dataset.clanId = own?.id ?? '';
  $('more-clans').hidden = !clanPage.next; updateButtons();
}
function renderActivity() {
  clear('activity', activityPage.items.map(event => {
    const row = node('tr'); const role = Object.entries(config.contracts).find(([, addr]) => addr === event.contract)?.[0] ?? token(event.contract)?.symbol ?? short(event.contract);
    const details = event.attributes.filter(a => ['wallet', 'winner', 'race', 'tick', 'token_id', 'listing_id', 'claim_id', 'job_id', 'amount'].includes(a.key)).map(a => `${a.key}: ${a.value}`).join(' · ');
    const raceEnded = event.attributes.some(a => a.key === 'race_ended');
    row.append(node('td', event.height), node('td', title(role)), node('td', raceEnded ? 'Race finished' : title(event.action)), node('td', details), node('td', short(event.tx_hash), 'mono')); return row;
  })); $('more-activity').hidden = !activityPage.next;
}
async function loadNfts(role, more = false) {
  if (!watched) { nftPages[role] = { items: [], next: null }; renderInventory(); return; }
  const result = await api(`wallet/${encodeURIComponent(watched)}/nfts?collection=${role}${more && nftPages[role].next ? `&start_after=${encodeURIComponent(nftPages[role].next)}` : ''}`);
  nftPages[role] = { items: more ? [...nftPages[role].items, ...result.items] : result.items, next: result.next }; renderInventory();
}
async function loadMarket(more = false) { const data = await api(`marketplace${more && marketPage.next ? `?start_after=${marketPage.next}` : ''}`); marketPage = { items: more ? [...marketPage.items, ...data.items] : data.items, next: data.next }; renderMarket(); }
async function loadClans(more = false) { const data = await api(`clans${more && clanPage.next ? `?start_after=${clanPage.next}` : ''}`); clanPage = { ...data, items: more ? [...clanPage.items, ...data.items] : data.items }; renderClans(); }
async function loadActivity(more = false) {
  const params = new URLSearchParams(); if ($('only-mine').checked && watched) params.set('wallet', watched); if (more && activityPage.next) params.set('before', activityPage.next);
  if (!$('network-activity').checked) params.set('view', 'gameplay');
  const data = await api(`activity?${params}`); activityPage = { items: more ? [...activityPage.items, ...data.items] : data.items, next: data.next }; renderActivity();
}
async function refresh(extra = false) {
  if (refreshing) return;
  refreshing = true;
  try {
    [state, treasury] = await Promise.all([api('state'), api('treasury')]);
    if (state.deployment && state.deployment !== config.deployment) {
      notice('The game has been upgraded. Loading the new contract addresses…'); location.reload(); return;
    }
    profile = watched ? await api(`wallet/${encodeURIComponent(watched)}`) : null;
    applyConfirmedUpgrades(profile);
    connectedProfile = wallet.address ? (watched === wallet.address ? profile : await api(`wallet/${encodeURIComponent(wallet.address)}`)) : null;
    if (connectedProfile !== profile) applyConfirmedUpgrades(connectedProfile);
    referral = watched ? await api(`wallet/${encodeURIComponent(watched)}/referrals`) : null;
    renderState(); renderProfile(); renderResources(); renderReferrals(); renderTreasury(); renderGuidance(); updateButtons();
    raceExperience.update(state, profile, busy);
    reinvestment.update(profile);
    alpha.update();
    recruitment.update(profile, watched);
    const historyKey = `${watched}:${state.game.race}`;
    if (boostHistoryKey !== historyKey) { boostHistory = []; lastBoostHistory = 0; boostHistoryKey = historyKey; }
    if (watched && (extra || Date.now() - lastBoostHistory > 6000)) {
      try {
        const [result, refunds] = await Promise.all([api(`wallet/${encodeURIComponent(watched)}/boosts`), api(`wallet/${encodeURIComponent(watched)}/refunds`)]);
        for (const entry of result.items) localBoostReceipts.delete(entry.txhash);
        boostHistory = [...localBoostReceipts.values()].filter(entry => entry.wallet === watched).concat(result.items);
        lastBoostHistory = Date.now();
        $('refund-count').textContent = `${refunds.boostCount} unused boost${refunds.boostCount === 1 ? '' : 's'}${BigInt(refunds.refundCredits ?? '0') > 0n ? ' + other credits' : ''}`;
        $('refund-amount').textContent = money(refunds.amount, config.srt);
      } catch { /* Telemetry must continue if optional receipt discovery fails. */ }
    }
    renderBoostHistory();
    if (!watched) { $('refund-count').textContent = '— boosts'; $('refund-amount').textContent = '— SRT'; }
    $('refund-summary').textContent = boostHistory.some(entry => entry.status === 'refundable') ? 'Some boosts landed after the race had already ended. Recover their SRT with one transaction.' : 'Unused boosts from finished races can be recovered together.';
    if (tab === 'inventory' || extra) await Promise.all(['commanders', 'skins'].map(role => loadNfts(role)));
    if (tab === 'market' || extra) await loadMarket();
    if (tab === 'clans' || extra) await loadClans();
    if (tab === 'activity' || extra) await loadActivity();
    await social.update(profile, connectedProfile, referral, tab);
    await competition.update(state, watched, wallet.address, tab);
  } finally { refreshing = false; }
}
async function setWatched(value) {
  watched = value ? tx.terra(value.trim()) : ''; $('watch-wallet').value = watched; localStorage.setItem('racer.watch', watched);
  selected.clear(); inventoryKey = ''; nftPages = { commanders: { items: [], next: null }, skins: { items: [], next: null } };
  await refresh(true);
}
function form(id, fn) { $(id).addEventListener('submit', event => { event.preventDefault(); const values = Object.fromEntries(new FormData($(id))); void run(title(id.replace('-form', '')), () => fn(values)); }); }
const routes = { game: '/game', hangar: '/hangar', inventory: '/inventory', market: '/marketplace', clans: '/clans', profile: '/profile', treasury: '/treasury', activity: '/activity' };
function navigate(section, push = true, fetch = true) {
  tab = section;
  document.body.dataset.view = section;
  document.querySelectorAll('[data-tab]').forEach(t => t.classList.toggle('selected', t.dataset.tab === tab));
  document.querySelectorAll('.mobile-game-nav [data-jump]').forEach(t => t.classList.toggle('active', t.dataset.jump === tab));
  document.querySelectorAll('.tabs [data-tab]').forEach(t => { if (t.dataset.tab === tab) t.setAttribute('aria-current', 'page'); else t.removeAttribute('aria-current'); });
  const strip = document.querySelector('.tabs'), activeTab = strip.querySelector(`[data-tab="${tab}"]`);
  const stripBox = strip.getBoundingClientRect(), activeBox = activeTab.getBoundingClientRect();
  if (activeBox.left < stripBox.left) strip.scrollLeft += activeBox.left - stripBox.left - 8;
  else if (activeBox.right > stripBox.right) strip.scrollLeft += activeBox.right - stripBox.right + 8;
  document.querySelectorAll('[data-panel]').forEach(panel => { panel.hidden = panel.dataset.panel !== tab; });
  if (push) { const url = new URL(location.href); url.pathname = appPath(routes[section]); if (watched) url.searchParams.set('wallet', watched); history.pushState({}, '', url); }
  if (push) document.querySelector('main').scrollIntoView({ block: 'start', behavior: 'instant' });
  if (fetch) void refresh(true).catch(e => notice(e.message, true));
}
async function routeLocation(initial = false) {
  const path = routePath(), clan = path.match(/^\/clans\/(\d+)$/);
  const section = clan ? 'clans' : path === '/commander-recruitment' ? 'inventory' : Object.keys(routes).find(key => routes[key] === path) ?? 'game';
  navigate(section, false, !initial);
  if (clan && !initial) await social.openClan(Number(clan[1]), false);
  return clan ? Number(clan[1]) : null;
}

async function start() {
  config = await api('config'); wallet = new Wallet(config);
  alpha = createAlphaOnboarding({ config, wallet, watchWallet: setWatched });
  document.querySelector('.local-profile-note').textContent = 'Saved here immediately. Your public callsign updates with your next signed game action. Sponsors are finalized onchain with the first purchase.';
  const guideKey = `racer.guide.${config.chainId}`;
  try { $('flight-guide').hidden = localStorage.getItem(guideKey) === 'closed'; } catch {}
  function showGuide(show) { $('flight-guide').hidden = !show; try { localStorage.setItem(guideKey, show ? 'open' : 'closed'); } catch {} }
  $('hide-flight-guide').addEventListener('click', () => showGuide(false));
  $('show-flight-guide').addEventListener('click', () => showGuide(true));
  $('guide-connect').addEventListener('click', () => $('connect').click());
  social = createSocial({ config, wallet, api, transact: run, navigate, watchWallet: setWatched, updateButtons, editPilot: () => openPilot() });
  competition = createCompetition({ config, api, pilot: pilotName, openClan: id => social.openClan(id), openPilot: async address => { await setWatched(address); navigate('profile'); } });
  raceExperience = createRaceExperience({ config, api, pilot: pilotName, isBusy: () => busy });
  reinvestment = createReinvestment({ config, wallet, transact: run, updateButtons });
  recruitment = createRecruitment({ config, wallet, api, transact: run, updateButtons, delivered: async () => {
    inventoryKey = ''; await Promise.all(['commanders', 'skins'].map(role => loadNfts(role))); await refresh(true);
  } });
  await recruitment.load();
  document.querySelectorAll('[data-tab]').forEach(button => button.setAttribute('aria-label', button.textContent.trim()));
  document.querySelectorAll('[data-jump]').forEach(button => button.addEventListener('click', () => {
    navigate(button.dataset.jump);
  }));
  const boostNames = { speed: 'overdrive', acceleration: 'afterburner', temporary: 'warp surge', rocket: 'rocket strike', heal: 'repair' };
  const healOption = node('option', 'Repair fleet'); healOption.value = 'heal'; healOption.hidden = true; healOption.disabled = true; $('boost-form').elements.kind.append(healOption);
  const repairCard = node('button', undefined, 'boost-card'); repairCard.type = 'button'; repairCard.dataset.boost = 'heal'; repairCard.hidden = true; repairCard.setAttribute('aria-pressed', 'false'); repairCard.append(node('span', '+', 'boost-icon'), node('strong', 'Repair'), node('small', 'HP, shields & revival')); document.querySelector('.boost-grid').append(repairCard);
  const repairLimit = node('label', 'Maximum SRT to spend'), limitInput = node('input'); repairLimit.id = 'repair-limit'; repairLimit.hidden = true;
  limitInput.type = 'number'; limitInput.name = 'heal_max'; limitInput.min = '0.000001'; limitInput.step = '0.000001'; limitInput.value = '10'; limitInput.disabled = true; repairLimit.append(limitInput); $('boost-form').insertBefore(repairLimit, $('boost-form').querySelector('button'));
  setupInterface();
  $('boost-form').elements.count.max = String(0xffffffff);
  function selectBoost(kind) {
    $('boost-form').elements.kind.value = kind;
    $('boost-form').elements.count.disabled = kind === 'heal';
    $('repair-limit').hidden = kind !== 'heal'; limitInput.disabled = kind !== 'heal';
    if (kind === 'heal') $('boost-form').elements.count.value = '1';
    document.querySelectorAll('[data-boost]').forEach(button => {
      const selected = button.dataset.boost === kind;
      button.classList.toggle('selected', selected); button.setAttribute('aria-pressed', String(selected));
    });
    $('boost-form').querySelector('button').textContent = `Activate ${boostNames[kind]} ↗`;
    updateButtons();
  }
  document.querySelectorAll('[data-boost]').forEach(button => button.addEventListener('click', () => selectBoost(button.dataset.boost)));
  $('boost-form').elements.kind.addEventListener('change', event => selectBoost(event.target.value));
  $('boost-form').elements.count.addEventListener('input', updateButtons);
  limitInput.addEventListener('input', updateButtons);
  $('network').textContent = config.alpha ? 'Alpha · test assets' : config.chainId === 'spaceship-racer-local' ? 'LocalTerra · test assets' : config.chainName;
  document.querySelectorAll('[data-token-select]').forEach(tokenSelect);
  for (const [id, provider] of [['connect', 'keplr'], ['leap', 'leap']]) $(id).addEventListener('click', async () => {
    try { await wallet.connect(provider); await setWatched(wallet.address); restorePilotIdentity(); notice(`Connected ${wallet.address}`); if (!pilotIdentity()) openPilot(true); } catch (error) { notice(error.message, true); } updateButtons();
  });
  function disconnectPilot() { wallet.disconnect(); connectedProfile = null; purchasePreview = null; $('pilot-dialog').close(); $('purchase-dialog').close(); renderResources(); updateButtons(); }
  $('disconnect').addEventListener('click', () => { disconnectPilot(); notice('Wallet disconnected. Public chain views remain available.'); });
  async function changedAccount() {
    const remembered = wallet.session(); wallet.disconnect(false);
    if (!remembered) return;
    try { await wallet.connect(remembered.provider, true); await setWatched(wallet.address); restorePilotIdentity(); if (!pilotIdentity()) openPilot(true); notice(`Connected ${wallet.address}`); }
    catch { connectedProfile = null; renderResources(); updateButtons(); notice('Unlock your wallet and reconnect to switch accounts.'); }
  }
  window.addEventListener('keplr_keystorechange', () => void changedAccount());
  window.addEventListener('leap_keystorechange', () => void changedAccount());
  $('open-profile').addEventListener('click', async () => { if (wallet.address && watched !== wallet.address) await setWatched(wallet.address); navigate('profile'); });
  $('close-profile').addEventListener('click', () => $('pilot-dialog').close());
  $('pilot-dialog').addEventListener('cancel', event => { if (!pilotIdentity()) event.preventDefault(); });
  $('pilot-form').addEventListener('submit', event => {
    event.preventDefault();
    try {
      if (!wallet.address) throw new Error('Connect your wallet first.');
      const nickname = $('pilot-form').elements.nickname.value.trim();
      if (nickname.length < 2 || nickname.length > 24) throw new Error('Choose a callsign between 2 and 24 characters.');
      const previous = pilotIdentity(); const fleet = connectedProfile?.fleet;
      const input = $('pilot-form').elements.sponsor.value.trim();
      const sponsor = fleet?.sponsor || previous?.sponsor || (!fleet?.purchased && input ? tx.terra(input) : '');
      if (sponsor === wallet.address) throw new Error('Your sponsor must be another pilot.');
      localStorage.setItem(pilotKey(), JSON.stringify({ nickname, sponsor }));
      $('ship-form').elements.sponsor.value = sponsor;
      $('pilot-dialog').close(); renderResources(); renderProfile(); updateButtons();
      notice(`Profile saved, ${nickname}.`);
      if (!fleet?.purchased) document.querySelector('[data-tab="hangar"]').click();
    } catch (error) { $('pilot-error').textContent = error.message; }
  });
  $('watch-form').addEventListener('submit', event => { event.preventDefault(); void setWatched($('watch-wallet').value).catch(e => notice(e.message, true)); });
  $('my-wallet').addEventListener('click', () => { if (wallet.address) void setWatched(wallet.address).catch(e => notice(e.message, true)); });
  document.querySelectorAll('[data-tab]').forEach(b => b.addEventListener('click', () => navigate(b.dataset.tab)));
  window.addEventListener('popstate', () => void routeLocation());
  $('claim-rewards').addEventListener('click', () => run('Claim rewards', () => wallet.claimRewards()));
  form('boost-form', async v => {
    const buyer = wallet.address;
    const result = await wallet.boost(v.kind, v.kind === 'heal' ? 1 : v.count, v.heal_max);
    const event = result.events?.find(event => event.attributes.some(a => a.key === 'action' && a.value === 'queue_block_boost'));
    if (event) {
      const attrs = Object.fromEntries(event.attributes.map(a => [a.key, a.value]));
      const receipt = { wallet: buyer, race: Number(attrs.race), tick: Number(attrs.tick), paid: attrs.paid,
        txhash: result.transactionHash, status: attrs.live_activated === 'true' ? 'active' : 'queued', counts: { [v.kind === 'rocket' ? 'rockets' : v.kind]: v.kind === 'heal' ? 1 : Number(v.count) } };
      localBoostReceipts.set(receipt.txhash, receipt);
      if (watched === buyer) { boostHistory.unshift(receipt); renderBoostHistory(); }
      showSceneAction(v.kind, `${boostNames[v.kind]} ×${receipt.counts[v.kind === 'rocket' ? 'rockets' : v.kind]} · ${receipt.status === 'active' ? 'active' : 'queued'}`);
    }
    return result;
  });
  $('quote-ship').addEventListener('click', () => void previewShip());
  $('ship-form').addEventListener('change', () => void previewShip());
  $('ship-form').addEventListener('submit', async event => {
    event.preventDefault(); if (busy) return;
    if (!pilotIdentity()) { openPilot(true); return; }
    await previewShip(); if (!purchasePreview) return;
    const p = purchasePreview;
    $('purchase-title').textContent = `${p.mode === 'buy' ? 'Get your' : 'Upgrade your'} ${title(p.ship)}`;
    const summary = [node('strong', `Level ${p.tier}`, 'purchase-level'), node('h3', money(p.quote.price, config.spaceusd))];
    summary.push(upgradeComparison(p.ship, Number(p.tier)));
    const shipIndex = ships.indexOf(p.ship);
    summary.push(node('p', `Active fleet: level ${profile?.fleet.active[shipIndex] ?? 0} · owned: level ${profile?.fleet.next[shipIndex] ?? 0} → ${p.tier}. New power activates next race; this race keeps its starting snapshot.`));
    if (p.token !== config.spaceusd) summary.push(node('p', `Estimated payment: ${money(p.quote.estimated_offer, p.token)} · automatic 2% swap protection`));
    if (p.sponsor) summary.push(node('p', `First-purchase sponsor: ${short(p.sponsor)}`));
    clear('purchase-summary', summary); $('purchase-dialog').showModal();
  });
  $('close-purchase').addEventListener('click', () => $('purchase-dialog').close());
  $('confirm-purchase').addEventListener('click', () => {
    const p = purchasePreview; if (!p || busy) return;
    $('purchase-dialog').close();
    const buyer = wallet.address;
    void run(`${p.mode} ${p.ship}`, async () => {
      const result = await wallet.buyShip(p.mode, p.ship, p.tier, p.token, purchaseSponsor(), 200);
      const index = ships.indexOf(p.ship);
      confirmedUpgrades.set(`${buyer}:${p.ship}`, { wallet: buyer, index, level: Number(p.tier), effectiveRace: state.game.race + 1 });
      if (profile?.wallet === buyer) { applyConfirmedUpgrades(profile); renderFleet(); updateButtons(); }
      return result;
    });
  });
  $('recover-boosts').addEventListener('click', () => run('Recover all unused boosts', () => wallet.recoverBoosts()));
  $('fuse').addEventListener('click', () => run('Fuse commanders', async () => {
    const result = await wallet.fuse([...selected]); selected.clear(); recruitment.follow(result); return result;
  }));
  $('clear-fusion').addEventListener('click', () => { selected.clear(); inventoryKey = ''; renderInventory(); });
  for (const role of ['commanders', 'skins']) $(`more-${role}`).addEventListener('click', () => { void loadNfts(role, true).catch(e => notice(e.message, true)); });
  $('more-market').addEventListener('click', () => { void loadMarket(true).catch(e => notice(e.message, true)); });
  $('more-clans').addEventListener('click', () => { void loadClans(true).catch(e => notice(e.message, true)); });
  $('more-activity').addEventListener('click', () => { void loadActivity(true).catch(e => notice(e.message, true)); });
  $('only-mine').addEventListener('change', () => { void loadActivity().catch(e => notice(e.message, true)); });
  $('network-activity').addEventListener('change', () => { void loadActivity().catch(e => notice(e.message, true)); });
  form('create-clan', async v => {
    const result = await wallet.execute([tx.clanAction(config, 'create', v)], 'create clan');
    const event = result.events?.find(event => event.attributes.some(a => a.key === 'action' && a.value === 'queue_create_clan'));
    const id = Number(event?.attributes.find(a => a.key === 'clan_id')?.value);
    if (id) await social.openClan(id);
    return result;
  });
  for (const [id, action] of [['leave-clan', 'leave'], ['cancel-clan', 'cancel_change']]) $(id).addEventListener('click', () => run(title(action), () => wallet.execute([tx.clanAction(config, action)], action)));
  form('leadership-form', v => wallet.execute([tx.clanAction(config, v.action, { ...v, id: $('leadership-form').dataset.clanId })], 'clan leadership'));
  $('pause-game').addEventListener('click', () => run('Pause game', () => wallet.execute([tx.instruction(config.contracts.pause, { set_paused: { paused: true } })], 'pause game', true)));
  $('resume-game').addEventListener('click', () => run('Resume game', () => wallet.execute([tx.instruction(config.contracts.pause, { set_paused: { paused: false } })], 'resume game', true)));
  const params = new URLSearchParams(location.search);
  try { const saved = params.get('wallet') ?? localStorage.getItem('racer.watch'); if (saved) watched = tx.terra(saved); } catch { watched = ''; }
  if (params.get('sponsor')) { try { $('ship-form').elements.sponsor.value = tx.terra(params.get('sponsor')); } catch { /* Invalid sponsor links are ignored. */ } }
  $('watch-wallet').value = watched;
  let restored = false;
  const remembered = wallet.session();
  if (remembered && window[remembered.provider]) {
    try {
      await wallet.connect(remembered.provider, true); restored = true;
      if (!params.has('wallet')) { watched = wallet.address; $('watch-wallet').value = watched; }
    } catch { wallet.disconnect(false); }
  }
  const initialClan = await routeLocation(true);
  await refresh(true); notice(restored ? `Flight deck ready. Reconnected ${wallet.address}.` : 'Flight deck ready. Connect your wallet to sign transactions.');
  if (restored) restorePilotIdentity();
  if (restored && !pilotIdentity()) openPilot(true);
  if (initialClan) await social.openClan(initialClan, false);
  setInterval(renderRaceTimer, 250);
  const poll = async () => { await refresh().catch(() => {}); setTimeout(poll, document.hidden ? 15000 : 6000); }; setTimeout(poll, 6000);
}
void start().catch(error => notice(error.message, true));
