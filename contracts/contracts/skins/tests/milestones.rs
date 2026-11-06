use cosmwasm_std::{to_json_binary, Addr, Binary, Empty, Timestamp, Uint128};
use cw20::{Cw20Coin, Cw20ExecuteMsg};
use cw721::{Cw721QueryMsg, NftInfoResponse, OwnerOfResponse};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use racer_collection::{Attributes, Bonus, ExtensionMsg, ExtensionQuery, Kind, Rarity, SkinCounts};
use racer_fleet::ShipType;
use racer_skins::{Claim, Entry, ExecuteMsg, QueryMsg, Readiness, Status};

#[cosmwasm_schema::cw_serde]
enum HoldingMsg {
    ReceiveNft(cw721::Cw721ReceiveMsg),
    Return {
        collection: String,
        token_id: String,
        recipient: String,
    },
}
fn holding_init(
    _deps: cosmwasm_std::DepsMut,
    _env: cosmwasm_std::Env,
    _info: cosmwasm_std::MessageInfo,
    _msg: Empty,
) -> cosmwasm_std::StdResult<cosmwasm_std::Response> {
    Ok(cosmwasm_std::Response::new())
}
fn holding_execute(
    _deps: cosmwasm_std::DepsMut,
    _env: cosmwasm_std::Env,
    info: cosmwasm_std::MessageInfo,
    msg: HoldingMsg,
) -> cosmwasm_std::StdResult<cosmwasm_std::Response> {
    match msg {
        HoldingMsg::ReceiveNft(_) => Ok(cosmwasm_std::Response::new()),
        HoldingMsg::Return {
            collection,
            token_id,
            recipient,
        } => {
            if info.sender != Addr::unchecked("owner") {
                return Err(cosmwasm_std::StdError::generic_err("unauthorized"));
            }
            Ok(
                cosmwasm_std::Response::new().add_message(cosmwasm_std::WasmMsg::Execute {
                    contract_addr: collection,
                    funds: vec![],
                    msg: to_json_binary(&cw721::Cw721ExecuteMsg::TransferNft {
                        token_id,
                        recipient,
                    })?,
                }),
            )
        }
    }
}
fn holding_query(
    _deps: cosmwasm_std::Deps,
    _env: cosmwasm_std::Env,
    _msg: Empty,
) -> cosmwasm_std::StdResult<Binary> {
    to_json_binary(&Empty {})
}
fn collection() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_collection::execute,
        racer_collection::instantiate,
        racer_collection::query,
    ))
}
fn skins() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_skins::execute,
        racer_skins::instantiate,
        racer_skins::query,
    ))
}
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
fn rng() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_randomness::execute,
        racer_randomness::instantiate,
        racer_randomness::query,
    ))
}
fn equipment() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_equipment::execute,
        racer_equipment::instantiate,
        racer_equipment::query,
    ))
}
fn market() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_marketplace::execute,
        racer_marketplace::instantiate,
        racer_marketplace::query,
    ))
}
fn pause() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_pause::execute,
        racer_pause::instantiate,
        racer_pause::query,
    ))
}
fn entry(kind: Kind, code: &str) -> Entry {
    Entry {
        attributes: Attributes {
            code: code.into(),
            kind,
            rarity: Rarity::Common,
            bonus: Bonus::Cosmetic,
            bonus_bps: 0,
        },
        weight: 1,
        token_uri: None,
    }
}
fn catalogue() -> Vec<Entry> {
    vec![
        entry(Kind::RacerSkin, "racer-a"),
        entry(Kind::RacerSkin, "racer-b"),
        entry(Kind::TankSkin, "racer-a"),
        entry(Kind::FighterSkin, "fighter-a"),
    ]
}
struct Suite {
    app: App,
    skins: Addr,
    nft: Addr,
    rng: Addr,
    vault: Addr,
    market: Addr,
    token: Addr,
    pause: Addr,
}
impl Suite {
    fn new() -> Self {
        let mut app = App::default();
        app.update_block(|b| b.time = Timestamp::from_seconds(racer_randomness::GENESIS + 357));
        let code = app.store_code(token());
        let token = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &cw20_base::msg::InstantiateMsg {
                    name: "SpaceUSD".into(),
                    symbol: "USD".into(),
                    decimals: 6,
                    initial_balances: ["player", "buyer"]
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
                "token",
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
                    spaceusd: token.to_string(),
                    treasury: "treasury".into(),
                    price_per_tier: Uint128::new(1_000_000),
                },
                &[],
                "fleet",
                None,
            )
            .unwrap();
        for (ship, tier) in [(ShipType::Racer, 1000), (ShipType::Tank, 100)] {
            app.execute_contract(
                Addr::unchecked("player"),
                token.clone(),
                &Cw20ExecuteMsg::Send {
                    contract: fleet.to_string(),
                    amount: Uint128::new(u128::from(tier) * 1_000_000),
                    msg: to_json_binary(&racer_fleet::ReceiveMsg::Buy {
                        ship,
                        target_tier: tier,
                        sponsor: None,
                    })
                    .unwrap(),
                },
                &[],
            )
            .unwrap();
        }
        let code = app.store_code(collection());
        let nft = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_collection::InstantiateMsg {
                    owner: "owner".into(),
                    minter: "owner".into(),
                    name: "Skins".into(),
                    symbol: "SKIN".into(),
                },
                &[],
                "skins collection",
                None,
            )
            .unwrap();
        for (id, index) in [
            ("seller-a", 0),
            ("seller-a-copy", 0),
            ("seller-b", 1),
            ("seller-tank", 2),
        ] {
            app.execute_contract(
                Addr::unchecked("owner"),
                nft.clone(),
                &racer_collection::ExecuteMsg::Mint {
                    token_id: id.into(),
                    owner: "seller".into(),
                    token_uri: None,
                    extension: catalogue()[index].attributes.clone(),
                },
                &[],
            )
            .unwrap();
        }
        let code = app.store_code(rng());
        let rng = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_randomness::InstantiateMsg {
                    owner: "owner".into(),
                    consumers: vec![],
                    safety_seconds: 6,
                },
                &[],
                "randomness",
                None,
            )
            .unwrap();
        let code = app.store_code(skins());
        let skins = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_skins::InstantiateMsg {
                    owner: "owner".into(),
                    fleet: fleet.to_string(),
                    collection: nft.to_string(),
                    randomness: rng.to_string(),
                    milestone_step: 100,
                    catalogue: catalogue(),
                },
                &[],
                "milestones",
                None,
            )
            .unwrap();
        let code = app.store_code(equipment());
        let vault = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_equipment::InstantiateMsg {
                    owner: "owner".into(),
                    engine: "owner".into(),
                    collections: vec![nft.to_string()],
                },
                &[],
                "vault",
                None,
            )
            .unwrap();
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
                    payment_tokens: vec![token.to_string()],
                },
                &[],
                "market",
                None,
            )
            .unwrap();
        for address in [&vault, &market] {
            app.execute_contract(
                Addr::unchecked("owner"),
                nft.clone(),
                &racer_collection::ExecuteMsg::Extension {
                    msg: ExtensionMsg::SetCustodian {
                        address: address.to_string(),
                        allowed: true,
                    },
                },
                &[],
            )
            .unwrap();
        }
        app.execute_contract(
            Addr::unchecked("owner"),
            nft.clone(),
            &racer_collection::ExecuteMsg::UpdateOwnership(cw_ownable::Action::TransferOwnership {
                new_owner: skins.to_string(),
                expiry: None,
            }),
            &[],
        )
        .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            skins.clone(),
            &ExecuteMsg::AcceptMinter {},
            &[],
        )
        .unwrap();
        app.execute_contract(
            Addr::unchecked("owner"),
            rng.clone(),
            &racer_randomness::ExecuteMsg::Configure {
                paused: None,
                consumer: Some(skins.to_string()),
                allowed: Some(true),
            },
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
        for (address, msg) in [
            (
                skins.clone(),
                to_json_binary(&ExecuteMsg::SetPauseController {
                    controller: Some(pause.to_string()),
                })
                .unwrap(),
            ),
            (
                nft.clone(),
                to_json_binary(&racer_collection::ExecuteMsg::Extension {
                    msg: ExtensionMsg::SetPauseController {
                        controller: Some(pause.to_string()),
                    },
                })
                .unwrap(),
            ),
            (
                vault.clone(),
                to_json_binary(&racer_equipment::ExecuteMsg::SetPauseController {
                    controller: Some(pause.to_string()),
                })
                .unwrap(),
            ),
            (
                market.clone(),
                to_json_binary(&racer_marketplace::ExecuteMsg::SetPauseController {
                    controller: Some(pause.to_string()),
                })
                .unwrap(),
            ),
            (
                rng.clone(),
                to_json_binary(&racer_randomness::ExecuteMsg::SetPauseController {
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
            skins,
            nft,
            rng,
            vault,
            market,
            token,
            pause,
        }
    }
    fn request(&mut self, ship: ShipType) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked("player"),
                self.skins.clone(),
                &ExecuteMsg::Request { ship },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn fulfill(&mut self, id: u64) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked("keeper"),
                self.skins.clone(),
                &ExecuteMsg::Fulfill { id },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn claim(&self, id: u64) -> Claim {
        self.app
            .wrap()
            .query_wasm_smart(&self.skins, &QueryMsg::Claim { id })
            .unwrap()
    }
    fn preview(&self, id: u64) -> Readiness {
        self.app
            .wrap()
            .query_wasm_smart(&self.skins, &QueryMsg::Preview { id })
            .unwrap()
    }
    fn status(&self, ship: ShipType) -> Status {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.skins,
                &QueryMsg::Status {
                    wallet: "player".into(),
                    ship,
                },
            )
            .unwrap()
    }
    fn count(&self, wallet: &str, kind: Kind, code: &str) -> u32 {
        let response: SkinCounts = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.nft,
                &racer_collection::QueryMsg::Extension {
                    msg: ExtensionQuery::SkinCounts {
                        wallet: wallet.into(),
                        kind,
                        codes: vec![code.into()],
                    },
                },
            )
            .unwrap();
        response.counts[0].count
    }
    fn owner(&self, id: &str) -> String {
        let r: OwnerOfResponse = self
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
        r.owner
    }
    fn code(&self, id: &str) -> String {
        let r: NftInfoResponse<Attributes> = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.nft,
                &Cw721QueryMsg::NftInfo {
                    token_id: id.into(),
                },
            )
            .unwrap();
        r.extension.code
    }
    fn transfer(&mut self, from: &str, id: &str, to: &str) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked(from),
                self.nft.clone(),
                &racer_collection::ExecuteMsg::TransferNft {
                    token_id: id.into(),
                    recipient: to.into(),
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn prove(&mut self, id: u64) {
        let req: racer_randomness::Request = self
            .app
            .wrap()
            .query_wasm_smart(
                &self.rng,
                &racer_randomness::QueryMsg::Request {
                    consumer: self.skins.to_string(),
                    id,
                },
            )
            .unwrap();
        let signature = match req.round {
            123 => "b75c69d0b72a5d906e854e808ba7e2accb1542ac355ae486d591aa9d43765482e26cd02df835d3546d23c4b13e0dfc92",
            126 => "8fe536df624578f70778bcbc43133ffd65cdc9ee35d8e795fc534ccdabc758dd0b8a08864625f6381a3bcfced419d277",
            129 => "b7a3cece1065c2601636463c9307012327a333490344eb5b287b0d45667b22283d5b85237c1e3eefc5a36dc1e1b38aa0",
            132 => "902399434e9695087ff44106217528d5394f9a481ab098e50296280062623bb94dd1dc77658ef9bde271a7da43e60390",
            n => panic!("missing public proof fixture {n}"),
        };
        self.app
            .update_block(|b| b.time = Timestamp::from_seconds(req.available_at));
        let signature = signature
            .as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        self.app
            .execute_contract(
                Addr::unchecked("relayer"),
                self.rng.clone(),
                &racer_randomness::ExecuteMsg::Submit {
                    consumer: self.skins.to_string(),
                    id,
                    signature: Binary::from(signature),
                },
                &[],
            )
            .unwrap();
    }
    fn bind(&mut self, wallet: &str, id: &str, slot: racer_equipment::Slot) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked(wallet),
                self.nft.clone(),
                &racer_collection::ExecuteMsg::SendNft {
                    contract: self.vault.to_string(),
                    token_id: id.into(),
                    msg: to_json_binary(&racer_equipment::BindMsg { slot }).unwrap(),
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
    }
    fn boundary(&mut self, wallet: &str) {
        for msg in [
            racer_equipment::ExecuteMsg::BeginBoundary {},
            racer_equipment::ExecuteMsg::Process {
                wallets: vec![wallet.into()],
            },
            racer_equipment::ExecuteMsg::EndBoundary {},
        ] {
            self.app
                .execute_contract(Addr::unchecked("owner"), self.vault.clone(), &msg, &[])
                .unwrap();
        }
    }
}

