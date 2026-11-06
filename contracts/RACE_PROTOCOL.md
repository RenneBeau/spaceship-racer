# Local race protocol

**Accounting version 2:** [DEFERRED_RACE.md](DEFERRED_RACE.md) supersedes the
per-tick movement/regeneration lifecycle described below. During the race only
actions and ending checks are recorded; canonical action replay and winner
selection run after closing. The indexer supplies labelled no-regeneration
previews. The former lifecycle remains supported until a running legacy race ends.

Implemented by `contracts/race`, integrated with fleet, equipment, rewards and
the proof-verified drand adapter. Clan aggregation is enabled through the optional
cycle-bound clan contract; see `PAYMENTS_AND_CLANS.md`.

## Lifecycle and authority

```
Boundary -> DrainEquipment -> Snapshot -> Waiting
                                            |
                               future verified seed
                                            v
                                        Movement
                                            v
                                         Combat
                                            |
                         continue -> Waiting / end -> Boundary
                                            |
                                 final race of cycle
                                            v
                           Checkpoint -> Budget -> Settlement
                                            v
                                    next cycle Boundary
```

- `Advance { limit }` is permissionless; limit is 1–20. Callers cannot choose
  which players are processed: cursors follow the snapshot order.
- Fleet advancement, equipment processing and prize allocation are exclusive
  cross-contract capabilities of the engine.
- `BeginBoundary` freezes new equipment changes while the existing queue drains.
  Every queued change must complete before `EndBoundary`. A request cannot enter
  a previously processed page and unpredictably activate in the same snapshot.
- Fleet purchases always write the next race's tier state. Current active tiers
  remain stable throughout paginated snapshot construction.
- Fleet registration is capped at 1,000 wallets, matching the engine's current
  supported scope. The owner may lower the cap before registration reaches it,
  or raise it back up to 1,000. Extra registrations cannot halt snapshots.
- A race with no participants produces no winner/score and returns to the
  boundary stage; it does not consume one of the cycle's 100 completed races.

## Block action windows and delayed verification

Timing version 2 separates **live utility activation** from **final race
resolution**. Accepting a paid speed, acceleration, or temporary-distance boost
writes the bounded `LIVE_PENDING` ledger in that same transaction/block.
`LivePlayer` immediately exposes that power, including acceleration and expiry
at the current action height. The UI uses it for live power counters and projects
travel from each boost's activation tick.

`Player` remains the resolved history used by movement, combat, and winner
selection. A later boost cannot alter an earlier unresolved tick. Movement
removes the corresponding live-pending record as the effect becomes resolved,
so powers are not applied twice. If the race ended before that action's tick,
the live effect is discarded and its escrow remains recoverable. Rocket targets
and final results still require the verified future beacon.

Let `H` be the first action block. Logical tick `t` accepts boosts only in block
`H + t - 1`. The engine derives this tick from the current height; a player cannot
choose an earlier tick in the message. Boost messages include the race ID to
prevent accidental application to a subsequent race.

`Seal { limit }` can seal consecutive ticks only after their action blocks have
closed. Each seal requests a fixed future drand round; the adapter stores the
request under the engine address and a monotonically increasing ID. An existing
request cannot be replaced, including when delivery is delayed.

The keeper can seal later closed blocks while earlier ticks are still processing.
This separates logical block timing from oracle and processing latency. Multiple
ticks may use the same beacon round when catching up; domain separation makes
their draws distinct. The race can be visually/indexer-lagged while still assigning
every accepted action to its original logical block.

If an earlier tick ends the race, later escrowed boosts never affect that race
and are refundable. The delay does not let a player buy a boost for a tick after
seeing that tick's stopping outcome.

## Hidden stopping rule

No random duration or race-ending seed is selected at race start. Configuration holds
only a public duration CDF. With prior cumulative probability `F(t-1)`, tick `t`
stops with conditional probability:

```
(F(t) - F(t-1)) / (1 - F(t-1))
```

This reproduces the configured duration distribution while requiring fresh,
unavailable randomness for each closed tick. The maximum duration remains public.

`local/parameters.mjs` discretizes the legacy `floor(U^1.5849625007 * 1450) + 1`
duration distribution into six-second blocks: 242 maximum ticks (1,452 seconds),
with the temporary-distance effect lasting 50 ticks (300 seconds). Only the
offchain configuration builder uses floating point; contract execution uses
integer thresholds and fixed-point stats.

All random draws use rejection sampling and distinct domains for multiplier,
stopping, winner, rocket target and critical hit, bound to the chain, engine,
race, tick, actor and rocket ordinal. There is no keeper-chosen random input.

## Movement and combat

The engine snapshots the permanent fleet and active commander at the race boundary.
Stats use milli-units. Each tick processes all movement before any rockets:

1. Expire temporary distance; apply speed/acceleration/temporary boosts.
2. Advance distance by damage-adjusted speed times configured tick seconds.
3. Accumulate acceleration and regenerate surviving ships for that interval.
4. Build a distance-weighted Fenwick tree in bounded pages.
5. Resolve rocket salvos in fixed wallet-snapshot order.

Rocket targets exclude their attacker and are drawn proportionally to currently
eligible distance. Dead fleets have zero weight. The Fenwick tree uses logarithmic
selection and updates, avoiding a scan of every wallet for each rocket.

Salvos scheduled while the fighter survived the movement phase remain outgoing
even if another salvo destroys it during that tick's combat phase. Targets and
deaths update in fixed actor order, never keeper-selected order. Unopposed rockets
are refunded. Tank shielding and the tank -> fighter -> racer damage priority
follow the legacy engine.

The original rocket payment expression used `attack * 0.4` **micro-SRT**, so a
tier-one fighter's rocket costs 200 micro-SRT in this implementation. This unit
choice is explicit and configurable through `rocket_cost_bps`; it is not silently
changed to 200 whole SRT.

Boost payments stay in escrow until processed. Used payments are split per
player/tick into 95% prize funding and 5% treasury fees. Per-player rounding keeps
the split independent of keeper batch size. Dead-before-movement actions and
unusable rockets accrue refund credits. `ClaimRefunds { race, ticks }` also claims
payments for ticks beyond the race's actual end; duplicate claims cannot pay twice.

## Scores and reward cycles

The multiplier is committed separately by `PrepareNextMultiplier` while the
current race is running. Its request is permanently bound to the next race ID;
repeated calls cannot replace it. The next boundary waits for that commitment's
verified proof, then exposes the multiplier as soon as the next race is launched,
before tick one is sealed. It no longer depends on the first movement/ending
beacon. The multiplier's domain uses tick zero and retains the legacy odds.

- Winner selection is proportional to post-combat distance, not greatest distance.
- Multipliers preserve the legacy probability thresholds.
- Score includes the winner's commander reward bonus. Total score is the sum of
  actual credited score, preventing commander bonuses from overallocating pools.
- After 100 completed races, checkpoint the actual reward reserve and freeze
  SpaceUSD/SRT budgets. Later funding/accrual cannot change this cycle's budget.
- Page through cycle participants; divide the frozen pools by score. Floors/dust
  remain backed in the reward contract for later cycles.
- Maintenance uses gross cycle SpaceUSD budget, each wallet's race-snapshot sums,
  and total participating tier sums. Marginal bracket weighting is computed for
  each race before summing. Equal race-count denominators cancel algebraically.
- Maintenance is capped at the player's gross SpaceUSD reward, with no wallet
  debit or debt. Referral shares come from collected maintenance; the rest goes
  to treasury. SRT prizes are not deducted for maintenance.
- Reward allocations create accumulated wallet claims. The settlement cursor
  cannot reallocate a completed page. Net allocations and fee transfers execute
  atomically with cursor advancement.

If all fleets die and there is no distance, no winner receives points. If a cycle
has no score, its budgets remain in the reward contract instead of being invented
or paid to a fallback winner. Empty funded pools do not prevent racing.

## Keeper operations

1. Query engine `State` and proactively prepare/reveal the next race multiplier.
2. During active races, seal closed blocks in bounded batches.
3. Query `SeedRequest`, then the randomness adapter's request, and relay that exact
   round's drand signature when available.
4. Call `Advance` repeatedly for the current phase. The native batch-invariance
   test compares complete results and payouts for limits 1 and 20.
5. Index `Result`, `CyclePlayer`, `Settlement`, allocation/claim and refund events.

The deployment smoke test performs these steps and exercises the executable
local keeper in `local/keeper.mjs`. Continuous local operation is available;
production operator setup and indexer integration remain.

## Verification and remaining boundaries

The suite tests 100-race settlement, 1,000-wallet bounded processing, batch
invariance, upgrade snapshots, action cutoffs, delayed-end refunds, expired
temporary boosts, unopposed rockets, paused-engine rejection, oracle stalls,
weighted interval selection and reward backing. Compiled Wasm is exercised with
live drand signatures and a deliberately accelerated one-race test cycle.

The measured local `Advance` gas is for one wallet; it is not a throughput result
for 1,000 wallets or worst-case rocket spam. Large compiled-Wasm load benchmarks,
mainnet checks and the remaining frontend/operations work remain
open. Shared global pause is now implemented across configured modules, including
NFT transfers; see `ASSET_PROTOCOL.md`. An oracle outage currently waits for the exact
requested beacon; there is no cancellation, reroll or weaker randomness fallback.
