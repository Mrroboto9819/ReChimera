use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let configs_dir = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or(r"C:\Users\flast\Downloads\rpcs3-v0.0.40-19397-20096463_win64_msvc\dev_hdd0\game\NPEA00431\USRDIR\packed\game\global_cached\data\configs");

    let names = lunalib::load_outfitter_names_from_configs_dir(Path::new(configs_dir))
        .expect("parse outfitter configs");

    println!("named TUIDs: {}", names.by_tuid.len());
    let mut sorted: Vec<(&u64, &String)> = names.by_tuid.iter().collect();
    sorted.sort_by_key(|(k, _)| **k);
    for (tuid, name) in sorted {
        println!("  0x{:016X}  {}", tuid, name);
    }
}
