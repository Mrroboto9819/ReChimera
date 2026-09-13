use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use lunalib::{AnimProfile, DecodedClip, LevelLayout, ShaderInfo};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;

use crate::cache::{
    cache_root, decode_clips_for_moby, decode_clips_for_moby_inline, resolve_profile_for_folder,
    tie_as_moby, AnimsetIndex,
};
use crate::{resolve_shader_textures, AssetMeshesDto};

#[derive(Deserialize)]
pub struct GlbExportOptions {
    pub include_mesh: bool,
    pub include_materials: bool,
    pub include_armature: bool,
    pub extra_clips: Vec<ClipPick>,
    #[serde(default)]
    pub texture_max_dim: Option<u32>,
}

#[derive(Deserialize)]
pub struct ClipPick {
    pub animset_hash: String,
    pub clip_indices: Vec<u32>,
}

#[tauri::command]
pub fn export_moby_glb_with_options(
    level_folder: String,
    asset_tuid_hex: String,
    out_path: String,
    options: GlbExportOptions,
) -> Result<u64, String> {
    let level_path = std::path::Path::new(&level_folder);
    let target_tuid = u64::from_str_radix(
        asset_tuid_hex
            .trim_start_matches("0x")
            .trim_start_matches("0X"),
        16,
    )
    .map_err(|e| format!("parse asset tuid {asset_tuid_hex}: {e}"))?;

    let layout = lunalib::detect_layout(level_path).map_err(|e| e.to_string())?;
    let engine = lunalib::engine_for_layout(layout);

    let mut moby_asset: Option<lunalib::MobyAsset> = None;
    let mut tie_asset: Option<lunalib::TieAsset> = None;
    engine
        .read_mobys(
            level_path,
            Some(&[target_tuid]),
            &mut |_: usize| {},
            &mut |a: lunalib::MobyAsset| {
                if a.tuid == target_tuid {
                    moby_asset = Some(a);
                }
            },
        )
        .map_err(|e| e.to_string())?;
    if moby_asset.is_none() {
        engine
            .read_ties(
                level_path,
                Some(&[target_tuid]),
                &mut |_: usize| {},
                &mut |a: lunalib::TieAsset| {
                    if a.tuid == target_tuid {
                        tie_asset = Some(a);
                    }
                },
            )
            .map_err(|e| e.to_string())?;
    }

    let mut synthetic_from_tie = false;
    let asset = match moby_asset {
        Some(a) => a,
        None => {
            let tie =
                tie_asset.ok_or_else(|| format!("asset {asset_tuid_hex} not found in level"))?;
            synthetic_from_tie = true;
            tie_as_moby(&tie)
        }
    };

    let asset = if options.include_armature {
        asset
    } else {
        let mut a = asset;
        a.skeleton = None;
        a
    };

    if !options.include_mesh {
        return Err("include_mesh=false is not yet supported (would produce empty GLB)".into());
    }

    let shaders = if options.include_materials {
        {
            let loaded = lunalib::engine_for_layout(layout).read_shaders(level_path);
            if matches!(layout, LevelLayout::V2) {
                loaded.map_err(|e| e.to_string())?
            } else {
                loaded.unwrap_or_default()
            }
        }
    } else {
        HashMap::new()
    };

    let texture_pngs = if options.include_materials {
        let mut needed: HashSet<u32> = HashSet::new();
        for bangle in &asset.bangles {
            for m in &bangle.meshes {
                let (a, n, e) = resolve_shader_textures(
                    &shaders,
                    &asset.shader_tuids,
                    m.shader_index as usize,
                );
                for id in [a, n, e].into_iter().flatten() {
                    needed.insert(id);
                }
            }
        }

        let max_dim = options.texture_max_dim.unwrap_or(u32::MAX);
        let needed_ids: Vec<u32> = needed.iter().copied().collect();

        let cached_lookup = || -> HashMap<u32, Vec<u8>> {
            let cache_root = cache_root(&level_folder);
            let tex_dir = cache_root.join("textures");
            let mut out = HashMap::with_capacity(needed.len());
            for id in &needed_ids {
                let path = tex_dir.join(format!("{id}.png"));
                if let Ok(bytes) = fs::read(&path) {
                    let resized = if max_dim != 0 && max_dim < u32::MAX {
                        lunalib::downsample_png_to(&bytes, max_dim).unwrap_or(bytes)
                    } else {
                        bytes
                    };
                    out.insert(*id, resized);
                }
            }
            out
        };

        // For sub-512 quality presets, just resize cached PNGs (every layout writes them).
        if max_dim <= 512 && max_dim != 0 {
            cached_lookup()
        } else {
            match layout {
                LevelLayout::V2 => match lunalib::bulk_extract_pngs(level_path, Some(&needed_ids), max_dim) {
                    Ok(pngs) => pngs.into_iter().collect(),
                    Err(e) => {
                        eprintln!("warn: V2 bulk_extract_pngs failed: {e} — falling back to cache");
                        cached_lookup()
                    }
                },
                LevelLayout::Tod => match lunalib::read_textures_old(level_path) {
                    Ok(textures) => {
                        use rayon::prelude::*;
                        let want: HashSet<u32> = needed.iter().copied().collect();
                        let out: HashMap<u32, Vec<u8>> = textures
                            .par_iter()
                            .filter(|t| want.contains(&t.id))
                            .filter_map(|t| {
                                lunalib::texture_to_png(t).map(|png| {
                                    let resized = if max_dim != 0 && max_dim < u32::MAX {
                                        lunalib::downsample_png_to(&png, max_dim).unwrap_or(png)
                                    } else {
                                        png
                                    };
                                    (t.id, resized)
                                })
                            })
                            .collect();
                        out
                    }
                    Err(e) => {
                        eprintln!("warn: TOD texture re-extract failed: {e} — falling back to cache");
                        cached_lookup()
                    }
                },
                LevelLayout::Rfom => match lunalib::read_textures_rfom(level_path) {
                    Ok(textures) => {
                        use rayon::prelude::*;
                        let want: HashSet<u32> = needed.iter().copied().collect();
                        let out: HashMap<u32, Vec<u8>> = textures
                            .par_iter()
                            .filter(|t| want.contains(&t.id))
                            .filter_map(|t| {
                                lunalib::texture_rfom_to_png(t).map(|png| {
                                    let resized = if max_dim != 0 && max_dim < u32::MAX {
                                        lunalib::downsample_png_to(&png, max_dim).unwrap_or(png)
                                    } else {
                                        png
                                    };
                                    (t.id, resized)
                                })
                            })
                            .collect();
                        out
                    }
                    Err(e) => {
                        eprintln!("warn: RFOM texture re-extract failed: {e} — falling back to cache");
                        cached_lookup()
                    }
                },
            }
        }
    } else {
        HashMap::new()
    };

    let mut clips: Vec<DecodedClip> = Vec::new();
    let animset_index = if matches!(layout, LevelLayout::V2) {
        AnimsetIndex::build(level_path).ok()
    } else {
        None
    };
    let animsets_path = level_path.join("animsets.dat");
    let mut animsets_file = if matches!(layout, LevelLayout::V2) {
        std::fs::File::open(&animsets_path).ok()
    } else {
        None
    };

    if options.include_armature && !synthetic_from_tie {
        let trans_shift = asset
            .skeleton
            .as_ref()
            .map(|s| s.translation_shift)
            .unwrap_or(0);
        let scale_shift_v = asset
            .skeleton
            .as_ref()
            .map(|s| s.scale_shift)
            .unwrap_or(0);
        let profile = resolve_profile_for_folder(&level_folder, None);
        let mconv = profile.matrix_convention();
        let pos_scale = mconv.shift_scale(trans_shift);
        let scale_scale = mconv.shift_scale(scale_shift_v);

        if matches!(layout, LevelLayout::Rfom | LevelLayout::Tod)
            && !asset.rfom_anim_offsets.is_empty()
        {
            if let Some(skel) = asset.skeleton.as_ref() {
                lunalib::skeleton::dump_skeleton_bind(asset.tuid, skel);
                let main_dat = match layout {
                    LevelLayout::Tod => "main.dat",
                    _ => "ps3levelmain.dat",
                };
                clips.extend(decode_clips_for_moby_inline(
                    level_path,
                    main_dat,
                    &asset.rfom_anim_offsets,
                    pos_scale,
                    scale_scale,
                    skel,
                    layout,
                    asset.tuid,
                    profile,
                ));
            }
        }

        if let (Some(hash), Some(idx), Some(file)) = (
            asset.animset_hash,
            animset_index.as_ref(),
            animsets_file.as_mut(),
        ) {
            let sb = asset
                .skeleton
                .as_ref()
                .map(|s| s.bones.len() as u16)
                .unwrap_or(0);
            clips.extend(decode_clips_for_moby(
                level_path,
                idx,
                file,
                hash,
                pos_scale,
                scale_scale,
                sb,
                asset.skeleton.as_ref(),
                profile,
            ));
        }

        if !options.extra_clips.is_empty() && !matches!(layout, LevelLayout::V2) {
            eprintln!(
                "warn: extra_clips picker is V2-only; {} pick(s) ignored for {} layout",
                options.extra_clips.len(),
                layout.tag()
            );
        }

        if let (Some(idx), Some(file)) = (animset_index.as_ref(), animsets_file.as_mut()) {
            for pick in &options.extra_clips {
                let hex = pick.animset_hash.trim_start_matches("0x").trim_start_matches("0X");
                let Ok(hash) = u64::from_str_radix(hex, 16) else { continue };
                if Some(hash) == asset.animset_hash {
                    continue;
                }
                let trans_shift = asset
                    .skeleton
                    .as_ref()
                    .map(|s| s.translation_shift)
                    .unwrap_or(0);
                let scale_shift_v = asset
                    .skeleton
                    .as_ref()
                    .map(|s| s.scale_shift)
                    .unwrap_or(0);
                let mconv = profile.matrix_convention();
                let pos_scale = mconv.shift_scale(trans_shift);
                let scale_scale = mconv.shift_scale(scale_shift_v);
                let sb = asset
                    .skeleton
                    .as_ref()
                    .map(|s| s.bones.len() as u16)
                    .unwrap_or(0);
                let extras = decode_clips_for_moby(
                    level_path,
                    idx,
                    file,
                    hash,
                    pos_scale,
                    scale_scale,
                    sb,
                    asset.skeleton.as_ref(),
                    profile,
                );
                if pick.clip_indices.is_empty() {
                    clips.extend(extras);
                } else {
                    let want: HashSet<u32> = pick.clip_indices.iter().copied().collect();
                    for (i, clip) in extras.into_iter().enumerate() {
                        if want.contains(&(i as u32)) {
                            clips.push(clip);
                        }
                    }
                }
            }
        }
    }

    let glb_bytes = lunalib::write_moby_glb_full(&asset, &clips, &shaders, &texture_pngs)
        .map_err(|e| format!("GLB build failed: {e}"))?;

    fs::write(&out_path, &glb_bytes)
        .map_err(|e| format!("write {out_path}: {e}"))?;
    Ok(glb_bytes.len() as u64)
}

