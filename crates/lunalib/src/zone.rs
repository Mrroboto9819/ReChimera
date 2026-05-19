

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::assetlookup::{AssetKind, AssetLookup};
use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::math::decompose_row_major;
use crate::moby::{half_to_f32, read_shader_table};

const SECT_UFRAG_VERTICES: u32 = 0x6000;
const SECT_UFRAG_INDICES: u32 = 0x6100;
const SECT_UFRAGS: u32 = 0x6200;
const SECT_UFRAG_SHADER_TABLE: u32 = 0x71A0;
const SECT_TIE_TUID_TABLE: u32 = 0x7200;
const SECT_TIE_INSTANCES: u32 = 0x7240;
const SECT_TIE_NAME_POINTERS: u32 = 0x72C0;
const SECT_FOLIAGE_TUID_TABLE: u32 = 0x7400;
const SECT_FOLIAGE_INSTANCES: u32 = 0x7440;
const SECT_SHRUB_TUID_TABLE: u32 = 0x7500;
const SECT_SHRUB_INSTANCES: u32 = 0x7540;

const TIE_INSTANCE_SIZE: u64 = 0x80;
const NAME_POINTER_SIZE: u64 = 0x10;
const SHRUB_INSTANCE_SIZE: u64 = 0x40;
const FOLIAGE_INSTANCE_SIZE: u64 = 0xB0;
const UFRAG_SIZE: u64 = 0x80;
const UFRAG_VERTEX_STRIDE: usize = 0x18;

/// IT's `RegionToGltf` (extract_gltf.cpp:881-884) decodes V2 region
/// vertex positions as `R16G16B16A16_NORM`, then applies
/// `world = normalized * mul + add` where:
///   `mul = (0x7FFF / 0x100) * YARD_TO_M`
///   `add = (item.position / 0x100) * YARD_TO_M`
///
/// Algebraically that simplifies to
/// `world_xyz = (raw_i16 + position_xyz) * YARD_TO_M / 256`.
/// `YARD_TO_M = 0.9144`, divisor = 256.
///
/// This V2 path serves R2 / R3 / RCFFA / A4O. RFOM uses
/// `region_rfom.rs` which intentionally does NOT scale (raw values
/// match the moby/tie placement unit system on that game).
// IT applies `* YARD_TO_M / 256` to ufrag positions
// (extract_gltf.cpp:882-883). We intentionally drop the YARD_TO_M factor
// here so ufrag world coords stay in the same units as moby/tie
// placements (raw yards), which is what the rest of the V2 pipeline +
// camera framing assume. Otherwise terrain ends up ~9% smaller than the
// mobys/ties placed on top of it.
const UFRAG_VERTEX_SCALE: f32 = 1.0 / 256.0;

#[derive(Debug, Clone)]
pub struct Zone {

    pub tuid: u64,
    pub tie_instances: Vec<TieInstance>,
    pub shrub_instances: Vec<TieInstance>,
    pub foliage_instances: Vec<TieInstance>,
    pub ufrags: Vec<UFrag>,

    pub ufrag_shader_tuids: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct TieInstance {

    pub tie_tuid: u64,

    pub instance_tuid: u64,
    pub name: String,
    pub position: [f32; 3],

    pub quaternion: [f32; 4],

    pub scale: [f32; 3],
    pub bounding_radius: f32,
}

#[derive(Debug, Clone)]
pub struct UFrag {
    pub tuid: u64,

