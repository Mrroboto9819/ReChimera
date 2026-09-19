

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use lunalib::{
    animation_section_offsets, decode_animation, decode_animation_with_skel, AnimProfile, Game,
    decode_animation_with_skeleton, detect_layout,
    read_animation_control, read_animation_header_at,
    read_shaders, read_tie_assets_with_total, AssetKind,
    AssetLookup, DecodedClip, IgFile, LevelLayout, ShaderInfo, Skeleton, UFrag, Zone,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;

use crate::{
    build_skeleton_dto, mesh_dto, resolve_shader_textures, AssetMeshesDto, UFragMeshDto,
};

const CACHE_DIR_NAME: &str = "_rechimera_cache";
const MANIFEST_NAME: &str = "manifest.json";

const MANIFEST_VERSION: u32 = 2;
const TEXTURE_MAX_DIM: u32 = 512;

const SOURCE_FILES: &[&str] = &[
    "assetlookup.dat",
    "mobys.dat",
    "ties.dat",
    "shaders.dat",
    "textures.dat",
    "highmips.dat",
    "zones.dat",
    "animsets.dat",
];

#[derive(Serialize, Deserialize, Clone)]
pub struct CacheManifestEntry {

    pub kind: String,

    pub tuid: String,

    pub name: String,

    pub file: String,
    pub size_bytes: u64,
}

#[derive(Serialize, Deserialize)]
pub struct CacheManifest {
    pub version: u32,
    pub folder: String,
    pub entries: Vec<CacheManifestEntry>,

    #[serde(default)]
    pub source_mtimes: HashMap<String, u64>,

    #[serde(default = "default_complete")]
    pub complete: bool,
}

fn default_complete() -> bool {
    true
}

#[derive(Serialize)]
pub struct CacheStatus {
    pub exists: bool,
    pub folder: String,

    pub cache_path: String,
    pub entry_count: usize,

    pub mobys: usize,
    pub ties: usize,
    pub textures: usize,

    pub stale: bool,

    pub incomplete: bool,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CacheEvent {

    Phase {
        phase: &'static str,
        total: usize,
    },

    Item {
        kind: &'static str,
        name: String,
        file: String,
    },

    Progress {
        current: usize,
    },

    Done {
        entry_count: usize,
    },

    Error {
        message: String,
    },
}

pub(crate) fn cache_root(folder: &str) -> PathBuf {
    Path::new(folder).join(CACHE_DIR_NAME)
}

fn mtime_unix_secs(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_secs())
}

fn snapshot_source_mtimes(folder: &Path) -> HashMap<String, u64> {
    let mut out = HashMap::with_capacity(SOURCE_FILES.len());
    for name in SOURCE_FILES {
        if let Some(mt) = mtime_unix_secs(&folder.join(name)) {
            out.insert((*name).to_string(), mt);
        }
    }
    out
}

fn is_cache_stale(folder: &Path, snapshot: &HashMap<String, u64>) -> bool {
    if snapshot.is_empty() {
        return true;
    }
    for (name, &snap) in snapshot {
        let current = match mtime_unix_secs(&folder.join(name)) {
            Some(m) => m,
            None => continue,
        };
        if current > snap {
            return true;
        }
    }
    false
}

pub(crate) struct AnimsetIndex {
    by_hash: HashMap<u64, (u32, u32)>,
}

impl AnimsetIndex {
    pub(crate) fn build(level_folder: &Path) -> Result<Self, String> {
        let path = level_folder.join("assetlookup.dat");
        let file = std::fs::File::open(&path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        let mut lookup = AssetLookup::open(std::io::BufReader::new(file))
            .map_err(|e| e.to_string())?;
        let ptrs = lookup
            .pointers(AssetKind::Animset)
            .map_err(|e| format!("read animset table: {e}"))?;
        let mut by_hash = HashMap::with_capacity(ptrs.len());
        for ptr in ptrs {
            by_hash.insert(ptr.tuid, (ptr.offset, ptr.length));
        }
        Ok(Self { by_hash })
    }
}

// One-shot probe to dump the raw bytes of the first anim's first 3 frames
// for byte-level comparison between layouts. Fires at most once per process
// per layout. Gated on `RECHIMERA_LOG_PROBES=1`.
static TOD_ANIM_BYTES_PROBE_FIRED: AtomicBool = AtomicBool::new(false);
static RFOM_ANIM_BYTES_PROBE_FIRED: AtomicBool = AtomicBool::new(false);

// Returns true if this anim's pair-frame decision should be logged.
// Dedupe key is (moby_tuid, anim_name) so each moby gets to log all of
// its anims even if the names appear on other mobys too. Cap at 250
// rows to keep the audit large enough for a full level but not unbounded.
fn should_log_tod_anim_decision(moby_tuid: u64, name: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SEEN: Mutex<Option<HashSet<(u64, String)>>> = Mutex::new(None);
    let mut guard = SEEN.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    if set.len() >= 250 {
        return false;
    }
    set.insert((moby_tuid, name.to_string()))
}
// Fires once for the first TOD anim that has at least one Position OR Scale
// track (not pure rotation), so we can probe the suspected translation/scale
// decode bug separately from the rotation-only animate_spin case.
static TOD_TRANS_PROBE_FIRED: AtomicBool = AtomicBool::new(false);

fn probe_anim_bytes<R: Read + Seek>(
    ig: &mut IgFile<R>,
    tag: &str,
    anim_off: u64,
) {
    let header = match read_animation_header_at(ig, anim_off) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[anim-bytes] {tag} header read failed: {e}");
            return;
        }
    };
    eprintln!(
        "[anim-bytes] {tag} name='{}' frames={} bones={} flags=0x{:04X} stride={} n16={} n8={} nrv={} frames_ptr=0x{:08X} ctrl_ptr=0x{:08X}",
        header.name,
        header.num_frames,
        header.num_bones,
        header.flags,
        header.frame_stride,
        header.num_16bit_tracks,
        header.num_8bit_tracks,
        header.num_reference_values,
        header.frames_ptr,
        header.control_ptr,
    );

    // Dump the AnimationControl block to recover (bone_index, component,
    // kind) for each track. Same parse our `read_animation_control` does.
    if let Ok(ctrl) = read_animation_control(ig, &header) {
        for (i, m) in ctrl.track16_masks.iter().enumerate() {
            eprintln!(
                "[anim-bytes] {tag} track16[{i}] bone={} comp={} kind={:?}",
                m.bone_index, m.component, m.kind
            );
        }
        for (i, m) in ctrl.track8_masks.iter().enumerate() {
            eprintln!(
                "[anim-bytes] {tag} track8[{i}] bone={} comp={} kind={:?} base={}",
                m.bone_index,
                m.component,
                m.kind,
                ctrl.track8_base_values.get(i).copied().unwrap_or(0)
            );
        }
        for (i, m) in ctrl.ref_pose_masks.iter().enumerate() {
            eprintln!(
                "[anim-bytes] {tag} refpose[{i}] bone={} comp={} kind={:?} val={}",
                m.bone_index,
                m.component,
                m.kind,
                ctrl.ref_pose_values.get(i).copied().unwrap_or(0)
            );
        }
    } else {
        eprintln!("[anim-bytes] {tag} (control block read failed)");
    }

    if header.frames_ptr == 0 || header.frame_stride == 0 || header.num_frames == 0 {
        eprintln!("[anim-bytes] {tag} (no frame data — skipping byte dump)");
        return;
    }
    let stride = header.frame_stride as usize;
    let n_frames_to_dump = header.num_frames.min(16) as usize;
    for f in 0..n_frames_to_dump {
        let off = u64::from(header.frames_ptr) + (f as u64) * (stride as u64);
        if ig.stream.seek_to(off).is_err() {
            eprintln!("[anim-bytes] {tag} frame[{f}] seek failed");
            continue;
        }
        let bytes = match ig.stream.read_bytes(stride) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[anim-bytes] {tag} frame[{f}] read failed: {e}");
                continue;
            }
        };
        // Hex dump
        let hex: String = bytes
            .iter()
            .take(64)
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join("");
        eprintln!(
            "[anim-bytes] {tag} frame[{f}] @ 0x{:08X} hex(64)={}",
            off, hex
        );
        // i16 BE view of the 16-bit track region (first 16 values)
        let n16_dump = (header.num_16bit_tracks as usize).min(16);
        let i16_view: Vec<i16> = (0..n16_dump)
            .map(|k| {
                let i = k * 2;
                i16::from_be_bytes([bytes[i], bytes[i + 1]])
            })
            .collect();
        eprintln!(
            "[anim-bytes] {tag} frame[{f}] i16[{}..{}]={:?}",
            0, n16_dump, i16_view
        );
        // i8 view of the 8-bit track region (first 16 values), starts after pad-to-16 of n16*2
        let n16_bytes = (header.num_16bit_tracks as usize) * 2;
        let off8 = (n16_bytes + 15) & !15;
        let n8_dump = (header.num_8bit_tracks as usize).min(16);
        if off8 + n8_dump <= bytes.len() {
            let i8_view: Vec<i8> = (0..n8_dump).map(|k| bytes[off8 + k] as i8).collect();
            eprintln!(
                "[anim-bytes] {tag} frame[{f}] i8[off8=0x{:X} 0..{}]={:?}",
                off8, n8_dump, i8_view
            );
        }
    }
}

