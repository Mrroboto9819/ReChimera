use std::env;

use crate::level_layout::LevelLayout;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Game {
    Rfom,
    R2,
    R3,
    Tod,
    A4O,
    ACiT,
    FFA,
}

impl Game {
    pub fn from_id(id: &str) -> Option<Self> {
        match id.to_ascii_lowercase().as_str() {
            "r1" | "rfom" | "resistance_fall_of_man" => Some(Self::Rfom),
            "r2" | "resistance2" => Some(Self::R2),
            "r3" | "resistance3" => Some(Self::R3),
            "rc_tod" | "tod" | "tools_of_destruction" => Some(Self::Tod),
            "rc_a4o" | "a4o" | "all_4_one" => Some(Self::A4O),
            "rc_acit" | "acit" | "crack_in_time" => Some(Self::ACiT),
            "rc_ffa" | "ffa" | "full_frontal_assault" => Some(Self::FFA),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rfom => "Resistance: Fall of Man",
            Self::R2 => "Resistance 2",
            Self::R3 => "Resistance 3",
            Self::Tod => "Ratchet & Clank: Tools of Destruction",
            Self::A4O => "Ratchet & Clank: All 4 One",
            Self::ACiT => "Ratchet & Clank: A Crack in Time",
            Self::FFA => "Ratchet & Clank: Full Frontal Assault",
        }
    }

    pub fn layout(self) -> LevelLayout {
        match self {
            Self::Rfom => LevelLayout::Rfom,
            Self::Tod => LevelLayout::Tod,
            Self::R2 | Self::R3 | Self::A4O | Self::ACiT | Self::FFA => LevelLayout::V2,
        }
    }

    pub fn anim_profile(self) -> AnimProfile {
        match self {
            Self::R3 => AnimProfile {
                game: Some(self),
                apply_delta_pos_scale: true,
                apply_blend_mask_rotation_gate: true,
                apply_frame_remap: true,
                untracked_rotation_bind_fallback: true,
                rebase_unflagged_positions: true,
            },
            Self::R2 => AnimProfile {
                game: Some(self),
                apply_delta_pos_scale: true,
                apply_blend_mask_rotation_gate: false,
                apply_frame_remap: false,
                untracked_rotation_bind_fallback: false,
                rebase_unflagged_positions: false,
            },
            Self::Rfom | Self::Tod | Self::A4O | Self::ACiT | Self::FFA => AnimProfile {
                game: Some(self),
                apply_delta_pos_scale: false,
                apply_blend_mask_rotation_gate: false,
                apply_frame_remap: false,
                untracked_rotation_bind_fallback: false,
                rebase_unflagged_positions: false,
            },
        }
    }

    pub fn matrix_convention(self) -> MatrixConvention {
        MatrixConvention::DEFAULT
    }

