use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response, StdError,
    StdResult,
};
use cw_storage_plus::{Bound, Item, Map};
#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub engine: String,
    pub fleet: String,
    pub current_cycle: u64,
    pub restore_from: Option<String>,
}
#[cw_serde]
pub struct Config {
    pub restored_from: Option<Addr>,
    pub restoration: Option<Restoration>,
    pub owner: Addr,
    pub engine: Addr,
    pub fleet: Addr,
    pub current_cycle: u64,
    pub next_id: u64,
    pub min_create_tiers: u32,
    pub max_members: u16,
    pub leader_premium_bps: u16,
}
#[cw_serde]
pub struct Restoration {
    pub source: Addr,
    pub after: Option<u64>,
}
#[cw_serde]
#[derive(Default)]
pub struct Member {
    pub active: Option<u64>,
    pub next: Option<u64>,
    pub effective_cycle: u64,
    pub forced: bool,
}
impl Member {
    fn normalize(&mut self, cycle: u64) {
        if self.effective_cycle <= cycle {
            self.active = self.next;
            self.forced = false;
        }
    }
}
#[cw_serde]
pub struct Clan {
    pub id: u64,
    pub name: String,
    pub code: String,
    pub members: Vec<Addr>,
    pub next_members: Vec<Addr>,
    pub leader: Addr,
    pub next_leader: Addr,
    pub effective_cycle: u64,
    pub disband_at: Option<u64>,
}
impl Clan {
    fn normalize(&mut self, cycle: u64) {
        if self.effective_cycle <= cycle {
            self.members = self.next_members.clone();
            self.leader = self.next_leader.clone();
        }
    }
}
#[cw_serde]
pub enum ExecuteMsg {
    RestoreMembers {
        wallets: Vec<String>,
    },
    SetEngine {
        engine: String,
    },
    Restore {
        limit: u32,
    },
    Create {
        name: String,
        code: String,
    },
    Join {
        code: String,
    },
    Leave {},
    CancelChange {},
    TransferLead {
        clan_id: u64,
        wallet: String,
    },
    Kick {
        clan_id: u64,
        wallet: String,
    },
    Dismantle {
        clan_id: u64,
    },
    BeginCycle {
        cycle: u64,
    },
    Configure {
        min_create_tiers: Option<u32>,
        max_members: Option<u16>,
        leader_premium_bps: Option<u16>,
    },
    SetPauseController {
        controller: Option<String>,
    },
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},
    #[returns(Member)]
    Member { wallet: String },
    #[returns(Clan)]
    Clan { id: u64 },
    #[returns(Vec<Clan>)]
    Clans {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
}
const CONFIG: Item<Config> = Item::new("config");
const MEMBERS: Map<&Addr, Member> = Map::new("members");
const CLANS: Map<u64, Clan> = Map::new("clans");
const CODES: Map<&str, u64> = Map::new("codes");
const NAMES: Map<&str, u64> = Map::new("names");
fn fail(s: &str) -> StdError {
    StdError::generic_err(s)
}
fn next_cycle(c: &Config) -> StdResult<u64> {
    c.current_cycle
        .checked_add(1)
        .ok_or_else(|| fail("cycle overflow"))
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
    CONFIG.save(
        deps.storage,
        &Config {
            restored_from: msg
                .restore_from
                .as_ref()
                .map(|source| deps.api.addr_validate(source))
                .transpose()?,
            restoration: msg
                .restore_from
                .as_ref()
                .map(|source| {
                    deps.api.addr_validate(source).map(|source| Restoration {
                        source,
                        after: None,
                    })
                })
                .transpose()?,
            owner: deps.api.addr_validate(&msg.owner)?,
            engine: deps.api.addr_validate(&msg.engine)?,
            fleet: deps.api.addr_validate(&msg.fleet)?,
            current_cycle: msg.current_cycle,
            next_id: 1,
            min_create_tiers: 100,
            max_members: 10,
            leader_premium_bps: 500,
        },
    )?;
    if let Some(source) = msg.restore_from {
        let prior: Config = deps
            .querier
            .query_wasm_smart(&source, &QueryMsg::Config {})?;
        let mut config = CONFIG.load(deps.storage)?;
        if prior.owner != info.sender
            || prior.owner != config.owner
            || prior.fleet != config.fleet
            || prior.current_cycle != config.current_cycle
        {
            return Err(fail("restoration wiring or owner mismatch"));
        }
        config.next_id = prior.next_id;
        config.min_create_tiers = prior.min_create_tiers;
        config.max_members = prior.max_members;
        config.leader_premium_bps = prior.leader_premium_bps;
        CONFIG.save(deps.storage, &config)?;
    }
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/clans",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_clans"))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(
    mut deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    let mut config = CONFIG.load(deps.storage)?;
    match msg {
        ExecuteMsg::RestoreMembers { wallets } => {
            if info.sender != config.owner
                || config.engine != config.owner
                || config.restoration.is_some()
                || wallets.len() > 20
            {
                return Err(fail("member restoration is locked"));
            }
            let source = config
                .restored_from
                .as_ref()
                .ok_or_else(|| fail("missing restoration source"))?;
            for wallet in wallets {
                let wallet = deps.api.addr_validate(&wallet)?;
                let member: Member = deps.querier.query_wasm_smart(
                    source,
                    &QueryMsg::Member {
                        wallet: wallet.to_string(),
                    },
                )?;
                for id in member.active.iter().chain(member.next.iter()) {
                    if !CLANS.has(deps.storage, *id) {
                        return Err(fail("restore clan before member"));
                    }
                }
                MEMBERS.save(deps.storage, &wallet, &member)?;
            }
            return Ok(Response::new().add_attribute("action", "restore_clan_members"));
        }
        ExecuteMsg::SetEngine { engine } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            config.engine = deps.api.addr_validate(&engine)?;
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new().add_attribute("action", "set_clan_engine"));
        }
        ExecuteMsg::Restore { limit } => {
            if info.sender != config.owner || limit == 0 || limit > 5 {
                return Err(fail("unauthorized or invalid restore batch"));
            }
            let progress = config
                .restoration
                .as_ref()
                .ok_or_else(|| fail("restoration already complete"))?;
            let source = progress.source.clone();
            let rows: Vec<Clan> = deps.querier.query_wasm_smart(
                &source,
                &QueryMsg::Clans {
                    start_after: progress.after,
                    limit: Some(limit),
                },
            )?;
            for clan in &rows {
                CLANS.save(deps.storage, clan.id, clan)?;
                CODES.save(deps.storage, &clan.code, &clan.id)?;
                NAMES.save(deps.storage, &clan.name, &clan.id)?;
                for wallet in clan.members.iter().chain(clan.next_members.iter()) {
                    let member: Member = deps.querier.query_wasm_smart(
                        &source,
                        &QueryMsg::Member {
                            wallet: wallet.to_string(),
                        },
                    )?;
                    MEMBERS.save(deps.storage, wallet, &member)?;
                }
            }
            if rows.len() < limit as usize {
                config.restoration = None;
            } else {
                config.restoration.as_mut().unwrap().after = rows.last().map(|clan| clan.id);
            }
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new()
                .add_attribute("action", "restore_clans")
                .add_attribute("count", rows.len().to_string()));
        }
        ExecuteMsg::SetPauseController { controller } => {
            return game_common::pause::set_controller(deps, &info, &config.owner, controller)
        }
        ExecuteMsg::Configure {
            min_create_tiers,
            max_members,
            leader_premium_bps,
        } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            if let Some(n) = min_create_tiers {
                if n > 3000 {
                    return Err(fail("invalid tier threshold"));
                }
                config.min_create_tiers = n;
            }
            if let Some(n) = max_members {
                if !(1..=20).contains(&n) {
                    return Err(fail("invalid member cap"));
                }
                config.max_members = n;
            }
            if let Some(n) = leader_premium_bps {
                if n > 10000 {
                    return Err(fail("invalid leader premium"));
                }
                config.leader_premium_bps = n;
            }
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new().add_attribute("action", "configure_clans"));
        }
        _ => {}
    }
    if config.restoration.is_some() {
        return Err(fail("restoration incomplete"));
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    let response =
        Response::new().add_attribute("effective_cycle", next_cycle(&config)?.to_string());
    match msg {
        ExecuteMsg::BeginCycle { cycle } => {
            if info.sender != config.engine
                || (cycle != config.current_cycle && cycle != next_cycle(&config)?)
            {
                return Err(fail("unauthorized or nonsequential cycle"));
            }
            config.current_cycle = cycle;
            CONFIG.save(deps.storage, &config)?;
            Ok(Response::new()
                .add_attribute("action", "begin_clan_cycle")
                .add_attribute("cycle", cycle.to_string()))
        }
        ExecuteMsg::Create { name, code } => {
            let name = name.trim().to_string();
            let code = code.trim().to_ascii_uppercase();
            if name.is_empty()
                || name.len() > 64
                || code.is_empty()
                || code.len() > 12
                || !code
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || CODES.has(deps.storage, &code)
                || NAMES.has(deps.storage, &name)
            {
                return Err(fail("invalid or duplicate clan name/code"));
            }
            let mut member = MEMBERS
                .may_load(deps.storage, &info.sender)?
                .unwrap_or_default();
            member.normalize(config.current_cycle);
            if member.active.is_some() || member.next.is_some() {
                return Err(fail("already in clan or change pending"));
            }
            let fleet: racer_fleet::Fleet = deps.querier.query_wasm_smart(
                &config.fleet,
                &racer_fleet::QueryMsg::Fleet {
                    wallet: info.sender.to_string(),
                },
            )?;
            if fleet.next.iter().map(|t| u32::from(*t)).sum::<u32>() < config.min_create_tiers {
                return Err(fail("insufficient tiers to create clan"));
            }
            let id = config.next_id;
            config.next_id = id.checked_add(1).ok_or_else(|| fail("clan ID overflow"))?;
            CLANS.save(
                deps.storage,
                id,
                &Clan {
                    id,
                    name: name.clone(),
                    code: code.clone(),
                    members: vec![],
                    next_members: vec![],
                    leader: info.sender.clone(),
                    next_leader: info.sender.clone(),
                    effective_cycle: next_cycle(&config)?,
                    disband_at: None,
                },
            )?;
            CODES.save(deps.storage, &code, &id)?;
            NAMES.save(deps.storage, &name, &id)?;
            CONFIG.save(deps.storage, &config)?;
            switch(deps.branch(), &config, &info.sender, Some(id), false)?;
            Ok(response
                .add_attribute("action", "queue_create_clan")
                .add_attribute("clan_id", id.to_string()))
        }
        ExecuteMsg::Join { code } => {
            let id = CODES.load(deps.storage, &code.trim().to_ascii_uppercase())?;
            switch(deps, &config, &info.sender, Some(id), false)?;
            Ok(response
                .add_attribute("action", "queue_join_clan")
                .add_attribute("clan_id", id.to_string()))
        }
        ExecuteMsg::Leave {} => {
            switch(deps, &config, &info.sender, None, false)?;
            Ok(response.add_attribute("action", "queue_leave_clan"))
        }
        ExecuteMsg::CancelChange {} => {
            let mut member = MEMBERS
                .may_load(deps.storage, &info.sender)?
                .unwrap_or_default();
            member.normalize(config.current_cycle);
            if member.effective_cycle <= config.current_cycle
                || member.active == member.next
                || member.forced
            {
                return Err(fail("no cancellable pending change"));
            }
            switch(deps, &config, &info.sender, member.active, true)?;
            Ok(response.add_attribute("action", "cancel_clan_change"))
        }
        ExecuteMsg::TransferLead { clan_id, wallet } => {
            let mut clan = CLANS.load(deps.storage, clan_id)?;
            clan.normalize(config.current_cycle);
            let wallet = deps.api.addr_validate(&wallet)?;
            if info.sender != clan.leader
                || !clan.next_members.contains(&wallet)
                || clan.disband_at.is_some()
            {
                return Err(fail("unauthorized or invalid next leader"));
            }
            clan.next_leader = wallet;
            clan.effective_cycle = next_cycle(&config)?;
            CLANS.save(deps.storage, clan_id, &clan)?;
            Ok(response.add_attribute("action", "queue_clan_leader"))
        }
        ExecuteMsg::Kick { clan_id, wallet } => {
            let mut clan = CLANS.load(deps.storage, clan_id)?;
            clan.normalize(config.current_cycle);
            let wallet = deps.api.addr_validate(&wallet)?;
            let mut member = MEMBERS.may_load(deps.storage, &wallet)?.unwrap_or_default();
            member.normalize(config.current_cycle);
            if info.sender != clan.leader
                || member.active != Some(clan_id)
                || member.next != Some(clan_id)
            {
                return Err(fail("unauthorized kick or target change pending"));
            }
            switch(deps.branch(), &config, &wallet, None, false)?;
            let mut member = MEMBERS.load(deps.storage, &wallet)?;
            member.forced = true;
            MEMBERS.save(deps.storage, &wallet, &member)?;
            Ok(response.add_attribute("action", "queue_clan_kick"))
        }
        ExecuteMsg::Dismantle { clan_id } => {
            let mut clan = CLANS.load(deps.storage, clan_id)?;
            clan.normalize(config.current_cycle);
            if info.sender != clan.leader || clan.disband_at.is_some() {
                return Err(fail("unauthorized dismantle"));
            }
            let effective = next_cycle(&config)?;
            for wallet in &clan.next_members {
                let mut member = MEMBERS.load(deps.storage, wallet)?;
                member.normalize(config.current_cycle);
                member.next = None;
                member.effective_cycle = effective;
                member.forced = true;
                MEMBERS.save(deps.storage, wallet, &member)?;
            }
            clan.next_members.clear();
            clan.effective_cycle = effective;
            clan.disband_at = Some(effective);
            CLANS.save(deps.storage, clan_id, &clan)?;
            Ok(response.add_attribute("action", "queue_dismantle_clan"))
        }
        _ => unreachable!(),
    }
}

