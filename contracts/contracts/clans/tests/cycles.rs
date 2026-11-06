use cosmwasm_std::{to_json_binary, Addr, Empty, Uint128};
use cw20::{Cw20Coin, Cw20ExecuteMsg};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use racer_clans::{Clan, ExecuteMsg, Member, QueryMsg};
fn clans() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_clans::execute,
        racer_clans::instantiate,
        racer_clans::query,
    ))
}
struct Suite {
    app: App,
    clans: Addr,
}
impl Suite {
    fn new() -> Self {
        let mut app = App::default();
        let code = app.store_code(Box::new(ContractWrapper::new(
            cw20_base::contract::execute,
            cw20_base::contract::instantiate,
            cw20_base::contract::query,
        )));
        let token = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &cw20_base::msg::InstantiateMsg {
                    name: "SpaceUSD".into(),
                    symbol: "USD".into(),
                    decimals: 6,
                    initial_balances: vec![Cw20Coin {
                        address: "leader".into(),
                        amount: Uint128::new(1_000_000_000),
                    }],
                    mint: None,
                    marketing: None,
                },
                &[],
                "token",
                None,
            )
            .unwrap();
        let code = app.store_code(Box::new(ContractWrapper::new(
            racer_fleet::execute,
            racer_fleet::instantiate,
            racer_fleet::query,
        )));
        let fleet = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_fleet::InstantiateMsg {
                    owner: "owner".into(),
                    engine: "engine".into(),
                    spaceusd: token.to_string(),
                    treasury: "treasury".into(),
                    price_per_tier: Uint128::new(1_000_000),
                },
                &[],
                "fleet",
                None,
            )
            .unwrap();
        app.execute_contract(
            Addr::unchecked("leader"),
            token,
            &Cw20ExecuteMsg::Send {
                contract: fleet.to_string(),
                amount: Uint128::new(100_000_000),
                msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
                    ship: racer_fleet::ShipType::Racer,
                    target_tier: 100,
                    sponsor: None,
                })
                .unwrap(),
            },
            &[],
        )
        .unwrap();
        let code = app.store_code(clans());
        let clans = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_clans::InstantiateMsg {
                    restore_from: None,
                    owner: "owner".into(),
                    engine: "engine".into(),
                    fleet: fleet.to_string(),
                    current_cycle: 0,
                },
                &[],
                "clans",
                None,
            )
            .unwrap();
        Self { app, clans }
    }
    fn exec(&mut self, wallet: &str, msg: ExecuteMsg) -> Result<(), String> {
        self.app
            .execute_contract(Addr::unchecked(wallet), self.clans.clone(), &msg, &[])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn member(&self, wallet: &str) -> Member {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.clans,
                &QueryMsg::Member {
                    wallet: wallet.into(),
                },
            )
            .unwrap()
    }
    fn clan(&self) -> Clan {
        self.app
            .wrap()
            .query_wasm_smart(&self.clans, &QueryMsg::Clan { id: 1 })
            .unwrap()
    }
    fn create(&mut self) {
        self.exec(
            "leader",
            ExecuteMsg::Create {
                name: "First clan".into(),
                code: "ONE".into(),
            },
        )
        .unwrap();
    }
}
#[test]
fn departed_member_metadata_is_restored_and_locked_when_the_new_engine_is_set() {
    let mut s = Suite::new();
    s.create();
    s.exec("mate", ExecuteMsg::Join { code: "ONE".into() })
        .unwrap();
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 1 })
        .unwrap();
    s.exec(
        "leader",
        ExecuteMsg::TransferLead {
            clan_id: 1,
            wallet: "mate".into(),
        },
    )
    .unwrap();
    s.exec("leader", ExecuteMsg::Leave {}).unwrap();
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 2 })
        .unwrap();
    let departed = s.member("leader");
    assert_eq!(departed.active, None);
    assert!(departed.effective_cycle > 0);
    let config: racer_clans::Config = s
        .app
        .wrap()
        .query_wasm_smart(&s.clans, &QueryMsg::Config {})
        .unwrap();
    let code = s.app.store_code(clans());
    let target = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_clans::InstantiateMsg {
                owner: "owner".into(),
                engine: "owner".into(),
                fleet: config.fleet.to_string(),
                current_cycle: 2,
                restore_from: Some(s.clans.to_string()),
            },
            &[],
            "departed-member-restore",
            None,
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            target.clone(),
            &ExecuteMsg::Restore { limit: 5 },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            target.clone(),
            &ExecuteMsg::RestoreMembers {
                wallets: vec!["leader".into()],
            },
            &[],
        )
        .unwrap();
    let member: Member = s
        .app
        .wrap()
        .query_wasm_smart(
            &target,
            &QueryMsg::Member {
                wallet: "leader".into(),
            },
        )
        .unwrap();
    assert_eq!(member, departed);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            target.clone(),
            &ExecuteMsg::SetEngine {
                engine: "engine-two".into(),
            },
            &[],
        )
        .unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("owner"),
            target,
            &ExecuteMsg::RestoreMembers {
                wallets: vec!["leader".into()]
            },
            &[]
        )
        .is_err());
}

