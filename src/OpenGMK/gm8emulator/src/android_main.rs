//! Glue between the Android activity and ramen's Android platform backend.
//!
//! `android-activity` hands us an [`InputEvent`] enum; everything it reports is translated
//! into ramen's `InputEvent` queue so that `Window::poll_events()` can drain it. Only the
//! public wrapper API is used, so this doesn't chase ndk's FFI shape between versions.

use std::time::{Duration, Instant};

use std::collections::HashMap;
use std::sync::Mutex;

use android_activity::{
    input::{Axis, InputEvent, KeyAction, MotionAction},
    AndroidApp, PollEvent,
};

use ramen::{input::Key, platform::android};

/// `AKEYCODE_*`, from <android/keycodes.h>. Letters are contiguous from `A`.
type AKeyCode = i32;
const AKEYCODE_A: AKeyCode = 29;
const AKEYCODE_DEL: AKeyCode = 67;
const AKEYCODE_TAB: AKeyCode = 61;
const AKEYCODE_ENTER: AKeyCode = 66;
const AKEYCODE_SPACE: AKeyCode = 62;
const AKEYCODE_DPAD_UP: AKeyCode = 19;
const AKEYCODE_DPAD_DOWN: AKeyCode = 20;
const AKEYCODE_DPAD_LEFT: AKeyCode = 21;
const AKEYCODE_DPAD_RIGHT: AKeyCode = 22;
const AKEYCODE_ESCAPE: AKeyCode = 111;

/// Gamepad buttons. Android reports the same keycodes for every pad, whatever the brand, so the
/// names below are the physical ones the player sees.
const AKEYCODE_BUTTON_A: AKeyCode = 96;
const AKEYCODE_BUTTON_B: AKeyCode = 97;
const AKEYCODE_BUTTON_X: AKeyCode = 99;
const AKEYCODE_BUTTON_Y: AKeyCode = 100;
const AKEYCODE_BUTTON_L1: AKeyCode = 102;
const AKEYCODE_BUTTON_R1: AKeyCode = 103;
const AKEYCODE_BUTTON_L2: AKeyCode = 104;
const AKEYCODE_BUTTON_R2: AKeyCode = 105;
const AKEYCODE_BUTTON_THUMBL: AKeyCode = 106;
const AKEYCODE_BUTTON_THUMBR: AKeyCode = 107;
const AKEYCODE_BUTTON_START: AKeyCode = 108;
const AKEYCODE_BUTTON_SELECT: AKeyCode = 109;

const LETTERS: [Key; 26] = [
    Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L,
    Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X,
    Key::Y, Key::Z,
];

fn map_key(keycode: AKeyCode) -> Option<Key> {
    // A layout saved by the launcher's control screen wins over the built-in one; anything it
    // doesn't mention keeps the default below.
    if let Some(map) = std::sync::OnceLock::get(&MAPPED_BUTTONS) {
        if let Some(mapped) = map.lock().ok().and_then(|map| map.get(&keycode).copied()) {
            return Some(mapped)
        }
    }
    Some(match keycode {
        AKEYCODE_DEL => Key::Backspace,
        AKEYCODE_TAB => Key::Tab,
        AKEYCODE_ENTER => Key::Return,
        AKEYCODE_SPACE => Key::Space,
        AKEYCODE_ESCAPE => Key::Escape,
        AKEYCODE_DPAD_UP => Key::UpArrow,
        AKEYCODE_DPAD_DOWN => Key::DownArrow,
        AKEYCODE_DPAD_LEFT => Key::LeftArrow,
        AKEYCODE_DPAD_RIGHT => Key::RightArrow,

        // Gamepad. The game only ever reads the keyboard, so a controller plays by pressing the
        // same keys the keyboard does. That is the whole trick, and it needs nothing from the game:
        //
        //   D-pad                  -> arrow keys
        //   A (Xbox/Nintendo) / X (PS)      -> Z
        //   B (Xbox/Nintendo) / circle (PS) -> X
        //   X (Xbox/Nintendo) / square (PS) -> C
        //   Y (Xbox/Nintendo) / triangle    -> V
        //   select, share, view, minus      -> Enter
        //   start, options, menu, plus      -> Space
        //   R1 / RB / R                      -> B
        //   L1 / LB / L                      -> M
        //   R2 / RT / ZR                     -> O
        //   L2 / LT / ZL                     -> S
        //   stick press (either one)         -> E
        AKEYCODE_BUTTON_A => Key::Z,
        AKEYCODE_BUTTON_B => Key::X,
        AKEYCODE_BUTTON_X => Key::C,
        AKEYCODE_BUTTON_Y => Key::V,
        AKEYCODE_BUTTON_SELECT => Key::Return,
        AKEYCODE_BUTTON_START => Key::Space,
        AKEYCODE_BUTTON_R1 => Key::B,
        AKEYCODE_BUTTON_L1 => Key::M,
        AKEYCODE_BUTTON_R2 => Key::O,
        AKEYCODE_BUTTON_L2 => Key::S,
        AKEYCODE_BUTTON_THUMBL | AKEYCODE_BUTTON_THUMBR => Key::E,

        _ => return None,
    })
}

fn map_letter(keycode: AKeyCode) -> Option<Key> {
    let index = (keycode - AKEYCODE_A) as usize;
    LETTERS.get(index).copied()
}

fn mapped_key(keycode: i32) -> Option<Key> {
    map_key(keycode).or_else(|| map_letter(keycode))
}

/// The pad layout the launcher's control screen saved, keyed by Android key code. The contents are
/// replaced on every run rather than cached, because the engine and the launcher share one process
/// and the layout can change between one run of the game and the next.
static MAPPED_BUTTONS: std::sync::OnceLock<Mutex<HashMap<AKeyCode, Key>>> = std::sync::OnceLock::new();

/// Whether the physical sticks move the player.
static STICK_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Which arrow keys the physical stick is holding (bit 0 up, 1 down, 2 left, 3 right). Kept apart
/// from the on-screen joystick's state so neither can release a key the other is holding.
static STICK_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Reads the video mode the launcher saved. Only one thing matters here: whether the picture keeps
/// the shape the game was drawn for, or stretches to fill the screen.
fn load_video() {
    let path = storage_path("video.conf");
    if let Ok(text) = fs::read_to_string(&path) {
        for line in text.lines() {
            let Some((name, value)) = line.trim().split_once('=') else { continue };
            match name.trim() {
                "scaling" => {
                    let value = value.trim();
                    if value == "aspect" || value == "full" {
                        eprintln!("[video] escalado: {}", value);
                        // Read by the engine when it builds its renderer.
                        std::env::set_var("OPENGMK_SCALING", value);
                    }
                },
                // How the engine paces drawing: native (one frame per game step), auto (vsync) or
                // uncapped (as fast as the device can). Read when the game loop starts.
                "render" => {
                    let value = value.trim();
                    // Named modes plus the numeric caps the Settings screen writes (60/120/...).
                    let numeric = value.parse::<u32>().map(|hz| hz > 0).unwrap_or(false);
                    if matches!(value, "native" | "auto" | "uncapped") || numeric {
                        eprintln!("[video] render: {}", value);
                        std::env::set_var("OPENGMK_RENDER", value);
                    }
                },
                // Refresh rate the launcher detected. `auto` uses it as a fallback cap when the
                // driver's swap interval answers success but never blocks (as on some PCs).
                "detected_hz" => {
                    if let Ok(hz) = value.trim().parse::<u32>() {
                        if hz > 0 {
                            eprintln!("[video] pantalla: {} Hz", hz);
                            std::env::set_var("OPENGMK_RENDER_HZ", hz.to_string());
                        }
                    }
                },
                "show_fps" => {
                    let value = value.trim();
                    eprintln!("[video] mostrar FPS: {}", value != "0");
                    std::env::set_var("OPENGMK_SHOW_FPS", if value == "1" { "1" } else { "0" });
                },
                _ => (),
            }
        }
    }
}

/// Reads the layout the launcher's control screen left behind. Best effort: with no file, the
/// built-in layout is used exactly as before.
fn load_controls() {
    let path = storage_path("controls.conf");
    let mut buttons = HashMap::new();
    crate::touch_controls::set_sprite_opacity(1.0);
    if let Ok(text) = fs::read_to_string(&path) {
        eprintln!("[pad] leyendo {}", path.display());
        for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue
                }
                eprintln!("[pad]   {}", line);
            let Some((name, value)) = line.split_once('=') else { continue };
            if name.trim() == "stick" {
                STICK_ENABLED.store(value.trim() != "0", std::sync::atomic::Ordering::Relaxed);
            } else if name.trim() == "opacity" {
                if let Ok(opacity) = value.trim().parse::<f32>() { crate::touch_controls::set_sprite_opacity(opacity); }
            } else if name.trim() == "pad" {
                // A pad was connected when the game started, so start with the controls out of the
                // way; touching the screen brings them back.
                crate::touch_controls::set_pad_active(value.trim() != "0");
            } else if let (Ok(code), Some(key)) = (name.trim().parse::<AKeyCode>(), parse_key(value.trim())) {
                buttons.insert(code, key);
            }
        }
    }
    // Replaced rather than cached: the launcher can change the layout while the app is still alive.
    if let Ok(mut map) = MAPPED_BUTTONS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        *map = buttons;
    }
}

