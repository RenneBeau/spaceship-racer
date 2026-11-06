use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    entry_point, from_json, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo,
    Reply, Response, StdError, StdResult, SubMsg, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw_storage_plus::{Item, Map};
use game_common::cl8y;
#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    /// Operator strategy/capital destination ("Collapsar"/Overlord).
    pub overlord: String,
    pub router: String,
    pub spaceusd: String,
    /// Global reward contract funded with SRT and SpaceUSD prizes.
    pub reward_pool: Option<String>,
    pub payment_tokens: Vec<String>,
    pub max_spread: Decimal,
}
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub overlord: Addr,
    pub router: Addr,
    pub spaceusd: Addr,
    pub reward_pool: Option<Addr>,
    pub payment_tokens: Vec<Addr>,
    pub max_spread: Decimal,
}
#[cw_serde]
pub enum Revenue {
    Revenue {},
}
#[cw_serde]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    Convert {
        token: String,
        amount: Uint128,
        minimum_receive: Uint128,
        max_spread: Decimal,
        deadline: u64,
        route: Vec<String>,
    },
    SendCapital {
        amount: Uint128,
    },
    /// Fund the global reward reserve with SRT or SpaceUSD.
    FundRewardPool {
        token: String,
        amount: Uint128,
    },
    SetRewardPool {
        address: Option<String>,
    },
    /// Alias for SetOverlord: the Collapsar/strategy capital destination.
    SetCollapsar {
        address: String,
    },
    SetRoute {
        token: String,
        path: Vec<String>,
    },
    SetOverlord {
        address: String,
    },
    SetPauseController {
        controller: Option<String>,
    },
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},
    #[returns(Vec<String>)]
    Route { token: String },
    #[returns(Uint128)]
    Balance { token: String },
}
#[cw_serde]
enum RewardHook {
    Fund {},
}
#[cw_serde]
struct Pending {
    before: Uint128,
    minimum: Uint128,
}
const CONFIG: Item<Config> = Item::new("config");
const ROUTES: Map<&Addr, Vec<Addr>> = Map::new("routes");
const PENDING: Item<Pending> = Item::new("pending");
fn fail(msg: &str) -> StdError {
    StdError::generic_err(msg)
}
#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty()
        || msg.payment_tokens.is_empty()
        || msg.payment_tokens.len() > 5
        || msg.max_spread > Decimal::percent(50)
    {
        return Err(fail("invalid treasury initialization"));
    }
    let tokens = msg
        .payment_tokens
        .into_iter()
        .map(|a| deps.api.addr_validate(&a))
        .collect::<StdResult<Vec<_>>>()?;
    let usd = deps.api.addr_validate(&msg.spaceusd)?;
    if !tokens.contains(&usd)
        || tokens
            .iter()
            .enumerate()
            .any(|(i, a)| tokens[..i].contains(a))
    {
        return Err(fail("duplicate token or SpaceUSD missing"));
    }
    CONFIG.save(
        deps.storage,
        &Config {
            owner: deps.api.addr_validate(&msg.owner)?,
            overlord: deps.api.addr_validate(&msg.overlord)?,
            router: deps.api.addr_validate(&msg.router)?,
            spaceusd: usd.clone(),
            reward_pool: msg
                .reward_pool
                .as_deref()
                .map(|a| deps.api.addr_validate(a))
                .transpose()?,
            payment_tokens: tokens,
            max_spread: msg.max_spread,
        },
    )?;
    ROUTES.save(deps.storage, &usd, &vec![usd.clone()])?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/treasury",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_treasury"))
}
#[entry_point]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    if !info.funds.is_empty() || PENDING.may_load(deps.storage)?.is_some() {
        return Err(fail("native funds or swap already in progress"));
    }
    let mut config = CONFIG.load(deps.storage)?;
    match msg {
        ExecuteMsg::SetPauseController { controller } => {
            return game_common::pause::set_controller(deps, &info, &config.owner, controller)
        }
        ExecuteMsg::SetOverlord { address } | ExecuteMsg::SetCollapsar { address } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            config.overlord = deps.api.addr_validate(&address)?;
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new()
                .add_attribute("action", "set_capital_destination")
                .add_attribute("destination", config.overlord));
        }
        ExecuteMsg::SetRewardPool { address } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            config.reward_pool = address
                .as_deref()
                .map(|a| deps.api.addr_validate(a))
                .transpose()?;
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new()
                .add_attribute("action", "set_reward_pool")
                .add_attribute(
                    "reward_pool",
                    config
                        .reward_pool
                        .as_ref()
                        .map(|a| a.to_string())
                        .unwrap_or_default(),
                ));
        }
        ExecuteMsg::SetRoute { token, path } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            let token = deps.api.addr_validate(&token)?;
            if !config.payment_tokens.contains(&token) {
                return Err(fail("unsupported token"));
            }
            let path = cl8y::route(deps.as_ref(), &token, &config.spaceusd, path)?;
            ROUTES.save(deps.storage, &token, &path)?;
            return Ok(Response::new().add_attribute("action", "set_treasury_route"));
        }
        _ => {}
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    match msg {
        ExecuteMsg::Receive(receive) => {
            let _: Revenue = from_json(receive.msg)?;
            if !config.payment_tokens.contains(&info.sender) || receive.amount.is_zero() {
                return Err(fail("unsupported revenue"));
            }
            Ok(Response::new()
                .add_attribute("action", "receive_treasury_revenue")
                .add_attribute("amount", receive.amount))
        }
        ExecuteMsg::FundRewardPool { token, amount } => {
            if info.sender != config.owner || amount.is_zero() {
                return Err(fail("unauthorized or zero reward funding"));
            }
            let pool = config
                .reward_pool
                .as_ref()
                .ok_or_else(|| fail("reward pool not configured"))?;
            let token = deps.api.addr_validate(&token)?;
            if !config.payment_tokens.contains(&token) {
                return Err(fail("unsupported reward token"));
            }
            if cl8y::balance(deps.as_ref(), &token, &env.contract.address)? < amount {
                return Err(fail("insufficient treasury balance"));
            }
            let hook = to_json_binary(&RewardHook::Fund {})?;
            Ok(Response::new()
                .add_message(WasmMsg::Execute {
                    contract_addr: token.to_string(),
                    funds: vec![],
                    msg: to_json_binary(&Cw20ExecuteMsg::Send {
                        contract: pool.to_string(),
                        amount,
                        msg: hook,
                    })?,
                })
                .add_attribute("action", "fund_reward_pool")
                .add_attribute("reward_pool", pool)
                .add_attribute("token", token)
                .add_attribute("amount", amount))
        }
        ExecuteMsg::SendCapital { amount } => {
            if info.sender != config.owner || amount.is_zero() {
                return Err(fail("unauthorized or zero capital"));
            }
            Ok(Response::new()
                .add_message(WasmMsg::Execute {
                    contract_addr: config.spaceusd.to_string(),
                    funds: vec![],
                    msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                        recipient: config.overlord.to_string(),
                        amount,
                    })?,
                })
                .add_attribute("action", "send_overlord_capital")
                .add_attribute("amount_spaceusd", amount))
        }
        ExecuteMsg::Convert {
            token,
            amount,
            minimum_receive,
            max_spread,
            deadline,
            route,
        } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            let token = deps.api.addr_validate(&token)?;
            if token == config.spaceusd
                || !config.payment_tokens.contains(&token)
                || amount.is_zero()
                || minimum_receive.is_zero()
            {
                return Err(fail("invalid treasury conversion"));
            }
            cl8y::validate_trade(
                env.block.time.seconds(),
                deadline,
                max_spread,
                config.max_spread,
            )?;
            let path = ROUTES.load(deps.storage, &token)?;
            if route != path.iter().map(|a| a.to_string()).collect::<Vec<_>>() {
                return Err(fail("treasury route changed"));
            }
            if cl8y::balance(deps.as_ref(), &token, &env.contract.address)? < amount {
                return Err(fail("insufficient treasury balance"));
            }
            let before = cl8y::balance(deps.as_ref(), &config.spaceusd, &env.contract.address)?;
            PENDING.save(
                deps.storage,
                &Pending {
                    before,
                    minimum: minimum_receive,
                },
            )?;
            Ok(Response::new()
                .add_submessage(SubMsg::reply_on_success(
                    cl8y::swap(
                        &config.router,
                        &path,
                        amount,
                        minimum_receive,
                        max_spread,
                        deadline,
                        &env.contract.address,
                    )?,
                    1,
                ))
                .add_attribute("action", "convert_treasury_revenue"))
        }
        _ => unreachable!(),
    }
}
#[entry_point]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> StdResult<Response> {
    game_common::pause::ensure_running(deps.as_ref())?;
    if reply.id != 1 || reply.result.is_err() {
        return Err(fail("invalid conversion reply"));
    }
    let config = CONFIG.load(deps.storage)?;
    let pending = PENDING.load(deps.storage)?;
    let received = cl8y::balance(deps.as_ref(), &config.spaceusd, &env.contract.address)?
        .checked_sub(pending.before)?;
    if received < pending.minimum {
        return Err(fail("insufficient actual treasury output"));
    }
    PENDING.remove(deps.storage);
    Ok(Response::new()
        .add_attribute("action", "settle_treasury_conversion")
        .add_attribute("received_spaceusd", received))
}
#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Route { token } => to_json_binary(
            &ROUTES
                .load(deps.storage, &deps.api.addr_validate(&token)?)?
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>(),
        ),
        QueryMsg::Balance { token } => to_json_binary(&cl8y::balance(
            deps,
            &deps.api.addr_validate(&token)?,
            &env.contract.address,
        )?),
    }
}
