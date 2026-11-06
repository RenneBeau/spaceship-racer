use cosmwasm_std::{to_json_binary, Addr, Empty, Uint128};
use cw20::{BalanceResponse, Cw20Coin, Cw20ExecuteMsg, Cw20QueryMsg};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use racer_fleet::{ExecuteMsg, Fleet, InstantiateMsg, QueryMsg, ReceiveMsg, ShipType};

fn fleet() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_fleet::execute,
        racer_fleet::instantiate,
        racer_fleet::query,
    ))
}
fn token() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    ))
}
struct Suite {
    app: App,
    token: Addr,
    fleet: Addr,
}

#[test]
fn fifty_day_presale_is_admin_started_continuous_and_gates_racing() {
    let mut s = Suite::new();
    let purchase = racer_fleet::Purchase::Buy { ship: ShipType::Racer, target_tier: 1, sponsor: None };
    assert!(s.app.execute_contract(Addr::unchecked("player"), s.fleet.clone(), &ExecuteMsg::PreparePresale {}, &[]).is_err());
    s.app.execute_contract(Addr::unchecked("owner"), s.fleet.clone(), &ExecuteMsg::PreparePresale {}, &[]).unwrap();
    assert!(s.app.wrap().query_wasm_smart::<racer_fleet::Quote>(&s.fleet, &QueryMsg::Quote { wallet: "player".into(), purchase: purchase.clone() }).is_err());
    s.app.execute_contract(Addr::unchecked("owner"), s.fleet.clone(), &ExecuteMsg::StartPresale {}, &[]).unwrap();
    let quote: racer_fleet::Quote = s.app.wrap().query_wasm_smart(&s.fleet, &QueryMsg::Quote { wallet: "player".into(), purchase: purchase.clone() }).unwrap();
    assert_eq!(quote.price, Uint128::new(500_000)); assert!(quote.maximum_price.unwrap() > quote.price);
    s.pay("player", 500_000, ReceiveMsg::Buy { ship: ShipType::Racer, target_tier: 1, sponsor: None }).unwrap();
    assert!(s.app.execute_contract(Addr::unchecked("engine"), s.fleet.clone(), &ExecuteMsg::AdvanceRace { race: 1 }, &[]).is_err());
    assert_eq!(s.state().active, [0, 0, 0]);
    s.app.update_block(|b| b.time = b.time.plus_seconds(racer_fleet::PRESALE_SECONDS / 2));
    let quote: racer_fleet::Quote = s.app.wrap().query_wasm_smart(&s.fleet, &QueryMsg::Quote { wallet: "player".into(), purchase: racer_fleet::Purchase::Upgrade { ship: ShipType::Racer, target_tier: 2 } }).unwrap();
    assert_eq!(quote.price, Uint128::new(750_000));
    assert!(s.app.execute_contract(Addr::unchecked("owner"), s.fleet.clone(), &ExecuteMsg::StartPresale {}, &[]).is_err());
    s.app.update_block(|b| b.time = b.time.plus_seconds(racer_fleet::PRESALE_SECONDS / 2));
    let quote: racer_fleet::Quote = s.app.wrap().query_wasm_smart(&s.fleet, &QueryMsg::Quote { wallet: "player".into(), purchase: racer_fleet::Purchase::Upgrade { ship: ShipType::Racer, target_tier: 2 } }).unwrap();
    assert_eq!(quote.price, Uint128::new(1_000_000)); assert_eq!(quote.maximum_price, None);
    s.advance(1); assert_eq!(s.state().active[0], 1);
}

#[test]
fn sponsor_can_be_new_wallet_but_cannot_close_a_cycle_later() {
    let mut s = Suite::new();
    s.pay(
        "player",
        950_000,
        ReceiveMsg::Buy {
            ship: ShipType::Racer,
            target_tier: 1,
            sponsor: Some("sponsor".into()),
        },
    )
    .unwrap();
    assert!(s
        .pay(
            "sponsor",
            950_000,
            ReceiveMsg::Buy {
                ship: ShipType::Racer,
                target_tier: 1,
                sponsor: Some("player".into()),
            }
        )
        .is_err());
    assert!(s
        .pay(
            "sponsor",
            950_000,
            ReceiveMsg::Buy {
                ship: ShipType::Racer,
                target_tier: 1,
                sponsor: Some("sponsor".into()),
            }
        )
        .is_err());
}

#[test]
fn registered_referrals_are_indexed_and_paid() {
    let mut s = Suite::new();
    let sponsor_before = s.balance("sponsor");
    let player_before = s.balance("player");
    s.pay(
        "player",
        950_000,
        ReceiveMsg::Buy {
            ship: ShipType::Racer,
            target_tier: 1,
            sponsor: Some("sponsor".into()),
        },
    )
    .unwrap();
    let referrals: Vec<String> = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.fleet,
            &QueryMsg::Referrals {
                sponsor: "sponsor".into(),
                start_after: None,
                limit: Some(10),
            },
        )
        .unwrap();
    assert_eq!(referrals, vec!["player".to_string()]);
    assert_eq!(s.balance("sponsor").u128(), sponsor_before.u128() + 47_500);
    assert_eq!(s.balance("player").u128(), player_before.u128() - 950_000);
    assert_eq!(s.state().sponsor, Some(Addr::unchecked("sponsor")));
}

