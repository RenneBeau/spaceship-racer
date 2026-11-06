const zero = () => ({ hp: '0', maxHp: '0', shield: '0', maxShield: '0', combatShield: '0', maxCombatShield: '0', ships: [] });
function shieldMaximum(player, index) {
  const base = [0n, 1000n, 100n][index], tier = BigInt(player.tiers?.[index] ?? 0);
  const scaled = base * 1000n * tier * (2000n + (tier > 0n ? tier - 1n : 0n)) / 2000n;
  const rounded = (scaled + 500n) / 1000n * 1000n;
  const bonus = player.commander?.bonus === 'shield' ? BigInt(player.commander.bonus_bps) : 0n;
  return (rounded * (10000n + bonus) / 10000n).toString();
}
export function fleetHealth(player) {
  if (!player) return null;
  const result = zero();
  result.ships = player.ships.map((ship, index) => ({ index, hp: ship.hp ?? '0', maxHp: ship.base_hp ?? '0', shield: ship.shield ?? '0', maxShield: shieldMaximum(player, index) }));
  for (const ship of result.ships) {
    for (const key of ['hp', 'maxHp', 'shield', 'maxShield']) result[key] = (BigInt(result[key]) + BigInt(ship[key])).toString();
  }
  result.combatShield = result.ships[1].shield; result.maxCombatShield = result.ships[1].maxShield;
  return result;
}
export function resolvedWeight(player) {
  return player?.alive ? BigInt(player.distance) + (player.temporary ?? []).reduce((sum, effect) => sum + BigInt(effect.distance), 0n) : 0n;
}
const compare = (a, b) => a === b ? 0 : a > b ? -1 : 1;
export function buildLeaderboards(state, players, clans) {
  const racers = players.map(row => ({ ...row, active: row.player?.race === state.race,
    distance: (row.player?.race === state.race ? resolvedWeight(row.player) : 0n).toString(),
    points: row.score.score, clanId: row.score.clan ?? null, health: row.player?.race === state.race ? fleetHealth(row.player) : null,
    alive: row.player?.race === state.race ? row.player.alive : null }));
  const teams = new Map();
  for (const clan of clans) if (!clan.disband_at || clan.disband_at > state.cycle) teams.set(`clan:${clan.id}`, { key: `clan:${clan.id}`, clanId: clan.id, name: clan.name, code: clan.code, solo: false, distance: '0', points: '0', members: [], activeMembers: 0, health: zero() });
  for (const racer of racers) {
    const key = racer.clanId === null ? `solo:${racer.wallet}` : `clan:${racer.clanId}`;
    if (!teams.has(key)) teams.set(key, { key, wallet: racer.clanId === null ? racer.wallet : null, clanId: racer.clanId, name: racer.clanId === null ? racer.nickname ?? racer.wallet : `Clan #${racer.clanId}`, solo: racer.clanId === null, distance: '0', points: '0', members: [], activeMembers: 0, health: zero() });
    const team = teams.get(key); team.members.push(racer.wallet); if (racer.active) team.activeMembers++;
    team.distance = (BigInt(team.distance) + BigInt(racer.distance)).toString(); team.points = (BigInt(team.points) + BigInt(racer.points)).toString();
    if (racer.health) for (const field of ['hp', 'maxHp', 'shield', 'maxShield', 'combatShield', 'maxCombatShield']) team.health[field] = (BigInt(team.health[field]) + BigInt(racer.health[field])).toString();
  }
  const sort = (rows, scope) => rows.slice().sort((a, b) => compare(BigInt(scope === 'race' ? a.distance : a.points), BigInt(scope === 'race' ? b.distance : b.points)) || compare(BigInt(a.distance), BigInt(b.distance)) || (a.wallet ?? a.key).localeCompare(b.wallet ?? b.key)).map((row, i) => ({ ...row, rank: i + 1 }));
  return { race: { racers: sort(racers.filter(r => r.active), 'race'), teams: sort([...teams.values()].filter(t => t.activeMembers > 0), 'race') },
    cycle: { racers: sort(racers, 'cycle'), teams: sort([...teams.values()], 'cycle') } };
}
