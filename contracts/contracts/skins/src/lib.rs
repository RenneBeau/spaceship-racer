use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdError, StdResult, Uint128, WasmMsg,
};
use cw_storage_plus::{Bound, Item, Map};
use racer_collection::{Attributes, Bonus, ExtensionQuery, Kind, SkinCounts};
use racer_fleet::ShipType;

#[cw_serde]
pub struct Entry {
    pub attributes: Attributes,
    pub weight: u32,
    pub token_uri: Option<String>,
}
#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub fleet: String,
    pub collection: String,
    pub randomness: String,
    pub milestone_step: u16,
    pub catalogue: Vec<Entry>,
}
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub fleet: Addr,
    pub collection: Addr,
    pub randomness: Addr,
    pub milestone_step: u16,
    pub catalogue: Vec<Entry>,
    pub next_id: u64,
}
#[cw_serde]
pub enum ExecuteMsg {
    Request {
        ship: ShipType,
    },
    Fulfill {
        id: u64,
    },
    AcceptMinter {},
    Configure {
        milestone_step: Option<u16>,
        catalogue: Option<Vec<Entry>>,
    },
    SetPauseController {
        controller: Option<String>,
    },
}
#[cw_serde]
#[derive(Default)]
pub struct Progress {
    pub committed: u32,
    pub fulfilled: u32,
    pub pending: Option<u64>,
}
#[cw_serde]
pub struct Status {
    pub earned: u32,
    pub available: u32,
    pub progress: Progress,
    pub eligible_codes: Vec<String>,
}
#[cw_serde]
pub struct Claim {
    pub id: u64,
    pub wallet: Addr,
    pub ship: ShipType,
    pub milestone: u32,
    pub catalogue: Vec<Entry>,
    pub token_id: Option<String>,
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Readiness)]
    Preview { id: u64 },
    #[returns(Config)]
    Config {},
    #[returns(Status)]
    Status { wallet: String, ship: ShipType },
    #[returns(Claim)]
    Claim { id: u64 },
    #[returns(Vec<u64>)]
    Pending {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
}
#[cw_serde]
pub enum Readiness {
    WaitingForRandomness,
    DuplicateOwned { code: String },
    Ready { code: String },
    Fulfilled { token_id: String },
}
const CONFIG: Item<Config> = Item::new("config");
const PROGRESS: Map<(&Addr, u8), Progress> = Map::new("progress");
const CLAIMS: Map<u64, Claim> = Map::new("claims");
const PENDING: Map<u64, bool> = Map::new("pending");
fn fail(s: &str) -> StdError {
    StdError::generic_err(s)
}
fn ship_kind(ship: &ShipType) -> (u8, Kind) {
    match ship {
        ShipType::Racer => (0, Kind::RacerSkin),
        ShipType::Tank => (1, Kind::TankSkin),
        ShipType::Fighter => (2, Kind::FighterSkin),
    }
}
fn call(addr: &Addr, msg: Binary) -> WasmMsg {
    WasmMsg::Execute {
        contract_addr: addr.to_string(),
        msg,
        funds: vec![],
    }
}

fn validate(step: u16, entries: &[Entry]) -> StdResult<()> {
    if step == 0 || step > 1000 || entries.is_empty() || entries.len() > 96 {
        return Err(fail("invalid skin configuration"));
    }
    let mut counts = [0u16; 3];
    for (i, e) in entries.iter().enumerate() {
        let kind = racer_collection::skin_kind(&e.attributes.kind)?;
        counts[usize::from(kind - 1)] += 1;
        if counts[usize::from(kind - 1)] > 32
            || e.weight == 0
            || e.attributes.code.is_empty()
            || e.attributes.code.len() > 64
            || e.attributes.bonus != Bonus::Cosmetic
            || e.attributes.bonus_bps != 0
            || e.token_uri.as_ref().map(|u| u.len() > 512).unwrap_or(false)
            || entries[..i].iter().any(|other| {
                other.attributes.kind == e.attributes.kind
                    && other.attributes.code == e.attributes.code
            })
        {
            return Err(fail("invalid skin catalogue"));
        }
    }
    Ok(())
}

