use std::fs::File;

use std::path::Path;

use lunalib::{
    animation_section_offsets, decode_animation, decode_animation_with_skel, detect_layout,
    read_animation_control, read_animation_header, read_animation_header_at, AssetKind,
    AssetLookup, IgFile, LevelLayout,
};
use crate::cache::resolve_profile_for_folder;
use crate::{open_lookup, DecodedBoneDto, DecodedClipDto};
use serde::Serialize;




#[derive(Serialize)]
pub(crate) struct AnimsetSummaryDto {

    tuid_hex: String,

    name: String,

    num_frames: u16,
    frame_rate: f32,

    num_bones: u16,
    looping: bool,
}


#[tauri::command]
pub(crate) fn list_animset_clips(level_folder: String) -> Result<Vec<AnimsetSummaryDto>, String> {
    match lunalib::detect_layout(Path::new(&level_folder)) {
        Ok(lunalib::LevelLayout::Tod) | Ok(lunalib::LevelLayout::Rfom) => {
            return Ok(Vec::new());
        }
        _ => {}
    }
    let mut lookup = open_lookup(&level_folder)?;
    let ptrs = lookup
        .pointers(AssetKind::Animset)
        .map_err(|e| format!("read animset table: {e}"))?;

    let path = Path::new(&level_folder).join("animsets.dat");
    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {

            eprintln!("list_animset_clips: no animsets.dat at {} ({e})", path.display());
            return Ok(Vec::new());
        }
    };

    let mut out: Vec<AnimsetSummaryDto> = Vec::new();
    use std::io::{Read, Seek, SeekFrom};
    for ptr in ptrs {
        if let Err(e) = file.seek(SeekFrom::Start(u64::from(ptr.offset))) {
            eprintln!("list_animset_clips: seek failed for 0x{:016X}: {e}", ptr.tuid);
            continue;
        }
        let mut buf = vec![0u8; ptr.length as usize];
        if let Err(e) = file.read_exact(&mut buf) {
            eprintln!("list_animset_clips: read failed for 0x{:016X}: {e}", ptr.tuid);
            continue;
        }
        let mut ig = match IgFile::open(std::io::Cursor::new(buf)) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let h = match read_animation_header(&mut ig) {
            Ok(Some(h)) => h,
            _ => continue,
        };
        out.push(AnimsetSummaryDto {
            tuid_hex: format!("0x{:016X}", ptr.tuid),
            name: h.name.clone(),
            num_frames: h.num_frames,
            frame_rate: h.frame_rate,
            num_bones: h.num_bones,
            looping: h.is_looping(),
        });
    }

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}


#[tauri::command]
pub(crate) fn fetch_animset_clip(
    level_folder: String,
    animset_hash_hex: String,
    position_scale: f32,
    scale_scale: f32,
) -> Result<DecodedClipDto, String> {
    let target = parse_hex_u64(&animset_hash_hex)?;


    let mut lookup = open_lookup(&level_folder)?;
    let ptrs = lookup
        .pointers(AssetKind::Animset)
        .map_err(|e| format!("read animset table: {e}"))?;
    let ptr = ptrs
        .iter()
        .find(|p| p.tuid == target)
        .ok_or_else(|| format!("animset 0x{:016X} not in 0x1D700 table", target))?;


    let path = Path::new(&level_folder).join("animsets.dat");
    let mut file =
        File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(u64::from(ptr.offset)))
        .map_err(|e| format!("seek animsets.dat: {e}"))?;
    let mut buf = vec![0u8; ptr.length as usize];
    file.read_exact(&mut buf)
        .map_err(|e| format!("read animsets.dat: {e}"))?;


    let mut ig = IgFile::open(std::io::Cursor::new(buf))
        .map_err(|e| format!("animset IGHW: {e}"))?;
    let header = read_animation_header(&mut ig)
        .map_err(|e| format!("animation header: {e}"))?
        .ok_or_else(|| {
            "animset chunk has no 0xF000 Animation section".to_string()
        })?;
    let ctrl = read_animation_control(&mut ig, &header)
        .map_err(|e| format!("animation control: {e}"))?;
    let clip = decode_animation(&mut ig, &header, &ctrl, position_scale, scale_scale)
        .map_err(|e| format!("animation decode: {e}"))?;


    Ok(DecodedClipDto {
        name: clip.name,
        num_frames: clip.num_frames,
        frame_rate: clip.frame_rate,
        looping: clip.looping,
        bones: clip
            .bones
            .into_iter()
            .map(|b| DecodedBoneDto {
                rotations: b.rotations,
                translations: b.translations,
                scales: b.scales,
                rotation_animated: b.rotation_animated,
                translation_animated: b.translation_animated,
                scale_animated: b.scale_animated,
            })
            .collect(),
    })
}


