use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(input) = env::args().nth(1) else {
        eprintln!("usage: list_psarc <archive.psarc>");
        return ExitCode::FAILURE;
    };
    let archive = match psarc::Archive::open(Path::new(&input)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("open {input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    for entry in &archive.entries {
        println!("{}", entry.name);
    }
    ExitCode::SUCCESS
}
