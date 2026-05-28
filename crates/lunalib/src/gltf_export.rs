

use byteorder::{LittleEndian, WriteBytesExt};
use gltf_json::validation::{Checked, USize64};
use gltf_json::{
    self,
    accessor::{ComponentType, GenericComponentType, Type as AccessorType},
    animation::{Channel, Property, Sampler, Target},
    buffer,
    mesh::{Mode, Primitive, Semantic},
    Index,
};

use std::collections::HashMap;

use crate::animation::DecodedClip;
use crate::error::{Error, Result};
use crate::math::decompose_col_major;
use crate::moby::{MobyAsset, MobyMesh};
use crate::shader::ShaderInfo;
use crate::skeleton::Skeleton;

const GLB_MAGIC: u32 = 0x46546C67;
const GLB_VERSION: u32 = 2;
const CHUNK_TYPE_JSON: u32 = 0x4E4F534A;
const CHUNK_TYPE_BIN: u32 = 0x004E4942;

pub fn write_moby_geometry_glb(asset: &MobyAsset) -> Result<Vec<u8>> {
    write_moby_glb_full(asset, &[], &HashMap::new(), &HashMap::new())
}

pub fn write_moby_glb_with_animations(
    asset: &MobyAsset,
    clips: &[DecodedClip],
) -> Result<Vec<u8>> {
    write_moby_glb_full(asset, clips, &HashMap::new(), &HashMap::new())
}

pub struct GltfDocBuilder {
    pub bin: Vec<u8>,
    pub accessors: Vec<gltf_json::Accessor>,
    pub buffer_views: Vec<gltf_json::buffer::View>,
    pub nodes: Vec<gltf_json::Node>,
    pub skins: Vec<gltf_json::Skin>,
    pub animations: Vec<gltf_json::Animation>,
    pub images: Vec<gltf_json::Image>,
    pub gltf_textures: Vec<gltf_json::Texture>,
    pub materials: Vec<gltf_json::Material>,
    pub meshes: Vec<gltf_json::Mesh>,
    pub image_idx_by_tex_id: HashMap<u32, u32>,
}

impl GltfDocBuilder {
    pub fn new() -> Self {
        Self {
            bin: Vec::new(),
            accessors: Vec::new(),
            buffer_views: Vec::new(),
            nodes: Vec::new(),
            skins: Vec::new(),
            animations: Vec::new(),
            images: Vec::new(),
            gltf_textures: Vec::new(),
            materials: Vec::new(),
            meshes: Vec::new(),
            image_idx_by_tex_id: HashMap::new(),
        }
    }
}

