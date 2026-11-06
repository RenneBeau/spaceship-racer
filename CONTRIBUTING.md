# Contributing

Thanks for helping build Spaceship Racer. Open an issue for bugs, protocol
questions or a proposed feature; include the affected screen/contract, expected
behavior and steps to reproduce. For mobile wallet problems, include the wallet
app and OS versions. Never include recovery phrases or private keys.

## Before a pull request

```sh
npm ci
npm test
npm run build
npm run test:contracts
```

For changes to deployment, contract interactions or the player flow, also run
the relevant integration/browser checks against your own local testnet. See
[development](docs/DEVELOPMENT.md). Explain which checks ran and any that could
not run; do not represent unit tests as end-to-end proof.

## Conventions

- Keep wallet transaction construction in the frontend and signing in Keplr or
  Leap. The indexer must stay read-only.
- Preserve exact token units and rounding. Use integers/BigInt for accounting.
- Race state is authoritative on-chain. UI estimates must be identified as such.
- Purchases/upgrades activate at the next race boundary; do not mutate current
  snapshots. Passive regeneration is disabled; repair is an explicit action.
- Keep mobile layouts, reduced motion, stable polling and focus retention in
  mind. Avoid rebuilding image-heavy sections on every refresh.
- Keep deployments, SQLite files, Docker keyrings and credentials out of Git.
- Keep fixes focused and describe any ABI or storage migration implications.

Please be respectful and constructive in issues and code review.
