mod backend;
#[cfg(unix)]
mod broker;
mod manifest;
mod ui;

use crate::config::Iocc;
use backend::{DeviceParams, Event as Ev, PluginEntry, Req};
use manifest::{Action, DeviceAccess, Metric, TableData, View, ViewKind};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

pub const TABS: [&str; 4] = ["Scan", "Registers", "Plugins", "Log"];
const HISTORY: usize = 120;
const PENDING_EXPIRE: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct LogEntry {
    pub at: Duration,
    pub ok: bool,
    pub text: String,
}

pub enum ViewContent {
    Text(String),
    Table(TableData),
    Metrics(Vec<Metric>),
    Error(String),
}

pub struct ViewData {
    pub content: ViewContent,
    pub updated: Instant,
}

pub enum PromptKind {
    RegAddr,
    RegStart,
    RegCount,
    RegWrite,
    PluginArg {
        plugin: usize,
        action: usize,
        values: Vec<String>,
    },
    Confirm {
        plugin: usize,
        action: usize,
        values: Vec<String>,
    },
}

pub struct Pinned {
    pub plugin: String,
    pub view: View,
}

pub struct Prompt {
    pub title: String,
    pub hint: String,
    pub input: String,
    pub kind: PromptKind,
}

pub struct App {
    pub tab: usize,
    tx: Sender<Req>,
    rx: Receiver<Ev>,
    pub started: Instant,
    pub quit: bool,
    pub connected: Option<bool>,
    pub device_info: Option<String>,
    pub params: DeviceParams,

    pub found: BTreeSet<u8>,
    pub cursor: u8,
    pub scanning: bool,
    pub scanned: bool,

    pub reg_addr: u8,
    pub reg_start: u8,
    pub reg_count: u16,
    pub reg_data: Vec<u8>,
    pub reg_prev: Vec<u8>,
    pub reg_sel: usize,
    pub reg_poll: bool,
    reg_pending: bool,
    reg_last: Instant,
    pub read_only: bool,

    pub plugins: Vec<PluginEntry>,
    pub plugins_loaded: bool,
    pub plugin_sel: usize,
    pub action_focus: bool,
    pub action_sel: usize,
    pub views: HashMap<String, ViewData>,
    pending: HashMap<String, Instant>,
    pub history: HashMap<String, VecDeque<f64>>,
    pub action_out: Option<(bool, String)>,
    running_actions: HashSet<String>,

    pub log: Vec<LogEntry>,
    pub log_scroll: usize,
    pub status: String,
    pub prompt: Option<Prompt>,

    /// Broker socket plugins use to share the device (None off Unix)
    pub socket: Option<PathBuf>,
    /// Streaming views currently running
    pub streams: HashSet<String>,
    /// Streaming views that ended by themselves; not restarted until the selection changes
    stream_done: HashSet<String>,
    /// True while an exclusive action owns the device (monitoring paused)
    pub exclusive_running: bool,
    /// Panels kept visible while other plugins are used
    pub pinned: Vec<Pinned>,
}

pub fn view_tag(plugin: &str, view: &str) -> String {
    format!("view:{plugin}:{view}")
}

impl App {
    fn new(config: &Iocc) -> App {
        let params = DeviceParams {
            speed: config.get_speed(),
            reset: config.is_reset_on_open(),
            timeout: config.get_timeout(),
        };
        let backend = backend::spawn(params);
        let (tx, rx, socket) = (backend.tx, backend.rx, backend.socket);
        App {
            tab: 0,
            tx,
            rx,
            started: Instant::now(),
            quit: false,
            connected: None,
            device_info: None,
            params,
            found: BTreeSet::new(),
            cursor: config.get_address().min(0x77),
            scanning: false,
            scanned: false,
            reg_addr: config.get_address(),
            reg_start: 0,
            reg_count: 16,
            reg_data: Vec::new(),
            reg_prev: Vec::new(),
            reg_sel: 0,
            reg_poll: false,
            reg_pending: false,
            reg_last: Instant::now(),
            read_only: false,
            plugins: Vec::new(),
            plugins_loaded: false,
            plugin_sel: 0,
            action_focus: false,
            action_sel: 0,
            views: HashMap::new(),
            pending: HashMap::new(),
            history: HashMap::new(),
            action_out: None,
            running_actions: HashSet::new(),
            log: Vec::new(),
            log_scroll: 0,
            status: "Ready".into(),
            prompt: None,
            socket,
            streams: HashSet::new(),
            stream_done: HashSet::new(),
            exclusive_running: false,
            pinned: Vec::new(),
        }
    }

