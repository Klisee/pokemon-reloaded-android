#[allow(unused_assignments)]
#[path = "../src/battle_sync.rs"]
mod battle_sync;

use battle_sync::{
    BattleSync, CHECKSUM_INTERVAL, FrameStep, INPUT_MASK, MIN_DELAY, OutgoingInput, STALL_BIT, frame_seed,
};
use reloaded_cleanroom::z063_sec::{BattleGuard, ChecksumDecision, checksum_text, parse_bye_frame, parse_sum_frame};

const EPOCH: i32 = 1_000_003;
const FRAMES: u32 = 600;
const DIVERGENT_FRAME: u32 = 330;

fn sample(side: u32, frame: u32) -> u32 {
    let mut value = frame.wrapping_mul(0x9e37_79b1) ^ side.wrapping_mul(0x85eb_ca77);
    value ^= value >> 15;
    value = value.wrapping_mul(0x2c1b_3c6d);
    value ^= value >> 12;
    value & INPUT_MASK
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wire {
    Input(u32, u32),
    Checksum(u32, u32),
    Bye(i32, u32),
}

struct Network {
    queues: [Vec<(u32, Wire)>; 2],
    held: [Vec<Wire>; 2],
    paused: [bool; 2],
}

impl Network {
    fn new() -> Self {
        Self { queues: [Vec::new(), Vec::new()], held: [Vec::new(), Vec::new()], paused: [false, false] }
    }

    fn send(&mut self, to: usize, at: u32, wire: Wire) {
        if self.paused[to] {
            self.held[to].push(wire);
        } else {
            self.queues[to].push((at, wire));
        }
    }

    fn pause(&mut self, to: usize) {
        self.paused[to] = true;
    }

    fn resume(&mut self, to: usize, at: u32) {
        self.paused[to] = false;
        for wire in self.held[to].drain(..) {
            self.queues[to].push((at, wire));
        }
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
    side: u32,
    sync: BattleSync,
    first_desync: Option<u32>,
    bye_decisions: Vec<(i32, u32, bool)>,
}

impl Client {
    fn new(side: u32, delay: i32) -> Self {
        Self { side, sync: BattleSync::new(EPOCH, delay), first_desync: None, bye_decisions: Vec::new() }
    }

    fn note_desync(&mut self, frame: u32, detected: bool) {
        if detected && self.first_desync.is_none() {
            self.first_desync = Some(frame);
        }
    }

    fn deliver(&mut self, wires: Vec<Wire>) {
        for wire in wires {
            match wire {
                Wire::Input(frame, mask) => self.sync.push_remote(frame, mask),
                Wire::Checksum(frame, checksum) => {
                    let was_desync = self.sync.desync();
                    let detected = self.sync.receive_checksum(frame, checksum);
                    self.note_desync(frame, detected && !was_desync);
                },
                Wire::Bye(epoch, frame) => {
                    let accepted = self.sync.receive_bye(epoch, frame);
                    self.bye_decisions.push((epoch, frame, accepted));
                },
            }
        }
    }
}

fn start(clients: &[Client; 2], network: &mut Network) {
    for (index, client) in clients.iter().enumerate() {
        let to = 1 - index;
        for input in client.sync.initial_inputs() {
            network.send(to, 1, Wire::Input(input.frame, input.mask));
        }
    }
}

fn tick(clients: &mut [Client; 2], network: &mut Network, frame: u32) -> [FrameStep; 2] {
    for index in 0..2 {
        let wires = network.take(index, frame);
        clients[index].deliver(wires);
    }

    let steps = [clients[0].sync.begin_frame(sample(1, frame)), clients[1].sync.begin_frame(sample(2, frame))];

    for index in 0..2 {
        if !steps[index].remote_ready {
            clients[index].sync.mark_stall();
        }
    }

    for index in 0..2 {
        let to = 1 - index;
        if let Some(outgoing) = steps[index].outgoing {
            network.send(to, frame + 1, Wire::Input(outgoing.frame, outgoing.mask));
        }
        for extra in &steps[index].extra_inputs {
            network.send(to, frame + 1, Wire::Input(extra.frame, extra.mask));
        }
    }

    steps
}

fn state_text(side: u32, step: &FrameStep, divergent: bool) -> String {
    let (p1, p2) = if side == 1 {
        (step.local_mask & INPUT_MASK, step.remote_mask & INPUT_MASK)
    } else {
        (step.remote_mask & INPUT_MASK, step.local_mask & INPUT_MASK)
    };
    let mut text = format!("{EPOCH}:{}:{}:{p1:03x}:{p2:03x}", step.frame, step.seed);
    if divergent {
        text.push_str("|divergent");
    }
    text
}

#[test]
fn two_clients_lockstep_for_600_frames_with_delayed_inputs() {
    let delay = 4;
    let delay_frames = delay as u32;
    let mut clients = [Client::new(1, delay), Client::new(2, delay)];
    let mut network = Network::new();
    start(&clients, &mut network);

    for frame in 1..=FRAMES {
        let steps = tick(&mut clients, &mut network, frame);
        for index in 0..2 {
            let step = &steps[index];
            let side = clients[index].side;
            let peer = 3 - side;
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
            assert_eq!(step.outgoing, Some(OutgoingInput { frame: frame + delay_frames, mask: sample(side, frame) }));
            assert!(step.extra_inputs.is_empty());
        }
    }

    assert_eq!(clients[0].sync.frame(), FRAMES);
    assert_eq!(clients[1].sync.frame(), FRAMES);
    assert!(clients.iter().all(|client| !client.sync.desync()));
    assert!(clients.iter().all(|client| client.sync.bye_frame().is_none()));
}

#[test]
fn two_clients_wait_out_a_stall_and_recover_within_600_frames() {
    let delay = 2;
    let mut clients = [Client::new(1, delay), Client::new(2, delay)];
    let mut network = Network::new();
    start(&clients, &mut network);

    let pause_at = 58;
    let missing_from = 60;
    let missing_through = 119;
    let resume_at = 120;
    let mut missing = [0u32; 2];
    let mut saw_stall_bit = false;

    for frame in 1..=FRAMES {
        if frame == pause_at {
            network.pause(0);
            network.pause(1);
        }
        if frame == resume_at {
            network.resume(0, resume_at);
            network.resume(1, resume_at);
        }

        let steps = tick(&mut clients, &mut network, frame);

        if (missing_from..=missing_through).contains(&frame) {
            for index in 0..2 {
                assert!(!steps[index].remote_ready, "side {} frame {frame} received a held input", clients[index].side);
                missing[index] += 1;
            }
        }

        if frame == resume_at {
            for index in 0..2 {
                assert!(steps[index].remote_ready, "side {} did not resume at {resume_at}", clients[index].side);
                assert_eq!(steps[index].delay, 3);
                assert_eq!(steps[index].extra_inputs, vec![OutgoingInput { frame: 123, mask: 0 }]);
            }
        }

        if frame == 180 {
            assert_eq!(steps[0].delay, 3);
            assert_eq!(steps[1].delay, 3);
        }
        if frame == 480 {
            assert_eq!(steps[0].delay, MIN_DELAY);
            assert_eq!(steps[1].delay, MIN_DELAY);
        }

        saw_stall_bit |= steps.iter().any(|step| {
            step.outgoing.is_some_and(|input| input.mask & STALL_BIT != 0)
                || step.extra_inputs.iter().any(|input| input.mask & STALL_BIT != 0)
        });
    }

    assert_eq!(missing, [60, 60]);
    assert!(saw_stall_bit);
    assert_eq!(clients[0].sync.frame(), FRAMES);
    assert_eq!(clients[1].sync.frame(), FRAMES);
    assert!(clients.iter().all(|client| !client.sync.desync()));
    assert!(clients.iter().all(|client| client.sync.bye_frame().is_none()));
}

#[test]
fn two_clients_detect_the_first_divergent_checksum_during_600_frames() {
    let delay = 2;
    let mut clients = [Client::new(1, delay), Client::new(2, delay)];
    let mut network = Network::new();
    start(&clients, &mut network);
    let mut sums: [Vec<(u32, u32)>; 2] = [Vec::new(), Vec::new()];

    for frame in 1..=FRAMES {
        let steps = tick(&mut clients, &mut network, frame);
        if frame % CHECKSUM_INTERVAL != 0 {
            continue;
        }

        let mut frame_sums = [0u32; 2];
        for index in 0..2 {
            let side = clients[index].side;
            let divergent = side == 1 && frame >= DIVERGENT_FRAME;
            let text = state_text(side, &steps[index], divergent);
            let checksum = checksum_text(text.as_bytes());
            assert_eq!(checksum_text(text.as_bytes()), checksum);
            assert_eq!(
                parse_sum_frame(BattleSync::checksum_payload(frame, checksum).as_bytes()),
                Ok((frame, checksum))
            );
            let was_desync = clients[index].sync.desync();
            let detected = clients[index].sync.store_checksum(frame, checksum);
            clients[index].note_desync(frame, detected && !was_desync);
            frame_sums[index] = checksum;
            sums[index].push((frame, checksum));
        }

        network.send(1, frame + 1, Wire::Checksum(frame, frame_sums[0]));
        network.send(0, frame + 1, Wire::Checksum(frame, frame_sums[1]));
    }

    for index in 0..2 {
        let wires = network.take(index, FRAMES + 1);
        clients[index].deliver(wires);
    }

    for client in &clients {
        assert_eq!(client.first_desync, Some(DIVERGENT_FRAME));
        assert!(client.sync.desync());
    }
    for (local, peer) in sums[0].iter().zip(&sums[1]) {
        assert_eq!(local.0, peer.0);
        if local.0 < DIVERGENT_FRAME {
            assert_eq!(local.1, peer.1, "checksums diverged early at frame {}", local.0);
        } else {
            assert_ne!(local.1, peer.1, "checksums did not diverge at frame {}", local.0);
        }
    }

    let mut guard = BattleGuard::new(EPOCH);
    let mut guard_first = None;
    for (local, peer) in sums[0].iter().zip(&sums[1]) {
        assert_eq!(local.0, peer.0);
        guard.advance_local_frame(local.0);
        assert_eq!(guard.note_local_checksum(local.0, local.1), Ok(None));
        match guard.receive_checksum(EPOCH, local.0 as i32, i64::from(peer.1)) {
            Ok(ChecksumDecision::Accepted) => assert!(local.0 < DIVERGENT_FRAME),
            Ok(ChecksumDecision::Desync(desync)) => {
                assert_eq!(desync.frame, local.0);
                assert_eq!(desync.local, local.1);
                assert_eq!(desync.peer, peer.1);
                guard_first.get_or_insert(local.0);
            },
            other => panic!("unexpected guard decision at frame {}: {other:?}", local.0),
        }
    }
    assert_eq!(guard_first, Some(DIVERGENT_FRAME));
    assert!(guard.desync());
}

#[test]
fn two_clients_share_the_rng_seed_for_all_600_frames() {
    let delay = 2;
    let mut clients = [Client::new(1, delay), Client::new(2, delay)];
    let mut network = Network::new();
    start(&clients, &mut network);
    let mut seeds: [Vec<i32>; 2] = [Vec::new(), Vec::new()];

    for frame in 1..=FRAMES {
        let steps = tick(&mut clients, &mut network, frame);
        let expected = (EPOCH % 65_536) * 16_384 + (frame % 16_384) as i32;
        for index in 0..2 {
            assert_eq!(steps[index].seed, expected, "side {} frame {frame}", clients[index].side);
            assert_eq!(steps[index].seed, frame_seed(EPOCH, frame));
            seeds[index].push(steps[index].seed);
        }
        assert_eq!(steps[0].seed, steps[1].seed, "frame {frame}");
    }

    assert_eq!(seeds[0], seeds[1]);
    assert_eq!(seeds[0].len(), FRAMES as usize);
    assert_eq!(seeds[0].first().copied(), Some(frame_seed(EPOCH, 1)));
    assert_eq!(seeds[0].last().copied(), Some(frame_seed(EPOCH, FRAMES)));
    assert_ne!(seeds[0][0], frame_seed(EPOCH + 1, 1));
}

#[test]
fn two_clients_honor_bye_after_600_frames() {
    let delay = 3;
    let mut clients = [Client::new(1, delay), Client::new(2, delay)];
    let mut network = Network::new();
    start(&clients, &mut network);

    for frame in 1..FRAMES {
        let steps = tick(&mut clients, &mut network, frame);
        assert!(!steps[0].leave);
        assert!(!steps[1].leave);
    }

    let payload = clients[0].sync.bye_payload(FRAMES);
    assert_eq!(parse_bye_frame(payload.as_bytes()), Ok((EPOCH, FRAMES)));
    network.send(1, FRAMES, Wire::Bye(EPOCH + 1, FRAMES));
    network.send(1, FRAMES, Wire::Bye(EPOCH, FRAMES));

    let steps = tick(&mut clients, &mut network, FRAMES);
    assert!(!steps[0].leave);
    assert!(!steps[1].leave);
    assert_eq!(clients[1].bye_decisions, vec![(EPOCH + 1, FRAMES, false), (EPOCH, FRAMES, true)]);
    assert_eq!(clients[1].sync.bye_frame(), Some(FRAMES));
    assert!(!clients[1].sync.should_leave());

    let steps = tick(&mut clients, &mut network, FRAMES + 1);
    assert!(!steps[0].leave);
    assert!(steps[1].leave);
    assert!(steps[1].outgoing.is_none());
    assert!(steps[1].extra_inputs.is_empty());
    assert!(clients[1].sync.should_leave());
}
