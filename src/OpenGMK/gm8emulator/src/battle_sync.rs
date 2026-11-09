//! Deterministic lockstep state for online battles.
//!
//! This mirrors the frame/checksum rules used by the GML `NetFrameBegin`,
//! `NetBattleBegin`, `NetChecksumSend`, `NetBattleMessages` and `NetDelayAdapt`
//! scripts without injecting or running any GML:
//!
//! - the first `delay` frames are seeded with empty inputs;
//! - local input sampled on frame `f` is applied on frame `f + delay`;
//! - every frame reseeds the RNG with `(epoch mod 65536) * 16384 + (frame mod 16384)`;
//! - a checksum is exchanged every 30 frames and kept in 64-frame buffers;
//! - `BYE <epoch> <frame>` ends the battle once the local frame passes it;
//! - the delay adapts in the 2..=10 range from stalled-frame statistics.
//!
//! The type is pure state: callers own sampling, simulation and transport.

use std::collections::HashMap;

/// Number of delayed input slots addressed by `frame mod 256`.
pub const INPUT_SLOTS: usize = 256;
/// Number of checksum slots addressed by `(frame div 30) mod 64`.
pub const CHECKSUM_SLOTS: usize = 64;
/// Frames between checksum messages.
pub const CHECKSUM_INTERVAL: u32 = 30;
/// Frames between delay-adaptation decisions.
pub const ADAPT_INTERVAL: u32 = 60;
/// Input bit used to mark a stalled frame.
pub const STALL_BIT: u32 = 1 << 15;
/// Bits that belong to the 11 battle buttons.
pub const INPUT_MASK: u32 = 0x07ff;
/// Initial and adaptive delay bounds.
pub const MIN_DELAY: i32 = 2;
pub const MAX_DELAY: i32 = 10;
/// Safety bound for out-of-order remote inputs.
const MAX_REMOTE_INPUTS: usize = 1024;

/// An input line that should be written to the peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutgoingInput {
    pub frame: u32,
    pub mask: u32,
}

/// Everything the caller needs to simulate one lockstep frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameStep {
    pub frame: u32,
    /// Mask stored for this frame, including the stall bit.
    pub local_mask: u32,
    /// Remote mask for this frame (stall bit included).
    pub remote_mask: u32,
    /// False when the peer input had not arrived yet; `remote_mask` is then zero.
    pub remote_ready: bool,
    /// Seed to install in the game RNG for this frame.
    pub seed: i32,
    /// Effective input delay after this frame's adaptation.
    pub delay: i32,
    /// New delayed input to send, if the send window advanced.
    pub outgoing: Option<OutgoingInput>,
    /// Extra empty inputs sent when the delay grew.
    pub extra_inputs: Vec<OutgoingInput>,
    /// True when `BYE` asked to leave before this frame.
    pub leave: bool,
    /// True on frames that carry a checksum.
    pub checksum_due: bool,
}

/// `median(2, delay, 10)` from `NetDelayFrames` / the guest `TEAM` handler.
pub fn initial_delay(offered: i32) -> i32 {
    offered.clamp(MIN_DELAY, MAX_DELAY)
}

/// `NetDelayFrames`: median of 2, the RTT-derived frame count and 10.
pub fn delay_from_rtt(rtt_ms: i32, room_speed: i32) -> i32 {
    if rtt_ms < 0 {
        return MIN_DELAY
    }
    let ms = 1000.0 / room_speed.max(1) as f64;
    let frames = ((rtt_ms as f64 / 2.0 + 15.0) / ms).ceil() as i32 + 1;
    initial_delay(frames)
}

/// Deterministic per-frame seed from `NetFrameBegin`.
pub fn frame_seed(epoch: i32, frame: u32) -> i32 {
    (epoch % 65_536) * 16_384 + (frame % 16_384) as i32
}

/// Checksum slot used by `NetChecksumSend`/`NetBattleMessages`.
pub fn checksum_slot(frame: u32) -> usize {
    ((frame / CHECKSUM_INTERVAL) % CHECKSUM_SLOTS as u32) as usize
}

