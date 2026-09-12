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
            },
            Self::R2 => AnimProfile {
                game: Some(self),
                apply_delta_pos_scale: true,
                apply_blend_mask_rotation_gate: false,
            },
            Self::Rfom | Self::Tod | Self::A4O | Self::ACiT | Self::FFA => AnimProfile {
                game: Some(self),
                apply_delta_pos_scale: false,
                apply_blend_mask_rotation_gate: false,
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
}

impl AnimProfile {
    pub const LEGACY: AnimProfile = AnimProfile {
        game: None,
        apply_delta_pos_scale: true,
        apply_blend_mask_rotation_gate: true,
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
