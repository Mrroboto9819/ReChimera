use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::ipc::Channel;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PsarcState {
    Ready,
    NotExtracted,
    Missing,
}

#[derive(Serialize, Clone, Debug)]
pub struct R2SetupStatus {
    pub is_usrdir: bool,
    pub global_cached: PsarcState,
    pub global_uncached: PsarcState,
    pub level_folder_count: usize,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum R2MapCategory {
    Campaign,
    Multiplayer,
    Coop,
    Lobby,
    Other,
}

#[derive(Serialize, Clone, Debug)]
pub struct R2MapInfo {
    pub id: String,
    pub display_name: String,
    pub category: R2MapCategory,
    pub ready: bool,
    pub psarc_present: bool,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum R2ExtractEvent {
    PsarcStart { psarc: String, total: usize },
    PsarcProgress { psarc: String, current: usize, name: String },
    PsarcDone { psarc: String, skipped: bool },
    Done,
    Warning { message: String },
    Error { message: String },
}

#[tauri::command]
pub fn r2_setup_check(usrdir: String) -> R2SetupStatus {
    let root = Path::new(&usrdir);
    let packed_game = root.join("packed").join("game");
    let packed_levels = root.join("packed").join("levels");
    let is_usrdir = packed_game.is_dir() && packed_levels.is_dir();
    let level_folder_count = if is_usrdir {
        std::fs::read_dir(&packed_levels)
            .map(|it| {
                it.filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .count()
            })
            .unwrap_or(0)
    } else {
        0
    };
    R2SetupStatus {
        is_usrdir,
        global_cached: global_psarc_state(&packed_game, "global_cached"),
        global_uncached: global_psarc_state(&packed_game, "global_uncached"),
        level_folder_count,
    }
}

fn global_psarc_state(packed_game: &Path, variant: &str) -> PsarcState {
    let psarc = packed_game.join(format!("{variant}.psarc"));
    let extracted_marker = packed_game.join(variant).join("built").join("tuids");
    let has_psarc = psarc.is_file();
    let is_extracted = extracted_marker.is_dir()
        && std::fs::read_dir(&extracted_marker)
            .map(|mut it| it.next().is_some())
            .unwrap_or(false);
    match (has_psarc, is_extracted) {
        (_, true) => PsarcState::Ready,
        (true, false) => PsarcState::NotExtracted,
        (false, false) => PsarcState::Missing,
    }
}

#[tauri::command]
pub fn r2_list_maps(usrdir: String) -> Result<Vec<R2MapInfo>, String> {
    let levels = Path::new(&usrdir).join("packed").join("levels");
    let it = std::fs::read_dir(&levels).map_err(|e| format!("read packed/levels/: {e}"))?;
    let mut out = Vec::new();
    for entry in it.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let folder = entry.path();
        let category = classify_map(&name);
        let level_lookup = folder
            .join("built")
            .join("levels")
            .join(&name)
            .join("assetlookup.dat");
        let ready = level_lookup.is_file();
        let psarc_present = folder.join("level_cached.psarc").is_file()
            || folder.join("level_uncached.psarc").is_file();
        out.push(R2MapInfo {
            display_name: humanize_map_name(&name),
            id: name,
            category,
            ready,
            psarc_present,
        });
    }
    out.sort_by(|a, b| {
        let by_cat = category_order(&a.category).cmp(&category_order(&b.category));
        if by_cat != std::cmp::Ordering::Equal {
            return by_cat;
        }
        a.display_name.cmp(&b.display_name)
    });
    Ok(out)
}

fn classify_map(name: &str) -> R2MapCategory {
    let lower = name.to_ascii_lowercase();
    if lower == "lobby" {
        R2MapCategory::Lobby
    } else if lower.ends_with("_coop") || lower.contains("_coop_") {
        R2MapCategory::Coop
    } else if lower.ends_with("_multiplayer") || lower.contains("_multiplayer_") {
        R2MapCategory::Multiplayer
    } else if lower.contains("debug") || lower.contains("test") {
        R2MapCategory::Other
    } else {
        R2MapCategory::Campaign
    }
}

fn category_order(c: &R2MapCategory) -> u8 {
    match c {
        R2MapCategory::Campaign => 0,
        R2MapCategory::Coop => 1,
        R2MapCategory::Multiplayer => 2,
        R2MapCategory::Lobby => 3,
        R2MapCategory::Other => 4,
    }
}

fn humanize_map_name(id: &str) -> String {
    id.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[tauri::command]
pub fn r2_extract_globals(
    usrdir: String,
    on_event: Channel<R2ExtractEvent>,
) -> Result<(), String> {
    let packed_game = Path::new(&usrdir).join("packed").join("game");
    for variant in ["global_cached", "global_uncached"] {
        let psarc = packed_game.join(format!("{variant}.psarc"));
        let out_dir = packed_game.join(variant);
        let marker = out_dir.join("built").join("tuids");
        if let Err(e) = extract_one_psarc(&psarc, &out_dir, Some(&marker), variant, &on_event)
        {
            let _ = on_event.send(R2ExtractEvent::Warning {
                message: format!("{variant}: {e}"),
            });
        }
    }
    let _ = on_event.send(R2ExtractEvent::Done);
    Ok(())
}

#[tauri::command]
pub fn r2_extract_level(
    usrdir: String,
    map_id: String,
    on_event: Channel<R2ExtractEvent>,
) -> Result<(), String> {
    let level_dir = Path::new(&usrdir)
        .join("packed")
        .join("levels")
        .join(&map_id);
    if !level_dir.is_dir() {
        let msg = format!("level dir not found: {}", level_dir.display());
        let _ = on_event.send(R2ExtractEvent::Error {
            message: msg.clone(),
        });
        return Err(msg);
    }
    let canonical_marker = level_dir
        .join("built")
        .join("levels")
        .join(&map_id)
        .join("assetlookup.dat");
    let already_extracted = canonical_marker.is_file();

    for variant in ["level_cached", "level_uncached"] {
        let psarc = level_dir.join(format!("{variant}.psarc"));
        if already_extracted {
            let _ = on_event.send(R2ExtractEvent::PsarcDone {
                psarc: variant.to_string(),
                skipped: true,
            });
            continue;
        }
        if let Err(e) = extract_one_psarc(&psarc, &level_dir, None, variant, &on_event)
        {
            let _ = on_event.send(R2ExtractEvent::Warning {
                message: format!("{variant}: {e}"),
            });
        }
    }
    let _ = on_event.send(R2ExtractEvent::Done);
    Ok(())
}

/// One sprite candidate that the user can assign as a level
/// thumbnail. We only expose the filename (already inside the
/// `global_cached/scaleform/` directory) — the frontend reads pixels
/// via `r2_read_scaleform_image`.
#[derive(Serialize, Clone, Debug)]
pub struct R2CardSprite {
    pub filename: String,
    pub short_label: String,
}

/// List card-aspect sprite filenames available for a given map kind.
/// R2 ships each menu screen's sprite atlas as plain `_i<hex>.tga`
/// files next to the parent SWF. The `kind` determines which atlas
/// to surface:
///   - `"coop"`  → `mainmenu_i*.tga` (4 coop campaign chapter cards)
///   - `"mp"`    → `competitivestaging_i*.tga` (3 MP map cards)
///   - `"coop2"` → `coopstaging_i*.tga` (coop session/staging cards, 2)
///
/// The sprite → level mapping isn't stored anywhere on disk — names
/// are sprite indices, not chapter names. The frontend picker lets
/// the user assign each sprite to one of their extracted levels and
/// persists the choice in localStorage. Robust to mods that
/// rearrange sprite indices and to other Insomniac titles using the
/// same wadpack flow.
#[tauri::command]
pub fn r2_list_card_sprites(
    usrdir: String,
    kind: String,
) -> Result<Vec<R2CardSprite>, String> {
    let prefix = match kind.as_str() {
        "coop" => "mainmenu_i",
        "mp" => "competitivestaging_i",
        "coop2" => "coopstaging_i",
        other => return Err(format!("unknown kind: {other}")),
    };
    let scaleform = Path::new(&usrdir)
        .join("packed")
        .join("game")
        .join("global_cached")
        .join("scaleform");
    let mut out = Vec::new();
    if !scaleform.is_dir() {
        return Ok(out);
    }
    let it = std::fs::read_dir(&scaleform).map_err(|e| e.to_string())?;
    for entry in it.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(prefix) {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".tga")
            || lower.ends_with(".dds")
            || lower.ends_with(".png"))
        {
            continue;
        }
        let stem = name
            .strip_prefix(prefix)
            .and_then(|s| s.split('.').next())
            .unwrap_or("")
            .to_string();
        out.push(R2CardSprite {
            filename: name,
            short_label: format!("i{}", stem),
        });
    }
    out.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(out)
}

