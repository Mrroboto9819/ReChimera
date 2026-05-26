# What's new

## All games

- New **app skin system** with five skins: Default, Resistance: Fall of Man, Resistance 2, Resistance 3, R&C: A Crack in Time
- Per-skin **light/dark mode locking** — dark-only skins disable the light toggle automatically
- Each skin recolors **the entire app shell** (top bar, panels, tabs, lists, modals), not just dialogs
- Per-skin **UI sound effects** — drop `select.wav` / `confirm.wav` / `error.wav` / `back.wav` / `modal-open.wav` / `skin-switch.wav` into `public/ui-effects/<skin>/`
- Settings → General gained a **Skin** picker and **UI sound on/off + volume** slider
- New reusable **SearchField** component, skin-aware
- New reusable **FallbackImage** component with per-game thumbnail resolution
- **Franchise placeholder images** when no per-level art is available (`r_notset.webp`, `rc_notset.jpeg`)
- Map list now sorts by **natural numeric order** (`level20`, `level99`, `level100` instead of `level10`, `level100`, `level11`)
- **Cache prompt modal auto-hidden** when the wizard owns the flow
- New **"Don't close the app — it's working"** alert during heavy extraction / cache-build phases (animated, prominent)
- Bulk sound extraction now packs the whole level into a single `.zip` in one click
- TabContainer **hardened against stale persisted tab IDs** — no more `i18nKey` crashes after upgrade
- PSARC encryption detection — wizard surfaces a clear message when a dump is still PS3-disc-encrypted, with RPCS3 boot guidance

## Resistance: Fall of Man

- USRDIR wizard now supports RFOM (was V2-only)
- **`game.psarc` auto-extract** step inside the wizard
- Encrypted PSARC magic-sniff with actionable guidance (boot the game in RPCS3 first)
- Detection of both **post-extract layouts** — V2-style `built/levels/<map>/<entry>` and RFOM-direct `packed/levels/<level>/<entry>`
- Frame-stride padding fix carried over from experimental
- Additive-animation `numBones` override carried over
- GLB material exporter binds **albedo + normal + emissive** (was albedo-only)
- Texture global fallback via shared TUID tree

## Resistance 2 / Resistance 3 / R&C: A Crack in Time / All 4 One

- **Single shared wizard** for every V2 game — pick the game and the same USRDIR → globals → maps → level flow runs
- Per-game **USRDIR persistence** in localStorage (so each game remembers its own folder)
- Per-game **map images folder** — drop `public/<gameId>/maps/<level>.png` and it shows up
- **SP-stem fallback** for coop / multiplayer maps — ship `chicago.png` once and `chicago_coop` / `chicago_multiplayer` pick it up
- PSARC extraction is **name-agnostic** — extracts every `.psarc` in a level folder regardless of name (mods, DLC, non-standard packings all work)
- **AssetWorkbench** view added (per-asset 3D scene, submesh / texture / animation drawer, frame-by-frame timeline scrubber, Export GLB)
- Open-in-Workbench entry points from Cache Library + Inspector
- Missing-texture submeshes get a **magenta tint** instead of default white-under-light
- Hierarchy shows **inline submeshes** with texture-availability chips (green if cached, magenta if engine-fallback)

## R&C: Tools of Destruction

- No new features this round — still routes through the generic folder picker
- See "Working on" below

## Working on / Future

- ToD wizard adaptation (still uses the generic folder picker)
- ToD character animations decoding (T-pose only currently)
- ToD skybox decoder
- Collision geometry parsing (currently a Godot-side workaround)
- FBX export (disabled this round; use GLB)
- Integrated PS3 disc decryption (currently an external pre-step)
- `hover.wav` global delegate and `back.wav` for non-modal back actions

## Install

Pick the bundle for your OS. Windows shows an **Update** button on existing installs; macOS / Linux reinstall manually.

For pre-release builds, see the [canary channel](../../releases?q=canary&expanded=true). Expect breakage; file issues with the full version string (e.g. `0.5.1-15`).

## Acknowledgements

- [@VELD-Dev](https://github.com/VELD-Dev) — [ReLunacy / LibLunacy](https://github.com/VELD-Dev/ReLunacy)
- [@NefariousTechSupport](https://github.com/NefariousTechSupport) — [Lunacy / 7th igRewrite](https://github.com/NefariousTechSupport/7thigRewrite)
- [@PredatorCZ](https://github.com/PredatorCZ) — [InsomniaToolset / Spike](https://github.com/PredatorCZ/InsomniaToolset)
