use std::path::Path;

use crate::write_bytes_to_path;

use serde::Serialize;
use tauri::ipc::Channel;

#[derive(Serialize)]
struct PsarcEntryDto {
    name: String,
    uncompressed_size: u64,
    file_offset: u64,
}

#[derive(Serialize)]
pub(crate) struct PsarcListDto {
    major: u16,
    minor: u16,
    compression: &'static str,
    block_size: u32,
    entry_count: usize,
    entries: Vec<PsarcEntryDto>,
}

#[tauri::command]
pub(crate) fn psarc_list(path: String) -> Result<PsarcListDto, String> {
    let archive = psarc::Archive::open(Path::new(&path)).map_err(|e| e.to_string())?;
    let compression = match archive.header.compression {
        psarc::Compression::Zlib => "zlib",
        psarc::Compression::Lzma => "lzma",
        psarc::Compression::Oodle => "oodle",
    };
    let entries = archive
        .entries
        .iter()
        .map(|e| PsarcEntryDto {
            name: e.name.clone(),
            uncompressed_size: e.uncompressed_size,
            file_offset: e.file_offset,
        })
        .collect();
    Ok(PsarcListDto {
        major: archive.header.major,
        minor: archive.header.minor,
        compression,
        block_size: archive.header.block_size,
        entry_count: archive.entries.len(),
        entries,
    })
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum PsarcEvent {

    Total { total: usize },

    File {
        index: usize,
        name: String,
        bytes: u64,
    },
    Done,
    Error { message: String },
}

#[tauri::command]
pub(crate) fn psarc_extract_stream(
    input: String,
    output: String,
    on_event: Channel<PsarcEvent>,
) -> Result<(), String> {
    if let Err(message) = run_psarc_extract(&input, &output, &on_event) {
        let _ = on_event.send(PsarcEvent::Error { message: message.clone() });
        return Err(message);
    }
    let _ = on_event.send(PsarcEvent::Done);
    Ok(())
}

fn run_psarc_extract(
    input: &str,
    output: &str,
    on_event: &Channel<PsarcEvent>,
) -> Result<(), String> {
    let mut archive = psarc::Archive::open(Path::new(input)).map_err(|e| e.to_string())?;
    let out_root = Path::new(output);
    std::fs::create_dir_all(out_root).map_err(|e| format!("create out dir: {e}"))?;

    let total = archive.entries.len();
    let _ = on_event.send(PsarcEvent::Total { total });


    let entries: Vec<_> = archive.entries.clone();

    for (i, entry) in entries.iter().enumerate() {
        let bytes = archive.read_entry(entry).map_err(|e| e.to_string())?;


        let mut rel = entry.name.replace('\\', "/");
        while rel.starts_with('/') {
            rel.remove(0);
        }
        if rel.split('/').any(|seg| seg == "..") {
            return Err(format!(
                "path traversal attempt blocked for entry: {}",
                entry.name
            ));
        }


        let dest = out_root.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }


        write_bytes_to_path(&dest, &bytes)
            .map_err(|e| format!("write {dest:?}: {e}"))?;

        let _ = on_event.send(PsarcEvent::File {
            index: i + 1,
            name: entry.name.clone(),
            bytes: bytes.len() as u64,
        });
    }
    Ok(())
}
