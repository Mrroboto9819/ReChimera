#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cache;
mod r2;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use std::collections::{HashMap, HashSet};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use lunalib::math::zyx_euler_to_quat;
use lunalib::{
    bulk_extract_pngs, decode_animation, downsample_rgba, encode_png, extract_bank_sounds_for_file, extract_stream_sounds,
    list_sounds as list_sounds_in, read_animation_control, read_animation_header, read_gameplay,
    read_moby_assets_with_total, read_shaders, read_textures_with_total,
    read_tie_assets_with_total, read_zones, read_zones_streaming, AssetKind, AssetLookup,
    AssetPointer, IgFile, ShaderInfo, SoundKind,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

#[derive(Serialize, Clone)]
struct SectionDto {
    id: u32,
    offset: u32,
    count: u32,
    length: u32,
}

#[derive(Serialize)]
struct AssetCount {
    kind: &'static str,
    section_id: u32,
    count: usize,
    present: bool,
}

#[derive(Serialize)]
struct LevelSummary {
    folder: String,
    version_major: u16,
    version_minor: u16,
    sections: Vec<SectionDto>,
    asset_counts: Vec<AssetCount>,
}

#[derive(Serialize)]
struct AssetPointerDto {

    tuid: String,
    offset: u32,
    length: u32,
}

#[derive(Serialize)]
pub(crate) struct InstanceDto {
    pub(crate) tuid: String,
    pub(crate) asset_tuid: String,
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    pub(crate) position: [f32; 3],
    pub(crate) quaternion: [f32; 4],
    pub(crate) scale: [f32; 3],
}

#[derive(Serialize)]
struct UFragDto {
    tuid: String,
    zone_tuid: String,
    position: [f32; 3],
    radius: f32,
    vertex_count: u16,
    triangle_count: u16,
}

#[derive(Serialize)]
struct LevelLayoutDto {
    instances: Vec<InstanceDto>,
    ufrags: Vec<UFragDto>,
}


#[derive(Serialize, Deserialize)]
pub(crate) struct MeshDto {
    pub positions_b64: String,
    pub uvs_b64: String,
    pub indices_b64: String,
    pub albedo_id: Option<u32>,
    pub normal_id: Option<u32>,
    pub emissive_id: Option<u32>,
    pub bone_indices_b64: String,
    pub bone_weights_b64: String,
}


#[derive(Serialize)]
struct TextureDto {
    id: u32,
    width: u32,
    height: u32,
}


#[derive(Serialize, Deserialize)]
pub(crate) struct SkeletonDto {
    bone_count: usize,
    root_bone: u16,
    parents: Vec<i16>,
    bind_local: Vec<[f32; 16]>,
    bind_world_inverse: Vec<[f32; 16]>,
    tms0_col: Vec<[f32; 16]>,
    tms1_col: Vec<[f32; 16]>,
    scale_shift: u16,
    translation_shift: u16,
}


#[derive(Serialize, Deserialize)]
pub(crate) struct AssetMeshesDto {
    pub(crate) asset_tuid: String,
    pub(crate) name: String,
    pub(crate) submeshes: Vec<MeshDto>,
    pub(crate) skeleton: Option<SkeletonDto>,
    pub(crate) animset_hash: Option<String>,
    pub(crate) bind_pose_inverse_offset: i16,
    #[serde(default)]
    pub(crate) embedded_animation_count: u32,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct UFragMeshDto {
    pub tuid: String,
    pub zone_tuid: String,
    pub position: [f32; 3],
    pub mesh: MeshDto,
}

fn encode_f32_buffer(values: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<f32>());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    BASE64.encode(bytes)
}

fn encode_u32_buffer(values: &[u32]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<u32>());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    BASE64.encode(bytes)
}

fn encode_u16_buffer(values: &[u16]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<u16>());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    BASE64.encode(bytes)
}

fn encode_u8_buffer(values: &[u8]) -> String {
    BASE64.encode(values)
}


fn downsample_dims(width: u32, height: u32, max_dim: u32) -> (u32, u32) {
    if width <= max_dim && height <= max_dim {
        return (width, height);
    }
    let (new_w, new_h) = if width >= height {
        (max_dim, ((height as u64 * max_dim as u64) / width as u64) as u32)
    } else {
        (((width as u64 * max_dim as u64) / height as u64) as u32, max_dim)
    };
    (new_w.max(1), new_h.max(1))
}

pub(crate) fn mesh_dto(
    positions: Vec<f32>,
    uvs: Vec<f32>,
    indices: Vec<u32>,
    albedo_id: Option<u32>,
    normal_id: Option<u32>,
    emissive_id: Option<u32>,
    bone_indices: Vec<u16>,
    bone_weights: Vec<u8>,
) -> MeshDto {
    MeshDto {
        positions_b64: encode_f32_buffer(&positions),
        uvs_b64: encode_f32_buffer(&uvs),
        indices_b64: encode_u32_buffer(&indices),
        albedo_id,
        normal_id,
        emissive_id,
        bone_indices_b64: encode_u16_buffer(&bone_indices),
        bone_weights_b64: encode_u8_buffer(&bone_weights),
    }
}


pub(crate) fn resolve_shader_textures(
    shaders: &HashMap<u64, ShaderInfo>,
    shader_tuids: &[u64],
    shader_index: usize,
) -> (Option<u32>, Option<u32>, Option<u32>) {
    let Some(&st) = shader_tuids.get(shader_index) else {
        return (None, None, None);
    };
    let Some(s) = shaders.get(&st) else {
        return (None, None, None);
    };
    (s.albedo_tex_id, s.normal_tex_id, s.expensive_tex_id)
}

pub(crate) fn build_skeleton_dto(skel: &Option<lunalib::Skeleton>) -> Option<SkeletonDto> {
    let s = skel.as_ref()?;
    Some(SkeletonDto {
        bone_count: s.bones.len(),
        root_bone: s.root_bone,
        parents: s.bones.iter().map(|b| b.parent_index).collect(),
        bind_local: s.bind_local.clone(),
        bind_world_inverse: s.bind_world_inverse.clone(),
        tms0_col: s.tms0_col.clone(),
        tms1_col: s.tms1_col.clone(),
        scale_shift: s.scale_shift,
        translation_shift: s.translation_shift,
    })
}

fn parse_kind(name: &str) -> Option<AssetKind> {
    AssetKind::all().iter().copied().find(|k| k.name() == name)
}

fn assetlookup_path(folder: &str) -> PathBuf {
    Path::new(folder).join("assetlookup.dat")
}

fn open_lookup(folder: &str) -> Result<AssetLookup<BufReader<File>>, String> {
    let path = assetlookup_path(folder);
    let file = File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
    AssetLookup::open(BufReader::new(file)).map_err(|e| e.to_string())
}


struct CachedFolder {
    version_major: u16,
    version_minor: u16,
    sections: Vec<SectionDto>,
    pointers_by_section: HashMap<u32, Vec<AssetPointer>>,
}

#[derive(Default)]
struct AssetCache {
    folders: HashMap<String, CachedFolder>,
}

impl AssetCache {
    fn ensure(&mut self, folder: &str) -> Result<&CachedFolder, String> {
        if !self.folders.contains_key(folder) {
            let entry = load_cached_folder(folder)?;
            self.folders.insert(folder.to_string(), entry);
        }
        Ok(self.folders.get(folder).expect("just inserted"))
    }
}

fn load_cached_folder(folder: &str) -> Result<CachedFolder, String> {
    let mut lookup = open_lookup(folder)?;
    let sections: Vec<SectionDto> = lookup
        .file
        .sections
        .iter()
        .map(|s| SectionDto {
            id: s.id,
            offset: s.offset,
            count: s.count,
            length: s.length,
        })
        .collect();
    let mut pointers_by_section = HashMap::new();
    for kind in AssetKind::all() {
        let ptrs = lookup.pointers(*kind).map_err(|e| e.to_string())?;
        pointers_by_section.insert(kind.section_id(), ptrs);
    }
    Ok(CachedFolder {
        version_major: lookup.file.version.major,
        version_minor: lookup.file.version.minor,
        sections,
        pointers_by_section,
    })
}

#[tauri::command]
fn open_level(
    folder: String,
    cache: State<'_, Mutex<AssetCache>>,
) -> Result<LevelSummary, String> {
    let layout = lunalib::detect_layout(Path::new(&folder)).map_err(|_| {
        "Folder has none of main.dat (TOD), assetlookup.dat (V2), or ps3levelmain.dat (RFOM)"
            .to_string()
    })?;
    let bundled_entry: Option<&'static str> = match layout {
        lunalib::LevelLayout::Tod => Some("main.dat"),
        lunalib::LevelLayout::Rfom => Some("ps3levelmain.dat"),
        lunalib::LevelLayout::V2 => None,
    };
    if let Some(filename) = bundled_entry {
        // TOD / RFOM — no assetlookup.dat to walk. Open the bundled
        // entry file just to surface its IGHW version + section list
        // to the toolbar; full extraction happens in
        // `extract_level_to_cache`.
        let entry_path = Path::new(&folder).join(filename);
        let file = File::open(&entry_path)
            .map_err(|e| format!("open {}: {e}", entry_path.display()))?;
        let ig = lunalib::IgFile::open(BufReader::new(file)).map_err(|e| e.to_string())?;
        let sections: Vec<SectionDto> = ig
            .sections
            .iter()
            .map(|s| SectionDto {
                id: s.id,
                offset: s.offset,
                count: s.count,
                length: s.length,
            })
            .collect();
        return Ok(LevelSummary {
            folder: folder.clone(),
            version_major: ig.version.major,
            version_minor: ig.version.minor,
            sections,
            asset_counts: Vec::new(),
        });
    }
    let mut cache = cache.lock().map_err(|e| format!("cache lock: {e}"))?;
    let entry = cache.ensure(&folder)?;
    let asset_counts = AssetKind::all()
        .iter()
        .map(|kind| {
            let count = entry
                .pointers_by_section
                .get(&kind.section_id())
                .map(|v| v.len())
                .unwrap_or(0);
            AssetCount {
                kind: kind.name(),
                section_id: kind.section_id(),
                count,
                present: count > 0,
            }
        })
        .collect();
    Ok(LevelSummary {
        folder: folder.clone(),
        version_major: entry.version_major,
        version_minor: entry.version_minor,
        sections: entry.sections.clone(),
        asset_counts,
    })
}

#[tauri::command]
fn list_assets(
    folder: String,
    kind: String,
    cache: State<'_, Mutex<AssetCache>>,
) -> Result<Vec<AssetPointerDto>, String> {
    let kind = parse_kind(&kind).ok_or_else(|| format!("unknown asset kind: {kind}"))?;
    let mut cache = cache.lock().map_err(|e| format!("cache lock: {e}"))?;
    let entry = cache.ensure(&folder)?;
    let ptrs = entry
        .pointers_by_section
        .get(&kind.section_id())
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    Ok(ptrs
        .iter()
        .map(|p| AssetPointerDto {
            tuid: format!("0x{:016X}", p.tuid),
            offset: p.offset,
            length: p.length,
        })
        .collect())
}

#[derive(Serialize)]
struct ManifestEntry {

    tuid: String,
    offset: u32,
    length: u32,
}

#[derive(Serialize)]
struct ManifestGroup {
    kind: &'static str,
    section_id: u32,

    decoded: bool,
    count: usize,
    entries: Vec<ManifestEntry>,
}

