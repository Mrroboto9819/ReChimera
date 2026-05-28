use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

#[derive(Debug, Clone, Default)]
pub struct OutfitterNames {
    pub by_tuid: HashMap<u64, String>,
}

impl OutfitterNames {
    pub fn is_empty(&self) -> bool {
        self.by_tuid.is_empty()
    }

    pub fn lookup(&self, tuid: u64) -> Option<&str> {
        self.by_tuid.get(&tuid).map(String::as_str)
    }

    pub fn merge(&mut self, other: OutfitterNames) {
        for (tuid, name) in other.by_tuid {
            self.by_tuid.entry(tuid).or_insert(name);
        }
    }
}

pub fn find_configs_dir(assetlookup_path: &Path) -> Option<PathBuf> {
    let mut cursor = assetlookup_path.parent()?.to_path_buf();
    for _ in 0..10 {
        let candidate = cursor
            .join("packed")
            .join("game")
            .join("global_cached")
            .join("data")
            .join("configs");
        if candidate.is_dir() {
            return Some(candidate);
        }
        let alt = cursor
            .join("global_cached")
            .join("data")
            .join("configs");
        if alt.is_dir() {
            return Some(alt);
        }
        if !cursor.pop() {
            break;
        }
    }
    None
}

pub fn load_from_assetlookup(assetlookup_path: &Path) -> Result<OutfitterNames> {
    match find_configs_dir(assetlookup_path) {
        Some(dir) => load_from_configs_dir(&dir),
        None => Ok(OutfitterNames::default()),
    }
}

pub fn load_from_configs_dir(configs_dir: &Path) -> Result<OutfitterNames> {
    let mut names = OutfitterNames::default();
    for csv in ["comp_outfitter.csv", "coop_outfitter.csv"] {
        let path = configs_dir.join(csv);
        if path.is_file() {
            let chunk = parse_outfitter_csv(&path)?;
            names.merge(chunk);
        }
    }
    Ok(names)
}

fn parse_outfitter_csv(path: &Path) -> Result<OutfitterNames> {
    let body = fs::read_to_string(path)?;
    let mut out = OutfitterNames::default();
    for raw_line in body.lines() {
        let line = raw_line.trim_end_matches('\r');
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 5 {
            continue;
        }
        let category = cols[0].trim();
        if !matches!(category, "Skin" | "Heads" | "Body") {
            continue;
        }
        let raw_id = cols[2].trim();
        let Some(tuid) = parse_hex_tuid(raw_id) else {
            continue;
        };
        let loc_tag = cols[3].trim();
        let unlock_id = cols.get(4).map(|s| s.trim()).unwrap_or("");
        let name = friendly_name(unlock_id, loc_tag, category, tuid);
        out.by_tuid.entry(tuid).or_insert(name);
    }
    Ok(out)
}

fn parse_hex_tuid(raw: &str) -> Option<u64> {
    let stripped = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))?;
    if stripped.len() != 16 {
        return None;
    }
    u64::from_str_radix(stripped, 16).ok()
}

fn friendly_name(unlock_id: &str, loc_tag: &str, category: &str, tuid: u64) -> String {
    if let Some(rest) = unlock_id.strip_prefix("BI_COMP_HMN_") {
        return sanitize(rest);
    }
    if let Some(rest) = unlock_id.strip_prefix("BI_COMP_CHIM_") {
        return format!("CHIM_{}", sanitize(rest));
    }
    if let Some(rest) = unlock_id.strip_prefix("BI_") {
        return sanitize(rest);
    }
    if let Some(rest) = loc_tag.strip_prefix("LOBBY_NAME_") {
        return sanitize(rest);
    }
    if loc_tag == "LOBBY_LABEL_HEAD" {
        return format!("COOP_HEAD_{:016X}", tuid);
    }
    if !unlock_id.is_empty() {
        return sanitize(unlock_id);
    }
    if !loc_tag.is_empty() {
        return sanitize(loc_tag);
    }
    format!("{}_{:016X}", category.to_uppercase(), tuid)
}

fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_unlock_id() {
        assert_eq!(
            friendly_name(
                "BI_COMP_HMN_HEAD_HALE",
                "LOBBY_NAME_HEADHALE",
                "Heads",
                0xD6A1F47F6A837C1F,
            ),
            "HEAD_HALE"
        );
    }

    #[test]
    fn falls_back_to_lobby_name() {
        assert_eq!(
            friendly_name("", "LOBBY_NAME_BODYRANGER", "Skin", 0x1234),
            "BODYRANGER"
        );
    }

    #[test]
    fn chim_gets_prefix() {
        assert_eq!(
            friendly_name(
                "BI_COMP_CHIM_HEAD_HYBRID",
                "",
                "Heads",
                0x1234
            ),
            "CHIM_HEAD_HYBRID"
        );
    }

    #[test]
    fn generic_coop_head_uses_tuid() {
        let name = friendly_name("", "LOBBY_LABEL_HEAD", "Heads", 0x8E0220726AB758E6);
        assert_eq!(name, "COOP_HEAD_8E0220726AB758E6");
    }

    #[test]
    fn parse_hex_requires_16_chars() {
        assert_eq!(parse_hex_tuid("0xD6A1F47F6A837C1F"), Some(0xD6A1F47F6A837C1F));
        assert_eq!(parse_hex_tuid("0x1234"), None);
        assert_eq!(parse_hex_tuid("D6A1F47F6A837C1F"), None);
    }
}