#[test]
fn milestone_credits_are_reserved_per_ship_and_cannot_be_claimed_twice() {
    let mut s = Suite::new();
    assert_eq!(s.status(ShipType::Racer).earned, 10);
    assert_eq!(s.status(ShipType::Tank).earned, 1);
    assert!(s.request(ShipType::Fighter).is_err());
    s.request(ShipType::Racer).unwrap();
    assert!(s.request(ShipType::Racer).is_err());
    assert_eq!(s.preview(1), Readiness::WaitingForRandomness);
    assert!(s.fulfill(1).is_err());
    s.request(ShipType::Tank).unwrap();
    let pending: Vec<u64> = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.skins,
            &QueryMsg::Pending {
                start_after: None,
                limit: None,
            },
        )
        .unwrap();
    assert_eq!(pending, vec![1, 2]);
    s.prove(1);
    s.fulfill(1).unwrap();
    s.prove(2);
    s.fulfill(2).unwrap();
    assert!(s.fulfill(1).is_err());
    assert_eq!(s.status(ShipType::Racer).progress.fulfilled, 1);
    assert_eq!(s.status(ShipType::Tank).available, 0);
    assert!(s.request(ShipType::Tank).is_err());
    assert_eq!(s.owner(&s.claim(1).token_id.unwrap()), "player");
}