/// Reads the overlay the launcher's editor left behind: the style, what drives the movement, the
/// overall size and where each control sits. Best effort: with no file, the built-in defaults are used.
fn load_touch() {
    let path = storage_path("touch.conf");
    let Ok(text) = fs::read_to_string(&path) else { return };
    eprintln!("[overlay] leyendo {}", path.display());
    crate::touch_controls::clear_layout();
    let mut positions: Vec<(String, f32, f32)> = Vec::new();
    let mut sizes: Vec<(String, f32)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue
        }
        let Some((name, value)) = line.split_once('=') else { continue };
        let (name, value) = (name.trim(), value.trim());
        match name {
            "style" => crate::touch_controls::set_overlay_style(match value {
                "classic" => crate::touch_controls::OverlayStyle::Classic,
                "none" => crate::touch_controls::OverlayStyle::None,
                _ => crate::touch_controls::OverlayStyle::New,
            }),
            "move" => crate::touch_controls::set_move_mode(if value == "dpad" {
                crate::touch_controls::MoveMode::Dpad
            } else {
                crate::touch_controls::MoveMode::Joystick
            }),
            "scale" => {
                if let Ok(scale) = value.parse::<f32>() {
                    crate::touch_controls::set_ui_scale(scale);
                }
            },
            "opacity" => {
                if let Ok(opacity) = value.parse::<f32>() {
                    crate::touch_controls::set_sprite_opacity(opacity);
                }
            },
            _ => {
                if let Some(target) = name.strip_prefix("pos.") {
                    let mut parts = value.split(',');
                    if let (Some(cx), Some(cy)) = (parts.next(), parts.next()) {
                        if let (Ok(cx), Ok(cy)) = (cx.trim().parse::<f32>(), cy.trim().parse::<f32>()) {
                            positions.push((target.to_string(), cx, cy));
                        }
                    }
                } else if let Some(target) = name.strip_prefix("size.") {
                    if let Ok(scale) = value.parse::<f32>() {
                        sizes.push((target.to_string(), scale));
                    }
                }
            },
        }
    }
    for (name, cx, cy) in &positions {
        let scale = sizes.iter().find(|(other, _)| other == name).map(|(_, scale)| *scale).unwrap_or(1.0);
        crate::touch_controls::set_placement(name, *cx, *cy, scale);
    }
    for (name, scale) in &sizes {
        if positions.iter().any(|(other, _, _)| other == name) {
            continue
        }
        let (cx, cy) = crate::touch_controls::default_center(name).unwrap_or((0.5, 0.5));
        crate::touch_controls::set_placement(name, cx, cy, *scale);
    }
    let style = match crate::touch_controls::overlay_style() {
        crate::touch_controls::OverlayStyle::New => "new",
        crate::touch_controls::OverlayStyle::Classic => "classic",
        crate::touch_controls::OverlayStyle::None => "none",
    };
    let movement = match crate::touch_controls::move_mode() {
        crate::touch_controls::MoveMode::Joystick => "joystick",
        crate::touch_controls::MoveMode::Dpad => "dpad",
    };
    eprintln!(
        "[overlay] estilo={} move={} escala={} controles={}",
        style,
        movement,
        crate::touch_controls::ui_scale(),
        positions.len().max(sizes.len())
    );
}

/// The key names the control screen can write into `controls.conf`.
fn parse_key(name: &str) -> Option<Key> {
    let upper = name.trim().to_ascii_uppercase();
    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];
        if byte.is_ascii_uppercase() {
            return LETTERS.get(usize::from(byte - b'A')).copied()
        }
    }
    Some(match upper.as_str() {
        "RETURN" | "ENTER" => Key::Return,
        "SPACE" => Key::Space,
        "ESCAPE" => Key::Escape,
        "TAB" => Key::Tab,
        "BACKSPACE" => Key::Backspace,
        "UP" => Key::UpArrow,
        "DOWN" => Key::DownArrow,
        "LEFT" => Key::LeftArrow,
        "RIGHT" => Key::RightArrow,
        _ => return None,
    })
}

/// Holds and releases the arrow keys the physical stick asks for, so pushing it feels exactly like
/// the on-screen joystick.
fn stick_update(x: f32, y: f32) {
    let dirs = crate::touch_controls::stick_dirs(x, y);
    let mut state = STICK_STATE.load(std::sync::atomic::Ordering::Relaxed);
    let before = state;
    for (index, held) in dirs.iter().enumerate() {
        let bit = 1u32 << index;
        let was = state & bit != 0;
        if *held && !was {
            android::push_input(ramen::platform::android::InputEvent::KeyDown { key: MOVEMENT_KEYS[index], text: None });
        } else if !*held && was {
            android::push_input(ramen::platform::android::InputEvent::KeyUp { key: MOVEMENT_KEYS[index] });
        }
        if *held {
            state |= bit;
        } else {
            state &= !bit;
        }
    }
    STICK_STATE.store(state, std::sync::atomic::Ordering::Relaxed);
    if state != before {
        // Only when the direction actually changes, so holding a direction does not flood logcat.
        eprintln!("[pad] ejes x={:.2} y={:.2} -> direcciones {}", x, y, state);
    }
}

/// The value of one axis of a motion event. Axis values belong to the event, and the crate reaches
/// them through its first pointer, so there has to be one.
fn axis_value(motion: &android_activity::input::MotionEvent, axis: Axis) -> f32 {
    if motion.pointer_count() == 0 {
        return 0.0
    }
    motion.pointer_at_index(0).axis_value(axis)
}

fn stick_release() {
    let state = STICK_STATE.swap(0, std::sync::atomic::Ordering::Relaxed);
    for (index, key) in MOVEMENT_KEYS.iter().enumerate() {
        if state & (1 << index) != 0 {
            android::push_input(ramen::platform::android::InputEvent::KeyUp { key: *key });
        }
    }
}

/// `Keycode` is a repr(i32) enum, so the raw AKEYCODE_* value is recoverable.
fn key_code_value(keycode: &android_activity::input::Keycode) -> i32 {
    // `Keycode` is `#[repr(u32)]` with `num_enum::IntoPrimitive`, so this conversion is the
    // supported one; reading it as an i32 through a pointer was undefined behaviour.
    u32::from(*keycode) as i32
}

/// Which control each active pointer is currently holding, so a multi-touch release is paired with
/// the right control. Keyed by Android pointer id.
static POINTER_CONTROLS: Mutex<Option<HashMap<i32, usize>>> = Mutex::new(None);

/// The pointer the party wheel is following, if any.
static WHEEL_PTR: Mutex<Option<i32>> = Mutex::new(None);
static FIELD_POINTER: Mutex<Option<(i32,usize)>> = Mutex::new(None);

/// Sends a key as a quick tap. The engine defers the release to the end of the poll, so the game
/// still sees it as pressed.
fn tap_key(key: ramen::input::Key) {
    if crate::touch_controls::key_held_by_buttons(key) || joystick_holds(key) { return }
    android::push_input(ramen::platform::android::InputEvent::KeyDown { key, text: None });
    android::push_input(ramen::platform::android::InputEvent::KeyUp { key });
}

fn joystick_holds(key: Key) -> bool {
    let state=MOVEMENT_STATE.load(std::sync::atomic::Ordering::Relaxed);
    MOVEMENT_KEYS.iter().enumerate().any(|(i,k)| *k==key && state & (1<<i)!=0)
}

fn control_key_down(index: usize) {
    use crate::touch_controls as tc;
    if tc::controls()[index].name == "o" && matches!(tc::current_context(),tc::WORLD|tc::BATTLE|tc::SAFARI) {crate::touch_navigation::arm_bag();}
    else {crate::touch_navigation::disarm_bag();}
    let keys=tc::control_keys(index);
    let held=keys.map(|key| key.is_some_and(|key| tc::key_held_by_buttons(key) || joystick_holds(key)));
    tc::set_pressed(index,true);
    for (i,key) in keys.into_iter().enumerate() {
        if let Some(key)=key.filter(|_| !held[i]) {
            match key {Key::LeftArrow=>tc::picker_step(-1),Key::RightArrow=>tc::picker_step(1),_=>{}}
            android::push_input(ramen::platform::android::InputEvent::KeyDown {key,text:None});
        }
    }
    if tc::controls()[index].name==tc::COMBO_BUTTON_NAME {tc::set_combo_used();}
}

fn control_key_up(index: usize) {
    use crate::touch_controls as tc;
    if !tc::is_pressed(index) {return}
    tc::set_pressed(index,false);
    for key in tc::control_keys(index).into_iter().flatten() {
        if !tc::key_held_by_buttons(key) && !joystick_holds(key) {
            android::push_input(ramen::platform::android::InputEvent::KeyUp {key});
        }
    }
}

/// Pointers whose press was swallowed to bring the on-screen controls back, so that their release
/// is ignored too instead of reaching the game as a stray mouse-up.
static SWALLOWED_INPUTS: Mutex<Option<std::collections::HashSet<i32>>> = Mutex::new(None);

/// A touch on an on-screen control presses the key that control maps to; any other touch is
/// forwarded to the game as a mouse, exactly as before.
fn touch_feedback() {
    if !crate::touch_appearance::settings().haptics { return }
    with_activity(|env, class| {
        env.call_static_method(class, "touchControlPressed", "()V", &[])?;
        Ok(())
    });
}

