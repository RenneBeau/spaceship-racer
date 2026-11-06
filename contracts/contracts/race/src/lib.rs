pub mod healing;
pub mod math;
pub mod motion;
pub mod registry;
pub mod state;
pub use state::{BoostKind, BoostMsg, ExecuteMsg, InstantiateMsg, Parameters, QueryMsg};

use crate::{math::*, state::*};
use cosmwasm_std::{
    entry_point, from_json, to_json_binary, to_json_string, Addr, Binary, Deps, DepsMut, Env,
    MessageInfo, Order, Response, StdResult, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw_storage_plus::Bound;
use game_common::{maintenance, weighted_tiers};
use racer_collection::Bonus;
use racer_equipment::{Position, Slot};
use racer_fleet::Fleet;
use racer_rewards::Allocation;

fn call(address: &Addr, msg: Binary) -> WasmMsg {
    WasmMsg::Execute {
        contract_addr: address.to_string(),
        msg,
        funds: vec![],
    }
}
#[entry_point]
pub fn migrate(deps: DepsMut, env: Env, msg: MigrateMsg) -> StdResult<Response> {
    let prior = cw2::get_contract_version(deps.storage)?;
    if prior.contract != "spaceship-racer/race" || CONFIG.load(deps.storage)?.timing_version != 2 {
        return Err(fail("unsupported race storage version"));
    }
    let rewards = msg
        .reward_gateway
        .map(|address| deps.api.addr_validate(&address))
        .transpose()?;
    if let Some(address) = &rewards {
        let gateway: racer_rewards::State = deps
            .querier
            .query_wasm_smart(address, &racer_rewards::QueryMsg::State {})?;
        let config = CONFIG.load(deps.storage)?;
        if gateway.allocator != env.contract.address
            || gateway.spaceusd
                != deps
                    .querier
                    .query_wasm_smart::<racer_rewards::State>(
                        &config.rewards,
                        &racer_rewards::QueryMsg::State {},
                    )?
                    .spaceusd
            || gateway.srt != config.srt
        {
            return Err(fail("incompatible reward gateway"));
        }
    }
    CONFIG.update(deps.storage, |mut config| -> StdResult<_> {
        if let Some(rewards) = rewards {
            config.rewards = rewards;
        }
        config.pending_accounting_upgrade =
            config.accounting_version < 3 || config.pending_tick_verifier.is_some();
        Ok(config)
    })?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/race",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "upgrade_lazy_accounting"))
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
    validate(&msg.parameters)?;
    CONFIG.save(
        deps.storage,
        &Config {
            healing: healing::Settings::default(),
            pending_healing: None,
            tick_verifier: None,
            pending_tick_verifier: None,
            tick_safety_seconds: 0,
            pending_accounting_upgrade: false,
            accounting_version: 3,
            timing_version: 2,
            restore_from: msg
                .resume_from
                .as_ref()
                .map(|a| deps.api.addr_validate(a))
                .transpose()?,
            clans: None,
            owner: deps.api.addr_validate(&msg.owner)?,
            fleet: deps.api.addr_validate(&msg.fleet)?,
            equipment: deps.api.addr_validate(&msg.equipment)?,
            rewards: deps.api.addr_validate(&msg.rewards)?,
            randomness: deps.api.addr_validate(&msg.randomness)?,
            srt: deps.api.addr_validate(&msg.srt)?,
            treasury: deps.api.addr_validate(&msg.treasury)?,
            parameters: msg.parameters,
            paused: false,
        },
    )?;
    let resumed = if let Some(source) = &msg.resume_from {
        let prior: Config = deps
            .querier
            .query_wasm_smart(source, &QueryMsg::Config {})?;
        if prior.owner != info.sender
            || prior.owner != msg.owner
            || prior.fleet != msg.fleet
            || prior.equipment != msg.equipment
            || prior.rewards != msg.rewards
            || prior.srt != msg.srt
        {
            return Err(fail("resume wiring or owner mismatch"));
        }
        let mut state: State = deps.querier.query_wasm_smart(source, &QueryMsg::State {})?;
        if state.phase != Phase::Boundary {
            return Err(fail("resume requires a race boundary"));
        }
        state.phase = Phase::Restore;
        state.snapshot_after = None;
        state.cursor = 0;
        state.participants = 0;
        state.start_height = 0;
        state.tick = 0;
        state.sealed = 0;
        state.next_request = 1;
        state.seed = Binary::default();
        state.multiplier = 0;
        state.total_distance = Uint128::zero();
        state.escrow_srt = Uint128::zero();
        Some(state)
    } else {
        None
    };
    STATE.save(
        deps.storage,
        &resumed.unwrap_or(State {
            inline_oracle: false,
            registry_enabled: false,
            registry_generation: 0,
            registry_scanned: 0,
            replay_until: 0,
            rocket_cursor: 0,
            combat_spent: Uint128::zero(),
            clan_after: None,
            leader_bps: 0,
            phase: Phase::Boundary,
            race: 0,
            cycle: 1,
            completed: 0,
            cursor: 0,
            snapshot_after: None,
            participants: 0,
            start_height: 0,
            tick: 0,
            sealed: 0,
            next_request: 1,
            seed: Binary::default(),
            multiplier: 0,
            total_distance: Uint128::zero(),
            total_score: Uint128::zero(),
            tier_sum: Uint128::zero(),
            budget_usd: Uint128::zero(),
            budget_srt: Uint128::zero(),
            settlement_after: None,
            escrow_srt: Uint128::zero(),
        }),
    )?;
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/race",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_race"))
}

