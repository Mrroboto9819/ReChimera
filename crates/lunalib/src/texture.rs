

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::igfile::IgFile;

const SECT_TEX_META: u32 = 0x1D140;
const SECT_HIGHMIP_PTRS: u32 = 0x1D1C0;

const NEW_TEX_META_SIZE: u64 = 0x04;
const ASSET_POINTER_SIZE: u64 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexFormat {
    R5G6B5,
    A8R8G8B8,
    Dxt1,
    Dxt3,
    Dxt5,
    R8,
    Rg8,
    Rgb5A1,
    Rgba4,
    Bc1Linear,

    Unknown(u8),
}

impl TexFormat {
    /// Map the format byte to a known format.
    ///
    /// Insomniac stores TWO related-but-distinct format-byte spaces, and
    /// **both must keep working** because different games use different ones:
    ///
    /// - **Low range `0x03..0x0A`** — the short enum stored in
    ///   `assetlookup.dat`'s per-texture metadata table. Used by **Resistance 2**
    ///   (verified end-to-end) and most likely R3 / R&C Tools of Destruction.
    ///
    /// - **High range `0x81..0x8B` + `0xA6`** — the PS3 NV4097 texture-format
    ///   byte stored in the full `Texture` struct (`0x5200`) inside
    ///   `textures.dat`. Used by **Ratchet & Clank: Full Frontal Assault**
    ///   (and likely other later RCF-engine titles). Mirrored from IT's
    ///   `classes/shader.hpp::TextureFormat`.
    ///
    /// **Do not collapse these into a single mapping.** The two ranges
    /// don't overlap (low ≤ 0x0A, high ≥ 0x81), so a single `match` over
    /// both is safe and keeps R2 + FFA functional simultaneously without a
    /// runtime "which game is this" branch. Unknown bytes return
    /// `Unknown(b)` so the caller can log them and we can map any
    /// remaining variants on the next pass.
    fn from_byte(b: u8) -> Self {
        match b {
            // ── Low range — Resistance 2 / older path via assetlookup.dat ──
            0x03 => TexFormat::R5G6B5,
            0x04 => TexFormat::Rgb5A1,
            0x05 => TexFormat::A8R8G8B8,
            0x06 => TexFormat::Dxt1,
            0x07 => TexFormat::Dxt3,
            0x08 => TexFormat::Dxt5,
            0x09 => TexFormat::R8,
            0x0A => TexFormat::Rg8,
            0x0B => TexFormat::Bc1Linear,

            // ── High range — R&C Full Frontal Assault (NV4097, IT's enum) ──
            0x81 => TexFormat::R8,
            0x82 => TexFormat::Rgb5A1,
            0x83 => TexFormat::Rgba4,
            0x84 => TexFormat::R5G6B5,
            0x85 => TexFormat::A8R8G8B8,
            0x86 => TexFormat::Dxt1,
            0x87 => TexFormat::Dxt3,
            0x88 => TexFormat::Dxt5,
            0x8B => TexFormat::Rg8,
            // Linear-tiled compressed block variants — symmetric to the
            // Morton-tiled 0x86/0x87/0x88 trio. 0xA6 (BC1_LN) is documented
            // in IT; 0xA7 (BC2_LN) and 0xA8 (BC3_LN) are the obvious
            // siblings, observed in R2's `packed/game/global_*` PSARC
            // texture headers where weapon / shared-character textures are
            // stored. We decode all three with `texpresso` using the same
            // Bc1/Bc2/Bc3 codecs — the linear vs swizzled difference is in
            // texel addressing, not block content, and our `decode_dxt`
            // path already treats the input as a linear block stream.
            0xA6 => TexFormat::Bc1Linear,
            0xA7 => TexFormat::Dxt3,
            0xA8 => TexFormat::Dxt5,

            other => TexFormat::Unknown(other),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TexFormat::R5G6B5 => "R5G6B5",
            TexFormat::A8R8G8B8 => "A8R8G8B8",
            TexFormat::Dxt1 => "DXT1",
            TexFormat::Dxt3 => "DXT3",
            TexFormat::Dxt5 => "DXT5",
            TexFormat::R8 => "R8",
            TexFormat::Rg8 => "RG8",
            TexFormat::Rgb5A1 => "RGB5A1",
            TexFormat::Rgba4 => "RGBA4",
            TexFormat::Bc1Linear => "BC1_LN",
            TexFormat::Unknown(_) => "unknown",
        }
    }

