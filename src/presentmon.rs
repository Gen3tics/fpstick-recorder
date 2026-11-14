//! Intel's PresentMon does the frame recording, unchanged. This builds the exact command for it.

use std::path::Path;

/// The name of the Windows trace session, so a recording can be told to stop from outside.
pub const SESSION: &str = "recorder";

/// The command that records. `seconds` is None for "until told to stop".
pub fn record_args(process: &str, out: &Path, seconds: Option<u64>) -> Vec<String> {
    let mut a: Vec<String> = ["--process_name", process, "--qpc_time", "--track_pc_latency", "--session_name", SESSION, "--stop_existing_session", "--no_console_stats", "--terminate_on_proc_exit"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(s) = seconds {
        a.extend(["--timed".to_string(), s.to_string(), "--terminate_after_timed".to_string()]);
    }
    a.extend(["--output_file".to_string(), out.display().to_string()]);
    a
}

/// The command that ends a running recording: PresentMon stops the named session and the recording one exits.
pub fn stop_args() -> Vec<String> {
    ["--session_name", SESSION, "--terminate_existing_session"].iter().map(|s| s.to_string()).collect()
}

/// The command as one line, as it would be typed.
pub fn shown(exe: &str, args: &[String]) -> String {
    let q = |s: &str| if s.contains(' ') { format!("\"{}\"", s) } else { s.to_string() };
    std::iter::once(q(exe)).chain(args.iter().map(|a| q(a))).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_what_the_window_shows() {
        let a = record_args("Overwatch.exe", Path::new("C:\\rec\\frames.csv"), Some(120));
        assert_eq!(shown("PresentMon.exe", &a), "PresentMon.exe --process_name Overwatch.exe --qpc_time --track_pc_latency --session_name recorder --stop_existing_session --no_console_stats --terminate_on_proc_exit --timed 120 --terminate_after_timed --output_file C:\\rec\\frames.csv");
        let whole = record_args("Overwatch.exe", Path::new("C:\\my recordings\\frames.csv"), None);
        assert!(!whole.contains(&"--timed".to_string()));
        assert!(shown("C:\\Program Files\\PM\\PresentMon.exe", &whole).starts_with("\"C:\\Program Files\\PM\\PresentMon.exe\" --process_name"));
        assert!(shown("p", &whole).ends_with("\"C:\\my recordings\\frames.csv\""));
        assert_eq!(stop_args(), ["--session_name", "recorder", "--terminate_existing_session"]);
    }
}