#[test]
fn bound_and_listed_skins_still_count_and_selling_restores_code_eligibility() {
    let mut s = Suite::new();
    s.request(ShipType::Racer).unwrap();
    s.prove(1);
    s.fulfill(1).unwrap();
    let first = s.claim(1).token_id.unwrap();
    s.bind("player", &first, racer_equipment::Slot::RacerSkin)
        .unwrap();
    s.boundary("player");
    let first_code = s.code(&first);
    assert_eq!(s.count("player", Kind::RacerSkin, &first_code), 1);
    assert_eq!(s.owner(&first), s.vault.to_string());
    s.request(ShipType::Racer).unwrap();
    s.prove(2);
    s.fulfill(2).unwrap();
    let second = s.claim(2).token_id.unwrap();
    let second_code = s.code(&second);
    assert_ne!(first_code, second_code);
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::SendNft {
                contract: s.market.to_string(),
                token_id: second.clone(),
                msg: to_json_binary(&racer_marketplace::ListMsg {
                    payment_token: s.token.to_string(),
                    price: Uint128::new(1000),
                })
                .unwrap(),
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.count("player", Kind::RacerSkin, &second_code), 1);
    assert!(s.status(ShipType::Racer).eligible_codes.is_empty());
    assert!(s.request(ShipType::Racer).is_err());
    assert_eq!(s.status(ShipType::Racer).available, 8);
    s.app
        .execute_contract(
            Addr::unchecked("buyer"),
            s.token.clone(),
            &Cw20ExecuteMsg::Send {
                contract: s.market.to_string(),
                amount: Uint128::new(1000),
                msg: to_json_binary(&racer_marketplace::BuyMsg { listing_id: 1 }).unwrap(),
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.count("player", Kind::RacerSkin, &second_code), 0);
    assert_eq!(s.count("buyer", Kind::RacerSkin, &second_code), 1);
    assert_eq!(
        s.status(ShipType::Racer).eligible_codes,
        vec![second_code.clone()]
    );
    s.request(ShipType::Racer).unwrap();
    s.prove(3);
    s.fulfill(3).unwrap();
    assert_eq!(s.code(&s.claim(3).token_id.unwrap()), second_code);
    assert_eq!(s.status(ShipType::Racer).available, 7);
}

#[test]
fn replacement_unbinding_and_rejected_callbacks_preserve_the_ownership_index() {
    let mut s = Suite::new();
    s.request(ShipType::Racer).unwrap();
    s.prove(1);
    s.fulfill(1).unwrap();
    let a = s.claim(1).token_id.unwrap();
    assert!(s
        .bind("player", &a, racer_equipment::Slot::TankSkin)
        .is_err());
    assert_eq!(s.owner(&a), "player");
    let vault = s.vault.clone();
    assert!(s.transfer("player", &a, vault.as_str()).is_err());
    s.bind("player", &a, racer_equipment::Slot::RacerSkin)
        .unwrap();
    s.boundary("player");
    s.request(ShipType::Racer).unwrap();
    s.prove(2);
    s.fulfill(2).unwrap();
    let b = s.claim(2).token_id.unwrap();
    s.bind("player", &b, racer_equipment::Slot::RacerSkin)
        .unwrap();
    s.boundary("player");
    assert_eq!(s.owner(&a), "player");
    assert_eq!(s.owner(&b), s.vault.to_string());
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.vault.clone(),
            &racer_equipment::ExecuteMsg::Unbind {
                slot: racer_equipment::Slot::RacerSkin,
            },
            &[],
        )
        .unwrap();
    s.boundary("player");
    assert_eq!(s.owner(&b), "player");
    for id in [a, b] {
        assert_eq!(s.count("player", Kind::RacerSkin, &s.code(&id)), 1);
    }
}

