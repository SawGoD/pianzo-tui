//! Определение активного окна/процесса — нативно, платформозависимо.
//! macOS: NSWorkspace.sharedWorkspace.frontmostApplication.localizedName
//! Windows: GetForegroundWindow → GetModuleFileNameExW

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

pub fn spawn(tx: Sender<Option<String>>) {
    thread::spawn(move || loop {
        let _ = tx.send(get_active_process_name());
        thread::sleep(Duration::from_millis(300));
    });
}

#[cfg(target_os = "macos")]
pub fn get_active_process_name() -> Option<String> {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;
    use std::os::raw::c_char;
    unsafe {
        // Фоновый поток не имеет авторелиз-пула — создаём свой на каждый вызов,
        // иначе NSString от localizedName авторелизнется в никуда.
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];

        let result = (|| {
            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() { return None; }
            let app: *mut Object = msg_send![workspace, frontmostApplication];
            if app.is_null() { return None; }
            let name: *mut Object = msg_send![app, localizedName];
            if name.is_null() { return None; }
            let bytes: *const c_char = msg_send![name, UTF8String];
            if bytes.is_null() { return None; }
            // Копируем в String пока pool ещё жив.
            Some(CStr::from_ptr(bytes).to_string_lossy().into_owned())
        })();

        let () = msg_send![pool, drain];
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
