mod app;
mod audio;
mod debug;
mod general;
mod hotkeys;
mod notifications;
mod parser;
mod player;
mod processes;
mod storage;
mod ui;
mod window_tracker;

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use app::{App, EditFocus, Mode, SettingsSection};
use audio::{AudioMsg, AudioRequest};
use hotkeys::HotkeyCmd;
use player::{PlayRequest, PlayerMsg};

fn main() -> io::Result<()> {
    let (hk_tx, hk_rx) = mpsc::channel::<HotkeyCmd>();
    let (pl_tx, pl_rx) = mpsc::channel::<PlayerMsg>();
    let (play_tx, play_rx) = mpsc::channel::<PlayRequest>();
    let (audio_tx, audio_rx) = mpsc::channel::<AudioRequest>();
    let (amsg_tx, amsg_rx) = mpsc::channel::<AudioMsg>();
    let (wt_tx, wt_rx) = mpsc::channel::<Option<String>>();
    let stop = Arc::new(AtomicBool::new(false));

    let mut app = App::new();
    debug::log("=== запуск Pianzo ===");

    // Слушатель глобальной клавиатуры с общим конфигом хоткеев.
    hotkeys::spawn(hk_tx, Arc::clone(&app.hotkeys));
    // Постоянный поток воспроизведения клавиш.
    player::spawn(play_rx, Arc::clone(&stop), pl_tx);
    // Постоянный аудио-поток.
    audio::spawn(audio_rx, Arc::clone(&stop), amsg_tx, Arc::clone(&app.volume));
    // Трекер активного окна.
    window_tracker::spawn(wt_tx);

    let mut terminal = ratatui::init();

    // Логируем панику (и даём ratatui восстановить терминал — это предыдущий хук).
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        debug::log(&format!("PANIC: {info}"));
        prev_hook(info);
    }));

    let result = run(&mut terminal, &mut app, &hk_rx, &pl_rx, &play_tx, &amsg_rx, &audio_tx, &wt_rx, &stop);
    ratatui::restore();
    debug::log("=== выход Pianzo ===");
    result
}