#[derive(Serialize)]
struct LevelManifest {
    folder: String,

    engine: &'static str,
    version_major: u16,
    version_minor: u16,
    sections: Vec<SectionDto>,
    groups: Vec<ManifestGroup>,
}


#[tauri::command]
fn build_level_manifest(
    folder: String,
    cache: State<'_, Mutex<AssetCache>>,
) -> Result<LevelManifest, String> {
    let mut cache = cache.lock().map_err(|e| format!("cache lock: {e}"))?;
    let entry = cache.ensure(&folder)?;
    let groups = AssetKind::all()
        .iter()
        .map(|kind| {
            let ptrs = entry
                .pointers_by_section
                .get(&kind.section_id())
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let entries: Vec<ManifestEntry> = ptrs
                .iter()
                .map(|p| ManifestEntry {
                    tuid: format!("0x{:016X}", p.tuid),
                    offset: p.offset,
                    length: p.length,
                })
                .collect();
            ManifestGroup {
                kind: kind.name(),
                section_id: kind.section_id(),
                decoded: kind.has_decoder(),
                count: entries.len(),
                entries,
            }
        })
        .collect();
    Ok(LevelManifest {
        folder: folder.clone(),
        engine: "new",
        version_major: entry.version_major,
        version_minor: entry.version_minor,
        sections: entry.sections.clone(),
        groups,
    })
}


#[tauri::command]
fn level_layout(folder: String) -> Result<LevelLayoutDto, String> {
    let mut instances = Vec::new();
    instances.extend(real_moby_layout(&folder).unwrap_or_default());
    instances.extend(real_tie_layout(&folder).unwrap_or_default());
    // NOTE: 0xC200 ("lights") and 0x9700 ("env-samplers") were both
    // misidentified — they are actually Foliage / FoliageInstance. The two
    // calls below are kept around only so the existing UI toggles don't
    // dangle, but they now return None for RFOM levels. Foliage placements
    // are surfaced through real_tie_layout via read_foliage_rfom.
    let ufrags = real_ufrag_bounds(&folder).unwrap_or_default();
    Ok(LevelLayoutDto { instances, ufrags })
}

// Both real_envsampler_layout and real_light_layout were removed once we
// learned 0xC200 = Foliage and 0x9700 = FoliageInstance, not lights or
// env-samplers. Foliage placements now flow through real_tie_layout via
// `read_foliage_rfom`.

pub(crate) fn real_moby_layout(folder: &str) -> Option<Vec<InstanceDto>> {
    let path = Path::new(folder);
    let layout = match lunalib::detect_layout(path) {
        Ok(lunalib::LevelLayout::Tod) => lunalib::read_gameplay_old(path).ok()?,
        Ok(lunalib::LevelLayout::Rfom) => lunalib::read_gameplay_rfom(path).ok()?,
        _ => read_gameplay(path).ok()?,
    };
    let mut out = Vec::new();
    for region in layout.regions {
        for inst in region.moby_instances {
            out.push(InstanceDto {
                tuid: format!("0x{:016X}", inst.instance_tuid),
                asset_tuid: format!("0x{:016X}", inst.moby_tuid),
                kind: AssetKind::Moby.name(),
                name: inst.name,
                position: inst.position,
                quaternion: zyx_euler_to_quat(inst.rotation),
                scale: [inst.scale, inst.scale, inst.scale],
            });
        }
    }
    (!out.is_empty()).then_some(out)
}

pub(crate) fn real_tie_layout(folder: &str) -> Option<Vec<InstanceDto>> {
    let path = Path::new(folder);
    let mut out = Vec::new();
    match lunalib::detect_layout(path) {
        Ok(lunalib::LevelLayout::Tod) => {
            for zone in lunalib::read_zones_old(path).ok()? {
                for inst in zone.tie_instances {
                    out.push(tie_instance_dto(&inst));
                }
            }
        }
        Ok(lunalib::LevelLayout::Rfom) => {
            for inst in lunalib::read_tie_instances_rfom(path).ok()? {
                out.push(tie_instance_dto(&inst));
            }
            if let Ok((_, detail_insts)) =
                lunalib::read_detail_clusters_rfom(path)
            {
                for inst in detail_insts {
                    out.push(detail_instance_dto(&inst));
                }
            }
            if let Ok((_, shrub_insts)) = lunalib::read_shrubs_rfom(path) {
                for inst in shrub_insts {
                    out.push(shrub_instance_dto(&inst));
                }
            }
            if let Ok((_, foliage_insts)) = lunalib::read_foliage_rfom(path) {
                for inst in foliage_insts {
                    out.push(foliage_instance_dto(&inst));
                }
            }
        }
        _ => {
            let zones = read_zones(path).ok()?;
            let zone_count = zones.len();
            let mut tie_count = 0usize;
            let mut shrub_count = 0usize;
            let mut foliage_count = 0usize;
            for zone in zones {
                tie_count += zone.tie_instances.len();
                shrub_count += zone.shrub_instances.len();
                foliage_count += zone.foliage_instances.len();
                for inst in zone.tie_instances {
                    out.push(tie_instance_dto(&inst));
                }
                for inst in zone.shrub_instances {
                    out.push(shrub_instance_dto(&inst));
                }
                for inst in zone.foliage_instances {
                    out.push(foliage_instance_dto(&inst));
                }
            }
            if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
                eprintln!(
                    "[v2-tie] read_zones returned {} zones: {} tie / {} shrub / {} foliage placements",
                    zone_count, tie_count, shrub_count, foliage_count
                );
            }
        }
    }
    if std::env::var("RECHIMERA_LOG_PROBES").is_ok()
        && matches!(lunalib::detect_layout(path), Ok(lunalib::LevelLayout::V2))
    {
        eprintln!(
            "[v2-layout] real_tie_layout produced {} total instances",
            out.len()
        );
    }
    (!out.is_empty()).then_some(out)
}

fn tie_instance_dto(inst: &lunalib::TieInstance) -> InstanceDto {
    InstanceDto {
        tuid: format!("0x{:016X}", inst.instance_tuid),
        asset_tuid: format!("0x{:016X}", inst.tie_tuid),
        kind: AssetKind::Tie.name(),
        name: inst.name.clone(),
        position: inst.position,
        quaternion: inst.quaternion,
        scale: inst.scale,
    }
}

fn detail_instance_dto(inst: &lunalib::TieInstance) -> InstanceDto {
    InstanceDto {
        tuid: format!("0x{:016X}", inst.instance_tuid),
        asset_tuid: format!("0x{:016X}", inst.tie_tuid),
        kind: "detail",
        name: inst.name.clone(),
        position: inst.position,
        quaternion: inst.quaternion,
        scale: inst.scale,
    }
}

fn shrub_instance_dto(inst: &lunalib::TieInstance) -> InstanceDto {
    InstanceDto {
        tuid: format!("0x{:016X}", inst.instance_tuid),
        asset_tuid: format!("0x{:016X}", inst.tie_tuid),
        kind: "shrub",
        name: inst.name.clone(),
        position: inst.position,
        quaternion: inst.quaternion,
        scale: inst.scale,
    }
}

fn foliage_instance_dto(inst: &lunalib::TieInstance) -> InstanceDto {
    InstanceDto {
        tuid: format!("0x{:016X}", inst.instance_tuid),
        asset_tuid: format!("0x{:016X}", inst.tie_tuid),
        kind: "foliage",
        name: inst.name.clone(),
        position: inst.position,
        quaternion: inst.quaternion,
        scale: inst.scale,
    }
}

fn real_ufrag_bounds(folder: &str) -> Option<Vec<UFragDto>> {
    let path = Path::new(folder);
    let zones = match lunalib::detect_layout(path) {
        Ok(lunalib::LevelLayout::Rfom) => lunalib::read_regions_rfom(path).ok()?,
        Ok(lunalib::LevelLayout::Tod) => return None,
        _ => read_zones(path).ok()?,
    };
    let mut out = Vec::new();
    for zone in zones {
        let zone_tuid_hex = format!("0x{:016X}", zone.tuid);
        for u in zone.ufrags {
            out.push(UFragDto {
                tuid: format!("0x{:016X}", u.tuid),
                zone_tuid: zone_tuid_hex.clone(),
                position: u.position,
                radius: u.radius,
                vertex_count: u.vertex_count,
                triangle_count: u.index_count / 3,
            });
        }
    }
    (!out.is_empty()).then_some(out)
}


const CHUNK_SIZE: usize = 4;

const CHUNK_PAUSE_MS: u64 = 4;
#[inline(always)]
fn chunk_yield(counter: usize) {
    if counter > 0 && counter % CHUNK_SIZE == 0 {
        std::thread::sleep(std::time::Duration::from_millis(CHUNK_PAUSE_MS));
    }
}


#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LevelEvent {

    Phase {
        phase: &'static str,
        label: &'static str,
        total: usize,
        chunk_size: usize,
    },

    Progress { current: usize },

    MobyAsset { asset: AssetMeshesDto },

    TieAsset { asset: AssetMeshesDto },

    UfragMesh { mesh: UFragMeshDto },

    Texture { texture: TextureDto },

    Done,

    Error { message: String },
}


#[tauri::command]
fn level_meshes_stream(folder: String, on_event: Channel<LevelEvent>) -> Result<(), String> {
    if let Err(message) = run_level_stream(&folder, &on_event) {
        let _ = on_event.send(LevelEvent::Error { message: message.clone() });
        return Err(message);
    }
    let _ = on_event.send(LevelEvent::Done);
    Ok(())
}

