# Persistent fleet registry

The registry reuses full-health base templates across races, applying
race-specific damage, utility and distance overlays separately. A new race epoch
excludes previous damage/boost state without requiring a full per-player reset.

Every launch still performs an authoritative **O(N) raw-fleet scan**. It compares
headers, appends new registrations and updates changed base entries/tree paths.
Equipment-boundary changes refresh commander statistics at the correct boundary.
This is not a claim of constant-time launch work.

Participation checkpoints accrue the old level segment before an upgrade
installs the new one. An accrual phase materializes remaining player/clan
contributions before cycle budgeting and allocation. Integer rounding and page
boundaries are covered by differential tests.

`advance_batch` permits bounded steps and page sizes, stopping at processing
family boundaries. The keeper simulates bounded candidates instead of assuming
every population fits one transaction.

The [integrated tick pipeline](TICK_PIPELINE.md) supplies the current oracle
processing path. See [deferred processing](DEFERRED_RACE.md) for replay semantics.