    pub position: [f32; 3],
    pub radius: f32,
    pub vertex_count: u16,
    pub index_count: u16,
    pub shader_index: u16,

    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
    pub indices: Vec<u32>,
}

pub fn read_zones(level_folder: &Path) -> Result<Vec<Zone>> {
    let mut out = Vec::new();
    read_zones_streaming(level_folder, |z| out.push(z))?;
    Ok(out)
}

pub fn read_zones_streaming<F>(level_folder: &Path, mut on_each: F) -> Result<()>
where
    F: FnMut(Zone),
{
    read_zones_with_total(level_folder, |_| {}, |z| on_each(z))
}

pub fn read_zones_with_total<T, F>(
    level_folder: &Path,
    mut on_total: T,
    mut on_each: F,
) -> Result<()>
where
    T: FnMut(usize),
    F: FnMut(Zone),
{
    let assetlookup_path = level_folder.join("assetlookup.dat");
    let mut lookup = AssetLookup::open(BufReader::new(File::open(&assetlookup_path)?))?;
    let zone_ptrs = lookup.pointers(AssetKind::Zone)?;

    on_total(zone_ptrs.len());
    if zone_ptrs.is_empty() {
        return Ok(());
    }

    let zones_dat_path = level_folder.join("zones.dat");
    let mut zones_file = File::open(&zones_dat_path)?;

    for ptr in zone_ptrs {
        if ptr.length > crate::MAX_ASSET_SIZE {
            return Err(Error::AllocLimitExceeded {
                size: u64::from(ptr.length),
                limit: u64::from(crate::MAX_ASSET_SIZE),
            });
        }
        zones_file.seek(SeekFrom::Start(u64::from(ptr.offset)))?;
        let mut buf = vec![0u8; ptr.length as usize];
        zones_file.read_exact(&mut buf)?;
        let mut zone_ig = IgFile::open(Cursor::new(buf))?;
        on_each(parse_zone(&mut zone_ig, ptr.tuid)?);
    }
    Ok(())
}

fn parse_zone<R: Read + Seek>(zone: &mut IgFile<R>, zone_tuid: u64) -> Result<Zone> {
    let ufrags = parse_ufrags(zone)?;
    let ufrag_shader_tuids = read_shader_table(zone, SECT_UFRAG_SHADER_TABLE)?;
    let shrub_instances = parse_shrub_instances(zone, zone_tuid)?;
    let foliage_instances = parse_foliage_instances(zone, zone_tuid)?;

    let inst_section = match zone.section(SECT_TIE_INSTANCES) {
        Some(s) => s,
        None => {

            return Ok(Zone {
                tuid: zone_tuid,
                tie_instances: Vec::new(),
                shrub_instances,
                foliage_instances,
                ufrags,
                ufrag_shader_tuids,
            });
        }
    };
    let name_section = zone.require_section(SECT_TIE_NAME_POINTERS)?;
    let tuid_section = zone.require_section(SECT_TIE_TUID_TABLE)?;

    let count = inst_section.count as usize;
    if count == 0 {
        return Ok(Zone {
            tuid: zone_tuid,
            tie_instances: Vec::new(),
            shrub_instances,
            foliage_instances,
            ufrags,
            ufrag_shader_tuids,
        });
    }

    let mut raws: Vec<RawTie> = Vec::with_capacity(count);
    for i in 0..count {
        let base = u64::from(inst_section.offset) + (i as u64) * TIE_INSTANCE_SIZE;
        zone.stream.seek_to(base)?;
        let mut matrix = [0f32; 16];
        for slot in matrix.iter_mut() {
            *slot = zone.stream.read_f32()?;
        }
        let (position, scale, quaternion) = decompose_row_major(&matrix);

        zone.stream.seek_to(base + 0x4C)?;
        let bounding_radius = zone.stream.read_f32()?;
        zone.stream.seek_to(base + 0x50)?;
        let tie_index = zone.stream.read_u32()?;
        raws.push(RawTie {
            tie_index,
            position,
            quaternion,
            scale,
            bounding_radius,
        });
    }

    let mut metas: Vec<RawTieMeta> = Vec::with_capacity(count);
    for i in 0..count {
        let base = u64::from(name_section.offset) + (i as u64) * NAME_POINTER_SIZE;
        zone.stream.seek_to(base)?;
        let instance_tuid = zone.stream.read_u64()?;
        let name_ptr = u64::from(zone.stream.read_u32()?);
        let _length = zone.stream.read_u32()?;
        metas.push(RawTieMeta {
            instance_tuid,
            name_ptr,
        });
    }

    let names: Vec<String> = metas
        .iter()
        .map(|m| zone.stream.read_cstring_at(m.name_ptr))
        .collect::<Result<_>>()?;

    let mut tie_instances = Vec::with_capacity(count);
    for ((r, m), name) in raws.iter().zip(metas.iter()).zip(names.into_iter()) {
        let tie_index = u64::from(r.tie_index);
        let byte_offset = tie_index
            .checked_mul(8)
            .and_then(|x| x.checked_add(u64::from(tuid_section.offset)))
            .ok_or(Error::OffsetOverflow { id: SECT_TIE_TUID_TABLE })?;
        zone.stream.seek_to(byte_offset)?;
        let tie_tuid = zone.stream.read_u64()?;
        tie_instances.push(TieInstance {
            tie_tuid,
            instance_tuid: m.instance_tuid,
            name,
            position: r.position,
            quaternion: r.quaternion,
            scale: r.scale,
            bounding_radius: r.bounding_radius,
        });
    }

    Ok(Zone {
        tuid: zone_tuid,
        tie_instances,
        shrub_instances,
        foliage_instances,
        ufrags,
        ufrag_shader_tuids,
    })
}

fn parse_shrub_instances<R: Read + Seek>(
    zone: &mut IgFile<R>,
    zone_tuid: u64,
) -> Result<Vec<TieInstance>> {
    let Some(inst_section) = zone.section(SECT_SHRUB_INSTANCES) else {
        return Ok(Vec::new());
    };
    let Some(tuid_section) = zone.section(SECT_SHRUB_TUID_TABLE) else {
        return Ok(Vec::new());
    };
    let count = inst_section.count as usize;
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = u64::from(inst_section.offset) + (i as u64) * SHRUB_INSTANCE_SIZE;
        zone.stream.seek_to(base + 0x00)?;
        let position = zone.stream.read_vec3()?;
        let scale = zone.stream.read_f32()?;
        zone.stream.seek_to(base + 0x10)?;
        let r1 = zone.stream.read_vec3()?;
        zone.stream.seek_to(base + 0x20)?;
        let r2 = zone.stream.read_vec3()?;
        zone.stream.seek_to(base + 0x3C)?;
        let shrub_index = zone.stream.read_u32()?;

        let r3 = [
            r1[1] * r2[2] - r1[2] * r2[1],
            r1[2] * r2[0] - r1[0] * r2[2],
            r1[0] * r2[1] - r1[1] * r2[0],
        ];
        let matrix = [
            r1[0] * scale, r1[1] * scale, r1[2] * scale, 0.0,
            r2[0] * scale, r2[1] * scale, r2[2] * scale, 0.0,
            r3[0] * scale, r3[1] * scale, r3[2] * scale, 0.0,
            position[0],   position[1],   position[2],   1.0,
        ];
        let (pos, scl, quat) = decompose_row_major(&matrix);

        let byte_offset = u64::from(shrub_index)
            .checked_mul(8)
            .and_then(|x| x.checked_add(u64::from(tuid_section.offset)))
            .ok_or(Error::OffsetOverflow { id: SECT_SHRUB_TUID_TABLE })?;
        zone.stream.seek_to(byte_offset)?;
        let shrub_tuid = zone.stream.read_u64()?;

        let instance_tuid = synthetic_instance_tuid(zone_tuid, b'S', i as u32);
        out.push(TieInstance {
            tie_tuid: shrub_tuid,
            instance_tuid,
            name: String::new(),
            position: pos,
            quaternion: quat,
            scale: scl,
            bounding_radius: 0.0,
        });
    }
    Ok(out)
}

