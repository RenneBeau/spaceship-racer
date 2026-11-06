//! Stateless, fixed-key verification. The caller binds deadlines and round IDs.
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Empty, Env, MessageInfo, Response, StdError, StdResult,
};
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Protocol)]
    Protocol {},
    #[returns(Binary)]
    Verify { round: u64, signature: Binary },
}
#[cw_serde]
pub struct Protocol {
    pub public_key: String,
    pub genesis: u64,
    pub period: u64,
}
pub fn protocol() -> Protocol {
    Protocol {
        public_key: racer_randomness::QUICKNET_PUBLIC_KEY.into(),
        genesis: racer_randomness::GENESIS,
        period: racer_randomness::PERIOD,
    }
}
#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    _msg: Empty,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(StdError::generic_err("native funds unsupported"));
    }
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/beacon-verifier",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_stateless_verifier"))
}
#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(_deps: DepsMut, _env: Env, _info: MessageInfo, _msg: Empty) -> StdResult<Response> {
    Err(StdError::generic_err(
        "stateless verifier has no execute operations",
    ))
}
#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(_deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Protocol {} => to_json_binary(&protocol()),
        QueryMsg::Verify { round, signature } => to_json_binary(
            &racer_randomness::verify_quicknet(round, signature.as_slice())?,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
    };
    fn bytes(s: &str) -> Binary {
        Binary::from(
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>(),
        )
    }
    #[test]
    fn published_proof_is_verified_and_wrong_round_or_signature_is_rejected() {
        let deps = mock_dependencies();
        let signature = bytes("b75c69d0b72a5d906e854e808ba7e2accb1542ac355ae486d591aa9d43765482e26cd02df835d3546d23c4b13e0dfc92");
        let seed: Binary = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::Verify {
                    round: 123,
                    signature: signature.clone(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            seed,
            bytes("fb8f7bc29bf24db51871ec8c79f3a1e4bd0557bc0dfcee9ed1d924e69d1c60dc")
        );
        assert!(query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Verify {
                round: 124,
                signature
            }
        )
        .is_err());
        assert!(query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Verify {
                round: 123,
                signature: Binary::from(vec![0; 48])
            }
        )
        .is_err());
        assert!(query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Verify {
                round: 123,
                signature: Binary::from(vec![0; 32])
            }
        )
        .is_err());
    }
}
