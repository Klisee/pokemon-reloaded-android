//! Independent, spec-derived clean-room modules for Reloaded OpenGMK.
//!
//! These modules do not depend on F3R GML, the game's data files, or the host
//! emulator. They are intentionally isolated so the host can adopt them behind
//! adapters without contaminating the core.

pub mod ports;
pub mod save;
pub mod team;
pub mod trade;
pub mod world;