/// Lockstep battle state.
pub struct BattleSync {
    epoch: i32,
    frame: u32,
    delay: i32,
    min_delay: i32,
    last_sent: u32,
    mine: [u32; INPUT_SLOTS],
    remote: HashMap<u32, u32>,
    local_sum_frame: [Option<u32>; CHECKSUM_SLOTS],
    local_sum_value: [u32; CHECKSUM_SLOTS],
    peer_sum_frame: [Option<u32>; CHECKSUM_SLOTS],
    peer_sum_value: [u32; CHECKSUM_SLOTS],
    bye_frame: Option<u32>,
    adapt_count: u32,
    adapt_queue: u32,
    stall: bool,
    desync: bool,
}

impl BattleSync {
    /// Start a battle at frame 0. `delay` is clamped with [`initial_delay`].
    pub fn new(epoch: i32, delay: i32) -> Self {
        let delay = initial_delay(delay);
        let mut sync = Self {
            epoch,
            frame: 0,
            delay,
            min_delay: delay,
            last_sent: 0,
            mine: [0; INPUT_SLOTS],
            remote: HashMap::new(),
            local_sum_frame: [None; CHECKSUM_SLOTS],
            local_sum_value: [0; CHECKSUM_SLOTS],
            peer_sum_frame: [None; CHECKSUM_SLOTS],
            peer_sum_value: [0; CHECKSUM_SLOTS],
            bye_frame: None,
            adapt_count: 0,
            adapt_queue: 0,
            stall: false,
            desync: false,
        };
        sync.seed_initial_inputs();
        sync
    }

    fn seed_initial_inputs(&mut self) {
        for frame in 1..=self.delay.max(0) as u32 {
            self.mine[frame as usize % INPUT_SLOTS] = 0;
        }
        self.last_sent = self.delay.max(0) as u32;
    }

    /// Frames `1..=delay` with an empty mask, as sent by `NetBattleBegin`.
    pub fn initial_inputs(&self) -> Vec<OutgoingInput> {
        (1..=self.delay.max(0) as u32)
            .map(|frame| OutgoingInput { frame, mask: 0 })
            .collect()
    }

