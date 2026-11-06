use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdError, StdResult, WasmMsg,
};
use cw721::{Cw721ExecuteMsg, Cw721QueryMsg, Cw721ReceiveMsg, NftInfoResponse, OwnerOfResponse};
use cw_storage_plus::{Bound, Item, Map};
use racer_collection::{Attributes, Kind};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub engine: String,
    pub collections: Vec<String>,
}
#[cw_serde]
pub enum Slot {
    Commander,
    RacerSkin,
    TankSkin,
    FighterSkin,
}
impl Slot {
    fn id(&self) -> u8 {
        match self {
            Self::Commander => 0,
            Self::RacerSkin => 1,
            Self::TankSkin => 2,
            Self::FighterSkin => 3,
        }
    }
    fn kind(&self) -> Kind {
        match self {
            Self::Commander => Kind::Commander,
            Self::RacerSkin => Kind::RacerSkin,
            Self::TankSkin => Kind::TankSkin,
            Self::FighterSkin => Kind::FighterSkin,
        }
    }
}
#[cw_serde]
pub struct BindMsg {
    pub slot: Slot,
}
#[cw_serde]
pub enum ExecuteMsg {
    SetPauseController {
        controller: Option<String>,
    },
    BeginBoundary {},
    EndBoundary {},
    ReceiveNft(Cw721ReceiveMsg),
    Unbind {
        slot: Slot,
    },
    Process {
        wallets: Vec<String>,
    },
    Configure {
        engine: Option<String>,
        paused: Option<bool>,
        collection: Option<String>,
        allowed: Option<bool>,
    },
}
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub engine: Addr,
    pub paused: bool,
    pub boundary: bool,
}
#[cw_serde]
pub struct Equipment {
    pub collection: Addr,
    pub token_id: String,
    pub attributes: Attributes,
}
#[cw_serde]
pub enum Change {
    Bind(Equipment),
    Unbind,
}
#[cw_serde]
#[derive(Default)]
pub struct Position {
    pub active: Option<Equipment>,
    pub pending: Option<Change>,
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Position)]
    Position { wallet: String, slot: Slot },
    #[returns(Vec<String>)]
    PendingWallets {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    #[returns(Config)]
    Config {},
}
const CONFIG: Item<Config> = Item::new("config");
const COLLECTIONS: Map<&Addr, bool> = Map::new("collections");
const POSITIONS: Map<(&Addr, u8), Position> = Map::new("positions");
const CUSTODY: Map<(&Addr, &str), Addr> = Map::new("custody");
const PENDING: Map<&Addr, bool> = Map::new("pending");
fn fail(message: &str) -> StdError {
    StdError::generic_err(message)
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() || msg.collections.len() > 20 {
        return Err(fail("invalid instantiate"));
    }
    CONFIG.save(
        deps.storage,
        &Config {
            owner: deps.api.addr_validate(&msg.owner)?,
            engine: deps.api.addr_validate(&msg.engine)?,
            paused: false,
            boundary: false,
        },
    )?;
    for collection in msg.collections {
        COLLECTIONS.save(deps.storage, &deps.api.addr_validate(&collection)?, &true)?;
    }
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/equipment",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_equipment"))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    let mut config = CONFIG.load(deps.storage)?;
    if let ExecuteMsg::SetPauseController { controller } = msg {
        return game_common::pause::set_controller(deps, &info, &config.owner, controller);
    }
    if let ExecuteMsg::Configure {
        engine,
        paused,
        collection,
        allowed,
    } = msg
    {
        if info.sender != config.owner {
            return Err(fail("unauthorized"));
        }
        if let Some(engine) = engine {
            config.engine = deps.api.addr_validate(&engine)?;
        }
        if let Some(paused) = paused {
            config.paused = paused;
        }
        if collection.is_some() != allowed.is_some() {
            return Err(fail("collection and allowed required together"));
        }
        if let (Some(collection), Some(allowed)) = (collection, allowed) {
            COLLECTIONS.save(
                deps.storage,
                &deps.api.addr_validate(&collection)?,
                &allowed,
            )?;
        }
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "configure_equipment"));
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    if config.paused {
        return Err(fail("paused"));
    }
    match msg {
        ExecuteMsg::BeginBoundary {} => {
            if info.sender != config.engine || config.boundary {
                return Err(fail("unauthorized or boundary already open"));
            }
            config.boundary = true;
            CONFIG.save(deps.storage, &config)?;
            Ok(Response::new().add_attribute("action", "begin_equipment_boundary"))
        }
        ExecuteMsg::EndBoundary {} => {
            if info.sender != config.engine || !config.boundary {
                return Err(fail("unauthorized or no boundary"));
            }
            if PENDING
                .keys(deps.storage, None, None, Order::Ascending)
                .next()
                .transpose()?
                .is_some()
            {
                return Err(fail("pending equipment remains"));
            }
            config.boundary = false;
            CONFIG.save(deps.storage, &config)?;
            Ok(Response::new().add_attribute("action", "end_equipment_boundary"))
        }
        ExecuteMsg::ReceiveNft(receive) => {
            if config.boundary {
                return Err(fail("equipment boundary in progress"));
            }
            if !COLLECTIONS
                .may_load(deps.storage, &info.sender)?
                .unwrap_or(false)
            {
                return Err(fail("unsupported collection"));
            }
            let BindMsg { slot } = from_json(receive.msg)?;
            let wallet = deps.api.addr_validate(&receive.sender)?;
            if CUSTODY.has(deps.storage, (&info.sender, &receive.token_id)) {
                return Err(fail("already bound"));
            }
            let owner: OwnerOfResponse = deps.querier.query_wasm_smart(
                &info.sender,
                &Cw721QueryMsg::OwnerOf {
                    token_id: receive.token_id.clone(),
                    include_expired: Some(false),
                },
            )?;
            if owner.owner != env.contract.address {
                return Err(fail("NFT not in custody"));
            }
            let nft: NftInfoResponse<Attributes> = deps.querier.query_wasm_smart(
                &info.sender,
                &Cw721QueryMsg::NftInfo {
                    token_id: receive.token_id.clone(),
                },
            )?;
            if nft.extension.kind != slot.kind() {
                return Err(fail("wrong equipment slot"));
            }
            if nft.extension.kind != Kind::Commander {
                let custody: racer_collection::Custodian = deps.querier.query_wasm_smart(
                    &info.sender,
                    &racer_collection::QueryMsg::Extension {
                        msg: racer_collection::ExtensionQuery::Custodian {
                            address: env.contract.address.to_string(),
                        },
                    },
                )?;
                if !custody.allowed {
                    return Err(fail("skin custody not registered"));
                }
            }
            let mut position = POSITIONS
                .may_load(deps.storage, (&wallet, slot.id()))?
                .unwrap_or_default();
            if position.pending.is_some() {
                return Err(fail("change already queued"));
            }
            CUSTODY.save(deps.storage, (&info.sender, &receive.token_id), &wallet)?;
            position.pending = Some(Change::Bind(Equipment {
                collection: info.sender,
                token_id: receive.token_id,
                attributes: nft.extension,
            }));
            POSITIONS.save(deps.storage, (&wallet, slot.id()), &position)?;
            PENDING.save(deps.storage, &wallet, &true)?;
            Ok(Response::new()
                .add_attribute("action", "queue_bind")
                .add_attribute("wallet", wallet))
        }
        ExecuteMsg::Unbind { slot } => {
            if config.boundary {
                return Err(fail("equipment boundary in progress"));
            }
            let mut position = POSITIONS
                .may_load(deps.storage, (&info.sender, slot.id()))?
                .unwrap_or_default();
            if position.active.is_none() || position.pending.is_some() {
                return Err(fail("no active NFT or change already queued"));
            }
            position.pending = Some(Change::Unbind);
            POSITIONS.save(deps.storage, (&info.sender, slot.id()), &position)?;
            PENDING.save(deps.storage, &info.sender, &true)?;
            Ok(Response::new()
                .add_attribute("action", "queue_unbind")
                .add_attribute("wallet", info.sender))
        }
        ExecuteMsg::Process { wallets } => {
            if info.sender != config.engine || !config.boundary {
                return Err(fail("unauthorized"));
            }
            if wallets.is_empty() || wallets.len() > 25 {
                return Err(fail("invalid batch size"));
            }
            let mut response =
                Response::new().add_attribute("action", "process_equipment_boundary");
            for wallet in wallets {
                let wallet = deps.api.addr_validate(&wallet)?;
                for slot in 0..4 {
                    let mut position = POSITIONS
                        .may_load(deps.storage, (&wallet, slot))?
                        .unwrap_or_default();
                    if let Some(change) = position.pending.take() {
                        if let Some(previous) = position.active.take() {
                            CUSTODY
                                .remove(deps.storage, (&previous.collection, &previous.token_id));
                            response = response.add_message(WasmMsg::Execute {
                                contract_addr: previous.collection.to_string(),
                                msg: to_json_binary(&Cw721ExecuteMsg::TransferNft {
                                    recipient: wallet.to_string(),
                                    token_id: previous.token_id,
                                })?,
                                funds: vec![],
                            });
                        }
                        if let Change::Bind(equipment) = change {
                            position.active = Some(equipment);
                        }
                        POSITIONS.save(deps.storage, (&wallet, slot), &position)?;
                    }
                }
                PENDING.remove(deps.storage, &wallet);
                response = response.add_attribute("wallet", wallet);
            }
            Ok(response)
        }
        ExecuteMsg::Configure { .. } => unreachable!(),
        ExecuteMsg::SetPauseController { .. } => unreachable!(),
    }
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Position { wallet, slot } => {
            let addr = deps.api.addr_validate(&wallet)?;
            to_json_binary(
                &POSITIONS
                    .may_load(deps.storage, (&addr, slot.id()))?
                    .unwrap_or_default(),
            )
        }
        QueryMsg::PendingWallets { start_after, limit } => {
            let start = start_after
                .map(|a| deps.api.addr_validate(&a))
                .transpose()?;
            let wallets: StdResult<Vec<_>> = PENDING
                .keys(
                    deps.storage,
                    start.as_ref().map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(25).clamp(1, 100) as usize)
                .map(|addr| addr.map(|a| a.to_string()))
                .collect();
            to_json_binary(&wallets?)
        }
    }
}
