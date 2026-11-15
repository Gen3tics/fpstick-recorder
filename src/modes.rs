//! Overwatch's play modes, as the game's own menu lists them (read from screenshots of every tab of the Play
//! menu, 15 Nov 2026: Unranked, Competitive, Stadium, Arcade, More). The recorder asks for one of these after
//! a recording; the run page shows it as a tag in the colour the game gives that tab.
//!
//! Arcade is one entry: its modes change from patch to patch. A mode that is played on a random pool of maps or
//! on one fixed place (Stadium, the practice ranges, the tutorial, missions) has no map to name.
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
    /// Whether there is a map to name for a recording in this mode.
    pub map: bool,
}

pub const MODES: [Mode; 14] = [
    Mode { name: "Quick Play 5v5", family: Family::Unranked, map: true },
    Mode { name: "Quick Play 6v6", family: Family::Unranked, map: true },
    Mode { name: "Mystery Heroes", family: Family::Unranked, map: true },
    Mode { name: "Competitive 5v5", family: Family::Competitive, map: true },
    Mode { name: "Competitive 6v6", family: Family::Competitive, map: true },
    Mode { name: "Stadium Quick Play", family: Family::Stadium, map: false },
    Mode { name: "Stadium Competitive", family: Family::Stadium, map: false },
    Mode { name: "Arcade", family: Family::Arcade, map: true },
    Mode { name: "Custom game", family: Family::Custom, map: true },
    Mode { name: "Practice range", family: Family::Practice, map: false },
    Mode { name: "Practice Stadium", family: Family::Practice, map: false },
    Mode { name: "Practice vs. AI", family: Family::Other, map: true },
    Mode { name: "Missions", family: Family::Other, map: false },
    Mode { name: "Tutorial", family: Family::Other, map: false },
];

pub fn find(name: &str) -> Option<&'static Mode> {
    MODES.iter().find(|m| m.name == name)
}

/// Whether a recording said to be in this mode has a map to name. A mode that is not in the list (nothing
/// chosen, or text from before the list existed) does.
pub fn has_map(name: &str) -> bool {
    find(name).map(|m| m.map).unwrap_or(true)
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
        assert!(!has_map("Stadium Competitive") && !has_map("Practice range") && has_map("Competitive 6v6") && has_map("") && has_map("6v6 comp"));
    }
}
