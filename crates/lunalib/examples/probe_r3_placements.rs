use std::env;
use std::path::Path;
use std::process::ExitCode;

use lunalib::{read_gameplay, read_zones, AssetKind, AssetLookup, IgFile};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};

fn finite3(v: [f32; 3]) -> bool {
    v.iter().all(|c| c.is_finite())
}

fn main() -> ExitCode {
    let Some(folder) = env::args().nth(1) else {
        eprintln!("usage: probe_r3_placements <level_folder>");
        return ExitCode::from(2);
    };
    let level = Path::new(&folder);

    // ---- moby placements (gameplay) ----
    let mut mn = [f32::INFINITY; 3];
    let mut mx = [f32::NEG_INFINITY; 3];
    let mut moby_total = 0usize;
    let mut moby_zero_pos = 0usize;
    let mut moby_bad = 0usize;
    let mut scale_hist: std::collections::BTreeMap<i64, usize> = Default::default();
    let mut rot_nonzero = 0usize;
    if let Ok(gp) = read_gameplay(level) {
        for region in &gp.regions {
            for inst in &region.moby_instances {
                moby_total += 1;
                if !finite3(inst.position) || !inst.scale.is_finite() {
                    moby_bad += 1;
                    if moby_bad <= 10 {
                        println!(
                            "  MOBY BAD 0x{:016X} '{}' pos={:?} rot={:?} scale={}",
                            inst.moby_tuid, inst.name, inst.position, inst.rotation, inst.scale
                        );
                    }
                    continue;
                }
                if inst.position == [0.0, 0.0, 0.0] {
                    moby_zero_pos += 1;
                }
                for a in 0..3 {
                    if inst.position[a] < mn[a] {
                        mn[a] = inst.position[a];
                    }
                    if inst.position[a] > mx[a] {
                        mx[a] = inst.position[a];
                    }
                }
                let sb = (inst.scale * 10.0).round() as i64;
                *scale_hist.entry(sb).or_insert(0) += 1;
                if inst.rotation.iter().any(|r| r.abs() > 1e-3) {
                    rot_nonzero += 1;
                }
            }
        }
    } else {
        println!("  read_gameplay failed");
    }
    println!(
        "MOBY placements: total={} zero_pos={} nonfinite={} rot_nonzero={} envelope min={:?} max={:?}",
        moby_total, moby_zero_pos, moby_bad, rot_nonzero, mn, mx
    );
    println!("  moby scale histogram (scale*10 -> count):");
    for (k, v) in &scale_hist {
        println!("    scale≈{:.1} : {}", *k as f32 / 10.0, v);
    }
    // sample a few rotated mobys to eyeball rotation magnitude (radians vs degrees)
    if let Ok(gp) = read_gameplay(level) {
        let mut shown = 0;
        for region in &gp.regions {
            for inst in &region.moby_instances {
                if inst.rotation.iter().any(|r| r.abs() > 1e-3) && shown < 10 {
                    println!(
                        "  ROT sample '{}' rot(rad?)={:?} |deg|={:?}",
                        inst.name,
                        inst.rotation,
                        inst.rotation.map(|r| r.to_degrees())
                    );
                    shown += 1;
                }
            }
        }
    }

    // ---- tie instances (zones) ----
    let mut tn = [f32::INFINITY; 3];
    let mut tx = [f32::NEG_INFINITY; 3];
    let mut tie_total = 0usize;
    let mut tie_bad = 0usize;
    let mut tie_zero = 0usize;
    let mut tscale_hist: std::collections::BTreeMap<i64, usize> = Default::default();
    let mut quat_bad = 0usize;
    if let Ok(zones) = read_zones(level) {
        for z in &zones {
            for t in &z.tie_instances {
                tie_total += 1;
                let ok = finite3(t.position) && finite3(t.scale) && t.quaternion.iter().all(|c| c.is_finite());
                if !ok {
                    tie_bad += 1;
                    if tie_bad <= 10 {
                        println!(
                            "  TIE BAD 0x{:016X} '{}' pos={:?} scale={:?} quat={:?}",
                            t.tie_tuid, t.name, t.position, t.scale, t.quaternion
                        );
                    }
                    continue;
                }
                if t.position == [0.0, 0.0, 0.0] {
                    tie_zero += 1;
                }
                let qn = (t.quaternion[0] * t.quaternion[0]
                    + t.quaternion[1] * t.quaternion[1]
                    + t.quaternion[2] * t.quaternion[2]
                    + t.quaternion[3] * t.quaternion[3])
                    .sqrt();
                if (qn - 1.0).abs() > 0.05 {
                    quat_bad += 1;
                    if quat_bad <= 10 {
                        println!(
                            "  TIE QUAT non-unit |q|={:.3} '{}' scale={:?} quat={:?}",
                            qn, t.name, t.scale, t.quaternion
                        );
                    }
                }
                for a in 0..3 {
                    if t.position[a] < tn[a] {
                        tn[a] = t.position[a];
                    }
                    if t.position[a] > tx[a] {
                        tx[a] = t.position[a];
                    }
                }
                let sb = (t.scale[0] * 10.0).round() as i64;
                *tscale_hist.entry(sb).or_insert(0) += 1;
            }
        }
    } else {
        println!("  read_zones failed");
    }
    println!(
        "TIE instances: total={} zero_pos={} nonfinite={} nonunit_quat={} envelope min={:?} max={:?}",
        tie_total, tie_zero, tie_bad, quat_bad, tn, tx
    );
    println!("  tie scale[x] histogram (scale*10 -> count):");
    for (k, v) in &tscale_hist {
        println!("    scale≈{:.1} : {}", *k as f32 / 10.0, v);
    }

    // ---- determinant census on ALL tie matrices (find reflections) ----
    println!("\ntie 3x3 determinant census (sign after column normalization):");
    {
        const TIE_INSTANCE_SIZE: u64 = 0x80;
        let lookup_file = File::open(level.join("assetlookup.dat")).expect("assetlookup");
        let mut lookup = AssetLookup::open(BufReader::new(lookup_file)).expect("parse");
        let zptrs = lookup.pointers(AssetKind::Zone).expect("zone ptrs");
        let mut zones_file = File::open(level.join("zones.dat")).expect("zones.dat");
        let mut neg_det = 0usize;
        let mut sheared = 0usize;
        let mut total = 0usize;
        let mut samples = 0usize;
        for ptr in &zptrs {
            zones_file.seek(SeekFrom::Start(u64::from(ptr.offset))).unwrap();
            let mut buf = vec![0u8; ptr.length as usize];
            zones_file.read_exact(&mut buf).unwrap();
            let ig = IgFile::open(Cursor::new(buf)).expect("igfile");
            let Some(sec) = ig.sections.iter().find(|s| s.id == 0x7240).copied() else { continue };
            let mut stream = ig.stream;
            for i in 0..sec.count as usize {
                let base = u64::from(sec.offset) + (i as u64) * TIE_INSTANCE_SIZE;
                stream.seek_to(base).unwrap();
                let mut m = [0f32; 16];
                for slot in m.iter_mut() { *slot = stream.read_f32().unwrap(); }
                total += 1;
                let col = |c: usize| [m[c], m[4 + c], m[8 + c]];
                let dot = |a: [f32; 3], b: [f32; 3]| a[0]*b[0]+a[1]*b[1]+a[2]*b[2];
                let cross = |a: [f32;3], b: [f32;3]| [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]];
                let (c0, c1, c2) = (col(0), col(1), col(2));
                let (l0, l1, l2) = (dot(c0,c0).sqrt(), dot(c1,c1).sqrt(), dot(c2,c2).sqrt());
                if l0 == 0.0 || l1 == 0.0 || l2 == 0.0 { continue; }
                let n = |v: [f32;3], l: f32| [v[0]/l, v[1]/l, v[2]/l];
                let (u0, u1, u2) = (n(c0,l0), n(c1,l1), n(c2,l2));
                let det = dot(u0, cross(u1, u2));
                let shear = dot(u0,u1).abs().max(dot(u0,u2).abs()).max(dot(u1,u2).abs());
                if det < 0.0 { neg_det += 1; }
                if shear > 0.02 { sheared += 1; }
                if (det < 0.0 || shear > 0.02) && samples < 8 {
                    samples += 1;
                    println!("  det={:.3} shear={:.3} |cols|=({:.2},{:.2},{:.2})", det, shear, l0, l1, l2);
                }
            }
        }
        println!("  TIES total={} negative_det(reflection)={} sheared(>0.02)={}", total, neg_det, sheared);
    }

    // ---- raw tie-instance matrix dump (first zone, first 6 records) ----
    println!("\nraw tie-instance 4x4 (first zone, 6 records) — check orthogonality of R after /scale:");
    {
        const TIE_INSTANCE_SIZE: u64 = 0x80;
        let lookup_file = File::open(level.join("assetlookup.dat")).expect("assetlookup");
        let mut lookup = AssetLookup::open(BufReader::new(lookup_file)).expect("parse");
        let zptrs = lookup.pointers(AssetKind::Zone).expect("zone ptrs");
        let mut zones_file = File::open(level.join("zones.dat")).expect("zones.dat");
        for ptr in zptrs.iter().take(1) {
            zones_file.seek(SeekFrom::Start(u64::from(ptr.offset))).unwrap();
            let mut buf = vec![0u8; ptr.length as usize];
            zones_file.read_exact(&mut buf).unwrap();
            let ig = IgFile::open(Cursor::new(buf)).expect("igfile");
            let Some(sec) = ig.sections.iter().find(|s| s.id == 0x7240).copied() else {
                println!("  no 0x7240 tie-instance section");
                continue;
            };
            println!("  0x7240 count={} length={} (record size assumed 0x80)", sec.count, sec.length);
            let mut stream = ig.stream;
            for i in 0..(sec.count as usize).min(6) {
                let base = u64::from(sec.offset) + (i as u64) * TIE_INSTANCE_SIZE;
                stream.seek_to(base).unwrap();
                let mut m = [0f32; 16];
                for slot in m.iter_mut() {
                    *slot = stream.read_f32().unwrap();
                }
                // column lengths of the 3x3 (row-major reading: rows are m[0..3],m[4..7],m[8..11])
                let col = |c: usize| [m[c], m[4 + c], m[8 + c]];
                let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                let (c0, c1, c2) = (col(0), col(1), col(2));
                let (l0, l1, l2) = (dot(c0, c0).sqrt(), dot(c1, c1).sqrt(), dot(c2, c2).sqrt());
                let n = |v: [f32; 3], l: f32| [v[0] / l, v[1] / l, v[2] / l];
                let (u0, u1, u2) = (n(c0, l0), n(c1, l1), n(c2, l2));
                println!(
                    "  [{i}] |cols|=({:.3},{:.3},{:.3}) col-dots after norm 01={:.3} 02={:.3} 12={:.3}  trans=({:.1},{:.1},{:.1}) lastrow=({:.2},{:.2},{:.2},{:.2})",
                    l0, l1, l2,
                    dot(u0, u1), dot(u0, u2), dot(u1, u2),
                    m[12], m[13], m[14],
                    m[3], m[7], m[11], m[15]
                );
            }
        }
    }

    ExitCode::SUCCESS
}
