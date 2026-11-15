//! What the person chose in the set-up window. Kept between runs in a small file of its own.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Seconds between pressing Record and the recording starting: time to get back into the game.
pub const COUNTDOWN_S: u64 = 5;
/// A recording ends by itself after this long, so a forgotten one does not run all night.
pub const CAP_S: u64 = 45 * 60;
/// How long the stop key is held when "hold to stop" is on.
pub const HOLD_MS: u64 = 1000;
/// How often the stop key is looked at while recording, in milliseconds. A key that must be held for a
/// second is found by a look every tenth of a second; a single press needs a look often enough not to miss it.
pub const LOOK_HOLD_MS: u32 = 100;
pub const LOOK_PRESS_MS: u32 = 25;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    InQueue,
    InMatch,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Length {
    Minutes(u64),
    WholeMatch,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Choices {
    pub place: Place,
    /// "Game network stats": the times and sizes of the game's packets.
    pub network: bool,
    /// With the network stats only: pings to the home router, the provider's first router and the last
    /// router before the server. They are most of what the recorder itself costs while recording.
    pub pings: bool,
    pub length: Length,
    /// Windows virtual-key code of the stop key. 0x06 is the mouse's front side button.
    pub stop_key: u16,
    pub hold_to_stop: bool,
    pub map: String,
    pub mode: String,
    /// A custom game's import code. Kept from one recording to the next: the same code is played again and again.
    pub code: String,
    pub process: String,
    /// Where Send goes. Empty: the run-page program on this PC. Otherwise the address of a run-page server,
    /// such as http://192.168.1.20:8770.
    pub send_to: String,
}

impl Default for Choices {
    fn default() -> Self {
        Choices {
            place: Place::InQueue,
            network: true,
            pings: true,
            length: Length::WholeMatch,
            stop_key: 0x06,
            hold_to_stop: true,
            map: String::new(),
            mode: String::new(),
            code: String::new(),
            process: "Overwatch.exe".into(),
            send_to: String::new(),
        }
    }
}

impl Choices {
    /// Choosing where you are presets the rest: from the queue a whole match, in a match a timed stretch.
    pub fn pick_place(&mut self, place: Place) {
        self.place = place;
        match place {
            Place::InQueue => self.length = Length::WholeMatch,
            Place::InMatch => {
                if self.length == Length::WholeMatch {
                    self.length = Length::Minutes(2)
                }
            }
        }
    }

    /// The fixed length in seconds, or None for "until the stop key".
    pub fn seconds(&self) -> Option<u64> {
        match self.length {
            Length::Minutes(m) => Some(m.clamp(1, CAP_S / 60) * 60),
            Length::WholeMatch => None,
        }
    }

    pub fn file() -> PathBuf {
        crate::platform::appdata_dir().join("choices.json")
    }

    pub fn load() -> Choices {
        let mut c: Choices = std::fs::read(Self::file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // the mode is chosen from the game's own list since 15 Nov 2026: text typed in before that is dropped
        if crate::modes::find(&c.mode).is_none() {
            c.mode.clear();
        }
        c
    }

    /// The map to say for this recording: none in a mode that has no map to name, whatever was typed earlier.
    pub fn map_said(&self) -> &str {
        if crate::modes::has_map(&self.mode) { self.map.trim() } else { "" }
    }

    /// The import code to say for this recording: only for a custom game.
    pub fn code_said(&self) -> String {
        if crate::modes::has_code(&self.mode) { crate::modes::code_shown(&self.code) } else { String::new() }
    }

    pub fn save(&self) {
        let f = Self::file();
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(f, b);
        }
    }
}

/// What a stop key is called on screen.
pub fn key_name(vk: u16) -> String {
    match vk {
        0x04 => "Mouse 3 · middle button".into(),
        0x05 => "Mouse 4 · back side button".into(),
        0x06 => "Mouse 5 · front side button".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x13 => "Pause".into(),
        0x14 => "Caps Lock".into(),
        0x20 => "Space".into(),
        0x21 => "Page Up".into(),
        0x22 => "Page Down".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x30..=0x39 | 0x41..=0x5A => ((vk as u8) as char).to_string(),
        0x60..=0x69 => format!("Numpad {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x90 => "Num Lock".into(),
        0x91 => "Scroll Lock".into(),
        _ => format!("Key {:#04x}", vk),
    }
}

/// Keys that may be chosen as the stop key: not the two main mouse buttons, not the modifier keys.
pub fn key_allowed(vk: u16) -> bool {
    !matches!(vk, 0x00..=0x03 | 0x10..=0x12 | 0x1B | 0x5B | 0x5C | 0xA0..=0xA5 | 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_you_are_presets_the_length() {
        let mut c = Choices::default();
        assert_eq!((c.length, c.seconds()), (Length::WholeMatch, None));
        c.pick_place(Place::InMatch);
        assert_eq!(c.seconds(), Some(120));
        c.length = Length::Minutes(5);
        c.pick_place(Place::InMatch);
        assert_eq!(c.seconds(), Some(300));
        c.pick_place(Place::InQueue);
        assert_eq!(c.length, Length::WholeMatch);
    }

    #[test]
    fn choices_survive_a_round_trip_and_an_old_file() {
        let c = Choices { map: "King's Row".into(), stop_key: 0x75, ..Choices::default() };
        let back: Choices = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        let old: Choices = serde_json::from_str("{\"map\":\"Ilios\"}").unwrap();
        assert_eq!((old.map.as_str(), old.stop_key, old.hold_to_stop), ("Ilios", 0x06, true));
        assert!(old.pings, "choices saved before the ping tick box existed keep pinging");
    }

    #[test]
    fn keys_have_names() {
        assert_eq!(key_name(0x06), "Mouse 5 · front side button");
        assert_eq!(key_name(0x7A), "F11");
        assert_eq!(key_name(0x4B), "K");
        assert!(!key_allowed(0x01) && !key_allowed(0x1B) && key_allowed(0x06));
    }
}
