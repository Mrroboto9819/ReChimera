# Changes vs `origin/develop`

This release is `feature/fix-RFOM-and-not-all-R&C` →
`develop` merge. The previous RFOM-heavy round shipped foliage / shrubs
/ skybox / visemes; **this round adds Tools of Destruction and unlocks
R&C: A Crack in Time experimentally**, plus cross-game cache modal +
texture pipeline improvements, the new dual-channel release pipeline
(stable + canary), and a frontend cleanup that pulls FBX export back
into the toolbox while it's not yet user-ready. ACiT levels (both vanilla and
mod-extracted) now open end-to-end with geometry + ties + ufrags +
~95% of textures; the remaining gap is shared `.psarc` content and a
shader-additional-slots question.

**New lunalib modules** (parsers / writers)
- `crates/lunalib/src/foliage_rfom.rs` — RFOM Foliage (`0xC200` + `0x9700`) branch meshes + sprite quads
- `crates/lunalib/src/shrub_rfom.rs` — RFOM Shrub (`0xC700` + `0xC650`) mesh vegetation
- `crates/lunalib/src/skybox_rfom.rs` — RFOM sky dome (`0x9150` + `0xDA00`) + GLB/OBJ/PLY/JSON writers
- `crates/lunalib/src/detail_rfom.rs` — RFOM DetailCluster (`0xB200`/`0xB300`/`0x9500`) static debris
- `crates/lunalib/src/lighting_rfom.rs` *(superseded — see notes below)*
- `crates/lunalib/src/envsampler_rfom.rs` *(superseded — see notes below)*
- `crates/lunalib/src/level_glb.rs` — static level-scope GLB writer for the full-map exporter
- `crates/lunalib/src/rfom_probe.rs` — multi-interpretation byte-dump helpers (hex + f32 BE + u32 BE)

**Significantly extended**
- `crates/lunalib/src/skeleton.rs` — viseme-rig fix (`swap & 0x1F` mask for the 14 RFOM head rigs)
- `crates/lunalib/src/sound.rs` — bank + stream extraction polish, V1/V2 ID detection
- `crates/lunalib/src/animation.rs` — RFOM inline-anim decode path
- `crates/lunalib/src/gameplay_rfom.rs` — `GameplayInstances.other[6]` raw-byte probe
- `crates/lunalib/src/region_rfom.rs`, `tie_rfom.rs`, `tie_inst_rfom.rs` — UFrag/tie placement decode hardening
- `crates/lunalib/src/tie_old.rs` *(TOD)* — per-axis scale per-vertex; pre-walk meshes to compute `max_v_local_end` and slurp only what's needed (fixes the 61 failing ties)
- `crates/lunalib/src/moby_old.rs`, `tie_old.rs`, `moby_rfom.rs`, `tie_rfom.rs` — new `_with_total` variants firing `on_total` callback with section count upfront so cache modal shows real progress
- `crates/lunalib/src/zone_old.rs` *(TOD)* — extended from tie-instances-only to also read UFrag terrain at `0x6200` + identity shader table sized to `main.dat:0x5000`
- `crates/lunalib/src/moby.rs` *(V2 / ACiT)* — graceful fallback when per-moby `0xE100` (indices) / `0xE200` (vertices) are absent: emit empty mesh + one-time `[acit-probe]` multi-interp hex dump of unknown sections; new `[cache] mobys: total / with_geometry / logic_only` summary
- `crates/lunalib/src/tie.rs` *(V2 / ACiT)* — `shader_index` resolution is now ACiT-aware: tries V2's `+0x28` first, falls back to ACiT's `+0x0C` when V2 offset reads out-of-range; `+0x28` fast path unchanged for R2 / R3 / RCF / A4O
- `crates/lunalib/src/texture.rs` *(V2 / ACiT)* — `textures.dat` fallback when `0x1D1C0` high-mip pointer has `length == 0` (recovers per-texture base mips); `highmips.dat` open is now optional — when absent, every texture routes through `textures.dat` with half-resolution decoding (the meta declares top-mip dimensions, base-mip chain starts at `width/2 × height/2`); new `[tex-drop]` summary line categorising every drop reason and `[tex-fallback]` line listing the first 8 truly-missing texture IDs
- `crates/lunalib/src/shader.rs` *(V2 / ACiT)* — `[shader-probe]` diagnostic dump of the first 3 shaders' `0x5D00` section (hex + u32 BE) to RE additional texture slots beyond albedo / normal / expensive
- `apps/desktop/src-tauri/src/cache.rs` — pair-frame anim transform (when `stride==min_data && n8==0`), 3-phase texture write (materials → normalmaps → textures), `[tod-anim]` + `[tod-trans-probe]` diagnostic probes, `[anim-bytes]` byte-dump probe gated on `RECHIMERA_LOG_PROBES`, new `[cache] -> phase ...` markers + `V2 tie reader done — N ties` + `V2 textures done — encoded=N PNGs` end-of-phase lines so the log shows exactly where extraction is in the pipeline

