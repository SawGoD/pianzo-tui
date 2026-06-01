//! Парсинг нотной записи в последовательность нажатий.
//!
//! Логика полностью повторяет оригинальный `main.py`, чтобы старые мелодии
//! проигрывались идентично:
//!   * токены разделяются пробелами;
//!   * `[abc]` — аккорд: все символы нажимаются одновременно (lower + мап спецсимволов);
//!   * одиночный токен: спецсимвол мапится, заглавная буква проигрывается как `Shift + буква`;
//!   * задержка `between_keys` ставится после каждой группы, кроме последней в строке,
//!     после последней группы строки ставится `between_lines`.

/// Одно действие внутри группы нажатия.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyAction {
    Shift,
    Char(char),
}

/// Группа клавиш, нажимаемых одновременно, и пауза (в секундах) после неё.
pub type Event = (Vec<KeyAction>, f64);

/// Положение токена в исходном тексте (для подсветки во время игры).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TokenSpan {
    /// Индекс строки (0-based).
    pub line: usize,
    /// Байтовое смещение начала токена в исходном тексте.
    pub start: usize,
    /// Байтовое смещение конца токена.
    pub end: usize,
}

/// Результат разбора: события для проигрывания + их позиции в тексте (1:1).
#[derive(Default)]
pub struct Parsed {
    pub events: Vec<Event>,
    pub spans: Vec<TokenSpan>,
}

/// Мап спецсимволов «как на клавиатуре с Shift» обратно в базовую цифру.
fn replace_special_char(c: char) -> char {
    match c {
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        other => other,
    }
}

/// Версия для одиночного токена: в Python мап применялся к токену целиком через
/// `dict.get(key, key)`, т.е. срабатывал только если весь токен — один спецсимвол.
fn replace_special_token(token: &str) -> String {
    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => replace_special_char(c).to_string(),
        _ => token.to_string(),
    }
}

/// Аналог `str.isupper()` в Python: есть хотя бы одна буква и все буквы заглавные.
fn is_upper(s: &str) -> bool {
    let mut has_alpha = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            has_alpha = true;
            if !c.is_uppercase() {
                return false;
            }
        }
    }
    has_alpha
}

/// Строит группу нажатий из одного токена. Пустую группу не возвращает.
fn build_group(token: &str) -> Vec<KeyAction> {
    let is_chord = token.len() >= 2 && token.starts_with('[') && token.ends_with(']');
    if is_chord {
        let inner = &token[1..token.len() - 1];
        inner
            .chars()
            .map(|c| KeyAction::Char(replace_special_char(c.to_ascii_lowercase())))
            .collect()
    } else {
        let replaced = replace_special_token(token);
        let lowered = replaced.to_lowercase();
        let mut group = Vec::new();
        if is_upper(&replaced) {
            group.push(KeyAction::Shift);
        }
        for c in lowered.chars() {
            group.push(KeyAction::Char(c));
        }
        group
    }
}

/// Разбивает строку на токены, возвращая их байтовые границы в пределах строки.
/// Ноты состоят из ASCII-символов, поэтому индексация по байтам безопасна.
fn tokens_with_spans(line: &str) -> Vec<(&str, usize, usize)> {
    let bytes = line.as_bytes();
    let len = line.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < len {
        while i < len && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= len {
            break;
        }
        let start = i;
        while i < len && !(bytes[i] as char).is_whitespace() {
            i += 1;
        }
        out.push((&line[start..i], start, i));
    }
    out
}

/// Разбирает нотную запись в события + их позиции в исходном тексте.
pub fn parse(notes: &str, between_keys: f64, between_lines: f64) -> Parsed {
    let mut parsed = Parsed::default();
    let mut line_start = 0usize;

    for (line_no, line) in notes.split('\n').enumerate() {
        let mut groups: Vec<(Vec<KeyAction>, TokenSpan)> = Vec::new();

        for (token, s, e) in tokens_with_spans(line) {
            let group = build_group(token);
            if !group.is_empty() {
                groups.push((
                    group,
                    TokenSpan {
                        line: line_no,
                        start: line_start + s,
                        end: line_start + e,
                    },
                ));
            }
        }

        let n = groups.len();
        for (i, (group, span)) in groups.into_iter().enumerate() {
            let pause = if i + 1 == n { between_lines } else { between_keys };
            parsed.events.push((group, pause));
            parsed.spans.push(span);
        }

        line_start += line.len() + 1; // +1 за '\n'
    }

    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_upper_becomes_shift() {
        let p = parse("E", 0.06, 0.1);
        assert_eq!(p.events.len(), 1);
        assert_eq!(p.events[0].0, vec![KeyAction::Shift, KeyAction::Char('e')]);
    }

    #[test]
    fn special_char_mapped() {
        let p = parse("^", 0.06, 0.1);
        assert_eq!(p.events[0].0, vec![KeyAction::Char('6')]);
    }

    #[test]
    fn chord_is_lowercased_without_shift() {
        let p = parse("[oP]", 0.06, 0.1);
        assert_eq!(
            p.events[0].0,
            vec![KeyAction::Char('o'), KeyAction::Char('p')]
        );
    }

    #[test]
    fn chord_special_chars_mapped() {
        let p = parse("[!E]", 0.06, 0.1);
        assert_eq!(
            p.events[0].0,
            vec![KeyAction::Char('1'), KeyAction::Char('e')]
        );
    }

    #[test]
    fn pauses_between_keys_and_lines() {
        // две группы в строке + ещё строка
        let p = parse("a b\nc", 0.06, 0.1);
        assert_eq!(p.events.len(), 3);
        assert_eq!(p.events[0].1, 0.06); // не последняя в строке
        assert_eq!(p.events[1].1, 0.1); // последняя в строке
        assert_eq!(p.events[2].1, 0.1); // последняя в строке
    }

    #[test]
    fn spans_match_tokens() {
        let notes = "ab cd\nef";
        let p = parse(notes, 0.06, 0.1);
        assert_eq!(p.spans.len(), p.events.len());
        // токены: "ab"@0..2 (стр0), "cd"@3..5 (стр0), "ef"@6..8 (стр1)
        assert_eq!((p.spans[0].line, p.spans[0].start, p.spans[0].end), (0, 0, 2));
        assert_eq!((p.spans[1].line, p.spans[1].start, p.spans[1].end), (0, 3, 5));
        assert_eq!((p.spans[2].line, p.spans[2].start, p.spans[2].end), (1, 6, 8));
        assert_eq!(&notes[p.spans[2].start..p.spans[2].end], "ef");
    }
}
