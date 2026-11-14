//! "Game network stats": when the game's packets left and arrived, how big they were, and how quickly
//! three routers on the way to the server answer a ping.
//!
//! The packets: Windows itself records every UDP datagram a process sends or receives (the provider
//! Microsoft-Windows-Kernel-Network). This listens to that record while the recording runs and keeps,
//! for the game's process only, the time, the direction and the size. It never sees what is in a packet.
//! The times are on the same clock PresentMon stamps frames with (the performance counter), so the two
//! line up without any marker packets.
//!
//! The route: once the game is talking to a server, the way there is looked up once (16 pings that are
//! allowed 1, 2, 3 ... hops), and three of the routers on it are then pinged five times a second: the
//! home router, the first router of the internet provider, and the last one before the server.
//!
//! Addresses are used while recording, to tell the server's packets from others and to aim the pings.
//! They are never written to a file: `udp.csv` numbers the remote ends (1 = the busiest, the match
//! server) and `ping.csv` names the routers by role.

use std::io::Write;
use std::path::Path;

/// One datagram of the game's process.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Datagram {
    pub qpc: i64,
    pub out: bool,
    pub bytes: u32,
    /// The other end, as four address bytes and a port. Never written to disk.
    pub remote: ([u8; 4], u16),
}

/// One ping: when it was sent, which router, and how long the answer took (None: no answer in 300 ms).
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub qpc: i64,
    pub hop: &'static str,
    pub rtt_ms: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetData {
    pub datagrams: Vec<Datagram>,
    pub probes: Vec<Probe>,
    /// Which of "router", "isp", "edge" were found and pinged.
    pub hops: Vec<&'static str>,
    /// Whether pinging the route was asked for at all.
    pub pings_asked: bool,
    /// Events Windows could not deliver in time.
    pub events_lost: u64,
    /// Datagrams of the game over IPv6, which are counted but not kept.
    pub ipv6: u64,
    pub qpc_per_s: i64,
}

/// What the files hold, for the window and for net.json.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetSummary {
    pub datagrams: usize,
    pub received: usize,
    pub sent: usize,
    pub flows: usize,
    pub flow1_share: f64,
    pub probes: usize,
    pub hops: Vec<&'static str>,
    pub pings_asked: bool,
    pub events_lost: u64,
    pub ipv6: u64,
}

/// Addresses that are not out on the internet: this PC, the home network, link-local, multicast.
pub fn is_local(a: [u8; 4]) -> bool {
    matches!(a, [127, ..] | [10, ..] | [192, 168, ..] | [169, 254, ..] | [255, ..]) || (a[0] == 172 && (16..=31).contains(&a[1])) || (224..=239).contains(&a[0])
}