#[tauri::command]
pub fn r2_read_scaleform_image(
    usrdir: String,
    file_name: String,
) -> Result<Vec<u8>, String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(format!("rejected file_name: {file_name}"));
    }
    let path = Path::new(&usrdir)
        .join("packed")
        .join("game")
        .join("global_cached")
        .join("scaleform")
        .join(&file_name);
    if !path.is_file() {
        return Err(format!("not found: {}", path.display()));
    }
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        return std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()));
    }
    lunalib::decode_image_file_to_png(&path)
        .map_err(|e| format!("decode {}: {e}", path.display()))
}

/// User-supplied PNG (from an RSX-saved texture, an RPCS3 screenshot
/// crop, or any external source) imported as a map-card thumbnail.
/// Copies the file under
/// `<usrdir>/_rechimera_wizard_thumbs/<sanitized>.png` and returns the
/// safe filename the frontend should store in localStorage. The
/// frontend renders it via `r2_read_imported_thumbnail` so the same
/// reader path that handles scaleform images can read these too.
#[tauri::command]
pub fn r2_import_thumbnail(
    usrdir: String,
    source_path: String,
    label: String,
) -> Result<String, String> {
    let src = Path::new(&source_path);
    if !src.is_file() {
        return Err(format!("source not a file: {}", src.display()));
    }
    let stem = label
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            _ => '_',
        })
        .take(60)
        .collect::<String>();
    let stem = if stem.is_empty() { "thumb".to_string() } else { stem };
    let out_dir = Path::new(&usrdir).join("_rechimera_wizard_thumbs");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {e}"))?;
    let mut out_name = format!("{stem}.png");
    let mut suffix = 1u32;
    while out_dir.join(&out_name).is_file() {
        out_name = format!("{stem}_{suffix}.png");
        suffix += 1;
    }
    let dest = out_dir.join(&out_name);
    let bytes = std::fs::read(src).map_err(|e| format!("read source: {e}"))?;
    // Reject obviously-wrong inputs — PNG starts 89 50 4E 47, JPEG
    // starts FF D8 FF. We accept JPEG too but transcode in the
    // frontend if needed. For now, just write whatever the user gave
    // us and let the renderer decode.
    std::fs::write(&dest, &bytes).map_err(|e| format!("write {}: {e}", dest.display()))?;
    Ok(out_name)
}

