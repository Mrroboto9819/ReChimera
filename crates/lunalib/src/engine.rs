use std::path::Path;

use crate::error::Result;
use crate::game::{Game, GameProfile};
use crate::level_layout::LevelLayout;
use crate::moby::MobyAsset;
use crate::reader::{moby_reader_for_layout, tie_reader_for_layout};
use crate::tie::TieAsset;

pub struct GameEngine {
    profile: GameProfile,
}

pub fn engine_for(game: Game) -> GameEngine {
    GameEngine {
        profile: game.profile(),
    }
}

pub fn engine_for_layout(layout: LevelLayout) -> GameEngine {
    GameEngine {
        profile: GameProfile::legacy(layout),
    }
}

impl GameEngine {
    pub fn from_profile(profile: GameProfile) -> Self {
        GameEngine { profile }
    }

    pub fn profile(&self) -> GameProfile {
        self.profile
    }

    pub fn game(&self) -> Option<Game> {
        self.profile.game
    }

    pub fn layout(&self) -> LevelLayout {
        self.profile.layout
    }

    pub fn read_mobys(
        &self,
        folder: &Path,
        tuids: Option<&[u64]>,
        on_total: &mut dyn FnMut(usize),
        on_each: &mut dyn FnMut(MobyAsset),
    ) -> Result<()> {
        moby_reader_for_layout(self.profile.layout).read(folder, tuids, on_total, on_each)
    }

    pub fn read_ties(
        &self,
        folder: &Path,
        tuids: Option<&[u64]>,
        on_total: &mut dyn FnMut(usize),
        on_each: &mut dyn FnMut(TieAsset),
    ) -> Result<()> {
        tie_reader_for_layout(self.profile.layout).read(folder, tuids, on_total, on_each)
    }
}
