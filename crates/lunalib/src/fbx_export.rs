use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use crate::animation::DecodedClip;
use crate::error::{Error, Result};
use crate::math::decompose_col_major;
use crate::moby::MobyAsset;
use crate::shader::ShaderInfo;
use crate::skeleton::Skeleton;

const FBX_VERSION: u32 = 7400;
const ROOT_NODE_ID: u64 = 0;
const KTIME_PER_SECOND: i64 = 46186158000;

// three.js's FBXLoader has a bug in isFbxFormatASCII() (see FBXLoader.js:4039):
// instead of reading characters 0..19 sequentially, the broken `text.slice` +
// `cursor++` reads triangular positions 0,1,3,6,10,15,21,28,36,45,55,66,78,91,
// 105,120,136,153,171,190 and rejects the file as "Unknown format" if any of
// those matches the corresponding char of "Kaydara\FBX\Binary\\". The header
// below was hand-tuned so every trap position lands on a safe character
// (mostly '-' from the divider lines). DO NOT edit unless you re-verify all
// 20 positions — otherwise three.js (and anything wrapping it) silently
// rejects every FBX we emit.
const LEADING_COMMENT_BLOCK: &str = "\
; FBX 7.4.0 project file
; ----------------------------------------------------
; ASCII export by ReChimera (not Autodesk software).
; ----------------------------------------------------

";

pub fn write_moby_fbx(
    asset: &MobyAsset,
    clips: &[DecodedClip],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut b = FbxBuilder::new(&asset.name);
    append_moby_to_fbx(&mut b, asset, clips, shaders, textures, None)?;
    Ok(b.finish().into_bytes())
}

fn append_moby_to_fbx(
    b: &mut FbxBuilder,
    asset: &MobyAsset,
    clips: &[DecodedClip],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
    placement: Option<&crate::level_glb::LevelGlbInstance>,
) -> Result<()> {
    let mut tex_id_by_png: HashMap<u32, u64> = HashMap::new();
    let mut video_id_by_png: HashMap<u32, u64> = HashMap::new();
    let mut mat_id_by_shader: HashMap<(u16, Option<u32>), u64> = HashMap::new();

    let default_mat_id = b.next_id();
    b.objects_materials.push(emit_material(
        default_mat_id,
        &format!("default_{}", asset.name),
        false,
    ));

    let mut model_geom_pairs: Vec<(u64, u64, u64, usize, usize)> = Vec::new();
    let mut submesh_counter: u32 = 0;

    for (bi, bangle) in asset.bangles.iter().enumerate() {
        for (mi, mesh) in bangle.meshes.iter().enumerate() {
            if mesh.positions.is_empty() || mesh.indices.is_empty() {
                continue;
            }
            if mesh.positions.len() % 3 != 0 {
                continue;
            }
            if mesh.indices.len() % 3 != 0 {
                continue;
            }

            let albedo_id = resolve_albedo(shaders, &asset.shader_tuids, mesh.shader_index);
            let key = (mesh.shader_index, albedo_id);
            let mat_id = if let Some(id) = mat_id_by_shader.get(&key) {
                *id
            } else {
                let mat_name = match albedo_id {
                    Some(id) => format!("mat_{}_albedo{}", asset.name, id),
                    None => format!("mat_{}_shader{}", asset.name, mesh.shader_index),
                };
                let new_mat_id = b.next_id();

                let tex_id_for_mat = if let Some(albedo) = albedo_id {
                    let tex_id = if let Some(id) = tex_id_by_png.get(&albedo) {
                        *id
                    } else if let Some(png_bytes) = textures.get(&albedo) {
                        let video_id = b.next_id();
                        b.objects_videos
                            .push(emit_video(video_id, albedo, png_bytes));
                        video_id_by_png.insert(albedo, video_id);

                        let texture_id = b.next_id();
                        b.objects_textures
                            .push(emit_texture(texture_id, albedo));
                        tex_id_by_png.insert(albedo, texture_id);

                        b.connections
                            .push(format!("\tC: \"OO\", {},{}", video_id, texture_id));
                        texture_id
                    } else {
                        0
                    };
                    if tex_id != 0 {
                        Some(tex_id)
                    } else {
                        None
                    }
                } else {
                    None
                };

                b.objects_materials.push(emit_material(
                    new_mat_id,
                    &mat_name,
                    tex_id_for_mat.is_some(),
                ));
                if let Some(tex_id) = tex_id_for_mat {
                    b.connections.push(format!(
                        "\tC: \"OP\", {},{}, \"DiffuseColor\"",
                        tex_id, new_mat_id
                    ));
                }
                mat_id_by_shader.insert(key, new_mat_id);
                new_mat_id
            };

            let geom_id = b.next_id();
            let model_id = b.next_id();
            let submesh_name = format!("{}_sm{}", asset.name, submesh_counter);
            submesh_counter += 1;

            b.objects_geometries.push(emit_geometry(
                geom_id,
                &submesh_name,
                &mesh.positions,
                &mesh.uvs,
                &mesh.indices,
            ));
            b.objects_models.push(emit_mesh_model(model_id, &submesh_name));

            model_geom_pairs.push((geom_id, model_id, mat_id, bi, mi));
        }
    }

    if model_geom_pairs.is_empty() {
        return Err(Error::GltfWrite(format!(
            "moby '{}' has no exportable geometry",
            asset.name
        )));
    }

    let parent_node_id = if let Some(inst) = placement {
        let null_id = b.next_id();
        b.objects_models
            .push(emit_transform_model(null_id, &inst.name, inst));
        b.connections
            .push(format!("\tC: \"OO\", {},{}", null_id, ROOT_NODE_ID));
        null_id
    } else {
        ROOT_NODE_ID
    };

    for (geom_id, model_id, mat_id, _, _) in &model_geom_pairs {
        b.connections
            .push(format!("\tC: \"OO\", {},{}", model_id, parent_node_id));
        b.connections
            .push(format!("\tC: \"OO\", {},{}", geom_id, model_id));
        b.connections
            .push(format!("\tC: \"OO\", {},{}", mat_id, model_id));
    }

    let mut bone_model_ids: Vec<u64> = Vec::new();
    if let Some(skel) = asset.skeleton.as_ref() {
        if !skel.bones.is_empty() && !skel.bind_local.is_empty() {
            emit_skeleton_and_skin_with_parent(
                b,
                &asset.name,
                skel,
                &asset.bangles,
                &model_geom_pairs,
                &mut bone_model_ids,
                parent_node_id,
            );
            if !clips.is_empty() {
                emit_animation_clips(b, &asset.name, clips, &bone_model_ids);
            }
        }
    }

    let _ = default_mat_id;
    Ok(())
}