impl Default for GltfDocBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn append_moby_to_doc(
    doc: &mut GltfDocBuilder,
    asset: &MobyAsset,
    clips: &[DecodedClip],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
    placement: Option<([f32; 3], [f32; 4], [f32; 3])>,
) -> Result<u32> {
    let asset_root_idx = doc.nodes.len() as u32;
    let (translation, rotation, scale) = placement.unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]));
    doc.nodes.push(gltf_json::Node {
        camera: None,
        children: None,
        extensions: Default::default(),
        extras: Default::default(),
        matrix: None,
        mesh: None,
        name: Some(asset_display_name(asset)),
        rotation: Some(gltf_json::scene::UnitQuaternion(rotation)),
        scale: Some(scale),
        translation: Some(translation),
        skin: None,
        weights: None,
    });

    let skin_idx = if let Some(skel) = asset.skeleton.as_ref() {
        if !skel.bones.is_empty() && !skel.bind_local.is_empty() {
            Some(emit_skin(
                &mut doc.bin,
                &mut doc.accessors,
                &mut doc.buffer_views,
                &mut doc.nodes,
                &mut doc.skins,
                skel,
                asset_root_idx,
            ))
        } else {
            None
        }
    } else {
        None
    };

    let (bone_node_base, bone_count) = match (skin_idx, asset.skeleton.as_ref()) {
        (Some(s), Some(skel)) => {
            let _ = s;
            let count = skel.bones.len();
            let base = (doc.nodes.len() - count) as u32;
            (base, count)
        }
        _ => (0u32, 0usize),
    };

    let mut bangle_node_indices: Vec<u32> = Vec::new();
    let mut total_primitives = 0usize;

    for (bi, bangle) in asset.bangles.iter().enumerate() {
        if bangle.meshes.is_empty() {
            continue;
        }
        let mut primitives: Vec<Primitive> = Vec::with_capacity(bangle.meshes.len());
        let mut bangle_has_skin = false;
        for mesh in &bangle.meshes {
            let material_idx = build_material(
                &mut doc.bin,
                &mut doc.buffer_views,
                &mut doc.images,
                &mut doc.gltf_textures,
                &mut doc.materials,
                &mut doc.image_idx_by_tex_id,
                &asset.shader_tuids,
                shaders,
                textures,
                mesh.shader_index as usize,
            );
            if let Some(prim) = push_submesh(
                &mut doc.bin,
                &mut doc.accessors,
                &mut doc.buffer_views,
                mesh,
                skin_idx.is_some(),
                bone_count,
                material_idx,
            )? {
                if prim
                    .attributes
                    .contains_key(&Checked::Valid(Semantic::Joints(0)))
                {
                    bangle_has_skin = true;
                }
                primitives.push(prim);
            }
        }
        if primitives.is_empty() {
            continue;
        }
        total_primitives += primitives.len();

        let mesh_idx = doc.meshes.len() as u32;
        doc.meshes.push(gltf_json::Mesh {
            extensions: Default::default(),
            extras: Default::default(),
            name: Some(format!("{}_Mesh_{bi}", asset_display_name(asset))),
            primitives,
            weights: None,
        });

        let node_idx = doc.nodes.len() as u32;
        doc.nodes.push(gltf_json::Node {
            camera: None,
            children: None,
            extensions: Default::default(),
            extras: Default::default(),
            matrix: None,
            mesh: Some(Index::new(mesh_idx)),
            name: Some(format!("{}_Mesh_{bi}", asset_display_name(asset))),
            rotation: None,
            scale: None,
            translation: None,
            skin: if bangle_has_skin && skin_idx.is_some() {
                Some(Index::new(skin_idx.unwrap()))
            } else {
                None
            },
            weights: None,
        });
        bangle_node_indices.push(node_idx);
    }

    if total_primitives == 0 {
        return Err(Error::SectionLengthMismatch {
            id: 0xD100,
            length: 0,
            entry: 1,
        });
    }

    {
        let root = &mut doc.nodes[asset_root_idx as usize];
        let kids = root.children.get_or_insert_with(Vec::new);
        for n in &bangle_node_indices {
            kids.push(Index::new(*n));
        }
    }

    if skin_idx.is_some() && !clips.is_empty() && bone_count > 0 {
        emit_animations(
            &mut doc.bin,
            &mut doc.accessors,
            &mut doc.buffer_views,
            &mut doc.animations,
            clips,
            bone_node_base,
            bone_count,
        );
    }

    Ok(asset_root_idx)
}

pub fn write_moby_glb_full(
    asset: &MobyAsset,
    clips: &[DecodedClip],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut doc = GltfDocBuilder::new();
    let asset_root_idx = append_moby_to_doc(&mut doc, asset, clips, shaders, textures, None)?;

    while doc.bin.len() % 4 != 0 {
        doc.bin.push(0);
    }

    let GltfDocBuilder {
        bin,
        accessors,
        buffer_views,
        nodes,
        skins,
        animations,
        images,
        gltf_textures,
        materials,
        meshes,
        image_idx_by_tex_id: _,
    } = doc;

    let buffer = gltf_json::Buffer {
        byte_length: USize64(bin.len() as u64),
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        uri: None,
    };

    let scene_root_nodes: Vec<Index<gltf_json::Node>> = vec![Index::new(asset_root_idx)];

    let scene = gltf_json::Scene {
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        nodes: scene_root_nodes,
    };

    let root = gltf_json::Root {
        accessors,
        animations,
        buffers: vec![buffer],
        buffer_views,
        images,
        materials,
        meshes,
        nodes,
        scene: Some(Index::new(0)),
        scenes: vec![scene],
        skins,
        textures: gltf_textures,
        ..Default::default()
    };

    serialize_glb(&root, &bin)
}

fn asset_display_name(asset: &MobyAsset) -> String {
    if asset.name.is_empty() {
        format!("moby_{:016X}", asset.tuid)
    } else {
        asset.name.clone()
    }
}

