use reloaded_online::roz::{decode_hex, encode_hex, RozClient, RozEvent, RozStatus};
use reloaded_online::ConnectionFailure;
use std::{
    io::{BufRead, BufReader, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

const NET_VERSION: &str = "8";
const NET_BUILD: &str = "20261109";
const TEAM_PREFIX: &[u8] = b"TEAM\x1f";
const VERSION_MISMATCH_REPLY: &[u8] = b"NO\x1f";
const CONFIG_SEPARATOR: u8 = 0x1d;

struct MockServer {
    stream: TcpStream,
    lines: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
}

impl MockServer {
    fn send(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).and_then(|_| self.stream.flush()).expect("mock send");
    }

    fn next_line(&self) -> Vec<u8> {
        self.lines.recv_timeout(Duration::from_secs(2)).expect("client protocol line")
    }

    fn next_line_text(&self) -> String {
        String::from_utf8(self.next_line()).expect("client protocol line is utf-8")
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn mock_server() -> (RozClient, MockServer) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
    let address = listener.local_addr().expect("mock server address");
    let client = RozClient::connect_with_timeout(&address.to_string(), Duration::from_secs(2))
        .expect("connect mock server");
    let (stream, _) = listener.accept().expect("accept mock client");
    let reader_stream = stream.try_clone().expect("clone mock stream");
    let (line_tx, lines) = mpsc::channel::<Vec<u8>>();
    let reader = thread::spawn(move || {
        let mut reader = BufReader::new(reader_stream);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if line.last() == Some(&b'\n') {
                        line.pop();
                    }
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    if line_tx.send(line.clone()).is_err() {
                        break;
                    }
                },
            }
        }
    });
    (client, MockServer { stream, lines, reader: Some(reader) })
}

