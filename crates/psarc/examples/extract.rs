use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: extract <input.psarc> <output_dir>");
        std::process::exit(2);
    }
    let input = PathBuf::from(&args[1]);
    let out_root = PathBuf::from(&args[2]);
    fs::create_dir_all(&out_root).expect("create output dir");

    let mut archive = psarc::Archive::open(&input).expect("open psarc");
    let total = archive.entries.len();
    println!("PSARC: {} entries, compression={:?}, version={}.{}",
        total, archive.header.compression,
        archive.header.major, archive.header.minor);

    let entries: Vec<psarc::Entry> = archive.entries.clone();
    for (i, entry) in entries.iter().enumerate() {
        let bytes = match archive.read_entry(entry) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  [{:5}/{}] FAIL {:?}: {}", i+1, total, entry.name, e);
                continue;
            }
        };
        let rel = entry.name.trim_start_matches('/').replace('\\', "/");
        let dest = out_root.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&dest, &bytes).expect("write file");
        if i % 25 == 0 || i + 1 == total {
            println!("  [{:5}/{}] {} ({} bytes)", i+1, total, rel, bytes.len());
        }
    }
    println!("done -> {}", out_root.display());
    let _ = Path::new("");
}
