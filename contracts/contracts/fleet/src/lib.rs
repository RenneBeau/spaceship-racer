use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdError, StdResult, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw_storage_plus::{Bound, Item, Map};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub engine: String,
    pub spaceusd: String,
    pub treasury: String,
    pub price_per_tier: Uint128,
}

#[cw_serde]
pub enum ShipType {
    Racer,
    Tank,
    Fighter,
}
impl ShipType {
    fn index(&self) -> usize {
        match self {
            Self::Racer => 0,
            Self::Tank => 1,
            Self::Fighter => 2,
        }
    }
}

#[cw_serde]
pub enum ReceiveMsg {
    PayFor {
        wallet: String,
        purchase: Purchase,
    },
    Buy {
        ship: ShipType,
        target_tier: u16,
        sponsor: Option<String>,
    },
    Upgrade {
        ship: ShipType,
        target_tier: u16,
    },
}

#[cw_serde]
pub enum Purchase {
    Buy {
        ship: ShipType,
        target_tier: u16,
        sponsor: Option<String>,
    },
    Upgrade {
        ship: ShipType,
        target_tier: u16,
    },
}
#[cw_serde]
pub struct Quote {
    pub price: Uint128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_price: Option<Uint128>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<u64>,
}

pub const PRESALE_SECONDS: u64 = 50 * 86400;
#[cw_serde]
pub struct Presale { pub starts_at: Option<u64>, pub duration_seconds: u64 }
pub fn launch_time(config: &Config) -> Option<u64> { config.presale.as_ref().map(|sale| sale.starts_at.map(|start| start + sale.duration_seconds).unwrap_or(u64::MAX)) }
pub fn price_at(config: &Config, now: u64) -> StdResult<Uint128> {
    let Some(sale) = &config.presale else { return Ok(config.price_per_tier); };
    let start = sale.starts_at.ok_or_else(|| fail("presale not started"))?;
    if now < start { return Err(fail("presale not started")); }
    let elapsed = now.saturating_sub(start).min(sale.duration_seconds);
    Ok(config.price_per_tier.multiply_ratio(u128::from(sale.duration_seconds + elapsed), u128::from(sale.duration_seconds) * 2))
}

#[cw_serde]
pub enum ExecuteMsg {
    PreparePresale {},
    StartPresale {},
    SchedulePresale { starts_at: u64 },
    SetPaymentAdapter {
        adapter: Option<String>,
    },
    SetPauseController {
        controller: Option<String>,
    },
    SetCapacity {
        max_players: u16,
    },
    Receive(Cw20ReceiveMsg),
    AdvanceRace {
        race: u64,
    },
    Configure {
        engine: Option<String>,
        treasury: Option<String>,
        price_per_tier: Option<Uint128>,
        paused: Option<bool>,
    },
}

#[cw_serde]
pub struct Config {
    #[serde(default)]
    pub presale: Option<Presale>,
    pub owner: Addr,
    pub engine: Addr,
    pub spaceusd: Addr,
    pub treasury: Addr,
    pub price_per_tier: Uint128,
    pub current_race: u64,
    pub paused: bool,
    pub players: u16,
    pub max_players: u16,
    pub payment_adapter: Option<Addr>,
}

