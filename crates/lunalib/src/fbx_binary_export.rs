use std::collections::HashMap;

use crate::animation::DecodedClip;
use crate::error::Result;
use crate::fbx_node::{serialize_fbx_binary, FbxNode, FbxProperty};
use crate::math::decompose_col_major;
use crate::moby::MobyAsset;
use crate::shader::ShaderInfo;
use crate::skeleton::Skeleton;

const ROOT_NODE_ID: i64 = 0;
const KTIME_PER_SECOND: i64 = 46186158000;
const DOCUMENT_ID: i64 = 1_000_001;

pub fn write_moby_fbx_binary(
    asset: &MobyAsset,
    clips: &[DecodedClip],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut b = BinaryFbxBuilder::new();
    b.append_moby(asset, clips, shaders, textures, None);
    let tree = b.finish(&asset.name);
    serialize_fbx_binary(&tree)
}

pub fn write_animated_level_fbx_binary(
    static_assets: &[crate::level_glb::LevelGlbAsset],
    static_instances: &[crate::level_glb::LevelGlbInstance],
    skinned_placements: &[crate::level_glb::SkinnedPlacement],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut b = BinaryFbxBuilder::new();
    for (ai, asset) in static_assets.iter().enumerate() {
        for inst in static_instances.iter().filter(|i| i.asset_idx == ai) {
            b.append_static_asset(asset, inst, textures);
        }
    }
    for placement in skinned_placements {
        let inst = crate::level_glb::LevelGlbInstance {
            asset_idx: 0,
            name: placement.name.clone(),
            translation: placement.translation,
            rotation: placement.rotation,
            scale: placement.scale,
        };
        b.append_moby(&placement.asset, &placement.clips, shaders, textures, Some(&inst));
    }
    let tree = b.finish("level");
    serialize_fbx_binary(&tree)
}

struct BinaryFbxBuilder {
    next_id: i64,
    geometries: Vec<FbxNode>,
    models: Vec<FbxNode>,
    materials: Vec<FbxNode>,
    textures: Vec<FbxNode>,
    videos: Vec<FbxNode>,
    node_attrs: Vec<FbxNode>,
    deformers: Vec<FbxNode>,
    sub_deformers: Vec<FbxNode>,
    poses: Vec<FbxNode>,
    anim_stacks: Vec<FbxNode>,
    anim_layers: Vec<FbxNode>,
    anim_curve_nodes: Vec<FbxNode>,
    anim_curves: Vec<FbxNode>,
    connections: Vec<FbxNode>,
}

impl BinaryFbxBuilder {
    fn new() -> Self {
        Self {
            next_id: 200,
            geometries: Vec::new(),
            models: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            videos: Vec::new(),
            node_attrs: Vec::new(),
            deformers: Vec::new(),
            sub_deformers: Vec::new(),
            poses: Vec::new(),
            anim_stacks: Vec::new(),
            anim_layers: Vec::new(),
            anim_curve_nodes: Vec::new(),
            anim_curves: Vec::new(),
            connections: Vec::new(),
        }
    }

    fn new_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn connect_oo(&mut self, src: i64, dst: i64) {
        let mut node = FbxNode::new("C");
        node.push_str_prop("OO");
        node.push_prop(FbxProperty::I64(src));
        node.push_prop(FbxProperty::I64(dst));
        self.connections.push(node);
    }

    fn connect_op(&mut self, src: i64, dst: i64, prop: &str) {
        let mut node = FbxNode::new("C");
        node.push_str_prop("OP");
        node.push_prop(FbxProperty::I64(src));
        node.push_prop(FbxProperty::I64(dst));
        node.push_str_prop(prop);
        self.connections.push(node);
    }

