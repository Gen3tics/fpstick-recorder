//! The window: one small window in three states (set up, recording, done).

use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, Vec2};
use recorder::config::{self, Choices, Length, Place};
use recorder::session::{self, Outcome, Phase, Session};
use recorder::{platform, presentmon, send};
use std::sync::{Arc, Mutex};

const BG: Color32 = Color32::from_rgb(0x0f, 0x12, 0x16);
const FIELD: Color32 = Color32::from_rgb(0x16, 0x1a, 0x20);
const LINE: Color32 = Color32::from_rgb(0x23, 0x27, 0x2e);
const EDGE: Color32 = Color32::from_rgb(0x3a, 0x40, 0x49);
const INK: Color32 = Color32::from_rgb(0xf1, 0xf3, 0xf5);
const MUT: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xad);
const AMBER: Color32 = Color32::from_rgb(0xf0, 0xa0, 0x4b);
const ON_AMBER: Color32 = Color32::from_rgb(0x14, 0x11, 0x0a);
const GREEN: Color32 = Color32::from_rgb(0x1f, 0xb8, 0x50);
const RED: Color32 = Color32::from_rgb(0xff, 0x6b, 0x6b);

const PRESENTMON_RELEASES: &str = "https://github.com/GameTechDev/PresentMon/releases";

enum Sent {
    No,
    Sending,
    Opened(String),
    Failed(String),
}

enum View {
    Setup,
    Running(Session),
    Done(Outcome, Arc<Mutex<Sent>>),
    Failed(String),
}

