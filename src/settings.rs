//! The game's own settings file, read when a recording starts and again when it ends.
//!
//! Overwatch keeps its settings in `Documents\Overwatch\Settings\Settings_v0.ini`: sections such as
//! `[Render.13]` holding lines such as `RenderScale = "0"`. Only the sections whose name begins with
//! "Render" are read: they hold the graphics settings. The copy goes into the recording's `run.json` and
//! stays on the PC; nothing of it is sent yet.
//!
//! Two limits, both noted with the copy:
//!   - The game is said to write this file only when it closes. A setting changed after the game was
//!     started would then not show until the game is closed, so what is read during a match is what the
//!     game last saved, which is usually but not surely what it is running with.
//!   - A person can edit the file by hand. It is evidence, not proof.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const FILE: [&str; 3] = ["Overwatch", "Settings", "Settings_v0.ini"];
const MAX_BYTES: u64 = 256 * 1024;
const MAX_KEYS: usize = 300;

pub fn file() -> PathBuf {
    FILE.iter().fold(crate::platform::documents_dir(), |p, part| p.join(part))
}

/// The lines of the sections whose name begins with "Render", as section -> key -> value.
pub fn render_sections(text: &str) -> Map<String, Value> {
    let mut out: Map<String, Value> = Map::new();
    let (mut section, mut keys): (Option<String>, usize) = (None, 0);
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_ascii_lowercase().starts_with("render").then(|| name.chars().take(40).collect());
            continue;
        }
        let (Some(sec), Some((k, v))) = (&section, line.split_once('=')) else { continue };
        let (k, v) = (k.trim(), v.trim().trim_matches('"'));
        if k.is_empty() || k.len() > 60 || v.len() > 120 || keys >= MAX_KEYS || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            continue;
        }
        keys += 1;
        let entry = out.entry(sec.clone()).or_insert_with(|| json!({}));
        entry[k] = json!(v);
    }
    out
}

/// What the file holds now: None if it is not there or cannot be read.
pub fn read() -> Option<Value> {
    read_at(&file())
}

pub fn read_at(path: &Path) -> Option<Value> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    // the file is plain text; a byte-order mark at its start is skipped
    let text = String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes)).into_owned();
    let render = render_sections(&text);
    if render.is_empty() {
        return None;
    }
    let age_s = meta.modified().ok().and_then(|m| m.elapsed().ok()).map(|d| d.as_secs());
    Some(json!({"render": render, "file_last_written_s_ago": age_s}))
}

/// The note for run.json from the copy taken at the start and the one taken at the end.
pub fn note(start: Option<Value>, end: Option<Value>) -> Value {
    let Some(start) = start else { return json!({"read": false, "why": "The game's settings file was not found where Overwatch keeps it."}) };
    let changed = end.as_ref().map(|e| e["render"] != start["render"]).unwrap_or(false);
    json!({
        "read": true,
        "file": FILE.join("\\"),
        "at_start": start,
        "changed_while_recording": changed,
        "at_end": if changed { end } else { None },
        "caution": "The game is said to write this file only when it closes, so a setting changed since the game was started may not show here.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Cinematics.1]\r\nShowIntro = \"0\"\r\n\r\n[GPU.6]\r\nGPUName = \"NVIDIA GeForce GTX 1050\"\r\n\r\n[Render.13]\r\nAADetail = \"0\"\r\nFullScreenRefresh = \"60\"\r\nRenderScale = \"-2\"\r\nWindowMode = \"2\"\r\nodd key = \"x\"\r\n\r\n[Sound.3]\r\nMusicVolume = \"10.000000\"\r\n";

    #[test]
    fn only_the_render_sections_are_read() {
        let r = render_sections(SAMPLE);
        assert_eq!(r.len(), 1);
        assert_eq!(r["Render.13"], json!({"AADetail": "0", "FullScreenRefresh": "60", "RenderScale": "-2", "WindowMode": "2"}));
        assert!(render_sections("no sections here").is_empty());
    }

    #[test]
    fn a_change_between_start_and_end_is_noted() {
        let dir = std::env::temp_dir().join(format!("recorder-settings-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("Settings_v0.ini");
        std::fs::write(&f, SAMPLE).unwrap();
        let a = read_at(&f);
        assert_eq!(a.as_ref().unwrap()["render"]["Render.13"]["RenderScale"], "-2");
        std::fs::write(&f, SAMPLE.replace("RenderScale = \"-2\"", "RenderScale = \"0\"")).unwrap();
        let b = read_at(&f);
        let n = note(a.clone(), b);
        assert_eq!((n["read"].as_bool(), n["changed_while_recording"].as_bool()), (Some(true), Some(true)));
        assert_eq!(n["at_end"]["render"]["Render.13"]["RenderScale"], "0");
        let same = note(a.clone(), a);
        assert_eq!(same["changed_while_recording"], false);
        assert!(same["at_end"].is_null());
        assert_eq!(note(None, None)["read"], false);
        assert!(!n.to_string().contains(&dir.display().to_string()), "no folder of this PC is in the note");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