fn resolve_albedo(
    shaders: &HashMap<u64, ShaderInfo>,
    shader_tuids: &[u64],
    shader_index: u16,
) -> Option<u32> {
    let st = shader_tuids.get(shader_index as usize)?;
    let s = shaders.get(st)?;
    s.albedo_tex_id
}

struct FbxBuilder {
    next_id_counter: u64,
    asset_name: String,
    objects_geometries: Vec<String>,
    objects_models: Vec<String>,
    objects_materials: Vec<String>,
    objects_textures: Vec<String>,
    objects_videos: Vec<String>,
    objects_limb_models: Vec<String>,
    objects_deformers: Vec<String>,
    objects_sub_deformers: Vec<String>,
    objects_poses: Vec<String>,
    objects_anim_stacks: Vec<String>,
    objects_anim_layers: Vec<String>,
    objects_anim_curve_nodes: Vec<String>,
    objects_anim_curves: Vec<String>,
    connections: Vec<String>,
}

impl FbxBuilder {
    fn new(asset_name: &str) -> Self {
        Self {
            next_id_counter: 100,
            asset_name: asset_name.to_string(),
            objects_geometries: Vec::new(),
            objects_models: Vec::new(),
            objects_materials: Vec::new(),
            objects_textures: Vec::new(),
            objects_videos: Vec::new(),
            objects_limb_models: Vec::new(),
            objects_deformers: Vec::new(),
            objects_sub_deformers: Vec::new(),
            objects_poses: Vec::new(),
            objects_anim_stacks: Vec::new(),
            objects_anim_layers: Vec::new(),
            objects_anim_curve_nodes: Vec::new(),
            objects_anim_curves: Vec::new(),
            connections: Vec::new(),
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id_counter;
        self.next_id_counter += 1;
        id
    }

    fn finish(self) -> String {
        let geom_count = self.objects_geometries.len();
        let model_count = self.objects_models.len() + self.objects_limb_models.len();
        let mat_count = self.objects_materials.len();
        let tex_count = self.objects_textures.len();
        let vid_count = self.objects_videos.len();
        let deformer_count = self.objects_deformers.len() + self.objects_sub_deformers.len();
        let pose_count = self.objects_poses.len();
        let stack_count = self.objects_anim_stacks.len();
        let layer_count = self.objects_anim_layers.len();
        let curve_node_count = self.objects_anim_curve_nodes.len();
        let curve_count = self.objects_anim_curves.len();

        let mut s = String::new();
        s.push_str(LEADING_COMMENT_BLOCK);

        s.push_str(&header_block());
        s.push_str(&global_settings_block());
        s.push_str("CreationTime: \"2026-01-01 12:00:00:000\"\n");
        s.push_str("Creator: \"ReChimera FBX exporter v0.3.5\"\n\n");
        s.push_str(&documents_block(&self.asset_name));
        s.push_str("References:  {\n}\n\n");
        s.push_str(&definitions_block(
            geom_count,
            model_count,
            mat_count,
            tex_count,
            vid_count,
            deformer_count,
            pose_count,
            stack_count,
            layer_count,
            curve_node_count,
            curve_count,
        ));

        s.push_str("Objects:  {\n");
        for g in &self.objects_geometries {
            s.push_str(g);
        }
        for m in &self.objects_models {
            s.push_str(m);
        }
        for m in &self.objects_limb_models {
            s.push_str(m);
        }
        for m in &self.objects_materials {
            s.push_str(m);
        }
        for t in &self.objects_textures {
            s.push_str(t);
        }
        for v in &self.objects_videos {
            s.push_str(v);
        }
        for d in &self.objects_deformers {
            s.push_str(d);
        }
        for sd in &self.objects_sub_deformers {
            s.push_str(sd);
        }
        for p in &self.objects_poses {
            s.push_str(p);
        }
        for a in &self.objects_anim_stacks {
            s.push_str(a);
        }
        for a in &self.objects_anim_layers {
            s.push_str(a);
        }
        for a in &self.objects_anim_curve_nodes {
            s.push_str(a);
        }
        for a in &self.objects_anim_curves {
            s.push_str(a);
        }
        s.push_str("}\n\n");

        s.push_str("Connections:  {\n");
        for c in &self.connections {
            s.push_str(c);
            s.push('\n');
        }
        s.push_str("}\n\n");

        s.push_str("Takes:  {\n\tCurrent: \"\"\n}\n");
        s
    }
}

fn header_block() -> String {
    format!(
        "FBXHeaderExtension:  {{\n\
            \tFBXHeaderVersion: 1003\n\
            \tFBXVersion: {ver}\n\
            \tCreationTimeStamp:  {{\n\
                \t\tVersion: 1000\n\
                \t\tYear: 2026\n\
                \t\tMonth: 1\n\
                \t\tDay: 1\n\
                \t\tHour: 0\n\
                \t\tMinute: 0\n\
                \t\tSecond: 0\n\
                \t\tMillisecond: 0\n\
            \t}}\n\
            \tCreator: \"ReChimera FBX writer\"\n\
            \tSceneInfo: \"SceneInfo::GlobalInfo\", \"UserData\" {{\n\
                \t\tType: \"UserData\"\n\
                \t\tVersion: 100\n\
                \t\tMetaData:  {{\n\
                    \t\t\tVersion: 100\n\
                    \t\t\tTitle: \"\"\n\
                    \t\t\tSubject: \"\"\n\
                    \t\t\tAuthor: \"\"\n\
                    \t\t\tKeywords: \"\"\n\
                    \t\t\tRevision: \"\"\n\
                    \t\t\tComment: \"\"\n\
                \t\t}}\n\
            \t}}\n\
        }}\n\n",
        ver = FBX_VERSION
    )
}

fn global_settings_block() -> String {
    "GlobalSettings:  {\n\
        \tVersion: 1000\n\
        \tProperties70:  {\n\
            \t\tP: \"UpAxis\", \"int\", \"Integer\", \"\",1\n\
            \t\tP: \"UpAxisSign\", \"int\", \"Integer\", \"\",1\n\
            \t\tP: \"FrontAxis\", \"int\", \"Integer\", \"\",2\n\
            \t\tP: \"FrontAxisSign\", \"int\", \"Integer\", \"\",1\n\
            \t\tP: \"CoordAxis\", \"int\", \"Integer\", \"\",0\n\
            \t\tP: \"CoordAxisSign\", \"int\", \"Integer\", \"\",1\n\
            \t\tP: \"OriginalUpAxis\", \"int\", \"Integer\", \"\",1\n\
            \t\tP: \"OriginalUpAxisSign\", \"int\", \"Integer\", \"\",1\n\
            \t\tP: \"UnitScaleFactor\", \"double\", \"Number\", \"\",1\n\
            \t\tP: \"OriginalUnitScaleFactor\", \"double\", \"Number\", \"\",1\n\
            \t\tP: \"AmbientColor\", \"ColorRGB\", \"Color\", \"\",0,0,0\n\
            \t\tP: \"DefaultCamera\", \"KString\", \"\", \"\", \"Producer Perspective\"\n\
            \t\tP: \"TimeMode\", \"enum\", \"\", \"\",6\n\
            \t\tP: \"TimeSpanStart\", \"KTime\", \"Time\", \"\",0\n\
            \t\tP: \"TimeSpanStop\", \"KTime\", \"Time\", \"\",0\n\
            \t\tP: \"CustomFrameRate\", \"double\", \"Number\", \"\",-1\n\
        \t}\n\
    }\n\n"
        .to_string()
}

fn documents_block(asset_name: &str) -> String {
    let safe = sanitize_fbx_name(asset_name);
    format!(
        "Documents:  {{\n\
            \tCount: 1\n\
            \tDocument: 1000001, \"Scene::{safe}\", \"Scene\" {{\n\
                \t\tProperties70:  {{\n\
                    \t\t\tP: \"SourceObject\", \"object\", \"\", \"\"\n\
                    \t\t\tP: \"ActiveAnimStackName\", \"KString\", \"\", \"\", \"\"\n\
                \t\t}}\n\
                \t\tRootNode: 0\n\
            \t}}\n\
        }}\n\n",
        safe = safe
    )
}

fn definitions_block(
    geom_count: usize,
    model_count: usize,
    mat_count: usize,
    tex_count: usize,
    vid_count: usize,
    deformer_count: usize,
    pose_count: usize,
    stack_count: usize,
    layer_count: usize,
    curve_node_count: usize,
    curve_count: usize,
) -> String {
    let mut total = 1usize;
    for c in [
        geom_count,
        model_count,
        mat_count,
        tex_count,
        vid_count,
        deformer_count,
        pose_count,
        stack_count,
        layer_count,
        curve_node_count,
        curve_count,
    ] {
        if c > 0 {
            total += 1;
        }
    }

    let mut s = String::new();
    s.push_str("Definitions:  {\n");
    s.push_str("\tVersion: 100\n");
    let _ = write!(s, "\tCount: {}\n", total);
    s.push_str("\tObjectType: \"GlobalSettings\" {\n\t\tCount: 1\n\t}\n");
    let mut decl = |typ: &str, count: usize| {
        if count > 0 {
            let _ = write!(
                s,
                "\tObjectType: \"{}\" {{\n\t\tCount: {}\n\t}}\n",
                typ, count
            );
        }
    };
    decl("Geometry", geom_count);
    decl("Model", model_count);
    decl("Material", mat_count);
    decl("Texture", tex_count);
    decl("Video", vid_count);
    decl("Deformer", deformer_count);
    decl("Pose", pose_count);
    decl("AnimationStack", stack_count);
    decl("AnimationLayer", layer_count);
    decl("AnimationCurveNode", curve_node_count);
    decl("AnimationCurve", curve_count);
    s.push_str("}\n\n");
    s
}

fn emit_geometry(
    id: u64,
    name: &str,
    positions: &[f32],
    uvs: &[f32],
    indices: &[u32],
) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "\tGeometry: {},\"Geometry::{}\", \"Mesh\" {{\n",
        id, name
    );

    let vertex_count = positions.len() / 3;
    let _ = write!(s, "\t\tVertices: *{} {{\n\t\t\ta: ", positions.len());
    for (i, v) in positions.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{}", format_f32(*v));
    }
    s.push_str("\n\t\t}\n");

    let _ = write!(
        s,
        "\t\tPolygonVertexIndex: *{} {{\n\t\t\ta: ",
        indices.len()
    );
    for tri in indices.chunks_exact(3) {
        let a = tri[0] as i32;
        let b = tri[1] as i32;
        let c = -(tri[2] as i32) - 1;
        let _ = write!(s, "{},{},{},", a, b, c);
    }
    if s.ends_with(',') {
        s.pop();
    }
    s.push_str("\n\t\t}\n");
    s.push_str("\t\tGeometryVersion: 124\n");

    let has_uvs = uvs.len() == vertex_count * 2;
    if has_uvs {
        let pv_count = indices.len();
        s.push_str("\t\tLayerElementUV: 0 {\n");
        s.push_str("\t\t\tVersion: 101\n");
        s.push_str("\t\t\tName: \"UVMap\"\n");
        s.push_str("\t\t\tMappingInformationType: \"ByPolygonVertex\"\n");
        s.push_str("\t\t\tReferenceInformationType: \"IndexToDirect\"\n");
        let _ = write!(s, "\t\t\tUV: *{} {{\n\t\t\t\ta: ", uvs.len());
        for (i, v) in uvs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let val = if i % 2 == 1 { 1.0 - *v } else { *v };
            let _ = write!(s, "{}", format_f32(val));
        }
        s.push_str("\n\t\t\t}\n");
        let _ = write!(s, "\t\t\tUVIndex: *{} {{\n\t\t\t\ta: ", pv_count);
        for (i, idx) in indices.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "{}", idx);
        }
        s.push_str("\n\t\t\t}\n");
        s.push_str("\t\t}\n");
    }

    s.push_str("\t\tLayerElementMaterial: 0 {\n");
    s.push_str("\t\t\tVersion: 101\n");
    s.push_str("\t\t\tName: \"\"\n");
    s.push_str("\t\t\tMappingInformationType: \"AllSame\"\n");
    s.push_str("\t\t\tReferenceInformationType: \"IndexToDirect\"\n");
    s.push_str("\t\t\tMaterials: *1 {\n\t\t\t\ta: 0\n\t\t\t}\n");
    s.push_str("\t\t}\n");

    s.push_str("\t\tLayer: 0 {\n");
    s.push_str("\t\t\tVersion: 100\n");
    if has_uvs {
        s.push_str("\t\t\tLayerElement:  {\n");
        s.push_str("\t\t\t\tType: \"LayerElementUV\"\n");
        s.push_str("\t\t\t\tTypedIndex: 0\n");
        s.push_str("\t\t\t}\n");
    }
    s.push_str("\t\t\tLayerElement:  {\n");
    s.push_str("\t\t\t\tType: \"LayerElementMaterial\"\n");
    s.push_str("\t\t\t\tTypedIndex: 0\n");
    s.push_str("\t\t\t}\n");
    s.push_str("\t\t}\n");

    s.push_str("\t}\n");
    s
}

