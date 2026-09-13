use std::collections::HashMap;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use lunalib::ShaderInfo;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
pub(crate) struct SectionDto {
    pub(crate) id: u32,
    pub(crate) offset: u32,
    pub(crate) count: u32,
    pub(crate) length: u32,
}

#[derive(Serialize)]
pub(crate) struct AssetCount {
    pub(crate) kind: &'static str,
    pub(crate) section_id: u32,
    pub(crate) count: usize,
    pub(crate) present: bool,
}

#[derive(Serialize)]
pub(crate) struct LevelSummary {
    pub(crate) folder: String,
    pub(crate) version_major: u16,
    pub(crate) version_minor: u16,
    pub(crate) sections: Vec<SectionDto>,
    pub(crate) asset_counts: Vec<AssetCount>,
}

#[derive(Serialize)]
pub(crate) struct AssetPointerDto {
    pub(crate) tuid: String,
    pub(crate) offset: u32,
    pub(crate) length: u32,
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
pub(crate) struct UFragDto {
    pub(crate) tuid: String,
    pub(crate) zone_tuid: String,
    pub(crate) position: [f32; 3],
    pub(crate) radius: f32,
    pub(crate) vertex_count: u16,
    pub(crate) triangle_count: u16,
}

#[derive(Serialize)]
pub(crate) struct LevelLayoutDto {
    pub(crate) instances: Vec<InstanceDto>,
    pub(crate) ufrags: Vec<UFragDto>,
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
pub(crate) struct TextureDto {
    pub(crate) id: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SkeletonDto {
    pub(crate) bone_count: usize,
    pub(crate) root_bone: u16,
    pub(crate) parents: Vec<i16>,
    pub(crate) bind_local: Vec<[f32; 16]>,
    pub(crate) bind_world_inverse: Vec<[f32; 16]>,
    pub(crate) tms0_col: Vec<[f32; 16]>,
    pub(crate) tms1_col: Vec<[f32; 16]>,
    pub(crate) scale_shift: u16,
    pub(crate) translation_shift: u16,
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

pub(crate) fn encode_f32_buffer(values: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<f32>());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    BASE64.encode(bytes)
}

pub(crate) fn encode_u32_buffer(values: &[u32]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<u32>());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    BASE64.encode(bytes)
}

pub(crate) fn encode_u16_buffer(values: &[u16]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<u16>());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    BASE64.encode(bytes)
}

pub(crate) fn encode_u8_buffer(values: &[u8]) -> String {
    BASE64.encode(values)
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
