use std::collections::HashMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use serde::Serialize;

use crate::assetlookup::{AssetKind, AssetLookup, AssetPointer};
use crate::cubemap::{read_cubemaps, CubemapFace};
use crate::error::{Error, Result};
use crate::gltf_export::write_moby_glb_full;
use crate::level_layout::{detect_layout, LevelLayout};
use crate::moby::{read_moby_assets_streaming, MobyAsset, MobyBangle, MobyMesh};
use crate::outfitter_names::{load_from_assetlookup as load_outfitter_names, OutfitterNames};
use crate::shader::{read_shaders, ShaderInfo};
use crate::texture::{bulk_extract_pngs, encode_png};
use crate::tie::{read_tie_assets_streaming, TieAsset};
use crate::zone::{read_zones_streaming, Zone};

#[derive(Debug, Clone)]
pub struct ExtractOptions {
    pub kinds: Vec<AssetKind>,
    pub max_texture_dim: u32,
}

pub enum ExtractEvent {
    Total { kind: AssetKind, count: usize },
    Entry { kind: AssetKind, index: usize, tuid: u64, ok: bool, message: Option<String> },
    KindDone { kind: AssetKind },
}

#[derive(Serialize)]
pub struct AssetLookupOverview {
    pub layout: &'static str,
    pub version_major: u16,
    pub version_minor: u16,
    pub kinds: Vec<KindOverview>,
}

#[derive(Serialize)]
pub struct KindOverview {
    pub name: &'static str,
    pub section_id: u32,
    pub count: usize,
    pub has_decoder: bool,
}

pub fn inspect_assetlookup(assetlookup_path: &Path) -> Result<AssetLookupOverview> {
    let folder = parent_folder(assetlookup_path)?;
    let layout = detect_layout(folder)?;
    if layout != LevelLayout::V2 {
        return Err(Error::GltfWrite(format!(
            "Only V2 layout is supported (R2/R3/RCF). Got: {}.",
            layout.label()
        )));
    }

    let mut lookup = AssetLookup::open(BufReader::new(File::open(assetlookup_path)?))?;
    let version_major = lookup.file.version.major;
    let version_minor = lookup.file.version.minor;

    let mut kinds = Vec::with_capacity(AssetKind::all().len());
    for kind in AssetKind::all() {
        let count = lookup.pointers(*kind).map(|p| p.len()).unwrap_or(0);
        kinds.push(KindOverview {
            name: kind.name(),
            section_id: kind.section_id(),
            count,
            has_decoder: extractor_for(*kind).is_some(),
        });
    }

    Ok(AssetLookupOverview {
        layout: layout.tag(),
        version_major,
        version_minor,
        kinds,
    })
}

pub fn extract_assetlookup<F>(
    assetlookup_path: &Path,
    output_dir: &Path,
    options: &ExtractOptions,
    mut on_event: F,
) -> Result<()>
where
    F: FnMut(ExtractEvent),
{
    let folder = parent_folder(assetlookup_path)?;
    let layout = detect_layout(folder)?;
    if layout != LevelLayout::V2 {
        return Err(Error::GltfWrite(format!(
            "Only V2 layout (R2/R3/RCF) is supported in this version. Got: {}.",
            layout.label()
        )));
    }

    fs::create_dir_all(output_dir)?;

    let names = load_outfitter_names(assetlookup_path).unwrap_or_default();
    if !names.is_empty() {
        write_name_lookup_index(output_dir, &names)?;
    }

    let needs_textures = options.kinds.iter().any(|k| {
        matches!(
            k,
            AssetKind::Texture | AssetKind::HighMip | AssetKind::Moby | AssetKind::Tie
        )
    });
    let texture_pngs: HashMap<u32, Vec<u8>> = if needs_textures {
        bulk_extract_pngs(folder, None, options.max_texture_dim)?
            .into_iter()
            .collect()
    } else {
        HashMap::new()
    };

    let needs_shaders = options
        .kinds
        .iter()
        .any(|k| matches!(k, AssetKind::Moby | AssetKind::Tie | AssetKind::Shader));
    let shaders: HashMap<u64, ShaderInfo> = if needs_shaders {
        read_shaders(folder)?
    } else {
        HashMap::new()
    };

    let mut lookup = AssetLookup::open(BufReader::new(File::open(assetlookup_path)?))?;
    let mut pointer_cache: HashMap<AssetKind, Vec<AssetPointer>> = HashMap::new();
    for kind in &options.kinds {
        let ptrs = lookup.pointers(*kind).unwrap_or_default();
        pointer_cache.insert(*kind, ptrs);
    }

    for kind in &options.kinds {
        let ptrs = pointer_cache.remove(kind).unwrap_or_default();
        on_event(ExtractEvent::Total { kind: *kind, count: ptrs.len() });

        match kind {
            AssetKind::Texture => {
                extract_textures(&ptrs, &texture_pngs, output_dir, *kind, &mut on_event)?;
            }
            AssetKind::HighMip => {
                extract_textures(&ptrs, &texture_pngs, output_dir, *kind, &mut on_event)?;
            }
            AssetKind::Cubemap => {
                extract_cubemaps(folder, output_dir, &mut on_event)?;
            }
            AssetKind::Moby => {
                extract_mobys(folder, &shaders, &texture_pngs, output_dir, &names, &mut on_event)?;
            }
            AssetKind::Tie => {
                extract_ties(folder, &shaders, &texture_pngs, output_dir, &names, &mut on_event)?;
            }
            AssetKind::Shader => {
                extract_shaders_json(&shaders, output_dir, &mut on_event)?;
            }
            AssetKind::Zone => {
                extract_zones(folder, output_dir, &mut on_event)?;
            }
            AssetKind::Animset
            | AssetKind::Foliage
            | AssetKind::Shrub
            | AssetKind::Cinematic
            | AssetKind::Lighting => {
                extract_pointer_table_json(*kind, &ptrs, output_dir, &mut on_event)?;
            }
        }
        on_event(ExtractEvent::KindDone { kind: *kind });
    }

    Ok(())
}

