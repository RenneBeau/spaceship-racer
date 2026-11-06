//! Proof-verified drand quicknet adapter. Consumers must close actions before
//! requesting a fixed future round. No caller-selected rounds or rerolls.
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult,
};
use cw_storage_plus::{Item, Map};
use drand_verify::{derive_randomness, G2PubkeyRfc, Pubkey};

pub const QUICKNET_PUBLIC_KEY: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";
pub const GENESIS: u64 = 1_692_803_367;
pub const PERIOD: u64 = 3;

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub consumers: Vec<String>,
    pub safety_seconds: u64,
}
#[cw_serde]
pub enum ExecuteMsg {
    SetPauseController {
        controller: Option<String>,
    },
    Request {
        id: u64,
    },
    Submit {
        consumer: String,
        id: u64,
        signature: Binary,
    },
    Configure {
        paused: Option<bool>,
        consumer: Option<String>,
        allowed: Option<bool>,
    },
}
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub paused: bool,
    pub safety_seconds: u64,
}
#[cw_serde]
pub struct Request {
    pub round: u64,
    pub available_at: u64,
    pub randomness: Option<Binary>,
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},
    #[returns(Request)]
    Request { consumer: String, id: u64 },
}
const CONFIG: Item<Config> = Item::new("config");
const CONSUMERS: Map<&Addr, bool> = Map::new("consumers");
const REQUESTS: Map<(&Addr, u64), Request> = Map::new("requests");
fn fail(msg: &str) -> StdError {
    StdError::generic_err(msg)
}

/// Shared by the stored-request adapter and stateless tick verifier.
pub fn future_request(now: u64, safety_seconds: u64) -> StdResult<Request> {
    let after = now
        .checked_add(safety_seconds)
        .ok_or_else(|| fail("time overflow"))?;
    let elapsed = after
        .checked_sub(GENESIS)
        .ok_or_else(|| fail("before quicknet genesis"))?;
    let round = elapsed
        .checked_div(PERIOD)
        .and_then(|r| r.checked_add(2))
        .ok_or_else(|| fail("round overflow"))?;
    let available_at = (round - 1)
        .checked_mul(PERIOD)
        .and_then(|s| GENESIS.checked_add(s))
        .ok_or_else(|| fail("time overflow"))?;
    Ok(Request {
        round,
        available_at,
        randomness: None,
    })
}

