use std::collections::{HashMap, VecDeque};

pub const MAX_LINE_BYTES: usize = 65_536;
pub const MAX_PARTS_SEGMENTS: usize = 10;
pub const MAX_LIST_ENTRIES: usize = 250;
pub const MAX_LIST_TEXT_BYTES: usize = 1024;
/// Longest remote player name kept from a `WP` line, in bytes.
pub const MAX_REMOTE_NAME_BYTES: usize = 128;
/// Alias kept for callers that treat remote names as the generic world name.
pub const MAX_NAME_BYTES: usize = MAX_REMOTE_NAME_BYTES;
/// Alias used by the list/render adapters.
pub const MAX_WORLD_NAME_BYTES: usize = MAX_REMOTE_NAME_BYTES;
/// Maximum number of remote peers retained at once.
///
/// This is a world-state bound, not the `WLIST` page size; a large world may
/// legitimately contain thousands of participants while still exposing only a
/// 250-entry list page.
pub const MAX_REMOTES: usize = 4096;
/// Alias used by the online host when it mirrors [`WorldState`].
pub const MAX_REMOTE_ENTRIES: usize = MAX_REMOTES;
/// Smallest world coordinate accepted from the network (signed 32-bit).
pub const MIN_COORDINATE: f64 = i32::MIN as f64;
/// Largest world coordinate accepted from the network (signed 32-bit).
pub const MAX_COORDINATE: f64 = i32::MAX as f64;
/// Alias used by parsers that speak about positions rather than coordinates.
pub const MAX_POSITION: f64 = MAX_COORDINATE;
/// Alias used by parsers that speak about positions rather than coordinates.
pub const MIN_POSITION: f64 = MIN_COORDINATE;
pub const HIDDEN_POSITION_X: f64 = -500.0;
pub const BATTLE_PLACEHOLDER_SPRITE: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    OverlongLine,
    EmbeddedControl,
    UnknownCommand,
    MissingField,
    ExtraField,
    MalformedNumeric,
    MalformedParts,
    MalformedText,
    InvalidSlot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineEvent {
    Line(Vec<u8>),
    Overlong,
}

#[derive(Debug, Default)]
pub struct LineBuffer {
    data: Vec<u8>,
    discarding: bool,
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, input: &[u8]) -> Vec<LineEvent> {
        let mut events = Vec::new();
        for &byte in input {
            if byte == b'\n' {
                if self.discarding {
                    self.discarding = false;
                } else {
                    let mut line = std::mem::take(&mut self.data);
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    if line.len() > MAX_LINE_BYTES {
                        events.push(LineEvent::Overlong);
                    } else {
                        events.push(LineEvent::Line(line));
                    }
                }
            } else if self.discarding {
                continue;
            } else {
                self.data.push(byte);
                let length = self.data.len();
                if length > MAX_LINE_BYTES + 1
                    || (length == MAX_LINE_BYTES + 1 && byte != b'\r')
                {
                    self.data.clear();
                    self.data.shrink_to(256);
                    self.discarding = true;
                    events.push(LineEvent::Overlong);
                }
            }
        }
        events
    }

    pub fn pending_len(&self) -> usize {
        self.data.len()
    }

    pub fn reset(&mut self) {
        self.data.clear();
        self.discarding = false;
    }
}

fn prepare_line(line: &[u8]) -> Result<&[u8], ParseError> {
    let mut line = line;
    if line.last() == Some(&b'\r') {
        line = &line[..line.len() - 1];
    }
    if line.len() > MAX_LINE_BYTES {
        return Err(ParseError::OverlongLine);
    }
    if line
        .iter()
        .any(|&byte| byte == b'\r' || byte == b'\n' || byte == 0)
    {
        return Err(ParseError::EmbeddedControl);
    }
    Ok(line)
}

fn command_rest<'a>(line: &'a [u8], command: &[u8]) -> Result<&'a [u8], ParseError> {
    if line.len() < command.len() || &line[..command.len()] != command {
        return Err(ParseError::UnknownCommand);
    }
    let rest = &line[command.len()..];
    if rest.is_empty() {
        return Ok(rest);
    }
    if rest[0] != b' ' {
        return Err(ParseError::UnknownCommand);
    }
    Ok(rest)
}

fn command_name(line: &[u8]) -> &[u8] {
    match line.iter().position(|&byte| byte == b' ') {
        Some(index) => &line[..index],
        None => line,
    }
}

fn skip_spaces<'a>(cursor: &mut &'a [u8]) {
    while cursor.first() == Some(&b' ') {
        *cursor = &cursor[1..];
    }
}

fn take_token<'a>(cursor: &mut &'a [u8]) -> Option<&'a [u8]> {
    skip_spaces(cursor);
    if cursor.is_empty() {
        return None;
    }
    let end = cursor
        .iter()
        .position(|&byte| byte == b' ')
        .unwrap_or(cursor.len());
    let token = &cursor[..end];
    *cursor = &cursor[end..];
    Some(token)
}

fn parse_u64_digits(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || (bytes.len() > 1 && bytes[0] == b'0') {
        return None;
    }
    let mut value = 0u64;
    for &byte in bytes {
        let digit = byte.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(digit as u64)?;
    }
    Some(value)
}

fn parse_u64_strict(bytes: &[u8]) -> Option<u64> {
    parse_u64_digits(bytes)
}

fn parse_i64_strict(bytes: &[u8]) -> Option<i64> {
    let (negative, digits) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    let magnitude = parse_u64_digits(digits)?;
    if negative {
        if magnitude == (1u64 << 63) {
            Some(i64::MIN)
        } else if magnitude <= i64::MAX as u64 {
            Some(-(magnitude as i64))
        } else {
            None
        }
    } else if magnitude <= i64::MAX as u64 {
        Some(magnitude as i64)
    } else {
        None
    }
}

fn parse_i32_strict(bytes: &[u8]) -> Option<i32> {
    let value = parse_i64_strict(bytes)?;
    i32::try_from(value).ok()
}

