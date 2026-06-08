//! Определение активного окна/процесса — нативно, платформозависимо.
//! macOS: NSWorkspace.sharedWorkspace.frontmostApplication.localizedName
//! Windows: GetForegroundWindow → GetModuleFileNameExW

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

pub fn spawn(tx: Sender<Option<String>>) {
    thread::spawn(move || {
        let mut last: Option<String> = None;
        loop {
            let current = get_active_process_name();
            if current != last {
                crate::debug::log(&format!(
                    "window_tracker: активное окно → {:?}",
                    current
                ));
                last = current.clone();
            }
            let _ = tx.send(current);
            thread::sleep(Duration::from_millis(300));
        }
    });
}

#[cfg(target_os = "macos")]
pub fn get_active_process_name() -> Option<String> {
    use core_foundation::array::CFArrayRef;
    use core_foundation::base::{CFTypeRef, TCFType};
    use core_foundation::dictionary::CFDictionaryRef;
    use core_foundation::number::CFNumberRef;
    use core_foundation::string::{CFString, CFStringRef};
    use std::os::raw::{c_int, c_void};

    type CFIndex = isize;

    #[allow(non_upper_case_globals)]
    const kCGWindowListOptionOnScreenOnly: u32 = 1;
    #[allow(non_upper_case_globals)]
    const kCGWindowListExcludeDesktopElements: u32 = 16;
    #[allow(non_upper_case_globals)]
    const kCGNullWindowID: u32 = 0;
    #[allow(non_upper_case_globals)]
    const kCFNumberSInt32Type: c_int = 3;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFArrayRef;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(arr: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: CFIndex) -> *const c_void;
        fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
        fn CFStringGetCString(
            s: CFStringRef,
            buf: *mut u8,
            buf_size: CFIndex,
            encoding: u32,
        ) -> bool;
        fn CFRelease(cf: CFTypeRef);
        fn CFNumberGetValue(num: CFNumberRef, typ: c_int, out: *mut c_void) -> bool;
    }

    // UTF-8 encoding
    const KCF_STRING_ENCODING_UTF8: u32 = 0x08000100;

    unsafe fn cf_str_to_rust(s: CFStringRef) -> Option<String> {
        let mut buf = [0u8; 256];
        if CFStringGetCString(s, buf.as_mut_ptr(), 256, KCF_STRING_ENCODING_UTF8) {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            Some(String::from_utf8_lossy(&buf[..end]).into_owned())
        } else {
            None
        }
    }

    unsafe {
        let list = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        );
        if list.is_null() {
            return None;
        }

        let layer_key = CFString::new("kCGWindowLayer");
        let owner_key = CFString::new("kCGWindowOwnerName");
        let count = CFArrayGetCount(list);
        let mut result = None;

        'outer: for i in 0..count {
            let dict = CFArrayGetValueAtIndex(list, i) as CFDictionaryRef;
            if dict.is_null() {
                continue;
            }

            // Проверяем layer == 0 (обычные окна приложений).
            let layer_val = CFDictionaryGetValue(dict, layer_key.as_concrete_TypeRef() as *const c_void);
            if layer_val.is_null() {
                continue;
            }
            let mut layer: i32 = -1;
            CFNumberGetValue(layer_val as CFNumberRef, kCFNumberSInt32Type, &mut layer as *mut i32 as *mut c_void);
            if layer != 0 {
                continue 'outer;
            }

            // Берём имя владельца окна.
            let name_val = CFDictionaryGetValue(dict, owner_key.as_concrete_TypeRef() as *const c_void);
            if name_val.is_null() {
                continue;
            }
            if let Some(name) = cf_str_to_rust(name_val as CFStringRef) {
                result = Some(name);
                break;
            }
        }

        CFRelease(list as CFTypeRef);
        result
    }
}

#[cfg(target_os = "windows")]
pub fn get_active_process_name() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::psapi::GetModuleFileNameExW;
    use winapi::um::winnt::{PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    use winapi::um::winuser::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 260];
        let len = GetModuleFileNameExW(handle, std::ptr::null_mut(), buf.as_mut_ptr(), 260);
        CloseHandle(handle);
        if len == 0 {
            return None;
        }
        let path = OsString::from_wide(&buf[..len as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn get_active_process_name() -> Option<String> {
    None
}