    fn append_moby(
        &mut self,
        asset: &MobyAsset,
        clips: &[DecodedClip],
        shaders: &HashMap<u64, ShaderInfo>,
        textures: &HashMap<u32, Vec<u8>>,
        placement: Option<&crate::level_glb::LevelGlbInstance>,
    ) {
        let mut tex_id_by_png: HashMap<u32, i64> = HashMap::new();
        let mut mat_id_by_key: HashMap<(u16, Option<u32>), i64> = HashMap::new();

        let parent_id = if let Some(inst) = placement {
            let null_id = self.new_id();
            self.models.push(build_null_model(null_id, &inst.name, inst));
            self.connect_oo(null_id, ROOT_NODE_ID);
            null_id
        } else {
            ROOT_NODE_ID
        };

        let mut mesh_targets: Vec<(i64, i64, i64, usize, usize)> = Vec::new();
        let mut submesh_counter: u32 = 0;
        for (bi, bangle) in asset.bangles.iter().enumerate() {
            for (mi, mesh) in bangle.meshes.iter().enumerate() {
                if mesh.positions.is_empty() || mesh.indices.is_empty() {
                    continue;
                }
                if mesh.positions.len() % 3 != 0 || mesh.indices.len() % 3 != 0 {
                    continue;
                }
                let albedo_id = resolve_albedo(shaders, &asset.shader_tuids, mesh.shader_index);
                let key = (mesh.shader_index, albedo_id);
                let mat_id = if let Some(id) = mat_id_by_key.get(&key) {
                    *id
                } else {
                    let new_mat_id = self.new_id();
                    let tex_node_id = if let Some(albedo) = albedo_id {
                        if let Some(id) = tex_id_by_png.get(&albedo) {
                            Some(*id)
                        } else if let Some(png) = textures.get(&albedo) {
                            let video_id = self.new_id();
                            self.videos.push(build_video(video_id, albedo, png));
                            let texture_id = self.new_id();
                            self.textures.push(build_texture(texture_id, albedo));
                            self.connect_oo(video_id, texture_id);
                            tex_id_by_png.insert(albedo, texture_id);
                            Some(texture_id)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let mat_name = match albedo_id {
                        Some(id) => format!("mat_{}_albedo{}", asset.name, id),
                        None => format!("mat_{}_shader{}", asset.name, mesh.shader_index),
                    };
                    self.materials
                        .push(build_material(new_mat_id, &mat_name, tex_node_id.is_some()));
                    if let Some(tex_node) = tex_node_id {
                        self.connect_op(tex_node, new_mat_id, "DiffuseColor");
                    }
                    mat_id_by_key.insert(key, new_mat_id);
                    new_mat_id
                };

                let geom_id = self.new_id();
                let model_id = self.new_id();
                let submesh_name = format!("{}_sm{}", asset.name, submesh_counter);
                submesh_counter += 1;
                self.geometries.push(build_geometry(
                    geom_id,
                    &submesh_name,
                    &mesh.positions,
                    &mesh.uvs,
                    &mesh.indices,
                ));
                self.models.push(build_mesh_model(model_id, &submesh_name));
                self.connect_oo(model_id, parent_id);
                self.connect_oo(geom_id, model_id);
                self.connect_oo(mat_id, model_id);
                mesh_targets.push((geom_id, model_id, mat_id, bi, mi));
            }
        }

        let mut bone_model_ids: Vec<i64> = Vec::new();
        if let Some(skel) = asset.skeleton.as_ref() {
            if !skel.bones.is_empty() && !skel.bind_local.is_empty() {
                self.append_skeleton_and_skin(
                    &asset.name,
                    skel,
                    &asset.bangles,
                    &mesh_targets,
                    parent_id,
                    &mut bone_model_ids,
                );
                if !clips.is_empty() && !bone_model_ids.is_empty() {
                    self.append_animation_clips(&asset.name, clips, &bone_model_ids);
                }
            }
        }
    }

    fn append_static_asset(
        &mut self,
        asset: &crate::level_glb::LevelGlbAsset,
        inst: &crate::level_glb::LevelGlbInstance,
        textures: &HashMap<u32, Vec<u8>>,
    ) {
        let parent_id = self.new_id();
        self.models.push(build_null_model(parent_id, &inst.name, inst));
        self.connect_oo(parent_id, ROOT_NODE_ID);

        let mut tex_id_by_png: HashMap<u32, i64> = HashMap::new();

        for (si, sub) in asset.submeshes.iter().enumerate() {
            if sub.positions.is_empty() || sub.indices.is_empty() {
                continue;
            }
            if sub.positions.len() % 3 != 0 || sub.indices.len() % 3 != 0 {
                continue;
            }
            let mat_id = self.new_id();
            let mat_name = format!("{}_mat{}", asset.name, si);
            let tex_node_id = if let Some(albedo) = sub.albedo_id {
                if let Some(id) = tex_id_by_png.get(&albedo) {
                    Some(*id)
                } else if let Some(png) = textures.get(&albedo) {
                    let video_id = self.new_id();
                    self.videos.push(build_video(video_id, albedo, png));
                    let texture_id = self.new_id();
                    self.textures.push(build_texture(texture_id, albedo));
                    self.connect_oo(video_id, texture_id);
                    tex_id_by_png.insert(albedo, texture_id);
                    Some(texture_id)
                } else {
                    None
                }
            } else {
                None
            };
            self.materials
                .push(build_material(mat_id, &mat_name, tex_node_id.is_some()));
            if let Some(tex_node) = tex_node_id {
                self.connect_op(tex_node, mat_id, "DiffuseColor");
            }

            let geom_id = self.new_id();
            let model_id = self.new_id();
            let submesh_name = format!("{}_s{}", asset.name, si);
            self.geometries.push(build_geometry(
                geom_id,
                &submesh_name,
                &sub.positions,
                &sub.uvs,
                &sub.indices,
            ));
            self.models.push(build_mesh_model(model_id, &submesh_name));
            self.connect_oo(model_id, parent_id);
            self.connect_oo(geom_id, model_id);
            self.connect_oo(mat_id, model_id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn append_skeleton_and_skin(
        &mut self,
        asset_name: &str,
        skel: &Skeleton,
        bangles: &[crate::moby::MobyBangle],
        mesh_targets: &[(i64, i64, i64, usize, usize)],
        parent_node_id: i64,
        out_bone_model_ids: &mut Vec<i64>,
    ) {
        let bone_count = skel.bones.len();
        let bind_world = compute_bind_world_matrices(skel);
        let mut bone_ids: Vec<i64> = Vec::with_capacity(bone_count);
        let mut bone_attr_ids: Vec<i64> = Vec::with_capacity(bone_count);

        for i in 0..bone_count {
            let local = skel
                .bind_local
                .get(i)
                .copied()
                .unwrap_or_else(mat4_identity);
            let (translation, scale, quat) = decompose_col_major(&local);
            let (rx, ry, rz) = quat_to_euler_xyz_degrees(quat);
            let bone_name = format!("{}_bone_{:03}", asset_name, i);

            let attr_id = self.new_id();
            self.node_attrs
                .push(build_limb_node_attribute(attr_id, &bone_name));

            let model_id = self.new_id();
            self.models.push(build_limb_node_model(
                model_id,
                &bone_name,
                translation,
                [rx, ry, rz],
                scale,
                attr_id,
            ));
            self.connect_oo(attr_id, model_id);
            bone_ids.push(model_id);
            bone_attr_ids.push(attr_id);
        }
        *out_bone_model_ids = bone_ids.clone();

        for i in 0..bone_count {
            let parent = skel.bones[i].parent_index;
            let parent_target = if parent < 0
                || (parent as usize) == i
                || (parent as usize) >= bone_count
            {
                parent_node_id
            } else {
                bone_ids[parent as usize]
            };
            self.connect_oo(bone_ids[i], parent_target);
        }

        let mut pose_entries: Vec<(i64, [f32; 16])> = Vec::with_capacity(bone_count + 1);
        pose_entries.push((parent_node_id, mat4_identity()));
        for i in 0..bone_count {
            pose_entries.push((bone_ids[i], bind_world[i]));
        }
        let pose_id = self.new_id();
        self.poses.push(build_pose(
            pose_id,
            &format!("{}_BindPose", asset_name),
            &pose_entries,
        ));

        let mut per_bone: HashMap<i64, Vec<(i32, f32)>> = HashMap::new();
        for (geom_id, _model_id, _mat_id, bi, mi) in mesh_targets {
            let mesh = match bangles.get(*bi).and_then(|b| b.meshes.get(*mi)) {
                Some(m) => m,
                None => continue,
            };
            if mesh.bone_indices.is_empty() || mesh.bone_weights.is_empty() {
                continue;
            }
            let vertex_count = mesh.positions.len() / 3;
            if mesh.bone_indices.len() < vertex_count * 4
                || mesh.bone_weights.len() < vertex_count * 4
            {
                continue;
            }
            per_bone.clear();
            for v in 0..vertex_count {
                let base = v * 4;
                for slot in 0..4 {
                    let bone = mesh.bone_indices[base + slot] as usize;
                    let weight = mesh.bone_weights[base + slot];
                    if weight == 0 || bone >= bone_count {
                        continue;
                    }
                    per_bone
                        .entry(bone_ids[bone])
                        .or_default()
                        .push((v as i32, weight as f32 / 255.0));
                }
            }
            if per_bone.is_empty() {
                continue;
            }
            let deformer_id = self.new_id();
            self.deformers.push(build_skin_deformer(deformer_id, asset_name));
            self.connect_oo(deformer_id, *geom_id);

            let mut sorted_bones: Vec<i64> = per_bone.keys().copied().collect();
            sorted_bones.sort_unstable();
            for bone_model_id in sorted_bones {
                let entries = &per_bone[&bone_model_id];
                let bone_index = bone_ids
                    .iter()
                    .position(|&id| id == bone_model_id)
                    .unwrap_or(0);
                let transform = skel
                    .bind_world_inverse
                    .get(bone_index)
                    .copied()
                    .unwrap_or_else(mat4_identity);
                let transform_link = bind_world[bone_index];
                let sub_id = self.new_id();
                self.sub_deformers.push(build_sub_deformer(
                    sub_id,
                    &format!("{}_cluster_{}", asset_name, bone_index),
                    entries,
                    &transform,
                    &transform_link,
                ));
                self.connect_oo(sub_id, deformer_id);
                self.connect_oo(bone_model_id, sub_id);
            }
        }
    }

    fn append_animation_clips(
        &mut self,
        asset_name: &str,
        clips: &[DecodedClip],
        bone_model_ids: &[i64],
    ) {
        for (clip_idx, clip) in clips.iter().enumerate() {
            let frame_count = clip.num_frames as usize;
            if frame_count == 0 || clip.bones.is_empty() {
                continue;
            }
            let fps = if clip.frame_rate > 0.0 {
                clip.frame_rate
            } else {
                30.0
            };
            let dt = 1.0 / fps as f64;
            let mut times: Vec<i64> = Vec::with_capacity(frame_count);
            for f in 0..frame_count {
                let t = f as f64 * dt;
                times.push((t * KTIME_PER_SECOND as f64) as i64);
            }
            let stop_t = *times.last().unwrap_or(&0);

            let stack_id = self.new_id();
            let clip_name = if clip.name.is_empty() {
                format!("{}_clip_{}", asset_name, clip_idx)
            } else {
                sanitize_name(&clip.name)
            };
            self.anim_stacks
                .push(build_anim_stack(stack_id, &clip_name, stop_t));

            let layer_id = self.new_id();
            self.anim_layers.push(build_anim_layer(layer_id, &clip_name));
            self.connect_oo(layer_id, stack_id);

            let limit = clip.bones.len().min(bone_model_ids.len());
            for bone_i in 0..limit {
                let bone = &clip.bones[bone_i];
                let bone_model_id = bone_model_ids[bone_i];
                if !bone.translations.is_empty() {
                    self.emit_channel(
                        layer_id,
                        bone_model_id,
                        "Lcl Translation",
                        &times,
                        &bone.translations,
                    );
                }
                if !bone.scales.is_empty() {
                    self.emit_channel(
                        layer_id,
                        bone_model_id,
                        "Lcl Scaling",
                        &times,
                        &bone.scales,
                    );
                }
                if !bone.rotations.is_empty() {
                    let euler = quaternions_to_euler_track(&bone.rotations);
                    self.emit_channel(
                        layer_id,
                        bone_model_id,
                        "Lcl Rotation",
                        &times,
                        &euler,
                    );
                }
            }
        }
    }

    fn emit_channel(
        &mut self,
        layer_id: i64,
        bone_model_id: i64,
        property_name: &str,
        times: &[i64],
        values: &[f32],
    ) {
        if values.is_empty() || times.is_empty() {
            return;
        }
        let default_xyz = if values.len() >= 3 {
            [values[0] as f64, values[1] as f64, values[2] as f64]
        } else {
            [0.0, 0.0, 0.0]
        };
        let curve_node_id = self.new_id();
        self.anim_curve_nodes
            .push(build_anim_curve_node(curve_node_id, property_name, default_xyz));
        self.connect_oo(curve_node_id, layer_id);
        self.connect_op(curve_node_id, bone_model_id, property_name);

        let axes = ["X", "Y", "Z"];
        let frame_count = times.len();
        for axis_i in 0..3 {
            let mut comp: Vec<f32> = Vec::with_capacity(frame_count);
            for f in 0..frame_count {
                let idx = f * 3 + axis_i;
                comp.push(if idx < values.len() {
                    values[idx]
                } else {
                    *comp.last().unwrap_or(&0.0)
                });
            }
            let curve_id = self.new_id();
            self.anim_curves
                .push(build_anim_curve(curve_id, times, &comp));
            self.connect_op(curve_id, curve_node_id, &format!("d|{}", axes[axis_i]));
        }
    }

    fn finish(self, scene_name: &str) -> Vec<FbxNode> {
        let geom_count = self.geometries.len();
        let model_count = self.models.len();
        let mat_count = self.materials.len();
        let tex_count = self.textures.len();
        let vid_count = self.videos.len();
        let attr_count = self.node_attrs.len();
        let deformer_count = self.deformers.len() + self.sub_deformers.len();
        let pose_count = self.poses.len();
        let stack_count = self.anim_stacks.len();
        let layer_count = self.anim_layers.len();
        let curve_node_count = self.anim_curve_nodes.len();
        let curve_count = self.anim_curves.len();

        let mut roots: Vec<FbxNode> = Vec::new();
        roots.push(build_header_extension());
        roots.push(build_global_settings());
        roots.push(FbxNode::new("CreationTime").with_prop(FbxProperty::str("2026-01-01 12:00:00:000")));
        roots.push(FbxNode::new("Creator").with_prop(FbxProperty::str("ReChimera FBX exporter v0.3.5")));
        roots.push(build_documents(scene_name));
        roots.push(FbxNode::new("References"));
        roots.push(build_definitions(
            geom_count,
            model_count,
            mat_count,
            tex_count,
            vid_count,
            attr_count,
            deformer_count,
            pose_count,
            stack_count,
            layer_count,
            curve_node_count,
            curve_count,
        ));

        let mut objects = FbxNode::new("Objects");
        for n in self.geometries {
            objects.children.push(n);
        }
        for n in self.models {
            objects.children.push(n);
        }
        for n in self.node_attrs {
            objects.children.push(n);
        }
        for n in self.materials {
            objects.children.push(n);
        }
        for n in self.textures {
            objects.children.push(n);
        }
        for n in self.videos {
            objects.children.push(n);
        }
        for n in self.deformers {
            objects.children.push(n);
        }
        for n in self.sub_deformers {
            objects.children.push(n);
        }
        for n in self.poses {
            objects.children.push(n);
        }
        for n in self.anim_stacks {
            objects.children.push(n);
        }
        for n in self.anim_layers {
            objects.children.push(n);
        }
        for n in self.anim_curve_nodes {
            objects.children.push(n);
        }
        for n in self.anim_curves {
            objects.children.push(n);
        }
        roots.push(objects);

        let mut connections = FbxNode::new("Connections");
        connections.children = self.connections;
        roots.push(connections);

        roots.push(FbxNode::new("Takes").with_prop(FbxProperty::str("")));
        roots
    }
}

fn resolve_albedo(
    shaders: &HashMap<u64, ShaderInfo>,
    shader_tuids: &[u64],
    shader_index: u16,
) -> Option<u32> {
    let st = shader_tuids.get(shader_index as usize)?;
    shaders.get(st)?.albedo_tex_id
}

fn build_header_extension() -> FbxNode {
    let mut header = FbxNode::new("FBXHeaderExtension");
    header.children.push(FbxNode::new("FBXHeaderVersion").with_prop(FbxProperty::I32(1003)));
    header.children.push(FbxNode::new("FBXVersion").with_prop(FbxProperty::I32(7400)));
    let mut ts = FbxNode::new("CreationTimeStamp");
    ts.children.push(FbxNode::new("Version").with_prop(FbxProperty::I32(1000)));
    ts.children.push(FbxNode::new("Year").with_prop(FbxProperty::I32(2026)));
    ts.children.push(FbxNode::new("Month").with_prop(FbxProperty::I32(1)));
    ts.children.push(FbxNode::new("Day").with_prop(FbxProperty::I32(1)));
    ts.children.push(FbxNode::new("Hour").with_prop(FbxProperty::I32(12)));
    ts.children.push(FbxNode::new("Minute").with_prop(FbxProperty::I32(0)));
    ts.children.push(FbxNode::new("Second").with_prop(FbxProperty::I32(0)));
    ts.children.push(FbxNode::new("Millisecond").with_prop(FbxProperty::I32(0)));
    header.children.push(ts);
    header
        .children
        .push(FbxNode::new("Creator").with_prop(FbxProperty::str("ReChimera FBX writer")));
    let mut scene_info = FbxNode::new("SceneInfo");
    scene_info.properties.push(FbxProperty::str("GlobalInfo\u{0}\u{1}SceneInfo"));
    scene_info.properties.push(FbxProperty::str("UserData"));
    scene_info.children.push(FbxNode::new("Type").with_prop(FbxProperty::str("UserData")));
    scene_info.children.push(FbxNode::new("Version").with_prop(FbxProperty::I32(100)));
    let mut meta = FbxNode::new("MetaData");
    meta.children.push(FbxNode::new("Version").with_prop(FbxProperty::I32(100)));
    meta.children.push(FbxNode::new("Title").with_prop(FbxProperty::str("")));
    meta.children.push(FbxNode::new("Subject").with_prop(FbxProperty::str("")));
    meta.children.push(FbxNode::new("Author").with_prop(FbxProperty::str("")));
    meta.children.push(FbxNode::new("Keywords").with_prop(FbxProperty::str("")));
    meta.children.push(FbxNode::new("Revision").with_prop(FbxProperty::str("")));
    meta.children.push(FbxNode::new("Comment").with_prop(FbxProperty::str("")));
    scene_info.children.push(meta);
    header.children.push(scene_info);
    header
}

fn build_global_settings() -> FbxNode {
    let mut gs = FbxNode::new("GlobalSettings");
    gs.children.push(FbxNode::new("Version").with_prop(FbxProperty::I32(1000)));
    let mut props = FbxNode::new("Properties70");
    let pairs: &[(&str, &str, &str, &str, &[FbxProperty])] = &[
        ("UpAxis", "int", "Integer", "", &[FbxProperty::I32(1)]),
        ("UpAxisSign", "int", "Integer", "", &[FbxProperty::I32(1)]),
        ("FrontAxis", "int", "Integer", "", &[FbxProperty::I32(2)]),
        ("FrontAxisSign", "int", "Integer", "", &[FbxProperty::I32(1)]),
        ("CoordAxis", "int", "Integer", "", &[FbxProperty::I32(0)]),
        ("CoordAxisSign", "int", "Integer", "", &[FbxProperty::I32(1)]),
        ("OriginalUpAxis", "int", "Integer", "", &[FbxProperty::I32(1)]),
        ("OriginalUpAxisSign", "int", "Integer", "", &[FbxProperty::I32(1)]),
        ("UnitScaleFactor", "double", "Number", "", &[FbxProperty::F64(1.0)]),
        ("OriginalUnitScaleFactor", "double", "Number", "", &[FbxProperty::F64(1.0)]),
        ("AmbientColor", "ColorRGB", "Color", "", &[FbxProperty::F64(0.0), FbxProperty::F64(0.0), FbxProperty::F64(0.0)]),
        ("DefaultCamera", "KString", "", "", &[FbxProperty::str("Producer Perspective")]),
        ("TimeMode", "enum", "", "", &[FbxProperty::I32(11)]),
        ("TimeProtocol", "enum", "", "", &[FbxProperty::I32(2)]),
        ("SnapOnFrameMode", "enum", "", "", &[FbxProperty::I32(0)]),
        ("TimeSpanStart", "KTime", "Time", "", &[FbxProperty::I64(0)]),
        ("TimeSpanStop", "KTime", "Time", "", &[FbxProperty::I64(0)]),
        ("CustomFrameRate", "double", "Number", "", &[FbxProperty::F64(-1.0)]),
    ];
    for (name, typ, sub, flag, vals) in pairs {
        props.children.push(make_property70_entry(name, typ, sub, flag, vals));
    }
    gs.children.push(props);
    gs
}

fn make_property70_entry(name: &str, typ: &str, subtype: &str, flag: &str, values: &[FbxProperty]) -> FbxNode {
    let mut p = FbxNode::new("P");
    p.properties.push(FbxProperty::str(name));
    p.properties.push(FbxProperty::str(typ));
    p.properties.push(FbxProperty::str(subtype));
    p.properties.push(FbxProperty::str(flag));
    for v in values {
        p.properties.push(v.clone());
    }
    p
}

fn build_documents(scene_name: &str) -> FbxNode {
    let mut docs = FbxNode::new("Documents");
    docs.children.push(FbxNode::new("Count").with_prop(FbxProperty::I32(1)));
    let mut doc = FbxNode::new("Document");
    doc.properties.push(FbxProperty::I64(DOCUMENT_ID));
    doc.properties.push(FbxProperty::obj_name(&sanitize_name(scene_name), "Scene"));
    doc.properties.push(FbxProperty::str("Scene"));
    let mut props = FbxNode::new("Properties70");
    props.children.push(make_property70_entry("SourceObject", "object", "", "", &[]));
    props.children.push(make_property70_entry(
        "ActiveAnimStackName",
        "KString",
        "",
        "",
        &[FbxProperty::str("")],
    ));
    doc.children.push(props);
    doc.children.push(FbxNode::new("RootNode").with_prop(FbxProperty::I64(0)));
    docs.children.push(doc);
    docs
}

#[allow(clippy::too_many_arguments)]
fn build_definitions(
    geom_count: usize,
    model_count: usize,
    mat_count: usize,
    tex_count: usize,
    vid_count: usize,
    attr_count: usize,
    deformer_count: usize,
    pose_count: usize,
    stack_count: usize,
    layer_count: usize,
    curve_node_count: usize,
    curve_count: usize,
) -> FbxNode {
    let mut defs = FbxNode::new("Definitions");
    defs.children.push(FbxNode::new("Version").with_prop(FbxProperty::I32(100)));
    let mut total = 1usize;
    for c in [geom_count, model_count, mat_count, tex_count, vid_count, attr_count, deformer_count, pose_count, stack_count, layer_count, curve_node_count, curve_count] {
        if c > 0 {
            total += 1;
        }
    }
    defs.children.push(FbxNode::new("Count").with_prop(FbxProperty::I32(total as i32)));

    let mut gs_type = FbxNode::new("ObjectType");
    gs_type.properties.push(FbxProperty::str("GlobalSettings"));
    gs_type.children.push(FbxNode::new("Count").with_prop(FbxProperty::I32(1)));
    defs.children.push(gs_type);

    let push_simple = |defs: &mut FbxNode, name: &str, count: usize| {
        if count == 0 {
            return;
        }
        let mut t = FbxNode::new("ObjectType");
        t.properties.push(FbxProperty::str(name));
        t.children.push(FbxNode::new("Count").with_prop(FbxProperty::I32(count as i32)));
        defs.children.push(t);
    };
    push_simple(&mut defs, "Geometry", geom_count);
    push_simple(&mut defs, "Model", model_count);
    push_simple(&mut defs, "NodeAttribute", attr_count);
    push_simple(&mut defs, "Material", mat_count);
    push_simple(&mut defs, "Texture", tex_count);
    push_simple(&mut defs, "Video", vid_count);
    push_simple(&mut defs, "Deformer", deformer_count);
    push_simple(&mut defs, "Pose", pose_count);
    push_simple(&mut defs, "AnimationStack", stack_count);
    push_simple(&mut defs, "AnimationLayer", layer_count);
    push_simple(&mut defs, "AnimationCurveNode", curve_node_count);
    push_simple(&mut defs, "AnimationCurve", curve_count);
    defs
}

fn build_geometry(id: i64, name: &str, positions: &[f32], uvs: &[f32], indices: &[u32]) -> FbxNode {
    let mut g = FbxNode::new("Geometry");
    g.properties.push(FbxProperty::I64(id));
    g.properties.push(FbxProperty::obj_name(name, "Geometry"));
    g.properties.push(FbxProperty::str("Mesh"));

    let verts_f64: Vec<f64> = positions.iter().map(|v| *v as f64).collect();
    g.children
        .push(FbxNode::new("Vertices").with_prop(FbxProperty::F64Array(verts_f64)));

    let mut pvi: Vec<i32> = Vec::with_capacity(indices.len());
    for tri in indices.chunks_exact(3) {
        pvi.push(tri[0] as i32);
        pvi.push(tri[1] as i32);
        pvi.push(-(tri[2] as i32) - 1);
    }
    g.children
        .push(FbxNode::new("PolygonVertexIndex").with_prop(FbxProperty::I32Array(pvi)));
    g.children
        .push(FbxNode::new("GeometryVersion").with_prop(FbxProperty::I32(124)));

    let vertex_count = positions.len() / 3;
    let has_uvs = uvs.len() == vertex_count * 2;
    if has_uvs {
        let mut uv_layer = FbxNode::new("LayerElementUV");
        uv_layer.properties.push(FbxProperty::I32(0));
        uv_layer
            .children
            .push(FbxNode::new("Version").with_prop(FbxProperty::I32(101)));
        uv_layer
            .children
            .push(FbxNode::new("Name").with_prop(FbxProperty::str("UVMap")));
        uv_layer.children.push(
            FbxNode::new("MappingInformationType")
                .with_prop(FbxProperty::str("ByPolygonVertex")),
        );
        uv_layer.children.push(
            FbxNode::new("ReferenceInformationType")
                .with_prop(FbxProperty::str("IndexToDirect")),
        );
        let mut uv_f64: Vec<f64> = Vec::with_capacity(uvs.len());
        for (i, v) in uvs.iter().enumerate() {
            let val = if i % 2 == 1 { 1.0 - *v } else { *v };
            uv_f64.push(val as f64);
        }
        uv_layer
            .children
            .push(FbxNode::new("UV").with_prop(FbxProperty::F64Array(uv_f64)));
        let uv_index: Vec<i32> = indices.iter().map(|i| *i as i32).collect();
        uv_layer
            .children
            .push(FbxNode::new("UVIndex").with_prop(FbxProperty::I32Array(uv_index)));
        g.children.push(uv_layer);
    }

    let mut mat_layer = FbxNode::new("LayerElementMaterial");
    mat_layer.properties.push(FbxProperty::I32(0));
    mat_layer
        .children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(101)));
    mat_layer
        .children
        .push(FbxNode::new("Name").with_prop(FbxProperty::str("")));
    mat_layer
        .children
        .push(FbxNode::new("MappingInformationType").with_prop(FbxProperty::str("AllSame")));
    mat_layer.children.push(
        FbxNode::new("ReferenceInformationType").with_prop(FbxProperty::str("IndexToDirect")),
    );
    mat_layer
        .children
        .push(FbxNode::new("Materials").with_prop(FbxProperty::I32Array(vec![0])));
    g.children.push(mat_layer);

    let mut layer = FbxNode::new("Layer");
    layer.properties.push(FbxProperty::I32(0));
    layer
        .children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(100)));
    if has_uvs {
        let mut le = FbxNode::new("LayerElement");
        le.children
            .push(FbxNode::new("Type").with_prop(FbxProperty::str("LayerElementUV")));
        le.children
            .push(FbxNode::new("TypedIndex").with_prop(FbxProperty::I32(0)));
        layer.children.push(le);
    }
    let mut le_mat = FbxNode::new("LayerElement");
    le_mat
        .children
        .push(FbxNode::new("Type").with_prop(FbxProperty::str("LayerElementMaterial")));
    le_mat
        .children
        .push(FbxNode::new("TypedIndex").with_prop(FbxProperty::I32(0)));
    layer.children.push(le_mat);
    g.children.push(layer);

    g
}

fn build_mesh_model(id: i64, name: &str) -> FbxNode {
    let mut m = FbxNode::new("Model");
    m.properties.push(FbxProperty::I64(id));
    m.properties.push(FbxProperty::obj_name(name, "Model"));
    m.properties.push(FbxProperty::str("Mesh"));
    m.children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(232)));
    let mut props = FbxNode::new("Properties70");
    props.children.push(make_property70_entry("RotationActive", "bool", "", "", &[FbxProperty::I32(1)]));
    props.children.push(make_property70_entry("InheritType", "enum", "", "", &[FbxProperty::I32(1)]));
    props.children.push(make_property70_entry(
        "ScalingMax",
        "Vector3D",
        "Vector",
        "",
        &[FbxProperty::F64(0.0), FbxProperty::F64(0.0), FbxProperty::F64(0.0)],
    ));
    props.children.push(make_property70_entry(
        "DefaultAttributeIndex",
        "int",
        "Integer",
        "",
        &[FbxProperty::I32(0)],
    ));
    m.children.push(props);
    m.children.push(FbxNode::new("Shading").with_prop(FbxProperty::Bool(true)));
    m.children
        .push(FbxNode::new("Culling").with_prop(FbxProperty::str("CullingOff")));
    m
}

