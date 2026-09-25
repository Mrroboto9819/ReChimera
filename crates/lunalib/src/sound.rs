


use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use rayon::prelude::*;

use crate::error::Result;
use crate::igfile::IgFile;


pub const SECT_SOUND_BANK: u32 = 0x21000;

pub const SECT_SOUND_STREAMS_V1: u32 = 0x21010;
pub const SECT_SOUND_NAMES_V1: u32 = 0x21100;
pub const SECT_SOUNDS_V1: u32 = 0x21200;

pub const SECT_SOUND_STREAMS_V2: u32 = 0x21100;
pub const SECT_SOUND_NAMES_V2: u32 = 0x21200;
pub const SECT_SOUNDS_V2: u32 = 0x21300;


pub const SECT_SOUND_STREAMS: u32 = SECT_SOUND_STREAMS_V1;
pub const SECT_SOUND_NAMES: u32 = SECT_SOUND_NAMES_V1;
pub const SECT_SOUNDS: u32 = SECT_SOUNDS_V1;

use std::cell::Cell;

thread_local! {
    static SCREAM_DIAG_FIRED: Cell<bool> = const { Cell::new(false) };
    static SCREAM_ENTRY_DIAG_FIRED: Cell<bool> = const { Cell::new(false) };
    static SCREAM_GAIN_DIAG_FIRED: Cell<bool> = const { Cell::new(false) };
    static SCREAM_WAVE_DIAG_FIRED: Cell<bool> = const { Cell::new(false) };
    static SCREAM_RESULT_DIAG_FIRED: Cell<bool> = const { Cell::new(false) };
}

pub fn reset_scream_diag() {
    SCREAM_DIAG_FIRED.with(|c| c.set(false));
    SCREAM_ENTRY_DIAG_FIRED.with(|c| c.set(false));
    SCREAM_GAIN_DIAG_FIRED.with(|c| c.set(false));
    SCREAM_WAVE_DIAG_FIRED.with(|c| c.set(false));
    SCREAM_RESULT_DIAG_FIRED.with(|c| c.set(false));
}

fn scream_diag(args: std::fmt::Arguments) {
    SCREAM_DIAG_FIRED.with(|c| {
        if !c.replace(true) {
            eprintln!("[scream] {}", args);
        }
    });
}
fn scream_entry_diag(args: std::fmt::Arguments) {
    SCREAM_ENTRY_DIAG_FIRED.with(|c| {
        if !c.replace(true) {
            eprintln!("[scream-entry] {}", args);
        }
    });
}


#[derive(Debug, Clone, Copy)]
pub struct SoundIds {
    pub bank: u32,
    pub streams: u32,
    pub names: u32,
    pub sounds: u32,
}


pub fn detect_sound_ids<R: Read + Seek>(ig: &mut IgFile<R>) -> SoundIds {
    if ig.section(SECT_SOUNDS_V2).is_some() {
        SoundIds {
            bank: SECT_SOUND_BANK,
            streams: SECT_SOUND_STREAMS_V2,
            names: SECT_SOUND_NAMES_V2,
            sounds: SECT_SOUNDS_V2,
        }
    } else {
        SoundIds {
            bank: SECT_SOUND_BANK,
            streams: SECT_SOUND_STREAMS_V1,
            names: SECT_SOUND_NAMES_V1,
            sounds: SECT_SOUNDS_V1,
        }
    }
}


#[derive(Debug, Clone)]
pub struct ExtractedSound {

    pub name: String,

    pub index: usize,

    pub gain_index: u8,

    pub sample_rate: u32,

    pub channels: u16,

    pub sample_count: u32,

    pub wav: Vec<u8>,
}


