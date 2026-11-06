# CL8Y payments, capital treasury and clans

## Verified integration targets

- CL8Y source revision: `8e865d3721231189cbad1730c7a24698038c9cfb`.
- Native integration uses the actual upstream factory, pair and router crates.
- Compiled-Wasm smoke deploys that factory/router and four real AMM pools on
  this project's isolated LocalTerra, with local CW20 assets.
- The LP fixture delegates to the upstream `cw20-mintable` implementation pinned
  at `73a206b5c39c5bd8f9d3574bda32fc5ae8f52ec8`. It is an entry-point wrapper,
  not alternative exchange or LP math.
- The upstream pair requires Binaryen optimization to fit the verified local
  819,200-byte uncompressed limit. `build-dex.sh` pins the optimizer image by
  digest; raw pair 916,665 bytes -> optimized pair 732,205 bytes.
- The build and local deployment do not alter or use mainnet pools/funds.

`game-common::cl8y` deliberately exposes only token-to-token, pool-only routing
with 1–4 hops. Hybrid books, native wrapping and user-selected recipients are not
part of the payment interface. Configured routes cannot repeat a token.

## Ship invoices and wallet identity

The fleet still settles in SpaceUSD. It exposes `Quote { wallet, purchase }`
using its tier prices and first-purchase referral discount. The payment contract
accepts any of its five approved CW20 assets and settles that exact invoice.

A CW20 `send` to payments uses this hook shape:

```json
{
  "purchase":{"upgrade":{"ship":"racer","target_tier":2}},
  "expected_price":"1000000",
  "minimum_receive":"1000000",
  "max_spread":"0.02",
  "deadline":1800000000,
  "route":["<input-token>","<SpaceUSD>"]
}
```

Use a current deadline, not the example timestamp. It must not be expired or
more than one hour ahead. The chosen route must exactly match the operator's
registered route, and user spread must be within the configured ceiling.

The contract:

1. Derives the payer from the authenticated CW20 sender, not a beneficiary field
   supplied by the website.
2. Re-queries the fleet invoice and checks `expected_price` and minimum output.
3. Executes the CL8Y swap with an enforced minimum and deadline, fixing output
   to the payment contract.
4. Measures actual SpaceUSD balance **increase**, excluding prior donations.
5. Pays the exact invoice using fleet `PayFor`, authorized only for the configured
   adapter, and returns surplus SpaceUSD to the original payer.

Direct SpaceUSD payments skip the DEX but use the same invoice and surplus rules.
The fleet records ships, tiers and referral relationships against the actual
wallet. It does not accidentally register the adapter as a player. A sponsor is
registered only on the first ship purchase, cannot change, and cannot form a
cycle; purchase events carry `sponsor` and `referral` attributes. `Referrals`
paginates the sponsor->wallet index, and the indexer exposes it at
`/api/wallet/:wallet/referrals` with accumulated SpaceUSD earnings.

Any swap, minimum-output, final invoice, authorization, or transfer failure rolls
back the entire transaction, including upstream AMM state. Network gas is not
refunded. There is no persistent deposited player balance. A pending-swap guard
prevents nested conversion calls.

Payment `Quote` also returns the registered route and an upstream reverse-simulated
offer estimate. Execution trusts neither that estimate nor a signed indexer quote:
the explicit minimum and actual token delta remain authoritative.

## Capital treasury and reward reserve separation

```
ship invoices -> SpaceUSD capital treasury -> operator dispatch -> Collapsar
fees/recruitment -> treasury in paid asset -> batched CL8Y conversion -> SpaceUSD

Collapsar realized rewards / manual seed -> reward contract Fund -> prize reserve
```

Marketplace fees, boost fees and recruitment proceeds can be directed to the
treasury address with their existing configuration interfaces. They arrive in the
asset paid. `Convert` is operator-only and uses a registered route, amount,
minimum, deadline and spread ceiling. Actual conversion output stays in treasury.
`SendCapital` (and its `SetCollapsar` alias) sends only SpaceUSD to the configured
Collapsar address.

