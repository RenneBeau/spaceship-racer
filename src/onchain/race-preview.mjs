// Non-authoritative replay of the no-regeneration protocol. Payouts remain on chain.
import { createHash } from 'node:crypto';
import { healingQuote } from './healing.mjs';
import { fleetHealth } from './leaderboards.mjs';

const ZERO = () => ({ speed: 0n, acceleration: 0n, temporary: 0n, rockets: 0n, heal: 0n });
const bonus = (p, kind) => p.commander?.bonus === kind ? BigInt(p.commander.bonus_bps) : 0n;
const clonePlayer = p => ({ ...p, safe_speed: BigInt(p.safe_speed), distance: BigInt(p.distance), acceleration_count: BigInt(p.acceleration_count),
  utility_spent: BigInt(p.utility_spent), salvo: BigInt(p.salvo), materialized_tick: p.materialized_tick ?? 0,
  ships: p.ships.map(s => Object.fromEntries(Object.entries(s).map(([k, v]) => [k, BigInt(v)]))),
  temporary: p.temporary.map(t => ({ expires: t.expires, distance: BigInt(t.distance) })) });
const wire = p => ({ ...p, safe_speed: String(p.safe_speed), distance: String(p.distance), acceleration_count: Number(p.acceleration_count),
  utility_spent: String(p.utility_spent), salvo: Number(p.salvo), ships: p.ships.map(s => Object.fromEntries(Object.entries(s).map(([k, v]) => [k, String(v)]))),
  temporary: p.temporary.map(t => ({ expires: t.expires, distance: String(t.distance) })) });
