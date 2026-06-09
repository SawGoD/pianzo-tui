//! Рендер интерфейса.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, EditFocus, Mode, SettingsSection, UpdateState};
use crate::parser::TokenSpan;

/// Цвет нот (превью и непроигранные в караоке) — мягкий бежевый.
const NOTES_COLOR: Color = Color::Rgb(222, 205, 165);

/// Цвет рамок интерфейса: фиолетовый в FOCUSED, белый в UNFOCUSED.
fn theme(app: &App) -> Color {
    if app.focused {
        Color::Magenta
    } else {
        Color::White
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // заголовок
            Constraint::Min(5),    // тело
            Constraint::Length(3), // статус-бар (req: над хоткеями)
            Constraint::Length(3), // хоткеи
        ])
        .split(frame.area());

    draw_title(frame, app, chunks[0]);
    draw_body(frame, app, chunks[1]);
    draw_status(frame, app, chunks[2]);
    draw_footer(frame, app, chunks[3]);

    match app.mode {
        Mode::Edit => draw_edit(frame, app),
        Mode::AddName => draw_name_modal(frame, app, "Название новой мелодии"),
        Mode::SaveBookmark => draw_name_modal(frame, app, "Имя закладки"),
        Mode::ConfirmDelete => draw_confirm_delete(frame, app),
        Mode::Settings => draw_settings(frame, app),
        Mode::CaptureStart => draw_capture(frame, "СТАРТА"),
        Mode::CaptureStop => draw_capture(frame, "СТОПА"),
        Mode::ImportUrl => draw_import_url_modal(frame, app),
        Mode::Normal => {}
    }
}

fn draw_title(frame: &mut Frame, app: &App, area: Rect) {
    let th = theme(app);
    let name = app.current_name.as_deref().unwrap_or("—");
    let focus_label = if app.focused {
        "● FOCUSED"
    } else {
        "○ UNFOCUSED"
    };
    let mut spans = vec![
        Span::styled(
            focus_label,
            Style::default().fg(th).add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled("Pianzo", Style::default().fg(th).add_modifier(Modifier::BOLD)),
        Span::raw("  ·  "),
        Span::styled("♪ заряжено: ", Style::default().fg(Color::Gray)),
        Span::styled(name.to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
    ];
    if let Some(n) = app.countdown {
        spans.push(Span::styled(
            format!("   ⏳ {n}"),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    } else if app.playing {
        let (done, total) = app.progress;
        spans.push(Span::styled(
            format!("   ▶ {done}/{total}"),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ));
    }
    let para = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(th)),
        )
        .alignment(Alignment::Center);
    frame.render_widget(para, area);
}

fn draw_body(frame: &mut Frame, app: &mut App, area: Rect) {
    let th = theme(app);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Левая колонка — закладки. Загруженная подсвечена цветом.
    let items: Vec<ListItem> = app
        .bookmarks
        .iter()
        .map(|b| {
            let loaded = app.current_name.as_deref() == Some(b.name.as_str());
            let (prefix, name_style) = if loaded {
                ("♪ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                ("  ", Style::default())
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Green)),
                Span::styled(b.name.clone(), name_style),
                Span::styled(
                    format!("  ({:.3}/{:.3})", b.between_keys, b.between_lines),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(th))
                .title(" Закладки "),
        )
        .highlight_style(Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD))
        .highlight_symbol("➤ ");
    frame.render_stateful_widget(list, cols[0], &mut app.list_state);

    // Правая колонка — параметры + ноты.
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3)])
        .split(cols[1]);

    // Задержки — наведённой (hovered) закладки.
    let (hk, hl) = app
        .selected_bookmark()
        .map(|b| (b.between_keys, b.between_lines))
        .unwrap_or((0.0, 0.0));
    let params = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Между клавишами: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{hk:.3} c"), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Между строками:  ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{hl:.3} c"), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Громкость:       ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}%", app.volume_pct()), Style::default().fg(Color::Magenta)),
            Span::styled("   [+/-]", Style::default().fg(Color::DarkGray)),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(th))
            .title(" Параметры — [e] правка "),
    );
    frame.render_widget(params, right[0]);

    draw_notes_panel(frame, app, right[1]);
}

