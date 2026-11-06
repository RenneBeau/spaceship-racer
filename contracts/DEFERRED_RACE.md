# Deferred race processing

The current engine retains these no-regeneration/deferred-combat rules while
using the [persistent registry](PERSISTENT_REGISTRY.md) and
[integrated tick pipeline](TICK_PIPELINE.md).

During a race, purchases retain their payment-block action window and ending
checks advance the compact race state. The first stopping draw fixes
`replay_until`; new actions are then rejected. Canonical replay resolves movement,
boosts and combat using committed seeds, with bounded pages for large salvos.
Final winner selection, rewards and refunds are based on on-chain results.

There is **no passive regeneration**, including from repair commanders. Paid
repair is explicit. Full-health ship state is restored by the next-race snapshot.

`src/onchain/race-preview.mjs` projects supported pending actions using the same
integer/floor-sum movement rules. Unknown randomness is never guessed. Fork
rewinds invalidate preview context, and confirmed combat reconciles matching
preview hits rather than replaying their animation.

The Rust `race` tests cover replay/page invariance, destruction, paid repair,
late-action refunds and exact closed-form movement. JavaScript tests cover
preview state and event/fork handling. See [development](../docs/DEVELOPMENT.md).
