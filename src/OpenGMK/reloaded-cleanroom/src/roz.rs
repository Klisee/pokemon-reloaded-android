//! Clean-room codec for the ROZ/Zibelan line protocol.
//!
//! This module implements only the wire behaviour described in
//! `cleanroom/spec/roz-protocol.md` and the Zibelan/ROZ section of
//! `cleanroom/spec/compatibility.md`: LF framing, the client handshake and
//! commands, typed server commands, the state blob, hex `MSG` framing, and the
//! `MSG` payload subcommands. It has no socket, thread, emulator, or game-asset
//! dependency.

use std::fmt;

/// Hard cap for one line, excluding the terminating LF.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Compatibility floor: implementations must accept at least this much.
pub const MIN_LINE_BYTES: usize = 8 * 1024;

/// Compatibility floor for a decoded `MSG` payload.
pub const MIN_MSG_PAYLOAD_BYTES: usize = 4 * 1024;

/// Largest payload that fits in a line with the `MSG ` prefix and hex encoding.
pub const MAX_MSG_PAYLOAD_BYTES: usize = (MAX_LINE_BYTES - 4) / 2;

/// Protocol version announced by the current world handshake (`WVER`).
pub const WORLD_PROTOCOL_VERSION: u32 = 8;

/// Build stamp announced by the current world handshake (`WBUILD`).
pub const WORLD_BUILD: u64 = 20_261_109;

/// `NET_VERSION` as named by the game's `NetInit` script.
pub const NET_VERSION: u32 = WORLD_PROTOCOL_VERSION;

/// `NET_BUILD` as named by the game's `NetInit` script.
pub const NET_BUILD: u64 = WORLD_BUILD;

/// Separates a `MSG` subcommand from its fields.
pub const UNIT_SEPARATOR: u8 = 0x1f;

/// Separates a human-readable header from a structured record.
pub const RECORD_SEPARATOR: u8 = 0x1e;

/// Separates fields inside a structured record.
pub const FIELD_SEPARATOR: u8 = 0x1d;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    OverlongLine,
    EmbeddedControl,
    MissingField,
    ExtraField,
    InvalidField,
    MalformedNumber,
    MalformedState,
    MalformedHex,
    MalformedPayload,
    LineTooLong,
    MessageTooLarge,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OverlongLine => "line exceeds the protocol limit",
            Self::EmbeddedControl => "line contains a forbidden CR or LF",
            Self::MissingField => "line is missing a required field",
            Self::ExtraField => "line has an unexpected extra field",
            Self::InvalidField => "field has an invalid shape",
            Self::MalformedNumber => "field is not a valid decimal number",
            Self::MalformedState => "state blob does not match the grammar",
            Self::MalformedHex => "MSG argument is not valid hexadecimal",
            Self::MalformedPayload => "MSG payload does not match its subcommand",
            Self::LineTooLong => "encoded line exceeds the protocol limit",
            Self::MessageTooLarge => "MSG payload exceeds the protocol limit",
        })
    }
}

impl std::error::Error for ProtocolError {}

/// One completed line or an overlong-line marker produced by [`LineBuffer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineEvent {
    /// A line with its LF and an optional preceding CR removed.
    Line(Vec<u8>),
    /// An overlong line was discarded up to and including its LF.
    Overlong,
}

/// Incremental LF framing with CRLF tolerance and a hard line cap.
#[derive(Clone, Debug, Default)]
pub struct LineBuffer {
    data: Vec<u8>,
    discarding: bool,
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed transport bytes and return every completed line.
    ///
    /// Empty lines are retained as `Line(Vec::new())`; the caller decides
    /// whether to ignore them. An overlong line emits one [`LineEvent::Overlong`]
    /// and is then discarded until the next LF, so the following line parses
    /// normally.
    pub fn feed(&mut self, input: &[u8]) -> Vec<LineEvent> {
        let mut events = Vec::new();
        for &byte in input {
            if byte == b'\n' {
                if self.discarding {
                    self.discarding = false;
                    continue;
                }
                let mut line = std::mem::take(&mut self.data);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.len() > MAX_LINE_BYTES {
                    events.push(LineEvent::Overlong);
                } else {
                    events.push(LineEvent::Line(line));
                }
                continue;
            }

            if self.discarding {
                continue;
            }

            self.data.push(byte);
            let length = self.data.len();
            if length > MAX_LINE_BYTES {
                // One extra CR is allowed because it may be the first half of a
                // CRLF terminator. Anything else means the content is too long.
                if length == MAX_LINE_BYTES + 1 && byte == b'\r' {
                    continue;
                }
                self.data.clear();
                self.data.shrink_to(256);
                self.discarding = true;
                events.push(LineEvent::Overlong);
            }
        }
        events
    }

    pub fn pending_len(&self) -> usize {
        self.data.len()
    }

    pub fn is_discarding(&self) -> bool {
        self.discarding
    }

    pub fn reset(&mut self) {
        self.data.clear();
        self.discarding = false;
    }
}

/// A validated state blob carried by `WPOS` and `WP`.
///
/// The original token is retained byte-for-byte. Parsing validates the known
/// segment tags but never rewrites the token, so relayed state is identical to
/// the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateBlob {
    raw: Vec<u8>,
    segments: Vec<StateSegment>,
}

/// One semicolon-separated segment of a [`StateBlob`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateSegment {
    /// `S`, the main state segment; at least six numeric fields, the last
    /// four of which are real-valued.
    Main { fields: Vec<Vec<u8>> },
    /// `P`, the additional state segment; at least seven numeric fields, the
    /// last four of which are real-valued.
    Pokemon { fields: Vec<Vec<u8>> },
    /// `B`, a marker without fields.
    Battle,
    /// `-`, the no-state placeholder.
    None,
    /// Any tag the protocol does not define; retained but not interpreted.
    Unknown { tag: u8, fields: Vec<Vec<u8>> },
}

impl StateBlob {
    /// The `-` no-state placeholder.
    pub fn none() -> Self {
        Self { raw: b"-".to_vec(), segments: vec![StateSegment::None] }
    }

    /// An absent state token, used when `WPOS` carries no parts field.
    pub fn empty() -> Self {
        Self { raw: Vec::new(), segments: Vec::new() }
    }

