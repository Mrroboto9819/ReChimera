# ReChimera — Project Memory

> Living document. Read this first if you're picking up the project after a break, or starting a new Claude session. Every section below was earned by debugging; nothing here is hypothetical.

---

## 1. What ReChimera is

A **desktop app** (Rust + Tauri 2 + React + three.js) that opens Insomniac Games PS3-era levels and renders them in real time. The goal is a viewer / inspector / exporter for game art that the original developer tools never shipped a Windows version of.

**Supported games** (status as of 2026-05-22):

| Game | Layout | Status |
|---|---|---|
| Resistance: Fall of Man (RFOM) | `ps3levelmain.dat` | Mesh / textures / skeletons OK. Animations + gameplay placements + UFrag pending. |
| Resistance 2 (R2) | V2 PSARC (`assetlookup.dat`) | **Primary focus.** Stable end-to-end: meshes, textures, materials, skeletons, animations, ufrags, sounds. |
| Resistance 3 (R3) | V2 PSARC | Stable, same parser as R2. |
| Ratchet & Clank: Tools of Destruction (TOD) | `main.dat` (no assetlookup) | Meshes, textures, skeletons, ufrags load. Animations decode via pair-frame fix. No cubemap. |
| Ratchet & Clank: A Crack in Time (ACiT) | V2 PSARC | Experimental — routes through R2/R3 parser. Some assets may be wrong. |
| Ratchet & Clank: Full Frontal Assault (FFA) | V2 PSARC | Not validated yet. |
| Ratchet & Clank: All 4 One (A4O) | V2 PSARC | Working (meshes / textures / materials / skeletons / animations / sounds). |

**Parent project lineage:** ReChimera ports logic from two reference codebases:
- **InsomniaToolset (IT)** — C++ CLI at `D:\mods\tools\InsomniaToolset-master\`. The canonical reference for **V2 (R2/R3/A4O/ACiT)** and **RFOM**. No TOD support.
- **ReLunacy** — C# project (multiple branches: master/dev/bliss/bliss-old-loader). The canonical reference for **TOD**. No RFOM support.

When porting any new format, **read IT (or ReLunacy for TOD) first** and cite `file:line` in chat. Don't implement from probe data alone.

---

## 2. Architecture

```
ReChimera/
├── apps/desktop/
│   ├── src/                     React + Vite frontend
│   │   ├── App.tsx              Top-level state machine, level loading orchestration
│   │   ├── api.ts               Typed wrappers around every Tauri command + Channel<T> events
│   │   ├── components/
│   │   │   ├── OpenLevelModal.tsx    Generic game-picker + source-step + folder/PSARC wizard
│   │   │   ├── R2Wizard.tsx          R2-specific replacement (USRDIR → globals → maps → prepare → render)
│   │   │   └── ...
│   │   └── views/
│   │       ├── Viewport.tsx          three.js scene
│   │       ├── GlbPreview.tsx        Per-asset preview (animation playback lives here)
│   │       └── ...
│   └── src-tauri/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs          Tauri entrypoint, command handler registration
│           ├── cache.rs         3000+ line decode pipeline (the heart of the app)
│           └── r2.rs            R2 wizard commands (USRDIR check, list maps, extract, prepare)
├── crates/
│   ├── lunalib/                 The Rust port of IT's reading logic
│   │   └── src/
│   │       ├── animation.rs     AnimationHeader + decode_animation* (track-mask aware)
│   │       ├── skeleton.rs      Bones, bind-pose, shift-byte recovery
│   │       ├── moby.rs          MobyV2, primitive parsing, skinned-vertex decode
│   │       ├── moby_old.rs      RFOM MobyV1
│   │       ├── moby_rfom.rs     RFOM-specific moby wiring
│   │       ├── texture.rs       NV4097 format decoder (R8/RGBA8/BC1/BC2/BC3/etc.)
│   │       ├── texture_global.rs    Global PSARC texture fallback loader
│   │       ├── shader.rs        Material → texture binding
│   │       ├── tie.rs           Static-prop "ties"
│   │       ├── zone.rs / region_rfom.rs    Zone/region level geometry
│   │       ├── gameplay.rs / gameplay_old.rs / gameplay_rfom.rs    Instance placements
│   │       ├── igfile.rs        IGHW container reader (TOC + fixups + sections)
│   │       └── stream.rs        Big-endian PS3 stream helper
│   └── psarc/                   Sony PSARC archive reader (Zlib / LZMA / Oodle compression markers)
├── docs/
│   ├── internal/lunalib-and-IT/    Deep-dive notes per subsystem
│   └── ...
└── memory/                      (under ~/.claude/projects/.../memory/ — auto-loaded)
    ├── MEMORY.md                Index of memory entries (loaded into every Claude session)
    └── project_*.md             Individual deep-dive memories