fn emit_skin(
    bin: &mut Vec<u8>,
    accessors: &mut Vec<gltf_json::Accessor>,
    buffer_views: &mut Vec<gltf_json::buffer::View>,
    nodes: &mut Vec<gltf_json::Node>,
    skins: &mut Vec<gltf_json::Skin>,
    skel: &Skeleton,
    asset_root_node_idx: u32,
) -> u32 {
    let bone_count = skel.bones.len();
    let bone_node_base = nodes.len() as u32;

    for i in 0..bone_count {
        let (translation, scale, quat) = match skel.bind_local.get(i).copied() {
            Some(local) => {
                let mut clean = local;
                clean[3] = 0.0;
                clean[7] = 0.0;
                clean[11] = 0.0;
                clean[15] = 1.0;
                decompose_col_major(&clean)
            }
            None => ([0.0; 3], [1.0; 3], [0.0, 0.0, 0.0, 1.0]),
        };
        nodes.push(gltf_json::Node {
            camera: None,
            children: None,
            extensions: Default::default(),
            extras: Default::default(),
            matrix: None,
            mesh: None,
            name: Some(format!("bone_{i}")),
            rotation: Some(gltf_json::scene::UnitQuaternion(quat)),
            scale: Some(scale),
            translation: Some(translation),
            skin: None,
            weights: None,
        });
    }

    let resolved_parents: Vec<Option<usize>> = (0..bone_count)
        .map(|i| {
            let p = skel.bones[i].parent_index;
            if p < 0 {
                return None;
            }
            let pu = p as usize;
            if pu == i || pu >= bone_count {
                return None;
            }
            let mut cursor = pu;
            for _ in 0..=bone_count {
                let next = skel.bones[cursor].parent_index;
                if next < 0 {
                    return Some(pu);
                }
                let nu = next as usize;
                if nu == cursor {
                    return Some(pu);
                }
                if nu == i {
                    return None;
                }
                if nu >= bone_count {
                    return None;
                }
                cursor = nu;
            }
            None
        })
        .collect();

    let mut asset_root_children: Vec<Index<gltf_json::Node>> = Vec::new();
    for i in 0..bone_count {
        let bone_node_idx = bone_node_base + i as u32;
        match resolved_parents[i] {
            None => {
                asset_root_children.push(Index::new(bone_node_idx));
            }
            Some(parent) => {
                let parent_node_idx = bone_node_base + parent as u32;
                let pnode = &mut nodes[parent_node_idx as usize];
                let kids = pnode.children.get_or_insert_with(Vec::new);
                kids.push(Index::new(bone_node_idx));
            }
        }
    }
    if !asset_root_children.is_empty() {
        let root_kids = nodes[asset_root_node_idx as usize]
            .children
            .get_or_insert_with(Vec::new);
        root_kids.extend(asset_root_children);
    }

    let ibm_bytes = pack_ibms(skel, bone_count);
    let ibm_view = push_buffer_view(bin, buffer_views, &ibm_bytes);
    let ibm_accessor_idx = accessors.len() as u32;
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(Index::new(ibm_view)),
        byte_offset: Some(USize64(0)),
        count: USize64(bone_count as u64),
        component_type: Checked::Valid(GenericComponentType(ComponentType::F32)),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Checked::Valid(AccessorType::Mat4),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    });

    let joints: Vec<Index<gltf_json::Node>> = (0..bone_count)
        .map(|i| Index::new(bone_node_base + i as u32))
        .collect();
    let skin_idx = skins.len() as u32;
    skins.push(gltf_json::Skin {
        extensions: Default::default(),
        extras: Default::default(),
        inverse_bind_matrices: Some(Index::new(ibm_accessor_idx)),
        joints,
        name: Some("skeleton".into()),
        skeleton: None,
    });
    let _ = resolved_parents;
    skin_idx
}

fn pack_ibms(skel: &Skeleton, bone_count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bone_count * 64);
    for i in 0..bone_count {
        let mut ibm = skel
            .bind_world_inverse
            .get(i)
            .copied()
            .unwrap_or(IDENTITY_MAT4);
        ibm[3] = 0.0;
        ibm[7] = 0.0;
        ibm[11] = 0.0;
        ibm[15] = 1.0;
        for v in ibm {
            let cleaned = if v.is_finite() { v } else { 0.0 };
            out.write_f32::<LittleEndian>(cleaned).expect("vec write");
        }
    }
    out
}

