//! Clean-room WCFG battle-configuration codec and post-TEAM exchange.

use std::fmt;
use std::time::Duration;

pub const FIELD_COUNT: usize = 37;
pub const NUMERIC_FIELD_COUNT: usize = 32;
pub const TEXT_FIELD_COUNT: usize = 5;
pub const FIELD_SEPARATOR: u8 = 0x1d;
pub const FRAME_TAG: &[u8] = b"WCFG";
pub const FRAME_SEPARATOR: u8 = 0x1f;
pub const MAX_PAYLOAD_BYTES: usize = 65_536;
pub const MAX_FRAME_BYTES: usize = MAX_PAYLOAD_BYTES + FRAME_TAG.len() + 1;
pub const MAX_TEXT_BYTES: usize = 4_096;
pub const MAX_NUMBER_BYTES: usize = 64;
pub const MAX_IGNORED_FRAMES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Number,
    Text,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldId {
    NumPokemon1 = 0,
    NumPokemon2 = 1,
    Difficulty1 = 2,
    Difficulty2 = 3,
    Changes1 = 4,
    Changes2 = 5,
    NumItems1 = 6,
    NumItems2 = 7,
    ItemAssignment1 = 8,
    ItemAssignment2 = 9,
    PokeControl1 = 10,
    PokeControl2 = 11,
    AttackSpeed1 = 12,
    AttackSpeed2 = 13,
    PpRestore1 = 14,
    PpRestore2 = 15,
    ShowType = 16,
    TimeBattle = 17,
    BattleBackIndex = 18,
    MusicIndex = 19,
    SuddenDeath = 20,
    BattleMode = 21,
    Day = 22,
    ShowNumber = 23,
    ShowBattleEvents = 24,
    ShowBattlePanel = 25,
    ShowNumberPs = 26,
    ModeBattleBack = 27,
    WorldWeather = 28,
    Climate = 29,
    Hour = 30,
    Minute = 31,
    Rules1 = 32,
    Rules2 = 33,
    OptionsBt = 34,
    MusicName = 35,
    ClimateText = 36,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    pub id: FieldId,
    pub name: &'static str,
    pub kind: FieldKind,
}

pub const FIELD_SPECS: [FieldSpec; FIELD_COUNT] = [
    FieldSpec { id: FieldId::NumPokemon1, name: "NUM_POKEMON1", kind: FieldKind::Number },
    FieldSpec { id: FieldId::NumPokemon2, name: "NUM_POKEMON2", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Difficulty1, name: "DIFICULTAD1", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Difficulty2, name: "DIFICULTAD2", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Changes1, name: "CAMBIOS1", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Changes2, name: "CAMBIOS2", kind: FieldKind::Number },
    FieldSpec { id: FieldId::NumItems1, name: "NUM_OBJ1", kind: FieldKind::Number },
    FieldSpec { id: FieldId::NumItems2, name: "NUM_OBJ2", kind: FieldKind::Number },
    FieldSpec { id: FieldId::ItemAssignment1, name: "OBJ_ASG1", kind: FieldKind::Number },
    FieldSpec { id: FieldId::ItemAssignment2, name: "OBJ_ASG2", kind: FieldKind::Number },
    FieldSpec { id: FieldId::PokeControl1, name: "CONT_POKE1", kind: FieldKind::Number },
    FieldSpec { id: FieldId::PokeControl2, name: "CONT_POKE2", kind: FieldKind::Number },
    FieldSpec { id: FieldId::AttackSpeed1, name: "VEL_ATAQUE1", kind: FieldKind::Number },
    FieldSpec { id: FieldId::AttackSpeed2, name: "VEL_ATAQUE2", kind: FieldKind::Number },
    FieldSpec { id: FieldId::PpRestore1, name: "REST_PP1", kind: FieldKind::Number },
    FieldSpec { id: FieldId::PpRestore2, name: "REST_PP2", kind: FieldKind::Number },
    FieldSpec { id: FieldId::ShowType, name: "SHOW_TYPE", kind: FieldKind::Number },
    FieldSpec { id: FieldId::TimeBattle, name: "TIME_BATTLE", kind: FieldKind::Number },
    FieldSpec { id: FieldId::BattleBackIndex, name: "BTTBACK_INDEX", kind: FieldKind::Number },
    FieldSpec { id: FieldId::MusicIndex, name: "MUSIC_INDEX", kind: FieldKind::Number },
    FieldSpec { id: FieldId::SuddenDeath, name: "SUBITA", kind: FieldKind::Number },
    FieldSpec { id: FieldId::BattleMode, name: "BATTLE_MODE", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Day, name: "DAY", kind: FieldKind::Number },
    FieldSpec { id: FieldId::ShowNumber, name: "shownumber", kind: FieldKind::Number },
    FieldSpec { id: FieldId::ShowBattleEvents, name: "showbattleevents", kind: FieldKind::Number },
    FieldSpec { id: FieldId::ShowBattlePanel, name: "showpanelcombate", kind: FieldKind::Number },
    FieldSpec { id: FieldId::ShowNumberPs, name: "shownumberPS", kind: FieldKind::Number },
    FieldSpec { id: FieldId::ModeBattleBack, name: "modeBattleBack", kind: FieldKind::Number },
    FieldSpec { id: FieldId::WorldWeather, name: "WorldClima", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Climate, name: "clima", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Hour, name: "Hora", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Minute, name: "Minuto", kind: FieldKind::Number },
    FieldSpec { id: FieldId::Rules1, name: "REGLAS_J1", kind: FieldKind::Text },
    FieldSpec { id: FieldId::Rules2, name: "REGLAS_J2", kind: FieldKind::Text },
    FieldSpec { id: FieldId::OptionsBt, name: "OPTIONS_BT", kind: FieldKind::Text },
    FieldSpec { id: FieldId::MusicName, name: "MUSIC_NAME", kind: FieldKind::Text },
    FieldSpec { id: FieldId::ClimateText, name: "txt_clima", kind: FieldKind::Text },
];