#[entry_point]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    let mut config = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    if let ExecuteMsg::SetTickVerifier { address } = msg {
        if info.sender != config.owner {
            return Err(fail("unauthorized"));
        }
        let address = deps.api.addr_validate(&address)?;
        let protocol: racer_beacon_verifier::Protocol = deps
            .querier
            .query_wasm_smart(&address, &racer_beacon_verifier::QueryMsg::Protocol {})?;
        if protocol != racer_beacon_verifier::protocol() {
            return Err(fail("unsupported verifier protocol"));
        }
        let oracle: racer_randomness::Config = deps
            .querier
            .query_wasm_smart(&config.randomness, &racer_randomness::QueryMsg::Config {})?;
        if !(6..=120).contains(&oracle.safety_seconds) {
            return Err(fail("unsupported beacon safety"));
        }
        config.pending_tick_verifier = Some(address);
        config.tick_safety_seconds = oracle.safety_seconds;
        config.pending_accounting_upgrade = true;
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "schedule_tick_verifier"));
    }
    if let ExecuteMsg::SetClans { address } = msg {
        if info.sender != config.owner
            || (state.phase != Phase::Restore
                && (state.phase != Phase::Boundary || state.completed != 0))
        {
            return Err(fail("clans can only change between cycles"));
        }
        config.clans = address.map(|a| deps.api.addr_validate(&a)).transpose()?;
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "set_race_clans"));
    }
    if let ExecuteMsg::SetPauseController { controller } = msg {
        return game_common::pause::set_controller(deps, &info, &config.owner, controller);
    }
    if let ExecuteMsg::Restore { limit } = msg {
        let response = restore(deps.branch(), &info, &config, &mut state, limit)?;
        STATE.save(deps.storage, &state)?;
        return Ok(response);
    }
    if let ExecuteMsg::ConfigureHealing { settings } = msg {
        if info.sender != config.owner {
            return Err(fail("unauthorized"));
        }
        settings.validate()?;
        config.pending_healing = Some(settings);
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "schedule_healing_settings"));
    }
    if let ExecuteMsg::Configure { paused, parameters } = msg {
        if info.sender != config.owner {
            return Err(fail("unauthorized"));
        }
        if let Some(parameters) = parameters {
            if state.phase != Phase::Boundary || state.completed != 0 {
                return Err(fail("parameters locked until next cycle"));
            }
            validate(&parameters)?;
            config.parameters = parameters;
        }
        if let Some(paused) = paused {
            config.paused = paused;
        }
        CONFIG.save(deps.storage, &config)?;
        return Ok(Response::new().add_attribute("action", "configure_race"));
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    if config.paused {
        return Err(fail("paused"));
    }
    let response = match msg {
        ExecuteMsg::PrepareNextMultiplier {} => {
            prepare_multiplier(deps.branch(), &config, &mut state)?
        }
        ExecuteMsg::Restore { .. } => unreachable!(),
        ExecuteMsg::Advance { limit } => {
            if limit == 0 || limit > MAX_BATCH {
                return Err(fail("invalid batch size"));
            }
            if state.phase == Phase::Boundary && config.pending_accounting_upgrade {
                if let Some(verifier) = config.pending_tick_verifier.take() {
                    config.tick_verifier = Some(verifier);
                    config.accounting_version = 4;
                } else if config.accounting_version < 3 {
                    config.accounting_version = 3;
                }
                config.pending_accounting_upgrade = false;
                config.parameters.max_actions_per_tick = 0;
                CONFIG.save(deps.storage, &config)?;
            }
            advance(deps.branch(), &env, &config, &mut state, limit)?
        }
        ExecuteMsg::AdvanceBatch { limit, steps } => {
            if config.accounting_version < 3
                || limit == 0
                || limit > MAX_BATCH
                || steps == 0
                || steps > 8
            {
                return Err(fail("invalid bounded batch"));
            }
            let family = match state.phase {
                Phase::Snapshot => 0,
                Phase::Movement | Phase::Combat => 1,
                Phase::Accrue => 2,
                _ => return Err(fail("phase cannot batch")),
            };
            let mut response = Response::new();
            for _ in 0..steps {
                let allowed = match family {
                    0 => state.phase == Phase::Snapshot,
                    1 => matches!(state.phase, Phase::Movement | Phase::Combat),
                    _ => state.phase == Phase::Accrue,
                };
                if !allowed {
                    break;
                }
                let part = advance(deps.branch(), &env, &config, &mut state, limit)?;
                response = response
                    .add_submessages(part.messages)
                    .add_attributes(part.attributes)
                    .add_events(part.events);
            }
            response
        }
        ExecuteMsg::CheckTick {
            race,
            tick,
            signature,
            seal_limit,
        } => {
            if !state.inline_oracle
                || state.phase != Phase::Waiting
                || state.replay_until != 0
                || race != state.race
                || tick != state.tick
                || seal_limit > MAX_BATCH
            {
                return Err(fail("stale or unsupported tick check"));
            }
            let beacon = INLINE_BEACONS
                .may_load(deps.storage, (race, tick))?
                .ok_or_else(|| fail("tick not sealed"))?;
            if env.block.time.seconds() < beacon.available_at {
                return Err(fail("beacon not due"));
            }
            state.seed =
                if let Some(seed) = VERIFIED_ROUNDS.may_load(deps.storage, (race, beacon.round))? {
                    seed
                } else {
                    let proof = signature.ok_or_else(|| fail("randomness pending"))?;
                    let verifier = config
                        .tick_verifier
                        .as_ref()
                        .ok_or_else(|| fail("tick verifier unavailable"))?;
                    let seed: Binary = deps.querier.query_wasm_smart(
                        verifier,
                        &racer_beacon_verifier::QueryMsg::Verify {
                            round: beacon.round,
                            signature: proof,
                        },
                    )?;
                    if seed.len() != 32 {
                        return Err(fail("invalid randomness length"));
                    }
                    VERIFIED_ROUNDS.save(deps.storage, (race, beacon.round), &seed)?;
                    seed
                };
            let mut response = ending_check(
                deps.storage,
                &env,
                &config,
                &mut state,
                Response::new()
                    .add_attribute("action", "check_race_tick")
                    .add_attribute("race", race.to_string()),
            )?;
            let closed = env
                .block
                .height
                .saturating_sub(state.start_height)
                .min(config.parameters.duration_cdf.len() as u64);
            if state.replay_until == 0 && seal_limit > 0 && closed > u64::from(state.sealed) {
                let sealed = seal(deps.branch(), &env, &config, &mut state, seal_limit)?;
                response = response
                    .add_attributes(sealed.attributes.into_iter().filter(|a| a.key != "action"));
            }
            response
        }
        ExecuteMsg::Seal { limit } => seal(deps.branch(), &env, &config, &mut state, limit)?,
        ExecuteMsg::Receive(receive) => {
            receive_boost(deps.branch(), &env, &info, &config, &mut state, receive)?
        }
        ExecuteMsg::ClaimRefunds { race, ticks } => {
            refund(deps.branch(), &info, &config, &mut state, race, ticks)?
        }
        ExecuteMsg::Configure { .. } | ExecuteMsg::ConfigureHealing { .. } => unreachable!(),
        ExecuteMsg::SetPauseController { .. } => unreachable!(),
        ExecuteMsg::SetClans { .. } => unreachable!(),
        ExecuteMsg::SetTickVerifier { .. } => unreachable!(),
    };
    STATE.save(deps.storage, &state)?;
    Ok(response)
}

fn active_phase(phase: &Phase) -> bool {
    matches!(phase, Phase::Waiting | Phase::Movement | Phase::Combat)
}

fn prepare_multiplier(deps: DepsMut, config: &Config, state: &mut State) -> StdResult<Response> {
    if state.phase == Phase::Restore {
        return Err(fail("restoration incomplete"));
    }
    let race = state
        .race
        .checked_add(1)
        .ok_or_else(|| fail("race overflow"))?;
    if MULTIPLIERS.has(deps.storage, race) {
        return Ok(Response::new().add_attribute("action", "multiplier_already_committed"));
    }
    let id = state.next_request;
    state.next_request = id.checked_add(1).ok_or_else(|| fail("request overflow"))?;
    MULTIPLIERS.save(deps.storage, race, &id)?;
    Ok(Response::new()
        .add_attribute("action", "commit_race_multiplier")
        .add_attribute("race", race.to_string())
        .add_message(call(
            &config.randomness,
            to_json_binary(&racer_randomness::ExecuteMsg::Request { id })?,
        )))
}
fn committed_multiplier(
    deps: Deps,
    env: &Env,
    config: &Config,
    state: &State,
    race: u64,
) -> StdResult<MultiplierCommitment> {
    let id = MULTIPLIERS.may_load(deps.storage, race)?;
    let Some(id) = id else {
        return Ok(MultiplierCommitment {
            race,
            request_id: None,
            available_at: None,
            multiplier: None,
        });
    };
    let request: racer_randomness::Request = deps.querier.query_wasm_smart(
        &config.randomness,
        &racer_randomness::QueryMsg::Request {
            consumer: env.contract.address.to_string(),
            id,
        },
    )?;
    let value = if let Some(seed) = request.randomness {
        let mut context = state.clone();
        context.race = race;
        context.tick = 0;
        context.seed = seed;
        Some(multiplier(
            draw(env, &context, b"multiplier", 0, 0, Uint128::new(1000))?.u128(),
        ))
    } else {
        None
    };
    Ok(MultiplierCommitment {
        race,
        request_id: Some(id),
        available_at: Some(request.available_at),
        multiplier: value,
    })
}
fn restore(
    deps: DepsMut,
    info: &MessageInfo,
    config: &Config,
    state: &mut State,
    limit: u16,
) -> StdResult<Response> {
    if info.sender != config.owner
        || state.phase != Phase::Restore
        || limit == 0
        || limit > MAX_BATCH
    {
        return Err(fail("unauthorized or invalid restoration"));
    }
    let source = config
        .restore_from
        .as_ref()
        .ok_or_else(|| fail("missing restoration source"))?;
    let wallets: Vec<String> = deps.querier.query_wasm_smart(
        &config.fleet,
        &racer_fleet::QueryMsg::Players {
            start_after: state.snapshot_after.clone(),
            limit: Some(u32::from(limit)),
        },
    )?;
    for wallet in &wallets {
        let entry: CyclePlayer = deps.querier.query_wasm_smart(
            source,
            &QueryMsg::CyclePlayer {
                cycle: state.cycle,
                wallet: wallet.clone(),
            },
        )?;
        let wallet = deps.api.addr_validate(wallet)?;
        if let Some(id) = entry.clan {
            let clans = config
                .clans
                .as_ref()
                .ok_or_else(|| fail("restore clans before cycle players"))?;
            let clan: racer_clans::Clan = deps
                .querier
                .query_wasm_smart(clans, &racer_clans::QueryMsg::Clan { id })?;
            CLAN_CYCLES.update(deps.storage, (state.cycle, id), |old| -> StdResult<_> {
                let mut row = old.unwrap_or(ClanCycle {
                    leader: clan.leader,
                    score: Uint128::zero(),
                    tiers: Uint128::zero(),
                });
                row.score = row.score.checked_add(entry.score)?;
                row.tiers = row.tiers.checked_add(entry.tier_sum)?;
                Ok(row)
            })?;
        }
        CYCLE_PLAYERS.save(deps.storage, (state.cycle, &wallet), &entry)?;
    }
    state.snapshot_after = wallets.last().cloned().or(state.snapshot_after.clone());
    if wallets.len() < usize::from(limit) {
        state.phase = Phase::Boundary;
        state.snapshot_after = None;
    }
    Ok(Response::new()
        .add_attribute("action", "restore_cycle_players")
        .add_attribute("count", wallets.len().to_string()))
}