const IDENTITY_MAT4: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn push_submesh(
    bin: &mut Vec<u8>,
    accessors: &mut Vec<gltf_json::Accessor>,
    views: &mut Vec<gltf_json::buffer::View>,
    mesh: &MobyMesh,
    skinned: bool,
    bone_count: usize,
    material_idx: Option<u32>,
) -> Result<Option<Primitive>> {
    use std::collections::BTreeMap;

    if mesh.positions.is_empty() || mesh.indices.is_empty() {
        // Empty primitive — skip rather than fail the whole asset.
        // TOD ties (and the occasional V2 moby) sometimes have stub
        // meshes with zero vertices for LOD slots that don't apply.
        // Returning Ok(None) lets the caller drop just this primitive.
        return Ok(None);
    }

    let mut attributes: BTreeMap<Checked<Semantic>, Index<gltf_json::Accessor>> =
        BTreeMap::new();

    let pos_view = push_buffer_view(bin, views, &positions_to_bytes(&mesh.positions));
    let (min, max) = position_bounds(&mesh.positions);
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(Index::new(pos_view)),
        byte_offset: Some(USize64(0)),
        count: USize64((mesh.positions.len() / 3) as u64),
        component_type: Checked::Valid(GenericComponentType(ComponentType::F32)),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Checked::Valid(AccessorType::Vec3),
        min: Some(serde_json::json!([min[0], min[1], min[2]])),
        max: Some(serde_json::json!([max[0], max[1], max[2]])),
        name: None,
        normalized: false,
        sparse: None,
    });
    attributes.insert(
        Checked::Valid(Semantic::Positions),
        Index::new((accessors.len() - 1) as u32),
    );

    if !mesh.uvs.is_empty() {
        let uv_view = push_buffer_view(bin, views, &uvs_to_bytes(&mesh.uvs));
        accessors.push(gltf_json::Accessor {
            buffer_view: Some(Index::new(uv_view)),
            byte_offset: Some(USize64(0)),
            count: USize64((mesh.uvs.len() / 2) as u64),
            component_type: Checked::Valid(GenericComponentType(ComponentType::F32)),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Checked::Valid(AccessorType::Vec2),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Checked::Valid(Semantic::TexCoords(0)),
            Index::new((accessors.len() - 1) as u32),
        );
    }

    if skinned {
        let vertex_count = mesh.positions.len() / 3;
        let real_skin = !mesh.bone_indices.is_empty()
            && mesh.bone_weights.len() == mesh.bone_indices.len()
            && mesh.bone_indices.len() == vertex_count * 4;

        let oversize = mesh.bone_indices.iter().filter(|&&v| v > 255).count();
        if oversize > 0 && std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
            eprintln!(
                "warn: [glb-skin] {} joint indices > 255 will be clamped (JOINTS_0 accessor is u8). \
                 Rig has > 256 bones — accessor should switch to u16 component type.",
                oversize,
            );
        }
        if !real_skin {
            eprintln!(
                "warn: [glb-skin] mesh skinned=true but weights/indices not vertex-aligned \
                 (verts={} bone_idx_len={} bone_wgt_len={}). Emitting bone0/weight=255 fallback.",
                vertex_count, mesh.bone_indices.len(), mesh.bone_weights.len(),
            );
        }

        let joints_bytes = if real_skin {
            joints_u8_bytes(&mesh.bone_indices, vertex_count, bone_count)
        } else {
            vec![0u8; vertex_count * 4]
        };
        let j_view = push_buffer_view(bin, views, &joints_bytes);
        accessors.push(gltf_json::Accessor {
            buffer_view: Some(Index::new(j_view)),
            byte_offset: Some(USize64(0)),
            count: USize64(vertex_count as u64),
            component_type: Checked::Valid(GenericComponentType(ComponentType::U8)),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Checked::Valid(AccessorType::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Checked::Valid(Semantic::Joints(0)),
            Index::new((accessors.len() - 1) as u32),
        );

        let weights_bytes = if real_skin {
            mesh.bone_weights.clone()
        } else {
            let mut buf = vec![0u8; vertex_count * 4];
            for v in 0..vertex_count {
                buf[v * 4] = 255;
            }
            buf
        };
        let w_view = push_buffer_view(bin, views, &weights_bytes);
        accessors.push(gltf_json::Accessor {
            buffer_view: Some(Index::new(w_view)),
            byte_offset: Some(USize64(0)),
            count: USize64(vertex_count as u64),
            component_type: Checked::Valid(GenericComponentType(ComponentType::U8)),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Checked::Valid(AccessorType::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: true,
            sparse: None,
        });
        attributes.insert(
            Checked::Valid(Semantic::Weights(0)),
            Index::new((accessors.len() - 1) as u32),
        );
    }

    let idx_view = push_buffer_view(bin, views, &indices_to_bytes(&mesh.indices));
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(Index::new(idx_view)),
        byte_offset: Some(USize64(0)),
        count: USize64(mesh.indices.len() as u64),
        component_type: Checked::Valid(GenericComponentType(ComponentType::U32)),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Checked::Valid(AccessorType::Scalar),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    });
    let indices_accessor = Index::new((accessors.len() - 1) as u32);

    Ok(Some(Primitive {
        attributes,
        extensions: Default::default(),
        extras: Default::default(),
        indices: Some(indices_accessor),
        material: material_idx.map(Index::new),
        mode: Checked::Valid(Mode::Triangles),
        targets: None,
    }))
}