pub struct App {
    choices: Choices,
    view: View,
    /// Waiting for the next key or mouse button, to become the stop key.
    choosing_key: bool,
    show_command: bool,
    note: Option<String>,
    go: bool,
    /// Redraw without pause (`--show spin`): gives PresentMon something to record when testing without a game.
    spin: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, go: bool, show: Option<&str>) -> Self {
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = BG;
        v.extreme_bg_color = FIELD;
        v.override_text_color = Some(INK);
        v.selection.bg_fill = AMBER;
        v.selection.stroke = Stroke::new(1.0, ON_AMBER);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.bg_fill = FIELD;
        v.widgets.inactive.weak_bg_fill = FIELD;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, EDGE);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x1c, 0x21, 0x28);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, MUT);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(0x22, 0x28, 0x30);
        v.hyperlink_color = INK;
        cc.egui_ctx.set_visuals(v);
        cc.egui_ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = Vec2::new(8.0, 7.0);
            s.spacing.button_padding = Vec2::new(10.0, 6.0);
        });
        let mut app = App { choices: Choices::load(), view: View::Setup, choosing_key: false, show_command: false, note: None, go, spin: matches!(show, Some("spin") | Some("spin-net")) };
        if show == Some("spin-net") {
            // for testing the network record without a game: 64 times a second, a small packet to an address set
            // aside for documentation (192.0.2.1), which no machine has, and a larger one to this program itself,
            // so there is something arriving as well as leaving
            std::thread::spawn(|| {
                let (Ok(out), Ok(me)) = (std::net::UdpSocket::bind("0.0.0.0:0"), std::net::UdpSocket::bind("127.0.0.1:0")) else { return };
                let _ = me.set_nonblocking(true);
                let Ok(my_addr) = me.local_addr() else { return };
                let mut buf = [0u8; 2048];
                loop {
                    let _ = out.send_to(&[0u8; 75], "192.0.2.1:9");
                    let _ = me.send_to(&[0u8; 420], my_addr);
                    while me.recv_from(&mut buf).is_ok() {}
                    std::thread::sleep(std::time::Duration::from_micros(15_625));
                }
            });
        }
        if let Some(state) = show {
            app.show(state);
        }
        app
    }

    /// Puts the window into one of its states with made-up content, without recording anything (`--show STATE`).
    fn show(&mut self, state: &str) {
        self.choices = Choices { map: "King's Row".into(), mode: "Quick play".into(), hero: "Sojourn".into(), frame_limit: "320".into(), ..Choices::default() };
        let exe = "C:\\Program Files\\Intel\\PresentMon\\PresentMonConsoleApplication\\PresentMon-2.6.0-x64.exe";
        let folder = std::path::PathBuf::from("C:\\Users\\you\\Documents\\Recorder\\2026-11-11_164713");
        let command = presentmon::shown(exe, &presentmon::record_args("Overwatch.exe", &folder.join("frames.csv"), None));
        let still = |phase: Phase, seconds: f64| View::Running(Session::still(session::Status { net: true, pings: true, since: "18:26:17".into(), phase, seconds, command: command.clone(), outcome: None }));
        self.view = match state {
            "countdown" => still(Phase::Countdown, 3.2),
            "recording" => still(Phase::Recording, 0.0),
            "finishing" => still(Phase::Finishing, 557.0),
            "done" => View::Done(Outcome { game_settings: Some(serde_json::json!({"section": "Render.13", "values": {"TextureDetail": "1"}, "changed_while_recording": false})), packed: folder.join("frames.fpsf"), folder, recording_bytes: 38_300_000, packed_bytes: 6_310_000, frames: 167_865, recorded_s: 557.0, captured_at: "2026-11-11T16:47:13".into(), cpu: Some("AMD Ryzen 7 9850X3D 8-Core Processor".into()), gpu: Some("NVIDIA GeForce RTX 5090".into()), display: Some("2560 x 1440 at 360 Hz".into()), net: Some(recorder::net::NetSummary { datagrams: 106_512, received: 35_310, sent: 35_600, flows: 3, flow1_share: 0.666, probes: 8_340, hops: vec!["router", "isp", "edge"], pings_asked: true, events_lost: 0, ipv6: 0 }), net_note: None }, Arc::new(Mutex::new(Sent::No))),
            "failed" => View::Failed("Overwatch is not running. Start the game, then press Record again.".into()),
            "command" => {
                self.show_command = true;
                View::Setup
            }
            _ => View::Setup,
        };
    }

    fn record(&mut self, ctx: &egui::Context) {
        self.note = None;
        self.choices.map.clear();
        self.choices.save();
        let Some(exe) = platform::find_presentmon() else {
            self.note = Some("Intel's PresentMon was not found on this PC. Install it, then press Record again.".into());
            return;
        };
        if !platform::is_elevated() {
            // Recording needs administrator rights. Windows asks once; the window then comes back already counting down.
            if platform::relaunch_elevated("--go") {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.note = Some("Windows did not give permission, so nothing was recorded.".into());
            }
            return;
        }
        let ctx = ctx.clone();
        self.view = View::Running(session::start(self.choices.clone(), exe, platform::recordings_dir(), move || ctx.request_repaint()));
    }
}

fn label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text.to_uppercase()).size(11.0).strong().color(MUT));
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(MUT));
}

/// A row of choices of which one is picked, drawn as one bar.
fn segments<T: PartialEq + Copy>(ui: &mut egui::Ui, current: T, options: &[(T, &str)]) -> Option<T> {
    let mut picked = None;
    egui::Frame::new().fill(FIELD).corner_radius(CornerRadius::same(10)).inner_margin(4.0).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = (ui.available_width() - 4.0 * (options.len() as f32 - 1.0)) / options.len() as f32;
        ui.horizontal(|ui| {
            for (value, text) in options {
                let on = *value == current;
                let b = egui::Button::new(RichText::new(*text).size(13.0).color(if on { ON_AMBER } else { INK }).strong())
                    .fill(if on { AMBER } else { Color32::TRANSPARENT })
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(7))
                    .min_size(Vec2::new(w, 30.0));
                if ui.add(b).clicked() && !on {
                    picked = Some(*value);
                }
            }
        });
    });
    picked
}

