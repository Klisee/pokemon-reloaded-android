//! Clean-room client for the Zibelan/ROZ line protocol.
//!
//! Transport is plain TCP with one UTF-8/ASCII line per command, terminated by `\n`.
//! Incoming lines may end in `\r\n`; the trailing `\r` is stripped. There is no TLS,
//! length prefix, JSON or binary framing.
//!
//! Client -> server commands implemented here:
//! `HELLO`, `JOIN`, `MSG`, `IN`, `WJOIN`, `WPOS`, `WSYNC`, `WREQ`, `WANS`,
//! `WCANCEL`, `WEND`, `BYE`.
//!
//! Server -> client commands: `WELCOME`, `WAIT`, `START`, `WOK`, `FULL`, `WFULL`,
//! `PEER_LEFT`, `MSG` (hex), `IN`, `WP`, `WDEL`, `WREQ`, `WANS`, `WCANCEL`.
//!
//! `MSG` framing:
//! - Server -> client `MSG` carries lowercase hex and is decoded here.
//! - Client -> server `MSG` carries lowercase hex too. This is confirmed by the
//!   static analysis of `ROZ.dll` (`net_send` and `net_send_file` both call the
//!   hex encoder before `MSG `; see `01-static.md` §5.3/§5.7). [`RozClient::send`]
//!   and [`RozClient::send_message_bytes`] therefore hex-encode their payload.
//!   [`RozClient::send_raw_line`] remains the raw escape hatch used by `net_line`.
//!
//! TODOs (not confirmed by a real capture):
//! - The native 60 FPS module (`roz_net.rs` inside the OpenGMK executable) has not
//!   been compared against `ROZ.dll` at runtime, so its framing is still unverified.
//!   A real client/server capture should confirm the hex decision above.
//! - The exact `WREQ`/`WANS`/`WCANCEL` payload layout and the meaning of `0x1E`
//!   inside `WREQ` are preserved as opaque data, not interpreted.
//! - The real-server line limit, keepalive/timeout policy and reconnect behaviour
//!   are unknown. A local safety limit of [`MAX_LINE`] bytes is enforced.
//! - `WELCOME 3011` is treated as a protocol banner/version, not as a player id.
//! - `WOK` and `START` both update `side`; whether they have different semantics
//!   on the real server is unconfirmed.

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

/// Maximum accepted line length, including the command but excluding the final LF.
pub const MAX_LINE: usize = 64 * 1024;

/// Default timeout used by [`RozClient::connect`].
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default number of events retained before the oldest one is dropped.
pub const DEFAULT_EVENT_CAPACITY: usize = 256;

