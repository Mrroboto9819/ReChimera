

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use rayon::prelude::*;

use crate::assetlookup::{AssetKind, AssetLookup};
use crate::error::{Error, Result};
use crate::igfile::IgFile;
use crate::moby::{half_to_f32, read_shader_table};

const SECT_TIE_VERTICES: u32 = 0x3000;
const SECT_TIE_INDICES: u32 = 0x3200;
const SECT_TIE_HEADER: u32 = 0x3400;
const SECT_TIE_SHADER_TABLE: u32 = 0x5600;

const TIE_MESH_SIZE: u64 = 0x40;
const TIE_VERTEX_STRIDE: usize = 0x14;

#[derive(Debug, Clone)]
pub struct TieAsset {
    pub tuid: u64,
    pub scale: [f32; 3],
    pub meshes: Vec<TieMeshGeom>,

    pub shader_tuids: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct TieMeshGeom {

    pub shader_index: u16,
    pub vertex_count: u16,
    pub index_count: u16,
    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
    pub indices: Vec<u32>,
}

pub fn read_tie_assets(level_folder: &Path, tuids: Option<&[u64]>) -> Result<Vec<TieAsset>> {
    let mut out = Vec::new();
    read_tie_assets_streaming(level_folder, tuids, |tie| out.push(tie))?;
    Ok(out)
}

pub fn read_tie_assets_streaming<F>(
    level_folder: &Path,
    tuids: Option<&[u64]>,
    mut on_each: F,
) -> Result<()>
where
    F: FnMut(TieAsset),
{
    read_tie_assets_with_total(level_folder, tuids, |_| {}, |t| on_each(t))
}

pub fn read_tie_assets_with_total<T, F>(
    level_folder: &Path,
    tuids: Option<&[u64]>,
    mut on_total: T,
    mut on_each: F,
) -> Result<()>
where
    T: FnMut(usize),
    F: FnMut(TieAsset),
{
    let assetlookup_path = level_folder.join("assetlookup.dat");
    let mut lookup = AssetLookup::open(BufReader::new(File::open(&assetlookup_path)?))?;
    let ptrs = lookup.pointers(AssetKind::Tie)?;

    let filtered: Vec<_> = ptrs
        .into_iter()
        .filter(|p| tuids.map_or(true, |allowed| allowed.contains(&p.tuid)))
        .collect();
    on_total(filtered.len());
    if filtered.is_empty() {
        return Ok(());
    }

    let ties_path = level_folder.join("ties.dat");
    let mut ties_file = File::open(&ties_path)?;

    const BUFFER_AHEAD: usize = 16;

    let total = filtered.len();
    let mut next_index = 0usize;
    while next_index < total {
        let end = (next_index + BUFFER_AHEAD).min(total);
        let slice = &filtered[next_index..end];

        let mut bufs: Vec<(usize, Vec<u8>, u64)> = Vec::with_capacity(slice.len());
        for (i, ptr) in slice.iter().enumerate() {
            if ptr.length > crate::MAX_ASSET_SIZE {
                return Err(Error::AllocLimitExceeded {
                    size: u64::from(ptr.length),
                    limit: u64::from(crate::MAX_ASSET_SIZE),
                });
            }
            ties_file.seek(SeekFrom::Start(u64::from(ptr.offset)))?;
            let mut buf = vec![0u8; ptr.length as usize];
            ties_file.read_exact(&mut buf)?;
            bufs.push((next_index + i, buf, ptr.tuid));
        }

        let mut parsed: Vec<(usize, u64, Result<TieAsset>)> = bufs
            .into_par_iter()
            .map(|(idx, buf, tuid)| {
                let parsed_or_err = (|| {
                    let mut tie_ig = IgFile::open(Cursor::new(buf))?;
                    parse_tie(&mut tie_ig, tuid)
                })();
                (idx, tuid, parsed_or_err)
            })
            .collect();

        parsed.sort_by_key(|(i, _, _)| *i);

        for (_i, tuid, result) in parsed {
            match result {
                Ok(tie) => on_each(tie),
                Err(e) => {
                    eprintln!("warn: tie 0x{:016X} skipped: {e}", tuid);
                }
            }
        }

        next_index = end;
    }
    Ok(())
}

fn parse_tie<R: Read + Seek>(ig: &mut IgFile<R>, tuid_hint: u64) -> Result<TieAsset> {
    let header_section = ig.require_section(SECT_TIE_HEADER)?;
    let header_off = u64::from(header_section.offset);

    ig.stream.seek_to(header_off + 0x00)?;
    let meshes_ptr = u64::from(ig.stream.read_u32()?);

    ig.stream.seek_to(header_off + 0x0F)?;
    let mesh_count = ig.stream.read_u8()? as usize;

    ig.stream.seek_to(header_off + 0x14)?;
    let vertex_buffer_start = ig.stream.read_u32()?;
    let vertex_buffer_size = ig.stream.read_u32()?;

    ig.stream.seek_to(header_off + 0x20)?;
    let scale = ig.stream.read_vec3()?;

    ig.stream.seek_to(header_off + 0x68)?;
    let header_tuid = ig.stream.read_u64()?;
    let tuid = if header_tuid != 0 {
        header_tuid
    } else {
        tuid_hint
    };

    let vsect = ig.require_section(SECT_TIE_VERTICES)?;
    ig.stream
        .seek_to(u64::from(vsect.offset) + u64::from(vertex_buffer_start))?;
    let vertex_buf = ig.stream.read_bytes(vertex_buffer_size as usize)?;

    let isect = ig.require_section(SECT_TIE_INDICES)?;
    ig.stream.seek_to(u64::from(isect.offset))?;
    let index_buf = ig.stream.read_bytes(isect.length as usize)?;

    let shader_count = ig
        .section(SECT_TIE_SHADER_TABLE)
        .map(|s| s.count as u16)
        .unwrap_or(0);
    let mut meshes = Vec::with_capacity(mesh_count);
    for m in 0..mesh_count {
        let mesh_base = meshes_ptr + (m as u64) * TIE_MESH_SIZE;
        ig.stream.seek_to(mesh_base + 0x00)?;
        let index_index = ig.stream.read_u32()?;
        ig.stream.seek_to(mesh_base + 0x04)?;
        let vertex_index = ig.stream.read_u16()?;

        ig.stream.seek_to(mesh_base + 0x08)?;
        let vertex_count = ig.stream.read_u16()?;
        ig.stream.seek_to(mesh_base + 0x12)?;
        let index_count = ig.stream.read_u16()?;

        ig.stream.seek_to(mesh_base + 0x28)?;
        let si_v2 = ig.stream.read_u16()?;
        let shader_index = if shader_count > 0 && si_v2 < shader_count {
            si_v2
        } else {
            ig.stream.seek_to(mesh_base + 0x0C)?;
            let si_acit = ig.stream.read_u16()?;
            if shader_count > 0 && si_acit < shader_count {
                si_acit
            } else {
                si_v2
            }
        };

        if m == 0 && std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static TIE_MESH_DUMP_FIRED: AtomicUsize = AtomicUsize::new(0);
            let n = TIE_MESH_DUMP_FIRED.fetch_add(1, Ordering::Relaxed);
            if n < 3 {
                ig.stream.seek_to(mesh_base)?;
                let mut bytes = [0u8; 0x40];
                for b in bytes.iter_mut() {
                    *b = ig.stream.read_u8().unwrap_or(0);
                }
                let mut hex = String::new();
                for (i, b) in bytes.iter().enumerate() {
                    if i % 8 == 0 && i > 0 {
                        hex.push(' ');
                    }
                    hex.push_str(&format!("{:02X} ", b));
                }
                let mut u16s = String::new();
                for i in 0..(0x40 / 2) {
                    let v = u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
                    u16s.push_str(&format!("[+0x{:02X}]={:5} ", i * 2, v));
                    if (i + 1) % 4 == 0 {
                        u16s.push('\n');
                        u16s.push_str("                  ");
                    }
                }
                let shader_table_count = ig
                    .section(SECT_TIE_SHADER_TABLE)
                    .map(|s| s.count as usize)
                    .unwrap_or(0);
                eprintln!(
                    "[tie-mesh-probe] tie 0x{:016X} mesh[0] (TIE_MESH_SIZE=0x{:X}) 0x5600.count={} read_shader_index@+0x28={}\n  raw: {}\n  u16 BE: {}",
                    tuid, TIE_MESH_SIZE, shader_table_count, shader_index, hex, u16s
                );
            }
        }

        meshes.push(decode_tie_mesh(
            &index_buf,
            &vertex_buf,
            index_index,
            index_count,
            vertex_index,
            vertex_count,
            scale,
            shader_index,
        )?);
    }

