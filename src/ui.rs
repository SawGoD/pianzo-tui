//! Рендер интерфейса.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, EditFocus, Mode};
use crate::parser::TokenSpan;

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
        Mode::HotkeyMenu => draw_hotkey_menu(frame, app),
        Mode::CaptureStart => draw_capture(frame, "СТАРТА"),
        Mode::CaptureStop => draw_capture(frame, "СТОПА"),
        Mode::Normal => {}
    }
}

fn draw_title(frame: &mut Frame, app: &App, area: Rect) {
    let name = app.current_name.as_deref().unwrap_or("—");
    let mut spans = vec![
        Span::styled("piano-tui", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  ·  "),
        Span::styled(name.to_string(), Style::default().fg(Color::Green)),
    ];
    if let Some(n) = app.countdown {
        spans.push(Span::styled(
            format!("   ⏳ {n}"),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    } else if app.playing {
        let (done, total) = app.progress;
        spans.push(Span::styled(
            format!("   ▶ {done}/{total}"),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ));
    }
    let para = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
    frame.render_widget(para, area);
}

fn draw_body(frame: &mut Frame, app: &mut App, area: Rect) {
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
        .block(Block::default().borders(Borders::ALL).title(" Закладки "))
        .highlight_style(Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD))
        .highlight_symbol("➤ ");
    frame.render_stateful_widget(list, cols[0], &mut app.list_state);

    // Правая колонка — параметры + ноты.
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3)])
        .split(cols[1]);

    let params = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Между клавишами: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{:.3} c", app.between_keys), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Между строками:  ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{:.3} c", app.between_lines), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Громкость:       ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}%", app.volume_pct()), Style::default().fg(Color::Magenta)),
            Span::styled("   [+/-]", Style::default().fg(Color::DarkGray)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Параметры — [E] задержки "));
    frame.render_widget(params, right[0]);

    draw_notes_panel(frame, app, right[1]);
}

/// Панель нот: во время игры — караоке-подсветка с прокруткой к текущей строке.
fn draw_notes_panel(frame: &mut Frame, app: &App, area: Rect) {
    let note_count = app.notes.split_whitespace().count();
    let title = format!(" Ноты ({note_count}) — [n/E] правка ");
    let block = Block::default().borders(Borders::ALL).title(title);

    if app.playing && !app.spans.is_empty() {
        let visible = area.height.saturating_sub(2) as usize;
        let total_lines = app.notes.split('\n').count();
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
        let para = Paragraph::new(app.notes.as_str())
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(para, area);
    }
}

/// Строит подсвеченный текст: сыгранные токены тусклые, текущий — выделен.
fn build_karaoke(app: &App) -> Text<'_> {
    let cur = app.play_event.min(app.spans.len() - 1);
    let cur_line = app.spans.get(cur).map(|s| s.line);

    let src_lines: Vec<&str> = app.notes.split('\n').collect();
    let mut offsets = Vec::with_capacity(src_lines.len());
    let mut off = 0usize;
    for l in &src_lines {
        offsets.push(off);
        off += l.len() + 1;
    }

    let played = Style::default().fg(Color::DarkGray);
    let current = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let future = Style::default().fg(Color::White);

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
        (format!("⏳ Старт через {n}…  (переключитесь в нужное окно)"), Color::Yellow)
    } else if app.playing {
        let (done, total) = app.progress;
        (format!("▶ ВОСПРОИЗВЕДЕНИЕ  {done}/{total}"), Color::Green)
    } else if app.audio_playing {
        (format!("♪ ЗВУЧИТ  ({}%)", app.volume_pct()), Color::Magenta)
    } else {
        (format!("● {}", app.status), Color::Cyan)
    };
    let para = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )))
    .block(Block::default().borders(Borders::ALL).title(" Статус "));
    frame.render_widget(para, area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let cfg = *app.hotkeys.lock().unwrap();

    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(36),
            Constraint::Percentage(34),
            Constraint::Percentage(30),
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
        Span::raw(" звук  "),
        Span::styled("a", Style::default().fg(Color::Green)),
        Span::raw(" доб.  "),
        Span::styled("E", Style::default().fg(Color::Cyan)),
        Span::raw(" правка  "),
        Span::styled("d", Style::default().fg(Color::Cyan)),
        Span::raw(" удал."),
    ]);

    // Справа: громкость, настройки, выход.
    let right = Line::from(vec![
        Span::styled("+/-", Style::default().fg(Color::Magenta)),
        Span::raw(" громк.  "),
        Span::styled("h", Style::default().fg(Color::Yellow)),
        Span::raw(" хоткеи  "),
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
        Span::styled("▏", Style::default().fg(Color::Yellow)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(format!(" {title} ")),
    );
    frame.render_widget(para, area);
}

/// Единое окно правки: ноты сверху, задержки снизу (req 2).
fn draw_edit(frame: &mut Frame, app: &mut App) {
    let area = modal_rect((frame.area().width * 88) / 100, (frame.area().height * 82) / 100, frame.area());
    frame.render_widget(Clear, area);

    let name = app
        .creating
        .clone()
        .or_else(|| app.current_name.clone())
        .unwrap_or_else(|| "новая".to_string());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(format!(
            " Правка: {name} — Tab: ноты/задержки · Esc/Ctrl+S — сохранить · Ctrl+Q — отмена "
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(4)])
        .split(inner);

    // Ноты.
    let notes_focused = app.edit_focus == EditFocus::Notes;
    let notes_border = if notes_focused { Color::Yellow } else { Color::DarkGray };
    app.textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(notes_border))
            .title(" Ноты "),
    );
    frame.render_widget(&app.textarea, rows[0]);

    // Задержки (два поля рядом, внизу).
    draw_edit_delays(frame, app, rows[1]);
}

fn draw_edit_delays(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.edit_focus == EditFocus::Delays;
    let border = if focused { Color::Yellow } else { Color::DarkGray };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(" Задержки — ←→ поле · ↑↓ ±0.001 · цифры/точка/Backspace ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let field = |label: &str, value: &str, active: bool| -> Paragraph {
        let value_style = if active {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Yellow)
        };
        let marker = if active { "▶ " } else { "  " };
        Paragraph::new(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(Color::Yellow)),
            Span::styled(label.to_string(), Style::default().fg(Color::Gray)),
            Span::styled(format!("{value} "), value_style),
        ]))
    };

    frame.render_widget(
        field("Клавиши: ", &app.input_keys, focused && app.delay_field == 0),
        cols[0],
    );
    frame.render_widget(
        field("Строки: ", &app.input_lines, focused && app.delay_field == 1),
        cols[1],
    );
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

fn draw_hotkey_menu(frame: &mut Frame, app: &App) {
    let cfg = *app.hotkeys.lock().unwrap();
    let area = modal_rect(56, 7, frame.area());
    frame.render_widget(Clear, area);
    let para = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("[1] Старт: ", Style::default().fg(Color::Gray)),
            Span::styled(cfg.start.label(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("[2] Стоп:  ", Style::default().fg(Color::Gray)),
            Span::styled(cfg.stop.label(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(Span::styled(
            "1/2 — переназначить · Ctrl+Backspace — сброс · Esc — закрыть",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Хоткеи "),
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
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Перепривязка "),
    );
    frame.render_widget(para, area);
}