/// Bound for a single `send_line`/`flush`, so a stuck peer cannot block the game
/// thread forever. `ROZ.dll` does not document its own send timeout.
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
    /// Original bytes after the `WP` command, including its leading space.
    ///
    /// `ROZ.dll` queues `WP` as `WP\x1f` + this exact remainder, so keeping it lets
    /// `net_recv` preserve the server's spacing and non-UTF-8 name bytes instead of
    /// re-formatting numbers.
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
    /// `WELCOME <version>` banner (for example `WELCOME 3011`).
    Welcome { version: u32 },
    /// `WAIT`.
    Wait,
    /// `START <side> <peer>`.
    Start { side: i32, peer: String },
    /// `WOK <side>`.
    WorldJoined { side: i32 },
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
        let mut line = Vec::with_capacity(6 + name.len());
        line.extend_from_slice(b"HELLO ");
        line.extend_from_slice(name);
        self.send_line_bytes(&line)
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
        self.send_line(&format!("WJOIN {name}"))
    }

    /// Send `MSG <hex>` for a UTF-8/text payload.
    ///
    /// The reference `ROZ.dll` hex-encodes the payload (lowercase) before sending
    /// `MSG`, so this does the same. Hex keeps CR/LF and non-UTF-8 bytes from
    /// breaking the line framing.
    pub fn send(&self, payload: &str) -> Result<(), String> {
        self.send_message_bytes(payload.as_bytes())
    }

    /// Send `MSG <hex>` for a raw byte payload.
    ///
    /// Used by `net_send`/`net_send_file` so GML strings are preserved byte for
    /// byte. The line is rejected locally when the hex form would exceed
    /// [`MAX_LINE`].
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
        let mut line = Vec::with_capacity(encoded_len);
        line.extend_from_slice(b"MSG ");
        encode_hex_into(payload, &mut line);
        self.send_line_bytes(&line)
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
        self.send_line(&format!("IN {epoch} {frame} {mask}"))
    }

    /// Send `SUM <epoch> <frame> <checksum>`.
    pub fn checksum_send<E, F, C>(&self, epoch: E, frame: F, checksum: C) -> Result<(), String>
    where
        E: fmt::Display,
        F: fmt::Display,
        C: fmt::Display,
    {
        self.send_line(&format!("SUM {epoch} {frame} {checksum}"))
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
        let mut line = format!("WPOS {room} {x} {y}");
        if !parts.is_empty() {
            line.push(' ');
            line.push_str(parts);
        }
        self.send_line(&line)
    }

    /// Send `WSYNC`.
    pub fn world_sync(&self) -> Result<(), String> {
        self.send_line("WSYNC")
    }

    /// Send `WREQ <target> <kind> <info>`.
    pub fn request<T>(&self, target: T, kind: &str, info: &str) -> Result<(), String>
    where
        T: fmt::Display,
    {
        let mut line = format!("WREQ {target} {kind} ");
        line.push_str(info);
        self.send_line(&line)
    }

    /// Send `WANS <request_id> <answer>`.
    pub fn response<T, A>(&self, request_id: T, answer: A) -> Result<(), String>
    where
        T: fmt::Display,
        A: fmt::Display,
    {
        self.send_line(&format!("WANS {request_id} {answer}"))
    }

    /// Send `WCANCEL`.
    pub fn cancel(&self) -> Result<(), String> {
        self.send_line("WCANCEL")
    }

    /// Send `WSAY <text>`.
    pub fn say(&self, text: &str) -> Result<(), String> {
        self.send_line(&format!("WSAY {text}"))
    }

    /// Send `WEXC <number>`.
    pub fn exclaim<T: fmt::Display>(&self, kind: T) -> Result<(), String> {
        self.send_line(&format!("WEXC {kind}"))
    }

    /// Request the online player list (`WLIST`).
    pub fn world_list(&self) -> Result<(), String> {
        self.send_line("WLIST")
    }

    /// Send `WEND`.
    pub fn end_world(&self) -> Result<(), String> {
        self.send_line("WEND")
    }

    /// Send `BYE`, shut the socket down and join the reader thread. Idempotent.
    pub fn disconnect(&self) -> Result<(), String> {
        if self.disconnecting.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let _ = self.send_line("BYE");
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
    let mut buffer = Vec::with_capacity(4096);
    let mut discarding = false;
    let mut chunk = [0u8; 4096];

    let reason = loop {
        match stream.read(&mut chunk) {
            Ok(0) => break "el servidor cerró la conexión".to_string(),
            Ok(read) => {
                for &byte in &chunk[..read] {
                    if discarding {
                        if byte == b'\n' {
                            discarding = false;
                        }
                        continue;
                    }
                    if byte == b'\n' {
                        let mut line = std::mem::take(&mut buffer);
                        if line.last() == Some(&b'\r') {
                            line.pop();
                        }
                        if line.len() > MAX_LINE {
                            events.push(RozEvent::Malformed {
                                line: String::new(),
                                reason: format!("línea supera {MAX_LINE} bytes"),
                            });
                        } else if !line.is_empty() {
                            let text = String::from_utf8_lossy(&line);
                            // Find the separator in the original bytes (ASCII spaces
                            // survive lossy decoding) so a malformed non-UTF-8 line
                            // can never make this slice panic. The remainder is kept
                            // verbatim for `WP`/`WDEL` byte preservation.
                            let raw_rest = match line.iter().position(|&byte| byte == b' ') {
                                Some(index) => &line[index..],
                                None => &[],
                            };
                            handle_line(&text, raw_rest, &events, &status, &side, &peer);
                        }
                        buffer = Vec::with_capacity(4096);
                    } else if buffer.len() >= MAX_LINE {
                        buffer.clear();
                        discarding = true;
                        events.push(RozEvent::Malformed {
                            line: String::new(),
                            reason: format!("línea supera {MAX_LINE} bytes"),
                        });
                    } else {
                        buffer.push(byte);
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

fn handle_line(
    line: &str,
    raw_rest: &[u8],
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
            let version = rest.trim().parse::<u32>().unwrap_or(0);
            events.push(RozEvent::Welcome { version });
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
                events.push(RozEvent::WorldJoined { side: side_value });
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
            let parsed = (
                fields[0].parse::<i32>(),
                fields[1].parse::<i32>(),
                fields[2].parse::<i32>(),
            );
            match parsed {
                (Ok(epoch), Ok(frame), Ok(mask)) => {
                    events.push(RozEvent::Input { epoch, frame, mask });
                }
                _ => push_malformed(events, line, "IN con enteros inválidos"),
            }
        }
        "SUM" => {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            if fields.len() != 3 {
                push_malformed(events, line, "SUM requiere epoch, frame y checksum");
                return;
            }
            let parsed = (
                fields[0].parse::<i32>(),
                fields[1].parse::<i32>(),
                fields[2].parse::<i64>(),
            );
            match parsed {
                (Ok(epoch), Ok(frame), Ok(checksum)) => {
                    events.push(RozEvent::Checksum { epoch, frame, checksum });
                }
                _ => push_malformed(events, line, "SUM con enteros inválidos"),
            }
        }
        "WP" => match parse_world_position(rest, raw_rest) {
            Ok(position) => events.push(RozEvent::WorldPosition(position)),
            Err(reason) => push_malformed(events, line, reason),
        },
        "WDEL" => match rest.trim().parse::<u64>() {
            Ok(id) => events.push(RozEvent::WorldDelete { id, raw: raw_rest.to_vec() }),
            Err(_) => push_malformed(events, line, "WDEL sin id válido"),
        },
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
        "WLIST" => events.push(parse_world_list(rest)),
        _ => {
            // Unknown commands are ignored on purpose.
        }
    }
}

fn parse_world_list(rest: &str) -> RozEvent {
    let mut parts = rest.split('\x1d');
    let total = parts.next().and_then(|value| value.trim().parse::<i32>().ok()).unwrap_or(0);
    let mut entries = Vec::new();
    for item in parts.take(250) {
        if item.is_empty() { continue; }
        let mut fields = item.split('\x1e');
        let name = fields.next().unwrap_or("").to_string();
        let place = fields.next().unwrap_or("").to_string();
        let flags = fields.next().unwrap_or("").to_string();
        if !name.is_empty() {
            entries.push(WorldListEntry { name, place, flags });
        }
    }
    RozEvent::WorldList { total, entries }
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

fn parse_world_position(rest: &str, raw_rest: &[u8]) -> Result<WorldPosition, String> {
    // The reference parser only documents `WP` as opaque data, but tolerating
    // repeated spaces between the numeric fields costs nothing and keeps the
    // player name (which may contain spaces) intact.
    let mut input = rest;
    let id = take_field(&mut input)
        .parse::<u64>()
        .map_err(|_| "WP con id inválido".to_string())?;
    let room = take_field(&mut input)
        .parse::<i32>()
        .map_err(|_| "WP con room inválido".to_string())?;
    let x = take_field(&mut input)
        .parse::<f64>()
        .map_err(|_| "WP con x inválido".to_string())?;
    let y = take_field(&mut input)
        .parse::<f64>()
        .map_err(|_| "WP con y inválido".to_string())?;
    let parts = take_field(&mut input).to_string();
    if parts.is_empty() {
        return Err("WP sin parts".to_string());
    }
    let name = input.trim_start_matches(' ').to_string();
    Ok(WorldPosition { id, room, x, y, parts, name, raw: raw_rest.to_vec() })
}

fn take_field<'a>(input: &mut &'a str) -> &'a str {
    *input = input.trim_start_matches(' ');
    match input.find(' ') {
        Some(index) => {
            let field = &input[..index];
            *input = &input[index + 1..];
            field
        },
        None => {
            let field = *input;
            *input = "";
            field
        },
    }
}

/// Encode bytes as lowercase hex, matching `ROZ.dll`'s encoder.
pub fn encode_hex(bytes: &[u8]) -> String {
    let mut output = Vec::with_capacity(bytes.len() * 2);
    encode_hex_into(bytes, &mut output);
    String::from_utf8(output).expect("hex output is ASCII")
}

fn encode_hex_into(bytes: &[u8], output: &mut Vec<u8>) {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    for &byte in bytes {
        output.push(TABLE[(byte >> 4) as usize]);
        output.push(TABLE[(byte & 0x0f) as usize]);
    }
}

/// Decode a `MSG` hex payload. Invalid or odd-length input is an error, never a panic.
/// Both lowercase and uppercase digits are accepted.
pub fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err("hex con longitud impar".to_string());
    }
    let mut output = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or_else(|| "hex con carácter inválido".to_string())?;
        let low = hex_digit(pair[1]).ok_or_else(|| "hex con carácter inválido".to_string())?;
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
        assert_eq!(next_event(&client), RozEvent::WorldJoined { side: 1 });
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
                raw: b"  10  2  1.5  -3  A,1  Nombre  Con Espacios".to_vec(),
            })
        );
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
                assert_eq!(position.raw, b" 1 2 3 4 P Nombre\xff\x80".to_vec());
            },
            other => panic!("evento inesperado: {other:?}"),
        }
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
        assert_eq!(next_event(&client), RozEvent::Welcome { version: 3011 });
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