/// Панель нот: в покое — превью НАВЕДЁННОЙ закладки; во время игры/теста —
/// караоке-подсветка проигрываемой мелодии.
fn draw_notes_panel(frame: &mut Frame, app: &App, area: Rect) {
    let playing = (app.playing || app.audio_playing) && !app.spans.is_empty();
    let display_notes: &str = if playing {
        &app.play_notes
    } else {
        app.selected_bookmark().map(|b| b.notes.as_str()).unwrap_or("")
    };
    let note_count = display_notes.split_whitespace().count();
    let has_import_meta = !playing
        && app.selected_bookmark()
            .and_then(|b| b.import_meta.as_ref())
            .is_some_and(|m| !m.validated);
    let title = if has_import_meta {
        format!(" Ноты ({note_count}) — [e] правка  [v] валидация ")
    } else {
        format!(" Ноты ({note_count}) — [e] правка ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme(app)))
        .title(title);

    if playing {
        let visible = area.height.saturating_sub(2) as usize;
        let total_lines = app.play_notes.split('\n').count();
        let cur_line = app
            .spans
            .get(app.play_event.min(app.spans.len() - 1))
            .map(|s| s.line)
            .unwrap_or(0);
        // Центрируем текущую строку и не даём прокрутке уйти за пределы текста.
        let max_scroll = total_lines.saturating_sub(visible);
        let scroll = cur_line.saturating_sub(visible / 2).min(max_scroll) as u16;
        let text = build_karaoke(app);
        let para = Paragraph::new(text).block(block).scroll((scroll, 0));
        frame.render_widget(para, area);
    } else {
        // Превью — бежевым.
        let para = Paragraph::new(display_notes)
            .style(Style::default().fg(NOTES_COLOR))
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(para, area);
    }
}

/// Строит подсвеченный текст: сыгранные токены тусклые, текущий — выделен.
fn build_karaoke(app: &App) -> Text<'_> {
    let cur = app.play_event.min(app.spans.len() - 1);
    let cur_line = app.spans.get(cur).map(|s| s.line);

    let src_lines: Vec<&str> = app.play_notes.split('\n').collect();
    let mut offsets = Vec::with_capacity(src_lines.len());
    let mut off = 0usize;
    for l in &src_lines {
        offsets.push(off);
        off += l.len() + 1;
    }

    let played = Style::default().fg(Color::DarkGray);
    let current = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let future = Style::default().fg(NOTES_COLOR);

    // Раскладываем спаны по строкам за один проход (спаны уже идут по порядку).
    let mut buckets: Vec<Vec<(usize, &TokenSpan)>> = vec![Vec::new(); src_lines.len()];
    for (g, s) in app.spans.iter().enumerate() {
        if s.line < buckets.len() {
            buckets[s.line].push((g, s));
        }
    }

    let mut lines: Vec<Line> = Vec::with_capacity(src_lines.len());
    for (li, src) in src_lines.iter().enumerate() {
        let base = offsets[li];
        let toks = &buckets[li];

        let mut spans: Vec<Span> = Vec::new();
        let marker = if Some(li) == cur_line { "▶ " } else { "  " };
        spans.push(Span::styled(marker, Style::default().fg(Color::Green)));

        let mut cursor = 0usize;
        for &(g, ts) in toks {
            // Защита от устаревших спанов (если текст сменили во время игры).
            if ts.start < base || ts.end > base + src.len() || ts.start > ts.end {
                continue;
            }
            let rs = ts.start - base;
            let re = ts.end - base;
            if rs > cursor {
                spans.push(Span::raw(&src[cursor..rs]));
            }
            let style = if g < cur {
                played
            } else if g == cur {
                current
            } else {
                future
            };
            spans.push(Span::styled(&src[rs..re], style));
            cursor = re;
        }
        if cursor < src.len() {
            spans.push(Span::raw(&src[cursor..]));
        }
        lines.push(Line::from(spans));
    }
    Text::from(lines)
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let (left_text, left_color) = if let Some(n) = app.countdown {
        (format!("⏳ Старт через {n}…  (переключитесь в нужное окно)"), Color::Cyan)
    } else if app.playing {
        let (done, total) = app.progress;
        (format!("▶ ВОСПРОИЗВЕДЕНИЕ  {done}/{total}"), Color::Green)
    } else if app.audio_playing {
        (format!("♪ ТЕСТ  ({}%)", app.volume_pct()), Color::Magenta)
    } else {
        (format!("● {}", app.status), Color::Cyan)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme(app)))
        .title(" Статус ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Правая часть: обновление (приоритет) или активное окно.
    let win_indicator: Option<(String, Color)> = if let UpdateState::Available(v) = &app.update_state {
        Some((format!("↑ v{v}"), Color::Green))
    } else if app.process_config.enabled {
        let win_name = app.active_window.as_deref().unwrap_or("—");
        let color = match app.process_config.window_mode(win_name) {
            Some(crate::processes::ProcessMode::Track) => Color::Green,
            Some(crate::processes::ProcessMode::Ignore) => Color::Red,
            _ => Color::DarkGray,
        };
        Some((win_name.to_string(), color))
    } else {
        None
    };

    if let Some((win_name, win_color)) = win_indicator {
        let right_width = (win_name.chars().count() as u16 + 5).min(inner.width.saturating_sub(20));
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(right_width)])
            .split(inner);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                left_text,
                Style::default().fg(left_color).add_modifier(Modifier::BOLD),
            ))),
            cols[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(win_name, Style::default().fg(win_color).add_modifier(Modifier::BOLD)),
            ]))
            .alignment(Alignment::Right),
            cols[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                left_text,
                Style::default().fg(left_color).add_modifier(Modifier::BOLD),
            ))),
            inner,
        );
    }
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let cfg = *app.hotkeys.lock().unwrap();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme(app)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(32),
            Constraint::Percentage(30),
            Constraint::Percentage(38),
        ])
        .split(inner);

    // Слева: старт / стоп + выбор.
    let left = Line::from(vec![
        Span::styled(cfg.start.label(), Style::default().fg(Color::Green)),
        Span::raw(" старт  "),
        Span::styled(cfg.stop.label(), Style::default().fg(Color::Red)),
        Span::raw(" стоп   "),
        Span::styled("↑↓", Style::default().fg(Color::Cyan)),
        Span::raw("/"),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" выбор"),
    ]);

    // По центру: действия с мелодией + звук.
    let center = Line::from(vec![
        Span::styled("t", Style::default().fg(Color::Magenta)),
        Span::raw(" тест  "),
        Span::styled("a", Style::default().fg(Color::Green)),
        Span::raw(" доб.  "),
        Span::styled("e", Style::default().fg(Color::Cyan)),
        Span::raw(" правка  "),
        Span::styled("d", Style::default().fg(Color::Cyan)),
        Span::raw(" удал."),
    ]);

    // Справа: громкость, фокус, настройки, выход.
    let right = Line::from(vec![
        Span::styled("+/-", Style::default().fg(Color::Magenta)),
        Span::raw(" громк.  "),
        Span::styled("u", Style::default().fg(Color::Cyan)),
        Span::raw(" фокус  "),
        Span::styled("s", Style::default().fg(Color::Cyan)),
        Span::raw(" настройки  "),
        Span::styled("q", Style::default().fg(Color::Magenta)),
        Span::raw(" выход"),
    ]);

    frame.render_widget(Paragraph::new(left).alignment(Alignment::Left), cols[0]);
    frame.render_widget(Paragraph::new(center).alignment(Alignment::Center), cols[1]);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), cols[2]);
}

