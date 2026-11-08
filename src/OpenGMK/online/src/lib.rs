//! Clean-room online transport for Pokemon Reloaded.
//!
//! This crate deliberately knows nothing about the game's GML or assets. It carries the
//! legacy line-delimited JSON between a client and a relay server, and re-exports the
//! clean-room world parsers used by the Zibelan transport. The game-side rendering adapter
//! lives in `gm8emulator::reloaded_online`.

use reloaded_cleanroom::roz::{LineBuffer, LineEvent};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub mod bridge;
pub mod failure;
pub mod roz;

pub use failure::ConnectionFailure;
pub use reloaded_cleanroom::world;

pub const DEFAULT_PORT: u16 = 7777;
pub const MAX_LINE: usize = 64 * 1024;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_EVENT_CAPACITY: usize = 4096;
pub const MAX_PEERS: usize = 1024;
pub const PEER_QUEUE_CAPACITY: usize = 64;

/// One line read from a socket, or the marker that an overlong line was discarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WireLine {
    Line(Vec<u8>),
    Overlong,
}

/// Incremental line reader with a hard per-line cap.
pub(crate) struct WireLineReader<R> {
    inner: R,
    framing: LineBuffer,
    queued: VecDeque<WireLine>,
    chunk: [u8; 4096],
}

impl<R> WireLineReader<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self { inner, framing: LineBuffer::new(), queued: VecDeque::new(), chunk: [0; 4096] }
    }

    pub(crate) fn get_ref(&self) -> &R {
        &self.inner
    }
}

impl<R: Read> WireLineReader<R> {
    pub(crate) fn next_line(&mut self) -> std::io::Result<Option<WireLine>> {
        loop {
            if let Some(line) = self.queued.pop_front() {
                return Ok(Some(line))
            }
            let read = match self.inner.read(&mut self.chunk) {
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if read == 0 {
                return Ok(None)
            }
            for event in self.framing.feed(&self.chunk[..read]) {
                self.queued.push_back(match event {
                    LineEvent::Line(line) => WireLine::Line(line),
                    LineEvent::Overlong => WireLine::Overlong,
                });
            }
        }
    }
}

pub(crate) fn lock_unpoison<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn encode_line<T: Serialize>(value: &T) -> std::io::Result<Vec<u8>> {
    let mut line = serde_json::to_vec(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if line.len() > MAX_LINE {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "message too large"))
    }
    line.push(b'\n');
    Ok(line)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello { name: String },
    Join { room: String },
    State {
        room: i32,
        x: f64,
        y: f64,
        sprite: i32,
        frame: i32,
        xs: f64,
        ys: f64,
        angle: f64,
        visible: bool,
        #[serde(default)]
        bbox: [i32; 4],
        #[serde(default)]
        depth: f64,
        blend: i32,
        alpha: f64,
    },
    Request { target: u64, kind: String, info: String },
    Response { target: u64, accept: bool },
    BattleTeam { target: u64, data: String },
    BattleInput {
        target: u64,
        mask: u32,
        #[serde(default)]
        seq: u32,
        #[serde(default)]
        frame: u32,
        #[serde(default)]
        checksum: u32,
    },
    Chat { text: String },
    Leave,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome { id: u64 },
    Joined { id: u64, name: String },
    State {
        id: u64,
        name: String,
        room: i32,
        x: f64,
        y: f64,
        sprite: i32,
        frame: i32,
        xs: f64,
        ys: f64,
        angle: f64,
        visible: bool,
        #[serde(default)]
        bbox: [i32; 4],
        #[serde(default)]
        depth: f64,
        blend: i32,
        alpha: f64,
    },
    Request { id: u64, name: String, kind: String, info: String },
    Response { id: u64, name: String, accept: bool },
    BattleTeam { id: u64, name: String, data: String },
    BattleInput {
        id: u64,
        mask: u32,
        #[serde(default)]
        seq: u32,
        #[serde(default)]
        frame: u32,
        #[serde(default)]
        checksum: u32,
    },
    Chat { id: u64, name: String, text: String },
    Left { id: u64 },
    Error { message: String },
}

