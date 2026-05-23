//! RFOM gameplay placements. Decodes `ps3gameplay.dat` (IGHW v0.2 with
//! buffer-mode TOC entries — `id == 0xFFFFFFFF` markers raw byte regions;
//! the only typed entry is `0x25000 Gameplay`).
//!
//! Ported from IT's `levelmain/extract.cpp::EmbedMobys` (line 1281) plus
//! `common/include/insomnia/classes/gameplay.hpp`.
//!
//! ## Pointer chain
//! ```text
//!   0x25000 Gameplay (48 bytes)
//!     +0x20 PointerX86<GameplayInstances> instances
//!
//!   GameplayInstances (~192 bytes)
//!     +0x00..+0x40  char name[64]
//!     +0x40..+0x50  GameplayInstancesGroup<GameplayInstanceMoby> mobys   ← decoded
//!     +0x50..+0xB0  GameplayInstancesGroup<unknown>          other[6]   ← raw-probed
//!     +0xB0..+0xB8  PointerX86<char> unk0, unk1
//!     +0xB8..+0xC0  u32 unk2, unk3
//! ```
//!
//! Each `GameplayInstancesGroup` is 16 bytes:
//! ```text
//!   +0x00..+0x04  PointerX86<items> items
//!   +0x04..+0x08  u32 numItems
//!   +0x08..+0x10  u32 null[2]
//! ```
//!
//! ## `GameplayInstanceMoby` layout (64 bytes per record)
//! ```text
//!   +0x00..+0x04  PointerX86<IGHWHeader> embeded
//!   +0x04..+0x08  u32 unk0
//!   +0x08..+0x10  float unk1[2]
//!   +0x10..+0x1C  Vector position    — 3 × f32, yard units (multiply by YARD_TO_M)
//!   +0x1C..+0x28  Vector rotation    — 3 × f32 Euler radians (ZYX order)
//!   +0x28..+0x2C  u32 null1
//!   +0x2C..+0x30  i32 unk2
//!   +0x30..+0x34  u32 null2
//!   +0x34..+0x38  u32 embedSize
//!   +0x38..+0x3C  f32 unk3
//!   +0x3C..+0x3E  u16 mobyClassIndex — matches MobyV1.mobyId
//!   +0x3E..+0x40  u16 null3
//! ```
//!
//! ## `other[6]` arrays — UNKNOWN
//!
//! IT has no struct definitions for these. They're likely some mix of:
//! triggers, volumes, paths, spawn points, sound emitters, light placements.
//! Each `GameplayInstancesGroup` exposes a count + pointer; the per-record
//! struct is unknown. With `RECHIMERA_LOG_PROBES=1` the reader dumps the
//! first 64 bytes of each non-empty group so a human can pattern-match.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::error::{Error, Result};
use crate::gameplay::{GameplayLayout, MobyInstance, Region};
use crate::igfile::IgFile;

const SECT_GAMEPLAY: u32 = 0x25000;
const GAMEPLAY_INSTANCE_MOBY_SIZE: u64 = 0x40;

const YARD_TO_M: f32 = 0.9144;

