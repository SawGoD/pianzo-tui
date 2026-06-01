//! Состояние приложения и логика, не зависящая от рендера.

use std::sync::{Arc, Mutex};

use ratatui::widgets::ListState;
use tui_textarea::TextArea;

use crate::hotkeys::HotkeyConfig;
use crate::parser::TokenSpan;
use crate::storage::{self, Bookmark};

/// Текущий режим ввода TUI.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// Навигация по списку закладок.
    Normal,
    /// Ввод имени для новой мелодии (маленькое окно).
    AddName,
    /// Единое окно правки: ноты + задержки (переключение Tab).
    Edit,
    /// Ввод имени при сохранении мелодии без имени.
    SaveBookmark,
    /// Подтверждение удаления закладки.
    ConfirmDelete,
    /// Меню настройки хоткеев.
    HotkeyMenu,
    /// Захват новой комбинации для старта.
    CaptureStart,
    /// Захват новой комбинации для стопа.
    CaptureStop,
}

/// Фокус внутри окна правки.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditFocus {
    Notes,
    Delays,
}

pub struct App {
    pub bookmarks: Vec<Bookmark>,
    pub list_state: ListState,

    /// Текущая (рабочая) мелодия.
    pub notes: String,
    pub between_keys: f64,
    pub between_lines: f64,
    /// Имя загруженной закладки, если есть.
    pub current_name: Option<String>,

    pub mode: Mode,
    /// Буфер однострочного ввода (имя).
    pub input: String,
    /// Буферы редактирования задержек.
    pub input_keys: String,
    pub input_lines: String,
    /// Активное поле задержек: 0 — между клавишами, 1 — между строками.
    pub delay_field: u8,
    /// Фокус в окне правки.
    pub edit_focus: EditFocus,
    /// Если задано — создаётся новая мелодия с этим именем.
    pub creating: Option<String>,
    /// Редактор нот.
    pub textarea: TextArea<'static>,

    /// Конфиг хоткеев (общий со слушателем).
    pub hotkeys: Arc<Mutex<HotkeyConfig>>,
    /// Громкость звука 0.0–1.0 (общая с аудио-потоком).
    pub volume: Arc<Mutex<f32>>,

    /// Активно ли реагирование на глобальный старт-хоткей.
    /// true = FOCUSED (норма), false = UNFOCUSED (старт не ловится вне терминала).
    pub focused: bool,

    pub status: String,
    pub playing: bool,
    /// Идёт ли проигрывание звука (а не нажатие клавиш).
    pub audio_playing: bool,
    /// Если идёт обратный отсчёт перед стартом — осталось секунд.
    pub countdown: Option<u64>,
    pub progress: (usize, usize),
    /// Позиции токенов проигрываемой мелодии (для караоке-подсветки).
    pub spans: Vec<TokenSpan>,
    /// Индекс текущего проигрываемого события.
    pub play_event: usize,

    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let bookmarks = storage::load_bookmarks();
        let (config, volume) = storage::load_config();
        let mut list_state = ListState::default();
        if !bookmarks.is_empty() {
            list_state.select(Some(0));
        }

        let mut app = App {
            bookmarks,
            list_state,
            notes: String::new(),
            between_keys: 0.110,
            between_lines: 0.110,
            current_name: None,
            mode: Mode::Normal,
            input: String::new(),
            input_keys: String::new(),
            input_lines: String::new(),
            delay_field: 0,
            edit_focus: EditFocus::Notes,
            creating: None,
            textarea: TextArea::default(),
            hotkeys: Arc::new(Mutex::new(config)),
            volume: Arc::new(Mutex::new(volume)),
            focused: true,
            status: String::new(),
            playing: false,
            audio_playing: false,
            countdown: None,
            progress: (0, 0),
            spans: Vec::new(),
            play_event: 0,
            should_quit: false,
        };

