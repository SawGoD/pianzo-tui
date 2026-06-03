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
    /// Меню настроек (двухколоночный попап).
    Settings,
    /// Захват новой комбинации для старта.
    CaptureStart,
    /// Захват новой комбинации для стопа.
    CaptureStop,
}

/// Разделы в меню настроек.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsSection {
    Hotkeys,
    Notifications,
}

impl SettingsSection {
    pub fn all() -> &'static [SettingsSection] {
        &[SettingsSection::Hotkeys, SettingsSection::Notifications]
    }

    pub fn label(self) -> &'static str {
        match self {
            SettingsSection::Hotkeys => "Хоткеи",
            SettingsSection::Notifications => "Уведомления",
        }
    }
}

/// Фокус внутри окна правки.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditFocus {
    Name,
    Notes,
    Delays,
}

impl EditFocus {
    /// Следующий фокус по Tab.
    pub fn next(self) -> Self {
        match self {
            EditFocus::Name => EditFocus::Notes,
            EditFocus::Notes => EditFocus::Delays,
            EditFocus::Delays => EditFocus::Name,
        }
    }

    /// Предыдущий фокус по Shift+Tab.
    pub fn prev(self) -> Self {
        match self {
            EditFocus::Name => EditFocus::Delays,
            EditFocus::Notes => EditFocus::Name,
            EditFocus::Delays => EditFocus::Notes,
        }
    }
}

pub struct App {
    pub bookmarks: Vec<Bookmark>,
    pub list_state: ListState,

    /// «Заряженная» по Enter мелодия — её играет ГЛОБАЛЬНЫЙ старт.
    pub notes: String,
    pub between_keys: f64,
    pub between_lines: f64,
    /// Имя заряженной (Enter) закладки, если есть.
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
    /// Если задано — правится существующая закладка с этим именем (hovered).
    pub editing: Option<String>,
    /// Редактор нот.
    pub textarea: TextArea<'static>,

