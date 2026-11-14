//! Reads a PresentMon per-frame file into the table of numbers the run page's server judges.
//!
//! FPStick's judgement was first written in Python, and that Python (not in this repository) has a reader
//! of its own. The two must give the same table for the same file, to the last digit, so that a recording
//! is judged the same whichever read it; they are compared on every change, on PresentMon's own test files
//! among others.
//! Where a choice below looks odd (the order of columns, how a sum is taken), it is that way
//! because the Python does it that way.

use std::collections::HashMap;

/// A column's cells; an empty cell is `None`.
pub type Column = Vec<Option<f64>>;

#[derive(Debug, Clone)]
pub struct Meta {
    pub file: String,
    pub format: String,
    pub app: Option<String>,
    pub frames: usize,
    pub duration_s: f64,
    /// In the order first seen, as the Python keeps them.
    pub present_modes: Vec<(String, u64)>,
    pub runtimes: Vec<(String, u64)>,
    pub dropped: Option<u64>,
    pub allows_tearing: Option<String>,
    pub gpu: Option<String>,
    pub cpu: Option<String>,
    pub resolution: Option<String>,
    pub fields: Vec<String>,
    pub qpc0: Option<f64>,
    pub qpc_freq: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Parsed {
    pub meta: Meta,
    /// Columns in the order the Python builds them.
    pub cols: Vec<(String, Column)>,
    pub notes: Vec<String>,
}

impl Parsed {
    pub fn col(&self, name: &str) -> Option<&Column> {
        self.cols.iter().find(|(n, _)| n == name).map(|(_, c)| c)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureError(pub String);

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CaptureError {}

/// field -> names it goes by (lower-case, no spaces); the first one present wins.
const ALIASES: &[(&str, &[&str])] = &[
    ("app", &["application", "processname"]),
    ("pid", &["processid"]),
    ("swapchain", &["swapchainaddress"]),
    ("runtime", &["presentruntime", "runtime"]),
    ("present_mode", &["presentmode"]),
    ("allows_tearing", &["allowstearing"]),
    ("dropped", &["dropped"]),
    ("time_s", &["timeinseconds", "cpustarttime", "cpustarttimeinseconds"]),
    ("frame_ms", &["frametime", "msbetweenpresents", "msbetweenappstart"]),
    ("cpu_busy", &["cpubusy", "mscpubusy"]),
    ("cpu_wait", &["cpuwait", "mscpuwait"]),
    ("gpu_busy", &["gpubusy", "msgpubusy", "msgpuactive"]),
    ("gpu_time", &["gputime", "msgputime"]),
    ("gpu_latency", &["gpulatency", "msgpulatency", "msuntilrenderstart"]),
    ("display_latency", &["displaylatency"]),
    ("until_displayed", &["msuntildisplayed"]),
    ("render_present", &["msrenderpresentlatency"]),
    ("displayed_ms", &["displayedtime"]),
    ("display_interval", &["msbetweendisplaychange"]),
    ("pc_latency", &["mspclatency", "pclatency"]),
    ("qpc", &["cpustartqpc"]),
    ("anim_error", &["animationerror", "msanimationerror"]),
    ("input_latency", &["allinputtophotonlatency", "msallinputtophotonlatency"]),
    ("gpu_util", &["gpu0util(%)"]),
    ("cpu_util", &["cpuutil(%)"]),
    ("gpu_name", &["gpu"]),
    ("cpu_name", &["cpu"]),
    ("resolution", &["resolution"]),
];

const NUMERIC: &[&str] = &[
    "time_s", "frame_ms", "cpu_busy", "cpu_wait", "gpu_busy", "gpu_time", "gpu_latency", "display_latency",
    "until_displayed", "render_present", "displayed_ms", "display_interval", "pc_latency", "qpc", "anim_error",
    "input_latency", "gpu_util", "cpu_util",
];

fn norm(name: &str) -> String {
    name.trim().to_lowercase().chars().filter(|c| !c.is_whitespace()).collect()
}

fn detect_format(header: &[String]) -> Option<&'static str> {
    let h: std::collections::HashSet<String> = header.iter().map(|c| norm(c)).collect();
    let has = |s: &str| h.contains(s);
    if has("gpu0util(%)") || has("renderqueuedepth") || (has("mspclatency") && has("gpu")) {
        return Some("frameview");
    }
    if has("frametime") && has("cpubusy") {
        return Some("presentmon-2.x");
    }
    if has("mscpubusy") {
        return Some("presentmon-current");
    }
    if has("msbetweenpresents") && has("timeinseconds") {
        return Some("presentmon-1.x");
    }
    if has("msbetweenpresents") || has("frametime") {
        return Some("presentmon-like");
    }
    None
}

/// A cell as a number: empty, "NA" and "nan" are no number, and so is anything that does not parse.
fn num(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s == "NA" || s.eq_ignore_ascii_case("nan") {
        return None;
    }
    s.parse::<f64>().ok()
}

/// Splits CSV text into rows of cells, with quoted cells and doubled quotes handled.
fn rows_of(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut any = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => {
                quoted = true;
                any = true;
            }
            ',' => {
                row.push(std::mem::take(&mut cell));
                any = true;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if any || !cell.is_empty() {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                }
                any = false;
            }
            _ => {
                cell.push(c);
                any = true;
            }
        }
    }
    if any || !cell.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// The sum of a list of numbers the way Python 3.12 and later take it: with a running correction
/// (Neumaier's), so that it does not depend on the order of small and large terms.
fn py_sum(v: impl Iterator<Item = f64>) -> f64 {
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for x in v {
        let t = s + x;
        if s.abs() >= x.abs() {
            c += (s - t) + x;
        } else {
            c += (x - t) + s;
        }
        s = t;
    }
    if c != 0.0 && c.is_finite() {
        s + c
    } else {
        s
    }
}

fn bump(counts: &mut Vec<(String, u64)>, key: &str) {
    match counts.iter_mut().find(|(k, _)| k == key) {
        Some((_, n)) => *n += 1,
        None => counts.push((key.to_string(), 1)),
    }
}

/// Reads a per-frame file. `name` is the file's name without its folder; it is all that is kept of where it was.
pub fn parse_capture(bytes: &[u8], name: &str) -> Result<Parsed, CaptureError> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut all = rows_of(text).into_iter();
    let header = all.next().ok_or_else(|| CaptureError("The file is empty.".into()))?;
    if detect_format(&header).is_none() && header.first().map(|c| c.trim_start_matches(['\u{feff}', ' ', '\t']).starts_with('{')).unwrap_or(false) {
        return Err(CaptureError("This is a JSON file, not a per-frame log. If it is a CapFrameX capture, set CapFrameX to save \"JSON + CSV\" and use the CSV file it writes beside the JSON.".into()));
    }
    let fmt = detect_format(&header).ok_or_else(|| {
        CaptureError(
            "This does not look like a PresentMon or FrameView per-frame log (no frame-time column found). \
             Summary files cannot be analysed."
                .into(),
        )
    })?;
    let mut idx: HashMap<String, usize> = HashMap::new();
    for (i, c) in header.iter().enumerate() {
        idx.insert(norm(c), i); // a later column of the same name wins, as in the Python
    }
    let mut col: HashMap<&str, usize> = HashMap::new();
    for (field, names) in ALIASES {
        if let Some(i) = names.iter().find_map(|n| idx.get(*n)) {
            col.insert(field, *i);
        }
    }
    let i_fm = *col.get("frame_ms").ok_or_else(|| CaptureError("No frame-time column found.".into()))?;

