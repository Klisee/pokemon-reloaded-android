//! Low-noise diagnostics for online battle lockstep.
//!
//! The GML scripts log a desync when the 30-frame checksum differs, but they do not say whether
//! the inputs themselves were arriving in time. A two-device test needs that second half: if both
//! peers report the same frame/seed/checksum stream and no waits, they are lockstepping.
//!
//! This module only observes the pure [`BattleSync`](crate::battle_sync::BattleSync) state owned
//! by the caller; it never changes frame, delay, input or checksum decisions. It emits one compact
//! line per interval (30 frames by default, roughly one second at 30 Hz) plus one line when the
//! battle starts and one when it ends. There is no per-frame output.
//!
//! The diagnostics are enabled by default so a device test can read them from logcat without a
//! special build. Set `OPENGMK_BATTLE_DIAG=0` to disable them or
//! `OPENGMK_BATTLE_DIAG_FRAMES=N` to change the report interval (1..=1800 frames).
//!
//! Format:
//!
//! ```text
//! [battle-diag] start role=P1 epoch=123 seed=123 delay=3 local=7 peer=9 interval=30
//! [battle-diag] role=P1 epoch=123 frame=30 seed=492030 delay=2 dmin=2 dmax=2 dchg=0 wait=0/30 ...
//! [battle-diag] end role=P1 epoch=123 frame=150 seed=492150 delay=3 dmin=2 dmax=3 ...
//! ```
//!
//! The periodic line continues with `wmax`, `rx`, `rq`, `rlate`, `sumf`, `lsum`, `rsumf`,
//! `rsum`, `sums` and `bad`. `wait` counts frames whose peer input had not arrived yet, `wmax`
//! the longest such streak, `rx`/`rq`/`rlate` the remote inputs received / still queued / already
//! too late, `sumf`+`lsum` and `rsumf`+`rsum` the local and peer checksum samples with their
//! frame, and `bad` the number of same-frame checksum mismatches in the window.

use crate::battle_sync::FrameStep;

const PREFIX: &str = "[battle-diag]";
const DEFAULT_INTERVAL: u32 = crate::battle_sync::CHECKSUM_INTERVAL;
const MIN_INTERVAL: u32 = 1;
const MAX_INTERVAL: u32 = 1800;

/// Enablement and report cadence for [`BattleDiag`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattleDiagConfig {
    enabled: bool,
    interval: u32,
}

impl Default for BattleDiagConfig {
    fn default() -> Self {
        Self { enabled: true, interval: DEFAULT_INTERVAL }
    }
}

impl BattleDiagConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("OPENGMK_BATTLE_DIAG").ok();
        let frames = std::env::var("OPENGMK_BATTLE_DIAG_FRAMES").ok();
        Self::from_values(enabled.as_deref(), frames.as_deref())
    }

    fn from_values(enabled: Option<&str>, frames: Option<&str>) -> Self {
        let enabled = enabled
            .map(|value| !matches!(value.trim().to_ascii_lowercase().as_str(), "" | "0" | "off" | "false" | "no"))
            .unwrap_or(true);
        let interval = frames
            .and_then(|value| value.trim().parse::<u32>().ok())
            .map(|value| value.clamp(MIN_INTERVAL, MAX_INTERVAL))
            .unwrap_or(DEFAULT_INTERVAL);
        Self { enabled, interval }
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self { enabled: false, interval: DEFAULT_INTERVAL }
    }

    #[cfg(test)]
    pub fn every(interval: u32) -> Self {
        Self { enabled: true, interval: interval.clamp(MIN_INTERVAL, MAX_INTERVAL) }
    }
}

/// Window counters reset after every emitted report.
#[derive(Default)]
struct Window {
    frames: u32,
    wait: u32,
    wait_streak: u32,
    wait_streak_max: u32,
    rx: u32,
    late: u32,
    sums: u32,
    bad: u32,
    delay_changes: u32,
}

/// Rate-limited lockstep observer. See the module documentation for the line format.
pub struct BattleDiag {
    config: BattleDiagConfig,
    active: bool,
    role: &'static str,
    epoch: i32,
    seed: i32,
    local_id: u64,
    peer: u64,
    interval: u32,
    next_report: u32,
    window: Window,
    delay: i32,
    min_delay: i32,
    max_delay: i32,
    last_frame: u32,
    last_seed: i32,
    queued: usize,
    local_sum: Option<(u32, u32)>,
    peer_sum: Option<(u32, u32)>,
    bad_frame: Option<u32>,
    total_frames: u64,
    total_wait: u64,
    max_wait_streak: u32,
    total_rx: u64,
    total_late: u64,
    total_sums: u64,
    total_bad: u64,
    total_delay_changes: u64,
}