#[allow(clippy::too_many_arguments)]
fn build_material(
    bin: &mut Vec<u8>,
    views: &mut Vec<gltf_json::buffer::View>,
    images: &mut Vec<gltf_json::Image>,
    gltf_textures: &mut Vec<gltf_json::Texture>,
    materials: &mut Vec<gltf_json::Material>,
    image_idx_by_tex_id: &mut HashMap<u32, u32>,
    shader_tuids: &[u64],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
    shader_index: usize,
) -> Option<u32> {
    let &shader_tuid = shader_tuids.get(shader_index)?;
    let shader = shaders.get(&shader_tuid)?;

    let albedo_idx = shader
        .albedo_tex_id
        .and_then(|id| push_or_reuse_texture(bin, views, images, gltf_textures, image_idx_by_tex_id, textures, id));
    let normal_idx = shader
        .normal_tex_id
        .and_then(|id| push_or_reuse_texture(bin, views, images, gltf_textures, image_idx_by_tex_id, textures, id));
    let emissive_idx = shader
        .expensive_tex_id
        .and_then(|id| push_or_reuse_texture(bin, views, images, gltf_textures, image_idx_by_tex_id, textures, id));

    if albedo_idx.is_none() && normal_idx.is_none() && emissive_idx.is_none() {
        return None;
    }

    let pbr = gltf_json::material::PbrMetallicRoughness {
        base_color_factor: gltf_json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, 1.0]),
        base_color_texture: albedo_idx.map(|idx| gltf_json::texture::Info {
            index: Index::new(idx),
            tex_coord: 0,
            extensions: Default::default(),
            extras: Default::default(),
        }),
        metallic_factor: gltf_json::material::StrengthFactor(0.0),
        roughness_factor: gltf_json::material::StrengthFactor(0.85),
        metallic_roughness_texture: None,
        extensions: Default::default(),
        extras: Default::default(),
    };

    let normal_texture = normal_idx.map(|idx| gltf_json::material::NormalTexture {
        index: Index::new(idx),
        scale: 1.0,
        tex_coord: 0,
        extensions: Default::default(),
        extras: Default::default(),
    });
    let emissive_texture = emissive_idx.map(|idx| gltf_json::texture::Info {
        index: Index::new(idx),
        tex_coord: 0,
        extensions: Default::default(),
        extras: Default::default(),
    });
    // Why: the third texture slot Insomniac calls "expensive" isn't a pure
    // PBR emissive map — it carries full-color spec/detail data that blows
    // out the albedo when multiplied at full strength. Export the map (so
    // downstream tools like Blender / Unreal still receive it) but keep
    // its contribution at zero in the GLB so albedo stays dominant in the
    // preview. Re-enable per-shader once we figure out which R2 shaders
    // actually want emission vs. spec / detail.
    let emissive_factor = gltf_json::material::EmissiveFactor([0.0, 0.0, 0.0]);

    let name = format!(
        "mat_a{}_n{}_e{}",
        shader.albedo_tex_id.map(|i| i.to_string()).unwrap_or_else(|| "_".into()),
        shader.normal_tex_id.map(|i| i.to_string()).unwrap_or_else(|| "_".into()),
        shader.expensive_tex_id.map(|i| i.to_string()).unwrap_or_else(|| "_".into()),
    );

    materials.push(gltf_json::Material {
        alpha_cutoff: None,
        alpha_mode: Checked::Valid(gltf_json::material::AlphaMode::Opaque),
        double_sided: false,
        name: Some(name),
        pbr_metallic_roughness: pbr,
        normal_texture,
        occlusion_texture: None,
        emissive_texture,
        emissive_factor,
        extensions: Default::default(),
        extras: Default::default(),
    });
    Some((materials.len() - 1) as u32)
}