fn emit_mesh_model(id: u64, name: &str) -> String {
    format!(
        "\tModel: {id},\"Model::{name}\", \"Mesh\" {{\n\
            \t\tVersion: 232\n\
            \t\tProperties70:  {{\n\
                \t\t\tP: \"RotationActive\", \"bool\", \"\", \"\",1\n\
                \t\t\tP: \"InheritType\", \"enum\", \"\", \"\",1\n\
                \t\t\tP: \"ScalingMax\", \"Vector3D\", \"Vector\", \"\",0,0,0\n\
                \t\t\tP: \"DefaultAttributeIndex\", \"int\", \"Integer\", \"\",0\n\
            \t\t}}\n\
            \t\tShading: T\n\
            \t\tCulling: \"CullingOff\"\n\
        \t}}\n",
        id = id,
        name = name
    )
}

fn emit_material(id: u64, name: &str, has_albedo: bool) -> String {
    let diffuse = if has_albedo { "1,1,1" } else { "0.8,0.8,0.8" };
    format!(
        "\tMaterial: {id},\"Material::{name}\", \"\" {{\n\
            \t\tVersion: 102\n\
            \t\tShadingModel: \"phong\"\n\
            \t\tMultiLayer: 0\n\
            \t\tProperties70:  {{\n\
                \t\t\tP: \"AmbientColor\", \"Color\", \"\", \"A\",0,0,0\n\
                \t\t\tP: \"DiffuseColor\", \"Color\", \"\", \"A\",{diffuse}\n\
                \t\t\tP: \"DiffuseFactor\", \"Number\", \"\", \"A\",1\n\
                \t\t\tP: \"SpecularColor\", \"Color\", \"\", \"A\",0,0,0\n\
                \t\t\tP: \"SpecularFactor\", \"Number\", \"\", \"A\",0\n\
                \t\t\tP: \"ShininessExponent\", \"Number\", \"\", \"A\",2\n\
                \t\t\tP: \"Emissive\", \"Vector3D\", \"Vector\", \"\",0,0,0\n\
                \t\t\tP: \"Ambient\", \"Vector3D\", \"Vector\", \"\",0,0,0\n\
                \t\t\tP: \"Diffuse\", \"Vector3D\", \"Vector\", \"\",{diffuse}\n\
                \t\t\tP: \"Specular\", \"Vector3D\", \"Vector\", \"\",0,0,0\n\
                \t\t\tP: \"Shininess\", \"double\", \"Number\", \"\",2\n\
                \t\t\tP: \"Opacity\", \"double\", \"Number\", \"\",1\n\
                \t\t\tP: \"Reflectivity\", \"double\", \"Number\", \"\",0\n\
            \t\t}}\n\
        \t}}\n",
        id = id,
        name = name,
        diffuse = diffuse
    )
}

