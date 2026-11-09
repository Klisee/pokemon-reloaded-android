#[allow(unused_assignments)]
#[path = "../src/battle_sync.rs"]
mod battle_sync;

use std::collections::{BTreeMap, BTreeSet};

use battle_sync::{BattleSync, CHECKSUM_INTERVAL, FrameStep, INPUT_MASK, OutgoingInput, frame_seed};
use reloaded_cleanroom::z063_sec::{
    NET_STATE_PARTY_SLOTS, NetState, checksum_text, net_checksum, net_state_text, parse_sum_frame,
};

const EPOCH: i32 = 1_000_003;
const FRAMES: u32 = 600;
const SIDES: usize = 2;
const DIVERGENT_FRAME: u32 = 330;
const UP: u32 = 1 << 0;
const DOWN: u32 = 1 << 1;
const LEFT: u32 = 1 << 2;
const RIGHT: u32 = 1 << 3;
const SWITCH: u32 = 1 << 4;
const ATTACK: u32 = 1 << 5;
const BASE_POSITIONS: [(f64, f64); SIDES] = [(160.0, 96.0), (304.0, 96.0)];
const SWITCH_FRAMES: [[u32; 3]; SIDES] = [[150, 330, 450], [180, 360, 540]];

fn sample(side: usize, frame: u32) -> u32 {
    let mut mask = 0;
    if frame % 37 == 3 {
        mask |= UP;
    }
    if frame % 41 == 7 {
        mask |= DOWN;
    }
    if frame % 43 == 11 {
        mask |= LEFT;
    }
    if frame % 47 == 13 {
        mask |= RIGHT;
    }
    if frame % 53 == 17 + side as u32 * 2 {
        mask |= ATTACK;
    }
    if SWITCH_FRAMES[side].contains(&frame) {
        mask |= SWITCH;
    }
    mask & INPUT_MASK
}

fn expected_active(side: usize, applied_through: u32) -> usize {
    SWITCH_FRAMES[side].iter().filter(|frame| **frame <= applied_through).count() % NET_STATE_PARTY_SLOTS
}

fn expected_position(side: usize, applied_through: u32) -> (f64, f64) {
    let mut position = BASE_POSITIONS[side];
    for frame in 1..=applied_through {
        let mask = sample(side, frame);
        if mask & UP != 0 {
            position.1 -= 1.0;
        }
        if mask & DOWN != 0 {
            position.1 += 1.0;
        }
        if mask & LEFT != 0 {
            position.0 -= 1.0;
        }
        if mask & RIGHT != 0 {
            position.0 += 1.0;
        }
    }
    position
}

#[derive(Clone, Debug, PartialEq)]
struct World {
    actual: [usize; SIDES],
    position: [(f64, f64); SIDES],
    ps: [[f64; NET_STATE_PARTY_SLOTS]; SIDES],
    pp: [[f64; NET_STATE_PARTY_SLOTS]; SIDES],
    switch_held: [bool; SIDES],
}

impl World {
    fn new() -> Self {
        Self {
            actual: [0; SIDES],
            position: BASE_POSITIONS,
            ps: [
                [150.0, 120.0, 90.0, 80.0, 70.0, 60.0],
                [140.0, 130.0, 110.0, 100.0, 90.0, 80.0],
            ],
            pp: [
                [10.0, 20.0, 30.0, 40.0, 50.0, 60.0],
                [11.0, 21.0, 31.0, 41.0, 51.0, 61.0],
            ],
            switch_held: [false; SIDES],
        }
    }

    fn apply(&mut self, masks: [u32; SIDES]) {
        for side in 0..SIDES {
            let mask = masks[side] & INPUT_MASK;
            if mask & UP != 0 {
                self.position[side].1 -= 1.0;
            }
            if mask & DOWN != 0 {
                self.position[side].1 += 1.0;
            }
            if mask & LEFT != 0 {
                self.position[side].0 -= 1.0;
            }
            if mask & RIGHT != 0 {
                self.position[side].0 += 1.0;
            }
            let switching = mask & SWITCH != 0;
            if switching && !self.switch_held[side] {
                self.actual[side] = (self.actual[side] + 1) % NET_STATE_PARTY_SLOTS;
            }
            self.switch_held[side] = switching;
            if mask & ATTACK != 0 {
                let active = self.actual[side];
                self.pp[side][active] = (self.pp[side][active] - 1.0).max(0.0);
                self.ps[side][active] = (self.ps[side][active] - 2.0).max(0.0);
            }
        }
    }

