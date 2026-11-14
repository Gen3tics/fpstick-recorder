//! Sends a packed recording. By default it goes to an FPStick run-page program running on this PC
//! (http://127.0.0.1:8765) and never leaves it. If an address is set, it goes to a run-page server there:
//! one upload holding what was typed in, what was read from the PC, the frame timing and, if the network
//! was recorded, when each of the game's packets passed and how large it was. No address is in any of it.
//!
//! Plain http only so far, which is right for a server on the home network and nowhere else.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const LOCAL: &str = "127.0.0.1:8765";

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

fn post(path: &str, content_type: &str, body: &[u8], wait: Duration) -> Result<serde_json::Value, String> {
    post_to(LOCAL, path, content_type, body, wait).map_err(|e| if e == NO_ANSWER { "No run-page program answered on this PC (127.0.0.1:8765). Start it and send again, or type a server's address into \"Send to\".".to_string() } else { e })
}

const NO_ANSWER: &str = "Nothing answered at that address.";

fn post_to(host: &str, path: &str, content_type: &str, body: &[u8], wait: Duration) -> Result<serde_json::Value, String> {
    let addr = host.to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or_else(|| format!("The address {} could not be found.", host))?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(4)).map_err(|_| NO_ANSWER.to_string())?;
    let _ = s.set_read_timeout(Some(wait));
    let head = format!("POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", path, host, content_type, body.len());
    s.write_all(head.as_bytes()).and_then(|_| s.write_all(body)).map_err(|e| format!("Sending failed: {}", e))?;
    let mut reply = Vec::new();
    s.read_to_end(&mut reply).map_err(|e| format!("No answer: {}", e))?;
    let text = String::from_utf8_lossy(&reply);
    let (top, rest) = text.split_once("\r\n\r\n").ok_or("The answer could not be read.")?;
    let v: serde_json::Value = serde_json::from_str(rest.trim()).map_err(|_| "The answer could not be read.".to_string())?;
    if top.starts_with("HTTP/1.0 200") || top.starts_with("HTTP/1.1 200") {
        Ok(v)
    } else {
        Err(v["error"].as_str().unwrap_or("The run was refused.").to_string())
    }
}

/// Sends the packed frame data and what was typed in. Returns the address of the run page.
pub fn send_local(packed: &[u8], kept_at: &str, label: &str, map: &str, mode: &str, frame_limit: &str, with_network: bool) -> Result<String, String> {
    let path = format!("/api/runs?name=frames.fpsf&label={}&source_path={}", enc(label), enc(kept_at));
    let v = post(&path, "application/octet-stream", packed, Duration::from_secs(180))?;
    let id = v["id"].as_i64().ok_or("The answer had no run in it.")?;
    let mut fields = serde_json::Map::new();
    for (k, val) in [("map", map), ("mode", mode)] {
        if !val.trim().is_empty() {
            fields.insert(k.into(), val.trim().into());
        }
    }
    if let Ok(n) = frame_limit.trim().parse::<u32>() {
        fields.insert("target_fps".into(), n.into());
    }
    if !fields.is_empty() {
        let _ = post(&format!("/api/runs/{}", id), "application/json", &serde_json::to_vec(&fields).unwrap_or_default(), Duration::from_secs(60));
    }
    if with_network {
        // the program on this PC reads udp.csv and ping.csv from the recording's folder, beside the packed frames
        post(&format!("/api/runs/{}/matchnet", id), "application/json", b"{}", Duration::from_secs(300))?;
    }
    Ok(format!("http://{}/#/run/{}", LOCAL, id))
}

/// Where a typed-in address points: None for the run-page program on this PC, otherwise "host:port".
pub fn target(typed: &str) -> Result<Option<String>, String> {
    let t = typed.trim().trim_end_matches('/');
    if t.is_empty() {
        return Ok(None);
    }
    if t.to_ascii_lowercase().starts_with("https://") {
        return Err("This version can only send over plain http, to a server on your own network.".into());
    }
    let host = t.strip_prefix("http://").unwrap_or(t);
    if host.is_empty() || host.contains(|c: char| c == '/' || c.is_whitespace()) {
        return Err("That does not look like an address. It should read like http://192.168.1.20:8770".into());
    }
    Ok(Some(if host.contains(':') { host.to_string() } else { format!("{}:80", host) }))
}

/// The screen as the recorder reads it ("2560 x 1440 at 360 Hz") cut into resolution and refresh rate.
pub fn screen(display: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(d) = display else { return (None, None) };
    match d.split_once(" at ") {
        Some((res, hz)) => (Some(res.trim().to_string()), Some(hz.trim().trim_end_matches("Hz").trim().to_string()).filter(|h| !h.is_empty())),
        None => (Some(d.trim().to_string()), None),
    }
}

/// The network timing recorded beside a packed recording, packed for sending: `udp.csv` and `ping.csv` from
/// the same folder, every time counted from the first frame's stamp (read from the packed frames' own header).
/// None: there are no network files, or the frames carry no stamp to count from.
pub fn network_block(packed_file: &std::path::Path, packed: &[u8]) -> Option<Vec<u8>> {
    let qpc0 = crate::pack::meta_of(packed)?.get("qpc0")?.as_f64().filter(|q| q.fract() == 0.0 && q.abs() < 9.0e15)?;
    let net = framedata::net::read_folder(packed_file.parent()?, qpc0 as i64).ok()??;
    if net.packets.is_empty() {
        return None;
    }
    framedata::net::pack(&net).ok()
}