/// An on/off switch with a round knob: green when on.
fn switch(ui: &mut egui::Ui, on: &mut bool, usable: bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), if usable { Sense::click() } else { Sense::hover() });
    if usable && response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
    let fill = if *on { GREEN } else { EDGE };
    ui.painter().rect_filled(rect, CornerRadius::same(11), if usable { fill } else { fill.gamma_multiply(0.45) });
    let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), how_on);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 9.0, if usable { Color32::WHITE } else { MUT });
    response
}

fn what_row(ui: &mut egui::Ui, on: &mut bool, usable: bool, name: &str, tag: &str, what: &str, help: &str) {
    ui.horizontal(|ui| {
        switch(ui, on, usable);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).size(14.0).strong().color(if usable || *on { INK } else { MUT }));
                if !tag.is_empty() {
                    ui.label(RichText::new(tag).size(11.0).color(MUT));
                }
            });
            ui.label(RichText::new(what).size(12.0).color(MUT));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(" ? ").size(12.0).color(MUT)).sense(Sense::hover())).on_hover_text(help);
        });
    });
}

/// An optional extra under a row of "What to record": a tick box, set in under the switch above it.
fn sub_check(ui: &mut egui::Ui, on: &mut bool, name: &str, what: &str, help: &str) {
    ui.horizontal(|ui| {
        ui.add_space(48.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.checkbox(on, RichText::new(name).size(13.0).color(INK));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(" ? ").size(12.0).color(MUT)).sense(Sense::hover())).on_hover_text(help);
                });
            });
            ui.label(RichText::new(what).size(12.0).color(MUT));
        });
    });
}

fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized(Vec2::new(ui.available_width(), 40.0), egui::Button::new(RichText::new(text).size(15.0).strong().color(ON_AMBER)).fill(AMBER).stroke(Stroke::NONE).corner_radius(CornerRadius::same(9)))
}

fn secondary(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    ui.add_sized(Vec2::new(width, 38.0), egui::Button::new(RichText::new(text).size(14.0).strong()).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, EDGE)).corner_radius(CornerRadius::same(9)))
}