fn emit_texture(id: u64, tex_png_id: u32) -> String {
    let filename = format!("tex_{}.png", tex_png_id);
    format!(
        "\tTexture: {id},\"Texture::tex_{png}\", \"\" {{\n\
            \t\tType: \"TextureVideoClip\"\n\
            \t\tVersion: 202\n\
            \t\tTextureName: \"Texture::tex_{png}\"\n\
            \t\tProperties70:  {{\n\
                \t\t\tP: \"UVSet\", \"KString\", \"\", \"\", \"UVMap\"\n\
                \t\t\tP: \"UseMaterial\", \"bool\", \"\", \"\",1\n\
            \t\t}}\n\
            \t\tMedia: \"Video::tex_{png}\"\n\
            \t\tFileName: \"{filename}\"\n\
            \t\tRelativeFilename: \"{filename}\"\n\
            \t\tModelUVTranslation: 0,0\n\
            \t\tModelUVScaling: 1,1\n\
            \t\tTexture_Alpha_Source: \"None\"\n\
            \t\tCropping: 0,0,0,0\n\
        \t}}\n",
        id = id,
        png = tex_png_id,
        filename = filename
    )
}

fn emit_video(id: u64, tex_png_id: u32, png_bytes: &[u8]) -> String {
    let filename = format!("tex_{}.png", tex_png_id);
    let mut s = String::new();
    let _ = write!(
        s,
        "\tVideo: {id},\"Video::tex_{png}\", \"Clip\" {{\n\
            \t\tType: \"Clip\"\n\
            \t\tProperties70:  {{\n\
                \t\t\tP: \"Path\", \"KString\", \"XRefUrl\", \"\", \"{filename}\"\n\
            \t\t}}\n\
            \t\tUseMipMap: 0\n\
            \t\tFilename: \"{filename}\"\n\
            \t\tRelativeFilename: \"{filename}\"\n",
        id = id,
        png = tex_png_id,
        filename = filename
    );
    let _ = write!(s, "\t\tContent: *{} {{\n\t\t\ta: ", png_bytes.len());
    for (i, byte) in png_bytes.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{}", byte);
    }
    s.push_str("\n\t\t}\n\t}\n");
    s
}

fn format_f32(v: f32) -> String {
    if v.is_finite() {
        if v == v.trunc() && v.abs() < 1e15 {
            format!("{}", v as i64)
        } else {
            format!("{:.6}", v)
        }
    } else {
        "0".to_string()
    }
}

