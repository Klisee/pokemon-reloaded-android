//! Host compatibility adapter for the clean-room `.TEAM` codec.
//!
//! The adapter owns the host <-> Profile R conversion. The codec itself lives in
//! `team_codec_clean.rs` and is never modified here.

use std::fmt;

use super::team_codec_clean::{
    self, DecodeOptions, EncodeOptions, InfoCell, PartySlot, Team, Trainer,
};
use crate::compat::f3r;

pub(crate) const CURRENT_VERSION: i32 = f3r::team::CURRENT_VERSION;

const PARTY_SIZE: usize = f3r::format::PARTY_SIZE;
const INFO_LINES: usize = f3r::format::TEAM_INFO_LINES;

const INFO_KIND_NUMERIC: u8 = f3r::team::INFO_KIND_NUMERIC;
const INFO_KIND_TEXT: u8 = f3r::team::INFO_KIND_TEXT;
const INFO_KIND_ENCODED: u8 = f3r::team::INFO_KIND_ENCODED;
const INFO_KINDS: [u8; INFO_LINES] = f3r::team::INFO_KINDS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TeamError(pub String);

impl TeamError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn from_clean(error: team_codec_clean::TeamError) -> Self {
        let path = error.path.unwrap_or("");
        let line = error
            .line
            .map(|line| format!(" line {line}"))
            .unwrap_or_default();
        if path.is_empty() {
            Self(format!("{:?}{line}", error.category))
        } else {
            Self(format!("{:?} at {path}{line}", error.category))
        }
    }

    fn field(field: &'static str) -> Self {
        Self(format!("{field}: value is not representable"))
    }
}

impl fmt::Display for TeamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TeamError {}

