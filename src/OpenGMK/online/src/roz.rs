//! Client for the Zibelan/ROZ line protocol.
//!
//! Transport is plain TCP with one UTF-8/ASCII line per command, terminated by `\n`.
//! Incoming lines may end in `\r\n`; the trailing `\r` is stripped. There is no TLS,
//! length prefix, JSON or binary framing.
//!
//! Client -> server commands implemented here:
//! `HELLO`, `JOIN`, `MSG`, `IN`, `WJOIN`, `WPOS`, `WSYNC`, `WREQ`, `WANS`,
//! `WCANCEL`, `WSAY`, `WLIST`, `WEND`, `BYE`.
//!
//! Server -> client commands: `WELCOME`, `WAIT`, `START`, `WOK`, `FULL`, `WFULL`,
//! `PEER_LEFT`, `MSG` (hex), `IN`, `SUM`, `WP`, `WDEL`, `WLIST`, `WREQ`, `WANS`,
//! `WCANCEL`, `WSAY`, `WEXC`.
//!
//! `MSG` framing:
//! - Server -> client `MSG` carries lowercase hex and is decoded here.
//! - Client -> server `MSG` carries lowercase hex too, so CR/LF, NUL and non-UTF-8
//!   bytes cannot break the line framing. [`RozClient::send`] and
//!   [`RozClient::send_message_bytes`] hex-encode their payload.
//!   [`RozClient::send_raw_line`] remains the raw escape hatch used by `net_line`.
//!
//! Wire encoding and parsing go through [`reloaded_cleanroom::roz`] and
//! [`reloaded_cleanroom::world`]. `JOIN`, `WEXC` and raw server `WSAY` are
//! handled locally because the clean-room codec does not define them.
//! Non-canonical forms that the codec rejects (`WOK -1`, `WANS 9 +1`,
//! `WREQ 4 F`, `START 1`, extra trailing spaces, malformed `WELCOME`, loose
//! `WPOS` parts, non-integer `IN`/`SUM` values) keep the historical
//! best-effort fallback so existing peers and callers observe the same wire
//! behaviour. `WREQ`/`WANS`/`WCANCEL` payloads remain opaque, as the
//! specification requires.

