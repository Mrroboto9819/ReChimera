use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;

use lunalib::{AssetKind, AssetLookup, IgFile};

fn main() -> ExitCode {
    let folder = match env::args().nth(1) {
        Some(a) => a,
        None => {
            eprintln!("usage: probe_foliage_asset <level_folder>");
            return ExitCode::FAILURE;
        }
    };
    let level = Path::new(&folder);
    let mut lookup =
        AssetLookup::open(BufReader::new(File::open(level.join("assetlookup.dat")).unwrap()))
            .unwrap();
    let ptrs = lookup.pointers(AssetKind::Foliage).unwrap();
    let mut f = File::open(level.join("foliages.dat")).unwrap();
    for ptr in ptrs {
        f.seek(SeekFrom::Start(u64::from(ptr.offset))).unwrap();
        let mut buf = vec![0u8; ptr.length as usize];
        f.read_exact(&mut buf).unwrap();
        let mut ig = IgFile::open(Cursor::new(buf)).unwrap();
        eprintln!("foliage 0x{:016X} chunk len 0x{:X}", ptr.tuid, ptr.length);
        for id in [0xA000u32, 0xA200, 0xA300, 0x5600] {
            match ig.section(id) {
                Some(s) => eprintln!(
                    "  section 0x{id:04X}: offset=0x{:X} count={} length=0x{:X}",
                    s.offset, s.count, s.length
                ),
                None => eprintln!("  section 0x{id:04X}: absent"),
            }
        }
        if let Some(h) = ig.section(0xA200) {
            let off = u64::from(h.offset);
            ig.stream.seek_to(off).unwrap();
            let n = (u64::from(h.length).min(0xC0) / 4) as usize;
            let mut words = Vec::new();
            for _ in 0..n.max(48) {
                match ig.stream.read_u32() {
                    Ok(w) => words.push(w),
                    Err(_) => break,
                }
            }
            for (i, chunk) in words.chunks(8).enumerate() {
                let hexs: Vec<String> = chunk.iter().map(|w| format!("{w:08X}")).collect();
                eprintln!("  hdr +0x{:02X}: {}", i * 32, hexs.join(" "));
            }
        }
        if let Some(b) = ig.section(0xA000) {
            let off = u64::from(b.offset);
            ig.stream.seek_to(off).unwrap();
            let mut words = Vec::new();
            for _ in 0..24 {
                match ig.stream.read_u32() {
                    Ok(w) => words.push(w),
                    Err(_) => break,
                }
            }
            for (i, chunk) in words.chunks(8).enumerate() {
                let hexs: Vec<String> = chunk.iter().map(|w| format!("{w:08X}")).collect();
                eprintln!("  buf +0x{:02X}: {}", i * 32, hexs.join(" "));
            }
        }
    }
    ExitCode::SUCCESS
}
