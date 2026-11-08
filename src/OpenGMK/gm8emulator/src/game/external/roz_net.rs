//! Native `net_*` externals backed by the clean-room Zibelan/ROZ client.
//!
//! This module replaces `ROZ.dll` on platforms where `external_define` cannot load a
//! real DLL (Android in particular). It only keeps the small, copyable [`RozFn`]
//! identifier in the external call table; sockets, queues and the reader thread live
//! in [`RozRuntime`], which is owned by `Game` and is never serialised in a savestate.
//!
//! Protocol notes:
//! - `MSG` is lowercase hex in both directions, confirmed from static analysis of
//!   `ROZ.dll` (`net_send` and `net_send_file` call the hex encoder). `net_line`
//!   stays raw and only sanitises CR/LF.
//! - `WP`/`WDEL` remain opaque. `net_recv` returns `CMD\x1f` followed by the original
//!   remainder (including the leading space), matching the reference queue format.
//! - `WREQ`/`WANS`/`WCANCEL` are not in the `ROZ.dll` parser dump; their event shape
//!   is an OpenGMK extension and stays marked as a TODO until captured.

use super::dll;
use crate::gml;
use reloaded_online_net::roz::{RozClient, RozEvent, RozStatus, MAX_LINE};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

/// Connection timeout used by `net_connect`.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum number of decoded messages retained for `net_recv`.
const MAX_INBOX: usize = 256;
/// Maximum number of pending remote input frames.
const MAX_INPUTS: usize = 4096;
/// Local safety cap for the `net_input_get` wait. `ROZ.dll` takes the value as a
/// Windows timeout (no documented cap); 60 s prevents a bad GML value from
/// freezing the emulator for minutes/days.
const MAX_INPUT_WAIT_MS: u64 = 60_000;

/// The fifteen `net_*` functions exposed by `ROZ.dll`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RozFn {
    Connect,
    Disconnect,
    Focus,
    InputGet,
    InputSend,
    Join,
    Line,
    Peer,
    Recv,
    Send,
    SendFile,
    SetEpoch,
    Side,
    Status,
    WriteFile,
}

const REAL: dll::ValueType = dll::ValueType::Real;
const STR: dll::ValueType = dll::ValueType::Str;

const CONNECT_ARGS: &[dll::ValueType] = &[STR, REAL];
const NO_ARGS: &[dll::ValueType] = &[];
const GET_ARGS: &[dll::ValueType] = &[REAL, REAL];
const SEND_ARGS: &[dll::ValueType] = &[REAL, REAL];
const JOIN_ARGS: &[dll::ValueType] = &[STR, STR];
const LINE_ARGS: &[dll::ValueType] = &[STR];
const SEND_TEXT_ARGS: &[dll::ValueType] = &[STR];
const SEND_FILE_ARGS: &[dll::ValueType] = &[STR, STR];
const SET_EPOCH_ARGS: &[dll::ValueType] = &[REAL];
const WRITE_FILE_ARGS: &[dll::ValueType] = &[STR, STR];

fn spec(name: &str) -> Option<(RozFn, &'static [dll::ValueType], dll::ValueType)> {
    Some(match name {
        "net_connect" => (RozFn::Connect, CONNECT_ARGS, REAL),
        "net_disconnect" => (RozFn::Disconnect, NO_ARGS, REAL),
        "net_focus" => (RozFn::Focus, NO_ARGS, REAL),
        "net_input_get" => (RozFn::InputGet, GET_ARGS, REAL),
        "net_input_send" => (RozFn::InputSend, SEND_ARGS, REAL),
        "net_join" => (RozFn::Join, JOIN_ARGS, REAL),
        "net_line" => (RozFn::Line, LINE_ARGS, REAL),
        "net_peer" => (RozFn::Peer, NO_ARGS, STR),
        "net_recv" => (RozFn::Recv, NO_ARGS, STR),
        "net_send" => (RozFn::Send, SEND_TEXT_ARGS, REAL),
        "net_send_file" => (RozFn::SendFile, SEND_FILE_ARGS, REAL),
        "net_set_epoch" => (RozFn::SetEpoch, SET_EPOCH_ARGS, REAL),
        "net_side" => (RozFn::Side, NO_ARGS, REAL),
        "net_status" => (RozFn::Status, NO_ARGS, REAL),
        "net_write_file" => (RozFn::WriteFile, WRITE_FILE_ARGS, REAL),
        _ => return None,
    })
}

fn normalize_dll(dll: &str) -> String {
    let base = dll.rsplit(|ch| ch == '/' || ch == '\\').next().unwrap_or(dll);
    let mut name = base.trim().to_ascii_lowercase();
    if let Some(stripped) = name.strip_suffix(".dll") {
        name = stripped.to_string();
    }
    name.retain(|ch| !matches!(ch, '-' | '_' | ' '));
    name
}

