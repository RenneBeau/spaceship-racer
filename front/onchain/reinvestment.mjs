import { node } from './battle-ui.mjs';
import * as tx from './transactions.mjs';
const ships = ['racer', 'tank', 'fighter'];
function shipSelect(off = false) {
  const select = node('select');
  for (const value of [...(off ? ['off'] : []), ...ships]) { const option = node('option', value === 'off' ? 'Off' : value[0].toUpperCase() + value.slice(1)); option.value = value; select.append(option); }
  return select;
}
function label(text, input) { const el = node('label', text); el.append(input); return el; }
export function createReinvestment({ config, wallet, transact, updateButtons }) {
  if (!config.contracts.reinvestment) return { update() {} };
  const section = node('section', undefined, 'reward-conversion');
  const manual = node('div', undefined, 'conversion-manual'), automatic = node('div', undefined, 'conversion-auto'), savings = node('div', undefined, 'conversion-savings');
  manual.append(node('h3', 'Convert now')); automatic.append(node('h3', 'Automatic upgrades')); savings.append(node('h3', 'Saved level progress'));
  const form = node('form'), ship = shipSelect(), amount = node('input');
  amount.type = 'number'; amount.min = '0.000001'; amount.step = '0.000001'; amount.required = true;
  amount.setAttribute('aria-label', 'SpaceUSD rewards to convert'); ship.setAttribute('aria-label', 'Ship to upgrade with rewards');
  const submit = node('button', 'Convert rewards'); submit.dataset.walletAction = '';
  form.append(label('Ship', ship), label('SpaceUSD', amount), submit);
  manual.append(form, node('p', 'Claims your rewards and buys levels in one transaction. Unspent SpaceUSD stays as progress toward the next level. SRT rewards go to your wallet.', 'muted'));
  const autoForm = node('form'), autoShip = shipSelect(true), percentage = node('input'), save = node('button', 'Save auto-upgrade', 'secondary');
  autoShip.setAttribute('aria-label', 'Automatic upgrade ship'); percentage.type = 'number'; percentage.min = '0.01'; percentage.max = '100'; percentage.step = '0.01'; percentage.value = '100'; percentage.required = true;
  save.dataset.walletAction = ''; save.type = 'submit';
  autoForm.append(label('Automatic upgrades', autoShip), label('% of future SpaceUSD rewards', percentage), save);
  const status = node('p', undefined, 'muted'), progress = node('div'); progress.id = 'saved-level-progress';
  automatic.append(autoForm, node('p', 'Invest a share of future SpaceUSD rewards at cycle settlement. The keeper retries deferred conversions. Only allocated rewards are used. You can turn it off at any time.', 'muted'), status);
  savings.append(progress); section.append(manual, automatic, savings);
  document.getElementById('reward-upgrade-content').append(section);
  const open = node('button', 'Reward upgrades', 'secondary'); open.type = 'button'; open.id = 'open-reward-upgrades';
  open.addEventListener('click', () => document.getElementById('reward-upgrade-dialog').showModal());
  document.getElementById('close-reward-upgrades').addEventListener('click', () => document.getElementById('reward-upgrade-dialog').close());
  document.querySelector('.reward-panel').append(open);
  let watched = '', dirty = false, amountDirty = false;
  amount.addEventListener('input', () => { amountDirty = true; });
  autoForm.addEventListener('input', () => { dirty = true; });
  autoShip.addEventListener('change', () => { percentage.disabled = autoShip.value === 'off'; dirty = true; });
  form.addEventListener('submit', event => { event.preventDefault(); void transact('Convert rewards', () => wallet.convertRewards(ship.value, amount.value)).then(() => { amountDirty = false; }); });
  autoForm.addEventListener('submit', event => {
    event.preventDefault();
    void transact('Save auto-upgrade', () => {
      const bps = autoShip.value === 'off' ? 0 : tx.number(tx.units(percentage.value, 2), 10000);
      return wallet.execute([tx.instruction(config.contracts.reinvestment, { set_auto_reinvest: { ship: autoShip.value === 'off' ? null : autoShip.value, bps } })], 'set automatic reward upgrades');
    }).then(() => { dirty = false; });
  });
  return { update(profile) {
    open.disabled = !profile?.reinvestment;
    if (!profile?.reinvestment) { section.hidden = true; return; }
    section.hidden = false;
    if (watched !== profile.wallet) { watched = profile.wallet; dirty = false; amountDirty = false; }
    const state = profile.reinvestment, preference = state.preference;
    for (const select of [ship, autoShip]) for (const option of select.options) {
      if (option.value !== 'off') option.disabled = profile.fleet.next[ships.indexOf(option.value)] >= 1000;
    }
    if (ship.selectedOptions[0]?.disabled) ship.value = [...ship.options].find(option => !option.disabled)?.value ?? '';
    if (!amountDirty && document.activeElement !== amount) amount.value = tx.display(profile.credit.spaceusd, 6);
    if (BigInt(profile.credit.spaceusd) === 0n) submit.dataset.blocked = ''; else delete submit.dataset.blocked;
    if (!profile.fleet.purchased) { submit.dataset.blocked = ''; save.dataset.blocked = ''; } else delete save.dataset.blocked;
    if (!ship.value) submit.dataset.blocked = '';
    if (!dirty) { autoShip.value = preference.ship ?? 'off'; percentage.value = preference.bps ? tx.display(String(preference.bps), 2) : '100'; percentage.disabled = !preference.ship; }
    status.textContent = preference.ship ? `Auto-upgrade: ${preference.bps / 100}% → ${preference.ship} at cycle settlement.${BigInt(state.ready) > 0n ? ` Waiting for keeper retry: ${tx.display(state.ready, 6)} SpaceUSD.` : ''}` : 'Automatic upgrades are off.';
    progress.replaceChildren(...ships.map((name, i) => {
      const saved = state.progress.spaceusd[i], row = node('div');
      const percent = BigInt(saved) * 10000n / BigInt(state.pricePerLevel);
      row.append(node('small', `${name[0].toUpperCase() + name.slice(1)}: ${Number(percent) / 100}% toward the next level · ${tx.display(saved, 6)} SpaceUSD saved.`));
      if (BigInt(saved) > 0n && profile.fleet.next[i] < 1000) { const bar = node('progress'); bar.max = 100; bar.value = Number(percent) / 100; bar.setAttribute('aria-label', `${name} next-level progress`); row.append(bar); }
      if (BigInt(saved) > 0n) {
        const withdraw = node('button', 'Withdraw saved progress', 'secondary'); withdraw.type = 'button'; withdraw.dataset.walletAction = '';
        withdraw.addEventListener('click', () => void transact('Withdraw level progress', () => wallet.execute([tx.instruction(config.contracts.reinvestment, { withdraw_progress: { ship: name } })], 'withdraw saved level progress'))); row.append(withdraw);
      }
      return row;
    }));
    updateButtons();
  } };
}
