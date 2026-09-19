use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::fs::File;
use std::path::Path;
use std::process::ExitCode;

use lunalib::{
    animation_section_offsets, decode_animation_with_skel, read_animation_control,
    read_animation_header_at, read_shaders, read_textures, write_moby_glb_full, AssetKind,
    AssetLookup, Game, IgFile, MobyAsset,
};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let folder = args.next().expect("level folder");
    let tuid = args
        .next()
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .expect("moby tuid hex");
    let out = args.next().expect("out.glb path");
    let with_anim = args.next().as_deref() == Some("anim");

    let level = Path::new(&folder);
    let game = Game::R3;
    let profile = game.anim_profile();

    let mut asset: Option<MobyAsset> = None;
    lunalib::read_moby_assets_with_total(level, None, |_| {}, |a| {
        if a.tuid == tuid {
            asset = Some(a.clone());
        }
    })
    .expect("read mobys");
    let asset = asset.expect("moby not found");
    let skel = asset.skeleton.clone().expect("no skeleton");
    println!(
        "moby 0x{:016X} '{}' bones={} bangles={} animset={:?}",
        asset.tuid,
        asset.name,
        skel.bones.len(),
        asset.bangles.len(),
        asset.animset_hash.map(|h| format!("0x{h:016X}"))
    );

    let shaders = read_shaders(level).unwrap_or_default();
    let textures: HashMap<u32, Vec<u8>> = read_textures(level)
        .map(|v| {
            v.into_iter()
                .filter(|t| t.is_decoded())
                .map(|t| (t.id, lunalib::encode_png(&t.rgba, t.width, t.height)))
                .collect()
        })
        .unwrap_or_default();

    let mut clips = Vec::new();
    if with_anim {
        if let Some(hash) = asset.animset_hash {
            let lf = File::open(level.join("assetlookup.dat")).expect("assetlookup");
            let mut lookup = AssetLookup::open(BufReader::new(lf)).expect("parse");
            let ptrs = lookup.pointers(AssetKind::Animset).expect("animset ptrs");
            if let Some(ptr) = ptrs.iter().find(|p| p.tuid == hash) {
                let mut af = File::open(level.join("animsets.dat")).expect("animsets");
                af.seek(SeekFrom::Start(u64::from(ptr.offset))).unwrap();
                let mut buf = vec![0u8; ptr.length as usize];
                af.read_exact(&mut buf).unwrap();
                let mut ig = IgFile::open(Cursor::new(buf)).expect("igfile");
                let mconv = profile.matrix_convention();
                let ps = mconv.shift_scale(skel.translation_shift);
                let ss = mconv.shift_scale(skel.scale_shift);
                let sb = skel.bones.len() as u16;
                for off in animation_section_offsets(&ig) {
                    let Ok(mut h) = read_animation_header_at(&mut ig, off) else { continue };
                    if h.is_additive() && sb > 0 {
                        h.num_bones = sb;
                    }
                    h.apply_frame_stride_padding();
                    let Ok(ctrl) = read_animation_control(&mut ig, &h) else { continue };
                    if let Ok(clip) =
                        decode_animation_with_skel(&mut ig, &h, &ctrl, ps, ss, &skel, profile)
                    {
                        // Match the app cache path (decode_clips_for_moby): NO
                        // compose_additive_with_skeleton for gameheads; clips
                        // are pushed raw.
                        clips.push(clip);
                    }
                }
            }
        }
        println!("decoded {} clips", clips.len());
    } else {
        println!("REST pose only (no animation channels)");
    }

    let glb = write_moby_glb_full(&asset, &clips, &shaders, &textures).expect("write glb");
    fs::write(&out, &glb).expect("write file");
    println!("wrote {} ({} bytes)", out, glb.len());
    ExitCode::SUCCESS
}