fn parse_real_strict(bytes: &[u8]) -> Option<f64> {
    let digits = match bytes.first() {
        Some(b'-') => &bytes[1..],
        _ => bytes,
    };
    if digits.is_empty() {
        return None;
    }
    let mut parts = digits.split(|&byte| byte == b'.');
    let integer = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some() {
        return None;
    }
    if integer.is_empty() || (integer.len() > 1 && integer[0] == b'0') {
        return None;
    }
    if !integer.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if let Some(fraction) = fraction {
        if fraction.is_empty() || !fraction.iter().all(u8::is_ascii_digit) {
            return None;
        }
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let value: f64 = text.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(value)
}

/// Whether a coordinate is finite and inside the signed 32-bit world range.
pub fn valid_coordinate(value: f64) -> bool {
    value.is_finite() && (MIN_COORDINATE..=MAX_COORDINATE).contains(&value)
}

/// Whether a remote position can be stored safely.
pub fn valid_position(x: f64, y: f64) -> bool {
    valid_coordinate(x) && valid_coordinate(y)
}

/// Whether the primary rendered part still lands inside the world range.
///
/// Part offsets and scales are reals and are not limited to the base position
/// range, but the final rendered origin must be representable consistently by
/// every transport (the JSON bridge also composes `x + dx`/`y + dy`).
pub fn valid_rendered_position(x: f64, y: f64, parts: &Parts) -> bool {
    if !valid_position(x, y) {
        return false;
    }
    match parts.primary_player() {
        Some(player) => valid_position(x + player.dx, y + player.dy),
        None => true,
    }
}

fn parse_coordinate_strict(bytes: &[u8]) -> Option<f64> {
    let value = parse_real_strict(bytes)?;
    valid_coordinate(value).then_some(value)
}

fn is_name_control(byte: u8) -> bool {
    byte < 0x20 || byte == 0x7f
}

/// Whether the bytes contain no C0 controls or DEL.
///
/// Stored names are sanitized with [`sanitize_name_bytes`]; this helper is
/// kept for callers that want to test an already-truncated prefix.
pub fn valid_remote_name(bytes: &[u8]) -> bool {
    !bytes.iter().any(|&byte| is_name_control(byte))
}

/// Truncates a remote name to [`MAX_REMOTE_NAME_BYTES`] without splitting a
/// valid UTF-8 sequence. Non-UTF-8 bytes are preserved as opaque name data.
pub fn limit_name_bytes(bytes: &[u8]) -> &[u8] {
    if bytes.len() <= MAX_REMOTE_NAME_BYTES {
        return bytes;
    }
    let end = MAX_REMOTE_NAME_BYTES;
    if end == bytes.len() {
        return bytes;
    }
    // If the first discarded byte is a UTF-8 continuation byte, the cut would
    // split the sequence that owns it. Move the cut back to that sequence's
    // lead byte, but only when the lead is a plausible UTF-8 start.
    if bytes[end] & 0xc0 == 0x80 {
        let mut lead = end;
        while lead > 0 && bytes[lead] & 0xc0 == 0x80 {
            lead -= 1;
        }
        let expected = match bytes[lead] {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 0,
        };
        if expected > 0 && lead + expected > end {
            return &bytes[..lead];
        }
    }
    &bytes[..end]
}

/// Truncates and removes C0 controls/DEL from a remote display name.
pub fn sanitize_name_bytes(bytes: &[u8]) -> Vec<u8> {
    limit_name_bytes(bytes)
        .iter()
        .copied()
        .filter(|&byte| !is_name_control(byte))
        .collect()
}

fn valid_parts_token(token: &[u8]) -> bool {
    !token.is_empty() && token.iter().all(|&byte| (0x21..=0x7e).contains(&byte))
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerPart {
    pub sprite: u16,
    pub frame: i64,
    pub dx: f64,
    pub dy: f64,
    pub xs: f64,
    pub ys: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PokemonPart {
    pub species: i64,
    pub facing: i64,
    pub frame: i64,
    pub dx: f64,
    pub dy: f64,
    pub xs: f64,
    pub ys: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Player(PlayerPart),
    Pokemon(PokemonPart),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parts {
    pub segments: Vec<Part>,
    pub battle: bool,
}

impl Parts {
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn has_visible_part(&self) -> bool {
        !self.segments.is_empty()
    }

    pub fn primary_player(&self) -> Option<&PlayerPart> {
        self.segments.iter().find_map(|part| match part {
            Part::Player(player) => Some(player),
            Part::Pokemon(_) => None,
        })
    }

    pub fn players(&self) -> impl Iterator<Item = &PlayerPart> {
        self.segments.iter().filter_map(|part| match part {
            Part::Player(player) => Some(player),
            Part::Pokemon(_) => None,
        })
    }

    pub fn pokemon(&self) -> impl Iterator<Item = &PokemonPart> {
        self.segments.iter().filter_map(|part| match part {
            Part::Pokemon(pokemon) => Some(pokemon),
            Part::Player(_) => None,
        })
    }
}

fn parse_player_segment(fields: &[&[u8]]) -> Option<PlayerPart> {
    if fields.len() != 7 {
        return None;
    }
    let sprite = parse_i64_strict(fields[1])?;
    if !(0..=3856).contains(&sprite) {
        return None;
    }
    Some(PlayerPart {
        sprite: sprite as u16,
        frame: parse_i64_strict(fields[2])?,
        dx: parse_real_strict(fields[3])?,
        dy: parse_real_strict(fields[4])?,
        xs: parse_real_strict(fields[5])?,
        ys: parse_real_strict(fields[6])?,
    })
}

fn parse_pokemon_segment(fields: &[&[u8]]) -> Option<PokemonPart> {
    if fields.len() != 8 {
        return None;
    }
    Some(PokemonPart {
        species: parse_i64_strict(fields[1])?,
        facing: parse_i64_strict(fields[2])?,
        frame: parse_i64_strict(fields[3])?,
        dx: parse_real_strict(fields[4])?,
        dy: parse_real_strict(fields[5])?,
        xs: parse_real_strict(fields[6])?,
        ys: parse_real_strict(fields[7])?,
    })
}

pub fn parse_parts(token: &[u8]) -> Parts {
    let mut parts = Parts::default();
    for segment in token.split(|&byte| byte == b';').take(MAX_PARTS_SEGMENTS) {
        if segment.is_empty() {
            continue;
        }
        let fields: Vec<&[u8]> = segment.split(|&byte| byte == b',').collect();
        let kind = fields[0];
        if kind == b"S" {
            if let Some(player) = parse_player_segment(&fields) {
                parts.segments.push(Part::Player(player));
            }
        } else if kind == b"P" {
            if let Some(pokemon) = parse_pokemon_segment(&fields) {
                parts.segments.push(Part::Pokemon(pokemon));
            }
        } else if kind == b"B" {
            parts.battle = true;
        }
    }
    parts
}

#[derive(Debug, Clone, PartialEq)]
pub struct WpMessage {
    pub id: u64,
    pub room: i32,
    pub x: f64,
    pub y: f64,
    pub parts: Parts,
    pub parts_token: Vec<u8>,
    pub name: Vec<u8>,
    pub raw_remainder: Vec<u8>,
}

pub fn parse_wp(line: &[u8]) -> Result<WpMessage, ParseError> {
    let line = prepare_line(line)?;
    let rest = command_rest(line, b"WP")?;
    let mut cursor = rest;
    let id_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;
    let room_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;
    let x_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;
    let y_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;
    let parts_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;

    let id = parse_u64_strict(id_token).ok_or(ParseError::MalformedNumeric)?;
    let room = parse_i32_strict(room_token).ok_or(ParseError::MalformedNumeric)?;
    let x = parse_coordinate_strict(x_token).ok_or(ParseError::MalformedNumeric)?;
    let y = parse_coordinate_strict(y_token).ok_or(ParseError::MalformedNumeric)?;
    if !valid_parts_token(parts_token) {
        return Err(ParseError::MalformedParts);
    }

    skip_spaces(&mut cursor);
    let name = sanitize_name_bytes(cursor);
    let parts = parse_parts(parts_token);
    Ok(WpMessage {
        id,
        room,
        x,
        y,
        parts,
        parts_token: parts_token.to_vec(),
        name,
        raw_remainder: line[2..].to_vec(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct WdelMessage {
    pub id: u64,
    pub raw_remainder: Vec<u8>,
}

pub fn parse_wdel(line: &[u8]) -> Result<WdelMessage, ParseError> {
    let line = prepare_line(line)?;
    let rest = command_rest(line, b"WDEL")?;
    let mut cursor = rest;
    let id_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;
    let id = parse_u64_strict(id_token).ok_or(ParseError::MalformedNumeric)?;
    skip_spaces(&mut cursor);
    if !cursor.is_empty() {
        return Err(ParseError::ExtraField);
    }
    Ok(WdelMessage {
        id,
        raw_remainder: line[4..].to_vec(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct WposMessage {
    pub room: i32,
    pub x: f64,
    pub y: f64,
    pub parts: Parts,
    pub parts_token: Option<Vec<u8>>,
}

pub fn parse_wpos(line: &[u8]) -> Result<WposMessage, ParseError> {
    let line = prepare_line(line)?;
    let rest = command_rest(line, b"WPOS")?;
    let mut cursor = rest;
    let room_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;
    let x_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;
    let y_token = take_token(&mut cursor).ok_or(ParseError::MissingField)?;

    let room = parse_i32_strict(room_token).ok_or(ParseError::MalformedNumeric)?;
    let x = parse_coordinate_strict(x_token).ok_or(ParseError::MalformedNumeric)?;
    let y = parse_coordinate_strict(y_token).ok_or(ParseError::MalformedNumeric)?;

    skip_spaces(&mut cursor);
    if cursor.is_empty() {
        return Ok(WposMessage {
            room,
            x,
            y,
            parts: Parts::default(),
            parts_token: None,
        });
    }

    let token = take_token(&mut cursor).unwrap_or_default();
    if !valid_parts_token(token) {
        return Err(ParseError::MalformedParts);
    }
    skip_spaces(&mut cursor);
    if !cursor.is_empty() {
        return Err(ParseError::ExtraField);
    }
    Ok(WposMessage {
        room,
        x,
        y,
        parts: parse_parts(token),
        parts_token: Some(token.to_vec()),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    pub name: String,
    pub place: String,
    pub flags: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldList {
    pub total: i32,
    pub entries: Vec<ListEntry>,
}

fn trim_ascii_spaces(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start] == b' ' {
        start += 1;
    }
    while end > start && bytes[end - 1] == b' ' {
        end -= 1;
    }
    &bytes[start..end]
}

fn cap_list_text(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if text.len() <= MAX_LIST_TEXT_BYTES {
        return Some(text.to_string());
    }
    let mut end = MAX_LIST_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Some(text[..end].to_string())
}

pub fn parse_wlist(line: &[u8]) -> Result<WorldList, ParseError> {
    let line = prepare_line(line)?;
    let rest = command_rest(line, b"WLIST")?;
    let payload = if rest.first() == Some(&b' ') {
        &rest[1..]
    } else {
        rest
    };
    let mut components = payload.split(|&byte| byte == 0x1d);
    let total_component = components.next().unwrap_or(&[]);
    let total = parse_i32_strict(trim_ascii_spaces(total_component)).unwrap_or(0);

    let mut entries = Vec::new();
    for component in components {
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }
        if component.is_empty() {
            continue;
        }
        let mut fields = component.split(|&byte| byte == 0x1e);
        let name_bytes = fields.next().unwrap_or(&[]);
        if name_bytes.is_empty() {
            continue;
        }
        let place_bytes = fields.next().unwrap_or(&[]);
        let flags_bytes = fields.next().unwrap_or(&[]);
        let name = match cap_list_text(name_bytes) {
            Some(name) => name,
            None => continue,
        };
        let place = match cap_list_text(place_bytes) {
            Some(place) => place,
            None => continue,
        };
        let flags = match cap_list_text(flags_bytes) {
            Some(flags) => flags,
            None => continue,
        };
        entries.push(ListEntry { name, place, flags });
    }
    Ok(WorldList { total, entries })
}

fn sanitize_display(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_control() {
            out.extend(character.escape_default());
        } else {
            out.push(character);
        }
    }
    out
}

pub fn wlist_display(entry: &ListEntry) -> String {
    let mut display = format!(
        "{} - {}",
        sanitize_display(&entry.name),
        sanitize_display(&entry.place)
    );
    if !entry.flags.is_empty() {
        display.push_str(" (");
        display.push_str(&sanitize_display(&entry.flags));
        display.push(')');
    }
    display
}

#[derive(Debug, Clone, PartialEq)]
pub struct Remote {
    pub id: u64,
    pub room: i32,
    pub x: f64,
    pub y: f64,
    pub parts: Parts,
    pub name: Vec<u8>,
}

impl Remote {
    pub fn is_protocol_visible(&self) -> bool {
        self.x > HIDDEN_POSITION_X && self.parts.has_visible_part()
    }

    pub fn is_drawable_in_room(&self, room: i32) -> bool {
        self.is_protocol_visible() && self.room == room
    }
}

#[derive(Debug)]
pub struct WorldState {
    local_id: Option<u64>,
    remotes: HashMap<u64, Remote>,
    order: VecDeque<u64>,
    max_remotes: usize,
}

impl Default for WorldState {
    fn default() -> Self {
        Self::with_capacity(None, MAX_REMOTES)
    }
}

impl WorldState {
    pub fn new(local_id: Option<u64>) -> Self {
        Self::with_capacity(local_id, MAX_REMOTES)
    }

    /// Creates a world with an explicit remote capacity (useful for tests).
    pub fn with_capacity(local_id: Option<u64>, max_remotes: usize) -> Self {
        Self {
            local_id,
            remotes: HashMap::new(),
            order: VecDeque::new(),
            max_remotes: max_remotes.max(1),
        }
    }

    pub fn set_local_id(&mut self, local_id: Option<u64>) {
        self.local_id = local_id;
    }

    pub fn local_id(&self) -> Option<u64> {
        self.local_id
    }

    pub fn get(&self, id: u64) -> Option<&Remote> {
        self.remotes.get(&id)
    }

    pub fn remotes(&self) -> impl Iterator<Item = &Remote> {
        self.remotes.values()
    }

    pub fn len(&self) -> usize {
        self.remotes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.remotes.is_empty()
    }

    /// Capacity in remote peers. The map never grows past this.
    pub fn capacity(&self) -> usize {
        self.max_remotes
    }

    fn touch(&mut self, id: u64) {
        self.order.retain(|&other| other != id);
        self.order.push_back(id);
    }

    /// Chooses the least-recently-used remote, preferring one in another room
    /// so idle peers elsewhere do not displace current-room participants.
    fn evict_victim(&mut self, incoming_room: i32) -> Option<u64> {
        let mut same_room = None;
        let mut victim = None;
        for &id in &self.order {
            let Some(remote) = self.remotes.get(&id) else { continue };
            if remote.room != incoming_room {
                victim = Some(id);
                break;
            }
            if same_room.is_none() {
                same_room = Some(id);
            }
        }
        let victim = victim
            .or(same_room)
            .or_else(|| self.remotes.keys().next().copied())?;
        self.remotes.remove(&victim);
        self.order.retain(|&other| other != victim);
        Some(victim)
    }

    fn upsert(
        &mut self,
        id: u64,
        room: i32,
        x: f64,
        y: f64,
        parts: Parts,
        name: Vec<u8>,
    ) -> (bool, Option<u64>) {
        if let Some(remote) = self.remotes.get_mut(&id) {
            remote.room = room;
            remote.x = x;
            remote.y = y;
            remote.parts = parts;
            remote.name = name;
            self.touch(id);
            return (true, None);
        }
        let evicted = if self.remotes.len() >= self.max_remotes {
            self.evict_victim(room)
        } else {
            None
        };
        self.remotes.insert(
            id,
            Remote {
                id,
                room,
                x,
                y,
                parts,
                name,
            },
        );
        self.order.push_back(id);
        (true, evicted)
    }

    fn apply_wp_inner(&mut self, message: &WpMessage) -> (bool, Option<u64>) {
        let mut parts = message.parts.clone();
        parts.segments.truncate(MAX_PARTS_SEGMENTS);
        if self.local_id == Some(message.id)
            || !valid_rendered_position(message.x, message.y, &parts)
        {
            return (false, None);
        }
        let name = sanitize_name_bytes(&message.name);
        self.upsert(message.id, message.room, message.x, message.y, parts, name)
    }

    /// Applies a position-only state update used by transports that already
    /// decoded the richer rendering fields.
    pub fn apply_remote_state(
        &mut self,
        id: u64,
        room: i32,
        x: f64,
        y: f64,
        name: &[u8],
    ) -> (bool, Option<u64>) {
        if self.local_id == Some(id) || !valid_position(x, y) {
            return (false, None);
        }
        let name = sanitize_name_bytes(name);
        self.upsert(id, room, x, y, Parts::default(), name)
    }

    /// Applies a remote update and returns whether it changed the visible state.
    pub fn apply_wp(&mut self, message: &WpMessage) -> bool {
        self.apply_wp_inner(message).0
    }

    /// Applies a remote update, returning the evicted id when the map was full.
    pub fn apply_wp_with_eviction(&mut self, message: &WpMessage) -> (bool, Option<u64>) {
        self.apply_wp_inner(message)
    }

    pub fn apply_wp_line(&mut self, line: &[u8]) -> Result<bool, ParseError> {
        let message = parse_wp(line)?;
        Ok(self.apply_wp(&message))
    }

    pub fn apply_wdel(&mut self, id: u64) -> bool {
        let removed = self.remotes.remove(&id).is_some();
        if removed {
            self.order.retain(|&other| other != id);
        }
        removed
    }

    pub fn apply_wdel_line(&mut self, line: &[u8]) -> Result<bool, ParseError> {
        let message = parse_wdel(line)?;
        Ok(self.apply_wdel(message.id))
    }

    pub fn clear(&mut self) {
        self.remotes.clear();
        self.order.clear();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorldMessage {
    Wp(WpMessage),
    Wdel(WdelMessage),
    Wpos(WposMessage),
    Wlist(WorldList),
    Unknown,
}

pub fn parse_world_line(line: &[u8]) -> Result<WorldMessage, ParseError> {
    let line = prepare_line(line)?;
    match command_name(line) {
        b"WP" => Ok(WorldMessage::Wp(parse_wp(line)?)),
        b"WDEL" => Ok(WorldMessage::Wdel(parse_wdel(line)?)),
        b"WPOS" => Ok(WorldMessage::Wpos(parse_wpos(line)?)),
        b"WLIST" => Ok(WorldMessage::Wlist(parse_wlist(line)?)),
        _ => Ok(WorldMessage::Unknown),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverworldLoader {
    Simple,
    Cached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurmyLocation {
    A,
    B,
    C,
}

impl Default for BurmyLocation {
    fn default() -> Self {
        Self::A
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeatherState {
    pub controller_823: bool,
    pub controller_824: bool,
    pub controller_825: bool,
}

impl WeatherState {
    pub fn any(&self) -> bool {
        self.controller_823 || self.controller_824 || self.controller_825
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverworldContext {
    pub loader: OverworldLoader,
    pub in_battle: bool,
    pub is_day: bool,
    pub weather: WeatherState,
    pub burmy_location: BurmyLocation,
}

impl Default for OverworldContext {
    fn default() -> Self {
        Self {
            loader: OverworldLoader::Cached,
            in_battle: false,
            is_day: false,
            weather: WeatherState::default(),
            burmy_location: BurmyLocation::default(),
        }
    }
}

impl OverworldContext {
    pub fn simple() -> Self {
        Self {
            loader: OverworldLoader::Simple,
            ..Self::default()
        }
    }

    pub fn cached() -> Self {
        Self::default()
    }

    fn day_or_weather(&self) -> bool {
        self.is_day || self.weather.any()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleContext {
    pub is_day: bool,
    pub weather: WeatherState,
    pub burmy_location: BurmyLocation,
}

impl Default for BattleContext {
    fn default() -> Self {
        Self {
            is_day: false,
            weather: WeatherState::default(),
            burmy_location: BurmyLocation::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverworldSprite {
    None,
    File(String),
    Placeholder(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BattleSprite {
    File(String),
    Mirrored { path: String, source_slot: i32 },
    Placeholder { missing: String, sprite: u8 },
}

pub fn is_shiny(id: i64) -> bool {
    (1000 < id && id <= 2000)
        || (3000 < id && id <= 4000)
        || (101000 < id && id <= 102000)
        || (103000 < id && id <= 104000)
        || (151000 < id && id <= 152000)
        || (153000 < id && id <= 154000)
        || (201000 < id && id <= 202000)
        || (203000 < id && id <= 204000)
}

fn remap_3171(id: i64) -> i64 {
    if (3171..=3226).contains(&id) {
        let remainder = id % 7;
        if remainder == 0 {
            3139
        } else {
            3164 + remainder
        }
    } else {
        id
    }
}

fn alias_egg(id: i64, loader: OverworldLoader) -> i64 {
    if (-628..=-601).contains(&id) {
        return -201;
    }
    if (-717..=-700).contains(&id) || id == -807 {
        return -493;
    }
    if loader == OverworldLoader::Cached {
        if matches!(id, -995 | -996 | -2017 | -2018) {
            return -834;
        }
        if (-1628..=-1601).contains(&id) {
            return -1201;
        }
        if (-1717..=-1700).contains(&id) || id == -1807 {
            return -1493;
        }
        if matches!(id, -1995 | -1996 | -3017 | -3018) {
            return -1834;
        }
    }
    id
}

fn burmy_id(base: i64, location: BurmyLocation) -> i64 {
    match location {
        BurmyLocation::A => base,
        BurmyLocation::B => base + 398,
        BurmyLocation::C => base + 399,
    }
}

pub fn resolve_overworld_id(id: i64, context: OverworldContext) -> i64 {
    let id = if id < 0 {
        alias_egg(id, context.loader)
    } else {
        id
    };
    if id <= 0 {
        return id;
    }
    let mut id = remap_3171(id);
    match context.loader {
        OverworldLoader::Simple => {
            if id == 421 {
                id = 809;
            }
            if id == 1421 {
                id = 1809;
            }
        }
        OverworldLoader::Cached => {
            if !context.in_battle {
                if context.day_or_weather() {
                    if id == 421 {
                        id = 809;
                    }
                    if id == 1421 {
                        id = 1809;
                    }
                }
                if id == 412 {
                    id = burmy_id(412, context.burmy_location);
                }
                if id == 1412 {
                    id = burmy_id(1412, context.burmy_location);
                }
            }
        }
    }
    id
}

pub fn overworld_asset_path(id: i64, context: OverworldContext) -> Option<String> {
    let id = resolve_overworld_id(id, context);
    if id == 0 {
        return None;
    }
    if id < 0 {
        let magnitude = id.checked_abs()?;
        if is_shiny(magnitude) {
            Some(format!("huevo_shiny/{}.gif", magnitude))
        } else {
            Some(format!("huevo/{}.gif", magnitude))
        }
    } else if is_shiny(id) {
        Some(format!("pkmn_shiny/{}.gif", id))
    } else {
        Some(format!("pkmn/{}.gif", id))
    }
}

pub fn overworld_sprite<F>(
    id: i64,
    context: OverworldContext,
    exists: F,
) -> OverworldSprite
where
    F: Fn(&str) -> bool,
{
    let resolved = resolve_overworld_id(id, context);
    let path = match overworld_asset_path(id, context) {
        Some(path) => path,
        None => return OverworldSprite::None,
    };
    if exists(&path) {
        OverworldSprite::File(path)
    } else if resolved < 0 {
        OverworldSprite::Placeholder("huevo/0.gif".to_string())
    } else {
        OverworldSprite::Placeholder("pkmn/0.gif".to_string())
    }
}

pub fn resolve_battle_id(id: i64, context: BattleContext) -> i64 {
    let mut id = id;
    if id == 493 {
        id = 700;
    }
    if id == 1493 {
        id = 1700;
    }
    if id == 351 {
        if context.weather.controller_823 {
            id = 823;
        }
        if context.weather.controller_824 {
            id = 824;
        }
        if context.weather.controller_825 {
            id = 825;
        }
    }
    if id == 1351 {
        if context.weather.controller_823 {
            id = 1823;
        }
        if context.weather.controller_824 {
            id = 1824;
        }
        if context.weather.controller_825 {
            id = 1825;
        }
    }
    if context.is_day || context.weather.any() {
        if id == 421 {
            id = 809;
        }
        if id == 1421 {
            id = 1809;
        }
    }
    if id == 412 {
        id = burmy_id(412, context.burmy_location);
    }
    if id == 1412 {
        id = burmy_id(1412, context.burmy_location);
    }
    remap_3171(id)
}

pub fn battle_slot_label(slot: i32) -> Option<(char, bool)> {
    if slot <= 0 {
        return None;
    }
    let (slot, alternate) = if slot > 20 {
        (slot - 20, true)
    } else {
        (slot, false)
    };
    let label = match slot {
        1..=9 => (b'0' + slot as u8) as char,
        10 => 'A',
        11 => 'H',
        12 => 'S',
        13 => 'B',
        14 => 'C',
        15 => 'D',
        _ => return None,
    };
    Some((label, alternate))
}

pub fn battle_filename(id: i64, slot: i32) -> Option<String> {
    let (label, alternate) = battle_slot_label(slot)?;
    let stem = format!("P{}_{}", id, label);
    if alternate {
        Some(format!("pkmnbattle/{}_ALT.gmspr", stem))
    } else {
        Some(format!("pkmnbattle/{}.gmspr", stem))
    }
}

fn required_battle_file<F>(path: String, exists: &F) -> BattleSprite
where
    F: Fn(&str) -> bool,
{
    if exists(&path) {
        BattleSprite::File(path)
    } else {
        BattleSprite::Placeholder {
            missing: path,
            sprite: BATTLE_PLACEHOLDER_SPRITE,
        }
    }
}

fn battle_base_sprite<F>(id: i64, base_slot: i32, exists: &F) -> BattleSprite
where
    F: Fn(&str) -> bool,
{
    let (label, _) = match battle_slot_label(base_slot) {
        Some(slot) => slot,
        None => {
            return BattleSprite::Placeholder {
                missing: String::new(),
                sprite: BATTLE_PLACEHOLDER_SPRITE,
            }
        }
    };
    if label == 'S' {
        let sleep_path = battle_filename(id, base_slot).unwrap_or_default();
        if exists(&sleep_path) {
            return BattleSprite::File(sleep_path);
        }
        let hurt_path = battle_filename(id, 11).unwrap_or_default();
        return required_battle_file(hurt_path, exists);
    }
    if matches!(label, '1' | '4' | '7') {
        let one_path = battle_filename(id, 1).unwrap_or_default();
        if !exists(&one_path) {
            let (mirror_slot, mirror_path) = match label {
                '1' => (3, battle_filename(id, 3).unwrap_or_default()),
                '4' => (6, battle_filename(id, 6).unwrap_or_default()),
                '7' => (9, battle_filename(id, 9).unwrap_or_default()),
                _ => unreachable!(),
            };
            if exists(&mirror_path) {
                return BattleSprite::Mirrored {
                    path: mirror_path,
                    source_slot: mirror_slot,
                };
            }
            return BattleSprite::Placeholder {
                missing: mirror_path,
                sprite: BATTLE_PLACEHOLDER_SPRITE,
            };
        }
    }
    required_battle_file(battle_filename(id, base_slot).unwrap_or_default(), exists)
}

pub fn battle_sprite<F>(
    id: i64,
    slot: i32,
    context: BattleContext,
    exists: F,
) -> Result<BattleSprite, ParseError>
where
    F: Fn(&str) -> bool,
{
    let (label, alternate) = battle_slot_label(slot).ok_or(ParseError::InvalidSlot)?;
    let resolved = resolve_battle_id(id, context);
    let base_slot = if alternate { slot - 20 } else { slot };
    let alternate_probe = alternate
        && matches!(label, '1' | '2' | '3' | '4' | '6' | '7' | '8' | '9' | 'A' | 'H');
    if alternate_probe {
        let alternate_path = battle_filename(resolved, slot).unwrap_or_default();
        if exists(&alternate_path) {
            return Ok(BattleSprite::File(alternate_path));
        }
    }
    Ok(battle_base_sprite(resolved, base_slot, &exists))
}

pub fn pokedex_battle_sprite<F>(id: i64, exists: F) -> BattleSprite
where
    F: Fn(&str) -> bool,
{
    let path = battle_filename(id, 2).unwrap_or_default();
    required_battle_file(path, &exists)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_wp_str(line: &str) -> WpMessage {
        parse_wp(line.as_bytes()).unwrap()
    }

    fn exists_in<'a>(paths: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |path| paths.contains(&path)
    }

    #[test]
    fn line_buffer_splits_crlf_and_lf() {
        let mut buffer = LineBuffer::new();
        let events = buffer.feed(b"WP 1 2\r\nWDEL 3\n\n");
        assert_eq!(
            events,
            vec![
                LineEvent::Line(b"WP 1 2".to_vec()),
                LineEvent::Line(b"WDEL 3".to_vec()),
                LineEvent::Line(Vec::new())
            ]
        );
    }

    #[test]
    fn line_buffer_accepts_exact_limit() {
        let mut buffer = LineBuffer::new();
        let mut input = vec![b'A'; MAX_LINE_BYTES];
        input.push(b'\n');
        let events = buffer.feed(&input);
        match &events[0] {
            LineEvent::Line(line) => assert_eq!(line.len(), MAX_LINE_BYTES),
            LineEvent::Overlong => panic!("exact limit rejected"),
        }

        let mut buffer = LineBuffer::new();
        let mut input = vec![b'A'; MAX_LINE_BYTES];
        input.extend_from_slice(b"\r\n");
        let events = buffer.feed(&input);
        match &events[0] {
            LineEvent::Line(line) => assert_eq!(line.len(), MAX_LINE_BYTES),
            LineEvent::Overlong => panic!("exact CRLF limit rejected"),
        }
    }

    #[test]
    fn line_buffer_rejects_overlong_and_resynchronizes() {
        let mut buffer = LineBuffer::new();
        let mut input = vec![b'A'; MAX_LINE_BYTES + 1];
        input.push(b'\n');
        input.extend_from_slice(b"WDEL 9\n");
        let mut events = Vec::new();
        for chunk in input.chunks(997) {
            events.extend(buffer.feed(chunk));
        }
        assert_eq!(events[0], LineEvent::Overlong);
        assert_eq!(events[1], LineEvent::Line(b"WDEL 9".to_vec()));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn wp_parses_full_example() {
        let message = parse_wp_str(
            "WP 42 7 128 96 S,25,2,0,0,1,1;P,25,4,3,8,12,1,1;B Ada Lovelace",
        );
        assert_eq!(message.id, 42);
        assert_eq!(message.room, 7);
        assert_eq!(message.x, 128.0);
        assert_eq!(message.y, 96.0);
        assert_eq!(message.parts.primary_player().unwrap().sprite, 25);
        assert_eq!(message.parts.primary_player().unwrap().frame, 2);
        assert_eq!(message.parts.pokemon().count(), 1);
        assert!(message.parts.battle);
        assert_eq!(message.name, b"Ada Lovelace".to_vec());
        assert_eq!(
            message.raw_remainder,
            b" 42 7 128 96 S,25,2,0,0,1,1;P,25,4,3,8,12,1,1;B Ada Lovelace".to_vec()
        );
    }

    #[test]
    fn wp_tolerates_repeated_spaces_and_keeps_name_spaces() {
        let message = parse_wp_str(
            "WP  42   7  128.5 -96.25  S,25,2,0,0,1,1  Ada  Lovelace  ",
        );
        assert_eq!(message.id, 42);
        assert_eq!(message.room, 7);
        assert_eq!(message.x, 128.5);
        assert_eq!(message.y, -96.25);
        assert_eq!(message.name, b"Ada  Lovelace  ".to_vec());
    }

    #[test]
    fn wp_accepts_absent_name() {
        let message = parse_wp_str("WP 1 2 3 4 S,1,0,0,0,1,1");
        assert!(message.name.is_empty());
        let message = parse_wp_str("WP 1 2 3 4 S,1,0,0,0,1,1    ");
        assert!(message.name.is_empty());
    }

    #[test]
    fn wp_preserves_non_utf8_name_and_remainder_bytes() {
        let line = b"WP 1 2 3 4 S,1,0,0,0,1,1 \xff\xfe";
        let message = parse_wp(line).unwrap();
        assert_eq!(message.name, b"\xff\xfe".to_vec());
        assert_eq!(
            message.raw_remainder,
            b" 1 2 3 4 S,1,0,0,0,1,1 \xff\xfe".to_vec()
        );
    }

    #[test]
    fn wp_rejects_missing_and_bad_fixed_fields() {
        assert_eq!(
            parse_wp(b"WP 42 7 128 96"),
            Err(ParseError::MissingField)
        );
        assert_eq!(
            parse_wp(b"WP -1 7 128 96 S,1,0,0,0,1,1"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(
            parse_wp(b"WP 18446744073709551616 7 128 96 S,1,0,0,0,1,1"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(
            parse_wp(b"WP 1 2147483648 0 0 S,1,0,0,0,1,1"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(
            parse_wp(b"WP 1 7 1e3 0 S,1,0,0,0,1,1"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(
            parse_wp(b"WP 1 7 1,5 0 S,1,0,0,0,1,1"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(
            parse_wp(b"WP 1 7 01 0 S,1,0,0,0,1,1"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(
            parse_wp(b"WP 1 2 3 4  "),
            Err(ParseError::MissingField)
        );
        assert_eq!(
            parse_wp(b"WP 1 2 3 4 S,1,0,0,0,1,1\x7f"),
            Err(ParseError::MalformedParts)
        );
        assert_eq!(
            parse_wp(b"wp 1 2 3 4 S,1,0,0,0,1,1"),
            Err(ParseError::UnknownCommand)
        );
    }

    #[test]
    fn wp_rejects_embedded_controls_and_overlong_lines() {
        assert_eq!(
            parse_wp(b"WP 1 2 3 4 S,1,0,0,0,1,1 A\rB"),
            Err(ParseError::EmbeddedControl)
        );
        assert_eq!(
            parse_wp(b"WP 1 2 3 4 S,1,0,0,0,1,1 A\0B"),
            Err(ParseError::EmbeddedControl)
        );
        let line = vec![b'A'; MAX_LINE_BYTES + 1];
        assert_eq!(parse_wp(&line), Err(ParseError::OverlongLine));
    }

    #[test]
    fn parts_decode_player_pokemon_and_battle() {
        let parts = parse_parts(b"S,25,2,0,0,1,1;P,25,4,3,8,12,1,1;B");
        assert_eq!(parts.segments.len(), 2);
        let player = parts.primary_player().unwrap();
        assert_eq!(player.sprite, 25);
        assert_eq!(player.frame, 2);
        assert_eq!(player.dx, 0.0);
        assert_eq!(player.xs, 1.0);
        let pokemon = parts.pokemon().next().unwrap();
        assert_eq!(pokemon.species, 25);
        assert_eq!(pokemon.facing, 4);
        assert_eq!(pokemon.frame, 3);
        assert_eq!(pokemon.dx, 8.0);
        assert_eq!(pokemon.dy, 12.0);
        assert!(parts.battle);
    }

    #[test]
    fn parts_hidden_markers_are_empty() {
        assert!(parse_parts(b"-").is_empty());
        assert!(parse_parts(b"").is_empty());
        assert!(parse_parts(b";;;").is_empty());
        assert!(!parse_parts(b"-").battle);
    }

    #[test]
    fn parts_sprite_bounds_and_field_counts() {
        assert_eq!(parse_parts(b"S,0,0,0,0,1,1").primary_player().unwrap().sprite, 0);
        assert_eq!(
            parse_parts(b"S,3856,0,0,0,1,1").primary_player().unwrap().sprite,
            3856
        );
        assert!(parse_parts(b"S,-1,0,0,0,1,1").is_empty());
        assert!(parse_parts(b"S,3857,0,0,0,1,1").is_empty());
        assert!(parse_parts(b"S,1,0,0,0,1").is_empty());
        assert!(parse_parts(b"S,1,0,0,0,1,1,1").is_empty());
        assert!(parse_parts(b"P,25,2,0,0,0,1").is_empty());
        assert!(parse_parts(b"P,25,2,0,0,0,1,1,1").is_empty());
        assert!(parse_parts(b"Q,1,2").is_empty());
        assert!(parse_parts(b"s,1,0,0,0,1,1").is_empty());
    }

    #[test]
    fn parts_skip_bad_segments_independently() {
        let parts = parse_parts(b"S,9999,0,0,0,1,1;Q,1;S,3,0,0,0,1,1;P,25,2,0,0,0");
        assert_eq!(parts.segments.len(), 1);
        assert_eq!(parts.primary_player().unwrap().sprite, 3);
    }

    #[test]
    fn parts_first_valid_player_is_primary() {
        let parts = parse_parts(b"S,7,1,0,0,1,1;S,9,2,0,0,1,1");
        assert_eq!(parts.players().count(), 2);
        assert_eq!(parts.primary_player().unwrap().sprite, 7);
        assert_eq!(parts.primary_player().unwrap().frame, 1);
    }

    #[test]
    fn parts_battle_marker_tolerates_extra_fields() {
        assert!(parse_parts(b"B,anything,else").battle);
        let parts = parse_parts(b"S,1,0,0,0,1,1;B");
        assert!(parts.battle);
        assert!(!parts.is_empty());
    }

    #[test]
    fn parts_process_only_first_ten_segments() {
        let mut token = String::new();
        for index in 0..10 {
            if index > 0 {
                token.push(';');
            }
            token.push_str("S,1,0,0,0,1,1");
        }
        token.push_str(";B");
        let parts = parse_parts(token.as_bytes());
        assert_eq!(parts.segments.len(), 10);
        assert!(!parts.battle);

        let mut token = String::new();
        for index in 0..9 {
            if index > 0 {
                token.push(';');
            }
            token.push_str("S,1,0,0,0,1,1");
        }
        token.push_str(";B");
        assert!(parse_parts(token.as_bytes()).battle);
    }

    #[test]
    fn world_state_ignores_self_and_applies_remote() {
        let mut world = WorldState::new(Some(42));
        assert_eq!(
            world.apply_wp_line(b"WP 42 1 0 0 S,1,0,0,0,1,1"),
            Ok(false)
        );
        assert!(world.is_empty());
        assert_eq!(
            world.apply_wp_line(b"WP 7 2 10 20 S,5,1,0,0,1,1 Remy"),
            Ok(true)
        );
        let remote = world.get(7).unwrap();
        assert_eq!(remote.id, 7);
        assert_eq!(remote.room, 2);
        assert_eq!(remote.name, b"Remy".to_vec());
        assert!(remote.is_protocol_visible());
        assert!(remote.is_drawable_in_room(2));
        assert!(!remote.is_drawable_in_room(3));
    }

    #[test]
    fn world_state_hidden_and_deleted_are_distinct() {
        let mut world = WorldState::new(None);
        world
            .apply_wp_line(b"WP 7 2 0 20 S,5,1,0,0,1,1")
            .unwrap();
        assert!(world.get(7).unwrap().is_protocol_visible());

        world
            .apply_wp_line(b"WP 7 2 -500 20 S,5,1,0,0,1,1")
            .unwrap();
        assert!(world.get(7).is_some());
        assert!(!world.get(7).unwrap().is_protocol_visible());

        world
            .apply_wp_line(b"WP 7 2 -499.5 20 S,5,1,0,0,1,1")
            .unwrap();
        assert!(world.get(7).unwrap().is_protocol_visible());

        world.apply_wp_line(b"WP 7 2 0 20 -").unwrap();
        assert!(world.get(7).is_some());
        assert!(!world.get(7).unwrap().is_protocol_visible());

        world.apply_wp_line(b"WP 7 2 0 20 Q,1").unwrap();
        assert!(world.get(7).is_some());
        assert!(world.get(7).unwrap().parts.is_empty());
        assert!(!world.get(7).unwrap().is_protocol_visible());

        assert_eq!(world.apply_wdel_line(b"WDEL 7"), Ok(true));
        assert!(world.get(7).is_none());
        assert_eq!(world.apply_wdel_line(b"WDEL 7"), Ok(false));
    }

    #[test]
    fn world_state_malformed_update_changes_nothing() {
        let mut world = WorldState::new(None);
        world
            .apply_wp_line(b"WP 7 2 10 20 S,5,1,0,0,1,1 Remy")
            .unwrap();
        let before = world.get(7).unwrap().clone();
        assert_eq!(
            world.apply_wp_line(b"WP 7 99 x 20 nope"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(world.get(7).unwrap(), &before);
        assert_eq!(
            world.apply_wdel_line(b"WDEL 7 extra"),
            Err(ParseError::ExtraField)
        );
        assert_eq!(world.get(7).unwrap(), &before);
    }

    #[test]
    fn wp_caps_remote_name_and_keeps_prefix() {
        let name = "N".repeat(MAX_REMOTE_NAME_BYTES * 3);
        let line = format!("WP 1 2 3 4 S,1,0,0,0,1,1 {name}");
        let message = parse_wp(line.as_bytes()).unwrap();
        assert_eq!(message.name.len(), MAX_REMOTE_NAME_BYTES);
        assert_eq!(message.name, vec![b'N'; MAX_REMOTE_NAME_BYTES]);
        assert!(message.raw_remainder.len() > MAX_REMOTE_NAME_BYTES);
    }

    #[test]
    fn wp_name_caps_on_utf8_boundary() {
        let name = format!("{}é{}", "a".repeat(MAX_REMOTE_NAME_BYTES - 1), "tail");
        let line = format!("WP 1 2 3 4 S,1,0,0,0,1,1 {name}");
        let message = parse_wp(line.as_bytes()).unwrap();
        assert!(message.name.len() <= MAX_REMOTE_NAME_BYTES);
        assert!(std::str::from_utf8(&message.name).is_ok());
        assert_eq!(message.name, vec![b'a'; MAX_REMOTE_NAME_BYTES - 1]);

        let exact = format!("{}é{}", "a".repeat(MAX_REMOTE_NAME_BYTES - 2), "tail");
        let line = format!("WP 1 2 3 4 S,1,0,0,0,1,1 {exact}");
        let message = parse_wp(line.as_bytes()).unwrap();
        assert_eq!(message.name.len(), MAX_REMOTE_NAME_BYTES);
        assert_eq!(std::str::from_utf8(&message.name).unwrap(), &exact[..MAX_REMOTE_NAME_BYTES]);
    }

    #[test]
    fn wp_sanitizes_control_bytes_inside_retained_prefix() {
        let message = parse_wp(b"WP 1 2 3 4 S,1,0,0,0,1,1 A\tB\x7fC\x1bD").unwrap();
        assert_eq!(message.name, b"ABCD");
        assert!(message.raw_remainder.ends_with(b"A\tB\x7fC\x1bD"));

        let prefix = "N".repeat(MAX_REMOTE_NAME_BYTES);
        let line = format!("WP 1 2 3 4 S,1,0,0,0,1,1 {prefix}\tignored");
        let message = parse_wp(line.as_bytes()).unwrap();
        assert_eq!(message.name, vec![b'N'; MAX_REMOTE_NAME_BYTES]);
    }

    #[test]
    fn wp_accepts_utf8_names_and_keeps_spaces() {
        assert!(parse_wp("WP 1 2 3 4 S,1,0,0,0,1,1 Aé B ".as_bytes()).is_ok());
        let message = parse_wp("WP 1 2 3 4 S,1,0,0,0,1,1 Aé B ".as_bytes()).unwrap();
        assert_eq!(message.name, "Aé B ".as_bytes());
    }

    #[test]
    fn wp_rejects_out_of_range_and_non_finite_positions() {
        for position in [
            MAX_COORDINATE.to_string(),
            format!("-{}", MAX_COORDINATE.abs()),
            "2147483648".to_string(),
            "-2147483649".to_string(),
            "1e3".to_string(),
            "NaN".to_string(),
            "inf".to_string(),
        ] {
            let line = format!("WP 1 2 {position} 0 S,1,0,0,0,1,1");
            let expected = if position == MAX_COORDINATE.to_string()
                || position == format!("-{}", MAX_COORDINATE.abs())
            {
                None
            } else {
                Some(ParseError::MalformedNumeric)
            };
            assert_eq!(parse_wp(line.as_bytes()).err(), expected, "{line}");
        }
        assert!(parse_wp(b"WP 1 2 2147483647 2147483647 S,1,0,0,0,1,1").is_ok());
        assert!(parse_wp(b"WP 1 2 -2147483648 -2147483648 S,1,0,0,0,1,1").is_ok());
    }

    #[test]
    fn parts_accept_large_finite_geometry_but_reject_non_finite() {
        let parts = parse_parts(b"S,1,0,3000000000,0,1,1;S,2,0,0,0,1,1");
        assert_eq!(parts.segments.len(), 2);
        assert_eq!(parts.primary_player().unwrap().dx, 3_000_000_000.0);
        assert!(parse_parts(b"P,25,2,0,0,0,1,3000000000").pokemon().next().is_some());
        assert!(parse_parts(b"S,1,0,NaN,0,1,1").is_empty());
        assert!(parse_parts(b"S,1,0,inf,0,1,1").is_empty());
    }

    #[test]
    fn world_state_rejects_rendered_position_outside_range() {
        let mut world = WorldState::new(None);
        let message =
            parse_wp(b"WP 1 2 2147483647 0 S,1,0,1,0,1,1").unwrap();
        assert!(!world.apply_wp(&message));
        assert!(world.is_empty());

        let message =
            parse_wp(b"WP 2 2 -1500000000 0 S,1,0,3000000000,0,1,1").unwrap();
        assert!(world.apply_wp(&message));
        assert_eq!(world.get(2).unwrap().parts.primary_player().unwrap().dx, 3_000_000_000.0);
    }

    #[test]
    fn wpos_rejects_out_of_range_positions() {
        assert_eq!(
            parse_wpos(b"WPOS 7 2147483648 96 S,1,0,0,0,1,1"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(
            parse_wpos(b"WPOS 7 128 -2147483649"),
            Err(ParseError::MalformedNumeric)
        );
        assert!(parse_wpos(b"WPOS 7 2147483647 -2147483648").is_ok());
    }

    #[test]
    fn world_state_bounds_remote_count_and_evicts_oldest() {
        let mut world = WorldState::new(None);
        for id in 0..MAX_REMOTES as u64 {
            assert_eq!(
                world.apply_wp_line(format!("WP {id} 2 {id} 20 S,5,1,0,0,1,1 R{id}").as_bytes()),
                Ok(true)
            );
        }
        assert_eq!(world.len(), MAX_REMOTES);

        let (applied, evicted) = world.apply_wp_with_eviction(
            &parse_wp(b"WP 999999 2 10 20 S,5,1,0,0,1,1 Nuevo").unwrap(),
        );
        assert!(applied);
        assert_eq!(evicted, Some(0));
        assert_eq!(world.len(), MAX_REMOTES);
        assert!(world.get(0).is_none());
        assert!(world.get(999999).is_some());
    }

    #[test]
    fn world_state_update_refreshes_order() {
        let mut world = WorldState::new(None);
        for id in 0..MAX_REMOTES as u64 {
            world.apply_wp_line(format!("WP {id} 2 {id} 20 S,5,1,0,0,1,1").as_bytes()).unwrap();
        }
        world.apply_wp_line(b"WP 0 2 0 20 S,5,1,0,0,1,1 refreshed").unwrap();
        world.apply_wp_line(b"WP 999999 2 10 20 S,5,1,0,0,1,1").unwrap();
        assert!(world.get(0).is_some());
        assert!(world.get(1).is_none());
    }

    #[test]
    fn world_state_capacity_is_independent_from_wlist_page_size() {
        assert!(MAX_REMOTES > MAX_LIST_ENTRIES);
        let world = WorldState::new(None);
        assert_eq!(world.capacity(), MAX_REMOTES);
    }

    #[test]
    fn world_state_evicts_idle_rooms_before_current_room() {
        let mut world = WorldState::with_capacity(None, 3);
        for (id, room) in [(0u64, 3i32), (1, 3), (2, 2)] {
            world
                .apply_wp_line(format!("WP {id} {room} {id} 20 S,5,1,0,0,1,1 R{id}").as_bytes())
                .unwrap();
        }

        let (applied, evicted) =
            world.apply_wp_with_eviction(&parse_wp(b"WP 3 2 10 20 S,5,1,0,0,1,1 R3").unwrap());
        assert!(applied);
        assert_eq!(evicted, Some(0));
        assert!(world.get(0).is_none());
        assert!(world.get(2).is_some());

        let (applied, evicted) =
            world.apply_wp_with_eviction(&parse_wp(b"WP 4 2 11 20 S,5,1,0,0,1,1 R4").unwrap());
        assert!(applied);
        assert_eq!(evicted, Some(1));
        assert!(world.get(1).is_none());
        assert!(world.get(2).is_some());

        let (applied, evicted) =
            world.apply_wp_with_eviction(&parse_wp(b"WP 5 2 12 20 S,5,1,0,0,1,1 R5").unwrap());
        assert!(applied);
        assert_eq!(evicted, Some(2));
        assert!(world.get(2).is_none());
        assert_eq!(world.len(), 3);
    }

    #[test]
    fn world_state_applies_position_only_state_with_same_capacity_policy() {
        let mut world = WorldState::with_capacity(None, 2);
        assert!(world.apply_remote_state(1, 2, 10.0, 20.0, b"Ada").0);
        assert!(world.apply_remote_state(2, 3, 11.0, 21.0, b"Bob").0);
        let (applied, evicted) = world.apply_remote_state(3, 2, 12.0, 22.0, b"Cy\t!");
        assert!(applied);
        assert_eq!(evicted, Some(2));
        assert_eq!(world.get(3).unwrap().name, b"Cy!");
        assert!(world.get(2).is_none());
    }

    #[test]
    fn world_state_clamps_external_messages() {
        let mut world = WorldState::new(None);
        let mut message = parse_wp(b"WP 7 2 10 20 S,5,1,0,0,1,1 Ada").unwrap();
        message.name = vec![b'X'; MAX_REMOTE_NAME_BYTES * 4];
        assert!(world.apply_wp(&message));
        assert_eq!(world.get(7).unwrap().name.len(), MAX_REMOTE_NAME_BYTES);

        message.id = 8;
        message.x = f64::INFINITY;
        assert!(!world.apply_wp(&message));
        assert!(world.get(8).is_none());
    }

    #[test]
    fn wdel_parses_and_enforces_arity() {
        let message = parse_wdel(b"WDEL  42  ").unwrap();
        assert_eq!(message.id, 42);
        assert_eq!(message.raw_remainder, b"  42  ".to_vec());
        assert_eq!(parse_wdel(b"WDEL"), Err(ParseError::MissingField));
        assert_eq!(
            parse_wdel(b"WDEL -1"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(
            parse_wdel(b"WDEL 18446744073709551616"),
            Err(ParseError::MalformedNumeric)
        );
        assert_eq!(parse_wdel(b"WDEL 1 2"), Err(ParseError::ExtraField));
    }

    #[test]
    fn wpos_parses_optional_parts() {
        let message = parse_wpos(b"WPOS 7 128 96 S,25,2,0,0,1,1").unwrap();
        assert_eq!(message.room, 7);
        assert_eq!(message.x, 128.0);
        assert_eq!(message.y, 96.0);
        assert_eq!(message.parts.primary_player().unwrap().sprite, 25);
        let message = parse_wpos(b"WPOS 7 128 96").unwrap();
        assert!(message.parts.is_empty());
        assert!(message.parts_token.is_none());
        assert_eq!(
            parse_wpos(b"WPOS 7 128 96 S,1,0,0,0,1,1 extra"),
            Err(ParseError::ExtraField)
        );
    }

    #[test]
    fn wlist_parses_entries_and_display() {
        let line = b"WLIST 2\x1dAda\x1eRoute 1\x1eGM\x1dBob\x1eRoute 2";
        let list = parse_wlist(line).unwrap();
        assert_eq!(list.total, 2);
        assert_eq!(list.entries.len(), 2);
        assert_eq!(
            list.entries[0],
            ListEntry {
                name: "Ada".to_string(),
                place: "Route 1".to_string(),
                flags: "GM".to_string()
            }
        );
        assert_eq!(
            list.entries[1],
            ListEntry {
                name: "Bob".to_string(),
                place: "Route 2".to_string(),
                flags: String::new()
            }
        );
        assert_eq!(wlist_display(&list.entries[0]), "Ada - Route 1 (GM)");
        assert_eq!(wlist_display(&list.entries[1]), "Bob - Route 2");
    }

    #[test]
    fn wlist_tolerates_bad_total_and_skips_bad_entries() {
        let list = parse_wlist(b"WLIST nope\x1dX").unwrap();
        assert_eq!(list.total, 0);
        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].name, "X");

        let list = parse_wlist(b"WLIST 1\x1d\x1eX\x1dA\x1e\x1eF\x1eignored").unwrap();
        assert_eq!(list.total, 1);
        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].name, "A");
        assert_eq!(list.entries[0].place, "");
        assert_eq!(list.entries[0].flags, "F");
    }

    #[test]
    fn wlist_preserves_spaces_and_trims_total() {
        let list = parse_wlist(b"WLIST   7 \x1d A \x1e P ").unwrap();
        assert_eq!(list.total, 7);
        assert_eq!(list.entries[0].name, " A ");
        assert_eq!(list.entries[0].place, " P ");
    }

    #[test]
    fn wlist_processes_at_most_250_entries() {
        let mut line = b"WLIST 251".to_vec();
        for index in 0..251 {
            line.push(0x1d);
            line.extend_from_slice(format!("n{}", index).as_bytes());
        }
        let list = parse_wlist(&line).unwrap();
        assert_eq!(list.total, 251);
        assert_eq!(list.entries.len(), 250);
        assert_eq!(list.entries[249].name, "n249");
    }

    #[test]
    fn wlist_caps_text_without_splitting_utf8() {
        let name = "é".repeat(MAX_LIST_TEXT_BYTES);
        let line = format!("WLIST 1\x1d{name}\x1eplace\x1eflags");
        let list = parse_wlist(line.as_bytes()).unwrap();
        assert_eq!(list.entries.len(), 1);
        assert!(list.entries[0].name.len() <= MAX_LIST_TEXT_BYTES);
        assert!(name.starts_with(&list.entries[0].name));
        assert_eq!(list.entries[0].place, "place");
        assert_eq!(list.entries[0].flags, "flags");
    }

    #[test]
    fn world_line_dispatches_and_ignores_unknown() {
        assert!(matches!(
            parse_world_line(b"WP 1 2 3 4 S,1,0,0,0,1,1"),
            Ok(WorldMessage::Wp(_))
        ));
        assert!(matches!(
            parse_world_line(b"WDEL 1"),
            Ok(WorldMessage::Wdel(_))
        ));
        assert!(matches!(
            parse_world_line(b"WLIST 0"),
            Ok(WorldMessage::Wlist(_))
        ));
        assert_eq!(parse_world_line(b"NOPE 1"), Ok(WorldMessage::Unknown));
    }

    #[test]
    fn shiny_predicate_uses_exact_exclusive_lower_bounds() {
        assert!(!is_shiny(1000));
        assert!(is_shiny(1001));
        assert!(is_shiny(2000));
        assert!(!is_shiny(2001));
        assert!(!is_shiny(3000));
        assert!(is_shiny(3001));
        assert!(is_shiny(4000));
        assert!(!is_shiny(4001));
        assert!(!is_shiny(101000));
        assert!(is_shiny(101001));
        assert!(is_shiny(102000));
        assert!(!is_shiny(102001));
        assert!(!is_shiny(103000));
        assert!(is_shiny(103001));
        assert!(is_shiny(104000));
        assert!(!is_shiny(104001));
        assert!(!is_shiny(151000));
        assert!(is_shiny(151001));
        assert!(is_shiny(152000));
        assert!(!is_shiny(152001));
        assert!(!is_shiny(153000));
        assert!(is_shiny(153001));
        assert!(is_shiny(154000));
        assert!(!is_shiny(154001));
        assert!(!is_shiny(201000));
        assert!(is_shiny(201001));
        assert!(is_shiny(202000));
        assert!(!is_shiny(202001));
        assert!(!is_shiny(203000));
        assert!(is_shiny(203001));
        assert!(is_shiny(204000));
        assert!(!is_shiny(204001));
        assert!(!is_shiny(-1001));
    }

    #[test]
    fn overworld_paths_use_full_id_and_shiny_band() {
        let context = OverworldContext::simple();
        assert_eq!(
            overworld_asset_path(25, context).as_deref(),
            Some("pkmn/25.gif")
        );
        assert_eq!(
            overworld_asset_path(1001, context).as_deref(),
            Some("pkmn_shiny/1001.gif")
        );
        assert_eq!(
            overworld_asset_path(100003, context).as_deref(),
            Some("pkmn/100003.gif")
        );
        assert_eq!(
            overworld_asset_path(101006, context).as_deref(),
            Some("pkmn_shiny/101006.gif")
        );
        assert_eq!(overworld_asset_path(0, context), None);
    }

    #[test]
    fn overworld_simple_remaps_are_unconditional() {
        let context = OverworldContext::simple();
        assert_eq!(
            overworld_asset_path(421, context).as_deref(),
            Some("pkmn/809.gif")
        );
        assert_eq!(
            overworld_asset_path(1421, context).as_deref(),
            Some("pkmn_shiny/1809.gif")
        );
        assert_eq!(
            overworld_asset_path(3171, context).as_deref(),
            Some("pkmn_shiny/3139.gif")
        );
        assert_eq!(
            overworld_asset_path(3172, context).as_deref(),
            Some("pkmn_shiny/3165.gif")
        );
        assert_eq!(
            overworld_asset_path(3226, context).as_deref(),
            Some("pkmn_shiny/3170.gif")
        );
    }

    #[test]
    fn overworld_cached_remaps_depend_on_context() {
        let mut context = OverworldContext::cached();
        assert_eq!(
            overworld_asset_path(421, context).as_deref(),
            Some("pkmn/421.gif")
        );
        context.is_day = true;
        assert_eq!(
            overworld_asset_path(421, context).as_deref(),
            Some("pkmn/809.gif")
        );
        assert_eq!(
            overworld_asset_path(1421, context).as_deref(),
            Some("pkmn_shiny/1809.gif")
        );
        context.in_battle = true;
        assert_eq!(
            overworld_asset_path(421, context).as_deref(),
            Some("pkmn/421.gif")
        );
        context.in_battle = false;
        context.is_day = false;
        context.weather.controller_825 = true;
        assert_eq!(
            overworld_asset_path(421, context).as_deref(),
            Some("pkmn/809.gif")
        );
        context.weather.controller_825 = false;
        context.burmy_location = BurmyLocation::B;
        assert_eq!(
            overworld_asset_path(412, context).as_deref(),
            Some("pkmn/810.gif")
        );
        assert_eq!(
            overworld_asset_path(1412, context).as_deref(),
            Some("pkmn_shiny/1810.gif")
        );
        context.burmy_location = BurmyLocation::C;
        assert_eq!(
            overworld_asset_path(412, context).as_deref(),
            Some("pkmn/811.gif")
        );
        assert_eq!(
            overworld_asset_path(1412, context).as_deref(),
            Some("pkmn_shiny/1811.gif")
        );
        context.in_battle = true;
        assert_eq!(
            overworld_asset_path(412, context).as_deref(),
            Some("pkmn/412.gif")
        );
    }

    #[test]
    fn egg_aliases_differ_between_loaders() {
        let simple = OverworldContext::simple();
        let cached = OverworldContext::cached();
        assert_eq!(
            overworld_asset_path(-628, simple).as_deref(),
            Some("huevo/201.gif")
        );
        assert_eq!(
            overworld_asset_path(-601, simple).as_deref(),
            Some("huevo/201.gif")
        );
        assert_eq!(
            overworld_asset_path(-717, simple).as_deref(),
            Some("huevo/493.gif")
        );
        assert_eq!(
            overworld_asset_path(-807, simple).as_deref(),
            Some("huevo/493.gif")
        );
        assert_eq!(
            overworld_asset_path(-995, simple).as_deref(),
            Some("huevo/995.gif")
        );
        assert_eq!(
            overworld_asset_path(-995, cached).as_deref(),
            Some("huevo/834.gif")
        );
        assert_eq!(
            overworld_asset_path(-1601, cached).as_deref(),
            Some("huevo_shiny/1201.gif")
        );
        assert_eq!(
            overworld_asset_path(-1807, cached).as_deref(),
            Some("huevo_shiny/1493.gif")
        );
        assert_eq!(
            overworld_asset_path(-1995, cached).as_deref(),
            Some("huevo_shiny/1834.gif")
        );
        assert_eq!(
            overworld_asset_path(-1601, simple).as_deref(),
            Some("huevo_shiny/1601.gif")
        );
    }

    #[test]
    fn overworld_missing_files_use_context_placeholder() {
        let context = OverworldContext::simple();
        let none = |_: &str| false;
        assert_eq!(
            overworld_sprite(1001, context, none),
            OverworldSprite::Placeholder("pkmn/0.gif".to_string())
        );
        assert_eq!(
            overworld_sprite(25, context, none),
            OverworldSprite::Placeholder("pkmn/0.gif".to_string())
        );
        assert_eq!(
            overworld_sprite(-1201, context, none),
            OverworldSprite::Placeholder("huevo/0.gif".to_string())
        );
        assert_eq!(overworld_sprite(0, context, none), OverworldSprite::None);

        let only_shiny = exists_in(&["pkmn_shiny/1001.gif"]);
        assert_eq!(
            overworld_sprite(1001, context, only_shiny),
            OverworldSprite::File("pkmn_shiny/1001.gif".to_string())
        );
        let only_normal = exists_in(&["pkmn/1.gif"]);
        assert_eq!(
            overworld_sprite(1001, context, only_normal),
            OverworldSprite::Placeholder("pkmn/0.gif".to_string())
        );
    }

    #[test]
    fn battle_slot_names_cover_numbers_letters_and_alt() {
        assert_eq!(
            battle_filename(6, 1).as_deref(),
            Some("pkmnbattle/P6_1.gmspr")
        );
        assert_eq!(
            battle_filename(6, 9).as_deref(),
            Some("pkmnbattle/P6_9.gmspr")
        );
        assert_eq!(
            battle_filename(6, 10).as_deref(),
            Some("pkmnbattle/P6_A.gmspr")
        );
        assert_eq!(
            battle_filename(6, 11).as_deref(),
            Some("pkmnbattle/P6_H.gmspr")
        );
        assert_eq!(
            battle_filename(6, 12).as_deref(),
            Some("pkmnbattle/P6_S.gmspr")
        );
        assert_eq!(
            battle_filename(6, 13).as_deref(),
            Some("pkmnbattle/P6_B.gmspr")
        );
        assert_eq!(
            battle_filename(6, 15).as_deref(),
            Some("pkmnbattle/P6_D.gmspr")
        );
        assert_eq!(
            battle_filename(6, 22).as_deref(),
            Some("pkmnbattle/P6_2_ALT.gmspr")
        );
        assert_eq!(
            battle_filename(6, 30).as_deref(),
            Some("pkmnbattle/P6_A_ALT.gmspr")
        );
        assert_eq!(
            battle_filename(6, 31).as_deref(),
            Some("pkmnbattle/P6_H_ALT.gmspr")
        );
        assert_eq!(battle_filename(6, 0), None);
        assert_eq!(battle_filename(6, 16), None);
    }

    #[test]
    fn battle_form_remaps_follow_priority_rules() {
        let mut context = BattleContext::default();
        assert_eq!(resolve_battle_id(493, context), 700);
        assert_eq!(resolve_battle_id(1493, context), 1700);
        assert_eq!(resolve_battle_id(351, context), 351);
        assert_eq!(resolve_battle_id(1351, context), 1351);
        context.weather.controller_823 = true;
        assert_eq!(resolve_battle_id(351, context), 823);
        assert_eq!(resolve_battle_id(1351, context), 1823);
        context.weather.controller_824 = true;
        assert_eq!(resolve_battle_id(351, context), 824);
        assert_eq!(resolve_battle_id(1351, context), 1824);
        context.weather.controller_825 = true;
        assert_eq!(resolve_battle_id(351, context), 825);
        assert_eq!(resolve_battle_id(1351, context), 1825);

        let mut context = BattleContext::default();
        assert_eq!(resolve_battle_id(421, context), 421);
        context.is_day = true;
        assert_eq!(resolve_battle_id(421, context), 809);
        assert_eq!(resolve_battle_id(1421, context), 1809);

        let mut context = BattleContext::default();
        context.burmy_location = BurmyLocation::B;
        assert_eq!(resolve_battle_id(412, context), 810);
        assert_eq!(resolve_battle_id(1412, context), 1810);
        context.burmy_location = BurmyLocation::C;
        assert_eq!(resolve_battle_id(412, context), 811);
        assert_eq!(resolve_battle_id(1412, context), 1811);
        assert_eq!(resolve_battle_id(3171, context), 3139);
        assert_eq!(resolve_battle_id(3172, context), 3165);
        assert_eq!(resolve_battle_id(3226, context), 3170);
    }

    #[test]
    fn battle_required_missing_uses_placeholder_eight() {
        let context = BattleContext::default();
        let none = |_: &str| false;
        assert_eq!(
            battle_sprite(6, 2, context, none).unwrap(),
            BattleSprite::Placeholder {
                missing: "pkmnbattle/P6_2.gmspr".to_string(),
                sprite: 8
            }
        );
        let only_two = exists_in(&["pkmnbattle/P6_2.gmspr"]);
        assert_eq!(
            battle_sprite(6, 2, context, only_two).unwrap(),
            BattleSprite::File("pkmnbattle/P6_2.gmspr".to_string())
        );
    }

    #[test]
    fn battle_sleep_aliases_to_hurt() {
        let context = BattleContext::default();
        let only_hurt = exists_in(&["pkmnbattle/P6_H.gmspr"]);
        assert_eq!(
            battle_sprite(6, 12, context, only_hurt).unwrap(),
            BattleSprite::File("pkmnbattle/P6_H.gmspr".to_string())
        );
        let none = |_: &str| false;
        assert_eq!(
            battle_sprite(6, 12, context, none).unwrap(),
            BattleSprite::Placeholder {
                missing: "pkmnbattle/P6_H.gmspr".to_string(),
                sprite: 8
            }
        );
    }

    #[test]
    fn battle_left_trio_mirrors_right_when_missing() {
        let context = BattleContext::default();
        let only_right = exists_in(&[
            "pkmnbattle/P6_3.gmspr",
            "pkmnbattle/P6_6.gmspr",
            "pkmnbattle/P6_9.gmspr",
        ]);
        assert_eq!(
            battle_sprite(6, 1, context, only_right).unwrap(),
            BattleSprite::Mirrored {
                path: "pkmnbattle/P6_3.gmspr".to_string(),
                source_slot: 3
            }
        );
        let only_right = exists_in(&[
            "pkmnbattle/P6_3.gmspr",
            "pkmnbattle/P6_6.gmspr",
            "pkmnbattle/P6_9.gmspr",
        ]);
        assert_eq!(
            battle_sprite(6, 4, context, only_right).unwrap(),
            BattleSprite::Mirrored {
                path: "pkmnbattle/P6_6.gmspr".to_string(),
                source_slot: 6
            }
        );
        let only_right = exists_in(&[
            "pkmnbattle/P6_3.gmspr",
            "pkmnbattle/P6_6.gmspr",
            "pkmnbattle/P6_9.gmspr",
        ]);
        assert_eq!(
            battle_sprite(6, 7, context, only_right).unwrap(),
            BattleSprite::Mirrored {
                path: "pkmnbattle/P6_9.gmspr".to_string(),
                source_slot: 9
            }
        );
    }

    #[test]
    fn battle_partial_left_trio_uses_placeholder() {
        let context = BattleContext::default();
        let only_one = exists_in(&["pkmnbattle/P6_1.gmspr"]);
        assert_eq!(
            battle_sprite(6, 1, context, only_one).unwrap(),
            BattleSprite::File("pkmnbattle/P6_1.gmspr".to_string())
        );
        let only_one = exists_in(&["pkmnbattle/P6_1.gmspr"]);
        assert_eq!(
            battle_sprite(6, 4, context, only_one).unwrap(),
            BattleSprite::Placeholder {
                missing: "pkmnbattle/P6_4.gmspr".to_string(),
                sprite: 8
            }
        );
    }

    #[test]
    fn battle_alt_files_alias_to_base_when_absent() {
        let context = BattleContext::default();
        let alt = exists_in(&["pkmnbattle/P6_2_ALT.gmspr"]);
        assert_eq!(
            battle_sprite(6, 22, context, alt).unwrap(),
            BattleSprite::File("pkmnbattle/P6_2_ALT.gmspr".to_string())
        );
        let base = exists_in(&["pkmnbattle/P6_2.gmspr"]);
        assert_eq!(
            battle_sprite(6, 22, context, base).unwrap(),
            BattleSprite::File("pkmnbattle/P6_2.gmspr".to_string())
        );
        let none = |_: &str| false;
        assert_eq!(
            battle_sprite(6, 22, context, none).unwrap(),
            BattleSprite::Placeholder {
                missing: "pkmnbattle/P6_2.gmspr".to_string(),
                sprite: 8
            }
        );
    }

    #[test]
    fn battle_sleep_alt_never_probes_s_alt() {
        let context = BattleContext::default();
        let sleep_alt = exists_in(&[
            "pkmnbattle/P6_S_ALT.gmspr",
            "pkmnbattle/P6_S.gmspr",
        ]);
        assert_eq!(
            battle_sprite(6, 32, context, sleep_alt).unwrap(),
            BattleSprite::File("pkmnbattle/P6_S.gmspr".to_string())
        );
    }

    #[test]
    fn battle_remap_target_missing_uses_placeholder() {
        let context = BattleContext::default();
        let none = |_: &str| false;
        assert_eq!(
            battle_sprite(493, 2, context, none).unwrap(),
            BattleSprite::Placeholder {
                missing: "pkmnbattle/P700_2.gmspr".to_string(),
                sprite: 8
            }
        );
    }

    #[test]
    fn pokedex_preview_only_requests_front_slot() {
        let none = |_: &str| false;
        assert_eq!(
            pokedex_battle_sprite(6, none),
            BattleSprite::Placeholder {
                missing: "pkmnbattle/P6_2.gmspr".to_string(),
                sprite: 8
            }
        );
        let front = exists_in(&["pkmnbattle/P6_2.gmspr"]);
        assert_eq!(
            pokedex_battle_sprite(6, front),
            BattleSprite::File("pkmnbattle/P6_2.gmspr".to_string())
        );
    }
}
