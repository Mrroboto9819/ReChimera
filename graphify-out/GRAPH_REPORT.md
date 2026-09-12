# Graph Report - ReChimera  (2026-09-11)

## Corpus Check
- Large corpus: 256 files · ~815,469 words. Semantic extraction will be expensive (many Claude tokens). Consider running on a subfolder.

## Summary
- 2309 nodes · 5334 edges · 120 communities (105 shown, 11 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 161 edges (avg confidence: 0.85)
- Token cost: 228,050 input · 0 output

## Community Hubs (Navigation)
- Tauri Backend Commands (main.rs)
- Cache & Extraction Pipeline
- IGHW Reader & Animation Decode
- Frontend API & R2 Wizard
- Binary FBX Export
- 3D Viewport & Scene Views
- Asset-Lookup Extractor & Outfitter
- GLTF/GLB Export Builder
- Sound Extraction (SCREAM/ADPCM)
- R2 PSARC Backend
- Zone Parsing (V2)
- UI Component Library
- Gameplay Placements Decode
- App Shell & Status UI
- Frontend Export & Skinning
- ASCII FBX Export
- Cache Library & Sound Player UI
- PSARC Reader Crate
- Texture Decode (V2)
- Moby Parsing (V2)
- Open-Level Modal
- Redux Store & Persistence
- Stream Helper (byte IO)
- GLB Preview & Export Options
- Asset Preview & Inspector
- Version-Sync Scripts
- About / Whats-New Modals
- Zone Parsing (TOD)
- Frontend Dependencies
- Settings Modal & i18n
- Cubemap Decode
- Moby Parsing (TOD)
- Character Modal (three-fiber)
- Asset Workbench View
- TypeScript Config
- Global Texture Fallback
- Tie Parsing (V2)
- Build Config (Vite/pkg)
- Tie Parsing (TOD)
- Moby Parsing (RFOM)
- PSARC Modal & Tools
- FBX Level Assembly
- Foliage Decode (RFOM)
- Level GLB Assembly
- Tie Parsing (RFOM)
- Release Channels & Updater
- Updater UI & Title Bar
- Modal & UI Hooks
- Asset Lookup Table
- Detail Mesh Decode (RFOM)
- Shrub Decode (RFOM)
- Skybox Decode (RFOM)
- Texture Decode (TOD)
- Animation Decode (docs)
- App Design System (docs)
- Tab Container & View Meta
- DDS Encode
- Region/UFrag Decode (RFOM)
- App Cache/Export (docs)
- Project Overview (docs)
- Asset-Lookup Tools View
- Docs Modal
- Prod CSP Config
- Dev CSP Config
- Menu Bar
- Env-Sampler Decode (RFOM)
- Texture Decode (RFOM)
- IGHW & Sound Format (docs)
- Vegetation & Sky Formats (docs)
- Comment-Stripping Script
- CI Build & Notify Workflows
- NPM Scripts
- GLB Preview/Export (docs)
- Layout Detect & Class IDs (docs)
- Version-Bump Script
- Frontend Dev Dependencies
- Shader Parsing (V2)
- Release Notes & Skins (docs)
- IgFile Tests
- Lighting Decode (RFOM)
- Shader Parsing (TOD)
- Shader Parsing (RFOM)
- Extraction Dispatch (docs)
- Texture/Skeleton Quirks (docs)
- GLTF Asset Classification
- Tauri Config (security)
- Example: dump_moby_skin
- Tie Instances (RFOM)
- Asset Lookup & Cache (docs)
- Moby Geometry Format (docs)
- Workspace Architecture (docs)
- Debugging Methodology (docs)
- Tauri Capabilities
- Windows Installer Config
- Tauri Build Hooks
- Bundle Config
- Updater Config
- Example: dump_moby_meshes
- Skeleton Format (docs)
- Skeleton Math (docs)
- TOD Format Quirks (docs)
- Example: dump_assetlookup
- Shader/Material Textures (docs)
- Example: dump_shaders
- Example: dump_textures
- Example: dump_tie_meshes
- Example: dump_ufrags
- lunalib Error Type
- Unknown-Format Registry
- psarc Error Type
- Workspace Crates
- Bind-Matrix Math (docs)
- decode_animset_clip cmd (docs)
- classifySound (docs)
- Pitch Table (docs)
- No-Game-Data Policy (docs)

## God Nodes (most connected - your core abstractions)
1. `IgFile` - 70 edges
2. `react` - 45 edges
3. `FbxNode` - 35 edges
4. `App()` - 33 edges
5. `R2Wizard()` - 29 edges
6. `MobyAsset` - 29 edges
7. `ShaderInfo` - 29 edges
8. `Skeleton` - 28 edges
9. `run_extract()` - 27 edges
10. `append_moby_to_fbx()` - 23 edges

## Surprising Connections (you probably didn't know these)
- `Engine-Era Autodetect via Layout Marker` --semantically_similar_to--> `LevelLayout Enum (per-engine dispatch)`  [INFERRED] [semantically similar]
  README.md → docs/internal/app/01-architecture.md
- `decode_clips_for_moby_inline()` --calls--> `decode_animation_with_skeleton()`  [INFERRED]
  apps/desktop/src-tauri/src/cache.rs → crates/lunalib/src/animation.rs
- `decode_clips_for_moby_inline()` --calls--> `read_animation_control()`  [INFERRED]
  apps/desktop/src-tauri/src/cache.rs → crates/lunalib/src/animation.rs
- `decode_clips_for_moby_inline()` --calls--> `read_animation_header_at()`  [INFERRED]
  apps/desktop/src-tauri/src/cache.rs → crates/lunalib/src/animation.rs
- `decode_clips_for_moby()` --calls--> `animation_section_offsets()`  [INFERRED]
  apps/desktop/src-tauri/src/cache.rs → crates/lunalib/src/animation.rs

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **Dual-Channel Release Pipeline Job Flow** — _github_workflows_release_compute_mode_job, _github_workflows_release_gate_job, _github_workflows_release_create_release_job, _github_workflows_release_build_matrix_job, _github_workflows_release_publish_release_job, _github_workflows_release_alias_canary_latest_job [EXTRACTED 1.00]
- **Cache-First Extraction & Consumption Subsystem** — docs_internal_app_01_architecture_cache_first_flow, docs_internal_app_02_cache_extract_level_to_cache, docs_internal_app_02_cache_rechimera_cache_layout, docs_internal_app_02_cache_manifest_json, docs_internal_app_05_export_pipeline_export_moby_glb_with_options, docs_internal_app_03_frontend_glb_preview_burger [INFERRED 0.85]
- **Per-Engine Parser Family Dispatch (V2/Rfom/Tod)** — docs_internal_app_01_architecture_level_layout_enum, docs_internal_app_01_architecture_moby_asset_struct, docs_internal_app_02_cache_per_engine_dispatch, readme_layout_marker_autodetect [INFERRED 0.85]
- **IGHW Container Reading Flow** — docs_internal_lunalib_and_it_01_ighw_container_igfile_reader, docs_internal_lunalib_and_it_01_ighw_container_endian_flip_autodetect, docs_internal_lunalib_and_it_01_ighw_container_section_header, docs_internal_lunalib_and_it_01_ighw_container_section_array_vs_buffer, docs_internal_lunalib_and_it_01_ighw_container_version_header_layout [EXTRACTED 1.00]
- **Moby Geometry Decode Chain (asset->bangle->primitive->vertex)** — docs_internal_lunalib_and_it_04_moby_tie_geometry_moby_asset, docs_internal_lunalib_and_it_04_moby_tie_geometry_moby_segment_bangle, docs_internal_lunalib_and_it_04_moby_tie_geometry_primitive_v2, docs_internal_lunalib_and_it_04_moby_tie_geometry_vertex1_skinned, docs_internal_lunalib_and_it_04_moby_tie_geometry_bone_palette_indirection [EXTRACTED 1.00]
- **Animation Decode Pipeline (header->control->decode->clip)** — docs_internal_lunalib_and_it_06_animation_animation_header, docs_internal_lunalib_and_it_06_animation_animation_control, docs_internal_lunalib_and_it_06_animation_decode_animation, docs_internal_lunalib_and_it_06_animation_decoded_clip, docs_internal_lunalib_and_it_06_animation_quantization_shift_formula [EXTRACTED 1.00]

## Communities (120 total, 11 thin omitted)

### Community 0 - "Tauri Backend Commands (main.rs)"
Cohesion: 0.06
Nodes (119): AnimsetSummaryDto, asset_lookup_extract_stream(), asset_lookup_inspect(), AssetCache, AssetCount, assetlookup_path(), AssetLookupExtractEvent, AssetLookupKindDto (+111 more)

### Community 1 - "Cache & Extraction Pipeline"
Cohesion: 0.06
Nodes (92): animset_matches_debug_env(), AnimsetClipMeta, AnimsetIndex, AnimsetSummary, CACHE_DIR_NAME, cache_root(), cache_status(), cache_status_from_dir() (+84 more)

### Community 2 - "IGHW Reader & Animation Decode"
Cohesion: 0.05
Nodes (69): probe_anim_bytes(), R, dump_decoded_clip(), flags_summary(), main(), DecodedClip, ExitCode, String (+61 more)

### Community 3 - "Frontend API & R2 Wizard"
Cohesion: 0.04
Nodes (66): AnimsetClipMeta, AssetCount, AssetLookupEvent, AssetPointer, CacheEvent, CacheLoadProgress, CharacterLibraryEvent, DecodedBone (+58 more)

### Community 4 - "Binary FBX Export"
Cohesion: 0.08
Nodes (60): BinaryFbxBuilder, build_anim_curve(), build_anim_curve_node(), build_anim_layer(), build_anim_stack(), build_definitions(), build_documents(), build_geometry() (+52 more)

### Community 5 - "3D Viewport & Scene Views"
Cohesion: 0.05
Nodes (49): AssetKind, AssetMeshes, CacheManifest, exportLevelGlb(), Instance, LevelGlbExportEvent, MeshGeom, UFragMesh (+41 more)

### Community 6 - "Asset-Lookup Extractor & Outfitter"
Cohesion: 0.08
Nodes (52): AssetLookupOverview, encode_face_png(), extract_assetlookup(), extract_cubemaps(), extract_mobys(), extract_pointer_table_json(), extract_shaders_json(), extract_textures() (+44 more)

### Community 7 - "GLTF/GLB Export Builder"
Cohesion: 0.09
Nodes (58): Accessor, Animation, append_moby_to_doc(), asset_display_name(), build_material(), CHUNK_TYPE_BIN, CHUNK_TYPE_JSON, emit_animations() (+50 more)

### Community 8 - "Sound Extraction (SCREAM/ADPCM)"
Cohesion: 0.10
Nodes (55): Arguments, ADPCM_TABLE, AdpcmCh, bank_pair_for(), decode_adpcm_block(), decode_adpcm_run(), decode_adpcm_stream(), decode_one_stream() (+47 more)

### Community 9 - "R2 PSARC Backend"
Cohesion: 0.12
Nodes (53): alias_patch_overlay(), category_order(), classify_map(), decode_image_file_to_png_bytes(), dir_mtime_recursive(), extract_one_psarc(), global_psarc_state(), humanize_map_name() (+45 more)

### Community 10 - "Zone Parsing (V2)"
Cohesion: 0.09
Nodes (39): main(), ExitCode, decode_ufrag_mesh(), FOLIAGE_INSTANCE_SIZE, NAME_POINTER_SIZE, parse_foliage_instances(), parse_shrub_instances(), parse_ufrags() (+31 more)

### Community 11 - "UI Component Library"
Cohesion: 0.08
Nodes (29): AssetLookupModal(), AssetLookupModalProps, SearchField, SearchFieldProps, Button, ButtonProps, ICON_SIZE, Size (+21 more)

### Community 12 - "Gameplay Placements Decode"
Cohesion: 0.08
Nodes (35): main(), ExitCode, GameplayLayout, MOBY_INSTANCE_SIZE, MOBY_METADATA_SIZE, MobyInstance, OLD_MOBY_INSTANCE_SIZE, read_gameplay_old() (+27 more)

### Community 13 - "App Shell & Status UI"
Cohesion: 0.12
Nodes (31): cacheStatus, dumpSoundBank(), extractLevelSounds(), extractLevelStreamSounds(), extractRawStreamingSounds(), levelLayout, LevelSummary, listGltfsInFolder() (+23 more)

### Community 14 - "Frontend Export & Skinning"
Cohesion: 0.10
Nodes (32): decodeBytes(), decodeFloat32(), decodeMeshGeom(), decodeUint16(), decodeUint32(), decodeUint8(), fetchAnimsetClip(), ExportProgress() (+24 more)

### Community 15 - "ASCII FBX Export"
Cohesion: 0.14
Nodes (30): compute_bind_world_matrices(), definitions_block(), documents_block(), emit_anim_curve(), emit_anim_curve_node(), emit_anim_layer(), emit_anim_stack(), emit_animation_clips() (+22 more)

### Community 16 - "Cache Library & Sound Player UI"
Cohesion: 0.11
Nodes (29): bulkExtractSoundsZip(), CacheManifestEntry, classifySound(), CubemapDescriptor, exportTextureDds(), exportTexturePng(), ExtractedSound, extractOneSound() (+21 more)

### Community 17 - "PSARC Reader Crate"
Cohesion: 0.12
Nodes (23): Archive, Archive<BufReader<File>>, Archive<R>, Compression, Entry, ENTRY_BYTES, FLAG_ABSOLUTE, FLAG_IGNORE_CASE (+15 more)

### Community 18 - "Texture Decode (V2)"
Cohesion: 0.19
Nodes (30): A, ASSET_POINTER_SIZE, bulk_extract_pngs(), decode_a8r8g8b8_morton(), decode_dxt(), decode_format(), decode_image_file_to_png(), decode_r5g6b5_morton() (+22 more)

### Community 19 - "Moby Parsing (V2)"
Cohesion: 0.14
Nodes (30): AtomicUsize, ACIT_PROBE_BYTES, BANGLE_SIZE, decode_moby_mesh(), dump_section_for_probe(), MOBY_MESH_SIZE, MOBY_STATS_NO_GEOM, MOBY_STATS_WITH_GEOM (+22 more)

### Community 20 - "Open-Level Modal"
Cohesion: 0.10
Nodes (27): FallbackImage(), FallbackImageProps, levelThumbCandidates(), levelThumbCandidatesFromPath(), Capabilities, CAPABILITY_LABELS, CapabilityState, capTooltip() (+19 more)

### Community 21 - "Redux Store & Persistence"
Cohesion: 0.09
Nodes (24): root, AppDispatch, AppSkinDef, BooleanViewKey, DEFAULT_LAYOUT, DEFAULT_PANELS, DEFAULT_SETTINGS, DEFAULT_VIEW (+16 more)

### Community 22 - "Stream Helper (byte IO)"
Cohesion: 0.15
Nodes (8): Endian, R, Result, Self, String, Vec, StreamHelper, StreamHelper<R>

### Community 23 - "GLB Preview & Export Options"
Cohesion: 0.12
Nodes (20): buildAnimationClip(), AnimsetSummary, ClipPick, decodeAnimsetClip(), DecodedClipDto, exportCachedMobyGlb(), exportMobyGlbWithOptions(), GlbExportOptions (+12 more)

### Community 24 - "Asset Preview & Inspector"
Cohesion: 0.13
Nodes (19): LevelMeshes, TextureBlobMap, TexturePayload, UFragBounds, CharacterPreviewModal(), CharacterPreviewModalProps, shortName(), AssetPreview() (+11 more)

### Community 25 - "Version-Sync Scripts"
Cohesion: 0.11
Nodes (24): args, canaryIconPath, cargoPath, checkOnly, frontendIconPath, here, patchPackageJsonVersion(), patchStoreBrandColor() (+16 more)

### Community 26 - "About / Whats-New Modals"
Cohesion: 0.13
Nodes (18): AboutModal(), CreditEntry, onAboutClick(), PEOPLE, WhatsNewModal(), WhatsNewModalProps, WhatsNewState, APP_BRAND_NAME (+10 more)

### Community 27 - "Zone Parsing (TOD)"
Cohesion: 0.15
Nodes (22): OLD_TIE_INSTANCE_SIZE, OLD_UFRAG_SIZE, OLD_UFRAG_VERTEX_STRIDE, parse_one_tie_instance(), POS_NORM_DIV, read_tie_instances(), read_ufrag_shader_table(), read_ufrags() (+14 more)

### Community 28 - "Frontend Dependencies"
Cohesion: 0.09
Nodes (22): dependencies, gsap, i18next, lucide-react, react, react-dom, react-i18next, react-markdown (+14 more)

### Community 29 - "Settings Modal & i18n"
Cohesion: 0.11
Nodes (18): Select(), SelectOption, SelectProps, ColorFieldProps, Credit, CREDITS, SettingsModalProps, TabKey (+10 more)

### Community 30 - "Cubemap Decode"
Cohesion: 0.16
Nodes (18): Cubemap, CubemapFace, derive_dxt_base_dim(), dxt_mip_chain_size(), dxt_mip_size(), FACES, parse_cubemap(), read_cubemaps() (+10 more)

### Community 31 - "Moby Parsing (TOD)"
Cohesion: 0.15
Nodes (21): FLAG_USE_VERTICES_DAT, MeshHeader, OLD_MOBY_BANGLE_SIZE, OLD_MOBY_HEADER_SIZE, OLD_MOBY_MESH_SIZE, parse_one(), read_buffer_slice(), read_moby_assets_old() (+13 more)

### Community 32 - "Character Modal (three-fiber)"
Cohesion: 0.12
Nodes (17): DecodedClip, findGlbTextures(), GltfFile, listAnimsetClips(), readFileBytes(), formatBigNum(), FpsOverlay(), FpsOverlayProps (+9 more)

### Community 33 - "Asset Workbench View"
Cohesion: 0.12
Nodes (14): AnimRigProps, AssetWorkbench(), collectMeshes(), collectTextures(), disposeGltfScene(), DrawerTab, LoadedGlb, MATERIAL_TEXTURE_SLOTS (+6 more)

### Community 34 - "TypeScript Config"
Cohesion: 0.10
Nodes (20): compilerOptions, allowSyntheticDefaultImports, esModuleInterop, isolatedModules, jsx, lib, module, moduleResolution (+12 more)

### Community 35 - "Global Texture Fallback"
Cohesion: 0.24
Nodes (20): build_global_texture_index(), compute_mip0_size(), discover_and_index(), find_global_tuid_roots(), find_patch_overlay(), find_sibling_extracted_levels(), GlobalTextureEntry, load_global_texture_png() (+12 more)

### Community 36 - "Tie Parsing (V2)"
Cohesion: 0.19
Nodes (20): decode_tie_mesh(), parse_tie(), read_tie_assets(), read_tie_assets_streaming(), read_tie_assets_with_total(), F, Option, Path (+12 more)

### Community 37 - "Build Config (Vite/pkg)"
Cohesion: 0.11
Nodes (18): name, private, type, version, here, pkg, react-resizable-panels, redux-persist (+10 more)

### Community 38 - "Tie Parsing (TOD)"
Cohesion: 0.15
Nodes (19): OLD_TIE_HEADER_SIZE, OLD_TIE_MESH_SIZE, OLD_TIE_VERTEX_STRIDE, parse_one(), read_section_slice(), read_tie_assets_old(), read_tie_assets_old_with_total(), F (+11 more)

### Community 39 - "Moby Parsing (RFOM)"
Cohesion: 0.18
Nodes (18): MeshHeader, parse_one(), read_at(), read_moby_assets_rfom(), read_moby_assets_rfom_with_total(), RFOM_BANGLE_SIZE, RFOM_MOBY_HEADER_SIZE, RFOM_PRIMITIVE_SIZE (+10 more)

### Community 40 - "PSARC Modal & Tools"
Cohesion: 0.20
Nodes (15): psarcExtractStream(), psarcList(), PsarcListDto, acceptPsarcDrop(), ExtractStatus, lastTwoSegments(), loadRecent(), PsarcModal() (+7 more)

### Community 41 - "FBX Level Assembly"
Cohesion: 0.31
Nodes (17): append_moby_to_fbx(), append_static_level_to_fbx(), emit_geometry(), emit_material(), emit_mesh_model(), emit_texture(), emit_video(), resolve_albedo() (+9 more)

### Community 42 - "Foliage Decode (RFOM)"
Cohesion: 0.14
Nodes (17): BRANCH_VERTEX_STRIDE, FOLIAGE_INSTANCE_SIZE, FOLIAGE_SIZE, read_foliage_rfom(), read_foliage_sprites(), Option, Path, R (+9 more)

### Community 43 - "Level GLB Assembly"
Cohesion: 0.26
Nodes (17): bounds(), LevelGlbAsset, LevelGlbInstance, LevelGlbSubmesh, pad_align(), DecodedClip, HashMap, Option (+9 more)

### Community 44 - "Tie Parsing (RFOM)"
Cohesion: 0.16
Nodes (17): parse_one(), read_tie_assets_rfom(), read_tie_assets_rfom_with_total(), RFOM_TIE_HEADER_SIZE, RFOM_TIE_PRIMITIVE_SIZE, RFOM_TIE_VERTEX_STRIDE, F, Option (+9 more)

### Community 45 - "Release Channels & Updater"
Cohesion: 0.14
Nodes (17): alias-canary-latest Job (rolling manifest mirror), Canary Release Channel, compute-mode Job, create-release Job, gate Job (version-sync + dedup), latest.json URL Rewrite for Canary, MSI Pre-release Numeric Version Constraint, Release Workflow (dual-channel) (+9 more)

### Community 46 - "Updater UI & Title Bar"
Cohesion: 0.15
Nodes (14): AboutModalProps, formatBytes(), UpdateChecker(), UpdateCheckerProps, isAutoUpdateSupported(), UpdatePhase, UpdaterState, useUpdater() (+6 more)

### Community 47 - "Modal & UI Hooks"
Cohesion: 0.21
Nodes (14): Modal(), ModalProps, SIZE_WIDTH, getAppSkin(), useAppSelector, useApplySettings(), audioCache, EXTS (+6 more)

### Community 48 - "Asset Lookup Table"
Cohesion: 0.16
Nodes (9): ASSET_POINTER_SIZE, AssetKind, AssetLookup, AssetLookup<R>, AssetPointer, R, Result, Self (+1 more)

### Community 49 - "Detail Mesh Decode (RFOM)"
Cohesion: 0.14
Nodes (15): DETAIL_CLUSTER_SIZE, DETAIL_INSTANCE_SIZE, DETAIL_SIZE, read_detail_clusters_rfom(), Path, Result, Vec, SECT_DETAIL (+7 more)

### Community 50 - "Shrub Decode (RFOM)"
Cohesion: 0.15
Nodes (15): read_shrubs_rfom(), rot_basis_to_quat(), Path, Result, Vec, SECT_LEVEL_INDEX_BUFFER, SECT_LEVEL_VERTEX_BUFFER, SECT_MATERIAL_V1 (+7 more)

### Community 51 - "Skybox Decode (RFOM)"
Cohesion: 0.25
Nodes (14): read_skybox_rfom(), Option, Path, Result, String, Vec, SECT_SKY_DESC, SECT_SKY_VERTS (+6 more)

### Community 52 - "Texture Decode (TOD)"
Cohesion: 0.20
Nodes (14): decode_one(), highmip_size(), OLD_TEXSTREAM_REF_SIZE, OLD_TEXTURE_REF_SIZE, OldTexstreamRef, read_textures_old(), Option, Path (+6 more)

### Community 53 - "Animation Decode (docs)"
Cohesion: 0.14
Nodes (15): IT MobyV2 -> MobySegment -> PrimitiveV2, MobyAsset Struct, MobyV2 Header (section 0xD100), AnimationControl (reference pose + track masks), decode_animation Flow, DecodedClip / DecodedBone Shape, IT LoadAnimations (gltf_shared.cpp), Quantization Shift Formula (pos_scale/scale_scale) (+7 more)

### Community 54 - "App Design System (docs)"
Cohesion: 0.14
Nodes (14): Inter Typeface + Positive Letter-Spacing, macOS-Native Multi-Layer Shadow System, Near-Black Blue Background (#07080a), Raycast-Inspired Design System, Vite index.html Entry Point, App.tsx Top-Level Layout, Per-Game Capability Badges, dragDropEnabled:false Window Config (+6 more)

### Community 55 - "Tab Container & View Meta"
Cohesion: 0.26
Nodes (12): SettingsModal(), PanelId, useAppDispatch, ViewId, ALL_VIEW_IDS, VIEW_META, ViewMeta, decode() (+4 more)

### Community 56 - "DDS Encode"
Cohesion: 0.16
Nodes (13): DDPF_ALPHAPIXELS, DDPF_RGB, DDS_MAGIC, DDSCAPS_TEXTURE, DDSD_CAPS, DDSD_HEIGHT, DDSD_PITCH, DDSD_PIXELFORMAT (+5 more)

### Community 57 - "Region/UFrag Decode (RFOM)"
Cohesion: 0.15
Nodes (13): POS_NORM_DIV, read_regions_rfom(), REGION_MESH_SIZE, REGION_VERTEX_SCALE, REGION_VERTEX_STRIDE, Path, Result, Vec (+5 more)

### Community 58 - "App Cache/Export (docs)"
Cohesion: 0.18
Nodes (13): Cache-First Flow, Architecture Overview, extract_level_to_cache Command, manifest.json Schema, Cache Pipeline, _rechimera_cache On-Disk Layout, cache.rs Module (~50 Tauri commands), cache_status Invalidation (source_mtimes) (+5 more)

### Community 59 - "Project Overview (docs)"
Cohesion: 0.22
Nodes (13): Internal Documentation Index, lunalib-and-IT Documentation Stack, Lunacy / 7th igRewrite (renderer inspiration), GPL-3.0-or-later Licensing, InsomniaToolset (upstream), Third-Party JS/TS Packages, ReChimera (project), ReLunacy / LibLunacy (upstream) (+5 more)

### Community 60 - "Asset-Lookup Tools View"
Cohesion: 0.21
Nodes (11): assetLookupExtractStream(), assetLookupInspect(), AssetLookupKindDto, AssetLookupOverviewDto, ALL_KIND_ORDER, AssetLookupTools(), AssetLookupToolsProps, DEFAULT_SELECTED (+3 more)

### Community 61 - "Docs Modal"
Cohesion: 0.24
Nodes (11): buildEntries(), DocEntry, DocGroup, DocsModal(), DocsModalProps, docsRaw, GROUP_META, groupOf() (+3 more)

### Community 62 - "Prod CSP Config"
Cohesion: 0.17
Nodes (12): base-uri, connect-src, default-src, font-src, frame-src, img-src, media-src, object-src (+4 more)

### Community 63 - "Dev CSP Config"
Cohesion: 0.17
Nodes (12): base-uri, connect-src, default-src, font-src, frame-src, img-src, media-src, object-src (+4 more)

### Community 64 - "Menu Bar"
Cohesion: 0.17
Nodes (11): Menu(), MenuBar(), MenuBarProps, MenuCheckItem(), MenuCheckItemProps, MenuContext, MenuContextValue, MenuItem() (+3 more)

### Community 65 - "Env-Sampler Decode (RFOM)"
Cohesion: 0.21
Nodes (10): EnvSampler, ENVSAMPLER_SIZE, read_envsamplers_rfom(), Path, Result, Vec, SECT_ENVSAMPLER, YARD_TO_M (+2 more)

### Community 66 - "Texture Decode (RFOM)"
Cohesion: 0.26
Nodes (10): base_mip_size(), read_textures_rfom(), Option, Path, Result, Vec, SECT_TEXTURE_V1, texture_rfom_to_png() (+2 more)

### Community 67 - "IGHW & Sound Format (docs)"
Cohesion: 0.18
Nodes (12): IGHW Endian-Flip Auto-Detect, IgFile Reader (igfile.rs), IGHW Container Format, IGHW Version Header Layout (v0.2 vs v1.1), Bank-Relative Pointer Fixup, PS-ADPCM Block Decoder (28 samples/16 bytes), SCREAM Bank Format (IGHW sections 0x21xxx), SCREAM V1/V2 Section-Collision Auto-Detect (+4 more)

### Community 68 - "Vegetation & Sky Formats (docs)"
Cohesion: 0.21
Nodes (12): DetailCluster (detail_rfom.rs), tie_as_moby (cache.rs adapter), Tie (static prop, tie.rs), Multi-Mesh Single-Skin Layout, Skybox Dome Heuristic Triangulation, Foliage Section Mislabel History (lighting/envsampler), Foliage (foliage_rfom.rs, 0xC200/0x9700), Half-Float Vertex-Position Gotcha (int16[4] declared, R16G16B16A16 FLOAT actual) (+4 more)

### Community 69 - "Comment-Stripping Script"
Cohesion: 0.18
Nodes (8): strip-comments, collected, here, repoRoot, ROOTS, SKIP_DIRS, SKIP_FILES, walk()

### Community 70 - "CI Build & Notify Workflows"
Cohesion: 0.18
Nodes (11): Bot-Opened Issue Filter, Discord Issue Notifier Workflow, ISSUE_WEBHOOK_URL Fallback to DISCORD_WEBHOOK_URL, build Job (4-platform matrix), Discord Release-Published Notification, Four Channel-Branding Surfaces, notify-build-failure Job, Prepare Channel Config Step (+3 more)

### Community 71 - "NPM Scripts"
Cohesion: 0.18
Nodes (11): scripts, build, dev, preview, tauri, tauri:build, tauri:dev, version:bump (+3 more)

### Community 72 - "GLB Preview/Export (docs)"
Cohesion: 0.20
Nodes (11): AssetPreview Routing (GLB vs JSON), GlbPreview + Animation Burger Menu, Portal-Rendered Select Component, Cache Bypass for High/Original Quality, export_moby_glb_with_options (custom rebuild), ExportOptionsModal (multi-step), GlbExportOptions Struct, Texture Quality Presets (+3 more)

### Community 73 - "Layout Detect & Class IDs (docs)"
Cohesion: 0.18
Nodes (11): IGHW Class-ID Catalogue, IGHW Section: Array vs Buffer Mode, SectionHeader (16-byte class-id entry), IT resource.hpp (ResourceMobys/Ties/Shaders/Animsets), detect_layout (level_layout.rs), Never Collapse the Three Layout Paths, RFOM Layout (ps3levelmain.dat), TOD Layout (main.dat) (+3 more)

### Community 74 - "Version-Bump Script"
Cohesion: 0.18
Nodes (10): args, cargoPath, cargoText, currentMatch, dryRun, here, newVersion, repoRoot (+2 more)

### Community 75 - "Frontend Dev Dependencies"
Cohesion: 0.20
Nodes (10): devDependencies, strip-comments, @tauri-apps/cli, @types/node, @types/react, @types/react-dom, @types/three, typescript (+2 more)

### Community 76 - "Shader Parsing (V2)"
Cohesion: 0.36
Nodes (9): parse_shader(), read_shaders(), HashMap, Option, Path, R, Result, SECT_SHADER_REFS (+1 more)

### Community 77 - "Release Notes & Skins (docs)"
Cohesion: 0.25
Nodes (9): Recognized UI Sound Event Filenames, UI Sound Effects Folder Layout, App Skin System (5 skins), AssetWorkbench View, Character GLBs Exported with Real Names, Per-Skin UI Sound Effects, RFOM USRDIR Wizard Support, TOD Pending Work (wizard, anims, skybox) (+1 more)

### Community 78 - "IgFile Tests"
Cohesion: 0.28
Nodes (7): MAGIC_BIG, MAGIC_LITTLE, parses_v1_1_header(), require_section_errors_when_missing(), Vec, synthetic_v1_1(), Version

### Community 79 - "Lighting Decode (RFOM)"
Cohesion: 0.31
Nodes (8): LIGHT_SIZE, LightInstance, read_lights_rfom(), Path, Result, Vec, SECT_LIGHT, YARD_TO_M

### Community 80 - "Shader Parsing (TOD)"
Cohesion: 0.33
Nodes (8): nonzero(), OLD_SHADER_SIZE, read_shaders_old(), HashMap, Option, Path, Result, SECT_OLD_SHADER

### Community 81 - "Shader Parsing (RFOM)"
Cohesion: 0.33
Nodes (8): MATERIAL_V1_SIZE, nonzero(), read_shaders_rfom(), HashMap, Option, Path, Result, SECT_MATERIAL_V1

### Community 82 - "Extraction Dispatch (docs)"
Cohesion: 0.22
Nodes (9): LevelLayout Enum (per-engine dispatch), Unified MobyAsset Struct, RECHIMERA_DEBUG_MOBY Single-Moby Filter, decode_clips_for_moby_inline (RFOM/TOD), Per-Engine Cache Dispatch (V2/Rfom/Tod), run_extract Extraction Flow, tie_as_moby Adapter, Engine-Era Autodetect via Layout Marker (+1 more)

### Community 83 - "Texture/Skeleton Quirks (docs)"
Cohesion: 0.25
Nodes (9): IT FByteswapper<Skeleton> (serialize.cpp:186-201), recover_shift Byte-Order Quirk, RFOM Viseme Head Rigs (0x0103 shift), bulk_extract_pngs Decode Pipeline, downsample_png_to (shared resize helper), IT shader.hpp::TextureFormat Enum, Morton (Z-Order) Unswizzle, TexFormat::from_byte Dual-Range Decoder (+1 more)

### Community 84 - "GLTF Asset Classification"
Cohesion: 0.25
Nodes (3): CATEGORY_COLOR, CATEGORY_LABEL, GltfCategory

### Community 85 - "Tauri Config (security)"
Cohesion: 0.25
Nodes (7): app, security, windows, identifier, productName, $schema, version

### Community 86 - "Example: dump_moby_skin"
Cohesion: 0.43
Nodes (5): main(), print_moby(), ExitCode, HashSet, SummaryStats

### Community 87 - "Tie Instances (RFOM)"
Cohesion: 0.32
Nodes (7): read_tie_instances_rfom(), Path, Result, Vec, SECT_TIE_INSTANCE, TIE_INSTANCE_SIZE, YARD_TO_M

### Community 88 - "Asset Lookup & Cache (docs)"
Cohesion: 0.25
Nodes (8): AnimsetIndex (O(1) hash map, cache.rs), AssetKind Enum, Asset Lookup (assetlookup.dat), AssetPointer (tuid, offset, length), open_level Tauri Command, Texture Metadata Entry (0x5A00, 4 bytes), Texture Two-File Scheme (assetlookup + highmips), decode_clips_for_moby (cache per-moby decode)

### Community 89 - "Moby Geometry Format (docs)"
Cohesion: 0.25
Nodes (8): Per-Primitive Bone-Palette Indirection, MobySegment / Bangle Struct, PrimitiveV2 Struct (64 bytes, section 0xDD00), Vertex0 purpose-Field Bone Formula, Vertex0 (static, stride 0x14), Vertex1 (skinned, stride 0x1C), IT MobyToGltf (extract_gltf.cpp), Emitted glTF Scene Structure

### Community 90 - "Workspace Architecture (docs)"
Cohesion: 0.33
Nodes (7): Binary IPC Response (base64 bypass), Cargo Workspace Layout, lunalib crate (pure parsing), psarc crate (archive reader), React + Three.js Frontend, Tauri Backend Command Layer, IGHW Container Reader (ported concept)

### Community 91 - "Debugging Methodology (docs)"
Cohesion: 0.33
Nodes (7): Debugging Anti-Patterns, Step 1: Cross-Reference Canonical Source First, Env-Sampler Position Worked Example, Step 7: Lock Invariants (memory + doc + comment-override), Step 2: Probe Before Decoding (rfom_probe.rs), Step 3: [tag] eprintln Diagnostics, Unknown-Bytes Debugging Loop (7 steps)

### Community 92 - "Tauri Capabilities"
Cohesion: 0.33
Nodes (5): description, identifier, permissions, $schema, windows

### Community 93 - "Windows Installer Config"
Cohesion: 0.33
Nodes (6): windows, installMode, silent, type, nsis, webviewInstallMode

### Community 94 - "Tauri Build Hooks"
Cohesion: 0.40
Nodes (5): build, beforeBuildCommand, beforeDevCommand, devUrl, frontendDist

### Community 95 - "Bundle Config"
Cohesion: 0.40
Nodes (5): bundle, active, createUpdaterArtifacts, icon, targets

### Community 96 - "Updater Config"
Cohesion: 0.40
Nodes (5): plugins, updater, active, endpoints, pubkey

### Community 97 - "Example: dump_moby_meshes"
Cohesion: 0.50
Nodes (4): main(), ExitCode, String, shorten()

### Community 98 - "Skeleton Format (docs)"
Cohesion: 0.50
Nodes (5): clean_rigid_col_major (FP-noise zeroing), Col-Major Col-Vector Matrix Convention, Insomniac Root-Bone Self-Parent Convention, Skeleton Struct (section 0xD300), pack_ibms (inverseBindMatrices cleanup)

### Community 99 - "Skeleton Math (docs)"
Cohesion: 0.40
Nodes (5): decompose_col_major (math.rs, Shepperd's method), IT GenerateSkeleton (extract_gltf.cpp), Joint=0 when Weight=0 Rule, Bone Nodes as TRS (not matrix), glTF Validator Zero-Warnings Milestone

### Community 100 - "TOD Format Quirks (docs)"
Cohesion: 0.40
Nodes (5): OldTieInstance (main.dat:0x9240), TOD Tie Quirks (tie_old.rs), Animation Decode (animation.rs), AnimationHeader Struct (section 0xF000), TOD Pair-Frame Encoding

### Community 101 - "Example: dump_assetlookup"
Cohesion: 0.67
Nodes (3): BufReader, main(), ExitCode

### Community 102 - "Shader/Material Textures (docs)"
Cohesion: 0.50
Nodes (4): resolve_shader_textures (section 0x5600), ShaderInfo (albedo/normal/emissive tex ids), build_material (texture embed + dedup), texture_idx = image_idx Dedup Fix

### Community 108 - "Unknown-Format Registry"
Cohesion: 0.67
Nodes (3): HashMap, Mutex, UNKNOWN_FORMAT_BYTES

### Community 110 - "Workspace Crates"
Cohesion: 0.67
Nodes (3): lunalib, psarc, rechimera-desktop

## Knowledge Gaps
- **588 isolated node(s):** `name`, `private`, `version`, `type`, `dev` (+583 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 788 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **11 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `IgFile` connect `IGHW Reader & Animation Decode` to `Tie Parsing (V2)`, `Tie Parsing (TOD)`, `Moby Parsing (RFOM)`, `Sound Extraction (SCREAM/ADPCM)`, `Foliage Decode (RFOM)`, `Zone Parsing (V2)`, `Gameplay Placements Decode`, `Shader Parsing (V2)`, `IgFile Tests`, `Tie Parsing (RFOM)`, `Asset Lookup Table`, `Moby Parsing (V2)`, `Stream Helper (byte IO)`, `Zone Parsing (TOD)`, `Moby Parsing (TOD)`?**
  _High betweenness centrality (0.065) - this node is a cross-community bridge._
- **Why does `Skeleton` connect `IGHW Reader & Animation Decode` to `Tauri Backend Commands (main.rs)`, `Cache & Extraction Pipeline`, `Binary FBX Export`, `GLTF/GLB Export Builder`, `ASCII FBX Export`, `Moby Parsing (V2)`?**
  _High betweenness centrality (0.019) - this node is a cross-community bridge._
- **Why does `react` connect `UI Component Library` to `Frontend API & R2 Wizard`, `3D Viewport & Scene Views`, `App Shell & Status UI`, `Cache Library & Sound Player UI`, `Open-Level Modal`, `Redux Store & Persistence`, `GLB Preview & Export Options`, `Asset Preview & Inspector`, `About / Whats-New Modals`, `Settings Modal & i18n`, `Character Modal (three-fiber)`, `Asset Workbench View`, `Build Config (Vite/pkg)`, `PSARC Modal & Tools`, `Updater UI & Title Bar`, `Modal & UI Hooks`, `Tab Container & View Meta`, `Asset-Lookup Tools View`, `Docs Modal`, `Menu Bar`?**
  _High betweenness centrality (0.017) - this node is a cross-community bridge._
- **What connects `name`, `private`, `version` to the rest of the system?**
  _588 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Tauri Backend Commands (main.rs)` be split into smaller, more focused modules?**
  _Cohesion score 0.06088154269972452 - nodes in this community are weakly interconnected._
- **Should `Cache & Extraction Pipeline` be split into smaller, more focused modules?**
  _Cohesion score 0.06455445544554456 - nodes in this community are weakly interconnected._
- **Should `IGHW Reader & Animation Decode` be split into smaller, more focused modules?**
  _Cohesion score 0.050793650793650794 - nodes in this community are weakly interconnected._