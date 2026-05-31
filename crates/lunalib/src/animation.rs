

use std::io::{Read, Seek};

use crate::error::Result;
use crate::game::AnimProfile;
use crate::igfile::IgFile;
use crate::skeleton::Skeleton;

pub const SECT_ANIMATION: u32 = 0xF000;

const fn pad_to(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

#[derive(Debug, Clone, Copy)]
pub enum TrackKind {
    Rotation,
    Scale,
    Position,

    Unknown,
}

impl TrackKind {
    fn from_bits(bits: u16) -> Self {
        match bits & 0b11 {
            0 => Self::Rotation,
            1 => Self::Scale,
            2 => Self::Position,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TrackMask {
    pub bone_index: u16,
    pub component: u8,
    pub kind: TrackKind,
}

impl TrackMask {
    fn unpack(raw: u16) -> Self {
        let unk = (raw & 0b11) as u8;
        if unk != 2 && std::env::var("RECHIMERA_LOG_ANIM_DETAIL").is_ok() {
            eprintln!(
                "[anim-track-mask] WARN raw=0x{:04X} unk={} (IT asserts == 2; suspect garbage control blob or wrong frame_stride padding)",
                raw, unk
            );
        }
        TrackMask {
            bone_index: (raw >> 6) & 0x3FF,
            component: ((raw >> 2) & 0b11) as u8,
            kind: TrackKind::from_bits(raw >> 4),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnimationHeader {
    pub anim_index: u16,
    pub flags: u16,
    pub num_bones: u16,
    pub num_frames: u16,

    pub name: String,
    pub frame_rate: f32,
    pub linear_speed: f32,
    pub frame_stride: u16,
    pub num_reference_values: u16,
    pub num_16bit_tracks: u16,
    pub num_8bit_tracks: u16,

    pub control_ptr: u32,
    pub frames_ptr: u32,
}

impl AnimationHeader {
    pub const fn duration_seconds(&self) -> f32 {
        if self.frame_rate <= 0.0 {
            0.0
        } else {
            (self.num_frames as f32) / self.frame_rate
        }
    }

    pub const fn is_looping(&self) -> bool {
        self.flags & 0x01 != 0
    }
    pub const fn is_additive(&self) -> bool {
        self.flags & 0x02 != 0
    }
    pub const fn is_packed_frames(&self) -> bool {
        self.flags & 0x04 != 0
    }
    /// Flag bit 0x0200 (R3 split upper/lower face rigs). When set, the clip's
    /// position and scale tracks are DELTAS from the bone's bind pose, not
    /// absolute values: final = bind_translation + delta, bind_scale + delta.
    /// Confirmed via John Harper head probe: `exp_*_lower` clips (0x0206) decode
    /// pos≈0 / scale≈0, while `head_visemes` (0x0007, no 0x200) decodes
    /// pos≈bind / scale≈1.0. IT's enum doesn't name this bit and its decoder
    /// ignores it, so this is RE'd from our own data.
    ///
    /// **A/B bypass:** Set `RECHIMERA_DISABLE_DELTA_PS=1` to force this to false
    /// regardless of the flag bit. Use to verify whether the §3.12 rule applies
    /// to a given game (R3 face = yes; R2 weapon `_fire_p` clips suspected no).
    /// Temporary: will be replaced by per-game `AnimProfile` once R2 fire is
    /// confirmed to not want delta-add.
    pub fn is_delta_pos_scale(&self) -> bool {
        if self.flags & 0x0200 == 0 {
            return false;
        }
        std::env::var("RECHIMERA_DISABLE_DELTA_PS").is_err()
    }

    /// Mirror IT's `FByteswapper<Animation>` (serialize.cpp:660): non-packed
    /// clips have their on-disk `frameStride` rounded UP to a 128-byte
    /// boundary before any decoder indexes frame data. Skipping this makes
    /// every frame after the first read from a misaligned offset, producing
    /// garbage rotations/translations that compound over the clip — symptom
    /// is "bones move but mesh detaches from rig" on long clips like
    /// `hyb_death_crouch_sighted_f` (adv_hybrid) and most trex clips.
    /// Packed-frames clips (flag 0x04) are stored tightly and must NOT be
    /// padded.
    pub fn apply_frame_stride_padding(&mut self) {
        if !self.is_packed_frames() {
            self.frame_stride = (self.frame_stride.saturating_add(0x7F)) & 0xFF80;
        }
    }
}

pub fn read_animation_header<R: Read + Seek>(
    ig: &mut IgFile<R>,
) -> Result<Option<AnimationHeader>> {
    let Some(section) = ig.section(SECT_ANIMATION) else {
        return Ok(None);
    };
    read_animation_header_at(ig, u64::from(section.offset)).map(Some)
}

pub fn animation_section_offsets<R: Read + Seek>(ig: &IgFile<R>) -> Vec<u64> {
    let mut out = Vec::new();
    for s in ig.sections.iter().filter(|s| s.id == SECT_ANIMATION) {
        let count = s.count.max(1);
        let stride = u64::from(s.length);
        let base = u64::from(s.offset);
        for i in 0..count {
            out.push(base + (i as u64) * stride);
        }
    }
    out
}

pub fn read_animation_header_at<R: Read + Seek>(
    ig: &mut IgFile<R>,
    off: u64,
) -> Result<AnimationHeader> {
    ig.stream.seek_to(off + 0x00)?;
    let anim_index = ig.stream.read_u16()?;
    let flags = ig.stream.read_u16()?;
    let num_bones = ig.stream.read_u16()?;
    let num_frames = ig.stream.read_u16()?;
    let name_ptr = u64::from(ig.stream.read_u32()?);
    let _loaded_tag = ig.stream.read_u32()?;
    let _unk4 = ig.stream.read_f32()?;
    let linear_speed = ig.stream.read_f32()?;
    let frame_rate = ig.stream.read_f32()?;
    let _root_motion_ptr = ig.stream.read_u32()?;
    let control_ptr = ig.stream.read_u32()?;
    let frames_ptr = ig.stream.read_u32()?;

    ig.stream.seek_to(off + 0x32)?;
    let frame_stride = ig.stream.read_u16()?;
    let num_reference_values = ig.stream.read_u16()?;
    let num_16bit_tracks = ig.stream.read_u16()?;
    let num_8bit_tracks = ig.stream.read_u16()?;

    let name = if name_ptr != 0 {
        ig.stream.read_cstring_at(name_ptr).unwrap_or_default()
    } else {
        String::new()
    };

    Ok(AnimationHeader {
        anim_index,
        flags,
        num_bones,
        num_frames,
        name,
        frame_rate,
        linear_speed,
        frame_stride,
        num_reference_values,
        num_16bit_tracks,
        num_8bit_tracks,
        control_ptr,
        frames_ptr,
    })
}

#[derive(Debug, Clone)]
pub struct AnimationControl {

    pub ref_pose_rotations: Vec<[i16; 4]>,

    pub ref_pose_values: Vec<i16>,
    pub ref_pose_masks: Vec<TrackMask>,
    pub track16_masks: Vec<TrackMask>,
    pub track8_masks: Vec<TrackMask>,
    pub track8_base_values: Vec<i16>,

    pub blend_masks: Vec<u8>,
}

pub fn read_animation_control<R: Read + Seek>(
    ig: &mut IgFile<R>,
    h: &AnimationHeader,
) -> Result<AnimationControl> {
    if h.control_ptr == 0 {
        return Ok(AnimationControl {
            ref_pose_rotations: Vec::new(),
            ref_pose_values: Vec::new(),
            ref_pose_masks: Vec::new(),
            track16_masks: Vec::new(),
            track8_masks: Vec::new(),
            track8_base_values: Vec::new(),
            blend_masks: Vec::new(),
        });
    }

    let base = u64::from(h.control_ptr);

    let nb = h.num_bones as u32;
    let nrv = h.num_reference_values as u32;
    let n16 = h.num_16bit_tracks as u32;
    let n8 = h.num_8bit_tracks as u32;

    let off_rotations = 0u32;
    let off_values = pad_to(off_rotations + nb * 8, 16);
    let off_value_masks = pad_to(off_values + nrv * 2, 16);
    let off_t16_masks = pad_to(off_value_masks + nrv * 2, 16);
    let off_t8_masks = pad_to(off_t16_masks + n16 * 2, 16);
    let off_t8_base = pad_to(off_t8_masks + n8 * 2, 16);
    let off_blend = pad_to(off_t8_base + n8 * 2, 16);

    ig.stream.seek_to(base + off_rotations as u64)?;
    let mut ref_pose_rotations = Vec::with_capacity(nb as usize);
    for _ in 0..nb {
        let x = ig.stream.read_i16()?;
        let y = ig.stream.read_i16()?;
        let z = ig.stream.read_i16()?;
        let w = ig.stream.read_i16()?;
        ref_pose_rotations.push([x, y, z, w]);
    }

    ig.stream.seek_to(base + off_values as u64)?;
    let mut ref_pose_values = Vec::with_capacity(nrv as usize);
    for _ in 0..nrv {
        ref_pose_values.push(ig.stream.read_i16()?);
    }

    ig.stream.seek_to(base + off_value_masks as u64)?;
    let mut ref_pose_masks = Vec::with_capacity(nrv as usize);
    for _ in 0..nrv {
        let raw = ig.stream.read_u16()?;
        ref_pose_masks.push(TrackMask::unpack(raw));
    }

    ig.stream.seek_to(base + off_t16_masks as u64)?;
    let mut track16_masks = Vec::with_capacity(n16 as usize);
    for _ in 0..n16 {
        let raw = ig.stream.read_u16()?;
        track16_masks.push(TrackMask::unpack(raw));
    }

    ig.stream.seek_to(base + off_t8_masks as u64)?;
    let mut track8_masks = Vec::with_capacity(n8 as usize);
    for _ in 0..n8 {
        let raw = ig.stream.read_u16()?;
        track8_masks.push(TrackMask::unpack(raw));
    }

    ig.stream.seek_to(base + off_t8_base as u64)?;
    let mut track8_base_values = Vec::with_capacity(n8 as usize);
    for _ in 0..n8 {
        track8_base_values.push(ig.stream.read_i16()?);
    }

    ig.stream.seek_to(base + off_blend as u64)?;
    let blend_masks = ig.stream.read_bytes(nb as usize)?;

    Ok(AnimationControl {
        ref_pose_rotations,
        ref_pose_values,
        ref_pose_masks,
        track16_masks,
        track8_masks,
        track8_base_values,
        blend_masks,
    })
}

pub fn read_animation_frame<R: Read + Seek>(
    ig: &mut IgFile<R>,
    h: &AnimationHeader,
    frame_index: u16,
) -> Result<(Vec<i16>, Vec<i8>)> {
    if h.frames_ptr == 0 || h.frame_stride == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    let frame_off = u64::from(h.frames_ptr) + (frame_index as u64) * (h.frame_stride as u64);

    ig.stream.seek_to(frame_off)?;
    let mut values16 = Vec::with_capacity(h.num_16bit_tracks as usize);
    for _ in 0..h.num_16bit_tracks {
        values16.push(ig.stream.read_i16()?);
    }

    let n16 = h.num_16bit_tracks as u32;
    let off8 = pad_to(n16 * 2, 16) as u64;
    ig.stream.seek_to(frame_off + off8)?;
    let mut values8 = Vec::with_capacity(h.num_8bit_tracks as usize);
    for _ in 0..h.num_8bit_tracks {
        values8.push(ig.stream.read_i8()?);
    }

    Ok((values16, values8))
}

#[derive(Debug, Clone)]
pub struct DecodedClip {
    pub name: String,
    pub num_frames: u16,
    pub frame_rate: f32,
    pub looping: bool,
    /// IT animation-flag bit 0x02. Values in `bones[..].translations/rotations/scales`
    /// for additive clips are DELTAS from the bind pose, not absolutes. The cache
    /// builder calls `compose_additive_with_skeleton()` to bake the bind pose in
    /// before export so three.js (which plays GLTF tracks as overrides, not as
    /// additive layers) renders the pose correctly.
    pub additive: bool,
    pub bones: Vec<DecodedBone>,
}

impl DecodedClip {
    /// For 0x0200 additive overlays (R2 weapon `*_fire_p` and similar),
    /// fill any non-animated bone channel with the corresponding channel from
    /// a `base` clip (the matching `*_idle_p`). This is the runtime "additive
    /// layer" the game does at playback: idle holds the weapon pose, fire
    /// only kicks the few recoil bones. Without composing, the fire overlay
    /// shows the un-kicked bones in skeleton bind pose (T-shape arms).
    ///
    /// Idle frames are sampled with wrap-around (`f % base_nf`) so a 21-frame
    /// fire clip can pull from a 50-frame looping idle. Position/scale/rotation
    /// are all replaced — for animated bones the fire's own per-frame values
    /// are kept.
    pub fn compose_with_base(&mut self, base: &DecodedClip, overlay_blend: bool) -> (usize, usize, usize) {
        if self.bones.len() != base.bones.len() {
            return (0, 0, 0);
        }
        let force_all = std::env::var("RECHIMERA_COMPOSE_FORCE")
            .map(|v| v.eq_ignore_ascii_case("all"))
            .unwrap_or(false);
        let fire_nf = self.num_frames.max(1) as usize;
        let base_nf = base.num_frames.max(1) as usize;
        let mut rot_copied = 0usize;
        let mut tra_copied = 0usize;
        let mut scl_copied = 0usize;
        for (b, bone) in self.bones.iter_mut().enumerate() {
            let base_bone = &base.bones[b];
            // Compute idle's per-frame rotation (expanded to fire_nf frames).
            let idle_rot: Option<Vec<f32>> = if base_bone.rotation_animated
                && base_bone.rotations.len() >= base_nf * 4
            {
                let mut out = Vec::with_capacity(fire_nf * 4);
                for f in 0..fire_nf {
                    let bf = (f % base_nf) * 4;
                    out.extend_from_slice(&base_bone.rotations[bf..bf + 4]);
                }
                Some(out)
            } else if base_bone.rotations.len() == 4 {
                let q = &base_bone.rotations[..4];
                let mut out = Vec::with_capacity(fire_nf * 4);
                for _ in 0..fire_nf {
                    out.extend_from_slice(q);
                }
                Some(out)
            } else {
                None
            };
            if let Some(idle_rot) = idle_rot {
                if bone.rotation_animated && overlay_blend && !force_all
                    && bone.rotations.len() >= fire_nf * 4
                {
                    // Runtime additive-layer blend (PS3 R2 weapon overlay).
                    // The encoded fire data per frame is a quaternion that
                    // ALMOST always equals fire's per-clip `ref_rotation`
                    // (the per-bone neutral pose for this clip). On frames
                    // where recoil kicks, the decoded quat diverges from the
                    // ref. The game runtime extracts that divergence:
                    //   delta[f] = decoded[f] * ref_rotation.inverse()
                    // and applies it on top of whatever the underlying layer
                    // (idle) provides:
                    //   final[f] = idle[f] * delta[f]
                    // When decoded ≈ ref, delta ≈ identity, final = idle.
                    // When recoil hits, delta is the recoil rotation and
                    // final = idle composed with recoil.
                    //
                    // This is what IT's `BlendResultAdditive` would do (if
                    // anyone ever wired it up — gltf_shared.cpp:36-64); IT's
                    // version uses skel_bind as the base, we use the matching
                    // idle clip's per-frame pose instead because that's what
                    // the game actually runs underneath.
                    let r = bone.ref_rotation;
                    // ref_rotation inverse = conjugate for a unit quat
                    let ri = [-r[0], -r[1], -r[2], r[3]];
                    for f in 0..fire_nf {
                        let d = [
                            bone.rotations[f * 4],
                            bone.rotations[f * 4 + 1],
                            bone.rotations[f * 4 + 2],
                            bone.rotations[f * 4 + 3],
                        ];
                        let i = [
                            idle_rot[f * 4], idle_rot[f * 4 + 1],
                            idle_rot[f * 4 + 2], idle_rot[f * 4 + 3],
                        ];
                        // delta = decoded * ref^-1
                        let (dx, dy, dz, dw) = quat_mul(d, ri);
                        // final = idle * delta
                        let (x, y, z, w) = quat_mul(i, [dx, dy, dz, dw]);
                        // Hemisphere fix vs idle so three.js doesn't
                        // interpolate the long way around.
                        let dot = x*i[0] + y*i[1] + z*i[2] + w*i[3];
                        let s = if dot < 0.0 { -1.0 } else { 1.0 };
                        let len_sq = x*x + y*y + z*z + w*w;
                        let inv = if len_sq > 1e-12 { s / len_sq.sqrt() } else { 1.0 };
                        bone.rotations[f * 4]     = x * inv;
                        bone.rotations[f * 4 + 1] = y * inv;
                        bone.rotations[f * 4 + 2] = z * inv;
                        bone.rotations[f * 4 + 3] = w * inv;
                    }
                    rot_copied += 1;
                } else if !bone.rotation_animated || force_all {
                    bone.rotations = idle_rot;
                    bone.rotation_animated = true;
                    rot_copied += 1;
                }
            }
            let want_tra = !bone.translation_animated || force_all;
            if want_tra {
                let new_tra = if base_bone.translation_animated
                    && base_bone.translations.len() >= base_nf * 3
                {
                    let mut out = Vec::with_capacity(fire_nf * 3);
                    for f in 0..fire_nf {
                        let bf = (f % base_nf) * 3;
                        out.extend_from_slice(&base_bone.translations[bf..bf + 3]);
                    }
                    Some(out)
                } else if base_bone.translations.len() == 3 {
                    let v = &base_bone.translations[..3];
                    let mut out = Vec::with_capacity(fire_nf * 3);
                    for _ in 0..fire_nf {
                        out.extend_from_slice(v);
                    }
                    Some(out)
                } else {
                    None
                };
                if let Some(out) = new_tra {
                    bone.translations = out;
                    bone.translation_animated = true;
                    tra_copied += 1;
                }
            }
            let want_scl = !bone.scale_animated || force_all;
            if want_scl {
                let new_scl = if base_bone.scale_animated
                    && base_bone.scales.len() >= base_nf * 3
                {
                    let mut out = Vec::with_capacity(fire_nf * 3);
                    for f in 0..fire_nf {
                        let bf = (f % base_nf) * 3;
                        out.extend_from_slice(&base_bone.scales[bf..bf + 3]);
                    }
                    Some(out)
                } else if base_bone.scales.len() == 3 {
                    let v = &base_bone.scales[..3];
                    let mut out = Vec::with_capacity(fire_nf * 3);
                    for _ in 0..fire_nf {
                        out.extend_from_slice(v);
                    }
                    Some(out)
                } else {
                    None
                };
                if let Some(out) = new_scl {
                    bone.scales = out;
                    bone.scale_animated = true;
                    scl_copied += 1;
                }
            }
        }
        (rot_copied, tra_copied, scl_copied)
    }

    /// For additive clips, compose the decoded delta values with the skeleton's
    /// bind pose to produce absolute values that three.js can play directly.
    /// Matches IT's `AnimationMachine::BlendResultAdditive` (gltf_shared.cpp):
    ///   - Translation: bind + delta
    ///   - Scale:       bind * delta (multiplicative)
    ///   - Rotation:    bind_quat * delta_quat (Hamilton product)
    /// No-op for non-additive clips.
    pub fn compose_additive_with_skeleton(&mut self, skel: &Skeleton) {
        if !self.additive {
            return;
        }
        let probe = std::env::var("RECHIMERA_LOG_ANIM_DETAIL").is_ok();
        let (ref_trans, ref_scale) = decompose_skeleton_ref_pose(skel);
        let nf = self.num_frames as usize;
        for (b, bone) in self.bones.iter_mut().enumerate() {
            let bt = ref_trans.get(b).copied().unwrap_or([0.0; 3]);
            let bs = ref_scale.get(b).copied().unwrap_or([1.0; 3]);
            let bq = skel
                .bones
                .get(b)
                .and_then(|sb| {
                    let bl = skel.bind_local.get(b).copied()?;
                    let _ = sb;
                    Some(extract_bind_rotation(&bl))
                })
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            if probe && b < 6 && !bone.rotations.is_empty() && bone.rotation_animated {
                let dx = bone.rotations[0];
                let dy = bone.rotations[1];
                let dz = bone.rotations[2];
                let dw = bone.rotations[3];
                let dot_id = dw;
                let dot_bind = bq[0] * dx + bq[1] * dy + bq[2] * dz + bq[3] * dw;
                eprintln!(
                    "[add-probe] clip='{}' bone={} decoded=({:.3},{:.3},{:.3},{:.3}) \
                     bind=({:.3},{:.3},{:.3},{:.3}) dot_identity={:.3} dot_bind={:.3} \
                     {}",
                    self.name, b, dx, dy, dz, dw,
                    bq[0], bq[1], bq[2], bq[3],
                    dot_id.abs(), dot_bind.abs(),
                    if dot_id.abs() > dot_bind.abs() { "~IDENTITY" } else { "~BIND" }
                );
            }
            if !bone.translations.is_empty() {
                let n = bone.translations.len() / 3;
                for f in 0..n {
                    bone.translations[f * 3] += bt[0];
                    bone.translations[f * 3 + 1] += bt[1];
                    bone.translations[f * 3 + 2] += bt[2];
                }
            }
            if !bone.scales.is_empty() {
                let n = bone.scales.len() / 3;
                for f in 0..n {
                    bone.scales[f * 3] *= bs[0];
                    bone.scales[f * 3 + 1] *= bs[1];
                    bone.scales[f * 3 + 2] *= bs[2];
                }
            }
            if !bone.rotations.is_empty() {
                let n = bone.rotations.len() / 4;
                for f in 0..n {
                    let dx = bone.rotations[f * 4];
                    let dy = bone.rotations[f * 4 + 1];
                    let dz = bone.rotations[f * 4 + 2];
                    let dw = bone.rotations[f * 4 + 3];
                    let (rx, ry, rz, rw) = quat_mul(bq, [dx, dy, dz, dw]);
                    bone.rotations[f * 4] = rx;
                    bone.rotations[f * 4 + 1] = ry;
                    bone.rotations[f * 4 + 2] = rz;
                    bone.rotations[f * 4 + 3] = rw;
                }
            }
            let _ = nf;
        }
    }
}

fn extract_bind_rotation(m: &[f32; 16]) -> [f32; 4] {
    // Column-major 4x4. Top-left 3x3 holds rotation (possibly with scale).
    // Normalize columns to remove scale, then convert to quaternion.
    let cx = [m[0], m[1], m[2]];
    let cy = [m[4], m[5], m[6]];
    let cz = [m[8], m[9], m[10]];
    let lx = (cx[0] * cx[0] + cx[1] * cx[1] + cx[2] * cx[2]).sqrt().max(1e-8);
    let ly = (cy[0] * cy[0] + cy[1] * cy[1] + cy[2] * cy[2]).sqrt().max(1e-8);
    let lz = (cz[0] * cz[0] + cz[1] * cz[1] + cz[2] * cz[2]).sqrt().max(1e-8);
    let r = [
        cx[0] / lx, cx[1] / lx, cx[2] / lx,
        cy[0] / ly, cy[1] / ly, cy[2] / ly,
        cz[0] / lz, cz[1] / lz, cz[2] / lz,
    ];
    matrix_to_quat(&r)
}

fn matrix_to_quat(m: &[f32; 9]) -> [f32; 4] {
    let m00 = m[0]; let m01 = m[3]; let m02 = m[6];
    let m10 = m[1]; let m11 = m[4]; let m12 = m[7];
    let m20 = m[2]; let m21 = m[5]; let m22 = m[8];
    let trace = m00 + m11 + m22;
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        [(m21 - m12) * s, (m02 - m20) * s, (m10 - m01) * s, 0.25 / s]
    } else if m00 > m11 && m00 > m22 {
        let s = 2.0 * (1.0 + m00 - m11 - m22).sqrt();
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = 2.0 * (1.0 + m11 - m00 - m22).sqrt();
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = 2.0 * (1.0 + m22 - m00 - m11).sqrt();
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    }
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> (f32, f32, f32, f32) {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )
}

#[derive(Debug, Clone)]
pub struct DecodedBone {

    pub rotations: Vec<f32>,

    pub translations: Vec<f32>,

    pub scales: Vec<f32>,

    pub rotation_animated: bool,
    pub translation_animated: bool,
    pub scale_animated: bool,
    /// 4-bit mask of which rotation components had at least one track entry
    /// (bit 0 = X, 1 = Y, 2 = Z, 3 = W). For 0x0200 additive overlays, missing
    /// components in animated bones need to be merged from the underlying idle
    /// layer at compose time — the on-disk data leaves them at clip ref-pose
    /// (often 0) because the runtime engine fills them from the layer below.
    /// See IT gltf_shared.cpp:137-140 — IT seeds the same way and has the
    /// same blind-spot for standalone export.
    pub rot_components: u8,
    /// Per-clip per-bone reference rotation (dequantized from
    /// `AnimationControl::ref_pose_rotations[b]`). Used by `compose_with_base`
    /// to compute the actual runtime additive delta:
    ///   `delta[f] = decoded[f] * ref_rotation.inverse()`
    ///   `final[f] = base[f]   * delta[f]`
    /// When fire's decoded ≈ ref_rotation (no recoil at that frame), the
    /// delta is identity and the bone stays at idle's pose. When recoil
    /// kicks, the delta is the rotation difference and lays on top of idle.
    pub ref_rotation: [f32; 4],
}

/// IT's `AnimationMachine::PropagateScaleFrames` (animation_machine.cpp:96-157)
/// pre-multiplies each bone's per-frame translation/scale by its parent's
/// per-frame scale. This compensates for IT's `_s` proxy-bone architecture
/// (extract_gltf.cpp:35-66) which splits every bone into a translation+rotation
/// node and a leaf `_s` scale node. Because IT puts scale on a leaf, GLTF
/// auto-inheritance can't propagate it to children, so IT re-injects it by
/// hand via this pre-multiply.
///
/// Our pipeline does NOT generate `_s` proxy bones — each Insomniac bone
/// becomes one GLTF node carrying translation+rotation+scale, and GLTF's
/// natural transform inheritance already propagates parent scale to children.
/// Applying IT's pre-multiply on top of that yields DOUBLE-scaling: idle
/// clips look fine because parent scales are ≈ 1.0 (1*1 = 1), but additive
/// overlays like `*_fire_p` that animate parent-bone scales contort visibly.
///
/// Kept here (not called) so the function is one line away if we ever add
/// the `_s` proxy hierarchy to match IT's GLTF layout.
#[allow(dead_code)]
fn propagate_scale_frames(clip: &mut DecodedClip, skel: &Skeleton) {
    let nf = clip.num_frames as usize;
    if nf == 0 {
        return;
    }
    let nb = clip.bones.len();
    if nb == 0 || nb != skel.bones.len() {
        return;
    }

    let bind_scale: Vec<[f32; 3]> = skel
        .bind_local
        .iter()
        .map(|bl| {
            let sx = (bl[0] * bl[0] + bl[1] * bl[1] + bl[2] * bl[2]).sqrt();
            let sy = (bl[4] * bl[4] + bl[5] * bl[5] + bl[6] * bl[6]).sqrt();
            let sz = (bl[8] * bl[8] + bl[9] * bl[9] + bl[10] * bl[10]).sqrt();
            [
                if sx > 0.0 { sx } else { 1.0 },
                if sy > 0.0 { sy } else { 1.0 },
                if sz > 0.0 { sz } else { 1.0 },
            ]
        })
        .collect();
    let bind_trans: Vec<[f32; 3]> = skel
        .bind_local
        .iter()
        .map(|bl| [bl[12], bl[13], bl[14]])
        .collect();

    // Helper: ensure a bone's scales/translations are expanded to nf frames.
    fn ensure_frames(buf: &mut Vec<f32>, animated: &mut bool, nf: usize, bind: [f32; 3]) {
        if *animated && buf.len() == nf * 3 {
            return;
        }
        let seed: [f32; 3] = if !*animated && buf.len() == 3 {
            [buf[0], buf[1], buf[2]]
        } else {
            bind
        };
        let mut next = Vec::with_capacity(nf * 3);
        for _ in 0..nf {
            next.extend_from_slice(&seed);
        }
        *buf = next;
        *animated = true;
    }

    // Topological order: each bone's parent must be processed before it.
    // Skeleton bones are stored in tree order in practice, but enforce here.
    let mut order: Vec<usize> = Vec::with_capacity(nb);
    let mut visited = vec![false; nb];
    fn visit(i: usize, skel: &Skeleton, order: &mut Vec<usize>, visited: &mut [bool]) {
        if i >= skel.bones.len() || visited[i] {
            return;
        }
        let b = skel.bones[i];
        let p = b.parent_index;
        if p >= 0 && (p as usize) != i && (p as usize) < skel.bones.len() {
            visit(p as usize, skel, order, visited);
        }
        visited[i] = true;
        order.push(i);
    }
    for i in 0..nb {
        visit(i, skel, &mut order, &mut visited);
    }

    for &i in &order {
        let bone = &skel.bones[i];
        if bone.dont_inherit_scale() {
            continue;
        }
        let parent = match bone.parent() {
            Some(p) if p != i && p < nb => p,
            _ => continue,
        };

        let parent_scale: Vec<[f32; 3]> = {
            let pb = &clip.bones[parent];
            if pb.scale_animated && pb.scales.len() == nf * 3 {
                (0..nf)
                    .map(|f| [pb.scales[f * 3], pb.scales[f * 3 + 1], pb.scales[f * 3 + 2]])
                    .collect()
            } else if !pb.scale_animated && pb.scales.len() == 3 {
                let s = [pb.scales[0], pb.scales[1], pb.scales[2]];
                (0..nf).map(|_| s).collect()
            } else {
                let s = bind_scale[parent];
                (0..nf).map(|_| s).collect()
            }
        };

        let nb_ref = &mut clip.bones[i];
        ensure_frames(&mut nb_ref.scales, &mut nb_ref.scale_animated, nf, bind_scale[i]);
        for f in 0..nf {
            nb_ref.scales[f * 3] *= parent_scale[f][0];
            nb_ref.scales[f * 3 + 1] *= parent_scale[f][1];
            nb_ref.scales[f * 3 + 2] *= parent_scale[f][2];
        }

        ensure_frames(
            &mut nb_ref.translations,
            &mut nb_ref.translation_animated,
            nf,
            bind_trans[i],
        );
        for f in 0..nf {
            nb_ref.translations[f * 3] *= parent_scale[f][0];
            nb_ref.translations[f * 3 + 1] *= parent_scale[f][1];
            nb_ref.translations[f * 3 + 2] *= parent_scale[f][2];
        }
    }
}

fn dequantize_quaternion(qi: [i16; 4]) -> [f32; 4] {
    const INV: f32 = 1.0 / 32767.0;
    let mut q = [
        qi[0] as f32 * INV,
        qi[1] as f32 * INV,
        qi[2] as f32 * INV,
        qi[3] as f32 * INV,
    ];
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if len_sq > 0.0 {
        let inv_len = 1.0 / len_sq.sqrt();
        q[0] *= inv_len;
        q[1] *= inv_len;
        q[2] *= inv_len;
        q[3] *= inv_len;
    } else {

        q = [0.0, 0.0, 0.0, 1.0];
    }
    q
}

pub fn decode_animation<R: Read + Seek>(
    ig: &mut IgFile<R>,
    h: &AnimationHeader,
    ctrl: &AnimationControl,
    position_scale: f32,
    scale_scale: f32,
) -> Result<DecodedClip> {
    decode_animation_with_skel_bones(
        ig, h, ctrl, position_scale, scale_scale, None, None, AnimProfile::LEGACY,
    )
}

/// Convenience wrapper used by the V2 cache path: passes the skeleton so the
/// decoder can fall back to the bone's bind translation/scale for any channel
/// component the clip doesn't touch (mirrors what IT's `gltf_shared.cpp` does
/// for partial-component animation tracks).
pub fn decode_animation_with_skel<R: Read + Seek>(
    ig: &mut IgFile<R>,
    h: &AnimationHeader,
    ctrl: &AnimationControl,
    position_scale: f32,
    scale_scale: f32,
    skel: &Skeleton,
    profile: AnimProfile,
) -> Result<DecodedClip> {
    decode_animation_with_skel_bones(
        ig, h, ctrl, position_scale, scale_scale, None, Some(skel), profile,
    )
}

/// Same as `decode_animation` but lets the caller override the bone count when
/// the header value is a per-anim subset count rather than the skeleton's full
/// bone count. Per IT's `LoadAnimations` (`gltf_shared.cpp:542`), RFOM animations
/// flagged Additive store an arbitrary `numBones` in their header and IT rewrites
/// it to `skel->numBones` before decoding so track-mask bone indices (which are
/// skeleton-space) clip against the right range. Non-additive anims always have
/// `header.num_bones == skel.num_bones`, so passing `Some(skel_bones)` is safe
/// for both cases.
pub fn decode_animation_with_skel_bones<R: Read + Seek>(
    ig: &mut IgFile<R>,
    h: &AnimationHeader,
    ctrl: &AnimationControl,
    position_scale: f32,
    scale_scale: f32,
    override_num_bones: Option<u16>,
    skel: Option<&Skeleton>,
    profile: AnimProfile,
) -> Result<DecodedClip> {
    let (ref_trans, ref_scale) = match skel {
        Some(s) => {
            let (rt, rs) = decompose_skeleton_ref_pose(s);
            (Some(rt), Some(rs))
        }
        None => (None, None),
    };
    let nb = override_num_bones
        .map(|n| n as usize)
        .unwrap_or(h.num_bones as usize);
    let nf = h.num_frames as usize;

    if std::env::var("RECHIMERA_LOG_ANIM_DETAIL").is_ok() {
        let additive = h.is_additive();
        let packed = (h.flags & 0x04) != 0;
        let looping = (h.flags & 0x01) != 0;
        let count_kinds = |masks: &[crate::animation::TrackMask]| -> (usize, usize, usize) {
            let mut r = 0usize;
            let mut p = 0usize;
            let mut s = 0usize;
            for m in masks {
                match m.kind {
                    TrackKind::Rotation => r += 1,
                    TrackKind::Position => p += 1,
                    TrackKind::Scale => s += 1,
                    TrackKind::Unknown => {}
                }
            }
            (r, p, s)
        };
        let (ref_r, ref_p, ref_s) = count_kinds(&ctrl.ref_pose_masks);
        let (t16_r, t16_p, t16_s) = count_kinds(&ctrl.track16_masks);
        let (t8_r, t8_p, t8_s) = count_kinds(&ctrl.track8_masks);
        eprintln!(
            "[anim-decode] name='{}' frames={} hdr_bones={} effective_bones={} flags=0x{:04X} \
             (looping={} additive={} packed={}) fps={} stride={} 16bit={} 8bit={} \
             ref_values={} pos_scale={:.6} scale_scale={:.6} \
             ref_RPS={}/{}/{} t16_RPS={}/{}/{} t8_RPS={}/{}/{}",
            h.name, nf, h.num_bones, nb, h.flags, looping, additive, packed,
            h.frame_rate, h.frame_stride, h.num_16bit_tracks, h.num_8bit_tracks,
            h.num_reference_values, position_scale, scale_scale,
            ref_r, ref_p, ref_s, t16_r, t16_p, t16_s, t8_r, t8_p, t8_s,
        );
    }

    let mut rot_values: Vec<[i16; 4]> = vec![[0; 4]; nb * nf];
    let mut rot_animated: Vec<bool> = vec![false; nb];
    // Per-bone 4-bit mask of which rotation components had at least one track.
    // See IT gltf_shared.cpp:137-140 — IT seeds animated rotations with the
    // clip's RefPoseRotations[bone] and overwrites only the components that
    // have tracks. For 0x0200 overlays (R2 weapon `_fire_p`) the engine relies
    // on the runtime layer below to provide the un-tracked components; for
    // standalone playback we have to do that ourselves in compose.
    let mut rot_components: Vec<u8> = vec![0u8; nb];

    for b in 0..nb {
        let r = ctrl.ref_pose_rotations.get(b).copied().unwrap_or([0, 0, 0, 32767]);
        for f in 0..nf {
            rot_values[b * nf + f] = r;
        }
    }

    let mut pos_values: Vec<[i16; 3]> = vec![[0; 3]; nb * nf];
    let mut pos_set: Vec<u8> = vec![0u8; nb * nf];
    let mut pos_static_value: Vec<[i16; 3]> = vec![[0; 3]; nb];
    let mut pos_static_set: Vec<u8> = vec![0u8; nb];
    let mut pos_animated: Vec<bool> = vec![false; nb];

    let mut scl_values: Vec<[i16; 3]> = vec![[0; 3]; nb * nf];
    let mut scl_set: Vec<u8> = vec![0u8; nb * nf];
    let mut scl_static_value: Vec<[i16; 3]> = vec![[0; 3]; nb];
    let mut scl_static_set: Vec<u8> = vec![0u8; nb];
    let mut scl_animated: Vec<bool> = vec![false; nb];

    for (i, m) in ctrl.ref_pose_masks.iter().enumerate() {
        let v = match ctrl.ref_pose_values.get(i) {
            Some(&v) => v,
            None => continue,
        };
        let b = m.bone_index as usize;
        if b >= nb {
            continue;
        }
        let c = m.component as usize;
        match m.kind {
            TrackKind::Rotation => {
                if c < 4 {
                    for f in 0..nf {
                        rot_values[b * nf + f][c] = v;
                    }
                }
            }
            TrackKind::Position => {
                if c < 3 {
                    pos_static_value[b][c] = v;
                    pos_static_set[b] |= 1 << c;
                }
            }
            TrackKind::Scale => {
                if c < 3 {
                    scl_static_value[b][c] = v;
                    scl_static_set[b] |= 1 << c;
                }
            }
            TrackKind::Unknown => {}
        }
    }

    let mark_pos_seed = |b: usize, set: &mut [u8]| {
        if pos_static_set[b] != 0 {
            for f in 0..nf {
                set[b * nf + f] = pos_static_set[b];
            }
        }
    };
    let mark_scl_seed = |b: usize, set: &mut [u8]| {
        if scl_static_set[b] != 0 {
            for f in 0..nf {
                set[b * nf + f] = scl_static_set[b];
            }
        }
    };

    let mut seed_for_bone_kind = |b: usize, kind: TrackKind| {
        if b >= nb {
            return;
        }
        match kind {
            TrackKind::Rotation => {
                rot_animated[b] = true;
            }
            TrackKind::Position => {
                if !pos_animated[b] {

                    if pos_static_set[b] != 0 {
                        let v = pos_static_value[b];
                        for f in 0..nf {
                            pos_values[b * nf + f] = v;
                        }
                        mark_pos_seed(b, &mut pos_set);
                    }
                    pos_animated[b] = true;
                }
            }
            TrackKind::Scale => {
                if !scl_animated[b] {
                    if scl_static_set[b] != 0 {
                        let v = scl_static_value[b];
                        for f in 0..nf {
                            scl_values[b * nf + f] = v;
                        }
                        mark_scl_seed(b, &mut scl_set);
                    }
                    scl_animated[b] = true;
                }
            }
            TrackKind::Unknown => {}
        }
    };

    for m in &ctrl.track16_masks {
        seed_for_bone_kind(m.bone_index as usize, m.kind);
    }
    for m in &ctrl.track8_masks {
        seed_for_bone_kind(m.bone_index as usize, m.kind);
    }

    for f in 0..nf {
        let (v16, v8) = read_animation_frame(ig, h, f as u16)?;

        for (i, m) in ctrl.track16_masks.iter().enumerate() {
            let v = match v16.get(i) {
                Some(&v) => v,
                None => continue,
            };
            let b = m.bone_index as usize;
            if b >= nb {
                continue;
            }
            let c = m.component as usize;
            match m.kind {
                TrackKind::Rotation => {
                    if c < 4 {
                        rot_values[b * nf + f][c] = v;
                        rot_components[b] |= 1 << c;
                    }
                }
                TrackKind::Position => {
                    if c < 3 {
                        pos_values[b * nf + f][c] = v;
                        pos_set[b * nf + f] |= 1 << c;
                    }
                }
                TrackKind::Scale => {
                    if c < 3 {
                        scl_values[b * nf + f][c] = v;
                        scl_set[b * nf + f] |= 1 << c;
                    }
                }
                TrackKind::Unknown => {}
            }
        }

        for (i, m) in ctrl.track8_masks.iter().enumerate() {
            let delta = match v8.get(i) {
                Some(&v) => v as i32,
                None => continue,
            };
            let base = ctrl
                .track8_base_values
                .get(i)
                .copied()
                .unwrap_or(0) as i32;

            let value = (base as i16).wrapping_add(delta as i16);
            let b = m.bone_index as usize;
            if b >= nb {
                continue;
            }
            let c = m.component as usize;
            match m.kind {
                TrackKind::Rotation => {
                    if c < 4 {
                        rot_values[b * nf + f][c] = value;
                        rot_components[b] |= 1 << c;
                    }
                }
                TrackKind::Position => {
                    if c < 3 {
                        pos_values[b * nf + f][c] = value;
                        pos_set[b * nf + f] |= 1 << c;
                    }
                }
                TrackKind::Scale => {
                    if c < 3 {
                        scl_values[b * nf + f][c] = value;
                        scl_set[b * nf + f] |= 1 << c;
                    }
                }
                TrackKind::Unknown => {}
            }
        }
    }

    let additive = h.is_additive();
    let delta_ps = profile.delta_pos_scale_active(h.flags & 0x0200 != 0);

    let mut bones = Vec::with_capacity(nb);
    let blend_gate = profile.blend_mask_rotation_gate_active();
    for b in 0..nb {

        let blend_mask = ctrl.blend_masks.get(b).copied().unwrap_or(1);
        // 0x0200 clips: the per-clip `ctrl.ref_pose_rotations` is in a
        // clip-local frame that does NOT match the skeleton bind frame
        // (bone 18 in carbine_fire_p: clip_ref ~identity vs skel_bind 90°X)
        // — using it as fallback for non-animated bones twists the body
        // onto its side. Using SKELETON BIND as the fallback puts the body
        // upright (verified 2026-05-30).
        //
        // Per-frame ANIMATED rotation tracks are the canonical absolute
        // rotation as-is. Tried composing `bind * decoded` 2026-05-30 —
        // produced T-pose arms because `decoded ≈ identity` for most
        // frames, proving the values aren't bind-relative quaternion
        // deltas. So animated rotations stay direct.
        let rotations = if rot_animated[b] {
            let mut out = Vec::with_capacity(nf * 4);
            for f in 0..nf {
                let q = dequantize_quaternion(rot_values[b * nf + f]);
                out.extend_from_slice(&q);
            }
            out
        } else if additive && blend_mask == 0 && blend_gate {
            // IT gltf_shared.cpp:282 — for additive clips, a bone outside
            // this blend layer (blend_mask == 0) gets NO rotation channel.
            // Gated by profile: needed for R3 split upper/lower face rigs;
            // misfires on R2 weapon `_fire_p` overlays where it drops valid
            // bone rotations from the rig.
            Vec::new()
        } else if delta_ps {
            let bind_q = skel
                .and_then(|s| s.bind_local.get(b).copied())
                .map(|bl| extract_bind_rotation(&bl))
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            bind_q.to_vec()
        } else {
            let q = dequantize_quaternion(
                ctrl.ref_pose_rotations.get(b).copied().unwrap_or([0, 0, 0, 32767]),
            );
            q.to_vec()
        };

        let rt_fallback = ref_trans.as_ref().and_then(|v| v.get(b).copied()).unwrap_or([0.0; 3]);
        let rs_fallback = ref_scale.as_ref().and_then(|v| v.get(b).copied()).unwrap_or([1.0; 3]);

        // For 0x200 (delta) clips both POSITION and SCALE are delta-from-bind:
        // `final = bind + decoded`. R2 weapon `_fire_p` overlays carry 237
        // scale ref entries with raw=(0,0,0) encoding "no change from bind";
        // without the bind bias those collapse to scale 0 and the mesh flattens.
        // R3 face rig fix 2026-05-28 (lower-face bones) needs the same for
        // position. IT's standalone path doesn't do this — IT's GLTF for these
        // clips is also broken; this is our delta-decode for the standalone
        // viewer.
        let pos_bias = |c: usize| if delta_ps { rt_fallback[c] } else { 0.0 };
        let scl_bias = |c: usize| if delta_ps { rs_fallback[c] } else { 0.0 };

        let translations = if pos_animated[b] {
            let mut out = Vec::with_capacity(nf * 3);
            for f in 0..nf {
                let raw = pos_values[b * nf + f];
                let mask = pos_set[b * nf + f];
                for c in 0..3 {
                    if mask & (1 << c) != 0 {
                        out.push(raw[c] as f32 * position_scale + pos_bias(c));
                    } else {
                        out.push(rt_fallback[c]);
                    }
                }
            }
            out
        } else if pos_static_set[b] != 0 {
            let raw = pos_static_value[b];
            let mask = pos_static_set[b];
            let mut out = Vec::with_capacity(3);
            for c in 0..3 {
                if mask & (1 << c) != 0 {
                    out.push(raw[c] as f32 * position_scale + pos_bias(c));
                } else {
                    out.push(rt_fallback[c]);
                }
            }
            out
        } else {
            Vec::new()
        };

        let scales = if scl_animated[b] {
            let mut out = Vec::with_capacity(nf * 3);
            for f in 0..nf {
                let raw = scl_values[b * nf + f];
                let mask = scl_set[b * nf + f];
                for c in 0..3 {
                    if mask & (1 << c) != 0 {
                        out.push(raw[c] as f32 * scale_scale + scl_bias(c));
                    } else {
                        out.push(rs_fallback[c]);
                    }
                }
            }
            out
        } else if scl_static_set[b] != 0 {
            let raw = scl_static_value[b];
            let mask = scl_static_set[b];
            let mut out = Vec::with_capacity(3);
            for c in 0..3 {
                if mask & (1 << c) != 0 {
                    out.push(raw[c] as f32 * scale_scale + scl_bias(c));
                } else {
                    out.push(rs_fallback[c]);
                }
            }
            out
        } else {
            Vec::new()
        };

        let ref_rot_q = dequantize_quaternion(
            ctrl.ref_pose_rotations
                .get(b)
                .copied()
                .unwrap_or([0, 0, 0, 32767]),
        );
        bones.push(DecodedBone {
            rotations,
            translations,
            scales,
            rotation_animated: rot_animated[b],
            translation_animated: pos_animated[b],
            scale_animated: scl_animated[b],
            rot_components: rot_components[b],
            ref_rotation: ref_rot_q,
        });
    }

    let clip = DecodedClip {
        name: h.name.clone(),
        num_frames: h.num_frames,
        frame_rate: h.frame_rate,
        looping: h.is_looping(),
        additive: h.is_additive(),
        bones,
    };
    let _ = skel;
    Ok(clip)
}

fn decompose_skeleton_ref_pose(skel: &Skeleton) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let n = skel.bones.len();
    let mut ref_trans = Vec::with_capacity(n);
    let mut ref_scale = Vec::with_capacity(n);
    for i in 0..n {
        let bl = skel.bind_local.get(i).copied().unwrap_or([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]);
        ref_trans.push([bl[12], bl[13], bl[14]]);
        let sx = (bl[0] * bl[0] + bl[1] * bl[1] + bl[2] * bl[2]).sqrt();
        let sy = (bl[4] * bl[4] + bl[5] * bl[5] + bl[6] * bl[6]).sqrt();
        let sz = (bl[8] * bl[8] + bl[9] * bl[9] + bl[10] * bl[10]).sqrt();
        ref_scale.push([
            if sx > 0.0 { sx } else { 1.0 },
            if sy > 0.0 { sy } else { 1.0 },
            if sz > 0.0 { sz } else { 1.0 },
        ]);
    }
    (ref_trans, ref_scale)
}

pub fn decode_animation_with_skeleton<R: Read + Seek>(
    ig: &mut IgFile<R>,
    h: &AnimationHeader,
    ctrl: &AnimationControl,
    position_scale: f32,
    scale_scale: f32,
    skel: &Skeleton,
) -> Result<DecodedClip> {
    let nb = skel.bones.len();
    let nf = h.num_frames as usize;
    let additive = h.is_additive();

    if std::env::var("RECHIMERA_LOG_ANIM_DETAIL").is_ok() {
        eprintln!(
            "[anim-decode] name='{}' frames={} hdr_bones={} skel_bones={} flags=0x{:04X} additive={} fps={} stride={} 16bit={} 8bit={} ref_values={} pos_scale={:.6} scale_scale={:.6}",
            h.name, nf, h.num_bones, nb, h.flags, additive, h.frame_rate, h.frame_stride,
            h.num_16bit_tracks, h.num_8bit_tracks, h.num_reference_values, position_scale, scale_scale,
        );
    }

    let (ref_trans, ref_scale) = decompose_skeleton_ref_pose(skel);

    let mut pos_static_value: Vec<[i16; 3]> = vec![[0; 3]; nb];
    let mut pos_static_mask: Vec<u8> = vec![0u8; nb];
    let mut scl_static_value: Vec<[i16; 3]> = vec![[0; 3]; nb];
    let mut scl_static_mask: Vec<u8> = vec![0u8; nb];

    for (i, m) in ctrl.ref_pose_masks.iter().enumerate() {
        let v = match ctrl.ref_pose_values.get(i) {
            Some(&v) => v,
            None => continue,
        };
        let b = m.bone_index as usize;
        if b >= nb {
            continue;
        }
        let c = m.component as usize;
        if c >= 3 {
            continue;
        }
        match m.kind {
            TrackKind::Position => {
                pos_static_value[b][c] = v;
                pos_static_mask[b] |= 1 << c;
            }
            TrackKind::Scale => {
                scl_static_value[b][c] = v;
                scl_static_mask[b] |= 1 << c;
            }
            _ => {}
        }
    }

    let mut rot_frames: Vec<Option<Vec<[i16; 4]>>> = vec![None; nb];
    let mut pos_frames: Vec<Option<Vec<[i16; 3]>>> = vec![None; nb];
    let mut scl_frames: Vec<Option<Vec<[i16; 3]>>> = vec![None; nb];
    let mut pos_set: Vec<Vec<u8>> = vec![Vec::new(); nb];
    let mut scl_set: Vec<Vec<u8>> = vec![Vec::new(); nb];

    let init_rot = |b: usize, rot_frames: &mut Vec<Option<Vec<[i16; 4]>>>| {
        if rot_frames[b].is_none() {
            let r = ctrl
                .ref_pose_rotations
                .get(b)
                .copied()
                .unwrap_or([0, 0, 0, 32767]);
            rot_frames[b] = Some(vec![r; nf]);
        }
    };

    for m in ctrl.track16_masks.iter().chain(ctrl.track8_masks.iter()) {
        let b = m.bone_index as usize;
        if b >= nb {
            continue;
        }
        match m.kind {
            TrackKind::Rotation => init_rot(b, &mut rot_frames),
            TrackKind::Position => {
                if pos_frames[b].is_none() {
                    let seed = if pos_static_mask[b] != 0 {
                        pos_static_value[b]
                    } else {
                        [0; 3]
                    };
                    pos_frames[b] = Some(vec![seed; nf]);
                    let seed_mask = pos_static_mask[b];
                    pos_set[b] = vec![seed_mask; nf];
                }
            }
            TrackKind::Scale => {
                if scl_frames[b].is_none() {
                    let seed = if scl_static_mask[b] != 0 {
                        scl_static_value[b]
                    } else {
                        [0; 3]
                    };
                    scl_frames[b] = Some(vec![seed; nf]);
                    let seed_mask = scl_static_mask[b];
                    scl_set[b] = vec![seed_mask; nf];
                }
            }
            TrackKind::Unknown => {}
        }
    }

    for f in 0..nf {
        let (v16, v8) = read_animation_frame(ig, h, f as u16)?;

        for (i, m) in ctrl.track16_masks.iter().enumerate() {
            let v = match v16.get(i) {
                Some(&v) => v,
                None => continue,
            };
            let b = m.bone_index as usize;
            if b >= nb {
                continue;
            }
            let c = m.component as usize;
            match m.kind {
                TrackKind::Rotation => {
                    if c < 4 {
                        if let Some(buf) = rot_frames[b].as_mut() {
                            buf[f][c] = v;
                        }
                    }
                }
                TrackKind::Position => {
                    if c < 3 {
                        if let Some(buf) = pos_frames[b].as_mut() {
                            buf[f][c] = v;
                            pos_set[b][f] |= 1 << c;
                        }
                    }
                }
                TrackKind::Scale => {
                    if c < 3 {
                        if let Some(buf) = scl_frames[b].as_mut() {
                            buf[f][c] = v;
                            scl_set[b][f] |= 1 << c;
                        }
                    }
                }
                TrackKind::Unknown => {}
            }
        }

        for (i, m) in ctrl.track8_masks.iter().enumerate() {
            let delta = match v8.get(i) {
                Some(&v) => v as i32,
                None => continue,
            };
            let base = ctrl.track8_base_values.get(i).copied().unwrap_or(0) as i32;
            let value = (base as i16).wrapping_add(delta as i16);
            let b = m.bone_index as usize;
            if b >= nb {
                continue;
            }
            let c = m.component as usize;
            match m.kind {
                TrackKind::Rotation => {
                    if c < 4 {
                        if let Some(buf) = rot_frames[b].as_mut() {
                            buf[f][c] = value;
                        }
                    }
                }
                TrackKind::Position => {
                    if c < 3 {
                        if let Some(buf) = pos_frames[b].as_mut() {
                            buf[f][c] = value;
                            pos_set[b][f] |= 1 << c;
                        }
                    }
                }
                TrackKind::Scale => {
                    if c < 3 {
                        if let Some(buf) = scl_frames[b].as_mut() {
                            buf[f][c] = value;
                            scl_set[b][f] |= 1 << c;
                        }
                    }
                }
                TrackKind::Unknown => {}
            }
        }
    }

    let mut bones = Vec::with_capacity(nb);
    for b in 0..nb {
        let blend_mask = ctrl.blend_masks.get(b).copied().unwrap_or(0xFF);

        let rotations = if let Some(frames) = rot_frames[b].as_ref() {
            let mut out = Vec::with_capacity(nf * 4);
            for q in frames {
                let dq = dequantize_quaternion(*q);
                out.extend_from_slice(&dq);
            }
            out
        } else if additive && blend_mask == 0 {
            Vec::new()
        } else {
            let q = dequantize_quaternion(
                ctrl.ref_pose_rotations
                    .get(b)
                    .copied()
                    .unwrap_or([0, 0, 0, 32767]),
            );
            q.to_vec()
        };

        let translations = if let Some(frames) = pos_frames[b].as_ref() {
            let mut out = Vec::with_capacity(nf * 3);
            for f in 0..nf {
                let raw = frames[f];
                let mask = pos_set[b][f];
                let rt = ref_trans[b];
                for c in 0..3 {
                    if mask & (1 << c) != 0 {
                        out.push(raw[c] as f32 * position_scale);
                    } else {
                        out.push(rt[c]);
                    }
                }
            }
            out
        } else if pos_static_mask[b] != 0 {
            let raw = pos_static_value[b];
            let mask = pos_static_mask[b];
            let rt = ref_trans[b];
            let mut out = Vec::with_capacity(3);
            for c in 0..3 {
                if mask & (1 << c) != 0 {
                    out.push(raw[c] as f32 * position_scale);
                } else {
                    out.push(rt[c]);
                }
            }
            out
        } else {
            Vec::new()
        };

        let scales = if let Some(frames) = scl_frames[b].as_ref() {
            let mut out = Vec::with_capacity(nf * 3);
            for f in 0..nf {
                let raw = frames[f];
                let mask = scl_set[b][f];
                let rs = ref_scale[b];
                for c in 0..3 {
                    if mask & (1 << c) != 0 {
                        out.push(raw[c] as f32 * scale_scale);
                    } else {
                        out.push(rs[c]);
                    }
                }
            }
            out
        } else if scl_static_mask[b] != 0 {
            let raw = scl_static_value[b];
            let mask = scl_static_mask[b];
            let rs = ref_scale[b];
            let mut out = Vec::with_capacity(3);
            for c in 0..3 {
                if mask & (1 << c) != 0 {
                    out.push(raw[c] as f32 * scale_scale);
                } else {
                    out.push(rs[c]);
                }
            }
            out
        } else {
            Vec::new()
        };

        bones.push(DecodedBone {
            rotation_animated: rot_frames[b].is_some(),
            translation_animated: pos_frames[b].is_some(),
            scale_animated: scl_frames[b].is_some(),
            rotations,
            translations,
            scales,
            rot_components: 0xF,
            ref_rotation: [0.0, 0.0, 0.0, 1.0],
        });
    }

    Ok(DecodedClip {
        name: h.name.clone(),
        num_frames: h.num_frames,
        frame_rate: h.frame_rate,
        looping: h.is_looping(),
        additive: h.is_additive(),
        bones,
    })
}

