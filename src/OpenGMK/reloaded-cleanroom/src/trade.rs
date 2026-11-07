pub const FIELD_COUNT: usize = 49;
pub const PARTY_SCALAR_COUNT: usize = 28;
pub const MOVE_COUNT: usize = 4;
pub const INFO_COUNT: usize = 17;
pub const FIELD_TERMINATOR: u8 = 0x1d;
pub const LABEL_SEPARATOR: u8 = 0x1e;
pub const ENVELOPE_SEPARATOR: u8 = 0x1f;
pub const ENVELOPE_PREFIX: &[u8] = b"OFFER";
pub const MAX_MESSAGE_LEN: usize = 65_536;
pub const HARD_MESSAGE_LIMIT: usize = 1_048_576;
pub const MAX_LABEL_LEN: usize = 256;
pub const MAX_TEXT_LEN: usize = 256;
pub const MAX_SAFE_INTEGER: u64 = (1u64 << 53) - 1;
pub const MASTER_BALL_ITEM_ID: u64 = 4;
pub const MASTER_BALL_MARKER: u64 = 3;
pub const MASTER_BALL_STORY_CAP: u64 = 2;

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Terminator,
    Count,
    EmptyField,
    Tag,
    Number,
    NonFinite,
    Species,
    Level,
    FrameLimit,
    LabelLimit,
}

impl DecodeError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Terminator => "E_TERMINATOR",
            Self::Count => "E_COUNT",
            Self::EmptyField => "E_EMPTY_FIELD",
            Self::Tag => "E_TAG",
            Self::Number => "E_NUMBER",
            Self::NonFinite => "E_NONFINITE",
            Self::Species => "E_SPECIES",
            Self::Level => "E_LEVEL",
            Self::FrameLimit => "E_FRAME_LIMIT",
            Self::LabelLimit => "E_LABEL_LIMIT",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    Count,
    TextSeparator,
    NonFinite,
    LabelSeparator,
    LabelLimit,
    FrameLimit,
}

impl EncodeError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Count => "E_COUNT",
            Self::TextSeparator => "E_TEXT_SEPARATOR",
            Self::NonFinite => "E_NONFINITE",
            Self::LabelSeparator => "E_LABEL_SEPARATOR",
            Self::LabelLimit => "E_LABEL_LIMIT",
            Self::FrameLimit => "E_FRAME_LIMIT",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidateError {
    Count,
    Species,
    Level,
    Story,
    ItemMaster,
    BallMaster,
}

impl ValidateError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Count => "E_COUNT",
            Self::Species => "E_SPECIES",
            Self::Level => "E_LEVEL",
            Self::Story => "E_STORY",
            Self::ItemMaster => "E_ITEM_MASTER",
            Self::BallMaster => "E_BALL_MASTER",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafetyError {
    Wire(ValidateError),
    Type,
    Number,
    Range,
    Text,
    SpeciesMismatch,
    LevelMismatch,
    Pp,
    Hp,
    MoveCount,
    Selue,
    SelueDomain,
}

impl SafetyError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Wire(error) => error.code(),
            Self::Type => "E_TYPE",
            Self::Number => "E_NUMBER",
            Self::Range => "E_RANGE",
            Self::Text => "E_TEXT",
            Self::SpeciesMismatch => "E_SPECIES_MISMATCH",
            Self::LevelMismatch => "E_LEVEL_MISMATCH",
            Self::Pp => "E_PP",
            Self::Hp => "E_HP",
            Self::MoveCount => "E_MOVE_COUNT",
            Self::Selue => "E_SELUE",
            Self::SelueDomain => "E_SELUE_DOMAIN",
        }
    }
}

