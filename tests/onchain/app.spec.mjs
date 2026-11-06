import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { DirectSecp256k1HdWallet } from '@cosmjs/proto-signing';
import { stringToPath } from '@cosmjs/crypto';
import { CosmWasmClient } from '@cosmjs/cosmwasm-stargate';
import { execFileSync } from 'node:child_process';

const deployment = JSON.parse(readFileSync(new URL('../../contracts/local/deployment.json', import.meta.url), 'utf8'));
// Public LocalTerra genesis fixture only. This never enters the app bundle or an
// HTTP route, and signing is refused unless the actual node is the isolated chain.
const PUBLIC_TEST_MNEMONIC = 'notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius';

test('fusion selection locks its tier, caps five inputs, and unlocks when cleared', async ({ page }) => {
  const rarities = [...Array(6).fill('common'), 'rare', 'epic', 'legendary'];
  await page.route('**/api/wallet/*/nfts?collection=commanders*', route => route.fulfill({ json: {
    items: rarities.map((rarity, i) => ({ tokenId: `fusion-ui-${i}`, collection: deployment.recruitment,
      extension: { kind: 'commander', code: 'shield_iris', rarity, bonus: 'shield', bonus_bps: 800 } })), next: null,
  } }));
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await page.getByRole('button', { name: 'NFT inventory', exact: true }).click();
  const inputs = page.locator('#commanders input[type=checkbox]');
  await expect(inputs).toHaveCount(9);
  await expect(inputs.nth(8)).toBeDisabled();
  await inputs.nth(0).check();
  await expect(inputs.nth(6)).toBeDisabled(); await expect(inputs.nth(7)).toBeDisabled();
  await expect(page.locator('#fuse')).toContainText('Common (1/5) → 1 Rare');
  for (let i = 1; i < 5; i++) await inputs.nth(i).check();
  await expect(inputs.nth(5)).toBeDisabled();
  await inputs.nth(0).uncheck(); await expect(inputs.nth(5)).toBeEnabled();
  await page.locator('#clear-fusion').click();
  await expect(inputs.nth(6)).toBeEnabled();
  await inputs.nth(6).check(); await expect(inputs.nth(0)).toBeDisabled();
  await expect(page.locator('#fuse')).toContainText('Rare (1/5) → 1 Epic');
  await inputs.nth(6).uncheck(); await expect(inputs.nth(0)).toBeEnabled();
});

