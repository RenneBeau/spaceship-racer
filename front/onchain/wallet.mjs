import { CosmWasmClient, SigningCosmWasmClient } from '@cosmjs/cosmwasm-stargate';
import { GasPrice } from '@cosmjs/stargate';
import { toBase64 } from '@cosmjs/encoding';
import * as tx from './transactions.mjs';
import { appPath } from './paths.mjs';
import { boostKey, refundKey, groupRefundTicks } from '../../src/onchain/refunds.mjs';

export class Wallet {
  constructor(config) { this.config = config; this.address = ''; }
  async reader() {
    if (!this.readClient) {
      this.readClient = await CosmWasmClient.connect(this.config.rpc);
      if (await this.readClient.getChainId() !== this.config.chainId) throw new Error('RPC chain ID does not match this game');
    }
    return this.readClient;
  }
  async query(contract, msg) { return (await this.reader()).queryContractSmart(contract, msg); }
  session() {
    try { const value = JSON.parse(localStorage.getItem(`racer.wallet.${this.config.chainId}`) ?? 'null'); return value?.provider === 'keplr' ? value : null; } catch { return null; }
  }
  async connect(name = 'keplr', restoring = false) {
    if (name !== 'keplr') throw new Error('Connect using Keplr.');
    const provider = window[name];
    if (!provider) throw new Error('Open this page with Keplr available');
    if (this.config.alpha && this.config.chainId === 'spaceship-racer-alpha' && this.config.portalWalletAccess) {
      const response = await fetch(appPath('/wallet-access'), { method: 'POST' });
      if (!response.ok) throw new Error('Sign in to the hosting portal with an authorized alpha wallet again, then reconnect.');
      const access = await response.json();
      this.config.rpc = access.rpc; this.config.lcd = access.lcd;
      this.readClient?.disconnect(); this.readClient = null;
    }
    if ((!restoring || this.config.alpha) && this.config.chainId !== 'columbus-5' && provider.experimentalSuggestChain) {
      await provider.experimentalSuggestChain({ chainId: this.config.chainId, chainName: this.config.chainName, rpc: this.config.rpc, rest: this.config.lcd,
        bip44: { coinType: 330 }, bech32Config: { bech32PrefixAccAddr: 'terra', bech32PrefixAccPub: 'terrapub', bech32PrefixValAddr: 'terravaloper',
          bech32PrefixValPub: 'terravaloperpub', bech32PrefixConsAddr: 'terravalcons', bech32PrefixConsPub: 'terravalconspub' },
        currencies: [{ coinDenom: 'LUNC', coinMinimalDenom: 'uluna', coinDecimals: 6 }],
        feeCurrencies: [{ coinDenom: 'LUNC', coinMinimalDenom: 'uluna', coinDecimals: 6, gasPriceStep: { low: 28.325, average: 35, high: 42 } }],
        stakeCurrency: { coinDenom: 'LUNC', coinMinimalDenom: 'uluna', coinDecimals: 6 }, features: ['cosmwasm'] });
    }
    await provider.enable(this.config.chainId);
    this.signer = provider.getOfflineSignerAuto ? await provider.getOfflineSignerAuto(this.config.chainId) : provider.getOfflineSigner(this.config.chainId);
    const [account] = await this.signer.getAccounts(); if (!account) throw new Error('Wallet has no account');
    this.address = tx.terra(account.address);
    if (this.config.alpha && this.config.chainId === 'spaceship-racer-alpha') {
      const response = await fetch(`${this.config.faucet}/first-login`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ address: this.address }) });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error ?? 'Alpha starter funding unavailable; reconnect to retry');
    }
    this.signing = await SigningCosmWasmClient.connectWithSigner(this.config.rpc, this.signer, {
      gasPrice: GasPrice.fromString(`${this.config.gasPrice}uluna`), broadcastPollIntervalMs: 500, broadcastTimeoutMs: 60000 });
    if (await this.signing.getChainId() !== this.config.chainId) throw new Error('Signing RPC chain mismatch');
    localStorage.setItem(`racer.wallet.${this.config.chainId}`, JSON.stringify({ provider: name, address: this.address }));
    return this.address;
  }
  disconnect(forget = true) {
    this.signing?.disconnect(); this.address = ''; this.signing = null; this.signer = null;
    if (forget) localStorage.removeItem(`racer.wallet.${this.config.chainId}`);
  }
  async execute(instructions, label, admin = false) {
    if (!this.signing || !this.address) throw new Error('Connect a wallet first');
    const [current] = await this.signer.getAccounts();
    if (current.address !== this.address) { this.disconnect(); throw new Error('Wallet account changed; reconnect'); }
    const pause = await this.query(this.config.contracts.pause, { state: {} });
    if (pause.paused && !admin) throw new Error('Game is paused');
    let nickname = '';
    try { nickname = JSON.parse(localStorage.getItem(`racer.pilot.${this.config.chainId}.${this.address}`) ?? 'null')?.nickname ?? ''; } catch {}
    const memo = `Spaceship Racer: ${label}${nickname ? ` | pilot:${toBase64(new TextEncoder().encode(nickname))}` : ''}`;
    const result = await this.signing.executeMultiple(this.address, instructions, 1.3, memo);
    if (Number(result.code ?? 0) !== 0) throw new Error(result.rawLog ?? 'Transaction failed');
    return result;
  }
  async buyShip(mode, ship, tier, token, sponsor, bps) {
    const wanted = tx.purchase(mode, ship, tier, sponsor);
    const quote = await this.query(this.config.contracts.payments, { quote: { wallet: this.address, token, purchase: wanted } });
    const block = await (await this.reader()).getBlock();
    return this.execute([tx.shipPayment(this.config, token, wanted, quote, bps, Math.floor(Date.parse(block.header.time) / 1000) + 300)], `${mode} ${ship}`);
  }
  async quoteShip(mode, ship, tier, token, sponsor) {
    return this.query(this.config.contracts.payments, { quote: { wallet: this.address, token, purchase: tx.purchase(mode, ship, tier, sponsor) } });
  }
  async rewardClaims() {
    const sources = [...new Set([this.config.contracts.rewards, this.config.contracts.backingRewards].filter(Boolean))];
    const instructions = []; let spaceusd = 0n;
    for (const contract of sources) {
      const credit = await this.query(contract, { credit: { wallet: this.address } });
      spaceusd += BigInt(credit.spaceusd);
      if (BigInt(credit.spaceusd) > 0n || BigInt(credit.srt) > 0n) instructions.push(tx.instruction(contract, { claim: {} }));
    }
    return { instructions, spaceusd };
  }
  async claimRewards() {
    const { instructions } = await this.rewardClaims();
    if (!instructions.length) throw new Error('No rewards to claim');
    return this.execute(instructions, 'claim rewards');
  }
  async convertRewards(ship, value) {
    if (!this.config.contracts.reinvestment || !['racer', 'tank', 'fighter'].includes(ship)) throw new Error('Reward conversion unavailable');
    const { instructions, spaceusd } = await this.rewardClaims();
    const decimals = this.config.tokens.find(token => token.address === this.config.spaceusd)?.decimals ?? 6;
    const amount = BigInt(tx.units(value, decimals));
    if (amount <= 0n || amount > spaceusd) throw new Error('Choose an amount within your unclaimed SpaceUSD rewards');
    instructions.push(tx.send(this.config.spaceusd, this.config.contracts.reinvestment, amount.toString(), { invest: { ship } }));
    return this.execute(instructions, 'convert rewards to ship levels');
  }
  async boost(kind, count, maximumSrt = null) {
    const c = this.config.contracts;
    const [state, config] = await Promise.all([this.query(c.game, { state: {} }), this.query(c.game, { config: {} })]);
    let price = config.parameters.boost_price;
    if (kind === 'heal') {
      if (!config.healing?.enabled) throw new Error('Repair boost is not enabled');
      const decimals = this.config.tokens.find(token => token.address === this.config.srt)?.decimals ?? 6;
      price = tx.units(maximumSrt, decimals); count = 1;
    }
    if (kind === 'rocket') {
      const player = await this.query(c.game, { player: { wallet: this.address } });
      if (player.ships[2].hp === '0') throw new Error('An active fighter is required');
      price = await rocketPrice((address, msg) => this.query(address, msg), c.game, this.address);
    }
    return this.execute([tx.boost(this.config, kind, count, state.race, price)], `${kind} boost`);
  }
  async recruit(count) {
    count = tx.number(count, 10); if (![1, 10].includes(count)) throw new Error('Choose one or ten pulls');
    const cfg = await this.query(this.config.contracts.recruitment, { config: {} });
    return this.execute([tx.send(cfg.srt, this.config.contracts.recruitment, (BigInt(cfg.pull_price) * BigInt(count)).toString(), { count })], 'recruit commanders');
  }
  async recoverBoosts() {
    if (!this.address) throw new Error('Connect your wallet first');
    const response = await fetch(appPath(`/api/wallet/${encodeURIComponent(this.address)}/refunds`));
    const discovered = await response.json();
    if (!response.ok) throw new Error(discovered.error ?? 'Could not discover refunds');
    const reader = await this.reader();
    const allowed = new Set([this.config.contracts.game, ...(this.config.previousGames ?? [])]);
    const raw = async (contract, key) => { const bytes = await reader.queryContractRaw(contract, key); return bytes?.length ? JSON.parse(new TextDecoder().decode(bytes)) : null; };
    // The indexer discovers keys; verify eligibility and amounts onchain before
    // presenting a single atomic multi-message transaction to the wallet.
    const entries = new Map(); const credits = new Map(); let total = 0n;
    for (const contract of allowed) { const amount = BigInt(await raw(contract, refundKey(this.address)) ?? '0'); credits.set(contract, amount); total += amount; entries.set(contract, []); }
    for (const group of discovered.claims) {
      const contract = group.contract ?? this.config.contracts.game;
      if (!allowed.has(contract)) throw new Error('Refund contract outside this deployment');
      if (!group.ticks.length) continue;
      const result = await this.query(contract, { result: { race: group.race } });
      for (const tick of group.ticks) {
        if (tick <= result.ticks) continue;
        const value = await raw(contract, boostKey(group.race, tick, this.address));
        if (value && BigInt(value.paid) > 0n) { entries.get(contract).push({ race: group.race, tick }); total += BigInt(value.paid); }
      }
    }
    if (total === 0n) throw new Error('No unused boosts to recover. Refunds become available when the race ends.');
    const instructions = [];
    for (const [contract, values] of entries) {
      const groups = groupRefundTicks(values);
      if (!groups.length && credits.get(contract) > 0n) groups.push({ race: 1, ticks: [] });
      instructions.push(...groups.map(group => tx.instruction(contract, { claim_refunds: group })));
    }
    return this.execute(instructions, 'recover all unused boosts');
  }
  async ownNft(collection, tokenId) {
    const owner = await this.query(collection, { owner_of: { token_id: tokenId } });
    if (owner.owner !== this.address) throw new Error('NFT must be unbound and owned by your connected wallet');
    return this.query(collection, { nft_info: { token_id: tokenId } });
  }
  async bind(collection, id) {
    const nft = await this.ownNft(collection, id);
    return this.execute([tx.bind(collection, id, this.config.contracts.equipment, nft.extension.kind)], 'bind NFT for next race');
  }
  async list(collection, id, token, price) {
    await this.ownNft(collection, id);
    const info = await this.query(token, { token_info: {} });
    return this.execute([tx.list(collection, id, this.config.contracts.marketplace, token, tx.units(price, info.decimals))], 'list NFT');
  }
  async buyListing(id) {
    const listing = await this.query(this.config.contracts.marketplace, { listing: { id: tx.number(id) } });
    if (listing.status !== 'active' || listing.seller === this.address) throw new Error('Listing unavailable or owned by you');
    return this.execute([tx.send(listing.payment_token, this.config.contracts.marketplace, listing.price, { listing_id: listing.id })], 'buy NFT');
  }
  async fuse(ids) {
    const collection = this.config.contracts.commanderCollection;
    const items = await Promise.all(ids.map(id => this.ownNft(collection, id)));
    if (items.some(item => item.extension.kind !== 'commander' || item.extension.rarity !== items[0].extension.rarity || item.extension.rarity === 'legendary')) throw new Error('Fusion needs five same-rarity, non-legendary commanders');
    return this.execute(tx.fusion(this.config, ids, await (await this.reader()).getHeight()), 'fuse five commanders');
  }
  async finalize(module, id) {
    if (!['skins', 'recruitment'].includes(module)) throw new Error('Unknown NFT module');
    id = tx.number(id);
    const consumer = this.config.contracts[module]; const cfg = await this.query(consumer, { config: {} });
    const request = await this.query(cfg.randomness, { request: { consumer, id } });
    const instructions = [];
    if (!request.randomness) {
      const response = await fetch(`https://api.drand.sh/52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971/public/${request.round}`);
      if (!response.ok) throw new Error('The requested beacon is not available yet');
      const beacon = await response.json();
      if (beacon.round !== request.round || !/^[0-9a-f]{96}$/i.test(beacon.signature)) throw new Error('Invalid beacon response');
      const signature = Uint8Array.from(beacon.signature.match(/../g), byte => parseInt(byte, 16));
      instructions.push(tx.instruction(cfg.randomness, { submit: { consumer, id, signature: toBase64(signature) } }));
    } else if (module === 'skins') {
      const preview = await this.query(consumer, { preview: { id } });
      if (preview.duplicate_owned) throw new Error(`You currently own selected skin ${preview.duplicate_owned.code}; the same draw must wait`);
    }
    instructions.push(tx.instruction(consumer, { fulfill: { id } }));
    return this.execute(instructions, 'finalize NFT draw');
  }
}
import { rocketPrice } from './rocket-price.mjs';