fn extract_textures<F>(
    ptrs: &[AssetPointer],
    texture_pngs: &HashMap<u32, Vec<u8>>,
    output_dir: &Path,
    kind: AssetKind,
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(ExtractEvent),
{
    let dir = output_dir.join(kind.name());
    fs::create_dir_all(&dir)?;
    for (idx, ptr) in ptrs.iter().enumerate() {
        let id = ptr.tuid as u32;
        if let Some(bytes) = texture_pngs.get(&id) {
            let path = dir.join(format!("0x{:08X}.png", id));
            let ok = fs::write(&path, bytes).is_ok();
            on_event(ExtractEvent::Entry {
                kind,
                index: idx + 1,
                tuid: ptr.tuid,
                ok,
                message: if ok { None } else { Some("write failed".into()) },
            });
        } else {
            on_event(ExtractEvent::Entry {
                kind,
                index: idx + 1,
                tuid: ptr.tuid,
                ok: false,
                message: Some("decode produced no PNG".into()),
            });
        }
    }
    Ok(())
}

fn extract_cubemaps<F>(folder: &Path, output_dir: &Path, on_event: &mut F) -> Result<()>
where
    F: FnMut(ExtractEvent),
{
    let dir = output_dir.join("cubemaps");
    fs::create_dir_all(&dir)?;
    let cubemaps = read_cubemaps(folder).unwrap_or_default();
    for (idx, cube) in cubemaps.iter().enumerate() {
        let cube_dir = dir.join(format!("0x{:016X}", cube.hash));
        let mut ok = fs::create_dir_all(&cube_dir).is_ok();
        let mut message: Option<String> = None;
        if ok {
            for (face_idx, face) in cube.faces.iter().enumerate() {
                let face_name = face_name_for(face_idx);
                let png = encode_face_png(face);
                if png.is_empty() {
                    ok = false;
                    message = Some(format!("face {face_name}: encode produced empty PNG"));
                    break;
                }
                let face_path = cube_dir.join(format!("{face_name}.png"));
                if fs::write(&face_path, &png).is_err() {
                    ok = false;
                    message = Some(format!("write face {face_name} failed"));
                    break;
                }
            }
        } else {
            message = Some("mkdir failed".into());
        }
        on_event(ExtractEvent::Entry {
            kind: AssetKind::Cubemap,
            index: idx + 1,
            tuid: cube.hash,
            ok,
            message,
        });
    }
    Ok(())
}

fn encode_face_png(face: &CubemapFace) -> Vec<u8> {
    encode_png(&face.rgba, face.width, face.height)
}

fn face_name_for(idx: usize) -> &'static str {
    match idx {
        0 => "px",
        1 => "nx",
        2 => "py",
        3 => "ny",
        4 => "pz",
        5 => "nz",
        _ => "face",
    }
}

