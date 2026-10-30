//! Clean-room online transport for Pokemon Reloaded.
//!
//! This crate deliberately knows nothing about the game's GML or assets. It only carries
//! line-delimited JSON between a client and a relay server. The game-side adapter lives in
//! `gm8emulator::online`.

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub mod bridge;

pub const DEFAULT_PORT: u16 = 7777;
pub const MAX_LINE: usize = 64 * 1024;

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
    BattleInput { target: u64, mask: u32 },
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
    BattleInput { id: u64, mask: u32 },
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
    BattleInput { id: u64, mask: u32 },
    Chat { id: u64, name: String, text: String },
    Left { id: u64 },
    Error(String),
    Disconnected(String),
}

fn write_line(stream: &mut TcpStream, value: &ClientMessage) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if line.len() > MAX_LINE {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "message too large"));
    }
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()
}

/// A client connection. The reader thread owns the read half; writes are serialized by a mutex.
pub struct OnlineClient {
    writer: Arc<Mutex<TcpStream>>,
    events: Receiver<Event>,
    reader: Option<thread::JoinHandle<()>>,
}

impl OnlineClient {
    pub fn connect(address: &str, name: &str, room: &str) -> Result<Self, String> {
        let addr: SocketAddr = address
            .to_socket_addrs()
            .map_err(|e| format!("direccion invalida: {e}"))?
            .next()
            .ok_or_else(|| "direccion vacia".to_string())?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .map_err(|e| format!("no pude conectar a {address}: {e}"))?;
        stream.set_nodelay(true).ok();
        let reader_stream = stream.try_clone().map_err(|e| format!("clonando socket: {e}"))?;
        let writer = Arc::new(Mutex::new(stream));
        write_line(&mut writer.lock().unwrap(), &ClientMessage::Hello { name: name.to_string() })
            .map_err(|e| format!("enviando hello: {e}"))?;
        write_line(&mut writer.lock().unwrap(), &ClientMessage::Join { room: room.to_string() })
            .map_err(|e| format!("enviando join: {e}"))?;

        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || read_loop(reader_stream, tx));
        Ok(Self { writer, events: rx, reader: Some(reader) })
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
        let mut stream = self.writer.lock().map_err(|_| "socket lock poisoned".to_string())?;
        write_line(&mut stream, &msg).map_err(|e| format!("enviando estado: {e}"))
    }

    pub fn send_chat(&self, text: &str) -> Result<(), String> {
        let mut stream = self.writer.lock().map_err(|_| "socket lock poisoned".to_string())?;
        write_line(&mut stream, &ClientMessage::Chat { text: text.to_string() })
            .map_err(|e| format!("enviando chat: {e}"))
    }

    pub fn send_request(&self, target: u64, kind: &str, info: &str) -> Result<(), String> {
        let mut stream = self.writer.lock().map_err(|_| "socket lock poisoned".to_string())?;
        write_line(
            &mut stream,
            &ClientMessage::Request { target, kind: kind.to_string(), info: info.to_string() },
        )
        .map_err(|e| format!("enviando solicitud: {e}"))
    }

    pub fn send_battle_input(&self, target: u64, mask: u32) -> Result<(), String> {
        let mut stream = self.writer.lock().map_err(|_| "socket lock poisoned".to_string())?;
        write_line(&mut stream, &ClientMessage::BattleInput { target, mask })
            .map_err(|e| format!("enviando input de combate: {e}"))
    }

    pub fn send_battle_team(&self, target: u64, data: &str) -> Result<(), String> {
        let mut stream = self.writer.lock().map_err(|_| "socket lock poisoned".to_string())?;
        write_line(&mut stream, &ClientMessage::BattleTeam { target, data: data.to_string() })
            .map_err(|e| format!("enviando equipo: {e}"))
    }

    pub fn send_response(&self, target: u64, accept: bool) -> Result<(), String> {
        let mut stream = self.writer.lock().map_err(|_| "socket lock poisoned".to_string())?;
        write_line(&mut stream, &ClientMessage::Response { target, accept })
            .map_err(|e| format!("enviando respuesta: {e}"))
    }

    pub fn try_event(&self) -> Option<Event> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn send_leave(&self) {
        if let Ok(mut stream) = self.writer.lock() {
            let _ = write_line(&mut stream, &ClientMessage::Leave);
        }
    }
}

