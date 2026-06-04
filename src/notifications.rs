use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub on_playing: bool,
    pub on_stopped: bool,
    #[serde(default = "bool_true")]
    pub on_finished: bool,
    #[serde(default = "bool_true")]
    pub on_error: bool,
}

fn bool_true() -> bool { true }

impl Default for NotificationConfig {
    fn default() -> Self {
        Self { enabled: true, on_playing: true, on_stopped: true, on_finished: true, on_error: true }
    }
}

impl NotificationConfig {
    fn any_individual(&self) -> bool {
        self.on_playing || self.on_stopped || self.on_finished || self.on_error
    }

    pub fn toggle_global(&mut self) {
        if self.enabled {
            self.enabled = false;
            self.on_playing = false;
            self.on_stopped = false;
            self.on_finished = false;
            self.on_error = false;
        } else {
            self.enabled = true;
            if !self.any_individual() {
                self.on_playing = true;
                self.on_stopped = true;
                self.on_finished = true;
                self.on_error = true;
            }
        }
    }

    pub fn toggle_playing(&mut self) {
        self.on_playing = !self.on_playing;
        if self.on_playing { self.enabled = true; }
    }

    pub fn toggle_stopped(&mut self) {
        self.on_stopped = !self.on_stopped;
        if self.on_stopped { self.enabled = true; }
    }

    pub fn toggle_finished(&mut self) {
        self.on_finished = !self.on_finished;
        if self.on_finished { self.enabled = true; }
    }

    pub fn toggle_error(&mut self) {
        self.on_error = !self.on_error;
        if self.on_error { self.enabled = true; }
    }
}

pub fn playing(name: &str) {
    send("Сейчас играет", name);
}

pub fn stopped(name: &str) {
    send("Остановлено", name);
}

pub fn finished(name: &str) {
    send("Воспроизведение завершено", name);
}

pub fn access_error() {
    send("Ошибка доступа", "Разрешите Accessibility в System Settings");
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
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("logo.ico")));
    let mut n = Notification::new();
    n.summary(title).body(body);
    if let Some(icon) = exe_dir.filter(|p| p.exists()) {
        n.icon(&icon.to_string_lossy());
    }
    let _ = n.show();
}
