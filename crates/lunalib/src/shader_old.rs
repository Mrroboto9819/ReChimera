//! TOD-era shader reader (R&C: Tools of Destruction).
//!
//! Ported from ReLunacy `LibLunacy/Shader.cs::CShader` `OldShader`
//! branch + `LibLunacy/AssetLoader.cs::LoadShadersOld`.
//!
//! ## Layout
//! - `main.dat` section `0x5000` — array of `count` × `0x80` bytes,
//!   each entry is an `OldShader` struct.
//! - Each `OldShader` references its three texture maps via **byte
//!   offsets into `main.dat`**, not via tuids. Those offsets
//!   correspond to entries in section `0x5200` — see
//!   [`crate::texture_old`] which keys textures by exactly that
//!   offset.
//!
//! ## OldShader struct (0x80 bytes)
//! - `+0x00` `u32` `albedoOffset`     — main.dat offset of the albedo texture record
//! - `+0x04` `u32` `normalOffset`
//! - `+0x08` `u32` `expensiveOffset`  — used as emissive in the V2 path
//! - `+0x11` `u8`  `renderingMode`    — 0 opaque, 4 alpha-clip, 6 alpha-blend
//! - `+0x20` `f32` `alphaClip`

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::error::Result;
use crate::igfile::IgFile;
use crate::shader::ShaderInfo;

const SECT_OLD_SHADER: u32 = 0x5000;
const OLD_SHADER_SIZE: u64 = 0x80;

/// Read every TOD shader from `main.dat`.
///
/// Returns a `HashMap` keyed by **shader index cast to u64** so
/// downstream code can look up `shaders[shader_tuids[mesh.shader_index]]`
/// the same way it does for V2 (where shader_tuid is also a u64). For
/// TOD the moby's `shader_tuids` list is just the identity mapping
/// `[0, 1, 2, …, N-1]` over the global shader DB.
///
/// Each `ShaderInfo`'s `*_tex_id` fields are the texture's main.dat
/// byte offset — the same value that `texture_old::read_textures_old`
/// uses for the `Texture::id` field. So the existing GLB-writer
/// material-builder works unchanged.
pub fn read_shaders_old(level_folder: &Path) -> Result<HashMap<u64, ShaderInfo>> {
    let main_path = level_folder.join("main.dat");
    let mut main_ig = IgFile::open(BufReader::new(File::open(&main_path)?))?;

    let section = match main_ig.section(SECT_OLD_SHADER) {
        Some(s) => s,
        None => return Ok(HashMap::new()),
    };

    let mut out: HashMap<u64, ShaderInfo> = HashMap::with_capacity(section.count as usize);
    for i in 0..(section.count as u64) {
        let base = u64::from(section.offset) + i * OLD_SHADER_SIZE;
        main_ig.stream.seek_to(base + 0x00)?;
        let albedo_offset = main_ig.stream.read_u32()?;
        let normal_offset = main_ig.stream.read_u32()?;
        let expensive_offset = main_ig.stream.read_u32()?;

        out.insert(
            i,
            ShaderInfo {
                tuid: i,
                albedo_tex_id: nonzero(albedo_offset),
                normal_tex_id: nonzero(normal_offset),
                expensive_tex_id: nonzero(expensive_offset),
                detail_tex_id: None,
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