    /// Public mapping wrapper so call sites outside this module
    /// (e.g. `texture_old.rs`) don't have to duplicate the byte table.
    pub fn from_format_byte(b: u8) -> Self {
        Self::from_byte(b)
    }
}

/// Centralized format-to-decoder dispatch. Returns RGBA8 bytes.
///
/// Used by both the `assetlookup`-driven V2 path
/// (`bulk_extract_pngs`) and the TOD path (`texture_old.rs`) so the
/// same format → decoder routing lives in one place.
pub fn decode_format(raw: &[u8], width: u32, height: u32, format: TexFormat) -> Vec<u8> {
    match format {
        TexFormat::Dxt1 => decode_dxt(raw, width, height, texpresso::Format::Bc1),
        TexFormat::Dxt3 => decode_dxt(raw, width, height, texpresso::Format::Bc2),
        TexFormat::Dxt5 => decode_dxt(raw, width, height, texpresso::Format::Bc3),
        TexFormat::Bc1Linear => decode_dxt(raw, width, height, texpresso::Format::Bc1),
        TexFormat::R5G6B5 => decode_r5g6b5_morton(raw, width, height),
        TexFormat::A8R8G8B8 => decode_a8r8g8b8_morton(raw, width, height),
        TexFormat::R8 => decode_r8_morton(raw, width, height),
        TexFormat::Rg8 => decode_rg8_morton(raw, width, height),
        TexFormat::Rgb5A1 => decode_rgb5a1_morton(raw, width, height),
        TexFormat::Rgba4 => decode_rgba4_morton(raw, width, height),
        TexFormat::Unknown(b) => {
            eprintln!("warn: decode_format byte 0x{b:02X} not supported");
            Vec::new()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Texture {

    pub id: u32,
    pub tuid: u64,
    pub width: u32,
    pub height: u32,
    pub format: TexFormat,
    pub mipmap_count: u8,

    pub rgba: Vec<u8>,
}

impl Texture {
    pub fn is_decoded(&self) -> bool {
        self.width > 0 && self.height > 0 && !self.rgba.is_empty()
    }
}

pub fn read_textures(level_folder: &Path) -> Result<Vec<Texture>> {
    let mut out = Vec::new();
    read_textures_streaming(level_folder, |_| true, |t| out.push(t))?;
    Ok(out)
}

pub fn read_textures_streaming<A, F>(
    level_folder: &Path,
    accept: A,
    mut on_each: F,
) -> Result<()>
where
    A: Fn(u32) -> bool,
    F: FnMut(Texture),
{
    read_textures_with_total(level_folder, accept, |_| {}, |t| on_each(t))
}

pub fn read_textures_with_total<A, T, F>(
    level_folder: &Path,
    accept: A,
    mut on_total: T,
    mut on_each: F,
) -> Result<()>
where
    A: Fn(u32) -> bool,
    T: FnMut(usize),
    F: FnMut(Texture),
{
    let assetlookup_path = level_folder.join("assetlookup.dat");
    let mut lookup = IgFile::open(BufReader::new(File::open(&assetlookup_path)?))?;

    let meta_section = lookup.require_section(SECT_TEX_META)?;
    let ptr_section = lookup.require_section(SECT_HIGHMIP_PTRS)?;

    let count = (ptr_section.length / ASSET_POINTER_SIZE as u32) as usize;
    if (meta_section.length / NEW_TEX_META_SIZE as u32) as usize != count {
        return Err(Error::SectionLengthMismatch {
            id: SECT_TEX_META,
            length: meta_section.length,
            entry: NEW_TEX_META_SIZE as u32,
        });
    }

    let mut metas: Vec<(TexFormat, u8, u32, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        lookup
            .stream
            .seek_to(u64::from(meta_section.offset) + (i as u64) * NEW_TEX_META_SIZE)?;
        let format_byte = lookup.stream.read_u8()?;
        let mip_count = lookup.stream.read_u8()?;
        let w_pow = lookup.stream.read_u8()?;
        let h_pow = lookup.stream.read_u8()?;
        let format = TexFormat::from_byte(format_byte);
        let width = 1u32 << w_pow;
        let height = 1u32 << h_pow;
        metas.push((format, mip_count, width, height));
    }

    let mut pointers: Vec<(u64, u32, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        lookup
            .stream
            .seek_to(u64::from(ptr_section.offset) + (i as u64) * ASSET_POINTER_SIZE)?;
        let tuid = lookup.stream.read_u64()?;
        let offset = lookup.stream.read_u32()?;
        let length = lookup.stream.read_u32()?;
        pointers.push((tuid, offset, length));
    }

    let accepted_count = pointers.iter().filter(|(t, _, _)| accept(*t as u32)).count();
    on_total(accepted_count);
    if accepted_count == 0 {
        return Ok(());
    }

    let highmips_path = level_folder.join("highmips.dat");
    let mut highmips = File::open(&highmips_path)?;

    for i in 0..count {
        let (format, mip_count, width, height) = metas[i];
        let (tuid, offset, length) = pointers[i];
        let id = tuid as u32;

        if !accept(id) {
            continue;
        }

        if length == 0 {
            on_each(Texture {
                id,
                tuid,
                width: 0,
                height: 0,
                format,
                mipmap_count: mip_count,
                rgba: Vec::new(),
            });
            continue;
        }

        highmips.seek(SeekFrom::Start(u64::from(offset)))?;
        let mut raw = vec![0u8; length as usize];
        highmips.read_exact(&mut raw)?;

        let rgba = match format {
            TexFormat::Dxt1 => decode_dxt(&raw, width, height, texpresso::Format::Bc1),
            TexFormat::Dxt3 => decode_dxt(&raw, width, height, texpresso::Format::Bc2),
            TexFormat::Dxt5 => decode_dxt(&raw, width, height, texpresso::Format::Bc3),
            TexFormat::Bc1Linear => decode_dxt(&raw, width, height, texpresso::Format::Bc1),
            TexFormat::R5G6B5 => decode_r5g6b5_morton(&raw, width, height),
            TexFormat::A8R8G8B8 => decode_a8r8g8b8_morton(&raw, width, height),
            TexFormat::R8 => decode_r8_morton(&raw, width, height),
            TexFormat::Rg8 => decode_rg8_morton(&raw, width, height),
            TexFormat::Rgb5A1 => decode_rgb5a1_morton(&raw, width, height),
            TexFormat::Rgba4 => decode_rgba4_morton(&raw, width, height),
            TexFormat::Unknown(b) => {
                let mut guard = UNKNOWN_FORMAT_BYTES.lock().unwrap();
                let map = guard.get_or_insert_with(std::collections::HashMap::new);
                let count = map.entry(b).or_insert(0);
                *count += 1;
                if *count <= 3 {
                    eprintln!(
                        "warn: texture id={id} (0x{tuid:016X}) format byte 0x{b:02X} not supported \
                         (w={width} h={height} len={length}) — emitting empty PNG (count {})",
                        count
                    );
                }
                Vec::new()
            }
        };

        let (w, h) = if rgba.is_empty() { (0, 0) } else { (width, height) };

        on_each(Texture {
            id,
            tuid,
            width: w,
            height: h,
            format,
            mipmap_count: mip_count,
            rgba,
        });
    }

    log_unknown_format_summary();
    Ok(())
}

static UNKNOWN_FORMAT_BYTES: std::sync::Mutex<Option<std::collections::HashMap<u8, usize>>> =
    std::sync::Mutex::new(None);

fn log_unknown_format_summary() {
    let mut guard = UNKNOWN_FORMAT_BYTES.lock().unwrap();
    if let Some(map) = guard.as_mut() {
        if !map.is_empty() {
            let mut entries: Vec<(u8, usize)> = map.iter().map(|(b, c)| (*b, *c)).collect();
            entries.sort_unstable_by(|a, b| b.1.cmp(&a.1));
            let summary: Vec<String> = entries
                .iter()
                .map(|(b, c)| format!("0x{:02X}×{}", b, c))
                .collect();
            eprintln!(
                "[tex-fmt] unknown format bytes seen this run: {}",
                summary.join(" ")
            );
            map.clear();
        }
    }
}

fn decode_dxt(raw: &[u8], width: u32, height: u32, format: texpresso::Format) -> Vec<u8> {
    let expected = format.compressed_size(width as usize, height as usize);
    if raw.len() < expected {
        return Vec::new();
    }
    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    format.decompress(&raw[..expected], width as usize, height as usize, &mut rgba);
    rgba
}

fn morton_index(t: u32, x: u32, y: u32) -> u32 {
    let mut num = 1u32;
    let mut num2 = 1u32;
    let mut num3 = t;
    let mut num4 = x;
    let mut num5 = y;
    let mut num6 = 0u32;
    let mut num7 = 0u32;
    while num4 > 1 || num5 > 1 {
        if num4 > 1 {
            num6 += num2 * (num3 & 1);
            num3 >>= 1;
            num2 *= 2;
            num4 >>= 1;
        }
        if num5 > 1 {
            num7 += num * (num3 & 1);
            num3 >>= 1;
            num *= 2;
            num5 >>= 1;
        }
    }
    num7 * x + num6
}

fn decode_a8r8g8b8_morton(raw: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pixels = (width as usize) * (height as usize);
    if raw.len() < pixels * 4 {
        return Vec::new();
    }
    let mut rgba = vec![0u8; pixels * 4];
    for t in 0..(pixels as u32) {
        let src = (t as usize) * 4;
        let dst = (morton_index(t, width, height) as usize) * 4;

        let a = raw[src + 0];
        let b = raw[src + 1];
        let g = raw[src + 2];
        let r = raw[src + 3];
        rgba[dst + 0] = r;
        rgba[dst + 1] = g;
        rgba[dst + 2] = b;
        rgba[dst + 3] = a;
    }
    rgba
}

fn decode_r5g6b5_morton(raw: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pixels = (width as usize) * (height as usize);
    if raw.len() < pixels * 2 {
        return Vec::new();
    }
    let mut rgba = vec![0u8; pixels * 4];
    for t in 0..(pixels as u32) {
        let src = (t as usize) * 2;
        let dst = (morton_index(t, width, height) as usize) * 4;

        let v = u16::from_be_bytes([raw[src], raw[src + 1]]);
        let r5 = ((v >> 11) & 0x1F) as u8;
        let g6 = ((v >> 5) & 0x3F) as u8;
        let b5 = (v & 0x1F) as u8;
        rgba[dst + 0] = (r5 << 3) | (r5 >> 2);
        rgba[dst + 1] = (g6 << 2) | (g6 >> 4);
        rgba[dst + 2] = (b5 << 3) | (b5 >> 2);
        rgba[dst + 3] = 0xFF;
    }
    rgba
}

fn decode_r8_morton(raw: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pixels = (width as usize) * (height as usize);
    if raw.len() < pixels {
        return Vec::new();
    }
    let mut rgba = vec![0u8; pixels * 4];
    for t in 0..(pixels as u32) {
        let src = t as usize;
        let dst = (morton_index(t, width, height) as usize) * 4;
        let r = raw[src];
        rgba[dst] = r;
        rgba[dst + 1] = r;
        rgba[dst + 2] = r;
        rgba[dst + 3] = 0xFF;
    }
    rgba
}

fn decode_rg8_morton(raw: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pixels = (width as usize) * (height as usize);
    if raw.len() < pixels * 2 {
        return Vec::new();
    }
    let mut rgba = vec![0u8; pixels * 4];
    for t in 0..(pixels as u32) {
        let src = (t as usize) * 2;
        let dst = (morton_index(t, width, height) as usize) * 4;
        rgba[dst] = raw[src];
        rgba[dst + 1] = raw[src + 1];
        rgba[dst + 2] = 0;
        rgba[dst + 3] = 0xFF;
    }
    rgba
}

fn decode_rgb5a1_morton(raw: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pixels = (width as usize) * (height as usize);
    if raw.len() < pixels * 2 {
        return Vec::new();
    }
    let mut rgba = vec![0u8; pixels * 4];
    for t in 0..(pixels as u32) {
        let src = (t as usize) * 2;
        let dst = (morton_index(t, width, height) as usize) * 4;
        let v = u16::from_be_bytes([raw[src], raw[src + 1]]);
        let r5 = ((v >> 11) & 0x1F) as u8;
        let g5 = ((v >> 6) & 0x1F) as u8;
        let b5 = ((v >> 1) & 0x1F) as u8;
        let a1 = (v & 0x01) as u8;
        rgba[dst] = (r5 << 3) | (r5 >> 2);
        rgba[dst + 1] = (g5 << 3) | (g5 >> 2);
        rgba[dst + 2] = (b5 << 3) | (b5 >> 2);
        rgba[dst + 3] = if a1 != 0 { 0xFF } else { 0x00 };
    }
    rgba
}

fn decode_rgba4_morton(raw: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pixels = (width as usize) * (height as usize);
    if raw.len() < pixels * 2 {
        return Vec::new();
    }
    let mut rgba = vec![0u8; pixels * 4];
    for t in 0..(pixels as u32) {
        let src = (t as usize) * 2;
        let dst = (morton_index(t, width, height) as usize) * 4;
        let v = u16::from_be_bytes([raw[src], raw[src + 1]]);
        let r4 = ((v >> 12) & 0xF) as u8;
        let g4 = ((v >> 8) & 0xF) as u8;
        let b4 = ((v >> 4) & 0xF) as u8;
        let a4 = (v & 0xF) as u8;
        rgba[dst] = (r4 << 4) | r4;
        rgba[dst + 1] = (g4 << 4) | g4;
        rgba[dst + 2] = (b4 << 4) | b4;
        rgba[dst + 3] = (a4 << 4) | a4;
    }
    rgba
}

pub fn decode_image_file_to_png(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    let img = image::open(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let mut out = Vec::with_capacity(64 * 1024);
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(out)
}

pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    if rgba.is_empty() || width == 0 || height == 0 {
        return Vec::new();
    }
    let mut out = Cursor::new(Vec::new());
    let img = match image::RgbaImage::from_raw(width, height, rgba.to_vec()) {
        Some(i) => i,
        None => return Vec::new(),
    };
    if img
        .write_to(&mut out, image::ImageFormat::Png)
        .is_err()
    {
        return Vec::new();
    }
    out.into_inner()
}

pub fn downsample_png_to(png_bytes: &[u8], max_dim: u32) -> Option<Vec<u8>> {
    if max_dim == 0 || max_dim == u32::MAX {
        return None;
    }
    use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, ImageReader};
    use std::io::Cursor;
    let img = ImageReader::with_format(Cursor::new(png_bytes), image::ImageFormat::Png)
        .decode()
        .ok()?;
    let (w, h) = (img.width(), img.height());
    if w <= max_dim && h <= max_dim {
        return None;
    }
    let rgba = img.to_rgba8();
    let new_w = if w >= h {
        max_dim.max(1)
    } else {
        ((w as u64 * max_dim as u64) / h as u64).max(1) as u32
    };
    let new_h = if w >= h {
        ((h as u64 * max_dim as u64) / w as u64).max(1) as u32
    } else {
        max_dim.max(1)
    };
    let resized = image::imageops::resize(
        &rgba,
        new_w,
        new_h,
        image::imageops::FilterType::Triangle,
    );
    let (rw, rh) = (resized.width(), resized.height());
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(resized.as_raw(), rw, rh, ColorType::Rgba8.into())
        .ok()?;
    Some(out)
}

pub fn downsample_rgba(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    max_dim: u32,
) -> (Vec<u8>, u32, u32) {
    if width <= max_dim && height <= max_dim {
        return (rgba, width, height);
    }
    let img = match image::RgbaImage::from_raw(width, height, rgba) {
        Some(i) => i,
        None => return (Vec::new(), 0, 0),
    };
    let (new_w, new_h) = if width >= height {
        (max_dim, ((height as u64 * max_dim as u64) / width as u64) as u32)
    } else {
        (((width as u64 * max_dim as u64) / height as u64) as u32, max_dim)
    };
    let resized = image::imageops::resize(
        &img,
        new_w.max(1),
        new_h.max(1),
        image::imageops::FilterType::Triangle,
    );
    let (w, h) = (resized.width(), resized.height());
    (resized.into_raw(), w, h)
}

pub fn bulk_extract_pngs(
    level_folder: &Path,
    wanted_ids: Option<&[u32]>,
    max_dim: u32,
) -> Result<Vec<(u32, Vec<u8>)>> {
    let assetlookup_path = level_folder.join("assetlookup.dat");
    let mut lookup = IgFile::open(BufReader::new(File::open(&assetlookup_path)?))?;

    let meta_section = lookup.require_section(SECT_TEX_META)?;
    let ptr_section = lookup.require_section(SECT_HIGHMIP_PTRS)?;

    let count = (ptr_section.length / ASSET_POINTER_SIZE as u32) as usize;
    if (meta_section.length / NEW_TEX_META_SIZE as u32) as usize != count {
        return Err(Error::SectionLengthMismatch {
            id: SECT_TEX_META,
            length: meta_section.length,
            entry: NEW_TEX_META_SIZE as u32,
        });
    }

    let mut metas: Vec<(TexFormat, u8, u32, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        lookup
            .stream
            .seek_to(u64::from(meta_section.offset) + (i as u64) * NEW_TEX_META_SIZE)?;
        let format_byte = lookup.stream.read_u8()?;
        let mip_count = lookup.stream.read_u8()?;
        let w_pow = lookup.stream.read_u8()?;
        let h_pow = lookup.stream.read_u8()?;
        let format = TexFormat::from_byte(format_byte);
        let width = 1u32 << w_pow;
        let height = 1u32 << h_pow;
        metas.push((format, mip_count, width, height));
    }

    let mut pointers: Vec<(u64, u32, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        lookup
            .stream
            .seek_to(u64::from(ptr_section.offset) + (i as u64) * ASSET_POINTER_SIZE)?;
        let tuid = lookup.stream.read_u64()?;
        let offset = lookup.stream.read_u32()?;
        let length = lookup.stream.read_u32()?;
        pointers.push((tuid, offset, length));
    }

    let mut base_by_tuid: HashMap<u64, (u32, u32)> = HashMap::new();
    if let Some(base_section) = lookup.section(0x1D180) {
        let base_count = (base_section.length / ASSET_POINTER_SIZE as u32) as usize;
        eprintln!(
            "[tex-fallback] 0x1D180 base-mip table: offset=0x{:X} length={} count={}",
            base_section.offset, base_section.length, base_count
        );
        for i in 0..base_count {
            lookup
                .stream
                .seek_to(u64::from(base_section.offset) + (i as u64) * ASSET_POINTER_SIZE)?;
            let tuid = lookup.stream.read_u64()?;
            let offset = lookup.stream.read_u32()?;
            let length = lookup.stream.read_u32()?;
            base_by_tuid.insert(tuid, (offset, length));
            if i < 3 {
                eprintln!(
                    "[tex-fallback]   base[{}] tuid=0x{:016X} offset=0x{:X} length={}",
                    i, tuid, offset, length
                );
            }
        }
    } else {
        eprintln!("[tex-fallback] 0x1D180 base-mip table: absent");
    }

    let want_set: Option<HashSet<u32>> =
        wanted_ids.map(|ids| ids.iter().copied().collect());
    let want = |id: u32| -> bool {
        match &want_set {
            Some(set) => set.contains(&id),
            None => true,
        }
    };

    let highmips_path = level_folder.join("highmips.dat");
    let mut highmips: Option<File> = match File::open(&highmips_path) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!(
                "[tex-fallback] highmips.dat absent ({}): routing every texture through textures.dat",
                e
            );
            None
        }
    };
    let textures_path = level_folder.join("textures.dat");
    let mut textures_dat: Option<File> = File::open(&textures_path).ok();
    if highmips.is_none() && textures_dat.is_none() {
        eprintln!("[tex-fallback] neither highmips.dat nor textures.dat is present — no textures will be loaded");
        return Ok(Vec::new());
    }

    struct Job {
        id: u32,
        format: TexFormat,
        width: u32,
        height: u32,
        raw: Vec<u8>,
    }
    let mut jobs: Vec<Job> = Vec::new();
    let mut requested = 0usize;
    let mut zero_length = 0usize;
    let mut fallback_textures_dat = 0usize;
    let still_missing;
    let mut highmip_tuids: HashSet<u64> = HashSet::with_capacity(count);
    for i in 0..count {
        let (format, _mip_count, width, height) = metas[i];
        let (tuid, offset, length) = pointers[i];
        let id = tuid as u32;
        highmip_tuids.insert(tuid);
        if !want(id) {
            continue;
        }
        requested += 1;
        let have_highmip = length > 0 && highmips.is_some();
        let (raw, eff_w, eff_h): (Vec<u8>, u32, u32) = if have_highmip {
            let f = highmips.as_mut().expect("checked Some above");
            f.seek(SeekFrom::Start(u64::from(offset)))?;
            let mut buf = vec![0u8; length as usize];
            f.read_exact(&mut buf)?;
            (buf, width, height)
        } else {
            if length == 0 {
                zero_length += 1;
            }
            match (textures_dat.as_mut(), base_by_tuid.get(&tuid)) {
                (Some(f), Some(&(b_off, b_len))) if b_len > 0 => {
                    f.seek(SeekFrom::Start(u64::from(b_off)))?;
                    let mut buf = vec![0u8; b_len as usize];
                    f.read_exact(&mut buf)?;
                    fallback_textures_dat += 1;
                    let half_w = (width / 2).max(1);
                    let half_h = (height / 2).max(1);
                    (buf, half_w, half_h)
                }
                _ => continue,
            }
        };
        jobs.push(Job {
            id,
            format,
            width: eff_w,
            height: eff_h,
            raw,
        });
    }

    let mut cubemap_ids: HashSet<u32> = HashSet::new();
    let mut cubemap_tuids: Vec<u64> = Vec::new();
    if let Some(cube_section) = lookup.section(0x1D200) {
        let cube_count = (cube_section.length / ASSET_POINTER_SIZE as u32) as usize;
        for i in 0..cube_count {
            lookup
                .stream
                .seek_to(u64::from(cube_section.offset) + (i as u64) * ASSET_POINTER_SIZE)?;
            let tuid = lookup.stream.read_u64()?;
            let _offset = lookup.stream.read_u32()?;
            let _length = lookup.stream.read_u32()?;
            cubemap_ids.insert(tuid as u32);
            cubemap_tuids.push(tuid);
        }
    }
    let mut all_table_tuids: Vec<u64> = Vec::with_capacity(
        pointers.len() + base_by_tuid.len() + cubemap_tuids.len(),
    );
    for (tuid, _, _) in &pointers {
        all_table_tuids.push(*tuid);
    }
    for tuid in base_by_tuid.keys() {
        all_table_tuids.push(*tuid);
    }
    all_table_tuids.extend(&cubemap_tuids);

    let mut needed_only_in_base = 0usize;
    let mut needed_in_cubemap = 0usize;
    let mut needed_in_neither = 0usize;
    let mut sample_missing: Vec<u32> = Vec::new();
    if let Some(set) = &want_set {
        let highmip_ids_u32: HashSet<u32> =
            highmip_tuids.iter().map(|t| *t as u32).collect();
        let base_ids_u32: HashSet<u32> =
            base_by_tuid.keys().map(|t| *t as u32).collect();
        for &id in set {
            if highmip_ids_u32.contains(&id) {
                continue;
            }
            if base_ids_u32.contains(&id) {
                needed_only_in_base += 1;
            } else if cubemap_ids.contains(&id) {
                needed_in_cubemap += 1;
            } else {
                needed_in_neither += 1;
                if sample_missing.len() < 8 {
                    sample_missing.push(id);
                }
            }
        }
    }
    if !sample_missing.is_empty() {
        let ids_hex: Vec<String> = sample_missing.iter().map(|id| format!("0x{:08X}", id)).collect();
        eprintln!(
            "[tex-fallback] first {} truly-missing texture IDs (not in 0x1D180/0x1D1C0/0x1D200): {}",
            sample_missing.len(),
            ids_hex.join(", ")
        );
        for &missing_id in &sample_missing {
            let hits: Vec<u64> = all_table_tuids
                .iter()
                .copied()
                .filter(|t| (*t as u32) == missing_id)
                .collect();
            if hits.is_empty() {
                eprintln!(
                    "[tex-fallback]   0x{:08X}: NO match in any texture-table tuid (bottom-32 check)",
                    missing_id
                );
            } else {
                let labels: Vec<String> = hits
                    .iter()
                    .map(|t| format!("0x{:016X}", t))
                    .collect();
                eprintln!(
                    "[tex-fallback]   0x{:08X}: matches {} table tuid(s): {}",
                    missing_id,
                    hits.len(),
                    labels.join(", ")
                );
            }
        }
    }
    still_missing = needed_only_in_base + needed_in_cubemap + needed_in_neither;

    let drop_decode = std::sync::atomic::AtomicUsize::new(0);
    let drop_downsample = std::sync::atomic::AtomicUsize::new(0);
    let drop_encode = std::sync::atomic::AtomicUsize::new(0);

    let pngs: Vec<(u32, Vec<u8>)> = jobs
        .into_par_iter()
        .filter_map(|job| {
            let rgba = match job.format {
                TexFormat::Dxt1 => {
                    decode_dxt(&job.raw, job.width, job.height, texpresso::Format::Bc1)
                }
                TexFormat::Dxt3 => {
                    decode_dxt(&job.raw, job.width, job.height, texpresso::Format::Bc2)
                }
                TexFormat::Dxt5 => {
                    decode_dxt(&job.raw, job.width, job.height, texpresso::Format::Bc3)
                }
                TexFormat::Bc1Linear => {
                    decode_dxt(&job.raw, job.width, job.height, texpresso::Format::Bc1)
                }
                TexFormat::R5G6B5 => decode_r5g6b5_morton(&job.raw, job.width, job.height),
                TexFormat::A8R8G8B8 => {
                    decode_a8r8g8b8_morton(&job.raw, job.width, job.height)
                }
                TexFormat::R8 => decode_r8_morton(&job.raw, job.width, job.height),
                TexFormat::Rg8 => decode_rg8_morton(&job.raw, job.width, job.height),
                TexFormat::Rgb5A1 => decode_rgb5a1_morton(&job.raw, job.width, job.height),
                TexFormat::Rgba4 => decode_rgba4_morton(&job.raw, job.width, job.height),
                TexFormat::Unknown(b) => {
                    let mut guard = UNKNOWN_FORMAT_BYTES.lock().unwrap();
                    let map = guard.get_or_insert_with(std::collections::HashMap::new);
                    let count = map.entry(b).or_insert(0);
                    *count += 1;
                    if *count <= 3 {
                        eprintln!(
                            "warn: texture id={} format byte 0x{b:02X} not supported \
                             (w={} h={} len={}) — emitting empty PNG (count {})",
                            job.id,
                            job.width,
                            job.height,
                            job.raw.len(),
                            count
                        );
                    }
                    Vec::new()
                }
            };
            if rgba.is_empty() {
                drop_decode.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                static SAMPLES: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                let n = SAMPLES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n < 5 {
                    let fmt_label = match job.format {
                        TexFormat::Dxt1 => "DXT1",
                        TexFormat::Dxt3 => "DXT3",
                        TexFormat::Dxt5 => "DXT5",
                        TexFormat::Bc1Linear => "BC1_LN",
                        TexFormat::R5G6B5 => "R5G6B5",
                        TexFormat::A8R8G8B8 => "A8R8G8B8",
                        TexFormat::R8 => "R8",
                        TexFormat::Rg8 => "RG8",
                        TexFormat::Rgb5A1 => "RGB5A1",
                        TexFormat::Rgba4 => "RGBA4",
                        TexFormat::Unknown(_) => "Unknown",
                    };
                    eprintln!(
                        "[tex-drop] id=0x{:08X} fmt={} declared w={} h={} raw_len={} — decode produced empty rgba",
                        job.id, fmt_label, job.width, job.height, job.raw.len()
                    );
                }
                return None;
            }
            let (rgba, w, h) = downsample_rgba(rgba, job.width, job.height, max_dim);
            if rgba.is_empty() {
                drop_downsample.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return None;
            }
            let png = encode_png(&rgba, w, h);
            if png.is_empty() {
                drop_encode.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return None;
            }
            Some((job.id, png))
        })
        .collect();
    let drops_decode = drop_decode.load(std::sync::atomic::Ordering::Relaxed);
    let drops_downsample = drop_downsample.load(std::sync::atomic::Ordering::Relaxed);
    let drops_encode = drop_encode.load(std::sync::atomic::Ordering::Relaxed);
    if zero_length + drops_decode + drops_downsample + drops_encode + still_missing > 0 {
        eprintln!(
            "[tex-drop] requested={} encoded={} drops: zero_length={} \
             recovered_from_textures_dat={} \
             needed_only_in_base_table={} needed_in_cubemap_table={} needed_in_neither_table={} \
             decode_empty={} downsample_empty={} encode_empty={}",
            requested,
            pngs.len(),
            zero_length,
            fallback_textures_dat,
            needed_only_in_base,
            needed_in_cubemap,
            needed_in_neither,
            drops_decode,
            drops_downsample,
            drops_encode
        );
    }
    log_unknown_format_summary();
    Ok(pngs)
}