fn build_null_model(id: i64, name: &str, inst: &crate::level_glb::LevelGlbInstance) -> FbxNode {
    let mut m = FbxNode::new("Model");
    m.properties.push(FbxProperty::I64(id));
    m.properties.push(FbxProperty::obj_name(&sanitize_name(name), "Model"));
    m.properties.push(FbxProperty::str("Null"));
    m.children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(232)));
    let mut props = FbxNode::new("Properties70");
    let (ex, ey, ez) = quat_to_euler_xyz_degrees(inst.rotation);
    props.children.push(make_property70_entry(
        "Lcl Translation",
        "Lcl Translation",
        "",
        "A",
        &[
            FbxProperty::F64(inst.translation[0] as f64),
            FbxProperty::F64(inst.translation[1] as f64),
            FbxProperty::F64(inst.translation[2] as f64),
        ],
    ));
    props.children.push(make_property70_entry(
        "Lcl Rotation",
        "Lcl Rotation",
        "",
        "A",
        &[
            FbxProperty::F64(ex as f64),
            FbxProperty::F64(ey as f64),
            FbxProperty::F64(ez as f64),
        ],
    ));
    props.children.push(make_property70_entry(
        "Lcl Scaling",
        "Lcl Scaling",
        "",
        "A",
        &[
            FbxProperty::F64(inst.scale[0] as f64),
            FbxProperty::F64(inst.scale[1] as f64),
            FbxProperty::F64(inst.scale[2] as f64),
        ],
    ));
    props.children.push(make_property70_entry("DefaultAttributeIndex", "int", "Integer", "", &[FbxProperty::I32(0)]));
    props.children.push(make_property70_entry("InheritType", "enum", "", "", &[FbxProperty::I32(1)]));
    m.children.push(props);
    m.children.push(FbxNode::new("Shading").with_prop(FbxProperty::Bool(true)));
    m.children
        .push(FbxNode::new("Culling").with_prop(FbxProperty::str("CullingOff")));
    m
}