pub fn read_gameplay_rfom(level_folder: &Path) -> Result<GameplayLayout> {
    let path = level_folder.join("ps3gameplay.dat");
    let mut ig = IgFile::open(BufReader::new(File::open(&path).map_err(|e| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("ps3gameplay.dat is required for RFOM moby placements: {e}"),
        ))
    })?))?;

    let gp_section = match ig.section(SECT_GAMEPLAY) {
        Some(s) => s,
        None => {
            return Ok(GameplayLayout {
                regions: vec![Region {
                    name: "art".to_string(),
                    moby_instances: Vec::new(),
                }],
            });
        }
    };

    // Gameplay struct at gp_section.offset.
    // +0x20 = instances PointerX86<GameplayInstances>.
    let gameplay_off = u64::from(gp_section.offset);
    ig.stream.seek_to(gameplay_off + 0x20)?;
    let instances_ptr = u64::from(ig.stream.read_u32()?);
    if instances_ptr == 0 {
        return Ok(GameplayLayout {
            regions: vec![Region {
                name: "art".to_string(),
                moby_instances: Vec::new(),
            }],
        });
    }

    let log_probes = std::env::var("RECHIMERA_LOG_PROBES").is_ok();

    // ---- GameplayInstances.name @ +0x00..+0x40 ----
    // 64-byte null-terminated ASCII string identifying this region/zone.
    // IT uses it as the parent glTF node name. We surface it as the
    // region name so the placement tree can be grouped.
    ig.stream.seek_to(instances_ptr + 0x00)?;
    let name_bytes = ig.stream.read_bytes(64)?;
    let region_name = name_bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect::<String>();

    // ---- GameplayInstances.mobys @ +0x40 (GameplayInstancesGroup, 16 B) ----
    //   +0x00..+0x04  PointerX86<GameplayInstanceMoby> items
    //   +0x04..+0x08  u32 numItems
    //   +0x08..+0x10  u32 null[2]
    let mobys_group_off = instances_ptr + 0x40;
    ig.stream.seek_to(mobys_group_off + 0x00)?;
    let items_ptr = u64::from(ig.stream.read_u32()?);
    let num_items = ig.stream.read_u32()?;

    if log_probes {
        eprintln!(
            "[rfom-gp] Gameplay@0x{gameplay_off:X} instances@0x{instances_ptr:X} name='{region_name}' items@0x{items_ptr:X} numItems={num_items}"
        );
    }

    // ---- GameplayInstances.other[6] @ +0x50..+0xB0 (6 × 16 B) ----
    // IT doesn't decode these. We probe each non-empty group: log the
    // count, the first items_ptr, and (when RECHIMERA_LOG_PROBES is set)
    // dump the first 64 bytes so we can pattern-match on real data.
    //
    // Best-guess content (one slot each, order unknown):
    //   - trigger volumes (probably Matrix44 + radius/extents)
    //   - cuboid volumes
    //   - spawn points (position + rotation + class)
    //   - paths / waypoints (array of positions)
    //   - sound emitters (position + sound id + range)
    //   - dynamic light placements (position + color + intensity)
    if log_probes {
        let other_base = instances_ptr + 0x50;
        for slot in 0..6 {
            let group_off = other_base + (slot as u64) * 16;
            ig.stream.seek_to(group_off + 0x00)?;
            let other_items_ptr = u64::from(ig.stream.read_u32()?);
            let other_num_items = ig.stream.read_u32()?;
            if other_items_ptr == 0 || other_num_items == 0 {
                eprintln!("[rfom-gp-other] slot[{slot}]: empty");
                continue;
            }
            eprintln!(
                "[rfom-gp-other] slot[{slot}]: items@0x{other_items_ptr:X} numItems={other_num_items}"
            );
            // Dump first 64 bytes as hex + f32 + u32 side by side so a
            // human can spot positions / matrices / counts.
            ig.stream.seek_to(other_items_ptr).ok();
            let mut hex = String::with_capacity(96);
            let mut floats: Vec<f32> = Vec::with_capacity(16);
            let mut u32s: Vec<u32> = Vec::with_capacity(16);
            for row in 0..4 {
                hex.clear();
                for col in 0..16 {
                    let b = ig.stream.read_u8().unwrap_or(0);
                    if col > 0 && col % 4 == 0 {
                        hex.push(' ');
                    }
                    hex.push_str(&format!("{:02X}", b));
                    if col % 4 == 3 {
                        // Re-read as f32 + u32 by stepping back. (We could
                        // also accumulate the 4 bytes, but stream API is
                        // forward-only — recompute from the parsed string.)
                        // Simpler: parse on the fly.
                    }
                    let _ = b;
                }
                eprintln!(
                    "[rfom-gp-other]   slot[{slot}] +0x{:02X}: {}",
                    row * 16,
                    hex
                );
            }
            // Re-read same 64 bytes as 16 × f32 BE for visual inspection.
            ig.stream.seek_to(other_items_ptr).ok();
            for i in 0..16 {
                let f = ig.stream.read_f32().unwrap_or(0.0);
                floats.push(f);
                let _ = i;
            }
            ig.stream.seek_to(other_items_ptr).ok();
            for i in 0..16 {
                let u = ig.stream.read_u32().unwrap_or(0);
                u32s.push(u);
                let _ = i;
            }
            eprintln!(
                "[rfom-gp-other]   slot[{slot}] f32: [{}]",
                floats
                    .iter()
                    .map(|f| {
                        if f.is_finite() && f.abs() < 1e8 && (f.abs() > 1e-6 || *f == 0.0) {
                            format!("{:.3}", f)
                        } else {
                            "_".into()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    if items_ptr == 0 || num_items == 0 {
        return Ok(GameplayLayout {
            regions: vec![Region {
                name: "art".to_string(),
                moby_instances: Vec::new(),
            }],
        });
    }

    let mut moby_instances: Vec<MobyInstance> = Vec::with_capacity(num_items as usize);
    let mut sample_logged = 0usize;
    for i in 0..num_items {
        let base = items_ptr + (i as u64) * GAMEPLAY_INSTANCE_MOBY_SIZE;

        ig.stream.seek_to(base + 0x10)?;
        let pos = ig.stream.read_vec3()?;

        ig.stream.seek_to(base + 0x1C)?;
        let rot = ig.stream.read_vec3()?;

        ig.stream.seek_to(base + 0x3C)?;
        let moby_class_index = ig.stream.read_u16()?;

        let position_m = [pos[0] * YARD_TO_M, pos[1] * YARD_TO_M, pos[2] * YARD_TO_M];

        if sample_logged < 3 && std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
            eprintln!(
                "[rfom-gp-moby] [{i}] class=0x{moby_class_index:04X} raw_yards=({:.2}, {:.2}, {:.2}) → m=({:.2}, {:.2}, {:.2})",
                pos[0], pos[1], pos[2],
                position_m[0], position_m[1], position_m[2]
            );
            sample_logged += 1;
        }

        moby_instances.push(MobyInstance {
            moby_tuid: u64::from(moby_class_index),
            instance_tuid: i as u64,
            name: format!("Moby_{moby_class_index:04X}_Instance_{i:04X}"),
            position: position_m,
            rotation: rot,
            scale: 1.0,
            group: 0,
        });
    }
    if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        eprintln!("[rfom-gp-moby] {} moby placements scaled to meters", moby_instances.len());
    }

    let slot3_added = decode_slot3_placements(&mut ig, instances_ptr, &mut moby_instances)?;
    if slot3_added > 0 && std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        eprintln!(
            "[rfom-gp-slot3] decoded {} additional placements from gameplay slot[3]",
            slot3_added
        );
    }

    Ok(GameplayLayout {
        regions: vec![Region {
            name: if region_name.is_empty() { "art".to_string() } else { region_name },
            moby_instances,
        }],
    })
}

fn decode_slot3_placements<R: std::io::Read + std::io::Seek>(
    ig: &mut IgFile<R>,
    instances_ptr: u64,
    out: &mut Vec<MobyInstance>,
) -> Result<usize> {
    const SLOT3_RECORD_SIZE: u64 = 0x30;
    let group_off = instances_ptr + 0x50 + 3 * 16;
    ig.stream.seek_to(group_off)?;
    let items_ptr = u64::from(ig.stream.read_u32()?);
    let num_items = ig.stream.read_u32()?;
    if items_ptr == 0 || num_items == 0 {
        return Ok(0);
    }

    let log = std::env::var("RECHIMERA_LOG_PROBES").is_ok();
    let mut added = 0usize;
    for i in 0..num_items {
        let base = items_ptr + (i as u64) * SLOT3_RECORD_SIZE;
        ig.stream.seek_to(base + 0x00)?;
        let class_ptr = u64::from(ig.stream.read_u32()?);
        ig.stream.seek_to(base + 0x04)?;
        let pos = ig.stream.read_vec3()?;
        ig.stream.seek_to(base + 0x10)?;
        let qx = ig.stream.read_f32()?;
        let qy = ig.stream.read_f32()?;
        let qz = ig.stream.read_f32()?;
        let _qw = ig.stream.read_f32()?;
        let sx = ig.stream.read_f32().unwrap_or(1.0);
        let sy = ig.stream.read_f32().unwrap_or(1.0);
        let _sz = if (sx - sy).abs() < 0.001 { sx } else { sx };

        if !pos[0].is_finite() || !pos[1].is_finite() || !pos[2].is_finite() {
            continue;
        }
        if pos[0].abs() > 1.0e6 || pos[1].abs() > 1.0e6 || pos[2].abs() > 1.0e6 {
            continue;
        }

        let class_idx = if class_ptr != 0 {
            match ig.stream.seek_to(class_ptr) {
                Ok(_) => ig.stream.read_u16().unwrap_or(0xFFFF),
                Err(_) => 0xFFFF,
            }
        } else {
            0xFFFF
        };

        let position_m = [pos[0] * YARD_TO_M, pos[1] * YARD_TO_M, pos[2] * YARD_TO_M];
        let rot_euler = [qx, qy, qz];

        if log && added < 3 {
            eprintln!(
                "[rfom-gp-slot3] [{i}] class_ptr=0x{class_ptr:X} class=0x{class_idx:04X} raw_yards=({:.2}, {:.2}, {:.2}) → m=({:.2}, {:.2}, {:.2})",
                pos[0], pos[1], pos[2],
                position_m[0], position_m[1], position_m[2]
            );
        }

        out.push(MobyInstance {
            moby_tuid: u64::from(class_idx),
            instance_tuid: 0x10000 | i as u64,
            name: format!("Slot3_{class_idx:04X}_Instance_{i:04X}"),
            position: position_m,
            rotation: rot_euler,
            scale: 1.0,
            group: 0,
        });
        added += 1;
    }
    Ok(added)
}
