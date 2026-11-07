//! Clean-room `.TEAM` codec (Profile R).

#![allow(dead_code)]

use std::fmt;

pub const SECRET_LINE_COUNT: usize = 167;
pub const CURRENT_VERSION: u32 = 20260802;
pub const DEFAULT_VERSION: u32 = 20240713;
pub const LEGACY_CUTOVER_VERSION: u32 = 20241217;
pub const MIN_VERSION: u32 = 20080101;
pub const MAX_VERSION: u32 = 21001231;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCategory {
    InputEmpty,
    InputTooLarge,
    LineFraming,
    TruncatedBody,
    MissingChecksum,
    UnexpectedTrailingData,
    ChecksumSyntax,
    ChecksumMismatch,
    VersionSyntax,
    VersionDate,
    VersionUnsupported,
    ExpectedNumber,
    NotInteger,
    NonFinite,
    NumberOutOfRange,
    NumberNotCanonical,
    TextTooLong,
    TextControl,
    NameEmpty,
    NoPokemon,
    SpeciesInvalid,
    LevelInvalid,
    SexInvalid,
    ExperienceInvalid,
    IvInvalid,
    ItemInvalid,
    SelueInvalid,
    MoveInvalid,
    InfoKindMismatch,
    InfoEncodingInvalid,
    EmptySlotNonCanonical,
    LegacyOutfitUnrepresentable,
    EnvelopePrefix,
    EnvelopeMetadataCount,
    EnvelopeMetadataSyntax,
    EnvelopeTeamEmpty,
    EnvelopeTooLarge,
    InternalLimit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamError {
    pub category: ErrorCategory,
    pub path: Option<&'static str>,
    pub line: Option<usize>,
}

impl TeamError {
    pub fn new(category: ErrorCategory) -> Self {
        Self {
            category,
            path: None,
            line: None,
        }
    }

    fn at(mut self, path: &'static str, line: Option<usize>) -> Self {
        self.path = Some(path);
        self.line = line;
        self
    }
}

impl fmt::Display for TeamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.category)
    }
}

