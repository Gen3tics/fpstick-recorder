//! Packs the table from `frames` small: this is what is sent in place of the recording.
//!
//! Format FPSF1, version 1: the bytes `FPSF1\n`, a 4-byte
//! little-endian length, that many bytes of JSON (the header), then one zlib stream holding the
//! numeric columns in the header's order.
//! A column is stored as 8-byte little-endian whole numbers: value x scale, each as the difference
//! from the row before. The scale is the smallest of 10^4, 10^7, 10^9 that gives back every value
//! exactly; a column none of them fits is stored as plain 8-byte floats. Empty cells are listed in
//! the header as [start, length] runs.

use crate::frames::{Column, Meta, Parsed};
use flate2::{write::ZlibEncoder, Compression};
use serde_json::{json, Map, Value};
use std::io::Write;

pub const MAGIC: &[u8] = b"FPSF1\n";
const SCALES: [f64; 3] = [1e4, 1e7, 1e9];

fn runs(col: &Column) -> Vec<[usize; 2]> {
    let mut out: Vec<[usize; 2]> = Vec::new();
    for (i, v) in col.iter().enumerate() {
        if v.is_none() {
            match out.last_mut() {
                Some(last) if last[0] + last[1] == i => last[1] += 1,
                _ => out.push([i, 1]),
            }
        }
    }
    out
}

/// The whole-number form of a column at one scale, if every value comes back exactly from it.
/// This repeats the Python's test step for step: round half to even, then divide and compare.
fn steps(vals: &[f64], scale: f64) -> Option<Vec<i64>> {
    let mut q = Vec::with_capacity(vals.len());
    for &v in vals {
        let x = (v * scale).round_ties_even();
        if !(x.abs() < 9.0e18) || x / scale != v {
            return None;
        }
        q.push(x as i64);
    }
    Some(q)
}

fn pairs(v: &[(String, u64)]) -> Value {
    let mut m = Map::new();
    for (k, n) in v {
        m.insert(k.clone(), json!(n));
    }
    Value::Object(m)
}

fn meta_json(m: &Meta) -> Value {
    json!({
        "file": m.file, "format": m.format, "app": m.app, "frames": m.frames, "duration_s": m.duration_s,
        "present_modes": pairs(&m.present_modes), "runtimes": pairs(&m.runtimes), "dropped": m.dropped,
        "allows_tearing": m.allows_tearing, "gpu": m.gpu, "cpu": m.cpu, "resolution": m.resolution,
        "fields": m.fields, "qpc0": m.qpc0, "qpc_freq": m.qpc_freq,
    })
}

pub fn pack(parsed: &Parsed) -> std::io::Result<Vec<u8>> {
    let rows = parsed.cols.first().map(|(_, c)| c.len()).unwrap_or(0);
    let mut cols_head: Vec<Value> = Vec::new();
    let mut body: Vec<u8> = Vec::with_capacity(rows * 8 * parsed.cols.len());
    for (name, col) in &parsed.cols {
        if col.len() != rows {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "The columns of this recording are not the same length."));
        }
        let vals: Vec<f64> = col.iter().map(|v| v.unwrap_or(0.0)).collect();
        let mut entry = json!({"name": name, "kind": "float", "empty": runs(col)});
        match SCALES.iter().find_map(|s| steps(&vals, *s).map(|q| (*s, q))) {
            Some((scale, q)) => {
                entry["kind"] = json!("steps");
                entry["scale"] = json!(scale as u64);
                let mut prev = 0i64;
                for (i, k) in q.iter().enumerate() {
                    body.extend_from_slice(&(if i == 0 { *k } else { k - prev }).to_le_bytes());
                    prev = *k;
                }
            }
            None => vals.iter().for_each(|v| body.extend_from_slice(&v.to_le_bytes())),
        }
        cols_head.push(entry);
    }
    let head = json!({"v": 1, "meta": meta_json(&parsed.meta), "notes": parsed.notes, "cols": cols_head, "rows": rows});
    let hj = serde_json::to_vec(&head)?;
    let mut out = Vec::with_capacity(MAGIC.len() + 4 + hj.len() + body.len() / 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(hj.len() as u32).to_le_bytes());
    out.extend_from_slice(&hj);
    let mut z = ZlibEncoder::new(out, Compression::new(6));
    z.write_all(&body)?;
    z.finish()
}

/// What `unpack` gives back: the table that was packed. The header's `meta` is kept as it was sent.
#[derive(Debug, Clone)]
pub struct Unpacked {
    pub meta: Value,
    /// Numeric columns in the header's order.
    pub cols: Vec<(String, Column)>,
    pub notes: Vec<String>,
}

