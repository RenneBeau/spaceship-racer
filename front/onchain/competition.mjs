import { node, healthBar, ratio, units } from './battle-ui.mjs';
import { display } from './transactions.mjs';
const $ = id => document.getElementById(id);
export function createCompetition({ config, api, pilot, openPilot, openClan }) {
  let scope = 'race', group = 'racers', watched = '', owner = '', state = null, frame = null, version = 0, lastKey = '', cursor = null;
  let combatWallet = '', combatCursor = null, combatAt = 0, combatLoading = false, gasAt = 0;
  const queue = []; let animating = false;
  const seenHits = new Set(), hitRows = new Map();
  const hitKey = hit => `${hit.race}:${hit.tick}:${hit.attacker}:${hit.ordinal}`;
  function renderBoard() {
    if (!frame) return;
    $('leaderboard-context').textContent = scope === 'race' ? `Race #${frame.race} · ${frame.preview ? 'preview distance' : 'resolved distance'}${frame.finalizing ? ' · finalizing' : ''}` : `Cycle #${frame.cycle} · winner points`;
    $('leaderboard-self').textContent = frame.self ? `${watched === owner ? 'Your' : 'Watched pilot'} rank: #${frame.self.rank}` : 'Buy a ship to join the next race.';
    $('leaderboard-rows').replaceChildren(...frame.items.map(row => {
      const own = row.wallet === watched || row.members?.includes(watched), dead = row.health && BigInt(row.health.maxHp) > 0n && BigInt(row.health.hp) === 0n;
      const item = node('div', undefined, `score-row${own ? ' own' : ''}${dead ? ' eliminated' : ''}`);
      const identity = node('div', undefined, 'score-identity'), button = node('button', group === 'teams' && !row.solo ? row.name : pilot(row.wallet, row.nickname ?? (row.solo && row.name !== row.wallet ? row.name : null)), 'pilot-link');
      button.type = 'button'; button.title = row.wallet ?? row.code ?? row.name;
      button.addEventListener('click', () => { if (group === 'teams' && !row.solo) void openClan(row.clanId); else void openPilot(row.wallet); });
      identity.append(button, node('small', group === 'teams' ? row.solo ? 'SOLO PILOT' : `${row.activeMembers} racing · ${row.members.length} cycle pilots` : row.clanId ? `Clan #${row.clanId}` : 'Solo pilot'));
      const metric = node('div', undefined, 'score-metric'); metric.append(node('strong', scope === 'race' ? units(row.distance) : display(row.points, 4)), node('small', scope === 'race' ? 'DISTANCE' : 'CYCLE POINTS'));
      if (scope === 'race') metric.append(node('small', `${(row.winChanceBps / 100).toFixed(2)}% win odds`));
      if (scope === 'race' && group === 'racers' && watched && row.wallet !== watched) metric.append(node('small', `${(row.rocketChanceBps / 100).toFixed(2)}% rocket target odds`, 'target-odds'));
      const vitals = node('div', undefined, 'score-vitals');
      if (row.health) vitals.append(healthBar(dead ? 'Eliminated · HP' : 'HP', row.health.hp, row.health.maxHp), healthBar('Tank shield', row.health.combatShield, row.health.maxCombatShield, true));
      else vitals.append(node('small', 'Joins next race', 'muted'));
      item.append(node('b', String(row.rank).padStart(2, '0'), 'score-rank'), identity, metric, vitals); return item;
    }));
    if (!frame.items.length) $('leaderboard-rows').append(node('p', 'No participants in this view yet.', 'muted'));
    $('leaderboard-more').hidden = cursor === null;
  }
  async function load(more = false) {
    const current = ++version;
    const params = new URLSearchParams({ scope, group, limit: '50' }); if (watched) params.set('wallet', watched); if (more && cursor !== null) params.set('offset', cursor);
    try {
      const data = await api(`leaderboards?${params}`); if (current !== version) return;
      if (state?.game && (data.cycle !== state.game.cycle || scope === 'race' && data.race !== state.game.race)) { lastKey = ''; $('leaderboard-context').textContent = 'Updating race standings…'; return; }
      frame = { ...data, items: more && frame ? [...frame.items, ...data.items] : data.items }; cursor = data.next; renderBoard();
    } catch (error) { if (current === version) $('leaderboard-context').textContent = error.message; }
  }
  for (const button of document.querySelectorAll('[data-board-scope]')) button.addEventListener('click', () => {
    scope = button.dataset.boardScope; document.querySelectorAll('[data-board-scope]').forEach(b => b.classList.toggle('selected', b === button)); void load();
  });
  for (const button of document.querySelectorAll('[data-board-group]')) button.addEventListener('click', () => {
    group = button.dataset.boardGroup; document.querySelectorAll('[data-board-group]').forEach(b => b.classList.toggle('selected', b === button)); void load();
  });
  $('leaderboard-more').addEventListener('click', () => void load(true));
  function animateNext() {
    if (animating || !queue.length) return; animating = true;
    const hit = queue.shift(), incoming = hit.target === watched;
    const popup = $('rocket-impact'); popup.hidden = false; popup.dataset.hitId = hit.id;
    $('rocket-impact-title').textContent = hit.preview ? 'Rocket impact · preview' : hit.destroyed ? incoming ? 'Your fleet was taken down' : 'TAKEDOWN' : incoming ? 'Incoming rocket hit' : hit.critical ? 'CRITICAL HIT' : 'Rocket hit confirmed';
    popup.dataset.target = hit.target;
    $('rocket-target').textContent = `${pilot(hit.attacker, hit.attackerNickname)} → ${pilot(hit.target, hit.targetNickname)}`;
    $('rocket-damage').textContent = `${units(hit.hp_lost)} HP lost · ${units(hit.shield_lost)} shield absorbed`;
    const hp = healthBar('Target HP', hit.hp_before, hit.max_hp), shield = healthBar('Target shields', hit.shield_before, hit.shield_before, true);
    $('rocket-impact-bars').replaceChildren(hp, shield);
    popup.classList.remove('impact-playing'); void popup.offsetWidth; popup.classList.add('impact-playing');
    requestAnimationFrame(() => requestAnimationFrame(() => {
      hp.querySelector('.vital-track span').style.width = `${ratio(hit.hp_after, hit.max_hp)}%`;
      hp.dataset.current = hit.hp_after; hp.querySelector('.vital-track').setAttribute('aria-valuenow', String(ratio(hit.hp_after, hit.max_hp)));
      hp.querySelector('.vital-label strong').textContent = `${units(hit.hp_after)} / ${units(hit.max_hp)}`;
      shield.querySelector('.vital-track span').style.width = `${ratio(hit.shield_after, hit.shield_before)}%`;
      shield.dataset.current = hit.shield_after; shield.querySelector('.vital-track').setAttribute('aria-valuenow', String(ratio(hit.shield_after, hit.shield_before)));
      shield.querySelector('.vital-label strong').textContent = `${units(hit.shield_after)} remaining`;
    }));
    setTimeout(() => { popup.hidden = true; animating = false; animateNext(); }, 2400);
  }
  async function loadCombat() {
    if (!watched || combatLoading || Date.now() - combatAt < 1500) return;
    combatLoading = true; const address = watched;
    try {
      if (combatWallet !== address) {
        seenHits.clear(); hitRows.clear();
        combatWallet = address; const saved = localStorage.getItem(`racer.hits.${config.deployment}.${address}`); combatCursor = saved === null ? null : Number(saved);
      }
      const hadCursor = combatCursor !== null;
      const result = await api(`combat?wallet=${address}${hadCursor ? `&after=${combatCursor}` : ''}`);
      if (address !== watched) return;
      combatCursor = result.next; localStorage.setItem(`racer.hits.${config.deployment}.${address}`, String(combatCursor));
      const fresh = [...result.items, ...(result.previewItems ?? [])].filter(hit => {
        const key = hitKey(hit);
        if (seenHits.has(key)) { if (!hit.preview && hitRows.has(key)) hitRows.get(key).querySelector('small').textContent = `Race ${hit.race} · tick ${hit.tick} · confirmed`; return false; }
        seenHits.add(key); return true;
      });
      if (hadCursor) queue.push(...fresh.slice(-20));
      if (fresh.length) {
        const rows = fresh.slice(-8).reverse().map(hit => {
          const row = node('div', undefined, 'hit-history-row'); row.append(node('strong', `${pilot(hit.attacker, hit.attackerNickname)} → ${pilot(hit.target, hit.targetNickname)}`), node('span', `−${units(hit.hp_lost)} HP · −${units(hit.shield_lost)} shield${hit.destroyed ? ' · TAKEDOWN' : ''}`), node('small', `Race ${hit.race} · tick ${hit.tick}${hit.preview ? ' · preview' : ' · confirmed'}`)); hitRows.set(hitKey(hit), row); return row;
        });
        if (!hadCursor) $('combat-history').replaceChildren(...rows); else $('combat-history').prepend(...rows);
        while ($('combat-history').children.length > 12) $('combat-history').lastElementChild.remove();
      }
      combatAt = Date.now(); animateNext();
    } catch { /* Keep gameplay interactive if the indexer is catching up. */ }
    finally { combatLoading = false; }
  }
  async function gas() {
    if (Date.now() - gasAt < 60000) return;
    try {
      const data = await api('keeper/gas'); gasAt = Date.now();
      $('keeper-gas').replaceChildren(node('p', `${data.transactions} recent transactions · ${Math.round(data.windowSeconds / 60)} minute sample`),
        node('strong', `${Number(data.averageGasUsed ?? 0).toLocaleString()} gas used / transaction`),
        node('p', `${Number(data.averageGasWanted ?? 0).toLocaleString()} average gas limit · ${data.failures ?? 0} failed transactions in the sample`),
        node('p', Object.entries(data.fees).map(([denom, amount]) => `${display(amount, 6)} ${denom === 'uusd' ? 'test USTC' : denom} paid`).join(' · ')),
        node('p', Object.entries(data.estimatedFeesPerHour ?? {}).map(([denom, amount]) => `Sample rate: ${Number(display(amount, 6)).toLocaleString(undefined, { maximumFractionDigits: 2 })} ${denom === 'uusd' ? 'test USTC' : denom}/hour`).join(' · ')), node('small', data.note, 'muted'));
    } catch (error) { $('keeper-gas').textContent = error.message; }
  }
  return { async update(currentState, address, signingAddress, tab) {
    state = currentState; watched = address; owner = signingAddress;
    const key = `${scope}:${group}:${watched}:${state.height}`;
    if (tab === 'game' && key !== lastKey) { lastKey = key; await load(); }
    await loadCombat(); if (tab === 'treasury') await gas();
  } };
}
