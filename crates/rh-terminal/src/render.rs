//! Ratatui rendering of the shared viewmodel.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use rh_client_core::view::{
    CaseReportView, Cell, CellColor, OverlayView, RunView, ScreenView, ViewModel,
};
use rh_core::events::EventKind;
use rh_core::geometry::{MAP_HEIGHT, MAP_WIDTH};

/// Interactive regions of the run screen, for mouse hit-testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunAreas {
    /// Interior of the tactical map (map-tile coordinates hang off its origin).
    pub map: Rect,
    /// Interior of the action list; row N of it is action index N.
    pub actions: Rect,
}

/// Draw one frame. Returns the interactive regions for mouse hit-testing.
pub fn draw(frame: &mut Frame, view: &ViewModel) -> RunAreas {
    match &view.screen {
        ScreenView::Splash {
            title,
            intro,
            bindings,
            options,
            selected,
        } => {
            draw_splash(frame, title, intro, bindings, options, *selected);
            RunAreas::default()
        }
        ScreenView::TextEntry {
            title,
            prompt,
            input,
            error,
        } => {
            draw_text_entry(frame, title, prompt, input, error.as_deref(), &view.status);
            RunAreas::default()
        }
        ScreenView::Run(run) => draw_run(frame, run, &view.status),
        ScreenView::List {
            title,
            entries,
            selected,
        } => {
            draw_list(frame, title, entries, *selected);
            RunAreas::default()
        }
        ScreenView::CaseReport(report) => {
            draw_case_report(frame, report);
            RunAreas::default()
        }
    }
}

fn cell_style(color: CellColor) -> Style {
    match color {
        CellColor::Hunter => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        CellColor::Npc => Style::default().fg(Color::Cyan),
        CellColor::Enemy => Style::default().fg(Color::Red),
        CellColor::Villain => Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
        CellColor::VillainVulnerable => Style::default()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD),
        CellColor::Terrain => Style::default().fg(Color::Gray),
        CellColor::TerrainDim => Style::default().fg(Color::DarkGray),
        CellColor::Feature => Style::default().fg(Color::LightBlue),
        CellColor::Opportunity => Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD),
        CellColor::Exit => Style::default().fg(Color::LightCyan),
        CellColor::Snare => Style::default().fg(Color::Green),
        CellColor::Unseen => Style::default().fg(Color::Black),
    }
}

fn event_style(kind: EventKind) -> Style {
    let color = match kind {
        EventKind::Combat => Color::Red,
        EventKind::Telegraph => Color::LightMagenta,
        EventKind::Clue => Color::LightGreen,
        EventKind::Clock => Color::Yellow,
        EventKind::Social => Color::Cyan,
        EventKind::Item => Color::LightBlue,
        EventKind::Travel => Color::White,
        EventKind::System => Color::Gray,
    };
    Style::default().fg(color)
}