fn pointer_down(id: i32, x: f32, y: f32) {
    // A new gesture cannot inherit a swallowed Up from an Android cancellation.
    if let Some(ids)=SWALLOWED_INPUTS.lock().unwrap().as_mut(){ids.remove(&id);}
    let (w, h) = android::window_size();
    let (wf, hf) = (w as f32, h as f32);
    // The first touch after using a pad only brings the controls back. It must not also land in the
    // game as a click, or reaching for the on-screen buttons would act on whatever is under them.
    if crate::touch_controls::pad_active() {
        crate::touch_controls::set_pad_active(false);
        SWALLOWED_INPUTS.lock().unwrap().get_or_insert_with(std::collections::HashSet::new).insert(id);
        return
    }
    let on_movement = wf > 0.0 && hf > 0.0 && crate::touch_controls::movement_hit(x / wf, y / hf, wf, hf);
    // Any *other* touch while the party wheel is up cancels it - but not the movement stick: while
    // walking the player keeps touching and lifting it, and that must not close the wheel.
    if crate::touch_controls::wheel_open() && *WHEEL_PTR.lock().unwrap() != Some(id) && !on_movement {
        crate::touch_controls::set_wheel_open(false);
        WHEEL_PTR.lock().unwrap().take();
    }
    if on_movement {
        *MOVEMENT_PTR.lock().unwrap() = Some(id);
        movement_update(x, y);
        return
    }
    let hit = if w > 0 && h > 0 { crate::touch_controls::hit(x / wf, y / hf, wf, hf) } else { None };
    if hit.is_some_and(|i|crate::touch_controls::controls()[i].name==crate::field_controls::NAME){
        crate::field_controls::toggle();touch_feedback();
        SWALLOWED_INPUTS.lock().unwrap().get_or_insert_with(std::collections::HashSet::new).insert(id);return
    }
    if crate::field_controls::open(){
        if let Some(slot)=crate::field_controls::item_at(x,y,wf,hf){let mut owner=FIELD_POINTER.lock().unwrap();if owner.is_none(){*owner=Some((id,slot));touch_feedback();}}
        SWALLOWED_INPUTS.lock().unwrap().get_or_insert_with(std::collections::HashSet::new).insert(id);return
    }
    if hit.is_none()&&crate::touch_navigation::active(){
        if !crate::touch_navigation::begin(id,x,y,hf){SWALLOWED_INPUTS.lock().unwrap().get_or_insert_with(std::collections::HashSet::new).insert(id);}return
    }
    match hit {
        Some(index) => {
            if crate::touch_controls::controls()[index].name == crate::touch_controls::ROTATE_NAME {
                queue_rotate_at_last_game_touch();
                SWALLOWED_INPUTS.lock().unwrap().get_or_insert_with(std::collections::HashSet::new).insert(id);
                touch_feedback();
                return;
            }
            if crate::touch_controls::controls()[index].name == crate::touch_controls::PARTY_NAME {
                // Holding it opens the party wheel instead of pressing a key of its own.
                crate::touch_controls::set_wheel_open(true);
                *WHEEL_PTR.lock().unwrap() = Some(id);
            } else {
                let first = {
                    let mut pointers = POINTER_CONTROLS.lock().unwrap();
                    let pointers = pointers.get_or_insert_with(HashMap::new);
                    let first = !pointers.values().any(|held| *held == index);
                    pointers.insert(id, index);
                    first
                };
                if first { control_key_down(index); }
            }
            touch_feedback();
        },
        None => {
            // The game asks for text through its own dialog, and touching it opens the system
            // keyboard. The only clickable thing the game has there is its "Aceptar" box, so a touch
            // anywhere else is free; touching "Aceptar" still confirms, because the game keeps the tap.
            if crate::touch_controls::text_entry_open() {
                ime_set(true);
            }
            mouse_down(id, x, y)
        },
    }
}

fn pointer_up(id: i32, x: f32, y: f32) {
    if crate::touch_navigation::owns(id){
        if let Some((x,y))=crate::touch_navigation::end(id,x,y){android::push_input(ramen::platform::android::InputEvent::PointerMoved{x,y});}return
    }
    let selected={let mut owner=FIELD_POINTER.lock().unwrap();if owner.is_some_and(|(pointer,_)|pointer==id){owner.take().map(|(_,slot)|slot)}else{None}};
    if let Some(slot)=selected{let(w,h)=android::window_size();if crate::field_controls::open()&&crate::field_controls::item_at(x,y,w as f32,h as f32)==Some(slot){tap_key(crate::field_controls::KEYS[slot]);crate::field_controls::close();}}

    let closes_wheel = {
        let mut owner = WHEEL_PTR.lock().unwrap();
        if *owner == Some(id) { *owner = None; true } else { false }
    };
    if closes_wheel {
        // The key only goes out now, on the release, and only if a slot was picked. Letting go over
        // the middle of the wheel, or letting go without moving at all, sends nothing.
        let selected = crate::touch_controls::wheel_selected();
        if selected >= 0 {
            tap_key(crate::touch_controls::PICKER_KEYS[selected as usize]);
        }
        crate::touch_controls::set_wheel_open(false);
        return
    }
    if SWALLOWED_INPUTS.lock().unwrap().get_or_insert_with(std::collections::HashSet::new).remove(&id) {
        return
    }
    if is_movement_pointer(id) {
        movement_release();
        return
    }
    let index = POINTER_CONTROLS.lock().unwrap().get_or_insert_with(HashMap::new).remove(&id);
    match index {
        Some(index) => {
            let still_held = POINTER_CONTROLS.lock().unwrap().as_ref().is_some_and(|p| p.values().any(|held| *held == index));
            if !still_held { control_key_up(index); }
        },
        None => mouse_up(id, x, y),
    }
}

/// A real tap lasts ~100 ms, but a quick one can deliver press and release inside a single poll.
/// The game reads the mouse once per frame, so such a tap is simply missed — that is why clicking
/// the game's menus with the finger felt random. Holding the release briefly makes taps behave
/// like real mouse clicks.
const MIN_TAP: Duration = Duration::from_millis(70);

/// The game decides which menu entry is under the cursor while it *moves*; a click only confirms
/// the current one. A finger sends move and press together, so the press used to land on whatever
/// was selected before. Delivering the move a hair earlier gives the game that frame.
const HOVER_DELAY: Duration = Duration::from_millis(45);

static MOUSE_DOWN_AT: Mutex<Vec<(i32, Instant)>> = Mutex::new(Vec::new());
static DELAYED_UPS: Mutex<Vec<(Instant, f32, f32)>> = Mutex::new(Vec::new());
static DELAYED_DOWNS: Mutex<Vec<(Instant, f32, f32)>> = Mutex::new(Vec::new());

/// Last place the finger touched the game itself, not a button.
static LAST_GAME_TOUCH: Mutex<Option<(f32, f32)>> = Mutex::new(None);
const ROTATE_DELAY: Duration = Duration::from_millis(350);
static PENDING_ROTATE: Mutex<Option<(Instant, f32, f32)>> = Mutex::new(None);

fn mouse_down(id: i32, x: f32, y: f32) {
    *LAST_GAME_TOUCH.lock().unwrap() = Some((x, y));
    let deliver_at = Instant::now() + HOVER_DELAY;
    let mut downs = MOUSE_DOWN_AT.lock().unwrap();
    downs.retain(|(pointer, _)| *pointer != id);
    downs.push((id, deliver_at));
    drop(downs);
    // The move goes out now; the press follows on a later poll so the hover is in place first.
    android::push_input(ramen::platform::android::InputEvent::PointerMoved { x, y });
    DELAYED_DOWNS.lock().unwrap().push((deliver_at, x, y));
}

fn mouse_up(id: i32, x: f32, y: f32) {
    let pressed_at = {
        let mut downs = MOUSE_DOWN_AT.lock().unwrap();
        let at = downs.iter().find(|(pointer, _)| *pointer == id).map(|(_, at)| *at);
        downs.retain(|(pointer, _)| *pointer != id);
        at
    };
    let deadline = pressed_at.map(|at| at + MIN_TAP).unwrap_or_else(Instant::now);
    if deadline > Instant::now() {
        DELAYED_UPS.lock().unwrap().push((deadline, x, y));
    } else {
        android::push_input(ramen::platform::android::InputEvent::PointerUp { x, y, button: 0 });
    }
}

fn right_click_at(x: f32, y: f32) {
    use ramen::platform::android::InputEvent;
    android::push_input(InputEvent::PointerMoved { x, y });
    android::push_input(InputEvent::PointerDown { x, y, button: 2 });
    android::push_input(InputEvent::PointerUp { x, y, button: 2 });
}

fn queue_rotate_at_last_game_touch() {
    if let Some((x, y)) = *LAST_GAME_TOUCH.lock().unwrap() {
        *PENDING_ROTATE.lock().unwrap() = Some((Instant::now() + ROTATE_DELAY, x, y));
        eprintln!("[puzzle] girar encolado en {},{}", x, y);
    }
}

fn flush_pending_rotate() {
    let mut pending = PENDING_ROTATE.lock().unwrap();
    let Some((at, x, y)) = *pending else { return };
    if Instant::now() < at {
        return;
    }
    *pending = None;
    eprintln!("[puzzle] girar enviado en {},{}", x, y);
    right_click_at(x, y);
}

pub fn show_puzzle_hint() {
    with_activity(|env, class| {
        env.call_static_method(class, "showTouchHint", "()V", &[])?;
        Ok(())
    });
}

