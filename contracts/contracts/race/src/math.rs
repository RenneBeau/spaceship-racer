use crate::state::*;
use cosmwasm_std::{Env, StdError, StdResult, Storage, Uint128, Uint256};
use racer_collection::Bonus;
use sha2::{Digest, Sha256};

pub fn fail(msg: &str) -> StdError {
    StdError::generic_err(msg)
}
pub fn validate(p: &Parameters) -> StdResult<()> {
    if p.tick_seconds == 0
        || p.tick_seconds > 60
        || p.races_per_cycle == 0
        || p.races_per_cycle > 1000
        || p.duration_cdf.is_empty()
        || p.duration_cdf.len() > 512
        || p.duration_cdf.last() != Some(&CDF_SCALE)
        || p.duration_cdf.windows(2).any(|w| w[0] > w[1])
        || p.duration_cdf.iter().any(|&n| n > CDF_SCALE)
        || p.boost_price.is_zero()
        || p.temporary_ticks == 0
        || p.temporary_ticks > 100
        || p.boost_fee_bps > 10000
        || p.maintenance_referral_bps > 10000
        || p.rocket_cost_bps == 0
        || p.max_players == 0
        || p.max_players > 1000
        || [
            p.boost_price,
            p.speed_boost,
            p.acceleration,
            p.temporary_distance,
        ]
        .iter()
        .any(|v| v.u128() > 1_000_000_000_000_000)
    {
        return Err(fail("invalid race parameters"));
    }
    Ok(())
}

pub fn mul(value: Uint128, factor: u128) -> StdResult<Uint128> {
    Ok(value.checked_mul(Uint128::new(factor))?)
}
pub fn bonus(player: &Player, kind: Bonus) -> u16 {
    player
        .commander
        .as_ref()
        .filter(|a| a.bonus == kind)
        .map(|a| a.bonus_bps)
        .unwrap_or(0)
}
fn scaled_stat(base_milli: u128, tier: u16) -> StdResult<Uint128> {
    let t = u128::from(tier);
    Ok(Uint128::new(base_milli)
        .checked_mul(Uint128::new(t * (2000 + t.saturating_sub(1))))?
        .checked_div(Uint128::new(2000))?)
}

pub fn make_player(
    race: u64,
    wallet: cosmwasm_std::Addr,
    tiers: [u16; 3],
    commander: Option<racer_collection::Attributes>,
) -> StdResult<Player> {
    let mut ships = [Ship::default(), Ship::default(), Ship::default()];
    for (i, (hp, speed, regen, shield, attack)) in [
        (55, 100, 500, 0, 0),
        (5500, 10, 50000, 1000, 0),
        (550, 20, 5000, 100, 500),
    ]
    .into_iter()
    .enumerate()
    {
        let tier = tiers[i];
        // Match legacy rounding for integer-valued stats, retain milli-units for regeneration.
        let integer_stat = |base| -> StdResult<Uint128> {
            let value = scaled_stat(base * SCALE, tier)?;
            mul(
                value
                    .checked_add(Uint128::new(SCALE / 2))?
                    .checked_div(Uint128::new(SCALE))?,
                SCALE,
            )
        };
        ships[i] = Ship {
            base_hp: integer_stat(hp)?,
            hp: integer_stat(hp)?,
            speed: integer_stat(speed)?,
            regen: scaled_stat(regen, tier)?,
            shield: integer_stat(shield)?,
            attack: integer_stat(attack)?,
        };
    }
    let mut player = Player {
        materialized_tick: 0,
        race,
        wallet,
        tiers,
        ships,
        commander,
        safe_speed: Uint128::zero(),
        distance: Uint128::zero(),
        temporary: vec![],
        acceleration_count: 0,
        alive: true,
        salvo: 0,
        utility_spent: Uint128::zero(),
    };
    for i in 0..3 {
        player.ships[i].speed = player.ships[i].speed.multiply_ratio(
            10000u128 + u128::from(bonus(&player, Bonus::Speed)),
            10000u128,
        );
        player.ships[i].shield = player.ships[i].shield.multiply_ratio(
            10000u128 + u128::from(bonus(&player, Bonus::Shield)),
            10000u128,
        );
        player.ships[i].regen = player.ships[i].regen.multiply_ratio(
            10000u128 + u128::from(bonus(&player, Bonus::Repair)),
            10000u128,
        );
        player.safe_speed = player.safe_speed.checked_add(player.ships[i].speed)?;
    }
    Ok(player)
}