```

**Data flow** (R2 example):
1. User picks USRDIR via `R2Wizard` → backend probes `packed/game/global_*.psarc` presence
2. Wizard extracts globals (one-time per install) → `packed/game/global_*/built/tuids/...`
3. Wizard lists maps from `packed/levels/*/` → categorized (Campaign / Coop / Multiplayer / Lobby / Other)
4. User picks a map → wizard extracts `level_cached.psarc` + `level_uncached.psarc` → `packed/levels/<map>/built/levels/<map>/{assetlookup,mobys,ties,shaders,textures,highmips,animsets,zones,...}.dat`
5. Wizard auto-runs `extract_level_to_cache` → decodes everything to `_rechimera_cache/` (JSON manifests + PNG textures + binary geometry blobs)
6. Wizard hands off to `App.tsx::handleOpen(folder, { skipCachePrompt: true })` → three.js streams meshes in
7. Texture gaps fall back to the global-PSARC tree via `lunalib::texture_global::discover_and_index`

---

## 3. Critical invariants (DO NOT regress)

These are reverse-engineered from the binary format. Each one was a multi-hour debug session. Keep them in code, in docs, and in [memory](memory/MEMORY.md).

### 3.1 Animation frame_stride padding (`AnimationFlag::PackedFrames`)
- **Source:** IT's `serialize.cpp:660` — `if (!item.flags[AnimationFlag::PackedFrames]) { item.frameStride += GetPadding(item.frameStride, 0x80); }`
- **What:** Non-packed animation clips (flag bit 0x04 = 0) have on-disk `frameStride` rounded UP to a 128-byte boundary before any frame data is indexed.
- **Symptom when missing:** "Bones move but mesh detaches from rig" or "mesh disappears" on long character clips. First frame mostly correct, every subsequent frame reads from a misaligned offset, decode error compounds.
- **Fix:** `AnimationHeader::apply_frame_stride_padding()` in `crates/lunalib/src/animation.rs`. Called at every V2 decode site in `apps/desktop/src-tauri/src/cache.rs` (`decode_clips_for_moby` and `decode_animset_clip`).
- **Do NOT apply in TOD path** — TOD has its own pair-frame encoding fix that would conflict.

### 3.2 Additive animation `numBones` override
- **Source:** IT's `gltf_shared.cpp:561-563` — additive clips lie about `numBones` in their header; IT overwrites it with the skeleton's max bone count before reading the control blob.
- **What:** Animation clips with flag bit 0x02 (`Additive`) need `header.num_bones = skeleton.bones.len()` applied before `read_animation_control`.
- **Symptom when missing:** Additive overlay clips (`mp_carbine_idle_p`, every `*_p` weapon overlay) silently lose all track data and stay at rest pose. `WARN: track masks reference bone_index >= skel_bones` in logs.
- **Fix:** Applied at the same call sites as 3.1.

### 3.3 Skeleton shift bytes (PS3 BE over-swap)
- **Source:** IT's `FByteswapper<Skeleton>` (`serialize.cpp:186`) deliberately skips `scaleShift` and `translationShift` u16s.
- **What:** On our PS3 BE reader, those two u16s get over-swapped. Real values are always 0-15; anything > 15 means we read swapped bytes.
- **Symptom when missing:** "Bones collapse to origin" — animation translations are scaled with wrong power-of-2.
- **Fix:** `recover_shift` in `crates/lunalib/src/skeleton.rs`. Detects `raw > 15` → `.swap_bytes()` to recover.

### 3.4 Texture format dual range
- **Source:** Cross-referenced from IT's NV4097 format byte values + observed mod content.
- **What:** `TexFormat::from_byte` must support BOTH ranges:
  - R2/V2: 0x03..0x0A (R8, RGBA8, BC1, BC2, BC3, etc.)
  - FFA/A4O/some mods: 0x81..0x8B and 0xA6 (alternative encoding for the same formats)
- **Never collapse them into one range.** Both ship in production files.
- **Fix:** `crates/lunalib/src/texture.rs::TexFormat::from_byte`.

### 3.5 Root-bone self-reference
- **Source:** IT skeleton structure convention.
- **What:** Insomniac skeletons mark roots with `parent_index == own_index`, NOT `-1`.
- **Symptom when missing:** Three.js's `GLTFLoader` stack-overflows because the parent walk loops forever.
- **Fix:** Skeleton walkers must treat self-reference as root.

### 3.6 TOD pair-frame animation encoding
- **What:** TOD packs each logical keyframe as TWO consecutive `frame_stride` rows — even-index = zero filler, odd-index = real values.
- **Decode:** Halve `num_frames`, double `frame_stride`, offset `frames_ptr` by `+frame_stride`, then use the standard IT decoder.
- **Source:** Reverse-engineered from `animate_spin` byte dump showing linear `bone1.y = 0,18,36,54,72,91,109,127` across odd frames.
- **Fix:** In TOD-specific code path of `cache.rs` (~line 419+). **Do not combine with 3.1.**

### 3.7 Global-PSARC texture fallback (+ cross-level)
- **What:** R2 (and presumably R3 / ACiT) stores shared art in **`packed/game/global_cached/built/tuids/<bucket>/<tuid>/{header,texel}.dat`** — NOT in the level's own `textures.dat` / `highmips.dat`. NB: R2's `global_uncached.psarc` is **dialogue-only** (streaming WAVs + a `.toc` stub), it does NOT contain texture content despite the name.
- **Two-layer fallback in `cache.rs` (V2 path):**
  1. `lunalib::texture_global::discover_and_index` → `global_cached/built/tuids/` lookup keyed by `tuid & 0xFFFFFFFF`.
  2. `lunalib::texture_global::find_sibling_extracted_levels` → re-runs `bulk_extract_pngs` against every other extracted level under `packed/levels/*/built/levels/*/` for IDs still missing. Catches lobby-UI / coop-shared / weapon-variant references that don't live in globals.
- **Cache implication:** The fallback only runs during cache *build*. After extracting globals OR after extracting another sibling level, the current level's cache must be rebuilt. The R2 wizard auto-handles the globals case (§5.2); cross-level reuse is opportunistic — extract lobby first to enrich every other level's recovery.
- **HINT gating:** `find_global_tuid_roots` only nags about a missing `built/tuids/` when the variant folder has a `built/` tree but no tuids/ inside it (real partial extraction). A folder with no `built/` at all is treated as expected layout for dialogue-only PSARCs — no nag.

### 3.8 TOD MobyV1 layout
- TOD's `OldMoby` matches IT's RFOM `MobyV1` byte-for-byte: skeleton ptr @+0x20, animations ptr @+0x24, numAnimations @+0x16.
- `main.dat:0xD300` is a single packed buffer holding all skeletons (not one per asset).

### 3.9 FBX header trap (three.js)
- three.js's `FBXLoader::isFbxFormatASCII` reads triangular byte positions (0,1,3,6,...,190) and rejects "Unknown format" if any of them match the binary FBX magic.
- Our `LEADING_COMMENT_BLOCK` in the FBX exporter is hand-tuned to avoid all those positions. **Never edit without re-running** `header_clears_threejs_ascii_check_traps` test.

---

## 4. Active workstreams

### 4.1 R2 wizard (current session, 2026-05-22)
**Goal:** Replace the generic "pick a folder with assetlookup.dat" flow for R2 with a USRDIR-rooted wizard that handles globals + level extraction + cache build all in one continuous progress UI.

**Flow:**
1. User picks R2 from game-picker → `R2Wizard` opens (not the generic `OpenLevelModal` source/PSARC step)
2. **USRDIR step:** User points to `.../PS3_GAME/USRDIR/`. Backend `r2_setup_check` reports `is_usrdir`, presence of each `global_*.psarc`, level folder count
3. **Globals step:** `r2_extract_globals` extracts both global PSARCs in-place into `packed/game/global_*/`. Skip-if-already-extracted is built in. Tracks `globalsForceRebuildRef` if any PSARC was freshly extracted (triggers cache rebuild later)
4. **Maps step:** `r2_list_maps` walks `packed/levels/` and categorizes by R2's naming convention. Wizard shows colored placeholder cards (no real thumbnails yet — see 4.2)
5. **Level / Prepare step:** Auto-flow — user picks a map and:
   - If `m.ready` (already extracted): skip to prepare
   - Else: `r2_extract_level` extracts both level PSARCs
   - Then `prepareAndOpen` runs: checks `cacheStatus`, decides `fresh` / `rebuild` / `reuse`, streams `CacheEvent` from `extract_level_to_cache` or `reextract_level_cache`
6. **Handoff:** `onOpen(path, { skipCachePrompt: true })` → `App.tsx::handleOpen` skips the "use cache vs rebuild" prompt (since wizard just built it) and goes straight to `loadFullMeshes(sum, "use-cache")`

**Backend commands** (`apps/desktop/src-tauri/src/r2.rs`):
- `r2_setup_check(usrdir)` — folder validation + per-PSARC state
- `r2_list_maps(usrdir)` — categorized map list
- `r2_extract_globals(usrdir, channel)` — both global PSARCs
- `r2_extract_level(usrdir, map_id, channel)` — both level PSARCs
- `r2_level_open_path(usrdir, map_id)` — resolves canonical `built/levels/<map>/` path
- `r2_cache_needs_rebuild(usrdir, map_id)` — compares globals folder mtime vs cache manifest mtime
- `r2_probe_level_thumbnails(usrdir, map_ids)` — exploratory probe (§4.2)

### 4.2 Level thumbnails (deferred / exploratory)
IT has no UI/menu extractor. R2 stores level-select art somewhere in `global_*.psarc` but the location isn't documented. We added `r2_probe_level_thumbnails` to scan the extracted globals folder for image-like files matching map names — run from the maps step's "Probe thumbnails" button. Results inform the next move:
- If files like `screens/levelselect/<map>.tga` show up → wire as `src="file://..."` images
- If only `built/tuids/` exists → R2 stores them TUID-keyed, requires asset-bank decoding
- If `.swf` shows up → Anark/Flash atlas, much bigger lift

### 4.3 Outstanding R2 bugs (open at the start of this session)
- `adv_hybrid` (TUID `0x5ED37B1C9C403839`, 143 bones) — clip `hyb_death_crouch_sighted_f` had "bones move but mesh detached" symptom → **Fixed via 3.1 (frame_stride padding)** this session
- `trex` (TUID `0x9D06A1D00494464D`, 98 bones) — many broken animations → same fix
- `minigun_r2` (TUID `0x1DE9F35E04C16889`, 21 bones) — missing textures → **Fixed via §5.2 (auto-rebuild)** this session

### 4.4 Non-R2 outstanding work
- TOD: skeleton shift recovery is in place but disabled (`recover_shift` is implemented but the deeper TOD anim format took priority; revisit now that the anim format is solved)
- RFOM: animations + gameplay placements + UFrag still pending. IT's `levelmain/extract.cpp` (1453 lines) is the canonical reference.
- Skybox V2 not investigated (RFOM sky is ported, TOD has no decoder anywhere).

---

## 5. Recent changes — this session (2026-05-22)

### 5.1 Frame-stride padding fix (animation correctness)
- Added `AnimationHeader::apply_frame_stride_padding()` in `crates/lunalib/src/animation.rs`
- Wired into V2 decode sites in `cache.rs` (`decode_clips_for_moby` and `decode_animset_clip`)
- TOD path deliberately untouched
- **Result:** adv_hybrid / trex broken clips now decode correctly

### 5.2 R2 wizard auto-rebuild
- Added `r2_cache_needs_rebuild` backend command — compares globals folder mtime vs cache manifest mtime
- Added `cacheStatus.stale` and `cacheStatus.incomplete` checks to wizard's `prepareAndOpen`
- Wizard now always builds (fresh) or rebuilds cache when level is in a state that requires it
- `App.tsx::handleOpen` accepts `{ skipCachePrompt: true }` from wizard to skip the "use cache?" dialog
- **Result:** Going through wizard auto-handles cache lifecycle end-to-end; no manual "Open" click; no cache prompt redundancy

### 5.3 Brand-color theming
- Active franchise tab in game-picker uses `--brand-color` (red `#FF6363`) instead of hardcoded blue
- Uses `color-mix(in srgb, var(--brand-color) X%, transparent)` so it follows any future brand-color change