/// Delivers the presses and releases that were held back to make a short tap a proper click.
fn flush_delayed_mouse() {
    let now = Instant::now();
    let mut downs: Vec<(f32, f32)> = Vec::new();
    DELAYED_DOWNS.lock().unwrap().retain(|(at, x, y)| {
        if *at <= now {
            downs.push((*x, *y));
            false
        } else {
            true
        }
    });
    for (x, y) in downs {
        android::push_input(ramen::platform::android::InputEvent::PointerDown { x, y, button: 0 });
    }

    let mut ups: Vec<(f32, f32)> = Vec::new();
    DELAYED_UPS.lock().unwrap().retain(|(at, x, y)| {
        if *at <= now {
            ups.push((*x, *y));
            false
        } else {
            true
        }
    });
    for (x, y) in ups {
        android::push_input(ramen::platform::android::InputEvent::PointerUp { x, y, button: 0 });
    }
}

fn pointer_is_control(id: i32) -> bool {
    POINTER_CONTROLS.lock().unwrap().as_ref().map(|m| m.contains_key(&id)).unwrap_or(false)
}

// --- Movement (joystick or D-pad) ---------------------------------------------------------

/// Pointer id currently driving the movement control, if any.
static MOVEMENT_PTR: Mutex<Option<i32>> = Mutex::new(None);
/// Which arrow keys the movement control is holding (bit 0 up, 1 down, 2 left, 3 right).
static MOVEMENT_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

const MOVEMENT_KEYS: [Key; 4] = [Key::UpArrow, Key::DownArrow, Key::LeftArrow, Key::RightArrow];

fn is_movement_pointer(id: i32) -> bool {
    *MOVEMENT_PTR.lock().unwrap() == Some(id)
}

/// Presses/releases the arrow keys so they match where the movement control is being pushed.
fn movement_update(x: f32, y: f32) {
    crate::touch_navigation::disarm_bag();
    use std::sync::atomic::Ordering;
    let (w, h) = android::window_size();
    let (wf, hf) = (w as f32, h as f32);
    if wf <= 0.0 || hf <= 0.0 {
        return
    }
    let dirs = crate::touch_controls::movement_dirs(x / wf, y / hf, wf, hf);
    crate::touch_controls::set_move_dirs(dirs);
    let mut state = MOVEMENT_STATE.load(Ordering::Relaxed);
    let mut new_direction = false;
    for (i, &on) in dirs.iter().enumerate() {
        let bit = 1u32 << i;
        if on != (state & bit != 0) {
            if on {
                if !crate::touch_controls::key_held_by_buttons(MOVEMENT_KEYS[i]) {
                    android::push_input(ramen::platform::android::InputEvent::KeyDown { key: MOVEMENT_KEYS[i], text: None });
                }
                new_direction = true;
                state |= bit;
            } else {
                if !crate::touch_controls::key_held_by_buttons(MOVEMENT_KEYS[i]) {
                    android::push_input(ramen::platform::android::InputEvent::KeyUp { key: MOVEMENT_KEYS[i] });
                }
                state &= !bit;
            }
        }
    }
    MOVEMENT_STATE.store(state, Ordering::Relaxed);
    // The D-pad gives a short tap each time a direction is marked; the analog stick stays silent.
    if new_direction && crate::touch_controls::move_mode() == crate::touch_controls::MoveMode::Dpad {
        touch_feedback();
    }

    // Clamp the drawn knob to the ring.
    let classic = crate::touch_controls::overlay_style() == crate::touch_controls::OverlayStyle::Classic;
    let (jcx, jcy) = if classic { crate::touch_controls::joystick_center_in_window(wf, hf) } else { crate::touch_controls::movement_center() };
    let r = if classic { crate::touch_controls::joystick_radius() } else { crate::touch_controls::movement_radius() } * hf;
    let mut dx = (x - jcx * wf) / r;
    let mut dy = (y - jcy * hf) / r;
    let len = (dx * dx + dy * dy).sqrt();
    if len > 1.0 {
        dx /= len;
        dy /= len;
    }
    crate::touch_controls::set_movement_offset(dx, dy);
}

fn movement_release() {
    use std::sync::atomic::Ordering;
    crate::touch_controls::set_move_dirs([false; 4]);
    let state = MOVEMENT_STATE.swap(0, Ordering::Relaxed);
    for i in 0..4 {
        if state & (1 << i) != 0 && !crate::touch_controls::key_held_by_buttons(MOVEMENT_KEYS[i]) {
            android::push_input(ramen::platform::android::InputEvent::KeyUp { key: MOVEMENT_KEYS[i] });
        }
    }
    crate::touch_controls::set_movement_offset(0.0, 0.0);
    *MOVEMENT_PTR.lock().unwrap() = None;
}

fn release_touch_profile() {
    let held = POINTER_CONTROLS.lock().unwrap().take().unwrap_or_default();
    let mut swallowed = SWALLOWED_INPUTS.lock().unwrap();
    let ids = swallowed.get_or_insert_with(std::collections::HashSet::new);
    for (id, index) in held {
        control_key_up(index);
        ids.insert(id);
    }
    if let Some(id) = *MOVEMENT_PTR.lock().unwrap() { ids.insert(id); }
    movement_release();
    if let Some(id) = WHEEL_PTR.lock().unwrap().take() { ids.insert(id); }
    crate::touch_controls::set_wheel_open(false);
    crate::field_controls::close();
    if let Some((id,_))=FIELD_POINTER.lock().unwrap().take(){ids.insert(id);}
    if let Some(id)=crate::touch_navigation::cancel(){ids.insert(id);}
    if let Some((x,y))=crate::touch_navigation::cancel_pointer(){
        android::push_input(ramen::platform::android::InputEvent::PointerUp{x,y,button:0});
    }
}

/// The pointer the current action refers to, if it is valid.
fn changed_pointer(motion: &android_activity::input::MotionEvent) -> Option<(i32, f32, f32)> {
    let index = motion.pointer_index();
    if index >= motion.pointer_count() {
        return None
    }
    let p = motion.pointer_at_index(index);
    Some((p.pointer_id(), p.x(), p.y()))
}

/// Android key codes for the hardware volume rocker.
const KEYCODE_VOLUME_UP: i32 = 24;
const KEYCODE_VOLUME_DOWN: i32 = 25;

/// Translates one `android-activity` input event into ramen's queue.
fn translate(event: &InputEvent) {
    match event {
        InputEvent::KeyEvent(key) => match key.action() {
            KeyAction::Down => {
                let code = key_code_value(&key.key_code());
                if code == KEYCODE_VOLUME_UP || code == KEYCODE_VOLUME_DOWN {
                    // Volume keys arrive through the native queue, so Android never handles them.
                    jni_adjust_volume(code == KEYCODE_VOLUME_UP);
                    return
                }
                if let Some(k) = mapped_key(code) {
                    // A real key press means a pad or a keyboard is in the player's hands, so the
                    // on-screen controls get out of the way.
                    crate::touch_controls::set_pad_active(true);
                    eprintln!("[pad] tecla {} -> {:?}", code, k);
                    android::push_input(ramen::platform::android::InputEvent::KeyDown { key: k, text: None });
                }
            },
            KeyAction::Up => {
                if let Some(k) = mapped_key(key_code_value(&key.key_code())) {
                    android::push_input(ramen::platform::android::InputEvent::KeyUp { key: k });
                }
            },
            _ => {},
        },
        InputEvent::MotionEvent(motion) => {
            // A pad's stick arrives as a generic motion event, not as a key, so its axes have to be
            // read from the event itself rather than from a pointer.
            if motion.source().is_joystick_class() {
                crate::touch_controls::set_pad_active(true);
                if STICK_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
                    let (mut x, mut y) = (axis_value(motion, Axis::X), axis_value(motion, Axis::Y));
                    // A d-pad is usually reported as a hat axis rather than as keys, and it has to
                    // win over the stick when it is pressed: a resting stick is not always at exactly
                    // zero, and requiring that is what kept the d-pad from doing anything.
                    let (hat_x, hat_y) = (axis_value(motion, Axis::HatX), axis_value(motion, Axis::HatY));
                    if hat_x != 0.0 || hat_y != 0.0 {
                        x = hat_x;
                        y = hat_y;
                    }
                    stick_update(x, y);
                }
                return
            }

            match motion.action() {
                MotionAction::Down | MotionAction::PointerDown => {
                    if let Some((id, x, y)) = changed_pointer(motion) {
                        pointer_down(id, x, y);
                    }
                },
                MotionAction::Up | MotionAction::PointerUp => {
                    if let Some((id, x, y)) = changed_pointer(motion) {
                        pointer_up(id, x, y);
                    }
                },
                MotionAction::Move | MotionAction::HoverMove => {
                    let wheel_ptr = *WHEEL_PTR.lock().unwrap();
                    for p in motion.pointers() {
                        if SWALLOWED_INPUTS.lock().unwrap().as_ref().is_some_and(|ids|ids.contains(&p.pointer_id())){continue}
                        if crate::touch_navigation::moved(p.pointer_id(),p.x(),p.y()){continue}
                        if wheel_ptr == Some(p.pointer_id()) {
                            let (w, h) = android::window_size();
                            let (wf, hf) = (w as f32, h as f32);
                            if wf > 0.0 && hf > 0.0 {
                                // Only highlight while the finger moves. The key waits for the
                                // release: in battle it sends the Pokémon out, so it must never go
                                // out on a touch that was not meant as a pick.
                                let picked = crate::touch_controls::wheel_item_at(p.x() / wf, p.y() / hf, wf, hf);
                                crate::touch_controls::set_wheel_selected(picked.map(|i| i as i32).unwrap_or(-1));
                            }
                        } else if is_movement_pointer(p.pointer_id()) {
                            movement_update(p.x(), p.y());
                        } else if !pointer_is_control(p.pointer_id()) {
                            *LAST_GAME_TOUCH.lock().unwrap() = Some((p.x(), p.y()));
                            android::push_input(ramen::platform::android::InputEvent::PointerMoved { x: p.x(), y: p.y() });
                        }
                    }
                },
                MotionAction::Cancel => {
                    release_touch_profile();
                    if let Some(ids)=SWALLOWED_INPUTS.lock().unwrap().as_mut(){ids.clear();}
                    movement_release();
                    let ids: Vec<i32> =
                        POINTER_CONTROLS.lock().unwrap().as_ref().map(|m| m.keys().copied().collect()).unwrap_or_default();
                    for id in ids {
                        if let Some(index) = POINTER_CONTROLS.lock().unwrap().get_or_insert_with(HashMap::new).remove(&id) {
                            control_key_up(index);
                        }
                    }
                },
                _ => {},
            }
        },
        _ => {},
    }
}

