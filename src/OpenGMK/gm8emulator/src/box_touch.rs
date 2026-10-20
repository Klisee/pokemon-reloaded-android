//! PC-box input arbitration. A stationary hold must not pick up a Pokémon first.
//! The Android bridge owns context/lifecycle cancellation and sends the initial hover.
use std::time::{Duration, Instant};

pub const LEFT: u8 = 0;
pub const RIGHT: u8 = 2; // ramen: 0=left, 1=middle, 2=right
const HOLD: Duration = Duration::from_secs(1);
const HOVER: Duration = Duration::from_millis(45);
const CLICK: Duration = Duration::from_millis(70);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point(pub f32, pub f32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Move(Point),
    Down(Point, u8),
    Up(Point, u8),
}

#[derive(Clone, Copy)]
enum Phase {
    Waiting,
    LeftPending(Instant),
    LeftDown { at: Instant, last: Point },
    RightDown(Instant),
    Consumed,
}

struct Gesture {
    id: i32,
    origin: Point,
    latest: Point,
    started: Instant,
    slop: f32,
    released: bool,
    allow_hold: bool,
    phase: Phase,
}

pub struct BoxTouch {
    gesture: Option<Gesture>,
}

impl BoxTouch {
    pub const fn new() -> Self { Self { gesture: None } }

    pub fn owns(&self, id: i32) -> bool {
        self.gesture.as_ref().is_some_and(|g| g.id == id)
    }

    pub fn begin(&mut self, id: i32, point: Point, height: f32, now: Instant) -> bool {
        if let Some(g) = self.gesture.as_mut() {
            // There is one game mouse. A second finger cannot move or release it.
            g.allow_hold = false;
            return false
        }
        self.gesture = Some(Gesture {
            id, origin: point, latest: point, started: now,
            slop: (height * 0.015).clamp(8.0, 24.0),
            released: false, allow_hold: true, phase: Phase::Waiting,
        });
        true
    }

    pub fn moved(&mut self, id: i32, point: Point, now: Instant) -> bool {
        let Some(g) = self.gesture.as_mut().filter(|g| g.id == id) else { return false };
        if g.released { return true }
        g.latest = point;
        let dx = point.0 - g.origin.0;
        let dy = point.1 - g.origin.1;
        if matches!(g.phase, Phase::Waiting) && dx * dx + dy * dy > g.slop * g.slop {
            g.allow_hold = false;
            g.phase = Phase::LeftPending((g.started + HOVER).max(now));
        }
        true
    }

    pub fn end(&mut self, id: i32, point: Point, now: Instant) -> bool {
        if !self.moved(id, point, now) { return false }
        let g = self.gesture.as_mut().unwrap();
        g.released = true;
        if matches!(g.phase, Phase::Waiting) && !(g.allow_hold && now >= g.started + HOLD) {
            g.phase = Phase::LeftPending((g.started + HOVER).max(now));
        }
        if matches!(g.phase, Phase::Consumed) { self.gesture = None; }
        true
    }

    /// Down and up always reach separate polls, including when a slow frame misses the deadline.
    pub fn poll(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(g) = self.gesture.as_mut() else { return events };
        match g.phase {
            Phase::Waiting if g.allow_hold && now >= g.started + HOLD => {
                events.push(Event::Down(g.origin, RIGHT));
                g.phase = Phase::RightDown(now + CLICK);
            },
            Phase::LeftPending(due) if now >= due => {
                events.push(Event::Down(g.origin, LEFT));
                g.phase = Phase::LeftDown { at: now, last: g.origin };
            },
            Phase::LeftDown { at, last } => {
                // First pick up at the original cell, then move to the drag destination.
                if now >= at + HOVER && last != g.latest {
                    events.push(Event::Move(g.latest));
                    g.phase = Phase::LeftDown { at, last: g.latest };
                }
                if g.released && now >= at + CLICK {
                    events.push(Event::Up(g.latest, LEFT));
                    self.gesture = None;
                }
            },
            Phase::RightDown(due) if now >= due => {
                events.push(Event::Up(g.origin, RIGHT));
                g.phase = Phase::Consumed;
                if g.released { self.gesture = None; }
            },
            _ => {},
        }
        events
    }