pub(crate) fn decode_clips_for_moby_inline(
    level_folder: &Path,
    main_dat_filename: &str,
    anim_offsets: &[u64],
    position_scale: f32,
    scale_scale: f32,
    skeleton: &Skeleton,
    layout: LevelLayout,
    moby_tuid: u64,
    profile: AnimProfile,
) -> Vec<DecodedClip> {
    let _ = profile;
    if anim_offsets.is_empty() {
        return Vec::new();
    }
    let log_probes = std::env::var("RECHIMERA_LOG_PROBES").is_ok();
    // Frame-bytes probe — fires once per process per layout, ahead of any
    // decode so the TOD early-return still gets a dump. The probe needs to
    // open main.dat itself since the TOD branch otherwise short-circuits.
    if log_probes {
        let probe_flag = match layout {
            LevelLayout::Tod => &TOD_ANIM_BYTES_PROBE_FIRED,
            LevelLayout::Rfom => &RFOM_ANIM_BYTES_PROBE_FIRED,
            LevelLayout::V2 => return Vec::new(), // shouldn't reach inline path
        };
        if probe_flag
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let probe_path = level_folder.join(main_dat_filename);
            if let Ok(file) = std::fs::File::open(&probe_path) {
                if let Ok(mut probe_ig) = IgFile::open(std::io::BufReader::new(file)) {
                    let tag = match layout {
                        LevelLayout::Tod => "tod",
                        LevelLayout::Rfom => "rfom",
                        _ => "?",
                    };
                    probe_anim_bytes(&mut probe_ig, tag, anim_offsets[0]);
                }
            }
        }
    }
    let main_path = level_folder.join(main_dat_filename);
    let file = match std::fs::File::open(&main_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("warn: inline anim decode — open {main_dat_filename}: {e}");
            return Vec::new();
        }
    };
    let mut ig = match IgFile::open(std::io::BufReader::new(file)) {
        Ok(ig) => ig,
        Err(e) => {
            eprintln!("warn: inline anim decode — IgFile::open {main_dat_filename}: {e}");
            return Vec::new();
        }
    };
    let skel_bones = skeleton.bones.len() as u16;
    let mut out = Vec::with_capacity(anim_offsets.len());
    let probe_layout = matches!(layout, LevelLayout::Rfom)
        && std::env::var("RECHIMERA_LOG_ANIM_DETAIL").is_ok();
    for (i, off) in anim_offsets.iter().enumerate() {
        if probe_layout && i == 0 {
            eprintln!("[rfom-anim-probe] anim[0] @ file_off=0x{:08X}", off);
        }
        let mut header = match read_animation_header_at(&mut ig, *off) {
            Ok(h) => h,
            Err(e) => {
                eprintln!(
                    "warn: inline anim[{i}] header read failed @ 0x{:08X}: {e}",
                    off
                );
                continue;
            }
        };
        if probe_layout && i == 0 {
            eprintln!(
                "[rfom-anim-probe] header name='{}' frames={} bones={} flags=0x{:04X} fps={} stride={} 16bit={} 8bit={} ctrl_ptr=0x{:08X} frames_ptr=0x{:08X}",
                header.name,
                header.num_frames,
                header.num_bones,
                header.flags,
                header.frame_rate,
                header.frame_stride,
                header.num_16bit_tracks,
                header.num_8bit_tracks,
                header.control_ptr,
                header.frames_ptr
            );
        }
        if header.is_additive() && skel_bones > 0 {
            header.num_bones = skel_bones;
        }
        if !matches!(layout, LevelLayout::Tod) {
            header.apply_frame_stride_padding();
        }
        // TOD pair-frame encoding: each logical keyframe is stored as TWO
        // consecutive frame_stride rows — even-index = zero filler,
        // odd-index = real values. To consume with the standard IT decoder
        // we halve the frame count, double the stride, and offset the
        // frames_ptr by one stride so frame[0] lands on the first real
        // keyframe. RE'd from `animate_spin` probe (see
        // `project_tod_anim_format` memory). RFOM/V2 anims are left alone.
        // TOD pair-frame transform — but ONLY for anims that actually
        // need it. The encoding is: every other disk frame is zero-filler
        // (real keyframes at odd indices). This shows up only when the
        // disk frame_stride equals the minimum data-size required for
        // the per-frame i16/i8 tracks. If the disk stride is larger than
        // the data size (e.g. 192 stride for ~96 bytes of data), the
        // anim already packs real keyframes into every disk frame and
        // applying the pair-frame halving WRECKS it by dropping every
        // other keyframe.
        if matches!(layout, LevelLayout::Tod)
            && header.num_frames >= 2
            && header.frame_stride > 0
            && header.frames_ptr != 0
        {
            let n16_bytes = (header.num_16bit_tracks as usize) * 2;
            let padded_i16 = (n16_bytes + 15) & !15;
            let n8_bytes = header.num_8bit_tracks as usize;
            let min_data = ((padded_i16 + n8_bytes) + 15) & !15;
            let stride_usize = header.frame_stride as usize;
            let is_simple_pair_frame = header.num_8bit_tracks == 0
                && min_data > 0
                && stride_usize == min_data;
            if is_simple_pair_frame {
                // n8=0 simple anims store every other disk frame as zero
                // filler; real keyframes live at odd indices. Halve
                // num_frames, double frame_stride, offset frames_ptr by
                // one (original) stride so frame[0] lands on the first
                // real keyframe. RE'd from `animate_spin` (smooth linear
                // Y rotation across odd frames). See `project_tod_anim_format`
                // memory.
                let real_frames = header.num_frames / 2;
                if real_frames >= 1 {
                    header.frames_ptr = header.frames_ptr.saturating_add(header.frame_stride as u32);
                    header.frame_stride = header.frame_stride.saturating_mul(2);
                    header.num_frames = real_frames;
                    header.frame_rate /= 2.0;
                    if log_probes && should_log_tod_anim_decision(moby_tuid, &header.name) {
                        eprintln!(
                            "[tod-anim] PAIR    moby_{:04X} '{}' n16={} stride→{} frames→{} fps→{}",
                            moby_tuid, header.name,
                            header.num_16bit_tracks,
                            header.frame_stride, header.num_frames, header.frame_rate,
                        );
                    }
                    // Fall through to standard decode below.
                } else {
                    if log_probes && should_log_tod_anim_decision(moby_tuid, &header.name) {
                        eprintln!(
                            "[tod-anim] T-POSE   moby_{:04X} '{}' n16={} (degenerate pair-frame: real_frames=0) → skip",
                            moby_tuid, header.name, header.num_16bit_tracks,
                        );
                    }
                    continue;
                }
            } else if header.num_8bit_tracks > 0 {
                // Complex anims (n8 > 0): per-frame i8 delta encoding
                // produces wild distortion when run through the standard
                // decoder. No reference implementation in IT or ReLunacy.
                // T-pose for now; revisit per `project_tod_anim_format`.
                if log_probes && should_log_tod_anim_decision(moby_tuid, &header.name) {
                    eprintln!(
                        "[tod-anim] T-POSE   moby_{:04X} '{}' n16={} n8={} stride={} min_data={} (n8>0 complex) → skip decode",
                        moby_tuid, header.name,
                        header.num_16bit_tracks, header.num_8bit_tracks,
                        stride_usize, min_data,
                    );
                }
                continue;
            }
            // n8=0 with stride > min_data: anim packs real keyframes into
            // every disk frame already, no pair-frame transform needed.
            // Fall through to standard decode.
        }
        let ctrl = match read_animation_control(&mut ig, &header) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "warn: inline anim[{i}] '{}' control read failed: {e}",
                    header.name
                );
                continue;
            }
        };

        // One-shot diagnostic probe for the first TOD anim that has at
        // least one Position OR Scale track. Dumps the actual scale
        // values being applied + raw track[0..n] mask info + first 3
        // frames of raw i16 values. This lets us see if pos_scale is
        // amplifying the data to absurd levels.
        if log_probes
            && matches!(layout, LevelLayout::Tod)
        {
            let has_non_rotation = ctrl.track16_masks.iter().any(|m| {
                !matches!(m.kind, lunalib::TrackKind::Rotation)
            }) || ctrl.track8_masks.iter().any(|m| {
                !matches!(m.kind, lunalib::TrackKind::Rotation)
            });
            if has_non_rotation
                && TOD_TRANS_PROBE_FIRED
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                eprintln!(
                    "[tod-trans-probe] moby_{:04X} '{}' has non-rotation tracks. nb={} nf={} stride={} n16={} n8={} pos_scale={:.8} scale_scale={:.8} skel.trans_shift={} skel.scale_shift={}",
                    moby_tuid, header.name,
                    header.num_bones,
                    header.num_frames,
                    header.frame_stride,
                    header.num_16bit_tracks,
                    header.num_8bit_tracks,
                    position_scale,
                    scale_scale,
                    skeleton.translation_shift,
                    skeleton.scale_shift,
                );
                // List the non-rotation track masks
                for (idx, m) in ctrl.track16_masks.iter().enumerate() {
                    if !matches!(m.kind, lunalib::TrackKind::Rotation) {
                        eprintln!(
                            "[tod-trans-probe]   t16[{}] bone={} comp={} kind={:?}",
                            idx, m.bone_index, m.component, m.kind
                        );
                        if idx >= 20 { eprintln!("[tod-trans-probe]   …"); break; }
                    }
                }
                // First 3 frames raw i16 values
                for f in 0..(header.num_frames as usize).min(3) {
                    let off = u64::from(header.frames_ptr)
                        + (f as u64) * (header.frame_stride as u64);
                    if ig.stream.seek_to(off).is_ok() {
                        let take = (header.num_16bit_tracks as usize).min(20);
                        let mut vals: Vec<i16> = Vec::with_capacity(take);
                        for _ in 0..take {
                            if let Ok(v) = ig.stream.read_i16() {
                                vals.push(v);
                            }
                        }
                        eprintln!(
                            "[tod-trans-probe]   frame[{f}] @ 0x{:08X} i16[0..{}]={:?}",
                            off, take, vals
                        );
                    }
                }
            }
        }

        let result = decode_animation_with_skeleton(
            &mut ig,
            &header,
            &ctrl,
            position_scale,
            scale_scale,
            skeleton,
        );
        match result {
            Ok(clip) => out.push(clip),
            Err(e) => {
                eprintln!(
                    "warn: inline anim[{i}] '{}' decode failed: {e}",
                    header.name
                );
            }
        }
    }
    compose_additive_overlays(&mut out, false, false);
    out
}

pub(crate) fn decode_clips_for_moby(
    level_folder: &Path,
    index: &AnimsetIndex,
    animsets_file: &mut std::fs::File,
    animset_hash: u64,
    position_scale: f32,
    scale_scale: f32,
    skel_bones: u16,
    skel: Option<&lunalib::skeleton::Skeleton>,
    profile: AnimProfile,
) -> Vec<DecodedClip> {
    let Some(&(offset, length)) = index.by_hash.get(&animset_hash) else {
        return Vec::new();
    };
    use std::io::{Read, Seek, SeekFrom};
    if animsets_file
        .seek(SeekFrom::Start(u64::from(offset)))
        .is_err()
    {
        return Vec::new();
    }
    let mut buf = vec![0u8; length as usize];
    if animsets_file.read_exact(&mut buf).is_err() {
        return Vec::new();
    }
    let mut ig = match IgFile::open(std::io::Cursor::new(buf)) {
        Ok(ig) => ig,
        Err(_) => return Vec::new(),
    };
    let offsets = animation_section_offsets(&ig);
    let mut out = Vec::with_capacity(offsets.len());
    let debug_this = animset_matches_debug_env(animset_hash);
    if debug_this {
        eprintln!(
            "[anim-debug] === animset 0x{animset_hash:016X} ({} clips, skel_bones={skel_bones}) ===",
            offsets.len()
        );
    }
    for (i, off) in offsets.into_iter().enumerate() {
        let mut header = match read_animation_header_at(&mut ig, off) {
            Ok(h) => h,
            Err(e) => {
                eprintln!(
                    "warn: animset 0x{animset_hash:016X} clip[{i}] header read failed: {e}"
                );
                continue;
            }
        };
        let raw_num_bones = header.num_bones;
        let raw_stride = header.frame_stride;
        // Additive `numBones` override — IT's LoadAnimations
        // (gltf_shared.cpp ~542) overwrites the clip's stored numBones with
        // the skeleton's full bone count BEFORE reading any control-blob
        // section. The control blob's section offsets are computed as
        // `numBones * 8 + padding`, so the on-disk layout uses the canonical
        // (max) bone count across the animset, not the per-clip annotation.
        // Without this override, track-mask and blend-mask reads land in
        // garbage and bone_index values overflow past skel_bones.
        if header.is_additive() && skel_bones > 0 {
            header.num_bones = skel_bones;
        }
        header.apply_frame_stride_padding();
        if debug_this {
            eprintln!(
                "[anim-debug] clip[{i:3}] '{}' flags=0x{:04X} (L={} A={} P={}) nb={}->{} nf={} stride={}->{} n16={} n8={} nrv={} frames@0x{:08X} ctrl@0x{:08X}",
                header.name,
                header.flags,
                header.is_looping() as u8,
                header.is_additive() as u8,
                header.is_packed_frames() as u8,
                raw_num_bones,
                header.num_bones,
                header.num_frames,
                raw_stride,
                header.frame_stride,
                header.num_16bit_tracks,
                header.num_8bit_tracks,
                header.num_reference_values,
                header.frames_ptr,
                header.control_ptr,
            );
        }
        let ctrl = match read_animation_control(&mut ig, &header) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "warn: animset 0x{animset_hash:016X} clip[{i}] '{}' control read failed: {e}",
                    header.name
                );
                continue;
            }
        };
        if debug_this {
            let max_t16_bone = ctrl
                .track16_masks
                .iter()
                .map(|m| m.bone_index)
                .max()
                .unwrap_or(0);
            let max_t8_bone = ctrl
                .track8_masks
                .iter()
                .map(|m| m.bone_index)
                .max()
                .unwrap_or(0);
            eprintln!(
                "[anim-debug] clip[{i:3}]   ctrl: t16_masks={} (max_bone={}) t8_masks={} (max_bone={}) ref_pose_masks={} blend_masks={}",
                ctrl.track16_masks.len(),
                max_t16_bone,
                ctrl.track8_masks.len(),
                max_t8_bone,
                ctrl.ref_pose_masks.len(),
                ctrl.blend_masks.len(),
            );
            if max_t16_bone >= skel_bones || max_t8_bone >= skel_bones {
                eprintln!(
                    "[anim-debug] clip[{i:3}]   *** WARN: track-mask bone index >= skel_bones={} (track will be silently dropped)",
                    skel_bones
                );
            }
        }

        let decode_result = match skel {
            Some(s) => decode_animation_with_skel(&mut ig, &header, &ctrl, position_scale, scale_scale, s, profile),
            None => decode_animation(&mut ig, &header, &ctrl, position_scale, scale_scale),
        };
        match decode_result {
            Ok(clip) => {
                if debug_this {
                    let animated_rot = clip.bones.iter().filter(|b| b.rotation_animated).count();
                    let animated_pos = clip.bones.iter().filter(|b| b.translation_animated).count();
                    let animated_scl = clip.bones.iter().filter(|b| b.scale_animated).count();
                    let first_rots: Vec<f32> = clip
                        .bones
                        .first()
                        .map(|b| b.rotations.iter().take(8).copied().collect())
                        .unwrap_or_default();
                    eprintln!(
                        "[anim-debug] clip[{i:3}]   decoded: bones={} animated R/P/S={}/{}/{} first_bone_rot[0..8]={:?}",
                        clip.bones.len(),
                        animated_rot,
                        animated_pos,
                        animated_scl,
                        first_rots,
                    );
                }
                out.push(clip);
            }
            Err(e) => {
                eprintln!(
                    "warn: animset 0x{animset_hash:016X} clip[{i}] '{}' decode failed (level {level_folder:?}): {e}",
                    header.name
                );
            }
        }
    }
    compose_additive_overlays(&mut out, debug_this, profile.game == Some(lunalib::Game::R3));
    out
}

/// Walks decoded clips and bakes the matching `*_idle_p` pose underneath every
/// `*_fire_p` / `*_alt_fire_p` / `*_fire_cycle_p` overlay. See the memory
/// [[r2-fire-p-additive-overlay-standalone-limit]] — R2 weapon recoils are
/// runtime-additive deltas, and without idle as the base the un-kicked bones
/// snap to skeleton bind (T-shape arms holding no rifle).
fn compose_additive_overlays(clips: &mut Vec<DecodedClip>, _debug: bool, r3: bool) {
    use std::collections::HashMap;
    let force_all = std::env::var("RECHIMERA_COMPOSE_FORCE")
        .map(|v| v.eq_ignore_ascii_case("all"))
        .unwrap_or(false);
    let name_to_idx: HashMap<String, usize> = clips
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.clone(), i))
        .collect();
    let fallback_idx = name_to_idx
        .get("mp_stand_dle")
        .or_else(|| name_to_idx.get("mp_stand_idle"))
        .copied();
    let mut pairs: Vec<(usize, Option<usize>, String, &'static str)> = Vec::new();
    for (i, c) in clips.iter().enumerate() {
        let base_name = idle_base_for_overlay(&c.name).or_else(|| {
            if r3 && c.additive {
                r3_base_for_overlay(&c.name)
            } else {
                None
            }
        });
        if let Some(base_name) = base_name {
            if let Some(&base_idx) = name_to_idx.get(&base_name) {
                if base_idx != i {
                    pairs.push((i, Some(base_idx), base_name, "weapon-idle"));
                    continue;
                }
            }
            if let Some(fb) = fallback_idx {
                if fb != i {
                    pairs.push((i, Some(fb), "mp_stand_dle".to_string(), "stand-dle fallback"));
                    continue;
                }
            }
            pairs.push((i, None, base_name, "none"));
        }
    }
    if pairs.is_empty() {
        return;
    }
    let verbose = std::env::var("RECHIMERA_LOG_ANIM_DETAIL").is_ok();
    let total = pairs.len();
    let mut composed = 0usize;
    let mut skipped = 0usize;
    for (fire_idx, base_idx_opt, base_name, src) in pairs {
        let fire_name = clips[fire_idx].name.clone();
        let fire_nf = clips[fire_idx].num_frames;
        match base_idx_opt {
            Some(base_idx) => {
                let base = clips[base_idx].clone();
                let base_nf = base.num_frames;
                if fire_name == "mp_carbine_fire_p"
                    && std::env::var("RECHIMERA_LOG_FIRE_VS_IDLE").is_ok()
                {
                    dump_fire_vs_idle_rotations(&clips[fire_idx], &base);
                }
                let (rc, tc, sc) = clips[fire_idx].compose_with_base(&base, true);
                composed += 1;
                if verbose {
                    eprintln!(
                        "[anim-compose]  '{fire_name}' (nf={fire_nf}) <- '{base_name}' (nf={base_nf}) [{src}] — \
                         copied rot={rc} tra={tc} scl={sc} bones"
                    );
                }
            }
            None => {
                skipped += 1;
                if verbose {
                    eprintln!(
                        "[anim-compose]  '{fire_name}' (nf={fire_nf}) — NO base '{base_name}' or 'mp_stand_dle' in animset"
                    );
                }
            }
        }
    }
    eprintln!(
        "[anim-compose] {} clips: composed {} of {} overlay candidates ({} without base, force_all={})",
        clips.len(),
        composed,
        total,
        skipped,
        force_all
    );
}

