use std::env;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process::ExitCode;

use lunalib::{find_global_sound_bank_dirs, list_sounds, streaming_sibling_for, IgFile, SoundKind};

fn main() -> ExitCode {
    let folder = match env::args().nth(1) {
        Some(a) => a,
        None => {
            eprintln!("usage: probe_global_sounds <level_folder>");
            return ExitCode::FAILURE;
        }
    };
    let level = Path::new(&folder);
    let dirs = find_global_sound_bank_dirs(level);
    eprintln!("found {} global bank dirs", dirs.len());

    let mut total_banks = 0usize;
    let mut total_sounds = 0usize;
    let mut with_stream_sibling = 0usize;
    let mut printed = 0usize;
    for d in &dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(".dat") || lower.contains("stream") {
                continue;
            }
            if !lower.starts_with("resident_sound") && !lower.starts_with("resident_dialogue") {
                continue;
            }
            total_banks += 1;
            let full = d.join(&name);
            let full_str = full.to_string_lossy().into_owned();
            let sibling = streaming_sibling_for(&full_str);
            if let Some(s) = &sibling {
                if Path::new(s).is_file() {
                    with_stream_sibling += 1;
                }
            }
            let Ok(f) = File::open(&full) else { continue };
            let Ok(mut ig) = IgFile::open(BufReader::new(f)) else { continue };
            match list_sounds(&mut ig) {
                Ok(sums) => {
                    total_sounds += sums.len();
                    for s in sums {
                        println!(
                            "{}\t{}\t{}",
                            s.name,
                            match s.kind {
                                SoundKind::Bank => "bank",
                                SoundKind::Stream => "stream",
                            },
                            name,
                        );
                        printed += 1;
                    }
                }
                Err(e) => eprintln!("  list_sounds {} failed: {e}", full.display()),
            }
        }
    }
    eprintln!(
        "TOTAL: {} banks, {} sounds, {} banks have their streaming sibling on disk",
        total_banks, total_sounds, with_stream_sibling
    );
    ExitCode::SUCCESS
}
