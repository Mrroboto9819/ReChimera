//! TOD-era zone reader (R&C: Tools of Destruction).
//!
//! Ported from ReLunacy `LibLunacy/Zone.cs::CZone` `isOld` branch +
//! `LibLunacy/AssetLoader.cs::LoadZonesOld`. Produces the same
//! [`Zone`] struct as the V2 / RFOM paths so the cache writer, MAP
//! renderer and GLB exporter consume it without branching.
//!
//! ## Layout — TOD has **one zone per level**
//! Per `LoadZonesOld:252` ReLunacy hard-codes a single iteration; the
//! whole level's tie placements and UFrag terrain live inline in
//! `main.dat`. There are no per-zone `.dat` files like V2.
//!
//! ## Tie instances — section `0x9240` in `main.dat`
//! Array of `count` × `0x80`-byte records (`Zone.cs:9-16`).
//! - `+0x00..0x40` `Matrix4x4` transformation (row-major; translation
//!   at flat indices [12..14] in yards — we multiply by `YARD_TO_M` to
//!   match the moby/tie/ufrag meter convention used by the rest of
//!   the pipeline)
//! - `+0x40..0x4C` `Vector3` boundingPosition
//! - `+0x4C..0x50` `float` boundingRadius
//! - `+0x50` `u32` tie reference — **byte offset in `main.dat` of the
//!   matching `OldTie` header**, which is exactly the value
//!   [`crate::tie::TieAsset::tuid`] carries on the TOD path. So linking
//!   instance → prototype is a hash-map lookup by `tie_tuid`.
//!
//! ## UFrag terrain — section `0x6200` in `main.dat`
//! Array of `count` × `0x80`-byte `OldUFrag` records (`Zone.cs:77-111`).
//! - `+0x00` `u64 tuid`
//! - `+0x10` `Vector4 rotation` (unused — we let the vertex coords
//!   carry world orientation)
//! - `+0x40` `u32 indexOffset` — **a u16 COUNT, not bytes**. Multiply
//!   by 2 to seek into `vertices.dat:0x9100`. Per `Zone.cs:286`:
//!   `oldUFrag.indexOffset *= sizeof(ushort)`.
//! - `+0x44` `u32 vertexOffset` — bytes into `vertices.dat:0x9000`
//! - `+0x48` `u16 indexCount`
//! - `+0x4A` `u16 vertexCount`
//! - `+0x50` `u16 shaderIndex` — into the `0x71A0` table below
//! - `+0x60` `Vector3 position` (yard-units; we apply `YARD_TO_M/256`
//!   so terrain lands at the same meter scale as moby/tie placements,
//!   mirroring `region_rfom.rs`)
//!
//! ## UFrag vertex stream (stride `0x18 = 24` bytes)
//! Per ReLunacy `OldUFragVertex` (`Zone.cs:128-141`):
//! - `+0x00..+0x06` i16 x, y, z
//! - `+0x08..+0x0C` half-float UV0
//! - `+0x0C..+0x10` half-float UV1 (unused)
//!
//! Position decode: `world = (raw_i16 + ufrag.position) * YARD_TO_M / 256`,
//! same simplified formula as `region_rfom.rs` and `zone.rs` (V2).
//!
//! ## UFrag shader resolution — direct into global DB
//! Per `Zone.cs:305-313`, for `isOld` (TOD) the ufrag's `shaderIndex`
//! is a **direct** index into the global shader DB (`main.dat 0x5000`).
//! The per-zone `0x71A0` indirection table only applies to the new
//! engine. So we synthesize an identity `ufrag_shader_tuids` table
//! sized to the global shader count — then the cache writer's lookup
//! `ufrag_shader_tuids[shader_index]` resolves to `shader_index`,
//! which matches the key in the global shader map (same convention as
//! `region_rfom.rs`).

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::math::decompose_row_major;
use crate::moby::half_to_f32;
use crate::zone::{TieInstance, UFrag, Zone};

const SECT_OLD_TIE_INSTANCE: u32 = 0x9240;
const OLD_TIE_INSTANCE_SIZE: u64 = 0x80;

const SECT_OLD_UFRAG: u32 = 0x6200;
const OLD_UFRAG_SIZE: u64 = 0x80;
const OLD_UFRAG_VERTEX_STRIDE: usize = 0x18;

const SECT_OLD_SHADER: u32 = 0x5000;

const SECT_LEVEL_VERTEX_BUFFER: u32 = 0x9000;
const SECT_LEVEL_INDEX_BUFFER: u32 = 0x9100;

/// Same convention as `region_rfom.rs` / `zone.rs`: tied to the
/// `R16G16B16A16_NORM` decode `(raw_i16 + position) * YARD_TO_M / 256`.
/// Brings TOD terrain into the meter space shared with mobys / ties.
const YARD_TO_M: f32 = 0.9144;
const POS_NORM_DIV: f32 = 256.0;
const UFRAG_VERTEX_SCALE: f32 = YARD_TO_M / POS_NORM_DIV;