    let shader_tuids = read_shader_table(ig, SECT_TIE_SHADER_TABLE)?;

    if shader_tuids.is_empty() {
        use std::sync::Mutex;
        static SEEN_LAYOUTS: Mutex<Option<std::collections::HashSet<u64>>> = Mutex::new(None);
        let mut sorted_ids: Vec<u32> = ig.sections.iter().map(|s| s.id).collect();
        sorted_ids.sort_unstable();
        let mut hasher: u64 = 0xCBF29CE484222325;
        for id in &sorted_ids {
            hasher ^= u64::from(*id);
            hasher = hasher.wrapping_mul(0x100000001B3);
        }
        let mut guard = SEEN_LAYOUTS.lock().unwrap();
        let set = guard.get_or_insert_with(std::collections::HashSet::new);
        if set.insert(hasher) && set.len() <= 4 {
            let summary: Vec<String> = ig.sections
                .iter()
                .map(|s| format!("0x{:X}(len={},cnt={})", s.id, s.length, s.count))
                .collect();
            eprintln!(
                "warn: tie 0x{:016X} has no shader table at 0x{:X} — meshes will render gray. sections: [{}]",
                tuid, SECT_TIE_SHADER_TABLE, summary.join(", ")
            );
        }
    }

    Ok(TieAsset {
        tuid,
        scale,
        meshes,
        shader_tuids,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_tie_mesh(
    index_buf: &[u8],
    vertex_buf: &[u8],
    index_index: u32,
    index_count: u16,
    vertex_index: u16,
    vertex_count: u16,
    scale: [f32; 3],
    shader_index: u16,
) -> Result<TieMeshGeom> {
    let i_byte_off = (index_index as usize) * 2;
    let i_byte_end = i_byte_off + (index_count as usize) * 2;
    if i_byte_end > index_buf.len() {
        return Err(Error::SectionLengthMismatch {
            id: SECT_TIE_INDICES,
            length: index_buf.len() as u32,
            entry: 2,
        });
    }
    let mut indices = Vec::with_capacity(index_count as usize);
    for k in 0..(index_count as usize) {
        let off = i_byte_off + k * 2;
        let v = u16::from_be_bytes([index_buf[off], index_buf[off + 1]]);
        indices.push(u32::from(v));
    }

    let v_byte_off = (vertex_index as usize) * TIE_VERTEX_STRIDE;
    let v_total = (vertex_count as usize) * TIE_VERTEX_STRIDE;
    if v_byte_off + v_total > vertex_buf.len() {
        return Err(Error::SectionLengthMismatch {
            id: SECT_TIE_VERTICES,
            length: vertex_buf.len() as u32,
            entry: TIE_VERTEX_STRIDE as u32,
        });
    }

    let mut positions = Vec::with_capacity((vertex_count as usize) * 3);
    let mut uvs = Vec::with_capacity((vertex_count as usize) * 2);
    for k in 0..(vertex_count as usize) {
        let base = v_byte_off + k * TIE_VERTEX_STRIDE;
        let x = i16::from_be_bytes([vertex_buf[base + 0], vertex_buf[base + 1]]) as f32;
        let y = i16::from_be_bytes([vertex_buf[base + 2], vertex_buf[base + 3]]) as f32;
        let z = i16::from_be_bytes([vertex_buf[base + 4], vertex_buf[base + 5]]) as f32;
        positions.push(x * scale[0]);
        positions.push(y * scale[1]);
        positions.push(z * scale[2]);

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

    Ok(TieMeshGeom {
        shader_index,
        vertex_count,
        index_count,
        positions,
        uvs,
        indices,
    })
}
