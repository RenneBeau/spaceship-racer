#!/usr/bin/env bash
set -euo pipefail
root="$(dirname "$(dirname "$(realpath "$0")")")"
dex="$(dirname "$root")/vendor/cl8y-dex-terraclassic"
revision=8e865d3721231189cbad1730c7a24698038c9cfb
if [[ "$(git -C "$dex" rev-parse HEAD)" != "$revision" ]]; then
  printf '%s\n' "CL8Y checkout must be at $revision" >&2
  exit 1
fi
cargo +1.81.0 build --manifest-path "$dex/smartcontracts/Cargo.toml" --locked --lib --release \
  --target wasm32-unknown-unknown -p cl8y-dex-factory -p cl8y-dex-router -p cl8y-dex-pair
artifacts="$dex/smartcontracts/target/wasm32-unknown-unknown/release"
optimizer=cosmwasm/optimizer@sha256:b9c92b2900b7ebaab3499203615c1b8589592bc557355ed3432e48851ffde69e
docker_user="$(id -u):$(id -g)"
# In rootless Docker, container uid 0 maps to the normal host user; container
# uid 1000 instead maps to a subordinate uid that cannot write these artifacts.
if [[ "$(docker info --format '{{json .SecurityOptions}}')" == *rootless* ]]; then
  docker_user=0:0
fi
for crate in cl8y_dex_factory cl8y_dex_router cl8y_dex_pair; do
  docker run --rm --user "$docker_user" -v "$artifacts:/artifacts:z" --entrypoint wasm-opt \
    "$optimizer" -Oz --signext-lowering "/artifacts/$crate.wasm" -o "/artifacts/$crate.optimized.wasm"
done
node "$root/local/size-check.mjs" --dex-only
# The game workspace's racer-local-lp entry-point wrapper builds the pinned
# upstream LP implementation without resolving unrelated upstream schema tools.