#[derive(Clone, Debug)]
pub enum Event {
    Connected { id: u64 },
    Joined { id: u64, name: String },
    State {
        id: u64,
        name: String,
        room: i32,
        x: f64,
        y: f64,
        sprite: i32,
        frame: i32,
        xs: f64,
        ys: f64,
        angle: f64,
        visible: bool,
        bbox: [i32; 4],
        depth: f64,
        blend: i32,
        alpha: f64,
    },
    Request { id: u64, name: String, kind: String, info: String },
    Response { id: u64, name: String, accept: bool },
    BattleTeam { id: u64, name: String, data: String },
    BattleInput {
        id: u64,
        mask: u32,
        seq: u32,
        frame: u32,
        checksum: u32,
    },
    Chat { id: u64, name: String, text: String },
    Left { id: u64 },
    Error(String),
    Disconnected(String),
}

fn write_line(stream: &mut TcpStream, value: &ClientMessage) -> std::io::Result<()> {
    let line = encode_line(value)?;
    stream.write_all(&line)?;
    stream.flush()
}

#[derive(Default)]
struct EventQueueState {
    events: VecDeque<Event>,
    closed: bool,
}

struct EventQueue {
    capacity: usize,
    state: Mutex<EventQueueState>,
}

impl EventQueue {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(EventQueueState::default()),
        }
    }

    fn push(&self, event: Event) {
        let mut state = lock_unpoison(&self.state);
        if state.closed {
            return
        }
        if state.events.len() >= self.capacity {
            state.events.pop_front();
        }
        state.events.push_back(event);
    }

    fn close(&self, reason: String) {
        let mut state = lock_unpoison(&self.state);
        if state.closed {
            return
        }
        if state.events.len() >= self.capacity {
            state.events.pop_front();
        }
        state.events.push_back(Event::Disconnected(reason));
        state.closed = true;
    }

    fn pop(&self) -> Option<Event> {
        lock_unpoison(&self.state).events.pop_front()
    }

    fn is_closed(&self) -> bool {
        lock_unpoison(&self.state).closed
    }
}

/// A client connection. The reader thread owns the read half; writes are serialized by a mutex.
pub struct OnlineClient {
    writer: Arc<Mutex<TcpStream>>,
    shutdown: TcpStream,
    events: Arc<EventQueue>,
    reader: Mutex<Option<thread::JoinHandle<()>>>,
    disconnecting: std::sync::atomic::AtomicBool,
}

impl OnlineClient {
    pub fn connect(address: &str, name: &str, room: &str) -> Result<Self, String> {
        Self::connect_with_options(address, name, room, CONNECT_TIMEOUT, DEFAULT_EVENT_CAPACITY)
    }