fn run_level_stream(folder: &str, on_event: &Channel<LevelEvent>) -> Result<(), String> {
    let path = Path::new(folder);

    if matches!(lunalib::detect_layout(path), Ok(lunalib::LevelLayout::Rfom)) {
        return run_level_stream_rfom(path, on_event);
    }

    let _ = on_event.send(LevelEvent::Phase {
        phase: "layout",
        label: "Reading placements",
        total: 1,
        chunk_size: CHUNK_SIZE,
    });

    let mut moby_tuids: HashSet<u64> = HashSet::new();
    let mut tie_tuids: HashSet<u64> = HashSet::new();

    let gameplay_layout = match lunalib::detect_layout(path) {
        Ok(lunalib::LevelLayout::Tod) => lunalib::read_gameplay_old(path).ok(),
        _ => read_gameplay(path).ok(),
    };
    if let Some(layout) = gameplay_layout {
        for region in layout.regions {
            for inst in region.moby_instances {
                moby_tuids.insert(inst.moby_tuid);
            }
        }
    }


    let mut zones: Vec<lunalib::Zone> = Vec::new();
    match lunalib::detect_layout(path) {
        Ok(lunalib::LevelLayout::Tod) => {
            for z in lunalib::read_zones_old(path).unwrap_or_default() {
                for inst in &z.tie_instances {
                    tie_tuids.insert(inst.tie_tuid);
                }
                zones.push(z);
            }
        }
        _ => {
            read_zones_streaming(path, |z| {
                for inst in &z.tie_instances {
                    tie_tuids.insert(inst.tie_tuid);
                }
                zones.push(z);
            })
            .map_err(|e| e.to_string())?;
        }
    }

    if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        eprintln!(
            "[v2-match] unique moby tuids in placements: {}, unique tie tuids in placements: {}",
            moby_tuids.len(),
            tie_tuids.len()
        );
    }
    let moby_tuids: Vec<u64> = moby_tuids.into_iter().collect();
    let tie_tuids: Vec<u64> = tie_tuids.into_iter().collect();
    let _ = on_event.send(LevelEvent::Progress { current: 1 });


    let _ = on_event.send(LevelEvent::Phase {
        phase: "shaders",
        label: "Reading shaders",
        total: 1,
        chunk_size: CHUNK_SIZE,
    });
    let shaders: HashMap<u64, ShaderInfo> = read_shaders(path).map_err(|e| e.to_string())?;
    let _ = on_event.send(LevelEvent::Progress { current: 1 });


    let mut needed_albedo: HashSet<u32> = HashSet::new();


    {
        let mut moby_done = 0usize;
        let mut emitted_moby_tuids: Vec<u64> = Vec::new();
        read_moby_assets_with_total(
            path,
            Some(&moby_tuids),
            |total| {
                let _ = on_event.send(LevelEvent::Phase {
                    phase: "mobys",
                    label: "Decoding mobys",
                    total,
                    chunk_size: CHUNK_SIZE,
                });
            },
            |asset| {
                let mut submeshes = Vec::new();
                for bangle in asset.bangles {
                    for m in bangle.meshes {
                        let (albedo, normal, emissive) = resolve_shader_textures(
                            &shaders,
                            &asset.shader_tuids,
                            m.shader_index as usize,
                        );
                        for id in [albedo, normal, emissive].into_iter().flatten() {
                            needed_albedo.insert(id);
                        }
                        submeshes.push(mesh_dto(
                            m.positions,
                            m.uvs,
                            m.indices,
                            albedo,
                            normal,
                            emissive,
                            m.bone_indices,
                            m.bone_weights,
                        ));
                    }
                }
                let skeleton = build_skeleton_dto(&asset.skeleton);
                let dto = AssetMeshesDto {
                    asset_tuid: format!("0x{:016X}", asset.tuid),
                    name: asset.name.clone(),
                    submeshes,
                    skeleton,
                    animset_hash: asset.animset_hash.map(|h| format!("0x{:016X}", h)),
                    bind_pose_inverse_offset: asset.bind_pose_inverse_offset,
                    embedded_animation_count: asset.rfom_anim_offsets.len() as u32,
                };
                emitted_moby_tuids.push(asset.tuid);
                let _ = on_event.send(LevelEvent::MobyAsset { asset: dto });
                moby_done += 1;
                let _ = on_event.send(LevelEvent::Progress { current: moby_done });
                chunk_yield(moby_done);
            },
        )
        .map_err(|e| e.to_string())?;
        if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
            let placement_set: std::collections::HashSet<u64> =
                moby_tuids.iter().copied().collect();
            let emitted_set: std::collections::HashSet<u64> =
                emitted_moby_tuids.iter().copied().collect();
            let matched = placement_set.intersection(&emitted_set).count();
            let sample_emitted: Vec<String> = emitted_moby_tuids
                .iter()
                .take(5)
                .map(|t| format!("0x{:016X}", t))
                .collect();
            eprintln!(
                "[v2-match] mobys: {} placement tuids, {} emitted assets, {} match — first emitted: {:?}",
                placement_set.len(),
                emitted_set.len(),
                matched,
                sample_emitted
            );
        }
    }


    {
        let mut tie_done = 0usize;
        read_tie_assets_with_total(
            path,
            Some(&tie_tuids),
            |total| {
                let _ = on_event.send(LevelEvent::Phase {
                    phase: "ties",
                    label: "Decoding ties",
                    total,
                    chunk_size: CHUNK_SIZE,
                });
            },
            |asset| {
                let submeshes: Vec<MeshDto> = asset
                    .meshes
                    .into_iter()
                    .map(|m| {
                        let (albedo, normal, emissive) = resolve_shader_textures(
                            &shaders,
                            &asset.shader_tuids,
                            m.shader_index as usize,
                        );
                        for id in [albedo, normal, emissive].into_iter().flatten() {
                            needed_albedo.insert(id);
                        }
                        mesh_dto(
                            m.positions,
                            m.uvs,
                            m.indices,
                            albedo,
                            normal,
                            emissive,

                            Vec::new(),
                            Vec::new(),
                        )
                    })
                    .collect();

                let dto = AssetMeshesDto {
                    asset_tuid: format!("0x{:016X}", asset.tuid),
                    name: String::new(),
                    submeshes,
                    skeleton: None,
                    animset_hash: None,
                    bind_pose_inverse_offset: 0,
                    embedded_animation_count: 0,
                };
                let _ = on_event.send(LevelEvent::TieAsset { asset: dto });
                tie_done += 1;
                let _ = on_event.send(LevelEvent::Progress { current: tie_done });
                chunk_yield(tie_done);
            },
        )
        .map_err(|e| e.to_string())?;
    }


    let total_ufrags: usize = zones
        .iter()
        .map(|z| {
            z.ufrags
                .iter()
                .filter(|u| !u.positions.is_empty() && !u.indices.is_empty())
                .count()
        })
        .sum();
    let _ = on_event.send(LevelEvent::Phase {
        phase: "ufrags",
        label: "Decoding terrain",
        total: total_ufrags,
        chunk_size: CHUNK_SIZE,
    });
    let mut ufrag_done = 0usize;
    for zone in zones {
        let zone_tuid_hex = format!("0x{:016X}", zone.tuid);
        for u in zone.ufrags {
            if u.positions.is_empty() || u.indices.is_empty() {
                continue;
            }
            let shader_info = zone
                .ufrag_shader_tuids
                .get(u.shader_index as usize)
                .and_then(|st| shaders.get(st));
            let albedo = shader_info.and_then(|s| s.albedo_tex_id);
            let normal = shader_info.and_then(|s| s.normal_tex_id);
            let emissive = shader_info.and_then(|s| s.expensive_tex_id);
            for id in [albedo, normal, emissive].into_iter().flatten() {
                needed_albedo.insert(id);
            }
            let dto = UFragMeshDto {
                tuid: format!("0x{:016X}", u.tuid),
                zone_tuid: zone_tuid_hex.clone(),
                position: u.position,
                mesh: mesh_dto(
                    u.positions,
                    u.uvs,
                    u.indices,
                    albedo,
                    normal,
                    emissive,

                    Vec::new(),
                    Vec::new(),
                ),
            };
            let _ = on_event.send(LevelEvent::UfragMesh { mesh: dto });
            ufrag_done += 1;
            let _ = on_event.send(LevelEvent::Progress { current: ufrag_done });
            chunk_yield(ufrag_done);
        }
    }


    if needed_albedo.is_empty() {
        let _ = on_event.send(LevelEvent::Phase {
            phase: "textures",
            label: "Decoding textures",
            total: 0,
            chunk_size: CHUNK_SIZE,
        });
    } else {
        let mut tex_done = 0usize;
        let needed = needed_albedo.clone();
        read_textures_with_total(
            path,
            move |id| needed.contains(&id),
            |total| {
                let _ = on_event.send(LevelEvent::Phase {
                    phase: "textures",
                    label: "Decoding textures",
                    total,
                    chunk_size: CHUNK_SIZE,
                });
            },
            |t| {
                if !t.is_decoded() {
                    return;
                }

                let (w, h) = downsample_dims(t.width, t.height, 512);
                let _ = on_event.send(LevelEvent::Texture {
                    texture: TextureDto {
                        id: t.id,
                        width: w,
                        height: h,
                    },
                });
                tex_done += 1;
                let _ = on_event.send(LevelEvent::Progress { current: tex_done });
                chunk_yield(tex_done);
            },
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn run_level_stream_rfom(path: &Path, on_event: &Channel<LevelEvent>) -> Result<(), String> {
    let _ = on_event.send(LevelEvent::Phase {
        phase: "layout",
        label: "Reading placements",
        total: 1,
        chunk_size: CHUNK_SIZE,
    });

    let mut wanted_moby_ids: HashSet<u64> = HashSet::new();
    if let Ok(gp) = lunalib::read_gameplay_rfom(path) {
        for region in gp.regions {
            for inst in region.moby_instances {
                wanted_moby_ids.insert(inst.moby_tuid);
            }
        }
    }
    let mut wanted_tie_tuids: HashSet<u64> = HashSet::new();
    if let Ok(insts) = lunalib::read_tie_instances_rfom(path) {
        for i in insts {
            wanted_tie_tuids.insert(i.tie_tuid);
        }
    }
    let _ = on_event.send(LevelEvent::Progress { current: 1 });

    let _ = on_event.send(LevelEvent::Phase {
        phase: "shaders",
        label: "Reading shaders",
        total: 1,
        chunk_size: CHUNK_SIZE,
    });
    let shaders: HashMap<u64, ShaderInfo> = lunalib::read_shaders_rfom(path)
        .map_err(|e| e.to_string())?;
    let _ = on_event.send(LevelEvent::Progress { current: 1 });

    let mut needed_albedo: HashSet<u32> = HashSet::new();

    let mut moby_assets: Vec<lunalib::MobyAsset> = Vec::new();
    lunalib::read_moby_assets_rfom(path, |a| {
        if wanted_moby_ids.is_empty() || wanted_moby_ids.contains(&a.tuid) {
            moby_assets.push(a);
        }
    })
        .map_err(|e| e.to_string())?;
    let _ = on_event.send(LevelEvent::Phase {
        phase: "mobys",
        label: "Decoding mobys",
        total: moby_assets.len(),
        chunk_size: CHUNK_SIZE,
    });
    let mut moby_done = 0usize;
    for asset in moby_assets {
        let mut submeshes = Vec::new();
        for bangle in asset.bangles {
            for m in bangle.meshes {
                let (albedo, normal, emissive) = resolve_shader_textures(
                    &shaders,
                    &asset.shader_tuids,
                    m.shader_index as usize,
                );
                for id in [albedo, normal, emissive].into_iter().flatten() {
                    needed_albedo.insert(id);
                }
                submeshes.push(mesh_dto(
                    m.positions,
                    m.uvs,
                    m.indices,
                    albedo,
                    normal,
                    emissive,
                    m.bone_indices,
                    m.bone_weights,
                ));
            }
        }
        let skeleton = build_skeleton_dto(&asset.skeleton);
        let dto = AssetMeshesDto {
            asset_tuid: format!("0x{:016X}", asset.tuid),
            name: asset.name.clone(),
            submeshes,
            skeleton,
            animset_hash: None,
            bind_pose_inverse_offset: asset.bind_pose_inverse_offset,
            embedded_animation_count: asset.rfom_anim_offsets.len() as u32,
        };
        let _ = on_event.send(LevelEvent::MobyAsset { asset: dto });
        moby_done += 1;
        let _ = on_event.send(LevelEvent::Progress { current: moby_done });
        chunk_yield(moby_done);
    }

    let mut tie_assets: Vec<lunalib::TieAsset> = Vec::new();
    lunalib::read_tie_assets_rfom(path, |a| {
        if wanted_tie_tuids.is_empty() || wanted_tie_tuids.contains(&a.tuid) {
            tie_assets.push(a);
        }
    })
        .map_err(|e| e.to_string())?;
    let _ = on_event.send(LevelEvent::Phase {
        phase: "ties",
        label: "Decoding ties",
        total: tie_assets.len(),
        chunk_size: CHUNK_SIZE,
    });
    let mut tie_done = 0usize;
    for asset in tie_assets {
        let submeshes: Vec<MeshDto> = asset
            .meshes
            .into_iter()
            .map(|m| {
                let (albedo, normal, emissive) = resolve_shader_textures(
                    &shaders,
                    &asset.shader_tuids,
                    m.shader_index as usize,
                );
                for id in [albedo, normal, emissive].into_iter().flatten() {
                    needed_albedo.insert(id);
                }
                mesh_dto(
                    m.positions,
                    m.uvs,
                    m.indices,
                    albedo,
                    normal,
                    emissive,
                    Vec::new(),
                    Vec::new(),
                )
            })
            .collect();
        let dto = AssetMeshesDto {
            asset_tuid: format!("0x{:016X}", asset.tuid),
            name: format!("tie_{:016X}", asset.tuid),
            submeshes,
            skeleton: None,
            animset_hash: None,
            bind_pose_inverse_offset: 0,
            embedded_animation_count: 0,
        };
        let _ = on_event.send(LevelEvent::TieAsset { asset: dto });
        tie_done += 1;
        let _ = on_event.send(LevelEvent::Progress { current: tie_done });
        chunk_yield(tie_done);
    }

    let zones = lunalib::read_regions_rfom(path).unwrap_or_default();
    let total_ufrags: usize = zones.iter().map(|z| z.ufrags.len()).sum();
    let _ = on_event.send(LevelEvent::Phase {
        phase: "ufrags",
        label: "Decoding terrain",
        total: total_ufrags,
        chunk_size: CHUNK_SIZE,
    });
    let mut ufrag_done = 0usize;
    for zone in zones {
        let zone_tuid_hex = format!("0x{:016X}", zone.tuid);
        for u in zone.ufrags {
            if u.positions.is_empty() || u.indices.is_empty() {
                continue;
            }
            let shader_info = zone
                .ufrag_shader_tuids
                .get(u.shader_index as usize)
                .and_then(|st| shaders.get(st));
            let albedo = shader_info.and_then(|s| s.albedo_tex_id);
            let normal = shader_info.and_then(|s| s.normal_tex_id);
            let emissive = shader_info.and_then(|s| s.expensive_tex_id);
            for id in [albedo, normal, emissive].into_iter().flatten() {
                needed_albedo.insert(id);
            }
            let dto = UFragMeshDto {
                tuid: format!("0x{:016X}", u.tuid),
                zone_tuid: zone_tuid_hex.clone(),
                position: u.position,
                mesh: mesh_dto(
                    u.positions,
                    u.uvs,
                    u.indices,
                    albedo,
                    normal,
                    emissive,
                    Vec::new(),
                    Vec::new(),
                ),
            };
            let _ = on_event.send(LevelEvent::UfragMesh { mesh: dto });
            ufrag_done += 1;
            let _ = on_event.send(LevelEvent::Progress { current: ufrag_done });
            chunk_yield(ufrag_done);
        }
    }

    if needed_albedo.is_empty() {
        let _ = on_event.send(LevelEvent::Phase {
            phase: "textures",
            label: "Decoding textures",
            total: 0,
            chunk_size: CHUNK_SIZE,
        });
    } else {
        let textures = lunalib::read_textures_rfom(path).unwrap_or_default();
        let needed = needed_albedo.clone();
        let filtered: Vec<&lunalib::Texture> = textures
            .iter()
            .filter(|t| needed.contains(&t.id))
            .collect();
        let _ = on_event.send(LevelEvent::Phase {
            phase: "textures",
            label: "Decoding textures",
            total: filtered.len(),
            chunk_size: CHUNK_SIZE,
        });
        let mut tex_done = 0usize;
        for t in filtered {
            if t.rgba.is_empty() {
                continue;
            }
            let (w, h) = downsample_dims(t.width, t.height, 512);
            let _ = on_event.send(LevelEvent::Texture {
                texture: TextureDto {
                    id: t.id,
                    width: w,
                    height: h,
                },
            });
            tex_done += 1;
            let _ = on_event.send(LevelEvent::Progress { current: tex_done });
            chunk_yield(tex_done);
        }
    }

    Ok(())
}



#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LibraryEvent {

    Missing,

    Located { path: String },

    Total { total: usize },

    Asset { asset: AssetMeshesDto },

    Texture { texture: TextureDto },
    Done,
    Error { message: String },
}


fn character_library_candidates(level_path: &Path) -> Vec<std::path::PathBuf> {
    let mut candidates = vec![
        level_path.join("entities").join("character"),
        level_path.join("character"),
    ];
    let mut cur = level_path.parent().map(|p| p.to_path_buf());
    for _ in 0..15 {
        let Some(p) = cur.clone() else { break };
        candidates.push(p.join("entities").join("character"));
        candidates.push(p.join("character"));
        cur = p.parent().map(|x| x.to_path_buf());
    }
    candidates
}


fn find_character_library(level_path: &Path) -> Option<std::path::PathBuf> {
    character_library_candidates(level_path)
        .into_iter()
        .find(|c| c.is_dir() && c.join("assetlookup.dat").exists())
}


fn find_gltf_library_dir(level_path: &Path) -> Option<std::path::PathBuf> {
    let candidates = character_library_candidates(level_path);
    eprintln!(
        "find_gltf_library_dir: trying {} candidates from level={}",
        candidates.len(),
        level_path.display()
    );
    for (i, c) in candidates.iter().enumerate() {
        let exists = c.is_dir();
        eprintln!(
            "  [{}] {} — {}",
            i,
            c.display(),
            if exists { "MATCH" } else { "miss" }
        );
        if exists {
            return Some(c.clone());
        }
    }
    None
}

#[tauri::command]
fn level_character_library_stream(
    folder: String,
    on_event: Channel<LibraryEvent>,
) -> Result<(), String> {
    let level_path = Path::new(&folder);
    let Some(char_path) = find_character_library(level_path) else {
        let _ = on_event.send(LibraryEvent::Missing);
        let _ = on_event.send(LibraryEvent::Done);
        return Ok(());
    };
    let _ = on_event.send(LibraryEvent::Located {
        path: char_path.display().to_string(),
    });
    if let Err(message) = run_library_stream(&char_path, &on_event) {
        let _ = on_event.send(LibraryEvent::Error { message: message.clone() });
        return Err(message);
    }
    let _ = on_event.send(LibraryEvent::Done);
    Ok(())
}

fn run_library_stream(
    folder: &Path,
    on_event: &Channel<LibraryEvent>,
) -> Result<(), String> {

    let shaders: HashMap<u64, ShaderInfo> =
        read_shaders(folder).map_err(|e| e.to_string())?;

    let mut needed_albedo: HashSet<u32> = HashSet::new();


    let mut done = 0usize;
    read_moby_assets_with_total(
        folder,
        None,
        |total| {
            let _ = on_event.send(LibraryEvent::Total { total });
        },
        |asset| {
            let mut submeshes = Vec::new();
            for bangle in asset.bangles {
                for m in bangle.meshes {
                    let (albedo, normal, emissive) = resolve_shader_textures(
                        &shaders,
                        &asset.shader_tuids,
                        m.shader_index as usize,
                    );
                    for id in [albedo, normal, emissive].into_iter().flatten() {
                        needed_albedo.insert(id);
                    }
                    submeshes.push(mesh_dto(
                        m.positions,
                        m.uvs,
                        m.indices,
                        albedo,
                        normal,
                        emissive,
                        m.bone_indices,
                        m.bone_weights,
                    ));
                }
            }

            let skeleton = build_skeleton_dto(&asset.skeleton);
            let dto = AssetMeshesDto {
                asset_tuid: format!("0x{:016X}", asset.tuid),
                name: asset.name.clone(),
                submeshes,
                skeleton,
                animset_hash: asset.animset_hash.map(|h| format!("0x{:016X}", h)),
                bind_pose_inverse_offset: asset.bind_pose_inverse_offset,
                embedded_animation_count: asset.rfom_anim_offsets.len() as u32,
            };
            let _ = on_event.send(LibraryEvent::Asset { asset: dto });
            done += 1;
            chunk_yield(done);
        },
    )
    .map_err(|e| e.to_string())?;


    if needed_albedo.is_empty() {
        return Ok(());
    }
    let needed = needed_albedo.clone();
    let mut tex_done = 0usize;
    read_textures_with_total(
        folder,
        move |id| needed.contains(&id),
        |_total| {},
        |t| {
            if !t.is_decoded() {
                return;
            }

            let (w, h) = downsample_dims(t.width, t.height, 512);
            let _ = on_event.send(LibraryEvent::Texture {
                texture: TextureDto {
                    id: t.id,
                    width: w,
                    height: h,
                },
            });
            tex_done += 1;
            chunk_yield(tex_done);
        },
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}



#[derive(Serialize)]
struct PsarcEntryDto {
    name: String,
    uncompressed_size: u64,
    file_offset: u64,
}

#[derive(Serialize)]
struct PsarcListDto {
    major: u16,
    minor: u16,
    compression: &'static str,
    block_size: u32,
    entry_count: usize,
    entries: Vec<PsarcEntryDto>,
}

#[tauri::command]
fn psarc_list(path: String) -> Result<PsarcListDto, String> {
    let archive = psarc::Archive::open(Path::new(&path)).map_err(|e| e.to_string())?;
    let compression = match archive.header.compression {
        psarc::Compression::Zlib => "zlib",
        psarc::Compression::Lzma => "lzma",
        psarc::Compression::Oodle => "oodle",
    };
    let entries = archive
        .entries
        .iter()
        .map(|e| PsarcEntryDto {
            name: e.name.clone(),
            uncompressed_size: e.uncompressed_size,
            file_offset: e.file_offset,
        })
        .collect();
    Ok(PsarcListDto {
        major: archive.header.major,
        minor: archive.header.minor,
        compression,
        block_size: archive.header.block_size,
        entry_count: archive.entries.len(),
        entries,
    })
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PsarcEvent {

    Total { total: usize },

    File {
        index: usize,
        name: String,
        bytes: u64,
    },
    Done,
    Error { message: String },
}

#[tauri::command]
fn psarc_extract_stream(
    input: String,
    output: String,
    on_event: Channel<PsarcEvent>,
) -> Result<(), String> {
    if let Err(message) = run_psarc_extract(&input, &output, &on_event) {
        let _ = on_event.send(PsarcEvent::Error { message: message.clone() });
        return Err(message);
    }
    let _ = on_event.send(PsarcEvent::Done);
    Ok(())
}

fn run_psarc_extract(
    input: &str,
    output: &str,
    on_event: &Channel<PsarcEvent>,
) -> Result<(), String> {
    let mut archive = psarc::Archive::open(Path::new(input)).map_err(|e| e.to_string())?;
    let out_root = Path::new(output);
    std::fs::create_dir_all(out_root).map_err(|e| format!("create out dir: {e}"))?;

    let total = archive.entries.len();
    let _ = on_event.send(PsarcEvent::Total { total });


    let entries: Vec<_> = archive.entries.clone();

    for (i, entry) in entries.iter().enumerate() {
        let bytes = archive.read_entry(entry).map_err(|e| e.to_string())?;


        let mut rel = entry.name.replace('\\', "/");
        while rel.starts_with('/') {
            rel.remove(0);
        }
        if rel.split('/').any(|seg| seg == "..") {
            return Err(format!(
                "path traversal attempt blocked for entry: {}",
                entry.name
            ));
        }


        let dest = out_root.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }


        write_bytes_to_path(&dest, &bytes)
            .map_err(|e| format!("write {dest:?}: {e}"))?;

        let _ = on_event.send(PsarcEvent::File {
            index: i + 1,
            name: entry.name.clone(),
            bytes: bytes.len() as u64,
        });
    }
    Ok(())
}


#[derive(Serialize)]
struct AssetLookupKindDto {
    name: String,
    section_id: u32,
    count: usize,
    has_decoder: bool,
}

#[derive(Serialize)]
struct AssetLookupOverviewDto {
    layout: String,
    version_major: u16,
    version_minor: u16,
    kinds: Vec<AssetLookupKindDto>,
}

#[tauri::command]
fn asset_lookup_inspect(path: String) -> Result<AssetLookupOverviewDto, String> {
    let overview = lunalib::inspect_assetlookup(Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(AssetLookupOverviewDto {
        layout: overview.layout.to_string(),
        version_major: overview.version_major,
        version_minor: overview.version_minor,
        kinds: overview
            .kinds
            .into_iter()
            .map(|k| AssetLookupKindDto {
                name: k.name.to_string(),
                section_id: k.section_id,
                count: k.count,
                has_decoder: k.has_decoder,
            })
            .collect(),
    })
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AssetLookupExtractEvent {
    Total { kind: String, count: usize },
    Entry {
        kind: String,
        index: usize,
        tuid: String,
        ok: bool,
        message: Option<String>,
    },
    KindDone { kind: String },
    Done,
    Error { message: String },
}

#[tauri::command]
fn asset_lookup_extract_stream(
    input: String,
    output: String,
    kinds: Vec<String>,
    max_texture_dim: Option<u32>,
    on_event: Channel<AssetLookupExtractEvent>,
) -> Result<(), String> {
    if let Err(message) =
        run_asset_lookup_extract(&input, &output, &kinds, max_texture_dim, &on_event)
    {
        let _ = on_event.send(AssetLookupExtractEvent::Error {
            message: message.clone(),
        });
        return Err(message);
    }
    let _ = on_event.send(AssetLookupExtractEvent::Done);
    Ok(())
}

fn run_asset_lookup_extract(
    input: &str,
    output: &str,
    kind_names: &[String],
    max_texture_dim: Option<u32>,
    on_event: &Channel<AssetLookupExtractEvent>,
) -> Result<(), String> {
    let input_path = Path::new(input);
    let output_path = Path::new(output);

    let kinds = parse_asset_kinds(kind_names)?;
    if kinds.is_empty() {
        return Err("no asset kinds selected".to_string());
    }

    std::fs::create_dir_all(output_path).map_err(|e| format!("create out dir: {e}"))?;

    let options = lunalib::ExtractOptions {
        kinds,
        max_texture_dim: max_texture_dim.unwrap_or(4096),
    };

    let on_event_cl = on_event.clone();
    lunalib::extract_assetlookup(input_path, output_path, &options, move |ev| match ev {
        lunalib::ExtractEvent::Total { kind, count } => {
            let _ = on_event_cl.send(AssetLookupExtractEvent::Total {
                kind: kind.name().to_string(),
                count,
            });
        }
        lunalib::ExtractEvent::Entry { kind, index, tuid, ok, message } => {
            let _ = on_event_cl.send(AssetLookupExtractEvent::Entry {
                kind: kind.name().to_string(),
                index,
                tuid: format!("0x{:016X}", tuid),
                ok,
                message,
            });
        }
        lunalib::ExtractEvent::KindDone { kind } => {
            let _ = on_event_cl.send(AssetLookupExtractEvent::KindDone {
                kind: kind.name().to_string(),
            });
        }
    })
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn parse_asset_kinds(names: &[String]) -> Result<Vec<lunalib::AssetKind>, String> {
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let kind = match name.as_str() {
            "shader" => lunalib::AssetKind::Shader,
            "texture" => lunalib::AssetKind::Texture,
            "highmip" => lunalib::AssetKind::HighMip,
            "cubemap" => lunalib::AssetKind::Cubemap,
            "tie" => lunalib::AssetKind::Tie,
            "foliage" => lunalib::AssetKind::Foliage,
            "shrub" => lunalib::AssetKind::Shrub,
            "moby" => lunalib::AssetKind::Moby,
            "animset" => lunalib::AssetKind::Animset,
            "cinematic" => lunalib::AssetKind::Cinematic,
            "zone" => lunalib::AssetKind::Zone,
            "lighting" => lunalib::AssetKind::Lighting,
            other => return Err(format!("unknown asset kind: {other}")),
        };
        out.push(kind);
    }
    Ok(out)
}

fn write_bytes_to_path(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();

        if !s.starts_with(r"\\?\") && !s.starts_with(r"\\.\") {

            let abs: std::path::PathBuf = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            };

            let normalized = abs.display().to_string().replace('/', "\\");
            let prefixed = format!(r"\\?\{}", normalized);
            return std::fs::write(prefixed, bytes);
        }
    }
    std::fs::write(path, bytes)
}


#[tauri::command]
fn write_bytes(path: String, bytes: Vec<u8>) -> Result<(), String> {
    if let Some(parent) = Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }
    }
    std::fs::write(&path, &bytes).map_err(|e| format!("write {path}: {e}"))
}