    fn net_state(&self) -> NetState {
        NetState {
            object_count: 17,
            object_sum: 4_242,
            pokemon1: Some(self.position[0]),
            pokemon2: Some(self.position[1]),
            actual1: self.actual[0] as f64,
            actual2: self.actual[1] as f64,
            ps1: self.ps[0],
            ps2: self.ps[1],
            pp1: self.pp[0],
            pp2: self.pp[1],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Wire {
    Input { frame: u32, mask: u32 },
    Sum(Vec<u8>),
}

struct Network {
    queues: [Vec<(u32, Wire)>; SIDES],
}

impl Network {
    fn new() -> Self {
        Self { queues: [Vec::new(), Vec::new()] }
    }

    fn send(&mut self, to: usize, at: u32, wire: Wire) {
        self.queues[to].push((at, wire));
    }

    fn take(&mut self, to: usize, now: u32) -> Vec<Wire> {
        let mut due = Vec::new();
        let mut pending = Vec::new();
        for (at, wire) in self.queues[to].drain(..) {
            if at <= now {
                due.push((at, wire));
            } else {
                pending.push((at, wire));
            }
        }
        due.sort_by_key(|(at, _)| *at);
        self.queues[to] = pending;
        due.into_iter().map(|(_, wire)| wire).collect()
    }
}

struct Client {
    side: usize,
    sync: BattleSync,
    world: World,
    local_sums: Vec<(u32, u32)>,
    peer_sums: Vec<(u32, u32)>,
    peer_order: Vec<u32>,
    first_desync: Option<u32>,
}

impl Client {
    fn new(side: usize, delay: i32) -> Self {
        Self {
            side,
            sync: BattleSync::new(EPOCH, delay),
            world: World::new(),
            local_sums: Vec::new(),
            peer_sums: Vec::new(),
            peer_order: Vec::new(),
            first_desync: None,
        }
    }

    fn note_desync(&mut self, frame: u32, detected: bool) {
        if detected && self.first_desync.is_none() {
            self.first_desync = Some(frame);
        }
    }

    fn deliver(&mut self, wires: Vec<Wire>) {
        for wire in wires {
            match wire {
                Wire::Input { frame, mask } => self.sync.push_remote(frame, mask),
                Wire::Sum(payload) => {
                    let (frame, checksum) = parse_sum_frame(&payload).expect("valid SUM payload");
                    self.peer_order.push(frame);
                    let was_desync = self.sync.desync();
                    let detected = self.sync.receive_checksum(frame, checksum);
                    self.peer_sums.push((frame, checksum));
                    self.note_desync(frame, detected && !was_desync);
                },
            }
        }
    }
}

fn start(clients: &[Client; SIDES], network: &mut Network) {
    for (index, client) in clients.iter().enumerate() {
        let to = 1 - index;
        for input in client.sync.initial_inputs() {
            network.send(to, 1, Wire::Input { frame: input.frame, mask: input.mask });
        }
    }
}

fn tick(clients: &mut [Client; SIDES], network: &mut Network, frame: u32) -> [FrameStep; SIDES] {
    for index in 0..SIDES {
        let wires = network.take(index, frame);
        clients[index].deliver(wires);
    }

    let steps = [
        clients[0].sync.begin_frame(sample(0, frame)),
        clients[1].sync.begin_frame(sample(1, frame)),
    ];

    for index in 0..SIDES {
        if !steps[index].remote_ready {
            clients[index].sync.mark_stall();
        }
    }

    let masks = [steps[0].local_mask, steps[1].local_mask];
    for client in clients.iter_mut() {
        client.world.apply(masks);
    }

    for index in 0..SIDES {
        let to = 1 - index;
        if let Some(outgoing) = steps[index].outgoing {
            network.send(to, frame + 1, Wire::Input { frame: outgoing.frame, mask: outgoing.mask });
        }
        for extra in &steps[index].extra_inputs {
            network.send(to, frame + 1, Wire::Input { frame: extra.frame, mask: extra.mask });
        }
    }

    steps
}

fn record_checksums(clients: &mut [Client; SIDES], steps: &[FrameStep; SIDES], frame: u32) -> [Option<Vec<u8>>; SIDES] {
    let mut payloads = [None, None];
    for index in 0..SIDES {
        if !steps[index].checksum_due {
            continue
        }
        let state = clients[index].world.net_state();
        let text = state.text();
        let checksum = state.checksum();
        assert_eq!(text, net_state_text(&state));
        assert_eq!(checksum, net_checksum(&state));
        assert_eq!(checksum, checksum_text(text.as_bytes()));
        clients[index].local_sums.push((frame, checksum));
        let was_desync = clients[index].sync.desync();
        let detected = clients[index].sync.store_checksum(frame, checksum);
        clients[index].note_desync(frame, detected && !was_desync);
        let payload = BattleSync::checksum_payload(frame, checksum).into_bytes();
        assert_eq!(parse_sum_frame(&payload), Ok((frame, checksum)));
        payloads[index] = Some(payload);
    }
    payloads
}

fn deliver_pending(clients: &mut [Client; SIDES], network: &mut Network, now: u32) {
    for index in 0..SIDES {
        let wires = network.take(index, now);
        clients[index].deliver(wires);
    }
}

fn sums_by_frame(sums: &[(u32, u32)]) -> BTreeMap<u32, u32> {
    sums.iter().copied().collect()
}

#[test]
fn two_clients_keep_the_real_netstate_in_lockstep_for_600_frames() {
    let delay = 5;
    let delay_frames = delay as u32;
    let mut clients = [Client::new(0, delay), Client::new(1, delay)];
    let mut network = Network::new();
    start(&clients, &mut network);

    for frame in 1..=FRAMES {
        let steps = tick(&mut clients, &mut network, frame);
        let applied_through = frame.saturating_sub(delay_frames);

        for index in 0..SIDES {
            let side = clients[index].side;
            let peer = 1 - side;
            let step = &steps[index];
            let expected_local = if frame > delay_frames { sample(side, frame - delay_frames) } else { 0 };
            let expected_remote = if frame > delay_frames { sample(peer, frame - delay_frames) } else { 0 };

            assert_eq!(step.frame, frame);
            assert_eq!(step.local_mask, expected_local, "local side {side} frame {frame}");
            assert!(step.remote_ready, "remote input missing for side {side} frame {frame}");
            assert_eq!(step.remote_mask, expected_remote, "remote side {side} frame {frame}");
            assert_eq!(step.delay, delay);
            assert_eq!(step.seed, frame_seed(EPOCH, frame));
            assert_eq!(step.checksum_due, frame % CHECKSUM_INTERVAL == 0);
            assert!(!step.leave);
            assert_eq!(
                step.outgoing,
                Some(OutgoingInput { frame: frame + delay_frames, mask: sample(side, frame) })
            );
            assert!(step.extra_inputs.is_empty());
        }

        let state1 = clients[0].world.net_state();
        let state2 = clients[1].world.net_state();
        assert_eq!(state1, state2, "clients diverged at frame {frame}");
        assert_eq!(state1.text(), net_state_text(&state2));
        if frame == 150 + delay_frames {
            let text = state1.text();
            assert!(text.contains(" A=1,0 PS="), "frame {frame}: {text}");
            assert!(text.contains(" PP=200,80"), "frame {frame}: {text}");
        }
        if frame == 180 + delay_frames {
            assert!(state1.text().contains(" A=1,1 PS="), "frame {frame}: {}", state1.text());
        }
        assert_eq!(state1.actual1, expected_active(0, applied_through) as f64, "P1 active frame {frame}");
        assert_eq!(state1.actual2, expected_active(1, applied_through) as f64, "P2 active frame {frame}");
        assert_eq!(state1.pokemon1, Some(expected_position(0, applied_through)), "P1 position frame {frame}");
        assert_eq!(state1.pokemon2, Some(expected_position(1, applied_through)), "P2 position frame {frame}");

        let payloads = record_checksums(&mut clients, &steps, frame);
        for (index, payload) in payloads.into_iter().enumerate() {
            if let Some(payload) = payload {
                network.send(1 - index, frame + 1, Wire::Sum(payload));
            }
        }
    }

    deliver_pending(&mut clients, &mut network, FRAMES + 1);

    for client in &clients {
        assert_eq!(client.sync.frame(), FRAMES);
        assert!(!client.sync.desync());
        assert!(client.sync.bye_frame().is_none());
        assert_eq!(client.local_sums.len(), (FRAMES / CHECKSUM_INTERVAL) as usize);
        assert_eq!(client.peer_sums.len(), (FRAMES / CHECKSUM_INTERVAL) as usize);
        assert_eq!(sums_by_frame(&client.local_sums), sums_by_frame(&client.peer_sums));
    }

    assert_eq!(clients[0].world.actual, [3, 3]);
    assert_eq!(clients[0].world.actual, clients[1].world.actual);
    assert_ne!(clients[0].world.position, BASE_POSITIONS);
    assert_eq!(clients[0].world.position, clients[1].world.position);
    let checksums: BTreeSet<u32> = sums_by_frame(&clients[0].local_sums).into_values().collect();
    assert!(checksums.len() > 1);
}

#[test]
fn two_clients_pair_sum_frames_when_checksums_arrive_out_of_order() {
    let delay = 3;
    let mut clients = [Client::new(0, delay), Client::new(1, delay)];
    let mut network = Network::new();
    start(&clients, &mut network);
    let mut pending: [Vec<Vec<u8>>; SIDES] = [Vec::new(), Vec::new()];

    for frame in 1..=FRAMES {
        let steps = tick(&mut clients, &mut network, frame);
        let payloads = record_checksums(&mut clients, &steps, frame);
        for (index, payload) in payloads.into_iter().enumerate() {
            if let Some(payload) = payload {
                pending[index].push(payload);
            }
        }

        if frame % (CHECKSUM_INTERVAL * 8) == 0 {
            for (index, payloads) in pending.iter_mut().enumerate() {
                for payload in payloads.drain(..).rev() {
                    network.send(1 - index, frame + 1, Wire::Sum(payload));
                }
            }
        }
    }

    for (index, payloads) in pending.iter_mut().enumerate() {
        for payload in payloads.drain(..).rev() {
            network.send(1 - index, FRAMES + 1, Wire::Sum(payload));
        }
    }
    deliver_pending(&mut clients, &mut network, FRAMES + 1);

    for client in &clients {
        assert_eq!(client.sync.frame(), FRAMES);
        assert!(!client.sync.desync());
        assert_eq!(client.peer_order.len(), (FRAMES / CHECKSUM_INTERVAL) as usize);
        assert_eq!(
            &client.peer_order[..8],
            &[240, 210, 180, 150, 120, 90, 60, 30],
            "side {} did not receive the first checksum batch in reverse order",
            client.side
        );
        assert_eq!(
            &client.peer_order[8..16],
            &[480, 450, 420, 390, 360, 330, 300, 270],
            "side {} did not receive the second checksum batch in reverse order",
            client.side
        );
        assert_eq!(sums_by_frame(&client.local_sums), sums_by_frame(&client.peer_sums));
    }
}

#[test]
fn two_clients_report_the_first_mismatched_netstate_sum_frame() {
    let delay = 3;
    let mut clients = [Client::new(0, delay), Client::new(1, delay)];
    let mut network = Network::new();
    start(&clients, &mut network);

    for frame in 1..=FRAMES {
        let steps = tick(&mut clients, &mut network, frame);
        if frame == DIVERGENT_FRAME {
            clients[0].world.actual[0] = (clients[0].world.actual[0] + 1) % NET_STATE_PARTY_SLOTS;
            assert_ne!(clients[0].world, clients[1].world);
        }
        let payloads = record_checksums(&mut clients, &steps, frame);
        for (index, payload) in payloads.into_iter().enumerate() {
            if let Some(payload) = payload {
                network.send(1 - index, frame + 1, Wire::Sum(payload));
            }
        }
    }

    deliver_pending(&mut clients, &mut network, FRAMES + 1);

    for client in &clients {
        assert_eq!(client.sync.frame(), FRAMES);
        assert!(client.sync.desync());
        assert_eq!(client.first_desync, Some(DIVERGENT_FRAME), "side {}", client.side);
        let local = sums_by_frame(&client.local_sums);
        let peer = sums_by_frame(&client.peer_sums);
        assert_eq!(local.len(), (FRAMES / CHECKSUM_INTERVAL) as usize);
        assert_eq!(peer.len(), (FRAMES / CHECKSUM_INTERVAL) as usize);
        for (&frame, &checksum) in &local {
            if frame < DIVERGENT_FRAME {
                assert_eq!(peer[&frame], checksum, "side {} frame {frame} diverged early", client.side);
            } else {
                assert_ne!(peer[&frame], checksum, "side {} frame {frame} did not diverge", client.side);
            }
        }
    }

    assert_eq!(clients[0].local_sums.iter().filter(|(frame, _)| *frame < DIVERGENT_FRAME).count(), 10);
    assert_eq!(clients[1].local_sums.iter().filter(|(frame, _)| *frame < DIVERGENT_FRAME).count(), 10);
}