### 5.4 R2 wizard backend foundation
- New module `apps/desktop/src-tauri/src/r2.rs` with all the R2-specific commands listed in §4.1
- New frontend component `apps/desktop/src/components/R2Wizard.tsx`
- All wired into `OpenLevelModal` so R2 game tile routes to the wizard

### 5.5 Memory entries added
- `memory/project_anim_frame_stride_padding.md` — new
- `memory/MEMORY.md` index updated

---

## 5b. Recent changes — session 2026-05-24

### 5b.1 R2 global-PSARC layout clarified (+ false-alarm HINT fix)
- **Finding:** R2's `global_uncached.psarc` (49 MB) ships only `packed/game/global_uncached.toc` + `sound/global/streaming_dialogue.us.dat`. It has **no texture content**. All R2 global-art TUIDs live in `global_cached.psarc` (280 MB, ~210 TUID folders).
- **Fix:** `crates/lunalib/src/texture_global.rs::find_global_tuid_roots` — HINT now requires `parent.join("built").is_dir()` before nagging. A folder with no `built/` is the PSARC's normal post-extraction state, not a missing extraction.
- **Side note:** `packed/game/game.psarc` (971 B) and `debug.psarc` (267 B) are manifest stubs, not asset archives — no need to extract them.

### 5b.2 Cross-level texture fallback
- **Why:** ~65 textures referenced by `scotia_coop` shaders aren't in globals OR the level's own tables. They're cross-level references — typically lobby UI / coop-shared / weapon-variant atlases shipped inside *another* level's PSARC.
- **Fix:** New `lunalib::texture_global::find_sibling_extracted_levels` walks `packed/levels/*/built/levels/*/` for siblings with an `assetlookup.dat`. `cache.rs` V2 path adds a second fallback pass after globals: for each sibling, calls `bulk_extract_pngs` narrowed by the residual missing-ID set; remaining IDs shrink with each hit, so siblings late in the iteration only attempt what nothing earlier provided.
- **Log line:** `[cross-level-tex] total recovered N / M via cross-level fallback (X siblings tried, Y still unrecovered)`.
- **Operational tip:** Extract `lobby` first when working in R2. Its UI/cursor/HUD textures are the most common cross-level dependency, and once extracted any future level cache build can recover them automatically.

