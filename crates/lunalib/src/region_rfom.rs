//! RFOM-era region/UFrag terrain reader.
//!
//! Ported from IT's `levelmain/extract.cpp::RegionToGltf` (line 859)
//! plus the `RegionMesh` struct in
//! `common/include/insomnia/classes/zone.hpp:45`. Produces one
//! synthetic [`Zone`] holding every region mesh as a [`UFrag`] so the
//! existing V2 UFrag pipeline (cache writer, MAP renderer, GLB
//! exporter) works unchanged.
//!
//! ## RegionMesh layout (`0x80` bytes — `zone.hpp:45`)
//! - `+0x00..+0x40` `OOBB bounds` (Spike OOBB = 64 bytes: origin Vector4A16 + 3×Vector4A16 extents)
//! - `+0x40` `Vector position` (3 floats, 12 bytes — yard-units, /256 NORM scale)
//! - `+0x4C` `u16 materialIndex0`
//! - `+0x4E` `u16 materialIndex1`
//! - `+0x50` `u32 unk3`
//! - `+0x54` `u32 indexOffset` — u16-units into ps3levelverts.dat 0x9100
//! - `+0x58` `u32 vertexOffset` — bytes into ps3levelverts.dat 0x9000
//! - `+0x5C` `u16 numIndices`
//! - `+0x5E` `u16 numVerties`
//!
//! ## RegionVertex (26 bytes — `internal/vertex.hpp:24`)
//! - `+0x00` `i16 position[3]`
//! - `+0x06` `i16 purpose`
//! - `+0x08` `f16 uv0[2]`
//! - `+0x0C` `f16 uv1[2]`
//! - `+0x10` `u16 normal[2]` — packed normal (we drop on import)
//! - `+0x14` `u16 tangent[2]`
//! - `+0x18` `u16 unk`
//!
//! Note: the V2 variant `RegionVertexV2` is 24 bytes (single `u32`
//! for normal + single `u32` for tangent, no trailing `unk`). Our
//! `zone.rs` (V2 path) correctly uses stride `0x18 = 24`. This
//! file (V1 path) used to use the wrong stride `0x14 = 20`, which
//! made every vertex past the first land inside the previous
//! vertex's normal field — giving random `i16` patterns that
//! filled the full ±32767 range and produced the "big chunks that
//! never end" terrain rendering.
//!
//! Position decode formula (per IT's AttributeMad):
//! ```text
//!   final_pos = (raw_i16 + region.position * 1.0) / 256 * YARD_TO_M
//! ```
//! where `YARD_TO_M = 0.9144`.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::moby::half_to_f32;
use crate::zone::{UFrag, Zone};

const SECT_REGION_MESH: u32 = 0x6200;
const REGION_MESH_SIZE: u64 = 0x80;
const REGION_VERTEX_STRIDE: usize = 0x1A;

const SECT_LEVEL_VERTEX_BUFFER: u32 = 0x9000;
const SECT_LEVEL_INDEX_BUFFER: u32 = 0x9100;

const SECT_MATERIAL_V1: u32 = 0x5001;

/// IT's `RegionToGltf` (`levelmain/extract.cpp:889-890`) decodes
/// V1 region vertex positions as `R16G16B16A16_NORM`, then applies
/// `world = normalized * mul + add` where:
///   `mul = (0x7FFF / 0x100) * YARD_TO_M`
///   `add = (item.position / 0x100) * YARD_TO_M`
/// which simplifies to `world = (raw_i16 + position) * YARD_TO_M / 256`
/// — bringing region geometry into meters, matching the meter-scale
/// moby (`gameplay_rfom.rs`) and tie (`tie_inst_rfom.rs`)
/// placements on this game.
const YARD_TO_M: f32 = 0.9144;
const POS_NORM_DIV: f32 = 256.0;
const REGION_VERTEX_SCALE: f32 = YARD_TO_M / POS_NORM_DIV;

