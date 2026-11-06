//! Treasury -> global reward pool and Collapsar capital routing.
use cosmwasm_std::{Addr, Decimal, Empty, Uint128};
use cw20::{BalanceResponse, Cw20Coin, Cw20ExecuteMsg, Cw20QueryMsg};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use racer_rewards::{QueryMsg as RewardQuery, State as RewardState};
use racer_treasury::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};

fn token() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    ))
}
fn rewards() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_rewards::execute,
        racer_rewards::instantiate,
        racer_rewards::query,
    ))
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
fn cw20(app: &mut App, code: u64, symbol: &str) -> Addr {
    app.instantiate_contract(
        code,
        Addr::unchecked("owner"),
        &cw20_base::msg::InstantiateMsg {
            name: symbol.into(),
            symbol: symbol.into(),
            decimals: 6,
            initial_balances: vec![Cw20Coin {
                address: "owner".into(),
                amount: Uint128::new(20_000_000),
            }],
            mint: None,
            marketing: None,
        },
        &[],
        symbol,
        None,
    )
    .unwrap()
}
fn balance(app: &App, token: &Addr, address: &Addr) -> Uint128 {
    let response: BalanceResponse = app
        .wrap()
        .query_wasm_smart(
            token,
            &Cw20QueryMsg::Balance {
                address: address.to_string(),
            },
        )
        .unwrap();
    response.balance
}
struct Suite {
    app: App,
    treasury: Addr,
    rewards: Addr,
    srt: Addr,
    usd: Addr,
}
fn suite(reward_pool: bool) -> Suite {
    let mut app = App::default();
    let token_code = app.store_code(token());
    let srt = cw20(&mut app, token_code, "SRT");
    let usd = cw20(&mut app, token_code, "SpaceUSD");
    let rewards_code = app.store_code(rewards());
    let rewards = app
        .instantiate_contract(
            rewards_code,
            Addr::unchecked("owner"),
            &racer_rewards::InstantiateMsg {
                owner: "owner".into(),
                allocator: "owner".into(),
                spaceusd: usd.to_string(),
                srt: srt.to_string(),
                annual_bps: 0,
            },
            &[],
            "rewards",
            None,
        )
        .unwrap();
    let treasury_code = app.store_code(treasury());
    let treasury = app
        .instantiate_contract(
            treasury_code,
            Addr::unchecked("owner"),
            &InstantiateMsg {
                owner: "owner".into(),
                overlord: "collapsar".into(),
                router: "router".into(),
                spaceusd: usd.to_string(),
                reward_pool: reward_pool.then(|| rewards.to_string()),
                payment_tokens: vec![srt.to_string(), usd.to_string()],
                max_spread: Decimal::percent(2),
            },
            &[],
            "treasury",
            None,
        )
        .unwrap();
    for token in [&srt, &usd] {
        app.execute_contract(
            Addr::unchecked("owner"),
            token.clone(),
            &Cw20ExecuteMsg::Transfer {
                recipient: treasury.to_string(),
                amount: Uint128::new(10_000_000),
            },
            &[],
        )
        .unwrap();
    }
    Suite {
        app,
        treasury,
        rewards,
        srt,
        usd,
    }
}
fn rewards_state(app: &App, rewards: &Addr) -> RewardState {
    app.wrap()
        .query_wasm_smart(rewards, &RewardQuery::State {})
        .unwrap()
}

#[test]
fn treasury_funds_srt_and_spaceusd_reward_pools() {
    let mut s = suite(true);
    let before = rewards_state(&s.app, &s.rewards);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &ExecuteMsg::FundRewardPool {
                token: s.srt.to_string(),
                amount: Uint128::new(1_500_000),
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &ExecuteMsg::FundRewardPool {
                token: s.usd.to_string(),
                amount: Uint128::new(2_500_000),
            },
            &[],
        )
        .unwrap();
    let after = rewards_state(&s.app, &s.rewards);
    assert_eq!(
        after.available.srt.u128(),
        before.available.srt.u128() + 1_500_000
    );
    assert_eq!(after.reserve.u128(), before.reserve.u128() + 2_500_000);
    assert_eq!(balance(&s.app, &s.srt, &s.treasury).u128(), 8_500_000);
    assert_eq!(balance(&s.app, &s.usd, &s.treasury).u128(), 7_500_000);
    assert_eq!(balance(&s.app, &s.srt, &s.rewards).u128(), 1_500_000);
    assert_eq!(balance(&s.app, &s.usd, &s.rewards).u128(), 2_500_000);
    let config: Config = s
        .app
        .wrap()
        .query_wasm_smart(&s.treasury, &QueryMsg::Config {})
        .unwrap();
    assert_eq!(config.reward_pool, Some(s.rewards.clone()));
}

#[test]
fn capital_and_reward_destinations_are_separate_and_owner_controlled() {
    let mut s = suite(true);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &ExecuteMsg::SendCapital {
                amount: Uint128::new(1_000_000),
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        balance(&s.app, &s.usd, &Addr::unchecked("collapsar")).u128(),
        1_000_000
    );
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &ExecuteMsg::SetCollapsar {
                address: "new-collapsar".into(),
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &ExecuteMsg::SendCapital {
                amount: Uint128::new(1_000_000),
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        balance(&s.app, &s.usd, &Addr::unchecked("new-collapsar")).u128(),
        1_000_000
    );
    let unauthorized = s.app.execute_contract(
        Addr::unchecked("buyer"),
        s.treasury.clone(),
        &ExecuteMsg::FundRewardPool {
            token: s.srt.to_string(),
            amount: Uint128::new(1),
        },
        &[],
    );
    assert!(unauthorized.is_err());
    assert_eq!(balance(&s.app, &s.srt, &s.rewards).u128(), 0);
}

#[test]
fn missing_reward_pool_and_unbacked_funding_are_rejected() {
    let mut s = suite(false);
    let error = s
        .app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &ExecuteMsg::FundRewardPool {
                token: s.srt.to_string(),
                amount: Uint128::new(1),
            },
            &[],
        )
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("reward pool not configured"),
        "{error:?}"
    );
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &ExecuteMsg::SetRewardPool {
                address: Some(s.rewards.to_string()),
            },
            &[],
        )
        .unwrap();
    let error = s
        .app
        .execute_contract(
            Addr::unchecked("owner"),
            s.treasury.clone(),
            &ExecuteMsg::FundRewardPool {
                token: s.srt.to_string(),
                amount: Uint128::new(20_000_000),
            },
            &[],
        )
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("insufficient treasury balance"),
        "{error:?}"
    );
}

#[test]
fn treasury_reports_its_token_balance() {
    let s = suite(true);
    let reported: Uint128 = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.treasury,
            &QueryMsg::Balance {
                token: s.srt.to_string(),
            },
        )
        .unwrap();
    assert_eq!(reported.u128(), 10_000_000);
}
