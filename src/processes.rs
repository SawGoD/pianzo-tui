//! Фильтрация по активному процессу/окну.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProcessMode {
    #[default]
    None,
    Track,
    Ignore,
}

impl ProcessMode {
    pub fn next(self) -> Self {
        match self {
            ProcessMode::None => ProcessMode::Track,
            ProcessMode::Track => ProcessMode::Ignore,
            ProcessMode::Ignore => ProcessMode::None,
        }
    }

}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessEntry {
    pub name: String,
    pub mode: ProcessMode,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ProcessConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub entries: Vec<ProcessEntry>,
}

impl ProcessConfig {
    /// Возвращает режим для заданного имени окна/процесса.
    pub fn window_mode(&self, name: &str) -> Option<ProcessMode> {
        let lower = name.to_lowercase();
        for entry in &self.entries {
            let entry_lower = entry.name.to_lowercase();
            if lower.contains(&entry_lower) || entry_lower.contains(&lower) {
                return Some(entry.mode);
            }
        }
        None
    }

    /// None = правила не применяются (используем FOCUSED/UNFOCUSED).
    /// Some(true) = разрешить воспроизведение.
    /// Some(false) = заблокировать.
    pub fn should_allow(&self, active_window: &Option<String>) -> Option<bool> {
        if !self.enabled {
            return None;
        }
        let name = active_window.as_deref().unwrap_or("");
        match self.window_mode(name) {
            Some(ProcessMode::Track) => Some(true),
            Some(ProcessMode::Ignore) => Some(false),
            _ => None,
        }
    }
}

fn strip_exe(name: String) -> String {
    #[cfg(target_os = "windows")]
    if name.to_lowercase().ends_with(".exe") {
        return name[..name.len() - 4].to_string();
    }
    name
}

pub fn list_running_processes() -> Vec<String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    let mut names: std::collections::HashSet<String> = sys
        .processes()
        .values()
        .map(|p| strip_exe(p.name().to_string()))
        .collect();
    let mut v: Vec<String> = names.drain().collect();
    v.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    v
}

pub fn search_processes(query: &str, cached: &[String]) -> Vec<String> {
    if query.is_empty() {
        return Vec::new();
    }
    let q = query.to_lowercase();
    cached
        .iter()
        .filter(|n| n.to_lowercase().contains(&q))
        .cloned()
        .take(8)
        .collect()
}