impl FieldId {
    pub const ALL: [FieldId; FIELD_COUNT] = [
        FieldId::NumPokemon1,
        FieldId::NumPokemon2,
        FieldId::Difficulty1,
        FieldId::Difficulty2,
        FieldId::Changes1,
        FieldId::Changes2,
        FieldId::NumItems1,
        FieldId::NumItems2,
        FieldId::ItemAssignment1,
        FieldId::ItemAssignment2,
        FieldId::PokeControl1,
        FieldId::PokeControl2,
        FieldId::AttackSpeed1,
        FieldId::AttackSpeed2,
        FieldId::PpRestore1,
        FieldId::PpRestore2,
        FieldId::ShowType,
        FieldId::TimeBattle,
        FieldId::BattleBackIndex,
        FieldId::MusicIndex,
        FieldId::SuddenDeath,
        FieldId::BattleMode,
        FieldId::Day,
        FieldId::ShowNumber,
        FieldId::ShowBattleEvents,
        FieldId::ShowBattlePanel,
        FieldId::ShowNumberPs,
        FieldId::ModeBattleBack,
        FieldId::WorldWeather,
        FieldId::Climate,
        FieldId::Hour,
        FieldId::Minute,
        FieldId::Rules1,
        FieldId::Rules2,
        FieldId::OptionsBt,
        FieldId::MusicName,
        FieldId::ClimateText,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub const fn spec(self) -> FieldSpec {
        FIELD_SPECS[self.index()]
    }

    pub const fn name(self) -> &'static str {
        self.spec().name
    }

    pub const fn kind(self) -> FieldKind {
        self.spec().kind
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(Vec<u8>),
}

impl Value {
    pub fn number(value: f64) -> Self {
        Self::Number(value)
    }

