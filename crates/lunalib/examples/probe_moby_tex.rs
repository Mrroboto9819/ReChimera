use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::process::ExitCode;

use lunalib::{read_moby_assets, read_shaders, read_textures};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(folder) = args.next() else {
        eprintln!("usage: probe_moby_tex <level_folder> <moby_tuid_hex>");
        return ExitCode::FAILURE;
    };
    let Some(tuid) = args
        .next()
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok())
    else {
        eprintln!("usage: probe_moby_tex <level_folder> <moby_tuid_hex>");
        return ExitCode::FAILURE;
    };
    let folder = Path::new(&folder);

    let mobys = match read_moby_assets(folder, Some(&[tuid])) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("read_moby_assets: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(asset) = mobys.iter().find(|a| a.tuid == tuid) else {
        eprintln!("moby 0x{tuid:016X} not found");
        return ExitCode::FAILURE;
    };
    println!("moby 0x{tuid:016X} '{}'", asset.name);
    println!("shader palette ({}):", asset.shader_tuids.len());
    let shaders_early = read_shaders(folder).unwrap_or_default();
    let early_textures = read_textures(folder).unwrap_or_default();
    let early_ids: HashSet<u32> = early_textures.iter().map(|t| t.id).collect();
    for (i, s) in asset.shader_tuids.iter().enumerate() {
        match shaders_early.get(s) {
            Some(sh) => {
                let fmt = |id: Option<u32>| match id {
                    None => "none".to_string(),
                    Some(v) if early_ids.contains(&v) => format!("0x{v:08X}(in-level)"),
                    Some(v) => format!("0x{v:08X}(ABSENT)"),
                };
                println!(
                    "  [{i}] 0x{s:016X} albedo={} normal={} exp={}",
                    fmt(sh.albedo_tex_id),
                    fmt(sh.normal_tex_id),
                    fmt(sh.expensive_tex_id)
                );
            }
            None => println!("  [{i}] 0x{s:016X} NOT-IN-SHADER-TABLE"),
        }
    }

    let shaders = match read_shaders(folder) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("read_shaders: {e}");
            return ExitCode::FAILURE;
        }
    };
    let textures = read_textures(folder).unwrap_or_default();
    let level_tex_ids: HashSet<u32> = textures.iter().map(|t| t.id).collect();
    let decoded_ids: HashSet<u32> = textures.iter().filter(|t| t.is_decoded()).map(|t| t.id).collect();
    println!("level textures: {} ({} decoded)", level_tex_ids.len(), decoded_ids.len());

    for (b_idx, bangle) in asset.bangles.iter().enumerate() {
        for (m_idx, mesh) in bangle.meshes.iter().enumerate() {
            let shader_tuid = asset
                .shader_tuids
                .get(mesh.shader_index as usize)
                .copied()
                .unwrap_or(0);
            match shaders.get(&shader_tuid) {
                Some(sh) => {
                    let check = |label: &str, id: Option<u32>| match id {
                        None => format!("{label}=none"),
                        Some(v) if decoded_ids.contains(&v) => format!("{label}=0x{v:08X} OK"),
                        Some(v) if level_tex_ids.contains(&v) => {
                            format!("{label}=0x{v:08X} PRESENT-BUT-UNDECODED")
                        }
                        Some(v) => format!("{label}=0x{v:08X} MISSING-FROM-LEVEL"),
                    };
                    println!(
                        "  b{b_idx}.m{m_idx} shader_idx={} tuid=0x{shader_tuid:016X} {} {} {}",
                        mesh.shader_index,
                        check("albedo", sh.albedo_tex_id),
                        check("normal", sh.normal_tex_id),
                        check("expensive", sh.expensive_tex_id),
                    );
                }
                None => {
                    println!(
                        "  b{b_idx}.m{m_idx} shader_idx={} tuid=0x{shader_tuid:016X} SHADER NOT IN LEVEL SHADER TABLE",
                        mesh.shader_index
                    );
                }
            }
        }
    }
    ExitCode::SUCCESS
}