impl BattleDiag {
    pub fn new(config: BattleDiagConfig) -> Self {
        Self {
            config,
            active: false,
            role: "",
            epoch: 0,
            seed: 0,
            local_id: 0,
            peer: 0,
            interval: DEFAULT_INTERVAL,
            next_report: DEFAULT_INTERVAL,
            window: Window::default(),
            delay: 0,
            min_delay: 0,
            max_delay: 0,
            last_frame: 0,
            last_seed: 0,
            queued: 0,
            local_sum: None,
            peer_sum: None,
            bad_frame: None,
            total_frames: 0,
            total_wait: 0,
            max_wait_streak: 0,
            total_rx: 0,
            total_late: 0,
            total_sums: 0,
            total_bad: 0,
            total_delay_changes: 0,
        }
    }

    pub fn from_env() -> Self {
        Self::new(BattleDiagConfig::from_env())
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn start(
        &mut self,
        role: &'static str,
        epoch: i32,
        seed: i32,
        delay: i32,
        local_id: u64,
        peer: u64,
    ) -> Option<String> {
        if !self.config.enabled {
            return None
        }
        self.active = true;
        self.role = role;
        self.epoch = epoch;
        self.seed = seed;
        self.local_id = local_id;
        self.peer = peer;
        self.interval = self.config.interval.max(MIN_INTERVAL);
        self.next_report = self.interval;
        self.window = Window::default();
        self.delay = delay;
        self.min_delay = delay;
        self.max_delay = delay;
        self.last_frame = 0;
        self.last_seed = seed;
        self.queued = 0;
        self.local_sum = None;
        self.peer_sum = None;
        self.bad_frame = None;
        self.total_frames = 0;
        self.total_wait = 0;
        self.max_wait_streak = 0;
        self.total_rx = 0;
        self.total_late = 0;
        self.total_sums = 0;
        self.total_bad = 0;
        self.total_delay_changes = 0;
        Some(format!(
            "{PREFIX} start role={} epoch={} seed={} delay={} local={} peer={} interval={}",
            self.role, self.epoch, self.seed, self.delay, self.local_id, self.peer, self.interval
        ))
    }

    /// Observe one simulated frame after `BattleSync::begin_frame`.
    pub fn record_frame(&mut self, step: &FrameStep, queued: usize) {
        if !self.active {
            return
        }
        self.last_frame = step.frame;
        self.last_seed = step.seed;
        self.queued = queued;
        self.window.frames += 1;
        self.total_frames += 1;

        if step.remote_ready {
            self.window.wait_streak = 0;
        } else {
            self.window.wait += 1;
            self.total_wait += 1;
            self.window.wait_streak += 1;
            self.window.wait_streak_max = self.window.wait_streak_max.max(self.window.wait_streak);
            self.max_wait_streak = self.max_wait_streak.max(self.window.wait_streak);
        }

        if step.delay != self.delay {
            self.window.delay_changes += 1;
            self.total_delay_changes += 1;
            self.delay = step.delay;
        }
        self.min_delay = self.min_delay.min(step.delay);
        self.max_delay = self.max_delay.max(step.delay);
    }

    /// Observe an accepted remote input before it is queued in `BattleSync`.
    pub fn record_remote_input(&mut self, frame: u32, local_frame: u32) {
        if !self.active {
            return
        }
        self.window.rx += 1;
        self.total_rx += 1;
        if frame <= local_frame {
            self.window.late += 1;
            self.total_late += 1;
        }
    }

    /// Observe the local checksum stored by `BattleSync::store_checksum`.
    pub fn record_local_checksum(&mut self, frame: u32, checksum: u32) {
        if !self.active {
            return
        }
        self.local_sum = Some((frame, checksum));
        self.window.sums += 1;
        self.total_sums += 1;
        self.bad_frame = None;
        self.compare_checksums();
    }

    /// Observe a peer checksum passed to `BattleSync::receive_checksum`.
    pub fn record_peer_checksum(&mut self, frame: u32, checksum: u32) {
        if !self.active {
            return
        }
        self.peer_sum = Some((frame, checksum));
        self.compare_checksums();
    }

    fn compare_checksums(&mut self) {
        let Some((local_frame, local_value)) = self.local_sum else { return };
        let Some((peer_frame, peer_value)) = self.peer_sum else { return };
        if local_frame == peer_frame && local_value != peer_value && self.bad_frame != Some(local_frame) {
            self.bad_frame = Some(local_frame);
            self.window.bad += 1;
            self.total_bad += 1;
        }
    }

    /// Emit the periodic report when the current frame reaches the next interval.
    pub fn maybe_report(&mut self) -> Option<String> {
        if !self.active || self.last_frame < self.next_report {
            return None
        }
        let line = self.report_line();
        self.next_report = self.last_frame.saturating_add(self.interval.max(MIN_INTERVAL));
        self.window = Window::default();
        Some(line)
    }

    fn report_line(&self) -> String {
        let (sum_frame, local_sum) = match self.local_sum {
            Some((frame, value)) => (frame.to_string(), format!("0x{value:08x}")),
            None => ("-".to_string(), "-".to_string()),
        };
        let (peer_frame, peer_sum) = match self.peer_sum {
            Some((frame, value)) => (frame.to_string(), format!("0x{value:08x}")),
            None => ("-".to_string(), "-".to_string()),
        };
        format!(
            "{PREFIX} role={} epoch={} frame={} seed={} delay={} dmin={} dmax={} dchg={} wait={}/{} wmax={} \
             rx={} rq={} rlate={} sumf={} lsum={} rsumf={} rsum={} sums={} bad={}",
            self.role,
            self.epoch,
            self.last_frame,
            self.last_seed,
            self.delay,
            self.min_delay,
            self.max_delay,
            self.window.delay_changes,
            self.window.wait,
            self.window.frames,
            self.window.wait_streak_max,
            self.window.rx,
            self.queued,
            self.window.late,
            sum_frame,
            local_sum,
            peer_frame,
            peer_sum,
            self.window.sums,
            self.window.bad,
        )
    }

    /// Emit the final summary once; later calls are a no-op.
    pub fn finish(&mut self) -> Option<String> {
        if !self.active {
            return None
        }
        self.active = false;
        Some(format!(
            "{PREFIX} end role={} epoch={} frame={} seed={} delay={} dmin={} dmax={} dchg={} frames={} \
             wait={} wmax={} rx={} rlate={} sums={} bad={}",
            self.role,
            self.epoch,
            self.last_frame,
            self.last_seed,
            self.delay,
            self.min_delay,
            self.max_delay,
            self.total_delay_changes,
            self.total_frames,
            self.total_wait,
            self.max_wait_streak,
            self.total_rx,
            self.total_late,
            self.total_sums,
            self.total_bad,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_sync::{CHECKSUM_INTERVAL, OutgoingInput, STALL_BIT, frame_seed};

    fn step(frame: u32, remote_ready: bool, delay: i32) -> FrameStep {
        FrameStep {
            frame,
            local_mask: 0,
            remote_mask: 0,
            remote_ready,
            seed: frame_seed(7, frame),
            delay,
            outgoing: Some(OutgoingInput { frame: frame + delay as u32, mask: 0 }),
            extra_inputs: Vec::new(),
            leave: false,
            checksum_due: frame % CHECKSUM_INTERVAL == 0,
        }
    }

    fn diag(interval: u32) -> BattleDiag {
        let mut diag = BattleDiag::new(BattleDiagConfig::every(interval));
        diag.start("P1", 7, 7, 3, 10, 20).expect("start line");
        diag
    }

    #[test]
    fn config_defaults_and_env_values_are_bounded() {
        let default = BattleDiagConfig::default();
        assert!(default.enabled);
        assert_eq!(default.interval, CHECKSUM_INTERVAL);

        assert!(!BattleDiagConfig::from_values(Some("0"), None).enabled);
        assert!(!BattleDiagConfig::from_values(Some("off"), None).enabled);
        assert!(!BattleDiagConfig::from_values(Some("false"), None).enabled);
        assert!(!BattleDiagConfig::from_values(Some(""), None).enabled);
        assert!(BattleDiagConfig::from_values(Some("1"), None).enabled);
        assert!(BattleDiagConfig::from_values(None, None).enabled);

        assert_eq!(BattleDiagConfig::from_values(None, Some("60")).interval, 60);
        assert_eq!(BattleDiagConfig::from_values(None, Some("0")).interval, MIN_INTERVAL);
        assert_eq!(BattleDiagConfig::from_values(None, Some("99999")).interval, MAX_INTERVAL);
        assert_eq!(BattleDiagConfig::from_values(None, Some("-3")).interval, DEFAULT_INTERVAL);
        assert_eq!(BattleDiagConfig::from_values(None, Some("abc")).interval, DEFAULT_INTERVAL);
    }

    #[test]
    fn start_and_end_lines_carry_the_identity() {
        let mut diag = diag(30);
        let end = diag.finish().expect("end line");
        assert_eq!(
            end,
            "[battle-diag] end role=P1 epoch=7 frame=0 seed=7 delay=3 dmin=3 dmax=3 dchg=0 frames=0 wait=0 wmax=0 \
             rx=0 rlate=0 sums=0 bad=0"
        );
        assert!(!diag.is_active());
        assert!(diag.finish().is_none());
    }

    #[test]
    fn reports_wait_remote_input_delay_and_seed_once_per_interval() {
        let mut diag = diag(2);
        diag.record_remote_input(1, 0);
        diag.record_frame(&step(1, true, 3), 0);
        assert!(diag.maybe_report().is_none());

        diag.record_remote_input(1, 1);
        diag.record_frame(&step(2, false, 4), 1);
        diag.record_local_checksum(2, 0x1111_2222);
        let line = diag.maybe_report().expect("first report");
        assert_eq!(
            line,
            "[battle-diag] role=P1 epoch=7 frame=2 seed=114690 delay=4 dmin=3 dmax=4 dchg=1 wait=1/2 wmax=1 \
             rx=2 rq=1 rlate=1 sumf=2 lsum=0x11112222 rsumf=- rsum=- sums=1 bad=0"
        );
        assert!(diag.maybe_report().is_none());

        diag.record_frame(&step(3, false, 4), 0);
        diag.record_frame(&step(4, false, 4), 0);
        let line = diag.maybe_report().expect("second report");
        assert!(line.contains("frame=4"));
        assert!(line.contains("wait=2/2"));
        assert!(line.contains("wmax=2"));
    }

    #[test]
    fn checksum_mismatches_are_counted_once_in_either_arrival_order() {
        let mut peer_first = diag(30);
        peer_first.record_peer_checksum(30, 222);
        peer_first.record_local_checksum(30, 111);
        peer_first.record_frame(&step(30, true, 3), 0);
        let line = peer_first.maybe_report().expect("peer-first report");
        assert!(line.contains("sumf=30 lsum=0x0000006f rsumf=30 rsum=0x000000de sums=1 bad=1"));

        let mut local_first = diag(30);
        local_first.record_local_checksum(30, 111);
        local_first.record_peer_checksum(30, 222);
        local_first.record_frame(&step(30, true, 3), 0);
        let line = local_first.maybe_report().expect("local-first report");
        assert!(line.contains("sumf=30 lsum=0x0000006f rsumf=30 rsum=0x000000de sums=1 bad=1"));

        let mut equal = diag(30);
        equal.record_local_checksum(30, 111);
        equal.record_peer_checksum(30, 111);
        equal.record_frame(&step(30, true, 3), 0);
        let line = equal.maybe_report().expect("equal report");
        assert!(line.contains("sums=1 bad=0"));
    }

    #[test]
    fn end_summary_keeps_totals_across_reports() {
        let mut diag = diag(2);
        diag.record_remote_input(1, 0);
        diag.record_frame(&step(1, true, 3), 0);
        diag.record_frame(&step(2, false, 4), 1);
        diag.maybe_report().expect("first report");

        diag.record_remote_input(1, 2);
        diag.record_frame(&step(3, false, 4), 0);
        diag.record_frame(&step(4, false, 4), 0);
        diag.maybe_report().expect("second report");
        let end = diag.finish().expect("end line");
        assert!(end.contains("frame=4 seed=114692 delay=4 dmin=3 dmax=4 dchg=1"));
        assert!(end.contains("frames=4 wait=3 wmax=2 rx=2 rlate=1 sums=0 bad=0"));
    }

    #[test]
    fn disabled_diagnostics_stay_silent() {
        let mut diag = BattleDiag::new(BattleDiagConfig::disabled());
        assert!(diag.start("P1", 1, 1, 2, 1, 2).is_none());
        diag.record_frame(&step(30, false, 2), 0);
        diag.record_local_checksum(30, 1);
        assert!(diag.maybe_report().is_none());
        assert!(diag.finish().is_none());
    }

    #[test]
    fn stall_bit_is_only_a_frame_property_not_a_diag_input() {
        let mut diag = diag(1);
        let mut stalled = step(1, false, 3);
        stalled.remote_mask = STALL_BIT;
        diag.record_frame(&stalled, 0);
        let line = diag.maybe_report().expect("report");
        assert!(line.contains("wait=1/1"));
        assert!(line.contains("wmax=1"));
    }
}