#[test]
fn duplicate_acquired_after_commit_blocks_the_same_draw_without_rerolling() {
    let mut s = Suite::new();
    s.request(ShipType::Racer).unwrap();
    let mut changed = catalogue();
    changed[0].attributes.code = "replacement-a".into();
    changed[1].attributes.code = "replacement-b".into();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.skins.clone(),
            &ExecuteMsg::Configure {
                milestone_step: None,
                catalogue: Some(changed),
            },
            &[],
        )
        .unwrap();
    s.prove(1);
    let Readiness::Ready { code } = s.preview(1) else {
        panic!("not ready")
    };
    assert!(!code.starts_with("replacement"));
    let incoming = if code == "racer-a" {
        "seller-a"
    } else {
        "seller-b"
    };
    s.transfer("seller", incoming, "player").unwrap();
    assert_eq!(
        s.preview(1),
        Readiness::DuplicateOwned { code: code.clone() }
    );
    assert!(s.fulfill(1).is_err());
    assert!(s.request(ShipType::Racer).is_err());
    assert_eq!(s.status(ShipType::Racer).progress.pending, Some(1));
    s.transfer("player", incoming, "buyer").unwrap();
    s.fulfill(1).unwrap();
    assert_eq!(s.code(&s.claim(1).token_id.unwrap()), code);
    assert_eq!(s.status(ShipType::Racer).progress.fulfilled, 1);
}