#[derive(Debug, Clone)]
pub struct SoundSummary {
    pub name: String,
    pub index: usize,
    pub kind: SoundKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundKind {

    Bank,

    Stream,
}


const ADPCM_TABLE: [[i32; 2]; 5] = [[0, 0], [60, 0], [115, -52], [98, -55], [122, -60]];


pub fn decode_adpcm_block(
    data: &[u8; 16],
    samples_out: &mut [i16; 28],
    prev_sample: &mut i32,
    pp_sample: &mut i32,
) {
    let filter = (data[0] >> 4) as usize;
    let shift = (data[0] & 0xF) as u32;
    let flags = data[1] & 0x7;

    if flags == 0x7 {
        *samples_out = [0; 28];
        *prev_sample = 0;
        *pp_sample = 0;
        return;
    }

    let filter = filter.min(4);
    let coef0 = ADPCM_TABLE[filter][0];
    let coef1 = ADPCM_TABLE[filter][1];

    for i in 0..28 {

        let raw = data[2 + i / 2] as i32;
        let nibble = (raw << (28 - (i as i32 % 2) * 4)) >> 28;

        let scaled = (nibble * (1 << 12)).wrapping_shr(shift);
        let predicted = ((*prev_sample) * coef0 + (*pp_sample) * coef1) / 64;
        let sample = (scaled + predicted).clamp(i16::MIN as i32, i16::MAX as i32);
        *pp_sample = *prev_sample;
        *prev_sample = sample;
        samples_out[i] = sample as i16;
    }
}


pub fn decode_adpcm_stream(blocks: &[u8]) -> Vec<i16> {
    let num_blocks = blocks.len() / 16;
    let mut out = Vec::with_capacity(num_blocks * 28);
    let mut prev = 0i32;
    let mut pp = 0i32;
    let mut buf = [0i16; 28];
    let mut block = [0u8; 16];
    for b in 0..num_blocks {
        block.copy_from_slice(&blocks[b * 16..b * 16 + 16]);
        decode_adpcm_block(&block, &mut buf, &mut prev, &mut pp);
        out.extend_from_slice(&buf);
    }
    out
}


pub fn write_wav_pcm16_mono(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    write_wav_pcm16(samples, sample_rate, 1)
}


pub fn write_wav_pcm16(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_size = samples.len() * 2;
    let total = 36 + data_size;
    let block_align = channels * 2;
    let byte_rate = sample_rate * u32::from(block_align);
    let mut out = Vec::with_capacity(8 + total);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_size as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}


const NOTE_PITCH_TABLE: [u16; 134] = [
    0x8000, 0x879C, 0x8FAC, 0x9837, 0xA145, 0xAADC, 0xB504, 0xBFC8, 0xCB2F, 0xD744, 0xE411, 0xF1A1,
    0x8000, 0x800E, 0x801D, 0x802C, 0x803B, 0x804A, 0x8058, 0x8067, 0x8076, 0x8085, 0x8094, 0x80A3,
    0x80B1, 0x80C0, 0x80CF, 0x80DE, 0x80ED, 0x80FC, 0x810B, 0x811A, 0x8129, 0x8138, 0x8146, 0x8155,
    0x8164, 0x8173, 0x8182, 0x8191, 0x81A0, 0x81AF, 0x81BE, 0x81CD, 0x81DC, 0x81EB, 0x81FA, 0x8209,
    0x8218, 0x8227, 0x8236, 0x8245, 0x8254, 0x8263, 0x8272, 0x8282, 0x8291, 0x82A0, 0x82AF, 0x82BE,
    0x82CD, 0x82DC, 0x82EB, 0x82FA, 0x830A, 0x8319, 0x8328, 0x8337, 0x8346, 0x8355, 0x8364, 0x8374,
    0x8383, 0x8392, 0x83A1, 0x83B0, 0x83C0, 0x83CF, 0x83DE, 0x83ED, 0x83FD, 0x840C, 0x841B, 0x842A,
    0x843A, 0x8449, 0x8458, 0x8468, 0x8477, 0x8486, 0x8495, 0x84A5, 0x84B4, 0x84C3, 0x84D3, 0x84E2,
    0x84F1, 0x8501, 0x8510, 0x8520, 0x852F, 0x853E, 0x854E, 0x855D, 0x856D, 0x857C, 0x858B, 0x859B,
    0x85AA, 0x85BA, 0x85C9, 0x85D9, 0x85E8, 0x85F8, 0x8607, 0x8617, 0x8626, 0x8636, 0x8645, 0x8655,
    0x8664, 0x8674, 0x8683, 0x8693, 0x86A2, 0x86B2, 0x86C1, 0x86D1, 0x86E0, 0x86F0, 0x8700, 0x870F,
    0x871F, 0x872E,
];

fn note_to_pitch(center_note: i8, center_fine: i8, note: i16, fine: i16) -> u16 {

    let ps1_note;
    let cn = if center_note >= 0 {
        ps1_note = true;
        center_note as i32
    } else {
        ps1_note = false;
        -(center_note as i32)
    };
    let cf = center_fine as i32;
    let n = note as i32;
    let f = fine as i32;

    let _fine = f.wrapping_add(cf as u16 as i32);
    let _fine2 = if _fine < 0 { _fine + 127 } else { _fine } / 128;
    let _note = n + _fine2 - cn;
    let mut val3 = _note / 6;
    if _note < 0 {
        val3 -= 1;
    }
    let offset2 = _fine - _fine2 * 128;
    let mut val2 = if _note < 0 { -1 } else { 0 };
    if val3 < 0 {
        val3 -= 1;
    }
    val2 = (val3 / 2) - val2;
    let mut val = val2 - 2;
    let mut offset1 = _note - val2 * 12;
    if offset1 < 0 || (offset1 == 0 && offset2 < 0) {
        offset1 += 12;
        val = val2 - 3;
    }
    let mut offset2 = offset2;
    let mut offset1 = offset1;
    if offset2 < 0 {
        offset1 = (offset1 - 1) + _fine2;
        offset2 += (_fine2 + 1) * 128;
    }
    let i1 = offset1.clamp(0, 11) as usize;
    let i2 = (offset2 + 12).clamp(0, 133) as usize;
    let mut ret = (NOTE_PITCH_TABLE[i1] as i32 * NOTE_PITCH_TABLE[i2] as i32) / 0x10000;
    if val < 0 {
        ret = (ret + (1i32 << (-val - 1))) >> -val;
    }
    let pitch = ret as u16;
    if ps1_note {
        ((0x10F4A_i32 * pitch as i32) >> 16) as u16
    } else {
        pitch
    }
}


fn sample_rate_for(center_note: i8, center_fine: i8) -> u32 {
    let pitch = note_to_pitch(center_note, center_fine, 0x3C, 0x00);

    ((pitch as f32) * 0.000_244_140_62 * 48000.0) as u32
}


pub fn list_sounds<R: Read + Seek>(ig: &mut IgFile<R>) -> Result<Vec<SoundSummary>> {
    let ids = detect_sound_ids(ig);
    let names_section = match ig.section(ids.names) {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };
    let sounds_section = match ig.section(ids.sounds) {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };


    ig.stream.seek_to(u64::from(sounds_section.offset))?;
    let num_sounds = ig.stream.read_u32()? as usize;
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let mut entries: Vec<(u16, i16)> = Vec::with_capacity(num_sounds);
    for _ in 0..num_sounds {
        let kind = ig.stream.read_u16()?;
        let idx = ig.stream.read_i16()?;
        entries.push((kind, idx));
    }


    let mut out = Vec::with_capacity(num_sounds);
    for (i, (kind, idx)) in entries.iter().enumerate() {
        if *idx < 0 {
            continue;
        }
        ig.stream.seek_to(u64::from(names_section.offset) + (i as u64) * 64)?;
        let buf = ig.stream.read_bytes(64)?;
        let nul = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        let name = String::from_utf8_lossy(&buf[..nul]).into_owned();
        out.push(SoundSummary {
            name,
            index: i,
            kind: if *kind == 0 {
                SoundKind::Bank
            } else {
                SoundKind::Stream
            },
        });
    }
    Ok(out)
}


pub fn extract_bank_sounds<R: Read + Seek>(
    ig: &mut IgFile<R>,
) -> Result<Vec<ExtractedSound>> {
    if ig.section(SECT_SOUNDS_V2).is_some() {
        extract_bank_sounds_v2(ig)
    } else {
        extract_bank_sounds_v1(ig)
    }
}

pub fn extract_bank_sounds_for_file<R: Read + Seek>(
    ig: &mut IgFile<R>,
    filename: &str,
) -> Result<Vec<ExtractedSound>> {
    let lower = filename.to_ascii_lowercase();
    let prefer_v1 =
        lower.starts_with("ps3sound") || lower.starts_with("ps3dialogue");
    let primary = if prefer_v1 {
        extract_bank_sounds_v1(ig)
    } else {
        extract_bank_sounds_v2(ig)
    };
    if let Ok(ref out) = primary {
        if !out.is_empty() {
            return primary;
        }
    }
    let fallback = if prefer_v1 {
        extract_bank_sounds_v2(ig)
    } else {
        extract_bank_sounds_v1(ig)
    };
    if let Ok(ref out) = fallback {
        if !out.is_empty() {
            eprintln!(
                "[scream] {} fell through {} primary path; using {} fallback ({} sounds)",
                filename,
                if prefer_v1 { "V1" } else { "V2" },
                if prefer_v1 { "V2" } else { "V1" },
                out.len()
            );
            return fallback;
        }
    }
    primary
}

fn extract_bank_sounds_v2<R: Read + Seek>(
    ig: &mut IgFile<R>,
) -> Result<Vec<ExtractedSound>> {
    extract_bank_sounds_with_offset(ig, 144, GainBase::BankData)
}

fn extract_bank_sounds_v1<R: Read + Seek>(
    ig: &mut IgFile<R>,
) -> Result<Vec<ExtractedSound>> {
    extract_bank_sounds_with_offset(ig, 144, GainBase::BankGainsArray)
}

#[derive(Clone, Copy, Debug)]
enum GainBase {
    BankData,
    BankGainsArray,
}

fn extract_bank_sounds_with_offset<R: Read + Seek>(
    ig: &mut IgFile<R>,
    header_skip: u64,
    gain_base: GainBase,
) -> Result<Vec<ExtractedSound>> {
    let ids = detect_sound_ids(ig);
    eprintln!(
        "[scream-step] entry: header_skip={} gain_base={:?} ids={{bank=0x{:X} names=0x{:X} sounds=0x{:X}}} all_sections={:?}",
        header_skip,
        gain_base,
        ids.bank, ids.names, ids.sounds,
        ig.sections.iter().map(|s| format!("0x{:X}@0x{:X}+{}", s.id, s.offset, s.length)).collect::<Vec<_>>()
    );
    let bank_section = match ig.section(ids.bank) {
        Some(s) => s,
        None => {
            scream_diag(format_args!("missing 0x{:X} SoundBank", ids.bank));
            return Ok(Vec::new());
        }
    };
    let names_section = match ig.section(ids.names) {
        Some(s) => s,
        None => {
            scream_diag(format_args!("missing 0x{:X} names", ids.names));
            return Ok(Vec::new());
        }
    };
    let sounds_section = match ig.section(ids.sounds) {
        Some(s) => s,
        None => {
            scream_diag(format_args!("missing 0x{:X} sounds", ids.sounds));
            return Ok(Vec::new());
        }
    };


    let bank_base = u64::from(bank_section.offset);
    let header_base = bank_base + header_skip;
    ig.stream.seek_to(header_base + 4)?;
    let num_sections = ig.stream.read_u32()? as usize;
    if num_sections < 2 {
        scream_diag(format_args!(
            "header_skip={} → numSections={} (<2), bailing",
            header_skip, num_sections
        ));
        return Ok(Vec::new());
    }
    let bank_data_ptr_rel = u64::from(ig.stream.read_u32()?);
    let _bank_size = ig.stream.read_u32()?;
    let data_data_ptr_rel = u64::from(ig.stream.read_u32()?);
    let _data_size = ig.stream.read_u32()?;
    let bank_data_pos = header_base + bank_data_ptr_rel;
    let data_data_pos = header_base + data_data_ptr_rel;


    ig.stream.seek_to(bank_data_pos + 0x16)?;
    let num_bank_sounds = ig.stream.read_u16()? as usize;
    let _num_bank_gains = ig.stream.read_u16()? as usize;
    let _unk0 = ig.stream.read_u16()?;
    let bank_sounds_ptr_rel = u64::from(ig.stream.read_u32()?);
    let _bank_gains_ptr_rel = u64::from(ig.stream.read_u32()?);
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let bank_gain_data_ptr_rel = u64::from(ig.stream.read_u32()?);
    let bank_sounds_pos = bank_data_pos + bank_sounds_ptr_rel;
    let bank_gain_data_pos = bank_data_pos + bank_gain_data_ptr_rel;


    ig.stream.seek_to(u64::from(sounds_section.offset))?;
    let num_sounds = ig.stream.read_u32()? as usize;
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let mut entries: Vec<(u16, i16)> = Vec::with_capacity(num_sounds);
    for _ in 0..num_sounds {
        let kind = ig.stream.read_u16()?;
        let idx = ig.stream.read_i16()?;
        entries.push((kind, idx));
    }

    let kind_zero_count = entries.iter().filter(|(k, _)| *k == 0).count();
    let kind_nonzero_count = entries.len() - kind_zero_count;
    let kind_sample: Vec<(u16, i16)> = entries.iter().take(8).copied().collect();

    let bank_gains_pos = bank_data_pos + _bank_gains_ptr_rel;
    let read_dump = |ig: &mut IgFile<R>, pos: u64, len: usize| -> String {
        if ig.stream.seek_to(pos).is_err() {
            return "(seek failed)".into();
        }
        match ig.stream.read_bytes(len) {
            Ok(bytes) => bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" "),
            Err(_) => "(read failed)".into(),
        }
    };
    let bank_dump = read_dump(ig, bank_data_pos, 64);
    let sounds_dump = read_dump(ig, bank_sounds_pos, 48);
    let bank_gains_dump = read_dump(ig, bank_gains_pos, 32);

