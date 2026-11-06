//! Narrow, pool-only wire interface matching CL8Y router revision
//! 8e865d3721231189cbad1730c7a24698038c9cfb. External implementations are tested
//! as dependencies, not embedded in the game contracts.
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Decimal, Deps, Empty, StdError, StdResult, Uint128, WasmMsg,
};
use cw20::{BalanceResponse, Cw20ExecuteMsg, Cw20QueryMsg};

#[cw_serde]
pub enum AssetInfo {
    Token { contract_addr: String },
}
#[cw_serde]
pub enum Operation {
    TerraSwap {
        offer_asset_info: AssetInfo,
        ask_asset_info: AssetInfo,
        hybrid: Option<Empty>,
        greedy: Option<Empty>,
        min_return: Option<Uint128>,
    },
}
#[cw_serde]
pub enum Hook {
    ExecuteSwapOperations {
        operations: Vec<Operation>,
        max_spread: Decimal,
        minimum_receive: Option<Uint128>,
        to: Option<String>,
        deadline: Option<u64>,
        unwrap_output: Option<bool>,
    },
}
#[cw_serde]
pub enum QueryMsg {
    ReverseSimulateSwapOperations {
        ask_amount: Uint128,
        operations: Vec<Operation>,
        trader: Option<String>,
        sender: Option<String>,
    },
}
#[cw_serde]
pub struct Simulation {
    pub amount: Uint128,
}

pub fn route(deps: Deps, input: &Addr, usd: &Addr, path: Vec<String>) -> StdResult<Vec<Addr>> {
    if input == usd {
        if path != vec![usd.to_string()] {
            return Err(StdError::generic_err("invalid direct route"));
        }
        return Ok(vec![usd.clone()]);
    }
    if !(2..=5).contains(&path.len()) {
        return Err(StdError::generic_err("route must contain 1 to 4 hops"));
    }
    let path = path
        .into_iter()
        .map(|p| deps.api.addr_validate(&p))
        .collect::<StdResult<Vec<_>>>()?;
    if path.first() != Some(input)
        || path.last() != Some(usd)
        || path.iter().enumerate().any(|(i, a)| path[..i].contains(a))
    {
        return Err(StdError::generic_err("invalid or cyclic route"));
    }
    Ok(path)
}
pub fn operations(path: &[Addr]) -> Vec<Operation> {
    path.windows(2)
        .map(|pair| Operation::TerraSwap {
            offer_asset_info: AssetInfo::Token {
                contract_addr: pair[0].to_string(),
            },
            ask_asset_info: AssetInfo::Token {
                contract_addr: pair[1].to_string(),
            },
            hybrid: None,
            greedy: None,
            min_return: None,
        })
        .collect()
}
pub fn balance(deps: Deps, token: &Addr, wallet: &Addr) -> StdResult<Uint128> {
    let response: BalanceResponse = deps.querier.query_wasm_smart(
        token,
        &Cw20QueryMsg::Balance {
            address: wallet.to_string(),
        },
    )?;
    Ok(response.balance)
}
pub fn validate_trade(now: u64, deadline: u64, spread: Decimal, maximum: Decimal) -> StdResult<()> {
    if deadline < now
        || deadline.saturating_sub(now) > 3600
        || spread > maximum
        || maximum > Decimal::percent(50)
    {
        return Err(StdError::generic_err("expired trade or invalid spread"));
    }
    Ok(())
}
pub fn swap(
    router: &Addr,
    path: &[Addr],
    amount: Uint128,
    minimum: Uint128,
    spread: Decimal,
    deadline: u64,
    recipient: &Addr,
) -> StdResult<WasmMsg> {
    let input = path
        .first()
        .ok_or_else(|| StdError::generic_err("missing swap path"))?;
    Ok(WasmMsg::Execute {
        contract_addr: input.to_string(),
        funds: vec![],
        msg: to_json_binary(&Cw20ExecuteMsg::Send {
            contract: router.to_string(),
            amount,
            msg: to_json_binary(&Hook::ExecuteSwapOperations {
                operations: operations(path),
                max_spread: spread,
                minimum_receive: Some(minimum),
                to: Some(recipient.to_string()),
                deadline: Some(deadline),
                unwrap_output: Some(false),
            })?,
        })?,
    })
}