/// The game's server, if the game is talking to one: the internet address that got at least 30 of
/// the datagrams sent in the last second.
pub fn server_of(recent_out: &[[u8; 4]]) -> Option<[u8; 4]> {
    let mut counts: Vec<([u8; 4], usize)> = Vec::new();
    for a in recent_out.iter().filter(|a| !is_local(**a)) {
        match counts.iter_mut().find(|(k, _)| k == a) {
            Some((_, n)) => *n += 1,
            None => counts.push((*a, 1)),
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).filter(|(_, n)| *n >= 30).map(|(a, _)| a)
}

/// Numbers the remote ends by how many datagrams they have: 1 is the busiest. Ties keep the order first seen.
pub fn flows(d: &[Datagram]) -> Vec<(([u8; 4], u16), usize)> {
    let mut counts: Vec<(([u8; 4], u16), usize)> = Vec::new();
    for x in d {
        match counts.iter_mut().find(|(k, _)| *k == x.remote) {
            Some((_, n)) => *n += 1,
            None => counts.push((x.remote, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    counts
}

impl NetData {
    /// Writes `udp.csv` (qpc,dir,bytes,flow), `ping.csv` (qpc,hop,rtt_ms) and `net.json` into the
    /// recording's folder, in the form the analysis already reads. No address goes into any of them.
    pub fn write(&self, folder: &Path) -> std::io::Result<NetSummary> {
        let ranks = flows(&self.datagrams);
        let rank = |r: &([u8; 4], u16)| ranks.iter().position(|(k, _)| k == r).unwrap_or(0) + 1;
        let mut d = self.datagrams.clone();
        d.sort_by_key(|x| x.qpc);
        let mut f = std::io::BufWriter::new(std::fs::File::create(folder.join("udp.csv"))?);
        f.write_all(b"qpc,dir,bytes,flow\r\n")?;
        for x in &d {
            write!(f, "{},{},{},{}\r\n", x.qpc, if x.out { "out" } else { "in" }, x.bytes, rank(&x.remote))?;
        }
        f.flush()?;
        if !self.probes.is_empty() {
            let mut p = self.probes.clone();
            p.sort_by_key(|x| x.qpc);
            let mut f = std::io::BufWriter::new(std::fs::File::create(folder.join("ping.csv"))?);
            f.write_all(b"qpc,hop,rtt_ms\n")?;
            for x in &p {
                match x.rtt_ms {
                    Some(ms) => writeln!(f, "{},{},{:.3}", x.qpc, x.hop, ms)?,
                    None => writeln!(f, "{},{},", x.qpc, x.hop)?,
                }
            }
            f.flush()?;
        }
        let main = ranks.first().map(|(k, _)| *k);
        let s = NetSummary {
            datagrams: d.len(),
            received: d.iter().filter(|x| !x.out && Some(x.remote) == main).count(),
            sent: d.iter().filter(|x| x.out && Some(x.remote) == main).count(),
            flows: ranks.len(),
            flow1_share: ranks.first().map(|(_, n)| *n as f64 / d.len().max(1) as f64).unwrap_or(0.0),
            probes: self.probes.len(),
            hops: self.hops.clone(),
            pings_asked: self.pings_asked,
            events_lost: self.events_lost,
            ipv6: self.ipv6,
        };
        let note = serde_json::json!({
            "recorder": env!("CARGO_PKG_VERSION"),
            "clock": "qpc values are the performance counter, the clock of PresentMon's CPUStartQPC; taken from Windows' own event stamps, no marker packets",
            "qpc_per_s": self.qpc_per_s,
            "game_datagrams": s.datagrams, "flows": s.flows,
            "flow_note": "flow 1 = the busiest remote end of the game process (the match server); numbers only, no addresses are kept",
            "flow1_received": s.received, "flow1_sent": s.sent, "flow1_share_of_game_datagrams": (s.flow1_share * 1e4).round() / 1e4,
            "route_pings_asked_for": s.pings_asked, "probes": s.probes, "probed_hops": s.hops, "events_lost": s.events_lost, "ipv6_datagrams_not_kept": s.ipv6,
        });
        std::fs::write(folder.join("net.json"), serde_json::to_vec_pretty(&note).unwrap_or_default())?;
        Ok(s)
    }
}

#[cfg(windows)]
mod imp {
    use super::{is_local, server_of, Datagram, NetData, Probe};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION};
    use windows_sys::Win32::System::Diagnostics::Etw::*;
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

    const SESSION: &str = "recorder-net";
    /// Microsoft-Windows-Kernel-Network
    const PROVIDER: GUID = GUID { data1: 0x7dd42a49, data2: 0x5329, data3: 0x4832, data4: [0x8d, 0xfd, 0x43, 0xd9, 0x79, 0x15, 0x3a, 0x88] };
    const KEYWORD_IPV4: u64 = 0x10;
    const KEYWORD_IPV6: u64 = 0x20;
    const UDP_SENT_V4: u16 = 42;
    const UDP_RECEIVED_V4: u16 = 43;
    const UDP_SENT_V6: u16 = 58;
    const UDP_RECEIVED_V6: u16 = 59;

    pub fn qpc() -> i64 {
        let mut v = 0i64;
        unsafe { QueryPerformanceCounter(&mut v) };
        v
    }

    pub fn qpc_per_s() -> i64 {
        let mut v = 0i64;
        unsafe { QueryPerformanceFrequency(&mut v) };
        v.max(1)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// EVENT_FILTER_EVENT_ID with room for four event numbers.
    #[repr(C)]
    struct EventIds {
        filter_in: u8,
        reserved: u8,
        count: u16,
        events: [u16; 4],
    }

    /// What the listener shares with the event callback.
    struct Shared {
        pid: u32,
        /// Whether Windows took the filter that leaves only the UDP events.
        filtered: bool,
        data: Mutex<Vec<Datagram>>,
        ipv6: AtomicU64,
        /// Every event of the provider seen, whoever's: shows the listener is alive.
        seen: AtomicU64,
        raw: Mutex<Vec<String>>,
        keep_raw: bool,
    }

    unsafe extern "system" fn on_event(rec: *mut EVENT_RECORD) {
        let rec = &*rec;
        let sh = &*(rec.UserContext as *const Shared);
        sh.seen.fetch_add(1, Ordering::Relaxed);
        let id = rec.EventHeader.EventDescriptor.Id;
        let user = std::slice::from_raw_parts(rec.UserData as *const u8, rec.UserDataLength as usize);
        if user.len() < 4 {
            return;
        }
        let pid = u32::from_le_bytes([user[0], user[1], user[2], user[3]]);
        if sh.keep_raw {
            let mut raw = sh.raw.lock().unwrap();
            if raw.len() < 4000 {
                raw.push(format!("{} {} {} {}", id, pid, rec.EventHeader.TimeStamp, user.iter().take(28).map(|b| format!("{:02x}", b)).collect::<String>()));
            }
        }
        if pid != sh.pid {
            return;
        }
        match id {
            UDP_SENT_V4 | UDP_RECEIVED_V4 if user.len() >= 20 => {
                // PID, size, destination address, source address, destination port, source port; ports in network order
                let out = id == UDP_SENT_V4;
                let bytes = u32::from_le_bytes([user[4], user[5], user[6], user[7]]);
                let (daddr, saddr) = ([user[8], user[9], user[10], user[11]], [user[12], user[13], user[14], user[15]]);
                let (dport, sport) = (u16::from_be_bytes([user[16], user[17]]), u16::from_be_bytes([user[18], user[19]]));
                let remote = if out { (daddr, dport) } else { (saddr, sport) };
                sh.data.lock().unwrap().push(Datagram { qpc: rec.EventHeader.TimeStamp, out, bytes, remote });
            }
            UDP_SENT_V6 | UDP_RECEIVED_V6 => {
                sh.ipv6.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    fn properties(name: &[u16]) -> Vec<u64> {
        // the structure, then room for the session's name; u64 cells keep it aligned
        let size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() + name.len() * 2 + 16;
        let mut buf = vec![0u64; size.div_ceil(8)];
        let p = unsafe { &mut *(buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES) };
        p.Wnode.BufferSize = (buf.len() * 8) as u32;
        p.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        p.Wnode.ClientContext = 1; // stamps are the performance counter
        p.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        p.BufferSize = 64;
        p.MinimumBuffers = 16;
        p.MaximumBuffers = 64;
        p.FlushTimer = 1;
        p.LoggerNameOffset = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        buf
    }

    pub struct NetCapture {
        shared: Arc<Shared>,
        session: CONTROLTRACE_HANDLE,
        reader: Option<std::thread::JoinHandle<()>>,
        prober: Option<std::thread::JoinHandle<(Vec<Probe>, Vec<&'static str>)>>,
        stop: Arc<AtomicBool>,
        pings: bool,
    }

    impl NetCapture {
        /// Starts listening for the datagrams of one process, and, if `pings`, pinging the route once the
        /// game is on a server. Needs administrator rights.
        pub fn start(pid: u32, keep_raw: bool, pings: bool) -> Result<NetCapture, String> {
            let name = wide(SESSION);
            let mut session: CONTROLTRACE_HANDLE = unsafe { std::mem::zeroed() };
            let mut props = properties(&name);
            let mut rc = unsafe { StartTraceW(&mut session, name.as_ptr(), props.as_mut_ptr() as *mut _) };
            if rc == 183 {
                // a session of this name was left behind: end it and start again
                let mut old = properties(&name);
                unsafe { ControlTraceW(std::mem::zeroed(), name.as_ptr(), old.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP) };
                props = properties(&name);
                rc = unsafe { StartTraceW(&mut session, name.as_ptr(), props.as_mut_ptr() as *mut _) };
            }
            if rc != 0 {
                return Err(format!("Windows would not start the network record (error {}). It needs administrator rights.", rc));
            }
            // Only the four UDP events are asked for. Windows drops every other event of this provider (all
            // TCP traffic of every program) before it reaches the session, so this program is not woken for them.
            // If Windows refuses the filter, the provider is switched on without it and the callback sorts instead.
            let mut wanted = EventIds { filter_in: 1, reserved: 0, count: 4, events: [UDP_SENT_V4, UDP_RECEIVED_V4, UDP_SENT_V6, UDP_RECEIVED_V6] };
            let mut desc = EVENT_FILTER_DESCRIPTOR { Ptr: &mut wanted as *mut EventIds as u64, Size: std::mem::size_of::<EventIds>() as u32, Type: EVENT_FILTER_TYPE_EVENT_ID };
            let mut params: ENABLE_TRACE_PARAMETERS = unsafe { std::mem::zeroed() };
            params.Version = ENABLE_TRACE_PARAMETERS_VERSION_2;
            params.EnableFilterDesc = &mut desc;
            params.FilterDescCount = 1;
            let mut rc = unsafe { EnableTraceEx2(session, &PROVIDER, EVENT_CONTROL_CODE_ENABLE_PROVIDER, 4, KEYWORD_IPV4 | KEYWORD_IPV6, 0, 0, &params) };
            let filtered = rc == 0;
            if !filtered {
                rc = unsafe { EnableTraceEx2(session, &PROVIDER, EVENT_CONTROL_CODE_ENABLE_PROVIDER, 4, KEYWORD_IPV4 | KEYWORD_IPV6, 0, 0, std::ptr::null()) };
            }
            if rc != 0 {
                let mut p = properties(&name);
                unsafe { ControlTraceW(session, std::ptr::null(), p.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP) };
                return Err(format!("Windows would not switch on its record of network packets (error {}).", rc));
            }
            // room for half an hour at 145 packets a second, so the list is never moved while the game is played
            let shared = Arc::new(Shared { pid, filtered, data: Mutex::new(Vec::with_capacity(1 << 18)), ipv6: AtomicU64::new(0), seen: AtomicU64::new(0), raw: Mutex::new(Vec::new()), keep_raw });
            let context = Arc::as_ptr(&shared) as usize;
            let reader = std::thread::spawn(move || unsafe {
                // Windows stamps each event when it happens, so this thread may read them as late as it likes
                crate::platform::yield_to_the_game();
                let mut name = wide(SESSION);
                let mut log: EVENT_TRACE_LOGFILEW = std::mem::zeroed();
                log.LoggerName = name.as_mut_ptr();
                log.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD | PROCESS_TRACE_MODE_RAW_TIMESTAMP;
                log.Anonymous2.EventRecordCallback = Some(on_event);
                log.Context = context as *mut _;
                let h = OpenTraceW(&mut log);
                ProcessTrace(&h, 1, std::ptr::null(), std::ptr::null());
                CloseTrace(h);
            });
            let stop = Arc::new(AtomicBool::new(false));
            let (sh, st) = (shared.clone(), stop.clone());
            // without pings nothing looks for the server and no ping is sent: the listener is all that runs
            let prober = pings.then(|| std::thread::spawn(move || probe(sh, st)));
            Ok(NetCapture { shared, session, reader: Some(reader), prober, stop, pings })
        }

        /// Stops listening and pinging and hands back what was gathered.
        pub fn stop(mut self) -> NetData {
            let (lost, probes, hops) = self.shut();
            let datagrams = std::mem::take(&mut *self.shared.data.lock().unwrap());
            NetData { datagrams, probes, hops, pings_asked: self.pings, events_lost: lost, ipv6: self.shared.ipv6.load(Ordering::Relaxed), qpc_per_s: qpc_per_s() }
        }

        /// Ends the Windows session and waits for both threads. Safe to call twice.
        fn shut(&mut self) -> (u64, Vec<Probe>, Vec<&'static str>) {
            self.stop.store(true, Ordering::SeqCst);
            let mut lost = 0;
            if let Some(r) = self.reader.take() {
                let name = wide(SESSION);
                let mut p = properties(&name);
                unsafe { ControlTraceW(self.session, std::ptr::null(), p.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP) };
                let after = unsafe { &*(p.as_ptr() as *const EVENT_TRACE_PROPERTIES) };
                lost = after.EventsLost as u64 + after.RealTimeBuffersLost as u64;
                let _ = r.join();
            }
            let (probes, hops) = self.prober.take().and_then(|p| p.join().ok()).unwrap_or_default();
            (lost, probes, hops)
        }

        /// The first raw events seen, for checking the layout (`recorder nettest`).
        pub fn raw(&self) -> (u64, Vec<String>) {
            (self.shared.seen.load(Ordering::Relaxed), self.shared.raw.lock().unwrap().clone())
        }

        /// Whether Windows hands over only the UDP events (true), or every event of the provider (false).
        pub fn filtered(&self) -> bool {
            self.shared.filtered
        }
    }

    impl Drop for NetCapture {
        fn drop(&mut self) {
            self.shut();
        }
    }

    /// Checks on this PC, for `recorder nettest`: one ping to this PC itself, and whether the first router on the way out answers.
    pub fn ping_check() -> (Option<f64>, bool) {
        let p = Pinger(unsafe { IcmpCreateFile() });
        let me = ping(&p, [127, 0, 0, 1], 64, 500).map(|(_, _, ms)| ms);
        let first = matches!(ping(&p, [1, 1, 1, 1], 1, 800), Some((a, false, _)) if is_local(a));
        (me, first)
    }

    struct Pinger(HANDLE);
    unsafe impl Send for Pinger {}
    impl Drop for Pinger {
        fn drop(&mut self) {
            unsafe { IcmpCloseHandle(self.0) };
        }
    }

    /// One ping. Returns who answered, whether it was the target itself, and how long it took on the performance counter.
    fn ping(p: &Pinger, to: [u8; 4], ttl: u8, timeout_ms: u32) -> Option<([u8; 4], bool, f64)> {
        let data = *b"recorder-route-probe";
        let opt = IP_OPTION_INFORMATION { Ttl: ttl, Tos: 0, Flags: 0, OptionsSize: 0, OptionsData: std::ptr::null_mut() };
        let mut reply = [0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + 64];
        let t0 = qpc();
        let n = unsafe { IcmpSendEcho(p.0, u32::from_le_bytes(to), data.as_ptr() as *const _, data.len() as u16, &opt, reply.as_mut_ptr() as *mut _, reply.len() as u32, timeout_ms) };
        let t1 = qpc();
        if n == 0 {
            return None;
        }
        let r = unsafe { &*(reply.as_ptr() as *const ICMP_ECHO_REPLY) };
        // 0 = the target answered; 11013 = a router on the way said the ping had used up its hops
        match r.Status {
            0 => Some((r.Address.to_le_bytes(), true, (t1 - t0) as f64 * 1000.0 / qpc_per_s() as f64)),
            11013 => Some((r.Address.to_le_bytes(), false, (t1 - t0) as f64 * 1000.0 / qpc_per_s() as f64)),
            _ => None,
        }
    }

    fn answers(to: [u8; 4]) -> bool {
        let p = Pinger(unsafe { IcmpCreateFile() });
        (0..3).any(|_| matches!(ping(&p, to, 64, 500), Some((_, true, _))))
    }

    /// Waits for the game to be on a server, finds three routers on the way, then pings them five times a second.
    fn probe(sh: Arc<Shared>, stop: Arc<AtomicBool>) -> (Vec<Probe>, Vec<&'static str>) {
        // 1. the server: the internet address that gets at least 30 of the game's datagrams in a second
        let per_s = qpc_per_s();
        let server = loop {
            if stop.load(Ordering::SeqCst) {
                return (Vec::new(), Vec::new());
            }
            std::thread::sleep(Duration::from_millis(500));
            // the last second of what has ARRIVED: Windows hands events over in batches up to a second late,
            // so "the last second by the clock" would mostly be empty
            let data = sh.data.lock().unwrap();
            let since = data.last().map(|d| d.qpc).unwrap_or(0) - per_s;
            let recent: Vec<[u8; 4]> = data.iter().rev().take_while(|d| d.qpc >= since).filter(|d| d.out).map(|d| d.remote.0).collect();
            drop(data);
            if let Some(s) = server_of(&recent) {
                break s;
            }
        };
        // 2. the route, once: pings allowed 1 to 16 hops, three rounds, all at the same time
        let mut at: Vec<Option<[u8; 4]>> = vec![None; 17];
        for _ in 0..3 {
            let jobs: Vec<_> = (1u8..=16).filter(|t| at[*t as usize].is_none()).map(|ttl| std::thread::spawn(move || (ttl, ping(&Pinger(unsafe { IcmpCreateFile() }), server, ttl, 450)))).collect();
            for j in jobs {
                if let Ok((ttl, Some((who, false, _)))) = j.join() {
                    at[ttl as usize] = Some(who);
                }
            }
        }
        let on_route: Vec<(u8, [u8; 4])> = (1u8..=16).filter_map(|t| at[t as usize].map(|a| (t, a))).collect();
        // which of them answer a ping of their own: asked of all at once, since one that does not costs 1.5 s
        let asked: Vec<_> = on_route.iter().map(|(_, a)| *a).map(|a| std::thread::spawn(move || (a, answers(a)))).collect();
        let alive: Vec<[u8; 4]> = asked.into_iter().filter_map(|j| j.join().ok()).filter(|(_, ok)| *ok).map(|(a, _)| a).collect();
        let answers = |a: [u8; 4]| alive.contains(&a);
        let edge = on_route.iter().rev().map(|(_, a)| *a).find(|a| *a != server && !is_local(*a) && answers(*a));
        let isp = on_route.iter().filter(|(t, _)| *t >= 2).map(|(_, a)| *a).find(|a| Some(*a) != edge && !is_local(*a) && answers(*a));
        let router = on_route.iter().find(|(t, _)| *t == 1).map(|(_, a)| *a).filter(|a| answers(*a));
        let hops: Vec<(&'static str, [u8; 4])> = [("router", router), ("isp", isp), ("edge", edge)].into_iter().filter_map(|(n, a)| a.map(|a| (n, a))).collect();
        // 3. five pings a second to each, each router on its own thread so a slow answer from one does not hold up the others
        let threads: Vec<_> = hops
            .iter()
            .map(|(name, addr)| {
                let (name, addr, stop) = (*name, *addr, stop.clone());
                std::thread::spawn(move || {
                    let p = Pinger(unsafe { IcmpCreateFile() });
                    // room for half an hour of pings. This thread keeps its ordinary priority: a ping's time is
                    // taken on this thread, so waking it late would be counted as a slow router.
                    let mut out = Vec::with_capacity(5 * 1800 + 64);
                    let mut next = Instant::now();
                    while !stop.load(Ordering::SeqCst) {
                        let sent = qpc();
                        let rtt = match ping(&p, addr, 64, 300) {
                            Some((_, true, ms)) => Some(ms),
                            _ => None,
                        };
                        out.push(Probe { qpc: sent, hop: name, rtt_ms: rtt });
                        next += Duration::from_millis(200);
                        let now = Instant::now();
                        if next > now {
                            std::thread::sleep(next - now);
                        } else {
                            next = now; // an answer took longer than a fifth of a second: carry on from here
                        }
                    }
                    out
                })
            })
            .collect();
        let mut probes = Vec::new();
        for t in threads {
            probes.extend(t.join().unwrap_or_default());
        }
        (probes, hops.iter().map(|(n, _)| *n).collect())
    }

    /// The process id of a running program, by its file name (never this program itself).
    pub fn pid_of(name: &str) -> Option<u32> {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            if Process32FirstW(snap, &mut e) != 0 {
                loop {
                    let exe = String::from_utf16_lossy(&e.szExeFile[..e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len())]);
                    if exe.eq_ignore_ascii_case(name) && e.th32ProcessID != std::process::id() {
                        found = Some(e.th32ProcessID);
                        break;
                    }
                    if Process32NextW(snap, &mut e) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            found
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::NetData;

    pub struct NetCapture;
    impl NetCapture {
        pub fn start(_pid: u32, _keep_raw: bool, _pings: bool) -> Result<NetCapture, String> {
            Err("The network record is only available on Windows.".into())
        }
        pub fn stop(self) -> NetData {
            NetData::default()
        }
        pub fn raw(&self) -> (u64, Vec<String>) {
            (0, Vec::new())
        }
        pub fn filtered(&self) -> bool {
            false
        }
    }
    pub fn pid_of(_name: &str) -> Option<u32> {
        None
    }
    pub fn ping_check() -> (Option<f64>, bool) {
        (None, false)
    }
    pub fn qpc() -> i64 {
        0
    }
    pub fn qpc_per_s() -> i64 {
        10_000_000
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn d(qpc: i64, out: bool, bytes: u32, last: u8, port: u16) -> Datagram {
        Datagram { qpc, out, bytes, remote: ([34, 2, 3, last], port) }
    }

    #[test]
    fn files_are_what_the_analysis_reads_and_hold_no_address() {
        let dir = std::env::temp_dir().join(format!("recorder-net-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data = NetData {
            datagrams: vec![d(300, false, 420, 9, 26000), d(100, true, 75, 9, 26000), d(200, true, 60, 77, 3478), d(400, false, 1100, 9, 26000)],
            probes: vec![Probe { qpc: 150, hop: "router", rtt_ms: Some(0.7071) }, Probe { qpc: 120, hop: "edge", rtt_ms: None }],
            hops: vec!["router", "edge"], pings_asked: true,
            events_lost: 0,
            ipv6: 2,
            qpc_per_s: 10_000_000,
        };
        let s = data.write(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("udp.csv")).unwrap(), "qpc,dir,bytes,flow\r\n100,out,75,1\r\n200,out,60,2\r\n300,in,420,1\r\n400,in,1100,1\r\n");
        assert_eq!(std::fs::read_to_string(dir.join("ping.csv")).unwrap(), "qpc,hop,rtt_ms\n120,edge,\n150,router,0.707\n");
        assert_eq!((s.datagrams, s.flows, s.received, s.sent, s.probes), (4, 2, 2, 1, 2));
        assert_eq!(s.flow1_share, 0.75);
        for f in ["udp.csv", "ping.csv", "net.json"] {
            let text = std::fs::read_to_string(dir.join(f)).unwrap();
            assert!(!text.contains("34.2.3") && !text.contains("26000"), "{} holds an address", f);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_server_is_the_busy_internet_address() {
        let mut recent = vec![[192, 168, 1, 1]; 200];
        recent.extend(vec![[34, 2, 3, 9]; 64]);
        recent.extend(vec![[8, 8, 8, 8]; 5]);
        assert_eq!(server_of(&recent), Some([34, 2, 3, 9]));
        assert_eq!(server_of(&[[34, 2, 3, 9]; 12]), None);
        assert!(is_local([172, 20, 0, 1]) && is_local([224, 0, 0, 251]) && !is_local([172, 32, 0, 1]) && !is_local([100, 64, 0, 1]));
    }
}
