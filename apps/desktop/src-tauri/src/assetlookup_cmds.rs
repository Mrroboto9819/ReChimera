use std::path::Path;

use serde::Serialize;
use tauri::ipc::Channel;

#[derive(Serialize)]
struct AssetLookupKindDto {
    name: String,
    section_id: u32,
    count: usize,
    has_decoder: bool,
}

#[derive(Serialize)]
pub(crate) struct AssetLookupOverviewDto {
    layout: String,
    version_major: u16,
    version_minor: u16,
    kinds: Vec<AssetLookupKindDto>,
}

#[tauri::command]
pub(crate) fn asset_lookup_inspect(path: String) -> Result<AssetLookupOverviewDto, String> {
    let overview = lunalib::inspect_assetlookup(Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(AssetLookupOverviewDto {
        layout: overview.layout.to_string(),
        version_major: overview.version_major,
        version_minor: overview.version_minor,
        kinds: overview
            .kinds
            .into_iter()
            .map(|k| AssetLookupKindDto {
                name: k.name.to_string(),
                section_id: k.section_id,
                count: k.count,
                has_decoder: k.has_decoder,
            })
            .collect(),
    })
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AssetLookupExtractEvent {
    Total { kind: String, count: usize },
    Entry {
        kind: String,
        index: usize,
        tuid: String,
        ok: bool,
        message: Option<String>,
    },
    KindDone { kind: String },
    Done,
    Error { message: String },
}

#[tauri::command]
pub(crate) fn asset_lookup_extract_stream(
    input: String,
    output: String,
    kinds: Vec<String>,
    max_texture_dim: Option<u32>,
    on_event: Channel<AssetLookupExtractEvent>,
) -> Result<(), String> {
    if let Err(message) =
        run_asset_lookup_extract(&input, &output, &kinds, max_texture_dim, &on_event)
    {
        let _ = on_event.send(AssetLookupExtractEvent::Error {
            message: message.clone(),
        });
        return Err(message);
    }
    let _ = on_event.send(AssetLookupExtractEvent::Done);
    Ok(())
}

fn run_asset_lookup_extract(
    input: &str,
    output: &str,
    kind_names: &[String],
    max_texture_dim: Option<u32>,
    on_event: &Channel<AssetLookupExtractEvent>,
) -> Result<(), String> {
    let input_path = Path::new(input);
    let output_path = Path::new(output);

    let kinds = parse_asset_kinds(kind_names)?;
    if kinds.is_empty() {
        return Err("no asset kinds selected".to_string());
    }

    std::fs::create_dir_all(output_path).map_err(|e| format!("create out dir: {e}"))?;

    let options = lunalib::ExtractOptions {
        kinds,
        max_texture_dim: max_texture_dim.unwrap_or(4096),
    };

    let on_event_cl = on_event.clone();
    lunalib::extract_assetlookup(input_path, output_path, &options, move |ev| match ev {
        lunalib::ExtractEvent::Total { kind, count } => {
            let _ = on_event_cl.send(AssetLookupExtractEvent::Total {
                kind: kind.name().to_string(),
                count,
            });
        }
        lunalib::ExtractEvent::Entry { kind, index, tuid, ok, message } => {
            let _ = on_event_cl.send(AssetLookupExtractEvent::Entry {
                kind: kind.name().to_string(),
                index,
                tuid: format!("0x{:016X}", tuid),
                ok,
                message,
            });
        }
        lunalib::ExtractEvent::KindDone { kind } => {
            let _ = on_event_cl.send(AssetLookupExtractEvent::KindDone {
                kind: kind.name().to_string(),
            });
        }
    })
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn parse_asset_kinds(names: &[String]) -> Result<Vec<lunalib::AssetKind>, String> {
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let kind = match name.as_str() {
            "shader" => lunalib::AssetKind::Shader,
            "texture" => lunalib::AssetKind::Texture,
            "highmip" => lunalib::AssetKind::HighMip,
            "cubemap" => lunalib::AssetKind::Cubemap,
            "tie" => lunalib::AssetKind::Tie,
            "foliage" => lunalib::AssetKind::Foliage,
            "shrub" => lunalib::AssetKind::Shrub,
            "moby" => lunalib::AssetKind::Moby,
            "animset" => lunalib::AssetKind::Animset,
            "cinematic" => lunalib::AssetKind::Cinematic,
            "zone" => lunalib::AssetKind::Zone,
            "lighting" => lunalib::AssetKind::Lighting,
            other => return Err(format!("unknown asset kind: {other}")),
        };
        out.push(kind);
    }
    Ok(out)
}