/// Diagnostic — dump per-frame decoded rotation for the first 6 ANIMATED bones
/// in `fire`, side-by-side with the matching frame from `base` (idle). If fire's
/// values look near-identity and idle's look like real rotations, fire is a
/// delta and needs the `idle * fire` multiply. If fire's values look like real
/// rotations (large XYZ components) and idle's look similar, fire is absolute
/// and the multiply is over-applying.
fn dump_fire_vs_idle_rotations(fire: &lunalib::DecodedClip, base: &lunalib::DecodedClip) {
    eprintln!("[fire-vs-idle] '{}' (nf={}) vs '{}' (nf={})",
        fire.name, fire.num_frames, base.name, base.num_frames);
    let fire_nf = fire.num_frames.max(1) as usize;
    let base_nf = base.num_frames.max(1) as usize;
    // Dump frame-0 only for ALL animated bones — find the ones whose rotation
    // would land somewhere wrong after fill. Look for bones where fire's
    // tracked components produce a quat that doesn't blend cleanly with idle.
    for (b, fb) in fire.bones.iter().enumerate() {
        if !fb.rotation_animated { continue; }
        if fb.rotations.len() < fire_nf * 4 { continue; }
        let bb = &base.bones[b];
        let mc = fb.rot_components;
        let missing = ['X','Y','Z','W'].iter().enumerate()
            .filter_map(|(i, ch)| if mc & (1 << i) == 0 { Some(*ch) } else { None })
            .collect::<String>();
        let missing_str = if missing.is_empty() { "none".to_string() } else { missing };
        let fq = (fb.rotations[0], fb.rotations[1], fb.rotations[2], fb.rotations[3]);
        let (ix, iy, iz, iw) = if bb.rotation_animated && bb.rotations.len() >= base_nf * 4 {
            (bb.rotations[0], bb.rotations[1], bb.rotations[2], bb.rotations[3])
        } else if bb.rotations.len() == 4 {
            (bb.rotations[0], bb.rotations[1], bb.rotations[2], bb.rotations[3])
        } else {
            (0.0, 0.0, 0.0, 1.0)
        };
        eprintln!(
            "[fire-vs-idle]   bone {b:3}: rc=0b{mc:04b} missing={missing_str:>4}  fire=({:+.3},{:+.3},{:+.3},{:+.3})  idle=({:+.3},{:+.3},{:+.3},{:+.3})",
            fq.0, fq.1, fq.2, fq.3, ix, iy, iz, iw,
        );
    }
}

/// Name heuristic: returns the matching idle clip for a fire/recoil overlay.
/// Examples:
///   mp_carbine_fire_p       -> mp_carbine_idle_p
///   mp_carbine_alt_fire_p   -> mp_carbine_idle_p
///   mp_minigun_fire_cycle_p -> mp_minigun_idle_p
pub(crate) fn idle_base_for_overlay(name: &str) -> Option<String> {
    for suffix in ["_alt_fire_p", "_fire_cycle_p", "_fire_p"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            return Some(format!("{stem}_idle_p"));
        }
    }
    None
}

pub(crate) fn r3_base_for_overlay(name: &str) -> Option<String> {
    if name == "visemes" || name == "head_visemes" {
        return Some("head_idle".to_string());
    }
    if name.starts_with("exp_") && (name.ends_with("_lower") || name.ends_with("_upper")) {
        return Some("head_idle".to_string());
    }
    if !name.ends_with("_idle_p") {
        if let Some(stem) = name.strip_suffix("_p") {
            return Some(format!("{stem}_idle_p"));
        }
    }
    None
}

const ANIMSET_MEMO_CAP: usize = 6;

fn fnv1a_fold(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash ^= u64::from(b);
        *hash = hash.wrapping_mul(0x100000001B3);
    }
}

fn anim_offsets_key(offsets: &[u64]) -> u64 {
    let mut h: u64 = 0xCBF29CE484222325;
    for &o in offsets {
        fnv1a_fold(&mut h, &o.to_le_bytes());
    }
    h
}

// Memo key MUST include the skeleton fingerprint, not just the animset id:
// decoded clips inherit bind-pose fallbacks and shift-derived pos/scale from
// the moby's skeleton, so two rigs sharing an animset decode differently.
fn skeleton_anim_fingerprint(skel: Option<&lunalib::Skeleton>) -> u64 {
    let mut h: u64 = 0xCBF29CE484222325;
    let Some(s) = skel else {
        return h;
    };
    fnv1a_fold(&mut h, &(s.bones.len() as u64).to_le_bytes());
    fnv1a_fold(&mut h, &s.translation_shift.to_le_bytes());
    fnv1a_fold(&mut h, &s.scale_shift.to_le_bytes());
    fnv1a_fold(&mut h, &s.root_bone.to_le_bytes());
    for b in &s.bones {
        fnv1a_fold(&mut h, &b.flags.to_le_bytes());
        fnv1a_fold(&mut h, &b.parent_index.to_le_bytes());
    }
    for m in &s.bind_local {
        for f in m {
            fnv1a_fold(&mut h, &f.to_bits().to_le_bytes());
        }
    }
    h
}

fn animset_matches_debug_env(animset_hash: u64) -> bool {
    let Ok(want) = std::env::var("RECHIMERA_DEBUG_ANIMSET") else {
        return false;
    };
    let want = want.trim().trim_start_matches("0x").trim_start_matches("0X");
    let Ok(v) = u64::from_str_radix(want, 16) else {
        return false;
    };
    v == animset_hash
}

fn ensure_dirs(root: &Path) -> Result<(), String> {
    for sub in ["mobys", "ties", "textures"] {
        fs::create_dir_all(root.join(sub))
            .map_err(|e| format!("create {sub} dir: {e}"))?;
    }
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<u64, String> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| format!("serialize {path:?}: {e}"))?;
    let len = bytes.len() as u64;
    fs::write(path, bytes).map_err(|e| format!("write {path:?}: {e}"))?;
    Ok(len)
}

#[tauri::command]
pub fn extract_level_to_cache(
    folder: String,
    game_id: Option<String>,
    on_event: Channel<CacheEvent>,
) -> Result<(), String> {
    let game = game_id
        .as_deref()
        .and_then(Game::from_id)
        .or_else(|| recover_game_from_sidecar(&folder));
    match run_extract(&folder, game, &on_event) {
        Ok(entry_count) => {
            let _ = on_event.send(CacheEvent::Done { entry_count });
            Ok(())
        }
        Err(message) => {
            let _ = on_event.send(CacheEvent::Error {
                message: message.clone(),
            });
            Err(message)
        }
    }
}

/// When `RECHIMERA_DEBUG_MOBY=<hex>[,<hex>...]` is set, only mobys whose tuid hex
/// ends with one of the given suffixes are processed (case-insensitive, accepts
/// `0x` prefix per entry). All other extraction phases (ties, details, ufrags,
/// sky, lights, env-samplers, gameplay) are skipped to make the iteration loop
/// tight for debugging. Textures still run — they're filtered to whatever the
/// matched mobys need.
///
/// Examples:
///   `RECHIMERA_DEBUG_MOBY=0212`        — one moby
///   `RECHIMERA_DEBUG_MOBY=0212,00CD`   — multiple
///   `RECHIMERA_DEBUG_MOBY=0x0212,0326` — `0x` prefix allowed
fn debug_moby_filter() -> Option<Vec<String>> {
    let raw = std::env::var("RECHIMERA_DEBUG_MOBY").ok()?;
    let suffixes: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().trim_start_matches("0x").trim_start_matches("0X").to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    if suffixes.is_empty() {
        None
    } else {
        Some(suffixes)
    }
}

fn debug_moby_match(tuid: u64, suffixes: &[String]) -> bool {
    let hex = format!("{:016X}", tuid);
    suffixes.iter().any(|s| hex.ends_with(s.as_str()))
}

/// Per-submesh shader → texture resolution dump for one moby. Self-gates on
/// `RECHIMERA_DEBUG_MOBY` matching the tuid suffix — same convention as
/// `dump_skeleton_bind`. Surfaces:
///   - the moby's full shader_tuids palette
///   - per submesh: shader_index → resolved shader_tuid → (albedo, normal,
///     expensive) texture IDs → whether each texture made it into the
///     encoded PNG set (✓) or got dropped (MISSING)
///   - rollup at the end: count of submeshes with a working albedo
fn dump_moby_shader_textures(
    asset: &lunalib::MobyAsset,
    shaders: &HashMap<u64, ShaderInfo>,
    texture_pngs: &HashMap<u32, Vec<u8>>,
) {
    let Some(suffixes) = debug_moby_filter() else {
        return;
    };
    if !debug_moby_match(asset.tuid, &suffixes) {
        return;
    }
    let total_submeshes: usize = asset.bangles.iter().map(|b| b.meshes.len()).sum();
    let per_submesh = std::env::var("RECHIMERA_LOG_MOBY_TEX").is_ok();
    let mut ok_albedo = 0usize;
    let mut missing_albedo = 0usize;
    let mut no_shader = 0usize;
    let mut submesh_idx = 0usize;
    for (bangle_idx, bangle) in asset.bangles.iter().enumerate() {
        for m in &bangle.meshes {
            let (albedo, normal, expensive) = resolve_shader_textures(
                shaders,
                &asset.shader_tuids,
                m.shader_index as usize,
            );
            let alb_state = match albedo {
                None => { no_shader += 1; "no-albedo".to_string() }
                Some(id) if texture_pngs.contains_key(&id) => {
                    ok_albedo += 1;
                    format!("OK 0x{:08X}", id)
                }
                Some(id) => { missing_albedo += 1; format!("MISSING 0x{:08X}", id) }
            };
            if per_submesh {
                let shader_tuid = asset
                    .shader_tuids
                    .get(m.shader_index as usize)
                    .copied()
                    .unwrap_or(0);
                let nrm_state = match normal {
                    None => "none".to_string(),
                    Some(id) if texture_pngs.contains_key(&id) => format!("0x{:08X}", id),
                    Some(id) => format!("MISSING 0x{:08X}", id),
                };
                let exp_state = match expensive {
                    None => "none".to_string(),
                    Some(id) if texture_pngs.contains_key(&id) => format!("0x{:08X}", id),
                    Some(id) => format!("MISSING 0x{:08X}", id),
                };
                eprintln!(
                    "[moby-tex]   submesh[{:3}] b{}.m shader_idx={} (tuid=0x{:016X}) verts={} alb={} nrm={} exp={}",
                    submesh_idx, bangle_idx, m.shader_index, shader_tuid,
                    m.vertex_count, alb_state, nrm_state, exp_state,
                );
            }
            submesh_idx += 1;
        }
    }
    eprintln!(
        "[moby-tex] moby_{:016X} '{}' submeshes={} ok_albedo={} missing={} no_shader={}",
        asset.tuid, asset.name, total_submeshes, ok_albedo, missing_albedo, no_shader,
    );
}