fn extract_mobys<F>(
    folder: &Path,
    shaders: &HashMap<u64, ShaderInfo>,
    texture_pngs: &HashMap<u32, Vec<u8>>,
    output_dir: &Path,
    names: &OutfitterNames,
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(ExtractEvent),
{
    let dir = output_dir.join("mobys");
    fs::create_dir_all(&dir)?;
    let mut idx = 0usize;
    read_moby_assets_streaming(folder, None, |asset| {
        idx += 1;
        let (ok, message) = match write_moby_glb_full(&asset, &[], shaders, texture_pngs) {
            Ok(bytes) => {
                let path = dir.join(glb_filename(asset.tuid, names));
                if fs::write(&path, &bytes).is_ok() {
                    (true, None)
                } else {
                    (false, Some("write failed".to_string()))
                }
            }
            Err(e) => (false, Some(e.to_string())),
        };
        on_event(ExtractEvent::Entry {
            kind: AssetKind::Moby,
            index: idx,
            tuid: asset.tuid,
            ok,
            message,
        });
    })?;
    Ok(())
}

fn extract_ties<F>(
    folder: &Path,
    shaders: &HashMap<u64, ShaderInfo>,
    texture_pngs: &HashMap<u32, Vec<u8>>,
    output_dir: &Path,
    names: &OutfitterNames,
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(ExtractEvent),
{
    let dir = output_dir.join("ties");
    fs::create_dir_all(&dir)?;
    let mut idx = 0usize;
    read_tie_assets_streaming(folder, None, |tie| {
        idx += 1;
        let synth = tie_as_moby(&tie);
        let (ok, message) = match write_moby_glb_full(&synth, &[], shaders, texture_pngs) {
            Ok(bytes) => {
                let path = dir.join(glb_filename(tie.tuid, names));
                if fs::write(&path, &bytes).is_ok() {
                    (true, None)
                } else {
                    (false, Some("write failed".to_string()))
                }
            }
            Err(e) => (false, Some(e.to_string())),
        };
        on_event(ExtractEvent::Entry {
            kind: AssetKind::Tie,
            index: idx,
            tuid: tie.tuid,
            ok,
            message,
        });
    })?;
    Ok(())
}

fn tie_as_moby(tie: &TieAsset) -> MobyAsset {
    let meshes: Vec<MobyMesh> = tie
        .meshes
        .iter()
        .map(|m| MobyMesh {
            shader_index: m.shader_index,
            vertex_count: m.vertex_count,
            index_count: m.index_count,
            vertex_stride: 0x14,
            positions: m.positions.clone(),
            uvs: m.uvs.clone(),
            indices: m.indices.clone(),
            bone_indices: Vec::new(),
            bone_weights: Vec::new(),
        })
        .collect();
    MobyAsset {
        tuid: tie.tuid,
        name: format!("tie_{:016X}", tie.tuid),
        bangles: vec![MobyBangle { meshes }],
        bsphere_position: [0.0, 0.0, 0.0],
        bsphere_radius: 0.0,
        shader_tuids: tie.shader_tuids.clone(),
        skeleton: None,
        animset_hash: None,
        bind_pose_inverse_offset: 0,
        rfom_anim_offsets: Vec::new(),
    }
}

#[derive(Serialize)]
struct ShaderJson {
    tuid: String,
    albedo_tex_id: Option<u32>,
    normal_tex_id: Option<u32>,
    expensive_tex_id: Option<u32>,
}

fn extract_shaders_json<F>(
    shaders: &HashMap<u64, ShaderInfo>,
    output_dir: &Path,
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(ExtractEvent),
{
    let dir = output_dir.join("shaders");
    fs::create_dir_all(&dir)?;
    let mut sorted: Vec<&ShaderInfo> = shaders.values().collect();
    sorted.sort_by_key(|s| s.tuid);
    for (idx, info) in sorted.iter().enumerate() {
        let json = ShaderJson {
            tuid: format!("0x{:016X}", info.tuid),
            albedo_tex_id: info.albedo_tex_id,
            normal_tex_id: info.normal_tex_id,
            expensive_tex_id: info.expensive_tex_id,
        };
        let path = dir.join(format!("0x{:016X}.json", info.tuid));
        let (ok, message) = match serde_json::to_vec_pretty(&json) {
            Ok(bytes) => match fs::write(&path, &bytes) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e.to_string())),
            },
            Err(e) => (false, Some(e.to_string())),
        };
        on_event(ExtractEvent::Entry {
            kind: AssetKind::Shader,
            index: idx + 1,
            tuid: info.tuid,
            ok,
            message,
        });
    }
    Ok(())
}