    scream_diag(format_args!(
        "header_skip={} bank_base=0x{:X} bank_data=0x{:X} data_data=0x{:X} \
         bank_sounds=0x{:X} bank_gains=0x{:X} bank_gain_data=0x{:X} \
         num_bank_sounds={} num_sounds={} kind=0:{} other:{} sample={:?}\n\
         [scream]   bank_data+0..64: {}\n\
         [scream]   sounds+0..48: {}\n\
         [scream]   bank_gains+0..32: {}",
        header_skip, bank_base, bank_data_pos, data_data_pos,
        bank_sounds_pos, bank_gains_pos, bank_gain_data_pos,
        num_bank_sounds, num_sounds, kind_zero_count, kind_nonzero_count, kind_sample,
        bank_dump, sounds_dump, bank_gains_dump,
    ));

    let mut out = Vec::new();
    for (i, (kind, idx)) in entries.iter().enumerate() {
        if *idx < 0 || *kind != 0 {
            continue;
        }
        let bank_idx = *idx as usize;
        if bank_idx >= num_bank_sounds {
            continue;
        }


        let sound_off = bank_sounds_pos + (bank_idx as u64) * 12;
        ig.stream.seek_to(sound_off + 0x04)?;
        let num_gains = ig.stream.read_u8()? as usize;
        let _ = ig.stream.read_u8()?;
        let _flags = ig.stream.read_u16()?;
        let gains_ptr_rel = u64::from(ig.stream.read_u32()?);

        let sound_gains_pos = match gain_base {
            GainBase::BankData => bank_data_pos + gains_ptr_rel,
            GainBase::BankGainsArray => bank_gains_pos + gains_ptr_rel,
        };


        ig.stream.seek_to(u64::from(names_section.offset) + (i as u64) * 64)?;
        let name_buf = ig.stream.read_bytes(64)?;
        let nul = name_buf.iter().position(|b| *b == 0).unwrap_or(name_buf.len());
        let name = String::from_utf8_lossy(&name_buf[..nul]).into_owned();

        scream_entry_diag(format_args!(
            "first valid entry: i={} bank_idx={} name='{}' sound_off=0x{:X} num_gains={} gains_ptr_rel=0x{:X} sound_gains_pos=0x{:X}",
            i, bank_idx, name, sound_off, num_gains, gains_ptr_rel, sound_gains_pos
        ));

        for g in 0..num_gains {

            ig.stream.seek_to(sound_gains_pos + (g as u64) * 8)?;
            let packed = ig.stream.read_u32()?;
            let stream_offset = packed & 0x00FFFFFF;
            let gain_type = (packed >> 24) & 0xFF;
            let _ = ig.stream.read_u32()?;
            if g == 0 {
                SCREAM_GAIN_DIAG_FIRED.with(|c| {
                    if !c.replace(true) {
                        eprintln!(
                            "[scream-gain] first gain: packed=0x{:08X} stream_offset=0x{:X} gain_type={} (skip if !=1)",
                            packed, stream_offset, gain_type
                        );
                    }
                });
            }
            if gain_type != 1 {
                continue;
            }


            let wform_off = bank_gain_data_pos + u64::from(stream_offset);
            ig.stream.seek_to(wform_off + 0x02)?;
            let center_note = ig.stream.read_u8()? as i8;
            let center_fine = ig.stream.read_u8()? as i8;
            ig.stream.seek_to(wform_off + 0x0E)?;
            let wf_flags = ig.stream.read_u16()?;
            let wf_stream_offset = u64::from(ig.stream.read_u32()?);
            let wf_stream_size = ig.stream.read_u32()? as usize;
            SCREAM_WAVE_DIAG_FIRED.with(|c| {
                if !c.replace(true) {
                    eprintln!(
                        "[scream-wave] first waveform: wform_off=0x{:X} center_note={} center_fine={} wf_flags=0x{:04X} wf_stream_offset=0x{:X} wf_stream_size={}",
                        wform_off, center_note, center_fine, wf_flags, wf_stream_offset, wf_stream_size
                    );
                }
            });
            if wf_stream_size == 0 {
                continue;
            }


            ig.stream.seek_to(data_data_pos + wf_stream_offset)?;
            let raw = ig.stream.read_bytes(wf_stream_size)?;

            let sample_rate = sample_rate_for(center_note, center_fine);
            let use_pcm = (wf_flags & 0x80) != 0;

            let samples: Vec<i16> = if use_pcm {

                let mut s = Vec::with_capacity(raw.len() / 2);
                let mut k = 0;
                while k + 1 < raw.len() {
                    let v = i16::from_be_bytes([raw[k], raw[k + 1]]);
                    s.push(v);
                    k += 2;
                }
                s
            } else {
                decode_adpcm_stream(&raw)
            };
            let sample_count = samples.len() as u32;
            let wav = write_wav_pcm16_mono(&samples, sample_rate);

            out.push(ExtractedSound {
                name: if num_gains > 1 {
                    format!("{}_{}", name, g)
                } else {
                    name.clone()
                },
                index: i,
                gain_index: g as u8,
                sample_rate,
                channels: 1,
                sample_count,
                wav,
            });
        }
    }
    SCREAM_RESULT_DIAG_FIRED.with(|c| {
        if !c.replace(true) {
            let first_3_names: Vec<String> = out.iter().take(3).map(|s| s.name.clone()).collect();
            eprintln!(
                "[scream-result] header_skip={} returned {} ExtractedSound items, first 3 names={:?}",
                header_skip,
                out.len(),
                first_3_names
            );
        }
    });
    Ok(out)
}