**App / UI**
- `apps/desktop/src-tauri/src/cache.rs` (+855 lines) — full-map export, multi-moby debug filter, foliage/shrub/sky/gameplay cache writers
- `apps/desktop/src-tauri/src/main.rs` (+521 lines) — 7 Tauri sound commands, foliage/shrub instance routing
- `apps/desktop/src/components/CacheLibraryModal.tsx` (+1738 lines) — Sound tab playlist + SFX/Dialog/Music sub-tabs
- `apps/desktop/src/components/OpenLevelModal.tsx` — wizard franchise tab strip (Resistance / R&C)
- `apps/desktop/src/views/Viewport.tsx`, `Hierarchy.tsx`, `Settings`, store — shrub / foliage / detail kinds wired through

**Docs**
- `docs/internal/lunalib-and-IT/09-debugging-methodology.md` — codified "probe → log → re-extract → range-check → lock" loop
- `docs/internal/lunalib-and-IT/10-vegetation-and-sky.md` — RFOM shrub/foliage/skybox layouts
- Inline byte-offset comments in every new module — IT struct references + every `+0xNN field` documented
- Cache + frontend + export-pipeline chapters updated with new kinds, phases, and the franchise tab

**Mislabel fix worth knowing about**

Sections `0xC200` and `0x9700` were previously routed through
`lighting_rfom.rs` and `envsampler_rfom.rs` (producing bogus instances
at world origin). An IT lookup found their real definitions in
`foliage.hpp:42-72` — they're `Foliage` and `FoliageInstance`. The
mislabeled readers still exist as files but are no longer called from
`level_layout`. Cleanup pass pending.

---

# What's new

## R&C: A Crack in Time pipeline (experimental, new this round)

ACiT was previously locked in the wizard. This round opens it up — the game's V2 layout (it ships `assetlookup.dat`) lets it ride on the R2/R3 parser, but it has enough engine-level differences to need dedicated handling. Net result: levels open and render with geometry + ties + ufrags + most textures.

