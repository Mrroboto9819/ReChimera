use std::env;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process::ExitCode;

use lunalib::{AssetKind, AssetLookup};

fn main() -> ExitCode {
    let folder = match env::args().nth(1) {
        Some(a) => a,
        None => {
            eprintln!("usage: probe_zone_table <level_folder>");
            return ExitCode::FAILURE;
        }
    };
    let level = Path::new(&folder);
    let mut lookup =
        AssetLookup::open(BufReader::new(File::open(level.join("assetlookup.dat")).unwrap()))
            .unwrap();
    let ptrs = lookup.pointers(AssetKind::Zone).unwrap();
    let zones_len = std::fs::metadata(level.join("zones.dat")).map(|m| m.len()).unwrap_or(0);
    eprintln!("zones.dat size = {zones_len} bytes, {} zone pointers", ptrs.len());
    for (i, p) in ptrs.iter().enumerate() {
        let end = u64::from(p.offset) + u64::from(p.length);
        let flag = if end > zones_len { "  <-- PAST EOF" } else { "" };
        eprintln!(
            "zone[{i:2}] tuid=0x{:016X} offset=0x{:08X} len=0x{:08X} end=0x{end:08X}{flag}",
            p.tuid, p.offset, p.length
        );
    }

    if let Some(bad) = ptrs.iter().find(|p| p.tuid == 0x16A917EFD041C6B1) {
        use std::io::{Cursor, Read, Seek, SeekFrom};
        let mut f = File::open(level.join("zones.dat")).unwrap();
        f.seek(SeekFrom::Start(u64::from(bad.offset))).unwrap();
        let mut buf = vec![0u8; bad.length as usize];
        f.read_exact(&mut buf).unwrap();
        let chunk_len = buf.len() as u64;
        let ig = lunalib::IgFile::open(Cursor::new(buf)).unwrap();
        eprintln!("--- bad zone chunk: {chunk_len} bytes ---");
        for id in [0x7100u32, 0x7140, 0x7200, 0x7240, 0x7400, 0x7440, 0x7500, 0x7540, 0x6200] {
            match ig.section(id) {
                Some(s) => {
                    eprintln!(
                        "section 0x{id:04X}: offset=0x{:08X} count={} length=0x{:X} end=0x{:X}{}",
                        s.offset,
                        s.count,
                        s.length,
                        u64::from(s.offset) + u64::from(s.length),
                        if u64::from(s.offset) + u64::from(s.length) > chunk_len {
                            "  <-- PAST CHUNK END"
                        } else {
                            ""
                        }
                    );
                }
                None => eprintln!("section 0x{id:04X}: absent"),
            }
        }
        if let Some(s) = ig.section(0x7440) {
            let need = u64::from(s.offset) + u64::from(s.count) * 0xB0;
            eprintln!(
                "foliage instances: count={} stride 0xB0 needs bytes up to 0x{need:X} (chunk 0x{chunk_len:X}){}",
                s.count,
                if need > chunk_len { "  <-- PAST CHUNK END" } else { "" }
            );
            let mut ig = ig;
            let mut hist = std::collections::BTreeMap::<u32, usize>::new();
            for i in 0..s.count as u64 {
                let base = u64::from(s.offset) + i * 0xB0;
                ig.stream.seek_to(base + 0x94).unwrap();
                let idx = ig.stream.read_u32().unwrap();
                *hist.entry(idx).or_default() += 1;
            }
            let ranges: Vec<String> = hist
                .iter()
                .take(20)
                .map(|(k, v)| format!("{k}x{v}"))
                .collect();
            eprintln!(
                "foliage_index histogram ({} distinct): {}",
                hist.len(),
                ranges.join(", ")
            );
            for rec in 0..2u64 {
                let base = u64::from(s.offset) + rec * 0xB0;
                ig.stream.seek_to(base + 0x80).unwrap();
                let mut tail = Vec::new();
                for _ in 0..12 {
                    tail.push(ig.stream.read_u32().unwrap());
                }
                let hexs: Vec<String> = tail.iter().map(|v| format!("{v:08X}")).collect();
                eprintln!("record[{rec}] +0x80..0xB0 u32s: {}", hexs.join(" "));
            }
        }
    }

    match lunalib::read_zones(level) {
        Ok(zones) => {
            eprintln!(
                "read_zones OK: {} zones, {} tie instances, {} ufrags, {} shrubs, {} foliage",
                zones.len(),
                zones.iter().map(|z| z.tie_instances.len()).sum::<usize>(),
                zones.iter().map(|z| z.ufrags.len()).sum::<usize>(),
                zones.iter().map(|z| z.shrub_instances.len()).sum::<usize>(),
                zones.iter().map(|z| z.foliage_instances.len()).sum::<usize>(),
            );
            for z in &zones {
                for f in z.foliage_instances.iter().take(3) {
                    eprintln!(
                        "foliage: tuid=0x{:016X} pos=({:.1},{:.1},{:.1}) scale=({:.3},{:.3},{:.3}) r={:.1} quat=({:.3},{:.3},{:.3},{:.3})",
                        f.tie_tuid,
                        f.position[0], f.position[1], f.position[2],
                        f.scale[0], f.scale[1], f.scale[2],
                        f.bounding_radius,
                        f.quaternion[0], f.quaternion[1], f.quaternion[2], f.quaternion[3],
                    );
                }
            }
            if let Ok(mut lookup2) =
                AssetLookup::open(BufReader::new(File::open(level.join("assetlookup.dat")).unwrap()))
            {
                if let Ok(fptrs) = lookup2.pointers(AssetKind::Foliage) {
                    eprintln!("level foliage asset table: {} entries", fptrs.len());
                    for p in fptrs.iter().take(5) {
                        eprintln!("  foliage asset 0x{:016X} len=0x{:X}", p.tuid, p.length);
                    }
                }
            }
            match lunalib::read_foliage_v2(level) {
                Ok(assets) => {
                    for a in &assets {
                        for m in &a.meshes {
                            let verts = m.positions.len() / 3;
                            let (mut minv, mut maxv) = ([f32::MAX; 3], [f32::MIN; 3]);
                            for v in 0..verts {
                                for c in 0..3 {
                                    let x = m.positions[v * 3 + c];
                                    if x < minv[c] { minv[c] = x; }
                                    if x > maxv[c] { maxv[c] = x; }
                                }
                            }
                            eprintln!(
                                "foliage mesh 0x{:016X}: verts={} indices={} shaders={} aabb min=({:.1},{:.1},{:.1}) max=({:.1},{:.1},{:.1})",
                                a.tuid,
                                verts,
                                m.indices.len(),
                                a.shader_tuids.len(),
                                minv[0], minv[1], minv[2],
                                maxv[0], maxv[1], maxv[2],
                            );
                        }
                    }
                }
                Err(e) => eprintln!("read_foliage_v2 FAILED: {e}"),
            }
        }
        Err(e) => eprintln!("read_zones FAILED: {e}"),
    }
    ExitCode::SUCCESS
}