#[derive(Clone, Debug)]
pub(crate) struct TeamFile {
    pub name: Vec<u8>,
    pub trainer_id: i32,
    pub trainer_index: i32,
    pub outfit: f64,
    pub phrase: Vec<u8>,
    pub mons: [TeamMon; PARTY_SIZE],
    pub version: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct TeamMon {
    pub species: i32,
    pub level: i32,
    pub sex: i32,
    pub exp: f64,
    pub iv: i32,
    pub item: i32,
    pub selue: f64,
    pub moves: [f64; 4],
    pub info: [Vec<u8>; INFO_LINES],
}

impl TeamMon {
    pub(crate) fn empty() -> Self {
        Self {
            species: 0,
            level: 0,
            sex: 0,
            exp: 0.0,
            iv: 0,
            item: 0,
            selue: 0.0,
            moves: [0.0; 4],
            info: std::array::from_fn(|_| Vec::new()),
        }
    }
}

impl TeamFile {
    pub(crate) fn decode(data: &[u8]) -> Result<Self, TeamError> {
        // `battle_team_import` already applies the pre-20241217 outfit rule.
        // Disable it in the codec so it is applied exactly once.
        let options = DecodeOptions {
            strict_canonical: false,
            allow_legacy_version_adjustment: false,
        };
        let decoded = team_codec_clean::decode(data, &options).map_err(TeamError::from_clean)?;
        Self::from_clean(decoded, data)
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, TeamError> {
        let team = self.to_clean()?;
        let version = u32::try_from(self.version).map_err(|_| TeamError::field("version"))?;
        team_codec_clean::encode(&team, &EncodeOptions { version: Some(version) })
            .map(|result| result.bytes)
            .map_err(TeamError::from_clean)
    }

    pub(crate) fn validate(&self) -> Result<(), TeamError> {
        let team = self.to_clean()?;
        let version = u32::try_from(self.version).map_err(|_| TeamError::field("version"))?;
        team_codec_clean::encode(&team, &EncodeOptions { version: Some(version) })
            .map(|_| ())
            .map_err(TeamError::from_clean)
    }

    fn from_clean(
        decoded: team_codec_clean::DecodeResult,
        data: &[u8],
    ) -> Result<Self, TeamError> {
        let team = decoded.team;
        if team.slots.len() != PARTY_SIZE {
            return Err(TeamError::new("team slot count"));
        }

        let mut mons = std::array::from_fn(|_| TeamMon::empty());
        for (slot_index, slot) in team.slots.iter().enumerate() {
            if slot.info.len() != INFO_LINES {
                return Err(TeamError::new("team info count"));
            }
            mons[slot_index] = TeamMon {
                species: slot.species as i32,
                level: slot.level as i32,
                sex: slot.sex as i32,
                exp: f64::from(slot.experience),
                iv: slot.iv as i32,
                item: slot.held_item as i32,
                selue: slot.selue as f64,
                moves: [
                    f64::from(slot.moves[0]),
                    f64::from(slot.moves[1]),
                    f64::from(slot.moves[2]),
                    f64::from(slot.moves[3]),
                ],
                info: std::array::from_fn(|field| {
                    // Profile R text cells are byte-preserving. The codec may
                    // classify an all-digit text cell as `Numeric`; recover the
                    // original line for text-kind fields so the host still gets
                    // the exact bytes it would have received before.
                    if INFO_KINDS[field] == INFO_KIND_TEXT {
                        raw_info_line(data, slot_index, field)
                            .unwrap_or_else(|| info_to_host(&slot.info[field]))
                    } else {
                        info_to_host(&slot.info[field])
                    }
                }),
            };
        }

        Ok(Self {
            name: team.trainer.name,
            trainer_id: team.trainer.id as i32,
            trainer_index: team.trainer.index as i32,
            outfit: f64::from(team.trainer.outfit),
            phrase: team.trainer.phrase,
            mons,
            version: decoded.version as i32,
        })
    }

    fn to_clean(&self) -> Result<Team, TeamError> {
        let version = u32::try_from(self.version).map_err(|_| TeamError::field("version"))?;
        let trainer = Trainer {
            name: self.name.clone(),
            id: to_u32(self.trainer_id as f64, "trainer.id")?,
            index: to_u32(self.trainer_index as f64, "trainer.index")?,
            outfit: to_u32(self.outfit, "trainer.outfit")?,
            phrase: self.phrase.clone(),
        };

        let mut slots = Vec::with_capacity(PARTY_SIZE);
        for mon in &self.mons {
            if mon.species == 0 {
                // Empty slots have no semantic values. Canonicalize them
                // instead of rejecting stale globals from the host party.
                slots.push(empty_clean_slot());
                continue;
            }
            slots.push(PartySlot {
                species: to_u32(mon.species as f64, "slot.species")?,
                level: to_u32(mon.level as f64, "slot.level")?,
                sex: to_u32(mon.sex as f64, "slot.sex")?,
                experience: to_u32(mon.exp, "slot.experience")?,
                iv: to_u32(mon.iv as f64, "slot.iv")?,
                held_item: to_u32(mon.item as f64, "slot.held_item")?,
                selue: to_u64(mon.selue, "slot.selue")?,
                moves: [
                    to_u32(mon.moves[0], "slot.moves[0]")?,
                    to_u32(mon.moves[1], "slot.moves[1]")?,
                    to_u32(mon.moves[2], "slot.moves[2]")?,
                    to_u32(mon.moves[3], "slot.moves[3]")?,
                ],
                info: mon
                    .info
                    .iter()
                    .enumerate()
                    .map(|(field, bytes)| info_to_clean(field, bytes))
                    .collect::<Result<Vec<_>, _>>()?,
            });
        }

        Ok(Team {
            source_version: Some(version),
            trainer,
            slots,
        })
    }
}

fn empty_clean_slot() -> PartySlot {
    PartySlot {
        species: 0,
        level: 0,
        sex: 0,
        experience: 0,
        iv: 0,
        held_item: 0,
        selue: 0,
        moves: [0; 4],
        info: INFO_KINDS
            .iter()
            .map(|kind| match kind {
                &INFO_KIND_NUMERIC => InfoCell::Numeric(0),
                &INFO_KIND_TEXT => InfoCell::Text(Vec::new()),
                _ => InfoCell::EncodedNumeric(0),
            })
            .collect(),
    }
}

fn info_to_host(cell: &InfoCell) -> Vec<u8> {
    match cell {
        InfoCell::Numeric(value) => value.to_string().into_bytes(),
        InfoCell::Text(value) => value.clone(),
        InfoCell::EncodedNumeric(value) => encode_encoded(*value),
    }
}

fn info_to_clean(field: usize, bytes: &[u8]) -> Result<InfoCell, TeamError> {
    match INFO_KINDS[field] {
        INFO_KIND_NUMERIC => Ok(InfoCell::Numeric(parse_host_integer(bytes)?)),
        INFO_KIND_TEXT => Ok(InfoCell::Text(bytes.to_vec())),
        INFO_KIND_ENCODED => Ok(InfoCell::EncodedNumeric(parse_encoded(bytes)?)),
        _ => Err(TeamError::new("info kind")),
    }
}

fn parse_host_integer(bytes: &[u8]) -> Result<i64, TeamError> {
    let text = std::str::from_utf8(bytes).map_err(|_| TeamError::field("info"))?;
    let value = text
        .trim()
        .parse::<f64>()
        .map_err(|_| TeamError::field("info"))?;
    if !value.is_finite() || (value - value.round()).abs() > 1e-9 {
        return Err(TeamError::field("info"));
    }
    let rounded = value.round();
    if rounded < i64::MIN as f64 || rounded > i64::MAX as f64 {
        return Err(TeamError::field("info"));
    }
    Ok(rounded as i64)
}

fn parse_encoded(bytes: &[u8]) -> Result<u64, TeamError> {
    if bytes.is_empty() {
        return Err(TeamError::field("info"));
    }
    let mut digits = String::with_capacity(bytes.len());
    for &byte in bytes {
        let digit = match byte {
            b'A' => '0',
            b'E' | b'I' | b'O' | b'U' | b'Y' => '0',
            b'F' => '1',
            b'R' | b'N' | b'D' | b'S' | b'G' | b'V' | b'L' => '1',
            b'0'..=b'9' => byte as char,
            _ => return Err(TeamError::field("info")),
        };
        digits.push(digit);
    }
    digits.parse::<u64>().map_err(|_| TeamError::field("info"))
}

fn encode_encoded(value: u64) -> Vec<u8> {
    if value == 0 {
        return b"A".to_vec();
    }
    let mut digits = Vec::new();
    let mut remaining = value;
    while remaining > 0 {
        digits.push((remaining % 10) as u8);
        remaining /= 10;
    }
    digits.reverse();
    digits
        .into_iter()
        .map(|digit| match digit {
            0 => b'A',
            1 => b'F',
            other => b'0' + other,
        })
        .collect()
}

fn raw_info_line(data: &[u8], slot: usize, field: usize) -> Option<Vec<u8>> {
    let mut lines: Vec<&[u8]> = data.split(|byte| *byte == b'\n').collect();
    for line in &mut lines {
        if line.last() == Some(&b'\r') {
            *line = &line[..line.len() - 1];
        }
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let index = 5 + 27 * slot + 11 + field;
    lines.get(index).map(|line| line.to_vec())
}

fn to_u32(value: f64, field: &'static str) -> Result<u32, TeamError> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < 0.0
        || value > f64::from(u32::MAX)
    {
        return Err(TeamError::field(field));
    }
    Ok(value as u32)
}

fn to_u64(value: f64, field: &'static str) -> Result<u64, TeamError> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < 0.0
        || value > u64::MAX as f64
    {
        return Err(TeamError::field(field));
    }
    let result = value as u64;
    if result as f64 != value {
        return Err(TeamError::field(field));
    }
    Ok(result)
}

pub(crate) fn encode_number_text(value: &[u8]) -> Vec<u8> {
    value
        .iter()
        .map(|byte| match byte {
            b'0' => b'A',
            b'1' => b'F',
            _ => *byte,
        })
        .collect()
}

pub(crate) fn decode_number_text(value: &[u8]) -> Vec<u8> {
    value
        .iter()
        .map(|byte| match byte {
            b'A' | b'E' | b'I' | b'O' | b'U' | b'Y' => b'0',
            b'F' | b'R' | b'N' | b'D' | b'S' | b'G' | b'V' | b'L' => b'1',
            _ => *byte,
        })
        .collect()
}

pub(crate) fn line_number(line: &[u8]) -> f64 {
    std::str::from_utf8(line)
        .ok()
        .and_then(|text| text.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

pub(crate) fn number_text(value: f64) -> Vec<u8> {
    if value.fract() == 0.0 {
        format!("{value:.0}").into_bytes()
    } else {
        format!("{value:.2}").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TeamFile {
        let mut mons = std::array::from_fn(|_| TeamMon::empty());
        for (slot, mon) in mons.iter_mut().enumerate() {
            *mon = TeamMon {
                species: 1 + slot as i32,
                level: 5 + slot as i32,
                sex: 1,
                exp: 100.0,
                iv: 5 + slot as i32,
                item: 0,
                selue: (9_000.0 - (1.0 + slot as f64)) * 1_000_000.0 + 555_555.0,
                moves: [1.0, 2.0, 3.0, 4.0],
                info: std::array::from_fn(|field| {
                    if field == 4 {
                        b"Ash".to_vec()
                    } else if field == 5 {
                        b"000123".to_vec()
                    } else if field == 10 || field == 11 {
                        b"A".to_vec()
                    } else {
                        b"0".to_vec()
                    }
                }),
            };
        }
        TeamFile {
            name: b"Ash".to_vec(),
            trainer_id: 1,
            trainer_index: 1,
            outfit: 1.0,
            phrase: b"Hello".to_vec(),
            mons,
            version: CURRENT_VERSION,
        }
    }

    #[test]
    fn round_trip_host_model() {
        let original = sample();
        let data = original.encode().expect("encode");
        let decoded = TeamFile::decode(&data).expect("decode");
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.trainer_id, original.trainer_id);
        assert_eq!(decoded.trainer_index, original.trainer_index);
        assert_eq!(decoded.outfit, original.outfit);
        assert_eq!(decoded.version, original.version);
        assert_eq!(decoded.mons[0].species, original.mons[0].species);
        assert_eq!(decoded.mons[5].moves, original.mons[5].moves);
        assert_eq!(decoded.mons[3].info[4], original.mons[3].info[4]);
        // Profile R text-kind fields are byte-preserving even when all digits.
        assert_eq!(decoded.mons[3].info[5], b"000123");
    }

    #[test]
    fn empty_slots_are_normalized_on_export() {
        let mut team = sample();
        team.mons[5] = TeamMon::empty();
        team.mons[5].level = 99;
        team.mons[5].info[4] = b"stale".to_vec();
        let data = team.encode().expect("empty slot normalizes");
        let decoded = TeamFile::decode(&data).expect("decode");
        assert_eq!(decoded.mons[5].level, 0);
        assert!(decoded.mons[5]
            .info
            .iter()
            .all(|line| line.is_empty() || line == b"0" || line == b"A"));
    }

    #[test]
    fn profile_r_ranges_are_enforced() {
        let mut team = sample();
        team.mons[0].iv = 555_555;
        assert!(team.encode().is_err());
        assert!(team.validate().is_err());

        let mut team = sample();
        for mon in &mut team.mons {
            mon.species = 0;
        }
        assert!(team.encode().is_err());
    }

    #[test]
    fn checksum_rejection_is_atomic() {
        let mut data = sample().encode().expect("encode");
        let index = data.len() / 2;
        data[index] = if data[index] == b'0' { b'1' } else { b'0' };
        assert!(TeamFile::decode(&data).is_err());
        assert!(TeamFile::decode(&data[..data.len() / 2]).is_err());
    }

    #[test]
    fn non_utf8_text_survives_round_trip() {
        let mut team = sample();
        team.name = b"Ash\xe9".to_vec();
        team.mons[0].info[4] = b"OT\xe9".to_vec();
        let data = team.encode().expect("encode");
        let decoded = TeamFile::decode(&data).expect("decode");
        assert_eq!(decoded.name, b"Ash\xe9");
        assert_eq!(decoded.mons[0].info[4], b"OT\xe9");
    }

    #[test]
    fn legacy_outfit_is_adjusted_once_by_the_host() {
        let mut team = sample();
        team.trainer_index = 10;
        team.outfit = 212.0;
        team.version = f3r::team::DEFAULT_VERSION;
        let data = team.encode().expect("legacy encode");
        let decoded = TeamFile::decode(&data).expect("decode");
        assert_eq!(decoded.outfit, 211.0, "codec must return the raw wire value");
        let mut host_char2 = decoded.outfit;
        if decoded.version < f3r::team::LEGACY_OUTFIT_CUTOVER && host_char2 > f3r::team::CUSTOM_OUTFIT_LIMIT {
            host_char2 += 1.0;
        }
        assert_eq!(host_char2, 212.0, "host rule applies the +1 exactly once");
    }

    #[test]
    fn helper_number_text_contract_is_unchanged() {
        assert_eq!(number_text(7.0), b"7");
        assert_eq!(number_text(7.5), b"7.50");
        assert_eq!(encode_number_text(b"101"), b"FAF");
        assert_eq!(decode_number_text(b"FAF"), b"101");
        assert_eq!(line_number(b" 12.5 "), 12.5);
    }
}
