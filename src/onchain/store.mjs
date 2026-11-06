import Database from 'better-sqlite3';
import { mkdirSync } from 'node:fs';
import { dirname } from 'node:path';

export class Store {
  constructor(filename, identity) {
    if (filename !== ':memory:') mkdirSync(dirname(filename), { recursive: true });
    this.db = new Database(filename);
    this.db.pragma('journal_mode = WAL'); this.db.pragma('foreign_keys = ON');
    this.db.exec(`CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS blocks(height INTEGER PRIMARY KEY,hash TEXT NOT NULL,time TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS transactions(hash TEXT PRIMARY KEY,height INTEGER NOT NULL REFERENCES blocks(height) ON DELETE CASCADE,tx_index INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS participants(hash TEXT NOT NULL REFERENCES transactions(hash) ON DELETE CASCADE,address TEXT NOT NULL,PRIMARY KEY(hash,address));
      CREATE INDEX IF NOT EXISTS participant_address ON participants(address,hash);
      CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY AUTOINCREMENT,height INTEGER NOT NULL REFERENCES blocks(height) ON DELETE CASCADE,
        tx_hash TEXT NOT NULL REFERENCES transactions(hash) ON DELETE CASCADE,event_index INTEGER NOT NULL,contract TEXT NOT NULL,action TEXT NOT NULL,attributes TEXT NOT NULL,
        UNIQUE(tx_hash,event_index));
      CREATE INDEX IF NOT EXISTS event_contract ON events(contract,id);
      CREATE TABLE IF NOT EXISTS pilot_profiles(tx_hash TEXT PRIMARY KEY REFERENCES transactions(hash) ON DELETE CASCADE,wallet TEXT NOT NULL,nickname TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS rocket_hits(id INTEGER PRIMARY KEY AUTOINCREMENT,event_id INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
        hit_index INTEGER NOT NULL,height INTEGER NOT NULL REFERENCES blocks(height) ON DELETE CASCADE,contract TEXT NOT NULL,attacker TEXT NOT NULL,target TEXT NOT NULL,payload TEXT NOT NULL,UNIQUE(event_id,hit_index));
      CREATE TABLE IF NOT EXISTS reward_cycles(contract TEXT NOT NULL,cycle INTEGER NOT NULL,height INTEGER NOT NULL REFERENCES blocks(height) ON DELETE CASCADE,PRIMARY KEY(contract,cycle));
      CREATE TABLE IF NOT EXISTS clan_rewards(contract TEXT NOT NULL,cycle INTEGER NOT NULL,clan_id INTEGER NOT NULL,height INTEGER NOT NULL REFERENCES blocks(height) ON DELETE CASCADE,
        time TEXT NOT NULL,spaceusd TEXT NOT NULL,srt TEXT NOT NULL,premium TEXT NOT NULL,gross TEXT NOT NULL,maintenance TEXT NOT NULL,PRIMARY KEY(contract,cycle,clan_id));`);
    const existing = this.meta('identity');
    if (existing && existing !== identity) { this.db.close(); throw new Error('Index database belongs to a different deployment/chain'); }
    this.setMeta('identity', identity);
    for (const event of this.db.prepare("SELECT e.id,e.height,e.contract,e.attributes FROM events e WHERE e.attributes LIKE '%rocket_hit%' AND NOT EXISTS(SELECT 1 FROM rocket_hits r WHERE r.event_id=e.id)").all()) {
      this.indexRocketHits(event.id, event.height, event.contract, JSON.parse(event.attributes));
    }
    this.commit = this.db.transaction((block, transactions) => {
      this.db.prepare('INSERT INTO blocks(height,hash,time) VALUES(?,?,?)').run(block.height, block.hash, block.time);
      for (const tx of transactions) {
        this.db.prepare('INSERT INTO transactions(hash,height,tx_index) VALUES(?,?,?)').run(tx.hash, block.height, tx.index);
        for (const wallet of tx.participants) this.db.prepare('INSERT INTO participants(hash,address) VALUES(?,?)').run(tx.hash, wallet);
        if (tx.profile) this.db.prepare('INSERT INTO pilot_profiles(tx_hash,wallet,nickname) VALUES(?,?,?)').run(tx.hash, tx.profile.wallet, tx.profile.nickname);
        for (const event of tx.events) {
          const inserted = this.db.prepare('INSERT INTO events(height,tx_hash,event_index,contract,action,attributes) VALUES(?,?,?,?,?,?)')
            .run(block.height, tx.hash, event.index, event.contract, event.action, JSON.stringify(event.attributes));
          this.indexRocketHits(Number(inserted.lastInsertRowid), block.height, event.contract, event.attributes);
        }
      }
    });
    this.saveRewardCycle = this.db.transaction((cycle, rows) => {
      for (const row of rows) this.db.prepare('INSERT OR REPLACE INTO clan_rewards(contract,cycle,clan_id,height,time,spaceusd,srt,premium,gross,maintenance) VALUES(?,?,?,?,?,?,?,?,?,?)')
        .run(cycle.contract, cycle.cycle, row.clanId, cycle.height, cycle.time, row.spaceusd, row.srt, row.premium, row.gross, row.maintenance);
      this.db.prepare('INSERT OR REPLACE INTO reward_cycles(contract,cycle,height) VALUES(?,?,?)').run(cycle.contract, cycle.cycle, cycle.height);
    });
  }
  meta(key) { return this.db.prepare('SELECT value FROM meta WHERE key=?').get(key)?.value ?? null; }
  setMeta(key, value) { this.db.prepare('INSERT INTO meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value').run(key, String(value)); }
  head() { return this.db.prepare('SELECT * FROM blocks ORDER BY height DESC LIMIT 1').get() ?? null; }
  canonicalHash(height) { return this.db.prepare('SELECT hash FROM blocks WHERE height=?').get(height)?.hash ?? null; }
  raceActions(contract, race, height) {
    return this.db.prepare("SELECT attributes FROM events WHERE contract=? AND action='queue_block_boost' AND height<=? ORDER BY id").all(contract, height).flatMap(row => {
      const a = Object.fromEntries(JSON.parse(row.attributes).map(a => [a.key, a.value]));
      if (Number(a.race) !== race || !['speed', 'acceleration', 'temporary', 'rocket', 'heal'].includes(a.kind) || !/^\d+$/.test(a.count ?? '')) return [];
      return [{ wallet: a.wallet, tick: Number(a.tick), kind: a.kind, count: a.count, ...(a.kind === 'heal' ? { paid: a.paid } : {}) }];
    });
  }
  rewind(height) { this.db.prepare('DELETE FROM blocks WHERE height>?').run(height); }
  events({ wallet, contract, before, limit = 30, gameplay = false } = {}) {
    const clauses = []; const values = [];
    if (wallet) { clauses.push('EXISTS(SELECT 1 FROM participants p WHERE p.hash=e.tx_hash AND p.address=?)'); values.push(wallet); }
    if (contract) { clauses.push('e.contract=?'); values.push(contract); }
    if (before) { clauses.push('e.id<?'); values.push(before); }
    if (gameplay) clauses.push("(e.action NOT IN ('execute','transfer','send','submit_randomness','request_randomness','fulfill_randomness','seal_race_blocks','check_race_tick','advance_race','advance_fleet_race','begin_boundary','end_boundary','checkpoint','wasm') OR EXISTS(SELECT 1 FROM json_each(e.attributes) a WHERE json_extract(a.value,'$.key')='race_ended'))");
    const rows = this.db.prepare(`SELECT e.*,b.time FROM events e JOIN blocks b ON b.height=e.height ${clauses.length ? `WHERE ${clauses.join(' AND ')}` : ''} ORDER BY e.id DESC LIMIT ?`)
      .all(...values, limit);
    return rows.map(row => ({ ...row, attributes: JSON.parse(row.attributes) }));
  }
  count() { return this.db.prepare('SELECT COUNT(*) AS total FROM events').get().total; }
  raceEnd(contract, race) {
    return this.db.prepare(`SELECT e.height,e.tx_hash,b.time FROM events e JOIN blocks b ON b.height=e.height
      WHERE e.contract=? AND EXISTS(SELECT 1 FROM json_each(e.attributes) a
        WHERE json_extract(a.value,'$.key')='race_ended' AND json_extract(a.value,'$.value')=?) ORDER BY e.id LIMIT 1`).get(contract, String(race)) ?? null;
  }
  profileName(wallet) { return this.db.prepare('SELECT p.nickname FROM pilot_profiles p JOIN transactions t ON t.hash=p.tx_hash WHERE p.wallet=? ORDER BY t.height DESC,t.tx_index DESC LIMIT 1').get(wallet)?.nickname ?? null; }
  indexRocketHits(eventId, height, contract, attributes) {
    let index = 0;
    for (const attribute of attributes) if (attribute.key === 'rocket_hit') {
      try {
        const hit = JSON.parse(attribute.value);
        if (typeof hit.attacker !== 'string' || typeof hit.target !== 'string' || !Number.isSafeInteger(hit.race) || !Number.isSafeInteger(hit.tick)) continue;
        this.db.prepare('INSERT OR IGNORE INTO rocket_hits(event_id,hit_index,height,contract,attacker,target,payload) VALUES(?,?,?,?,?,?,?)')
          .run(eventId, index++, height, contract, hit.attacker, hit.target, JSON.stringify(hit));
      } catch { /* Other contracts cannot create a valid combat record accidentally. */ }
    }
  }
  rocketHits(contracts, wallet, after = null, limit = 30) {
    const where = [`r.contract IN (${contracts.map(() => '?').join(',')})`], values = [...contracts];
    if (wallet) { where.push('(r.attacker=? OR r.target=?)'); values.push(wallet, wallet); }
    if (after !== null) { where.push('r.id>?'); values.push(after); }
    const rows = this.db.prepare(`SELECT r.*,b.time,e.tx_hash FROM rocket_hits r JOIN blocks b ON b.height=r.height JOIN events e ON e.id=r.event_id
      WHERE ${where.join(' AND ')} ORDER BY r.id ${after === null ? 'DESC' : 'ASC'} LIMIT ?`).all(...values, limit);
    if (after === null) rows.reverse();
    return rows.map(row => ({ id: row.id, height: row.height, time: row.time, txhash: row.tx_hash, ...JSON.parse(row.payload) }));
  }
  completedCycles(contracts) {
    const rows = this.db.prepare(`SELECT e.contract,e.height,b.time,b.hash,e.attributes FROM events e JOIN blocks b ON b.height=e.height
      WHERE e.contract IN (${contracts.map(() => '?').join(',')}) AND EXISTS(SELECT 1 FROM json_each(e.attributes) a WHERE json_extract(a.value,'$.key')='settled_cycle') ORDER BY e.id`).all(...contracts);
    return rows.map(row => ({ contract: row.contract, height: row.height, time: row.time, hash: row.hash, cycle: Number(JSON.parse(row.attributes).find(a => a.key === 'settled_cycle').value) }));
  }
  cycleStart(cycle, contracts, through) {
    return this.db.prepare(`SELECT e.contract,e.height FROM events e WHERE e.action='begin_clan_cycle' AND e.contract IN (${contracts.map(() => '?').join(',')}) AND e.height<=?
      AND EXISTS(SELECT 1 FROM json_each(e.attributes) a WHERE json_extract(a.value,'$.key')='cycle' AND json_extract(a.value,'$.value')=?) ORDER BY e.id DESC LIMIT 1`).get(...contracts, through, String(cycle)) ?? null;
  }
  hasRewardCycle(contract, cycle) { return !!this.db.prepare('SELECT 1 FROM reward_cycles WHERE contract=? AND cycle=?').get(contract, cycle); }
  clanRewardHistory(clanId) {
    const rows = this.db.prepare('SELECT * FROM clan_rewards WHERE clan_id=? ORDER BY cycle DESC').all(clanId);
    const totals = { spaceusd: 0n, srt: 0n, premium: 0n, gross: 0n, maintenance: 0n };
    for (const row of rows) for (const key of Object.keys(totals)) totals[key] += BigInt(row[key]);
    return { totals: Object.fromEntries(Object.entries(totals).map(([key, value]) => [key, value.toString()])), cycles: rows };
  }
  boostEntries(wallet, contract) {
    const rows = this.db.prepare("SELECT attributes FROM events WHERE contract=? AND action='queue_block_boost'").all(contract);
    const entries = new Map();
    for (const row of rows) {
      const attrs = Object.fromEntries(JSON.parse(row.attributes).map(a => [a.key, a.value]));
      if (attrs.wallet !== wallet) continue;
      const race = Number(attrs.race), tick = Number(attrs.tick);
      if (Number.isSafeInteger(race) && race > 0 && Number.isSafeInteger(tick) && tick > 0) {
        const key = `${race}:${tick}`, heals = (entries.get(key)?.heals ?? 0) + (attrs.kind === 'heal' ? 1 : 0);
        entries.set(key, { race, tick, ...(heals ? { heals } : {}) });
      }
    }
    return [...entries.values()];
  }
  boostHistory(wallet, contract) {
    const contracts = Array.isArray(contract) ? contract : [contract];
    const rows = this.db.prepare(`SELECT e.attributes,e.tx_hash,e.contract FROM events e
      WHERE e.contract IN (${contracts.map(() => '?').join(',')}) AND e.action='queue_block_boost'
      AND EXISTS(SELECT 1 FROM participants p WHERE p.hash=e.tx_hash AND p.address=?)
      ORDER BY e.id DESC LIMIT 12`).all(...contracts, wallet);
    return rows.flatMap(row => {
      const attrs = Object.fromEntries(JSON.parse(row.attributes).map(a => [a.key, a.value]));
      return attrs.wallet === wallet ? [{ race: Number(attrs.race), tick: Number(attrs.tick), paid: attrs.paid, kind: attrs.kind, count: Number(attrs.count), txhash: row.tx_hash, contract: row.contract, liveActivated: attrs.live_activated === 'true' }] : [];
    });
  }
  victories(wallet, contract) {
    const contracts = Array.isArray(contract) ? contract : [contract];
    const rows = this.db.prepare(`SELECT e.attributes,e.tx_hash,b.time FROM events e JOIN blocks b ON b.height=e.height
      WHERE e.contract IN (${contracts.map(() => '?').join(',')}) AND EXISTS(SELECT 1 FROM json_each(e.attributes) a
        WHERE json_extract(a.value,'$.key')='winner' AND json_extract(a.value,'$.value')=?)
      ORDER BY e.id DESC`).all(...contracts, wallet);
    const seen = new Set();
    return rows.flatMap(row => {
      const attrs = Object.fromEntries(JSON.parse(row.attributes).map(a => [a.key, a.value]));
      const race = Number(attrs.race_ended);
      if (!race || seen.has(race)) return []; seen.add(race);
      return [{ race, txhash: row.tx_hash, time: row.time }];
    });
  }
  rewardClaims(wallet, rewards, srt, spaceusd) {
    const contracts = Array.isArray(rewards) ? rewards : [rewards];
    const transactions = this.db.prepare(`SELECT DISTINCT e.tx_hash,e.height,b.time FROM events e JOIN blocks b ON b.height=e.height
      WHERE e.contract IN (${contracts.map(() => '?').join(',')}) AND e.action='claim_rewards' AND EXISTS(SELECT 1 FROM participants p WHERE p.hash=e.tx_hash AND p.address=?)
      ORDER BY e.id DESC LIMIT 20`).all(...contracts, wallet);
    return transactions.map(tx => {
      const transfers = this.db.prepare("SELECT contract,attributes FROM events WHERE tx_hash=? AND action='transfer'").all(tx.tx_hash);
      const amounts = { srt: 0n, spaceusd: 0n };
      for (const event of transfers) {
        const attrs = Object.fromEntries(JSON.parse(event.attributes).map(a => [a.key, a.value]));
        if (!contracts.includes(attrs.from) || attrs.to !== wallet || !/^\d+$/.test(attrs.amount ?? '')) continue;
        if (event.contract === srt) amounts.srt += BigInt(attrs.amount);
        if (event.contract === spaceusd) amounts.spaceusd += BigInt(attrs.amount);
      }
      return { ...tx, srt: amounts.srt.toString(), spaceusd: amounts.spaceusd.toString() };
    }).filter(claim => BigInt(claim.srt) > 0n || BigInt(claim.spaceusd) > 0n);
  }
  referralEarnings(sponsor, contract) {
    const rows = this.db.prepare(`SELECT e.attributes FROM events e
      JOIN participants p ON p.hash=e.tx_hash
      WHERE p.address=? AND e.contract=? AND e.action IN ('buy_ship','upgrade_ship')`).all(sponsor, contract);
    let total = 0n;
    for (const row of rows) {
      const attributes = JSON.parse(row.attributes);
      if (attributes.find(a => a.key === 'sponsor')?.value !== sponsor) continue;
      const value = attributes.find(a => a.key === 'referral')?.value;
      if (/^\d+$/.test(value ?? '')) total += BigInt(value);
    }
    return total.toString();
  }
  close() { this.db.close(); }
}