        if !app.bookmarks.is_empty() {
            app.load_bookmark(0);
        }
        app.status = app.ready_hint();
        app
    }

    /// Подсказка с актуальными хоткеями.
    pub fn ready_hint(&self) -> String {
        let cfg = *self.hotkeys.lock().unwrap();
        format!(
            "Готово. {} — старт, {} — стоп.",
            cfg.start.label(),
            cfg.stop.label()
        )
    }

    pub fn selected(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn select_next(&mut self) {
        if self.bookmarks.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.bookmarks.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
        self.sync_selected();
    }

    pub fn select_prev(&mut self) {
        if self.bookmarks.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(0) | None => self.bookmarks.len() - 1,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(i));
        self.sync_selected();
    }

    /// Делает выделенную (hovered) закладку активной рабочей мелодией.
    /// Вызывается при навигации — так CRUD/тест/старт всегда работают по тому,
    /// что под курсором, а не по «загруженной по Enter».
    pub fn sync_selected(&mut self) {
        if let Some(b) = self
            .list_state
            .selected()
            .and_then(|i| self.bookmarks.get(i))
            .cloned()
        {
            self.notes = b.notes;
            self.between_keys = b.between_keys;
            self.between_lines = b.between_lines;
            self.current_name = Some(b.name);
        }
    }

    /// Загружает закладку в рабочую область (с сообщением в статусе — для Enter).
    pub fn load_bookmark(&mut self, idx: usize) {
        if let Some(b) = self.bookmarks.get(idx).cloned() {
            self.notes = b.notes;
            self.between_keys = b.between_keys;
            self.between_lines = b.between_lines;
            self.current_name = Some(b.name.clone());
            self.status = format!("Загружено: {}", b.name);
        }
    }

    /// Открывает окно правки для текущей мелодии.
    pub fn begin_edit(&mut self) {
        self.textarea = TextArea::new(self.notes.lines().map(String::from).collect());
        self.input_keys = format!("{:.3}", self.between_keys);
        self.input_lines = format!("{:.3}", self.between_lines);
        self.delay_field = 0;
        self.edit_focus = EditFocus::Notes;
        self.creating = None;
        self.mode = Mode::Edit;
    }

    /// Начинает создание новой мелодии: открывает окно правки с пустыми нотами.
    pub fn begin_create(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.status = "Имя не может быть пустым".to_string();
            self.mode = Mode::Normal;
            return;
        }
        self.notes = String::new();
        self.textarea = TextArea::default();
        self.input_keys = format!("{:.3}", self.between_keys);
        self.input_lines = format!("{:.3}", self.between_lines);
        self.delay_field = 0;
        self.edit_focus = EditFocus::Notes;
        self.creating = Some(name);
        self.mode = Mode::Edit;
    }

    /// Сохраняет правки (ноты + задержки). Если имени нет — просит ввести.
    pub fn commit_edit(&mut self) {
        self.notes = self.textarea.lines().join("\n");
        self.apply_delays_silent();
        let name = self.creating.take().or_else(|| self.current_name.clone());
        match name {
            Some(n) => {
                self.save_current_as(n);
                self.mode = Mode::Normal;
            }
            None => {
                self.input.clear();
                self.mode = Mode::SaveBookmark;
            }
        }
    }

    pub fn cancel_edit(&mut self) {
        self.creating = None;
        self.mode = Mode::Normal;
        self.status = "Правка отменена.".to_string();
    }

    /// Сохраняет текущую рабочую мелодию как закладку с именем.
    pub fn save_current_as(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.status = "Имя закладки не может быть пустым".to_string();
            return;
        }
        let bookmark = Bookmark {
            name: name.clone(),
            notes: self.notes.clone(),
            between_keys: self.between_keys,
            between_lines: self.between_lines,
        };
        if let Err(e) = storage::save_bookmark(&bookmark) {
            self.status = format!("Ошибка сохранения: {e}");
            return;
        }
        if let Some(existing) = self.bookmarks.iter_mut().find(|b| b.name == name) {
            *existing = bookmark;
        } else {
            self.bookmarks.push(bookmark);
        }
        self.bookmarks
            .sort_by_key(|b| b.name.to_lowercase());
        if let Some(idx) = self.bookmarks.iter().position(|b| b.name == name) {
            self.list_state.select(Some(idx));
        }
        self.current_name = Some(name.clone());
        self.status = format!("Сохранено в Documents/Piano: {name}");
    }

    pub fn delete_selected(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if idx < self.bookmarks.len() {
                let removed = self.bookmarks.remove(idx);
                let _ = storage::delete_bookmark(&removed.name);
                if self.current_name.as_deref() == Some(removed.name.as_str()) {
                    self.current_name = None;
                }
                self.status = format!("Удалено: {}", removed.name);
                if self.bookmarks.is_empty() {
                    self.list_state.select(None);
                    self.notes.clear();
                    self.current_name = None;
                } else {
                    self.list_state
                        .select(Some(idx.min(self.bookmarks.len() - 1)));
                    self.sync_selected();
                }
            }
        }
    }

    /// Парсит оба поля задержек и применяет (без сообщения об успехе).
    fn apply_delays_silent(&mut self) {
        let parse = |s: &str| s.trim().replace(',', ".").parse::<f64>();
        if let (Ok(k), Ok(l)) = (parse(&self.input_keys), parse(&self.input_lines)) {
            if k >= 0.0 && l >= 0.0 {
                self.between_keys = k;
                self.between_lines = l;
            }
        }
    }

    /// Ссылка на буфер активного поля задержек.
    pub fn active_delay_buf(&mut self) -> &mut String {
        if self.delay_field == 0 {
            &mut self.input_keys
        } else {
            &mut self.input_lines
        }
    }

    /// Изменяет активное поле задержек на `delta` (стрелки вверх/вниз).
    pub fn nudge_delay(&mut self, delta: f64) {
        let buf = self.active_delay_buf();
        let cur = buf.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0);
        let next = (cur + delta).max(0.0);
        *buf = format!("{next:.3}");
    }

    /// Сохраняет текущие хоткеи и громкость в конфиг.
    fn persist_config(&self) -> std::io::Result<()> {
        let cfg = *self.hotkeys.lock().unwrap();
        let vol = *self.volume.lock().unwrap();
        storage::save_config(&cfg, vol)
    }

    /// Сбрасывает хоткеи к значениям по умолчанию и сохраняет конфиг.
    pub fn reset_hotkeys(&mut self) {
        let cfg = HotkeyConfig::default();
        *self.hotkeys.lock().unwrap() = cfg;
        if let Err(e) = self.persist_config() {
            self.status = format!("Ошибка сохранения конфига: {e}");
        } else {
            self.status = format!(
                "Хоткеи сброшены: старт {}, стоп {}",
                cfg.start.label(),
                cfg.stop.label()
            );
        }
    }

    /// Применяет новую привязку хоткея и сохраняет конфиг.
    pub fn set_hotkey(&mut self, start: bool, spec: crate::hotkeys::HotkeySpec) {
        let cfg = {
            let mut guard = self.hotkeys.lock().unwrap();
            if start {
                guard.start = spec;
            } else {
                guard.stop = spec;
            }
            *guard
        };
        if let Err(e) = self.persist_config() {
            self.status = format!("Ошибка сохранения конфига: {e}");
        } else {
            self.status = format!(
                "Хоткей обновлён: старт {}, стоп {}",
                cfg.start.label(),
                cfg.stop.label()
            );
        }
    }

    /// Переключает слежение за глобальным стартом (FOCUSED/UNFOCUSED).
    pub fn toggle_focus(&mut self) {
        self.focused = !self.focused;
        self.status = if self.focused {
            "FOCUSED — старт-хоткей активен.".to_string()
        } else {
            "UNFOCUSED — старт-хоткей отключён (стоп работает).".to_string()
        };
    }

    /// Текущая громкость в процентах.
    pub fn volume_pct(&self) -> u32 {
        (*self.volume.lock().unwrap() * 100.0).round() as u32
    }

    /// Меняет громкость на `delta` (с зажимом 0–1) и сохраняет конфиг.
    pub fn change_volume(&mut self, delta: f32) {
        {
            let mut v = self.volume.lock().unwrap();
            *v = (*v + delta).clamp(0.0, 1.0);
        }
        let _ = self.persist_config();
        self.status = format!("Громкость: {}%", self.volume_pct());
    }
}
