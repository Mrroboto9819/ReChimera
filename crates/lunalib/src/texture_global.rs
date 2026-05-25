//! Loader for R2's `packed/game/global_cached/` and `global_uncached/`
//! shared-asset PSARC contents. These store textures the engine treats as
//! cross-level art (weapons, shared characters, UI elements) in a different
//! layout than the per-level `assetlookup.dat` + `textures.dat` /
//! `highmips.dat` packed files:
//!
//! ```text
//! packed/game/global_cached/
//!   built/tuids/
//!     <3-hex-prefix>/
//!       <full-64-bit-hex-tuid>/
//!         header.dat      ← IGHW container, one TOC entry (id=0x5200 Texture)
//!         texel.dat       ← IGHW container, two TOC entries
//!                              (id=0x5400 TextureData buffer,
//!                               id=0x5A00 TextureResource hash+totalSize)
//! ```
//!
//! Shaders inside level data reference textures by their lower-32-bit TUID
//! ("texture ID"). When a shader-referenced ID is not present in the
//! level's own texture tables (`0x1D180` / `0x1D1C0` / `0x1D200`), we look
//! it up here by scanning every TUID folder once at extract time and
//! mapping `(tuid & 0xFFFFFFFF) → folder`.
//!
//! IT-reference: the format is the same per-TUID `<tuid>/header.dat` +
//! `<tuid>/texel.dat` pair `extract_textures` writes when dumping
//! `.tph`/`.tp` content for RFOM. The PS3 NV4097 `Texture` struct
//! (`shader.hpp::Texture`, ID=0x5200) is reused unchanged.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::igfile::IgFile;
use crate::texture::{decode_format, downsample_png_to, encode_png, TexFormat};

const SECT_TEXTURE: u32 = 0x5200;
const SECT_TEXTURE_DATA: u32 = 0x5400;
const SECT_TEXTURE_RESOURCE: u32 = 0x5A00;

/// One entry in the global texture index: where the texture lives on disk
/// plus its full 64-bit tuid so callers can disambiguate if two textures
/// happen to share the low 32 bits.
#[derive(Debug, Clone)]
pub struct GlobalTextureEntry {
    pub full_tuid: u64,
    pub folder: PathBuf,
}