    pub fn connect_with_options(
        address: &str,
        name: &str,
        room: &str,
        timeout: Duration,
        event_capacity: usize,
    ) -> Result<Self, String> {
        let addresses: Vec<SocketAddr> = address
            .to_socket_addrs()
            .map_err(|e| format!("direccion invalida: {e}"))?
            .collect();
        if addresses.is_empty() {
            return Err("direccion vacia".into())
        }
        let mut last_error = None;
        let mut connected = None;
        for addr in addresses {
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(stream) => {
                    connected = Some(stream);
                    break
                },
                Err(error) => last_error = Some(error),
            }
        }
        let mut stream = connected.ok_or_else(|| match last_error {
            Some(error) => format!("no pude conectar a {address}: {error}"),
            None => format!("no pude conectar a {address}"),
        })?;
        stream.set_nodelay(true).ok();
        stream.set_write_timeout(Some(WRITE_TIMEOUT)).ok();
        let reader_stream = stream.try_clone().map_err(|e| format!("clonando socket: {e}"))?;
        let shutdown = stream.try_clone().map_err(|e| format!("clonando socket: {e}"))?;
        if let Err(error) = write_line(&mut stream, &ClientMessage::Hello { name: name.to_string() })
            .and_then(|_| write_line(&mut stream, &ClientMessage::Join { room: room.to_string() }))
        {
            let _ = stream.shutdown(Shutdown::Both);
            return Err(format!("enviando handshake: {error}"))
        }
        let events = Arc::new(EventQueue::new(event_capacity));
        let reader = {
            let events = events.clone();
            thread::Builder::new()
                .name("json-reader".into())
                .spawn(move || read_loop(reader_stream, events))
                .map_err(|error| {
                    let _ = stream.shutdown(Shutdown::Both);
                    format!("no pude iniciar el hilo lector: {error}")
                })?
        };
        Ok(Self {
            writer: Arc::new(Mutex::new(stream)),
            shutdown,
            events,
            reader: Mutex::new(Some(reader)),
            disconnecting: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn send_state(
        &self,
        room: i32,
        x: f64,
        y: f64,
        sprite: i32,
        frame: i32,
        xs: f64,
        ys: f64,
        angle: f64,
        visible: bool,
        bbox: [i32; 4],
        depth: f64,
        blend: i32,
        alpha: f64,
    ) -> Result<(), String> {
        let msg = ClientMessage::State { room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha };
        self.send_message(&msg).map_err(|e| format!("enviando estado: {e}"))
    }

    pub fn send_chat(&self, text: &str) -> Result<(), String> {
        self.send_message(&ClientMessage::Chat { text: text.to_string() })
            .map_err(|e| format!("enviando chat: {e}"))
    }

    pub fn send_request(&self, target: u64, kind: &str, info: &str) -> Result<(), String> {
        self.send_message(&ClientMessage::Request {
            target,
            kind: kind.to_string(),
            info: info.to_string(),
        })
        .map_err(|e| format!("enviando solicitud: {e}"))
    }

    pub fn send_battle_input(&self, target: u64, mask: u32, seq: u32, frame: u32, checksum: u32) -> Result<(), String> {
        self.send_message(&ClientMessage::BattleInput { target, mask, seq, frame, checksum })
            .map_err(|e| format!("enviando input de combate: {e}"))
    }

    pub fn send_battle_team(&self, target: u64, data: &str) -> Result<(), String> {
        self.send_message(&ClientMessage::BattleTeam { target, data: data.to_string() })
            .map_err(|e| format!("enviando equipo: {e}"))
    }

    pub fn send_response(&self, target: u64, accept: bool) -> Result<(), String> {
        self.send_message(&ClientMessage::Response { target, accept })
            .map_err(|e| format!("enviando respuesta: {e}"))
    }

    pub fn try_event(&self) -> Option<Event> {
        self.events.pop()
    }

    /// True once the reader stopped. Events queued before it stopped can still be drained.
    pub fn is_closed(&self) -> bool {
        self.events.is_closed()
    }

    pub fn send_leave(&self) {
        let _ = self.send_message(&ClientMessage::Leave);
    }

    fn send_message(&self, message: &ClientMessage) -> Result<(), String> {
        if self.disconnecting.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("conexion cerrada".into())
        }
        let result = {
            let Ok(mut stream) = self.writer.lock() else {
                return Err("socket lock poisoned".into())
            };
            write_line(&mut stream, message)
        };
        if let Err(error) = result {
            if !matches!(
                error.kind(),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData
            ) {
                let _ = self.shutdown.shutdown(Shutdown::Both);
            }
            return Err(error.to_string())
        }
        Ok(())
    }

    /// Send `leave`, shut both halves of the socket down and join the reader thread.
    ///
    /// Idempotent: a second call only reports whether the socket was already closed.
    pub fn disconnect(&self) -> Result<(), String> {
        if self.disconnecting.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Ok(())
        }
        let leave = match self.writer.lock() {
            Ok(mut stream) => write_line(&mut stream, &ClientMessage::Leave)
                .map_err(|error| error.to_string()),
            Err(_) => Err("socket lock poisoned".into()),
        };
        let _ = self.shutdown.shutdown(Shutdown::Both);
        let reader = lock_unpoison(&self.reader).take();
        if let Some(reader) = reader {
            let _ = reader.join();
        }
        self.events.close("conexion cerrada".into());
        leave
    }
}

