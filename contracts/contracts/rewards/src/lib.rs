use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult, Uint128, Uint256, WasmMsg,
};
use cw20::{BalanceResponse, Cw20ExecuteMsg, Cw20QueryMsg, Cw20ReceiveMsg};
use cw_storage_plus::{Item, Map};
use game_common::{BPS, YEAR_SECONDS};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub allocator: String,
    pub spaceusd: String,
    pub srt: String,
    pub annual_bps: u16,
}

#[cw_serde]
pub enum ExecuteMsg {
    SetPauseController {
        controller: Option<String>,
    },
    Receive(Cw20ReceiveMsg),
    Checkpoint {},
    Allocate {
        recipients: Vec<Allocation>,
    },
    PayMaintenance {
        amount: Uint128,
        treasury: String,
    },
    Claim {},
    Configure {
        allocator: Option<String>,
        annual_bps: Option<u16>,
        paused: Option<bool>,
    },
}

#[cw_serde]
pub enum ReceiveMsg {
    Fund {},
}

#[cw_serde]
pub struct Allocation {
    pub wallet: String,
    pub spaceusd: Uint128,
    pub srt: Uint128,
}

#[cw_serde]
#[derive(Default)]
pub struct Credit {
    pub spaceusd: Uint128,
    pub srt: Uint128,
}

#[cw_serde]
pub struct State {
    pub owner: Addr,
    pub allocator: Addr,
    pub spaceusd: Addr,
    pub srt: Addr,
    pub annual_bps: u16,
    pub paused: bool,
    pub checkpoint_time: u64,
    pub accrual_remainder: Uint256,
    pub reserve: Uint128,
    pub available: Credit,
    pub liabilities: Credit,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(State)]
    State {},
    #[returns(Credit)]
    Credit { wallet: String },
}

const STATE: Item<State> = Item::new("state");
const CREDITS: Map<&Addr, Credit> = Map::new("credits");
const MAX_ALLOCATIONS: usize = 50;

fn fail(message: &str) -> StdError {
    StdError::generic_err(message)
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    if msg.annual_bps > 10000 {
        return Err(fail("invalid annual rate"));
    }
    let state = State {
        owner: deps.api.addr_validate(&msg.owner)?,
        allocator: deps.api.addr_validate(&msg.allocator)?,
        spaceusd: deps.api.addr_validate(&msg.spaceusd)?,
        srt: deps.api.addr_validate(&msg.srt)?,
        annual_bps: msg.annual_bps,
        paused: false,
        checkpoint_time: env.block.time.seconds(),
        accrual_remainder: Uint256::zero(),
        reserve: Uint128::zero(),
        available: Credit::default(),
        liabilities: Credit::default(),
    };
    if state.spaceusd == state.srt {
        return Err(fail("distinct tokens required"));
    }
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/rewards",
        env!("CARGO_PKG_VERSION"),
    )?;
    STATE.save(deps.storage, &state)?;
    Ok(Response::new().add_attribute("action", "instantiate_rewards"))
}