fn parse_hex_u64(s: &str) -> Result<u64, String> {
    let trimmed = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(trimmed, 16).map_err(|e| format!("invalid hex u64 {s:?}: {e}"))
}




#[derive(Serialize)]
pub struct AnimsetClipMeta {
    pub name: String,
    pub num_frames: u16,
    pub frame_rate: f32,
    pub looping: bool,
}

#[derive(Serialize)]
pub struct AnimsetSummary {
    pub hash: String,
    pub clips: Vec<AnimsetClipMeta>,
}


#[tauri::command]
pub fn decode_animset_clip(
    folder: String,
    asset_tuid_hex: String,
    animset_hash: String,
    clip_index: u32,
    game_id: Option<String>,
) -> Result<DecodedClipDto, String> {
    let profile = resolve_profile_for_folder(&folder, game_id.as_deref());
    use lunalib::read_moby_assets_with_total;
    use std::io::{Read, Seek, SeekFrom};

    let level_path = std::path::Path::new(&folder);
    let target_tuid = u64::from_str_radix(
        asset_tuid_hex.trim_start_matches("0x").trim_start_matches("0X"),
        16,
    )
    .map_err(|e| format!("parse asset tuid: {e}"))?;
    let target_animset = u64::from_str_radix(
        animset_hash.trim_start_matches("0x").trim_start_matches("0X"),
        16,
    )
    .map_err(|e| format!("parse animset hash: {e}"))?;

    let mut moby_skeleton: Option<lunalib::Skeleton> = None;
    read_moby_assets_with_total(
        level_path,
        Some(&[target_tuid]),
        |_| {},
        |a| {
            if a.tuid == target_tuid {
                moby_skeleton = a.skeleton.clone();
            }
        },
    )
    .map_err(|e| e.to_string())?;
    let skeleton = moby_skeleton
        .ok_or_else(|| format!("asset {asset_tuid_hex} has no skeleton"))?;

    let trans_shift = skeleton.translation_shift;
    let scale_shift_v = skeleton.scale_shift;
    let mconv = profile.matrix_convention();
    let pos_scale = mconv.shift_scale(trans_shift);
    let scale_scale = mconv.shift_scale(scale_shift_v);

    let lookup_path = level_path.join("assetlookup.dat");
    let lookup_file =
        std::fs::File::open(&lookup_path).map_err(|e| format!("open {lookup_path:?}: {e}"))?;
    let mut lookup =
        AssetLookup::open(std::io::BufReader::new(lookup_file)).map_err(|e| e.to_string())?;
    let ptrs = lookup
        .pointers(AssetKind::Animset)
        .map_err(|e| format!("read animset table: {e}"))?;
    let ptr = ptrs
        .iter()
        .find(|p| p.tuid == target_animset)
        .ok_or_else(|| format!("animset {animset_hash} not found in level"))?;

    let animsets_path = level_path.join("animsets.dat");
    let mut animsets_file =
        std::fs::File::open(&animsets_path).map_err(|e| format!("open {animsets_path:?}: {e}"))?;
    animsets_file
        .seek(SeekFrom::Start(u64::from(ptr.offset)))
        .map_err(|e| format!("seek animset: {e}"))?;
    let mut buf = vec![0u8; ptr.length as usize];
    animsets_file
        .read_exact(&mut buf)
        .map_err(|e| format!("read animset: {e}"))?;
    let mut ig =
        IgFile::open(std::io::Cursor::new(buf)).map_err(|e| format!("ighw open: {e}"))?;

    let offsets = animation_section_offsets(&ig);
    let off = offsets
        .get(clip_index as usize)
        .copied()
        .ok_or_else(|| format!("clip index {clip_index} out of range ({} clips)", offsets.len()))?;
    let mut header =
        read_animation_header_at(&mut ig, off).map_err(|e| format!("header: {e}"))?;
    // Mirror IT's additive numBones override (see decode_clips_for_moby for
    // the long-form comment) — without this, V2 additive overlay clips
    // (`mp_carbine_idle_p`, `mp_minigun_melee_a_p`, every weapon overlay)
    // read their control blob at the wrong offsets and silently lose all
    // their track data.
    let skel_bone_count = skeleton.bones.len() as u16;
    if header.is_additive() && skel_bone_count > 0 {
        header.num_bones = skel_bone_count;
    }
    header.apply_frame_stride_padding();
    let ctrl =
        read_animation_control(&mut ig, &header).map_err(|e| format!("control: {e}"))?;

    let mut clip = decode_animation_with_skel(&mut ig, &header, &ctrl, pos_scale, scale_scale, &skeleton, profile)
        .map_err(|e| format!("decode: {e}"))?;

    if clip.additive && profile.game == Some(lunalib::Game::R3) {
        let mut base_names: Vec<String> = Vec::new();
        if let Some(n) = crate::cache::idle_base_for_overlay(&clip.name) {
            base_names.push(n);
        } else if let Some(n) = crate::cache::r3_base_for_overlay(&clip.name) {
            base_names.push(n);
        }
        if !base_names.is_empty() {
            base_names.push("mp_stand_idle".to_string());
            base_names.push("mp_stand_dle".to_string());
        }
        'outer: for want in &base_names {
            for off in &offsets {
                let Ok(mut bh) = read_animation_header_at(&mut ig, *off) else {
                    continue;
                };
                if &bh.name != want || bh.name == clip.name {
                    continue;
                }
                if bh.is_additive() && skel_bone_count > 0 {
                    bh.num_bones = skel_bone_count;
                }
                bh.apply_frame_stride_padding();
                let Ok(bctrl) = read_animation_control(&mut ig, &bh) else {
                    continue;
                };
                if let Ok(base) = decode_animation_with_skel(
                    &mut ig, &bh, &bctrl, pos_scale, scale_scale, &skeleton, profile,
                ) {
                    let (rc, tc, sc) = clip.compose_with_base(&base, true);
                    eprintln!(
                        "[anim-compose] preview '{}' <- '{}' rot={rc} tra={tc} scl={sc}",
                        clip.name, base.name
                    );
                    break 'outer;
                }
            }
        }
    }

    Ok(DecodedClipDto {
        name: clip.name,
        num_frames: clip.num_frames,
        frame_rate: clip.frame_rate,
        looping: clip.looping,
        bones: clip
            .bones
            .into_iter()
            .map(|b| DecodedBoneDto {
                rotations: b.rotations,
                translations: b.translations,
                scales: b.scales,
                rotation_animated: b.rotation_animated,
                translation_animated: b.translation_animated,
                scale_animated: b.scale_animated,
            })
            .collect(),
    })
}

