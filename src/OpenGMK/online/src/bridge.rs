//! Bridge between Reloaded Online's JSON transport and the community world server.
//!
//! The community server speaks a small newline-delimited text protocol:
//! `WELCOME <id>`, `WOK <id>`, `WP <id> <room> <x> <y> <parts> <name>` and
//! `WDEL <id>`. This bridge accepts the JSON protocol used by the app and
//! translates world positions in both directions. It does not contain or call
//! any code from ROZ.dll or the original game.

use crate::{
    lock_unpoison, ClientMessage, ServerMessage, WireLine, WireLineReader, CONNECT_TIMEOUT,
    MAX_PEERS, WRITE_TIMEOUT,
};
use std::{
    collections::HashMap,
    io::Write,
    net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

type Writer = Arc<Mutex<TcpStream>>;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT_POLL: Duration = Duration::from_millis(25);

#[derive(Clone)]
struct Hub {
    peers: Arc<Mutex<HashMap<u64, Writer>>>,
}

impl Hub {
    fn new() -> Self {
        Self { peers: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn len(&self) -> usize {
        lock_unpoison(&self.peers).len()
    }

    fn add(&self, id: u64, writer: Writer) -> bool {
        let mut peers = lock_unpoison(&self.peers);
        if peers.len() >= MAX_PEERS {
            return false
        }
        peers.insert(id, writer);
        true
    }

    fn remove(&self, id: u64) {
        lock_unpoison(&self.peers).remove(&id);
    }

    fn send_to(&self, id: u64, message: &ServerMessage) -> bool {
        let writer = lock_unpoison(&self.peers).get(&id).cloned();
        match writer {
            Some(writer) => send_json(&writer, message),
            None => false,
        }
    }

    fn broadcast(&self, message: &ServerMessage) {
        let writers: Vec<Writer> = lock_unpoison(&self.peers).values().cloned().collect();
        for writer in writers {
            send_json(&writer, message);
        }
    }

    fn shutdown_all(&self) {
        let writers: Vec<Writer> = lock_unpoison(&self.peers).values().cloned().collect();
        for writer in writers {
            if let Ok(stream) = writer.lock() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }
}

fn send_json(writer: &Writer, message: &ServerMessage) -> bool {
    let Ok(mut stream) = writer.lock() else { return false };
    let Ok(line) = crate::encode_line(message) else { return false };
    if stream.write_all(&line).and_then(|_| stream.flush()).is_err() {
        let _ = stream.shutdown(Shutdown::Both);
        return false
    }
    true
}

struct Session {
    id: u64,
    hub: Hub,
    local: Writer,
    upstream: Mutex<Option<TcpStream>>,
    closed: AtomicBool,
}

impl Session {
    fn write_upstream(&self, line: &[u8]) -> std::io::Result<()> {
        let mut upstream = lock_unpoison(&self.upstream);
        match upstream.as_mut() {
            Some(stream) => stream.write_all(line).and_then(|_| stream.flush()),
            None => Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "upstream cerrado")),
        }
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return
        }
        if let Some(stream) = lock_unpoison(&self.upstream).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Ok(stream) = self.local.lock() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.hub.remove(self.id);
    }
}

pub fn run(listener: TcpListener, upstream: String) {
    run_until(listener, upstream, Arc::new(AtomicBool::new(false)));
}

/// Run the bridge until `shutdown` becomes true, then close every active session.
pub fn run_until(listener: TcpListener, upstream: String, shutdown: Arc<AtomicBool>) {
    eprintln!(
        "reloaded-online-bridge escuchando en {}, servidor {upstream}",
        listener.local_addr().map(|a| a.to_string()).unwrap_or_default()
    );
    if let Err(error) = listener.set_nonblocking(true) {
        eprintln!("[bridge] no pude poner el listener en modo no bloqueante: {error}");
    }
    let hub = Hub::new();
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).ok();
                let hub = hub.clone();
                let upstream = upstream.clone();
                if let Err(error) = thread::Builder::new()
                    .name("bridge-client".into())
                    .spawn(move || handle_client(stream, upstream, hub))
                {
                    eprintln!("[bridge] no pude iniciar el cliente: {error}");
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            },
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
            Err(error) => {
                eprintln!("[bridge] accept: {error}");
                thread::sleep(ACCEPT_POLL);
            },
        }
    }
    hub.shutdown_all();
}

