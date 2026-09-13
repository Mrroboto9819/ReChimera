use std::collections::HashMap;
use std::path::Path;

use crate::error::Result;
use crate::game::{Game, GameProfile};
use crate::gameplay::GameplayLayout;
use crate::level_layout::LevelLayout;
use crate::moby::MobyAsset;
use crate::reader::{moby_reader_for_layout, tie_reader_for_layout};
use crate::shader::ShaderInfo;
use crate::texture::Texture;
use crate::tie::TieAsset;
use crate::zone::Zone;

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

    pub fn read_shaders(&self, folder: &Path) -> Result<HashMap<u64, ShaderInfo>> {
        match self.profile.layout {
            LevelLayout::V2 => crate::shader::read_shaders(folder),
            LevelLayout::Tod => crate::shader_old::read_shaders_old(folder),
            LevelLayout::Rfom => crate::shader_rfom::read_shaders_rfom(folder),
        }
    }

    pub fn read_gameplay(&self, folder: &Path) -> Result<GameplayLayout> {
        match self.profile.layout {
            LevelLayout::V2 => crate::gameplay::read_gameplay(folder),
            LevelLayout::Tod => crate::gameplay_old::read_gameplay_old(folder),
            LevelLayout::Rfom => crate::gameplay_rfom::read_gameplay_rfom(folder),
        }
    }

    pub fn read_zones(&self, folder: &Path) -> Result<Vec<Zone>> {
        match self.profile.layout {
            LevelLayout::V2 => crate::zone::read_zones(folder),
            LevelLayout::Tod => crate::zone_old::read_zones_old(folder),
            LevelLayout::Rfom => crate::region_rfom::read_regions_rfom(folder),
        }
    }

    pub fn read_textures(&self, folder: &Path) -> Result<Vec<Texture>> {
        match self.profile.layout {
            LevelLayout::V2 => crate::texture::read_textures(folder),
            LevelLayout::Tod => crate::texture_old::read_textures_old(folder),
            LevelLayout::Rfom => crate::texture_rfom::read_textures_rfom(folder),
        }
    }
}
