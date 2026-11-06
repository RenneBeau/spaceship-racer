use cosmwasm_std::{to_json_binary, Addr, Empty};
use cw721::{Cw721QueryMsg, OwnerOfResponse};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use racer_collection::{Attributes, Bonus, Kind, Rarity};
use racer_equipment::{BindMsg, ExecuteMsg, InstantiateMsg, Position, QueryMsg, Slot};

fn equipment() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_equipment::execute,
        racer_equipment::instantiate,
        racer_equipment::query,
    ))
}
fn collection() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        racer_collection::execute,
        racer_collection::instantiate,
        racer_collection::query,
    ))
}
struct Suite {
    app: App,
    nft: Addr,
    vault: Addr,
}
impl Suite {
    fn new() -> Self {
        let mut app = App::default();
        let code = app.store_code(collection());
        let nft = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &racer_collection::InstantiateMsg {
                    name: "Commanders".into(),
                    symbol: "CMD".into(),
                    minter: "minter".into(),
                    owner: "owner".into(),
                },
                &[],
                "collection",
                None,
            )
            .unwrap();
        let code = app.store_code(equipment());
        let vault = app
            .instantiate_contract(
                code,
                Addr::unchecked("owner"),
                &InstantiateMsg {
                    owner: "owner".into(),
                    engine: "engine".into(),
                    collections: vec![nft.to_string()],
                },
                &[],
                "vault",
                None,
            )
            .unwrap();
        for id in ["one", "two", "three"] {
            app.execute_contract(
                Addr::unchecked("minter"),
                nft.clone(),
                &racer_collection::ExecuteMsg::Mint {
                    token_id: id.into(),
                    owner: "player".into(),
                    token_uri: None,
                    extension: Attributes {
                        code: "nova".into(),
                        kind: Kind::Commander,
                        rarity: Rarity::Common,
                        bonus: Bonus::Speed,
                        bonus_bps: 300,
                    },
                },
                &[],
            )
            .unwrap();
        }
        Self { app, nft, vault }
    }
    fn bind(&mut self, id: &str) -> Result<(), String> {
        self.app
            .execute_contract(
                Addr::unchecked("player"),
                self.nft.clone(),
                &racer_collection::ExecuteMsg::SendNft {
                    contract: self.vault.to_string(),
                    token_id: id.into(),
                    msg: to_json_binary(&BindMsg {
                        slot: Slot::Commander,
                    })
                    .unwrap(),
                },
                &[],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
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
    fn position(&self) -> Position {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.vault,
                &QueryMsg::Position {
                    wallet: "player".into(),
                    slot: Slot::Commander,
                },
            )
            .unwrap()
    }
    fn process(&mut self) {
        self.app
            .execute_contract(
                Addr::unchecked("engine"),
                self.vault.clone(),
                &ExecuteMsg::BeginBoundary {},
                &[],
            )
            .unwrap();
        self.app
            .execute_contract(
                Addr::unchecked("engine"),
                self.vault.clone(),
                &ExecuteMsg::Process {
                    wallets: vec!["player".into()],
                },
                &[],
            )
            .unwrap();
        self.app
            .execute_contract(
                Addr::unchecked("engine"),
                self.vault.clone(),
                &ExecuteMsg::EndBoundary {},
                &[],
            )
            .unwrap();
    }
}

#[test]
fn replacement_and_unbinding_return_custody_only_at_boundary() {
    let mut s = Suite::new();
    s.bind("one").unwrap();
    assert_eq!(s.owner("one"), s.vault.to_string());
    assert!(s.position().active.is_none());
    assert!(s.bind("two").is_err());
    assert_eq!(s.owner("two"), "player");
    s.process();
    assert_eq!(s.position().active.unwrap().token_id, "one");
    s.bind("two").unwrap();
    assert_eq!(s.position().active.unwrap().token_id, "one");
    assert_eq!(s.owner("one"), s.vault.to_string());
    s.process();
    assert_eq!(s.owner("one"), "player");
    assert_eq!(s.position().active.unwrap().token_id, "two");
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.vault.clone(),
            &ExecuteMsg::Unbind {
                slot: Slot::Commander,
            },
            &[],
        )
        .unwrap();
    assert_eq!(s.owner("two"), s.vault.to_string());
    s.process();
    assert_eq!(s.owner("two"), "player");
    assert!(s.position().active.is_none());
}

#[test]
fn unauthorized_boundary_forged_callback_and_paused_transfers_rejected() {
    let mut s = Suite::new();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.vault.clone(),
            &ExecuteMsg::ReceiveNft(cw721::Cw721ReceiveMsg {
                sender: "player".into(),
                token_id: "one".into(),
                msg: to_json_binary(&BindMsg {
                    slot: Slot::Commander
                })
                .unwrap()
            }),
            &[]
        )
        .is_err());
    s.bind("one").unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("attacker"),
            s.vault.clone(),
            &ExecuteMsg::Process {
                wallets: vec!["player".into()]
            },
            &[]
        )
        .is_err());
    s.process();
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
    assert!(s.bind("two").is_err());
    assert_eq!(s.owner("two"), "player");
    s.app
        .execute_contract(
            Addr::unchecked("player"),
            s.vault.clone(),
            &ExecuteMsg::Unbind {
                slot: Slot::Commander,
            },
            &[],
        )
        .unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("engine"),
            s.vault.clone(),
            &ExecuteMsg::BeginBoundary {},
            &[],
        )
        .unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("engine"),
            s.vault.clone(),
            &ExecuteMsg::Process {
                wallets: vec!["player".into()]
            },
            &[]
        )
        .is_err());
    assert!(s.position().pending.is_some());
    assert_eq!(s.owner("one"), s.vault.to_string());
}

#[test]
fn boundary_freezes_new_requests_until_every_queued_change_is_processed() {
    let mut s = Suite::new();
    s.bind("one").unwrap();
    s.app
        .execute_contract(
            Addr::unchecked("engine"),
            s.vault.clone(),
            &ExecuteMsg::BeginBoundary {},
            &[],
        )
        .unwrap();
    assert!(s.bind("two").is_err());
    assert_eq!(s.owner("two"), "player");
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("engine"),
            s.vault.clone(),
            &ExecuteMsg::EndBoundary {},
            &[]
        )
        .is_err());
    s.app
        .execute_contract(
            Addr::unchecked("engine"),
            s.vault.clone(),
            &ExecuteMsg::Process {
                wallets: vec!["player".into()],
            },
            &[],
        )
        .unwrap();
    assert!(s
        .app
        .execute_contract(
            Addr::unchecked("player"),
            s.vault.clone(),
            &ExecuteMsg::Unbind {
                slot: Slot::Commander
            },
            &[]
        )
        .is_err());
    s.app
        .execute_contract(
            Addr::unchecked("engine"),
            s.vault.clone(),
            &ExecuteMsg::EndBoundary {},
            &[],
        )
        .unwrap();
    s.bind("two").unwrap();
}
