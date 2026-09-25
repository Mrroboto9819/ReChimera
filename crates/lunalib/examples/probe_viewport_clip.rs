use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;

use lunalib::{
    animation_section_offsets, decode_animation_with_skel, read_animation_control,
    read_animation_header_at, AssetKind, AssetLookup, Game, IgFile,
};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (folder, tuid_hex, animset_hex) = match (args.next(), args.next(), args.next()) {
        (Some(a), Some(b), Some(c)) => (a, b, c),
        _ => {
            eprintln!("usage: probe_viewport_clip <level_folder> <moby_tuid_hex> <animset_hash_hex> [clip_index]");
            return ExitCode::FAILURE;
        }
    };
    let clip_index: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let target_tuid = u64::from_str_radix(tuid_hex.trim_start_matches("0x"), 16).unwrap();
    let target_animset = u64::from_str_radix(animset_hex.trim_start_matches("0x"), 16).unwrap();
    let level = Path::new(&folder);
    let profile = Game::R3.anim_profile();

    let mut skel = None;
    let r = lunalib::read_moby_assets_with_total(level, Some(&[target_tuid]), |_| {}, |a| {
        if a.tuid == target_tuid {
            skel = a.skeleton.clone();
        }
    });
    if let Err(e) = r {
        eprintln!("read mobys FAILED: {e}");
        return ExitCode::FAILURE;
    }
    let Some(skeleton) = skel else {
        eprintln!("moby has NO skeleton");
        return ExitCode::FAILURE;
    };
    eprintln!(
        "skeleton: bones={} trans_shift={} scale_shift={}",
        skeleton.bones.len(),
        skeleton.translation_shift,
        skeleton.scale_shift
    );

    let mconv = profile.matrix_convention();
    let pos_scale = mconv.shift_scale(skeleton.translation_shift);
    let scale_scale = mconv.shift_scale(skeleton.scale_shift);
    eprintln!("pos_scale={pos_scale} scale_scale={scale_scale}");

    let lookup_file = File::open(level.join("assetlookup.dat")).unwrap();
    let mut lookup = AssetLookup::open(BufReader::new(lookup_file)).unwrap();
    let ptrs = lookup.pointers(AssetKind::Animset).unwrap();
    let Some(ptr) = ptrs.iter().find(|p| p.tuid == target_animset) else {
        eprintln!("animset NOT FOUND in lookup");
        return ExitCode::FAILURE;
    };
    let mut f = File::open(level.join("animsets.dat")).unwrap();
    f.seek(SeekFrom::Start(u64::from(ptr.offset))).unwrap();
    let mut buf = vec![0u8; ptr.length as usize];
    f.read_exact(&mut buf).unwrap();
    let mut ig = IgFile::open(Cursor::new(buf)).unwrap();

    let offsets = animation_section_offsets(&ig);
    eprintln!("animset has {} clips", offsets.len());
    let Some(&off) = offsets.get(clip_index) else {
        eprintln!("clip index {clip_index} out of range");
        return ExitCode::FAILURE;
    };
    let mut header = match read_animation_header_at(&mut ig, off) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("header FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "clip '{}' flags=0x{:04X} additive={} num_bones={} num_frames={} stride={}",
        header.name,
        header.flags,
        header.is_additive(),
        header.num_bones,
        header.num_frames,
        header.frame_stride
    );
    let skel_bones = skeleton.bones.len() as u16;
    if header.is_additive() && skel_bones > 0 {
        header.num_bones = skel_bones;
    }
    header.apply_frame_stride_padding();
    let ctrl = match read_animation_control(&mut ig, &header) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("control FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };
    let clip = match decode_animation_with_skel(
        &mut ig, &header, &ctrl, pos_scale, scale_scale, &skeleton, profile,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("decode FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ar = clip.bones.iter().filter(|b| b.rotation_animated).count();
    let ap = clip.bones.iter().filter(|b| b.translation_animated).count();
    let asc = clip.bones.iter().filter(|b| b.scale_animated).count();
    eprintln!(
        "decoded OK: name='{}' frames={} fps={} bones={} animated R/P/S = {}/{}/{}",
        clip.name,
        clip.num_frames,
        clip.frame_rate,
        clip.bones.len(),
        ar,
        ap,
        asc
    );
    let sample_frames = [0usize, 27, 54, 81, 107];
    for (b, bone) in clip.bones.iter().enumerate() {
        if !bone.rotation_animated && !bone.translation_animated && !bone.scale_animated {
            continue;
        }
        let nf = clip.num_frames as usize;
        let mut line = format!("  bone[{b}]");
        for &f in &sample_frames {
            if f >= nf {
                continue;
            }
            let mut ang = 0.0_f32;
            if bone.rotation_animated && bone.rotations.len() >= (f + 1) * 4 {
                let w = bone.rotations[f * 4 + 3].clamp(-1.0, 1.0);
                ang = 2.0 * w.abs().acos().to_degrees();
            }
            let (tx, ty, tz) = if bone.translation_animated && bone.translations.len() >= (f + 1) * 3
            {
                (
                    bone.translations[f * 3],
                    bone.translations[f * 3 + 1],
                    bone.translations[f * 3 + 2],
                )
            } else {
                (0.0, 0.0, 0.0)
            };
            let (sx, sy, sz) = if bone.scale_animated && bone.scales.len() >= (f + 1) * 3 {
                (
                    bone.scales[f * 3],
                    bone.scales[f * 3 + 1],
                    bone.scales[f * 3 + 2],
                )
            } else {
                (1.0, 1.0, 1.0)
            };
            line.push_str(&format!(
                " | f{f}: rot{ang:.0}° t=({tx:.2},{ty:.2},{tz:.2}) s=({sx:.2},{sy:.2},{sz:.2})"
            ));
        }
        eprintln!("{line}");
    }
    ExitCode::SUCCESS
}
