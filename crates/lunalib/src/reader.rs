use std::path::Path;

use crate::error::Result;
use crate::game::Game;
use crate::level_layout::LevelLayout;
use crate::moby::{read_moby_assets_with_total, MobyAsset};
use crate::moby_old::read_moby_assets_old_with_total;
use crate::moby_rfom::read_moby_assets_rfom_with_total;

pub trait MobyReader {
    fn read(
        &self,
        folder: &Path,
        tuids: Option<&[u64]>,
        on_total: &mut dyn FnMut(usize),
        on_each: &mut dyn FnMut(MobyAsset),
    ) -> Result<()>;
}

struct V2MobyReader;
struct TodMobyReader;
struct RfomMobyReader;

impl MobyReader for V2MobyReader {
    fn read(
        &self,
        folder: &Path,
        tuids: Option<&[u64]>,
        on_total: &mut dyn FnMut(usize),
        on_each: &mut dyn FnMut(MobyAsset),
    ) -> Result<()> {
        read_moby_assets_with_total(folder, tuids, |n| on_total(n), |a| on_each(a))
    }
}

impl MobyReader for TodMobyReader {
    fn read(
        &self,
        folder: &Path,
        _tuids: Option<&[u64]>,
        on_total: &mut dyn FnMut(usize),
        on_each: &mut dyn FnMut(MobyAsset),
    ) -> Result<()> {
        read_moby_assets_old_with_total(folder, |n| on_total(n), |a| on_each(a))
    }
}

impl MobyReader for RfomMobyReader {
    fn read(
        &self,
        folder: &Path,
        _tuids: Option<&[u64]>,
        on_total: &mut dyn FnMut(usize),
        on_each: &mut dyn FnMut(MobyAsset),
    ) -> Result<()> {
        read_moby_assets_rfom_with_total(folder, |n| on_total(n), |a| on_each(a))
    }
}

pub fn moby_reader(game: Game) -> Box<dyn MobyReader> {
    reader_for_layout(game.layout())
}

pub fn moby_reader_for_layout(layout: LevelLayout) -> Box<dyn MobyReader> {
    reader_for_layout(layout)
}

fn reader_for_layout(layout: LevelLayout) -> Box<dyn MobyReader> {
    match layout {
        LevelLayout::V2 => Box::new(V2MobyReader),
        LevelLayout::Tod => Box::new(TodMobyReader),
        LevelLayout::Rfom => Box::new(RfomMobyReader),
    }
}