test('spectator mobile view renders without an account or horizontal overflow', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/?wallet=${deployment.owner}&sponsor=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await expect(page.locator('#network')).toContainText('test assets');
  expect(await page.locator('#boost-form input[name=count]').evaluate(input => { input.value = '1000'; return input.checkValidity(); })).toBe(true);
  await expect(page.locator('#connect')).toHaveText('Connect Keplr');
  await expect(page.locator('#claim-rewards')).toBeDisabled();
  await page.getByRole('button', { name: 'Hangar', exact: true }).click();
  await expect(page.locator('#fleet-cards .card')).toHaveCount(3);
  await expect(page.locator('#ship-form input[name=sponsor]')).toHaveValue(deployment.owner);
  await expect(page.locator('#referral')).toContainText('Referred pilots');
  await page.getByRole('button', { name: 'Treasury', exact: true }).click();
  await expect(page.locator('#treasury-summary')).toContainText('Collapsar');
  await expect(page.locator('#treasury-balances .balance-list, #treasury-balances > div')).not.toHaveCount(0);
  await page.getByRole('button', { name: 'Activity', exact: true }).click();
  await expect(page.locator('#network-activity')).not.toBeChecked();
  await expect(page.locator('#activity')).not.toContainText(/fulfill randomness/i);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test('wallet directly signs a real ship payment and NFT listing/cancellation', async ({ page, request }) => {
  test.setTimeout(150000);
  const publicConfig = await (await request.get('/api/config')).json();
  const reader = await CosmWasmClient.connect(publicConfig.rpc);
  expect(await reader.getChainId()).toBe('spaceship-racer-local');
  expect(publicConfig.chainId).toBe('spaceship-racer-local');
  const signer = await DirectSecp256k1HdWallet.fromMnemonic(PUBLIC_TEST_MNEMONIC, { prefix: 'terra', hdPaths: [stringToPath("m/44'/330'/0'/0/0")] });
  const [account] = await signer.getAccounts(); expect(account.address).toBe(deployment.owner);
  let signatures = 0;
  await page.exposeFunction('__localSignDirect', async (address, payload) => {
    signatures++;
    expect(payload.chainId).toBe('spaceship-racer-local');
    const result = await signer.signDirect(address, { chainId: payload.chainId, accountNumber: BigInt(payload.accountNumber),
      bodyBytes: Buffer.from(payload.bodyBytes, 'base64'), authInfoBytes: Buffer.from(payload.authInfoBytes, 'base64') });
    return { signed: { chainId: result.signed.chainId, accountNumber: result.signed.accountNumber.toString(),
      bodyBytes: Buffer.from(result.signed.bodyBytes).toString('base64'), authInfoBytes: Buffer.from(result.signed.authInfoBytes).toString('base64') }, signature: result.signature };
  });
  await page.addInitScript(({ address, pubkey }) => {
    const decode = text => Uint8Array.from(atob(text), c => c.charCodeAt(0));
    const encode = bytes => btoa(String.fromCharCode(...bytes));
    window.keplr = { experimentalSuggestChain: async () => {}, enable: async () => {}, getOfflineSignerAuto: async () => ({
      getAccounts: async () => [{ address, algo: 'secp256k1', pubkey: decode(pubkey) }],
      signDirect: async (address, doc) => {
        const result = await window.__localSignDirect(address, { chainId: doc.chainId, accountNumber: doc.accountNumber.toString(), bodyBytes: encode(doc.bodyBytes), authInfoBytes: encode(doc.authInfoBytes) });
        return { signed: { ...result.signed, accountNumber: BigInt(result.signed.accountNumber), bodyBytes: decode(result.signed.bodyBytes), authInfoBytes: decode(result.signed.authInfoBytes) }, signature: result.signature };
      },
    }) };
  }, { address: account.address, pubkey: Buffer.from(account.pubkey).toString('base64') });
  const apiWrites = []; const errors = [];
  page.on('request', r => { if (new URL(r.url()).pathname.startsWith('/api/') && !['GET', 'HEAD'].includes(r.method())) apiWrites.push(r.url()); });
  page.on('pageerror', e => errors.push(e.message));
  await page.goto('/'); await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await page.locator('#connect').click(); await expect(page.locator('#notice')).toContainText('Connected terra1');
  if (!await page.locator('#pilot-dialog').isVisible()) { await page.locator('#open-profile').click(); await page.locator('#edit-pilot').click(); }
  await expect(page.locator('#pilot-dialog')).toBeVisible();
  await page.locator('#pilot-form input[name=nickname]').fill('Test pilot');
  await expect(page.locator('#pilot-form input[name=sponsor]')).toBeDisabled();
  await page.locator('#save-pilot').click();
  await expect(page.locator('#open-profile')).toContainText('Test pilot');
  await expect(page.locator('#resource-header')).toContainText('SRT');
  await page.locator('#open-profile').click();
  await expect(page.locator('[data-panel=profile]')).toBeVisible();
  await page.locator('#edit-pilot').click();
  await page.locator('#pilot-form input[name=nickname]').fill('Ace pilot');
  await page.locator('#save-pilot').click();
  await expect(page.locator('#open-profile')).toContainText('Ace pilot');
  await page.getByRole('button', { name: 'Hangar', exact: true }).click();
  const before = await reader.queryContractSmart(deployment.fleet, { fleet: { wallet: account.address } });
  const target = before.next[2] + 1; expect(target).toBeLessThanOrEqual(1000);
  await page.locator('#ship-form select[name=ship]').selectOption('fighter');
  await page.locator('#ship-form input[name=tier]').fill(String(target));
  await page.locator('#ship-form select[name=token]').selectOption(publicConfig.spaceusd);
  await page.locator('#quote-ship').click(); await expect(page.locator('#ship-quote')).toContainText('Invoice: 1 SpaceUSD');
  await page.getByRole('button', { name: 'Submit ship purchase', exact: true }).click();
  await expect(page.locator('#purchase-dialog')).toBeVisible();
  await expect(page.locator('#purchase-summary')).toContainText('1 SpaceUSD');
  await page.locator('#confirm-purchase').click();
  await expect(page.locator('#notice')).toContainText('Confirmed at block', { timeout: 45000 });
  expect((await reader.queryContractSmart(deployment.fleet, { fleet: { wallet: account.address } })).next[2]).toBe(target);
  await expect(page.locator('#fleet-cards .fighter .owned-level')).toHaveText(`LVL ${target}`);
  await expect(page.locator('#fleet-cards .fighter .ship-activation')).toContainText(new RegExp(`[Ll]evel ${target}`));
  if (publicConfig.contracts.reinvestment) {
    await page.getByRole('button', { name: 'Race', exact: true }).click();
    await page.locator('[data-console-tab=rewards]').click();
    await page.locator('#open-reward-upgrades').click();
    const liveProfile = await (await request.get(`/api/wallet/${account.address}`)).json();
    if (BigInt(liveProfile.credit.spaceusd) >= 400000n) {
      await page.getByLabel('Ship to upgrade with rewards', { exact: true }).selectOption('fighter');
      await page.getByLabel('SpaceUSD rewards to convert', { exact: true }).fill('0.4');
      await page.getByRole('button', { name: 'Convert rewards', exact: true }).click();
      await expect(page.locator('#notice')).toContainText('Confirmed at block', { timeout: 45000 });
      const saved = await reader.queryContractSmart(publicConfig.contracts.reinvestment, { reinvestment: { wallet: account.address } });
      expect(BigInt(saved.progress.spaceusd[2])).toBeGreaterThanOrEqual(400000n);
      const row = page.locator('#saved-level-progress > div').filter({ hasText: 'Fighter:' });
      await row.getByRole('button', { name: 'Withdraw saved progress', exact: true }).click();
      await expect(page.locator('#notice')).toContainText('Confirmed at block', { timeout: 45000 });
      expect((await reader.queryContractSmart(publicConfig.contracts.reinvestment, { reinvestment: { wallet: account.address } })).progress.spaceusd[2]).toBe('0');
    }
    await page.getByLabel('Automatic upgrade ship', { exact: true }).selectOption('fighter');
    await page.getByLabel('% of future SpaceUSD rewards', { exact: true }).fill('50');
    await page.getByRole('button', { name: 'Save auto-upgrade', exact: true }).click();
    await expect(page.locator('#notice')).toContainText('Confirmed at block', { timeout: 45000 });
    expect((await reader.queryContractSmart(publicConfig.contracts.reinvestment, { reinvestment: { wallet: account.address } })).preference).toEqual({ ship: 'fighter', bps: 5000 });
    await page.getByLabel('Automatic upgrade ship', { exact: true }).selectOption('off');
    await page.getByRole('button', { name: 'Save auto-upgrade', exact: true }).click();
    await expect(page.locator('#notice')).toContainText('Confirmed at block', { timeout: 45000 });
    expect((await reader.queryContractSmart(publicConfig.contracts.reinvestment, { reinvestment: { wallet: account.address } })).preference).toEqual({ ship: null, bps: 0 });
    await page.locator('#close-reward-upgrades').click();
  }
  await page.getByRole('button', { name: 'NFT inventory', exact: true }).click();
  const card = page.locator('#commanders .card').first(); await expect(card).toBeVisible();
  const tokenId = await card.locator('code').textContent();
  await card.locator('input[placeholder="Sale price"]').fill('1');
  await card.getByRole('button', { name: 'List NFT', exact: true }).click();
  await expect(page.locator('#notice')).toContainText('Confirmed at block', { timeout: 45000 });
  expect((await reader.queryContractSmart(publicConfig.contracts.commanderCollection, { owner_of: { token_id: tokenId } })).owner).toBe(deployment.marketplace);
  await page.getByRole('button', { name: 'Marketplace', exact: true }).click();
  const listing = page.locator('#my-listings .job').filter({ hasText: tokenId }); await expect(listing).toBeVisible();
  await listing.getByRole('button', { name: 'Cancel listing', exact: true }).click();
  await expect(page.locator('#notice')).toContainText('Confirmed at block', { timeout: 45000 });
  expect((await reader.queryContractSmart(publicConfig.contracts.commanderCollection, { owner_of: { token_id: tokenId } })).owner).toBe(account.address);
  await page.getByRole('button', { name: 'NFT inventory', exact: true }).click();
  const signedBeforeDraw = signatures;
  const drawStarted = Date.now();
  await page.locator('#draw-one').click();
  await expect(page.locator('#draw-stage')).toHaveAttribute('data-job-id', /\d+/, { timeout: 45000 });
  const jobId = Number(await page.locator('#draw-stage').getAttribute('data-job-id'));
  expect(signatures).toBe(signedBeforeDraw + 1);
  await expect.poll(async () => {
    execFileSync(process.execPath, ['contracts/local/keeper.mjs', '--once', '--jobs'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, RACER_KEEPER_KEY: 'test1' } });
    return (await reader.queryContractSmart(deployment.recruitment, { job: { id: jobId } })).completed;
  }, { timeout: 60000, intervals: [2000, 3000] }).toBe(true);
  await expect(page.locator('#draw-stage')).toHaveAttribute('data-stage', 'complete', { timeout: 15000 });
  await expect(page.locator('#draw-results .draw-reveal-card')).toHaveCount(1);
  console.log(`Single-signature draw delivered and revealed in ${((Date.now() - drawStarted) / 1000).toFixed(1)} seconds`);
  expect(signatures).toBe(signedBeforeDraw + 1);
  await page.locator('#close-draw').click();
  await expect.poll(async () => (await (await request.get(`/api/wallet/${account.address}`)).json()).nickname, { timeout: 15000 }).toBe('Ace pilot');
  const signaturesBeforeReload = signatures;
  await page.reload(); await expect(page.locator('#notice')).toContainText('Reconnected');
  await expect(page.locator('#disconnect')).toBeVisible(); expect(signatures).toBe(signaturesBeforeReload);
  await page.locator('.brand').click(); await expect(page.locator('#notice')).toContainText('Reconnected');
  await expect(page.locator('#open-profile')).toContainText('Ace pilot'); expect(signatures).toBe(signaturesBeforeReload);
  const session = await page.evaluate(chain => JSON.parse(localStorage.getItem(`racer.wallet.${chain}`)), publicConfig.chainId);
  expect(Object.keys(session).sort()).toEqual(['address', 'provider']);
  await page.locator('#disconnect').click(); await page.reload();
  await expect(page.locator('#notice')).toContainText('Flight deck ready'); await expect(page.locator('#connect')).toHaveText('Connect Keplr');
  expect(apiWrites).toEqual([]); expect(errors).toEqual([]);
  await expect.poll(async () => (await (await request.get(`/api/activity?wallet=${account.address}`)).json()).items.some(e => e.action === 'cancel_listing'), { timeout: 30000 }).toBe(true);
  reader.disconnect();
});

