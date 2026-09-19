use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;

use lunalib::{
    animation_section_offsets, decode_animation_with_skel, read_animation_control,
    read_animation_header_at, AssetKind, AssetLookup, Game, IgFile,
};

struct Suspect {
    moby: u64,
    clip: String,
    bones: usize,
    frames: u16,
    worst_ratio: f32,
    min_scale: f32,
    max_scale: f32,
    worst_qnorm: f32,
    nan: bool,
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let folder = match args.next() {
        Some(a) => a,
        None => {
            eprintln!("usage: audit_anims <level_folder> [game_id]");
            return ExitCode::FAILURE;
        }
    };
    let game = args
        .next()
        .and_then(|id| Game::from_id(&id))
        .unwrap_or(Game::R3);
    let profile = game.anim_profile();
    let level = Path::new(&folder);

    let mut mobys: Vec<(u64, lunalib::Skeleton, u64)> = Vec::new();
    let r = lunalib::read_moby_assets_with_total(level, None, |_| {}, |a| {
        if let (Some(skel), Some(hash)) = (a.skeleton.clone(), a.animset_hash) {
            mobys.push((a.tuid, skel, hash));
        }
    });
    if let Err(e) = r {
        eprintln!("read mobys: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("{} mobys with skeleton+animset", mobys.len());

    let lookup_file = match File::open(level.join("assetlookup.dat")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("assetlookup.dat: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut lookup = match AssetLookup::open(BufReader::new(lookup_file)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("assetlookup: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ptrs = match lookup.pointers(AssetKind::Animset) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("animset table: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut animsets_file = match File::open(level.join("animsets.dat")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("animsets.dat: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut suspects: Vec<Suspect> = Vec::new();
    let mut clips_total = 0usize;
    let mut decode_fail = 0usize;
    let mut audited_sets = std::collections::HashSet::new();

    for (moby_tuid, skel, hash) in &mobys {
        if !audited_sets.insert(*hash) {
            continue;
        }
        let Some(ptr) = ptrs.iter().find(|p| p.tuid == *hash) else {
            continue;
        };
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
        let Ok(mut ig) = IgFile::open(Cursor::new(buf)) else {
            continue;
        };

        let mconv = profile.matrix_convention();
        let pos_scale = mconv.shift_scale(skel.translation_shift);
        let scale_scale = mconv.shift_scale(skel.scale_shift);
        let skel_bones = skel.bones.len() as u16;

        let mut bind_mags: Vec<f32> = Vec::with_capacity(skel.bones.len());
        for m in &skel.bind_local {
            bind_mags.push((m[12] * m[12] + m[13] * m[13] + m[14] * m[14]).sqrt());
        }
        let mut depth: Vec<u32> = vec![0; skel.bones.len()];
        for i in 0..skel.bones.len() {
            let mut d = 0u32;
            let mut cur = i;
            let mut hops = 0;
            while let Some(p) = skel.bones[cur].parent() {
                if p == cur || hops > skel.bones.len() {
                    break;
                }
                d += 1;
                cur = p;
                hops += 1;
            }
            depth[i] = d;
        }
        let typical_bind = {
            let mut v: Vec<f32> = bind_mags.iter().copied().filter(|m| *m > 1e-4).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v.get(v.len() / 2).copied().unwrap_or(0.1)
        };

        for off in animation_section_offsets(&ig) {
            clips_total += 1;
            let Ok(mut header) = read_animation_header_at(&mut ig, off) else {
                decode_fail += 1;
                continue;
            };
            if header.is_additive() && skel_bones > 0 {
                header.num_bones = skel_bones;
            }
            header.apply_frame_stride_padding();
            let Ok(ctrl) = read_animation_control(&mut ig, &header) else {
                decode_fail += 1;
                continue;
            };
            let clip = match decode_animation_with_skel(
                &mut ig,
                &header,
                &ctrl,
                pos_scale,
                scale_scale,
                skel,
                profile,
            ) {
                Ok(c) => c,
                Err(_) => {
                    decode_fail += 1;
                    continue;
                }
            };

            let mut worst_ratio = 0f32;
            let mut min_scale = f32::INFINITY;
            let mut max_scale = f32::NEG_INFINITY;
            let mut worst_qnorm = 0f32;
            let mut nan = false;
            for (bi, bone) in clip.bones.iter().enumerate() {
                let bind = bind_mags.get(bi).copied().unwrap_or(0.0).max(typical_bind);
                let root_motion_carrier = depth.get(bi).copied().unwrap_or(0) <= 1;
                if bone.translation_animated && !root_motion_carrier {
                    for f in bone.translations.chunks_exact(3) {
                        let mag = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
                        if !mag.is_finite() {
                            nan = true;
                            continue;
                        }
                        let ratio = mag / bind.max(1e-3);
                        if ratio > worst_ratio {
                            worst_ratio = ratio;
                        }
                    }
                }
                if bone.scale_animated {
                    for s in &bone.scales {
                        if !s.is_finite() {
                            nan = true;
                            continue;
                        }
                        if *s < min_scale {
                            min_scale = *s;
                        }
                        if *s > max_scale {
                            max_scale = *s;
                        }
                    }
                }
                if bone.rotation_animated {
                    for q in bone.rotations.chunks_exact(4) {
                        let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
                        if !n.is_finite() {
                            nan = true;
                            continue;
                        }
                        let dev = (n - 1.0).abs();
                        if dev > worst_qnorm {
                            worst_qnorm = dev;
                        }
                    }
                }
            }
            if min_scale == f32::INFINITY {
                min_scale = 1.0;
                max_scale = 1.0;
            }

            let bad = nan
                || worst_ratio > 20.0
                || min_scale < 0.01
                || max_scale > 20.0
                || worst_qnorm > 0.05;
            if bad {
                suspects.push(Suspect {
                    moby: *moby_tuid,
                    clip: clip.name.clone(),
                    bones: clip.bones.len(),
                    frames: clip.num_frames,
                    worst_ratio,
                    min_scale,
                    max_scale,
                    worst_qnorm,
                    nan,
                });
            }
        }
    }

    suspects.sort_by(|a, b| b.worst_ratio.partial_cmp(&a.worst_ratio).unwrap());
    println!(
        "audited {} animsets, {} clips, {} decode failures, {} suspects (game={:?})",
        audited_sets.len(),
        clips_total,
        decode_fail,
        suspects.len(),
        game
    );
    for s in suspects.iter().take(60) {
        println!(
            "SUSPECT moby=0x{:016X} clip='{}' bones={} frames={} trans_ratio={:.1} scale=[{:.4}..{:.2}] qnorm_dev={:.4}{}",
            s.moby,
            s.clip,
            s.bones,
            s.frames,
            s.worst_ratio,
            s.min_scale,
            s.max_scale,
            s.worst_qnorm,
            if s.nan { " NAN" } else { "" }
        );
    }
    ExitCode::SUCCESS
}
