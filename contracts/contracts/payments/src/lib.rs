use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdError, StdResult, SubMsg, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw_storage_plus::{Item, Map};
use game_common::cl8y;
use racer_fleet::Purchase;

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub fleet: String,
    pub spaceusd: String,
    pub router: String,
    pub payment_tokens: Vec<String>,
    pub max_spread: Decimal,
}
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub fleet: Addr,
    pub spaceusd: Addr,
    pub router: Addr,
    pub payment_tokens: Vec<Addr>,
    pub max_spread: Decimal,
}
#[cw_serde]
pub struct Payment {
    pub purchase: Purchase,
    pub expected_price: Uint128,
    pub minimum_receive: Uint128,
    pub max_spread: Decimal,
    pub deadline: u64,
    pub route: Vec<String>,
    #[serde(default)]
    pub maximum_price: Option<Uint128>,
}
#[cw_serde]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    SetRoute { token: String, path: Vec<String> },
    SetPauseController { controller: Option<String> },
}
#[cw_serde]
pub struct Quote {
    pub price: Uint128,
    pub route: Vec<String>,
    pub estimated_offer: Uint128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_price: Option<Uint128>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<u64>,
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},
    #[returns(Vec<String>)]
    Route { token: String },
    #[returns(Quote)]
    Quote {
        wallet: String,
        token: String,
        purchase: Purchase,
    },
}
#[cw_serde]
struct Pending {
    wallet: Addr,
    purchase: Purchase,
    price: Uint128,
    minimum: Uint128,
    before: Uint128,
}
const CONFIG: Item<Config> = Item::new("config");
const ROUTES: Map<&Addr, Vec<Addr>> = Map::new("routes");
const PENDING: Item<Pending> = Item::new("pending");
const SWAP_REPLY: u64 = 1;
fn fail(msg: &str) -> StdError {
    StdError::generic_err(msg)
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
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
        return Err(fail("invalid payment configuration"));
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
            fleet: deps.api.addr_validate(&msg.fleet)?,
            spaceusd: usd.clone(),
            router: deps.api.addr_validate(&msg.router)?,
            payment_tokens: tokens,
            max_spread: msg.max_spread,
        },
    )?;
    ROUTES.save(deps.storage, &usd, &vec![usd.clone()])?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/payments",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_payments"))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    if !info.funds.is_empty() || PENDING.may_load(deps.storage)?.is_some() {
        return Err(fail("native funds or swap already in progress"));
    }
    let config = CONFIG.load(deps.storage)?;
    match msg {
        ExecuteMsg::SetPauseController { controller } => {
            return game_common::pause::set_controller(deps, &info, &config.owner, controller)
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
            return Ok(Response::new().add_attribute("action", "set_payment_route"));
        }
        _ => {}
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    let ExecuteMsg::Receive(receive) = msg else {
        unreachable!()
    };
    if receive.amount.is_zero() || !config.payment_tokens.contains(&info.sender) {
        return Err(fail("unsupported payment token"));
    }
    let wallet = deps.api.addr_validate(&receive.sender)?;
    let payment: Payment = from_json(receive.msg)?;
    cl8y::validate_trade(
        env.block.time.seconds(),
        payment.deadline,
        payment.max_spread,
        config.max_spread,
    )?;
    let path = ROUTES.load(deps.storage, &info.sender)?;
    if path.iter().map(|a| a.to_string()).collect::<Vec<_>>() != payment.route {
        return Err(fail("payment route changed"));
    }
    let quote: racer_fleet::Quote = deps.querier.query_wasm_smart(
        &config.fleet,
        &racer_fleet::QueryMsg::Quote {
            wallet: wallet.to_string(),
            purchase: payment.purchase.clone(),
        },
    )?;
    let accepted = match payment.maximum_price {
        Some(maximum) => maximum >= payment.expected_price && quote.price <= maximum,
        None => quote.price == payment.expected_price,
    };
    if !accepted || payment.minimum_receive < quote.price {
        return Err(fail("invoice changed or insufficient minimum"));
    }
    if info.sender == config.spaceusd {
        if receive.amount < payment.minimum_receive {
            return Err(fail("insufficient direct payment"));
        }
        return settle(
            &config,
            wallet,
            payment.purchase,
            quote.price,
            receive.amount,
        );
    }
    let before = cl8y::balance(deps.as_ref(), &config.spaceusd, &env.contract.address)?;
    PENDING.save(
        deps.storage,
        &Pending {
            wallet,
            purchase: payment.purchase,
            price: quote.price,
            minimum: payment.minimum_receive,
            before,
        },
    )?;
    Ok(Response::new()
        .add_submessage(SubMsg::reply_on_success(
            cl8y::swap(
                &config.router,
                &path,
                receive.amount,
                payment.minimum_receive,
                payment.max_spread,
                payment.deadline,
                &env.contract.address,
            )?,
            SWAP_REPLY,
        ))
        .add_attribute("action", "swap_ship_payment"))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> StdResult<Response> {
    game_common::pause::ensure_running(deps.as_ref())?;
    if reply.id != SWAP_REPLY || reply.result.is_err() {
        return Err(fail("invalid swap reply"));
    }
    let config = CONFIG.load(deps.storage)?;
    let pending = PENDING.load(deps.storage)?;
    let received = cl8y::balance(deps.as_ref(), &config.spaceusd, &env.contract.address)?
        .checked_sub(pending.before)?;
    if received < pending.minimum || received < pending.price {
        return Err(fail("insufficient actual swap output"));
    }
    PENDING.remove(deps.storage);
    settle(
        &config,
        pending.wallet,
        pending.purchase,
        pending.price,
        received,
    )
}
fn settle(
    config: &Config,
    wallet: Addr,
    purchase: Purchase,
    price: Uint128,
    received: Uint128,
) -> StdResult<Response> {
    let mut response = Response::new().add_message(WasmMsg::Execute {
        contract_addr: config.spaceusd.to_string(),
        funds: vec![],
        msg: to_json_binary(&Cw20ExecuteMsg::Send {
            contract: config.fleet.to_string(),
            amount: price,
            msg: to_json_binary(&racer_fleet::ReceiveMsg::PayFor {
                wallet: wallet.to_string(),
                purchase,
            })?,
        })?,
    });
    let surplus = received.checked_sub(price)?;
    if !surplus.is_zero() {
        response = response.add_message(WasmMsg::Execute {
            contract_addr: config.spaceusd.to_string(),
            funds: vec![],
            msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                recipient: wallet.to_string(),
                amount: surplus,
            })?,
        });
    }
    Ok(response
        .add_attribute("action", "settle_ship_payment")
        .add_attribute("wallet", wallet)
        .add_attribute("price_spaceusd", price)
        .add_attribute("surplus_spaceusd", surplus))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    match msg {
        QueryMsg::Config {} => to_json_binary(&config),
        QueryMsg::Route { token } => to_json_binary(
            &ROUTES
                .load(deps.storage, &deps.api.addr_validate(&token)?)?
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>(),
        ),
        QueryMsg::Quote {
            wallet,
            token,
            purchase,
        } => {
            let wallet = deps.api.addr_validate(&wallet)?;
            let token = deps.api.addr_validate(&token)?;
            let quote: racer_fleet::Quote = deps.querier.query_wasm_smart(
                &config.fleet,
                &racer_fleet::QueryMsg::Quote {
                    wallet: wallet.to_string(),
                    purchase,
                },
            )?;
            let path = ROUTES.load(deps.storage, &token)?;
            let needed = quote.maximum_price.unwrap_or(quote.price);
            let estimated_offer = if token == config.spaceusd {
                needed
            } else {
                let sim: cl8y::Simulation = deps.querier.query_wasm_smart(
                    &config.router,
                    &cl8y::QueryMsg::ReverseSimulateSwapOperations {
                        ask_amount: needed,
                        operations: cl8y::operations(&path),
                        trader: Some(env.contract.address.to_string()),
                        sender: Some(env.contract.address.to_string()),
                    },
                )?;
                sim.amount
            };
            to_json_binary(&Quote {
                price: quote.price,
                route: path.iter().map(|a| a.to_string()).collect(),
                estimated_offer,
                maximum_price: quote.maximum_price,
                valid_until: quote.valid_until,
            })
        }
    }
}