/// A NativeActivity receives key events through the native queue, so the system never sees the
/// volume keys and the volume can't be changed. Adjust the music stream ourselves instead.
fn jni_adjust_volume(up: bool) {
    let ctx = ndk_context::android_context();
    let vm = match unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(vm) => vm,
        Err(_) => return,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(_) => return,
    };
    let _ = (|| -> jni::errors::Result<()> {
        let activity = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
        let name = env.new_string("audio")?;
        let manager = env
            .call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[jni::objects::JValue::Object(&name)],
            )?
            .l()?;
        if manager.is_null() {
            return Ok(())
        }
        // STREAM_MUSIC = 3, ADJUST_RAISE = 1 / ADJUST_LOWER = -1, FLAG_SHOW_UI = 1
        let direction = if up { 1 } else { -1 };
        env.call_method(
            &manager,
            "adjustStreamVolume",
            "(III)V",
            &[
                jni::objects::JValue::Int(3),
                jni::objects::JValue::Int(direction),
                jni::objects::JValue::Int(1),
            ],
        )?;
        Ok(())
    })();
}

// ---------------------------------------------------------------------------------------------
// System (soft) keyboard.
//
// The game has no Android view of its own to type into, so the activity's `InputConnection` sends
// what is typed here through the JNI exports below. `ime_pump` then feeds it to ramen the same way
// a hardware keyboard would, right before each poll, so the game's dialog sees a normal key tap.

/// What the system keyboard committed, waiting for the next poll.
enum ImeEvent {
    Text(String),
    Backspace,
    Enter,
}

fn ime_queue() -> &'static Mutex<std::collections::VecDeque<ImeEvent>> {
    static QUEUE: std::sync::OnceLock<Mutex<std::collections::VecDeque<ImeEvent>>> = std::sync::OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(std::collections::VecDeque::new()))
}

/// Feeds what the system keyboard typed into ramen's queue. Each character goes in as a quick key
/// tap that carries its text (`KeyDown{text}` + `KeyUp`): the press sets `keyboard_lastkey` and
/// `vk_anykey`, the text fills `keyboard_string`, and the deferred release keeps the key down for
/// the poll it arrived in - exactly what `tap_key` does for the on-screen buttons. Backspace and
/// enter are plain taps of the keys the game already reads for those.
pub fn ime_pump() {
    let events: Vec<ImeEvent> = match ime_queue().lock() {
        Ok(mut queue) => queue.drain(..).collect(),
        Err(_) => return,
    };
    for event in events {
        match event {
            ImeEvent::Text(text) => {
                for c in text.chars() {
                    let key = ime_char_to_key(c);
                    android::push_input(ramen::platform::android::InputEvent::KeyDown { key, text: Some(c) });
                    android::push_input(ramen::platform::android::InputEvent::KeyUp { key });
                }
            },
            ImeEvent::Backspace => tap_key(Key::Backspace),
            ImeEvent::Enter => tap_key(Key::Return),
        }
    }
}

/// The key a committed character is reported as. It only decides `keyboard_lastkey`; the character
/// itself travels in the event's text. Letters and digits become the same keys a hardware keyboard
/// would send; anything without a Game Maker key (accents, most signs) falls back to space, whose
/// code no dialog key uses.
fn ime_char_to_key(c: char) -> Key {
    if c.is_ascii_alphabetic() {
        return LETTERS[usize::from(c.to_ascii_uppercase() as u8 - b'A')]
    }
    if c.is_ascii_digit() {
        const DIGITS: [Key; 10] = [
            Key::Alpha0, Key::Alpha1, Key::Alpha2, Key::Alpha3, Key::Alpha4, Key::Alpha5, Key::Alpha6,
            Key::Alpha7, Key::Alpha8, Key::Alpha9,
        ];
        return DIGITS[usize::from(c as u8 - b'0')]
    }
    match c {
        ' ' => Key::Space,
        '.' => Key::Period,
        ',' => Key::Comma,
        '-' => Key::Minus,
        '+' => Key::Plus,
        '=' => Key::Equals,
        '/' => Key::Slash,
        '\\' => Key::Backslash,
        ';' => Key::Semicolon,
        ':' => Key::Colon,
        '\'' => Key::Apostrophe,
        '"' => Key::Quote,
        '?' => Key::QuestionMark,
        '_' => Key::Underscore,
        _ => Key::Space,
    }
}

/// Called from the activity's `InputConnection` for every committed string.
#[no_mangle]
pub extern "system" fn Java_com_klisee_reloaded_GameActivity_nativeCommitText<'local>(
    mut env: jni::JNIEnv<'local>,
    _this: jni::objects::JObject<'local>,
    text: jni::objects::JString<'local>,
) {
    let value: String = env.get_string(&text).map(Into::into).unwrap_or_default();
    if value.is_empty() {
        return
    }
    if let Ok(mut queue) = ime_queue().lock() {
        queue.push_back(ImeEvent::Text(value));
    }
}

/// Called from the activity's `InputConnection` for the delete key.
#[no_mangle]
pub extern "system" fn Java_com_klisee_reloaded_GameActivity_nativeBackspace<'local>(
    _env: jni::JNIEnv<'local>,
    _this: jni::objects::JObject<'local>,
) {
    if let Ok(mut queue) = ime_queue().lock() {
        queue.push_back(ImeEvent::Backspace);
    }
}

/// Called from the activity's `InputConnection` for the enter/done key.
#[no_mangle]
pub extern "system" fn Java_com_klisee_reloaded_GameActivity_nativeEnter<'local>(
    _env: jni::JNIEnv<'local>,
    _this: jni::objects::JObject<'local>,
) {
    if let Ok(mut queue) = ime_queue().lock() {
        queue.push_back(ImeEvent::Enter);
    }
}

/// Called from the activity whenever the system keyboard's height changes, so the renderer can keep
/// the game above it.
#[no_mangle]
pub extern "system" fn Java_com_klisee_reloaded_GameActivity_nativeImeInset<'local>(
    _env: jni::JNIEnv<'local>,
    _this: jni::objects::JObject<'local>,
    height: jni::sys::jint,
) {
    // A keyboard taller than the window is not a keyboard: a value like that would push the game off
    // the top and hide the controls behind nothing, so it is dropped rather than believed.
    let (_, window_height) = android::window_size();
    let height = height.max(0) as u32;
    let height = if window_height > 0 && height >= window_height { 0 } else { height };
    crate::touch_controls::set_ime_inset(height);
    // The keyboard can be dropped by the system as well as by us - its own close button, or the back
    // gesture - so whether it is up is taken from its height. Without this, the next touch on the
    // game's text would be read as "already up" and would not raise it again.
    IME_SHOWN.store(height > 0, std::sync::atomic::Ordering::Relaxed);
}

/// Called from the activity when another app takes (or gives back) the audio focus, so the game's
/// output does not talk over a call or a notification.
#[no_mangle]
pub extern "system" fn Java_com_klisee_reloaded_GameActivity_nativeAudioFocus<'local>(
    _env: jni::JNIEnv<'local>,
    _this: jni::objects::JObject<'local>,
    lost: jni::sys::jboolean,
) {
    crate::game::audio::set_output_paused(lost != 0);
}

/// Whether the system keyboard is up, so it is only raised or dropped on a change.
static IME_SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Raises or drops the system keyboard, but only when that is a change.
fn ime_set(visible: bool) {
    if IME_SHOWN.swap(visible, std::sync::atomic::Ordering::Relaxed) != visible {
        jni_set_ime(visible);
    }
}

/// Raises or drops the system keyboard by calling the activity's static `GameActivity.setImeVisible`.
/// The activity shows it from the UI thread, which Kotlin takes care of; here we only need to reach
/// the class. `FindClass` would look on the system class path (the context the activity publishes is
/// the Application), so the class is loaded through its own loader instead.
fn jni_set_ime(visible: bool) {
    with_activity(|env, class| {
        env.call_static_method(class, "setImeVisible", "(Z)V", &[jni::objects::JValue::Bool(visible as u8)])?;
        Ok(())
    });
}

/// Asks the activity to read the keyboard's height again.
///
/// The height normally arrives on its own, from the inset callback. This is the belt to that braces:
/// while the engine believes the keyboard is up it asks again once in a while, so an inset callback
/// that never came cannot leave the game pushed up top with the controls hidden.
fn jni_refresh_ime_height() {
    with_activity(|env, class| {
        env.call_static_method(class, "refreshImeHeight", "()V", &[])?;
        Ok(())
    });
}

