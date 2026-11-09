//! One-shot "the game is ready" signal consumed by the launcher overlay.
//!
//! The engine paints its own "Loading…" screen before the first real frame, so the signal is
//! tied to the first audible sample ([`crate::game::audio::sound_started`]). Three fallbacks
//! make it robust:
//!
//! - [`GameReadyEvent::Timeout`] when the device produces no audio at all.
//! - [`GameReadyEvent::GameError`] when the game failed to start or crashed.
//! - [`GameReadySignal::destroy`] when the activity goes away: it cancels the listener, so the
//!   callback can never run after `destroy` returns.
//!
//! Every signal is one-shot: the listener runs at most once per session, whatever the order of
//! the events, and [`GameReadySignal::reset`] starts a fresh session.
//!
//! State machine:
//!
//! ```text
//! Waiting -- audio started --> Signaled(AudioStarted)
//! Waiting -- timeout --------> Signaled(Timeout)
//! Waiting -- game error -----> Signaled(GameError)
//! any     -- destroy --------> Destroyed
//! ```
//!
//! Only `Waiting` can signal. `Signaled` and `Destroyed` are terminal until the next
//! [`GameReadySignal::reset`]; in particular a late timeout or a late game-start failure can
//! never reach the listener of a session that was already destroyed.

use std::{
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

/// How long to wait for the first sound before signalling anyway. Generous: the game data can
/// be extracted and decoded on a slow device before the first frame is even painted.
pub const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// Why the one-shot signal fired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameReadyEvent {
    /// The first non-silent audio buffer was heard.
    AudioStarted,
    /// No audio arrived before the timeout.
    Timeout,
    /// The game failed to start, or crashed.
    GameError,
}

impl GameReadyEvent {
    /// Stable text used by the launcher's marker file.
    pub fn as_str(self) -> &'static str {
        match self {
            GameReadyEvent::AudioStarted => "audio",
            GameReadyEvent::Timeout => "timeout",
            GameReadyEvent::GameError => "error",
        }
    }
}

/// Where the signal is in its small lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameReadyState {
    /// Still loading; a signal may still fire.
    Waiting,
    /// The listener ran once. The event says why.
    Signaled(GameReadyEvent),
    /// The activity is gone; no listener can run any more.
    Destroyed,
}

/// The callback run once when the game is ready (or failed).
pub type GameReadyListener = Box<dyn FnOnce(GameReadyEvent) + Send + 'static>;

struct Inner {
    state: GameReadyState,
    started_at: Instant,
    timeout: Duration,
    listener: Option<GameReadyListener>,
}

impl Inner {
    /// Moves out of `Waiting` exactly once and runs the listener, if any.
    fn fire(&mut self, event: GameReadyEvent) -> Option<GameReadyEvent> {
        if self.state != GameReadyState::Waiting {
            return None
        }
        self.state = GameReadyState::Signaled(event);
        // Run the listener while the lock is held: a concurrent `destroy` either finishes before
        // this point (and then no listener runs) or waits until the listener is done. This is what
        // makes "no callback after destroy" true once `destroy` has returned.
        if let Some(listener) = self.listener.take() {
            listener(event);
        }
        Some(event)
    }
}

/// The signal and its one-shot listener.
pub struct GameReadySignal {
    inner: Mutex<Inner>,
}

impl GameReadySignal {
    /// Starts a session that times out after `timeout`.
    pub fn new(timeout: Duration) -> Self {
        Self::started_at(Instant::now(), timeout)
    }

