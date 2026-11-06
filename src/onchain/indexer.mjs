import { createHash } from 'node:crypto';
import { address } from './config.mjs';
import { decodeTxRaw, decodePubkey } from '@cosmjs/proto-signing';
import { pubkeyToAddress } from '@cosmjs/amino';

const ADDRESS_KEYS = new Set(['wallet', 'sender', 'recipient', 'owner', 'seller', 'buyer', 'from', 'to', 'creator', 'spender', 'receiver', 'fee_payer', 'new_owner', 'minter', 'sponsor']);
function attrs(event) {
  const values = event.attributes ?? [];
  const encoded = values.some(a => {
    const key = Buffer.from(a.key, 'base64').toString();
    return key === '_contract_address' || key === 'action' || ADDRESS_KEYS.has(key);
  });
  return values.map(a => ({ key: encoded ? Buffer.from(a.key, 'base64').toString() : a.key,
    value: encoded ? Buffer.from(a.value, 'base64').toString() : a.value }));
}
export function extractTransactions(block, results, watched) {
  const output = [];
  for (const [index, tx] of (results.txs_results ?? []).entries()) {
    if (Number(tx.code ?? 0) !== 0) continue;
    const events = []; const participants = new Set();
    for (const [eventIndex, event] of (tx.events ?? []).entries()) {
      const attributes = attrs(event);
      for (const a of attributes) if (ADDRESS_KEYS.has(a.key)) {
        try { participants.add(address(a.value)); } catch { /* Non-address attribute. */ }
      }
      const contract = attributes.find(a => a.key === '_contract_address')?.value;
      if (!watched.has(contract)) continue;
      events.push({ index: eventIndex, contract, action: attributes.find(a => a.key === 'action')?.value ?? event.type, attributes });
    }
    if (!events.length) continue;
    const raw = block.block.data.txs?.[index];
    if (!raw) throw new Error('Block results do not match block transactions');
    let profile = null;
    try {
      const decoded = decodeTxRaw(Buffer.from(raw, 'base64'));
      const match = decoded.body.memo.match(/ \| pilot:([A-Za-z0-9+/=]+)$/);
      if (match && decoded.authInfo.signerInfos.length === 1) {
        const wallet = address(pubkeyToAddress(decodePubkey(decoded.authInfo.signerInfos[0].publicKey), 'terra'));
        const nickname = Buffer.from(match[1], 'base64').toString('utf8').trim();
        if (nickname.length >= 2 && nickname.length <= 24) { profile = { wallet, nickname }; participants.add(wallet); }
      }
    } catch { /* Older transactions have no signed pilot metadata. */ }
    output.push({ hash: createHash('sha256').update(Buffer.from(raw, 'base64')).digest('hex').toUpperCase(), index, events, participants: [...participants], ...(profile ? { profile } : {}) });
  }
  return output;
}

export class Indexer {
  running = false; timer = null; lastError = null; latest = 0; stopped = false;
  constructor(chain, store, watched, startHeight) { Object.assign(this, { chain, store, watched, startHeight }); }
  async syncOnce(budget = 50) {
    if (this.running) return;
    this.running = true;
    try {
      const tip = await this.chain.tip(); this.latest = tip.height;
      let head = this.store.head();
      if (head) {
        if (head.height > tip.height) { this.store.rewind(tip.height); this.chain.cache?.clear(); return; }
        const canonical = await this.chain.rpc('block', { height: String(head.height) });
        if (canonical.block_id.hash !== head.hash) { this.store.rewind(head.height - 1); this.chain.cache?.clear(); this.lastError = 'Canonical chain changed; rewinding index'; return; }
      }
      let next = head ? head.height + 1 : this.startHeight;
      const through = Math.min(tip.height, next + budget - 1);
      while (next <= through) {
        const heights = Array.from({ length: Math.min(5, through - next + 1) }, (_, i) => next + i);
        const records = await Promise.all(heights.map(async height => {
          const [block, results] = await Promise.all([this.chain.rpc('block', { height: String(height) }), this.chain.rpc('block_results', { height: String(height) })]);
          if (block.block.header.chain_id !== this.chain.config.chainId || Number(block.block.header.height) !== height || Number(results.height) !== height) throw new Error('Invalid block identity');
          return { block, results, height };
        }));
        for (const { block, results, height } of records) {
          head = this.store.head();
          if (head && block.block.header.last_block_id.hash !== head.hash) throw new Error('Block continuity changed during indexing');
          this.store.commit({ height, hash: block.block_id.hash, time: block.block.header.time }, extractTransactions(block, results, this.watched));
        }
        next += records.length;
      }
      this.lastError = null;
    } catch (error) { this.lastError = error.message; }
    finally { this.running = false; }
  }
  start() {
    this.stopped = false;
    const loop = async () => {
      await this.syncOnce();
      if (!this.stopped) this.timer = setTimeout(loop, this.latest - (this.store.head()?.height ?? this.startHeight - 1) > 50 ? 0 : 1000);
    };
    void loop();
  }
  async stop() { this.stopped = true; clearTimeout(this.timer); while (this.running) await new Promise(r => setTimeout(r, 25)); }
  status() { return { chain_height: this.latest, indexed_height: this.store.head()?.height ?? this.startHeight - 1, start_height: this.startHeight, events: this.store.count(), error: this.lastError }; }
}