pub fn write_static_level_fbx(
    assets: &[crate::level_glb::LevelGlbAsset],
    instances: &[crate::level_glb::LevelGlbInstance],
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut b = FbxBuilder::new("level");
    let mut tex_id_by_png: HashMap<u32, u64> = HashMap::new();
    let mut mat_id_by_tex: HashMap<Option<u32>, u64> = HashMap::new();

    let default_mat_id = b.next_id();
    b.objects_materials
        .push(emit_material(default_mat_id, "default", false));
    mat_id_by_tex.insert(None, default_mat_id);

    let mut asset_to_geoms: Vec<Vec<(u64, u64)>> = Vec::with_capacity(assets.len());

    for (ai, asset) in assets.iter().enumerate() {
        let mut geoms_for_asset: Vec<(u64, u64)> = Vec::new();
        for (si, sub) in asset.submeshes.iter().enumerate() {
            if sub.positions.is_empty() || sub.indices.is_empty() {
                continue;
            }
            if sub.positions.len() % 3 != 0 || sub.indices.len() % 3 != 0 {
                continue;
            }
            let mat_id = if let Some(id) = mat_id_by_tex.get(&sub.albedo_id) {
                *id
            } else if let Some(tex_id) = sub.albedo_id {
                let new_mat_id = b.next_id();
                let tex_node_id = if let Some(id) = tex_id_by_png.get(&tex_id) {
                    Some(*id)
                } else if let Some(png) = textures.get(&tex_id) {
                    let video_id = b.next_id();
                    b.objects_videos.push(emit_video(video_id, tex_id, png));
                    let texture_id = b.next_id();
                    b.objects_textures.push(emit_texture(texture_id, tex_id));
                    b.connections
                        .push(format!("\tC: \"OO\", {},{}", video_id, texture_id));
                    tex_id_by_png.insert(tex_id, texture_id);
                    Some(texture_id)
                } else {
                    None
                };
                b.objects_materials.push(emit_material(
                    new_mat_id,
                    &format!("mat_{}", tex_id),
                    tex_node_id.is_some(),
                ));
                if let Some(tex_node) = tex_node_id {
                    b.connections.push(format!(
                        "\tC: \"OP\", {},{}, \"DiffuseColor\"",
                        tex_node, new_mat_id
                    ));
                }
                mat_id_by_tex.insert(sub.albedo_id, new_mat_id);
                new_mat_id
            } else {
                default_mat_id
            };

            let geom_id = b.next_id();
            let name = format!("a{}_s{}", ai, si);
            b.objects_geometries.push(emit_geometry(
                geom_id,
                &name,
                &sub.positions,
                &sub.uvs,
                &sub.indices,
            ));
            geoms_for_asset.push((geom_id, mat_id));
        }
        asset_to_geoms.push(geoms_for_asset);
    }

    for (idx, inst) in instances.iter().enumerate() {
        let geoms = match asset_to_geoms.get(inst.asset_idx) {
            Some(g) if !g.is_empty() => g,
            _ => continue,
        };
        let parent_id = b.next_id();
        b.objects_models
            .push(emit_transform_model(parent_id, &inst.name, inst));
        b.connections
            .push(format!("\tC: \"OO\", {},{}", parent_id, ROOT_NODE_ID));

        for (sub_i, (geom_id, mat_id)) in geoms.iter().enumerate() {
            let model_id = b.next_id();
            let model_name = format!("{}_sub{}", inst.name, sub_i);
            b.objects_models
                .push(emit_mesh_model(model_id, &model_name));
            b.connections
                .push(format!("\tC: \"OO\", {},{}", model_id, parent_id));
            b.connections
                .push(format!("\tC: \"OO\", {},{}", geom_id, model_id));
            b.connections
                .push(format!("\tC: \"OO\", {},{}", mat_id, model_id));
        }
        let _ = idx;
    }

    if b.objects_models.is_empty() {
        return Err(Error::GltfWrite(
            "level FBX export had no placeable assets".into(),
        ));
    }

    Ok(b.finish().into_bytes())
}

pub fn write_animated_level_fbx(
    static_assets: &[crate::level_glb::LevelGlbAsset],
    static_instances: &[crate::level_glb::LevelGlbInstance],
    skinned_placements: &[crate::level_glb::SkinnedPlacement],
    shaders: &HashMap<u64, ShaderInfo>,
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut b = FbxBuilder::new("level");
    if !static_assets.is_empty() && !static_instances.is_empty() {
        append_static_level_to_fbx(&mut b, static_assets, static_instances, textures)?;
    }
    for placement in skinned_placements {
        let inst = crate::level_glb::LevelGlbInstance {
            asset_idx: 0,
            name: placement.name.clone(),
            translation: placement.translation,
            rotation: placement.rotation,
            scale: placement.scale,
        };
        append_moby_to_fbx(
            &mut b,
            &placement.asset,
            &placement.clips,
            shaders,
            textures,
            Some(&inst),
        )?;
    }
    if b.objects_models.is_empty()
        && b.objects_limb_models.is_empty()
        && b.objects_geometries.is_empty()
    {
        return Err(Error::GltfWrite(
            "animated level FBX export had no content".into(),
        ));
    }
    Ok(b.finish().into_bytes())
}

fn append_static_level_to_fbx(
    b: &mut FbxBuilder,
    assets: &[crate::level_glb::LevelGlbAsset],
    instances: &[crate::level_glb::LevelGlbInstance],
    textures: &HashMap<u32, Vec<u8>>,
) -> Result<()> {
    let mut tex_id_by_png: HashMap<u32, u64> = HashMap::new();
    let mut mat_id_by_tex: HashMap<Option<u32>, u64> = HashMap::new();

    let default_mat_id = b.next_id();
    b.objects_materials
        .push(emit_material(default_mat_id, "level_default", false));
    mat_id_by_tex.insert(None, default_mat_id);

    let mut asset_to_geoms: Vec<Vec<(u64, u64)>> = Vec::with_capacity(assets.len());

    for (ai, asset) in assets.iter().enumerate() {
        let mut geoms_for_asset: Vec<(u64, u64)> = Vec::new();
        for (si, sub) in asset.submeshes.iter().enumerate() {
            if sub.positions.is_empty() || sub.indices.is_empty() {
                continue;
            }
            if sub.positions.len() % 3 != 0 || sub.indices.len() % 3 != 0 {
                continue;
            }
            let mat_id = if let Some(id) = mat_id_by_tex.get(&sub.albedo_id) {
                *id
            } else if let Some(tex_id) = sub.albedo_id {
                let new_mat_id = b.next_id();
                let tex_node_id = if let Some(id) = tex_id_by_png.get(&tex_id) {
                    Some(*id)
                } else if let Some(png) = textures.get(&tex_id) {
                    let video_id = b.next_id();
                    b.objects_videos.push(emit_video(video_id, tex_id, png));
                    let texture_id = b.next_id();
                    b.objects_textures.push(emit_texture(texture_id, tex_id));
                    b.connections
                        .push(format!("\tC: \"OO\", {},{}", video_id, texture_id));
                    tex_id_by_png.insert(tex_id, texture_id);
                    Some(texture_id)
                } else {
                    None
                };
                b.objects_materials.push(emit_material(
                    new_mat_id,
                    &format!("mat_{}", tex_id),
                    tex_node_id.is_some(),
                ));
                if let Some(tex_node) = tex_node_id {
                    b.connections.push(format!(
                        "\tC: \"OP\", {},{}, \"DiffuseColor\"",
                        tex_node, new_mat_id
                    ));
                }
                mat_id_by_tex.insert(sub.albedo_id, new_mat_id);
                new_mat_id
            } else {
                default_mat_id
            };
            let geom_id = b.next_id();
            let name = format!("a{}_s{}", ai, si);
            b.objects_geometries.push(emit_geometry(
                geom_id,
                &name,
                &sub.positions,
                &sub.uvs,
                &sub.indices,
            ));
            geoms_for_asset.push((geom_id, mat_id));
        }
        asset_to_geoms.push(geoms_for_asset);
    }

    for inst in instances {
        let geoms = match asset_to_geoms.get(inst.asset_idx) {
            Some(g) if !g.is_empty() => g,
            _ => continue,
        };
        let parent_id = b.next_id();
        b.objects_models
            .push(emit_transform_model(parent_id, &inst.name, inst));
        b.connections
            .push(format!("\tC: \"OO\", {},{}", parent_id, ROOT_NODE_ID));
        for (sub_i, (geom_id, mat_id)) in geoms.iter().enumerate() {
            let model_id = b.next_id();
            let model_name = format!("{}_sub{}", inst.name, sub_i);
            b.objects_models
                .push(emit_mesh_model(model_id, &model_name));
            b.connections
                .push(format!("\tC: \"OO\", {},{}", model_id, parent_id));
            b.connections
                .push(format!("\tC: \"OO\", {},{}", geom_id, model_id));
            b.connections
                .push(format!("\tC: \"OO\", {},{}", mat_id, model_id));
        }
    }
    Ok(())
}