    /// Parse and validate a state token without normalising it.
    pub fn parse(token: &[u8]) -> Result<Self, ProtocolError> {
        if token.is_empty() || token.iter().any(|&byte| !(0x21..=0x7e).contains(&byte)) {
            return Err(ProtocolError::MalformedState);
        }
        if token == b"-" {
            return Ok(Self::none());
        }

        let mut segments = Vec::new();
        for segment in token.split(|&byte| byte == b';') {
            if segment.is_empty() {
                return Err(ProtocolError::MalformedState);
            }
            let tag = segment[0];
            let rest = &segment[1..];
            match tag {
                b'-' => return Err(ProtocolError::MalformedState),
                b'B' => {
                    if !rest.is_empty() {
                        return Err(ProtocolError::MalformedState);
                    }
                    segments.push(StateSegment::Battle);
                },
                b'S' | b'P' => {
                    if rest.first() != Some(&b',') {
                        return Err(ProtocolError::MalformedState);
                    }
                    let fields: Vec<Vec<u8>> = rest[1..].split(|&byte| byte == b',').map(<[u8]>::to_vec).collect();
                    let required = if tag == b'S' { 6 } else { 7 };
                    if fields.len() < required {
                        return Err(ProtocolError::MalformedState);
                    }
                    // `S` is sprite, frame, dx, dy, xscale, yscale; `P` is
                    // species, direction, frame, dx, dy, xscale, yscale. Only
                    // the leading identity/frame fields are integers: the
                    // position and scale fields are real-valued.
                    let integer_fields = if tag == b'S' { 2 } else { 3 };
                    for field in &fields[..integer_fields] {
                        parse_i64(field)?;
                    }
                    for field in &fields[integer_fields..required] {
                        parse_real(field)?;
                    }
                    if tag == b'S' {
                        segments.push(StateSegment::Main { fields });
                    } else {
                        segments.push(StateSegment::Pokemon { fields });
                    }
                },
                _ => {
                    let fields = if rest.is_empty() {
                        Vec::new()
                    } else if rest[0] == b',' {
                        rest[1..].split(|&byte| byte == b',').map(<[u8]>::to_vec).collect()
                    } else {
                        vec![rest.to_vec()]
                    };
                    segments.push(StateSegment::Unknown { tag, fields });
                },
            }
        }

        Ok(Self { raw: token.to_vec(), segments })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.raw
    }

    pub fn segments(&self) -> &[StateSegment] {
        &self.segments
    }

    pub fn is_none(&self) -> bool {
        self.raw == b"-"
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

/// The payload of a client `WPOS` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfPosition {
    pub room: i32,
    pub x: i32,
    pub y: i32,
    pub state: StateBlob,
}

/// A complete `WP` peer snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerPosition {
    pub id: u64,
    pub room: i32,
    pub x: i32,
    pub y: i32,
    pub state: StateBlob,
    /// Raw bytes after the state field; spaces are preserved.
    pub name: Vec<u8>,
}

/// Commands a client may write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientCommand {
    /// `HELLO <name>`
    Hello { name: Vec<u8> },
    /// `WJOIN <name>`
    Join { name: Vec<u8> },
    /// `WVER <version>`
    Version { version: u32 },
    /// `WBUILD <build-id>`
    Build { build: u64 },
    /// `WTID <trainer-id>`
    WorldTrainerId { id: Vec<u8> },
    /// `WTSPR <sprite>`
    WorldSprite { sprite: Vec<u8> },
    /// `WSYNC`
    Sync,
    /// `WPOS <room> <x> <y> <state>`
    Position(SelfPosition),
    /// `WREQ <target-id> <kind> <info>`
    Request { target: u64, kind: Vec<u8>, info: Vec<u8> },
    /// `WANS <request-id> <answer>`
    Answer { request: u64, answer: i32 },
    /// `WCANCEL` or `WCANCEL <id>`
    Cancel { id: Option<u64> },
    /// `WSAY <text>`
    Say { text: Vec<u8> },
    /// `WLIST`
    WorldList,
    /// `IN <epoch> <frame> <mask>`
    Input { epoch: i32, frame: i32, mask: i32 },
    /// `SUM <epoch> <frame> <checksum>`
    Checksum { epoch: i32, frame: i32, checksum: i64 },
    /// `WEND`
    End,
    /// `BYE`
    Bye,
    /// `MSG <hex>`
    Message { payload: Vec<u8> },
}

impl ClientCommand {
    /// Encode one complete command line, including its trailing LF.
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut line = Vec::new();
        match self {
            Self::Hello { name } => {
                check_text(name)?;
                line.extend_from_slice(b"HELLO ");
                line.extend_from_slice(name);
            },
            Self::Join { name } => {
                check_text(name)?;
                line.extend_from_slice(b"WJOIN ");
                line.extend_from_slice(name);
            },
            Self::Version { version } => {
                line.extend_from_slice(b"WVER ");
                line.extend_from_slice(version.to_string().as_bytes());
            },
            Self::Build { build } => {
                line.extend_from_slice(b"WBUILD ");
                line.extend_from_slice(build.to_string().as_bytes());
            },
            Self::WorldTrainerId { id } => {
                check_token(id)?;
                line.extend_from_slice(b"WTID ");
                line.extend_from_slice(id);
            },
            Self::WorldSprite { sprite } => {
                check_token(sprite)?;
                line.extend_from_slice(b"WTSPR ");
                line.extend_from_slice(sprite);
            },
            Self::Sync => line.extend_from_slice(b"WSYNC"),
            Self::Position(position) => {
                line.extend_from_slice(b"WPOS ");
                push_i32(&mut line, position.room);
                line.push(b' ');
                push_i32(&mut line, position.x);
                line.push(b' ');
                push_i32(&mut line, position.y);
                if !position.state.is_empty() {
                    line.push(b' ');
                    line.extend_from_slice(position.state.as_bytes());
                }
            },
            Self::Request { target, kind, info } => {
                check_token(kind)?;
                check_text(info)?;
                line.extend_from_slice(b"WREQ ");
                line.extend_from_slice(target.to_string().as_bytes());
                line.push(b' ');
                line.extend_from_slice(kind);
                line.push(b' ');
                line.extend_from_slice(info);
            },
            Self::Answer { request, answer } => {
                line.extend_from_slice(b"WANS ");
                line.extend_from_slice(request.to_string().as_bytes());
                line.push(b' ');
                push_i32(&mut line, *answer);
            },
            Self::Cancel { id } => {
                line.extend_from_slice(b"WCANCEL");
                if let Some(id) = id {
                    line.push(b' ');
                    line.extend_from_slice(id.to_string().as_bytes());
                }
            },
            Self::Say { text } => {
                check_text(text)?;
                line.extend_from_slice(b"WSAY ");
                line.extend_from_slice(text);
            },
            Self::WorldList => line.extend_from_slice(b"WLIST"),
            Self::Input { epoch, frame, mask } => {
                line.extend_from_slice(b"IN ");
                push_i32(&mut line, *epoch);
                line.push(b' ');
                push_i32(&mut line, *frame);
                line.push(b' ');
                push_i32(&mut line, *mask);
            },
            Self::Checksum { epoch, frame, checksum } => {
                line.extend_from_slice(b"SUM ");
                push_i32(&mut line, *epoch);
                line.push(b' ');
                push_i32(&mut line, *frame);
                line.push(b' ');
                line.extend_from_slice(checksum.to_string().as_bytes());
            },
            Self::End => line.extend_from_slice(b"WEND"),
            Self::Bye => line.extend_from_slice(b"BYE"),
            Self::Message { payload } => {
                if payload.len() > MAX_MSG_PAYLOAD_BYTES {
                    return Err(ProtocolError::MessageTooLarge);
                }
                line.extend_from_slice(b"MSG ");
                push_hex(payload, &mut line);
            },
        }
        finish_line(line)
    }
}

/// Encode the three-line normal join sequence in protocol order.
pub fn encode_handshake(name: &[u8], version: u32, build: u64) -> Result<Vec<Vec<u8>>, ProtocolError> {
    Ok(vec![
        ClientCommand::Join { name: name.to_vec() }.encode()?,
        ClientCommand::Version { version }.encode()?,
        ClientCommand::Build { build }.encode()?,
    ])
}

