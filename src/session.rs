//! One recording, from the countdown to the packed file. Runs on its own thread; the window only watches.

use crate::config::{Choices, CAP_S, COUNTDOWN_S, HOLD_MS, LOOK_HOLD_MS, LOOK_PRESS_MS};
use crate::net::{self, NetSummary};
use crate::{frames, pack, platform, presentmon};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Countdown,
    Starting,
    Recording,
    Finishing,
    Done,
    Failed(String),
    Cancelled,
}

/// What a finished recording left behind.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub folder: PathBuf,
    pub packed: PathBuf,
    pub recording_bytes: u64,
    pub packed_bytes: u64,
    pub frames: usize,
    pub recorded_s: f64,
    pub captured_at: String,
    pub cpu: Option<String>,
    pub gpu: Option<String>,
    pub display: Option<String>,
    /// What the network record holds, if one was asked for and the game was on a server.
    pub net: Option<NetSummary>,
    /// Why there is no network record although one was asked for.
    pub net_note: Option<String>,
    /// The game's graphics settings as its settings file held them, in the shape that is sent; None if the file was not found.
    pub game_settings: Option<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct Status {
    /// Whether the network is being recorded too.
    pub net: bool,
    /// Whether the route is being pinged as well.
    pub pings: bool,
    /// The time of day the recording began, such as "18:26:17".
    pub since: String,
    pub phase: Phase,
    /// Seconds left in the countdown. Not updated while recording: nothing is, so the window has nothing to redraw.
    pub seconds: f64,
    pub command: String,
    pub outcome: Option<Outcome>,
}

