//! The few things the clipboard library can't tell us, asked of the operating system directly.
//! On Windows: a cheap "has the clipboard changed?" counter, which app put the copy there, and the
//! markers password managers add to say "don't record this". Elsewhere these return nothing and
//! ClipNest falls back to comparing what's on the clipboard.

/// A number that changes every time anything is copied, if the system has one.
pub fn change_count() -> Option<u32> {
    #[cfg(any(windows, clipnest_check_win))]
    {
        // SAFETY: no arguments, no pointers.
        return Some(unsafe { win::GetClipboardSequenceNumber() });
    }
    #[allow(unreachable_code)]
    None
}

/// The program that owns the clipboard right now ("chrome.exe"), if the system says.
pub fn source_app() -> Option<String> {
    #[cfg(any(windows, clipnest_check_win))]
    {
        return win::owner_exe();
    }
    #[allow(unreachable_code)]
    None
}

/// Password managers mark copies they don't want recorded; true when the current copy has a mark.
pub fn copy_asks_not_to_be_recorded() -> bool {
    #[cfg(any(windows, clipnest_check_win))]
    {
        return win::has_ignore_marker();
    }
    #[allow(unreachable_code)]
    false
}

/// Whether a program is on the "ignore copies from" list (matched by file name, any case, with or
/// without ".exe").
pub fn is_excluded(source: Option<&str>, excluded: &[String]) -> bool {
    let Some(src) = source else { return false };
    let name = src.rsplit(['\\', '/']).next().unwrap_or(src).to_lowercase();
    let bare = name.trim_end_matches(".exe");
    excluded.iter().any(|e| {
        let e = e.trim().to_lowercase();
        let eb = e.trim_end_matches(".exe");
        !eb.is_empty() && (eb == bare)
    })
}

#[cfg(any(windows, clipnest_check_win))]
#[allow(non_snake_case)]
mod win {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;
    type Handle = *mut c_void;

    #[link(name = "user32")]
    extern "system" {
        pub fn GetClipboardSequenceNumber() -> u32;
        fn GetClipboardOwner() -> Hwnd;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn QueryFullProcessImageNameW(process: Handle, flags: u32, name: *mut u16, size: *mut u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn owner_exe() -> Option<String> {
        // SAFETY: plain Win32 calls; every pointer passed points at a live local buffer.
        unsafe {
            let hwnd = GetClipboardOwner();
            if hwnd.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut buf = vec![0u16; 1024];
            let mut len: u32 = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(process);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit('\\').next().map(|s| s.to_string())
        }
    }

    pub fn has_ignore_marker() -> bool {
        // Formats KeePass, 1Password, Bitwarden and others set on copies of passwords.
        for name in ["ExcludeClipboardContentFromMonitorProcessing", "Clipboard Viewer Ignore"] {
            let w = wide(name);
            // SAFETY: `w` is a NUL-terminated UTF-16 string that outlives the call.
            unsafe {
                let format = RegisterClipboardFormatW(w.as_ptr());
                if format != 0 && IsClipboardFormatAvailable(format) != 0 {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions() {
        let list = vec!["KeePassXC.exe".to_string(), "1password".to_string(), " ".to_string()];
        assert!(is_excluded(Some("keepassxc.exe"), &list));
        assert!(is_excluded(Some(r"C:\Program Files\KeePassXC\KeePassXC.exe"), &list));
        assert!(is_excluded(Some("1Password.exe"), &list));
        assert!(!is_excluded(Some("chrome.exe"), &list));
        assert!(!is_excluded(None, &list));
        assert!(!is_excluded(Some(".exe"), &list));
    }
}