### 5b.3 Memory entries to add
- `memory/project_r2_global_psarc_layout.md` — new (R2 globals texture/dialogue split + HINT-gating rule)
- `memory/project_cross_level_texture_fallback.md` — new (sibling-level fallback exists in V2 path)

### 5b.4 GLB exporter binds all three texture maps (was albedo-only)
- **Symptom:** minigun_r2 (`0x1DE9F35E04C16889`, 44 submeshes) rendered fully gray in `GlbPreview` even though the texture `971113027` (= `0x39E16903`, RSX-debugger confirmed) was present in the cache PNG set and the sidecar JSON correctly bound `albedo_id` / `normal_id` / `emissive_id` per submesh.
- **Root cause:** `crates/lunalib/src/gltf_export.rs::build_material` hardcoded `normal_texture: None` and `emissive_texture: None`. Worse, it returned `None` (drop entire material → default-gray fallback) when albedo couldn't be resolved, even if normal/emissive were valid.
- **Fix:** refactored to use a shared `push_or_reuse_texture` helper across all three maps; `build_material` now binds whichever maps the shader has and only returns None if **all three** are missing. `emissive_factor` flips to `[1,1,1]` when an emissive map is bound so it actually contributes.
- **Cache implication:** `.glb` files are baked at cache-build time. To pick up the fix on a previously-built level, wipe `_rechimera_cache/` and re-open.
- **Two consumer code paths use materials differently:**
  - `Viewport.tsx` and `AssetPreview.tsx` patch `MeshStandardMaterial.map/normalMap/emissiveMap` post-load from the JSON sidecar — already correct, didn't need this fix.
  - `GlbPreview.tsx` (per-asset preview with animation dropdown) uses `GLTFLoader` and consumes whatever materials the GLB carries — this was the broken path.

