use cosmwasm_std::{to_json_binary, Addr, Binary, Empty, Timestamp, Uint128};
use cw20::{BalanceResponse, Cw20Coin, Cw20ExecuteMsg, Cw20QueryMsg};
use cw721::{Cw721QueryMsg, NftInfoResponse, OwnerOfResponse};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use racer_collection::{Attributes, Bonus, Kind, Rarity};
use racer_recruitment::{Entry, ExecuteMsg, Job, QueryMsg, RecruitMsg};

fn nft() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_collection::execute,
        racer_collection::instantiate,
        racer_collection::query,
    ))
}
fn token() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    ))
}
fn market() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_marketplace::execute,
        racer_marketplace::instantiate,
        racer_marketplace::query,
    ))
}
fn recruit() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_recruitment::execute,
        racer_recruitment::instantiate,
        racer_recruitment::query,
    ))
}
fn randomness() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_randomness::execute,
        racer_randomness::instantiate,
        racer_randomness::query,
    ))
}
fn pause() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_pause::execute,
        racer_pause::instantiate,
        racer_pause::query,
    ))
}
fn equipment() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_equipment::execute,
        racer_equipment::instantiate,
        racer_equipment::query,
    ))
}

fn catalogue() -> Vec<Entry> {
    [
        Rarity::Common,
        Rarity::Rare,
        Rarity::Epic,
        Rarity::Legendary,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, rarity)| Entry {
        attributes: Attributes {
            code: format!("commander_{i}"),
            kind: Kind::Commander,
            rarity,
            bonus: Bonus::Speed,
            bonus_bps: 300,
        },
        weight: [700, 270, 25, 5][i],
        token_uri: None,
    })
    .collect()
}

