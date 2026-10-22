//! PC storage: click to pick up, free hover to move, click to place.
//! Only a stationary occupied slot can trigger a right-click after one second.
use std::time::{Duration, Instant};
pub const LEFT: u8 = 0;
pub const RIGHT: u8 = 2; // ramen mouse: left=0, middle=1, right=2
const HOLD: Duration = Duration::from_secs(1);
const HOVER: Duration = Duration::from_millis(45);
const CLICK: Duration = Duration::from_millis(70);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point(pub f32, pub f32);
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event { Move(Point), Down(Point, u8), Up(Point, u8) }
#[derive(Clone, Copy)]
enum Phase {
    Waiting,
    LeftPending { due: Instant, point: Point, finish: bool },
    LeftDown { at: Instant, point: Point, finish: bool },
    Hovering { last: Point, tap_places: bool },
    RightDown(Instant),
    Consumed,
}
struct Gesture {
    id: i32, origin: Point, latest: Point, started: Instant, slop: f32,
    released: bool, dragged: bool, allow_hold: bool, phase: Phase,
}
pub struct BoxTouch { gesture: Option<Gesture> }
impl BoxTouch {
    pub const fn new() -> Self { Self { gesture: None } }
    pub fn owns(&self, id: i32) -> bool { self.gesture.as_ref().is_some_and(|g| g.id == id) }
    pub fn begin(&mut self, id: i32, point: Point, height: f32, now: Instant, carrying: bool) -> bool {
        if let Some(g) = self.gesture.as_mut() {
            g.allow_hold = false; // second finger cannot steal the sole mouse
            return false
        }
        self.gesture = Some(Gesture {
            id, origin: point, latest: point, started: now,
            slop: (height * 0.015).clamp(8.0, 24.0),
            released: false, dragged: false, allow_hold: !carrying,
            phase: if carrying { Phase::Hovering { last: point, tap_places: true } } else { Phase::Waiting },
        });
        true
    }
    pub fn moved(&mut self, id: i32, point: Point, now: Instant) -> bool {
        let Some(g) = self.gesture.as_mut().filter(|g| g.id == id) else { return false };
        if g.released { return true }
        g.latest = point;
        let dx = point.0 - g.origin.0;
        let dy = point.1 - g.origin.1;
        if dx * dx + dy * dy > g.slop * g.slop {
            g.dragged = true;
            g.allow_hold = false;
            if matches!(g.phase, Phase::Waiting) {
                g.phase = Phase::LeftPending { due: (g.started + HOVER).max(now), point: g.origin, finish: false };
            }
        }
        true
    }
    pub fn end(&mut self, id: i32, point: Point, now: Instant) -> bool {
        if !self.moved(id, point, now) { return false }
        let g = self.gesture.as_mut().unwrap();
        g.released = true;
        if matches!(g.phase, Phase::Waiting) && !(g.allow_hold && now >= g.started + HOLD) {
            g.phase = Phase::LeftPending { due: (g.started + HOVER).max(now), point: g.origin, finish: true };
        }
        if matches!(g.phase, Phase::Consumed) { self.gesture = None; }
        true
    }
    /// Down/up are separate frames. Mouse is released after pickup; hover remains free.
    pub fn poll(&mut self, now: Instant, carrying: bool, resting_target: bool) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(g) = self.gesture.as_mut() else { return events };
        if carrying && matches!(g.phase, Phase::Waiting | Phase::LeftPending { finish: false, .. }) {
            g.allow_hold = false;
            g.phase = Phase::Hovering { last: g.origin, tap_places: true };
        }
        match g.phase {
            Phase::Waiting if g.allow_hold && !carrying && resting_target && now >= g.started + HOLD => {
                events.push(Event::Down(g.origin, RIGHT));
                g.phase = Phase::RightDown(now + CLICK);
            },
            Phase::Waiting if g.released => {
                g.phase = Phase::LeftPending { due: now, point: g.origin, finish: true };
            },
            Phase::LeftPending { due, point, finish } if now >= due => {
                events.push(Event::Down(point, LEFT));
                g.phase = Phase::LeftDown { at: now, point, finish };
            },
            Phase::LeftDown { at, point, finish } if now >= at + CLICK => {
                events.push(Event::Up(point, LEFT));
                if g.latest != point { events.push(Event::Move(g.latest)); }
                if finish || g.released { self.gesture = None; }
                else { g.phase = Phase::Hovering { last: g.latest, tap_places: false }; }
            },
            Phase::Hovering { last, tap_places } => {
                if last != g.latest { events.push(Event::Move(g.latest)); }
                g.phase = Phase::Hovering { last: g.latest, tap_places };
                if g.released {
                    if tap_places && !g.dragged {
                        g.phase = Phase::LeftPending { due: now + HOVER, point: g.latest, finish: true };
                    } else { self.gesture = None; }
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
    pub fn cancel(&mut self) -> (Option<i32>, Vec<Event>) {
        let Some(g) = self.gesture.take() else { return (None, Vec::new()) };
        let events = match g.phase {
            Phase::LeftDown { point, .. } => vec![Event::Up(point, LEFT)],
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
    fn setup(carrying: bool) -> (BoxTouch, Instant) {
        let mut b = BoxTouch::new();
        let t = Instant::now();
        assert!(b.begin(7, ORIGIN, 720.0, t, carrying));
        (b, t)
    }
    fn ms(t: Instant, n: u64) -> Instant { t + Duration::from_millis(n) }
    #[test]
    fn stationary_occupied_slot_right_clicks_once() {
        let (mut b,t)=setup(false);
        assert!(b.poll(ms(t,999),false,true).is_empty());
        assert_eq!(b.poll(ms(t,1000),false,true),vec![Event::Down(ORIGIN,RIGHT)]);
        assert_eq!(b.poll(ms(t,1070),false,true),vec![Event::Up(ORIGIN,RIGHT)]);
        assert!(b.poll(ms(t,5000),false,true).is_empty());
    }
    #[test]
    fn cannot_right_click_when_carrying_or_over_empty_slot() {
        let (mut b,t)=setup(true);
        assert!(b.poll(ms(t,2000),true,true).is_empty());
        let (mut b,t)=setup(false);
        assert!(b.poll(ms(t,2000),false,false).is_empty());
    }
    #[test]
    fn picked_up_pokemon_moves_with_hover_not_pressed_mouse() {
        let (mut b,t)=setup(false);
        b.moved(7, DEST, ms(t,100));
        assert_eq!(b.poll(ms(t,100),false,true),vec![Event::Down(ORIGIN,LEFT)]);
        assert_eq!(b.poll(ms(t,170),true,false),vec![Event::Up(ORIGIN,LEFT),Event::Move(DEST)]);
        b.moved(7,ORIGIN,ms(t,1000));
        assert_eq!(b.poll(ms(t,1000),true,false),vec![Event::Move(ORIGIN)]);
        b.end(7,DEST,ms(t,1500));
        assert_eq!(b.poll(ms(t,1500),true,false),vec![Event::Move(DEST)]);
    }
    #[test]
    fn carried_pokemon_tap_places_and_movement_never_clicks() {
        let (mut b,t)=setup(true);
        b.moved(7, DEST, ms(t,100));
        assert_eq!(b.poll(ms(t,100),true,false),vec![Event::Move(DEST)]);
        b.end(7,DEST,ms(t,200));
        assert!(b.poll(ms(t,200),true,false).is_empty());
        let (mut b,t)=setup(true); b.end(7,ORIGIN,ms(t,10));
        assert!(b.poll(ms(t,10),true,false).is_empty());
        assert_eq!(b.poll(ms(t,55),true,false),vec![Event::Down(ORIGIN,LEFT)]);
        assert_eq!(b.poll(ms(t,125),true,false),vec![Event::Up(ORIGIN,LEFT)]);
    }
    #[test]
    fn canceled_and_second_finger_are_safe() {
        let (mut b,t)=setup(false);
        assert!(!b.begin(8, DEST,720.0,ms(t,500),false));
        assert!(b.poll(ms(t,2000),false,true).is_empty());
        assert_eq!(b.cancel(),(Some(7),vec![]));
        assert!(!b.owns(7));
        assert!(b.begin(7,DEST,720.0,ms(t,2100),true));
    }
}