fn canonical_symbol(symbol: &str) -> Option<&'static str> {
    let name = symbol
        .strip_prefix("roz_net_")
        .or_else(|| symbol.strip_prefix("net_"))
        .or_else(|| symbol.strip_prefix("roz_"))?;
    Some(match name {
        "connect" => "net_connect",
        "disconnect" => "net_disconnect",
        "focus" => "net_focus",
        "input_get" => "net_input_get",
        "input_send" => "net_input_send",
        "join" => "net_join",
        "line" => "net_line",
        "peer" => "net_peer",
        "recv" => "net_recv",
        "send" => "net_send",
        "send_file" => "net_send_file",
        "set_epoch" => "net_set_epoch",
        "side" => "net_side",
        "status" => "net_status",
        "write_file" => "net_write_file",
        _ => return None,
    })
}

/// Recognise a `ROZ.dll`/`roz*`/`net_*` external and validate its declared signature.
///
/// Returns `Ok(None)` for externals that do not belong to the ROZ family. A symbol
/// that looks like ROZ but has an unknown name or the wrong argument types is an
/// error, so a bad `external_define` is reported instead of silently misbehaving.
pub fn lookup(signature: &dll::ExternalSignature) -> Result<Option<RozFn>, String> {
    let dll_name = normalize_dll(&signature.dll);
    let symbol = signature.symbol.trim().to_ascii_lowercase();
    let looks_like_roz = dll_name.starts_with("roz")
        || dll_name == "nativeonline"
        || dll_name == "opengmkonline"
        || symbol.starts_with("net_")
        || symbol.starts_with("roz_");

    if !looks_like_roz {
        return Ok(None);
    }

    let canonical = canonical_symbol(&symbol).ok_or_else(|| {
        format!("símbolo ROZ desconocido: {} (DLL {})", signature.symbol, signature.dll)
    })?;
    let (function, args, returns) =
        spec(canonical).ok_or_else(|| format!("función ROZ desconocida: {canonical}"))?;

    if signature.type_args != args {
        return Err(format!(
            "firma ROZ inválida para {}: argumentos {:?}, esperados {:?}",
            signature.symbol, signature.type_args, args
        ));
    }
    if signature.type_return != returns {
        return Err(format!(
            "firma ROZ inválida para {}: retorno {:?}, esperado {:?}",
            signature.symbol, signature.type_return, returns
        ));
    }

    // Both cdecl and stdcall are accepted: the native dispatcher does not use the ABI.
    Ok(Some(function))
}

enum PumpResult {
    Event,
    Timeout,
    Closed,
}

/// Owns the live ROZ connection and all non-serialisable network state.
pub struct RozRuntime {
    client: Option<RozClient>,
    inbox: VecDeque<Vec<u8>>,
    inputs: HashMap<(i32, i32), i32>,
    epoch: i32,
    program_directory: PathBuf,
    focused: bool,
}

impl RozRuntime {
    pub fn new(program_directory: impl AsRef<Path>) -> Self {
        Self {
            client: None,
            inbox: VecDeque::new(),
            inputs: HashMap::new(),
            epoch: 0,
            program_directory: program_directory.as_ref().to_path_buf(),
            focused: true,
        }
    }

    /// Keep the desktop window focus flag in sync with `Event::Focus`.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Dispatch one native `net_*` call.
    pub fn call(&mut self, function: RozFn, args: &[gml::Value]) -> gml::Value {
        match function {
            RozFn::Connect => {
                let host = arg_string(args, 0);
                let port = arg_real(args, 1);
                self.net_connect(&host, port).into()
            },
            RozFn::Disconnect => self.net_disconnect().into(),
            RozFn::Focus => self.net_focus().into(),
            RozFn::InputGet => {
                let seq = arg_real(args, 0) as i32;
                let timeout = arg_real(args, 1);
                self.net_input_get(seq, timeout).into()
            },
            RozFn::InputSend => {
                let seq = arg_real(args, 0) as i32;
                let value = arg_real(args, 1) as i32;
                self.net_input_send(seq, value).into()
            },
            RozFn::Join => {
                let room = arg_bytes(args, 0);
                let name = arg_bytes(args, 1);
                self.net_join(room, name).into()
            },
            RozFn::Line => {
                let line = arg_bytes(args, 0);
                self.net_line(line).into()
            },
            RozFn::Peer => gml::Value::Str(self.net_peer().into()),
            RozFn::Recv => gml::Value::Str(self.net_recv().into()),
            RozFn::Send => {
                let text = arg_bytes(args, 0);
                self.net_send(text).into()
            },
            RozFn::SendFile => {
                let prefix = arg_bytes(args, 0);
                let filename = arg_bytes(args, 1);
                self.net_send_file(prefix, filename).into()
            },
            RozFn::SetEpoch => {
                let epoch = arg_real(args, 0) as i32;
                self.net_set_epoch(epoch).into()
            },
            RozFn::Side => (self.net_side() as f64).into(),
            RozFn::Status => (self.net_status() as f64).into(),
            RozFn::WriteFile => {
                let filename = arg_bytes(args, 0);
                let data = arg_bytes(args, 1);
                self.net_write_file(filename, data).into()
            },
        }
    }

