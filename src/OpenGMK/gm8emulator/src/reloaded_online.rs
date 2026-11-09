//! Reloaded Online: clean-room world adapter.
//!
//! This module only reads the current player's position from the emulator and draws remote
//! players with the game's own sprites. It does not contain or call any of the game's GML.

use crate::{
    asset,
    compat::f3r,
    game::{Assets, GetAsset, RoomState},
    gml::kernel::parse_gmspr,
    math::Real,
    render::Renderer,
    types::ID,
    util,
};
use reloaded_online_net::{
    failure::ConnectionFailure,
    roz::{RozClient, RozEvent},
    world, Event, OnlineClient,
};
use serde::Deserialize;
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

const SAY_BASE_MS: u128 = 3000;
const SAY_PER_CHAR_MS: u128 = 60;
const SAY_MAX_MS: u128 = 9000;
const EXCLAIM_MS: u128 = 1500;
const SERVER_SENDER: &str = "Servidor";
/// Upper bound for opening the socket and sending the initial handshake. A slow DNS
/// lookup or a stuck peer can only delay the failure, never hide it.
const CONNECT_WATCHDOG: Duration = Duration::from_secs(20);
/// Upper bound for `WELCOME` and the first `WOK`/`WAIT`/`START`. A server that accepts
/// the socket but never finishes the handshake must not leave a half-open session.
const HANDSHAKE_WATCHDOG: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Deserialize)]
struct ReloadedOnlineConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_zibelan_mode")]
    zibelan_mode: String,
    #[serde(default = "default_server")]
    server: String,
    #[serde(default = "default_name")]
    name: String,
    #[serde(default = "default_room")]
    room: String,
    #[serde(default)]
    bridge_upstream: String,
    #[serde(default = "default_battle_epoch")]
    battle_epoch: i32,
}

fn default_mode() -> String {
    "json".into()
}
fn default_zibelan_mode() -> String {
    "world".into()
}
fn default_server() -> String {
    "127.0.0.1:7777".into()
}
fn default_name() -> String {
    "Jugador".into()
}
fn default_room() -> String {
    "mundo".into()
}
fn default_battle_epoch() -> i32 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OnlineMode {
    Json,
    ZibelanWorld,
    ZibelanLobby,
}

impl OnlineMode {
    fn from_config(mode: &str, zibelan_mode: &str) -> Self {
        let mode = mode.trim().to_ascii_lowercase();
        let zibelan_mode = zibelan_mode.trim().to_ascii_lowercase();
        match mode.as_str() {
            "zibelan" | "roz" => {
                if zibelan_mode == "lobby" {
                    Self::ZibelanLobby
                } else {
                    Self::ZibelanWorld
                }
            },
            "world" | "zibelan-world" => Self::ZibelanWorld,
            "lobby" | "zibelan-lobby" => Self::ZibelanLobby,
            _ => Self::Json,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum RemotePartKind {
    Player { sprite: i32 },
    Pokemon { pkmn: i32, mira: i32 },
}

#[derive(Clone, Debug, PartialEq)]
struct RemotePart {
    kind: RemotePartKind,
    frame: i32,
    dx: f64,
    dy: f64,
    xs: f64,
    ys: f64,
}

#[derive(Clone, Debug)]
struct Remote {
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
    render_x: f64,
    render_y: f64,
    drawn: bool,
    parts: Vec<RemotePart>,
    drawn_parts: Vec<bool>,
    battle: bool,
    say: Option<(String, Instant)>,
    exclaim: Option<i32>,
    exclaim_at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PokemonSpritePlan {
    File(PathBuf),
    Mirrored(PathBuf),
    Placeholder { sprite: i32, missing: String },
    Missing,
}

#[derive(Clone, Debug)]
pub struct RemoteOverlay {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub top: f64,
    pub battle: bool,
    pub say: Option<String>,
    pub exclaim: Option<i32>,
}

enum Backend {
    Json(OnlineClient),
    Zibelan(RozClient),
}

enum Connection {
    Waiting(Receiver<Result<Backend, String>>),
    Ready(Backend),
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct IncomingRequest {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub info: String,
}

#[derive(Clone, Debug)]
pub struct IncomingResponse {
    pub id: u64,
    pub name: String,
    pub accept: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerMessageKind {
    Offer,
    Gift,
    Ok,
    No,
}

#[derive(Clone, Debug)]
pub struct PeerMessage {
    pub kind: PeerMessageKind,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct IncomingBattleInput {
    pub id: u64,
    pub mask: u32,
    pub seq: u32,
    pub frame: u32,
    pub checksum: u32,
}

/// The five numeric fields that travel in the `TEAM` prefix used by Zibelan:
/// `TEAM\x1f<seed>\x1e<fondo>\x1e<modo>\x1e<sala>\x1e<delay>\x1e<.TEAM>`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BattleTeamMeta {
    pub seed: i32,
    pub fondo: i32,
    pub modo: i32,
    pub sala: i32,
    pub delay: i32,
}

/// Number of fields in the `WCFG` battle configuration produced by
/// `NetConfigString` and consumed by `NetConfigApply` in Reloaded 0.6.3.
pub const BATTLE_CONFIG_FIELDS: usize = 37;

/// Field separator used by `NetConfigString` (`chr(29)`).
const BATTLE_CONFIG_SEPARATOR: u8 = 0x1d;

/// The battle settings that side 1 sends to side 2 as
/// `WCFG\x1f<field>\x1d<field>\x1d...`, matching Reloaded 0.6.3
/// `NetConfigString`/`NetConfigApply`.
///
/// The order of the fields is part of the wire contract and must not change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BattleConfig {
    pub num_pokemon1: i32,
    pub num_pokemon2: i32,
    pub dificultad1: i32,
    pub dificultad2: i32,
    pub cambios1: i32,
    pub cambios2: i32,
    pub num_obj1: i32,
    pub num_obj2: i32,
    pub obj_asg1: i32,
    pub obj_asg2: i32,
    pub cont_poke1: i32,
    pub cont_poke2: i32,
    pub vel_ataque1: i32,
    pub vel_ataque2: i32,
    pub rest_pp1: i32,
    pub rest_pp2: i32,
    pub show_type: i32,
    pub time_battle: i32,
    pub bttback_index: i32,
    pub music_index: i32,
    pub subita: i32,
    pub battle_mode: i32,
    pub day: i32,
    pub shownumber: i32,
    pub showbattleevents: i32,
    pub showpanelcombate: i32,
    pub shownumber_ps: i32,
    pub mode_battle_back: i32,
    pub world_clima: i32,
    pub clima: i32,
    pub hora: i32,
    pub minuto: i32,
    pub rules_j1: Vec<u8>,
    pub rules_j2: Vec<u8>,
    pub options_bt: Vec<u8>,
    pub music_name: Vec<u8>,
    pub txt_clima: Vec<u8>,
}

impl BattleConfig {
    fn numeric_fields(&self) -> [i32; 32] {
        [
            self.num_pokemon1,
            self.num_pokemon2,
            self.dificultad1,
            self.dificultad2,
            self.cambios1,
            self.cambios2,
            self.num_obj1,
            self.num_obj2,
            self.obj_asg1,
            self.obj_asg2,
            self.cont_poke1,
            self.cont_poke2,
            self.vel_ataque1,
            self.vel_ataque2,
            self.rest_pp1,
            self.rest_pp2,
            self.show_type,
            self.time_battle,
            self.bttback_index,
            self.music_index,
            self.subita,
            self.battle_mode,
            self.day,
            self.shownumber,
            self.showbattleevents,
            self.showpanelcombate,
            self.shownumber_ps,
            self.mode_battle_back,
            self.world_clima,
            self.clima,
            self.hora,
            self.minuto,
        ]
    }

    /// Encodes only the `NetConfigString` body, without the `WCFG\x1f` prefix.
    pub fn encode_body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for value in self.numeric_fields() {
            out.extend_from_slice(value.to_string().as_bytes());
            out.push(BATTLE_CONFIG_SEPARATOR);
        }
        for text in [
            &self.rules_j1,
            &self.rules_j2,
            &self.options_bt,
            &self.music_name,
            &self.txt_clima,
        ] {
            out.extend_from_slice(text);
            out.push(BATTLE_CONFIG_SEPARATOR);
        }
        // `NetConfigString` has no trailing separator: the last field runs to the end.
        out.pop();
        out
    }

    /// Decodes a `NetConfigString` body. Reloaded accepts at least 37 fields and ignores
    /// anything extra, so old/new peers can append fields without breaking a battle.
    pub fn decode(body: &[u8]) -> Result<Self, String> {
        let fields: Vec<&[u8]> = body.split(|byte| *byte == BATTLE_CONFIG_SEPARATOR).collect();
        if fields.len() < BATTLE_CONFIG_FIELDS {
            return Err(format!(
                "WCFG incompleto: {} campos, se esperaban {}",
                fields.len(),
                BATTLE_CONFIG_FIELDS
            ))
        }
        let number = |index: usize| -> Result<i32, String> {
            let field = fields[index];
            let text = std::str::from_utf8(field)
                .map_err(|_| format!("WCFG campo {index} no es texto UTF-8"))?
                .trim();
            text.parse::<i32>()
                .map_err(|_| format!("WCFG campo {index} no es un numero: {text:?}"))
        };
        let text = |index: usize| -> Vec<u8> { fields[index].to_vec() };
        let num = [
            number(0)?, number(1)?, number(2)?, number(3)?, number(4)?, number(5)?, number(6)?,
            number(7)?, number(8)?, number(9)?, number(10)?, number(11)?, number(12)?, number(13)?,
            number(14)?, number(15)?, number(16)?, number(17)?, number(18)?, number(19)?,
            number(20)?, number(21)?, number(22)?, number(23)?, number(24)?, number(25)?,
            number(26)?, number(27)?, number(28)?, number(29)?, number(30)?, number(31)?,
        ];
        Ok(Self {
            num_pokemon1: num[0],
            num_pokemon2: num[1],
            dificultad1: num[2],
            dificultad2: num[3],
            cambios1: num[4],
            cambios2: num[5],
            num_obj1: num[6],
            num_obj2: num[7],
            obj_asg1: num[8],
            obj_asg2: num[9],
            cont_poke1: num[10],
            cont_poke2: num[11],
            vel_ataque1: num[12],
            vel_ataque2: num[13],
            rest_pp1: num[14],
            rest_pp2: num[15],
            show_type: num[16],
            time_battle: num[17],
            bttback_index: num[18],
            music_index: num[19],
            subita: num[20],
            battle_mode: num[21],
            day: num[22],
            shownumber: num[23],
            showbattleevents: num[24],
            showpanelcombate: num[25],
            shownumber_ps: num[26],
            mode_battle_back: num[27],
            world_clima: num[28],
            clima: num[29],
            hora: num[30],
            minuto: num[31],
            rules_j1: text(32),
            rules_j2: text(33),
            options_bt: text(34),
            music_name: text(35),
            txt_clima: text(36),
        })
    }
}

#[derive(Clone, Debug)]
pub struct IncomingBattleTeam {
    pub id: u64,
    pub meta: BattleTeamMeta,
    /// `.TEAM` bytes for Zibelan, or the JSON team for the legacy backend.
    pub data: Vec<u8>,
    pub zibelan: bool,
}

pub struct ReloadedOnline {
    connection: Option<Connection>,
    connection_mode: Option<OnlineMode>,
    connect_deadline: Option<Instant>,
    handshake_deadline: Option<Instant>,
    remotes: HashMap<u64, Remote>,
    local_id: Option<u64>,
    local_name: String,
    server: String,
    room_name: String,
    bridge_upstream: String,
    mode: OnlineMode,
    zibelan_mode: OnlineMode,
    side: i32,
    peer: String,
    battle_epoch: i32,
    battle_peer: Option<u64>,
    pending_request: Option<(u64, String)>,
    battle_active: bool,
    battle_failure: Option<String>,
    last_world_room: Option<i32>,
    last_world_key: Option<String>,
    pending_checksums: HashMap<(i32, i32), u32>,
    last_send: Instant,
    last_draw: Instant,
    last_debug: Instant,
    requests: Vec<IncomingRequest>,
    responses: Vec<IncomingResponse>,
    peer_messages: VecDeque<PeerMessage>,
    peer_left: bool,
    left_peers: VecDeque<u64>,
    battle_teams: Vec<IncomingBattleTeam>,
    battle_configs: VecDeque<BattleConfig>,
    battle_config_error: Option<String>,
    battle_inputs: Vec<IncomingBattleInput>,
    status: String,
    chat: Vec<(String, String)>,
    welcome_notice: Option<String>,
    connection_notice_pending: bool,
    connection_notice_seen: bool,
    world_list: Vec<String>,
    program_directory: PathBuf,
    world: world::WorldState,
    battle_context: world::BattleContext,
    pokemon_plans: HashMap<(i64, i32), PokemonSpritePlan>,
    pokemon_missing: HashMap<String, ()>,
    pokemon_sprites: HashMap<String, Option<Box<asset::Sprite>>>,
    pending_say: HashMap<u64, (String, Instant)>,
    pending_exclaim: HashMap<u64, (i32, Instant)>,
    local_say: Option<(String, Instant)>,
}

impl Default for ReloadedOnline {
    fn default() -> Self {
        Self {
            connection: None,
            connection_mode: None,
            connect_deadline: None,
            handshake_deadline: None,
            remotes: HashMap::new(),
            local_id: None,
            local_name: default_name(),
            server: default_server(),
            room_name: default_room(),
            bridge_upstream: String::new(),
            mode: OnlineMode::Json,
            zibelan_mode: OnlineMode::ZibelanWorld,
            side: 0,
            peer: String::new(),
            battle_epoch: default_battle_epoch(),
            battle_peer: None,
            pending_request: None,
            battle_active: false,
            battle_failure: None,
            last_world_room: None,
            last_world_key: None,
            pending_checksums: HashMap::new(),
            last_send: Instant::now(),
            last_draw: Instant::now(),
            last_debug: Instant::now(),
            requests: Vec::new(),
            responses: Vec::new(),
            peer_messages: VecDeque::new(),
            peer_left: false,
            left_peers: VecDeque::new(),
            battle_teams: Vec::new(),
            battle_configs: VecDeque::new(),
            battle_config_error: None,
            battle_inputs: Vec::new(),
            status: "Reloaded Online desactivado".into(),
            chat: Vec::new(),
            welcome_notice: None,
            connection_notice_pending: false,
            connection_notice_seen: false,
            world_list: Vec::new(),
            program_directory: PathBuf::new(),
            world: world::WorldState::new(None),
            battle_context: world::BattleContext::default(),
            pokemon_plans: HashMap::new(),
            pokemon_missing: HashMap::new(),
            pokemon_sprites: HashMap::new(),
            pending_say: HashMap::new(),
            pending_exclaim: HashMap::new(),
            local_say: None,
        }
    }
}

impl ReloadedOnline {
    pub fn new(program_directory: &str) -> Self {
        let mut online = Self::default();
        online.program_directory = PathBuf::from(program_directory);
        let path = Path::new(program_directory).join("reloaded-online.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return online
        };
        let config: ReloadedOnlineConfig = match serde_json::from_str(&text) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("[reloaded-online] reloaded-online.json invalido: {error}");
                return online
            },
        };
        online.local_name = config.name.clone();
        online.server = config.server.clone();
        online.room_name = config.room.clone();
        online.bridge_upstream = config.bridge_upstream.clone();
        online.mode = OnlineMode::from_config(&config.mode, &config.zibelan_mode);
        online.zibelan_mode = match online.mode {
            OnlineMode::ZibelanLobby => OnlineMode::ZibelanLobby,
            OnlineMode::ZibelanWorld => OnlineMode::ZibelanWorld,
            OnlineMode::Json if config.zibelan_mode.trim().eq_ignore_ascii_case("lobby") => {
                OnlineMode::ZibelanLobby
            },
            OnlineMode::Json => OnlineMode::ZibelanWorld,
        };
        online.battle_epoch = config.battle_epoch.max(1);
        // Online is opt-in: the Multijugador menu starts the connection, and the
        // world socket is opened only after Cargar partida has loaded a save.
        // `enabled` in the JSON is intentionally not an auto-connect switch.
        online
    }

    pub fn is_active(&self) -> bool {
        self.connection.is_some()
    }

    #[allow(dead_code)]
    pub fn start_own(&mut self) {
        self.connect_json(false);
    }

    #[allow(dead_code)]
    pub fn start_remote(&mut self) {
        self.connect_json(true);
    }

    pub fn start_zibelan(&mut self) {
        match self.zibelan_mode {
            OnlineMode::ZibelanLobby => self.start_lobby(),
            _ => self.start_world(),
        }
    }

    pub fn start_world(&mut self) {
        self.connect_zibelan(OnlineMode::ZibelanWorld);
    }

    pub fn start_lobby(&mut self) {
        self.connect_zibelan(OnlineMode::ZibelanLobby);
    }

    pub fn begin_battle(&mut self) {
        self.battle_active = true;
        self.battle_failure = None;
    }

    pub fn end_battle(&mut self) {
        self.battle_active = false;
        self.battle_peer = None;
        self.pending_request = None;
        self.battle_teams.clear();
        self.battle_configs.clear();
        self.battle_config_error = None;
        self.battle_inputs.clear();
        self.pending_checksums.clear();
    }

    fn fail_battle(&mut self, reason: impl Into<String>) {
        if self.battle_active {
            self.battle_failure = Some(reason.into());
        }
        self.end_battle();
    }

    pub fn take_battle_failure(&mut self) -> Option<String> {
        self.battle_failure.take()
    }

    pub fn stop(&mut self) {
        if self.battle_active {
            self.battle_failure = Some("sesion online detenida".into());
        }
        if let Some(connection) = self.connection.take() {
            if let Connection::Ready(Backend::Zibelan(client)) = connection {
                let _ = client.end_world();
                let _ = client.disconnect();
            }
        }
        self.connection_mode = None;
        self.connect_deadline = None;
        self.handshake_deadline = None;
        self.remotes.clear();
        self.local_id = None;
        self.world.clear();
        self.world.set_local_id(None);
        self.side = 0;
        self.pending_request = None;
        self.peer.clear();
        self.battle_peer = None;
        self.last_world_room = None;
        self.last_world_key = None;
        self.pending_checksums.clear();
        self.requests.clear();
        self.responses.clear();
        self.peer_messages.clear();
        self.peer_left = false;
        self.left_peers.clear();
        self.battle_teams.clear();
        self.battle_inputs.clear();
        self.pending_say.clear();
        self.pending_exclaim.clear();
        self.local_say = None;
        self.connection_notice_pending = false;
        self.connection_notice_seen = false;
        self.end_battle();
        self.status = "Reloaded Online desconectado".into();
    }

    fn connect_json(&mut self, use_bridge: bool) {
        self.stop();
        let mut server = self.server.clone();
        if use_bridge {
            let upstream = if self.bridge_upstream.trim().is_empty() {
                "64.177.48.215:5000".to_string()
            } else {
                self.bridge_upstream.trim().to_string()
            };
            match std::net::TcpListener::bind("127.0.0.1:0") {
                Ok(listener) => {
                    let local = listener
                        .local_addr()
                        .map(|address| format!("127.0.0.1:{}", address.port()))
                        .unwrap_or_else(|_| "127.0.0.1:7778".into());
                    eprintln!("[reloaded-online] bridge local en {local}, servidor remoto {upstream}");
                    thread::spawn(move || reloaded_online_net::bridge::run(listener, upstream));
                    server = local;
                },
                Err(error) => {
                    eprintln!("[reloaded-online] no pude abrir el bridge local: {error}");
                },
            }
        }
        let name = self.local_name.clone();
        let room = self.room_name.clone();
        let log_server = server.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = OnlineClient::connect(&server, &name, &room).map(Backend::Json);
            let _ = tx.send(result);
        });
        eprintln!("[reloaded-online] conectando a {} como {} en {}", log_server, self.local_name, self.room_name);
        self.connection = Some(Connection::Waiting(rx));
        self.connection_mode = Some(OnlineMode::Json);
        self.connect_deadline = Some(Instant::now() + CONNECT_WATCHDOG);
        self.handshake_deadline = None;
        self.status = format!("Reloaded Online conectando a {log_server}");
    }