pub fn read_stream_offsets<R: Read + Seek>(ig: &mut IgFile<R>) -> Result<Vec<u32>> {
    let ids = detect_sound_ids(ig);
    let section = match ig.section(ids.streams) {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };

    let total_len = section.length as u64;
    if total_len < 136 {
        return Ok(Vec::new());
    }
    let section_base = u64::from(section.offset);
    ig.stream.seek_to(section_base + 128)?;
    let stream_offsets_ptr = ig.stream.read_u32()? as u64;
    let unk1_ptr = ig.stream.read_u32()? as u64;
    if stream_offsets_ptr == 0 || unk1_ptr <= stream_offsets_ptr {
        return Ok(Vec::new());
    }
    let count = ((unk1_ptr - stream_offsets_ptr) / 4) as usize;
    if count == 0 {
        return Ok(Vec::new());
    }
    ig.stream.seek_to(stream_offsets_ptr)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(ig.stream.read_u32()?);
    }
    Ok(out)
}


#[derive(Default, Clone, Copy)]
struct AdpcmCh {
    prev: i32,
    pp: i32,
}


fn decode_adpcm_run(blocks: &[u8], state: &mut AdpcmCh) -> Vec<i16> {
    let n = blocks.len() / 16;
    let mut out = Vec::with_capacity(n * 28);
    let mut buf = [0i16; 28];
    let mut block = [0u8; 16];
    for b in 0..n {
        block.copy_from_slice(&blocks[b * 16..b * 16 + 16]);
        decode_adpcm_block(&block, &mut buf, &mut state.prev, &mut state.pp);
        out.extend_from_slice(&buf);
    }
    out
}


