use cosmwasm_std::{
    to_json_binary, Addr, Binary, Deps, DepsMut, Empty, Env, MessageInfo, Response, StdResult,
    Uint128,
};
use cw20::{BalanceResponse, Cw20Coin, Cw20ExecuteMsg, Cw20QueryMsg};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use cw_storage_plus::Map;
use racer_race::state::{Phase, Player, RaceResult, Settlement, State};
use racer_race::{BoostKind, BoostMsg, ExecuteMsg, InstantiateMsg, Parameters, QueryMsg};

fn token() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    ))
}
fn fleet() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_fleet::execute,
        racer_fleet::instantiate,
        racer_fleet::query,
    ))
}
fn rewards() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_rewards::execute,
        racer_rewards::instantiate,
        racer_rewards::query,
    ))
}
fn equipment() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_equipment::execute,
        racer_equipment::instantiate,
        racer_equipment::query,
    ))
}
fn race() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new(
            racer_race::execute,
            racer_race::instantiate,
            racer_race::query,
        )
        .with_migrate(racer_race::migrate),
    )
}

fn legacy_instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    let result = racer_race::instantiate(deps.branch(), env, info, msg)?;
    racer_race::state::CONFIG.update(deps.storage, |mut config| -> StdResult<_> {
        config.accounting_version = 0;
        Ok(config)
    })?;
    Ok(result)
}

fn eager_instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    let result = racer_race::instantiate(deps.branch(), env, info, msg)?;
    racer_race::state::CONFIG.update(deps.storage, |mut config| -> StdResult<_> {
        config.accounting_version = 1;
        Ok(config)
    })?;
    Ok(result)
}

fn v2_instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    let result = racer_race::instantiate(deps.branch(), env, info, msg)?;
    racer_race::state::CONFIG.update(deps.storage, |mut config| -> StdResult<_> {
        config.accounting_version = 2;
        Ok(config)
    })?;
    Ok(result)
}

// Test-only oracle: never part of a deployable crate. Real proofs are verified
// by racer-randomness and by the compiled-Wasm local smoke scenario.
const REQUESTS: Map<(&Addr, u64), racer_randomness::Request> = Map::new("requests");
const TIME_ROUNDS: cw_storage_plus::Item<bool> = cw_storage_plus::Item::new("time_rounds");
fn time_oracle_instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: Empty,
) -> StdResult<Response> {
    TIME_ROUNDS.save(deps.storage, &true)?;
    Ok(Response::new())
}
fn mock_verifier_query(
    _deps: Deps,
    _env: Env,
    msg: racer_beacon_verifier::QueryMsg,
) -> StdResult<Binary> {
    match msg {
        racer_beacon_verifier::QueryMsg::Protocol {} => {
            to_json_binary(&racer_beacon_verifier::protocol())
        }
        racer_beacon_verifier::QueryMsg::Verify {
            round: _,
            signature,
        } => {
            if signature.len() != 48 || signature.as_slice()[0] == 0 {
                return Err(cosmwasm_std::StdError::generic_err("invalid beacon proof"));
            }
            to_json_binary(&Binary::from(signature.as_slice()[..32].to_vec()))
        }
    }
}
fn mock_instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: Empty,
) -> StdResult<Response> {
    Ok(Response::new())
}
fn mock_execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: racer_randomness::ExecuteMsg,
) -> StdResult<Response> {
    match msg {
        racer_randomness::ExecuteMsg::Request { id } => {
            let timed = TIME_ROUNDS.may_load(deps.storage)?.unwrap_or(false);
            let request = if timed {
                racer_randomness::future_request(env.block.time.seconds(), 6)?
            } else {
                racer_randomness::Request {
                    round: id,
                    available_at: env.block.time.seconds() + 6,
                    randomness: None,
                }
            };
            REQUESTS.save(deps.storage, (&info.sender, id), &request)?;
        }
        racer_randomness::ExecuteMsg::Submit {
            consumer,
            id,
            signature,
        } => {
            assert_eq!(info.sender, Addr::unchecked("owner"));
            let addr = deps.api.addr_validate(&consumer)?;
            REQUESTS.update(deps.storage, (&addr, id), |old| -> StdResult<_> {
                let mut req = old.unwrap();
                assert!(env.block.time.seconds() >= req.available_at);
                assert!(req.randomness.is_none());
                req.randomness = Some(signature);
                Ok(req)
            })?;
        }
        _ => panic!("unsupported mock operation"),
    }
    Ok(Response::new())
}
fn mock_query(deps: Deps, _env: Env, msg: racer_randomness::QueryMsg) -> StdResult<Binary> {
    match msg {
        racer_randomness::QueryMsg::Config {} => to_json_binary(&racer_randomness::Config {
            owner: Addr::unchecked("owner"),
            paused: false,
            safety_seconds: 6,
        }),
        racer_randomness::QueryMsg::Request { consumer, id } => {
            to_json_binary(&REQUESTS.load(deps.storage, (&deps.api.addr_validate(&consumer)?, id))?)
        }
    }
}

fn params(cdf: Vec<u32>) -> Parameters {
    Parameters {
        tick_seconds: 6,
        races_per_cycle: 1,
        duration_cdf: cdf,
        boost_price: Uint128::new(1_000_000),
        speed_boost: Uint128::new(10000),
        acceleration: Uint128::new(100),
        temporary_distance: Uint128::new(2_000_000),
        temporary_ticks: 50,
        rocket_cost_bps: 4000,
        rocket_damage_bps: 10000,
        boost_fee_bps: 500,
        maintenance_referral_bps: 500,
        max_actions_per_tick: 10,
        max_players: 1000,
    }
}