fn handle_client(stream: TcpStream, upstream: String, hub: Hub) {
    stream.set_nodelay(true).ok();
    stream.set_write_timeout(Some(WRITE_TIMEOUT)).ok();
    let writer_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("[bridge] clone local: {error}");
            return
        },
    };
    let writer: Writer = Arc::new(Mutex::new(writer_stream));
    let mut lines = WireLineReader::new(stream);
    let mut name = String::from("Jugador");
    let mut session: Option<Arc<Session>> = None;

    loop {
        match lines.next_line() {
            Ok(Some(WireLine::Line(bytes))) => {
                let text = match std::str::from_utf8(&bytes) {
                    Ok(text) => text,
                    Err(error) => {
                        eprintln!("[bridge] UTF-8 invalido: {error}");
                        continue
                    },
                };
                let message: ClientMessage = match serde_json::from_str(text.trim()) {
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
                    ClientMessage::Join { .. } if session.is_none() => {
                        if hub.len() >= MAX_PEERS {
                            send_json(
                                &writer,
                                &ServerMessage::Error { message: "el bridge esta lleno".into() },
                            );
                            break
                        }
                        match connect_upstream(&upstream, &name) {
                            Ok((id, upstream_reader, upstream_writer_stream)) => {
                                let session_state = Arc::new(Session {
                                    id,
                                    hub: hub.clone(),
                                    local: writer.clone(),
                                    upstream: Mutex::new(Some(upstream_writer_stream)),
                                    closed: AtomicBool::new(false),
                                });
                                if !hub.add(id, writer.clone()) {
                                    send_json(
                                        &writer,
                                        &ServerMessage::Error { message: "el bridge esta lleno".into() },
                                    );
                                    session_state.close();
                                    break
                                }
                                let welcomed = send_json(&writer, &ServerMessage::Welcome { id })
                                    && send_json(
                                        &writer,
                                        &ServerMessage::Joined { id, name: name.clone() },
                                    );
                                if !welcomed {
                                    session_state.close();
                                    break
                                }
                                let spawned = {
                                    let session_state = session_state.clone();
                                    thread::Builder::new()
                                        .name(format!("bridge-upstream-{id}"))
                                        .spawn(move || upstream_loop(id, upstream_reader, session_state))
                                };
                                if let Err(error) = spawned {
                                    eprintln!("[bridge] no pude iniciar el lector remoto: {error}");
                                    send_json(
                                        &writer,
                                        &ServerMessage::Error {
                                            message: "no pude iniciar el lector remoto".into(),
                                        },
                                    );
                                    session_state.close();
                                    break
                                }
                                session = Some(session_state);
                                eprintln!("[bridge] {name} -> upstream #{id}");
                            },
                            Err(error) => {
                                eprintln!("[bridge] upstream: {error}");
                                send_json(&writer, &ServerMessage::Error { message: error });
                                break
                            },
                        }
                    },
                    ClientMessage::State { room, x, y, sprite, frame, xs, ys, visible, .. }
                        if session.is_some() =>
                    {
                        let Some(session) = session.as_ref() else { break };
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
                        if let Err(error) = session.write_upstream(line.as_bytes()) {
                            eprintln!("[bridge] upstream write: {error}");
                            break
                        }
                    },
                    ClientMessage::Request { .. }
                    | ClientMessage::Response { .. }
                    | ClientMessage::BattleTeam { .. }
                    | ClientMessage::BattleInput { .. } => (),
                    ClientMessage::Chat { .. } => (),
                    ClientMessage::Leave => break,
                    ClientMessage::Join { .. } => (),
                    ClientMessage::State { .. } => (),
                }
            },
            Ok(Some(WireLine::Overlong)) => {
                send_json(
                    &writer,
                    &ServerMessage::Error { message: "mensaje del cliente demasiado grande".into() },
                );
            },
            Ok(None) => break,
            Err(error) => {
                eprintln!("[bridge] local read: {error}");
                break
            },
        }
    }

    if let Some(session) = session.take() {
        session.close();
        eprintln!("[bridge] desconectado #{}", session.id);
    }
}

