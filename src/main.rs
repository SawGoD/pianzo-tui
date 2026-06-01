mod app;
mod audio;
mod debug;
mod hotkeys;
mod parser;
mod player;
mod storage;
mod ui;

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use app::{App, EditFocus, Mode};
use audio::{AudioMsg, AudioRequest};
use hotkeys::HotkeyCmd;
use parser::Event as NoteEvent;
use player::PlayerMsg;

fn main() -> io::Result<()> {
    let (hk_tx, hk_rx) = mpsc::channel::<HotkeyCmd>();
    let (pl_tx, pl_rx) = mpsc::channel::<PlayerMsg>();
    let (play_tx, play_rx) = mpsc::channel::<Vec<NoteEvent>>();
    let (audio_tx, audio_rx) = mpsc::channel::<AudioRequest>();
    let (amsg_tx, amsg_rx) = mpsc::channel::<AudioMsg>();
    let stop = Arc::new(AtomicBool::new(false));

    let mut app = App::new();
    debug::log("=== запуск piano-tui ===");

    // Слушатель глобальной клавиатуры с общим конфигом хоткеев.
    hotkeys::spawn(hk_tx, Arc::clone(&app.hotkeys));
    // Постоянный поток воспроизведения клавиш.
    player::spawn(play_rx, Arc::clone(&stop), pl_tx);
    // Постоянный аудио-поток.
    audio::spawn(audio_rx, Arc::clone(&stop), amsg_tx, Arc::clone(&app.volume));

    let mut terminal = ratatui::init();

    // Логируем панику (и даём ratatui восстановить терминал — это предыдущий хук).
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        debug::log(&format!("PANIC: {info}"));
        prev_hook(info);
    }));

    let result = run(&mut terminal, &mut app, &hk_rx, &pl_rx, &play_tx, &amsg_rx, &audio_tx, &stop);
    ratatui::restore();
    debug::log("=== выход piano-tui ===");
    result
}