The contract does not swap each tiny fee automatically: operators/automation can
batch economically sized conversions. It has no automatic purchase-to-prize-pool
path. The smoke test proves capital conversion/dispatch leaves reward holdings
unchanged, then deliberately seeds additional rewards in a separate transaction.

The Collapsar strategy bot and profit-settlement policy are separate work. Its
eventual SpaceUSD rewards can use the existing reward-contract funding hook.

`FundRewardPool { token, amount }` moves treasury-held SRT or SpaceUSD into the
configured `reward_pool` by CW20 `Send` with the `{"fund":{}}` hook. This is the
only reward-pool automation in the local stack: `local/keeper-treasury.mjs`
funds balances above an optional `TREASURY_RETAIN` reserve and never dispatches
Collapsar capital. `SetRewardPool` configures the destination.

Wire payments and treasury to the same emergency pause controller as the game.
Paused conversion/capital requests fail; admin routing and controller setup remain
available. Raw external CW20 transfers cannot be stopped by the receiving contract.

## Clan cycle state

The clan contract is bound to the race engine and fleet. Defaults preserve:

- 100 permanent tiers required to create a clan.
- 10 members, with a hard bounded roster implementation of at most 20.
- 5% leader premium.
- Wallet identity and unique onchain name/code (codes normalized to uppercase).

Create, join/switch, leave, leadership transfer, kicks and dismantling affect the
**next reward cycle**, never the cycle already running. Active and queued rosters
are stored separately; queries lazily normalize them against the cycle activated
by the engine. Pending joins reserve future capacity immediately, preventing
multiple wallets from overfilling a clan before settlement.

Only the engine can activate sequential cycles. Repeating the current cycle is
idempotent for empty-race retries. The engine can attach/change its clan contract
only between cycles. A fresh clan contract may be initialized at the engine's
previous cycle when attached to an existing local deployment.

A leader must schedule a successor or dismantle before leaving. The future
leader must remain in the queued roster. Current leadership keeps its authority
until the next cycle. A kicked wallet cannot cancel its own forced removal.
Voluntary pending changes can be cancelled if the restored roster still has
capacity. Dismantled clans retain historical IDs/name/code reservations.

The operator may configure the creation threshold, member cap and premium rate.
The engine snapshots the premium rate at cycle start so later changes do not
alter a settlement already in progress.

## Clan reward allocation

Each race snapshots current clan membership along with the fleet/commander.
The engine accumulates actual scores and participating tier sums per clan.

At cycle settlement:

1. Freeze the actual SpaceUSD/SRT budgets.
2. Give a clan a SpaceUSD bucket proportional to its total score versus all
   scored clan/solo buckets.
3. Allocate its leader premium in a separate bounded pass. A leader need not
   appear in the participant settlement page to receive the premium.
4. Divide the remainder among members by their race-summed participating tiers.
5. Apply each member's capped maintenance to that gross member allocation.
6. Allocate individual SRT prizes by individual scores, as before.

Solo wallets retain individual score-based SpaceUSD allocation. Per-race tier
sums preserve time-weighting when a member upgrades during the cycle. Rounding
dust remains backed in the reward contract. Leader premiums, member net payouts,
sponsor maintenance shares and treasury maintenance transfers cannot collectively
exceed the frozen budget.

Native tests verify membership/leadership activation, capacity reservations,
voluntary cancellation, forced kicks, dismantling, clan tier splits, separate
premiums and reserve backing. Compiled-Wasm smoke runs a two-wallet clan cycle,
checks both allocations, and activates a queued exit in the following cycle.

## Remaining feature work

Milestone skin issuance and bounded custody-aware duplicate avoidance are now
implemented, along with the executable local race/NFT keeper. See
`SKINS_AND_KEEPER.md` for claim commitment, current-ownership checks and automation.

Other open work: extra clan cancellation/admin UX, remaining fee referrals,
treasury conversion automation and production keeper operation, service-fee referrals beyond the first-purchase flow, final
parameter/migration policy, and worst-case compiled-Wasm load measurements.
