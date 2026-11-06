use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult,
};
use cw_storage_plus::Item;
#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
}
#[cw_serde]
pub enum ExecuteMsg {
    SetPaused { paused: bool },
}
#[cw_serde]
pub struct State {
    pub owner: Addr,
    pub paused: bool,
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(State)]
    State {},
}
const STATE: Item<State> = Item::new("state");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(StdError::generic_err("native funds unsupported"));
    }
    STATE.save(
        deps.storage,
        &State {
            owner: deps.api.addr_validate(&msg.owner)?,
            paused: false,
        },
    )?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/pause",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_pause"))
}
#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(StdError::generic_err("native funds unsupported"));
    }
    let mut state = STATE.load(deps.storage)?;
    if info.sender != state.owner {
        return Err(StdError::generic_err("unauthorized"));
    }
    let ExecuteMsg::SetPaused { paused } = msg;
    state.paused = paused;
    STATE.save(deps.storage, &state)?;
    Ok(Response::new()
        .add_attribute("action", "set_global_pause")
        .add_attribute("paused", paused.to_string()))
}
#[entry_point]
pub fn query(deps: Deps, _env: Env, _msg: QueryMsg) -> StdResult<Binary> {
    to_json_binary(&STATE.load(deps.storage)?)
}