fn seal(
    deps: DepsMut,
    env: &Env,
    config: &Config,
    state: &mut State,
    limit: u16,
) -> StdResult<Response> {
    if !active_phase(&state.phase) || limit == 0 || limit > MAX_BATCH {
        return Err(fail("cannot seal here"));
    }
    let mut response = Response::new().add_attribute("action", "seal_race_blocks");
    let closed = env
        .block
        .height
        .saturating_sub(state.start_height)
        .min(config.parameters.duration_cdf.len() as u64) as u32;
    let through = closed.min(state.sealed + u32::from(limit));
    if through <= state.sealed {
        return Err(fail("no closed unsealed blocks"));
    }
    while state.sealed < through {
        state.sealed += 1;
        if state.inline_oracle {
            let request = racer_randomness::future_request(
                env.block.time.seconds(),
                config.tick_safety_seconds,
            )?;
            INLINE_BEACONS.save(
                deps.storage,
                (state.race, state.sealed),
                &SealedBeacon {
                    round: request.round,
                    available_at: request.available_at,
                },
            )?;
            continue;
        }
        let id = state.next_request;
        state.next_request = id.checked_add(1).ok_or_else(|| fail("request overflow"))?;
        SEEDS.save(deps.storage, (state.race, state.sealed), &id)?;
        response = response.add_message(call(
            &config.randomness,
            to_json_binary(&racer_randomness::ExecuteMsg::Request { id })?,
        ));
    }
    Ok(response.add_attribute("sealed_through", state.sealed.to_string()))
}

fn receive_boost(
    deps: DepsMut,
    env: &Env,
    info: &MessageInfo,
    config: &Config,
    state: &mut State,
    receive: Cw20ReceiveMsg,
) -> StdResult<Response> {
    if info.sender != config.srt || !active_phase(&state.phase) || state.replay_until > 0 {
        return Err(fail("unsupported token or race closed"));
    }
    let request: BoostMsg = from_json(receive.msg)?;
    let immediate = !matches!(&request.kind, BoostKind::Rocket | BoostKind::Heal);
    if request.race != state.race || request.count == 0 {
        return Err(fail("wrong race or zero boosts"));
    }
    if env.block.height < state.start_height {
        return Err(fail("race action window not open"));
    }
    let tick = env.block.height - state.start_height + 1;
    if tick > config.parameters.duration_cdf.len() as u64 || tick <= u64::from(state.sealed) {
        return Err(fail("action window closed"));
    }
    let tick = tick as u32;
    let wallet = deps.api.addr_validate(&receive.sender)?;
    let (registered_race, index) = INDEX.load(deps.storage, &wallet)?;
    if !state.registry_enabled && registered_race != state.race {
        return Err(fail("not participating this race"));
    }
    let race = state.race;
    let player = registry::load_player(deps.storage, state, index)?;
    if player.wallet != wallet {
        return Err(fail("registry wallet mismatch"));
    }
    if !player.alive && !matches!(request.kind, BoostKind::Heal) {
        return Err(fail("fleet destroyed"));
    }
    let mut boosts = BOOSTS
        .may_load(deps.storage, (race, tick, &wallet))?
        .unwrap_or_default();
    if config.accounting_version == 0
        && boosts.count() + u64::from(request.count)
            > u64::from(config.parameters.max_actions_per_tick)
    {
        return Err(fail("per-block boost limit"));
    }
    let price = match request.kind {
        BoostKind::Heal => {
            if !config.healing.enabled
                || config.accounting_version < 3
                || request.count != 1
                || receive.amount.is_zero()
            {
                return Err(fail("healing unavailable or invalid reserve"));
            }
            if healing::RESERVED.has(deps.storage, (race, tick, &wallet)) {
                return Err(fail("one heal request per race window"));
            }
            healing::RESERVED.save(deps.storage, (race, tick, &wallet), &receive.amount)?;
            receive.amount
        }
        BoostKind::Speed => {
            boosts.speed = boosts
                .speed
                .checked_add(request.count)
                .ok_or_else(|| fail("boost count overflow"))?;
            config.parameters.boost_price
        }
        BoostKind::Acceleration => {
            boosts.acceleration = boosts
                .acceleration
                .checked_add(request.count)
                .ok_or_else(|| fail("boost count overflow"))?;
            config.parameters.boost_price
        }
        BoostKind::Temporary => {
            boosts.temporary = boosts
                .temporary
                .checked_add(request.count)
                .ok_or_else(|| fail("boost count overflow"))?;
            config.parameters.boost_price
        }
        BoostKind::Rocket => {
            if player.ships[2].hp.is_zero() {
                return Err(fail("no active fighter"));
            }
            boosts.rockets = boosts
                .rockets
                .checked_add(request.count)
                .ok_or_else(|| fail("boost count overflow"))?;
            priced_rocket(config, &player)
        }
    };
    let payment = mul(price, u128::from(request.count))?;
    if payment.is_zero() || receive.amount != payment {
        return Err(fail("incorrect boost payment"));
    }
    boosts.paid = boosts.paid.checked_add(payment)?;
    BOOSTS.save(deps.storage, (race, tick, &wallet), &boosts)?;
    if config.accounting_version > 0 {
        motion::WORK.save(deps.storage, (race, tick, index), &true)?;
        motion::ACTIONS.save(deps.storage, (race, tick, index), &true)?;
    }
    if immediate {
        let mut live = LIVE_PENDING
            .may_load(deps.storage, &wallet)?
            .filter(|p| p.race == race)
            .unwrap_or(LivePending {
                race,
                boosts: vec![],
            });
        let value = LiveTick {
            tick,
            speed: boosts.speed,
            acceleration: boosts.acceleration,
            temporary: boosts.temporary,
        };
        if let Some(entry) = live.boosts.iter_mut().find(|entry| entry.tick == tick) {
            *entry = value;
        } else {
            if live.boosts.len() >= config.parameters.duration_cdf.len() {
                return Err(fail("live boost bound exceeded"));
            }
            live.boosts.push(value);
        }
        LIVE_PENDING.save(deps.storage, &wallet, &live)?;
    }
    state.escrow_srt = state.escrow_srt.checked_add(payment)?;
    Ok(Response::new()
        .add_attribute("action", "queue_block_boost")
        .add_attribute("wallet", wallet)
        .add_attribute("race", race.to_string())
        .add_attribute("tick", tick.to_string())
        .add_attribute(
            "kind",
            match request.kind {
                BoostKind::Heal => "heal",
                BoostKind::Speed => "speed",
                BoostKind::Acceleration => "acceleration",
                BoostKind::Temporary => "temporary",
                BoostKind::Rocket => "rocket",
            },
        )
        .add_attribute("count", request.count.to_string())
        .add_attribute("paid", payment))
    .map(|response| {
        response
            .add_attribute("live_activated", immediate.to_string())
            .add_attribute("activation_height", env.block.height.to_string())
    })
}