- **Game card unlocked.** Wizard adds R&C: A Crack in Time to the Ratchet & Clank tab with the franchise art. R&C: Full Frontal Assault is now `supported: false` (locked with the "Not yet supported" overlay) until its V2 path is re-validated end-to-end.
- **Logic-only mobys handled gracefully.** ~1 in 175 mobys (e.g. trigger / audio / spawn entities) ship without `0xE100`/`0xE200` index/vertex sections. Previously the cache extraction aborted with `section 0xE100 not found`; now those mobys emit empty meshes and the rest of the level extracts. Verified `total=115 with_geometry=114 logic_only=1` on `acid_refinery_pre`, `total=175 with_geometry=175 logic_only=0` on `nefarious_station`.
- **`highmips.dat` optional.** Vanilla ACiT levels (e.g. `nefarious_station/built/levels/`) don't ship `highmips.dat` — only `textures.dat`. Previously the cache aborted with `io: The system cannot find the file specified`. Now every texture routes through `textures.dat` at half resolution (the meta's `width`/`height` describe the top mip in `highmips.dat`; the base-mip chain in `textures.dat` starts at `width/2 × height/2`). Verified by byte-math: `DXT1 512² mip chain starting at 256² = 32768 + 8192 + 2048 + 512 + 128 + 32 + 8 = 43688 bytes` — matches the raw length exactly. All 1410 → 1524 needed textures decode cleanly.
- **`textures.dat` per-texture fallback.** When a high-mip pointer in `0x1D1C0` has `length == 0` (5 textures on `acid_refinery_pre`), the per-texture loop now looks up the same tuid in `0x1D180` (base-mip table) and reads from `textures.dat` — recovers all 5 with the same half-res mip-chain interpretation.
- **Tie shader_index offset fix.** ACiT ties' per-mesh struct stores `shader_index` at `+0x0C`, not V2's `+0x28`. Without the fix, every tie mesh resolved to no shader (V2's `+0x28` reads `0xFFFF` sentinel in ACiT) → all 96 ties rendered gray. Fix is a try-V2-then-ACiT cascade keyed on whether the read value is `< shader_count`: R2 / R3 / RCF / A4O always hit the V2 fast path, ACiT falls through and recovers. Verified across three ties — `+0x0C` reads `{0, 1, 3}` (in `[0, count)` range) where `+0x28` reads `{65535, 65535, 65535}`.
- **Diagnostic probe stack** (all gated on `RECHIMERA_LOG_PROBES=1`): `[acit-probe]` dumps unknown moby sections; `[tex-fallback]` lists base-mip table layout + first 8 truly-missing texture IDs with bottom-32 cross-check against every table tuid; `[tex-drop]` categorises every dropped texture (zero-length / recovered-from-textures-dat / needed-only-in-base / needed-in-cubemap / needed-in-neither / decode-empty / downsample-empty / encode-empty); `[tie-mesh-probe]` dumps first-mesh 0x40 bytes with hex + u16 BE for offset hunting; `[shader-probe]` dumps the first 3 shaders' `0x5D00` section for additional texture-slot RE.
- **External-asset gap quantified.** On `acid_refinery_pre`: 36/736 textures are referenced by shaders but absent from `0x1D180` / `0x1D1C0` / `0x1D200` in this level's `assetlookup.dat`. On `nefarious_station`: 86/1610. Cross-check confirms these aren't truncation collisions — no table tuid's bottom-32 matches any missing ID. They live in a sibling `.psarc` (shared art / globals / UI) that needs to be extracted into the same level folder. Engine renders engine-default placeholders for these at runtime; our viewport renders gray.

## R&C: Tools of Destruction pipeline

- **All 142 ties extract** (was failing at indices 81-141 due to an over-allocated `vbuf_size` header field that overruns `vertices.dat:0x9000`). Fixed by pre-walking each tie's per-mesh structs to compute the actual `max_v_local_end`, then slurping only that — same pattern `moby_old.rs` already uses for mobys.
- **Per-axis tie scale applied per-vertex.** Was using raw `i16` positions producing 30 km buildings. Now multiplies `x/y/z` by `scale[0..2]` at decode time, matching ReLunacy `Tie.cs:93-95` and the V2 `tie.rs:220-222` path.
- **Zone reader ported** — TOD's single-zone-per-level "art zone" loads cleanly. 5684 tie instances + 5410 ufrag terrain pieces on `stratus city`. Tie instance section `0x9240` (matrix → TRS decompose). UFrag terrain section `0x6200` (`OldUFrag` 0x80 struct, `indexOffset` is a u16 COUNT not bytes per ReLunacy `Zone.cs:286`). Vertex stride 0x18 `OldUFragVertex`. Identity shader table for ufrags per `Zone.cs:308` — TOD ufrags look up shaders directly in the global `main.dat:0x5000` DB.
- **Pair-frame animation encoding RE'd** for simple anims. TOD stores `animate_spin` and a handful of door/spin/fill anims as `(zero-filler, real-keyframe)` pairs at half the apparent rate. Detection signal: `frame_stride == min_data_size AND num_8bit_tracks == 0`. When matched, we offset `frames_ptr` by one stride, double the stride, halve `num_frames` and `frame_rate`, then decode with the standard IT-style path.
- **Complex animations T-pose** (n8>0). Roughly 95% of TOD's character anims use a per-frame i8 delta-track encoding we haven't RE'd yet — applying the IT decoder produces visible distortion. To avoid shipping broken motion, every TOD anim with `n8>0` skips decode and renders the bind pose. Probe scaffold logs `[tod-anim] T-POSE moby_XXXX 'anim_name' ...`.

## World content (RFOM, from prior round, still shipping)

- **Foliage extraction.** RFOM levels now surface both vegetation systems — `Shrub` (mesh-based bushes and trees) and `Foliage` (branch meshes + sprite quads for grass/leaves). Auto-toggle in the viewport via the **Shrubs** and **Foliage** buttons. Both are included in full-map GLB exports.
- **Details clusters.** Small static debris and props (RFOM-specific) are now surfaced as their own asset type in the hierarchy with a configurable color in Settings → Colors. They render in the viewport and ship in level GLBs.
- **Skybox.** Dome geometry is decoded and emitted as a GLB on extraction; the texture (when found) renders as a viewport background. Future investigation needed on the descriptor's texture-ID offset, but for most levels the sky shows up correctly.
- **Full-map GLB export upgraded.** Exports now include mobys, ties, details, shrubs, foliage, terrain (UFrags), and the skybox dome in one file — drag the result straight into Godot, Blender, or Unreal.

## Animations

- **Viseme head rigs.** The 14 head rigs in RFOM (soldier, cartwright, Winters, ...) that previously animated frozen or radiated bones from a single point now play their `*_visemes`, `*_expressions`, and `*_blink` clips correctly. Fix: byte-swap-plus-5-bit-mask on the skeleton's translation/scale shift fields (matches what IT's effective behaviour ends up being on x86).
- **Skinning works in Godot.** The full weapon suite (carbine, magnum, auger, bullseye, sawgun, shotgun, sniper, sharpshooter, medicator, minigun, rocket / grenade launchers, uranium cannon) drives the mesh end-to-end through `AnimationPlayer`. Soldier walk / idle / fire / reload / get-hit clips bind cleanly.

