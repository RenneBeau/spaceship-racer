# Third-party notices

The root ISC license applies to original game code, not automatically to every
dependency, font, or visual asset distributed with this project.

## CL8Y DEX and local LP fixture

- Upstream: <https://github.com/PlasticDigits/cl8y-dex-terraclassic>
- Development checkout: `8e865d3721231189cbad1730c7a24698038c9cfb`
- Downloaded by `npm run local:deps` into ignored `vendor/`; upstream source and
  its license notices remain intact there.
- `contracts/contracts/local-lp` declares **GPL-3.0-only** and delegates to
  <https://github.com/PlasticDigits/cw20-mintable>, pinned at
  `73a206b5c39c5bd8f9d3574bda32fc5ae8f52ec8`. Its license is separate from the
  original game's ISC code. Review the upstream terms when distributing that
  fixture or its compiled Wasm.

## Fonts

The Rajdhani font files in `front/ui-assets/` retain the terms in
[`RAJDHANI-LICENSE.txt`](front/ui-assets/RAJDHANI-LICENSE.txt).

## Artwork

The ship, commander and interface imagery is carried forward from the original
Spaceship Racer project. No separate permissive artwork license is granted by
the code license. Ask the maintainer before reusing branding or artwork outside
this project.

## Other dependencies

JavaScript dependencies are locked in `package-lock.json`; Rust dependencies are
locked in `contracts/Cargo.lock`. Their own licenses and notices apply.
