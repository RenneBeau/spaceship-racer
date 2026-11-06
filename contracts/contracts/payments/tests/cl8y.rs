//! Integration against the pinned upstream factory/pair/router implementations,
//! not a fixed-price mock swap service.
use cosmwasm_std::{to_json_binary, Addr, Decimal, Empty, Uint128};
use cw20::{BalanceResponse, Cw20Coin, Cw20ExecuteMsg, Cw20QueryMsg};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use dex_common::types::{Asset, AssetInfo};
use racer_fleet::{Fleet, Purchase, ShipType};
use racer_payments::{ExecuteMsg, Payment, QueryMsg};

fn token() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    ))
}
fn lp() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        cw20_mintable::contract::execute,
        cw20_mintable::contract::instantiate,
        cw20_mintable::contract::query,
    ))
}
fn factory() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new(
            cl8y_dex_factory::contract::execute,
            cl8y_dex_factory::contract::instantiate,
            cl8y_dex_factory::contract::query,
        )
        .with_reply(cl8y_dex_factory::contract::reply),
    )
}
fn pair() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new(
            cl8y_dex_pair::contract::execute,
            cl8y_dex_pair::contract::instantiate,
            cl8y_dex_pair::contract::query,
        )
        .with_reply(cl8y_dex_pair::contract::reply),
    )
}
fn router() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new(
            cl8y_dex_router::contract::execute,
            cl8y_dex_router::contract::instantiate,
            cl8y_dex_router::contract::query,
        )
        .with_reply(cl8y_dex_router::contract::reply),
    )
}
fn fleet() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_fleet::execute,
        racer_fleet::instantiate,
        racer_fleet::query,
    ))
}
fn payments() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new(
            racer_payments::execute,
            racer_payments::instantiate,
            racer_payments::query,
        )
        .with_reply(racer_payments::reply),
    )
}
fn treasury() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new(
            racer_treasury::execute,
            racer_treasury::instantiate,
            racer_treasury::query,
        )
        .with_reply(racer_treasury::reply),
    )
}
fn token_info(a: &Addr) -> AssetInfo {
    AssetInfo::Token {
        contract_addr: a.to_string(),
    }
}
struct Suite {
    app: App,
    tokens: Vec<Addr>,
    usd: Addr,
    router: Addr,
    payments: Addr,
    fleet: Addr,
    treasury: Addr,
    pause: Addr,
}
impl Suite {
    fn new() -> Self {
        let mut app = App::default();
        let token_code = app.store_code(token());
        let mut tokens = vec![];
        for symbol in ["CLUNC", "CUSTC", "UST", "SRT", "USD"] {
            tokens.push(
                app.instantiate_contract(
                    token_code,
                    Addr::unchecked("owner"),
                    &cw20_base::msg::InstantiateMsg {
                        name: symbol.into(),
                        symbol: symbol.into(),
                        decimals: 6,
                        initial_balances: ["owner", "player", "sponsor"]
                            .into_iter()
                            .map(|s| Cw20Coin {
                                address: s.into(),
                                amount: Uint128::new(1_000_000_000_000_000),
                            })
                            .collect(),
                        mint: None,
                        marketing: None,
                    },
                    &[],
                    symbol,
                    None,
                )
                .unwrap(),
            );
        }
        let usd = tokens[4].clone();
        let pair_code = app.store_code(pair());
        let lp_code = app.store_code(lp());
        let code = app.store_code(factory());
        let factory = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &dex_common::factory::InstantiateMsg {
                    governance: "owner".into(),
                    treasury: "dex-fees".into(),
                    default_fee_bps: 30,
                    pair_code_id: pair_code,
                    lp_token_code_id: lp_code,
                    whitelisted_code_ids: vec![token_code],
                    default_limit_batch_max_rungs: 20,
                    pair_creation_fee_uluna: Uint128::zero(),
                },
                &[],
                "factory",
                None,
            )
            .unwrap();
        for (token, quote) in tokens[..4]
            .iter()
            .map(|token| (token, &usd))
            .chain([(&tokens[0], &tokens[3])])
        {
            app.update_block(|b| b.height += 1);
            app.execute_contract(
                Addr::unchecked("owner"),
                factory.clone(),
                &dex_common::factory::ExecuteMsg::CreatePair {
                    asset_infos: [token_info(token), token_info(quote)],
                },
                &[],
            )
            .unwrap();
            let response: dex_common::factory::PairResponse = app
                .wrap()
                .query_wasm_smart(
                    &factory,
                    &dex_common::factory::QueryMsg::Pair {
                        asset_infos: [token_info(token), token_info(quote)],
                    },
                )
                .unwrap();
            let pair = response.pair;
            for addr in [token, quote] {
                app.execute_contract(
                    Addr::unchecked("owner"),
                    addr.clone(),
                    &Cw20ExecuteMsg::IncreaseAllowance {
                        spender: pair.contract_addr.to_string(),
                        amount: Uint128::new(1_000_000_000_000),
                        expires: None,
                    },
                    &[],
                )
                .unwrap();
            }
            app.execute_contract(
                Addr::unchecked("owner"),
                pair.contract_addr,
                &dex_common::pair::ExecuteMsg::ProvideLiquidity {
                    assets: [
                        Asset {
                            info: token_info(token),
                            amount: Uint128::new(1_000_000_000_000),
                        },
                        Asset {
                            info: token_info(quote),
                            amount: Uint128::new(1_000_000_000_000),
                        },
                    ],
                    slippage_tolerance: Some(Decimal::percent(1)),
                    receiver: None,
                    deadline: None,
                },
                &[],
            )
            .unwrap();
        }
        let code = app.store_code(router());
        let router = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &cl8y_dex_router::msg::InstantiateMsg {
                    factory: factory.to_string(),
                },
                &[],
                "router",
                None,
            )
            .unwrap();
        let code = app.store_code(treasury());
        let treasury = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_treasury::InstantiateMsg {
                    owner: "owner".into(),
                    overlord: "overlord".into(),
                    router: router.to_string(),
                    spaceusd: usd.to_string(),
                    payment_tokens: tokens.iter().map(|a| a.to_string()).collect(),
                    max_spread: Decimal::percent(2),
                    reward_pool: None,
                },
                &[],
                "treasury",
                None,
            )
            .unwrap();
        let code = app.store_code(fleet());
        let fleet = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_fleet::InstantiateMsg {
                    owner: "owner".into(),
                    engine: "engine".into(),
                    spaceusd: usd.to_string(),
                    treasury: treasury.to_string(),
                    price_per_tier: Uint128::new(1_000_000),
                },
                &[],
                "fleet",
                None,
            )
            .unwrap();
        let code = app.store_code(payments());
        let payments = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_payments::InstantiateMsg {
                    owner: "owner".into(),
                    fleet: fleet.to_string(),
                    spaceusd: usd.to_string(),
                    router: router.to_string(),
                    payment_tokens: tokens.iter().map(|a| a.to_string()).collect(),
                    max_spread: Decimal::percent(2),
                },
                &[],
                "payments",
                None,
            )
            .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            fleet.clone(),
            &racer_fleet::ExecuteMsg::SetPaymentAdapter {
                adapter: Some(payments.to_string()),
            },
            &[],
        )
        .unwrap();
        for token in &tokens[..4] {
            let path = vec![token.to_string(), usd.to_string()];
            app.execute_contract(
                Addr::unchecked("owner"),
                payments.clone(),
                &ExecuteMsg::SetRoute {
                    token: token.to_string(),
                    path: path.clone(),
                },
                &[],
            )
            .unwrap();
            app.execute_contract(
                Addr::unchecked("owner"),
                treasury.clone(),
                &racer_treasury::ExecuteMsg::SetRoute {
                    token: token.to_string(),
                    path,
                },
                &[],
            )
            .unwrap();
        }
        let code = app.store_code(Box::new(ContractWrapper::new(
            racer_pause::execute,
            racer_pause::instantiate,
            racer_pause::query,
        )));
        let pause = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_pause::InstantiateMsg {
                    owner: "owner".into(),
                },
                &[],
                "pause",
                None,
            )
            .unwrap();
        for (address, msg) in [
            (
                payments.clone(),
                to_json_binary(&ExecuteMsg::SetPauseController {
                    controller: Some(pause.to_string()),
                })
                .unwrap(),
            ),
            (
                treasury.clone(),
                to_json_binary(&racer_treasury::ExecuteMsg::SetPauseController {
                    controller: Some(pause.to_string()),
                })
                .unwrap(),
            ),
            (
                fleet.clone(),
                to_json_binary(&racer_fleet::ExecuteMsg::SetPauseController {
                    controller: Some(pause.to_string()),
                })
                .unwrap(),
            ),
        ] {
            app.execute(
                Addr::unchecked("owner"),
                cosmwasm_std::WasmMsg::Execute {
                    contract_addr: address.to_string(),
                    msg,
                    funds: vec![],
                }
                .into(),
            )
            .unwrap();
        }
        Self {
            app,
            tokens,
            usd,
            router,
            payments,
            fleet,
            treasury,
            pause,
        }
    }
    fn balance(&self, token: &Addr, wallet: &str) -> Uint128 {
        let b: BalanceResponse = self
            .app
            .wrap()
            .query_wasm_smart(
                token,
                &Cw20QueryMsg::Balance {
                    address: wallet.into(),
                },
            )
            .unwrap();
        b.balance
    }
    fn fleet(&self, wallet: &str) -> Fleet {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.fleet,
                &racer_fleet::QueryMsg::Fleet {
                    wallet: wallet.into(),
                },
            )
            .unwrap()
    }
    fn payment(&self, index: usize, purchase: Purchase, price: u128) -> Payment {
        Payment {
            purchase,
            expected_price: Uint128::new(price),
            maximum_price: None,
            minimum_receive: Uint128::new(price),
            max_spread: Decimal::percent(2),
            deadline: self.app.block_info().time.seconds() + 300,
            route: if index == 4 {
                vec![self.usd.to_string()]
            } else {
                vec![self.tokens[index].to_string(), self.usd.to_string()]
            },
        }
    }
    fn pay(&mut self, index: usize, amount: u128, msg: Payment) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked("player"),
                self.tokens[index].clone(),
                &Cw20ExecuteMsg::Send {
                    contract: self.payments.to_string(),
                    amount: Uint128::new(amount),
                    msg: to_json_binary(&msg).unwrap(),
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
fn buy() -> Purchase {
    Purchase::Buy {
        ship: ShipType::Racer,
        target_tier: 1,
        sponsor: None,
    }
}
#[test]
fn all_five_assets_buy_real_fleet_tiers_without_assigning_ships_to_the_adapter() {
    let mut s = Suite::new();
    for index in 0..5 {
        let purchase = if index == 0 {
            buy()
        } else {
            Purchase::Upgrade {
                ship: ShipType::Racer,
                target_tier: index as u16 + 1,
            }
        };
        let quote: racer_payments::Quote = s
            .app
            .wrap()
            .query_wasm_smart(
                &s.payments,
                &QueryMsg::Quote {
                    wallet: "player".into(),
                    token: s.tokens[index].to_string(),
                    purchase: purchase.clone(),
                },
            )
            .unwrap();
        assert_eq!(quote.price, Uint128::new(1_000_000));
        let before = s.balance(&s.usd, "player");
        s.pay(index, 1_100_000, s.payment(index, purchase, 1_000_000))
            .unwrap();
        assert_eq!(s.fleet("player").next, [index as u16 + 1, 0, 0]);
        assert_eq!(s.fleet(s.payments.as_str()).next, [0, 0, 0]);
        assert_eq!(s.balance(&s.usd, s.payments.as_str()), Uint128::zero());
        if index != 4 {
            assert!(s.balance(&s.usd, "player") > before);
        }
    }
    assert_eq!(
        s.balance(&s.usd, s.treasury.as_str()),
        Uint128::new(5_000_000)
    );
}
#[test]
fn donation_cannot_cover_failed_minimum_and_swap_failure_rolls_back_everything() {
    let mut s = Suite::new();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.usd.clone(),
            &Cw20ExecuteMsg::Transfer {
                recipient: s.payments.to_string(),
                amount: Uint128::new(5_000_000),
            },
            &[],
        )
        .unwrap();
    let before = s.balance(&s.tokens[0], "player");
    assert!(s.pay(0, 900_000, s.payment(0, buy(), 1_000_000)).is_err());
    assert_eq!(s.balance(&s.tokens[0], "player"), before);
    assert_eq!(s.fleet("player").next, [0, 0, 0]);
    assert_eq!(
        s.balance(&s.usd, s.payments.as_str()),
        Uint128::new(5_000_000)
    );
    s.pay(0, 1_100_000, s.payment(0, buy(), 1_000_000)).unwrap();
    assert_eq!(
        s.balance(&s.usd, s.payments.as_str()),
        Uint128::new(5_000_000)
    );
}

