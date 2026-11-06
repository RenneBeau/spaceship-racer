#!/usr/bin/env bash
set -euo pipefail
root="$(dirname "$(dirname "$(realpath "$0")")")"
# Race imports the other crates in library mode. Build deployable dependencies
# separately afterward so feature unification cannot suppress their entry points.
for crate in racer-race racer-reinvestment racer-beacon-verifier racer-skins racer-clans racer-payments racer-treasury racer-recruitment racer-marketplace racer-pause racer-rewards racer-fleet racer-randomness racer-local-token racer-local-lp racer-equipment racer-collection; do
  cargo +1.81.0 build --manifest-path "$root/Cargo.toml" --locked --release --target wasm32-unknown-unknown -p "$crate"
done
node "$root/local/size-check.mjs"