impl std::error::Error for TeamError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InfoCell {
    Numeric(i64),
    Text(Vec<u8>),
    EncodedNumeric(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trainer {
    pub name: Vec<u8>,
    pub id: u32,
    pub index: u32,
    pub outfit: u32,
    pub phrase: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartySlot {
    pub species: u32,
    pub level: u32,
    pub sex: u32,
    pub experience: u32,
    pub iv: u32,
    pub held_item: u32,
    pub selue: u64,
    pub moves: [u32; 4],
    pub info: Vec<InfoCell>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Team {
    pub source_version: Option<u32>,
    pub trainer: Trainer,
    pub slots: Vec<PartySlot>,
}

#[derive(Clone, Debug)]
pub struct DecodeOptions {
    pub strict_canonical: bool,
    pub allow_legacy_version_adjustment: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            strict_canonical: false,
            allow_legacy_version_adjustment: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EncodeOptions {
    pub version: Option<u32>,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self { version: None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeResult {
    pub team: Team,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeResult {
    pub bytes: Vec<u8>,
    pub version: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InfoKind {
    Numeric,
    Text,
    Encoded,
}

const INFO_KINDS: [InfoKind; 16] = [
    InfoKind::Numeric,
    InfoKind::Numeric,
    InfoKind::Numeric,
    InfoKind::Numeric,
    InfoKind::Text,
    InfoKind::Text,
    InfoKind::Numeric,
    InfoKind::Text,
    InfoKind::Text,
    InfoKind::Numeric,
    InfoKind::Encoded,
    InfoKind::Encoded,
    InfoKind::Numeric,
    InfoKind::Numeric,
    InfoKind::Numeric,
    InfoKind::Numeric,
];

fn err(category: ErrorCategory) -> TeamError {
    TeamError::new(category)
}

fn is_ascii_space(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = value.split_first() {
        if !is_ascii_space(first) {
            break;
        }
        value = rest;
    }
    while let Some((&last, rest)) = value.split_last() {
        if !is_ascii_space(last) {
            break;
        }
        value = rest;
    }
    value
}

fn decimal_integer_syntax(value: &[u8]) -> bool {
    if value.is_empty() {
        return false;
    }
    let digits = match value[0] {
        b'+' | b'-' => &value[1..],
        _ => value,
    };
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

fn parse_general_integer(line: &[u8]) -> Result<i128, ErrorCategory> {
    let trimmed = trim_ascii(line);
    if trimmed.is_empty() {
        return Err(ErrorCategory::ExpectedNumber);
    }
    if decimal_integer_syntax(trimmed) {
        let text = std::str::from_utf8(trimmed).map_err(|_| ErrorCategory::ExpectedNumber)?;
        return text
            .parse::<i128>()
            .map_err(|_| ErrorCategory::NumberOutOfRange);
    }
    let text = std::str::from_utf8(trimmed).map_err(|_| ErrorCategory::ExpectedNumber)?;
    let value = text
        .parse::<f64>()
        .map_err(|_| ErrorCategory::ExpectedNumber)?;
    if !value.is_finite() {
        return Err(ErrorCategory::NonFinite);
    }
    let rounded = value.round();
    let tolerance = 1e-9f64.max(value.abs() * 1e-12);
    if (value - rounded).abs() > tolerance {
        return Err(ErrorCategory::NotInteger);
    }
    if rounded < i128::MIN as f64 || rounded > i128::MAX as f64 {
        return Err(ErrorCategory::NumberOutOfRange);
    }
    Ok(rounded as i128)
}

fn parse_checksum_value(line: &[u8]) -> Result<i128, TeamError> {
    parse_general_integer(line).map_err(|_| err(ErrorCategory::ChecksumSyntax))
}

fn decode_affine(line: &[u8], factor: i128, offset: i128) -> Result<i128, TeamError> {
    let wire = parse_general_integer(line).map_err(err)?;
    let difference = wire
        .checked_sub(offset)
        .ok_or_else(|| err(ErrorCategory::NumberOutOfRange))?;
    if difference % factor != 0 {
        return Err(err(ErrorCategory::NotInteger));
    }
    Ok(difference / factor)
}

fn check_range(
    value: i128,
    minimum: i128,
    maximum: i128,
    category: ErrorCategory,
) -> Result<(), TeamError> {
    if value < minimum || value > maximum {
        Err(err(category))
    } else {
        Ok(())
    }
}

fn validate_text(value: &[u8], maximum: usize) -> Result<(), TeamError> {
    if value.len() > maximum {
        return Err(err(ErrorCategory::TextTooLong));
    }
    if value.iter().any(|&byte| byte < 0x20 || byte == 0x7f) {
        return Err(err(ErrorCategory::TextControl));
    }
    Ok(())
}

fn integer_literal_value(value: &[u8]) -> Option<i64> {
    if !decimal_integer_syntax(value) {
        return None;
    }
    let text = std::str::from_utf8(value).ok()?;
    let number = text.parse::<i128>().ok()?;
    let canonical = number.to_string();
    if canonical.as_bytes() != value {
        return None;
    }
    i64::try_from(number).ok()
}

fn decode_encoded_cell(line: &[u8]) -> Result<(u64, bool), TeamError> {
    if line.is_empty() {
        return Err(err(ErrorCategory::InfoEncodingInvalid));
    }
    let mut digits = Vec::with_capacity(line.len());
    let mut canonical = true;
    for &byte in line {
        let digit = match byte {
            b'A' => b'0',
            b'E' | b'I' | b'O' | b'U' | b'Y' => {
                canonical = false;
                b'0'
            }
            b'F' => b'1',
            b'R' | b'N' | b'D' | b'S' | b'G' | b'V' | b'L' => {
                canonical = false;
                b'1'
            }
            b'0'..=b'9' => {
                if byte == b'0' || byte == b'1' {
                    canonical = false;
                }
                byte
            }
            _ => return Err(err(ErrorCategory::InfoEncodingInvalid)),
        };
        digits.push(digit);
    }
    if digits.len() > 1 && digits[0] == b'0' {
        canonical = false;
    }
    let text = std::str::from_utf8(&digits).map_err(|_| err(ErrorCategory::InfoEncodingInvalid))?;
    let value = text
        .parse::<u64>()
        .map_err(|_| err(ErrorCategory::InfoEncodingInvalid))?;
    Ok((value, canonical))
}

fn encode_encoded_cell(value: u64) -> Vec<u8> {
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

fn checked_info_range(index: usize, value: i128) -> Result<(), TeamError> {
    let range = match index {
        0 => (0, 99999),
        1 => (0, 100),
        2 => (0, 65535),
        3 => (0, 255),
        6 => (0, 65535),
        9 | 13 | 15 => (0, 2147483647),
        12 => (-2147483648, 2147483647),
        14 => (0, 9999999999999999),
        _ => return Ok(()),
    };
    check_range(value, range.0, range.1, ErrorCategory::NumberOutOfRange)
}

fn zero_info_cells() -> Vec<InfoCell> {
    INFO_KINDS
        .iter()
        .map(|kind| match kind {
            InfoKind::Numeric => InfoCell::Numeric(0),
            InfoKind::Text => InfoCell::Text(Vec::new()),
            InfoKind::Encoded => InfoCell::EncodedNumeric(0),
        })
        .collect()
}

fn split_lines(input: &[u8]) -> (Vec<Vec<u8>>, bool) {
    let mut lines = Vec::new();
    let mut noncanonical = false;
    let mut start = 0usize;
    while start < input.len() {
        if let Some(relative) = input[start..].iter().position(|&byte| byte == b'\n') {
            let end = start + relative;
            let mut segment = &input[start..end];
            if let Some((&b'\r', rest)) = segment.split_last() {
                segment = rest;
            } else {
                noncanonical = true;
            }
            lines.push(segment.to_vec());
            start = end + 1;
        } else {
            let segment = &input[start..];
            if !segment.is_empty() {
                lines.push(segment.to_vec());
                noncanonical = true;
            }
            break;
        }
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
        noncanonical = true;
    }
    (lines, noncanonical)
}

fn parse_version(line: &[u8]) -> Result<u32, TeamError> {
    let trimmed = trim_ascii(line);
    if trimmed.is_empty() {
        return Ok(DEFAULT_VERSION);
    }
    if trimmed.len() != 8 || !trimmed.iter().all(u8::is_ascii_digit) {
        return Err(err(ErrorCategory::VersionSyntax));
    }
    let text = std::str::from_utf8(trimmed).map_err(|_| err(ErrorCategory::VersionSyntax))?;
    let value: u32 = text
        .parse()
        .map_err(|_| err(ErrorCategory::VersionSyntax))?;
    if value < MIN_VERSION || value > MAX_VERSION {
        return Err(err(ErrorCategory::VersionUnsupported));
    }
    if !valid_ymd(value) {
        return Err(err(ErrorCategory::VersionDate));
    }
    Ok(value)
}

fn valid_ymd(value: u32) -> bool {
    let year = value / 10000;
    let month = (value / 100) % 100;
    let day = value % 100;
    if month < 1 || month > 12 || day < 1 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    day <= days
}

pub fn checksum(lines: &[&[u8]]) -> Result<u64, TeamError> {
    if lines.len() != SECRET_LINE_COUNT {
        return Err(err(ErrorCategory::TruncatedBody));
    }
    let mut secret = Vec::new();
    for line in lines {
        secret.extend_from_slice(line);
    }
    let n = secret.len();
    let mut accumulator: i64 = 0;
    for k in 0..=n {
        let byte = if k == 0 {
            secret.first().copied().unwrap_or(0) as i64
        } else {
            secret[k - 1] as i64
        };
        let kk = k as i64;
        let term = match k % 8 {
            0 => 7 * byte + kk + 9,
            1 => 3 * byte + 2 * kk,
            2 => 5 * byte + kk + (kk + 2) / 3,
            3 => 9 * byte + kk + 53,
            4 => 15 * byte + 3 * kk,
            5 => (3 * byte + kk) ^ 2,
            6 => 7 * byte + 37 * kk,
            _ => {
                let floor = kk / 2;
                let rounded = if kk % 2 == 0 || floor % 2 == 0 {
                    floor
                } else {
                    floor + 1
                };
                9 * byte + kk + rounded
            }
        };
        accumulator = accumulator.wrapping_add(term);
    }
    let result = accumulator.wrapping_mul(7).wrapping_sub(20053).wrapping_abs();
    Ok(result as u64)
}

fn decode_slot(
    lines: &[Vec<u8>],
    index: usize,
    strict_canonical: bool,
) -> Result<PartySlot, TeamError> {
    let base = 5 + 27 * index;
    let i = index as i128;
    let species = decode_affine(&lines[base], 5 + i, 6852)?;
    let level = decode_affine(&lines[base + 1], 37 + i, 78)?;
    let sex = decode_affine(&lines[base + 2], 5, 34)?;
    let experience = decode_affine(&lines[base + 3], 3, 32657)?;
    let iv = decode_affine(&lines[base + 4], 23, 3127)?;
    let held_item = decode_affine(&lines[base + 5], 43, 29 * i)?;
    let selue = decode_affine(&lines[base + 6], 7, 678)?;
    let move_0 = decode_affine(&lines[base + 7], 13, 354)?;
    let move_1 = decode_affine(&lines[base + 8], 17, 323)?;
    let move_2 = decode_affine(&lines[base + 9], 19, 857)?;
    let move_3 = decode_affine(&lines[base + 10], 29, 354)?;

    let empty = species == 0;
    let mut info = Vec::with_capacity(16);
    for (info_index, kind) in INFO_KINDS.iter().enumerate() {
        let line = &lines[base + 11 + info_index];
        match kind {
            InfoKind::Numeric => {
                let value = parse_general_integer(line).map_err(err)?;
                if empty {
                    info.push(InfoCell::Numeric(0));
                } else {
                    checked_info_range(info_index, value)?;
                    info.push(InfoCell::Numeric(value as i64));
                }
            }
            InfoKind::Text => {
                if empty {
                    info.push(InfoCell::Text(Vec::new()));
                } else if let Some(value) = integer_literal_value(line) {
                    info.push(InfoCell::Numeric(value));
                } else {
                    validate_text(line, 256)?;
                    info.push(InfoCell::Text(line.clone()));
                }
            }
            InfoKind::Encoded => {
                let (value, canonical) = decode_encoded_cell(line)?;
                if strict_canonical && !canonical {
                    return Err(err(ErrorCategory::InfoEncodingInvalid));
                }
                if empty {
                    info.push(InfoCell::EncodedNumeric(0));
                } else {
                    check_range(
                        value as i128,
                        0,
                        4294967295,
                        ErrorCategory::NumberOutOfRange,
                    )?;
                    info.push(InfoCell::EncodedNumeric(value));
                }
            }
        }
    }

    if empty {
        return Ok(PartySlot {
            species: 0,
            level: 0,
            sex: 0,
            experience: 0,
            iv: 0,
            held_item: 0,
            selue: 0,
            moves: [0, 0, 0, 0],
            info: zero_info_cells(),
        });
    }

    check_range(species, 1, 65535, ErrorCategory::SpeciesInvalid)?;
    check_range(level, 1, 100, ErrorCategory::LevelInvalid)?;
    check_range(sex, 0, 2, ErrorCategory::SexInvalid)?;
    check_range(
        experience,
        0,
        2147483647,
        ErrorCategory::ExperienceInvalid,
    )?;
    check_range(iv, 0, 9_999_999, ErrorCategory::IvInvalid)?;
    check_range(held_item, 0, 65535, ErrorCategory::ItemInvalid)?;
    check_range(selue, 0, 9999999999999999, ErrorCategory::SelueInvalid)?;
    check_range(move_0, 0, 65535, ErrorCategory::MoveInvalid)?;
    check_range(move_1, 0, 65535, ErrorCategory::MoveInvalid)?;
    check_range(move_2, 0, 65535, ErrorCategory::MoveInvalid)?;
    check_range(move_3, 0, 65535, ErrorCategory::MoveInvalid)?;

    Ok(PartySlot {
        species: species as u32,
        level: level as u32,
        sex: sex as u32,
        experience: experience as u32,
        iv: iv as u32,
        held_item: held_item as u32,
        selue: selue as u64,
        moves: [
            move_0 as u32,
            move_1 as u32,
            move_2 as u32,
            move_3 as u32,
        ],
        info,
    })
}

pub fn decode(input: &[u8], options: &DecodeOptions) -> Result<DecodeResult, TeamError> {
    if input.is_empty() {
        return Err(err(ErrorCategory::InputEmpty));
    }
    if input.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(err(ErrorCategory::LineFraming));
    }
    let (lines, noncanonical_endings) = split_lines(input);
    if lines.len() < SECRET_LINE_COUNT + 1 {
        return Err(err(ErrorCategory::TruncatedBody));
    }
    if options.strict_canonical && noncanonical_endings {
        return Err(err(ErrorCategory::NumberNotCanonical));
    }

    let payload = &lines[..SECRET_LINE_COUNT];
    let refs: Vec<&[u8]> = payload.iter().map(|line| line.as_slice()).collect();
    let computed = checksum(&refs)?;
    let stored = parse_checksum_value(&lines[SECRET_LINE_COUNT])?;
    if stored != computed as i128 {
        return Err(err(ErrorCategory::ChecksumMismatch));
    }

    let explicit_version = lines
        .get(SECRET_LINE_COUNT + 1)
        .is_some_and(|line| !trim_ascii(line).is_empty());
    let version = match lines.get(SECRET_LINE_COUNT + 1) {
        Some(line) => parse_version(line)?,
        None => DEFAULT_VERSION,
    };

    let name = &lines[0];
    validate_text(name, 64)?;
    if name.is_empty() {
        return Err(err(ErrorCategory::NameEmpty));
    }
    let id = decode_affine(&lines[1], 54, 99)?;
    check_range(id, 0, 99999, ErrorCategory::NumberOutOfRange)?;
    let index = decode_affine(&lines[2], 3, 749104)?;
    check_range(index, 0, 255, ErrorCategory::NumberOutOfRange)?;
    let outfit = decode_affine(&lines[3], 68, 53)?;
    check_range(outfit, 0, 65535, ErrorCategory::NumberOutOfRange)?;
    let phrase = &lines[4];
    validate_text(phrase, 256)?;

    let mut slots = Vec::with_capacity(6);
    let mut has_pokemon = false;
    for slot_index in 0..6 {
        let slot = decode_slot(&lines, slot_index, options.strict_canonical)?;
        if slot.species != 0 {
            has_pokemon = true;
        }
        slots.push(slot);
    }
    if !has_pokemon {
        return Err(err(ErrorCategory::NoPokemon));
    }

    let mut effective_outfit = outfit as u32;
    if options.allow_legacy_version_adjustment
        && explicit_version
        && version < LEGACY_CUTOVER_VERSION
        && index == 10
        && effective_outfit > 210
    {
        effective_outfit += 1;
    }

    Ok(DecodeResult {
        team: Team {
            source_version: Some(version),
            trainer: Trainer {
                name: name.clone(),
                id: id as u32,
                index: index as u32,
                outfit: effective_outfit,
                phrase: phrase.clone(),
            },
            slots,
        },
        version,
    })
}

fn validate_info_for_encode(index: usize, cell: &InfoCell) -> Result<(), TeamError> {
    match (INFO_KINDS[index], cell) {
        (InfoKind::Numeric, InfoCell::Numeric(value)) => {
            checked_info_range(index, *value as i128)
        }
        (InfoKind::Text, InfoCell::Text(value)) => validate_text(value, 256),
        (InfoKind::Text, InfoCell::Numeric(_)) => Ok(()),
        (InfoKind::Encoded, InfoCell::EncodedNumeric(value)) => {
            check_range(*value as i128, 0, 4294967295, ErrorCategory::NumberOutOfRange)
        }
        _ => Err(err(ErrorCategory::InfoKindMismatch)),
    }
}

fn info_is_empty(cell: &InfoCell) -> bool {
    match cell {
        InfoCell::Numeric(value) => *value == 0,
        InfoCell::Text(value) => value.is_empty(),
        InfoCell::EncodedNumeric(value) => *value == 0,
    }
}

fn encode_info_line(index: usize, cell: &InfoCell) -> Result<Vec<u8>, TeamError> {
    validate_info_for_encode(index, cell)?;
    match cell {
        InfoCell::Numeric(value) => Ok(value.to_string().into_bytes()),
        InfoCell::Text(value) => Ok(value.clone()),
        InfoCell::EncodedNumeric(value) => Ok(encode_encoded_cell(*value)),
    }
}

pub fn encode(team: &Team, options: &EncodeOptions) -> Result<EncodeResult, TeamError> {
    let version = options.version.unwrap_or(CURRENT_VERSION);
    if version < MIN_VERSION || version > MAX_VERSION {
        return Err(err(ErrorCategory::VersionUnsupported));
    }
    if !valid_ymd(version) {
        return Err(err(ErrorCategory::VersionDate));
    }

    validate_text(&team.trainer.name, 64)?;
    if team.trainer.name.is_empty() {
        return Err(err(ErrorCategory::NameEmpty));
    }
    check_range(
        team.trainer.id as i128,
        0,
        99999,
        ErrorCategory::NumberOutOfRange,
    )?;
    check_range(
        team.trainer.index as i128,
        0,
        255,
        ErrorCategory::NumberOutOfRange,
    )?;
    check_range(
        team.trainer.outfit as i128,
        0,
        65535,
        ErrorCategory::NumberOutOfRange,
    )?;
    validate_text(&team.trainer.phrase, 256)?;

    if team.slots.len() != 6 {
        return Err(err(ErrorCategory::NumberOutOfRange));
    }

    let mut lines: Vec<Vec<u8>> = Vec::with_capacity(SECRET_LINE_COUNT);
    lines.push(team.trainer.name.clone());
    lines.push((team.trainer.id as i128 * 54 + 99).to_string().into_bytes());
    lines.push(
        (team.trainer.index as i128 * 3 + 749104)
            .to_string()
            .into_bytes(),
    );

    let mut wire_outfit = team.trainer.outfit;
    if version < LEGACY_CUTOVER_VERSION && team.trainer.index == 10 {
        if team.trainer.outfit == 211 {
            return Err(err(ErrorCategory::LegacyOutfitUnrepresentable));
        }
        if team.trainer.outfit > 211 {
            wire_outfit -= 1;
        }
    }
    lines.push(
        (wire_outfit as i128 * 68 + 53)
            .to_string()
            .into_bytes(),
    );
    lines.push(team.trainer.phrase.clone());

    let mut has_pokemon = false;
    for (index, slot) in team.slots.iter().enumerate() {
        let i = index as i128;
        if slot.species != 0 {
            has_pokemon = true;
        }

        if slot.species == 0 {
            if slot.level != 0
                || slot.sex != 0
                || slot.experience != 0
                || slot.iv != 0
                || slot.held_item != 0
                || slot.selue != 0
                || slot.moves != [0, 0, 0, 0]
                || slot.info.len() != 16
                || slot.info.iter().any(|cell| !info_is_empty(cell))
            {
                return Err(err(ErrorCategory::EmptySlotNonCanonical));
            }
        } else {
            check_range(
                slot.species as i128,
                1,
                65535,
                ErrorCategory::SpeciesInvalid,
            )?;
            check_range(slot.level as i128, 1, 100, ErrorCategory::LevelInvalid)?;
            check_range(
                slot.experience as i128,
                0,
                2147483647,
                ErrorCategory::ExperienceInvalid,
            )?;
            check_range(slot.iv as i128, 0, 9_999_999, ErrorCategory::IvInvalid)?;
            check_range(
                slot.held_item as i128,
                0,
                65535,
                ErrorCategory::ItemInvalid,
            )?;
            check_range(
                slot.selue as i128,
                0,
                9999999999999999,
                ErrorCategory::SelueInvalid,
            )?;
            for &move_id in &slot.moves {
                check_range(move_id as i128, 0, 65535, ErrorCategory::MoveInvalid)?;
            }
            if slot.info.len() != 16 {
                return Err(err(ErrorCategory::InfoKindMismatch));
            }
        }
        check_range(slot.sex as i128, 0, 2, ErrorCategory::SexInvalid)?;

        lines.push(
            (slot.species as i128 * (5 + i) + 6852)
                .to_string()
                .into_bytes(),
        );
        lines.push(
            (slot.level as i128 * (37 + i) + 78)
                .to_string()
                .into_bytes(),
        );
        lines.push((slot.sex as i128 * 5 + 34).to_string().into_bytes());
        lines.push(
            (slot.experience as i128 * 3 + 32657)
                .to_string()
                .into_bytes(),
        );
        lines.push((slot.iv as i128 * 23 + 3127).to_string().into_bytes());
        lines.push(
            (slot.held_item as i128 * 43 + 29 * i)
                .to_string()
                .into_bytes(),
        );
        lines.push((slot.selue as i128 * 7 + 678).to_string().into_bytes());
        lines.push(
            (slot.moves[0] as i128 * 13 + 354)
                .to_string()
                .into_bytes(),
        );
        lines.push(
            (slot.moves[1] as i128 * 17 + 323)
                .to_string()
                .into_bytes(),
        );
        lines.push(
            (slot.moves[2] as i128 * 19 + 857)
                .to_string()
                .into_bytes(),
        );
        lines.push(
            (slot.moves[3] as i128 * 29 + 354)
                .to_string()
                .into_bytes(),
        );

        if slot.species == 0 {
            for kind in INFO_KINDS.iter() {
                let line = match kind {
                    InfoKind::Numeric => b"0".to_vec(),
                    InfoKind::Text => Vec::new(),
                    InfoKind::Encoded => b"A".to_vec(),
                };
                lines.push(line);
            }
        } else {
            for (info_index, cell) in slot.info.iter().enumerate() {
                lines.push(encode_info_line(info_index, cell)?);
            }
        }
    }

    if !has_pokemon {
        return Err(err(ErrorCategory::NoPokemon));
    }
    if lines.len() != SECRET_LINE_COUNT {
        return Err(err(ErrorCategory::InternalLimit));
    }
    let refs: Vec<&[u8]> = lines.iter().map(|line| line.as_slice()).collect();
    let sum = checksum(&refs)?;
    lines.push(sum.to_string().into_bytes());
    lines.push(version.to_string().into_bytes());

    let mut bytes = Vec::new();
    for line in &lines {
        bytes.extend_from_slice(line);
        bytes.extend_from_slice(b"\r\n");
    }
    Ok(EncodeResult { bytes, version })
}

pub fn canonicalize(input: &[u8], options: &DecodeOptions) -> Result<Vec<u8>, TeamError> {
    let decoded = decode(input, options)?;
    let encoded = encode(&decoded.team, &EncodeOptions::default())?;
    Ok(encoded.bytes)
}

#[cfg(test)]
#[path = "team_file_tests.rs"]
mod tests;