### 5b.5 "Missing textures" are mostly dead shader refs, not extractor bugs
- **Finding:** During the cross-level investigation, the recurrent missing ID `0x5DF7D75E` appears as a 4-byte pattern exactly once each in `highmips.dat` of all four extracted R2 levels (chicago / scotia_coop / bay_area / scotia_multi) but **never** in any `assetlookup.dat`. That's the statistical signature of coincidental byte collision inside compressed BC texture blocks (~1% per ~50 MB file), not a real texture entry.
- **Interpretation:** the 50+ "missing" texture IDs are predominantly **dead shader references** that ship with the level data. The runtime engine handles them by falling back to a default texture. We should do the same at the renderer level eventually (placeholder gray/transparent texture for unresolved IDs) rather than treating them as cache-build failures.
- **One exception:** `0xFABAB505` IS a real entry in `bay_area_multiplayer/assetlookup.dat`. Cross-level fallback will recover it once chicago's cache is rebuilt against the now-extracted bay_area.

### 5b.6 Memory entries to add (continued)
- `memory/project_glb_material_three_maps.md` — new (GLB exporter binds all three texture maps and the "drop-only-if-all-three-missing" rule)

### 5b.7 Shader-slot reader: V1 layout, 4 hashes, detail map exposed
- **Finding via `RECHIMERA_LOG_SHADER_SLOTS=1`:** R2 (V2 game) actually uses IT's V1 lookup struct (`MaterialResourceNameLookup` at shader.hpp:213) — 4 `mapHashes[]` from `base+0x10`, then 4 `mapLookupPaths` u32 pointers. Slot order is **diffuse / normal / specular / detail**. IT's V2 struct (6 hashes) is for R3/A4O, not R2.
- **Bug fixed:** earlier patch read 6 slots; slots 4-5 were path-string pointers (tiny values 0x190-0x2B0), not texture hashes. `ShaderInfo` now exposes 4 named fields: `albedo_tex_id`, `normal_tex_id`, `expensive_tex_id` (specular per IT), `detail_tex_id`. RFOM reader also now populates the previously-discarded detail slot.
- **Diagnostic cleanup:** `[moby-tex]` log now shows ` det=0x... ` / ` det=MISSING 0x... ` only when shader's detail slot is non-zero; no more spurious `extras=[s3 s4 s5]` block.
- **Open question:** Whether to bind detail as a fallback albedo or as a separate PBR slot (occlusion / detailNormalMap) — waiting on RSX-Debugger confirmation for the minigun wheel before committing.

