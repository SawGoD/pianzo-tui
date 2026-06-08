//! Простое файловое логирование для отладки (TUI прячет stdout/stderr).
//! Пишет в `~/Documents/Pianzo/pianzo-tui.log`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::pianzo_dir;

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Дописывает строку в лог-файл (молча игнорирует ошибки записи).
pub fn log(msg: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let dir = pianzo_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("pianzo-tui.log");

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{ts:.3}] {msg}");
    }
}