fn live_player(
    deps: Deps,
    env: &Env,
    config: &Config,
    state: &State,
    wallet: &str,
) -> StdResult<LivePlayer> {
    let wallet = deps.api.addr_validate(wallet)?;
    let (race, index) = INDEX.load(deps.storage, &wallet)?;
    let mut player = registry::load_player(deps.storage, state, index)?;
    if config.accounting_version > 0 && player.race == state.race {
        project_to(config, &mut player, processed_tick(state, index))?;
    }
    if (!state.registry_enabled && race != player.race) || player.wallet != wallet {
        return Err(fail("player no longer in active snapshot"));
    }
    let tick = env
        .block
        .height
        .saturating_sub(state.start_height)
        .saturating_add(1)
        .min(config.parameters.duration_cdf.len() as u64) as u32;
    let pending = LIVE_PENDING
        .may_load(deps.storage, &wallet)?
        .filter(|p| p.race == state.race)
        .map(|p| p.boosts)
        .unwrap_or_default();
    if !active_phase(&state.phase) || !player.alive || player.race != state.race {
        return Ok(LivePlayer {
            player,
            pending: vec![],
            activation_tick: tick,
        });
    }
    let processed = match state.phase {
        Phase::Waiting => state.tick.saturating_sub(1),
        Phase::Movement if index > state.cursor => state.tick.saturating_sub(1),
        _ => state.tick,
    };
    let accel_bonus = 10000u128 + u128::from(bonus(&player, Bonus::Acceleration));
    let base_ticks = tick.saturating_sub(processed.saturating_add(1));
    let accrued = mul(
        config.parameters.acceleration,
        SCALE
            * u128::from(player.acceleration_count)
            * u128::from(base_ticks)
            * u128::from(config.parameters.tick_seconds),
    )?
    .multiply_ratio(accel_bonus, 10000u128);
    player.safe_speed = player.safe_speed.checked_add(accrued)?;
    player
        .temporary
        .retain(|temporary| temporary.expires > tick);
    for entry in &pending {
        player.safe_speed = player.safe_speed.checked_add(mul(
            config.parameters.speed_boost,
            SCALE * u128::from(entry.speed),
        )?)?;
        player.acceleration_count = player
            .acceleration_count
            .checked_add(u64::from(entry.acceleration))
            .ok_or_else(|| fail("boost overflow"))?;
        let ticks = tick.saturating_sub(entry.tick);
        let accrued = mul(
            config.parameters.acceleration,
            SCALE
                * u128::from(entry.acceleration)
                * u128::from(ticks)
                * u128::from(config.parameters.tick_seconds),
        )?
        .multiply_ratio(accel_bonus, 10000u128);
        player.safe_speed = player.safe_speed.checked_add(accrued)?;
        let expires = entry.tick + u32::from(config.parameters.temporary_ticks);
        if entry.temporary > 0 && expires > tick {
            player.temporary.push(Temporary {
                expires,
                distance: mul(
                    config.parameters.temporary_distance,
                    SCALE * u128::from(entry.temporary),
                )?,
            });
        }
    }
    Ok(LivePlayer {
        player,
        pending,
        activation_tick: tick,
    })
}

fn refund(
    deps: DepsMut,
    info: &MessageInfo,
    config: &Config,
    state: &mut State,
    race: u64,
    ticks: Vec<u32>,
) -> StdResult<Response> {
    if ticks.len() > 20 {
        return Err(fail("refund batch too large"));
    }
    let mut total = REFUNDS
        .may_load(deps.storage, &info.sender)?
        .unwrap_or_default();
    if !ticks.is_empty() {
        let result = RESULTS.load(deps.storage, race)?;
        for tick in ticks {
            if tick <= result.ticks {
                return Err(fail("tick was processed"));
            }
            if let Some(boosts) = BOOSTS.may_load(deps.storage, (race, tick, &info.sender))? {
                total = total.checked_add(boosts.paid)?;
                BOOSTS.remove(deps.storage, (race, tick, &info.sender));
                healing::RESERVED.remove(deps.storage, (race, tick, &info.sender));
            }
        }
    }
    if total.is_zero() {
        return Err(fail("nothing to refund"));
    }
    REFUNDS.remove(deps.storage, &info.sender);
    state.escrow_srt = state.escrow_srt.checked_sub(total)?;
    Ok(Response::new()
        .add_message(call(
            &config.srt,
            to_json_binary(&Cw20ExecuteMsg::Transfer {
                recipient: info.sender.to_string(),
                amount: total,
            })?,
        ))
        .add_attribute("action", "refund_unused_boosts")
        .add_attribute("amount", total))
}

