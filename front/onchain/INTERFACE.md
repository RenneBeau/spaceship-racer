# Game interface

`interface.css` supplies shared controls and responsive layouts. `scene.css` is
the game-art layer: self-hosted Rajdhani typography, a full-width space scene,
translucent HUD, formation flight and an illustrated ability dock. Component
styles remain in their existing files.

`interface.mjs` assembles the race workspace once during startup, preserving
element IDs and event handlers. Race statistics, pools and the multiplier sit
inside the scene HUD. Boost controls form its lower action dock. Standings,
combat/recent actions and wallet rewards use a tabbed console below the scene.
Phones use the same artwork and controls with a compact HUD and five-section
bottom navigation. Full boost calculations are available from the Details card.
Reward-upgrade settings use a dedicated dialog.

`scripts/prepare-scene-assets.mjs` creates compressed WebP derivatives of existing
project art and downloads two licensed Rajdhani font subsets for self-hosting.
Normal builds use those checked-in assets and do not contact a font service.
The font license is in `front/ui-assets/RAJDHANI-LICENSE.txt`.

`boost-info.mjs` computes display-only boost statistics from the active parameters
and current race player. Values use the contracts' milli-unit scale and BigInt:

- Speed: base gain, damage-adjusted gain and extra distance over one full window.
- Acceleration: commander-adjusted acceleration, per-window speed gain and an
  example over five windows. The example respects movement-before-acceleration
  ordering and integer floors; it is conditional on unchanged health and the race
  lasting that long.
- Warp: temporary selection distance, expiry and its effect on winner and rocket
  target weight. It is not travelled distance or extra movement speed.
- Rockets: fighter attack scaled by the configured damage rate, base salvo
  damage, critical chance and hit order. Overkill does not spill into another ship.
- Repair: missing HP/shields, fallen ships and tank level. The existing repair
  quote and player-authorized spending limit remain separate from these stats.

Per-boost summaries and selected-quantity details update from the same helper.
Missing weapon data is shown as pending instead of preventing the page from
rendering. Fleet image nodes persist during polling so scene animation is not
restarted by data refreshes. Reduced-motion rules remain supported.

Verification covers all eight sections at 320, 390, 768, 1024 and 1440 pixels,
boost parameter/quantity changes, reward-upgrade dialogs, equipped skins,
telemetry, wallet transactions and race-end overlays. Layout checks also ensure
ship health bars clear the instrument HUD. Purchase effects are shown only after
confirmed transactions, with queued actions labelled as queued. Reduced-motion
preferences disable decorative flight, star trails and action animation.