pub fn verify_quicknet(round: u64, signature: &[u8]) -> StdResult<Binary> {
    if round == 0 || signature.len() != 48 {
        return Err(fail("invalid signature length or round"));
    }
    let bytes = hex::decode(QUICKNET_PUBLIC_KEY).map_err(|_| fail("invalid configured key"))?;
    let key = G2PubkeyRfc::from_variable(&bytes).map_err(|_| fail("invalid configured key"))?;
    if !key
        .verify(round, b"", signature)
        .map_err(|_| fail("invalid beacon proof"))?
    {
        return Err(fail("invalid beacon proof"));
    }
    Ok(Binary::from(derive_randomness(signature).to_vec()))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty()
        || msg.consumers.len() > 20
        || !(6..=120).contains(&msg.safety_seconds)
    {
        return Err(fail("invalid initialization"));
    }
    CONFIG.save(
        deps.storage,
        &Config {
            owner: deps.api.addr_validate(&msg.owner)?,
            paused: false,
            safety_seconds: msg.safety_seconds,
        },
    )?;
    for consumer in msg.consumers {
        CONSUMERS.save(deps.storage, &deps.api.addr_validate(&consumer)?, &true)?;
    }
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/randomness",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_randomness"))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    let mut config = CONFIG.load(deps.storage)?;
    if let ExecuteMsg::SetPauseController { controller } = msg {
        return game_common::pause::set_controller(deps, &info, &config.owner, controller);
    }
    if let ExecuteMsg::Configure {
        paused,
        consumer,
        allowed,
    } = msg
    {
        if info.sender != config.owner {
            return Err(fail("unauthorized"));
        }
        if consumer.is_some() != allowed.is_some() {
            return Err(fail("consumer and allowed required together"));
        }
        if let (Some(consumer), Some(allowed)) = (consumer, allowed) {
            CONSUMERS.save(deps.storage, &deps.api.addr_validate(&consumer)?, &allowed)?;
        }
        if let Some(paused) = paused {
            config.paused = paused;
        }
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "configure_randomness"));
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    if config.paused {
        return Err(fail("paused"));
    }
    match msg {
        ExecuteMsg::Request { id } => {
            if !CONSUMERS
                .may_load(deps.storage, &info.sender)?
                .unwrap_or(false)
            {
                return Err(fail("unauthorized consumer"));
            }
            if REQUESTS.has(deps.storage, (&info.sender, id)) {
                return Err(fail("request already exists"));
            }
            let request = future_request(env.block.time.seconds(), config.safety_seconds)?;
            REQUESTS.save(deps.storage, (&info.sender, id), &request)?;
            Ok(Response::new()
                .add_attribute("action", "request_randomness")
                .add_attribute("consumer", info.sender)
                .add_attribute("id", id.to_string())
                .add_attribute("round", request.round.to_string()))
        }
        ExecuteMsg::Submit {
            consumer,
            id,
            signature,
        } => {
            let consumer = deps.api.addr_validate(&consumer)?;
            let mut request = REQUESTS.load(deps.storage, (&consumer, id))?;
            if request.randomness.is_some() {
                return Err(fail("already fulfilled"));
            }
            if env.block.time.seconds() < request.available_at {
                return Err(fail("beacon not due"));
            }
            request.randomness = Some(verify_quicknet(request.round, signature.as_slice())?);
            REQUESTS.save(deps.storage, (&consumer, id), &request)?;
            Ok(Response::new()
                .add_attribute("action", "fulfill_randomness")
                .add_attribute("consumer", consumer)
                .add_attribute("id", id.to_string())
                .add_attribute("round", request.round.to_string()))
        }
        ExecuteMsg::Configure { .. } => unreachable!(),
        ExecuteMsg::SetPauseController { .. } => unreachable!(),
    }
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Request { consumer, id } => {
            let consumer = deps.api.addr_validate(&consumer)?;
            to_json_binary(&REQUESTS.load(deps.storage, (&consumer, id))?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env, mock_info},
        Timestamp,
    };
    #[test]
    fn future_round_is_strictly_after_the_existing_safety_deadline() {
        for now in GENESIS..GENESIS + 100 {
            for safety in [6, 12, 120] {
                let r = future_request(now, safety).unwrap();
                assert!(r.available_at > now + safety);
                assert!(r.available_at <= now + safety + PERIOD);
                assert_eq!(r.available_at, GENESIS + (r.round - 1) * PERIOD);
            }
        }
    }

    #[test]
    fn real_quicknet_proof_is_bound_to_future_request_and_cannot_be_replayed() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        // Select round 123, whose public test vector is published by drand-verify.
        env.block.time = Timestamp::from_seconds(GENESIS + 357);
        instantiate(
            deps.as_mut(),
            env.clone(),
            mock_info("owner", &[]),
            InstantiateMsg {
                owner: "owner".into(),
                consumers: vec!["engine".into()],
                safety_seconds: 6,
            },
        )
        .unwrap();
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("engine", &[]),
            ExecuteMsg::Request { id: 1 },
        )
        .unwrap();
        let request = REQUESTS
            .load(deps.as_ref().storage, (&Addr::unchecked("engine"), 1))
            .unwrap();
        assert_eq!(request.round, 123);
        let submit = ExecuteMsg::Submit { consumer: "engine".into(), id: 1, signature: Binary::from(hex::decode(
            "b75c69d0b72a5d906e854e808ba7e2accb1542ac355ae486d591aa9d43765482e26cd02df835d3546d23c4b13e0dfc92").unwrap()) };
        assert!(execute(
            deps.as_mut(),
            env.clone(),
            mock_info("relayer", &[]),
            submit.clone()
        )
        .is_err());
        env.block.time = Timestamp::from_seconds(request.available_at);
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("relayer", &[]),
            submit.clone(),
        )
        .unwrap();
        assert!(execute(
            deps.as_mut(),
            env.clone(),
            mock_info("relayer", &[]),
            submit
        )
        .is_err());
        assert!(execute(
            deps.as_mut(),
            env.clone(),
            mock_info("engine", &[]),
            ExecuteMsg::Request { id: 1 }
        )
        .is_err());
        assert!(execute(
            deps.as_mut(),
            env,
            mock_info("attacker", &[]),
            ExecuteMsg::Request { id: 2 }
        )
        .is_err());
    }

    #[test]
    fn proof_for_another_round_is_rejected() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(GENESIS + 360);
        instantiate(
            deps.as_mut(),
            env.clone(),
            mock_info("owner", &[]),
            InstantiateMsg {
                owner: "owner".into(),
                consumers: vec!["engine".into()],
                safety_seconds: 6,
            },
        )
        .unwrap();
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("engine", &[]),
            ExecuteMsg::Request { id: 1 },
        )
        .unwrap();
        env.block.time = Timestamp::from_seconds(GENESIS + 369);
        assert!(execute(deps.as_mut(), env, mock_info("relayer", &[]), ExecuteMsg::Submit {
            consumer: "engine".into(), id: 1,
            signature: Binary::from(hex::decode("b75c69d0b72a5d906e854e808ba7e2accb1542ac355ae486d591aa9d43765482e26cd02df835d3546d23c4b13e0dfc92").unwrap()),
        }).is_err());
        assert!(REQUESTS
            .load(deps.as_ref().storage, (&Addr::unchecked("engine"), 1))
            .unwrap()
            .randomness
            .is_none());
    }
}
