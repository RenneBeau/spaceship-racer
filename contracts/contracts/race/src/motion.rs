//! Exact time-aware aggregates and no-regeneration floor-sum trajectories.
//! The former per-tick regeneration scheduler is retained only for an already
//! running legacy race; deferred races reconcile at action/ending windows.
use crate::{
    math::{bonus, fail, mul, weight},
    state::*,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Int256, Order, StdResult, Storage, Uint128};
use cw_storage_plus::{Bound, Map};
use racer_collection::Bonus;

#[cw_serde]
#[derive(Default)]
pub struct Polynomial {
    pub constant: Int256,
    pub linear: Int256,
    pub quadratic: Int256,
}
#[cw_serde]
pub struct Node {
    pub race: u64,
    pub polynomial: Polynomial,
}
const NODES: Map<u16, Node> = Map::new("motion_nodes");
const LEAVES: Map<u16, Node> = Map::new("motion_leaves");
const BASE_NODES: Map<u16, Node> = Map::new("motion_base_nodes");
const BASE_LEAVES: Map<u16, Node> = Map::new("motion_base_leaves");
pub const WORK: Map<(u64, u32, u16), bool> = Map::new("motion_work");
pub const ACTIONS: Map<(u64, u32, u16), bool> = Map::new("motion_actions");
pub const DAMAGED: Map<(u64, u16), bool> = Map::new("motion_damaged");

pub fn healthy(p: &Player) -> bool {
    p.ships.iter().all(|s| s.hp == s.base_hp)
}
pub fn catch_up_legacy(p: &mut Player, tick: u32, params: &Parameters) -> StdResult<()> {
    while p.materialized_tick < tick {
        if healthy(p) && p.alive {
            let n = u128::from(tick - p.materialized_tick);
            let acceleration = mul(
                params.acceleration,
                SCALE * u128::from(p.acceleration_count) * u128::from(params.tick_seconds),
            )?
            .multiply_ratio(
                10000u128 + u128::from(bonus(p, Bonus::Acceleration)),
                10000u128,
            );
            p.distance = p
                .distance
                .checked_add(mul(p.safe_speed, u128::from(params.tick_seconds) * n)?)?
                .checked_add(mul(
                    acceleration,
                    u128::from(params.tick_seconds) * n * (n - 1) / 2,
                )?)?;
            p.safe_speed = p.safe_speed.checked_add(mul(acceleration, n)?)?;
            p.temporary.retain(|entry| entry.expires > tick);
            p.salvo = 0;
            p.utility_spent = Uint128::zero();
            p.materialized_tick = tick;
        } else {
            let next = p.materialized_tick + 1;
            crate::math::move_player_legacy(p, &Boosts::default(), params, next)?;
            p.materialized_tick = next;
        }
    }
    Ok(())
}

/// Sum floor((a*i+b)/m), i=0..n, using Euclidean reduction. This preserves
/// damage/acceleration rounding without replaying every skipped tick.
pub fn floor_sum(
    mut n: cosmwasm_std::Uint256,
    mut m: cosmwasm_std::Uint256,
    mut a: cosmwasm_std::Uint256,
    mut b: cosmwasm_std::Uint256,
) -> StdResult<cosmwasm_std::Uint256> {
    use cosmwasm_std::Uint256 as U;
    let mut sum = U::zero();
    while !n.is_zero() {
        if a >= m {
            sum = sum.checked_add(
                n.checked_mul(n.checked_sub(U::one())?)?
                    .checked_div(U::from(2u32))?
                    .checked_mul(a.checked_div(m)?)?,
            )?;
            a = a.checked_rem(m)?;
        }
        if b >= m {
            sum = sum.checked_add(n.checked_mul(b.checked_div(m)?)?)?;
            b = b.checked_rem(m)?;
        }
        let y = a.checked_mul(n)?.checked_add(b)?;
        if y < m {
            break;
        }
        n = y.checked_div(m)?;
        b = y.checked_rem(m)?;
        std::mem::swap(&mut m, &mut a);
    }
    Ok(sum)
}