fn advance(
    deps: DepsMut,
    env: &Env,
    config: &Config,
    state: &mut State,
    limit: u16,
) -> StdResult<Response> {
    let response = Response::new()
        .add_attribute("action", "advance_race")
        .add_attribute("race", state.race.to_string());
    match state.phase {
        Phase::Restore => Err(fail("restoration incomplete")),
        Phase::Boundary => {
            let commitment =
                committed_multiplier(deps.as_ref(), env, config, state, state.race + 1)?;
            if commitment.request_id.is_none() {
                return prepare_multiplier(deps, config, state);
            }
            if commitment.multiplier.is_none() {
                return Err(fail("next multiplier pending"));
            }
            state.phase = Phase::DrainEquipment;
            if let Some(settings) = &config.pending_healing {
                CONFIG.update(deps.storage, |mut current| -> StdResult<_> {
                    current.healing = settings.clone();
                    current.pending_healing = None;
                    Ok(current)
                })?;
            }
            let mut response = response;
            if state.completed == 0 {
                if let Some(clans) = &config.clans {
                    let clan_config: racer_clans::Config = deps
                        .querier
                        .query_wasm_smart(clans, &racer_clans::QueryMsg::Config {})?;
                    state.leader_bps = clan_config.leader_premium_bps;
                    response = response.add_message(call(
                        clans,
                        to_json_binary(&racer_clans::ExecuteMsg::BeginCycle {
                            cycle: state.cycle,
                        })?,
                    ));
                }
            }
            Ok(response.add_message(call(
                &config.equipment,
                to_json_binary(&racer_equipment::ExecuteMsg::BeginBoundary {})?,
            )))
        }
        Phase::DrainEquipment => {
            let wallets: Vec<String> = deps.querier.query_wasm_smart(
                &config.equipment,
                &racer_equipment::QueryMsg::PendingWallets {
                    start_after: None,
                    limit: Some(u32::from(limit)),
                },
            )?;
            if !wallets.is_empty() {
                if config.accounting_version >= 3 {
                    for wallet in &wallets {
                        registry::DIRTY.save(
                            deps.storage,
                            &deps.api.addr_validate(wallet)?,
                            &true,
                        )?;
                    }
                }
                return Ok(response.add_message(call(
                    &config.equipment,
                    to_json_binary(&racer_equipment::ExecuteMsg::Process { wallets })?,
                )));
            }
            state.race = state
                .race
                .checked_add(1)
                .ok_or_else(|| fail("race overflow"))?;
            state.inline_oracle = config.accounting_version >= 4 && config.tick_verifier.is_some();
            state.multiplier = committed_multiplier(deps.as_ref(), env, config, state, state.race)?
                .multiplier
                .ok_or_else(|| fail("next multiplier pending"))?;
            state.phase = Phase::Snapshot;
            state.snapshot_after = None;
            state.participants = 0;
            state.cursor = 0;
            state.sealed = 0;
            state.tick = 1;
            state.total_distance = Uint128::zero();
            if config.accounting_version >= 3 {
                registry::begin(deps.storage, state, &config.parameters)?;
            }
            Ok(response
                .add_message(call(
                    &config.equipment,
                    to_json_binary(&racer_equipment::ExecuteMsg::EndBoundary {})?,
                ))
                .add_message(call(
                    &config.fleet,
                    to_json_binary(&racer_fleet::ExecuteMsg::AdvanceRace { race: state.race })?,
                )))
        }
        Phase::Snapshot => snapshot(deps, env, config, state, limit, response),
        Phase::Waiting => {
            if state.inline_oracle {
                let beacon = INLINE_BEACONS
                    .may_load(deps.storage, (state.race, state.tick))?
                    .ok_or_else(|| fail("tick not sealed"))?;
                if env.block.time.seconds() < beacon.available_at {
                    return Err(fail("beacon not due"));
                }
                state.seed = VERIFIED_ROUNDS
                    .may_load(deps.storage, (state.race, beacon.round))?
                    .ok_or_else(|| fail("randomness pending"))?;
            } else {
                let id = SEEDS
                    .may_load(deps.storage, (state.race, state.tick))?
                    .ok_or_else(|| fail("tick not sealed"))?;
                let request: racer_randomness::Request = deps.querier.query_wasm_smart(
                    &config.randomness,
                    &racer_randomness::QueryMsg::Request {
                        consumer: env.contract.address.to_string(),
                        id,
                    },
                )?;
                if config.accounting_version >= 3 {
                    if let Some(seed) = &request.randomness {
                        if VERIFIED_ROUNDS
                            .may_load(deps.storage, (state.race, request.round))?
                            .as_ref()
                            .map(|known| known != seed)
                            .unwrap_or(false)
                        {
                            return Err(fail("conflicting verified round"));
                        }
                        if !VERIFIED_ROUNDS.has(deps.storage, (state.race, request.round)) {
                            VERIFIED_ROUNDS.save(
                                deps.storage,
                                (state.race, request.round),
                                seed,
                            )?;
                        }
                    }
                }
                state.seed = request
                    .randomness
                    .or(if config.accounting_version >= 3 {
                        VERIFIED_ROUNDS.may_load(deps.storage, (state.race, request.round))?
                    } else {
                        None
                    })
                    .ok_or_else(|| fail("randomness pending"))?;
            }
            if state.seed.len() != 32 {
                return Err(fail("invalid randomness length"));
            }
            if config.accounting_version >= 2 && state.replay_until == 0 {
                return ending_check(deps.storage, env, config, state, response);
            }
            if config.accounting_version >= 2 {
                let has_rocket = motion::ACTIONS
                    .prefix((state.race, state.tick))
                    .keys(deps.storage, None, None, Order::Ascending)
                    .map(|index| -> StdResult<bool> {
                        let p = registry::load_player(deps.storage, state, index?)?;
                        Ok(BOOSTS
                            .load(deps.storage, (state.race, state.tick, &p.wallet))?
                            .rockets
                            > 0)
                    })
                    .collect::<StdResult<Vec<_>>>()?
                    .into_iter()
                    .any(|v| v);
                if has_rocket || state.tick == state.replay_until {
                    let damaged = motion::DAMAGED
                        .prefix(state.race)
                        .keys(deps.storage, None, None, Order::Ascending)
                        .collect::<StdResult<Vec<_>>>()?;
                    for index in damaged {
                        motion::WORK.save(deps.storage, (state.race, state.tick, index), &true)?;
                    }
                }
            }
            state.total_distance = Uint128::zero();
            state.cursor = 0;
            state.phase = Phase::Movement;
            Ok(response)
        }
        Phase::Movement => {
            if config.accounting_version > 0 {
                return lazy_movement(deps, config, state, limit, response);
            }
            let through = state.participants.min(state.cursor + limit);
            for index in state.cursor + 1..=through {
                let mut player = PLAYERS.load(deps.storage, index)?;
                let boosts = BOOSTS
                    .may_load(deps.storage, (state.race, state.tick, &player.wallet))?
                    .unwrap_or_default();
                move_player_legacy(&mut player, &boosts, &config.parameters, state.tick)?;
                if let Some(mut live) = LIVE_PENDING.may_load(deps.storage, &player.wallet)? {
                    live.boosts.retain(|entry| entry.tick != state.tick);
                    if live.boosts.is_empty() {
                        LIVE_PENDING.remove(deps.storage, &player.wallet);
                    } else {
                        LIVE_PENDING.save(deps.storage, &player.wallet, &live)?;
                    }
                }
                let distance = weight(&player)?;
                tree_change(deps.storage, state, index, Uint128::zero(), distance)?;
                state.total_distance = state.total_distance.checked_add(distance)?;
                PLAYERS.save(deps.storage, index, &player)?;
            }
            state.cursor = through;
            if through == state.participants {
                state.phase = Phase::Combat;
                state.cursor = 0;
            }
            Ok(response)
        }
        Phase::Combat => combat(deps, env, config, state, limit, response),
        Phase::Checkpoint => {
            state.phase = if state.registry_enabled {
                state.settlement_after = None;
                Phase::Accrue
            } else {
                Phase::Budget
            };
            Ok(response.add_message(call(
                &config.rewards,
                to_json_binary(&racer_rewards::ExecuteMsg::Checkpoint {})?,
            )))
        }
        Phase::Accrue => {
            if registry::accrue_page(deps, state, limit)? {
                state.settlement_after = None;
                state.phase = Phase::Budget;
            }
            Ok(response)
        }
        Phase::Budget => {
            let rewards: racer_rewards::State = deps
                .querier
                .query_wasm_smart(&config.rewards, &racer_rewards::QueryMsg::State {})?;
            state.budget_usd = rewards.available.spaceusd;
            state.budget_srt = rewards.available.srt;
            state.settlement_after = None;
            state.clan_after = None;
            state.phase = if config.clans.is_some() {
                Phase::ClanPremiums
            } else {
                Phase::Settlement
            };
            Ok(response
                .add_attribute("cycle_usd", state.budget_usd)
                .add_attribute("cycle_srt", state.budget_srt))
        }
        Phase::Settlement => settle(deps, config, state, limit, response),
        Phase::ClanPremiums => {
            let rows: Vec<_> = CLAN_CYCLES
                .prefix(state.cycle)
                .range(
                    deps.storage,
                    state.clan_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(usize::from(limit))
                .collect::<StdResult<_>>()?;
            let mut recipients = vec![];
            for (_, clan) in &rows {
                let gross = if state.total_score.is_zero() {
                    Uint128::zero()
                } else {
                    state
                        .budget_usd
                        .multiply_ratio(clan.score, state.total_score)
                };
                let premium = gross.multiply_ratio(u128::from(state.leader_bps), 10000u128);
                if !premium.is_zero() {
                    recipients.push(Allocation {
                        wallet: clan.leader.to_string(),
                        spaceusd: premium,
                        srt: Uint128::zero(),
                    });
                }
            }
            state.clan_after = rows.last().map(|(id, _)| *id).or(state.clan_after);
            if rows.len() < usize::from(limit) {
                state.phase = Phase::Settlement;
            }
            if recipients.is_empty() {
                Ok(response)
            } else {
                Ok(response.add_message(call(
                    &config.rewards,
                    to_json_binary(&racer_rewards::ExecuteMsg::Allocate { recipients })?,
                )))
            }
        }
    }
}

fn snapshot(
    deps: DepsMut,
    env: &Env,
    config: &Config,
    state: &mut State,
    limit: u16,
    response: Response,
) -> StdResult<Response> {
    if state.registry_enabled {
        let done = registry::scan(deps, config, state, limit)?;
        if done {
            if state.participants == 0 {
                state.phase = Phase::Boundary;
            } else {
                state.start_height = env
                    .block
                    .height
                    .checked_add(1)
                    .ok_or_else(|| fail("height overflow"))?;
                state.phase = Phase::Waiting;
            }
        }
        return Ok(response
            .add_attribute("participants", state.participants.to_string())
            .add_attribute("registry_scanned", state.registry_scanned.to_string()));
    }
    let wallets: Vec<String> = deps.querier.query_wasm_smart(
        &config.fleet,
        &racer_fleet::QueryMsg::Players {
            start_after: state.snapshot_after.clone(),
            limit: Some(u32::from(limit)),
        },
    )?;
    for wallet in &wallets {
        let fleet: Fleet = deps.querier.query_wasm_smart(
            &config.fleet,
            &racer_fleet::QueryMsg::Fleet {
                wallet: wallet.clone(),
            },
        )?;
        let tier_sum =
            u32::from(fleet.active[0]) + u32::from(fleet.active[1]) + u32::from(fleet.active[2]);
        if tier_sum == 0 {
            continue;
        }
        if state.participants >= config.parameters.max_players {
            return Err(fail("race capacity exceeded"));
        }
        let wallet = deps.api.addr_validate(wallet)?;
        let equipment: Position = deps.querier.query_wasm_smart(
            &config.equipment,
            &racer_equipment::QueryMsg::Position {
                wallet: wallet.to_string(),
                slot: Slot::Commander,
            },
        )?;
        let mut player = make_player(
            state.race,
            wallet.clone(),
            fleet.active,
            equipment.active.map(|e| e.attributes),
        )?;
        if config.accounting_version >= 2 {
            for ship in &mut player.ships {
                ship.regen = Uint128::zero();
            }
        }
        let clan_id = if let Some(clans) = &config.clans {
            let member: racer_clans::Member = deps.querier.query_wasm_smart(
                clans,
                &racer_clans::QueryMsg::Member {
                    wallet: wallet.to_string(),
                },
            )?;
            if let Some(id) = member.active {
                let clan: racer_clans::Clan = deps
                    .querier
                    .query_wasm_smart(clans, &racer_clans::QueryMsg::Clan { id })?;
                CLAN_CYCLES.update(deps.storage, (state.cycle, id), |old| -> StdResult<_> {
                    let mut row = old.unwrap_or(ClanCycle {
                        score: Uint128::zero(),
                        tiers: Uint128::zero(),
                        leader: clan.leader,
                    });
                    row.tiers = row.tiers.checked_add(Uint128::from(tier_sum))?;
                    Ok(row)
                })?;
            }
            member.active
        } else {
            None
        };
        state.participants += 1;
        PLAYERS.save(deps.storage, state.participants, &player)?;
        if config.accounting_version > 0 {
            update_motion(deps.storage, config, state, state.participants, &player)?;
        }
        LIVE_PENDING.remove(deps.storage, &wallet);
        INDEX.save(deps.storage, &wallet, &(state.race, state.participants))?;
        CYCLE_PLAYERS.update(
            deps.storage,
            (state.cycle, &wallet),
            |old| -> StdResult<_> {
                let mut entry = old.unwrap_or_default();
                entry.tier_sum = entry.tier_sum.checked_add(Uint128::from(tier_sum))?;
                entry.weighted_sum = entry.weighted_sum.checked_add(weighted_tiers(tier_sum)?)?;
                entry.sponsor = fleet.sponsor;
                entry.clan = clan_id;
                Ok(entry)
            },
        )?;
        state.tier_sum = state.tier_sum.checked_add(Uint128::from(tier_sum))?;
    }
    state.snapshot_after = wallets.last().cloned().or(state.snapshot_after.clone());
    if wallets.len() < usize::from(limit) {
        if state.participants == 0 {
            state.phase = Phase::Boundary;
        } else {
            state.start_height = env
                .block
                .height
                .checked_add(1)
                .ok_or_else(|| fail("height overflow"))?;
            state.phase = Phase::Waiting;
        }
    }
    Ok(response.add_attribute("participants", state.participants.to_string()))
}

fn project_to(config: &Config, player: &mut Player, tick: u32) -> StdResult<()> {
    if config.accounting_version >= 2 {
        motion::catch_up(player, tick, &config.parameters)
    } else {
        motion::catch_up_legacy(player, tick, &config.parameters)
    }
}

fn update_motion(
    storage: &mut dyn cosmwasm_std::Storage,
    config: &Config,
    state: &State,
    index: u16,
    player: &Player,
) -> StdResult<()> {
    if state.registry_enabled {
        motion::update_persistent(storage, state, index, player, &config.parameters)
    } else if config.accounting_version >= 2 {
        motion::update_deferred(storage, state.race, index, player, &config.parameters)
    } else {
        motion::update(storage, state.race, index, player, &config.parameters)
    }
}

fn ending_check(
    storage: &dyn cosmwasm_std::Storage,
    env: &Env,
    config: &Config,
    state: &mut State,
    response: Response,
) -> StdResult<Response> {
    if !stops_here(env, state, &config.parameters)? {
        state.tick += 1;
        return Ok(response.add_attribute("preview_tick", (state.tick - 1).to_string()));
    }
    state.replay_until = state.tick;
    state.tick = next_event(storage, state, 0)?;
    Ok(response
        .add_attribute("race_closing", state.race.to_string())
        .add_attribute("ending_tick", state.replay_until.to_string()))
}

fn stops_here(env: &Env, state: &State, params: &Parameters) -> StdResult<bool> {
    let prior = if state.tick == 1 {
        0
    } else {
        params.duration_cdf[state.tick as usize - 2]
    };
    let current = params.duration_cdf[state.tick as usize - 1];
    Ok(draw(
        env,
        state,
        b"race-stop",
        0,
        0,
        Uint128::from(CDF_SCALE - prior),
    )?
    .u128()
        < u128::from(current - prior))
}

fn next_event(storage: &dyn cosmwasm_std::Storage, state: &State, after: u32) -> StdResult<u32> {
    let next = motion::ACTIONS
        .sub_prefix(state.race)
        .keys(
            storage,
            Some(Bound::exclusive((after, u16::MAX))),
            None,
            Order::Ascending,
        )
        .next()
        .transpose()?
        .map(|(tick, _)| tick)
        .unwrap_or(state.replay_until);
    Ok(next.min(state.replay_until))
}

fn priced_rocket(config: &Config, player: &Player) -> Uint128 {
    if config.accounting_version == 0 {
        player.ships[2]
            .attack
            .multiply_ratio(u128::from(config.parameters.rocket_cost_bps), SCALE * 10000)
    } else {
        rocket_price(player, &config.parameters)
    }
}

fn processed_tick(state: &State, index: u16) -> u32 {
    match state.phase {
        Phase::Waiting => state.tick.saturating_sub(1),
        Phase::Movement if index > state.cursor => state.tick.saturating_sub(1),
        Phase::Snapshot | Phase::DrainEquipment => 0,
        _ => state.tick,
    }
}

fn lazy_movement(
    deps: DepsMut,
    config: &Config,
    state: &mut State,
    limit: u16,
    response: Response,
) -> StdResult<Response> {
    let indices = motion::page(deps.storage, &motion::WORK, state, limit)?;
    for index in indices {
        let mut player = registry::load_player(deps.storage, state, index)?;
        project_to(config, &mut player, state.tick.saturating_sub(1))?;
        let boosts = BOOSTS
            .may_load(deps.storage, (state.race, state.tick, &player.wallet))?
            .unwrap_or_default();
        let repair_spent = healing::apply(deps.storage, config, state, index, &mut player)?;
        if config.accounting_version >= 2 {
            move_player(&mut player, &boosts, &config.parameters, state.tick)?;
        } else {
            move_player_legacy(&mut player, &boosts, &config.parameters, state.tick)?;
        }
        player.utility_spent = player.utility_spent.checked_add(repair_spent)?;
        player.materialized_tick = state.tick;
        if let Some(mut live) = LIVE_PENDING.may_load(deps.storage, &player.wallet)? {
            live.boosts.retain(|entry| entry.tick != state.tick);
            if live.boosts.is_empty() {
                LIVE_PENDING.remove(deps.storage, &player.wallet);
            } else {
                LIVE_PENDING.save(deps.storage, &player.wallet, &live)?;
            }
        }
        update_motion(deps.storage, config, state, index, &player)?;
        PLAYERS.save(deps.storage, index, &player)?;
        motion::WORK.remove(deps.storage, (state.race, state.tick, index));
        state.cursor = index;
    }
    if motion::page(deps.storage, &motion::WORK, state, 1)?.is_empty() {
        state.total_distance = motion::prefix(deps.storage, state, TREE_SIZE)?;
        state.phase = Phase::Combat;
        state.cursor = 0;
    }
    Ok(response)
}

fn combat(
    deps: DepsMut,
    env: &Env,
    config: &Config,
    state: &mut State,
    limit: u16,
    mut response: Response,
) -> StdResult<Response> {
    let through = state.participants.min(state.cursor + limit);
    let mut prizes = Uint128::zero();
    let mut fees = Uint128::zero();
    let lazy = config.accounting_version > 0;
    let indices = if lazy {
        motion::page(deps.storage, &motion::ACTIONS, state, limit)?
    } else {
        (state.cursor + 1..=through).collect()
    };
    let mut rocket_work = 0u16;
    'actors: for index in indices {
        let attacker = registry::load_player(deps.storage, state, index)?;
        let mut spent = if state.rocket_cursor > 0 {
            state.combat_spent
        } else {
            attacker.utility_spent
        };
        for rocket in state.rocket_cursor..attacker.salvo {
            if lazy && rocket_work >= limit {
                state.rocket_cursor = rocket;
                state.combat_spent = spent;
                break 'actors;
            }
            rocket_work += 1;
            let mut own = registry::load_player(deps.storage, state, index)?;
            if lazy {
                project_to(config, &mut own, state.tick)?;
            }
            let own_weight = weight(&own)?;
            let eligible = state.total_distance.checked_sub(own_weight)?;
            if eligible.is_zero() {
                break;
            }
            let mut offset = draw(env, state, b"rocket-target", index, rocket, eligible)?;
            let before_attacker = if lazy {
                motion::prefix(deps.storage, state, index - 1)?
            } else {
                prefix(deps.storage, state, index - 1)?
            };
            if offset >= before_attacker {
                offset = offset.checked_add(own_weight)?;
            }
            let target_index = if lazy {
                motion::find(deps.storage, state, offset)?
            } else {
                tree_find(deps.storage, state, offset)?
            };
            let mut target = registry::load_player(deps.storage, state, target_index)?;
            if lazy {
                project_to(config, &mut target, state.tick)?;
            }
            let before = target.clone();
            let old_weight = weight(&target)?;
            let critical = draw(
                env,
                state,
                b"rocket-critical",
                index,
                rocket,
                Uint128::new(10000),
            )?
            .u128()
                < u128::from(bonus(&attacker, Bonus::Critical).min(10000));
            let amount = attacker.ships[2]
                .attack
                .multiply_ratio(u128::from(config.parameters.rocket_damage_bps), 10000u128);
            let incoming = if critical { mul(amount, 2)? } else { amount };
            damage(&mut target, incoming)?;
            if config.healing.enabled && before.alive && !target.alive {
                healing::remember_death(deps.storage, state, target_index, &before)?;
            }
            let sum = |ships: &[Ship; 3], field: fn(&Ship) -> Uint128| -> StdResult<Uint128> {
                ships.iter().try_fold(Uint128::zero(), |sum, ship| {
                    Ok(sum.checked_add(field(ship))?)
                })
            };
            let hp_before = sum(&before.ships, |ship| ship.hp)?;
            let hp_after = sum(&target.ships, |ship| ship.hp)?;
            let shield_before = before.ships[1].shield;
            let shield_after = target.ships[1].shield;
            let hit = RocketHit {
                race: state.race,
                cycle: state.cycle,
                tick: state.tick,
                attacker: attacker.wallet.to_string(),
                target: target.wallet.to_string(),
                ordinal: rocket,
                critical,
                damage: incoming,
                hp_before,
                hp_after,
                hp_lost: hp_before.checked_sub(hp_after)?,
                max_hp: sum(&target.ships, |ship| ship.base_hp)?,
                shield_before,
                shield_after,
                shield_lost: shield_before.checked_sub(shield_after)?,
                destroyed: before.alive && !target.alive,
                ships: before
                    .ships
                    .iter()
                    .zip(target.ships.iter())
                    .enumerate()
                    .map(|(index, (a, b))| RocketShipDelta {
                        index: index as u8,
                        hp_before: a.hp,
                        hp_after: b.hp,
                        max_hp: b.base_hp,
                        shield_before: a.shield,
                        shield_after: b.shield,
                    })
                    .collect(),
            };
            response = response.add_attribute("rocket_hit", to_json_string(&hit)?);
            let new_weight = weight(&target)?;
            if lazy {
                update_motion(deps.storage, config, state, target_index, &target)?;
            } else {
                tree_change(deps.storage, state, target_index, old_weight, new_weight)?;
            }
            state.total_distance = state
                .total_distance
                .checked_sub(old_weight)?
                .checked_add(new_weight)?;
            PLAYERS.save(deps.storage, target_index, &target)?;
            spent = spent.checked_add(priced_rocket(config, &attacker))?;
        }
        let boosts = BOOSTS
            .may_load(deps.storage, (state.race, state.tick, &attacker.wallet))?
            .unwrap_or_default();
        let unused = boosts.paid.checked_sub(spent)?;
        if !unused.is_zero() {
            REFUNDS.update(deps.storage, &attacker.wallet, |old| -> StdResult<_> {
                Ok(old.unwrap_or_default().checked_add(unused)?)
            })?;
        }
        BOOSTS.remove(deps.storage, (state.race, state.tick, &attacker.wallet));
        healing::RESERVED.remove(deps.storage, (state.race, state.tick, &attacker.wallet));
        let fee = spent.multiply_ratio(u128::from(config.parameters.boost_fee_bps), 10000u128);
        fees = fees.checked_add(fee)?;
        prizes = prizes.checked_add(spent.checked_sub(fee)?)?;
        state.escrow_srt = state.escrow_srt.checked_sub(spent)?;
        state.cursor = index;
        state.rocket_cursor = 0;
        state.combat_spent = Uint128::zero();
        if lazy {
            motion::ACTIONS.remove(deps.storage, (state.race, state.tick, index));
        }
    }
    if !prizes.is_zero() {
        response = response.add_message(call(
            &config.srt,
            to_json_binary(&Cw20ExecuteMsg::Send {
                contract: config.rewards.to_string(),
                amount: prizes,
                msg: to_json_binary(&racer_rewards::ReceiveMsg::Fund {})?,
            })?,
        ));
    }
    if !fees.is_zero() {
        response = response.add_message(call(
            &config.srt,
            to_json_binary(&Cw20ExecuteMsg::Transfer {
                recipient: config.treasury.to_string(),
                amount: fees,
            })?,
        ));
    }
    let finished = state.rocket_cursor == 0
        && if lazy {
            motion::page(deps.storage, &motion::ACTIONS, state, 1)?.is_empty()
        } else {
            state.cursor == state.participants
        };
    if finished {
        let prior = if state.tick == 1 {
            0
        } else {
            config.parameters.duration_cdf[state.tick as usize - 2]
        };
        let current = config.parameters.duration_cdf[state.tick as usize - 1];
        let stop = draw(
            env,
            state,
            b"race-stop",
            0,
            0,
            Uint128::from(CDF_SCALE - prior),
        )?
        .u128()
            < u128::from(current - prior);
        if stop {
            let winner = if state.total_distance.is_zero() {
                None
            } else {
                let offset = draw(env, state, b"race-winner", 0, 0, state.total_distance)?;
                let winner_index = if lazy {
                    motion::find(deps.storage, state, offset)?
                } else {
                    tree_find(deps.storage, state, offset)?
                };
                let winner = registry::load_player(deps.storage, state, winner_index)?;
                let score = Uint128::from(state.multiplier).checked_mul(Uint128::new(
                    10000 + u128::from(bonus(&winner, Bonus::Reward)),
                ))?;
                let mut winner_clan = None;
                CYCLE_PLAYERS.update(
                    deps.storage,
                    (state.cycle, &winner.wallet),
                    |old| -> StdResult<_> {
                        let mut entry = old.ok_or_else(|| fail("missing cycle player"))?;
                        winner_clan = entry.clan;
                        entry.score = entry.score.checked_add(score)?;
                        Ok(entry)
                    },
                )?;
                if let Some(id) = winner_clan {
                    CLAN_CYCLES.update(deps.storage, (state.cycle, id), |old| -> StdResult<_> {
                        let mut clan = old.ok_or_else(|| fail("missing clan cycle"))?;
                        clan.score = clan.score.checked_add(score)?;
                        Ok(clan)
                    })?;
                }
                state.total_score = state.total_score.checked_add(score)?;
                Some(winner.wallet.to_string())
            };
            RESULTS.save(
                deps.storage,
                state.race,
                &RaceResult {
                    race: state.race,
                    cycle: state.cycle,
                    ticks: state.tick,
                    winner: winner.clone(),
                    multiplier: state.multiplier,
                    total_distance: state.total_distance,
                },
            )?;
            state.completed += 1;
            state.replay_until = 0;
            state.phase = if state.completed == config.parameters.races_per_cycle {
                Phase::Checkpoint
            } else {
                Phase::Boundary
            };
            response = response
                .add_attribute("race_ended", state.race.to_string())
                .add_attribute("winner", winner.unwrap_or_else(|| "none".into()));
        } else {
            state.tick = if config.accounting_version >= 2 {
                next_event(deps.storage, state, state.tick)?
            } else {
                state.tick + 1
            };
            state.cursor = 0;
            state.phase = Phase::Waiting;
        }
    }
    Ok(response)
}