    // First pass: rows per (app, process id, swap chain); the busiest one is the game.
    let (ia, ip, isw) = (col.get("app").copied(), col.get("pid").copied(), col.get("swapchain").copied());
    let cell = |row: &Vec<String>, i: Option<usize>| -> String { i.and_then(|i| row.get(i)).cloned().unwrap_or_default() };
    let mut groups: Vec<((String, String, String), Vec<Vec<String>>)> = Vec::new();
    let mut where_is: HashMap<(String, String, String), usize> = HashMap::new();
    let mut nrows = 0usize;
    for row in all {
        if row.len() <= i_fm {
            continue;
        }
        nrows += 1;
        let key = (cell(&row, ia), cell(&row, ip), cell(&row, isw));
        let at = *where_is.entry(key.clone()).or_insert_with(|| {
            groups.push((key, Vec::new()));
            groups.len() - 1
        });
        groups[at].1.push(row);
    }
    if groups.is_empty() {
        return Err(CaptureError("The file has a header but no frames.".into()));
    }
    let mut best = 0;
    for (i, g) in groups.iter().enumerate() {
        if g.1.len() > groups[best].1.len() {
            best = i; // the first of equally long groups stays, as Python's max() keeps it
        }
    }
    let n_groups = groups.len();
    let (key, rows) = groups.swap_remove(best);
    let mut notes = Vec::new();
    if n_groups > 1 {
        notes.push(format!(
            "The file holds {} swap chains or processes; analysed the busiest ({}), ignored {} other rows.",
            n_groups,
            if key.0.is_empty() { "unnamed" } else { &key.0 },
            nrows - rows.len()
        ));
    }

