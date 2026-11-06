# Project status

## Included

Current on-chain game source, contract unit tests, browser interface, read-only
indexer, local keeper planning/execution, alpha faucet and gateway, isolated
Docker chain definitions and local deployment fixtures.

Core features include fleet purchases/upgrades, race/boost/repair processing,
verifiable randomness, claims/refunds, commanders/fusion, skins, clans,
marketplace, conversion routes and a 50-day presale implementation.

## Important boundaries

- The game is in **private alpha**, not an audited mainnet release.
- Release presale activation is a separate explicit deployment/operator action.
- The hosted Colapsar portal and its permissions are managed outside this repo.
- Contract tests and UI projections do not establish mainnet performance,
  economic safety or successful mobile signing on every supported wallet.
- Historical migration/recovery scripts and benchmark dumps from the development
  machine are intentionally excluded from this clean source distribution.
- Generated manifests, blockchain volumes, databases, secrets, logs and old Git
  history are excluded. A clone starts with source, not the hosted alpha state.
- Existing protocol reference documents may describe earlier fixture verification
  stages; the root README and `docs/` are the current setup guide.

## Useful next work

- Broader end-to-end mobile-wallet coverage.
- Reproducible fixture-based browser testing on CI.
- Independent contract/architecture review before real-value deployments.
- Further test coverage for storage migrations and interruption recovery.
