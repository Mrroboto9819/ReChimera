//! RFOM-era shader/material reader.
//!
//! Ported from InsomniaToolset (`common/include/insomnia/classes/shader.hpp`
//! `MaterialV1` + `levelmain/extract.cpp::MakeMaterials` line 258). Each
//! 0x5001 record is **128 bytes** (matches the RFOM probe).
//!
//! ## MaterialV1 layout (`0x80` bytes)
//! - `+0x00` `float unk0[5]`           — 20 bytes
//! - `+0x14` `uint32 unk1[2]`          — 8 bytes
//! - `+0x1C` packed-bit flags byte (`useSpecular`, `useGlossiness`,
//!           `useNormalMap`, `useDetailMap`, …)
//! - `+0x1D` `uint8 blendMode`         — 0 opaque, 4 alpha-mask, else blend
//! - `+0x1E..+0x20` 2 unknown bytes
//! - `+0x20` `PointerX86<Texture> textures[4]` — 16 bytes (albedo,
//!   normal, special, detail). Each is a u32 file offset into
//!   `ps3levelmain.dat` pointing at a 0x5300 TextureV1 entry, or 0 if
//!   the slot is unused.
//! - `+0x30` `Vector4A16 values[5]`    — 80 bytes
//!
//! ## ID convention (mirrors TOD)
//! [`ShaderInfo`]'s `*_tex_id` fields are **byte offsets of the
//! corresponding 0x5300 entry inside `ps3levelmain.dat`**, NOT tuids.
//! [`crate::texture_rfom`] keys textures by exactly that offset, so
//! the existing GLB material builder works unchanged.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::error::Result;
use crate::igfile::IgFile;
use crate::shader::ShaderInfo;

const SECT_MATERIAL_V1: u32 = 0x5001;
const MATERIAL_V1_SIZE: u64 = 0x80;

pub fn read_shaders_rfom(level_folder: &Path) -> Result<HashMap<u64, ShaderInfo>> {
    let main_path = level_folder.join("ps3levelmain.dat");
    let mut main_ig = IgFile::open(BufReader::new(File::open(&main_path)?))?;

    let section = match main_ig.section(SECT_MATERIAL_V1) {
        Some(s) => s,
        None => return Ok(HashMap::new()),
    };
    if section.length != MATERIAL_V1_SIZE as u32 {
        eprintln!(
            "warn: 0x5001 section length is {} (expected {} for MaterialV1) — skipping",
            section.length, MATERIAL_V1_SIZE
        );
        return Ok(HashMap::new());
    }

    let mut out: HashMap<u64, ShaderInfo> = HashMap::with_capacity(section.count as usize);
    for i in 0..(section.count as u64) {
        let base = u64::from(section.offset) + i * MATERIAL_V1_SIZE;
        main_ig.stream.seek_to(base + 0x20)?;
        let albedo = main_ig.stream.read_u32()?;
        let normal = main_ig.stream.read_u32()?;
        let special = main_ig.stream.read_u32()?;
        let detail = main_ig.stream.read_u32()?;

        out.insert(
            i,
            ShaderInfo {
                tuid: i,
                albedo_tex_id: nonzero(albedo),
                normal_tex_id: nonzero(normal),
                expensive_tex_id: nonzero(special),
                detail_tex_id: nonzero(detail),
            },
        );
    }
    Ok(out)
}

fn nonzero(v: u32) -> Option<u32> {
    if v == 0 {
        None
    } else {
        Some(v)
    }
}
