# Standalone alpha and private hosting

The hosted Colapsar alpha and a developer's standalone alpha are separate
deployments. This repository includes no hosted keys, login sessions, wallet
allowlist, deployment manifest or private portal configuration.

## Run a standalone test alpha

After installing dependencies and building game/DEX Wasm as in the README:

```sh
npm run alpha:up
npm run alpha:deploy
npm run alpha:start
```

Open **http://127.0.0.1:5110/spaceship-racer/**. The alpha uses RPC **27659**,
LCD **1419**, and chain ID `spaceship-racer-alpha`. Deployment refuses to overwrite
an existing alpha manifest.

Run the race/NFT keeper in another terminal:

```sh
RACER_MANIFEST="$PWD/contracts/local/alpha-deployment.json" \
RACER_COMPOSE="$PWD/contracts/local/compose-alpha.yml" \
node contracts/local/keeper.mjs
```

The alpha server binds to loopback. `RACER_PUBLIC_ORIGIN` must match the URL
opened by the browser; its default is `http://127.0.0.1:5110`.

## Starter funds

Connecting a wallet requests a server-recorded starter grant:

- **1,000,000 test LUNC** for gas.
- **1,000 each** of CLUNC, CUSTC, UST1 and SpaceUSD.
- **10,000 SRT**.

Funding uses a serial queue and durable SQLite grant records. Manual faucet
claims have per-wallet, per-IP and daily limits. Those limits also apply to new
automatic grants. Unknown transaction outcomes remain reserved for operator
review instead of being blindly retried. Protect and retain `data/alpha-faucet.sqlite`
when restarting/moving a deployment; deleting it loses the grant history.

The deployer does not pre-seed the SRT reward pool. Test-wallet balances, DEX
liquidity and native keeper gas are distinct from reward-pool funding.

## Private hosting

Before exposing this loopback service, put an authenticated reverse proxy in
front of **all** `/spaceship-racer` routes, including assets, faucet and chain
gateway routes. Do not equate a user-supplied wallet address with a verified
wallet login. Use a signed challenge and server-side session/authorization.

Native mobile wallets fetch balances separately from the in-app browser. Decide
explicitly how those reads are authorized; browser-only cookies are insufficient.
`RACER_PORTAL_WALLET_ACCESS=true` enables the frontend integration hook for a
hosting portal that implements `POST /spaceship-racer/wallet-access` and returns
authorized `{ rpc, lcd }` URLs. That portal is not provided by this repo.

Use HTTPS, keep Docker chain ports on loopback, and never serve `.env`, generated
manifests, SQLite files, Docker keyrings or `key-backups/` as web assets.

The standalone alpha uses freely mintable test tokens. Do not present those
assets as a release presale or as redeemable balances.