struct Suite {
    app: App,
    nft: Addr,
    tokens: Vec<Addr>,
    market: Addr,
    recruit: Addr,
    rng: Addr,
    pause: Addr,
    equipment: Addr,
}
impl Suite {
    fn new() -> Self {
        let mut app = App::default();
        app.update_block(|b| b.time = Timestamp::from_seconds(racer_randomness::GENESIS + 357));
        let code = app.store_code(token());
        let mut tokens = vec![];
        // cw20-base's fixture symbol validator excludes digits; the third
        // contract stands in for UST1 regardless of this local display symbol.
        for symbol in ["CLUNC", "CUSTC", "UST", "USD", "SRT"] {
            tokens.push(
                app.instantiate_contract(
                    code,
                    Addr::unchecked("owner"),
                    &cw20_base::msg::InstantiateMsg {
                        name: symbol.into(),
                        symbol: symbol.into(),
                        decimals: 6,
                        initial_balances: ["owner", "player", "buyer"]
                            .into_iter()
                            .map(|a| Cw20Coin {
                                address: a.into(),
                                amount: Uint128::new(1_000_000_000_000),
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
        let code = app.store_code(nft());
        let nft = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_collection::InstantiateMsg {
                    name: "Commanders".into(),
                    symbol: "CMD".into(),
                    owner: "owner".into(),
                    minter: "owner".into(),
                },
                &[],
                "commanders",
                None,
            )
            .unwrap();
        for n in 0..7 {
            app.execute_contract(
                Addr::unchecked("owner"),
                nft.clone(),
                &racer_collection::ExecuteMsg::Mint {
                    token_id: format!("common{n}"),
                    owner: "player".into(),
                    token_uri: None,
                    extension: catalogue()[0].attributes.clone(),
                },
                &[],
            )
            .unwrap();
        }
        for (id, index) in [("rare", 1), ("legend", 3)] {
            app.execute_contract(
                Addr::unchecked("owner"),
                nft.clone(),
                &racer_collection::ExecuteMsg::Mint {
                    token_id: id.into(),
                    owner: "player".into(),
                    token_uri: None,
                    extension: catalogue()[index].attributes.clone(),
                },
                &[],
            )
            .unwrap();
        }
        let code = app.store_code(market());
        let market = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_marketplace::InstantiateMsg {
                    owner: "owner".into(),
                    treasury: "treasury".into(),
                    fee_bps: 500,
                    collections: vec![nft.to_string()],
                    payment_tokens: tokens.iter().map(|a| a.to_string()).collect(),
                },
                &[],
                "market",
                None,
            )
            .unwrap();
        let code = app.store_code(randomness());
        let rng = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_randomness::InstantiateMsg {
                    owner: "owner".into(),
                    consumers: vec!["owner".into()],
                    safety_seconds: 6,
                },
                &[],
                "randomness",
                None,
            )
            .unwrap();
        let code = app.store_code(recruit());
        let recruit = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_recruitment::InstantiateMsg {
                    owner: "owner".into(),
                    srt: tokens[4].to_string(),
                    collection: nft.to_string(),
                    randomness: rng.to_string(),
                    treasury: "treasury".into(),
                    pull_price: Uint128::new(500_000_000),
                    catalogue: catalogue(),
                },
                &[],
                "recruitment",
                None,
            )
            .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            rng.clone(),
            &racer_randomness::ExecuteMsg::Configure {
                paused: None,
                consumer: Some(recruit.to_string()),
                allowed: Some(true),
            },
            &[],
        )
        .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            nft.clone(),
            &racer_collection::ExecuteMsg::UpdateOwnership(cw_ownable::Action::TransferOwnership {
                new_owner: recruit.to_string(),
                expiry: None,
            }),
            &[],
        )
        .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            recruit.clone(),
            &ExecuteMsg::AcceptMinter {},
            &[],
        )
        .unwrap();
        let code = app.store_code(pause());
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
        let code = app.store_code(equipment());
        let equipment = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_equipment::InstantiateMsg {
                    owner: "owner".into(),
                    engine: "owner".into(),
                    collections: vec![nft.to_string()],
                },
                &[],
                "equipment",
                None,
            )
            .unwrap();
        let mut s = Self {
            app,
            nft,
            tokens,
            market,
            recruit,
            rng,
            pause,
            equipment,
        };
        s.wire_pause();
        s
    }
    fn wire_pause(&mut self) {
        for (addr, msg) in [
            (
                self.nft.clone(),
                to_json_binary(&racer_collection::ExecuteMsg::Extension {
                    msg: racer_collection::ExtensionMsg::SetPauseController {
                        controller: Some(self.pause.to_string()),
                    },
                })
                .unwrap(),
            ),
            (
                self.market.clone(),
                to_json_binary(&racer_marketplace::ExecuteMsg::SetPauseController {
                    controller: Some(self.pause.to_string()),
                })
                .unwrap(),
            ),
            (
                self.recruit.clone(),
                to_json_binary(&ExecuteMsg::SetPauseController {
                    controller: Some(self.pause.to_string()),
                })
                .unwrap(),
            ),
            (
                self.rng.clone(),
                to_json_binary(&racer_randomness::ExecuteMsg::SetPauseController {
                    controller: Some(self.pause.to_string()),
                })
                .unwrap(),
            ),
            (
                self.equipment.clone(),
                to_json_binary(&racer_equipment::ExecuteMsg::SetPauseController {
                    controller: Some(self.pause.to_string()),
                })
                .unwrap(),
            ),
        ] {
            self.app
                .execute(
                    Addr::unchecked("owner"),
                    cosmwasm_std::WasmMsg::Execute {
                        contract_addr: addr.to_string(),
                        msg,
                        funds: vec![],
                    }
                    .into(),
                )
                .unwrap();
        }
    }
    fn set_paused(&mut self, paused: bool) {
        self.app
            .execute_contract(
                Addr::unchecked("owner"),
                self.pause.clone(),
                &racer_pause::ExecuteMsg::SetPaused { paused },
                &[],
            )
            .unwrap();
    }
    fn balance(&self, index: usize, wallet: &str) -> Uint128 {
        let b: BalanceResponse = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.tokens[index],
                &Cw20QueryMsg::Balance {
                    address: wallet.into(),
                },
            )
            .unwrap();
        b.balance
    }
    fn owner(&self, id: &str) -> String {
        let owner: OwnerOfResponse = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.nft,
                &Cw721QueryMsg::OwnerOf {
                    token_id: id.into(),
                    include_expired: None,
                },
            )
            .unwrap();
        owner.owner
    }
    fn job(&self, id: u64) -> Job {
        self.app
            .wrap()
            .query_wasm_smart(&self.recruit, &QueryMsg::Job { id })
            .unwrap()
    }
    fn pull(&mut self, count: u8) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked("player"),
                self.tokens[4].clone(),
                &Cw20ExecuteMsg::Send {
                    contract: self.recruit.to_string(),
                    amount: Uint128::new(500_000_000 * u128::from(count)),
                    msg: to_json_binary(&RecruitMsg { count }).unwrap(),
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn prove(&mut self, id: u64) {
        let request: racer_randomness::Request = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.rng,
                &racer_randomness::QueryMsg::Request {
                    consumer: self.recruit.to_string(),
                    id,
                },
            )
            .unwrap();
        assert_eq!(request.round, 123);
        self.app
            .update_block(|b| b.time = Timestamp::from_seconds(request.available_at));
        self.app.execute_contract(Addr::unchecked("relayer"),self.rng.clone(),&racer_randomness::ExecuteMsg::Submit { consumer: self.recruit.to_string(),id,
            signature: Binary::from(hex_decode("b75c69d0b72a5d906e854e808ba7e2accb1542ac355ae486d591aa9d43765482e26cd02df835d3546d23c4b13e0dfc92")) },&[]).unwrap();
    }
    fn list(&mut self, id: &str, token_index: usize) {
        self.app
            .execute_contract(
                Addr::unchecked("player"),
                self.nft.clone(),
                &racer_collection::ExecuteMsg::SendNft {
                    contract: self.market.to_string(),
                    token_id: id.into(),
                    msg: to_json_binary(&racer_marketplace::ListMsg {
                        payment_token: self.tokens[token_index].to_string(),
                        price: Uint128::new(1000),
                    })
                    .unwrap(),
                },
                &[],
            )
            .unwrap();
    }
    fn buy(&mut self, id: u64, index: usize, amount: u128) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked("buyer"),
                self.tokens[index].clone(),
                &Cw20ExecuteMsg::Send {
                    contract: self.market.to_string(),
                    amount: Uint128::new(amount),
                    msg: to_json_binary(&racer_marketplace::BuyMsg { listing_id: id }).unwrap(),
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn approve_fusion(&mut self) {
        self.app
            .execute_contract(
                Addr::unchecked("player"),
                self.nft.clone(),
                &racer_collection::ExecuteMsg::ApproveAll {
                    operator: self.recruit.to_string(),
                    expires: None,
                },
                &[],
            )
            .unwrap();
    }
    fn fuse(&mut self, ids: Vec<String>) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked("player"),
                self.recruit.clone(),
                &ExecuteMsg::Fuse { token_ids: ids },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
fn hex_decode(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn five() -> Vec<String> {
    (0..5).map(|n| format!("common{n}")).collect()
}

#[test]
fn marketplace_settles_all_five_assets_with_snapshotted_fees_and_no_double_sale() {
    let mut s = Suite::new();
    for n in 0..5 {
        s.list(&format!("common{n}"), n);
    }
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.market.clone(),
            &racer_marketplace::ExecuteMsg::Configure {
                treasury: Some("new-treasury".into()),
                fee_bps: Some(1000),
            },
            &[],
        )
        .unwrap();
    for n in 0..5 {
        let before = s.balance(n, "buyer");
        assert!(s.buy(n as u64 + 1, n, 999).is_err());
        assert_eq!(s.balance(n, "buyer"), before);
        s.buy(n as u64 + 1, n, 1000).unwrap();
        assert_eq!(s.owner(&format!("common{n}")), "buyer");
        assert_eq!(s.balance(n, "treasury"), Uint128::new(50));
        assert_eq!(s.balance(n, "new-treasury"), Uint128::zero());
        assert!(s.buy(n as u64 + 1, n, 1000).is_err());
    }
    let active: Vec<racer_marketplace::Listing> = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.market,
            &racer_marketplace::QueryMsg::Active {
                start_after: None,
                limit: None,
            },
        )
        .unwrap();
    assert!(active.is_empty());
}