    /// Release only a button actually delivered. Pending gestures never become clicks on exit.
    pub fn cancel(&mut self) -> (Option<i32>, Vec<Event>) {
        let Some(g) = self.gesture.take() else { return (None, Vec::new()) };
        let events = match g.phase {
            Phase::LeftDown { last, .. } => vec![Event::Up(last, LEFT)],
            Phase::RightDown(_) => vec![Event::Up(g.origin, RIGHT)],
            _ => Vec::new(),
        };
        (Some(g.id), events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ORIGIN: Point = Point(300.0, 200.0);
    const DEST: Point = Point(500.0, 250.0);
    fn setup() -> (BoxTouch, Instant) {
        let mut touch = BoxTouch::new();
        let t = Instant::now();
        assert!(touch.begin(7, ORIGIN, 720.0, t));
        (touch, t)
    }
    fn ms(t: Instant, n: u64) -> Instant { t + Duration::from_millis(n) }

    #[test]
    fn stationary_hold_is_one_right_click_without_primary_click() {
        let (mut b, t) = setup();
        assert!(b.poll(ms(t, 999)).is_empty());
        assert_eq!(b.poll(ms(t, 1000)), vec![Event::Down(ORIGIN, RIGHT)]);
        assert!(b.poll(ms(t, 1069)).is_empty());
        assert_eq!(b.poll(ms(t, 1070)), vec![Event::Up(ORIGIN, RIGHT)]);
        assert!(b.poll(ms(t, 5000)).is_empty());
        assert!(b.end(7, ORIGIN, ms(t, 6000)));
        assert!(b.poll(ms(t, 6000)).is_empty());
        assert!(!b.owns(7));
    }

    #[test]
    fn quick_tap_waits_for_hover_and_keeps_primary_down_long_enough() {
        let (mut b, t) = setup();
        assert!(b.end(7, ORIGIN, ms(t, 10)));
        assert!(b.poll(ms(t, 44)).is_empty());
        assert_eq!(b.poll(ms(t, 45)), vec![Event::Down(ORIGIN, LEFT)]);
        assert!(b.poll(ms(t, 114)).is_empty());
        assert_eq!(b.poll(ms(t, 115)), vec![Event::Up(ORIGIN, LEFT)]);
        assert!(!b.owns(7));
    }

    #[test]
    fn drag_starts_at_original_cell_and_never_becomes_a_hold() {
        let (mut b, t) = setup();
        b.moved(7, DEST, ms(t, 100));
        assert_eq!(b.poll(ms(t, 100)), vec![Event::Down(ORIGIN, LEFT)]);
        assert!(b.poll(ms(t, 144)).is_empty());
        assert_eq!(b.poll(ms(t, 145)), vec![Event::Move(DEST)]);
        b.moved(7, ORIGIN, ms(t, 800));
        assert_eq!(b.poll(ms(t, 2000)), vec![Event::Move(ORIGIN)]);
        b.end(7, DEST, ms(t, 2500));
        assert_eq!(b.poll(ms(t, 2500)), vec![Event::Move(DEST), Event::Up(DEST, LEFT)]);
    }

    #[test]
    fn small_finger_jitter_does_not_cancel_hold() {
        let (mut b, t) = setup();
        b.moved(7, Point(304.0, 203.0), ms(t, 900));
        assert_eq!(b.poll(ms(t, 1000)), vec![Event::Down(ORIGIN, RIGHT)]);
    }

    #[test]
    fn second_finger_cannot_steal_mouse_or_trigger_long_press() {
        let (mut b, t) = setup();
        assert!(!b.begin(8, DEST, 720.0, ms(t, 500)));
        assert!(!b.moved(8, DEST, ms(t, 600)));
        assert!(!b.end(8, DEST, ms(t, 700)));
        assert!(b.poll(ms(t, 2000)).is_empty());
        b.end(7, ORIGIN, ms(t, 2100));
        assert_eq!(b.poll(ms(t, 2100)), vec![Event::Down(ORIGIN, LEFT)]);
    }

    #[test]
    fn cancellation_discards_pending_click_and_allows_pointer_reuse() {
        let (mut b, t) = setup();
        assert_eq!(b.cancel(), (Some(7), vec![]));
        assert!(b.poll(ms(t, 5000)).is_empty());
        assert!(!b.end(7, ORIGIN, ms(t, 5001)));
        assert!(b.begin(7, DEST, 720.0, ms(t, 5002)));
    }

    #[test]
    fn cancellation_releases_each_delivered_mouse_button_once() {
        for button in [LEFT, RIGHT] {
            let (mut b, t) = setup();
            if button == LEFT { b.end(7, ORIGIN, ms(t, 10)); }
            assert_eq!(b.poll(ms(t, 1000)), vec![Event::Down(ORIGIN, button)]);
            assert_eq!(b.cancel(), (Some(7), vec![Event::Up(ORIGIN, button)]));
            assert_eq!(b.cancel(), (None, vec![]));
            assert!(b.poll(ms(t, 5000)).is_empty());
        }
    }

    #[test]
    fn release_at_hold_deadline_never_produces_both_clicks() {
        for before_poll in [true, false] {
            let (mut b, t) = setup();
            if before_poll { b.end(7, ORIGIN, ms(t, 1000)); }
            assert_eq!(b.poll(ms(t, 1000)), vec![Event::Down(ORIGIN, RIGHT)]);
            if !before_poll { b.end(7, ORIGIN, ms(t, 1030)); }
            assert_eq!(b.poll(ms(t, 1070)), vec![Event::Up(ORIGIN, RIGHT)]);
            assert!(b.poll(ms(t, 3000)).is_empty());
            assert!(!b.owns(7));
        }
    }
}
