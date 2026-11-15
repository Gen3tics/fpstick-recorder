// A release build is a window with no console behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;

use recorder::{platform, session};
use std::path::Path;

/// `recorder`                      the window
/// `recorder --go`                 the window, recording at once (used after Windows gave permission)
/// `recorder pack IN.csv OUT.fpsf` read a recording and pack its frame data, nothing else
/// `recorder info`                 what this PC looks like to the recorder
/// `recorder record --process NAME.exe --out FOLDER [--seconds N | --stop-after N] [--network [--no-pings]] [--hold]`
///                                 one recording without the window, for testing; --stop-after ends it as the stop key would
/// `recorder nettest`              checks the network record on this PC with packets this program sends to itself
/// `recorder send FILE.fpsf`       hand a packed recording to the run-page program on this PC and print the run page's address
/// `recorder send FILE.fpsf --to http://HOST:PORT`   send it to a run-page server instead
/// The picture and the screen of a program's window, as one line.
fn screen_line(process: &str) -> String {
    match platform::game_screen(process) {
        Some(s) => format!("{}, on a screen of {} x {}{}{}", s.shown(), s.screen.0, s.screen.1, if s.main_screen { " (the main one)" } else { " (not the main one)" }, if s.in_front { ", in front" } else { ", not in front" }),
        None => "not found (not running, or minimised)".into(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("pack") if args.len() == 3 => match session::pack_file(Path::new(&args[1]), Path::new(&args[2])) {
            Ok((frames, seconds, bytes)) => println!("{} frames, {:.3} s, {} bytes packed", frames, seconds, bytes),
            Err(why) => {
                eprintln!("{}", why);
                std::process::exit(1);
            }
        },
        Some("record") => std::process::exit(record(&args)),
        Some("nettest") => std::process::exit(nettest()),
        Some("send") if args.len() == 4 && args[2] == "--to" => {
            // to a run-page server, with what this PC says about itself and what the recording's own notes hold
            // the game's graphics settings, the picture's size and refresh rate, and the time of the recording come
            // from the run.json beside the file, if it is there: they are what was read while the game was running
            let folder = std::path::Path::new(&args[1]).parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let game_settings = recorder::settings::for_upload_from(&folder);
            let note = std::fs::read(folder.join("run.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            let captured_at = note.as_ref().and_then(|n| n.get("captured_at").cloned());
            let (resolution, refresh_hz) = recorder::send::screen(note.as_ref().and_then(|n| n["read_from_this_pc"]["display"].as_str()));
            if game_settings.is_some() {
                println!("with the game's graphics settings from run.json");
            }
            // what was typed in on the Done screen when the recording was made is in run.json too
            let typed = |k: &str| note.as_ref().and_then(|n| n["scene"][k].as_str()).map(|v| v.trim().to_string()).unwrap_or_default();
            let declared = serde_json::json!({"map": typed("map"), "mode": typed("mode"), "hero": typed("hero"), "limit_fps": typed("frame_limit")});
            let info = serde_json::json!({"declared": declared, "read": {"cpu": platform::cpu_name(), "gpu": platform::gpu_name(), "memory": platform::memory(), "resolution": resolution, "refresh_hz": refresh_hz}, "captured_at": captured_at, "game_settings": game_settings});
            let sent = recorder::send::target(&args[3]).and_then(|t| t.ok_or_else(|| "Give an address after --to.".to_string())).and_then(|host| {
                std::fs::read(&args[1]).map_err(|e| e.to_string()).and_then(|d| {
                    // the network timing recorded beside it goes along, if there is any
                    let net = recorder::send::network_block(std::path::Path::new(&args[1]), &d);
                    if let Some(n) = &net {
                        println!("with the network timing, {} KB packed", n.len() / 1024);
                    }
                    recorder::send::send_server(&host, &d, &info, net.as_deref())
                })
            });
            match sent {
                Ok(kept) => println!("{}\ndelete key: {}", kept.page, kept.delete_key),
                Err(why) => {
                    eprintln!("{}", why);
                    std::process::exit(1);
                }
            }
        }
        Some("send") if args.len() == 2 => match std::fs::read(&args[1]).map_err(|e| e.to_string()).and_then(|d| recorder::send::send_local(&d, &args[1], "sent from the command line", "", "", "", std::path::Path::new(&args[1]).with_file_name("udp.csv").exists())) {
            Ok(url) => println!("{}", url),
            Err(why) => {
                eprintln!("{}", why);
                std::process::exit(1);
            }
        },
        // what Windows says about the largest window of any running program: for trying the reading without the game
        Some("screen") if args.len() == 2 => println!("{}: {}", args[1], screen_line(&args[1])),
        Some("info") => {
            println!("PresentMon: {}", platform::find_presentmon().map(|p| p.display().to_string()).unwrap_or_else(|| "not found".into()));
            println!("administrator rights: {}", platform::is_elevated());
            println!("Overwatch running: {}", platform::process_running("Overwatch.exe"));
            println!("CPU: {}", platform::cpu_name().unwrap_or_else(|| "not read".into()));
            println!("graphics card: {}", platform::gpu_name().unwrap_or_else(|| "not read".into()));
            println!("memory: {}", platform::memory().unwrap_or_else(|| "not read".into()));
            println!("main screen: {}", platform::display().unwrap_or_else(|| "not read".into()));
            println!("the game's window: {}", screen_line("Overwatch.exe"));
            println!("recordings go to: {}", platform::recordings_dir().display());
            println!("the game's settings file: {}", match recorder::settings::read() {
                Some(v) => format!("read, {} graphics settings, last written {} s ago", v["render"].as_object().map(|o| o.values().filter_map(|s| s.as_object()).map(|s| s.len()).sum::<usize>()).unwrap_or(0), v["file_last_written_s_ago"]),
                None => "not found".into(),
            });
            println!("now: {}", platform::now().1);
        }
        _ => {
            let go = args.iter().any(|a| a == "--go");
            // `--show STATE` draws one of the window's states with made-up content: setup, command, countdown, recording, finishing, done, failed
            let show = args.iter().position(|a| a == "--show").and_then(|i| args.get(i + 1)).cloned();
            let options = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default().with_title("FPStick Recorder").with_inner_size([430.0, 820.0]).with_min_inner_size([380.0, 520.0]),
                ..Default::default()
            };
            if let Err(e) = eframe::run_native("FPStick Recorder", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, go, show.as_deref()))))) {
                eprintln!("The window could not be opened: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn record(args: &[String]) -> i32 {
    use recorder::config::{Choices, Length};
    use recorder::session::Phase;
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (Some(process), Some(out)) = (value("--process"), value("--out")) else {
        eprintln!("record needs --process NAME.exe and --out FOLDER");
        return 2;
    };
    let Some(exe) = platform::find_presentmon() else {
        eprintln!("PresentMon was not found.");
        return 1;
    };
    let seconds = value("--seconds").and_then(|s| s.parse::<u64>().ok());
    let stop_after = value("--stop-after").and_then(|s| s.parse::<f64>().ok());
    // a fixed length is given in minutes in the window; here seconds are allowed, so the choice is made by hand
    let choices = Choices { process, length: Length::WholeMatch, hold_to_stop: args.iter().any(|a| a == "--hold"), network: args.iter().any(|a| a == "--network"), pings: !args.iter().any(|a| a == "--no-pings"), ..Choices::default() };
    let s = match seconds {
        Some(n) => session::start_for(choices, exe, out.into(), Some(n), || {}),
        None => session::start(choices, exe, out.into(), || {}),
    };
    let (mut last, mut recording_since): (String, Option<std::time::Instant>) = (String::new(), None);
    loop {
        let st = s.get();
        let line = format!("{:?}", st.phase);
        if line != last {
            println!("{}  ({:.1} s)", line, st.seconds);
            last = line;
        }
        match st.phase {
            Phase::Done => {
                let o = st.outcome.unwrap();
                println!("{} frames, {:.2} s recorded, {} bytes -> {} packed, in {}", o.frames, o.recorded_s, o.recording_bytes, o.packed_bytes, o.folder.display());
                println!("{}", st.command);
                match (&o.net, &o.net_note) {
                    (Some(n), _) => println!("network: {} packets ({} in, {} out on the busiest of {} remote ends, share {:.3}), {} pings to {:?}, {} events lost, {} over IPv6", n.datagrams, n.received, n.sent, n.flows, n.flow1_share, n.probes, n.hops, n.events_lost, n.ipv6),
                    (None, Some(why)) => println!("network: none. {}", why),
                    _ => {}
                }
                return 0;
            }
            Phase::Failed(_) | Phase::Cancelled => return 1,
            Phase::Recording => {
                let since = *recording_since.get_or_insert_with(std::time::Instant::now);
                if stop_after.map(|n| since.elapsed().as_secs_f64() >= n).unwrap_or(false) {
                    s.stop();
                }
            }
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Listens to Windows' record of UDP packets for this program's own process, sends five packets of known
/// sizes to this PC itself, and checks that each one comes back with the right size, direction, address
/// and a time stamp that lies between the moments before and after it was sent.
fn nettest() -> i32 {
    use recorder::net;
    let cap = match net::NetCapture::start(std::process::id(), true, false) {
        Ok(c) => c,
        Err(why) => {
            eprintln!("{}", why);
            return 1;
        }
    };
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");
    let mut sent = Vec::new();
    for i in 1..=5usize {
        let before = net::qpc();
        let _ = sock.send_to(&vec![0u8; 1300 + i], "127.0.0.1:9");
        sent.push((1300 + i as u32, before, net::qpc()));
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    std::thread::sleep(std::time::Duration::from_millis(2500));
    let (seen, raw) = cap.raw();
    println!("Windows hands over only the UDP events: {}", cap.filtered());
    let data = cap.stop();
    println!("events seen from all programs: {}; lost: {}; kept for this program: {}; clock {} per second", seen, data.events_lost, data.datagrams.len(), data.qpc_per_s);
    let mut ok = 0;
    for (size, before, after) in &sent {
        match data.datagrams.iter().find(|d| d.bytes == *size) {
            Some(d) => {
                let right = d.out && d.remote == ([127, 0, 0, 1], 9) && d.qpc >= *before && d.qpc <= *after;
                ok += right as i32;
                println!("  {} bytes: found, out={}, to this PC port 9: {}, stamp {} us after the send began, inside the send: {}", size, d.out, d.remote == ([127, 0, 0, 1], 9), (d.qpc - before) * 1_000_000 / data.qpc_per_s, d.qpc >= *before && d.qpc <= *after);
            }
            None => println!("  {} bytes: NOT FOUND", size),
        }
    }
    if ok < 5 {
        println!("first raw events (id pid stamp bytes):");
        for line in raw.iter().filter(|l| l.contains(&format!(" {} ", std::process::id()))).take(8) {
            println!("  {}", line);
        }
    }
    let (me, first) = net::ping_check();
    println!("ping to this PC: {}; first router on the way out answered: {}", me.map(|ms| format!("{:.3} ms", ms)).unwrap_or_else(|| "no answer".into()), first);
    println!("{} of 5 packets found exactly", ok);
    if ok == 5 { 0 } else { 1 }
}
