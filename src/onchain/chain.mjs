import { assessChainHealth } from './health.mjs';
export class ChainError extends Error {
  constructor(message, missing = false) { super(message); this.missing = missing; this.status = missing ? 404 : 503; }
}
class Pool {
  active = 0; waiting = [];
  async run(fn) {
    if (this.active >= 8) await new Promise(resolve => this.waiting.push(resolve));
    else this.active++;
    try { return await fn(); } finally {
      const next = this.waiting.shift(); if (next) next(); else this.active--;
    }
  }
}
export class Chain {
  pool = new Pool(); cache = new Map();
  constructor(config, fetcher = fetch) { this.config = config; this.fetch = fetcher; }
  async json(url, options = {}) {
    return this.pool.run(async () => {
      const response = await this.fetch(url, { ...options, signal: AbortSignal.timeout(10000) });
      const reader = response.body.getReader(); const chunks = []; let length = 0;
      while (true) {
        const { value, done } = await reader.read(); if (done) break;
        length += value.byteLength;
        if (length > 8 * 1024 * 1024) { await reader.cancel(); throw new ChainError('Chain response too large'); }
        chunks.push(value);
      }
      let data;
      try { data = JSON.parse(Buffer.concat(chunks).toString()); } catch { throw new ChainError('Invalid chain response'); }
      if (!response.ok || data.error || (data.code && !data.data)) {
        const message = String(data.message ?? data.error?.data ?? data.error?.message ?? response.statusText).slice(0, 500);
        throw new ChainError(message, /not found|does not exist/i.test(message));
      }
      return { data, headers: response.headers };
    });
  }
  async rpc(method, params = {}) {
    const { data } = await this.json(this.config.rpc, { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) });
    return data.result;
  }
  async tip() {
    const [result, info] = await Promise.all([this.rpc('status'), this.rpc('abci_info')]);
    if (result.node_info.network !== this.config.chainId) throw new ChainError('Chain ID does not match deployment');
    const consensusHeight = Number(result.sync_info.latest_block_height), applicationHeight = Number(info.response.last_block_height);
    const height = Math.min(consensusHeight, applicationHeight);
    if (![height, consensusHeight, applicationHeight].every(Number.isSafeInteger) || height < 1) throw new ChainError('Invalid chain height');
    if (height === consensusHeight) return { height, time: result.sync_info.latest_block_time, hash: result.sync_info.latest_block_hash, consensusHeight, applicationHeight };
    const block = await this.rpc('block', { height: String(height) });
    return { height, time: block.block.header.time, hash: block.block_id.hash, consensusHeight, applicationHeight };
  }
  async pinnedTip() {
    return this.tip();
  }
  async health() {
    const at = await this.tip();
    if (!this.validatorHealth || Date.now() - this.validatorHealth.at > 5000) {
      const { data } = await this.json(`${this.config.lcd}/cosmos/staking/v1beta1/validators?status=BOND_STATUS_BONDED&pagination.limit=200`);
      if (!Array.isArray(data.validators)) throw new ChainError('Invalid validator health response');
      this.validatorHealth = { at: Date.now(), validators: data.validators };
    }
    return { ...at, ...assessChainHealth(at, this.validatorHealth.validators) };
  }
  async lcd(path, height) {
    let pinned = height;
    for (let attempt = 0; ; attempt++) {
      try {
        const { data, headers } = await this.json(`${this.config.lcd}${path}`, { headers: { 'x-cosmos-block-height': String(pinned) } });
        if (headers.get('x-cosmos-block-height') !== String(pinned)) throw new ChainError('LCD did not honor pinned query height');
        return data;
      } catch (error) {
        // LCD can report its latest block before that app height is queryable.
        if (attempt < 3 && pinned > 1 && /height in the future/i.test(error.message)) { pinned--; continue; }
        throw error;
      }
    }
  }
  async smart(contract, message, height, optional = false) {
    const key = `${height}:${contract}:${JSON.stringify(message)}`;
    if (this.cache.has(key)) return this.cache.get(key);
    const pending = (async () => {
      try {
        const query = encodeURIComponent(Buffer.from(JSON.stringify(message)).toString('base64'));
        return (await this.lcd(`/cosmwasm/wasm/v1/contract/${contract}/smart/${query}`, height)).data;
      } catch (error) { if (optional && error.missing) return null; throw error; }
    })();
    this.cache.set(key, pending);
    if (this.cache.size > 2048) this.cache.delete(this.cache.keys().next().value);
    try { return await pending; } catch (error) { this.cache.delete(key); throw error; }
  }
  async raw(contract, key, height) {
    // Binary keys can contain '/' in base64. Some LCD gateways decode that into
    // another path segment and return Not Implemented; use pinned protobuf RPC.
    const request = QueryRawContractStateRequest.encode({ address: contract, queryData: key }).finish();
    for (let attempt = 0, pinned = height; ; attempt++, pinned--) {
      const result = await this.rpc('abci_query', { path: '/cosmwasm.wasm.v1.Query/RawContractState', data: Buffer.from(request).toString('hex'), height: String(pinned), prove: false });
      const response = result.response;
      if (Number(response.code ?? 0)) {
        if (attempt < 3 && pinned > 1 && /height in the future/i.test(response.log)) continue;
        throw new ChainError(response.log ?? 'Raw contract query failed');
      }
      if (Number(response.height) !== pinned) throw new ChainError('RPC did not honor pinned query height');
      const decoded = QueryRawContractStateResponse.decode(Buffer.from(response.value ?? '', 'base64')).data;
      return decoded.length ? JSON.parse(Buffer.from(decoded).toString()) : null;
    }
  }
}
import { QueryRawContractStateRequest, QueryRawContractStateResponse } from 'cosmjs-types/cosmwasm/wasm/v1/query';
