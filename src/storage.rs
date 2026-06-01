//! Закладки мелодий: ноты + индивидуальные задержки.
//! Каждая мелодия — отдельный JSON-файл в `~/Documents/Piano`.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::hotkeys::HotkeyConfig;

/// Сохранённая мелодия со своими задержками.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub notes: String,
    pub between_keys: f64,
    pub between_lines: f64,
}

/// Каталог с мелодиями: `~/Documents/Piano`.
pub fn piano_dir() -> PathBuf {
    let mut p = dirs::document_dir().unwrap_or_else(|| {
        let mut home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.push("Documents");
        home
    });
    p.push("Piano");
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

/// Путь к файлу мелодии по её имени.
pub fn file_path(name: &str) -> PathBuf {
    let mut p = piano_dir();
    p.push(format!("{}.json", sanitize(name)));
    p
}

/// Мелодия по умолчанию — «Für Elise» из оригинального temp.txt.
fn default_bookmark() -> Bookmark {
    Bookmark {
        name: "Für Elise".to_string(),
        notes: include_str!("../assets/fur_elise.txt").trim_end().to_string(),
        between_keys: 0.075,
        between_lines: 0.09,
    }
}

/// Загружает все мелодии из каталога. При первом запуске создаёт каталог
/// и засеивает пример.
pub fn load_bookmarks() -> Vec<Bookmark> {
    let dir = piano_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
        let seed = default_bookmark();
        let _ = save_bookmark(&seed);
        return vec![seed];
    }

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
    out.sort_by_key(|b| b.name.to_lowercase());
    out
}

/// Сохраняет одну мелодию в свой файл.
pub fn save_bookmark(b: &Bookmark) -> std::io::Result<()> {
    let dir = piano_dir();
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
    let mut p = piano_dir();
    p.push("config.json");
    p
}

/// Загружает настройки хоткеев (или значения по умолчанию).
pub fn load_config() -> HotkeyConfig {
    match fs::read_to_string(config_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => HotkeyConfig::default(),
    }
}

/// Сохраняет настройки хоткеев.
pub fn save_config(config: &HotkeyConfig) -> std::io::Result<()> {
    let dir = piano_dir();
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(config).unwrap_or_else(|_| "{}".to_string());
    fs::write(config_path(), json)
}
