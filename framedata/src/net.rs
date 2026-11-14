//! The network timing of a recording, packed small for sending (format FPSN1), and unpacked again.
//!
//! What it holds: for each datagram the game exchanged with its match server, when it passed (on the
//! frame recording's clock), which way it went and how many bytes it was; for each ping to a router on
//! the way, when it was sent, which router by role ("router", "isp", "edge") and how long the reply took.
//! What it never holds: an address, a port, or anything that was inside a packet.
//!
//! Format, version 1: the bytes `FPSN1\n`, a 4-byte little-endian length, that many bytes of JSON
//! (`{"v":1,"packets":P,"probes":Q,"hops":[names]}`), then one zlib stream holding, in this order:
//!   P x 8-byte whole numbers   when each datagram passed, in steps of the performance counter, each as the
//!                              difference from the one before; the first counts from the first frame's stamp
//!   P x 1 byte                 0 = received, 1 = sent
//!   P x 4-byte whole numbers   its size in bytes
//!   Q x 8-byte whole numbers   when each ping was sent, stored the same way as the datagrams
//!   Q x 1 byte                 which router: its place in the header's list
//!   Q x 8-byte floats          the reply time in milliseconds; not-a-number where no reply came
//! All little-endian. A second implementation, in Python and not in this repository, writes and reads the
//! same format; the two are checked against each other on every change.

use crate::pack::UnpackError;
use flate2::{write::ZlibEncoder, Compression};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

pub const MAGIC: &[u8] = b"FPSN1\n";
/// Four hours at 250 datagrams a second.
pub const MAX_PACKETS: usize = 4_000_000;
/// Four hours of three routers pinged ten times a second each.
pub const MAX_PROBES: usize = 500_000;
pub const MAX_HOPS: usize = 8;
const MAX_HEAD: usize = 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    /// Steps of the performance counter since the first frame's stamp (negative: before it).
    pub ticks: i64,
    pub out: bool,
    pub bytes: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub ticks: i64,
    /// Place in `NetData::hops`.
    pub hop: u8,
    pub rtt_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NetData {
    /// The match server's datagrams only, in the order they were recorded.
    pub packets: Vec<Packet>,
    pub probes: Vec<Probe>,
    /// The routers by role, in the order they first appear.
    pub hops: Vec<String>,
}

fn field<'a>(head: &[&str], row: &[&'a str], name: &str) -> Option<&'a str> {
    head.iter().position(|h| *h == name).and_then(|i| row.get(i).copied())
}

/// Reads `udp.csv` (and `ping.csv`, if it is there) from a recording's folder. `qpc0`: the first frame's
/// stamp, which every time is counted from. Only the match server's datagrams are kept (flow 1; a file
/// from before flows were numbered has none of that column and is kept whole).
/// Ok(None): the folder has no network record.
pub fn read_folder(folder: &Path, qpc0: i64) -> std::io::Result<Option<NetData>> {
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_string());
    let udp = match std::fs::read_to_string(folder.join("udp.csv")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut net = NetData::default();
    let mut lines = udp.lines();
    let head: Vec<&str> = lines.next().unwrap_or("").trim().split(',').collect();
    for line in lines {
        let row: Vec<&str> = line.trim().split(',').collect();
        if row.len() < 3 || field(&head, &row, "flow").unwrap_or("1") != "1" {
            continue;
        }
        let qpc: i64 = field(&head, &row, "qpc").and_then(|v| v.parse().ok()).ok_or_else(|| bad("udp.csv holds a line without a time."))?;
        let bytes: u32 = field(&head, &row, "bytes").and_then(|v| v.parse().ok()).ok_or_else(|| bad("udp.csv holds a line without a size."))?;
        let out = match field(&head, &row, "dir") {
            Some("out") => true,
            Some("in") => false,
            _ => return Err(bad("udp.csv holds a line that is neither sent nor received.")),
        };
        net.packets.push(Packet { ticks: qpc - qpc0, out, bytes });
    }
    if let Ok(ping) = std::fs::read_to_string(folder.join("ping.csv")) {
        let mut lines = ping.lines();
        let head: Vec<&str> = lines.next().unwrap_or("").trim().split(',').collect();
        for line in lines {
            let row: Vec<&str> = line.trim().split(',').collect();
            if row.len() < 2 {
                continue;
            }
            let qpc: i64 = field(&head, &row, "qpc").and_then(|v| v.parse().ok()).ok_or_else(|| bad("ping.csv holds a line without a time."))?;
            let name = field(&head, &row, "hop").ok_or_else(|| bad("ping.csv holds a line without a router."))?;
            let hop = match net.hops.iter().position(|h| h == name) {
                Some(i) => i,
                None => {
                    if net.hops.len() >= MAX_HOPS {
                        return Err(bad("ping.csv names more routers than this tool keeps."));
                    }
                    net.hops.push(name.to_string());
                    net.hops.len() - 1
                }
            };
            let rtt_ms = match field(&head, &row, "rtt_ms").unwrap_or("") {
                "" => None,
                v => Some(v.parse::<f64>().ok().filter(|x| !x.is_nan()).ok_or_else(|| bad("ping.csv holds a reply time that is not a number."))?),
            };
            net.probes.push(Probe { ticks: qpc - qpc0, hop: hop as u8, rtt_ms });
        }
    }
    Ok(Some(net))
}