pub fn catch_up(p: &mut Player, tick: u32, params: &Parameters) -> StdResult<()> {
    use cosmwasm_std::Uint256 as U;
    if tick <= p.materialized_tick {
        return Ok(());
    }
    let n = u128::from(tick - p.materialized_tick);
    if p.alive {
        let mut loss = Uint128::zero();
        for (i, w) in [(0, 5u128), (1, 1), (2, 3)] {
            let ship = &p.ships[i];
            if !ship.base_hp.is_zero() {
                loss = loss.checked_add(
                    ship.base_hp
                        .checked_sub(ship.hp)?
                        .multiply_ratio(w * 1_000_000, ship.base_hp),
                )?;
            }
        }
        let factor = 9_000_000u128 - loss.u128().min(9_000_000);
        let acceleration = mul(
            params.acceleration,
            SCALE * u128::from(p.acceleration_count) * u128::from(params.tick_seconds),
        )?
        .multiply_ratio(
            10000u128 + u128::from(bonus(p, Bonus::Acceleration)),
            10000u128,
        );
        let distance = floor_sum(
            U::from(n),
            U::from(9_000_000u128),
            U::from(acceleration).checked_mul(U::from(factor))?,
            U::from(p.safe_speed).checked_mul(U::from(factor))?,
        )?;
        p.distance = p.distance.checked_add(Uint128::try_from(
            distance.checked_mul(U::from(params.tick_seconds))?,
        )?)?;
        p.safe_speed = p.safe_speed.checked_add(mul(acceleration, n)?)?;
        p.temporary.retain(|entry| entry.expires > tick);
    }
    p.salvo = 0;
    p.utility_spent = Uint128::zero();
    p.materialized_tick = tick;
    Ok(())
}
fn polynomial(p: &Player, params: &Parameters) -> StdResult<Polynomial> {
    if !p.alive {
        return Ok(Polynomial::default());
    }
    let distance = Int256::from(weight(p)?);
    if !healthy(p) {
        return Ok(Polynomial {
            constant: distance,
            ..Polynomial::default()
        });
    }
    let t = Int256::from(p.materialized_tick);
    let speed = Int256::from(mul(p.safe_speed, u128::from(params.tick_seconds))?);
    let acceleration = mul(
        params.acceleration,
        SCALE * u128::from(p.acceleration_count) * u128::from(params.tick_seconds),
    )?
    .multiply_ratio(
        10000u128 + u128::from(bonus(p, Bonus::Acceleration)),
        10000u128,
    );
    let quadratic = Int256::from(mul(acceleration, u128::from(params.tick_seconds))?);
    let linear = speed.checked_sub(quadratic.checked_mul(t)?)?;
    let constant = distance.checked_sub(speed.checked_mul(t)?)?.checked_add(
        quadratic
            .checked_mul(t)?
            .checked_mul(t.checked_add(Int256::one())?)?
            .checked_div(Int256::from(2))
            .map_err(|e| fail(&e.to_string()))?,
    )?;
    Ok(Polynomial {
        constant,
        linear,
        quadratic,
    })
}
fn node(storage: &dyn Storage, race: u64, i: u16) -> StdResult<Polynomial> {
    Ok(NODES
        .may_load(storage, i)?
        .filter(|n| n.race == race)
        .map(|n| n.polynomial)
        .unwrap_or_default())
}
fn base(storage: &dyn Storage, generation: u64, i: u16) -> StdResult<Polynomial> {
    Ok(BASE_NODES
        .may_load(storage, i)?
        .filter(|n| n.race == generation)
        .map(|n| n.polynomial)
        .unwrap_or_default())
}
fn combine(a: Polynomial, b: Polynomial) -> StdResult<Polynomial> {
    Ok(Polynomial {
        constant: a.constant.checked_add(b.constant)?,
        linear: a.linear.checked_add(b.linear)?,
        quadratic: a.quadratic.checked_add(b.quadratic)?,
    })
}
fn difference(a: &Polynomial, b: &Polynomial) -> StdResult<Polynomial> {
    Ok(Polynomial {
        constant: a.constant.checked_sub(b.constant)?,
        linear: a.linear.checked_sub(b.linear)?,
        quadratic: a.quadratic.checked_sub(b.quadratic)?,
    })
}
fn effective_node(storage: &dyn Storage, state: &State, i: u16) -> StdResult<Polynomial> {
    let value = node(storage, state.race, i)?;
    if state.registry_enabled {
        combine(base(storage, state.registry_generation, i)?, value)
    } else {
        Ok(value)
    }
}
pub fn set_base(
    storage: &mut dyn Storage,
    generation: u64,
    index: u16,
    player: &Player,
    params: &Parameters,
) -> StdResult<()> {
    let old = BASE_LEAVES
        .may_load(storage, index)?
        .filter(|n| n.race == generation)
        .map(|n| n.polynomial)
        .unwrap_or_default();
    let new = polynomial(player, params)?;
    if old == new {
        return Ok(());
    }
    let delta = difference(&new, &old)?;
    let mut i = index;
    while i <= TREE_SIZE {
        let value = combine(base(storage, generation, i)?, delta.clone())?;
        BASE_NODES.save(
            storage,
            i,
            &Node {
                race: generation,
                polynomial: value,
            },
        )?;
        i += i & i.wrapping_neg();
    }
    BASE_LEAVES.save(
        storage,
        index,
        &Node {
            race: generation,
            polynomial: new,
        },
    )?;
    Ok(())
}
pub fn update(
    storage: &mut dyn Storage,
    race: u64,
    index: u16,
    p: &Player,
    params: &Parameters,
) -> StdResult<()> {
    update_inner(storage, race, index, p, params, true, None)
}