/// Encode the world join burst used by `NetWorldConnect`.
///
/// Order is `WJOIN`, `WTID`, optional `WTSPR`, `WVER`, `WBUILD`.
pub fn encode_world_handshake(
    name: &[u8],
    trainer_id: &[u8],
    sprite: Option<&[u8]>,
    version: u32,
    build: u64,
) -> Result<Vec<Vec<u8>>, ProtocolError> {
    let mut lines = vec![
        ClientCommand::Join { name: name.to_vec() }.encode()?,
        ClientCommand::WorldTrainerId { id: trainer_id.to_vec() }.encode()?,
    ];
    if let Some(sprite) = sprite {
        lines.push(ClientCommand::WorldSprite { sprite: sprite.to_vec() }.encode()?);
    }
    lines.push(ClientCommand::Version { version }.encode()?);
    lines.push(ClientCommand::Build { build }.encode()?);
    Ok(lines)
}

/// Encode the current world join burst with the `NetInit` version/build.
pub fn encode_current_world_handshake(
    name: &[u8],
    trainer_id: &[u8],
    sprite: Option<&[u8]>,
) -> Result<Vec<Vec<u8>>, ProtocolError> {
    encode_world_handshake(name, trainer_id, sprite, WORLD_PROTOCOL_VERSION, WORLD_BUILD)
}

/// A parsed server-to-client command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerEvent {
    /// `WELCOME <id>`
    Welcome { id: u64 },
    /// `WOK <id>`
    Ok { id: u64 },
    /// `WAIT`
    Wait,
    /// `START <side> <peer-name>`
    Start { side: i32, peer: Vec<u8> },
    /// `FULL`
    Full,
    /// `WFULL`
    WorldFull,
    /// `PEER_LEFT`
    PeerLeft,
    /// `WP <id> <room> <x> <y> <state> <name>`
    Position(PeerPosition),
    /// `WDEL <id>`
    Delete { id: u64 },
    /// Relayed `WREQ <id> <kind> <info>`.
    Request { id: u64, kind: Vec<u8>, info: Vec<u8> },
    /// Relayed `WANS <id> <answer>`.
    Answer { id: u64, answer: i32 },
    /// Relayed `WCANCEL` or `WCANCEL <id>`.
    Cancel { id: Option<u64> },
    /// `IN <epoch> <frame> <mask>`.
    Input { epoch: i32, frame: i32, mask: i32 },
    /// `SUM <epoch> <frame> <checksum>`.
    Checksum { epoch: i32, frame: i32, checksum: i64 },
    /// `MSG <hex>`; the decoded bytes are returned uninterpreted.
    Message { payload: Vec<u8> },
}

/// Parse one server line.
///
/// Empty/whitespace-only lines and unknown commands return `Ok(None)` so a
/// caller can ignore them without resynchronising the stream. A known command
/// with malformed fields returns an error; the caller discards that line and
/// continues with the next one.
pub fn parse_server_line(line: &[u8]) -> Result<Option<ServerEvent>, ProtocolError> {
    let mut line = line;
    if line.last() == Some(&b'\r') {
        line = &line[..line.len() - 1];
    }
    if line.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::OverlongLine);
    }
    if line.iter().any(|&byte| byte == b'\r' || byte == b'\n') {
        return Err(ProtocolError::EmbeddedControl);
    }
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }

    let (command, args) = match line.iter().position(|&byte| byte == b' ') {
        Some(index) => (&line[..index], Some(&line[index + 1..])),
        None => (line, None),
    };

    let event = match command {
        b"WELCOME" => {
            let id = parse_u64(exact_fields(required_args(args)?, 1)?[0])?;
            ServerEvent::Welcome { id }
        },
        b"WOK" => {
            let id = parse_u64(exact_fields(required_args(args)?, 1)?[0])?;
            ServerEvent::Ok { id }
        },
        b"WAIT" => {
            no_args(args)?;
            ServerEvent::Wait
        },
        b"START" => {
            let (fields, peer) = prefix_fields(required_args(args)?, 1)?;
            let side = parse_i32(fields[0])?;
            ServerEvent::Start { side, peer: peer.to_vec() }
        },
        b"FULL" => {
            no_args(args)?;
            ServerEvent::Full
        },
        b"WFULL" => {
            no_args(args)?;
            ServerEvent::WorldFull
        },
        b"PEER_LEFT" => {
            no_args(args)?;
            ServerEvent::PeerLeft
        },
        b"WP" => {
            let (fields, name) = prefix_fields(required_args(args)?, 5)?;
            ServerEvent::Position(PeerPosition {
                id: parse_u64(fields[0])?,
                room: parse_i32(fields[1])?,
                x: parse_i32(fields[2])?,
                y: parse_i32(fields[3])?,
                state: StateBlob::parse(fields[4])?,
                name: name.to_vec(),
            })
        },
        b"WDEL" => {
            let id = parse_u64(exact_fields(required_args(args)?, 1)?[0])?;
            ServerEvent::Delete { id }
        },
        b"WREQ" => {
            let (fields, info) = prefix_fields(required_args(args)?, 2)?;
            ServerEvent::Request { id: parse_u64(fields[0])?, kind: fields[1].to_vec(), info: info.to_vec() }
        },
        b"WANS" => {
            let fields = exact_fields(required_args(args)?, 2)?;
            ServerEvent::Answer { id: parse_u64(fields[0])?, answer: parse_i32(fields[1])? }
        },
        b"WCANCEL" => match args {
            None => ServerEvent::Cancel { id: None },
            Some(rest) if rest.is_empty() => ServerEvent::Cancel { id: None },
            Some(rest) => ServerEvent::Cancel { id: Some(parse_u64(exact_fields(rest, 1)?[0])?) },
        },
        b"IN" => {
            let fields = exact_fields(required_args(args)?, 3)?;
            ServerEvent::Input {
                epoch: parse_i32(fields[0])?,
                frame: parse_i32(fields[1])?,
                mask: parse_i32(fields[2])?,
            }
        },
        b"SUM" => {
            let fields = exact_fields(required_args(args)?, 3)?;
            ServerEvent::Checksum {
                epoch: parse_i32(fields[0])?,
                frame: parse_i32(fields[1])?,
                checksum: parse_i64(fields[2])?,
            }
        },
        b"MSG" => ServerEvent::Message { payload: decode_hex(required_args(args)?)? },
        _ => return Ok(None),
    };
    Ok(Some(event))
}

/// A decoded `MSG` payload subcommand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessagePayload {
    /// `WSAY\x1f<sender-id> <text>`
    Say { sender: u64, text: Vec<u8> },
    /// `OFFER\x1f<header>\x1e<record>`; the record is opaque.
    Offer { header: Vec<u8>, record: Vec<u8> },
    /// `OK\x1f`
    Ok,
    /// `NO\x1f`
    No,
    /// A subcommand this specification does not define.
    Unknown { command: Vec<u8>, body: Vec<u8> },
}