    /// Same as [`Self::new`], with the start instant supplied by the caller. Tests use it to
    /// exercise the timeout without sleeping.
    pub fn started_at(started_at: Instant, timeout: Duration) -> Self {
        Self { inner: Mutex::new(Inner { state: GameReadyState::Waiting, started_at, timeout, listener: None }) }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panicking listener must not wedge the next session: recover the state.
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts a new session. Any previous signal and listener are forgotten.
    pub fn reset(&self, now: Instant) {
        let mut inner = self.lock();
        inner.state = GameReadyState::Waiting;
        inner.started_at = now;
        inner.listener = None;
    }

    /// Installs the listener that runs once. If the signal already fired or was destroyed, the
    /// listener is dropped without being called.
    pub fn on_ready(&self, listener: GameReadyListener) {
        let mut inner = self.lock();
        if inner.state == GameReadyState::Waiting {
            inner.listener = Some(listener);
        }
    }

    /// Polls the two automatic fallbacks. `audio_started` is the engine's
    /// [`crate::game::audio::sound_started`] flag.
    pub fn poll(&self, audio_started: bool) -> Option<GameReadyEvent> {
        self.poll_at(Instant::now(), audio_started)
    }

    /// [`Self::poll`] with an explicit clock, for tests.
    pub fn poll_at(&self, now: Instant, audio_started: bool) -> Option<GameReadyEvent> {
        let mut inner = self.lock();
        if inner.state != GameReadyState::Waiting {
            return None
        }
        let event = if audio_started {
            Some(GameReadyEvent::AudioStarted)
        } else if now.saturating_duration_since(inner.started_at) >= inner.timeout {
            Some(GameReadyEvent::Timeout)
        } else {
            None
        };
        event.and_then(|event| inner.fire(event))
    }

    /// Reports that the game failed to start or crashed. Signalling once, like every other path.
    pub fn game_error(&self) -> Option<GameReadyEvent> {
        let mut inner = self.lock();
        inner.fire(GameReadyEvent::GameError)
    }

    /// The activity is being destroyed. Cancels any pending listener; later signals are ignored.
    /// Idempotent.
    pub fn destroy(&self) {
        let mut inner = self.lock();
        inner.state = GameReadyState::Destroyed;
        inner.listener = None;
    }

    /// Current state, mostly for diagnostics and tests.
    pub fn state(&self) -> GameReadyState {
        self.lock().state
    }

    /// True once [`Self::destroy`] ran.
    pub fn is_destroyed(&self) -> bool {
        self.state() == GameReadyState::Destroyed
    }
}

/// The process-wide signal used by the Android entry point.
#[cfg(target_os = "android")]
mod android {
    use super::*;

    static GLOBAL: OnceLock<GameReadySignal> = OnceLock::new();

    fn signal() -> &'static GameReadySignal {
        GLOBAL.get_or_init(|| GameReadySignal::new(READY_TIMEOUT))
    }

    /// Resets the signal for a new Activity session, removes a marker left by the previous one and
    /// installs the listener the launcher sees.
    pub fn start_session() {
        remove_stale_marker();
        let signal = signal();
        signal.reset(Instant::now());
        signal.on_ready(Box::new(notify));
    }

    /// Polls audio start and the no-audio timeout. Called from the activity pump, which keeps
    /// running even when the game thread is stuck.
    pub fn poll(audio_started: bool) -> Option<GameReadyEvent> {
        signal().poll(audio_started)
    }

    /// Reports a launch failure or crash.
    pub fn game_error() -> Option<GameReadyEvent> {
        signal().game_error()
    }

    /// The Activity is gone: cancel anything still pending.
    pub fn destroy() {
        signal().destroy();
    }

    /// Current state, exposed for the launcher/debugging.
    pub fn state() -> GameReadyState {
        signal().state()
    }

    fn notify(event: GameReadyEvent) {
        write_marker(event);
        crate::android_main::notify_game_ready();
    }

    fn marker_path() -> std::path::PathBuf {
        crate::android_main::storage_path("game-ready")
    }

