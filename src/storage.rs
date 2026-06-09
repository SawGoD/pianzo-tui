//! Закладки мелодий: ноты + индивидуальные задержки.
//! Каждая мелодия — отдельный JSON-файл в `~/Documents/Pianzo`.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::general::GeneralConfig;
use crate::hotkeys::HotkeyConfig;
use crate::notifications::NotificationConfig;
use crate::processes::ProcessConfig;

/// Сохранённая мелодия со своими задержками.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub notes: String,
    pub between_keys: f64,
    pub between_lines: f64,
    /// Метаданные импорта — заполняются только при импорте с сайта.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_meta: Option<ImportMeta>,
}

/// Метаданные оригинального источника (для пересчёта задержек после валидации).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportMeta {
    pub source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo_bpm: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_length_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transposition: Option<i32>,
    /// true — ноты уже прошли валидацию пробелами.
    #[serde(default)]
    pub validated: bool,
}

/// Базовый каталог документов (`~/Documents`).
fn documents_base() -> PathBuf {
    dirs::document_dir().unwrap_or_else(|| {
        let mut home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.push("Documents");
        home
    })
}

/// Корневой каталог приложения: `~/Documents/Pianzo`
/// (здесь лежат `config.json` и лог).
pub fn pianzo_dir() -> PathBuf {
    let mut p = documents_base();
    p.push("Pianzo");
    p
}

/// Разовая миграция со старого имени каталога `Piano` → `Pianzo`.
pub fn migrate_app_root() {
    let new = pianzo_dir();
    if new.exists() {
        return;
    }
    let mut old = documents_base();
    old.push("Piano");
    if old.exists() {
        let _ = fs::rename(&old, &new);
    }
}

/// Каталог с мелодиями: `~/Documents/Pianzo/tracks`.
pub fn tracks_dir() -> PathBuf {
    let mut p = pianzo_dir();
    p.push("tracks");
    p
}

/// Превращает имя в безопасное имя файла.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "melody".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Путь к файлу мелодии по её имени (в каталоге `tracks`).
pub fn file_path(name: &str) -> PathBuf {
    let mut p = tracks_dir();
    p.push(format!("{}.json", sanitize(name)));
    p
}

/// Переносит мелодии из старого расположения (`Pianzo/*.json`) в `Pianzo/tracks/`.
fn migrate_legacy() {
    let root = pianzo_dir();
    let legacy: Vec<PathBuf> = match fs::read_dir(&root) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().and_then(|s| s.to_str()) == Some("json")
                    && p.file_name().and_then(|s| s.to_str()) != Some("config.json")
            })
            .collect(),
        Err(_) => return,
    };
    if legacy.is_empty() {
        return;
    }
    let tracks = tracks_dir();
    let _ = fs::create_dir_all(&tracks);
    for p in legacy {
        if let Some(name) = p.file_name() {
            let _ = fs::rename(&p, tracks.join(name));
        }
    }
}

/// Мелодия по умолчанию — «Für Elise» из оригинального temp.txt.
fn default_bookmark() -> Bookmark {
    Bookmark {
        name: "Für Elise".to_string(),
        notes: include_str!("../assets/fur_elise.txt").trim_end().to_string(),
        between_keys: 0.165,
        between_lines: 0.160,
        import_meta: None,
    }
}

/// Загружает все мелодии из `Pianzo/tracks`. На первом запуске засеивает пример;
/// при необходимости переносит мелодии из старого расположения.
pub fn load_bookmarks() -> Vec<Bookmark> {
    migrate_app_root();
    let dir = tracks_dir();
    // Первый запуск, если нет ни каталога tracks, ни старых файлов в корне.
    let first_run = !dir.exists() && !has_legacy();
    migrate_legacy();
    let _ = fs::create_dir_all(&dir);

    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(text) = fs::read_to_string(&path) {
                    if let Ok(b) = serde_json::from_str::<Bookmark>(&text) {
                        out.push(b);
                    }
                }
            }
        }
    }

    if out.is_empty() && first_run {
        let seed = default_bookmark();
        let _ = save_bookmark(&seed);
        out.push(seed);
    }

    out.sort_by_key(|b| b.name.to_lowercase());
    out
}

/// Есть ли мелодии в старом расположении (`Pianzo/*.json`, кроме config.json).
fn has_legacy() -> bool {
    match fs::read_dir(pianzo_dir()) {
        Ok(entries) => entries.flatten().any(|e| {
            let p = e.path();
            p.is_file()
                && p.extension().and_then(|s| s.to_str()) == Some("json")
                && p.file_name().and_then(|s| s.to_str()) != Some("config.json")
        }),
        Err(_) => false,
    }
}

/// Сохраняет одну мелодию в свой файл (в каталоге `tracks`).
pub fn save_bookmark(b: &Bookmark) -> std::io::Result<()> {
    let dir = tracks_dir();
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(b).unwrap_or_else(|_| "{}".to_string());
    fs::write(file_path(&b.name), json)
}

/// Удаляет файл мелодии.
pub fn delete_bookmark(name: &str) -> std::io::Result<()> {
    let path = file_path(name);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn config_path() -> PathBuf {
    let mut p = pianzo_dir();
    p.push("config.json");
    p
}

fn default_volume() -> f32 {
    0.5
}

/// Сохраняемый конфиг: хоткеи + громкость + уведомления + общие + процессы.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredConfig {
    #[serde(flatten)]
    hotkeys: HotkeyConfig,
    #[serde(default = "default_volume")]
    volume: f32,
    #[serde(default)]
    notifications: NotificationConfig,
    #[serde(default)]
    general: GeneralConfig,
    #[serde(default)]
    processes: ProcessConfig,
}

/// Загружает конфиг (или значения по умолчанию).
pub fn load_config() -> (HotkeyConfig, f32, NotificationConfig, GeneralConfig, ProcessConfig) {
    match fs::read_to_string(config_path()) {
        Ok(s) => match serde_json::from_str::<StoredConfig>(&s) {
            Ok(c) => (c.hotkeys, c.volume.clamp(0.0, 1.0), c.notifications, c.general, c.processes),
            Err(_) => defaults(),
        },
        Err(_) => defaults(),
    }
}

fn defaults() -> (HotkeyConfig, f32, NotificationConfig, GeneralConfig, ProcessConfig) {
    (HotkeyConfig::default(), default_volume(), NotificationConfig::default(), GeneralConfig::default(), ProcessConfig::default())
}

/// Сохраняет конфиг.
pub fn save_config(
    hotkeys: &HotkeyConfig,
    volume: f32,
    notifications: &NotificationConfig,
    general: &GeneralConfig,
    processes: &ProcessConfig,
) -> std::io::Result<()> {
    let dir = pianzo_dir();
    fs::create_dir_all(&dir)?;
    let stored = StoredConfig {
        hotkeys: *hotkeys,
        volume,
        notifications: *notifications,
        general: *general,
        processes: processes.clone(),
    };
    let json = serde_json::to_string_pretty(&stored).unwrap_or_else(|_| "{}".to_string());
    fs::write(config_path(), json)
}