impl Drop for OnlineClient {
    fn drop(&mut self) {
        self.send_leave();
        if let Some(reader) = self.reader.take() {
            // The reader exits when the socket is closed; do not block the game thread on it.
            let _ = reader;
        }
    }
}

fn read_loop(stream: TcpStream, tx: Sender<Event>) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                let _ = tx.send(Event::Disconnected(e.to_string()));
                return
            },
        };
        if line.len() > MAX_LINE {
            let _ = tx.send(Event::Error("mensaje del servidor demasiado grande".into()));
            continue
        }
        let msg: ServerMessage = match serde_json::from_str(&line) {
            Ok(msg) => msg,
            Err(e) => {
                let _ = tx.send(Event::Error(format!("mensaje invalido: {e}")));
                continue
            },
        };
        let event = match msg {
            ServerMessage::Welcome { id } => Event::Connected { id },
            ServerMessage::Joined { id, name } => Event::Joined { id, name },
            ServerMessage::State { id, name, room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha } => {
                Event::State { id, name, room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha }
            },
            ServerMessage::Request { id, name, kind, info } => Event::Request { id, name, kind, info },
            ServerMessage::Response { id, name, accept } => Event::Response { id, name, accept },
            ServerMessage::BattleTeam { id, name, data } => Event::BattleTeam { id, name, data },
            ServerMessage::BattleInput { id, mask } => Event::BattleInput { id, mask },
            ServerMessage::Chat { id, name, text } => Event::Chat { id, name, text },
            ServerMessage::Left { id } => Event::Left { id },
            ServerMessage::Error { message } => Event::Error(message),
        };
        if tx.send(event).is_err() {
            return
        }
    }
    let _ = tx.send(Event::Disconnected("servidor cerro la conexion".into()));
}

// ---------------------------------------------------------------------------------------
// Relay server