/// Tells the app which room the game is in, so it can show the player's location on Discord.
///
/// Called from `Game::sync_overlay`, which runs wherever the game draws, so it is guarded here: the
/// name is only produced (and only crosses JNI) when the room id actually changes.
pub(crate) fn report_route(room_id: i32, name: impl FnOnce() -> String) {
    use std::sync::atomic::{AtomicI32, Ordering};
    static LAST_ROOM: AtomicI32 = AtomicI32::new(i32::MIN);
    if LAST_ROOM.load(Ordering::Relaxed) == room_id {
        return
    }
    let route = name();
    if route.trim().is_empty() {
        return
    }
    let sent = with_activity(|env, class| {
        let route = env.new_string(route.as_str())?;
        env.call_static_method(
            class,
            "onGameRoute",
            "(ILjava/lang/String;)V",
            &[jni::objects::JValue::Int(room_id), jni::objects::JValue::Object(&route)],
        )?;
        Ok(())
    });
    // Only remember the room once the app actually got it, so a transition (no activity yet) is
    // retried on the next frame instead of leaving the presence stuck in the previous room.
    if sent {
        LAST_ROOM.store(room_id, Ordering::Relaxed);
    }
}

/// Tells the app which character is being played, so the Rich Presence can show its head.
///
/// `prota` is the protagonist slot (0..9 are the base roster, 10 the custom one), `outfit` the
/// costume and `char2` the trainer id of a custom character. Only changes cross JNI.
pub(crate) fn report_character(prota: i32, outfit: i32, char2: i32) {
    use std::sync::atomic::Ordering;
    static LAST: std::sync::Mutex<Option<(i32, i32, i32)>> = std::sync::Mutex::new(None);
    if RESET_CHARACTER.swap(false, Ordering::Relaxed) {
        if let Ok(mut last) = LAST.lock() {
            *last = None;
        }
    }
    let changed = {
        let Ok(last) = LAST.lock() else { return };
        last.as_ref() != Some(&(prota, outfit, char2))
    };
    if !changed {
        return
    }
    let sent = with_activity(|env, class| {
        env.call_static_method(
            class,
            "onGameCharacter",
            "(III)V",
            &[
                jni::objects::JValue::Int(prota),
                jni::objects::JValue::Int(outfit),
                jni::objects::JValue::Int(char2),
            ],
        )?;
        Ok(())
    });
    if sent {
        if let Ok(mut last) = LAST.lock() {
            *last = Some((prota, outfit, char2));
        }
    }
}

/// Tells the app the trainer's nickname, so the Rich Presence can show it instead of the
/// character's default name. Only changes cross JNI.
pub(crate) fn report_player_name(name: &str) {
    static LAST: std::sync::Mutex<Option<std::string::String>> = std::sync::Mutex::new(None);
    if RESET_NAME.swap(false, std::sync::atomic::Ordering::Relaxed) {
        if let Ok(mut last) = LAST.lock() {
            *last = None;
        }
    }
    let changed = {
        let Ok(last) = LAST.lock() else { return };
        last.as_deref() != Some(name)
    };
    if !changed {
        return
    }
    let sent = with_activity(|env, class| {
        let name = env.new_string(name)?;
        env.call_static_method(
            class,
            "onGamePlayerName",
            "(Ljava/lang/String;)V",
            &[jni::objects::JValue::Object(&name)],
        )?;
        Ok(())
    });
    if sent {
        if let Ok(mut last) = LAST.lock() {
            *last = Some(name.to_string());
        }
    }
}

/// Tells the app whether a battle is running and who is on the other side. Only changes cross JNI.
pub(crate) fn report_battle(in_battle: bool, trainer: &str, species: i32) {
    static LAST: std::sync::Mutex<Option<(bool, std::string::String, i32)>> = std::sync::Mutex::new(None);
    let changed = {
        let Ok(last) = LAST.lock() else { return };
        if let Some((was_battle, was_trainer, was_species)) = last.as_ref() {
            *was_battle != in_battle || was_trainer != trainer || *was_species != species
        } else {
            true
        }
    };
    if !changed {
        return
    }
    let sent = with_activity(|env, class| {
        let trainer = env.new_string(trainer)?;
        env.call_static_method(
            class,
            "onGameBattle",
            "(ZLjava/lang/String;I)V",
            &[
                jni::objects::JValue::Bool(in_battle as u8),
                jni::objects::JValue::Object(&trainer),
                jni::objects::JValue::Int(species),
            ],
        )?;
        Ok(())
    });
    if sent {
        if let Ok(mut last) = LAST.lock() {
            *last = Some((in_battle, trainer.to_string(), species));
        }
    }
}

static RESET_CHARACTER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static RESET_NAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Forgets the last reported player so the next room (the intro chat) reports it again even when
/// it is the same character as the previous game.
pub(crate) fn reset_player_reports() {
    RESET_CHARACTER.store(true, std::sync::atomic::Ordering::Relaxed);
    RESET_NAME.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The activity's class, held for the life of the process.
///
/// It has to be looked up through the activity's own class loader: `FindClass` would look on the
/// system class path, because the context the activity publishes for JNI is the Application. Holding
/// it globally means the lookup happens once instead of on every call, and that the watchdog below
/// does not leave a reference behind each time it runs.
static ACTIVITY_CLASS: std::sync::OnceLock<jni::objects::GlobalRef> = std::sync::OnceLock::new();

/// Attaches this thread and runs `f` with the activity's class.
fn with_activity(f: impl FnOnce(&mut jni::JNIEnv, &jni::objects::JClass) -> jni::errors::Result<()>) -> bool {
    let ctx = ndk_context::android_context();
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }) else { return false };
    // Permanent: the thread stays attached, and attaching on every call would only add work.
    let Ok(mut env) = vm.attach_current_thread_permanently() else { return false };
    if ACTIVITY_CLASS.get().is_none() {
        let loaded = (|| -> jni::errors::Result<jni::objects::GlobalRef> {
            let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
            let loader = env.call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?.l()?;
            let name = env.new_string("com.klisee.reloaded.GameActivity")?;
            let class = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[jni::objects::JValue::Object(&name)],
                )?
                .l()?;
            env.new_global_ref(class)
        })();
        match loaded {
            Ok(global) => {
                let _ = ACTIVITY_CLASS.set(global);
            },
            Err(_) => return false,
        }
    }
    let Some(global) = ACTIVITY_CLASS.get() else { return false };
    let class = unsafe { jni::objects::JClass::from_raw(global.as_obj().as_raw()) };
    f(&mut env, &class).is_ok()
}

/// The `ANativeWindow` whose reference this process owns, if any.
///
/// The wrapper `AndroidApp::native_window()` hands out drops its own reference when the call ends,
/// so one of ours is needed for the renderer to use it. Taking one on every event and never
/// releasing it leaked a reference each time; the previous window is now released when it is
/// replaced.
static PUBLISHED_WINDOW: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

fn publish_window(app: &AndroidApp) {
    let Some(window) = app.native_window() else { return };
    let ptr = window.ptr().as_ptr() as *mut std::ffi::c_void;
    let width = window.width() as u32;
    let height = window.height() as u32;
    let previous = PUBLISHED_WINDOW.swap(ptr, std::sync::atomic::Ordering::SeqCst);
    // Acquire first, then tell the renderer, and only then drop the old one: at no point is the
    // window the renderer has been told about already freed.
    if previous != ptr {
        unsafe { ANativeWindow_acquire(ptr) };
    }
    android::set_native_window(ptr, width, height);
    if !previous.is_null() && previous != ptr {
        unsafe { ANativeWindow_release(previous) };
    }
}

extern "C" {
    /// Bumps the reference count of an `ANativeWindow`. From `libandroid`, always linked.
    fn ANativeWindow_acquire(window: *mut std::ffi::c_void) -> i32;
    /// Drops a reference taken with [ANativeWindow_acquire].
    fn ANativeWindow_release(window: *mut std::ffi::c_void);
}

/// Android can recreate the Activity inside the same process. Carrying the previous session's
/// input state over would leave keys held down (the player walking on their own, a wheel stuck...).
fn reset_input_state() {
    if let Ok(mut value) = POINTER_CONTROLS.lock() { *value = None; }
    if let Ok(mut value) = WHEEL_PTR.lock() { *value = None; }
    if let Ok(mut value) = FIELD_POINTER.lock() { *value = None; }
    if let Ok(mut value) = SWALLOWED_INPUTS.lock() { *value = None; }
    if let Ok(mut value) = MOUSE_DOWN_AT.lock() { value.clear(); }
    if let Ok(mut value) = DELAYED_UPS.lock() { value.clear(); }
    if let Ok(mut value) = DELAYED_DOWNS.lock() { value.clear(); }
    if let Ok(mut value) = MOVEMENT_PTR.lock() { *value = None; }
    MOVEMENT_STATE.store(0, std::sync::atomic::Ordering::Relaxed);
    STICK_STATE.store(0, std::sync::atomic::Ordering::Relaxed);
    IME_SHOWN.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Copies everything the engine prints to logcat.
///
/// An Android app's stdout and stderr go nowhere, so every diagnostic the emulator writes with
/// `println!`/`eprintln!` - the `Runtime error: ... (room N)` line that says why the runner stopped
/// above all - is thrown away, and a report of "the game threw me back to the app" arrives with no
/// reason in it. With the process's own stdout and stderr pointing at a pipe, all of it lands in
/// logcat, which is also what the launcher's log screen reads.
fn pipe_std_to_logcat() {
    // The native entry can run again if the Activity is recreated in the same process; a second
    // pipe thread would keep the first one blocked on a dead pipe forever.
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(install_std_log_pipe);
}

fn install_std_log_pipe() {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a valid array of two ints, which is what pipe() fills in.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return
    }
    // SAFETY: both ends are valid fds. Duplicating the write end onto stdout and stderr is what puts
    // the engine's prints in the pipe, and the spare write end is not needed after that.
    unsafe {
        libc::dup2(fds[1], libc::STDOUT_FILENO);
        libc::dup2(fds[1], libc::STDERR_FILENO);
        libc::close(fds[1]);
    }
    let read_fd = fds[0];
    std::thread::spawn(move || {
        use std::io::Read;
        use std::os::unix::io::FromRawFd;
        // SAFETY: read_fd is the pipe's read end and nothing else owns it.
        let mut pipe = unsafe { std::fs::File::from_raw_fd(read_fd) };
        let mut chunk = [0u8; 2048];
        let mut pending: Vec<u8> = Vec::new();
        let mut last = String::new();
        let mut repeats = 0u32;
        loop {
            let Ok(read) = pipe.read(&mut chunk) else { break };
            if read == 0 {
                break
            }
            pending.extend_from_slice(&chunk[..read]);
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                let text = String::from_utf8_lossy(&line).trim_end().to_string();
                if text.is_empty() {
                    continue
                }
                // A message repeated every frame - an unimplemented function called from a draw
                // event, say - would flood the log and slow the game down with it, so runs of the
                // same line are counted instead of copied.
                if text == last {
                    repeats += 1;
                    continue
                }
                if repeats > 0 {
                    android_log_line(&format!("  (repetido {} veces)", repeats));
                    repeats = 0;
                }
                last = text.clone();
                android_log_line(&text);
            }
        }
    });
}

