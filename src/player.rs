//! Воспроизведение последовательности нажатий в системе (активном окне).
//!
//! Один долгоживущий поток держит единственный `Enigo` и принимает мелодии
//! через канал — так мы не пересоздаём ввод на каждый запуск.
//!
//! Клавиши шлются по фиксированным US-ANSI keycode'ам (как делал pyautogui),
//! поэтому нажатия не зависят от активной раскладки.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::debug;
use crate::parser::{Event, KeyAction};

/// Запрос на воспроизведение мелодии.
pub struct PlayRequest {
    pub events: Vec<Event>,
    pub countdown_secs: u64,
}

/// Сообщения от потока воспроизведения в UI.
#[derive(Debug)]
pub enum PlayerMsg {
    /// Обратный отсчёт перед стартом (осталось N секунд).
    Countdown(u64),
    /// (сыграно, всего)
    Progress(usize, usize),
    Finished,
    /// Остановлено на позиции.
    Stopped(usize),
    /// Не удалось инициализировать ввод (нет прав доступа и т.п.).
    Error(String),
}

/// macOS: US-ANSI virtual keycode (kVK_ANSI_*). `None` — нет в таблице.
#[cfg(target_os = "macos")]
fn char_to_keycode(c: char) -> Option<u16> {
    let code: u16 = match c {
        'a' => 0,
        's' => 1,
        'd' => 2,
        'f' => 3,
        'h' => 4,
        'g' => 5,
        'z' => 6,
        'x' => 7,
        'c' => 8,
        'v' => 9,
        'b' => 11,
        'q' => 12,
        'w' => 13,
        'e' => 14,
        'r' => 15,
        'y' => 16,
        't' => 17,
        'o' => 31,
        'u' => 32,
        'i' => 34,
        'p' => 35,
        'l' => 37,
        'j' => 38,
        'k' => 40,
        'n' => 45,
        'm' => 46,
        '1' => 18,
        '2' => 19,
        '3' => 20,
        '4' => 21,
        '5' => 23,
        '6' => 22,
        '7' => 26,
        '8' => 28,
        '9' => 25,
        '0' => 29,
        '-' => 27,
        '=' => 24,
        '[' => 33,
        ']' => 30,
        ';' => 41,
        '\'' => 39,
        '\\' => 42,
        ',' => 43,
        '.' => 47,
        '/' => 44,
        '`' => 50,
        ' ' => 49,
        _ => return None,
    };
    Some(code)
}

/// Windows: Virtual-Key код (VK_*). Буквы/цифры совпадают с ASCII-кодом
/// заглавного символа, пунктуация — OEM-коды раскладки US. `None` — нет.
#[cfg(target_os = "windows")]
fn char_to_keycode(c: char) -> Option<u16> {
    let code: u16 = match c {
        'a'..='z' => c.to_ascii_uppercase() as u16, // VK_A..VK_Z = 0x41..0x5A
        '0'..='9' => c as u16,                       // VK_0..VK_9 = 0x30..0x39
        ' ' => 0x20,                                 // VK_SPACE
        '-' => 0xBD,                                 // VK_OEM_MINUS
        '=' => 0xBB,                                 // VK_OEM_PLUS
        '[' => 0xDB,                                 // VK_OEM_4
        ']' => 0xDD,                                 // VK_OEM_6
        ';' => 0xBA,                                 // VK_OEM_1
        '\'' => 0xDE,                                // VK_OEM_7
        '\\' => 0xDC,                                // VK_OEM_5
        ',' => 0xBC,                                 // VK_OEM_COMMA
        '.' => 0xBE,                                 // VK_OEM_PERIOD
        '/' => 0xBF,                                 // VK_OEM_2
        '`' => 0xC0,                                 // VK_OEM_3
        _ => return None,
    };
    Some(code)
}

/// Прочие платформы (Linux): полагаемся на keysym из символа.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn char_to_keycode(_c: char) -> Option<u16> {
    None
}

fn key_for(action: &KeyAction) -> Key {
    match action {
        KeyAction::Shift => Key::Shift,
        KeyAction::Char(c) => match char_to_keycode(*c) {
            Some(code) => Key::Other(code as u32),
            None => Key::Unicode(*c), // запасной вариант для нестандартных символов
        },
    }
}

/// Спит `secs`, но просыпается раньше при выставленном флаге остановки.
fn sleep_interruptible(secs: f64, stop: &AtomicBool) {
    let total = Duration::from_secs_f64(secs.max(0.0));
    let step = Duration::from_millis(5);
    let mut elapsed = Duration::ZERO;
    while elapsed < total {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let chunk = step.min(total - elapsed);
        thread::sleep(chunk);
        elapsed += chunk;
    }
}

/// Запускает постоянный поток воспроизведения. Мелодии приходят через `rx`.
pub fn spawn(rx: Receiver<PlayRequest>, stop: Arc<AtomicBool>, tx: Sender<PlayerMsg>) {
    thread::spawn(move || match Enigo::new(&Settings::default()) {
        Ok(mut enigo) => {
            debug::log("player: enigo инициализирован, поток готов");
            while let Ok(req) = rx.recv() {
                play_one(&mut enigo, &req.events, req.countdown_secs, &stop, &tx);
            }
            debug::log("player: канал закрыт, поток завершается");
        }
        Err(e) => {
            debug::log(&format!("player: ОШИБКА инициализации enigo: {e}"));
            let msg = format!("Не удалось получить доступ к вводу: {e}");
            while rx.recv().is_ok() {
                let _ = tx.send(PlayerMsg::Error(msg.clone()));
            }
        }
    });
}

fn play_one(enigo: &mut Enigo, events: &[Event], countdown_secs: u64, stop: &AtomicBool, tx: &Sender<PlayerMsg>) {
    // Обратный отсчёт перед стартом.
    for n in (1..=countdown_secs).rev() {
        if stop.load(Ordering::Relaxed) {
            let _ = tx.send(PlayerMsg::Stopped(0));
            return;
        }
        let _ = tx.send(PlayerMsg::Countdown(n));
        sleep_interruptible(1.0, stop);
    }
    if stop.load(Ordering::Relaxed) {
        let _ = tx.send(PlayerMsg::Stopped(0));
        return;
    }

    let total = events.len();
    debug::log(&format!("player: старт воспроизведения, событий: {total}"));

    for (i, (group, pause)) in events.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            debug::log(&format!("player: остановлено на {i}"));
            let _ = tx.send(PlayerMsg::Stopped(i));
            return;
        }

        if group.len() > 1 {
            for action in group {
                if let Err(e) = enigo.key(key_for(action), Direction::Press) {
                    debug::log(&format!("player: ошибка press на событии {i}: {e}"));
                }
            }
            for action in group.iter().rev() {
                let _ = enigo.key(key_for(action), Direction::Release);
            }
        } else if let Some(action) = group.first() {
            if let Err(e) = enigo.key(key_for(action), Direction::Click) {
                debug::log(&format!("player: ошибка click на событии {i}: {e}"));
            }
        }

        let _ = tx.send(PlayerMsg::Progress(i + 1, total));
        sleep_interruptible(*pause, stop);
    }

    debug::log("player: воспроизведение завершено");
    let _ = tx.send(PlayerMsg::Finished);
}