struct Suite {
    app: App,
    engine: Addr,
    fleet: Addr,
    rewards: Addr,
    rng: Addr,
    srt: Addr,
    usd: Addr,
    count: usize,
}
impl Suite {
    fn new(count: usize, p: Parameters) -> Self {
        Self::new_mode(count, p, false)
    }
    fn new_mode(count: usize, p: Parameters, legacy: bool) -> Self {
        Self::new_version(count, p, if legacy { 0 } else { 1 })
    }
    fn new_version(count: usize, p: Parameters, version: u8) -> Self {
        let mut app = App::default();
        if version >= 4 {
            app.update_block(|b| {
                b.time =
                    cosmwasm_std::Timestamp::from_seconds(racer_randomness::GENESIS + 1_000_000)
            });
        }
        let code = app.store_code(token());
        let accounts: Vec<_> = (0..count)
            .map(|n| Cw20Coin {
                address: format!("player{n:04}"),
                amount: Uint128::new(1_000_000_000_000),
            })
            .chain([Cw20Coin {
                address: "owner".into(),
                amount: Uint128::new(1_000_000_000_000),
            }])
            .collect();
        let mut create = |symbol: &str| {
            app.instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &cw20_base::msg::InstantiateMsg {
                    name: symbol.into(),
                    symbol: symbol.into(),
                    decimals: 6,
                    initial_balances: accounts.clone(),
                    mint: None,
                    marketing: None,
                },
                &[],
                symbol,
                None,
            )
            .unwrap()
        };
        let usd = create("USD");
        let srt = create("SRT");
        let code = app.store_code(fleet());
        let fleet = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_fleet::InstantiateMsg {
                    owner: "owner".into(),
                    engine: "owner".into(),
                    spaceusd: usd.to_string(),
                    treasury: "treasury".into(),
                    price_per_tier: Uint128::new(1_000_000),
                },
                &[],
                "fleet",
                None,
            )
            .unwrap();
        let code = app.store_code(rewards());
        let rewards = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_rewards::InstantiateMsg {
                    owner: "owner".into(),
                    allocator: "owner".into(),
                    spaceusd: usd.to_string(),
                    srt: srt.to_string(),
                    annual_bps: 2500,
                },
                &[],
                "rewards",
                None,
            )
            .unwrap();
        let code = app.store_code(equipment());
        let equipment = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_equipment::InstantiateMsg {
                    owner: "owner".into(),
                    engine: "owner".into(),
                    collections: vec![],
                },
                &[],
                "equipment",
                None,
            )
            .unwrap();
        let oracle_instantiate: fn(DepsMut, Env, MessageInfo, Empty) -> StdResult<Response> =
            if version >= 4 {
                time_oracle_instantiate
            } else {
                mock_instantiate
            };
        let code = app.store_code(Box::new(ContractWrapper::new(
            mock_execute,
            oracle_instantiate,
            mock_query,
        )));
        let rng = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &Empty {},
                &[],
                "test-only randomness",
                None,
            )
            .unwrap();
        let code = if version == 0 {
            app.store_code(Box::new(
                ContractWrapper::new(racer_race::execute, legacy_instantiate, racer_race::query)
                    .with_migrate(racer_race::migrate),
            ))
        } else if version == 1 {
            app.store_code(Box::new(
                ContractWrapper::new(racer_race::execute, eager_instantiate, racer_race::query)
                    .with_migrate(racer_race::migrate),
            ))
        } else if version == 2 {
            app.store_code(Box::new(
                ContractWrapper::new(racer_race::execute, v2_instantiate, racer_race::query)
                    .with_migrate(racer_race::migrate),
            ))
        } else {
            app.store_code(race())
        };
        let engine = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &InstantiateMsg {
                    owner: "owner".into(),
                    fleet: fleet.to_string(),
                    equipment: equipment.to_string(),
                    rewards: rewards.to_string(),
                    randomness: rng.to_string(),
                    srt: srt.to_string(),
                    treasury: "treasury".into(),
                    parameters: p,
                    resume_from: None,
                },
                &[],
                "race",
                Some("owner".into()),
            )
            .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            fleet.clone(),
            &racer_fleet::ExecuteMsg::Configure {
                engine: Some(engine.to_string()),
                treasury: None,
                price_per_tier: None,
                paused: None,
            },
            &[],
        )
        .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            equipment,
            &racer_equipment::ExecuteMsg::Configure {
                engine: Some(engine.to_string()),
                paused: None,
                collection: None,
                allowed: None,
            },
            &[],
        )
        .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            rewards.clone(),
            &racer_rewards::ExecuteMsg::Configure {
                allocator: Some(engine.to_string()),
                annual_bps: None,
                paused: None,
            },
            &[],
        )
        .unwrap();
        let mut suite = Self {
            app,
            engine,
            fleet,
            rewards,
            rng,
            srt,
            usd,
            count,
        };
        for n in 0..count {
            for ship in [racer_fleet::ShipType::Racer, racer_fleet::ShipType::Fighter] {
                suite
                    .app
                    .execute_contract(
                        Addr::unchecked(format!("player{n:04}")),
                        suite.usd.clone(),
                        &Cw20ExecuteMsg::Send {
                            contract: suite.fleet.to_string(),
                            amount: Uint128::new(1_000_000),
                            msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
                                ship,
                                target_tier: 1,
                                sponsor: None,
                            })
                            .unwrap(),
                        },
                        &[],
                    )
                    .unwrap();
            }
        }
        suite
    }
    fn state(&self) -> State {
        self.app
            .wrap()
            .query_wasm_smart(&self.engine, &QueryMsg::State {})
            .unwrap()
    }
    fn player(&self, n: usize) -> Player {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.engine,
                &QueryMsg::Player {
                    wallet: format!("player{n:04}"),
                },
            )
            .unwrap()
    }
    fn advance(&mut self, limit: u16) {
        if self.state().phase == Phase::Boundary {
            self.prepare_next();
        }
        self.app
            .execute_contract(
                Addr::unchecked("permissionless-keeper"),
                self.engine.clone(),
                &ExecuteMsg::Advance { limit },
                &[],
            )
            .unwrap();
    }
    fn prepare_next(&mut self) {
        self.app
            .execute_contract(
                Addr::unchecked("keeper"),
                self.engine.clone(),
                &ExecuteMsg::PrepareNextMultiplier {},
                &[],
            )
            .unwrap();
        let commitment: racer_race::state::MultiplierCommitment = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.engine,
                &QueryMsg::Multiplier {
                    race: self.state().race + 1,
                },
            )
            .unwrap();
        if commitment.multiplier.is_none() {
            self.app.update_block(|b| b.time = b.time.plus_seconds(10));
            self.app
                .execute_contract(
                    Addr::unchecked("owner"),
                    self.rng.clone(),
                    &racer_randomness::ExecuteMsg::Submit {
                        consumer: self.engine.to_string(),
                        id: commitment.request_id.unwrap(),
                        signature: Binary::from(vec![42; 32]),
                    },
                    &[],
                )
                .unwrap();
        }
    }
    fn start(&mut self, limit: u16) {
        for _ in 0..1100 {
            if self.state().phase == Phase::Waiting {
                let height = self.state().start_height;
                self.app.update_block(|b| b.height = height);
                return;
            }
            self.advance(limit);
        }
        panic!("snapshot did not complete");
    }
    fn boost(&mut self, n: usize, kind: BoostKind, count: u32, amount: u128) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked(format!("player{n:04}")),
                self.srt.clone(),
                &Cw20ExecuteMsg::Send {
                    contract: self.engine.to_string(),
                    amount: Uint128::new(amount),
                    msg: to_json_binary(&BoostMsg {
                        race: self.state().race,
                        kind,
                        count,
                    })
                    .unwrap(),
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn seal(&mut self) {
        let state = self.state();
        self.app.update_block(|b| {
            b.height = b.height.max(state.start_height + u64::from(state.tick));
        });
        self.app
            .execute_contract(
                Addr::unchecked("different-keeper"),
                self.engine.clone(),
                &ExecuteMsg::Seal { limit: 1 },
                &[],
            )
            .unwrap();
    }
    fn publish(&mut self, seed: u8) {
        let state = self.state();
        let id: Option<u64> = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.engine,
                &QueryMsg::SeedRequest {
                    race: state.race,
                    tick: state.tick,
                },
            )
            .unwrap();
        self.app.update_block(|b| b.time = b.time.plus_seconds(10));
        self.app
            .execute_contract(
                Addr::unchecked("owner"),
                self.rng.clone(),
                &racer_randomness::ExecuteMsg::Submit {
                    consumer: self.engine.to_string(),
                    id: id.unwrap(),
                    signature: Binary::from(vec![seed; 32]),
                },
                &[],
            )
            .unwrap();
    }
    fn resolve(&mut self, limit: u16) {
        self.advance(limit); // waiting -> movement
        for _ in 0..2200 {
            if !matches!(self.state().phase, Phase::Movement | Phase::Combat) {
                return;
            }
            self.advance(limit);
        }
        panic!("tick did not complete");
    }
    fn tick(&mut self, limit: u16, seed: u8) {
        self.seal();
        self.publish(seed);
        self.resolve(limit);
    }
    fn settle(&mut self, limit: u16) {
        for _ in 0..1100 {
            if self.state().cycle == 2 {
                return;
            }
            self.advance(limit);
        }
        panic!("settlement did not complete");
    }
    fn fund(&mut self) {
        for (token, amount) in [
            (self.usd.clone(), 100_000_000u128),
            (self.srt.clone(), 1_000_000),
        ] {
            self.app
                .execute_contract(
                    Addr::unchecked("owner"),
                    token,
                    &Cw20ExecuteMsg::Send {
                        contract: self.rewards.to_string(),
                        amount: Uint128::new(amount),
                        msg: to_json_binary(&racer_rewards::ReceiveMsg::Fund {}).unwrap(),
                    },
                    &[],
                )
                .unwrap();
        }
        self.app
            .update_block(|b| b.time = b.time.plus_seconds(game_common::YEAR_SECONDS));
    }
    fn balance(&self, token: &Addr, wallet: &str) -> Uint128 {
        let balance: BalanceResponse = self
            .app
            .wrap()
            .query_wasm_smart(
                token,
                &Cw20QueryMsg::Balance {
                    address: wallet.into(),
                },
            )
            .unwrap();
        balance.balance
    }
}

#[test]
fn healing_revives_a_destroyed_fleet_restores_distance_and_refunds_unused_reserve() {
    let mut s = Suite::new_version(2, params(vec![0, 1_000_000_000]), 3);
    let settings = racer_race::healing::Settings {
        enabled: true,
        ..Default::default()
    };
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.engine.clone(),
            &ExecuteMsg::ConfigureHealing { settings },
            &[],
        )
        .unwrap();
    s.start(20);
    s.boost(0, BoostKind::Rocket, 3, 600_000).unwrap();
    s.tick(20, 7);
    let before = s.balance(&s.srt, "player0001");
    s.boost(1, BoostKind::Heal, 1, 10_000_000).unwrap();
    assert!(s.boost(1, BoostKind::Heal, 1, 10_000_000).is_err());
    s.seal();
    s.publish(8);
    s.advance(20);
    for _ in 0..50 {
        if !matches!(
            s.state().phase,
            Phase::Waiting | Phase::Movement | Phase::Combat
        ) {
            break;
        }
        s.advance(20);
    }
    let player = s.player(1);
    assert!(player.alive);
    assert_eq!(player.ships[0].hp, player.ships[0].base_hp);
    assert_eq!(player.ships[2].hp, player.ships[2].base_hp);
    assert_eq!(player.distance, Uint128::new(1_440_000));
    assert!(
        s.state().escrow_srt > Uint128::zero() && s.state().escrow_srt < Uint128::new(10_000_000)
    );
    s.app
        .execute_contract(
            Addr::unchecked("player0001"),
            s.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![],
            },
            &[],
        )
        .unwrap();
    assert!(s.balance(&s.srt, "player0001") > before - Uint128::new(10_000_000));
    assert_eq!(s.state().escrow_srt, Uint128::zero());
}

#[test]
fn healing_does_not_exceed_the_players_authorized_reserve() {
    let mut s = Suite::new_version(2, params(vec![0, 1_000_000_000]), 3);
    let settings = racer_race::healing::Settings {
        enabled: true,
        ..Default::default()
    };
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.engine.clone(),
            &ExecuteMsg::ConfigureHealing { settings },
            &[],
        )
        .unwrap();
    s.start(20);
    s.boost(0, BoostKind::Rocket, 3, 600_000).unwrap();
    s.tick(20, 7);
    let before = s.balance(&s.srt, "player0001");
    s.boost(1, BoostKind::Heal, 1, 1).unwrap();
    s.seal();
    s.publish(8);
    s.advance(20);
    for _ in 0..50 {
        if !matches!(
            s.state().phase,
            Phase::Waiting | Phase::Movement | Phase::Combat
        ) {
            break;
        }
        s.advance(20);
    }
    assert!(!s.player(1).alive);
    s.app
        .execute_contract(
            Addr::unchecked("player0001"),
            s.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![],
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.balance(&s.srt, "player0001"), before);
}

