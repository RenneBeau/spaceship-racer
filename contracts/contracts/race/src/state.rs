use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Uint128};
use cw20::Cw20ReceiveMsg;
use cw_storage_plus::{Item, Map};
use racer_collection::Attributes;

pub const CDF_SCALE: u32 = 1_000_000_000;
pub const SCALE: u128 = 1000;
pub const MAX_BATCH: u16 = 20;
pub const TREE_SIZE: u16 = 16384;

#[cw_serde]
#[derive(Default)]
pub struct MigrateMsg {
    #[serde(default)]
    pub reward_gateway: Option<String>,
}

#[cw_serde]
pub struct Parameters {
    pub tick_seconds: u16,
    pub races_per_cycle: u16,
    /// Integer cumulative duration probabilities. Generated offchain from the
    /// legacy distribution; never contains a preselected race end.
    pub duration_cdf: Vec<u32>,
    pub boost_price: Uint128,
    pub speed_boost: Uint128,
    pub acceleration: Uint128,
    pub temporary_distance: Uint128,
    pub temporary_ticks: u16,
    /// 4000 applies an economy rate of 0.0004 SRT per attack unit.
    pub rocket_cost_bps: u16,
    pub rocket_damage_bps: u16,
    pub boost_fee_bps: u16,
    pub maintenance_referral_bps: u16,
    pub max_actions_per_tick: u16,
    pub max_players: u16,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: String,
    pub fleet: String,
    pub equipment: String,
    pub rewards: String,
    pub randomness: String,
    pub srt: String,
    pub treasury: String,
    pub parameters: Parameters,
    pub resume_from: Option<String>,
}

#[cw_serde]
pub struct Config {
    #[serde(default)]
    pub healing: crate::healing::Settings,
    #[serde(default)]
    pub pending_healing: Option<crate::healing::Settings>,
    #[serde(default)]
    pub tick_verifier: Option<Addr>,
    #[serde(default)]
    pub pending_tick_verifier: Option<Addr>,
    #[serde(default)]
    pub tick_safety_seconds: u64,
    #[serde(default)]
    pub pending_accounting_upgrade: bool,
    #[serde(default)]
    pub accounting_version: u8,
    #[serde(default)]
    pub timing_version: u8,
    pub restore_from: Option<Addr>,
    pub clans: Option<Addr>,
    pub owner: Addr,
    pub fleet: Addr,
    pub equipment: Addr,
    pub rewards: Addr,
    pub randomness: Addr,
    pub srt: Addr,
    pub treasury: Addr,
    pub parameters: Parameters,
    pub paused: bool,
}

#[cw_serde]
pub enum ExecuteMsg {
    ConfigureHealing {
        settings: crate::healing::Settings,
    },
    SetTickVerifier {
        address: String,
    },
    CheckTick {
        race: u64,
        tick: u32,
        signature: Option<Binary>,
        seal_limit: u16,
    },
    PrepareNextMultiplier {},
    Restore {
        limit: u16,
    },
    SetClans {
        address: Option<String>,
    },
    SetPauseController {
        controller: Option<String>,
    },
    Advance {
        limit: u16,
    },
    AdvanceBatch {
        limit: u16,
        steps: u16,
    },
    Seal {
        limit: u16,
    },
    Receive(Cw20ReceiveMsg),
    ClaimRefunds {
        race: u64,
        ticks: Vec<u32>,
    },
    Configure {
        paused: Option<bool>,
        parameters: Option<Parameters>,
    },
}

#[cw_serde]
pub enum BoostKind {
    Heal,
    Speed,
    Acceleration,
    Temporary,
    Rocket,
}
#[cw_serde]
pub struct BoostMsg {
    pub race: u64,
    pub kind: BoostKind,
    pub count: u32,
}

#[cw_serde]
pub enum Phase {
    Restore,
    ClanPremiums,
    Boundary,
    DrainEquipment,
    Snapshot,
    Waiting,
    Movement,
    Combat,
    Checkpoint,
    Accrue,
    Budget,
    Settlement,
}

#[cw_serde]
pub struct State {
    #[serde(default)]
    pub inline_oracle: bool,
    #[serde(default)]
    pub registry_enabled: bool,
    #[serde(default)]
    pub registry_generation: u64,
    #[serde(default)]
    pub registry_scanned: u16,
    #[serde(default)]
    pub replay_until: u32,
    #[serde(default)]
    pub rocket_cursor: u32,
    #[serde(default)]
    pub combat_spent: Uint128,
    pub clan_after: Option<u64>,
    pub leader_bps: u16,
    pub phase: Phase,
    pub race: u64,
    pub cycle: u64,
    pub completed: u16,
    pub cursor: u16,
    pub snapshot_after: Option<String>,
    pub participants: u16,
    pub start_height: u64,
    pub tick: u32,
    pub sealed: u32,
    pub next_request: u64,
    pub seed: Binary,
    pub multiplier: u16,
    pub total_distance: Uint128,
    pub total_score: Uint128,
    pub tier_sum: Uint128,
    pub budget_usd: Uint128,
    pub budget_srt: Uint128,
    pub settlement_after: Option<String>,
    pub escrow_srt: Uint128,
}

#[cw_serde]
#[derive(Default)]
pub struct Ship {
    pub base_hp: Uint128,
    pub hp: Uint128,
    pub shield: Uint128,
    pub regen: Uint128,
    pub speed: Uint128,
    pub attack: Uint128,
}
#[cw_serde]
pub struct Temporary {
    pub expires: u32,
    pub distance: Uint128,
}
#[cw_serde]
pub struct Player {
    #[serde(default)]
    pub materialized_tick: u32,
    pub race: u64,
    pub wallet: Addr,
    pub tiers: [u16; 3],
    pub ships: [Ship; 3],
    pub commander: Option<Attributes>,
    pub safe_speed: Uint128,
    pub distance: Uint128,
    pub temporary: Vec<Temporary>,
    pub acceleration_count: u64,
    pub alive: bool,
    pub salvo: u32,
    pub utility_spent: Uint128,
}