#[derive(Debug)]
struct StreamPcm {
    sample_rate: u32,
    channels: u16,

    samples: Vec<i16>,
}


fn decode_one_stream<R: Read + Seek>(
    rd: &mut R,
    stream_offset: u64,
) -> std::io::Result<StreamPcm> {
    rd.seek(SeekFrom::Start(stream_offset))?;
    let mut magic = [0u8; 4];
    rd.read_exact(&mut magic)?;


    match &magic {
        b"VAGp" => decode_vagp(rd, stream_offset,  true),
        b"pGAV" => decode_vagp(rd, stream_offset,  false),
        b"VPK " => decode_vpk(rd, stream_offset),
        b"XVAG" => decode_xvag(rd, stream_offset),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "unknown streaming sound magic {:02X?} at offset {}",
                magic, stream_offset
            ),
        )),
    }
}


fn decode_vagp<R: Read + Seek>(
    rd: &mut R,
    stream_offset: u64,
    big_endian: bool,
) -> std::io::Result<StreamPcm> {
    let mut header_rest = [0u8; 44];
    rd.read_exact(&mut header_rest)?;
    let read_u32 = |bytes: [u8; 4]| {
        if big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    };
    let data_size = read_u32([
        header_rest[8],
        header_rest[9],
        header_rest[10],
        header_rest[11],
    ]);
    let sample_rate = read_u32([
        header_rest[12],
        header_rest[13],
        header_rest[14],
        header_rest[15],
    ]);
    if data_size == 0 || sample_rate == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "VAGp at offset {stream_offset} has zero data_size or sample_rate"
            ),
        ));
    }


    let mut data = vec![0u8; data_size as usize];
    rd.read_exact(&mut data)?;
    let mut state = AdpcmCh::default();
    let samples = decode_adpcm_run(&data, &mut state);
    Ok(StreamPcm {
        sample_rate,
        channels: 1,
        samples,
    })
}


fn decode_vpk<R: Read + Seek>(
    rd: &mut R,
    stream_offset: u64,
) -> std::io::Result<StreamPcm> {
    let mut buf = [0u8; 32];
    rd.read_exact(&mut buf)?;

    let channel_size = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64;
    let data_offset = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as u64;
    let channel_block_size_raw = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as u64;
    let sample_rate = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
    let num_channels = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]) as u16;
    if num_channels == 0 || sample_rate == 0 || channel_block_size_raw < 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("VPK at {stream_offset} has invalid header"),
        ));
    }

    let channel_block_size = channel_block_size_raw / 2;


    rd.seek(SeekFrom::Start(stream_offset + data_offset))?;


    let mut ch_states: Vec<AdpcmCh> = vec![AdpcmCh::default(); num_channels as usize];
    let mut ch_pcm: Vec<Vec<i16>> = (0..num_channels).map(|_| Vec::new()).collect();

    let num_groups = channel_size / channel_block_size;
    let rem = channel_size % channel_block_size;

    let decode_group = |group_size: u64,
                        rd: &mut R,
                        ch_states: &mut [AdpcmCh],
                        ch_pcm: &mut [Vec<i16>]|
     -> std::io::Result<()> {
        for c in 0..num_channels as usize {
            let mut block = vec![0u8; channel_block_size as usize];

            rd.read_exact(&mut block)?;
            let usable = group_size.min(channel_block_size) as usize;
            let pcm = decode_adpcm_run(&block[..usable], &mut ch_states[c]);
            ch_pcm[c].extend_from_slice(&pcm);
        }
        Ok(())
    };

    for _ in 0..num_groups {
        decode_group(channel_block_size, rd, &mut ch_states, &mut ch_pcm)?;
    }
    if rem > 0 {
        decode_group(rem, rd, &mut ch_states, &mut ch_pcm)?;
    }


    let per_channel = ch_pcm.iter().map(|v| v.len()).min().unwrap_or(0);
    let mut interleaved = Vec::with_capacity(per_channel * num_channels as usize);
    for s in 0..per_channel {
        for c in 0..num_channels as usize {
            interleaved.push(ch_pcm[c][s]);
        }
    }
    Ok(StreamPcm {
        sample_rate,
        channels: num_channels,
        samples: interleaved,
    })
}