#[test]
fn wrong_token_forged_callback_and_unauthorized_cancel_do_not_move_assets() {
    let mut s = Suite::new();
    s.list("common0", 3);
    assert!(s.buy(1, 4, 1000).is_err());
    assert_eq!(s.owner("common0"), s.market.to_string());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.market.clone(),
            &racer_marketplace::ExecuteMsg::ReceiveNft(cw721::Cw721ReceiveMsg {
                sender: "player".into(),
                token_id: "common1".into(),
                msg: to_json_binary(&racer_marketplace::ListMsg {
                    payment_token: s.tokens[3].to_string(),
                    price: Uint128::new(1000)
                })
                .unwrap()
            }),
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("buyer"),
            s.market.clone(),
            &racer_marketplace::ExecuteMsg::Cancel { listing_id: 1 },
            &[]
        )
        .is_err());
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.market.clone(),
            &racer_marketplace::ExecuteMsg::AllowCollection {
                collection: s.nft.to_string(),
                allowed: false,
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.market.clone(),
            &racer_marketplace::ExecuteMsg::Cancel { listing_id: 1 },
            &[],
        )
        .unwrap();
    assert_eq!(s.owner("common0"), "player");
}

#[test]
fn ten_pull_uses_committed_catalogue_and_guarantees_epic_without_rerolls() {
    let mut s = Suite::new();
    s.pull(10).unwrap();
    assert_eq!(
        s.balance(4, s.recruit.as_str()),
        Uint128::new(5_000_000_000)
    );
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("relayer"),
            s.recruit.clone(),
            &ExecuteMsg::Fulfill { id: 1 },
            &[]
        )
        .is_err());
    let mut changed = catalogue();
    for e in &mut changed {
        e.attributes.code.push_str("_new");
    }
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.recruit.clone(),
            &ExecuteMsg::Configure {
                treasury: Some("new-treasury".into()),
                pull_price: Some(Uint128::new(1)),
                catalogue: Some(changed),
            },
            &[],
        )
        .unwrap();
    s.prove(1);
    s.app
        .execute_contract(
            Addr::unchecked("relayer"),
            s.recruit.clone(),
            &ExecuteMsg::Fulfill { id: 1 },
            &[],
        )
        .unwrap();
    let job = s.job(1);
    assert_eq!(job.minted.len(), 10);
    for (n, id) in job.minted.iter().enumerate() {
        assert_eq!(s.owner(id), "player");
        let nft: NftInfoResponse<Attributes> = s
            .app
            .wrap()
            .query_wasm_smart(
                &s.nft,
                &Cw721QueryMsg::NftInfo {
                    token_id: id.clone(),
                },
            )
            .unwrap();
        assert!(!nft.extension.code.ends_with("_new"));
        if n == 0 {
            assert!(matches!(
                nft.extension.rarity,
                Rarity::Epic | Rarity::Legendary
            ));
        }
    }
    assert_eq!(s.balance(4, "treasury"), Uint128::new(5_000_000_000));
    assert_eq!(s.balance(4, "new-treasury"), Uint128::zero());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.recruit.clone(),
            &ExecuteMsg::Fulfill { id: 1 },
            &[]
        )
        .is_err());
}