#[test]
fn rocket_events_report_shield_absorption_and_a_fleet_takedown() {
    let mut s = Suite::new(2, params(vec![1_000_000_000]));
    s.start(20);
    s.boost(0, BoostKind::Rocket, 3, 600_000).unwrap();
    s.seal();
    s.publish(7);
    s.advance(20);
    s.advance(20);
    let response = s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::Advance { limit: 20 },
            &[],
        )
        .unwrap();
    let hits: Vec<racer_race::state::RocketHit> = response
        .events
        .iter()
        .flat_map(|e| &e.attributes)
        .filter(|a| a.key == "rocket_hit")
        .map(|a| cosmwasm_std::from_json(a.value.as_bytes()).unwrap())
        .collect();
    assert_eq!(hits.len(), 3);
    assert!(hits[2].destroyed);
    assert_eq!(hits[2].hp_after, Uint128::zero());
    assert!(!s.player(1).alive);
    let mut shielded = Suite::new(2, params(vec![1_000_000_000]));
    shielded
        .app
        .execute_contract(
            Addr::unchecked("player0001"),
            shielded.usd.clone(),
            &Cw20ExecuteMsg::Send {
                contract: shielded.fleet.to_string(),
                amount: Uint128::new(1_000_000),
                msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
                    ship: racer_fleet::ShipType::Tank,
                    target_tier: 1,
                    sponsor: None,
                })
                .unwrap(),
            },
            &[],
        )
        .unwrap();
    shielded.start(20);
    shielded.boost(0, BoostKind::Rocket, 1, 200_000).unwrap();
    shielded.seal();
    shielded.publish(7);
    shielded.advance(20);
    shielded.advance(20);
    let response = shielded
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            shielded.engine.clone(),
            &ExecuteMsg::Advance { limit: 20 },
            &[],
        )
        .unwrap();
    let hit: racer_race::state::RocketHit = response
        .events
        .iter()
        .flat_map(|e| &e.attributes)
        .find(|a| a.key == "rocket_hit")
        .map(|a| cosmwasm_std::from_json(a.value.as_bytes()).unwrap())
        .unwrap();
    assert_eq!(hit.hp_lost, Uint128::zero());
    assert_eq!(hit.shield_lost, Uint128::new(500000));
    assert_eq!(hit.shield_after, Uint128::new(500000));
}

#[test]
fn legacy_paid_actions_keep_their_price_until_lazy_upgrade_activates_at_a_race_boundary() {
    let mut s = Suite::new_mode(2, params(vec![0, 1_000_000_000]), true);
    s.start(20);
    s.boost(0, BoostKind::Rocket, 1, 200).unwrap();
    let code = s.app.store_code(race());
    s.app
        .migrate_contract(Addr::unchecked("owner"), s.engine.clone(), &Empty {}, code)
        .unwrap();
    let cfg: racer_race::state::Config = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Config {})
        .unwrap();
    assert_eq!(cfg.accounting_version, 0);
    assert!(cfg.pending_accounting_upgrade);
    let price: Uint128 = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::RocketPrice {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(price, Uint128::new(200));
    s.tick(20, 7);
    assert_eq!(s.state().escrow_srt, Uint128::zero());
    s.tick(20, 8);
    s.settle(20);
    s.start(20);
    let cfg: racer_race::state::Config = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Config {})
        .unwrap();
    assert_eq!(cfg.accounting_version, 3);
    assert!(!cfg.pending_accounting_upgrade);
    let price: Uint128 = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::RocketPrice {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(price, Uint128::new(200_000));
    s.boost(0, BoostKind::Speed, 1000, 1_000_000_000).unwrap();
}

#[test]
fn one_thousand_rockets_and_boosts_are_accepted_and_combat_is_page_invariant() {
    let mut outcomes = vec![];
    for limit in [1, 20] {
        let mut s = Suite::new_version(2, params(vec![1_000_000_000]), 3);
        s.app
            .execute_contract(
                Addr::unchecked("player0001"),
                s.usd.clone(),
                &Cw20ExecuteMsg::Send {
                    contract: s.fleet.to_string(),
                    amount: Uint128::new(1_000_000_000),
                    msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
                        ship: racer_fleet::ShipType::Tank,
                        target_tier: 1000,
                        sponsor: None,
                    })
                    .unwrap(),
                },
                &[],
            )
            .unwrap();
        s.start(20);
        let config: racer_race::state::Config = s
            .app
            .wrap()
            .query_wasm_smart(&s.engine, &QueryMsg::Config {})
            .unwrap();
        let before = s.balance(&s.srt, config.treasury.as_str());
        s.boost(0, BoostKind::Speed, 1000, 1_000_000_000).unwrap();
        s.boost(0, BoostKind::Rocket, 1000, 200_000_000).unwrap();
        s.seal();
        s.publish(7);
        let mut hits = vec![];
        let mut calls = 0;
        while matches!(
            s.state().phase,
            Phase::Waiting | Phase::Movement | Phase::Combat
        ) {
            let batch = limit == 20 && matches!(s.state().phase, Phase::Movement | Phase::Combat);
            let msg = if batch {
                ExecuteMsg::AdvanceBatch { limit, steps: 8 }
            } else {
                ExecuteMsg::Advance { limit }
            };
            let response = s
                .app
                .execute_contract(Addr::unchecked("keeper"), s.engine.clone(), &msg, &[])
                .unwrap();
            let frame: Vec<_> = response
                .events
                .iter()
                .flat_map(|event| &event.attributes)
                .filter(|a| a.key == "rocket_hit")
                .map(|a| a.value.clone())
                .collect();
            assert!(frame.len() <= usize::from(limit) * if batch { 8 } else { 1 });
            hits.extend(frame);
            calls += 1;
            assert!(calls <= 1110);
        }
        assert_eq!(hits.len(), 1000);
        assert_eq!(s.state().escrow_srt, Uint128::zero());
        assert_eq!(
            s.balance(&s.srt, config.treasury.as_str()),
            before + Uint128::new(60_000_000)
        );
        let result: racer_race::state::RaceResult = s
            .app
            .wrap()
            .query_wasm_smart(&s.engine, &QueryMsg::Result { race: 1 })
            .unwrap();
        outcomes.push((hits, s.player(0), s.player(1), result));
    }
    assert_eq!(outcomes[0], outcomes[1]);
}

#[test]
fn deferred_race_keeps_players_untouched_until_ending_and_never_regenerates_damage() {
    let mut s = Suite::new_version(2, params(vec![0, 0, 1_000_000_000]), 2);
    s.start(20);
    let before = s.player(1);
    assert!(before.ships.iter().all(|ship| ship.regen.is_zero()));
    s.boost(0, BoostKind::Rocket, 1, 200_000).unwrap();
    s.tick(20, 7);
    assert_eq!(s.state().tick, 2);
    let raw: Vec<(u16, Player)> = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::Players {
                start_after: None,
                limit: 20,
            },
        )
        .unwrap();
    assert!(raw
        .iter()
        .all(|(_, p)| p.distance.is_zero() && p.materialized_tick == 0));
    assert_eq!(s.state().escrow_srt, Uint128::new(200_000));
    s.tick(20, 8);
    assert_eq!(s.player(1).ships[2].hp, before.ships[2].hp);
    s.seal();
    s.publish(9);
    s.advance(20);
    assert_eq!(s.state().replay_until, 3);
    for _ in 0..30 {
        if !matches!(
            s.state().phase,
            Phase::Waiting | Phase::Movement | Phase::Combat
        ) {
            break;
        }
        s.advance(20);
    }
    assert_eq!(s.state().replay_until, 0);
    assert_eq!(s.state().escrow_srt, Uint128::zero());
    let after = s.player(1);
    assert_eq!(after.ships[2].hp, Uint128::new(50_000));
    assert!(after.ships.iter().all(|ship| ship.regen.is_zero()));
    let result: RaceResult = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Result { race: 1 })
        .unwrap();
    assert_eq!(result.ticks, 3);
}

