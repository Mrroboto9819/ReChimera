use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(input), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: extract_psarc <archive.psarc> <out_dir>");
        return ExitCode::FAILURE;
    };
    let out_dir = PathBuf::from(out);

    let mut archive = match psarc::Archive::open(Path::new(&input)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("open {input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let entries = archive.entries.clone();
    let total = entries.len();
    eprintln!("{total} entries");
    for (i, entry) in entries.iter().enumerate() {
        let bytes = match archive.read_entry(entry) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("read {}: {e}", entry.name);
                return ExitCode::FAILURE;
            }
        };
        let mut rel = entry.name.replace('\\', "/");
        while rel.starts_with('/') {
            rel.remove(0);
        }
        if rel.split('/').any(|seg| seg == "..") {
            eprintln!("path traversal blocked: {}", entry.name);
            return ExitCode::FAILURE;
        }
        let dest = out_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("mkdir {}: {e}", parent.display());
                return ExitCode::FAILURE;
            }
        }
        if let Err(e) = std::fs::write(&dest, &bytes) {
            eprintln!("write {}: {e}", dest.display());
            return ExitCode::FAILURE;
        }
        if (i + 1) % 500 == 0 || i + 1 == total {
            eprintln!("{}/{total}", i + 1);
        }
    }
    ExitCode::SUCCESS
}
