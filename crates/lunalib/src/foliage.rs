use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::assetlookup::{AssetKind, AssetLookup};
use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::moby::{half_to_f32, read_shader_table};
use crate::tie::{TieAsset, TieMeshGeom};

const SECT_FOLIAGE_HEADER: u32 = 0xA200;
const SECT_FOLIAGE_BUFFER: u32 = 0xA000;
const SECT_FOLIAGE_SHADER_TABLE: u32 = 0x5600;

const SPRITE_VERTEX_SIZE: u64 = 8;
const FOLIAGE_VERTEX_SIZE: u64 = 14;

/// V2 foliage assets (`foliages.dat`, IT's `FoliageToGltf`,
/// extract_gltf.cpp:631-716): one 0xA200 FoliageV2 header per asset plus a
/// 0xA000 raw buffer. The buffer starts with `SpriteV2Vertex` corner records
/// (f16 size.xy + f16 uv.xy, 8 bytes); `FoliageV2Vertex` sprite centers
/// (f16 position.xyz + 4×i16, 14 bytes) start at `spriteVertexOffset`.
/// Four consecutive corners form one billboard quad anchored at
/// `positions[corner / 4]`; corner position = center + (size.x, size.y, 0).
/// IT multiplies by YARD_TO_M — dropped here, same as ufrags, so foliage
/// stays in the raw-yard unit system the rest of the V2 scene uses.
/// Only LOD 0 of `spriteLodRanges[5]` is meshed (IT does the same).
pub fn read_foliage_v2(level_folder: &Path) -> Result<Vec<TieAsset>> {
    let assetlookup_path = level_folder.join("assetlookup.dat");
    let mut lookup = AssetLookup::open(BufReader::new(File::open(&assetlookup_path)?))?;
    let ptrs = lookup.pointers(AssetKind::Foliage)?;
    if ptrs.is_empty() {
        return Ok(Vec::new());
    }

    let data_path = level_folder.join("foliages.dat");
    let mut data_file = File::open(&data_path)?;

    let mut out = Vec::with_capacity(ptrs.len());
    for ptr in ptrs {
        if ptr.length > crate::MAX_ASSET_SIZE {
            return Err(Error::AllocLimitExceeded {
                size: u64::from(ptr.length),
                limit: u64::from(crate::MAX_ASSET_SIZE),
            });
        }
        data_file.seek(SeekFrom::Start(u64::from(ptr.offset)))?;
        let mut buf = vec![0u8; ptr.length as usize];
        data_file.read_exact(&mut buf)?;
        let parsed = (|| -> Result<TieAsset> {
            let mut ig = IgFile::open(Cursor::new(buf))?;
            parse_foliage(&mut ig, ptr.tuid)
        })();
        match parsed {
            Ok(asset) => out.push(asset),
            Err(e) => eprintln!("warn: foliage 0x{:016X} skipped: {e}", ptr.tuid),
        }
    }
    Ok(out)
}

fn parse_foliage<R: Read + Seek>(ig: &mut IgFile<R>, tuid: u64) -> Result<TieAsset> {
    let header = ig.require_section(SECT_FOLIAGE_HEADER)?;
    let buffer = ig.require_section(SECT_FOLIAGE_BUFFER)?;
    let shader_tuids = read_shader_table(ig, SECT_FOLIAGE_SHADER_TABLE)?;

    let header_off = u64::from(header.offset);
    // Two 0xA200 header layouts, discriminated by the section entry length
    // (same trick as the 0x7440 instance records). 0xC0 is IT's R2-era
    // FoliageV2: spriteVertexOffset at +0x58, spriteLodRanges[5] at +0x60.
    // 0x80 is the R3 header, RE'd from mine_town_approach foliage
    // 0x00096685D792B4B8: spriteVertexOffset at +0x50, corner count at
    // +0x58 (begin 0). Self-validating: 0x1A0/8 = 52 corners = 13 quads,
    // and the buffer remainder 182 bytes = exactly 13 x 14-byte centers.
    let (sprite_vertex_offset, corner_begin, corner_end) = if u64::from(header.length) >= 0xC0 {
        ig.stream.seek_to(header_off + 0x58)?;
        let svo = u64::from(ig.stream.read_u32()?);
        ig.stream.seek_to(header_off + 0x60)?;
        let begin = u64::from(ig.stream.read_u16()?);
        let end = u64::from(ig.stream.read_u16()?);
        (svo, begin, end)
    } else {
        ig.stream.seek_to(header_off + 0x50)?;
        let svo = u64::from(ig.stream.read_u32()?);
        ig.stream.seek_to(header_off + 0x58)?;
        let end = u64::from(ig.stream.read_u32()?);
        (svo, 0, end)
    };

    let buffer_off = u64::from(buffer.offset);
    let buffer_len = u64::from(buffer.length);
    let corner_capacity = sprite_vertex_offset.min(buffer_len) / SPRITE_VERTEX_SIZE;
    let center_capacity = buffer_len.saturating_sub(sprite_vertex_offset) / FOLIAGE_VERTEX_SIZE;
    if corner_end <= corner_begin
        || corner_end > corner_capacity
        || corner_end.div_ceil(4) > center_capacity
    {
        return Err(Error::IndexOutOfBounds {
            id: SECT_FOLIAGE_BUFFER,
            index: corner_end,
            max: corner_capacity,
        });
    }

    let corner_count = (corner_end - corner_begin) as usize;
    let mut positions = Vec::with_capacity(corner_count * 3);
    let mut uvs = Vec::with_capacity(corner_count * 2);
    for vt in corner_begin..corner_end {
        ig.stream.seek_to(buffer_off + vt * SPRITE_VERTEX_SIZE)?;
        let size_x = half_to_f32(ig.stream.read_u16()?);
        let size_y = half_to_f32(ig.stream.read_u16()?);
        let u = half_to_f32(ig.stream.read_u16()?);
        let v = half_to_f32(ig.stream.read_u16()?);

        let center_index = vt / 4;
        ig.stream
            .seek_to(buffer_off + sprite_vertex_offset + center_index * FOLIAGE_VERTEX_SIZE)?;
        let px = half_to_f32(ig.stream.read_u16()?);
        let py = half_to_f32(ig.stream.read_u16()?);
        let pz = half_to_f32(ig.stream.read_u16()?);

        positions.push(px + size_x);
        positions.push(py + size_y);
        positions.push(pz);
        uvs.push(u);
        uvs.push(v);
    }

    let quad_count = corner_count / 4;
    let mut indices = Vec::with_capacity(quad_count * 6);
    for q in 0..quad_count as u32 {
        indices.push(q * 4);
        indices.push(q * 4 + 1);
        indices.push(q * 4 + 2);
        indices.push(q * 4 + 3);
        indices.push(q * 4);
        indices.push(q * 4 + 2);
    }

    let mesh = TieMeshGeom {
        shader_index: 0,
        vertex_count: corner_count.min(u16::MAX as usize) as u16,
        index_count: indices.len().min(u16::MAX as usize) as u16,
        positions,
        uvs,
        indices,
    };

    Ok(TieAsset {
        tuid,
        scale: [1.0, 1.0, 1.0],
        meshes: vec![mesh],
        shader_tuids,
    })
}