#[allow(clippy::too_many_arguments)]
fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    hk_rx: &Receiver<HotkeyCmd>,
    pl_rx: &Receiver<PlayerMsg>,
    play_tx: &Sender<Vec<NoteEvent>>,
    amsg_rx: &Receiver<AudioMsg>,
    audio_tx: &Sender<AudioRequest>,
    stop: &Arc<AtomicBool>,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        while let Ok(cmd) = hk_rx.try_recv() {
            match cmd {
                // Старт работает только когда FOCUSED и в обычном режиме —
                // чтобы глобальный хоткей не мешал вводу и не срабатывал,
                // пока ты в другом окне (UNFOCUSED).
                HotkeyCmd::Start if app.focused && app.mode == Mode::Normal => {
                    start_playback(app, stop, play_tx)
                }
                HotkeyCmd::Start => {
                    debug::log("main: старт проигнорирован (UNFOCUSED или модальное окно)");
                }
                HotkeyCmd::Stop => stop_playback(app, stop),
                HotkeyCmd::ListenError(e) => {
                    app.status = format!(
                        "Глобальные хоткеи недоступны ({e}). Разрешите Input Monitoring."
                    );
                }
            }
        }

        while let Ok(msg) = pl_rx.try_recv() {
            match msg {
                PlayerMsg::Countdown(n) => {
                    app.countdown = Some(n);
                    app.status = format!("Старт через {n}… (переключитесь в нужное окно)");
                }
                PlayerMsg::Progress(done, total) => {
                    app.countdown = None;
                    app.progress = (done, total);
                    app.play_event = done.saturating_sub(1);
                }
                PlayerMsg::Finished => {
                    app.playing = false;
                    app.countdown = None;
                    app.status = "Готово.".to_string();
                }
                PlayerMsg::Stopped(at) => {
                    app.playing = false;
                    app.countdown = None;
                    app.status = format!("Остановлено на позиции {at}.");
                }
                PlayerMsg::Error(e) => {
                    app.playing = false;
                    app.countdown = None;
                    app.status = format!("{e}. Разрешите Accessibility в System Settings.");
                }
            }
        }

        while let Ok(msg) = amsg_rx.try_recv() {
            match msg {
                AudioMsg::Progress(i) => app.play_event = i,
                AudioMsg::Finished => {
                    app.audio_playing = false;
                    app.status = "Тест: готово.".to_string();
                }
                AudioMsg::Stopped => {
                    app.audio_playing = false;
                    app.status = "Тест остановлен.".to_string();
                }
                AudioMsg::Error(e) => {
                    app.audio_playing = false;
                    app.status = e;
                }
            }
        }

        if event::poll(Duration::from_millis(50))? {
            let ev = event::read()?;
            if let Event::Key(key) = ev {
                if key.kind == KeyEventKind::Release {
                    // На Windows crossterm шлёт ещё Release-события — игнорируем их,
                    // чтобы не было двойного ввода (на macOS таких событий нет).
                } else if app.playing {
                    // Во время воспроизведения игнорируем ввод в TUI,
                    // чтобы синтезированные клавиши не нажимали кнопки интерфейса.
                } else if app.mode == Mode::Edit {
                    handle_edit(app, key, ev);
                } else if matches!(app.mode, Mode::CaptureStart | Mode::CaptureStop) {
                    handle_capture(app, key);
                } else {
                    handle_key(app, key, stop, play_tx, audio_tx);
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn start_playback(app: &mut App, stop: &Arc<AtomicBool>, play_tx: &Sender<Vec<NoteEvent>>) {
    if app.playing || app.audio_playing {
        return;
    }
    if app.current_name.is_none() {
        app.status = "Ничего не заряжено — нажми Enter на нужной мелодии.".to_string();
        return;
    }
    // Играет ЗАРЯЖЕННАЯ по Enter мелодия (рабочая копия).
    let parsed = parser::parse(&app.notes, app.between_keys, app.between_lines);
    if parsed.events.is_empty() {
        app.status = "Нет нот для воспроизведения.".to_string();
        return;
    }
    stop.store(false, Ordering::Relaxed);
    app.playing = true;
    app.play_notes = app.notes.clone();
    app.spans = parsed.spans;
    app.play_event = 0;
    app.progress = (0, parsed.events.len());
    app.countdown = Some(player::COUNTDOWN_SECS);
    app.status = format!("Старт через {}…", player::COUNTDOWN_SECS);

    debug::log(&format!(
        "main: запрос воспроизведения «{}», событий: {}",
        app.current_name.as_deref().unwrap_or("—"),
        parsed.events.len()
    ));

    if play_tx.send(parsed.events).is_err() {
        app.playing = false;
        app.countdown = None;
        app.status = "Поток воспроизведения недоступен.".to_string();
        debug::log("main: канал плеера закрыт — воспроизведение невозможно");
    }
}

fn stop_playback(app: &mut App, stop: &Arc<AtomicBool>) {
    if app.playing || app.audio_playing {
        stop.store(true, Ordering::Relaxed);
        app.status = "Остановка...".to_string();
        debug::log("main: запрошена остановка");
    }
}

fn start_audio(app: &mut App, stop: &Arc<AtomicBool>, audio_tx: &Sender<AudioRequest>) {
    if app.playing || app.audio_playing {
        return;
    }
    // Тестируется НАВЕДЁННАЯ (hovered) закладка — не трогая заряженную.
    let Some(b) = app.selected_bookmark().cloned() else {
        app.status = "Нет закладки для теста.".to_string();
        return;
    };
    let parsed = parser::parse_pitches(&b.notes, b.between_keys, b.between_lines);
    if parsed.groups.is_empty() {
        app.status = "Нет нот для теста.".to_string();
        return;
    }
    stop.store(false, Ordering::Relaxed);
    app.audio_playing = true;
    app.play_notes = b.notes.clone();
    app.spans = parsed.spans;
    app.play_event = 0;
    app.status = format!("♪ Тест «{}» (громкость {}%)", b.name, app.volume_pct());
    debug::log(&format!(
        "main: запрос теста «{}», групп: {}",
        b.name,
        parsed.groups.len()
    ));
    if audio_tx
        .send(AudioRequest {
            groups: parsed.groups,
        })
        .is_err()
    {
        app.audio_playing = false;
        app.status = "Аудио-поток недоступен.".to_string();
    }
}

// --- Окно правки (ноты + задержки) ---

fn handle_edit(app: &mut App, key: KeyEvent, ev: Event) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => return app.commit_edit(),
        KeyCode::Char('s') if ctrl => return app.commit_edit(),
        KeyCode::Char('q') if ctrl => return app.cancel_edit(),
        KeyCode::Tab | KeyCode::BackTab => {
            app.edit_focus = match app.edit_focus {
                EditFocus::Notes => EditFocus::Delays,
                EditFocus::Delays => EditFocus::Notes,
            };
            return;
        }
        _ => {}
    }

    match app.edit_focus {
        EditFocus::Notes => {
            app.textarea.input(ev);
        }
        EditFocus::Delays => match key.code {
            KeyCode::Left | KeyCode::Right => app.delay_field ^= 1,
            KeyCode::Up => app.nudge_delay(0.001),
            KeyCode::Down => app.nudge_delay(-0.001),
            KeyCode::Char(c) if c.is_ascii_digit() || c == '.' || c == ',' => {
                app.active_delay_buf().push(c);
            }
            KeyCode::Backspace => {
                app.active_delay_buf().pop();
            }
            _ => {}
        },
    }
}

// --- Захват комбинации для перепривязки ---

fn handle_capture(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        app.mode = Mode::HotkeyMenu;
        app.status = "Перепривязка отменена.".to_string();
        return;
    }
    match hotkeys::spec_from_crossterm(key) {
        Some(spec) => {
            let start = app.mode == Mode::CaptureStart;
            app.set_hotkey(start, spec);
            app.mode = Mode::HotkeyMenu;
        }
        None => {
            app.status = "Эту клавишу нельзя назначить.".to_string();
        }
    }
}