    /// Состояние меню настроек: выбранный раздел, признак «внутри раздела»,
    /// и выбранный пункт внутри раздела.
    pub settings_selected: usize,
    pub settings_inside: bool,
    pub settings_item: usize,

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
    /// Ноты мелодии, которая сейчас играет/тестируется (для караоке).
    pub play_notes: String,
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
            editing: None,
            textarea: TextArea::default(),
            settings_selected: 0,
            settings_inside: false,
            settings_item: 0,
            hotkeys: Arc::new(Mutex::new(config)),
            volume: Arc::new(Mutex::new(volume)),
            focused: true,
            status: String::new(),
            playing: false,
            audio_playing: false,
            countdown: None,
            progress: (0, 0),
            play_notes: String::new(),
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
    }

    /// Наведённая (hovered) закладка — с ней работают правка/удаление/тест.
    pub fn selected_bookmark(&self) -> Option<&Bookmark> {
        self.list_state.selected().and_then(|i| self.bookmarks.get(i))
    }

    /// «Заряжает» закладку по Enter — её играет глобальный старт.
    pub fn load_bookmark(&mut self, idx: usize) {
        if let Some(b) = self.bookmarks.get(idx).cloned() {
            self.notes = b.notes;
            self.between_keys = b.between_keys;
            self.between_lines = b.between_lines;
            self.current_name = Some(b.name.clone());
            self.status = format!("Заряжено (играет старт): {}", b.name);
        }
    }

    /// Открывает окно правки для НАВЕДЁННОЙ закладки.
    pub fn begin_edit(&mut self) {
        let Some(b) = self.selected_bookmark().cloned() else {
            self.status = "Нет закладки для правки.".to_string();
            return;
        };
        self.textarea = TextArea::new(b.notes.lines().map(String::from).collect());
        self.input = b.name.clone();
        self.input_keys = format!("{:.3}", b.between_keys);
        self.input_lines = format!("{:.3}", b.between_lines);
        self.delay_field = 0;
        self.edit_focus = EditFocus::Delays;
        self.creating = None;
        self.editing = Some(b.name);
        self.mode = Mode::Edit;
    }

    /// Начинает создание новой мелодии: окно правки с пустыми нотами.
    pub fn begin_create(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.status = "Имя не может быть пустым".to_string();
            self.mode = Mode::Normal;
            return;
        }
        self.textarea = TextArea::default();
        self.input = name.clone();
        self.input_keys = "0.110".to_string();
        self.input_lines = "0.110".to_string();
        self.delay_field = 0;
        self.edit_focus = EditFocus::Notes;
        self.creating = Some(name);
        self.editing = None;
        self.mode = Mode::Edit;
    }

    /// Сохраняет правки (имя + ноты + задержки). Имя можно менять прямо в окне.
    pub fn commit_edit(&mut self) {
        let new_name = self.input.trim().to_string();
        if new_name.is_empty() {
            self.status = "Имя не может быть пустым — исправь и сохрани.".to_string();
            return; // не закрываем окно
        }
        let notes = self.textarea.lines().join("\n");
        let (bk, bl) = self.parse_delay_bufs();
        let creating = self.creating.take();
        let editing = self.editing.take();

        // Переименование: правили существующую под другим именем.
        if let Some(orig) = editing {
            if orig != new_name {
                let _ = storage::delete_bookmark(&orig);
                self.bookmarks.retain(|b| b.name != orig);
                if self.current_name.as_deref() == Some(orig.as_str()) {
                    self.current_name = Some(new_name.clone());
                }
            }
        }
        let _ = creating; // создание — просто сохраняем под new_name

        self.store_bookmark(new_name, notes, bk, bl);
        self.mode = Mode::Normal;
    }

    pub fn cancel_edit(&mut self) {
        self.creating = None;
        self.editing = None;
        self.mode = Mode::Normal;
        self.status = "Правка отменена.".to_string();
    }

    /// Сохраняет/обновляет закладку и обновляет список.
    /// Если правится «заряженная» мелодия — обновляет и её рабочую копию.
    fn store_bookmark(&mut self, name: String, notes: String, bk: f64, bl: f64) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.status = "Имя закладки не может быть пустым".to_string();
            return;
        }
        let bookmark = Bookmark {
            name: name.clone(),
            notes: notes.clone(),
            between_keys: bk,
            between_lines: bl,
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
        self.bookmarks.sort_by_key(|b| b.name.to_lowercase());
        if let Some(idx) = self.bookmarks.iter().position(|b| b.name == name) {
            self.list_state.select(Some(idx));
        }
        // Если это заряженная мелодия — синхронизируем рабочую копию для старта.
        if self.current_name.as_deref() == Some(name.as_str()) {
            self.notes = notes;
            self.between_keys = bk;
            self.between_lines = bl;
        }
        self.status = format!("Сохранено в Documents/Pianzo/tracks: {name}");
    }

    /// Сохраняет НАВЕДЁННУЮ закладку под (новым) именем — дубликат/переименование.
    pub fn save_hovered_as(&mut self, name: String) {
        let Some(b) = self.selected_bookmark().cloned() else {
            self.status = "Нет закладки для сохранения.".to_string();
            return;
        };
        self.store_bookmark(name, b.notes, b.between_keys, b.between_lines);
    }

    pub fn delete_selected(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if idx < self.bookmarks.len() {
                let removed = self.bookmarks.remove(idx);
                let _ = storage::delete_bookmark(&removed.name);
                // Если удалили заряженную — снимаем заряд.
                if self.current_name.as_deref() == Some(removed.name.as_str()) {
                    self.current_name = None;
                    self.notes.clear();
                }
                self.status = format!("Удалено: {}", removed.name);
                if self.bookmarks.is_empty() {
                    self.list_state.select(None);
                } else {
                    self.list_state
                        .select(Some(idx.min(self.bookmarks.len() - 1)));
                }
            }
        }
    }

    /// Парсит поля задержек (запятая → точка), при ошибке — дефолт 0.110.
    fn parse_delay_bufs(&self) -> (f64, f64) {
        let p = |s: &str| {
            s.trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|v| *v >= 0.0)
                .unwrap_or(0.110)
        };
        (p(&self.input_keys), p(&self.input_lines))
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
