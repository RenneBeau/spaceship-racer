//! Load-injection CW20 fixture, restricted to the separate cost chain.
//! SendFor debits the named actor through the normal CW20 implementation so the
//! production engine sees real balances/payment callbacks in one action block.
//! Input/setup gas is excluded from keeper measurements. Never a game asset.
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{entry_point, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128};
use cw_storage_plus::Item;
use cw20_base::{msg::{ExecuteMsg as Cw20Msg, InstantiateMsg, QueryMsg}, ContractError};
const OWNER: Item<Addr> = Item::new("cost_fixture_owner");
#[cw_serde]
pub enum ExecuteMsg {
    Transfer { recipient: String, amount: Uint128 },
    Send { contract: String, amount: Uint128, msg: Binary },
    SendFor { wallet: String, contract: String, amount: Uint128, msg: Binary },
}
#[entry_point]
pub fn instantiate(mut deps: DepsMut, env: Env, info: MessageInfo, msg: InstantiateMsg) -> Result<Response, ContractError> {
    if env.block.chain_id != "spaceship-racer-cost" { return Err(StdError::generic_err("benchmark fixture is cost-chain-only").into()); }
    OWNER.save(deps.storage, &info.sender)?;
    cw20_base::contract::instantiate(deps.branch(), env, info, msg)
}
#[entry_point]
pub fn execute(deps: DepsMut, env: Env, mut info: MessageInfo, msg: ExecuteMsg) -> Result<Response, ContractError> {
    let message = match msg {
        ExecuteMsg::Transfer { recipient, amount } => Cw20Msg::Transfer { recipient, amount },
        ExecuteMsg::Send { contract, amount, msg } => Cw20Msg::Send { contract, amount, msg },
        ExecuteMsg::SendFor { wallet, contract, amount, msg } => {
            if info.sender != OWNER.load(deps.storage)? { return Err(ContractError::Unauthorized {}); }
            info.sender = deps.api.addr_validate(&wallet)?;
            Cw20Msg::Send { contract, amount, msg }
        }
    };
    cw20_base::contract::execute(deps, env, info, message)
}
#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> { cw20_base::contract::query(deps, env, msg) }

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    #[test]
    fn fixture_cannot_be_instantiated_on_the_live_game_chain() {
        let mut deps = mock_dependencies(); let mut env = mock_env(); env.block.chain_id = "spaceship-racer-local".into();
        let msg = InstantiateMsg { name: "Bench SRT".into(), symbol: "SRT".into(), decimals: 6, initial_balances: vec![], mint: None, marketing: None };
        assert!(instantiate(deps.as_mut(), env, mock_info("owner", &[]), msg).is_err());
    }
    #[test]
    fn only_fixture_owner_can_inject_actor_payments() {
        let mut deps = mock_dependencies(); let mut env = mock_env(); env.block.chain_id = "spaceship-racer-cost".into();
        instantiate(deps.as_mut(), env.clone(), mock_info("owner", &[]), InstantiateMsg { name: "Bench SRT".into(), symbol: "SRT".into(), decimals: 6, initial_balances: vec![], mint: None, marketing: None }).unwrap();
        assert!(execute(deps.as_mut(), env, mock_info("other", &[]), ExecuteMsg::SendFor { wallet: "actor".into(), contract: "engine".into(), amount: Uint128::new(1), msg: Binary::default() }).is_err());
    }
}