#[test]
fn restoring_a_clan_preserves_ids_rosters_and_queued_leadership_changes() {
    let mut s = Suite::new();
    s.create();
    s.exec("mate", ExecuteMsg::Join { code: "ONE".into() })
        .unwrap();
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 1 })
        .unwrap();
    s.exec(
        "leader",
        ExecuteMsg::TransferLead {
            clan_id: 1,
            wallet: "mate".into(),
        },
    )
    .unwrap();
    s.exec("leader", ExecuteMsg::Leave {}).unwrap();
    let original = s.clan();
    let member = s.member("leader");
    let config: racer_clans::Config = s
        .app
        .wrap()
        .query_wasm_smart(&s.clans, &QueryMsg::Config {})
        .unwrap();
    let code = s.app.store_code(clans());
    let replacement = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_clans::InstantiateMsg {
                owner: "owner".into(),
                engine: "engine-two".into(),
                fleet: config.fleet.to_string(),
                current_cycle: 1,
                restore_from: Some(s.clans.to_string()),
            },
            &[],
            "replacement-clans",
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
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("other"),
            replacement.clone(),
            &ExecuteMsg::Join { code: "ONE".into() },
            &[]
        )
        .is_err());
    for _ in 0..2 {
        s.app
            .execute_contract(
                Addr::unchecked("owner"),
                replacement.clone(),
                &ExecuteMsg::Restore { limit: 1 },
                &[],
            )
            .unwrap();
    }
    s.clans = replacement;
    assert_eq!(s.clan(), original);
    assert_eq!(s.member("leader"), member);
    s.exec("other", ExecuteMsg::Join { code: "ONE".into() })
        .unwrap();
    assert_eq!(s.member("other").next, Some(1));
    assert!(s
        .exec("engine", ExecuteMsg::BeginCycle { cycle: 2 })
        .is_err());
    s.exec("engine-two", ExecuteMsg::BeginCycle { cycle: 2 })
        .unwrap();
    assert_eq!(s.clan().leader, Addr::unchecked("mate"));
    assert_eq!(s.member("leader").active, None);
    assert!(s
        .exec(
            "attacker",
            ExecuteMsg::SetEngine {
                engine: "bad".into()
            }
        )
        .is_err());
}

#[test]
fn creation_join_leave_and_leadership_wait_for_the_next_cycle() {
    let mut s = Suite::new();
    s.create();
    s.exec("mate", ExecuteMsg::Join { code: "one".into() })
        .unwrap();
    assert_eq!(s.member("leader").active, None);
    assert_eq!(s.member("mate").active, None);
    assert!(s
        .exec("attacker", ExecuteMsg::BeginCycle { cycle: 1 })
        .is_err());
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 1 })
        .unwrap();
    assert_eq!(s.member("mate").active, Some(1));
    assert!(s.exec("leader", ExecuteMsg::Leave {}).is_err());
    s.exec(
        "leader",
        ExecuteMsg::TransferLead {
            clan_id: 1,
            wallet: "mate".into(),
        },
    )
    .unwrap();
    s.exec("leader", ExecuteMsg::Leave {}).unwrap();
    assert_eq!(s.clan().leader, Addr::unchecked("leader"));
    assert_eq!(s.member("leader").active, Some(1));
    assert!(s.exec("mate", ExecuteMsg::Leave {}).is_err());
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 2 })
        .unwrap();
    assert_eq!(s.clan().leader, Addr::unchecked("mate"));
    assert_eq!(s.member("leader").active, None);
    assert_eq!(s.clan().members, vec![Addr::unchecked("mate")]);
}
#[test]
fn future_roster_reserves_capacity_and_a_kicked_member_cannot_cancel_removal() {
    let mut s = Suite::new();
    s.create();
    s.exec(
        "owner",
        ExecuteMsg::Configure {
            min_create_tiers: None,
            max_members: Some(2),
            leader_premium_bps: None,
        },
    )
    .unwrap();
    s.exec("mate", ExecuteMsg::Join { code: "ONE".into() })
        .unwrap();
    assert!(s
        .exec("other", ExecuteMsg::Join { code: "ONE".into() })
        .is_err());
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 1 })
        .unwrap();
    s.exec(
        "leader",
        ExecuteMsg::Kick {
            clan_id: 1,
            wallet: "mate".into(),
        },
    )
    .unwrap();
    assert!(s.exec("mate", ExecuteMsg::CancelChange {}).is_err());
    assert_eq!(s.member("mate").active, Some(1));
    s.exec("other", ExecuteMsg::Join { code: "ONE".into() })
        .unwrap();
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 2 })
        .unwrap();
    assert_eq!(s.member("mate").active, None);
    assert_eq!(s.member("other").active, Some(1));
}
#[test]
fn voluntary_changes_cancel_and_dismantle_preserves_the_current_cycle() {
    let mut s = Suite::new();
    assert!(s
        .exec(
            "mate",
            ExecuteMsg::Create {
                name: "No tiers".into(),
                code: "NO".into()
            }
        )
        .is_err());
    s.create();
    s.exec("mate", ExecuteMsg::Join { code: "ONE".into() })
        .unwrap();
    s.exec("mate", ExecuteMsg::CancelChange {}).unwrap();
    assert_eq!(s.member("mate").next, None);
    s.exec("mate", ExecuteMsg::Join { code: "ONE".into() })
        .unwrap();
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 1 })
        .unwrap();
    s.exec("leader", ExecuteMsg::Dismantle { clan_id: 1 })
        .unwrap();
    assert_eq!(s.member("mate").active, Some(1));
    assert_eq!(s.clan().members.len(), 2);
    assert!(s
        .exec("other", ExecuteMsg::Join { code: "ONE".into() })
        .is_err());
    s.exec("engine", ExecuteMsg::BeginCycle { cycle: 2 })
        .unwrap();
    assert_eq!(s.member("mate").active, None);
    assert!(s.clan().members.is_empty());
}