fn build_limb_node_attribute(id: i64, name: &str) -> FbxNode {
    let mut a = FbxNode::new("NodeAttribute");
    a.properties.push(FbxProperty::I64(id));
    a.properties.push(FbxProperty::obj_name(name, "NodeAttribute"));
    a.properties.push(FbxProperty::str("LimbNode"));
    let mut props = FbxNode::new("Properties70");
    props.children.push(make_property70_entry("Size", "double", "Number", "", &[FbxProperty::F64(1.0)]));
    a.children.push(props);
    a.children
        .push(FbxNode::new("TypeFlags").with_prop(FbxProperty::str("Skeleton")));
    a
}

fn build_limb_node_model(
    id: i64,
    name: &str,
    translation: [f32; 3],
    rotation_deg: [f32; 3],
    scale: [f32; 3],
    _attr_id: i64,
) -> FbxNode {
    let mut m = FbxNode::new("Model");
    m.properties.push(FbxProperty::I64(id));
    m.properties.push(FbxProperty::obj_name(name, "Model"));
    m.properties.push(FbxProperty::str("LimbNode"));
    m.children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(232)));
    let mut props = FbxNode::new("Properties70");
    props.children.push(make_property70_entry("InheritType", "enum", "", "", &[FbxProperty::I32(1)]));
    props.children.push(make_property70_entry("DefaultAttributeIndex", "int", "Integer", "", &[FbxProperty::I32(0)]));
    props.children.push(make_property70_entry(
        "Lcl Translation",
        "Lcl Translation",
        "",
        "A",
        &[
            FbxProperty::F64(translation[0] as f64),
            FbxProperty::F64(translation[1] as f64),
            FbxProperty::F64(translation[2] as f64),
        ],
    ));
    props.children.push(make_property70_entry(
        "Lcl Rotation",
        "Lcl Rotation",
        "",
        "A",
        &[
            FbxProperty::F64(rotation_deg[0] as f64),
            FbxProperty::F64(rotation_deg[1] as f64),
            FbxProperty::F64(rotation_deg[2] as f64),
        ],
    ));
    props.children.push(make_property70_entry(
        "Lcl Scaling",
        "Lcl Scaling",
        "",
        "A",
        &[
            FbxProperty::F64(scale[0] as f64),
            FbxProperty::F64(scale[1] as f64),
            FbxProperty::F64(scale[2] as f64),
        ],
    ));
    m.children.push(props);
    m.children.push(FbxNode::new("Shading").with_prop(FbxProperty::Bool(true)));
    m.children
        .push(FbxNode::new("Culling").with_prop(FbxProperty::str("CullingOff")));
    m
}