// --- Модальные окна ---

/// Прямоугольник фиксированного размера по центру.
fn modal_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect { x, y, width, height }
}

/// Маленькое окно ввода имени (req 1).
fn draw_name_modal(frame: &mut Frame, app: &App, title: &str) {
    let area = modal_rect(44, 3, frame.area());
    frame.render_widget(Clear, area);
    let para = Paragraph::new(Line::from(vec![
        Span::raw(app.input.as_str()),
        Span::styled("▏", Style::default().fg(Color::Cyan)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(format!(" {title} ")),
    );
    frame.render_widget(para, area);
}

/// Единое окно правки. Раскладка:
///   Имя | Клавиши/Строки
///   ─────────────────────
///   Ноты
///   ─────────────────────
///   Хоткеи
fn draw_edit(frame: &mut Frame, app: &mut App) {
    let th = theme(app);
    let area = modal_rect(
        (frame.area().width * 88) / 100,
        (frame.area().height * 82) / 100,
        frame.area(),
    );
    frame.render_widget(Clear, area);

    let what = if app.creating.is_some() {
        "новая мелодия"
    } else {
        "правка"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th))
        .title(format!(" {what} "));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // имя | клавиши/строки
            Constraint::Length(1), // ── разделитель
            Constraint::Min(3),    // ноты
            Constraint::Length(1), // ── разделитель
            Constraint::Length(1), // хоткеи
        ])
        .split(inner);

    // --- Верх: имя слева, задержки справа (2 строки) ---
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(rows[0]);

    // Имя (редактируемое).
    let name_active = app.edit_focus == EditFocus::Name;
    let name_val_style = if name_active {
        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let mut name_spans = vec![
        focus_marker(name_active, th),
        Span::styled("Имя: ", Style::default().fg(Color::Gray)),
        Span::styled(format!(" {} ", app.input), name_val_style),
    ];
    if name_active {
        name_spans.push(Span::styled("▏", Style::default().fg(th)));
    }
    frame.render_widget(Paragraph::new(Line::from(name_spans)), top[0]);

    // Задержки — 2 строки справа от имени.
    let delays_active = app.edit_focus == EditFocus::Delays;
    let field = |label: &str, value: &str, active: bool, lead: Span<'static>| -> Line<'static> {
        let vs = if active {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        Line::from(vec![
            lead,
            Span::styled(label.to_string(), Style::default().fg(Color::Gray)),
            Span::styled(format!(" {value} "), vs),
        ])
    };
    let delays = Paragraph::new(vec![
        field(
            "Клавиши:",
            &app.input_keys,
            delays_active && app.delay_field == 0,
            focus_marker(delays_active, th),
        ),
        field(
            "Строки: ",
            &app.input_lines,
            delays_active && app.delay_field == 1,
            Span::raw("  "),
        ),
    ]);
    frame.render_widget(delays, top[1]);

    // --- Разделитель ---
    frame.render_widget(
        Block::default().borders(Borders::TOP).border_style(Style::default().fg(th)),
        rows[1],
    );

    // --- Ноты (редактор без своей рамки) ---
    let notes_active = app.edit_focus == EditFocus::Notes;
    if notes_active {
        app.textarea
            .set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
        app.textarea
            .set_cursor_line_style(Style::default().add_modifier(Modifier::UNDERLINED));
    } else {
        app.textarea.set_cursor_style(Style::default());
        app.textarea.set_cursor_line_style(Style::default());
    }
    app.textarea.set_block(Block::default());
    frame.render_widget(&app.textarea, rows[2]);

    // --- Разделитель ---
    frame.render_widget(
        Block::default().borders(Borders::TOP).border_style(Style::default().fg(th)),
        rows[3],
    );

    // --- Хоткеи внизу: ключ ярко, описание тускло ---
    let key = |k: &str| Span::styled(k.to_string(), Style::default().fg(th).add_modifier(Modifier::BOLD));
    let dim = |d: &str| Span::styled(d.to_string(), Style::default().fg(Color::DarkGray));
    let mut hint_spans = vec![
        key("Tab"),
        dim(" поле     "),
        key("Esc"),
        dim(" сохранить     "),
        key("Ctrl+Q"),
        dim(" отмена"),
    ];
    if delays_active {
        hint_spans.push(dim("     "));
        hint_spans.push(key("↑↓"));
        hint_spans.push(dim(" поле  "));
        hint_spans.push(key("←→"));
        hint_spans.push(dim(" ±0.001"));
    }
    frame.render_widget(Paragraph::new(Line::from(hint_spans)), rows[4]);
}

/// Маркер фокуса секции: «▶ » активной, «  » иначе.
fn focus_marker(active: bool, color: Color) -> Span<'static> {
    Span::styled(
        if active { "▶ " } else { "  " },
        Style::default().fg(color),
    )
}

fn draw_confirm_delete(frame: &mut Frame, app: &App) {
    let area = modal_rect(50, 5, frame.area());
    frame.render_widget(Clear, area);
    let name = app
        .selected()
        .and_then(|i| app.bookmarks.get(i))
        .map(|b| b.name.as_str())
        .unwrap_or("?");
    let para = Paragraph::new(vec![
        Line::from(format!("Удалить закладку «{name}»?")),
        Line::from(Span::styled(
            "y — да    n/Esc — нет",
            Style::default().fg(Color::Gray),
        )),
    ])
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title(" Подтверждение "),
    );
    frame.render_widget(para, area);
}