    let mut cols: Vec<(String, Column)> = NUMERIC.iter().filter(|f| col.contains_key(**f)).map(|f| (f.to_string(), Vec::new())).collect();
    let col_at: Vec<usize> = cols.iter().map(|(f, _)| col[f.as_str()]).collect();
    let (i_mode, i_tear, i_rt, i_drop) = (col.get("present_mode").copied(), col.get("allows_tearing").copied(), col.get("runtime").copied(), col.get("dropped").copied());
    let (mut modes, mut runtimes): (Vec<(String, u64)>, Vec<(String, u64)>) = (Vec::new(), Vec::new());
    let mut mode_ids: Vec<String> = Vec::new();
    let mut mode_code: Vec<Option<f64>> = Vec::new();
    let mut dropped = 0u64;
    for row in &rows {
        match num(&row[i_fm]) {
            Some(fm) if fm > 0.0 => {}
            _ => continue,
        }
        for (k, (_, c)) in cols.iter_mut().enumerate() {
            c.push(row.get(col_at[k]).and_then(|s| num(s)));
        }
        if let Some(i) = i_mode {
            let m = row.get(i).map(|s| s.trim()).unwrap_or("");
            let m_key = match i_tear {
                Some(t) => format!("{}|{}", m, row.get(t).map(|s| s.trim()).unwrap_or("")),
                None => m.to_string(),
            };
            bump(&mut modes, m);
            let id = match mode_ids.iter().position(|k| *k == m_key) {
                Some(p) => p,
                None => {
                    mode_ids.push(m_key);
                    mode_ids.len() - 1
                }
            };
            mode_code.push(Some(id as f64));
        }
        if let Some(i) = i_rt {
            bump(&mut runtimes, row.get(i).map(|s| s.trim()).unwrap_or(""));
        }
        if let Some(i) = i_drop {
            if row.get(i).map(|s| s.trim()) == Some("1") {
                dropped += 1;
            }
        }
    }
    let get = |cols: &Vec<(String, Column)>, f: &str| -> Option<Column> { cols.iter().find(|(n, _)| n == f).map(|(_, c)| c.clone()) };
    let frame_ms: Vec<f64> = get(&cols, "frame_ms").unwrap().into_iter().map(|v| v.unwrap()).collect();
    let n = frame_ms.len();
    if n < 100 {
        return Err(CaptureError(format!("Only {} usable frames; at least 100 are needed for any statement.", n)));
    }