impl MessagePayload {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut payload = Vec::new();
        match self {
            Self::Say { sender, text } => {
                payload.extend_from_slice(b"WSAY");
                payload.push(UNIT_SEPARATOR);
                payload.extend_from_slice(sender.to_string().as_bytes());
                payload.push(b' ');
                payload.extend_from_slice(text);
            },
            Self::Offer { header, record } => {
                check_payload_field(header)?;
                check_payload_field(record)?;
                payload.extend_from_slice(b"OFFER");
                payload.push(UNIT_SEPARATOR);
                payload.extend_from_slice(header);
                payload.push(RECORD_SEPARATOR);
                payload.extend_from_slice(record);
            },
            Self::Ok => {
                payload.extend_from_slice(b"OK");
                payload.push(UNIT_SEPARATOR);
            },
            Self::No => {
                payload.extend_from_slice(b"NO");
                payload.push(UNIT_SEPARATOR);
            },
            Self::Unknown { command, body } => {
                check_payload_field(command)?;
                if command.is_empty() {
                    return Err(ProtocolError::InvalidField);
                }
                payload.extend_from_slice(command);
                payload.push(UNIT_SEPARATOR);
                payload.extend_from_slice(body);
            },
        }
        Ok(payload)
    }
}

/// Parse a decoded `MSG` payload. Unknown subcommands are preserved, not
/// rejected.
pub fn parse_message_payload(payload: &[u8]) -> Result<MessagePayload, ProtocolError> {
    let separator = payload.iter().position(|&byte| byte == UNIT_SEPARATOR).ok_or(ProtocolError::MalformedPayload)?;
    let command = &payload[..separator];
    let body = &payload[separator + 1..];
    if command.is_empty() {
        return Err(ProtocolError::MalformedPayload);
    }

    match command {
        b"WSAY" => {
            let space = body.iter().position(|&byte| byte == b' ').ok_or(ProtocolError::MalformedPayload)?;
            Ok(MessagePayload::Say { sender: parse_u64(&body[..space])?, text: body[space + 1..].to_vec() })
        },
        b"OFFER" => {
            let separator =
                body.iter().position(|&byte| byte == RECORD_SEPARATOR).ok_or(ProtocolError::MalformedPayload)?;
            Ok(MessagePayload::Offer { header: body[..separator].to_vec(), record: body[separator + 1..].to_vec() })
        },
        b"OK" => {
            if !body.is_empty() {
                return Err(ProtocolError::MalformedPayload);
            }
            Ok(MessagePayload::Ok)
        },
        b"NO" => {
            if !body.is_empty() {
                return Err(ProtocolError::MalformedPayload);
            }
            Ok(MessagePayload::No)
        },
        _ => Ok(MessagePayload::Unknown { command: command.to_vec(), body: body.to_vec() }),
    }
}

/// Encode bytes as lowercase hexadecimal.
pub fn encode_hex(bytes: &[u8]) -> String {
    let mut output = Vec::with_capacity(bytes.len() * 2);
    push_hex(bytes, &mut output);
    String::from_utf8(output).expect("hex output is ASCII")
}

/// Decode a `MSG` hexadecimal argument. Odd length and non-hex input are
/// rejected; uppercase and lowercase are both accepted.
pub fn decode_hex(text: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if text.len() % 2 != 0 {
        return Err(ProtocolError::MalformedHex);
    }
    if text.len() / 2 > MAX_MSG_PAYLOAD_BYTES {
        return Err(ProtocolError::MessageTooLarge);
    }
    let mut output = Vec::with_capacity(text.len() / 2);
    for pair in text.chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or(ProtocolError::MalformedHex)?;
        let low = hex_digit(pair[1]).ok_or(ProtocolError::MalformedHex)?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn push_hex(bytes: &[u8], output: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.reserve(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize]);
        output.push(HEX[(byte & 0x0f) as usize]);
    }
}

fn parse_u64(bytes: &[u8]) -> Result<u64, ProtocolError> {
    if bytes.is_empty() {
        return Err(ProtocolError::MalformedNumber);
    }
    let mut value = 0u64;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(ProtocolError::MalformedNumber);
        }
        value = value
            .checked_mul(10)
            .and_then(|current| current.checked_add(u64::from(byte - b'0')))
            .ok_or(ProtocolError::MalformedNumber)?;
    }
    Ok(value)
}

fn parse_i64(bytes: &[u8]) -> Result<i64, ProtocolError> {
    let (negative, digits) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    let magnitude = parse_u64(digits)?;
    if negative {
        if magnitude == (1u64 << 63) {
            Ok(i64::MIN)
        } else if magnitude <= i64::MAX as u64 {
            Ok(-(magnitude as i64))
        } else {
            Err(ProtocolError::MalformedNumber)
        }
    } else if magnitude <= i64::MAX as u64 {
        Ok(magnitude as i64)
    } else {
        Err(ProtocolError::MalformedNumber)
    }
}

fn parse_i32(bytes: &[u8]) -> Result<i32, ProtocolError> {
    let value = parse_i64(bytes)?;
    i32::try_from(value).map_err(|_| ProtocolError::MalformedNumber)
}

/// Parse a finite decimal real. It accepts an optional sign, a decimal point
/// and an exponent, so both observed integers and fractional scales such as
/// `1.5` or `-0.25` validate without being rewritten.
fn parse_real(bytes: &[u8]) -> Result<f64, ProtocolError> {
    if bytes.is_empty() {
        return Err(ProtocolError::MalformedNumber);
    }

    let mut index = 0;
    if matches!(bytes[0], b'+' | b'-') {
        index += 1;
    }
    let integer_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == integer_start {
        return Err(ProtocolError::MalformedNumber);
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == fraction_start {
            return Err(ProtocolError::MalformedNumber);
        }
    }
    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start {
            return Err(ProtocolError::MalformedNumber);
        }
    }
    if index != bytes.len() {
        return Err(ProtocolError::MalformedNumber);
    }

    let text = std::str::from_utf8(bytes).map_err(|_| ProtocolError::MalformedNumber)?;
    let value: f64 = text.parse().map_err(|_| ProtocolError::MalformedNumber)?;
    if !value.is_finite() {
        return Err(ProtocolError::MalformedNumber);
    }
    Ok(value)
}

fn required_args(args: Option<&[u8]>) -> Result<&[u8], ProtocolError> {
    args.filter(|args| !args.is_empty()).ok_or(ProtocolError::MissingField)
}

fn no_args(args: Option<&[u8]>) -> Result<(), ProtocolError> {
    match args {
        None => Ok(()),
        Some(args) if args.is_empty() => Ok(()),
        Some(_) => Err(ProtocolError::ExtraField),
    }
}

/// Split `count` fields, each followed by one space, and return the rest.
fn prefix_fields(input: &[u8], count: usize) -> Result<(Vec<&[u8]>, &[u8]), ProtocolError> {
    let mut fields = Vec::with_capacity(count);
    let mut rest = input;
    for _ in 0..count {
        let index = rest.iter().position(|&byte| byte == b' ').ok_or(ProtocolError::MissingField)?;
        fields.push(&rest[..index]);
        rest = &rest[index + 1..];
    }
    Ok((fields, rest))
}