export function floorSum(n, m, a, b) {
  let total = 0n;
  while (n > 0n) {
    if (a >= m) { total += n * (n - 1n) / 2n * (a / m); a %= m; }
    if (b >= m) { total += n * (b / m); b %= m; }
    const y = a * n + b; if (y < m) break;
    n = y / m; b = y % m; [m, a] = [a, m];
  }
  return total;
}
function factor(p) {
  let loss = 0n;
  for (const [i, w] of [[0, 5n], [1, 1n], [2, 3n]]) {
    const s = p.ships[i]; if (s.base_hp > 0n) loss += (s.base_hp - s.hp) * w * 1000000n / s.base_hp;
  }
  return 9000000n - (loss < 9000000n ? loss : 9000000n);
}
const acceleration = (p, params) => BigInt(params.acceleration) * 1000n * p.acceleration_count * BigInt(params.tick_seconds) * (10000n + bonus(p, 'acceleration')) / 10000n;
function catchUp(p, tick, params) {
  if (tick <= p.materialized_tick) return;
  if (p.alive) {
    const n = BigInt(tick - p.materialized_tick), a = acceleration(p, params), f = factor(p);
    p.distance += floorSum(n, 9000000n, a * f, p.safe_speed * f) * BigInt(params.tick_seconds);
    p.safe_speed += a * n; p.temporary = p.temporary.filter(t => t.expires > tick);
  }
  p.materialized_tick = tick; p.salvo = 0n; p.utility_spent = 0n;
}
function move(p, b, tick, params) {
  catchUp(p, tick - 1, params); p.salvo = 0n; p.utility_spent = 0n; p.materialized_tick = tick;
  if (!p.alive) return;
  p.temporary = p.temporary.filter(t => t.expires > tick);
  p.safe_speed += BigInt(params.speed_boost) * 1000n * b.speed; p.acceleration_count += b.acceleration;
  if (b.temporary > 0n) p.temporary.push({ expires: tick + params.temporary_ticks, distance: BigInt(params.temporary_distance) * 1000n * b.temporary });
  p.salvo = p.ships[2].hp === 0n ? 0n : b.rockets;
  p.utility_spent = BigInt(params.boost_price) * (b.speed + b.acceleration + b.temporary);
  p.distance += p.safe_speed * factor(p) / 9000000n * BigInt(params.tick_seconds);
  p.safe_speed += acceleration(p, params);
}
const weight = p => p.alive ? p.temporary.reduce((sum, t) => sum + t.distance, p.distance) : 0n;
function draw(chainId, game, race, tick, seed, domain, actor, ordinal, bound) {
  const max = (1n << 256n) - 1n, limit = max - max % bound;
  for (let attempt = 0; attempt < 16; attempt++) {
    const h = createHash('sha256');
    for (const part of [Buffer.from(chainId), Buffer.from(game), Buffer.from(domain), Buffer.from(seed, 'base64')]) {
      const len = Buffer.alloc(8); len.writeBigUInt64BE(BigInt(part.length)); h.update(len); h.update(part);
    }
    const suffix = Buffer.alloc(14); suffix.writeBigUInt64BE(BigInt(race)); suffix.writeUInt32BE(tick, 8); suffix.writeUInt16BE(actor, 12); h.update(suffix);
    if (ordinal <= 65535) { const o = Buffer.alloc(2); o.writeUInt16BE(ordinal); h.update(o); }
    else { h.update('large-ordinal'); const o = Buffer.alloc(4); o.writeUInt32BE(ordinal); h.update(o); }
    h.update(Buffer.from([attempt])); const value = BigInt(`0x${h.digest('hex')}`);
    if (value < limit) return value % bound;
  }
  throw Error('Preview random rejection limit');
}
class Tree {
  constructor(players) { this.n = players.length; this.values = Array(this.n + 1).fill(0n); players.forEach((p, i) => this.add(i + 1, weight(p))); }
  add(i, delta) { while (i <= this.n) { this.values[i] += delta; i += i & -i; } }
  prefix(i) { let sum = 0n; while (i > 0) { sum += this.values[i]; i -= i & -i; } return sum; }
  find(offset) { let i = 0, bit = 1; while (bit * 2 <= this.n) bit *= 2;
    for (; bit; bit >>= 1) { const next = i + bit; if (next <= this.n && this.values[next] <= offset) { offset -= this.values[next]; i = next; } } return i;
  }
}
export function replayRace({ chainId, game, race, snapshots, actions, seeds, throughTick, parameters, healing = null }) {
  const players = snapshots.map(([_i, p]) => clonePlayer(p)), byWallet = new Map(players.map((p, i) => [p.wallet, i]));
  const events = new Map(), hits = [], deaths = new Map(); let work = 0;
  for (const a of actions) {
    if (a.tick > throughTick) continue;
    if (!events.has(a.tick)) events.set(a.tick, new Map());
    const map = events.get(a.tick), b = map.get(a.wallet) ?? ZERO();
    const key = a.kind === 'rocket' ? 'rockets' : a.kind;
    if (!(key in b) || !byWallet.has(a.wallet)) continue;
    b[key] += BigInt(a.kind === 'heal' ? a.paid ?? '0' : a.count); map.set(a.wallet, b);
  }
  for (const tick of [...events.keys()].sort((a, b) => a - b)) {
    const boosts = events.get(tick);
    for (const p of players) {
      const b = boosts.get(p.wallet) ?? ZERO(); let repairSpent = 0n;
      catchUp(p, tick - 1, parameters);
      if (healing?.enabled && b.heal > 0n) {
        const quote = healingQuote(p, healing), price = BigInt(quote.price);
        if (price > 0n && price <= b.heal) {
          if (!p.alive) {
            const death = deaths.get(p.wallet); if (!death) throw Error('Missing destruction checkpoint in preview');
            p.distance = death.distance; p.temporary = death.temporary.filter(effect => effect.expires > tick);
            p.safe_speed = death.safe_speed + acceleration(p, parameters) * BigInt(Math.max(0, tick - death.tick - 1));
          }
          const full = fleetHealth(p);
          p.ships.forEach((ship, i) => { ship.hp = ship.base_hp; ship.shield = BigInt(full.ships[i].maxShield); ship.regen = 0n; });
          p.alive = true; repairSpent = price;
        }
      }
      move(p, b, tick, parameters); p.utility_spent += repairSpent;
    }
    const tree = new Tree(players); let total = tree.prefix(players.length);
    for (let i = 0; i < players.length; i++) {
      const attacker = players[i];
      for (let rocket = 0; rocket < Number(attacker.salvo); rocket++) {
        const own = weight(attacker), eligible = total - own; if (eligible === 0n) break;
        if (++work > 50000) throw Object.assign(Error('Preview work budget reached'), { previewPending: true });
        const seed = seeds.get(tick); if (!seed) throw Error('Missing verified preview seed');
        let offset = draw(chainId, game, race, tick, seed, 'rocket-target', i + 1, rocket, eligible);
        if (offset >= tree.prefix(i)) offset += own;
        const targetIndex = tree.find(offset), target = players[targetIndex], old = weight(target);
        const before = target.ships.map(s => ({ ...s }));
        const critical = draw(chainId, game, race, tick, seed, 'rocket-critical', i + 1, rocket, 10000n) < bonus(attacker, 'critical');
        const damage = attacker.ships[2].attack * BigInt(parameters.rocket_damage_bps) / 10000n * (critical ? 2n : 1n);
        for (const index of [1, 2, 0]) {
          const ship = target.ships[index]; if (ship.hp === 0n) continue;
          let incoming = damage;
          if (index === 1) { const absorbed = ship.shield < incoming ? ship.shield : incoming; ship.shield -= absorbed; incoming -= absorbed; }
          ship.hp = ship.hp > incoming ? ship.hp - incoming : 0n; break;
        }
        if (target.ships.every(s => s.hp === 0n)) {
          deaths.set(target.wallet, { tick, distance: target.distance, safe_speed: target.safe_speed, temporary: target.temporary.map(effect => ({ ...effect })) });
          target.alive = false; target.distance = 0n; target.temporary = [];
        }
        const now = weight(target); tree.add(targetIndex + 1, now - old); total += now - old;
        const hpBefore = before.reduce((a, s) => a + s.hp, 0n), hpAfter = target.ships.reduce((a, s) => a + s.hp, 0n);
        hits.push({ id: `preview:${race}:${tick}:${i + 1}:${rocket}`, preview: true, race, tick, ordinal: rocket,
          attacker: attacker.wallet, target: target.wallet, critical, damage: String(damage), hp_before: String(hpBefore), hp_after: String(hpAfter), hp_lost: String(hpBefore - hpAfter),
          max_hp: String(target.ships.reduce((a, s) => a + s.base_hp, 0n)), shield_before: String(before[1].shield), shield_after: String(target.ships[1].shield),
          shield_lost: String(before[1].shield - target.ships[1].shield), destroyed: target.ships.every(s => s.hp === 0n),
          ships: before.map((s, index) => ({ index, hp_before: String(s.hp), hp_after: String(target.ships[index].hp), max_hp: String(s.base_hp), shield_before: String(s.shield), shield_after: String(target.ships[index].shield) })) });
      }
    }
  }
  for (const p of players) catchUp(p, throughTick, parameters);
  return { players: new Map(players.map(p => [p.wallet, wire(p)])), hits, throughTick, preview: true };
}

