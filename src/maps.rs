//! The maps of Overwatch's 5v5 and 6v6 queues, by the kind of match played on them. Read on 15 Nov 2026 from the
//! map filter of Blizzard's own hero statistics page (overwatch.blizzard.com/en-us/rates), which lists them for
//! "Quick Play - Role Queue" and "Competitive - Role Queue": 31 maps, the newest being Grímsvötn (6 Oct 2026).
//! The two Clash maps are not in it; they are played in Stadium only. No list for 6v6 alone is published; the
//! same maps are offered for it.
//!
//! The recorder offers this list after a recording made in one of those queues (Luka, 15 Nov 2026). A map that
//! is newer than this list can still be typed in.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Control,
    Escort,
    Hybrid,
    Push,
    Flashpoint,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Control => "Control",
            Kind::Escort => "Escort",
            Kind::Hybrid => "Hybrid",
            Kind::Push => "Push",
            Kind::Flashpoint => "Flashpoint",
        }
    }
}

pub struct Map {
    pub name: &'static str,
    pub kind: Kind,
}

pub const MAPS: [Map; 31] = [
    Map { name: "Antarctic Peninsula", kind: Kind::Control },
    Map { name: "Busan", kind: Kind::Control },
    Map { name: "Ilios", kind: Kind::Control },
    Map { name: "Lijiang Tower", kind: Kind::Control },
    Map { name: "Nepal", kind: Kind::Control },
    Map { name: "Oasis", kind: Kind::Control },
    Map { name: "Samoa", kind: Kind::Control },
    Map { name: "Circuit Royal", kind: Kind::Escort },
    Map { name: "Dorado", kind: Kind::Escort },
    Map { name: "Grímsvötn", kind: Kind::Escort },
    Map { name: "Havana", kind: Kind::Escort },
    Map { name: "Junkertown", kind: Kind::Escort },
    Map { name: "Rialto", kind: Kind::Escort },
    Map { name: "Route 66", kind: Kind::Escort },
    Map { name: "Shambali Monastery", kind: Kind::Escort },
    Map { name: "Watchpoint: Gibraltar", kind: Kind::Escort },
    Map { name: "Blizzard World", kind: Kind::Hybrid },
    Map { name: "Eichenwalde", kind: Kind::Hybrid },
    Map { name: "Hollywood", kind: Kind::Hybrid },
    Map { name: "King's Row", kind: Kind::Hybrid },
    Map { name: "Midtown", kind: Kind::Hybrid },
    Map { name: "Neon Junction", kind: Kind::Hybrid },
    Map { name: "Numbani", kind: Kind::Hybrid },
    Map { name: "Paraíso", kind: Kind::Hybrid },
    Map { name: "Colosseo", kind: Kind::Push },
    Map { name: "Esperança", kind: Kind::Push },
    Map { name: "New Queen Street", kind: Kind::Push },
    Map { name: "Runasapi", kind: Kind::Push },
    Map { name: "Aatlis", kind: Kind::Flashpoint },
    Map { name: "New Junk City", kind: Kind::Flashpoint },
    Map { name: "Suravasa", kind: Kind::Flashpoint },
];

/// A name as it is compared with what was typed: lower case, accents dropped, letters and digits only, one space
/// between words. "Paraíso" and "paraiso" are the same; so are "King's Row" and "kings row".
pub fn plain(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        };
        if c.is_alphanumeric() {
            out.push(c);
        } else if c.is_whitespace() && !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

/// The listed maps that fit what was typed, best first: a name that begins with it, then a name with a word that
/// begins with it, then a name that holds it anywhere. Nothing typed fits nothing.
pub fn matching(typed: &str) -> Vec<&'static Map> {
    let t = plain(typed);
    if t.is_empty() {
        return Vec::new();
    }
    let rank = |m: &Map| {
        let n = plain(m.name);
        if n.starts_with(&t) {
            Some(0)
        } else if n.split(' ').any(|w| w.starts_with(&t)) {
            Some(1)
        } else if n.contains(&t) || n.replace(' ', "").contains(&t.replace(' ', "")) {
            Some(2)
        } else {
            None
        }
    };
    let mut found: Vec<(u8, &'static Map)> = MAPS.iter().filter_map(|m| rank(m).map(|r| (r, m))).collect();
    found.sort_by_key(|(r, _)| *r); // stable: the list's own order inside a rank
    found.into_iter().map(|(_, m)| m).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(typed: &str) -> Vec<&'static str> {
        matching(typed).iter().map(|m| m.name).collect()
    }

    #[test]
    fn the_list_is_whole_and_each_name_can_be_sent() {
        let count = |k: Kind| MAPS.iter().filter(|m| m.kind == k).count();
        assert_eq!((count(Kind::Control), count(Kind::Escort), count(Kind::Hybrid), count(Kind::Push), count(Kind::Flashpoint)), (7, 9, 8, 4, 3));
        for (i, m) in MAPS.iter().enumerate() {
            assert!(m.name.chars().count() <= 40 && !m.name.is_empty(), "{}", m.name);
            assert!(MAPS[..i].iter().all(|o| plain(o.name) != plain(m.name)), "{} is listed twice", m.name);
        }
    }

    #[test]
    fn a_few_letters_find_a_map() {
        assert_eq!(names("ki"), ["King's Row"]);
        assert_eq!(names("kings row"), ["King's Row"]);
        assert_eq!(names("KINGSROW"), ["King's Row"]);
        assert_eq!(names("paraiso"), ["Paraíso"]);
        assert_eq!(names("Grims"), ["Grímsvötn"]);
        assert_eq!(names("esperanca"), ["Esperança"]);
        assert_eq!(names("gib"), ["Watchpoint: Gibraltar"]);
        assert_eq!(names("66"), ["Route 66"]);
        // a name that begins with what was typed comes before one that only holds it
        assert_eq!(names("new"), ["New Queen Street", "New Junk City"]);
        assert_eq!(names("n")[..5], ["Nepal", "Neon Junction", "Numbani", "New Queen Street", "New Junk City"]);  // then the names that only hold an n
        assert_eq!(names("o")[0], "Oasis");
        assert_eq!(names("zz"), ["Blizzard World"]);
        assert!(names("xq").is_empty() && names("  ").is_empty() && names("'").is_empty());
    }
}
