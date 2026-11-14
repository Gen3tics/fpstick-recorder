//! The recorder: chooses, records, packs and sends. No charts; the run page opens in the browser.
//!
//! `frames` and `pack` (the crate `framedata`) read PresentMon's file and pack it; the server that judges
//! a recording unpacks it with the same crate. `session` runs one recording from countdown to packed file. `platform` is everything
//! that talks to Windows.

pub use framedata::{frames, pack};

pub mod config;
pub mod net;
pub mod platform;
pub mod presentmon;
pub mod send;
pub mod session;
pub mod settings;
