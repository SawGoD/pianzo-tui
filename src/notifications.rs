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
    send("Сейчас играет", name);
}

pub fn stopped(name: &str) {
    send("Остановлено", name);
}

#[cfg(target_os = "macos")]
fn send(title: &str, body: &str) {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        body.replace('"', "\\\""),
        title.replace('"', "\\\""),
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn send(title: &str, body: &str) {
    use notify_rust::Notification;
    let _ = Notification::new().summary(title).body(body).show();
}