#[derive(Serialize)]
struct ZoneJson {
    tuid: String,
    tie_instance_count: usize,
    shrub_instance_count: usize,
    foliage_instance_count: usize,
    ufrag_count: usize,
    ufrag_shader_count: usize,
}

fn extract_zones<F>(folder: &Path, output_dir: &Path, on_event: &mut F) -> Result<()>
where
    F: FnMut(ExtractEvent),
{
    let dir = output_dir.join("zones");
    fs::create_dir_all(&dir)?;
    let mut idx = 0usize;
    read_zones_streaming(folder, |zone: Zone| {
        idx += 1;
        let json = ZoneJson {
            tuid: format!("0x{:016X}", zone.tuid),
            tie_instance_count: zone.tie_instances.len(),
            shrub_instance_count: zone.shrub_instances.len(),
            foliage_instance_count: zone.foliage_instances.len(),
            ufrag_count: zone.ufrags.len(),
            ufrag_shader_count: zone.ufrag_shader_tuids.len(),
        };
        let path = dir.join(format!("0x{:016X}.json", zone.tuid));
        let (ok, message) = match serde_json::to_vec_pretty(&json) {
            Ok(bytes) => match fs::write(&path, &bytes) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e.to_string())),
            },
            Err(e) => (false, Some(e.to_string())),
        };
        on_event(ExtractEvent::Entry {
            kind: AssetKind::Zone,
            index: idx,
            tuid: zone.tuid,
            ok,
            message,
        });
    })?;
    Ok(())
}

#[derive(Serialize)]
struct PointerRow {
    tuid: String,
    offset: u32,
    length: u32,
}

#[derive(Serialize)]
struct PointerTable {
    kind: &'static str,
    section_id: u32,
    count: usize,
    entries: Vec<PointerRow>,
}

fn extract_pointer_table_json<F>(
    kind: AssetKind,
    ptrs: &[AssetPointer],
    output_dir: &Path,
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(ExtractEvent),
{
    let dir = output_dir.join(kind.name());
    fs::create_dir_all(&dir)?;
    let table = PointerTable {
        kind: kind.name(),
        section_id: kind.section_id(),
        count: ptrs.len(),
        entries: ptrs
            .iter()
            .map(|p| PointerRow {
                tuid: format!("0x{:016X}", p.tuid),
                offset: p.offset,
                length: p.length,
            })
            .collect(),
    };
    let path = dir.join("pointer_table.json");
    let (ok, message) = match serde_json::to_vec_pretty(&table) {
        Ok(bytes) => match fs::write(&path, &bytes) {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        },
        Err(e) => (false, Some(e.to_string())),
    };
    for (idx, ptr) in ptrs.iter().enumerate() {
        on_event(ExtractEvent::Entry {
            kind,
            index: idx + 1,
            tuid: ptr.tuid,
            ok,
            message: message.clone(),
        });
    }
    Ok(())
}

fn parent_folder(assetlookup_path: &Path) -> Result<&Path> {
    assetlookup_path.parent().ok_or_else(|| {
        Error::GltfWrite(format!(
            "input path has no parent folder: {}",
            assetlookup_path.display()
        ))
    })
}

fn glb_filename(tuid: u64, names: &OutfitterNames) -> String {
    match names.lookup(tuid) {
        Some(name) => format!("{name}_0x{:016X}.glb", tuid),
        None => format!("0x{:016X}.glb", tuid),
    }
}

fn write_name_lookup_index(output_dir: &Path, names: &OutfitterNames) -> Result<()> {
    let mut rows: Vec<(String, String)> = names
        .by_tuid
        .iter()
        .map(|(tuid, name)| (format!("0x{:016X}", tuid), name.clone()))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let body: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(tuid, name)| {
            serde_json::json!({
                "tuid": tuid,
                "name": name,
            })
        })
        .collect();
    let json = serde_json::to_vec_pretty(&body)
        .map_err(|e| Error::GltfWrite(format!("encode name_lookup.json: {e}")))?;
    fs::write(output_dir.join("name_lookup.json"), json)?;
    Ok(())
}

fn extractor_for(kind: AssetKind) -> Option<&'static str> {
    match kind {
        AssetKind::Texture | AssetKind::HighMip => Some("png"),
        AssetKind::Cubemap => Some("png-faces"),
        AssetKind::Moby | AssetKind::Tie => Some("glb"),
        AssetKind::Shader | AssetKind::Zone => Some("json"),
        AssetKind::Animset
        | AssetKind::Foliage
        | AssetKind::Shrub
        | AssetKind::Cinematic
        | AssetKind::Lighting => None,
    }
}