#[derive(Serialize)]
struct GltfFileDto {

    name: String,

    path: String,

    extension: String,
    size_bytes: u64,

    category: String,
}

#[derive(Serialize)]
struct GltfLibraryDto {

    folder: String,
    files: Vec<GltfFileDto>,
}


#[tauri::command]
fn list_character_gltfs(folder: String) -> Result<GltfLibraryDto, String> {
    let level_path = Path::new(&folder);

    let Some(char_path) = find_gltf_library_dir(level_path) else {
        eprintln!(
            "list_character_gltfs: no character/ directory found near {}",
            level_path.display()
        );
        return Ok(GltfLibraryDto {
            folder: String::new(),
            files: Vec::new(),
        });
    };

    eprintln!(
        "list_character_gltfs: scanning {}",
        char_path.display()
    );
    let mut files: Vec<GltfFileDto> = Vec::new();
    walk_gltf(&char_path, "character", &mut files).map_err(|e| e.to_string())?;
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    eprintln!(
        "list_character_gltfs: found {} files at {}",
        files.len(),
        char_path.display()
    );

    Ok(GltfLibraryDto {
        folder: char_path.display().to_string(),
        files,
    })
}

fn walk_gltf(dir: &Path, category: &str, out: &mut Vec<GltfFileDto>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ftype = entry.file_type()?;
        if ftype.is_dir() {
            walk_gltf(&path, category, out)?;
        } else if ftype.is_file() {
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            if matches!(ext.as_deref(), Some("gltf") | Some("glb")) {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(GltfFileDto {
                    name,
                    path: path.display().to_string(),
                    extension: ext.unwrap_or_default(),
                    size_bytes: size,
                    category: category.to_string(),
                });
            }
        }
    }
    Ok(())
}