#[test]
fn fusion_requires_approval_then_escrows_burns_and_mints_atomically() {
    let mut s = Suite::new();
    assert!(s.fuse(five()).is_err());
    let cfg: racer_recruitment::Config = s
        .app
        .wrap()
        .query_wasm_smart(&s.recruit, &QueryMsg::Config {})
        .unwrap();
    assert_eq!(cfg.next_id, 1);
    s.approve_fusion();
    assert!(s.fuse(vec!["common0".into(); 5]).is_err());
    assert!(s
        .fuse(vec![
            "common0".into(),
            "common1".into(),
            "common2".into(),
            "common3".into(),
            "rare".into()
        ])
        .is_err());
    s.fuse(five()).unwrap();
    for id in five() {
        assert_eq!(s.owner(&id), s.recruit.to_string());
    }
    s.prove(1);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Extension {
                msg: racer_collection::ExtensionMsg::SetPaused { paused: true },
            },
            &[],
        )
        .unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("relayer"),
            s.recruit.clone(),
            &ExecuteMsg::Fulfill { id: 1 },
            &[]
        )
        .is_err());
    assert!(!s.job(1).completed);
    for id in five() {
        assert_eq!(s.owner(&id), s.recruit.to_string());
    }
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Extension {
                msg: racer_collection::ExtensionMsg::SetPaused { paused: false },
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("relayer"),
            s.recruit.clone(),
            &ExecuteMsg::Fulfill { id: 1 },
            &[],
        )
        .unwrap();
    let output = &s.job(1).minted[0];
    assert_eq!(s.owner(output), "player");
    let nft: NftInfoResponse<Attributes> = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.nft,
            &Cw721QueryMsg::NftInfo {
                token_id: output.clone(),
            },
        )
        .unwrap();
    assert_eq!(nft.extension.rarity, Rarity::Rare);
    for id in five() {
        assert!(s
            .app
            .wrap()
            .query_wasm_smart::<OwnerOfResponse>(
                &s.nft,
                &Cw721QueryMsg::OwnerOf {
                    token_id: id,
                    include_expired: None
                }
            )
            .is_err());
    }
}

