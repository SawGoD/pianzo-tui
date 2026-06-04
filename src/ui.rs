//! Рендер интерфейса.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, EditFocus, Mode, SettingsSection};
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
    let title = format!(" Ноты ({note_count}) — [e] правка ");
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
    let (text, color) = if let Some(n) = app.countdown {
        (format!("⏳ Старт через {n}…  (переключитесь в нужное окно)"), Color::Cyan)
    } else if app.playing {
        let (done, total) = app.progress;
        (format!("▶ ВОСПРОИЗВЕДЕНИЕ  {done}/{total}"), Color::Green)
    } else if app.audio_playing {
        (format!("♪ ТЕСТ  ({}%)", app.volume_pct()), Color::Magenta)
    } else {
        (format!("● {}", app.status), Color::Cyan)
    };
    let para = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme(app)))
            .title(" Статус "),
    );
    frame.render_widget(para, area);
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

fn settings_section_preview<'a>(app: &'a App, section: SettingsSection) -> Vec<Line<'a>> {
    match section {
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

    match section {
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
            frame.render_widget(Paragraph::new(content), rows[2]);
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

            // Префикс global=2, child=6 → toggle на колонке 33.
            // global label pad = 33 - 2 - 2 = 29, child label pad = 33 - 6 - 2 = 25.
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
                child("Сейчас играет",            nc.on_playing,  1),
                child("Остановлено",              nc.on_stopped,  2),
                child("Воспроизведение завершено", nc.on_finished, 3),
                child("Ошибка доступа",           nc.on_error,    4),
            ];
            frame.render_widget(Paragraph::new(content), rows[2]);

            let hint = Line::from(Span::styled(
                "↑↓ выбор   Enter/Пробел переключить   ←/Esc назад",
                Style::default().fg(Color::DarkGray),
            ));
            frame.render_widget(Paragraph::new(hint), rows[3]);
        }
    }
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