/// Split exactly `count` space-separated fields and reject any remainder.
fn exact_fields(input: &[u8], count: usize) -> Result<Vec<&[u8]>, ProtocolError> {
    let mut fields = Vec::with_capacity(count);
    let mut rest = input;
    for index in 0..count {
        if index + 1 == count {
            if rest.contains(&b' ') {
                return Err(ProtocolError::ExtraField);
            }
            fields.push(rest);
        } else {
            let position = rest.iter().position(|&byte| byte == b' ').ok_or(ProtocolError::MissingField)?;
            fields.push(&rest[..position]);
            rest = &rest[position + 1..];
        }
    }
    Ok(fields)
}

fn check_text(bytes: &[u8]) -> Result<(), ProtocolError> {
    if bytes.iter().any(|&byte| byte == b'\r' || byte == b'\n') { Err(ProtocolError::EmbeddedControl) } else { Ok(()) }
}

fn check_token(bytes: &[u8]) -> Result<(), ProtocolError> {
    check_text(bytes)?;
    if bytes.is_empty() || bytes.contains(&b' ') { Err(ProtocolError::InvalidField) } else { Ok(()) }
}

fn check_payload_field(bytes: &[u8]) -> Result<(), ProtocolError> {
    if bytes.iter().any(|&byte| byte == UNIT_SEPARATOR || byte == RECORD_SEPARATOR) {
        Err(ProtocolError::InvalidField)
    } else {
        Ok(())
    }
}

fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(value.to_string().as_bytes());
}

