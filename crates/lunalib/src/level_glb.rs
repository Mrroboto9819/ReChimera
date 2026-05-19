use std::collections::{BTreeMap, HashMap};

use byteorder::{LittleEndian, WriteBytesExt};
use gltf_json::validation::{Checked, USize64};
use gltf_json::{
    self,
    accessor::{ComponentType, GenericComponentType, Type as AccessorType},
    buffer,
    mesh::{Mode, Primitive, Semantic},
    Index,
};

use crate::animation::DecodedClip;
use crate::error::{Error, Result};
use crate::gltf_export::{append_moby_to_doc, serialize_glb, GltfDocBuilder};
use crate::moby::MobyAsset;
use crate::shader::ShaderInfo;

pub struct LevelGlbSubmesh {
    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
    pub indices: Vec<u32>,
    pub albedo_id: Option<u32>,
}

pub struct LevelGlbAsset {
    pub name: String,
    pub submeshes: Vec<LevelGlbSubmesh>,
}

pub struct LevelGlbInstance {
    pub asset_idx: usize,
    pub name: String,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

pub fn write_static_level_glb(
    assets: &[LevelGlbAsset],
    instances: &[LevelGlbInstance],
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut bin: Vec<u8> = Vec::new();
    let mut accessors: Vec<gltf_json::Accessor> = Vec::new();
    let mut buffer_views: Vec<gltf_json::buffer::View> = Vec::new();
    let mut images: Vec<gltf_json::Image> = Vec::new();
    let mut gltf_textures: Vec<gltf_json::Texture> = Vec::new();
    let mut materials: Vec<gltf_json::Material> = Vec::new();
    let mut meshes: Vec<gltf_json::Mesh> = Vec::new();
    let mut nodes: Vec<gltf_json::Node> = Vec::new();

    let mut material_idx_by_tex_id: HashMap<u32, u32> = HashMap::new();
    let default_material_idx = materials.len() as u32;
    materials.push(gltf_json::Material {
        name: Some("default".into()),
        ..Default::default()
    });

    for (asset_i, asset) in assets.iter().enumerate() {
        let mut primitives: Vec<Primitive> = Vec::with_capacity(asset.submeshes.len());

        for (sub_i, sub) in asset.submeshes.iter().enumerate() {
            if sub.positions.is_empty() || sub.indices.is_empty() {
                continue;
            }
            if sub.positions.len() % 3 != 0 {
                continue;
            }
            let vertex_count = sub.positions.len() / 3;

            let pos_view_offset = pad_align(&mut bin, 4);
            for v in &sub.positions {
                bin.write_f32::<LittleEndian>(*v)
                    .map_err(|e| Error::GltfWrite(e.to_string()))?;
            }
            let pos_view_len = bin.len() - pos_view_offset;
            buffer_views.push(gltf_json::buffer::View {
                buffer: Index::new(0),
                byte_length: USize64::from(pos_view_len),
                byte_offset: Some(USize64::from(pos_view_offset)),
                byte_stride: Some(buffer::Stride(12)),
                target: Some(Checked::Valid(buffer::Target::ArrayBuffer)),
                extensions: Default::default(),
                extras: Default::default(),
                name: None,
            });
            let pos_view_idx = (buffer_views.len() - 1) as u32;
            let (pos_min, pos_max) = bounds(&sub.positions);
            accessors.push(gltf_json::Accessor {
                buffer_view: Some(Index::new(pos_view_idx)),
                byte_offset: Some(USize64::from(0usize)),
                count: USize64::from(vertex_count),
                component_type: Checked::Valid(GenericComponentType(ComponentType::F32)),
                type_: Checked::Valid(AccessorType::Vec3),
                min: Some(serde_json::json!(pos_min)),
                max: Some(serde_json::json!(pos_max)),
                normalized: false,
                sparse: None,
                extensions: Default::default(),
                extras: Default::default(),
                name: Some(format!("a{}_s{}_pos", asset_i, sub_i)),
            });
            let pos_acc_idx = (accessors.len() - 1) as u32;

            let uv_acc_idx_opt = if sub.uvs.len() == vertex_count * 2 {
                let uv_offset = pad_align(&mut bin, 4);
                for v in &sub.uvs {
                    bin.write_f32::<LittleEndian>(*v)
                        .map_err(|e| Error::GltfWrite(e.to_string()))?;
                }
                let uv_len = bin.len() - uv_offset;
                buffer_views.push(gltf_json::buffer::View {
                    buffer: Index::new(0),
                    byte_length: USize64::from(uv_len),
                    byte_offset: Some(USize64::from(uv_offset)),
                    byte_stride: Some(buffer::Stride(8)),
                    target: Some(Checked::Valid(buffer::Target::ArrayBuffer)),
                    extensions: Default::default(),
                    extras: Default::default(),
                    name: None,
                });
                let uv_view_idx = (buffer_views.len() - 1) as u32;
                accessors.push(gltf_json::Accessor {
                    buffer_view: Some(Index::new(uv_view_idx)),
                    byte_offset: Some(USize64::from(0usize)),
                    count: USize64::from(vertex_count),
                    component_type: Checked::Valid(GenericComponentType(ComponentType::F32)),
                    type_: Checked::Valid(AccessorType::Vec2),
                    min: None,
                    max: None,
                    normalized: false,
                    sparse: None,
                    extensions: Default::default(),
                    extras: Default::default(),
                    name: Some(format!("a{}_s{}_uv", asset_i, sub_i)),
                });
                Some((accessors.len() - 1) as u32)
            } else {
                None
            };

            let idx_offset = pad_align(&mut bin, 4);
            for i in &sub.indices {
                bin.write_u32::<LittleEndian>(*i)
                    .map_err(|e| Error::GltfWrite(e.to_string()))?;
            }
            let idx_len = bin.len() - idx_offset;
            buffer_views.push(gltf_json::buffer::View {
                buffer: Index::new(0),
                byte_length: USize64::from(idx_len),
                byte_offset: Some(USize64::from(idx_offset)),
                byte_stride: None,
                target: Some(Checked::Valid(buffer::Target::ElementArrayBuffer)),
                extensions: Default::default(),
                extras: Default::default(),
                name: None,
            });
            let idx_view_idx = (buffer_views.len() - 1) as u32;
            accessors.push(gltf_json::Accessor {
                buffer_view: Some(Index::new(idx_view_idx)),
                byte_offset: Some(USize64::from(0usize)),
                count: USize64::from(sub.indices.len()),
                component_type: Checked::Valid(GenericComponentType(ComponentType::U32)),
                type_: Checked::Valid(AccessorType::Scalar),
                min: None,
                max: None,
                normalized: false,
                sparse: None,
                extensions: Default::default(),
                extras: Default::default(),
                name: Some(format!("a{}_s{}_idx", asset_i, sub_i)),
            });
            let idx_acc_idx = (accessors.len() - 1) as u32;

            let mat_idx = match sub.albedo_id {
                Some(tex_id) => {
                    if let Some(idx) = material_idx_by_tex_id.get(&tex_id) {
                        *idx
                    } else if let Some(png_bytes) = textures.get(&tex_id) {
                        let img_offset = pad_align(&mut bin, 4);
                        bin.extend_from_slice(png_bytes);
                        let img_len = bin.len() - img_offset;
                        buffer_views.push(gltf_json::buffer::View {
                            buffer: Index::new(0),
                            byte_length: USize64::from(img_len),
                            byte_offset: Some(USize64::from(img_offset)),
                            byte_stride: None,
                            target: None,
                            extensions: Default::default(),
                            extras: Default::default(),
                            name: None,
                        });
                        let img_view_idx = (buffer_views.len() - 1) as u32;
                        images.push(gltf_json::Image {
                            buffer_view: Some(Index::new(img_view_idx)),
                            mime_type: Some(gltf_json::image::MimeType("image/png".into())),
                            uri: None,
                            extensions: Default::default(),
                            extras: Default::default(),
                            name: Some(format!("tex_{}", tex_id)),
                        });
                        let image_idx = (images.len() - 1) as u32;
                        gltf_textures.push(gltf_json::Texture {
                            sampler: None,
                            source: Index::new(image_idx),
                            extensions: Default::default(),
                            extras: Default::default(),
                            name: None,
                        });
                        let tex_idx = (gltf_textures.len() - 1) as u32;
                        let mut mat = gltf_json::Material {
                            name: Some(format!("mat_{}", tex_id)),
                            ..Default::default()
                        };
                        mat.pbr_metallic_roughness.base_color_texture =
                            Some(gltf_json::texture::Info {
                                index: Index::new(tex_idx),
                                tex_coord: 0,
                                extensions: Default::default(),
                                extras: Default::default(),
                            });
                        materials.push(mat);
                        let mat_idx = (materials.len() - 1) as u32;
                        material_idx_by_tex_id.insert(tex_id, mat_idx);
                        mat_idx
                    } else {
                        default_material_idx
                    }
                }
                None => default_material_idx,
            };

            let mut attributes: BTreeMap<Checked<Semantic>, Index<gltf_json::Accessor>> =
                BTreeMap::new();
            attributes.insert(
                Checked::Valid(Semantic::Positions),
                Index::new(pos_acc_idx),
            );
            if let Some(uv) = uv_acc_idx_opt {
                attributes.insert(Checked::Valid(Semantic::TexCoords(0)), Index::new(uv));
            }

            primitives.push(Primitive {
                attributes,
                indices: Some(Index::new(idx_acc_idx)),
                material: Some(Index::new(mat_idx)),
                mode: Checked::Valid(Mode::Triangles),
                targets: None,
                extensions: Default::default(),
                extras: Default::default(),
            });
        }

        meshes.push(gltf_json::Mesh {
            primitives,
            weights: None,
            extensions: Default::default(),
            extras: Default::default(),
            name: Some(asset.name.clone()),
        });
    }

    let mut scene_root_nodes: Vec<Index<gltf_json::Node>> = Vec::new();
    for inst in instances {
        if inst.asset_idx >= meshes.len() {
            continue;
        }
        nodes.push(gltf_json::Node {
            camera: None,
            children: None,
            extensions: Default::default(),
            extras: Default::default(),
            matrix: None,
            mesh: Some(Index::new(inst.asset_idx as u32)),
            name: Some(inst.name.clone()),
            rotation: Some(gltf_json::scene::UnitQuaternion(inst.rotation)),
            scale: Some(inst.scale),
            translation: Some(inst.translation),
            skin: None,
            weights: None,
        });
        scene_root_nodes.push(Index::new((nodes.len() - 1) as u32));
    }

    let buffer = gltf_json::Buffer {
        byte_length: USize64::from(bin.len()),
        name: None,
        uri: None,
        extensions: Default::default(),
        extras: Default::default(),
    };
    let scene = gltf_json::Scene {
        nodes: scene_root_nodes,
        name: Some("level".into()),
        extensions: Default::default(),
        extras: Default::default(),
    };

    let root = gltf_json::Root {
        asset: gltf_json::Asset {
            generator: Some("rechimera level export".into()),
            ..Default::default()
        },
        accessors,
        buffers: vec![buffer],
        buffer_views,
        meshes,
        nodes,
        scenes: vec![scene],
        scene: Some(Index::new(0)),
        materials,
        images,
        textures: gltf_textures,
        ..Default::default()
    };

    serialize_glb(&root, &bin)
}

fn pad_align(bin: &mut Vec<u8>, align: usize) -> usize {
    while bin.len() % align != 0 {
        bin.push(0);
    }
    bin.len()
}

pub struct SkinnedPlacement {
    pub asset: MobyAsset,
    pub clips: Vec<DecodedClip>,
    pub name: String,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

pub fn write_animated_level_glb(
    static_assets: &[LevelGlbAsset],
    static_instances: &[LevelGlbInstance],
    skinned_placements: &[SkinnedPlacement],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<Vec<u8>> {
    let static_glb = write_static_level_glb(static_assets, static_instances, textures)?;
    let (static_root, static_bin) = strip_glb(&static_glb)?;

    let mut doc = GltfDocBuilder::new();
    doc.bin = static_bin;
    doc.accessors = static_root.accessors;
    doc.buffer_views = static_root.buffer_views;
    doc.nodes = static_root.nodes;
    doc.materials = static_root.materials;
    doc.images = static_root.images;
    doc.gltf_textures = static_root.textures;
    doc.meshes = static_root.meshes;
    let mut scene_roots: Vec<Index<gltf_json::Node>> = static_root
        .scenes
        .first()
        .map(|s| s.nodes.clone())
        .unwrap_or_default();

    for placement in skinned_placements {
        while doc.bin.len() % 4 != 0 {
            doc.bin.push(0);
        }
        let root_idx = append_moby_to_doc(
            &mut doc,
            &placement.asset,
            &placement.clips,
            shaders,
            textures,
            Some((placement.translation, placement.rotation, placement.scale)),
        )?;
        if !placement.name.is_empty() {
            doc.nodes[root_idx as usize].name = Some(placement.name.clone());
        }
        scene_roots.push(Index::new(root_idx));
    }

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

    let scene = gltf_json::Scene {
        extensions: Default::default(),
        extras: Default::default(),
        name: Some("level".into()),
        nodes: scene_roots,
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

fn strip_glb(glb: &[u8]) -> Result<(gltf_json::Root, Vec<u8>)> {
    if glb.len() < 28 || &glb[0..4] != b"glTF" {
        return Err(Error::GltfWrite("invalid GLB magic".into()));
    }
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    if &glb[16..20] != b"JSON" {
        return Err(Error::GltfWrite("GLB chunk 0 is not JSON".into()));
    }
    let json_end = 20 + json_len;
    if glb.len() < json_end + 8 {
        return Err(Error::GltfWrite("GLB truncated before BIN chunk".into()));
    }
    let bin_len = u32::from_le_bytes(glb[json_end..json_end + 4].try_into().unwrap()) as usize;
    if &glb[json_end + 4..json_end + 8] != b"BIN\0" {
        return Err(Error::GltfWrite("GLB chunk 1 is not BIN".into()));
    }
    let json_bytes = &glb[20..json_end];
    let bin_bytes = &glb[json_end + 8..json_end + 8 + bin_len];
    let root: gltf_json::Root = serde_json::from_slice(trim_padding(json_bytes))
        .map_err(|e| Error::GltfWrite(format!("parse static GLB JSON: {e}")))?;
    Ok((root, bin_bytes.to_vec()))
}

fn trim_padding(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == 0x20 || bytes[end - 1] == 0x00) {
        end -= 1;
    }
    &bytes[..end]
}

fn bounds(positions: &[f32]) -> ([f32; 3], [f32; 3]) {
    if positions.len() < 3 {
        return ([0.0; 3], [0.0; 3]);
    }
    let mut min = [positions[0], positions[1], positions[2]];
    let mut max = min;
    let mut i = 0;
    while i + 2 < positions.len() {
        let p = [positions[i], positions[i + 1], positions[i + 2]];
        for k in 0..3 {
            if p[k] < min[k] {
                min[k] = p[k];
            }
            if p[k] > max[k] {
                max[k] = p[k];
            }
        }
        i += 3;
    }
    (min, max)
}