/// R3 mobys carry legacy shader slots whose textures never shipped in retail
/// (Rachel's `legacy/rachel_cinemares_face` -> `damon/rachel/rachael_
/// cinematicres_head_*`; the blackops head's `artdepartment/character/
/// blackops/*`). When a shader's albedo is provably absent from the encoded
/// set after every recovery tier, replace its ABSENT channels (albedo,
/// normal, expensive) with the full channel set of a resolved SIBLING
/// shader from the same bangle, majority-voted across bangles — legacy
/// face-variant meshes share bangles with the shipped face parts, so the
/// sibling carries the correct atlas set (verified by hand-mapping in
/// Blender). Raw palette dominance is only the last resort for shaders
/// with no resolved sibling anywhere (it can pick hair over face).
/// R3-only at the call site; returns None when nothing needs patching.
fn substitute_absent_albedos(
    shaders: &HashMap<u64, lunalib::ShaderInfo>,
    asset: &lunalib::MobyAsset,
    texture_pngs: &HashMap<u32, Vec<u8>>,
) -> Option<HashMap<u64, lunalib::ShaderInfo>> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    let mut missing: Vec<u64> = Vec::new();
    for &st in &asset.shader_tuids {
        let Some(s) = shaders.get(&st) else { continue };
        match s.albedo_tex_id {
            Some(id) if texture_pngs.contains_key(&id) => {
                *counts.entry(id).or_insert(0) += 1;
            }
            Some(_) => missing.push(st),
            None => {}
        }
    }
    if missing.is_empty() {
        return None;
    }
    missing.sort_unstable();
    missing.dedup();

    let present = |id: Option<u32>| id.map_or(false, |v| texture_pngs.contains_key(&v));
    let mut votes: HashMap<u64, HashMap<u64, usize>> = HashMap::new();
    for bangle in &asset.bangles {
        let mut broken_here: Vec<u64> = Vec::new();
        let mut donors_here: Vec<u64> = Vec::new();
        for m in &bangle.meshes {
            let Some(&st) = asset.shader_tuids.get(m.shader_index as usize) else {
                continue;
            };
            let Some(s) = shaders.get(&st) else { continue };
            if present(s.albedo_tex_id) {
                donors_here.push(st);
            } else if s.albedo_tex_id.is_some() {
                broken_here.push(st);
            }
        }
        for b in &broken_here {
            for d in &donors_here {
                *votes.entry(*b).or_default().entry(*d).or_insert(0) += 1;
            }
        }
    }

    let dominant = counts.iter().max_by_key(|(_, c)| **c).map(|(id, _)| *id)?;
    let mut patched = shaders.clone();
    for st in &missing {
        let donor = votes
            .get(st)
            .and_then(|v| v.iter().max_by_key(|(_, c)| **c).map(|(d, _)| *d))
            .and_then(|d| shaders.get(&d).copied());
        if let Some(s) = patched.get_mut(st) {
            match donor {
                Some(d) => {
                    eprintln!(
                        "[tex-substitute] moby 0x{:016X} shader 0x{:016X}: absent channels replaced from same-bangle sibling (albedo 0x{:08X})",
                        asset.tuid,
                        st,
                        d.albedo_tex_id.unwrap_or(0)
                    );
                    if present(d.albedo_tex_id) {
                        s.albedo_tex_id = d.albedo_tex_id;
                    } else {
                        s.albedo_tex_id = Some(dominant);
                    }
                    if !present(s.normal_tex_id) && present(d.normal_tex_id) {
                        s.normal_tex_id = d.normal_tex_id;
                    }
                    if !present(s.expensive_tex_id) && present(d.expensive_tex_id) {
                        s.expensive_tex_id = d.expensive_tex_id;
                    }
                }
                None => {
                    eprintln!(
                        "[tex-substitute] moby 0x{:016X} shader 0x{:016X}: albedo 0x{:08X} absent, no bangle sibling — substituting dominant 0x{:08X}",
                        asset.tuid,
                        st,
                        s.albedo_tex_id.unwrap_or(0),
                        dominant
                    );
                    s.albedo_tex_id = Some(dominant);
                }
            }
        }
    }
    Some(patched)
}

fn recover_game_from_sidecar(folder: &str) -> Option<Game> {
    let sidecar = cache_root(folder).join("game.json");
    let bytes = fs::read(&sidecar).ok()?;
    let s = std::str::from_utf8(&bytes).ok()?;
    let start = s.find("\"game\":\"")?;
    let rest = &s[start + 8..];
    let end = rest.find('"')?;
    Game::from_id(&rest[..end])
}

pub(crate) fn resolve_profile_for_folder(folder: &str, override_game_id: Option<&str>) -> AnimProfile {
    if let Some(id) = override_game_id {
        if let Some(g) = Game::from_id(id) {
            return g.anim_profile();
        }
    }
    if let Some(g) = recover_game_from_sidecar(folder) {
        return g.anim_profile();
    }
    AnimProfile::LEGACY
}