fn checkpoint(state: &mut State, now: u64) -> StdResult<()> {
    let elapsed = now
        .checked_sub(state.checkpoint_time)
        .ok_or_else(|| fail("time moved backwards"))?;
    let denominator = Uint256::from(BPS * u128::from(YEAR_SECONDS));
    let numerator = Uint256::from(state.reserve)
        .checked_mul(Uint256::from(u128::from(elapsed)))?
        .checked_mul(Uint256::from(u128::from(state.annual_bps)))?
        .checked_add(state.accrual_remainder)?;
    let amount = Uint128::try_from(
        numerator
            .checked_div(denominator)?
            .min(Uint256::from(state.reserve)),
    )?;
    state.reserve = state.reserve.checked_sub(amount)?;
    state.available.spaceusd = state.available.spaceusd.checked_add(amount)?;
    state.accrual_remainder = if state.reserve.is_zero() {
        Uint256::zero()
    } else {
        numerator.checked_rem(denominator)?
    };
    state.checkpoint_time = now;
    Ok(())
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    let mut state = STATE.load(deps.storage)?;
    if let ExecuteMsg::SetPauseController { controller } = msg {
        return game_common::pause::set_controller(deps, &info, &state.owner, controller);
    }
    if let ExecuteMsg::Configure {
        allocator,
        annual_bps,
        paused,
    } = msg
    {
        if info.sender != state.owner {
            return Err(fail("unauthorized"));
        }
        checkpoint(&mut state, env.block.time.seconds())?;
        if let Some(value) = allocator {
            state.allocator = deps.api.addr_validate(&value)?;
        }
        if let Some(value) = annual_bps {
            if value > 10000 {
                return Err(fail("invalid annual rate"));
            }
            state.annual_bps = value;
            state.checkpoint_time = env.block.time.seconds();
        }
        if let Some(value) = paused {
            state.paused = value;
        }
        STATE.save(deps.storage, &state)?;
        return Ok(Response::new().add_attribute("action", "configure_rewards"));
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    if state.paused {
        return Err(fail("paused"));
    }
    checkpoint(&mut state, env.block.time.seconds())?;
    let mut response = Response::new();
    match msg {
        ExecuteMsg::Receive(receive) => {
            let _: ReceiveMsg = from_json(receive.msg)?;
            if receive.amount.is_zero() {
                return Err(fail("zero funding"));
            }
            let accounted = if info.sender == state.spaceusd {
                state
                    .reserve
                    .checked_add(state.available.spaceusd)?
                    .checked_add(state.liabilities.spaceusd)?
            } else if info.sender == state.srt {
                state.available.srt.checked_add(state.liabilities.srt)?
            } else {
                return Err(fail("unsupported token"));
            };
            let balance: BalanceResponse = deps.querier.query_wasm_smart(
                &info.sender,
                &Cw20QueryMsg::Balance {
                    address: env.contract.address.to_string(),
                },
            )?;
            if balance.balance < accounted.checked_add(receive.amount)? {
                return Err(fail("unbacked funding"));
            }
            if info.sender == state.spaceusd {
                state.reserve = state.reserve.checked_add(receive.amount)?;
                state.checkpoint_time = env.block.time.seconds();
            } else {
                state.available.srt = state.available.srt.checked_add(receive.amount)?;
            }
            response = response
                .add_attribute("action", "fund_rewards")
                .add_attribute("amount", receive.amount);
        }
        ExecuteMsg::Checkpoint {} => {
            response = response.add_attribute("action", "checkpoint_rewards");
        }
        ExecuteMsg::Allocate { recipients } => {
            if info.sender != state.allocator {
                return Err(fail("unauthorized"));
            }
            if recipients.is_empty() || recipients.len() > MAX_ALLOCATIONS {
                return Err(fail("invalid batch size"));
            }
            for allocation in recipients {
                let wallet = deps.api.addr_validate(&allocation.wallet)?;
                state.available.spaceusd =
                    state.available.spaceusd.checked_sub(allocation.spaceusd)?;
                state.available.srt = state.available.srt.checked_sub(allocation.srt)?;
                state.liabilities.spaceusd = state
                    .liabilities
                    .spaceusd
                    .checked_add(allocation.spaceusd)?;
                state.liabilities.srt = state.liabilities.srt.checked_add(allocation.srt)?;
                CREDITS.update(deps.storage, &wallet, |old| -> StdResult<_> {
                    let mut credit = old.unwrap_or_default();
                    credit.spaceusd = credit.spaceusd.checked_add(allocation.spaceusd)?;
                    credit.srt = credit.srt.checked_add(allocation.srt)?;
                    Ok(credit)
                })?;
            }
            response = response.add_attribute("action", "allocate_rewards");
        }
        ExecuteMsg::PayMaintenance { amount, treasury } => {
            if info.sender != state.allocator {
                return Err(fail("unauthorized"));
            }
            let treasury = deps.api.addr_validate(&treasury)?;
            if amount.is_zero() {
                return Err(fail("zero maintenance"));
            }
            state.available.spaceusd = state.available.spaceusd.checked_sub(amount)?;
            response = response
                .add_message(WasmMsg::Execute {
                    contract_addr: state.spaceusd.to_string(),
                    msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                        recipient: treasury.to_string(),
                        amount,
                    })?,
                    funds: vec![],
                })
                .add_attribute("action", "pay_maintenance")
                .add_attribute("amount", amount);
        }
        ExecuteMsg::Claim {} => {
            let credit = CREDITS
                .may_load(deps.storage, &info.sender)?
                .unwrap_or_default();
            if credit == Credit::default() {
                return Err(fail("nothing to claim"));
            }
            CREDITS.remove(deps.storage, &info.sender);
            state.liabilities.spaceusd = state.liabilities.spaceusd.checked_sub(credit.spaceusd)?;
            state.liabilities.srt = state.liabilities.srt.checked_sub(credit.srt)?;
            for (token, amount) in [(&state.spaceusd, credit.spaceusd), (&state.srt, credit.srt)] {
                if !amount.is_zero() {
                    response = response.add_message(WasmMsg::Execute {
                        contract_addr: token.to_string(),
                        msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                            recipient: info.sender.to_string(),
                            amount,
                        })?,
                        funds: vec![],
                    });
                }
            }
            response = response
                .add_attribute("action", "claim_rewards")
                .add_attribute("wallet", info.sender);
        }
        ExecuteMsg::Configure { .. } => unreachable!(),
        ExecuteMsg::SetPauseController { .. } => unreachable!(),
    }
    STATE.save(deps.storage, &state)?;
    Ok(response)
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::State {} => to_json_binary(&STATE.load(deps.storage)?),
        QueryMsg::Credit { wallet } => {
            let addr = deps.api.addr_validate(&wallet)?;
            to_json_binary(&CREDITS.may_load(deps.storage, &addr)?.unwrap_or_default())
        }
    }
}
