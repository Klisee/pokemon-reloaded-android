//! Reloaded Online: clean-room world adapter.
//!
//! This module only reads the current player's position from the emulator and draws remote
//! players with the game's own sprites. It does not contain or call any of the game's GML.

use crate::{
    game::{Assets, GetAsset, RoomState},
    math::Real,
    render::Renderer,
    types::ID,
    util,
};
use reloaded_online_net::{Event, OnlineClient};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize)]
struct ReloadedOnlineConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_server")]
    server: String,
    #[serde(default = "default_name")]
    name: String,
    #[serde(default = "default_room")]
    room: String,
    #[serde(default)]
    bridge_upstream: String,
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
}

enum Connection {
    Waiting(Receiver<Result<OnlineClient, String>>),
    Ready(OnlineClient),
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

pub struct ReloadedOnline {
    connection: Option<Connection>,
    remotes: HashMap<u64, Remote>,
    local_id: Option<u64>,
    local_name: String,
    server: String,
    room_name: String,
    bridge_upstream: String,
    last_send: Instant,
    last_draw: Instant,
    last_debug: Instant,
    requests: Vec<IncomingRequest>,
    responses: Vec<IncomingResponse>,
    battle_teams: Vec<(u64, String)>,
    battle_inputs: Vec<(u64, u32)>,
    status: String,
}

impl Default for ReloadedOnline {
    fn default() -> Self {
        Self {
            connection: None,
            remotes: HashMap::new(),
            local_id: None,
            local_name: default_name(),
            server: default_server(),
            room_name: default_room(),
            bridge_upstream: String::new(),
            last_send: Instant::now(),
            last_draw: Instant::now(),
            last_debug: Instant::now(),
            requests: Vec::new(),
            responses: Vec::new(),
            battle_teams: Vec::new(),
            battle_inputs: Vec::new(),
            status: "Reloaded Online desactivado".into(),
        }
    }
}

impl ReloadedOnline {
    pub fn new(program_directory: &str) -> Self {
        let mut online = Self::default();
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
        // A bridge configured from the launcher must not connect at startup: the in-game
        // Multijugador menu owns that connection and starts it only when the player picks it.
        if config.enabled && config.bridge_upstream.trim().is_empty() {
            online.connect(false);
        }
        online
    }

    pub fn is_active(&self) -> bool {
        self.connection.is_some()
    }

    pub fn start_own(&mut self) {
        self.connect(false);
    }

    pub fn start_remote(&mut self) {
        self.connect(true);
    }

    pub fn stop(&mut self) {
        self.connection = None;
        self.remotes.clear();
        self.local_id = None;
        self.requests.clear();
        self.responses.clear();
        self.battle_teams.clear();
        self.battle_inputs.clear();
        self.status = "Reloaded Online desconectado".into();
    }

    fn connect(&mut self, use_bridge: bool) {
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
            let result = OnlineClient::connect(&server, &name, &room);
            let _ = tx.send(result);
        });
        eprintln!("[reloaded-online] conectando a {} como {} en {}", log_server, self.local_name, self.room_name);
        self.connection = Some(Connection::Waiting(rx));
        self.status = format!("Reloaded Online conectando a {log_server}");
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

