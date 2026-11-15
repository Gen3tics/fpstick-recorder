//! Sends a packed recording. By default it goes to FPStick's run-page server (https://fpstick.com): one upload
//! holding what was chosen, what was read from the PC, the frame timing and, if the network was recorded, when
//! each of the game's packets passed and how large it was. No address is in any of it.
//!
//! Another server can be named. Anywhere on the internet it must be reached over https; plain http is taken
//! only for a server on the same PC or the same home network, where nobody else is on the way.
//! "this PC" (or 127.0.0.1:8765) means a run-page program running on this PC; then nothing leaves it.

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

/// FPStick's own run-page server, where a recording goes unless another address is typed in.
pub const HOME: &str = "https://fpstick.com";

/// Where a recording is sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    /// A run-page program on this same PC (127.0.0.1:8765). Nothing leaves the PC.
    ThisPc,
    /// A run-page server: its address without a path, such as "https://fpstick.com" or "http://192.168.1.20:8770".
    Server(String),
}

impl Target {
    /// Whether the recording crosses a network unprotected: plain http, which is only taken for a home network.
    pub fn plain(&self) -> bool {
        matches!(self, Target::Server(base) if base.starts_with("http://"))
    }
}

/// Whether a host is this PC or an address of a home network (10.x, 172.16 to 172.31, 192.168.x, 169.254.x, a ".local" name).
/// Only such a host is sent to over plain http.
fn is_home(host: &str) -> bool {
    let name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    if name.eq_ignore_ascii_case("localhost") || name.to_ascii_lowercase().ends_with(".local") {
        return true; // ".local" names are given out inside a home network only
    }
    match name.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Err(_) => false,
    }
}

/// Where a typed-in address points. Nothing typed: FPStick's own server. A name alone ("example.org") is
/// reached over https; an address of the home network alone ("192.168.1.20:8770") over plain http.
pub fn target(typed: &str) -> Result<Target, String> {
    let t = typed.trim().trim_end_matches('/');
    if t.is_empty() {
        return Ok(Target::Server(HOME.into()));
    }
    let low = t.to_ascii_lowercase();
    let (scheme, host) = match (low.strip_prefix("https://"), low.strip_prefix("http://")) {
        (Some(h), _) => (Some("https"), h),
        (_, Some(h)) => (Some("http"), h),
        _ => (None, low.as_str()),
    };
    if host == "this pc" || host == LOCAL || host == "localhost:8765" {
        return Ok(Target::ThisPc);
    }
    let port_ok = |h: &str| h.rsplit_once(':').map(|(n, p)| !n.is_empty() && p.parse::<u16>().is_ok()).unwrap_or(true);
    if host.is_empty() || !port_ok(host) || !host.chars().all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c)) {
        return Err("That does not look like an address. It should read like https://fpstick.com, or http://192.168.1.20:8770 for a server at home.".into());
    }
    match (scheme, is_home(host)) {
        (Some("http"), false) => Err("Over plain http a recording is only sent to a server on your own network. For any other, the address must begin with https://".into()),
        (Some("https"), _) | (None, false) => Ok(Target::Server(format!("https://{}", host))),
        _ => Ok(Target::Server(format!("http://{}", host))),
    }
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