fn owned(
    deps: Deps,
    config: &Config,
    wallet: &Addr,
    kind: Kind,
    codes: Vec<String>,
) -> StdResult<SkinCounts> {
    deps.querier.query_wasm_smart(
        &config.collection,
        &racer_collection::QueryMsg::Extension {
            msg: ExtensionQuery::SkinCounts {
                wallet: wallet.to_string(),
                kind,
                codes,
            },
        },
    )
}
fn eligible(deps: Deps, config: &Config, wallet: &Addr, ship: &ShipType) -> StdResult<Vec<Entry>> {
    let (_, kind) = ship_kind(ship);
    let candidates: Vec<_> = config
        .catalogue
        .iter()
        .filter(|e| e.attributes.kind == kind)
        .cloned()
        .collect();
    let counts = owned(
        deps,
        config,
        wallet,
        kind,
        candidates
            .iter()
            .map(|e| e.attributes.code.clone())
            .collect(),
    )?;
    if counts.counts.len() != candidates.len() {
        return Err(fail("invalid ownership response"));
    }
    let mut result = vec![];
    for (entry, count) in candidates.into_iter().zip(counts.counts) {
        if entry.attributes.code != count.code {
            return Err(fail("invalid ownership response"));
        }
        if count.count == 0 {
            result.push(entry);
        }
    }
    Ok(result)
}
fn earned(deps: Deps, config: &Config, wallet: &Addr, ship: &ShipType) -> StdResult<u32> {
    let fleet: racer_fleet::Fleet = deps.querier.query_wasm_smart(
        &config.fleet,
        &racer_fleet::QueryMsg::Fleet {
            wallet: wallet.to_string(),
        },
    )?;
    Ok(u32::from(
        fleet.next[usize::from(ship_kind(ship).0)] / config.milestone_step,
    ))
}

fn select(env: &Env, claim: &Claim, seed: &[u8]) -> StdResult<Entry> {
    let total = claim.catalogue.iter().map(|e| u128::from(e.weight)).sum();
    let mut roll = game_common::random::draw(
        seed,
        &[
            env.block.chain_id.as_bytes(),
            env.contract.address.as_bytes(),
            b"milestone-skin",
            &claim.id.to_be_bytes(),
        ],
        Uint128::new(total),
    )?
    .u128();
    for entry in &claim.catalogue {
        if roll < u128::from(entry.weight) {
            return Ok(entry.clone());
        }
        roll -= u128::from(entry.weight);
    }
    Err(fail("skin draw failed"))
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    validate(msg.milestone_step, &msg.catalogue)?;
    CONFIG.save(
        deps.storage,
        &Config {
            owner: deps.api.addr_validate(&msg.owner)?,
            fleet: deps.api.addr_validate(&msg.fleet)?,
            collection: deps.api.addr_validate(&msg.collection)?,
            randomness: deps.api.addr_validate(&msg.randomness)?,
            milestone_step: msg.milestone_step,
            catalogue: msg.catalogue,
            next_id: 1,
        },
    )?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/skins",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_skins"))
}

