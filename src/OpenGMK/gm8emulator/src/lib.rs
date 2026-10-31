//! Library root for the GameMaker Classic emulator.
//!
//! All modules live here so that both the desktop binary (`src/main.rs`) and the Android
//! entry point (`src/android_main.rs`) can share the same tree. [`run_game`] is the single
//! entry point used by both.

mod action;
mod asset;
mod game;
mod gml;
mod handleman;
mod imgui_key_utils;
mod imgui_utils;
mod input;
mod instance;
mod instancelist;
mod math;
mod mods;
mod render;
mod tile;
mod types;
mod util;

#[cfg(target_os = "android")]
mod android_main;

/// The on-screen touch controls and their customisable layout. Built on every target so the
/// `dump_layout` example can turn the built-in layout into the file the launcher app draws.
pub mod touch_controls;
pub mod touch_appearance;
pub mod touch_skin;
pub mod field_controls;
pub mod touch_navigation;

mod main_impl;

mod scan;

pub use main_impl::{help, run_game};

#[cfg(target_os = "android")]
pub use android_main::android_entry;