    fn send(&self, req: Req) {
        let _ = self.tx.send(req);
    }

    pub fn push_log(&mut self, ok: bool, text: impl Into<String>) {
        let text = text.into();
        self.status = text.clone();
        self.log.push(LogEntry {
            at: self.started.elapsed(),
            ok,
            text,
        });
    }

    pub fn selected_plugin(&self) -> Option<&PluginEntry> {
        self.plugins.get(self.plugin_sel)
    }

    pub fn actions(&self) -> &[Action] {
        self.selected_plugin()
            .map(|p| p.manifest.actions.as_slice())
            .unwrap_or(&[])
    }

    // ---- requests -------------------------------------------------------

    fn request_scan(&mut self) {
        if !self.scanning {
            self.scanning = true;
            self.send(Req::Scan);
        }
    }

    pub fn reg_count_clamped(&self) -> usize {
        (self.reg_count as usize).clamp(1, 256 - self.reg_start as usize)
    }

    fn request_read(&mut self) {
        if !self.reg_pending {
            self.reg_pending = true;
            self.reg_last = Instant::now();
            self.send(Req::Read {
                addr: self.reg_addr,
                reg: self.reg_start,
                count: self.reg_count_clamped(),
            });
        }
    }

    fn request_view(&mut self, plugin: &str, view: &View) {
        let tag = view_tag(plugin, &view.id);
        if view.device == DeviceAccess::Shared && self.socket.is_none() {
            self.views.insert(
                tag,
                ViewData {
                    content: ViewContent::Error(
                        "this panel needs the device broker (shared device), unavailable here"
                            .into(),
                    ),
                    updated: Instant::now(),
                },
            );
            return;
        }
        self.pending.insert(tag.clone(), Instant::now());
        self.send(Req::Plugin {
            tag,
            name: plugin.to_string(),
            args: view.command.clone(),
            shared: view.device == DeviceAccess::Shared,
        });
    }

