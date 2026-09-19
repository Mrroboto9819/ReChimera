use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;

use lunalib::{
    animation_section_offsets, read_animation_header_at, AssetKind, AssetLookup, IgFile,
};

fn main() -> ExitCode {
    let folder = match env::args().nth(1) {
        Some(a) => a,
        None => {
            eprintln!("usage: list_characters <level_folder>");
            return ExitCode::FAILURE;
        }
    };
    let level = Path::new(&folder);

    struct Row {
        tuid: u64,
        name: String,
        bones: usize,
        animset: Option<u64>,
        verts: usize,
        submeshes: usize,
    }
    let mut rows: Vec<Row> = Vec::new();
    let r = lunalib::read_moby_assets_with_total(level, None, |_| {}, |a| {
        let verts: usize = a
            .bangles
            .iter()
            .flat_map(|b| b.meshes.iter())
            .map(|m| m.vertex_count as usize)
            .sum();
        let submeshes: usize = a.bangles.iter().map(|b| b.meshes.len()).sum();
        rows.push(Row {
            tuid: a.tuid,
            name: a.name.clone(),
            bones: a.skeleton.as_ref().map_or(0, |s| s.bones.len()),
            animset: a.animset_hash,
            verts,
            submeshes,
        });
    });
    if let Err(e) = r {
        eprintln!("read mobys: {e}");
        return ExitCode::FAILURE;
    }

    let mut clip_info: HashMap<u64, (usize, Vec<String>)> = HashMap::new();
    if let (Ok(lookup_file), Ok(mut animsets_file)) = (
        File::open(level.join("assetlookup.dat")),
        File::open(level.join("animsets.dat")),
    ) {
        if let Ok(mut lookup) = AssetLookup::open(BufReader::new(lookup_file)) {
            if let Ok(ptrs) = lookup.pointers(AssetKind::Animset) {
                for p in &ptrs {
                    if animsets_file
                        .seek(SeekFrom::Start(u64::from(p.offset)))
                        .is_err()
                    {
                        continue;
                    }
                    let mut buf = vec![0u8; p.length as usize];
                    if animsets_file.read_exact(&mut buf).is_err() {
                        continue;
                    }
                    let Ok(mut ig) = IgFile::open(Cursor::new(buf)) else {
                        continue;
                    };
                    let offs = animation_section_offsets(&ig);
                    let mut samples: Vec<String> = Vec::new();
                    for off in offs.iter().take(400) {
                        if samples.len() >= 6 {
                            break;
                        }
                        if let Ok(h) = read_animation_header_at(&mut ig, *off) {
                            if !h.name.is_empty() {
                                samples.push(h.name);
                            }
                        }
                    }
                    clip_info.insert(p.tuid, (offs.len(), samples));
                }
            }
        }
    }

    rows.sort_by(|a, b| b.bones.cmp(&a.bones).then(b.verts.cmp(&a.verts)));

    println!("=== rigged mobys (skeleton present) ===");
    for r in rows.iter().filter(|r| r.bones > 0) {
        let (clips, samples) = r
            .animset
            .and_then(|h| clip_info.get(&h))
            .map(|(c, s)| (*c, s.join(", ")))
            .unwrap_or((0, String::new()));
        println!(
            "0x{:016X} bones={:<3} verts={:<6} sub={:<3} clips={:<4} name='{}'\n    clips: {}",
            r.tuid, r.bones, r.verts, r.submeshes, clips, r.name, samples
        );
    }

    println!("\n=== unrigged mobys ===");
    for r in rows.iter().filter(|r| r.bones == 0) {
        println!(
            "0x{:016X} verts={:<6} sub={:<3} name='{}'",
            r.tuid, r.verts, r.submeshes, r.name
        );
    }

    ExitCode::SUCCESS
}