    pub fn net_connect(&mut self, host: &str, port: f64) -> bool {
        self.close_client();

        let host = host.trim();
        if host.is_empty() || !port.is_finite() || port < 1.0 || port > 65535.0 {
            return false;
        }
        let address = socket_address(host, port as u16);
        match RozClient::connect_with_timeout(&address, CONNECT_TIMEOUT) {
            Ok(client) => {
                self.client = Some(client);
                true
            },
            Err(error) => {
                eprintln!("[roz-net] no pude conectar a {address}: {error}");
                false
            },
        }
    }

    pub fn net_disconnect(&mut self) -> bool {
        self.close_client();
        true
    }

    fn close_client(&mut self) {
        if let Some(client) = self.client.take() {
            let _ = client.disconnect();
        }
        self.inbox.clear();
        self.inputs.clear();
        // The epoch is a local lockstep setting owned by the GML, not session state.
        // Keep it across disconnect/reconnect; only `net_set_epoch` changes it.
    }

    pub fn net_join(&mut self, room: &[u8], name: &[u8]) -> bool {
        let Some(client) = self.client.as_ref() else { return false };
        // The reference client sends HELLO (player name) before JOIN (room).
        client.hello_bytes(name).and_then(|_| client.join_bytes(room)).is_ok()
    }

    pub fn net_line(&mut self, line: &[u8]) -> bool {
        let Some(client) = self.client.as_ref() else { return false };
        client.send_raw_line_bytes(line).is_ok()
    }

    pub fn net_send(&mut self, payload: &[u8]) -> bool {
        let Some(client) = self.client.as_ref() else { return false };
        // Confirmed by static analysis of ROZ.dll: client -> server `MSG` is hex.
        // The online client applies the hex encoding and the local line limit.
        client.send_message_bytes(payload).is_ok()
    }

    pub fn net_send_file(&mut self, prefix: &[u8], filename: &[u8]) -> bool {
        let Some(path) = self.resolve_path(filename) else { return false };
        let Ok(metadata) = fs::metadata(&path) else { return false };
        let Ok(file_len) = usize::try_from(metadata.len()) else { return false };
        let Some(payload_len) = prefix.len().checked_add(file_len) else { return false };
        // ROZ.dll concatenates prefix + file bytes and hex-encodes the whole thing
        // in a single `MSG` line. Reject early instead of reading a huge file that
        // could never fit in the local line limit.
        if !message_fits(payload_len) {
            return false;
        }
        let Ok(data) = fs::read(path) else { return false };

        let mut payload = Vec::with_capacity(payload_len);
        payload.extend_from_slice(prefix);
        payload.extend_from_slice(&data);
        self.net_send(&payload)
    }

    pub fn net_write_file(&mut self, filename: &[u8], data: &[u8]) -> bool {
        let Some(path) = self.resolve_path(filename) else { return false };
        fs::write(path, data).is_ok()
    }

    pub fn net_focus(&self) -> bool {
        #[cfg(target_os = "android")]
        {
            true
        }
        #[cfg(not(target_os = "android"))]
        {
            self.focused
        }
    }

    pub fn net_set_epoch(&mut self, epoch: i32) -> bool {
        self.epoch = epoch;
        self.inputs.retain(|(event_epoch, _), _| *event_epoch == self.epoch);
        true
    }

    pub fn net_input_send(&mut self, seq: i32, value: i32) -> bool {
        let Some(client) = self.client.as_ref() else { return false };
        client.input_send(self.epoch, seq, value).is_ok()
    }