#[cw_serde]
#[derive(Default)]
pub struct Boosts {
    pub speed: u32,
    pub acceleration: u32,
    pub temporary: u32,
    pub rockets: u32,
    pub paid: Uint128,
}
#[cw_serde]
#[derive(Default)]
pub struct LiveTick {
    pub tick: u32,
    pub speed: u32,
    pub acceleration: u32,
    pub temporary: u32,
}
#[cw_serde]
pub struct LivePending {
    pub race: u64,
    pub boosts: Vec<LiveTick>,
}
#[cw_serde]
pub struct LivePlayer {
    pub player: Player,
    pub pending: Vec<LiveTick>,
    pub activation_tick: u32,
}
#[cw_serde]
pub struct MultiplierCommitment {
    pub race: u64,
    pub request_id: Option<u64>,
    pub available_at: Option<u64>,
    pub multiplier: Option<u16>,
}
impl Boosts {
    pub fn count(&self) -> u64 {
        u64::from(self.speed)
            + u64::from(self.acceleration)
            + u64::from(self.temporary)
            + u64::from(self.rockets)
    }
}

#[cw_serde]
#[derive(Default)]
pub struct CyclePlayer {
    pub clan: Option<u64>,
    pub score: Uint128,
    pub tier_sum: Uint128,
    pub weighted_sum: Uint128,
    pub sponsor: Option<Addr>,
}
#[cw_serde]
pub struct ClanCycle {
    pub score: Uint128,
    pub tiers: Uint128,
    pub leader: Addr,
}
#[cw_serde]
pub struct RaceResult {
    pub race: u64,
    pub cycle: u64,
    pub ticks: u32,
    pub winner: Option<String>,
    pub multiplier: u16,
    pub total_distance: Uint128,
}
#[cw_serde]
pub struct RocketShipDelta {
    pub index: u8,
    pub hp_before: Uint128,
    pub hp_after: Uint128,
    pub max_hp: Uint128,
    pub shield_before: Uint128,
    pub shield_after: Uint128,
}
#[cw_serde]
pub struct RocketHit {
    pub race: u64,
    pub cycle: u64,
    pub tick: u32,
    pub attacker: String,
    pub target: String,
    pub ordinal: u32,
    pub critical: bool,
    pub damage: Uint128,
    pub hp_before: Uint128,
    pub hp_after: Uint128,
    pub hp_lost: Uint128,
    pub max_hp: Uint128,
    pub shield_before: Uint128,
    pub shield_after: Uint128,
    pub shield_lost: Uint128,
    pub destroyed: bool,
    pub ships: Vec<RocketShipDelta>,
}
#[cw_serde]
pub struct Settlement {
    pub gross_usd: Uint128,
    pub srt: Uint128,
    pub maintenance: Uint128,
    pub sponsor_usd: Uint128,
}
#[cw_serde]
pub struct TreeNode {
    pub race: u64,
    pub tick: u32,
    pub weight: Uint128,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(crate::healing::Quote)]
    HealQuote { wallet: String },
    #[returns(Option<racer_randomness::Request>)]
    TickBeacon { race: u64, tick: u32 },
    #[returns(Option<Binary>)]
    TickSeed { race: u64, tick: u32 },
    #[returns(Vec<(u16, Player)>)]
    Players {
        start_after: Option<u16>,
        limit: u16,
    },
    #[returns(Uint128)]
    RocketPrice { wallet: String },
    #[returns(LivePlayer)]
    LivePlayer { wallet: String },
    #[returns(MultiplierCommitment)]
    Multiplier { race: u64 },
    #[returns(Config)]
    Config {},
    #[returns(State)]
    State {},
    #[returns(Player)]
    Player { wallet: String },
    #[returns(RaceResult)]
    Result { race: u64 },
    #[returns(CyclePlayer)]
    CyclePlayer { cycle: u64, wallet: String },
    #[returns(Settlement)]
    Settlement { cycle: u64, wallet: String },
    #[returns(Option<u64>)]
    SeedRequest { race: u64, tick: u32 },
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const STATE: Item<State> = Item::new("state");
pub const PLAYERS: Map<u16, Player> = Map::new("players");
pub const INDEX: Map<&Addr, (u64, u16)> = Map::new("index");
pub const BOOSTS: Map<(u64, u32, &Addr), Boosts> = Map::new("boosts");
pub const REFUNDS: Map<&Addr, Uint128> = Map::new("refunds");
pub const CYCLE_PLAYERS: Map<(u64, &Addr), CyclePlayer> = Map::new("cycle_players");
pub const CLAN_CYCLES: Map<(u64, u64), ClanCycle> = Map::new("clan_cycles");
pub const RESULTS: Map<u64, RaceResult> = Map::new("results");
pub const SETTLEMENTS: Map<(u64, &Addr), Settlement> = Map::new("settlements");
pub const SEEDS: Map<(u64, u32), u64> = Map::new("seeds");
pub const MULTIPLIERS: Map<u64, u64> = Map::new("multipliers");
pub const LIVE_PENDING: Map<&Addr, LivePending> = Map::new("live_pending");
pub const VERIFIED_ROUNDS: Map<(u64, u64), Binary> = Map::new("verified_rounds");
#[cw_serde]
pub struct SealedBeacon {
    pub round: u64,
    pub available_at: u64,
}
pub const INLINE_BEACONS: Map<(u64, u32), SealedBeacon> = Map::new("inline_beacons");
pub const TREE: Map<u16, TreeNode> = Map::new("tree");