fn switch(
    deps: DepsMut,
    config: &Config,
    wallet: &Addr,
    target: Option<u64>,
    allow_pending: bool,
) -> StdResult<()> {
    let mut member = MEMBERS.may_load(deps.storage, wallet)?.unwrap_or_default();
    member.normalize(config.current_cycle);
    if !allow_pending
        && member.effective_cycle > config.current_cycle
        && member.active != member.next
    {
        return Err(fail("clan change already pending"));
    }
    if target == member.next {
        return Err(fail("already in target clan"));
    }
    if let Some(old) = member.next {
        let mut clan = CLANS.load(deps.storage, old)?;
        clan.normalize(config.current_cycle);
        if clan.next_leader == *wallet {
            return Err(fail("leader must transfer leadership or dismantle"));
        }
        clan.next_members.retain(|a| a != wallet);
        clan.effective_cycle = next_cycle(config)?;
        CLANS.save(deps.storage, old, &clan)?;
    }
    if let Some(id) = target {
        let mut clan = CLANS.load(deps.storage, id)?;
        clan.normalize(config.current_cycle);
        if clan.disband_at.is_some() || clan.next_members.len() >= usize::from(config.max_members) {
            return Err(fail("clan disbanding or full"));
        }
        if !clan.next_members.contains(wallet) {
            clan.next_members.push(wallet.clone());
        }
        clan.effective_cycle = next_cycle(config)?;
        CLANS.save(deps.storage, id, &clan)?;
    }
    member.next = target;
    member.effective_cycle = next_cycle(config)?;
    member.forced = false;
    MEMBERS.save(deps.storage, wallet, &member)?;
    Ok(())
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    match msg {
        QueryMsg::Config {} => to_json_binary(&config),
        QueryMsg::Member { wallet } => {
            let mut member = MEMBERS
                .may_load(deps.storage, &deps.api.addr_validate(&wallet)?)?
                .unwrap_or_default();
            member.normalize(config.current_cycle);
            to_json_binary(&member)
        }
        QueryMsg::Clan { id } => {
            let mut clan = CLANS.load(deps.storage, id)?;
            clan.normalize(config.current_cycle);
            to_json_binary(&clan)
        }
        QueryMsg::Clans { start_after, limit } => {
            let clans = CLANS
                .range(
                    deps.storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(20).clamp(1, 50) as usize)
                .map(|row| {
                    row.map(|(_, mut clan)| {
                        clan.normalize(config.current_cycle);
                        clan
                    })
                })
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&clans)
        }
    }
}