fn emit_transform_model(
    id: u64,
    name: &str,
    inst: &crate::level_glb::LevelGlbInstance,
) -> String {
    let (ex, ey, ez) = quat_to_euler_xyz_degrees(inst.rotation);
    format!(
        "\tModel: {id},\"Model::{name}\", \"Null\" {{\n\
            \t\tVersion: 232\n\
            \t\tProperties70:  {{\n\
                \t\t\tP: \"Lcl Translation\", \"Lcl Translation\", \"\", \"A\",{tx},{ty},{tz}\n\
                \t\t\tP: \"Lcl Rotation\", \"Lcl Rotation\", \"\", \"A\",{rx},{ry},{rz}\n\
                \t\t\tP: \"Lcl Scaling\", \"Lcl Scaling\", \"\", \"A\",{sx},{sy},{sz}\n\
                \t\t\tP: \"DefaultAttributeIndex\", \"int\", \"Integer\", \"\",0\n\
                \t\t\tP: \"InheritType\", \"enum\", \"\", \"\",1\n\
            \t\t}}\n\
            \t\tShading: Y\n\
            \t\tCulling: \"CullingOff\"\n\
        \t}}\n",
        id = id,
        name = name,
        tx = format_f32(inst.translation[0]),
        ty = format_f32(inst.translation[1]),
        tz = format_f32(inst.translation[2]),
        rx = format_f32(ex),
        ry = format_f32(ey),
        rz = format_f32(ez),
        sx = format_f32(inst.scale[0]),
        sy = format_f32(inst.scale[1]),
        sz = format_f32(inst.scale[2]),
    )
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

fn format_f64(v: f64) -> String {
    if v.is_finite() {
        if v == v.trunc() && v.abs() < 1e15 {
            format!("{}", v as i64)
        } else {
            format!("{:.9}", v)
        }
    } else {
        "0".to_string()
    }
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

fn mat4_identity() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ]
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
            world[i] = skel
                .bind_local
                .get(i)
                .copied()
                .unwrap_or_else(mat4_identity);
        }
    }
    world
}

fn emit_skeleton_and_skin_with_parent(
    b: &mut FbxBuilder,
    asset_name: &str,
    skel: &Skeleton,
    bangles: &[crate::moby::MobyBangle],
    mesh_targets: &[(u64, u64, u64, usize, usize)],
    out_bone_model_ids: &mut Vec<u64>,
    parent_node_id: u64,
) {
    let bone_count = skel.bones.len();
    let bind_world = compute_bind_world_matrices(skel);

    let mut bone_model_ids: Vec<u64> = Vec::with_capacity(bone_count);
    for i in 0..bone_count {
        let id = b.next_id();
        bone_model_ids.push(id);
        let local = skel
            .bind_local
            .get(i)
            .copied()
            .unwrap_or_else(mat4_identity);
        let (translation, scale, quat) = decompose_col_major(&local);
        let (rx, ry, rz) = quat_to_euler_xyz_degrees(quat);
        let name = format!("{}_bone_{:03}", asset_name, i);
        b.objects_limb_models.push(emit_limb_node_model(
            id,
            &name,
            translation,
            [rx, ry, rz],
            scale,
        ));
    }
    *out_bone_model_ids = bone_model_ids.clone();

    for i in 0..bone_count {
        let parent = skel.bones[i].parent_index;
        let parent_target = if parent < 0
            || (parent as usize) == i
            || (parent as usize) >= bone_count
        {
            parent_node_id
        } else {
            bone_model_ids[parent as usize]
        };
        b.connections.push(format!(
            "\tC: \"OO\", {},{}",
            bone_model_ids[i], parent_target
        ));
    }

    let mut pose_entries: Vec<(u64, [f32; 16])> = Vec::with_capacity(bone_count + 1);
    pose_entries.push((parent_node_id, mat4_identity()));
    for i in 0..bone_count {
        pose_entries.push((bone_model_ids[i], bind_world[i]));
    }
    let pose_id = b.next_id();
    b.objects_poses
        .push(emit_pose(pose_id, &format!("{}_BindPose", asset_name), &pose_entries));

    let mut deformers_per_bone: HashMap<u64, Vec<(u32, f32)>> = HashMap::new();
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
        deformers_per_bone.clear();
        for v in 0..vertex_count {
            let base = v * 4;
            for slot in 0..4 {
                let bone = mesh.bone_indices[base + slot] as u32;
                let weight = mesh.bone_weights[base + slot];
                if weight == 0 {
                    continue;
                }
                if (bone as usize) >= bone_count {
                    continue;
                }
                let bone_model_id = bone_model_ids[bone as usize];
                deformers_per_bone
                    .entry(bone_model_id)
                    .or_default()
                    .push((v as u32, weight as f32 / 255.0));
            }
        }
        if deformers_per_bone.is_empty() {
            continue;
        }
        let deformer_id = b.next_id();
        b.objects_deformers
            .push(emit_skin_deformer(deformer_id, asset_name));
        b.connections
            .push(format!("\tC: \"OO\", {},{}", deformer_id, geom_id));

        let mut sorted_bones: Vec<u64> = deformers_per_bone.keys().copied().collect();
        sorted_bones.sort_unstable();
        for bone_model_id in sorted_bones {
            let entries = &deformers_per_bone[&bone_model_id];
            let bone_index = bone_model_ids
                .iter()
                .position(|&id| id == bone_model_id)
                .unwrap_or(0);
            let transform = skel
                .bind_world_inverse
                .get(bone_index)
                .copied()
                .unwrap_or_else(mat4_identity);
            let transform_link = bind_world[bone_index];
            let sub_id = b.next_id();
            b.objects_sub_deformers.push(emit_sub_deformer(
                sub_id,
                &format!("{}_cluster_{}", asset_name, bone_index),
                entries,
                &transform,
                &transform_link,
            ));
            b.connections
                .push(format!("\tC: \"OO\", {},{}", sub_id, deformer_id));
            b.connections
                .push(format!("\tC: \"OO\", {},{}", bone_model_id, sub_id));
        }
    }
}