fn build_material(id: i64, name: &str, has_albedo: bool) -> FbxNode {
    let mut mat = FbxNode::new("Material");
    mat.properties.push(FbxProperty::I64(id));
    mat.properties.push(FbxProperty::obj_name(&sanitize_name(name), "Material"));
    mat.properties.push(FbxProperty::str(""));
    mat.children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(102)));
    mat.children
        .push(FbxNode::new("ShadingModel").with_prop(FbxProperty::str("Phong")));
    mat.children
        .push(FbxNode::new("MultiLayer").with_prop(FbxProperty::I32(0)));
    let diffuse = if has_albedo { [1.0, 1.0, 1.0] } else { [0.8, 0.8, 0.8] };
    let mut props = FbxNode::new("Properties70");
    props.children.push(make_property70_entry(
        "AmbientColor",
        "Color",
        "",
        "A",
        &[FbxProperty::F64(0.0), FbxProperty::F64(0.0), FbxProperty::F64(0.0)],
    ));
    props.children.push(make_property70_entry(
        "DiffuseColor",
        "Color",
        "",
        "A",
        &[
            FbxProperty::F64(diffuse[0]),
            FbxProperty::F64(diffuse[1]),
            FbxProperty::F64(diffuse[2]),
        ],
    ));
    props.children.push(make_property70_entry(
        "DiffuseFactor",
        "Number",
        "",
        "A",
        &[FbxProperty::F64(1.0)],
    ));
    props.children.push(make_property70_entry(
        "SpecularColor",
        "Color",
        "",
        "A",
        &[FbxProperty::F64(0.0), FbxProperty::F64(0.0), FbxProperty::F64(0.0)],
    ));
    props.children.push(make_property70_entry(
        "SpecularFactor",
        "Number",
        "",
        "A",
        &[FbxProperty::F64(0.0)],
    ));
    props.children.push(make_property70_entry(
        "ShininessExponent",
        "Number",
        "",
        "A",
        &[FbxProperty::F64(2.0)],
    ));
    props.children.push(make_property70_entry(
        "Emissive",
        "Vector3D",
        "Vector",
        "",
        &[FbxProperty::F64(0.0), FbxProperty::F64(0.0), FbxProperty::F64(0.0)],
    ));
    props.children.push(make_property70_entry(
        "Opacity",
        "double",
        "Number",
        "",
        &[FbxProperty::F64(1.0)],
    ));
    mat.children.push(props);
    mat
}