### 5b.8 AssetWorkbench center-panel tab (UI)
- **What:** New center-panel tab `assetWorkbench` (label "Asset") at `apps/desktop/src/views/AssetWorkbench.tsx`. Shows the currently-selected asset (moby/tie) as a standalone 3D scene with a Blender-style left burger drawer + bottom playback bar. Sits alongside the existing Viewport tab, both unmodified.
- **Why:** Users wanted a Photoshop/Blender-style per-asset inspector — see all submeshes / textures / animations as discrete rows, with hide/show toggles and a frame-by-frame animation scrubber. The existing `GlbPreview` modal showed the same scene but had no layered controls.
- **Layout:**
  - **Left** — collapsible drawer (default open, 240px). Top of drawer = burger button (X/menu) to collapse to 26px. Below = 3 sub-tab strip: Submeshes / Textures / Animations (Blender-floating-panel feel rejected by user in favor of integrated drawer).
  - **Center** — `<Canvas>` from `@react-three/fiber` with `Bounds + OrbitControls`, parses the cached `mobys/<tuid>.glb` (or `ties/<tuid>.glb`) via `GLTFLoader`. Same loader path as `GlbPreview` — reuses `readCachedBytes` from `api.ts`.
  - **Bottom-of-center** — floating playback bar with play/pause/stop + active clip name + frame counter (`frame N / M @ 30fps`). Hidden state visible only when a clip is selected.
