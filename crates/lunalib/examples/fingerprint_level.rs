use std::env;
use std::path::Path;
use std::process::ExitCode;

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x100000001B3);
    }
}

fn main() -> ExitCode {
    let arg = match env::args().nth(1) {
        Some(a) => a,
        None => {
            eprintln!("usage: fingerprint_level <level_folder>");
            return ExitCode::FAILURE;
        }
    };
    let folder = Path::new(&arg);
    let layout = match lunalib::detect_layout(folder) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("detect_layout failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let engine = lunalib::engine_for_layout(layout);
    println!("layout={}", layout.tag());

    let mut h: u64 = 0xCBF29CE484222325;
    let mut count = 0usize;
    let mut verts = 0u64;
    let mut inds = 0u64;
    let mut skel_bones = 0u64;
    let r = engine.read_mobys(
        folder,
        None,
        &mut |_| {},
        &mut |a: lunalib::MobyAsset| {
            count += 1;
            fnv(&mut h, &a.tuid.to_le_bytes());
            for b in &a.bangles {
                for m in &b.meshes {
                    verts += u64::from(m.vertex_count);
                    inds += u64::from(m.index_count);
                    for p in &m.positions {
                        fnv(&mut h, &p.to_bits().to_le_bytes());
                    }
                    for i in &m.indices {
                        fnv(&mut h, &i.to_le_bytes());
                    }
                    for w in &m.bone_weights {
                        fnv(&mut h, &[*w]);
                    }
                    for bi in &m.bone_indices {
                        fnv(&mut h, &bi.to_le_bytes());
                    }
                }
            }
            if let Some(s) = &a.skeleton {
                skel_bones += s.bones.len() as u64;
                for m in &s.bind_local {
                    for f in m {
                        fnv(&mut h, &f.to_bits().to_le_bytes());
                    }
                }
            }
        },
    );
    match r {
        Ok(()) => println!(
            "mobys count={count} verts={verts} indices={inds} skel_bones={skel_bones} hash={h:016X}"
        ),
        Err(e) => println!("mobys ERROR {e}"),
    }

    let mut h: u64 = 0xCBF29CE484222325;
    let mut count = 0usize;
    let mut verts = 0u64;
    let r = engine.read_ties(
        folder,
        None,
        &mut |_| {},
        &mut |t: lunalib::TieAsset| {
            count += 1;
            fnv(&mut h, &t.tuid.to_le_bytes());
            for m in &t.meshes {
                verts += u64::from(m.vertex_count);
                for p in &m.positions {
                    fnv(&mut h, &p.to_bits().to_le_bytes());
                }
                for i in &m.indices {
                    fnv(&mut h, &i.to_le_bytes());
                }
            }
        },
    );
    match r {
        Ok(()) => println!("ties count={count} verts={verts} hash={h:016X}"),
        Err(e) => println!("ties ERROR {e}"),
    }

    match engine.read_shaders(folder) {
        Ok(map) => {
            let mut keys: Vec<u64> = map.keys().copied().collect();
            keys.sort_unstable();
            let mut h: u64 = 0xCBF29CE484222325;
            for k in &keys {
                fnv(&mut h, &k.to_le_bytes());
                let s = &map[k];
                for id in [s.albedo_tex_id, s.normal_tex_id, s.expensive_tex_id] {
                    fnv(&mut h, &id.unwrap_or(u32::MAX).to_le_bytes());
                }
            }
            println!("shaders count={} hash={h:016X}", keys.len());
        }
        Err(e) => println!("shaders ERROR {e}"),
    }

    match engine.read_textures(folder) {
        Ok(texs) => {
            let mut h: u64 = 0xCBF29CE484222325;
            let mut px = 0u64;
            for t in &texs {
                fnv(&mut h, &t.id.to_le_bytes());
                fnv(&mut h, &t.width.to_le_bytes());
                fnv(&mut h, &t.height.to_le_bytes());
                px += t.rgba.len() as u64;
            }
            println!("textures count={} rgba_bytes={px} hash={h:016X}", texs.len());
        }
        Err(e) => println!("textures ERROR {e}"),
    }

    match engine.read_zones(folder) {
        Ok(zones) => {
            let mut h: u64 = 0xCBF29CE484222325;
            let mut ufrags = 0usize;
            let mut tie_insts = 0usize;
            for z in &zones {
                fnv(&mut h, &z.tuid.to_le_bytes());
                tie_insts += z.tie_instances.len();
                ufrags += z.ufrags.len();
                for u in &z.ufrags {
                    fnv(&mut h, &u.tuid.to_le_bytes());
                    for p in &u.positions {
                        fnv(&mut h, &p.to_bits().to_le_bytes());
                    }
                    for i in &u.indices {
                        fnv(&mut h, &i.to_le_bytes());
                    }
                }
            }
            println!(
                "zones count={} tie_instances={tie_insts} ufrags={ufrags} hash={h:016X}",
                zones.len()
            );
        }
        Err(e) => println!("zones ERROR {e}"),
    }

    match engine.read_gameplay(folder) {
        Ok(gp) => {
            let mut h: u64 = 0xCBF29CE484222325;
            let mut insts = 0usize;
            for region in &gp.regions {
                insts += region.moby_instances.len();
                for inst in &region.moby_instances {
                    fnv(&mut h, &inst.moby_tuid.to_le_bytes());
                    for v in inst.position {
                        fnv(&mut h, &v.to_bits().to_le_bytes());
                    }
                }
            }
            println!(
                "gameplay regions={} moby_instances={insts} hash={h:016X}",
                gp.regions.len()
            );
        }
        Err(e) => println!("gameplay ERROR {e}"),
    }

    ExitCode::SUCCESS
}
