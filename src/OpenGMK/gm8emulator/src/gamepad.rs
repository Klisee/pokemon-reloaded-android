//! Soporte de mandos para escritorio (Linux y Windows).
//!
//! El juego solo lee el teclado, así que el mando se traduce a las mismas teclas que usa
//! un teclado: D-pad y stick izquierdo a flechas, y los botones a Z/X/C/V/Enter/Espacio/B/M/O/S/E.
//! Así funciona con cualquier rebinding que haya hecho el jugador en el launcher, porque el
//! remapeo se aplica después, en `Game::process_window_events`.

use gilrs::{Button, EventType, Gilrs};

pub struct Gamepads {
    gilrs: Gilrs,
    active: bool,
}

impl Gamepads {
    pub fn new() -> Option<Self> {
        let gilrs = Gilrs::new().ok()?;
        let names: Vec<String> = gilrs.gamepads().map(|(_, pad)| pad.name().to_string()).collect();
        let active = !names.is_empty();
        if active {
            eprintln!("[pad] mandos detectados: {}", names.join(", "));
        }
        Some(Self { gilrs, active })
    }

    /// Vacía la cola de eventos del mando y devuelve las teclas que hay que pulsar o soltar.
    pub fn poll(&mut self) -> Vec<PadAction> {
        let mut actions = Vec::new();
        while let Some(event) = self.gilrs.next_event() {
            match event.event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(key) = button_key(button) {
                        actions.push(PadAction::Down(key));
                    }
                },
                EventType::ButtonReleased(button, _) => {
                    if let Some(key) = button_key(button) {
                        actions.push(PadAction::Up(key));
                    }
                },
                EventType::AxisChanged(_, _, _) => {
                    // Los ejes se leen enteros en cada pasada para no depender del delta.
                },
                _ => (),
            }
        }
        // El stick se evalúa como estado, no como evento: así no se pierde si el sistema
        // agrupa varios cambios en un solo evento.
        if let Some((id, _)) = self.gilrs.gamepads().next() {
            let pad = self.gilrs.gamepad(id);
            let x = pad.value(gilrs::Axis::LeftStickX);
            let y = pad.value(gilrs::Axis::LeftStickY);
            let left = x < -DEADZONE || pad.is_pressed(Button::DPadLeft);
            let right = x > DEADZONE || pad.is_pressed(Button::DPadRight);
            let up = y > DEADZONE || pad.is_pressed(Button::DPadUp);
            let down = y < -DEADZONE || pad.is_pressed(Button::DPadDown);
            for (held, key) in [(left, PadKey::Left), (right, PadKey::Right), (up, PadKey::Up), (down, PadKey::Down)] {
                actions.push(if held { PadAction::Down(key) } else { PadAction::Up(key) });
            }
        }
        actions
    }
}

const DEADZONE: f32 = 0.35;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PadKey {
    Z,
    X,
    C,
    V,
    Return,
    Space,
    B,
    M,
    O,
    S,
    E,
    Up,
    Down,
    Left,
    Right,
}

pub enum PadAction {
    Down(PadKey),
    Up(PadKey),
}

fn button_key(button: Button) -> Option<PadKey> {
    Some(match button {
        Button::South => PadKey::Z,
        Button::East => PadKey::X,
        Button::West => PadKey::C,
        Button::North => PadKey::V,
        Button::Start => PadKey::Space,
        Button::Select => PadKey::Return,
        Button::RightTrigger => PadKey::B,
        Button::LeftTrigger => PadKey::M,
        Button::RightTrigger2 => PadKey::O,
        Button::LeftTrigger2 => PadKey::S,
        Button::LeftThumb => PadKey::E,
        _ => return None,
    })
}