## Sound

- **Sound tab now categorised.** SFX / Dialog / Music sub-tabs above the playlist, with counts per category. Classification by source filename — works for RFOM and the V2 games (R2 / R3 / RCF / A4O). Click the sub-tab to filter, search across the filtered set, batch-export only what you need.
- **Dialogue / music stream extraction.** Both bank sounds (SCREAM banks → WAV) and stream sounds (VPK / VAGp / XVAG → WAV) are decoded across all supported games. Cached as WAVs so playback in the modal is instant on second open.

## Cache pipeline / UX

- **Texture cache split into 3 phases.** Materials (albedo) → Normal maps → Textures (other). Each unique PNG is written exactly once, deduped via a `written` set. The cache-build modal shows three meaningful progress bars instead of one monolithic one. Works for V2 / RFOM / TOD uniformly. Backend event `CacheEvent::Phase { phase: "materials" | "normalmaps" | "textures", total: N }`.
- **Real mobys / ties progress counts.** Cache modal no longer shows `123/1` placeholder during the mobys/ties phases. Added `_with_total` variants of every streaming reader (TOD + RFOM moby/tie) that fire an `on_total` callback with the section count upfront so the progress bar fills from real values.
- **Toolbar info expanded.** Status line shows `N mobys · N ties · N terrain · N materials · N textures · N anims`. Materials count is the number of distinct `(albedo, normal, emissive)` shader triples (matches what `Viewport.tsx::getMaterial` keys by). Anims count sums `embedded_animation_count` across all moby assets.
- **Collision viewport toggle removed.** The "Collision (wireframe)" button never had a backend reader and rendered an empty group. Pulled from both the View menu and the floating toolbar across all games until a real `collision.dat` decoder lands.
- **`RECHIMERA_SKIP_TEXTURES` env var fixed.** Was treating any value as "skip" (including `0`). Now correctly off for `0`, `false`, `no`, or empty — on for any other value. Useful for fast anim/decode iteration without re-encoding 1500 PNGs each run.

## Wizard / UX

- **Franchise tab strip** at the top of the game picker — switch between Resistance and Ratchet & Clank with a single click instead of scrolling stacked sections. Your last choice is remembered.
- **Cleaner default output.** Per-record byte dumps and probe scaffolding now stay quiet unless you opt in with `RECHIMERA_LOG_PROBES=1`. Default extraction output is ~10 summary lines plus any `warn:` lines for real problems.
- **Multi-moby debug filter.** Set `RECHIMERA_DEBUG_MOBY=0212,00CD,0326` to extract only those mobys — skips ties / details / ufrags / sky entirely. Useful for tight iteration when investigating a specific asset; ~seconds per re-extract vs minutes for a full level.

## Documentation

- **Byte-level annotations** in every parser module (`skybox_rfom`, `shrub_rfom`, `foliage_rfom`, `gameplay_rfom`, ...). Each struct field's offset, type, and meaning is documented inline with hex offsets — so the next person porting from IT or reverse-engineering on top of this can audit the bytes without re-reading the C++ source.
- **Methodology doc + skill.** The "probe → log → re-extract → range-check → lock" loop we've been running is codified in `docs/internal/lunalib-and-IT/09-debugging-methodology.md` and mirrored into the `/insomnia-toolset` and `/relunacy` skills for future contributors.

## Export pipeline

