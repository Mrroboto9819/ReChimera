


use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use crate::error::{Error, Result};
use crate::igfile::IgFile;

const SECT_GAMEPLAY_STRINGS: u32 = 0x25000;
const SECT_PRIUS_MOBY_INSTANCES: u32 = 0x25048;
const SECT_PRIUS_MOBY_METADATA: u32 = 0x2504C;
const SECT_REGION_MOBY_TUIDS: u32 = 0x1C600;

const MOBY_INSTANCE_SIZE: u64 = 0x50;
const MOBY_METADATA_SIZE: u64 = 0x10;

#[derive(Debug, Clone)]
pub struct GameplayLayout {
    pub regions: Vec<Region>,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub name: String,
    pub moby_instances: Vec<MobyInstance>,
}

#[derive(Debug, Clone)]
pub struct MobyInstance {

    pub moby_tuid: u64,

    pub instance_tuid: u64,
    pub name: String,
    pub position: [f32; 3],

    pub rotation: [f32; 3],
    pub scale: f32,
    pub group: u16,
}


pub fn read_gameplay(level_folder: &Path) -> Result<GameplayLayout> {
    let gameplay_path = level_folder.join("gameplay.dat");
    let gp_file = File::open(&gameplay_path)?;
    let mut gameplay = IgFile::open(BufReader::new(gp_file))?;
    let log = std::env::var("RECHIMERA_LOG_PROBES").is_ok();

    let region_names = read_region_names(&mut gameplay)?;
    if log {
        eprintln!(
            "[v2-gp] gameplay.dat region names: {:?}",
            region_names
        );
    }

    let mut regions = Vec::with_capacity(region_names.len());
    let mut total_placements = 0usize;
    for name in region_names {
        match read_region(level_folder, &name) {
            Ok(region) => {
                if log {
                    eprintln!(
                        "[v2-gp]   region '{}': {} moby placements",
                        name,
                        region.moby_instances.len()
                    );
                    let sample: Vec<String> = region
                        .moby_instances
                        .iter()
                        .take(10)
                        .map(|inst| format!("0x{:016X}", inst.moby_tuid))
                        .collect();
                    eprintln!(
                        "[v2-gp]   region '{}' first 10 placement tuids: {:?}",
                        name, sample
                    );
                    let unique: std::collections::HashSet<u64> = region
                        .moby_instances
                        .iter()
                        .map(|inst| inst.moby_tuid)
                        .collect();
                    let zero_count = region
                        .moby_instances
                        .iter()
                        .filter(|inst| inst.moby_tuid == 0)
                        .count();
                    eprintln!(
                        "[v2-gp]   region '{}' unique placement tuids: {} ({} are 0x0000…)",
                        name,
                        unique.len(),
                        zero_count
                    );
                }
                total_placements += region.moby_instances.len();
                regions.push(region);
            }
            Err(e) => {
                eprintln!(
                    "[v2-gp]   region '{}': SKIPPED — {}",
                    name, e
                );
            }
        }
    }
    if log {
        eprintln!(
            "[v2-gp] total moby placements across {} regions: {}",
            regions.len(),
            total_placements
        );
    }
    Ok(GameplayLayout { regions })
}


fn read_region_names<R: Read + Seek>(gameplay: &mut IgFile<R>) -> Result<Vec<String>> {
    let str_section = gameplay.require_section(SECT_GAMEPLAY_STRINGS)?;
    let bytes = u64::from(str_section.count);
    if bytes < 0x10 {
        return Err(Error::SectionLengthMismatch {
            id: SECT_GAMEPLAY_STRINGS,
            length: str_section.count,
            entry: 0x10,
        });
    }
    let descriptor_offset = u64::from(str_section.offset) + bytes - 0x10;

    gameplay.stream.seek_to(descriptor_offset)?;
    let region_count = gameplay.stream.read_u32()?;
    let region_table_offset = u64::from(gameplay.stream.read_u32()?);

    let mut names = Vec::with_capacity(region_count as usize);
    for i in 0..region_count {
        gameplay
            .stream
            .seek_to(region_table_offset + 4 * u64::from(i))?;
        let name_offset = u64::from(gameplay.stream.read_u32()?);
        let name = gameplay.stream.read_cstring_at(name_offset)?;
        names.push(name);
    }
    Ok(names)
}