fn run_extract(folder: &str, game: Option<Game>, on_event: &Channel<CacheEvent>) -> Result<usize, String> {
    let profile = match game {
        Some(g) => g.anim_profile(),
        None => AnimProfile::LEGACY,
    };
    eprintln!(
        "[cache] anim profile: game={:?} delta_pos_scale={} blend_mask_gate={}",
        profile.game, profile.apply_delta_pos_scale, profile.apply_blend_mask_rotation_gate,
    );
    let level_path = Path::new(folder);
    let root = cache_root(folder);
    ensure_dirs(&root)?;

    if let Some(g) = game {
        let sidecar = root.join("game.json");
        let _ = fs::write(
            &sidecar,
            format!("{{\"game\":\"{}\"}}\n", match g {
                Game::Rfom => "r1",
                Game::R2 => "r2",
                Game::R3 => "r3",
                Game::Tod => "rc_tod",
                Game::A4O => "rc_a4o",
                Game::ACiT => "rc_acit",
                Game::FFA => "rc_ffa",
            }),
        );
    }

    let layout = detect_layout(level_path)
        .map_err(|_| {
            "Folder has none of main.dat (TOD), assetlookup.dat (V2), or ps3levelmain.dat (RFOM)"
                .to_string()
        })?;
    eprintln!(
        "[cache] level layout detected: {} ({})",
        layout.tag(),
        layout.label()
    );

    let debug_filter = debug_moby_filter();
    if let Some(suffixes) = &debug_filter {
        eprintln!(
            "[cache] DEBUG MODE: RECHIMERA_DEBUG_MOBY={} — extracting only matching mobys, \
             skipping ties/details/ufrags/sky/lights/envsamplers/gameplay phases.",
            suffixes.join(","),
        );
    }

    // RFOM section-list probe — only useful when investigating unknown
    // section IDs. Gated behind RECHIMERA_LOG_PROBES=1.
    if matches!(layout, LevelLayout::Rfom) && std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        let entry_path = level_path.join("ps3levelmain.dat");
        match std::fs::File::open(&entry_path) {
            Ok(file) => match IgFile::open(std::io::BufReader::new(file)) {
                Ok(ig) => {
                    eprintln!(
                        "[probe-rfom] ps3levelmain.dat IGHW v{}.{} — {} section(s):",
                        ig.version.major,
                        ig.version.minor,
                        ig.sections.len()
                    );
                    for s in &ig.sections {
                        eprintln!(
                            "[probe-rfom]   id=0x{:04X} offset=0x{:X} count={} length={}",
                            s.id, s.offset, s.count, s.length
                        );
                    }
                }
                Err(e) => eprintln!("warn: [probe-rfom] parse ps3levelmain.dat: {e}"),
            },
            Err(e) => eprintln!("warn: [probe-rfom] open ps3levelmain.dat: {e}"),
        }
        let gp_path = level_path.join("ps3gameplay.dat");
        if gp_path.exists() {
            match std::fs::File::open(&gp_path) {
                Ok(file) => match IgFile::open(std::io::BufReader::new(file)) {
                    Ok(ig) => {
                        eprintln!(
                            "[probe-rfom-gp] ps3gameplay.dat IGHW v{}.{} — {} section(s):",
                            ig.version.major,
                            ig.version.minor,
                            ig.sections.len()
                        );
                        for s in &ig.sections {
                            eprintln!(
                                "[probe-rfom-gp]   id=0x{:04X} offset=0x{:X} count={} length={}",
                                s.id, s.offset, s.count, s.length
                            );
                        }
                    }
                    Err(e) => eprintln!("warn: [probe-rfom-gp] parse ps3gameplay.dat: {e}"),
                },
                Err(e) => eprintln!("warn: [probe-rfom-gp] open ps3gameplay.dat: {e}"),
            }
        }
    }

    // Stage A — TOD skeleton / animation probe. We log whatever
    // sections exist with class IDs 0xD300 (skeleton) and 0xF000
    // (animation) inside main.dat so we know whether the V2 layout
    // applies, the layout differs, or those sections don't exist at
    // all for TOD-era levels. Drives stage D (reverse engineering)
    // decisions.
    if matches!(layout, LevelLayout::Tod) {
        let main_path = level_path.join("main.dat");
        if let Ok(file) = std::fs::File::open(&main_path) {
            if let Ok(ig) = IgFile::open(std::io::BufReader::new(file)) {
                let mut skel_sections = 0usize;
                let mut anim_sections = 0usize;
                for s in &ig.sections {
                    match s.id {
                        0xD300 => {
                            eprintln!(
                                "[probe-tod-skeleton] section 0xD300 @ offset 0x{:X}, count={}, length={}",
                                s.offset, s.count, s.length
                            );
                            skel_sections += 1;
                        }
                        0xF000 => {
                            eprintln!(
                                "[probe-tod-animation] section 0xF000 @ offset 0x{:X}, count={}, length={}",
                                s.offset, s.count, s.length
                            );
                            anim_sections += 1;
                        }
                        _ => {}
                    }
                }
                if skel_sections == 0 {
                    eprintln!("[probe-tod-skeleton] no 0xD300 section in main.dat");
                }
                if anim_sections == 0 {
                    eprintln!("[probe-tod-animation] no 0xF000 section in main.dat");
                }
            }
        }
    }

    let manifest_path = root.join(MANIFEST_NAME);
    let in_progress = CacheManifest {
        version: MANIFEST_VERSION,
        folder: folder.to_string(),
        entries: Vec::new(),
        source_mtimes: HashMap::new(),
        complete: false,
    };
    write_json(&manifest_path, &in_progress)?;

    let shaders: HashMap<u64, ShaderInfo> = match layout {
        LevelLayout::V2 => read_shaders(level_path).map_err(|e| e.to_string())?,
        LevelLayout::Rfom => match lunalib::read_shaders_rfom(level_path) {
            Ok(map) => {
                eprintln!(
                    "[cache] RFOM layout: read {} materials from ps3levelmain.dat",
                    map.len()
                );
                if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
                    let _ = lunalib::probe_rfom_unknowns(level_path);
                }
                map
            }
            Err(e) => {
                eprintln!("warn: RFOM shader read failed ({e}) — using empty shader table");
                HashMap::new()
            }
        },
        LevelLayout::Tod => match lunalib::read_shaders_old(level_path) {
            Ok(map) => {
                eprintln!("[cache] TOD layout: read {} shaders from main.dat", map.len());
                map
            }
            Err(e) => {
                eprintln!("warn: TOD shader read failed ({e}) — using empty shader table");
                HashMap::new()
            }
        },
    };

    let animset_index = AnimsetIndex::build(level_path).ok();
    let animsets_path = level_path.join("animsets.dat");
    let mut animsets_file = std::fs::File::open(&animsets_path).ok();

    let mut entries: Vec<CacheManifestEntry> = Vec::new();
    // Track texture IDs by role so the cache phase can be split into
    // Materials (albedos) → Normal maps → Textures (emissive/other) for the
    // progress UI. Same convention across V2 / RFOM / TOD since the
    // categorisation happens here, not in the per-game decoders below.
    let mut needed_albedos: HashSet<u32> = HashSet::new();
    let mut needed_normals: HashSet<u32> = HashSet::new();
    let mut needed_emissives: HashSet<u32> = HashSet::new();

    let mut moby_assets_for_glb: Vec<lunalib::MobyAsset> = Vec::new();
    let mut tie_assets_for_glb: Vec<lunalib::TieAsset> = Vec::new();

    let mut moby_done = 0usize;
    let mut emitted_moby_tuids: Vec<u64> = Vec::new();
    let moby_result = {
        let mut on_total = |total: usize| {
            let _ = on_event.send(CacheEvent::Phase {
                phase: "mobys",
                total,
            });
        };
        let mut on_moby = |asset: lunalib::MobyAsset| {
            if let Some(suffixes) = &debug_filter {
                if !debug_moby_match(asset.tuid, suffixes) {
                    return;
                }
            }
            emitted_moby_tuids.push(asset.tuid);
            moby_assets_for_glb.push(asset.clone());

            let mut submeshes = Vec::new();
            for bangle in &asset.bangles {
                for m in &bangle.meshes {
                    let (albedo, normal, emissive) = resolve_shader_textures(
                        &shaders,
                        &asset.shader_tuids,
                        m.shader_index as usize,
                    );
                    if let Some(id) = albedo { needed_albedos.insert(id); }
                    if let Some(id) = normal { needed_normals.insert(id); }
                    if let Some(id) = emissive { needed_emissives.insert(id); }
                    submeshes.push(mesh_dto(
                        m.positions.clone(),
                        m.uvs.clone(),
                        m.indices.clone(),
                        albedo,
                        normal,
                        emissive,
                        m.bone_indices.clone(),
                        m.bone_weights.clone(),
                    ));
                }
            }
            let dto = AssetMeshesDto {
                asset_tuid: format!("0x{:016X}", asset.tuid),
                name: asset.name.clone(),
                submeshes,
                skeleton: build_skeleton_dto(&asset.skeleton),
                animset_hash: asset.animset_hash.map(|h| format!("0x{:016X}", h)),
                bind_pose_inverse_offset: asset.bind_pose_inverse_offset,
                embedded_animation_count: asset.rfom_anim_offsets.len() as u32,
            };
            let file_rel = format!("mobys/0x{:016X}.json", asset.tuid);
            let path = root.join(&file_rel);
            if let Ok(size_bytes) = write_json(&path, &dto) {
                entries.push(CacheManifestEntry {
                    kind: "moby".into(),
                    tuid: dto.asset_tuid.clone(),
                    name: dto.name.clone(),
                    file: file_rel.clone(),
                    size_bytes,
                });
            }
            moby_done += 1;
            let _ = on_event.send(CacheEvent::Item {
                kind: "moby",
                name: dto.name,
                file: file_rel,
            });
            let _ = on_event.send(CacheEvent::Progress { current: moby_done });
        };
        lunalib::engine_for_layout(layout).read_mobys(level_path, None, &mut on_total, &mut on_moby)
    };
    if let Err(e) = moby_result {
        match layout {
            LevelLayout::Rfom => eprintln!("warn: RFOM moby read failed: {e}"),
            LevelLayout::Tod => return Err(format!("TOD moby read failed: {e}")),
            LevelLayout::V2 => return Err(e.to_string()),
        }
    }
    eprintln!("[cache] {} layout: extracted {} mobys", layout.tag(), moby_done);
    if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        let sample_moby_tuids: Vec<String> = emitted_moby_tuids
            .iter()
            .take(10)
            .map(|t| format!("0x{:016X}", t))
            .collect();
        eprintln!(
            "[moby-match] {} extracted {} mobys; first 10 tuids: {:?}",
            layout.tag(),
            emitted_moby_tuids.len(),
            sample_moby_tuids
        );
    }

    eprintln!("[cache] -> phase ties (layout={})", layout.tag());
    let mut tie_done = 0usize;
    if debug_filter.is_some() {
        eprintln!("[cache] debug mode: skipping ties / details / ufrags / sky / lights / envsamplers");
    } else if matches!(layout, LevelLayout::Tod) {
        if let Err(e) = lunalib::read_tie_assets_old_with_total(
            level_path,
            |total| {
                let _ = on_event.send(CacheEvent::Phase {
                    phase: "ties",
                    total,
                });
            },
            |asset| {
            tie_assets_for_glb.push(asset.clone());

            let submeshes: Vec<_> = asset
                .meshes
                .into_iter()
                .map(|m| {
                    let (albedo, normal, emissive) = resolve_shader_textures(
                        &shaders,
                        &asset.shader_tuids,
                        m.shader_index as usize,
                    );
                    if let Some(id) = albedo { needed_albedos.insert(id); }
                    if let Some(id) = normal { needed_normals.insert(id); }
                    if let Some(id) = emissive { needed_emissives.insert(id); }
                    mesh_dto(
                        m.positions,
                        m.uvs,
                        m.indices,
                        albedo,
                        normal,
                        emissive,
                        Vec::new(),
                        Vec::new(),
                    )
                })
                .collect();
            let dto = AssetMeshesDto {
                asset_tuid: format!("0x{:016X}", asset.tuid),
                name: format!("tie_{:016X}", asset.tuid),
                submeshes,
                skeleton: None,
                animset_hash: None,
                bind_pose_inverse_offset: 0,
                embedded_animation_count: 0,
            };
            let file_rel = format!("ties/0x{:016X}.json", asset.tuid);
            let path = root.join(&file_rel);
            if let Ok(size_bytes) = write_json(&path, &dto) {
                entries.push(CacheManifestEntry {
                    kind: "tie".into(),
                    tuid: dto.asset_tuid.clone(),
                    name: dto.name.clone(),
                    file: file_rel.clone(),
                    size_bytes,
                });
            }
            tie_done += 1;
            let _ = on_event.send(CacheEvent::Item {
                kind: "tie",
                name: dto.name,
                file: file_rel,
            });
            let _ = on_event.send(CacheEvent::Progress { current: tie_done });
        },
        ) {
            eprintln!("warn: TOD tie read failed: {e}");
        }
        eprintln!("[cache] TOD layout: extracted {tie_done} ties");
    } else if matches!(layout, LevelLayout::Rfom) {
        if let Err(e) = lunalib::read_tie_assets_rfom_with_total(
            level_path,
            |total| {
                let _ = on_event.send(CacheEvent::Phase {
                    phase: "ties",
                    total,
                });
            },
            |asset| {
            tie_assets_for_glb.push(asset.clone());

            let submeshes: Vec<_> = asset
                .meshes
                .into_iter()
                .map(|m| {
                    let (albedo, normal, emissive) = resolve_shader_textures(
                        &shaders,
                        &asset.shader_tuids,
                        m.shader_index as usize,
                    );
                    if let Some(id) = albedo { needed_albedos.insert(id); }
                    if let Some(id) = normal { needed_normals.insert(id); }
                    if let Some(id) = emissive { needed_emissives.insert(id); }
                    mesh_dto(
                        m.positions,
                        m.uvs,
                        m.indices,
                        albedo,
                        normal,
                        emissive,
                        Vec::new(),
                        Vec::new(),
                    )
                })
                .collect();
            let dto = AssetMeshesDto {
                asset_tuid: format!("0x{:016X}", asset.tuid),
                name: format!("tie_{:016X}", asset.tuid),
                submeshes,
                skeleton: None,
                animset_hash: None,
                bind_pose_inverse_offset: 0,
                embedded_animation_count: 0,
            };
            let file_rel = format!("ties/0x{:016X}.json", asset.tuid);
            let path = root.join(&file_rel);
            if let Ok(size_bytes) = write_json(&path, &dto) {
                entries.push(CacheManifestEntry {
                    kind: "tie".into(),
                    tuid: dto.asset_tuid.clone(),
                    name: dto.name.clone(),
                    file: file_rel.clone(),
                    size_bytes,
                });
            }
            tie_done += 1;
            let _ = on_event.send(CacheEvent::Item {
                kind: "tie",
                name: dto.name,
                file: file_rel,
            });
            let _ = on_event.send(CacheEvent::Progress { current: tie_done });
        },
        ) {
            eprintln!("warn: RFOM tie read failed: {e}");
        }
        eprintln!("[cache] RFOM layout: extracted {tie_done} ties");

        // RFOM also has DetailCluster (0xB300) — small static props
        // (debris, signs, etc.) that share the V1 Vertex0 stride and
        // meshScale convention with regular ties. Surface them under
        // their own `kind: "detail"` so the cache modal can filter
        // them into a dedicated tab and the viewport can color them
        // distinctly. Geometry pipeline is identical to ties — they
        // still feed `tie_assets_for_glb` so the per-asset GLB +
        // texture resolution work the same.
        let _ = lunalib::read_detail_clusters_rfom(level_path)
            .map(|(detail_assets, _)| {
                let _ = fs::create_dir_all(root.join("details"));
                let mut detail_done = 0usize;
                for asset in detail_assets {
                    tie_assets_for_glb.push(asset.clone());
                    let submeshes: Vec<_> = asset
                        .meshes
                        .into_iter()
                        .map(|m| {
                            let (albedo, normal, emissive) = resolve_shader_textures(
                                &shaders,
                                &asset.shader_tuids,
                                m.shader_index as usize,
                            );
                            if let Some(id) = albedo { needed_albedos.insert(id); }
                            if let Some(id) = normal { needed_normals.insert(id); }
                            if let Some(id) = emissive { needed_emissives.insert(id); }
                            mesh_dto(
                                m.positions,
                                m.uvs,
                                m.indices,
                                albedo,
                                normal,
                                emissive,
                                Vec::new(),
                                Vec::new(),
                            )
                        })
                        .collect();
                    let dto = AssetMeshesDto {
                        asset_tuid: format!("0x{:016X}", asset.tuid),
                        name: format!("detail_{:016X}", asset.tuid),
                        submeshes,
                        skeleton: None,
                        animset_hash: None,
                        bind_pose_inverse_offset: 0,
                        embedded_animation_count: 0,
                    };
                    let file_rel = format!("details/0x{:016X}.json", asset.tuid);
                    let path = root.join(&file_rel);
                    if let Ok(size_bytes) = write_json(&path, &dto) {
                        entries.push(CacheManifestEntry {
                            kind: "detail".into(),
                            tuid: dto.asset_tuid.clone(),
                            name: dto.name.clone(),
                            file: file_rel.clone(),
                            size_bytes,
                        });
                    }
                    detail_done += 1;
                    let _ = on_event.send(CacheEvent::Item {
                        kind: "tie",
                        name: dto.name,
                        file: file_rel,
                    });
                }
                eprintln!("[cache] RFOM layout: extracted {detail_done} detail clusters");
            })
            .map_err(|e| eprintln!("warn: RFOM detail-cluster read failed: {e}"));

        // RFOM Shrubs (0xC700 + 0xC650) — foliage / vegetation. IT calls
        // these "shrubs" and emits them via gpu-instancing; we route the
        // meshes through `tie_assets_for_glb` (same Vertex0 path as
        // details) and expose individual placements as `kind: "shrub"`
        // tie-instances so they show up in the viewport and exports.
        let _ = lunalib::read_shrubs_rfom(level_path)
            .map(|(shrub_assets, _)| {
                let _ = fs::create_dir_all(root.join("shrubs"));
                let mut shrub_done = 0usize;
                for asset in shrub_assets {
                    tie_assets_for_glb.push(asset.clone());
                    let submeshes: Vec<_> = asset
                        .meshes
                        .into_iter()
                        .map(|m| {
                            let (albedo, normal, emissive) = resolve_shader_textures(
                                &shaders,
                                &asset.shader_tuids,
                                m.shader_index as usize,
                            );
                            if let Some(id) = albedo { needed_albedos.insert(id); }
                            if let Some(id) = normal { needed_normals.insert(id); }
                            if let Some(id) = emissive { needed_emissives.insert(id); }
                            mesh_dto(
                                m.positions,
                                m.uvs,
                                m.indices,
                                albedo,
                                normal,
                                emissive,
                                Vec::new(),
                                Vec::new(),
                            )
                        })
                        .collect();
                    let dto = AssetMeshesDto {
                        asset_tuid: format!("0x{:016X}", asset.tuid),
                        name: format!("shrub_{:016X}", asset.tuid),
                        submeshes,
                        skeleton: None,
                        animset_hash: None,
                        bind_pose_inverse_offset: 0,
                        embedded_animation_count: 0,
                    };
                    let file_rel = format!("shrubs/0x{:016X}.json", asset.tuid);
                    let path = root.join(&file_rel);
                    if let Ok(size_bytes) = write_json(&path, &dto) {
                        entries.push(CacheManifestEntry {
                            kind: "shrub".into(),
                            tuid: dto.asset_tuid.clone(),
                            name: dto.name.clone(),
                            file: file_rel.clone(),
                            size_bytes,
                        });
                    }
                    shrub_done += 1;
                    let _ = on_event.send(CacheEvent::Item {
                        kind: "tie",
                        name: dto.name,
                        file: file_rel,
                    });
                }
                eprintln!("[cache] RFOM layout: extracted {shrub_done} shrub meshes");
            })
            .map_err(|e| eprintln!("warn: RFOM shrub read failed: {e}"));

        // RFOM Foliage (0xC200 Foliage + 0x9700 FoliageInstance) — sprite +
        // branch vegetation. Branch-mesh path only for now; sprites are
        // omitted (billboarded quads need viewport-side special handling).
        // NOTE: 0xC200 and 0x9700 used to be mis-routed through
        // read_lights_rfom / read_envsamplers_rfom — both readers now
        // disabled in main.rs's level_layout.
        let _ = lunalib::read_foliage_rfom(level_path)
            .map(|(foliage_assets, _)| {
                let _ = fs::create_dir_all(root.join("foliage"));
                let mut foliage_done = 0usize;
                for asset in foliage_assets {
                    tie_assets_for_glb.push(asset.clone());
                    let submeshes: Vec<_> = asset
                        .meshes
                        .into_iter()
                        .map(|m| {
                            let (albedo, normal, emissive) = resolve_shader_textures(
                                &shaders,
                                &asset.shader_tuids,
                                m.shader_index as usize,
                            );
                            if let Some(id) = albedo { needed_albedos.insert(id); }
                            if let Some(id) = normal { needed_normals.insert(id); }
                            if let Some(id) = emissive { needed_emissives.insert(id); }
                            mesh_dto(
                                m.positions,
                                m.uvs,
                                m.indices,
                                albedo,
                                normal,
                                emissive,
                                Vec::new(),
                                Vec::new(),
                            )
                        })
                        .collect();
                    let dto = AssetMeshesDto {
                        asset_tuid: format!("0x{:016X}", asset.tuid),
                        name: format!("foliage_{:016X}", asset.tuid),
                        submeshes,
                        skeleton: None,
                        animset_hash: None,
                        bind_pose_inverse_offset: 0,
                        embedded_animation_count: 0,
                    };
                    let file_rel = format!("foliage/0x{:016X}.json", asset.tuid);
                    let path = root.join(&file_rel);
                    if let Ok(size_bytes) = write_json(&path, &dto) {
                        entries.push(CacheManifestEntry {
                            kind: "foliage".into(),
                            tuid: dto.asset_tuid.clone(),
                            name: dto.name.clone(),
                            file: file_rel.clone(),
                            size_bytes,
                        });
                    }
                    foliage_done += 1;
                    let _ = on_event.send(CacheEvent::Item {
                        kind: "tie",
                        name: dto.name,
                        file: file_rel,
                    });
                }
                eprintln!("[cache] RFOM layout: extracted {foliage_done} foliage meshes");
            })
            .map_err(|e| eprintln!("warn: RFOM foliage read failed: {e}"));

        match lunalib::read_skybox_rfom(level_path) {
            Ok(Some(sky)) => {
                let sky_dir = root.join("skybox");
                let _ = fs::create_dir_all(&sky_dir);
                let glb = lunalib::write_skybox_glb(&sky)
                    .unwrap_or_default();
                let obj = lunalib::write_skybox_obj(&sky);
                let ply = lunalib::write_skybox_ply(&sky);
                let json = serde_json::json!({
                    "vertex_count": sky.vertices.len(),
                    "triangle_count": sky.indices.len() / 3,
                    "aabb_min": sky.aabb_min,
                    "aabb_max": sky.aabb_max,
                    "texture_offset": sky.texture_offset,
                });

                let glb_path = sky_dir.join("sky.glb");
                let obj_path = sky_dir.join("sky.obj");
                let ply_path = sky_dir.join("sky.ply");
                let json_path = sky_dir.join("sky.json");
                let _ = fs::write(&glb_path, &glb);
                let _ = fs::write(&obj_path, obj.as_bytes());
                let _ = fs::write(&ply_path, ply.as_bytes());
                let _ = fs::write(
                    &json_path,
                    serde_json::to_vec_pretty(&json).unwrap_or_default(),
                );

                let glb_size = glb.len() as u64;
                entries.push(CacheManifestEntry {
                    kind: "sky".into(),
                    tuid: format!("0x{:08X}", sky.texture_offset.unwrap_or(0)),
                    name: "sky_dome".into(),
                    file: "skybox/sky.glb".into(),
                    size_bytes: glb_size,
                });
                eprintln!(
                    "[cache] RFOM layout: wrote skybox GLB ({} verts, {} tris, {} bytes)",
                    sky.vertices.len(),
                    sky.indices.len() / 3,
                    glb_size
                );
            }
            Ok(None) => eprintln!("[cache] RFOM layout: no skybox sections found"),
            Err(e) => eprintln!("warn: RFOM skybox read failed: {e}"),
        }

        // Gameplay placements (ps3gameplay.dat) — moby instance positions
        // plus our raw-byte probe of GameplayInstances.other[6] (only when
        // RECHIMERA_LOG_PROBES=1). Result discarded here — the level_layout
        // Tauri command reads it again on demand for the viewport. We call
        // it during cache extraction so the [rfom-gp]/[rfom-gp-other] logs
        // fire on every full re-extract, not only when the level is opened.
        match lunalib::read_gameplay_rfom(level_path) {
            Ok(gp) => {
                let placement_count: usize =
                    gp.regions.iter().map(|r| r.moby_instances.len()).sum();
                eprintln!(
                    "[cache] RFOM layout: parsed {} gameplay placement(s) from ps3gameplay.dat",
                    placement_count
                );
            }
            Err(e) => eprintln!("warn: RFOM gameplay read failed: {e}"),
        }
    } else {
    let tie_phase_emit = |total: usize| {
        let _ = on_event.send(CacheEvent::Phase {
            phase: "ties",
            total,
        });
    };
    eprintln!("[cache] -> V2 tie reader starting");
    read_tie_assets_with_total(
        level_path,
        None,
        tie_phase_emit,
        |asset| {

            tie_assets_for_glb.push(asset.clone());

            let mut tie_meshes_total = 0usize;
            let mut tie_meshes_with_albedo = 0usize;
            let mut tie_first_st: Option<u64> = None;
            let submeshes: Vec<_> = asset
                .meshes
                .into_iter()
                .map(|m| {
                    tie_meshes_total += 1;
                    if tie_first_st.is_none() {
                        tie_first_st = asset.shader_tuids
                            .get(m.shader_index as usize)
                            .copied();
                    }
                    let (albedo, normal, emissive) = resolve_shader_textures(
                        &shaders,
                        &asset.shader_tuids,
                        m.shader_index as usize,
                    );
                    if albedo.is_some() { tie_meshes_with_albedo += 1; }
                    if let Some(id) = albedo { needed_albedos.insert(id); }
                    if let Some(id) = normal { needed_normals.insert(id); }
                    if let Some(id) = emissive { needed_emissives.insert(id); }
                    mesh_dto(
                        m.positions,
                        m.uvs,
                        m.indices,
                        albedo,
                        normal,
                        emissive,
                        Vec::new(),
                        Vec::new(),
                    )
                })
                .collect();
            if tie_meshes_total > 0 && tie_meshes_with_albedo == 0 {
                use std::sync::atomic::{AtomicUsize, Ordering};
                static TIE_NO_ALBEDO_FIRED: AtomicUsize = AtomicUsize::new(0);
                let n = TIE_NO_ALBEDO_FIRED.fetch_add(1, Ordering::Relaxed);
                if n < 4 {
                    let st_label = tie_first_st
                        .map(|s| format!("0x{:016X}", s))
                        .unwrap_or_else(|| "(none)".to_string());
                    let in_shaders = tie_first_st
                        .map(|s| shaders.contains_key(&s))
                        .unwrap_or(false);
                    eprintln!(
                        "warn: tie 0x{:016X} resolved 0/{} meshes to a shader. first shader_tuid={} (in shaders map: {}). shader_tuids.len={}, shaders.len={}",
                        asset.tuid,
                        tie_meshes_total,
                        st_label,
                        in_shaders,
                        asset.shader_tuids.len(),
                        shaders.len(),
                    );
                }
            }
            let dto = AssetMeshesDto {
                asset_tuid: format!("0x{:016X}", asset.tuid),
                name: String::new(),
                submeshes,
                skeleton: None,
                animset_hash: None,
                bind_pose_inverse_offset: 0,
                embedded_animation_count: 0,
            };
            let file_rel = format!("ties/0x{:016X}.json", asset.tuid);
            let path = root.join(&file_rel);
            if let Ok(size_bytes) = write_json(&path, &dto) {
                entries.push(CacheManifestEntry {
                    kind: "tie".into(),
                    tuid: dto.asset_tuid.clone(),
                    name: dto.name.clone(),
                    file: file_rel.clone(),
                    size_bytes,
                });
            }
            tie_done += 1;
            let _ = on_event.send(CacheEvent::Item {
                kind: "tie",
                name: dto.name,
                file: file_rel,
            });
            let _ = on_event.send(CacheEvent::Progress { current: tie_done });
        },
    )
    .map_err(|e| e.to_string())?;
    eprintln!("[cache] V2 tie reader done — {} ties", tie_done);
    }

    eprintln!("[cache] -> phase ufrags");
    let zones: Vec<Zone> = if debug_filter.is_some() {
        Vec::new()
    } else {
        match lunalib::engine_for_layout(layout).read_zones(level_path) {
            Ok(z) => {
                eprintln!(
                    "[cache] {} layout: read {} zone(s) with {} tie instances, {} ufrags",
                    layout.tag(),
                    z.len(),
                    z.iter().map(|x| x.tie_instances.len()).sum::<usize>(),
                    z.iter().map(|x| x.ufrags.len()).sum::<usize>()
                );
                z
            }
            Err(e) => {
                eprintln!(
                    "warn: {} zone read failed ({e}); skipping ufrag phase",
                    layout.tag()
                );
                Vec::new()
            }
        }
    };

    let mut total_ufrags = 0usize;
    for z in &zones {
        for u in &z.ufrags {
            if u.positions.is_empty() || u.indices.is_empty() {
                continue;
            }
            total_ufrags += 1;
        }
    }
    let _ = on_event.send(CacheEvent::Phase {
        phase: "ufrags",
        total: total_ufrags,
    });
    fs::create_dir_all(root.join("ufrags")).map_err(|e| format!("create ufrags dir: {e}"))?;
    let mut ufrag_done = 0usize;
    for zone in zones {
        let zone_tuid_hex = format!("0x{:016X}", zone.tuid);
        let shader_tuids = zone.ufrag_shader_tuids.clone();
        for u in zone.ufrags {
            if u.positions.is_empty() || u.indices.is_empty() {
                continue;
            }
            let UFrag {
                tuid,
                shader_index,
                position,
                positions,
                uvs,
                indices,
                ..
            } = u;
            let shader_info = shader_tuids
                .get(shader_index as usize)
                .and_then(|st| shaders.get(st));
            let albedo = shader_info.and_then(|s| s.albedo_tex_id);
            let normal = shader_info.and_then(|s| s.normal_tex_id);
            let emissive = shader_info.and_then(|s| s.expensive_tex_id);
            if let Some(id) = albedo { needed_albedos.insert(id); }
            if let Some(id) = normal { needed_normals.insert(id); }
            if let Some(id) = emissive { needed_emissives.insert(id); }
            let dto = UFragMeshDto {
                tuid: format!("0x{:016X}", tuid),
                zone_tuid: zone_tuid_hex.clone(),
                position,
                mesh: mesh_dto(positions, uvs, indices, albedo, normal, emissive, Vec::new(), Vec::new()),
            };
            let file_rel = format!("ufrags/{}.json", dto.tuid);
            let path = root.join(&file_rel);
            let size_bytes = write_json(&path, &dto).unwrap_or(0);
            entries.push(CacheManifestEntry {
                kind: "ufrag".into(),
                tuid: dto.tuid,
                name: dto.zone_tuid.clone(),
                file: file_rel.clone(),
                size_bytes,
            });
            ufrag_done += 1;
            let _ = on_event.send(CacheEvent::Item {
                kind: "ufrag",
                name: dto.zone_tuid,
                file: file_rel,
            });
            let _ = on_event.send(CacheEvent::Progress { current: ufrag_done });
        }
    }

    // Cubemap extraction (V2 only). Reads cubemaps.dat per the
    // ResourceCubemap (0x1D200) entries in assetlookup.dat, decodes
    // 6 faces × base mip → 6 PNGs per cubemap, emits a small JSON
    // descriptor so the viewport can wire them into a CubeTexture.
    if matches!(layout, LevelLayout::V2) {
        let cubemaps = lunalib::read_cubemaps(level_path).unwrap_or_default();
        if !cubemaps.is_empty() {
            let cube_dir = root.join("cubemaps");
            fs::create_dir_all(&cube_dir)
                .map_err(|e| format!("create cubemaps dir: {e}"))?;
            for cm in cubemaps {
                let hash_hex = format!("{:016X}", cm.hash);
                let sub = cube_dir.join(&hash_hex);
                fs::create_dir_all(&sub)
                    .map_err(|e| format!("create cubemap subdir: {e}"))?;
                let mut face_paths: Vec<String> = Vec::with_capacity(6);
                for (i, face) in cm.faces.iter().enumerate() {
                    if face.rgba.is_empty() {
                        continue;
                    }
                    let png = lunalib::texture::encode_png(
                        &face.rgba,
                        face.width,
                        face.height,
                    );
                    if png.is_empty() {
                        continue;
                    }
                    let rel = format!("cubemaps/{}/face_{}.png", hash_hex, i);
                    let path = root.join(&rel);
                    if fs::write(&path, &png).is_err() {
                        continue;
                    }
                    face_paths.push(rel);
                }
                if face_paths.len() != 6 {
                    eprintln!(
                        "[cubemap] 0x{}: emitted {} faces; skipping descriptor",
                        hash_hex,
                        face_paths.len()
                    );
                    continue;
                }
                let dto = serde_json::json!({
                    "tuid": format!("0x{}", hash_hex),
                    "width": cm.width,
                    "height": cm.height,
                    "faces": face_paths,
                });
                let file_rel = format!("cubemaps/{}.json", hash_hex);
                let path = root.join(&file_rel);
                let json_bytes = match serde_json::to_vec(&dto) {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                if fs::write(&path, &json_bytes).is_err() {
                    continue;
                }
                entries.push(CacheManifestEntry {
                    kind: "cubemap".into(),
                    tuid: format!("0x{}", hash_hex),
                    name: String::new(),
                    file: file_rel.clone(),
                    size_bytes: json_bytes.len() as u64,
                });
                let _ = on_event.send(CacheEvent::Item {
                    kind: "cubemap",
                    name: format!("0x{}", hash_hex),
                    file: file_rel,
                });
            }
        }
    }

    // Union of all roles — the actual set we decode from source files once.
    let needed_union: HashSet<u32> = needed_albedos
        .iter()
        .chain(needed_normals.iter())
        .chain(needed_emissives.iter())
        .copied()
        .collect();
    let needed_ids: Vec<u32> = needed_union.iter().copied().collect();
    // Treat "0", "false", "no", "" as off — `.is_ok()` alone would
    // make any value (including "0") enable the skip.
    let skip_textures = std::env::var("RECHIMERA_SKIP_TEXTURES")
        .ok()
        .map(|v| {
            let lower = v.to_ascii_lowercase();
            !lower.is_empty() && lower != "0" && lower != "false" && lower != "no"
        })
        .unwrap_or(false);

    // Single decode pass from source (V2 / RFOM / TOD diverge only here).
    // Categorised writes happen below in 3 progress phases.
    let pngs: Vec<(u32, Vec<u8>)> = if skip_textures {
        // Fast iteration mode for the anim probe — skip the slow decode and
        // just re-use whatever PNG files are already on disk in the cache
        // textures/ directory from a previous extraction.
        let tex_dir = root.join("textures");
        let mut reused: Vec<(u32, Vec<u8>)> = Vec::new();
        if tex_dir.is_dir() {
            let needed_set: HashSet<u32> = needed_ids.iter().copied().collect();
            for entry in fs::read_dir(&tex_dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(id) = stem.parse::<u32>() else { continue };
                if !needed_set.is_empty() && !needed_set.contains(&id) {
                    continue;
                }
                if let Ok(bytes) = fs::read(&path) {
                    reused.push((id, bytes));
                }
            }
        }
        eprintln!(
            "[cache] RECHIMERA_SKIP_TEXTURES=1 — skipping texture decode, reused {} cached PNGs (needed={})",
            reused.len(),
            needed_ids.len()
        );
        reused
    } else { match layout {
        LevelLayout::V2 => {
            eprintln!(
                "[cache] -> V2 bulk_extract_pngs: needed={} textures",
                needed_ids.len()
            );
            let mut r = lunalib::bulk_extract_pngs(level_path, Some(&needed_ids), TEXTURE_MAX_DIM)
                .map_err(|e| e.to_string())?;

            // Global-PSARC fallback. R2 (and presumably R3 / ACiT) store
            // shared art (weapons, common characters, UI) in sibling
            // `packed/game/global_*/built/tuids/<tuid>/{header,texel}.dat`
            // folders, NOT in the level's own textures.dat / highmips.dat.
            // Anything still missing after the in-level pass we re-try
            // there. The global folders are optional — when the user only
            // extracted the level PSARC (or the game ships everything per-
            // level), `discover_and_index` returns an empty map and this
            // block is a no-op with no log noise. Per-game/mod variability
            // is expected; absent globals must not block the cache build.
            let extracted_ids: HashSet<u32> = r.iter().map(|(id, _)| *id).collect();
            let still_missing: Vec<u32> = needed_ids
                .iter()
                .copied()
                .filter(|id| !extracted_ids.contains(id))
                .collect();
            if !still_missing.is_empty() {
                let index = lunalib::texture_global::discover_and_index(level_path);
                if !index.is_empty() {
                    let mut recovered = 0usize;
                    let mut attempted = 0usize;
                    for id in &still_missing {
                        let Some(entry) = index.get(id) else { continue };
                        attempted += 1;
                        match lunalib::texture_global::load_global_texture_png(
                            &entry.folder,
                            TEXTURE_MAX_DIM,
                        ) {
                            Ok(Some(png)) => {
                                r.push((*id, png));
                                recovered += 1;
                            }
                            Ok(None) => {
                                eprintln!(
                                    "[global-tex] tex 0x{:08X} at {} returned None — format unsupported or empty texel",
                                    id,
                                    entry.folder.display(),
                                );
                            }
                            Err(e) => {
                                eprintln!(
                                    "[global-tex] tex 0x{:08X} at {} decode failed: {}",
                                    id,
                                    entry.folder.display(),
                                    e,
                                );
                            }
                        }
                    }
                    eprintln!(
                        "[global-tex] recovered {} / {} missing textures via global fallback ({} candidates in index, {} attempted)",
                        recovered,
                        still_missing.len(),
                        index.len(),
                        attempted,
                    );
                }
            }

            // Patch overlay fallback. R2 PSN installs ship DLC content
            // as `data/patch_NN.psarc`; `r2_extract_patches` unpacks them
            // into `<usrdir>/built/patch/{textures,highmips,mobys,shaders,…}.dat`.
            // That overlay carries the texture set for DLC characters
            // (Rachel head, Female Soldier head, Grim/Ravager/Cloven body
            // skins, blackops2/ranger2 variants) — a base-disc level can
            // reference any of these IDs and only the overlay has the
            // bytes. Treated as a fourth tier between globals and the
            // sibling-level scan because the overlay is the authoritative
            // DLC source (more specific than scrabbling through siblings).
            let after_global: HashSet<u32> = r.iter().map(|(id, _)| *id).collect();
            let still_missing_post_global: Vec<u32> = needed_ids
                .iter()
                .copied()
                .filter(|id| !after_global.contains(id))
                .collect();
            if !still_missing_post_global.is_empty() {
                if let Some(overlay) = lunalib::texture_global::find_patch_overlay(level_path) {
                    match lunalib::bulk_extract_pngs(
                        &overlay,
                        Some(&still_missing_post_global),
                        TEXTURE_MAX_DIM,
                    ) {
                        Ok(recovered) => {
                            let n = recovered.len();
                            for (id, png) in recovered {
                                r.push((id, png));
                            }
                            if n > 0 {
                                eprintln!(
                                    "[patch-overlay-tex] recovered {} / {} missing textures from {}",
                                    n,
                                    still_missing_post_global.len(),
                                    overlay.display(),
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[patch-overlay-tex] overlay {} failed: {}",
                                overlay.display(),
                                e,
                            );
                        }
                    }
                }
            }

            // Cross-level fallback. R2 shares art (lobby UI, coop/MP
            // overlays, weapon variants) across level PSARCs, not the
            // globals — so meshes in this level can reference IDs whose
            // bytes only exist in another already-extracted level. Recompute
            // still-missing AFTER the global pass and try each sibling
            // level's textures.dat / highmips.dat as additional sources.
            // Each sibling call is narrowed by the residual missing set so
            // it only decodes what we actually need.
            let after_global: HashSet<u32> = r.iter().map(|(id, _)| *id).collect();
            let still_missing: Vec<u32> = needed_ids
                .iter()
                .copied()
                .filter(|id| !after_global.contains(id))
                .collect();
            if !still_missing.is_empty() {
                let siblings = lunalib::texture_global::find_sibling_extracted_levels(level_path);
                if !siblings.is_empty() {
                    let mut total_recovered = 0usize;
                    let mut remaining: Vec<u32> = still_missing.clone();
                    for sibling in &siblings {
                        if remaining.is_empty() {
                            break;
                        }
                        match lunalib::bulk_extract_pngs(
                            sibling,
                            Some(&remaining),
                            TEXTURE_MAX_DIM,
                        ) {
                            Ok(recovered) => {
                                let mut got: HashSet<u32> = HashSet::new();
                                for (id, png) in recovered {
                                    r.push((id, png));
                                    got.insert(id);
                                    total_recovered += 1;
                                }
                                if !got.is_empty() {
                                    eprintln!(
                                        "[cross-level-tex] recovered {} textures from {}",
                                        got.len(),
                                        sibling.display(),
                                    );
                                }
                                remaining.retain(|id| !got.contains(id));
                            }
                            Err(e) => {
                                eprintln!(
                                    "[cross-level-tex] sibling {} failed: {}",
                                    sibling.display(),
                                    e,
                                );
                            }
                        }
                    }
                    eprintln!(
                        "[cross-level-tex] total recovered {} / {} via cross-level fallback ({} siblings tried, {} still unrecovered)",
                        total_recovered,
                        still_missing.len(),
                        siblings.len(),
                        remaining.len(),
                    );
                }
            }

            eprintln!("[cache] V2 textures done — encoded={} PNGs", r.len());
            r
        }
        LevelLayout::Tod => match lunalib::read_textures_old(level_path) {
            Ok(textures) => {
                use rayon::prelude::*;
                let needed: HashSet<u32> = needed_ids.iter().copied().collect();
                let mut out: Vec<(u32, Vec<u8>)> = textures
                    .par_iter()
                    .filter(|t| needed.contains(&t.id))
                    .filter_map(|t| {
                        lunalib::texture_to_png(t).map(|png| {
                            let resized = lunalib::downsample_png_to(&png, TEXTURE_MAX_DIM)
                                .unwrap_or(png);
                            (t.id, resized)
                        })
                    })
                    .collect();
                out.sort_by_key(|(id, _)| *id);
                eprintln!(
                    "[cache] TOD layout: encoded {} / {} requested textures",
                    out.len(),
                    needed_ids.len()
                );
                out
            }
            Err(e) => {
                eprintln!("warn: TOD texture read failed ({e}) — emitting no textures");
                Vec::new()
            }
        },
        LevelLayout::Rfom => match lunalib::read_textures_rfom(level_path) {
            Ok(textures) => {
                use rayon::prelude::*;
                let needed: HashSet<u32> = needed_ids.iter().copied().collect();
                let mut out: Vec<(u32, Vec<u8>)> = textures
                    .par_iter()
                    .filter(|t| needed.contains(&t.id))
                    .filter_map(|t| {
                        lunalib::texture_rfom_to_png(t).map(|png| {
                            let resized = lunalib::downsample_png_to(&png, TEXTURE_MAX_DIM)
                                .unwrap_or(png);
                            (t.id, resized)
                        })
                    })
                    .collect();
                out.sort_by_key(|(id, _)| *id);
                eprintln!(
                    "[cache] RFOM layout: encoded {} / {} requested textures",
                    out.len(),
                    needed_ids.len()
                );
                out
            }
            Err(e) => {
                eprintln!("warn: RFOM texture read failed ({e}) — emitting no textures");
                Vec::new()
            }
        },
    }};

    // Write in 3 progress phases — emissions first, then normal maps, then
    // albedos LAST. Albedos drive the visible look of every mesh, so loading
    // them last means the viewport "completes" at the end of the run instead
    // of starting full-color and then flickering as the duller channels
    // (normal / emission) arrive. (The phase names "materials"/"normalmaps"/
    // "textures" remain as-is — they're the typed union shared with the
    // frontend; only the iteration order changes here.)
    // Each unique texture ID is written exactly once — IDs present in multiple
    // roles land in the first phase they appear in (so an emissive that's
    // also someone's albedo will be written during the emissions phase, not
    // duplicated in the albedos phase).
    let mut pngs_map: HashMap<u32, Vec<u8>> = pngs.into_iter().collect();
    let mut texture_pngs: HashMap<u32, Vec<u8>> = HashMap::with_capacity(pngs_map.len());
    let mut written: HashSet<u32> = HashSet::new();
    let phases: [(&'static str, &HashSet<u32>); 3] = [
        ("textures", &needed_emissives),
        ("normalmaps", &needed_normals),
        ("materials", &needed_albedos),
    ];
    for (phase_name, ids) in phases.iter() {
        let pending: Vec<u32> = ids
            .iter()
            .filter(|id| !written.contains(id))
            .copied()
            .collect();
        let _ = on_event.send(CacheEvent::Phase {
            phase: phase_name,
            total: pending.len(),
        });
        let progress_every = (pending.len() / 50).max(1);
        let mut done = 0usize;
        for id in pending {
            written.insert(id);
            if let Some(png) = pngs_map.remove(&id) {
                let file_rel = format!("textures/{id}.png");
                let path = root.join(&file_rel);
                let size_bytes = png.len() as u64;
                if fs::write(&path, &png).is_ok() {
                    entries.push(CacheManifestEntry {
                        kind: "texture".into(),
                        tuid: id.to_string(),
                        name: String::new(),
                        file: file_rel.clone(),
                        size_bytes,
                    });
                }
                texture_pngs.insert(id, png);
            }
            done += 1;
            if done % progress_every == 0 {
                let _ = on_event.send(CacheEvent::Progress { current: done });
            }
        }
        let _ = on_event.send(CacheEvent::Progress { current: done });
    }

    let _ = on_event.send(CacheEvent::Phase {
        phase: "mobys",
        total: moby_assets_for_glb.len() + tie_assets_for_glb.len(),
    });
    let mut animset_memo: HashMap<(u64, u64), Vec<DecodedClip>> = HashMap::new();
    let mut animset_memo_order: std::collections::VecDeque<(u64, u64)> = Default::default();
    let mut animset_memo_hits = 0usize;
    let mut animset_memo_misses = 0usize;
    let mut glb_done = 0usize;
    for asset in moby_assets_for_glb.into_iter() {
        let trans_shift = asset
            .skeleton
            .as_ref()
            .map(|s| s.translation_shift)
            .unwrap_or(0);
        let scale_shift_v = asset
            .skeleton
            .as_ref()
            .map(|s| s.scale_shift)
            .unwrap_or(0);
        let mconv = profile.matrix_convention();
        let pos_scale = mconv.shift_scale(trans_shift);
        let scale_scale = mconv.shift_scale(scale_shift_v);

        // Per-moby shader / texture diagnostic (self-gated on
        // RECHIMERA_DEBUG_MOBY match — silent for unfiltered runs).
        // Cross-references each submesh's shader_index against the encoded
        // PNG set so MISSING textures show up alongside the submeshes that
        // actually need them.
        dump_moby_shader_textures(&asset, &shaders, &texture_pngs);

        let clips: Vec<DecodedClip> = if !asset.rfom_anim_offsets.is_empty() {
            match asset.skeleton.as_ref() {
                Some(skel) => {
                    lunalib::skeleton::dump_skeleton_bind(asset.tuid, skel);
                    let key = (
                        anim_offsets_key(&asset.rfom_anim_offsets),
                        skeleton_anim_fingerprint(Some(skel)),
                    );
                    let decoded = if let Some(cached) = animset_memo.get(&key) {
                        animset_memo_hits += 1;
                        cached.clone()
                    } else {
                        animset_memo_misses += 1;
                        let main_dat = match layout {
                            LevelLayout::Tod => "main.dat",
                            _ => "ps3levelmain.dat",
                        };
                        let d = decode_clips_for_moby_inline(
                            level_path,
                            main_dat,
                            &asset.rfom_anim_offsets,
                            pos_scale,
                            scale_scale,
                            skel,
                            layout,
                            asset.tuid,
                            profile,
                        );
                        animset_memo.insert(key, d.clone());
                        animset_memo_order.push_back(key);
                        if animset_memo_order.len() > ANIMSET_MEMO_CAP {
                            if let Some(old) = animset_memo_order.pop_front() {
                                animset_memo.remove(&old);
                            }
                        }
                        d
                    };
                    if matches!(layout, LevelLayout::Rfom)
                        && decoded.len() != asset.rfom_anim_offsets.len()
                    {
                        eprintln!(
                            "warn: [rfom-anim] moby_{:04X}: {} offsets but only {} clips decoded (skel_bones={})",
                            asset.tuid,
                            asset.rfom_anim_offsets.len(),
                            decoded.len(),
                            skel.bones.len()
                        );
                    }
                    decoded
                }
                None => {
                    if matches!(layout, LevelLayout::Rfom)
                        && !asset.rfom_anim_offsets.is_empty()
                    {
                        eprintln!(
                            "warn: [rfom-anim] moby_{:04X}: {} offsets but no skeleton — skipping",
                            asset.tuid,
                            asset.rfom_anim_offsets.len()
                        );
                    }
                    Vec::new()
                }
            }
        } else {
            match (
                asset.animset_hash,
                animset_index.as_ref(),
                animsets_file.as_mut(),
            ) {
                (Some(hash), Some(idx), Some(file)) => {
                    let key = (hash, skeleton_anim_fingerprint(asset.skeleton.as_ref()));
                    if let Some(cached) = animset_memo.get(&key) {
                        animset_memo_hits += 1;
                        cached.clone()
                    } else {
                        animset_memo_misses += 1;
                        let sb = asset
                            .skeleton
                            .as_ref()
                            .map(|s| s.bones.len() as u16)
                            .unwrap_or(0);
                        let d = decode_clips_for_moby(
                            level_path,
                            idx,
                            file,
                            hash,
                            pos_scale,
                            scale_scale,
                            sb,
                            asset.skeleton.as_ref(),
                            profile,
                        );
                        animset_memo.insert(key, d.clone());
                        animset_memo_order.push_back(key);
                        if animset_memo_order.len() > ANIMSET_MEMO_CAP {
                            if let Some(old) = animset_memo_order.pop_front() {
                                animset_memo.remove(&old);
                            }
                        }
                        d
                    }
                }
                _ => Vec::new(),
            }
        };
        const PROBE_TUID: u64 = 0x079D;
        let probe = asset.tuid == PROBE_TUID;
        if probe {
            eprintln!(
                "[probe-moby-{:04X}] (glb-stage) bangles={} primitives={} skeleton_bones={} clip_count={} anim_offsets={} pos_scale={pos_scale} scale_scale={scale_scale}",
                PROBE_TUID,
                asset.bangles.len(),
                asset.bangles.iter().map(|b| b.meshes.len()).sum::<usize>(),
                asset.skeleton.as_ref().map(|s| s.bones.len()).unwrap_or(0),
                clips.len(),
                asset.rfom_anim_offsets.len()
            );
            for (bi, bangle) in asset.bangles.iter().enumerate() {
                for (mi, m) in bangle.meshes.iter().enumerate() {
                    eprintln!(
                        "[probe-moby-{:04X}]   bangle[{bi}].mesh[{mi}] verts={} idx={} stride={} mat={} bone_idx_len={} bone_w_len={}",
                        PROBE_TUID,
                        m.vertex_count, m.index_count, m.vertex_stride, m.shader_index,
                        m.bone_indices.len(), m.bone_weights.len()
                    );
                }
            }
            for (ci, clip) in clips.iter().enumerate().take(3) {
                let mut animated_bones = 0usize;
                let mut total_rot = 0usize;
                let mut total_trans = 0usize;
                for b in &clip.bones {
                    if b.rotation_animated || b.translation_animated || b.scale_animated {
                        animated_bones += 1;
                    }
                    total_rot += b.rotations.len();
                    total_trans += b.translations.len();
                }
                eprintln!(
                    "[probe-moby-{:04X}]   clip[{ci}] name='{}' frames={} fps={} loop={} bones={} animated_bones={} total_rot_floats={} total_trans_floats={}",
                    PROBE_TUID,
                    clip.name, clip.num_frames, clip.frame_rate, clip.looping,
                    clip.bones.len(), animated_bones, total_rot, total_trans
                );
            }
        }
        let has_any_mesh = asset
            .bangles
            .iter()
            .any(|b| b.meshes.iter().any(|m| m.vertex_count > 0));
        if !has_any_mesh {
            if probe {
                eprintln!(
                    "[probe-moby-{:04X}] no geometry — skipping GLB write",
                    PROBE_TUID
                );
            }
            glb_done += 1;
            let _ = on_event.send(CacheEvent::Progress { current: glb_done });
            continue;
        }
        let patched_shaders = if profile.game == Some(lunalib::Game::R3) {
            substitute_absent_albedos(&shaders, &asset, &texture_pngs)
        } else {
            None
        };
        let shaders_for_glb = patched_shaders.as_ref().unwrap_or(&shaders);
        match lunalib::write_moby_glb_full(&asset, &clips, shaders_for_glb, &texture_pngs) {
            Ok(glb_bytes) => {
                if probe {
                    eprintln!(
                        "[probe-moby-{:04X}] GLB write OK — {} bytes",
                        PROBE_TUID,
                        glb_bytes.len()
                    );
                }
                let glb_rel = format!("mobys/0x{:016X}.glb", asset.tuid);
                let glb_path = root.join(&glb_rel);
                if fs::write(&glb_path, &glb_bytes).is_ok() {
                    entries.push(CacheManifestEntry {
                        kind: "moby_glb".into(),
                        tuid: format!("0x{:016X}", asset.tuid),
                        name: asset.name.clone(),
                        file: glb_rel,
                        size_bytes: glb_bytes.len() as u64,
                    });
                } else if probe {
                    eprintln!("[probe-moby-{:04X}] GLB fs::write FAILED", PROBE_TUID);
                }
            }
            Err(e) => {
                eprintln!(
                    "warn: GLB export failed for moby 0x{:016X}: {e}",
                    asset.tuid
                );
                if probe {
                    eprintln!("[probe-moby-{:04X}] GLB write FAILED: {e}", PROBE_TUID);
                }
            }
        }
        glb_done += 1;
        let _ = on_event.send(CacheEvent::Progress { current: glb_done });
    }
    if animset_memo_hits + animset_memo_misses > 0 {
        eprintln!(
            "[anim-memo] {} animset decodes, {} reused from memo",
            animset_memo_misses, animset_memo_hits
        );
    }
    drop(animset_memo);
    drop(animset_memo_order);

    fs::create_dir_all(root.join("ties")).map_err(|e| format!("create ties dir: {e}"))?;
    for tie in tie_assets_for_glb.into_iter() {
        let synth = tie_as_moby(&tie);
        match lunalib::write_moby_glb_full(&synth, &[], &shaders, &texture_pngs) {
            Ok(glb_bytes) => {
                let glb_rel = format!("ties/0x{:016X}.glb", tie.tuid);
                let glb_path = root.join(&glb_rel);
                if fs::write(&glb_path, &glb_bytes).is_ok() {
                    entries.push(CacheManifestEntry {
                        kind: "tie_glb".into(),
                        tuid: format!("0x{:016X}", tie.tuid),
                        name: synth.name.clone(),
                        file: glb_rel,
                        size_bytes: glb_bytes.len() as u64,
                    });
                }
            }
            Err(e) => {
                eprintln!(
                    "warn: GLB export failed for tie 0x{:016X}: {e}",
                    tie.tuid
                );
            }
        }
        glb_done += 1;
        let _ = on_event.send(CacheEvent::Progress { current: glb_done });
    }

    let manifest = CacheManifest {
        version: MANIFEST_VERSION,
        folder: folder.to_string(),
        entries,
        source_mtimes: snapshot_source_mtimes(level_path),
        complete: true,
    };
    write_json(&manifest_path, &manifest)?;
    Ok(manifest.entries.len())
}

fn count_files_in(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .count()
}

#[tauri::command]
pub fn cache_status(folder: String) -> Result<CacheStatus, String> {
    let root = cache_root(&folder);
    let manifest_path = root.join(MANIFEST_NAME);

    if !manifest_path.is_file() {

        if root.is_dir() {
            let mobys = count_files_in(&root.join("mobys"));
            let ties = count_files_in(&root.join("ties"));
            let textures = count_files_in(&root.join("textures"));
            let total = mobys + ties + textures;
            if total > 0 {
                return Ok(CacheStatus {
                    exists: true,
                    folder,
                    cache_path: root.to_string_lossy().into_owned(),
                    entry_count: total,
                    mobys,
                    ties,
                    textures,
                    stale: true,
                    incomplete: true,
                });
            }
        }
        return Ok(CacheStatus {
            exists: false,
            folder,
            cache_path: root.to_string_lossy().into_owned(),
            entry_count: 0,
            mobys: 0,
            ties: 0,
            textures: 0,
            stale: false,
            incomplete: false,
        });
    }

    let bytes = match fs::read(&manifest_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cache_status: read manifest failed: {e}");
            return cache_status_from_dir(&folder, &root, true, true);
        }
    };
    let manifest: CacheManifest = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("cache_status: parse manifest failed: {e}");
            return cache_status_from_dir(&folder, &root, true, true);
        }
    };
    let mut mobys = 0usize;
    let mut ties = 0usize;
    let mut textures = 0usize;
    for entry in &manifest.entries {
        match entry.kind.as_str() {
            "moby" => mobys += 1,
            "tie" => ties += 1,
            "texture" => textures += 1,
            _ => {}
        }
    }
    let stale = is_cache_stale(Path::new(&folder), &manifest.source_mtimes);

    // Force a fresh extraction on every open when a debug filter or the
    // explicit force flag is set. Lets the debug loop (set RECHIMERA_DEBUG_MOBY,
    // re-open, read logs) skip the manual `_rechimera_cache` wipe — the cache
    // is reported incomplete so the open flow rebuilds it.
    let force = force_reextract_env();
    let incomplete = !manifest.complete || force;
    Ok(CacheStatus {
        exists: true,
        folder,
        cache_path: root.to_string_lossy().into_owned(),
        entry_count: manifest.entries.len(),
        mobys,
        ties,
        textures,
        stale: stale || incomplete,
        incomplete,
    })
}