    // ---- backend events ---------------------------------------------------

    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.on_event(ev);
        }
    }

    fn on_event(&mut self, ev: Ev) {
        match ev {
            Ev::Connection(up) => self.connected = Some(up),
            Ev::Status(Ok(info)) => self.device_info = Some(info),
            Ev::Status(Err(e)) => self.push_log(false, format!("status: {e}")),
            Ev::Scan(result) => {
                self.scanning = false;
                self.scanned = true;
                match result {
                    Ok(found) => {
                        self.push_log(true, format!("scan: {} device(s) found", found.len()));
                        self.found = found.into_iter().collect();
                    }
                    Err(e) => self.push_log(false, format!("scan: {e}")),
                }
            }
            Ev::Read { addr, reg, result } => {
                self.reg_pending = false;
                match result {
                    Ok(data) => {
                        self.reg_prev = if self.reg_data.len() == data.len() {
                            std::mem::take(&mut self.reg_data)
                        } else {
                            data.clone()
                        };
                        self.reg_data = data;
                        self.reg_sel = self.reg_sel.min(self.reg_data.len().saturating_sub(1));
                        if !self.reg_poll {
                            self.push_log(
                                true,
                                format!("read {addr:#04X} @{reg:#04X} x{}", self.reg_data.len()),
                            );
                        }
                    }
                    Err(e) => {
                        self.reg_poll = false;
                        self.push_log(false, format!("read {addr:#04X} @{reg:#04X}: {e}"));
                    }
                }
            }
            Ev::Written {
                addr,
                reg,
                data,
                result,
            } => match result {
                Ok(()) => {
                    let bytes: Vec<String> = data.iter().map(|b| format!("{b:#04X}")).collect();
                    self.push_log(
                        true,
                        format!("write {addr:#04X} @{reg:#04X} <- {}", bytes.join(" ")),
                    );
                    self.request_read();
                }
                Err(e) => self.push_log(false, format!("write {addr:#04X} @{reg:#04X}: {e}")),
            },
            Ev::Plugins(entries) => {
                self.plugins_loaded = true;
                self.plugins = entries;
                self.plugin_sel = 0;
                let described = self.plugins.iter().filter(|p| p.described).count();
                self.push_log(
                    true,
                    format!(
                        "plugins: {} found, {described} with TUI manifest",
                        self.plugins.len()
                    ),
                );
            }
            Ev::Plugin {
                tag,
                name,
                args,
                result,
            } => self.on_plugin_result(tag, name, args, result),
            Ev::StreamLine { tag, line } => self.on_stream_line(&tag, &line),
            Ev::StreamEnd {
                tag,
                code,
                stderr,
                stopped,
            } => self.on_stream_end(&tag, code, &stderr, stopped),
        }
    }

    fn on_plugin_result(
        &mut self,
        tag: String,
        name: String,
        args: Vec<String>,
        result: Result<backend::PluginOutput, String>,
    ) {
        let command = format!("iocc {name} {}", args.join(" "));
        if let Some(rest) = tag.strip_prefix("view:") {
            self.pending.remove(&tag);
            let view_kind = rest
                .split_once(':')
                .and_then(|(plugin, id)| {
                    self.plugins
                        .iter()
                        .find(|p| p.name == plugin)?
                        .manifest
                        .views
                        .iter()
                        .find(|v| v.id == id)
                        .map(|v| v.kind)
                })
                .unwrap_or_default();
            let content = match result {
                Ok(out) if out.code == 0 => self.parse_view(&tag, view_kind, out.stdout),
                Ok(out) => ViewContent::Error(format!(
                    "exit code {}\n{}{}",
                    out.code, out.stdout, out.stderr
                )),
                Err(e) => ViewContent::Error(e),
            };
            if let ViewContent::Error(e) = &content {
                let first = e.lines().next().unwrap_or("").to_string();
                self.push_log(false, format!("{command}: {first}"));
            }
            self.views.insert(
                tag,
                ViewData {
                    content,
                    updated: Instant::now(),
                },
            );
        } else {
            self.running_actions.remove(&tag);
            self.exclusive_running = false;
            match result {
                Ok(out) => {
                    let ok = out.code == 0;
                    let mut text = out.stdout;
                    if !out.stderr.trim().is_empty() {
                        text.push_str(&out.stderr);
                    }
                    self.push_log(ok, format!("{command} -> exit {}", out.code));
                    self.action_out = Some((ok, text));
                }
                Err(e) => {
                    self.push_log(false, format!("{command}: {e}"));
                    self.action_out = Some((false, e));
                }
            }
        }
    }

    fn parse_view(&mut self, tag: &str, kind: ViewKind, stdout: String) -> ViewContent {
        match kind {
            ViewKind::Text => ViewContent::Text(stdout),
            ViewKind::Table => manifest::parse_table(&stdout)
                .map(ViewContent::Table)
                .unwrap_or_else(ViewContent::Error),
            ViewKind::Metrics => match manifest::parse_metrics(&stdout) {
                Ok(metrics) => {
                    for m in &metrics {
                        let h = self.history.entry(format!("{tag}:{}", m.name)).or_default();
                        if h.len() == HISTORY {
                            h.pop_front();
                        }
                        h.push_back(m.value);
                    }
                    ViewContent::Metrics(metrics)
                }
                Err(e) => ViewContent::Error(e),
            },
        }
    }

    // ---- periodic work ----------------------------------------------------

    fn tick(&mut self) {
        self.pending.retain(|_, at| at.elapsed() < PENDING_EXPIRE);

        if self.tab == 1
            && self.reg_poll
            && !self.reg_pending
            && self.reg_last.elapsed() > POLL_INTERVAL
        {
            self.request_read();
        }

        if self.tab == 2 && !self.plugins_loaded && !self.pending.contains_key("load") {
            self.pending.insert("load".into(), Instant::now());
            self.send(Req::LoadPlugins);
        }
        if self.plugins_loaded && (self.tab == 2 || !self.pinned.is_empty()) {
            self.tick_views();
        }
    }

    /**
     * Views that must be alive right now: the pinned ones plus the selected plugin
     */
    fn wanted_views(&self) -> Vec<(String, View)> {
        let mut wanted: Vec<(String, View)> = self
            .pinned
            .iter()
            .map(|p| (p.plugin.clone(), p.view.clone()))
            .collect();
        if let Some(plugin) = self.selected_plugin() {
            for view in &plugin.manifest.views {
                let tag = view_tag(&plugin.name, &view.id);
                if !wanted.iter().any(|(p, v)| view_tag(p, &v.id) == tag) {
                    wanted.push((plugin.name.clone(), view.clone()));
                }
            }
        }
        wanted
    }

    fn tick_views(&mut self) {
        let wanted = self.wanted_views();
        let tags: HashSet<String> = wanted
            .iter()
            .map(|(plugin, view)| view_tag(plugin, &view.id))
            .collect();

        // Streams that are no longer wanted (selection or tab change)
        for tag in self.streams.iter().cloned().collect::<Vec<_>>() {
            if !tags.contains(&tag) {
                self.send(Req::StreamStop { tag: tag.clone() });
                self.streams.remove(&tag);
                self.views.remove(&tag);
            }
        }
        self.stream_done.retain(|tag| tags.contains(tag));

        if self.exclusive_running {
            return;
        }
        for (name, view) in wanted {
            let tag = view_tag(&name, &view.id);
            if view.stream {
                if self.streams.contains(&tag) || self.stream_done.contains(&tag) {
                    continue;
                }
                if view.device == DeviceAccess::Shared && self.socket.is_none() {
                    continue;
                }
                self.streams.insert(tag.clone());
                self.send(Req::StreamStart {
                    tag,
                    name: name.clone(),
                    args: view.command.clone(),
                });
            } else {
                if self.pending.contains_key(&tag) {
                    continue;
                }
                let due = match (self.views.get(&tag), view.refresh_ms) {
                    (None, _) => true,
                    (Some(d), Some(ms)) => d.updated.elapsed() >= Duration::from_millis(ms),
                    (Some(_), None) => false,
                };
                if due {
                    self.request_view(&name, &view);
                }
            }
        }
    }

    /**
     * Keeps a panel visible while other plugins are used (press again to unpin)
     */
    fn toggle_pin(&mut self) {
        let Some(plugin) = self.selected_plugin() else {
            return;
        };
        let name = plugin.name.clone();
        let Some(view) = plugin
            .manifest
            .views
            .iter()
            .find(|v| v.stream)
            .or_else(|| plugin.manifest.views.first())
            .cloned()
        else {
            self.push_log(false, format!("{name} has no panel to pin"));
            return;
        };
        let tag = view_tag(&name, &view.id);
        if let Some(pos) = self
            .pinned
            .iter()
            .position(|p| view_tag(&p.plugin, &p.view.id) == tag)
        {
            self.pinned.remove(pos);
            self.send(Req::StreamStop { tag: tag.clone() });
            self.streams.remove(&tag);
            self.views.remove(&tag);
            self.push_log(true, format!("unpinned {name}"));
        } else {
            let title = view.title().to_string();
            self.pinned.push(Pinned {
                plugin: name.clone(),
                view,
            });
            self.push_log(
                true,
                format!("pinned {name}: {title} (stays visible on every tab)"),
            );
        }
    }

    // ---- streams ------------------------------------------------------------

    fn stop_streams(&mut self) {
        for tag in std::mem::take(&mut self.streams) {
            self.send(Req::StreamStop { tag: tag.clone() });
            self.views.remove(&tag);
        }
    }

    fn view_kind(&self, tag: &str) -> ViewKind {
        tag.strip_prefix("view:")
            .and_then(|rest| rest.split_once(':'))
            .and_then(|(plugin, id)| {
                self.plugins
                    .iter()
                    .find(|p| p.name == plugin)?
                    .manifest
                    .views
                    .iter()
                    .find(|v| v.id == id)
                    .map(|v| v.kind)
            })
            .unwrap_or_default()
    }

    fn on_stream_line(&mut self, tag: &str, line: &str) {
        if line.trim().is_empty() || !self.streams.contains(tag) {
            return;
        }
        let kind = self.view_kind(tag);
        let content = match kind {
            ViewKind::Metrics => match manifest::parse_metrics(line) {
                Ok(metrics) => {
                    for m in &metrics {
                        let h = self.history.entry(format!("{tag}:{}", m.name)).or_default();
                        if h.len() == HISTORY {
                            h.pop_front();
                        }
                        h.push_back(m.value);
                    }
                    ViewContent::Metrics(metrics)
                }
                Err(e) => ViewContent::Error(e),
            },
            ViewKind::Table => match manifest::parse_table(line) {
                Ok(table) => ViewContent::Table(table),
                Err(e) => ViewContent::Error(e),
            },
            ViewKind::Text => {
                let mut text = match self.views.get(tag).map(|d| &d.content) {
                    Some(ViewContent::Text(t)) => t.clone(),
                    _ => String::new(),
                };
                text.push_str(line);
                text.push('\n');
                if text.lines().count() > 200 {
                    text = text.lines().skip(1).collect::<Vec<_>>().join("\n") + "\n";
                }
                ViewContent::Text(text)
            }
        };
        self.views.insert(
            tag.to_string(),
            ViewData {
                content,
                updated: Instant::now(),
            },
        );
    }

    fn on_stream_end(&mut self, tag: &str, code: Option<i32>, stderr: &str, stopped: bool) {
        if stopped {
            // Paused for an exclusive plugin or a context switch: restart when possible
            self.streams.remove(tag);
            self.views.remove(tag);
            return;
        }
        if !self.streams.remove(tag) {
            return;
        }
        self.stream_done.insert(tag.to_string());
        let message = match (code, stderr.trim()) {
            (Some(0), _) => "command exited".to_string(),
            (Some(code), "") => format!("command exited with code {code}"),
            (Some(code), stderr) => format!("command exited with code {code}: {stderr}"),
            (None, "") => "command stopped".to_string(),
            (None, stderr) => format!("command stopped: {stderr}"),
        };
        self.push_log(false, message.clone());
        self.views.insert(
            tag.to_string(),
            ViewData {
                content: ViewContent::Error(message),
                updated: Instant::now(),
            },
        );
    }

    fn reload_plugins(&mut self) {
        self.stop_streams();
        self.stream_done.clear();
        self.plugins_loaded = false;
        self.views.clear();
        self.pending.remove("load");
    }

    // ---- keyboard -----------------------------------------------------------

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.prompt.is_some() {
            self.on_prompt_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Tab => self.set_tab((self.tab + 1) % TABS.len()),
            KeyCode::BackTab => self.set_tab((self.tab + TABS.len() - 1) % TABS.len()),
            KeyCode::Char(c @ '1'..='4') => self.set_tab(c as usize - '1' as usize),
            _ => match self.tab {
                0 => self.on_scan_key(key.code),
                1 => self.on_regs_key(key.code),
                2 => self.on_plugins_key(key.code),
                _ => self.on_log_key(key.code),
            },
        }
    }

    fn set_tab(&mut self, tab: usize) {
        self.tab = tab;
        if tab == 0 && !self.scanned {
            self.request_scan();
        }
    }

    fn on_scan_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(0x7F),
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(16),
            KeyCode::Down => self.cursor = (self.cursor + 16).min(0x7F),
            KeyCode::Char('s') => self.request_scan(),
            KeyCode::Char('i') => self.send(Req::Status),
            KeyCode::Enter => {
                self.reg_addr = self.cursor;
                self.reg_data.clear();
                self.tab = 1;
                self.request_read();
            }
            _ => {}
        }
    }

    fn open_prompt(&mut self, title: &str, hint: &str, kind: PromptKind, initial: String) {
        self.prompt = Some(Prompt {
            title: title.into(),
            hint: hint.into(),
            input: initial,
            kind,
        });
    }

    fn on_regs_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up => self.reg_sel = self.reg_sel.saturating_sub(1),
            KeyCode::Down => {
                self.reg_sel = (self.reg_sel + 1).min(self.reg_data.len().saturating_sub(1))
            }
            KeyCode::Char('r') => self.request_read(),
            KeyCode::Char('p') => self.reg_poll = !self.reg_poll,
            KeyCode::Char('m') => self.read_only = !self.read_only,
            KeyCode::Char('a') => self.open_prompt(
                "Device address",
                "7bit hex, 0x03-0x77",
                PromptKind::RegAddr,
                format!("{:02X}", self.reg_addr),
            ),
            KeyCode::Char('g') => self.open_prompt(
                "Start register",
                "hex, 0x00-0xFF",
                PromptKind::RegStart,
                format!("{:02X}", self.reg_start),
            ),
            KeyCode::Char('n') => self.open_prompt(
                "Bytes to read",
                "decimal, 1-256",
                PromptKind::RegCount,
                self.reg_count.to_string(),
            ),
            KeyCode::Enter if self.reg_data.is_empty() => self.request_read(),
            KeyCode::Enter if self.read_only => {
                self.push_log(false, "read only mode is on, press m to allow writes")
            }
            KeyCode::Enter => {
                let reg = self.reg_start as usize + self.reg_sel;
                self.open_prompt(
                    &format!("Write register {reg:#04X}"),
                    "hex byte",
                    PromptKind::RegWrite,
                    String::new(),
                );
            }
            _ => {}
        }
    }

    fn on_plugins_key(&mut self, code: KeyCode) {
        let count = self.plugins.len();
        if self.action_focus {
            let n = self.actions().len();
            match code {
                KeyCode::Up => self.action_sel = self.action_sel.saturating_sub(1),
                KeyCode::Down => self.action_sel = (self.action_sel + 1).min(n.saturating_sub(1)),
                KeyCode::Esc | KeyCode::Left => self.action_focus = false,
                KeyCode::Enter if n > 0 => self.start_action(self.plugin_sel, self.action_sel),
                _ => {}
            }
            return;
        }
        match code {
            KeyCode::Up => self.select_plugin(self.plugin_sel.saturating_sub(1)),
            KeyCode::Down => self.select_plugin((self.plugin_sel + 1).min(count.saturating_sub(1))),
            KeyCode::Right | KeyCode::Enter if !self.actions().is_empty() => {
                self.action_focus = true;
                self.action_sel = 0;
            }
            KeyCode::Char('R') => self.reload_plugins(),
            KeyCode::Char('p') => self.toggle_pin(),
            KeyCode::Char('P') => {
                self.stop_streams();
                self.stream_done.clear();
                self.pinned.clear();
                self.push_log(true, "all watches unpinned");
            }
            _ => {}
        }
    }

    fn select_plugin(&mut self, index: usize) {
        if index != self.plugin_sel {
            self.plugin_sel = index;
            self.action_out = None;
        }
    }

    fn on_log_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up => self.log_scroll = (self.log_scroll + 1).min(self.log.len()),
            KeyCode::Down => self.log_scroll = self.log_scroll.saturating_sub(1),
            KeyCode::Char('c') => {
                self.log.clear();
                self.log_scroll = 0;
            }
            _ => {}
        }
    }

    /**
     * Collects the arguments of an action one prompt at a time, then runs it
     */
    fn start_action(&mut self, plugin: usize, action: usize) {
        self.continue_action(plugin, action, Vec::new());
    }

    fn continue_action(&mut self, plugin: usize, action: usize, values: Vec<String>) {
        let Some(act) = self
            .plugins
            .get(plugin)
            .and_then(|p| p.manifest.actions.get(action))
            .cloned()
        else {
            return;
        };
        if let Some(spec) = act.args.get(values.len()) {
            let hint = match spec.options.is_empty() {
                true => String::new(),
                false => format!("options: {}", spec.options.join(", ")),
            };
            self.open_prompt(
                &format!("{} - {}", act.title(), spec.title()),
                &hint,
                PromptKind::PluginArg {
                    plugin,
                    action,
                    values,
                },
                spec.default.clone().unwrap_or_default(),
            );
        } else if act.confirm {
            self.open_prompt(
                &format!("Run '{}'?", act.title()),
                "type y to confirm",
                PromptKind::Confirm {
                    plugin,
                    action,
                    values,
                },
                String::new(),
            );
        } else {
            self.run_action(plugin, &act, &values);
        }
    }

    fn run_action(&mut self, plugin: usize, act: &Action, values: &[String]) {
        let Some(name) = self.plugins.get(plugin).map(|p| p.name.clone()) else {
            return;
        };
        let tag = format!("action:{name}:{}", act.id);
        if !self.running_actions.insert(tag.clone()) {
            return;
        }
        let shared = act.device == DeviceAccess::Shared;
        if shared && self.socket.is_none() {
            self.push_log(
                false,
                "action needs a shared device but no broker socket is available",
            );
            self.running_actions.remove(&tag);
            return;
        }
        if !shared {
            // The plugin opens the chip itself: pause monitoring until it finishes
            self.exclusive_running = true;
            self.stop_streams();
        }
        self.action_out = Some((true, "Running...".into()));
        self.send(Req::Plugin {
            tag,
            name,
            args: act.build_args(values),
            shared,
        });
    }

    fn on_prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Backspace => {
                if let Some(p) = &mut self.prompt {
                    p.input.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(p) = &mut self.prompt {
                    p.input.push(c);
                }
            }
            KeyCode::Enter => {
                let Some(prompt) = self.prompt.take() else {
                    return;
                };
                if let Err(msg) = self.submit_prompt(&prompt) {
                    self.status = msg;
                    self.prompt = Some(prompt);
                }
            }
            _ => {}
        }
    }

    fn submit_prompt(&mut self, prompt: &Prompt) -> Result<(), String> {
        let input = prompt.input.trim();
        let hex = |s: &str| {
            let s = s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .unwrap_or(s);
            u8::from_str_radix(s, 16).map_err(|_| format!("'{s}' is not a hex byte"))
        };
        match &prompt.kind {
            PromptKind::RegAddr => {
                let a = hex(input)?;
                if !(0x03..=0x77).contains(&a) {
                    return Err("address must be between 0x03 and 0x77".into());
                }
                self.reg_addr = a;
                self.reg_data.clear();
                self.request_read();
            }
            PromptKind::RegStart => {
                self.reg_start = hex(input)?;
                self.reg_data.clear();
                self.request_read();
            }
            PromptKind::RegCount => {
                let n: u16 = input.parse().map_err(|_| "not a number".to_string())?;
                if !(1..=256).contains(&n) {
                    return Err("count must be between 1 and 256".into());
                }
                self.reg_count = n;
                self.reg_data.clear();
                self.request_read();
            }
            PromptKind::RegWrite => {
                let value = hex(input)?;
                self.send(Req::Write {
                    addr: self.reg_addr,
                    reg: self.reg_start.wrapping_add(self.reg_sel as u8),
                    data: vec![value],
                });
            }
            PromptKind::PluginArg {
                plugin,
                action,
                values,
            } => {
                let spec = self
                    .plugins
                    .get(*plugin)
                    .and_then(|p| p.manifest.actions.get(*action))
                    .and_then(|a| a.args.get(values.len()))
                    .cloned();
                if let Some(spec) = spec {
                    spec.validate(input)?;
                }
                let mut values = values.clone();
                values.push(input.to_string());
                self.continue_action(*plugin, *action, values);
            }
            PromptKind::Confirm {
                plugin,
                action,
                values,
            } => {
                if matches!(input, "y" | "Y" | "yes")
                    && let Some(act) = self
                        .plugins
                        .get(*plugin)
                        .and_then(|p| p.manifest.actions.get(*action))
                        .cloned()
                {
                    self.run_action(*plugin, &act, values);
                }
            }
        }
        Ok(())
    }
}