use reloaded_cleanroom::{
    roz::{ClientCommand, ServerEvent},
    world,
};
use std::{
    collections::VecDeque,
    fmt,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Adapter over the clean-room ROZ codec.
///
/// The codec owns framing, command encoding and server parsing. These helpers
/// only map host-facing strings to the typed codec model and apply the
/// compatibility rule that outgoing CR/LF become spaces.
mod adapter {
    use reloaded_cleanroom::roz::{self as codec, ClientCommand, ProtocolError, ServerEvent, StateBlob};

    pub(super) use codec::{LineBuffer, LineEvent};

    pub(super) fn encode_command(command: &ClientCommand) -> Result<Vec<u8>, ProtocolError> {
        command.encode().map(|mut line| {
            line.pop();
            line
        })
    }

    pub(super) fn parse_server_line(line: &[u8]) -> Result<Option<ServerEvent>, ProtocolError> {
        codec::parse_server_line(line)
    }

    pub(super) fn position(room: &str, x: &str, y: &str, parts: &str) -> Result<Vec<u8>, String> {
        let room = canonical_i32(room, "room")?;
        let x = canonical_i32(x, "x")?;
        let y = canonical_i32(y, "y")?;
        let state = if parts.is_empty() {
            StateBlob::empty()
        } else {
            StateBlob::parse(parts.as_bytes()).map_err(|error| format!("parts inválidos: {error}"))?
        };
        encode_command(&ClientCommand::Position(codec::SelfPosition { room, x, y, state }))
            .map_err(|error| format!("WPOS inválido: {error}"))
    }

    pub(super) fn request(target: &str, kind: &str, info: &str) -> Result<Vec<u8>, String> {
        let target = canonical_u64(target, "target")?;
        encode_command(&ClientCommand::Request {
            target,
            kind: kind.as_bytes().to_vec(),
            info: info.as_bytes().to_vec(),
        })
        .map_err(|error| format!("WREQ inválido: {error}"))
    }

    pub(super) fn response(request: &str, answer: &str) -> Result<Vec<u8>, String> {
        let request = canonical_u64(request, "request_id")?;
        let answer = canonical_i32(answer, "answer")?;
        encode_command(&ClientCommand::Answer { request, answer })
            .map_err(|error| format!("WANS inválido: {error}"))
    }

    pub(super) fn input(epoch: &str, frame: &str, mask: &str) -> Result<Vec<u8>, String> {
        let epoch = canonical_i32(epoch, "epoch")?;
        let frame = canonical_i32(frame, "frame")?;
        let mask = canonical_i32(mask, "mask")?;
        encode_command(&ClientCommand::Input { epoch, frame, mask })
            .map_err(|error| format!("IN inválido: {error}"))
    }

    pub(super) fn checksum(epoch: &str, frame: &str, checksum: &str) -> Result<Vec<u8>, String> {
        let epoch = canonical_i32(epoch, "epoch")?;
        let frame = canonical_i32(frame, "frame")?;
        let checksum = canonical_i64(checksum, "checksum")?;
        encode_command(&ClientCommand::Checksum { epoch, frame, checksum })
            .map_err(|error| format!("SUM inválido: {error}"))
    }

    pub(super) fn sanitize_bytes(bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .map(|&byte| if byte == b'\r' || byte == b'\n' { b' ' } else { byte })
            .collect()
    }

    fn canonical_u64(text: &str, field: &str) -> Result<u64, String> {
        let value = text.parse::<u64>().map_err(|_| format!("{field} inválido: {text}"))?;
        if value.to_string() == text {
            Ok(value)
        } else {
            Err(format!("{field} no canónico: {text}"))
        }
    }

    fn canonical_i32(text: &str, field: &str) -> Result<i32, String> {
        let value = text.parse::<i32>().map_err(|_| format!("{field} inválido: {text}"))?;
        if value.to_string() == text {
            Ok(value)
        } else {
            Err(format!("{field} no canónico: {text}"))
        }
    }

    fn canonical_i64(text: &str, field: &str) -> Result<i64, String> {
        let value = text.parse::<i64>().map_err(|_| format!("{field} inválido: {text}"))?;
        if value.to_string() == text {
            Ok(value)
        } else {
            Err(format!("{field} no canónico: {text}"))
        }
    }
}

/// Maximum accepted line length, including the command but excluding the final LF.
pub const MAX_LINE: usize = reloaded_cleanroom::roz::MAX_LINE_BYTES;

/// Default timeout used by [`RozClient::connect`].
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default number of events retained before the oldest one is dropped.
pub const DEFAULT_EVENT_CAPACITY: usize = 256;

/// Bound for a single `send_line`/`flush`, so a stuck peer cannot block the game
/// thread forever. The public server does not document its own send timeout.
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Connection/session state, mirroring the values observed in `net_status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RozStatus {
    /// Not connected, or the reader stopped/errored.
    Disconnected = 0,
    /// Socket created / handshake pending.
    Connecting = 1,
    /// `WAIT`: lobby waiting for a peer.
    Waiting = 2,
    /// `START`: lobby session ready.
    Started = 3,
    /// `PEER_LEFT`: the peer disappeared.
    PeerLeft = 4,
    /// `FULL` or `WFULL`.
    Full = 5,
    /// `WOK`: world joined.
    WorldJoined = 6,
}

impl RozStatus {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Connecting,
            2 => Self::Waiting,
            3 => Self::Started,
            4 => Self::PeerLeft,
            5 => Self::Full,
            6 => Self::WorldJoined,
            _ => Self::Disconnected,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl Default for RozStatus {
    fn default() -> Self {
        Self::Disconnected
    }
}

impl fmt::Display for RozStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::Waiting => "Waiting",
            Self::Started => "Started",
            Self::PeerLeft => "PeerLeft",
            Self::Full => "Full",
            Self::WorldJoined => "WorldJoined",
        };
        f.write_str(name)
    }
}

/// A `WP <id> <room> <x> <y> <parts> <name>` update from the server.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldPosition {
    pub id: u64,
    pub room: i32,
    pub x: f64,
    pub y: f64,
    pub parts: String,
    pub name: String,
    /// Name bytes exactly as received, before any lossy display conversion.
    pub name_bytes: Vec<u8>,
    /// Parsed parts, owned by the clean-room world module.
    pub decoded: world::Parts,
    /// Original parts token bytes.
    pub parts_token: Vec<u8>,
    /// Original bytes after the `WP` command, including its leading space.
    ///
    /// Keeping the exact remainder preserves the server's spacing and non-UTF-8
    /// name bytes instead of re-formatting numbers.
    pub raw: Vec<u8>,
}

/// One entry of a `WLIST` response.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldListEntry {
    pub name: String,
    pub place: String,
    pub flags: String,
}

/// Events produced by the reader thread.
#[derive(Clone, Debug, PartialEq)]
pub enum RozEvent {
    /// `WELCOME <id>` greeting; the numeric value is the connection identity.
    Welcome {
        /// Protocol-banner value expected by the host (the same number).
        version: u32,
        /// Connection identity used to suppress self updates.
        id: u64,
    },
    /// `WAIT`.
    Wait,
    /// `START <side> <peer>`.
    Start { side: i32, peer: String },
    /// `WOK <id>`. The host still exposes it as the session side.
    WorldJoined {
        side: i32,
        /// Connection identity when the value is a non-negative integer.
        id: Option<u64>,
    },
    /// `FULL` (`world = false`) or `WFULL` (`world = true`).
    Full { world: bool },
    /// `PEER_LEFT`.
    PeerLeft,
    /// `MSG <hex>` decoded to its original bytes.
    Message(Vec<u8>),
    /// `IN <epoch> <frame> <mask>`.
    Input { epoch: i32, frame: i32, mask: i32 },
    /// `SUM <epoch> <frame> <checksum>`.
    Checksum { epoch: i32, frame: i32, checksum: i64 },
    /// `WP ...`.
    WorldPosition(WorldPosition),
    /// Incoming `WPOS ...` (normally client-to-server); parsed for completeness.
    WorldSelfPosition(world::WposMessage),
    /// `WDEL <id>`.
    WorldDelete {
        id: u64,
        /// Original bytes after the `WDEL` command, including its leading space.
        raw: Vec<u8>,
    },
    /// `WREQ <id> <kind> <payload>`; payload is kept opaque on purpose.
    WorldRequest { id: u64, kind: String, payload: String },
    /// `WANS <id> <answer>`.
    WorldAnswer { id: u64, answer: i32 },
    /// `WCANCEL` or `WCANCEL <id>`.
    WorldCancel { id: Option<u64> },
    /// `WSAY <id> <text>` chat message.
    WorldSay { id: u64, text: String },
    /// `WEXC <id> <number>` emote.
    WorldExclaim { id: u64, kind: i32 },
    /// `WLIST <total>` followed by entries.
    WorldList { total: i32, entries: Vec<WorldListEntry> },
    /// A line that looked like a known command but could not be parsed.
    Malformed { line: String, reason: String },
    /// The reader stopped (EOF, network error, or local disconnect).
    Disconnected { reason: String },
}

struct QueueState {
    events: VecDeque<RozEvent>,
    closed: bool,
}

struct EventQueue {
    capacity: usize,
    state: Mutex<QueueState>,
    ready: Condvar,
}

impl EventQueue {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(QueueState { events: VecDeque::new(), closed: false }),
            ready: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, QueueState> {
        lock_unpoison(&self.state)
    }

    fn push(&self, event: RozEvent) {
        let mut state = self.lock();
        if state.closed {
            return;
        }
        while state.events.len() >= self.capacity {
            state.events.pop_front();
        }
        state.events.push_back(event);
        self.ready.notify_one();
    }

    fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        self.ready.notify_all();
    }

    fn try_pop(&self) -> Option<RozEvent> {
        self.lock().events.pop_front()
    }

    fn is_closed(&self) -> bool {
        self.lock().closed
    }

    fn pop_timeout(&self, timeout: Duration) -> Option<RozEvent> {
        // `Instant::now() + timeout` can overflow for huge user-supplied durations.
        // When that happens, wait in bounded slices instead of panicking.
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.lock();
        loop {
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            if state.closed {
                return None;
            }
            let now = Instant::now();
            let remaining = match deadline {
                Some(deadline) if now >= deadline => return None,
                Some(deadline) => deadline.saturating_duration_since(now),
                None => Duration::from_secs(3600),
            };
            let (next, _) = self
                .ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }
    }

    fn pop_blocking(&self) -> Option<RozEvent> {
        let mut state = self.lock();
        loop {
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            if state.closed {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

fn lock_unpoison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A connected ROZ client with a dedicated reader thread and a bounded event queue.
pub struct RozClient {
    writer: Mutex<TcpStream>,
    reader: Mutex<Option<JoinHandle<()>>>,
    events: Arc<EventQueue>,
    status: Arc<AtomicU8>,
    side: Arc<AtomicI32>,
    peer: Arc<Mutex<String>>,
    disconnecting: AtomicBool,
    local_addr: Option<SocketAddr>,
    peer_addr: Option<SocketAddr>,
}

impl RozClient {
    /// Connect with the default timeout and event capacity.
    pub fn connect(address: &str) -> Result<Self, String> {
        Self::connect_with_timeout(address, DEFAULT_CONNECT_TIMEOUT)
    }

    /// Connect with a custom timeout and the default event capacity.
    pub fn connect_with_timeout(address: &str, timeout: Duration) -> Result<Self, String> {
        Self::connect_with_options(address, timeout, DEFAULT_EVENT_CAPACITY)
    }

    /// Connect with a custom timeout and event queue capacity.
    pub fn connect_with_options(
        address: &str,
        timeout: Duration,
        event_capacity: usize,
    ) -> Result<Self, String> {
        let addresses = address
            .to_socket_addrs()
            .map_err(|error| format!("dirección inválida ({address}): {error}"))?;
        let mut last_error = None;
        for addr in addresses {
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(stream) => return Self::from_stream(stream, event_capacity),
                Err(error) => last_error = Some(error),
            }
        }
        Err(match last_error {
            Some(error) => format!("no pude conectar a {address}: {error}"),
            None => format!("{address} no resolvió a ninguna dirección"),
        })
    }

    fn from_stream(stream: TcpStream, event_capacity: usize) -> Result<Self, String> {
        stream.set_nodelay(true).ok();
        stream.set_write_timeout(Some(DEFAULT_WRITE_TIMEOUT)).ok();
        let reader_stream = stream
            .try_clone()
            .map_err(|error| format!("clonando socket: {error}"))?;
        let local_addr = stream.local_addr().ok();
        let peer_addr = stream.peer_addr().ok();

        let events = Arc::new(EventQueue::new(event_capacity));
        let status = Arc::new(AtomicU8::new(RozStatus::Connecting.as_u8()));
        let side = Arc::new(AtomicI32::new(0));
        let peer = Arc::new(Mutex::new(String::new()));

        let reader = {
            let events = Arc::clone(&events);
            let status = Arc::clone(&status);
            let side = Arc::clone(&side);
            let peer = Arc::clone(&peer);
            thread::Builder::new()
                .name("roz-reader".into())
                .spawn(move || reader_loop(reader_stream, events, status, side, peer))
                .map_err(|error| format!("no pude iniciar el hilo lector: {error}"))?
        };

        Ok(Self {
            writer: Mutex::new(stream),
            reader: Mutex::new(Some(reader)),
            events,
            status,
            side,
            peer,
            disconnecting: AtomicBool::new(false),
            local_addr,
            peer_addr,
        })
    }

    /// Send `HELLO <name>`.
    pub fn hello(&self, name: &str) -> Result<(), String> {
        self.hello_bytes(name.as_bytes())
    }

    /// Send `HELLO <name>` from raw GML bytes.
    pub fn hello_bytes(&self, name: &[u8]) -> Result<(), String> {
        let name = adapter::sanitize_bytes(name);
        self.send_command(ClientCommand::Hello { name })
    }

    /// Send `JOIN <room>`.
    pub fn join(&self, room: &str) -> Result<(), String> {
        self.join_bytes(room.as_bytes())
    }

    /// Send `JOIN <room>` from raw GML bytes.
    pub fn join_bytes(&self, room: &[u8]) -> Result<(), String> {
        let mut line = Vec::with_capacity(5 + room.len());
        line.extend_from_slice(b"JOIN ");
        line.extend_from_slice(room);
        self.send_line_bytes(&line)
    }

    /// Send `WJOIN <name>`.
    pub fn world_join(&self, name: &str) -> Result<(), String> {
        let name = adapter::sanitize_bytes(name.as_bytes());
        self.send_command(ClientCommand::Join { name })
    }

    /// Send `MSG <hex>` for a UTF-8/text payload.
    ///
    /// Hex keeps CR/LF and non-UTF-8 bytes from breaking the line framing.
    pub fn send(&self, payload: &str) -> Result<(), String> {
        self.send_message_bytes(payload.as_bytes())
    }

    /// Send `MSG <hex>` for a raw byte payload.
    ///
    /// Binary and text payloads are preserved byte for byte. The line is
    /// rejected locally when the hex form would exceed [`MAX_LINE`].
    pub fn send_message_bytes(&self, payload: &[u8]) -> Result<(), String> {
        let encoded_len = payload
            .len()
            .checked_mul(2)
            .and_then(|len| len.checked_add(4))
            .ok_or_else(|| "mensaje demasiado largo".to_string())?;
        if encoded_len > MAX_LINE {
            return Err(format!(
                "mensaje demasiado largo ({} bytes codificados, máximo {MAX_LINE})",
                encoded_len
            ));
        }
        self.send_command(ClientCommand::Message { payload: payload.to_vec() })
    }

    /// Send an arbitrary protocol line, without adding a command prefix.
    ///
    /// This is the escape hatch used by `net_line`. CR/LF are sanitized and a single
    /// trailing LF is added.
    pub fn send_raw_line(&self, line: &str) -> Result<(), String> {
        self.send_raw_line_bytes(line.as_bytes())
    }

    /// Send an arbitrary protocol line from raw GML bytes.
    ///
    /// CR/LF are replaced with spaces so one call can never produce more than one
    /// protocol line.
    pub fn send_raw_line_bytes(&self, line: &[u8]) -> Result<(), String> {
        self.send_line_bytes(line)
    }

    /// Send `IN <epoch> <frame> <mask>`.
    pub fn input_send<E, F, M>(&self, epoch: E, frame: F, mask: M) -> Result<(), String>
    where
        E: fmt::Display,
        F: fmt::Display,
        M: fmt::Display,
    {
        let epoch = epoch.to_string();
        let frame = frame.to_string();
        let mask = mask.to_string();
        self.send_encoded_or(adapter::input(&epoch, &frame, &mask), || {
            self.send_line(&format!("IN {epoch} {frame} {mask}"))
        })
    }

    /// Send `SUM <epoch> <frame> <checksum>`.
    pub fn checksum_send<E, F, C>(&self, epoch: E, frame: F, checksum: C) -> Result<(), String>
    where
        E: fmt::Display,
        F: fmt::Display,
        C: fmt::Display,
    {
        let epoch = epoch.to_string();
        let frame = frame.to_string();
        let checksum = checksum.to_string();
        self.send_encoded_or(adapter::checksum(&epoch, &frame, &checksum), || {
            self.send_line(&format!("SUM {epoch} {frame} {checksum}"))
        })
    }

    /// Send `WPOS <room> <x> <y> <parts>`.
    pub fn world_position<R, X, Y>(
        &self,
        room: R,
        x: X,
        y: Y,
        parts: &str,
    ) -> Result<(), String>
    where
        R: fmt::Display,
        X: fmt::Display,
        Y: fmt::Display,
    {
        let room = room.to_string();
        let x = x.to_string();
        let y = y.to_string();
        self.send_encoded_or(adapter::position(&room, &x, &y, parts), || {
            let mut line = format!("WPOS {room} {x} {y}");
            if !parts.is_empty() {
                line.push(' ');
                line.push_str(parts);
            }
            self.send_line(&line)
        })
    }

    /// Send `WSYNC`.
    pub fn world_sync(&self) -> Result<(), String> {
        self.send_command(ClientCommand::Sync)
    }

    /// Send `WREQ <target> <kind> <info>`.
    pub fn request<T>(&self, target: T, kind: &str, info: &str) -> Result<(), String>
    where
        T: fmt::Display,
    {
        let target = target.to_string();
        self.send_encoded_or(adapter::request(&target, kind, info), || {
            let mut line = format!("WREQ {target} {kind} ");
            line.push_str(info);
            self.send_line(&line)
        })
    }

    /// Send `WANS <request_id> <answer>`.
    pub fn response<T, A>(&self, request_id: T, answer: A) -> Result<(), String>
    where
        T: fmt::Display,
        A: fmt::Display,
    {
        let request_id = request_id.to_string();
        let answer = answer.to_string();
        self.send_encoded_or(adapter::response(&request_id, &answer), || {
            self.send_line(&format!("WANS {request_id} {answer}"))
        })
    }

    /// Send `WCANCEL`.
    pub fn cancel(&self) -> Result<(), String> {
        self.send_command(ClientCommand::Cancel { id: None })
    }

    /// Send `WSAY <text>`.
    pub fn say(&self, text: &str) -> Result<(), String> {
        let text = adapter::sanitize_bytes(text.as_bytes());
        self.send_command(ClientCommand::Say { text })
    }

    /// Send `WEXC <number>`.
    pub fn exclaim<T: fmt::Display>(&self, kind: T) -> Result<(), String> {
        self.send_line(&format!("WEXC {kind}"))
    }

    /// Request the online player list (`WLIST`).
    pub fn world_list(&self) -> Result<(), String> {
        self.send_command(ClientCommand::WorldList)
    }

    /// Send `WEND`.
    pub fn end_world(&self) -> Result<(), String> {
        self.send_command(ClientCommand::End)
    }

    /// Send `BYE`, shut the socket down and join the reader thread. Idempotent.
    pub fn disconnect(&self) -> Result<(), String> {
        if self.disconnecting.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let _ = self.send_command(ClientCommand::Bye);
        {
            let stream = lock_unpoison(&self.writer);
            let _ = stream.shutdown(Shutdown::Both);
        }

        let reader = {
            let mut guard = lock_unpoison(&self.reader);
            guard.take()
        };
        if let Some(handle) = reader {
            let _ = handle.join();
        }

        self.events.close();
        self.status.store(RozStatus::Disconnected.as_u8(), Ordering::SeqCst);
        Ok(())
    }

    /// Current session state.
    pub fn status(&self) -> RozStatus {
        RozStatus::from_u8(self.status.load(Ordering::SeqCst))
    }

    /// Whether the reader has stopped and the event queue is closed.
    ///
    /// A closed client still drains events that were queued before the reader
    /// stopped, but no new ones can arrive.
    pub fn is_closed(&self) -> bool {
        self.events.is_closed()
    }

    /// Last `side` received in `START` or `WOK`; 0 when none was received.
    pub fn side(&self) -> i32 {
        self.side.load(Ordering::SeqCst)
    }

    /// Peer name received in `START`; empty when unknown or after `PEER_LEFT`.
    pub fn peer(&self) -> String {
        lock_unpoison(&self.peer).clone()
    }

    /// Next queued event, or `None` when the queue is empty.
    pub fn try_recv(&self) -> Option<RozEvent> {
        self.events.try_pop()
    }

    /// Wait for the next event, up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<RozEvent> {
        self.events.pop_timeout(timeout)
    }

    /// Wait indefinitely for the next event. Returns `None` once the reader stops.
    pub fn recv(&self) -> Option<RozEvent> {
        self.events.pop_blocking()
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer_addr
    }

    fn send_command(&self, command: ClientCommand) -> Result<(), String> {
        let line =
            adapter::encode_command(&command).map_err(|error| format!("comando ROZ inválido: {error}"))?;
        self.send_line_bytes(&line)
    }

    fn send_encoded_or(
        &self,
        encoded: Result<Vec<u8>, String>,
        legacy: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        match encoded {
            Ok(line) => self.send_line_bytes(&line),
            Err(_) => legacy(),
        }
    }

    fn send_line(&self, line: &str) -> Result<(), String> {
        self.send_line_bytes(line.as_bytes())
    }

    fn send_line_bytes(&self, line: &[u8]) -> Result<(), String> {
        let mut sanitized = Vec::with_capacity(line.len() + 1);
        for &byte in line {
            sanitized.push(if byte == b'\r' || byte == b'\n' { b' ' } else { byte });
        }
        if sanitized.len() > MAX_LINE {
            return Err(format!(
                "línea demasiado larga ({} bytes, máximo {MAX_LINE})",
                sanitized.len()
            ));
        }
        sanitized.push(b'\n');
        let mut stream = lock_unpoison(&self.writer);
        stream
            .write_all(&sanitized)
            .and_then(|_| stream.flush())
            .map_err(|error| format!("error enviando línea: {error}"))
    }
}

impl Drop for RozClient {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

fn reader_loop(
    mut stream: TcpStream,
    events: Arc<EventQueue>,
    status: Arc<AtomicU8>,
    side: Arc<AtomicI32>,
    peer: Arc<Mutex<String>>,
) {
    let mut buffer = adapter::LineBuffer::new();
    let mut chunk = [0u8; 4096];

    let reason = loop {
        match stream.read(&mut chunk) {
            Ok(0) => break "el servidor cerró la conexión".to_string(),
            Ok(read) => {
                for event in buffer.feed(&chunk[..read]) {
                    match event {
                        adapter::LineEvent::Line(line) => {
                            if line.is_empty() {
                                continue;
                            }
                            if dispatch_world_line(&line, &events) {
                                continue;
                            }
                            if dispatch_codec_line(&line, &events, &status, &side, &peer) {
                                continue;
                            }
                            let text = String::from_utf8_lossy(&line);
                            handle_line(&text, &events, &status, &side, &peer);
                        }
                        adapter::LineEvent::Overlong => events.push(RozEvent::Malformed {
                            line: String::new(),
                            reason: format!("línea supera {MAX_LINE} bytes"),
                        }),
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => break format!("error de red: {error}"),
        }
    };

    status.store(RozStatus::Disconnected.as_u8(), Ordering::SeqCst);
    events.push(RozEvent::Disconnected { reason });
    events.close();
}

fn dispatch_world_line(line: &[u8], events: &EventQueue) -> bool {
    let command = match line.iter().position(|&byte| byte == b' ') {
        Some(index) => &line[..index],
        None => line,
    };
    match command {
        b"WP" => {
            match world::parse_wp(line) {
                Ok(message) => events.push(RozEvent::WorldPosition(WorldPosition {
                    id: message.id,
                    room: message.room,
                    x: message.x,
                    y: message.y,
                    parts: String::from_utf8_lossy(&message.parts_token).into_owned(),
                    name: world::sanitize_name_display(&message.name),
                    name_bytes: message.name,
                    decoded: message.parts,
                    parts_token: message.parts_token,
                    raw: message.raw_remainder,
                })),
                Err(error) => push_malformed(events, &String::from_utf8_lossy(line), format!("{error:?}")),
            }
            true
        },
        b"WDEL" => {
            match world::parse_wdel(line) {
                Ok(message) => events.push(RozEvent::WorldDelete {
                    id: message.id,
                    raw: message.raw_remainder,
                }),
                Err(error) => push_malformed(events, &String::from_utf8_lossy(line), format!("{error:?}")),
            }
            true
        },
        b"WPOS" => {
            match world::parse_wpos(line) {
                Ok(message) => events.push(RozEvent::WorldSelfPosition(message)),
                Err(error) => push_malformed(events, &String::from_utf8_lossy(line), format!("{error:?}")),
            }
            true
        },
        b"WLIST" => {
            match world::parse_wlist(line) {
                Ok(list) => events.push(RozEvent::WorldList {
                    total: list.total,
                    entries: list
                        .entries
                        .into_iter()
                        .map(|entry| WorldListEntry {
                            name: entry.name,
                            place: entry.place,
                            flags: entry.flags,
                        })
                        .collect(),
                }),
                Err(error) => push_malformed(events, &String::from_utf8_lossy(line), format!("{error:?}")),
            }
            true
        },
        _ => false,
    }
}

fn dispatch_codec_line(
    line: &[u8],
    events: &EventQueue,
    status: &AtomicU8,
    side: &AtomicI32,
    peer: &Mutex<String>,
) -> bool {
    let event = match adapter::parse_server_line(line) {
        Ok(Some(event)) => event,
        Ok(None) => return false,
        Err(_) => {
            // The clean-room codec stays strict, but the public client keeps
            // the pre-cleanroom best-effort parse for non-canonical server
            // lines such as `WOK -1`, `WANS 9 +1` or `WREQ 4 F`.
            handle_line(&String::from_utf8_lossy(line), events, status, side, peer);
            return true;
        },
    };

    match event {
        ServerEvent::Welcome { id } => {
            let version = u32::try_from(id).unwrap_or(u32::MAX);
            events.push(RozEvent::Welcome { version, id });
        },
        ServerEvent::Wait => {
            status.store(RozStatus::Waiting.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::Wait);
        },
        ServerEvent::Start { side: side_value, peer: peer_bytes } => {
            let peer_name = String::from_utf8_lossy(&peer_bytes).into_owned();
            side.store(side_value, Ordering::SeqCst);
            *lock_unpoison(peer) = peer_name.clone();
            status.store(RozStatus::Started.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::Start { side: side_value, peer: peer_name });
        },
        ServerEvent::Ok { id } => {
            let Ok(side_value) = i32::try_from(id) else {
                push_malformed(events, &String::from_utf8_lossy(line), "WOK fuera del rango i32");
                return true;
            };
            side.store(side_value, Ordering::SeqCst);
            status.store(RozStatus::WorldJoined.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::WorldJoined { side: side_value, id: Some(id) });
        },
        ServerEvent::Full => {
            status.store(RozStatus::Full.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::Full { world: false });
        },
        ServerEvent::WorldFull => {
            status.store(RozStatus::Full.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::Full { world: true });
        },
        ServerEvent::PeerLeft => {
            lock_unpoison(peer).clear();
            status.store(RozStatus::PeerLeft.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::PeerLeft);
        },
        ServerEvent::Request { id, kind, info } => {
            if kind.is_empty() {
                push_malformed(events, &String::from_utf8_lossy(line), "WREQ sin kind");
                return true;
            }
            events.push(RozEvent::WorldRequest {
                id,
                kind: String::from_utf8_lossy(&kind).into_owned(),
                payload: String::from_utf8_lossy(&info).into_owned(),
            });
        },
        ServerEvent::Answer { id, answer } => {
            events.push(RozEvent::WorldAnswer { id, answer });
        },
        ServerEvent::Cancel { id } => {
            events.push(RozEvent::WorldCancel { id });
        },
        ServerEvent::Input { epoch, frame, mask } => {
            events.push(RozEvent::Input { epoch, frame, mask });
        },
        ServerEvent::Checksum { epoch, frame, checksum } => {
            events.push(RozEvent::Checksum { epoch, frame, checksum });
        },
        ServerEvent::Message { payload } => {
            events.push(RozEvent::Message(payload));
        },
        ServerEvent::Position(_) | ServerEvent::Delete { .. } => return false,
    }
    true
}

fn handle_line(
    line: &str,
    events: &EventQueue,
    status: &AtomicU8,
    side: &AtomicI32,
    peer: &Mutex<String>,
) {
    let (command, rest) = match line.split_once(' ') {
        Some((command, rest)) => (command, rest.trim_start()),
        None => (line, ""),
    };

    match command {
        "WELCOME" => {
            let id = rest.trim().parse::<u64>().unwrap_or(0);
            let version = u32::try_from(id).unwrap_or(u32::MAX);
            events.push(RozEvent::Welcome { version, id });
        }
        "WAIT" => {
            status.store(RozStatus::Waiting.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::Wait);
        }
        "START" => {
            let (side_text, peer_text) = split_first(rest);
            match side_text.parse::<i32>() {
                Ok(side_value) => {
                    let peer_name = peer_text.trim_start().to_string();
                    side.store(side_value, Ordering::SeqCst);
                    *lock_unpoison(peer) = peer_name.clone();
                    status.store(RozStatus::Started.as_u8(), Ordering::SeqCst);
                    events.push(RozEvent::Start { side: side_value, peer: peer_name });
                }
                Err(_) => push_malformed(events, line, "START sin side válido"),
            }
        }
        "WOK" => match rest.trim().parse::<i32>() {
            Ok(side_value) => {
                side.store(side_value, Ordering::SeqCst);
                status.store(RozStatus::WorldJoined.as_u8(), Ordering::SeqCst);
                events.push(RozEvent::WorldJoined {
                    side: side_value,
                    id: u64::try_from(side_value).ok(),
                });
            }
            Err(_) => push_malformed(events, line, "WOK sin side válido"),
        },
        "FULL" => {
            status.store(RozStatus::Full.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::Full { world: false });
        }
        "WFULL" => {
            status.store(RozStatus::Full.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::Full { world: true });
        }
        "PEER_LEFT" => {
            lock_unpoison(peer).clear();
            status.store(RozStatus::PeerLeft.as_u8(), Ordering::SeqCst);
            events.push(RozEvent::PeerLeft);
        }
        "MSG" => match decode_hex(rest) {
            Ok(payload) => events.push(RozEvent::Message(payload)),
            Err(reason) => push_malformed(events, line, reason),
        },
        "IN" => {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            if fields.len() != 3 {
                push_malformed(events, line, "IN requiere epoch, frame y mask");
                return;
            }
            let parsed = (fields[0].parse::<i32>(), fields[1].parse::<i32>(), fields[2].parse::<i32>());
            match parsed {
                (Ok(epoch), Ok(frame), Ok(mask)) => events.push(RozEvent::Input { epoch, frame, mask }),
                _ => push_malformed(events, line, "IN con enteros inválidos"),
            }
        }
        "SUM" => {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            if fields.len() != 3 {
                push_malformed(events, line, "SUM requiere epoch, frame y checksum");
                return;
            }
            let parsed = (fields[0].parse::<i32>(), fields[1].parse::<i32>(), fields[2].parse::<i64>());
            match parsed {
                (Ok(epoch), Ok(frame), Ok(checksum)) => events.push(RozEvent::Checksum { epoch, frame, checksum }),
                _ => push_malformed(events, line, "SUM con enteros inválidos"),
            }
        }
        "WREQ" => {
            let (id_text, remainder) = split_first(rest);
            let (kind, payload) = split_first(remainder);
            match id_text.parse::<u64>() {
                Ok(id) if !kind.is_empty() => {
                    events.push(RozEvent::WorldRequest {
                        id,
                        kind: kind.to_string(),
                        payload: payload.to_string(),
                    });
                }
                _ => push_malformed(events, line, "WREQ requiere id y kind"),
            }
        }
        "WANS" => {
            let (id_text, answer_text) = split_first(rest);
            match (id_text.parse::<u64>(), answer_text.trim().parse::<i32>()) {
                (Ok(id), Ok(answer)) => events.push(RozEvent::WorldAnswer { id, answer }),
                _ => push_malformed(events, line, "WANS requiere id y respuesta"),
            }
        }
        "WCANCEL" => {
            let id_text = rest.trim();
            if id_text.is_empty() {
                events.push(RozEvent::WorldCancel { id: None });
            } else {
                match id_text.parse::<u64>() {
                    Ok(id) => events.push(RozEvent::WorldCancel { id: Some(id) }),
                    Err(_) => push_malformed(events, line, "WCANCEL con id inválido"),
                }
            }
        }
        "WSAY" => {
            let (id_text, text) = split_first(rest);
            match id_text.parse::<u64>() {
                Ok(id) if !text.is_empty() => events.push(RozEvent::WorldSay { id, text: text.to_string() }),
                _ => push_malformed(events, line, "WSAY requiere id y texto"),
            }
        }
        "WEXC" => {
            let (id_text, kind_text) = split_first(rest);
            match (id_text.parse::<u64>(), kind_text.trim().parse::<i32>()) {
                (Ok(id), Ok(kind)) => events.push(RozEvent::WorldExclaim { id, kind }),
                _ => push_malformed(events, line, "WEXC requiere id y número"),
            }
        }
        _ => {
            // Unknown commands are ignored on purpose.
        }
    }
}

fn split_first(value: &str) -> (&str, &str) {
    match value.split_once(' ') {
        Some((first, rest)) => (first, rest.trim_start()),
        None => (value, ""),
    }
}

fn push_malformed(events: &EventQueue, line: &str, reason: impl Into<String>) {
    events.push(RozEvent::Malformed { line: line.to_string(), reason: reason.into() });
}

/// Encode bytes as lowercase hex.
pub fn encode_hex(bytes: &[u8]) -> String {
    reloaded_cleanroom::roz::encode_hex(bytes)
}

/// Decode a `MSG` hex payload. Invalid or odd-length input is an error, never a panic.
/// Both lowercase and uppercase digits are accepted.
pub fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    reloaded_cleanroom::roz::decode_hex(text.as_bytes()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    fn spawn_mock<F>(handler: F) -> SocketAddr
    where
        F: FnOnce(TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            handler(stream);
        });
        addr
    }

    fn connect(addr: SocketAddr) -> RozClient {
        RozClient::connect_with_timeout(&addr.to_string(), Duration::from_secs(2))
            .expect("connect")
    }

    fn read_line(reader: &mut BufReader<TcpStream>) -> String {
        let mut line = String::new();
        let read = reader.read_line(&mut line).expect("read_line");
        assert!(read > 0, "el cliente cerró antes de enviar la línea");
        line.trim_end_matches(|ch| ch == '\r' || ch == '\n').to_string()
    }

    fn read_line_bytes(reader: &mut BufReader<TcpStream>) -> Vec<u8> {
        let mut line = Vec::new();
        let read = reader.read_until(b'\n', &mut line).expect("read_until");
        assert!(read > 0, "el cliente cerró antes de enviar la línea");
        while line.last() == Some(&b'\n') || line.last() == Some(&b'\r') {
            line.pop();
        }
        line
    }

    fn next_event(client: &RozClient) -> RozEvent {
        client
            .recv_timeout(Duration::from_secs(2))
            .expect("evento del servidor")
    }

    #[test]
    fn sends_hello_and_join_with_crlf_sanitized() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let hello = read_line(&mut reader);
            let join = read_line(&mut reader);
            tx.send((hello, join)).expect("send lines");
        });

        let client = connect(addr);
        client.hello("Ana\nRival\r").expect("hello");
        client.join("sala\n1").expect("join");

        let (hello, join) = rx.recv_timeout(Duration::from_secs(2)).expect("lines");
        assert_eq!(hello, "HELLO Ana Rival ");
        assert_eq!(join, "JOIN sala 1");
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn hello_join_and_raw_line_preserve_non_utf8_bytes() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let lines = vec![
                read_line_bytes(&mut reader),
                read_line_bytes(&mut reader),
                read_line_bytes(&mut reader),
            ];
            tx.send(lines).expect("send lines");
        });

        let client = connect(addr);
        client.hello_bytes(b"Ana\xff").expect("hello bytes");
        client.join_bytes(b"sala\x80").expect("join bytes");
        client.send_raw_line_bytes(b"LINE\xfe\r\n").expect("raw bytes");

        let lines = rx.recv_timeout(Duration::from_secs(2)).expect("lines");
        assert_eq!(
            lines,
            vec![
                b"HELLO Ana\xff".to_vec(),
                b"JOIN sala\x80".to_vec(),
                b"LINE\xfe  ".to_vec(),
            ]
        );
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn parses_wait_start_and_state() {
        let (release_tx, release_rx) = mpsc::channel();
        let addr = spawn_mock(move |mut stream| {
            stream.write_all(b"WAIT\nSTART 2 Rival\n").expect("write");
            stream.flush().expect("flush");
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });

        let client = connect(addr);
        assert_eq!(next_event(&client), RozEvent::Wait);
        assert_eq!(
            next_event(&client),
            RozEvent::Start { side: 2, peer: "Rival".to_string() }
        );
        assert_eq!(client.status(), RozStatus::Started);
        assert_eq!(client.side(), 2);
        assert_eq!(client.peer(), "Rival");
        let _ = release_tx.send(());
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn parses_wok_and_full_states() {
        let (release_tx, release_rx) = mpsc::channel();
        let addr = spawn_mock(move |mut stream| {
            stream.write_all(b"WOK 1\nWFULL\n").expect("write");
            stream.flush().expect("flush");
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });

        let client = connect(addr);
        assert_eq!(next_event(&client), RozEvent::WorldJoined { side: 1, id: Some(1) });
        assert_eq!(next_event(&client), RozEvent::Full { world: true });
        assert_eq!(client.status(), RozStatus::Full);
        let _ = release_tx.send(());
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn decodes_msg_hex_and_survives_invalid_hex() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"MSG 48656c6c6f\nMSG zz\nMSG 123\nMSG 4F4B\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(next_event(&client), RozEvent::Message(b"Hello".to_vec()));
        assert!(matches!(next_event(&client), RozEvent::Malformed { .. }));
        assert!(matches!(next_event(&client), RozEvent::Malformed { .. }));
        assert_eq!(next_event(&client), RozEvent::Message(b"OK".to_vec()));
    }

    #[test]
    fn parses_input_events_and_rejects_bad_fields() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"IN 7 42 305419896\nIN 1 oops 3\nWAIT\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(
            next_event(&client),
            RozEvent::Input { epoch: 7, frame: 42, mask: 305_419_896 }
        );
        assert!(matches!(next_event(&client), RozEvent::Malformed { .. }));
        assert_eq!(next_event(&client), RozEvent::Wait);
    }

    #[test]
    fn parses_checksum_events_and_rejects_bad_fields() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"SUM 7 42 305419896\nSUM 7 42\nWAIT\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(
            next_event(&client),
            RozEvent::Checksum { epoch: 7, frame: 42, checksum: 305_419_896 }
        );
        assert!(matches!(next_event(&client), RozEvent::Malformed { .. }));
        assert_eq!(next_event(&client), RozEvent::Wait);
    }

    #[test]
    fn sends_checksum_command() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            tx.send(read_line(&mut reader)).expect("send line");
        });

        let client = connect(addr);
        client.checksum_send(7, 42, 99).expect("checksum_send");

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("line"),
            "SUM 7 42 99"
        );
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn parses_world_position_and_delete() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(
                    b"WP 9 5 120 240 S,3,2,0,0,1,1 Juan Perez\nWDEL 9\n",
                )
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(
            next_event(&client),
            RozEvent::WorldPosition(WorldPosition {
                id: 9,
                room: 5,
                x: 120.0,
                y: 240.0,
                parts: "S,3,2,0,0,1,1".to_string(),
                name: "Juan Perez".to_string(),
                name_bytes: b"Juan Perez".to_vec(),
                decoded: world::parse_parts(b"S,3,2,0,0,1,1"),
                parts_token: b"S,3,2,0,0,1,1".to_vec(),
                raw: b" 9 5 120 240 S,3,2,0,0,1,1 Juan Perez".to_vec(),
            })
        );
        assert_eq!(
            next_event(&client),
            RozEvent::WorldDelete { id: 9, raw: b" 9".to_vec() }
        );
    }

    #[test]
    fn world_position_preserves_raw_spacing_and_tolerates_repeated_spaces() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"WP  10  2  1.5  -3  A,1  Nombre  Con Espacios\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(
            next_event(&client),
            RozEvent::WorldPosition(WorldPosition {
                id: 10,
                room: 2,
                x: 1.5,
                y: -3.0,
                parts: "A,1".to_string(),
                name: "Nombre  Con Espacios".to_string(),
                name_bytes: b"Nombre  Con Espacios".to_vec(),
                decoded: world::parse_parts(b"A,1"),
                parts_token: b"A,1".to_vec(),
                raw: b"  10  2  1.5  -3  A,1  Nombre  Con Espacios".to_vec(),
            })
        );
    }

    #[test]
    fn parses_wpos_and_wlist_with_cleanroom_parsers() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"WPOS 5 100.5 200 S,1,2,0,0,1,1\nWLIST 2\x1dAda\x1eRoute 1\x1eGM\x1dBob\x1eRoute 2\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        match next_event(&client) {
            RozEvent::WorldSelfPosition(message) => {
                assert_eq!(message.room, 5);
                assert_eq!(message.x, 100.5);
                assert_eq!(message.parts_token.as_deref(), Some(b"S,1,2,0,0,1,1".as_slice()));
                assert_eq!(message.parts.segments.len(), 1);
            },
            other => panic!("evento inesperado: {other:?}"),
        }
        match next_event(&client) {
            RozEvent::WorldList { total, entries } => {
                assert_eq!(total, 2);
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].name, "Ada");
                assert_eq!(entries[0].place, "Route 1");
                assert_eq!(entries[0].flags, "GM");
                assert_eq!(entries[1].name, "Bob");
            },
            other => panic!("evento inesperado: {other:?}"),
        }
    }

    #[test]
    fn world_position_raw_keeps_non_utf8_name_bytes() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"WP 1 2 3 4 P Nombre\xff\x80\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        match next_event(&client) {
            RozEvent::WorldPosition(position) => {
                assert_eq!(position.id, 1);
                assert_eq!(position.parts, "P");
                assert_eq!(position.name_bytes, b"Nombre\xff\x80".to_vec());
                assert_eq!(position.raw, b" 1 2 3 4 P Nombre\xff\x80".to_vec());
            },
            other => panic!("evento inesperado: {other:?}"),
        }
    }

    #[test]
    fn world_position_limits_names_accepts_i32_and_sanitizes_controls() {
        let long_name = "N".repeat(world::MAX_REMOTE_NAME_BYTES * 2);
        let utf8_name = format!("A{}", "é".repeat(64));
        let addr = spawn_mock(move |mut stream| {
            stream
                .write_all(
                    format!(
                        "WP 1 2 3 4 S,1,0,0,0,1,1 {long_name}\n\
                         WP 2 2 2147483647 -2147483648 S,1,0,0,0,1,1 big\n\
                         WP 3 2 3 4 S,1,0,0,0,1,1 bad\tname\n\
                         WP 4 2 2147483648 4 S,1,0,0,0,1,1 bad\n\
                         WP 5 2 5 6 S,1,0,0,0,1,1 {utf8_name}\nWAIT\n"
                    )
                    .as_bytes(),
                )
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        match next_event(&client) {
            RozEvent::WorldPosition(position) => {
                assert_eq!(position.id, 1);
                assert_eq!(position.name_bytes.len(), world::MAX_REMOTE_NAME_BYTES);
                assert_eq!(position.name.len(), world::MAX_REMOTE_NAME_BYTES);
            },
            other => panic!("evento inesperado: {other:?}"),
        }
        match next_event(&client) {
            RozEvent::WorldPosition(position) => {
                assert_eq!(position.id, 2);
                assert_eq!((position.x, position.y), (2147483647.0, -2147483648.0));
            },
            other => panic!("evento inesperado: {other:?}"),
        }
        match next_event(&client) {
            RozEvent::WorldPosition(position) => {
                assert_eq!(position.id, 3);
                assert_eq!(position.name_bytes, b"badname".to_vec());
                assert_eq!(position.name, "badname");
            },
            other => panic!("evento inesperado: {other:?}"),
        }
        assert!(matches!(next_event(&client), RozEvent::Malformed { .. }));
        match next_event(&client) {
            RozEvent::WorldPosition(position) => {
                assert_eq!(position.id, 5);
                assert_eq!(position.name_bytes.len(), world::MAX_REMOTE_NAME_BYTES - 1);
                assert!(std::str::from_utf8(&position.name_bytes).is_ok());
                assert_eq!(position.name, format!("A{}", "é".repeat(63)));
            },
            other => panic!("evento inesperado: {other:?}"),
        }
        assert_eq!(next_event(&client), RozEvent::Wait);
    }

    #[test]
    fn parses_world_requests_answers_and_cancels() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"WREQ 4 F Juan\x1einfo\nWANS 4 1\nWCANCEL 4\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(
            next_event(&client),
            RozEvent::WorldRequest {
                id: 4,
                kind: "F".to_string(),
                payload: "Juan\u{1e}info".to_string(),
            }
        );
        assert_eq!(
            next_event(&client),
            RozEvent::WorldAnswer { id: 4, answer: 1 }
        );
        assert_eq!(
            next_event(&client),
            RozEvent::WorldCancel { id: Some(4) }
        );
    }

    #[test]
    fn sends_all_world_commands() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let mut lines = Vec::new();
            for _ in 0..9 {
                lines.push(read_line(&mut reader));
            }
            tx.send(lines).expect("send lines");
        });

        let client = connect(addr);
        client.world_join("Mundo").expect("world_join");
        client.input_send(7, 42, 99).expect("input_send");
        client.send("hola mundo").expect("send");
        client
            .world_position(5, 100, 200, "S,1,2,0,0,1,1")
            .expect("world_position");
        client.world_sync().expect("world_sync");
        client.request(9, "F", "hola").expect("request");
        client.response(9, 1).expect("response");
        client.cancel().expect("cancel");
        client.end_world().expect("end_world");

        let lines = rx.recv_timeout(Duration::from_secs(2)).expect("lines");
        assert_eq!(
            lines,
            vec![
                "WJOIN Mundo".to_string(),
                "IN 7 42 99".to_string(),
                "MSG 686f6c61206d756e646f".to_string(),
                "WPOS 5 100 200 S,1,2,0,0,1,1".to_string(),
                "WSYNC".to_string(),
                "WREQ 9 F hola".to_string(),
                "WANS 9 1".to_string(),
                "WCANCEL".to_string(),
                "WEND".to_string(),
            ]
        );
    }

    #[test]
    fn parses_cleanroom_control_events_and_msg() {
        let (release_tx, release_rx) = mpsc::channel();
        let addr = spawn_mock(move |mut stream| {
            stream
                .write_all(b"WREQ 4 F Juan\x1einfo\nWANS 4 -1\nSTART 2 Rival\nWOK 42\nMSG 4F4B1F\n")
                .expect("write");
            stream.flush().expect("flush");
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });

        let client = connect(addr);
        assert_eq!(
            next_event(&client),
            RozEvent::WorldRequest {
                id: 4,
                kind: "F".to_string(),
                payload: "Juan\u{1e}info".to_string(),
            }
        );
        assert_eq!(next_event(&client), RozEvent::WorldAnswer { id: 4, answer: -1 });
        assert_eq!(
            next_event(&client),
            RozEvent::Start { side: 2, peer: "Rival".to_string() }
        );
        assert_eq!(client.peer(), "Rival");
        assert_eq!(
            next_event(&client),
            RozEvent::WorldJoined { side: 42, id: Some(42) }
        );
        assert_eq!(next_event(&client), RozEvent::Message(b"OK\x1f".to_vec()));
        let _ = release_tx.send(());
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn malformed_cleanroom_lines_are_reported_and_reader_continues() {
        let (release_tx, release_rx) = mpsc::channel();
        let addr = spawn_mock(move |mut stream| {
            stream
                .write_all(b"WREQ\nWANS 4 nope\nSTART nope Rival\nWOK 4294967296\nMSG zz\nWAIT\n")
                .expect("write");
            stream.flush().expect("flush");
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });

        let client = connect(addr);
        let mut malformed = 0;
        loop {
            match next_event(&client) {
                RozEvent::Malformed { .. } => malformed += 1,
                RozEvent::Wait => break,
                other => panic!("evento inesperado: {other:?}"),
            }
        }
        assert_eq!(malformed, 5);
        assert_eq!(client.status(), RozStatus::Waiting);
        let _ = release_tx.send(());
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn lossy_peer_and_request_payload_keep_spec_remainder() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"START 2 Rival\xff Name\nWREQ 9 T  dos\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(
            next_event(&client),
            RozEvent::Start { side: 2, peer: "Rival\u{fffd} Name".to_string() }
        );
        assert_eq!(
            next_event(&client),
            RozEvent::WorldRequest { id: 9, kind: "T".to_string(), payload: " dos".to_string() }
        );
    }

    #[test]
    fn world_position_empty_and_none_parts_use_cleanroom_codec() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let lines = vec![read_line(&mut reader), read_line(&mut reader)];
            tx.send(lines).expect("send lines");
        });

        let client = connect(addr);
        client.world_position(5, 100, 200, "").expect("empty parts");
        client.world_position(5, 100, 200, "-").expect("none parts");

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("lines"),
            vec!["WPOS 5 100 200".to_string(), "WPOS 5 100 200 -".to_string()]
        );
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn fractional_scale_parts_are_forwarded_by_the_cleanroom_codec() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let lines = vec![read_line(&mut reader), read_line(&mut reader)];
            tx.send(lines).expect("send lines");
        });

        let client = connect(addr);
        client
            .world_position(25, 100, 200, "S,25,2,0,0,1.5,-0.75")
            .expect("fractional scale");
        client
            .world_position(25, 100, 200, "S,25,2,0.5,-1.25,1.5,0.75;P,25,4,3,8.5,-12.25,1.5,0.5")
            .expect("fractional position and scale");

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("lines"),
            vec![
                "WPOS 25 100 200 S,25,2,0,0,1.5,-0.75".to_string(),
                "WPOS 25 100 200 S,25,2,0.5,-1.25,1.5,0.75;P,25,4,3,8.5,-12.25,1.5,0.5".to_string(),
            ]
        );
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn sends_hello_input_checksum_and_world_list() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let lines = vec![
                read_line(&mut reader),
                read_line(&mut reader),
                read_line(&mut reader),
                read_line(&mut reader),
            ];
            tx.send(lines).expect("send lines");
        });

        let client = connect(addr);
        client.hello("Ana").expect("hello");
        client.input_send(-1, 2, -3).expect("input_send");
        client.checksum_send(7, 42, -99).expect("checksum_send");
        client.world_list().expect("world_list");

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("lines"),
            vec![
                "HELLO Ana".to_string(),
                "IN -1 2 -3".to_string(),
                "SUM 7 42 -99".to_string(),
                "WLIST".to_string(),
            ]
        );
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn legacy_noncanonical_client_forms_are_forwarded_verbatim() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let mut lines = Vec::new();
            for _ in 0..7 {
                lines.push(read_line(&mut reader));
            }
            tx.send(lines).expect("send lines");
        });

        let client = connect(addr);
        client.request(9, "T T", "hola").expect("request with spaced kind");
        client.request("05", "F", "").expect("request with noncanonical target");
        client.response(9, "yes").expect("response with text answer");
        client.input_send("x", 1, 1).expect("input with text epoch");
        client.checksum_send(1, 1, "x").expect("checksum with text value");
        client.world_position(1, 2, 3, "not a state").expect("loose parts");
        client
            .world_position("05", "100.5", "200", "S,1,2,0,0,1.5,0.75")
            .expect("noncanonical position");

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("lines"),
            vec![
                "WREQ 9 T T hola".to_string(),
                "WREQ 05 F ".to_string(),
                "WANS 9 yes".to_string(),
                "IN x 1 1".to_string(),
                "SUM 1 1 x".to_string(),
                "WPOS 1 2 3 not a state".to_string(),
                "WPOS 05 100.5 200 S,1,2,0,0,1.5,0.75".to_string(),
            ]
        );
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn legacy_malformed_server_forms_keep_previous_events() {
        let (release_tx, release_rx) = mpsc::channel();
        let addr = spawn_mock(move |mut stream| {
            stream
                .write_all(
                    b"WELCOME greeting\nWOK -1\nWANS 9 +1\nWREQ 4 F\nSTART 1\nWOK 42 \nWAIT extra\n",
                )
                .expect("write");
            stream.flush().expect("flush");
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });

        let client = connect(addr);
        assert_eq!(next_event(&client), RozEvent::Welcome { version: 0, id: 0 });
        assert_eq!(
            next_event(&client),
            RozEvent::WorldJoined { side: -1, id: None }
        );
        assert_eq!(next_event(&client), RozEvent::WorldAnswer { id: 9, answer: 1 });
        assert_eq!(
            next_event(&client),
            RozEvent::WorldRequest { id: 4, kind: "F".to_string(), payload: String::new() }
        );
        assert_eq!(
            next_event(&client),
            RozEvent::Start { side: 1, peer: String::new() }
        );
        assert_eq!(
            next_event(&client),
            RozEvent::WorldJoined { side: 42, id: Some(42) }
        );
        assert_eq!(next_event(&client), RozEvent::Wait);
        assert_eq!(client.side(), 42);
        assert_eq!(client.status(), RozStatus::Waiting);
        let _ = release_tx.send(());
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn legacy_join_wexc_and_raw_wsay_still_work() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |mut stream| {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let lines = vec![read_line(&mut reader), read_line(&mut reader)];
            tx.send(lines).expect("send lines");
            stream
                .write_all(b"WSAY 9 hola mundo\nWEXC 9 3\nWAIT\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        client.join("sala\r\n1").expect("join");
        client.exclaim(7).expect("exclaim");

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("lines"),
            vec!["JOIN sala  1".to_string(), "WEXC 7".to_string()]
        );
        assert_eq!(
            next_event(&client),
            RozEvent::WorldSay { id: 9, text: "hola mundo".to_string() }
        );
        assert_eq!(
            next_event(&client),
            RozEvent::WorldExclaim { id: 9, kind: 3 }
        );
        assert_eq!(next_event(&client), RozEvent::Wait);
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn sends_raw_message_bytes_and_raw_lines() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let mut lines = Vec::new();
            for _ in 0..2 {
                lines.push(read_line(&mut reader));
            }
            tx.send(lines).expect("send lines");
        });

        let client = connect(addr);
        client
            .send_message_bytes(b"hola\nmundo")
            .expect("send_message_bytes");
        client.send_raw_line("WSYNC\r\n").expect("send_raw_line");

        let lines = rx.recv_timeout(Duration::from_secs(2)).expect("lines");
        assert_eq!(
            lines,
            vec![
                "MSG 686f6c610a6d756e646f".to_string(),
                "WSYNC  ".to_string(),
            ]
        );
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn hex_encoder_is_lowercase_and_binary_safe() {
        assert_eq!(encode_hex(b""), "");
        assert_eq!(encode_hex(b"Hello"), "48656c6c6f");
        assert_eq!(encode_hex(&[0x00, 0xff, 0x0a, 0x0d]), "00ff0a0d");
    }

    #[test]
    fn cleanroom_adapter_encodes_and_parses_spec_commands() {
        assert_eq!(adapter::encode_command(&ClientCommand::Sync).unwrap(), b"WSYNC");
        assert_eq!(
            adapter::encode_command(&ClientCommand::Message { payload: b"OK\x1f".to_vec() }).unwrap(),
            b"MSG 4f4b1f"
        );
        assert_eq!(
            adapter::position("5", "100", "200", "S,1,2,0,0,1,1").unwrap(),
            b"WPOS 5 100 200 S,1,2,0,0,1,1"
        );
        assert_eq!(adapter::position("5", "100", "200", "").unwrap(), b"WPOS 5 100 200");
        assert_eq!(
            adapter::position("5", "100", "200", "S,25,2,0,0,1.5,0.75").unwrap(),
            b"WPOS 5 100 200 S,25,2,0,0,1.5,0.75"
        );
        assert!(adapter::position("05", "100", "200", "S,1,2,0,0,1,1").is_err());
        assert!(adapter::position("x", "100", "200", "S,1,2,0,0,1,1").is_err());
        assert!(adapter::position("5", "100", "200", "not a state").is_err());

        assert_eq!(
            adapter::parse_server_line(b"WOK 42").unwrap(),
            Some(ServerEvent::Ok { id: 42 })
        );
        assert_eq!(
            adapter::parse_server_line(b"IN 7 42 99").unwrap(),
            Some(ServerEvent::Input { epoch: 7, frame: 42, mask: 99 })
        );
        assert!(adapter::parse_server_line(b"WOK -1").is_err());
        assert_eq!(adapter::parse_server_line(b"GET / HTTP/1.1").unwrap(), None);
    }

    #[test]
    fn oversized_messages_are_rejected_before_writing() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let line = read_line(&mut reader);
            tx.send(line).expect("send line");
        });

        let client = connect(addr);
        let too_big = vec![0u8; MAX_LINE / 2];
        assert!(client.send_message_bytes(&too_big).is_err());

        client.send("ok").expect("small message");
        let line = rx.recv_timeout(Duration::from_secs(2)).expect("line");
        assert_eq!(line, "MSG 6f6b");
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn disconnect_sends_bye() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            loop {
                let line = read_line(&mut reader);
                if line == "BYE" {
                    tx.send(line).expect("send bye");
                    break;
                }
            }
        });

        let client = connect(addr);
        client.disconnect().expect("disconnect");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("bye"),
            "BYE"
        );
        assert_eq!(client.status(), RozStatus::Disconnected);
    }

    #[test]
    fn unknown_and_invalid_lines_do_not_stop_the_reader() {
        let (release_tx, release_rx) = mpsc::channel();
        let addr = spawn_mock(move |mut stream| {
            stream
                .write_all(b"WHAT 1\nIN 1 2\nSTART nope X\nWAIT\n")
                .expect("write");
            stream.flush().expect("flush");
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });

        let client = connect(addr);
        let mut malformed = 0;
        loop {
            match next_event(&client) {
                RozEvent::Malformed { .. } => malformed += 1,
                RozEvent::Wait => break,
                other => panic!("evento inesperado: {other:?}"),
            }
        }
        assert_eq!(malformed, 2);
        assert_eq!(client.status(), RozStatus::Waiting);
        let _ = release_tx.send(());
        client.disconnect().expect("disconnect");
    }

    #[test]
    fn reassembles_split_lines_and_strips_crlf() {
        let addr = spawn_mock(|mut stream| {
            stream.write_all(b"WEL").expect("write");
            stream.flush().expect("flush");
            thread::sleep(Duration::from_millis(20));
            stream.write_all(b"COME 3011\r\nWA").expect("write");
            stream.flush().expect("flush");
            thread::sleep(Duration::from_millis(20));
            stream.write_all(b"IT\r\n").expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(next_event(&client), RozEvent::Welcome { version: 3011, id: 3011 });
        assert_eq!(next_event(&client), RozEvent::Wait);
    }

    #[test]
    fn oversized_server_line_is_reported_and_reader_continues() {
        let addr = spawn_mock(|mut stream| {
            let mut data = vec![b'A'; MAX_LINE + 10];
            data.push(b'\n');
            data.extend_from_slice(b"WAIT\n");
            stream.write_all(&data).expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert!(matches!(next_event(&client), RozEvent::Malformed { .. }));
        assert_eq!(next_event(&client), RozEvent::Wait);
    }

    #[test]
    fn malformed_non_utf8_line_without_space_does_not_panic() {
        let addr = spawn_mock(|mut stream| {
            stream.write_all(b"WP\xff\nWAIT\n").expect("write");
            stream.flush().expect("flush");
        });

        let client = connect(addr);
        assert_eq!(next_event(&client), RozEvent::Wait);
    }

    #[test]
    fn server_close_emits_disconnected_without_panic() {
        let addr = spawn_mock(|stream| drop(stream));

        let client = connect(addr);
        match next_event(&client) {
            RozEvent::Disconnected { .. } => (),
            other => panic!("evento inesperado: {other:?}"),
        }
        assert_eq!(client.status(), RozStatus::Disconnected);
    }

    #[test]
    fn event_queue_is_bounded_and_keeps_latest_events() {
        let queue = EventQueue::new(2);
        queue.push(RozEvent::Wait);
        queue.push(RozEvent::WorldDelete { id: 1, raw: b" 1".to_vec() });
        queue.push(RozEvent::WorldDelete { id: 2, raw: b" 2".to_vec() });
        assert_eq!(queue.try_pop(), Some(RozEvent::WorldDelete { id: 1, raw: b" 1".to_vec() }));
        assert_eq!(queue.try_pop(), Some(RozEvent::WorldDelete { id: 2, raw: b" 2".to_vec() }));
        assert_eq!(queue.try_pop(), None);
    }

    #[test]
    fn recv_timeout_handles_zero_and_huge_durations() {
        let queue = EventQueue::new(1);
        assert_eq!(queue.pop_timeout(Duration::ZERO), None);
        queue.push(RozEvent::Wait);
        assert_eq!(queue.pop_timeout(Duration::MAX), Some(RozEvent::Wait));
    }

    #[test]
    fn hex_decoder_handles_odd_and_invalid_input() {
        assert_eq!(decode_hex("48656C6C6F").expect("uppercase hex"), b"Hello");
        assert!(decode_hex("123").is_err());
        assert!(decode_hex("zz").is_err());
        assert!(decode_hex("").expect("empty").is_empty());
    }
}