fn connect_upstream(address: &str, name: &str) -> Result<(u64, WireLineReader<TcpStream>, TcpStream), String> {
    let addresses: Vec<SocketAddr> = address
        .to_socket_addrs()
        .map_err(|error| format!("direccion invalida: {error}"))?
        .collect();
    if addresses.is_empty() {
        return Err("direccion vacia".into())
    }
    let mut last_error = None;
    let mut connected = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                connected = Some(stream);
                break
            },
            Err(error) => last_error = Some(error),
        }
    }
    let mut writer = connected.ok_or_else(|| match last_error {
        Some(error) => format!("no pude conectar a {address}: {error}"),
        None => format!("no pude conectar a {address}"),
    })?;
    writer.set_nodelay(true).ok();
    writer.set_write_timeout(Some(WRITE_TIMEOUT)).ok();
    let reader_stream = writer.try_clone().map_err(|error| format!("clonando socket: {error}"))?;
    let mut reader = WireLineReader::new(reader_stream);
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    let welcome = handshake_line(&mut reader, deadline, "welcome")?;
    let text = String::from_utf8_lossy(&welcome);
    let id = text
        .trim()
        .strip_prefix("WELCOME ")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| format!("welcome inesperado: {}", text.trim()))?;
    writer
        .write_all(format!("WJOIN {name}\n").as_bytes())
        .and_then(|_| writer.flush())
        .map_err(|error| format!("enviando WJOIN: {error}"))?;
    loop {
        let line = handshake_line(&mut reader, deadline, "WOK")?;
        let trimmed = String::from_utf8_lossy(&line);
        let trimmed = trimmed.trim();
        if trimmed.starts_with("WOK ") {
            break
        }
        if trimmed.is_empty() {
            return Err("el servidor cerro la conexion".into())
        }
    }
    reader.get_ref().set_read_timeout(None).ok();
    Ok((id, reader, writer))
}

fn handshake_line(
    reader: &mut WireLineReader<TcpStream>,
    deadline: Instant,
    what: &str,
) -> Result<Vec<u8>, String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| format!("tiempo de espera agotado leyendo {what}"))?;
    let timeout = remaining.min(Duration::from_secs(5)).max(Duration::from_millis(1));
    reader.get_ref().set_read_timeout(Some(timeout)).ok();
    match reader.next_line() {
        Ok(Some(WireLine::Line(line))) => Ok(line),
        Ok(Some(WireLine::Overlong)) => Err("linea del servidor remoto demasiado grande".into()),
        Ok(None) => Err("el servidor cerro la conexion".into()),
        Err(error) => Err(format!("leyendo {what}: {error}")),
    }
}

fn upstream_loop(id: u64, mut lines: WireLineReader<TcpStream>, session: Arc<Session>) {
    let reason = loop {
        match lines.next_line() {
            Ok(Some(WireLine::Line(bytes))) => {
                let line = String::from_utf8_lossy(&bytes);
                if let Some(rest) = line.strip_prefix("WP ") {
                    if let Some(state) = parse_wp(rest) {
                        session.hub.broadcast(&state);
                    }
                } else if let Some(rest) = line.strip_prefix("WDEL ") {
                    if let Ok(remote_id) = rest.trim().parse::<u64>() {
                        session.hub.broadcast(&ServerMessage::Left { id: remote_id });
                    }
                }
            },
            Ok(Some(WireLine::Overlong)) => (),
            Ok(None) => break "el servidor remoto cerro la conexion".to_string(),
            Err(error) => {
                eprintln!("[bridge] upstream read: {error}");
                break "el servidor remoto cerro la conexion".to_string()
            },
        }
    };
    if !session.closed.load(Ordering::SeqCst) {
        session
            .hub
            .send_to(id, &ServerMessage::Error { message: reason.clone() });
    }
    session.close();
    eprintln!("[bridge] upstream #{id} desconectado: {reason}");
}