fn find_entities_dir(level_path: &Path) -> Option<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = vec![level_path.join("entities")];
    let mut cur = level_path.parent().map(|p| p.to_path_buf());
    for _ in 0..15 {
        let Some(p) = cur.clone() else { break };
        candidates.push(p.join("entities"));
        cur = p.parent().map(|x| x.to_path_buf());
    }
    eprintln!(
        "find_entities_dir: trying {} candidates from level={}",
        candidates.len(),
        level_path.display()
    );
    for (i, c) in candidates.iter().enumerate() {
        let exists = c.is_dir();
        eprintln!(
            "  [{}] {} — {}",
            i,
            c.display(),
            if exists { "MATCH" } else { "miss" }
        );
        if exists {
            return Some(c.clone());
        }
    }
    None
}


#[tauri::command]
fn list_entities_gltfs(folder: String) -> Result<GltfLibraryDto, String> {
    let level_path = Path::new(&folder);
    let Some(entities_root) = find_entities_dir(level_path) else {
        eprintln!(
            "list_entities_gltfs: no entities/ directory found near {}",
            level_path.display()
        );
        return Ok(GltfLibraryDto {
            folder: String::new(),
            files: Vec::new(),
        });
    };

    eprintln!(
        "list_entities_gltfs: scanning {}",
        entities_root.display()
    );

    let mut files: Vec<GltfFileDto> = Vec::new();
    let entries = std::fs::read_dir(&entities_root).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let ftype = entry.file_type().map_err(|e| e.to_string())?;
        if ftype.is_dir() {
            let category = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("other")
                .to_string();
            walk_gltf(&path, &category, &mut files).map_err(|e| e.to_string())?;
        } else if ftype.is_file() {

            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            if matches!(ext.as_deref(), Some("gltf") | Some("glb")) {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push(GltfFileDto {
                    name,
                    path: path.display().to_string(),
                    extension: ext.unwrap_or_default(),
                    size_bytes: size,
                    category: "other".to_string(),
                });
            }
        }
    }

    files.sort_by(|a, b| {
        a.category
            .to_lowercase()
            .cmp(&b.category.to_lowercase())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    eprintln!(
        "list_entities_gltfs: found {} files at {}",
        files.len(),
        entities_root.display()
    );

    Ok(GltfLibraryDto {
        folder: entities_root.display().to_string(),
        files,
    })
}


#[tauri::command]

fn read_file_bytes(path: String) -> Result<tauri::ipc::Response, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}


#[derive(Serialize)]
struct DecodedBoneDto {

    rotations: Vec<f32>,

    translations: Vec<f32>,
    scales: Vec<f32>,
    rotation_animated: bool,
    translation_animated: bool,
    scale_animated: bool,
}


#[derive(Serialize)]
struct DecodedClipDto {
    name: String,
    num_frames: u16,
    frame_rate: f32,
    looping: bool,

    bones: Vec<DecodedBoneDto>,
}


#[derive(Serialize)]
struct GlbMaterialTexturesDto {

    material_name: String,

    albedo_path: Option<String>,

    normal_path: Option<String>,

    emissive_path: Option<String>,
}


#[tauri::command]
fn find_glb_textures(
    level_folder: String,
    material_names: Vec<String>,
) -> Result<Vec<GlbMaterialTexturesDto>, String> {
    let textures_root = Path::new(&level_folder).join("textures");
    if !textures_root.is_dir() {

        eprintln!(
            "find_glb_textures: no textures/ at {}",
            textures_root.display()
        );
        return Ok(material_names
            .into_iter()
            .map(|n| GlbMaterialTexturesDto {
                material_name: n,
                albedo_path: None,
                normal_path: None,
                emissive_path: None,
            })
            .collect());
    }


    let mut by_stem: HashMap<String, std::path::PathBuf> = HashMap::new();
    walk_dds_files(&textures_root, &mut by_stem).map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(material_names.len());
    for name in material_names {

        let base = name
            .rsplit(|c: char| c == '/' || c == '\\')
            .next()
            .unwrap_or(&name)
            .to_string();
        let albedo_path = by_stem
            .get(&format!("{}_c", base))
            .map(|p| p.display().to_string());
        let normal_path = by_stem
            .get(&format!("{}_n", base))
            .map(|p| p.display().to_string());
        let emissive_path = by_stem
            .get(&format!("{}_e", base))
            .map(|p| p.display().to_string());
        out.push(GlbMaterialTexturesDto {
            material_name: name,
            albedo_path,
            normal_path,
            emissive_path,
        });
    }

    Ok(out)
}