#[test]
fn deferred_preview_golden_fixture() {
    #[cosmwasm_schema::cw_serde]
    struct Action {
        wallet: String,
        tick: u32,
        kind: String,
        count: String,
    }
    #[cosmwasm_schema::cw_serde]
    struct Fixture {
        chain_id: String,
        game: String,
        race: u64,
        snapshots: Vec<(u16, Player)>,
        actions: Vec<Action>,
        seeds: Vec<(u32, String)>,
        parameters: Parameters,
        players: Vec<Player>,
        hits: Vec<racer_race::state::RocketHit>,
    }
    let mut p = params(vec![0, 0, 0, 1_000_000_000]);
    p.temporary_ticks = 2;
    let mut s = Suite::new_version(8, p.clone(), 3);
    s.start(20);
    let snapshots = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::Players {
                start_after: None,
                limit: 20,
            },
        )
        .unwrap();
    let mut actions = vec![];
    let mut seeds = vec![];
    for tick in 1..=4u32 {
        let height = s.state().start_height + u64::from(tick) - 1;
        s.app.update_block(|b| b.height = height);
        let orders: Vec<(usize, BoostKind, u32, u128)> = match tick {
            1 => vec![
                (0, BoostKind::Rocket, 3, 600_000),
                (1, BoostKind::Rocket, 2, 400_000),
            ],
            2 => vec![
                (3, BoostKind::Speed, 1000, 1_000_000_000),
                (3, BoostKind::Acceleration, 7, 7_000_000),
                (3, BoostKind::Temporary, 3, 3_000_000),
            ],
            3 => vec![(4, BoostKind::Rocket, 2, 400_000)],
            _ => vec![],
        };
        for (wallet, kind, count, paid) in orders {
            let name = match kind {
                BoostKind::Heal => "heal",
                BoostKind::Rocket => "rocket",
                BoostKind::Speed => "speed",
                BoostKind::Acceleration => "acceleration",
                BoostKind::Temporary => "temporary",
            };
            actions.push(Action {
                wallet: format!("player{wallet:04}"),
                tick,
                kind: name.into(),
                count: count.to_string(),
            });
            s.boost(wallet, kind, count, paid).unwrap();
        }
        let seed = tick as u8 + 6;
        seeds.push((tick, Binary::from(vec![seed; 32]).to_base64()));
        s.tick(20, seed);
    }
    let mut hits = vec![];
    for _ in 0..100 {
        if !matches!(
            s.state().phase,
            Phase::Waiting | Phase::Movement | Phase::Combat
        ) {
            break;
        }
        let response = s
            .app
            .execute_contract(
                Addr::unchecked("keeper"),
                s.engine.clone(),
                &ExecuteMsg::Advance { limit: 2 },
                &[],
            )
            .unwrap();
        for a in response
            .events
            .iter()
            .flat_map(|e| &e.attributes)
            .filter(|a| a.key == "rocket_hit")
        {
            hits.push(cosmwasm_std::from_json(a.value.as_bytes()).unwrap());
        }
    }
    assert_eq!(s.state().replay_until, 0);
    let fixture = Fixture {
        chain_id: s.app.block_info().chain_id,
        game: s.engine.to_string(),
        race: 1,
        snapshots,
        actions,
        seeds,
        parameters: p,
        players: (0..8).map(|i| s.player(i)).collect(),
        hits,
    };
    println!(
        "PREVIEW_FIXTURE={}",
        String::from_utf8(to_json_binary(&fixture).unwrap().to_vec()).unwrap()
    );
}

#[test]
fn deferred_closing_rejects_new_actions_and_refunds_windows_after_the_ending() {
    let mut s = Suite::new_version(2, params(vec![1_000_000_000, 1_000_000_000]), 2);
    s.start(20);
    let before = s.balance(&s.srt, "player0000");
    s.seal();
    s.boost(0, BoostKind::Speed, 1, 1_000_000).unwrap();
    s.publish(4);
    s.advance(20);
    assert_eq!(s.state().replay_until, 1);
    assert!(s.boost(0, BoostKind::Speed, 1, 1_000_000).is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![2]
            },
            &[]
        )
        .is_err());
    while matches!(
        s.state().phase,
        Phase::Waiting | Phase::Movement | Phase::Combat
    ) {
        s.advance(20);
    }
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![2],
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.balance(&s.srt, "player0000"), before);
    assert!(s.state().escrow_srt.is_zero());
}

fn finish_deferred(s: &mut Suite, seed: u8) {
    s.tick(20, seed);
    for _ in 0..2000 {
        if !matches!(
            s.state().phase,
            Phase::Waiting | Phase::Movement | Phase::Combat
        ) {
            return;
        }
        s.advance(20);
    }
    panic!("deferred replay did not finish");
}

#[test]
fn registry_reuses_base_templates_and_lazily_resets_destroyed_fleets() {
    let mut p = params(vec![1_000_000_000]);
    p.races_per_cycle = 3;
    let mut s = Suite::new_version(2, p, 3);
    s.start(20);
    let template_key =
        cw_storage_plus::Map::<u16, racer_race::registry::Template>::new("registry_templates")
            .key(2)
            .to_vec();
    let base_key = cw_storage_plus::Map::<u16, racer_race::motion::Node>::new("motion_base_nodes")
        .key(16384)
        .to_vec();
    let template = s
        .app
        .wrap()
        .query_wasm_raw(&s.engine, template_key.clone())
        .unwrap();
    let base = s
        .app
        .wrap()
        .query_wasm_raw(&s.engine, base_key.clone())
        .unwrap();
    s.boost(0, BoostKind::Rocket, 3, 600_000).unwrap();
    finish_deferred(&mut s, 7);
    assert!(!s.player(1).alive);
    s.start(20);
    assert!(s.player(1).alive);
    assert!(s.player(1).ships.iter().all(|ship| ship.hp == ship.base_hp));
    assert_eq!(
        s.app
            .wrap()
            .query_wasm_raw(&s.engine, template_key)
            .unwrap(),
        template
    );
    assert_eq!(
        s.app.wrap().query_wasm_raw(&s.engine, base_key).unwrap(),
        base
    );
    let index: (u64, u16) = cosmwasm_std::from_json(
        s.app
            .wrap()
            .query_wasm_raw(
                &s.engine,
                racer_race::state::INDEX
                    .key(&Addr::unchecked("player0001"))
                    .to_vec(),
            )
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(index, (1, 2)); // Registration is persistent, not rewritten for race two.
}

fn install_test_verifier(s: &mut Suite) {
    let code = s.app.store_code(Box::new(ContractWrapper::new(
        racer_beacon_verifier::execute,
        racer_beacon_verifier::instantiate,
        mock_verifier_query,
    )));
    let verifier = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &Empty {},
            &[],
            "test-only verifier",
            None,
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.engine.clone(),
            &ExecuteMsg::SetTickVerifier {
                address: verifier.to_string(),
            },
            &[],
        )
        .unwrap();
}

#[test]
fn integrated_checks_match_legacy_rounds_endings_damage_and_rewards_without_extra_delay() {
    let mut outcomes = vec![];
    for integrated in [false, true] {
        let mut s = Suite::new_version(
            3,
            params(vec![0, 0, 1_000_000_000]),
            if integrated { 4 } else { 5 },
        );
        if integrated {
            install_test_verifier(&mut s);
        }
        s.start(20);
        s.boost(0, BoostKind::Rocket, 1, 200_000).unwrap();
        let mut rounds = vec![];
        for tick in 1..=3 {
            s.seal();
            let beacon: racer_randomness::Request = if integrated {
                s.app
                    .wrap()
                    .query_wasm_smart::<Option<racer_randomness::Request>>(
                        &s.engine,
                        &QueryMsg::TickBeacon { race: 1, tick },
                    )
                    .unwrap()
                    .unwrap()
            } else {
                let id: Option<u64> = s
                    .app
                    .wrap()
                    .query_wasm_smart(&s.engine, &QueryMsg::SeedRequest { race: 1, tick })
                    .unwrap();
                s.app
                    .wrap()
                    .query_wasm_smart(
                        &s.rng,
                        &racer_randomness::QueryMsg::Request {
                            consumer: s.engine.to_string(),
                            id: id.unwrap(),
                        },
                    )
                    .unwrap()
            };
            rounds.push((beacon.round, beacon.available_at));
            let seed = tick as u8 + 6;
            if integrated {
                let check = ExecuteMsg::CheckTick {
                    race: 1,
                    tick,
                    signature: Some(Binary::from(vec![seed; 48])),
                    seal_limit: 0,
                };
                assert!(s
                    .app
                    .execute_contract(Addr::unchecked("keeper"), s.engine.clone(), &check, &[])
                    .is_err());
                s.app.update_block(|b| b.time = b.time.plus_seconds(10));
                assert!(s
                    .app
                    .execute_contract(
                        Addr::unchecked("keeper"),
                        s.engine.clone(),
                        &ExecuteMsg::CheckTick {
                            race: 1,
                            tick,
                            signature: Some(Binary::from(vec![0; 48])),
                            seal_limit: 0
                        },
                        &[]
                    )
                    .is_err());
                let before = s.state();
                assert!(s
                    .app
                    .execute_contract(
                        Addr::unchecked("keeper"),
                        s.engine.clone(),
                        &ExecuteMsg::CheckTick {
                            race: 1,
                            tick: tick + 1,
                            signature: Some(Binary::from(vec![seed; 48])),
                            seal_limit: 0
                        },
                        &[]
                    )
                    .is_err());
                assert_eq!(s.state(), before);
                s.app
                    .execute_contract(Addr::unchecked("keeper"), s.engine.clone(), &check, &[])
                    .unwrap();
                assert!(s
                    .app
                    .execute_contract(Addr::unchecked("keeper"), s.engine.clone(), &check, &[])
                    .is_err());
            } else {
                s.publish(seed);
                s.resolve(20);
            }
        }
        assert_eq!(s.state().replay_until, 3);
        while matches!(
            s.state().phase,
            Phase::Waiting | Phase::Movement | Phase::Combat
        ) {
            s.advance(20);
        }
        let result: RaceResult = s
            .app
            .wrap()
            .query_wasm_smart(&s.engine, &QueryMsg::Result { race: 1 })
            .unwrap();
        let players = (0..3).map(|i| s.player(i)).collect::<Vec<_>>();
        s.settle(20);
        let settlements = (0..3)
            .map(|i| {
                s.app
                    .wrap()
                    .query_wasm_smart::<Settlement>(
                        &s.engine,
                        &QueryMsg::Settlement {
                            cycle: 1,
                            wallet: format!("player{i:04}"),
                        },
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        outcomes.push((rounds, result, players, settlements));
    }
    assert_eq!(outcomes[0], outcomes[1]);
}

#[test]
fn integrated_check_seals_only_closed_windows_with_the_same_future_safety() {
    let mut s = Suite::new_version(2, params(vec![0, 1_000_000_000]), 4);
    install_test_verifier(&mut s);
    s.start(20);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::Seal { limit: 1 },
            &[]
        )
        .is_err());
    s.seal();
    let original: Option<racer_randomness::Request> = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::TickBeacon { race: 1, tick: 1 })
        .unwrap();
    s.app.update_block(|b| {
        b.time = b.time.plus_seconds(10);
        b.height += 1;
    });
    let now = s.app.block_info().time.seconds();
    s.app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::CheckTick {
                race: 1,
                tick: 1,
                signature: Some(Binary::from(vec![7; 48])),
                seal_limit: 1,
            },
            &[],
        )
        .unwrap();
    let sealed: Option<racer_randomness::Request> = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::TickBeacon { race: 1, tick: 2 })
        .unwrap();
    assert!(sealed.unwrap().available_at > now + 6);
    let current: Option<racer_randomness::Request> = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::TickBeacon { race: 1, tick: 1 })
        .unwrap();
    assert_eq!(original.unwrap().round, current.unwrap().round);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::CheckTick {
                race: 1,
                tick: 2,
                signature: Some(Binary::from(vec![7; 48])),
                seal_limit: 0
            },
            &[]
        )
        .is_err());
}