#[test]
fn counters_handle_duplicates_class_separation_burns_and_approved_operators() {
    let mut s = Suite::new();
    assert_eq!(s.count("seller", Kind::RacerSkin, "racer-a"), 2);
    assert_eq!(s.count("seller", Kind::TankSkin, "racer-a"), 1);
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Burn {
                token_id: "seller-a".into()
            },
            &[]
        )
        .is_err());
    s.app
        .execute_contract(
            Addr::unchecked("seller"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Burn {
                token_id: "seller-a-copy".into(),
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.count("seller", Kind::RacerSkin, "racer-a"), 1);
    s.app
        .execute_contract(
            Addr::unchecked("seller"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::ApproveAll {
                operator: "operator".into(),
                expires: None,
            },
            &[],
        )
        .unwrap();
    s.bind("operator", "seller-a", racer_equipment::Slot::RacerSkin)
        .unwrap();
    assert_eq!(s.count("seller", Kind::RacerSkin, "racer-a"), 0);
    assert_eq!(s.count("operator", Kind::RacerSkin, "racer-a"), 1);
    let beneficiary: racer_collection::Beneficiary = s
        .app
        .wrap()
        .query_wasm_smart(
            &s.nft,
            &racer_collection::QueryMsg::Extension {
                msg: ExtensionQuery::Beneficiary {
                    token_id: "seller-a".into(),
                },
            },
        )
        .unwrap();
    assert_eq!(beneficiary.beneficiary, Addr::unchecked("operator"));
    assert_eq!(s.count("seller", Kind::TankSkin, "racer-a"), 1);
}

#[test]
fn pause_and_failed_mint_roll_back_progress_and_allow_the_original_claim_to_resume() {
    let mut s = Suite::new();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Extension {
                msg: ExtensionMsg::SetCustodian {
                    address: "fake".into(),
                    allowed: true
                }
            },
            &[]
        )
        .is_err());
    s.request(ShipType::Racer).unwrap();
    s.prove(1);
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Extension {
                msg: ExtensionMsg::SetPaused { paused: true },
            },
            &[],
        )
        .unwrap();
    assert!(s.fulfill(1).is_err());
    assert!(s.claim(1).token_id.is_none());
    assert_eq!(s.status(ShipType::Racer).progress.pending, Some(1));
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Extension {
                msg: ExtensionMsg::SetPaused { paused: false },
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.pause.clone(),
            &racer_pause::ExecuteMsg::SetPaused { paused: true },
            &[],
        )
        .unwrap();
    assert!(s.fulfill(1).is_err());
    assert!(s.request(ShipType::Tank).is_err());
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.pause.clone(),
            &racer_pause::ExecuteMsg::SetPaused { paused: false },
            &[],
        )
        .unwrap();
    s.fulfill(1).unwrap();
    assert_eq!(s.status(ShipType::Racer).progress.fulfilled, 1);
}

