use super::manifest::{Metric, ViewKind, cell_text};
use super::{App, TABS, TERMINAL_TAB, ViewContent, WATCH_PANEL_H, view_tag};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Cell, Chart, Clear, Dataset, GraphType, List, ListItem, ListState,
    Paragraph, Row, Sparkline, Table, Tabs, Wrap,
};

const OK: Color = Color::Green;
const ERR: Color = Color::Red;
const CHANGED: Color = Color::Yellow;
const DIM: Color = Color::DarkGray;

/// Width reserved for the y-axis labels. Keeping it constant across charts
/// makes the graph area start at the same column when panels are stacked in the
/// *Watches* column, even when one axis has decimals and another does not.
const CHART_LABEL_WIDTH: usize = 8;

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
        3 => draw_log(frame, app, body),
        _ => draw_terminal(frame, app, body),
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
    let mut spans = vec![
        Span::raw(" iocc "),
        Span::styled(dot, Style::new().fg(color)),
        Span::raw(format!(
            " {text} · {} kHz · timeout {} ms ",
            app.params.speed / 1000,
            app.params.timeout
        )),
    ];
    if app.is_recording() {
        spans.push(Span::styled("● REC ", Style::new().fg(ERR).bold()));
    }
    let title = Line::from(spans);
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
    if app.terminal_focus {
        let line = Line::from(vec![
            Span::styled(format!(" {} ", app.status), Style::new().fg(Color::Cyan)),
            Span::styled(
                "│ terminal focused · keys go to the shell · F12 releases",
                Style::new().fg(DIM),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    let mut keys = match app.tab {
        0 => "←↑↓→ move · Enter open · s scan · i info",
        1 => "↑↓ · Enter write · r read · p poll · a/g/n set · m read-only",
        2 if app.action_focus => "↑↓ select · Enter run · Esc back",
        2 if app.zoom => "z close · Ctrl+↑↓ scroll · g CSV · P unpin · R reload",
        2 => "↑↓ plugin · [ ] panel · p pin · x unpin · ,/. order · g CSV · z zoom · R reload",
        3 => "↑↓ scroll · c clear",
        _ => "Enter/F12 type · PgUp/PgDn history · Enter restarts after exit",
    }
    .to_string();
    if !app.pinned.is_empty() && app.tab != TERMINAL_TAB {
        keys.push_str(" · PgUp/PgDn watches");
    }
    let line = Line::from(vec![
        Span::styled(format!(" {} ", app.status), Style::new().fg(Color::Cyan)),
        Span::styled(format!("│ {keys} │ Tab · q quit"), Style::new().fg(DIM)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/**
 * Panels pinned with `p`, kept visible on every tab while other plugins are used.
 * When they do not fit, the column shows a window scrolled with PageUp/PageDown.
 */
fn draw_pinned(frame: &mut Frame, app: &App, area: Rect) {
    let total = app.pinned.len();
    if total == 0 {
        return;
    }
    let inner = Block::bordered().inner(area);
    let visible = ((inner.height / WATCH_PANEL_H) as usize).clamp(1, total);
    let start = app.watch_scroll.min(total - visible);
    let shown = &app.pinned[start..start + visible];
    let title = if visible < total {
        format!(
            " Watches (p/P) {}-{}/{} ",
            start + 1,
            start + visible,
            total
        )
    } else {
        " Watches (p/P) ".to_string()
    };
    let outer = Block::bordered().title(title);
    frame.render_widget(outer, area);
    let areas = Layout::vertical(vec![Constraint::Fill(1); visible]).split(inner);
    for (pinned, area) in shown.iter().zip(areas.iter()) {
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
    if app.zoom
        && let Some(plugin) = app.selected_plugin()
        && let Some(view) = plugin.manifest.views.get(app.view_sel)
    {
        draw_view(frame, app, &plugin.name, view, area);
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
    let scroll = app.scroll.get(&tag).copied().unwrap_or(0);
    let selected = app.tab == 2
        && app.selected_plugin().is_some_and(|p| {
            p.name == plugin
                && p.manifest
                    .views
                    .get(app.view_sel)
                    .is_some_and(|v| v.id == view.id)
        });
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
        .border_style(if selected {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new().fg(DIM)
        })
        .title(format!(
            " {}{}{state} ",
            if selected { "▸ " } else { "" },
            view.title()
        ));
    let Some(data) = data else {
        frame.render_widget(Paragraph::new("loading...".fg(DIM)).block(block), area);
        return;
    };
    match &data.content {
        ViewContent::Text(text) => frame.render_widget(
            Paragraph::new(text.trim_end().to_string())
                .scroll((scroll, 0))
                .block(block),
            area,
        ),
        ViewContent::Error(e) => frame.render_widget(
            Paragraph::new(e.trim_end().to_string().fg(ERR))
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0))
                .block(block),
            area,
        ),
        ViewContent::Table(t) => {
            let widths = vec![Constraint::Fill(1); t.columns.len().max(1)];
            let table = Table::new(
                t.rows
                    .iter()
                    .skip(scroll as usize)
                    .map(|r| Row::new(r.iter().map(|c| Cell::from(cell_text(c))))),
                widths,
            )
            .header(Row::new(t.columns.clone()).style(Style::new().fg(DIM).bold()))
            .block(block);
            frame.render_widget(table, area);
        }
        ViewContent::Metrics(metrics) => {
            if view.kind == ViewKind::Chart {
                draw_chart(frame, app, &tag, view, metrics, block, area);
                return;
            }
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
                    let points = spark_points(h);
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

/**
 * Line chart of one metric, using the history the TUI already keeps for a
 * streaming or periodically refreshed metrics view. On a short panel (a watch,
 * for example) it falls back to the compact sparkline.
 */
fn draw_chart(
    frame: &mut Frame,
    app: &App,
    tag: &str,
    view: &super::manifest::View,
    metrics: &[Metric],
    block: Block<'_>,
    area: Rect,
) {
    let chosen = app
        .chart_metric
        .get(tag)
        .map(String::as_str)
        .or(view.metric.as_deref());
    let metric = match chosen {
        Some(name) => metrics.iter().find(|m| m.name == name),
        None => metrics.first(),
    }
    .or_else(|| metrics.first());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(metric) = metric else {
        frame.render_widget(
            Paragraph::new("chart: the output has no metrics")
                .wrap(Wrap { trim: true })
                .style(Style::new().fg(ERR)),
            inner,
        );
        return;
    };
    let unit = metric.unit.as_deref().unwrap_or("");
    let Some(history) = app
        .history
        .get(&format!("{tag}:{}", metric.name))
        .filter(|h| !h.is_empty())
    else {
        frame.render_widget(Paragraph::new("waiting for samples...").fg(DIM), inner);
        return;
    };
    let (min, max) = history
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(*v), hi.max(*v))
        });
    // A plugin can pin the axis for discrete signals (0/1, states, limits).
    // Without both bounds the axis keeps a 10% margin around the samples.
    let fixed = match (view.min, view.max) {
        (Some(lo), Some(hi)) if hi > lo => Some((lo, hi)),
        _ => None,
    };
    let format_value = |value: f64| chart_value(value, fixed.is_some());
    let header = Line::from(vec![
        Span::raw(format!("{}: ", metric.name)),
        Span::styled(
            format!("{} {unit}", format_value(metric.value)),
            Style::new().fg(Color::Cyan).bold(),
        ),
        Span::styled(
            format!(
                "   min {}  max {}  ({} samples)",
                format_value(min),
                format_value(max),
                history.len()
            ),
            Style::new().fg(DIM),
        ),
        Span::styled(metric_hint(metrics, metric), Style::new().fg(DIM)),
    ]);
    if inner.height < 5 || inner.width < 24 {
        let [head, spark] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
        frame.render_widget(Paragraph::new(header), head);
        frame.render_widget(
            Sparkline::default()
                .data(spark_points(history))
                .max(100)
                .style(Style::new().fg(OK)),
            spark,
        );
        return;
    }
    let [head, chart_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    frame.render_widget(Paragraph::new(header), head);

    // Without fixed bounds, leave a small margin so the line does not touch the axes
    let (lo, hi) = fixed.unwrap_or_else(|| {
        let span = max - min;
        let pad = if span > f64::EPSILON {
            span * 0.1
        } else {
            max.abs().max(1.0) * 0.1
        };
        (min - pad, max + pad)
    });
    let points: Vec<(f64, f64)> = history
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v))
        .collect();
    let last = history.len().saturating_sub(1).max(1) as f64;
    let data = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::new().fg(OK))
        .data(&points);
    let chart = Chart::new(vec![data])
        .x_axis(
            Axis::default()
                .bounds([0.0, last])
                .style(Style::new().fg(DIM))
                .title(Span::styled("time ->", Style::new().fg(DIM))),
        )
        .y_axis(
            Axis::default()
                .bounds([lo, hi])
                .style(Style::new().fg(DIM))
                .labels(chart_axis_labels(lo, hi, fixed.is_some())),
        );
    frame.render_widget(chart, chart_area);
}

/**
 * Position of the charted metric and the key that switches it, for panels
 * whose plugin publishes more than one metric
 */
fn metric_hint(metrics: &[Metric], current: &Metric) -> String {
    if metrics.len() < 2 {
        return String::new();
    }
    let pos = metrics
        .iter()
        .position(|m| m.name == current.name)
        .unwrap_or(0);
    format!("  m: next ({}/{})", pos + 1, metrics.len())
}

fn axis_value(value: f64) -> String {
    let abs = value.abs();
    if abs >= 100.0 {
        format!("{value:.0}")
    } else if abs >= 10.0 {
        format!("{value:.1}")
    } else if abs >= 1.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

/**
 * Y-axis labels, left padded to the same width for every chart so the graph
 * area starts at the same column when panels are stacked in *Watches*.
 */
fn chart_axis_labels(lo: f64, hi: f64, fixed: bool) -> [String; 3] {
    let format = |value: f64| {
        if fixed {
            compact_value(value)
        } else {
            axis_value(value)
        }
    };
    let labels = [format(lo), format((lo + hi) / 2.0), format(hi)];
    let width = labels
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or(0)
        .max(CHART_LABEL_WIDTH);
    labels.map(|label| format!("{label:>width$}"))
}

/**
 * Value shown in a chart header. Fixed axes (0/1 states, for example) use the
 * shortest form instead of three decimals, so the panel reads "1" and not
 * "1.000".
 */
fn chart_value(value: f64, fixed: bool) -> String {
    if fixed {
        compact_value(value)
    } else {
        format!("{value:.3}")
    }
}

/**
 * Shortest representation with at most three decimals: 0, 1, 0.5, 12.25
 */
fn compact_value(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    if value.fract() == 0.0 {
        return format!("{value:.0}");
    }
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/**
 * Normalizes a history window to 0..100 for a Sparkline
 */
fn spark_points(history: &std::collections::VecDeque<f64>) -> Vec<u64> {
    let min = history.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = history.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(f64::EPSILON);
    history
        .iter()
        .map(|v| (((v - min) / span) * 100.0) as u64)
        .collect()
}

/**
 * Interactive shell. The vt100 screen is painted cell by cell, grouping cells
 * with the same style into spans, so colours and attributes from programs like
 * `ls --color` or `vim` survive. When the tab has the focus the real cursor is
 * placed at the shell position and F12 releases the keyboard back to the TUI.
 */
fn draw_terminal(frame: &mut Frame, app: &App, area: Rect) {
    let (state, color) = match (&app.terminal, app.terminal_focus) {
        (Some(_), true) => ("FOCUSED · F12 releases", Color::Cyan),
        (Some(session), false) if session.exited().is_some() => ("exited", ERR),
        (Some(_), false) => ("F12 or Enter to type", DIM),
        (None, _) if app.terminal_error.is_some() => ("failed", ERR),
        (None, _) => ("starting", DIM),
    };
    let shell = app
        .terminal
        .as_ref()
        .map(|session| session.shell().to_string())
        .unwrap_or_else(|| "shell".to_string());
    let title = format!(" Terminal · {shell} · {state} ");
    let block = Block::bordered()
        .title(title)
        .border_style(Style::new().fg(color));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    // `tick` applies this to the pty, resizing the shell with the tab
    app.terminal_size.set((inner.height, inner.width));

    let Some(session) = &app.terminal else {
        let message = match &app.terminal_error {
            Some(err) => vec![
                Line::from("The shell could not be started:".fg(ERR)),
                Line::from(err.clone().fg(ERR)),
                Line::from("Enter tries again · F12 returns to the tabs".fg(DIM)),
            ],
            None => vec![
                Line::from("Starting the shell...".fg(DIM)),
                Line::from("F12 or Enter gives it the keyboard".fg(DIM)),
            ],
        };
        frame.render_widget(Paragraph::new(message), inner);
        return;
    };

    frame.render_widget(Clear, inner);
    draw_terminal_screen(frame, session.screen(), inner);

    if let Some(message) = session.exited() {
        let line = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        frame.render_widget(Clear, line);
        frame.render_widget(
            Paragraph::new(Line::from(format!(
                " {message} · Enter restarts · F12 releases "
            )))
            .style(Style::new().fg(Color::Yellow)),
            line,
        );
        return;
    }

    let (row, column) = session.screen().cursor_position();
    if app.terminal_focus
        && !session.screen().hide_cursor()
        && session.screen().scrollback() == 0
        && row < inner.height
        && column < inner.width
    {
        frame.set_cursor_position((inner.x + column, inner.y + row));
    }
}

fn draw_terminal_screen(frame: &mut Frame, screen: &vt100::Screen, area: Rect) {
    let (rows, cols) = screen.size();
    let height = area.height.min(rows.get());
    let width = area.width.min(cols.get());
    let mut lines = Vec::with_capacity(height as usize);
    for row in 0..height {
        let mut spans = Vec::new();
        let mut text = String::new();
        let mut style = Style::new();
        let mut started = false;
        for column in 0..width {
            let Some(cell) = screen.cell(row, column) else {
                break;
            };
            // The wide character itself already covers two columns
            if cell.is_wide_continuation() {
                continue;
            }
            let cell_style = terminal_style(cell);
            if !started {
                style = cell_style;
                started = true;
            } else if cell_style != style {
                spans.push(Span::styled(std::mem::take(&mut text), style));
                style = cell_style;
            }
            if cell.has_contents() {
                text.push_str(cell.contents());
            } else {
                text.push(' ');
            }
        }
        if !text.is_empty() {
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn terminal_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::new()
        .fg(terminal_color(cell.fgcolor()))
        .bg(terminal_color(cell.bgcolor()));
    if cell.bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.dim() {
        style = style.add_modifier(Modifier::DIM);
    }
    if cell.italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.inverse() {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

fn terminal_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
        vt100::Color::Idx(index) => match index {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::Gray,
            8 => Color::DarkGray,
            9 => Color::LightRed,
            10 => Color::LightGreen,
            11 => Color::LightYellow,
            12 => Color::LightBlue,
            13 => Color::LightMagenta,
            14 => Color::LightCyan,
            15 => Color::White,
            other => Color::Indexed(other),
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_axis_labels_share_the_same_width() {
        let labels = chart_axis_labels(0.0, 1.0, true);
        assert_eq!(
            labels,
            [
                "       0".to_string(),
                "     0.5".to_string(),
                "       1".to_string()
            ]
        );

        // Decimals or not, every chart reserves CHART_LABEL_WIDTH columns
        let labels = chart_axis_labels(1.0, 1.4, false);
        assert!(
            labels
                .iter()
                .all(|label| label.chars().count() == CHART_LABEL_WIDTH)
        );
        assert_eq!(labels[0].trim(), "1.00");
        assert_eq!(labels[2].trim(), "1.40");
    }

    #[test]
    fn chart_axis_labels_grow_together_for_wide_values() {
        let labels = chart_axis_labels(0.0, 12_345_678.5, true);
        let width = labels[2].chars().count();
        assert!(width > CHART_LABEL_WIDTH, "{labels:?}");
        assert!(labels.iter().all(|label| label.chars().count() == width));
    }
}