#[test]
fn integrated_early_ending_does_not_seal_unused_windows_and_keeps_refunds() {
    let mut s = Suite::new_version(2, params(vec![1_000_000_000, 1_000_000_000]), 4);
    install_test_verifier(&mut s);
    s.start(20);
    let before = s.balance(&s.srt, "player0000");
    s.seal();
    s.boost(0, BoostKind::Speed, 1, 1_000_000).unwrap();
    s.app.update_block(|b| {
        b.time = b.time.plus_seconds(10);
        b.height += 1;
    });
    s.app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::CheckTick {
                race: 1,
                tick: 1,
                signature: Some(Binary::from(vec![7; 48])),
                seal_limit: 1,
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.state().replay_until, 1);
    assert_eq!(s.state().sealed, 1);
    assert_eq!(
        s.app
            .wrap()
            .query_wasm_smart::<Option<racer_randomness::Request>>(
                &s.engine,
                &QueryMsg::TickBeacon { race: 1, tick: 2 }
            )
            .unwrap(),
        None
    );
    while matches!(
        s.state().phase,
        Phase::Waiting | Phase::Movement | Phase::Combat
    ) {
        s.advance(20);
    }
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![2],
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.balance(&s.srt, "player0000"), before);
    assert!(s.state().escrow_srt.is_zero());
}

#[test]
fn tick_verifier_activation_waits_for_boundary_and_preserves_committed_multiplier() {
    let mut p = params(vec![1_000_000_000]);
    p.races_per_cycle = 2;
    let mut s = Suite::new_version(2, p, 5);
    s.start(20);
    s.prepare_next();
    let before: racer_race::state::MultiplierCommitment = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Multiplier { race: 2 })
        .unwrap();
    install_test_verifier(&mut s);
    assert!(!s.state().inline_oracle);
    finish_deferred(&mut s, 7);
    s.start(20);
    assert!(s.state().inline_oracle);
    let after: racer_race::state::MultiplierCommitment = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Multiplier { race: 2 })
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(
        s.app
            .wrap()
            .query_wasm_smart::<racer_race::state::Config>(&s.engine, &QueryMsg::Config {})
            .unwrap()
            .accounting_version,
        4
    );
}

#[test]
fn registry_checkpoint_rewards_match_v2_with_late_join_upgrades_and_referrals() {
    let mut outcomes = vec![];
    for version in [2, 3, 4] {
        let mut p = params(vec![1_000_000_000]);
        p.races_per_cycle = 4;
        let mut s = Suite::new_version(2, p, if version == 4 { 2 } else { version });
        s.fund();
        let mut results = vec![];
        for race in 1..=4 {
            s.start(20);
            if race == 1 {
                s.app
                    .execute_contract(
                        Addr::unchecked("player0000"),
                        s.usd.clone(),
                        &Cw20ExecuteMsg::Send {
                            contract: s.fleet.to_string(),
                            amount: Uint128::new(4_000_000),
                            msg: to_json_binary(&racer_fleet::ReceiveMsg::Upgrade {
                                ship: racer_fleet::ShipType::Fighter,
                                target_tier: 5,
                            })
                            .unwrap(),
                        },
                        &[],
                    )
                    .unwrap();
                s.app
                    .execute_contract(
                        Addr::unchecked("owner"),
                        s.usd.clone(),
                        &Cw20ExecuteMsg::Transfer {
                            recipient: "zzlate".into(),
                            amount: Uint128::new(20_000_000),
                        },
                        &[],
                    )
                    .unwrap();
                s.app
                    .execute_contract(
                        Addr::unchecked("zzlate"),
                        s.usd.clone(),
                        &Cw20ExecuteMsg::Send {
                            contract: s.fleet.to_string(),
                            amount: Uint128::new(2_850_000),
                            msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
                                ship: racer_fleet::ShipType::Racer,
                                target_tier: 3,
                                sponsor: Some("player0000".into()),
                            })
                            .unwrap(),
                        },
                        &[],
                    )
                    .unwrap();
                s.app
                    .execute_contract(
                        Addr::unchecked("zzlate"),
                        s.usd.clone(),
                        &Cw20ExecuteMsg::Send {
                            contract: s.fleet.to_string(),
                            amount: Uint128::new(2_000_000),
                            msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
                                ship: racer_fleet::ShipType::Fighter,
                                target_tier: 2,
                                sponsor: None,
                            })
                            .unwrap(),
                        },
                        &[],
                    )
                    .unwrap();
                assert_eq!(s.player(0).tiers[2], 1);
            }
            if race == 2 {
                assert_eq!(s.player(0).tiers[2], 5);
                let entry: racer_race::state::CyclePlayer = s
                    .app
                    .wrap()
                    .query_wasm_smart(
                        &s.engine,
                        &QueryMsg::CyclePlayer {
                            cycle: 1,
                            wallet: "player0000".into(),
                        },
                    )
                    .unwrap();
                assert_eq!(entry.tier_sum, Uint128::new(8));
                s.app
                    .execute_contract(
                        Addr::unchecked("player0001"),
                        s.usd.clone(),
                        &Cw20ExecuteMsg::Send {
                            contract: s.fleet.to_string(),
                            amount: Uint128::new(2_000_000),
                            msg: to_json_binary(&racer_fleet::ReceiveMsg::Upgrade {
                                ship: racer_fleet::ShipType::Racer,
                                target_tier: 3,
                            })
                            .unwrap(),
                        },
                        &[],
                    )
                    .unwrap();
            }
            finish_deferred(&mut s, race as u8 + 7);
            results.push(
                s.app
                    .wrap()
                    .query_wasm_smart::<RaceResult>(&s.engine, &QueryMsg::Result { race })
                    .unwrap(),
            );
            if version == 4 && race == 1 {
                let code = s.app.store_code(crate::race());
                s.app
                    .migrate_contract(Addr::unchecked("owner"), s.engine.clone(), &Empty {}, code)
                    .unwrap();
            }
        }
        assert_eq!(s.state().tier_sum, Uint128::new(47));
        s.settle(20);
        let entries: Vec<racer_race::state::CyclePlayer> = ["player0000", "player0001", "zzlate"]
            .iter()
            .map(|w| {
                s.app
                    .wrap()
                    .query_wasm_smart(
                        &s.engine,
                        &QueryMsg::CyclePlayer {
                            cycle: 1,
                            wallet: (*w).into(),
                        },
                    )
                    .unwrap()
            })
            .collect();
        assert_eq!(
            entries
                .iter()
                .map(|e| e.tier_sum.u128())
                .collect::<Vec<_>>(),
            vec![20, 12, 15]
        );
        let settlements: Vec<Settlement> = ["player0000", "player0001", "zzlate"]
            .iter()
            .map(|w| {
                s.app
                    .wrap()
                    .query_wasm_smart(
                        &s.engine,
                        &QueryMsg::Settlement {
                            cycle: 1,
                            wallet: (*w).into(),
                        },
                    )
                    .unwrap()
            })
            .collect();
        outcomes.push((results, entries, settlements));
    }
    assert_eq!(outcomes[0], outcomes[1]);
    assert_eq!(outcomes[0], outcomes[2]);
}