fn deltas(out: &mut Vec<u8>, ticks: impl Iterator<Item = i64>) {
    let mut prev = 0i64;
    for t in ticks {
        out.extend_from_slice(&t.wrapping_sub(prev).to_le_bytes());
        prev = t;
    }
}

pub fn pack(net: &NetData) -> std::io::Result<Vec<u8>> {
    let (p, q) = (net.packets.len(), net.probes.len());
    let mut body: Vec<u8> = Vec::with_capacity(p * 13 + q * 17);
    deltas(&mut body, net.packets.iter().map(|x| x.ticks));
    body.extend(net.packets.iter().map(|x| x.out as u8));
    net.packets.iter().for_each(|x| body.extend_from_slice(&x.bytes.to_le_bytes()));
    deltas(&mut body, net.probes.iter().map(|x| x.ticks));
    body.extend(net.probes.iter().map(|x| x.hop));
    net.probes.iter().for_each(|x| body.extend_from_slice(&x.rtt_ms.unwrap_or(f64::NAN).to_le_bytes()));
    let hj = serde_json::to_vec(&json!({"v": 1, "packets": p, "probes": q, "hops": net.hops}))?;
    let mut out = Vec::with_capacity(MAGIC.len() + 4 + hj.len() + body.len() / 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(hj.len() as u32).to_le_bytes());
    out.extend_from_slice(&hj);
    let mut z = ZlibEncoder::new(out, Compression::new(6));
    z.write_all(&body)?;
    z.finish()
}

const NOT_OURS: UnpackError = UnpackError("This is not network data from this tool, or it is from a newer version.");
const BAD_SIZE: UnpackError = UnpackError("This network data has a version or a size this tool does not accept.");
const CUT: UnpackError = UnpackError("This network data is cut short or longer than it says.");
const UNREADABLE: UnpackError = UnpackError("This network data could not be read.");

