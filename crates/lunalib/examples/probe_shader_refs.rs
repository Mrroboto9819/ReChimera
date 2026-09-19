use std::env;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;

use lunalib::{AssetKind, AssetLookup, IgFile};

fn read_cstr<R: Read + Seek>(ig: &mut IgFile<R>, at: u64) -> String {
    if ig.stream.seek_to(at).is_err() {
        return String::new();
    }
    let mut s = String::new();
    for _ in 0..128 {
        match ig.stream.read_u8() {
            Ok(0) | Err(_) => break,
            Ok(b) => s.push(b as char),
        }
    }
    s
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(folder), Some(tuid_raw)) = (args.next(), args.next()) else {
        eprintln!("usage: probe_shader_refs <level_folder> <shader_tuid_hex>");
        return ExitCode::FAILURE;
    };
    let target = u64::from_str_radix(tuid_raw.trim_start_matches("0x"), 16).unwrap_or(0);
    let level = Path::new(&folder);

    let mut lookup =
        AssetLookup::open(BufReader::new(File::open(level.join("assetlookup.dat")).unwrap()))
            .unwrap();
    let ptrs = lookup.pointers(AssetKind::Shader).unwrap();
    let Some(ptr) = ptrs.iter().find(|p| p.tuid == target) else {
        eprintln!("shader 0x{target:016X} not in table");
        return ExitCode::FAILURE;
    };
    let mut f = File::open(level.join("shaders.dat")).unwrap();
    f.seek(SeekFrom::Start(u64::from(ptr.offset))).unwrap();
    let mut buf = vec![0u8; ptr.length as usize];
    f.read_exact(&mut buf).unwrap();
    let mut ig = IgFile::open(Cursor::new(buf)).unwrap();

    let Some(refs) = ig.section(0x5D00) else {
        eprintln!("no 0x5D00 section");
        return ExitCode::FAILURE;
    };
    let base = u64::from(refs.offset);
    ig.stream.seek_to(base).unwrap();
    let this_tuid = ig.stream.read_u64().unwrap_or(0);
    let name_ptr = ig.stream.read_u32().unwrap_or(0);
    let _pad = ig.stream.read_u32().unwrap_or(0);
    let a = ig.stream.read_u32().unwrap_or(0);
    let n = ig.stream.read_u32().unwrap_or(0);
    let e = ig.stream.read_u32().unwrap_or(0);
    ig.stream.seek_to(base + 0x28).unwrap();
    let a_name_ptr = ig.stream.read_u32().unwrap_or(0);
    let n_name_ptr = ig.stream.read_u32().unwrap_or(0);
    let e_name_ptr = ig.stream.read_u32().unwrap_or(0);

    println!("shader chunk 0x{target:016X} (0x5D00 @0x{base:X}, len={})", refs.length);
    println!("  thisTuid=0x{this_tuid:016X}");
    println!("  name       @0x{name_ptr:08X}: '{}'", read_cstr(&mut ig, u64::from(name_ptr)));
    println!("  albedo    0x{a:08X} @0x{a_name_ptr:08X}: '{}'", read_cstr(&mut ig, u64::from(a_name_ptr)));
    println!("  normal    0x{n:08X} @0x{n_name_ptr:08X}: '{}'", read_cstr(&mut ig, u64::from(n_name_ptr)));
    println!("  expensive 0x{e:08X} @0x{e_name_ptr:08X}: '{}'", read_cstr(&mut ig, u64::from(e_name_ptr)));
    ExitCode::SUCCESS
}
