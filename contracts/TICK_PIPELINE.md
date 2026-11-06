# Integrated tick pipeline

The race engine supports verified tick checks using the configured beacon
verifier. A keeper can batch the proof/check and bounded follow-up processing
without changing action-window eligibility, random outcomes or page invariance.

- Open windows cannot be processed as closed windows.
- Proofs remain tied to the committed drand round and race/tick context.
- Cached authenticated seeds do not permit skipping future-round validation.
- Catch-up stops at the revealed race-ending boundary; later paid windows remain
  refundable after the final result is committed.
- Launches commit their multiplier ahead of the next race; the first movement
  proof is not required merely to display that committed multiplier.

The executable local keeper is `local/keeper.mjs`. Its planning module has
chain-independent tests; contract tests compare integrated/legacy paths and
bounded processing. UI telemetry remains a labelled estimate, not an oracle.