/// One line to logcat, under the tag the launcher's log screen shows.
fn android_log_line(text: &str) {
    const ANDROID_LOG_INFO: i32 = 4;
    let text = format!("{}\0", text);
    // SAFETY: both arguments are NUL-terminated C strings that outlive the call.
    unsafe {
        __android_log_write(
            ANDROID_LOG_INFO,
            b"gm8emulator\0".as_ptr() as *const libc::c_char,
            text.as_ptr() as *const libc::c_char,
        );
    }
}

extern "C" {
    /// From `liblog`, which this library already depends on.
    fn __android_log_write(priority: i32, tag: *const libc::c_char, text: *const libc::c_char) -> i32;
}

/// Runs the emulator. `poll_events` must be driven from the thread `android_main` is called
/// on, because android-activity asserts it, so the pump cannot live on a worker thread.
/// The emulator itself runs on a worker thread instead.
pub fn android_entry(app: AndroidApp) {
    // Resolve the app's real internal storage from the running Activity before anything reads a
    // setting or writes a log. `/data/data/<package>` is only user 0's data: in a work profile, a
    // secondary user or a cloned app it points somewhere this process cannot write, which is what
    // made the engine fail with "Permission denied" creating `files/game` after a good install.
    let storage = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from(format!("/data/data/{}/files", PACKAGE_DIR)));
    let _ = STORAGE_DIR.set(storage);
    // First thing, so the opening lines of the session are captured as well.
    pipe_std_to_logcat();
    logf(&format!("almacenamiento: {}", storage_dir().display()));
    // Panics then land in the same log, with a backtrace.
    std::env::set_var("RUST_BACKTRACE", "1");
    // A recreated Activity must not inherit the previous session's held keys.
    reset_input_state();

    // The pad layout, the touch overlay and the video mode are read once, before any input can arrive.
    load_controls();
    load_touch();
    load_video();
    crate::touch_controls::load_layout(&fs::read_to_string(storage_path("touch-layout.conf")).unwrap_or_default());
    crate::touch_appearance::load(&fs::read_to_string(storage_path("touch-appearance.conf")).unwrap_or_default());

    publish_window(&app);

    let app = std::sync::Arc::new(app);
    let game_app = app.clone();

    // Set when the emulator returns, i.e. the game ended from its own "Salir del juego" (or failed).
    // Returning from `android_main` makes android-activity finish the Activity, so the launcher
    // comes back instead of leaving a dead black window on screen.
    let game_over = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let game_over_thread = game_over.clone();

    std::thread::spawn(move || {
        // The activity asks for landscape in the manifest, so the window is normally already the right
        // way round; the rotate still takes a moment on some phones. The engine lays its screens out
        // from the size of the window it is given, and whatever it gets here is what the whole session
        // keeps, so wait for the landscape window instead of racing it. Bounded, so a phone that never
        // reports one cannot hang the launch.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
        while std::time::Instant::now() < deadline {
            let (w, h) = android::window_size();
            if w > h {
                break
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let game_exe = match ensure_game_extracted() {
            Ok(exe) => exe,
            Err(e) => {
                logf(&format!("no se pudo preparar el juego: {}", e));
                game_over_thread.store(true, std::sync::atomic::Ordering::SeqCst);
                return
            },
        };

        crate::render::opengl::egl::log_stubbed_once();
        provision_soundfont(game_exe.parent().unwrap_or_else(|| std::path::Path::new("/")));
        logf(&format!("arrancando {}", game_exe.display()));
        // The emulator returning is what finishes the Activity, so how it returned - its exit code,
        // or a panic - is the answer to "why did the game kick me back to the launcher?".
        let outcome = std::panic::catch_unwind(|| {
            crate::run_game(vec!["gm8emulator".to_string(), game_exe.to_string_lossy().into_owned()])
        });
        match outcome {
            Ok(code) => logf(&format!("el emulador termino (codigo {}); se cierra la actividad", code)),
            Err(_) => logf("el emulador se cayo (panic); se cierra la actividad"),
        }
        game_over_thread.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let mut destroying = false;
    let mut input_context = crate::touch_controls::current_context();
    let mut input_identity=crate::touch_navigation::identity();
    let mut ime_check = Instant::now();
    let mut text_entry_was_open = false;
    loop {
        if game_over.load(std::sync::atomic::Ordering::SeqCst) || destroying {
            break
        }
        let context = crate::touch_controls::current_context();
        let identity=crate::touch_navigation::identity();
        if context!=input_context || identity!=input_identity {release_touch_profile();input_context=context;input_identity=identity;}
        if let Some(key)=crate::touch_navigation::next_key(){tap_key(key);}
        if let Some((down,x,y))=crate::touch_navigation::next_pointer(){
            use ramen::platform::android::InputEvent;
            android::push_input(if down {InputEvent::PointerDown{x,y,button:0}} else {InputEvent::PointerUp{x,y,button:0}});
        }
        // When the game moves to another state, drop the keys of controls that are no longer shown
        // so none of them stays stuck down.
        for index in crate::touch_controls::stale_pressed() {
            control_key_up(index);
        }
        flush_delayed_mouse();
        flush_pending_rotate();
        // The keyboard follows the game's text dialog: it comes up by itself the moment the game asks
        // for a name, and goes away when the dialog closes. Only the change is acted on, so closing
        // it by hand while the dialog is open is respected instead of being undone on the next tick;
        // touching the text brings it back (see `pointer_down`). The engine reports the dialog from
        // inside its own wait loop, so the game thread keeps the flag current even though the frame
        // loop is not coming round.
        let text_entry = crate::touch_controls::text_entry_open();
        if text_entry != text_entry_was_open {
            text_entry_was_open = text_entry;
            ime_set(text_entry);
        }
        // Belt and braces: while the keyboard is believed to be up, ask the activity for its height
        // again about once a second. A callback that never arrived would otherwise leave the game
        // pushed up top and the controls hidden, with no way for the player to get them back.
        if crate::touch_controls::ime_inset() > 0 && ime_check.elapsed() >= Duration::from_secs(1) {
            ime_check = Instant::now();
            jni_refresh_ime_height();
        }
        // While paused the engine is not drawing, so the pump does not need to spin: a long wait
        // still wakes immediately for Resume, input or Destroy.
        let poll_wait = if ramen::platform::android::paused() {
            Duration::from_secs(1)
        } else {
            Duration::from_millis(8)
        };
        game_app.poll_events(Some(poll_wait), |event| match event {
            PollEvent::Main(main_event) => match main_event {
                // Init/Resume/Start all mean the surface is (re)available, which is also what
                // happens when the user comes back from the home screen.
                android_activity::MainEvent::InitWindow { .. }
                | android_activity::MainEvent::WindowResized { .. }
                | android_activity::MainEvent::RedrawNeeded { .. }
                | android_activity::MainEvent::Resume { .. }
                | android_activity::MainEvent::GainedFocus
                | android_activity::MainEvent::Start => {
                    publish_window(&game_app);
                    if let Some(window) = game_app.native_window() {
                        // The activity has given us a window: build the GL surface for it now, while
                        // it is alive. Building it later, from the render loop, is what left the
                        // screen black and made EGL complain about a window already connected.
                        unsafe {
                            let ptr = window.ptr().as_ptr() as *mut std::ffi::c_void;
                            crate::render::opengl::egl::attach_surface(ptr);
                        }
                        android::push_resize(window.width() as u16, window.height() as u16);
                        // Only wake the engine once it has somewhere to draw. Resuming before the
                        // surface exists made the render loop spin without presenting a frame.
                        if crate::render::opengl::egl::has_surface() {
                            ramen::platform::android::set_paused(false);
                            crate::game::audio::set_output_paused(false);
                        }
                    }
                },
                // Going away. The surface is let go on `Pause`/`Stop` as well as on
                // `TerminateWindow`, because Android does not always send the latter - and a dead
                // surface left lying around is worse than none: the renderer believes it can draw,
                // and OpenGL answers with an error that kills the thread.
                android_activity::MainEvent::TerminateWindow { .. }
                | android_activity::MainEvent::Pause { .. }
                | android_activity::MainEvent::Stop => {
                    ramen::platform::android::set_paused(true);
                    crate::game::audio::set_output_paused(true);
                    unsafe { crate::render::opengl::egl::detach_surface() };
                },
                android_activity::MainEvent::InputAvailable => {
                    if let Ok(mut iter) = game_app.input_events_iter() {
                        loop {
                            let handled = iter.next(|event| {
                                translate(&event);
                                android_activity::InputStatus::Handled
                            });
                            if !handled {
                                break
                            }
                        }
                    }
                },
                // The system is tearing the activity down (the app was closed): stop pumping so
                // `android_main` can return and the activity really goes away.
                android_activity::MainEvent::Destroy => destroying = true,
                _ => {},
            },
            PollEvent::Wake => {},
            PollEvent::Timeout => {},
            _ => {},
        });
    }
    logf(&format!(
        "cerrando la actividad (juego terminado: {}, destruyendo: {})",
        game_over.load(std::sync::atomic::Ordering::SeqCst),
        destroying
    ));
}

/// android-activity provides `ANativeActivity_onCreate`; this is the plain-Rust-ABI symbol
/// it looks for and calls on a dedicated thread.
#[no_mangle]
fn android_main(app: AndroidApp) {
    android_entry(app);
}

// ---------------------------------------------------------------------------------------
// Asset provisioning
//
// The game ships as a .zip laid out exactly like the game folder it comes from, so
// extracting it reproduces the layout the engine already expects on Linux. It is found
// in a list of conventional places, extracted once into the app's own storage, and then
// the game runs straight off the extracted files.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Where the app looks for `game.zip`, most specific first. The first two are the app's own
/// storage and need no permissions; the last one is shared storage and may need one.
const ZIP_NAME: &str = "game.zip";
pub(crate) const PACKAGE_DIR: &str = "com.klisee.reloaded";

/// The app's internal storage, as the running Activity reports it (`Context.getFilesDir()`).
/// `android_entry` sets it from `AndroidApp::internal_data_path()`; the fallback only matters on a
/// host build or if the Activity is already gone.
static STORAGE_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub(crate) fn storage_dir() -> PathBuf {
    STORAGE_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(format!("/data/data/{}/files", PACKAGE_DIR)))
}

pub(crate) fn storage_path(name: &str) -> PathBuf {
    storage_dir().join(name)
}

/// Free bytes on the filesystem containing `path`, or `None` if it can't be queried.
fn free_bytes_on(path: &Path) -> Option<u64> {
    // SAFETY: zeroed statvfs is a valid argument, and the call only reads.
    unsafe {
        let path_c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
        let mut stats: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path_c.as_ptr(), &mut stats) != 0 {
            return None
        }
        // `statvfs` widens these fields on 64-bit targets and keeps them 32-bit on armv7.
        Some(stats.f_bavail as u64 * stats.f_frsize as u64)
    }
}

/// Enough room for the zip plus its extracted contents.
fn enough_space(zip_len: u64, target: &Path) -> bool {
    match free_bytes_on(target.parent().unwrap_or(target)) {
        Some(free) => free > zip_len * 2,
        None => true, // can't tell, so let the extraction try and fail loudly if it must
    }
}

fn zip_candidates() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from(format!("/sdcard/Android/data/{}/files", PACKAGE_DIR)),
        storage_dir(),
        PathBuf::from("/sdcard/Download".to_string()),
        PathBuf::from("/sdcard".to_string()),
        // Writable by `adb push`, which is how the game is usually sideloaded for testing.
        PathBuf::from("/data/local/tmp".to_string()),
    ];
    dirs.retain(|d| d.is_dir());
    dirs.into_iter().map(|d| d.join(ZIP_NAME)).filter(|p| p.is_file()).collect()
}