impl Unpacked {
    pub fn col(&self, name: &str) -> Option<&Column> {
        self.cols.iter().find(|(n, _)| n == name).map(|(_, c)| c)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpackError(pub &'static str);

impl std::fmt::Display for UnpackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for UnpackError {}

/// A whole match at 600 frames a second is under a million rows.
pub const MAX_ROWS: usize = 5_000_000;
const NOT_OURS: UnpackError = UnpackError("This is not frame data from this tool, or it is from a newer version.");
const BAD_SIZE: UnpackError = UnpackError("This frame data has a version or a size this tool does not accept.");
const CUT: UnpackError = UnpackError("This frame data is cut short or longer than it says.");
const UNREADABLE: UnpackError = UnpackError("This frame data could not be read.");

/// A whole number of steps divided by its scale, rounded once, as Python's `int / int` does it.
fn unscale(run: i64, scale: u64) -> f64 {
    if run.unsigned_abs() <= (1u64 << 53) {
        return run as f64 / scale as f64; // both exact as floats, so the division rounds once
    }
    // beyond 2^53 the whole number itself would be rounded first: read the exact decimal instead
    let digits = scale.to_string().len() - 1;
    format!("{}e-{}", run, digits).parse().unwrap_or(f64::NAN)
}

/// The header's description of the recording, without unpacking the columns.
pub fn meta_of(data: &[u8]) -> Option<Value> {
    let at = MAGIC.len();
    let rest = data.strip_prefix(MAGIC)?;
    let hl = rest.get(..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)?;
    let head: Value = serde_json::from_slice(data.get(at + 4..at + 4 + hl)?).ok()?;
    head.get("meta").cloned()
}

/// Gives back exactly the table that was packed. Whatever is malformed is refused with a plain sentence;
/// nothing here trusts the sender: sizes are checked before memory is taken, and no more is inflated
/// than the header declares.
pub fn unpack(data: &[u8]) -> Result<Unpacked, UnpackError> {
    use std::io::Read;
    if !data.starts_with(MAGIC) {
        return Err(NOT_OURS);
    }
    let at = MAGIC.len();
    let hl = data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or(UNREADABLE)?;
    let head: Value = data.get(at + 4..at + 4 + hl).and_then(|b| serde_json::from_slice(b).ok()).ok_or(UNREADABLE)?;
    let rows = head.get("rows").and_then(Value::as_u64).ok_or(UNREADABLE)? as usize;
    if head.get("v").and_then(Value::as_u64) != Some(1) || rows > MAX_ROWS {
        return Err(BAD_SIZE);
    }
    let heads = head.get("cols").and_then(Value::as_array).ok_or(UNREADABLE)?;
    let numeric: Vec<&Value> = heads.iter().filter(|c| c.get("kind").and_then(Value::as_str) != Some("text")).collect();
    if numeric.len() > 64 {
        return Err(BAD_SIZE);
    }
    let want = rows * 8 * numeric.len();
    let mut raw = Vec::with_capacity(want.min(1 << 26));
    let mut z = flate2::read::ZlibDecoder::new(&data[at + 4 + hl..]);
    (&mut z).take(want as u64 + 1).read_to_end(&mut raw).map_err(|_| CUT)?;
    if raw.len() != want || z.total_in() as usize != data.len() - (at + 4 + hl) {
        return Err(CUT);
    }
    let mut cols = Vec::with_capacity(numeric.len());
    for (k, c) in numeric.iter().enumerate() {
        let name = c.get("name").and_then(Value::as_str).ok_or(UNREADABLE)?.to_string();
        let cells = raw[k * rows * 8..(k + 1) * rows * 8].chunks_exact(8).map(|b| [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        let mut out: Column = match c.get("kind").and_then(Value::as_str) {
            Some("steps") => {
                let scale = c.get("scale").and_then(Value::as_u64).filter(|s| SCALES.contains(&(*s as f64))).ok_or(UNREADABLE)?;
                let mut run = 0i64;
                let mut v = Vec::with_capacity(rows);
                for b in cells {
                    run = run.checked_add(i64::from_le_bytes(b)).ok_or(UNREADABLE)?;
                    v.push(Some(unscale(run, scale)));
                }
                v
            }
            Some("float") => cells.map(|b| Some(f64::from_le_bytes(b))).collect(),
            _ => return Err(UNREADABLE),
        };
        for run in c.get("empty").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]) {
            let (start, len) = (run.get(0).and_then(Value::as_u64).ok_or(UNREADABLE)? as usize, run.get(1).and_then(Value::as_u64).ok_or(UNREADABLE)? as usize);
            out.get_mut(start..start.checked_add(len).ok_or(UNREADABLE)?).ok_or(UNREADABLE)?.fill(None);
        }
        cols.push((name, out));
    }
    let notes = head.get("notes").and_then(Value::as_array).map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect()).unwrap_or_default();
    Ok(Unpacked { meta: head.get("meta").cloned().unwrap_or(Value::Null), cols, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    fn parsed() -> Parsed {
        let cols = vec![
            ("frame_ms".to_string(), vec![Some(3.1254), Some(3.2), Some(2.9999)]),
            ("pc_latency".to_string(), vec![None, None, Some(7.1234567)]),
            ("odd".to_string(), vec![Some(1.0 / 3.0), Some(2.0), None]),
        ];
        let meta = Meta { file: "frames.csv".into(), format: "presentmon-2.x".into(), app: Some("Overwatch.exe".into()), frames: 3, duration_s: 0.0093, present_modes: vec![("Hardware: Independent Flip".into(), 3)], runtimes: vec![], dropped: None, allows_tearing: Some("1".into()), gpu: None, cpu: None, resolution: None, fields: vec!["frame_ms".into(), "odd".into(), "pc_latency".into()], qpc0: Some(5e9), qpc_freq: Some(1e7) };
        Parsed { meta, cols, notes: vec![] }
    }

    #[test]
    fn packs_what_the_python_reader_expects() {
        let data = pack(&parsed()).unwrap();
        assert!(data.starts_with(MAGIC));
        let hl = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
        let head: Value = serde_json::from_slice(&data[10..10 + hl]).unwrap();
        assert_eq!(head["v"], 1);
        assert_eq!(head["rows"], 3);
        assert_eq!(head["cols"][0], json!({"name": "frame_ms", "kind": "steps", "empty": [], "scale": 10000}));
        assert_eq!(head["cols"][1], json!({"name": "pc_latency", "kind": "steps", "empty": [[0, 2]], "scale": 10000000}));
        assert_eq!(head["cols"][2]["kind"], "float");
        assert_eq!(head["cols"][2]["empty"], json!([[2, 1]]));
        assert_eq!(head["meta"]["qpc_freq"].to_string(), "10000000.0");
        let mut raw = Vec::new();
        ZlibDecoder::new(&data[10 + hl..]).read_to_end(&mut raw).unwrap();
        assert_eq!(raw.len(), 3 * 8 * 3);
        let k = |i: usize| i64::from_le_bytes(raw[i * 8..i * 8 + 8].try_into().unwrap());
        assert_eq!((k(0), k(1), k(2)), (31254, 32000 - 31254, 29999 - 32000));
        assert_eq!(f64::from_le_bytes(raw[48..56].try_into().unwrap()), 1.0 / 3.0);
    }

    #[test]
    fn whole_numbers_too_large_for_a_scale_fall_back_to_floats() {
        assert!(steps(&[1e15], 1e4).is_none());
        assert_eq!(steps(&[123456789012.0], 1e4), Some(vec![1234567890120000]));
    }

    #[test]
    fn unpack_gives_back_what_was_packed() {
        let p = parsed();
        let data = pack(&p).unwrap();
        let u = unpack(&data).unwrap();
        assert_eq!(u.cols, p.cols);
        assert_eq!(u.meta["app"], "Overwatch.exe");
        assert_eq!(u.meta["frames"], 3);
        assert_eq!(u.col("pc_latency").unwrap()[2], Some(7.1234567));
    }

    #[test]
    fn unpack_refuses_what_is_malformed() {
        let data = pack(&parsed()).unwrap();
        assert_eq!(unpack(b"not a digest").unwrap_err(), NOT_OURS);
        assert!(unpack(&data[..data.len() / 2]).is_err());
        let mut longer = data.clone();
        longer.extend_from_slice(b"extra");
        assert_eq!(unpack(&longer).unwrap_err(), CUT);
        // a header that claims more rows than the data holds
        let hl = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
        let mut head: Value = serde_json::from_slice(&data[10..10 + hl]).unwrap();
        head["rows"] = json!(4);
        let hj = serde_json::to_vec(&head).unwrap();
        let mut lying = MAGIC.to_vec();
        lying.extend_from_slice(&(hj.len() as u32).to_le_bytes());
        lying.extend_from_slice(&hj);
        lying.extend_from_slice(&data[10 + hl..]);
        assert_eq!(unpack(&lying).unwrap_err(), CUT);
        head["rows"] = json!(MAX_ROWS + 1);
        let hj = serde_json::to_vec(&head).unwrap();
        let mut huge = MAGIC.to_vec();
        huge.extend_from_slice(&(hj.len() as u32).to_le_bytes());
        huge.extend_from_slice(&hj);
        assert_eq!(unpack(&huge).unwrap_err(), BAD_SIZE);
    }

    #[test]
    fn large_whole_numbers_divide_as_python_does() {
        assert_eq!(unscale(31254, 10_000), 3.1254);
        assert_eq!(unscale(-5, 10_000_000), -5e-7);
        // 2^53 + 1 is not a float; Python's int / int still rounds the true quotient once
        assert_eq!(unscale(9_007_199_254_740_993, 10_000), 900719925474.0993);
    }
}
