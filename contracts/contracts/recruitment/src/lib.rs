use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdError, StdResult, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw721::{Cw721ExecuteMsg, Cw721QueryMsg, NftInfoResponse, OwnerOfResponse};
use cw_storage_plus::{Bound, Item, Map};
use racer_collection::{Attributes, Kind, Rarity};

#[cw_serde]
pub struct Entry {
    pub attributes: Attributes,
    pub weight: u32,
    pub token_uri: Option<String>,
}
#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub srt: String,
    pub collection: String,
    pub randomness: String,
    pub treasury: String,
    pub pull_price: Uint128,
    pub catalogue: Vec<Entry>,
}
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub srt: Addr,
    pub collection: Addr,
    pub randomness: Addr,
    pub treasury: Addr,
    pub pull_price: Uint128,
    pub catalogue: Vec<Entry>,
    pub next_id: u64,
    pub escrow_srt: Uint128,
}
#[cw_serde]
pub struct RecruitMsg {
    pub count: u8,
}
#[cw_serde]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    Fuse {
        token_ids: Vec<String>,
    },
    Fulfill {
        id: u64,
    },
    AcceptMinter {},
    Configure {
        treasury: Option<String>,
        pull_price: Option<Uint128>,
        catalogue: Option<Vec<Entry>>,
    },
    SetPauseController {
        controller: Option<String>,
    },
}
#[cw_serde]
pub enum JobKind {
    Recruit { count: u8 },
    Fusion { token_ids: Vec<String> },
}
#[cw_serde]
pub struct Job {
    pub id: u64,
    pub wallet: Addr,
    pub kind: JobKind,
    pub paid: Uint128,
    pub treasury: Addr,
    pub catalogue: Vec<Entry>,
    pub completed: bool,
    pub minted: Vec<String>,
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Vec<u64>)]
    Pending {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    #[returns(Config)]
    Config {},
    #[returns(Job)]
    Job { id: u64 },
    #[returns(Vec<Job>)]
    Jobs {
        wallet: String,
        start_after: Option<u64>,
        limit: Option<u32>,
    },
}
const CONFIG: Item<Config> = Item::new("config");
const JOBS: Map<u64, Job> = Map::new("jobs");
const WALLET_JOBS: Map<(&Addr, u64), bool> = Map::new("wallet_jobs");
const LOCKED: Map<&str, u64> = Map::new("locked");
const PENDING: Map<u64, bool> = Map::new("pending_jobs");
fn fail(msg: &str) -> StdError {
    StdError::generic_err(msg)
}
fn call(addr: &Addr, msg: Binary) -> WasmMsg {
    WasmMsg::Execute {
        contract_addr: addr.to_string(),
        msg,
        funds: vec![],
    }
}
fn rarity(r: &Rarity) -> u8 {
    match r {
        Rarity::Common => 0,
        Rarity::Rare => 1,
        Rarity::Epic => 2,
        Rarity::Legendary => 3,
    }
}