- **FBX export pulled from the UI.** The frontend "Export FBX (beta)" path and the per-format picker that exposed it are gone — every export menu now goes straight to GLB. The Rust-side FBX writers (`crates/lunalib/src/fbx_export.rs`, `fbx_binary_export.rs`, `fbx_node.rs`, ~3.3 KLoC) landed in the same branch and stay in the workspace as scaffolding, but the Tauri commands are no longer wired into any button. The path will come back once the rotation curves + material round-trip have been validated against Blender / Unreal / Maya end-to-end; until then GLB is the single supported interchange format.
- **Map export simplified.** The viewport's "Export map" toolbar button is now a plain GLB action; the split-button picker that let you choose between formats was removed alongside the FBX wiring.

## Release infrastructure & tooling

- **Dual-channel release pipeline.** `.github/workflows/release.yml` now drives two release channels off two branches:
  - **`main` → stable.** Same gated flow as before — only builds when the version in `package.json` has been bumped and no matching `v$VERSION` release exists yet. Updater endpoint stays at `releases/latest/download/latest.json`.
  - **`develop` → canary.** Every push produces a pre-release tagged `canary-v<base>-canary.<sha7>` with all four platform bundles. A rolling `canary-latest` release holds only the updater manifest so installed canary apps poll a fixed URL while the per-commit releases preserve history.
- **Canary apps install side-by-side with stable.** Different bundle identifier (`dev.rechimera.desktop.canary`), different product name (`ReChimera Canary`), different default brand color (yellow `#fcd202` vs the stable red `#FF6363`), and a dedicated icon (`apps/desktop/icon_canary.png`) regenerated through `tauri icon` at build time. Each app has its own settings store, so the two installs can coexist without stepping on each other.
- **`bun run version:canary` for local canary builds.** New flag in `scripts/sync-version.mjs --canary [--suffix=<value>]` mirrors what CI does — patches `tauri.conf.json` (version / productName / identifier / updater endpoint), flips the default `brandColor` in `store.ts`, and regenerates the bundled icon set from `icon_canary.png`. Useful for testing a canary bundle locally before pushing to `develop`. SHA suffix auto-detects from `git rev-parse --short HEAD`; the `--suffix=` escape hatch accepts arbitrary SemVer pre-release identifiers.
- **Same signing key, two channels.** Updater signatures verify in both apps; the channel separation comes from the bundle identifier and the endpoint URL, not from key separation. GitHub's `releases/latest/` magic skips prereleases, so canary publishes can never poison the stable channel.

# Per-game changes this release

## Resistance: Fall of Man (the heaviest update this round)

- **Foliage extraction.** Both `Shrub` (mesh-based bushes/trees) and `Foliage` (branch meshes + sprite quads for grass/leaves) now extract, render in the viewport behind their own toggles, and ship in full-map GLBs.
- **Details clusters** surface as a dedicated asset type — small static debris and props get their own hierarchy entry and a configurable Settings color.
-  **Skybox** — dome geometry decoded and emitted as a GLB; the texture (when found) renders as the viewport background. Cleaner than the previous "no sky" state.
- **Viseme rigs fixed** — the 14 head rigs (soldier, cartwright, Winters, ...) that previously animated frozen or radiated bones from one point now play their `*_visemes`, `*_expressions`, `*_blink` clips correctly. Root cause was a u16 byte-order quirk on `Skeleton.translationShift`; fix is documented in `project_skeleton_shift_byte_quirk` memory.
- **Gameplay placements** — `ps3gameplay.dat` is parsed and 184-ish moby placements feed the viewport / Godot. Probing scaffold added for the 6 unidentified sub-arrays (triggers / volumes / spawns) — set `RECHIMERA_LOG_PROBES=1` to dump them and help reverse-engineer.
- **Full-map GLB export** — now bakes everything (mobys, ties, details, shrubs, foliage, terrain, sky) into a single drag-into-Godot file.

## R&C: A Crack in Time (newly unlocked, experimental)

- **Wizard card unlocked.** Tile is selectable, opens through the same V2 path as R2/R3 with ACiT-specific fallbacks documented above. Logo: `R&Clank_A_Crack_in_Time.png`. Marked experimental in the byline because the 86-texture external-psarc gap and the shader-additional-slots question remain open.
- **`acid_refinery_pre` (mod, with `highmips.dat`) verified end-to-end** — 114/115 mobys with geometry, 86 ties (all resolving shaders correctly after the `+0x0C` fix), 700/736 textures, ufrags loading, animset library populated.
- **`nefarious_station` (vanilla, no `highmips.dat`) verified end-to-end** — 175/175 mobys with geometry, 96 ties (all resolving), 1524/1610 textures via the `textures.dat` half-res fallback, ufrags loading.
- **Sibling-psarc hint to be added to wizard** — the 36–86 truly-missing textures per level are shared assets. Users with vanilla extracts should pull `common.psarc` / `globals.psarc` / `ratchet.psarc` (whatever is in the parent of the level folder) into the same directory before extraction. The card hint will be updated to mention this in the next round.

