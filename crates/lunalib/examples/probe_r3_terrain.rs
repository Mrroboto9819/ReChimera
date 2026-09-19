use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lunalib::{encode_png, read_shaders, read_textures, read_zones};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(folder) = args.next() else {
        eprintln!("usage: probe_r3_terrain <level_folder> [png_out_dir]");
        return ExitCode::from(2);
    };
    let out_dir = args.next().map(PathBuf::from);
    let level = Path::new(&folder);

    let zones = read_zones(level).expect("read_zones");
    let shaders = read_shaders(level).expect("read_shaders");
    println!(
        "{} zones, {} shaders",
        zones.len(),
        shaders.len()
    );

    struct Agg {
        tris: u64,
        ufrags: usize,
        min_u: f32,
        max_u: f32,
        min_v: f32,
        max_v: f32,
        nan_uv: usize,
        albedo: Option<u32>,
        normal: Option<u32>,
        detail: Option<u32>,
        shader_tuid: Option<u64>,
        resolved: bool,
    }
    let mut per_shader: HashMap<(usize, u16), Agg> = HashMap::new();

    for (zi, zone) in zones.iter().enumerate() {
        for u in &zone.ufrags {
            if u.positions.is_empty() {
                continue;
            }
            let st = zone.ufrag_shader_tuids.get(u.shader_index as usize).copied();
            let info = st.and_then(|t| shaders.get(&t));
            let e = per_shader.entry((zi, u.shader_index)).or_insert(Agg {
                tris: 0,
                ufrags: 0,
                min_u: f32::INFINITY,
                max_u: f32::NEG_INFINITY,
                min_v: f32::INFINITY,
                max_v: f32::NEG_INFINITY,
                nan_uv: 0,
                albedo: info.and_then(|s| s.albedo_tex_id),
                normal: info.and_then(|s| s.normal_tex_id),
                detail: info.and_then(|s| s.detail_tex_id),
                shader_tuid: st,
                resolved: info.is_some(),
            });
            e.tris += u64::from(u.index_count) / 3;
            e.ufrags += 1;
            for uv in u.uvs.chunks_exact(2) {
                if !uv[0].is_finite() || !uv[1].is_finite() {
                    e.nan_uv += 1;
                    continue;
                }
                if uv[0] < e.min_u {
                    e.min_u = uv[0];
                }
                if uv[0] > e.max_u {
                    e.max_u = uv[0];
                }
                if uv[1] < e.min_v {
                    e.min_v = uv[1];
                }
                if uv[1] > e.max_v {
                    e.max_v = uv[1];
                }
            }
        }
    }

    let mut rows: Vec<((usize, u16), Agg)> = per_shader.into_iter().collect();
    rows.sort_by(|a, b| b.1.tris.cmp(&a.1.tris));
    println!("top ufrag shader groups by triangle count:");
    for ((zi, si), a) in rows.iter().take(20) {
        println!(
            "  zone{} shader_index={:<4} tris={:<8} ufrags={:<4} shader={} resolved={} albedo={:?} normal={:?} detail={:?} U=[{:.2}..{:.2}] V=[{:.2}..{:.2}]{}",
            zi,
            si,
            a.tris,
            a.ufrags,
            a.shader_tuid
                .map(|t| format!("0x{t:016X}"))
                .unwrap_or_else(|| "OOB".into()),
            a.resolved,
            a.albedo,
            a.normal,
            a.detail,
            a.min_u,
            a.max_u,
            a.min_v,
            a.max_v,
            if a.nan_uv > 0 {
                format!(" nan_uv={}", a.nan_uv)
            } else {
                String::new()
            }
        );
    }

    let unresolved: usize = rows.iter().filter(|(_, a)| !a.resolved).count();
    let no_albedo: usize = rows
        .iter()
        .filter(|(_, a)| a.resolved && a.albedo.is_none())
        .count();
    println!(
        "groups total={} unresolved_shader={} resolved_but_no_albedo={}",
        rows.len(),
        unresolved,
        no_albedo
    );

    println!("\nimplied vertex stride census (gap between consecutive vertex_offsets / vertex_count):");
    for (zi, zone) in zones.iter().enumerate() {
        let mut offs: Vec<(u32, u16)> = zone
            .ufrags
            .iter()
            .map(|u| (u.vertex_offset, u.vertex_count))
            .collect();
        offs.sort_unstable();
        let mut hist: HashMap<u32, usize> = HashMap::new();
        for w in offs.windows(2) {
            let (o0, n0) = w[0];
            let (o1, _) = w[1];
            if n0 == 0 || o1 <= o0 {
                continue;
            }
            let stride = (o1 - o0) / u32::from(n0);
            *hist.entry(stride).or_insert(0) += 1;
        }
        let mut h: Vec<(u32, usize)> = hist.into_iter().collect();
        h.sort_by(|a, b| b.1.cmp(&a.1));
        println!("  zone{zi}: {:?}", &h[..h.len().min(6)]);
    }

    println!("\nvertex position outlier census (per-ufrag max |xyz| vs bsphere radius):");
    let mut outliers = 0usize;
    let mut checked = 0usize;
    for (zi, zone) in zones.iter().enumerate() {
        for u in &zone.ufrags {
            if u.positions.is_empty() {
                continue;
            }
            checked += 1;
            let mut maxd = 0f32;
            for p in u.positions.chunks_exact(3) {
                let dx = p[0] - u.position[0];
                let dy = p[1] - u.position[1];
                let dz = p[2] - u.position[2];
                let d = (dx * dx + dy * dy + dz * dz).sqrt();
                if d > maxd {
                    maxd = d;
                }
            }
            if maxd > u.radius * 4.0 + 10.0 {
                outliers += 1;
                if outliers <= 12 {
                    println!(
                        "  OUTLIER zone{zi} ufrag 0x{:016X} shader_index={} verts={} maxdist={:.1} vs radius {:.1} pos [{:.1} {:.1} {:.1}]",
                        u.tuid, u.shader_index, u.vertex_count, maxd, u.radius,
                        u.position[0], u.position[1], u.position[2]
                    );
                }
            }
        }
    }
    println!("  {} outlier ufrags / {} checked", outliers, checked);

    println!("\nraw ufrag record interpretations (zone asset 0, first 8 records):");
    {
        use std::fs::File;
        use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
        let lookup_file = File::open(level.join("assetlookup.dat")).expect("assetlookup");
        let mut lookup =
            lunalib::AssetLookup::open(BufReader::new(lookup_file)).expect("assetlookup parse");
        let zone_ptrs = lookup
            .pointers(lunalib::AssetKind::Zone)
            .expect("zone ptrs");
        let mut zones_file = File::open(level.join("zones.dat")).expect("zones.dat");
        for ptr in zone_ptrs.iter().take(1) {
            zones_file
                .seek(SeekFrom::Start(u64::from(ptr.offset)))
                .expect("seek");
            let mut buf = vec![0u8; ptr.length as usize];
            zones_file.read_exact(&mut buf).expect("read zone");
            let ig = lunalib::IgFile::open(Cursor::new(buf)).expect("igfile");
            let Some(section) = ig.sections.iter().find(|s| s.id == 0x6200).copied() else {
                println!("  no 0x6200 section");
                continue;
            };
            println!(
                "  0x6200: count={} length={} offset=0x{:X}",
                section.count, section.length, section.offset
            );
            let mut stream = ig.stream;
            for i in 0..(section.count as usize).min(8) {
                let base = u64::from(section.offset) + (i as u64) * 0x80;
                stream.seek_to(base).expect("seek rec");
                let mut rec = [0u8; 0x80];
                for b in rec.iter_mut() {
                    *b = stream.read_u8().unwrap_or(0);
                }
                let f32_at = |o: usize| {
                    f32::from_be_bytes([rec[o], rec[o + 1], rec[o + 2], rec[o + 3]])
                };
                let u64_at = |o: usize| {
                    u64::from_be_bytes([
                        rec[o],
                        rec[o + 1],
                        rec[o + 2],
                        rec[o + 3],
                        rec[o + 4],
                        rec[o + 5],
                        rec[o + 6],
                        rec[o + 7],
                    ])
                };
                println!(
                    "  [{i}] u64@00=0x{:016X}  f32x4@30=({:.2},{:.2},{:.2} r={:.2})  f32x4@60=({:.2},{:.2},{:.2} r={:.2})  f32x4@00=({:.2},{:.2},{:.2},{:.2})",
                    u64_at(0x00),
                    f32_at(0x30),
                    f32_at(0x34),
                    f32_at(0x38),
                    f32_at(0x3C),
                    f32_at(0x60),
                    f32_at(0x64),
                    f32_at(0x68),
                    f32_at(0x6C),
                    f32_at(0x00),
                    f32_at(0x04),
                    f32_at(0x08),
                    f32_at(0x0C),
                );
            }
        }
    }

    println!("\ntie instance envelope for sanity (zone0):");
    if let Some(z0) = zones.first() {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for t in &z0.tie_instances {
            for a in 0..3 {
                if t.position[a] < min[a] {
                    min[a] = t.position[a];
                }
                if t.position[a] > max[a] {
                    max[a] = t.position[a];
                }
            }
        }
        println!(
            "  {} ties min=({:.1},{:.1},{:.1}) max=({:.1},{:.1},{:.1})",
            z0.tie_instances.len(),
            min[0],
            min[1],
            min[2],
            max[0],
            max[1],
            max[2]
        );
    }

    let Some(out_dir) = out_dir else {
        return ExitCode::SUCCESS;
    };
    fs::create_dir_all(&out_dir).expect("mkdir out");
    let textures = read_textures(level).expect("read_textures");
    let mut wanted: Vec<u32> = rows
        .iter()
        .take(8)
        .flat_map(|(_, a)| [a.albedo, a.normal, a.detail])
        .flatten()
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    for id in wanted {
        let Some(t) = textures
            .iter()
            .find(|t| t.id == id)
            .or_else(|| textures.get(id as usize))
        else {
            println!("tex id {id}: NOT FOUND ({} textures)", textures.len());
            continue;
        };
        if !t.is_decoded() {
            println!(
                "tex id {id}: NOT DECODED format={} {}x{}",
                t.format.name(),
                t.width,
                t.height
            );
            continue;
        }
        let png = encode_png(&t.rgba, t.width, t.height);
        if png.is_empty() {
            println!("tex id {id}: png encode produced no data");
            continue;
        }
        let p = out_dir.join(format!(
            "tex_{id}_{}_{}x{}.png",
            t.format.name(),
            t.width,
            t.height
        ));
        fs::write(&p, png).expect("write png");
        println!("wrote {}", p.display());
    }
    ExitCode::SUCCESS
}