    // A time axis that works for every format: the recorded one when it is in seconds and increasing,
    // the performance counter when the file has it, otherwise the frame times added up.
    let total_s = py_sum(frame_ms.iter().copied()) / 1000.0;
    let t = get(&cols, "time_s");
    let ok = match &t {
        Some(t) if !t.is_empty() && t.iter().all(|v| v.is_some()) => {
            let (a, b) = (t[0].unwrap(), t[n - 1].unwrap());
            b > a && ((b - a) - total_s).abs() < 0.2 * (total_s + 1.0)
        }
        _ => false,
    };
    let q = get(&cols, "qpc");
    let (mut qpc0, mut qpc_freq) = (None, None);
    let time_s: Vec<f64> = match (&q, &t) {
        (Some(q), _) if !q.is_empty() && q.iter().all(|v| v.is_some()) && q[n - 1].unwrap() > q[0].unwrap() => {
            // The counter's frequency is not in the file; it is worked out from the frame times and
            // taken as 10 MHz when within 2 % of it.
            let est = (q[n - 1].unwrap() - q[0].unwrap()) / (py_sum(frame_ms[1..].iter().copied()) / 1000.0);
            let freq = if (est - 1e7).abs() / 1e7 < 0.02 { 1e7 } else { est };
            let q0 = q[0].unwrap();
            qpc0 = Some(q0);
            qpc_freq = Some(freq);
            q.iter().map(|v| (v.unwrap() - q0) / freq).collect()
        }
        (_, Some(t)) if ok && (fmt == "frameview" || fmt == "presentmon-1.x") => {
            let start: Vec<f64> = t.iter().zip(&frame_ms).map(|(v, fm)| v.unwrap() - fm / 1000.0).collect();
            start.iter().map(|v| v - start[0]).collect()
        }
        (_, Some(t)) if ok => {
            let t0 = t[0].unwrap();
            t.iter().map(|v| v.unwrap() - t0).collect()
        }
        _ => {
            let mut acc = 0.0;
            frame_ms
                .iter()
                .map(|fm| {
                    let at = acc;
                    acc += fm / 1000.0;
                    at
                })
                .collect()
        }
    };

    // PresentMon's default columns have no DisplayLatency or DisplayedTime; both are rebuilt as the Python does.
    let has = |cols: &Vec<(String, Column)>, f: &str| cols.iter().any(|(n, _)| n == f);
    if !has(&cols, "display_latency") && ["until_displayed", "render_present", "gpu_latency", "gpu_time"].iter().all(|f| has(&cols, f)) {
        let (u, r, g, gt) = (get(&cols, "until_displayed").unwrap(), get(&cols, "render_present").unwrap(), get(&cols, "gpu_latency").unwrap(), get(&cols, "gpu_time").unwrap());
        let out: Column = (0..n)
            .map(|i| match (u[i], r[i], g[i], gt[i]) {
                (Some(u), Some(r), Some(g), Some(gt)) => Some(u - r + g + gt),
                _ => None,
            })
            .collect();
        cols.push(("display_latency".into(), out));
    }
    if !has(&cols, "displayed_ms") && has(&cols, "display_interval") {
        let di = get(&cols, "display_interval").unwrap();
        let mut out: Column = vec![None; n];
        let mut nxt: Option<f64> = None;
        for i in (0..n).rev() {
            // the interval reported on the NEXT displayed frame
            out[i] = if di[i].is_some() { nxt } else { None };
            if di[i].is_some() {
                nxt = di[i];
            }
        }
        cols.push(("displayed_ms".into(), out));
    }
    let time_col: Column = time_s.iter().map(|v| Some(*v)).collect();
    match cols.iter_mut().find(|(f, _)| f == "time_s") {
        Some((_, c)) => *c = time_col,
        None => cols.push(("time_s".into(), time_col)),
    }
    if mode_code.len() == n {
        cols.push(("mode_code".into(), mode_code));
    }

