//! RFOM-era tie reader (Resistance: Fall of Man).
//!
//! Ported from InsomniaToolset's `levelmain/extract.cpp::TieToGltf`
//! (line 679) plus the `TieV1` / `TiePrimitiveV1` struct definitions
//! in `common/include/insomnia/classes/tie.hpp:114-144`. Produces the
//! same [`TieAsset`] shape as the V2 / TOD readers so the cache
//! pipeline doesn't branch.
//!
//! ## Critical: ties read from `ps3levelverts.dat`, not `ps3leveltexs.dat`
//! IT line 1369-1372 parses `ps3levelverts.dat` as IGHW and pulls the
//! `LevelVertexBuffer` (`0x9000`) + `LevelIndexBuffer` (`0x9100`)
//! sections — those feed ties, regions, details and shrubs. (Mobys
//! source from `ps3leveltexs.dat` — different file, raw bytes.)
//!
//! ## TieV1 layout (`0x40` bytes — `tie.hpp:129`)
//! - `+0x00` `PointerX86<TiePrimitiveV1>` primitives
//! - `+0x04` `PointerX86<OOBB>` bounds  (unused here)
//! - `+0x08` `u16` numMeshes
//! - `+0x0A` `u16` numBounds
//! - `+0x0C` `u32` unk02
//! - `+0x10` `u32` vertexBufferOffset0  (byte offset into 0x9000)
//! - `+0x14` `u32` vertexBufferOffset1  (UV2 stream — only when useUv2)
//! - `+0x18` `u16` tieId
//! - `+0x1A` `u16` unk16
//! - `+0x1C` `u32` null00
//! - `+0x20..+0x2C` `Vector meshScale` (3 floats)
//! - `+0x2C..+0x40` `float unk03[5]` (5 floats = 20 bytes)
//!
//! ## TiePrimitiveV1 layout (`0x20` bytes — `tie.hpp:114`)
//! - `+0x00` `u16` materialIndex   (index into global 0x5001 array)
//! - `+0x02` `u16` unk1
//! - `+0x04` `u32` indexOffset     (u16-units into 0x9100)
//! - `+0x08` `u16` numIndices
//! - `+0x0A` `u16` numVertices
//! - `+0x0C` `u16` vertexOffset0   (Vertex0-units = 0x14 bytes each, into the tie's
//!                                  local block at `tie.vertexBufferOffset0`)
//! - `+0x0E` `u16` vertexOffset1
//! - `+0x10` `u8`  useUv2
//! - `+0x11..+0x20` unk

use std::io::{Cursor, Read, Seek};
use std::path::Path;

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::moby::half_to_f32;
use crate::tie::{TieAsset, TieMeshGeom};

const SECT_TIE: u32 = 0x3400;
const RFOM_TIE_HEADER_SIZE: u64 = 0x40;
const RFOM_TIE_PRIMITIVE_SIZE: u64 = 0x20;
const RFOM_TIE_VERTEX_STRIDE: usize = 0x14;

const SECT_LEVEL_VERTEX_BUFFER: u32 = 0x9000;
const SECT_LEVEL_INDEX_BUFFER: u32 = 0x9100;

const SECT_MATERIAL_V1: u32 = 0x5001;

pub fn read_tie_assets_rfom<F>(level_folder: &Path, mut on_each: F) -> Result<()>
where
    F: FnMut(TieAsset),
{
    read_tie_assets_rfom_with_total(level_folder, |_| {}, |a| on_each(a))
}