    pub fn net_input_get(&mut self, seq: i32, timeout_ms: f64) -> f64 {
        self.pump();
        if let Some(value) = self.take_input(seq) {
            return f64::from(value);
        }
        if !self.is_started() {
            return -2.0;
        }
        // Sentinel semantics from ROZ.dll: -2 when the session is not in START,
        // -1 when START is active but the frame did not arrive in time.
        if !timeout_ms.is_finite() || timeout_ms <= 0.0 {
            return -1.0;
        }

        let timeout = Duration::from_millis((timeout_ms as u64).min(MAX_INPUT_WAIT_MS));
        // The cap keeps the deadline addition far below `Instant`'s range.
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return -1.0;
            }
            let slice = remaining.min(Duration::from_millis(50));
            match self.pump_timeout(slice) {
                PumpResult::Event => {
                    if let Some(value) = self.take_input(seq) {
                        return f64::from(value);
                    }
                    if !self.is_started() {
                        return -2.0;
                    }
                },
                PumpResult::Timeout => {},
                // Reader stopped (EOF/network error) while waiting: match the
                // reference's "not in START" sentinel instead of spinning.
                PumpResult::Closed => return -2.0,
            }
        }
    }

    fn take_input(&mut self, seq: i32) -> Option<i32> {
        self.inputs.remove(&(self.epoch, seq & 0xFFFFFF))
    }

    fn is_started(&self) -> bool {
        self.client.as_ref().is_some_and(|client| client.status() == RozStatus::Started)
    }

    pub fn net_status(&mut self) -> u8 {
        self.pump();
        self.client.as_ref().map_or(0, |client| client.status().as_u8())
    }

    pub fn net_side(&mut self) -> i32 {
        self.pump();
        self.client.as_ref().map_or(0, RozClient::side)
    }

    pub fn net_peer(&mut self) -> String {
        self.pump();
        self.client.as_ref().map_or_else(String::new, RozClient::peer)
    }

    pub fn net_recv(&mut self) -> gml::String {
        self.pump();
        match self.inbox.pop_front() {
            Some(bytes) => gml::String::from(bytes),
            None => gml::String::from(""),
        }
    }

    fn pump(&mut self) {
        loop {
            let event = self.client.as_ref().and_then(RozClient::try_recv);
            match event {
                Some(event) => self.handle_event(event),
                None => break,
            }
        }
    }

    fn pump_timeout(&mut self, timeout: Duration) -> PumpResult {
        let event = match self.client.as_ref() {
            Some(client) => client.recv_timeout(timeout),
            None => return PumpResult::Closed,
        };
        match event {
            Some(event) => {
                self.handle_event(event);
                PumpResult::Event
            },
            None if self.client.as_ref().is_some_and(RozClient::is_closed) => PumpResult::Closed,
            None => PumpResult::Timeout,
        }
    }

    fn handle_event(&mut self, event: RozEvent) {
        match event {
            RozEvent::Message(bytes) => self.push_inbox(bytes),
            RozEvent::Input { epoch, frame, mask } => {
                self.inputs.insert((epoch, frame & 0xFFFFFF), mask);
                if self.inputs.len() > MAX_INPUTS {
                    self.inputs.retain(|(event_epoch, _), _| *event_epoch == self.epoch);
                    // Drop arbitrary leftovers if the current epoch alone exceeds
                    // the safety bound. The reference map is unbounded; this only
                    // protects the emulator from a hostile peer.
                    while self.inputs.len() > MAX_INPUTS {
                        let Some(key) = self.inputs.keys().next().copied() else { break };
                        self.inputs.remove(&key);
                    }
                }
            },
            RozEvent::WorldPosition(position) => {
                // ROZ.dll queues `WP` + 0x1F + the original remainder, which starts
                // with the space that followed the command.
                let mut text = b"WP\x1f".to_vec();
                text.extend_from_slice(&position.raw);
                self.push_inbox(text);
            },
            RozEvent::WorldDelete { raw, .. } => {
                let mut text = b"WDEL\x1f".to_vec();
                text.extend_from_slice(&raw);
                self.push_inbox(text);
            },
            // TODO(protocol): `WREQ`/`WANS`/`WCANCEL` were not found in the ROZ.dll
            // parser dump. Their `CMD\x1f...` shape is an OpenGMK extension and is
            // kept as an opaque event format, not confirmed by a capture.
            RozEvent::WorldRequest { id, kind, payload } => {
                self.push_inbox(format!("WREQ\x1f{id} {kind} {payload}").into_bytes());
            },
            RozEvent::WorldAnswer { id, answer } => {
                self.push_inbox(format!("WANS\x1f{id} {answer}").into_bytes());
            },
            RozEvent::WorldCancel { id: Some(id) } => {
                self.push_inbox(format!("WCANCEL\x1f{id}").into_bytes());
            },
            RozEvent::WorldCancel { id: None } => self.push_inbox(b"WCANCEL\x1f".to_vec()),
            RozEvent::ConnectionRejected { failure } => {
                eprintln!("[roz-net] conexión rechazada: {failure}");
                self.client = None;
            },
            RozEvent::Disconnected { reason } => {
                eprintln!("[roz-net] desconectado: {reason}");
                self.client = None;
                // Keep already-received input frames: ROZ.dll checks the input map
                // before looking at the status, so a frame that arrived just before
                // EOF can still be consumed. An explicit `net_disconnect` clears
                // them through `close_client`.
            },
            _ => {},
        }
    }

    fn push_inbox(&mut self, bytes: Vec<u8>) {
        while self.inbox.len() >= MAX_INBOX {
            self.inbox.pop_front();
        }
        self.inbox.push_back(bytes);
    }

    fn resolve_path(&self, filename: &[u8]) -> Option<PathBuf> {
        let filename = String::from_utf8_lossy(filename);
        let relative = Path::new(filename.as_ref());
        if relative.as_os_str().is_empty() || relative.is_absolute() {
            return None;
        }

        let mut clean = PathBuf::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => clean.push(part),
                Component::CurDir => {},
                _ => return None,
            }
        }
        if clean.as_os_str().is_empty() {
            return None;
        }

        let base = self.program_directory.canonicalize().ok()?;
        let candidate = base.join(clean);
        let checked = match candidate.canonicalize() {
            Ok(path) => path,
            Err(_) => {
                // A dangling symlink has metadata but cannot be canonicalised.
                // Reject it instead of letting `fs::write` follow it outside the
                // program directory.
                if candidate.symlink_metadata().is_ok() {
                    return None;
                }
                let parent = candidate.parent()?.canonicalize().ok()?;
                parent.join(candidate.file_name()?)
            },
        };
        checked.starts_with(&base).then_some(checked)
    }
}

