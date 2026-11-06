#!/usr/bin/env bash
set -euo pipefail
home="$HOME/.terra"
if [ ! -f "$home/config/genesis.json" ]; then
  terrad init spaceship-racer-alpha --chain-id spaceship-racer-alpha >/dev/null 2>&1
  mkdir -p "$home/key-backups"
  chmod 700 "$home/key-backups"
  for role in operator keeper faucet; do
    terrad keys add "alpha-$role" --keyring-backend test --output json > "$home/key-backups/$role.json" 2>/dev/null
    chmod 600 "$home/key-backups/$role.json"
  done
  operator="$(terrad keys show alpha-operator -a --keyring-backend test)"
  keeper="$(terrad keys show alpha-keeper -a --keyring-backend test)"
  faucet="$(terrad keys show alpha-faucet -a --keyring-backend test)"
  terrad add-genesis-account "$operator" 1000000000000000000uluna,1000000000000000000uusd
  terrad add-genesis-account "$keeper" 10000000000000000uluna,1000000000000000uusd
  terrad add-genesis-account "$faucet" 100000000000000000uluna,100000000000000000uusd
  jq --arg operator "$operator" '
    .app_state.staking.params.bond_denom = "uluna" |
    .app_state.mint.params.mint_denom = "uluna" |
    .app_state.crisis.constant_fee.denom = "uluna" |
    .app_state.oracle.params.min_valid_per_window = "0.000000000000000000" |
    .app_state.wasm.params.code_upload_access = {permission:"AnyOfAddresses", addresses:[$operator]} |
    .app_state.wasm.params.instantiate_default_permission = "Everybody" |
    .consensus_params.block.max_gas = "100000000" |
    .consensus_params.block.max_bytes = "2000000"
  ' "$home/config/genesis.json" > "$home/config/genesis.updated.json"
  mv "$home/config/genesis.updated.json" "$home/config/genesis.json"
  terrad gentx alpha-operator 1000000000000uluna --keyring-backend test --chain-id spaceship-racer-alpha >/dev/null 2>&1
  terrad collect-gentxs >/dev/null 2>&1
fi
perl -0pi -e 's/^timeout_commit\s*=\s*"[^"]*"/timeout_commit = "6s"/m; s/^skip_timeout_commit\s*=\s*\w+/skip_timeout_commit = false/m; s/^laddr\s*=\s*"tcp:\/\/127\.0\.0\.1:26657"/laddr = "tcp:\/\/0.0.0.0:26657"/m' "$home/config/config.toml"
perl -0pi -e 's/^minimum-gas-prices\s*=\s*"[^"]*"/minimum-gas-prices = "28.325uluna,0.75uusd"/m; s/(\[api\]\s*\n(?:[^\[]*?))enable\s*=\s*false/${1}enable = true/s' "$home/config/app.toml"
# Docker publishes the API through the container interface, not its loopback.
perl -0pi -e 's/^address\s*=\s*"tcp:\/\/localhost:1317"/address = "tcp:\/\/0.0.0.0:1317"/m' "$home/config/app.toml"
exec terrad start --home "$home"
