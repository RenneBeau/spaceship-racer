//! Payment adapter and reward gateway. Fractional progress is backed SpaceUSD,
//! never a permission for keepers to spend an unrelated wallet balance.
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Deps, DepsMut, Empty, Env, MessageInfo, Order, Reply,
    ReplyOn, Response, StdError, StdResult, SubMsg, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw_storage_plus::{Bound, Item, Map};
use racer_fleet::{Fleet, Purchase, ShipType};
use racer_rewards::{Allocation, Credit};

#[cw_serde]
pub struct InstantiateMsg {
    pub payments: racer_payments::InstantiateMsg,
    pub backing_rewards: String,
    pub allocator: String,
}
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub backing_rewards: Addr,
    pub allocator: Addr,
}
#[cw_serde]
#[derive(Default)]
pub struct Preference {
    pub ship: Option<ShipType>,
    pub bps: u16,
}
#[cw_serde]
#[derive(Default)]
pub struct Progress {
    pub spaceusd: [Uint128; 3],
}
#[cw_serde]
pub enum Hook {
    Fund {},
    Invest { ship: ShipType },
}
#[cw_serde]
pub enum ExecuteMsg {
    ConfigureAllocator { allocator: String },
    Receive(Cw20ReceiveMsg),
    SetRoute { token: String, path: Vec<String> },
    SetPauseController { controller: Option<String> },
    Checkpoint {},
    Allocate { recipients: Vec<Allocation> },
    PayMaintenance { amount: Uint128, treasury: String },
    Claim {},
    SetAutoReinvest { ship: Option<ShipType>, bps: u16 },
    ProcessAuto { wallet: String },
    WithdrawProgress { ship: ShipType },
}
#[cw_serde]
pub struct Reinvestment {
    pub preference: Preference,
    pub ready: Uint128,
    pub progress: Progress,
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(racer_payments::Config)]
    Config {},
    #[returns(Config)]
    ReinvestmentConfig {},
    #[returns(racer_rewards::State)]
    State {},
    #[returns(Credit)]
    Credit { wallet: String },
    #[returns(Reinvestment)]
    Reinvestment { wallet: String },
    #[returns(Vec<String>)]
    AutoReady {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    #[returns(Vec<String>)]
    Route { token: String },
    #[returns(racer_payments::Quote)]
    Quote {
        wallet: String,
        token: String,
        purchase: Purchase,
    },
}
const CONFIG: Item<Config> = Item::new("reinvestment_config");
const CREDITS: Map<&Addr, Credit> = Map::new("reinvestment_credits");
const LIABILITIES: Item<Credit> = Item::new("reinvestment_liabilities");
const PREFERENCES: Map<&Addr, Preference> = Map::new("reinvestment_preferences");
const READY: Map<&Addr, Uint128> = Map::new("reinvestment_ready");
const PROGRESS: Map<&Addr, Progress> = Map::new("reinvestment_progress");
fn fail(message: &str) -> StdError {
    StdError::generic_err(message)
}
fn call(address: &Addr, msg: &impl serde::Serialize) -> StdResult<WasmMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: address.to_string(),
        msg: to_json_binary(msg)?,
        funds: vec![],
    })
}
fn index(ship: &ShipType) -> usize {
    match ship {
        ShipType::Racer => 0,
        ShipType::Tank => 1,
        ShipType::Fighter => 2,
    }
}
#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: Empty) -> StdResult<Response> {
    if cw2::get_contract_version(deps.storage)?.contract != "spaceship-racer/reinvestment" {
        return Err(fail("unsupported reward gateway version"));
    }
    let cfg = CONFIG.load(deps.storage)?;
    let backing: racer_rewards::State = deps
        .querier
        .query_wasm_smart(&cfg.backing_rewards, &racer_rewards::QueryMsg::State {})?;
    if backing.allocator != env.contract.address {
        return Err(fail("backing allocator mismatch"));
    }
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/reinvestment",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "upgrade_cycle_reinvestment"))
}
fn payments(deps: Deps, env: Env) -> StdResult<racer_payments::Config> {
    from_json(racer_payments::query(
        deps,
        env,
        racer_payments::QueryMsg::Config {},
    )?)
}
#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    let backing = deps.api.addr_validate(&msg.backing_rewards)?;
    let state: racer_rewards::State = deps
        .querier
        .query_wasm_smart(&backing, &racer_rewards::QueryMsg::State {})?;
    if state.spaceusd != msg.payments.spaceusd {
        return Err(fail("SpaceUSD does not match backing pool"));
    }
    let owner = deps.api.addr_validate(&msg.payments.owner)?;
    let allocator = deps.api.addr_validate(&msg.allocator)?;
    let response = racer_payments::instantiate(deps.branch(), env, info, msg.payments)?;
    CONFIG.save(
        deps.storage,
        &Config {
            owner,
            backing_rewards: backing,
            allocator,
        },
    )?;
    LIABILITIES.save(deps.storage, &Credit::default())?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/reinvestment",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(response.add_attribute("module", "reinvestment"))
}
fn invest(
    deps: DepsMut,
    env: Env,
    wallet: &Addr,
    ship: ShipType,
    amount: Uint128,
) -> StdResult<Response> {
    if amount.is_zero() {
        return Err(fail("zero investment"));
    }
    let payment = payments(deps.as_ref(), env)?;
    let cfg: racer_fleet::Config = deps
        .querier
        .query_wasm_smart(&payment.fleet, &racer_fleet::QueryMsg::Config {})?;
    let fleet: Fleet = deps.querier.query_wasm_smart(
        &payment.fleet,
        &racer_fleet::QueryMsg::Fleet {
            wallet: wallet.to_string(),
        },
    )?;
    if !fleet.purchased {
        return Err(fail("buy your first ship before reinvesting"));
    }
    let i = index(&ship);
    let previous = fleet.next[i];
    let mut progress = PROGRESS.may_load(deps.storage, wallet)?.unwrap_or_default();
    let total = progress.spaceusd[i].checked_add(amount)?;
    let levels = total
        .checked_div(cfg.price_per_tier)?
        .u128()
        .min(u128::from(1000u16.saturating_sub(previous))) as u16;
    // Unspent fractions remain withdrawable. Never lose credit at the legacy level limit.
    let spent = cfg.price_per_tier.checked_mul(Uint128::from(levels))?;
    progress.spaceusd[i] = total.checked_sub(spent)?;
    PROGRESS.save(deps.storage, wallet, &progress)?;
    let mut response = Response::new()
        .add_attribute("action", "invest_rewards")
        .add_attribute("wallet", wallet)
        .add_attribute("ship", i.to_string())
        .add_attribute("levels", levels.to_string())
        .add_attribute("saved_spaceusd", progress.spaceusd[i]);
    if levels > 0 {
        let purchase = if previous == 0 {
            Purchase::Buy {
                ship,
                target_tier: levels,
                sponsor: None,
            }
        } else {
            Purchase::Upgrade {
                ship,
                target_tier: previous + levels,
            }
        };
        response = response.add_message(call(
            &payment.spaceusd,
            &Cw20ExecuteMsg::Send {
                contract: payment.fleet.to_string(),
                amount: spent,
                msg: to_json_binary(&racer_fleet::ReceiveMsg::PayFor {
                    wallet: wallet.to_string(),
                    purchase,
                })?,
            },
        )?);
    }
    Ok(response)
}
#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    match msg {
        ExecuteMsg::ConfigureAllocator { allocator } => {
            let mut config = CONFIG.load(deps.storage)?;
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            config.allocator = deps.api.addr_validate(&allocator)?;
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new().add_attribute("action", "configure_reward_allocator"));
        }
        ExecuteMsg::SetRoute { token, path } => {
            return racer_payments::execute(
                deps,
                env,
                info,
                racer_payments::ExecuteMsg::SetRoute { token, path },
            )
        }
        ExecuteMsg::SetPauseController { controller } => {
            return racer_payments::execute(
                deps,
                env,
                info,
                racer_payments::ExecuteMsg::SetPauseController { controller },
            )
        }
        ExecuteMsg::SetAutoReinvest { ship, bps } => {
            if (ship.is_some() && (bps == 0 || bps > 10000)) || (ship.is_none() && bps != 0) {
                return Err(fail("invalid reinvestment percentage"));
            }
            if ship.is_some() {
                let payment = payments(deps.as_ref(), env)?;
                let fleet: Fleet = deps.querier.query_wasm_smart(
                    &payment.fleet,
                    &racer_fleet::QueryMsg::Fleet {
                        wallet: info.sender.to_string(),
                    },
                )?;
                if !fleet.purchased {
                    return Err(fail(
                        "buy your first ship before enabling automatic upgrades",
                    ));
                }
            }
            PREFERENCES.save(deps.storage, &info.sender, &Preference { ship, bps })?;
            // A changed policy applies only to future allocations. Existing credits stay claimable.
            READY.remove(deps.storage, &info.sender);
            return Ok(Response::new()
                .add_attribute("action", "set_auto_reinvest")
                .add_attribute("wallet", info.sender));
        }
        _ => {}
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    let cfg = CONFIG.load(deps.storage)?;
    let state: racer_rewards::State = deps
        .querier
        .query_wasm_smart(&cfg.backing_rewards, &racer_rewards::QueryMsg::State {})?;
    if state.paused {
        return Err(fail("paused"));
    }
    match msg {
        ExecuteMsg::Receive(receive) => match from_json::<Hook>(&receive.msg) {
            Ok(Hook::Fund {}) => {
                if info.sender != state.spaceusd && info.sender != state.srt {
                    return Err(fail("unsupported funding token"));
                }
                Ok(Response::new().add_message(call(
                    &info.sender,
                    &Cw20ExecuteMsg::Send {
                        contract: cfg.backing_rewards.to_string(),
                        amount: receive.amount,
                        msg: to_json_binary(&racer_rewards::ReceiveMsg::Fund {})?,
                    },
                )?))
            }
            Ok(Hook::Invest { ship }) => {
                if info.sender != state.spaceusd {
                    return Err(fail("SpaceUSD required for level progress"));
                }
                let wallet = deps.api.addr_validate(&receive.sender)?;
                invest(deps, env, &wallet, ship, receive.amount)
            }
            Err(_) => racer_payments::execute(
                deps,
                env,
                info,
                racer_payments::ExecuteMsg::Receive(receive),
            ),
        },
        ExecuteMsg::Checkpoint {} => Ok(Response::new().add_message(call(
            &cfg.backing_rewards,
            &racer_rewards::ExecuteMsg::Checkpoint {},
        )?)),
        ExecuteMsg::Allocate { recipients } => {
            if info.sender != cfg.allocator {
                return Err(fail("unauthorized allocator"));
            }
            if recipients.is_empty() || recipients.len() > 50 {
                return Err(fail("invalid allocation batch"));
            }
            let mut total = Credit::default();
            let mut automatic = Vec::new();
            for allocation in recipients {
                let wallet = deps.api.addr_validate(&allocation.wallet)?;
                total.spaceusd = total.spaceusd.checked_add(allocation.spaceusd)?;
                total.srt = total.srt.checked_add(allocation.srt)?;
                CREDITS.update(deps.storage, &wallet, |old| -> StdResult<_> {
                    let mut credit = old.unwrap_or_default();
                    credit.spaceusd = credit.spaceusd.checked_add(allocation.spaceusd)?;
                    credit.srt = credit.srt.checked_add(allocation.srt)?;
                    Ok(credit)
                })?;
                let preference = PREFERENCES
                    .may_load(deps.storage, &wallet)?
                    .unwrap_or_default();
                if preference.ship.is_some() {
                    let amount = allocation
                        .spaceusd
                        .multiply_ratio(u128::from(preference.bps), 10000u128);
                    if !amount.is_zero() {
                        READY.update(deps.storage, &wallet, |old| -> StdResult<_> {
                            Ok(old.unwrap_or_default().checked_add(amount)?)
                        })?;
                        if !automatic.contains(&wallet) {
                            automatic.push(wallet);
                        }
                    }
                }
            }
            LIABILITIES.update(deps.storage, |mut value| -> StdResult<_> {
                value.spaceusd = value.spaceusd.checked_add(total.spaceusd)?;
                value.srt = value.srt.checked_add(total.srt)?;
                Ok(value)
            })?;
            if total == Credit::default() {
                return Ok(Response::new());
            }
            let mut response = Response::new()
                .add_message(call(
                    &cfg.backing_rewards,
                    &racer_rewards::ExecuteMsg::Allocate {
                        recipients: vec![Allocation {
                            wallet: env.contract.address.to_string(),
                            spaceusd: total.spaceusd,
                            srt: total.srt,
                        }],
                    },
                )?)
                .add_message(call(
                    &cfg.backing_rewards,
                    &racer_rewards::ExecuteMsg::Claim {},
                )?);
            for wallet in automatic {
                response = response.add_submessage(SubMsg {
                    id: 2,
                    msg: call(
                        &env.contract.address,
                        &ExecuteMsg::ProcessAuto {
                            wallet: wallet.to_string(),
                        },
                    )?
                    .into(),
                    gas_limit: Some(1_500_000),
                    reply_on: ReplyOn::Error,
                });
            }
            Ok(response)
        }
        ExecuteMsg::PayMaintenance { amount, treasury } => {
            if info.sender != cfg.allocator {
                return Err(fail("unauthorized allocator"));
            }
            Ok(Response::new().add_message(call(
                &cfg.backing_rewards,
                &racer_rewards::ExecuteMsg::PayMaintenance { amount, treasury },
            )?))
        }
        ExecuteMsg::Claim {} => {
            let credit = CREDITS
                .may_load(deps.storage, &info.sender)?
                .unwrap_or_default();
            if credit == Credit::default() {
                return Err(fail("nothing to claim"));
            }
            CREDITS.remove(deps.storage, &info.sender);
            READY.remove(deps.storage, &info.sender);
            LIABILITIES.update(deps.storage, |mut value| -> StdResult<_> {
                value.spaceusd = value.spaceusd.checked_sub(credit.spaceusd)?;
                value.srt = value.srt.checked_sub(credit.srt)?;
                Ok(value)
            })?;
            let mut response = Response::new()
                .add_attribute("action", "claim_rewards")
                .add_attribute("wallet", &info.sender);
            for (token, amount) in [(&state.spaceusd, credit.spaceusd), (&state.srt, credit.srt)] {
                if !amount.is_zero() {
                    response = response.add_message(call(
                        token,
                        &Cw20ExecuteMsg::Transfer {
                            recipient: info.sender.to_string(),
                            amount,
                        },
                    )?);
                }
            }
            Ok(response)
        }
        ExecuteMsg::ProcessAuto { wallet } => {
            let wallet = deps.api.addr_validate(&wallet)?;
            let preference = PREFERENCES
                .may_load(deps.storage, &wallet)?
                .unwrap_or_default();
            let ship = preference
                .ship
                .ok_or_else(|| fail("automatic reinvestment disabled"))?;
            let amount = READY.may_load(deps.storage, &wallet)?.unwrap_or_default();
            if amount.is_zero() {
                return Err(fail("no automatic reward allocation pending"));
            }
            CREDITS.update(deps.storage, &wallet, |old| -> StdResult<_> {
                let mut value = old.unwrap_or_default();
                value.spaceusd = value.spaceusd.checked_sub(amount)?;
                Ok(value)
            })?;
            LIABILITIES.update(deps.storage, |mut value| -> StdResult<_> {
                value.spaceusd = value.spaceusd.checked_sub(amount)?;
                Ok(value)
            })?;
            READY.remove(deps.storage, &wallet);
            invest(deps, env, &wallet, ship, amount)
        }
        ExecuteMsg::WithdrawProgress { ship } => {
            let i = index(&ship);
            let mut progress = PROGRESS
                .may_load(deps.storage, &info.sender)?
                .unwrap_or_default();
            let amount = progress.spaceusd[i];
            if amount.is_zero() {
                return Err(fail("no saved progress"));
            }
            progress.spaceusd[i] = Uint128::zero();
            PROGRESS.save(deps.storage, &info.sender, &progress)?;
            Ok(Response::new()
                .add_message(call(
                    &state.spaceusd,
                    &Cw20ExecuteMsg::Transfer {
                        recipient: info.sender.to_string(),
                        amount,
                    },
                )?)
                .add_attribute("action", "withdraw_level_progress"))
        }
        ExecuteMsg::SetRoute { .. }
        | ExecuteMsg::ConfigureAllocator { .. }
        | ExecuteMsg::SetPauseController { .. }
        | ExecuteMsg::SetAutoReinvest { .. } => unreachable!(),
    }
}
#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    if msg.id == 2 {
        // The failed self-call rolls back its credit/progress writes. Allocation
        // remains backed and claimable, with READY retained for keeper retries.
        return Ok(Response::new().add_attribute("action", "auto_reinvestment_deferred"));
    }
    racer_payments::reply(deps, env, msg)
}
#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => {
            racer_payments::query(deps, env, racer_payments::QueryMsg::Config {})
        }
        QueryMsg::Route { token } => {
            racer_payments::query(deps, env, racer_payments::QueryMsg::Route { token })
        }
        QueryMsg::Quote {
            wallet,
            token,
            purchase,
        } => racer_payments::query(
            deps,
            env,
            racer_payments::QueryMsg::Quote {
                wallet,
                token,
                purchase,
            },
        ),
        QueryMsg::ReinvestmentConfig {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::State {} => {
            let cfg = CONFIG.load(deps.storage)?;
            let mut state: racer_rewards::State = deps
                .querier
                .query_wasm_smart(&cfg.backing_rewards, &racer_rewards::QueryMsg::State {})?;
            let own = LIABILITIES.load(deps.storage)?;
            state.liabilities.spaceusd = state.liabilities.spaceusd.checked_add(own.spaceusd)?;
            state.liabilities.srt = state.liabilities.srt.checked_add(own.srt)?;
            state.allocator = cfg.allocator;
            to_json_binary(&state)
        }
        QueryMsg::Credit { wallet } => to_json_binary(
            &CREDITS
                .may_load(deps.storage, &deps.api.addr_validate(&wallet)?)?
                .unwrap_or_default(),
        ),
        QueryMsg::Reinvestment { wallet } => {
            let wallet = deps.api.addr_validate(&wallet)?;
            to_json_binary(&Reinvestment {
                preference: PREFERENCES
                    .may_load(deps.storage, &wallet)?
                    .unwrap_or_default(),
                ready: READY.may_load(deps.storage, &wallet)?.unwrap_or_default(),
                progress: PROGRESS
                    .may_load(deps.storage, &wallet)?
                    .unwrap_or_default(),
            })
        }
        QueryMsg::AutoReady { start_after, limit } => {
            let start = start_after
                .map(|wallet| deps.api.addr_validate(&wallet))
                .transpose()?;
            to_json_binary(
                &READY
                    .keys(
                        deps.storage,
                        start.as_ref().map(Bound::exclusive),
                        None,
                        Order::Ascending,
                    )
                    .take(limit.unwrap_or(20).clamp(1, 50) as usize)
                    .map(|wallet| wallet.map(|wallet| wallet.to_string()))
                    .collect::<StdResult<Vec<_>>>()?,
            )
        }
    }
}