pub struct Session {
    pub status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

impl Session {
    pub fn get(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    /// A session that does nothing, standing still in one state: for showing the window's states without recording.
    pub fn still(status: Status) -> Session {
        Session { status: Arc::new(Mutex::new(status)), stop: Arc::new(AtomicBool::new(false)), cancel: Arc::new(AtomicBool::new(false)) }
    }
    /// End the recording now and keep it.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    /// End the recording now and delete it.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// Reads a recording and writes its packed frame data beside it. Returns (frames, seconds recorded, packed bytes).
pub fn pack_file(recording: &Path, packed: &Path) -> Result<(usize, f64, u64), String> {
    let bytes = std::fs::read(recording).map_err(|e| format!("The recording could not be read: {}", e))?;
    let parsed = frames::parse_capture(&bytes, "frames.csv").map_err(|e| e.0)?;
    let data = pack::pack(&parsed).map_err(|e| format!("The frame data could not be packed: {}", e))?;
    std::fs::write(packed, &data).map_err(|e| format!("The packed file could not be written: {}", e))?;
    Ok((parsed.meta.frames, parsed.meta.duration_s, data.len() as u64))
}

/// Starts a recording. `changed` is called whenever the status moves, so the window can redraw.
pub fn start(choices: Choices, exe: PathBuf, root: PathBuf, changed: impl Fn() + Send + 'static) -> Session {
    let seconds = choices.seconds();
    start_for(choices, exe, root, seconds, changed)
}

/// As `start`, with the fixed length given in seconds (None: until told to stop).
pub fn start_for(choices: Choices, exe: PathBuf, root: PathBuf, seconds: Option<u64>, changed: impl Fn() + Send + 'static) -> Session {
    let status = Arc::new(Mutex::new(Status { net: false, pings: false, since: String::new(), phase: Phase::Countdown, seconds: COUNTDOWN_S as f64, command: String::new(), outcome: None }));
    let (stop, cancel) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (st, sp, cn) = (status.clone(), stop.clone(), cancel.clone());
    std::thread::spawn(move || {
        let set = |f: &dyn Fn(&mut Status)| {
            f(&mut st.lock().unwrap());
            changed();
        };
        match run(&choices, seconds, &exe, &root, &sp, &cn, &set) {
            Ok(Some(o)) => set(&|s| {
                s.phase = Phase::Done;
                s.outcome = Some(o.clone());
            }),
            Ok(None) => set(&|s| s.phase = Phase::Cancelled),
            Err(why) => set(&|s| s.phase = Phase::Failed(why.clone())),
        }
    });
    Session { status, stop, cancel }
}

fn run(c: &Choices, seconds: Option<u64>, exe: &Path, root: &Path, stop: &AtomicBool, cancel: &AtomicBool, set: &dyn Fn(&dyn Fn(&mut Status))) -> Result<Option<Outcome>, String> {
    if !platform::process_running(&c.process) {
        return Err(format!("{} is not running. Start the game, then press Record again.", c.process.trim_end_matches(".exe")));
    }

    // 1. The countdown: time to get back into the game.
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(COUNTDOWN_S) {
        if cancel.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let left = COUNTDOWN_S as f64 - t0.elapsed().as_secs_f64();
        set(&|s| s.seconds = left);
        std::thread::sleep(Duration::from_millis(100));
    }

    // 2. PresentMon starts. The recording has begun once its file appears.
    let (stamp, captured_at) = platform::now();
    let folder = root.join(&stamp);
    std::fs::create_dir_all(&folder).map_err(|e| format!("The folder for the recording could not be made: {}", e))?;
    let recording = folder.join("frames.csv");
    let args = presentmon::record_args(&c.process, &recording, seconds);
    let shown = presentmon::shown(&exe.display().to_string(), &args);
    set(&|s| {
        s.phase = Phase::Starting;
        s.seconds = 0.0;
        s.command = shown.clone();
    });
    // the game's settings as its file holds them now; read again when the recording has ended
    let settings_at_start = crate::settings::read();
    // The network record starts first and stops last, so it covers the whole frame recording.
    // If it cannot start, the frames are still recorded and the reason is kept.
    let (mut capture, mut net_note) = (None, None);
    if c.network {
        match net::pid_of(&c.process).ok_or_else(|| "The game's process was not found for the network record.".to_string()).and_then(|pid| net::NetCapture::start(pid, false, c.pings)) {
            Ok(n) => capture = Some(n),
            Err(why) => net_note = Some(why),
        }
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    platform::hidden(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("PresentMon could not be started: {}", e))?;
    // PresentMon records from about 0.2 s after it is started, but its file only appears some two seconds
    // later (measured on the first PC: asked for 6 s, it ended 6.20 s after being started; the file showed
    // after 2.16 s). So the start is taken from the launch, and the file is checked for in the loop below.
    let asked = Instant::now();
    std::thread::sleep(Duration::from_millis(250));
    if let Ok(Some(code)) = child.try_wait() {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(format!("PresentMon stopped before recording anything ({}). It needs administrator rights.", code));
    }
    let started = asked + Duration::from_millis(200);
    std::thread::spawn(platform::sound_start);
    let (with_net, since) = (capture.is_some(), captured_at[11..].to_string());
    set(&|s| {
        s.phase = Phase::Recording;
        s.net = with_net;
        s.pings = with_net && c.pings;
        s.since = since.clone();
    });

    // 3. Recording, until the length is reached, the stop key, the cap, or the game closing.
    // While the game is being played this thread is asleep in Windows, waiting for PresentMon to end
    // (which is how a timed recording and a closed game show). It wakes 10 times a second to ask whether
    // the stop key is down (40 if a single press stops it) and to look at two flags, and it runs below
    // the game's priority. It tells the window nothing, so the window redraws nothing.
    platform::yield_to_the_game();
    let hold = if c.hold_to_stop { Duration::from_millis(HOLD_MS) } else { Duration::ZERO };
    let look = if c.hold_to_stop { LOOK_HOLD_MS } else { LOOK_PRESS_MS };
    let (mut released_once, mut down_since): (bool, Option<Instant>) = (false, None);
    let mut looks: u64 = 0;
    loop {
        if platform::wait_exit(&mut child, look) {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            end(exe, &mut child);
            let _ = std::fs::remove_dir_all(&folder);
            return Ok(None);
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // The stop key counts only after it has been seen up once, so a key held at the start does not end it.
        let down = platform::key_down(c.stop_key);
        released_once |= !down;
        down_since = if down && released_once { down_since.or(Some(Instant::now())) } else { None };
        if down_since.map(|t| t.elapsed() >= hold).unwrap_or(false) {
            break;
        }
        // about once a second: the cap, and once, whether any frames have arrived at all
        looks += 1;
        if looks % (1000 / look as u64) == 0 {
            let elapsed = started.elapsed();
            if elapsed >= Duration::from_secs(CAP_S) {
                break;
            }
            if elapsed > Duration::from_secs(40) && elapsed < Duration::from_secs(43) && !std::fs::metadata(&recording).map(|m| m.len() > 0).unwrap_or(false) {
                end(exe, &mut child);
                let _ = std::fs::remove_dir_all(&folder);
                return Err("No frames arrived within 40 seconds. Is the game on screen?".into());
            }
        }
    }
    let recorded = started.elapsed().as_secs_f64();
    set(&|s| {
        s.phase = Phase::Finishing;
        s.seconds = recorded;
    });
    end(exe, &mut child);
    std::thread::spawn(platform::sound_stop);

    // 4. Read the recording and pack it. Nothing is sent here.
    let packed = folder.join("frames.fpsf");
    if !recording.exists() {
        let _ = std::fs::remove_dir_all(&folder);
        return Err("PresentMon wrote no file: it saw no frames from the game. Is the game on screen?".into());
    }
    let (frames, recorded_s, packed_bytes) = pack_file(&recording, &packed)?;
    // The network record: written only if the game was talking to a server at all.
    let net = match capture.take().map(|n| n.stop()) {
        Some(data) if data.datagrams.len() >= 100 => match data.write(&folder) {
            Ok(summary) => Some(summary),
            Err(e) => {
                net_note = Some(format!("The network files could not be written: {}", e));
                None
            }
        },
        Some(_) => {
            net_note = Some("The game exchanged hardly any packets during the recording: it was not on a game server.".into());
            None
        }
        None => None,
    };
    let settings_note = crate::settings::note(settings_at_start, crate::settings::read());
    let out = Outcome {
        game_settings: crate::settings::for_upload(&settings_note),
        net,
        net_note,
        recording_bytes: std::fs::metadata(&recording).map(|m| m.len()).unwrap_or(0),
        folder: folder.clone(),
        packed,
        packed_bytes,
        frames,
        recorded_s,
        captured_at,
        cpu: platform::cpu_name(),
        gpu: platform::gpu_name(),
        display: platform::display(),
    };
    let note = serde_json::json!({
        "recorder": env!("CARGO_PKG_VERSION"), "captured_at": out.captured_at, "command": shown,
        "scene": {"map": c.map, "mode": c.mode, "hero": c.hero, "frame_limit": c.frame_limit},
        "read_from_this_pc": {"cpu": out.cpu, "gpu": out.gpu, "display": out.display},
        "frames": frames, "recorded_s": recorded_s,
        "network": out.net.as_ref().map(|n| serde_json::json!({"packets": n.datagrams, "remote_ends": n.flows, "route_pings_asked_for": n.pings_asked, "pings": n.probes, "routers_pinged": n.hops, "events_lost": n.events_lost})),
        "network_note": out.net_note,
        "game_settings": settings_note,
    });
    let _ = std::fs::write(folder.join("run.json"), serde_json::to_vec_pretty(&note).unwrap_or_default());
    Ok(Some(out))
}

/// Notes in the recording's run.json where it was sent, with the key that deletes it there.
pub fn save_sent(folder: &Path, page: &str, delete_key: &str) {
    let file = folder.join("run.json");
    let Ok(mut note) = std::fs::read(&file).map_err(|_| ()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|_| ())) else { return };
    note["sent"] = serde_json::json!({"run_page": page, "delete_key": delete_key});
    let _ = std::fs::write(file, serde_json::to_vec_pretty(&note).unwrap_or_default());
}

/// Writes what the person typed in after the match (map, mode, hero, frame limit) into the recording's run.json.
pub fn save_scene(folder: &Path, c: &Choices) {
    let file = folder.join("run.json");
    let Ok(mut note) = std::fs::read(&file).map_err(|_| ()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|_| ())) else { return };
    note["scene"] = serde_json::json!({"map": c.map, "mode": c.mode, "hero": c.hero, "frame_limit": c.frame_limit});
    let _ = std::fs::write(file, serde_json::to_vec_pretty(&note).unwrap_or_default());
}

/// Ends PresentMon the way it is meant to be ended, so the file is complete: a second PresentMon stops
/// the named session and the recording one exits by itself. If it has not gone after ten seconds it is closed.
fn end(exe: &Path, child: &mut std::process::Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.args(presentmon::stop_args()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    platform::hidden(&mut cmd);
    let _ = cmd.status();
    let asked = Instant::now();
    while asked.elapsed() < Duration::from_secs(10) {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}