// --- Обычные режимы ---

fn handle_key(
    app: &mut App,
    key: KeyEvent,
    stop: &Arc<AtomicBool>,
    play_tx: &Sender<Vec<NoteEvent>>,
    audio_tx: &Sender<AudioRequest>,
) {
    match app.mode {
        Mode::Normal => handle_normal(app, key, stop, play_tx, audio_tx),
        Mode::AddName => handle_add_name(app, key),
        Mode::SaveBookmark => handle_save_input(app, key),
        Mode::ConfirmDelete => handle_confirm_delete(app, key),
        Mode::HotkeyMenu => handle_hotkey_menu(app, key),
        Mode::Edit | Mode::CaptureStart | Mode::CaptureStop => {} // обрабатываются отдельно
    }
}

fn handle_normal(
    app: &mut App,
    key: KeyEvent,
    stop: &Arc<AtomicBool>,
    play_tx: &Sender<Vec<NoteEvent>>,
    audio_tx: &Sender<AudioRequest>,
) {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Enter => {
            if let Some(i) = app.selected() {
                app.load_bookmark(i);
            }
        }
        KeyCode::Char('a') => {
            app.input.clear();
            app.mode = Mode::AddName;
        }
        KeyCode::Char('n') | KeyCode::Char('E') | KeyCode::Char('e') => app.begin_edit(),
        KeyCode::Char('h') | KeyCode::Char('H') => app.mode = Mode::HotkeyMenu,
        KeyCode::Char('s') => {
            app.input = app
                .selected_bookmark()
                .map(|b| b.name.clone())
                .unwrap_or_default();
            app.mode = Mode::SaveBookmark;
        }
        KeyCode::Char('d') if app.selected().is_some() => app.mode = Mode::ConfirmDelete,
        KeyCode::Char('p') => start_playback(app, stop, play_tx),
        KeyCode::Char('t') | KeyCode::Char('T') => start_audio(app, stop, audio_tx),
        KeyCode::Char('u') | KeyCode::Char('U') => app.toggle_focus(),
        KeyCode::Char('+') | KeyCode::Char('=') => app.change_volume(0.1),
        KeyCode::Char('-') | KeyCode::Char('_') => app.change_volume(-0.1),
        _ => {}
    }
}

fn handle_add_name(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => app.input.push(c),
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Enter => {
            let name = app.input.clone();
            app.begin_create(name); // переключит режим на Edit
        }
        KeyCode::Esc => app.mode = Mode::Normal,
        _ => {}
    }
}

fn handle_save_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => app.input.push(c),
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Enter => {
            let name = app.input.clone();
            app.save_hovered_as(name);
            app.mode = Mode::Normal;
        }
        KeyCode::Esc => app.mode = Mode::Normal,
        _ => {}
    }
}

fn handle_confirm_delete(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.delete_selected();
            app.mode = Mode::Normal;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.mode = Mode::Normal,
        _ => {}
    }
}

fn handle_hotkey_menu(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Backspace && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.reset_hotkeys();
        return;
    }
    match key.code {
        KeyCode::Char('1') => {
            app.mode = Mode::CaptureStart;
            app.status = "Нажмите новую комбинацию для СТАРТА (Esc — отмена).".to_string();
        }
        KeyCode::Char('2') => {
            app.mode = Mode::CaptureStop;
            app.status = "Нажмите новую комбинацию для СТОПА (Esc — отмена).".to_string();
        }
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        _ => {}
    }
}