fn emit_limb_node_model(
    id: u64,
    name: &str,
    translation: [f32; 3],
    rotation_deg: [f32; 3],
    scale: [f32; 3],
) -> String {
    format!(
        "\tModel: {id},\"Model::{name}\", \"LimbNode\" {{\n\
            \t\tVersion: 232\n\
            \t\tProperties70:  {{\n\
                \t\t\tP: \"InheritType\", \"enum\", \"\", \"\",1\n\
                \t\t\tP: \"DefaultAttributeIndex\", \"int\", \"Integer\", \"\",0\n\
                \t\t\tP: \"Lcl Translation\", \"Lcl Translation\", \"\", \"A\",{tx},{ty},{tz}\n\
                \t\t\tP: \"Lcl Rotation\", \"Lcl Rotation\", \"\", \"A\",{rx},{ry},{rz}\n\
                \t\t\tP: \"Lcl Scaling\", \"Lcl Scaling\", \"\", \"A\",{sx},{sy},{sz}\n\
            \t\t}}\n\
            \t\tShading: Y\n\
            \t\tCulling: \"CullingOff\"\n\
        \t}}\n",
        id = id,
        name = name,
        tx = format_f32(translation[0]),
        ty = format_f32(translation[1]),
        tz = format_f32(translation[2]),
        rx = format_f32(rotation_deg[0]),
        ry = format_f32(rotation_deg[1]),
        rz = format_f32(rotation_deg[2]),
        sx = format_f32(scale[0]),
        sy = format_f32(scale[1]),
        sz = format_f32(scale[2]),
    )
}

fn emit_skin_deformer(id: u64, asset_name: &str) -> String {
    format!(
        "\tDeformer: {id},\"Deformer::{asset_name}_Skin\", \"Skin\" {{\n\
            \t\tVersion: 101\n\
            \t\tLink_DeformAcuracy: 50\n\
        \t}}\n"
    )
}

fn emit_sub_deformer(
    id: u64,
    name: &str,
    entries: &[(u32, f32)],
    transform: &[f32; 16],
    transform_link: &[f32; 16],
) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "\tDeformer: {id},\"SubDeformer::{name}\", \"Cluster\" {{\n\
            \t\tVersion: 100\n\
            \t\tUserData: \"\", \"\"\n",
    );
    let _ = write!(s, "\t\tIndexes: *{} {{\n\t\t\ta: ", entries.len());
    for (i, (v, _w)) in entries.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{}", v);
    }
    s.push_str("\n\t\t}\n");
    let _ = write!(s, "\t\tWeights: *{} {{\n\t\t\ta: ", entries.len());
    for (i, (_v, w)) in entries.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{}", format_f64(*w as f64));
    }
    s.push_str("\n\t\t}\n");
    s.push_str("\t\tTransform: *16 {\n\t\t\ta: ");
    for (i, v) in transform.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{}", format_f64(*v as f64));
    }
    s.push_str("\n\t\t}\n");
    s.push_str("\t\tTransformLink: *16 {\n\t\t\ta: ");
    for (i, v) in transform_link.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{}", format_f64(*v as f64));
    }
    s.push_str("\n\t\t}\n");
    s.push_str("\t}\n");
    s
}

fn emit_pose(id: u64, name: &str, entries: &[(u64, [f32; 16])]) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "\tPose: {id},\"Pose::{name}\", \"BindPose\" {{\n\
            \t\tType: \"BindPose\"\n\
            \t\tVersion: 100\n\
            \t\tNbPoseNodes: {}\n",
        entries.len()
    );
    for (node_id, matrix) in entries {
        let _ = write!(
            s,
            "\t\tPoseNode:  {{\n\t\t\tNode: {}\n\t\t\tMatrix: *16 {{\n\t\t\t\ta: ",
            node_id
        );
        for (i, v) in matrix.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "{}", format_f64(*v as f64));
        }
        s.push_str("\n\t\t\t}\n\t\t}\n");
    }
    s.push_str("\t}\n");
    s
}

fn emit_animation_clips(
    b: &mut FbxBuilder,
    asset_name: &str,
    clips: &[DecodedClip],
    bone_model_ids: &[u64],
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
        let dt_seconds = 1.0 / fps as f64;
        let mut times_ktime: Vec<i64> = Vec::with_capacity(frame_count);
        for f in 0..frame_count {
            let t = f as f64 * dt_seconds;
            times_ktime.push((t * KTIME_PER_SECOND as f64) as i64);
        }
        let stop_ktime = *times_ktime.last().unwrap_or(&0);

        let stack_id = b.next_id();
        let clip_name = if clip.name.is_empty() {
            format!("{}_clip_{}", asset_name, clip_idx)
        } else {
            sanitize_fbx_name(&clip.name)
        };
        b.objects_anim_stacks
            .push(emit_anim_stack(stack_id, &clip_name, stop_ktime));

        let layer_id = b.next_id();
        b.objects_anim_layers
            .push(emit_anim_layer(layer_id, &clip_name));
        b.connections
            .push(format!("\tC: \"OO\", {},{}", layer_id, stack_id));

        let bone_limit = clip.bones.len().min(bone_model_ids.len());
        for bone_i in 0..bone_limit {
            let bone = &clip.bones[bone_i];
            let bone_model_id = bone_model_ids[bone_i];
            if !bone.translations.is_empty() {
                emit_one_channel(
                    b,
                    layer_id,
                    bone_model_id,
                    "Lcl Translation",
                    &times_ktime,
                    &bone.translations,
                    3,
                    false,
                );
            }
            if !bone.scales.is_empty() {
                emit_one_channel(
                    b,
                    layer_id,
                    bone_model_id,
                    "Lcl Scaling",
                    &times_ktime,
                    &bone.scales,
                    3,
                    false,
                );
            }
            if !bone.rotations.is_empty() {
                let euler = quaternions_to_euler_track(&bone.rotations);
                emit_one_channel(
                    b,
                    layer_id,
                    bone_model_id,
                    "Lcl Rotation",
                    &times_ktime,
                    &euler,
                    3,
                    true,
                );
            }
        }
    }
}

