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
    /// RFOM (and possibly other Insomniac titles) ship a root-level
    /// `game.psarc` that has to be extracted before any level becomes
    /// playable. V2 USRDIRs (R2/R3/ACiT/A4O) have nothing at root → list
    /// is empty.
    pub root_psarcs: Vec<String>,
    /// True when at least one level folder under `packed/levels/<level>/`
    /// has a `built/` subdirectory. Used to detect "RFOM pre-extract
    /// needed" — a USRDIR can have level folders populated with dialogue
    /// streams but still be unplayable until `game.psarc` is unpacked,
    /// at which point `built/` materializes.
    pub any_level_built: bool,
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

    // List any `.psarc` at the USRDIR root — RFOM ships `game.psarc`
    // here that has to be extracted before `packed/levels/` exists.
    let root_psarcs: Vec<String> = std::fs::read_dir(root)
        .ok()
        .map(|it| {
            it.flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter_map(|e| {
                    let p = e.path();
                    let ext_match = p
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case("psarc"))
                        .unwrap_or(false);
                    if ext_match {
                        e.file_name().to_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let (level_folder_count, any_level_built) = if packed_levels.is_dir() {
        std::fs::read_dir(&packed_levels)
            .map(|it| {
                let mut count = 0usize;
                let mut any_built = false;
                for e in it.flatten() {
                    if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    count += 1;
                    if !any_built && level_dir_is_extracted(&e.path()) {
                        any_built = true;
                    }
                }
                (count, any_built)
            })
            .unwrap_or((0, false))
    } else {
        (0, false)
    };

    // Accept the folder as a USRDIR in three cases:
    //  - V2 layout already present (packed/game/ + packed/levels/)
    //  - A root-level PSARC is waiting to be unpacked (RFOM fresh disc)
    //  - Levels are already extracted by an external tool — packed/levels/
    //    has folders with `built/` subdirectories (RFOM pre-extracted case).
    let is_usrdir = (packed_game.is_dir() && packed_levels.is_dir())
        || !root_psarcs.is_empty()
        || (packed_levels.is_dir() && any_level_built);

    R2SetupStatus {
        is_usrdir,
        global_cached: global_psarc_state(&packed_game, "global_cached"),
        global_uncached: global_psarc_state(&packed_game, "global_uncached"),
        level_folder_count,
        root_psarcs,
        any_level_built,
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

/// `entry_file` is the per-game ready marker (defaults to V2's
/// `assetlookup.dat`). RFOM passes `ps3levelmain.dat`. Used both for
/// the ready flag in the maps list and for resolving the open path.
#[tauri::command]
pub fn r2_list_maps(
    usrdir: String,
    entry_file: Option<String>,
) -> Result<Vec<R2MapInfo>, String> {
    let entry_file = entry_file.unwrap_or_else(|| "assetlookup.dat".to_string());
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
        let ready = resolve_level_data_dir(&folder, &name, &entry_file).is_some();
        let psarc_present = std::fs::read_dir(&folder)
            .ok()
            .map(|it| {
                it.flatten().any(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case("psarc"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
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
        natural_cmp(&a.id, &b.id).then_with(|| a.display_name.cmp(&b.display_name))
    });
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
enum NaturalChunk<'a> {
    Text(&'a str),
    Number(u64),
}

impl<'a> NaturalChunk<'a> {
    fn cmp_chunk(&self, other: &NaturalChunk<'a>) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (NaturalChunk::Number(a), NaturalChunk::Number(b)) => a.cmp(b),
            (NaturalChunk::Text(a), NaturalChunk::Text(b)) => {
                a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
            }
            (NaturalChunk::Number(_), NaturalChunk::Text(_)) => Ordering::Less,
            (NaturalChunk::Text(_), NaturalChunk::Number(_)) => Ordering::Greater,
        }
    }
}

fn split_natural(s: &str) -> Vec<NaturalChunk<'_>> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let is_digit = bytes[i].is_ascii_digit();
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() == is_digit {
            i += 1;
        }
        let slice = &s[start..i];
        if is_digit {
            if let Ok(n) = slice.parse::<u64>() {
                out.push(NaturalChunk::Number(n));
            } else {
                out.push(NaturalChunk::Text(slice));
            }
        } else {
            out.push(NaturalChunk::Text(slice));
        }
    }
    out
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let ca = split_natural(a);
    let cb = split_natural(b);
    for (x, y) in ca.iter().zip(cb.iter()) {
        let o = x.cmp_chunk(y);
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
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

/// Pre-extract step for games (RFOM) that ship a single root-level
/// `game.psarc` in the USRDIR. Extracts every `.psarc` sitting at the
/// USRDIR root into the USRDIR itself — the archive's internal paths
/// (`packed/game/...`, `packed/levels/<level>/...`) reconstitute the
/// folder tree the rest of the wizard expects. After this completes
/// the frontend re-runs `r2_setup_check` and proceeds with the normal
/// globals → maps → level flow.
#[tauri::command]
pub fn r2_extract_root_psarcs(
    usrdir: String,
    on_event: Channel<R2ExtractEvent>,
) -> Result<(), String> {
    let root = Path::new(&usrdir);
    if !root.is_dir() {
        let msg = format!("usrdir not a directory: {}", root.display());
        let _ = on_event.send(R2ExtractEvent::Error {
            message: msg.clone(),
        });
        return Err(msg);
    }
    let mut psarcs: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| format!("read_dir {}: {e}", root.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("psarc"))
                    .unwrap_or(false)
        })
        .collect();
    psarcs.sort();

    if psarcs.is_empty() {
        let msg = format!("no root-level .psarc files in {}", root.display());
        let _ = on_event.send(R2ExtractEvent::Error {
            message: msg.clone(),
        });
        return Err(msg);
    }

    for psarc in &psarcs {
        let label = psarc
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("psarc")
            .to_string();
        if let Some(diag) = sniff_psarc_magic(psarc) {
            let _ = on_event.send(R2ExtractEvent::Warning {
                message: format!("{label}: {diag}"),
            });
            continue;
        }
        if let Err(e) = extract_one_psarc(psarc, root, None, &label, &on_event) {
            let _ = on_event.send(R2ExtractEvent::Warning {
                message: format!("{label}: {e}"),
            });
        }
    }
    let _ = on_event.send(R2ExtractEvent::Done);
    Ok(())
}

fn sniff_psarc_magic(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 4];
    if f.read_exact(&mut buf).is_err() {
        return Some(format!("file is shorter than 4 bytes: {}", path.display()));
    }
    if &buf == b"PSAR" {
        return None;
    }
    let hex = buf
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!(
        "first 4 bytes are [{hex}], not 'PSAR' (50 53 41 52). \
         File appears to still be encrypted at the PS3 disc-key level. \
         Boot the game at least once in RPCS3 first — that pass decrypts \
         disc files into a PSARC-readable state — then re-point the \
         wizard at the same USRDIR."
    ))
}

#[tauri::command]
pub fn r2_extract_level(
    usrdir: String,
    map_id: String,
    entry_file: Option<String>,
    on_event: Channel<R2ExtractEvent>,
) -> Result<(), String> {
    let entry_file = entry_file.unwrap_or_else(|| "assetlookup.dat".to_string());
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
    let already_extracted =
        resolve_level_data_dir(&level_dir, &map_id, &entry_file).is_some();

    let mut psarcs: Vec<PathBuf> = std::fs::read_dir(&level_dir)
        .map_err(|e| format!("read_dir {}: {e}", level_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("psarc"))
                    .unwrap_or(false)
        })
        .collect();
    psarcs.sort();

    if psarcs.is_empty() {
        let msg = format!("no .psarc files in {}", level_dir.display());
        let _ = on_event.send(R2ExtractEvent::Error {
            message: msg.clone(),
        });
        return Err(msg);
    }

    for psarc in &psarcs {
        let label = psarc
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("psarc")
            .to_string();
        if already_extracted {
            let _ = on_event.send(R2ExtractEvent::PsarcDone {
                psarc: label,
                skipped: true,
            });
            continue;
        }
        if let Err(e) = extract_one_psarc(psarc, &level_dir, None, &label, &on_event) {
            let _ = on_event.send(R2ExtractEvent::Warning {
                message: format!("{label}: {e}"),
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
    decode_image_file_to_png_bytes(&path)
        .map_err(|e| format!("decode {}: {e}", path.display()))
}

fn decode_image_file_to_png_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(buf)
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
        decode_image_file_to_png_bytes(&path)
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
pub fn r2_level_open_path(
    usrdir: String,
    map_id: String,
    entry_file: Option<String>,
) -> Result<String, String> {
    let entry_file = entry_file.unwrap_or_else(|| "assetlookup.dat".to_string());
    let level_dir = Path::new(&usrdir)
        .join("packed")
        .join("levels")
        .join(&map_id);
    match resolve_level_data_dir(&level_dir, &map_id, &entry_file) {
        Some(dir) => Ok(dir.to_string_lossy().into_owned()),
        None => Err(format!(
            "level not extracted yet — no {} found under {} (checked V2 path {}/built/levels/{} and direct path {})",
            entry_file,
            level_dir.display(),
            level_dir.display(),
            map_id,
            level_dir.display()
        )),
    }
}

fn level_dir_is_extracted(level_dir: &Path) -> bool {
    if level_dir.join("built").is_dir() {
        return true;
    }
    if level_dir.join("ps3levelmain.dat").is_file() {
        return true;
    }
    false
}

fn resolve_level_data_dir(level_dir: &Path, map_id: &str, entry_file: &str) -> Option<PathBuf> {
    let v2 = level_dir.join("built").join("levels").join(map_id);
    if v2.join(entry_file).is_file() {
        return Some(v2);
    }
    if level_dir.join(entry_file).is_file() {
        return Some(level_dir.to_path_buf());
    }
    None
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