#[test]
fn bound_and_listed_commanders_cannot_be_fused() {
    let mut s = Suite::new();
    s.approve_fusion();
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::SendNft {
                contract: s.equipment.to_string(),
                token_id: "common0".into(),
                msg: to_json_binary(&racer_equipment::BindMsg {
                    slot: racer_equipment::Slot::Commander,
                })
                .unwrap(),
            },
            &[],
        )
        .unwrap();
    assert!(s.fuse(five()).is_err());
    s.list("common1", 3);
    assert!(s
        .fuse(vec![
            "common1".into(),
            "common2".into(),
            "common3".into(),
            "common4".into(),
            "common5".into()
        ])
        .is_err());
}

#[test]
fn one_global_pause_blocks_nft_sends_market_recruitment_equipment_and_proofs() {
    let mut s = Suite::new();
    s.list("common0", 3);
    s.pull(1).unwrap();
    s.set_paused(true);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.pause.clone(),
            &racer_pause::ExecuteMsg::SetPaused { paused: false },
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::TransferNft {
                recipient: "buyer".into(),
                token_id: "common1".into()
            },
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::SendNft {
                contract: s.equipment.to_string(),
                token_id: "common1".into(),
                msg: to_json_binary(&racer_equipment::BindMsg {
                    slot: racer_equipment::Slot::Commander
                })
                .unwrap()
            },
            &[]
        )
        .is_err());
    assert!(s.buy(1, 3, 1000).is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.market.clone(),
            &racer_marketplace::ExecuteMsg::Cancel { listing_id: 1 },
            &[]
        )
        .is_err());
    assert!(s.pull(1).is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("relayer"),
            s.recruit.clone(),
            &ExecuteMsg::Fulfill { id: 1 },
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("owner"),
            s.rng.clone(),
            &racer_randomness::ExecuteMsg::Request { id: 99 },
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("owner"),
            s.equipment.clone(),
            &racer_equipment::ExecuteMsg::BeginBoundary {},
            &[]
        )
        .is_err());
    s.set_paused(false);
    s.buy(1, 3, 1000).unwrap();
    s.prove(1);
    s.app
        .execute_contract(
            Addr::unchecked("relayer"),
            s.recruit.clone(),
            &ExecuteMsg::Fulfill { id: 1 },
            &[],
        )
        .unwrap();
    assert_eq!(s.owner("common0"), "buyer");
}

#[test]
fn failed_nft_delivery_rolls_back_market_payment_and_listing_state() {
    let mut s = Suite::new();
    s.list("common0", 3);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Extension {
                msg: racer_collection::ExtensionMsg::SetPaused { paused: true },
            },
            &[],
        )
        .unwrap();
    let before = s.balance(3, "buyer");
    assert!(s.buy(1, 3, 1000).is_err());
    assert_eq!(s.balance(3, "buyer"), before);
    assert_eq!(s.balance(3, "treasury"), Uint128::zero());
    let listing: racer_marketplace::Listing = s
        .app
        .wrap()
        .query_wasm_smart(&s.market, &racer_marketplace::QueryMsg::Listing { id: 1 })
        .unwrap();
    assert_eq!(listing.status, racer_marketplace::Status::Active);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Extension {
                msg: racer_collection::ExtensionMsg::SetPaused { paused: false },
            },
            &[],
        )
        .unwrap();
    s.buy(1, 3, 1000).unwrap();
}