fn parse_wp(rest: &str) -> Option<ServerMessage> {
    let mut fields = rest.splitn(6, ' ');
    let id = fields.next()?.parse::<u64>().ok()?;
    let room = fields.next()?.parse::<i32>().ok()?;
    let x = fields.next()?.parse::<f64>().ok()?;
    let y = fields.next()?.parse::<f64>().ok()?;
    if !crate::world::valid_position(x, y) {
        return None;
    }
    let parts = fields.next()?;
    let name = cap_name(fields.next().unwrap_or(""));
    let (sprite, frame, xs, ys, dx, dy) = parse_parts(parts);
    let visible = x > -500.0;
    let x = x + dx;
    let y = y + dy;
    if !crate::world::valid_position(x, y) {
        return None;
    }
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

fn cap_name(name: &str) -> String {
    crate::world::sanitize_name_display(name.as_bytes())
}

fn finite_component(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn parse_parts(parts: &str) -> (i32, i32, f64, f64, f64, f64) {
    for part in parts.split(';').take(crate::world::MAX_PARTS_SEGMENTS) {
        let mut fields = part.split(',');
        if fields.next() != Some("S") {
            continue
        }
        let Some(sprite) = fields.next().and_then(|value| value.parse::<i32>().ok()) else { continue };
        if !(0..=3856).contains(&sprite) {
            continue
        }
        let Some(frame) = fields.next().and_then(|value| value.parse::<i32>().ok()) else { continue };
        let Some(dx) = fields.next().and_then(|value| value.parse::<f64>().ok()).and_then(finite_component) else { continue };
        let Some(dy) = fields.next().and_then(|value| value.parse::<f64>().ok()).and_then(finite_component) else { continue };
        let Some(xs) = fields.next().and_then(|value| value.parse::<f64>().ok()).and_then(finite_component) else { continue };
        let Some(ys) = fields.next().and_then(|value| value.parse::<f64>().ok()).and_then(finite_component) else { continue };
        if fields.next().is_some() {
            continue
        }
        return (sprite, frame, xs, ys, dx, dy)
    }
    (-1, 0, 1.0, 1.0, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(rest: &str) -> ServerMessage {
        parse_wp(rest).expect("valid upstream WP")
    }

    fn state_fields(message: ServerMessage) -> (u64, String, i32, f64, f64) {
        match message {
            ServerMessage::State { id, name, room, x, y, .. } => (id, name, room, x, y),
            other => panic!("expected state, got {other:?}"),
        }
    }

    #[test]
    fn wp_accepts_signed_32_bit_positions_and_large_finite_offsets() {
        let (id, _, room, x, y) =
            state_fields(state("1 2 2147483647 -2147483648 S,1,0,0,0,1,1 Ada"));
        assert_eq!((id, room, x, y), (1, 2, 2147483647.0, -2147483648.0));

        let (_, _, _, x, _) =
            state_fields(state("2 2 -1500000000 0 S,1,0,3000000000,0,1,1 Ada"));
        assert_eq!(x, 1_500_000_000.0);

        assert!(parse_wp("3 2 2147483648 0 S,1,0,0,0,1,1 Ada").is_none());
        assert!(parse_wp("4 2 2147483647 0 S,1,0,1,0,1,1 Ada").is_none());
    }

    #[test]
    fn wp_truncates_names_on_char_boundaries_and_strips_controls() {
        let long_name = format!("{}é{}", "a".repeat(crate::world::MAX_REMOTE_NAME_BYTES - 1), "tail");
        let (_, name, _, _, _) = state_fields(state(&format!("1 2 3 4 S,1,0,0,0,1,1 {long_name}")));
        assert_eq!(name.len(), crate::world::MAX_REMOTE_NAME_BYTES - 1);
        assert_eq!(name, "a".repeat(crate::world::MAX_REMOTE_NAME_BYTES - 1));

        let (_, name, _, _, _) = state_fields(state("1 2 3 4 S,1,0,0,0,1,1 A\tB\x7fC"));
        assert_eq!(name, "ABC");
    }

    use crate::{Event, OnlineClient};
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        sync::mpsc,
        time::Instant,
    };

    fn bind_local() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("local addr");
        (listener, address)
    }

    fn read_line(reader: &mut BufReader<TcpStream>) -> Option<String> {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(line),
            Err(error) => panic!("read_line: {error}"),
        }
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

    #[test]
    fn bridge_joins_relays_state_and_closes_upstream_when_client_leaves() {
        let (upstream_listener, upstream_address) = bind_local();
        let (tx, rx) = mpsc::channel::<String>();
        let upstream = thread::spawn(move || {
            let (stream, _) = upstream_listener.accept().expect("upstream accept");
            stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut writer = stream;
            writer.write_all(b"WELCOME 42\n").expect("welcome");
            let join = read_line(&mut reader).expect("WJOIN");
            tx.send(join.trim().to_string()).expect("join channel");
            writer
                .write_all(b"WOK 42\nWP 9 3 10 20 S,5,2,0,0,1,1 Bob\n")
                .expect("wok");
            loop {
                match read_line(&mut reader) {
                    Some(line) => {
                        tx.send(line.trim().to_string()).expect("line channel");
                    },
                    None => {
                        tx.send("EOF".into()).expect("eof channel");
                        break
                    },
                }
            }
        });

        let (bridge_listener, bridge_address) = bind_local();
        let shutdown = Arc::new(AtomicBool::new(false));
        let bridge = {
            let shutdown = shutdown.clone();
            let upstream = upstream_address.to_string();
            thread::spawn(move || run_until(bridge_listener, upstream, shutdown))
        };

        let client = OnlineClient::connect(&bridge_address.to_string(), "Ada", "sala").expect("connect");
        assert!(matches!(
            next_event(&client, Duration::from_secs(3)),
            Some(Event::Connected { id: 42 })
        ));
        assert!(matches!(
            next_event(&client, Duration::from_secs(3)),
            Some(Event::Joined { id: 42, .. })
        ));
        let state = next_event(&client, Duration::from_secs(3)).expect("state");
        assert!(matches!(state, Event::State { id: 9, x, y, .. } if x == 10.0 && y == 20.0));

        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).expect("wjoin"), "WJOIN Ada");
        client
            .send_state(7, 100.4, 200.6, 5, 2, 1.0, 1.0, 0.0, true, [0; 4], 0.0, 0xffffff, 1.0)
            .expect("send state");
        let wpos = rx.recv_timeout(Duration::from_secs(3)).expect("wpos");
        assert!(wpos.starts_with("WPOS 7 100 201 S,5,2,0,0,1,1"), "unexpected {wpos}");

        drop(client);
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).expect("upstream eof"), "EOF");
        shutdown.store(true, Ordering::SeqCst);
        bridge.join().expect("bridge");
        upstream.join().expect("upstream");
    }

    #[test]
    fn bridge_reports_upstream_close_and_tears_down_the_local_socket() {
        let (upstream_listener, upstream_address) = bind_local();
        let upstream = thread::spawn(move || {
            let (stream, _) = upstream_listener.accept().expect("upstream accept");
            stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut writer = stream;
            writer.write_all(b"WELCOME 5\n").expect("welcome");
            read_line(&mut reader).expect("WJOIN");
            writer.write_all(b"WOK 5\n").expect("wok");
        });

        let (bridge_listener, bridge_address) = bind_local();
        let shutdown = Arc::new(AtomicBool::new(false));
        let bridge = {
            let shutdown = shutdown.clone();
            let upstream = upstream_address.to_string();
            thread::spawn(move || run_until(bridge_listener, upstream, shutdown))
        };

        let client = OnlineClient::connect(&bridge_address.to_string(), "Ada", "sala").expect("connect");
        let mut saw_error = false;
        for _ in 0..6 {
            match next_event(&client, Duration::from_secs(2)) {
                Some(Event::Error(message)) => {
                    assert!(message.contains("servidor remoto"), "unexpected {message}");
                    saw_error = true;
                    break
                },
                Some(_) => (),
                None => break,
            }
        }
        assert!(saw_error, "missing upstream disconnect error");
        let mut closed = false;
        for _ in 0..20 {
            if client.is_closed() {
                closed = true;
                break
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(closed, "local socket was not torn down");
        drop(client);
        shutdown.store(true, Ordering::SeqCst);
        bridge.join().expect("bridge");
        upstream.join().expect("upstream");
    }

    #[test]
    fn connect_upstream_rejects_a_malformed_welcome() {
        let (listener, address) = bind_local();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream.write_all(b"NOPE\n").expect("write");
        });
        let error = connect_upstream(&address.to_string(), "Ada")
            .err()
            .expect("malformed welcome");
        assert!(error.contains("welcome inesperado"), "unexpected {error}");
        server.join().expect("server");
    }
}