fn push_or_reuse_texture(
    bin: &mut Vec<u8>,
    views: &mut Vec<gltf_json::buffer::View>,
    images: &mut Vec<gltf_json::Image>,
    gltf_textures: &mut Vec<gltf_json::Texture>,
    image_idx_by_tex_id: &mut HashMap<u32, u32>,
    textures: &HashMap<u32, Vec<u8>>,
    tex_id: u32,
) -> Option<u32> {
    if let Some(&idx) = image_idx_by_tex_id.get(&tex_id) {
        return Some(idx);
    }
    let png_bytes = textures.get(&tex_id)?;
    if png_bytes.is_empty() {
        return None;
    }
    let view = push_buffer_view(bin, views, png_bytes);
    images.push(gltf_json::Image {
        buffer_view: Some(Index::new(view)),
        mime_type: Some(gltf_json::image::MimeType("image/png".into())),
        name: Some(format!("tex_{tex_id}")),
        uri: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    let img_idx = (images.len() - 1) as u32;
    gltf_textures.push(gltf_json::Texture {
        name: Some(format!("tex_{tex_id}")),
        sampler: None,
        source: Index::new(img_idx),
        extensions: Default::default(),
        extras: Default::default(),
    });
    image_idx_by_tex_id.insert(tex_id, img_idx);
    Some(img_idx)
}

fn push_buffer_view(
    bin: &mut Vec<u8>,
    views: &mut Vec<gltf_json::buffer::View>,
    bytes: &[u8],
) -> u32 {
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let offset = bin.len() as u64;
    bin.extend_from_slice(bytes);
    views.push(buffer::View {
        buffer: Index::new(0),
        byte_length: USize64(bytes.len() as u64),
        byte_offset: Some(USize64(offset)),
        byte_stride: None,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: None,
    });
    (views.len() - 1) as u32
}

fn positions_to_bytes(positions: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(positions.len() * 4);
    for v in positions {
        out.write_f32::<LittleEndian>(*v).expect("vec write");
    }
    out
}

fn uvs_to_bytes(uvs: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(uvs.len() * 4);
    for v in uvs {
        out.write_f32::<LittleEndian>(*v).expect("vec write");
    }
    out
}

fn indices_to_bytes(indices: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(indices.len() * 4);
    for v in indices {
        out.write_u32::<LittleEndian>(*v).expect("vec write");
    }
    out
}

fn joints_u8_bytes(indices: &[u16], vertex_count: usize, bone_count: usize) -> Vec<u8> {
    let max_bone = if bone_count == 0 {
        0
    } else {
        (bone_count - 1).min(255) as u16
    };
    let mut out = Vec::with_capacity(vertex_count * 4);
    let mut oob = 0usize;
    for i in 0..vertex_count {
        for k in 0..4 {
            let v = indices.get(i * 4 + k).copied().unwrap_or(0);
            if v > max_bone {
                oob += 1;
                out.push(max_bone as u8);
            } else {
                out.push(v as u8);
            }
        }
    }
    if oob > 0 && std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        eprintln!(
            "warn: [glb-skin] clamped {} joint indices >= bone_count ({}) to {}; \
             likely missing per-bangle bone remap or wrong sub-skeleton",
            oob, bone_count, max_bone,
        );
    }
    out
}

fn position_bounds(positions: &[f32]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for chunk in positions.chunks_exact(3) {
        for (i, v) in chunk.iter().enumerate() {
            if *v < min[i] {
                min[i] = *v;
            }
            if *v > max[i] {
                max[i] = *v;
            }
        }
    }
    if !positions.is_empty() {
        (min, max)
    } else {
        ([0.0; 3], [0.0; 3])
    }
}

pub(crate) fn serialize_glb(root: &gltf_json::Root, bin: &[u8]) -> Result<Vec<u8>> {
    let json_string =
        serde_json::to_vec(root).map_err(|e| Error::GltfWrite(e.to_string()))?;
    let mut json_chunk = json_string;
    while json_chunk.len() % 4 != 0 {
        json_chunk.push(0x20);
    }

    let mut bin_chunk = bin.to_vec();
    while bin_chunk.len() % 4 != 0 {
        bin_chunk.push(0);
    }

    let total_len = 12 + 8 + json_chunk.len() + 8 + bin_chunk.len();
    let mut out = Vec::with_capacity(total_len);

    out.write_u32::<LittleEndian>(GLB_MAGIC)
        .map_err(|e| Error::GltfWrite(e.to_string()))?;
    out.write_u32::<LittleEndian>(GLB_VERSION)
        .map_err(|e| Error::GltfWrite(e.to_string()))?;
    out.write_u32::<LittleEndian>(total_len as u32)
        .map_err(|e| Error::GltfWrite(e.to_string()))?;

    out.write_u32::<LittleEndian>(json_chunk.len() as u32)
        .map_err(|e| Error::GltfWrite(e.to_string()))?;
    out.write_u32::<LittleEndian>(CHUNK_TYPE_JSON)
        .map_err(|e| Error::GltfWrite(e.to_string()))?;
    out.extend_from_slice(&json_chunk);

    out.write_u32::<LittleEndian>(bin_chunk.len() as u32)
        .map_err(|e| Error::GltfWrite(e.to_string()))?;
    out.write_u32::<LittleEndian>(CHUNK_TYPE_BIN)
        .map_err(|e| Error::GltfWrite(e.to_string()))?;
    out.extend_from_slice(&bin_chunk);

    Ok(out)
}

fn emit_animations(
    bin: &mut Vec<u8>,
    accessors: &mut Vec<gltf_json::Accessor>,
    views: &mut Vec<gltf_json::buffer::View>,
    animations: &mut Vec<gltf_json::Animation>,
    clips: &[DecodedClip],
    bone_node_base: u32,
    bone_count: usize,
) {
    let detail_log = std::env::var("RECHIMERA_LOG_ANIM_DETAIL").is_ok();
    for (clip_idx, clip) in clips.iter().enumerate() {
        let dt = if clip.frame_rate > 0.0 {
            1.0 / clip.frame_rate
        } else {
            1.0 / 30.0
        };

        let animated_time_acc = if clip.num_frames > 0 {
            let times: Vec<f32> = (0..clip.num_frames as usize)
                .map(|i| i as f32 * dt)
                .collect();
            Some(push_scalar_f32_accessor(bin, accessors, views, &times))
        } else {
            None
        };
        let static_time_acc = push_scalar_f32_accessor(bin, accessors, views, &[0.0_f32]);

        let mut channels: Vec<Channel> = Vec::new();
        let mut samplers: Vec<Sampler> = Vec::new();

        let bones_to_use = clip.bones.len().min(bone_count);
        for b in 0..bones_to_use {
            let bone = &clip.bones[b];
            let target_node = Index::new(bone_node_base + b as u32);

            if !bone.rotations.is_empty() {
                let time_acc = if bone.rotation_animated {
                    animated_time_acc.unwrap_or(static_time_acc)
                } else {
                    static_time_acc
                };
                // Quaternion double-cover fixup: q and -q represent the
                // same rotation but linear interpolation between them
                // sweeps through the singular point at the antipode,
                // causing visible 180° flips ("shaking"). For every pair
                // of consecutive keyframes, if dot(prev, curr) < 0 we
                // negate curr so adjacent quats stay in the same
                // hemisphere and Linear interpolation takes the short path.
                let mut fixed = bone.rotations.clone();
                let frames = fixed.len() / 4;
                for f in 1..frames {
                    let p = (f - 1) * 4;
                    let c = f * 4;
                    let dot = fixed[p] * fixed[c]
                        + fixed[p + 1] * fixed[c + 1]
                        + fixed[p + 2] * fixed[c + 2]
                        + fixed[p + 3] * fixed[c + 3];
                    if dot < 0.0 {
                        fixed[c] = -fixed[c];
                        fixed[c + 1] = -fixed[c + 1];
                        fixed[c + 2] = -fixed[c + 2];
                        fixed[c + 3] = -fixed[c + 3];
                    }
                }
                let val_acc =
                    push_vec4_f32_accessor(bin, accessors, views, &fixed);
                let sampler_idx = samplers.len() as u32;
                samplers.push(Sampler {
                    extensions: Default::default(),
                    extras: Default::default(),
                    input: time_acc,
                    output: val_acc,
                    interpolation: Checked::Valid(
                        gltf_json::animation::Interpolation::Linear,
                    ),
                });
                channels.push(Channel {
                    extensions: Default::default(),
                    extras: Default::default(),
                    sampler: Index::new(sampler_idx),
                    target: Target {
                        extensions: Default::default(),
                        extras: Default::default(),
                        node: target_node,
                        path: Checked::Valid(Property::Rotation),
                    },
                });
            }

            if !bone.translations.is_empty() {
                let time_acc = if bone.translation_animated {
                    animated_time_acc.unwrap_or(static_time_acc)
                } else {
                    static_time_acc
                };
                let val_acc =
                    push_vec3_f32_accessor(bin, accessors, views, &bone.translations);
                let sampler_idx = samplers.len() as u32;
                samplers.push(Sampler {
                    extensions: Default::default(),
                    extras: Default::default(),
                    input: time_acc,
                    output: val_acc,
                    interpolation: Checked::Valid(
                        gltf_json::animation::Interpolation::Linear,
                    ),
                });
                channels.push(Channel {
                    extensions: Default::default(),
                    extras: Default::default(),
                    sampler: Index::new(sampler_idx),
                    target: Target {
                        extensions: Default::default(),
                        extras: Default::default(),
                        node: target_node,
                        path: Checked::Valid(Property::Translation),
                    },
                });
            }

            if !bone.scales.is_empty() {
                let time_acc = if bone.scale_animated {
                    animated_time_acc.unwrap_or(static_time_acc)
                } else {
                    static_time_acc
                };
                let val_acc =
                    push_vec3_f32_accessor(bin, accessors, views, &bone.scales);
                let sampler_idx = samplers.len() as u32;
                samplers.push(Sampler {
                    extensions: Default::default(),
                    extras: Default::default(),
                    input: time_acc,
                    output: val_acc,
                    interpolation: Checked::Valid(
                        gltf_json::animation::Interpolation::Linear,
                    ),
                });
                channels.push(Channel {
                    extensions: Default::default(),
                    extras: Default::default(),
                    sampler: Index::new(sampler_idx),
                    target: Target {
                        extensions: Default::default(),
                        extras: Default::default(),
                        node: target_node,
                        path: Checked::Valid(Property::Scale),
                    },
                });
            }
        }

        if detail_log {
            eprintln!(
                "[glb-anim] clip[{}] '{}' frames={} fps={:.1} channels={} samplers={} bones_used={}",
                clip_idx, clip.name, clip.num_frames, clip.frame_rate, channels.len(), samplers.len(), bones_to_use,
            );
        }
        if !channels.is_empty() {
            animations.push(gltf_json::Animation {
                channels,
                extensions: Default::default(),
                extras: Default::default(),
                name: Some(clip.name.clone()),
                samplers,
            });
        }
    }
}

fn push_scalar_f32_accessor(
    bin: &mut Vec<u8>,
    accessors: &mut Vec<gltf_json::Accessor>,
    views: &mut Vec<gltf_json::buffer::View>,
    values: &[f32],
) -> Index<gltf_json::Accessor> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for v in values {
        bytes.write_f32::<LittleEndian>(*v).expect("vec write");
    }
    let view = push_buffer_view(bin, views, &bytes);
    let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(Index::new(view)),
        byte_offset: Some(USize64(0)),
        count: USize64(values.len() as u64),
        component_type: Checked::Valid(GenericComponentType(ComponentType::F32)),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Checked::Valid(AccessorType::Scalar),
        min: Some(serde_json::json!([min])),
        max: Some(serde_json::json!([max])),
        name: None,
        normalized: false,
        sparse: None,
    });
    Index::new((accessors.len() - 1) as u32)
}