#[test]
fn the_same_controller_stops_fleet_reward_claims_and_race_advancement() {
    let mut s = Suite::new();
    let code = s.app.store_code(Box::new(ContractWrapper::new(
        racer_fleet::execute,
        racer_fleet::instantiate,
        racer_fleet::query,
    )));
    let fleet = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_fleet::InstantiateMsg {
                owner: "owner".into(),
                engine: "owner".into(),
                spaceusd: s.tokens[3].to_string(),
                treasury: "treasury".into(),
                price_per_tier: Uint128::new(1_000_000),
            },
            &[],
            "fleet",
            None,
        )
        .unwrap();
    let code = s.app.store_code(Box::new(ContractWrapper::new(
        racer_rewards::execute,
        racer_rewards::instantiate,
        racer_rewards::query,
    )));
    let rewards = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_rewards::InstantiateMsg {
                owner: "owner".into(),
                allocator: "owner".into(),
                spaceusd: s.tokens[3].to_string(),
                srt: s.tokens[4].to_string(),
                annual_bps: 2500,
            },
            &[],
            "rewards",
            None,
        )
        .unwrap();
    let parameters = racer_race::Parameters {
        tick_seconds: 6,
        races_per_cycle: 100,
        duration_cdf: vec![1_000_000_000],
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
    };
    let code = s.app.store_code(Box::new(ContractWrapper::new(
        racer_race::execute,
        racer_race::instantiate,
        racer_race::query,
    )));
    let race = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_race::InstantiateMsg {
                owner: "owner".into(),
                fleet: fleet.to_string(),
                equipment: s.equipment.to_string(),
                rewards: rewards.to_string(),
                randomness: s.rng.to_string(),
                srt: s.tokens[4].to_string(),
                treasury: "treasury".into(),
                parameters,
                resume_from: None,
            },
            &[],
            "race",
            None,
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.rng.clone(),
            &racer_randomness::ExecuteMsg::Configure {
                paused: None,
                consumer: Some(race.to_string()),
                allowed: Some(true),
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.equipment.clone(),
            &racer_equipment::ExecuteMsg::Configure {
                engine: Some(race.to_string()),
                paused: None,
                collection: None,
                allowed: None,
            },
            &[],
        )
        .unwrap();
    for (address, msg) in [
        (
            fleet.clone(),
            to_json_binary(&racer_fleet::ExecuteMsg::SetPauseController {
                controller: Some(s.pause.to_string()),
            })
            .unwrap(),
        ),
        (
            rewards.clone(),
            to_json_binary(&racer_rewards::ExecuteMsg::SetPauseController {
                controller: Some(s.pause.to_string()),
            })
            .unwrap(),
        ),
        (
            race.clone(),
            to_json_binary(&racer_race::ExecuteMsg::SetPauseController {
                controller: Some(s.pause.to_string()),
            })
            .unwrap(),
        ),
    ] {
        s.app
            .execute(
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
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.tokens[4].clone(),
            &Cw20ExecuteMsg::Send {
                contract: rewards.to_string(),
                amount: Uint128::new(100),
                msg: to_json_binary(&racer_rewards::ReceiveMsg::Fund {}).unwrap(),
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            rewards.clone(),
            &racer_rewards::ExecuteMsg::Allocate {
                recipients: vec![racer_rewards::Allocation {
                    wallet: "player".into(),
                    spaceusd: Uint128::zero(),
                    srt: Uint128::new(100),
                }],
            },
            &[],
        )
        .unwrap();
    s.set_paused(true);
    let buy = Cw20ExecuteMsg::Send {
        contract: fleet.to_string(),
        amount: Uint128::new(1_000_000),
        msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
            ship: racer_fleet::ShipType::Racer,
            target_tier: 1,
            sponsor: None,
        })
        .unwrap(),
    };
    let before = s.balance(3, "player");
    assert!(s
        .app
        .execute_contract(Addr::unchecked("player"), s.tokens[3].clone(), &buy, &[])
        .is_err());
    assert_eq!(s.balance(3, "player"), before);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            rewards.clone(),
            &racer_rewards::ExecuteMsg::Claim {},
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            rewards.clone(),
            &racer_rewards::ExecuteMsg::Checkpoint {},
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("keeper"),
            race.clone(),
            &racer_race::ExecuteMsg::Advance { limit: 20 },
            &[]
        )
        .is_err());
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            race.clone(),
            &racer_race::ExecuteMsg::SetPauseController { controller: None },
            &[]
        )
        .is_err());
    s.set_paused(false);
    s.app
        .execute_contract(Addr::unchecked("player"), s.tokens[3].clone(), &buy, &[])
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            rewards,
            &racer_rewards::ExecuteMsg::Claim {},
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("keeper"),
            race,
            &racer_race::ExecuteMsg::Advance { limit: 20 },
            &[],
        )
        .unwrap();
}