fn walk_dds_files(
    dir: &Path,
    out: &mut HashMap<String, std::path::PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ftype = entry.file_type()?;
        if ftype.is_dir() {
            walk_dds_files(&path, out)?;
        } else if ftype.is_file() {
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            if matches!(ext.as_deref(), Some("dds")) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {

                    out.insert(stem.to_string(), path.clone());
                }
            }
        }
    }
    Ok(())
}


#[derive(Serialize)]
struct AnimsetSummaryDto {

    tuid_hex: String,

    name: String,

    num_frames: u16,
    frame_rate: f32,

    num_bones: u16,
    looping: bool,
}


#[tauri::command]
fn list_animset_clips(level_folder: String) -> Result<Vec<AnimsetSummaryDto>, String> {
    match lunalib::detect_layout(Path::new(&level_folder)) {
        Ok(lunalib::LevelLayout::Tod) | Ok(lunalib::LevelLayout::Rfom) => {
            return Ok(Vec::new());
        }
        _ => {}
    }
    let mut lookup = open_lookup(&level_folder)?;
    let ptrs = lookup
        .pointers(AssetKind::Animset)
        .map_err(|e| format!("read animset table: {e}"))?;

    let path = Path::new(&level_folder).join("animsets.dat");
    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {

            eprintln!("list_animset_clips: no animsets.dat at {} ({e})", path.display());
            return Ok(Vec::new());
        }
    };

    let mut out: Vec<AnimsetSummaryDto> = Vec::new();
    use std::io::{Read, Seek, SeekFrom};
    for ptr in ptrs {
        if let Err(e) = file.seek(SeekFrom::Start(u64::from(ptr.offset))) {
            eprintln!("list_animset_clips: seek failed for 0x{:016X}: {e}", ptr.tuid);
            continue;
        }
        let mut buf = vec![0u8; ptr.length as usize];
        if let Err(e) = file.read_exact(&mut buf) {
            eprintln!("list_animset_clips: read failed for 0x{:016X}: {e}", ptr.tuid);
            continue;
        }
        let mut ig = match IgFile::open(std::io::Cursor::new(buf)) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let h = match read_animation_header(&mut ig) {
            Ok(Some(h)) => h,
            _ => continue,
        };
        out.push(AnimsetSummaryDto {
            tuid_hex: format!("0x{:016X}", ptr.tuid),
            name: h.name.clone(),
            num_frames: h.num_frames,
            frame_rate: h.frame_rate,
            num_bones: h.num_bones,
            looping: h.is_looping(),
        });
    }

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}


#[tauri::command]
fn fetch_animset_clip(
    level_folder: String,
    animset_hash_hex: String,
    position_scale: f32,
    scale_scale: f32,
) -> Result<DecodedClipDto, String> {
    let target = parse_hex_u64(&animset_hash_hex)?;


    let mut lookup = open_lookup(&level_folder)?;
    let ptrs = lookup
        .pointers(AssetKind::Animset)
        .map_err(|e| format!("read animset table: {e}"))?;
    let ptr = ptrs
        .iter()
        .find(|p| p.tuid == target)
        .ok_or_else(|| format!("animset 0x{:016X} not in 0x1D700 table", target))?;


    let path = Path::new(&level_folder).join("animsets.dat");
    let mut file =
        File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(u64::from(ptr.offset)))
        .map_err(|e| format!("seek animsets.dat: {e}"))?;
    let mut buf = vec![0u8; ptr.length as usize];
    file.read_exact(&mut buf)
        .map_err(|e| format!("read animsets.dat: {e}"))?;


    let mut ig = IgFile::open(std::io::Cursor::new(buf))
        .map_err(|e| format!("animset IGHW: {e}"))?;
    let header = read_animation_header(&mut ig)
        .map_err(|e| format!("animation header: {e}"))?
        .ok_or_else(|| {
            "animset chunk has no 0xF000 Animation section".to_string()
        })?;
    let ctrl = read_animation_control(&mut ig, &header)
        .map_err(|e| format!("animation control: {e}"))?;
    let clip = decode_animation(&mut ig, &header, &ctrl, position_scale, scale_scale)
        .map_err(|e| format!("animation decode: {e}"))?;


    Ok(DecodedClipDto {
        name: clip.name,
        num_frames: clip.num_frames,
        frame_rate: clip.frame_rate,
        looping: clip.looping,
        bones: clip
            .bones
            .into_iter()
            .map(|b| DecodedBoneDto {
                rotations: b.rotations,
                translations: b.translations,
                scales: b.scales,
                rotation_animated: b.rotation_animated,
                translation_animated: b.translation_animated,
                scale_animated: b.scale_animated,
            })
            .collect(),
    })
}


fn parse_hex_u64(s: &str) -> Result<u64, String> {
    let trimmed = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(trimmed, 16).map_err(|e| format!("invalid hex u64 {s:?}: {e}"))
}


#[tauri::command]
fn list_gltfs_in_folder(path: String) -> Result<GltfLibraryDto, String> {
    let root = Path::new(&path);
    if !root.is_dir() {
        return Err(format!("not a directory: {path}"));
    }

    let mut files: Vec<GltfFileDto> = Vec::new();
    let entries = std::fs::read_dir(root).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path();
        let ftype = entry.file_type().map_err(|e| e.to_string())?;
        if ftype.is_dir() {
            let category = entry_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("other")
                .to_string();
            walk_gltf(&entry_path, &category, &mut files).map_err(|e| e.to_string())?;
        } else if ftype.is_file() {
            let ext = entry_path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            if matches!(ext.as_deref(), Some("gltf") | Some("glb")) {
                let name = entry_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push(GltfFileDto {
                    name,
                    path: entry_path.display().to_string(),
                    extension: ext.unwrap_or_default(),
                    size_bytes: size,
                    category: "other".to_string(),
                });
            }
        }
    }
    files.sort_by(|a, b| {
        a.category
            .to_lowercase()
            .cmp(&b.category.to_lowercase())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    eprintln!(
        "list_gltfs_in_folder: found {} files at {}",
        files.len(),
        path
    );
    Ok(GltfLibraryDto {
        folder: path,
        files,
    })
}


#[derive(Serialize)]
struct SoundEntryDto {
    name: String,

    index: usize,

    kind: &'static str,

    source: String,
}


#[tauri::command]
fn list_level_sounds(level_folder: String) -> Result<Vec<SoundEntryDto>, String> {
    let folder = Path::new(&level_folder);
    let read_dir = match std::fs::read_dir(folder) {
        Ok(d) => d,
        Err(e) => return Err(format!("read_dir {}: {e}", folder.display())),
    };

    let mut candidates: Vec<String> = Vec::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".dat") || lower.contains("stream") {
            continue;
        }
        let is_bank = lower == "resident_sound.dat"
            || lower == "ps3sound.dat"
            || lower.starts_with("resident_dialogue")
            || lower.starts_with("ps3dialogue");
        if is_bank {
            candidates.push(name);
        }
    }
    candidates.sort();

    let mut out: Vec<SoundEntryDto> = Vec::new();
    for filename in &candidates {
        let path = folder.join(filename);
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[list_level_sounds] open {}: {e}", path.display());
                continue;
            }
        };
        let mut ig = match IgFile::open(BufReader::new(file)) {
            Ok(ig) => ig,
            Err(e) => {
                eprintln!("[list_level_sounds] IgFile {}: {e}", path.display());
                continue;
            }
        };
        let summaries = match list_sounds_in(&mut ig) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[list_level_sounds] list_sounds {}: {e}", path.display());
                continue;
            }
        };

        let sibling_exists = lunalib::streaming_sibling_for(filename)
            .map(|s| folder.join(s).is_file())
            .unwrap_or(false);
        for s in summaries {
            let kind = match s.kind {
                SoundKind::Bank => "bank",
                SoundKind::Stream => {
                    if sibling_exists { "stream" } else { "stream-missing" }
                }
            };
            out.push(SoundEntryDto {
                name: s.name,
                index: s.index,
                kind,
                source: filename.clone(),
            });
        }
    }


    let bank_lookup: HashSet<String> =
        candidates.iter().map(|s| s.to_ascii_lowercase()).collect();
    let read_dir2 = std::fs::read_dir(folder)
        .map_err(|e| format!("read_dir {}: {e}", folder.display()))?;
    let mut stream_candidates: Vec<String> = Vec::new();
    for entry in read_dir2.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        let is_stream = lower.starts_with("streaming_sound")
            || lower.starts_with("streaming_dialogue")
            || lower.starts_with("ps3soundstream")
            || lower.starts_with("ps3dialoguestream");
        if is_stream && lower.ends_with(".dat") {
            stream_candidates.push(name);
        }
    }
    stream_candidates.sort();
    for stream_name in &stream_candidates {
        let expected_bank = lunalib::bank_pair_for(stream_name);
        if let Some(bank) = expected_bank {
            if bank_lookup.contains(&bank.to_ascii_lowercase()) {
                continue;
            }
        }
        let stream_path = folder.join(stream_name);
        let summaries = match lunalib::list_raw_streaming(&stream_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[list_level_sounds] raw scan {}: {e}",
                    stream_path.display()
                );
                continue;
            }
        };
        for s in summaries {
            out.push(SoundEntryDto {
                name: s.name,
                index: s.index,
                kind: "raw",
                source: stream_name.clone(),
            });
        }
    }

    Ok(out)
}


#[derive(Serialize)]
struct ExtractedSoundDto {
    name: String,
    sample_rate: u32,

    channels: u16,
    sample_count: u32,

    wav_b64: String,
}

fn list_sound_banks_in(folder: &Path) -> std::io::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(folder)?.flatten() {
        let n = entry.file_name().to_string_lossy().into_owned();
        let lower = n.to_ascii_lowercase();
        if !lower.ends_with(".dat") {
            continue;
        }
        if lower.contains("stream") {
            continue;
        }
        let is_bank = lower == "resident_sound.dat"
            || lower == "ps3sound.dat"
            || lower.starts_with("resident_dialogue")
            || (lower.starts_with("ps3dialogue") && !lower.starts_with("ps3dialoguestream"));
        if is_bank {
            out.push(n);
        }
    }
    out.sort();
    Ok(out)
}

fn cached_wav_path(level_folder: &Path, name: &str) -> std::path::PathBuf {
    let safe: String = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '.' => c,
            _ => '_',
        })
        .collect();
    level_folder
        .join("_rechimera_cache")
        .join("sounds")
        .join(format!("{}.wav", safe))
}

fn parse_wav_meta(wav: &[u8]) -> Option<(u32, u16, u32)> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12usize;
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 0;
    let mut data_len: u32 = 0;
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let len = u32::from_le_bytes([wav[i + 4], wav[i + 5], wav[i + 6], wav[i + 7]]) as usize;
        i += 8;
        if id == b"fmt " && len >= 16 && i + 16 <= wav.len() {
            channels = u16::from_le_bytes([wav[i + 2], wav[i + 3]]);
            sample_rate = u32::from_le_bytes([wav[i + 4], wav[i + 5], wav[i + 6], wav[i + 7]]);
        } else if id == b"data" {
            data_len = len as u32;
        }
        i += len + (len & 1);
    }
    if sample_rate == 0 || channels == 0 {
        return None;
    }
    let bytes_per_sample_per_channel = 2u32;
    let total_samples =
        data_len / (channels as u32 * bytes_per_sample_per_channel);
    Some((sample_rate, channels, total_samples))
}