test('new pilot setup locks a sponsor, permits callsign edits, and hangar fits mobile', async ({ page }) => {
  const signer = await DirectSecp256k1HdWallet.fromMnemonic(PUBLIC_TEST_MNEMONIC, { prefix: 'terra', hdPaths: [stringToPath("m/44'/330'/0'/0/1")] });
  const [account] = await signer.getAccounts();
  await page.addInitScript(({ address, pubkey }) => {
    window.keplr = { experimentalSuggestChain: async () => {}, enable: async () => {}, getOfflineSignerAuto: async () => ({
      getAccounts: async () => [{ address, algo: 'secp256k1', pubkey: Uint8Array.from(atob(pubkey), c => c.charCodeAt(0)) }],
      signDirect: async () => { throw new Error('This onboarding test must not sign transactions'); },
    }) };
  }, { address: account.address, pubkey: Buffer.from(account.pubkey).toString('base64') });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await page.locator('#connect').click();
  await expect(page.locator('#pilot-dialog')).toBeVisible();
  await expect(page.locator('#pilot-form input[name=sponsor]')).toBeEnabled();
  await page.locator('#pilot-form input[name=nickname]').fill('Nova');
  await page.locator('#pilot-form input[name=sponsor]').fill(deployment.owner);
  await page.locator('#save-pilot').click();
  await page.locator('#open-profile').click();
  await page.locator('#edit-pilot').click();
  await expect(page.locator('#pilot-form input[name=sponsor]')).toBeDisabled();
  await expect(page.locator('#pilot-form input[name=sponsor]')).toHaveValue(deployment.owner);
  await page.locator('#pilot-form input[name=nickname]').fill('Nova Prime');
  await page.locator('#save-pilot').click();
  await expect(page.locator('#open-profile')).toContainText('Nova Prime');
  await page.getByRole('button', { name: 'Hangar', exact: true }).click();
  await expect(page.locator('#fleet-cards .card')).toHaveCount(3);
  await page.getByRole('button', { name: 'Get Racer ↗', exact: true }).click();
  await expect(page.locator('#workshop-title')).toContainText('Racer → level 1');
  await expect(page.locator('#ship-quote')).toContainText('Invoice:');
  for (const width of [360, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  }
});

test('boost stats use the live parameters, selected quantity and current fleet', async ({ page, request }) => {
  const state = await (await request.get('/api/state')).json();
  const profile = await (await request.get(`/api/wallet/${deployment.owner}`)).json();
  await page.route('**/api/state', route => route.fulfill({ json: { ...state, configuration: { ...state.configuration, parameters: {
    ...state.configuration.parameters, speed_boost: '1234', acceleration: '25', temporary_distance: '56789', temporary_ticks: 10, rocket_damage_bps: 5000,
  } } } }));
  await page.route(new RegExp(`/api/wallet/${deployment.owner}$`), route => route.fulfill({ json: { ...profile, livePlayer: null, player: {
    ...profile.player, race: state.game.race, alive: true, safe_speed: '120000', acceleration_count: 0, tiers: [1, 1, 1], commander: { bonus: 'acceleration', bonus_bps: 2500 },
    ships: Array.from({ length: 3 }, () => ({ hp: '1000000', base_hp: '1000000', shield: '0', attack: '2000000' })),
  } } }));
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await page.locator('#boost-form input[name=count]').fill('3');
  const digits = index => page.locator('#boost-stat-grid dd').nth(index).evaluate(el => el.textContent.replace(/\D/g, ''));
  expect(await digits(0)).toBe('3702');
  await page.locator('[data-boost=acceleration]').click();
  await expect(page.locator('#boost-stat-grid dd').first()).toContainText(/93[.,]75/);
  expect(await digits(2)).toBe('33750');
  await page.locator('[data-boost=temporary]').click();
  expect(await digits(0)).toBe('170367');
  await expect(page.locator('#boost-stat-grid')).toContainText('60');
  await page.locator('[data-boost=rocket]').click();
  expect(await digits(0)).toBe('1000'); expect(await digits(1)).toBe('3000');
  await expect(page.locator('#boost-stat-note')).toContainText('does not spill');
});

test('game sections fit narrow phones, tablets and desktop with the same navigation', async ({ page }) => {
  test.setTimeout(90000);
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await page.locator('#hide-flight-guide').click();
  for (const width of [320, 390, 768, 1024, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    for (const tab of ['game', 'hangar', 'inventory', 'market', 'clans', 'profile', 'treasury', 'activity']) {
      await page.locator(`[data-tab=${tab}]`).click();
      await expect(page.locator(`[data-tab=${tab}]`)).toHaveAttribute('aria-current', 'page');
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${tab} at ${width}px`).toBe(true);
      if (tab === 'game') {
        await page.evaluate(() => document.fonts.ready);
        expect(await page.evaluate(() => {
          const bottom = document.querySelector('#pilot-stats').getBoundingClientRect().top;
          return [...document.querySelectorAll('#race-fleet .vital-bar')].every(bar => bar.getBoundingClientRect().bottom <= bottom + 1);
        }), `ship bars clear the HUD at ${width}px`).toBe(true);
      }
    }
  }
});

test('reward panel uses earned-point SRT share and the net clan-based SpaceUSD allocation', async ({ page, request }) => {
  const state = await (await request.get('/api/state')).json();
  const profile = await (await request.get(`/api/wallet/${deployment.owner}`)).json();
  await page.route('**/api/state', route => route.fulfill({ json: { ...state,
    game: { ...state.game, total_score: '1078000' },
    rewardEstimate: { ...state.rewardEstimate, currentScore: '1078000', projectedScore: '3992525', available: { spaceusd: '17000000', srt: '612000000' }, perPoint: { spaceusd: '157699', srt: '5677180' } },
  } }));
  await page.route(new RegExp(`/api/wallet/${deployment.owner}$`), route => route.fulfill({ json: { ...profile,
    score: { ...profile.score, score: '418000' }, rewardAllocation: { cycle: state.game.cycle, srt: '237306122', spaceusd: '4126174', grossSpaceusd: '4693419', maintenance: '567245', leaderPremium: '0', referral: '0', clanId: 1, participatingLevels: '12240', clanParticipatingLevels: '43592' },
  } }));
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await expect(page.locator('#pilot-reward-estimate')).toContainText('41.8 / 107.8 points');
  await expect(page.locator('#pilot-reward-estimate')).toContainText('237.306122 SRT');
  await expect(page.locator('#pilot-reward-estimate')).toContainText('4.126174 after maintenance and clan allocation');
  await expect(page.locator('#reward-breakdown')).toContainText('0.567245');
  await expect(page.locator('#cycle-estimate')).not.toContainText('SpaceUSD +');
});

test('race view shows equipped skins, pools and a deduplicated winner card after a race transition', async ({ page, request }) => {
  const state = await (await request.get('/api/state')).json();
  const profile = await (await request.get(`/api/wallet/${deployment.owner}`)).json();
  let race = 900001;
  const equipment = { ...profile.equipment, racer_skin: { active: { attributes: { code: 'racer2', kind: 'racer_skin' } }, pending: null } };
  await page.route('**/api/state', route => route.fulfill({ json: { ...state, game: { ...state.game, race, phase: 'waiting', replay_until: 0 } } }));
  await page.route(new RegExp(`/api/wallet/${deployment.owner}$`), route => route.fulfill({ json: { ...profile, race, player: { ...profile.player, race }, equipment } }));
  await page.route('**/api/races/900001', route => route.fulfill({ json: { result: { race: 900001, cycle: 82, winner: deployment.owner, ticks: 20, multiplier: 8 }, winner: { wallet: deployment.owner, nickname: 'Test racer', clan: { name: 'Test clan' }, fleet: profile.fleet, equipment } } }));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await expect(page.locator('#race-fleet .race-ship')).toHaveCount(3);
  await expect(page.locator('#race-fleet img').first()).toHaveAttribute('src', '/assets/ship-skins/racer2.png');
  await page.evaluate(() => { window.__raceShip = document.querySelector('#race-fleet img'); });
  await expect(page.locator('#cycle-pools')).toContainText('SpaceUSD cycle pool');
  await expect(page.locator('#cycle-pools')).toContainText('SRT cycle pool');
  await expect(page.locator('#race-end-dialog')).not.toBeVisible();
  await page.getByRole('button', { name: 'How reward estimates work', exact: true }).click();
  await expect(page.locator('#reward-help')).toBeVisible();
  await page.locator('#reward-help button').click();
  race++;
  await expect(page.locator('#race-end-dialog')).toBeVisible({ timeout: 15000 });
  await expect(page.locator('#race-end-summary')).toContainText('Test racer');
  await expect(page.locator('#race-end-summary')).toContainText('Test clan');
  await expect(page.locator('#winner-fleet .race-ship')).toHaveCount(3);
  await page.locator('#close-race-end').click();
  await page.waitForTimeout(2500);
  await expect(page.locator('#race-end-dialog')).not.toBeVisible();
  expect(await page.evaluate(() => window.__raceShip === document.querySelector('#race-fleet img'))).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('first-flight guidance distinguishes queued levels and quotes exact boost totals from current rules', async ({ page, request }) => {
  const state = await (await request.get('/api/state')).json();
  const profile = await (await request.get(`/api/wallet/${deployment.owner}`)).json();
  await page.route('**/api/state', route => route.fulfill({ json: {
    ...state, configuration: { ...state.configuration, parameters: { ...state.configuration.parameters, boost_fee_bps: 1000, boost_price: '1234567' } },
  } }));
  await page.route(new RegExp(`/api/wallet/${deployment.owner}$`), route => route.fulfill({ json: {
    ...profile, fleet: { ...profile.fleet, active: [10, 0, 18], next: [10, 0, 19], effective_race: state.game.race + 1 },
  } }));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await expect(page.locator('#flight-guide')).toContainText('Boosts are optional');
  await expect(page.locator('#guide-wallet')).toContainText('Connect Keplr or Leap');
  await expect(page.locator('#boost-rules')).toContainText('90% prize allocation · 10% service fee');
  await page.locator('#boost-form input[name=count]').fill('3');
  await expect(page.locator('#boost-cost')).toContainText('3.703701 SRT');
  await expect(page.locator('#boost-availability')).toContainText('Connect your wallet');
  await page.locator('#boost-form input[name=count]').fill('');
  await expect(page.locator('#boost-cost')).toContainText('Enter a whole quantity');
  await expect(page.locator('#boost-form button')).toBeDisabled();
  await page.getByRole('button', { name: 'Hangar', exact: true }).click();
  await expect(page.locator('.fighter .active-level')).toContainText('Level 18');
  await expect(page.locator('.fighter .queued-level')).toContainText(`Queued · race #${state.game.race + 1}`);
  await expect(page.locator('.fighter .queued-level')).toContainText('Level 19');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.locator('#hide-flight-guide').click();
  await page.reload();
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await expect(page.locator('#flight-guide')).toBeHidden();
});

test('multiplier and boost receipts stay visible while distance progresses across slow movement snapshots', async ({ page, request }) => {
  const state = await (await request.get('/api/state')).json();
  const profile = await (await request.get(`/api/wallet/${deployment.owner}`)).json();
  const started = Date.now() - 60000;
  await page.route('**/api/state', route => route.fulfill({ json: { ...state, time: new Date().toISOString(), raceStartedAt: new Date(started).toISOString(),
    raceElapsedSeconds: Math.floor((Date.now() - started) / 6000) * 6, chainHealth: { healthy: true },
    game: { ...state.game, start_height: Math.floor(started / 6000), race: 999, tick: 11, phase: 'waiting', multiplier: 25 } } }));
  await page.route(new RegExp(`/api/wallet/${deployment.owner}$`), route => route.fulfill({ json: {
    ...profile, livePlayer: null, livePending: [], height: Math.floor(Date.now() / 6000), time: new Date().toISOString(), playerTick: 10,
    player: { ...profile.player, race: 999, alive: true, safe_speed: '1000000', distance: '60000000', temporary: [{ distance: '2000000000', expires: 60 }],
      ships: Array.from({ length: 3 }, () => ({ hp: '1000', base_hp: '1000' })) },
  } }));
  await page.route(`**/api/wallet/${deployment.owner}/boosts`, route => route.fulfill({ json: { items: [
    { race: 998, tick: 9, paid: '10000000', txhash: 'REFUND', status: 'refundable', counts: {} },
    { race: 999, tick: 10, paid: '1000000', txhash: 'QUEUED', status: 'queued', counts: { speed: 1 } },
  ] } }));
  await page.route(`**/api/wallet/${deployment.owner}/refunds`, route => route.fulfill({ json: { boostCount: 11, amount: '11000000', refundCredits: '0', claims: [] } }));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await expect(page.locator('#race-multiplier')).toHaveText('×25');
  await expect(page.locator('#boost-feedback')).toContainText('Race ended earlier');
  await expect(page.locator('#boost-feedback')).toContainText('Overdrive ×1');
  await expect(page.locator('#pilot-warp')).not.toHaveText('+0');
  await expect(page.locator('#refund-count')).toHaveText('11 unused boosts');
  await expect(page.locator('#refund-amount')).toHaveText('11 SRT');
  await page.evaluate(() => { window.__distanceNode = document.getElementById('pilot-distance'); });
  const readDistance = () => page.locator('#pilot-distance').evaluate(element => {
    const parts = new Intl.NumberFormat().formatToParts(12345.6);
    const group = parts.find(p => p.type === 'group')?.value ?? ',';
    const decimal = parts.find(p => p.type === 'decimal')?.value ?? '.';
    return Number(element.textContent.replaceAll(group, '').replace(decimal, '.'));
  });
  const first = await readDistance();
  await page.waitForTimeout(7500); const second = await readDistance();
  await page.waitForTimeout(7500); const third = await readDistance();
  expect(second).toBeGreaterThan(first + 5000); expect(third).toBeGreaterThan(second + 5000);
  expect(await page.evaluate(() => window.__distanceNode === document.getElementById('pilot-distance'))).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('clan deep links expose rosters and member profiles on mobile', async ({ page, request }) => {
  const clans = await (await request.get('/api/clans')).json();
  expect(clans.items.length).toBeGreaterThan(0);
  const clan = clans.items[0];
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/clans/${clan.id}?wallet=${deployment.owner}`);
  await expect(page.locator('#clan-detail-hero')).toContainText(clan.name);
  await expect(page.locator('#clan-roster .roster-row')).not.toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.locator('#clan-roster .roster-row').first().locator('button').first().click();
  await expect(page.locator('[data-panel=profile]')).toBeVisible();
  await expect(page.locator('#profile-fleet > div')).toHaveCount(3);
  await expect(page.locator('#profile-hero code')).toContainText('terra1');
  await expect(page.locator('#profile-referrals input')).toHaveValue(/sponsor=terra1/);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.goto(`/clans?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await page.locator('#clan-search').fill('no-such-squadron');
  await expect(page.locator('#clans .clan-card')).toHaveCount(0);
  await page.locator('#clan-search').fill(clan.code);
  await expect(page.locator('#clans .clan-card')).toHaveCount(1);
});

test('activated utility power is visible before its race tick has resolved', async ({ page, request }) => {
  const state = await (await request.get('/api/state')).json();
  const profile = await (await request.get(`/api/wallet/${deployment.owner}`)).json();
  const player = { ...profile.player, race: 999, alive: true, safe_speed: '1000000', distance: '60000000', acceleration_count: 0, temporary: [],
    ships: Array.from({ length: 3 }, () => ({ hp: '1000', base_hp: '1000' })) };
  const livePlayer = { ...player, safe_speed: '11000000', acceleration_count: 1, temporary: [{ distance: '2000000000', expires: 61 }] };
  await page.route('**/api/state', route => route.fulfill({ json: { ...state, raceStartedAt: new Date(Date.now() - 60000).toISOString(), time: new Date().toISOString(), game: { ...state.game, race: 999, phase: 'waiting', tick: 11, multiplier: 7 } } }));
  await page.route(new RegExp(`/api/wallet/${deployment.owner}$`), route => route.fulfill({ json: { ...profile, player, livePlayer, playerTick: 10, livePending: [{ tick: 11, speed: 1, acceleration: 1, temporary: 1 }] } }));
  await page.route(`**/api/wallet/${deployment.owner}/boosts`, route => route.fulfill({ json: { items: [{ race: 999, tick: 11, paid: '3000000', txhash: 'ACTIVE', status: 'active', counts: { speed: 1, acceleration: 1, temporary: 1 } }] } }));
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await expect(page.locator('#boost-feedback')).toContainText('Active now');
  await expect(page.locator('#pilot-acceleration')).toHaveText('1');
  await expect(page.locator('#pilot-warp')).not.toHaveText('+0');
  await expect(page.locator('#race-multiplier')).toHaveText('×7');
  await expect(page.locator('#pilot-speed')).toHaveAttribute('title', /11[\s,]?000/);
});

test('race and cycle leaderboards include clan and solo rows with health, and fleet cards show vitals', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/game?wallet=${deployment.owner}`); await expect(page.locator('#notice')).toContainText('Flight deck ready');
  await expect(page.locator('#leaderboard-rows .score-row')).not.toHaveCount(0);
  await expect(page.locator('#leaderboard-rows .vital-bar.hp')).not.toHaveCount(0);
  await page.locator('[data-board-scope=cycle]').click(); await page.locator('[data-board-group=teams]').click();
  await expect(page.locator('#leaderboard-context')).toContainText('Cycle #');
  await expect(page.locator('#leaderboard-rows')).toContainText('SOLO PILOT');
  await page.locator('[data-board-scope=race]').click(); await expect(page.locator('#leaderboard-context')).toContainText('Race #');
  await page.getByRole('button', { name: 'Hangar', exact: true }).click();
  await expect(page.locator('#fleet-cards .fleet-vitals')).toHaveCount(3);
  for (const width of [360, 390, 768, 1440]) { await page.setViewportSize({ width, height: 900 }); expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true); }
});

test('rocket feedback animates only a confirmed target and its recorded health loss', async ({ page, request }) => {
  const config = await (await request.get('/api/config')).json();
  await page.addInitScript(({ chain, address }) => localStorage.setItem(`racer.hits.${chain}.${address}`, '0'), { chain: config.deployment, address: deployment.owner });
  const target = 'terra1hcm3sjdzelk35z322thy2qvr429v80v0ktunk7';
  const hit = { id: 1, race: 1, tick: 2, attacker: deployment.owner, target, critical: false, damage: '400000', hp_before: '1000000', hp_after: '800000', max_hp: '1000000', hp_lost: '200000', shield_before: '200000', shield_after: '0', shield_lost: '200000', destroyed: false, ships: [] };
  await page.route(/\/api\/combat(?:\?.*)?$/, route => {
    const after = new URL(route.request().url()).searchParams.get('after');
    return route.fulfill({ json: { items: after === '0' ? [hit] : [], next: 1 } });
  });
  await page.goto(`/?wallet=${deployment.owner}`);
  await expect(page.locator('#rocket-impact')).toBeVisible();
  await expect(page.locator('#rocket-target')).toContainText(target.slice(0, 12));
  await expect(page.locator('#rocket-damage')).toContainText('200 HP lost');
  await expect(page.locator('#rocket-impact .hp .vital-track')).toHaveAttribute('aria-valuenow', '80');
  await expect(page.locator('#rocket-impact .hp')).toHaveAttribute('data-current', '800000');
});