#[test]
fn registry_defers_first_purchases_during_a_paginated_launch() {
    let mut p = params(vec![1_000_000_000]);
    p.races_per_cycle = 2;
    let mut s = Suite::new_version(21, p, 3);
    s.advance(20);
    s.advance(20);
    s.advance(20);
    assert_eq!(s.state().phase, Phase::Snapshot);
    for wallet in ["aaa_new", "zzz_new"] {
        s.app
            .execute_contract(
                Addr::unchecked("owner"),
                s.usd.clone(),
                &Cw20ExecuteMsg::Transfer {
                    recipient: wallet.into(),
                    amount: Uint128::new(2_000_000),
                },
                &[],
            )
            .unwrap();
        for ship in [racer_fleet::ShipType::Racer, racer_fleet::ShipType::Fighter] {
            s.app
                .execute_contract(
                    Addr::unchecked(wallet),
                    s.usd.clone(),
                    &Cw20ExecuteMsg::Send {
                        contract: s.fleet.to_string(),
                        amount: Uint128::new(1_000_000),
                        msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
                            ship,
                            target_tier: 1,
                            sponsor: None,
                        })
                        .unwrap(),
                    },
                    &[],
                )
                .unwrap();
        }
    }
    s.start(20);
    assert_eq!(s.state().participants, 21);
    assert!(s
        .app
        .wrap()
        .query_wasm_smart::<Player>(
            &s.engine,
            &QueryMsg::Player {
                wallet: "zzz_new".into()
            }
        )
        .is_err());
    finish_deferred(&mut s, 7);
    s.start(20);
    assert_eq!(s.state().participants, 23);
    for wallet in ["aaa_new", "zzz_new"] {
        let entry: racer_race::state::CyclePlayer = s
            .app
            .wrap()
            .query_wasm_smart(
                &s.engine,
                &QueryMsg::CyclePlayer {
                    cycle: 1,
                    wallet: wallet.into(),
                },
            )
            .unwrap();
        assert_eq!(entry.tier_sum, Uint128::new(2));
    }
    assert_eq!(s.state().tier_sum, Uint128::new(88));
}

#[test]
fn registry_refreshes_bound_and_unbound_commander_stats_only_at_the_boundary() {
    let mut p = params(vec![1_000_000_000]);
    p.races_per_cycle = 3;
    let mut s = Suite::new_version(2, p, 3);
    let code = s.app.store_code(Box::new(ContractWrapper::new(
        racer_collection::execute,
        racer_collection::instantiate,
        racer_collection::query,
    )));
    let collection = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_collection::InstantiateMsg {
                name: "Commanders".into(),
                symbol: "CMD".into(),
                minter: "owner".into(),
                owner: "owner".into(),
            },
            &[],
            "commanders",
            None,
        )
        .unwrap();
    let config: racer_race::state::Config = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Config {})
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            config.equipment.clone(),
            &racer_equipment::ExecuteMsg::Configure {
                engine: None,
                paused: None,
                collection: Some(collection.to_string()),
                allowed: Some(true),
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            collection.clone(),
            &racer_collection::ExecuteMsg::Mint {
                token_id: "speed".into(),
                owner: "player0000".into(),
                token_uri: None,
                extension: racer_collection::Attributes {
                    code: "speed".into(),
                    kind: racer_collection::Kind::Commander,
                    rarity: racer_collection::Rarity::Common,
                    bonus: racer_collection::Bonus::Speed,
                    bonus_bps: 300,
                },
            },
            &[],
        )
        .unwrap();
    s.start(20);
    let original = s.player(0).safe_speed;
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            collection,
            &racer_collection::ExecuteMsg::SendNft {
                contract: config.equipment.to_string(),
                token_id: "speed".into(),
                msg: to_json_binary(&racer_equipment::BindMsg {
                    slot: racer_equipment::Slot::Commander,
                })
                .unwrap(),
            },
            &[],
        )
        .unwrap();
    assert!(s.player(0).commander.is_none());
    finish_deferred(&mut s, 7);
    s.start(20);
    assert!(s.player(0).commander.is_some());
    assert!(s.player(0).safe_speed > original);
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            config.equipment,
            &racer_equipment::ExecuteMsg::Unbind {
                slot: racer_equipment::Slot::Commander,
            },
            &[],
        )
        .unwrap();
    assert!(s.player(0).commander.is_some());
    finish_deferred(&mut s, 8);
    s.start(20);
    assert!(s.player(0).commander.is_none());
    assert_eq!(s.player(0).safe_speed, original);
}

#[test]
fn rocket_events_identify_the_actual_target_and_exact_hp_and_shield_loss() {
    let mut s = Suite::new(2, params(vec![1_000_000_000]));
    s.start(20);
    s.boost(0, BoostKind::Rocket, 1, 200_000).unwrap();
    s.seal();
    s.publish(7);
    s.advance(20);
    s.advance(20);
    let response = s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::Advance { limit: 20 },
            &[],
        )
        .unwrap();
    let hits: Vec<racer_race::state::RocketHit> = response
        .events
        .iter()
        .flat_map(|e| &e.attributes)
        .filter(|a| a.key == "rocket_hit")
        .map(|a| cosmwasm_std::from_json(a.value.as_bytes()).unwrap())
        .collect();
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.attacker, "player0000");
    assert_eq!(hit.target, "player0001");
    assert_eq!(
        hit.hp_before.checked_sub(hit.hp_after).unwrap(),
        hit.hp_lost
    );
    assert_eq!(
        hit.shield_before.checked_sub(hit.shield_after).unwrap(),
        hit.shield_lost
    );
    assert_eq!(hit.hp_lost, Uint128::new(500000));
    assert_eq!(hit.shield_lost, Uint128::zero());
    assert_eq!(
        hit.hp_after,
        s.player(1)
            .ships
            .iter()
            .fold(Uint128::zero(), |sum, ship| sum + ship.hp)
    );
    assert_eq!(hit.ships[2].hp_after, s.player(1).ships[2].hp);
    assert!(!hit.destroyed);
}

#[test]
fn combat_event_upgrade_requires_admin_and_preserves_running_race_state() {
    let mut s = Suite::new(2, params(vec![0, 1_000_000_000]));
    s.start(20);
    s.boost(0, BoostKind::Speed, 1, 1_000_000).unwrap();
    let before = s.state();
    let player = s.player(0);
    let code = s.app.store_code(race());
    assert!(s
        .app
        .migrate_contract(
            Addr::unchecked("attacker"),
            s.engine.clone(),
            &Empty {},
            code
        )
        .is_err());
    s.app
        .migrate_contract(Addr::unchecked("owner"), s.engine.clone(), &Empty {}, code)
        .unwrap();
    assert_eq!(s.state(), before);
    assert_eq!(s.player(0), player);
    let live: racer_race::state::LivePlayer = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::LivePlayer {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(live.pending[0].speed, 1);
}

#[test]
fn multiplier_is_committed_once_and_visible_before_the_first_race_tick() {
    let mut s = Suite::new(2, params(vec![1_000_000_000]));
    s.prepare_next();
    let commitment: racer_race::state::MultiplierCommitment = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Multiplier { race: 1 })
        .unwrap();
    let next_id = s.state().next_request;
    s.app
        .execute_contract(
            Addr::unchecked("another-keeper"),
            s.engine.clone(),
            &ExecuteMsg::PrepareNextMultiplier {},
            &[],
        )
        .unwrap();
    assert_eq!(s.state().next_request, next_id);
    s.start(20);
    assert_eq!(s.state().multiplier, commitment.multiplier.unwrap());
    assert!(s.state().multiplier > 0);
    assert_eq!(s.state().sealed, 0);
    let tick_seed: Option<u64> = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::SeedRequest { race: 1, tick: 1 })
        .unwrap();
    assert_eq!(tick_seed, None);
    s.app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::PrepareNextMultiplier {},
            &[],
        )
        .unwrap();
    let next: racer_race::state::MultiplierCommitment = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Multiplier { race: 2 })
        .unwrap();
    assert!(next.request_id.is_some());
    assert_eq!(next.multiplier, None);
    assert_eq!(
        s.app
            .wrap()
            .query_wasm_smart::<racer_race::state::MultiplierCommitment>(
                &s.engine,
                &QueryMsg::Multiplier { race: 1 }
            )
            .unwrap(),
        commitment
    );
}

