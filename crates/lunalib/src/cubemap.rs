use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::assetlookup::{AssetKind, AssetLookup};
use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::texture::{decode_format, TexFormat};

const SECT_CUBEMAP_DESC: u32 = 0x5920;
const SECT_CUBEMAP_DATA: u32 = 0x5940;
const FACES: usize = 6;

#[derive(Debug, Clone)]
pub struct CubemapFace {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Cubemap {
    pub hash: u64,
    pub width: u32,
    pub height: u32,
    pub format: TexFormat,
    pub faces: Vec<CubemapFace>,
}

pub fn read_cubemaps(level_folder: &Path) -> Result<Vec<Cubemap>> {
    let assetlookup_path = level_folder.join("assetlookup.dat");
    let mut lookup = AssetLookup::open(BufReader::new(File::open(&assetlookup_path)?))?;
    let ptrs = lookup.pointers(AssetKind::Cubemap)?;
    if ptrs.is_empty() {
        return Ok(Vec::new());
    }

    let cubemaps_path = level_folder.join("cubemaps.dat");
    let mut file = File::open(&cubemaps_path)?;
    let mut out = Vec::with_capacity(ptrs.len());

    for ptr in ptrs {
        if ptr.length > crate::MAX_ASSET_SIZE {
            return Err(Error::AllocLimitExceeded {
                size: u64::from(ptr.length),
                limit: u64::from(crate::MAX_ASSET_SIZE),
            });
        }
        file.seek(SeekFrom::Start(u64::from(ptr.offset)))?;
        let mut buf = vec![0u8; ptr.length as usize];
        file.read_exact(&mut buf)?;
        match parse_cubemap(&buf, ptr.tuid) {
            Ok(cm) => out.push(cm),
            Err(e) => eprintln!(
                "[cubemap] skipping 0x{:016X}: {}",
                ptr.tuid, e
            ),
        }
    }
    Ok(out)
}

fn parse_cubemap(buf: &[u8], hash: u64) -> Result<Cubemap> {
    let mut ig = IgFile::open(Cursor::new(buf.to_vec()))?;
    let desc = ig.require_section(SECT_CUBEMAP_DESC)?;
    let data = ig.require_section(SECT_CUBEMAP_DATA)?;

    // 0x5920 descriptor — 32 bytes, mirrors IT's `Texture` struct
    // (shader.hpp:126). Layout per RSX NV4097 texture register dump:
    //   +0x00 u32 offset
    //   +0x04 u16 numMips
    //   +0x06 u8  format       <- format byte (V2 high-range encoding)
    //   +0x07 u8  flags
    //   +0x08 u32 address
    //   +0x0C u32 control0
    //   +0x10 u32 control3
    //   +0x14 u32 filter
    //   +0x18 u16 width
    //   +0x1A u16 height
    //   +0x1C u32 borderColor
    let base = u64::from(desc.offset);
    ig.stream.seek_to(base + 0x06)?;
    let format_byte = ig.stream.read_u8()?;
    let _flags = ig.stream.read_u8()?;
    ig.stream.seek_to(base + 0x04)?;
    let num_mips = ig.stream.read_u16()?;
    ig.stream.seek_to(base + 0x18)?;
    let width_field = ig.stream.read_u16()?;
    let height_field = ig.stream.read_u16()?;
    let mut format = TexFormat::from_format_byte(format_byte);

    // 0x5940 — 6 faces × per-face byte stride, base mip first, then
    // a halving mip chain padded so each mip is at least one DXT
    // block (32 bytes per face for trailing 4×4-and-smaller mips).
    ig.stream.seek_to(u64::from(data.offset))?;
    let raw = ig.stream.read_bytes(data.length as usize)?;
    if raw.len() % FACES != 0 {
        return Err(Error::SectionLengthMismatch {
            id: SECT_CUBEMAP_DATA,
            length: data.length,
            entry: FACES as u32,
        });
    }
    let face_stride = raw.len() / FACES;

    // R2 axbridge_coop reports format byte 0x9A which isn't in IT's
    // public TextureFormat enum (0x81..0x8B, 0xA6). Per shader.hpp:118
    // BC1 (DXT1) lives at 0x86 for 2D textures, but cubemap descriptors
    // seem to use a different format-byte range. Since the size math
    // is unambiguous (11008 = full DXT1 mip chain to 1×1 for 128×128),
    // assume DXT1 when the byte is unknown and the data length agrees.
    if matches!(format, TexFormat::Unknown(_))
        && derive_dxt_base_dim(face_stride, TexFormat::Dxt1).is_some()
    {
        format = TexFormat::Dxt1;
    }

    // The descriptor's width/height fields aren't always trustworthy
    // (R2 axbridge_coop reports w=h=32 even though the data is plainly
    // 128×128 — 8192 bytes per base mip + mip chain = 11008/face).
    // So derive the *base mip* size from the face stride using the
    // DXT block-size math, and fall back to the descriptor only if
    // the math doesn't resolve.
    let (base_w, base_h) = derive_dxt_base_dim(face_stride, format)
        .unwrap_or((u32::from(width_field), u32::from(height_field)));

    let base_mip_bytes = dxt_mip_size(base_w, base_h, format) as usize;
    let face_bytes_available = face_stride.min(raw.len() / FACES);
    let base_bytes = base_mip_bytes.min(face_bytes_available);

    let mut faces = Vec::with_capacity(FACES);
    for f in 0..FACES {
        let start = f * face_stride;
        let end = start + base_bytes;
        let rgba = decode_format(&raw[start..end], base_w, base_h, format);
        faces.push(CubemapFace {
            width: base_w,
            height: base_h,
            rgba,
        });
    }

    let _ = num_mips;
    Ok(Cubemap {
        hash,
        width: base_w,
        height: base_h,
        format,
        faces,
    })
}

fn dxt_mip_size(w: u32, h: u32, format: TexFormat) -> u32 {
    let block_bytes = match format {
        TexFormat::Dxt1 | TexFormat::Bc1Linear => 8,
        TexFormat::Dxt3 | TexFormat::Dxt5 => 16,
        _ => return w * h * 4,
    };
    let bw = w.max(4).div_ceil(4);
    let bh = h.max(4).div_ceil(4);
    // PS3 cubemap mip chain pads every level to at least 32 bytes
    // per face (RSX texel-tile alignment). Without this floor, the
    // 4×4 / 2×2 / 1×1 mips report only 8 bytes and the chain math
    // disagrees with the actual on-disk stride (11 008 vs 10 936
    // for 128×128 DXT1) → fallback to DXT1 fails to trigger.
    let raw = bw * bh * block_bytes;
    raw.max(32)
}

fn dxt_mip_chain_size(base_w: u32, base_h: u32, format: TexFormat) -> u32 {
    let mut total = 0u32;
    let mut w = base_w;
    let mut h = base_h;
    loop {
        total += dxt_mip_size(w, h, format);
        if w == 1 && h == 1 {
            break;
        }
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    total
}

fn derive_dxt_base_dim(face_stride: usize, format: TexFormat) -> Option<(u32, u32)> {
    if !matches!(
        format,
        TexFormat::Dxt1 | TexFormat::Dxt3 | TexFormat::Dxt5 | TexFormat::Bc1Linear
    ) {
        return None;
    }
    for pow in 0..=10u32 {
        let side = 1u32 << pow;
        if dxt_mip_chain_size(side, side, format) as usize == face_stride {
            return Some((side, side));
        }
    }
    None
}
