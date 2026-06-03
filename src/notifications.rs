use notify_rust::Notification;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub on_playing: bool,
    pub on_stopped: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self { enabled: true, on_playing: true, on_stopped: true }
    }
}

impl NotificationConfig {
    /// Включить/выключить глобальный тумблер.
    /// OFF → все гасятся. ON (из полностью выключенного состояния) → все включаются.
    pub fn toggle_global(&mut self) {
        if self.enabled {
            self.enabled = false;
            self.on_playing = false;
            self.on_stopped = false;
        } else {
            self.enabled = true;
            if !self.on_playing && !self.on_stopped {
                self.on_playing = true;
                self.on_stopped = true;
            }
        }
    }

    pub fn toggle_playing(&mut self) {
        self.on_playing = !self.on_playing;
        if self.on_playing {
            self.enabled = true;
        }
    }

    pub fn toggle_stopped(&mut self) {
        self.on_stopped = !self.on_stopped;
        if self.on_stopped {
            self.enabled = true;
        }
    }
}

pub fn playing(name: &str) {
    let _ = Notification::new()
        .summary("Сейчас играет")
        .body(name)
        .show();
}

pub fn stopped(name: &str) {
    let _ = Notification::new()
        .summary("Остановлено")
        .body(name)
        .show();
}