    pub fn epoch(&self) -> i32 {
        self.epoch
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    pub fn delay(&self) -> i32 {
        self.delay
    }

    pub fn min_delay(&self) -> i32 {
        self.min_delay
    }

    pub fn desync(&self) -> bool {
        self.desync
    }

    pub fn bye_frame(&self) -> Option<u32> {
        self.bye_frame
    }

    /// Mark the next sampled input as a stall (`NetFrameBegin` sets bit 15).
    pub fn mark_stall(&mut self) {
        self.stall = true;
    }

    /// Store an input received from the peer.
    pub fn push_remote(&mut self, frame: u32, mask: u32) {
        if self.remote.len() >= MAX_REMOTE_INPUTS {
            self.remote.retain(|queued, _| *queued >= self.frame);
            while self.remote.len() >= MAX_REMOTE_INPUTS {
                let Some(key) = self.remote.keys().next().copied() else { break };
                self.remote.remove(&key);
            }
        }
        self.remote.insert(frame, mask);
    }

    /// True when the peer input for `frame` is waiting in the queue.
    pub fn remote_pending(&self, frame: u32) -> bool {
        self.remote.contains_key(&frame)
    }

    /// Number of peer inputs queued for frames that have not been simulated yet.
    pub fn remote_queued(&self) -> usize {
        self.remote.keys().filter(|frame| **frame > self.frame).count()
    }

    /// Advance to the next frame and plan its inputs.
    pub fn begin_frame(&mut self, local_sample: u32) -> FrameStep {
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        let leave = self.bye_frame.is_some_and(|bye| frame > bye);

        let mut outgoing = None;
        if !leave {
            let target = frame.saturating_add(self.delay.max(0) as u32);
            if target > self.last_sent {
                let mut mask = local_sample & (INPUT_MASK | STALL_BIT);
                if self.stall {
                    mask |= STALL_BIT;
                }
                self.mine[target as usize % INPUT_SLOTS] = mask;
                self.last_sent = target;
                outgoing = Some(OutgoingInput { frame: target, mask });
            }
        }
        self.stall = false;

        let local_mask = self.mine[frame as usize % INPUT_SLOTS] & (INPUT_MASK | STALL_BIT);
        let remote = self.remote.remove(&frame);
        let remote_ready = remote.is_some();
        let remote_mask = remote.unwrap_or(0) & (INPUT_MASK | STALL_BIT);

        let seed = frame_seed(self.epoch, frame);
        let extra_inputs = if leave { Vec::new() } else { self.adapt(frame, local_mask, remote_mask) };

        FrameStep {
            frame,
            local_mask,
            remote_mask,
            remote_ready,
            seed,
            delay: self.delay,
            outgoing,
            extra_inputs,
            leave,
            checksum_due: frame % CHECKSUM_INTERVAL == 0,
        }
    }

    /// `NetDelayAdapt`: grow after six stall bits in 60 frames, shrink after
    /// five quiet windows while above the initial delay.
    fn adapt(&mut self, frame: u32, local_mask: u32, remote_mask: u32) -> Vec<OutgoingInput> {
        if local_mask & STALL_BIT != 0 || remote_mask & STALL_BIT != 0 {
            self.adapt_count += 1;
        }
        if frame % ADAPT_INTERVAL != 0 {
            return Vec::new()
        }

        let previous = self.delay;
        if self.adapt_count >= 6 {
            self.delay = (self.delay + 1).min(MAX_DELAY);
            self.adapt_queue = 0;
        } else if self.adapt_count <= 1 {
            self.adapt_queue += 1;
            if self.adapt_queue >= 5 && self.delay > self.min_delay {
                self.delay -= 1;
                self.adapt_queue = 0;
            }
        } else {
            self.adapt_queue = 0;
        }
        self.adapt_count = 0;

        if self.delay <= previous {
            return Vec::new()
        }

        let end = frame.saturating_add(self.delay as u32);
        let mut extra = Vec::new();
        for target in (self.last_sent + 1)..=end {
            self.mine[target as usize % INPUT_SLOTS] = 0;
            extra.push(OutgoingInput { frame: target, mask: 0 });
        }
        self.last_sent = self.last_sent.max(end);
        extra
    }

    /// Store the local checksum and compare it with an already-received peer value.
    /// Returns true when it detects a desynchronization.
    pub fn store_checksum(&mut self, frame: u32, checksum: u32) -> bool {
        let slot = checksum_slot(frame);
        self.local_sum_frame[slot] = Some(frame);
        self.local_sum_value[slot] = checksum;
        if self.peer_sum_frame[slot] == Some(frame) && self.peer_sum_value[slot] != checksum {
            self.desync = true;
        }
        self.desync
    }

    /// Store a peer checksum and compare it with a local value already sent.
    /// Returns true when it detects a desynchronization.
    pub fn receive_checksum(&mut self, frame: u32, checksum: u32) -> bool {
        let slot = checksum_slot(frame);
        self.peer_sum_frame[slot] = Some(frame);
        self.peer_sum_value[slot] = checksum;
        if self.local_sum_frame[slot] == Some(frame) && self.local_sum_value[slot] != checksum {
            self.desync = true;
        }
        self.desync
    }

    /// `NetBattleMessages`: only a `BYE` for this epoch is accepted.
    pub fn receive_bye(&mut self, epoch: i32, frame: u32) -> bool {
        if epoch != self.epoch {
            return false
        }
        self.bye_frame = Some(frame);
        true
    }

    /// Set an already-filtered `BYE` frame.
    pub fn receive_bye_frame(&mut self, frame: u32) {
        self.bye_frame = Some(frame);
    }

    /// True when the local simulation has passed the peer's `BYE` frame.
    pub fn should_leave(&self) -> bool {
        self.bye_frame.is_some_and(|bye| self.frame > bye)
    }

    /// Payload sent by `NetBattleEnd`: `BYE\x1f<epoch> <frame>`.
    pub fn bye_payload(&self, frame: u32) -> String {
        format!("BYE\x1f{} {}", self.epoch, frame)
    }

    /// Payload used by `NetChecksumSend`: `SUM\x1f<frame>\x1e<checksum>`.
    pub fn checksum_payload(frame: u32, checksum: u32) -> String {
        format!("SUM\x1f{frame}\x1e{checksum}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_frames(sync: &mut BattleSync, frames: u32, sample: u32) -> FrameStep {
        let mut step = sync.begin_frame(sample);
        for _ in 1..frames {
            step = sync.begin_frame(sample);
        }
        step
    }

    #[test]
    fn initial_delay_is_the_gml_median() {
        assert_eq!(initial_delay(-4), 2);
        assert_eq!(initial_delay(0), 2);
        assert_eq!(initial_delay(1), 2);
        assert_eq!(initial_delay(2), 2);
        assert_eq!(initial_delay(5), 5);
        assert_eq!(initial_delay(10), 10);
        assert_eq!(initial_delay(11), 10);
        assert_eq!(initial_delay(99), 10);
    }

    #[test]
    fn rtt_delay_matches_net_delay_frames() {
        assert_eq!(delay_from_rtt(-1, 60), 2);
        assert_eq!(delay_from_rtt(0, 60), 2);
        assert_eq!(delay_from_rtt(300, 60), 10);
        assert_eq!(delay_from_rtt(200, 60), 8);
        assert_eq!(delay_from_rtt(100, 0), 2);
        assert_eq!(delay_from_rtt(100_000, 60), 10);
    }

    #[test]
    fn frame_seed_uses_the_gml_formula() {
        assert_eq!(frame_seed(1, 1), 16_385);
        assert_eq!(frame_seed(7, 42), 7 * 16_384 + 42);
        assert_eq!(frame_seed(65_537, 16_385), 16_385);
        assert_eq!(frame_seed(65_536, 16_384), 0);
        assert_eq!(frame_seed(-3, 5), -3 * 16_384 + 5);
    }

    #[test]
    fn initial_inputs_fill_the_delay_window_and_delay_local_samples() {
        let mut sync = BattleSync::new(7, 3);
        assert_eq!(sync.epoch(), 7);
        assert_eq!(sync.delay(), 3);
        assert_eq!(sync.min_delay(), 3);
        assert_eq!(
            sync.initial_inputs(),
            vec![
                OutgoingInput { frame: 1, mask: 0 },
                OutgoingInput { frame: 2, mask: 0 },
                OutgoingInput { frame: 3, mask: 0 },
            ]
        );

        let first = sync.begin_frame(0x01);
        assert_eq!(first.frame, 1);
        assert_eq!(first.local_mask, 0);
        assert!(!first.remote_ready);
        assert_eq!(first.outgoing, Some(OutgoingInput { frame: 4, mask: 0x01 }));
        assert_eq!(first.seed, frame_seed(7, 1));

        let second = sync.begin_frame(0x02);
        assert_eq!(second.outgoing, Some(OutgoingInput { frame: 5, mask: 0x02 }));
        let third = sync.begin_frame(0x04);
        assert_eq!(third.outgoing, Some(OutgoingInput { frame: 6, mask: 0x04 }));

        let fourth = sync.begin_frame(0x08);
        assert_eq!(fourth.frame, 4);
        assert_eq!(fourth.local_mask, 0x01);
        assert_eq!(fourth.outgoing, Some(OutgoingInput { frame: 7, mask: 0x08 }));
        let fifth = sync.begin_frame(0x10);
        assert_eq!(fifth.local_mask, 0x02);
        let sixth = sync.begin_frame(0x20);
        assert_eq!(sixth.local_mask, 0x04);
        let seventh = sync.begin_frame(0x40);
        assert_eq!(seventh.local_mask, 0x08);
    }

    #[test]
    fn remote_inputs_are_consumed_once_and_stall_bits_are_preserved() {
        let mut sync = BattleSync::new(1, 2);
        sync.push_remote(1, 0x1234_8000 | 0x05);
        let step = sync.begin_frame(0);
        assert!(step.remote_ready);
        assert_eq!(step.remote_mask, STALL_BIT | 0x05);
        assert!(!sync.remote_pending(1));

        sync.mark_stall();
        let step = sync.begin_frame(0);
        assert_eq!(step.outgoing.expect("scheduled input").mask & STALL_BIT, STALL_BIT);
        let next = sync.begin_frame(0);
        assert_eq!(next.outgoing.expect("scheduled input").mask & STALL_BIT, 0);
    }

    #[test]
    fn remote_queue_counts_only_future_frames() {
        let mut sync = BattleSync::new(1, 2);
        sync.push_remote(1, 0x01);
        sync.push_remote(5, 0x02);
        assert_eq!(sync.remote_queued(), 2);

        sync.begin_frame(0); // frame 1 consumes its input
        assert_eq!(sync.remote_queued(), 1);

        sync.push_remote(0, 0x03); // stale, already simulated
        assert_eq!(sync.remote_queued(), 1);
        assert!(sync.remote_pending(0));
    }

    #[test]
    fn checksum_slots_and_buffers_detect_desync_in_both_orders() {
        assert_eq!(checksum_slot(0), 0);
        assert_eq!(checksum_slot(30), 1);
        assert_eq!(checksum_slot(29), 0);
        assert_eq!(checksum_slot(30 * 64), 0);
        assert_eq!(checksum_slot(30 * 65), 1);

        let mut peer_first = BattleSync::new(1, 2);
        assert!(!peer_first.receive_checksum(30, 222));
        assert!(peer_first.store_checksum(30, 111));
        assert!(peer_first.desync());

        let mut local_first = BattleSync::new(1, 2);
        assert!(!local_first.store_checksum(30, 111));
        assert!(local_first.receive_checksum(30, 222));
        assert!(local_first.desync());

        let mut equal = BattleSync::new(1, 2);
        assert!(!equal.receive_checksum(30, 111));
        assert!(!equal.store_checksum(30, 111));
        assert!(!equal.desync());
    }

    #[test]
    fn checksum_is_due_every_thirty_frames() {
        let mut sync = BattleSync::new(1, 2);
        for frame in 1..=60 {
            let step = sync.begin_frame(0);
            assert_eq!(step.frame, frame);
            assert_eq!(step.checksum_due, frame % 30 == 0);
        }
    }

    #[test]
    fn bye_is_epoch_scoped_and_stops_after_its_frame() {
        let mut sync = BattleSync::new(7, 2);
        assert!(!sync.receive_bye(6, 3));
        assert_eq!(sync.bye_frame(), None);
        assert!(sync.receive_bye(7, 3));

        assert!(!sync.should_leave());
        assert!(!sync.begin_frame(0).leave); // frame 1
        assert!(!sync.begin_frame(0).leave); // frame 2
        assert!(!sync.begin_frame(0).leave); // frame 3
        assert!(sync.begin_frame(0).leave); // frame 4
        assert!(sync.should_leave());
        assert_eq!(sync.bye_payload(3), "BYE\x1f7 3");
    }

    #[test]
    fn bitmap_bye_frames_are_accepted_without_an_epoch() {
        let mut sync = BattleSync::new(7, 2);
        sync.receive_bye_frame(0);
        assert!(sync.begin_frame(0).leave);
    }

    #[test]
    fn stall_heavy_windows_grow_the_delay_with_catch_up_inputs() {
        let mut sync = BattleSync::new(1, 2);
        let step = run_frames(&mut sync, 60, STALL_BIT);
        assert_eq!(step.frame, 60);
        assert_eq!(step.delay, 3);
        assert_eq!(step.extra_inputs, vec![OutgoingInput { frame: 63, mask: 0 }]);
        assert_eq!(sync.delay(), 3);
        assert_eq!(sync.min_delay(), 2);
    }

    #[test]
    fn delay_never_exceeds_ten() {
        let mut sync = BattleSync::new(1, 9);
        run_frames(&mut sync, 60, STALL_BIT);
        assert_eq!(sync.delay(), 10);
        run_frames(&mut sync, 60, STALL_BIT);
        assert_eq!(sync.delay(), 10);
    }

    #[test]
    fn quiet_windows_shrink_the_delay_but_not_below_the_initial_value() {
        let mut sync = BattleSync::new(1, 4);
        run_frames(&mut sync, 60, STALL_BIT);
        assert_eq!(sync.delay(), 5);

        let mut step = FrameStep {
            frame: 60,
            local_mask: 0,
            remote_mask: 0,
            remote_ready: false,
            seed: 0,
            delay: 5,
            outgoing: None,
            extra_inputs: Vec::new(),
            leave: false,
            checksum_due: false,
        };
        // Frames 61..64 still carry the stall bit scheduled before the delay grew,
        // so the first fully quiet 60-frame window ends at 120. Five quiet windows
        // finish at 420.
        for _ in 0..360 {
            step = sync.begin_frame(0);
        }
        assert_eq!(step.frame, 420);
        assert_eq!(step.delay, 4);
        assert!(step.extra_inputs.is_empty());
        assert_eq!(sync.delay(), 4);

        for _ in 0..300 {
            step = sync.begin_frame(0);
        }
        assert_eq!(sync.delay(), 4);
    }

    #[test]
    fn checksum_payload_matches_the_gml_record_format() {
        assert_eq!(BattleSync::checksum_payload(30, 111), "SUM\x1f30\x1e111");
    }
}
