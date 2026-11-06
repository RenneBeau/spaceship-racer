# NFT assets and coordinated pause

Implemented modules: `racer-marketplace`, `racer-recruitment`, `racer-collection`
and `racer-pause`, integrated with fleet, equipment, rewards, randomness and race.

## Marketplace

An allowlisted collection is listed using CW721 `send_nft` to the marketplace:

```json
{"payment_token":"<CW20 address>","price":"1000000"}
```

The JSON above is the base64-encoded hook message. The marketplace confirms NFT
custody, checks the payment allowlist, and records seller, collection/token ID,
price, fee and treasury. It never relies on an indexer ownership claim.

A buyer sends the exact price using that listing's CW20 `send` hook:

```json
{"listing_id":1}
```

Purchase atomically transfers the NFT to the buyer, seller net proceeds to the
seller, and the fee to treasury. The initial fee is 500 basis points, deducted
from the sale price. Fee/treasury changes apply to new listings only; an existing
listing retains its recorded terms. There is no internal user balance or deposit
wallet, and the seller cannot buy their own listing.

`Cancel { listing_id }` is seller-only and returns the escrowed NFT. Removal of
a collection from the listing allowlist does not prevent existing sellers from
cancelling. Disabling a payment token stops new purchases with that token. A failed
NFT delivery or payout rolls back payment, listing state and all other messages.

Queries expose individual listing history, paginated active listings, and
paginated active listings by seller. Each query returns at most 100 rows.

The five configured assets are independently allowlisted CW20 addresses. A listing
is denominated in exactly one of them; arbitrary other-token settlement is not
silently accepted or valued by an offchain quote. CL8Y ship-payment and treasury
conversion are implemented separately; marketplace listings retain their chosen
currency. Both native tests and compiled-Wasm smoke exercise five
local CW20 fixtures; these are not deployments of the live CL8Y assets.

## Collection and minter authority

The collection uses standard CW721 ownership, approvals, transfers, sends and
burns, with immutable typed gameplay attributes in each NFT extension. Cosmetic
metadata does not determine game stats.

The deployment owner initially instantiates the commander collection, then uses
CW721's two-step minter ownership handover:

1. Collection `UpdateOwnership(TransferOwnership)` names the recruitment contract.
2. Owner calls recruitment `AcceptMinter`, which sends `AcceptOwnership` as that
   contract.

After acceptance, only recruitment can mint commanders. Collection pause authority
remains the configured operator independently of minter ownership. The indexer
has neither minting authority nor control of the original NFT recipient.

## Recruitment commitment and randomness

Send SRT to recruitment using a CW20 hook:

```json
{"count":10}
```

Only counts 1 and 10 are accepted. The initial price is 500 SRT per pull, expressed
as 500,000,000 micro-SRT for the six-decimal fixture. Configuration stores the
smallest-unit price explicitly.

The transaction commits the original wallet, paid amount, treasury, count and a
bounded catalogue snapshot, then requests one fixed future drand round. Paid SRT
stays in contract escrow until successful fulfillment. Updating prices, weights,
catalogue entries or treasury cannot alter an already committed job.

After the proof is available, anyone may call `Fulfill { id }`. Each output is
selected by a domain-separated, rejection-sampled weighted draw and minted to the
original wallet. The first result of a ten-pull uses only epic/legendary entries;
the remaining nine use the full committed pool. A job can complete only once.

Minting and release of committed SRT to the recorded treasury happen atomically.
If a mint fails, payment remains escrowed and the same job/proof can be retried.
There is no cancellation or request replacement that could let a player reroll a
known outcome. An unavailable beacon waits for that exact round.

The default catalogue in `local/parameters.mjs` preserves the six legacy commander
codes, rarities, bonuses and weights. Catalogues are bounded to 64 unique codes,
require nonzero weights and all four rarities, and store authoritative attributes
onchain. Request IDs, recipients and minted token IDs are queryable by wallet.

## Fusion

The wallet approves recruitment to transfer the five inputs, either individually
or using CW721 `approve_all`, then calls:

```json
{"fuse":{"token_ids":["id1","id2","id3","id4","id5"]}}
```

All inputs must be distinct commanders from the configured collection, currently
owned by the caller, of the same non-legendary rarity. Bound and listed NFTs fail
the wallet-ownership check, even if an old approval exists.

The request escrows all five NFTs atomically and commits the next-rarity pool
before requesting future randomness. After verification, fulfillment burns all
five escrowed inputs and mints one next-rarity commander to the original wallet.
All burns and the mint roll back together if any operation fails. Inputs are not
burned while the outcome is still waiting on the oracle. No extra SRT fusion fee
is introduced; normal network gas still applies.

## One emergency pause

The pause contract exposes `State { owner, paused }`. The operator calls:

```json
{"set_paused":{"paused":true}}
```

Each participating contract checks that same state on every non-administrative
execution. This avoids partially completed batches of pause fan-out messages.
The shared state stops:

- Fleet purchases/upgrades and race processing/actions/refunds.
- Reward funding, allocation, maintenance transfers and claims.
- NFT minting, transfer/send, approvals and burns, including NFTs in user wallets.
- Equipment binding, unbinding and boundary processing.
- Marketplace listing, buying and cancellation.
- Recruitment requests, fusion intake and fulfillment.
- Randomness requests and proof submission.

Read-only queries and owner configuration remain available, including the ability
to unpause. Existing contract-local pause flags are additional restrictions; a
local unpause does not override a globally paused controller.

### Required wiring

After deployment, the owner sets the same address with `SetPauseController` on
fleet, rewards, equipment, race, randomness, marketplace, recruitment, payments,
treasury, clans and skins. For each
NFT collection, use `Extension(SetPauseController)`. The setter validates the
controller query interface and rejects unauthorized callers.

Controller configuration is optional for isolated unit fixtures and must be wired
for every game deployment. The compiled-Wasm smoke does this before exercising
the contracts, including the skin collection/module. This controller
cannot pause unrelated third-party CW20 contracts or external wallet operations.

## Verification and open integrations

The native asset suite tests five-token settlement, frozen listing terms,
unauthorized callbacks/cancellation, atomic rollback on failed NFT delivery,
committed catalogue/treasury behavior, epic guarantees, immutable recipients,
fusion approvals and duplicate/mixed/bound/listed input rejection, failed-burn
rollback, and coordinated pause across all current game modules.

Compiled-Wasm smoke uses independent local buyer and treasury wallets, real future
drand proofs, five payment fixtures, a ten-pull, a fusion, and pause/recovery of
pending operations. Test keys exist only in the isolated chain's keyring volume;
no live wallet credentials or real funds are used.

Milestone skins and the executable local race/NFT keeper are implemented; see
`SKINS_AND_KEEPER.md`. Remaining work includes referral services, production bot
operations and indexer/frontend integration. Clan state/reward settlement and
CL8Y conversion are covered in `PAYMENTS_AND_CLANS.md`. Recruitment revenue
and marketplace fees reach treasury in their paid asset, where the operator can
batch conversion into SpaceUSD. They do not directly refill the prize reserve.