fn clock(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn span(s: f64) -> String {
    let s = s.round() as u64;
    if s >= 60 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{} s", s)
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else {
        format!("{} KB", (bytes as f64 / 1e3).round().max(1.0))
    }
}

impl App {
    fn setup(&mut self, ui: &mut egui::Ui) {
        label(ui, "Where are you?");
        if let Some(p) = segments(ui, self.choices.place, &[(Place::InQueue, "In queue or menu"), (Place::InMatch, "Already in a match")]) {
            self.choices.pick_place(p);
        }
        note(ui, match (self.choices.place, self.choices.network) {
            (Place::InQueue, true) => "Records the whole match. Everything before the doors open is cut off for you.",
            (Place::InQueue, false) => "Records the whole match, menus included: without the network stats nothing can tell where play begins.",
            (Place::InMatch, _) => "Records from five seconds after you press Record, for the length you pick.",
        });

        label(ui, "What to record");
        let (mut always, mut later) = (true, false);
        what_row(ui, &mut always, false, "Frame rate performance", "always", "FPS, FPS stability, PC latency",
            "Intel's PresentMon records when each frame started and how long it took. Windows asks for administrator rights for this.");
        what_row(ui, &mut self.choices.network, true, "Game network stats", "", "Server updates and how many arrive late",
            "Windows' own record of the packets the game sends and receives: their times and sizes, never their contents. Addresses are used while recording and never written to a file.");
        if self.choices.network {
            sub_check(ui, &mut self.choices.pings, "Ping the route to the server",
                "Your router, your provider's first router and the last router before the server. Adds a little: whether a network delay may come from your path to the server.",
                "Five pings a second to each of the three. This is most of what the recorder itself costs while recording: roughly a thousandth of one processor core. Without it the game's own packets are still recorded.");
        }
        what_row(ui, &mut later, false, "Your inputs", "later", "Mouse and key timing", "Not recorded.");

        label(ui, "How long");
        if let Some(l) = segments(ui, self.choices.length, &[(Length::Minutes(1), "1 min"), (Length::Minutes(2), "2 min"), (Length::Minutes(5), "5 min"), (Length::WholeMatch, "Whole match")]) {
            self.choices.length = l;
        }

        label(ui, "Stop key");
        ui.horizontal(|ui| {
            let text = if self.choosing_key { "Press the key or mouse button…".to_string() } else { config::key_name(self.choices.stop_key) };
            let w = ui.available_width() - 86.0;
            egui::Frame::new().fill(FIELD).stroke(Stroke::new(1.0, if self.choosing_key { AMBER } else { LINE })).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                ui.set_width(w - 22.0);
                ui.label(RichText::new(text).size(13.0));
            });
            if secondary(ui, if self.choosing_key { "Cancel" } else { "Change" }, 74.0).clicked() {
                self.choosing_key = !self.choosing_key;
            }
        });
        if self.choosing_key {
            if let Some(vk) = platform::pressed_key() {
                self.choices.stop_key = vk;
                self.choosing_key = false;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }
        ui.horizontal(|ui| {
            switch(ui, &mut self.choices.hold_to_stop, true);
            ui.label(RichText::new("Hold for 1 second, so a press in a fight does not end it").size(13.0));
        });
        note(ui, "It also stops after 30 minutes, or when the game closes.");

        note(ui, "Also read from this PC when a recording ends: the CPU and graphics card model and the screen's mode.");

        ui.add_space(8.0);
        let record = primary(ui, "    Record");
        ui.painter().circle_filled(egui::pos2(record.rect.center().x - 30.0, record.rect.center().y), 5.5, Color32::from_rgb(0xb3, 0x26, 0x1e));
        if record.clicked() {
            let ctx = ui.ctx().clone();
            self.record(&ctx);
        }
        ui.vertical_centered(|ui| {
            note(ui, "Starts 5 seconds after you press it, with a sound. Windows asks for permission once.");
            if let Some(n) = &self.note {
                ui.label(RichText::new(n).size(13.0).color(RED));
                if n.contains("PresentMon was not found") {
                    ui.hyperlink_to("Intel's PresentMon releases", PRESENTMON_RELEASES);
                }
            }
            if ui.link(RichText::new(if self.show_command { "Hide what will run" } else { "Don't trust us? See exactly what will run" }).size(13.0)).clicked() {
                self.show_command = !self.show_command;
            }
        });
        if self.show_command {
            let exe = platform::find_presentmon().map(|p| p.display().to_string()).unwrap_or_else(|| "PresentMon.exe (not found on this PC)".into());
            let out = platform::recordings_dir().join("<date and time>").join("frames.csv");
            command_box(ui, &presentmon::shown(&exe, &presentmon::record_args(&self.choices.process, &out, self.choices.seconds())));
            note(ui, "That is Intel's PresentMon, unchanged. This program starts it, waits for your stop key, then reads the file it wrote. Nothing is sent anywhere until you press Send.");
        }
    }

    fn running(&mut self, ui: &mut egui::Ui) -> Option<View> {
        let View::Running(s) = &self.view else { return None };
        let st = s.get();
        match &st.phase {
            Phase::Done => return st.outcome.clone().map(|o| View::Done(o, Arc::new(Mutex::new(Sent::No)))),
            Phase::Failed(why) => return Some(View::Failed(why.clone())),
            Phase::Cancelled => {
                self.note = None;
                return Some(View::Setup);
            }
            _ => {}
        }
        let key = config::key_name(self.choices.stop_key);
        let key = key.split(" · ").next().unwrap_or(&key).to_string();
        let how_to_stop = if self.choices.hold_to_stop { format!("To stop, hold {} for a second.", key) } else { format!("To stop, press {}.", key) };
        if st.phase == Phase::Recording {
            // While the game is played this window is drawn once and then left alone: no timer, no progress, no
            // redraw until something is clicked. Everything on it is dimmed to say so.
            ui.add_space(26.0);
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 110.0).max(0.0) / 2.0);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 6.0, RED.gamma_multiply(0.6));
                    ui.label(RichText::new("Recording").size(15.0).strong().color(MUT));
                });
                ui.label(RichText::new(format!("since {}", st.since)).size(30.0).strong().color(MUT));
                note(ui, if st.net { "Frame rate performance · Game network stats" } else { "Frame rate performance" });
                ui.add_space(16.0);
                egui::Frame::new().fill(FIELD).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(8)).inner_margin(12.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("This window is paused while you play").size(14.0).strong());
                    note(ui, "Nothing here updates until the recording ends: no timer, no counters, no redrawing. The recorder only checks your stop key and lets PresentMon and Windows do the recording.");
                });
                ui.add_space(10.0);
                ui.label(RichText::new(how_to_stop.as_str()).size(14.0).color(MUT));
                note(ui, match self.choices.seconds() {
                    Some(n) => format!("It stops by itself after {}, or when the game closes. You will hear a sound.", span(n as f64)),
                    None => "You will hear a sound when it stops. It also stops after 30 minutes, or when the game closes.".to_string(),
                }
                .as_str());
            });
        } else {
            ui.add_space(26.0);
            ui.vertical_centered(|ui| {
                let (dot, title, big) = match st.phase {
                    Phase::Countdown => (AMBER, "Get back into the game", format!("{}", st.seconds.ceil() as u64)),
                    Phase::Starting => (AMBER, "Starting…", String::new()),
                    _ => (MUT, "Finishing…", clock(st.seconds)),
                };
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 190.0).max(0.0) / 2.0);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 6.0, dot);
                    ui.label(RichText::new(title).size(15.0).strong());
                });
                ui.label(RichText::new(big).size(58.0).strong());
                ui.add_space(8.0);
                ui.label(RichText::new(how_to_stop.as_str()).size(14.0));
                note(ui, "You will hear a sound when the recording starts and when it stops.");
            });
        }
        ui.add_space(12.0);
        ui.columns(2, |c| {
            let w = c[0].available_width();
            if secondary(&mut c[0], "Stop now", w).clicked() {
                s.stop();
            }
            if secondary(&mut c[1], "Cancel and delete", w).clicked() {
                s.cancel();
            }
        });
        if !st.command.is_empty() {
            label(ui, "Exactly what is running");
            command_box(ui, &st.command);
            note(ui, "Frames: Intel's PresentMon, unchanged. When each frame started and how long it took.");
            if st.net {
                note(ui, "Network: Windows' own record of the packets Overwatch.exe sends and receives. Their times and sizes, never their contents.");
                if st.pings {
                    note(ui, "Route: pings to three routers on the way to the server.");
                }
            }
        }
        None
    }

    fn done(&mut self, ui: &mut egui::Ui) -> Option<View> {
        let View::Done(o, sent) = &self.view else { return None };
        let mut next = None;
        ui.add_space(8.0);
        ui.label(RichText::new(format!("Recorded {}", span(o.recorded_s))).size(22.0).strong());
        note(ui, &format!("{} frames", o.frames));

        // what was played is asked now, when it is known, not before the match
        label(ui, "This match (optional)");
        // two to a row: four to a row cut a map's name short ("Practice Rang")
        for pair in [[("Map", &mut self.choices.map), ("Mode", &mut self.choices.mode)], [("Hero", &mut self.choices.hero), ("Frame limit", &mut self.choices.frame_limit)]] {
            ui.columns(2, |c| {
                for (i, (name, value)) in pair.into_iter().enumerate() {
                    c[i].label(RichText::new(name).size(11.0).color(MUT));
                    c[i].add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY).char_limit(40));
                }
            });
        }

        let to = send::target(&self.choices.send_to);
        label(ui, "Will be sent if you press Send");
        // a filled green dot for what goes, an empty grey ring for what stays
        let item = |ui: &mut egui::Ui, goes: bool, text: String| {
            ui.horizontal_wrapped(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(12.0, 16.0), Sense::hover());
                if goes {
                    ui.painter().circle_filled(r.center(), 4.5, GREEN);
                } else {
                    ui.painter().circle_stroke(r.center(), 4.0, Stroke::new(1.5, MUT));
                }
                ui.label(RichText::new(text).size(13.0));
            });
        };
        item(ui, true, format!("Frame timing, packed: {}", size(o.packed_bytes)));
        let parts: Vec<String> = [o.cpu.clone(), o.gpu.clone(), o.display.clone()].into_iter().flatten().collect();
        if !parts.is_empty() {
            item(ui, true, format!("Read from this PC: {}", parts.join(" · ")));
        }
        let net_line = o.net.as_ref().map(|n| {
            let routers = match n.hops.len() {
                0 if !n.pings_asked => "the route was not pinged, as you chose".to_string(),
                0 => "no router answered, so the route was not pinged".to_string(),
                k => format!("{} pings to {} router{} on the way", n.probes, k, if k == 1 { "" } else { "s" }),
            };
            format!("Times and sizes of {} of the game's packets; {}", n.datagrams, routers)
        });
        if let Some(line) = &net_line {
            item(ui, true, line.clone());
        }
        if o.game_settings.is_some() {
            item(ui, true, "Your Overwatch graphics settings, to make comparisons fair".into());
        }
        item(ui, true, "What you typed in: map, mode, hero, frame limit".into());
        if let Some(why) = &o.net_note {
            ui.label(RichText::new(format!("No network stats: {}", why)).size(13.0).color(AMBER));
        }

        label(ui, "Stays on this PC");
        item(ui, false, format!("The recording itself ({}), kept in {} until you delete it", size(o.recording_bytes), o.folder.display()));
        if o.net.is_some() {
            item(ui, false, "What was in the packets, and every address: the server's and the routers'".into());
        }
        item(ui, false, "File names, folders, your Windows user name, the PC's name".into());

        ui.add_space(10.0);
        let state = sent.lock().unwrap();
        match &*state {
            Sent::No | Sent::Failed(_) => {
                if let Sent::Failed(why) = &*state {
                    ui.label(RichText::new(why).size(13.0).color(RED));
                }
                drop(state);
                if primary(ui, "Send and open my run page").clicked() {
                    self.choices.save();
                    session::save_scene(&o.folder, &self.choices);
                    *sent.lock().unwrap() = Sent::Sending;
                    let (sent, ctx, o, c) = (sent.clone(), ui.ctx().clone(), o.clone(), self.choices.clone());
                    std::thread::spawn(move || {
                        let label = [c.map.as_str(), c.mode.as_str(), c.hero.as_str()].iter().filter(|s| !s.trim().is_empty()).cloned().collect::<Vec<_>>().join(", ");
                        let data = std::fs::read(&o.packed).map_err(|e| format!("The packed file could not be read: {}", e));
                        let result = match (send::target(&c.send_to), data) {
                            (Err(why), _) | (_, Err(why)) => Err(why),
                            (Ok(None), Ok(data)) => send::send_local(&data, &o.packed.display().to_string(), &label, &c.map, &c.mode, &c.frame_limit, o.net.is_some()).map(|url| (url.clone(), url)),
                            (Ok(Some(host)), Ok(data)) => {
                                let (resolution, refresh_hz) = send::screen(o.display.as_deref());
                                let info = serde_json::json!({
                                    "declared": {"map": c.map.trim(), "mode": c.mode.trim(), "hero": c.hero.trim(), "limit_fps": c.frame_limit.trim()},
                                    "read": {"cpu": o.cpu, "gpu": o.gpu, "resolution": resolution, "refresh_hz": refresh_hz},
                                    "captured_at": o.captured_at,
                                    "game_settings": o.game_settings,
                                });
                                let net = if o.net.is_some() { send::network_block(&o.packed, &data) } else { None };
                                send::send_server(&host, &data, &info, net.as_deref()).map(|kept| {
                                    session::save_sent(&o.folder, &kept.page, &kept.delete_key);
                                    // the delete key rides after the "#": the browser keeps it, no server ever sees it
                                    (format!("{}#k={}", kept.page, kept.delete_key), kept.page)
                                })
                            }
                        };
                        *sent.lock().unwrap() = match result {
                            Ok((open, shown)) => {
                                platform::open_url(&open);
                                Sent::Opened(shown)
                            }
                            Err(why) => Sent::Failed(why),
                        };
                        ctx.request_repaint();
                    });
                }
                if secondary(ui, "Keep it on this PC", ui.available_width()).clicked() {
                    self.choices.save();
                    session::save_scene(&o.folder, &self.choices);
                    next = Some(View::Setup);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Send to").size(11.0).color(MUT));
                    ui.add(egui::TextEdit::singleline(&mut self.choices.send_to).hint_text("this PC (127.0.0.1:8765)").desired_width(f32::INFINITY).char_limit(120));
                });
                match &to {
                    Ok(None) => ui.vertical_centered(|ui| note(ui, "Nothing has left this PC yet. Send goes to the run-page program on this same PC.")),
                    Ok(Some(host)) => ui.vertical_centered(|ui| note(ui, &format!("Nothing has left this PC yet. Send goes to the run-page server at {}, over plain http.", host))),
                    Err(why) => ui.vertical_centered(|ui| {
                        ui.label(RichText::new(why).size(12.0).color(AMBER));
                    }),
                };
            }
            Sent::Sending => {
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    note(ui, "Sending and working out the run…");
                });
            }
            Sent::Opened(url) => {
                ui.label(RichText::new("Sent. Your run page is open in the browser.").size(14.0).color(GREEN));
                ui.hyperlink(url);
                drop(state);
                if secondary(ui, "Record another", ui.available_width()).clicked() {
                    next = Some(View::Setup);
                }
            }
        }
        next
    }
}