fn build_texture(id: i64, png_id: u32) -> FbxNode {
    let filename = format!("tex_{}.png", png_id);
    let mut t = FbxNode::new("Texture");
    t.properties.push(FbxProperty::I64(id));
    t.properties.push(FbxProperty::obj_name(&format!("tex_{}", png_id), "Texture"));
    t.properties.push(FbxProperty::str(""));
    t.children
        .push(FbxNode::new("Type").with_prop(FbxProperty::str("TextureVideoClip")));
    t.children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(202)));
    t.children
        .push(FbxNode::new("TextureName").with_prop(FbxProperty::obj_name(&format!("tex_{}", png_id), "Texture")));
    let mut props = FbxNode::new("Properties70");
    props.children.push(make_property70_entry(
        "UVSet",
        "KString",
        "",
        "",
        &[FbxProperty::str("UVMap")],
    ));
    props.children.push(make_property70_entry(
        "UseMaterial",
        "bool",
        "",
        "",
        &[FbxProperty::I32(1)],
    ));
    t.children.push(props);
    t.children
        .push(FbxNode::new("Media").with_prop(FbxProperty::obj_name(&format!("tex_{}", png_id), "Video")));
    t.children
        .push(FbxNode::new("FileName").with_prop(FbxProperty::str(&filename)));
    t.children
        .push(FbxNode::new("RelativeFilename").with_prop(FbxProperty::str(&filename)));
    let mut mut_t = t;
    let mut model_uv_translation = FbxNode::new("ModelUVTranslation");
    model_uv_translation.properties.push(FbxProperty::F64(0.0));
    model_uv_translation.properties.push(FbxProperty::F64(0.0));
    mut_t.children.push(model_uv_translation);
    let mut model_uv_scaling = FbxNode::new("ModelUVScaling");
    model_uv_scaling.properties.push(FbxProperty::F64(1.0));
    model_uv_scaling.properties.push(FbxProperty::F64(1.0));
    mut_t.children.push(model_uv_scaling);
    mut_t.children.push(
        FbxNode::new("Texture_Alpha_Source").with_prop(FbxProperty::str("None")),
    );
    let mut crop = FbxNode::new("Cropping");
    crop.properties.push(FbxProperty::I32(0));
    crop.properties.push(FbxProperty::I32(0));
    crop.properties.push(FbxProperty::I32(0));
    crop.properties.push(FbxProperty::I32(0));
    mut_t.children.push(crop);
    mut_t
}