fn decode_xvag<R: Read + Seek>(
    rd: &mut R,
    stream_offset: u64,
) -> std::io::Result<StreamPcm> {

    let mut hdr = [0u8; 28];
    rd.read_exact(&mut hdr)?;
    let size_to_body = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
    if size_to_body < 32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "XVAG at {stream_offset} size_to_body={size_to_body} too small"
            ),
        ));
    }
    let chunks_end = stream_offset + size_to_body;

    let mut format = 0u32;
    let mut sample_rate = 0u32;
    let mut num_channels = 0u32;
    let mut interleave = 0u32;
    let mut buffer_size = 0u32;


    loop {
        let chunk_start = rd.stream_position()?;
        if chunk_start + 8 > chunks_end {
            break;
        }
        let mut chunk = [0u8; 8];
        rd.read_exact(&mut chunk)?;
        let chunk_id = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let chunk_size = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        if &chunk_id == b"fmat" {
            let mut fmat = [0u8; 28];
            rd.read_exact(&mut fmat)?;
            num_channels = u32::from_be_bytes([fmat[0], fmat[1], fmat[2], fmat[3]]);
            format = u32::from_be_bytes([fmat[4], fmat[5], fmat[6], fmat[7]]);
            let _num_samples =
                u32::from_be_bytes([fmat[8], fmat[9], fmat[10], fmat[11]]);
            let _unk1 =
                u32::from_be_bytes([fmat[12], fmat[13], fmat[14], fmat[15]]);
            interleave =
                u32::from_be_bytes([fmat[16], fmat[17], fmat[18], fmat[19]]);
            sample_rate =
                u32::from_be_bytes([fmat[20], fmat[21], fmat[22], fmat[23]]);
            buffer_size =
                u32::from_be_bytes([fmat[24], fmat[25], fmat[26], fmat[27]]);
            break;
        } else {
            rd.seek(SeekFrom::Start(chunk_start + 8 + u64::from(chunk_size)))?;
        }
    }

    if sample_rate == 0 || num_channels == 0 || buffer_size == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("XVAG at {stream_offset} missing fmat chunk"),
        ));
    }
    if format != 0x06 {

        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!(
                "XVAG at {stream_offset} uses format {format:#x} (only PS_ADPCM=0x06 supported)"
            ),
        ));
    }
    if interleave != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("XVAG at {stream_offset} interleave={interleave} unsupported"),
        ));
    }


    rd.seek(SeekFrom::Start(stream_offset + size_to_body))?;

    let num_blocks_audio =
        (buffer_size as u64 / 16) / num_channels as u64;
    let mut ch_states: Vec<AdpcmCh> = vec![AdpcmCh::default(); num_channels as usize];
    let mut interleaved =
        Vec::with_capacity((num_blocks_audio * 28) as usize * num_channels as usize);

    let mut lane = [0u8; 16];
    let mut samples = [0i16; 28];
    for _ in 0..num_blocks_audio {

        let mut per_ch_samples: Vec<[i16; 28]> = Vec::with_capacity(num_channels as usize);
        for c in 0..num_channels as usize {
            rd.read_exact(&mut lane)?;
            let st = &mut ch_states[c];
            decode_adpcm_block(&lane, &mut samples, &mut st.prev, &mut st.pp);
            per_ch_samples.push(samples);
        }
        for s in 0..28 {
            for c in 0..num_channels as usize {
                interleaved.push(per_ch_samples[c][s]);
            }
        }
    }
    Ok(StreamPcm {
        sample_rate,
        channels: num_channels as u16,
        samples: interleaved,
    })
}


pub fn extract_stream_sounds<R: Read + Seek>(
    ig: &mut IgFile<R>,
    stream_path: &Path,
    errors_out: &mut Vec<String>,
) -> Result<Vec<ExtractedSound>> {
    let ids = detect_sound_ids(ig);
    let names_section = match ig.section(ids.names) {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };
    let sounds_section = match ig.section(ids.sounds) {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };
    let stream_offsets = read_stream_offsets(ig)?;
    if stream_offsets.is_empty() {
        return Ok(Vec::new());
    }


    ig.stream.seek_to(u64::from(sounds_section.offset))?;
    let num_sounds = ig.stream.read_u32()? as usize;
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let _ = ig.stream.read_u32()?;
    let mut entries: Vec<(u16, i16)> = Vec::with_capacity(num_sounds);
    for _ in 0..num_sounds {
        let kind = ig.stream.read_u16()?;
        let idx = ig.stream.read_i16()?;
        entries.push((kind, idx));
    }


    let mut work: Vec<(usize, String, u64)> = Vec::new();
    for (i, (kind, idx)) in entries.iter().enumerate() {
        if *idx < 0 || *kind == 0 {
            continue;
        }
        let stream_idx = *idx as usize;
        if stream_idx >= stream_offsets.len() {
            continue;
        }
        let offset = stream_offsets[stream_idx] as u64;

        ig.stream.seek_to(u64::from(names_section.offset) + (i as u64) * 64)?;
        let name_buf = ig.stream.read_bytes(64)?;
        let nul = name_buf
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(name_buf.len());
        let name = String::from_utf8_lossy(&name_buf[..nul]).into_owned();
        work.push((i, name, offset));
    }


    let stream_path_buf = stream_path.to_path_buf();
    let results: Vec<std::result::Result<ExtractedSound, String>> = work
        .par_iter()
        .map(|(i, name, offset)| {
            let file = File::open(&stream_path_buf).map_err(|e| {
                format!("{name}: open stream file: {e}")
            })?;
            let mut rd = BufReader::new(file);
            match decode_one_stream(&mut rd, *offset) {
                Ok(pcm) => {
                    let sample_count = if pcm.channels > 0 {
                        (pcm.samples.len() / pcm.channels as usize) as u32
                    } else {
                        0
                    };
                    let wav =
                        write_wav_pcm16(&pcm.samples, pcm.sample_rate, pcm.channels);
                    Ok(ExtractedSound {
                        name: name.clone(),
                        index: *i,
                        gain_index: 0,
                        sample_rate: pcm.sample_rate,
                        channels: pcm.channels,
                        sample_count,
                        wav,
                    })
                }
                Err(e) => Err(format!("{name}: {e}")),
            }
        })
        .collect();

    let mut out = Vec::with_capacity(results.len());
    for r in results {
        match r {
            Ok(s) => out.push(s),
            Err(msg) => errors_out.push(msg),
        }
    }

    Ok(out)
}


pub fn scan_raw_audio_offsets(stream_path: &Path) -> Result<Vec<u64>> {
    let mut file = File::open(stream_path).map_err(crate::error::Error::Io)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(crate::error::Error::Io)?;
    let mut offsets = Vec::new();
    if data.len() < 4 {
        return Ok(offsets);
    }
    let scan_end = data.len() - 3;
    let mut i = 0;
    while i < scan_end {
        let m = &data[i..i + 4];
        if m == b"VAGp" || m == b"pGAV" || m == b"XVAG" || m == b"VPK " {
            offsets.push(i as u64);

            i += 32;
        } else {
            i += 1;
        }
    }
    Ok(offsets)
}