/// Read TOD-era zone(s) from `main.dat` + `vertices.dat`.
///
/// Returns a single-element `Vec<Zone>` matching ReLunacy's
/// "one art zone" convention.
pub fn read_zones_old(level_folder: &Path) -> Result<Vec<Zone>> {
    let main_path = level_folder.join("main.dat");
    let mut main_ig = IgFile::open(BufReader::new(File::open(&main_path)?))?;

    let log_probes = std::env::var("RECHIMERA_LOG_PROBES").is_ok();

    let tie_instances = read_tie_instances(&mut main_ig, log_probes);
    let ufrag_shader_tuids = read_ufrag_shader_table(&mut main_ig);
    let ufrags = read_ufrags(&mut main_ig, level_folder, log_probes)?;

    if log_probes {
        eprintln!(
            "[tod-zone] one zone: {} tie instances, {} ufrags, {} ufrag shaders",
            tie_instances.len(),
            ufrags.len(),
            ufrag_shader_tuids.len()
        );
    }

    Ok(vec![Zone {
        tuid: 0,
        tie_instances,
        shrub_instances: Vec::new(),
        foliage_instances: Vec::new(),
        ufrags,
        ufrag_shader_tuids,
    }])
}

fn read_tie_instances<R: std::io::Read + std::io::Seek>(
    ig: &mut IgFile<R>,
    log_probes: bool,
) -> Vec<TieInstance> {
    let section = match ig.section(SECT_OLD_TIE_INSTANCE) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let count = section.count as usize;
    let mut out: Vec<TieInstance> = Vec::with_capacity(count);
    let mut sample_logged = 0usize;
    for i in 0..count {
        let base = u64::from(section.offset) + (i as u64) * OLD_TIE_INSTANCE_SIZE;
        match parse_one_tie_instance(ig, base, i) {
            Ok(inst) => {
                if sample_logged < 3 && log_probes {
                    eprintln!(
                        "[tod-zone-tieinst] [{i}] tie_ref=0x{:X} pos_m=({:.2},{:.2},{:.2}) \
                         scale=({:.3},{:.3},{:.3}) r={:.2}",
                        inst.tie_tuid,
                        inst.position[0], inst.position[1], inst.position[2],
                        inst.scale[0], inst.scale[1], inst.scale[2],
                        inst.bounding_radius,
                    );
                    sample_logged += 1;
                }
                out.push(inst);
            }
            Err(e) => {
                eprintln!("warn: TOD tie-instance[{i}] skipped: {e}");
            }
        }
    }
    out
}

fn parse_one_tie_instance<R: std::io::Read + std::io::Seek>(
    ig: &mut IgFile<R>,
    base: u64,
    index: usize,
) -> Result<TieInstance> {
    ig.stream.seek_to(base + 0x00)?;
    let mut m = [0f32; 16];
    for slot in m.iter_mut() {
        *slot = ig.stream.read_f32()?;
    }
    let (translation_raw, scale, quat) = decompose_row_major(&m);

    ig.stream.seek_to(base + 0x4C)?;
    let bounding_radius = ig.stream.read_f32()?;

    ig.stream.seek_to(base + 0x50)?;
    let tie_ref = ig.stream.read_u32()?;

    let position = [
        translation_raw[0] * YARD_TO_M,
        translation_raw[1] * YARD_TO_M,
        translation_raw[2] * YARD_TO_M,
    ];
    let bounding_radius_m = bounding_radius * YARD_TO_M;

    Ok(TieInstance {
        // Tie reference is the byte offset of the OldTie header in
        // main.dat — matches `tie_old::TieAsset::tuid` so downstream
        // lookups work without translation.
        tie_tuid: u64::from(tie_ref),
        instance_tuid: index as u64,
        name: format!("TieInstance_{index:04X}"),
        position,
        quaternion: quat,
        scale,
        bounding_radius: bounding_radius_m,
    })
}

fn read_ufrag_shader_table<R: std::io::Read + std::io::Seek>(ig: &mut IgFile<R>) -> Vec<u64> {
    // Identity table sized to the global shader DB (main.dat 0x5000).
    // Per ReLunacy Zone.cs:308, TOD ufrags resolve shaders by
    // `al.shaders[shaderIndex]` directly — no per-zone indirection.
    let count = ig
        .section(SECT_OLD_SHADER)
        .map(|s| s.count as u64)
        .unwrap_or(0);
    (0..count).collect()
}