#[derive(Clone)]
struct Peer {
    name: String,
    room: String,
    stream: Arc<Mutex<TcpStream>>,
    last_state: Option<ServerMessage>,
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
        self.next_id += 1;
        id
    }

    fn send(peer: &Peer, msg: &ServerMessage) -> bool {
        let Ok(mut stream) = peer.stream.lock() else { return false };
        let Ok(mut line) = serde_json::to_vec(msg) else { return false };
        line.push(b'\n');
        stream.write_all(&line).is_ok() && stream.flush().is_ok()
    }

    fn broadcast(&self, room: &str, except: u64, msg: &ServerMessage) {
        for (id, peer) in &self.peers {
            if *id != except && peer.room == room {
                let _ = Self::send(peer, msg);
            }
        }
    }

    fn remove(&mut self, id: u64) {
        if let Some(peer) = self.peers.remove(&id) {
            self.broadcast(&peer.room, id, &ServerMessage::Left { id });
        }
    }

    pub fn handle_line(&mut self, id: u64, line: &str) {
        let msg: ClientMessage = match serde_json::from_str(line) {
            Ok(msg) => msg,
            Err(e) => {
                if let Some(peer) = self.peers.get(&id) {
                    let _ = Self::send(peer, &ServerMessage::Error { message: format!("mensaje invalido: {e}") });
                }
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
                if let Some(peer) = self.peers.get(&id) {
                    let _ = Self::send(peer, &ServerMessage::Joined { id, name });
                    for state in existing {
                        let _ = Self::send(peer, &state);
                    }
                }
            },
            ClientMessage::State { room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha } => {
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
                self.broadcast(&peer_room, id, &msg);
            },
            ClientMessage::Request { target, kind, info } => {
                let Some((name, peer_room)) = self.peers.get(&id).map(|p| (p.name.clone(), p.room.clone())) else { return };
                let Some(target_peer) = self.peers.get(&target) else {
                    if let Some(peer) = self.peers.get(&id) {
                        let _ = Self::send(peer, &ServerMessage::Error { message: "el jugador ya no esta conectado".into() });
                    }
                    return
                };
                if target_peer.room != peer_room {
                    if let Some(peer) = self.peers.get(&id) {
                        let _ = Self::send(peer, &ServerMessage::Error { message: "el jugador no esta en tu mapa".into() });
                    }
                    return
                }
                let msg = ServerMessage::Request { id, name, kind: kind.chars().take(8).collect(), info: info.chars().take(64).collect() };
                let _ = Self::send(target_peer, &msg);
            },
            ClientMessage::Response { target, accept } => {
                let Some((name, peer_room)) = self.peers.get(&id).map(|p| (p.name.clone(), p.room.clone())) else { return };
                eprintln!("[relay] response id={id} name={name} target={target} accept={accept} room={peer_room}");
                let Some(target_peer) = self.peers.get(&target) else { return };
                if target_peer.room != peer_room {
                    return
                }
                let msg = ServerMessage::Response { id, name, accept };
                let _ = Self::send(target_peer, &msg);
            },
            ClientMessage::BattleTeam { target, data } => {
                let Some((name, peer_room)) = self.peers.get(&id).map(|p| (p.name.clone(), p.room.clone())) else { return };
                eprintln!("[relay] battle_team id={id} name={name} target={target} bytes={} room={peer_room}", data.len());
                let Some(target_peer) = self.peers.get(&target) else { return };
                if target_peer.room != peer_room { return }
                let msg = ServerMessage::BattleTeam { id, name, data: data.chars().take(65536).collect() };
                let _ = Self::send(target_peer, &msg);
            },
            ClientMessage::BattleInput { target, mask } => {
                let Some((peer_room, _)) = self.peers.get(&id).map(|p| (p.room.clone(), ())) else { return };
                eprintln!("[relay] battle_input id={id} target={target} mask={mask} room={peer_room}");
                let Some(target_peer) = self.peers.get(&target) else { return };
                if target_peer.room != peer_room { return }
                let msg = ServerMessage::BattleInput { id, mask };
                let _ = Self::send(target_peer, &msg);
            },
            ClientMessage::Chat { text } => {
                if let Some(peer) = self.peers.get(&id) {
                    let msg = ServerMessage::Chat {
                        id,
                        name: peer.name.clone(),
                        text: text.chars().take(300).collect(),
                    };
                    self.broadcast(&peer.room, id, &msg);
                }
            },
            ClientMessage::Leave => self.remove(id),
        }
    }

    pub fn add_peer(&mut self, id: u64, stream: TcpStream, name: String) {
        self.peers.insert(
            id,
            Peer { name, room: String::new(), stream: Arc::new(Mutex::new(stream)), last_state: None },
        );
    }

    pub fn alloc(&mut self) -> u64 {
        self.alloc_id()
    }
}

pub fn serve(listener: std::net::TcpListener) -> std::io::Result<()> {
    let relay = Arc::new(Mutex::new(Relay::new()));
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(_) => continue,
        };
        let relay = relay.clone();
        thread::spawn(move || {
            let id = {
                let mut relay = relay.lock().unwrap();
                let id = relay.alloc();
                relay.add_peer(id, stream.try_clone().unwrap(), format!("player-{id}"));
                id
            };
            if let Ok(guard) = relay.lock() {
                if let Some(peer) = guard.peers.get(&id) {
                    let _ = Relay::send(peer, &ServerMessage::Welcome { id });
                }
            }
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(_) => break,
                };
                if line.len() > MAX_LINE {
                    continue
                }
                let mut relay = relay.lock().unwrap();
                relay.handle_line(id, &line);
            }
            relay.lock().unwrap().remove(id);
        });
    }
    Ok(())
}