pub fn read_tie_assets_rfom_with_total<T, F>(
    level_folder: &Path,
    mut on_total: T,
    mut on_each: F,
) -> Result<()>
where
    T: FnMut(usize),
    F: FnMut(TieAsset),
{
    let main_path = level_folder.join("ps3levelmain.dat");
    let main_bytes = std::fs::read(&main_path)?;
    let main_ig = IgFile::open(Cursor::new(main_bytes.as_slice()))?;

    let global_material_count = main_ig
        .section(SECT_MATERIAL_V1)
        .map(|s| s.count)
        .unwrap_or(0) as usize;

    let tie_section = match main_ig.section(SECT_TIE) {
        Some(s) => s,
        None => return Ok(()),
    };
    if tie_section.length != RFOM_TIE_HEADER_SIZE as u32 {
        eprintln!(
            "warn: ps3levelmain.dat 0x3400 length is {} (expected {} for TieV1) — skipping",
            tie_section.length, RFOM_TIE_HEADER_SIZE
        );
        return Ok(());
    }

    let verts_path = level_folder.join("ps3levelverts.dat");
    let vert_bytes = std::fs::read(&verts_path).map_err(|e| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("ps3levelverts.dat is required for RFOM ties: {e}"),
        ))
    })?;
    let verts_ig = IgFile::open(Cursor::new(vert_bytes.as_slice()))?;
    let vertex_section = verts_ig
        .section(SECT_LEVEL_VERTEX_BUFFER)
        .ok_or(Error::SectionNotFound(SECT_LEVEL_VERTEX_BUFFER))?;
    let index_section = verts_ig
        .section(SECT_LEVEL_INDEX_BUFFER)
        .ok_or(Error::SectionNotFound(SECT_LEVEL_INDEX_BUFFER))?;

    let count = tie_section.count as usize;
    on_total(count);
    if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        eprintln!("[rfom] ties section: {count} ties");
    }
    let section_offset = u64::from(tie_section.offset);
    let v_off = u64::from(vertex_section.offset);
    let i_off = u64::from(index_section.offset);
    let v_len = u64::from(vertex_section.length);
    let i_len = u64::from(index_section.length);
    let mut parsed: Vec<(usize, Result<Option<TieAsset>>)> = (0..count)
        .into_par_iter()
        .map(|i| {
            let header_off = section_offset + (i as u64) * RFOM_TIE_HEADER_SIZE;
            let result = (|| {
                let mut mig = IgFile::open(Cursor::new(main_bytes.as_slice()))?;
                let mut vig = IgFile::open(Cursor::new(vert_bytes.as_slice()))?;
                parse_one(
                    &mut mig,
                    &mut vig,
                    header_off,
                    v_off,
                    i_off,
                    v_len,
                    i_len,
                    global_material_count,
                )
            })();
            (i, result)
        })
        .collect();
    parsed.sort_by_key(|(i, _)| *i);
    for (i, result) in parsed {
        match result {
            Ok(Some(asset)) => on_each(asset),
            Ok(None) => {}
            Err(e) => {
                eprintln!("warn: RFOM tie[{i}] skipped — parse failed: {e}");
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_one<R1: Read + Seek, R2: Read + Seek>(
    main_ig: &mut IgFile<R1>,
    verts_ig: &mut IgFile<R2>,
    header_off: u64,
    vertex_section_offset: u64,
    index_section_offset: u64,
    vertex_section_length: u64,
    index_section_length: u64,
    global_material_count: usize,
) -> Result<Option<TieAsset>> {
    main_ig.stream.seek_to(header_off + 0x00)?;
    let primitives_ptr = u64::from(main_ig.stream.read_u32()?);

    main_ig.stream.seek_to(header_off + 0x08)?;
    let mesh_count = main_ig.stream.read_u16()? as usize;

    main_ig.stream.seek_to(header_off + 0x10)?;
    let vertex_buffer_offset0 = u64::from(main_ig.stream.read_u32()?);
    let _vertex_buffer_offset1 = u64::from(main_ig.stream.read_u32()?);

    main_ig.stream.seek_to(header_off + 0x18)?;
    let _tie_id = main_ig.stream.read_u16()?;

    main_ig.stream.seek_to(header_off + 0x20)?;
    let scale_x = main_ig.stream.read_f32()?;
    let scale_y = main_ig.stream.read_f32()?;
    let scale_z = main_ig.stream.read_f32()?;

    if mesh_count == 0 || primitives_ptr == 0 {
        return Ok(None);
    }

    let mut meshes: Vec<TieMeshGeom> = Vec::with_capacity(mesh_count);
    for m in 0..mesh_count {
        let prim_base = primitives_ptr + (m as u64) * RFOM_TIE_PRIMITIVE_SIZE;

        main_ig.stream.seek_to(prim_base + 0x00)?;
        let material_index = main_ig.stream.read_u16()?;
        let _unk1 = main_ig.stream.read_u16()?;
        let index_offset = main_ig.stream.read_u32()?;
        let index_count = main_ig.stream.read_u16()?;
        let vertex_count = main_ig.stream.read_u16()?;
        let vertex_offset0 = main_ig.stream.read_u16()?;

        let v_byte_off = vertex_buffer_offset0 + (vertex_offset0 as u64) * RFOM_TIE_VERTEX_STRIDE as u64;
        let v_byte_len = (vertex_count as u64) * RFOM_TIE_VERTEX_STRIDE as u64;
        let i_byte_off = (index_offset as u64) * 2;
        let i_byte_len = (index_count as u64) * 2;

        if v_byte_off + v_byte_len > vertex_section_length
            || i_byte_off + i_byte_len > index_section_length
        {
            eprintln!(
                "warn: RFOM tie mesh out of range — vert [{v_byte_off}..{}], idx [{i_byte_off}..{}]; skipping",
                v_byte_off + v_byte_len,
                i_byte_off + i_byte_len
            );
            continue;
        }

        verts_ig
            .stream
            .seek_to(vertex_section_offset + v_byte_off)?;
        let vertex_block = verts_ig.stream.read_bytes(v_byte_len as usize)?;

        verts_ig
            .stream
            .seek_to(index_section_offset + i_byte_off)?;
        let mut indices: Vec<u32> = Vec::with_capacity(index_count as usize);
        for _ in 0..index_count {
            indices.push(u32::from(verts_ig.stream.read_u16()?));
        }

        let mut positions: Vec<f32> = Vec::with_capacity(vertex_count as usize * 3);
        let mut uvs: Vec<f32> = Vec::with_capacity(vertex_count as usize * 2);
        let mut min_xyz = [f32::INFINITY; 3];
        let mut max_xyz = [f32::NEG_INFINITY; 3];
        for k in 0..(vertex_count as usize) {
            let v_off = k * RFOM_TIE_VERTEX_STRIDE;
            let raw_x = i16::from_be_bytes([vertex_block[v_off], vertex_block[v_off + 1]]) as f32;
            let raw_y =
                i16::from_be_bytes([vertex_block[v_off + 2], vertex_block[v_off + 3]]) as f32;
            let raw_z =
                i16::from_be_bytes([vertex_block[v_off + 4], vertex_block[v_off + 5]]) as f32;
            // IT `levelmain/extract.cpp:693`:
            //   `AttributeMul attributeMul{tie.meshScale * 0x7fff}`
            // applied via R16G16B16A16 NORM, i.e.
            //   world_local = (raw_i16 / 32767) * (meshScale * 32767)
            //              = raw_i16 * meshScale  (per-axis)
            // `meshScale` is the `Vector meshScale` at `+0x20..+0x2C`.
            let x = raw_x * scale_x;
            let y = raw_y * scale_y;
            let z = raw_z * scale_z;
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

        if header_off < 0x100000 && m < 2 {
            eprintln!(
                "[rfom-tie-mesh] tie@0x{header_off:X} mesh[{m}] scale=({:.4}, {:.4}, {:.4}) verts={vertex_count} idx={index_count} local_aabb=[{:.2}..{:.2}, {:.2}..{:.2}, {:.2}..{:.2}]",
                scale_x, scale_y, scale_z,
                min_xyz[0], max_xyz[0], min_xyz[1], max_xyz[1], min_xyz[2], max_xyz[2],
            );
        }

        meshes.push(TieMeshGeom {
            shader_index: material_index,
            vertex_count,
            index_count,
            positions,
            uvs,
            indices,
        });
    }

    if meshes.is_empty() {
        return Ok(None);
    }

    let identity_shader_tuids: Vec<u64> = (0..global_material_count as u64).collect();

    Ok(Some(TieAsset {
        tuid: header_off,
        scale: [scale_x, scale_y, scale_z],
        meshes,
        shader_tuids: identity_shader_tuids,
    }))
}