    fn connect_zibelan(&mut self, mode: OnlineMode) {
        self.stop();
        let server = self.server.clone();
        let name = self.local_name.clone();
        let room = self.room_name.clone();
        let log_server = server.clone();
        let log_name = name.clone();
        let log_room = room.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = (|| {
                let client = RozClient::connect_with_timeout(&server, Duration::from_secs(5))?;
                match mode {
                    OnlineMode::ZibelanLobby => {
                        client.hello(&name)?;
                        client.join(&room)?;
                    },
                    _ => {
                        client.world_join(&name)?;
                        // Protocol metadata used by the Zibelan server to reject mixed
                        // client versions before a battle starts.
                        client.send_raw_line("WVER 6")?;
                        client.send_raw_line("WBUILD 20261103")?;
                    }
                }
                Ok(Backend::Zibelan(client))
            })();
            let _ = tx.send(result);
        });
        if mode == OnlineMode::ZibelanLobby {
            eprintln!("[reloaded-online] Zibelan lobby {log_server} como {log_name} en {log_room}");
        } else {
            eprintln!("[reloaded-online] Zibelan mundo {log_server} como {log_name}");
        }
        self.connection = Some(Connection::Waiting(rx));
        self.connection_mode = Some(mode);
        self.connect_deadline = Some(Instant::now() + CONNECT_WATCHDOG);
        self.handshake_deadline = None;
        self.status = format!("Zibelan conectando a {log_server}");
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn local_name(&self) -> &str {
        &self.local_name
    }

    pub fn local_id(&self) -> Option<u64> {
        self.local_id
    }

    pub fn battle_side(&self) -> Option<i32> {
        (self.side > 0).then_some(self.side)
    }

    pub fn battle_seed(&self, target: u64) -> i32 {
        if self.side > 0 {
            (i64::from(self.battle_epoch.max(1)) * 1_000_003).min(i64::from(i32::MAX)) as i32
        } else {
            let local = self.local_id.unwrap_or(0);
            (local.min(target) * 1_000_003 + local.max(target)) as i32
        }
    }

    /// The seed/epoch chosen for the current battle. `NetBattleBegin` uses it for both the
    /// RNG and the `IN`/`SUM` epoch.
    pub fn set_battle_epoch(&mut self, epoch: i32) {
        self.battle_epoch = epoch.max(1);
    }

    /// True when the current backend speaks the Zibelan text protocol.
    pub fn is_zibelan(&self) -> bool {
        matches!(self.connection, Some(Connection::Ready(Backend::Zibelan(_))))
    }

    /// Legacy JSON backend: send the JSON team snapshot unchanged.
    pub fn send_battle_team_json(&mut self, target: u64, data: &str) -> bool {
        self.battle_peer = Some(target);
        let result = match &self.connection {
            Some(Connection::Ready(Backend::Json(client))) => client.send_battle_team(target, data),
            _ => return false,
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            },
        }
    }

    /// Zibelan backend: `TEAM\x1f<seed>\x1e<fondo>\x1e<modo>\x1e<sala>\x1e<delay>\x1e`
    /// followed by the `.TEAM` bytes. `send_message_bytes` hex-encodes the whole payload,
    /// matching `net_send_file(prefix, path)`.
    pub fn send_battle_team(&mut self, target: u64, data: &[u8], meta: &BattleTeamMeta) -> bool {
        self.battle_peer = Some(target);
        let result = match &self.connection {
            Some(Connection::Ready(Backend::Zibelan(client))) => {
                let payload = zibelan_team_payload(data, meta);
                client.send_message_bytes(&payload)
            },
            _ => return false,
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            },
        }
    }

    /// Zibelan side 1: `WCFG\x1f` followed by the 37 `NetConfigString` fields.
    pub fn send_battle_config(&mut self, config: &BattleConfig) -> bool {
        let result = match &self.connection {
            Some(Connection::Ready(Backend::Zibelan(client))) => {
                client.send_message_bytes(&zibelan_config_payload(config))
            },
            _ => return false,
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            },
        }
    }

    pub fn send_battle_input(&mut self, target: u64, mask: u32, seq: u32, frame: u32, checksum: u32) -> bool {
        let result = match &self.connection {
            Some(Connection::Ready(Backend::Json(client))) => {
                return match client.send_battle_input(target, mask, seq, frame, checksum) {
                    Ok(()) => true,
                    Err(error) => {
                        self.status = error;
                        false
                    },
                }
            },
            Some(Connection::Ready(Backend::Zibelan(client))) => {
                let epoch = self.battle_epoch;
                let result = client.input_send(epoch, frame, mask);
                if result.is_ok() && frame % 30 == 0 {
                    let _ = client.checksum_send(epoch, frame, checksum);
                }
                result
            },
            _ => return false,
        };
        match result {
            Ok(()) => {
                self.battle_peer = Some(target);
                true
            },
            Err(error) => {
                self.status = error;
                false
            },
        }
    }

    pub fn take_battle_input(&mut self) -> Option<IncomingBattleInput> {
        if self.battle_inputs.is_empty() {
            None
        } else {
            Some(self.battle_inputs.remove(0))
        }
    }

    pub fn take_battle_team(&mut self) -> Option<IncomingBattleTeam> {
        if self.battle_teams.is_empty() {
            None
        } else {
            Some(self.battle_teams.remove(0))
        }
    }

    pub fn take_battle_config(&mut self) -> Option<BattleConfig> {
        self.battle_configs.pop_front()
    }

    pub fn take_battle_config_error(&mut self) -> Option<String> {
        self.battle_config_error.take()
    }

    pub fn nearest_remote(&self, room_id: i32, x: f64, y: f64, max_distance: f64) -> Option<(u64, String)> {
        let selected = self.remotes
            .iter()
            .filter(|(_, remote)| remote.visible && remote.room == room_id)
            .filter_map(|(id, remote)| {
                let dx = remote.render_x - x;
                let dy = remote.render_y - y;
                let distance = (dx * dx + dy * dy).sqrt();
                (distance <= max_distance).then_some((*id, remote.name.clone(), distance))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, name, _)| (id, name));
        if let Some((id, name)) = &selected {
            eprintln!("[online] target cercano id={id} name={name} local={}", self.local_name);
        }
        selected
    }

    pub fn send_request(&mut self, target: u64, kind: &str) -> bool {
        let result = match &self.connection {
            Some(Connection::Ready(Backend::Json(client))) => client.send_request(target, kind, ""),
            Some(Connection::Ready(Backend::Zibelan(client))) => client.request(target, kind, ""),
            _ => return false,
        };
        match result {
            Ok(()) => {
                self.pending_request = Some((target, kind.to_string()));
                eprintln!("[online] WREQ enviado target={target} kind={kind}");
                true
            },
            Err(error) => {
                eprintln!("[online] WREQ fallo target={target} kind={kind}: {error}");
                self.status = error;
                false
            },
        }
    }

    pub fn send_response(&mut self, target: u64, accept: bool) -> bool {
        let result = match &self.connection {
            Some(Connection::Ready(Backend::Json(client))) => client.send_response(target, accept),
            Some(Connection::Ready(Backend::Zibelan(client))) => {
                client.response(target, if accept { 1 } else { 0 })
            },
            _ => return false,
        };
        match result {
            Ok(()) => {
                eprintln!("[online] WANS enviado target={target} accept={accept}");
                true
            },
            Err(error) => {
                eprintln!("[online] WANS fallo target={target} accept={accept}: {error}");
                self.status = error;
                false
            },
        }
    }

    pub fn take_request(&mut self) -> Option<IncomingRequest> {
        if self.requests.is_empty() {
            None
        } else {
            Some(self.requests.remove(0))
        }
    }

    pub fn take_response(&mut self) -> Option<IncomingResponse> {
        if self.responses.is_empty() {
            None
        } else {
            Some(self.responses.remove(0))
        }
    }

    pub fn take_peer_message(&mut self) -> Option<PeerMessage> {
        self.peer_messages.pop_front()
    }

    /// Drops messages queued before a trade session started, so an old offer
    /// or confirmation cannot be adopted by the new session.
    pub fn clear_peer_messages(&mut self) {
        self.peer_messages.clear();
    }

    /// Drops only queued `OK`/`NO` controls. A legitimately early `OFFER`
    /// from a legacy peer is kept, while a stale confirmation cannot cancel or
    /// complete the new session.
    pub fn clear_peer_controls(&mut self) {
        self.peer_messages.retain(|message| {
            !matches!(
                message.kind,
                PeerMessageKind::Ok | PeerMessageKind::No
            )
        });
    }

    /// True while a backend is connected. A failed connection is not ready,
    /// even though `is_active` keeps the online UI visible.
    pub fn is_ready(&self) -> bool {
        matches!(self.connection, Some(Connection::Ready(_)))
    }

    /// Takes the `PEER_LEFT` signal emitted by the server.
    pub fn take_peer_left(&mut self) -> bool {
        std::mem::take(&mut self.peer_left)
    }

    /// Takes the next remote id that left the world.
    pub fn take_left_peer(&mut self) -> Option<u64> {
        self.left_peers.pop_front()
    }

    /// Clears stale leave signals when a new trade session starts.
    pub fn clear_peer_left(&mut self) {
        self.peer_left = false;
        self.left_peers.clear();
    }

    pub fn send_peer_message(&mut self, kind: PeerMessageKind, payload: &[u8]) -> bool {
        let prefix = match kind {
            PeerMessageKind::Offer => b"OFFER".as_slice(),
            PeerMessageKind::Gift => b"GIFT".as_slice(),
            PeerMessageKind::Ok => b"OK".as_slice(),
            PeerMessageKind::No => b"NO".as_slice(),
        };
        let mut message = Vec::with_capacity(prefix.len() + 1 + payload.len());
        message.extend_from_slice(prefix);
        message.push(0x1f);
        message.extend_from_slice(payload);
        eprintln!("[trade] peer enviado kind={kind:?} bytes={}", message.len());
        let result = match &self.connection {
            Some(Connection::Ready(Backend::Zibelan(client))) => client.send_message_bytes(&message),
            _ => return false,
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                eprintln!("[online] peer message fallo: {error}");
                self.status = error;
                false
            },
        }
    }

    pub fn end_session(&mut self) -> bool {
        let sent = match &self.connection {
            Some(Connection::Ready(Backend::Zibelan(client))) => client.end_world().is_ok(),
            _ => false,
        };
        self.end_battle();
        self.requests.clear();
        self.responses.clear();
        self.peer_messages.clear();
        self.pending_request = None;
        self.side = 0;
        self.peer.clear();
        sent
    }

    pub fn remote_overlays(&mut self, room_id: i32, assets: &Assets) -> Vec<RemoteOverlay> {
        let now = Instant::now();
        let remotes: Vec<Remote> = self
            .remotes
            .values()
            .filter(|remote| remote.visible && remote.room == room_id)
            .cloned()
            .collect();
        let mut overlays = Vec::new();
        for remote in remotes {
            let mut top = remote.render_y - 28.0;
            let mut any_visible = false;
            for part in &remote.parts {
                if let Some(part_top) = self.part_top(assets, &remote, part) {
                    any_visible = true;
                    if part_top < top {
                        top = part_top;
                    }
                }
            }
            if !any_visible {
                continue
            }
            let say = remote.say.as_ref().and_then(|(text, at)| {
                (now.saturating_duration_since(*at).as_millis() < say_time_ms(text)).then(|| text.clone())
            });
            let exclaim = remote.exclaim.and_then(|kind| {
                (now.saturating_duration_since(remote.exclaim_at).as_millis() < EXCLAIM_MS).then_some(kind)
            });
            overlays.push(RemoteOverlay {
                name: remote.name.clone(),
                x: remote.render_x,
                y: remote.render_y,
                top,
                battle: remote.battle,
                say,
                exclaim,
            });
        }
        overlays
    }

    fn part_top(&mut self, assets: &Assets, remote: &Remote, part: &RemotePart) -> Option<f64> {
        let sprite = match part.kind {
            RemotePartKind::Player { sprite } => assets.sprites.get_asset(sprite)?,
            RemotePartKind::Pokemon { pkmn, mira } => match self.pokemon_sprite_plan(pkmn, mira) {
                PokemonSpritePlan::File(path) | PokemonSpritePlan::Mirrored(path) => {
                    let key = path.to_string_lossy().into_owned();
                    self.pokemon_sprites.get(&key)?.as_deref()?
                },
                PokemonSpritePlan::Placeholder { sprite, .. } => assets.sprites.get_asset(sprite)?,
                PokemonSpritePlan::Missing => return None,
            },
        };
        let y = remote.render_y + part.dy;
        Some(y + (f64::from(sprite.bbox_top) - f64::from(sprite.origin_y)) * part.ys)
    }

    pub fn tick(&mut self, room: &RoomState, assets: &Assets) {
        self.poll_connection();
        self.poll_events();
        if matches!(self.connection, Some(Connection::Ready(Backend::Json(_)))) {
            self.tick_json(room, assets);
        } else if matches!(self.connection, Some(Connection::Ready(Backend::Zibelan(_)))) {
            self.tick_zibelan(room, assets);
        }
    }

    fn tick_json(&mut self, room: &RoomState, assets: &Assets) {
        let Some(Connection::Ready(Backend::Json(client))) = &self.connection else { return };
        if self.last_send.elapsed() < Duration::from_millis(50) {
            return
        }
        self.last_send = Instant::now();
        let Some(state) = local_state(room, assets) else { return };
        if let Err(error) = client.send_state(
            state.0, state.1, state.2, state.3, state.4, state.5, state.6, state.7, state.8, state.9, state.10,
            state.11, state.12,
        ) {
            self.status = error;
        }
    }

    fn tick_zibelan(&mut self, room: &RoomState, assets: &Assets) {
        if self.connection_mode != Some(OnlineMode::ZibelanWorld) {
            return
        }
        let Some(state) = local_state(room, assets) else { return };
        let room_changed = self.last_world_room != Some(state.0);
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            state.0,
            state.1.round() as i64,
            state.2.round() as i64,
            state.3,
            state.4,
            state.5.to_bits(),
            state.6.to_bits(),
            state.8
        );
        let changed = self.last_world_key.as_deref() != Some(key.as_str());
        if !room_changed && !changed && self.last_send.elapsed() < Duration::from_secs(1) {
            return
        }
        self.last_send = Instant::now();
        self.last_world_room = Some(state.0);
        self.last_world_key = Some(key);
        let parts = if state.8 {
            format!("S,{},{},0,0,{},{}", state.3, state.4, state.5, state.6)
        } else {
            "-".into()
        };
        let Some(Connection::Ready(Backend::Zibelan(client))) = &self.connection else { return };
        if room_changed {
            let _ = client.world_sync();
        }
        if let Err(error) = client.world_position(
            state.0,
            state.1.round() as i64,
            state.2.round() as i64,
            &parts,
        ) {
            self.status = error;
        }
    }

    pub fn begin_draw(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_draw).as_secs_f64().clamp(0.0, 0.2);
        self.last_draw = now;
        let k = (dt * 18.0).min(1.0);
        for remote in self.remotes.values_mut() {
            remote.render_x += (remote.x - remote.render_x) * k;
            remote.render_y += (remote.y - remote.render_y) * k;
            remote.drawn = false;
            remote.drawn_parts.clear();
            remote.drawn_parts.resize(remote.parts.len(), false);
        }
    }

    pub fn draw_before_depth(
        &mut self,
        renderer: &mut Renderer,
        assets: &Assets,
        room_id: i32,
        depth: f64,
    ) {
        let candidates = self.part_candidates(room_id, Some(depth));
        self.draw_part_candidates(renderer, assets, candidates);
    }

    pub fn draw_remaining(&mut self, renderer: &mut Renderer, assets: &Assets, room_id: i32) {
        let candidates = self.part_candidates(room_id, None);
        self.draw_part_candidates(renderer, assets, candidates);
    }

    fn part_candidates(&self, room_id: i32, min_depth: Option<f64>) -> Vec<(u64, usize, f64)> {
        let mut candidates = Vec::new();
        for (id, remote) in &self.remotes {
            if !remote.visible || remote.room != room_id {
                continue
            }
            for (index, part) in remote.parts.iter().enumerate() {
                if remote.drawn_parts.get(index).copied().unwrap_or(false) {
                    continue
                }
                let depth = part_depth(remote, part);
                if min_depth.map(|min| depth >= min).unwrap_or(true) {
                    candidates.push((*id, index, depth));
                }
            }
        }
        candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        candidates
    }

    fn draw_part_candidates(
        &mut self,
        renderer: &mut Renderer,
        assets: &Assets,
        candidates: Vec<(u64, usize, f64)>,
    ) {
        for (id, index, depth) in candidates {
            let snapshot = {
                let Some(remote) = self.remotes.get_mut(&id) else { continue };
                let Some(drawn) = remote.drawn_parts.get_mut(index) else { continue };
                if std::mem::replace(drawn, true) {
                    continue
                }
                let Some(part) = remote.parts.get(index).cloned() else { continue };
                let shadow = remote
                    .parts
                    .iter()
                    .position(|part| matches!(part.kind, RemotePartKind::Player { .. }))
                    .map(|primary| primary == index)
                    .unwrap_or(false);
                (
                    remote.render_x,
                    remote.render_y,
                    remote.angle,
                    remote.blend,
                    remote.alpha,
                    part,
                    shadow,
                )
            };
            self.draw_remote_part(
                renderer, assets, snapshot.0, snapshot.1, snapshot.2, snapshot.3, snapshot.4, &snapshot.5, snapshot.6,
                depth,
            );
        }
    }

    fn draw_remote_part(
        &mut self,
        renderer: &mut Renderer,
        assets: &Assets,
        owner_x: f64,
        owner_y: f64,
        angle: f64,
        blend: i32,
        alpha: f64,
        part: &RemotePart,
        shadow: bool,
        depth: f64,
    ) {
        let x = owner_x + part.dx;
        let y = owner_y + part.dy;
        renderer.set_draw_offset(0.0, 0.0);
        renderer.set_depth(depth as f32);
        match part.kind {
            RemotePartKind::Player { sprite } => {
                let Some(sprite) = assets.sprites.get_asset(sprite) else { return };
                let Some(atlas) = sprite.get_atlas_ref(part.frame) else { return };
                let left = x + (f64::from(sprite.bbox_left) - f64::from(sprite.origin_x)) * part.xs;
                let right = x + (f64::from(sprite.bbox_right) - f64::from(sprite.origin_x)) * part.xs;
                let bottom = y + (f64::from(sprite.bbox_bottom) - f64::from(sprite.origin_y)) * part.ys;
                let (left, right) = if left <= right { (left, right) } else { (right, left) };
                if shadow {
                    let radius_x = ((right - left) * 0.5).max(0.5);
                    let radius_y = (5.0 * part.ys.abs()).max(0.5);
                    renderer.draw_ellipse(
                        (left + right) * 0.5,
                        bottom,
                        radius_x,
                        radius_y,
                        0,
                        0,
                        (alpha * 0.3).clamp(0.0, 1.0),
                        false,
                    );
                }
                renderer.draw_sprite(atlas, x, y, part.xs, part.ys, angle, blend, alpha);
            },
            RemotePartKind::Pokemon { pkmn, mira } => {
                let plan = self.pokemon_sprite_plan(pkmn, mira);
                match plan {
                    PokemonSpritePlan::File(path) => {
                        let Some(key) = self.loaded_pokemon_sprite(renderer, &path) else { return };
                        let Some(sprite) = self.pokemon_sprites.get(&key).and_then(|sprite| sprite.as_deref()) else {
                            return
                        };
                        let Some(atlas) = sprite.get_atlas_ref(part.frame) else { return };
                        renderer.draw_sprite(atlas, x, y, part.xs, part.ys, 0.0, 0xffffff, 1.0);
                    },
                    PokemonSpritePlan::Mirrored(path) => {
                        let Some(key) = self.loaded_pokemon_sprite(renderer, &path) else { return };
                        let Some(sprite) = self.pokemon_sprites.get(&key).and_then(|sprite| sprite.as_deref()) else {
                            return
                        };
                        let Some(atlas) = sprite.get_atlas_ref(part.frame) else { return };
                        renderer.draw_sprite(atlas, x, y, -part.xs, part.ys, 0.0, 0xffffff, 1.0);
                    },
                    PokemonSpritePlan::Placeholder { sprite, .. } => {
                        let Some(sprite) = assets.sprites.get_asset(sprite) else { return };
                        let Some(atlas) = sprite.get_atlas_ref(part.frame) else { return };
                        renderer.draw_sprite(atlas, x, y, part.xs, part.ys, 0.0, 0xffffff, 1.0);
                    },
                    PokemonSpritePlan::Missing => (),
                }
            },
        }
    }

    pub fn fix_collisions(&mut self, room: &RoomState, assets: &Assets) {
        if matches!(self.connection, Some(Connection::Ready(_))) {
            self.resolve_remote_collisions(room, assets);
        }
    }

    fn resolve_remote_collisions(&mut self, room: &RoomState, assets: &Assets) {
        let Some(object_index) = assets
            .objects
            .iter()
            .enumerate()
            .find(|(_, object)| {
                object
                    .as_ref()
                    .map(|object| f3r::object::is_player_name(object.name.as_ref()))
                    .unwrap_or(false)
            })
            .map(|(index, _)| index as ID)
        else {
            return
        };
        let Some(handle) = room.instance_list.iter_by_identity(object_index).next(&room.instance_list) else {
            return
        };
        let local = room.instance_list.get(handle);
        if !local.visible.get() {
            return
        }
        let mask_index = local.mask_index.get();
        let mask_sprite_index = if mask_index >= 0 { mask_index } else { local.sprite_index.get() };
        let Some(mask) = assets.sprites.get_asset(mask_sprite_index) else { return };
        local.bbox_is_stale.set(true);
        local.update_bbox(Some(mask));
        let remote_boxes: Vec<(i32, i32, i32, i32)> = self
            .remotes
            .values()
            .filter(|remote| remote.visible && remote.room == room.id)
            .filter_map(remote_bbox)
            .collect();
        if remote_boxes.is_empty() {
            return
        }
        let local_box = (local.bbox_left.get(), local.bbox_right.get(), local.bbox_top.get(), local.bbox_bottom.get());
        if self.last_debug.elapsed() >= Duration::from_millis(500) {
            self.last_debug = Instant::now();
            eprintln!(
                "[online-collision] local_moved={} local={:?} remotes={:?} pos=({}, {}) prev=({}, {})",
                local.x.get() != local.xprevious.get() || local.y.get() != local.yprevious.get(),
                local_box,
                remote_boxes,
                local.x.get().into_inner(),
                local.y.get().into_inner(),
                local.xprevious.get().into_inner(),
                local.yprevious.get().into_inner(),
            );
        }
        if local.x.get() == local.xprevious.get() && local.y.get() == local.yprevious.get() {
            let local_center = (
                (local_box.0 + local_box.1) as f64 * 0.5,
                (local_box.2 + local_box.3) as f64 * 0.5,
            );
            let exactly_overlapping = remote_boxes.iter().any(|remote| {
                let remote_center = (
                    (remote.0 + remote.1) as f64 * 0.5,
                    (remote.2 + remote.3) as f64 * 0.5,
                );
                (local_center.0 - remote_center.0).abs() <= 1.0
                    && (local_center.1 - remote_center.1).abs() <= 1.0
            });
            if !exactly_overlapping {
                return
            }
        }
        if !remote_boxes.iter().any(|remote| overlaps(local_box, *remote)) {
            return
        }
        let previous_x = local.xprevious.get();
        let previous_y = local.yprevious.get();
        local.x.set(previous_x);
        local.y.set(previous_y);
        local.bbox_is_stale.set(true);
        local.update_bbox(Some(mask));
        let local_box = (local.bbox_left.get(), local.bbox_right.get(), local.bbox_top.get(), local.bbox_bottom.get());
        if !remote_boxes.iter().any(|remote| overlaps(local_box, *remote)) {
            return
        }
        let local_center = (
            (local_box.0 + local_box.1) as f64 * 0.5,
            (local_box.2 + local_box.3) as f64 * 0.5,
        );
        let exactly_overlapping = remote_boxes.iter().any(|remote| {
            let remote_center = (
                (remote.0 + remote.1) as f64 * 0.5,
                (remote.2 + remote.3) as f64 * 0.5,
            );
            (local_center.0 - remote_center.0).abs() <= 1.0
                && (local_center.1 - remote_center.1).abs() <= 1.0
        });
        if !exactly_overlapping {
            return
        }
        for _ in 0..3 {
            let local_box = (local.bbox_left.get(), local.bbox_right.get(), local.bbox_top.get(), local.bbox_bottom.get());
            let Some(remote_box) = remote_boxes.iter().copied().find(|remote| overlaps(local_box, *remote)) else {
                break
            };
            let push_left = local_box.1 - remote_box.0 + 1;
            let push_right = remote_box.1 - local_box.0 + 1;
            let push_up = local_box.3 - remote_box.2 + 1;
            let push_down = remote_box.3 - local_box.2 + 1;
            let (dx, dy) = if push_left <= push_right && push_left <= push_up && push_left <= push_down {
                (-push_left, 0)
            } else if push_right <= push_up && push_right <= push_down {
                (push_right, 0)
            } else if push_up <= push_down {
                (0, -push_up)
            } else {
                (0, push_down)
            };
            local.x.set(Real::from(local.x.get().into_inner() + f64::from(dx)));
            local.y.set(Real::from(local.y.get().into_inner() + f64::from(dy)));
            local.bbox_is_stale.set(true);
            local.update_bbox(Some(mask));
        }
    }

    fn fail_connection(&mut self, failure: ConnectionFailure) {
        self.fail_connection_with_reason(failure.message().to_string());
    }

    fn fail_connection_with_reason(&mut self, reason: String) {
        eprintln!("[reloaded-online] conexión terminada: {reason}");
        self.apply_local_id(None);
        self.remotes.clear();
        self.world.clear();
        self.world_list.clear();
        self.peer.clear();
        self.side = 0;
        self.pending_request = None;
        self.requests.clear();
        self.responses.clear();
        self.peer_messages.clear();
        self.last_world_room = None;
        self.last_world_key = None;
        self.connect_deadline = None;
        self.handshake_deadline = None;
        self.status = reason.clone();
        self.fail_battle(reason.clone());
        self.connection = Some(Connection::Failed(reason));
    }

    fn check_handshake_watchdog(&mut self) {
        if !matches!(self.connection, Some(Connection::Ready(Backend::Zibelan(_)))) {
            return
        }
        if self.handshake_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            self.fail_connection(ConnectionFailure::Timeout);
        }
    }

    fn localize_connect_error(reason: String) -> String {
        match ConnectionFailure::from_reason(&reason) {
            ConnectionFailure::Other => reason,
            failure => failure.message().to_string(),
        }
    }

    fn poll_connection(&mut self) {
        if !matches!(self.connection, Some(Connection::Waiting(_))) {
            return
        }
        let result = match &self.connection {
            Some(Connection::Waiting(rx)) => rx.try_recv(),
            _ => return,
        };
        match result {
            Ok(Ok(backend)) => {
                let status = match &backend {
                    Backend::Json(_) => "Reloaded Online conectado".to_string(),
                    Backend::Zibelan(_) if self.connection_mode == Some(OnlineMode::ZibelanLobby) => {
                        "Zibelan lobby conectado; esperando rival".to_string()
                    },
                    Backend::Zibelan(_) => "Zibelan conectado; esperando WOK".to_string(),
                };
                let zibelan = matches!(backend, Backend::Zibelan(_));
                self.connection = Some(Connection::Ready(backend));
                self.status = status;
                self.connect_deadline = None;
                self.handshake_deadline =
                    zibelan.then(|| Instant::now() + HANDSHAKE_WATCHDOG);
            },
            Ok(Err(error)) => {
                let zibelan = matches!(
                    self.connection_mode,
                    Some(OnlineMode::ZibelanWorld) | Some(OnlineMode::ZibelanLobby)
                );
                let reason = if zibelan { Self::localize_connect_error(error) } else { error };
                self.fail_connection_with_reason(reason);
                return
            },
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                self.fail_connection(ConnectionFailure::Other);
                return
            },
        }
        if matches!(self.connection, Some(Connection::Waiting(_)))
            && self.connect_deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.fail_connection(ConnectionFailure::Timeout);
        }
    }

    fn poll_events(&mut self) {
        let mut json_events = Vec::new();
        let mut roz_events = Vec::new();
        if let Some(Connection::Ready(backend)) = &self.connection {
            match backend {
                Backend::Json(client) => {
                    while let Some(event) = client.try_event() {
                        json_events.push(event);
                    }
                },
                Backend::Zibelan(client) => {
                    while let Some(event) = client.try_recv() {
                        roz_events.push(event);
                    }
                },
            }
        }
        for event in json_events {
            self.handle_json_event(event);
        }
        for event in roz_events {
            self.handle_roz_event(event);
        }
        self.check_handshake_watchdog();
    }

    fn handle_json_event(&mut self, event: Event) {
        match event {
            Event::Connected { id } => {
                self.local_id = Some(id);
                self.world.set_local_id(Some(id));
                self.announce_welcome("Reloaded Online", id, None, None);
                self.queue_connection_notice();
            },
            Event::Joined { id, name } => {
                let name = (!name.is_empty()).then_some(name.as_str());
                self.announce_welcome("Reloaded Online", id, None, name);
            },
            Event::State { id, name, room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha } => {
                self.world.set_local_id(self.local_id);
                let (applied, evicted) =
                    self.world.apply_remote_state(id, room, x, y, name.as_bytes());
                if let Some(evicted) = evicted {
                    self.drop_remote_state(evicted);
                }
                if !applied {
                    return
                }
                let parts = if visible {
                    vec![RemotePart {
                        kind: RemotePartKind::Player { sprite },
                        frame,
                        dx: 0.0,
                        dy: 0.0,
                        xs,
                        ys,
                    }]
                } else {
                    Vec::new()
                };
                self.upsert_remote(
                    id, name, room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha, parts, false,
                );
            },
            Event::Request { id, name, kind, info } => {
                self.requests.push(IncomingRequest { id, name, kind, info });
            },
            Event::Response { id, name, accept } => {
                self.responses.push(IncomingResponse { id, name, accept });
            },
            Event::BattleTeam { id, data, .. } => {
                self.battle_teams.push(IncomingBattleTeam {
                    id,
                    meta: BattleTeamMeta::default(),
                    data: data.into_bytes(),
                    zibelan: false,
                });
            },
            Event::BattleInput { id, mask, seq, frame, checksum } => {
                self.battle_inputs.push(IncomingBattleInput { id, mask, seq, frame, checksum });
            },
            Event::Chat { name, text, .. } => {
                eprintln!("[reloaded-online] {name}: {text}");
            },
            Event::Left { id } => {
                if self.battle_active && self.battle_peer == Some(id) {
                    self.fail_battle("el rival salio");
                }
                if self.left_peers.len() >= 64 {
                    self.left_peers.pop_front();
                }
                self.left_peers.push_back(id);
                self.world.apply_wdel(id);
                self.drop_remote_state(id);
            },
            Event::Error(error) => {
                eprintln!("[reloaded-online] {error}");
                self.status = error;
            },
            Event::Disconnected(error) => {
                self.status = error.clone();
                self.remotes.clear();
                self.world.clear();
                self.world.set_local_id(None);
                self.connection = Some(Connection::Failed(error.clone()));
                self.connection_notice_pending = false;
                self.connection_notice_seen = false;
                self.peer_left = true;
                self.fail_battle(error);
            },
        }
    }

    fn handle_roz_event(&mut self, event: RozEvent) {
        match event {
            RozEvent::Welcome { version, id } => {
                self.apply_local_id(Some(id));
                self.announce_welcome("Zibelan", id, Some(version), None);
                self.handshake_deadline = Some(Instant::now() + HANDSHAKE_WATCHDOG);
            },
            RozEvent::Wait => {
                self.handshake_deadline = None;
                self.status = "Zibelan: esperando rival".into();
            },
            RozEvent::Start { side, peer } => {
                self.handshake_deadline = None;
                eprintln!("[online] START side={side} peer={peer}");
                self.side = side;
                self.peer = peer.clone();
                if let Some((target, kind)) = self.pending_request.take() {
                    // El servidor moderno no manda WANS: START es la aceptación.
                    if kind == "F" {
                        self.battle_peer = Some(target);
                    }
                    self.responses.push(IncomingResponse {
                        id: target,
                        name: peer.clone(),
                        accept: true,
                    });
                    eprintln!("[online] START acepta solicitud target={target} kind={kind}");
                }
                self.status = format!("Zibelan: combate contra {peer} (lado {side})");
            },
            RozEvent::WorldJoined { side, id } => {
                eprintln!("[online] WOK side={side}");
                self.handshake_deadline = None;
                if let Some(id) = id {
                    let changed = self.local_id != Some(id);
                    self.apply_local_id(Some(id));
                    if changed {
                        self.announce_welcome("Zibelan", id, None, None);
                    }
                }
                if self.connection_mode != Some(OnlineMode::ZibelanLobby) {
                    self.queue_connection_notice();
                }
                self.side = side;
                self.status = match &self.welcome_notice {
                    Some(notice) => format!("{notice} | mundo conectado (lado {side})"),
                    None => format!("Zibelan: mundo conectado (lado {side})"),
                };
            },
            RozEvent::Full { world } => {
                self.fail_connection_with_reason(ConnectionFailure::full_message(world).to_string());
            },
            RozEvent::PeerLeft => {
                self.peer.clear();
                self.pending_request = None;
                self.status = ConnectionFailure::PeerLeft.message().to_string();
                self.peer_left = true;
                self.fail_battle(ConnectionFailure::PeerLeft.message());
            },
            RozEvent::ConnectionRejected { failure } => {
                self.fail_connection(failure);
            },
            RozEvent::Message(bytes) => {
                self.handle_roz_message(&bytes);
            },
            RozEvent::Input { epoch, frame, mask } => {
                let id = self.battle_peer.unwrap_or(0);
                let checksum = self.pending_checksums.remove(&(epoch, frame)).unwrap_or(0);
                self.battle_inputs.push(IncomingBattleInput {
                    id,
                    mask: mask as u32,
                    seq: epoch as u32,
                    frame: frame as u32,
                    checksum,
                });
            },
            RozEvent::Checksum { epoch, frame, checksum } => {
                let id = self.battle_peer.unwrap_or(0);
                if let Some(input) = self
                    .battle_inputs
                    .iter_mut()
                    .rev()
                    .find(|input| input.id == id && input.frame == frame as u32)
                {
                    input.checksum = checksum as u32;
                } else {
                    self.pending_checksums.insert((epoch, frame), checksum as u32);
                    if self.pending_checksums.len() > 64 {
                        self.pending_checksums.clear();
                    }
                }
            },
            RozEvent::WorldPosition(position) => {
                let message = world::WpMessage {
                    id: position.id,
                    room: position.room,
                    x: position.x,
                    y: position.y,
                    parts: position.decoded,
                    parts_token: position.parts_token,
                    name: position.name_bytes,
                    raw_remainder: position.raw,
                };
                self.apply_wp_message(&message);
            },
            RozEvent::WorldSelfPosition(_) => {
                // `WPOS` is client-to-server; a server echo carries no remote state.
            },
            RozEvent::WorldDelete { id, raw } => {
                self.apply_wdel_message(&world::WdelMessage { id, raw_remainder: raw });
            },
            RozEvent::WorldRequest { id, kind, payload } => {
                self.requests.push(IncomingRequest {
                    id,
                    name: self.remote_name(id),
                    kind,
                    info: payload,
                });
            },
            RozEvent::WorldAnswer { id, answer } => {
                eprintln!("[online] WANS top id={id} answer={answer}");
                self.responses.push(IncomingResponse {
                    id,
                    name: self.remote_name(id),
                    accept: answer == 1,
                });
            },
            RozEvent::WorldCancel { id } => {
                if let Some(id) = id {
                    self.requests.retain(|request| request.id != id);
                    self.responses.retain(|response| response.id != id);
                } else {
                    self.requests.clear();
                    self.responses.clear();
                }
            },
            RozEvent::WorldSay { id, text } => {
                let name = self.remote_name(id);
                self.chat.push((name.clone(), text.clone()));
                self.status = format!("{name}: {text}");
                self.set_remote_say(id, text);
            },
            RozEvent::WorldExclaim { id, kind } => {
                self.set_remote_exclaim(id, kind);
            },
            RozEvent::WorldList { entries, .. } => {
                self.world_list = entries
                    .into_iter()
                    .map(|entry| {
                        world::wlist_display(&world::ListEntry {
                            name: entry.name,
                            place: entry.place,
                            flags: entry.flags,
                        })
                    })
                    .collect();
            },
            RozEvent::Malformed { line, reason } => {
                eprintln!("[reloaded-online] linea Zibelan invalida ({reason}): {line}");
            },
            RozEvent::Disconnected { reason } => {
                self.status = reason.clone();
                self.apply_local_id(None);
                self.world.clear();
                self.remotes.clear();
                self.connection_notice_pending = false;
                self.connection_notice_seen = false;
                self.peer_left = true;
                self.fail_connection_with_reason(reason);
            },
        }
    }

    fn handle_roz_message(&mut self, bytes: &[u8]) {
        // Keep the `.TEAM` bytes intact: they are ANSI text and may contain non-UTF-8 names.
        if let Some(rest) = bytes.strip_prefix(b"TEAM\x1f") {
            match parse_zibelan_team(rest) {
                Some((meta, data)) => {
                    self.battle_teams.push(IncomingBattleTeam {
                        id: self.battle_peer.unwrap_or(0),
                        meta,
                        data,
                        zibelan: true,
                    });
                },
                None => eprintln!("[reloaded-online] TEAM Zibelan invalido"),
            }
            return;
        }
        if let Some(rest) = bytes.strip_prefix(b"WCFG\x1f") {
            match BattleConfig::decode(rest) {
                Ok(config) => {
                    if self.battle_configs.len() >= 4 {
                        self.battle_configs.pop_front();
                    }
                    self.battle_configs.push_back(config);
                },
                Err(error) => {
                    eprintln!("[reloaded-online] WCFG Zibelan invalido: {error}");
                    self.battle_config_error = Some(error);
                },
            }
            return;
        }
        if let Some(separator) = bytes.iter().position(|byte| *byte == 0x1f) {
            let kind = match &bytes[..separator] {
                b"OFFER" => Some(PeerMessageKind::Offer),
                b"GIFT" => Some(PeerMessageKind::Gift),
                b"OK" => Some(PeerMessageKind::Ok),
                b"NO" => Some(PeerMessageKind::No),
                _ => None,
            };
            if let Some(kind) = kind {
                eprintln!("[trade] peer recibido kind={kind:?} bytes={}", bytes.len() - separator - 1);
                if self.peer_messages.len() >= 64 {
                    self.peer_messages.pop_front();
                }
                self.peer_messages.push_back(PeerMessage {
                    kind,
                    payload: bytes[separator + 1..].to_vec(),
                });
                return;
            }
        }
        let text = String::from_utf8_lossy(bytes);
        let (kind, payload) = match text.split_once('\x1f') {
            Some((kind, payload)) => (kind, payload),
            None => {
                let kind = text.split_whitespace().next().unwrap_or("");
                let payload = text.strip_prefix(kind).unwrap_or("").trim_start();
                (kind, payload)
            },
        };
        match kind {
            "TEAM" => {
                self.battle_teams.push(IncomingBattleTeam {
                    id: self.battle_peer.unwrap_or(0),
                    meta: BattleTeamMeta::default(),
                    data: payload.as_bytes().to_vec(),
                    zibelan: false,
                });
            },
            "WREQ" => {
                let (id_text, remainder) = payload.split_once(' ').unwrap_or((payload, ""));
                let (kind, rest) = remainder.split_once(' ').unwrap_or((remainder, ""));
                if let Ok(id) = id_text.parse::<u64>() {
                    let (name, info) = rest.split_once('\x1e').unwrap_or((rest, ""));
                    eprintln!("[online] WREQ recibido id={id} kind={kind} rest={rest}");
                    self.requests.push(IncomingRequest {
                        id,
                        name: if name.is_empty() { self.remote_name(id) } else { name.to_string() },
                        kind: kind.to_string(),
                        info: info.to_string(),
                    });
                }
            },
            "WANS" => {
                let (id_text, answer_text) = payload.split_once(' ').unwrap_or((payload, ""));
                if let (Ok(id), Ok(answer)) = (id_text.parse::<u64>(), answer_text.trim().parse::<i32>()) {
                    eprintln!("[online] WANS recibido id={id} answer={answer}");
                    self.responses.push(IncomingResponse { id, name: self.remote_name(id), accept: answer == 1 });
                }
            },
            "WCANCEL" => {
                if let Ok(id) = payload.trim().parse::<u64>() {
                    self.requests.retain(|request| request.id != id);
                    self.responses.retain(|response| response.id != id);
                } else {
                    self.requests.clear();
                    self.responses.clear();
                }
            },
            "WSAY" => {
                let (id_text, message) = payload.split_once(' ').unwrap_or((payload, ""));
                if let Ok(id) = id_text.parse::<u64>() {
                    let name = self.remote_name(id);
                    self.chat.push((name.clone(), message.to_string()));
                    self.status = format!("{name}: {message}");
                    self.set_remote_say(id, message.to_string());
                }
            },
            "WEXC" => {
                let (id_text, emote) = payload.split_once(' ').unwrap_or((payload, ""));
                if let (Ok(id), Ok(kind)) = (id_text.parse::<u64>(), emote.trim().parse::<i32>()) {
                    self.set_remote_exclaim(id, kind);
                }
            },
            "WLIST" => {
                let mut line = b"WLIST ".to_vec();
                line.extend_from_slice(payload.as_bytes());
                self.world_list = world::parse_wlist(&line)
                    .map(|list| list.entries.iter().map(world::wlist_display).collect())
                    .unwrap_or_default();
            },
            _ => {
                eprintln!("[reloaded-online] rival: {text}");
                self.status = format!("Mensaje: {text}");
            },
        }
    }

    fn queue_connection_notice(&mut self) {
        if self.connection_notice_seen {
            return
        }
        self.connection_notice_seen = true;
        self.connection_notice_pending = true;
    }

    /// Takes the one-shot notice for the first successful world connection.
    ///
    /// `WELCOME` only greets the client and `WOK` joins the world; queueing here from the
    /// latter (or from the JSON `Connected` event) keeps the native notice out of the chat
    /// stream and stops a repeated event from opening it twice on the same connection.
    pub fn take_connection_notice(&mut self) -> bool {
        std::mem::take(&mut self.connection_notice_pending)
    }

    fn announce_welcome(&mut self, server: &str, id: u64, version: Option<u32>, name: Option<&str>) {
        let notice = match version {
            Some(version) => format!("Bienvenido a {server}: id #{id} (protocolo {version})"),
            None => format!("Bienvenido a {server}: id #{id}"),
        };
        self.status = match name {
            Some(name) if !name.is_empty() => format!("{notice} - {name}"),
            _ => notice.clone(),
        };
        if self.welcome_notice.as_deref() == Some(notice.as_str()) {
            return
        }
        if let Some(previous) = self.welcome_notice.take() {
            self.chat.retain(|(sender, text)| sender != SERVER_SENDER || text != &previous);
        }
        self.welcome_notice = Some(notice.clone());
        self.chat.push((SERVER_SENDER.to_string(), notice));
    }

    pub fn take_chat(&mut self) -> Option<(String, String)> {
        if self.chat.is_empty() { None } else { Some(self.chat.remove(0)) }
    }

    pub fn world_list(&self) -> &[String] {
        &self.world_list
    }

    pub fn request_world_list(&mut self) -> bool {
        match &self.connection {
            Some(Connection::Ready(Backend::Zibelan(client))) => client.world_list().is_ok(),
            _ => false,
        }
    }

    pub fn send_chat(&mut self, text: &str) -> bool {
        let sent = match &self.connection {
            Some(Connection::Ready(Backend::Zibelan(client))) => client.say(text).is_ok(),
            Some(Connection::Ready(Backend::Json(client))) => client.send_chat(text).is_ok(),
            _ => false,
        };
        if sent {
            self.local_say = Some((text.to_string(), Instant::now()));
        }
        sent
    }

    fn set_remote_say(&mut self, id: u64, text: String) {
        if let Some(remote) = self.remotes.get_mut(&id) {
            remote.say = Some((text, Instant::now()));
        } else {
            self.pending_say.insert(id, (text, Instant::now()));
            if self.pending_say.len() > 64 {
                self.pending_say.clear();
            }
        }
    }

    fn set_remote_exclaim(&mut self, id: u64, kind: i32) {
        if let Some(remote) = self.remotes.get_mut(&id) {
            remote.exclaim = Some(kind);
            remote.exclaim_at = Instant::now();
        } else {
            self.pending_exclaim.insert(id, (kind, Instant::now()));
            if self.pending_exclaim.len() > 64 {
                self.pending_exclaim.clear();
            }
        }
    }

    fn drop_remote_state(&mut self, id: u64) {
        self.remotes.remove(&id);
        self.pending_say.remove(&id);
        self.pending_exclaim.remove(&id);
    }

    fn apply_local_id(&mut self, id: Option<u64>) {
        self.local_id = id;
        self.world.set_local_id(id);
        if let Some(id) = id {
            self.world.apply_wdel(id);
            self.drop_remote_state(id);
        }
    }

    fn apply_wp_message(&mut self, message: &world::WpMessage) {
        self.world.set_local_id(self.local_id);
        let (applied, evicted) = self.world.apply_wp_with_eviction(message);
        if let Some(id) = evicted {
            self.drop_remote_state(id);
        }
        if !applied {
            return
        }
        let Some(remote) = self.world.get(message.id).cloned() else { return };
        self.project_world_remote(&remote);
    }

    fn apply_wdel_message(&mut self, message: &world::WdelMessage) {
        self.world.apply_wdel(message.id);
        self.drop_remote_state(message.id);
    }

    fn project_world_remote(&mut self, remote: &world::Remote) {
        let projected = project_remote(remote);
        self.upsert_remote(
            projected.id,
            projected.name,
            projected.room,
            projected.x,
            projected.y,
            projected.sprite,
            projected.frame,
            projected.xs,
            projected.ys,
            0.0,
            projected.visible,
            projected.bbox,
            projected.depth,
            0xffffff,
            1.0,
            projected.parts,
            projected.battle,
        );
    }

    fn pokemon_sprite_plan(&mut self, pkmn: i32, mira: i32) -> PokemonSpritePlan {
        let species = i64::from(pkmn);
        let slot = battle_slot_for_facing(i64::from(mira));
        let key = (species, slot);
        if let Some(plan) = self.pokemon_plans.get(&key) {
            return plan.clone()
        }
        let plan = plan_battle_sprite(&self.program_directory, species, slot, self.battle_context);
        if let PokemonSpritePlan::Placeholder { missing, .. } = &plan {
            if self.pokemon_missing.insert(missing.clone(), ()).is_none() {
                eprintln!("[online] falta sprite de companero: {missing}");
            }
        }
        self.pokemon_plans.insert(key, plan.clone());
        plan
    }

    fn loaded_pokemon_sprite(&mut self, renderer: &mut Renderer, path: &Path) -> Option<String> {
        let key = path.to_string_lossy().into_owned();
        if !self.pokemon_sprites.contains_key(&key) {
            let loaded = load_pokemon_sprite(renderer, path).ok();
            self.pokemon_sprites.insert(key.clone(), loaded);
        }
        Some(key)
    }

    pub fn local_say(&self) -> Option<&str> {
        self.local_say.as_ref().and_then(|(text, at)| {
            (Instant::now().saturating_duration_since(*at).as_millis() < say_time_ms(text)).then_some(text.as_str())
        })
    }

    fn remote_name(&self, id: u64) -> String {
        self.remotes
            .get(&id)
            .map(|remote| remote.name.clone())
            .filter(|name| !name.is_empty())
            .or_else(|| (!self.peer.is_empty()).then(|| self.peer.clone()))
            .unwrap_or_else(|| format!("#{id}"))
    }

    fn upsert_remote(
        &mut self,
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
        parts: Vec<RemotePart>,
        battle: bool,
    ) {
        if Some(id) == self.local_id || !world::valid_position(x, y) {
            return
        }
        let name = cap_remote_name(&name);
        let pending_say = self.pending_say.remove(&id);
        let pending_exclaim = self.pending_exclaim.remove(&id);
        if !self.remotes.contains_key(&id) {
            eprintln!("[online] remote nuevo id={id} name={name} room={room}");
        }
        self.remotes
            .entry(id)
            .and_modify(|remote| {
                remote.name = name.clone();
                remote.room = room;
                remote.x = x;
                remote.y = y;
                remote.sprite = sprite;
                remote.frame = frame;
                remote.xs = xs;
                remote.ys = ys;
                remote.angle = angle;
                remote.visible = visible;
                remote.bbox = bbox;
                remote.depth = depth;
                remote.blend = blend;
                remote.alpha = alpha;
                remote.parts = parts.clone();
                remote.drawn_parts.clear();
                remote.drawn_parts.resize(parts.len(), false);
                remote.battle = battle;
            })
            .or_insert(Remote {
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
                render_x: x,
                render_y: y,
                drawn: false,
                parts,
                drawn_parts: Vec::new(),
                battle,
                say: None,
                exclaim: None,
                exclaim_at: Instant::now(),
            });
        if let Some(remote) = self.remotes.get_mut(&id) {
            if remote.drawn_parts.len() != remote.parts.len() {
                remote.drawn_parts.resize(remote.parts.len(), false);
            }
            if let Some(value) = pending_say {
                remote.say = Some(value);
            }
            if let Some(value) = pending_exclaim {
                remote.exclaim = Some(value.0);
                remote.exclaim_at = value.1;
            }
        }
    }
}