fn force_reextract_env() -> bool {
    std::env::var("RECHIMERA_FORCE_REEXTRACT").is_ok()
        || std::env::var("RECHIMERA_DEBUG_MOBY").is_ok()
}

fn cache_status_from_dir(
    folder: &str,
    root: &Path,
    stale: bool,
    incomplete: bool,
) -> Result<CacheStatus, String> {
    let mobys = count_files_in(&root.join("mobys"));
    let ties = count_files_in(&root.join("ties"));
    let textures = count_files_in(&root.join("textures"));
    let total = mobys + ties + textures;
    Ok(CacheStatus {
        exists: total > 0,
        folder: folder.to_owned(),
        cache_path: root.to_string_lossy().into_owned(),
        entry_count: total,
        mobys,
        ties,
        textures,
        stale,
        incomplete,
    })
}

#[tauri::command]
pub fn read_cached_manifest(folder: String) -> Result<CacheManifest, String> {
    let root = cache_root(&folder);
    let manifest_path = root.join(MANIFEST_NAME);
    let bytes = fs::read(&manifest_path)
        .map_err(|e| format!("read {manifest_path:?}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse manifest: {e}"))
}


#[tauri::command]
pub fn read_cached_asset(folder: String, file: String) -> Result<serde_json::Value, String> {
    let path = sanitized_cache_path(&folder, &file)?;
    let bytes = fs::read(&path).map_err(|e| format!("read {path:?}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse {file}: {e}"))
}