## Resistance 2 / Resistance 3 / R&C: Full Frontal Assault / R&C: All 4 One

- **Sound tab categorised** — SFX / Dialog / Music sub-tabs with counts. Source-filename classifier works the same across all V2 games.
-  **Wizard franchise tabs** — top-of-step-1 tab strip groups Resistance vs Ratchet & Clank with persisted state.
- **Materials / Normalmaps / Textures phase split** in the cache build modal. Texture extraction is split into three sequential passes (albedos → normals → other) so the progress UI shows three meaningful bars instead of one monolithic one. Each unique PNG is written exactly once, deduped via a `written` set. Works uniformly for V2 / RFOM / TOD.
- **Real mobys / ties progress counts.** Cache build modal no longer shows `123/1` placeholder during the mobys/ties phases — added `_with_total` variants of every streaming reader (TOD + RFOM moby/tie) that emit the section count upfront so the bar fills from real values.
- **Toolbar info expanded.** Status line now shows `N mobys · N ties · N terrain · N materials · N textures · N anims` — added distinct `(albedo, normal, emissive)` shader-triple count and total embedded animation clip count.
-  **`RECHIMERA_SKIP_TEXTURES` env var fixed.** Was treating any value as on (including `0`). Now correctly off for `0`, `false`, `no`, or empty — on for any other value. Useful for fast anim/decode iteration without re-encoding 1500 PNGs each run.
-  **Internal methodology consolidated** — same parser robustness improvements benefit the V2 path; nothing game-specific to flag.

## R&C: Tools of Destruction (the main focus this round)

-  **Tie pipeline fixed end-to-end.** All 142 ties extract cleanly. Previously the last 61 (indices 81–141) failed with `io: failed to fill whole buffer` because the on-disk `vbuf_size` header field is wildly over-allocated (for many ties it claims 8–20 MB of vertex data when the actual per-tie data is ~100 KB). The fix walks the per-mesh structs first to compute the real `max_v_local_end`, then slurps only that much — same pattern `moby_old.rs` already uses for mobys. Also added per-vertex `* scale.{x,y,z}` multiplication that was missing (positions used to come out at i16 raw scale = 30 km tall buildings; ReLunacy `Tie.cs:93-95` applies the per-axis scale at decode time, we now do the same).
-  **Zone reader ported.** TOD's single-zone-per-level art zone now loads — `5684 tie instances + 3613 ufrag terrain pieces` on stratus city. Ported from ReLunacy `Zone.cs::CZone(isOld)`. Tie instance section `0x9240` (matrix-row-major transform → decomposed TRS, tie key as `main.dat` byte offset). UFrag terrain at `0x6200` (`OldUFrag` 0x80 record per ReLunacy `Zone.cs:77-111`; `indexOffset` is a u16 COUNT not bytes per `Zone.cs:286`, we multiply by 2 when seeking). Vertices stride 0x18 `OldUFragVertex`. Identity shader table for ufrags per `Zone.cs:308` — TOD ufrags lookup shaders directly in the global `main.dat:0x5000` DB.
- **Animation pair-frame encoding RE'd (partial).** Simple TOD anims (`animate_spin`, doors, fills, `kerchuu_roller_roll`) store keyframes as `(zero-filler, real-data)` pairs at half the apparent rate — even-index frames are zero padding, real keyframes at odd indices. Signal: `frame_stride == min_data_size AND num_8bit_tracks == 0`. When those conditions match, we offset `frames_ptr` by one stride, double the stride, halve `num_frames` and `frame_rate`, then run the standard IT-style decoder.
- **Complex animations (n8>0) ship as T-pose.** ~95% of TOD's character anims (wasp, beetle, pterodactyl, ratchet, all idles/walks/attacks) use a per-frame i8 delta-track encoding we haven't RE'd yet. Applying the IT-style decoder produces visible distortion (bones radiating from origin, mesh stretched into wing/blade shapes). Rather than ship a mix of working + broken motion, **every TOD anim now skips decode and renders bind pose**. Probe scaffold at `[tod-anim] T-POSE moby_XXXX ...` shows which anims are affected.
-  **Texture loading restored to async streaming.** Fixed a regression where awaiting all 1422 textures before mounting meshes made level open feel slow. Reverted to fire-and-forget: meshes appear instantly, textures stream in over a couple seconds while three.js's material cache picks up late-arriving texture references via `m.map = tex; m.needsUpdate = true`. The race we originally awaited for is now handled at the material-cache layer.
- **Collision viewport toggle removed.** The "Collision (wireframe)" button never had a backend reader and only rendered an empty group — pulled from the View menu and the floating toolbar across all games until a real `collision.dat` decoder lands.