fn build_video(id: i64, png_id: u32, png_bytes: &[u8]) -> FbxNode {
    let filename = format!("tex_{}.png", png_id);
    let mut v = FbxNode::new("Video");
    v.properties.push(FbxProperty::I64(id));
    v.properties.push(FbxProperty::obj_name(&format!("tex_{}", png_id), "Video"));
    v.properties.push(FbxProperty::str("Clip"));
    v.children
        .push(FbxNode::new("Type").with_prop(FbxProperty::str("Clip")));
    let mut props = FbxNode::new("Properties70");
    props.children.push(make_property70_entry(
        "Path",
        "KString",
        "XRefUrl",
        "",
        &[FbxProperty::str(&filename)],
    ));
    v.children.push(props);
    v.children
        .push(FbxNode::new("UseMipMap").with_prop(FbxProperty::I32(0)));
    v.children
        .push(FbxNode::new("Filename").with_prop(FbxProperty::str(&filename)));
    v.children
        .push(FbxNode::new("RelativeFilename").with_prop(FbxProperty::str(&filename)));
    v.children
        .push(FbxNode::new("Content").with_prop(FbxProperty::Raw(png_bytes.to_vec())));
    v
}

fn build_skin_deformer(id: i64, asset_name: &str) -> FbxNode {
    let mut d = FbxNode::new("Deformer");
    d.properties.push(FbxProperty::I64(id));
    d.properties.push(FbxProperty::obj_name(&format!("{}_Skin", sanitize_name(asset_name)), "Deformer"));
    d.properties.push(FbxProperty::str("Skin"));
    d.children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(101)));
    d.children
        .push(FbxNode::new("Link_DeformAcuracy").with_prop(FbxProperty::F64(50.0)));
    d
}

fn build_sub_deformer(
    id: i64,
    name: &str,
    entries: &[(i32, f32)],
    transform: &[f32; 16],
    transform_link: &[f32; 16],
) -> FbxNode {
    let mut s = FbxNode::new("Deformer");
    s.properties.push(FbxProperty::I64(id));
    s.properties.push(FbxProperty::obj_name(&sanitize_name(name), "SubDeformer"));
    s.properties.push(FbxProperty::str("Cluster"));
    s.children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(100)));
    let mut user_data = FbxNode::new("UserData");
    user_data.properties.push(FbxProperty::str(""));
    user_data.properties.push(FbxProperty::str(""));
    s.children.push(user_data);
    let indexes: Vec<i32> = entries.iter().map(|(v, _w)| *v).collect();
    let weights: Vec<f64> = entries.iter().map(|(_v, w)| *w as f64).collect();
    s.children
        .push(FbxNode::new("Indexes").with_prop(FbxProperty::I32Array(indexes)));
    s.children
        .push(FbxNode::new("Weights").with_prop(FbxProperty::F64Array(weights)));
    let transform_f64: Vec<f64> = transform.iter().map(|v| *v as f64).collect();
    let transform_link_f64: Vec<f64> = transform_link.iter().map(|v| *v as f64).collect();
    s.children
        .push(FbxNode::new("Transform").with_prop(FbxProperty::F64Array(transform_f64)));
    s.children.push(
        FbxNode::new("TransformLink").with_prop(FbxProperty::F64Array(transform_link_f64)),
    );
    s
}

fn build_pose(id: i64, name: &str, entries: &[(i64, [f32; 16])]) -> FbxNode {
    let mut p = FbxNode::new("Pose");
    p.properties.push(FbxProperty::I64(id));
    p.properties.push(FbxProperty::obj_name(&sanitize_name(name), "Pose"));
    p.properties.push(FbxProperty::str("BindPose"));
    p.children
        .push(FbxNode::new("Type").with_prop(FbxProperty::str("BindPose")));
    p.children
        .push(FbxNode::new("Version").with_prop(FbxProperty::I32(100)));
    p.children
        .push(FbxNode::new("NbPoseNodes").with_prop(FbxProperty::I32(entries.len() as i32)));
    for (node_id, matrix) in entries {
        let mut pose_node = FbxNode::new("PoseNode");
        pose_node
            .children
            .push(FbxNode::new("Node").with_prop(FbxProperty::I64(*node_id)));
        let m_f64: Vec<f64> = matrix.iter().map(|v| *v as f64).collect();
        pose_node
            .children
            .push(FbxNode::new("Matrix").with_prop(FbxProperty::F64Array(m_f64)));
        p.children.push(pose_node);
    }
    p
}