#[cw_serde]
#[derive(Default)]
pub struct Fleet {
    pub active: [u16; 3],
    pub next: [u16; 3],
    pub effective_race: u64,
    pub sponsor: Option<Addr>,
    pub purchased: bool,
}
impl Fleet {
    fn normalize(&mut self, race: u64) {
        if self.effective_race <= race {
            self.active = self.next;
        }
    }
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Quote)]
    Quote { wallet: String, purchase: Purchase },
    #[returns(Config)]
    Config {},
    #[returns(Fleet)]
    Fleet { wallet: String },
    #[returns(Vec<String>)]
    Players {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    #[returns(Vec<String>)]
    Referrals {
        sponsor: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
}
const CONFIG: Item<Config> = Item::new("config");
const FLEETS: Map<&Addr, Fleet> = Map::new("fleets");
const REFERRALS: Map<(&Addr, &Addr), ()> = Map::new("referrals");
fn fail(msg: &str) -> StdError {
    StdError::generic_err(msg)
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() || msg.price_per_tier.is_zero() {
        return Err(fail("invalid funding or price"));
    }
    CONFIG.save(
        deps.storage,
        &Config {
            presale: None,
            owner: deps.api.addr_validate(&msg.owner)?,
            engine: deps.api.addr_validate(&msg.engine)?,
            spaceusd: deps.api.addr_validate(&msg.spaceusd)?,
            treasury: deps.api.addr_validate(&msg.treasury)?,
            price_per_tier: msg.price_per_tier,
            current_race: 0,
            paused: false,
            players: 0,
            max_players: 1000,
            payment_adapter: None,
        },
    )?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/fleet",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_fleet"))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    let mut config = CONFIG.load(deps.storage)?;
    if matches!(msg, ExecuteMsg::PreparePresale {} | ExecuteMsg::StartPresale {} | ExecuteMsg::SchedulePresale { .. }) {
        if info.sender != config.owner || config.current_race != 0 || config.players != 0 { return Err(fail("presale configuration requires owner and an unlaunched fleet")); }
        match msg {
            ExecuteMsg::PreparePresale {} => {
                if config.presale.is_some() { return Err(fail("presale already prepared")); }
                config.presale = Some(Presale { starts_at: None, duration_seconds: PRESALE_SECONDS });
            }
            ExecuteMsg::StartPresale {} | ExecuteMsg::SchedulePresale { .. } => {
                let mut sale = config.presale.clone().ok_or_else(|| fail("prepare presale first"))?;
                if sale.starts_at.is_some() { return Err(fail("presale start is already committed")); }
                let start = match msg { ExecuteMsg::SchedulePresale { starts_at } => starts_at, _ => env.block.time.seconds() };
                if start < env.block.time.seconds() || start > u64::MAX - PRESALE_SECONDS { return Err(fail("invalid presale start")); }
                sale.starts_at = Some(start); config.presale = Some(sale);
            }
            _ => unreachable!(),
        }
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "configure_presale"));
    }
    if let ExecuteMsg::SetPaymentAdapter { adapter } = msg {
        if info.sender != config.owner {
            return Err(fail("unauthorized"));
        }
        config.payment_adapter = adapter.map(|a| deps.api.addr_validate(&a)).transpose()?;
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "configure_payment_adapter"));
    }
    if let ExecuteMsg::SetPauseController { controller } = msg {
        return game_common::pause::set_controller(deps, &info, &config.owner, controller);
    }
    if let ExecuteMsg::SetCapacity { max_players } = msg {
        if info.sender != config.owner
            || max_players == 0
            || max_players > 1000
            || max_players < config.players
        {
            return Err(fail("unauthorized or invalid player capacity"));
        }
        config.max_players = max_players;
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "configure_fleet_capacity"));
    }
    if let ExecuteMsg::Configure {
        engine,
        treasury,
        price_per_tier,
        paused,
    } = msg
    {
        if info.sender != config.owner {
            return Err(fail("unauthorized"));
        }
        if let Some(value) = engine {
            config.engine = deps.api.addr_validate(&value)?;
        }
        if let Some(value) = treasury {
            config.treasury = deps.api.addr_validate(&value)?;
        }
        if let Some(value) = price_per_tier {
            if config.presale.as_ref().is_some_and(|sale| sale.starts_at.is_some()) && launch_time(&config).is_some_and(|launch| env.block.time.seconds() < launch) { return Err(fail("presale full price is locked until launch")); }
            if value.is_zero() {
                return Err(fail("zero price"));
            }
            config.price_per_tier = value;
        }
        if let Some(value) = paused {
            config.paused = value;
        }
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "configure_fleet"));
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    if config.paused {
        return Err(fail("paused"));
    }
    match msg {
        ExecuteMsg::AdvanceRace { race } => {
            if launch_time(&config).is_some_and(|launch| env.block.time.seconds() < launch) { return Err(fail("game launch not reached")); }
            if info.sender != config.engine {
                return Err(fail("unauthorized"));
            }
            let next = config
                .current_race
                .checked_add(1)
                .ok_or_else(|| fail("race overflow"))?;
            if race != next {
                return Err(fail("race must advance exactly once"));
            }
            config.current_race = race;
            CONFIG.save(deps.storage, &config)?;
            Ok(Response::new()
                .add_attribute("action", "advance_fleet_race")
                .add_attribute("race", race.to_string()))
        }
        ExecuteMsg::Receive(receive) => {
            if info.sender != config.spaceusd {
                return Err(fail("unsupported settlement token"));
            }
            let hook: ReceiveMsg = from_json(receive.msg)?;
            let (wallet, purchase) = match hook {
                ReceiveMsg::PayFor { wallet, purchase } => {
                    if config.payment_adapter.as_ref().map(|a| a.as_str())
                        != Some(receive.sender.as_str())
                    {
                        return Err(fail("unauthorized payment adapter"));
                    }
                    (deps.api.addr_validate(&wallet)?, purchase)
                }
                ReceiveMsg::Buy {
                    ship,
                    target_tier,
                    sponsor,
                } => (
                    deps.api.addr_validate(&receive.sender)?,
                    Purchase::Buy {
                        ship,
                        target_tier,
                        sponsor,
                    },
                ),
                ReceiveMsg::Upgrade { ship, target_tier } => (
                    deps.api.addr_validate(&receive.sender)?,
                    Purchase::Upgrade { ship, target_tier },
                ),
            };
            let mut fleet = FLEETS.may_load(deps.storage, &wallet)?.unwrap_or_default();
            if !fleet.purchased && config.players >= config.max_players {
                return Err(fail("fleet player capacity reached"));
            }
            fleet.normalize(config.current_race);
            let (ship, target, sponsor, buying) = match purchase {
                Purchase::Buy {
                    ship,
                    target_tier,
                    sponsor,
                } => (ship, target_tier, sponsor, true),
                Purchase::Upgrade { ship, target_tier } => (ship, target_tier, None, false),
            };
            if target == 0 || target > 1000 {
                return Err(fail("invalid tier"));
            }
            let index = ship.index();
            let previous = fleet.next[index];
            if buying && previous != 0 {
                return Err(fail("ship already owned"));
            }
            if !buying && previous == 0 {
                return Err(fail("ship not owned"));
            }
            if target <= previous {
                return Err(fail("tier must increase"));
            }
            if let Some(value) = sponsor {
                if fleet.purchased {
                    return Err(fail("sponsor registration closed"));
                }
                let addr = deps.api.addr_validate(&value)?;
                // A sponsor may be a wallet that has not purchased yet. Bound
                // traversal and reject cycles, including those closed later.
                let mut ancestor = Some(addr.clone());
                for _ in 0..1000 {
                    let Some(current) = ancestor.take() else {
                        break;
                    };
                    if current == wallet {
                        return Err(fail("cyclic referral"));
                    }
                    ancestor = FLEETS
                        .may_load(deps.storage, &current)?
                        .and_then(|f| f.sponsor);
                }
                if ancestor.is_some() {
                    return Err(fail("referral depth limit"));
                }
                REFERRALS.save(deps.storage, (&addr, &wallet), &())?;
                fleet.sponsor = Some(addr);
            }
            let base = price_at(&config, env.block.time.seconds())?
                .checked_mul(Uint128::from(target - previous))?;
            let discount = if !fleet.purchased && fleet.sponsor.is_some() {
                base.multiply_ratio(500u128, 10000u128)
            } else {
                Uint128::zero()
            };
            let price = base.checked_sub(discount)?;
            if receive.amount != price {
                return Err(fail("incorrect payment"));
            }
            let referral = if fleet.sponsor.is_some() {
                price.multiply_ratio(500u128, 10000u128)
            } else {
                Uint128::zero()
            };
            fleet.next[index] = target;
            fleet.effective_race = config
                .current_race
                .checked_add(1)
                .ok_or_else(|| fail("race overflow"))?;
            if !fleet.purchased {
                config.players += 1;
                CONFIG.save(deps.storage, &config)?;
            }
            fleet.purchased = true;
            FLEETS.save(deps.storage, &wallet, &fleet)?;
            let mut response = Response::new()
                .add_attribute("action", if buying { "buy_ship" } else { "upgrade_ship" })
                .add_attribute("wallet", wallet)
                .add_attribute("ship", index.to_string())
                .add_attribute("target_tier", target.to_string())
                .add_attribute("effective_race", fleet.effective_race.to_string())
                .add_attribute("referral", referral.to_string());
            if let Some(sponsor) = fleet.sponsor.as_ref() {
                response = response.add_attribute("sponsor", sponsor);
            }
            for (recipient, amount) in [
                (Some(config.treasury), price.checked_sub(referral)?),
                (fleet.sponsor, referral),
            ] {
                if let Some(recipient) = recipient {
                    if !amount.is_zero() {
                        response = response.add_message(WasmMsg::Execute {
                            contract_addr: config.spaceusd.to_string(),
                            msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                                recipient: recipient.to_string(),
                                amount,
                            })?,
                            funds: vec![],
                        });
                    }
                }
            }
            Ok(response)
        }
        ExecuteMsg::Configure { .. } => unreachable!(),
        ExecuteMsg::PreparePresale {} | ExecuteMsg::StartPresale {} | ExecuteMsg::SchedulePresale { .. } => unreachable!(),
        ExecuteMsg::SetCapacity { .. } => unreachable!(),
        ExecuteMsg::SetPauseController { .. } => unreachable!(),
        ExecuteMsg::SetPaymentAdapter { .. } => unreachable!(),
    }
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Quote { wallet, purchase } => {
            let config = CONFIG.load(deps.storage)?;
            let addr = deps.api.addr_validate(&wallet)?;
            let fleet = FLEETS.may_load(deps.storage, &addr)?.unwrap_or_default();
            let (ship, target, sponsor, buying) = match purchase {
                Purchase::Buy {
                    ship,
                    target_tier,
                    sponsor,
                } => (ship, target_tier, sponsor, true),
                Purchase::Upgrade { ship, target_tier } => (ship, target_tier, None, false),
            };
            if target == 0
                || target > 1000
                || (!fleet.purchased && config.players >= config.max_players)
            {
                return Err(fail("invalid fleet quote"));
            }
            let previous = fleet.next[ship.index()];
            if target <= previous || (buying && previous != 0) || (!buying && previous == 0) {
                return Err(fail("invalid ship purchase"));
            }
            if let Some(sponsor) = sponsor.as_ref() {
                if fleet.purchased || deps.api.addr_validate(sponsor)? == addr {
                    return Err(fail("invalid sponsor"));
                }
            }
            let base = price_at(&config, env.block.time.seconds())?
                .checked_mul(Uint128::from(target - previous))?;
            let discounted = !fleet.purchased && (fleet.sponsor.is_some() || sponsor.is_some());
            let discount = if discounted {
                base.multiply_ratio(500u128, 10000u128)
            } else {
                Uint128::zero()
            };
            let ongoing = launch_time(&config).is_some_and(|launch| env.block.time.seconds() < launch);
            let valid_until = ongoing.then_some(env.block.time.seconds() + 300);
            let maximum_price = if let Some(until) = valid_until {
                let maximum = price_at(&config, until)?.checked_mul(Uint128::from(target - previous))?;
                Some(if discounted { maximum.checked_sub(maximum.multiply_ratio(500u128, 10000u128))? } else { maximum })
            } else { None };
            to_json_binary(&Quote { price: base.checked_sub(discount)?, maximum_price, valid_until })
        }
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Fleet { wallet } => {
            let addr = deps.api.addr_validate(&wallet)?;
            let mut fleet = FLEETS.may_load(deps.storage, &addr)?.unwrap_or_default();
            fleet.normalize(CONFIG.load(deps.storage)?.current_race);
            to_json_binary(&fleet)
        }
        QueryMsg::Players { start_after, limit } => {
            let start = start_after
                .map(|addr| deps.api.addr_validate(&addr))
                .transpose()?;
            let rows: StdResult<Vec<_>> = FLEETS
                .keys(
                    deps.storage,
                    start.as_ref().map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(30).clamp(1, 100) as usize)
                .map(|addr| addr.map(|a| a.to_string()))
                .collect();
            to_json_binary(&rows?)
        }
        QueryMsg::Referrals {
            sponsor,
            start_after,
            limit,
        } => {
            let sponsor = deps.api.addr_validate(&sponsor)?;
            let start = start_after
                .map(|addr| deps.api.addr_validate(&addr))
                .transpose()?;
            let rows: StdResult<Vec<_>> = REFERRALS
                .prefix(&sponsor)
                .keys(
                    deps.storage,
                    start.as_ref().map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(30).clamp(1, 100) as usize)
                .map(|addr| addr.map(|a| a.to_string()))
                .collect();
            to_json_binary(&rows?)
        }
    }
}