pub fn weight(p: &Player) -> StdResult<Uint128> {
    if !p.alive {
        return Ok(Uint128::zero());
    }
    p.temporary
        .iter()
        .try_fold(p.distance, |total, t| Ok(total.checked_add(t.distance)?))
}
pub fn rocket_price(p: &Player, params: &Parameters) -> Uint128 {
    // Economy rate: 0.0004 SRT per attack unit at 4000 bps.
    // Level 100 has 52,475 attack: 20.99 SRT per rocket.
    p.ships[2]
        .attack
        .multiply_ratio(u128::from(params.rocket_cost_bps), 10000u128)
}

pub fn move_player(
    p: &mut Player,
    boosts: &Boosts,
    params: &Parameters,
    tick: u32,
) -> StdResult<()> {
    p.utility_spent = Uint128::zero();
    p.salvo = 0;
    if !p.alive {
        return Ok(());
    }
    p.temporary.retain(|t| t.expires > tick);
    p.safe_speed = p
        .safe_speed
        .checked_add(mul(params.speed_boost, SCALE * u128::from(boosts.speed))?)?;
    p.acceleration_count = p
        .acceleration_count
        .checked_add(u64::from(boosts.acceleration))
        .ok_or_else(|| fail("boost overflow"))?;
    if boosts.temporary > 0 {
        p.temporary.push(Temporary {
            expires: tick + u32::from(params.temporary_ticks),
            distance: mul(
                params.temporary_distance,
                SCALE * u128::from(boosts.temporary),
            )?,
        });
    }
    p.salvo = if p.ships[2].hp.is_zero() {
        0
    } else {
        boosts.rockets
    };
    p.utility_spent = mul(
        params.boost_price,
        u128::from(boosts.speed) + u128::from(boosts.acceleration) + u128::from(boosts.temporary),
    )?;
    let speed = damaged_speed(p)?;
    p.distance = p
        .distance
        .checked_add(mul(speed, u128::from(params.tick_seconds))?)?;
    let acceleration = mul(
        params.acceleration,
        SCALE * u128::from(p.acceleration_count) * u128::from(params.tick_seconds),
    )?
    .multiply_ratio(
        10000u128 + u128::from(bonus(p, Bonus::Acceleration)),
        10000u128,
    );
    p.safe_speed = p.safe_speed.checked_add(acceleration)?;
    Ok(())
}

/// Compatibility for a race started under the former regeneration rules.
pub fn move_player_legacy(
    p: &mut Player,
    boosts: &Boosts,
    params: &Parameters,
    tick: u32,
) -> StdResult<()> {
    move_player(p, boosts, params, tick)?;
    if !p.alive {
        return Ok(());
    }
    for ship in &mut p.ships {
        if !ship.hp.is_zero() {
            ship.hp = ship
                .hp
                .checked_add(mul(ship.regen, u128::from(params.tick_seconds))?)?
                .min(ship.base_hp);
        }
    }
    Ok(())
}

fn damaged_speed(p: &Player) -> StdResult<Uint128> {
    let mut loss = Uint128::zero();
    for (i, weight) in [(0, 5u128), (1, 1), (2, 3)] {
        let ship = &p.ships[i];
        if !ship.base_hp.is_zero() {
            loss = loss.checked_add(
                ship.base_hp
                    .checked_sub(ship.hp)?
                    .multiply_ratio(weight * 1_000_000, ship.base_hp),
            )?;
        }
    }
    Ok(p.safe_speed
        .multiply_ratio(9_000_000u128 - loss.u128().min(9_000_000), 9_000_000u128))
}
pub fn damage(p: &mut Player, amount: Uint128) -> StdResult<()> {
    for i in [1, 2, 0] {
        let ship = &mut p.ships[i];
        if ship.hp.is_zero() {
            continue;
        }
        // Legacy tank shield absorbs rockets; fighter shields are cosmetic in the old engine.
        let remaining = if i == 1 {
            let absorbed = ship.shield.min(amount);
            ship.shield = ship.shield.checked_sub(absorbed)?;
            amount.checked_sub(absorbed)?
        } else {
            amount
        };
        ship.hp = ship.hp.saturating_sub(remaining);
        break;
    }
    if p.ships.iter().all(|s| s.hp.is_zero()) {
        p.alive = false;
        p.distance = Uint128::zero();
        p.temporary.clear();
    }
    Ok(())
}