fn settings_modal_rect(area: Rect) -> Rect {
    let width = (area.width * 70) / 100;
    let height = (area.height * 70) / 100;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect { x, y, width, height }
}

fn draw_settings(frame: &mut Frame, app: &App) {
    let area = settings_modal_rect(frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Настройки ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.settings_inside {
        draw_settings_section(frame, app, inner);
    } else {
        draw_settings_list(frame, app, inner);
    }
}

fn draw_settings_list(frame: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    // Левая колонка — список разделов.
    let sections = SettingsSection::all();
    let items: Vec<Line> = sections
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if i == app.settings_selected {
                Line::from(vec![
                    Span::styled("▶ ", Style::default().fg(Color::Cyan)),
                    Span::styled(s.label(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ])
            } else {
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(s.label(), Style::default().fg(Color::White)),
                ])
            }
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::DarkGray));
    let list_inner = list_block.inner(cols[0]);
    frame.render_widget(list_block, cols[0]);
    frame.render_widget(Paragraph::new(items), list_inner);

    // Правая колонка — превью выбранного раздела.
    let preview = settings_section_preview(app, sections[app.settings_selected]);
    let hint = Line::from(Span::styled(
        "↑↓ выбор   →/Enter войти   Esc закрыть",
        Style::default().fg(Color::DarkGray),
    ));

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(cols[1]);

    let padded = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(right_rows[0]);

    frame.render_widget(Paragraph::new(preview), padded[1]);
    frame.render_widget(Paragraph::new(hint).alignment(Alignment::Center), right_rows[1]);
}

