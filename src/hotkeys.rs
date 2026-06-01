//! Глобальные хоткеи: работают, даже когда фокус в другом приложении.
//!
//! Привязки настраиваются (см. [`HotkeyConfig`]) и хранятся в конфиге.
//! По умолчанию: старт — `Ctrl+\`, стоп — `Esc`.
//! Требует разрешения «Input Monitoring» в System Settings → Privacy & Security.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rdev::{listen, Event, EventType, Key};
use serde::{Deserialize, Serialize};

/// Команды от слушателя клавиатуры в UI.
#[derive(Debug)]
pub enum HotkeyCmd {
    Start,
    Stop,
    /// Не удалось запустить слушатель (обычно — нет прав доступа).
    ListenError(String),
}

/// Описание одного хоткея: модификаторы + клавиша.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HotkeySpec {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl HotkeySpec {
    /// Человекочитаемая подпись, например «Ctrl+\» или «Esc».
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(key_label(self.key));
        parts.join("+")
    }
}

/// Настройки горячих клавиш.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub start: HotkeySpec,
    pub stop: HotkeySpec,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        HotkeyConfig {
            start: HotkeySpec {
                ctrl: true,
                shift: false,
                alt: false,
                key: Key::BackSlash,
            },
            stop: HotkeySpec {
                ctrl: false,
                shift: false,
                alt: false,
                key: Key::Escape,
            },
        }
    }
}

fn is_ctrl(k: Key) -> bool {
    matches!(k, Key::ControlLeft | Key::ControlRight)
}
fn is_shift(k: Key) -> bool {
    matches!(k, Key::ShiftLeft | Key::ShiftRight)
}
fn is_alt(k: Key) -> bool {
    matches!(k, Key::Alt | Key::AltGr)
}

fn matches(spec: &HotkeySpec, ctrl: bool, shift: bool, alt: bool, key: Key) -> bool {
    spec.key == key && spec.ctrl == ctrl && spec.shift == shift && spec.alt == alt
}

/// Запускает фоновый поток-слушатель глобальной клавиатуры.
/// Конфиг читается из `config` на каждом нажатии, поэтому перепривязка
/// применяется на лету.
pub fn spawn(tx: Sender<HotkeyCmd>, config: Arc<Mutex<HotkeyConfig>>) {
    thread::spawn(move || {
        let err_tx = tx.clone();
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;

        let callback = move |event: Event| match event.event_type {
            EventType::KeyPress(k) => {
                if is_ctrl(k) {
                    ctrl = true;
                } else if is_shift(k) {
                    shift = true;
                } else if is_alt(k) {
                    alt = true;
                } else if let Ok(cfg) = config.lock() {
                    if matches(&cfg.start, ctrl, shift, alt, k) {
                        let _ = tx.send(HotkeyCmd::Start);
                    } else if matches(&cfg.stop, ctrl, shift, alt, k) {
                        let _ = tx.send(HotkeyCmd::Stop);
                    }
                }
            }
            EventType::KeyRelease(k) => {
                if is_ctrl(k) {
                    ctrl = false;
                } else if is_shift(k) {
                    shift = false;
                } else if is_alt(k) {
                    alt = false;
                }
            }
            _ => {}
        };

        if let Err(e) = listen(callback) {
            let _ = err_tx.send(HotkeyCmd::ListenError(format!("{e:?}")));
        }
    });
}

/// Преобразует нажатие из crossterm (терминал) в [`HotkeySpec`] для перепривязки.
/// Возвращает `None`, если клавишу нельзя сопоставить с глобальной.
pub fn spec_from_crossterm(key: KeyEvent) -> Option<HotkeySpec> {
    let mods = key.modifiers;
    let mut ctrl = mods.contains(KeyModifiers::CONTROL);
    let shift = mods.contains(KeyModifiers::SHIFT);
    let alt = mods.contains(KeyModifiers::ALT);

    let rkey = match key.code {
        KeyCode::Char(c) => {
            // Ctrl+<буква/символ> часто приходит как управляющий символ (<0x20).
            let c = if (c as u32) < 0x20 {
                ctrl = true;
                ((c as u8) | 0x40) as char
            } else {
                c
            };
            char_to_key(c)?
        }
        KeyCode::F(n) => f_to_key(n)?,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Insert => Key::Insert,
        KeyCode::Delete => Key::Delete,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Up => Key::UpArrow,
        KeyCode::Down => Key::DownArrow,
        KeyCode::Left => Key::LeftArrow,
        KeyCode::Right => Key::RightArrow,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::Enter => Key::Return,
        _ => return None,
    };
    Some(HotkeySpec {
        ctrl,
        shift,
        alt,
        key: rkey,
    })
}

