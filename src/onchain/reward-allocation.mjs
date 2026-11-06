// Mirrors race::settle and ClanPremiums. Integer floors follow contract order.
export function rewardAllocation(wallet, game, parameters, pool, players, clans) {
  const totalScore = BigInt(game.total_score), usd = BigInt(pool.spaceusd), srt = BigInt(pool.srt);
  const tierSum = players.reduce((sum, row) => sum + BigInt(row.score.tier_sum), 0n);
  const buckets = new Map(clans.map(clan => {
    const members = players.filter(row => row.score.clan === clan.id);
    const score = members.reduce((sum, row) => sum + BigInt(row.score.score), 0n);
    const tiers = members.reduce((sum, row) => sum + BigInt(row.score.tier_sum), 0n);
    const amount = totalScore ? usd * score / totalScore : 0n;
    return [clan.id, { clan, score, tiers, amount, premium: amount * BigInt(game.leader_bps) / 10000n }];
  }));
  const allocations = players.map(row => {
    const score = row.score, clan = buckets.get(score.clan);
    const gross = score.clan !== null ? clan?.tiers > 0n ? (clan.amount - clan.premium) * BigInt(score.tier_sum) / clan.tiers : 0n : totalScore ? usd * BigInt(score.score) / totalScore : 0n;
    const calculated = tierSum ? usd * BigInt(score.weighted_sum) / (tierSum * 100n) : 0n;
    const maintenance = calculated < gross ? calculated : gross;
    return { wallet: row.wallet, score, gross, maintenance, srt: totalScore ? srt * BigInt(score.score) / totalScore : 0n };
  });
  const own = allocations.find(row => row.wallet === wallet), clan = buckets.get(own?.score.clan);
  const leaderPremium = [...buckets.values()].filter(bucket => bucket.clan.leader === wallet).reduce((sum, bucket) => sum + bucket.premium, 0n);
  const referral = allocations.filter(row => row.score.sponsor === wallet).reduce((sum, row) => sum + row.maintenance * BigInt(parameters.maintenance_referral_bps) / 10000n, 0n);
  const gross = own?.gross ?? 0n, maintenance = own?.maintenance ?? 0n;
  return { spaceusd: (gross - maintenance + leaderPremium + referral).toString(), srt: (own?.srt ?? 0n).toString(),
    grossSpaceusd: gross.toString(), maintenance: maintenance.toString(), leaderPremium: leaderPremium.toString(), referral: referral.toString(),
    clanId: own?.score.clan ?? null, participatingLevels: own?.score.tier_sum ?? '0', clanParticipatingLevels: clan?.tiers.toString() ?? '0',
    basis: 'Current pool balances, earned points and race-snapshot level participation; not a final or claimable payout.' };
}
export function clanCycleKey(cycle, clan) {
  const namespace = Buffer.from('clan_cycles'), length = Buffer.alloc(2), part = Buffer.alloc(2), a = Buffer.alloc(8), b = Buffer.alloc(8);
  length.writeUInt16BE(namespace.length); part.writeUInt16BE(8); a.writeBigUInt64BE(BigInt(cycle)); b.writeBigUInt64BE(BigInt(clan));
  return Buffer.concat([length, namespace, part, a, b]);
}
