use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;

use lunalib::{
    animation_section_offsets, decode_animation_with_skel, read_animation_control,
    read_animation_header_at, AssetKind, AssetLookup, Game, IgFile, MobyAsset,
};

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x100000001B3);
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let folder = args.next().expect("level folder");
    let tuids: Vec<u64> = args
        .filter_map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .collect();
    let level = Path::new(&folder);
    let game = Game::R3;
    let profile = game.anim_profile();
    let mconv = profile.matrix_convention();

    let mut found: Vec<MobyAsset> = Vec::new();
    lunalib::read_moby_assets_with_total(level, None, |_| {}, |a| {
        if tuids.contains(&a.tuid) {
            found.push(a.clone());
        }
    })
    .expect("read mobys");

    let lookup_file = File::open(level.join("assetlookup.dat")).expect("assetlookup.dat");
    let mut lookup = AssetLookup::open(BufReader::new(lookup_file)).expect("assetlookup");
    let ptrs = lookup.pointers(AssetKind::Animset).expect("animset table");
    let mut animsets_file = File::open(level.join("animsets.dat")).expect("animsets.dat");

    for a in &found {
        let Some(skel) = a.skeleton.as_ref() else {
            println!("0x{:016X} {} NO SKELETON", a.tuid, a.name);
            continue;
        };
        let mut bind_hash = 0xcbf29ce484222325u64;
        for m in &skel.bind_local {
            for f in m {
                fnv(&mut bind_hash, &f.to_le_bytes());
            }
        }
        let mut parent_hash = 0xcbf29ce484222325u64;
        for b in &skel.bones {
            fnv(&mut parent_hash, &b.parent_index.to_le_bytes());
            fnv(&mut parent_hash, &b.flags.to_le_bytes());
        }
        let mut inv_hash = 0xcbf29ce484222325u64;
        for m in &skel.bind_world_inverse {
            for f in m {
                fnv(&mut inv_hash, &f.to_le_bytes());
            }
        }
        println!(
            "0x{:016X} bones={} shifts={}/{} bind_local=0x{:016X} parents+flags=0x{:016X} bind_inv=0x{:016X} animset={:?} name={}",
            a.tuid,
            skel.bones.len(),
            skel.translation_shift,
            skel.scale_shift,
            bind_hash,
            parent_hash,
            inv_hash,
            a.animset_hash.map(|h| format!("0x{h:016X}")),
            a.name
        );

        let Some(hash) = a.animset_hash else { continue };
        let Some(ptr) = ptrs.iter().find(|p| p.tuid == hash) else {
            continue;
        };
        animsets_file
            .seek(SeekFrom::Start(u64::from(ptr.offset)))
            .expect("seek");
        let mut buf = vec![0u8; ptr.length as usize];
        animsets_file.read_exact(&mut buf).expect("read");
        let mut ig = IgFile::open(Cursor::new(buf)).expect("igfile");
        let pos_scale = mconv.shift_scale(skel.translation_shift);
        let scale_scale = mconv.shift_scale(skel.scale_shift);
        let skel_bones = skel.bones.len() as u16;

        for off in animation_section_offsets(&ig) {
            let Ok(mut h) = read_animation_header_at(&mut ig, off) else {
                continue;
            };
            if h.is_additive() && skel_bones > 0 {
                h.num_bones = skel_bones;
            }
            h.apply_frame_stride_padding();
            let Ok(ctrl) = read_animation_control(&mut ig, &h) else {
                println!("    '{}' control FAILED", h.name);
                continue;
            };
            match decode_animation_with_skel(
                &mut ig,
                &h,
                &ctrl,
                pos_scale,
                scale_scale,
                skel,
                profile,
            ) {
                Ok(clip) => {
                    let mut n_rot = 0usize;
                    let mut n_pos = 0usize;
                    let mut n_scl = 0usize;
                    let mut worst_ratio = 0f32;
                    let mut worst_ratio_bone = 0usize;
                    let mut min_scale = f32::INFINITY;
                    let mut max_scale = f32::NEG_INFINITY;
                    let mut worst_qnorm = 0f32;
                    for (bi, b) in clip.bones.iter().enumerate() {
                        let bind = skel
                            .bind_local
                            .get(bi)
                            .map(|m| (m[12] * m[12] + m[13] * m[13] + m[14] * m[14]).sqrt())
                            .unwrap_or(0.0)
                            .max(1e-3);
                        if b.rotation_animated {
                            n_rot += 1;
                            for q in b.rotations.chunks_exact(4) {
                                let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3])
                                    .sqrt();
                                let dev = (n - 1.0).abs();
                                if dev > worst_qnorm {
                                    worst_qnorm = dev;
                                }
                            }
                        }
                        if b.translation_animated {
                            n_pos += 1;
                            for f in b.translations.chunks_exact(3) {
                                let mag = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
                                let ratio = mag / bind;
                                if ratio > worst_ratio {
                                    worst_ratio = ratio;
                                    worst_ratio_bone = bi;
                                }
                            }
                        }
                        if b.scale_animated {
                            n_scl += 1;
                            for s in &b.scales {
                                if *s < min_scale {
                                    min_scale = *s;
                                }
                                if *s > max_scale {
                                    max_scale = *s;
                                }
                            }
                        }
                    }
                    if min_scale == f32::INFINITY {
                        min_scale = 1.0;
                        max_scale = 1.0;
                    }
                    println!(
                        "    '{}' flags=0x{:04X} frames={} R/P/S={}/{}/{} trans_ratio={:.1}@b{} scale=[{:.3}..{:.3}] qnorm_dev={:.4}",
                        clip.name,
                        h.flags,
                        clip.num_frames,
                        n_rot,
                        n_pos,
                        n_scl,
                        worst_ratio,
                        worst_ratio_bone,
                        min_scale,
                        max_scale,
                        worst_qnorm
                    );
                }
                Err(e) => println!("    '{}' decode FAILED: {e}", h.name),
            }

            if h.name == "face_angry" || h.name == "head_visemes" || h.name == "head_idle" {
                let mut tracked = vec![false; skel.bones.len()];
                for m in ctrl.track16_masks.iter().chain(ctrl.track8_masks.iter()) {
                    if matches!(m.kind, lunalib::TrackKind::Rotation) {
                        if let Some(t) = tracked.get_mut(m.bone_index as usize) {
                            *t = true;
                        }
                    }
                }
                let mut worst: Vec<(usize, f32)> = Vec::new();
                for (b, bl) in skel.bind_local.iter().enumerate() {
                    if tracked.get(b).copied().unwrap_or(false) {
                        continue;
                    }
                    let bind_q = {
                        let m = bl;
                        let (sx, sy, sz) = (
                            (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-6),
                            (m[4] * m[4] + m[5] * m[5] + m[6] * m[6]).sqrt().max(1e-6),
                            (m[8] * m[8] + m[9] * m[9] + m[10] * m[10]).sqrt().max(1e-6),
                        );
                        let r = [
                            m[0] / sx,
                            m[1] / sx,
                            m[2] / sx,
                            m[4] / sy,
                            m[5] / sy,
                            m[6] / sy,
                            m[8] / sz,
                            m[9] / sz,
                            m[10] / sz,
                        ];
                        let tr = r[0] + r[4] + r[8];
                        if tr > 0.0 {
                            let s = (tr + 1.0).sqrt() * 2.0;
                            [
                                (r[5] - r[7]) / s,
                                (r[6] - r[2]) / s,
                                (r[1] - r[3]) / s,
                                0.25 * s,
                            ]
                        } else {
                            [0.0, 0.0, 0.0, 1.0]
                        }
                    };
                    let rr = ctrl
                        .ref_pose_rotations
                        .get(b)
                        .copied()
                        .unwrap_or([0, 0, 0, 32767]);
                    let n = ((rr[0] as f32).powi(2)
                        + (rr[1] as f32).powi(2)
                        + (rr[2] as f32).powi(2)
                        + (rr[3] as f32).powi(2))
                    .sqrt()
                    .max(1e-6);
                    let rq = [
                        rr[0] as f32 / n,
                        rr[1] as f32 / n,
                        rr[2] as f32 / n,
                        rr[3] as f32 / n,
                    ];
                    let dot = (bind_q[0] * rq[0]
                        + bind_q[1] * rq[1]
                        + bind_q[2] * rq[2]
                        + bind_q[3] * rq[3])
                        .abs()
                        .min(1.0);
                    let angle_deg = 2.0 * dot.acos().to_degrees();
                    worst.push((b, angle_deg));
                }
                worst.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                let mean: f32 = if worst.is_empty() {
                    0.0
                } else {
                    worst.iter().map(|w| w.1).sum::<f32>() / worst.len() as f32
                };
                let top: Vec<String> = worst
                    .iter()
                    .take(10)
                    .map(|(b, a)| {
                        format!(
                            "b{b}={a:.1}(bm{})",
                            ctrl.blend_masks.get(*b).copied().unwrap_or(255)
                        )
                    })
                    .collect();
                println!(
                    "    [{}] untracked_rot_bones={} ref-vs-bind mean={:.2} deg, worst: {}",
                    h.name,
                    worst.len(),
                    mean,
                    top.join(" ")
                );
            }
        }
    }
    ExitCode::SUCCESS
}