pub fn list_raw_streaming(stream_path: &Path) -> Result<Vec<SoundSummary>> {
    let offsets = scan_raw_audio_offsets(stream_path)?;
    Ok(offsets
        .iter()
        .enumerate()
        .map(|(i, off)| SoundSummary {
            name: format!("stream_{:05}_0x{:08X}", i, off),
            index: i,
            kind: SoundKind::Stream,
        })
        .collect())
}


pub fn extract_raw_streaming(
    stream_path: &Path,
    errors_out: &mut Vec<String>,
) -> Result<Vec<ExtractedSound>> {
    let offsets = scan_raw_audio_offsets(stream_path)?;
    if offsets.is_empty() {
        return Ok(Vec::new());
    }
    let stream_path_buf = stream_path.to_path_buf();
    let results: Vec<std::result::Result<ExtractedSound, String>> = offsets
        .par_iter()
        .enumerate()
        .map(|(i, offset)| {
            let label = format!("stream_{:05}_0x{:08X}", i, offset);
            let file = File::open(&stream_path_buf)
                .map_err(|e| format!("{label}: open: {e}"))?;
            let mut rd = BufReader::new(file);
            match decode_one_stream(&mut rd, *offset) {
                Ok(pcm) => {
                    let sample_count = if pcm.channels > 0 {
                        (pcm.samples.len() / pcm.channels as usize) as u32
                    } else {
                        0
                    };
                    let wav =
                        write_wav_pcm16(&pcm.samples, pcm.sample_rate, pcm.channels);
                    Ok(ExtractedSound {
                        name: label,
                        index: i,
                        gain_index: 0,
                        sample_rate: pcm.sample_rate,
                        channels: pcm.channels,
                        sample_count,
                        wav,
                    })
                }
                Err(e) => Err(format!("{label}: {e}")),
            }
        })
        .collect();

    let mut out = Vec::with_capacity(results.len());
    for r in results {
        match r {
            Ok(s) => out.push(s),
            Err(msg) => errors_out.push(msg),
        }
    }
    Ok(out)
}


// Both mappers accept either a bare filename or a path (the final
// component is mapped, the directory part is preserved) and both accept
// R3's language-infixed names: `resident_sound.us.dat` pairs with
// `streaming_sound.us.dat` inside `packed/game/global_sound_<lang>/
// built/sound/bank/<hash>/` — R3 ships no per-level banks at all.
fn split_dir_and_name(path: &str) -> (&str, &str) {
    match path.rfind(['/', '\\']) {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    }
}

pub fn bank_pair_for(stream_filename: &str) -> Option<String> {
    let (dir, name) = split_dir_and_name(stream_filename);
    let lower = name.to_lowercase();
    let mapped = if let Some(rest) = lower.strip_prefix("streaming_sound") {
        format!("resident_sound{rest}")
    } else if let Some(rest) = lower.strip_prefix("streaming_dialogue") {
        format!("resident_dialogue{rest}")
    } else if lower == "ps3soundstream.dat" {
        "ps3sound.dat".to_string()
    } else if let Some(rest) = lower.strip_prefix("ps3dialoguestream") {
        format!("ps3dialogue{rest}")
    } else {
        return None;
    };
    Some(format!("{dir}{mapped}"))
}


pub fn streaming_sibling_for(filename: &str) -> Option<String> {
    let (dir, name) = split_dir_and_name(filename);
    let mapped = if let Some(rest) = name.strip_prefix("resident_sound") {
        format!("streaming_sound{rest}")
    } else if let Some(rest) = name.strip_prefix("resident_dialogue") {
        format!("streaming_dialogue{rest}")
    } else if name == "ps3sound.dat" {
        "ps3soundstream.dat".to_string()
    } else if let Some(rest) = name.strip_prefix("ps3dialogue") {
        format!("ps3dialoguestream{rest}")
    } else {
        return None;
    };
    Some(format!("{dir}{mapped}"))
}