/// Read back an imported thumbnail from the wizard cache directory.
/// Mirrors r2_read_scaleform_image but for user-supplied files.
#[tauri::command]
pub fn r2_read_imported_thumbnail(
    usrdir: String,
    file_name: String,
) -> Result<Vec<u8>, String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(format!("rejected file_name: {file_name}"));
    }
    let path = Path::new(&usrdir).join("_rechimera_wizard_thumbs").join(&file_name);
    if !path.is_file() {
        return Err(format!("not found: {}", path.display()));
    }
    std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))
}

/// Decode a scaleform image to RGBA, crop to a rectangle, return PNG bytes.
/// Lets the frontend treat any large atlas (`campaignload_id.dds`,
/// `levelselect_id.tga`, etc.) as a grid of virtual thumbnails — the
/// caller passes a `(x, y, w, h)` rect and gets just that slice as a
/// PNG. Clamps the rect to the image bounds; returns an error if the
/// resulting region is empty.
#[tauri::command]
pub fn r2_read_scaleform_image_crop(
    usrdir: String,
    file_name: String,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Result<Vec<u8>, String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(format!("rejected file_name: {file_name}"));
    }
    let path = Path::new(&usrdir)
        .join("packed")
        .join("game")
        .join("global_cached")
        .join("scaleform")
        .join(&file_name);
    if !path.is_file() {
        return Err(format!("not found: {}", path.display()));
    }
    let png_full = if file_name.to_ascii_lowercase().ends_with(".png") {
        std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?
    } else {
        lunalib::decode_image_file_to_png(&path)
            .map_err(|e| format!("decode {}: {e}", path.display()))?
    };
    // Re-decode the PNG so we have raw RGBA to crop. Could be smarter
    // by cropping the source TGA/DDS directly, but going through PNG
    // keeps one decode path and the cost is negligible for the small
    // atlases we're dealing with (~2 MB max).
    let img = image::load_from_memory(&png_full)
        .map_err(|e| format!("decode png {}: {e}", path.display()))?;
    let (iw, ih) = (img.width(), img.height());
    let x = x.min(iw.saturating_sub(1));
    let y = y.min(ih.saturating_sub(1));
    let w = w.min(iw.saturating_sub(x));
    let h = h.min(ih.saturating_sub(y));
    if w == 0 || h == 0 {
        return Err(format!(
            "crop rect (x={x} y={y} w={w} h={h}) is empty for {}x{} image {}",
            iw, ih, file_name
        ));
    }
    let cropped = image::imageops::crop_imm(&img, x, y, w, h).to_image();
    let mut out: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgba8(cropped)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| format!("encode png: {e}"))?;
    Ok(out)
}