fn read_region(level_folder: &Path, name: &str) -> Result<Region> {
    let region_dir = level_folder.join(name);

    let prius_path = region_dir.join("gp_prius.dat");
    let region_path = region_dir.join("region.dat");

    let mut prius = IgFile::open(BufReader::new(File::open(&prius_path)?))?;
    let mut region = IgFile::open(BufReader::new(File::open(&region_path)?))?;

    let inst_section = prius.require_section(SECT_PRIUS_MOBY_INSTANCES)?;
    let meta_section = prius.require_section(SECT_PRIUS_MOBY_METADATA)?;
    let tuid_section = region.require_section(SECT_REGION_MOBY_TUIDS)?;

    if std::env::var("RECHIMERA_LOG_PROBES").is_ok() {
        eprintln!(
            "[v2-gp-probe] inst section 0x{:X}: offset=0x{:X} count={} length={}",
            SECT_PRIUS_MOBY_INSTANCES,
            inst_section.offset, inst_section.count, inst_section.length
        );
        eprintln!(
            "[v2-gp-probe] meta section 0x{:X}: offset=0x{:X} count={} length={}",
            SECT_PRIUS_MOBY_METADATA,
            meta_section.offset, meta_section.count, meta_section.length
        );
        eprintln!(
            "[v2-gp-probe] region tuid table 0x{:X}: offset=0x{:X} count={} length={}",
            SECT_REGION_MOBY_TUIDS,
            tuid_section.offset, tuid_section.count, tuid_section.length
        );
    }

    let count = inst_section.count as usize;


    let mut raw_instances: Vec<RawMobyInstance> = Vec::with_capacity(count);
    for i in 0..count {
        let base = u64::from(inst_section.offset) + (i as u64) * MOBY_INSTANCE_SIZE;
        prius.stream.seek_to(base + 0x00)?;
        let moby_index = prius.stream.read_u16()?;
        let _group_index = prius.stream.read_u16()?;
        prius.stream.seek_to(base + 0x14)?;
        let position = prius.stream.read_vec3()?;
        prius.stream.seek_to(base + 0x20)?;
        let rotation = prius.stream.read_vec3()?;
        prius.stream.seek_to(base + 0x2C)?;
        let scale = prius.stream.read_f32()?;
        raw_instances.push(RawMobyInstance {
            moby_index,
            position,
            rotation,
            scale,
        });
    }


    let mut raw_metadata: Vec<RawMobyMetadata> = Vec::with_capacity(count);
    for i in 0..count {
        let base = u64::from(meta_section.offset) + (i as u64) * MOBY_METADATA_SIZE;
        prius.stream.seek_to(base + 0x00)?;
        let instance_tuid = prius.stream.read_u64()?;
        let name_ptr = u64::from(prius.stream.read_u32()?);
        let group = prius.stream.read_u16()?;
        raw_metadata.push(RawMobyMetadata {
            instance_tuid,
            name_ptr,
            group,
        });
    }


    let names: Vec<String> = raw_metadata
        .iter()
        .map(|m| prius.stream.read_cstring_at(m.name_ptr))
        .collect::<Result<_>>()?;


    let mut moby_instances = Vec::with_capacity(count);
    for ((raw, meta), name) in raw_instances
        .iter()
        .zip(raw_metadata.iter())
        .zip(names.into_iter())
    {
        region
            .stream
            .seek_to(u64::from(tuid_section.offset) + 8 * u64::from(raw.moby_index))?;
        let moby_tuid = region.stream.read_u64()?;
        moby_instances.push(MobyInstance {
            moby_tuid,
            instance_tuid: meta.instance_tuid,
            name,
            position: raw.position,
            rotation: raw.rotation,
            scale: raw.scale,
            group: meta.group,
        });
    }

    Ok(Region {
        name: name.to_string(),
        moby_instances,
    })
}

struct RawMobyInstance {
    moby_index: u16,
    position: [f32; 3],
    rotation: [f32; 3],
    scale: f32,
}

struct RawMobyMetadata {
    instance_tuid: u64,
    name_ptr: u64,
    group: u16,
}
