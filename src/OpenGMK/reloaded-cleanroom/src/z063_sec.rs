//! Security validation for the Zibelan 0.6.3 world/battle protocol.
//!
//! The 0.6.3 profile adds `WTID`/`WTSPR`, a 37-field `WCFG`, five `TEAM`
//! metadata fields, `IN`/`SUM` lockstep frames and `BYE`. These validators are
//! pure: they never open a socket, run GML, or mutate game state. They are kept
//! strict enough to reject spoofed/replayed control frames while accepting the
//! official 0.6.3 producer and the older `WVER 6` handshake.
//!
//! Framing is owned by `ROZ.dll`; this module validates the application
//! payloads described by the static analysis of the extracted 0.6.3 GML:
//!
//! - `WTID` is decimal digits only; `WTSPR` is `-1` (unset) or `0..=3856`.
//! - `WVER`/`WBUILD` must be the exact `(6, 20261103)` or `(8, 20261109)` pair.
//! - `TEAM` carries five `RS`-terminated numeric metadata fields and an opaque
//!   `.TEAM` body.
//! - `WCFG` carries 37 `GS`-separated fields (up to 40 are accepted because the
//!   GML parser ignores extras).
//! - `IN`/`SUM`/`BYE` are bounded by the local frame window and the battle
//!   epoch; duplicated frames must not be allowed to rewrite earlier values.

use std::collections::{HashMap, HashSet};
use std::fmt;

/// Separator between a command and its body in the 0.6.3 payloads.
pub const UNIT_SEPARATOR: u8 = 0x1f;
/// Metadata/record separator used by `TEAM` and `SUM`.
pub const RECORD_SEPARATOR: u8 = 0x1e;
/// Field separator used by `WCFG`.
pub const FIELD_SEPARATOR: u8 = 0x1d;

/// Legacy profile announced by the 0.6.7 host and the capture-derived notes.
pub const WVER_LEGACY: u32 = 6;
pub const WBUILD_LEGACY: u64 = 20_261_103;
/// Profile announced by the extracted 0.6.3 GML (`NET_VERSION`/`NET_BUILD`).
pub const WVER_063: u32 = 8;
pub const WBUILD_063: u64 = 20_261_109;

/// `WJOIN` name cap from `NetCleanText(SayMyName(), 16)`.
pub const MAX_JOIN_NAME_BYTES: usize = 16;
/// The game writes five decimal digits but pads/loads them as a string.
pub const MAX_WTID_DIGITS: usize = 10;
pub const MAX_WTID: u64 = u32::MAX as u64;
/// `-1` is the game's "no sprite" sentinel; `NetWorldPart` accepts `0..3857`.
pub const MIN_WTSPR: i64 = -1;
pub const MAX_WTSPR: i64 = 3_856;

/// Number of fields produced by `NetConfigString`.
pub const WCFG_FIELD_COUNT: usize = 37;
/// `NetConfigApply` walks a 40-entry array, so extras are tolerated.
pub const WCFG_MAX_FIELD_COUNT: usize = 40;
pub const WCFG_NUMERIC_FIELD_COUNT: usize = 32;
pub const WCFG_TEXT_FIELD_COUNT: usize = 5;
pub const MAX_WCFG_BYTES: usize = 65_536;
pub const MAX_WCFG_TEXT_BYTES: usize = 4_096;

/// `TEAM` metadata fields before the raw `.TEAM` body.
pub const TEAM_META_FIELD_COUNT: usize = 5;
pub const MAX_TEAM_BYTES: usize = 1_048_576;
pub const MIN_SEED: i64 = 1;
pub const MAX_SEED: i64 = 1_000_000_000;
pub const MIN_DELAY: i64 = 2;
pub const MAX_DELAY: i64 = 10;

/// `NetChecksum` computes `c = (c*31 + ord) mod 1000003`.
pub const CHECKSUM_MODULUS: i64 = 1_000_003;
pub const MAX_CHECKSUM: i64 = CHECKSUM_MODULUS - 1;
/// Bits exchanged by `set_input`: 11 controls per side, with bit 15 as stall.
pub const INPUT_MASK_BITS: u32 = 22;
pub const MAX_INPUT_MASK: u32 = (1 << INPUT_MASK_BITS) - 1;
/// `NET_MINE` is a 256-slot ring; inputs outside it cannot be part of a frame.
pub const INPUT_WINDOW: u32 = 256;
/// Checksum messages arrive every 30 frames.
pub const CHECKSUM_INTERVAL: u32 = 30;
/// `NET_SUMF`/`NET_PSUMF` keep 64 checksum blocks (1920 frames).
pub const CHECKSUM_SLOTS: u32 = 64;
pub const CHECKSUM_STORAGE_WINDOW: u32 = CHECKSUM_SLOTS * CHECKSUM_INTERVAL;
/// Older checksums are treated as replays instead of being compared. Eight
/// blocks (240 frames, 8 s at 30 Hz) is far beyond the negotiated 2..=10 frame
/// delay and leaves room for a stalled peer without accepting an old replay.
pub const CHECKSUM_COMPARE_WINDOW: u32 = 8 * CHECKSUM_INTERVAL;
/// A peer cannot legitimately declare a final frame this far ahead.
pub const BYE_WINDOW: u32 = CHECKSUM_STORAGE_WINDOW;