    fn remove_stale_marker() {
        let path = marker_path();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("tmp"));
    }

    /// The old launcher only checked that the file existed. Keeping it, with the reason inside,
    /// lets both the old polling code and a newer reader work during the transition.
    fn write_marker(event: GameReadyEvent) {
        let path = marker_path();
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, event.as_str().as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[cfg(target_os = "android")]
pub use android::{destroy, game_error, poll, start_session, state};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    };

    fn record(signal: &GameReadySignal) -> Arc<StdMutex<Vec<GameReadyEvent>>> {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        signal.on_ready(Box::new(move |event| sink.lock().unwrap().push(event)));
        seen
    }

    #[test]
    fn audio_start_signals_once() {
        let start = Instant::now();
        let signal = GameReadySignal::started_at(start, Duration::from_secs(20));
        let seen = record(&signal);

        assert_eq!(signal.poll_at(start, false), None);
        assert_eq!(signal.poll_at(start, true), Some(GameReadyEvent::AudioStarted));
        assert_eq!(signal.poll_at(start + Duration::from_secs(1), true), None);
        assert_eq!(signal.game_error(), None);
        assert_eq!(seen.lock().unwrap().as_slice(), &[GameReadyEvent::AudioStarted]);
        assert_eq!(signal.state(), GameReadyState::Signaled(GameReadyEvent::AudioStarted));
    }

    #[test]
    fn no_audio_times_out_once() {
        let start = Instant::now();
        let signal = GameReadySignal::started_at(start, Duration::from_secs(20));
        let seen = record(&signal);

        assert_eq!(signal.poll_at(start + Duration::from_secs(19), false), None);
        assert_eq!(signal.poll_at(start + Duration::from_secs(20), false), Some(GameReadyEvent::Timeout));
        assert_eq!(signal.poll_at(start + Duration::from_secs(40), false), None);
        assert_eq!(signal.poll_at(start + Duration::from_secs(40), true), None);
        assert_eq!(seen.lock().unwrap().as_slice(), &[GameReadyEvent::Timeout]);
        assert_eq!(signal.state(), GameReadyState::Signaled(GameReadyEvent::Timeout));
    }

    #[test]
    fn game_error_signals_once_and_wins_over_later_audio() {
        let signal = GameReadySignal::new(Duration::from_secs(20));
        let seen = record(&signal);

        assert_eq!(signal.game_error(), Some(GameReadyEvent::GameError));
        assert_eq!(signal.poll(true), None);
        assert_eq!(signal.game_error(), None);
        assert_eq!(signal.poll_at(Instant::now() + Duration::from_secs(60), false), None);
        assert_eq!(seen.lock().unwrap().as_slice(), &[GameReadyEvent::GameError]);
        assert_eq!(signal.state(), GameReadyState::Signaled(GameReadyEvent::GameError));
    }

    #[test]
    fn destroy_prevents_the_callback_from_ever_firing() {
        let start = Instant::now();
        let signal = GameReadySignal::started_at(start, Duration::from_secs(20));
        let seen = record(&signal);

        signal.destroy();
        assert_eq!(signal.state(), GameReadyState::Destroyed);
        assert!(signal.is_destroyed());
        assert_eq!(signal.poll_at(start, true), None);
        assert_eq!(signal.poll_at(start + Duration::from_secs(60), false), None);
        assert_eq!(signal.game_error(), None);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn destroy_after_signal_does_not_fire_again() {
        let signal = GameReadySignal::new(Duration::from_secs(20));
        let seen = record(&signal);

        assert_eq!(signal.poll(true), Some(GameReadyEvent::AudioStarted));
        signal.destroy();
        assert_eq!(signal.game_error(), None);
        assert_eq!(signal.poll(true), None);
        assert_eq!(seen.lock().unwrap().as_slice(), &[GameReadyEvent::AudioStarted]);
        assert_eq!(signal.state(), GameReadyState::Destroyed);
    }

    #[test]
    fn destroy_cancels_a_signal_pending_on_another_thread() {
        let signal = Arc::new(GameReadySignal::new(Duration::from_secs(20)));
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        signal.on_ready(Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        let late = Arc::clone(&signal);
        let late_calls = Arc::clone(&calls);
        let worker = std::thread::spawn(move || {
            while !late.is_destroyed() {
                std::thread::yield_now();
            }
            // Every path a late timeout, the audio starting or a crash could take.
            late.poll(true);
            late.game_error();
            late.poll_at(Instant::now() + Duration::from_secs(60), false);
            late_calls.load(Ordering::SeqCst)
        });

        signal.destroy();
        let after = worker.join().unwrap();
        assert_eq!(after, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reset_starts_a_new_one_shot_session() {
        let start = Instant::now();
        let signal = GameReadySignal::started_at(start, Duration::from_secs(20));
        let seen = record(&signal);

        assert_eq!(signal.poll_at(start, true), Some(GameReadyEvent::AudioStarted));

        let next = start + Duration::from_secs(30);
        signal.reset(next);
        assert_eq!(signal.state(), GameReadyState::Waiting);
        let second = record(&signal);
        assert_eq!(signal.poll_at(next, false), None);
        assert_eq!(signal.poll_at(next + Duration::from_secs(20), false), Some(GameReadyEvent::Timeout));
        assert_eq!(signal.poll_at(next + Duration::from_secs(21), true), None);

        assert_eq!(seen.lock().unwrap().as_slice(), &[GameReadyEvent::AudioStarted]);
        assert_eq!(second.lock().unwrap().as_slice(), &[GameReadyEvent::Timeout]);
    }

    #[test]
    fn events_have_stable_marker_text() {
        assert_eq!(GameReadyEvent::AudioStarted.as_str(), "audio");
        assert_eq!(GameReadyEvent::Timeout.as_str(), "timeout");
        assert_eq!(GameReadyEvent::GameError.as_str(), "error");
    }
}
