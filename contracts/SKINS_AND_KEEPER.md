# Milestone skins and local keeper

## Milestone entitlement

`racer-skins` reads the wallet's permanent fleet directly. By default each ship
type earns one skin credit per 100 purchased tiers, up to ten credits per type at
tier 1,000. Credits are independent for racer, tank and fighter. Purchasing through
the payment adapter still credits the real wallet, not the adapter.

The wallet calls `Request { ship }`. No additional SRT price is introduced for
these earned rewards; transaction gas still applies. One credit is reserved,
the eligible catalogue is committed, and a fixed future drand round is requested.
There is at most one pending draw per wallet/ship type. Multiple earned credits
are claimed sequentially, not by looping over unbounded inventory.

Anyone can call `Fulfill { id }` after verification. The result is selected using
the committed weights and domain-separated rejection sampling, and minted directly
to the original wallet. Minting and claim completion are atomic. A failed mint,
pause or unavailable proof cannot consume the claim without delivering its NFT.

The operator can configure the milestone step and catalogue. Existing claims keep
their catalogue and reserved milestone. Eligibility for later requests uses the
current step. The collection minter is handed to the skin contract through the
same two-step CW721 ownership flow used for recruitment.

## Current-owned duplicate avoidance

The collection maintains a skin-code count keyed by wallet, ship kind and code.
Mint, transfer, send and burn update it in the same transaction as the actual
CW721 ledger. Multiple copies acquired by trading are counted correctly. Racer
and tank codes remain independent even if their strings match.

For approved game custodians:

- A bound or pending-bound skin still counts for the equipment holder.
- A listed skin still counts for the seller.
- Unbinding or cancelling a listing preserves that wallet's ownership count.
- A completed sale removes the seller's count and adds the buyer's count.

CW721's approved-operator sends attribute custody to `ReceiveNft.sender`, matching
the wallet recorded by equipment/marketplace. Ownership updates require standard
CW721 authorization; a website or receiver cannot fabricate a counter update.

This uses bounded direct counters, not a scan of every NFT a wallet holds. The
catalogue is limited to 32 skins per ship type (96 total); a draw checks at most
32 counters in one collection query. Cosmetics remain authoritative in equipment
slots and do not change ship statistics.

### Required custody setup

The collection owner registers equipment and marketplace using:

```json
{"extension":{"msg":{"set_custodian":{"address":"<contract>","allowed":true}}}}
```

Registration requires a contract address and must happen before that address
holds any NFTs from this collection. Equipment and marketplace independently
reject skin deposits when their registration is missing. This prevents silent
misattribution from a partially configured deployment.

Skin entry into game custody must use `SendNft`, not bare `TransferNft`. Transfers
back to wallets on unbind/cancellation/sale remain standard. Disabling a custodian
blocks new deposits but preserves existing beneficiary records and permits exits
when the game is otherwise unpaused. Re-enabling it does not require rebuilding
the index. The global emergency pause still blocks all user NFT operations.

The index covers wallet ownership plus approved game custody. It does not claim
to identify the human behind other wallets or infer entitlements in arbitrary
external contracts. This is current ownership, not lifetime uniqueness.

### Catalogue exhaustion and ownership changes during a draw

If every available code is already owned, `Request` fails without reserving a
credit or requesting randomness. Unclaimed credits remain available for a later
sale/transfer/burn or catalogue expansion.

If the selected code is acquired after commitment, fulfillment waits. It does
not pick another code, reset the request or mint a duplicate. The original claim
can resume once that code is no longer owned. This prevents outcome-dependent
rerolls while enforcing current-owned avoidance at mint time too.

`Preview { id }` exposes one of `waiting_for_randomness`, `duplicate_owned`,
`ready` or `fulfilled`. It uses the same selector as execution. `Status` exposes
earned/available credits, progress and currently eligible codes. `Pending` exposes
bounded global request IDs for a keeper, with no indexer authority required.

## Collection queries

Counts:

```json
{"extension":{"msg":{"skin_counts":{"wallet":"<wallet>","kind":"racer_skin","codes":["racer1","racer2"]}}}}
```

`Beneficiary { token_id }` returns both the actual CW721 owner and tracked skin
beneficiary. `Custodian { address }` reports registration, and `Controls` reports
the operator, local pause flag and shared pause controller. All are read-only.

These counters are populated from fresh skin mints. A future upgrade of a
collection that already contains unindexed skin NFTs needs an explicit backfill;
the current launch plan is fresh deployment, not legacy-asset migration.

## Executable local keeper

The smoke script writes public addresses to ignored `local/deployment.json`.
After it completes, run from `contracts/`:

```sh
node local/keeper.mjs --once         # one bounded race/proof action
node local/keeper.mjs --once --jobs  # one recruitment/skin action
node local/keeper.mjs                # continuous alternating queues; Ctrl-C stops
```

The keeper checks both manifest and node chain ID before signing, and only
supports `spaceship-racer-local`. It uses the isolated Docker test keyring, gas
simulation with adjustment 1.3, and the configured gas price. It is not a mainnet
operator/key-management deployment.

It:

1. Stops broadcasting while the shared game pause is active.
2. Seals only closed action windows and prioritizes the current tick over a
   growing backlog of later blocks.
3. Relays the exact requested drand proof, with a bounded public-beacon cache.
4. Advances deterministic race pages.
5. Rotates through bounded recruitment/skin pending-job pages and finalizes ready
   jobs for their original wallets.
6. Skips locally paused collections and duplicate-blocked skin draws without a
   paid fulfillment attempt.

Normal stale-work conflicts from other permissionless callers are retried in a
later pass. Unexpected authorization, invalid-proof or broadcast failures stop
the process instead of repeatedly spending fees. There is no weaker randomness
fallback and no private signing material in the deployment manifest.

The keeper does not choose treasury trading policy or dispatch capital. Batched
conversion remains operator-triggered; automated conversion thresholds and the
Overlord strategy bot are separate work.

## Verification

Native tests cover credits, per-ship reservations, real drand proof fixtures,
wallet/bound/listed ownership, sale-driven eligibility, replacement/unbinding,
rejected callbacks, counter updates/burns/operators, post-commit duplicates,
catalogue changes, pause/failed-mint recovery, revoked custody and misconfiguration.

Compiled-Wasm smoke earns ten racer credits, draws three distinct local catalogue
skins, binds/lists them, confirms exhaustion retains seven credits, sells one and
claims that code again. It also runs the keeper executable to relay a live proof,
mint a tank milestone skin and advance the race. Local catalogue entries reference
the existing image codes; public rarity/weight balancing remains operator tuning.

Remaining project work includes the read-only indexer/frontend, production keeper
operations, treasury automation/strategy bot, remaining referral/admin flows,
final parameter/migration policy and worst-case compiled-Wasm load testing.