/// Search upward from `level_folder` to find sibling
/// `packed/game/global_cached/` and `packed/game/global_uncached/`
/// directories that contain a `built/tuids/` tree. The user's level path is
/// typically deep inside `packed/levels/<name>/...`, so we walk parents
/// looking for a `packed/` ancestor and then probe its `game/global_*/`
/// children.
///
/// Returns each root's `built/tuids/` directory directly, since that's
/// what the index builder wants to walk. Empty vec if nothing matches —
/// the caller should treat that as "no global fallback available," not an
/// error.
pub fn find_global_tuid_roots(level_folder: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cur = Some(level_folder);
    while let Some(p) = cur {
        // Check this directory's name first — if it IS `packed`, look down.
        if p.file_name().map(|n| n == "packed").unwrap_or(false) {
            for variant in ["global_cached", "global_uncached"] {
                let parent = p.join("game").join(variant);
                let tuids = parent.join("built").join("tuids");
                if tuids.is_dir() {
                    out.push(tuids);
                } else if parent.join("built").is_dir() {
                    // Why: parent has a `built/` tree but no `built/tuids/`
                    // inside it — that's a real partial extraction (the
                    // PSARC produced the built/ root but the tuid buckets
                    // are missing or got deleted). Worth flagging because
                    // texture fallback won't recover.
                    //
                    // We deliberately do NOT flag when `parent.is_dir()`
                    // alone is true — in R2 the `global_uncached.psarc`
                    // ships only `packed/<toc>` + `sound/global/` (no
                    // `built/` at all), so its folder existing without
                    // `built/tuids/` is the PSARC's normal layout, not a
                    // missing extraction. Nagging there was a false alarm.
                    let psarc_hint = p
                        .join("game")
                        .join(format!("{variant}.psarc"));
                    if psarc_hint.is_file() {
                        eprintln!(
                            "[global-tex] HINT: {} has a built/ tree but no built/tuids/. \
                             Re-extract {} to repair cross-level texture fallback for shared art \
                             (weapons / characters / UI).",
                            parent.display(),
                            psarc_hint.display(),
                        );
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        cur = p.parent();
    }
    out
}

/// Walk every `<prefix>/<full-tuid>/` folder under each root and build a
/// `lower-32-of-tuid → entry` map. The roots are passed in priority order;
/// later roots OVERWRITE earlier ones on collision (we expect uncached to
/// shadow cached for the same id when both ship it, which is benign — both
/// would decode to identical bytes).
pub fn build_global_texture_index(roots: &[PathBuf]) -> HashMap<u32, GlobalTextureEntry> {
    let mut index: HashMap<u32, GlobalTextureEntry> = HashMap::new();
    for root in roots {
        let bucket_iter = match std::fs::read_dir(root) {
            Ok(it) => it,
            Err(e) => {
                eprintln!("[global-tex] could not read {}: {}", root.display(), e);
                continue;
            }
        };
        for bucket in bucket_iter.flatten() {
            let bucket_path = bucket.path();
            if !bucket_path.is_dir() {
                continue;
            }
            let tuid_iter = match std::fs::read_dir(&bucket_path) {
                Ok(it) => it,
                Err(_) => continue,
            };
            for entry in tuid_iter.flatten() {
                let folder = entry.path();
                if !folder.is_dir() {
                    continue;
                }
                // Folder name is the full 16-hex tuid. Skip non-tuid folders
                // (e.g. the `tuids/00f` bucket itself shouldn't have stray
                // files but be defensive).
                let name = match folder.file_name().and_then(|n| n.to_str()) {
                    Some(n) if n.len() == 16 => n,
                    _ => continue,
                };
                let full_tuid = match u64::from_str_radix(name, 16) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let low = (full_tuid & 0xFFFFFFFF) as u32;
                // Only record if the pair of files actually exists — keeps
                // the index honest so a later open never has to fall back.
                if folder.join("header.dat").is_file() && folder.join("texel.dat").is_file() {
                    index.insert(
                        low,
                        GlobalTextureEntry {
                            full_tuid,
                            folder,
                        },
                    );
                }
            }
        }
    }
    index
}

/// Decode one global texture (mip 0) to a PNG, optionally downscaled to
/// `max_dim`. Returns `Ok(None)` if the format isn't decodable (an unknown
/// `TextureFormat` byte or a zero-sized texel buffer) — that's a soft
/// failure the caller should log and skip, not propagate.
pub fn load_global_texture_png(folder: &Path, max_dim: u32) -> Result<Option<Vec<u8>>> {
    let (format, width, height, offset_in_texel, num_mips) = read_texture_descriptor(folder)?;
    if width == 0 || height == 0 {
        return Ok(None);
    }
    let raw = match read_texel_bytes(folder, offset_in_texel, width, height, format)? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };
    let rgba = decode_format(&raw, u32::from(width), u32::from(height), format);
    if rgba.is_empty() {
        eprintln!(
            "[global-tex] decode produced empty rgba for {} (fmt={} w={} h={} raw_len={} mips={})",
            folder.display(),
            format.name(),
            width,
            height,
            raw.len(),
            num_mips,
        );
        return Ok(None);
    }
    let png = encode_png(&rgba, u32::from(width), u32::from(height));
    if max_dim > 0 && max_dim < u32::MAX {
        Ok(Some(downsample_png_to(&png, max_dim).unwrap_or(png)))
    } else {
        Ok(Some(png))
    }
}

/// Parse the 32-byte NV4097 `Texture` struct out of `header.dat`. Layout
/// matches IT's `shader.hpp::Texture` (`offset @0x00 u32`, `numMips @0x04
/// u16`, `format @0x06 u8`, `flags @0x07 u8`, four 4-byte register
/// bitfields, `filter @0x14 u32`, `width @0x18 u16`, `height @0x1A u16`,
/// `borderColor @0x1C u32`). We only need format / dimensions / texel
/// offset / mip count.
fn read_texture_descriptor(folder: &Path) -> Result<(TexFormat, u16, u16, u32, u16)> {
    let header_path = folder.join("header.dat");
    let file = File::open(&header_path)?;
    let mut ig = IgFile::open(BufReader::new(file))?;
    let section = ig.require_section(SECT_TEXTURE)?;
    let base = u64::from(section.offset);

    ig.stream.seek_to(base + 0x00)?;
    let offset_in_texel = ig.stream.read_u32()?;
    let num_mips = ig.stream.read_u16()?;
    let format_byte = ig.stream.read_u8()?;
    let _flags = ig.stream.read_u8()?;

    // Skip address (u32) + control0 (u32) + control3 (u32) + filter (u32) =
    // 16 bytes of NV4097 register bitfields we don't need for decode.
    ig.stream.seek_to(base + 0x18)?;
    let width = ig.stream.read_u16()?;
    let height = ig.stream.read_u16()?;

    let format = TexFormat::from_format_byte(format_byte);
    Ok((format, width, height, offset_in_texel, num_mips))
}

/// Pull mip 0 raw bytes out of `texel.dat`. The IGHW container holds the
/// texel buffer in section `0x5400` (TextureData) — we slice from
/// `offset_in_texel` for the computed mip-0 byte size. The buffer also
/// contains the full mip chain past mip 0; we don't need those (PNG is one
/// resolution).
fn read_texel_bytes(
    folder: &Path,
    offset_in_texel: u32,
    width: u16,
    height: u16,
    format: TexFormat,
) -> Result<Option<Vec<u8>>> {
    let mip0_size = match compute_mip0_size(width, height, format) {
        Some(n) => n,
        None => return Ok(None),
    };
    let texel_path = folder.join("texel.dat");
    let mut buf = Vec::new();
    File::open(&texel_path)?.read_to_end(&mut buf)?;
    let mut ig = IgFile::open(Cursor::new(buf))?;
    let section = ig.require_section(SECT_TEXTURE_DATA)?;
    if mip0_size > section.length as usize {
        eprintln!(
            "[global-tex] mip0 size {} exceeds TextureData section length {} in {}",
            mip0_size,
            section.length,
            folder.display(),
        );
        return Ok(None);
    }
    ig.stream
        .seek_to(u64::from(section.offset) + u64::from(offset_in_texel))?;
    let bytes = ig.stream.read_bytes(mip0_size)?;
    // Belt-and-braces: if the read returned short (shouldn't, given the
    // length check), bail rather than feed a partial block to the decoder.
    if bytes.len() != mip0_size {
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// Bytes in mip 0 for `(width, height, format)`. Block-compressed formats
/// round up to 4-pixel block boundaries. Returns `None` for an
/// `Unknown` format byte so the caller can skip the texture rather than
/// reading a length we can't justify.
fn compute_mip0_size(width: u16, height: u16, format: TexFormat) -> Option<usize> {
    let w = width as usize;
    let h = height as usize;
    let bw = (w + 3) / 4;
    let bh = (h + 3) / 4;
    let n = match format {
        TexFormat::Dxt1 | TexFormat::Bc1Linear => bw * bh * 8,
        TexFormat::Dxt3 | TexFormat::Dxt5 => bw * bh * 16,
        TexFormat::A8R8G8B8 => w * h * 4,
        TexFormat::R5G6B5 | TexFormat::Rgb5A1 | TexFormat::Rgba4 | TexFormat::Rg8 => {
            w * h * 2
        }
        TexFormat::R8 => w * h,
        TexFormat::Unknown(_) => return None,
    };
    Some(n)
}

/// One-shot: locate global roots from a level folder, build the index,
/// and report stats. Convenience wrapper for cache extraction so the call
/// site doesn't have to thread the two steps. Returns an empty index if no
/// global roots are reachable from this level — that's the common case for
/// games / mods that ship everything per-level.
pub fn discover_and_index(level_folder: &Path) -> HashMap<u32, GlobalTextureEntry> {
    let roots = find_global_tuid_roots(level_folder);
    if roots.is_empty() {
        return HashMap::new();
    }
    let index = build_global_texture_index(&roots);
    eprintln!(
        "[global-tex] indexed {} textures from {} root(s): {}",
        index.len(),
        roots.len(),
        roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
    );
    index
}

/// Find sibling level folders extracted under the same `packed/levels/`
/// root, suitable for cross-level texture fallback. R2 ships shared art
/// (lobby UI, coop/MP overlays, weapon variants) inside individual level
/// PSARCs rather than the globals — meshes in `scotia_coop` legitimately
/// reference texture IDs that only exist in `lobby/level_cached.psarc`
/// (or another extracted level). When those textures aren't in the
/// global TUID tree either, the only way to recover them without
/// teaching the user to dump every PSARC is to read from sibling level
/// folders the user has already extracted.
///
/// Returns the canonical inner level dir for each sibling
/// (`.../packed/levels/<id>/built/levels/<id>/`), which is what
/// `bulk_extract_pngs` expects as its `level_folder` argument. Excludes
/// the caller's own level (compared by canonical path) so we don't
/// recurse on ourselves. Empty vec if no sibling tree is reachable.
pub fn find_sibling_extracted_levels(level_folder: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let self_canon = level_folder.canonicalize().ok();
    let mut cur = Some(level_folder);
    while let Some(p) = cur {
        let is_levels_dir = p
            .file_name()
            .map(|n| n == "levels")
            .unwrap_or(false)
            && p.parent()
                .and_then(|gp| gp.file_name())
                .map(|n| n == "packed")
                .unwrap_or(false);
        if is_levels_dir {
            let it = match std::fs::read_dir(p) {
                Ok(it) => it,
                Err(_) => break,
            };
            for entry in it.flatten() {
                let folder = entry.path();
                if !folder.is_dir() {
                    continue;
                }
                let Some(name) = folder.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let sibling_level_path =
                    folder.join("built").join("levels").join(name);
                if !sibling_level_path.join("assetlookup.dat").is_file() {
                    continue;
                }
                if let Some(ref self_c) = self_canon {
                    if sibling_level_path.canonicalize().ok().as_ref() == Some(self_c) {
                        continue;
                    }
                }
                out.push(sibling_level_path);
            }
            break;
        }
        cur = p.parent();
    }
    out
}

/// Verify the suspected texture in this folder really is the one with id
/// `expected_low_32`. Reads the `TextureResource` (id `0x5A00`) which
/// stores `{ uint32 hash; uint32 totalSize; }` — `hash` should match the
/// folder's tuid lower-32. Returns `true` on match, `false` otherwise.
/// Used by callers that want belt-and-braces collision detection before
/// adopting a global texture.
pub fn verify_global_texture_id(folder: &Path, expected_low_32: u32) -> bool {
    let texel_path = folder.join("texel.dat");
    let buf = match std::fs::read(&texel_path) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let mut ig = match IgFile::open(Cursor::new(buf)) {
        Ok(ig) => ig,
        Err(_) => return false,
    };
    let section = match ig.section(SECT_TEXTURE_RESOURCE) {
        Some(s) => s,
        None => return false,
    };
    if ig.stream.seek_to(u64::from(section.offset)).is_err() {
        return false;
    }
    let hash = match ig.stream.read_u32() {
        Ok(v) => v,
        Err(_) => return false,
    };
    hash == expected_low_32
}