#[tauri::command]
pub fn read_cached_bytes(
    folder: String,
    file: String,
) -> Result<tauri::ipc::Response, String> {
    let path = sanitized_cache_path(&folder, &file)?;
    let bytes = fs::read(&path).map_err(|e| format!("read {path:?}: {e}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}


#[tauri::command]
pub fn reextract_level_cache(
    folder: String,
    game_id: Option<String>,
    on_event: Channel<CacheEvent>,
) -> Result<(), String> {
    // Recover game_id from the sidecar BEFORE wiping the cache dir — otherwise
    // a re-extract triggered by App.tsx (which doesn't know the game) would
    // fall back to LEGACY profile and clobber a prior wizard-built cache.
    let resolved_game_id = game_id.or_else(|| {
        recover_game_from_sidecar(&folder).map(|g| match g {
            Game::Rfom => "r1".to_string(),
            Game::R2 => "r2".to_string(),
            Game::R3 => "r3".to_string(),
            Game::Tod => "rc_tod".to_string(),
            Game::A4O => "rc_a4o".to_string(),
            Game::ACiT => "rc_acit".to_string(),
            Game::FFA => "rc_ffa".to_string(),
        })
    });
    let root = cache_root(&folder);
    if root.exists() {
        fs::remove_dir_all(&root)
            .map_err(|e| format!("remove cache dir: {e}"))?;
    }
    extract_level_to_cache(folder, resolved_game_id, on_event)
}

#[derive(Serialize)]
pub struct ClearedCache {
    pub existed: bool,
    pub freed_bytes: u64,
}

pub(crate) fn dir_size_bytes(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            total += dir_size_bytes(&p);
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

#[tauri::command]
pub fn clear_level_cache(folder: String) -> Result<ClearedCache, String> {
    if folder.trim().is_empty() {
        return Err("empty level folder".to_string());
    }
    let root = cache_root(&folder);
    if root.file_name().map(|n| n != CACHE_DIR_NAME).unwrap_or(true) {
        return Err(format!("refusing to delete {root:?}"));
    }
    if !root.exists() {
        return Ok(ClearedCache {
            existed: false,
            freed_bytes: 0,
        });
    }
    let freed_bytes = dir_size_bytes(&root);
    fs::remove_dir_all(&root).map_err(|e| {
        format!("delete {root:?}: {e} — close any program using the cache folder and retry")
    })?;
    Ok(ClearedCache {
        existed: true,
        freed_bytes,
    })
}

fn sanitized_cache_path(folder: &str, file: &str) -> Result<PathBuf, String> {
    if file.split(['/', '\\']).any(|seg| seg == ".." || seg.is_empty()) {
        return Err(format!("rejected path: {file}"));
    }
    if Path::new(file).is_absolute() {
        return Err(format!("rejected absolute path: {file}"));
    }
    Ok(cache_root(folder).join(file))
}

pub(crate) fn tie_as_moby(tie: &lunalib::TieAsset) -> lunalib::MobyAsset {
    let meshes: Vec<lunalib::MobyMesh> = tie
        .meshes
        .iter()
        .map(|m| lunalib::MobyMesh {
            shader_index: m.shader_index,
            vertex_count: m.vertex_count,
            index_count: m.index_count,
            vertex_stride: 0x14,
            positions: m.positions.clone(),
            uvs: m.uvs.clone(),
            indices: m.indices.clone(),
            bone_indices: Vec::new(),
            bone_weights: Vec::new(),
        })
        .collect();
    lunalib::MobyAsset {
        tuid: tie.tuid,
        name: format!("tie_{:016X}", tie.tuid),
        bangles: vec![lunalib::MobyBangle { meshes }],
        bsphere_position: [0.0, 0.0, 0.0],
        bsphere_radius: 0.0,
        shader_tuids: tie.shader_tuids.clone(),
        skeleton: None,
        animset_hash: None,
        bind_pose_inverse_offset: 0,
        rfom_anim_offsets: Vec::new(),
    }
}