#[test]
fn disabled_custody_blocks_new_deposits_but_preserves_existing_ownership_and_exits() {
    let mut s = Suite::new();
    s.request(ShipType::Racer).unwrap();
    s.prove(1);
    s.fulfill(1).unwrap();
    let id = s.claim(1).token_id.unwrap();
    let code = s.code(&id);
    s.bind("player", &id, racer_equipment::Slot::RacerSkin)
        .unwrap();
    s.boundary("player");
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            s.nft.clone(),
            &racer_collection::ExecuteMsg::Extension {
                msg: ExtensionMsg::SetCustodian {
                    address: s.vault.to_string(),
                    allowed: false,
                },
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.count("player", Kind::RacerSkin, &code), 1);
    assert!(s
        .bind("seller", "seller-b", racer_equipment::Slot::RacerSkin)
        .unwrap_err()
        .contains("skin custodian disabled"));
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.vault.clone(),
            &racer_equipment::ExecuteMsg::Unbind {
                slot: racer_equipment::Slot::RacerSkin,
            },
            &[],
        )
        .unwrap();
    s.boundary("player");
    assert_eq!(s.owner(&id), "player");
    assert_eq!(s.count("player", Kind::RacerSkin, &code), 1);
}

#[test]
fn a_custodian_cannot_be_registered_after_it_already_received_untracked_nfts() {
    let mut s = Suite::new();
    let code = s.app.store_code(Box::new(ContractWrapper::new(
        holding_execute,
        holding_init,
        holding_query,
    )));
    let late = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &Empty {},
            &[],
            "late custody",
            None,
        )
        .unwrap();
    let send = racer_collection::ExecuteMsg::SendNft {
        contract: late.to_string(),
        token_id: "seller-a".into(),
        msg: to_json_binary(&Empty {}).unwrap(),
    };
    s.app
        .execute_contract(Addr::unchecked("seller"), s.nft.clone(), &send, &[])
        .unwrap();
    let register = racer_collection::ExecuteMsg::Extension {
        msg: ExtensionMsg::SetCustodian {
            address: late.to_string(),
            allowed: true,
        },
    };
    let error = s
        .app
        .execute_contract(Addr::unchecked("owner"), s.nft.clone(), &register, &[])
        .unwrap_err();
    assert!(format!("{error:#}").contains("register custody before accepting NFTs"));
    s.app
        .execute_contract(
            Addr::unchecked("owner"),
            late.clone(),
            &HoldingMsg::Return {
                collection: s.nft.to_string(),
                token_id: "seller-a".into(),
                recipient: "seller".into(),
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(Addr::unchecked("owner"), s.nft.clone(), &register, &[])
        .unwrap();
    s.app
        .execute_contract(Addr::unchecked("seller"), s.nft.clone(), &send, &[])
        .unwrap();
    assert_eq!(s.count("seller", Kind::RacerSkin, "racer-a"), 2);
    assert_eq!(s.count(late.as_str(), Kind::RacerSkin, "racer-a"), 0);
}

#[test]
fn misconfigured_game_custody_rejects_skins_instead_of_losing_wallet_attribution() {
    let mut s = Suite::new();
    let code = s.app.store_code(equipment());
    let unregistered_vault = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_equipment::InstantiateMsg {
                owner: "owner".into(),
                engine: "owner".into(),
                collections: vec![s.nft.to_string()],
            },
            &[],
            "unregistered vault",
            None,
        )
        .unwrap();
    let code = s.app.store_code(market());
    let unregistered_market = s
        .app
        .instantiate_contract(
            code,
            Addr::unchecked("owner"),
            &racer_marketplace::InstantiateMsg {
                owner: "owner".into(),
                treasury: "treasury".into(),
                fee_bps: 500,
                collections: vec![s.nft.to_string()],
                payment_tokens: vec![s.token.to_string()],
            },
            &[],
            "unregistered market",
            None,
        )
        .unwrap();
    for (contract, msg) in [
        (
            unregistered_vault,
            to_json_binary(&racer_equipment::BindMsg {
                slot: racer_equipment::Slot::RacerSkin,
            })
            .unwrap(),
        ),
        (
            unregistered_market,
            to_json_binary(&racer_marketplace::ListMsg {
                payment_token: s.token.to_string(),
                price: Uint128::new(1000),
            })
            .unwrap(),
        ),
    ] {
        let error = s
            .app
            .execute_contract(
                Addr::unchecked("seller"),
                s.nft.clone(),
                &racer_collection::ExecuteMsg::SendNft {
                    contract: contract.to_string(),
                    token_id: "seller-a".into(),
                    msg,
                },
                &[],
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("skin custody not registered"));
        assert_eq!(s.owner("seller-a"), "seller");
        assert_eq!(s.count("seller", Kind::RacerSkin, "racer-a"), 2);
    }
}