#[tauri::command]
pub fn export_moby_fbx_with_options(
    level_folder: String,
    asset_tuid_hex: String,
    out_path: String,
    options: GlbExportOptions,
) -> Result<u64, String> {
    let level_path = std::path::Path::new(&level_folder);
    let target_tuid = u64::from_str_radix(
        asset_tuid_hex
            .trim_start_matches("0x")
            .trim_start_matches("0X"),
        16,
    )
    .map_err(|e| format!("parse asset tuid {asset_tuid_hex}: {e}"))?;

    let layout = lunalib::detect_layout(level_path).map_err(|e| e.to_string())?;
    let engine = lunalib::engine_for_layout(layout);

    let mut moby_asset: Option<lunalib::MobyAsset> = None;
    let mut tie_asset: Option<lunalib::TieAsset> = None;
    engine
        .read_mobys(
            level_path,
            Some(&[target_tuid]),
            &mut |_: usize| {},
            &mut |a: lunalib::MobyAsset| {
                if a.tuid == target_tuid {
                    moby_asset = Some(a);
                }
            },
        )
        .map_err(|e| e.to_string())?;
    if moby_asset.is_none() {
        engine
            .read_ties(
                level_path,
                Some(&[target_tuid]),
                &mut |_: usize| {},
                &mut |a: lunalib::TieAsset| {
                    if a.tuid == target_tuid {
                        tie_asset = Some(a);
                    }
                },
            )
            .map_err(|e| e.to_string())?;
    }

    let asset = match moby_asset {
        Some(a) => a,
        None => {
            let tie = tie_asset
                .ok_or_else(|| format!("asset {asset_tuid_hex} not found in level"))?;
            tie_as_moby(&tie)
        }
    };

    if !options.include_mesh {
        return Err("include_mesh=false is not yet supported".into());
    }

    let shaders = if options.include_materials {
        {
            let loaded = lunalib::engine_for_layout(layout).read_shaders(level_path);
            if matches!(layout, LevelLayout::V2) {
                loaded.map_err(|e| e.to_string())?
            } else {
                loaded.unwrap_or_default()
            }
        }
    } else {
        HashMap::new()
    };

    let texture_pngs = if options.include_materials {
        let mut needed: HashSet<u32> = HashSet::new();
        for bangle in &asset.bangles {
            for m in &bangle.meshes {
                let (a, _n, _e) =
                    resolve_shader_textures(&shaders, &asset.shader_tuids, m.shader_index as usize);
                if let Some(id) = a {
                    needed.insert(id);
                }
            }
        }
        let max_dim = options.texture_max_dim.unwrap_or(u32::MAX);
        let cache_root = cache_root(&level_folder);
        let tex_dir = cache_root.join("textures");
        let mut out = HashMap::with_capacity(needed.len());
        for id in &needed {
            let path = tex_dir.join(format!("{id}.png"));
            if let Ok(bytes) = fs::read(&path) {
                let resized = if max_dim != 0 && max_dim < u32::MAX {
                    lunalib::downsample_png_to(&bytes, max_dim).unwrap_or(bytes)
                } else {
                    bytes
                };
                out.insert(*id, resized);
            }
        }
        out
    } else {
        HashMap::new()
    };

    let mut clips: Vec<DecodedClip> = Vec::new();
    if options.include_armature && asset.skeleton.is_some() {
        let animset_index = if matches!(layout, LevelLayout::V2) {
            AnimsetIndex::build(level_path).ok()
        } else {
            None
        };
        let mut animsets_file = if matches!(layout, LevelLayout::V2) {
            std::fs::File::open(level_path.join("animsets.dat")).ok()
        } else {
            None
        };
        let target_tuid_u = u64::from_str_radix(
            asset_tuid_hex
                .trim_start_matches("0x")
                .trim_start_matches("0X"),
            16,
        )
        .unwrap_or(0);
        let profile = resolve_profile_for_folder(&level_folder, None);
        if let Some((_asset_again, decoded)) = try_load_skinned_moby_for_level(
            level_path,
            layout,
            target_tuid_u,
            animset_index.as_ref(),
            animsets_file.as_mut(),
            profile,
        ) {
            clips = decoded;
        }
    }

    let fbx_bytes = lunalib::write_moby_fbx(&asset, &clips, &shaders, &texture_pngs)
        .map_err(|e| format!("FBX build failed: {e}"))?;
    fs::write(&out_path, &fbx_bytes).map_err(|e| format!("write {out_path}: {e}"))?;
    Ok(fbx_bytes.len() as u64)
}

