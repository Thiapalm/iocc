use super::manifest::cell_text;
use super::{App, TABS, ViewContent, view_tag};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Sparkline, Table, Tabs,
    Wrap,
};

const OK: Color = Color::Green;
const ERR: Color = Color::Red;
const CHANGED: Color = Color::Yellow;
const DIM: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, app, header);
    let body = if app.pinned.is_empty() {
        body
    } else {
        let [main, watches] =
            Layout::horizontal([Constraint::Min(50), Constraint::Length(38)]).areas(body);
        draw_pinned(frame, app, watches);
        main
    };
    match app.tab {
        0 => draw_scan(frame, app, body),
        1 => draw_regs(frame, app, body),
        2 => draw_plugins(frame, app, body),
        _ => draw_log(frame, app, body),
    }
    draw_footer(frame, app, footer);
    if let Some(prompt) = &app.prompt {
        draw_prompt(frame, app, prompt);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let (dot, color, text) = match app.connected {
        Some(true) => ("●", OK, "connected"),
        Some(false) => ("○", ERR, "disconnected"),
        None => ("○", DIM, "connecting"),
    };
    let title = Line::from(vec![
        Span::raw(" iocc "),
        Span::styled(dot, Style::new().fg(color)),
        Span::raw(format!(
            " {text} · {} kHz · timeout {} ms ",
            app.params.speed / 1000,
            app.params.timeout
        )),
    ]);
    let tabs = Tabs::new(
        TABS.iter()
            .enumerate()
            .map(|(i, t)| format!("{} {t}", i + 1)),
    )
    .select(app.tab)
    .highlight_style(Style::new().bold().fg(Color::Cyan))
    .block(Block::bordered().title(title));
    frame.render_widget(tabs, area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let keys = match app.tab {
        0 => "←↑↓→ move · Enter open · s scan · i info",
        1 => "↑↓ · Enter write · r read · p poll · a/g/n set · m read-only",
        2 if app.action_focus => "↑↓ select · Enter run · Esc back",
        2 => "↑↓ plugin · Enter/→ actions · p pin · P unpin · R reload",
        _ => "↑↓ scroll · c clear",
    };
    let line = Line::from(vec![
        Span::styled(format!(" {} ", app.status), Style::new().fg(Color::Cyan)),
        Span::styled(format!("│ {keys} │ Tab · q quit"), Style::new().fg(DIM)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/**
 * Panels pinned with `p`, kept visible on every tab while other plugins are used
 */
fn draw_pinned(frame: &mut Frame, app: &App, area: Rect) {
    let outer = Block::bordered().title(" Watches (p/P) ");
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let areas = Layout::vertical(vec![Constraint::Fill(1); app.pinned.len()]).split(inner);
    for (pinned, area) in app.pinned.iter().zip(areas.iter()) {
        draw_view(frame, app, &pinned.plugin, &pinned.view, *area);
    }
}

fn draw_scan(frame: &mut Frame, app: &App, area: Rect) {
    let [grid_area, info_area] =
        Layout::horizontal([Constraint::Length(72), Constraint::Min(20)]).areas(area);

    let mut lines = vec![Line::from(
        std::iter::once(Span::styled("     ", Style::new()))
            .chain((0..16).map(|c| Span::styled(format!(" {c:X}  "), Style::new().fg(DIM))))
            .collect::<Vec<_>>(),
    )];
    for row in 0..8u8 {
        let mut spans = vec![Span::styled(format!("0x{:X}0 ", row), Style::new().fg(DIM))];
        for col in 0..16u8 {
            let addr = row * 16 + col;
            let reserved = !(0x03..=0x77).contains(&addr);
            let (text, mut style) = if reserved {
                (" ·  ", Style::new().fg(DIM))
            } else if app.found.contains(&addr) {
                (" ■  ", Style::new().fg(OK).bold())
            } else {
                (" -  ", Style::new().fg(DIM))
            };
            if addr == app.cursor {
                style = style.add_modifier(Modifier::REVERSED);
            }
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::default());
    lines.push(Line::from("■ answered   - no answer   · reserved".fg(DIM)));
    let title = if app.scanning {
        " I2C bus (scanning...) "
    } else {
        " I2C bus "
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title)),
        grid_area,
    );

    let found = if app.found.contains(&app.cursor) {
        "answers"
    } else {
        "no answer"
    };
    let mut info = vec![
        Line::from(format!("Address  {:#04X}  ({found})", app.cursor)),
        Line::from(format!("Devices  {}", app.found.len())),
        Line::default(),
    ];
    info.extend(app.found.iter().map(|a| Line::from(format!("  {a:#04X}"))));
    if let Some(device) = &app.device_info {
        info.push(Line::default());
        info.extend(device.lines().map(|l| Line::from(l.to_string().fg(DIM))));
    }
    frame.render_widget(
        Paragraph::new(info).block(Block::bordered().title(" Details ")),
        info_area,
    );
}

fn draw_regs(frame: &mut Frame, app: &App, area: Rect) {
    let ro = if app.read_only { " [read-only]" } else { "" };
    let poll = if app.reg_poll { " [polling]" } else { "" };
    let title = format!(
        " Device {:#04X} · from {:#04X} · {} byte(s){ro}{poll} ",
        app.reg_addr,
        app.reg_start,
        app.reg_count_clamped()
    );
    if app.reg_data.is_empty() {
        let msg = "No data yet. Press r to read (a/g/n change the address, start and count).";
        frame.render_widget(
            Paragraph::new(msg).block(Block::bordered().title(title)),
            area,
        );
        return;
    }
    let rows = app.reg_data.iter().enumerate().map(|(i, &value)| {
        let changed = app.reg_prev.get(i).is_some_and(|&p| p != value);
        let style = if changed {
            Style::new().fg(CHANGED).bold()
        } else {
            Style::new()
        };
        let row = Row::new(vec![
            Cell::from(format!("{:#04X}", app.reg_start as usize + i)),
            Cell::from(format!("{value:02X}")),
            Cell::from(format!("{value:08b}")),
            Cell::from(format!("{value:>3}")),
            Cell::from(if value.is_ascii_graphic() {
                (value as char).to_string()
            } else {
                ".".into()
            }),
        ])
        .style(style);
        if i == app.reg_sel {
            row.style(style.add_modifier(Modifier::REVERSED))
        } else {
            row
        }
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(12),
            Constraint::Length(6),
            Constraint::Length(5),
        ],
    )
    .header(Row::new(["Reg", "Hex", "Bin", "Dec", "Asc"]).style(Style::new().fg(DIM).bold()))
    .block(Block::bordered().title(title));
    let mut state = ratatui::widgets::TableState::default().with_selected(app.reg_sel);
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_plugins(frame: &mut Frame, app: &App, area: Rect) {
    if !app.plugins_loaded {
        frame.render_widget(
            Paragraph::new("Loading plugins...").block(Block::bordered().title(" Plugins ")),
            area,
        );
        return;
    }
    let [list_area, detail] =
        Layout::horizontal([Constraint::Length(24), Constraint::Min(30)]).areas(area);

    let items: Vec<ListItem> = app
        .plugins
        .iter()
        .map(|p| {
            let mark = if p.described { "●" } else { "○" };
            ListItem::new(format!("{mark} {}", p.name))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.plugin_sel));
    let list = List::new(items)
        .block(Block::bordered().title(" Plugins "))
        .highlight_style(if app.action_focus {
            Style::new().bold()
        } else {
            Style::new().reversed()
        });
    if app.plugins.is_empty() {
        frame.render_widget(
            Paragraph::new("No plugin detected.\nInstall iocc-<name> next to iocc or in PATH.")
                .wrap(Wrap { trim: true })
                .block(Block::bordered().title(" Plugins ")),
            area,
        );
        return;
    }
    frame.render_stateful_widget(list, list_area, &mut state);

    let Some(plugin) = app.selected_plugin() else {
        return;
    };
    let m = &plugin.manifest;
    let title = format!(
        " {}{} ",
        m.title.as_deref().unwrap_or(&plugin.name),
        m.version
            .as_ref()
            .map(|v| format!(" v{v}"))
            .unwrap_or_default()
    );

    let actions_h = (m.actions.len() as u16 + 2).min(8);
    let out_h = if app.action_out.is_some() { 7 } else { 0 };
    let [views_area, actions_area, out_area] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(actions_h),
        Constraint::Length(out_h),
    ])
    .areas(detail);

    let outer = Block::bordered().title(title);
    let inner = outer.inner(views_area);
    frame.render_widget(outer, views_area);

    let mut header = vec![Line::from(plugin.path.display().to_string().fg(DIM))];
    if let Some(d) = &m.description {
        header.push(Line::from(d.clone().fg(DIM)));
    }
    if !plugin.described {
        header.push(Line::from(
            "No TUI manifest (answer --iocc-manifest to get custom panels)".fg(DIM),
        ));
    }
    let header_h = header.len() as u16;
    let [head_area, views_inner] =
        Layout::vertical([Constraint::Length(header_h), Constraint::Min(0)]).areas(inner);
    frame.render_widget(Paragraph::new(header), head_area);

    if m.views.is_empty() {
        frame.render_widget(Paragraph::new("No panels.".fg(DIM)), views_inner);
    } else {
        let areas = Layout::vertical(vec![Constraint::Fill(1); m.views.len()]).split(views_inner);
        for (view, area) in m.views.iter().zip(areas.iter()) {
            draw_view(frame, app, &plugin.name, view, *area);
        }
    }

    let items: Vec<ListItem> = m
        .actions
        .iter()
        .map(|a| {
            let mut spans = vec![Span::raw(a.title().to_string())];
            if let Some(d) = &a.description {
                spans.push(Span::styled(format!("  {d}"), Style::new().fg(DIM)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let mut state = ListState::default();
    if app.action_focus {
        state.select(Some(app.action_sel));
    }
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::bordered().title(" Actions "))
            .highlight_style(Style::new().reversed()),
        actions_area,
        &mut state,
    );

    if let Some((ok, text)) = &app.action_out {
        let color = if *ok { OK } else { ERR };
        frame.render_widget(
            Paragraph::new(text.trim_end().to_string())
                .wrap(Wrap { trim: false })
                .block(
                    Block::bordered()
                        .title(" Output ")
                        .border_style(Style::new().fg(color)),
                ),
            out_area,
        );
    }
}

fn draw_view(frame: &mut Frame, app: &App, plugin: &str, view: &super::manifest::View, area: Rect) {
    let tag = view_tag(plugin, &view.id);
    let data = app.views.get(&tag);
    let age = data
        .map(|d| format!(" · {}s ago", d.updated.elapsed().as_secs()))
        .unwrap_or_default();
    let state = if view.stream {
        if app.exclusive_running {
            " · paused".to_string()
        } else if app.streams.contains(&tag) {
            format!(" · live{age}")
        } else {
            " · stopped".to_string()
        }
    } else {
        age
    };
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .title(format!(" {}{state} ", view.title()));
    let Some(data) = data else {
        frame.render_widget(Paragraph::new("loading...".fg(DIM)).block(block), area);
        return;
    };
    match &data.content {
        ViewContent::Text(text) => frame.render_widget(
            Paragraph::new(text.trim_end().to_string()).block(block),
            area,
        ),
        ViewContent::Error(e) => frame.render_widget(
            Paragraph::new(e.trim_end().to_string().fg(ERR))
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        ),
        ViewContent::Table(t) => {
            let widths = vec![Constraint::Fill(1); t.columns.len().max(1)];
            let table = Table::new(
                t.rows
                    .iter()
                    .map(|r| Row::new(r.iter().map(|c| Cell::from(cell_text(c))))),
                widths,
            )
            .header(Row::new(t.columns.clone()).style(Style::new().fg(DIM).bold()))
            .block(block);
            frame.render_widget(table, area);
        }
        ViewContent::Metrics(metrics) => {
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let rows = Layout::vertical(vec![Constraint::Length(1); metrics.len()]).split(inner);
            for (metric, row) in metrics.iter().zip(rows.iter()) {
                let [label, spark] =
                    Layout::horizontal([Constraint::Length(30), Constraint::Min(5)]).areas(*row);
                let unit = metric.unit.as_deref().unwrap_or("");
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::raw(format!("{:<12}", metric.name)),
                        Span::styled(
                            format!("{:>10.3} {unit}", metric.value),
                            Style::new().fg(Color::Cyan).bold(),
                        ),
                    ])),
                    label,
                );
                if let Some(h) = app.history.get(&format!("{tag}:{}", metric.name)) {
                    let min = h.iter().cloned().fold(f64::INFINITY, f64::min);
                    let max = h.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    let span = (max - min).max(f64::EPSILON);
                    let points: Vec<u64> = h
                        .iter()
                        .map(|v| (((v - min) / span) * 100.0) as u64)
                        .collect();
                    frame.render_widget(
                        Sparkline::default()
                            .data(&points)
                            .max(100)
                            .style(Style::new().fg(OK)),
                        spark,
                    );
                }
            }
        }
    }
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    let end = app.log.len().saturating_sub(app.log_scroll);
    let start = end.saturating_sub(height);
    let lines: Vec<Line> = app.log[start..end]
        .iter()
        .map(|e| {
            let secs = e.at.as_secs_f32();
            Line::from(vec![
                Span::styled(format!("+{secs:>8.1}s "), Style::new().fg(DIM)),
                Span::styled(
                    if e.ok { "ok  " } else { "FAIL" },
                    Style::new().fg(if e.ok { OK } else { ERR }),
                ),
                Span::raw(format!(" {}", e.text)),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(format!(" Log ({}) ", app.log.len()))),
        area,
    );
}

fn draw_prompt(frame: &mut Frame, _app: &App, prompt: &super::Prompt) {
    let area = frame.area();
    let width = area.width.min(60);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + area.height.saturating_sub(5) / 2,
        width,
        height: 5.min(area.height),
    };
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from(format!("> {}█", prompt.input)),
        Line::from(prompt.hint.clone().fg(DIM)),
        Line::from("Enter confirm · Esc cancel".fg(DIM)),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .block(Block::bordered().title(format!(" {} ", prompt.title))),
        popup,
    );
}