/// The `.exe` inside the extracted game directory. Its name is not pinned, so a different
/// version of the game, or another GameMaker 8 fangame, works unchanged.
fn find_exe(dir: &Path) -> Option<PathBuf> {
    let mut found = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        })
        .collect::<Vec<_>>();
    found.sort();
    found.into_iter().next()
}

fn logf(msg: &str) {
    // `eprintln` goes to logcat under NativeActivity, which is where this is read from.
    eprintln!("[assets] {}", msg);
    // Keep a small file too, so the launcher can show a log without needing adb/logcat.
    append_log_file(msg);
}

/// Appends one line to `<files>/emulator.log`, rotating once past [LOG_MAX_BYTES].
fn append_log_file(msg: &str) {
    const LOG_MAX_BYTES: u64 = 256 * 1024;
    let path = storage_path("emulator.log");
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = (|| -> std::io::Result<()> {
        use std::io::Write;
        if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > LOG_MAX_BYTES {
            let _ = fs::rename(&path, path.with_extension("log.1"));
        }
        // O_APPEND means concurrent writers interleave whole lines rather than corrupting them.
        let mut file = fs::OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(file, "{}", msg)
    })();
}

/// MIDI music needs a General MIDI SoundFont, and Android ships none at the paths the engine looks
/// at. If one was placed next to the game (or pushed to the app's storage), point the engine at it.
fn provision_soundfont(game_dir: &Path) {
    let candidates = [
        game_dir.join("soundfont.sf2"),
        game_dir.join("TimGM6mb.sf2"),
        PathBuf::from(format!("/sdcard/Android/data/{}/files/soundfont.sf2", PACKAGE_DIR)),
        PathBuf::from("/sdcard/Download/soundfont.sf2"),
    ];
    for path in candidates {
        if path.is_file() {
            logf(&format!("soundfont MIDI: {}", path.display()));
            std::env::set_var("OPENGMK_SOUNDFONT", &path);
            return
        }
    }
    logf("aviso: no hay .sf2; la musica MIDI no sonara (copia soundfont.sf2 a la carpeta del juego)");
}

/// Makes the game available on disk and returns the path of its executable.
fn ensure_game_extracted() -> Result<PathBuf, String> {
    let storage = storage_dir();
    let game_dir = storage.join("game");

    // Already provisioned? This is checked before creating anything: when the launcher installed
    // the game, reading the exe is all that has to happen at startup.
    if let Some(exe) = find_exe(&game_dir) {
        if game_dir.join("data").is_dir() {
            logf(&format!("juego ya extraido en {}", game_dir.display()));
            return Ok(exe)
        }
    }

    if game_dir.exists() && !game_dir.is_dir() {
        return Err(format!("{} existe pero no es una carpeta", game_dir.display()))
    }
    if !game_dir.is_dir() {
        fs::create_dir_all(&game_dir).map_err(|e| {
            format!(
                "no se pudo crear {}: {} (almacenamiento real: {})",
                game_dir.display(),
                e,
                storage.display()
            )
        })?;
    }

    let zips = zip_candidates();
    if zips.is_empty() {
        return Err(format!(
            "no encontre {}. Copialo a /sdcard/Android/data/{}/files/",
            ZIP_NAME, PACKAGE_DIR
        ))
    }
    let zip_path = &zips[0];
    let zip_len = fs::metadata(zip_path).map_err(|e| format!("{}: {}", zip_path.display(), e))?.len();
    logf(&format!("encontrado {} ({} bytes)", zip_path.display(), zip_len));

    let file = fs::File::open(zip_path).map_err(|e| format!("{}: {}", zip_path.display(), e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("no pude leer el zip {}: {}", zip_path.display(), e))?;
    // The zip is compressed: what has to fit on disk is its contents, not the archive itself.
    let mut uncompressed = 0u64;
    for index in 0..archive.len() {
        if let Ok(entry) = archive.by_index_raw(index) {
            uncompressed += entry.size();
        }
    }
    if !enough_space(uncompressed.max(zip_len), &game_dir) {
        return Err(format!(
            "espacio insuficiente en {}: el juego ocupa unos {} MB",
            storage.display(),
            uncompressed / (1024 * 1024)
        ))
    }

    // Extract into a staging dir and swap it in, so a failed run leaves the previous
    // contents intact rather than a half-extracted game.
    let staging = storage.join("game.staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|e| format!("no pude limpiar {}: {}", staging.display(), e))?;
    }
    fs::create_dir_all(&staging).map_err(|e| format!("no se pudo crear {}: {}", staging.display(), e))?;

    logf(&format!("extrayendo {} entradas", archive.len()));
    archive
        .extract(&staging)
        .map_err(|e| format!("fallo la extraccion de {}: {}", zip_path.display(), e))?;

    let exe = find_exe(&staging).ok_or_else(|| "el zip no contiene ningun .exe en su raiz".to_string())?;
    if !staging.join("data").is_dir() {
        let _ = fs::remove_dir_all(&staging);
        return Err("el zip no contiene la carpeta data/ en su raiz".into())
    }

    // Keep the previous install aside until the new one is in place.
    let previous = storage.join("game.old");
    let _ = fs::remove_dir_all(&previous);
    let moved_aside = game_dir.is_dir() && fs::rename(&game_dir, &previous).is_ok();
    if let Err(e) = fs::rename(&staging, &game_dir) {
        if moved_aside {
            let _ = fs::rename(&previous, &game_dir);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("no pude mover lo extraido a {}: {}", game_dir.display(), e))
    }
    let _ = fs::remove_dir_all(&previous);

    let exe = game_dir.join(exe.file_name().unwrap());
    logf(&format!("listo: {}", exe.display()));
    Ok(exe)
}
