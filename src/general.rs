use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_delay")]
    pub default_keys: f64,
    #[serde(default = "default_delay")]
    pub default_lines: f64,
    #[serde(default = "default_countdown")]
    pub countdown_secs: u64,
    #[serde(default)]
    pub logging_enabled: bool,
}

fn default_delay() -> f64 { 0.110 }
fn default_countdown() -> u64 { 3 }

impl Default for GeneralConfig {
    fn default() -> Self {
        Self { default_keys: 0.110, default_lines: 0.110, countdown_secs: 3, logging_enabled: false }
    }
}

impl GeneralConfig {
    pub fn nudge_keys(&mut self, delta: f64) {
        self.default_keys = (self.default_keys + delta).max(0.0);
        self.default_keys = (self.default_keys * 1000.0).round() / 1000.0;
    }

    pub fn nudge_lines(&mut self, delta: f64) {
        self.default_lines = (self.default_lines + delta).max(0.0);
        self.default_lines = (self.default_lines * 1000.0).round() / 1000.0;
    }

    pub fn nudge_countdown(&mut self, delta: i64) {
        let next = self.countdown_secs as i64 + delta;
        self.countdown_secs = next.max(1) as u64;
    }
}