fn read_ufrags<R: std::io::Read + std::io::Seek>(
    main_ig: &mut IgFile<R>,
    level_folder: &Path,
    log_probes: bool,
) -> Result<Vec<UFrag>> {
    let section = match main_ig.section(SECT_OLD_UFRAG) {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };
    if section.length != OLD_UFRAG_SIZE as u32 {
        eprintln!(
            "warn: TOD 0x6200 OldUFrag length is {} (expected {}) — skipping ufrags",
            section.length, OLD_UFRAG_SIZE
        );
        return Ok(Vec::new());
    }

    let verts_path = level_folder.join("vertices.dat");
    let mut verts_ig = IgFile::open(BufReader::new(File::open(&verts_path).map_err(|e| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("vertices.dat is required for TOD ufrags: {e}"),
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

    let count = section.count as usize;
    if log_probes {
        eprintln!(
            "[tod-zone-ufrag] section 0x6200: count={count} offset=0x{:X} length={} \
             | 0x9000 length={vertex_section_length} | 0x9100 length={index_section_length}",
            section.offset, section.length
        );
    }

    let mut out: Vec<UFrag> = Vec::with_capacity(count);
    let mut skipped = 0usize;
    let mut sample_logged = 0usize;
    for i in 0..count {
        let base = u64::from(section.offset) + (i as u64) * OLD_UFRAG_SIZE;

        main_ig.stream.seek_to(base + 0x00)?;
        let tuid = main_ig.stream.read_u64()?;

        main_ig.stream.seek_to(base + 0x40)?;
        // indexOffset is a u16 COUNT (not bytes) per Zone.cs:286.
        let index_count_offset = main_ig.stream.read_u32()?;
        let vertex_offset = main_ig.stream.read_u32()?;
        let index_count = main_ig.stream.read_u16()?;
        let vertex_count = main_ig.stream.read_u16()?;

        main_ig.stream.seek_to(base + 0x50)?;
        let shader_index = main_ig.stream.read_u16()?;

        main_ig.stream.seek_to(base + 0x60)?;
        let ufrag_pos_raw = main_ig.stream.read_vec3()?;

        if vertex_count == 0 || index_count == 0 {
            skipped += 1;
            continue;
        }

        let v_byte_off = u64::from(vertex_offset);
        let v_byte_len = u64::from(vertex_count) * OLD_UFRAG_VERTEX_STRIDE as u64;
        let i_byte_off = u64::from(index_count_offset) * 2;
        let i_byte_len = u64::from(index_count) * 2;

        if v_byte_off + v_byte_len > vertex_section_length
            || i_byte_off + i_byte_len > index_section_length
        {
            eprintln!(
                "warn: TOD ufrag[{i}] geometry out of range; skipping \
                 (vert end {} > {}, idx end {} > {})",
                v_byte_off + v_byte_len,
                vertex_section_length,
                i_byte_off + i_byte_len,
                index_section_length
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
        let mut indices: Vec<u32> = Vec::with_capacity(index_count as usize);
        for _ in 0..index_count {
            indices.push(u32::from(verts_ig.stream.read_u16()?));
        }

        let mut positions: Vec<f32> = Vec::with_capacity(vertex_count as usize * 3);
        let mut uvs: Vec<f32> = Vec::with_capacity(vertex_count as usize * 2);
        for k in 0..(vertex_count as usize) {
            let v_off = k * OLD_UFRAG_VERTEX_STRIDE;
            let raw_x =
                i16::from_be_bytes([vertex_block[v_off], vertex_block[v_off + 1]]) as f32;
            let raw_y =
                i16::from_be_bytes([vertex_block[v_off + 2], vertex_block[v_off + 3]]) as f32;
            let raw_z =
                i16::from_be_bytes([vertex_block[v_off + 4], vertex_block[v_off + 5]]) as f32;
            // Local mesh coords in meters; viewport applies the
            // per-ufrag world `position` on top so we don't bake the
            // ufrag's world offset into the vertex array.
            positions.push(raw_x * UFRAG_VERTEX_SCALE);
            positions.push(raw_y * UFRAG_VERTEX_SCALE);
            positions.push(raw_z * UFRAG_VERTEX_SCALE);

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

        let position_m = [
            ufrag_pos_raw[0] * UFRAG_VERTEX_SCALE,
            ufrag_pos_raw[1] * UFRAG_VERTEX_SCALE,
            ufrag_pos_raw[2] * UFRAG_VERTEX_SCALE,
        ];

        if sample_logged < 3 && log_probes {
            eprintln!(
                "[tod-zone-ufrag] [{i}] tuid=0x{:X} pos_m=({:.2},{:.2},{:.2}) verts={vertex_count} \
                 idx={index_count} shader={shader_index}",
                tuid, position_m[0], position_m[1], position_m[2]
            );
            sample_logged += 1;
        }

        out.push(UFrag {
            tuid,
            position: position_m,
            radius: 0.0,
            vertex_count,
            index_count,
            shader_index,
            positions,
            uvs,
            indices,
        });
    }
    if skipped > 0 {
        eprintln!("[tod-zone-ufrag] skipped {skipped} empty/out-of-range ufrags");
    }
    Ok(out)
}
