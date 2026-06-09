//! Импорт нот из внешних сайтов по URL.

/// Ошибка импорта.
#[derive(Debug)]
pub enum ImportError {
    Fetch(String),
    Parse(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Fetch(e) => write!(f, "Ошибка загрузки: {e}"),
            ImportError::Parse(e) => write!(f, "Ошибка парсинга: {e}"),
        }
    }
}

/// Результат импорта.
pub struct ImportResult {
    pub name: String,
    pub notes: String,
    /// Рассчитанная задержка между клавишами (None — использовать дефолт приложения).
    pub between_keys: Option<f64>,
    /// Рассчитанная задержка между строками (None — использовать дефолт приложения).
    pub between_lines: Option<f64>,
}

/// Импортирует мелодию из URL. Определяет сайт и вызывает нужный парсер.
pub fn import(url: &str) -> Result<ImportResult, ImportError> {
    crate::debug::log(&format!("[importer] import url: {url}"));
    let body = fetch(url)?;
    crate::debug::log(&format!("[importer] fetched {} bytes", body.len()));
    if url.contains("virtualpiano.net") {
        parse_virtualpiano(&body)
    } else {
        let msg = "Сайт не поддерживается";
        crate::debug::log(&format!("[importer] {msg}: {url}"));
        Err(ImportError::Parse(msg.to_string()))
    }
}

fn fetch(url: &str) -> Result<String, ImportError> {
    ureq::get(url)
        .call()
        .map_err(|e| {
            let msg = e.to_string();
            crate::debug::log(&format!("[importer] fetch error: {msg}"));
            ImportError::Fetch(msg)
        })?
        .into_string()
        .map_err(|e| {
            let msg = e.to_string();
            crate::debug::log(&format!("[importer] read error: {msg}"));
            ImportError::Fetch(msg)
        })
}

/// Парсит страницу virtualpiano.net/music-sheet/...
///
/// Ноты — в `<p>` внутри `<div id="sheet-content">`, строки разделены `||`.
/// Задержки рассчитываются из TARGET LENGTH и количества токенов/строк.
fn parse_virtualpiano(html: &str) -> Result<ImportResult, ImportError> {
    crate::debug::log("[importer] parse_virtualpiano: start");
    let name = extract_title_virtualpiano(html);
    crate::debug::log(&format!("[importer] title: {name}"));
    let notes = extract_notes_virtualpiano(html)
        .ok_or_else(|| ImportError::Parse("Ноты не найдены на странице".to_string()))?;
    crate::debug::log(&format!("[importer] notes length: {} chars", notes.len()));

    let (between_keys, between_lines) = calc_delays(&notes, html);

    Ok(ImportResult { name, notes, between_keys, between_lines })
}

/// Вычисляет задержки на основе TARGET LENGTH и числа токенов.
///
/// `||` в VP — визуальный разделитель, не музыкальная пауза,
/// поэтому between_lines = between_keys.
/// TARGET LENGTH рассчитан на живого игрока, автомат играет вдвое быстрее,
/// поэтому применяем коэффициент HUMAN_FACTOR = 0.5.
fn calc_delays(notes: &str, html: &str) -> (Option<f64>, Option<f64>) {
    const HUMAN_FACTOR: f64 = 0.5;
    const MIN_DELAY: f64 = 0.05;
    const MAX_DELAY: f64 = 2.0;

    let Some(total) = extract_target_length(html) else {
        crate::debug::log("[importer] TARGET LENGTH not found, using app defaults");
        return (None, None);
    };

    let num_tokens: f64 = notes.lines()
        .map(|l| l.split_whitespace().count() as f64)
        .sum();

    if num_tokens < 1.0 {
        return (None, None);
    }

    let k = ((total * HUMAN_FACTOR) / num_tokens).clamp(MIN_DELAY, MAX_DELAY);

    crate::debug::log(&format!(
        "[importer] TARGET LENGTH={total:.1}s tokens={num_tokens} factor={HUMAN_FACTOR} => between_keys={k:.3}s"
    ));

    (Some(k), Some(k))
}

/// Извлекает TARGET LENGTH в секундах из `<span id="target-length">M:SS</span>`.
fn extract_target_length(html: &str) -> Option<f64> {
    let marker = "id=\"target-length\">";
    let pos = html.find(marker)?;
    let rest = &html[pos + marker.len()..];
    let end = rest.find('<')?;
    let raw = rest[..end].trim();
    // формат M:SS или MM:SS
    let mut parts = raw.splitn(2, ':');
    let mins: f64 = parts.next()?.trim().parse().ok()?;
    let secs: f64 = parts.next()?.trim().parse().ok()?;
    let total = mins * 60.0 + secs;
    crate::debug::log(&format!("[importer] TARGET LENGTH raw='{raw}' => {total}s"));
    Some(total)
}

fn extract_title_virtualpiano(html: &str) -> String {
    if let Some(s) = between(html, "<title>", "</title>") {
        let s = strip_tags(s);
        let s = s.trim().trim_start_matches("Play ").to_string();
        let s = if let Some(pos) = s.find(" Music Sheet") {
            s[..pos].trim().to_string()
        } else if let Some(pos) = s.find(" | ") {
            s[..pos].trim().to_string()
        } else {
            s
        };
        if !s.is_empty() {
            return s;
        }
    }
    "Imported".to_string()
}

fn extract_notes_virtualpiano(html: &str) -> Option<String> {
    let marker = "id=\"sheet-content\"";
    let pos = html.find(marker)?;
    let rest = &html[pos + marker.len()..];
    let p_start = rest.find("<p")?;
    let inner_start = rest[p_start..].find('>')? + p_start + 1;
    let inner_end = rest[inner_start..].find("</p>")?;
    let raw = &rest[inner_start..inner_start + inner_end];

    crate::debug::log(&format!("[importer] raw notes preview: {:.120}", raw));

    let decoded = html_decode(&strip_tags(raw));
    let notes: String = decoded
        .split("||")
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if notes.is_empty() {
        crate::debug::log("[importer] notes empty after split");
        None
    } else {
        Some(notes)
    }
}

// --- Минимальные HTML-утилиты (без внешних зависимостей) ---

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let end = s[start..].find(close)?;
    Some(&s[start..start + end])
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&#13;", "\r")
        .replace("&#10;", "\n")
}
