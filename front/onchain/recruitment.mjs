import { appPath } from './paths.mjs';
const $ = id => document.getElementById(id);
const el = (tag, text, cls) => { const node = document.createElement(tag); if (text !== undefined) node.textContent = text; if (cls) node.className = cls; return node; };
const title = code => code.replaceAll('_', ' ').replace(/\b\w/g, c => c.toUpperCase());

export function createRecruitment({ config, wallet, api, transact, delivered, updateButtons }) {
  const balanceLabel = el('p', 'Connect your wallet to view recruitment funds.', 'draw-balance'); balanceLabel.id = 'draw-balance';
  document.querySelector('.recruitment-copy').insertBefore(balanceLabel, document.querySelector('.draw-options'));
  for (const id of ['draw-one', 'draw-ten']) $(id).setAttribute('aria-describedby', 'draw-balance');
  let settings, currentWallet = '', activeId = null, activeWallet = '', currentProfile = null;
  const watches = new Map(), jobs = new Map(), notified = new Set(), historyLoaded = new Set();
  const key = address => `racer.pending-draws.${config.chainId}.${address}`;
  const saved = address => { try { return JSON.parse(localStorage.getItem(key(address)) ?? '[]'); } catch { return []; } };
  const remember = (address, id, remove = false) => localStorage.setItem(key(address), JSON.stringify(remove ? saved(address).filter(value => value !== id) : [...new Set([...saved(address), id])]));
  function stage(value, message) {
    $('draw-stage').dataset.stage = value; $('draw-status').textContent = message;
  }
  function reveal(data) {
    const fusedRarity = data.job.kind.fusion && data.items[0]?.extension?.rarity;
    $('draw-title').textContent = fusedRarity ? `Fusion complete: ${title(fusedRarity)} commander` : data.items.length === 1 ? 'Your new commander' : `${data.items.length} commanders recruited`;
    stage('complete', fusedRarity ? `Five commanders fused into one ${fusedRarity.toUpperCase()} commander. Delivered to your wallet.` : 'Delivered to your wallet. Ready to equip.');
    $('draw-results').replaceChildren(...data.items.map((item, i) => {
      const a = item.extension, card = el('article', undefined, `draw-reveal-card rarity-${a?.rarity ?? 'common'}`);
      card.style.setProperty('--reveal-delay', `${i * 180}ms`);
      if (!a) { card.append(el('h3', item.tokenId), el('p', 'This commander was consumed or burned after recruitment.')); return card; }
      const image = el('img'); image.src = appPath(`/assets/commander-skins/${a.code}.png`); image.alt = title(a.code);
      card.append(el('span', a.rarity.toUpperCase(), 'rarity'), image, el('h3', title(a.code)), el('p', `+${a.bonus_bps / 100}% ${title(a.bonus)}`));
      return card;
    }));
  }
  function renderJobs() {
    const list = [...jobs.values()].filter(job => job.wallet === currentWallet).sort((a, b) => b.id - a.id).slice(0, 12);
    $('recruit-jobs').replaceChildren(...list.map(job => {
      const row = el('div', undefined, 'recruit-receipt');
      row.append(el('span', job.kind.recruit ? `Recruitment ×${job.kind.recruit.count}` : 'Commander fusion'), el('small', job.completed ? `${job.minted.length} delivered` : 'Completing automatically…'));
      const view = el('button', job.completed ? 'View commander' : 'View progress', 'secondary'); view.type = 'button';
      view.addEventListener('click', () => { activeId = job.id; activeWallet = job.wallet; $('draw-results').replaceChildren(); $('draw-dialog').showModal(); stage('randomness', 'Loading draw status…'); void check(job.wallet, job.id); });
      row.append(view); return row;
    }));
  }
  async function check(address, id) {
    try {
      const data = await api(`wallet/${address}/recruitment/${id}`);
      jobs.set(`${address}:${id}`, data.job); renderJobs();
      if (activeId === id && activeWallet === address) {
        if (data.job.completed) reveal(data);
        else stage(data.stage, data.stage === 'minting' ? 'Draw verified. Creating your commander…' : 'Waiting for the draw result…');
      }
      if (data.job.completed) {
        remember(address, id, true); watches.delete(`${address}:${id}`);
        if (currentWallet === address && !notified.has(`${address}:${id}`)) { notified.add(`${address}:${id}`); await delivered(); }
        return;
      }
    } catch {
      if (activeId === id && activeWallet === address) stage('randomness', 'Reconnecting to your draw. No additional signature is needed.');
    }
    watches.set(`${address}:${id}`, setTimeout(() => void check(address, id), 2500));
  }
  function watch(address, id) {
    const jobKey = `${address}:${id}`;
    if (watches.has(jobKey)) return;
    watches.set(jobKey, null); remember(address, id); void check(address, id);
  }
  async function loadHistory(address) {
    if (!address || historyLoaded.has(address)) return;
    historyLoaded.add(address);
    let after = null;
    try {
      for (let page = 0; page < 20; page++) {
        const result = await api(`wallet/${address}/recruitment?limit=50${after ? `&start_after=${after}` : ''}`);
        for (const job of result.items) {
          jobs.set(`${address}:${job.id}`, job);
          if (!job.completed && wallet.address === address) watch(address, job.id);
        }
        renderJobs();
        if (!result.next) break;
        after = result.next;
      }
    } catch { historyLoaded.delete(address); }
  }
  async function draw(count) {
    if (!wallet.address || currentWallet !== wallet.address) return;
    const address = wallet.address;
    activeId = null; activeWallet = address; $('draw-title').textContent = count === 1 ? 'Recruit commander' : `Recruit ${count} commanders`;
    delete $('draw-stage').dataset.jobId;
    $('draw-results').replaceChildren(); stage('signing', 'Approve the draw in your wallet. This is the only signature.');
    if (!$('draw-dialog').open) $('draw-dialog').showModal();
    const result = await transact(`Recruit ×${count}`, () => wallet.recruit(count));
    if (!result) { stage('error', 'The draw was not submitted. You can close this window and try again.'); return; }
    const event = result.events?.find(event => event.attributes.some(a => a.key === 'action' && a.value === 'request_commanders'));
    const id = Number(event?.attributes.find(a => a.key === 'job_id')?.value);
    if (!Number.isSafeInteger(id) || id < 1) { stage('error', 'Transaction confirmed. Your draw will appear in recruitment history shortly.'); return; }
    activeId = id; $('draw-stage').dataset.jobId = String(id); stage('randomness', 'Payment confirmed. Waiting for the draw result…'); watch(address, id);
  }
  $('draw-one').addEventListener('click', () => void draw(1)); $('draw-ten').addEventListener('click', () => void draw(10));
  $('close-draw').addEventListener('click', () => $('draw-dialog').close());
  return {
    follow(result, label = 'Commander fusion') {
      const event = result.events?.find(event => event.attributes.some(a => a.key === 'action' && a.value === 'request_commanders'));
      const id = Number(event?.attributes.find(a => a.key === 'job_id')?.value);
      if (!id) return;
      activeId = id; activeWallet = wallet.address; $('draw-title').textContent = label; $('draw-results').replaceChildren();
      $('draw-stage').dataset.jobId = String(id); stage('randomness', 'Fusion confirmed. Your new commander will arrive automatically.');
      if (!$('draw-dialog').open) $('draw-dialog').showModal();
      watch(wallet.address, id);
    },
    async load() {
      settings = await api('recruitment');
      const asset = config.tokens.find(t => t.address === config.srt);
      const price = Number(settings.pullPrice) / 10 ** asset.decimals;
      $('draw-one-price').textContent = `${price.toLocaleString()} SRT`; $('draw-ten-price').textContent = `${(price * 10).toLocaleString()} SRT`;
      const total = settings.catalogue.reduce((sum, entry) => sum + entry.weight, 0);
      $('commander-catalogue').replaceChildren(...settings.catalogue.map(entry => {
        const a = entry.attributes, card = el('article', undefined, `catalogue-card rarity-${a.rarity}`), img = el('img'); img.src = appPath(`/assets/commander-skins/${a.code}.png`); img.alt = title(a.code);
        card.append(img, el('h3', title(a.code)), el('small', `${a.rarity} · ${(entry.weight / total * 100).toFixed(2)}%`), el('p', `+${a.bonus_bps / 100}% ${title(a.bonus)}`)); return card;
      }));
    },
    update(profile, watched) {
      currentWallet = watched; currentProfile = profile;
      void loadHistory(watched);
      for (const job of currentProfile?.jobs ?? []) {
        const existing = jobs.get(`${job.wallet}:${job.id}`);
        if (!existing?.completed || job.completed) jobs.set(`${job.wallet}:${job.id}`, job);
        if (!job.completed && !existing?.completed && job.wallet === wallet.address) watch(job.wallet, job.id);
      }
      if (watched && watched === wallet.address) for (const id of saved(watched)) watch(watched, id);
      renderJobs();
      const balance = profile?.balances.find(t => t.address === config.srt)?.balance ?? '0';
      const decimals = config.tokens.find(t => t.address === config.srt)?.decimals ?? 6;
      const format = value => (Number(value) / 10 ** decimals).toLocaleString(undefined, { maximumFractionDigits: 6 });
      const deficit = settings && BigInt(balance) < BigInt(settings.pullPrice) ? BigInt(settings.pullPrice) - BigInt(balance) : 0n;
      $('draw-balance').textContent = !profile ? 'Connect your wallet to see your SRT and draw a commander.' : `${watched === wallet.address ? 'Your' : 'Watched wallet'} SRT: ${format(balance)}${deficit ? ` · Need ${format(deficit)} more SRT for one draw.` : ' · One signature, automatic delivery.'}`;
      $('draw-balance').classList.toggle('insufficient', deficit > 0n);
      for (const [id, count] of [['draw-one', 1], ['draw-ten', 10]]) {
        const insufficient = !settings || BigInt(balance) < BigInt(settings.pullPrice) * BigInt(count);
        if (insufficient) $(id).dataset.blocked = ''; else delete $(id).dataset.blocked;
        $(id).title = insufficient ? 'Not enough SRT in this wallet' : 'One signature. Automatic delivery.';
      }
      updateButtons();
    },
  };
}