/// Sends one recording to the run-page server at `base` ("https://fpstick.com", "http://192.168.1.20:8770").
/// Over https the server's certificate is checked against Mozilla's list of certificate authorities, which is
/// built into this program; a server that cannot show a valid one is not sent to.
pub fn send_server(base: &str, packed: &[u8], info: &serde_json::Value, net: Option<&[u8]>) -> Result<Kept, String> {
    let body = upload_body(info, packed, net);
    // a whole match is about 20 MB: ten minutes covers a slow line out and the server's judging
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(600)))
        .http_status_as_error(false) // a refusal comes with words of its own, which are passed on
        .max_redirects(0) // a recording goes to the address that was named and nowhere else
        .user_agent(concat!("FPStick-Recorder/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut reply = agent.post(format!("{}/api/upload", base)).header("Content-Type", "application/octet-stream").send(&body[..]).map_err(|e| said(&e, base))?;
    let status = reply.status().as_u16();
    let text = reply.body_mut().with_config().limit(64 * 1024).read_to_string().unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap_or(serde_json::Value::Null);
    if status != 200 {
        // the server says why in plain words; something on the way to it (a proxy's own page) does not
        return Err(match (v["error"].as_str(), status) {
            (Some(why), _) => why.to_string(),
            (None, 413) => "The recording is too large to send.".to_string(),
            (None, 300..=399) => format!("{} sent the recording on to another address, and it was not followed.", base),
            (None, 500..=599) => format!("The server at {} could not be reached or is not answering right now (answer {}). Try again in a moment; the recording is kept on this PC.", base, status),
            (None, _) => format!("{} refused the recording (answer {}).", base, status),
        });
    }
    let (path, key) = (v["url"].as_str().ok_or("The answer had no run page in it.")?, v["delete_key"].as_str().unwrap_or(""));
    if !path.starts_with("/r/") || path.len() > 64 || !path[3..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("The answer had no run page in it.".into());
    }
    Ok(Kept { page: format!("{}{}", base, path), delete_key: key.to_string() })
}

/// What went wrong on the way to the server, in plain words.
fn said(e: &ureq::Error, base: &str) -> String {
    use ureq::Error as E;
    match e {
        E::HostNotFound => format!("The address {} could not be found. Check the spelling and that this PC is online.", base),
        E::ConnectionFailed => NO_ANSWER.to_string(),
        E::Timeout(_) => format!("{} took too long. The recording is kept on this PC; try again.", base),
        E::Tls(_) | E::Rustls(_) | E::TlsRequired => format!("The connection to {} could not be secured ({}), so nothing was sent.", base, e),
        E::Io(io) if matches!(io.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::TimedOut) => NO_ANSWER.to_string(),
        // a certificate that does not hold arrives as an input/output failure with the reason inside (seen on Windows, 15 Nov 2026)
        E::Io(io) if ["certificate", "handshake", "tls"].iter().any(|w| io.to_string().to_ascii_lowercase().contains(w)) => {
            format!("The connection to {} could not be secured ({}), so nothing was sent.", base, io)
        }
        E::Io(io) => format!("{} could not be reached ({}). Check the address and that this PC is online.", base, io),
        _ => format!("Sending failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn addresses_and_screens_are_read() {
        use super::{screen, target, Target};
        let server = |a: &str| Ok(Target::Server(a.to_string()));
        // nothing typed: FPStick's own server, over https
        assert_eq!(target("  "), server("https://fpstick.com"));
        assert_eq!(target("https://fpstick.com/"), server("https://fpstick.com"));
        assert_eq!(target("HTTPS://Example.org:8443"), server("https://example.org:8443"));
        // a name alone is reached over https; a home address alone over plain http
        assert_eq!(target("example.org"), server("https://example.org"));
        assert_eq!(target("192.168.1.20:8770"), server("http://192.168.1.20:8770"));
        assert_eq!(target("http://192.168.1.20:8770/"), server("http://192.168.1.20:8770"));
        assert_eq!(target("http://10.0.0.5"), server("http://10.0.0.5"));
        assert_eq!(target("pi.local:8770"), server("http://pi.local:8770"));
        assert_eq!(target("https://192.168.1.20"), server("https://192.168.1.20"));
        assert!(target("http://192.168.1.20:8770").unwrap().plain() && !target("").unwrap().plain() && !Target::ThisPc.plain());
        // plain http never leaves the home network
        for away in ["http://example.org", "http://fpstick.com", "http://8.8.8.8:80", "http://172.32.0.1", "http://192.169.1.1"] {
            assert!(target(away).unwrap_err().contains("https://"), "{}", away);
        }
        // the run-page program on this same PC
        for here in ["this pc", "This PC", "127.0.0.1:8765", "http://127.0.0.1:8765", "localhost:8765"] {
            assert_eq!(target(here), Ok(Target::ThisPc), "{}", here);
        }
        assert_eq!(target("127.0.0.1:8770"), server("http://127.0.0.1:8770"));
        for odd in ["http://a b", "https://a/b", "https://", "a:b", "exa mple.org", "https://user@example.org", "example.org:99999", "x?y"] {
            assert!(target(odd).is_err(), "{}", odd);
        }
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