- **Drawer-tab contents:**
  - *Submeshes*: scene traversal collects every `THREE.Mesh`, lists each with name + vertex count + eye-icon toggle. Toggling sets `mesh.visible = false` directly on the live scene (no re-render of the GLB).
  - *Textures*: walks `MATERIAL_TEXTURE_SLOTS` across all materials, dedups by texture UUID + slot, renders a 48×48 thumbnail by drawing the texture image onto an offscreen canvas + `toDataURL`. Slot name (`map`/`normalMap`/`emissiveMap` etc.) shown next to the texture name.
  - *Animations*: lists `gltf.animations` (embedded clips only — animset-loaded clips from the old `GlbPreview` menu aren't surfaced here yet). Clicking play on a row sets it active and starts playback.
- **Playback engine:** internal `AnimRig` component owns one `THREE.AnimationMixer` bound to the loaded scene. `clipAction.play()` + `action.paused = !isPlaying` keeps state in sync. `useFrame(delta)` advances the mixer and reports `Math.floor(action.time * FPS)` back to the parent — current FPS hardcoded to **30** (matches the engine baseline; revisit if R2 ever uses a non-30 source rate).
- **Inputs:** `instance: Instance | null` from `selection.primary`, `cacheFolder: string | null` from `summary?.folder`. Asset TUID parsed as `instance.asset_tuid.split("#")[0]`. Empty state shown if no selection or no cache folder.
- **Registration:** added `assetWorkbench` to `ViewId` in `store.ts`, `VIEW_META` + `ALL_VIEW_IDS` in `viewMeta.ts`, `viewBodies` map in `App.tsx`, and `views.assetWorkbench` i18n key across all 5 locales (en + 4 stubs).
- **Style:** all classes prefixed `.aw-*` (drawer/list/playback) in `styles.css`, ~190 lines at the bottom of the file.
- **Future scope (not yet built):**
  1. Surface animset-loaded clips from `listAnimsets` API (currently only embedded animations).
  2. Texture thumbnail click → open in `CacheLibraryModal`.

### 5b.9 Open-in-Workbench entry points (modal + Inspector) + missing-tex tint
- **What:** The CacheLibraryModal's fullscreen icon and a new "View model" button in the Inspector both route the selected moby/tie into the AssetWorkbench tab instead of opening their own preview.
- **Flow:**
  1. Click triggers `App.tsx::openInWorkbench(tuid, kind)`.
  2. Handler sets `workbenchAsset` state to `{tuid, kind}` (the override).
  3. Reads `s.panels.panels` to find which panel currently owns `assetWorkbench`. If center: `setActiveTab`. If another panel: `moveTab` to center. If nowhere: `addTabToPanel(center)` (creates + activates).
  4. Modal handler also closes the modal so the workbench is foregrounded immediately.
  5. AssetWorkbench reads `overrideAsset` first; when null, falls back to `instance` from selection. The early-return key is `assetTuidHex || cacheFolder || kind` — any of them missing → empty state.
- **Why moveTab + addTabToPanel logic:** `assetWorkbench` is `singleton: true` in `viewMeta`. The TabContainer's `+`-picker filters out singletons that exist in another panel, so naïvely calling `addTabToPanel("center")` while the tab lives in `right` would dupe it across panels. Using `moveTab` keeps the singleton invariant honest.
- **Missing-texture tint (also new):** After GLB parse, `AssetWorkbench::tintMissingTextureMaterials` walks all materials; any `MeshStandardMaterial` whose `.map`, `.normalMap`, AND `.emissiveMap` are ALL null gets `color = #bb55ff` (soft magenta) instead of three.js's default white-under-light. Lets data gaps stand out from "looks gray due to lighting." Materials are shared across submeshes that use the same shader, so this naturally tints every primitive of that shader at once.
- **Files touched:**
  - `apps/desktop/src/views/AssetWorkbench.tsx` — added `overrideAsset` prop + `tintMissingTextureMaterials` helper called right after `gltf.parse`.
  - `apps/desktop/src/components/CacheLibraryModal.tsx` — new `onOpenInWorkbench` prop, fullscreen Maximize2 button now routes to it (falls back to old in-modal fullscreen if callback not passed).
  - `apps/desktop/src/views/Inspector.tsx` — new `onOpenInWorkbench` prop + "View model" button (Maximize2 icon) in `inspector-actions`, only rendered when the selection is a moby/tie.
  - `apps/desktop/src/App.tsx` — new `workbenchAsset` state, `openInWorkbench` callback, wired into both modal and Inspector props.

### 5b.10 AssetWorkbench polish — timeline scrubber + Export .glb button
- **Timeline scrubber:** replaced bare "Frame N / M" text in the playback bar with a clickable+draggable progress bar. Fills with `--brand-color` as the clip plays; thin cursor marks the current frame; dotted tick marks at evenly-spaced intervals. Bidirectional sync: `useFrame` writes `frame` state every tick; mouse interactions write back via a `seekRef` callback installed by `AnimRig` (parent-held `MutableRefObject` to avoid re-mounting the mixer on each scrub).
- **Export .glb button:** floating top-right of the viewport. Uses the shared `Button` component (`variant="primary"`, `icon={Download}`) so it matches the modal's "Export .glb" pattern instead of a custom FAB. On click: `saveDialog` → `readCachedBytes("mobys/<tuid>.glb")` → `writeBytes(path, ...)`. Cached GLB IS the exported GLB — no re-encoding step.
- **Drawer tabs spacing:** `Submeshes / Textures / Animations` tabs now stack label-over-count vertically and ellipsis-truncate, so all three labels + counts fit in the 240px drawer regardless of locale.
- **Base `.btn` class:** added `display: inline-flex; align-items: center; justify-content: center; gap: 6px; line-height: 1.2` so icon + label are properly centered with consistent spacing across every Button in the app. Inspector buttons now sit as a 2-column grid (Go to + View model on top, Export .glb full-width primary below).

### 5b.11 Hierarchy: inline submeshes + texture-availability chips
- Each moby/tie leaf in `Hierarchy` now has a `▸` toggle. Expanded, it shows one indented row per submesh `[N]` with three texture-id chips: `a` (albedo), `n` (normal), `e` (emissive). Chips render an 8×8 colored square — **green `#4ade80`** if the texture is in the cache manifest, **purple `#bb55ff`** (matching AssetWorkbench's missing-data tint) if the ID resolves nowhere on disk. Hover shows the full `0x...` ID and a "not in cache — engine fallback" note when missing.
- **Per-asset expansion state:** `expandedAssets: Set<string>` (asset_tuid keys) at the Hierarchy level. Defaults to collapsed because 44-submesh mobys would drown the panel.
- **Texture availability set:** `cacheTextureIds: Set<number>` derived once from `cacheManifest.entries.filter(kind==="texture")`. Submesh ID lookup is O(1).
- Files: `apps/desktop/src/views/Hierarchy.tsx` (added state + threaded props to `AssetLibraryTree`, new `SubmeshList` + `TextureChip` components at bottom of file); `styles.css` (`.hierarchy-submesh-*` + `.hierarchy-tex-chip*` rules).

### 5b.12 Music classifier looks at sound name too
- `classifySound(source, name?)` in `api.ts` now falls through to test the sound's own name against `MUSIC_PATTERN` when the source bank doesn't match. Fix for R2 streaming music (`music_lxx_good4_080809_stg`) that lives inside `resident_sound.dat` — the source-bank-only classifier was sending those tracks into the SFX tab.
- Updated both call sites in `CacheLibraryModal` to pass `s.name`. No backend/Rust changes.

### 5b.13a SoundPlayer always-mounted + global volume + UI cleanup
- Player is now **always rendered** in `CacheLibraryModal`'s sound dock — no `nowPlaying && <SoundPlayer/>` gate. When nothing's loaded it shows an `empty` placeholder (`▶` disabled, `—` for name, "Select a sound to play" hint). Selecting any sound binds it to the same instance.
- **Volume is global, persisted to localStorage** (`rechimera.soundPlayerVolume`). Whenever a new audio element comes in, `useEffect([audio, volume])` writes the persisted value to it immediately — the user's level is applied to every track without touching the slider again.
- **Removed the download (`sp-action`) and close (`sp-close`) buttons** — bulk export lives in the modal toolbar (`Save WAV` / `Save N WAVs` / `Extract all → .zip`), so an inline download was redundant; close was unnecessary once the player became permanent. `SoundPlayer` no longer accepts an `onClose` prop; App.tsx's invocation was simplified accordingly.

### 5b.13b Music classifier tightened — dropped `mus` and `ost`
- `MUSIC_PATTERN` regex narrowed from `(music|mus|bgm|theme|ost)` to `(music|bgm|theme)`. Reason: `_ost_` and `_mus_` false-positive on weapon SFX names like `wep_sharpshooter_fire_mono_ost_dko`. Real R2 music tracks always carry the full `music_` prefix (`music_lxx_good4_080809_stg`) so the tighter pattern still catches every legitimate music asset.

### 5b.13 Bulk sound extraction → single .zip
- **Why:** users wanted a one-click "give me everything" download for sound assets — both bank-sourced SFX/music/dialog AND streamed music. The existing multi-select export went to a chosen folder one WAV at a time.
- **Backend (`src-tauri/src/main.rs::bulk_extract_sounds_zip`):** iterates `list_sound_banks_in(level_folder)`, calls `extract_bank_sounds_for_file` once per bank (so each bank is decoded one time, not per-sound), writes each ExtractedSound into a `zip::ZipWriter` as a single Store-mode entry (`<safe_name>.wav`). After the bank pass, opens the corresponding `<bank-stem>stream.dat` sidecar and pipes `extract_stream_sounds` results into the same zip. Filename collisions across banks resolve by appending `__<bank-stem>` (or `__stream`) to keep both copies.
- **Why Store mode, not Deflate:** PCM WAV barely compresses; the encode CPU would dominate the wall time. `zip` crate is loaded with `default-features = false` so we don't link bzip2/deflate libs we'd never call.
- **Frontend:** `api.ts::bulkExtractSoundsZip(folder, outPath) → Promise<number>`, button "Extract all → .zip" in `CacheLibraryModal`'s sound section header (alongside the existing "Save WAV"/"Save N WAVs" buttons). Uses `bulkBusy` flag for loading state and `bulkStatus` for the final "Wrote N WAVs to …" or error string.
- **New Cargo dep:** `zip = { version = "0.6", default-features = false }`.

---

## 6. Debugging methodology (for unknown formats)

When porting a new IGHW section or struct, follow this loop (codified in `docs/internal/lunalib-and-IT/09-debugging-methodology.md`):

1. **Cross-ref IT (or ReLunacy for TOD)** — find the existing reader. Cite `file:line` in chat.
2. **Probe** — multi-interpretation hex dump of the relevant bytes (i16, u32, f32, ascii).
3. **`[tag]` eprintln logs** — gated on `RECHIMERA_DEBUG_MOBY=<tuid>` env var (lowercase hex, no `0x`). Lets you run a level extract focused on ONE asset without 10MB of log noise.
4. **Re-extract** — usually with the wizard so cache rebuilds with the new code.
5. **Range-check** — assert reasonable values (rotations in unit quat range, indices < bone count, etc.).
6. **Lock** — when behavior is correct, codify with: (a) inline comment if invariant is critical (§3), (b) test or example dump tool in `crates/lunalib/examples/`, (c) memory entry.

**Diagnostic env vars currently respected:**
- `RECHIMERA_DEBUG_MOBY` — uppercase hex TUID (no `0x`). Filters debug logs to one moby.

---

## 7. Conventions / preferences

- **No code comments** unless explaining a critical invariant (the §3 things). Code explanations go in chat replies. If a `// Why:` comment exists, treat it as load-bearing.
- **Always check IT (or ReLunacy for TOD) before implementing** any new section/struct port. Don't infer from probe data alone.
- **Snake-case in Rust, camelCase in JS** — Tauri auto-converts at the boundary (e.g. `on_event: Channel<T>` → JS `onEvent: ch`, `map_id: String` → JS `mapId`).
- **Don't add a SQLite/Redis DB** for asset storage. The filesystem cache is sufficient; the cost would be a duplicate copy of data we already parse correctly.
- **PS3 is BIG-ENDIAN.** All `stream.rs` reads default to BE. The skeleton shift quirk (§3.3) is the one place that needs careful handling.

---

## 8. Where to look first

| If you're touching... | Start here |
|---|---|
| Level open / cache build | `apps/desktop/src-tauri/src/cache.rs::extract_level_to_cache` |
| R2 wizard flow | `apps/desktop/src/components/R2Wizard.tsx` + `apps/desktop/src-tauri/src/r2.rs` |
| Animation decode | `crates/lunalib/src/animation.rs` + `cache.rs::decode_clips_for_moby` / `decode_animset_clip` |
| Skinning / bones | `crates/lunalib/src/skeleton.rs` |
| Texture decode | `crates/lunalib/src/texture.rs` + `texture_global.rs` |
| Three.js scene | `apps/desktop/src/views/Viewport.tsx` |
| GLB/FBX export | `crates/lunalib/src/gltf_export.rs` + `fbx_binary_export.rs` |
| Game-picker / "Open" dialog | `apps/desktop/src/components/OpenLevelModal.tsx` |

---

## 9. Update protocol

- **Whenever you discover a new critical invariant** (one of "this took hours to find and would break silently if regressed"), update §3 here, add an inline `// Why:` comment at the call site, and write a memory entry.
- **At end of each significant work session**, append to §5 with the date and a one-line summary. Roll old entries into §3/§4 if they've matured into permanent invariants or active workstreams.
- **Per-game support matrix in §1** — update only when a game's status materially changes (new format supported, new bug surfaced).