fn settings_section_preview(app: &App, section: SettingsSection) -> Vec<Line<'static>> {
    match section {
        SettingsSection::General => {
            let g = app.general;
            vec![
                Line::from(vec![
                    Span::styled("Клавиши:  ", Style::default().fg(Color::Gray)),
                    Span::styled(format!("{:.3} с", g.default_keys), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Строки:   ", Style::default().fg(Color::Gray)),
                    Span::styled(format!("{:.3} с", g.default_lines), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Пауза:    ", Style::default().fg(Color::Gray)),
                    Span::styled(format!("{} сек", g.countdown_secs), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Логи:     ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        if g.logging_enabled { "вкл" } else { "выкл" },
                        if g.logging_enabled { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::DarkGray) },
                    ),
                ]),
            ]
        }
        SettingsSection::Hotkeys => {
            let cfg = *app.hotkeys.lock().unwrap();
            vec![
                Line::from(vec![
                    Span::styled("Старт: ", Style::default().fg(Color::Gray)),
                    Span::styled(cfg.start.label(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("Стоп:  ", Style::default().fg(Color::Gray)),
                    Span::styled(cfg.stop.label(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                ]),
            ]
        }
        SettingsSection::Notifications => {
            let nc = app.notif_config;
            let val = |on: bool| if on { "вкл" } else { "выкл" };
            // Одинаковые названия с разделом — паддинг до 28 символов для выравнивания.
            let row = |label: &'static str, on: bool, color: Color| -> Line<'static> {
                Line::from(vec![
                    Span::styled(format!("{:<28}", format!("{}:", label)), Style::default().fg(Color::Gray)),
                    Span::styled(val(on), Style::default().fg(color)),
                ])
            };
            vec![
                row("Разрешить уведомления",       nc.enabled,    if nc.enabled    { Color::Cyan } else { Color::DarkGray }),
                row("Сейчас играет",               nc.on_playing, if nc.on_playing { Color::Cyan } else { Color::DarkGray }),
                row("Остановлено",                 nc.on_stopped, if nc.on_stopped { Color::Cyan } else { Color::DarkGray }),
                row("Воспроизведение завершено",   nc.on_finished,if nc.on_finished{ Color::Cyan } else { Color::DarkGray }),
                row("Ошибка доступа",              nc.on_error,   if nc.on_error   { Color::Cyan } else { Color::DarkGray }),
            ]
        }
        SettingsSection::Processes => {
            let pc = &app.process_config;
            let status = if pc.enabled { "ВКЛ" } else { "ВЫКЛ" };
            let status_color = if pc.enabled { Color::Cyan } else { Color::DarkGray };
            vec![
                Line::from(vec![
                    Span::styled("Фильтрация: ", Style::default().fg(Color::Gray)),
                    Span::styled(status, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("Процессов: ", Style::default().fg(Color::Gray)),
                    Span::styled(format!("{}", pc.entries.len()), Style::default().fg(Color::Cyan)),
                ]),
            ]
        }
        SettingsSection::Updates => {
            let (state_str, state_color) = match &app.update_state {
                UpdateState::Idle | UpdateState::Checking => ("проверяется…".to_string(), Color::DarkGray),
                UpdateState::Available(v) => (v.clone(), Color::Green),
                UpdateState::UpToDate => ("актуальна".to_string(), Color::Cyan),
                UpdateState::Downloading => ("скачивание…".to_string(), Color::Yellow),
                UpdateState::Done => ("перезапусти".to_string(), Color::Green),
                UpdateState::Error(_) => ("ошибка".to_string(), Color::Red),
            };
            vec![
                Line::from(vec![
                    Span::styled("Версия:    ", Style::default().fg(Color::Gray)),
                    Span::styled(env!("CARGO_PKG_VERSION"), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Доступна:  ", Style::default().fg(Color::Gray)),
                    Span::styled(state_str, Style::default().fg(state_color).add_modifier(Modifier::BOLD)),
                ]),
            ]
        }
    }
}

fn draw_settings_section(frame: &mut Frame, app: &App, area: Rect) {
    let sections = SettingsSection::all();
    let section = sections[app.settings_selected];

    // Строка-хлебная крошка: < На главную · **Хоткеи**
    let breadcrumb = Line::from(vec![
        Span::styled("< ", Style::default().fg(Color::DarkGray)),
        Span::styled("На главную", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
        Span::styled("  ·  ", Style::default().fg(Color::DarkGray)),
        Span::styled(section.label(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // хлебная крошка
            Constraint::Length(1), // разделитель
            Constraint::Min(1),    // содержимое
            Constraint::Length(1), // подсказка
        ])
        .split(area);

    frame.render_widget(Paragraph::new(breadcrumb), rows[0]);
    frame.render_widget(
        Block::default().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray)),
        rows[1],
    );

    // Разбиваем область содержимого на левую (список) и правую (описание).
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(rows[2]);

    let desc_block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));
    let desc_inner = desc_block.inner(cols[1]);
    frame.render_widget(desc_block, cols[1]);

    match section {
        SettingsSection::General => {
            let g = app.general;
            let sel = app.settings_item;
            let editing = app.settings_editing;

            // Метка child+marker = 2+4+label, метка top = 2+label.
            // Самая длинная строка: "Пауза перед воспроизведением" = 28 → с маркером 30.
            // child label pad = 30 - 2 - 4 = 24, top label pad = 30 - 2 = 28.
            const C_PAD: usize = 24;
            const T_PAD: usize = 28;

            let value_style = |idx: usize| -> Style {
                if sel == idx && editing {
                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                }
            };
            let child_row = |label: &'static str, value: String, idx: usize| -> Line<'static> {
                let selected = sel == idx;
                let marker = if selected { Span::styled("▶ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
                let label_style = if selected { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::White) };
                Line::from(vec![
                    marker,
                    Span::styled(format!("  · {:<C_PAD$}", label), label_style),
                    Span::styled(value, value_style(idx)),
                ])
            };
            let top_row = |label: &'static str, value: String, idx: usize| -> Line<'static> {
                let selected = sel == idx;
                let marker = if selected { Span::styled("▶ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
                let label_style = if selected { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::White) };
                Line::from(vec![
                    marker,
                    Span::styled(format!("{:<T_PAD$}", label), label_style),
                    Span::styled(value, value_style(idx)),
                ])
            };

            let log_toggle = if g.logging_enabled {
                Span::styled(" ВКЛ ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(" ВЫКЛ", Style::default().fg(Color::DarkGray))
            };
            let log_marker = if sel == 3 { Span::styled("▶ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
            let log_label_style = if sel == 3 { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::White) };
            let log_row = Line::from(vec![
                log_marker,
                Span::styled(format!("{:<T_PAD$}", "Ведение логов"), log_label_style),
                log_toggle,
            ]);

            let content = vec![
                Line::from(Span::styled("  Задержка по умолчанию", Style::default().fg(Color::DarkGray))),
                child_row("Между клавишами", format!("{:.3} с", g.default_keys),  0),
                child_row("Между строками",  format!("{:.3} с", g.default_lines), 1),
                Line::from(""),
                top_row("Пауза перед воспроизведением", format!("{} сек", g.countdown_secs), 2),
                Line::from(""),
                log_row,
            ];
            frame.render_widget(Paragraph::new(content), cols[0]);

            let desc_text = match sel {
                0 => "Задержка между нажатиями клавиш при создании новой мелодии.",
                1 => "Задержка между строками нот при создании новой мелодии.",
                2 => "Через сколько секунд начнётся воспроизведение после нажатия хоткея.",
                3 => "Записывает отладочную информацию в файл ~/Documents/Pianzo/pianzo-tui.log. Отключай когда не нужна диагностика.",
                _ => "",
            };
            frame.render_widget(
                Paragraph::new(desc_text)
                    .style(Style::default().fg(Color::DarkGray))
                    .wrap(Wrap { trim: false }),
                desc_inner,
            );

            let hint = if editing {
                Line::from(Span::styled(
                    "←/→ изменить   Enter/Esc подтвердить",
                    Style::default().fg(Color::Cyan),
                ))
            } else {
                Line::from(Span::styled(
                    "↑↓ выбор   Enter/→ изменить   Enter/Пробел (логи)   ←/Esc назад",
                    Style::default().fg(Color::DarkGray),
                ))
            };
            frame.render_widget(Paragraph::new(hint), rows[3]);
        }
        SettingsSection::Hotkeys => {
            let cfg = *app.hotkeys.lock().unwrap();
            let item = |label: &'static str, value: String, color: Color, selected: bool| -> Line<'static> {
                let (marker, label_style) = if selected {
                    ("▶ ", Style::default().fg(Color::Cyan))
                } else {
                    ("  ", Style::default().fg(Color::Gray))
                };
                Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Cyan)),
                    Span::styled(label, label_style),
                    Span::styled(value, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                ])
            };
            let content = vec![
                item("Старт: ", cfg.start.label(), Color::Green, app.settings_item == 0),
                item("Стоп:  ", cfg.stop.label(), Color::Red,   app.settings_item == 1),
            ];
            frame.render_widget(Paragraph::new(content), cols[0]);

            let desc_text = match app.settings_item {
                0 => "Глобальный хоткей для запуска воспроизведения нот в активном окне.",
                1 => "Глобальный хоткей для остановки воспроизведения.",
                _ => "",
            };
            frame.render_widget(
                Paragraph::new(desc_text)
                    .style(Style::default().fg(Color::DarkGray))
                    .wrap(Wrap { trim: false }),
                desc_inner,
            );

            let hint = Line::from(Span::styled(
                "↑↓ выбор   Enter/→ переназначить   Ctrl+Backspace сброс   ←/Esc назад",
                Style::default().fg(Color::DarkGray),
            ));
            frame.render_widget(Paragraph::new(hint), rows[3]);
        }
        SettingsSection::Notifications => {
            let nc = app.notif_config;
            let toggle = |on: bool| -> Span<'static> {
                if on {
                    Span::styled(" ВКЛ ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled(" ВЫКЛ", Style::default().fg(Color::DarkGray))
                }
            };
            let sel = app.settings_item;

            const G_PAD: usize = 29;
            const C_PAD: usize = 25;

            let global_marker = if sel == 0 { Span::styled("▶ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
            let global_label_style = if sel == 0 { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::White) };
            let global_label = Span::styled(format!("{:<G_PAD$}", "Разрешить уведомления"), global_label_style);

            let child = |label: &str, on: bool, idx: usize| -> Line<'_> {
                let selected = sel == idx;
                let active = nc.enabled;
                let prefix_style = if selected && active { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::DarkGray) };
                let label_style = if !active { Style::default().fg(Color::DarkGray) } else if selected { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::White) };
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(if selected { "▶ · " } else { "  · " }, prefix_style),
                    Span::styled(format!("{:<C_PAD$}", label), label_style),
                    toggle(on && active),
                ])
            };

            let content = vec![
                Line::from(vec![global_marker, global_label, toggle(nc.enabled)]),
                child("Сейчас играет",             nc.on_playing,  1),
                child("Остановлено",               nc.on_stopped,  2),
                child("Воспроизведение завершено",  nc.on_finished, 3),
                child("Ошибка доступа",            nc.on_error,    4),
            ];
            frame.render_widget(Paragraph::new(content), cols[0]);

            let desc_text = match sel {
                0 => "Главный переключатель. Отключает все уведомления сразу.",
                1 => "Появляется в момент начала воспроизведения нот.",
                2 => "Появляется при остановке воспроизведения по хоткею.",
                3 => "Появляется когда ноты доиграли до конца.",
                4 => "Появляется если macOS заблокировала Accessibility или Input Monitoring.",
                _ => "",
            };
            frame.render_widget(
                Paragraph::new(desc_text)
                    .style(Style::default().fg(Color::DarkGray))
                    .wrap(Wrap { trim: false }),
                desc_inner,
            );

            let hint = Line::from(Span::styled(
                "↑↓ выбор   Enter/Пробел переключить   ←/Esc назад",
                Style::default().fg(Color::DarkGray),
            ));
            frame.render_widget(Paragraph::new(hint), rows[3]);
        }
        SettingsSection::Processes => {
            draw_settings_processes(frame, app, cols[0], desc_inner, rows[3]);
        }
        SettingsSection::Updates => {
            draw_settings_updates(frame, app, cols[0], desc_inner, rows[3]);
        }
    }
}

fn draw_settings_updates(frame: &mut Frame, app: &App, left: Rect, desc: Rect, hint_area: Rect) {
    let current = env!("CARGO_PKG_VERSION");
    let sel = app.settings_item;
    let has_update = matches!(app.update_state, UpdateState::Available(_));
    let is_downloading = matches!(app.update_state, UpdateState::Downloading);

    let (status_line, status_color) = match &app.update_state {
        UpdateState::Idle | UpdateState::Checking =>
            ("Проверяется…".to_string(), Color::DarkGray),
        UpdateState::Available(v) =>
            (format!("Доступна v{v}!"), Color::Green),
        UpdateState::UpToDate =>
            ("Версия актуальна".to_string(), Color::Cyan),
        UpdateState::Downloading =>
            ("Скачивание…".to_string(), Color::Yellow),
        UpdateState::Done =>
            ("Установлено. Перезапусти приложение.".to_string(), Color::Green),
        UpdateState::Error(e) =>
            (format!("Ошибка: {e}"), Color::Red),
    };

    let btn = |label: &'static str, idx: usize, active: bool| -> Line<'static> {
        let selected = sel == idx;
        let marker = if selected { Span::styled("▶ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
        let style = if !active {
            Style::default().fg(Color::DarkGray)
        } else if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        Line::from(vec![marker, Span::styled(label, style)])
    };

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("Текущая версия:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("v{current}"), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Статус:          ", Style::default().fg(Color::DarkGray)),
            Span::styled(status_line, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        btn("Проверить обновления", 0, !is_downloading),
    ];
    if has_update {
        if let UpdateState::Available(v) = &app.update_state {
            let label = format!("Обновить до v{v}");
            let selected = sel == 1;
            let marker = if selected { Span::styled("▶ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
            let style = if selected {
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            lines.push(Line::from(vec![marker, Span::styled(label, style)]));
        }
    }
    frame.render_widget(Paragraph::new(lines), left);

    let desc_text = match sel {
        0 => "Проверяет GitHub Releases на наличие новой версии.\n\nТребует интернет-соединения. git не нужен.",
        1 => "Скачивает и устанавливает новую версию.\n\nПосле установки необходимо перезапустить приложение.",
        _ => "",
    };
    frame.render_widget(
        Paragraph::new(desc_text)
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false }),
        desc,
    );

    let hint = Line::from(Span::styled(
        "↑↓ выбор   Enter выполнить   ←/Esc назад",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(hint), hint_area);
}

fn draw_settings_processes(
    frame: &mut Frame,
    app: &App,
    left: Rect,
    desc: Rect,
    hint_area: Rect,
) {
    use crate::processes::ProcessMode;

    let pc = &app.process_config;
    let sel = app.settings_item;
    let in_search = app.proc_in_search;

    // --- Левая колонка ---
    let toggle_span = if pc.enabled {
        Span::styled(" ВКЛ ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" ВЫКЛ", Style::default().fg(Color::DarkGray))
    };
    let toggle_marker = if sel == 0 && !in_search {
        Span::styled("▶ ", Style::default().fg(Color::Cyan))
    } else {
        Span::raw("  ")
    };
    let toggle_label_style = if sel == 0 && !in_search {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };
    let toggle_row = Line::from(vec![
        toggle_marker,
        Span::styled(format!("{:<28}", "Фильтрация по процессам"), toggle_label_style),
        toggle_span,
    ]);

    // Строка поиска.
    let search_marker = if sel == 1 && !in_search {
        Span::styled("▶ ", Style::default().fg(Color::Cyan))
    } else {
        Span::raw("  ")
    };
    let search_style = if in_search {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else if sel == 1 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let search_text = if in_search {
        format!(" {} ", app.proc_search)
    } else {
        " Поиск процесса... ".to_string()
    };
    let search_row = Line::from(vec![
        search_marker,
        Span::styled("[ ", Style::default().fg(Color::DarkGray)),
        Span::styled(search_text, search_style),
        Span::styled(" ]", Style::default().fg(Color::DarkGray)),
    ]);

    let mut lines: Vec<Line> = vec![
        toggle_row,
        Line::from(""),
        search_row,
    ];

    // Дропдаун — только в режиме ввода.
    if in_search {
        if app.proc_filtered.is_empty() && !app.proc_search.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (ничего не найдено)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (i, name) in app.proc_filtered.iter().enumerate() {
                let selected = i == app.proc_dropdown_sel;
                if selected {
                    lines.push(Line::from(vec![
                        Span::styled("  ▶ ", Style::default().fg(Color::Cyan)),
                        Span::styled(name.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(name.clone(), Style::default().fg(Color::White)),
                    ]));
                }
            }
        }
    } else if !pc.entries.is_empty() {
        // Список добавленных процессов.
        lines.push(Line::from(Span::styled(
            "  ──────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )));
        for (i, entry) in pc.entries.iter().enumerate() {
            let item_idx = 2 + i;
            let selected = sel == item_idx;
            let marker = if selected {
                Span::styled("▶ ", Style::default().fg(Color::Cyan))
            } else {
                Span::raw("  ")
            };
            let name_style = if selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            let (badge_text, badge_style) = match entry.mode {
                ProcessMode::Track => (
                    " ОТС ",
                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                ProcessMode::Ignore => (
                    " ИГН ",
                    Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                ProcessMode::None => (
                    "  —  ",
                    Style::default().fg(Color::DarkGray),
                ),
            };
            lines.push(Line::from(vec![
                marker,
                Span::styled(format!("{:<28}", entry.name.as_str()), name_style),
                Span::styled(badge_text, badge_style),
            ]));
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Список пуст. Найдите процесс выше.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Paragraph::new(lines), left);

    // --- Правая колонка: описание ---
    let desc_text: &str = if in_search {
        "Введите часть имени процесса.\n\n↑↓ — выбор в списке\nEnter — добавить\nEsc — отмена"
    } else {
        match sel {
            0 => "Включает фильтрацию по процессам.\n\nКогда ВКЛ — воспроизведение разрешается или блокируется в зависимости от активного окна.\n\nКогда ВЫКЛ — работает только FOCUSED/UNFOCUSED.",
            1 => "Поиск по запущенным процессам.\n\nНажмите Enter чтобы начать ввод.",
            idx if idx >= 2 => {
                let entry_idx = idx - 2;
                match pc.entries.get(entry_idx).map(|e| e.mode) {
                    Some(ProcessMode::Track) => "Режим: ОТСЛЕЖИВАТЬ\n\nВоспроизведение разрешено когда этот процесс в фокусе.\n\nEnter — сменить режим\nDel/d — удалить из списка",
                    Some(ProcessMode::Ignore) => "Режим: ИГНОРИРОВАТЬ\n\nВоспроизведение заблокировано когда этот процесс в фокусе.\n\nEnter — сменить режим\nDel/d — удалить из списка",
                    _ => "Режим: НЕ ЗАДАНО\n\nПроцесс добавлен, но режим не выбран. Используется FOCUSED/UNFOCUSED.\n\nEnter — сменить режим\nDel/d — удалить из списка",
                }
            }
            _ => "",
        }
    };
    frame.render_widget(
        Paragraph::new(desc_text)
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false }),
        desc,
    );

    // --- Подсказка внизу ---
    let hint = if in_search {
        Line::from(Span::styled(
            "Ввод — поиск   ↑↓ выбор   Enter добавить   Esc отмена",
            Style::default().fg(Color::Cyan),
        ))
    } else {
        Line::from(Span::styled(
            "↑↓ выбор   Enter изменить   Del/d удалить   ←/Esc назад",
            Style::default().fg(Color::DarkGray),
        ))
    };
    frame.render_widget(Paragraph::new(hint), hint_area);
}

fn draw_import_url_modal(frame: &mut Frame, app: &App) {
    let height = if app.import_error.is_some() { 5 } else { 5 };
    let area = modal_rect(70, height, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = vec![
        Line::from(vec![
            Span::raw(app.input.as_str()),
            Span::styled("▏", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Enter — импорт  Esc — отмена",
            Style::default().fg(Color::Gray),
        )),
    ];
    if let Some(err) = &app.import_error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Red),
        )));
    }

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Импорт по URL  (virtualpiano.net) "),
    );
    frame.render_widget(para, area);
}

fn draw_capture(frame: &mut Frame, target: &str) {
    let area = modal_rect(50, 4, frame.area());
    frame.render_widget(Clear, area);
    let para = Paragraph::new(vec![
        Line::from(format!("Нажмите комбинацию для {target}")),
        Line::from(Span::styled("Esc — отмена", Style::default().fg(Color::Gray))),
    ])
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Перепривязка "),
    );
    frame.render_widget(para, area);
}
