use cosmwasm_std::{StdError, StdResult, Uint128, Uint256};
use sha2::{Digest, Sha256};

/// Length-delimited transcript and rejection sampling, shared by NFT draws.
pub fn draw(seed: &[u8], scope: &[&[u8]], bound: Uint128) -> StdResult<Uint128> {
    if seed.len() != 32 || bound.is_zero() {
        return Err(StdError::generic_err("invalid random draw"));
    }
    let bound = Uint256::from(bound);
    let limit = Uint256::MAX - Uint256::MAX.checked_rem(bound)?;
    for attempt in 0u8..16 {
        let mut hash = Sha256::new();
        hash.update(seed);
        for part in scope {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        hash.update([attempt]);
        let bytes: [u8; 32] = hash.finalize().into();
        let value = Uint256::from_be_bytes(bytes);
        if value < limit {
            return Ok(Uint128::try_from(value.checked_rem(bound)?)?);
        }
    }
    Err(StdError::generic_err("random rejection limit"))
}
