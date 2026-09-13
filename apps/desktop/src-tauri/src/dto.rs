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