# Coming next

- **TOD complex animations (n8>0).** Pair-frame encoding for simple anims is RE'd this round — `animate_spin` and a handful of door/spin/fill anims play correctly. The i8-delta encoding used by ~95% of TOD content (every character idle, attack, walk) remains unsolved. Next probe target: byte-level audit of the i8 region for a known-correct anim like `beetle_idle` to crack the encoding. Probe scaffold lives in `cache.rs::probe_anim_bytes` + `[tod-trans-probe]`.
- **TOD `collision.dat`.** Separate file in every TOD level folder — not opened anywhere yet. Format unknown to us, no IT/ReLunacy reference. Adding a parser would unlock Godot/Unity import with real collision volumes rather than mesh-derived approximations.
- **TOD lighting data.** TOD definitely has dynamic lighting in-game; the section IDs that hold light placements aren't identified. RFOM has its own (different) lighting sections and ReLunacy doesn't cover this for TOD.
- **TOD skybox.** No working reference exists — IT has no TOD decoder, and every ReLunacy branch we audited (`master`, `dev`, `bliss`, `bliss-old-loader`) lacks one (only an unused stub in `dev`). TOD levels currently render with no sky. Would need raw-byte probing of `main.dat` for the dome / texture descriptor sections; no candidate section IDs identified yet.
- **RFOM lights and gameplay sub-arrays.** If anyone wants to help, see "Help wanted" — both are reverse-engineering tasks gated on raw byte probes from real levels.
- **ACiT shared-psarc texture hint in wizard.** The ACiT card needs a "extract any sibling `.psarc` (globals / common / art) into the same folder for full texture coverage" note added to its `hint` string.
- **ACiT shader additional texture slots.** Current `parse_shader` reads 3 u32s from `0x5D00 + 0x10` (albedo / normal / expensive). ACiT shaders may have more slots (detail / lightmap / specular / decal) at higher offsets — the `[shader-probe]` dump in this round captures the data needed to RE them and adds 0–N more texture refs to the resolution path.
- **ACiT animations audit.** Verified mobys carry skeletons (`[skel-shift]` lines show recovered shifts) but the V2 animset decoder hasn't been tested against ACiT animsets — needs a clip-by-clip check on a known-animated moby.
- **R&C Full Frontal Assault re-validation.** Locked in the wizard this round; needs a fresh test pass on real FFA level data to confirm whether the V2 path still handles it cleanly or whether it needs FFA-specific gates (its texture format bytes `0x81..0x8B` + `0xA6` are still in `TexFormat::from_byte`, so the decode side should be intact).

# Game support

