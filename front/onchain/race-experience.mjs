import { node, healthBar } from './battle-ui.mjs';
import { fleetHealth } from '../../src/onchain/leaderboards.mjs';
import { display } from './transactions.mjs';
import { appPath } from './paths.mjs';
const $ = id => document.getElementById(id), ships = ['racer', 'tank', 'fighter'];
const name = ship => ship[0].toUpperCase() + ship.slice(1);
export function shipAppearance(equipment, ship) {
  const code = equipment?.[`${ship}_skin`]?.active?.attributes?.code;
  return code && /^[a-z0-9_-]+$/.test(code) ? code : `${ship}1`;
}
export function fleetCards({ fleet, equipment, player }, race) {
  const current = player?.race === race ? player : null, health = current ? fleetHealth(current) : null;
  return ships.map((ship, i) => {
    const level = current?.tiers[i] ?? fleet?.active[i] ?? 0, vital = health?.ships[i];
    const card = node('div', undefined, `race-ship ${ship}${vital && BigInt(vital.hp) === 0n && level ? ' destroyed' : ''}${level ? '' : ' absent'}`);
    const img = node('img'); img.src = appPath(`/assets/ship-skins/${shipAppearance(equipment, ship)}.png`); img.alt = name(ship);
    card.append(node('strong', `${name(ship)} · ${level ? `LVL ${level}` : 'Not owned'}`), img);
    if (vital && level) card.append(healthBar('HP', vital.hp, vital.maxHp), healthBar(i === 1 ? 'Shield' : 'Shield stat', vital.shield, vital.maxShield, true));
    else card.append(node('small', level ? 'Race snapshot' : 'Buy in the hangar'));
    return card;
  });
}
function updateFleetScene(container, profile, race) {
  const next = fleetCards(profile, race);
  if (container.children.length !== next.length) { container.replaceChildren(...next); return; }
  next.forEach((ship, i) => {
    const previous = container.children[i];
    previous.className = ship.className;
    previous.querySelector('strong').textContent = ship.querySelector('strong').textContent;
    const src = ship.querySelector('img').getAttribute('src');
    if (previous.querySelector('img').getAttribute('src') !== src) previous.querySelector('img').src = src;
    previous.querySelectorAll(':scope > .vital-bar, :scope > small').forEach(element => element.remove());
    previous.append(...ship.querySelectorAll(':scope > .vital-bar, :scope > small'));
  });
}
export function createRaceExperience({ api, config, pilot, isBusy = () => false }) {
  let observedRace = null, pendingRace = null, checking = false, shownRace = null, busy = false;
  $('close-race-end').addEventListener('click', () => $('race-end-dialog').close());
  async function results() {
    if (pendingRace === null || checking) return;
    // Do not interrupt a wallet confirmation, draw or other modal.
    if (busy || isBusy() || document.querySelector('dialog[open]')) return;
    checking = true;
    try {
      const data = await api(`races/${pendingRace}`), result = data.result;
      $('race-end-title').textContent = `Race #${result.race} finished`;
      const winner = data.winner;
      const summary = [node('h3', result.winner ? pilot(result.winner, winner?.nickname) : 'No winner')];
      summary.push(node('p', winner?.clan ? `Clan: ${winner.clan.name}` : 'Clan: Solo'));
      summary.push(node('p', `${result.multiplier} base cycle points · ${result.ticks * (lastTickSeconds ?? 6)} seconds of race time`));
      $('race-end-summary').replaceChildren(...summary);
      $('winner-fleet').replaceChildren(...(winner?.fleet ? fleetCards(winner, result.race) : [node('p', 'Historical fleet appearance is not available.', 'muted')]));
      if (busy || isBusy() || document.querySelector('dialog[open]')) return;
      $('race-end-dialog').showModal(); shownRace = result.race; pendingRace = null;
    } catch { /* Result/indexed appearance may arrive one block later. */ }
    finally { checking = false; }
  }
  let lastTickSeconds = 6;
  return { update(state, profile, transacting = false) {
    busy = transacting;
    lastTickSeconds = state.configuration.parameters.tick_seconds;
    const phase = $('scene-state');
    const status = state.chainHealth?.healthy === false ? 'Waiting for chain' : state.pause.paused ? 'Paused' : state.game.race === 0 ? 'Waiting for the first fleet' : state.game.replay_until ? 'Resolving actions' : ['waiting', 'movement', 'combat'].includes(state.game.phase) ? 'Race in progress' : 'Preparing next race';
    if (phase.textContent !== status) phase.textContent = status;
    phase.dataset.mode = state.chainHealth?.healthy === false || state.pause.paused ? 'paused' : 'active';
    $('scene-pilot-name').textContent = profile ? pilot(profile.wallet, profile.nickname) : 'No pilot selected';
    const commander = profile?.player?.race === state.game.race ? profile.player.commander?.code : profile?.equipment?.commander?.active?.attributes?.code;
    const portrait = $('scene-commander');
    portrait.hidden = !commander || !/^[a-z0-9_-]+$/.test(commander);
    const src = appPath(`/assets/commander-skins/${commander}.png`);
    if (!portrait.hidden && portrait.getAttribute('src') !== src) portrait.src = src;
    const estimate = state.rewardEstimate;
    if (estimate) {
      const amount = (value, address) => `${display(value, config.tokens.find(t => t.address === address)?.decimals ?? 6)}`;
      const pools = [['SpaceUSD cycle pool', estimate.available.spaceusd, config.spaceusd], ['SRT cycle pool', estimate.available.srt, config.srt]];
      $('cycle-pools').replaceChildren(...pools.map(([label, value, address]) => {
        const card = node('div'); card.append(node('small', label), node('strong', amount(value, address))); return card;
      }));
      const usd = estimate.perPoint.spaceusd, srt = estimate.perPoint.srt;
      $('cycle-estimate').textContent = usd === null ? 'Point value will appear when the cycle has points.' : `At current totals, 1 cycle point represents ${amount(srt, config.srt)} SRT. SpaceUSD uses clan allocation and participating ship levels. About ${Math.ceil(estimate.remainingSeconds / 60)} minutes remaining; final amounts will change.`;
      const score = BigInt(profile?.score?.score ?? '0'), total = BigInt(estimate.currentScore ?? state.game.total_score);
      const allocation = profile?.rewardAllocation?.cycle === state.game.cycle ? profile.rewardAllocation : null;
      const brief = [];
      if (profile && total) {
        for (const [label, value, address] of [
          ['SRT share', allocation?.srt ?? (BigInt(estimate.available.srt) * score / total).toString(), config.srt],
          ['SpaceUSD net', allocation?.spaceusd ?? null, config.spaceusd],
        ]) { const item = node('div'); item.append(node('small', label), node('strong', value === null ? 'Updating…' : amount(value, address))); brief.push(item); }
        brief.push(node('small', `${display(score.toString(), 4)} / ${display(total.toString(), 4)} points · estimate at current totals, not claimable yet`, 'share-context'));
      } else brief.push(node('small', profile ? 'Waiting for cycle points.' : 'Watch or connect a wallet to see its estimated allocation.', 'share-context'));
      $('cycle-your-rewards').replaceChildren(...brief);
      $('pilot-reward-estimate').textContent = profile && total ? `Your ${display(score.toString(), 4)} / ${display(total.toString(), 4)} points (${Number(score * 10000n / total) / 100}%): ${amount(allocation?.srt ?? (BigInt(estimate.available.srt) * score / total).toString(), config.srt)} SRT at the current pool balance.${allocation ? ` Estimated SpaceUSD payout: ${amount(allocation.spaceusd, config.spaceusd)} after maintenance and clan allocation.` : ' SpaceUSD allocation is updating.'} This is not yet claimable.` : 'Connect or watch a wallet to estimate its rewards.';
      $('reward-breakdown').replaceChildren(...(allocation ? [
        ['SpaceUSD member share', allocation.grossSpaceusd], ['Maintenance', allocation.maintenance], ['Leader premium', allocation.leaderPremium], ['Referral income', allocation.referral],
      ].map(([label, value]) => { const row = node('div'); row.append(node('span', label), node('strong', amount(value, config.spaceusd))); return row; }) : []));
      if (allocation?.clanId !== null && allocation) $('reward-breakdown').append(node('small', `Clan #${allocation.clanId}: ${allocation.participatingLevels} / ${allocation.clanParticipatingLevels} participating level-races. Each race uses the levels active in that race.`));
    }
    updateFleetScene($('race-fleet'), profile ?? {}, state.game.race);
    if (observedRace !== null && state.game.race > observedRace) pendingRace = state.game.race - 1;
    else if (state.game.race > 0 && observedRace !== null && ['boundary', 'checkpoint', 'accrue', 'budget', 'clan_premiums', 'settlement'].includes(state.game.phase) && observedRace === state.game.race && pendingRace !== state.game.race) {
      // Race ID may stay unchanged throughout settlement; deduplicate below.
      if (shownRace !== state.game.race) pendingRace = state.game.race;
    }
    observedRace = state.game.race;
    if (pendingRace === shownRace) pendingRace = null;
    if (pendingRace !== null) void results();
  } };
}
