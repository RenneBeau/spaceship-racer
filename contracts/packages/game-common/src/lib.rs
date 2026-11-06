use cosmwasm_std::{StdError, StdResult, Uint128, Uint256};
pub mod cl8y;
pub mod pause;
pub mod random;

pub const BPS: u128 = 10_000;
pub const YEAR_SECONDS: u64 = 31_557_600;

/// Tier units scaled by ten: marginal brackets, never a whole-fleet surcharge.
pub fn weighted_tiers(tiers: u32) -> StdResult<Uint128> {
    if tiers > 3_000 {
        return Err(StdError::generic_err("fleet exceeds 3000 tiers"));
    }
    let t = u128::from(tiers);
    Ok(Uint128::new(
        t.min(25) * 10
            + t.saturating_sub(25).min(175) * 11
            + t.saturating_sub(200).min(1800) * 12
            + t.saturating_sub(2000) * 13,
    ))
}

/// Equal race counts cancel from numerator and denominator. Inputs are sums
/// of race snapshots, not end-of-cycle fleet tiers. Round only once, downward.
pub fn maintenance(
    gross_cycle: Uint128,
    player_weighted_tier_sum: Uint128,
    all_raw_tier_sum: Uint128,
    player_reward: Uint128,
) -> StdResult<Uint128> {
    if all_raw_tier_sum.is_zero() {
        return Ok(Uint128::zero());
    }
    let amount = Uint256::from(gross_cycle)
        .checked_mul(Uint256::from(player_weighted_tier_sum))?
        .checked_div(Uint256::from(all_raw_tier_sum).checked_mul(Uint256::from(100u128))?)?;
    Ok(Uint128::try_from(amount.min(Uint256::from(player_reward)))?)
}

/// Linear accrual against a checkpointed unallocated reserve; no compounding
/// assumptions, no allocation beyond actual reserve, and no floating point.
pub fn accrued(reserve: Uint128, elapsed: u64, annual_bps: u16) -> StdResult<Uint128> {
    if u128::from(annual_bps) > BPS {
        return Err(StdError::generic_err("annual rate exceeds 100%"));
    }
    let amount = Uint256::from(reserve)
        .checked_mul(Uint256::from(u128::from(elapsed)))?
        .checked_mul(Uint256::from(u128::from(annual_bps)))?
        .checked_div(Uint256::from(BPS * u128::from(YEAR_SECONDS)))?;
    Ok(Uint128::try_from(amount.min(Uint256::from(reserve)))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marginal_bracket_boundaries() {
        for (tiers, expected) in [
            (0, 0),
            (25, 250),
            (26, 261),
            (200, 2175),
            (201, 2187),
            (2000, 23775),
            (2001, 23788),
            (3000, 36775),
        ] {
            assert_eq!(weighted_tiers(tiers).unwrap(), Uint128::new(expected));
        }
        assert!(weighted_tiers(3001).is_err());
    }

    #[test]
    fn late_upgrade_charges_only_participating_races_and_caps_at_reward() {
        // 99 races at tier 1, final race at tier 100.
        let weighted = weighted_tiers(1).unwrap() * Uint128::new(99) + weighted_tiers(100).unwrap();
        assert_eq!(
            maintenance(
                Uint128::new(10000),
                weighted,
                Uint128::new(199),
                Uint128::new(10000)
            )
            .unwrap(),
            Uint128::new(1037)
        );
        assert_eq!(
            maintenance(Uint128::MAX, weighted, Uint128::new(199), Uint128::new(7)).unwrap(),
            Uint128::new(7)
        );
        assert_eq!(
            maintenance(Uint128::MAX, weighted, Uint128::zero(), Uint128::MAX).unwrap(),
            Uint128::zero()
        );
    }

    #[test]
    fn annual_accrual_is_backed_and_overflow_safe() {
        assert_eq!(
            accrued(Uint128::new(10000), YEAR_SECONDS, 2500).unwrap(),
            Uint128::new(2500)
        );
        assert_eq!(accrued(Uint128::MAX, u64::MAX, 2500).unwrap(), Uint128::MAX);
        assert!(accrued(Uint128::new(1), 1, 10001).is_err());
    }
}