#[derive(Serialize, Clone, Debug)]
pub struct R2ThumbnailProbe {
    pub matches: std::collections::HashMap<String, Vec<String>>,
    pub top_level_dirs: Vec<String>,
    pub image_extensions_seen: Vec<String>,
    pub scanned_file_count: usize,
    pub truncated: bool,
}

#[tauri::command]
pub fn r2_probe_level_thumbnails(
    usrdir: String,
    map_ids: Vec<String>,
) -> Result<R2ThumbnailProbe, String> {
    const SCAN_CAP: usize = 50_000;
    const IMAGE_EXTS: &[&str] = &[
        "tga", "dds", "png", "tex", "swf", "bmp", "jpg", "jpeg", "webp",
    ];

    let packed_game = Path::new(&usrdir).join("packed").join("game");
    let mut matches: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for id in &map_ids {
        matches.insert(id.clone(), Vec::new());
    }
    let mut top_level_dirs: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut exts_seen: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut scanned = 0usize;
    let mut truncated = false;

    let lowered_ids: Vec<(String, String)> = map_ids
        .iter()
        .map(|id| (id.clone(), id.to_ascii_lowercase()))
        .collect();

    for variant in ["global_cached", "global_uncached"] {
        let root = packed_game.join(variant);
        if !root.is_dir() {
            continue;
        }
        if let Ok(it) = std::fs::read_dir(&root) {
            for entry in it.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        top_level_dirs.insert(format!("{variant}/{name}"));
                    }
                }
            }
        }
        walk_for_thumbnails(
            &root,
            &lowered_ids,
            IMAGE_EXTS,
            &mut matches,
            &mut exts_seen,
            &mut scanned,
            &mut truncated,
            SCAN_CAP,
        );
        if truncated {
            break;
        }
    }

    Ok(R2ThumbnailProbe {
        matches,
        top_level_dirs: top_level_dirs.into_iter().collect(),
        image_extensions_seen: exts_seen.into_iter().collect(),
        scanned_file_count: scanned,
        truncated,
    })
}

fn walk_for_thumbnails(
    dir: &Path,
    map_ids: &[(String, String)],
    image_exts: &[&str],
    matches: &mut std::collections::HashMap<String, Vec<String>>,
    exts_seen: &mut std::collections::BTreeSet<String>,
    scanned: &mut usize,
    truncated: &mut bool,
    cap: usize,
) {
    if *truncated {
        return;
    }
    let it = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in it.flatten() {
        if *scanned >= cap {
            *truncated = true;
            return;
        }
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            walk_for_thumbnails(
                &path, map_ids, image_exts, matches, exts_seen, scanned, truncated, cap,
            );
            if *truncated {
                return;
            }
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        *scanned += 1;
        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
        else {
            continue;
        };
        if !image_exts.iter().any(|x| *x == ext) {
            continue;
        }
        exts_seen.insert(ext.clone());
        let path_lower = path.to_string_lossy().to_ascii_lowercase();
        for (id, lower) in map_ids {
            if path_lower.contains(lower) {
                let list = matches.entry(id.clone()).or_default();
                if list.len() < 8 {
                    list.push(path.to_string_lossy().into_owned());
                }
            }
        }
    }
}