fn validate(catalogue: &[Entry], price: Uint128) -> StdResult<()> {
    if catalogue.is_empty()
        || catalogue.len() > 64
        || price.is_zero()
        || price.u128() > 1_000_000_000_000_000
    {
        return Err(fail("invalid recruitment configuration"));
    }
    let mut present = [false; 4];
    for (index, e) in catalogue.iter().enumerate() {
        let a = &e.attributes;
        if a.kind != Kind::Commander
            || a.code.is_empty()
            || a.code.len() > 64
            || a.bonus_bps > 10000
            || e.weight == 0
            || e.token_uri.as_ref().map(|s| s.len() > 512).unwrap_or(false)
            || catalogue[..index]
                .iter()
                .any(|other| other.attributes.code == a.code)
        {
            return Err(fail("invalid commander catalogue"));
        }
        present[rarity(&a.rarity) as usize] = true;
    }
    if present.iter().any(|p| !p) {
        return Err(fail("all commander rarities required"));
    }
    Ok(())
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    validate(&msg.catalogue, msg.pull_price)?;
    CONFIG.save(
        deps.storage,
        &Config {
            owner: deps.api.addr_validate(&msg.owner)?,
            srt: deps.api.addr_validate(&msg.srt)?,
            collection: deps.api.addr_validate(&msg.collection)?,
            randomness: deps.api.addr_validate(&msg.randomness)?,
            treasury: deps.api.addr_validate(&msg.treasury)?,
            pull_price: msg.pull_price,
            catalogue: msg.catalogue,
            next_id: 1,
            escrow_srt: Uint128::zero(),
        },
    )?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/recruitment",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_recruitment"))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
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
            treasury,
            pull_price,
            catalogue,
        } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            if let Some(addr) = treasury {
                config.treasury = deps.api.addr_validate(&addr)?;
            }
            if let Some(price) = pull_price {
                config.pull_price = price;
            }
            if let Some(entries) = catalogue {
                config.catalogue = entries;
            }
            validate(&config.catalogue, config.pull_price)?;
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new().add_attribute("action", "configure_recruitment"));
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
        ExecuteMsg::Receive(receive) => {
            if info.sender != config.srt {
                return Err(fail("recruitment requires SRT"));
            }
            let hook: RecruitMsg = from_json(receive.msg)?;
            if hook.count != 1 && hook.count != 10 {
                return Err(fail("recruit count must be 1 or 10"));
            }
            let paid = config.pull_price.checked_mul(Uint128::from(hook.count))?;
            if receive.amount != paid {
                return Err(fail("incorrect recruitment payment"));
            }
            let wallet = deps.api.addr_validate(&receive.sender)?;
            let job = Job {
                id: config.next_id,
                wallet,
                kind: JobKind::Recruit { count: hook.count },
                paid,
                treasury: config.treasury.clone(),
                catalogue: config.catalogue.clone(),
                completed: false,
                minted: vec![],
            };
            config.escrow_srt = config.escrow_srt.checked_add(paid)?;
            create_job(deps, &mut config, job)
        }
        ExecuteMsg::Fuse { token_ids } => {
            if token_ids.len() != 5
                || token_ids
                    .iter()
                    .enumerate()
                    .any(|(i, id)| id.len() > 256 || token_ids[..i].contains(id))
            {
                return Err(fail("exactly five unique commanders required"));
            }
            let mut input_rarity = None;
            for id in &token_ids {
                if LOCKED.has(deps.storage, id) {
                    return Err(fail("commander already committed"));
                }
                let owner: OwnerOfResponse = deps.querier.query_wasm_smart(
                    &config.collection,
                    &Cw721QueryMsg::OwnerOf {
                        token_id: id.clone(),
                        include_expired: None,
                    },
                )?;
                if owner.owner != info.sender {
                    return Err(fail("fusion requires wallet-owned unbound NFTs"));
                }
                let nft: NftInfoResponse<Attributes> = deps.querier.query_wasm_smart(
                    &config.collection,
                    &Cw721QueryMsg::NftInfo {
                        token_id: id.clone(),
                    },
                )?;
                let r = rarity(&nft.extension.rarity);
                if nft.extension.kind != Kind::Commander
                    || r == 3
                    || input_rarity.map(|old| old != r).unwrap_or(false)
                {
                    return Err(fail("fusion requires same non-legendary rarity"));
                }
                input_rarity = Some(r);
            }
            let next = input_rarity.ok_or_else(|| fail("missing fusion rarity"))? + 1;
            let catalogue: Vec<_> = config
                .catalogue
                .iter()
                .filter(|e| rarity(&e.attributes.rarity) == next)
                .cloned()
                .collect();
            if catalogue.is_empty() {
                return Err(fail("fusion output pool empty"));
            }
            let job = Job {
                id: config.next_id,
                wallet: info.sender.clone(),
                kind: JobKind::Fusion {
                    token_ids: token_ids.clone(),
                },
                paid: Uint128::zero(),
                treasury: config.treasury.clone(),
                catalogue,
                completed: false,
                minted: vec![],
            };
            for id in &token_ids {
                LOCKED.save(deps.storage, id, &job.id)?;
            }
            let mut response = create_job(deps, &mut config, job)?;
            for id in token_ids {
                response = response.add_message(call(
                    &config.collection,
                    to_json_binary(&Cw721ExecuteMsg::TransferNft {
                        recipient: env.contract.address.to_string(),
                        token_id: id,
                    })?,
                ));
            }
            Ok(response)
        }
        ExecuteMsg::Fulfill { id } => {
            let mut job = JOBS.load(deps.storage, id)?;
            if job.completed {
                return Err(fail("job already fulfilled"));
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
            let count = match &job.kind {
                JobKind::Recruit { count } => *count,
                JobKind::Fusion { .. } => 1,
            };
            let mut response = Response::new()
                .add_attribute("action", "fulfill_commanders")
                .add_attribute("job_id", id.to_string())
                .add_attribute("wallet", job.wallet.to_string());
            if let JobKind::Fusion { token_ids } = &job.kind {
                for token_id in token_ids {
                    LOCKED.remove(deps.storage, token_id);
                    response = response.add_message(call(
                        &config.collection,
                        to_json_binary(&Cw721ExecuteMsg::Burn {
                            token_id: token_id.clone(),
                        })?,
                    ));
                }
            }
            for ordinal in 0..count {
                let guarantee = matches!(job.kind, JobKind::Recruit { count: 10 }) && ordinal == 0;
                let pool: Vec<_> = job
                    .catalogue
                    .iter()
                    .filter(|e| !guarantee || rarity(&e.attributes.rarity) >= 2)
                    .collect();
                let total = pool.iter().map(|e| u128::from(e.weight)).sum::<u128>();
                let mut roll = game_common::random::draw(
                    seed.as_slice(),
                    &[
                        env.block.chain_id.as_bytes(),
                        env.contract.address.as_bytes(),
                        b"commander",
                        &id.to_be_bytes(),
                        &[ordinal],
                    ],
                    Uint128::new(total),
                )?
                .u128();
                let mut chosen = None;
                for entry in pool {
                    if roll < u128::from(entry.weight) {
                        chosen = Some(entry);
                        break;
                    }
                    roll -= u128::from(entry.weight);
                }
                let entry = chosen.ok_or_else(|| fail("commander draw failed"))?;
                let token_id = format!("cmd:{}:{id}:{ordinal}", env.contract.address);
                response = response.add_message(call(
                    &config.collection,
                    to_json_binary(&racer_collection::ExecuteMsg::Mint {
                        token_id: token_id.clone(),
                        owner: job.wallet.to_string(),
                        token_uri: entry.token_uri.clone(),
                        extension: entry.attributes.clone(),
                    })?,
                ));
                job.minted.push(token_id);
            }
            if !job.paid.is_zero() {
                response = response.add_message(call(
                    &config.srt,
                    to_json_binary(&Cw20ExecuteMsg::Transfer {
                        recipient: job.treasury.to_string(),
                        amount: job.paid,
                    })?,
                ));
                config.escrow_srt = config.escrow_srt.checked_sub(job.paid)?;
            }
            job.completed = true;
            PENDING.remove(deps.storage, id);
            job.catalogue.clear();
            JOBS.save(deps.storage, id, &job)?;
            CONFIG.save(deps.storage, &config)?;
            Ok(response)
        }
        _ => unreachable!(),
    }
}

