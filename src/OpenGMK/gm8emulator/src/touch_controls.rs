//! On-screen touch controls for the Android port.
//!
//! The game only understands keyboard input, so each on-screen control injects the key the game
//! expects. The key set is the one the game binds by default:
//!
//! - arrows: move
//! - `Z` confirm/act, `X` cancel, `C`/`V` extra actions (fishing/bike, attacks 3/4)
//! - `1`..`6`: pick a party Pokémon in battle (and act as the HM shortcuts / berries)
//! - `B` pokéball, `S` switch, `M` mega, `E` run, `A` walk/assign, `N` use item
//! - `ENTER` menus, `ESC` quit
//!
//! Movement is an analog joystick (bottom-left); the rest are buttons. Positions are normalised to
//! the window so the input hit-test and the renderer share the exact same layout.
//!
//! Controls are grouped by the game's current state (see [current_context]): the title/menu rooms
//! only need the basics, the overworld needs the joystick and interaction keys, and battle needs
//! the combat extras. The emulator reports the current room every frame.

use ramen::input::Key;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

/// How long a state the engine reported keeps being believed after it stops being reported.
///
/// A long conversation is drawn as several boxes one after another, and between two of them the game
/// clears its modal for a frame or two. Reading that literally put the overlay back in the middle of
/// the conversation: the buttons blink in, and they are useless there anyway, because the player
/// cannot move while a dialogue is up.
const GRACE: Duration = Duration::from_millis(600);

/// A yes/no the engine reports every frame, that only drops once the game has stopped saying yes for
/// [GRACE].
struct Sticky {
    flag: AtomicBool,
    seen: Mutex<Option<Instant>>,
}

impl Sticky {
    const fn new() -> Self {
        Self { flag: AtomicBool::new(false), seen: Mutex::new(None) }
    }

    /// Notes this frame's reading. True when the answer actually changed.
    fn report(&self, on: bool) -> bool {
        if on {
            *self.seen.lock().unwrap() = Some(Instant::now());
            return !self.flag.swap(true, Ordering::Relaxed)
        }
        let recent = self.seen.lock().unwrap().map(|at| at.elapsed()).is_some_and(|ago| ago < GRACE);
        if recent {
            return false
        }
        self.flag.swap(false, Ordering::Relaxed)
    }

    fn get(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }
}

/// Which game states a control is shown in.
pub const MENU: u8 = 1;
pub const WORLD: u8 = 2;
pub const BATTLE: u8 = 4;
/// A dialogue box is open: only the button that advances it is shown, so nothing covers the text.
pub const DIALOG: u8 = 8;
/// A new game asking you to pick your character. That screen is moved with the left and right keys,
/// so it gets a pair of arrows and nothing else.
pub const CHARACTER: u8 = 16;
/// A modal asking you to pick from a list of options - the preferences the new game asks about right
/// after the character, drawn over that same screen. The list is walked with the movement keys and
/// answered with Z and X, so it gets the joystick and those two buttons, and nothing else.
pub const CHOICE: u8 = 32;
/// The Safari Zone's catching room: everything the map shows, minus the movement stick and the
/// attack diamond - none of your own Pokémon fights there - plus the ball, which is the key that
/// room answers to for throwing. The flee (E), the bag and the rest stay where the map has them.
pub const SAFARI: u8 = 64;
pub const MENU_NAV: u8 = 128;

/// The actual puzzle room: PuzleUNW, 202.
pub const PUZZLE_ROOM: i32 = 202;
pub const ROTATE_NAME: &str = "girar";

#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    Circle { cx: f32, cy: f32, r: f32 },
    Rect { cx: f32, cy: f32, w: f32, h: f32 },
}

#[derive(Clone, Copy, PartialEq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Label {
    None,
    Arrow(Dir),
    Text(&'static str),
}

pub struct Control {
    /// Stable name, used as the key when the layout is saved and loaded. Reordering the array of
    /// controls must not move a player's customised button, so the name is what identifies it.
    pub name: &'static str,
    pub key: Key,
    pub shape: Shape,
    pub label: Label,
    /// Bitmask of [MENU] / [WORLD] / [BATTLE] states this control belongs to.
    pub contexts: u8,
}

/// The joystick is not a button but is customised (and saved) the same way.
pub const JOYSTICK_NAME: &str = "joystick";