fn finish_line(mut line: Vec<u8>) -> Result<Vec<u8>, ProtocolError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::LineTooLong);
    }
    line.push(b'\n');
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(command: ClientCommand) -> Vec<u8> {
        command.encode().expect("encode")
    }

    fn server(text: &str) -> ServerEvent {
        parse_server_line(text.as_bytes()).expect("parse").expect("known command")
    }

    #[test]
    fn limits_keep_compatibility_floors() {
        assert!(MAX_LINE_BYTES >= MIN_LINE_BYTES);
        assert!(MIN_LINE_BYTES >= 8 * 1024);
        assert!(MAX_LINE_BYTES >= 64 * 1024);
        assert!(MAX_MSG_PAYLOAD_BYTES >= MIN_MSG_PAYLOAD_BYTES);
        assert!(MIN_MSG_PAYLOAD_BYTES >= 4 * 1024);
    }

    #[test]
    fn framing_splits_on_lf() {
        let mut buffer = LineBuffer::new();
        let events = buffer.feed(b"WELCOME 7432\nWOK 7432\n");
        assert_eq!(events, vec![LineEvent::Line(b"WELCOME 7432".to_vec()), LineEvent::Line(b"WOK 7432".to_vec()),]);
    }

    #[test]
    fn framing_handles_split_chunks() {
        let mut buffer = LineBuffer::new();
        assert!(buffer.feed(b"WEL").is_empty());
        assert_eq!(buffer.pending_len(), 3);
        let events = buffer.feed(b"COME 1\nWP 2");
        assert_eq!(events, vec![LineEvent::Line(b"WELCOME 1".to_vec())]);
        assert_eq!(buffer.pending_len(), 4);
        assert_eq!(buffer.feed(b" 3\n"), vec![LineEvent::Line(b"WP 2 3".to_vec())]);
    }

    #[test]
    fn framing_strips_cr_before_lf_only() {
        let mut buffer = LineBuffer::new();
        let events = buffer.feed(b"WELCOME 7\r\nWAIT\r\n");
        assert_eq!(events, vec![LineEvent::Line(b"WELCOME 7".to_vec()), LineEvent::Line(b"WAIT".to_vec()),]);
    }

    #[test]
    fn framing_retains_empty_lines() {
        let mut buffer = LineBuffer::new();
        assert_eq!(buffer.feed(b"\n\r\n"), vec![LineEvent::Line(Vec::new()), LineEvent::Line(Vec::new()),]);
        assert_eq!(parse_server_line(b""), Ok(None));
        assert_eq!(parse_server_line(b"\r"), Ok(None));
        assert_eq!(parse_server_line(b"   "), Ok(None));
        assert_eq!(parse_server_line(b"\t"), Ok(None));
    }

    #[test]
    fn framing_accepts_line_at_limit() {
        let mut buffer = LineBuffer::new();
        let mut input = vec![b'a'; MAX_LINE_BYTES];
        input.push(b'\n');
        assert_eq!(buffer.feed(&input), vec![LineEvent::Line(vec![b'a'; MAX_LINE_BYTES])]);
    }

    #[test]
    fn framing_accepts_crlf_at_limit() {
        let mut buffer = LineBuffer::new();
        let mut input = vec![b'a'; MAX_LINE_BYTES];
        input.extend_from_slice(b"\r\n");
        assert_eq!(buffer.feed(&input), vec![LineEvent::Line(vec![b'a'; MAX_LINE_BYTES])]);
    }

    #[test]
    fn framing_rejects_overlong_and_resynchronises() {
        let mut buffer = LineBuffer::new();
        let mut input = vec![b'a'; MAX_LINE_BYTES + 1];
        input.extend_from_slice(b"\nWELCOME 1\n");
        assert_eq!(buffer.feed(&input), vec![LineEvent::Overlong, LineEvent::Line(b"WELCOME 1".to_vec()),]);
        assert!(!buffer.is_discarding());
    }

    #[test]
    fn framing_rejects_overlong_after_tolerated_cr() {
        let mut buffer = LineBuffer::new();
        let mut input = vec![b'a'; MAX_LINE_BYTES];
        input.extend_from_slice(b"\rX\nWELCOME 2\n");
        assert_eq!(buffer.feed(&input), vec![LineEvent::Overlong, LineEvent::Line(b"WELCOME 2".to_vec()),]);
    }

    #[test]
    fn framing_reset_clears_partial_line() {
        let mut buffer = LineBuffer::new();
        buffer.feed(b"WELCOME");
        buffer.reset();
        assert_eq!(buffer.pending_len(), 0);
        assert_eq!(buffer.feed(b"WAIT\n"), vec![LineEvent::Line(b"WAIT".to_vec())]);
    }

    #[test]
    fn handshake_is_three_commands_in_order() {
        let lines = encode_handshake(b"Klisee", WORLD_PROTOCOL_VERSION, WORLD_BUILD).unwrap();
        assert_eq!(
            lines,
            vec![b"WJOIN Klisee\n".to_vec(), b"WVER 8\n".to_vec(), b"WBUILD 20261109\n".to_vec(),]
        );
    }

    #[test]
    fn world_handshake_is_net_world_connect_order() {
        let lines =
            encode_current_world_handshake(b"Klisee", b"00123", Some(b"456")).unwrap();
        assert_eq!(
            lines,
            vec![
                b"WJOIN Klisee\n".to_vec(),
                b"WTID 00123\n".to_vec(),
                b"WTSPR 456\n".to_vec(),
                b"WVER 8\n".to_vec(),
                b"WBUILD 20261109\n".to_vec(),
            ]
        );
    }

    #[test]
    fn world_handshake_omits_sprite_when_absent() {
        let lines = encode_world_handshake(b"Klisee", b"123", None, 8, 20_261_109).unwrap();
        assert_eq!(
            lines,
            vec![
                b"WJOIN Klisee\n".to_vec(),
                b"WTID 123\n".to_vec(),
                b"WVER 8\n".to_vec(),
                b"WBUILD 20261109\n".to_vec(),
            ]
        );
    }

    #[test]
    fn identity_commands_reject_whitespace_and_controls() {
        assert_eq!(client(ClientCommand::WorldTrainerId { id: b"123".to_vec() }), b"WTID 123\n");
        assert_eq!(client(ClientCommand::WorldSprite { sprite: b"-1".to_vec() }), b"WTSPR -1\n");
        assert_eq!(
            ClientCommand::WorldTrainerId { id: b"12 3".to_vec() }.encode(),
            Err(ProtocolError::InvalidField)
        );
        assert_eq!(
            ClientCommand::WorldSprite { sprite: b"1\n2".to_vec() }.encode(),
            Err(ProtocolError::EmbeddedControl)
        );
    }

    #[test]
    fn client_commands_encode() {
        assert_eq!(client(ClientCommand::Sync), b"WSYNC\n");
        assert_eq!(client(ClientCommand::End), b"WEND\n");
        assert_eq!(client(ClientCommand::Bye), b"BYE\n");
        assert_eq!(client(ClientCommand::Version { version: 8 }), b"WVER 8\n");
    }

    #[test]
    fn client_hello_input_checksum_and_world_list_encode() {
        assert_eq!(client(ClientCommand::Hello { name: b"Ana".to_vec() }), b"HELLO Ana\n");
        assert_eq!(client(ClientCommand::Input { epoch: 7, frame: 42, mask: -305_419_896 }), b"IN 7 42 -305419896\n");
        assert_eq!(
            client(ClientCommand::Checksum { epoch: 7, frame: 42, checksum: -12_345_678_901 }),
            b"SUM 7 42 -12345678901\n"
        );
        assert_eq!(client(ClientCommand::WorldList), b"WLIST\n");
        assert_eq!(
            ClientCommand::Hello { name: b"bad\nname".to_vec() }.encode(),
            Err(ProtocolError::EmbeddedControl)
        );
    }

    #[test]
    fn client_position_preserves_state() {
        let state = StateBlob::parse(b"S,452,1,0,0,-1,1;P,25,6,1,15,7,1,1;B").unwrap();
        assert_eq!(
            client(ClientCommand::Position(SelfPosition { room: 26, x: 0, y: 0, state })),
            b"WPOS 26 0 0 S,452,1,0,0,-1,1;P,25,6,1,15,7,1,1;B\n"
        );
    }

    #[test]
    fn client_position_preserves_fractional_scale_state() {
        let token = b"S,25,2,0,0,1.5,-0.75;P,25,4,3,8.5,-12.25,1.5,0.5";
        let state = StateBlob::parse(token).unwrap();
        assert_eq!(state.as_bytes(), token);
        assert_eq!(
            client(ClientCommand::Position(SelfPosition { room: 26, x: 0, y: 0, state })),
            b"WPOS 26 0 0 S,25,2,0,0,1.5,-0.75;P,25,4,3,8.5,-12.25,1.5,0.5\n"
        );
    }

    #[test]
    fn state_blob_accepts_real_values_but_keeps_integer_identity_fields() {
        assert!(StateBlob::parse(b"S,25,2,+0.5,-1.25,1e3,0.75").is_ok());
        assert!(StateBlob::parse(b"P,25,4,3,8.5,-12.25,1.5,0.5").is_ok());
        assert_eq!(StateBlob::parse(b"S,25,2,0,0,nope,1"), Err(ProtocolError::MalformedNumber));
        assert_eq!(StateBlob::parse(b"P,25,4,3.5,8,12,1,1"), Err(ProtocolError::MalformedNumber));
        assert_eq!(StateBlob::parse(b"S,25.5,2,0,0,1,1"), Err(ProtocolError::MalformedNumber));
    }

    #[test]
    fn client_position_none_state() {
        assert_eq!(
            client(ClientCommand::Position(SelfPosition { room: 26, x: 0, y: 0, state: StateBlob::none() })),
            b"WPOS 26 0 0 -\n"
        );
    }

    #[test]
    fn client_position_empty_state_omits_trailing_space() {
        assert_eq!(
            client(ClientCommand::Position(SelfPosition { room: 26, x: 0, y: 0, state: StateBlob::empty() })),
            b"WPOS 26 0 0\n"
        );
    }

    #[test]
    fn client_request_keeps_trailing_empty_info() {
        assert_eq!(
            client(ClientCommand::Request { target: 7395, kind: b"T".to_vec(), info: Vec::new() }),
            b"WREQ 7395 T \n"
        );
        assert_eq!(
            client(ClientCommand::Request { target: 7461, kind: b"T".to_vec(), info: b"hello there".to_vec() }),
            b"WREQ 7461 T hello there\n"
        );
    }

    #[test]
    fn client_answer_and_cancel_encode() {
        assert_eq!(client(ClientCommand::Answer { request: 9, answer: 1 }), b"WANS 9 1\n");
        assert_eq!(client(ClientCommand::Cancel { id: None }), b"WCANCEL\n");
        assert_eq!(client(ClientCommand::Cancel { id: Some(9) }), b"WCANCEL 9\n");
    }

    #[test]
    fn client_say_keeps_spaces_and_rejects_line_breaks() {
        assert_eq!(client(ClientCommand::Say { text: b"hello there".to_vec() }), b"WSAY hello there\n");
        assert_eq!(ClientCommand::Say { text: b"bad\nline".to_vec() }.encode(), Err(ProtocolError::EmbeddedControl));
        assert_eq!(ClientCommand::Join { name: b"bad\rname".to_vec() }.encode(), Err(ProtocolError::EmbeddedControl));
    }

    #[test]
    fn client_request_rejects_multitoken_kind() {
        assert_eq!(
            ClientCommand::Request { target: 1, kind: b"T T".to_vec(), info: Vec::new() }.encode(),
            Err(ProtocolError::InvalidField)
        );
    }

    #[test]
    fn client_message_uses_lowercase_hex() {
        assert_eq!(client(ClientCommand::Message { payload: b"OK\x1f".to_vec() }), b"MSG 4f4b1f\n");
    }

    #[test]
    fn hex_round_trip_accepts_both_cases() {
        assert_eq!(decode_hex(b"4f4B1f"), Ok(b"OK\x1f".to_vec()));
        assert_eq!(encode_hex(b"OK\x1f"), "4f4b1f");
    }

    #[test]
    fn hex_rejects_odd_and_non_hex() {
        assert_eq!(decode_hex(b"4f4"), Err(ProtocolError::MalformedHex));
        assert_eq!(decode_hex(b"4g"), Err(ProtocolError::MalformedHex));
    }

    #[test]
    fn welcome_and_ok_use_full_u64_ids() {
        assert_eq!(server("WELCOME 7432"), ServerEvent::Welcome { id: 7432 });
        assert_eq!(server("WELCOME 18446744073709551615"), ServerEvent::Welcome { id: u64::MAX });
        assert_eq!(server("WOK 7466"), ServerEvent::Ok { id: 7466 });
    }

    #[test]
    fn no_argument_server_commands_parse() {
        assert_eq!(server("WAIT"), ServerEvent::Wait);
        assert_eq!(server("FULL"), ServerEvent::Full);
        assert_eq!(server("WFULL"), ServerEvent::WorldFull);
        assert_eq!(server("PEER_LEFT"), ServerEvent::PeerLeft);
    }

    #[test]
    fn start_keeps_rest_of_line_as_peer_name() {
        assert_eq!(server("START 1 Blaze"), ServerEvent::Start { side: 1, peer: b"Blaze".to_vec() });
        assert_eq!(server("START 2 A B"), ServerEvent::Start { side: 2, peer: b"A B".to_vec() });
    }

    #[test]
    fn wp_parses_snapshot_and_preserves_name_and_state() {
        let event = server("WP 7395 2 145 130 S,1862,1,0,0,-1,1 Blaze");
        match event {
            ServerEvent::Position(position) => {
                assert_eq!(position.id, 7395);
                assert_eq!(position.room, 2);
                assert_eq!(position.x, 145);
                assert_eq!(position.y, 130);
                assert_eq!(position.state.as_bytes(), b"S,1862,1,0,0,-1,1");
                assert_eq!(position.name, b"Blaze");
            },
            other => panic!("unexpected event: {other:?}"),
        }

        let event = server("WP 1 26 10 20 S,1,2,3,4,5,6 Some One");
        match event {
            ServerEvent::Position(position) => {
                assert_eq!(position.name, b"Some One");
            },
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn wdel_parses_id() {
        assert_eq!(server("WDEL 7210"), ServerEvent::Delete { id: 7210 });
    }

    #[test]
    fn relayed_request_answer_and_cancel_parse() {
        assert_eq!(server("WREQ 7395 T "), ServerEvent::Request { id: 7395, kind: b"T".to_vec(), info: Vec::new() });
        assert_eq!(
            server("WREQ 7 X payload with spaces"),
            ServerEvent::Request { id: 7, kind: b"X".to_vec(), info: b"payload with spaces".to_vec() }
        );
        assert_eq!(server("WANS 9 0"), ServerEvent::Answer { id: 9, answer: 0 });
        assert_eq!(server("WANS 9 -1"), ServerEvent::Answer { id: 9, answer: -1 });
        assert_eq!(server("WCANCEL"), ServerEvent::Cancel { id: None });
        assert_eq!(server("WCANCEL 9"), ServerEvent::Cancel { id: Some(9) });
    }

    #[test]
    fn server_input_and_checksum_parse() {
        assert_eq!(server("IN 7 42 -305419896"), ServerEvent::Input { epoch: 7, frame: 42, mask: -305_419_896 });
        assert_eq!(
            server("SUM 7 42 305419896"),
            ServerEvent::Checksum { epoch: 7, frame: 42, checksum: 305_419_896 }
        );
        assert_eq!(
            server("SUM -1 -2 -9223372036854775808"),
            ServerEvent::Checksum { epoch: -1, frame: -2, checksum: i64::MIN }
        );
        assert_eq!(parse_server_line(b"IN 1 2"), Err(ProtocolError::MissingField));
        assert_eq!(parse_server_line(b"SUM 1 2"), Err(ProtocolError::MissingField));
        assert_eq!(parse_server_line(b"IN 1 2 3 4"), Err(ProtocolError::ExtraField));
        assert_eq!(parse_server_line(b"SUM 1 nope 3"), Err(ProtocolError::MalformedNumber));
    }

    #[test]
    fn server_msg_decodes_hex() {
        assert_eq!(server("MSG 4f4b1f"), ServerEvent::Message { payload: b"OK\x1f".to_vec() });
    }

    #[test]
    fn unknown_and_lowercase_commands_are_ignored() {
        assert_eq!(parse_server_line(b"GET / HTTP/1.1"), Ok(None));
        assert_eq!(parse_server_line(b"HELLO 1"), Ok(None));
        assert_eq!(parse_server_line(b"wp 1 2 3 4 - n"), Ok(None));
        assert_eq!(parse_server_line(b"WELCOME 1 extra"), Err(ProtocolError::ExtraField));
        assert_eq!(parse_server_line(b"WELCOME"), Err(ProtocolError::MissingField));
    }

    #[test]
    fn malformed_numeric_fields_are_rejected() {
        assert_eq!(parse_server_line(b"WELCOME nope"), Err(ProtocolError::MalformedNumber));
        assert_eq!(parse_server_line(b"WP 1 2 x 4 - n"), Err(ProtocolError::MalformedNumber));
        assert_eq!(parse_server_line(b"WELCOME -1"), Err(ProtocolError::MalformedNumber));
    }

    #[test]
    fn signed_room_and_coordinates_are_accepted() {
        match server("WP 1 -2 -145 130 S,1,2,3,4,5,6 n") {
            ServerEvent::Position(position) => {
                assert_eq!(position.room, -2);
                assert_eq!(position.x, -145);
                assert_eq!(position.y, 130);
            },
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn embedded_cr_is_rejected() {
        assert_eq!(parse_server_line(b"WELCOME 1\rWAIT"), Err(ProtocolError::EmbeddedControl));
    }

    #[test]
    fn state_blob_preserves_examples() {
        for token in
            [b"S,2261,1,0,0,1,1".as_slice(), b"S,452,1,0,0,-1,1;P,25,6,1,15,7,1,1;B".as_slice(), b"-".as_slice()]
        {
            let blob = StateBlob::parse(token).unwrap();
            assert_eq!(blob.as_bytes(), token);
        }
    }

    #[test]
    fn state_blob_keeps_unknown_tags() {
        let blob = StateBlob::parse(b"Z,1,2;S,1,2,3,4,5,6;Q").unwrap();
        assert_eq!(blob.segments().len(), 3);
        assert_eq!(blob.segments()[0], StateSegment::Unknown { tag: b'Z', fields: vec![b"1".to_vec(), b"2".to_vec()] });
        assert_eq!(
            blob.segments()[1],
            StateSegment::Main {
                fields: [b"1", b"2", b"3", b"4", b"5", b"6"].iter().map(|field| field.to_vec()).collect(),
            }
        );
        assert_eq!(blob.segments()[2], StateSegment::Unknown { tag: b'Q', fields: Vec::new() });
    }

    #[test]
    fn state_blob_requires_known_field_counts() {
        assert_eq!(StateBlob::parse(b"S,1,2,3,4,5"), Err(ProtocolError::MalformedState));
        assert_eq!(StateBlob::parse(b"P,1,2,3,4,5,6"), Err(ProtocolError::MalformedState));
        assert!(StateBlob::parse(b"S,1,2,3,4,5,6,extra,opaque").is_ok());
    }

    #[test]
    fn state_blob_rejects_malformed_known_segments() {
        assert_eq!(StateBlob::parse(b"S,1,2,3,4,5,x"), Err(ProtocolError::MalformedNumber));
        assert_eq!(StateBlob::parse(b"B,1"), Err(ProtocolError::MalformedState));
        assert_eq!(StateBlob::parse(b"S,1,2,3,4,5,6;;B"), Err(ProtocolError::MalformedState));
        assert_eq!(StateBlob::parse(b"S,1,2,3,4,5,6;-"), Err(ProtocolError::MalformedState));
        assert_eq!(StateBlob::parse(b"S,1,2,3,4,5,6;B;"), Err(ProtocolError::MalformedState));
        assert_eq!(StateBlob::parse(b"S,1,2,3,4,5,6;B "), Err(ProtocolError::MalformedState));
        assert_eq!(StateBlob::parse(b""), Err(ProtocolError::MalformedState));
    }

    #[test]
    fn state_blob_accepts_more_than_three_segments() {
        let token = b"S,1,2,3,4,5,6;P,1,2,3,4,5,6,7;B;B;S,1,2,3,4,5,6";
        let blob = StateBlob::parse(token).unwrap();
        assert_eq!(blob.segments().len(), 5);
        assert_eq!(blob.as_bytes(), token);
    }

    #[test]
    fn payload_say_keeps_rest_of_payload() {
        assert_eq!(
            parse_message_payload(b"WSAY\x1f7395 hello there"),
            Ok(MessagePayload::Say { sender: 7395, text: b"hello there".to_vec() })
        );
        assert_eq!(parse_message_payload(b"WSAY\x1f7 "), Ok(MessagePayload::Say { sender: 7, text: Vec::new() }));
    }

    #[test]
    fn payload_offer_is_opaque() {
        assert_eq!(
            parse_message_payload(b"OFFER\x1fLucario Nv. 100\x1er1.000000\x1dshello"),
            Ok(MessagePayload::Offer { header: b"Lucario Nv. 100".to_vec(), record: b"r1.000000\x1dshello".to_vec() })
        );
    }

    #[test]
    fn payload_ok_no_and_unknown() {
        assert_eq!(parse_message_payload(b"OK\x1f"), Ok(MessagePayload::Ok));
        assert_eq!(parse_message_payload(b"NO\x1f"), Ok(MessagePayload::No));
        assert_eq!(
            parse_message_payload(b"FUTURE\x1f1\x1e2"),
            Ok(MessagePayload::Unknown { command: b"FUTURE".to_vec(), body: b"1\x1e2".to_vec() })
        );
    }

    #[test]
    fn payload_rejects_malformed_shapes() {
        assert_eq!(parse_message_payload(b"OK"), Err(ProtocolError::MalformedPayload));
        assert_eq!(parse_message_payload(b"WSAY\x1fnotanumber hi"), Err(ProtocolError::MalformedNumber));
        assert_eq!(parse_message_payload(b"WSAY\x1f7nospace"), Err(ProtocolError::MalformedPayload));
        assert_eq!(parse_message_payload(b"OFFER\x1fheader-only"), Err(ProtocolError::MalformedPayload));
        assert_eq!(parse_message_payload(b"OK\x1fextra"), Err(ProtocolError::MalformedPayload));
        assert_eq!(parse_message_payload(b"\x1fbody"), Err(ProtocolError::MalformedPayload));
    }

    #[test]
    fn payload_encode_round_trips() {
        let messages = [
            MessagePayload::Say { sender: 7395, text: b"hello there".to_vec() },
            MessagePayload::Offer { header: b"Lucario Nv. 100".to_vec(), record: b"r1.000000\x1dshello".to_vec() },
            MessagePayload::Ok,
            MessagePayload::No,
            MessagePayload::Unknown { command: b"FUTURE".to_vec(), body: b"1\x1e2".to_vec() },
        ];
        for message in messages {
            let encoded = message.encode().unwrap();
            assert_eq!(parse_message_payload(&encoded), Ok(message));
        }
    }

    #[test]
    fn payload_encode_rejects_separators_in_fields() {
        assert_eq!(
            MessagePayload::Offer { header: b"bad\x1eheader".to_vec(), record: Vec::new() }.encode(),
            Err(ProtocolError::InvalidField)
        );
        assert_eq!(
            MessagePayload::Unknown { command: Vec::new(), body: Vec::new() }.encode(),
            Err(ProtocolError::InvalidField)
        );
    }

    #[test]
    fn encoded_client_message_fits_the_line_cap() {
        let payload = vec![0u8; MAX_MSG_PAYLOAD_BYTES];
        let encoded = ClientCommand::Message { payload: payload.clone() }.encode().unwrap();
        assert_eq!(encoded.len(), MAX_LINE_BYTES + 1);
        assert_eq!(encoded.last(), Some(&b'\n'));
        assert_eq!(decode_hex(&encoded[4..encoded.len() - 1]).unwrap(), payload);

        let too_big = ClientCommand::Message { payload: vec![0u8; MAX_MSG_PAYLOAD_BYTES + 1] }.encode();
        assert_eq!(too_big, Err(ProtocolError::MessageTooLarge));
    }

    #[test]
    fn oversized_text_line_is_rejected() {
        let text = vec![b'a'; MAX_LINE_BYTES];
        assert_eq!(ClientCommand::Say { text }.encode(), Err(ProtocolError::LineTooLong));
    }

    #[test]
    fn wp_with_full_u64_id_and_negative_state_numbers() {
        match server("WP 18446744073709551615 1 2 3 S,-9,0,-1,0,1,-2;P,-1,-2,-3,-4,-5,-6,-7;B name") {
            ServerEvent::Position(position) => {
                assert_eq!(position.id, u64::MAX);
                assert_eq!(position.state.as_bytes(), b"S,-9,0,-1,0,1,-2;P,-1,-2,-3,-4,-5,-6,-7;B");
            },
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn leading_zeros_and_zero_are_valid_decimals() {
        assert_eq!(server("WOK 0007"), ServerEvent::Ok { id: 7 });
        assert_eq!(server("WDEL 0"), ServerEvent::Delete { id: 0 });
    }

    #[test]
    fn missing_and_extra_fields_are_rejected() {
        assert_eq!(parse_server_line(b"START"), Err(ProtocolError::MissingField));
        assert_eq!(parse_server_line(b"START 1"), Err(ProtocolError::MissingField));
        assert_eq!(parse_server_line(b"WP 1 2 3 4 -"), Err(ProtocolError::MissingField));
        assert_eq!(parse_server_line(b"WAIT extra"), Err(ProtocolError::ExtraField));
        assert_eq!(parse_server_line(b"WOK 1 2"), Err(ProtocolError::ExtraField));
    }

    #[test]
    fn direct_parser_strips_one_trailing_cr() {
        assert_eq!(parse_server_line(b"WELCOME 12\r"), Ok(Some(ServerEvent::Welcome { id: 12 })));
    }

    #[test]
    fn direct_parser_enforces_line_cap() {
        let mut line = vec![b'W'; MAX_LINE_BYTES + 1];
        assert_eq!(parse_server_line(&line), Err(ProtocolError::OverlongLine));
        line.truncate(MAX_LINE_BYTES);
        assert_eq!(parse_server_line(&line), Ok(None));
    }

    #[test]
    fn line_buffer_feed_returns_only_complete_lines() {
        let mut buffer = LineBuffer::new();
        assert!(buffer.feed(b"").is_empty());
        assert!(buffer.feed(b"WELCOME 1").is_empty());
        assert_eq!(buffer.pending_len(), 9);
        assert_eq!(buffer.feed(b"\n"), vec![LineEvent::Line(b"WELCOME 1".to_vec())]);
        assert_eq!(buffer.pending_len(), 0);
    }
}
