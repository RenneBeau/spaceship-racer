//! CW721 collection with immutable typed gameplay attributes, assigned at mint.
//! Minter is intended to be the recruitment/milestone contract, not the indexer.
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Deps, DepsMut, Empty, Env, MessageInfo, Order, Response,
    StdError, StdResult,
};
use cw721_base::{ContractError, Cw721Contract};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub enum Kind {
    Commander,
    RacerSkin,
    TankSkin,
    FighterSkin,
}

#[cw_serde]
pub enum Bonus {
    Speed,
    Acceleration,
    Shield,
    Reward,
    Critical,
    Repair,
    Cosmetic,
}

#[cw_serde]
pub enum Rarity {
    Common,
    Rare,
    Epic,
    Legendary,
}

#[cw_serde]
pub struct Attributes {
    pub code: String,
    pub kind: Kind,
    pub rarity: Rarity,
    pub bonus: Bonus,
    pub bonus_bps: u16,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub name: String,
    pub symbol: String,
    pub minter: String,
    pub owner: String,
}
#[cw_serde]
pub enum ExtensionMsg {
    SetPaused { paused: bool },
    SetPauseController { controller: Option<String> },
    SetCustodian { address: String, allowed: bool },
}
impl cosmwasm_std::CustomMsg for ExtensionMsg {}
pub type ExecuteMsg = cw721_base::ExecuteMsg<Attributes, ExtensionMsg>;
#[cw_serde]
pub enum ExtensionQuery {
    Controls {},
    SkinCounts {
        wallet: String,
        kind: Kind,
        codes: Vec<String>,
    },
    Beneficiary {
        token_id: String,
    },
    Custodian {
        address: String,
    },
}
impl cosmwasm_std::CustomMsg for ExtensionQuery {}
#[cw_serde]
pub struct SkinCount {
    pub code: String,
    pub count: u32,
}
#[cw_serde]
pub struct SkinCounts {
    pub counts: Vec<SkinCount>,
}
#[cw_serde]
pub struct Beneficiary {
    pub owner: Addr,
    pub beneficiary: Addr,
}
#[cw_serde]
pub struct Custodian {
    pub allowed: bool,
}
#[cw_serde]
pub struct Controls {
    pub owner: Addr,
    pub paused: bool,
    pub pause_controller: Option<Addr>,
}
pub type QueryMsg = cw721_base::QueryMsg<ExtensionQuery>;
type Collection<'a> = Cw721Contract<'a, Attributes, Empty, ExtensionMsg, ExtensionQuery>;
const OWNER: Item<Addr> = Item::new("pause_owner");
const PAUSED: Item<bool> = Item::new("paused");
const CUSTODIANS: Map<&Addr, bool> = Map::new("skin_custodians");
const BENEFICIARIES: Map<&str, Addr> = Map::new("skin_beneficiaries");
const SKIN_COUNTS: Map<(&Addr, u8, &str), u32> = Map::new("skin_counts");

pub fn skin_kind(kind: &Kind) -> StdResult<u8> {
    match kind {
        Kind::RacerSkin => Ok(1),
        Kind::TankSkin => Ok(2),
        Kind::FighterSkin => Ok(3),
        Kind::Commander => Err(StdError::generic_err("not a ship skin")),
    }
}
fn is_custodian(deps: Deps, address: &Addr) -> StdResult<bool> {
    Ok(CUSTODIANS.may_load(deps.storage, address)?.unwrap_or(false))
}