fn cap_remote_name(name: &str) -> String {
    world::sanitize_name_display(name.as_bytes())
}

fn overlaps(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    a.0 <= b.1 && a.1 >= b.0 && a.2 <= b.3 && a.3 >= b.2
}

fn remote_bbox(remote: &Remote) -> Option<(i32, i32, i32, i32)> {
    let dx = (remote.render_x - remote.x).round() as i32;
    let dy = (remote.render_y - remote.y).round() as i32;
    Some((remote.bbox[0] + dx, remote.bbox[1] + dx, remote.bbox[2] + dy, remote.bbox[3] + dy))
}

struct ProjectedRemote {
    id: u64,
    name: String,
    room: i32,
    x: f64,
    y: f64,
    sprite: i32,
    frame: i32,
    xs: f64,
    ys: f64,
    visible: bool,
    bbox: [i32; 4],
    depth: f64,
    parts: Vec<RemotePart>,
    battle: bool,
}

fn project_remote(remote: &world::Remote) -> ProjectedRemote {
    let parts: Vec<RemotePart> = remote.parts.segments.iter().filter_map(project_part).collect();
    let primary = remote.parts.primary_player();
    let (sprite, frame, xs, ys) = primary
        .map(|player| {
            (
                i32::from(player.sprite),
                i32::try_from(player.frame).unwrap_or(0),
                player.xs,
                player.ys,
            )
        })
        .unwrap_or((-1, 0, 1.0, 1.0));
    let x = remote.x;
    let y = remote.y;
    ProjectedRemote {
        id: remote.id,
        name: String::from_utf8_lossy(&remote.name).into_owned(),
        room: remote.room,
        x,
        y,
        sprite,
        frame,
        xs,
        ys,
        visible: remote.is_protocol_visible(),
        bbox: [
            (x - 8.0).round() as i32,
            (x + 8.0).round() as i32,
            (y - 20.0).round() as i32,
            (y + 4.0).round() as i32,
        ],
        depth: -(y / 100.0),
        parts,
        battle: remote.parts.battle,
    }
}