#[tauri::command]
pub fn export_skybox(
    level_folder: String,
    format: String,
    out_path: String,
) -> Result<u64, String> {
    let root = cache_root(&level_folder);
    let file = match format.as_str() {
        "glb" => "skybox/sky.glb",
        "obj" => "skybox/sky.obj",
        "ply" => "skybox/sky.ply",
        "json" => "skybox/sky.json",
        other => return Err(format!("Unsupported skybox format: {other}")),
    };
    let src = root.join(file);
    if !src.is_file() {
        return Err(format!(
            "Skybox not in cache yet ({}). Re-extract the level.",
            file
        ));
    }
    fs::copy(&src, &out_path)
        .map_err(|e| format!("copy {} → {}: {e}", src.display(), out_path))
}

#[tauri::command]
pub fn read_cached_skybox_meta(level_folder: String) -> Result<serde_json::Value, String> {
    let root = cache_root(&level_folder);
    let meta = root.join("skybox/sky.json");
    if !meta.is_file() {
        return Err("no skybox in cache".into());
    }
    let bytes = fs::read(&meta).map_err(|e| format!("read {}: {e}", meta.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse skybox meta: {e}"))
}

#[tauri::command]
pub fn export_cached_moby_glb(
    level_folder: String,
    asset_tuid_hex: String,
    out_path: String,
) -> Result<u64, String> {
    let root = cache_root(&level_folder);
    let moby_glb = root.join("mobys").join(format!("{asset_tuid_hex}.glb"));
    let tie_glb = root.join("ties").join(format!("{asset_tuid_hex}.glb"));
    let cache_glb = if moby_glb.is_file() {
        moby_glb
    } else if tie_glb.is_file() {
        tie_glb
    } else {
        return Err(format!(
            "Cached GLB not found in mobys/ or ties/ for {asset_tuid_hex} — re-extract the level cache first"
        ));
    };
    fs::copy(&cache_glb, &out_path).map_err(|e| {
        format!("copy {} → {}: {e}", cache_glb.display(), out_path)
    })
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LevelGlbExportEvent {
    Phase { label: &'static str, total: usize },
    Progress { current: usize },
    Done { bytes_written: usize, instance_count: usize, asset_count: usize },
    Error { message: String },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LevelExportFormat {
    Glb { with_anims: bool },
    Fbx,
}

#[tauri::command]
pub fn export_level_glb(
    level_folder: String,
    out_path: String,
    on_event: Channel<LevelGlbExportEvent>,
) -> Result<(), String> {
    if let Err(message) = run_export_level(
        &level_folder,
        &out_path,
        &on_event,
        LevelExportFormat::Glb { with_anims: true },
    ) {
        let _ = on_event.send(LevelGlbExportEvent::Error { message: message.clone() });
        return Err(message);
    }
    Ok(())
}

#[tauri::command]
pub fn export_level_fbx(
    level_folder: String,
    out_path: String,
    on_event: Channel<LevelGlbExportEvent>,
) -> Result<(), String> {
    if let Err(message) =
        run_export_level(&level_folder, &out_path, &on_event, LevelExportFormat::Fbx)
    {
        let _ = on_event.send(LevelGlbExportEvent::Error { message: message.clone() });
        return Err(message);
    }
    Ok(())
}

fn try_load_skinned_moby_for_level(
    level_path: &Path,
    layout: LevelLayout,
    tuid: u64,
    animset_index: Option<&AnimsetIndex>,
    animsets_file: Option<&mut std::fs::File>,
    profile: AnimProfile,
) -> Option<(lunalib::MobyAsset, Vec<DecodedClip>)> {
    let mut asset: Option<lunalib::MobyAsset> = None;
    let _ = lunalib::engine_for_layout(layout).read_mobys(
        level_path,
        Some(&[tuid]),
        &mut |_: usize| {},
        &mut |a: lunalib::MobyAsset| {
            if a.tuid == tuid {
                asset = Some(a);
            }
        },
    );
    let asset = asset?;
    if asset.skeleton.is_none() {
        return None;
    }

    let (trans_shift, scale_shift_v) = {
        let s = asset.skeleton.as_ref()?;
        (s.translation_shift, s.scale_shift)
    };
    let mconv = profile.matrix_convention();
    let pos_scale = mconv.shift_scale(trans_shift);
    let scale_scale = mconv.shift_scale(scale_shift_v);

    let mut clips: Vec<DecodedClip> = Vec::new();

    if matches!(layout, LevelLayout::Rfom | LevelLayout::Tod)
        && !asset.rfom_anim_offsets.is_empty()
    {
        if let Some(skel) = asset.skeleton.as_ref() {
            let main_dat = match layout {
                LevelLayout::Tod => "main.dat",
                _ => "ps3levelmain.dat",
            };
            clips.extend(decode_clips_for_moby_inline(
                level_path,
                main_dat,
                &asset.rfom_anim_offsets,
                pos_scale,
                scale_scale,
                skel,
                layout,
                asset.tuid,
                profile,
            ));
        }
    }

    if matches!(layout, LevelLayout::V2) {
        if let (Some(hash), Some(idx), Some(file)) =
            (asset.animset_hash, animset_index, animsets_file)
        {
            let sb = asset
                .skeleton
                .as_ref()
                .map(|s| s.bones.len() as u16)
                .unwrap_or(0);
            clips.extend(decode_clips_for_moby(
                level_path,
                idx,
                file,
                hash,
                pos_scale,
                scale_scale,
                sb,
                asset.skeleton.as_ref(),
                profile,
            ));
        }
    }

    let _ = profile;
    Some((asset, clips))
}

fn run_export_level(
    level_folder: &str,
    out_path: &str,
    on_event: &Channel<LevelGlbExportEvent>,
    format: LevelExportFormat,
) -> Result<(), String> {
    use base64::engine::general_purpose::STANDARD as BASE64;

    let _ = on_event.send(LevelGlbExportEvent::Phase {
        label: "Reading placements",
        total: 1,
    });

    let mobys = crate::real_moby_layout(level_folder).unwrap_or_default();
    let ties = crate::real_tie_layout(level_folder).unwrap_or_default();

    if mobys.is_empty() && ties.is_empty() {
        return Err("No placements found in this level. Open and extract the level cache first.".into());
    }

    let level_path = Path::new(level_folder);
    let layout = lunalib::detect_layout(level_path).ok();
    let with_anims = matches!(format, LevelExportFormat::Glb { with_anims: true })
        || matches!(format, LevelExportFormat::Fbx);

    let mut skinned_classes: HashMap<String, (lunalib::MobyAsset, Vec<DecodedClip>)> = HashMap::new();
    let mut shaders_for_anim: HashMap<u64, ShaderInfo> = HashMap::new();

    if with_anims {
        if let Some(lay) = layout {
            let mut unique_tuids: HashSet<String> = HashSet::new();
            for inst in &mobys {
                unique_tuids.insert(inst.asset_tuid.clone());
            }
            shaders_for_anim = lunalib::engine_for_layout(lay).read_shaders(level_path).unwrap_or_default();
            let animset_index = if matches!(lay, LevelLayout::V2) {
                AnimsetIndex::build(level_path).ok()
            } else {
                None
            };
            let mut animsets_file = if matches!(lay, LevelLayout::V2) {
                std::fs::File::open(level_path.join("animsets.dat")).ok()
            } else {
                None
            };

            let _ = on_event.send(LevelGlbExportEvent::Phase {
                label: "Loading skinned characters",
                total: unique_tuids.len(),
            });
            let mut scanned = 0usize;
            for tuid_hex in &unique_tuids {
                scanned += 1;
                if scanned % 4 == 0 {
                    let _ = on_event.send(LevelGlbExportEvent::Progress { current: scanned });
                }
                let tuid_u = match u64::from_str_radix(
                    tuid_hex.trim_start_matches("0x").trim_start_matches("0X"),
                    16,
                ) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some((asset, clips)) = try_load_skinned_moby_for_level(
                    level_path,
                    lay,
                    tuid_u,
                    animset_index.as_ref(),
                    animsets_file.as_mut(),
                    resolve_profile_for_folder(level_folder, None),
                ) {
                    skinned_classes.insert(tuid_hex.clone(), (asset, clips));
                }
            }
            let _ = on_event.send(LevelGlbExportEvent::Progress { current: scanned });
            eprintln!(
                "[level-glb] skinned classes detected: {} of {} unique moby tuids",
                skinned_classes.len(),
                unique_tuids.len()
            );
        }
    }

    let _ = on_event.send(LevelGlbExportEvent::Phase {
        label: "Loading assets",
        total: mobys.len() + ties.len(),
    });

    let cache_root = cache_root(level_folder);
    let mut asset_idx_by_tuid: HashMap<(String, &'static str), usize> = HashMap::new();
    let mut assets: Vec<lunalib::level_glb::LevelGlbAsset> = Vec::new();
    let mut needed_textures: HashSet<u32> = HashSet::new();

    let mut load_one = |kind_name: &'static str, asset_tuid: &str| -> Result<Option<usize>, String> {
        let key = (asset_tuid.to_string(), kind_name);
        if let Some(idx) = asset_idx_by_tuid.get(&key) {
            return Ok(Some(*idx));
        }
        let folder = match kind_name {
            "moby" => "mobys",
            "detail" => "details",
            "shrub" => "shrubs",
            "foliage" => "foliage",
            _ => "ties",
        };
        let primary = cache_root.join(folder).join(format!("{}.json", asset_tuid));
        let bytes = match fs::read(&primary) {
            Ok(b) => b,
            Err(_) => {
                // Fallback: detail JSONs may have been routed to ties/ in older
                // caches, or vice versa.
                let alt = if folder == "details" {
                    cache_root.join("ties").join(format!("{}.json", asset_tuid))
                } else if folder == "ties" {
                    cache_root.join("details").join(format!("{}.json", asset_tuid))
                } else {
                    primary.clone()
                };
                match fs::read(&alt) {
                    Ok(b) => b,
                    Err(_) => return Ok(None),
                }
            }
        };
        let dto: AssetMeshesDto = serde_json::from_slice(&bytes)
            .map_err(|e| format!("parse {}: {e}", primary.display()))?;
        let mut submeshes: Vec<lunalib::level_glb::LevelGlbSubmesh> =
            Vec::with_capacity(dto.submeshes.len());
        for m in &dto.submeshes {
            let positions = decode_f32_b64(&m.positions_b64, &BASE64)?;
            let uvs = decode_f32_b64(&m.uvs_b64, &BASE64)?;
            let indices = decode_u32_b64(&m.indices_b64, &BASE64)?;
            if let Some(id) = m.albedo_id {
                needed_textures.insert(id);
            }
            submeshes.push(lunalib::level_glb::LevelGlbSubmesh {
                positions,
                uvs,
                indices,
                albedo_id: m.albedo_id,
            });
        }
        let idx = assets.len();
        assets.push(lunalib::level_glb::LevelGlbAsset {
            name: if dto.name.is_empty() { asset_tuid.to_string() } else { dto.name.clone() },
            submeshes,
        });
        asset_idx_by_tuid.insert(key, idx);
        Ok(Some(idx))
    };

    let mut instances: Vec<lunalib::level_glb::LevelGlbInstance> = Vec::new();
    let mut progress: usize = 0;
    let mut moby_count = 0usize;
    let mut tie_count = 0usize;
    let mut detail_count = 0usize;
    let mut shrub_count = 0usize;
    let mut foliage_count = 0usize;

    for inst in &mobys {
        progress += 1;
        if progress % 16 == 0 {
            let _ = on_event.send(LevelGlbExportEvent::Progress { current: progress });
        }
        if skinned_classes.contains_key(&inst.asset_tuid) {
            continue;
        }
        if let Some(idx) = load_one("moby", &inst.asset_tuid)? {
            instances.push(lunalib::level_glb::LevelGlbInstance {
                asset_idx: idx,
                name: format!("moby:{}:{}", inst.name, inst.tuid),
                translation: inst.position,
                rotation: inst.quaternion,
                scale: inst.scale,
            });
            moby_count += 1;
        }
    }
    for inst in &ties {
        progress += 1;
        if progress % 16 == 0 {
            let _ = on_event.send(LevelGlbExportEvent::Progress { current: progress });
        }
        let kind_name: &'static str = match inst.kind {
            "detail" => "detail",
            "shrub" => "shrub",
            "foliage" => "foliage",
            _ => "tie",
        };
        if let Some(idx) = load_one(kind_name, &inst.asset_tuid)? {
            instances.push(lunalib::level_glb::LevelGlbInstance {
                asset_idx: idx,
                name: format!("{}:{}:{}", kind_name, inst.name, inst.tuid),
                translation: inst.position,
                rotation: inst.quaternion,
                scale: inst.scale,
            });
            match kind_name {
                "detail" => detail_count += 1,
                "shrub" => shrub_count += 1,
                "foliage" => foliage_count += 1,
                _ => tie_count += 1,
            }
        }
    }

    let _ = on_event.send(LevelGlbExportEvent::Progress { current: progress });

    eprintln!(
        "[level-glb] placements baked: {} mobys, {} ties, {} details, {} shrubs, {} foliage",
        moby_count, tie_count, detail_count, shrub_count, foliage_count
    );

    let ufrag_count = load_ufrags_into_level(
        &cache_root,
        &mut assets,
        &mut instances,
        &mut needed_textures,
        on_event,
    )?;
    eprintln!("[level-glb] terrain baked: {} ufrags", ufrag_count);

    let sky_added = load_skybox_into_level(
        level_folder,
        &mut assets,
        &mut instances,
        on_event,
    );
    eprintln!(
        "[level-glb] skybox baked: {}",
        if sky_added { "yes (dome geometry)" } else { "no" }
    );

    let mut skinned_placements: Vec<lunalib::level_glb::SkinnedPlacement> = Vec::new();
    if with_anims && !skinned_classes.is_empty() {
        for (_tuid, (asset, _clips)) in &skinned_classes {
            for bangle in &asset.bangles {
                for m in &bangle.meshes {
                    let (a, n, e) = resolve_shader_textures(
                        &shaders_for_anim,
                        &asset.shader_tuids,
                        m.shader_index as usize,
                    );
                    for id in [a, n, e].into_iter().flatten() {
                        needed_textures.insert(id);
                    }
                }
            }
        }
        for inst in &mobys {
            if let Some((asset, clips)) = skinned_classes.get(&inst.asset_tuid) {
                skinned_placements.push(lunalib::level_glb::SkinnedPlacement {
                    asset: asset.clone(),
                    clips: clips.clone(),
                    name: format!("moby:{}:{}", inst.name, inst.tuid),
                    translation: inst.position,
                    rotation: inst.quaternion,
                    scale: inst.scale,
                });
            }
        }
        eprintln!(
            "[level-glb] skinned placements queued: {} (across {} classes)",
            skinned_placements.len(),
            skinned_classes.len()
        );
    }

    let _ = on_event.send(LevelGlbExportEvent::Phase {
        label: "Loading textures",
        total: needed_textures.len(),
    });

    let mut textures: HashMap<u32, Vec<u8>> = HashMap::new();
    let textures_dir = cache_root.join("textures");
    let mut tex_progress = 0usize;
    for tex_id in &needed_textures {
        tex_progress += 1;
        if tex_progress % 16 == 0 {
            let _ = on_event.send(LevelGlbExportEvent::Progress { current: tex_progress });
        }
        let path = textures_dir.join(format!("{}.png", tex_id));
        if let Ok(b) = fs::read(&path) {
            textures.insert(*tex_id, b);
        }
    }
    let _ = on_event.send(LevelGlbExportEvent::Progress { current: tex_progress });

    let phase_label: &'static str = match format {
        LevelExportFormat::Glb { with_anims: true } if !skinned_placements.is_empty() => {
            "Writing animated GLB"
        }
        LevelExportFormat::Glb { .. } => "Writing GLB",
        LevelExportFormat::Fbx if !skinned_placements.is_empty() => "Writing animated FBX",
        LevelExportFormat::Fbx => "Writing FBX",
    };
    let _ = on_event.send(LevelGlbExportEvent::Phase {
        label: phase_label,
        total: 1,
    });

    let out_bytes = match format {
        LevelExportFormat::Glb { with_anims: true } if !skinned_placements.is_empty() => {
            lunalib::level_glb::write_animated_level_glb(
                &assets,
                &instances,
                &skinned_placements,
                &shaders_for_anim,
                &textures,
            )
            .map_err(|e| format!("write_animated_level_glb: {e}"))?
        }
        LevelExportFormat::Glb { .. } => {
            lunalib::level_glb::write_static_level_glb(&assets, &instances, &textures)
                .map_err(|e| format!("write_static_level_glb: {e}"))?
        }
        LevelExportFormat::Fbx => {
            lunalib::write_animated_level_fbx(
                &assets,
                &instances,
                &skinned_placements,
                &shaders_for_anim,
                &textures,
            )
            .map_err(|e| format!("write_animated_level_fbx (binary): {e}"))?
        }
    };

    fs::write(out_path, &out_bytes).map_err(|e| format!("write {out_path}: {e}"))?;

    let _ = on_event.send(LevelGlbExportEvent::Done {
        bytes_written: out_bytes.len(),
        instance_count: instances.len(),
        asset_count: assets.len(),
    });

    Ok(())
}

fn load_ufrags_into_level(
    cache_root: &Path,
    assets: &mut Vec<lunalib::level_glb::LevelGlbAsset>,
    instances: &mut Vec<lunalib::level_glb::LevelGlbInstance>,
    needed_textures: &mut HashSet<u32>,
    on_event: &Channel<LevelGlbExportEvent>,
) -> Result<usize, String> {
    use base64::engine::general_purpose::STANDARD as BASE64;

    let ufrags_dir = cache_root.join("ufrags");
    if !ufrags_dir.is_dir() {
        return Ok(0);
    }

    let entries: Vec<PathBuf> = match fs::read_dir(&ufrags_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
            .collect(),
        Err(_) => return Ok(0),
    };

    let _ = on_event.send(LevelGlbExportEvent::Phase {
        label: "Loading terrain",
        total: entries.len(),
    });

    let mut done = 0usize;
    for path in entries {
        done += 1;
        if done % 32 == 0 {
            let _ = on_event.send(LevelGlbExportEvent::Progress { current: done });
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let dto: crate::UFragMeshDto = match serde_json::from_slice(&bytes) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("warn: parse {}: {e}", path.display());
                continue;
            }
        };
        let positions = decode_f32_b64(&dto.mesh.positions_b64, &BASE64)?;
        let uvs = decode_f32_b64(&dto.mesh.uvs_b64, &BASE64)?;
        let indices = decode_u32_b64(&dto.mesh.indices_b64, &BASE64)?;
        if positions.is_empty() || indices.is_empty() {
            continue;
        }
        if let Some(id) = dto.mesh.albedo_id {
            needed_textures.insert(id);
        }
        let asset_idx = assets.len();
        assets.push(lunalib::level_glb::LevelGlbAsset {
            name: format!("ufrag_{}", dto.tuid),
            submeshes: vec![lunalib::level_glb::LevelGlbSubmesh {
                positions,
                uvs,
                indices,
                albedo_id: dto.mesh.albedo_id,
            }],
        });
        instances.push(lunalib::level_glb::LevelGlbInstance {
            asset_idx,
            name: format!("ufrag:{}", dto.tuid),
            translation: dto.position,
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        });
    }
    let _ = on_event.send(LevelGlbExportEvent::Progress { current: done });
    Ok(done)
}

fn load_skybox_into_level(
    level_folder: &str,
    assets: &mut Vec<lunalib::level_glb::LevelGlbAsset>,
    instances: &mut Vec<lunalib::level_glb::LevelGlbInstance>,
    on_event: &Channel<LevelGlbExportEvent>,
) -> bool {
    let _ = on_event.send(LevelGlbExportEvent::Phase {
        label: "Loading sky",
        total: 1,
    });

    let sky = match lunalib::read_skybox_rfom(Path::new(level_folder)) {
        Ok(Some(s)) => s,
        Ok(None) => return false,
        Err(e) => {
            eprintln!("warn: skybox read for export: {e}");
            return false;
        }
    };

    if sky.vertices.is_empty() || sky.indices.is_empty() {
        return false;
    }

    let mut positions: Vec<f32> = Vec::with_capacity(sky.vertices.len() * 3);
    let mut uvs: Vec<f32> = Vec::with_capacity(sky.vertices.len() * 2);
    for v in &sky.vertices {
        positions.extend_from_slice(v);
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
        let nx = v[0] / len;
        let ny = v[1] / len;
        let nz = v[2] / len;
        let u = 0.5 + nz.atan2(nx) / (2.0 * std::f32::consts::PI);
        let vv = 0.5 - ny.asin() / std::f32::consts::PI;
        uvs.push(u);
        uvs.push(vv);
    }

    let asset_idx = assets.len();
    assets.push(lunalib::level_glb::LevelGlbAsset {
        name: "skybox_dome".into(),
        submeshes: vec![lunalib::level_glb::LevelGlbSubmesh {
            positions,
            uvs,
            indices: sky.indices.clone(),
            albedo_id: None,
        }],
    });
    instances.push(lunalib::level_glb::LevelGlbInstance {
        asset_idx,
        name: "skybox".into(),
        translation: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    });
    let _ = on_event.send(LevelGlbExportEvent::Progress { current: 1 });
    true
}

fn decode_f32_b64(
    s: &str,
    engine: &base64::engine::general_purpose::GeneralPurpose,
) -> Result<Vec<f32>, String> {
    use base64::Engine as _;
    let bytes = engine.decode(s).map_err(|e| format!("base64: {e}"))?;
    if bytes.len() % 4 != 0 {
        return Err(format!("f32 buffer length {} not /4", bytes.len()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

fn decode_u32_b64(
    s: &str,
    engine: &base64::engine::general_purpose::GeneralPurpose,
) -> Result<Vec<u32>, String> {
    use base64::Engine as _;
    let bytes = engine.decode(s).map_err(|e| format!("base64: {e}"))?;
    if bytes.len() % 4 != 0 {
        return Err(format!("u32 buffer length {} not /4", bytes.len()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

#[tauri::command]
pub fn export_texture_png(
    level_folder: String,
    tex_id: u32,
    out_path: String,
) -> Result<u64, String> {
    let cache_root = cache_root(&level_folder);
    let src = cache_root.join("textures").join(format!("{}.png", tex_id));
    fs::copy(&src, &out_path)
        .map_err(|e| format!("copy {} → {}: {e}", src.display(), out_path))
}

#[tauri::command]
pub fn export_texture_dds(
    level_folder: String,
    tex_id: u32,
    out_path: String,
) -> Result<u64, String> {
    let cache_root = cache_root(&level_folder);
    let src = cache_root.join("textures").join(format!("{}.png", tex_id));
    let png_bytes =
        fs::read(&src).map_err(|e| format!("read {}: {e}", src.display()))?;
    let dds_bytes = lunalib::dds::png_to_uncompressed_dds(&png_bytes)
        .map_err(|e| format!("png→dds: {e}"))?;
    fs::write(&out_path, &dds_bytes)
        .map_err(|e| format!("write {}: {e}", out_path))?;
    Ok(dds_bytes.len() as u64)
}
