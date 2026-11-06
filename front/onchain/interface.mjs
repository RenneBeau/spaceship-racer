import { appPath } from './paths.mjs';
const paths = {
  game: ['M4 17l4-10 4 5 4-5 4 10', 'M8 17h8'],
  hangar: ['M3 10l9-6 9 6v10H3z', 'M8 20v-7h8v7'],
  inventory: ['M4 7l8-4 8 4v11l-8 4-8-4z', 'M4 7l8 4 8-4', 'M12 11v11'],
  market: ['M3 5h18l-2 7H5z', 'M6 12v8h12v-8', 'M9 20v-5h6v5'],
  clans: ['M12 3l8 4v6c0 4-8 8-8 8s-8-4-8-8V7z', 'M8 11h8', 'M12 7v8'],
  profile: ['M16 7a4 4 0 11-8 0 4 4 0 018 0', 'M4 21v-2a8 8 0 0116 0v2'],
  treasury: ['M3 9l9-6 9 6z', 'M5 12v6', 'M12 12v6', 'M19 12v6', 'M3 21h18'],
  activity: ['M3 12h4l3-7 4 14 3-7h4'],
  speed: ['M4 5l7 7-7 7', 'M12 5l7 7-7 7'],
  acceleration: ['M5 17l7-12 7 12', 'M5 22l7-12 7 12'],
  temporary: ['M2 8c4-8 8 8 12 0s8 8 8 0', 'M2 16c4-8 8 8 12 0s8 8 8 0'],
  rocket: ['M14 4l6-1-1 6-9 9-4-4z', 'M6 14l-3 1v6l6-3', 'M14 9h.01'],
  heal: ['M9 3h6v6h6v6h-6v6H9v-6H3V9h6z'],
  disconnect: ['M10 4H4v16h6', 'M10 12h11', 'M17 8l4 4-4 4'],
};
export function interfaceIcon(name) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24'); svg.setAttribute('fill', 'none'); svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '1.7'); svg.setAttribute('stroke-linecap', 'round'); svg.setAttribute('stroke-linejoin', 'round'); svg.setAttribute('aria-hidden', 'true');
  for (const path of paths[name] ?? []) { const el = document.createElementNS(svg.namespaceURI, 'path'); el.setAttribute('d', path); svg.append(el); }
  return svg;
}
export function setupInterface() {
  const brand = document.querySelector('.brand');
  for (const child of brand.childNodes) if (child.nodeType === Node.TEXT_NODE) child.textContent = '';
  brand.prepend(interfaceIcon('speed'));
  const tools = document.createElement('div'); tools.className = 'view-tools';
  const guide = document.querySelector('.guide-toolbar'), scanner = document.querySelector('.spectator-tools');
  guide.before(tools); tools.append(guide, scanner);
  const game = document.querySelector('[data-panel="game"]'), original = game.querySelector(':scope > .columns');
  const workspace = document.createElement('div'), main = document.createElement('div'), sidebar = document.createElement('aside');
  workspace.className = 'race-workspace'; main.className = 'race-main'; sidebar.className = 'race-sidebar'; sidebar.setAttribute('aria-label', 'Race rewards and wallet');
  main.classList.add('scene-shell');
  const arena = game.querySelector('.arena'), armory = game.querySelector('.armory');
  const raceStats = document.getElementById('race-stats'), pools = game.querySelector('.cycle-pools'), multiplier = game.querySelector('.race-stakes');
  const hud = document.createElement('div'); hud.className = 'scene-hud'; hud.append(raceStats, multiplier, pools); arena.prepend(hud);
  const state = document.createElement('div'); state.id = 'scene-state'; state.className = 'scene-state'; state.textContent = 'Reading race state'; arena.append(state);
  const pilot = document.createElement('div'); pilot.className = 'scene-pilot';
  const avatar = document.createElement('div'); avatar.className = 'scene-commander'; avatar.append(interfaceIcon('profile'));
  const portrait = document.createElement('img'); portrait.id = 'scene-commander'; portrait.alt = 'Equipped commander'; portrait.hidden = true; avatar.append(portrait);
  const identity = document.createElement('div'), pilotLabel = document.createElement('small'), pilotName = document.createElement('strong');
  pilotLabel.textContent = 'Pilot'; pilotName.id = 'scene-pilot-name'; pilotName.textContent = 'No pilot selected'; identity.append(pilotLabel, pilotName); pilot.append(avatar, identity); arena.append(pilot);
  const feedback = document.createElement('div'); feedback.id = 'scene-action-feedback'; feedback.setAttribute('aria-hidden', 'true'); arena.append(feedback);
  main.append(arena, armory);
  const generalHelp = armory.querySelector('.section-head button'); generalHelp.classList.add('dock-help');
  armory.querySelector('.boost-preview-heading').append(generalHelp); armory.querySelector('.section-head').hidden = true;
  const stack = original.querySelector('.stack'), balances = stack.children[1], shortcuts = stack.querySelector('.mission-panel');
  balances.classList.add('wallet-panel'); shortcuts.hidden = true;
  const estimate = document.createElement('article'); estimate.className = 'panel personal-estimate';
  const estimateTitle = document.createElement('h2'); estimateTitle.textContent = 'Cycle estimate';
  estimate.append(estimateTitle, pools.querySelector('#cycle-your-rewards'), pools.querySelector('.text-button'));
  sidebar.append(estimate, game.querySelector('.reward-panel'), game.querySelector('.refund-panel'), balances, shortcuts);
  const odds = multiplier.querySelector('button'); odds.textContent = 'i'; odds.classList.add('scene-odds'); odds.setAttribute('aria-label', 'Multiplier odds'); multiplier.append(odds);
  workspace.append(main); game.prepend(workspace); original.remove();
  const console = document.createElement('section'); console.className = 'race-console';
  const consoleNav = document.createElement('nav'); consoleNav.className = 'console-tabs'; consoleNav.setAttribute('aria-label', 'Race panels');
  const standings = game.querySelector('.standings-panel'), combat = document.getElementById('combat-history').closest('article');
  const recent = document.createElement('div'); recent.className = 'recent-actions';
  const recentTitle = document.createElement('h3'); recentTitle.textContent = 'Your recent actions'; recent.append(recentTitle, document.getElementById('boost-feedback')); combat.append(recent);
  const panels = { standings, combat, rewards: sidebar };
  for (const [name, panel] of Object.entries(panels)) {
    panel.dataset.consolePanel = name; panel.hidden = name !== 'standings';
    const button = document.createElement('button'); button.type = 'button'; button.dataset.consoleTab = name;
    button.textContent = name[0].toUpperCase() + name.slice(1); button.setAttribute('aria-label', `View ${name}`); button.setAttribute('aria-expanded', String(!panel.hidden));
    button.classList.toggle('selected', !panel.hidden);
    button.addEventListener('click', () => {
      for (const [key, value] of Object.entries(panels)) value.hidden = key !== name;
      for (const control of consoleNav.children) { const selected = control.dataset.consoleTab === name; control.classList.toggle('selected', selected); control.setAttribute('aria-expanded', String(selected)); }
    });
    consoleNav.append(button);
  }
  console.append(consoleNav, standings, combat, sidebar); workspace.after(console);
  for (const button of document.querySelectorAll('.tabs [data-tab]')) button.prepend(interfaceIcon(button.dataset.tab));
  for (const button of document.querySelectorAll('.mobile-game-nav [data-jump]')) {
    const label = { game: 'Race', hangar: 'Hangar', inventory: 'Inventory', market: 'Market', clans: 'Clans' }[button.dataset.jump];
    button.replaceChildren(interfaceIcon(button.dataset.jump), document.createTextNode(label));
    button.setAttribute('aria-label', `Open ${label.toLowerCase()}`);
  }
  const disconnect = document.getElementById('disconnect'); disconnect.prepend(interfaceIcon('disconnect')); disconnect.setAttribute('aria-label', 'Disconnect wallet');
  const art = { speed: 'speed', acceleration: 'acceleration', temporary: 'warp', rocket: 'rocket', heal: 'repair' };
  for (const button of document.querySelectorAll('[data-boost]')) {
    const image = document.createElement('img'); image.src = appPath(`/assets/ui/ability-${art[button.dataset.boost]}.webp`); image.alt = ''; image.decoding = 'async';
    const icon = button.querySelector('.boost-icon'); icon.replaceChildren(image);
    if (button.dataset.boost === 'heal') icon.append(interfaceIcon('heal'));
  }
}

let feedbackTimer;
export function showSceneAction(kind, label) {
  const arena = document.querySelector('.arena'), message = document.getElementById('scene-action-feedback');
  clearTimeout(feedbackTimer); arena.classList.remove('action-feedback'); message.textContent = label; message.dataset.kind = kind;
  void arena.offsetWidth; arena.classList.add('action-feedback');
  feedbackTimer = setTimeout(() => arena.classList.remove('action-feedback'), 3200);
}
