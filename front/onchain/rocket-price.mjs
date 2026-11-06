// The deployed contract is authoritative; old deployments enforce the old units.
export async function rocketPrice(query, game, wallet) {
  try { return await query(game, { rocket_price: { wallet } }); }
  catch (error) {
    if (!String(error.message).includes('unknown variant `rocket_price`')) throw error;
    const [player, config] = await Promise.all([
      query(game, { player: { wallet } }), query(game, { config: {} }),
    ]);
    return (BigInt(player.ships[2].attack) * BigInt(config.parameters.rocket_cost_bps) / 10000000n).toString();
  }
}