| Game | Status |
| --- | --- |
| Resistance 2 | stable end-to-end |
| Resistance 3 | stable end-to-end |
| Resistance: Fall of Man | full pipeline + foliage + skybox + visemes + sound (extract `game.psarc` first) |
| R&C: All 4 One | all features working |
| R&C: Full Frontal Assault | **locked in wizard** this round — needs a re-validation pass against current V2 pipeline before re-enabling |
| R&C: Tools of Destruction | meshes / textures / skeletons / ties / terrain / tie placements all load. Animations export T-pose (pair-frame encoding RE'd for simple anims, complex i8-delta encoding unsolved — see Coming next). No skybox (no IT/ReLunacy reference exists). |
| R&C: A Crack in Time | **newly unlocked, experimental** — meshes / skeletons / ties / ufrags / animsets all load on both vanilla and mod-extracted levels. Textures ~95% (the rest live in a shared sibling `.psarc` not in the level folder). Animations untested. |

# Known limitations

- **Light placements (RFOM)** — no light section identified yet. The previously-rendered "lights" at world origin were a mislabel (the section turned out to be `Foliage`); we've removed the bogus render. Real lights await a future probe pass.
- **Trigger / volume / spawn placements** — IT doesn't decode RFOM's `GameplayInstances.other[6]` arrays, so they remain opaque. Set `RECHIMERA_LOG_PROBES=1` and re-extract to dump the raw bytes if you want to help reverse-engineer them.
- **Collision geometry** — no IT reference exists for RFOM collision, and TOD's `collision.dat` isn't parsed yet either. Godot users can auto-generate collision from the GLB mesh geometry on import as a workaround. The viewport "Collision" toggle was removed this round since it had no backend.
- **TOD complex animations** — i8 delta-track encoding (n8>0) remains unsolved; ~95% of TOD anims export as T-pose. Simple anims (n8=0, pair-frame encoded) play correctly. See Coming next.
- **TOD skybox** — not extracted. No reference in any audited ReLunacy branch (`master` / `dev` / `bliss` / `bliss-old-loader`) and no IT support. TOD levels render without a sky background; the cache produces no `skybox/` folder.
- **ACiT external textures** — ~5% of shader-referenced texture IDs (36 on `acid_refinery_pre`, 86 on `nefarious_station`) aren't in the level's own `assetlookup.dat`. These are shared assets in a sibling `.psarc` that needs to be extracted alongside the level (e.g. `common.psarc` / `globals.psarc`). Engine renders default placeholders in-game; our viewport renders gray for these meshes.
- **ACiT shader extra slots** — `parse_shader` reads only albedo / normal / expensive at `+0x10..+0x18`. If ACiT shaders carry more texture references (detail / lightmap / specular at later offsets), they aren't applied yet. Probe scaffold is in place — see Coming next.
- **R&C: Full Frontal Assault locked in wizard** — pending re-validation; previously listed as "a few textures still missing" but not retested against the current V2 pipeline.
- **FBX export not available in this build.** The "Export FBX (beta)" option is intentionally removed from the export menus while we validate the rotation / material / skinning round-trip in Blender / Unreal / Maya. Use GLB in the meantime — it carries the same content (mesh + materials + embedded textures + skinning + animation) and lands cleanly in every major DCC tool we test against.

# Install

Pick the bundle below for your OS. Windows users with the previous build installed will see an **Update** button in the title bar — click it to install in place. macOS / Linux still need a manual reinstall from this page.

**Want pre-release builds?** This release also introduces a separate canary channel published from the `develop` branch. The canary build installs side-by-side with stable (different icon, different identifier, yellow brand color so you can tell them apart at a glance), auto-updates on every commit landed in `develop`, and is intended for testing in-flight work before it ships to stable. Grab the latest canary from the [`canary-latest`](../../releases/tag/canary-latest) release page — the bundles themselves live in the per-commit canary releases the manifest points at. **Expect breakage on canary**; file issues with the full version string (e.g. `0.3.5-canary.a1b2c3d`) so we can pin which build broke.

# Help wanted

- **Testers per game.** Even just running the tool on a level you own and confirming it works is genuinely helpful. If something breaks (placements off, textures missing, foliage in the wrong spot, animations not driving the mesh, exports failing) let me know which game + which level so I can reproduce. Logs from the bottom panel are gold — especially anything starting with `warn:`.
- **Real lights / triggers data.** If you're comfortable poking at hex, set `RECHIMERA_LOG_PROBES=1` and re-extract a level — share the `[rfom-gp-other]` slot dumps so we can pattern-match the unidentified gameplay sub-arrays.
- **GIFs / screenshots / Godot integration examples.** Visual examples for the README and releases page. Short clips of a MAP loading, an asset preview, or an export-to-Blender / -Godot flow on any of the games are very welcome — I'll credit you and use them in the docs.

# Bugs / requests

Open a GitHub issue if you can reproduce something, otherwise feel free to DM. I'll answer in my free time.

# Acknowledgements

- [@NefariousTechSupport](https://github.com/NefariousTechSupport) — original Lunacy and the [7thigRewrite](https://github.com/NefariousTechSupport/7thigRewrite) architecture this project draws from.
- [@VELD-Dev](https://github.com/VELD-Dev) — [ReLunacy / LibLunacy](https://github.com/VELD-Dev/ReLunacy) (the C# predecessor that the moby / tie / texture decode paths port from).
- [@PredatorCZ](https://github.com/PredatorCZ) — [InsomniaToolset / Spike](https://github.com/PredatorCZ/InsomniaToolset) (canonical reference for IGHW container, RFOM `levelmain` extract, foliage / shrub / animation decode, and the V2 GLTF emit pipeline).

Thanks to anyone willing to use, test, or try the tool — every game / level you run through it helps validate the parsers work outside my own small sample.