/// Walk up from a level folder to the `packed/` ancestor and collect every
/// extracted `game/*/built/sound/bank/<hash>/` directory. R3 keeps ALL its
/// sound banks at game scope (the per-level `resident_sound.dat`
/// convention is R2/RFOM-era), split across TWO archive families:
/// `global_sound_<lang>.psarc` → language-infixed dialogue/VO bank+stream
/// pairs, and `global_uncached.psarc` → language-free `resident_sound.dat`
/// SFX/music/weapon banks (2,754 in R3 US, self-contained, no streaming
/// sibling). Probe every extracted variant dir rather than a name prefix
/// so both families — and any future ones — are found. Empty vec when
/// nothing is extracted.
pub fn find_global_sound_bank_dirs(level_folder: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut cur = Some(level_folder);
    while let Some(p) = cur {
        if p.file_name().map(|n| n == "packed").unwrap_or(false) {
            let game = p.join("game");
            if let Ok(rd) = std::fs::read_dir(&game) {
                let mut variants: Vec<std::path::PathBuf> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|v| v.is_dir())
                    .collect();
                variants.sort();
                for v in variants {
                    let bank_root = v.join("built").join("sound").join("bank");
                    if let Ok(banks) = std::fs::read_dir(&bank_root) {
                        let mut dirs: Vec<std::path::PathBuf> = banks
                            .flatten()
                            .map(|e| e.path())
                            .filter(|d| d.is_dir())
                            .collect();
                        dirs.sort();
                        out.extend(dirs);
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        cur = p.parent();
    }
    out
}




fn fourcc_be(id: u32) -> String {
    let bytes = id.to_be_bytes();
    bytes
        .iter()
        .map(|b| {
            if (0x20..=0x7e).contains(b) {
                *b as char
            } else {
                '.'
            }
        })
        .collect()
}


pub fn dump_sound_bank_info<R: Read + Seek>(ig: &mut IgFile<R>) -> Result<String> {
    use std::fmt::Write as _;
    let mut out = String::new();
    let ids = detect_sound_ids(ig);
    let version_tag = if ids.sounds == SECT_SOUNDS_V2 {
        "V2"
    } else {
        "V1"
    };
    let _ = writeln!(out, "SCREAM bank diagnostic — {} layout", version_tag);
    let _ = writeln!(
        out,
        "  IDs: bank={:#x}, sounds={:#x}, names={:#x}, streams={:#x}",
        ids.bank, ids.sounds, ids.names, ids.streams
    );


    for (label, id) in [
        ("SoundBank", ids.bank),
        ("Sounds", ids.sounds),
        ("SoundNames", ids.names),
        ("SoundStreams", ids.streams),
    ] {
        match ig.section(id) {
            Some(s) => {
                let _ = writeln!(
                    out,
                    "  Section {:13} ({:#06x}): offset={:#010x} length={:#010x}",
                    label, id, s.offset, s.length
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "  Section {:13} ({:#06x}): MISSING",
                    label, id
                );
            }
        }
    }


    if let Some(bs) = ig.section(ids.bank) {
        let header_base = u64::from(bs.offset) + 144;
        ig.stream.seek_to(header_base)?;
        let bank_header_version = ig.stream.read_u32()?;
        let num_sections = ig.stream.read_u32()?;
        let _ = writeln!(
            out,
            "  SCREAMBankHeader @ {:#x}: version={} numSections={}",
            header_base, bank_header_version, num_sections
        );
        let cap_sections = num_sections.min(8) as usize;
        let mut section_data_pos: [u64; 2] = [0; 2];
        for i in 0..cap_sections {
            let ptr = ig.stream.read_u32()?;
            let size = ig.stream.read_u32()?;
            let resolved = header_base + u64::from(ptr);
            if i < section_data_pos.len() {
                section_data_pos[i] = resolved;
            }
            let _ = writeln!(
                out,
                "    sections[{}].data: ptr={:#010x} (→ {:#010x}), size={:#010x}",
                i, ptr, resolved, size
            );
        }


        if cap_sections >= 1 && section_data_pos[0] != 0 {
            let bank_pos = section_data_pos[0];
            ig.stream.seek_to(bank_pos)?;
            let id = ig.stream.read_u32()?;
            let bank_version = ig.stream.read_u32()?;
            let flags = ig.stream.read_u32()?;
            ig.stream.seek_to(bank_pos + 0x16)?;
            let num_sounds = ig.stream.read_u16()?;
            let num_gains = ig.stream.read_u16()?;
            let unk0 = ig.stream.read_u16()?;
            let sounds_ptr = ig.stream.read_u32()?;
            let gains_ptr = ig.stream.read_u32()?;
            let _unk2 = ig.stream.read_u32()?;
            let _ds0 = ig.stream.read_u32()?;
            let _ds1 = ig.stream.read_u32()?;
            let _null2 = ig.stream.read_u32()?;
            let gain_data_ptr = ig.stream.read_u32()?;
            let _ = writeln!(
                out,
                "  SCREAMBank @ {:#x}: id={:#010x} (\"{}\") version={} flags={:#x}",
                bank_pos,
                id,
                fourcc_be(id),
                bank_version,
                flags
            );
            let _ = writeln!(
                out,
                "    numSounds={} numGains={} unk0={:#x}",
                num_sounds, num_gains, unk0
            );
            let _ = writeln!(
                out,
                "    sounds_ptr   = {:#010x} (→ {:#010x})",
                sounds_ptr,
                bank_pos + u64::from(sounds_ptr)
            );
            let _ = writeln!(
                out,
                "    gains_ptr    = {:#010x} (→ {:#010x})",
                gains_ptr,
                bank_pos + u64::from(gains_ptr)
            );
            let _ = writeln!(
                out,
                "    gainData_ptr = {:#010x} (→ {:#010x})",
                gain_data_ptr,
                bank_pos + u64::from(gain_data_ptr)
            );
        }
    }


    if let Some(s) = ig.section(ids.sounds) {
        ig.stream.seek_to(u64::from(s.offset))?;
        let num = ig.stream.read_u32()?;
        let _ = ig.stream.read_u32()?;
        let _ = ig.stream.read_u32()?;
        let _ = ig.stream.read_u32()?;
        let _ = writeln!(out, "  Sounds: numSounds={}", num);
        let preview = num.min(5);
        for i in 0..preview {
            let kind = ig.stream.read_u16()?;
            let idx = ig.stream.read_i16()?;
            let kind_label = if kind == 0 { "bank" } else { "stream" };
            let _ = writeln!(
                out,
                "    [{}] kind={} ({}) index={}",
                i, kind, kind_label, idx
            );
        }
        if num > preview {
            let _ = writeln!(out, "    … ({} more)", num - preview);
        }
    }


    if let Some(s) = ig.section(ids.names) {
        let _ = writeln!(
            out,
            "  SoundNames: capacity={} (slot=64 bytes)",
            s.length / 64
        );
        for i in 0..3.min(s.length / 64) {
            ig.stream.seek_to(u64::from(s.offset) + u64::from(i) * 64)?;
            let buf = ig.stream.read_bytes(64)?;
            let nul = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            let name = String::from_utf8_lossy(&buf[..nul]);
            let _ = writeln!(out, "    [{}] \"{}\"", i, name);
        }
    }


    let stream_offsets = read_stream_offsets(ig).unwrap_or_default();
    let _ = writeln!(
        out,
        "  SoundStreams: {} offsets (resolved via 128-byte header + pointer table)",
        stream_offsets.len()
    );
    for (i, off) in stream_offsets.iter().take(5).enumerate() {
        let _ = writeln!(out, "    [{}] file_offset={:#010x}", i, off);
    }
    if stream_offsets.len() > 5 {
        let _ = writeln!(out, "    … ({} more)", stream_offsets.len() - 5);
    }

    Ok(out)
}