fn message_fits(payload_len: usize) -> bool {
    payload_len
        .checked_mul(2)
        .and_then(|len| len.checked_add(4))
        .is_some_and(|line_len| line_len <= MAX_LINE)
}

fn socket_address(host: &str, port: u16) -> String {
    if host.starts_with('[') || !host.contains(':') {
        format!("{host}:{port}")
    } else {
        format!("[{host}]:{port}")
    }
}

fn arg_real(args: &[gml::Value], index: usize) -> f64 {
    match args.get(index) {
        Some(gml::Value::Real(value)) => (*value).into(),
        Some(gml::Value::Str(value)) => value.decode_utf8().trim().parse().unwrap_or(0.0),
        None => 0.0,
    }
}

fn arg_bytes(args: &[gml::Value], index: usize) -> &[u8] {
    match args.get(index) {
        Some(gml::Value::Str(value)) => value.as_ref(),
        _ => &[],
    }
}

fn arg_string(args: &[gml::Value], index: usize) -> String {
    String::from_utf8_lossy(arg_bytes(args, index)).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::external::dll::{CallConv, ExternalSignature, ValueType};
    use std::{
        io::{BufRead, BufReader, Write},
        net::{SocketAddr, TcpListener, TcpStream},
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_dir(name: &str) -> PathBuf {
        let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "roz-net-test-{}-{name}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create test directory");
        dir
    }

    fn spawn_mock<F>(handler: F) -> SocketAddr
    where
        F: FnOnce(TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
            handler(stream);
        });
        addr
    }

    fn read_line(reader: &mut BufReader<TcpStream>) -> String {
        let mut line = String::new();
        let read = reader.read_line(&mut line).expect("read_line");
        assert!(read > 0, "el cliente cerró antes de enviar la línea");
        line.trim_end_matches(|ch| ch == '\r' || ch == '\n').to_string()
    }

    fn connect_runtime(addr: SocketAddr, dir: PathBuf) -> RozRuntime {
        let mut runtime = RozRuntime::new(dir);
        assert!(runtime.net_connect("127.0.0.1", f64::from(addr.port())));
        runtime
    }

    fn pump_event(runtime: &mut RozRuntime) {
        match runtime.pump_timeout(Duration::from_secs(2)) {
            PumpResult::Event => (),
            PumpResult::Timeout => panic!("timeout esperando un evento del servidor"),
            PumpResult::Closed => panic!("la conexión se cerró esperando un evento"),
        }
    }

    fn recv_with_timeout(runtime: &mut RozRuntime) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let value = runtime.net_recv();
            if !value.as_ref().is_empty() {
                return value.as_ref().to_vec();
            }
            assert!(Instant::now() < deadline, "timeout esperando net_recv");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn signature(dll: &str, symbol: &str, args: &[ValueType], returns: ValueType) -> ExternalSignature {
        ExternalSignature {
            dll: dll.to_string(),
            symbol: symbol.to_string(),
            call_conv: CallConv::Cdecl,
            type_args: args.to_vec(),
            type_return: returns,
        }
    }

    #[test]
    fn registry_accepts_roz_dll_and_net_symbols() {
        let connect = signature("C:\\juego\\ROZ.dll", "net_connect", &[ValueType::Str, ValueType::Real], ValueType::Real);
        assert_eq!(lookup(&connect).unwrap(), Some(RozFn::Connect));

        let peer = signature("roz.dll", "net_peer", &[], ValueType::Str);
        assert_eq!(lookup(&peer).unwrap(), Some(RozFn::Peer));

        let recv = signature("ROZ.dll", "net_recv", &[], ValueType::Str);
        assert_eq!(lookup(&recv).unwrap(), Some(RozFn::Recv));
    }

    #[test]
    fn registry_accepts_aliases_and_stdcall() {
        let mut alias = signature("roz-net.dll", "roz_join", &[ValueType::Str, ValueType::Str], ValueType::Real);
        alias.call_conv = CallConv::Stdcall;
        assert_eq!(lookup(&alias).unwrap(), Some(RozFn::Join));

        let prefixed = signature("ROZ.dll", "roz_net_send_file", &[ValueType::Str, ValueType::Str], ValueType::Real);
        assert_eq!(lookup(&prefixed).unwrap(), Some(RozFn::SendFile));
    }

    #[test]
    fn registry_rejects_wrong_types_and_unknown_symbols() {
        let wrong_args = signature("ROZ.dll", "net_connect", &[ValueType::Real, ValueType::Real], ValueType::Real);
        assert!(lookup(&wrong_args).is_err());

        let wrong_return = signature("ROZ.dll", "net_send", &[ValueType::Str], ValueType::Str);
        assert!(lookup(&wrong_return).is_err());

        let unknown = signature("ROZ.dll", "net_explode", &[], ValueType::Real);
        assert!(lookup(&unknown).is_err());
    }

    #[test]
    fn registry_ignores_unrelated_dlls() {
        let other = signature("gmfmodsimple.dll", "FMODSoundAdd", &[ValueType::Real], ValueType::Real);
        assert_eq!(lookup(&other).unwrap(), None);
    }

    #[test]
    fn write_file_rejects_paths_outside_program_directory() {
        let base = std::env::temp_dir().join(format!("roz-net-test-{}", std::process::id()));
        fs::create_dir_all(base.join("sub")).expect("create test directory");
        let runtime = RozRuntime::new(&base);

        assert!(runtime.resolve_path(b"../escape.txt").is_none());
        assert!(runtime.resolve_path(b"/etc/passwd").is_none());
        assert!(runtime.resolve_path(b"sub/../../escape.txt").is_none());
        assert!(runtime.resolve_path(b"sub/file.txt").is_some());

        fs::remove_dir_all(base).ok();
    }

    #[cfg(unix)]
    #[test]
    fn write_file_rejects_dangling_symlinks_that_escape() {
        use std::os::unix::fs::symlink;

        let base = unique_dir("symlink");
        let outside = std::env::temp_dir().join(format!(
            "roz-net-outside-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        symlink(&outside, base.join("link")).expect("create symlink");

        let mut runtime = RozRuntime::new(&base);
        assert!(runtime.resolve_path(b"link").is_none());
        assert!(!runtime.net_write_file(b"link", b"x"));
        assert!(!outside.exists());

        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn input_keys_are_scoped_to_the_current_epoch() {
        let mut runtime = RozRuntime::new(std::env::temp_dir());
        runtime.epoch = 7;
        runtime.inputs.insert((7, 42), 99);
        runtime.inputs.insert((6, 42), 11);
        assert_eq!(runtime.take_input(42), Some(99));
        assert_eq!(runtime.take_input(42), None);

        runtime.net_set_epoch(8);
        assert!(runtime.inputs.is_empty());
    }

    #[test]
    fn inbox_is_bounded_and_drops_the_oldest_message() {
        let dir = unique_dir("inbox");
        let mut runtime = RozRuntime::new(&dir);
        for id in 0..(MAX_INBOX + 10) {
            runtime.push_inbox(id.to_string().into_bytes());
        }
        assert_eq!(runtime.inbox.len(), MAX_INBOX);
        assert_eq!(runtime.inbox.front().map(Vec::as_slice), Some(b"10".as_slice()));
        let last = (MAX_INBOX + 9).to_string();
        assert_eq!(runtime.inbox.back().map(Vec::as_slice), Some(last.as_bytes()));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn socket_address_brackets_ipv6_hosts() {
        assert_eq!(socket_address("64.177.48.215", 5000), "64.177.48.215:5000");
        assert_eq!(socket_address("::1", 5000), "[::1]:5000");
    }

    #[test]
    fn no_session_sentinels_focus_and_invalid_connect() {
        let dir = unique_dir("none");
        let mut runtime = RozRuntime::new(&dir);

        assert_eq!(runtime.net_status(), 0);
        assert_eq!(runtime.net_side(), 0);
        assert_eq!(runtime.net_peer(), "");
        assert!(runtime.net_recv().as_ref().is_empty());
        assert_eq!(runtime.net_input_get(1, 10.0), -2.0);
        assert!(!runtime.net_join(b"sala", b"Ana"));
        assert!(!runtime.net_line(b"WSYNC"));
        assert!(!runtime.net_send(b"hola"));
        assert!(!runtime.net_input_send(1, 2));
        assert!(runtime.net_disconnect());

        assert!(!runtime.net_connect("", 5000.0));
        assert!(!runtime.net_connect("127.0.0.1", 0.0));
        assert!(!runtime.net_connect("127.0.0.1", 65536.0));
        assert!(!runtime.net_connect("127.0.0.1", f64::NAN));

        #[cfg(not(target_os = "android"))]
        {
            runtime.set_focused(false);
            assert!(!runtime.net_focus());
            runtime.set_focused(true);
            assert!(runtime.net_focus());
        }

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn join_line_input_and_hex_send_use_the_expected_wire_format() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let mut lines = Vec::new();
            for _ in 0..5 {
                lines.push(read_line(&mut reader));
            }
            tx.send(lines).expect("send lines");
        });

        let dir = unique_dir("wire");
        let mut runtime = connect_runtime(addr, dir.clone());
        assert!(runtime.net_join(b"sala", b"Ana\nRival"));
        assert!(runtime.net_line(b"WSYNC\r\n"));
        runtime.net_set_epoch(-3);
        assert!(runtime.net_input_send(42, -7));
        assert!(runtime.net_send(b"hola\nmundo"));

        let lines = rx.recv_timeout(Duration::from_secs(2)).expect("lines");
        assert_eq!(
            lines,
            vec![
                "HELLO Ana Rival".to_string(),
                "JOIN sala".to_string(),
                "WSYNC  ".to_string(),
                "IN -3 42 -7".to_string(),
                "MSG 686f6c610a6d756e646f".to_string(),
            ]
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn status_side_and_peer_follow_wait_start_peer_left_and_wok() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<&'static str>();
        let addr = spawn_mock(move |mut stream| {
            stream.write_all(b"WAIT\n").expect("write wait");
            stream.flush().expect("flush");
            while let Ok(line) = cmd_rx.recv_timeout(Duration::from_secs(3)) {
                stream.write_all(line.as_bytes()).expect("write command");
                stream.flush().expect("flush");
            }
        });

        let dir = unique_dir("state");
        let mut runtime = connect_runtime(addr, dir.clone());
        pump_event(&mut runtime);
        assert_eq!(runtime.net_status(), 2);

        cmd_tx.send("START 3 Rival\n").expect("send start");
        pump_event(&mut runtime);
        assert_eq!(runtime.net_status(), 3);
        assert_eq!(runtime.net_side(), 3);
        assert_eq!(runtime.net_peer(), "Rival");

        cmd_tx.send("PEER_LEFT\n").expect("send peer left");
        pump_event(&mut runtime);
        assert_eq!(runtime.net_status(), 4);
        assert_eq!(runtime.net_peer(), "");

        cmd_tx.send("WOK 7\n").expect("send wok");
        pump_event(&mut runtime);
        assert_eq!(runtime.net_status(), 6);
        assert_eq!(runtime.net_side(), 7);

        drop(cmd_tx);
        assert!(runtime.net_disconnect());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn recv_returns_message_bytes_and_preserves_wp_wdel_payloads() {
        let (release_tx, release_rx) = mpsc::channel();
        let addr = spawn_mock(move |mut stream| {
            stream
                .write_all(
                    b"MSG 00ff410a\nWP 9 5 120 240 S,3,2,0,0,1,1 Juan Perez\nWDEL 9\n",
                )
                .expect("write");
            stream.flush().expect("flush");
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });

        let dir = unique_dir("recv");
        let mut runtime = connect_runtime(addr, dir.clone());
        assert_eq!(recv_with_timeout(&mut runtime), vec![0x00, 0xff, b'A', b'\n']);
        assert_eq!(
            recv_with_timeout(&mut runtime),
            b"WP\x1f 9 5 120 240 S,3,2,0,0,1,1 Juan Perez".to_vec()
        );
        assert_eq!(recv_with_timeout(&mut runtime), b"WDEL\x1f 9".to_vec());

        let _ = release_tx.send(());
        assert!(runtime.net_disconnect());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn input_get_handles_input_timeout_epoch_and_peer_left() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<&'static str>();
        let addr = spawn_mock(move |mut stream| {
            stream.write_all(b"START 1 Rival\n").expect("write start");
            stream.flush().expect("flush");
            while let Ok(line) = cmd_rx.recv_timeout(Duration::from_secs(3)) {
                stream.write_all(line.as_bytes()).expect("write command");
                stream.flush().expect("flush");
            }
        });

        let dir = unique_dir("input");
        let mut runtime = connect_runtime(addr, dir.clone());
        pump_event(&mut runtime);
        assert_eq!(runtime.net_status(), 3);

        runtime.net_set_epoch(5);
        cmd_tx.send("IN 5 42 99\n").expect("send input");
        assert_eq!(runtime.net_input_get(42, 2000.0), 99.0);

        // Same frame again was consumed, so a short wait times out with -1.
        assert_eq!(runtime.net_input_get(42, 100.0), -1.0);
        assert_eq!(runtime.net_input_get(43, f64::INFINITY), -1.0);
        assert_eq!(runtime.net_input_get(43, f64::NAN), -1.0);
        assert_eq!(runtime.net_input_get(43, -1.0), -1.0);

        // Changing epoch drops pending frames from the previous epoch.
        runtime.net_set_epoch(6);
        cmd_tx.send("IN 5 44 11\n").expect("send old epoch");
        assert_eq!(runtime.net_input_get(44, 100.0), -1.0);

        // PEER_LEFT wakes a blocked input_get and yields the -2 sentinel.
        cmd_tx.send("PEER_LEFT\n").expect("send peer left");
        assert_eq!(runtime.net_input_get(45, 2000.0), -2.0);
        assert_eq!(runtime.net_status(), 4);

        drop(cmd_tx);
        assert!(runtime.net_disconnect());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn send_file_and_write_file_are_bounded_to_program_directory() {
        let (tx, rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let line = read_line(&mut reader);
            tx.send(line).expect("send line");
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });

        let dir = unique_dir("files");
        fs::write(dir.join("team.bin"), [0x00, 0xff, b'A', b'\n']).expect("write team");
        fs::write(dir.join("big.bin"), vec![0u8; MAX_LINE / 2]).expect("write big");

        let mut runtime = connect_runtime(addr, dir.clone());
        assert!(runtime.net_send_file(b"TAG:", b"team.bin"));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("line"),
            "MSG 5441473a00ff410a"
        );

        assert!(!runtime.net_send_file(b"", b"missing.bin"));
        assert!(!runtime.net_send_file(b"", b"big.bin"));
        assert!(!runtime.net_send_file(b"", b"../outside.bin"));

        assert!(runtime.net_write_file(b"out.bin", &[0, 1, 2, 255]));
        assert_eq!(fs::read(dir.join("out.bin")).expect("read out"), vec![0, 1, 2, 255]);
        assert!(!runtime.net_write_file(b"../escape.bin", b"x"));
        assert!(!runtime.net_write_file(b"/tmp/escaped.bin", b"x"));
        assert!(!dir.parent().expect("parent").join("escape.bin").exists());

        let _ = release_tx.send(());
        assert!(runtime.net_disconnect());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn disconnect_sends_bye_clears_queues_and_preserves_epoch() {
        let (tx, rx) = mpsc::channel();
        let addr = spawn_mock(move |stream| {
            let mut reader = BufReader::new(stream);
            let mut lines = Vec::new();
            loop {
                let line = read_line(&mut reader);
                let bye = line == "BYE";
                lines.push(line);
                if bye {
                    break;
                }
            }
            tx.send(lines).expect("send lines");
        });

        let dir = unique_dir("bye");
        let mut runtime = connect_runtime(addr, dir.clone());
        assert_eq!(runtime.net_status(), 1);
        runtime.net_set_epoch(9);
        runtime.inbox.push_back(b"pending".to_vec());

        assert!(runtime.net_disconnect());
        assert!(runtime.net_disconnect());

        let lines = rx.recv_timeout(Duration::from_secs(2)).expect("lines");
        assert_eq!(lines.last().map(String::as_str), Some("BYE"));
        assert_eq!(runtime.epoch, 9);
        assert!(runtime.inbox.is_empty());
        assert_eq!(runtime.net_status(), 0);
        assert_eq!(runtime.net_side(), 0);
        assert_eq!(runtime.net_peer(), "");
        assert!(runtime.net_recv().as_ref().is_empty());
        assert_eq!(runtime.net_input_get(1, 10.0), -2.0);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn input_received_before_eof_can_still_be_consumed() {
        let addr = spawn_mock(|mut stream| {
            stream
                .write_all(b"START 1 Rival\nIN 1 42 99\n")
                .expect("write");
            stream.flush().expect("flush");
        });

        let dir = unique_dir("eof-input");
        let mut runtime = connect_runtime(addr, dir.clone());
        pump_event(&mut runtime); // START
        pump_event(&mut runtime); // IN
        runtime.net_set_epoch(1);
        assert_eq!(runtime.net_input_get(42, 0.0), 99.0);
        assert_eq!(runtime.net_status(), 0);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn server_close_preserves_queued_messages_and_sets_status_zero() {
        let addr = spawn_mock(|mut stream| {
            stream.write_all(b"MSG 4142\n").expect("write");
            stream.flush().expect("flush");
        });

        let dir = unique_dir("eof");
        let mut runtime = connect_runtime(addr, dir.clone());
        assert_eq!(recv_with_timeout(&mut runtime), b"AB".to_vec());
        assert_eq!(runtime.net_status(), 0);
        assert_eq!(runtime.net_side(), 0);
        assert_eq!(runtime.net_peer(), "");

        fs::remove_dir_all(dir).ok();
    }
}