#[test]
fn capacity_prevents_extra_wallets_from_stalling_the_race_snapshot() {
    let mut s = Suite::new();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.fleet.clone(),
            &ExecuteMsg::SetCapacity { max_players: 1 },
            &[],
        )
        .unwrap();
    s.pay(
        "player",
        1_000_000,
        ReceiveMsg::Buy {
            ship: ShipType::Racer,
            target_tier: 1,
            sponsor: None,
        },
    )
    .unwrap();
    assert!(s
        .pay(
            "sponsor",
            1_000_000,
            ReceiveMsg::Buy {
                ship: ShipType::Racer,
                target_tier: 1,
                sponsor: None
            }
        )
        .is_err());
    s.pay(
        "player",
        1_000_000,
        ReceiveMsg::Upgrade {
            ship: ShipType::Racer,
            target_tier: 2,
        },
    )
    .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.fleet.clone(),
            &ExecuteMsg::SetCapacity { max_players: 2 },
            &[],
        )
        .unwrap();
    s.pay(
        "sponsor",
        1_000_000,
        ReceiveMsg::Buy {
            ship: ShipType::Racer,
            target_tier: 1,
            sponsor: None,
        },
    )
    .unwrap();
}
impl Suite {
    fn new() -> Self {
        let mut app = App::default();
        let code = app.store_code(token());
        let token = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &cw20_base::msg::InstantiateMsg {
                    name: "SpaceUSD".into(),
                    symbol: "USD".into(),
                    decimals: 6,
                    initial_balances: ["player", "sponsor"]
                        .into_iter()
                        .map(|address| Cw20Coin {
                            address: address.into(),
                            amount: Uint128::new(1_000_000_000),
                        })
                        .collect(),
                    mint: None,
                    marketing: None,
                },
                &[],
                "token",
                None,
            )
            .unwrap();
        let code = app.store_code(fleet());
        let fleet = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &InstantiateMsg {
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
        Self { app, token, fleet }
    }
    fn pay(&mut self, wallet: &str, amount: u128, msg: ReceiveMsg) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked(wallet),
                self.token.clone(),
                &Cw20ExecuteMsg::Send {
                    contract: self.fleet.to_string(),
                    amount: Uint128::new(amount),
                    msg: to_json_binary(&msg).unwrap(),
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn state(&self) -> Fleet {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.fleet,
                &QueryMsg::Fleet {
                    wallet: "player".into(),
                },
            )
            .unwrap()
    }
    fn balance(&self, wallet: &str) -> Uint128 {
        let balance: BalanceResponse = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.token,
                &Cw20QueryMsg::Balance {
                    address: wallet.into(),
                },
            )
            .unwrap();
        balance.balance
    }
    fn advance(&mut self, race: u64) {
        self.app
            .execute_contract(
                Addr::unchecked("engine"),
                self.fleet.clone(),
                &ExecuteMsg::AdvanceRace { race },
                &[],
            )
            .unwrap();
    }
}

#[test]
fn purchases_and_multiple_upgrades_activate_only_next_race() {
    let mut s = Suite::new();
    s.pay(
        "player",
        1_000_000,
        ReceiveMsg::Buy {
            ship: ShipType::Racer,
            target_tier: 1,
            sponsor: None,
        },
    )
    .unwrap();
    assert_eq!(s.state().active, [0, 0, 0]);
    assert_eq!(s.state().next, [1, 0, 0]);
    s.advance(1);
    s.pay(
        "player",
        4_000_000,
        ReceiveMsg::Upgrade {
            ship: ShipType::Racer,
            target_tier: 5,
        },
    )
    .unwrap();
    s.pay(
        "player",
        5_000_000,
        ReceiveMsg::Upgrade {
            ship: ShipType::Racer,
            target_tier: 10,
        },
    )
    .unwrap();
    assert_eq!(s.state().active, [1, 0, 0]);
    assert_eq!(s.state().next, [10, 0, 0]);
    s.advance(2);
    assert_eq!(s.state().active, [10, 0, 0]);
    assert_eq!(s.balance("treasury"), Uint128::new(10_000_000));
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.fleet.clone(),
            &ExecuteMsg::AdvanceRace { race: 3 },
            &[]
        )
        .is_err());
}

#[test]
fn referral_discount_once_and_rejected_payment_rolls_back_token_transfer() {
    let mut s = Suite::new();
    s.pay(
        "sponsor",
        1_000_000,
        ReceiveMsg::Buy {
            ship: ShipType::Racer,
            target_tier: 1,
            sponsor: None,
        },
    )
    .unwrap();
    let before = s.balance("player");
    assert!(s
        .pay(
            "player",
            1,
            ReceiveMsg::Buy {
                ship: ShipType::Racer,
                target_tier: 1,
                sponsor: Some("sponsor".into())
            }
        )
        .is_err());
    assert_eq!(s.balance("player"), before);
    assert!(!s.state().purchased);
    s.pay(
        "player",
        950_000,
        ReceiveMsg::Buy {
            ship: ShipType::Racer,
            target_tier: 1,
            sponsor: Some("sponsor".into()),
        },
    )
    .unwrap();
    assert_eq!(s.balance("sponsor"), Uint128::new(999_047_500));
    s.pay(
        "player",
        1_000_000,
        ReceiveMsg::Buy {
            ship: ShipType::Tank,
            target_tier: 1,
            sponsor: None,
        },
    )
    .unwrap();
    assert_eq!(s.balance("sponsor"), Uint128::new(999_097_500));
    assert!(s
        .pay(
            "player",
            1_000_000,
            ReceiveMsg::Buy {
                ship: ShipType::Fighter,
                target_tier: 1,
                sponsor: Some("other".into())
            }
        )
        .is_err());
}