fn parse_foliage_instances<R: Read + Seek>(
    zone: &mut IgFile<R>,
    zone_tuid: u64,
) -> Result<Vec<TieInstance>> {
    let Some(inst_section) = zone.section(SECT_FOLIAGE_INSTANCES) else {
        return Ok(Vec::new());
    };
    let Some(tuid_section) = zone.section(SECT_FOLIAGE_TUID_TABLE) else {
        return Ok(Vec::new());
    };
    let count = inst_section.count as usize;
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = u64::from(inst_section.offset) + (i as u64) * FOLIAGE_INSTANCE_SIZE;
        zone.stream.seek_to(base)?;
        let mut matrix = [0f32; 16];
        for slot in matrix.iter_mut() {
            *slot = zone.stream.read_f32()?;
        }
        let (pos, scl, quat) = decompose_row_major(&matrix);

        zone.stream.seek_to(base + 0x94)?;
        let foliage_index = zone.stream.read_u32()?;

        let byte_offset = u64::from(foliage_index)
            .checked_mul(8)
            .and_then(|x| x.checked_add(u64::from(tuid_section.offset)))
            .ok_or(Error::OffsetOverflow { id: SECT_FOLIAGE_TUID_TABLE })?;
        zone.stream.seek_to(byte_offset)?;
        let foliage_tuid = zone.stream.read_u64()?;

        let instance_tuid = synthetic_instance_tuid(zone_tuid, b'F', i as u32);
        out.push(TieInstance {
            tie_tuid: foliage_tuid,
            instance_tuid,
            name: String::new(),
            position: pos,
            quaternion: quat,
            scale: scl,
            bounding_radius: 0.0,
        });
    }
    Ok(out)
}

fn synthetic_instance_tuid(zone_tuid: u64, kind: u8, index: u32) -> u64 {
    let tag = (kind as u64) << 56;
    let zone_low = (zone_tuid & 0x00FF_FFFF_FFFF_FFFF) ^ ((index as u64) << 32);
    tag | (zone_low & 0x00FF_FFFF_FFFF_FFFF) | (index as u64)
}