fn create_job(deps: DepsMut, config: &mut Config, job: Job) -> StdResult<Response> {
    config.next_id = config
        .next_id
        .checked_add(1)
        .ok_or_else(|| fail("job ID overflow"))?;
    JOBS.save(deps.storage, job.id, &job)?;
    PENDING.save(deps.storage, job.id, &true)?;
    WALLET_JOBS.save(deps.storage, (&job.wallet, job.id), &true)?;
    CONFIG.save(deps.storage, config)?;
    Ok(Response::new()
        .add_message(call(
            &config.randomness,
            to_json_binary(&racer_randomness::ExecuteMsg::Request { id: job.id })?,
        ))
        .add_attribute("action", "request_commanders")
        .add_attribute("job_id", job.id.to_string())
        .add_attribute("wallet", job.wallet))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
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
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Job { id } => to_json_binary(&JOBS.load(deps.storage, id)?),
        QueryMsg::Jobs {
            wallet,
            start_after,
            limit,
        } => {
            let wallet = deps.api.addr_validate(&wallet)?;
            let jobs = WALLET_JOBS
                .prefix(&wallet)
                .keys(
                    deps.storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(20).clamp(1, 50) as usize)
                .map(|id| JOBS.load(deps.storage, id?))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&jobs)
        }
    }
}