fn draw_run(frame: &mut Frame, run: &RunView, status: &str) -> RunAreas {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(MAP_HEIGHT as u16 + 2),
            Constraint::Min(10),
        ])
        .split(frame.area());
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(MAP_WIDTH as u16 + 2),
            Constraint::Min(26),
            Constraint::Length(28),
        ])
        .split(vertical[0]);

    // The tactical map, with the look cursor highlighted.
    let map_block = Block::default()
        .borders(Borders::ALL)
        .title(run.header.clone());
    let map_inner = map_block.inner(top[0]);
    let cursor = run.cursor;
    let mut lines: Vec<Line> = Vec::with_capacity(MAP_HEIGHT as usize);
    for y in 0..MAP_HEIGHT as usize {
        let mut spans: Vec<Span> = Vec::with_capacity(MAP_WIDTH as usize);
        for x in 0..MAP_WIDTH as usize {
            let cell: Cell = run.cells[y * MAP_WIDTH as usize + x];
            let mut style = cell_style(cell.color);
            if let Some(c) = cursor {
                if c.x as usize == x && c.y as usize == y {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            }
            spans.push(Span::styled(cell.glyph.to_string(), style));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).block(map_block), top[0]);

    // The vitals + look column: who and what the cursor is on.
    let mut side: Vec<Line> = vec![
        Line::styled(
            run.clock_line.clone(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            run.health_line.clone(),
            Style::default().fg(Color::LightRed),
        ),
        Line::raw(run.pools_line.clone()),
        Line::raw(run.stamina_line.clone()),
        Line::raw(""),
        Line::styled(
            if run.looking {
                "Look (cursor)"
            } else {
                "Look (hover)"
            },
            Style::default().add_modifier(Modifier::UNDERLINED),
        ),
    ];
    match &run.inspect {
        Some(text) => side.push(Line::styled(
            format!("  {text}"),
            Style::default().fg(Color::White),
        )),
        None => side.push(Line::styled(
            "  hover a tile, or press ; to look",
            Style::default().fg(Color::DarkGray),
        )),
    }
    side.push(Line::raw(""));
    side.push(Line::styled(
        "Pack",
        Style::default().add_modifier(Modifier::UNDERLINED),
    ));
    for item in &run.inventory {
        side.push(Line::raw(format!("  {item}")));
    }
    frame.render_widget(
        Paragraph::new(Text::from(side))
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("The Hunter")),
        top[1],
    );

    // The clickable action list.
    let actions_block = Block::default().borders(Borders::ALL).title("Actions");
    let actions_inner = actions_block.inner(top[2]);
    let action_lines: Vec<Line> = run
        .actions
        .iter()
        .map(|action| {
            let key_style = if action.enabled {
                Style::default().fg(Color::LightCyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let label_style = if action.enabled {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let mut spans = vec![
                Span::styled(format!("{:>2} ", action.key), key_style),
                Span::styled(action.label.clone(), label_style),
            ];
            if let Some(note) = &action.note {
                spans.push(Span::styled(
                    format!(" ({note})"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Line::from(spans)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(Text::from(action_lines)).block(actions_block),
        top[2],
    );

    // The lower band: event log and status. Keep the newest lines that fit,
    // so the bottom of the log is never clipped off-screen.
    let mut log_lines: Vec<Line> = run
        .log_tail
        .iter()
        .map(|(kind, text)| Line::styled(text.clone(), event_style(*kind)))
        .collect();
    if !status.is_empty() {
        log_lines.push(Line::styled(
            status.to_owned(),
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    let inner_height = vertical[1].height.saturating_sub(2) as usize;
    if log_lines.len() > inner_height {
        log_lines.drain(0..log_lines.len() - inner_height);
    }
    // No wrapping here: one event per row, so the row budget is exact and the
    // newest lines always land inside the panel.
    frame.render_widget(
        Paragraph::new(Text::from(log_lines))
            .block(Block::default().borders(Borders::ALL).title("The Record")),
        vertical[1],
    );

    // Modal overlay.
    if let Some(overlay) = &run.overlay {
        draw_overlay(frame, overlay);
    }
    RunAreas {
        map: map_inner,
        actions: actions_inner,
    }
}

fn draw_overlay(frame: &mut Frame, overlay: &OverlayView) {
    let area = centered(frame.area(), 46, (overlay.items.len() as u16 + 4).max(5));
    frame.render_widget(Clear, area);
    if overlay.items.is_empty() {
        frame.render_widget(
            Paragraph::new(overlay.title.clone())
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = overlay
        .items
        .iter()
        .map(|(label, blocked)| match blocked {
            None => ListItem::new(label.clone()),
            Some(reason) => ListItem::new(format!("{label} — {reason}"))
                .style(Style::default().fg(Color::DarkGray)),
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(overlay.selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(overlay.title.clone()),
            ),
        area,
        &mut state,
    );
}

fn draw_splash(
    frame: &mut Frame,
    title: &str,
    intro: &[String],
    bindings: &[(String, String)],
    options: &[String],
    selected: usize,
) {
    let area = frame.area();
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(
        Line::styled(
            title.to_owned(),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center),
    );
    lines.push(Line::raw(""));
    for paragraph in intro {
        lines.push(Line::raw(paragraph.clone()).alignment(Alignment::Center));
        lines.push(Line::raw(""));
    }
    for (keys, action) in bindings {
        lines.push(
            Line::from(vec![
                Span::styled(format!("{keys:>22}"), Style::default().fg(Color::LightCyan)),
                Span::raw("  "),
                Span::styled(action.clone(), Style::default().fg(Color::Gray)),
            ])
            .alignment(Alignment::Center),
        );
    }
    lines.push(Line::raw(""));
    for (index, option) in options.iter().enumerate() {
        let style = if index == selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::styled(format!("  {option}  "), style).alignment(Alignment::Center));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_text_entry(
    frame: &mut Frame,
    title: &str,
    prompt: &str,
    input: &str,
    error: Option<&str>,
    status: &str,
) {
    let area = centered(frame.area(), 60, 9);
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::raw(prompt.to_owned()),
        Line::raw(""),
        Line::styled(
            format!("> {input}_"),
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(error) = error {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            error.to_owned(),
            Style::default().fg(Color::LightRed),
        ));
    }
    if !status.is_empty() {
        lines.push(Line::styled(
            status.to_owned(),
            Style::default().fg(Color::LightYellow),
        ));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.to_owned()),
            ),
        area,
    );
}

fn draw_list(
    frame: &mut Frame,
    title: &str,
    entries: &[(String, String)],
    selected: Option<usize>,
) {
    let area = frame.area();
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(34), Constraint::Min(30)])
        .split(area);
    let items: Vec<ListItem> = entries
        .iter()
        .map(|(heading, _)| ListItem::new(heading.clone()))
        .collect();
    let mut state = ListState::default();
    state.select(selected.or(Some(0)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.to_owned()),
            ),
        columns[0],
        &mut state,
    );
    let body = selected
        .or(Some(0))
        .and_then(|index| entries.get(index))
        .map(|(_, body)| body.clone())
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(body)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Detail")),
        columns[1],
    );
}

fn draw_case_report(frame: &mut Frame, report: &CaseReportView) {
    let area = frame.area();
    let mut lines: Vec<Line> = vec![
        Line::styled(
            report.outcome.clone(),
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            format!("The villain: {}", report.villain),
            Style::default().fg(Color::LightRed),
        ),
        Line::raw(format!("Origin — {}", report.origin)),
        Line::raw(format!("Scheme — {}", report.scheme)),
        Line::raw(""),
    ];
    if !report.hidden_clues.is_empty() {
        lines.push(Line::styled(
            "What you never found:",
            Style::default().add_modifier(Modifier::UNDERLINED),
        ));
        for clue in &report.hidden_clues {
            lines.push(Line::styled(
                format!("  {clue}"),
                Style::default().fg(Color::Gray),
            ));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "The certified routes:",
        Style::default().add_modifier(Modifier::UNDERLINED),
    ));
    for route in &report.routes {
        for line in route.lines() {
            lines.push(Line::styled(
                format!("  {line}"),
                Style::default().fg(Color::LightGreen),
            ));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "Replay share code (copy to relive or share this run):",
        Style::default().add_modifier(Modifier::UNDERLINED),
    ));
    lines.push(Line::styled(
        report.share_code.clone(),
        Style::default().fg(Color::LightCyan),
    ));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Enter to return to the fireside.",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Case Report")),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