fn parse_ufrags<R: Read + Seek>(zone: &mut IgFile<R>) -> Result<Vec<UFrag>> {
    let Some(section) = zone.section(SECT_UFRAGS) else {
        return Ok(Vec::new());
    };

    let vertex_buf = if let Some(s) = zone.section(SECT_UFRAG_VERTICES) {
        zone.stream.seek_to(u64::from(s.offset))?;
        zone.stream.read_bytes(s.length as usize)?
    } else {
        Vec::new()
    };
    let index_buf = if let Some(s) = zone.section(SECT_UFRAG_INDICES) {
        zone.stream.seek_to(u64::from(s.offset))?;
        zone.stream.read_bytes(s.length as usize)?
    } else {
        Vec::new()
    };

    let count = section.count as usize;
    let mut ufrags = Vec::with_capacity(count);
    for i in 0..count {
        let base = u64::from(section.offset) + (i as u64) * UFRAG_SIZE;
        zone.stream.seek_to(base + 0x00)?;
        let tuid = zone.stream.read_u64()?;

        zone.stream.seek_to(base + 0x60)?;
        let position_raw = zone.stream.read_vec3()?;
        let position = [
            position_raw[0] * UFRAG_VERTEX_SCALE,
            position_raw[1] * UFRAG_VERTEX_SCALE,
            position_raw[2] * UFRAG_VERTEX_SCALE,
        ];
        let radius = zone.stream.read_f32()? * UFRAG_VERTEX_SCALE;

        zone.stream.seek_to(base + 0x40)?;
        let index_offset = zone.stream.read_u32()?;
        let vertex_offset = zone.stream.read_u32()?;

        let index_count = zone.stream.read_u16()?;
        let vertex_count = zone.stream.read_u16()?;

        zone.stream.seek_to(base + 0x50)?;
        let shader_index = zone.stream.read_u16()?;

        let (positions, uvs, indices) = decode_ufrag_mesh(
            &vertex_buf,
            &index_buf,
            vertex_offset,
            vertex_count,
            index_offset,
            index_count,
        );

        ufrags.push(UFrag {
            tuid,
            position,
            radius,
            vertex_count,
            index_count,
            shader_index,
            positions,
            uvs,
            indices,
        });
    }
    Ok(ufrags)
}

fn decode_ufrag_mesh(
    vertex_buf: &[u8],
    index_buf: &[u8],
    vertex_offset: u32,
    vertex_count: u16,
    index_offset: u32,
    index_count: u16,
) -> (Vec<f32>, Vec<f32>, Vec<u32>) {
    let v_start = vertex_offset as usize;
    let v_total = (vertex_count as usize) * UFRAG_VERTEX_STRIDE;
    let positions_uvs = if v_start + v_total > vertex_buf.len() {
        None
    } else {
        let mut positions = Vec::with_capacity((vertex_count as usize) * 3);
        let mut uvs = Vec::with_capacity((vertex_count as usize) * 2);
        for k in 0..(vertex_count as usize) {
            let base = v_start + k * UFRAG_VERTEX_STRIDE;
            let x = i16::from_be_bytes([vertex_buf[base], vertex_buf[base + 1]]) as f32
                * UFRAG_VERTEX_SCALE;
            let y = i16::from_be_bytes([vertex_buf[base + 2], vertex_buf[base + 3]]) as f32
                * UFRAG_VERTEX_SCALE;
            let z = i16::from_be_bytes([vertex_buf[base + 4], vertex_buf[base + 5]]) as f32
                * UFRAG_VERTEX_SCALE;
            positions.push(x);
            positions.push(y);
            positions.push(z);

            let u = half_to_f32(u16::from_be_bytes([
                vertex_buf[base + 0x08],
                vertex_buf[base + 0x09],
            ]));
            let v = half_to_f32(u16::from_be_bytes([
                vertex_buf[base + 0x0A],
                vertex_buf[base + 0x0B],
            ]));
            uvs.push(u);
            uvs.push(v);
        }
        Some((positions, uvs))
    };

    let i_start = index_offset as usize;
    let i_total = (index_count as usize) * 2;
    let indices = if i_start + i_total > index_buf.len() {
        Vec::new()
    } else {
        let mut indices = Vec::with_capacity(index_count as usize);
        for k in 0..(index_count as usize) {
            let off = i_start + k * 2;
            let v = u16::from_be_bytes([index_buf[off], index_buf[off + 1]]);
            indices.push(u32::from(v));
        }
        indices
    };

    let (positions, uvs) = positions_uvs.unwrap_or_else(|| (Vec::new(), Vec::new()));
    (positions, uvs, indices)
}

struct RawTie {
    tie_index: u32,
    position: [f32; 3],
    quaternion: [f32; 4],
    scale: [f32; 3],
    bounding_radius: f32,
}

struct RawTieMeta {
    instance_tuid: u64,
    name_ptr: u64,
}