/// Battle-background values produced by `BattleBackground`.
pub const BATTLE_BACKGROUND_RANGES: &[(i64, i64)] = &[(1, 253), (1_000, 1_063), (2_000, 2_004)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolProfile {
    Legacy06,
    Zibelan063,
}

impl ProtocolProfile {
    pub fn version(self) -> u32 {
        match self {
            Self::Legacy06 => WVER_LEGACY,
            Self::Zibelan063 => WVER_063,
        }
    }

    pub fn build(self) -> u64 {
        match self {
            Self::Legacy06 => WBUILD_LEGACY,
            Self::Zibelan063 => WBUILD_063,
        }
    }

    pub fn requires_trainer_id(self) -> bool {
        self == Self::Zibelan063
    }
}

/// Return the exact profile for a `WVER`/`WBUILD` pair.
pub fn protocol_profile(version: u32, build: u64) -> Result<ProtocolProfile, HandshakeError> {
    match (version, build) {
        (WVER_LEGACY, WBUILD_LEGACY) => Ok(ProtocolProfile::Legacy06),
        (WVER_063, WBUILD_063) => Ok(ProtocolProfile::Zibelan063),
        (WVER_LEGACY, _) | (WVER_063, _) => Err(HandshakeError::VersionBuildMismatch),
        _ => Err(HandshakeError::UnsupportedVersion),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainerId {
    digits: Vec<u8>,
    value: u64,
}

impl TrainerId {
    pub fn digits(&self) -> &[u8] {
        &self.digits
    }

    pub fn value(&self) -> u64 {
        self.value
    }
}

/// Validate `WTID`: non-empty decimal digits, at most [`MAX_WTID_DIGITS`].
///
/// Leading zeros are accepted because the game writes a five-digit trainer id.
pub fn validate_trainer_id(input: &[u8]) -> Result<TrainerId, HandshakeError> {
    if input.is_empty() {
        return Err(HandshakeError::TrainerIdEmpty);
    }
    if !input.iter().all(u8::is_ascii_digit) {
        return Err(HandshakeError::TrainerIdNotDigits);
    }
    if input.len() > MAX_WTID_DIGITS {
        return Err(HandshakeError::TrainerIdTooLong);
    }
    let mut value = 0u64;
    for &digit in input {
        value = value * 10 + u64::from(digit - b'0');
    }
    if value > MAX_WTID {
        return Err(HandshakeError::TrainerIdOverflow);
    }
    Ok(TrainerId { digits: input.to_vec(), value })
}

/// Validate `WTSPR`; `-1` is accepted as the "unset" sentinel used by the game.
pub fn validate_sprite(value: i64) -> Result<i32, HandshakeError> {
    if !(MIN_WTSPR..=MAX_WTSPR).contains(&value) {
        return Err(HandshakeError::SpriteOutOfRange);
    }
    Ok(value as i32)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldHandshake {
    pub name: Vec<u8>,
    pub trainer_id: Option<TrainerId>,
    pub sprite: Option<i32>,
    pub version: u32,
    pub build: u64,
    pub profile: ProtocolProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeError {
    EmptyName,
    NameTooLong,
    NameControl,
    MissingJoin,
    UnexpectedCommand,
    DuplicateField,
    MissingVersion,
    MissingBuild,
    VersionNotCanonical,
    BuildNotCanonical,
    UnsupportedVersion,
    VersionBuildMismatch,
    TrainerIdEmpty,
    TrainerIdNotDigits,
    TrainerIdTooLong,
    TrainerIdOverflow,
    SpriteOutOfRange,
    MissingTrainerId,
    SpriteWithoutTrainer,
    Order,
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for HandshakeError {}

/// Parse and validate the `NetWorldConnect` burst.
///
/// Accepted order is `WJOIN`, optional `WTID`, optional `WTSPR`, `WVER`,
/// `WBUILD`. The legacy three-line burst (`WJOIN`, `WVER`, `WBUILD`) remains
/// valid. Version 8 requires `WTID`; version 6 tolerates an identity prefix.
pub fn parse_world_handshake(lines: &[Vec<u8>]) -> Result<WorldHandshake, HandshakeError> {
    let mut commands = Vec::with_capacity(lines.len());
    for line in lines {
        commands.push(split_line(line)?);
    }
    let Some((first_name, first_args)) = commands.first() else {
        return Err(HandshakeError::MissingJoin);
    };
    if *first_name != b"WJOIN" {
        return Err(HandshakeError::MissingJoin);
    }
    let name = first_args.ok_or(HandshakeError::EmptyName)?;
    validate_name(name)?;

    let mut trainer_id = None;
    let mut sprite = None;
    let mut version = None;
    let mut build = None;
    let mut saw_sprite = false;
    let mut saw_version = false;
    let mut saw_build = false;

    for (command, args) in commands.iter().skip(1) {
        let args = args.ok_or(HandshakeError::UnexpectedCommand)?;
        match *command {
            b"WTID" => {
                if trainer_id.is_some() || saw_version || saw_build {
                    return Err(HandshakeError::Order);
                }
                trainer_id = Some(validate_trainer_id(args)?);
            },
            b"WTSPR" => {
                if trainer_id.is_none() || saw_sprite || saw_version || saw_build {
                    return Err(HandshakeError::Order);
                }
                saw_sprite = true;
                sprite = Some(validate_sprite(parse_signed(args)?)?);
            },
            b"WVER" => {
                if saw_version || saw_build {
                    return Err(HandshakeError::Order);
                }
                saw_version = true;
                version = Some(parse_version(args)?);
            },
            b"WBUILD" => {
                if !saw_version || saw_build {
                    return Err(HandshakeError::Order);
                }
                saw_build = true;
                build = Some(parse_build(args)?);
            },
            _ => return Err(HandshakeError::UnexpectedCommand),
        }
    }

    let version = version.ok_or(HandshakeError::MissingVersion)?;
    let build = build.ok_or(HandshakeError::MissingBuild)?;
    let profile = protocol_profile(version, build)?;
    if profile.requires_trainer_id() && trainer_id.is_none() {
        return Err(HandshakeError::MissingTrainerId);
    }
    if trainer_id.is_none() && sprite.is_some() {
        return Err(HandshakeError::SpriteWithoutTrainer);
    }

    Ok(WorldHandshake { name: name.to_vec(), trainer_id, sprite, version, build, profile })
}

fn split_line(line: &[u8]) -> Result<(&[u8], Option<&[u8]>), HandshakeError> {
    let mut line = line;
    if line.last() == Some(&b'\n') {
        line = &line[..line.len() - 1];
    }
    if line.last() == Some(&b'\r') {
        line = &line[..line.len() - 1];
    }
    if line.iter().any(|&byte| byte == b'\r' || byte == b'\n') {
        return Err(HandshakeError::NameControl);
    }
    match line.iter().position(|&byte| byte == b' ') {
        Some(index) => Ok((&line[..index], Some(&line[index + 1..]))),
        None => Ok((line, None)),
    }
}

fn validate_name(name: &[u8]) -> Result<(), HandshakeError> {
    if name.is_empty() {
        return Err(HandshakeError::EmptyName);
    }
    if name.len() > MAX_JOIN_NAME_BYTES {
        return Err(HandshakeError::NameTooLong);
    }
    if name.iter().any(|&byte| byte < 0x20 || byte == 0x7f) {
        return Err(HandshakeError::NameControl);
    }
    Ok(())
}

fn parse_version(input: &[u8]) -> Result<u32, HandshakeError> {
    match parse_canonical_unsigned(input, u32::MAX as u64) {
        Ok(value) => Ok(value as u32),
        Err(_) => Err(HandshakeError::VersionNotCanonical),
    }
}

fn parse_build(input: &[u8]) -> Result<u64, HandshakeError> {
    parse_canonical_unsigned(input, u64::MAX).map_err(|_| HandshakeError::BuildNotCanonical)
}

fn parse_signed(input: &[u8]) -> Result<i64, HandshakeError> {
    let (negative, digits) = match input.first() {
        Some(b'-') => (true, &input[1..]),
        _ => (false, input),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(HandshakeError::SpriteOutOfRange);
    }
    let value = parse_digits(digits).map_err(|_| HandshakeError::SpriteOutOfRange)?;
    if negative {
        let magnitude = i64::try_from(value).map_err(|_| HandshakeError::SpriteOutOfRange)?;
        Ok(-magnitude)
    } else {
        i64::try_from(value).map_err(|_| HandshakeError::SpriteOutOfRange)
    }
}

fn parse_canonical_unsigned(input: &[u8], max: u64) -> Result<u64, NumberError> {
    if input.is_empty() {
        return Err(NumberError::Empty);
    }
    if !input.iter().all(u8::is_ascii_digit) {
        return Err(NumberError::NotDigits);
    }
    if input.len() > 1 && input[0] == b'0' {
        return Err(NumberError::NotCanonical);
    }
    let value = parse_digits(input)?;
    if value > max {
        return Err(NumberError::Overflow);
    }
    Ok(value)
}

fn parse_digits(input: &[u8]) -> Result<u64, NumberError> {
    let mut value = 0u64;
    for &digit in input {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(digit - b'0')))
            .ok_or(NumberError::Overflow)?;
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberError {
    Empty,
    NotDigits,
    NotCanonical,
    Overflow,
    OutOfRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WcfgKind {
    Number,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WcfgFieldSpec {
    pub name: &'static str,
    pub kind: WcfgKind,
    pub min: i64,
    pub max: i64,
}

pub const WCFG_FIELD_SPECS: [WcfgFieldSpec; WCFG_FIELD_COUNT] = [
    spec("NUM_POKEMON1", 1, 6),
    spec("NUM_POKEMON2", 1, 6),
    spec("DIFICULTAD1", 0, 2),
    spec("DIFICULTAD2", 0, 2),
    spec("CAMBIOS1", 0, 66),
    spec("CAMBIOS2", 0, 66),
    spec("NUM_OBJ1", 0, 66),
    spec("NUM_OBJ2", 0, 66),
    spec("OBJ_ASG1", 0, 821),
    spec("OBJ_ASG2", 0, 821),
    spec("CONT_POKE1", 0, 1),
    spec("CONT_POKE2", 0, 1),
    spec("VEL_ATAQUE1", 0, 1),
    spec("VEL_ATAQUE2", 0, 1),
    spec("REST_PP1", 0, 4),
    spec("REST_PP2", 0, 4),
    spec("SHOW_TYPE", 0, 1),
    spec("TIME_BATTLE", 0, 11),
    spec("BTTBACK_INDEX", 1, 2_004),
    spec("MUSIC_INDEX", 1, 179),
    spec("SUBITA", 0, 1),
    spec("BATTLE_MODE", 0, 2),
    spec("DAY", 0, 1),
    spec("shownumber", 0, 1),
    spec("showbattleevents", 0, 1),
    spec("showpanelcombate", 0, 1),
    spec("shownumberPS", 0, 1),
    spec("modeBattleBack", 0, 2),
    spec("WorldClima", 0, 9),
    spec("clima", 0, 9),
    spec("Hora", 0, 23),
    spec("Minuto", 0, 59),
    text("REGLAS_J1"),
    text("REGLAS_J2"),
    text("OPTIONS_BT"),
    text("MUSIC_NAME"),
    text("txt_clima"),
];

const fn spec(name: &'static str, min: i64, max: i64) -> WcfgFieldSpec {
    WcfgFieldSpec { name, kind: WcfgKind::Number, min, max }
}

const fn text(name: &'static str) -> WcfgFieldSpec {
    WcfgFieldSpec { name, kind: WcfgKind::Text, min: 0, max: 0 }
}

pub fn wcfg_field_spec(index: usize) -> Option<&'static WcfgFieldSpec> {
    WCFG_FIELD_SPECS.get(index)
}

/// True for the three background ranges emitted by `BattleBackground`.
pub fn is_valid_battle_background(value: i64) -> bool {
    BATTLE_BACKGROUND_RANGES.iter().any(|&(min, max)| (min..=max).contains(&value))
}

#[derive(Clone, Debug, PartialEq)]
pub enum WcfgValue {
    Number(i64),
    Text(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wcfg {
    fields: [WcfgValue; WCFG_FIELD_COUNT],
    extras: Vec<Vec<u8>>,
}

impl Wcfg {
    pub fn field(&self, index: usize) -> Option<&WcfgValue> {
        self.fields.get(index)
    }

    pub fn number(&self, index: usize) -> Option<i64> {
        match self.field(index) {
            Some(WcfgValue::Number(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn text(&self, index: usize) -> Option<&[u8]> {
        match self.field(index) {
            Some(WcfgValue::Text(value)) => Some(value),
            _ => None,
        }
    }

    pub fn extras(&self) -> &[Vec<u8>] {
        &self.extras
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WcfgError {
    MissingTag,
    MissingTagSeparator,
    FrameTooLarge,
    FieldCount { actual: usize },
    EmptyField { index: usize },
    NotCanonical { index: usize },
    OutOfRange { index: usize, min: i64, max: i64 },
    InvalidBattleBackground { index: usize },
    TextControl { index: usize },
    TextTooLarge { index: usize },
}

impl fmt::Display for WcfgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for WcfgError {}

/// Parse `WCFG` + `US` + 37 `GS`-separated fields.
pub fn parse_wcfg_frame(frame: &[u8]) -> Result<Wcfg, WcfgError> {
    if frame.len() > MAX_WCFG_BYTES + 5 {
        return Err(WcfgError::FrameTooLarge);
    }
    if !frame.starts_with(b"WCFG") {
        return Err(WcfgError::MissingTag);
    }
    if frame.get(4) != Some(&UNIT_SEPARATOR) {
        return Err(WcfgError::MissingTagSeparator);
    }
    parse_wcfg_payload(&frame[5..])
}

/// Parse the `WCFG` payload (without the `WCFG` + `US` prefix).
pub fn parse_wcfg_payload(payload: &[u8]) -> Result<Wcfg, WcfgError> {
    if payload.len() > MAX_WCFG_BYTES {
        return Err(WcfgError::FrameTooLarge);
    }
    let parts: Vec<&[u8]> = payload.split(|&byte| byte == FIELD_SEPARATOR).collect();
    if parts.len() < WCFG_FIELD_COUNT || parts.len() > WCFG_MAX_FIELD_COUNT {
        return Err(WcfgError::FieldCount { actual: parts.len() });
    }

    let mut fields: [WcfgValue; WCFG_FIELD_COUNT] = std::array::from_fn(|_| WcfgValue::Number(0));
    for (index, part) in parts.iter().take(WCFG_FIELD_COUNT).enumerate() {
        let spec = &WCFG_FIELD_SPECS[index];
        match spec.kind {
            WcfgKind::Number => {
                if part.is_empty() {
                    return Err(WcfgError::EmptyField { index });
                }
                let value = parse_canonical_unsigned(part, i64::MAX as u64).map_err(|error| match error {
                    NumberError::Overflow => WcfgError::OutOfRange { index, min: spec.min, max: spec.max },
                    _ => WcfgError::NotCanonical { index },
                })? as i64;
                if !(spec.min..=spec.max).contains(&value) {
                    return Err(WcfgError::OutOfRange { index, min: spec.min, max: spec.max });
                }
                if index == 18 && !is_valid_battle_background(value) {
                    return Err(WcfgError::InvalidBattleBackground { index });
                }
                fields[index] = WcfgValue::Number(value);
            },
            WcfgKind::Text => {
                if part.len() > MAX_WCFG_TEXT_BYTES {
                    return Err(WcfgError::TextTooLarge { index });
                }
                if part.iter().any(|&byte| byte < 0x20 || byte == 0x7f) {
                    return Err(WcfgError::TextControl { index });
                }
                fields[index] = WcfgValue::Text(part.to_vec());
            },
        }
    }

    let extras = parts.iter().skip(WCFG_FIELD_COUNT).map(|part| part.to_vec()).collect();
    Ok(Wcfg { fields, extras })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamMeta {
    pub seed: i64,
    pub fondo: i64,
    pub modo: i64,
    pub sala: i64,
    pub delay: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamFrame<'a> {
    pub meta: TeamMeta,
    pub body: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeamError {
    MissingTag,
    MissingTagSeparator,
    FrameTooLarge,
    MetadataCount { actual: usize },
    EmptyField { index: usize },
    NotCanonical { index: usize },
    OutOfRange { index: usize, min: i64, max: i64 },
}

impl fmt::Display for TeamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for TeamError {}

/// Parse `TEAM` + `US` + five `RS`-terminated metadata fields + raw team body.
pub fn parse_team_frame(frame: &[u8]) -> Result<TeamFrame<'_>, TeamError> {
    if frame.len() > MAX_TEAM_BYTES + 5 {
        return Err(TeamError::FrameTooLarge);
    }
    if !frame.starts_with(b"TEAM") {
        return Err(TeamError::MissingTag);
    }
    if frame.get(4) != Some(&UNIT_SEPARATOR) {
        return Err(TeamError::MissingTagSeparator);
    }
    parse_team_payload(&frame[5..])
}

/// Parse the metadata prefix of a `TEAM` payload.
///
/// The fifth `RS` is the metadata/file boundary and must be present; the body
/// itself is opaque and is returned unmodified.
pub fn parse_team_payload(mut payload: &[u8]) -> Result<TeamFrame<'_>, TeamError> {
    if payload.len() > MAX_TEAM_BYTES {
        return Err(TeamError::FrameTooLarge);
    }
    let mut values = [0i64; TEAM_META_FIELD_COUNT];
    for (index, value) in values.iter_mut().enumerate() {
        let Some(separator) = payload.iter().position(|&byte| byte == RECORD_SEPARATOR) else {
            return Err(TeamError::MetadataCount { actual: index });
        };
        let field = &payload[..separator];
        payload = &payload[separator + 1..];
        if field.is_empty() {
            return Err(TeamError::EmptyField { index });
        }
        let parsed =
            parse_canonical_unsigned(field, i64::MAX as u64).map_err(|_| TeamError::NotCanonical { index })? as i64;
        *value = parsed;
    }

    let meta = TeamMeta { seed: values[0], fondo: values[1], modo: values[2], sala: values[3], delay: values[4] };
    validate_team_meta(meta)?;
    Ok(TeamFrame { meta, body: payload })
}

pub fn validate_team_meta(meta: TeamMeta) -> Result<(), TeamError> {
    range(meta.seed, 0, MIN_SEED, MAX_SEED)?;
    range(meta.fondo, 1, 0, i64::MAX)?;
    range(meta.modo, 2, 0, 2)?;
    range(meta.sala, 3, 0, 1_000)?;
    range(meta.delay, 4, MIN_DELAY, MAX_DELAY)?;
    if !is_valid_battle_background(meta.fondo) {
        return Err(TeamError::OutOfRange {
            index: 1,
            min: BATTLE_BACKGROUND_RANGES[0].0,
            max: BATTLE_BACKGROUND_RANGES[2].1,
        });
    }
    Ok(())
}

fn range(value: i64, index: usize, min: i64, max: i64) -> Result<(), TeamError> {
    if !(min..=max).contains(&value) {
        return Err(TeamError::OutOfRange { index, min, max });
    }
    Ok(())
}

/// `NetChecksum` over an ASCII/ANSI state string.
pub fn checksum_text(text: &[u8]) -> u32 {
    let mut checksum = 0u64;
    for &byte in text {
        checksum = (checksum * 31 + u64::from(byte)) % CHECKSUM_MODULUS as u64;
    }
    checksum as u32
}

/// Party slots walked by `NetStateText` (`for (i = 0; i <= 5; i += 1)`).
pub const NET_STATE_PARTY_SLOTS: usize = 6;

/// Snapshot of the values `NetStateText` reads from the running game.
///
/// The fields mirror the GML globals and object positions one-to-one so the
/// string can be reproduced without evaluating any GML:
///
/// - `object_count`/`object_sum` are `NET_TMP2`/`NET_TMP` from `with(all)`
///   over non-persistent instances;
/// - `pokemon1`/`pokemon2` are the rounded `Pokemon1`/`Pokemon2` positions,
///   absent when `instance_exists` is false;
/// - `actual1`/`actual2` index the active party slots;
/// - `ps1`/`ps2` are `PS_1`/`PS_2` and `pp1`/`pp2` are `PP_1`/`PP_2`.
#[derive(Clone, Debug, PartialEq)]
pub struct NetState {
    pub object_count: i64,
    pub object_sum: i64,
    pub pokemon1: Option<(f64, f64)>,
    pub pokemon2: Option<(f64, f64)>,
    pub actual1: f64,
    pub actual2: f64,
    pub ps1: [f64; NET_STATE_PARTY_SLOTS],
    pub ps2: [f64; NET_STATE_PARTY_SLOTS],
    pub pp1: [f64; NET_STATE_PARTY_SLOTS],
    pub pp2: [f64; NET_STATE_PARTY_SLOTS],
}

impl NetState {
    /// Exact `NetStateText()` string, including its spaces and trailing commas.
    pub fn text(&self) -> String {
        net_state_text(self)
    }

    /// Exact `NetChecksum()` value for this state.
    pub fn checksum(&self) -> u32 {
        net_checksum(self)
    }
}

/// GML `string()` for a real: integral values print without decimals and other
/// values use the two-decimal form of the GM8 runner.
fn gml_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

/// GML `round()`: nearest integer with ties to even (Delphi/IEEE rounding).
fn gml_round(value: f64) -> f64 {
    let floor = value.floor();
    let fraction = value - floor;
    if fraction < 0.5 {
        floor
    } else if fraction > 0.5 {
        floor + 1.0
    } else {
        floor + (floor as i64 & 1) as f64
    }
}

fn rounded_number(value: f64) -> String {
    gml_number(gml_round(value))
}

fn scaled_number(value: f64) -> String {
    rounded_number(value * 10.0)
}

fn active_pp(actual: f64, pp: &[f64; NET_STATE_PARTY_SLOTS]) -> String {
    let index = gml_round(actual);
    if !index.is_finite() || index < 0.0 {
        return scaled_number(0.0)
    }
    scaled_number(pp.get(index as usize).copied().unwrap_or(0.0))
}

/// Reproduce the 0.6.3 `NetStateText()` companion function exactly.
///
/// The GML builds `"<count> <sum>"`, appends `P1`/`P2` only when those objects
/// exist, then `A`, both six-slot `PS` arrays, and the active `PP` pair.
pub fn net_state_text(state: &NetState) -> String {
    let mut text = format!("{} {}", state.object_count, state.object_sum);
    if let Some((x, y)) = state.pokemon1 {
        text.push_str(&format!(" P1={},{}", rounded_number(x), rounded_number(y)));
    }
    if let Some((x, y)) = state.pokemon2 {
        text.push_str(&format!(" P2={},{}", rounded_number(x), rounded_number(y)));
    }
    text.push_str(&format!(" A={},{} PS=", gml_number(state.actual1), gml_number(state.actual2)));
    for value in state.ps1 {
        text.push_str(&scaled_number(value));
        text.push(',');
    }
    text.push('/');
    for value in state.ps2 {
        text.push_str(&scaled_number(value));
        text.push(',');
    }
    text.push_str(" PP=");
    text.push_str(&active_pp(state.actual1, &state.pp1));
    text.push(',');
    text.push_str(&active_pp(state.actual2, &state.pp2));
    text
}

/// Exact `NetChecksum()`: base-31 polynomial over `NetStateText()` mod 1000003.
pub fn net_checksum(state: &NetState) -> u32 {
    checksum_text(net_state_text(state).as_bytes())
}

pub fn validate_checksum(value: i64) -> Result<u32, GuardError> {
    if !(0..=MAX_CHECKSUM).contains(&value) {
        return Err(GuardError::ChecksumOutOfRange);
    }
    Ok(value as u32)
}

pub fn validate_frame(frame: i32) -> Result<u32, GuardError> {
    u32::try_from(frame).map_err(|_| GuardError::NegativeFrame)
}

pub fn validate_input_mask(mask: i32) -> Result<u32, GuardError> {
    let mask = u32::try_from(mask).map_err(|_| GuardError::MaskOutOfRange)?;
    if mask > MAX_INPUT_MASK {
        return Err(GuardError::MaskOutOfRange);
    }
    Ok(mask)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleFrameError {
    MissingTag,
    MissingTagSeparator,
    Malformed,
    NegativeFrame,
    ChecksumOutOfRange,
}

impl fmt::Display for BattleFrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for BattleFrameError {}

/// Parse `SUM` + `US` + `<frame>` + `RS`/space + `<checksum>`.
///
/// The GML producer uses `RS`; a space is accepted because some relay/legacy
/// peers normalize the second separator.
pub fn parse_sum_frame(frame: &[u8]) -> Result<(u32, u32), BattleFrameError> {
    let rest = battle_body(frame, b"SUM")?;
    let separator =
        rest.iter().position(|&byte| byte == RECORD_SEPARATOR || byte == b' ').ok_or(BattleFrameError::Malformed)?;
    let (frame, checksum) = (&rest[..separator], &rest[separator + 1..]);
    let frame = parse_battle_unsigned(frame, i32::MAX as u64).map_err(|_| BattleFrameError::NegativeFrame)?;
    let checksum = parse_battle_unsigned(checksum, MAX_CHECKSUM as u64).map_err(|error| match error {
        NumberError::Overflow => BattleFrameError::ChecksumOutOfRange,
        _ => BattleFrameError::Malformed,
    })?;
    Ok((frame as u32, checksum as u32))
}

/// Parse `BYE` + `US` + `<epoch> <frame>` (space, as emitted by `NetBattleEnd`).
pub fn parse_bye_frame(frame: &[u8]) -> Result<(i32, u32), BattleFrameError> {
    let rest = battle_body(frame, b"BYE")?;
    let separator = rest.iter().position(|&byte| byte == b' ').ok_or(BattleFrameError::Malformed)?;
    let (epoch, frame) = (&rest[..separator], &rest[separator + 1..]);
    let epoch = parse_battle_i32(epoch)?;
    let frame = parse_battle_unsigned(frame, i32::MAX as u64).map_err(|_| BattleFrameError::NegativeFrame)?;
    Ok((epoch, frame as u32))
}

fn battle_body<'a>(frame: &'a [u8], tag: &[u8]) -> Result<&'a [u8], BattleFrameError> {
    if !frame.starts_with(tag) {
        return Err(BattleFrameError::MissingTag);
    }
    if frame.get(tag.len()) != Some(&UNIT_SEPARATOR) {
        return Err(BattleFrameError::MissingTagSeparator);
    }
    Ok(&frame[tag.len() + 1..])
}

fn parse_battle_unsigned(input: &[u8], max: u64) -> Result<u64, NumberError> {
    if input.is_empty() {
        return Err(NumberError::Empty);
    }
    if !input.iter().all(u8::is_ascii_digit) {
        return Err(NumberError::NotDigits);
    }
    let value = parse_digits(input)?;
    if value > max {
        return Err(NumberError::Overflow);
    }
    Ok(value)
}

fn parse_battle_i32(input: &[u8]) -> Result<i32, BattleFrameError> {
    let (negative, digits) = match input.first() {
        Some(b'-') => (true, &input[1..]),
        _ => (false, input),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(BattleFrameError::Malformed);
    }
    let magnitude = parse_digits(digits).map_err(|_| BattleFrameError::Malformed)?;
    let magnitude = i32::try_from(magnitude).map_err(|_| BattleFrameError::Malformed)?;
    Ok(if negative { -magnitude } else { magnitude })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardError {
    NegativeFrame,
    ChecksumOutOfRange,
    MaskOutOfRange,
    LocalContradiction,
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for GuardError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desync {
    pub frame: u32,
    pub local: u32,
    pub peer: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputDecision {
    Accepted { frame: u32, mask: u32 },
    IgnoreStaleEpoch,
    IgnoreStaleFrame,
    IgnoreFutureFrame,
    IgnoreDuplicate,
    RejectConflictingReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksumDecision {
    Accepted,
    Desync(Desync),
    IgnoreStaleEpoch,
    IgnoreStaleFrame,
    IgnoreFutureFrame,
    IgnoreDuplicate,
    RejectConflictingReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByeDecision {
    Accepted { frame: u32 },
    IgnoreStaleEpoch,
    IgnoreDuplicate,
    IgnoreFutureFrame,
    RejectConflictingReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChecksumSlot {
    frame: u32,
    value: u32,
}

/// Stateful anti-replay/desync guard for one battle epoch.
pub struct BattleGuard {
    epoch: i32,
    local_frame: u32,
    local_checksums: [Option<ChecksumSlot>; CHECKSUM_SLOTS as usize],
    peer_checksums: [Option<ChecksumSlot>; CHECKSUM_SLOTS as usize],
    peer_inputs: HashMap<u32, u32>,
    bye_frame: Option<u32>,
    desync_frames: HashSet<u32>,
    desync: bool,
}

impl BattleGuard {
    pub fn new(epoch: i32) -> Self {
        Self {
            epoch,
            local_frame: 0,
            local_checksums: [None; CHECKSUM_SLOTS as usize],
            peer_checksums: [None; CHECKSUM_SLOTS as usize],
            peer_inputs: HashMap::new(),
            bye_frame: None,
            desync_frames: HashSet::new(),
            desync: false,
        }
    }

    pub fn epoch(&self) -> i32 {
        self.epoch
    }

    pub fn local_frame(&self) -> u32 {
        self.local_frame
    }

    pub fn desync(&self) -> bool {
        self.desync
    }

    pub fn bye_frame(&self) -> Option<u32> {
        self.bye_frame
    }

    pub fn should_leave(&self) -> bool {
        self.bye_frame.is_some_and(|frame| self.local_frame > frame)
    }

    /// Advance the local frame and drop inputs that can no longer be consumed.
    pub fn advance_local_frame(&mut self, frame: u32) {
        if frame > self.local_frame {
            self.local_frame = frame;
        }
        let oldest = self.local_frame.saturating_sub(INPUT_WINDOW);
        self.peer_inputs.retain(|&queued, _| queued >= oldest);
        let oldest_checksum = self.local_frame.saturating_sub(CHECKSUM_COMPARE_WINDOW);
        self.desync_frames.retain(|&frame| frame >= oldest_checksum);
    }

    pub fn receive_input(&mut self, epoch: i32, frame: i32, mask: i32) -> Result<InputDecision, GuardError> {
        if epoch != self.epoch {
            return Ok(InputDecision::IgnoreStaleEpoch);
        }
        let frame = validate_frame(frame)?;
        let mask = validate_input_mask(mask)?;
        if frame.saturating_add(INPUT_WINDOW) < self.local_frame {
            return Ok(InputDecision::IgnoreStaleFrame);
        }
        if frame > self.local_frame.saturating_add(INPUT_WINDOW) {
            return Ok(InputDecision::IgnoreFutureFrame);
        }
        match self.peer_inputs.get(&frame) {
            Some(&previous) if previous == mask => Ok(InputDecision::IgnoreDuplicate),
            Some(_) => Ok(InputDecision::RejectConflictingReplay),
            None => {
                self.peer_inputs.insert(frame, mask);
                Ok(InputDecision::Accepted { frame, mask })
            },
        }
    }

    /// Record a locally computed checksum for `frame`.
    pub fn note_local_checksum(&mut self, frame: u32, checksum: u32) -> Result<Option<Desync>, GuardError> {
        validate_checksum(i64::from(checksum))?;
        let slot = checksum_slot(frame);
        match self.local_checksums[slot] {
            Some(previous) if previous.frame == frame && previous.value != checksum => {
                return Err(GuardError::LocalContradiction);
            },
            _ => {},
        }
        self.local_checksums[slot] = Some(ChecksumSlot { frame, value: checksum });
        if let Some(peer) = self.peer_checksums[slot] {
            if peer.frame == frame && peer.value != checksum {
                return Ok(self.report_desync(frame, checksum, peer.value));
            }
        }
        Ok(None)
    }

    pub fn receive_checksum(&mut self, epoch: i32, frame: i32, checksum: i64) -> Result<ChecksumDecision, GuardError> {
        if epoch != self.epoch {
            return Ok(ChecksumDecision::IgnoreStaleEpoch);
        }
        let frame = validate_frame(frame)?;
        let checksum = validate_checksum(checksum)?;
        if frame.saturating_add(CHECKSUM_COMPARE_WINDOW) < self.local_frame {
            return Ok(ChecksumDecision::IgnoreStaleFrame);
        }
        if frame > self.local_frame.saturating_add(CHECKSUM_COMPARE_WINDOW) {
            return Ok(ChecksumDecision::IgnoreFutureFrame);
        }
        let slot = checksum_slot(frame);
        match self.peer_checksums[slot] {
            Some(previous) if previous.frame == frame && previous.value == checksum => {
                return Ok(ChecksumDecision::IgnoreDuplicate);
            },
            Some(previous) if previous.frame == frame => {
                return Ok(ChecksumDecision::RejectConflictingReplay);
            },
            Some(previous) if previous.frame > frame => {
                return Ok(ChecksumDecision::IgnoreStaleFrame);
            },
            _ => {},
        }
        self.peer_checksums[slot] = Some(ChecksumSlot { frame, value: checksum });
        if let Some(local) = self.local_checksums[slot] {
            if local.frame == frame && local.value != checksum {
                if let Some(desync) = self.report_desync(frame, local.value, checksum) {
                    return Ok(ChecksumDecision::Desync(desync));
                }
                return Ok(ChecksumDecision::IgnoreDuplicate);
            }
        }
        Ok(ChecksumDecision::Accepted)
    }

    pub fn receive_bye(&mut self, epoch: i32, frame: i32) -> Result<ByeDecision, GuardError> {
        if epoch != self.epoch {
            return Ok(ByeDecision::IgnoreStaleEpoch);
        }
        let frame = validate_frame(frame)?;
        if frame > self.local_frame.saturating_add(BYE_WINDOW) {
            return Ok(ByeDecision::IgnoreFutureFrame);
        }
        match self.bye_frame {
            Some(previous) if previous == frame => Ok(ByeDecision::IgnoreDuplicate),
            Some(_) => Ok(ByeDecision::RejectConflictingReplay),
            None => {
                self.bye_frame = Some(frame);
                Ok(ByeDecision::Accepted { frame })
            },
        }
    }

    fn report_desync(&mut self, frame: u32, local: u32, peer: u32) -> Option<Desync> {
        if !self.desync_frames.insert(frame) {
            return None;
        }
        self.desync = true;
        Some(Desync { frame, local, peer })
    }
}

fn checksum_slot(frame: u32) -> usize {
    ((frame / CHECKSUM_INTERVAL) % CHECKSUM_SLOTS) as usize
}

#[cfg(test)]
#[path = "z063_sec_tests.rs"]
mod tests;