/// Gives back exactly what was packed. Nothing here trusts the sender: sizes are checked before memory
/// is taken, and no more is inflated than the header declares.
pub fn unpack(data: &[u8]) -> Result<NetData, UnpackError> {
    use std::io::Read;
    if !data.starts_with(MAGIC) {
        return Err(NOT_OURS);
    }
    let at = MAGIC.len();
    let hl = data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or(UNREADABLE)?;
    if hl > MAX_HEAD {
        return Err(BAD_SIZE);
    }
    let head: Value = data.get(at + 4..at + 4 + hl).and_then(|b| serde_json::from_slice(b).ok()).ok_or(UNREADABLE)?;
    let count = |k: &str| head.get(k).and_then(Value::as_u64).map(|n| n as usize);
    let (Some(p), Some(q)) = (count("packets"), count("probes")) else { return Err(UNREADABLE) };
    let hops: Vec<String> = match head.get("hops").and_then(Value::as_array) {
        Some(a) if a.iter().all(Value::is_string) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => return Err(UNREADABLE),
    };
    if head.get("v").and_then(Value::as_u64) != Some(1) || p > MAX_PACKETS || q > MAX_PROBES || hops.len() > MAX_HOPS {
        return Err(BAD_SIZE);
    }
    let want = p * 13 + q * 17;
    let mut raw = Vec::with_capacity(want);
    let mut z = flate2::read::ZlibDecoder::new(&data[at + 4 + hl..]);
    // one byte more than declared, so a stream that holds more is seen without inflating it all
    (&mut z).take(want as u64 + 1).read_to_end(&mut raw).map_err(|_| CUT)?;
    if raw.len() != want || z.total_in() as usize != data.len() - (at + 4 + hl) {
        return Err(CUT);
    }
    let i64s = |from: usize, n: usize| {
        let mut run = 0i64;
        raw[from..from + n * 8].chunks_exact(8).map(move |b| {
            run = run.wrapping_add(i64::from_le_bytes(b.try_into().expect("eight bytes")));
            run
        })
    };
    let mut net = NetData { packets: Vec::with_capacity(p), probes: Vec::with_capacity(q), hops };
    let (dir_at, bytes_at) = (p * 8, p * 9);
    for (i, ticks) in i64s(0, p).enumerate() {
        let out = match raw[dir_at + i] {
            0 => false,
            1 => true,
            _ => return Err(UNREADABLE),
        };
        let b = &raw[bytes_at + i * 4..bytes_at + i * 4 + 4];
        net.packets.push(Packet { ticks, out, bytes: u32::from_le_bytes([b[0], b[1], b[2], b[3]]) });
    }
    let base = p * 13;
    let (hop_at, rtt_at) = (base + q * 8, base + q * 9);
    for (i, ticks) in i64s(base, q).enumerate() {
        let hop = raw[hop_at + i];
        if hop as usize >= net.hops.len() {
            return Err(UNREADABLE);
        }
        let b = &raw[rtt_at + i * 8..rtt_at + i * 8 + 8];
        let rtt = f64::from_le_bytes(b.try_into().expect("eight bytes"));
        net.probes.push(Probe { ticks, hop, rtt_ms: if rtt.is_nan() { None } else { Some(rtt) } });
    }
    Ok(net)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NetData {
        NetData {
            packets: vec![
                Packet { ticks: -5_000_000, out: true, bytes: 75 },
                Packet { ticks: -4_999_000, out: false, bytes: 420 },
                Packet { ticks: 12, out: false, bytes: 1100 },
                Packet { ticks: 9_000_000_000, out: true, bytes: 60 },
            ],
            probes: vec![Probe { ticks: -100, hop: 0, rtt_ms: Some(0.707) }, Probe { ticks: 50, hop: 1, rtt_ms: None }, Probe { ticks: 70, hop: 0, rtt_ms: Some(12.5) }],
            hops: vec!["router".into(), "edge".into()],
        }
    }

    #[test]
    fn what_is_packed_comes_back() {
        let n = sample();
        let packed = pack(&n).unwrap();
        assert!(packed.starts_with(MAGIC));
        assert_eq!(unpack(&packed).unwrap(), n);
        assert_eq!(unpack(&pack(&NetData::default()).unwrap()).unwrap(), NetData::default());
    }

    #[test]
    fn what_is_malformed_is_refused() {
        let packed = pack(&sample()).unwrap();
        assert_eq!(unpack(b"FPSF1\nxxxx"), Err(NOT_OURS));
        assert_eq!(unpack(&packed[..packed.len() - 3]), Err(CUT));
        let mut longer = packed.clone();
        longer.extend_from_slice(b"more");
        assert_eq!(unpack(&longer), Err(CUT));
        // a header that declares more than the stream holds, and one that declares far too much
        let relabel = |from: &str, to: &str| {
            let hl = u32::from_le_bytes(packed[6..10].try_into().unwrap()) as usize;
            let head = String::from_utf8(packed[10..10 + hl].to_vec()).unwrap().replace(from, to);
            let mut out = MAGIC.to_vec();
            out.extend_from_slice(&(head.len() as u32).to_le_bytes());
            out.extend_from_slice(head.as_bytes());
            out.extend_from_slice(&packed[10 + hl..]);
            out
        };
        assert_eq!(unpack(&relabel("\"packets\":4", "\"packets\":5")), Err(CUT));
        assert_eq!(unpack(&relabel("\"packets\":4", "\"packets\":400000000")), Err(BAD_SIZE));
        assert_eq!(unpack(&relabel("\"v\":1", "\"v\":2")), Err(BAD_SIZE));
        assert_eq!(unpack(&relabel("[\"router\",\"edge\"]", "[\"router\"]")), Err(UNREADABLE));
    }

    #[test]
    fn a_folder_is_read_as_the_python_reads_it() {
        let dir = std::env::temp_dir().join(format!("fpsn-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("udp.csv"), "qpc,dir,bytes,flow\r\n100,out,75,1\r\n200,out,60,2\r\n300,in,420,1\r\n").unwrap();
        std::fs::write(dir.join("ping.csv"), "qpc,hop,rtt_ms\n120,edge,\n150,router,0.707\n160,edge,3.250\n").unwrap();
        let n = read_folder(&dir, 250).unwrap().unwrap();
        assert_eq!(n.packets, vec![Packet { ticks: -150, out: true, bytes: 75 }, Packet { ticks: 50, out: false, bytes: 420 }]);
        assert_eq!(n.hops, vec!["edge".to_string(), "router".to_string()]);
        assert_eq!(n.probes, vec![Probe { ticks: -130, hop: 0, rtt_ms: None }, Probe { ticks: -100, hop: 1, rtt_ms: Some(0.707) }, Probe { ticks: -90, hop: 0, rtt_ms: Some(3.25) }]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(read_folder(&dir, 0).unwrap().is_none());
    }
}
