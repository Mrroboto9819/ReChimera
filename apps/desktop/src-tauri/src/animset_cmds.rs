use std::fs::File;

use std::path::Path;

use lunalib::{decode_animation, read_animation_control, read_animation_header, AssetKind, IgFile};
use crate::open_lookup;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct DecodedBoneDto {

    rotations: Vec<f32>,

    translations: Vec<f32>,
    scales: Vec<f32>,
    rotation_animated: bool,
    translation_animated: bool,
    scale_animated: bool,
}


#[derive(Serialize)]
pub(crate) struct DecodedClipDto {
    name: String,
    num_frames: u16,
    frame_rate: f32,
    looping: bool,

    bones: Vec<DecodedBoneDto>,
}



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



