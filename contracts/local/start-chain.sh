#!/usr/bin/env bash
set -euo pipefail
home="${TERRA_HOME:-$HOME/.terra}"
if [ ! -f "$home/config/genesis.json" ]; then
  /usr/local/bin/init-chain.sh
  # This isolated fixture does not run Terra's native exchange-rate feeder.
  # Keep its only validator from being jailed for missing those votes. Spaceship
  # drand proof validation and consensus downtime protection are independent.
  perl -0pi -e 's/("min_valid_per_window"\s*:\s*)"[^"]*"/${1}"0.000000000000000000"/' "$home/config/genesis.json"
fi
# The upstream fixture defaults to 200ms blocks, but race action windows assume
# Terra's six-second block cadence. Preserve all existing chain data.
export LOCAL_BLOCK_TIME="${LOCAL_BLOCK_TIME:-6s}"
perl -0pi -e 's/^timeout_commit\s*=\s*"[^"]*"/timeout_commit = "$ENV{LOCAL_BLOCK_TIME}"/m' "$home/config/config.toml"
exec terrad start --home "$home"
