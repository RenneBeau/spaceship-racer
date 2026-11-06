// Immutable completed-cycle settlements are materialized separately from live
// leaderboards. Amounts are integer micro-tokens, never floating-point sums.
export class ClanRewardHistory {
  running = false; stopping = false; timer = null; lastError = null;
  constructor(chain, store, config) { Object.assign(this, { chain, store, config }); }
  contracts() { return [this.config.contracts.game, ...(this.config.previousGames ?? [])]; }
  status() {
    const cycles = this.store.completedCycles(this.contracts());
    const fromHeight = Number(this.store.meta('start_height') ?? 1);
    return { totalCycles: cycles.length, indexedCycles: cycles.filter(c => this.store.hasRewardCycle(c.contract, c.cycle)).length,
      fromHeight, fullHistory: fromHeight <= (this.config.historyStartHeight ?? fromHeight), error: this.lastError };
  }
  async tick() {
    if (this.running) return; this.running = true;
    try {
      const cycle = this.store.completedCycles(this.contracts()).find(c => !this.store.hasRewardCycle(c.contract, c.cycle));
      if (!cycle) return false;
      const tip = await this.chain.pinnedTip(), players = []; let after = null;
      for (;;) {
        const page = await this.chain.smart(this.config.contracts.fleet, { players: { start_after: after, limit: 100 } }, tip.height);
        players.push(...page); if (!page.length) break; after = page.at(-1);
      }
      const budget = await this.chain.smart(cycle.contract, { state: {} }, cycle.height);
      const entries = await Promise.all(players.map(async wallet => {
        const score = await this.chain.smart(cycle.contract, { cycle_player: { cycle: cycle.cycle, wallet } }, cycle.height);
        if (BigInt(score.tier_sum) === 0n) return null;
        const settlement = await this.chain.smart(cycle.contract, { settlement: { cycle: cycle.cycle, wallet } }, cycle.height);
        return { wallet, score, settlement };
      }));
      const groups = new Map(); let totalScore = 0n;
      for (const entry of entries.filter(Boolean)) {
        totalScore += BigInt(entry.score.score);
        if (entry.score.clan === null) continue;
        const id = entry.score.clan;
        if (!groups.has(id)) groups.set(id, { clanId: id, score: 0n, spaceusd: 0n, srt: 0n, gross: 0n, maintenance: 0n, premium: 0n });
        const group = groups.get(id), row = entry.settlement;
        group.score += BigInt(entry.score.score); group.gross += BigInt(row.gross_usd); group.maintenance += BigInt(row.maintenance);
        group.spaceusd += BigInt(row.gross_usd) - BigInt(row.maintenance); group.srt += BigInt(row.srt);
      }
      let bps = 0n;
      if (totalScore > 0n && groups.size) {
        const clans = Object.entries(this.config.contracts).filter(([key]) => key === 'clans' || key.startsWith('legacyClans')).map(([, value]) => value);
        const start = this.store.cycleStart(cycle.cycle, clans, cycle.height);
        if (!start) throw new Error(`Cycle ${cycle.cycle} start is not indexed yet`);
        const cfg = await this.chain.smart(start.contract, { config: {} }, start.height);
        const initial = await this.chain.smart(cfg.engine, { state: {} }, start.height);
        bps = BigInt(initial.leader_bps);
      }
      const rows = [...groups.values()].map(group => {
        const premium = totalScore ? BigInt(budget.budget_usd) * group.score / totalScore * bps / 10000n : 0n;
        return { clanId: group.clanId, spaceusd: (group.spaceusd + premium).toString(), srt: group.srt.toString(), premium: premium.toString(), gross: group.gross.toString(), maintenance: group.maintenance.toString() };
      });
      const canonical = await this.chain.rpc('block', { height: String(cycle.height) });
      if (canonical.block_id.hash !== cycle.hash) throw new Error('Waiting for canonical cycle replay');
      this.store.saveRewardCycle(cycle, rows); this.lastError = null;
      return true;
    } catch (error) { this.lastError = error.message; }
    finally { this.running = false; }
  }
  start() {
    this.stopping = false;
    const loop = async () => { const worked = await this.tick(); if (!this.stopping) this.timer = setTimeout(loop, this.lastError ? 3000 : worked ? 25 : 1000); };
    void loop();
  }
  async stop() { this.stopping = true; clearTimeout(this.timer); while (this.running) await new Promise(resolve => setTimeout(resolve, 25)); }
}