fn build_anim_stack(id: i64, name: &str, stop_t: i64) -> FbxNode {
    let mut s = FbxNode::new("AnimationStack");
    s.properties.push(FbxProperty::I64(id));
    s.properties.push(FbxProperty::obj_name(&sanitize_name(name), "AnimStack"));
    s.properties.push(FbxProperty::str(""));
    let mut props = FbxNode::new("Properties70");
    props
        .children
        .push(make_property70_entry("LocalStart", "KTime", "Time", "", &[FbxProperty::I64(0)]));
    props.children.push(make_property70_entry(
        "LocalStop",
        "KTime",
        "Time",
        "",
        &[FbxProperty::I64(stop_t)],
    ));
    props.children.push(make_property70_entry(
        "ReferenceStart",
        "KTime",
        "Time",
        "",
        &[FbxProperty::I64(0)],
    ));
    props.children.push(make_property70_entry(
        "ReferenceStop",
        "KTime",
        "Time",
        "",
        &[FbxProperty::I64(stop_t)],
    ));
    s.children.push(props);
    s
}

fn build_anim_layer(id: i64, name: &str) -> FbxNode {
    let mut l = FbxNode::new("AnimationLayer");
    l.properties.push(FbxProperty::I64(id));
    l.properties.push(FbxProperty::obj_name(&sanitize_name(name), "AnimLayer"));
    l.properties.push(FbxProperty::str(""));
    l
}

fn build_anim_curve_node(id: i64, property_name: &str, default_xyz: [f64; 3]) -> FbxNode {
    let mut n = FbxNode::new("AnimationCurveNode");
    n.properties.push(FbxProperty::I64(id));
    n.properties.push(FbxProperty::obj_name(property_name, "AnimCurveNode"));
    n.properties.push(FbxProperty::str(""));
    let mut props = FbxNode::new("Properties70");
    props.children.push(make_property70_entry(
        "d|X",
        "Number",
        "",
        "A",
        &[FbxProperty::F64(default_xyz[0])],
    ));
    props.children.push(make_property70_entry(
        "d|Y",
        "Number",
        "",
        "A",
        &[FbxProperty::F64(default_xyz[1])],
    ));
    props.children.push(make_property70_entry(
        "d|Z",
        "Number",
        "",
        "A",
        &[FbxProperty::F64(default_xyz[2])],
    ));
    n.children.push(props);
    n
}

fn build_anim_curve(id: i64, times: &[i64], values: &[f32]) -> FbxNode {
    let mut c = FbxNode::new("AnimationCurve");
    c.properties.push(FbxProperty::I64(id));
    c.properties.push(FbxProperty::obj_name("", "AnimCurve"));
    c.properties.push(FbxProperty::str(""));
    let default = values.first().copied().unwrap_or(0.0) as f64;
    c.children
        .push(FbxNode::new("Default").with_prop(FbxProperty::F64(default)));
    c.children
        .push(FbxNode::new("KeyVer").with_prop(FbxProperty::I32(4008)));
    c.children
        .push(FbxNode::new("KeyTime").with_prop(FbxProperty::I64Array(times.to_vec())));
    let vals_f32: Vec<f32> = values.to_vec();
    c.children
        .push(FbxNode::new("KeyValueFloat").with_prop(FbxProperty::F32Array(vals_f32)));
    c.children
        .push(FbxNode::new("KeyAttrFlags").with_prop(FbxProperty::I32Array(vec![24840])));
    c.children.push(
        FbxNode::new("KeyAttrDataFloat")
            .with_prop(FbxProperty::F32Array(vec![0.0, 0.0, 218434821.0, 0.0])),
    );
    c.children.push(
        FbxNode::new("KeyAttrRefCount").with_prop(FbxProperty::I32Array(vec![values.len() as i32])),
    );
    c
}

fn quaternions_to_euler_track(rotations: &[f32]) -> Vec<f32> {
    let frame_count = rotations.len() / 4;
    let mut out: Vec<f32> = Vec::with_capacity(frame_count * 3);
    let mut prev: Option<[f32; 4]> = None;
    for f in 0..frame_count {
        let base = f * 4;
        let mut q = [
            rotations[base],
            rotations[base + 1],
            rotations[base + 2],
            rotations[base + 3],
        ];
        if let Some(p) = prev {
            let dot = p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3];
            if dot < 0.0 {
                q[0] = -q[0];
                q[1] = -q[1];
                q[2] = -q[2];
                q[3] = -q[3];
            }
        }
        prev = Some(q);
        let (rx, ry, rz) = quat_to_euler_xyz_degrees(q);
        out.push(rx);
        out.push(ry);
        out.push(rz);
    }
    out
}

fn quat_to_euler_xyz_degrees(q: [f32; 4]) -> (f32, f32, f32) {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let sinr_cosp = 2.0 * (w * x + y * z);
    let cosr_cosp = 1.0 - 2.0 * (x * x + y * y);
    let roll = sinr_cosp.atan2(cosr_cosp);

    let sinp = 2.0 * (w * y - z * x);
    let pitch = if sinp.abs() >= 1.0 {
        std::f32::consts::FRAC_PI_2.copysign(sinp)
    } else {
        sinp.asin()
    };

    let siny_cosp = 2.0 * (w * z + x * y);
    let cosy_cosp = 1.0 - 2.0 * (y * y + z * z);
    let yaw = siny_cosp.atan2(cosy_cosp);

    let to_deg = 180.0 / std::f32::consts::PI;
    (roll * to_deg, pitch * to_deg, yaw * to_deg)
}

fn mat4_identity() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ]
}

fn mat4_mul_col_major(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut r = [0.0f32; 16];
    for c in 0..4 {
        for row in 0..4 {
            let mut s = 0.0f32;
            for k in 0..4 {
                s += a[k * 4 + row] * b[c * 4 + k];
            }
            r[c * 4 + row] = s;
        }
    }
    r
}

fn compute_bind_world_matrices(skel: &Skeleton) -> Vec<[f32; 16]> {
    let n = skel.bones.len();
    let mut world = vec![mat4_identity(); n];
    let mut done = vec![false; n];
    loop {
        let mut progress = false;
        for i in 0..n {
            if done[i] {
                continue;
            }
            let local = skel.bind_local.get(i).copied().unwrap_or_else(mat4_identity);
            let parent = skel.bones[i].parent_index;
            if parent < 0 || parent as usize == i {
                world[i] = local;
                done[i] = true;
                progress = true;
            } else {
                let p = parent as usize;
                if p < n && done[p] {
                    world[i] = mat4_mul_col_major(&world[p], &local);
                    done[i] = true;
                    progress = true;
                }
            }
        }
        if !progress {
            break;
        }
    }
    for i in 0..n {
        if !done[i] {
            world[i] = skel.bind_local.get(i).copied().unwrap_or_else(mat4_identity);
        }
    }
    world
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == ' ' || c == ':' || c == '#' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}
