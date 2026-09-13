use std::path::Path;

use serde::Serialize;

use std::collections::HashMap;
use crate::find_gltf_library_dir;

#[derive(Serialize)]
pub(crate) struct GltfFileDto {

    name: String,

    path: String,

    extension: String,
    size_bytes: u64,

    category: String,
}

#[derive(Serialize)]
pub(crate) struct GltfLibraryDto {

    folder: String,
    files: Vec<GltfFileDto>,
}


#[tauri::command]
pub(crate) fn list_character_gltfs(folder: String) -> Result<GltfLibraryDto, String> {
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
pub(crate) fn list_entities_gltfs(folder: String) -> Result<GltfLibraryDto, String> {
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



#[derive(Serialize)]
pub(crate) struct GlbMaterialTexturesDto {

    material_name: String,

    albedo_path: Option<String>,

    normal_path: Option<String>,

    emissive_path: Option<String>,
}


#[tauri::command]
pub(crate) fn find_glb_textures(
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



#[tauri::command]
pub(crate) fn list_gltfs_in_folder(path: String) -> Result<GltfLibraryDto, String> {
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