export function createRacePreview(chain, store, game) {
  const contexts = new Map(); let cached = null, lastHead = null;
  return { async get(s, state, configuration) {
    if ((configuration.accounting_version ?? 0) < 2 || !state.start_height || s.at.height < state.start_height || !['waiting', 'movement', 'combat'].includes(state.phase)) return null;
    const head = store.head(); if (!head || head.height < state.start_height) return null;
    if (lastHead && store.canonicalHash(lastHead.height) !== lastHead.hash) { contexts.clear(); cached = null; }
    lastHead = head;
    const key = `${s.at.height}:${head.height}:${head.hash}:${state.race}:${state.replay_until}`;
    if (cached?.key === key) return cached.promise;
    const promise = (async () => {
      const contextKey = `${state.race}:${state.start_height}`;
      let context = contexts.get(contextKey);
      if (!context) {
        context = { seeds: new Map(), snapshots: (async () => {
          const snapshots = []; let after = null, openingHeight = state.start_height;
          // A hard rollback can leave its exact historical IAVL version unreadable.
          // Only accept the next block if it is still the untouched opening state.
          try {
            await chain.smart(game, { players: { start_after: null, limit: 20 } }, openingHeight);
          } catch (error) {
            if (!/code 111222: panic/.test(error.message) || openingHeight >= s.at.height) throw error;
            const next = await chain.smart(game, { state: {} }, openingHeight + 1);
            if (next.race !== state.race || next.start_height !== state.start_height || next.tick !== 1 || next.phase !== 'waiting' || next.replay_until) throw error;
            openingHeight++;
          }
          for (;;) {
            const page = await chain.smart(game, { players: { start_after: after, limit: 20 } }, openingHeight);
            if (openingHeight !== state.start_height && page.some(([_i, p]) => (p.materialized_tick ?? 0) !== 0 || p.distance !== '0')) throw Error('Recovery preview snapshot is not pristine');
            snapshots.push(...page.filter(([_i, p]) => p.race === state.race));
            if (page.length < 20 || snapshots.length >= state.participants) break; after = page.at(-1)[0];
          }
          if (snapshots.length !== state.participants) throw Error('Incomplete preview snapshot');
          return snapshots;
        })().catch(error => { contexts.delete(contextKey); throw error; }) };
        contexts.set(contextKey, context);
        if (contexts.size > 4) contexts.delete(contexts.keys().next().value);
      }
      const snapshots = await context.snapshots, seeds = context.seeds;
      const verified = state.replay_until || (state.phase === 'waiting' ? Math.max(0, state.tick - 1) : state.tick);
      const throughTick = Math.min(verified, Math.max(0, head.height - state.start_height));
      const actions = store.raceActions(game, state.race, Math.min(s.at.height, head.height));
      for (const tick of new Set(actions.filter(a => a.kind === 'rocket' && a.tick <= throughTick).map(a => a.tick))) {
        if (seeds.has(tick)) continue;
        if ((configuration.accounting_version ?? 0) >= 3) {
          const seed = await s.smart(game, { tick_seed: { race: state.race, tick } });
          if (!seed) throw Error('Preview seed has not been verified'); seeds.set(tick, seed);
        } else {
          const id = await s.smart(game, { seed_request: { race: state.race, tick } });
          const request = await s.smart(configuration.randomness, { request: { consumer: game, id } });
          if (!request.randomness) throw Error('Preview seed has not been verified'); seeds.set(tick, request.randomness);
        }
      }
      const result = replayRace({ chainId: chain.config.chainId, game, race: state.race, snapshots, actions, seeds, throughTick, parameters: configuration.parameters, healing: configuration.healing });
      return { ...result, actions, stale: head.height < s.at.height };
    })();
    cached = { key, promise };
    try { return await promise; } catch (error) { cached = null; if (error.previewPending) return null; throw error; }
  } };
}
