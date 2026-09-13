//! TOD-era tie reader (R&C: Tools of Destruction).
//!
//! Ported from ReLunacy `LibLunacy/Tie.cs::CTie` `isOld` branch +
//! `LibLunacy/AssetLoader.cs::LoadTiesOld`. Produces the same
//! [`TieAsset`] struct as the V2 reader (`tie.rs`) so downstream code
//! does not branch.
//!
//! ## Layout
//! - `main.dat` section `0x3400` — array of `count` × `0x80` bytes,
//!   one `OldTie` header per entry. Tie ID for TOD is the entry's
//!   byte offset inside `main.dat` (`section.offset + i * 0x80`).
//! - Vertex stream lives in `vertices.dat` section `0x9000`, indexed
//!   by `vertexBufferStart` + per-mesh `vertexIndex * 0x14`.
//! - Index stream lives in `vertices.dat` section `0x9100`, indexed
//!   by per-mesh `indexIndex` (in u16 units, * 2 for bytes).
//! - Per-mesh `oldShaderIndex` is a u16 at +0x28 — direct lookup into
//!   the global shader DB read from `main.dat` 0x5000.
//!
//! Vertex layout is the static `Vertex0` form (stride 0x14):
//! `i16 x, i16 y, i16 z, i16 purpose, f16 u, f16 v, ...`.

use std::io::{Cursor, Read, Seek};
use std::path::Path;

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::moby::half_to_f32;
use crate::tie::{TieAsset, TieMeshGeom};

const SECT_OLD_TIE: u32 = 0x3400;
const OLD_TIE_HEADER_SIZE: u64 = 0x80;
const OLD_TIE_MESH_SIZE: u64 = 0x40;
const OLD_TIE_VERTEX_STRIDE: usize = 0x14;

const SECT_LEVEL_VERTEX_BUFFER: u32 = 0x9000;
const SECT_LEVEL_INDEX_BUFFER: u32 = 0x9100;

/// Read every TOD tie from `main.dat` + `vertices.dat`.
pub fn read_tie_assets_old<F>(level_folder: &Path, mut on_each: F) -> Result<()>
where
    F: FnMut(TieAsset),
{
    read_tie_assets_old_with_total(level_folder, |_| {}, |a| on_each(a))
}