    pub fn send_battle_team(&mut self, target: u64, data: &str) -> bool {
        let Some(Connection::Ready(client)) = &self.connection else { return false };
        match client.send_battle_team(target, data) {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            },
        }
    }

    pub fn send_battle_input(&mut self, target: u64, mask: u32) -> bool {
        let Some(Connection::Ready(client)) = &self.connection else { return false };
        match client.send_battle_input(target, mask) {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            },
        }
    }

    pub fn take_battle_input(&mut self) -> Option<(u64, u32)> {
        if self.battle_inputs.is_empty() {
            None
        } else {
            Some(self.battle_inputs.remove(0))
        }
    }

    pub fn take_battle_team(&mut self) -> Option<(u64, String)> {
        if self.battle_teams.is_empty() {
            None
        } else {
            Some(self.battle_teams.remove(0))
        }
    }

    pub fn nearest_remote(&self, room_id: i32, x: f64, y: f64, max_distance: f64) -> Option<(u64, String)> {
        self.remotes
            .iter()
            .filter(|(_, remote)| remote.visible && remote.room == room_id)
            .filter_map(|(id, remote)| {
                let dx = remote.render_x - x;
                let dy = remote.render_y - y;
                let distance = (dx * dx + dy * dy).sqrt();
                (distance <= max_distance).then_some((*id, remote.name.clone(), distance))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, name, _)| (id, name))
    }

    pub fn send_request(&mut self, target: u64, kind: &str) -> bool {
        let Some(Connection::Ready(client)) = &self.connection else { return false };
        match client.send_request(target, kind, "") {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            },
        }
    }

    pub fn send_response(&mut self, target: u64, accept: bool) -> bool {
        let Some(Connection::Ready(client)) = &self.connection else { return false };
        match client.send_response(target, accept) {
            Ok(()) => true,
            Err(error) => {
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

    pub fn remote_labels(&self, room_id: i32, assets: &Assets) -> Vec<(String, f64, f64)> {
        self.remotes
            .values()
            .filter(|remote| remote.visible && remote.room == room_id)
            .filter_map(|remote| {
                let sprite = assets.sprites.get_asset(remote.sprite)?;
                let top = remote.render_y - f64::from(sprite.origin_y) * remote.ys;
                Some((remote.name.clone(), remote.render_x, top))
            })
            .collect()
    }

    pub fn tick(&mut self, room: &RoomState, assets: &Assets) {
        self.poll_connection();
        self.poll_events();
        let Some(Connection::Ready(client)) = &self.connection else { return };
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

    pub fn begin_draw(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_draw).as_secs_f64().clamp(0.0, 0.2);
        self.last_draw = now;
        let k = (dt * 18.0).min(1.0);
        for remote in self.remotes.values_mut() {
            remote.render_x += (remote.x - remote.render_x) * k;
            remote.render_y += (remote.y - remote.render_y) * k;
            remote.drawn = false;
        }
    }

    pub fn draw_before_depth(
        &mut self,
        renderer: &mut Renderer,
        assets: &Assets,
        room_id: i32,
        depth: f64,
    ) {
        let mut candidates: Vec<(u64, f64)> = self
            .remotes
            .iter()
            .filter(|(_, remote)| {
                !remote.drawn && remote.visible && remote.room == room_id && remote.depth >= depth
            })
            .map(|(id, remote)| (*id, remote.depth))
            .collect();
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (id, _) in candidates {
            if let Some(remote) = self.remotes.get_mut(&id) {
                remote.drawn = true;
                draw_remote_one(renderer, assets, remote);
            }
        }
    }

    pub fn draw_remaining(&mut self, renderer: &mut Renderer, assets: &Assets, room_id: i32) {
        let ids: Vec<u64> = self
            .remotes
            .iter()
            .filter(|(_, remote)| !remote.drawn && remote.visible && remote.room == room_id)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(remote) = self.remotes.get_mut(&id) {
                remote.drawn = true;
                draw_remote_one(renderer, assets, remote);
            }
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
                    .map(|object| object.name.as_ref() == b"obj_Prota")
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

    fn poll_connection(&mut self) {
        let Some(Connection::Waiting(rx)) = &self.connection else { return };
        match rx.try_recv() {
            Ok(Ok(client)) => {
                self.connection = Some(Connection::Ready(client));
                self.status = "Reloaded Online conectado".into();
            },
            Ok(Err(error)) => {
                self.status = error.clone();
                self.connection = Some(Connection::Failed(error));
            },
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                self.connection = Some(Connection::Failed("el hilo de conexion termino".into()));
            },
        }
    }

    fn poll_events(&mut self) {
        let mut events = Vec::new();
        if let Some(Connection::Ready(client)) = &self.connection {
            while let Some(event) = client.try_event() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                Event::Connected { id } => {
                    self.local_id = Some(id);
                    self.status = format!("Reloaded Online conectado como #{id}");
                },
                Event::Joined { id, name } => {
                    self.status = format!("Reloaded Online conectado con {name} (#{id})");
                },
                Event::State { id, name, room, x, y, sprite, frame, xs, ys, angle, visible, bbox, depth, blend, alpha } => {
                    if Some(id) == self.local_id {
                        continue
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
                        });
                },
                Event::Request { id, name, kind, info } => {
                    self.requests.push(IncomingRequest { id, name, kind, info });
                },
                Event::Response { id, name, accept } => {
                    self.responses.push(IncomingResponse { id, name, accept });
                },
                Event::BattleTeam { id, data, .. } => {
                    self.battle_teams.push((id, data));
                },
                Event::BattleInput { id, mask } => {
                    self.battle_inputs.push((id, mask));
                },
                Event::Chat { name, text, .. } => {
                    eprintln!("[reloaded-online] {name}: {text}");
                },
                Event::Left { id } => {
                    self.remotes.remove(&id);
                },
                Event::Error(error) => {
                    eprintln!("[reloaded-online] {error}");
                    self.status = error;
                },
                Event::Disconnected(error) => {
                    self.status = error.clone();
                    self.remotes.clear();
                    self.connection = Some(Connection::Failed(error));
                },
            }
        }
    }
}

fn draw_remote_one(renderer: &mut Renderer, assets: &Assets, remote: &Remote) {
    let Some(sprite) = assets.sprites.get_asset(remote.sprite) else { return };
    let Some(atlas) = sprite.get_atlas_ref(remote.frame) else { return };
    let x = remote.render_x;
    let y = remote.render_y;
    let left = x + (f64::from(sprite.bbox_left) - f64::from(sprite.origin_x)) * remote.xs;
    let right = x + (f64::from(sprite.bbox_right) - f64::from(sprite.origin_x)) * remote.xs;
    let bottom = y + (f64::from(sprite.bbox_bottom) - f64::from(sprite.origin_y)) * remote.ys;
    let (left, right) = if left <= right { (left, right) } else { (right, left) };
    let radius_x = ((right - left) * 0.5).max(0.5);
    let radius_y = (5.0 * remote.ys.abs()).max(0.5);
    renderer.set_draw_offset(0.0, 0.0);
    renderer.set_depth(remote.depth as f32);
    renderer.draw_ellipse(
        (left + right) * 0.5,
        bottom,
        radius_x,
        radius_y,
        0,
        0,
        (remote.alpha * 0.3).clamp(0.0, 1.0),
        false,
    );
    renderer.draw_sprite(
        atlas,
        x,
        y,
        remote.xs,
        remote.ys,
        remote.angle,
        remote.blend,
        remote.alpha,
    );
}

fn overlaps(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    a.0 <= b.1 && a.1 >= b.0 && a.2 <= b.3 && a.3 >= b.2
}

fn remote_bbox(remote: &Remote) -> Option<(i32, i32, i32, i32)> {
    let dx = (remote.render_x - remote.x).round() as i32;
    let dy = (remote.render_y - remote.y).round() as i32;
    Some((remote.bbox[0] + dx, remote.bbox[1] + dx, remote.bbox[2] + dy, remote.bbox[3] + dy))
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
                .map(|object| object.name.as_ref() == b"obj_Prota")
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