fn project_part(part: &world::Part) -> Option<RemotePart> {
    match part {
        world::Part::Player(player) => Some(RemotePart {
            kind: RemotePartKind::Player { sprite: i32::from(player.sprite) },
            frame: i32::try_from(player.frame).ok()?,
            dx: player.dx,
            dy: player.dy,
            xs: player.xs,
            ys: player.ys,
        }),
        world::Part::Pokemon(pokemon) => Some(RemotePart {
            kind: RemotePartKind::Pokemon {
                pkmn: i32::try_from(pokemon.species).ok()?,
                mira: i32::try_from(pokemon.facing).ok()?,
            },
            frame: i32::try_from(pokemon.frame).ok()?,
            dx: pokemon.dx,
            dy: pokemon.dy,
            xs: pokemon.xs,
            ys: pokemon.ys,
        }),
    }
}

fn battle_slot_for_facing(facing: i64) -> i32 {
    if facing == 0 {
        return f3r::battle::SLOT_FACING_ZERO
    }
    if facing == -1 {
        return f3r::battle::SLOT_FACING_HURT
    }
    let Ok(slot) = i32::try_from(facing) else { return f3r::battle::SLOT_DEFAULT };
    if world::battle_slot_label(slot).is_some() {
        slot
    } else {
        f3r::battle::SLOT_DEFAULT
    }
}