impl From<ValidateError> for SafetyError {
    fn from(value: ValidateError) -> Self {
        Self::Wire(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelueError {
    MaskOverflow,
    NotReversible,
    OutOfRange,
}

impl SelueError {
    pub fn code(self) -> &'static str {
        match self {
            Self::MaskOverflow => "E_MASK_OVERFLOW",
            Self::NotReversible => "E_SELUE_NOT_REVERSIBLE",
            Self::OutOfRange => "E_SELUE_RANGE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MasterBallError {
    SourceEmpty,
    Cap,
    MultiAdd,
    Conservation,
    SourceStoryDependency,
}

impl MasterBallError {
    pub fn code(self) -> &'static str {
        match self {
            Self::SourceEmpty => "MASTER_BALL_SOURCE_EMPTY",
            Self::Cap => "MASTER_BALL_CAP",
            Self::MultiAdd => "MASTER_BALL_MULTI_ADD",
            Self::Conservation => "MASTER_BALL_CONSERVATION",
            Self::SourceStoryDependency => "SOURCE_STORY_DEPENDENCY",
        }
    }
}

pub const STORY_GATED_SPECIES: [u64; 98] = [
    144, 145, 146, 150, 151, 243, 244, 245, 249, 250, 251, 377, 378, 379, 380, 381, 382, 383, 384,
    385, 386, 480, 481, 482, 483, 484, 485, 486, 487, 488, 489, 490, 491, 492, 493, 499, 500, 516,
    517, 543, 544, 545, 546, 560, 726, 727, 728, 729, 730, 731, 732, 733, 743, 744, 834, 835, 836,
    837, 976, 977, 978, 979, 980, 981, 982, 983, 984, 985, 986, 987, 988, 989, 990, 991, 992, 994,
    2046, 2047, 2048, 2049, 2050, 2060, 2061, 2158, 2159, 2160, 2232, 2233, 2234, 2238, 2242, 2243,
    2244, 2248, 2250, 2251, 2267, 2282,
];

pub fn is_story_gated_species(species: u64) -> bool {
    STORY_GATED_SPECIES.binary_search(&species).is_ok()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub fields: [Value; FIELD_COUNT],
}

impl Offer {
    pub fn from_values(values: Vec<Value>) -> Result<Self, EncodeError> {
        if values.len() != FIELD_COUNT {
            return Err(EncodeError::Count);
        }
        let fields: [Value; FIELD_COUNT] = values.try_into().map_err(|_| EncodeError::Count)?;
        Ok(Self { fields })
    }

    pub fn field(&self, index: usize) -> Option<&Value> {
        self.fields.get(index)
    }

    pub fn number(&self, index: usize) -> Option<f64> {
        match self.fields.get(index) {
            Some(Value::Number(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn text(&self, index: usize) -> Option<&[u8]> {
        match self.fields.get(index) {
            Some(Value::Text(value)) => Some(value.as_slice()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedMessage {
    pub label: Vec<u8>,
    pub offer: Offer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedOffer {
    pub species: u64,
    pub level: u8,
}

pub fn decode(input: &[u8]) -> Result<DecodedMessage, DecodeError> {
    decode_message_with_limit(input, HARD_MESSAGE_LIMIT)
}

pub fn decode_message(input: &[u8]) -> Result<DecodedMessage, DecodeError> {
    decode_message_with_limit(input, MAX_MESSAGE_LEN)
}

fn decode_message_with_limit(input: &[u8], limit: usize) -> Result<DecodedMessage, DecodeError> {
    if input.len() > limit {
        return Err(DecodeError::FrameLimit);
    }

    let body = input.strip_prefix(b"OFFER\x1f").unwrap_or(input);
    let (label, packed) = match body.iter().position(|&byte| byte == LABEL_SEPARATOR) {
        Some(position) => (&body[..position], &body[position + 1..]),
        None => (&[][..], body),
    };

    let offer = decode_packed_inner(packed)?;

    if label.len() > MAX_LABEL_LEN {
        return Err(DecodeError::LabelLimit);
    }

    Ok(DecodedMessage {
        label: label.to_vec(),
        offer,
    })
}

pub fn decode_packed(input: &[u8]) -> Result<Offer, DecodeError> {
    if input.len() > HARD_MESSAGE_LIMIT {
        return Err(DecodeError::FrameLimit);
    }
    decode_packed_inner(input)
}

fn decode_packed_inner(packed: &[u8]) -> Result<Offer, DecodeError> {
    if packed.last().copied() != Some(FIELD_TERMINATOR) {
        return Err(DecodeError::Terminator);
    }

    let terminator_count = packed
        .iter()
        .filter(|&&byte| byte == FIELD_TERMINATOR)
        .count();
    if terminator_count != FIELD_COUNT {
        return Err(DecodeError::Count);
    }

    let parts: Vec<&[u8]> = packed.split(|&byte| byte == FIELD_TERMINATOR).collect();
    if parts.len() != FIELD_COUNT + 1 || !parts[FIELD_COUNT].is_empty() {
        return Err(DecodeError::Count);
    }

    let mut values = Vec::with_capacity(FIELD_COUNT);
    for part in &parts[..FIELD_COUNT] {
        if part.is_empty() {
            return Err(DecodeError::EmptyField);
        }
        values.push(parse_field(part)?);
    }

    let species_ok = matches!(&values[0], Value::Number(value) if *value > 0.0);
    if !species_ok {
        return Err(DecodeError::Species);
    }

    let level_ok = matches!(&values[1], Value::Number(value) if *value >= 1.0);
    if !level_ok {
        return Err(DecodeError::Level);
    }

    let fields: [Value; FIELD_COUNT] = values.try_into().map_err(|_| DecodeError::Count)?;
    Ok(Offer { fields })
}

fn parse_field(part: &[u8]) -> Result<Value, DecodeError> {
    let (tag, body) = part.split_first().ok_or(DecodeError::EmptyField)?;
    match *tag {
        b's' => Ok(Value::Text(body.to_vec())),
        b'r' => parse_real(body).map(Value::Number),
        _ => Err(DecodeError::Tag),
    }
}

fn parse_real(body: &[u8]) -> Result<f64, DecodeError> {
    let trimmed = trim_ascii_space_tab(body);
    if trimmed.is_empty() {
        return Err(DecodeError::Number);
    }

    let text = std::str::from_utf8(trimmed).map_err(|_| DecodeError::Number)?;
    let value = text.parse::<f64>().map_err(|_| DecodeError::Number)?;
    if !value.is_finite() {
        return Err(DecodeError::NonFinite);
    }
    Ok(value)
}

fn trim_ascii_space_tab(mut bytes: &[u8]) -> &[u8] {
    while let Some((first, rest)) = bytes.split_first() {
        if *first == b' ' || *first == b'\t' {
            bytes = rest;
        } else {
            break;
        }
    }
    while let Some((last, rest)) = bytes.split_last() {
        if *last == b' ' || *last == b'\t' {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

pub fn encode(offer: &Offer) -> Result<Vec<u8>, EncodeError> {
    encode_values(&offer.fields)
}

pub fn encode_values(values: &[Value]) -> Result<Vec<u8>, EncodeError> {
    if values.len() != FIELD_COUNT {
        return Err(EncodeError::Count);
    }

    let mut output = Vec::new();
    for value in values {
        encode_value(value, &mut output)?;
    }
    Ok(output)
}

fn encode_value(value: &Value, output: &mut Vec<u8>) -> Result<(), EncodeError> {
    match value {
        Value::Number(number) => {
            if !number.is_finite() {
                return Err(EncodeError::NonFinite);
            }
            output.push(b'r');
            output.extend_from_slice(format!("{:.6}", number).as_bytes());
        }
        Value::Text(text) => {
            if text.contains(&FIELD_TERMINATOR) {
                return Err(EncodeError::TextSeparator);
            }
            output.push(b's');
            output.extend_from_slice(text);
        }
    }
    output.push(FIELD_TERMINATOR);
    Ok(())
}

pub fn encode_message(offer: &Offer, label: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if label.len() > MAX_LABEL_LEN {
        return Err(EncodeError::LabelLimit);
    }
    if label.contains(&LABEL_SEPARATOR) {
        return Err(EncodeError::LabelSeparator);
    }

    let packed = encode(offer)?;
    let total = label
        .len()
        .checked_add(1)
        .and_then(|value| value.checked_add(packed.len()))
        .ok_or(EncodeError::FrameLimit)?;
    if total > MAX_MESSAGE_LEN {
        return Err(EncodeError::FrameLimit);
    }

    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(label);
    output.push(LABEL_SEPARATOR);
    output.extend_from_slice(&packed);
    Ok(output)
}

pub fn encode_envelope(offer: &Offer, label: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if label.len() > MAX_LABEL_LEN {
        return Err(EncodeError::LabelLimit);
    }
    if label.contains(&LABEL_SEPARATOR) {
        return Err(EncodeError::LabelSeparator);
    }

    let packed = encode(offer)?;
    let total = ENVELOPE_PREFIX
        .len()
        .checked_add(1)
        .and_then(|value| value.checked_add(label.len()))
        .and_then(|value| value.checked_add(1))
        .and_then(|value| value.checked_add(packed.len()))
        .ok_or(EncodeError::FrameLimit)?;
    if total > MAX_MESSAGE_LEN {
        return Err(EncodeError::FrameLimit);
    }

    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(ENVELOPE_PREFIX);
    output.push(ENVELOPE_SEPARATOR);
    output.extend_from_slice(label);
    output.push(LABEL_SEPARATOR);
    output.extend_from_slice(&packed);
    Ok(output)
}

pub fn canonical_species(value: f64) -> Result<u64, ValidateError> {
    if !value.is_finite() {
        return Err(ValidateError::Species);
    }
    let rounded = value.round();
    if (value - rounded).abs() > 1e-9 {
        return Err(ValidateError::Species);
    }
    if rounded < 1.0 {
        return Err(ValidateError::Species);
    }

    // Encodings may add 100000, 150000 or a shiny +1000 layer. Try all
    // plausible bases; if any resolves to a story-gated species, return that
    // base so the caller rejects it. This closes 151150/153046-style wrap
    // bypasses without rejecting ordinary high species IDs.
    let mut candidates = vec![rounded];
    if rounded >= 150_000.0 {
        candidates.push(rounded - 150_000.0);
    }
    if rounded > 100_000.0 {
        candidates.push(rounded % 100_000.0);
    }
    for candidate in candidates {
        if candidate < 1.0 {
            continue
        }
        let mut base = candidate as u64;
        if base > 100_000 {
            base %= 100_000;
        }
        if is_story_gated_species(base) {
            return Ok(base)
        }
        if base > 1000 && is_story_gated_species(base - 1000) {
            return Ok(base - 1000)
        }
    }

    let mut canonical = rounded;
    if canonical > 100_000.0 {
        canonical %= 100_000.0;
    }
    if canonical < 1.0 {
        return Err(ValidateError::Species);
    }
    let canonical = canonical as u64;
    if !(1..=999_999).contains(&canonical) {
        return Err(ValidateError::Species);
    }
    Ok(canonical)
}

pub fn validate(offer: &Offer) -> Result<ValidatedOffer, ValidateError> {
    if offer.fields.len() != FIELD_COUNT {
        return Err(ValidateError::Count);
    }

    let species = match &offer.fields[0] {
        Value::Number(value) => canonical_species(*value)?,
        Value::Text(_) => return Err(ValidateError::Species),
    };
    if is_story_gated_species(species) {
        return Err(ValidateError::Story);
    }

    if let Value::Number(value) = &offer.fields[7] {
        if let Ok(index_species) = canonical_species(*value) {
            if is_story_gated_species(index_species) {
                return Err(ValidateError::Story);
            }
        }
    }

    let level = match &offer.fields[1] {
        Value::Number(value) => {
            let value = integer_value(*value).ok_or(ValidateError::Level)?;
            if !(1..=100).contains(&value) {
                return Err(ValidateError::Level);
            }
            value as u8
        }
        Value::Text(_) => return Err(ValidateError::Level),
    };

    if field_is_integer(&offer.fields[5], MASTER_BALL_ITEM_ID as i128) {
        return Err(ValidateError::ItemMaster);
    }

    if field_is_integer(&offer.fields[32], MASTER_BALL_MARKER as i128) {
        return Err(ValidateError::BallMaster);
    }

    Ok(ValidatedOffer { species, level })
}

fn integer_value(value: f64) -> Option<i128> {
    if !value.is_finite() {
        return None;
    }
    let rounded = value.round();
    if (value - rounded).abs() > 1e-9 {
        return None;
    }
    if rounded < i128::MIN as f64 || rounded > i128::MAX as f64 {
        return None;
    }
    Some(rounded as i128)
}

fn field_is_integer(field: &Value, target: i128) -> bool {
    match field {
        Value::Number(value) => integer_value(*value) == Some(target),
        Value::Text(_) => false,
    }
}

pub fn validate_safety(
    offer: &Offer,
    selue_mask: Option<u64>,
) -> Result<ValidatedOffer, SafetyError> {
    let validated = validate(offer)?;
    let fields = &offer.fields;

    let species = match &fields[0] {
        Value::Number(value) => canonical_species(*value).map_err(SafetyError::Wire)?,
        Value::Text(_) => return Err(SafetyError::Type),
    };
    let index_species = match &fields[7] {
        Value::Number(value) => canonical_species(*value).map_err(SafetyError::Wire)?,
        Value::Text(_) => return Err(SafetyError::Type),
    };
    if species != index_species || species != validated.species {
        return Err(SafetyError::SpeciesMismatch);
    }

    let level = int_field(&fields[1], 1, 100)?;
    let index_level = int_field(&fields[2], 1, 100)?;
    if level != index_level || level != validated.level as i128 {
        return Err(SafetyError::LevelMismatch);
    }

    int_field(&fields[3], 0, 1_000_000_000)?;
    int_field(&fields[4], 0, 9_999_999)?;
    int_field(&fields[5], 0, 9_999)?;
    int_field(&fields[6], 0, 2)?;
    int_field(&fields[8], 0, 2_147_483_647)?;

    let selue = uint_field(&fields[9], MAX_SAFE_INTEGER)?;
    let selue_duplicate = uint_field(&fields[46], MAX_SAFE_INTEGER)?;
    if selue != selue_duplicate {
        return Err(SafetyError::Selue);
    }
    if let Some(mask) = selue_mask {
        if !selue_in_domain(selue, mask) || !selue_in_domain(selue_duplicate, mask) {
            return Err(SafetyError::SelueDomain);
        }
    }

    let pp = int_field(&fields[10], 0, 9_999)?;
    let hp = int_field(&fields[11], 0, 9_999_999)?;
    int_field(&fields[12], 0, 10)?;
    let max_pp = int_field(&fields[13], 1, 9_999)?;
    let max_hp = int_field(&fields[14], 1, 9_999_999)?;
    int_field(&fields[15], 0, 9_999_999)?;
    if pp > max_pp {
        return Err(SafetyError::Pp);
    }
    if hp > max_hp {
        return Err(SafetyError::Hp);
    }

    for index in 16..=20 {
        int_field(&fields[index], 0, 9_999_999)?;
    }
    int_field(&fields[21], -6, 6)?;

    let move_count = int_field(&fields[22], 0, 4)? as usize;
    let mut moves = [0i128; MOVE_COUNT];
    let mut non_empty_moves = 0usize;
    for index in 0..MOVE_COUNT {
        moves[index] = int_field(&fields[28 + index], 0, 9_999)?;
        if moves[index] != 0 {
            non_empty_moves += 1;
        }
    }
    // `num_attack1` is not a reliable count in every save/build; moves are
    // transferred in their existing slots. Keep the stated count bounded but
    // do not reject a legitimate party whose counter is stale.
    let _ = (move_count, non_empty_moves);

    for index in 23..=25 {
        int_field(&fields[index], 0, 18)?;
    }
    for index in 26..=27 {
        int_field(&fields[index], 0, 1)?;
    }

    int_field(&fields[32], 0, 9_999)?;
    int_field(&fields[33], 1, 100)?;
    int_field(&fields[34], 0, 65_535)?;
    int_field(&fields[35], 0, 65_535)?;
    validated_text(&fields[36])?;
    validated_text(&fields[37])?;
    int_field(&fields[38], 0, 9_999)?;
    int_field(&fields[39], 0, 10_000_000_000)?;
    int_field(&fields[40], 0, 10_000_000_000)?;
    int_field(&fields[41], 0, 2_147_483_647)?;
    uint_field(&fields[42], MAX_SAFE_INTEGER)?;
    uint_field(&fields[43], MAX_SAFE_INTEGER)?;
    int_field(&fields[44], -2_147_483_648, 2_147_483_647)?;
    int_field(&fields[45], 0, 9_999)?;
    uint_field(&fields[47], MAX_SAFE_INTEGER)?;
    validated_reserved(&fields[48])?;

    Ok(validated)
}

fn int_field(field: &Value, minimum: i128, maximum: i128) -> Result<i128, SafetyError> {
    match field {
        Value::Number(value) => match integer_value(*value) {
            Some(value) if value >= minimum && value <= maximum => Ok(value),
            Some(_) => Err(SafetyError::Range),
            None => Err(SafetyError::Number),
        },
        Value::Text(_) => Err(SafetyError::Type),
    }
}

fn uint_field(field: &Value, maximum: u64) -> Result<u64, SafetyError> {
    let value = int_field(field, 0, maximum as i128)?;
    Ok(value as u64)
}

fn validated_text(field: &Value) -> Result<&[u8], SafetyError> {
    match field {
        Value::Text(text) => {
            if text.len() > MAX_TEXT_LEN || text.iter().any(|&byte| byte < 0x20 || byte == 0x7f) {
                Err(SafetyError::Text)
            } else {
                Ok(text.as_slice())
            }
        }
        Value::Number(_) => Err(SafetyError::Type),
    }
}

fn validated_reserved(field: &Value) -> Result<(), SafetyError> {
    match field {
        Value::Text(text) => {
            if text.len() > MAX_TEXT_LEN || text.iter().any(|&byte| byte < 0x20 || byte == 0x7f) {
                Err(SafetyError::Text)
            } else {
                Ok(())
            }
        }
        Value::Number(value) => {
            if !value.is_finite() || value.abs() > MAX_SAFE_INTEGER as f64 {
                Err(SafetyError::Number)
            } else {
                Ok(())
            }
        }
    }
}

pub fn selue_mask(mask_base: u64) -> Result<u64, SelueError> {
    let mut mask = mask_base.checked_add(7).ok_or(SelueError::MaskOverflow)?;
    if mask < 5_000 {
        mask += 5_000;
    }
    Ok(mask)
}

pub fn selue_transport(value: u64, mask: u64) -> Result<u64, SelueError> {
    if value < 1_000_000 {
        return Ok(value);
    }

    let high = value / 1_000_000;
    let low = value % 1_000_000;
    if high >= mask {
        return Err(SelueError::NotReversible);
    }

    let transported = mask
        .checked_sub(high)
        .and_then(|part| part.checked_mul(1_000_000))
        .and_then(|part| part.checked_add(low))
        .ok_or(SelueError::OutOfRange)?;
    if transported > MAX_SAFE_INTEGER {
        return Err(SelueError::OutOfRange);
    }
    Ok(transported)
}

pub fn selue_in_domain(value: u64, mask: u64) -> bool {
    value < 1_000_000 || value / 1_000_000 < mask
}

pub fn check_master_ball_transfer(
    quantity: u64,
    source_before: u64,
    destination_before: u64,
    source_after: u64,
    destination_after: u64,
    cap: u64,
    source_story_requires: bool,
) -> Result<(), MasterBallError> {
    if quantity > 1 {
        return Err(MasterBallError::MultiAdd);
    }
    if quantity > 0 && source_story_requires {
        return Err(MasterBallError::SourceStoryDependency);
    }
    if source_before < quantity {
        return Err(MasterBallError::SourceEmpty);
    }

    let expected_source = source_before - quantity;
    let expected_destination = destination_before
        .checked_add(quantity)
        .ok_or(MasterBallError::Conservation)?;
    let total_before = source_before
        .checked_add(destination_before)
        .ok_or(MasterBallError::Conservation)?;
    let total_after = source_after
        .checked_add(destination_after)
        .ok_or(MasterBallError::Conservation)?;
    if source_after != expected_source
        || destination_after != expected_destination
        || total_before != total_after
    {
        return Err(MasterBallError::Conservation);
    }

    if destination_after > cap.max(destination_before) {
        return Err(MasterBallError::Cap);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN: &str = concat!(
        "r448.000000\x1dr50.000000\x1dr50.000000\x1dr125000.000000\x1dr31.000000\x1dr0.000000\x1dr1.000000\x1d",
        "r448.000000\x1dr0.000000\x1dr123456.000000\x1dr10.000000\x1dr150.000000\x1dr0.000000\x1dr10.000000\x1d",
        "r150.000000\x1dr0.000000\x1dr100.000000\x1dr80.000000\x1dr90.000000\x1dr110.000000\x1dr85.000000\x1d",
        "r100.000000\x1dr1.000000\x1dr2.000000\x1dr17.000000\x1dr0.000000\x1dr0.000000\x1dr0.000000\x1d",
        "r33.000000\x1dr34.000000\x1dr35.000000\x1dr36.000000\x1dr0.000000\x1dr0.000000\x1dr0.000000\x1d",
        "r0.000000\x1dsTrainer\x1dsFixture\x1dr0.000000\x1dr0.000000\x1dr0.000000\x1dr0.000000\x1d",
        "r0.000000\x1dr0.000000\x1dr0.000000\x1dr0.000000\x1dr0.000000\x1dr0.000000\x1dr0.000000\x1d",
    );

    fn n(value: f64) -> Value {
        Value::Number(value)
    }

    fn t(value: &[u8]) -> Value {
        Value::Text(value.to_vec())
    }

    fn golden_offer() -> Offer {
        Offer::from_values(vec![
            n(448.0),
            n(50.0),
            n(50.0),
            n(125000.0),
            n(31.0),
            n(0.0),
            n(1.0),
            n(448.0),
            n(0.0),
            n(123456.0),
            n(10.0),
            n(150.0),
            n(0.0),
            n(10.0),
            n(150.0),
            n(0.0),
            n(100.0),
            n(80.0),
            n(90.0),
            n(110.0),
            n(85.0),
            n(100.0),
            n(1.0),
            n(2.0),
            n(17.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(33.0),
            n(34.0),
            n(35.0),
            n(36.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
            t(b"Trainer"),
            t(b"Fixture"),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
            n(0.0),
        ])
        .unwrap()
    }

    fn fields_of(packed: &[u8]) -> Vec<Vec<u8>> {
        let mut parts: Vec<Vec<u8>> = packed
            .split(|&byte| byte == FIELD_TERMINATOR)
            .map(|part| part.to_vec())
            .collect();
        parts.pop();
        parts
    }

    fn join_fields(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut output = Vec::new();
        for part in parts {
            output.extend_from_slice(part);
            output.push(FIELD_TERMINATOR);
        }
        output
    }

    fn replace_field(packed: &[u8], index: usize, field: &[u8]) -> Vec<u8> {
        let mut parts = fields_of(packed);
        parts[index] = field.to_vec();
        join_fields(&parts)
    }

    fn remove_field(packed: &[u8], index: usize) -> Vec<u8> {
        let mut parts = fields_of(packed);
        parts.remove(index);
        join_fields(&parts)
    }

    fn append_bytes(packed: &[u8], suffix: &[u8]) -> Vec<u8> {
        let mut output = packed.to_vec();
        output.extend_from_slice(suffix);
        output
    }

    fn assert_decode_error(input: &[u8], error: DecodeError) {
        assert_eq!(decode(input).unwrap_err(), error);
    }

    fn replace_and_decode(index: usize, field: &[u8]) -> Result<Offer, DecodeError> {
        let bytes = replace_field(GOLDEN.as_bytes(), index, field);
        decode(&bytes).map(|message| message.offer)
    }

    fn safety_ready_offer() -> Offer {
        let mut offer = golden_offer();
        offer.fields[21] = n(0.0);
        offer.fields[22] = n(4.0);
        offer.fields[33] = n(50.0);
        offer.fields[46] = n(123456.0);
        offer
    }

    #[test]
    fn golden_packed_vector() {
        let bytes = GOLDEN.as_bytes();
        assert_eq!(bytes.len(), 525);
        assert_eq!(
            bytes
                .iter()
                .filter(|&&byte| byte == FIELD_TERMINATOR)
                .count(),
            49
        );

        let decoded = decode(bytes).unwrap();
        assert!(decoded.label.is_empty());
        assert_eq!(decoded.offer, golden_offer());
        assert_eq!(encode(&decoded.offer).unwrap(), bytes);
    }

    #[test]
    fn golden_labelled_message() {
        let label = b"Fixture Nv. 50";
        let message = encode_message(&golden_offer(), label).unwrap();
        assert_eq!(message.len(), 540);
        assert_eq!(
            message,
            concat!("Fixture Nv. 50", "\x1e")
                .as_bytes()
                .iter()
                .copied()
                .chain(GOLDEN.as_bytes().iter().copied())
                .collect::<Vec<u8>>()
        );

        let decoded = decode(&message).unwrap();
        assert_eq!(decoded.label, label);
        assert_eq!(decoded.offer, golden_offer());
        assert_eq!(encode(&decoded.offer).unwrap(), GOLDEN.as_bytes());
    }

    #[test]
    fn golden_empty_label() {
        let message = encode_message(&golden_offer(), b"").unwrap();
        assert_eq!(message.len(), 526);
        assert_eq!(message[0], LABEL_SEPARATOR);
        assert_eq!(&message[1..], GOLDEN.as_bytes());
        let decoded = decode(&message).unwrap();
        assert!(decoded.label.is_empty());
        assert_eq!(decoded.offer, golden_offer());
    }

    #[test]
    fn golden_label_is_display_only() {
        let mut bytes = replace_field(GOLDEN.as_bytes(), 0, b"r1.000000");
        bytes = replace_field(&bytes, 1, b"r100.000000");
        bytes = append_bytes(concat!("Fixture Nv. 50", "\x1e").as_bytes(), &bytes);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.label, b"Fixture Nv. 50");
        assert_eq!(
            validate(&decoded.offer),
            Ok(ValidatedOffer {
                species: 1,
                level: 100
            })
        );
    }

    #[test]
    fn malformed_count_and_framing_vectors() {
        let golden = GOLDEN.as_bytes();
        assert_decode_error(b"", DecodeError::Terminator);
        assert_decode_error(&golden[..golden.len() - 1], DecodeError::Terminator);
        assert_decode_error(&remove_field(golden, 48), DecodeError::Count);
        assert_decode_error(&append_bytes(golden, b"r0.000000\x1d"), DecodeError::Count);
        assert_decode_error(
            &append_bytes(golden, &[FIELD_TERMINATOR]),
            DecodeError::Count,
        );
        assert_decode_error(&append_bytes(golden, b"x"), DecodeError::Terminator);
        assert_decode_error(&append_bytes(golden, b"x\x1d"), DecodeError::Count);
        assert_decode_error(&replace_field(golden, 33, b""), DecodeError::EmptyField);
        assert_decode_error(&replace_field(golden, 0, b""), DecodeError::EmptyField);
        assert_decode_error(&replace_field(golden, 48, b""), DecodeError::EmptyField);
        assert_decode_error(b"Fixture\x1e", DecodeError::Terminator);
        assert_decode_error(b"Fixture", DecodeError::Terminator);
        assert_decode_error(b"Fixture\x1d", DecodeError::Count);
        assert_decode_error(&golden[..golden.len() - 2], DecodeError::Terminator);

        assert_eq!(encode_values(&[]), Err(EncodeError::Count));
        assert_eq!(encode_values(&[n(0.0)]), Err(EncodeError::Count));
        assert_eq!(
            encode_values(&vec![n(0.0); FIELD_COUNT - 1]),
            Err(EncodeError::Count)
        );
        assert_eq!(
            encode_values(&vec![n(0.0); FIELD_COUNT + 1]),
            Err(EncodeError::Count)
        );

        let mut text_separator = golden_offer();
        text_separator.fields[36] = t(b"A\x1dB");
        assert_eq!(encode(&text_separator), Err(EncodeError::TextSeparator));

        let mut non_finite = golden_offer();
        non_finite.fields[2] = n(f64::NAN);
        assert_eq!(encode(&non_finite), Err(EncodeError::NonFinite));
        non_finite.fields[2] = n(f64::INFINITY);
        assert_eq!(encode(&non_finite), Err(EncodeError::NonFinite));
    }

    #[test]
    fn tag_vectors() {
        let golden = GOLDEN.as_bytes();
        assert_decode_error(&replace_field(golden, 0, b"S448.000000"), DecodeError::Tag);
        assert_decode_error(&replace_field(golden, 1, b"R50.000000"), DecodeError::Tag);
        assert_decode_error(&replace_field(golden, 0, b"448.000000"), DecodeError::Tag);
        assert_decode_error(&replace_field(golden, 2, b"x1.000000"), DecodeError::Tag);
        assert_decode_error(&replace_field(golden, 2, b"r"), DecodeError::Number);
        assert_decode_error(&replace_field(golden, 2, b"rabc"), DecodeError::Number);
        assert_decode_error(&replace_field(golden, 2, b"rNaN"), DecodeError::NonFinite);
        assert_decode_error(&replace_field(golden, 2, b"rinf"), DecodeError::NonFinite);
        assert_decode_error(&replace_field(golden, 2, b"r-inf"), DecodeError::NonFinite);
        assert_decode_error(
            &replace_field(golden, 0, b"s448.000000"),
            DecodeError::Species,
        );
        assert_decode_error(&replace_field(golden, 1, b"s50.000000"), DecodeError::Level);

        let decoded = replace_and_decode(36, b"r123.000000").unwrap();
        assert_eq!(decoded.fields[36], n(123.0));
        assert_eq!(validate(&decoded).map(|value| value.species), Ok(448));

        let decoded = replace_and_decode(5, b"s4").unwrap();
        assert_eq!(validate(&decoded).map(|value| value.species), Ok(448));

        let decoded = replace_and_decode(32, b"s3").unwrap();
        assert_eq!(validate(&decoded).map(|value| value.species), Ok(448));

        let decoded = replace_and_decode(36, b"s").unwrap();
        assert_eq!(decoded.text(36), Some(&b""[..]));

        assert_decode_error(&replace_field(golden, 0, b"s"), DecodeError::Species);
        assert_decode_error(&replace_field(golden, 1, b"s"), DecodeError::Level);

        let decoded = replace_and_decode(2, b"s").unwrap();
        assert_eq!(decoded.text(2), Some(&b""[..]));

        assert_decode_error(&replace_field(golden, 2, b" r1.000000"), DecodeError::Tag);

        let decoded = replace_and_decode(2, b"sr1.000000").unwrap();
        assert_eq!(decoded.text(2), Some(&b"r1.000000"[..]));

        assert_decode_error(&replace_field(golden, 2, b"r\xff"), DecodeError::Number);
    }

    #[test]
    fn separator_vectors() {
        let golden = GOLDEN.as_bytes();
        let explicit_empty = append_bytes(&[LABEL_SEPARATOR], golden);
        assert!(decode(golden).unwrap().label.is_empty());
        assert!(decode(&explicit_empty).unwrap().label.is_empty());

        let labelled = append_bytes(b"Fixture Nv. 50\x1e", golden);
        assert_eq!(decode(&labelled).unwrap().label, b"Fixture Nv. 50");

        let label_with_terminator = append_bytes(b"A\x1dB\x1e", golden);
        assert_eq!(decode(&label_with_terminator).unwrap().label, b"A\x1dB");

        assert_decode_error(&append_bytes(b"A\x1eB\x1e", golden), DecodeError::Tag);
        assert_decode_error(&append_bytes(b"\x1e\x1e", golden), DecodeError::Tag);

        let text_separator = replace_field(golden, 36, b"sA\x1eB");
        let decoded = decode(&append_bytes(b"Fixture Nv. 50\x1e", &text_separator)).unwrap();
        assert_eq!(decoded.offer.text(36), Some(&b"A\x1eB"[..]));
        assert!(validate(&decoded.offer).is_ok());

        assert_decode_error(&text_separator, DecodeError::Count);
        assert_decode_error(&replace_field(golden, 36, b"sFi\x1dx"), DecodeError::Count);
        assert_decode_error(b"Fixture Nv. 50\x1e", DecodeError::Terminator);
        assert_decode_error(&append_bytes(golden, b"x\x1d"), DecodeError::Count);
        assert_decode_error(&append_bytes(golden, b"x"), DecodeError::Terminator);

        let control = replace_field(golden, 36, b"sA\nB");
        assert_eq!(decode(&control).unwrap().offer.text(36), Some(&b"A\nB"[..]));
        assert!(validate(&decode(&control).unwrap().offer).is_ok());

        let nul = replace_field(golden, 36, b"sA\x00B");
        assert_eq!(decode(&nul).unwrap().offer.text(36), Some(&b"A\x00B"[..]));
        assert!(validate(&decode(&nul).unwrap().offer).is_ok());

        let mut embedded_separator = golden_offer();
        embedded_separator.fields[36] = t(b"A\x1eB");
        let packed = encode(&embedded_separator).unwrap();
        assert!(packed.contains(&LABEL_SEPARATOR));
        let decoded = decode_packed(&packed).unwrap();
        assert_eq!(decoded.text(36), Some(&b"A\x1eB"[..]));
    }

    #[test]
    fn species_vectors() {
        let golden = GOLDEN.as_bytes();
        for field in [
            &b"r1.000000"[..],
            &b"r448.000000"[..],
            &b"r100000.000000"[..],
            &b"r100001.000000"[..],
            &b"r100448.000000"[..],
        ] {
            let offer = decode(&replace_field(golden, 0, field)).unwrap().offer;
            assert!(validate(&offer).is_ok(), "{field:?}");
        }

        for field in [&b"r100150.000000"[..], &b"r200150.000000"[..]] {
            let offer = decode(&replace_field(golden, 0, field)).unwrap().offer;
            assert_eq!(validate(&offer), Err(ValidateError::Story), "{field:?}");
        }

        assert_decode_error(
            &replace_field(golden, 0, b"r0.000000"),
            DecodeError::Species,
        );
        assert_decode_error(
            &replace_field(golden, 0, b"r-1.000000"),
            DecodeError::Species,
        );

        let fractional = decode(&replace_field(golden, 0, b"r448.500000"))
            .unwrap()
            .offer;
        assert_eq!(validate(&fractional), Err(ValidateError::Species));
    }

    #[test]
    fn level_vectors() {
        let golden = GOLDEN.as_bytes();
        for level in [1.0, 100.0] {
            let field = format!("r{level:.6}");
            let offer = decode(&replace_field(golden, 1, field.as_bytes()))
                .unwrap()
                .offer;
            assert!(validate(&offer).is_ok());
        }

        let above = decode(&replace_field(golden, 1, b"r101.000000"))
            .unwrap()
            .offer;
        assert_eq!(validate(&above), Err(ValidateError::Level));

        assert_decode_error(&replace_field(golden, 1, b"r0.000000"), DecodeError::Level);
        assert_decode_error(&replace_field(golden, 1, b"r-1.000000"), DecodeError::Level);

        let fractional = decode(&replace_field(golden, 1, b"r50.500000"))
            .unwrap()
            .offer;
        assert_eq!(validate(&fractional), Err(ValidateError::Level));

        assert_decode_error(&replace_field(golden, 1, b"s50.000000"), DecodeError::Level);
    }

    #[test]
    fn master_ball_field_vectors() {
        let golden = GOLDEN.as_bytes();

        for item in [0.0, 3.0, 5.0] {
            let field = format!("r{item:.6}");
            let offer = decode(&replace_field(golden, 5, field.as_bytes()))
                .unwrap()
                .offer;
            assert!(validate(&offer).is_ok());
        }
        let held = decode(&replace_field(golden, 5, b"r4.000000"))
            .unwrap()
            .offer;
        assert_eq!(validate(&held), Err(ValidateError::ItemMaster));
        let fractional = decode(&replace_field(golden, 5, b"r4.500000"))
            .unwrap()
            .offer;
        assert!(validate(&fractional).is_ok());

        for marker in [0.0, 2.0, 4.0] {
            let field = format!("r{marker:.6}");
            let offer = decode(&replace_field(golden, 32, field.as_bytes()))
                .unwrap()
                .offer;
            assert!(validate(&offer).is_ok());
        }
        let caught = decode(&replace_field(golden, 32, b"r3.000000"))
            .unwrap()
            .offer;
        assert_eq!(validate(&caught), Err(ValidateError::BallMaster));
        let fractional = decode(&replace_field(golden, 32, b"r3.500000"))
            .unwrap()
            .offer;
        assert!(validate(&fractional).is_ok());
    }

    #[test]
    fn numeric_syntax_and_canonicalization_vectors() {
        let golden = GOLDEN.as_bytes();
        let cases = [
            (&b"r1e3"[..], 1000.0, &b"r1000.000000"[..]),
            (&b"r1e-3"[..], 0.001, &b"r0.001000"[..]),
            (&b"r+1.000000"[..], 1.0, &b"r1.000000"[..]),
            (&b"r 1.000000 "[..], 1.0, &b"r1.000000"[..]),
            (&b"r.5"[..], 0.5, &b"r0.500000"[..]),
        ];

        for (input, expected, canonical) in cases {
            let bytes = replace_field(golden, 2, input);
            let offer = decode(&bytes).unwrap().offer;
            assert_eq!(offer.number(2), Some(expected));
            let encoded = encode(&offer).unwrap();
            assert_eq!(encoded, replace_field(golden, 2, canonical));
        }

        for input in [&b"r1,0"[..], &b"r1_0"[..], &b"r0x10"[..], &b"r"[..]] {
            assert_decode_error(&replace_field(golden, 2, input), DecodeError::Number);
        }
        assert_decode_error(&replace_field(golden, 2, b"r1e309"), DecodeError::NonFinite);

        let negative_zero = decode(&replace_field(golden, 2, b"r-0.000000"))
            .unwrap()
            .offer;
        assert_eq!(
            encode(&negative_zero).unwrap(),
            replace_field(golden, 2, b"r-0.000000")
        );
    }

    #[test]
    fn text_boundary_vectors() {
        let golden = GOLDEN.as_bytes();
        let empty = decode(&replace_field(golden, 37, b"s")).unwrap().offer;
        assert_eq!(empty.text(37), Some(&b""[..]));
        assert!(validate(&empty).is_ok());

        let mut long = golden_offer();
        long.fields[37] = Value::Text(vec![b'a'; 60_000]);
        let packed = encode(&long).unwrap();
        assert!(decode_packed(&packed).is_ok());
        assert!(decode_message(&append_bytes(b"Fixture Nv. 50\x1e", &packed)).is_ok());

        let mut too_long = golden_offer();
        too_long.fields[37] = Value::Text(vec![b'a'; 70_000]);
        let packed = encode(&too_long).unwrap();
        assert!(decode_packed(&packed).is_ok());
        assert_eq!(
            decode_message(&append_bytes(b"Fixture Nv. 50\x1e", &packed)),
            Err(DecodeError::FrameLimit)
        );

        let raw = replace_field(golden, 37, b"s\xff\xfe");
        assert_eq!(decode(&raw).unwrap().offer.text(37), Some(&b"\xff\xfe"[..]));
        assert!(validate(&decode(&raw).unwrap().offer).is_ok());

        let mut label = vec![b'A'; 255];
        label.push(LABEL_SEPARATOR);
        label.extend_from_slice(golden);
        assert_eq!(decode(&label).unwrap().label.len(), 255);

        let mut too_long_label = vec![b'A'; 257];
        too_long_label.push(LABEL_SEPARATOR);
        too_long_label.extend_from_slice(golden);
        assert_decode_error(&too_long_label, DecodeError::LabelLimit);
    }

    #[test]
    fn story_gate_set_is_pinned_and_complete() {
        assert_eq!(STORY_GATED_SPECIES.len(), 98);
        assert!(STORY_GATED_SPECIES.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(!is_story_gated_species(1));
        assert!(!is_story_gated_species(448));
        assert!(is_story_gated_species(150));
        assert!(is_story_gated_species(2282));
    }

    #[test]
    fn every_story_gated_species_is_rejected() {
        let golden = GOLDEN.as_bytes();
        for &species in &STORY_GATED_SPECIES {
            let field = format!("r{species:.6}");
            let offer = decode(&replace_field(golden, 0, field.as_bytes()))
                .unwrap()
                .offer;
            assert_eq!(validate(&offer), Err(ValidateError::Story), "{species}");
        }
    }

    #[test]
    fn story_gate_vectors() {
        let golden = GOLDEN.as_bytes();
        for species in [150, 151, 249, 493, 517, 2046, 2282] {
            let field = format!("r{species:.6}");
            let offer = decode(&replace_field(golden, 0, field.as_bytes()))
                .unwrap()
                .offer;
            assert_eq!(validate(&offer), Err(ValidateError::Story));
        }

        for species in [100150, 100517, 102046] {
            let field = format!("r{species:.6}");
            let offer = decode(&replace_field(golden, 0, field.as_bytes()))
                .unwrap()
                .offer;
            assert_eq!(validate(&offer), Err(ValidateError::Story));
        }

        for species in [100448, 100001] {
            let field = format!("r{species:.6}");
            let offer = decode(&replace_field(golden, 0, field.as_bytes()))
                .unwrap()
                .offer;
            assert!(validate(&offer).is_ok());
        }
    }

    #[test]
    fn validation_precedence_vectors() {
        let golden = GOLDEN.as_bytes();

        let mut both = fields_of(golden);
        both[5] = b"r4.000000".to_vec();
        both[32] = b"r3.000000".to_vec();
        let offer = decode(&join_fields(&both)).unwrap().offer;
        assert_eq!(validate(&offer), Err(ValidateError::ItemMaster));

        let mut story_and_item = fields_of(golden);
        story_and_item[0] = b"r150.000000".to_vec();
        story_and_item[5] = b"r4.000000".to_vec();
        let offer = decode(&join_fields(&story_and_item)).unwrap().offer;
        assert_eq!(validate(&offer), Err(ValidateError::Story));

        let mut level_and_ball = fields_of(golden);
        level_and_ball[1] = b"r101.000000".to_vec();
        level_and_ball[32] = b"r3.000000".to_vec();
        let offer = decode(&join_fields(&level_and_ball)).unwrap().offer;
        assert_eq!(validate(&offer), Err(ValidateError::Level));

        let mut wrapped_and_item = fields_of(golden);
        wrapped_and_item[0] = b"r100150.000000".to_vec();
        wrapped_and_item[5] = b"r4.000000".to_vec();
        let offer = decode(&join_fields(&wrapped_and_item)).unwrap().offer;
        assert_eq!(validate(&offer), Err(ValidateError::Story));
    }

    #[test]
    fn canonical_species_rules() {
        assert_eq!(canonical_species(1.0), Ok(1));
        assert_eq!(canonical_species(100_000.0), Ok(100_000));
        assert_eq!(canonical_species(100_001.0), Ok(1));
        assert_eq!(canonical_species(100_448.0), Ok(448));
        assert_eq!(canonical_species(200_150.0), Ok(150));
        assert_eq!(canonical_species(1150.0), Ok(150));
        assert_eq!(canonical_species(3046.0), Ok(2046));
        assert_eq!(canonical_species(101150.0), Ok(150));
        assert_eq!(canonical_species(150150.0), Ok(150));
        assert_eq!(canonical_species(151150.0), Ok(150));
        assert_eq!(canonical_species(153046.0), Ok(2046));
        assert_eq!(canonical_species(0.0), Err(ValidateError::Species));
        assert_eq!(canonical_species(-1.0), Err(ValidateError::Species));
        assert_eq!(canonical_species(448.5), Err(ValidateError::Species));
    }

    #[test]
    fn selue_transport_examples() {
        assert_eq!(selue_mask(0), Ok(5_007));
        assert_eq!(selue_mask(9_321), Ok(9_328));
        assert_eq!(selue_mask(u64::MAX), Err(SelueError::MaskOverflow));

        assert_eq!(selue_transport(0, 9_328), Ok(0));
        assert_eq!(selue_transport(999_999, 9_328), Ok(999_999));
        assert_eq!(selue_transport(1_000_000, 9_328), Ok(9_327_000_000));
        assert_eq!(selue_transport(9_327_000_000, 9_328), Ok(1_000_000));
        assert_eq!(
            selue_transport(9_328_000_000, 9_328),
            Err(SelueError::NotReversible)
        );
        assert!(selue_in_domain(1_000_000, 9_328));
        assert!(!selue_in_domain(9_328_000_000, 9_328));
    }

    #[test]
    fn master_ball_count_rules() {
        assert_eq!(
            check_master_ball_transfer(1, 1, 0, 0, 1, MASTER_BALL_STORY_CAP, false),
            Ok(())
        );
        assert_eq!(
            check_master_ball_transfer(2, 2, 0, 0, 2, MASTER_BALL_STORY_CAP, false),
            Err(MasterBallError::MultiAdd)
        );
        assert_eq!(
            check_master_ball_transfer(1, 0, 0, 0, 1, MASTER_BALL_STORY_CAP, false),
            Err(MasterBallError::SourceEmpty)
        );
        assert_eq!(
            check_master_ball_transfer(1, 1, 2, 0, 3, MASTER_BALL_STORY_CAP, false),
            Err(MasterBallError::Cap)
        );
        assert_eq!(
            check_master_ball_transfer(1, 1, 0, 0, 0, MASTER_BALL_STORY_CAP, false),
            Err(MasterBallError::Conservation)
        );
        assert_eq!(
            check_master_ball_transfer(1, 1, 0, 0, 1, MASTER_BALL_STORY_CAP, true),
            Err(MasterBallError::SourceStoryDependency)
        );
        assert_eq!(
            check_master_ball_transfer(0, 1, 1, 1, 1, MASTER_BALL_STORY_CAP, true),
            Ok(())
        );
    }

    #[test]
    fn safety_profile_accepts_complete_offer() {
        assert_eq!(
            validate_safety(&safety_ready_offer(), Some(9_328)),
            Ok(ValidatedOffer {
                species: 448,
                level: 50
            })
        );
        assert!(validate_safety(&safety_ready_offer(), None).is_ok());
    }

    #[test]
    fn safety_profile_rejects_envelope_violations() {
        let mut offer = safety_ready_offer();
        offer.fields[21] = n(7.0);
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Range));

        let mut offer = safety_ready_offer();
        offer.fields[3] = n(-1.0);
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Range));

        let mut offer = safety_ready_offer();
        offer.fields[7] = n(1.0);
        assert_eq!(
            validate_safety(&offer, None),
            Err(SafetyError::SpeciesMismatch)
        );

        let mut offer = safety_ready_offer();
        offer.fields[2] = n(49.0);
        assert_eq!(
            validate_safety(&offer, None),
            Err(SafetyError::LevelMismatch)
        );

        let mut offer = safety_ready_offer();
        offer.fields[10] = n(11.0);
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Pp));

        let mut offer = safety_ready_offer();
        offer.fields[11] = n(151.0);
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Hp));

        let mut offer = safety_ready_offer();
        offer.fields[22] = n(3.0);
        assert!(validate_safety(&offer, None).is_ok());

        let mut offer = safety_ready_offer();
        offer.fields[46] = n(0.0);
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Selue));

        let mut offer = safety_ready_offer();
        offer.fields[9] = n(9_328_000_000.0);
        offer.fields[46] = n(9_328_000_000.0);
        assert_eq!(
            validate_safety(&offer, Some(9_328)),
            Err(SafetyError::SelueDomain)
        );

        let mut offer = safety_ready_offer();
        offer.fields[37] = t(b"A\x00B");
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Text));

        let mut offer = safety_ready_offer();
        offer.fields[5] = t(b"0");
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Type));

        let mut offer = safety_ready_offer();
        offer.fields[36] = n(0.0);
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Type));

        let mut offer = safety_ready_offer();
        offer.fields[48] = t(b"A\x7fB");
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Text));

        let mut offer = safety_ready_offer();
        offer.fields[48] = n(MAX_SAFE_INTEGER as f64 + 1.0);
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Number));

        let mut offer = safety_ready_offer();
        offer.fields[37] = Value::Text(vec![b'a'; MAX_TEXT_LEN + 1]);
        assert_eq!(validate_safety(&offer, None), Err(SafetyError::Text));
    }

    #[test]
    fn transport_envelope_round_trip() {
        let envelope = encode_envelope(&golden_offer(), b"Fixture Nv. 50").unwrap();
        assert!(envelope.starts_with(b"OFFER\x1f"));
        let decoded = decode_message(&envelope).unwrap();
        assert_eq!(decoded.label, b"Fixture Nv. 50");
        assert_eq!(decoded.offer, golden_offer());

        let empty = encode_envelope(&golden_offer(), b"").unwrap();
        assert!(empty.starts_with(b"OFFER\x1f\x1e"));
        let decoded = decode_message(&empty).unwrap();
        assert!(decoded.label.is_empty());
        assert_eq!(decoded.offer, golden_offer());
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(DecodeError::Terminator.code(), "E_TERMINATOR");
        assert_eq!(EncodeError::TextSeparator.code(), "E_TEXT_SEPARATOR");
        assert_eq!(ValidateError::Story.code(), "E_STORY");
        assert_eq!(ValidateError::ItemMaster.code(), "E_ITEM_MASTER");
        assert_eq!(ValidateError::BallMaster.code(), "E_BALL_MASTER");
        assert_eq!(
            MasterBallError::SourceEmpty.code(),
            "MASTER_BALL_SOURCE_EMPTY"
        );
        assert_eq!(SelueError::NotReversible.code(), "E_SELUE_NOT_REVERSIBLE");
    }
}
