# Player rewards, reinvestment and repair

Deployed on LocalTerra on 2026-11-03. Race code **49**; payment/reinvestment
gateway code **51** (initial cutover used code 50), address
`terra10jsmew5rqtk4kptymz75lncg0tguwleuj60mw5hn0wxuexkder0qvmuvjt`.
Healing activated for race **309**.

## Reward display

The main display uses **current earned points and current distributable pools**.
It does not divide a player's earned points by projected future points.
For example, 41.8 / 107.8 points against 612 SRT is 237.306122 SRT.

SRT allocation is `pool × player points / total points`.

SpaceUSD mirrors `race::settle` and `ClanPremiums`:

1. A clan receives `pool × clan points / total points`.
2. The cycle's snapshotted leader premium is removed from that bucket.
3. Members split the rest by accumulated participating ship levels, using each
   race's active fleet rather than a player's latest purchased levels.
4. Solo players use their own point share.
5. Maintenance is deducted, capped at the member's gross reward.
6. Leader premium and earned maintenance referrals are added to their recipients.

`reward-allocation.mjs` follows the contract's integer-floor order. Clan leader
identity is read from the on-chain cycle snapshot, not queued clan changes.
The displayed breakdown is the allocation if settlement used current totals;
it is separate from already claimable credit. Available SpaceUSD includes
uncheckpointed accrual against the funded reserve. Existing liabilities and
stale previous-cycle budgets are excluded. The final result changes with future
races, active levels, funding, boosts and cycle duration.

## Reward conversion and fractional progress

The gateway reuses the existing payment adapter's five-token routes and exact
SpaceUSD invoice settlement. The existing fleet authorizes the gateway's
`PayFor` callbacks; fleets are always credited to the original player.

Manual **Convert rewards** submits one player-signed transaction containing:

- Claims from the legacy backing pool and gateway wherever credits exist.
- A SpaceUSD CW20 send for the amount selected by the player, with the chosen
  ship in the `invest` hook.

All steps revert together if the investment fails. SRT and uninvested claimed
SpaceUSD go to the player. Fractions stay in a per-wallet, per-ship SpaceUSD
balance until enough accumulates to buy whole levels. The UI shows next-level
progress; whole levels activate next race. Saved progress is withdrawable.
The fraction displayed uses the current SpaceUSD price per level.

The legacy fleet still enforces **1–1000 whole levels per ship**. This gateway
does not remove that limit; capped ships are excluded from new UI selections.
Any excess saved balance remains withdrawable.

Automatic conversion requires the player's signed `SetAutoReinvest` preference
(ship and percentage). That share of **future allocations** is invested inside
cycle settlement, before the next race launches. Each automatic subcall has a
bounded gas budget. Failed conversions roll back independently, leaving the
reward credit claimable and queued for the keeper to retry in batches of five.
The keeper cannot spend other wallet funds, change the selected ship,
or redirect proceeds. Changing or disabling the preference clears pending
automatic processing while leaving the credits claimable. Legacy credits remain
claimable through the original pool; no existing credit was moved during cutover.

## Repair boost

One repair request per wallet per race window is accepted through the SRT CW20
hook `{"race":…, "kind":"heal", "count":1}`. The amount sent is a **player-chosen
spending limit**, not a fixed repair price. The preview quotes known damage;
the authoritative cost is determined during deferred replay.

The current settings are:

- `price_unit`: 1 SRT
- `hp_per_unit`: 1000 HP
- `fallen_ship_bps`: 2500 (25% surcharge per fallen owned ship)

Ignoring micro-token rounding:

`price = price_unit × sqrt(missing HP and shields / (hp_per_unit × (tank level + 1))) × (1 + fallen ships × surcharge)`.

There is no price ceiling. Cost increases sublinearly; higher tank levels reduce
cost for an equal amount of damage. Integer square roots round the quoted cost
up. An owner can queue new validated settings with `ConfigureHealing`; they
activate at a race boundary.

Repairs apply before movement/combat in their action window. They restore owned
ships' HP and shields, including fallen ships. Full-fleet revival restores the
distance saved at destruction. Speed is reconstructed from elapsed logical ticks
and acceleration boosts. Distance does not accumulate while the fleet is dead;
expired temporary-distance bonuses are not resurrected. There are no per-tick
healing writes and no passive regeneration.

If the actual price exceeds the authorized reserve, no repair is performed and
the reserve becomes refundable. If it fits, only actual repair cost is spent;
the balance is refundable. An unused request after an earlier race ending is
also refundable. Executed repair spending follows the normal boost fee/prize
split. Native and JavaScript replay agree on restoration, price and movement.

## Display

The race scene has three animated ships with HP/shield bars and active equipped
skins. Image nodes persist across polling, avoiding animation resets. Reduced
motion is supported. The profile fleet now uses equipped skins too.

A finished-race overlay shows the winner, clan and historical active fleet,
with a short finish animation. It deduplicates race results and defers while
another modal or signing operation is active. Historical fleet data is not
substituted with a later upgraded fleet if unavailable.

Small native popover info cards explain races, pools, boosts and multiplier odds.

## Verification and cutover evidence

- Workspace Rust tests and Clippy (`-D warnings`) pass.
- The isolated `compose-feature-test.yml` chain runs on RPC **28659**, LCD **1519**.
- `verify-player-features.mjs` executes compiled Wasm with real drand BLS proofs:
  fractional accumulation, opt-in automatic upgrades, gateway cycle settlement,
  full-fleet destruction/revival, reserve refunds and exact indexer replay match.
- `feature-verification-results.json` records the compiled-Wasm run and artifact
  checksums. The upgrade script refuses artifacts that differ from this report.
- The live browser test claims legacy credits, saves and withdraws fractional
  progress, enables a 50% fighter auto-upgrade policy and disables it again.
- Cutover preserved eight wallets, three fleets, running race 308/cycle 82,
  balances, old reward credits, clan data and NFT collections. The index database
  was backed up and copied with its new deployment identity, retaining history.

Historical verification reports and operator recovery snapshots are not part of
this source distribution. See [development](../docs/DEVELOPMENT.md) for the
supported checks in a fresh clone.
