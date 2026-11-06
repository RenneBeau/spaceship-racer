use cosmwasm_std::{to_json_binary, Addr, Empty, Uint128};
use cw20::{BalanceResponse, Cw20Coin, Cw20ExecuteMsg, Cw20QueryMsg};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use racer_rewards::{Allocation, Credit, ExecuteMsg, InstantiateMsg, QueryMsg, ReceiveMsg, State};

fn rewards() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_rewards::execute,
        racer_rewards::instantiate,
        racer_rewards::query,
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
    usd: Addr,
    srt: Addr,
    rewards: Addr,
}
impl Suite {
    fn new() -> Self {
        let mut app = App::default();
        let code = app.store_code(token());
        let mut create = |name: &str| {
            app.instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &cw20_base::msg::InstantiateMsg {
                    name: name.into(),
                    symbol: name.into(),
                    decimals: 6,
                    initial_balances: vec![Cw20Coin {
                        address: "owner".into(),
                        amount: Uint128::new(1_000_000_000),
                    }],
                    mint: None,
                    marketing: None,
                },
                &[],
                name,
                None,
            )
            .unwrap()
        };
        let usd = create("USD");
        let srt = create("SRT");
        let code = app.store_code(rewards());
        let rewards = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &InstantiateMsg {
                    owner: "owner".into(),
                    allocator: "engine".into(),
                    spaceusd: usd.to_string(),
                    srt: srt.to_string(),
                    annual_bps: 2500,
                },
                &[],
                "rewards",
                None,
            )
            .unwrap();
        Self {
            app,
            usd,
            srt,
            rewards,
        }
    }
    fn fund(&mut self, token: Addr, amount: u128) {
        self.app
            .execute_contract(
                Addr::unchecked("owner"),
                token,
                &Cw20ExecuteMsg::Send {
                    contract: self.rewards.to_string(),
                    amount: Uint128::new(amount),
                    msg: to_json_binary(&ReceiveMsg::Fund {}).unwrap(),
                },
                &[],
            )
            .unwrap();
    }
    fn state(&self) -> State {
        self.app
            .wrap()
            .query_wasm_smart(&self.rewards, &QueryMsg::State {})
            .unwrap()
    }
    fn allocate(&mut self, usd: u128, srt: u128) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked("engine"),
                self.rewards.clone(),
                &ExecuteMsg::Allocate {
                    recipients: vec![Allocation {
                        wallet: "player".into(),
                        spaceusd: Uint128::new(usd),
                        srt: Uint128::new(srt),
                    }],
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[test]
fn funded_reserve_accrues_then_batches_claims_without_double_spend() {
    let mut s = Suite::new();
    s.fund(s.usd.clone(), 1_000_000);
    s.fund(s.srt.clone(), 500);
    assert!(s.allocate(1, 0).is_err());
    s.app.update_block(|block| {
        block.time = block.time.plus_seconds(game_common::YEAR_SECONDS);
    });
    s.allocate(100_000, 100).unwrap();
    s.allocate(150_000, 200).unwrap();
    assert!(s.allocate(1, 0).is_err());
    let state = s.state();
    assert_eq!(state.reserve, Uint128::new(750_000));
    assert_eq!(state.liabilities.spaceusd, Uint128::new(250_000));
    assert_eq!(state.available.srt, Uint128::new(200));
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.rewards.clone(),
            &ExecuteMsg::Claim {},
            &[],
        )
        .unwrap();
    for (token, expected) in [(s.usd.clone(), 250_000), (s.srt.clone(), 300)] {
        let balance: BalanceResponse = s
            .app
            .wrap()
            .query_wasm_smart(
                token,
                &Cw20QueryMsg::Balance {
                    address: "player".into(),
                },
            )
            .unwrap();
        assert_eq!(balance.balance, Uint128::new(expected));
    }
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.rewards.clone(),
            &ExecuteMsg::Claim {},
            &[]
        )
        .is_err());
    assert_eq!(s.state().liabilities, Credit::default());
}

#[test]
fn authorization_pause_and_failed_batch_are_atomic() {
    let mut s = Suite::new();
    s.fund(s.srt.clone(), 100);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.rewards.clone(),
            &ExecuteMsg::Configure {
                allocator: Some("attacker".into()),
                annual_bps: None,
                paused: None
            },
            &[]
        )
        .is_err());
    let batch = ExecuteMsg::Allocate {
        recipients: vec![
            Allocation {
                wallet: "player".into(),
                spaceusd: Uint128::zero(),
                srt: Uint128::new(50),
            },
            Allocation {
                wallet: "other".into(),
                spaceusd: Uint128::zero(),
                srt: Uint128::new(100),
            },
        ],
    };
    assert!(s
        .app
        .execute_contract(Addr::unchecked("engine"), s.rewards.clone(), &batch, &[])
        .is_err());
    assert_eq!(s.state().available.srt, Uint128::new(100));
    let credit: Credit = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.rewards,
            &QueryMsg::Credit {
                wallet: "player".into(),
            },
        )
        .unwrap();
    assert_eq!(credit, Credit::default());
    s.allocate(0, 50).unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.rewards.clone(),
            &ExecuteMsg::Configure {
                allocator: None,
                annual_bps: None,
                paused: Some(true),
            },
            &[],
        )
        .unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.rewards.clone(),
            &ExecuteMsg::Claim {},
            &[]
        )
        .is_err());
    assert!(s.allocate(0, 1).is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.rewards.clone(),
            &ExecuteMsg::Receive(cw20::Cw20ReceiveMsg {
                sender: "owner".into(),
                amount: Uint128::new(50),
                msg: to_json_binary(&ReceiveMsg::Fund {}).unwrap()
            }),
            &[]
        )
        .is_err());
}

#[test]
fn frequent_checkpoints_preserve_fractional_accrual() {
    let mut s = Suite::new();
    s.fund(s.usd.clone(), 1000);
    // Each hour accrues less than one micro-unit, but the accumulated fraction must survive.
    for _ in 0..40 {
        s.app.update_block(|b| b.time = b.time.plus_seconds(3600));
        s.app
            .execute_contract(
                Addr::unchecked("keeper"),
                s.rewards.clone(),
                &ExecuteMsg::Checkpoint {},
                &[],
            )
            .unwrap();
    }
    assert_eq!(s.state().available.spaceusd, Uint128::new(1));
}
