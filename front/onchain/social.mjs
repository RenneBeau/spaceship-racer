import * as tx from './transactions.mjs';
import { fleetHealth } from '../../src/onchain/leaderboards.mjs';
import { healthBar } from './battle-ui.mjs';
import { appPath } from './paths.mjs';
const $ = id => document.getElementById(id);
const el = (tag, text, cls) => { const e = document.createElement(tag); if (text !== undefined) e.textContent = text; if (cls) e.className = cls; return e; };
const short = value => value ? `${value.slice(0, 10)}…${value.slice(-6)}` : 'Solo';
const stat = (label, value) => { const box = el('div', undefined, 'stat'); box.append(el('span', label), el('strong', value)); return box; };

export function createSocial({ config, wallet, api, transact, navigate, watchWallet, updateButtons, editPilot }) {
  let profile = null, connected = null, clanData = null, clanItems = [], clanConfig = null, history = null, historyWallet = '', historyAt = 0, detailAt = 0, selectedClan = null;
  let rewardsAt = 0, rewardData = null, rewardLimit = 20;
  const rewardsPanel = el('article', undefined, 'panel'); rewardsPanel.id = 'clan-reward-panel';
  rewardsPanel.append(el('h2', 'Clan rewards earned'), el('div', undefined, 'clan-reward-summary'), el('div', undefined, 'clan-reward-history'));
  const moreRewards = el('button', 'Older cycles', 'secondary'); moreRewards.type = 'button'; moreRewards.hidden = true; rewardsPanel.append(moreRewards);
  $('clan-detail-hero').insertAdjacentElement('afterend', rewardsPanel);
  const name = (address, provided = null) => { try { const saved = JSON.parse(localStorage.getItem(`racer.pilot.${config.chainId}.${address}`) ?? 'null')?.nickname; const indexed = profile?.wallet === address ? profile.nickname : null; return address === wallet.address ? saved || provided || indexed || short(address) : provided || indexed || saved || short(address); } catch { return provided || short(address); } };
  const money = (amount, token = config.spaceusd) => { const a = config.tokens.find(a => a.address === token); return `${tx.display(amount, a?.decimals ?? 6)} ${a?.symbol ?? ''}`; };
  function button(text, fn, signing = false) {
    const b = el('button', text, 'secondary'); b.type = 'button'; if (signing) b.dataset.walletAction = '';
    b.addEventListener('click', () => { void fn(); }); return b;
  }
  const openProfile = async address => { await watchWallet(address); navigate('profile'); };
  const clanAction = (action, values = {}) => transact(action.replaceAll('_', ' '), () => wallet.execute([tx.clanAction(config, action, values)], `clan ${action}`));
  function renderDirectory() {
    const needle = $('clan-search').value.trim().toLowerCase();
    $('clans').replaceChildren(...clanItems.filter(c => `${c.name} ${c.code}`.toLowerCase().includes(needle)).map(clan => {
      const card = el('article', undefined, 'card clan-card'), insignia = el('div', clan.code.slice(0, 2), 'clan-insignia');
      card.append(insignia, el('span', clan.code, 'badge'), el('h2', clan.name), el('p', `${clan.members.length} pilots · ${clan.next_members.length} next cycle`), el('small', `Led by ${name(clan.leader)}`));
      card.append(button('View squadron ↗', () => openClan(clan.id)));
      return card;
    }));
    if (clanConfig) {
      const tiers = connected?.fleet.next.reduce((sum, n) => sum + n, 0) ?? 0;
      const blocked = tiers < clanConfig.min_create_tiers || !!connected?.membership.active || !!connected?.membership.next;
      $('clan-create-gate').textContent = `Requires ${clanConfig.min_create_tiers} total ship levels. Your fleet: ${tiers}. Clans hold up to ${clanConfig.max_members} pilots. Leader premium: ${clanConfig.leader_premium_bps / 100}%.`;
      const submit = $('create-clan').querySelector('button'); if (blocked) submit.dataset.blocked = ''; else delete submit.dataset.blocked;
      const membership = connected?.membership;
      const pending = membership && membership.effective_cycle > clanConfig.current_cycle && membership.active !== membership.next;
      const own = clanItems.find(c => c.id === membership?.next);
      const canLeave = membership?.next && !pending && own?.next_leader !== wallet.address;
      for (const [id, blocked] of [['leave-clan', !canLeave], ['cancel-clan', !pending || membership?.forced || own?.next_leader === wallet.address]]) {
        if (blocked) $(id).dataset.blocked = ''; else delete $(id).dataset.blocked;
      }
    }
    $('my-clan').disabled = !(connected?.membership.active || connected?.membership.next || profile?.membership.active);
    updateButtons();
  }
  function renderClan() {
    if (!clanData) return;
    const { clan, members, configuration, cycle } = clanData;
    const hero = $('clan-detail-hero'); hero.replaceChildren();
    const identity = el('div', clan.code.slice(0, 2), 'clan-insignia large');
    const copy = el('div'); copy.append(el('span', clan.code, 'badge'), el('h1', clan.name), el('p', `Led by ${name(clan.leader, clanData.leaderNickname)} · Cycle ${cycle}`));
    if (clan.next_leader !== clan.leader) copy.append(el('p', `Next leader: ${name(clan.next_leader)} · cycle ${clan.effective_cycle}`, 'muted'));
    const stats = el('div', undefined, 'mini-stats');
    const score = members.filter(m => m.active).reduce((sum, m) => sum + BigInt(m.score.score), 0n);
    stats.append(stat('Members', `${clan.members.length} / ${configuration.max_members}`), stat('Cycle points', tx.display(score.toString(), 4)), stat('Leader premium', `${configuration.leader_premium_bps / 100}%`));
    hero.append(identity, copy, stats);
    $('clan-roster').replaceChildren(...members.map(member => {
      const row = el('div', undefined, 'roster-row');
      row.append(button(`${member.leader ? '♛ ' : ''}${name(member.wallet, member.nickname)}`, () => openProfile(member.wallet)),
        el('span', `${member.fleet.next.reduce((a, b) => a + b, 0)} fleet levels`), el('strong', `${tx.display(member.score.score, 4)} pts`),
        el('small', member.active && member.next ? 'Active' : member.active ? 'Leaving next cycle' : 'Joining next cycle'));
      if (wallet.address === clan.leader && member.wallet !== clan.leader && !clan.disband_at) {
        if (member.next) row.append(button('Promote', () => clanAction('transfer_lead', { id: clan.id, wallet: member.wallet }), true));
        if (member.active && member.next) row.append(button('Remove', () => clanAction('kick', { id: clan.id, wallet: member.wallet }), true));
      }
      return row;
    }));
    const actions = $('clan-detail-actions'); actions.replaceChildren();
    const membership = connected?.membership;
    const pending = membership && membership.effective_cycle > configuration.current_cycle && membership.active !== membership.next;
    if (clan.disband_at) actions.append(el('p', `Squadron dismantles in cycle ${clan.disband_at}.`, 'muted'));
    else if (connected?.membership.active === clan.id || connected?.membership.next === clan.id) {
      if (!pending && clan.next_leader !== wallet.address) actions.append(button('Queue leave', () => clanAction('leave'), true));
      if (pending && !membership.forced && clan.next_leader !== wallet.address) actions.append(button('Cancel queued membership change', () => clanAction('cancel_change'), true));
      if (pending) actions.append(el('p', `Your membership change is queued for cycle ${membership.effective_cycle}.`, 'muted'));
      if (clan.next_leader === wallet.address) actions.append(el('p', 'Promote another member before leaving, or dismantle the squadron.', 'muted'));
      if (wallet.address === clan.leader) actions.append(button('Dismantle squadron', () => clanAction('dismantle', { id: clan.id }), true));
    } else if (pending) actions.append(el('p', `Your existing membership change takes effect in cycle ${membership.effective_cycle}.`, 'muted'));
    else if (clan.next_members.length < configuration.max_members) actions.append(button('Join this squadron ↗', () => clanAction('join', { code: clan.code }), true));
    else actions.append(el('p', 'This squadron has no next-cycle slots available.', 'muted'));
    const share = el('input'); share.readOnly = true; share.value = `${location.origin}${appPath(`/clans/${clan.id}`)}`; share.setAttribute('aria-label', 'Clan page link'); actions.append(share);
    updateButtons();
  }
  async function openClan(id, push = true) {
    if (selectedClan !== Number(id)) { rewardData = null; rewardsAt = 0; rewardLimit = 20; }
    selectedClan = Number(id); navigate('clans', false);
    $('clan-directory').hidden = true; $('clan-detail').hidden = false;
    $('clan-detail-hero').replaceChildren(el('p', 'Opening squadron…', 'muted'));
    if (push) historyPush(`/clans/${id}`);
    try { clanData = await api(`clans/${id}`); detailAt = Date.now(); renderClan(); await loadRewards(); }
    catch (error) { $('clan-detail-hero').replaceChildren(el('p', error.message, 'muted')); }
  }
  function renderRewards() {
    const summary = rewardsPanel.querySelector('.clan-reward-summary'), rows = rewardsPanel.querySelector('.clan-reward-history');
    if (!rewardData) { summary.replaceChildren(el('p', 'Loading verified clan rewards…', 'muted')); rows.replaceChildren(); return; }
    const sync = rewardData.sync;
    summary.replaceChildren(stat('Net SpaceUSD earned', money(rewardData.totals.spaceusd)), stat('SRT earned', money(rewardData.totals.srt, config.srt)), stat('Leader premiums included', money(rewardData.totals.premium)),
      el('p', sync.indexedCycles < sync.totalCycles ? `Syncing historical settlements: ${sync.indexedCycles}/${sync.totalCycles} cycles. These are indexed totals so far.` : sync.fullHistory ? `All ${sync.totalCycles} completed cycles indexed. Rewards are recorded at cycle settlement.` : `Indexed history starts at block ${sync.fromHeight}; earlier cycles are outside this index.`, 'muted'), el('small', rewardData.basis, 'muted'));
    if (sync.error) summary.append(el('small', `History sync: ${sync.error}`, 'muted'));
    rows.replaceChildren(...rewardData.cycles.slice(0, rewardLimit).map(cycle => {
      const row = el('div', undefined, 'history-row'); row.append(el('strong', `Cycle ${cycle.cycle}`), el('span', money(cycle.spaceusd)), el('span', money(cycle.srt, config.srt)), el('small', new Date(cycle.time).toLocaleString())); return row;
    }));
    if (!rewardData.cycles.length) rows.append(el('p', 'No completed-cycle rewards recorded for this clan yet.', 'muted'));
    moreRewards.hidden = rewardData.cycles.length <= rewardLimit;
  }
  async function loadRewards() {
    renderRewards();
    try { const data = await api(`clans/${selectedClan}/rewards`); if (data.clanId !== selectedClan) return; rewardData = data; rewardsAt = Date.now(); renderRewards(); }
    catch (error) { rewardsPanel.querySelector('.clan-reward-summary').textContent = error.message; }
  }
  moreRewards.addEventListener('click', () => { rewardLimit += 20; renderRewards(); });
  function historyPush(path) { const url = new URL(location.href); url.pathname = appPath(path); window.history.pushState({}, '', url); }
  function renderProfile(referrals) {
    const own = profile && profile.wallet === wallet.address;
    $('edit-pilot').hidden = !own;
    if (!profile) {
      $('profile-hero').replaceChildren(el('h2', 'Choose a pilot'), el('p', 'Connect your wallet or use the Pilot scanner to explore a wallet profile.'));
      for (const id of ['profile-stats', 'profile-fleet', 'profile-equipment', 'profile-rewards', 'profile-clan', 'profile-referrals', 'profile-victories', 'profile-claims']) $(id).replaceChildren();
      return;
    }
    const hero = $('profile-hero'); hero.replaceChildren(el('div', name(profile.wallet).slice(0, 2).toUpperCase(), 'pilot-avatar'), el('h2', name(profile.wallet)), el('code', profile.wallet));
    const share = button('Copy profile link', async () => {
      const link = `${location.origin}${appPath('/profile')}?wallet=${profile.wallet}`;
      try { await navigator.clipboard.writeText(link); share.textContent = 'Copied'; } catch { window.prompt('Copy profile link', link); }
    }); hero.append(share);
    const tiers = profile.fleet.next.reduce((a, b) => a + b, 0);
    $('profile-stats').replaceChildren(stat('Fleet levels', tiers), stat('Cycle points', tx.display(profile.score.score, 4)), stat('Indexed victories', history?.wins ?? '…'), stat('Sponsor', short(profile.fleet.sponsor)));
    const vitals = profile.player?.race === profile.race ? fleetHealth(profile.player) : null;
    $('profile-fleet').replaceChildren(...['racer', 'tank', 'fighter'].map((ship, i) => {
      const card = el('div'); const img = el('img');
      const code = profile.equipment[`${ship}_skin`]?.active?.attributes?.code;
      img.src = appPath(`/assets/ship-skins/${code && /^[a-z0-9_-]+$/.test(code) ? code : `${ship}1`}.png`); img.alt = ship;
      card.append(img, el('strong', `${ship.toUpperCase()} · LVL ${profile.fleet.next[i]}`), el('small', `Active level ${profile.fleet.active[i]}`));
      if (vitals) card.append(healthBar('HP', vitals.ships[i].hp, vitals.ships[i].maxHp), healthBar('Shield', vitals.ships[i].shield, vitals.ships[i].maxShield, true));
      return card;
    }));
    $('profile-equipment').replaceChildren(...Object.entries(profile.equipment).map(([slot, position]) => el('p', `${slot.replaceAll('_', ' ')}: ${position.active?.attributes.code.replaceAll('_', ' ') ?? 'Empty'}${position.pending ? ' · change queued' : ''}`)));
    $('profile-rewards').replaceChildren(el('h3', money(profile.credit.spaceusd)), el('h3', money(profile.credit.srt, config.srt)));
    const clan = clanItems.find(c => c.id === profile.membership.active);
    $('profile-clan').replaceChildren(profile.membership.active ? button(`${clan?.name ?? `Clan #${profile.membership.active}`} ↗`, () => openClan(profile.membership.active)) : button('Find a squadron ↗', () => navigate('clans')));
    if (clan?.leader === profile.wallet && clanConfig) $('profile-clan').append(el('p', `Leader premium: ${clanConfig.leader_premium_bps / 100}% of the clan's gross cycle rewards.`, 'muted'));
    const referralsBox = $('profile-referrals'); referralsBox.replaceChildren(el('p', `Sponsor earnings: ${money(referrals?.earnings ?? '0')}`));
    const link = el('input'); link.readOnly = true; link.value = `${location.origin}${appPath('/')}?sponsor=${profile.wallet}`; link.setAttribute('aria-label', 'Profile referral link'); referralsBox.append(link);
    referralsBox.append(el('p', profile.fleet.sponsor ? `Your sponsor: ${profile.fleet.sponsor}` : profile.fleet.purchased ? 'No sponsor · registration closed after first purchase.' : 'Set an optional sponsor in your pilot settings before your first purchase.', 'muted'));
    for (const address of referrals?.referred ?? []) referralsBox.append(button(name(address), () => openProfile(address)));
    if (!referrals?.referred.length) referralsBox.append(el('p', 'Your invited wingmates will appear here.', 'muted'));
    $('profile-victories').replaceChildren(...(history?.victories.length ? history.victories.map(win => {
      const row = el('div', undefined, 'history-row'); row.append(el('strong', `Race #${win.race}`), el('span', `×${win.result.multiplier} · cycle ${win.result.cycle}`), el('small', new Date(win.time).toLocaleString())); return row;
    }) : [el('p', history ? 'Your race victories will appear here.' : 'Loading victories…', 'muted')]));
    $('profile-claims').replaceChildren(...(history?.claims.length ? history.claims.map(claim => {
      const row = el('div', undefined, 'history-row'); row.append(el('strong', money(claim.spaceusd)), el('span', money(claim.srt, config.srt)), el('small', new Date(claim.time).toLocaleString())); return row;
    }) : [el('p', history ? 'Your claimed rewards will appear here.' : 'Loading reward history…', 'muted')]));
    updateButtons();
  }
  $('clan-search').addEventListener('input', renderDirectory);
  $('back-clans').addEventListener('click', () => { selectedClan = null; clanData = null; $('clan-detail').hidden = true; $('clan-directory').hidden = false; historyPush('/clans'); });
  $('my-clan').addEventListener('click', () => { const id = connected?.membership.active || connected?.membership.next || profile?.membership.active; if (id) void openClan(id); });
  $('edit-pilot').addEventListener('click', editPilot);
  $('profile-claim').addEventListener('click', () => void transact('Claim rewards', () => wallet.claimRewards()));
  return {
    openClan,
    clans(data) { clanItems = data.items; clanConfig = data.configuration; renderDirectory(); },
    async update(data, connectedData, referrals, tab) {
      profile = data; connected = connectedData; renderDirectory();
      if (tab === 'profile') {
        if (profile && (historyWallet !== profile.wallet || Date.now() - historyAt > 10000)) {
          if (historyWallet !== profile.wallet) history = null;
          historyWallet = profile.wallet;
          try { history = await api(`wallet/${profile.wallet}/history`); historyAt = Date.now(); } catch { /* Keep the profile usable while history catches up. */ }
        }
        renderProfile(referrals);
      }
      if (tab === 'clans' && selectedClan && Date.now() - detailAt > 6000) {
        try { clanData = await api(`clans/${selectedClan}`); detailAt = Date.now(); renderClan(); } catch { /* Retry next refresh. */ }
      }
      if (tab === 'clans' && selectedClan && Date.now() - rewardsAt > 10000) await loadRewards();
    },
  };
}