#[test]
fn utility_power_activates_in_the_payment_block_without_a_tick_proof_or_double_application() {
    let mut s = Suite::new(2, params(vec![0, 1_000_000_000]));
    s.start(20);
    let initial = s.player(0);
    s.boost(0, BoostKind::Speed, 1, 1_000_000).unwrap();
    s.boost(0, BoostKind::Acceleration, 2, 2_000_000).unwrap();
    s.boost(0, BoostKind::Temporary, 3, 3_000_000).unwrap();
    let live: racer_race::state::LivePlayer = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::LivePlayer {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(
        live.player.safe_speed,
        initial.safe_speed + Uint128::new(10_000_000)
    );
    assert_eq!(live.player.acceleration_count, 2);
    assert_eq!(
        live.player.temporary[0].distance,
        Uint128::new(6_000_000_000)
    );
    assert_eq!(live.pending.len(), 1);
    assert_eq!(s.player(0), initial); // Earlier unresolved ticks are not rewritten.
    assert_eq!(s.state().sealed, 0);
    s.tick(20, 5);
    let settled = s.player(0);
    let live: racer_race::state::LivePlayer = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::LivePlayer {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert!(live.pending.is_empty());
    assert_eq!(live.player.safe_speed, settled.safe_speed);
    assert_eq!(live.player.temporary, settled.temporary);
    assert_eq!(settled.acceleration_count, 2);
}

#[test]
fn immediate_live_power_cannot_bias_an_earlier_ending_and_remains_refundable() {
    let mut a = Suite::new(2, params(vec![1_000_000_000, 1_000_000_000]));
    let mut b = Suite::new(2, params(vec![1_000_000_000, 1_000_000_000]));
    for s in [&mut a, &mut b] {
        s.start(20);
        s.seal();
    }
    let before = a.balance(&a.srt, "player0000");
    a.boost(0, BoostKind::Temporary, 10, 10_000_000).unwrap();
    let live: racer_race::state::LivePlayer = a
        .app
        .wrap()
        .query_wasm_smart(
            &a.engine,
            &QueryMsg::LivePlayer {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(
        live.player.temporary[0].distance,
        Uint128::new(20_000_000_000)
    );
    for s in [&mut a, &mut b] {
        s.publish(4);
        s.resolve(20);
    }
    let result = |s: &Suite| {
        s.app
            .wrap()
            .query_wasm_smart::<RaceResult>(&s.engine, &QueryMsg::Result { race: 1 })
            .unwrap()
    };
    assert_eq!(result(&a), result(&b));
    let live: racer_race::state::LivePlayer = a
        .app
        .wrap()
        .query_wasm_smart(
            &a.engine,
            &QueryMsg::LivePlayer {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert!(live.pending.is_empty());
    a.app
        .execute_contract(
            Addr::unchecked("player0000"),
            a.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![2],
            },
            &[],
        )
        .unwrap();
    assert_eq!(a.balance(&a.srt, "player0000"), before);
}

#[test]
fn replacement_engine_restores_cycle_points_and_keeps_existing_fleet_and_reward_balances() {
    let mut p = params(vec![1_000_000_000]);
    p.races_per_cycle = 2;
    let mut s = Suite::new(2, p);
    s.fund();
    s.start(20);
    s.tick(20, 7);
    assert_eq!(s.state().phase, Phase::Boundary);
    let prior = s.engine.clone();
    let old_state = s.state();
    let config: racer_race::state::Config = s
        .app
        .wrap()
        .query_wasm_smart(&prior, &QueryMsg::Config {})
        .unwrap();
    let entries: Vec<racer_race::state::CyclePlayer> = (0..2)
        .map(|n| {
            s.app
                .wrap()
                .query_wasm_smart(
                    &prior,
                    &QueryMsg::CyclePlayer {
                        cycle: 1,
                        wallet: format!("player{n:04}"),
                    },
                )
                .unwrap()
        })
        .collect();
    let backing = s.balance(&s.usd, s.rewards.as_str());
    let code = s.app.store_code(race());
    let replacement = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &InstantiateMsg {
                owner: "owner".into(),
                fleet: s.fleet.to_string(),
                equipment: config.equipment.to_string(),
                rewards: s.rewards.to_string(),
                randomness: s.rng.to_string(),
                srt: s.srt.to_string(),
                treasury: config.treasury.to_string(),
                parameters: config.parameters,
                resume_from: Some(prior.to_string()),
            },
            &[],
            "replacement",
            None,
        )
        .unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            replacement.clone(),
            &ExecuteMsg::Restore { limit: 1 },
            &[]
        )
        .is_err());
    for _ in 0..3 {
        s.app
            .execute_contract(
                Addr::unchecked("owner"),
                replacement.clone(),
                &ExecuteMsg::Restore { limit: 1 },
                &[],
            )
            .unwrap();
    }
    let restored: State = s
        .app
        .wrap()
        .query_wasm_smart(&replacement, &QueryMsg::State {})
        .unwrap();
    assert_eq!(restored.phase, Phase::Boundary);
    assert_eq!(restored.completed, old_state.completed);
    assert_eq!(restored.total_score, old_state.total_score);
    for (n, entry) in entries.iter().enumerate() {
        let copied: racer_race::state::CyclePlayer = s
            .app
            .wrap()
            .query_wasm_smart(
                &replacement,
                &QueryMsg::CyclePlayer {
                    cycle: 1,
                    wallet: format!("player{n:04}"),
                },
            )
            .unwrap();
        assert_eq!(&copied, entry);
    }
    assert_eq!(s.balance(&s.usd, s.rewards.as_str()), backing);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.fleet.clone(),
            &racer_fleet::ExecuteMsg::Configure {
                engine: Some(replacement.to_string()),
                treasury: None,
                price_per_tier: None,
                paused: None,
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            config.equipment,
            &racer_equipment::ExecuteMsg::Configure {
                engine: Some(replacement.to_string()),
                paused: None,
                collection: None,
                allowed: None,
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.rewards.clone(),
            &racer_rewards::ExecuteMsg::Configure {
                allocator: Some(replacement.to_string()),
                annual_bps: None,
                paused: None,
            },
            &[],
        )
        .unwrap();
    s.engine = replacement;
    s.start(20);
    assert_eq!(s.state().race, 2);
    s.tick(20, 8);
    s.settle(20);
    assert_eq!(s.state().cycle, 2);
}

#[test]
fn results_combat_fees_and_settlement_do_not_depend_on_batch_size() {
    let mut small = Suite::new(5, params(vec![0, 1_000_000_000]));
    let mut large = Suite::new(5, params(vec![0, 1_000_000_000]));
    for (s, limit) in [(&mut small, 1), (&mut large, 20)] {
        s.fund();
        s.start(limit);
        s.boost(0, BoostKind::Speed, 1, 1_000_000).unwrap();
        s.boost(1, BoostKind::Rocket, 2, 400_000).unwrap();
        s.tick(limit, 7);
        s.tick(limit, 9);
    }
    let result = |s: &Suite| -> RaceResult {
        s.app
            .wrap()
            .query_wasm_smart(&s.engine, &QueryMsg::Result { race: 1 })
            .unwrap()
    };
    assert_eq!(result(&small), result(&large));
    for n in 0..5 {
        assert_eq!(small.player(n), large.player(n));
    }
    small.settle(1);
    large.settle(20);
    for n in 0..5 {
        let wallet = format!("player{n:04}");
        let credit = |s: &Suite| -> racer_rewards::Credit {
            s.app
                .wrap()
                .query_wasm_smart(
                    &s.rewards,
                    &racer_rewards::QueryMsg::Credit {
                        wallet: wallet.clone(),
                    },
                )
                .unwrap()
        };
        assert_eq!(credit(&small), credit(&large));
    }
    assert_eq!(
        small.balance(&small.srt, "treasury"),
        large.balance(&large.srt, "treasury")
    );
    assert_eq!(
        small.balance(&small.usd, "treasury"),
        large.balance(&large.usd, "treasury")
    );
    assert!(small.state().escrow_srt.is_zero());
}

#[test]
fn later_block_boosts_are_refundable_when_an_earlier_tick_ends_the_race() {
    let mut s = Suite::new(2, params(vec![1_000_000_000, 1_000_000_000]));
    s.start(20);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::Seal { limit: 1 },
            &[]
        )
        .is_err());
    let before = s.balance(&s.srt, "player0000");
    assert!(s.boost(0, BoostKind::Speed, 1, 1).is_err());
    assert_eq!(s.balance(&s.srt, "player0000"), before);
    s.seal(); // first tick closed; current block belongs to tick two
    s.boost(0, BoostKind::Speed, 1, 1_000_000).unwrap();
    s.publish(4);
    s.resolve(20);
    assert_eq!(s.state().phase, Phase::Checkpoint);
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![2, 2],
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.balance(&s.srt, "player0000"), before);
    assert!(s.state().escrow_srt.is_zero());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![2]
            },
            &[]
        )
        .is_err());
}