#[tauri::command]
pub fn list_animsets(folder: String) -> Result<Vec<AnimsetSummary>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let level_path = std::path::Path::new(&folder);

    // TOD and RFOM layouts have no `assetlookup.dat` / `animsets.dat`
    // — animation data isn't yet ported for either layout. Surface an
    // empty list rather than a hard error so the UI doesn't show a
    // misleading "animset list failed" warning every time a non-V2
    // level loads.
    match detect_layout(level_path) {
        Ok(LevelLayout::Tod) | Ok(LevelLayout::Rfom) => return Ok(Vec::new()),
        _ => {}
    }

    let lookup_path = level_path.join("assetlookup.dat");
    let lookup_file =
        std::fs::File::open(&lookup_path).map_err(|e| format!("open {lookup_path:?}: {e}"))?;
    let mut lookup = AssetLookup::open(std::io::BufReader::new(lookup_file))
        .map_err(|e| e.to_string())?;
    let ptrs = lookup
        .pointers(AssetKind::Animset)
        .map_err(|e| format!("read animset table: {e}"))?;

    let animsets_path = level_path.join("animsets.dat");
    let mut animsets_file =
        std::fs::File::open(&animsets_path).map_err(|e| format!("open {animsets_path:?}: {e}"))?;

    let mut out = Vec::with_capacity(ptrs.len());
    for ptr in ptrs {
        if animsets_file
            .seek(SeekFrom::Start(u64::from(ptr.offset)))
            .is_err()
        {
            continue;
        }
        let mut buf = vec![0u8; ptr.length as usize];
        if animsets_file.read_exact(&mut buf).is_err() {
            continue;
        }
        let mut ig = match IgFile::open(std::io::Cursor::new(buf)) {
            Ok(ig) => ig,
            Err(_) => continue,
        };
        let offsets = animation_section_offsets(&ig);
        let mut clips = Vec::with_capacity(offsets.len());
        for off in offsets {
            if let Ok(h) = read_animation_header_at(&mut ig, off) {
                let looping = h.is_looping();
                clips.push(AnimsetClipMeta {
                    name: h.name,
                    num_frames: h.num_frames,
                    frame_rate: h.frame_rate,
                    looping,
                });
            }
        }
        out.push(AnimsetSummary {
            hash: format!("0x{:016X}", ptr.tuid),
            clips,
        });
    }
    Ok(out)
}