fn char_to_key(c: char) -> Option<Key> {
    let c = c.to_ascii_lowercase();
    let k = match c {
        'a' => Key::KeyA,
        'b' => Key::KeyB,
        'c' => Key::KeyC,
        'd' => Key::KeyD,
        'e' => Key::KeyE,
        'f' => Key::KeyF,
        'g' => Key::KeyG,
        'h' => Key::KeyH,
        'i' => Key::KeyI,
        'j' => Key::KeyJ,
        'k' => Key::KeyK,
        'l' => Key::KeyL,
        'm' => Key::KeyM,
        'n' => Key::KeyN,
        'o' => Key::KeyO,
        'p' => Key::KeyP,
        'q' => Key::KeyQ,
        'r' => Key::KeyR,
        's' => Key::KeyS,
        't' => Key::KeyT,
        'u' => Key::KeyU,
        'v' => Key::KeyV,
        'w' => Key::KeyW,
        'x' => Key::KeyX,
        'y' => Key::KeyY,
        'z' => Key::KeyZ,
        '0' => Key::Num0,
        '1' => Key::Num1,
        '2' => Key::Num2,
        '3' => Key::Num3,
        '4' => Key::Num4,
        '5' => Key::Num5,
        '6' => Key::Num6,
        '7' => Key::Num7,
        '8' => Key::Num8,
        '9' => Key::Num9,
        '-' => Key::Minus,
        '=' => Key::Equal,
        '[' => Key::LeftBracket,
        ']' => Key::RightBracket,
        ';' => Key::SemiColon,
        '\'' => Key::Quote,
        '\\' => Key::BackSlash,
        ',' => Key::Comma,
        '.' => Key::Dot,
        '/' => Key::Slash,
        '`' => Key::BackQuote,
        ' ' => Key::Space,
        _ => return None,
    };
    Some(k)
}

fn f_to_key(n: u8) -> Option<Key> {
    Some(match n {
        1 => Key::F1,
        2 => Key::F2,
        3 => Key::F3,
        4 => Key::F4,
        5 => Key::F5,
        6 => Key::F6,
        7 => Key::F7,
        8 => Key::F8,
        9 => Key::F9,
        10 => Key::F10,
        11 => Key::F11,
        12 => Key::F12,
        _ => return None,
    })
}

/// Подпись клавиши для отображения.
fn key_label(key: Key) -> String {
    let s = match key {
        Key::BackSlash => "\\",
        Key::Slash => "/",
        Key::Space => "Space",
        Key::Escape => "Esc",
        Key::Return => "Enter",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Home => "Home",
        Key::End => "End",
        Key::Insert => "Insert",
        Key::Delete => "Delete",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::UpArrow => "↑",
        Key::DownArrow => "↓",
        Key::LeftArrow => "←",
        Key::RightArrow => "→",
        Key::Minus => "-",
        Key::Equal => "=",
        Key::LeftBracket => "[",
        Key::RightBracket => "]",
        Key::SemiColon => ";",
        Key::Quote => "'",
        Key::Comma => ",",
        Key::Dot => ".",
        Key::BackQuote => "`",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        other => {
            // KeyA -> "A", Num1 -> "1" и т.п.
            let dbg = format!("{other:?}");
            if let Some(rest) = dbg.strip_prefix("Key") {
                return rest.to_string();
            }
            if let Some(rest) = dbg.strip_prefix("Num") {
                return rest.to_string();
            }
            return dbg;
        }
    };
    s.to_string()
}