/// One upload: the bytes "FPSU1\n", a 4-byte length, that many bytes of JSON about the run, then the packed
/// frame data, then the packed network timing if there is any (the JSON's `net_bytes` says how long it is).
pub fn upload_body(info: &serde_json::Value, packed: &[u8], net: Option<&[u8]>) -> Vec<u8> {
    let mut info = info.clone();
    if let (Some(n), Some(o)) = (net, info.as_object_mut()) {
        o.insert("net_bytes".into(), n.len().into());
    }
    let j = serde_json::to_vec(&info).unwrap_or_else(|_| b"{}".to_vec());
    let net = net.unwrap_or(&[]);
    let mut out = Vec::with_capacity(10 + j.len() + packed.len() + net.len());
    out.extend_from_slice(b"FPSU1\n");
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(&j);
    out.extend_from_slice(packed);
    out.extend_from_slice(net);
    out
}

pub struct Kept {
    /// The run page's address.
    pub page: String,
    /// The key that deletes the run. It is given to the browser after the "#" of the address, which is never sent to a server.
    pub delete_key: String,
}

/// Sends one recording to a run-page server at `host` ("192.168.1.20:8770").
pub fn send_server(host: &str, packed: &[u8], info: &serde_json::Value, net: Option<&[u8]>) -> Result<Kept, String> {
    let v = post_to(host, "/api/upload", "application/octet-stream", &upload_body(info, packed, net), Duration::from_secs(120))?;
    let (path, key) = (v["url"].as_str().ok_or("The answer had no run page in it.")?, v["delete_key"].as_str().unwrap_or(""));
    if !path.starts_with("/r/") {
        return Err("The answer had no run page in it.".into());
    }
    Ok(Kept { page: format!("http://{}{}", host, path), delete_key: key.to_string() })
}

#[cfg(test)]
mod tests {
    #[test]
    fn addresses_and_screens_are_read() {
        use super::{screen, target};
        assert_eq!(target("  "), Ok(None));
        assert_eq!(target("http://192.168.1.20:8770/"), Ok(Some("192.168.1.20:8770".into())));
        assert_eq!(target("pi.local"), Ok(Some("pi.local:80".into())));
        assert!(target("https://example.org").is_err() && target("http://a b").is_err() && target("http://a/b").is_err());
        assert_eq!(screen(Some("2560 x 1440 at 360 Hz")), (Some("2560 x 1440".into()), Some("360".into())));
        assert_eq!(screen(None), (None, None));
    }

    #[test]
    fn an_upload_is_the_info_then_the_frame_data() {
        let b = super::upload_body(&serde_json::json!({"declared": {"map": "Ilios"}}), b"FPSF1\nxyz", None);
        assert!(b.starts_with(b"FPSU1\n"));
        let n = u32::from_le_bytes(b[6..10].try_into().unwrap()) as usize;
        assert_eq!(&b[10..10 + n], br#"{"declared":{"map":"Ilios"}}"#);
        assert_eq!(&b[10 + n..], b"FPSF1\nxyz");
    }

    #[test]
    fn the_network_timing_rides_at_the_end_and_the_info_says_how_long_it_is() {
        let b = super::upload_body(&serde_json::json!({"declared": {}}), b"FPSF1\nxyz", Some(b"FPSN1\nabcd"));
        let n = u32::from_le_bytes(b[6..10].try_into().unwrap()) as usize;
        let info: serde_json::Value = serde_json::from_slice(&b[10..10 + n]).unwrap();
        assert_eq!(info["net_bytes"], 10);
        assert_eq!(&b[10 + n..], b"FPSF1\nxyzFPSN1\nabcd");
    }

    #[test]
    fn the_network_block_is_made_from_the_files_beside_the_packed_frames() {
        let dir = std::env::temp_dir().join(format!("recorder-netblock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let head = br#"{"v":1,"meta":{"qpc0":250.0,"qpc_freq":10000000.0},"notes":[],"cols":[],"rows":0}"#;
        let mut packed = b"FPSF1\n".to_vec();
        packed.extend_from_slice(&(head.len() as u32).to_le_bytes());
        packed.extend_from_slice(head);
        let file = dir.join("frames.fpsf");
        assert!(super::network_block(&file, &packed).is_none(), "no network files, no block");
        std::fs::write(dir.join("udp.csv"), "qpc,dir,bytes,flow\r\n100,out,75,1\r\n200,out,60,2\r\n300,in,420,1\r\n").unwrap();
        std::fs::write(dir.join("ping.csv"), "qpc,hop,rtt_ms\n120,edge,\n150,router,0.707\n").unwrap();
        let net = framedata::net::unpack(&super::network_block(&file, &packed).unwrap()).unwrap();
        assert_eq!(net.packets.iter().map(|p| (p.ticks, p.out, p.bytes)).collect::<Vec<_>>(), vec![(-150, true, 75), (50, false, 420)], "the match server's packets only, counted from the first frame");
        assert_eq!(net.hops, vec!["edge".to_string(), "router".to_string()]);
        assert_eq!(net.probes.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn query_text_is_escaped() {
        assert_eq!(super::enc("King's Row, C:\\a b"), "King%27s%20Row%2C%20C%3A%5Ca%20b");
    }
}