    pub fn profile(self) -> GameProfile {
        GameProfile {
            game: Some(self),
            layout: self.layout(),
            anim: self.anim_profile(),
            matrix: self.matrix_convention(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AnimProfile {
    pub game: Option<Game>,
    pub apply_delta_pos_scale: bool,
    pub apply_blend_mask_rotation_gate: bool,
    /// R3-only: honor the frame-remap table pointed to by header +0x0C
    /// (logical frame -> stored frame). R2/RFOM/TOD clips do not use it and
    /// were rendering correctly without it, so it stays off for them.
    pub apply_frame_remap: bool,
    /// R3-only: rest untracked-rotation bones at the SKELETON BIND instead of
    /// the clip's ref pose. Needed because R3 gameheads share one animset whose
    /// ref pose is up to 180° off each head's bind on teeth bones. R2 faces
    /// were working with the ref-pose fallback, so it stays off for them.
    pub untracked_rotation_bind_fallback: bool,
    /// R3-only: clips WITHOUT header flag 0x0400 (old-generation encoding,
    /// e.g. child_head) author positions against a ref pose that does not
    /// match the target skeleton (child face_angry refs run ~1.4x the child
    /// bind — adult-head proportions). Rebase tracked positions to
    /// `bind + (decoded - clip_ref)` and ref-only positions to plain bind.
    /// 0x0400 clips (all adult gameheads) keep absolute positions.
    pub rebase_unflagged_positions: bool,
}

impl AnimProfile {
    pub const LEGACY: AnimProfile = AnimProfile {
        game: None,
        apply_delta_pos_scale: true,
        apply_blend_mask_rotation_gate: true,
        apply_frame_remap: true,
        untracked_rotation_bind_fallback: true,
        rebase_unflagged_positions: false,
    };

    pub fn delta_pos_scale_active(&self, header_flag_set: bool) -> bool {
        if !header_flag_set {
            return false;
        }
        if env::var("RECHIMERA_DISABLE_DELTA_PS").is_ok() {
            return false;
        }
        self.apply_delta_pos_scale
    }

    pub fn blend_mask_rotation_gate_active(&self) -> bool {
        if env::var("RECHIMERA_DISABLE_BLEND_GATE").is_ok() {
            return false;
        }
        self.apply_blend_mask_rotation_gate
    }

    pub fn frame_remap_active(&self) -> bool {
        if env::var("RECHIMERA_DISABLE_FRAME_REMAP").is_ok() {
            return false;
        }
        self.apply_frame_remap
    }

    pub fn untracked_bind_fallback_active(&self) -> bool {
        if env::var("RECHIMERA_DISABLE_BIND_FALLBACK").is_ok() {
            return false;
        }
        self.untracked_rotation_bind_fallback
    }

    pub fn unflagged_pos_rebase_active(&self, header_has_0x0400: bool) -> bool {
        if header_has_0x0400 {
            return false;
        }
        if env::var("RECHIMERA_DISABLE_POS_REBASE").is_ok() {
            return false;
        }
        self.rebase_unflagged_positions
    }

    pub fn matrix_convention(&self) -> MatrixConvention {
        self.game
            .map(Game::matrix_convention)
            .unwrap_or(MatrixConvention::DEFAULT)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MatrixConvention {
    pub recover_skeleton_shift_bytes: bool,
    pub propagate_scale_frames: bool,
    pub yard_to_meter: f32,
}

impl MatrixConvention {
    pub const DEFAULT: MatrixConvention = MatrixConvention {
        recover_skeleton_shift_bytes: false,
        propagate_scale_frames: false,
        yard_to_meter: 0.9144,
    };

    pub fn shift_scale(&self, shift: u16) -> f32 {
        let mut s = shift;
        if self.recover_skeleton_shift_bytes && s > 15 {
            s = s.swap_bytes();
        }
        if (s as u32) < 15 {
            1.0 / (0x8000u32 >> s) as f32
        } else {
            1.0 / 32768.0
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GameProfile {
    pub game: Option<Game>,
    pub layout: LevelLayout,
    pub anim: AnimProfile,
    pub matrix: MatrixConvention,
}

impl GameProfile {
    pub fn legacy(layout: LevelLayout) -> Self {
        GameProfile {
            game: None,
            layout,
            anim: AnimProfile::LEGACY,
            matrix: MatrixConvention::DEFAULT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_scale_matches_legacy_formula() {
        let m = MatrixConvention::DEFAULT;
        assert_eq!(m.shift_scale(0), 1.0 / 32768.0);
        assert_eq!(m.shift_scale(5), 1.0 / 1024.0);
        assert_eq!(m.shift_scale(14), 0.5);
        assert_eq!(m.shift_scale(15), 1.0 / 32768.0);
        assert_eq!(m.shift_scale(768), 1.0 / 32768.0);
    }

    #[test]
    fn shift_scale_recovery_unswaps_bytes() {
        let m = MatrixConvention {
            recover_skeleton_shift_bytes: true,
            ..MatrixConvention::DEFAULT
        };
        assert_eq!(m.shift_scale(0x0300), 1.0 / 4096.0);
        assert_eq!(m.shift_scale(5), 1.0 / 1024.0);
    }

    #[test]
    fn game_layout_mapping_keeps_v2_games_distinct() {
        use crate::level_layout::LevelLayout;
        assert_eq!(Game::R2.layout(), LevelLayout::V2);
        assert_eq!(Game::R3.layout(), LevelLayout::V2);
        assert_eq!(Game::FFA.layout(), LevelLayout::V2);
        assert_eq!(Game::Tod.layout(), LevelLayout::Tod);
        assert_eq!(Game::Rfom.layout(), LevelLayout::Rfom);
        assert_ne!(
            Game::R2.anim_profile().apply_blend_mask_rotation_gate,
            Game::R3.anim_profile().apply_blend_mask_rotation_gate
        );
    }

    #[test]
    fn legacy_profile_falls_back_to_default_convention() {
        let m = AnimProfile::LEGACY.matrix_convention();
        assert!(!m.recover_skeleton_shift_bytes);
        assert!(!m.propagate_scale_frames);
        assert_eq!(m.yard_to_meter, 0.9144);
    }
}