    pub fn text(value: impl Into<Vec<u8>>) -> Self {
        Self::Text(value.into())
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&[u8]> {
        match self {
            Self::Number(_) => None,
            Self::Text(value) => Some(value.as_slice()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    FieldKind { index: usize, expected: FieldKind },
    NonFinite { index: usize },
    TextControl { index: usize },
    TextLimit { index: usize },
    NumberLimit { index: usize },
    FrameLimit,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldKind { index, expected } => {
                write!(f, "field {index} must be {expected:?}")
            }
            Self::NonFinite { index } => write!(f, "field {index} is not finite"),
            Self::TextControl { index } => write!(f, "field {index} contains a control byte"),
            Self::TextLimit { index } => write!(f, "field {index} is too long"),
            Self::NumberLimit { index } => write!(f, "field {index} number is too long"),
            Self::FrameLimit => write!(f, "frame is too long"),
        }
    }
}

impl std::error::Error for EncodeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    FrameLimit,
    MissingTag,
    MissingTagSeparator,
    FieldCount { actual: usize },
    Number { index: usize },
    NonFinite { index: usize },
    NumberLimit { index: usize },
    TextControl { index: usize },
    TextLimit { index: usize },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameLimit => write!(f, "frame is too long"),
            Self::MissingTag => write!(f, "frame does not start with WCFG"),
            Self::MissingTagSeparator => write!(f, "frame has no tag separator"),
            Self::FieldCount { actual } => {
                write!(f, "expected {FIELD_COUNT} fields, got {actual}")
            }
            Self::Number { index } => write!(f, "field {index} is not a decimal number"),
            Self::NonFinite { index } => write!(f, "field {index} is not finite"),
            Self::NumberLimit { index } => write!(f, "field {index} number is too long"),
            Self::TextControl { index } => write!(f, "field {index} contains a control byte"),
            Self::TextLimit { index } => write!(f, "field {index} is too long"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[derive(Clone, Debug, PartialEq)]
pub struct Wcfg {
    fields: [Value; FIELD_COUNT],
}

impl Wcfg {
    pub fn from_fields(fields: [Value; FIELD_COUNT]) -> Result<Self, EncodeError> {
        let config = Self { fields };
        config.validate()?;
        Ok(config)
    }

    pub fn fields(&self) -> &[Value; FIELD_COUNT] {
        &self.fields
    }

    pub fn field(&self, id: FieldId) -> &Value {
        &self.fields[id.index()]
    }

    pub fn number(&self, id: FieldId) -> Option<f64> {
        self.field(id).as_number()
    }

    pub fn text(&self, id: FieldId) -> Option<&[u8]> {
        self.field(id).as_text()
    }

    pub fn set_number(&mut self, id: FieldId, value: f64) -> Result<(), EncodeError> {
        if id.kind() != FieldKind::Number {
            return Err(EncodeError::FieldKind {
                index: id.index(),
                expected: FieldKind::Number,
            });
        }
        validate_number(value, id.index())?;
        self.fields[id.index()] = Value::Number(if value == 0.0 { 0.0 } else { value });
        Ok(())
    }

    pub fn set_text(&mut self, id: FieldId, value: impl Into<Vec<u8>>) -> Result<(), EncodeError> {
        if id.kind() != FieldKind::Text {
            return Err(EncodeError::FieldKind {
                index: id.index(),
                expected: FieldKind::Text,
            });
        }
        let value = value.into();
        validate_text(&value, id.index())?;
        self.fields[id.index()] = Value::Text(value);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), EncodeError> {
        for (index, field) in self.fields.iter().enumerate() {
            match (FIELD_SPECS[index].kind, field) {
                (FieldKind::Number, Value::Number(value)) => {
                    validate_number(*value, index)?;
                }
                (FieldKind::Text, Value::Text(value)) => {
                    validate_text(value, index)?;
                }
                (expected, _) => {
                    return Err(EncodeError::FieldKind { index, expected });
                }
            }
        }
        Ok(())
    }

    pub fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        self.validate()?;
        let mut output = Vec::with_capacity(256);
        for (index, field) in self.fields.iter().enumerate() {
            if index > 0 {
                output.push(FIELD_SEPARATOR);
            }
            match field {
                Value::Number(value) => {
                    let text = encode_number(*value);
                    output.extend_from_slice(text.as_bytes());
                }
                Value::Text(value) => output.extend_from_slice(value),
            }
            if output.len() > MAX_PAYLOAD_BYTES {
                return Err(EncodeError::FrameLimit);
            }
        }
        Ok(output)
    }

    pub fn encode_frame(&self) -> Result<Vec<u8>, EncodeError> {
        let payload = self.encode_payload()?;
        let mut frame = Vec::with_capacity(FRAME_TAG.len() + 1 + payload.len());
        frame.extend_from_slice(FRAME_TAG);
        frame.push(FRAME_SEPARATOR);
        frame.extend_from_slice(&payload);
        if frame.len() > MAX_FRAME_BYTES {
            return Err(EncodeError::FrameLimit);
        }
        Ok(frame)
    }

    pub fn decode_payload(payload: &[u8]) -> Result<Self, DecodeError> {
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(DecodeError::FrameLimit);
        }
        let parts: Vec<&[u8]> = payload.split(|byte| *byte == FIELD_SEPARATOR).collect();
        if parts.len() != FIELD_COUNT {
            return Err(DecodeError::FieldCount { actual: parts.len() });
        }
        let mut fields = Vec::with_capacity(FIELD_COUNT);
        for (index, part) in parts.iter().enumerate() {
            match FIELD_SPECS[index].kind {
                FieldKind::Number => {
                    fields.push(Value::Number(parse_number(part, index)?));
                }
                FieldKind::Text => {
                    if part.len() > MAX_TEXT_BYTES {
                        return Err(DecodeError::TextLimit { index });
                    }
                    if !text_is_valid(part) {
                        return Err(DecodeError::TextControl { index });
                    }
                    fields.push(Value::Text(part.to_vec()));
                }
            }
        }
        let fields: [Value; FIELD_COUNT] = fields
            .try_into()
            .map_err(|_| DecodeError::FieldCount { actual: FIELD_COUNT })?;
        Ok(Self { fields })
    }

    pub fn decode_frame(frame: &[u8]) -> Result<Self, DecodeError> {
        if frame.len() > MAX_FRAME_BYTES {
            return Err(DecodeError::FrameLimit);
        }
        if !frame.starts_with(FRAME_TAG) {
            return Err(DecodeError::MissingTag);
        }
        let remainder = &frame[FRAME_TAG.len()..];
        if remainder.first() != Some(&FRAME_SEPARATOR) {
            return Err(DecodeError::MissingTagSeparator);
        }
        Self::decode_payload(&remainder[1..])
    }
}

impl Default for Wcfg {
    fn default() -> Self {
        let mut config = Self {
            fields: std::array::from_fn(|index| match FIELD_SPECS[index].kind {
                FieldKind::Number => Value::Number(0.0),
                FieldKind::Text => Value::Text(Vec::new()),
            }),
        };
        let numbers = [
            (FieldId::NumPokemon1, 6.0),
            (FieldId::NumPokemon2, 6.0),
            (FieldId::Difficulty1, 1.0),
            (FieldId::Difficulty2, 1.0),
            (FieldId::Changes1, 18.0),
            (FieldId::Changes2, 18.0),
            (FieldId::NumItems1, 6.0),
            (FieldId::NumItems2, 6.0),
            (FieldId::ItemAssignment1, 15.0),
            (FieldId::ItemAssignment2, 15.0),
            (FieldId::PokeControl1, 1.0),
            (FieldId::PokeControl2, 1.0),
            (FieldId::ShowType, 1.0),
            (FieldId::MusicIndex, 161.0),
            (FieldId::Day, 1.0),
            (FieldId::ShowNumber, 1.0),
            (FieldId::ShowBattleEvents, 1.0),
            (FieldId::ShowBattlePanel, 1.0),
            (FieldId::Hour, 8.0),
            (FieldId::Minute, 30.0),
        ];
        for (id, value) in numbers {
            config.fields[id.index()] = Value::Number(value);
        }
        config
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    AwaitingTeam,
    WaitingConfig,
    Sent,
    Applied,
}

impl Phase {
    pub fn is_ready_for_battle(self) -> bool {
        matches!(self, Self::Sent | Self::Applied)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExchangeOutcome {
    Sent(Vec<u8>),
    Applied(Wcfg),
}

pub trait ExchangeTransport {
    type Error;

    fn send_frame(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
    fn recv_frame(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, Self::Error>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExchangeError<E> {
    Encode(EncodeError),
    Decode(DecodeError),
    Transport(E),
    Timeout,
    Phase(Phase),
    UnexpectedFrame,
}

impl<E: fmt::Display> fmt::Display for ExchangeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(error) => write!(f, "WCFG encode failed: {error}"),
            Self::Decode(error) => write!(f, "WCFG decode failed: {error}"),
            Self::Transport(error) => write!(f, "WCFG transport failed: {error}"),
            Self::Timeout => write!(f, "timed out waiting for WCFG"),
            Self::Phase(phase) => write!(f, "WCFG exchange is in phase {phase:?}"),
            Self::UnexpectedFrame => write!(f, "too many unrelated peer frames before WCFG"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ExchangeError<E> {}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigExchange {
    side: Side,
    phase: Phase,
    local: Wcfg,
    remote: Option<Wcfg>,
}

impl ConfigExchange {
    pub fn new(side: Side, local: Wcfg) -> Self {
        Self {
            side,
            phase: Phase::AwaitingTeam,
            local,
            remote: None,
        }
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn local(&self) -> &Wcfg {
        &self.local
    }

    pub fn remote(&self) -> Option<&Wcfg> {
        self.remote.as_ref()
    }

    pub fn ready_for_battle(&self) -> bool {
        self.phase.is_ready_for_battle()
    }

    pub fn after_team<T: ExchangeTransport>(
        &mut self,
        transport: &mut T,
        timeout: Duration,
    ) -> Result<ExchangeOutcome, ExchangeError<T::Error>> {
        if self.phase != Phase::AwaitingTeam {
            return Err(ExchangeError::Phase(self.phase));
        }
        match self.side {
            Side::One => {
                let frame = self.local.encode_frame().map_err(ExchangeError::Encode)?;
                transport
                    .send_frame(&frame)
                    .map_err(ExchangeError::Transport)?;
                self.phase = Phase::Sent;
                Ok(ExchangeOutcome::Sent(frame))
            }
            Side::Two => {
                self.phase = Phase::WaitingConfig;
                let mut ignored = 0;
                loop {
                    match transport.recv_frame(timeout) {
                        Ok(Some(frame)) => {
                            if !frame.starts_with(FRAME_TAG) {
                                ignored += 1;
                                if ignored > MAX_IGNORED_FRAMES {
                                    self.phase = Phase::AwaitingTeam;
                                    return Err(ExchangeError::UnexpectedFrame);
                                }
                                continue;
                            }
                            match Wcfg::decode_frame(&frame) {
                                Ok(config) => {
                                    self.remote = Some(config.clone());
                                    self.phase = Phase::Applied;
                                    return Ok(ExchangeOutcome::Applied(config));
                                }
                                Err(error) => {
                                    self.phase = Phase::AwaitingTeam;
                                    return Err(ExchangeError::Decode(error));
                                }
                            }
                        }
                        Ok(None) => {
                            self.phase = Phase::AwaitingTeam;
                            return Err(ExchangeError::Timeout);
                        }
                        Err(error) => {
                            self.phase = Phase::AwaitingTeam;
                            return Err(ExchangeError::Transport(error));
                        }
                    }
                }
            }
        }
    }
}

fn validate_number(value: f64, index: usize) -> Result<(), EncodeError> {
    if !value.is_finite() {
        return Err(EncodeError::NonFinite { index });
    }
    if encode_number(value).len() > MAX_NUMBER_BYTES {
        return Err(EncodeError::NumberLimit { index });
    }
    Ok(())
}

fn validate_text(value: &[u8], index: usize) -> Result<(), EncodeError> {
    if value.len() > MAX_TEXT_BYTES {
        return Err(EncodeError::TextLimit { index });
    }
    if !text_is_valid(value) {
        return Err(EncodeError::TextControl { index });
    }
    Ok(())
}

fn text_is_valid(value: &[u8]) -> bool {
    value.iter().all(|byte| *byte >= 0x20 && *byte != 0x7f)
}

fn encode_number(value: f64) -> String {
    if value == 0.0 {
        "0".to_string()
    } else {
        value.to_string()
    }
}

fn parse_number(input: &[u8], index: usize) -> Result<f64, DecodeError> {
    let value = trim_ascii_whitespace(input);
    if value.is_empty() {
        return Err(DecodeError::Number { index });
    }
    if value.len() > MAX_NUMBER_BYTES {
        return Err(DecodeError::NumberLimit { index });
    }
    if !decimal_syntax(value) {
        return Err(DecodeError::Number { index });
    }
    let text = std::str::from_utf8(value).map_err(|_| DecodeError::Number { index })?;
    let parsed = text
        .parse::<f64>()
        .map_err(|_| DecodeError::Number { index })?;
    if !parsed.is_finite() {
        return Err(DecodeError::NonFinite { index });
    }
    if encode_number(parsed).len() > MAX_NUMBER_BYTES {
        return Err(DecodeError::NumberLimit { index });
    }
    Ok(if parsed == 0.0 { 0.0 } else { parsed })
}

fn trim_ascii_whitespace(mut input: &[u8]) -> &[u8] {
    while input.first().is_some_and(u8::is_ascii_whitespace) {
        input = &input[1..];
    }
    while input.last().is_some_and(u8::is_ascii_whitespace) {
        input = &input[..input.len() - 1];
    }
    input
}

fn decimal_syntax(input: &[u8]) -> bool {
    let mut cursor = 0;
    if input
        .first()
        .is_some_and(|byte| *byte == b'+' || *byte == b'-')
    {
        cursor += 1;
    }
    let mut digits = 0;
    while input
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        cursor += 1;
        digits += 1;
    }
    if input.get(cursor) == Some(&b'.') {
        cursor += 1;
        while input
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            cursor += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if input
        .get(cursor)
        .is_some_and(|byte| *byte == b'e' || *byte == b'E')
    {
        cursor += 1;
        if input
            .get(cursor)
            .is_some_and(|byte| *byte == b'+' || *byte == b'-')
        {
            cursor += 1;
        }
        let mut exponent = 0;
        while input
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            cursor += 1;
            exponent += 1;
        }
        if exponent == 0 {
            return false;
        }
    }
    cursor == input.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct MockTransport {
        inbox: VecDeque<Vec<u8>>,
        sent: Vec<Vec<u8>>,
        recv_calls: usize,
        send_fails: bool,
    }

    impl ExchangeTransport for MockTransport {
        type Error = &'static str;

        fn send_frame(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
            if self.send_fails {
                return Err("send failed");
            }
            self.sent.push(frame.to_vec());
            Ok(())
        }

        fn recv_frame(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>, Self::Error> {
            self.recv_calls += 1;
            Ok(self.inbox.pop_front())
        }
    }

    fn sample() -> Wcfg {
        let mut config = Wcfg::default();
        config.set_number(FieldId::NumPokemon1, 4.0).unwrap();
        config.set_number(FieldId::NumPokemon2, 3.0).unwrap();
        config.set_number(FieldId::Difficulty1, 2.0).unwrap();
        config.set_number(FieldId::Changes1, 12.0).unwrap();
        config.set_number(FieldId::MusicIndex, 7.0).unwrap();
        config.set_number(FieldId::SuddenDeath, 1.0).unwrap();
        config.set_number(FieldId::Hour, 23.0).unwrap();
        config.set_number(FieldId::Minute, 59.0).unwrap();
        config.set_text(FieldId::Rules1, b"6#No items#18".to_vec()).unwrap();
        config.set_text(FieldId::Rules2, b"6#Sleep clause#12".to_vec()).unwrap();
        config.set_text(FieldId::OptionsBt, b"1#1#0#1#1#0#1".to_vec()).unwrap();
        config.set_text(FieldId::MusicName, b"Battle Theme".to_vec()).unwrap();
        config.set_text(FieldId::ClimateText, b"Sunny".to_vec()).unwrap();
        config
    }

    fn replace_field(payload: &[u8], index: usize, value: &[u8]) -> Vec<u8> {
        let mut parts: Vec<&[u8]> = payload.split(|byte| *byte == FIELD_SEPARATOR).collect();
        assert_eq!(parts.len(), FIELD_COUNT);
        parts[index] = value;
        join_fields(&parts)
    }

    fn join_fields(parts: &[&[u8]]) -> Vec<u8> {
        let mut output = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                output.push(FIELD_SEPARATOR);
            }
            output.extend_from_slice(part);
        }
        output
    }

    fn gml_names() -> [&'static str; FIELD_COUNT] {
        [
            "NUM_POKEMON1",
            "NUM_POKEMON2",
            "DIFICULTAD1",
            "DIFICULTAD2",
            "CAMBIOS1",
            "CAMBIOS2",
            "NUM_OBJ1",
            "NUM_OBJ2",
            "OBJ_ASG1",
            "OBJ_ASG2",
            "CONT_POKE1",
            "CONT_POKE2",
            "VEL_ATAQUE1",
            "VEL_ATAQUE2",
            "REST_PP1",
            "REST_PP2",
            "SHOW_TYPE",
            "TIME_BATTLE",
            "BTTBACK_INDEX",
            "MUSIC_INDEX",
            "SUBITA",
            "BATTLE_MODE",
            "DAY",
            "shownumber",
            "showbattleevents",
            "showpanelcombate",
            "shownumberPS",
            "modeBattleBack",
            "WorldClima",
            "clima",
            "Hora",
            "Minuto",
            "REGLAS_J1",
            "REGLAS_J2",
            "OPTIONS_BT",
            "MUSIC_NAME",
            "txt_clima",
        ]
    }

    #[test]
    fn field_table_matches_the_gml_order() {
        assert_eq!(FIELD_COUNT, 37);
        assert_eq!(FieldId::ALL.len(), FIELD_COUNT);
        assert_eq!(FIELD_SPECS.len(), FIELD_COUNT);
        let names = gml_names();
        for (index, id) in FieldId::ALL.iter().enumerate() {
            assert_eq!(id.index(), index);
            assert_eq!(id.name(), names[index]);
            assert_eq!(id.spec().id, *id);
            assert_eq!(FieldId::from_index(index), Some(*id));
        }
        assert_eq!(FieldId::from_index(FIELD_COUNT), None);
        assert!(FieldId::ALL[..NUMERIC_FIELD_COUNT]
            .iter()
            .all(|id| id.kind() == FieldKind::Number));
        assert!(FieldId::ALL[NUMERIC_FIELD_COUNT..]
            .iter()
            .all(|id| id.kind() == FieldKind::Text));
        assert_eq!(
            FieldId::ALL[NUMERIC_FIELD_COUNT..].len(),
            TEXT_FIELD_COUNT
        );
    }

    #[test]
    fn default_config_round_trips() {
        let config = Wcfg::default();
        let payload = config.encode_payload().unwrap();
        assert_eq!(Wcfg::decode_payload(&payload).unwrap(), config);
        assert_eq!(config.number(FieldId::NumPokemon1), Some(6.0));
        assert_eq!(config.number(FieldId::Hour), Some(8.0));
        assert_eq!(config.number(FieldId::Minute), Some(30.0));
        assert_eq!(config.text(FieldId::Rules1), Some(&b""[..]));
    }

    #[test]
    fn wire_layout_follows_the_field_table() {
        let mut config = Wcfg::default();
        for (index, id) in FieldId::ALL[..NUMERIC_FIELD_COUNT].iter().enumerate() {
            config.set_number(*id, index as f64 + 0.5).unwrap();
        }
        for (offset, id) in FieldId::ALL[NUMERIC_FIELD_COUNT..].iter().enumerate() {
            config
                .set_text(*id, format!("text-{offset}").into_bytes())
                .unwrap();
        }
        let payload = config.encode_payload().unwrap();
        let parts: Vec<&[u8]> = payload.split(|byte| *byte == FIELD_SEPARATOR).collect();
        assert_eq!(parts.len(), FIELD_COUNT);
        for index in 0..NUMERIC_FIELD_COUNT {
            assert_eq!(parts[index], format!("{}.5", index).as_bytes());
        }
        for offset in 0..TEXT_FIELD_COUNT {
            assert_eq!(
                parts[NUMERIC_FIELD_COUNT + offset],
                format!("text-{offset}").as_bytes()
            );
        }
    }

    #[test]
    fn numbers_encode_canonically() {
        let mut config = Wcfg::default();
        config.set_number(FieldId::NumPokemon1, -0.0).unwrap();
        config.set_number(FieldId::NumPokemon2, 1.0).unwrap();
        config.set_number(FieldId::Difficulty1, 1.5).unwrap();
        config.set_number(FieldId::Difficulty2, -2.25).unwrap();
        let payload = config.encode_payload().unwrap();
        let parts: Vec<&[u8]> = payload.split(|byte| *byte == FIELD_SEPARATOR).collect();
        assert_eq!(parts[0], b"0");
        assert_eq!(parts[1], b"1");
        assert_eq!(parts[2], b"1.5");
        assert_eq!(parts[3], b"-2.25");
    }

    #[test]
    fn frame_carries_the_wcfg_tag() {
        let config = sample();
        let frame = config.encode_frame().unwrap();
        assert!(frame.starts_with(b"WCFG\x1f"));
        assert_eq!(&frame[..FRAME_TAG.len()], FRAME_TAG);
        assert_eq!(frame[FRAME_TAG.len()], FRAME_SEPARATOR);
        assert_eq!(Wcfg::decode_frame(&frame).unwrap(), config);
        assert_eq!(
            Wcfg::decode_payload(&frame[FRAME_TAG.len() + 1..]).unwrap(),
            config
        );
    }

    #[test]
    fn frame_requires_the_tag_and_separator() {
        let config = sample();
        let payload = config.encode_payload().unwrap();
        assert_eq!(
            Wcfg::decode_frame(&payload).unwrap_err(),
            DecodeError::MissingTag
        );
        assert_eq!(
            Wcfg::decode_frame(b"WCFG").unwrap_err(),
            DecodeError::MissingTagSeparator
        );
    }

    #[test]
    fn decode_requires_exactly_field_count_fields() {
        let payload = sample().encode_payload().unwrap();
        let parts: Vec<&[u8]> = payload.split(|byte| *byte == FIELD_SEPARATOR).collect();
        let short = join_fields(&parts[..FIELD_COUNT - 1]);
        assert_eq!(
            Wcfg::decode_payload(&short).unwrap_err(),
            DecodeError::FieldCount {
                actual: FIELD_COUNT - 1
            }
        );
        let mut long = payload.clone();
        long.push(FIELD_SEPARATOR);
        long.extend_from_slice(b"extra");
        assert_eq!(
            Wcfg::decode_payload(&long).unwrap_err(),
            DecodeError::FieldCount {
                actual: FIELD_COUNT + 1
            }
        );
        assert_eq!(
            Wcfg::decode_payload(b"").unwrap_err(),
            DecodeError::FieldCount { actual: 1 }
        );
    }

    #[test]
    fn decode_requires_decimal_numbers() {
        let payload = sample().encode_payload().unwrap();
        let bad_cases: [&[u8]; 7] = [b"abc", b"", b"1.2.3", b"1,5", b"0x10", b"NaN", b"inf"];
        for bad in bad_cases {
            let wire = replace_field(&payload, 5, bad);
            assert_eq!(
                Wcfg::decode_payload(&wire).unwrap_err(),
                DecodeError::Number { index: 5 }
            );
        }
    }

    #[test]
    fn decode_accepts_gml_number_forms() {
        let payload = sample().encode_payload().unwrap();
        let cases: [(&[u8], f64); 6] = [
            (b" +2.500 ", 2.5),
            (b"1e3", 1000.0),
            (b".5", 0.5),
            (b"5.", 5.0),
            (b"-7", -7.0),
            (b"0004", 4.0),
        ];
        for (wire, expected) in cases {
            let payload = replace_field(&payload, 0, wire);
            let config = Wcfg::decode_payload(&payload).unwrap();
            assert_eq!(config.number(FieldId::NumPokemon1), Some(expected));
        }
    }

    #[test]
    fn decode_rejects_non_finite_and_overflowing_numbers() {
        let payload = sample().encode_payload().unwrap();
        let wire = replace_field(&payload, 2, b"1e999");
        assert_eq!(
            Wcfg::decode_payload(&wire).unwrap_err(),
            DecodeError::NonFinite { index: 2 }
        );
        let too_long = vec![b'1'; MAX_NUMBER_BYTES + 1];
        let wire = replace_field(&payload, 2, &too_long);
        assert_eq!(
            Wcfg::decode_payload(&wire).unwrap_err(),
            DecodeError::NumberLimit { index: 2 }
        );
        let wire = replace_field(&payload, 2, b"1e-300");
        assert_eq!(
            Wcfg::decode_payload(&wire).unwrap_err(),
            DecodeError::NumberLimit { index: 2 }
        );
    }

    #[test]
    fn decode_rejects_control_bytes_in_text() {
        let payload = sample().encode_payload().unwrap();
        let bad_cases: [&[u8]; 4] = [b"a\nb", b"a\rb", b"a\0b", b"a\x7fb"];
        for bad in bad_cases {
            let wire = replace_field(&payload, FieldId::Rules1.index(), bad);
            assert_eq!(
                Wcfg::decode_payload(&wire).unwrap_err(),
                DecodeError::TextControl { index: 32 }
            );
        }
        let wire = replace_field(&payload, FieldId::Rules1.index(), b"a\x1db");
        assert_eq!(
            Wcfg::decode_payload(&wire).unwrap_err(),
            DecodeError::FieldCount { actual: FIELD_COUNT + 1 }
        );
    }

    #[test]
    fn decode_preserves_ansi_and_utf8_text() {
        let payload = sample().encode_payload().unwrap();
        let raw = [0x80, 0xff, 0x41, 0xc3, 0xa9];
        let wire = replace_field(&payload, FieldId::MusicName.index(), &raw);
        let config = Wcfg::decode_payload(&wire).unwrap();
        assert_eq!(config.text(FieldId::MusicName), Some(&raw[..]));
    }

    #[test]
    fn decode_rejects_oversized_text_and_payload() {
        let payload = sample().encode_payload().unwrap();
        let text = vec![b'a'; MAX_TEXT_BYTES + 1];
        let wire = replace_field(&payload, FieldId::ClimateText.index(), &text);
        assert_eq!(
            Wcfg::decode_payload(&wire).unwrap_err(),
            DecodeError::TextLimit { index: 36 }
        );
        let huge = vec![b'a'; MAX_PAYLOAD_BYTES + 1];
        assert_eq!(
            Wcfg::decode_payload(&huge).unwrap_err(),
            DecodeError::FrameLimit
        );
    }

    #[test]
    fn encode_rejects_non_finite_wrong_kind_and_bad_text() {
        let mut config = Wcfg::default();
        assert_eq!(
            config.set_number(FieldId::NumPokemon1, f64::NAN).unwrap_err(),
            EncodeError::NonFinite { index: 0 }
        );
        assert_eq!(
            config
                .set_number(FieldId::Rules1, 1.0)
                .unwrap_err(),
            EncodeError::FieldKind {
                index: 32,
                expected: FieldKind::Number
            }
        );
        assert_eq!(
            config
                .set_text(FieldId::NumPokemon1, b"6".to_vec())
                .unwrap_err(),
            EncodeError::FieldKind {
                index: 0,
                expected: FieldKind::Text
            }
        );
        assert_eq!(
            config
                .set_text(FieldId::Rules1, b"a\x1db".to_vec())
                .unwrap_err(),
            EncodeError::TextControl { index: 32 }
        );
        let oversized = vec![b'a'; MAX_TEXT_BYTES + 1];
        assert_eq!(
            config.set_text(FieldId::Rules1, oversized).unwrap_err(),
            EncodeError::TextLimit { index: 32 }
        );
    }

    #[test]
    fn from_fields_enforces_the_field_table() {
        let mut fields: [Value; FIELD_COUNT] =
            std::array::from_fn(|index| match FIELD_SPECS[index].kind {
                FieldKind::Number => Value::Number(1.0),
                FieldKind::Text => Value::Text(Vec::new()),
            });
        fields[FieldId::Rules1.index()] = Value::Number(1.0);
        assert_eq!(
            Wcfg::from_fields(fields).unwrap_err(),
            EncodeError::FieldKind {
                index: 32,
                expected: FieldKind::Text
            }
        );
    }

    #[test]
    fn exchange_starts_only_after_team() {
        let sender = ConfigExchange::new(Side::One, sample());
        let receiver = ConfigExchange::new(Side::Two, sample());
        for exchange in [&sender, &receiver] {
            assert_eq!(exchange.phase(), Phase::AwaitingTeam);
            assert!(!exchange.ready_for_battle());
            assert!(exchange.remote().is_none());
        }
    }

    #[test]
    fn side_one_sends_one_frame_after_team() {
        let config = sample();
        let mut exchange = ConfigExchange::new(Side::One, config.clone());
        let mut transport = MockTransport::default();
        let outcome = exchange
            .after_team(&mut transport, Duration::from_secs(1))
            .unwrap();
        let frame = match outcome {
            ExchangeOutcome::Sent(frame) => frame,
            _ => panic!("side one must send"),
        };
        assert_eq!(frame, config.encode_frame().unwrap());
        assert_eq!(transport.sent, vec![frame]);
        assert_eq!(transport.recv_calls, 0);
        assert_eq!(exchange.phase(), Phase::Sent);
        assert!(exchange.ready_for_battle());
        assert!(exchange.remote().is_none());
    }

    #[test]
    fn side_two_waits_and_applies_the_remote_config() {
        let config = sample();
        let mut exchange = ConfigExchange::new(Side::Two, Wcfg::default());
        let mut transport = MockTransport::default();
        transport.inbox.push_back(b"PING\x1f123".to_vec());
        transport.inbox.push_back(config.encode_frame().unwrap());
        let outcome = exchange
            .after_team(&mut transport, Duration::from_secs(1))
            .unwrap();
        assert_eq!(outcome, ExchangeOutcome::Applied(config.clone()));
        assert_eq!(exchange.remote(), Some(&config));
        assert_eq!(exchange.phase(), Phase::Applied);
        assert!(exchange.ready_for_battle());
        assert!(transport.sent.is_empty());
        assert_eq!(transport.recv_calls, 2);
    }

    #[test]
    fn side_two_times_out_and_can_retry() {
        let config = sample();
        let mut exchange = ConfigExchange::new(Side::Two, Wcfg::default());
        let mut transport = MockTransport::default();
        assert_eq!(
            exchange
                .after_team(&mut transport, Duration::from_secs(1))
                .unwrap_err(),
            ExchangeError::Timeout
        );
        assert_eq!(exchange.phase(), Phase::AwaitingTeam);
        assert!(!exchange.ready_for_battle());
        transport.inbox.push_back(config.encode_frame().unwrap());
        assert_eq!(
            exchange
                .after_team(&mut transport, Duration::from_secs(1))
                .unwrap(),
            ExchangeOutcome::Applied(config)
        );
    }

    #[test]
    fn side_two_rejects_malformed_wcfg_frames() {
        let mut exchange = ConfigExchange::new(Side::Two, Wcfg::default());
        let mut transport = MockTransport::default();
        transport.inbox.push_back(b"WCFG".to_vec());
        assert_eq!(
            exchange
                .after_team(&mut transport, Duration::from_secs(1))
                .unwrap_err(),
            ExchangeError::Decode(DecodeError::MissingTagSeparator)
        );
        assert_eq!(exchange.phase(), Phase::AwaitingTeam);

        transport.inbox.push_back(b"WCFG\x1f1\x1d2".to_vec());
        assert_eq!(
            exchange
                .after_team(&mut transport, Duration::from_secs(1))
                .unwrap_err(),
            ExchangeError::Decode(DecodeError::FieldCount { actual: 2 })
        );
    }

    #[test]
    fn side_two_bounds_unrelated_frames() {
        let mut exchange = ConfigExchange::new(Side::Two, Wcfg::default());
        let mut transport = MockTransport::default();
        for _ in 0..=MAX_IGNORED_FRAMES {
            transport.inbox.push_back(b"PING\x1f1".to_vec());
        }
        assert_eq!(
            exchange
                .after_team(&mut transport, Duration::from_secs(1))
                .unwrap_err(),
            ExchangeError::UnexpectedFrame
        );
        assert_eq!(exchange.phase(), Phase::AwaitingTeam);
    }

    #[test]
    fn side_one_keeps_awaiting_team_when_send_fails() {
        let config = sample();
        let mut exchange = ConfigExchange::new(Side::One, config);
        let mut transport = MockTransport {
            send_fails: true,
            ..MockTransport::default()
        };
        assert_eq!(
            exchange
                .after_team(&mut transport, Duration::from_secs(1))
                .unwrap_err(),
            ExchangeError::Transport("send failed")
        );
        assert_eq!(exchange.phase(), Phase::AwaitingTeam);
        assert!(!exchange.ready_for_battle());
    }

    #[test]
    fn exchange_rejects_a_second_after_team_call() {
        let config = sample();
        let mut sender = ConfigExchange::new(Side::One, config.clone());
        let mut sender_transport = MockTransport::default();
        sender
            .after_team(&mut sender_transport, Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            sender
                .after_team(&mut sender_transport, Duration::from_secs(1))
                .unwrap_err(),
            ExchangeError::Phase(Phase::Sent)
        );

        let mut receiver = ConfigExchange::new(Side::Two, Wcfg::default());
        let mut receiver_transport = MockTransport::default();
        receiver_transport
            .inbox
            .push_back(config.encode_frame().unwrap());
        receiver
            .after_team(&mut receiver_transport, Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            receiver
                .after_team(&mut receiver_transport, Duration::from_secs(1))
                .unwrap_err(),
            ExchangeError::Phase(Phase::Applied)
        );
    }

    #[test]
    fn full_post_team_exchange_agrees_on_the_config() {
        let config = sample();
        let mut sender = ConfigExchange::new(Side::One, config.clone());
        let mut receiver = ConfigExchange::new(Side::Two, Wcfg::default());
        let mut sender_transport = MockTransport::default();
        let frame = match sender
            .after_team(&mut sender_transport, Duration::from_secs(1))
            .unwrap()
        {
            ExchangeOutcome::Sent(frame) => frame,
            _ => panic!("side one must send"),
        };
        let mut receiver_transport = MockTransport::default();
        receiver_transport.inbox.push_back(frame);
        let applied = match receiver
            .after_team(&mut receiver_transport, Duration::from_secs(1))
            .unwrap()
        {
            ExchangeOutcome::Applied(config) => config,
            _ => panic!("side two must apply"),
        };
        assert_eq!(applied, config);
        assert_eq!(receiver.remote(), Some(sender.local()));
        assert!(sender.ready_for_battle());
        assert!(receiver.ready_for_battle());
        assert_eq!(sender_transport.recv_calls, 0);
        assert!(receiver_transport.sent.is_empty());
    }
}
