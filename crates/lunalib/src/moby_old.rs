use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::moby::{decode_moby_mesh, MobyAsset, MobyBangle, MobyMesh};
use crate::skeleton::read_skeleton_at;

static TOD_BONE_WEIGHT_PROBE_FIRED: AtomicBool = AtomicBool::new(false);

const OLD_MOBY_HEADER_SIZE: u64 = 0xC0;
const OLD_MOBY_MESH_SIZE: u64 = 0x40;
const OLD_MOBY_BANGLE_SIZE: u64 = 0x08;

const SECT_MOBY: u32 = 0xD100;
const SECT_LEVEL_VERTEX_BUFFER: u32 = 0x9000;
const SECT_LEVEL_INDEX_BUFFER: u32 = 0x9100;

const FLAG_USE_VERTICES_DAT: u32 = 0x8000_0000;

pub fn read_moby_assets_old<F>(level_folder: &Path, mut on_each: F) -> Result<()>
where
    F: FnMut(MobyAsset),
{
    read_moby_assets_old_with_total(level_folder, |_| {}, |a| on_each(a))
}

pub fn read_moby_assets_old_with_total<T, F>(
    level_folder: &Path,
    mut on_total: T,
    mut on_each: F,
) -> Result<()>
where
    T: FnMut(usize),
    F: FnMut(MobyAsset),
{
    let main_path = level_folder.join("main.dat");
    let main_bytes = std::fs::read(&main_path)?;
    let main_ig = IgFile::open(Cursor::new(main_bytes.as_slice()))?;

    let global_shader_count = main_ig.section(0x5000).map(|s| s.count).unwrap_or(0) as usize;
    let identity_shader_tuids: Vec<u64> = (0..global_shader_count as u64).collect();

    let moby_section = match main_ig.section(SECT_MOBY) {
        Some(s) => s,
        None => return Ok(()),
    };

    if moby_section.length != OLD_MOBY_HEADER_SIZE as u32 {
        eprintln!(
            "warn: main.dat 0xD100 section length is {} (expected {} for TOD OldMoby) — \
             skipping. The file may be V2-format mis-detected as TOD.",
            moby_section.length, OLD_MOBY_HEADER_SIZE
        );
        return Ok(());
    }

    let vertices_path = level_folder.join("vertices.dat");
    let vertices_ig =
        IgFile::open(BufReader::new(File::open(&vertices_path).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vertices.dat is required for TOD layout: {e}"),
            ))
        })?))?;
    let vertices_dat_path = vertices_path.clone();
    let textures_dat_path = level_folder.join("textures.dat");

    let vertex_section = vertices_ig.section(SECT_LEVEL_VERTEX_BUFFER);
    let index_section = vertices_ig.section(SECT_LEVEL_INDEX_BUFFER);
    let vertices_geom_offset = vertex_section.map(|s| u64::from(s.offset));
    let indices_geom_offset = index_section.map(|s| u64::from(s.offset));

    let log_probes = std::env::var("RECHIMERA_LOG_PROBES").is_ok();
    let count = moby_section.count as usize;
    on_total(count);
    if log_probes {
        eprintln!(
            "[tod-moby] section 0xD100: count={} offset=0x{:X} length={} (stride 0x{:X})",
            count, moby_section.offset, moby_section.length, OLD_MOBY_HEADER_SIZE
        );
        if let Some(s) = vertex_section {
            eprintln!(
                "[tod-moby] vertices.dat 0x9000: offset=0x{:X} length={} bytes",
                s.offset, s.length
            );
        }
        if let Some(s) = index_section {
            eprintln!(
                "[tod-moby] vertices.dat 0x9100: offset=0x{:X} length={} bytes",
                s.offset, s.length
            );
        }
    }
    let section_offset = u64::from(moby_section.offset);
    let mut parsed: Vec<(usize, Result<MobyAsset>)> = (0..count)
        .into_par_iter()
        .map(|i| {
            let base = section_offset + (i as u64) * OLD_MOBY_HEADER_SIZE;
            let result = (|| {
                let mut ig = IgFile::open(Cursor::new(main_bytes.as_slice()))?;
                parse_one(
                    &mut ig,
                    base,
                    i,
                    &vertices_dat_path,
                    &textures_dat_path,
                    vertices_geom_offset,
                    indices_geom_offset,
                    &identity_shader_tuids,
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
                eprintln!("warn: TOD moby[{i}] skipped — parse failed: {e}");
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_one<R: Read + Seek>(
    main_ig: &mut IgFile<R>,
    base: u64,
    index: usize,
    vertices_dat_path: &Path,
    textures_dat_path: &Path,
    vertices_geom_offset: Option<u64>,
    indices_geom_offset: Option<u64>,
    identity_shader_tuids: &[u64],
    log_probes: bool,
) -> Result<MobyAsset> {
    main_ig.stream.seek_to(base + 0x00)?;
    let bsphere_position = main_ig.stream.read_vec3()?;
    main_ig.stream.seek_to(base + 0x0C)?;
    let bsphere_radius = main_ig.stream.read_f32()?;

    main_ig.stream.seek_to(base + 0x16)?;
    let num_animations = main_ig.stream.read_u16()?;

    main_ig.stream.seek_to(base + 0x18)?;
    let bangle_count1 = main_ig.stream.read_u16()?;
    let _bangle_count2 = main_ig.stream.read_u16()?;

    main_ig.stream.seek_to(base + 0x20)?;
    let skeleton_ptr = u64::from(main_ig.stream.read_u32()?);
    let animations_ptr = u64::from(main_ig.stream.read_u32()?);

    main_ig.stream.seek_to(base + 0x28)?;
    let bangles_ptr = u64::from(main_ig.stream.read_u32()?);

    main_ig.stream.seek_to(base + 0x34)?;
    let index_offset_raw = main_ig.stream.read_u32()?;
    let vertex_offset_raw = main_ig.stream.read_u32()?;
    let scale = main_ig.stream.read_f32()?;

    let skeleton = if skeleton_ptr == 0 {
        None
    } else {
        read_skeleton_at(main_ig, skeleton_ptr).unwrap_or(None)
    };

    let index_use_vertices_dat = (index_offset_raw & FLAG_USE_VERTICES_DAT) != 0;
    let vertex_use_vertices_dat = (vertex_offset_raw & FLAG_USE_VERTICES_DAT) != 0;
    let index_offset = u64::from(index_offset_raw & !FLAG_USE_VERTICES_DAT);
    let vertex_offset = u64::from(vertex_offset_raw & !FLAG_USE_VERTICES_DAT);

    if log_probes && index < 5 {
        eprintln!(
            "[tod-moby] moby[{}] base=0x{:X} bsphere=({:.3},{:.3},{:.3} r={:.3}) bangles={} \
             num_anims={} skel_ptr=0x{:X} anims_ptr=0x{:X} bangles_ptr=0x{:X} \
             idx_off=0x{:X}{} v_off=0x{:X}{} scale={:.6}",
            index,
            base,
            bsphere_position[0],
            bsphere_position[1],
            bsphere_position[2],
            bsphere_radius,
            bangle_count1,
            num_animations,
            skeleton_ptr,
            animations_ptr,
            bangles_ptr,
            index_offset,
            if index_use_vertices_dat { " (vertices.dat)" } else { " (textures.dat)" },
            vertex_offset,
            if vertex_use_vertices_dat { " (vertices.dat)" } else { " (textures.dat)" },
            scale,
        );
    }

    let mut bangle_descriptors: Vec<(u64, u32)> = Vec::with_capacity(bangle_count1 as usize);
    for b in 0..bangle_count1 {
        let bangle_base = bangles_ptr + (b as u64) * OLD_MOBY_BANGLE_SIZE;
        main_ig.stream.seek_to(bangle_base + 0x00)?;
        let meshes_ptr = u64::from(main_ig.stream.read_u32()?);
        let mesh_count = main_ig.stream.read_u32()?;
        if log_probes && index < 5 {
            eprintln!(
                "[tod-moby]   bangle[{}] meshes_ptr=0x{:X} mesh_count={}",
                b, meshes_ptr, mesh_count
            );
        }
        bangle_descriptors.push((meshes_ptr, mesh_count));
    }

    let mut max_vertex_end: u64 = 0;
    let mut max_index_end: u64 = 0;
    let mut all_meshes: Vec<MeshHeader> = Vec::new();

    for (meshes_ptr, mesh_count) in &bangle_descriptors {
        let mut bangle_meshes: Vec<MeshHeader> = Vec::with_capacity(*mesh_count as usize);
        for m in 0..*mesh_count {
            let mesh_base = meshes_ptr + (m as u64) * OLD_MOBY_MESH_SIZE;
            main_ig.stream.seek_to(mesh_base + 0x00)?;
            let index_index = main_ig.stream.read_u32()?;
            let mesh_vertex_offset = main_ig.stream.read_u32()?;
            let shader_index = main_ig.stream.read_u16()?;
            let vertex_count = main_ig.stream.read_u16()?;

            main_ig.stream.seek_to(mesh_base + 0x0C)?;
            let bone_map_count = main_ig.stream.read_u8()?;
            let vertex_type = main_ig.stream.read_u8()?;
            let _bone_map_index = main_ig.stream.read_u8()?;

            main_ig.stream.seek_to(mesh_base + 0x12)?;
            let index_count = main_ig.stream.read_u16()?;

            main_ig.stream.seek_to(mesh_base + 0x20)?;
            let bone_map_ptr = u64::from(main_ig.stream.read_u32()?);

            let mut bone_map: Vec<u16> = Vec::with_capacity(bone_map_count as usize);
            if bone_map_ptr != 0 && bone_map_count > 0 {
                main_ig.stream.seek_to(bone_map_ptr)?;
                for _ in 0..bone_map_count {
                    bone_map.push(main_ig.stream.read_u16()?);
                }
            }

            let stride: u32 = if vertex_type == 1 { 0x1C } else { 0x14 };
            let vertex_end = (mesh_vertex_offset as u64) + (vertex_count as u64) * (stride as u64);
            let index_end = ((index_index as u64) + (index_count as u64)) * 2;
            if vertex_end > max_vertex_end {
                max_vertex_end = vertex_end;
            }
            if index_end > max_index_end {
                max_index_end = index_end;
            }
            if log_probes && index < 5 {
                eprintln!(
                    "[tod-moby]     mesh[{m}] idx_idx={} v_off=0x{:X} shader=0x{:X} v_cnt={} \
                     v_type={} stride=0x{:X} bone_map_count={} idx_cnt={} bone_map_ptr=0x{:X}",
                    index_index,
                    mesh_vertex_offset,
                    shader_index,
                    vertex_count,
                    vertex_type,
                    stride,
                    bone_map_count,
                    index_count,
                    bone_map_ptr,
                );
            }
            bangle_meshes.push(MeshHeader {
                index_index,
                vertex_offset: mesh_vertex_offset,
                shader_index,
                vertex_count,
                vertex_type,
                index_count,
                bone_map,
            });
        }
        all_meshes.extend(bangle_meshes);
    }

    let vertex_buf = read_buffer_slice(
        vertex_use_vertices_dat,
        vertices_dat_path,
        textures_dat_path,
        vertices_geom_offset,
        vertex_offset,
        max_vertex_end,
    )?;
    let index_buf = read_buffer_slice(
        index_use_vertices_dat,
        vertices_dat_path,
        textures_dat_path,
        indices_geom_offset,
        index_offset,
        max_index_end,
    )?;

    let mut bangles: Vec<MobyBangle> = Vec::with_capacity(bangle_descriptors.len());
    let mut mesh_cursor = 0usize;
    for (_, mesh_count) in &bangle_descriptors {
        let mut decoded: Vec<MobyMesh> = Vec::with_capacity(*mesh_count as usize);
        for _ in 0..*mesh_count {
            let h = &all_meshes[mesh_cursor];
            mesh_cursor += 1;
            let stride: u32 = if h.vertex_type == 1 { 0x1C } else { 0x14 };
            let mesh = decode_moby_mesh(
                &index_buf,
                &vertex_buf,
                h.index_index,
                h.index_count,
                h.vertex_offset,
                h.vertex_count,
                stride,
                h.shader_index,
                scale,
                &h.bone_map,
            )?;
            decoded.push(mesh);
        }
        bangles.push(MobyBangle { meshes: decoded });
    }

    let mut anim_offsets: Vec<u64> = Vec::new();
    if animations_ptr != 0 && num_animations > 0 {
        for a in 0..num_animations {
            let slot_off = animations_ptr + (a as u64) * 4;
            if main_ig.stream.seek_to(slot_off).is_ok() {
                if let Ok(off) = main_ig.stream.read_u32() {
                    if off != 0 {
                        anim_offsets.push(u64::from(off));
                    }
                }
            }
        }
    }

    if std::env::var("RECHIMERA_LOG_PROBES").is_ok()
        && skeleton.is_some()
        && !bangles.is_empty()
        && !bangles[0].meshes.is_empty()
        && TOD_BONE_WEIGHT_PROBE_FIRED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    {
        let skel = skeleton.as_ref().unwrap();
        let mesh = &bangles[0].meshes[0];
        eprintln!(
            "[tod-bone-probe] moby_{:04X}: skeleton.bones.len()={} translation_shift={} scale_shift={}",
            index, skel.bones.len(), skel.translation_shift, skel.scale_shift,
        );
        for b in 0..skel.bones.len().min(8) {
            let bone = &skel.bones[b];
            let parent = if bone.parent_index == b as i16 {
                "ROOT".to_string()
            } else {
                format!("{}", bone.parent_index)
            };
            let t = skel.bind_local.get(b).map(|m| [m[12], m[13], m[14]]).unwrap_or([0.0; 3]);
            eprintln!(
                "[tod-bone-probe]   bone[{}] parent={} flags=0x{:04X} bind_local.t=({:.4},{:.4},{:.4})",
                b, parent, bone.flags, t[0], t[1], t[2]
            );
        }
        eprintln!(
            "[tod-weight-probe] moby_{:04X} bangle[0].mesh[0]: v_cnt={} stride={} skinned={}",
            index,
            mesh.vertex_count,
            mesh.vertex_stride,
            mesh.vertex_stride == 0x1C
        );
        for v in 0..(mesh.vertex_count as usize).min(8) {
            let bi: [u16; 4] = [
                mesh.bone_indices.get(v * 4).copied().unwrap_or(0),
                mesh.bone_indices.get(v * 4 + 1).copied().unwrap_or(0),
                mesh.bone_indices.get(v * 4 + 2).copied().unwrap_or(0),
                mesh.bone_indices.get(v * 4 + 3).copied().unwrap_or(0),
            ];
            let bw: [u8; 4] = [
                mesh.bone_weights.get(v * 4).copied().unwrap_or(0),
                mesh.bone_weights.get(v * 4 + 1).copied().unwrap_or(0),
                mesh.bone_weights.get(v * 4 + 2).copied().unwrap_or(0),
                mesh.bone_weights.get(v * 4 + 3).copied().unwrap_or(0),
            ];
            let wsum: u32 = bw.iter().map(|&w| w as u32).sum();
            eprintln!(
                "[tod-weight-probe]   v[{}] bones={:?} weights={:?} sum={} (255 = normalized)",
                v, bi, bw, wsum,
            );
        }
        eprintln!(
            "[tod-weight-probe] moby_{:04X}: max_bone_index in mesh[0] = {} (skel has {} bones)",
            index,
            mesh.bone_indices.iter().copied().max().unwrap_or(0),
            skel.bones.len(),
        );
    }

    Ok(MobyAsset {
        tuid: index as u64,
        name: format!("moby_{:04X}", index),
        bangles,
        bsphere_position,
        bsphere_radius,
        shader_tuids: identity_shader_tuids.to_vec(),
        skeleton,
        animset_hash: None,
        bind_pose_inverse_offset: 0,
        rfom_anim_offsets: anim_offsets,
    })
}

struct MeshHeader {
    index_index: u32,
    vertex_offset: u32,
    shader_index: u16,
    vertex_count: u16,
    vertex_type: u8,
    index_count: u16,
    bone_map: Vec<u16>,
}

fn read_buffer_slice(
    use_vertices_dat: bool,
    vertices_dat_path: &Path,
    textures_dat_path: &Path,
    vertices_geom_offset: Option<u64>,
    relative_offset: u64,
    length: u64,
) -> Result<Vec<u8>> {
    if length == 0 {
        return Ok(Vec::new());
    }
    let (path, base_offset) = if use_vertices_dat {
        let geom = vertices_geom_offset.ok_or(Error::SectionNotFound(SECT_LEVEL_VERTEX_BUFFER))?;
        (vertices_dat_path, geom + relative_offset)
    } else {
        (textures_dat_path, relative_offset)
    };

    let mut file = File::open(path).map_err(|e| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("opening {}: {e}", path.display()),
        ))
    })?;
    file.seek(SeekFrom::Start(base_offset))?;
    let mut buf = vec![0u8; length as usize];
    file.read_exact(&mut buf)?;

    Ok(buf)
}
