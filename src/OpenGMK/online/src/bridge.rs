//! Bridge between Reloaded Online's JSON transport and the community world server.
//!
//! The community server speaks a small newline-delimited text protocol:
//! `WELCOME <id>`, `WOK <id>`, `WP <id> <room> <x> <y> <parts> <name>` and
//! `WDEL <id>`. This bridge accepts the JSON protocol used by the app and
//! translates world positions in both directions. It does not contain or call
//! any code from ROZ.dll or the original game.

use crate::{ClientMessage, ServerMessage};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

type Writer = Arc<Mutex<TcpStream>>;

#[derive(Clone)]
struct Hub {
    peers: Arc<Mutex<HashMap<u64, Writer>>>,
}

impl Hub {
    fn new() -> Self {
        Self { peers: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn add(&self, id: u64, writer: Writer) {
        if let Ok(mut peers) = self.peers.lock() {
            peers.insert(id, writer);
        }
    }

    fn remove(&self, id: u64) {
        if let Ok(mut peers) = self.peers.lock() {
            peers.remove(&id);
        }
    }

    fn send_to(&self, id: u64, message: &ServerMessage) {
        let writer = self.peers.lock().ok().and_then(|peers| peers.get(&id).cloned());
        if let Some(writer) = writer {
            send_json(&writer, message);
        }
    }

    fn broadcast(&self, message: &ServerMessage) {
        let writers: Vec<Writer> = self
            .peers
            .lock()
            .map(|peers| peers.values().cloned().collect())
            .unwrap_or_default();
        for writer in writers {
            send_json(&writer, message);
        }
    }
}

fn send_json(writer: &Writer, message: &ServerMessage) {
    let Ok(mut stream) = writer.lock() else { return };
    let Ok(mut line) = serde_json::to_vec(message) else { return };
    line.push(b'\n');
    let _ = stream.write_all(&line);
    let _ = stream.flush();
}

pub fn run(listener: TcpListener, upstream: String) {
    eprintln!("reloaded-online-bridge escuchando en {}, servidor {upstream}", listener.local_addr().map(|a| a.to_string()).unwrap_or_default());
    let hub = Hub::new();
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("[bridge] accept: {error}");
                continue
            },
        };
        let hub = hub.clone();
        let upstream = upstream.clone();
        thread::spawn(move || handle_client(stream, upstream, hub));
    }
}

fn handle_client(stream: TcpStream, upstream: String, hub: Hub) {
    stream.set_nodelay(true).ok();
    let writer_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("[bridge] clone local: {error}");
            return
        },
    };
    let writer: Writer = Arc::new(Mutex::new(writer_stream));
    let mut reader = BufReader::new(stream);
    let mut name = String::from("Jugador");
    let mut joined = false;
    let mut upstream_id = 0u64;
    let mut upstream_writer: Option<TcpStream> = None;
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => (),
            Err(error) => {
                eprintln!("[bridge] local read: {error}");
                break
            },
        }
        let message: ClientMessage = match serde_json::from_str(line.trim()) {
            Ok(message) => message,
            Err(error) => {
                eprintln!("[bridge] JSON invalido: {error}");
                continue
            },
        };
        match message {
            ClientMessage::Hello { name: requested } => {
                name = requested.chars().take(24).collect();
            },
            ClientMessage::Join { .. } if !joined => {
                match connect_upstream(&upstream, &name) {
                    Ok((id, upstream_reader, upstream_writer_stream)) => {
                        upstream_id = id;
                        upstream_writer = Some(upstream_writer_stream);
                        joined = true;
                        hub.add(id, writer.clone());
                        send_json(&writer, &ServerMessage::Welcome { id });
                        send_json(&writer, &ServerMessage::Joined { id, name: name.clone() });
                        let hub = hub.clone();
                        thread::spawn(move || upstream_loop(id, upstream_reader, hub));
                        eprintln!("[bridge] {name} -> upstream #{id}");
                    },
                    Err(error) => {
                        eprintln!("[bridge] upstream: {error}");
                        send_json(&writer, &ServerMessage::Error { message: error });
                        break
                    },
                }
            },
            ClientMessage::State { room, x, y, sprite, frame, xs, ys, visible, .. } if joined => {
                let Some(upstream) = upstream_writer.as_mut() else { continue };
                let parts = if visible {
                    format!("S,{sprite},{frame},0,0,{xs},{ys}")
                } else {
                    "-".into()
                };
                let line = format!(
                    "WPOS {} {} {} {}\n",
                    room,
                    x.round() as i64,
                    y.round() as i64,
                    parts,
                );
                if let Err(error) = upstream.write_all(line.as_bytes()).and_then(|_| upstream.flush()) {
                    eprintln!("[bridge] upstream write: {error}");
                    break
                }
            },
            ClientMessage::Request { .. } | ClientMessage::Response { .. } | ClientMessage::BattleTeam { .. } | ClientMessage::BattleInput { .. } => (),
            ClientMessage::Chat { .. } => (),
            ClientMessage::Leave => break,
            ClientMessage::Join { .. } => (),
            ClientMessage::State { .. } => (),
        }
    }

    if joined {
        hub.remove(upstream_id);
        eprintln!("[bridge] desconectado #{upstream_id}");
    }
}

