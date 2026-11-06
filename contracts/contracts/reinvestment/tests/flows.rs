use cosmwasm_std::{to_json_binary, Addr, Decimal, Empty, Uint128};
use cw20::{BalanceResponse, Cw20Coin, Cw20ExecuteMsg, Cw20QueryMsg};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use racer_fleet::{ReceiveMsg as FleetHook, ShipType};
use racer_reinvestment::{ExecuteMsg, Hook, QueryMsg, Reinvestment};
fn token() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    ))
}
struct Suite {
    app: App,
    usd: Addr,
    srt: Addr,
    fleet: Addr,
    gateway: Addr,
}
impl Suite {
    fn new() -> Self {
        let mut app = App::default();
        let token_code = app.store_code(token());
        let fleet_code = app.store_code(Box::new(ContractWrapper::new(
            racer_fleet::execute,
            racer_fleet::instantiate,
            racer_fleet::query,
        )));
        let reward_code = app.store_code(Box::new(ContractWrapper::new(
            racer_rewards::execute,
            racer_rewards::instantiate,
            racer_rewards::query,
        )));
        let gateway_code = app.store_code(Box::new(
            ContractWrapper::new(
                racer_reinvestment::execute,
                racer_reinvestment::instantiate,
                racer_reinvestment::query,
            )
            .with_reply(racer_reinvestment::reply)
            .with_migrate(racer_reinvestment::migrate),
        ));
        let mut instantiate_token = |name: &str| {
            app.instantiate_contract(
                token_code,
                Addr::unchecked("owner"),
                &cw20_base::msg::InstantiateMsg {
                    name: name.into(),
                    symbol: name.into(),
                    decimals: 6,
                    initial_balances: vec![
                        Cw20Coin {
                            address: "owner".into(),
                            amount: Uint128::new(1_000_000_000_000),
                        },
                        Cw20Coin {
                            address: "player".into(),
                            amount: Uint128::new(1_000_000_000_000),
                        },
                    ],
                    mint: None,
                    marketing: None,
                },
                &[],
                name,
                None,
            )
            .unwrap()
        };
        let usd = instantiate_token("SpaceUSD");
        let srt = instantiate_token("SRT");
        let fleet = app
            .instantiate_contract(
                fleet_code,
                Addr::unchecked("owner"),
                &racer_fleet::InstantiateMsg {
                    owner: "owner".into(),
                    engine: "engine".into(),
                    spaceusd: usd.to_string(),
                    treasury: "treasury".into(),
                    price_per_tier: Uint128::new(1_000_000),
                },
                &[],
                "fleet",
                None,
            )
            .unwrap();
        app.execute_contract(
            Addr::unchecked("player"),
            usd.clone(),
            &Cw20ExecuteMsg::Send {
                contract: fleet.to_string(),
                amount: Uint128::new(1_000_000),
                msg: to_json_binary(&FleetHook::Buy {
                    ship: ShipType::Racer,
                    target_tier: 1,
                    sponsor: None,
                })
                .unwrap(),
            },
            &[],
        )
        .unwrap();
        app.execute_contract(
            Addr::unchecked("engine"),
            fleet.clone(),
            &racer_fleet::ExecuteMsg::AdvanceRace { race: 1 },
            &[],
        )
        .unwrap();
        let backing = app
            .instantiate_contract(
                reward_code,
                Addr::unchecked("owner"),
                &racer_rewards::InstantiateMsg {
                    owner: "owner".into(),
                    allocator: "engine".into(),
                    spaceusd: usd.to_string(),
                    srt: srt.to_string(),
                    annual_bps: 10000,
                },
                &[],
                "backing",
                None,
            )
            .unwrap();
        let gateway = app
            .instantiate_contract(
                gateway_code,
                Addr::unchecked("owner"),
                &racer_reinvestment::InstantiateMsg {
                    payments: racer_payments::InstantiateMsg {
                        owner: "owner".into(),
                        fleet: fleet.to_string(),
                        spaceusd: usd.to_string(),
                        router: "router".into(),
                        payment_tokens: vec![usd.to_string(), srt.to_string()],
                        max_spread: Decimal::percent(2),
                    },
                    backing_rewards: backing.to_string(),
                    allocator: "engine".into(),
                },
                &[],
                "gateway",
                Some("owner".into()),
            )
            .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            fleet.clone(),
            &racer_fleet::ExecuteMsg::SetPaymentAdapter {
                adapter: Some(gateway.to_string()),
            },
            &[],
        )
        .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            backing.clone(),
            &racer_rewards::ExecuteMsg::Configure {
                allocator: Some(gateway.to_string()),
                annual_bps: None,
                paused: None,
            },
            &[],
        )
        .unwrap();
        for (asset, amount) in [(&usd, 100_000_000u128), (&srt, 100_000_000)] {
            app.execute_contract(
                Addr::unchecked("owner"),
                asset.clone(),
                &Cw20ExecuteMsg::Send {
                    contract: backing.to_string(),
                    amount: Uint128::new(amount),
                    msg: to_json_binary(&racer_rewards::ReceiveMsg::Fund {}).unwrap(),
                },
                &[],
            )
            .unwrap();
        }
        app.update_block(|block| {
            block.time = block.time.plus_seconds(31_557_600);
            block.height += 1;
        });
        app.execute_contract(
            Addr::unchecked("keeper"),
            gateway.clone(),
            &ExecuteMsg::Checkpoint {},
            &[],
        )
        .unwrap();
        Self {
            app,
            usd,
            srt,
            fleet,
            gateway,
        }
    }
    fn invest(&mut self, amount: u128) {
        self.app
            .execute_contract(
                Addr::unchecked("player"),
                self.usd.clone(),
                &Cw20ExecuteMsg::Send {
                    contract: self.gateway.to_string(),
                    amount: Uint128::new(amount),
                    msg: to_json_binary(&Hook::Invest {
                        ship: ShipType::Racer,
                    })
                    .unwrap(),
                },
                &[],
            )
            .unwrap();
    }
    fn reinvestment(&self) -> Reinvestment {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.gateway,
                &QueryMsg::Reinvestment {
                    wallet: "player".into(),
                },
            )
            .unwrap()
    }
    fn fleet(&self) -> racer_fleet::Fleet {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.fleet,
                &racer_fleet::QueryMsg::Fleet {
                    wallet: "player".into(),
                },
            )
            .unwrap()
    }
    fn balance(&self, token: &Addr, wallet: &str) -> Uint128 {
        self.app
            .wrap()
            .query_wasm_smart::<BalanceResponse>(
                token,
                &Cw20QueryMsg::Balance {
                    address: wallet.into(),
                },
            )
            .unwrap()
            .balance
    }
    fn adapter(&mut self, address: Addr) {
        self.app
            .execute_contract(
                Addr::unchecked("owner"),
                self.fleet.clone(),
                &racer_fleet::ExecuteMsg::SetPaymentAdapter {
                    adapter: Some(address.to_string()),
                },
                &[],
            )
            .unwrap();
    }
    fn allocate(&mut self) {
        self.app
            .execute_contract(
                Addr::unchecked("engine"),
                self.gateway.clone(),
                &ExecuteMsg::Allocate {
                    recipients: vec![racer_rewards::Allocation {
                        wallet: "player".into(),
                        spaceusd: Uint128::new(4_800_000),
                        srt: Uint128::new(7_000_000),
                    }],
                },
                &[],
            )
            .unwrap();
    }
}
#[test]
fn fractional_progress_accumulates_and_only_whole_levels_activate_next_race() {
    let mut s = Suite::new();
    s.invest(400_000);
    assert_eq!(s.reinvestment().progress.spaceusd[0], Uint128::new(400_000));
    assert_eq!(s.fleet().next[0], 1);
    s.invest(600_000);
    assert_eq!(s.reinvestment().progress.spaceusd[0], Uint128::zero());
    assert_eq!(s.fleet().active[0], 1);
    assert_eq!(s.fleet().next[0], 2);
    s.app
        .execute_contract(
            Addr::unchecked("engine"),
            s.fleet.clone(),
            &racer_fleet::ExecuteMsg::AdvanceRace { race: 2 },
            &[],
        )
        .unwrap();
    assert_eq!(s.fleet().active[0], 2);
}
#[test]
fn keeper_can_only_invest_explicitly_authorized_future_reward_share() {
    let mut s = Suite::new();
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.gateway.clone(),
            &ExecuteMsg::SetAutoReinvest {
                ship: Some(ShipType::Racer),
                bps: 5000,
            },
            &[],
        )
        .unwrap();
    s.adapter(Addr::unchecked("temporarily-unavailable-adapter"));
    s.allocate();
    s.adapter(s.gateway.clone());
    assert_eq!(s.reinvestment().ready, Uint128::new(2_400_000));
    let before = s.balance(&s.usd, "player");
    s.app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.gateway.clone(),
            &ExecuteMsg::ProcessAuto {
                wallet: "player".into(),
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.balance(&s.usd, "player"), before);
    assert_eq!(s.fleet().next[0], 3);
    assert_eq!(s.reinvestment().progress.spaceusd[0], Uint128::new(400_000));
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.gateway.clone(),
            &ExecuteMsg::ProcessAuto {
                wallet: "player".into()
            },
            &[]
        )
        .is_err());
    let credit: racer_rewards::Credit = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.gateway,
            &QueryMsg::Credit {
                wallet: "player".into(),
            },
        )
        .unwrap();
    assert_eq!(credit.spaceusd, Uint128::new(2_400_000));
    assert_eq!(credit.srt, Uint128::new(7_000_000));
}
#[test]
fn disabling_auto_preserves_credit_and_saved_progress_can_be_withdrawn() {
    let mut s = Suite::new();
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.gateway.clone(),
            &ExecuteMsg::SetAutoReinvest {
                ship: Some(ShipType::Tank),
                bps: 10000,
            },
            &[],
        )
        .unwrap();
    s.adapter(Addr::unchecked("temporarily-unavailable-adapter"));
    s.allocate();
    s.adapter(s.gateway.clone());
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.gateway.clone(),
            &ExecuteMsg::SetAutoReinvest { ship: None, bps: 0 },
            &[],
        )
        .unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.gateway.clone(),
            &ExecuteMsg::ProcessAuto {
                wallet: "player".into()
            },
            &[]
        )
        .is_err());
    let before = s.balance(&s.usd, "player");
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.gateway.clone(),
            &ExecuteMsg::Claim {},
            &[],
        )
        .unwrap();
    assert_eq!(
        s.balance(&s.usd, "player"),
        before + Uint128::new(4_800_000)
    );
    assert_eq!(s.balance(&s.srt, "player"), Uint128::new(1_000_007_000_000));
    s.invest(400_000);
    let before = s.balance(&s.usd, "player");
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.gateway.clone(),
            &ExecuteMsg::WithdrawProgress {
                ship: ShipType::Racer,
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.balance(&s.usd, "player"), before + Uint128::new(400_000));
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.gateway.clone(),
            &ExecuteMsg::WithdrawProgress {
                ship: ShipType::Racer
            },
            &[]
        )
        .is_err());
}
#[test]
fn automatic_upgrades_complete_inside_allocation_before_the_next_race_can_launch() {
    let mut s = Suite::new();
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.gateway.clone(),
            &ExecuteMsg::SetAutoReinvest {
                ship: Some(ShipType::Racer),
                bps: 5000,
            },
            &[],
        )
        .unwrap();
    s.allocate();
    assert_eq!(s.fleet().next[0], 3);
    assert_eq!(s.reinvestment().ready, Uint128::zero());
    assert_eq!(s.reinvestment().progress.spaceusd[0], Uint128::new(400_000));
    s.app
        .execute_contract(
            Addr::unchecked("engine"),
            s.fleet.clone(),
            &racer_fleet::ExecuteMsg::AdvanceRace { race: 2 },
            &[],
        )
        .unwrap();
    assert_eq!(s.fleet().active[0], 3);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("no-fleet"),
            s.gateway.clone(),
            &ExecuteMsg::SetAutoReinvest {
                ship: Some(ShipType::Racer),
                bps: 10000
            },
            &[]
        )
        .is_err());
}