pub fn update_deferred(
    storage: &mut dyn Storage,
    race: u64,
    index: u16,
    p: &Player,
    params: &Parameters,
) -> StdResult<()> {
    if p.alive && (!healthy(p) || !p.temporary.is_empty()) {
        DAMAGED.save(storage, (race, index), &true)?;
    } else {
        DAMAGED.remove(storage, (race, index));
    }
    update_inner(storage, race, index, p, params, false, None)
}
pub fn update_persistent(
    storage: &mut dyn Storage,
    state: &State,
    index: u16,
    p: &Player,
    params: &Parameters,
) -> StdResult<()> {
    if p.alive && (!healthy(p) || !p.temporary.is_empty()) {
        DAMAGED.save(storage, (state.race, index), &true)?;
    } else {
        DAMAGED.remove(storage, (state.race, index));
    }
    update_inner(
        storage,
        state.race,
        index,
        p,
        params,
        false,
        Some(state.registry_generation),
    )
}

fn update_inner(
    storage: &mut dyn Storage,
    race: u64,
    index: u16,
    p: &Player,
    params: &Parameters,
    schedule: bool,
    generation: Option<u64>,
) -> StdResult<()> {
    let old = LEAVES
        .may_load(storage, index)?
        .filter(|n| n.race == race)
        .map(|n| n.polynomial)
        .unwrap_or_default();
    let mut new = polynomial(p, params)?;
    if let Some(generation) = generation {
        let baseline = BASE_LEAVES.load(storage, index)?;
        if baseline.race != generation {
            return Err(fail("registry base generation mismatch"));
        }
        new = difference(&new, &baseline.polynomial)?;
    }
    if old != new {
        let mut i = index;
        while i <= TREE_SIZE {
            let n = node(storage, race, i)?;
            let value = Polynomial {
                constant: n
                    .constant
                    .checked_sub(old.constant)?
                    .checked_add(new.constant)?,
                linear: n.linear.checked_sub(old.linear)?.checked_add(new.linear)?,
                quadratic: n
                    .quadratic
                    .checked_sub(old.quadratic)?
                    .checked_add(new.quadratic)?,
            };
            NODES.save(
                storage,
                i,
                &Node {
                    race,
                    polynomial: value,
                },
            )?;
            i += i & i.wrapping_neg();
        }
        LEAVES.save(
            storage,
            index,
            &Node {
                race,
                polynomial: new,
            },
        )?;
    }
    if schedule && p.alive && !healthy(p) {
        WORK.save(storage, (race, p.materialized_tick + 1, index), &true)?;
    }
    if schedule {
        for temporary in &p.temporary {
            if temporary.expires > p.materialized_tick {
                WORK.save(storage, (race, temporary.expires, index), &true)?;
            }
        }
    }
    Ok(())
}
fn evaluate(n: &Polynomial, tick: u32) -> StdResult<Int256> {
    let t = Int256::from(tick);
    Ok(n.constant
        .checked_add(n.linear.checked_mul(t)?)?
        .checked_add(
            n.quadratic
                .checked_mul(t)?
                .checked_mul(t.checked_sub(Int256::one())?)?
                .checked_div(Int256::from(2))
                .map_err(|e| fail(&e.to_string()))?,
        )?)
}
pub fn prefix(storage: &dyn Storage, state: &State, mut i: u16) -> StdResult<Uint128> {
    let mut sum = Int256::zero();
    while i > 0 {
        sum = sum.checked_add(evaluate(&effective_node(storage, state, i)?, state.tick)?)?;
        i -= i & i.wrapping_neg();
    }
    Uint128::try_from(sum).map_err(|_| fail("invalid motion weight"))
}
pub fn find(storage: &dyn Storage, state: &State, offset: Uint128) -> StdResult<u16> {
    let mut offset = Int256::from(offset);
    let mut index = 0;
    let mut bit = TREE_SIZE;
    while bit > 0 {
        let next = index + bit;
        if next <= TREE_SIZE {
            let value = evaluate(&effective_node(storage, state, next)?, state.tick)?;
            if value <= offset {
                offset = offset.checked_sub(value)?;
                index = next;
            }
        }
        bit >>= 1;
    }
    if index >= state.participants {
        return Err(fail("motion target outside snapshot"));
    }
    Ok(index + 1)
}
pub fn page(
    storage: &dyn Storage,
    map: &Map<(u64, u32, u16), bool>,
    state: &State,
    limit: u16,
) -> StdResult<Vec<u16>> {
    map.prefix((state.race, state.tick))
        .keys(
            storage,
            Some(Bound::exclusive(state.cursor)),
            None,
            Order::Ascending,
        )
        .take(usize::from(limit))
        .collect()
}