/// Rejection sampling over the full hash prevents modulo bias. Hash transcript
/// separates contracts, chains, races, ticks, purposes, actors and rocket indices.
pub fn draw(
    env: &Env,
    state: &State,
    domain: &[u8],
    actor: u16,
    ordinal: u32,
    bound: Uint128,
) -> StdResult<Uint128> {
    if bound.is_zero() {
        return Err(fail("empty random range"));
    }
    let b = Uint256::from(bound);
    let limit = Uint256::MAX - Uint256::MAX.checked_rem(b)?;
    for attempt in 0u8..16 {
        let mut h = Sha256::new();
        for part in [
            env.block.chain_id.as_bytes(),
            env.contract.address.as_bytes(),
            domain,
            state.seed.as_slice(),
        ] {
            h.update((part.len() as u64).to_be_bytes());
            h.update(part);
        }
        h.update(state.race.to_be_bytes());
        h.update(state.tick.to_be_bytes());
        h.update(actor.to_be_bytes());
        if ordinal <= u32::from(u16::MAX) {
            h.update((ordinal as u16).to_be_bytes());
        } else {
            h.update(b"large-ordinal");
            h.update(ordinal.to_be_bytes());
        }
        h.update([attempt]);
        let bytes: [u8; 32] = h.finalize().into();
        let value = Uint256::from_be_bytes(bytes);
        if value < limit {
            return Ok(Uint128::try_from(value.checked_rem(b)?)?);
        }
    }
    Err(fail("random rejection limit"))
}

pub fn multiplier(roll: u128) -> u16 {
    for (ceiling, value) in [
        (318, 1),
        (508, 2),
        (628, 3),
        (713, 4),
        (783, 5),
        (838, 6),
        (883, 7),
        (923, 8),
        (978, 25),
        (990, 50),
        (996, 75),
        (1000, 100),
    ] {
        if roll < ceiling {
            return value;
        }
    }
    100
}

fn node(storage: &dyn Storage, state: &State, index: u16) -> StdResult<Uint128> {
    Ok(TREE
        .may_load(storage, index)?
        .filter(|n| n.race == state.race && n.tick == state.tick)
        .map(|n| n.weight)
        .unwrap_or_default())
}
pub fn tree_change(
    storage: &mut dyn Storage,
    state: &State,
    index: u16,
    old: Uint128,
    new: Uint128,
) -> StdResult<()> {
    let mut i = index;
    while i <= TREE_SIZE {
        let weight = node(storage, state, i)?
            .checked_sub(old)?
            .checked_add(new)?;
        TREE.save(
            storage,
            i,
            &TreeNode {
                race: state.race,
                tick: state.tick,
                weight,
            },
        )?;
        i += i & i.wrapping_neg();
    }
    Ok(())
}
pub fn prefix(storage: &dyn Storage, state: &State, mut index: u16) -> StdResult<Uint128> {
    let mut sum = Uint128::zero();
    while index > 0 {
        sum = sum.checked_add(node(storage, state, index)?)?;
        index -= index & index.wrapping_neg();
    }
    Ok(sum)
}
pub fn tree_find(storage: &dyn Storage, state: &State, mut offset: Uint128) -> StdResult<u16> {
    let mut index = 0u16;
    let mut step = TREE_SIZE;
    while step > 0 {
        let next = index + step;
        if next <= TREE_SIZE {
            let weight = node(storage, state, next)?;
            if weight <= offset {
                index = next;
                offset = offset.checked_sub(weight)?;
            }
        }
        step /= 2;
    }
    if index >= state.participants {
        return Err(fail("invalid weighted selection"));
    }
    Ok(index + 1)
}