impl Drop for OnlineClient {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

fn cap_name(name: &str) -> String {
    world::sanitize_name_display(name.as_bytes())
}

fn event_from_message(msg: ServerMessage) -> Event {
    match msg {
        ServerMessage::Welcome { id } => Event::Connected { id },
        ServerMessage::Joined { id, name } => Event::Joined { id, name: cap_name(&name) },
        ServerMessage::State { id, name, room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha } => {
            if !world::valid_position(x, y) {
                return Event::Error("estado remoto fuera de rango".into())
            }
            Event::State {
                id,
                name: cap_name(&name),
                room,
                x,
                y,
                sprite,
                frame,
                xs,
                ys,
                angle,
                visible,
                bbox,
                depth,
                blend,
                alpha,
            }
        },
        ServerMessage::Request { id, name, kind, info } => {
            Event::Request { id, name: cap_name(&name), kind, info }
        },
        ServerMessage::Response { id, name, accept } => {
            Event::Response { id, name: cap_name(&name), accept }
        },
        ServerMessage::BattleTeam { id, name, data } => Event::BattleTeam { id, name: cap_name(&name), data },
        ServerMessage::BattleInput { id, mask, seq, frame, checksum } => {
            Event::BattleInput { id, mask, seq, frame, checksum }
        },
        ServerMessage::Chat { id, name, text } => Event::Chat { id, name: cap_name(&name), text },
        ServerMessage::Left { id } => Event::Left { id },
        ServerMessage::Error { message } => Event::Error(message),
    }
}

fn read_loop(stream: TcpStream, events: Arc<EventQueue>) {
    let mut lines = WireLineReader::new(stream);
    let reason = loop {
        match lines.next_line() {
            Ok(Some(WireLine::Line(bytes))) => {
                let line = match std::str::from_utf8(&bytes) {
                    Ok(line) => line,
                    Err(error) => {
                        events.push(Event::Error(format!("mensaje invalido: {error}")));
                        continue
                    },
                };
                let msg: ServerMessage = match serde_json::from_str(line) {
                    Ok(msg) => msg,
                    Err(error) => {
                        events.push(Event::Error(format!("mensaje invalido: {error}")));
                        continue
                    },
                };
                events.push(event_from_message(msg));
            },
            Ok(Some(WireLine::Overlong)) => {
                events.push(Event::Error("mensaje del servidor demasiado grande".into()));
            },
            Ok(None) => break "servidor cerro la conexion".to_string(),
            Err(error) => break format!("error de red: {error}"),
        }
    };
    events.close(reason);
}

// ---------------------------------------------------------------------------------------
// Relay server

struct Peer {
    name: String,
    room: String,
    sender: SyncSender<ServerMessage>,
    shutdown: TcpStream,
    last_state: Option<ServerMessage>,
}

impl Peer {
    fn send(&self, msg: &ServerMessage) -> bool {
        self.sender.try_send(msg.clone()).is_ok()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.shutdown.shutdown(Shutdown::Both);
    }
}

fn relay_writer(mut stream: TcpStream, receiver: Receiver<ServerMessage>) {
    for message in receiver {
        let Ok(line) = encode_line(&message) else { continue };
        if stream.write_all(&line).and_then(|_| stream.flush()).is_err() {
            break
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}

#[derive(Default)]
pub struct Relay {
    next_id: u64,
    peers: HashMap<u64, Peer>,
}

impl Relay {
    pub fn new() -> Self {
        Self { next_id: 1, peers: HashMap::new() }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    fn send_to(&self, id: u64, msg: &ServerMessage) -> bool {
        self.peers.get(&id).map(|peer| peer.send(msg)).unwrap_or(false)
    }

    fn send_to_or_remove(&mut self, id: u64, msg: &ServerMessage) {
        let failed = match self.peers.get(&id) {
            Some(peer) => !peer.send(msg),
            None => false,
        };
        if failed {
            self.remove(id);
        }
    }

    fn broadcast(&self, room: &str, except: u64, msg: &ServerMessage) -> Vec<u64> {
        let mut failed = Vec::new();
        for (id, peer) in &self.peers {
            if *id != except && peer.room == room && !peer.send(msg) {
                failed.push(*id);
            }
        }
        failed
    }

    fn broadcast_or_prune(&mut self, room: &str, except: u64, msg: &ServerMessage) {
        let failed = self.broadcast(room, except, msg);
        if !failed.is_empty() {
            self.remove_all(failed);
        }
    }

    fn remove_all(&mut self, ids: Vec<u64>) {
        let mut pending = ids;
        while let Some(id) = pending.pop() {
            if let Some(peer) = self.peers.remove(&id) {
                let left = ServerMessage::Left { id };
                pending.extend(self.broadcast(&peer.room, id, &left));
            }
        }
    }

    fn remove(&mut self, id: u64) {
        self.remove_all(vec![id]);
    }

    pub fn handle_line(&mut self, id: u64, line: &str) {
        let msg: ClientMessage = match serde_json::from_str(line) {
            Ok(msg) => msg,
            Err(e) => {
                self.send_to_or_remove(
                    id,
                    &ServerMessage::Error { message: format!("mensaje invalido: {e}") },
                );
                return
            },
        };
        match msg {
            ClientMessage::Hello { name } => {
                let name = name.chars().take(24).collect::<String>();
                if let Some(peer) = self.peers.get_mut(&id) {
                    peer.name = name;
                }
            },
            ClientMessage::Join { room } => {
                let room = room.chars().take(48).collect::<String>();
                let Some(name) = self.peers.get(&id).map(|p| p.name.clone()) else { return };
                if let Some(peer) = self.peers.get_mut(&id) {
                    peer.room = room.clone();
                }
                let existing: Vec<ServerMessage> = self
                    .peers
                    .iter()
                    .filter(|(other, p)| **other != id && p.room == room)
                    .filter_map(|(_, p)| p.last_state.clone())
                    .collect();
                let mut failed = false;
                if let Some(peer) = self.peers.get(&id) {
                    failed = !peer.send(&ServerMessage::Joined { id, name });
                    if !failed {
                        for state in existing {
                            if !peer.send(&state) {
                                failed = true;
                                break
                            }
                        }
                    }
                }
                if failed {
                    self.remove(id);
                }
            },
            ClientMessage::State { room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha } => {
                if !world::valid_position(x, y) {
                    return
                }
                let Some((name, peer_room)) = self.peers.get(&id).map(|p| (p.name.clone(), p.room.clone())) else { return };
                let msg = ServerMessage::State {
                    id,
                    name,
                    room,
                    x,
                    y,
                    sprite,
                    frame,
                    xs,
                    ys,
                    angle,
                    visible,
                    bbox,
                    depth,
                    blend,
                    alpha,
                };
                if let Some(peer) = self.peers.get_mut(&id) {
                    peer.last_state = Some(msg.clone());
                }
                self.broadcast_or_prune(&peer_room, id, &msg);
            },
            ClientMessage::Request { target, kind, info } => {
                let Some((name, peer_room)) = self.peers.get(&id).map(|p| (p.name.clone(), p.room.clone())) else { return };
                let Some(target_room) = self.peers.get(&target).map(|p| p.room.clone()) else {
                    self.send_to_or_remove(
                        id,
                        &ServerMessage::Error { message: "el jugador ya no esta conectado".into() },
                    );
                    return
                };
                if target_room != peer_room {
                    self.send_to_or_remove(
                        id,
                        &ServerMessage::Error { message: "el jugador no esta en tu mapa".into() },
                    );
                    return
                }
                let msg = ServerMessage::Request { id, name, kind: kind.chars().take(8).collect(), info: info.chars().take(64).collect() };
                self.send_to_or_remove(target, &msg);
            },
            ClientMessage::Response { target, accept } => {
                let Some((name, peer_room)) = self.peers.get(&id).map(|p| (p.name.clone(), p.room.clone())) else { return };
                eprintln!("[relay] response id={id} name={name} target={target} accept={accept} room={peer_room}");
                let Some(target_room) = self.peers.get(&target).map(|p| p.room.clone()) else { return };
                if target_room != peer_room {
                    return
                }
                let msg = ServerMessage::Response { id, name, accept };
                self.send_to_or_remove(target, &msg);
            },
            ClientMessage::BattleTeam { target, data } => {
                let Some((name, peer_room)) = self.peers.get(&id).map(|p| (p.name.clone(), p.room.clone())) else { return };
                eprintln!("[relay] battle_team id={id} name={name} target={target} bytes={} room={peer_room}", data.len());
                let Some(target_room) = self.peers.get(&target).map(|p| p.room.clone()) else { return };
                if target_room != peer_room { return }
                let msg = ServerMessage::BattleTeam { id, name, data: data.chars().take(65536).collect() };
                self.send_to_or_remove(target, &msg);
            },
            ClientMessage::BattleInput { target, mask, seq, frame, checksum } => {
                let Some((peer_room, _)) = self.peers.get(&id).map(|p| (p.room.clone(), ())) else { return };
                eprintln!(
                    "[relay] battle_input id={id} target={target} mask={mask} seq={seq} frame={frame} checksum={checksum} room={peer_room}"
                );
                let Some(target_room) = self.peers.get(&target).map(|p| p.room.clone()) else { return };
                if target_room != peer_room { return }
                let msg = ServerMessage::BattleInput { id, mask, seq, frame, checksum };
                self.send_to_or_remove(target, &msg);
            },
            ClientMessage::Chat { text } => {
                if let Some(peer) = self.peers.get(&id) {
                    let msg = ServerMessage::Chat {
                        id,
                        name: peer.name.clone(),
                        text: text.chars().take(300).collect(),
                    };
                    let room = peer.room.clone();
                    self.broadcast_or_prune(&room, id, &msg);
                }
            },
            ClientMessage::Leave => self.remove(id),
        }
    }

    pub fn add_peer(&mut self, id: u64, stream: TcpStream, name: String) -> bool {
        let writer = stream;
        let shutdown = match writer.try_clone() {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("[relay] clonando socket: {error}");
                return false
            },
        };
        writer.set_nodelay(true).ok();
        writer.set_write_timeout(Some(WRITE_TIMEOUT)).ok();
        let (sender, receiver) = mpsc::sync_channel(PEER_QUEUE_CAPACITY);
        if let Err(error) = thread::Builder::new()
            .name(format!("relay-writer-{id}"))
            .spawn(move || relay_writer(writer, receiver))
        {
            eprintln!("[relay] no pude iniciar el escritor: {error}");
            return false
        }
        self.peers.insert(id, Peer { name, room: String::new(), sender, shutdown, last_state: None });
        true
    }

    pub fn alloc(&mut self) -> u64 {
        self.alloc_id()
    }
}

fn reject_stream(mut stream: TcpStream, message: &str) {
    stream.set_write_timeout(Some(WRITE_TIMEOUT)).ok();
    if let Ok(line) = encode_line(&ServerMessage::Error { message: message.into() }) {
        let _ = stream.write_all(&line).and_then(|_| stream.flush());
    }
}

fn relay_reader(stream: TcpStream, id: u64, relay: Arc<Mutex<Relay>>) {
    let mut lines = WireLineReader::new(stream);
    loop {
        match lines.next_line() {
            Ok(Some(WireLine::Line(bytes))) => {
                let line = match std::str::from_utf8(&bytes) {
                    Ok(line) => line,
                    Err(error) => {
                        let mut relay = lock_unpoison(&relay);
                        relay.send_to_or_remove(
                            id,
                            &ServerMessage::Error { message: format!("mensaje invalido: {error}") },
                        );
                        continue
                    },
                };
                lock_unpoison(&relay).handle_line(id, line);
            },
            Ok(Some(WireLine::Overlong)) => {
                let mut relay = lock_unpoison(&relay);
                relay.send_to_or_remove(
                    id,
                    &ServerMessage::Error { message: "mensaje del cliente demasiado grande".into() },
                );
            },
            Ok(None) => break,
            Err(error) => {
                eprintln!("[relay] lectura #{id}: {error}");
                break
            },
        }
    }
    lock_unpoison(&relay).remove(id);
}

pub fn serve(listener: std::net::TcpListener) -> std::io::Result<()> {
    let relay = Arc::new(Mutex::new(Relay::new()));
    loop {
        let (stream, _) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::Interrupted {
                    eprintln!("[relay] accept: {error}");
                    thread::sleep(Duration::from_millis(50));
                }
                continue
            },
        };
        let relay_for_reader = relay.clone();
        let writer = match stream.try_clone() {
            Ok(writer) => writer,
            Err(error) => {
                eprintln!("[relay] clonando socket: {error}");
                reject_stream(stream, "error interno del servidor");
                continue
            },
        };
        let id = {
            let mut relay = lock_unpoison(&relay);
            if relay.peers.len() >= MAX_PEERS {
                None
            } else {
                let id = relay.alloc();
                if relay.add_peer(id, writer, format!("player-{id}")) {
                    let _ = relay.send_to(id, &ServerMessage::Welcome { id });
                    Some(id)
                } else {
                    None
                }
            }
        };
        let Some(id) = id else {
            reject_stream(stream, "servidor lleno");
            continue
        };
        let spawned = thread::Builder::new()
            .name(format!("relay-reader-{id}"))
            .spawn(move || relay_reader(stream, id, relay_for_reader));
        if let Err(error) = spawned {
            eprintln!("[relay] no pude iniciar el lector: {error}");
            lock_unpoison(&relay).remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        time::Instant,
    };

    fn bind_local() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("local addr");
        (listener, address)
    }

    fn next_event(client: &OnlineClient, timeout: Duration) -> Option<Event> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(event) = client.try_event() {
                return Some(event)
            }
            if Instant::now() >= deadline {
                return None
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn read_line(reader: &mut BufReader<TcpStream>) -> Option<String> {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(line),
            Err(error) => panic!("read_line: {error}"),
        }
    }

    #[test]
    fn client_reports_server_disconnect_and_closes_its_reader() {
        let (listener, address) = bind_local();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            assert!(read_line(&mut reader).expect("hello").contains("\"hello\""));
            assert!(read_line(&mut reader).expect("join").contains("\"join\""));
            stream
                .write_all(b"{\"t\":\"welcome\",\"id\":7}\n{\"t\":\"joined\",\"id\":8,\"name\":\"Ada\"}\n")
                .expect("write");
        });
        let client = OnlineClient::connect(&address.to_string(), "Ada", "sala").expect("connect");
        assert!(matches!(next_event(&client, Duration::from_secs(2)), Some(Event::Connected { id: 7 })));
        assert!(matches!(
            next_event(&client, Duration::from_secs(2)),
            Some(Event::Joined { id: 8, .. })
        ));
        assert!(matches!(
            next_event(&client, Duration::from_secs(2)),
            Some(Event::Disconnected(_))
        ));
        assert!(client.is_closed());
        assert!(client.try_event().is_none());
        server.join().expect("server");
    }

    #[test]
    fn dropping_client_sends_leave_then_closes_both_halves() {
        let (listener, address) = bind_local();
        let (tx, rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            assert!(read_line(&mut reader).expect("hello").contains("\"hello\""));
            assert!(read_line(&mut reader).expect("join").contains("\"join\""));
            tx.send(read_line(&mut reader)).ok();
            tx.send(read_line(&mut reader)).ok();
        });
        let client = OnlineClient::connect(&address.to_string(), "Ada", "sala").expect("connect");
        drop(client);
        let leave = rx.recv_timeout(Duration::from_secs(3)).expect("leave").expect("line");
        assert!(leave.contains("\"leave\""), "expected leave, got {leave}");
        assert!(rx.recv_timeout(Duration::from_secs(3)).expect("eof").is_none());
        server.join().expect("server");
    }

    #[test]
    fn client_reports_overlong_and_invalid_utf8_lines_without_stopping() {
        let (listener, address) = bind_local();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            read_line(&mut reader).expect("hello");
            read_line(&mut reader).expect("join");
            let mut overlong = vec![b'a'; MAX_LINE + 10];
            overlong.push(b'\n');
            overlong.extend_from_slice(b"\xff\n");
            overlong.extend_from_slice(b"{\"t\":\"welcome\",\"id\":9}\n");
            stream.write_all(&overlong).expect("write");
        });
        let client = OnlineClient::connect(&address.to_string(), "Ada", "sala").expect("connect");
        assert!(matches!(next_event(&client, Duration::from_secs(2)), Some(Event::Error(_))));
        assert!(matches!(next_event(&client, Duration::from_secs(2)), Some(Event::Error(_))));
        assert!(matches!(next_event(&client, Duration::from_secs(2)), Some(Event::Connected { id: 9 })));
        server.join().expect("server");
    }

    #[test]
    fn oversized_client_message_is_rejected_without_closing_the_connection() {
        let (listener, address) = bind_local();
        let (tx, rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut writer = stream;
            read_line(&mut reader).expect("hello");
            read_line(&mut reader).expect("join");
            writer.write_all(b"{\"t\":\"welcome\",\"id\":1}\n").expect("welcome");
            tx.send(read_line(&mut reader)).ok();
        });
        let client = OnlineClient::connect(&address.to_string(), "Ada", "sala").expect("connect");
        assert!(matches!(next_event(&client, Duration::from_secs(2)), Some(Event::Connected { id: 1 })));
        let error = client.send_chat(&"x".repeat(MAX_LINE)).expect_err("oversized chat");
        assert!(error.contains("too large"), "unexpected {error}");
        client.send_chat("hola").expect("chat after rejection");
        let line = rx.recv_timeout(Duration::from_secs(2)).expect("line").expect("chat line");
        assert!(line.contains("hola"), "unexpected {line}");
        server.join().expect("server");
    }

    #[test]
    fn event_queue_is_bounded_and_always_keeps_the_terminal_event() {
        let queue = EventQueue::new(3);
        for id in 0..10 {
            queue.push(Event::Connected { id });
        }
        queue.close("fin".into());
        let mut events = Vec::new();
        while let Some(event) = queue.pop() {
            events.push(event);
        }
        assert_eq!(events.len(), 3);
        assert!(matches!(events.last(), Some(Event::Disconnected(reason)) if reason == "fin"));
        assert!(queue.is_closed());
        queue.push(Event::Connected { id: 99 });
        assert!(queue.pop().is_none());
    }

    struct RawClient {
        reader: BufReader<TcpStream>,
        writer: TcpStream,
    }

    impl RawClient {
        fn connect(address: SocketAddr, name: &str, room: &str) -> Self {
            let stream = TcpStream::connect(address).expect("raw connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut client = Self { reader: BufReader::new(stream.try_clone().expect("clone")), writer: stream };
            client.send(&ClientMessage::Hello { name: name.into() });
            client.send(&ClientMessage::Join { room: room.into() });
            client
        }

        fn send(&mut self, message: &ClientMessage) {
            let mut line = serde_json::to_vec(message).expect("json");
            line.push(b'\n');
            self.writer.write_all(&line).expect("write");
            self.writer.flush().expect("flush");
        }

        fn read(&mut self) -> Option<serde_json::Value> {
            read_line(&mut self.reader).map(|line| serde_json::from_str(&line).expect("json line"))
        }

        fn expect(&mut self, kind: &str) -> serde_json::Value {
            for _ in 0..10 {
                let value = self.read().expect("relay line");
                if value["t"] == kind {
                    return value
                }
            }
            panic!("did not receive {kind}")
        }
    }

    #[test]
    fn relay_routes_states_and_broadcasts_left_on_socket_eof() {
        let (listener, address) = bind_local();
        thread::spawn(move || {
            let _ = serve(listener);
        });
        let mut first = RawClient::connect(address, "Ada", "sala");
        let first_id = first.expect("welcome")["id"].as_u64().expect("id");
        first.expect("joined");
        let mut second = RawClient::connect(address, "Bob", "sala");
        let second_id = second.expect("welcome")["id"].as_u64().expect("id");
        second.expect("joined");

        first.send(&ClientMessage::State {
            room: 3,
            x: 10.0,
            y: 20.0,
            sprite: 1,
            frame: 0,
            xs: 1.0,
            ys: 1.0,
            angle: 0.0,
            visible: true,
            bbox: [0; 4],
            depth: 0.0,
            blend: 0xffffff,
            alpha: 1.0,
        });
        let state = second.expect("state");
        assert_eq!(state["id"].as_u64(), Some(first_id));
        assert_eq!(state["room"].as_i64(), Some(3));

        drop(first);
        let left = second.expect("left");
        assert_eq!(left["id"].as_u64(), Some(first_id));
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn relay_rejects_overlong_frames_without_dropping_the_session() {
        let (listener, address) = bind_local();
        thread::spawn(move || {
            let _ = serve(listener);
        });
        let stream = TcpStream::connect(address).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
        let mut client = RawClient { reader: BufReader::new(stream.try_clone().expect("clone")), writer: stream };
        client.expect("welcome");
        let mut overlong = vec![b'a'; MAX_LINE + 10];
        overlong.push(b'\n');
        client.writer.write_all(&overlong).expect("overlong");
        client
            .writer
            .write_all(b"{\"t\":\"hello\",\"name\":\"Ada\"}\n{\"t\":\"join\",\"room\":\"sala\"}\n")
            .expect("join");
        client.writer.flush().expect("flush");
        loop {
            let value = client.read().expect("line");
            if value["t"] == "joined" {
                break
            }
            assert_eq!(value["t"], "error");
        }
    }

    #[test]
    fn relay_handle_line_returns_even_when_a_peer_stops_reading() {
        let (listener, address) = bind_local();
        let slow = TcpStream::connect(address).expect("slow connect");
        let (peer, _) = listener.accept().expect("accept");
        let mut relay = Relay::new();
        assert!(relay.add_peer(1, peer, "Lento".into()));
        relay.handle_line(1, "{\"t\":\"join\",\"room\":\"sala\"}");
        let data = "x".repeat(60_000);
        let message = serde_json::to_string(&ClientMessage::BattleTeam { target: 1, data }).expect("json");
        let started = Instant::now();
        for _ in 0..400 {
            relay.handle_line(1, &message);
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "relay dispatch blocked for {:?}",
            started.elapsed()
        );
        assert!(!relay.peers.contains_key(&1), "slow peer must be pruned");
        drop(slow);
    }
}