fn command_box(ui: &mut egui::Ui, command: &str) {
    egui::Frame::new().fill(FIELD).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(8)).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(command).monospace().size(11.0)).wrap().selectable(true));
    });
    if ui.small_button("Copy").clicked() {
        ui.ctx().copy_text(command.to_string());
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.spin {
            ui.ctx().request_repaint();
        }
        if self.go {
            self.go = false;
            let ctx = ui.ctx().clone();
            self.record(&ctx);
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(16.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("FPStick Recorder").size(15.0).strong());
                ui.label(RichText::new(match &self.view {
                    View::Setup => "· ready",
                    View::Running(_) => "· recording",
                    View::Done(..) => "· finished",
                    View::Failed(_) => "· stopped",
                }).size(13.0).color(MUT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::new().stroke(Stroke::new(1.0, EDGE)).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(6, 1)).show(ui, |ui| ui.label(RichText::new("Beta").size(11.0).color(MUT)));
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let next = match &self.view {
                    View::Setup => {
                        self.setup(ui);
                        None
                    }
                    View::Running(_) => self.running(ui),
                    View::Done(..) => self.done(ui),
                    View::Failed(why) => {
                        ui.add_space(20.0);
                        ui.label(RichText::new("Nothing was recorded").size(20.0).strong());
                        ui.label(RichText::new(why.as_str()).size(14.0));
                        ui.add_space(10.0);
                        if primary(ui, "Back").clicked() { Some(View::Setup) } else { None }
                    }
                };
                if let Some(v) = next {
                    self.view = v;
                }
            });
        });
    }
}