fn plan_battle_sprite(
    program_directory: &Path,
    species: i64,
    slot: i32,
    context: world::BattleContext,
) -> PokemonSpritePlan {
    if species <= 0 {
        return PokemonSpritePlan::Missing
    }
    let data_root = program_directory.join(f3r::save::ROOT);
    let exists = |relative: &str| data_root.join(relative).is_file();
    match world::battle_sprite(species, slot, context, exists) {
        Ok(world::BattleSprite::File(path)) => PokemonSpritePlan::File(data_root.join(path)),
        Ok(world::BattleSprite::Mirrored { path, .. }) => {
            PokemonSpritePlan::Mirrored(data_root.join(path))
        },
        Ok(world::BattleSprite::Placeholder { missing, sprite }) => PokemonSpritePlan::Placeholder {
            sprite: i32::from(sprite),
            missing,
        },
        Err(_) => PokemonSpritePlan::Missing,
    }
}

fn part_depth(remote: &Remote, part: &RemotePart) -> f64 {
    match part.kind {
        RemotePartKind::Player { .. } => -(remote.render_y / 100.0) - 0.01,
        RemotePartKind::Pokemon { .. } => -((remote.render_y + part.dy) / 100.0),
    }
}

fn say_time_ms(text: &str) -> u128 {
    (SAY_BASE_MS + text.len() as u128 * SAY_PER_CHAR_MS).min(SAY_MAX_MS)
}

fn load_pokemon_sprite(renderer: &mut Renderer, path: &Path) -> Result<Box<asset::Sprite>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("failed to open '{}': {error}", path.display()))?;
    let parsed = parse_gmspr(&bytes).map_err(|error| format!("failed to parse '{}': {error}", path.display()))?;
    let origin_x = parsed.origin_x;
    let origin_y = parsed
        .colliders
        .first()
        .map(|collider| collider.bbox_bottom as i32)
        .unwrap_or(parsed.origin_y)
        - 10;
    let mut frames = Vec::with_capacity(parsed.frames.len());
    for frame in &parsed.frames {
        let mut data = frame.data.to_vec();
        util::bgra2rgba(&mut data);
        let atlas_ref = renderer.upload_sprite(
            data.into_boxed_slice(),
            frame.width as i32,
            frame.height as i32,
            origin_x,
            origin_y,
        )?;
        frames.push(asset::sprite::Frame { width: frame.width, height: frame.height, atlas_ref });
    }
    let colliders = parsed
        .colliders
        .iter()
        .map(|collider| asset::sprite::Collider {
            width: collider.width,
            height: collider.height,
            bbox_left: collider.bbox_left,
            bbox_right: collider.bbox_right,
            bbox_top: collider.bbox_top,
            bbox_bottom: collider.bbox_bottom,
            data: collider.data.clone(),
        })
        .collect::<Vec<_>>();
    let (width, height) = parsed.frames.first().map(|frame| (frame.width, frame.height)).unwrap_or((0, 0));
    let bbox = colliders.first();
    Ok(Box::new(asset::Sprite {
        name: "<gmspr>".into(),
        frames,
        width,
        height,
        origin_x,
        origin_y,
        per_frame_colliders: parsed.per_frame_colliders,
        bbox_left: bbox.map(|bbox| bbox.bbox_left).unwrap_or(0),
        bbox_right: bbox.map(|bbox| bbox.bbox_right).unwrap_or(0),
        bbox_top: bbox.map(|bbox| bbox.bbox_top).unwrap_or(0),
        bbox_bottom: bbox.map(|bbox| bbox.bbox_bottom).unwrap_or(0),
        colliders,
    }))
}

/// Returns (room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha).
fn local_state(room: &RoomState, assets: &Assets) -> Option<(i32, f64, f64, i32, i32, f64, f64, f64, bool, [i32; 4], f64, i32, f64)> {
    let object_index = assets
        .objects
        .iter()
        .enumerate()
        .find(|(_, object)| {
            object
                .as_ref()
                .map(|object| f3r::object::is_player_name(object.name.as_ref()))
                .unwrap_or(false)
        })
        .map(|(index, _)| index as ID)?;
    let handle = room.instance_list.iter_by_identity(object_index).next(&room.instance_list)?;
    let instance = room.instance_list.get(handle);
    if !instance.visible.get() {
        return Some((room.id, 0.0, 0.0, -1, 0, 1.0, 1.0, 0.0, false, [0; 4], 0.0, 0xffffff, 1.0))
    }
    let x = instance.x.get().into_inner();
    let y = instance.y.get().into_inner();
    let xs = instance.image_xscale.get().into_inner();
    let ys = instance.image_yscale.get().into_inner();
    let angle = instance.image_angle.get().into_inner();
    let frame = instance.image_index.get().floor().to_i32();
    let sprite_index = instance.sprite_index.get();
    let mask_index = instance.mask_index.get();
    let mask_sprite_index = if mask_index >= 0 { mask_index } else { sprite_index };
    if let Some(mask) = assets.sprites.get_asset(mask_sprite_index) {
        instance.bbox_is_stale.set(true);
        instance.update_bbox(Some(mask));
    }
    let bbox = [instance.bbox_left.get(), instance.bbox_right.get(), instance.bbox_top.get(), instance.bbox_bottom.get()];
    Some((
        room.id,
        x,
        y,
        sprite_index,
        frame,
        xs,
        ys,
        angle,
        true,
        bbox,
        instance.depth.get().into_inner(),
        instance.image_blend.get(),
        instance.image_alpha.get().into_inner(),
    ))
}

fn zibelan_team_payload(data: &[u8], meta: &BattleTeamMeta) -> Vec<u8> {
    let mut payload = Vec::with_capacity(64 + data.len());
    payload.extend_from_slice(b"TEAM\x1f");
    for value in [meta.seed, meta.fondo, meta.modo, meta.sala, meta.delay] {
        payload.extend_from_slice(value.to_string().as_bytes());
        payload.push(0x1e);
    }
    payload.extend_from_slice(data);
    payload
}

fn zibelan_config_payload(config: &BattleConfig) -> Vec<u8> {
    let body = config.encode_body();
    let mut payload = Vec::with_capacity(5 + body.len());
    payload.extend_from_slice(b"WCFG\x1f");
    payload.extend_from_slice(&body);
    payload
}

fn parse_zibelan_team(mut rest: &[u8]) -> Option<(BattleTeamMeta, Vec<u8>)> {
    let mut fields = [0i32; 5];
    for field in &mut fields {
        let end = rest.iter().position(|byte| *byte == 0x1e)?;
        let text = std::str::from_utf8(&rest[..end]).ok()?.trim();
        *field = text.parse::<i32>().unwrap_or(0);
        rest = &rest[end + 1..];
    }
    Some((
        BattleTeamMeta {
            seed: fields[0],
            fondo: fields[1],
            modo: fields[2],
            sala: fields[3],
            delay: fields[4],
        },
        rest.to_vec(),
    ))
}

#[cfg(test)]
mod team_payload_tests {
    use super::*;

    #[test]
    fn team_payload_has_five_prefix_fields_and_bytes() {
        let meta = BattleTeamMeta { seed: 7, fondo: 3, modo: 1, sala: f3r::room::BATTLE_AQUA, delay: 2 };
        let payload = zibelan_team_payload(b"line1\r\nline2\r\n", &meta);
        let expected = format!(
            "TEAM\x1f7\x1e3\x1e1\x1e{}\x1e2\x1eline1\r\nline2\r\n",
            f3r::room::BATTLE_AQUA
        );
        assert_eq!(payload, expected.as_bytes());
    }