#[test]
fn final_fleet_rejection_reverts_the_upstream_pool_swap_and_payment() {
    let mut s = Suite::new();
    let router: cl8y_dex_router::msg::ConfigResponse = s
        .app
        .wrap()
        .query_wasm_smart(&s.router, &cl8y_dex_router::msg::QueryMsg::Config {})
        .unwrap();
    let pair: dex_common::factory::PairResponse = s
        .app
        .wrap()
        .query_wasm_smart(
            &router.factory,
            &dex_common::factory::QueryMsg::Pair {
                asset_infos: [token_info(&s.tokens[0]), token_info(&s.usd)],
            },
        )
        .unwrap();
    let before: dex_common::pair::PoolResponse = s
        .app
        .wrap()
        .query_wasm_smart(
            &pair.pair.contract_addr,
            &dex_common::pair::QueryMsg::Pool {},
        )
        .unwrap();
    let balance = s.balance(&s.tokens[0], "player");
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.fleet.clone(),
            &racer_fleet::ExecuteMsg::SetPaymentAdapter { adapter: None },
            &[],
        )
        .unwrap();
    assert!(s.pay(0, 1_100_000, s.payment(0, buy(), 1_000_000)).is_err());
    let after: dex_common::pair::PoolResponse = s
        .app
        .wrap()
        .query_wasm_smart(
            &pair.pair.contract_addr,
            &dex_common::pair::QueryMsg::Pool {},
        )
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(s.balance(&s.tokens[0], "player"), balance);
    assert_eq!(s.balance(&s.usd, s.payments.as_str()), Uint128::zero());
    assert_eq!(s.fleet("player").next, [0, 0, 0]);
}
#[test]
fn expired_quotes_wrong_invoices_routes_and_forwarded_impersonation_are_rejected() {
    let mut s = Suite::new();
    let mut request = s.payment(0, buy(), 1_000_000);
    request.deadline = 1;
    assert!(s.pay(0, 1_100_000, request).is_err());
    let mut request = s.payment(0, buy(), 1_000_000);
    request.route.reverse();
    assert!(s.pay(0, 1_100_000, request).is_err());
    assert!(s.pay(0, 1_100_000, s.payment(0, buy(), 999_999)).is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.usd.clone(),
            &Cw20ExecuteMsg::Send {
                contract: s.fleet.to_string(),
                amount: Uint128::new(1_000_000),
                msg: to_json_binary(&racer_fleet::ReceiveMsg::PayFor {
                    wallet: "sponsor".into(),
                    purchase: buy()
                })
                .unwrap()
            },
            &[]
        )
        .is_err());
    let mut request = s.payment(0, buy(), 1_000_000);
    request.max_spread = Decimal::percent(50);
    assert!(s.pay(0, 1_100_000, request).is_err());
    assert_eq!(s.fleet("sponsor").next, [0, 0, 0]);
}
#[test]
fn treasury_converts_in_batches_and_only_operator_can_send_spaceusd_capital() {
    let mut s = Suite::new();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.tokens[0].clone(),
            &Cw20ExecuteMsg::Transfer {
                recipient: s.treasury.to_string(),
                amount: Uint128::new(2_000_000),
            },
            &[],
        )
        .unwrap();
    let msg = racer_treasury::ExecuteMsg::Convert {
        token: s.tokens[0].to_string(),
        amount: Uint128::new(2_000_000),
        minimum_receive: Uint128::new(1_900_000),
        max_spread: Decimal::percent(2),
        deadline: s.app.block_info().time.seconds() + 300,
        route: vec![s.tokens[0].to_string(), s.usd.to_string()],
    };
    assert!(s
        .app
        .execute_contract(Addr::unchecked("attacker"), s.treasury.clone(), &msg, &[])
        .is_err());
    s.app
        .execute_contract(Addr::unchecked("owner"), s.treasury.clone(), &msg, &[])
        .unwrap();
    assert!(s.balance(&s.usd, s.treasury.as_str()) > Uint128::new(1_900_000));
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.treasury.clone(),
            &racer_treasury::ExecuteMsg::SendCapital {
                amount: Uint128::new(1_000_000)
            },
            &[]
        )
        .is_err());
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &racer_treasury::ExecuteMsg::SendCapital {
                amount: Uint128::new(1_000_000),
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.balance(&s.usd, "overlord"), Uint128::new(1_000_000));
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.pause.clone(),
            &racer_pause::ExecuteMsg::SetPaused { paused: true },
            &[],
        )
        .unwrap();
    assert!(s.pay(4, 1_100_000, s.payment(4, buy(), 1_000_000)).is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &racer_treasury::ExecuteMsg::SendCapital {
                amount: Uint128::new(1)
            },
            &[]
        )
        .is_err());
}
#[test]
fn multiple_hops_use_upstream_router_and_preserve_referral_identity() {
    let mut s = Suite::new();
    // Reject a repeated USD hop, then use the real cLUNC/SRT and SRT/USD pools.
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("owner"),
            s.payments.clone(),
            &ExecuteMsg::SetRoute {
                token: s.tokens[0].to_string(),
                path: vec![
                    s.tokens[0].to_string(),
                    s.usd.to_string(),
                    s.tokens[3].to_string(),
                    s.usd.to_string()
                ]
            },
            &[]
        )
        .is_err());
    let mut purchase = buy();
    if let Purchase::Buy { sponsor, .. } = &mut purchase {
        *sponsor = Some("sponsor".into());
    }
    let path = vec![
        s.tokens[0].to_string(),
        s.tokens[3].to_string(),
        s.usd.to_string(),
    ];
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.payments.clone(),
            &ExecuteMsg::SetRoute {
                token: s.tokens[0].to_string(),
                path: path.clone(),
            },
            &[],
        )
        .unwrap();
    let mut request = s.payment(0, purchase, 950_000);
    request.route = path;
    s.pay(0, 1_100_000, request).unwrap();
    assert_eq!(s.fleet("player").sponsor, Some(Addr::unchecked("sponsor")));
    assert_eq!(
        s.balance(&s.usd, s.treasury.as_str()),
        Uint128::new(902_500)
    );
    // Keep the router fixture referenced: its balance is not a user account.
    assert_eq!(s.balance(&s.usd, s.router.as_str()), Uint128::zero());
}