fn connect_upstream(address: &str, name: &str) -> Result<(u64, BufReader<TcpStream>, TcpStream), String> {
    let address: SocketAddr = address
        .to_socket_addrs()
        .map_err(|error| format!("direccion invalida: {error}"))?
        .next()
        .ok_or_else(|| "direccion vacia".to_string())?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| format!("no pude conectar a {address}: {error}"))?;
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| format!("clonando socket: {error}"))?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|error| format!("leyendo welcome: {error}"))?;
    let id = line
        .trim()
        .strip_prefix("WELCOME ")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| format!("welcome inesperado: {}", line.trim()))?;
    let mut writer = stream;
    writer
        .write_all(format!("WJOIN {name}\n").as_bytes())
        .and_then(|_| writer.flush())
        .map_err(|error| format!("enviando WJOIN: {error}"))?;
    loop {
        line.clear();
        reader.read_line(&mut line).map_err(|error| format!("leyendo WOK: {error}"))?;
        let trimmed = line.trim();
        if trimmed.starts_with("WOK ") {
            break
        }
        if trimmed.is_empty() {
            return Err("el servidor cerro la conexion".into())
        }
    }
    reader.get_ref().set_read_timeout(None).ok();
    writer.set_write_timeout(Some(Duration::from_secs(5))).ok();
    Ok((id, reader, writer))
}

fn upstream_loop(id: u64, reader: BufReader<TcpStream>, hub: Hub) {
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[bridge] upstream read: {error}");
                break
            },
        };
        if let Some(rest) = line.strip_prefix("WP ") {
            if let Some(state) = parse_wp(rest) {
                hub.broadcast(&state);
            }
        } else if let Some(rest) = line.strip_prefix("WDEL ") {
            if let Ok(remote_id) = rest.trim().parse::<u64>() {
                hub.broadcast(&ServerMessage::Left { id: remote_id });
            }
        }
    }
    hub.send_to(id, &ServerMessage::Error { message: "el servidor remoto cerro la conexion".into() });
    hub.remove(id);
    eprintln!("[bridge] upstream #{id} desconectado");
}

fn parse_wp(rest: &str) -> Option<ServerMessage> {
    let mut fields = rest.splitn(6, ' ');
    let id = fields.next()?.parse::<u64>().ok()?;
    let room = fields.next()?.parse::<i32>().ok()?;
    let x = fields.next()?.parse::<f64>().ok()?;
    let y = fields.next()?.parse::<f64>().ok()?;
    let parts = fields.next()?;
    let name = fields.next().unwrap_or("").to_string();
    let (sprite, frame, xs, ys, dx, dy) = parse_parts(parts);
    let visible = x > -500.0;
    let x = x + dx;
    let y = y + dy;
    Some(ServerMessage::State {
        id,
        name,
        room,
        x,
        y,
        sprite,
        frame,
        xs,
        ys,
        angle: 0.0,
        visible,
        bbox: [
            (x - 8.0).round() as i32,
            (x + 8.0).round() as i32,
            (y - 20.0).round() as i32,
            (y + 4.0).round() as i32,
        ],
        depth: -(y / 100.0),
        blend: 0xffffff,
        alpha: 1.0,
    })
}

fn parse_parts(parts: &str) -> (i32, i32, f64, f64, f64, f64) {
    for part in parts.split(';') {
        let mut fields = part.split(',');
        if fields.next() != Some("S") {
            continue
        }
        let Some(sprite) = fields.next().and_then(|value| value.parse::<i32>().ok()) else { continue };
        let Some(frame) = fields.next().and_then(|value| value.parse::<i32>().ok()) else { continue };
        let Some(dx) = fields.next().and_then(|value| value.parse::<f64>().ok()) else { continue };
        let Some(dy) = fields.next().and_then(|value| value.parse::<f64>().ok()) else { continue };
        let Some(xs) = fields.next().and_then(|value| value.parse::<f64>().ok()) else { continue };
        let Some(ys) = fields.next().and_then(|value| value.parse::<f64>().ok()) else { continue };
        return (sprite, frame, xs, ys, dx, dy)
    }
    (-1, 0, 1.0, 1.0, 0.0, 0.0)
}