    let first = &rows[0];
    let text_of = |field: &str| -> Option<String> { col.get(field).and_then(|i| first.get(*i)).map(|s| s.trim().to_string()) };
    let mut fields: Vec<String> = cols.iter().map(|(f, _)| f.clone()).collect();
    fields.sort();
    let meta = Meta {
        file: name.to_string(),
        format: fmt.to_string(),
        app: if key.0.is_empty() { text_of("app") } else { Some(key.0.clone()) },
        frames: n,
        duration_s: time_s[n - 1] + frame_ms[n - 1] / 1000.0,
        present_modes: modes,
        runtimes,
        dropped: i_drop.map(|_| dropped),
        allows_tearing: text_of("allows_tearing"),
        gpu: text_of("gpu_name"),
        cpu: text_of("cpu_name"),
        resolution: text_of("resolution"),
        fields,
        qpc0,
        qpc_freq,
    };
    Ok(Parsed { meta, cols, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(rows: usize) -> String {
        let mut s = String::from("Application,ProcessID,SwapChainAddress,PresentRuntime,PresentMode,AllowsTearing,CPUStartQPC,FrameTime,CPUBusy,CPUWait,GPULatency,GPUTime,GPUBusy,DisplayLatency,DisplayedTime,MsPCLatency\r\n");
        let mut q = 5_000_000_000u64;
        for i in 0..rows {
            let ft = 3.125 + (i % 7) as f64 * 0.01;
            s.push_str(&format!("Overwatch.exe,4242,0x1,DXGI,Hardware: Independent Flip,1,{},{:.4},2.1000,0.9000,0.3000,1.5000,1.4000,5.2000,{},{}\r\n", q, ft, if i % 50 == 0 { "NA".to_string() } else { format!("{:.4}", ft) }, if i % 3 == 0 { "7.1000" } else { "NA" }));
            q += (ft * 10_000.0).round() as u64;
        }
        s
    }

    #[test]
    fn reads_a_presentmon_file() {
        let p = parse_capture(sample(500).as_bytes(), "frames.csv").unwrap();
        assert_eq!(p.meta.format, "presentmon-2.x");
        assert_eq!(p.meta.app.as_deref(), Some("Overwatch.exe"));
        assert_eq!(p.meta.frames, 500);
        assert_eq!(p.meta.qpc_freq, Some(1e7));
        assert_eq!(p.col("time_s").unwrap()[0], Some(0.0));
        assert!(p.col("displayed_ms").unwrap()[0].is_none());
        assert_eq!(p.col("pc_latency").unwrap()[1], None);
        assert_eq!(p.meta.present_modes, vec![("Hardware: Independent Flip".to_string(), 500)]);
        assert_eq!(p.col("mode_code").unwrap()[499], Some(0.0));
        let names: Vec<&str> = p.cols.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["frame_ms", "cpu_busy", "cpu_wait", "gpu_busy", "gpu_time", "gpu_latency", "display_latency", "displayed_ms", "pc_latency", "qpc", "time_s", "mode_code"]);
    }

    #[test]
    fn says_why_a_file_cannot_be_read() {
        assert_eq!(parse_capture(b"", "x.csv").unwrap_err().0, "The file is empty.");
        assert!(parse_capture(b"a,b\r\n1,2\r\n", "x.csv").unwrap_err().0.contains("does not look like"));
        assert!(parse_capture(b"{\"Hash\":\"x\",\"Runs\":[]}", "x.json").unwrap_err().0.contains("CapFrameX"));
        assert!(parse_capture(sample(40).as_bytes(), "x.csv").unwrap_err().0.contains("Only 40 usable frames"));
    }

    #[test]
    fn quoted_cells_and_a_byte_order_mark() {
        let rows = rows_of("a,\"b,1\",\"say \"\"hi\"\"\"\r\n1,2,3\n");
        assert_eq!(rows, vec![vec!["a", "b,1", "say \"hi\""], vec!["1", "2", "3"]]);
        let mut text = String::from("\u{feff}");
        text.push_str(&sample(120));
        assert_eq!(parse_capture(text.as_bytes(), "x.csv").unwrap().meta.frames, 120);
    }

    #[test]
    fn sums_like_python() {
        // Python: sum([1e16, 1.0, -1e16]) == 1.0 from 3.12 on; a plain running sum gives 0.0
        assert_eq!(py_sum([1e16, 1.0, -1e16].into_iter()), 1.0);
    }
}
