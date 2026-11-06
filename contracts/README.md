# CosmWasm workspace

The Rust workspace contains the on-chain Spaceship Racer game and local test
fixtures. Start with the [root README](../README.md) for installation and the
[architecture guide](../docs/ARCHITECTURE.md) for responsibilities.

```sh
cargo test --locked --manifest-path contracts/Cargo.toml --workspace
```

Deployable Wasm is built with Rust 1.81.0 by `npm run local:build`. CL8Y test
dependencies are fetched and built separately with `local:deps` and
`local:build:dex`.

## Protocol references

- [Race processing](RACE_PROTOCOL.md)
- [NFTs and marketplace](ASSET_PROTOCOL.md)
- [Payments and clans](PAYMENTS_AND_CLANS.md)
- [Skins and keeper](SKINS_AND_KEEPER.md)
- [Player features, repair and reinvestment](PLAYER_FEATURES.md)

These references include historical fixture notes. They are not a declaration
that any address or deployment on the maintainer's machine is available to a
fresh clone. Runtime manifests are generated locally and ignored by Git.
