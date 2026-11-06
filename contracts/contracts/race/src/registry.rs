//! Persistent full-health templates; only changed fleets rebuild base weights.
//! Immutable legacy fleets have no change journal, so launches retain a cheap
//! authoritative raw-fleet scan. Participation is checkpointed, not rewritten.
use crate::{
    math::{fail, make_player},
    motion,
    state::*,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{from_json, Addr, Deps, DepsMut, Order, StdResult, Storage, Uint128};
use cw_storage_plus::{Bound, Item, Map};
use game_common::weighted_tiers;
use racer_equipment::{Position, Slot};
use racer_fleet::Fleet;

#[cw_serde]
#[derive(Default)]
pub struct Meta {
    pub count: u16,
    pub generation: u64,
    pub tick_seconds: u16,
    pub total_tiers: Uint128,
}
#[cw_serde]
pub struct Template {
    pub player: Player,
    pub generation: u64,
}
#[cw_serde]
pub struct Header {
    pub index: u16,
    pub tiers: [u16; 3],
    pub generation: u64,
    pub cycle: u64,
}
#[cw_serde]
pub struct Participation {
    pub cycle: u64,
    pub through: u16,
    pub tiers: Uint128,
    pub weighted: Uint128,
}
pub const META: Item<Meta> = Item::new("registry_meta");
pub const TEMPLATES: Map<u16, Template> = Map::new("registry_templates");
pub const DIRTY: Map<&Addr, bool> = Map::new("registry_equipment_dirty");
const PARTICIPATION: Map<&Addr, Participation> = Map::new("registry_participation");
const HEADERS: Map<&Addr, Header> = Map::new("registry_headers");

pub fn load_player(storage: &dyn Storage, state: &State, index: u16) -> StdResult<Player> {
    if !state.registry_enabled {
        return PLAYERS.load(storage, index);
    }
    if index == 0 || index > state.participants {
        return Err(fail("player outside active registry"));
    }
    if let Some(p) = PLAYERS.may_load(storage, index)? {
        if p.race == state.race {
            return Ok(p);
        }
    }
    let mut p = TEMPLATES.load(storage, index)?.player;
    p.race = state.race;
    Ok(p)
}

pub fn begin(storage: &mut dyn Storage, state: &mut State, params: &Parameters) -> StdResult<()> {
    let mut meta = META.may_load(storage)?.unwrap_or_default();
    if meta.tick_seconds != params.tick_seconds {
        meta.tick_seconds = params.tick_seconds;
        meta.generation = meta
            .generation
            .checked_add(1)
            .ok_or_else(|| fail("registry generation overflow"))?;
        META.save(storage, &meta)?;
    }
    state.registry_enabled = true;
    state.registry_generation = meta.generation;
    state.registry_scanned = 0;
    state.participants = meta.count;
    Ok(())
}

fn totals(entry: &mut CyclePlayer, part: &Participation, through: u16) -> StdResult<()> {
    let n = u128::from(
        through
            .checked_sub(part.through)
            .ok_or_else(|| fail("participation checkpoint ahead"))?,
    );
    entry.tier_sum = entry
        .tier_sum
        .checked_add(part.tiers.checked_mul(Uint128::new(n))?)?;
    entry.weighted_sum = entry
        .weighted_sum
        .checked_add(part.weighted.checked_mul(Uint128::new(n))?)?;
    Ok(())
}
pub fn accrue(storage: &mut dyn Storage, state: &State, wallet: &Addr) -> StdResult<()> {
    let Some(mut part) = PARTICIPATION.may_load(storage, wallet)? else {
        return Ok(());
    };
    if part.cycle != state.cycle || part.through == state.completed {
        return Ok(());
    }
    let mut entry = CYCLE_PLAYERS.load(storage, (state.cycle, wallet))?;
    let before = entry.tier_sum;
    totals(&mut entry, &part, state.completed)?;
    if let Some(id) = entry.clan {
        CLAN_CYCLES.update(storage, (state.cycle, id), |old| -> StdResult<_> {
            let mut row = old.ok_or_else(|| fail("missing clan participation"))?;
            row.tiers = row.tiers.checked_add(entry.tier_sum.checked_sub(before)?)?;
            Ok(row)
        })?;
    }
    part.through = state.completed;
    CYCLE_PLAYERS.save(storage, (state.cycle, wallet), &entry)?;
    PARTICIPATION.save(storage, wallet, &part)?;
    Ok(())
}
pub fn cycle_player(
    deps: Deps,
    state: &State,
    cycle: u64,
    wallet: &Addr,
) -> StdResult<CyclePlayer> {
    let mut entry = CYCLE_PLAYERS
        .may_load(deps.storage, (cycle, wallet))?
        .unwrap_or_default();
    if state.registry_enabled && cycle == state.cycle {
        if let Some(part) = PARTICIPATION
            .may_load(deps.storage, wallet)?
            .filter(|p| p.cycle == cycle)
        {
            let mut through = state.completed;
            if matches!(
                state.phase,
                Phase::Waiting | Phase::Movement | Phase::Combat
            ) {
                through += 1;
            }
            if matches!(state.phase, Phase::Snapshot) {
                if let Some((_, index)) = INDEX.may_load(deps.storage, wallet)? {
                    if let Some(t) = TEMPLATES.may_load(deps.storage, index)? {
                        if state
                            .snapshot_after
                            .as_ref()
                            .map(|a| wallet.as_str() <= a.as_str())
                            .unwrap_or(false)
                            && t.player.wallet == *wallet
                        {
                            through += 1;
                        }
                    }
                }
            }
            totals(&mut entry, &part, through)?;
        }
    }
    Ok(entry)
}

fn participation(
    mut deps: DepsMut,
    config: &Config,
    state: &State,
    wallet: &Addr,
    fleet: &Fleet,
    tiers: Uint128,
) -> StdResult<()> {
    let part = PARTICIPATION.may_load(deps.storage, wallet)?;
    if part
        .as_ref()
        .map(|p| p.cycle == state.cycle && p.tiers == tiers)
        .unwrap_or(false)
    {
        return Ok(());
    }
    if part
        .as_ref()
        .map(|p| p.cycle == state.cycle)
        .unwrap_or(false)
    {
        accrue(deps.branch().storage, state, wallet)?;
    } else {
        let clan = if let Some(address) = &config.clans {
            let member: racer_clans::Member = deps.querier.query_wasm_smart(
                address,
                &racer_clans::QueryMsg::Member {
                    wallet: wallet.to_string(),
                },
            )?;
            if let Some(id) = member.active {
                if !CLAN_CYCLES.has(deps.storage, (state.cycle, id)) {
                    let info: racer_clans::Clan = deps
                        .querier
                        .query_wasm_smart(address, &racer_clans::QueryMsg::Clan { id })?;
                    CLAN_CYCLES.save(
                        deps.storage,
                        (state.cycle, id),
                        &ClanCycle {
                            score: Uint128::zero(),
                            tiers: Uint128::zero(),
                            leader: info.leader,
                        },
                    )?;
                }
            }
            member.active
        } else {
            None
        };
        CYCLE_PLAYERS.update(deps.storage, (state.cycle, wallet), |old| -> StdResult<_> {
            let mut entry = old.unwrap_or_default();
            entry.clan = clan;
            entry.sponsor = fleet.sponsor.clone();
            Ok(entry)
        })?;
    }
    PARTICIPATION.save(
        deps.storage,
        wallet,
        &Participation {
            cycle: state.cycle,
            through: state.completed,
            tiers,
            weighted: weighted_tiers(tiers.u128() as u32)?,
        },
    )?;
    Ok(())
}

pub fn scan(mut deps: DepsMut, config: &Config, state: &mut State, limit: u16) -> StdResult<bool> {
    let wallets: Vec<String> = deps.querier.query_wasm_smart(
        &config.fleet,
        &racer_fleet::QueryMsg::Players {
            start_after: state.snapshot_after.clone(),
            limit: Some(u32::from(limit)),
        },
    )?;
    let mut meta = META.load(deps.storage)?;
    let before = meta.clone();
    for text in &wallets {
        let wallet = deps.api.addr_validate(text)?;
        let path = Map::<&Addr, Fleet>::new("fleets").key(&wallet);
        let bytes = deps
            .querier
            .query_wasm_raw(&config.fleet, path.to_vec())?
            .ok_or_else(|| fail("missing authoritative fleet"))?;
        let mut fleet: Fleet = from_json(bytes)?;
        if fleet.effective_race <= state.race {
            fleet.active = fleet.next;
        }
        let tiers = Uint128::new(fleet.active.iter().map(|t| u128::from(*t)).sum());
        if tiers.is_zero() {
            continue;
        } // A first purchase after activation joins the following race.
        let previous = HEADERS.may_load(deps.storage, &wallet)?;
        let index = if let Some(h) = &previous {
            if h.index == 0 || h.index > meta.count {
                return Err(fail("invalid registry index"));
            }
            h.index
        } else {
            if meta.count >= config.parameters.max_players {
                return Err(fail("race capacity exceeded"));
            }
            meta.count += 1;
            INDEX.save(deps.storage, &wallet, &(state.race, meta.count))?;
            meta.count
        };
        let old_tiers = previous
            .as_ref()
            .map(|h| h.tiers.iter().map(|n| u128::from(*n)).sum())
            .unwrap_or(0);
        let dirty = DIRTY.has(deps.storage, &wallet);
        let changed = previous
            .as_ref()
            .map(|h| h.tiers != fleet.active)
            .unwrap_or(true)
            || dirty;
        if changed
            || previous
                .as_ref()
                .map(|h| h.generation != meta.generation)
                .unwrap_or(true)
        {
            let player = if changed {
                let equipment: Position = deps.querier.query_wasm_smart(
                    &config.equipment,
                    &racer_equipment::QueryMsg::Position {
                        wallet: wallet.to_string(),
                        slot: Slot::Commander,
                    },
                )?;
                let mut p = make_player(
                    state.race,
                    wallet.clone(),
                    fleet.active,
                    equipment.active.map(|e| e.attributes),
                )?;
                for ship in &mut p.ships {
                    ship.regen = Uint128::zero();
                }
                p
            } else {
                TEMPLATES.load(deps.storage, index)?.player
            };
            motion::set_base(
                deps.storage,
                meta.generation,
                index,
                &player,
                &config.parameters,
            )?;
            TEMPLATES.save(
                deps.storage,
                index,
                &Template {
                    player,
                    generation: meta.generation,
                },
            )?;
        }
        if dirty {
            DIRTY.remove(deps.storage, &wallet);
        }
        meta.total_tiers = meta
            .total_tiers
            .checked_sub(Uint128::new(old_tiers))?
            .checked_add(tiers)?;
        if previous
            .as_ref()
            .map(|h| h.cycle != state.cycle || h.tiers != fleet.active)
            .unwrap_or(true)
        {
            participation(deps.branch(), config, state, &wallet, &fleet, tiers)?;
        }
        let header = Header {
            index,
            tiers: fleet.active,
            generation: meta.generation,
            cycle: state.cycle,
        };
        if previous.as_ref() != Some(&header) {
            HEADERS.save(deps.storage, &wallet, &header)?;
        }
    }
    if meta != before {
        META.save(deps.storage, &meta)?;
    }
    state.participants = meta.count;
    state.registry_scanned = state
        .registry_scanned
        .checked_add(wallets.len() as u16)
        .ok_or_else(|| fail("registry scan overflow"))?;
    state.snapshot_after = wallets.last().cloned().or(state.snapshot_after.clone());
    if wallets.len() < usize::from(limit) {
        state.tier_sum = state.tier_sum.checked_add(meta.total_tiers)?;
    }
    Ok(wallets.len() < usize::from(limit))
}

pub fn accrue_page(mut deps: DepsMut, state: &mut State, limit: u16) -> StdResult<bool> {
    let after = state
        .settlement_after
        .as_ref()
        .map(|s| deps.api.addr_validate(s))
        .transpose()?;
    let wallets = CYCLE_PLAYERS
        .prefix(state.cycle)
        .keys(
            deps.storage,
            after.as_ref().map(Bound::exclusive),
            None,
            Order::Ascending,
        )
        .take(usize::from(limit))
        .collect::<StdResult<Vec<_>>>()?;
    for wallet in &wallets {
        accrue(deps.branch().storage, state, wallet)?;
    }
    state.settlement_after = wallets
        .last()
        .map(ToString::to_string)
        .or(state.settlement_after.clone());
    Ok(wallets.len() < usize::from(limit))
}