#[tauri::command]
fn extract_one_stream_sound(
    level_folder: String,
    name: String,
    source: String,
) -> Result<ExtractedSoundDto, String> {
    lunalib::reset_scream_diag();
    let folder = Path::new(&level_folder);
    let target = name.trim();
    eprintln!(
        "[scream-stream] target='{}' bank='{}'",
        target, source
    );

    let cached_path = cached_wav_path(folder, &name);
    if cached_path.is_file() {
        if let Ok(bytes) = std::fs::read(&cached_path) {
            if let Some((sample_rate, channels, sample_count)) = parse_wav_meta(&bytes) {
                eprintln!("[scream-stream] cache hit → {}", cached_path.display());
                return Ok(ExtractedSoundDto {
                    name,
                    sample_rate,
                    channels,
                    sample_count,
                    wav_b64: BASE64.encode(&bytes),
                });
            }
        }
    }

    let bank_path = folder.join(&source);
    if !bank_path.is_file() {
        return Err(format!("missing bank {}", bank_path.display()));
    }
    let stream_filename = lunalib::streaming_sibling_for(&source)
        .ok_or_else(|| format!("no streaming sibling for {}", source))?;
    let stream_path = folder.join(stream_filename);
    if !stream_path.is_file() {
        return Err(format!(
            "missing stream sibling {} (this entry isn't playable in this build)",
            stream_path.display()
        ));
    }

    let file = File::open(&bank_path)
        .map_err(|e| format!("open {}: {e}", bank_path.display()))?;
    let mut ig = IgFile::open(BufReader::new(file)).map_err(|e| e.to_string())?;
    let mut errors: Vec<String> = Vec::new();
    let extracted = extract_stream_sounds(&mut ig, &stream_path, &mut errors)
        .map_err(|e| format!("extract_stream_sounds: {e}"))?;
    eprintln!(
        "[scream-stream] decoded {} stream entries from {} (errors={})",
        extracted.len(),
        source,
        errors.len()
    );

    let suffix_pattern = format!("{}_", target);
    let mut found_match: Option<lunalib::ExtractedSound> = None;
    for s in extracted {
        if s.name == target {
            found_match = Some(s);
            break;
        }
        if found_match.is_none() && s.name.starts_with(&suffix_pattern) {
            let tail = &s.name[suffix_pattern.len()..];
            if tail.chars().all(|c| c.is_ascii_digit()) {
                found_match = Some(s);
            }
        }
    }
    if let Some(found) = found_match {
        eprintln!(
            "[scream-stream] matched '{}' in {} ({} bytes)",
            found.name,
            source,
            found.wav.len()
        );
        if let Some(parent) = cached_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&cached_path, &found.wav);
        return Ok(ExtractedSoundDto {
            name: found.name,
            sample_rate: found.sample_rate,
            channels: found.channels,
            sample_count: found.sample_count,
            wav_b64: BASE64.encode(&found.wav),
        });
    }
    Err(format!(
        "stream sound '{}' not found in {} (decoded {} entries)",
        target, source, errors.len()
    ))
}

#[tauri::command]
fn extract_one_sound(
    level_folder: String,
    name: String,
    source: Option<String>,
) -> Result<ExtractedSoundDto, String> {
    lunalib::reset_scream_diag();
    let folder = Path::new(&level_folder);

    let cached_path = cached_wav_path(folder, &name);
    if cached_path.is_file() {
        if let Ok(bytes) = std::fs::read(&cached_path) {
            if let Some((sample_rate, channels, sample_count)) = parse_wav_meta(&bytes) {
                eprintln!("[scream-loop] cache hit for '{}' → {}", name, cached_path.display());
                return Ok(ExtractedSoundDto {
                    name,
                    sample_rate,
                    channels,
                    sample_count,
                    wav_b64: BASE64.encode(&bytes),
                });
            }
        }
    }

    let mut bank_candidates =
        list_sound_banks_in(folder).map_err(|e| format!("read_dir {}: {e}", folder.display()))?;
    if let Some(s) = &source {
        bank_candidates.sort_by_key(|n| if n == s { 0 } else { 1 });
    }
    if bank_candidates.is_empty() {
        return Err(format!("no sound bank found in {}", folder.display()));
    }

    let target = name.trim();
    eprintln!(
        "[scream-loop] extract_one_sound target='{}' source_hint={:?} candidates=[{}]",
        target,
        source,
        bank_candidates.join(", ")
    );
    let mut decode_diagnostics: Vec<String> = Vec::new();
    for filename in &bank_candidates {
        eprintln!("[scream-loop] trying {} …", filename);
        let path = folder.join(filename);
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[scream-loop]   open failed: {e}");
                decode_diagnostics.push(format!("open {}: {e}", filename));
                continue;
            }
        };
        let mut ig = match IgFile::open(BufReader::new(file)) {
            Ok(ig) => ig,
            Err(e) => {
                eprintln!("[scream-loop]   igfile failed: {e}");
                decode_diagnostics.push(format!("igfile {}: {e}", filename));
                continue;
            }
        };
        let extracted = match extract_bank_sounds_for_file(&mut ig, filename) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[scream-loop]   decode failed: {e}");
                decode_diagnostics.push(format!("decode {}: {e}", filename));
                continue;
            }
        };
        eprintln!(
            "[scream-loop]   {} returned {} ExtractedSound items",
            filename,
            extracted.len()
        );
        if extracted.is_empty() {
            decode_diagnostics.push(format!("decode {}: 0 sounds (V1 layout?)", filename));
        }
        let suffix_pattern = format!("{}_", target);
        let mut found_match: Option<lunalib::ExtractedSound> = None;
        for s in extracted {
            if s.name == target {
                found_match = Some(s);
                break;
            }
            if found_match.is_none() && s.name.starts_with(&suffix_pattern) {
                let tail = &s.name[suffix_pattern.len()..];
                if tail.chars().all(|c| c.is_ascii_digit()) {
                    found_match = Some(s);
                }
            }
        }
        if let Some(found) = found_match {
            eprintln!(
                "[scream-loop]   matched '{}' in {} (gain_index={}, wav={} bytes)",
                found.name,
                filename,
                found.gain_index,
                found.wav.len()
            );
            if let Some(parent) = cached_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&cached_path, &found.wav);
            return Ok(ExtractedSoundDto {
                name: found.name,
                sample_rate: found.sample_rate,
                channels: found.channels,
                sample_count: found.sample_count,
                wav_b64: BASE64.encode(&found.wav),
            });
        }
        eprintln!(
            "[scream-loop]   '{}' not in {}",
            target, filename
        );
    }
    let diag = if decode_diagnostics.is_empty() {
        String::new()
    } else {
        format!(" · {}", decode_diagnostics.join(" · "))
    };
    Err(format!(
        "sound '{}' not found in any bank (tried {}){}",
        target,
        bank_candidates.join(", "),
        diag
    ))
}


/// Bulk-extract every bank sound + every stream sound the level has,
/// write each WAV into a single .zip at `zip_out_path`, return the
/// count written. Skips already-extracted sounds in the level cache
/// only when reading is fast enough — usually a full bank pass is
/// cheaper than checking each cache file individually, so we just
/// re-extract.
///
/// The zip uses Store mode (no compression) since PCM WAV data
/// compresses poorly and the encode overhead would dominate. Filename
/// inside the zip is `<safe_sound_name>.wav`; collisions across banks
/// resolve by appending a `__<bankstem>` suffix to keep both files.
#[tauri::command]
fn bulk_extract_sounds_zip(
    level_folder: String,
    zip_out_path: String,
) -> Result<usize, String> {
    use std::io::Write;
    let folder = Path::new(&level_folder);
    let banks = list_sound_banks_in(folder)
        .map_err(|e| format!("read_dir {}: {e}", folder.display()))?;
    if banks.is_empty() {
        return Err(format!("no sound banks found in {}", folder.display()));
    }

    let zip_file = File::create(&zip_out_path)
        .map_err(|e| format!("create zip {zip_out_path}: {e}"))?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let opts: zip::write::FileOptions =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut written = 0usize;
    let mut errors: Vec<String> = Vec::new();

    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect::<String>()
    };

    for bank in &banks {
        let path = folder.join(bank);
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("open {bank}: {e}"));
                continue;
            }
        };
        let mut ig = match IgFile::open(BufReader::new(file)) {
            Ok(ig) => ig,
            Err(e) => {
                errors.push(format!("igfile {bank}: {e}"));
                continue;
            }
        };

        // Banks: parse once, write every contained sound to zip.
        match extract_bank_sounds_for_file(&mut ig, bank) {
            Ok(sounds) => {
                for s in sounds {
                    let mut name = format!("{}.wav", safe(&s.name));
                    if !seen.insert(name.clone()) {
                        let stem = bank.rsplit('.').nth(1).unwrap_or(bank);
                        name = format!("{}__{}.wav", safe(&s.name), safe(stem));
                        if !seen.insert(name.clone()) {
                            continue;
                        }
                    }
                    if let Err(e) = zip.start_file(&name, opts) {
                        errors.push(format!("zip entry {name}: {e}"));
                        continue;
                    }
                    if let Err(e) = zip.write_all(&s.wav) {
                        errors.push(format!("zip write {name}: {e}"));
                        continue;
                    }
                    written += 1;
                }
            }
            Err(e) => errors.push(format!("bank {bank}: {e}")),
        }

        // Streams: each bank has its own stream file (resolved inside
        // extract_stream_sounds via filename convention). Best-effort —
        // banks without a stream sidecar just no-op.
        let stream_filename = bank
            .strip_suffix(".dat")
            .map(|stem| format!("{stem}stream.dat"))
            .unwrap_or_else(|| format!("{bank}.stream"));
        let stream_path = folder.join(&stream_filename);
        if !stream_path.is_file() {
            continue;
        }
        let stream_file = match File::open(&stream_path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let mut sig = match IgFile::open(BufReader::new(stream_file)) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut stream_errors: Vec<String> = Vec::new();
        if let Ok(streams) =
            extract_stream_sounds(&mut sig, &stream_path, &mut stream_errors)
        {
            for s in streams {
                let mut name = format!("{}.wav", safe(&s.name));
                if !seen.insert(name.clone()) {
                    name = format!("{}__stream.wav", safe(&s.name));
                    if !seen.insert(name.clone()) {
                        continue;
                    }
                }
                if let Err(e) = zip.start_file(&name, opts) {
                    errors.push(format!("zip entry {name}: {e}"));
                    continue;
                }
                if let Err(e) = zip.write_all(&s.wav) {
                    errors.push(format!("zip write {name}: {e}"));
                    continue;
                }
                written += 1;
            }
        }
    }

    zip.finish()
        .map_err(|e| format!("zip finish: {e}"))?;
    eprintln!(
        "[bulk-sound-zip] wrote {} sounds to {} (errors={})",
        written,
        zip_out_path,
        errors.len()
    );
    Ok(written)
}

