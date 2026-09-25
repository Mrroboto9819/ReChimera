use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use lunalib::{
    extract_bank_sounds_for_file, extract_stream_sounds, list_sounds as list_sounds_in, IgFile,
    SoundKind,
};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct SoundEntryDto {
    name: String,

    index: usize,

    kind: &'static str,

    source: String,
}


#[tauri::command]
pub(crate) fn list_level_sounds(level_folder: String) -> Result<Vec<SoundEntryDto>, String> {
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
        // Prefix matches (not exact names) so R3's language-infixed
        // `resident_sound.us.dat` banks are recognized alongside the
        // R2/RFOM-era exact `resident_sound.dat`.
        let is_bank = lower.starts_with("resident_sound")
            || lower.starts_with("ps3sound")
            || lower.starts_with("resident_dialogue")
            || lower.starts_with("ps3dialogue");
        if is_bank {
            candidates.push(name);
        }
    }
    candidates.sort();

    // R3 keeps no banks in the level folder at all — they live in the
    // extracted `packed/game/global_sound_<lang>/built/sound/bank/<hash>/`
    // trees. Emit those with ABSOLUTE paths as `source`: every extract
    // command resolves sources via `folder.join(source)`, and joining an
    // absolute path yields it unchanged, so the same commands work for
    // both per-level and global banks.
    for bank_dir in lunalib::find_global_sound_bank_dirs(folder) {
        let Ok(rd) = std::fs::read_dir(&bank_dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(".dat") || lower.contains("stream") {
                continue;
            }
            if lower.starts_with("resident_sound") || lower.starts_with("resident_dialogue") {
                candidates.push(bank_dir.join(&name).to_string_lossy().into_owned());
            }
        }
    }

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
pub(crate) struct ExtractedSoundDto {
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
        let is_bank = lower.starts_with("resident_sound")
            || lower.starts_with("ps3sound")
            || lower.starts_with("resident_dialogue")
            || (lower.starts_with("ps3dialogue") && !lower.starts_with("ps3dialoguestream"));
        if is_bank {
            out.push(n);
        }
    }
    out.sort();
    // R3: banks live under packed/game/global_sound_<lang>/built/sound/
    // bank/<hash>/, not in the level folder. Absolute-path entries resolve
    // unchanged through every `folder.join(source)` call site.
    for bank_dir in lunalib::find_global_sound_bank_dirs(folder) {
        let Ok(rd) = std::fs::read_dir(&bank_dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let n = entry.file_name().to_string_lossy().into_owned();
            let lower = n.to_ascii_lowercase();
            if !lower.ends_with(".dat") || lower.contains("stream") {
                continue;
            }
            if lower.starts_with("resident_sound") || lower.starts_with("resident_dialogue") {
                out.push(bank_dir.join(&n).to_string_lossy().into_owned());
            }
        }
    }
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
pub(crate) fn extract_one_stream_sound(
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
pub(crate) fn extract_one_sound(
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
pub(crate) fn bulk_extract_sounds_zip(
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
pub(crate) fn extract_level_sounds(level_folder: String) -> Result<Vec<ExtractedSoundDto>, String> {
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
pub(crate) fn dump_sound_bank(
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
pub(crate) fn extract_level_stream_sounds(
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
pub(crate) fn extract_raw_streaming_sounds(
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

