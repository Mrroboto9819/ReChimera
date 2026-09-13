use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::moby::{decode_moby_mesh, MobyAsset, MobyBangle};
use crate::skeleton::read_skeleton_at;

const SECT_MOBY: u32 = 0xD100;
const RFOM_MOBY_HEADER_SIZE: u64 = 0xC0;
const RFOM_PRIMITIVE_SIZE: u64 = 0x20;
const RFOM_BANGLE_SIZE: u64 = 0x08;

const SECT_MATERIAL_V1: u32 = 0x5001;

pub fn read_moby_assets_rfom<F>(level_folder: &Path, mut on_each: F) -> Result<()>
where
    F: FnMut(MobyAsset),
{
    read_moby_assets_rfom_with_total(level_folder, |_| {}, |a| on_each(a))
}

pub fn read_moby_assets_rfom_with_total<T, F>(
    level_folder: &Path,
    mut on_total: T,
    mut on_each: F,
) -> Result<()>
where
    T: FnMut(usize),
    F: FnMut(MobyAsset),
{
    let main_path = level_folder.join("ps3levelmain.dat");
    let main_bytes = std::fs::read(&main_path)?;
    let main_ig = IgFile::open(Cursor::new(main_bytes.as_slice()))?;

    let global_material_count = main_ig
        .section(SECT_MATERIAL_V1)
        .map(|s| s.count)
        .unwrap_or(0) as usize;
    let identity_shader_tuids: Vec<u64> = (0..global_material_count as u64).collect();

    let moby_section = match main_ig.section(SECT_MOBY) {
        Some(s) => s,
        None => return Ok(()),
    };
    if moby_section.length != RFOM_MOBY_HEADER_SIZE as u32 {
        eprintln!(
            "warn: ps3levelmain.dat 0xD100 length is {} (expected {} for MobyV1) — skipping",
            moby_section.length, RFOM_MOBY_HEADER_SIZE
        );
        return Ok(());
    }

    let texs_path = level_folder.join("ps3leveltexs.dat");
    if !texs_path.exists() {
        eprintln!(
            "warn: {} missing — RFOM moby geometry lives in ps3leveltexs.dat per IT levelmain/extract.cpp:1366",
            texs_path.display()
        );
        return Ok(());
    }
    let texs_size = std::fs::metadata(&texs_path).map(|m| m.len()).unwrap_or(0);

    let count = moby_section.count as usize;
    on_total(count);
    let section_offset = u64::from(moby_section.offset);
    let mut parsed: Vec<(usize, Result<Option<MobyAsset>>)> = (0..count)
        .into_par_iter()
        .map(|i| {
            let header_off = section_offset + (i as u64) * RFOM_MOBY_HEADER_SIZE;
            let result = (|| {
                let mut ig = IgFile::open(Cursor::new(main_bytes.as_slice()))?;
                parse_one(
                    &mut ig,
                    header_off,
                    i,
                    &texs_path,
                    texs_size,
                    &identity_shader_tuids,
                )
            })();
            (i, result)
        })
        .collect();
    parsed.sort_by_key(|(i, _)| *i);
    let mut skipped_empty = 0usize;
    for (i, result) in parsed {
        match result {
            Ok(Some(asset)) => on_each(asset),
            Ok(None) => skipped_empty += 1,
            Err(e) => {
                eprintln!("warn: RFOM moby[{i}] skipped — parse failed: {e}");
            }
        }
    }
    if skipped_empty > 0 {
        eprintln!("[rfom] skipped {skipped_empty} mobys with zero meshes / null mesh pointer");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_one<R: Read + Seek>(
    main_ig: &mut IgFile<R>,
    base: u64,
    index: usize,
    texs_path: &Path,
    texs_size: u64,
    identity_shader_tuids: &[u64],
) -> Result<Option<MobyAsset>> {
    main_ig.stream.seek_to(base + 0x00)?;
    let bsphere_position = main_ig.stream.read_vec3()?;
    main_ig.stream.seek_to(base + 0x0C)?;
    let bsphere_radius = main_ig.stream.read_f32()?;

    main_ig.stream.seek_to(base + 0x16)?;
    let num_animations = main_ig.stream.read_u16()?;

    main_ig.stream.seek_to(base + 0x18)?;
    let bangle_count = main_ig.stream.read_u16()?;

    main_ig.stream.seek_to(base + 0x1A)?;
    let moby_id = main_ig.stream.read_u16()?;

    main_ig.stream.seek_to(base + 0x20)?;
    let skeleton_ptr = u64::from(main_ig.stream.read_u32()?);
    let animations_ptr = u64::from(main_ig.stream.read_u32()?);

    main_ig.stream.seek_to(base + 0x28)?;
    let bangles_ptr = u64::from(main_ig.stream.read_u32()?);

    main_ig.stream.seek_to(base + 0x34)?;
    let index_buffer_offset = u64::from(main_ig.stream.read_u32()?);
    let vertex_buffer_offset = u64::from(main_ig.stream.read_u32()?);
    let scale = main_ig.stream.read_f32()?;

    let skeleton = if skeleton_ptr == 0 {
        None
    } else {
        read_skeleton_at(main_ig, skeleton_ptr).unwrap_or(None)
    };

    let mut rfom_anim_offsets: Vec<u64> = Vec::new();
    if animations_ptr != 0 && num_animations > 0 {
        for a in 0..num_animations {
            let slot_off = animations_ptr + (a as u64) * 4;
            if main_ig.stream.seek_to(slot_off).is_ok() {
                if let Ok(off) = main_ig.stream.read_u32() {
                    if off != 0 {
                        rfom_anim_offsets.push(u64::from(off));
                    }
                }
            }
        }
    }

    if bangle_count == 0 || bangles_ptr == 0 {
        return Ok(None);
    }

    let mut bangle_descriptors: Vec<(u64, u32)> = Vec::with_capacity(bangle_count as usize);
    for b in 0..bangle_count {
        let bangle_base = bangles_ptr + (b as u64) * RFOM_BANGLE_SIZE;
        main_ig.stream.seek_to(bangle_base + 0x00)?;
        let primitives_ptr = u64::from(main_ig.stream.read_u32()?);
        let primitive_count = main_ig.stream.read_u32()?;
        bangle_descriptors.push((primitives_ptr, primitive_count));
    }

    let mut all_meshes: Vec<MeshHeader> = Vec::new();
    let mut have_geom = false;
    for (bangle_idx, (primitives_ptr, primitive_count)) in bangle_descriptors.iter().enumerate() {
        if *primitives_ptr == 0 || *primitive_count == 0 {
            continue;
        }
        for p in 0..*primitive_count {
            let prim_base = primitives_ptr + (p as u64) * RFOM_PRIMITIVE_SIZE;

            main_ig.stream.seek_to(prim_base + 0x00)?;
            let material_index = main_ig.stream.read_u16()?;
            let vertex_count = main_ig.stream.read_u16()?;
            let index_count = main_ig.stream.read_u16()?;

            main_ig.stream.seek_to(prim_base + 0x06)?;
            let num_joints = main_ig.stream.read_u8()?;
            let vertex_format = main_ig.stream.read_u8()?;

            main_ig.stream.seek_to(prim_base + 0x08)?;
            let prim_index_offset = main_ig.stream.read_u32()?;
            let prim_vertex_offset = main_ig.stream.read_u32()?;

            main_ig.stream.seek_to(prim_base + 0x10)?;
            let joints_ptr = u64::from(main_ig.stream.read_u32()?);

            let mut bone_map: Vec<u16> = Vec::with_capacity(num_joints as usize);
            if joints_ptr != 0 && num_joints > 0 {
                main_ig.stream.seek_to(joints_ptr)?;
                for _ in 0..num_joints {
                    bone_map.push(main_ig.stream.read_u16()?);
                }
            }

            if vertex_count > 0 && index_count > 0 {
                have_geom = true;
            }
            all_meshes.push(MeshHeader {
                bangle_idx,
                index_index: prim_index_offset,
                vertex_offset: prim_vertex_offset,
                shader_index: material_index,
                vertex_count,
                vertex_type: vertex_format,
                index_count,
                bone_map,
            });
        }
    }
    if !have_geom {
        return Ok(None);
    }

    let mut texs = File::open(texs_path).map_err(|e| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("opening {}: {e}", texs_path.display()),
        ))
    })?;

    let mut bangles: Vec<MobyBangle> = (0..bangle_descriptors.len())
        .map(|_| MobyBangle { meshes: Vec::new() })
        .collect();

    for h in &all_meshes {
        if h.vertex_count == 0 || h.index_count == 0 {
            continue;
        }
        let stride: u32 = if h.vertex_type == 1 { 0x1C } else { 0x14 };
        let v_start = vertex_buffer_offset + u64::from(h.vertex_offset);
        let v_len = u64::from(h.vertex_count) * u64::from(stride);
        let i_start = index_buffer_offset + u64::from(h.index_index) * 2;
        let i_len = u64::from(h.index_count) * 2;

        if v_start + v_len > texs_size || i_start + i_len > texs_size {
            eprintln!(
                "[rfom] moby[{index}] mesh skipped — geometry extends past ps3leveltexs.dat \
                 (file size {}, vertex end {}, index end {})",
                texs_size,
                v_start + v_len,
                i_start + i_len
            );
            continue;
        }

        let vertex_buf = read_at(&mut texs, v_start, v_len as usize)?;
        let index_buf = read_at(&mut texs, i_start, i_len as usize)?;

        let mesh = decode_moby_mesh(
            &index_buf,
            &vertex_buf,
            0,
            h.index_count,
            0,
            h.vertex_count,
            stride,
            h.shader_index,
            scale,
            &h.bone_map,
        )?;
        bangles[h.bangle_idx].meshes.push(mesh);
    }

    bangles.retain(|b| !b.meshes.is_empty());
    if bangles.is_empty() {
        return Ok(None);
    }

    Ok(Some(MobyAsset {
        tuid: u64::from(moby_id),
        name: format!("moby_{:04X}", moby_id),
        bangles,
        bsphere_position,
        bsphere_radius,
        shader_tuids: identity_shader_tuids.to_vec(),
        skeleton,
        animset_hash: None,
        bind_pose_inverse_offset: 0,
        rfom_anim_offsets,
    }))
}

struct MeshHeader {
    bangle_idx: usize,
    index_index: u32,
    vertex_offset: u32,
    shader_index: u16,
    vertex_count: u16,
    vertex_type: u8,
    index_count: u16,
    bone_map: Vec<u16>,
}

fn read_at(file: &mut File, offset: u64, length: usize) -> Result<Vec<u8>> {
    if length == 0 {
        return Ok(Vec::new());
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; length];
    file.read_exact(&mut buf)?;
    Ok(buf)
}