#[tauri::command]
fn extract_level_sounds(level_folder: String) -> Result<Vec<ExtractedSoundDto>, String> {
    let folder = Path::new(&level_folder);
    let read_dir = std::fs::read_dir(folder)
        .map_err(|e| format!("read_dir {}: {e}", folder.display()))?;
    let mut bank_candidates: Vec<String> = Vec::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        let is_bank = lower == "resident_sound.dat"
            || lower == "ps3sound.dat"
            || lower.starts_with("resident_dialogue")
            || lower.starts_with("ps3dialogue");
        if is_bank && lower.ends_with(".dat") {
            bank_candidates.push(name);
        }
    }
    bank_candidates.sort();
    if bank_candidates.is_empty() {
        return Err(format!(
            "no sound bank found in {} (expected resident_sound.dat or ps3sound.dat)",
            folder.display()
        ));
    }

    let mut out: Vec<ExtractedSoundDto> = Vec::new();
    for filename in &bank_candidates {
        let path = folder.join(filename);
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[extract_level_sounds] open {}: {e}", path.display());
                continue;
            }
        };
        let mut ig = match IgFile::open(BufReader::new(file)) {
            Ok(ig) => ig,
            Err(e) => {
                eprintln!("[extract_level_sounds] IgFile {}: {e}", path.display());
                continue;
            }
        };
        let extracted = match extract_bank_sounds_for_file(&mut ig, filename) {
            Ok(s) => s,
            Err(e) => {
                let dump = lunalib::dump_sound_bank_info(&mut ig)
                    .unwrap_or_else(|de| format!("(dump itself failed: {de})"));
                eprintln!(
                    "[extract_level_sounds] {} failed: {e}\n{dump}",
                    path.display()
                );
                continue;
            }
        };
        if extracted.is_empty() {
            let dump = lunalib::dump_sound_bank_info(&mut ig)
                .unwrap_or_else(|e| format!("(dump failed: {e})"));
            eprintln!(
                "[extract_level_sounds] {} returned 0 sounds — dumping structure:\n{dump}",
                path.display()
            );
        }
        for s in extracted {
            out.push(ExtractedSoundDto {
                name: s.name,
                sample_rate: s.sample_rate,
                channels: s.channels,
                sample_count: s.sample_count,
                wav_b64: BASE64.encode(&s.wav),
            });
        }
    }
    Ok(out)
}


#[tauri::command]
fn dump_sound_bank(
    level_folder: String,
    bank_filename: String,
) -> Result<String, String> {
    let path = Path::new(&level_folder).join(&bank_filename);
    if !path.is_file() {
        return Err(format!("missing bank {}", path.display()));
    }
    let file = File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut ig = IgFile::open(BufReader::new(file)).map_err(|e| e.to_string())?;
    lunalib::dump_sound_bank_info(&mut ig).map_err(|e| e.to_string())
}


#[tauri::command]
fn extract_level_stream_sounds(
    level_folder: String,
    bank_filename: String,
) -> Result<Vec<ExtractedSoundDto>, String> {
    let folder = Path::new(&level_folder);
    let bank_path = folder.join(&bank_filename);
    if !bank_path.is_file() {
        return Err(format!("missing bank {}", bank_path.display()));
    }
    let stream_filename = lunalib::streaming_sibling_for(&bank_filename)
        .ok_or_else(|| format!("unrecognized bank filename '{bank_filename}'"))?;
    let stream_path = folder.join(&stream_filename);
    if !stream_path.is_file() {
        return Err(format!("missing stream sibling {}", stream_path.display()));
    }
    let file =
        File::open(&bank_path).map_err(|e| format!("open {}: {e}", bank_path.display()))?;
    let mut ig = IgFile::open(BufReader::new(file)).map_err(|e| e.to_string())?;
    let mut errors: Vec<String> = Vec::new();
    let extracted = lunalib::extract_stream_sounds(&mut ig, &stream_path, &mut errors)
        .map_err(|e| e.to_string())?;
    if !errors.is_empty() {

        eprintln!(
            "[extract_level_stream_sounds] {} entries failed:",
            errors.len()
        );
        for e in &errors {
            eprintln!("  · {e}");
        }
    }
    Ok(extracted
        .into_iter()
        .map(|s| ExtractedSoundDto {
            name: s.name,
            sample_rate: s.sample_rate,
            channels: s.channels,
            sample_count: s.sample_count,
            wav_b64: BASE64.encode(&s.wav),
        })
        .collect())
}


#[tauri::command]
fn extract_raw_streaming_sounds(
    level_folder: String,
    stream_filename: String,
) -> Result<Vec<ExtractedSoundDto>, String> {
    let path = Path::new(&level_folder).join(&stream_filename);
    if !path.is_file() {
        return Err(format!("missing stream {}", path.display()));
    }
    let mut errors: Vec<String> = Vec::new();
    let extracted =
        lunalib::extract_raw_streaming(&path, &mut errors).map_err(|e| e.to_string())?;
    if !errors.is_empty() {
        eprintln!(
            "[extract_raw_streaming_sounds] {} entries failed (false-positive magics or unsupported formats):",
            errors.len()
        );
        for e in &errors {
            eprintln!("  · {e}");
        }
    }
    Ok(extracted
        .into_iter()
        .map(|s| ExtractedSoundDto {
            name: s.name,
            sample_rate: s.sample_rate,
            channels: s.channels,
            sample_count: s.sample_count,
            wav_b64: BASE64.encode(&s.wav),
        })
        .collect())
}



#[tauri::command]
fn get_level_texture_png(
    level_folder: String,
    texture_id: u32,
) -> Result<tauri::ipc::Response, String> {
    let path = Path::new(&level_folder);
    let mut found: Option<(Vec<u8>, u32, u32)> = None;
    read_textures_with_total(
        path,
        |id| id == texture_id,
        |_| {},
        |t| {
            if t.id == texture_id && t.is_decoded() {
                let (rgba, w, h) = downsample_rgba(t.rgba, t.width, t.height, 512);
                if !rgba.is_empty() {
                    let png = encode_png(&rgba, w, h);
                    if !png.is_empty() {
                        found = Some((png, w, h));
                    }
                }
            }
        },
    )
    .map_err(|e| e.to_string())?;
    let (png, _w, _h) = found.ok_or_else(|| {
        format!(
            "texture id {texture_id:#010x} not found in {}",
            path.display()
        )
    })?;
    Ok(tauri::ipc::Response::new(png))
}


#[tauri::command]
fn get_level_textures_bulk(
    level_folder: String,
    texture_ids: Vec<u32>,
) -> Result<tauri::ipc::Response, String> {
    let path = Path::new(&level_folder);

    let collected =
        bulk_extract_pngs(path, Some(&texture_ids), 512).map_err(|e| e.to_string())?;


    let payload_bytes: usize = collected.iter().map(|(_, p)| p.len()).sum();
    let mut out = Vec::with_capacity(4 + collected.len() * 8 + payload_bytes);
    out.extend_from_slice(&(collected.len() as u32).to_le_bytes());
    for (id, png) in &collected {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(png);
    }
    Ok(tauri::ipc::Response::new(out))
}


#[derive(Serialize)]
struct LevelFileDto {

    name: String,

    size_bytes: u64,

    category: &'static str,

    parsed: bool,
}


#[tauri::command]
fn list_level_files(level_folder: String) -> Result<Vec<LevelFileDto>, String> {
    let dir = Path::new(&level_folder);
    if !dir.is_dir() {
        return Err(format!("not a directory: {level_folder}"));
    }
    let mut out: Vec<LevelFileDto> = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy().to_string();
        let lower = name.to_lowercase();
        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);


        let (category, parsed) = if lower == "assetlookup.dat" || lower == "assetstats.dat" {
            ("lookup", true)
        } else if lower == "mobys.dat"
            || lower == "ties.dat"
            || lower == "animsets.dat"
            || lower == "shaders.dat"
            || lower == "highmips.dat"
            || lower == "textures.dat"
            || lower == "zones.dat"
            || lower == "gameplay.dat"
        {
            ("core", true)
        } else if lower == "resident_sound.dat" || lower == "ps3sound.dat" {

            ("audio", true)
        } else if lower.starts_with("streaming_sound")
            || lower.starts_with("ps3soundstream")
        {
            ("audio-stream", true)
        } else if lower.starts_with("resident_dialogue")
            || lower.starts_with("ps3dialogue")
        {

            ("audio", true)
        } else if lower.starts_with("streaming_dialogue")
            || lower.starts_with("ps3dialoguestream")
        {
            ("audio-stream", true)
        } else if lower.starts_with("dialogue.") && lower.ends_with(".pkg") {
            ("localization", false)
        } else if lower.starts_with("lipsync.") {
            ("lipsync", false)
        } else if lower == "lighting.dat" || lower == "cubemaps.dat" {
            ("lighting", false)
        } else if lower == "effect.dat" || lower.starts_with("vfx_system") || lower == "fxconduit_packed.dat" {
            ("vfx", false)
        } else if lower == "cinematics.dat" {
            ("cinematic", false)
        } else if lower == "shrubs.dat" || lower == "foliages.dat" {
            ("foliage", false)
        } else if lower.ends_with(".lc") {
            ("config", false)
        } else if lower.ends_with(".dat") || lower.ends_with(".pkg") {
            ("other", false)
        } else {
            continue;
        };
        out.push(LevelFileDto {
            name,
            size_bytes,
            category,
            parsed,
        });
    }

    out.sort_by(|a, b| {
        b.parsed
            .cmp(&a.parsed)
            .then(a.category.cmp(b.category))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

fn main() {

    let dotenv_result = dotenvy::dotenv();


    eprintln!("─── ReChimera startup ───");
    match dotenv_result {
        Ok(path) => eprintln!("  .env loaded from: {}", path.display()),
        Err(_) => eprintln!("  .env: not found (using process environment only)"),
    }

    eprintln!("─────────────────────────");

    tauri::Builder::default()
        .manage(Mutex::new(AssetCache::default()))
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())

        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())

        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            open_level,
            list_assets,
            build_level_manifest,
            cache::extract_level_to_cache,
            cache::reextract_level_cache,
            cache::cache_status,
            cache::read_cached_manifest,
            cache::read_cached_asset,
            cache::read_cached_bytes,
            cache::export_cached_moby_glb,
            cache::export_skybox,
            cache::read_cached_skybox_meta,
            cache::export_moby_glb_with_options,
            cache::export_moby_fbx_with_options,
            cache::list_animsets,
            cache::decode_animset_clip,
            cache::export_level_glb,
            cache::export_level_fbx,
            cache::export_texture_png,
            cache::export_texture_dds,
            level_layout,
            level_meshes_stream,
            level_character_library_stream,
            list_character_gltfs,
            list_entities_gltfs,
            list_gltfs_in_folder,
            read_file_bytes,
            fetch_animset_clip,
            list_animset_clips,
            find_glb_textures,
            list_level_sounds,
            dump_sound_bank,
            extract_level_sounds,
            extract_one_sound,
            extract_one_stream_sound,
            bulk_extract_sounds_zip,
            extract_level_stream_sounds,
            extract_raw_streaming_sounds,
            get_level_texture_png,
            get_level_textures_bulk,
            list_level_files,
            psarc_list,
            psarc_extract_stream,
            asset_lookup_inspect,
            asset_lookup_extract_stream,
            write_bytes,
            r2::r2_setup_check,
            r2::r2_list_maps,
            r2::r2_extract_globals,
            r2::r2_extract_root_psarcs,
            r2::r2_extract_level,
            r2::r2_level_open_path,
            r2::r2_cache_needs_rebuild,
            r2::r2_probe_level_thumbnails,
            r2::r2_read_scaleform_image,
            r2::r2_read_scaleform_image_crop,
            r2::r2_list_card_sprites,
            r2::r2_import_thumbnail,
            r2::r2_read_imported_thumbnail,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
