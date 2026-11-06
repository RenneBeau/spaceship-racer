# Development

Run commands from the repository root unless stated otherwise.

## Dependencies

- Node 22 LTS and npm (`.nvmrc` is provided); `better-sqlite3` may need a native
  compiler and Python 3 if a prebuilt module is unavailable for your platform.
- Rust **1.81.0** and its `wasm32-unknown-unknown` target for deployable Wasm.
  The build scripts explicitly select this version to match the tested Terra VM.
- Docker/Compose and Git for local chains and CL8Y dependencies.
- Network access for dependency downloads and live drand proofs.

The committed JS and Rust lockfiles pin dependency resolutions. `local:deps`
checks out the pinned CL8Y revision into `vendor/`. Existing mismatched checkouts
are rejected rather than overwritten.

The scoped `@scure/base` override preserves CosmJS 0.38.1's working Bech32
decoder behavior. Later 2.x versions reject the infinite decode limit used by
that CosmJS release; removing the override requires validating address parsing.

## Checks by level

| Command | Requires | What it checks |
| --- | --- | --- |
| `npm test` | JS dependencies | Chain-independent unit and keeper-planning suites |
| `npm run build` | JS dependencies | Browser JavaScript and CSS bundling |
| `npm run test:contracts` | Rust dependencies | Native CosmWasm contract tests |
| `npm run test:integration` | Local smoke deployment | Live read-only HTTP API and manifest wiring |
| `npx playwright install chromium` | Network | Browser binary installation |
| `npm run test:browser` | Local smoke deployment + Chromium | Browser layouts, fixture-wallet actions and game interactions |

`test:browser` can send transactions using public local fixture credentials. Run
it only on a disposable local testnet. It is not a hosted-alpha/mainnet check.

## Local stack

Follow the README build/deploy sequence. The chain ID is
`spaceship-racer-local`, with loopback RPC **27657**, LCD **1417** and default UI
**8787**. A distinct Docker Compose project/volume name is used by this repo.
The six-second block cadence matches interactive action windows.

`local:stack` expects a generated manifest and a running chain; it starts the
indexer and keepers. Do not run duplicate keepers against the same fixture keys.
The `--deploy` option redeploys fixtures and should be used only intentionally.

No generated deployment manifest is bundled. `local:smoke` creates one after
deploying/exercising the compiled contracts. Do not point this command at a
production environment or reuse its fixture wallets with real funds.

## Environment

The application reads exported environment variables. `.env.example` is a
reference, not automatically loaded by the scripts. Node also supports
`node --env-file=.env ...` when invoking an entry point directly.

| Variable | Purpose |
| --- | --- |
| `RACER_MANIFEST` | Generated deployment JSON to load |
| `RACER_RPC`, `RACER_LCD` | Internal chain endpoints; otherwise from manifest |
| `RACER_PUBLIC_RPC`, `RACER_PUBLIC_LCD` | Browser/wallet-facing endpoints |
| `RACER_HOST`, `RACER_PORT` | Indexer bind address and port |
| `RACER_DATABASE` | SQLite index path; defaults under ignored `data/` |
| `RACER_START_HEIGHT` | Optional index starting height |
| `RACER_BLOCK_TIME` | Local chain block interval, default `6s` |
| `RACER_COMPOSE`, `RACER_KEEPER_KEY` | Keeper fixture Compose file/key name |
| `RACER_PUBLIC_ORIGIN` | Standalone alpha browser origin |
| `RACER_PORTAL_WALLET_ACCESS` | Enable integration with an external portal's wallet-access endpoint |

When a deployment changes, the interface fingerprints the contract set to avoid
silently mixing old addresses and new chain data.

## Troubleshooting

- **Missing manifest:** run the local build/up/smoke sequence first.
- **Wallet account does not exist:** standalone local fixtures require funded
  test accounts; the separate alpha grants a starter kit on first connection.
- **No races:** the keeper waits for the first fleet before launching.
- **Stale blocks:** inspect chain/keeper logs and `/api/health`; do not infer
  progress solely from HTTP reachability.
- **Mobile balance fetch fails:** native wallets do not share browser cookies.
  A private hosting proxy must explicitly support the required wallet queries.
- **Wasm rejected:** use the pinned compiler and the supplied DEX optimizer
  script; do not replace it with an arbitrary compiler's output.