#[entry_point]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    let mut config = CONFIG.load(deps.storage)?;
    match msg {
        ExecuteMsg::SetPauseController { controller } => {
            return game_common::pause::set_controller(deps, &info, &config.owner, controller)
        }
        ExecuteMsg::Configure {
            milestone_step,
            catalogue,
        } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            if let Some(step) = milestone_step {
                config.milestone_step = step;
            }
            if let Some(entries) = catalogue {
                config.catalogue = entries;
            }
            validate(config.milestone_step, &config.catalogue)?;
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new().add_attribute("action", "configure_skins"));
        }
        _ => {}
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    match msg {
        ExecuteMsg::AcceptMinter {} => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            Ok(Response::new().add_message(call(
                &config.collection,
                to_json_binary(&racer_collection::ExecuteMsg::UpdateOwnership(
                    cw_ownable::Action::AcceptOwnership,
                ))?,
            )))
        }
        ExecuteMsg::Request { ship } => {
            let (index, _) = ship_kind(&ship);
            let mut progress = PROGRESS
                .may_load(deps.storage, (&info.sender, index))?
                .unwrap_or_default();
            if progress.pending.is_some() {
                return Err(fail("skin claim already pending"));
            }
            let earned = earned(deps.as_ref(), &config, &info.sender, &ship)?;
            if progress.committed >= earned {
                return Err(fail("no unclaimed ship milestone"));
            }
            let catalogue = eligible(deps.as_ref(), &config, &info.sender, &ship)?;
            if catalogue.is_empty() {
                return Err(fail("all available skins owned; milestone credit retained"));
            }
            let id = config.next_id;
            config.next_id = id.checked_add(1).ok_or_else(|| fail("claim ID overflow"))?;
            progress.committed = progress
                .committed
                .checked_add(1)
                .ok_or_else(|| fail("claim count overflow"))?;
            progress.pending = Some(id);
            CLAIMS.save(
                deps.storage,
                id,
                &Claim {
                    id,
                    wallet: info.sender.clone(),
                    ship,
                    milestone: progress.committed * u32::from(config.milestone_step),
                    catalogue,
                    token_id: None,
                },
            )?;
            PENDING.save(deps.storage, id, &true)?;
            PROGRESS.save(deps.storage, (&info.sender, index), &progress)?;
            CONFIG.save(deps.storage, &config)?;
            Ok(Response::new()
                .add_message(call(
                    &config.randomness,
                    to_json_binary(&racer_randomness::ExecuteMsg::Request { id })?,
                ))
                .add_attribute("action", "request_milestone_skin")
                .add_attribute("claim_id", id.to_string())
                .add_attribute("wallet", info.sender))
        }
        ExecuteMsg::Fulfill { id } => {
            let mut claim = CLAIMS.load(deps.storage, id)?;
            if claim.token_id.is_some() {
                return Err(fail("skin claim already fulfilled"));
            }
            let request: racer_randomness::Request = deps.querier.query_wasm_smart(
                &config.randomness,
                &racer_randomness::QueryMsg::Request {
                    consumer: env.contract.address.to_string(),
                    id,
                },
            )?;
            let seed = request
                .randomness
                .ok_or_else(|| fail("randomness pending"))?;
            let picked = select(&env, &claim, seed.as_slice())?;
            let counts = owned(
                deps.as_ref(),
                &config,
                &claim.wallet,
                picked.attributes.kind.clone(),
                vec![picked.attributes.code.clone()],
            )?;
            if counts.counts.len() != 1
                || counts.counts[0].code != picked.attributes.code
                || counts.counts[0].count != 0
            {
                return Err(fail("selected skin currently owned; same claim must wait"));
            }
            let index = ship_kind(&claim.ship).0;
            let mut progress = PROGRESS.load(deps.storage, (&claim.wallet, index))?;
            if progress.pending != Some(id) {
                return Err(fail("claim reservation mismatch"));
            }
            let token_id = format!("skin:{}:{id}", env.contract.address);
            claim.token_id = Some(token_id.clone());
            claim.catalogue.clear();
            progress.fulfilled = progress
                .fulfilled
                .checked_add(1)
                .ok_or_else(|| fail("claim count overflow"))?;
            progress.pending = None;
            PROGRESS.save(deps.storage, (&claim.wallet, index), &progress)?;
            CLAIMS.save(deps.storage, id, &claim)?;
            PENDING.remove(deps.storage, id);
            Ok(Response::new()
                .add_message(call(
                    &config.collection,
                    to_json_binary(&racer_collection::ExecuteMsg::Mint {
                        token_id: token_id.clone(),
                        owner: claim.wallet.to_string(),
                        token_uri: picked.token_uri,
                        extension: picked.attributes,
                    })?,
                ))
                .add_attribute("action", "fulfill_milestone_skin")
                .add_attribute("claim_id", id.to_string())
                .add_attribute("token_id", token_id)
                .add_attribute("wallet", claim.wallet))
        }
        _ => unreachable!(),
    }
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Preview { id } => {
            let config = CONFIG.load(deps.storage)?;
            let claim = CLAIMS.load(deps.storage, id)?;
            if let Some(token_id) = claim.token_id.clone() {
                return to_json_binary(&Readiness::Fulfilled { token_id });
            }
            let request: racer_randomness::Request = deps.querier.query_wasm_smart(
                &config.randomness,
                &racer_randomness::QueryMsg::Request {
                    consumer: env.contract.address.to_string(),
                    id,
                },
            )?;
            let Some(seed) = request.randomness else {
                return to_json_binary(&Readiness::WaitingForRandomness);
            };
            let picked = select(&env, &claim, seed.as_slice())?;
            let counts = owned(
                deps,
                &config,
                &claim.wallet,
                picked.attributes.kind,
                vec![picked.attributes.code.clone()],
            )?;
            if counts.counts.len() != 1 || counts.counts[0].code != picked.attributes.code {
                return Err(fail("invalid ownership response"));
            }
            to_json_binary(&if counts.counts[0].count == 0 {
                Readiness::Ready {
                    code: picked.attributes.code,
                }
            } else {
                Readiness::DuplicateOwned {
                    code: picked.attributes.code,
                }
            })
        }
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Claim { id } => to_json_binary(&CLAIMS.load(deps.storage, id)?),
        QueryMsg::Pending { start_after, limit } => to_json_binary(
            &PENDING
                .keys(
                    deps.storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(20).clamp(1, 100) as usize)
                .collect::<StdResult<Vec<_>>>()?,
        ),
        QueryMsg::Status { wallet, ship } => {
            let config = CONFIG.load(deps.storage)?;
            let wallet = deps.api.addr_validate(&wallet)?;
            let progress = PROGRESS
                .may_load(deps.storage, (&wallet, ship_kind(&ship).0))?
                .unwrap_or_default();
            let earned = earned(deps, &config, &wallet, &ship)?;
            let eligible_codes = eligible(deps, &config, &wallet, &ship)?
                .into_iter()
                .map(|e| e.attributes.code)
                .collect();
            to_json_binary(&Status {
                earned,
                available: earned.saturating_sub(progress.committed),
                progress,
                eligible_codes,
            })
        }
    }
}
