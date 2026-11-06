use crate::{
    math::{bonus, fail, make_player},
    state::*,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Isqrt, StdResult, Storage, Uint128, Uint256};
use cw_storage_plus::Map;
use racer_collection::Bonus;

#[cw_serde]
pub struct Settings {
    pub enabled: bool,
    pub price_unit: Uint128,
    pub hp_per_unit: Uint128,
    pub fallen_ship_bps: u16,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            price_unit: Uint128::new(1_000_000),
            hp_per_unit: Uint128::new(1000),
            fallen_ship_bps: 2500,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> StdResult<()> {
        if self.price_unit.is_zero()
            || self.price_unit.u128() > 1_000_000_000_000_000
            || self.hp_per_unit.is_zero()
            || self.hp_per_unit.u128() > 1_000_000_000_000_000
            || self.fallen_ship_bps > 10000
        {
            return Err(fail("invalid healing settings"));
        }
        Ok(())
    }
}
#[cw_serde]
pub struct Quote {
    pub missing: Uint128,
    pub fallen_ships: u8,
    pub price: Uint128,
}
#[cw_serde]
pub struct Death {
    pub tick: u32,
    pub distance: Uint128,
    pub temporary: Vec<Temporary>,
    pub safe_speed: Uint128,
}
pub const RESERVED: Map<(u64, u32, &cosmwasm_std::Addr), Uint128> = Map::new("heal_reserves");
pub const DEATHS: Map<(u64, u16), Death> = Map::new("heal_deaths");

pub fn price(settings: &Settings, missing: Uint128, tank: u16, fallen: u8) -> StdResult<Uint128> {
    if missing.is_zero() {
        return Ok(Uint128::zero());
    }
    // SRT unit * sqrt(missing HP / (HP unit * (tank level + 1))).
    // No ceiling; marginal cost declines as repairs grow. All values are integers.
    let numerator = Uint256::from(missing)
        .checked_mul(Uint256::from(settings.price_unit))?
        .checked_mul(Uint256::from(settings.price_unit))?;
    let denominator = Uint256::from(settings.hp_per_unit)
        .checked_mul(Uint256::from(SCALE))?
        .checked_mul(Uint256::from(u128::from(tank) + 1))?;
    let squared = numerator
        .checked_add(denominator.checked_sub(Uint256::one())?)?
        .checked_div(denominator)?;
    let root = squared.isqrt();
    let root = if root.checked_mul(root)? < squared {
        root.checked_add(Uint256::one())?
    } else {
        root
    };
    let base = Uint128::try_from(root)?;
    let bps = 10000u128 + u128::from(settings.fallen_ship_bps) * u128::from(fallen);
    let total = Uint256::from(base)
        .checked_mul(Uint256::from(bps))?
        .checked_add(Uint256::from(9999u128))?
        .checked_div(Uint256::from(10000u128))?;
    Ok(Uint128::try_from(total)?)
}
pub fn quote(player: &Player, settings: &Settings) -> StdResult<Quote> {
    let restored = make_player(
        player.race,
        player.wallet.clone(),
        player.tiers,
        player.commander.clone(),
    )?;
    let mut missing = Uint128::zero();
    let mut fallen = 0;
    for (ship, full) in player.ships.iter().zip(restored.ships.iter()) {
        missing = missing
            .checked_add(full.base_hp.saturating_sub(ship.hp))?
            .checked_add(full.shield.saturating_sub(ship.shield))?;
        if !full.base_hp.is_zero() && ship.hp.is_zero() {
            fallen += 1;
        }
    }
    Ok(Quote {
        missing,
        fallen_ships: fallen,
        price: price(settings, missing, player.tiers[1], fallen)?,
    })
}
pub fn remember_death(
    storage: &mut dyn Storage,
    state: &State,
    index: u16,
    before: &Player,
) -> StdResult<()> {
    DEATHS.save(
        storage,
        (state.race, index),
        &Death {
            tick: state.tick,
            distance: before.distance,
            temporary: before.temporary.clone(),
            safe_speed: before.safe_speed,
        },
    )
}
pub fn apply(
    storage: &mut dyn Storage,
    config: &Config,
    state: &State,
    index: u16,
    player: &mut Player,
) -> StdResult<Uint128> {
    let Some(reserved) = RESERVED.may_load(storage, (state.race, state.tick, &player.wallet))?
    else {
        return Ok(Uint128::zero());
    };
    let quote = quote(player, &config.healing)?;
    if quote.price > reserved || quote.price.is_zero() {
        return Ok(Uint128::zero());
    }
    if !player.alive {
        let death = DEATHS
            .may_load(storage, (state.race, index))?
            .ok_or_else(|| fail("missing fleet destruction checkpoint"))?;
        player.distance = death.distance;
        player.temporary = death
            .temporary
            .into_iter()
            .filter(|effect| effect.expires > state.tick)
            .collect();
        let elapsed = state.tick.saturating_sub(death.tick).saturating_sub(1);
        let acceleration = config
            .parameters
            .acceleration
            .checked_mul(Uint128::new(SCALE))?
            .checked_mul(Uint128::from(player.acceleration_count))?
            .checked_mul(Uint128::from(config.parameters.tick_seconds))?
            .multiply_ratio(
                10000u128 + u128::from(bonus(player, Bonus::Acceleration)),
                10000u128,
            );
        player.safe_speed = death
            .safe_speed
            .checked_add(acceleration.checked_mul(Uint128::from(elapsed))?)?;
    }
    let restored = make_player(
        player.race,
        player.wallet.clone(),
        player.tiers,
        player.commander.clone(),
    )?;
    for (ship, full) in player.ships.iter_mut().zip(restored.ships.iter()) {
        ship.hp = full.base_hp;
        ship.shield = full.shield;
        ship.regen = Uint128::zero();
    }
    player.alive = true;
    Ok(quote.price)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn healing_is_uncapped_sublinear_tank_discounted_and_charges_for_fallen_ships() {
        let settings = Settings::default();
        let cost = |hp, tank, fallen| {
            price(&settings, Uint128::new(hp * SCALE), tank, fallen)
                .unwrap()
                .u128()
        };
        assert_eq!(cost(1000, 0, 0), 1_000_000);
        assert_eq!(cost(4000, 0, 0), 2_000_000);
        assert_eq!(cost(9000, 0, 0), 3_000_000);
        assert!(cost(1_000_000, 0, 0) > cost(9000, 0, 0));
        assert!(cost(9000, 8, 0) < cost(9000, 0, 0));
        assert_eq!(cost(4000, 0, 2), 3_000_000);
        assert_eq!(cost(0, 0, 0), 0);
    }
}