#[allow(clippy::too_many_arguments)]
fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    hk_rx: &Receiver<HotkeyCmd>,
    pl_rx: &Receiver<PlayerMsg>,
    play_tx: &Sender<PlayRequest>,
    amsg_rx: &Receiver<AudioMsg>,
    audio_tx: &Sender<AudioRequest>,
    wt_rx: &Receiver<Option<String>>,
    stop: &Arc<AtomicBool>,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Обновляем активное окно из трекера.
        while let Ok(win) = wt_rx.try_recv() {
            app.active_window = win;
        }

        while let Ok(cmd) = hk_rx.try_recv() {
            match cmd {
                // Старт разрешён в обычном режиме; can_start_play() учитывает
                // FOCUSED/UNFOCUSED и правила фильтрации процессов.
                HotkeyCmd::Start if app.can_start_play() && app.mode == Mode::Normal => {
                    start_playback(app, stop, play_tx)
                }
                HotkeyCmd::Start => {
                    debug::log("main: старт проигнорирован (UNFOCUSED/процесс/модальное окно)");
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
                    if app.notif_config.enabled && app.notif_config.on_finished {
                        if let Some(name) = &app.current_name {
                            notifications::finished(name);
                        }
                    }
                }
                PlayerMsg::Stopped(at) => {
                    app.playing = false;
                    app.countdown = None;
                    app.status = format!("Остановлено на позиции {at}.");
                    if app.notif_config.enabled && app.notif_config.on_stopped {
                        if let Some(name) = &app.current_name {
                            notifications::stopped(name);
                        }
                    }
                }
                PlayerMsg::Error(e) => {
                    app.playing = false;
                    app.countdown = None;
                    app.status = format!("{e}. Разрешите Accessibility в System Settings.");
                    if app.notif_config.enabled && app.notif_config.on_error {
                        notifications::access_error();
                    }
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

fn start_playback(app: &mut App, stop: &Arc<AtomicBool>, play_tx: &Sender<PlayRequest>) {
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
    let countdown = app.general.countdown_secs;
    app.countdown = Some(countdown);
    app.status = format!("Старт через {}…", countdown);
    if app.notif_config.enabled && app.notif_config.on_playing {
        if let Some(name) = &app.current_name {
            notifications::playing(name);
        }
    }

    debug::log(&format!(
        "main: запрос воспроизведения «{}», событий: {}",
        app.current_name.as_deref().unwrap_or("—"),
        parsed.events.len()
    ));

    if play_tx.send(PlayRequest { events: parsed.events, countdown_secs: countdown }).is_err() {
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
        KeyCode::Char('q') if ctrl => return app.cancel_edit(),
        KeyCode::Tab => {
            app.edit_focus = app.edit_focus.next();
            return;
        }
        KeyCode::BackTab => {
            app.edit_focus = app.edit_focus.prev();
            return;
        }
        _ => {}
    }

    match app.edit_focus {
        EditFocus::Name => match key.code {
            KeyCode::Char(c) => app.input.push(c),
            KeyCode::Backspace => {
                app.input.pop();
            }
            _ => {}
        },
        EditFocus::Notes => {
            app.textarea.input(ev);
        }
        EditFocus::Delays => match key.code {
            KeyCode::Up | KeyCode::Down => app.delay_field ^= 1,
            KeyCode::Right => app.nudge_delay(0.001),
            KeyCode::Left => app.nudge_delay(-0.001),
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
        app.mode = Mode::Settings;
        app.status = "Перепривязка отменена.".to_string();
        return;
    }
    match hotkeys::spec_from_crossterm(key) {
        Some(spec) => {
            let start = app.mode == Mode::CaptureStart;
            app.set_hotkey(start, spec);
            app.mode = Mode::Settings;
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
    play_tx: &Sender<PlayRequest>,
    audio_tx: &Sender<AudioRequest>,
) {
    match app.mode {
        Mode::Normal => handle_normal(app, key, stop, play_tx, audio_tx),
        Mode::AddName => handle_add_name(app, key),
        Mode::SaveBookmark => handle_save_input(app, key),
        Mode::ConfirmDelete => handle_confirm_delete(app, key),
        Mode::Settings => handle_settings(app, key),
        Mode::Edit | Mode::CaptureStart | Mode::CaptureStop => {}  // обрабатываются отдельно
    }
}

fn handle_normal(
    app: &mut App,
    key: KeyEvent,
    stop: &Arc<AtomicBool>,
    play_tx: &Sender<PlayRequest>,
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
        KeyCode::Char('e') => app.begin_edit(),
        KeyCode::Char('s') => {
            app.settings_selected = 0;
            app.settings_inside = false;
            app.mode = Mode::Settings;
        }
        KeyCode::Char('S') => {
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

fn handle_settings(app: &mut App, key: KeyEvent) {
    let sections = SettingsSection::all();

    if !app.settings_inside {
        // Список разделов — навигация вверх/вниз, вправо/Enter — войти, Esc — закрыть.
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if app.settings_selected > 0 {
                    app.settings_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.settings_selected + 1 < sections.len() {
                    app.settings_selected += 1;
                }
            }
            KeyCode::Right | KeyCode::Enter => {
                app.settings_inside = true;
            }
            KeyCode::Esc => app.mode = Mode::Normal,
            _ => {}
        }
        return;
    }

    // Внутри раздела.
    let section = sections[app.settings_selected];
    match section {
        SettingsSection::General => {
            if app.settings_editing {
                // Режим редактирования числовых значений (items 0–2).
                match key.code {
                    KeyCode::Left => {
                        match app.settings_item {
                            0 => app.general.nudge_keys(-0.001),
                            1 => app.general.nudge_lines(-0.001),
                            2 => app.general.nudge_countdown(-1),
                            _ => {}
                        }
                        let _ = app.persist_config_pub();
                    }
                    KeyCode::Right => {
                        match app.settings_item {
                            0 => app.general.nudge_keys(0.001),
                            1 => app.general.nudge_lines(0.001),
                            2 => app.general.nudge_countdown(1),
                            _ => {}
                        }
                        let _ = app.persist_config_pub();
                    }
                    KeyCode::Enter | KeyCode::Esc => {
                        app.settings_editing = false;
                    }
                    _ => {}
                }
            } else {
                // Навигация по пунктам.
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.settings_item > 0 { app.settings_item -= 1; }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if app.settings_item < 3 { app.settings_item += 1; }
                    }
                    KeyCode::Enter | KeyCode::Char(' ') if app.settings_item == 3 => {
                        app.general.logging_enabled = !app.general.logging_enabled;
                        crate::debug::set_enabled(app.general.logging_enabled);
                        let _ = app.persist_config_pub();
                    }
                    KeyCode::Right | KeyCode::Enter => {
                        if app.settings_item < 3 { app.settings_editing = true; }
                    }
                    KeyCode::Left | KeyCode::Esc => {
                        app.settings_inside = false;
                        app.settings_item = 0;
                    }
                    _ => {}
                }
            }
        }
        SettingsSection::Hotkeys => {
            if key.code == KeyCode::Backspace && key.modifiers.contains(KeyModifiers::CONTROL) {
                app.reset_hotkeys();
                return;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.settings_item > 0 {
                        app.settings_item -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if app.settings_item < 1 {
                        app.settings_item += 1;
                    }
                }
                KeyCode::Enter | KeyCode::Right => {
                    if app.settings_item == 0 {
                        app.mode = Mode::CaptureStart;
                        app.status = "Нажмите новую комбинацию для СТАРТА (Esc — отмена).".to_string();
                    } else {
                        app.mode = Mode::CaptureStop;
                        app.status = "Нажмите новую комбинацию для СТОПА (Esc — отмена).".to_string();
                    }
                }
                KeyCode::Left | KeyCode::Esc => {
                    app.settings_inside = false;
                    app.settings_item = 0;
                    app.settings_editing = false;
                }
                _ => {}
            }
        }
        SettingsSection::Notifications => {
            let max_item = 4;
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.settings_item > 0 {
                        app.settings_item -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if app.settings_item < max_item {
                        app.settings_item += 1;
                    }
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    match app.settings_item {
                        0 => app.notif_config.toggle_global(),
                        1 if app.notif_config.enabled => app.notif_config.toggle_playing(),
                        2 if app.notif_config.enabled => app.notif_config.toggle_stopped(),
                        3 if app.notif_config.enabled => app.notif_config.toggle_finished(),
                        4 if app.notif_config.enabled => app.notif_config.toggle_error(),
                        _ => {}
                    }
                    let _ = app.persist_config_pub();
                }
                KeyCode::Left | KeyCode::Esc => {
                    app.settings_inside = false;
                    app.settings_item = 0;
                    app.settings_editing = false;
                }
                _ => {}
            }
        }
        SettingsSection::Processes => {
            // Режим ввода поиска — все клавиши идут в строку поиска.
            if app.proc_in_search {
                match key.code {
                    KeyCode::Esc => app.proc_exit_search(),
                    KeyCode::Enter => {
                        if !app.proc_filtered.is_empty() {
                            app.proc_add_selected();
                        } else {
                            app.proc_exit_search();
                        }
                    }
                    KeyCode::Up => {
                        if app.proc_dropdown_sel > 0 {
                            app.proc_dropdown_sel -= 1;
                        }
                    }
                    KeyCode::Down => {
                        let max = app.proc_filtered.len().saturating_sub(1);
                        if app.proc_dropdown_sel < max {
                            app.proc_dropdown_sel += 1;
                        }
                    }
                    KeyCode::Backspace => {
                        app.proc_search.pop();
                        app.proc_update_filter();
                    }
                    KeyCode::Char(c) => {
                        app.proc_search.push(c);
                        app.proc_update_filter();
                    }
                    _ => {}
                }
                return;
            }

            // Обычная навигация внутри раздела.
            let entry_count = app.process_config.entries.len();
            let max_item = if entry_count == 0 { 1 } else { 1 + entry_count };
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.settings_item > 0 {
                        app.settings_item -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if app.settings_item < max_item {
                        app.settings_item += 1;
                    }
                }
                KeyCode::Enter | KeyCode::Char(' ') => match app.settings_item {
                    0 => {
                        app.process_config.enabled = !app.process_config.enabled;
                        let _ = app.persist_config_pub();
                    }
                    1 => app.proc_enter_search(),
                    idx => {
                        let entry_idx = idx - 2;
                        app.proc_cycle_mode(entry_idx);
                    }
                },
                KeyCode::Delete | KeyCode::Char('d') => {
                    if app.settings_item >= 2 {
                        let entry_idx = app.settings_item - 2;
                        app.proc_remove_entry(entry_idx);
                        // Скорректировать выбор если удалили последний элемент.
                        let new_max = if app.process_config.entries.is_empty() { 1 } else { 1 + app.process_config.entries.len() };
                        if app.settings_item > new_max {
                            app.settings_item = new_max;
                        }
                    }
                }
                KeyCode::Left | KeyCode::Esc => {
                    app.settings_inside = false;
                    app.settings_item = 0;
                    app.settings_editing = false;
                }
                _ => {}
            }
        }
    }
}
