# What's new

## New

- **R&C: A Crack in Time** is now playable (experimental)
- **Canary** release channel — opt into bleeding-edge builds, installs side-by-side with stable
- **In-app Documentation** viewer under Help → Documentation
- **Full-map GLB export** — bake an entire level (mobys, ties, details, shrubs, foliage, terrain, sky) into a single file
- **Sound tab** with SFX / Dialog / Music sub-tabs and per-category counts
- **Wizard franchise tabs** — switch Resistance ↔ Ratchet & Clank in one click

## Resistance: Fall of Man

- Foliage and shrubs extract and render
- Detail clusters surfaced as their own asset type
- Skybox dome decoded and visible in the viewport
- Viseme rigs work — soldier, cartwright, Winters etc. play their expression / blink / lipsync clips correctly
- Gameplay placements parsed from `ps3gameplay.dat`
- Cached weapon rigs drive cleanly in Godot

## R&C: Tools of Destruction

- All 142 ties extract (was failing at 81+)
- Buildings are no longer 30 km tall (per-axis tie scale fixed)
- Zone reader ported — ~5,700 tie instances + ~5,400 terrain pieces on stratus city
- Simple animations (doors, spinners, fills) play correctly

## R&C: A Crack in Time

- Wizard card unlocked, opens on the V2 pipeline
- Both vanilla and mod-extracted levels supported
- `highmips.dat` is optional now — works with `textures.dat` alone (half-res mip chain)
- Tie shader resolution fixed (ACiT uses a different offset than R2 / R3)
- Logic-only mobys without geometry no longer crash extraction

## All games

- Texture extraction now runs in three phases (Materials → Normal maps → Textures) with real progress counts
- Status bar shows mobys / ties / terrain / materials / textures / animations
- Cleaner default extraction log — probes only fire with `RECHIMERA_LOG_PROBES=1`
- Multi-moby debug filter: `RECHIMERA_DEBUG_MOBY=0212,00CD,0326` extracts only what you ask for
- Texture streaming back to async — meshes show up instantly, textures fill in as they arrive

## Tooling

- Dual-channel release pipeline — `main` ships stable, `develop` ships canary
- Auto-update wired up on Windows for both channels
- `bun run version:canary` puts your local working tree into canary mode for `cargo tauri dev`
- Versions auto-sync across `Cargo.toml`, `package.json`, and `tauri.conf.json`

## Known issues

- TOD character animations export as T-pose (complex format not yet reverse-engineered)
- TOD has no skybox decoder
- ACiT levels need their sibling `.psarc` (globals / common) extracted alongside for the last ~5% of textures
- Collision geometry isn't parsed — generate from GLB on Godot import as a workaround
- FBX export is disabled this round (use GLB)

## Install

Pick the bundle for your OS. Windows shows an **Update** button on existing installs; macOS / Linux reinstall manually.

For pre-release builds, see the [canary channel](../../releases?q=canary&expanded=true). Expect breakage; file issues with the full version string (e.g. `0.4.0-15`).

## Acknowledgements

- [@VELD-Dev](https://github.com/VELD-Dev) — [ReLunacy / LibLunacy](https://github.com/VELD-Dev/ReLunacy)
- [@NefariousTechSupport](https://github.com/NefariousTechSupport) — [Lunacy / 7th igRewrite](https://github.com/NefariousTechSupport/7thigRewrite)
- [@PredatorCZ](https://github.com/PredatorCZ) — [InsomniaToolset / Spike](https://github.com/PredatorCZ/InsomniaToolset)
