


use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::assetlookup::{AssetKind, AssetLookup};
use crate::error::Result;
use crate::igfile::IgFile;

const SECT_SHADER_REFS: u32 = 0x5D00;

#[derive(Debug, Clone, Copy)]
pub struct ShaderInfo {
    pub tuid: u64,

    pub albedo_tex_id: Option<u32>,
    pub normal_tex_id: Option<u32>,
    pub expensive_tex_id: Option<u32>,
    // Why: R2 (V2) shaders use IT's `MaterialResourceNameLookup`
    // (shader.hpp:213, V1 layout) — 4 hashes at base+0x10..0x20 then 4
    // `mapLookupPaths` u32 pointers at base+0x20..0x30. Slot order is
    // diffuse / normal / specular / detail per V1_5's `Material`
    // (shader.hpp:184). Detail map is sometimes the only populated slot
    // on shaders whose other 3 are zero. Slots past offset 0x20 are
    // path-string pointers, NOT texture hashes — empirically they read
    // as tiny values (0x190-0x2B0) and binding them would mis-render.
    pub detail_tex_id: Option<u32>,
}


pub fn read_shaders(level_folder: &Path) -> Result<HashMap<u64, ShaderInfo>> {
    let assetlookup_path = level_folder.join("assetlookup.dat");
    let mut lookup = AssetLookup::open(BufReader::new(File::open(&assetlookup_path)?))?;
    let ptrs = lookup.pointers(AssetKind::Shader)?;
    if ptrs.is_empty() {
        return Ok(HashMap::new());
    }

    let shaders_path = level_folder.join("shaders.dat");
    let mut shaders_file = File::open(&shaders_path)?;

    let mut out = HashMap::with_capacity(ptrs.len());
    for ptr in ptrs {
        if ptr.length > crate::MAX_ASSET_SIZE {
            return Err(crate::error::Error::AllocLimitExceeded {
                size: u64::from(ptr.length),
                limit: u64::from(crate::MAX_ASSET_SIZE),
            });
        }
        shaders_file.seek(SeekFrom::Start(u64::from(ptr.offset)))?;
        let mut buf = vec![0u8; ptr.length as usize];
        shaders_file.read_exact(&mut buf)?;
        let mut shader_ig = IgFile::open(Cursor::new(buf))?;

        match parse_shader(&mut shader_ig, ptr.tuid) {
            Ok(info) => {
                out.insert(info.tuid, info);
            }
            Err(_e) => {

            }
        }
    }
    Ok(out)
}

fn parse_shader<R: Read + Seek>(ig: &mut IgFile<R>, tuid: u64) -> Result<ShaderInfo> {
    let Some(refs) = ig.section(SECT_SHADER_REFS) else {
        return Ok(ShaderInfo {
            tuid,
            albedo_tex_id: None,
            normal_tex_id: None,
            expensive_tex_id: None,
            detail_tex_id: None,
        });
    };

    let base = u64::from(refs.offset);
    let section_len = refs.length;
    ig.stream.seek_to(base + 0x10)?;
    let albedo = ig.stream.read_u32()?;
    let normal = ig.stream.read_u32()?;
    let expensive = ig.stream.read_u32()?;
    let detail = ig.stream.read_u32().unwrap_or(0);

    if std::env::var("RECHIMERA_LOG_SHADER_SLOTS").is_ok() && detail != 0 {
        eprintln!(
            "[shader-slots] tuid=0x{:016X} a=0x{:08X} n=0x{:08X} s=0x{:08X} d=0x{:08X}",
            tuid, albedo, normal, expensive, detail
        );
    }

    if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static FIRED: AtomicUsize = AtomicUsize::new(0);
        let n = FIRED.fetch_add(1, Ordering::Relaxed);
        if n < 3 {
            let take = (section_len as usize).min(0x80);
            ig.stream.seek_to(base)?;
            let mut bytes = vec![0u8; take];
            for b in bytes.iter_mut() {
                *b = ig.stream.read_u8().unwrap_or(0);
            }
            let mut hex = String::new();
            for (i, b) in bytes.iter().enumerate() {
                if i % 16 == 0 && i > 0 {
                    hex.push('\n');
                    hex.push_str("            ");
                }
                hex.push_str(&format!("{:02X} ", b));
            }
            let mut u32s = String::new();
            for i in 0..(take / 4) {
                let off = i * 4;
                let v = u32::from_be_bytes([bytes[off], bytes[off+1], bytes[off+2], bytes[off+3]]);
                if i % 4 == 0 && i > 0 {
                    u32s.push('\n');
                    u32s.push_str("            ");
                }
                u32s.push_str(&format!("+0x{:02X}=0x{:08X} ", off, v));
            }
            eprintln!(
                "[shader-probe] tuid=0x{:016X} 0x5D00 length={} take={}\n  raw: {}\n  u32 BE: {}",
                tuid, section_len, take, hex, u32s
            );
        }
    }

    Ok(ShaderInfo {
        tuid,
        albedo_tex_id: (albedo != 0).then_some(albedo),
        normal_tex_id: (normal != 0).then_some(normal),
        expensive_tex_id: (expensive != 0).then_some(expensive),
        detail_tex_id: (detail != 0).then_some(detail),
    })
}