#[test]
fn gateway_migration_preserves_claims_progress_preferences_and_backing() {
    let mut s = Suite::new();
    s.invest(400_000);
    s.allocate();
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.gateway.clone(),
            &ExecuteMsg::SetAutoReinvest {
                ship: Some(ShipType::Tank),
                bps: 5000,
            },
            &[],
        )
        .unwrap();
    let before = s.reinvestment();
    let credit: racer_rewards::Credit = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.gateway,
            &QueryMsg::Credit {
                wallet: "player".into(),
            },
        )
        .unwrap();
    let balance = s.balance(&s.usd, s.gateway.as_str());
    let code = s.app.store_code(Box::new(
        ContractWrapper::new(
            racer_reinvestment::execute,
            racer_reinvestment::instantiate,
            racer_reinvestment::query,
        )
        .with_reply(racer_reinvestment::reply)
        .with_migrate(racer_reinvestment::migrate),
    ));
    s.app
        .migrate_contract(Addr::unchecked("owner"), s.gateway.clone(), &Empty {}, code)
        .unwrap();
    assert_eq!(s.reinvestment(), before);
    assert_eq!(
        s.app
            .wrap()
            .query_wasm_smart::<racer_rewards::Credit>(
                &s.gateway,
                &QueryMsg::Credit {
                    wallet: "player".into()
                }
            )
            .unwrap(),
        credit
    );
    assert_eq!(s.balance(&s.usd, s.gateway.as_str()), balance);
}

#[test]
fn unauthorized_allocation_and_unapproved_automatic_spending_are_rejected() {
    let mut s = Suite::new();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.gateway.clone(),
            &ExecuteMsg::Allocate {
                recipients: vec![racer_rewards::Allocation {
                    wallet: "attacker".into(),
                    spaceusd: Uint128::new(1),
                    srt: Uint128::zero()
                }]
            },
            &[]
        )
        .is_err());
    s.allocate();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            s.gateway.clone(),
            &ExecuteMsg::ProcessAuto {
                wallet: "player".into()
            },
            &[]
        )
        .is_err());
    assert_eq!(s.reinvestment().ready, Uint128::zero());
}
