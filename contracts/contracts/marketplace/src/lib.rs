use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdError, StdResult, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw721::{Cw721ExecuteMsg, Cw721QueryMsg, Cw721ReceiveMsg, NftInfoResponse, OwnerOfResponse};
use cw_storage_plus::{Bound, Item, Map};

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub treasury: String,
    pub fee_bps: u16,
    pub collections: Vec<String>,
    pub payment_tokens: Vec<String>,
}
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub treasury: Addr,
    pub fee_bps: u16,
    pub next_id: u64,
}
#[cw_serde]
pub struct ListMsg {
    pub payment_token: String,
    pub price: Uint128,
}
#[cw_serde]
pub struct BuyMsg {
    pub listing_id: u64,
}
#[cw_serde]
pub enum ExecuteMsg {
    ReceiveNft(Cw721ReceiveMsg),
    Receive(Cw20ReceiveMsg),
    Cancel {
        listing_id: u64,
    },
    Configure {
        treasury: Option<String>,
        fee_bps: Option<u16>,
    },
    AllowCollection {
        collection: String,
        allowed: bool,
    },
    AllowPayment {
        token: String,
        allowed: bool,
    },
    SetPauseController {
        controller: Option<String>,
    },
}
#[cw_serde]
pub enum Status {
    Active,
    Sold { buyer: Addr },
    Cancelled,
}
#[cw_serde]
pub struct Listing {
    pub id: u64,
    pub seller: Addr,
    pub collection: Addr,
    pub token_id: String,
    pub payment_token: Addr,
    pub price: Uint128,
    pub fee_bps: u16,
    pub treasury: Addr,
    pub status: Status,
}
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},
    #[returns(Listing)]
    Listing { id: u64 },
    #[returns(Vec<Listing>)]
    Active {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    #[returns(Vec<Listing>)]
    Seller {
        wallet: String,
        start_after: Option<u64>,
        limit: Option<u32>,
    },
}
const CONFIG: Item<Config> = Item::new("config");
const COLLECTIONS: Map<&Addr, bool> = Map::new("collections");
const TOKENS: Map<&Addr, bool> = Map::new("tokens");
const LISTINGS: Map<u64, Listing> = Map::new("listings");
const ACTIVE: Map<u64, bool> = Map::new("active");
const SELLERS: Map<(&Addr, u64), bool> = Map::new("sellers");
const ESCROW: Map<(&Addr, &str), u64> = Map::new("escrow");
fn fail(msg: &str) -> StdError {
    StdError::generic_err(msg)
}
fn call(address: &Addr, msg: Binary) -> WasmMsg {
    WasmMsg::Execute {
        contract_addr: address.to_string(),
        msg,
        funds: vec![],
    }
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if !info.funds.is_empty()
        || msg.fee_bps > 10000
        || msg.collections.len() > 20
        || msg.payment_tokens.is_empty()
        || msg.payment_tokens.len() > 5
    {
        return Err(fail("invalid marketplace initialization"));
    }
    CONFIG.save(
        deps.storage,
        &Config {
            owner: deps.api.addr_validate(&msg.owner)?,
            treasury: deps.api.addr_validate(&msg.treasury)?,
            fee_bps: msg.fee_bps,
            next_id: 1,
        },
    )?;
    for addr in msg.collections {
        COLLECTIONS.save(deps.storage, &deps.api.addr_validate(&addr)?, &true)?;
    }
    for addr in msg.payment_tokens {
        TOKENS.save(deps.storage, &deps.api.addr_validate(&addr)?, &true)?;
    }
    cw2::set_contract_version(
        deps.storage,
        "spaceship-racer/marketplace",
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Response::new().add_attribute("action", "instantiate_marketplace"))
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    if !info.funds.is_empty() {
        return Err(fail("native funds unsupported"));
    }
    let mut config = CONFIG.load(deps.storage)?;
    match msg {
        ExecuteMsg::SetPauseController { controller } => {
            return game_common::pause::set_controller(deps, &info, &config.owner, controller)
        }
        ExecuteMsg::Configure { treasury, fee_bps } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            if let Some(addr) = treasury {
                config.treasury = deps.api.addr_validate(&addr)?;
            }
            if let Some(bps) = fee_bps {
                if bps > 10000 {
                    return Err(fail("invalid fee"));
                }
                config.fee_bps = bps;
            }
            CONFIG.save(deps.storage, &config)?;
            return Ok(Response::new().add_attribute("action", "configure_marketplace"));
        }
        ExecuteMsg::AllowCollection {
            collection,
            allowed,
        } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            COLLECTIONS.save(
                deps.storage,
                &deps.api.addr_validate(&collection)?,
                &allowed,
            )?;
            return Ok(Response::new().add_attribute("action", "allow_marketplace_collection"));
        }
        ExecuteMsg::AllowPayment { token, allowed } => {
            if info.sender != config.owner {
                return Err(fail("unauthorized"));
            }
            TOKENS.save(deps.storage, &deps.api.addr_validate(&token)?, &allowed)?;
            return Ok(Response::new().add_attribute("action", "allow_marketplace_payment"));
        }
        _ => {}
    }
    game_common::pause::ensure_running(deps.as_ref())?;
    match msg {
        ExecuteMsg::ReceiveNft(receive) => {
            if !COLLECTIONS
                .may_load(deps.storage, &info.sender)?
                .unwrap_or(false)
            {
                return Err(fail("unsupported collection"));
            }
            let hook: ListMsg = from_json(receive.msg)?;
            let payment_token = deps.api.addr_validate(&hook.payment_token)?;
            if hook.price.is_zero()
                || !TOKENS
                    .may_load(deps.storage, &payment_token)?
                    .unwrap_or(false)
            {
                return Err(fail("invalid listing payment"));
            }
            if ESCROW.has(deps.storage, (&info.sender, &receive.token_id)) {
                return Err(fail("already listed"));
            }
            let owner: OwnerOfResponse = deps.querier.query_wasm_smart(
                &info.sender,
                &Cw721QueryMsg::OwnerOf {
                    token_id: receive.token_id.clone(),
                    include_expired: None,
                },
            )?;
            if owner.owner != env.contract.address {
                return Err(fail("NFT not escrowed"));
            }
            let nft: NftInfoResponse<racer_collection::Attributes> =
                deps.querier.query_wasm_smart(
                    &info.sender,
                    &Cw721QueryMsg::NftInfo {
                        token_id: receive.token_id.clone(),
                    },
                )?;
            if nft.extension.kind != racer_collection::Kind::Commander {
                let custody: racer_collection::Custodian = deps.querier.query_wasm_smart(
                    &info.sender,
                    &racer_collection::QueryMsg::Extension {
                        msg: racer_collection::ExtensionQuery::Custodian {
                            address: env.contract.address.to_string(),
                        },
                    },
                )?;
                if !custody.allowed {
                    return Err(fail("skin custody not registered"));
                }
            }
            let seller = deps.api.addr_validate(&receive.sender)?;
            let id = config.next_id;
            config.next_id = id
                .checked_add(1)
                .ok_or_else(|| fail("listing ID overflow"))?;
            let listing = Listing {
                id,
                seller: seller.clone(),
                collection: info.sender.clone(),
                token_id: receive.token_id.clone(),
                payment_token,
                price: hook.price,
                fee_bps: config.fee_bps,
                treasury: config.treasury.clone(),
                status: Status::Active,
            };
            ESCROW.save(deps.storage, (&info.sender, &receive.token_id), &id)?;
            LISTINGS.save(deps.storage, id, &listing)?;
            ACTIVE.save(deps.storage, id, &true)?;
            SELLERS.save(deps.storage, (&seller, id), &true)?;
            CONFIG.save(deps.storage, &config)?;
            Ok(Response::new()
                .add_attribute("action", "list_nft")
                .add_attribute("listing_id", id.to_string())
                .add_attribute("seller", seller))
        }
        ExecuteMsg::Receive(receive) => {
            let hook: BuyMsg = from_json(receive.msg)?;
            let mut listing = LISTINGS.load(deps.storage, hook.listing_id)?;
            if listing.status != Status::Active
                || info.sender != listing.payment_token
                || !TOKENS
                    .may_load(deps.storage, &info.sender)?
                    .unwrap_or(false)
            {
                return Err(fail("inactive listing or wrong payment token"));
            }
            if receive.amount != listing.price {
                return Err(fail("incorrect listing payment"));
            }
            let buyer = deps.api.addr_validate(&receive.sender)?;
            if buyer == listing.seller {
                return Err(fail("cannot buy own listing"));
            }
            let fee = listing
                .price
                .multiply_ratio(u128::from(listing.fee_bps), 10000u128);
            let net = listing.price.checked_sub(fee)?;
            listing.status = Status::Sold {
                buyer: buyer.clone(),
            };
            close(deps.storage, &listing)?;
            let mut response = Response::new().add_message(call(
                &listing.collection,
                to_json_binary(&Cw721ExecuteMsg::TransferNft {
                    recipient: buyer.to_string(),
                    token_id: listing.token_id,
                })?,
            ));
            for (recipient, amount) in [(listing.seller, net), (listing.treasury, fee)] {
                if !amount.is_zero() {
                    response = response.add_message(call(
                        &listing.payment_token,
                        to_json_binary(&Cw20ExecuteMsg::Transfer {
                            recipient: recipient.to_string(),
                            amount,
                        })?,
                    ));
                }
            }
            Ok(response
                .add_attribute("action", "buy_nft")
                .add_attribute("listing_id", listing.id.to_string())
                .add_attribute("buyer", buyer)
                .add_attribute("fee", fee))
        }
        ExecuteMsg::Cancel { listing_id } => {
            let mut listing = LISTINGS.load(deps.storage, listing_id)?;
            if listing.seller != info.sender || listing.status != Status::Active {
                return Err(fail("not seller or listing closed"));
            }
            listing.status = Status::Cancelled;
            close(deps.storage, &listing)?;
            Ok(Response::new()
                .add_message(call(
                    &listing.collection,
                    to_json_binary(&Cw721ExecuteMsg::TransferNft {
                        recipient: listing.seller.to_string(),
                        token_id: listing.token_id,
                    })?,
                ))
                .add_attribute("action", "cancel_listing")
                .add_attribute("listing_id", listing_id.to_string()))
        }
        _ => unreachable!(),
    }
}

fn close(storage: &mut dyn cosmwasm_std::Storage, listing: &Listing) -> StdResult<()> {
    LISTINGS.save(storage, listing.id, listing)?;
    ACTIVE.remove(storage, listing.id);
    SELLERS.remove(storage, (&listing.seller, listing.id));
    ESCROW.remove(storage, (&listing.collection, &listing.token_id));
    Ok(())
}

#[cfg_attr(not(feature = "library"), cosmwasm_std::entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Listing { id } => to_json_binary(&LISTINGS.load(deps.storage, id)?),
        QueryMsg::Active { start_after, limit } => {
            let rows = ACTIVE
                .keys(
                    deps.storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(30).clamp(1, 100) as usize)
                .map(|id| LISTINGS.load(deps.storage, id?))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&rows)
        }
        QueryMsg::Seller {
            wallet,
            start_after,
            limit,
        } => {
            let wallet = deps.api.addr_validate(&wallet)?;
            let rows = SELLERS
                .prefix(&wallet)
                .keys(
                    deps.storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(30).clamp(1, 100) as usize)
                .map(|id| LISTINGS.load(deps.storage, id?))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&rows)
        }
    }
}