#[test]
fn permanent_upgrades_wait_for_next_race_and_maintenance_is_capped() {
    let mut p = params(vec![1_000_000_000]);
    p.races_per_cycle = 2;
    let mut s = Suite::new(2, p);
    s.fund();
    s.start(20);
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.usd.clone(),
            &Cw20ExecuteMsg::Send {
                contract: s.fleet.to_string(),
                amount: Uint128::new(99_000_000),
                msg: to_json_binary(&racer_fleet::ReceiveMsg::Upgrade {
                    ship: racer_fleet::ShipType::Racer,
                    target_tier: 100,
                })
                .unwrap(),
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.player(0).tiers, [1, 0, 1]);
    s.tick(20, 1);
    s.start(20);
    assert_eq!(s.player(0).tiers, [100, 0, 1]);
    s.tick(20, 2);
    s.settle(20);
    let entry: racer_race::state::CyclePlayer = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::CyclePlayer {
                cycle: 1,
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(entry.tier_sum, Uint128::new(103));
    let settlement: Settlement = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::Settlement {
                cycle: 1,
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert!(settlement.maintenance <= settlement.gross_usd);
    let reserve: racer_rewards::State = s
        .app
        .wrap()
        .query_wasm_smart(&s.rewards, &racer_rewards::QueryMsg::State {})
        .unwrap();
    assert_eq!(
        s.balance(&s.usd, s.rewards.as_str()),
        reserve.reserve + reserve.available.spaceusd + reserve.liabilities.spaceusd
    );
}

#[test]
fn one_thousand_wallets_advance_with_bounded_batches_and_zero_reward_pool() {
    let mut s = Suite::new(1000, params(vec![1_000_000_000]));
    s.start(20);
    assert_eq!(usize::from(s.state().participants), s.count);
    s.tick(20, 42);
    s.settle(20);
    assert_eq!(s.state().cycle, 2);
    let result: RaceResult = s
        .app
        .wrap()
        .query_wasm_smart(&s.engine, &QueryMsg::Result { race: 1 })
        .unwrap();
    assert!(result.winner.is_some());
    let reserve: racer_rewards::State = s
        .app
        .wrap()
        .query_wasm_smart(&s.rewards, &racer_rewards::QueryMsg::State {})
        .unwrap();
    assert_eq!(reserve.liabilities, racer_rewards::Credit::default());
}

#[test]
fn one_hundred_races_settle_once_and_never_allocate_early() {
    let mut p = params(vec![1_000_000_000]);
    p.races_per_cycle = 100;
    let mut s = Suite::new(1, p);
    s.fund();
    for race in 1..=100 {
        s.start(20);
        assert_eq!(s.state().race, race);
        s.tick(20, race as u8);
        assert_eq!(s.state().completed, race as u16);
        assert_eq!(s.state().cycle, 1);
        let credit: racer_rewards::Credit = s
            .app
            .wrap()
            .query_wasm_smart(
                &s.rewards,
                &racer_rewards::QueryMsg::Credit {
                    wallet: "player0000".into(),
                },
            )
            .unwrap();
        assert_eq!(credit, racer_rewards::Credit::default());
    }
    s.settle(20);
    let entry: racer_race::state::CyclePlayer = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::CyclePlayer {
                cycle: 1,
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(entry.tier_sum, Uint128::new(200));
    let credit: racer_rewards::Credit = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.rewards,
            &racer_rewards::QueryMsg::Credit {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(credit.srt, Uint128::new(1_000_000));
    assert!(!credit.spaceusd.is_zero());
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.rewards.clone(),
            &racer_rewards::ExecuteMsg::Claim {},
            &[],
        )
        .unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.rewards.clone(),
            &racer_rewards::ExecuteMsg::Claim {},
            &[]
        )
        .is_err());
}

#[test]
fn unresolved_randomness_cannot_be_skipped_and_parameters_are_locked() {
    let mut s = Suite::new(1, params(vec![0, 1_000_000_000]));
    s.start(20);
    s.seal();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.engine.clone(),
            &ExecuteMsg::Advance { limit: 20 },
            &[]
        )
        .is_err());
    assert_eq!(s.state().phase, Phase::Waiting);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("owner"),
            s.engine.clone(),
            &ExecuteMsg::Configure {
                paused: None,
                parameters: Some(params(vec![1_000_000_000]))
            },
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.engine.clone(),
            &ExecuteMsg::Configure {
                paused: Some(true),
                parameters: None
            },
            &[]
        )
        .is_err());
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.engine.clone(),
            &ExecuteMsg::Configure {
                paused: Some(true),
                parameters: None,
            },
            &[],
        )
        .unwrap();
    let balance = s.balance(&s.srt, "player0000");
    assert!(s.boost(0, BoostKind::Speed, 1, 1_000_000).is_err());
    assert_eq!(s.balance(&s.srt, "player0000"), balance);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.engine.clone(),
            &ExecuteMsg::Configure {
                paused: Some(false),
                parameters: None,
            },
            &[],
        )
        .unwrap();
    s.publish(7);
    s.resolve(20);
    assert_eq!(s.state().tick, 2);
}

#[test]
fn logarithmic_weight_tree_selects_exact_intervals_and_removes_destroyed_fleets() {
    use cosmwasm_std::testing::MockStorage;
    let s = Suite::new(1, params(vec![1_000_000_000]));
    let mut state = s.state();
    state.race = 1;
    state.tick = 1;
    state.participants = 3;
    let mut storage = MockStorage::new();
    for (index, amount) in [(1, 10), (2, 30), (3, 60)] {
        racer_race::math::tree_change(
            &mut storage,
            &state,
            index,
            Uint128::zero(),
            Uint128::new(amount),
        )
        .unwrap();
    }
    let mut counts = [0; 3];
    for offset in 0..100 {
        counts[usize::from(
            racer_race::math::tree_find(&storage, &state, Uint128::new(offset)).unwrap(),
        ) - 1] += 1;
    }
    assert_eq!(counts, [10, 30, 60]);
    racer_race::math::tree_change(&mut storage, &state, 2, Uint128::new(30), Uint128::zero())
        .unwrap();
    assert_eq!(
        racer_race::math::prefix(&storage, &state, 3).unwrap(),
        Uint128::new(70)
    );
    for offset in 0..70 {
        assert_ne!(
            racer_race::math::tree_find(&storage, &state, Uint128::new(offset)).unwrap(),
            2
        );
    }
}

#[test]
fn temporary_distance_expires_and_an_unopposed_rocket_is_refunded() {
    let mut p = params(vec![0, 0, 1_000_000_000]);
    p.temporary_ticks = 1;
    let mut s = Suite::new(1, p);
    s.start(20);
    s.boost(0, BoostKind::Temporary, 1, 1_000_000).unwrap();
    s.boost(0, BoostKind::Rocket, 1, 200_000).unwrap();
    s.tick(20, 1);
    assert_eq!(s.player(0).temporary.len(), 1);
    let before = s.balance(&s.srt, "player0000");
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.engine.clone(),
            &ExecuteMsg::ClaimRefunds {
                race: 1,
                ticks: vec![],
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        s.balance(&s.srt, "player0000"),
        before + Uint128::new(200_000)
    );
    s.tick(20, 2);
    assert!(s.player(0).temporary.is_empty());
}

#[test]
fn clan_score_pool_splits_by_participating_tiers_and_pays_a_separate_leader_premium() {
    let mut s = Suite::new_version(2, params(vec![1_000_000_000]), 3);
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            s.usd.clone(),
            &Cw20ExecuteMsg::Send {
                contract: s.fleet.to_string(),
                amount: Uint128::new(99_000_000),
                msg: to_json_binary(&racer_fleet::ReceiveMsg::Upgrade {
                    ship: racer_fleet::ShipType::Racer,
                    target_tier: 100,
                })
                .unwrap(),
            },
            &[],
        )
        .unwrap();
    let code = s.app.store_code(Box::new(ContractWrapper::new(
        racer_clans::execute,
        racer_clans::instantiate,
        racer_clans::query,
    )));
    let clans = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_clans::InstantiateMsg {
                restore_from: None,
                owner: "owner".into(),
                engine: s.engine.to_string(),
                fleet: s.fleet.to_string(),
                current_cycle: 0,
            },
            &[],
            "clans",
            None,
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("player0000"),
            clans.clone(),
            &racer_clans::ExecuteMsg::Create {
                name: "Team".into(),
                code: "TEAM".into(),
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("player0001"),
            clans.clone(),
            &racer_clans::ExecuteMsg::Join {
                code: "TEAM".into(),
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.engine.clone(),
            &ExecuteMsg::SetClans {
                address: Some(clans.to_string()),
            },
            &[],
        )
        .unwrap();
    s.fund();
    s.start(20);
    s.tick(20, 8);
    s.settle(20);
    let a: Settlement = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::Settlement {
                cycle: 1,
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    let b: Settlement = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.engine,
            &QueryMsg::Settlement {
                cycle: 1,
                wallet: "player0001".into(),
            },
        )
        .unwrap();
    assert!(a.gross_usd > b.gross_usd && !b.gross_usd.is_zero());
    let state = s.state();
    let bucket = state.budget_usd;
    let premium = bucket.multiply_ratio(500u128, 10000u128);
    assert_eq!(
        a.gross_usd,
        bucket
            .checked_sub(premium)
            .unwrap()
            .multiply_ratio(101u128, 103u128)
    );
    assert_eq!(
        b.gross_usd,
        bucket
            .checked_sub(premium)
            .unwrap()
            .multiply_ratio(2u128, 103u128)
    );
    let credit: racer_rewards::Credit = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.rewards,
            &racer_rewards::QueryMsg::Credit {
                wallet: "player0000".into(),
            },
        )
        .unwrap();
    assert_eq!(
        credit.spaceusd,
        a.gross_usd.checked_sub(a.maintenance).unwrap() + premium
    );
    let reserve: racer_rewards::State = s
        .app
        .wrap()
        .query_wasm_smart(&s.rewards, &racer_rewards::QueryMsg::State {})
        .unwrap();
    assert_eq!(
        s.balance(&s.usd, s.rewards.as_str()),
        reserve.reserve + reserve.available.spaceusd + reserve.liabilities.spaceusd
    );
}
