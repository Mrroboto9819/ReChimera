use std::env;
use std::path::Path;
use std::process::ExitCode;

use lunalib::bulk_extract_pngs;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(folder), Some(ids_raw)) = (args.next(), args.next()) else {
        eprintln!("usage: probe_tex_ids <level_folder> <id_hex,id_hex,...>");
        return ExitCode::FAILURE;
    };
    let ids: Vec<u32> = ids_raw
        .split(',')
        .filter_map(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .collect();
    match bulk_extract_pngs(Path::new(&folder), Some(&ids), 64) {
        Ok(r) => {
            for (id, png) in &r {
                println!("HIT 0x{id:08X} ({} bytes)", png.len());
            }
            if r.is_empty() {
                println!("none");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bulk_extract_pngs: {e}");
            ExitCode::FAILURE
        }
    }
}