fn change_count(
    storage: &mut dyn cosmwasm_std::Storage,
    wallet: &Addr,
    kind: u8,
    code: &str,
    increase: bool,
) -> StdResult<()> {
    let previous = SKIN_COUNTS
        .may_load(storage, (wallet, kind, code))?
        .unwrap_or(0);
    let next = if increase {
        previous.checked_add(1)
    } else {
        previous.checked_sub(1)
    }
    .ok_or_else(|| StdError::generic_err("skin ownership counter overflow/underflow"))?;
    if next == 0 {
        SKIN_COUNTS.remove(storage, (wallet, kind, code));
    } else {
        SKIN_COUNTS.save(storage, (wallet, kind, code), &next)?;
    }
    Ok(())
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    if !info.funds.is_empty() {
        return Err(StdError::generic_err("native funds unsupported").into());
    }
    OWNER.save(deps.storage, &deps.api.addr_validate(&msg.owner)?)?;
    PAUSED.save(deps.storage, &false)?;
    Ok(Collection::default().instantiate(
        deps,
        env,
        info,
        cw721_base::InstantiateMsg {
            name: msg.name,
            symbol: msg.symbol,
            minter: msg.minter,
        },
    )?)
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    if !info.funds.is_empty() {
        return Err(StdError::generic_err("native funds unsupported").into());
    }
    if let ExecuteMsg::Extension {
        msg: ExtensionMsg::SetPauseController { controller },
    } = msg
    {
        let owner = OWNER.load(deps.storage)?;
        return Ok(game_common::pause::set_controller(
            deps, &info, &owner, controller,
        )?);
    }
    if let ExecuteMsg::Extension {
        msg: ExtensionMsg::SetCustodian { address, allowed },
    } = msg
    {
        if info.sender != OWNER.load(deps.storage)? {
            return Err(StdError::generic_err("unauthorized").into());
        }
        let address = deps.api.addr_validate(&address)?;
        deps.querier.query_wasm_contract_info(&address)?;
        if !CUSTODIANS.has(deps.storage, &address)
            && Collection::default()
                .tokens
                .idx
                .owner
                .prefix(address.clone())
                .keys(deps.storage, None, None, Order::Ascending)
                .next()
                .transpose()?
                .is_some()
        {
            return Err(StdError::generic_err("register custody before accepting NFTs").into());
        }
        CUSTODIANS.save(deps.storage, &address, &allowed)?;
        return Ok(Response::new()
            .add_attribute("action", "set_skin_custodian")
            .add_attribute("address", address)
            .add_attribute("allowed", allowed.to_string()));
    }
    if let ExecuteMsg::Extension {
        msg: ExtensionMsg::SetPaused { paused },
    } = msg
    {
        if info.sender != OWNER.load(deps.storage)? {
            return Err(StdError::generic_err("unauthorized").into());
        }
        PAUSED.save(deps.storage, &paused)?;
        return Ok(Response::new()
            .add_attribute("action", "pause_collection")
            .add_attribute("paused", paused.to_string()));
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    if PAUSED.load(deps.storage)? {
        return Err(StdError::generic_err("paused").into());
    }
    // Prepare only ownership effects; base CW721 authorization must succeed
    // before committing the counters. Receiver callbacks run after both writes.
    let effect = match &msg {
        ExecuteMsg::Mint {
            token_id,
            owner,
            extension,
            ..
        } if extension.kind != Kind::Commander => {
            if extension.code.is_empty()
                || extension.code.len() > 64
                || extension.bonus != Bonus::Cosmetic
                || extension.bonus_bps != 0
            {
                return Err(StdError::generic_err("invalid cosmetic skin attributes").into());
            }
            let owner = deps.api.addr_validate(owner)?;
            if CUSTODIANS.has(deps.storage, &owner) {
                return Err(StdError::generic_err("cannot mint directly into game custody").into());
            }
            Some((token_id.clone(), extension.clone(), None, Some(owner)))
        }
        ExecuteMsg::TransferNft {
            token_id,
            recipient,
        } => {
            let nft = Collection::default().tokens.load(deps.storage, token_id)?;
            if nft.extension.kind == Kind::Commander {
                None
            } else {
                let recipient = deps.api.addr_validate(recipient)?;
                if CUSTODIANS.has(deps.storage, &recipient) {
                    return Err(StdError::generic_err("use SendNft to enter game custody").into());
                }
                Some((
                    token_id.clone(),
                    nft.extension,
                    Some(BENEFICIARIES.load(deps.storage, token_id)?),
                    Some(recipient),
                ))
            }
        }
        ExecuteMsg::SendNft {
            token_id, contract, ..
        } => {
            let nft = Collection::default().tokens.load(deps.storage, token_id)?;
            if nft.extension.kind == Kind::Commander {
                None
            } else {
                let recipient = deps.api.addr_validate(contract)?;
                if CUSTODIANS.has(deps.storage, &recipient)
                    && !is_custodian(deps.as_ref(), &recipient)?
                {
                    return Err(StdError::generic_err("skin custodian disabled").into());
                }
                let beneficiary = if is_custodian(deps.as_ref(), &recipient)? {
                    if CUSTODIANS.has(deps.storage, &nft.owner) {
                        return Err(StdError::generic_err(
                            "exit custody before rebinding or listing",
                        )
                        .into());
                    }
                    // CW721 ReceiveNft.sender is the operator who initiated
                    // SendNft; vault/listing entitlement uses that same sender.
                    info.sender.clone()
                } else {
                    recipient
                };
                Some((
                    token_id.clone(),
                    nft.extension,
                    Some(BENEFICIARIES.load(deps.storage, token_id)?),
                    Some(beneficiary),
                ))
            }
        }
        ExecuteMsg::Burn { token_id } => {
            let nft = Collection::default().tokens.load(deps.storage, token_id)?;
            if nft.extension.kind == Kind::Commander {
                None
            } else {
                Some((
                    token_id.clone(),
                    nft.extension,
                    Some(BENEFICIARIES.load(deps.storage, token_id)?),
                    None,
                ))
            }
        }
        _ => None,
    };
    let response = Collection::default().execute(deps.branch(), env, info, msg)?;
    if let Some((id, attributes, old, new)) = effect {
        let kind = skin_kind(&attributes.kind)?;
        if old != new {
            if let Some(wallet) = old {
                change_count(deps.storage, &wallet, kind, &attributes.code, false)?;
            }
            if let Some(wallet) = &new {
                change_count(deps.storage, wallet, kind, &attributes.code, true)?;
            }
        }
        if let Some(wallet) = new {
            BENEFICIARIES.save(deps.storage, &id, &wallet)?;
        } else {
            BENEFICIARIES.remove(deps.storage, &id);
        }
    }
    Ok(response)
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    if let QueryMsg::Extension { msg } = msg {
        return match msg {
            ExtensionQuery::Controls {} => to_json_binary(&Controls {
                owner: OWNER.load(deps.storage)?,
                paused: PAUSED.load(deps.storage)?,
                pause_controller: game_common::pause::controller(deps)?,
            }),
            ExtensionQuery::SkinCounts {
                wallet,
                kind,
                codes,
            } => {
                if codes.len() > 96 || codes.iter().any(|code| code.is_empty() || code.len() > 64) {
                    return Err(StdError::generic_err("invalid bounded skin query"));
                }
                let wallet = deps.api.addr_validate(&wallet)?;
                let kind = skin_kind(&kind)?;
                let counts = codes
                    .into_iter()
                    .map(|code| {
                        let count = SKIN_COUNTS
                            .may_load(deps.storage, (&wallet, kind, code.as_str()))?
                            .unwrap_or(0);
                        Ok(SkinCount { code, count })
                    })
                    .collect::<StdResult<Vec<_>>>()?;
                to_json_binary(&SkinCounts { counts })
            }
            ExtensionQuery::Beneficiary { token_id } => {
                let nft = Collection::default().tokens.load(deps.storage, &token_id)?;
                let beneficiary = if nft.extension.kind == Kind::Commander {
                    nft.owner.clone()
                } else {
                    BENEFICIARIES.load(deps.storage, &token_id)?
                };
                to_json_binary(&Beneficiary {
                    owner: nft.owner,
                    beneficiary,
                })
            }
            ExtensionQuery::Custodian { address } => to_json_binary(&Custodian {
                allowed: is_custodian(deps, &deps.api.addr_validate(&address)?)?,
            }),
        };
    }
    Collection::default().query(deps, env, msg)
}
