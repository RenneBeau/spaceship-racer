# Spaceship Racer

[![Checks](https://github.com/RenneBeau/spaceship-racer/actions/workflows/checks.yml/badge.svg)](https://github.com/RenneBeau/spaceship-racer/actions/workflows/checks.yml)
![Status: private alpha](https://img.shields.io/badge/status-private_alpha-91c9ed)
![CosmWasm](https://img.shields.io/badge/contracts-CosmWasm-b5e88b)

**Build a permanent fleet. Race on-chain. Upgrade for the next launch.**

![Spaceship Racer hangar artwork](front/ui-assets/scene-hangar.webp)

Spaceship Racer is a Terra Classic game with CosmWasm contracts, a responsive
Keplr wallet interface, a read-only indexer, and a keeper that advances races
and relays verifiable drand randomness.

This is the standalone source repository for the current on-chain implementation.
The hosted private alpha is at [colapsar.space](https://colapsar.space): authorized
wallets sign in there and use the **Open Spaceship Racer** button. Access is
invite-only; this repository does not grant access to that deployment.

## The game

- **Three ship classes:** racers for speed, tanks for protection and reduced
  repair costs, fighters for rocket attacks.
- **Permanent ownership:** buy ships and levels; upgrades become active at the
  next race boundary, preserving the current race's snapshot.
- **On-chain boosts:** overdrive, acceleration, warp, rockets and paid repair.
- **Verifiable randomness:** future drand rounds, BLS proof verification,
  distance-weighted winners and bounded race processing.
- **Fleet progression:** commanders, fusion, skins, clans, an NFT marketplace,
  reward claims and optional reward-to-level reinvestment.
- **Transparent settlement:** integer accounting, separate SpaceUSD/SRT rewards,
  confirmed transaction receipts and recovery of unused boosts.
- **Presale implementation:** an explicitly started 50-day schedule that moves
  continuously from a 50% discount to full price. Its presence in source does
  not mean a release sale has begun.

The alpha uses **test assets with no monetary value**. This is development code,
not an audited mainnet release. See [project status](docs/STATUS.md).

## Quick start: source review and offline tests

Requirements: **Node.js 22 LTS** (Node 20.19+ also supported) and npm. No wallet,
blockchain, secrets or Docker are needed for the following commands:

```sh
git clone https://github.com/RenneBeau/spaceship-racer.git
cd spaceship-racer
npm ci
npm test
npm run build
```

`npm test` runs the chain-independent JavaScript suites and keeper-planning
tests. `npm run build` bundles the wallet UI; it does not start a chain or deploy
contracts. Starting the interactive game requires the local stack below.

## Run your own local game

You need Linux or a compatible development environment, Docker with Compose,
Git, Rust/rustup and network access for dependency downloads and drand proofs.

```sh
rustup toolchain install 1.81.0 --profile minimal
rustup target add wasm32-unknown-unknown --toolchain 1.81.0
npm run local:deps
npm run local:build:dex
npm run local:build
npm run local:up
npm run local:smoke
npm run build
npm run local:stack
```

Open **http://127.0.0.1:8787**. The stack runs the indexer, race/NFT keeper and
treasury keeper in the foreground. Stop those processes with Ctrl+C; stop the
chain with `npm run local:stop`.

The smoke command deploys and exercises compiled Wasm on the isolated local
chain and writes `contracts/local/deployment.json`. It creates fixture activity
and is intended for a fresh local environment, not as a production deployment
command. Existing chain state is not automatically erased.

Local fixture wallet keys are publicly known test credentials. **Never fund
them on a real network.** Generated alpha keys remain inside Docker's test
keyring. Details: [development](docs/DEVELOPMENT.md).

## Repository map

| Path | Purpose |
| --- | --- |
| [`contracts/contracts/`](contracts/contracts/) | Fleet, races, rewards, NFTs, payments, clans and supporting CosmWasm contracts |
| [`contracts/packages/`](contracts/packages/) | Shared contract rules and integer math |
| [`contracts/local/`](contracts/local/) | Isolated testnets, Wasm builds, deployment fixtures and keepers |
| [`src/onchain/`](src/onchain/) | Read-only HTTP API, SQLite event index, history and display projections |
| [`src/alpha/`](src/alpha/) | Standalone test-asset faucet and restricted RPC/LCD gateway |
| [`front/onchain/`](front/onchain/) | Responsive wallet UI, transaction builders and hangar comparisons |
| [`tests/onchain/`](tests/onchain/) | Unit, integration and browser tests |
| [`docs/`](docs/) | Setup, architecture, status and operations guidance |

## Documentation

- [Architecture and trust boundaries](docs/ARCHITECTURE.md)
- [Development, environment and test commands](docs/DEVELOPMENT.md)
- [Private alpha hosting](docs/ALPHA.md)
- [Current status and limitations](docs/STATUS.md)
- [Race protocol](contracts/RACE_PROTOCOL.md)
- [Player features and repair](contracts/PLAYER_FEATURES.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Third-party notices](NOTICE.md)

## License

Original game source is [ISC licensed](LICENSE), except where a component states
otherwise. In particular, the local LP-token wrapper is **GPL-3.0-only** and the
self-hosted Rajdhani fonts carry their included license. Artwork is not covered
by the source-code license; see [NOTICE.md](NOTICE.md).