    #[test]
    fn parses_team_payload_without_touching_team_bytes() {
        let payload = format!(
            "9\x1e1001\x1e2\x1e{}\x1e4\x1euno\r\ndos\r\n",
            f3r::room::BATTLE_AQUA
        );
        let (meta, data) = parse_zibelan_team(payload.as_bytes()).expect("valid team");
        assert_eq!(meta.seed, 9);
        assert_eq!(meta.fondo, 1001);
        assert_eq!(meta.modo, 2);
        assert_eq!(meta.sala, f3r::room::BATTLE_AQUA);
        assert_eq!(meta.delay, 4);
        assert_eq!(data, b"uno\r\ndos\r\n");
    }
}

#[cfg(test)]
mod battle_config_tests {
    use super::*;

    pub(super) fn sample() -> BattleConfig {
        BattleConfig {
            num_pokemon1: 6,
            num_pokemon2: 3,
            dificultad1: 2,
            dificultad2: 0,
            cambios1: 18,
            cambios2: 66,
            num_obj1: 6,
            num_obj2: 0,
            obj_asg1: 15,
            obj_asg2: 16,
            cont_poke1: 1,
            cont_poke2: 0,
            vel_ataque1: 1,
            vel_ataque2: 0,
            rest_pp1: 4,
            rest_pp2: 2,
            show_type: 1,
            time_battle: 11,
            bttback_index: 1001,
            music_index: 109,
            subita: 1,
            battle_mode: 2,
            day: 0,
            shownumber: 1,
            showbattleevents: 0,
            showpanelcombate: 1,
            shownumber_ps: 0,
            mode_battle_back: 2,
            world_clima: 5,
            clima: 7,
            hora: 20,
            minuto: 30,
            rules_j1: "6#Normal#18#6#Piedra#Sí#No#0".as_bytes().to_vec(),
            rules_j2: "3#Difícil#66#0#Ninguno#No#Sí#4".as_bytes().to_vec(),
            options_bt: "1#0#1#1#0#10:00#1".as_bytes().to_vec(),
            music_name: "Pista 109".as_bytes().to_vec(),
            txt_clima: "Lluvia".as_bytes().to_vec(),
        }
    }

    #[test]
    fn battle_config_round_trips_every_field() {
        let config = sample();
        let body = config.encode_body();
        assert_eq!(body.split(|byte| *byte == BATTLE_CONFIG_SEPARATOR).count(), BATTLE_CONFIG_FIELDS);
        assert_eq!(BattleConfig::decode(&body).expect("valid config"), config);
    }

    #[test]
    fn battle_config_accepts_trailing_and_extra_fields() {
        let config = sample();
        let mut body = config.encode_body();
        body.push(BATTLE_CONFIG_SEPARATOR);
        assert_eq!(BattleConfig::decode(&body).expect("trailing separator"), config);

        body.extend_from_slice(b"99\x1dextra");
        assert_eq!(BattleConfig::decode(&body).expect("extra fields"), config);
    }

    #[test]
    fn battle_config_requires_the_0_6_3_field_count() {
        let body = sample().encode_body();
        let last_separator = body.iter().rposition(|byte| *byte == BATTLE_CONFIG_SEPARATOR).expect("separator");
        let error = BattleConfig::decode(&body[..last_separator]).expect_err("short config");
        assert!(error.contains("WCFG incompleto"));
    }

    #[test]
    fn battle_config_rejects_a_non_numeric_numeric_field() {
        let body = sample().encode_body();
        let text = String::from_utf8(body).expect("ascii config");
        let mut fields: Vec<&str> = text.split('\x1d').collect();
        fields[18] = "no-es-un-fondo";
        let broken = fields.join("\x1d");
        let error = BattleConfig::decode(broken.as_bytes()).expect_err("invalid background");
        assert!(error.contains("campo 18"));
    }

    #[test]
    fn battle_config_preserves_non_utf8_text_fields() {
        let mut config = sample();
        config.rules_j1 = vec![0xff, 0xfe, b'#', 0x80];
        config.txt_clima = vec![0x81, 0x82];
        let body = config.encode_body();
        assert_eq!(BattleConfig::decode(&body).expect("raw text"), config);
    }

    #[test]
    fn battle_config_payload_has_the_expected_prefix() {
        let payload = zibelan_config_payload(&sample());
        assert!(payload.starts_with(b"WCFG\x1f"));
        let rest = payload.strip_prefix(b"WCFG\x1f").expect("prefix");
        assert_eq!(BattleConfig::decode(rest).expect("round trip"), sample());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Shutdown, TcpListener, TcpStream};

    struct MockWorldTransport {
        lines: Option<mpsc::Sender<Vec<u8>>>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl MockWorldTransport {
        fn send(&self, bytes: &[u8]) {
            self.lines
                .as_ref()
                .expect("mock transport is open")
                .send(bytes.to_vec())
                .expect("mock transport line");
        }
    }

    impl Drop for MockWorldTransport {
        fn drop(&mut self) {
            self.lines.take();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn mock_world_transport() -> (ReloadedOnline, MockWorldTransport) {
        use std::io::Write as _;

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock world transport");
        let addr = listener.local_addr().expect("mock world transport address");
        let (lines, lines_rx) = mpsc::channel::<Vec<u8>>();
        let thread = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            while let Ok(bytes) = lines_rx.recv() {
                if stream.write_all(&bytes).and_then(|_| stream.flush()).is_err() {
                    break
                }
            }
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });
        let client = RozClient::connect_with_timeout(&addr.to_string(), Duration::from_secs(2))
            .expect("connect mock world transport");
        let mut online = ReloadedOnline::default();
        online.connection = Some(Connection::Ready(Backend::Zibelan(client)));
        online.connection_mode = Some(OnlineMode::ZibelanWorld);
        (online, MockWorldTransport { lines: Some(lines), thread: Some(thread) })
    }

    struct MockDuplexTransport {
        stream: TcpStream,
        lines: mpsc::Receiver<Vec<u8>>,
        reader: Option<thread::JoinHandle<()>>,
    }

    impl MockDuplexTransport {
        fn send(&mut self, bytes: &[u8]) {
            use std::io::Write as _;
            self.stream
                .write_all(bytes)
                .and_then(|_| self.stream.flush())
                .expect("mock duplex send");
        }

        fn next_line(&self) -> Vec<u8> {
            self.lines
                .recv_timeout(Duration::from_secs(2))
                .expect("mock duplex line from client")
        }
    }

    impl Drop for MockDuplexTransport {
        fn drop(&mut self) {
            let _ = self.stream.shutdown(Shutdown::Both);
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }

    fn mock_duplex_transport() -> (ReloadedOnline, MockDuplexTransport) {
        use std::io::{BufRead, BufReader};

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock duplex transport");
        let addr = listener.local_addr().expect("mock duplex transport address");
        let client = RozClient::connect_with_timeout(&addr.to_string(), Duration::from_secs(2))
            .expect("connect mock duplex transport");
        let (stream, _) = listener.accept().expect("accept mock duplex transport");
        let reader_stream = stream.try_clone().expect("clone mock duplex transport");
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
                            break
                        }
                    },
                }
            }
        });
        let mut online = ReloadedOnline::default();
        online.connection = Some(Connection::Ready(Backend::Zibelan(client)));
        online.connection_mode = Some(OnlineMode::ZibelanWorld);
        (online, MockDuplexTransport { stream, lines, reader: Some(reader) })
    }

    fn next_client_line(transport: &MockDuplexTransport) -> String {
        String::from_utf8(transport.next_line()).expect("client protocol line is utf-8")
    }

    fn next_client_message(transport: &MockDuplexTransport) -> Vec<u8> {
        let line = next_client_line(transport);
        let payload = line.strip_prefix("MSG ").expect("MSG line");
        hex::decode(payload).expect("hex payload")
    }

    fn pump_world(online: &mut ReloadedOnline, mut done: impl FnMut(&ReloadedOnline) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            online.poll_events();
            if done(online) {
                return
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for mock world transport (status: {})",
                online.status()
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn apply_line(online: &mut ReloadedOnline, line: &str) -> world::WpMessage {
        let message = world::parse_wp(line.as_bytes()).expect("valid WP");
        online.apply_wp_message(&message);
        message
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "opengmk-world-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(f3r::save::ROOT).join(f3r::world::BATTLE_SPRITE_DIR)).unwrap();
        root
    }

    fn touch(root: &Path, names: &[&str]) {
        for name in names {
            std::fs::write(
                root.join(f3r::save::ROOT)
                    .join(f3r::world::BATTLE_SPRITE_DIR)
                    .join(format!("{name}.{}", f3r::world::BATTLE_SPRITE_EXT)),
                [],
            )
            .unwrap();
        }
    }

    fn plan_file(plan: PokemonSpritePlan) -> String {
        match plan {
            PokemonSpritePlan::File(path) | PokemonSpritePlan::Mirrored(path) => {
                path.file_name().unwrap().to_string_lossy().into_owned()
            },
            other => panic!("expected file plan, got {other:?}"),
        }
    }

    fn test_assets_with_one_sprite() -> Assets {
        Assets {
            backgrounds: Vec::new(),
            fonts: Vec::new(),
            objects: Vec::new(),
            paths: Vec::new(),
            rooms: Vec::new(),
            scripts: Vec::new(),
            sounds: Vec::new(),
            sprites: vec![Some(Box::new(asset::Sprite {
                name: "test".into(),
                frames: vec![asset::sprite::Frame {
                    width: 1,
                    height: 1,
                    atlas_ref: crate::render::atlas::AtlasRef(0),
                }],
                colliders: Vec::new(),
                width: 1,
                height: 1,
                origin_x: 0,
                origin_y: 0,
                per_frame_colliders: false,
                bbox_left: 0,
                bbox_right: 1,
                bbox_top: 0,
                bbox_bottom: 1,
            }))],
            timelines: Vec::new(),
            triggers: Vec::new(),
        }
    }

    #[test]
    fn wp_projects_player_part() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 S,25,2,0,-10,1,-1 Ada");
        let remote = online.remotes.get(&1).expect("remote");
        assert_eq!(remote.name, "Ada");
        assert_eq!(remote.parts.len(), 1);
        assert_eq!(
            remote.parts[0],
            RemotePart {
                kind: RemotePartKind::Player { sprite: 25 },
                frame: 2,
                dx: 0.0,
                dy: -10.0,
                xs: 1.0,
                ys: -1.0,
            }
        );
        assert_eq!((remote.sprite, remote.frame, remote.xs, remote.ys), (25, 2, 1.0, -1.0));
    }

    #[test]
    fn wp_projects_companion_part() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 P,25,4,3,8,12,1,1 Ada");
        let remote = online.remotes.get(&1).expect("remote");
        assert_eq!(remote.parts.len(), 1);
        assert_eq!(remote.parts[0].kind, RemotePartKind::Pokemon { pkmn: 25, mira: 4 });
        assert_eq!(remote.parts[0].frame, 3);
        assert_eq!((remote.parts[0].dx, remote.parts[0].dy), (8.0, 12.0));
    }

    #[test]
    fn wp_battle_flag() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 S,25,0,0,0,1,1;P,25,4,3,8,12,1,1;B;X Ada");
        let remote = online.remotes.get(&1).expect("remote");
        assert!(remote.battle);
        assert_eq!(remote.parts.len(), 2);
    }

    #[test]
    fn wp_first_player_is_primary() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 S,25,2,0,0,1,1;S,30,4,1,1,2,2 Ada");
        let remote = online.remotes.get(&1).expect("remote");
        assert_eq!(remote.sprite, 25);
        assert_eq!(remote.frame, 2);
        assert_eq!(remote.parts.len(), 2);
        assert_eq!(remote.parts[1].kind, RemotePartKind::Player { sprite: 30 });
    }

    #[test]
    fn wp_pokemon_only_is_visible() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 P,25,4,3,8,12,1,1 Ada");
        let remote = online.remotes.get(&1).expect("remote");
        assert!(remote.visible);
        assert_eq!((remote.sprite, remote.frame, remote.xs, remote.ys), (-1, 0, 1.0, 1.0));
    }

    #[test]
    fn wp_hidden_token_retains_record() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 - Ada");
        let remote = online.remotes.get(&1).expect("remote retained");
        assert!(!remote.visible);
        assert!(remote.parts.is_empty());
    }

    #[test]
    fn wp_hidden_x_retains_record() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 -500 200 S,25,0,0,0,1,1 Ada");
        assert!(!online.remotes.get(&1).expect("remote").visible);
        apply_line(&mut online, "WP 1 2 -499.5 200 S,25,0,0,0,1,1 Ada");
        assert!(online.remotes.get(&1).expect("remote").visible);
    }

    #[test]
    fn wp_malformed_leaves_previous() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 S,25,0,0,0,1,1 Ada");
        assert!(world::parse_wp(b"WP 1 2 nope 200 S,25,0,0,0,1,1 Ada").is_err());
        let remote = online.remotes.get(&1).expect("remote");
        assert_eq!(remote.x, 100.0);
        assert_eq!(remote.sprite, 25);
    }

    #[test]
    fn wp_self_update_is_ignored() {
        let mut online = ReloadedOnline::default();
        online.apply_local_id(Some(5));
        apply_line(&mut online, "WP 5 2 100 200 S,25,0,0,0,1,1 Ada");
        assert!(!online.remotes.contains_key(&5));
        assert!(online.world.is_empty());
    }

    #[test]
    fn wp_control_bytes_are_sanitized_for_world_and_host() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 S,25,0,0,0,1,1 A\tB\x7fC");
        assert_eq!(online.remotes.get(&1).expect("remote").name, "ABC");
        assert_eq!(online.world.get(1).expect("world remote").name, b"ABC");
    }

    #[test]
    fn wp_accepts_signed_32_bit_coordinates() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 2147483647 -2147483648 S,25,0,0,0,1,1 Ada");
        let remote = online.remotes.get(&1).expect("remote");
        assert_eq!((remote.x, remote.y), (2_147_483_647.0, -2_147_483_648.0));
        assert!(world::parse_wp(b"WP 2 2 2147483648 0 S,1,0,0,0,1,1").is_err());
    }

    fn json_state(id: u64, name: &str, room: i32, x: f64, y: f64) -> Event {
        Event::State {
            id,
            name: name.to_string(),
            room,
            x,
            y,
            sprite: 1,
            frame: 0,
            xs: 1.0,
            ys: 1.0,
            angle: 0.0,
            visible: true,
            bbox: [0, 0, 0, 0],
            depth: 0.0,
            blend: 0xffffff,
            alpha: 1.0,
        }
    }

    #[test]
    fn json_state_uses_world_lru_capacity_authority() {
        let mut online = ReloadedOnline::default();
        online.world = world::WorldState::with_capacity(None, 2);
        online.handle_json_event(json_state(1, "A\t", 3, 1.0, 1.0));
        online.handle_json_event(json_state(2, "B", 2, 2.0, 2.0));
        online.handle_json_event(json_state(3, "C\x7f", 2, 3.0, 3.0));

        assert!(!online.remotes.contains_key(&1));
        assert!(online.remotes.contains_key(&2));
        assert!(online.remotes.contains_key(&3));
        assert!(online.world.get(1).is_none());
        assert_eq!(online.world.len(), 2);
        assert_eq!(online.remotes.get(&3).expect("remote").name, "C");
        assert_eq!(online.world.get(3).expect("world remote").name, b"C");

        online.handle_json_event(Event::Left { id: 2 });
        assert!(!online.remotes.contains_key(&2));
        assert!(online.world.get(2).is_none());
        online.handle_json_event(Event::Disconnected("fin".into()));
        assert!(online.remotes.is_empty());
        assert!(online.world.is_empty());
    }

    #[test]
    fn json_state_eviction_matches_world_room_policy_and_clears_pending() {
        let mut online = ReloadedOnline::default();
        online.world = world::WorldState::with_capacity(None, 3);
        online.handle_json_event(json_state(1, "A", 3, 1.0, 1.0));
        online.handle_json_event(json_state(2, "B", 2, 2.0, 2.0));
        online.handle_json_event(json_state(3, "C", 3, 3.0, 3.0));
        online.pending_say.insert(1, ("hola".into(), Instant::now()));

        online.handle_json_event(json_state(4, "D", 2, 4.0, 4.0));

        assert_eq!(online.world.get(1), None);
        assert!(!online.remotes.contains_key(&1));
        assert!(!online.pending_say.contains_key(&1));
        assert!(online.world.get(2).is_some());
        assert!(online.world.get(3).is_some());
        assert!(online.world.get(4).is_some());
        assert_eq!(online.world.len(), online.remotes.len());
        let mut world_ids: Vec<u64> = online.world.remotes().map(|remote| remote.id).collect();
        let mut host_ids: Vec<u64> = online.remotes.keys().copied().collect();
        world_ids.sort_unstable();
        host_ids.sort_unstable();
        assert_eq!(world_ids, host_ids);

        online.handle_json_event(json_state(2, "B2", 2, 5.0, 5.0));
        assert_eq!(online.world.len(), 3);
        assert_eq!(online.remotes.len(), 3);
        assert_eq!(online.remotes.get(&2).expect("remote").name, "B2");
    }

    #[test]
    fn wdel_known_and_unknown() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 7 2 100 200 S,25,0,0,0,1,1 Ada");
        let message = world::parse_wdel(b"WDEL 7").unwrap();
        online.apply_wdel_message(&message);
        assert!(!online.remotes.contains_key(&7));
        let unknown = world::parse_wdel(b"WDEL 99").unwrap();
        online.apply_wdel_message(&unknown);
        assert!(!online.remotes.contains_key(&99));
    }

    #[test]
    fn wdel_malformed_leaves_previous() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 7 2 100 200 S,25,0,0,0,1,1 Ada");
        for line in [b"WDEL 7 9".as_slice(), b"WDEL -1", b"WDEL 18446744073709551616"] {
            assert!(world::parse_wdel(line).is_err());
        }
        assert!(online.remotes.contains_key(&7));
    }

    #[test]
    fn s_part_uses_gm_sprite_index() {
        let mut online = ReloadedOnline::default();
        apply_line(&mut online, "WP 1 2 100 200 S,0,0,0,0,1,1;S,3856,1,0,0,1,1 Ada");
        let remote = online.remotes.get(&1).expect("remote");
        assert_eq!(remote.parts[0].kind, RemotePartKind::Player { sprite: 0 });
        assert_eq!(remote.parts[1].kind, RemotePartKind::Player { sprite: 3856 });
        assert_eq!(remote.sprite, 0);
    }

    #[test]
    fn p_species_493_maps_700() {
        let root = temp_root("p493");
        touch(&root, &["P700_A"]);
        let plan = plan_battle_sprite(&root, 493, 10, world::BattleContext::default());
        assert_eq!(plan_file(plan), "P700_A.gmspr");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn p_species_1493_maps_1700() {
        let root = temp_root("p1493");
        touch(&root, &["P1700_H"]);
        let plan = plan_battle_sprite(&root, 1493, 11, world::BattleContext::default());
        assert_eq!(plan_file(plan), "P1700_H.gmspr");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn p_facing_aliases() {
        let root = temp_root("facing");
        touch(&root, &["P25_2", "P25_A", "P25_H"]);
        let context = world::BattleContext::default();
        assert_eq!(plan_file(plan_battle_sprite(&root, 25, battle_slot_for_facing(0), context)), "P25_A.gmspr");
        assert_eq!(plan_file(plan_battle_sprite(&root, 25, battle_slot_for_facing(-1), context)), "P25_H.gmspr");
        assert_eq!(plan_file(plan_battle_sprite(&root, 25, battle_slot_for_facing(99), context)), "P25_2.gmspr");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn p_missing_left_uses_mirrored() {
        let root = temp_root("mirrored");
        touch(&root, &["P25_3"]);
        let plan = plan_battle_sprite(&root, 25, 1, world::BattleContext::default());
        assert!(matches!(&plan, PokemonSpritePlan::Mirrored(_)));
        assert_eq!(plan_file(plan), "P25_3.gmspr");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn p_missing_sleep_uses_hurt() {
        let root = temp_root("sleep");
        touch(&root, &["P25_H"]);
        let plan = plan_battle_sprite(&root, 25, 12, world::BattleContext::default());
        assert_eq!(plan_file(plan), "P25_H.gmspr");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn p_missing_required_uses_placeholder_8() {
        let root = temp_root("placeholder");
        let plan = plan_battle_sprite(&root, 25, 2, world::BattleContext::default());
        assert!(matches!(&plan, PokemonSpritePlan::Placeholder { sprite: 8, .. }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn p_alt_missing_uses_base() {
        let root = temp_root("alt");
        touch(&root, &["P25_2"]);
        let plan = plan_battle_sprite(&root, 25, 22, world::BattleContext::default());
        assert_eq!(plan_file(plan), "P25_2.gmspr");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn p_shiny_uses_shiny_id() {
        let root = temp_root("shiny");
        touch(&root, &["P1001_2"]);
        let plan = plan_battle_sprite(&root, 1001, 2, world::BattleContext::default());
        assert_eq!(plan_file(plan), "P1001_2.gmspr");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn p_remap_target_missing_placeholder() {
        let root = temp_root("remapmissing");
        let plan = plan_battle_sprite(&root, 493, 2, world::BattleContext::default());
        assert!(matches!(&plan, PokemonSpritePlan::Placeholder { sprite: 8, .. }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn net_say_time_matches_gml_limits() {
        assert_eq!(say_time_ms(""), 3000);
        assert_eq!(say_time_ms("12345"), 3300);
        assert_eq!(say_time_ms(&"x".repeat(200)), 9000);
    }

    fn queue_wire(online: &mut ReloadedOnline, wire: &[u8]) {
        online.handle_roz_message(wire);
    }

    #[test]
    fn offer_transport_queues_the_payload_after_the_envelope() {
        let payload = b"Fixture Nv. 50\x1er448.000000\x1d";
        let mut wire = b"OFFER\x1f".to_vec();
        wire.extend_from_slice(payload);

        let mut online = ReloadedOnline::default();
        queue_wire(&mut online, &wire);

        let message = online.take_peer_message().expect("queued offer");
        assert_eq!(message.kind, PeerMessageKind::Offer);
        assert_eq!(message.payload, payload);
        assert!(online.take_peer_message().is_none());
    }

    #[test]
    fn offer_transport_keeps_empty_and_non_utf8_payloads_verbatim() {
        let mut online = ReloadedOnline::default();
        queue_wire(&mut online, b"OFFER\x1f");
        queue_wire(&mut online, b"OFFER\x1f\xff\xfe\x1f\x00");

        assert_eq!(online.take_peer_message().unwrap().payload, b"");
        assert_eq!(
            online.take_peer_message().unwrap().payload,
            b"\xff\xfe\x1f\x00"
        );
    }

    #[test]
    fn offer_transport_requires_a_complete_prefix() {
        let mut online = ReloadedOnline::default();
        queue_wire(&mut online, b"OFFER");
        queue_wire(&mut online, b"OFFERX\x1fpayload");
        assert!(online.take_peer_message().is_none());

        queue_wire(&mut online, b"GIFT\x1fgift");
        queue_wire(&mut online, b"OK\x1f");
        queue_wire(&mut online, b"NO\x1f");
        assert_eq!(online.take_peer_message().unwrap().kind, PeerMessageKind::Gift);
        assert_eq!(online.take_peer_message().unwrap().kind, PeerMessageKind::Ok);
        assert_eq!(online.take_peer_message().unwrap().kind, PeerMessageKind::No);
    }

    #[test]
    fn repeated_offer_is_queued_twice_without_deduplication() {
        let mut online = ReloadedOnline::default();
        let wire = b"OFFER\x1fsame";
        queue_wire(&mut online, wire);
        queue_wire(&mut online, wire);

        assert_eq!(online.take_peer_message().unwrap().payload, b"same");
        assert_eq!(online.take_peer_message().unwrap().payload, b"same");
        assert!(online.take_peer_message().is_none());
    }

    #[test]
    fn peer_message_queue_is_capped_at_64_and_drops_the_oldest() {
        let mut online = ReloadedOnline::default();
        for index in 0..66 {
            let mut wire = b"OFFER\x1f".to_vec();
            wire.extend_from_slice(format!("offer-{index}").as_bytes());
            queue_wire(&mut online, &wire);
        }

        let queued: Vec<Vec<u8>> = std::iter::from_fn(|| online.take_peer_message())
            .map(|message| message.payload)
            .collect();
        assert_eq!(queued.len(), 64);
        assert_eq!(queued.first().unwrap(), b"offer-2");
        assert_eq!(queued.last().unwrap(), b"offer-65");
    }

    #[test]
    fn clear_peer_messages_drops_stale_confirmations_before_a_new_trade() {
        let mut online = ReloadedOnline::default();
        queue_wire(&mut online, b"OFFER\x1fold");
        queue_wire(&mut online, b"OK\x1f");
        queue_wire(&mut online, b"NO\x1f");
        assert_eq!(online.peer_messages.len(), 3);

        online.clear_peer_messages();
        assert!(online.take_peer_message().is_none());
    }

    #[test]
    fn clear_peer_controls_keeps_an_early_offer_for_legacy_peers() {
        let mut online = ReloadedOnline::default();
        queue_wire(&mut online, b"OFFER\x1fearly");
        queue_wire(&mut online, b"OK\x1f");
        queue_wire(&mut online, b"NO\x1f");

        online.clear_peer_controls();
        let message = online.take_peer_message().expect("early offer survives");
        assert_eq!(message.kind, PeerMessageKind::Offer);
        assert_eq!(message.payload, b"early");
        assert!(online.take_peer_message().is_none());
    }

    #[test]
    fn peer_left_and_remote_left_are_reported_once_for_trade_cancellation() {
        let mut online = ReloadedOnline::default();
        online.handle_roz_event(RozEvent::PeerLeft);
        assert!(online.take_peer_left());
        assert!(!online.take_peer_left());

        online.handle_json_event(Event::Left { id: 7 });
        assert_eq!(online.take_left_peer(), Some(7));
        assert_eq!(online.take_left_peer(), None);
    }

    #[test]
    fn failed_connection_is_not_ready_and_looks_like_a_peer_loss() {
        let mut online = ReloadedOnline::default();
        assert!(!online.is_ready());
        online.handle_roz_event(RozEvent::Disconnected { reason: "fin".into() });
        assert!(!online.is_ready());
        assert!(online.take_peer_left());
        assert!(!online.take_peer_left());
    }

    #[test]
    fn zibelan_full_is_a_terminal_failure_and_clears_remotes() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WP 7 2 100 200 S,25,0,0,0,1,1 Ada\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&7));

        transport.send(b"WFULL\n");
        pump_world(&mut online, |online| matches!(&online.connection, Some(Connection::Failed(_))));

        assert!(!online.is_ready());
        assert!(online.remotes.is_empty());
        assert!(online.world.is_empty());
        assert!(online.status().contains("mundo está lleno"), "{}", online.status());
    }

    #[test]
    fn zibelan_rate_limit_is_a_terminal_failure() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"RATE_LIMIT 30\n");
        pump_world(&mut online, |online| matches!(&online.connection, Some(Connection::Failed(_))));

        assert!(!online.is_ready());
        assert!(online.status().contains("limitó"), "{}", online.status());
    }

    #[test]
    fn zibelan_version_rejection_is_a_terminal_failure() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"ERROR version incompatible\n");
        pump_world(&mut online, |online| matches!(&online.connection, Some(Connection::Failed(_))));

        assert!(!online.is_ready());
        assert!(online.status().contains("versión"), "{}", online.status());
    }

    #[test]
    fn zibelan_peer_left_reports_a_clear_message_without_dropping_the_world() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"PEER_LEFT\n");
        pump_world(&mut online, |online| online.status().contains("rival abandonó"));

        assert!(online.take_peer_left());
        assert!(!online.take_peer_left());
        assert!(online.is_ready());
        assert!(online.status().contains("rival"), "{}", online.status());
    }

    #[test]
    fn connect_watchdog_turns_a_hung_worker_into_a_terminal_timeout() {
        let (_tx, rx) = mpsc::channel();
        let mut online = ReloadedOnline::default();
        online.connection = Some(Connection::Waiting(rx));
        online.connection_mode = Some(OnlineMode::ZibelanWorld);
        online.connect_deadline = Some(Instant::now() - Duration::from_secs(1));

        online.poll_connection();

        assert!(matches!(&online.connection, Some(Connection::Failed(_))));
        assert!(!online.is_ready());
        assert!(online.status().contains("tiempo de espera"), "{}", online.status());
    }

    #[test]
    fn handshake_watchdog_turns_a_silent_server_into_a_terminal_timeout() {
        let (mut online, _transport) = mock_world_transport();
        online.handshake_deadline = Some(Instant::now() - Duration::from_secs(1));

        online.poll_events();

        assert!(matches!(&online.connection, Some(Connection::Failed(_))));
        assert!(!online.is_ready());
        assert!(online.status().contains("tiempo de espera"), "{}", online.status());
    }

    #[test]
    fn localize_connect_error_keeps_unknown_details_but_translates_known_failures() {
        assert!(ReloadedOnline::localize_connect_error("Connection refused".into()).contains("rechazó"));
        assert_eq!(
            ReloadedOnline::localize_connect_error("detalle propio".into()),
            "detalle propio"
        );
    }

    #[test]
    fn mock_transport_applies_wp_and_wdel_to_cleanroom_state() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WP 7 2 100.5 -200.25 S,25,2,0,-10,1,-1 Ada\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&7));

        let remote = online.remotes.get(&7).expect("remote projected");
        assert_eq!(remote.name, "Ada");
        assert_eq!(remote.room, 2);
        assert_eq!((remote.x, remote.y), (100.5, -200.25));
        assert_eq!((remote.sprite, remote.frame), (25, 2));
        assert_eq!(remote.parts.len(), 1);
        let state = online.world.get(7).expect("cleanroom remote");
        assert_eq!(state.room, 2);
        assert_eq!(state.name, b"Ada");

        transport.send(b"WDEL 7\r\n");
        pump_world(&mut online, |online| {
            !online.remotes.contains_key(&7) && online.world.get(7).is_none()
        });
    }

    #[test]
    fn mock_transport_wpos_echo_does_not_touch_remote_state() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WP 7 2 10 20 S,25,2,0,0,1,1 Ada\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&7));

        transport.send(b"WPOS 3 999 999 S,30,5,1,2,3,2\n");
        transport.send(b"WP 8 2 30 40 S,26,1,0,0,1,1 Bob\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&8));

        assert_eq!(online.remotes.len(), 2);
        let remote = online.remotes.get(&7).expect("remote untouched");
        assert_eq!((remote.x, remote.y, remote.sprite), (10.0, 20.0, 25));
        assert!(!online.remotes.values().any(|remote| remote.room == 3));
        assert!(online.world.get(8).is_some());
    }

    #[test]
    fn mock_transport_wlist_updates_display_and_ignores_malformed() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WLIST 2\x1dAda\x1eRoute 1\x1eGM\x1dBob\x1eRoute 2\n");
        pump_world(&mut online, |online| online.world_list.len() == 2);
        assert_eq!(online.world_list, ["Ada - Route 1 (GM)", "Bob - Route 2"]);
        let previous = online.world_list.clone();

        transport.send(b"WLIST 1\x00\n");
        transport.send(b"WP 9 2 1 2 S,1,0,0,0,1,1 sentinel\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&9));
        assert_eq!(online.world_list, previous);
    }

    #[test]
    fn mock_transport_self_updates_are_suppressed_after_wok() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WOK 42\n");
        pump_world(&mut online, |online| {
            online.local_id() == Some(42) && online.world.local_id() == Some(42)
        });

        transport.send(b"WP 42 2 100 200 S,25,0,0,0,1,1 Self\n");
        transport.send(b"WP 100 2 100 200 S,26,0,0,0,1,1 Other\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&100));

        assert!(!online.remotes.contains_key(&42));
        assert!(online.world.get(42).is_none());
        assert_eq!(online.remotes.len(), 1);
    }

    #[test]
    fn mock_transport_room_filter_selects_host_draw_candidates() {
        let (mut online, transport) = mock_world_transport();
        for line in [
            b"WP 1 2 10 20 S,25,0,0,0,1,1 A\n".as_slice(),
            b"WP 2 3 10 20 S,26,0,0,0,1,1 B\n",
            b"WP 3 2 -500 20 S,27,0,0,0,1,1 C\n",
            b"WP 4 2 10 20 - D\n",
            b"WP 5 2 10 20 P,25,4,3,8,12,1,1 E\n",
        ] {
            transport.send(line);
        }
        pump_world(&mut online, |online| online.remotes.len() == 5);

        let mut room_two: Vec<u64> =
            online.part_candidates(2, None).into_iter().map(|(id, _, _)| id).collect();
        room_two.sort_unstable();
        assert_eq!(room_two, [1, 5]);
        let mut room_three: Vec<u64> =
            online.part_candidates(3, None).into_iter().map(|(id, _, _)| id).collect();
        room_three.sort_unstable();
        assert_eq!(room_three, [2]);
        assert!(online.part_candidates(9, None).is_empty());
        assert!(!online.remotes.get(&3).expect("hidden retained").visible);
        assert!(!online.remotes.get(&4).expect("token hidden retained").visible);
    }

    #[test]
    fn mock_transport_overlays_filter_room_and_hidden_remotes() {
        let (mut online, transport) = mock_world_transport();
        for line in [
            b"WP 1 2 10 20 S,0,0,0,0,1,1 Visible\n".as_slice(),
            b"WP 2 3 10 20 S,0,0,0,0,1,1 Other\n",
            b"WP 3 2 -500 20 S,0,0,0,0,1,1 Hidden\n",
            b"WP 4 2 10 20 - NoToken\n",
        ] {
            transport.send(line);
        }
        pump_world(&mut online, |online| online.remotes.len() == 4);

        let assets = test_assets_with_one_sprite();
        let overlays = online.remote_overlays(2, &assets);
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].name, "Visible");
        let other = online.remote_overlays(3, &assets);
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].name, "Other");
        assert!(online.remote_overlays(9, &assets).is_empty());
    }

    #[test]
    fn mock_transport_hidden_remote_is_retained_until_delete() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WP 7 2 100 200 S,25,0,0,0,1,1 Ada\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&7));

        transport.send(b"WP 7 2 -500 200 S,25,0,0,0,1,1 Ada\n");
        pump_world(&mut online, |online| {
            online.remotes.get(&7).map(|remote| !remote.visible).unwrap_or(false)
        });
        assert_eq!(online.world.get(7).expect("hidden record").x, -500.0);

        transport.send(b"WP 7 2 -499.5 200 S,25,0,0,0,1,1 Ada\n");
        pump_world(&mut online, |online| {
            online.remotes.get(&7).map(|remote| remote.visible).unwrap_or(false)
        });

        transport.send(b"WDEL 7\n");
        pump_world(&mut online, |online| {
            !online.remotes.contains_key(&7) && online.world.get(7).is_none()
        });
    }

    #[test]
    fn mock_transport_malformed_lines_leave_previous_state_intact() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WP 7 2 100 200 S,25,0,0,0,1,1 Ada\n");
        transport.send(b"WLIST 1\x1dAda\x1eRoute 1\x1eGM\n");
        pump_world(&mut online, |online| {
            online.remotes.contains_key(&7) && online.world_list.len() == 1
        });
        let before_state = online.world.get(7).cloned().expect("cleanroom remote");
        let before_list = online.world_list.clone();

        for line in [
            b"WP 7 2 nope 200 S,25,0,0,0,1,1 Ada\n".as_slice(),
            b"WDEL 7 extra\n",
            b"WPOS 2 1 2 S,1,0,0,0,1,1 extra\n",
            b"WLIST 1\x00\n",
        ] {
            transport.send(line);
        }
        transport.send(b"WP 9 2 1 2 S,1,0,0,0,1,1 sentinel\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&9));

        let remote = online.remotes.get(&7).expect("previous remote kept");
        assert_eq!((remote.x, remote.y, remote.sprite), (100.0, 200.0, 25));
        assert_eq!(online.world.get(7), Some(&before_state));
        assert_eq!(online.world_list, before_list);
        assert_eq!(online.remotes.len(), 2);
    }

    #[test]
    fn mock_transport_welcome_shows_id_and_version_once() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WELCOME 3011\n");
        pump_world(&mut online, |online| online.local_id() == Some(3011));

        assert!(online.status().contains("Bienvenido a Zibelan"));
        assert!(online.status().contains("id #3011"));
        assert!(online.status().contains("protocolo 3011"));
        assert_eq!(
            online.take_chat(),
            Some((
                "Servidor".to_string(),
                "Bienvenido a Zibelan: id #3011 (protocolo 3011)".to_string()
            ))
        );

        transport.send(b"WELCOME 3011\nWOK 3011\n");
        pump_world(&mut online, |online| online.status().contains("mundo conectado"));
        assert!(online.status().contains("Bienvenido a Zibelan"));
        assert_eq!(online.take_chat(), None);
    }

    #[test]
    fn mock_transport_malformed_welcome_keeps_fallback_parse() {
        let (mut online, transport) = mock_world_transport();
        transport.send(b"WELCOME greeting\nWAIT\n");
        pump_world(&mut online, |online| online.status().contains("esperando rival"));

        assert_eq!(online.local_id(), Some(0));
        assert_eq!(
            online.take_chat(),
            Some((
                "Servidor".to_string(),
                "Bienvenido a Zibelan: id #0 (protocolo 0)".to_string()
            ))
        );
    }

    #[test]
    fn json_welcome_is_single_notice_across_reconnect() {
        let mut online = ReloadedOnline::default();
        online.handle_json_event(Event::Connected { id: 7 });
        online.handle_json_event(Event::Connected { id: 7 });
        assert_eq!(
            online.take_chat(),
            Some((
                "Servidor".to_string(),
                "Bienvenido a Reloaded Online: id #7".to_string()
            ))
        );
        assert_eq!(online.take_chat(), None);

        online.handle_json_event(Event::Connected { id: 8 });
        assert_eq!(
            online.take_chat(),
            Some((
                "Servidor".to_string(),
                "Bienvenido a Reloaded Online: id #8".to_string()
            ))
        );
        assert_eq!(online.take_chat(), None);

        online.handle_json_event(Event::Joined { id: 8, name: "Ana".to_string() });
        assert!(online.status().contains("Bienvenido a Reloaded Online"));
        assert!(online.status().contains("Ana"));
        assert_eq!(online.take_chat(), None);
    }

    #[test]
    fn mock_transport_world_joined_queues_one_notice_for_the_connection() {
        let (mut online, transport) = mock_world_transport();
        assert!(!online.take_connection_notice());

        transport.send(b"WELCOME 3011\n");
        pump_world(&mut online, |online| online.local_id() == Some(3011));
        assert!(!online.take_connection_notice());

        transport.send(b"WOK 3011\n");
        pump_world(&mut online, |online| online.status().contains("mundo conectado"));

        assert!(online.take_connection_notice());
        assert!(!online.take_connection_notice());
        assert_eq!(
            online.take_chat(),
            Some((
                "Servidor".to_string(),
                "Bienvenido a Zibelan: id #3011 (protocolo 3011)".to_string()
            ))
        );

        transport.send(b"WOK 3011\nWP 77 2 10 20 S,25,0,0,0,1,1 Ada\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&77));
        assert!(!online.take_connection_notice());
        assert_eq!(online.take_chat(), None);
    }

    #[test]
    fn zibelan_lobby_does_not_raise_the_world_notice() {
        let (mut online, transport) = mock_world_transport();
        online.connection_mode = Some(OnlineMode::ZibelanLobby);

        transport.send(b"WOK 42\n");
        pump_world(&mut online, |online| online.local_id() == Some(42));

        assert!(!online.take_connection_notice());
    }

    #[test]
    fn json_connected_queues_one_notice_per_connection() {
        let mut online = ReloadedOnline::default();

        online.handle_json_event(Event::Connected { id: 7 });
        assert!(online.take_connection_notice());
        assert!(!online.take_connection_notice());

        online.handle_json_event(Event::Connected { id: 8 });
        online.handle_json_event(Event::Joined { id: 8, name: "Ana".to_string() });
        assert!(!online.take_connection_notice());

        online.stop();
        online.handle_json_event(Event::Connected { id: 9 });
        assert!(online.take_connection_notice());
    }

    #[test]
    fn disconnecting_drops_a_pending_connection_notice() {
        let mut online = ReloadedOnline::default();
        online.handle_json_event(Event::Connected { id: 7 });
        online.handle_json_event(Event::Disconnected("fin".into()));
        assert!(!online.take_connection_notice());

        online.handle_json_event(Event::Connected { id: 8 });
        assert!(online.take_connection_notice());
    }

    #[test]
    fn mock_transport_battle_start_sets_side_seed_and_status() {
        let (mut online, mut transport) = mock_duplex_transport();
        transport.send(b"WELCOME 7\nSTART 1 Rival\n");
        pump_world(&mut online, |online| {
            online.local_id() == Some(7) && online.battle_side() == Some(1)
        });

        assert_eq!(online.battle_seed(42), 1_000_003);
        online.set_battle_epoch(17);
        assert_eq!(online.battle_seed(42), 17 * 1_000_003);
        assert!(online.status().contains("lado 1"));
    }

    #[test]
    fn mock_transport_battle_team_exchange_round_trips_meta_and_bytes() {
        let (mut online, mut transport) = mock_duplex_transport();
        transport.send(b"WELCOME 7\nSTART 1 Rival\n");
        pump_world(&mut online, |online| online.battle_side() == Some(1));
        online.begin_battle();

        let meta = BattleTeamMeta {
            seed: 1_000_003,
            fondo: 7,
            modo: 1,
            sala: f3r::room::BATTLE_AQUA,
            delay: 3,
        };
        let data = b"Trainer\x1fteam\r\n\xffbytes\r\n".to_vec();
        let expected = zibelan_team_payload(&data, &meta);
        assert!(online.send_battle_team(42, &data, &meta));
        assert_eq!(next_client_message(&transport), expected);

        transport.send(format!("MSG {}\n", hex::encode(&expected)).as_bytes());
        pump_world(&mut online, |online| !online.battle_teams.is_empty());
        let team = online.take_battle_team().expect("incoming team");
        assert_eq!(team.id, 42);
        assert_eq!(team.meta.seed, meta.seed);
        assert_eq!(team.meta.fondo, meta.fondo);
        assert_eq!(team.meta.modo, meta.modo);
        assert_eq!(team.meta.sala, meta.sala);
        assert_eq!(team.meta.delay, meta.delay);
        assert_eq!(team.data, data);
        assert!(team.zibelan);
    }

    #[test]
    fn mock_transport_battle_config_round_trips() {
        let (mut online, mut transport) = mock_duplex_transport();
        transport.send(b"WELCOME 7\nSTART 1 Rival\n");
        pump_world(&mut online, |online| online.battle_side() == Some(1));
        online.begin_battle();

        let config = super::battle_config_tests::sample();
        let expected = zibelan_config_payload(&config);
        assert!(online.send_battle_config(&config));
        assert_eq!(next_client_message(&transport), expected);

        transport.send(format!("MSG {}\n", hex::encode(&expected)).as_bytes());
        pump_world(&mut online, |online| online.battle_configs.len() == 1);
        assert_eq!(online.take_battle_config(), Some(config));
    }

    #[test]
    fn mock_transport_invalid_battle_config_reports_error() {
        let (mut online, mut transport) = mock_duplex_transport();
        let bad = b"WCFG\x1f1\x1e2".to_vec();
        transport.send(format!("MSG {}\n", hex::encode(&bad)).as_bytes());
        pump_world(&mut online, |online| online.battle_config_error.is_some());

        assert!(online.take_battle_config().is_none());
        let error = online.take_battle_config_error().expect("config error");
        assert!(error.contains("WCFG incompleto"));
    }

    #[test]
    fn mock_transport_battle_checksum_pairs_with_input_in_both_orders() {
        let (mut online, mut transport) = mock_duplex_transport();
        assert!(online.send_battle_input(42, 0, 1, 1, 0));
        assert_eq!(next_client_line(&transport), "IN 1 1 0");

        transport.send(b"SUM 5 10 111\nIN 5 10 3\n");
        pump_world(&mut online, |online| !online.battle_inputs.is_empty());
        let input = online.take_battle_input().expect("input after checksum");
        assert_eq!(
            (input.id, input.mask, input.seq, input.frame, input.checksum),
            (42, 3, 5, 10, 111)
        );

        transport.send(b"IN 5 11 4\nSUM 5 11 222\n");
        pump_world(&mut online, |online| {
            online.battle_inputs.iter().any(|input| input.frame == 11 && input.checksum == 222)
        });
        let input = online.take_battle_input().expect("input before checksum");
        assert_eq!(
            (input.id, input.mask, input.seq, input.frame, input.checksum),
            (42, 4, 5, 11, 222)
        );

        online.set_battle_epoch(7);
        assert!(online.send_battle_input(42, 5, 9, 29, 0xaaaa));
        assert_eq!(next_client_line(&transport), "IN 7 29 5");
        assert!(online.send_battle_input(42, 5, 10, 30, 0xbbbb));
        assert_eq!(next_client_line(&transport), "IN 7 30 5");
        assert_eq!(next_client_line(&transport), "SUM 7 30 48059");
    }

    #[test]
    fn mock_transport_trade_offer_and_ok_round_trip() {
        let (mut online, mut transport) = mock_duplex_transport();
        let payload = b"Fixture Nv. 50\x1er448.000000\x1d";
        let offer = [b"OFFER\x1f".as_slice(), payload.as_slice()].concat();
        let ok = b"OK\x1f".to_vec();

        transport.send(
            format!("MSG {}\nMSG {}\n", hex::encode(&offer), hex::encode(&ok)).as_bytes(),
        );
        pump_world(&mut online, |online| online.peer_messages.len() == 2);
        let message = online.take_peer_message().expect("incoming offer");
        assert_eq!(message.kind, PeerMessageKind::Offer);
        assert_eq!(message.payload, payload);
        let message = online.take_peer_message().expect("incoming ok");
        assert_eq!(message.kind, PeerMessageKind::Ok);
        assert!(message.payload.is_empty());

        assert!(online.send_peer_message(PeerMessageKind::Offer, payload));
        assert_eq!(next_client_message(&transport), offer);
        assert!(online.send_peer_message(PeerMessageKind::Ok, &[]));
        assert_eq!(next_client_message(&transport), b"OK\x1f".to_vec());
        assert!(online.send_peer_message(PeerMessageKind::No, &[]));
        assert_eq!(next_client_message(&transport), b"NO\x1f".to_vec());
    }

    #[test]
    fn mock_transport_wp_wdel_round_trip_keeps_other_remotes() {
        let (mut online, mut transport) = mock_duplex_transport();
        for chunk in [
            b"WP 7 2 10".as_slice(),
            b"0.5 -20.".as_slice(),
            b"25 S,25,2,0,-10,1,-1 Ada\n".as_slice(),
            b"WP 8 2 1 2 S,26,1,0,0,1,1 Bob\n".as_slice(),
        ] {
            transport.send(chunk);
        }
        pump_world(&mut online, |online| online.remotes.len() == 2);

        let remote = online.remotes.get(&7).expect("remote projected");
        assert_eq!(remote.name, "Ada");
        assert_eq!((remote.x, remote.y), (100.5, -20.25));
        assert_eq!(online.world.get(7).expect("cleanroom remote").x, 100.5);

        transport.send(b"WDEL 7\n");
        pump_world(&mut online, |online| {
            !online.remotes.contains_key(&7) && online.world.get(7).is_none()
        });
        assert_eq!(online.remotes.len(), 1);
        assert!(online.remotes.contains_key(&8));
        assert!(online.world.get(8).is_some());
    }

    #[test]
    fn mock_transport_pokemon_part_uses_cleanroom_sprite_mapping() {
        let root = temp_root("transport_sprite");
        touch(&root, &["P700_1", "P700_4", "P25_1", "P25_4"]);
        let (mut online, transport) = mock_world_transport();
        online.program_directory = root.clone();

        transport.send(b"WP 1 2 100 200 P,493,4,3,8,12,1,1 Ada\n");
        pump_world(&mut online, |online| online.remotes.contains_key(&1));

        let part = &online.remotes.get(&1).expect("remote projected").parts[0];
        assert_eq!(part.kind, RemotePartKind::Pokemon { pkmn: 493, mira: 4 });
        assert_eq!(plan_file(online.pokemon_sprite_plan(493, 4)), "P700_4.gmspr");
        assert_eq!(plan_file(online.pokemon_sprite_plan(25, 4)), "P25_4.gmspr");
        assert!(matches!(
            online.pokemon_sprite_plan(25, 5),
            PokemonSpritePlan::Placeholder { sprite: 8, .. }
        ));

        std::fs::remove_dir_all(root).unwrap();
    }
}
