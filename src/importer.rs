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
/// Ноты лежат в `<p>` внутри `<div id="sheet-content">`.
/// Строки разделены `||`; пустые строки (из `||||`) игнорируются.
/// Имя берётся из `<title>`.
fn parse_virtualpiano(html: &str) -> Result<ImportResult, ImportError> {
    crate::debug::log("[importer] parse_virtualpiano: start");
    let name = extract_title_virtualpiano(html);
    crate::debug::log(&format!("[importer] title: {name}"));
    let notes = extract_notes_virtualpiano(html)
        .ok_or_else(|| ImportError::Parse("Ноты не найдены на странице".to_string()))?;
    crate::debug::log(&format!("[importer] notes length: {} chars", notes.len()));
    Ok(ImportResult { name, notes })
}

fn extract_title_virtualpiano(html: &str) -> String {
    // <title>Play NAME Music Sheet | ...</title>
    if let Some(s) = between(html, "<title>", "</title>") {
        let s = strip_tags(s);
        // убрать "Play " в начале и " Music Sheet | ..." в конце
        let s = s.trim()
            .trim_start_matches("Play ")
            .to_string();
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
    // Ноты в <p> внутри <div id="sheet-content" ...>
    // Структура: id="sheet-content" class="..."><...виджеты...><p>НОТЫ</p>
    let marker = "id=\"sheet-content\"";
    let pos = html.find(marker)?;
    let rest = &html[pos + marker.len()..];
    // найти первый <p>
    let p_start = rest.find("<p")?;
    let inner_start = rest[p_start..].find('>')? + p_start + 1;
    let inner_end = rest[inner_start..].find("</p>")?;
    let raw = &rest[inner_start..inner_start + inner_end];

    crate::debug::log(&format!("[importer] raw notes preview: {:.120}", raw));

    // Убрать HTML-теги, декодировать сущности
    let decoded = html_decode(&strip_tags(raw));

    // Разделитель строк — "||"; заменяем на переводы строк, убираем пустые
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

/// Возвращает текст между двумя подстроками.
fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let end = s[start..].find(close)?;
    Some(&s[start..start + end])
}

/// Убирает HTML-теги.
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

/// Декодирует HTML-сущности (&amp; &lt; &#39; и т.д.).
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