pub fn read_tie_assets_old_with_total<T, F>(
    level_folder: &Path,
    mut on_total: T,
    mut on_each: F,
) -> Result<()>
where
    T: FnMut(usize),
    F: FnMut(TieAsset),
{
    let main_path = level_folder.join("main.dat");
    let main_bytes = std::fs::read(&main_path)?;
    let main_ig = IgFile::open(Cursor::new(main_bytes.as_slice()))?;

    let tie_section = match main_ig.section(SECT_OLD_TIE) {
        Some(s) => s,
        None => return Ok(()),
    };

    let vertices_path = level_folder.join("vertices.dat");
    let vert_bytes = std::fs::read(&vertices_path).map_err(|e| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("vertices.dat is required for TOD ties: {e}"),
        ))
    })?;
    let vertices_ig = IgFile::open(Cursor::new(vert_bytes.as_slice()))?;
    let vertex_section = vertices_ig
        .section(SECT_LEVEL_VERTEX_BUFFER)
        .ok_or(Error::SectionNotFound(SECT_LEVEL_VERTEX_BUFFER))?;
    let index_section = vertices_ig
        .section(SECT_LEVEL_INDEX_BUFFER)
        .ok_or(Error::SectionNotFound(SECT_LEVEL_INDEX_BUFFER))?;

    let log_probes = std::env::var("RECHIMERA_LOG_PROBES").is_ok();
    let count = tie_section.count as usize;
    on_total(count);
    if log_probes {
        eprintln!(
            "[tod-tie] section 0x3400: count={} offset=0x{:X} length={} (stride 0x{:X})",
            count, tie_section.offset, tie_section.length, OLD_TIE_HEADER_SIZE
        );
        eprintln!(
            "[tod-tie] vertices.dat 0x9000: offset=0x{:X} length={} bytes",
            vertex_section.offset, vertex_section.length
        );
        eprintln!(
            "[tod-tie] vertices.dat 0x9100: offset=0x{:X} length={} bytes",
            index_section.offset, index_section.length
        );
    }
    let section_offset = u64::from(tie_section.offset);
    let v_off = u64::from(vertex_section.offset);
    let i_off = u64::from(index_section.offset);
    let v_len = u64::from(vertex_section.length);
    let i_len = u64::from(index_section.length);
    let mut parsed: Vec<(usize, Result<TieAsset>)> = (0..count)
        .into_par_iter()
        .map(|i| {
            let header_off = section_offset + (i as u64) * OLD_TIE_HEADER_SIZE;
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
                    i,
                    log_probes,
                )
            })();
            (i, result)
        })
        .collect();
    parsed.sort_by_key(|(i, _)| *i);
    for (i, result) in parsed {
        match result {
            Ok(asset) => on_each(asset),
            Err(e) => {
                eprintln!("warn: TOD tie[{i}] skipped — parse failed: {e}");
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_one<R1: Read + Seek, R2: Read + Seek>(
    main_ig: &mut IgFile<R1>,
    vertices_ig: &mut IgFile<R2>,
    header_off: u64,
    vertex_section_offset: u64,
    index_section_offset: u64,
    vertex_section_length: u64,
    index_section_length: u64,
    tie_index: usize,
    log_probes: bool,
) -> Result<TieAsset> {
    main_ig.stream.seek_to(header_off + 0x00)?;
    let meshes_ptr = u64::from(main_ig.stream.read_u32()?);
    main_ig.stream.seek_to(header_off + 0x0F)?;
    let mesh_count = main_ig.stream.read_u8()? as usize;

    main_ig.stream.seek_to(header_off + 0x14)?;
    let vertex_buffer_start = main_ig.stream.read_u32()? as u64;
    let vertex_buffer_size = main_ig.stream.read_u32()? as usize;

    main_ig.stream.seek_to(header_off + 0x20)?;
    let scale_x = main_ig.stream.read_f32()?;
    let scale_y = main_ig.stream.read_f32()?;
    let scale_z = main_ig.stream.read_f32()?;

    if log_probes {
        let overruns_vbuf =
            vertex_buffer_start + (vertex_buffer_size as u64) > vertex_section_length;
        eprintln!(
            "[tod-tie] tie[{}] header=0x{:X} meshes_ptr=0x{:X} mesh_count={} \
             vbuf_start=0x{:X} vbuf_size={} scale=({:.4},{:.4},{:.4}){}",
            tie_index,
            header_off,
            meshes_ptr,
            mesh_count,
            vertex_buffer_start,
            vertex_buffer_size,
            scale_x,
            scale_y,
            scale_z,
            if overruns_vbuf { " [OVERRUNS 0x9000]" } else { "" }
        );
    }

    let mut headers: Vec<TodTieMeshHeader> = Vec::with_capacity(mesh_count);
    let mut shader_tuids: Vec<u64> = Vec::with_capacity(mesh_count);
    let mut max_v_local_end: usize = 0;
    for m in 0..mesh_count {
        let mesh_base = meshes_ptr + (m as u64) * OLD_TIE_MESH_SIZE;

        main_ig.stream.seek_to(mesh_base + 0x00)?;
        let index_index = main_ig.stream.read_u32()?;

        main_ig.stream.seek_to(mesh_base + 0x04)?;
        let vertex_index = main_ig.stream.read_u16()? as usize;

        main_ig.stream.seek_to(mesh_base + 0x08)?;
        let vertex_count = main_ig.stream.read_u16()?;

        main_ig.stream.seek_to(mesh_base + 0x12)?;
        let index_count = main_ig.stream.read_u16()?;

        main_ig.stream.seek_to(mesh_base + 0x28)?;
        let old_shader_index = main_ig.stream.read_u16()?;

        let v_local_end = (vertex_index + vertex_count as usize) * OLD_TIE_VERTEX_STRIDE;
        if v_local_end > max_v_local_end {
            max_v_local_end = v_local_end;
        }

        if log_probes && (tie_index < 3 || (78..=85).contains(&tie_index)) {
            let idx_byte_end = (index_index as u64) * 2 + (index_count as u64) * 2;
            eprintln!(
                "[tod-tie]   mesh[{m}] idx_idx={} idx_cnt={} v_idx={} v_cnt={} shader=0x{:X} \
                 idx_byte_end=0x{:X}/{:X} v_local_end=0x{:X}",
                index_index,
                index_count,
                vertex_index,
                vertex_count,
                old_shader_index,
                idx_byte_end,
                index_section_length,
                v_local_end,
            );
        }

        headers.push(TodTieMeshHeader {
            index_index,
            vertex_index,
            vertex_count,
            index_count,
        });
        shader_tuids.push(old_shader_index as u64);
    }

    // Slurp only what per-mesh data actually addresses, ignoring the
    // header's `vertex_buffer_size` field (it's wildly over-allocated and
    // for later ties points past EOF — ReLunacy survives via short-read
    // tolerance; we compute the real extent instead, same pattern as
    // `moby_old.rs::max_vertex_end`).
    let vertex_block = read_section_slice(
        vertices_ig,
        vertex_section_offset + vertex_buffer_start,
        max_v_local_end,
    )?;

    let mut meshes: Vec<TieMeshGeom> = Vec::with_capacity(mesh_count);
    for (m, h) in headers.iter().enumerate() {
        let index_byte_offset = index_section_offset + (h.index_index as u64) * 2;
        let mut indices = Vec::with_capacity(h.index_count as usize);
        vertices_ig.stream.seek_to(index_byte_offset)?;
        for _ in 0..h.index_count {
            indices.push(u32::from(vertices_ig.stream.read_u16()?));
        }

        let mut positions: Vec<f32> = Vec::with_capacity(h.vertex_count as usize * 3);
        let mut uvs: Vec<f32> = Vec::with_capacity(h.vertex_count as usize * 2);
        for k in 0..(h.vertex_count as usize) {
            let v_off = (h.vertex_index + k) * OLD_TIE_VERTEX_STRIDE;
            if v_off + OLD_TIE_VERTEX_STRIDE > vertex_block.len() {
                return Err(Error::SectionLengthMismatch {
                    id: SECT_OLD_TIE,
                    length: vertex_block.len() as u32,
                    entry: OLD_TIE_VERTEX_STRIDE as u32,
                });
            }
            let raw_x = i16::from_be_bytes([vertex_block[v_off], vertex_block[v_off + 1]]) as f32;
            let raw_y =
                i16::from_be_bytes([vertex_block[v_off + 2], vertex_block[v_off + 3]]) as f32;
            let raw_z =
                i16::from_be_bytes([vertex_block[v_off + 4], vertex_block[v_off + 5]]) as f32;
            positions.push(raw_x * scale_x);
            positions.push(raw_y * scale_y);
            positions.push(raw_z * scale_z);

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

        meshes.push(TieMeshGeom {
            shader_index: m as u16,
            vertex_count: h.vertex_count,
            index_count: h.index_count,
            positions,
            uvs,
            indices,
        });
    }

    Ok(TieAsset {
        tuid: header_off,
        scale: [scale_x, scale_y, scale_z],
        meshes,
        shader_tuids,
    })
}

struct TodTieMeshHeader {
    index_index: u32,
    vertex_index: usize,
    vertex_count: u16,
    index_count: u16,
}

fn read_section_slice<R: Read + Seek>(
    ig: &mut IgFile<R>,
    abs_offset: u64,
    length: usize,
) -> Result<Vec<u8>> {
    if length == 0 {
        return Ok(Vec::new());
    }
    ig.stream.seek_to(abs_offset)?;
    ig.stream.read_bytes(length)
}
