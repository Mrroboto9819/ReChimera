use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;

use lunalib::{AssetKind, AssetLookup, IgFile};

fn main() -> ExitCode {
    let root = match env::args().nth(1) {
        Some(a) => a,
        None => {
            eprintln!("usage: probe_foliage_records <levels_root_or_level> [dump]");
            return ExitCode::FAILURE;
        }
    };
    let dump = env::args().nth(2).is_some();
    let mut levels: Vec<std::path::PathBuf> = Vec::new();
    let root = Path::new(&root);
    if root.join("assetlookup.dat").exists() {
        levels.push(root.to_path_buf());
    } else if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            let inner = p
                .join("built")
                .join("levels")
                .join(p.file_name().unwrap_or_default());
            if inner.join("assetlookup.dat").exists() {
                levels.push(inner);
            } else if p.join("assetlookup.dat").exists() {
                levels.push(p);
            }
        }
    }

    for level in levels {
        let name = level
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(f) = File::open(level.join("assetlookup.dat")) else {
            continue;
        };
        let Ok(mut lookup) = AssetLookup::open(BufReader::new(f)) else {
            continue;
        };
        let Ok(ptrs) = lookup.pointers(AssetKind::Zone) else {
            continue;
        };
        let Ok(mut zf) = File::open(level.join("zones.dat")) else {
            continue;
        };
        for ptr in ptrs {
            if zf.seek(SeekFrom::Start(u64::from(ptr.offset))).is_err() {
                continue;
            }
            let mut buf = vec![0u8; ptr.length as usize];
            if zf.read_exact(&mut buf).is_err() {
                continue;
            }
            let Ok(mut ig) = IgFile::open(Cursor::new(buf)) else {
                continue;
            };
            let Some(inst) = ig.section(0x7440) else {
                continue;
            };
            if inst.count == 0 {
                continue;
            }
            let table = ig.section(0x7400);
            let (tc, tl) = table.map(|t| (t.count, t.length)).unwrap_or((0, 0));
            eprintln!(
                "{name}: zone 0x{:016X} foliage count={} entry_len=0x{:X} table count={} entry_len=0x{:X}",
                ptr.tuid, inst.count, inst.length, tc, tl
            );
            if dump {
                let stride = u64::from(inst.length);
                let n = (inst.count as u64).min(6);
                for i in 0..n {
                    let base = u64::from(inst.offset) + i * stride;
                    let mut words = Vec::new();
                    ig.stream.seek_to(base + 0x40).unwrap();
                    for _ in 0..16 {
                        words.push(ig.stream.read_u32().unwrap());
                    }
                    let hexs: Vec<String> =
                        words.iter().map(|v| format!("{v:08X}")).collect();
                    eprintln!("  rec[{i}] +0x40..0x80: {}", hexs.join(" "));
                }
                let mut per_slot: Vec<Vec<u32>> = vec![Vec::new(); 16];
                for i in 0..inst.count as u64 {
                    let base = u64::from(inst.offset) + i * stride;
                    ig.stream.seek_to(base + 0x40).unwrap();
                    for slot in per_slot.iter_mut() {
                        slot.push(ig.stream.read_u32().unwrap());
                    }
                }
                for (s, vals) in per_slot.iter().enumerate() {
                    let max = vals.iter().max().copied().unwrap_or(0);
                    let distinct: std::collections::BTreeSet<_> = vals.iter().collect();
                    if max < 4096 {
                        eprintln!(
                            "  slot +0x{:02X}: max={} distinct={} (small-int candidate)",
                            0x40 + s * 4,
                            max,
                            distinct.len()
                        );
                    }
                }
            }
        }
    }
    ExitCode::SUCCESS
}
