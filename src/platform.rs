//! Everything that talks to Windows, in one place. On other systems the same functions exist and do
//! nothing, so the rest of the program can be built and tested anywhere.

use std::path::PathBuf;

pub fn appdata_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Recorder")
}

/// The person's Documents folder. Windows is asked, because the folder can be moved (to OneDrive, to another
/// drive); if it will not say, the usual place under the user's folder is taken.
pub fn documents_dir() -> PathBuf {
    known_documents().unwrap_or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")).unwrap_or_else(std::env::temp_dir))
}

/// Where recordings are kept until the person deletes them.
pub fn recordings_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")).unwrap_or_else(std::env::temp_dir).join("Recorder")
}

/// Intel's PresentMon, where its installer or a download usually leaves it. The newest version wins.
pub fn find_presentmon() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let dirs = [
        var("ProgramFiles").map(|p| p.join("Intel").join("PresentMon").join("PresentMonConsoleApplication")),
        var("ProgramFiles").map(|p| p.join("PresentMon")),
        var("USERPROFILE").map(|p| p.join("Downloads")),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())),
    ];
    for dir in dirs.into_iter().flatten() {
        let mut hits: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
                n.starts_with("presentmon-") && n.ends_with("-x64.exe")
            })
            .collect();
        hits.sort();
        if let Some(p) = hits.pop() {
            return Some(p);
        }
    }
    None
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW, ENUM_CURRENT_SETTINGS};
    use windows_sys::Win32::System::Diagnostics::Debug::Beep;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, WaitForSingleObject, THREAD_PRIORITY_LOWEST};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn text(buf: &[u16]) -> String {
        String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())]).trim().to_string()
    }

    pub fn is_elevated() -> bool {
        unsafe { IsUserAnAdmin() != 0 }
    }

    /// Starts this program again with administrator rights. Windows shows its permission prompt; false if it was declined.
    pub fn relaunch_elevated(args: &str) -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        let (verb, file, params) = (wide("runas"), wide(&exe.display().to_string()), wide(args));
        unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), std::ptr::null(), 1) as isize > 32 }
    }

    pub fn open_url(url: &str) {
        let (verb, file) = (wide("open"), wide(url));
        unsafe {
            ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1);
        }
    }

    /// Whether a key or mouse button is down right now. This asks Windows for the key's state 10 or 40
    /// times a second. The other ways of hearing a key cost the game more: a hook sits in the path of
    /// every mouse and keyboard event, and raw input wakes the listener for every mouse report, thousands
    /// a second from a fast mouse.
    pub fn key_down(vk: u16) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
    }

    pub fn beep(freq: u32, ms: u32) {
        unsafe {
            Beep(freq, ms);
        }
    }

    pub fn cpu_name() -> Option<String> {
        let (key, val) = (wide("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0"), wide("ProcessorNameString"));
        let mut buf = [0u16; 256];
        let mut size = (buf.len() * 2) as u32;
        let rc = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), val.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut size) };
        (rc == 0).then(|| text(&buf)).filter(|s| !s.is_empty())
    }

    /// The graphics card the main screen is plugged into.
    pub fn gpu_name() -> Option<String> {
        let mut i = 0;
        loop {
            let mut d: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
            d.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut d, 0) } == 0 {
                return None;
            }
            if d.StateFlags & 0x4 != 0 {
                // DISPLAY_DEVICE_PRIMARY_DEVICE
                return Some(text(&d.DeviceString)).filter(|s| !s.is_empty());
            }
            i += 1;
        }
    }

    /// The main screen's mode, such as "2560 x 1440 at 360 Hz". The game may render at another size.
    pub fn display() -> Option<String> {
        let mut m: DEVMODEW = unsafe { std::mem::zeroed() };
        m.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if unsafe { EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut m) } == 0 {
            return None;
        }
        Some(format!("{} x {} at {} Hz", m.dmPelsWidth, m.dmPelsHeight, m.dmDisplayFrequency))
    }

    pub fn process_running(name: &str) -> bool {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = false;
            if Process32FirstW(snap, &mut e) != 0 {
                loop {
                    if text(&e.szExeFile).eq_ignore_ascii_case(name) {
                        found = true;
                        break;
                    }
                    if Process32NextW(snap, &mut e) == 0 {
                        break;
                    }
                }
            }
            windows_sys::Win32::Foundation::CloseHandle(snap);
            found
        }
    }

    /// Local date and time as "2026-11-13_173005" and "2026-11-13T17:30:05".
    pub fn now() -> (String, String) {
        let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut t) };
        (
            format!("{:04}-{:02}-{:02}_{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond),
            format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond),
        )
    }

    pub fn hidden(cmd: &mut std::process::Command) {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    pub fn known_documents() -> Option<std::path::PathBuf> {
        use windows_sys::Win32::System::Com::CoTaskMemFree;
        use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath};
        let mut p: *mut u16 = std::ptr::null_mut();
        let ok = unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut p) } == 0 && !p.is_null();
        let out = ok.then(|| unsafe {
            let len = (0..32768).take_while(|i| *p.add(*i) != 0).count();
            std::path::PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)))
        });
        unsafe { CoTaskMemFree(p as *const _) };
        out
    }

    /// Sleeps up to `ms`, or until the program has ended, whichever comes first; true if it has ended.
    /// The thread is asleep in Windows the whole time: nothing is asked or checked while it waits.
    pub fn wait_exit(child: &mut std::process::Child, ms: u32) -> bool {
        use std::os::windows::io::AsRawHandle;
        unsafe { WaitForSingleObject(child.as_raw_handle() as _, ms) == 0 } // 0 = WAIT_OBJECT_0
    }

    /// Puts the calling thread below ordinary programs. Windows runs a thread only when no thread of a
    /// higher priority wants the processor, so from here on this thread never takes one from the game.
    pub fn yield_to_the_game() {
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_elevated() -> bool {
        false
    }
    pub fn relaunch_elevated(_args: &str) -> bool {
        false
    }
    pub fn open_url(_url: &str) {}
    pub fn key_down(_vk: u16) -> bool {
        false
    }
    pub fn beep(_freq: u32, _ms: u32) {}
    pub fn cpu_name() -> Option<String> {
        None
    }
    pub fn gpu_name() -> Option<String> {
        None
    }
    pub fn display() -> Option<String> {
        None
    }
    pub fn process_running(_name: &str) -> bool {
        false
    }
    pub fn now() -> (String, String) {
        let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        (format!("t{}", s), format!("t{}", s))
    }
    pub fn hidden(_cmd: &mut std::process::Command) {}
    pub fn known_documents() -> Option<std::path::PathBuf> {
        None
    }
    pub fn wait_exit(child: &mut std::process::Child, ms: u32) -> bool {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
        matches!(child.try_wait(), Ok(Some(_)))
    }
    pub fn yield_to_the_game() {}
}

pub use imp::*;

/// The first key or mouse button found down that may serve as a stop key.
pub fn pressed_key() -> Option<u16> {
    (1u16..=0xFE).find(|vk| crate::config::key_allowed(*vk) && key_down(*vk))
}

/// Two rising tones: the recording has started.
pub fn sound_start() {
    beep(660, 110);
    beep(990, 160);
}

/// Two falling tones: the recording has stopped.
pub fn sound_stop() {
    beep(990, 110);
    beep(660, 160);
}