#[tauri::command]
pub fn r2_cache_needs_rebuild(usrdir: String, map_id: String) -> bool {
    let level = Path::new(&usrdir)
        .join("packed")
        .join("levels")
        .join(&map_id)
        .join("built")
        .join("levels")
        .join(&map_id);
    let manifest = level.join("_rechimera_cache").join("manifest.json");
    let Some(cache_mtime) = mtime_secs(&manifest) else {
        return false;
    };
    let packed_game = Path::new(&usrdir).join("packed").join("game");
    for variant in ["global_cached", "global_uncached"] {
        let tuids = packed_game.join(variant).join("built").join("tuids");
        if let Some(g) = dir_mtime_recursive(&tuids, 2) {
            if g > cache_mtime {
                return true;
            }
        }
    }
    false
}

fn mtime_secs(path: &Path) -> Option<u64> {
    let m = std::fs::metadata(path).ok()?;
    let t = m.modified().ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_secs())
}

fn dir_mtime_recursive(path: &Path, max_depth: u32) -> Option<u64> {
    if !path.is_dir() {
        return None;
    }
    let mut latest = mtime_secs(path).unwrap_or(0);
    if max_depth == 0 {
        return Some(latest);
    }
    let it = std::fs::read_dir(path).ok()?;
    for e in it.flatten() {
        let p = e.path();
        let m = if p.is_dir() {
            dir_mtime_recursive(&p, max_depth - 1).unwrap_or(0)
        } else {
            mtime_secs(&p).unwrap_or(0)
        };
        if m > latest {
            latest = m;
        }
    }
    Some(latest)
}

#[tauri::command]
pub fn r2_level_open_path(usrdir: String, map_id: String) -> Result<String, String> {
    let candidate = Path::new(&usrdir)
        .join("packed")
        .join("levels")
        .join(&map_id)
        .join("built")
        .join("levels")
        .join(&map_id);
    if candidate.join("assetlookup.dat").is_file() {
        Ok(candidate.to_string_lossy().into_owned())
    } else {
        Err(format!(
            "level not extracted yet — no assetlookup.dat at {}",
            candidate.display()
        ))
    }
}

fn extract_one_psarc(
    psarc_path: &Path,
    out_dir: &Path,
    expect_marker: Option<&Path>,
    label: &str,
    on_event: &Channel<R2ExtractEvent>,
) -> Result<(), String> {
    if !psarc_path.is_file() {
        return Err(format!("missing {}", psarc_path.display()));
    }
    if is_already_extracted(out_dir, expect_marker) {
        let _ = on_event.send(R2ExtractEvent::PsarcDone {
            psarc: label.to_string(),
            skipped: true,
        });
        return Ok(());
    }
    std::fs::create_dir_all(out_dir).map_err(|e| format!("mkdir {}: {e}", out_dir.display()))?;

    let mut archive = psarc::Archive::open(psarc_path).map_err(|e| e.to_string())?;
    let entries: Vec<_> = archive.entries.clone();
    let total = entries.len();
    let _ = on_event.send(R2ExtractEvent::PsarcStart {
        psarc: label.to_string(),
        total,
    });

    for (i, entry) in entries.iter().enumerate() {
        let bytes = archive.read_entry(entry).map_err(|e| e.to_string())?;
        let mut rel = entry.name.replace('\\', "/");
        while rel.starts_with('/') {
            rel.remove(0);
        }
        if rel.split('/').any(|seg| seg == "..") {
            return Err(format!("path traversal blocked: {}", entry.name));
        }
        let dest = out_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }
        write_bytes_safe(&dest, &bytes).map_err(|e| format!("write {dest:?}: {e}"))?;

        if i == 0 || (i + 1) == total || (i + 1) % 50 == 0 {
            let _ = on_event.send(R2ExtractEvent::PsarcProgress {
                psarc: label.to_string(),
                current: i + 1,
                name: entry.name.clone(),
            });
        }
    }

    let _ = on_event.send(R2ExtractEvent::PsarcDone {
        psarc: label.to_string(),
        skipped: false,
    });
    Ok(())
}

fn is_already_extracted(_out_dir: &Path, expect_marker: Option<&Path>) -> bool {
    let Some(marker) = expect_marker else {
        return false;
    };
    if marker.is_file() {
        return true;
    }
    if !marker.is_dir() {
        return false;
    }
    std::fs::read_dir(marker)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

fn write_bytes_safe(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if !s.starts_with(r"\\?\") && !s.starts_with(r"\\.\") {
            let abs: PathBuf = if path.is_absolute() {
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
