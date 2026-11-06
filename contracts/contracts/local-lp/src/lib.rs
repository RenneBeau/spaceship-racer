//! Local fixture entry points delegating to the exact pinned upstream LP token.
//! No replacement AMM/LP math; this only avoids upstream schema/dev-tool targets.
use cosmwasm_std::{entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
use cw20_mintable::{
    msg::{ExecuteMsg, InstantiateMsg, QueryMsg},
    ContractError,
};
#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    cw20_mintable::contract::instantiate(deps, env, info, msg)
}
#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    cw20_mintable::contract::execute(deps, env, info, msg)
}
#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    cw20_mintable::contract::query(deps, env, msg)
}