fn push_vec3_f32_accessor(
    bin: &mut Vec<u8>,
    accessors: &mut Vec<gltf_json::Accessor>,
    views: &mut Vec<gltf_json::buffer::View>,
    values: &[f32],
) -> Index<gltf_json::Accessor> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for v in values {
        bytes.write_f32::<LittleEndian>(*v).expect("vec write");
    }
    let view = push_buffer_view(bin, views, &bytes);
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(Index::new(view)),
        byte_offset: Some(USize64(0)),
        count: USize64((values.len() / 3) as u64),
        component_type: Checked::Valid(GenericComponentType(ComponentType::F32)),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Checked::Valid(AccessorType::Vec3),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    });
    Index::new((accessors.len() - 1) as u32)
}

fn push_vec4_f32_accessor(
    bin: &mut Vec<u8>,
    accessors: &mut Vec<gltf_json::Accessor>,
    views: &mut Vec<gltf_json::buffer::View>,
    values: &[f32],
) -> Index<gltf_json::Accessor> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for v in values {
        bytes.write_f32::<LittleEndian>(*v).expect("vec write");
    }
    let view = push_buffer_view(bin, views, &bytes);
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(Index::new(view)),
        byte_offset: Some(USize64(0)),
        count: USize64((values.len() / 4) as u64),
        component_type: Checked::Valid(GenericComponentType(ComponentType::F32)),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Checked::Valid(AccessorType::Vec4),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    });
    Index::new((accessors.len() - 1) as u32)
}