fn settle(
    deps: DepsMut,
    config: &Config,
    state: &mut State,
    limit: u16,
    response: Response,
) -> StdResult<Response> {
    let start = state
        .settlement_after
        .as_ref()
        .map(|s| deps.api.addr_validate(s))
        .transpose()?;
    let rows: Vec<_> = CYCLE_PLAYERS
        .prefix(state.cycle)
        .range(
            deps.storage,
            start.as_ref().map(Bound::exclusive),
            None,
            Order::Ascending,
        )
        .take(usize::from(limit))
        .collect::<StdResult<_>>()?;
    let mut recipients = vec![];
    let mut fees = Uint128::zero();
    for (wallet, entry) in &rows {
        let share = |pool: Uint128| {
            if state.total_score.is_zero() {
                Uint128::zero()
            } else {
                pool.multiply_ratio(entry.score, state.total_score)
            }
        };
        let gross_usd = if let Some(id) = entry.clan {
            let clan = CLAN_CYCLES.load(deps.storage, (state.cycle, id))?;
            let bucket = if state.total_score.is_zero() {
                Uint128::zero()
            } else {
                state
                    .budget_usd
                    .multiply_ratio(clan.score, state.total_score)
            };
            let premium = bucket.multiply_ratio(u128::from(state.leader_bps), 10000u128);
            if clan.tiers.is_zero() {
                Uint128::zero()
            } else {
                bucket
                    .checked_sub(premium)?
                    .multiply_ratio(entry.tier_sum, clan.tiers)
            }
        } else {
            share(state.budget_usd)
        };
        let srt = share(state.budget_srt);
        let fee = maintenance(
            state.budget_usd,
            entry.weighted_sum,
            state.tier_sum,
            gross_usd,
        )?;
        let sponsor_usd = if entry.sponsor.is_some() {
            fee.multiply_ratio(
                u128::from(config.parameters.maintenance_referral_bps),
                10000u128,
            )
        } else {
            Uint128::zero()
        };
        recipients.push(Allocation {
            wallet: wallet.to_string(),
            spaceusd: gross_usd.checked_sub(fee)?,
            srt,
        });
        if let Some(sponsor) = &entry.sponsor {
            if !sponsor_usd.is_zero() {
                recipients.push(Allocation {
                    wallet: sponsor.to_string(),
                    spaceusd: sponsor_usd,
                    srt: Uint128::zero(),
                });
            }
        }
        fees = fees.checked_add(fee.checked_sub(sponsor_usd)?)?;
        SETTLEMENTS.save(
            deps.storage,
            (state.cycle, wallet),
            &Settlement {
                gross_usd,
                srt,
                maintenance: fee,
                sponsor_usd,
            },
        )?;
    }
    let mut response = response;
    if !recipients.is_empty() {
        response = response.add_message(call(
            &config.rewards,
            to_json_binary(&racer_rewards::ExecuteMsg::Allocate { recipients })?,
        ));
    }
    if !fees.is_zero() {
        response = response.add_message(call(
            &config.rewards,
            to_json_binary(&racer_rewards::ExecuteMsg::PayMaintenance {
                amount: fees,
                treasury: config.treasury.to_string(),
            })?,
        ));
    }
    state.settlement_after = rows
        .last()
        .map(|(wallet, _)| wallet.to_string())
        .or(state.settlement_after.clone());
    if rows.len() < usize::from(limit) {
        response = response.add_attribute("settled_cycle", state.cycle.to_string());
        state.cycle = state
            .cycle
            .checked_add(1)
            .ok_or_else(|| fail("cycle overflow"))?;
        state.completed = 0;
        state.total_score = Uint128::zero();
        state.tier_sum = Uint128::zero();
        state.phase = Phase::Boundary;
    }
    Ok(response)
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    if let QueryMsg::Multiplier { race } = msg {
        return to_json_binary(&committed_multiplier(
            deps,
            &env,
            &CONFIG.load(deps.storage)?,
            &STATE.load(deps.storage)?,
            race,
        )?);
    }
    if let QueryMsg::LivePlayer { ref wallet } = msg {
        return to_json_binary(&live_player(
            deps,
            &env,
            &CONFIG.load(deps.storage)?,
            &STATE.load(deps.storage)?,
            wallet,
        )?);
    }
    match msg {
        QueryMsg::HealQuote { wallet } => {
            let config = CONFIG.load(deps.storage)?;
            if !config.healing.enabled {
                return Err(fail("healing disabled"));
            }
            let player: Player = from_json(query(deps, env, QueryMsg::Player { wallet })?)?;
            to_json_binary(&healing::quote(&player, &config.healing)?)
        }
        QueryMsg::TickBeacon { race, tick } => {
            let beacon = INLINE_BEACONS
                .may_load(deps.storage, (race, tick))?
                .map(|b| -> StdResult<_> {
                    Ok(racer_randomness::Request {
                        round: b.round,
                        available_at: b.available_at,
                        randomness: VERIFIED_ROUNDS.may_load(deps.storage, (race, b.round))?,
                    })
                })
                .transpose()?;
            to_json_binary(&beacon)
        }
        QueryMsg::TickSeed { race, tick } => {
            if let Some(beacon) = INLINE_BEACONS.may_load(deps.storage, (race, tick))? {
                return to_json_binary(
                    &VERIFIED_ROUNDS.may_load(deps.storage, (race, beacon.round))?,
                );
            }
            let config = CONFIG.load(deps.storage)?;
            let seed = if let Some(id) = SEEDS.may_load(deps.storage, (race, tick))? {
                let request: racer_randomness::Request = deps.querier.query_wasm_smart(
                    config.randomness,
                    &racer_randomness::QueryMsg::Request {
                        consumer: env.contract.address.to_string(),
                        id,
                    },
                )?;
                request
                    .randomness
                    .or(VERIFIED_ROUNDS.may_load(deps.storage, (race, request.round))?)
            } else {
                None
            };
            to_json_binary(&seed)
        }
        QueryMsg::Players { start_after, limit } => {
            if limit == 0 || limit > MAX_BATCH {
                return Err(fail("invalid player page"));
            }
            let state = STATE.load(deps.storage)?;
            if state.registry_enabled {
                let indices = registry::TEMPLATES
                    .keys(
                        deps.storage,
                        start_after.map(Bound::exclusive),
                        None,
                        Order::Ascending,
                    )
                    .take(usize::from(limit))
                    .collect::<StdResult<Vec<_>>>()?;
                return to_json_binary(
                    &indices
                        .into_iter()
                        .map(|index| {
                            Ok((index, registry::load_player(deps.storage, &state, index)?))
                        })
                        .collect::<StdResult<Vec<_>>>()?,
                );
            }
            to_json_binary(
                &PLAYERS
                    .range(
                        deps.storage,
                        start_after.map(Bound::exclusive),
                        None,
                        Order::Ascending,
                    )
                    .take(usize::from(limit))
                    .collect::<StdResult<Vec<_>>>()?,
            )
        }
        QueryMsg::Multiplier { .. } | QueryMsg::LivePlayer { .. } => unreachable!(),
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::State {} => to_json_binary(&STATE.load(deps.storage)?),
        QueryMsg::RocketPrice { wallet } => {
            let addr = deps.api.addr_validate(&wallet)?;
            let (race, index) = INDEX.load(deps.storage, &addr)?;
            let state = STATE.load(deps.storage)?;
            let player = registry::load_player(deps.storage, &state, index)?;
            if (!state.registry_enabled && player.race != race) || player.wallet != addr {
                return Err(fail("player no longer in active snapshot"));
            }
            to_json_binary(&priced_rocket(&CONFIG.load(deps.storage)?, &player))
        }
        QueryMsg::Player { wallet } => {
            let addr = deps.api.addr_validate(&wallet)?;
            let (race, index) = INDEX.load(deps.storage, &addr)?;
            let state = STATE.load(deps.storage)?;
            let mut player = registry::load_player(deps.storage, &state, index)?;
            if (!state.registry_enabled && player.race != race) || player.wallet != addr {
                return Err(fail("player no longer in active snapshot"));
            }
            let config = CONFIG.load(deps.storage)?;
            if config.accounting_version > 0 && player.race == state.race {
                project_to(&config, &mut player, processed_tick(&state, index))?;
            }
            to_json_binary(&player)
        }
        QueryMsg::Result { race } => to_json_binary(&RESULTS.load(deps.storage, race)?),
        QueryMsg::CyclePlayer { cycle, wallet } => to_json_binary(&registry::cycle_player(
            deps,
            &STATE.load(deps.storage)?,
            cycle,
            &deps.api.addr_validate(&wallet)?,
        )?),
        QueryMsg::Settlement { cycle, wallet } => to_json_binary(
            &SETTLEMENTS.load(deps.storage, (cycle, &deps.api.addr_validate(&wallet)?))?,
        ),
        QueryMsg::SeedRequest { race, tick } => {
            to_json_binary(&SEEDS.may_load(deps.storage, (race, tick))?)
        }
    }
}
