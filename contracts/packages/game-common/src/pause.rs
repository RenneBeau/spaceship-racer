//! Each participating contract checks the same controller on every non-admin
//! execution. A single controller update is atomic across the whole game.
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Deps, DepsMut, MessageInfo, Response, StdError, StdResult};
use cw_storage_plus::Item;

#[cw_serde]
pub enum QueryMsg {
    State {},
}
#[cw_serde]
pub struct State {
    pub owner: Addr,
    pub paused: bool,
}

const CONTROLLER: Item<Option<Addr>> = Item::new("game_pause_controller");

pub fn ensure_running(deps: Deps) -> StdResult<()> {
    if let Some(controller) = CONTROLLER.may_load(deps.storage)?.flatten() {
        let state: State = deps
            .querier
            .query_wasm_smart(controller, &QueryMsg::State {})?;
        if state.paused {
            return Err(StdError::generic_err("game globally paused"));
        }
    }
    Ok(())
}

pub fn set_controller(
    deps: DepsMut,
    info: &MessageInfo,
    owner: &Addr,
    controller: Option<String>,
) -> StdResult<Response> {
    if info.sender != *owner {
        return Err(StdError::generic_err("unauthorized"));
    }
    let controller = controller.map(|s| deps.api.addr_validate(&s)).transpose()?;
    if let Some(address) = &controller {
        let _: State = deps
            .querier
            .query_wasm_smart(address, &QueryMsg::State {})?;
    }
    CONTROLLER.save(deps.storage, &controller)?;
    Ok(Response::new()
        .add_attribute("action", "set_pause_controller")
        .add_attribute(
            "controller",
            controller
                .map(|a| a.to_string())
                .unwrap_or_else(|| "none".into()),
        ))
}

pub fn controller(deps: Deps) -> StdResult<Option<Addr>> {
    Ok(CONTROLLER.may_load(deps.storage)?.flatten())
}