fn emit_one_channel(
    b: &mut FbxBuilder,
    layer_id: u64,
    bone_model_id: u64,
    property_name: &str,
    times_ktime: &[i64],
    values: &[f32],
    components: usize,
    _is_rotation: bool,
) {
    if values.is_empty() || times_ktime.is_empty() {
        return;
    }
    let frame_count = times_ktime.len();
    let value_len = values.len();
    let default_xyz = if value_len >= components {
        [
            values[0] as f64,
            values[1] as f64,
            values[2] as f64,
        ]
    } else {
        [0.0; 3]
    };
    let curve_node_id = b.next_id();
    b.objects_anim_curve_nodes
        .push(emit_anim_curve_node(curve_node_id, property_name, default_xyz));
    b.connections
        .push(format!("\tC: \"OO\", {},{}", curve_node_id, layer_id));
    b.connections.push(format!(
        "\tC: \"OP\", {},{}, \"{}\"",
        curve_node_id, bone_model_id, property_name
    ));

    let axes = ["X", "Y", "Z"];
    for axis_i in 0..components {
        let mut comp_values: Vec<f32> = Vec::with_capacity(frame_count);
        for f in 0..frame_count {
            let idx = f * components + axis_i;
            if idx < value_len {
                comp_values.push(values[idx]);
            } else if !comp_values.is_empty() {
                comp_values.push(*comp_values.last().unwrap());
            } else {
                comp_values.push(0.0);
            }
        }
        let curve_id = b.next_id();
        b.objects_anim_curves
            .push(emit_anim_curve(curve_id, times_ktime, &comp_values));
        b.connections.push(format!(
            "\tC: \"OP\", {},{}, \"d|{}\"",
            curve_id, curve_node_id, axes[axis_i]
        ));
    }
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

fn emit_anim_stack(id: u64, name: &str, stop_ktime: i64) -> String {
    format!(
        "\tAnimationStack: {id},\"AnimStack::{name}\", \"\" {{\n\
            \t\tProperties70:  {{\n\
                \t\t\tP: \"LocalStart\", \"KTime\", \"Time\", \"\",0\n\
                \t\t\tP: \"LocalStop\", \"KTime\", \"Time\", \"\",{stop}\n\
                \t\t\tP: \"ReferenceStart\", \"KTime\", \"Time\", \"\",0\n\
                \t\t\tP: \"ReferenceStop\", \"KTime\", \"Time\", \"\",{stop}\n\
            \t\t}}\n\
        \t}}\n",
        id = id,
        name = name,
        stop = stop_ktime
    )
}

fn emit_anim_layer(id: u64, name: &str) -> String {
    format!(
        "\tAnimationLayer: {id},\"AnimLayer::{name}\", \"\" {{\n\t}}\n",
        id = id,
        name = name
    )
}

fn emit_anim_curve_node(id: u64, property_name: &str, default_xyz: [f64; 3]) -> String {
    format!(
        "\tAnimationCurveNode: {id},\"AnimCurveNode::{prop}\", \"\" {{\n\
            \t\tProperties70:  {{\n\
                \t\t\tP: \"d|X\", \"Number\", \"\", \"A\",{x}\n\
                \t\t\tP: \"d|Y\", \"Number\", \"\", \"A\",{y}\n\
                \t\t\tP: \"d|Z\", \"Number\", \"\", \"A\",{z}\n\
            \t\t}}\n\
        \t}}\n",
        id = id,
        prop = property_name,
        x = format_f64(default_xyz[0]),
        y = format_f64(default_xyz[1]),
        z = format_f64(default_xyz[2]),
    )
}

fn emit_anim_curve(id: u64, times_ktime: &[i64], values: &[f32]) -> String {
    let mut s = String::new();
    let default = values.first().copied().unwrap_or(0.0);
    let _ = write!(
        s,
        "\tAnimationCurve: {id},\"AnimCurve::\", \"\" {{\n\
            \t\tDefault: {default}\n\
            \t\tKeyVer: 4008\n",
        id = id,
        default = format_f64(default as f64)
    );
    let _ = write!(s, "\t\tKeyTime: *{} {{\n\t\t\ta: ", times_ktime.len());
    for (i, t) in times_ktime.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{}", t);
    }
    s.push_str("\n\t\t}\n");
    let _ = write!(s, "\t\tKeyValueFloat: *{} {{\n\t\t\ta: ", values.len());
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{}", format_f32(*v));
    }
    s.push_str("\n\t\t}\n");
    let _ = write!(
        s,
        "\t\tKeyAttrFlags: *1 {{\n\t\t\ta: 24840\n\t\t}}\n\
            \t\tKeyAttrDataFloat: *4 {{\n\t\t\ta: 0,0,218434821,0\n\t\t}}\n\
            \t\tKeyAttrRefCount: *1 {{\n\t\t\ta: {}\n\t\t}}\n\
        \t}}\n",
        values.len()
    );
    s
}

fn sanitize_fbx_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::LEADING_COMMENT_BLOCK;

    #[test]
    fn header_clears_threejs_ascii_check_traps() {
        let correct: [char; 20] = [
            'K', 'a', 'y', 'd', 'a', 'r', 'a', '\\', 'F', 'B', 'X', '\\', 'B', 'i', 'n', 'a',
            'r', 'y', '\\', '\\',
        ];
        let trap_positions = [
            0usize, 1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 66, 78, 91, 105, 120, 136, 153, 171, 190,
        ];
        let header = format!(
            "{}FBXHeaderExtension:  {{\n\tFBXHeaderVersion: 1003\n\tFBXVersion: 7400\n",
            LEADING_COMMENT_BLOCK
        );
        let bytes: Vec<char> = header.chars().collect();
        for (i, &pos) in trap_positions.iter().enumerate() {
            let ch = bytes.get(pos).copied().unwrap_or('?');
            assert_ne!(
                ch, correct[i],
                "trap position {} matches CORRECT[{}]={:?} — three.js FBXLoader will reject the file",
                pos, i, correct[i]
            );
        }
    }
}