pub fn read_regions_rfom(level_folder: &Path) -> Result<Vec<Zone>> {
    let main_path = level_folder.join("ps3levelmain.dat");
    let mut main_ig = IgFile::open(BufReader::new(File::open(&main_path)?))?;

    let global_material_count = main_ig
        .section(SECT_MATERIAL_V1)
        .map(|s| s.count)
        .unwrap_or(0) as usize;
    let identity_shader_tuids: Vec<u64> = (0..global_material_count as u64).collect();

    let region_section = match main_ig.section(SECT_REGION_MESH) {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };
    if region_section.length != REGION_MESH_SIZE as u32 {
        eprintln!(
            "warn: 0x6200 RegionMesh length is {} (expected {}) — skipping",
            region_section.length, REGION_MESH_SIZE
        );
        return Ok(Vec::new());
    }

    let verts_path = level_folder.join("ps3levelverts.dat");
    let mut verts_ig = IgFile::open(BufReader::new(File::open(&verts_path).map_err(|e| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("ps3levelverts.dat is required for RFOM regions: {e}"),
        ))
    })?))?;
    let vertex_section = verts_ig
        .section(SECT_LEVEL_VERTEX_BUFFER)
        .ok_or(Error::SectionNotFound(SECT_LEVEL_VERTEX_BUFFER))?;
    let index_section = verts_ig
        .section(SECT_LEVEL_INDEX_BUFFER)
        .ok_or(Error::SectionNotFound(SECT_LEVEL_INDEX_BUFFER))?;
    let vertex_section_offset = u64::from(vertex_section.offset);
    let index_section_offset = u64::from(index_section.offset);
    let vertex_section_length = u64::from(vertex_section.length);
    let index_section_length = u64::from(index_section.length);

    let count = region_section.count as usize;
    if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        eprintln!("[rfom] region meshes section: {count} entries");
    }

    let mut ufrags: Vec<UFrag> = Vec::with_capacity(count);
    let mut skipped = 0usize;
    for i in 0..count {
        let base = u64::from(region_section.offset) + (i as u64) * REGION_MESH_SIZE;

        main_ig.stream.seek_to(base + 0x40)?;
        let region_pos_x = main_ig.stream.read_f32()?;
        let region_pos_y = main_ig.stream.read_f32()?;
        let region_pos_z = main_ig.stream.read_f32()?;

        main_ig.stream.seek_to(base + 0x4C)?;
        let material_index = main_ig.stream.read_u16()?;
        let _material_index1 = main_ig.stream.read_u16()?;

        main_ig.stream.seek_to(base + 0x54)?;
        let index_offset = main_ig.stream.read_u32()?;
        let vertex_offset = main_ig.stream.read_u32()?;
        let num_indices = main_ig.stream.read_u16()?;
        let num_verts = main_ig.stream.read_u16()?;

        if num_indices == 0 || num_verts == 0 {
            skipped += 1;
            continue;
        }

        let v_byte_off = u64::from(vertex_offset);
        let v_byte_len = u64::from(num_verts) * REGION_VERTEX_STRIDE as u64;
        let i_byte_off = u64::from(index_offset) * 2;
        let i_byte_len = u64::from(num_indices) * 2;

        if v_byte_off + v_byte_len > vertex_section_length
            || i_byte_off + i_byte_len > index_section_length
        {
            eprintln!(
                "warn: RFOM region[{i}] geometry out of range; skipping (vert end {}, idx end {})",
                v_byte_off + v_byte_len,
                i_byte_off + i_byte_len
            );
            skipped += 1;
            continue;
        }

        verts_ig
            .stream
            .seek_to(vertex_section_offset + v_byte_off)?;
        let vertex_block = verts_ig.stream.read_bytes(v_byte_len as usize)?;

        verts_ig
            .stream
            .seek_to(index_section_offset + i_byte_off)?;
        let mut indices: Vec<u32> = Vec::with_capacity(num_indices as usize);
        for _ in 0..num_indices {
            indices.push(u32::from(verts_ig.stream.read_u16()?));
        }

        let mut positions: Vec<f32> = Vec::with_capacity(num_verts as usize * 3);
        let mut uvs: Vec<f32> = Vec::with_capacity(num_verts as usize * 2);
        let mut min_xyz = [f32::INFINITY, f32::INFINITY, f32::INFINITY];
        let mut max_xyz = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for k in 0..(num_verts as usize) {
            let v_off = k * REGION_VERTEX_STRIDE;
            let raw_x =
                i16::from_be_bytes([vertex_block[v_off], vertex_block[v_off + 1]]) as f32;
            let raw_y =
                i16::from_be_bytes([vertex_block[v_off + 2], vertex_block[v_off + 3]]) as f32;
            let raw_z =
                i16::from_be_bytes([vertex_block[v_off + 4], vertex_block[v_off + 5]]) as f32;

            // Local mesh coords in meters — per IT's `RegionToGltf`
            // formula simplification (yard-256ths × YARD_TO_M / 256).
            // The per-region world offset is carried by `UFrag.position`
            // below and applied once by the viewport's
            // `<mesh position={ufrag.position}>`. Don't bake it in here
            // or the offset gets applied twice.
            let x = raw_x * REGION_VERTEX_SCALE;
            let y = raw_y * REGION_VERTEX_SCALE;
            let z = raw_z * REGION_VERTEX_SCALE;
            positions.push(x);
            positions.push(y);
            positions.push(z);
            if x < min_xyz[0] { min_xyz[0] = x; }
            if x > max_xyz[0] { max_xyz[0] = x; }
            if y < min_xyz[1] { min_xyz[1] = y; }
            if y > max_xyz[1] { max_xyz[1] = y; }
            if z < min_xyz[2] { min_xyz[2] = z; }
            if z > max_xyz[2] { max_xyz[2] = z; }

            let uv_base = v_off + 0x08;
            let u = half_to_f32(u16::from_be_bytes([
                vertex_block[uv_base],
                vertex_block[uv_base + 1],
            ]));
            let v = half_to_f32(u16::from_be_bytes([
                vertex_block[uv_base + 2],
                vertex_block[uv_base + 3],
            ]));
            uvs.push(u);
            uvs.push(v);
        }

        // Position is in yard-256ths on disk; scale to meters so it
        // matches the moby/tie placements (those are in meters via
        // `* YARD_TO_M` in their respective readers). The viewport
        // renders `<mesh position={ufrag.position}>` which translates
        // the local-meters vertex array by this offset, so world
        // = local + position is consistent with moby/tie coords.
        let pos_meters = [
            region_pos_x * REGION_VERTEX_SCALE,
            region_pos_y * REGION_VERTEX_SCALE,
            region_pos_z * REGION_VERTEX_SCALE,
        ];
        if i < 3 && std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
            eprintln!(
                "[rfom-region] [{i}] raw_pos=({:.0}, {:.0}, {:.0}) → m=({:.2}, {:.2}, {:.2}) mesh_local_aabb m=[{:.2}..{:.2}, {:.2}..{:.2}, {:.2}..{:.2}] verts={num_verts} idx={num_indices}",
                region_pos_x, region_pos_y, region_pos_z,
                pos_meters[0], pos_meters[1], pos_meters[2],
                min_xyz[0], max_xyz[0], min_xyz[1], max_xyz[1], min_xyz[2], max_xyz[2],
            );
        }
        ufrags.push(UFrag {
            tuid: i as u64,
            position: pos_meters,
            radius: 0.0,
            vertex_count: num_verts,
            index_count: num_indices,
            shader_index: material_index,
            positions,
            uvs,
            indices,
        });
    }
    if skipped > 0 {
        eprintln!("[rfom] skipped {skipped} region meshes with empty/out-of-range geometry");
    }

    Ok(vec![Zone {
        tuid: 0,
        tie_instances: Vec::new(),
        shrub_instances: Vec::new(),
        foliage_instances: Vec::new(),
        ufrags,
        ufrag_shader_tuids: identity_shader_tuids,
    }])
}