pub const CLASSIC_CONTROLS: &[Control] = &[
    // Number keys (first row along the top). They are for the base game only: in battle those keys
    // pick a party Pokémon, and that is what the SEL wheel below is for, so showing them there would
    // only get in the way.
    Control { name: "alpha1", key: Key::Alpha1, shape: Shape::Circle { cx: 0.300, cy: 0.055, r: 0.020 }, label: Label::Text("1"), contexts: SAFARI },
    Control { name: "alpha2", key: Key::Alpha2, shape: Shape::Circle { cx: 0.372, cy: 0.055, r: 0.020 }, label: Label::Text("2"), contexts: SAFARI },
    Control { name: "alpha3", key: Key::Alpha3, shape: Shape::Circle { cx: 0.444, cy: 0.055, r: 0.020 }, label: Label::Text("3"), contexts: SAFARI },
    Control { name: "alpha4", key: Key::Alpha4, shape: Shape::Circle { cx: 0.516, cy: 0.055, r: 0.020 }, label: Label::Text("4"), contexts: SAFARI },
    Control { name: "alpha5", key: Key::Alpha5, shape: Shape::Circle { cx: 0.588, cy: 0.055, r: 0.020 }, label: Label::Text("5"), contexts: SAFARI },
    Control { name: "alpha6", key: Key::Alpha6, shape: Shape::Circle { cx: 0.660, cy: 0.055, r: 0.020 }, label: Label::Text("6"), contexts: SAFARI },
    Control { name: "alpha7", key: Key::Alpha7, shape: Shape::Circle { cx: 0.732, cy: 0.055, r: 0.020 }, label: Label::Text("7"), contexts: WORLD },
    Control { name: crate::field_controls::NAME, key: Key::Alpha1, shape: Shape::Circle { cx: 0.300, cy: 0.760, r: 0.038 }, label: Label::Text("SEL"), contexts: WORLD },
    // Menu / misc keys (second row along the top).
    Control { name: "space", key: Key::Space, shape: Shape::Circle { cx: 0.300, cy: 0.145, r: 0.022 }, label: Label::Text("PKM"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: ROTATE_NAME, key: Key::Z, shape: Shape::Circle { cx: 0.935, cy: 0.150, r: 0.036 }, label: Label::Text("Girar"), contexts: 0 },
    Control { name: "f1", key: Key::F1, shape: Shape::Circle { cx: 0.935, cy: 0.300, r: 0.036 }, label: Label::Text("F1"), contexts: 0 },
    Control { name: "box_z", key: Key::Z, shape: Shape::Circle { cx: 0.865, cy: 0.851, r: 0.033 }, label: Label::Text("Z"), contexts: 0 },
    Control { name: "box_x", key: Key::X, shape: Shape::Circle { cx: 0.933, cy: 0.700, r: 0.033 }, label: Label::Text("X"), contexts: 0 },
    Control { name: "box_s", key: Key::S, shape: Shape::Circle { cx: 0.712, cy: 0.700, r: 0.026 }, label: Label::Text("S"), contexts: 0 },
    Control { name: "box_o", key: Key::O, shape: Shape::Circle { cx: 0.712, cy: 0.840, r: 0.026 }, label: Label::Text("O"), contexts: 0 },
    Control { name: "o", key: Key::O, shape: Shape::Circle { cx: 0.372, cy: 0.145, r: 0.022 }, label: Label::Text("O"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "m", key: Key::M, shape: Shape::Circle { cx: 0.444, cy: 0.145, r: 0.022 }, label: Label::Text("M"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "e", key: Key::E, shape: Shape::Circle { cx: 0.516, cy: 0.145, r: 0.022 }, label: Label::Text("E"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "t", key: Key::T, shape: Shape::Circle { cx: 0.588, cy: 0.145, r: 0.022 }, label: Label::Text("T"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "l", key: Key::L, shape: Shape::Circle { cx: 0.660, cy: 0.145, r: 0.022 }, label: Label::Text("L"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "p", key: Key::P, shape: Shape::Circle { cx: 0.732, cy: 0.145, r: 0.022 }, label: Label::Text("P"), contexts: WORLD | BATTLE | SAFARI },
    // Main actions, in the diamond a gamepad's face buttons use: the main one at the bottom, cancel
    // to the right, and the other two on the left and on top. The cluster is about the size of the
    // movement joystick and sits at the same height, so the two thumbs fall in the same place.
    Control { name: "z", key: Key::Z, shape: Shape::Circle { cx: 0.865, cy: 0.851, r: 0.033 }, label: Label::Text("Z"), contexts: WORLD | BATTLE | CHOICE | MENU_NAV },
    Control { name: "x", key: Key::X, shape: Shape::Circle { cx: 0.933, cy: 0.700, r: 0.033 }, label: Label::Text("X"), contexts: WORLD | BATTLE | CHOICE | MENU_NAV },
    Control { name: "c", key: Key::C, shape: Shape::Circle { cx: 0.797, cy: 0.700, r: 0.033 }, label: Label::Text("C"), contexts: WORLD | BATTLE },
    Control { name: "v", key: Key::V, shape: Shape::Circle { cx: 0.865, cy: 0.549, r: 0.033 }, label: Label::Text("V"), contexts: WORLD | BATTLE },
    // Secondary.
    Control { name: "b", key: Key::B, shape: Shape::Circle { cx: 0.712, cy: 0.840, r: 0.026 }, label: Label::Text("B"), contexts: BATTLE | SAFARI },
    Control { name: "s", key: Key::S, shape: Shape::Circle { cx: 0.712, cy: 0.700, r: 0.026 }, label: Label::Text("S"), contexts: BATTLE },
    // Holding this opens the party wheel (see below): each slot is one Pokémon of the team.
    Control { name: PARTY_NAME, key: Key::Alpha1, shape: Shape::Circle { cx: 0.300, cy: 0.760, r: 0.038 }, label: Label::Text("SEL"), contexts: BATTLE },
    Control { name: "a", key: Key::A, shape: Shape::Circle { cx: 0.640, cy: 0.900, r: 0.024 }, label: Label::Text("A"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "n", key: Key::N, shape: Shape::Circle { cx: 0.640, cy: 0.760, r: 0.024 }, label: Label::Text("N"), contexts: WORLD | SAFARI },
    // Menu keys.
    Control { name: "return", key: Key::Return, shape: Shape::Circle { cx: 0.500, cy: 0.945, r: 0.032 }, label: Label::Text("OK"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "escape", key: Key::Escape, shape: Shape::Circle { cx: 0.400, cy: 0.905, r: 0.026 }, label: Label::Text("ESC"), contexts: WORLD | BATTLE | SAFARI },
    // While a dialogue box is open the overlay leaves only this one button, so the text and the
    // portrait are not buried under anything else. Its position is measured from the game image (not
    // the window), so it sits on the box whatever the screen's shape is. It sends the key the game
    // advances dialogues with; a dialogue is only ever gone through forwards, so there is no cancel.
    Control { name: "dialogo", key: Key::Z, shape: Shape::Circle { cx: 0.940, cy: 0.880, r: 0.024 }, label: Label::Text("Z"), contexts: DIALOG },
    // The character picker of a new game: an arrow either side of the character, which is where the
    // eye already is. They are the only controls on that screen.
    Control { name: LEFT_ARROW_NAME, key: Key::LeftArrow, shape: Shape::Circle { cx: LEFT_ARROW_CX, cy: ARROW_CY, r: ARROW_R }, label: Label::Arrow(Dir::Left), contexts: CHARACTER },
    Control { name: RIGHT_ARROW_NAME, key: Key::RightArrow, shape: Shape::Circle { cx: RIGHT_ARROW_CX, cy: ARROW_CY, r: ARROW_R }, label: Label::Arrow(Dir::Right), contexts: CHARACTER },
    // The picker's own buttons, down the right edge: the screen is confirmed with enter and left
    // with the key its own text names ("Pulsa X para cancelar"). Both sit above the dialogue box,
    // clear of it, with the X set a little further up so the two do not crowd each other.
    Control { name: "eleccion_ok", key: Key::Return, shape: Shape::Circle { cx: 0.932, cy: 0.600, r: 0.036 }, label: Label::Text("OK"), contexts: CHARACTER },
    Control { name: "eleccion_atras", key: Key::X, shape: Shape::Circle { cx: 0.972, cy: 0.420, r: 0.028 }, label: Label::Text("X"), contexts: CHARACTER },
    // TEMP: the key combination the game listens for as a trick. Its `key` is the first of the two;
    // the pair is pressed together (see `control_key_down`). Centred along the very top, and drawn
    // smaller than the rest because its label is a wide one.
    Control { name: COMBO_BUTTON_NAME, key: Key::LeftControl, shape: Shape::Circle { cx: 0.500, cy: 0.060, r: 0.018 }, label: Label::Text("C+P"), contexts: CHARACTER },
    // Optional keys for the controls editor. `contexts: 0` means "no profile by default": the
    // generic `is_active` path only draws them when the layout file has an entry for the current
    // profile, so the player can add Shift, Alt, F3, H... without them cluttering the overlay.
    Control { name: "shift", key: Key::LeftShift, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("Shift"), contexts: 0 },
    Control { name: "alt", key: Key::LeftAlt, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("Alt"), contexts: 0 },
    Control { name: "ctrl", key: Key::LeftControl, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("Ctrl"), contexts: 0 },
    Control { name: "d", key: Key::D, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("D"), contexts: 0 },
    Control { name: "f3", key: Key::F3, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("F3"), contexts: 0 },
    Control { name: "h", key: Key::H, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("H"), contexts: 0 },
    Control { name: "k", key: Key::K, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("K"), contexts: 0 },
];
/// Our own (new-style) catalogue, left as it was.
pub const NEW_CONTROLS: &[Control] = &[
    // Number keys (first row along the top). They are for the base game only: in battle those keys
    // pick a party Pokémon, and that is what the SEL wheel below is for, so showing them there would
    // only get in the way.
    Control { name: "alpha1", key: Key::Alpha1, shape: Shape::Circle { cx: 0.300, cy: 0.055, r: 0.020 }, label: Label::Text("1"), contexts: WORLD | SAFARI },
    Control { name: "alpha2", key: Key::Alpha2, shape: Shape::Circle { cx: 0.372, cy: 0.055, r: 0.020 }, label: Label::Text("2"), contexts: WORLD | SAFARI },
    Control { name: "alpha3", key: Key::Alpha3, shape: Shape::Circle { cx: 0.444, cy: 0.055, r: 0.020 }, label: Label::Text("3"), contexts: WORLD | SAFARI },
    Control { name: "alpha5", key: Key::Alpha5, shape: Shape::Circle { cx: 0.516, cy: 0.055, r: 0.020 }, label: Label::Text("5"), contexts: WORLD | SAFARI },
    Control { name: "alpha7", key: Key::Alpha7, shape: Shape::Circle { cx: 0.588, cy: 0.055, r: 0.020 }, label: Label::Text("7"), contexts: WORLD | SAFARI },
    // Menu / misc keys (second row along the top).
    Control { name: "space", key: Key::Space, shape: Shape::Circle { cx: 0.300, cy: 0.145, r: 0.022 }, label: Label::Text("PKM"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: ROTATE_NAME, key: Key::Z, shape: Shape::Circle { cx: 0.935, cy: 0.150, r: 0.036 }, label: Label::Text("Girar"), contexts: 0 },
    Control { name: "f1", key: Key::F1, shape: Shape::Circle { cx: 0.935, cy: 0.300, r: 0.036 }, label: Label::Text("F1"), contexts: 0 },
    Control { name: "box_z", key: Key::Z, shape: Shape::Circle { cx: 0.865, cy: 0.851, r: 0.033 }, label: Label::Text("Z"), contexts: 0 },
    Control { name: "box_x", key: Key::X, shape: Shape::Circle { cx: 0.933, cy: 0.700, r: 0.033 }, label: Label::Text("X"), contexts: 0 },
    Control { name: "box_s", key: Key::S, shape: Shape::Circle { cx: 0.712, cy: 0.700, r: 0.026 }, label: Label::Text("S"), contexts: 0 },
    Control { name: "box_o", key: Key::O, shape: Shape::Circle { cx: 0.712, cy: 0.840, r: 0.026 }, label: Label::Text("O"), contexts: 0 },
    Control { name: "o", key: Key::O, shape: Shape::Circle { cx: 0.372, cy: 0.145, r: 0.022 }, label: Label::Text("O"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "m", key: Key::M, shape: Shape::Circle { cx: 0.444, cy: 0.145, r: 0.022 }, label: Label::Text("M"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "e", key: Key::E, shape: Shape::Circle { cx: 0.516, cy: 0.145, r: 0.022 }, label: Label::Text("E"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "t", key: Key::T, shape: Shape::Circle { cx: 0.588, cy: 0.145, r: 0.022 }, label: Label::Text("T"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "l", key: Key::L, shape: Shape::Circle { cx: 0.660, cy: 0.145, r: 0.022 }, label: Label::Text("L"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "p", key: Key::P, shape: Shape::Circle { cx: 0.732, cy: 0.145, r: 0.022 }, label: Label::Text("P"), contexts: WORLD | BATTLE | SAFARI },
    // Main actions, in the diamond a gamepad's face buttons use: the main one at the bottom, cancel
    // to the right, and the other two on the left and on top. The cluster is about the size of the
    // movement joystick and sits at the same height, so the two thumbs fall in the same place.
    Control { name: "z", key: Key::Z, shape: Shape::Circle { cx: 0.865, cy: 0.851, r: 0.033 }, label: Label::Text("Z"), contexts: WORLD | BATTLE | CHOICE | MENU_NAV },
    Control { name: "x", key: Key::X, shape: Shape::Circle { cx: 0.933, cy: 0.700, r: 0.033 }, label: Label::Text("X"), contexts: WORLD | BATTLE | CHOICE | MENU_NAV },
    Control { name: "c", key: Key::C, shape: Shape::Circle { cx: 0.797, cy: 0.700, r: 0.033 }, label: Label::Text("C"), contexts: WORLD | BATTLE },
    Control { name: "v", key: Key::V, shape: Shape::Circle { cx: 0.865, cy: 0.549, r: 0.033 }, label: Label::Text("V"), contexts: WORLD | BATTLE },
    // Secondary.
    Control { name: "b", key: Key::B, shape: Shape::Circle { cx: 0.712, cy: 0.840, r: 0.026 }, label: Label::Text("B"), contexts: BATTLE | SAFARI },
    Control { name: "s", key: Key::S, shape: Shape::Circle { cx: 0.712, cy: 0.700, r: 0.026 }, label: Label::Text("S"), contexts: BATTLE },
    // Holding this opens the party wheel (see below): each slot is one Pokémon of the team.
    Control { name: PARTY_NAME, key: Key::Alpha1, shape: Shape::Circle { cx: 0.300, cy: 0.760, r: 0.038 }, label: Label::Text("SEL"), contexts: BATTLE },
    Control { name: "a", key: Key::A, shape: Shape::Circle { cx: 0.640, cy: 0.900, r: 0.024 }, label: Label::Text("A"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "n", key: Key::N, shape: Shape::Circle { cx: 0.640, cy: 0.760, r: 0.024 }, label: Label::Text("N"), contexts: WORLD | SAFARI },
    // Menu keys.
    Control { name: "return", key: Key::Return, shape: Shape::Circle { cx: 0.500, cy: 0.945, r: 0.032 }, label: Label::Text("OK"), contexts: WORLD | BATTLE | SAFARI },
    Control { name: "escape", key: Key::Escape, shape: Shape::Circle { cx: 0.400, cy: 0.905, r: 0.026 }, label: Label::Text("ESC"), contexts: WORLD | BATTLE | SAFARI },
    // While a dialogue box is open the overlay leaves only this one button, so the text and the
    // portrait are not buried under anything else. Its position is measured from the game image (not
    // the window), so it sits on the box whatever the screen's shape is. It sends the key the game
    // advances dialogues with; a dialogue is only ever gone through forwards, so there is no cancel.
    Control { name: "dialogo", key: Key::Z, shape: Shape::Circle { cx: 0.940, cy: 0.880, r: 0.024 }, label: Label::Text("Z"), contexts: DIALOG },
    // The character picker of a new game: an arrow either side of the character, which is where the
    // eye already is. They are the only controls on that screen.
    Control { name: LEFT_ARROW_NAME, key: Key::LeftArrow, shape: Shape::Circle { cx: LEFT_ARROW_CX, cy: ARROW_CY, r: ARROW_R }, label: Label::Arrow(Dir::Left), contexts: CHARACTER },
    Control { name: RIGHT_ARROW_NAME, key: Key::RightArrow, shape: Shape::Circle { cx: RIGHT_ARROW_CX, cy: ARROW_CY, r: ARROW_R }, label: Label::Arrow(Dir::Right), contexts: CHARACTER },
    // The picker's own buttons, down the right edge: the screen is confirmed with enter and left
    // with the key its own text names ("Pulsa X para cancelar"). Both sit above the dialogue box,
    // clear of it, with the X set a little further up so the two do not crowd each other.
    Control { name: "eleccion_ok", key: Key::Return, shape: Shape::Circle { cx: 0.932, cy: 0.600, r: 0.036 }, label: Label::Text("OK"), contexts: CHARACTER },
    Control { name: "eleccion_atras", key: Key::X, shape: Shape::Circle { cx: 0.972, cy: 0.420, r: 0.028 }, label: Label::Text("X"), contexts: CHARACTER },
    // TEMP: the key combination the game listens for as a trick. Its `key` is the first of the two;
    // the pair is pressed together (see `control_key_down`). Centred along the very top, and drawn
    // smaller than the rest because its label is a wide one.
    Control { name: COMBO_BUTTON_NAME, key: Key::LeftControl, shape: Shape::Circle { cx: 0.500, cy: 0.060, r: 0.018 }, label: Label::Text("C+P"), contexts: CHARACTER },
    // Optional keys for the controls editor. `contexts: 0` means "no profile by default": the
    // generic `is_active` path only draws them when the layout file has an entry for the current
    // profile, so the player can add Shift, Alt, F3, H... without them cluttering the overlay.
    Control { name: "shift", key: Key::LeftShift, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("Shift"), contexts: 0 },
    Control { name: "alt", key: Key::LeftAlt, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("Alt"), contexts: 0 },
    Control { name: "ctrl", key: Key::LeftControl, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("Ctrl"), contexts: 0 },
    Control { name: "d", key: Key::D, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("D"), contexts: 0 },
    Control { name: "f3", key: Key::F3, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("F3"), contexts: 0 },
    Control { name: "h", key: Key::H, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("H"), contexts: 0 },
    Control { name: "k", key: Key::K, shape: Shape::Circle { cx: 0.500, cy: 0.500, r: 0.028 }, label: Label::Text("K"), contexts: 0 },
];
/// The catalogue the active overlay draws and hit-tests.
pub fn controls() -> &'static [Control] {
    if overlay_style() == OverlayStyle::Classic { CLASSIC_CONTROLS } else { NEW_CONTROLS }
}


/// TEMP: the button that presses the game's trick combination. The game wants both keys held at the
/// same time, so it is not an ordinary one-key control.
pub const COMBO_BUTTON_NAME: &str = "truco_ctrl_p";

/// Whether the picker's one-off trick button has already been used in this new game. It opens the
/// whole character roster, which only needs doing once, so it disappears after the first press and
/// is offered again only when a new game brings the picker back.
static COMBO_USED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_combo_used() {
    COMBO_USED.store(true, Ordering::Relaxed);
}

/// The room the character picker reports. Its own dialogues and its list of questions both live here.
const CHARACTER_ROOM: i32 = 3;

/// The modals the game draws, right after the character, as lists of options to pick from: the
/// preferences it asks about first and the difficulty right after. Both are walked with the movement
/// keys and answered with Z and X. Confirmed on the device: the picker's room 3 reports these ids
/// while each list is up, and nothing that is not one of them.
const PREFERENCES_MODAL: i32 = 7744;
const DIFFICULTY_MODAL: i32 = 7836;
const QUESTION_MODAL: i32 = 9673;
const LIST_MODAL: i32 = 7778;

/// The most characters the picker offers at once: one while choosing the player, two while choosing
/// the rival. Anything beyond that is the whole roster drawn at once by the game's own trick.
const PICKER_MAX_CHARACTERS: usize = 3;

/// The character picker's two arrows, and where they sit: one either side of the character.
const LEFT_ARROW_NAME: &str = "flechaizquierda";
const RIGHT_ARROW_NAME: &str = "flechaderecha";
const LEFT_ARROW_CX: f32 = 0.095;
const RIGHT_ARROW_CX: f32 = 0.430;
const ARROW_CY: f32 = 0.340;
const ARROW_R: f32 = 0.030;

/// How far from the character's edge the picker's arrows sit, as a fraction of the window width.
const ARROW_MARGIN: f32 = 0.045;

thread_local! {
    /// One box per character-sized sprite drawn this frame, in the game's own coordinates. The picker's
    /// arrows are placed from this, so they follow the character instead of sitting at a spot guessed
    /// in advance. Each candidate is kept apart: the arrows belong to the one that is selected, not
    /// around all of them.
    static CHARACTER_SPRITES: std::cell::RefCell<Vec<(f64, f64, f64, f64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Notes one drawn sprite that the renderer judged to be character-sized.
pub fn note_sprite(x1: f64, y1: f64, x2: f64, y2: f64) {
    CHARACTER_SPRITES.with(|cell| {
        let mut seen = cell.borrow_mut();
        // The same sprite is drawn more than once in a frame, and one character can be drawn again in
        // another way (a shadow, an outline). Anything overlapping a box already kept is the same
        // character, and counting it twice would make the arrows step twice over one of them.
        let overlaps = seen.iter().any(|other: &(f64, f64, f64, f64)| {
            x1 < other.2 && other.0 < x2 && y1 < other.3 && other.1 < y2
        });
        if !overlaps {
            seen.push((x1, y1, x2, y2));
        }
    });
}

/// Starts a frame with nothing noted.
pub fn reset_character_sprites() {
    CHARACTER_SPRITES.with(|cell| cell.borrow_mut().clear());
}

/// The candidate boxes of the frame, in game coordinates, left to right.
pub fn character_sprites() -> Vec<(f64, f64, f64, f64)> {
    CHARACTER_SPRITES.with(|cell| {
        let mut boxes = cell.borrow().clone();
        boxes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        boxes
    })
}

/// The candidates' boxes as fractions of the window, left to right, as the engine last worked them
/// out. On the picker's screen there are two of them (the rival's) or one (the player's).
static PICKER_BOXES: Mutex<Vec<(f32, f32, f32, f32)>> = Mutex::new(Vec::new());

pub fn set_picker_boxes(boxes: Vec<(f32, f32, f32, f32)>) {
    // A frame that draws no character at all is almost always a blink of the game's own redrawing
    // rather than the end of the picker. Keeping the last ones means the arrows stay where they are
    // instead of falling back to their guessed spot and out again on the next frame.
    if boxes.is_empty() && !picker_boxes().is_empty() {
        return
    }
    *PICKER_BOXES.lock().unwrap() = boxes;
}

fn picker_boxes() -> Vec<(f32, f32, f32, f32)> {
    PICKER_BOXES.lock().unwrap().clone()
}

/// Whether the game has any of its modals up - the text dialog, a question it asks, a summary. The
/// engine reports it from the game's own modal state, and the picker's controls step aside for all of
/// them: those screens are gone through with the game's own keyboard and buttons.
static MODAL_OPEN: Sticky = Sticky::new();

pub fn set_modal_open(open: bool) {
    MODAL_OPEN.report(open);
}

/// The id of the modal the game is drawing (-1 when there is none), kept only for the log.
///
/// The game gives every one of its screens a modal of its own, and that id is what tells them apart:
/// the text dialog it types a name in is one, the boxes it asks questions with are others, the lists
/// it makes you choose from are others again. Reading that id is the only exact way of saying which
/// screens should have the joystick and the two buttons and which should have nothing at all.
static MODAL_ID: AtomicI32 = AtomicI32::new(-1);

pub fn set_modal_id(id: i32) {
    if MODAL_ID.swap(id, Ordering::Relaxed) != id {
        eprintln!("[overlay] modal {id}");
    }
}

pub fn modal_open() -> bool {
    MODAL_OPEN.get()
}

/// The modal the game is drawing right now, as it reported it this frame (-1 when none).
pub fn modal_id() -> i32 {
    MODAL_ID.load(Ordering::Relaxed)
}

/// Which of the candidates the arrows are around. The buttons step it, so they move from one to the
/// other the way the game's own selection does.
static PICKER_TARGET: AtomicUsize = AtomicUsize::new(0);

pub fn picker_step(delta: isize) {
    let next = PICKER_TARGET.load(Ordering::Relaxed) as isize + delta;
    PICKER_TARGET.store(next.max(0) as usize, Ordering::Relaxed);
}

pub fn picker_reset() {
    PICKER_TARGET.store(0, Ordering::Relaxed);
}

/// The control's shape, with whatever the player moved or resized it to (see [set_placement]) and the
/// overall size of the new overlay on top. The renderer and the hit test share it, so what is drawn is
/// what is touched.
pub fn effective_shape(index: usize) -> Shape {
    let classic = overlay_style() == OverlayStyle::Classic;
    let control = &controls()[index];
    // The classic overlay has its own per-context layout (the positions/tamaños editor).
    if classic {
        if let Some(entry) = layout_override(control.name) {
            let scale = entry.scale * ui_scale();
            return match control.shape {
                Shape::Circle { r, .. } => Shape::Circle { cx: entry.x, cy: entry.y, r: r * scale },
                Shape::Rect { w, h, .. } => Shape::Rect { cx: entry.x, cy: entry.y, w: w * scale, h: h * scale },
            }
        }
    }
    let mut shape = control.shape;
    // The picker's arrows sit beside the character, placed from where the game drew it: that way they
    // follow the sprite, on the player's screen and on the rival's, where the two characters are a
    // good way apart. Once the player has placed an arrow by hand, their place wins.
    if (control.name == LEFT_ARROW_NAME || control.name == RIGHT_ARROW_NAME)
        && (classic || placement(control.name).is_none())
    {
        let boxes = picker_boxes();
        let target = PICKER_TARGET.load(Ordering::Relaxed).min(boxes.len().saturating_sub(1));
        if let Some(&(x1, y1, x2, y2)) = boxes.get(target) {
            let cx = if control.name == LEFT_ARROW_NAME { x1 - ARROW_MARGIN } else { x2 + ARROW_MARGIN };
            // Kept inside the window, so a character drawn at the edge cannot push its arrow off it.
            let inside = |v: f32| v.clamp(ARROW_R, 1.0 - ARROW_R);
            shape = Shape::Circle { cx: inside(cx), cy: inside((y1 + y2) / 2.0), r: ARROW_R };
        }
    }
    // Our own overlay's saved placement applies only to the new style; the overall size applies to
    // both, so the size slider works on the classic overlay too.
    if !classic {
        if let Some((cx, cy, scale)) = placement(control.name) {
            shape = match shape {
                Shape::Circle { r, .. } => Shape::Circle { cx, cy, r: r * scale },
                Shape::Rect { w, h, .. } => Shape::Rect { cx, cy, w: w * scale, h: h * scale },
            };
        }
    }
    scale_shape(shape, ui_scale())
}

#[derive(Clone, Copy, Debug)]
struct LayoutOverride {
    context: u8,
    name: &'static str,
    x: f32,
    y: f32,
    scale: f32,
    visible: bool,
}

static CLASSIC_LAYOUT: Mutex<Vec<LayoutOverride>> = Mutex::new(Vec::new());

/// The launcher saves each real context independently. Unknown controls and contexts cannot
/// introduce extra buttons. Invalid/missing files leave the original layout intact.
pub fn load_layout(text: &str) {
    let mut entries: Vec<LayoutOverride> = Vec::new();
    if text.len() <= 65536 && text.lines().next() == Some("touch-layout-v1") {
        for line in text.lines().skip(1) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() != 6 { continue }
            let context = match fields[0] {
                "world" => WORLD, "battle" => BATTLE, "dialog" => DIALOG, "character" => CHARACTER, "choice" => CHOICE, "safari" => SAFARI, "menu_nav" => MENU_NAV, "menu" => MENU, _ => continue,
            };
            let name = if fields[1] == JOYSTICK_NAME && context & (WORLD | BATTLE | CHOICE | MENU_NAV) != 0 {
                JOYSTICK_NAME
            } else if let Some(c) = CLASSIC_CONTROLS.iter().find(|c| c.name == fields[1]) {
                c.name
            } else { continue };
            let (Ok(x), Ok(y), Ok(scale)) = (fields[2].parse::<f32>(), fields[3].parse::<f32>(), fields[4].parse::<f32>()) else { continue };
            if !x.is_finite() || !y.is_finite() || !scale.is_finite() { continue }
            let visible = match fields[5] { "1" => true, "0" => false, _ => continue };
            entries.retain(|e| e.context != context || e.name != name);
            entries.push(LayoutOverride { context, name, x: x.clamp(0.0, 1.0), y: y.clamp(0.0, 1.0), scale: scale.clamp(0.5, 2.0), visible });
        }
    }
    *CLASSIC_LAYOUT.lock().unwrap() = entries;
}

fn layout_override(name: &str) -> Option<LayoutOverride> {
    let context = current_context();
    CLASSIC_LAYOUT.lock().unwrap().iter().find(|entry| entry.context == context && entry.name == name).copied()
        .or_else(|| profile_default(name, context))
}

fn profile_default(name: &str, context: u8) -> Option<LayoutOverride> {
    let (name, x, y) = match (context, name) {
        (WORLD, "space") => ("space", 0.300, 0.050),
        (WORLD, "o") => ("o", 0.372, 0.050),
        (WORLD, "m") => ("m", 0.444, 0.050),
        (WORLD, "e") => ("e", 0.516, 0.050),
        (WORLD, "t") => ("t", 0.588, 0.050),
        (WORLD, "l") => ("l", 0.660, 0.050),
        (WORLD, "p") => ("p", 0.732, 0.050),
        (WORLD, "n") => ("n", 0.645, 0.720),
        (WORLD, "a") => ("a", 0.645, 0.890),
        (BATTLE, "space") => ("space", 0.042, 0.180),
        (BATTLE, "o") => ("o", 0.096, 0.180),
        (BATTLE, "e") => ("e", 0.148, 0.180),
        // Level with the three on the left, clear of the party's status bars.
        (BATTLE, "t") => ("t", 0.440, 0.180),
        (BATTLE, "l") => ("l", 0.520, 0.180),
        (BATTLE, "p") => ("p", 0.600, 0.180),
        (BATTLE, "m") => ("m", 0.860, 0.330),
        (BATTLE, "a") => ("a", 0.755, 0.448),
        (BATTLE, "s") => ("s", 0.700, 0.650),
        _ => return None,
    };
    Some(LayoutOverride {context, name, x, y, scale: 1.0, visible: true})
}


/// Geometry and hit testing share the same saved position and scale.


/// Overall overlay opacity. At 100% the original artwork and colours are preserved.
/// Kept separate from hit testing, so faint controls still accept the same touches.
static OPACITY_PERCENT: AtomicU32 = AtomicU32::new(100);

pub fn set_opacity_percent(percent: u32) {
    OPACITY_PERCENT.store(percent.clamp(10, 100), Ordering::Relaxed);
}

pub fn opacity() -> f32 {
    OPACITY_PERCENT.load(Ordering::Relaxed) as f32 / 100.0
}

/// Overlay vertices contain x, y, r, g, b, alpha, u, v. Fade all batches alike,
/// including artwork, labels, the joystick and the party selector.
pub fn apply_opacity(vertices: &mut [f32]) {
    let alpha = opacity();
    for vertex in vertices.chunks_exact_mut(8) {
        vertex[5] *= alpha;
    }
}


/// The rectangle the game image occupies in the window (normalised), as the renderer last presented
/// it. Dialogue controls are placed inside it, so the button lands on the dialogue box whatever the
/// screen's shape is and whichever scaling mode is in use.
static VIEWPORT: Mutex<(f32, f32, f32, f32)> = Mutex::new((0.0, 0.0, 1.0, 1.0));

pub fn set_viewport(x: f32, y: f32, w: f32, h: f32) {
    *VIEWPORT.lock().unwrap() = (x, y, w, h);
}

fn viewport() -> (f32, f32, f32, f32) {
    *VIEWPORT.lock().unwrap()
}

/// A control's shape in window coordinates. Most controls are placed relative to the window (they
/// live around its edges, where the thumbs are); the dialogue button is placed relative to the game
/// image, because it has to sit on the dialogue box the game draws.
pub fn window_shape(index: usize) -> Shape {
    let shape = effective_shape(index);
    let classic = overlay_style() == OverlayStyle::Classic;
    let placed = if classic {
        layout_override(controls()[index].name).is_some()
    } else {
        placement(controls()[index].name).is_some()
    };
    // A control the player placed by hand is already in window coordinates; only the built-in ones
    // that ride on the game image (the dialogue button) are moved with it.
    if controls()[index].contexts & DIALOG == 0 || placed {
        return shape
    }
    let (vx, vy, vw, vh) = viewport();
    match shape {
        Shape::Circle { cx, cy, r } => Shape::Circle { cx: vx + cx * vw, cy: vy + cy * vh, r: r * vw },
        Shape::Rect { cx, cy, w, h } => Shape::Rect { cx: vx + cx * vw, cy: vy + cy * vh, w: w * vw, h: h * vh },
    }
}

pub fn window_shape_for(index: usize, ww: f32, wh: f32) -> Shape {
    let shape = window_shape(index);
    if overlay_style() != OverlayStyle::Classic { return shape }
    if ww <= 0.0 || wh <= 0.0 || layout_override(controls()[index].name).is_none() { return shape }
    match shape {
        Shape::Circle { cx, cy, r } => {
            let aspect = control_art_aspect(controls()[index].name).unwrap_or(1.0);
            let rx = r.min(0.49 / aspect).min(0.49 * wh / ww);
            let margin = rx * aspect;
            let ry = rx * ww / wh;
            Shape::Circle { cx: cx.clamp(margin, 1.0 - margin), cy: cy.clamp(ry, 1.0 - ry), r: rx }
        },
        Shape::Rect { cx, cy, w, h } => {
            let (w, h) = (w.min(0.98), h.min(0.98));
            Shape::Rect { cx: cx.clamp(w / 2.0, 1.0 - w / 2.0), cy: cy.clamp(h / 2.0, 1.0 - h / 2.0), w, h }
        },
    }
}

/// The inverse: a point in window coordinates, expressed in a control's own coordinates.
pub fn control_point(index: usize, nx: f32, ny: f32) -> (f32, f32) {
    let classic = overlay_style() == OverlayStyle::Classic;
    let placed = if classic {
        layout_override(controls()[index].name).is_some()
    } else {
        placement(controls()[index].name).is_some()
    };
    if controls()[index].contexts & DIALOG == 0 || placed {
        return (nx, ny)
    }
    let (vx, vy, vw, vh) = viewport();
    ((nx - vx) / vw.max(0.0001), (ny - vy) / vh.max(0.0001))
}

/// Looks a control up by its name.
pub fn index_of(name: &str) -> Option<usize> {
    controls().iter().position(|control| control.name == name)
}

/// Where a control sits when the player has not moved it. Used when the saved layout only carries a
/// size, and by the launcher's editor as its starting point.
pub fn default_center(name: &str) -> Option<(f32, f32)> {
    if name == MOVEMENT_NAME {
        return Some((JOYSTICK_CX, JOYSTICK_CY));
    }
    controls().iter().find(|control| control.name == name).map(|control| match control.shape {
        Shape::Circle { cx, cy, .. } => (cx, cy),
        Shape::Rect { cx, cy, .. } => (cx, cy),
    })
}

/// How big a control is when the player has not resized it.
pub fn default_size(name: &str) -> f32 {
    if name == MOVEMENT_NAME {
        return 1.0;
    }
    let size = controls().iter().find(|control| control.name == name).map(|control| match control.shape {
        Shape::Circle { r, .. } => r,
        Shape::Rect { w, .. } => w,
    });
    size.unwrap_or(0.03)
}

/// The room the emulator reported, or -1 before the first frame.
static ROOM: AtomicI32 = AtomicI32::new(-1);

/// Called by the emulator every frame with the current room id.
pub fn set_room(room: i32) {
    static LAST_ROOM: AtomicI32 = AtomicI32::new(i32::MIN);
    if LAST_ROOM.swap(room, Ordering::Relaxed) != room {
        // A new screen starts on its first character, which is where the game's own selection starts.
        picker_reset();
        // A fresh picker offers its one-off trick button again; using it once was enough last time.
        COMBO_USED.store(false, Ordering::Relaxed);
        #[cfg(target_os = "android")]
        if room == PUZZLE_ROOM {
            crate::android_main::show_puzzle_hint();
        }
    }
    ROOM.store(room, Ordering::Relaxed);
    log_state_change(room);
}

/// Whether the game has one of its dialogue boxes open. Reported every frame by the engine, which
/// reads it from the game's own modal state; the overlay then leaves a single button.
static DIALOGUE: Sticky = Sticky::new();

pub fn set_dialogue(open: bool) {
    if DIALOGUE.report(open) {
        eprintln!(
            "[overlay] dialogo {}",
            if open { "abierto: solo el boton de avanzar" } else { "cerrado" }
        );
    }
}

pub fn dialogue_open() -> bool {
    DIALOGUE.get()
}

/// Whether the game's own Pokémon box (the PC storage) is open. Its screen is driven entirely with
/// the game's own click boxes, so nothing of the overlay is wanted there; the engine reports it from
/// the game's `ENCAJA` global.
static BOX_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_box_open(open: bool) {
    BOX_OPEN.store(open, Ordering::Relaxed);
}

pub fn box_open() -> bool {
    BOX_OPEN.load(Ordering::Relaxed)
}

/// Whether the game has its own text-entry dialog open (the modal that draws the on-screen keyboard,
/// GML script 7754). The engine reports it every time it draws, and the Android side raises the
/// system keyboard while it is true.
static TEXT_ENTRY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_text_entry(open: bool) {
    let was = TEXT_ENTRY.swap(open, Ordering::Relaxed);
    if was != open {
        eprintln!("[ime] entrada de texto {}", if open { "abierta" } else { "cerrada" });
    }
}

pub fn text_entry_open() -> bool {
    TEXT_ENTRY.load(Ordering::Relaxed)
}

/// Height of the system keyboard, in window pixels, while it is up. The Android side reports it and
/// the renderer slides the game picture up with it, so the text being edited stays above the
/// keyboard instead of hiding under it.
static IME_INSET: AtomicU32 = AtomicU32::new(0);

pub fn set_ime_inset(pixels: u32) {
    IME_INSET.store(pixels, Ordering::Relaxed);
}

pub fn ime_inset() -> u32 {
    IME_INSET.load(Ordering::Relaxed)
}

/// The height the system keyboard is taking, but only while the game's text dialog is open.
///
/// Outside the text dialog the reported height is ignored on purpose. A height left behind by a
/// callback that never came would otherwise keep the game pushed up top and the on-screen controls
/// hidden, and the player would have no way of getting them back: tying it to the dialog means
/// closing the dialog always puts everything back where it was.
pub fn text_entry_inset() -> u32 {
    if text_entry_open() {
        ime_inset()
    } else {
        0
    }
}

/// Records every change of state, which is what decides which on-screen controls exist.
///
/// A report of buttons missing, or in the wrong place, cannot be chased without knowing what the
/// overlay believed at the time: which room, which group of controls it picked, and how many of
/// them it counted as active. It only writes when something changes, so it stays quiet while
/// playing.
#[cfg(target_os = "android")]
fn log_state_change(room: i32) {
    static LAST: AtomicU32 = AtomicU32::new(u32::MAX);
    static LAST_ROOM: AtomicI32 = AtomicI32::new(i32::MIN);
    let group = u32::from(current_context());
    let room_changed = LAST_ROOM.swap(room, Ordering::Relaxed) != room;
    if LAST.swap(group, Ordering::Relaxed) == group {
        // The group did not change, but knowing which room we are in is what makes it possible to
        // say "this screen needs no controls" about a specific one.
        if room_changed {
            eprintln!("[overlay] sala {} -> grupo {}", room, group);
        }
        return
    }
    let (window_w, window_h) = ramen::platform::android::window_size();
    let active = (0..controls().len()).filter(|index| is_active(*index)).count();
    eprintln!("[overlay] sala {} -> grupo {} | ventana {}x{} | {} de {} activos", room, group, window_w, window_h, active, controls().len());
    // Every control, one per line, so a report of "buttons missing" can be checked against what the
    // overlay actually decided: which key, where it was put, how big it is, and whether it counted
    // as visible. Normalised coordinates are fractions of the window, so they can be compared
    // between devices of any size.
    for (index, control) in controls().iter().enumerate() {
        // The shape actually in use, not the table's: a control that is moved to follow the game (the
        // picker's arrows) has to be reported where it really is.
        let (at, size) = match effective_shape(index) {
            Shape::Circle { cx, cy, r } => (format!("{:.3},{:.3}", cx, cy), format!("r={:.3}", r)),
            Shape::Rect { cx, cy, w, h } => (format!("{:.3},{:.3}", cx, cy), format!("{:.3}x{:.3}", w, h)),
        };
        let name = match control.label {
            Label::None => "",
            Label::Arrow(_) => "flecha",
            Label::Text(text) => text,
        };
        eprintln!(
            "[overlay]   {:>2}. {:<6} {:<12} en ({}) {} -> {}",
            index,
            name,
            format!("{:?}", control.key),
            at,
            size,
            if is_active(index) { "visibles" } else { "ocultos" }
        );
    }
}

/// Nothing to report where there is no overlay.
#[cfg(not(target_os = "android"))]
fn log_state_change(_room: i32) {}

/// Which group of controls the current game state needs.
pub fn current_context() -> u8 {
    // A dialogue box takes over the overlay: only the buttons that go with it are shown, so the text
    // and the portrait stay readable.
    if dialogue_open() {
        return DIALOG
    }
    // The Pokémon box is entirely point-and-click, so nothing is needed next to it: no controls at
    // all. MENU is the context no control belongs to, which is exactly that.
    if box_open() {
        return MENU
    }
    // The system keyboard is up. It owns the bottom of the screen and the controls would sit under
    // it, so the whole overlay steps aside - buttons and joystick - until it is gone. Typing needs
    // no buttons, and the game's own dialogs are advanced with the keyboard's own key.
    if text_entry_inset() > 0 {
        return MENU
    }
    // The game's yes/no questions are answered by touching their Sí/No boxes: the buttons would
    // only cover them. Confirmed on the device: the exit-game and the escape question both report
    // 9673, and 9674 is the bigger variant. Read straight from the current frame (not the sticky
    // state) so the buttons come back as soon as the question closes instead of after a grace.
    if matches!(modal_id(), QUESTION_MODAL | 9674) {
        return MENU
    }
    // The game's option lists are navigated with the joystick and the entries are touched directly.
    if modal_id() == LIST_MODAL || crate::touch_navigation::bag_open() {
        return MENU_NAV
    }
    match ROOM.load(Ordering::Relaxed) {
        // IntroROOM, InicioROOM, CargarPartida, MultijugadorMenu, CreditosInicioScreen, OptionsROOM:
        // these screens are fully clickable (once the game's own click boxes are fixed, see
        // `crate::patches`), so they need no on-screen controls at all.
        //
        // 528 is the Oak laboratory's introduction and 527 the unknown place that follows it: both
        // are cutscenes the player watches before they can take a step, so nothing of the map's
        // overlay belongs over them. Their dialogues still get their own button, handled above.
        // Confirmed on the device - the two report as rooms 528 and 527.
        22 | 30 | 43 | 461 | 465 | 466 | 527 | 528 => MENU,
        // The screen where a new game asks you to pick your character, just after the title menu. It
        // has controls of its own (the two arrows) instead of the map's. Its room cannot be named
        // from the game's data - the names are not readable - so this is the room it reports.
        //
        // A modal of its own - the preferences it asks about next, drawn as a list over the same
        // screen - is walked with the movement keys and answered with Z and X, so it gets those and
        // the joystick. A dialogue here is handled above and stays on the dialogue's two buttons; the
        // name the game types in is handled by the keyboard, so it gets nothing.
        CHARACTER_ROOM => {
            if text_entry_open() {
                MENU
            } else if matches!(modal_id(), PREFERENCES_MODAL | DIFFICULTY_MODAL) {
                CHOICE
            } else {
                CHARACTER
            }
        },
        // BattleROOM, BattleROOMAqua, BattleROOMLava.
        47 | 191 | 709 => BATTLE,
        // The Safari Zone's catching room. Confirmed on the device: it reports room 715 while the
        // ball is being thrown there. It keeps the map's buttons bar the stick and the attacks.
        715 => SAFARI,
        // The puzzle uses only its three buttons; everything else steps aside.
        PUZZLE_ROOM => MENU,
        // Before the first frame we don't know the room yet; show nothing rather than flashing the
        // whole overlay for one frame.
        -1 => MENU,
        _ => WORLD,
    }
}

/// True while the player is on a pad. The on-screen controls then step aside, because somebody
/// holding a controller does not want buttons under their thumbs; the first touch on the screen
/// brings them back.
static PAD_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_pad_active(active: bool) {
    PAD_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn pad_active() -> bool {
    PAD_ACTIVE.load(Ordering::Relaxed)
}

static INFO_WINDOW_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_info_window(open: bool) {
    INFO_WINDOW_OPEN.store(open, Ordering::Relaxed);
}

pub fn info_window_open() -> bool {
    INFO_WINDOW_OPEN.load(Ordering::Relaxed)
}

pub fn is_active(index: usize) -> bool {
    if overlay_style() == OverlayStyle::None {
        return false
    }
    if ROOM.load(Ordering::Relaxed) == PUZZLE_ROOM
        && matches!(controls()[index].name, ROTATE_NAME | "f1" | "x")
    {
        return !pad_active() && !info_window_open()
    }
    if matches!(controls()[index].name, "box_z" | "box_x" | "box_s" | "box_o") {
        return box_open()
            && !pad_active()
            && !info_window_open()
            && !layout_override(controls()[index].name).is_some_and(|entry| !entry.visible)
    }
    let classic = overlay_style() == OverlayStyle::Classic;
    if classic {
        if current_context() == WORLD && controls()[index].name.starts_with("alpha") { return false }
        if controls()[index].name == crate::field_controls::NAME && current_context() != WORLD { return false }
        if controls()[index].name == PARTY_NAME && current_context() != BATTLE { return false }
        if layout_override(controls()[index].name).is_some_and(|entry| !entry.visible) { return false }
    }
    if pad_active() || info_window_open()
        || (controls()[index].contexts & current_context() == 0
            && !(classic && layout_override(controls()[index].name).is_some()))
    {
        return false
    }
    // The picker's trick button opens the whole roster once, and then has done its job: it is taken
    // away rather than left over the screen for the rest of the game. It comes back with the next new
    // game (see [set_room]).
    if controls()[index].name == COMBO_BUTTON_NAME && COMBO_USED.load(Ordering::Relaxed) {
        return false
    }
    // The picker's own controls belong to the moment the game is showing the characters to choose
    // from. With any of its modals up - the name dialog, the question it asks to check the data, a
    // summary - nothing of the picker belongs on top: those screens are gone through with the game's
    // own keyboard and buttons.
    if current_context() == CHARACTER && controls()[index].contexts & CHARACTER != 0 {
        if modal_open() {
            return false
        }
        // The two arrows travel with the characters, so they only belong where those are being chosen:
        // one (the player) or two (the rival). The game's own trick puts up a screen with the whole
        // roster at once, drawn as a grid; the boxes churn from frame to frame there and the arrows
        // would hop about, so they sit that one out. The rest of the controls stay put: they do not
        // follow anything.
        let candidates = picker_boxes().len();
        if matches!(controls()[index].label, Label::Arrow(_)) && !(1..=PICKER_MAX_CHARACTERS).contains(&candidates) {
            return false
        }
    }
    true
}

/// The movement control (joystick or D-pad) drives the arrow keys, which is how the game's menus (the
/// ones that still show controls) and the overworld are navigated.
pub fn movement_active() -> bool {
    if overlay_style() == OverlayStyle::Classic { return joystick_active() }
    overlay_style() != OverlayStyle::None
        && !pad_active()
        && !info_window_open()
        && current_context() & (WORLD | BATTLE | CHOICE | MENU_NAV) != 0
}

/// The joystick drives the arrow keys. Used by the classic overlay.
pub fn joystick_active() -> bool {
    !pad_active() && !info_window_open() && joystick_visible()
        && overlay_style() != OverlayStyle::None
        && current_context() & (WORLD | BATTLE | CHOICE | MENU_NAV) != 0
}

/// Returns the index of the control containing the point, if any. The point is normalised, but the
/// test is done in pixels so the touchable area matches what is drawn on a screen that is not square.
pub fn hit(nx: f32, ny: f32, ww: f32, wh: f32) -> Option<usize> {
    controls()
        .iter()
        .enumerate()
        .position(|(index, control)| is_active(index) && control_contains(index, control.name, nx, ny, ww, wh))
}

/// Whether a point is inside a control. A control with artwork is tested against the rectangle its
/// picture was actually drawn into, so a wide label is as touchable as it is painted instead of only
/// inside the little circle its key is defined with. The rest keep their own shape's test.
fn control_contains(index: usize, name: &str, nx: f32, ny: f32, ww: f32, wh: f32) -> bool {
    let shape = window_shape_for(index, ww, wh);
    if let Some(aspect) = control_art_aspect(name) {
        let (cx, cy, half) = match shape {
            Shape::Circle { cx, cy, r } => (cx, cy, r * ww),
            Shape::Rect { cx, cy, w, h } => (cx, cy, (w * ww + h * wh) / 4.0),
        };
        // The renderer sizes every picture by its height and lets its width follow its own aspect.
        return (nx - cx).abs() * ww <= half * aspect && (ny - cy).abs() * wh <= half
    }
    contains(shape, nx, ny, ww, wh)
}

/// The shape each control's artwork was painted in (width over height), as the renderer packed it.
/// Empty until the skin is loaded, and only used to widen the touch area of a picture that is not as
/// tall as it is wide.
static SPRITE_OPACITY: Mutex<f32> = Mutex::new(0.5);

pub fn set_sprite_opacity(value: f32) {
    *SPRITE_OPACITY.lock().unwrap() = value.clamp(0.05, 1.0);
}

pub fn sprite_opacity() -> f32 {
    *SPRITE_OPACITY.lock().unwrap()
}

/// Which overlay the player wants drawn.
#[derive(Clone, Copy, PartialEq)]
pub enum OverlayStyle {
    /// The new, semi-transparent controls drawn as smooth shapes. This is the default.
    New,
    /// The original dynamic controls: the pixel-art skin when it is there, plain shapes otherwise.
    Classic,
    /// No on-screen controls at all; the game is played with a pad or by touching the game itself.
    None,
}

static STYLE: AtomicU8 = AtomicU8::new(1);

pub fn set_overlay_style(style: OverlayStyle) {
    STYLE.store(
        match style {
            OverlayStyle::New => 0,
            OverlayStyle::Classic => 1,
            OverlayStyle::None => 2,
        },
        Ordering::Relaxed,
    );
}

pub fn overlay_style() -> OverlayStyle {
    match STYLE.load(Ordering::Relaxed) {
        1 => OverlayStyle::Classic,
        2 => OverlayStyle::None,
        _ => OverlayStyle::New,
    }
}

/// What drives the arrow keys: the analog joystick or a four-way pad.
#[derive(Clone, Copy, PartialEq)]
pub enum MoveMode {
    Joystick,
    Dpad,
}

static MOVE: AtomicU8 = AtomicU8::new(0);

pub fn set_move_mode(mode: MoveMode) {
    MOVE.store(if mode == MoveMode::Dpad { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn move_mode() -> MoveMode {
    // Moderno always moves with the joystick; the choice only exists for the classic look.
    if crate::touch_appearance::settings().style == crate::touch_appearance::Style::Modern {
        return MoveMode::Joystick
    }
    if MOVE.load(Ordering::Relaxed) == 1 {
        MoveMode::Dpad
    } else {
        MoveMode::Joystick
    }
}

/// The name the movement control (joystick or D-pad) is saved under, both in the layout and in the
/// launcher's editor. It is not one of [controls()]: it is drawn and hit-tested on its own.
pub const MOVEMENT_NAME: &str = "dpad";

/// Where the player moved and resized a control, keyed by its [Control::name]. Only the entries the
/// player actually touched are stored, so the rest keep the places built into the table.
static NEW_LAYOUT: Mutex<Vec<(String, f32, f32, f32)>> = Mutex::new(Vec::new());

pub fn set_placement(name: &str, cx: f32, cy: f32, scale: f32) {
    let mut layout = NEW_LAYOUT.lock().unwrap();
    if let Some(entry) = layout.iter_mut().find(|entry| entry.0 == name) {
        *entry = (name.to_string(), cx, cy, scale);
    } else {
        layout.push((name.to_string(), cx, cy, scale));
    }
}

pub fn clear_layout() {
    NEW_LAYOUT.lock().unwrap().clear();
}

fn placement(name: &str) -> Option<(f32, f32, f32)> {
    NEW_LAYOUT.lock().unwrap().iter().find(|entry| entry.0 == name).map(|entry| (entry.1, entry.2, entry.3))
}

/// A size multiplier applied to every control of the new overlay.
static UI_SCALE: Mutex<f32> = Mutex::new(1.0);

pub fn set_ui_scale(scale: f32) {
    *UI_SCALE.lock().unwrap() = scale.clamp(0.3, 3.0);
}

pub fn ui_scale() -> f32 {
    *UI_SCALE.lock().unwrap()
}

/// Multiplies the size part of a shape, leaving its centre where it is.
fn scale_shape(shape: Shape, scale: f32) -> Shape {
    match shape {
        Shape::Circle { cx, cy, r } => Shape::Circle { cx, cy, r: r * scale },
        Shape::Rect { cx, cy, w, h } => Shape::Rect { cx, cy, w: w * scale, h: h * scale },
    }
}

/// The shape each control's artwork was painted in (width over height), as the renderer packed it.
/// Empty until the skin is loaded, and only used to widen the touch area of a picture that is not as
/// tall as it is wide.
static ART_ASPECTS: Mutex<Vec<(&'static str, f32)>> = Mutex::new(Vec::new());

pub fn set_art_aspect(name: &'static str, aspect: f32) {
    let mut aspects = ART_ASPECTS.lock().unwrap();
    aspects.retain(|(known, _)| *known != name);
    aspects.push((name, aspect));
}

pub fn control_art_aspect(name: &str) -> Option<f32> {
    ART_ASPECTS.lock().unwrap().iter().find(|(known, _)| *known == name).map(|(_, aspect)| *aspect)
}

static PRESSED: AtomicU64 = AtomicU64::new(0);

pub fn set_pressed(index: usize, pressed: bool) {
    if is_pressed(index) != pressed { crate::touch_appearance::set_pressed(index, pressed); }
    if pressed {
        PRESSED.fetch_or(1 << index, Ordering::Relaxed);
    } else {
        PRESSED.fetch_and(!(1 << index), Ordering::Relaxed);
    }
}

pub fn is_pressed(index: usize) -> bool {
    PRESSED.load(Ordering::Relaxed) & (1 << index) != 0
}

/// Controls that are pressed but no longer valid, so the caller can release their keys instead of
/// leaving one stuck down when the game changes state.
pub fn stale_pressed() -> Vec<usize> {
    (0..controls().len()).filter(|&index| is_pressed(index) && !is_active(index)).collect()
}

// ---------------------------------------------------------------------------------------------
// Movement (joystick or D-pad). It presses the arrow keys depending on which way it is pushed.

pub const JOYSTICK_CX: f32 = 0.135;
pub const JOYSTICK_CY: f32 = 0.700;
/// Radius as a fraction of the window *height*, so the base stays round on screen.
pub const JOYSTICK_R: f32 = 0.200;
/// Dead zone, as a fraction of the radius, before a direction registers.
pub const JOYSTICK_DEAD: f32 = 0.35;

/// The movement control's centre, in normalised coordinates.
pub fn movement_center() -> (f32, f32) {
    if overlay_style() == OverlayStyle::Classic { return joystick_center() }
    placement(MOVEMENT_NAME).map(|(cx, cy, _)| (cx, cy)).unwrap_or((JOYSTICK_CX, JOYSTICK_CY))
}

/// Radius as a fraction of the window height.
pub fn movement_radius() -> f32 {
    if overlay_style() == OverlayStyle::Classic { return joystick_radius() }
    let scale = placement(MOVEMENT_NAME).map(|(_, _, scale)| scale).unwrap_or(1.0);
    JOYSTICK_R * scale * ui_scale()
}

/// The joystick's centre, in normalised coordinates.
pub fn joystick_center() -> (f32, f32) {
    layout_override(JOYSTICK_NAME).map_or((JOYSTICK_CX, JOYSTICK_CY), |entry| (entry.x, entry.y))
}

pub fn joystick_center_in_window(ww: f32, wh: f32) -> (f32, f32) {
    let (cx, cy) = joystick_center();
    if ww <= 0.0 || wh <= 0.0 || layout_override(JOYSTICK_NAME).is_none() { return (cx, cy) }
    let ry = (joystick_radius() * 1.05).min(0.49);
    let rx = (ry * wh / ww).min(0.49);
    (cx.clamp(rx, 1.0 - rx), cy.clamp(ry, 1.0 - ry))
}

/// Radius as a fraction of the window height.
pub fn joystick_radius() -> f32 {
    JOYSTICK_R * layout_override(JOYSTICK_NAME).map_or(1.0, |entry| entry.scale)
}

pub fn joystick_visible() -> bool {
    layout_override(JOYSTICK_NAME).map_or(true, |entry| entry.visible)
}

pub fn movement_hit(nx: f32, ny: f32, ww: f32, wh: f32) -> bool {
    if !movement_active() {
        return false
    }
    let (cx, cy) = if overlay_style() == OverlayStyle::Classic { joystick_center_in_window(ww, wh) } else { movement_center() };
    let r = if overlay_style() == OverlayStyle::Classic { joystick_radius() } else { movement_radius() } * wh;
    let dx = (nx - cx) * ww;
    let dy = (ny - cy) * wh;
    dx * dx + dy * dy <= r * r
}

/// `[up, down, left, right]` to hold for the movement control at (nx, ny).
pub fn movement_dirs(nx: f32, ny: f32, ww: f32, wh: f32) -> [bool; 4] {
    let classic = overlay_style() == OverlayStyle::Classic;
    let (cx, cy) = if classic { joystick_center_in_window(ww, wh) } else { movement_center() };
    let r = if classic { joystick_radius() } else { movement_radius() } * wh;
    let dx = (nx - cx) * ww / r;
    let dy = (ny - cy) * wh / r;
    // The D-pad is four discrete zones, not an analog stick: the strongest arm wins and the centre
    // does nothing. That is what makes it feel (and light up) like a real cross.
    if move_mode() == MoveMode::Dpad {
        const DPAD_DEAD: f32 = 0.26;
        if dx.abs().max(dy.abs()) < DPAD_DEAD {
            return [false; 4]
        }
        return if dx.abs() > dy.abs() {
            [false, false, dx < 0.0, dx > 0.0]
        } else {
            [dy < 0.0, dy > 0.0, false, false]
        }
    }
    stick_dirs(dx, dy)
}

/// Which directions a stick pushed by (dx, dy) is asking for: up, down, left, right. Both are in
/// the range -1..1, and this is the same dead zone the on-screen joystick uses - a physical pad's
/// stick goes through here too, so both feel identical.
pub fn stick_dirs(dx: f32, dy: f32) -> [bool; 4] {
    [
        dy < -JOYSTICK_DEAD,
        dy > JOYSTICK_DEAD,
        dx < -JOYSTICK_DEAD,
        dx > JOYSTICK_DEAD,
    ]
}

/// The stick's offset from its centre, in units of its radius (for drawing the knob).
static MOVEMENT_OFFSET: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));

pub fn set_movement_offset(dx: f32, dy: f32) {
    *MOVEMENT_OFFSET.lock().unwrap() = (dx, dy);
}

pub fn movement_offset() -> (f32, f32) {
    *MOVEMENT_OFFSET.lock().unwrap()
}

/// Which directions the movement control is pressing right now, so the D-pad can light up the arm.
static MOVE_DIRS: AtomicU32 = AtomicU32::new(0);

pub fn set_move_dirs(dirs: [bool; 4]) {
    let mut bits = 0u32;
    for (i, on) in dirs.iter().enumerate() {
        if *on {
            bits |= 1 << i;
        }
    }
    MOVE_DIRS.store(bits, Ordering::Relaxed);
}

pub fn move_dirs() -> [bool; 4] {
    let bits = MOVE_DIRS.load(Ordering::Relaxed);
    [bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0]
}

/// Point-in-shape, in pixels so it matches what is drawn.
pub fn contains(shape: Shape, nx: f32, ny: f32, ww: f32, wh: f32) -> bool {
    match shape {
        Shape::Circle { cx, cy, r } => {
            let (dx, dy) = ((nx - cx) * ww, (ny - cy) * wh);
            let radius = r * ww;
            dx * dx + dy * dy <= radius * radius
        },
        Shape::Rect { cx, cy, w, h } => (nx - cx).abs() * ww <= w * ww / 2.0 && (ny - cy).abs() * wh <= h * wh / 2.0,
    }
}

// ---------------------------------------------------------------------------------------------
// Party data. The game keeps it in `global.pkmn1[0..5]` (each slot's species); `game.rs` reads it
// and drops it here, so the overlay can draw each Pokémon's own sprite in its hexagon.

/// Name of the button that opens the party wheel. It presses no key of its own. It deliberately has
/// no `pkmn.png` of its own: the honeycomb artwork belongs to the wheel, not to the button.
pub const PARTY_NAME: &str = "pkmn";

pub const PICKER_SLOTS: usize = 6;

/// The key each slot presses: the game's 1..6.
pub const PICKER_KEYS: [Key; PICKER_SLOTS] =
    [Key::Alpha1, Key::Alpha2, Key::Alpha3, Key::Alpha4, Key::Alpha5, Key::Alpha6];

static PARTY_SPECIES: Mutex<Option<[i32; PICKER_SLOTS]>> = Mutex::new(None);
static PARTY_VERSION: AtomicU64 = AtomicU64::new(0);

pub fn set_party(species: [i32; PICKER_SLOTS]) {
    let changed = PARTY_SPECIES.lock().unwrap().as_ref() != Some(&species);
    *PARTY_SPECIES.lock().unwrap() = Some(species);
    if changed {
        // The renderer watches this to know when the GIFs under the slots changed.
        PARTY_VERSION.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn party_species() -> [i32; PICKER_SLOTS] {
    PARTY_SPECIES.lock().unwrap().unwrap_or([0; PICKER_SLOTS])
}

pub fn party_version() -> u64 {
    PARTY_VERSION.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------------------------
// Party wheel. Holding the selector button opens the team around the middle of the screen, like the
// weapon wheel in a certain game: slide towards the one you want and let go. Each step sends the key
// the game uses to pick, so its own screen highlights the same one as you move.

static WHEEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WHEEL_SEL: AtomicI32 = AtomicI32::new(-1);

pub fn wheel_open() -> bool {
    WHEEL.load(Ordering::Relaxed) && index_of(PARTY_NAME).is_some_and(is_active)
}

pub fn wheel_selected() -> i32 {
    WHEEL_SEL.load(Ordering::Relaxed)
}

pub fn set_wheel_open(open: bool) {
    WHEEL.store(open, Ordering::Relaxed);
    if !open {
        WHEEL_SEL.store(-1, Ordering::Relaxed);
    }
}

pub fn set_wheel_selected(index: i32) {
    WHEEL_SEL.store(index, Ordering::Relaxed);
}

/// The wheel's centre, in pixels: the middle of the screen, since the honeycomb needs the room.
pub fn wheel_center(ww: f32, wh: f32) -> (f32, f32) {
    (ww * 0.5, wh * 0.5)
}

/// How far out the slots sit, in pixels: the honeycomb is three of these wide.
pub fn wheel_radius(ww: f32, wh: f32) -> f32 {
    ww.min(wh) * 0.21
}

/// Where each of the honeycomb's six slots sits, taken from the shape of the artwork (79x74). A
/// honeycomb of seven hexagons is 2(1+√3) radii wide and 5 radii tall, so its hexagons have a
/// circumradius of 79/2/(1+√3) ≈ 14.5: each side slot sits √3 radii out and each diagonal
/// (√3/2, 1.5) radii out. Horizontal offsets are fractions of the width, vertical of the height,
/// which is how the renderer scales it.
const WHEEL_OFFSETS: [(f32, f32); PICKER_SLOTS] = [
    (-0.159, -0.293), // up-left
    (0.159, -0.293),  // up-right
    (-0.317, 0.0),    // left
    (0.317, 0.0),     // right
    (-0.159, 0.293),  // down-left
    (0.159, 0.293),   // down-right
];

/// How far a slot's centre sits from the selector's, as a fraction of the artwork's half-width. The
/// renderer sizes the artwork with this so its hexagons land exactly on the slots.
pub const SELECTOR_SLOT_DISTANCE: f32 = 0.634;

/// The artwork is very slightly wider than it is tall; the renderer keeps that when scaling it.
pub const SELECTOR_ASPECT: f32 = 74.0 / 79.0;

/// The centre of one of the wheel's slots, in pixels.
pub fn wheel_item_center(index: usize, ww: f32, wh: f32) -> (f32, f32) {
    let (cx, cy) = wheel_center(ww, wh);
    let half = wheel_radius(ww, wh) / SELECTOR_SLOT_DISTANCE;
    let (fx, fy) = WHEEL_OFFSETS[index];
    // The artwork is scaled by its width, so its vertical distances have to follow the same squeeze.
    (cx + fx * 2.0 * half, cy + fy * 2.0 * half * SELECTOR_ASPECT)
}

/// Which slot the finger is pointing at, once it is far enough from the middle.
pub fn wheel_item_at(nx: f32, ny: f32, ww: f32, wh: f32) -> Option<usize> {
    let (cx, cy) = wheel_center(ww, wh);
    let half = wheel_radius(ww, wh) / SELECTOR_SLOT_DISTANCE;
    let (fx, fy) = ((nx * ww - cx) / (2.0 * half), (ny * wh - cy) / (2.0 * half * SELECTOR_ASPECT));
    if fx * fx + fy * fy < 0.10 {
        return None
    }
    let mut best = None;
    let mut closest = f32::MAX;
    for (index, (ox, oy)) in WHEEL_OFFSETS.iter().enumerate() {
        let apart = (fx - ox).powi(2) + (fy - oy).powi(2);
        if apart < closest {
            closest = apart;
            best = Some(index);
        }
    }
    best
}

#[cfg(test)]
pub fn expire_context_grace() {
    *DIALOGUE.seen.lock().unwrap() = None;
    *MODAL_OPEN.seen.lock().unwrap() = None;
}

/// Different visible controls may share a key after adding buttons to another profile.
pub fn control_keys(index: usize) -> [Option<Key>; 2] {
    if controls()[index].name == COMBO_BUTTON_NAME { [Some(Key::LeftControl),Some(Key::P)] }
    else { [Some(controls()[index].key),None] }
}
pub fn key_held_by_buttons(key: Key) -> bool {
    (0..controls().len()).any(|i| is_pressed(i) && control_keys(i).contains(&Some(key)))
}
