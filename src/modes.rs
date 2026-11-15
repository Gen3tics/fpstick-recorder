//! Overwatch's play modes, as the game's own menu lists them (read from screenshots of every tab of the Play
//! menu, 15 Nov 2026: Unranked, Competitive, Stadium, Arcade, More). The recorder asks for one of these after
//! a recording; the run page shows it as a tag in the colour the game gives that tab.
//!
//! Arcade is one entry: its modes change from patch to patch. The map is asked for only in the 5v5 and 6v6 queues,
//! quick play and competitive (Luka, 15 Nov 2026): that is where nearly everyone plays, and their maps are one
//! known list (`maps.rs`).
//!
//! The run page has the same list in `god/static/public/modes.js`; `tests/modes_check.js` holds the two together.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Unranked,
    Competitive,
    Stadium,
    Arcade,
    Custom,
    Practice,
    Other,
}

impl Family {
    /// The family's colour: the game's own for that tab or tile, lightened where it would be hard to read on a
    /// dark ground (contrast of at least 5.5 to 1 on the run page's background).
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Family::Unranked => (0x3c, 0x87, 0xf9),    // the game's #146ff8, lightened
            Family::Competitive => (0xea, 0x53, 0x7a), // the game's #e94a73, lightened
            Family::Stadium => (0xfa, 0x6d, 0x1e),
            Family::Arcade => (0x0a, 0xc4, 0x0a),
            Family::Custom => (0xb8, 0x66, 0xd9), // the game's #722492, lightened
            Family::Practice => (0xe8, 0xa5, 0x09),
            Family::Other => (0x9a, 0xa3, 0xad),
        }
    }
}

pub struct Mode {
    pub name: &'static str,
    pub family: Family,
    /// Whether the map is asked for after a recording in this mode.
    pub map: bool,
}

pub const MODES: [Mode; 14] = [
    Mode { name: "Quick Play 5v5", family: Family::Unranked, map: true },
    Mode { name: "Quick Play 6v6", family: Family::Unranked, map: true },
    Mode { name: "Mystery Heroes", family: Family::Unranked, map: false },
    Mode { name: "Competitive 5v5", family: Family::Competitive, map: true },
    Mode { name: "Competitive 6v6", family: Family::Competitive, map: true },
    Mode { name: "Stadium Quick Play", family: Family::Stadium, map: false },
    Mode { name: "Stadium Competitive", family: Family::Stadium, map: false },
    Mode { name: "Arcade", family: Family::Arcade, map: false },
    Mode { name: "Custom game", family: Family::Custom, map: false },
    Mode { name: "Practice range", family: Family::Practice, map: false },
    Mode { name: "Practice Stadium", family: Family::Practice, map: false },
    Mode { name: "Practice vs. AI", family: Family::Other, map: false },
    Mode { name: "Missions", family: Family::Other, map: false },
    Mode { name: "Tutorial", family: Family::Other, map: false },
];

pub fn find(name: &str) -> Option<&'static Mode> {
    MODES.iter().find(|m| m.name == name)
}

/// Whether the map is asked for, and sent, with a recording said to be in this mode. Not for a mode that is not
/// in the list (nothing chosen yet).
pub fn has_map(name: &str) -> bool {
    find(name).map(|m| m.map).unwrap_or(false)
}

/// Whether a recording in this mode is named by an import code: a custom game, which is started from one
/// (VAXTA, the aim trainer nearly every player knows; or a player's own).
pub fn has_code(name: &str) -> bool {
    find(name).map(|m| m.family == Family::Custom).unwrap_or(false)
}

/// An import code as the game writes it: capital letters and digits, at most eight.
pub fn code_shown(typed: &str) -> String {
    typed.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_distinct_and_short_enough_to_send() {
        for (i, m) in MODES.iter().enumerate() {
            assert!(m.name.len() <= 40 && !m.name.is_empty(), "{}", m.name);
            assert!(MODES[..i].iter().all(|o| o.name != m.name), "{} is listed twice", m.name);
        }
        assert!(has_map("Quick Play 5v5") && has_map("Quick Play 6v6") && has_map("Competitive 5v5") && has_map("Competitive 6v6"));
        assert_eq!(MODES.iter().filter(|m| m.map).count(), 4);
        assert!(has_code("Custom game") && !has_code("Competitive 5v5") && !has_code(""));
        assert_eq!((code_shown(" vaxta "), code_shown("eat-xtt"), code_shown("abcdefghijk"), code_shown("é!")), ("VAXTA".into(), "EATXTT".into(), "ABCDEFGH".into(), String::new()));
        assert!(!has_map("Stadium Competitive") && !has_map("Custom game") && !has_map("Mystery Heroes") && !has_map("") && !has_map("6v6 comp"));
    }
}
