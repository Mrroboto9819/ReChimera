use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;

use lunalib::{
    animation_section_offsets, decode_animation_with_skel, read_animation_control,
    read_animation_frame, read_animation_header_at, AssetKind, AssetLookup, Game, IgFile,
};

const fn pad_to(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

fn kind_char(bits: u16) -> char {
    match bits & 0b11 {
        0 => 'R',
        1 => 'S',
        2 => 'P',
        _ => '?',
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let folder = match args.next() {
        Some(a) => a,
        None => {
            eprintln!("usage: probe_r3_face <level_folder> [moby_tuid_hex] [clip_substr,clip_substr,...]");
            return ExitCode::FAILURE;
        }
    };
    let target_tuid = args
        .next()
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x5ED37B1C9C403839);
    let filters: Vec<String> = args
        .next()
        .unwrap_or_else(|| "hyb_plagued_bomb_fall_c,hyb_hose_react_lower_left".to_string())
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let game = Game::R3;
    let profile = game.anim_profile();
    let level = Path::new(&folder);

    let mut found: Option<(lunalib::Skeleton, u64)> = None;
    let r = lunalib::read_moby_assets_with_total(level, None, |_| {}, |a| {
        if a.tuid == target_tuid {
            if let (Some(skel), Some(hash)) = (a.skeleton.clone(), a.animset_hash) {
                found = Some((skel, hash));
            }
        }
    });
    if let Err(e) = r {
        eprintln!("read mobys: {e}");
        return ExitCode::FAILURE;
    }
    let Some((skel, hash)) = found else {
        eprintln!("moby 0x{target_tuid:016X} not found or lacks skeleton/animset");
        return ExitCode::FAILURE;
    };

    let mconv = profile.matrix_convention();
    println!("moby 0x{target_tuid:016X} animset 0x{hash:016X}");
    println!(
        "skeleton: {} bones, translation_shift=0x{:04X} ({}) -> pos_scale={:e}, scale_shift=0x{:04X} ({}) -> scale_scale={:e}",
        skel.bones.len(),
        skel.translation_shift,
        skel.translation_shift,
        mconv.shift_scale(skel.translation_shift),
        skel.scale_shift,
        skel.scale_shift,
        mconv.shift_scale(skel.scale_shift),
    );
    let n_roots = skel.bones.iter().filter(|b| b.is_root()).count();
    println!("roots={} first parents: {:?}", n_roots, skel.bones.iter().take(8).map(|b| b.parent_index).collect::<Vec<_>>());

    let lookup_file = File::open(level.join("assetlookup.dat")).expect("assetlookup.dat");
    let mut lookup = AssetLookup::open(BufReader::new(lookup_file)).expect("assetlookup");
    let ptrs = lookup.pointers(AssetKind::Animset).expect("animset table");
    let ptr = ptrs.iter().find(|p| p.tuid == hash).expect("animset ptr");
    let mut animsets_file = File::open(level.join("animsets.dat")).expect("animsets.dat");
    animsets_file
        .seek(SeekFrom::Start(u64::from(ptr.offset)))
        .expect("seek");
    let mut buf = vec![0u8; ptr.length as usize];
    animsets_file.read_exact(&mut buf).expect("read animset");
    let mut ig = IgFile::open(Cursor::new(buf)).expect("igfile");

    let pos_scale = mconv.shift_scale(skel.translation_shift);
    let scale_scale = mconv.shift_scale(skel.scale_shift);
    let skel_bones = skel.bones.len() as u16;

    let offsets = animation_section_offsets(&ig);
    let mut headers: Vec<(u64, lunalib::AnimationHeader)> = Vec::new();
    for off in &offsets {
        if let Ok(h) = read_animation_header_at(&mut ig, *off) {
            headers.push((*off, h));
        }
    }
    println!("\n=== clip list ({} sections) ===", headers.len());
    for (_, h) in &headers {
        println!(
            "  '{}' flags=0x{:04X} bones={} frames={} stride={} nrv={} n16={} n8={} fps={}",
            h.name,
            h.flags,
            h.num_bones,
            h.num_frames,
            h.frame_stride,
            h.num_reference_values,
            h.num_16bit_tracks,
            h.num_8bit_tracks,
            h.frame_rate
        );
    }

    let mut boundaries: Vec<u64> = Vec::new();
    for (off, h) in &headers {
        boundaries.push(*off);
        if h.control_ptr != 0 {
            boundaries.push(u64::from(h.control_ptr));
        }
        if h.frames_ptr != 0 {
            boundaries.push(u64::from(h.frames_ptr));
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    println!("\n=== frame-data extent census (packed vs stride*num_frames) ===");
    let mut overrun_count = 0usize;
    for (_, h) in &headers {
        if h.frames_ptr == 0 || h.frame_stride == 0 || h.num_frames == 0 {
            continue;
        }
        let mut eff = h.clone();
        if !eff.is_packed_frames() {
            eff.apply_frame_stride_padding();
        }
        let fp = u64::from(eff.frames_ptr);
        let next = boundaries
            .iter()
            .copied()
            .find(|&b| b > fp)
            .unwrap_or(u64::MAX);
        if next == u64::MAX {
            continue;
        }
        let extent = next - fp;
        let need = u64::from(eff.num_frames) * u64::from(eff.frame_stride);
        let fit = extent / u64::from(eff.frame_stride);
        if need > extent {
            overrun_count += 1;
            println!(
                "  OVERRUN '{}' flags=0x{:04X} frames={} stride={} fit={} missing={} extent=0x{:X} fps={}",
                eff.name,
                eff.flags,
                eff.num_frames,
                eff.frame_stride,
                fit,
                u64::from(eff.num_frames) - fit,
                extent,
                eff.frame_rate
            );
        }
    }
    println!("total overruns: {overrun_count} / {}", headers.len());

    println!("\n=== raw header fields (overruns + first 6 normal packed clips) ===");
    let mut shown_normal = 0usize;
    for (off, h) in &headers {
        if h.frames_ptr == 0 || h.frame_stride == 0 || h.num_frames == 0 {
            continue;
        }
        let fp = u64::from(h.frames_ptr);
        let next = boundaries
            .iter()
            .copied()
            .find(|&b| b > fp)
            .unwrap_or(u64::MAX);
        let extent = if next == u64::MAX { u64::MAX } else { next - fp };
        let need = u64::from(h.num_frames) * u64::from(h.frame_stride);
        let overrun = need > extent;
        if !overrun {
            if shown_normal >= 6 || !h.is_packed_frames() {
                continue;
            }
            shown_normal += 1;
        }
        let _ = ig.stream.seek_to(off + 0x0C);
        let loaded_tag = ig.stream.read_u32().unwrap_or(0);
        let _ = ig.stream.seek_to(off + 0x10);
        let unk4 = ig.stream.read_f32().unwrap_or(0.0);
        let _ = ig.stream.seek_to(off + 0x1C);
        let root_motion_ptr = ig.stream.read_u32().unwrap_or(0);
        let _ = ig.stream.seek_to(off + 0x28);
        let null0a = ig.stream.read_u32().unwrap_or(0);
        let null0b = ig.stream.read_u32().unwrap_or(0);
        let ref_pose_buffer_size = ig.stream.read_u16().unwrap_or(0);
        let _ = ig.stream.seek_to(off + 0x3A);
        let unk10 = ig.stream.read_u16().unwrap_or(0);
        let null1 = ig.stream.read_u32().unwrap_or(0);
        let fit = if extent == u64::MAX {
            0
        } else {
            extent / u64::from(h.frame_stride)
        };
        println!(
            "  {} '{}' frames={} fit={} fps={} unk4={} linear_speed={} loaded_tag=0x{:08X} rm_ptr=0x{:X} null0=0x{:X},0x{:X} refposebuf={} unk10={} null1=0x{:X}",
            if overrun { "OVR " } else { "ok  " },
            h.name,
            h.num_frames,
            fit,
            h.frame_rate,
            unk4,
            h.linear_speed,
            loaded_tag,
            root_motion_ptr,
            null0a,
            null0b,
            ref_pose_buffer_size,
            unk10,
            null1
        );
    }

    for off in &offsets {
        let Ok(orig) = read_animation_header_at(&mut ig, *off) else {
            continue;
        };
        let lname = orig.name.to_lowercase();
        if !filters.iter().any(|f| lname.contains(f)) {
            continue;
        }

        println!("\n=== DEEP DUMP '{}' @0x{:X} ===", orig.name, off);
        println!(
            "header: anim_index={} flags=0x{:04X} (loop={} additive={} packed={} delta0x200={}) num_bones={} num_frames={} stride_raw={} nrv={} n16={} n8={} control_ptr=0x{:X} frames_ptr=0x{:X}",
            orig.anim_index,
            orig.flags,
            orig.flags & 1 != 0,
            orig.flags & 2 != 0,
            orig.flags & 4 != 0,
            orig.flags & 0x200 != 0,
            orig.num_bones,
            orig.num_frames,
            orig.frame_stride,
            orig.num_reference_values,
            orig.num_16bit_tracks,
            orig.num_8bit_tracks,
            orig.control_ptr,
            orig.frames_ptr
        );

        let mut h = orig.clone();
        if h.is_additive() && skel_bones > 0 {
            h.num_bones = skel_bones;
        }
        h.apply_frame_stride_padding();
        println!(
            "effective: num_bones={} stride={} (padding {})",
            h.num_bones,
            h.frame_stride,
            if h.is_packed_frames() { "skipped: packed" } else { "applied" }
        );

        let nb = h.num_bones as u32;
        let nrv = h.num_reference_values as u32;
        let n16 = h.num_16bit_tracks as u32;
        let n8 = h.num_8bit_tracks as u32;
        let base = u64::from(h.control_ptr);
        let off_values = pad_to(nb * 8, 16);
        let off_value_masks = pad_to(off_values + nrv * 2, 16);
        let off_t16_masks = pad_to(off_value_masks + nrv * 2, 16);
        let off_t8_masks = pad_to(off_t16_masks + n16 * 2, 16);

        let read_raw_masks = |ig: &mut IgFile<Cursor<Vec<u8>>>, at: u64, n: u32| -> Vec<u16> {
            let mut v = Vec::with_capacity(n as usize);
            if ig.stream.seek_to(at).is_ok() {
                for _ in 0..n {
                    match ig.stream.read_u16() {
                        Ok(x) => v.push(x),
                        Err(_) => break,
                    }
                }
            }
            v
        };
        let raw_ref_masks = read_raw_masks(&mut ig, base + off_value_masks as u64, nrv);
        let raw_t16_masks = read_raw_masks(&mut ig, base + off_t16_masks as u64, n16);
        let raw_t8_masks = read_raw_masks(&mut ig, base + off_t8_masks as u64, n8);

        let census = |label: &str, raws: &[u16]| {
            let mut unk_hist = [0usize; 4];
            let mut kinds = [0usize; 4];
            let mut oob = 0usize;
            for &raw in raws {
                unk_hist[(raw & 0b11) as usize] += 1;
                let k = ((raw >> 4) & 0b11) as usize;
                kinds[k] += 1;
                if ((raw >> 6) & 0x3FF) as u16 >= h.num_bones {
                    oob += 1;
                }
            }
            println!(
                "{label}: n={} unk_bits[0,1,2,3]={:?} kinds[R,S,P,?]={:?} bone_oob={}",
                raws.len(),
                unk_hist,
                kinds,
                oob
            );
        };
        census("ref_masks", &raw_ref_masks);
        census("t16_masks", &raw_t16_masks);
        census("t8_masks", &raw_t8_masks);

        let Ok(ctrl) = read_animation_control(&mut ig, &h) else {
            println!("control read FAILED");
            continue;
        };

        let tracked_rot: Vec<u16> = raw_t16_masks
            .iter()
            .chain(raw_t8_masks.iter())
            .filter(|&&r| (r >> 4) & 0b11 == 0)
            .map(|&r| (r >> 6) & 0x3FF)
            .collect();
        println!(
            "blend_masks ({}): {:?}",
            ctrl.blend_masks.len(),
            ctrl.blend_masks
        );
        println!("rot-tracked bones: {:?}", {
            let mut v = tracked_rot.clone();
            v.sort_unstable();
            v.dedup();
            v
        });

        let tracked_set: std::collections::HashSet<u16> = tracked_rot.iter().copied().collect();
        println!("--- per-bone ref rotation vs bind (untracked only) ---");
        for b in 0..(h.num_bones as usize).min(skel.bones.len()) {
            if tracked_set.contains(&(b as u16)) {
                continue;
            }
            let raw = ctrl.ref_pose_rotations.get(b).copied().unwrap_or([0, 0, 0, 32767]);
            let norm = ((raw[0] as f64).powi(2)
                + (raw[1] as f64).powi(2)
                + (raw[2] as f64).powi(2)
                + (raw[3] as f64).powi(2))
            .sqrt();
            let rq = lunalib::dequantize_quaternion(raw);
            let bl = skel.bind_local.get(b).copied().unwrap_or([0.0; 16]);
            let bq = lunalib::extract_bind_rotation(&bl);
            let dot = (rq[0] * bq[0] + rq[1] * bq[1] + rq[2] * bq[2] + rq[3] * bq[3])
                .abs()
                .min(1.0);
            let angle_deg = 2.0 * dot.acos().to_degrees();
            println!(
                "  bone[{:3}] mask={:3} raw={:?} |raw|={:.0} angle_to_bind={:6.1}",
                b,
                ctrl.blend_masks.get(b).copied().unwrap_or(255),
                raw,
                norm,
                angle_deg
            );
        }

        let mut ref_scale_entries: Vec<(usize, u16, i16)> = Vec::new();
        for (i, &raw) in raw_ref_masks.iter().enumerate() {
            if (raw >> 4) & 0b11 == 1 {
                ref_scale_entries.push((i, raw, ctrl.ref_pose_values.get(i).copied().unwrap_or(0)));
            }
        }
        println!("ref scale entries: {}", ref_scale_entries.len());
        for (i, raw, v) in ref_scale_entries.iter().take(20) {
            println!(
                "  ref[{i}] raw=0x{raw:04X} bone={} comp={} unk={} value={v} -> {:.4} (+bias)",
                (raw >> 6) & 0x3FF,
                (raw >> 2) & 0b11,
                raw & 0b11,
                *v as f32 * scale_scale
            );
        }

        let bind_t = |bone: usize| -> [f32; 3] {
            skel.bind_local
                .get(bone)
                .map(|m| [m[12], m[13], m[14]])
                .unwrap_or([0.0; 3])
        };
        let mut ref_pos_entries: Vec<(usize, u16, i16)> = Vec::new();
        for (i, &raw) in raw_ref_masks.iter().enumerate() {
            if (raw >> 4) & 0b11 == 2 {
                ref_pos_entries.push((i, raw, ctrl.ref_pose_values.get(i).copied().unwrap_or(0)));
            }
        }
        println!("ref POSITION entries vs bind: {}", ref_pos_entries.len());
        for (i, raw, v) in ref_pos_entries.iter() {
            let bone = ((raw >> 6) & 0x3FF) as usize;
            let comp = ((raw >> 2) & 0b11) as usize;
            let decoded = *v as f32 * pos_scale;
            let b = bind_t(bone).get(comp).copied().unwrap_or(f32::NAN);
            println!(
                "  ref[{i}] raw=0x{:04X} bone={bone} comp={comp} value={v} -> {decoded:.4} bind={b:.4} delta={:+.4}",
                raw,
                decoded - b
            );
        }


        let nf = h.num_frames as usize;
        let mut frames16: Vec<Vec<i16>> = Vec::with_capacity(nf);
        let mut frames8: Vec<Vec<i8>> = Vec::with_capacity(nf);
        for f in 0..nf {
            match read_animation_frame(&mut ig, &h, f as u16) {
                Ok((v16, v8)) => {
                    frames16.push(v16);
                    frames8.push(v8);
                }
                Err(e) => {
                    println!("frame {f} read FAILED: {e}");
                    frames16.push(Vec::new());
                    frames8.push(Vec::new());
                }
            }
        }

        println!("--- 16-bit SCALE tracks ---");
        for (ti, &raw) in raw_t16_masks.iter().enumerate() {
            if (raw >> 4) & 0b11 != 1 {
                continue;
            }
            let bone = (raw >> 6) & 0x3FF;
            let comp = (raw >> 2) & 0b11;
            let vals: Vec<i16> = frames16.iter().filter_map(|f| f.get(ti).copied()).collect();
            let mn = vals.iter().copied().min().unwrap_or(0);
            let mx = vals.iter().copied().max().unwrap_or(0);
            let head: Vec<i16> = vals.iter().copied().take(8).collect();
            println!(
                "  t16[{ti}] raw=0x{raw:04X} bone={bone} comp={comp} unk={} min={mn} max={mx} -> [{:.3}..{:.3}] head={:?}",
                raw & 0b11,
                mn as f32 * scale_scale,
                mx as f32 * scale_scale,
                head
            );
        }
        println!("--- 8-bit SCALE tracks (base + delta) ---");
        for (ti, &raw) in raw_t8_masks.iter().enumerate() {
            if (raw >> 4) & 0b11 != 1 {
                continue;
            }
            let bone = (raw >> 6) & 0x3FF;
            let comp = (raw >> 2) & 0b11;
            let bases = ctrl.track8_base_values.get(ti).copied().unwrap_or(0);
            let vals: Vec<i16> = frames8
                .iter()
                .filter_map(|f| f.get(ti).copied())
                .map(|d| bases.wrapping_add(d as i16))
                .collect();
            let mn = vals.iter().copied().min().unwrap_or(0);
            let mx = vals.iter().copied().max().unwrap_or(0);
            let head: Vec<i16> = vals.iter().copied().take(8).collect();
            println!(
                "  t8[{ti}] raw=0x{raw:04X} bone={bone} comp={comp} unk={} base={bases} min={mn} max={mx} -> [{:.3}..{:.3}] head={:?}",
                raw & 0b11,
                mn as f32 * scale_scale,
                mx as f32 * scale_scale,
                head
            );
        }

        println!("--- 16-bit POSITION tracks vs bind ---");
        for (ti, &raw) in raw_t16_masks.iter().enumerate() {
            if (raw >> 4) & 0b11 != 2 {
                continue;
            }
            let bone = ((raw >> 6) & 0x3FF) as usize;
            let comp = ((raw >> 2) & 0b11) as usize;
            let vals: Vec<i16> = frames16.iter().filter_map(|f| f.get(ti).copied()).collect();
            let mn = vals.iter().copied().min().unwrap_or(0);
            let mx = vals.iter().copied().max().unwrap_or(0);
            let b = bind_t(bone).get(comp).copied().unwrap_or(f32::NAN);
            println!(
                "  t16[{ti}] raw=0x{raw:04X} bone={bone} comp={comp} range=[{:.4}..{:.4}] bind={b:.4} delta_mid={:+.4} head={:?}",
                mn as f32 * pos_scale,
                mx as f32 * pos_scale,
                ((mn as f32 + mx as f32) * 0.5) * pos_scale - b,
                vals.iter().copied().take(6).collect::<Vec<i16>>()
            );
        }
        println!("--- 8-bit POSITION tracks (base+delta) vs bind ---");
        for (ti, &raw) in raw_t8_masks.iter().enumerate() {
            if (raw >> 4) & 0b11 != 2 {
                continue;
            }
            let bone = ((raw >> 6) & 0x3FF) as usize;
            let comp = ((raw >> 2) & 0b11) as usize;
            let base = ctrl.track8_base_values.get(ti).copied().unwrap_or(0);
            let vals: Vec<i16> = frames8
                .iter()
                .filter_map(|f| f.get(ti).copied())
                .map(|d| base.wrapping_add(d as i16))
                .collect();
            let mn = vals.iter().copied().min().unwrap_or(0);
            let mx = vals.iter().copied().max().unwrap_or(0);
            let b = bind_t(bone).get(comp).copied().unwrap_or(f32::NAN);
            println!(
                "  t8[{ti}] raw=0x{raw:04X} bone={bone} comp={comp} base={base} range=[{:.4}..{:.4}] bind={b:.4} delta_mid={:+.4}",
                mn as f32 * pos_scale,
                mx as f32 * pos_scale,
                ((mn as f32 + mx as f32) * 0.5) * pos_scale - b
            );
        }

        println!("--- full per-frame series (first scale track, widest scale track, t16[0]) ---");
        let scale_tracks: Vec<usize> = raw_t16_masks
            .iter()
            .enumerate()
            .filter(|(_, &r)| (r >> 4) & 0b11 == 1)
            .map(|(i, _)| i)
            .collect();
        let widest = scale_tracks
            .iter()
            .copied()
            .max_by_key(|&ti| {
                let vals: Vec<i16> = frames16.iter().filter_map(|f| f.get(ti).copied()).collect();
                let mn = vals.iter().copied().min().unwrap_or(0) as i32;
                let mx = vals.iter().copied().max().unwrap_or(0) as i32;
                mx - mn
            });
        let mut dump_set: Vec<usize> = Vec::new();
        if let Some(&first) = scale_tracks.first() {
            dump_set.push(first);
        }
        if let Some(w) = widest {
            if !dump_set.contains(&w) {
                dump_set.push(w);
            }
        }
        if !raw_t16_masks.is_empty() && !dump_set.contains(&0) {
            dump_set.push(0);
        }
        for ti in dump_set {
            let raw = raw_t16_masks[ti];
            let vals: Vec<i16> = frames16.iter().filter_map(|f| f.get(ti).copied()).collect();
            println!(
                "  t16[{ti}] raw=0x{raw:04X} {} bone={} comp={} all {} frames:",
                kind_char(raw >> 4),
                (raw >> 6) & 0x3FF,
                (raw >> 2) & 0b11,
                vals.len()
            );
            for chunk in vals.chunks(10) {
                println!("    {:?}", chunk);
            }
        }

        println!("--- neighbor 16-bit tracks around scale tracks (context) ---");
        for (ti, &raw) in raw_t16_masks.iter().enumerate() {
            let bone = (raw >> 6) & 0x3FF;
            let comp = (raw >> 2) & 0b11;
            let vals: Vec<i16> = frames16.iter().filter_map(|f| f.get(ti).copied()).collect();
            let mn = vals.iter().copied().min().unwrap_or(0);
            let mx = vals.iter().copied().max().unwrap_or(0);
            if ti < 24 || (raw >> 4) & 0b11 == 1 {
                println!(
                    "  t16[{ti}] raw=0x{raw:04X} {} bone={bone} comp={comp} unk={} min={mn} max={mx}",
                    kind_char(raw >> 4),
                    raw & 0b11
                );
            }
        }

        let _ = ig.stream.seek_to(off + 0x0C);
        let loaded_tag = ig.stream.read_u32().unwrap_or(0);
        if loaded_tag != 0 {
            let start = u64::from(loaded_tag);
            let end = u64::from(h.frames_ptr);
            if end > start && end - start < 4096 {
                let n = (end - start) as usize;
                println!("--- loaded_tag block [0x{start:X}..0x{end:X}) = {n} bytes ---");
                if ig.stream.seek_to(start).is_ok() {
                    let mut words: Vec<i16> = Vec::with_capacity(n / 2);
                    for _ in 0..n / 2 {
                        match ig.stream.read_i16() {
                            Ok(v) => words.push(v),
                            Err(_) => break,
                        }
                    }
                    for (i, chunk) in words.chunks(16).enumerate() {
                        println!("  +0x{:03X} {:?}", i * 32, chunk);
                    }
                }
            } else {
                println!("--- loaded_tag=0x{loaded_tag:X} does not sit just before frames_ptr=0x{:X} ---", h.frames_ptr);
            }
        }

        match decode_animation_with_skel(&mut ig, &h, &ctrl, pos_scale, scale_scale, &skel, profile)
        {
            Ok(clip) => {
                let mut worst: Vec<(usize, f32, f32, u8)> = Vec::new();
                for (bi, bone) in clip.bones.iter().enumerate() {
                    if !bone.scale_animated || bone.scales.is_empty() {
                        continue;
                    }
                    let mn = bone.scales.iter().copied().fold(f32::INFINITY, f32::min);
                    let mx = bone
                        .scales
                        .iter()
                        .copied()
                        .fold(f32::NEG_INFINITY, f32::max);
                    let bm = ctrl.blend_masks.get(bi).copied().unwrap_or(255);
                    worst.push((bi, mn, mx, bm));
                }
                worst.sort_by(|a, b| {
                    (b.2 - b.1)
                        .abs()
                        .partial_cmp(&(a.2 - a.1).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                println!("decoded: {} bones with scale channel; worst 12:", worst.len());
                for (bi, mn, mx, bm) in worst.iter().take(12) {
                    println!("  bone {bi} scale [{mn:.4}..{mx:.4}] blend_mask={bm}");
                }
            }
            Err(e) => println!("decode FAILED: {e}"),
        }
    }

    ExitCode::SUCCESS
}