fn message_payload(line: &str) -> Vec<u8> {
    let payload = line.strip_prefix("MSG ").expect("MSG line");
    decode_hex(payload).expect("hex payload")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TeamMeta {
    seed: i32,
    fondo: i32,
    modo: i32,
    sala: i32,
    delay: i32,
}

fn team_payload(meta: &TeamMeta, data: &[u8]) -> Vec<u8> {
    let mut payload = TEAM_PREFIX.to_vec();
    for value in [meta.seed, meta.fondo, meta.modo, meta.sala, meta.delay] {
        payload.extend_from_slice(value.to_string().as_bytes());
        payload.push(0x1e);
    }
    payload.extend_from_slice(data);
    payload
}

fn parse_team_prefix(payload: &[u8]) -> Option<(TeamMeta, Vec<u8>)> {
    let mut rest = payload.strip_prefix(TEAM_PREFIX)?;
    let mut values = [0i32; 5];
    for value in &mut values {
        let end = rest.iter().position(|byte| *byte == 0x1e)?;
        let text = std::str::from_utf8(&rest[..end]).ok()?.trim();
        *value = text.parse::<i32>().unwrap_or(0);
        rest = &rest[end + 1..];
    }
    Some((
        TeamMeta {
            seed: values[0],
            fondo: values[1],
            modo: values[2],
            sala: values[3],
            delay: values[4],
        },
        rest.to_vec(),
    ))
}

fn config_fields() -> Vec<String> {
    [
        "6", "6", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "3", "3", "1", "1", "1", "0", "7",
        "7", "0", "0", "1", "1", "1", "1", "1", "2", "3", "4", "8", "30", "regla1", "regla2", "opciones",
        "tema", "Despejado",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn wcfg_payload(fields: &[String]) -> Vec<u8> {
    let mut payload = b"WCFG\x1f".to_vec();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            payload.push(CONFIG_SEPARATOR);
        }
        payload.extend_from_slice(field.as_bytes());
    }
    payload
}

fn parse_wcfg(payload: &[u8]) -> Option<Vec<String>> {
    let rest = payload.strip_prefix(b"WCFG\x1f")?;
    let text = std::str::from_utf8(rest).ok()?;
    let fields: Vec<String> = text.split(char::from(CONFIG_SEPARATOR)).map(str::to_string).collect();
    (fields.len() >= 37).then_some(fields)
}

fn net_checksum(text: &str) -> i64 {
    let mut checksum = 0i64;
    for byte in text.bytes() {
        checksum = (checksum * 31 + i64::from(byte)) % 1_000_003;
    }
    checksum
}

fn bye_frame(payload: &[u8], epoch: i32) -> Option<i32> {
    let rest = payload.strip_prefix(b"BYE\x1f")?;
    let text = std::str::from_utf8(rest).ok()?;
    let (epoch_text, frame_text) = text.split_once(' ')?;
    (epoch_text.parse::<i32>().ok()? == epoch)
        .then(|| frame_text.parse::<i32>().ok())
        .flatten()
}

fn input_frame(battle_frame: i32, delay: i32) -> i32 {
    battle_frame + delay
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorldOutcome {
    Joined,
    Full,
    Failed,
}

fn handshake_outcome(status: u8) -> WorldOutcome {
    match status {
        6 => WorldOutcome::Joined,
        5 => WorldOutcome::Full,
        _ => WorldOutcome::Failed,
    }
}

#[test]
fn world_handshake_063_matches_gml_order_and_values() {
    let (client, server) = mock_server();

    client.world_join("Jugador").expect("WJOIN");
    client.send_raw_line("WTID 123456").expect("WTID");
    client.send_raw_line("WTSPR 25").expect("WTSPR");
    client.send_raw_line(&format!("WVER {NET_VERSION}")).expect("WVER");
    client.send_raw_line(&format!("WBUILD {NET_BUILD}")).expect("WBUILD");

    assert_eq!(server.next_line_text(), "WJOIN Jugador");
    assert_eq!(server.next_line_text(), "WTID 123456");
    assert_eq!(server.next_line_text(), "WTSPR 25");
    assert_eq!(server.next_line_text(), "WVER 8");
    assert_eq!(server.next_line_text(), "WBUILD 20261109");
}

#[test]
fn world_status_063_joined_full_and_failure_are_distinct() {
    let (joined_client, mut joined_server) = mock_server();
    joined_server.send(b"WELCOME 7432\nWOK 7432\n");
    assert_eq!(
        joined_client.recv_timeout(Duration::from_secs(2)).expect("WELCOME"),
        RozEvent::Welcome { version: 7432, id: 7432 }
    );
    assert_eq!(
        joined_client.recv_timeout(Duration::from_secs(2)).expect("WOK"),
        RozEvent::WorldJoined { side: 7432, id: Some(7432) }
    );
    assert_eq!(joined_client.status(), RozStatus::WorldJoined);
    assert_eq!(joined_client.status().as_u8(), 6);
    assert_eq!(handshake_outcome(joined_client.status().as_u8()), WorldOutcome::Joined);

    let (full_client, mut full_server) = mock_server();
    full_server.send(b"WFULL\n");
    assert_eq!(
        full_client.recv_timeout(Duration::from_secs(2)).expect("WFULL"),
        RozEvent::Full { world: true }
    );
    assert_eq!(full_client.status(), RozStatus::Full);
    assert_eq!(full_client.status().as_u8(), 5);
    assert_eq!(handshake_outcome(full_client.status().as_u8()), WorldOutcome::Full);

    let (failed_client, mut failed_server) = mock_server();
    failed_server.send(b"WELCOME 9\nERROR version incompatible\n");
    assert_eq!(
        failed_client.recv_timeout(Duration::from_secs(2)).expect("WELCOME"),
        RozEvent::Welcome { version: 9, id: 9 }
    );
    assert_eq!(
        failed_client.recv_timeout(Duration::from_secs(2)).expect("version error"),
        RozEvent::ConnectionRejected { failure: ConnectionFailure::Protocol }
    );
    assert_ne!(handshake_outcome(failed_client.status().as_u8()), WorldOutcome::Joined);
    for status in 0..=4 {
        assert_eq!(handshake_outcome(status), WorldOutcome::Failed);
    }
}

#[test]
fn handshake_version_mismatch_063_is_reported_as_protocol_failure() {
    let (client, server) = mock_server();

    client.world_join("Jugador").expect("WJOIN");
    client.send_raw_line("WTID 123456").expect("WTID");
    client.send_raw_line("WTSPR 25").expect("WTSPR");
    client.send_raw_line("WVER 8").expect("WVER");
    client.send_raw_line("WBUILD 20261110").expect("WBUILD");

    assert_eq!(server.next_line_text(), "WJOIN Jugador");
    assert_eq!(server.next_line_text(), "WTID 123456");
    assert_eq!(server.next_line_text(), "WTSPR 25");
    assert_eq!(server.next_line_text(), "WVER 8");
    assert_eq!(server.next_line_text(), "WBUILD 20261110");

    let mut server = server;
    server.send(b"WELCOME 10\nERROR version incompatible\n");
    assert_eq!(
        client.recv_timeout(Duration::from_secs(2)).expect("WELCOME"),
        RozEvent::Welcome { version: 10, id: 10 }
    );
    assert_eq!(
        client.recv_timeout(Duration::from_secs(2)).expect("version error"),
        RozEvent::ConnectionRejected { failure: ConnectionFailure::Protocol }
    );
    assert_ne!(client.status().as_u8(), 6);
}

#[test]
fn team_meta_063_round_trips_five_fields_and_binary_bytes() {
    let (client, mut server) = mock_server();
    let meta = TeamMeta { seed: 1_000_003, fondo: 7, modo: 1, sala: 3, delay: 2 };
    let data = b"Trainer\x1fteam\r\n\xffbytes\r\n".to_vec();
    let payload = team_payload(&meta, &data);

    client.send_message_bytes(&payload).expect("TEAM send");
    let sent = message_payload(&server.next_line_text());
    assert_eq!(sent, payload);
    let (parsed, rest) = parse_team_prefix(&sent).expect("five prefix fields");
    assert_eq!(parsed, meta);
    assert_eq!(rest, data);

    server.send(format!("MSG {}\n", encode_hex(&payload)).as_bytes());
    match client.recv_timeout(Duration::from_secs(2)).expect("TEAM receive") {
        RozEvent::Message(bytes) => {
            assert_eq!(bytes, payload);
            let (parsed, rest) = parse_team_prefix(&bytes).expect("five prefix fields");
            assert_eq!(parsed, meta);
            assert_eq!(rest, data);
        },
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn team_version_mismatch_063_requires_all_five_prefix_fields() {
    assert!(parse_team_prefix(b"TEAM\x1fold-team-only").is_none());
    assert!(parse_team_prefix(b"TEAM\x1f1\x1e2\x1e3\x1e4\x1eteam").is_none());
    assert_eq!(VERSION_MISMATCH_REPLY, b"NO\x1f");

    let meta = TeamMeta { seed: 42, fondo: 0, modo: 0, sala: 0, delay: 2 };
    let (parsed, rest) = parse_team_prefix(&team_payload(&meta, b"")).expect("empty team");
    assert_eq!(parsed, meta);
    assert!(rest.is_empty());
}

#[test]
fn wcfg_063_exchanges_37_fields_in_both_directions() {
    let (client, mut server) = mock_server();
    let fields = config_fields();
    assert_eq!(fields.len(), 37);
    let payload = wcfg_payload(&fields);

    client.send_message_bytes(&payload).expect("WCFG send");
    let sent = message_payload(&server.next_line_text());
    assert_eq!(sent, payload);
    let parsed = parse_wcfg(&sent).expect("37 config fields");
    assert_eq!(parsed, fields);
    assert_eq!(parsed[18], "7");
    assert_eq!(parsed[27], "2");
    assert_eq!(parsed[30], "8");
    assert_eq!(parsed[31], "30");
    assert_eq!(parsed[36], "Despejado");
    assert!(parse_wcfg(&wcfg_payload(&fields[..36])).is_none());

    server.send(format!("MSG {}\n", encode_hex(&payload)).as_bytes());
    match client.recv_timeout(Duration::from_secs(2)).expect("WCFG receive") {
        RozEvent::Message(bytes) => {
            assert_eq!(bytes, payload);
            assert_eq!(parse_wcfg(&bytes).expect("37 config fields"), fields);
        },
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn battle_delay_input_sum_bye_and_checksum_063_match_gml() {
    let (client, mut server) = mock_server();
    let epoch = 1_000_003;
    let delay = 3;

    for frame in 1..=delay {
        client.input_send(epoch, frame, 0).expect("initial IN");
        assert_eq!(server.next_line_text(), format!("IN {epoch} {frame} 0"));
    }
    assert_eq!(input_frame(1, delay), 4);
    assert_eq!(input_frame(30, delay), 33);

    client.input_send(epoch, input_frame(30, delay), 7).expect("IN");
    assert_eq!(server.next_line_text(), format!("IN {epoch} 33 7"));

    let checksum = net_checksum("abc");
    assert_eq!(checksum, 96_354);
    client.checksum_send(epoch, 30, checksum).expect("SUM");
    assert_eq!(server.next_line_text(), format!("SUM {epoch} 30 96354"));

    server.send(format!("IN {epoch} 40 5\nSUM {epoch} 40 222\n").as_bytes());
    assert_eq!(
        client.recv_timeout(Duration::from_secs(2)).expect("IN event"),
        RozEvent::Input { epoch, frame: 40, mask: 5 }
    );
    assert_eq!(
        client.recv_timeout(Duration::from_secs(2)).expect("SUM event"),
        RozEvent::Checksum { epoch, frame: 40, checksum: 222 }
    );

    let bye = format!("BYE\x1f{epoch} 40");
    client.send_message_bytes(bye.as_bytes()).expect("BYE send");
    assert_eq!(message_payload(&server.next_line_text()), bye.as_bytes());

    server.send(format!("MSG {}\n", encode_hex(bye.as_bytes())).as_bytes());
    match client.recv_timeout(Duration::from_secs(2)).expect("BYE receive") {
        RozEvent::Message(bytes) => {
            assert_eq!(bytes, bye.as_bytes());
            assert_eq!(bye_frame(&bytes, epoch), Some(40));
            assert_eq!(bye_frame(&bytes, epoch + 1), None);
        },
        other => panic!("unexpected event: {other:?}"),
    }
}