/**
 * Runs the TUI until the user quits
 */
pub fn run(config: &Iocc) -> std::io::Result<()> {
    let mut app = App::new(config);
    app.send(Req::Status);
    app.request_scan();
    let mut terminal = ratatui::try_init()?;
    let result = (|| {
        while !app.quit {
            terminal.draw(|frame| ui::draw(frame, &app))?;
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
            {
                app.on_key(key);
            }
            app.drain_events();
            app.tick();
        }
        Ok(())
    })();
    app.stop_streams();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn render(app: &App) -> String {
        let mut term = Terminal::new(TestBackend::new(110, 32)).unwrap();
        term.draw(|f| ui::draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_all_tabs() {
        let mut app = App::new(&Iocc::default());
        app.connected = Some(true);
        app.found.extend([0x20, 0x4A]);
        app.reg_data = vec![0xFF, 0x00, 0x08, 0x41];
        app.reg_prev = vec![0xFF, 0x00, 0x00, 0x41];
        app.push_log(true, "scan: 2 device(s) found");

        let scan = render(&app);
        assert!(scan.contains("I2C bus") && scan.contains("■"));

        app.tab = 1;
        let regs = render(&app);
        assert!(regs.contains("00001000") && regs.contains("0x03"));

        app.tab = 3;
        assert!(render(&app).contains("scan: 2 device(s)"));
        println!("{scan}\n{regs}");
    }

    #[cfg(unix)]
    #[test]
    fn demo_plugin_end_to_end() {
        let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/examples");
        // SAFETY: no other test reads or writes PATH
        unsafe { std::env::set_var("PATH", format!("{examples}:/usr/bin:/bin")) };

        let mut app = App::new(&Iocc::default());
        app.tab = 2;
        let wait = |app: &mut App, cond: &dyn Fn(&App) -> bool| {
            for _ in 0..100 {
                app.drain_events();
                app.tick();
                if cond(app) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("timed out\n{}", render(app));
        };

        wait(&mut app, &|a| a.plugins_loaded);
        app.plugin_sel = app.plugins.iter().position(|p| p.name == "demo").unwrap();
        let demo = app.selected_plugin().unwrap();
        assert!(demo.described);
        assert_eq!(demo.manifest.views.len(), 3);

        wait(&mut app, &|a| a.views.len() == 3);
        let screen = render(&app);
        assert!(
            screen.contains("Voltage") && screen.contains("IOCC_TUI=1"),
            "{screen}"
        );

        // pinning keeps the live panel (and its stream) alive on other tabs
        let live = view_tag("demo", "live");
        app.toggle_pin();
        assert_eq!(app.pinned.len(), 1);
        app.set_tab(3);
        app.tick();
        let watches = render(&app);
        assert!(
            watches.contains("Watches") && watches.contains("Voltage"),
            "{watches}"
        );
        assert!(app.streams.contains(&live));

        // an exclusive action pauses the watch and it restarts afterwards
        app.continue_action(app.plugin_sel, 0, vec!["b".into(), "on".into()]);
        wait(&mut app, &|a| {
            a.action_out
                .as_ref()
                .is_some_and(|(ok, t)| *ok && t.contains("b -> on"))
        });
        wait(&mut app, &|a| a.streams.contains(&live));

        // a second plugin can be pinned at the same time (examples/iocc-wave)
        let wave = app
            .plugins
            .iter()
            .position(|p| p.name == "wave")
            .expect("wave demo plugin");
        app.select_plugin(wave);
        let signal = view_tag("wave", "live");
        wait(&mut app, &|a| a.streams.contains(&signal));
        app.toggle_pin();
        assert_eq!(app.pinned.len(), 2);
        app.set_tab(0);
        app.tick();
        let watches = render(&app);
        assert!(
            watches.contains("Live sensors") && watches.contains("Live signals"),
            "{watches}"
        );
        assert!(app.streams.contains(&live) && app.streams.contains(&signal));
        println!("{}", render(&app));
    }
}
