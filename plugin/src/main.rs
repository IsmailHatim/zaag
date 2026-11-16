//! zaag-sidebar: the agents of every zellij session, in a sidebar (`mode "sidebar"`) or in a
//! searchable list (`mode "picker"`). It draws what `zaag list --machines` answers and never
//! works anything out by itself.
use serde::Deserialize;
use std::collections::BTreeMap;
use zellij_tile::prelude::*;

const GREEN: u8 = 114;
const OLIVE: u8 = 143;
const YELLOW: u8 = 221;
const NAME: u8 = 245;
const RULE: u8 = 238;
const RED: u8 = 203;
const DIM: u8 = 243;
const TEXT: u8 = 223;
const PINK: u8 = 175;
const HERE_BG: u8 = 236;
const PICK_BG: u8 = 172;
const PICK_FG: u8 = 234;
const POLL_SECS: f64 = 2.0;

#[derive(Deserialize, Clone, Default, Debug)]
struct Agent {
    id: String,
    session: String,
    pane: u32,
    name: String,
    #[serde(default)]
    agent: Option<String>,
    status: String,
    #[serde(default)]
    branch: Option<String>,
}

#[derive(Deserialize, Clone, Default, Debug)]
struct Machine {
    id: String,
    label: String,
    online: bool,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    agents: Vec<Agent>,
    #[serde(default)]
    sessions: Vec<String>,
}

#[derive(Deserialize)]
struct Listing {
    machines: Vec<Machine>,
}

#[derive(Deserialize)]
struct Answer {
    result: Option<Listing>,
}

#[derive(Clone, Debug)]
enum Target {
    Session { machine: String, name: String },
    Agent { machine: String, agent: Agent },
}

struct Seg {
    text: String,
    fg: u8,
    bold: bool,
}

fn seg(text: impl Into<String>, fg: u8) -> Seg {
    Seg { text: text.into(), fg, bold: false }
}

fn bold(text: impl Into<String>, fg: u8) -> Seg {
    Seg { text: text.into(), fg, bold: true }
}

#[derive(Default)]
struct Line {
    left: Vec<Seg>,
    right: Vec<Seg>,
    bg: Option<u8>,
    target: Option<Target>,
}

#[derive(PartialEq, Default)]
enum Mode {
    #[default]
    Sidebar,
    Picker,
}

#[derive(Default)]
struct State {
    mode: Mode,
    zaag: String,
    machines: Vec<Machine>,
    loaded: bool,
    session: Option<String>,
    active_tab: usize,
    manifest: PaneManifest,
    focused: Option<u32>,
    granted: bool,
    visible: bool,
    timer_armed: bool,
    pending: u32,
    hits: Vec<Option<Target>>,
    scroll: usize,
    query: String,
    selected: usize,
    renaming: Option<(Target, String)>,
    note: Option<String>,
    seen_sent: Option<String>,
    cols: usize,
    buttons_row: Option<usize>,
    follow: Option<String>,
    last_timer: Option<std::time::Instant>,
    list_seq: u32,
    follow_from: u32,
}

register_plugin!(State);

fn ask_permission() {
    request_permission(&[PermissionType::ReadApplicationState, PermissionType::ChangeApplicationState, PermissionType::RunCommands]);
}

fn rank(status: &str) -> u8 {
    match status {
        "blocked" => 0,
        "done" => 1,
        "working" => 2,
        "idle" => 3,
        _ => 4,
    }
}

fn width(s: &str) -> usize {
    s.chars().count()
}

fn clip(s: &str, max: usize) -> String {
    if width(s) <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        s.chars().take(max - 1).chain(std::iter::once('…')).collect()
    }
}

impl State {
    fn dot(&self, status: &str) -> Seg {
        match status {
            "blocked" => seg("●", RED),
            "done" => seg("●", GREEN),
            "working" => seg("●", YELLOW),
            "idle" => seg("○", OLIVE),
            _ => seg("·", DIM),
        }
    }

    fn zaag(&self, args: &[&str], kind: &str) {
        let mut cmd = vec![self.zaag.as_str()];
        cmd.extend_from_slice(args);
        let mut ctx = BTreeMap::new();
        ctx.insert("kind".to_string(), kind.to_string());
        run_command(&cmd, ctx);
    }

    fn zaag_on(&self, machine: &str, args: &[&str], kind: &str) {
        if machine == "local" {
            self.zaag(args, kind);
        } else {
            let mut all = vec!["-m", machine];
            all.extend_from_slice(args);
            self.zaag(&all, kind);
        }
    }

    fn poll(&mut self) {
        if !self.granted || !self.visible {
            return;
        }
        // a listing that never came back (ssh hanging) must not stop the next ones for ever
        if self.pending == 0 || self.pending > 8 {
            self.pending = 1;
            self.list_seq += 1;
            let mut ctx = BTreeMap::new();
            ctx.insert("kind".to_string(), "list".to_string());
            ctx.insert("seq".to_string(), self.list_seq.to_string());
            run_command(&[self.zaag.as_str(), "list", "--machines"], ctx);
        } else {
            self.pending += 1;
        }
    }

    fn arm(&mut self) {
        if !self.timer_armed && self.visible && self.granted {
            self.timer_armed = true;
            set_timeout(POLL_SECS);
        }
    }

    fn refocus(&mut self) {
        self.focused = self
            .manifest
            .panes
            .get(&self.active_tab)
            .and_then(|panes| panes.iter().find(|p| !p.is_plugin && p.is_focused && !p.is_floating).or_else(|| panes.iter().find(|p| !p.is_plugin && p.is_focused)))
            .map(|p| p.id);
    }

    /// Machine names are only worth a line when more than one machine has something to show.
    fn many(&self) -> bool {
        self.machines.iter().filter(|m| !m.online || !m.sessions.is_empty()).count() > 1
    }

    /// With a search typed, Enter should reach the first agent found, not its project's line.
    fn select_first_agent(&mut self) {
        self.selected = self.picker_targets().iter().position(|t| matches!(t, Target::Agent { .. })).filter(|_| !self.query.is_empty()).unwrap_or(0);
    }

    fn is_here(&self, machine: &str, session: &str) -> bool {
        machine == "local" && self.session.as_deref() == Some(session)
    }

    fn is_focused(&self, machine: &str, a: &Agent) -> bool {
        self.is_here(machine, &a.session) && self.focused == Some(a.pane)
    }

    /// Looking at a finished agent is what makes it idle again.
    fn mark_seen(&mut self) {
        if self.mode != Mode::Sidebar || !self.visible {
            return;
        }
        let found = self
            .machines
            .iter()
            .filter(|m| m.id == "local")
            .flat_map(|m| m.agents.iter())
            .find(|a| a.status == "done" && self.session.as_deref() == Some(a.session.as_str()) && self.focused == Some(a.pane))
            .map(|a| a.id.clone());
        match found {
            Some(id) if self.seen_sent.as_deref() != Some(id.as_str()) => {
                self.zaag(&["seen", &id], "act");
                self.seen_sent = Some(id);
            }
            None => self.seen_sent = None,
            _ => {}
        }
    }

    fn jump(&mut self, target: &Target) {
        match target {
            Target::Agent { machine, agent } if machine == "local" => {
                if self.session.as_deref() == Some(agent.session.as_str()) {
                    focus_terminal_pane(agent.pane, false, false);
                } else {
                    switch_session_with_focus(&agent.session, None, Some((agent.pane, false)));
                }
            }
            Target::Session { machine, name } if machine == "local" => {
                if self.session.as_deref() != Some(name.as_str()) {
                    switch_session(Some(name));
                }
            }
            Target::Agent { machine, agent } => {
                self.note = Some(format!("on {machine}: ssh -t {machine} zellij attach {}", agent.session));
            }
            Target::Session { machine, name } => {
                self.note = Some(format!("on {machine}: ssh -t {machine} zellij attach {name}"));
            }
        }
    }

    // ------------------------------------------------------------ sidebar

    /// The top half: one entry per project, its dot the most urgent of its agents.
    fn project_lines(&self) -> Vec<Line> {
        let mut lines = vec![Line { left: vec![seg(" projects", DIM)], ..Default::default() }, Line::default()];
        let many = self.many();
        for m in &self.machines {
            if !m.online || many && !m.sessions.is_empty() {
                let mut left = vec![seg(format!(" {}", m.label), DIM)];
                if !m.online {
                    left.push(seg(format!(" · {}", m.reason.clone().unwrap_or_else(|| "offline".into())), RED));
                }
                lines.push(Line { left, ..Default::default() });
            }
            for s in &m.sessions {
                let agents: Vec<&Agent> = m.agents.iter().filter(|a| &a.session == s).collect();
                let status = agents.iter().map(|a| a.status.as_str()).min_by_key(|st| rank(st)).unwrap_or("idle");
                let here = self.is_here(&m.id, s);
                let bg = if here { Some(HERE_BG) } else { None };
                let target = Some(Target::Session { machine: m.id.clone(), name: s.clone() });
                lines.push(Line {
                    left: vec![seg(" ", TEXT), self.dot(status), seg(" ", TEXT), if here { bold(s.clone(), TEXT) } else { seg(s.clone(), TEXT) }],
                    bg,
                    target: target.clone(),
                    ..Default::default()
                });
                if let Some(branch) = agents.iter().find_map(|a| a.branch.clone()) {
                    lines.push(Line { left: vec![seg(format!("   {branch}"), if here { PINK } else { DIM })], bg, target, ..Default::default() });
                }
            }
        }
        if !self.loaded {
            lines.push(Line { left: vec![seg(" …", DIM)], ..Default::default() });
        }
        lines
    }

    /// The bottom half: every agent, the most urgent first.
    fn agent_lines(&self) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut all: Vec<(&Machine, &Agent)> = self.machines.iter().flat_map(|m| m.agents.iter().map(move |a| (m, a))).collect();
        all.sort_by_key(|(_, a)| rank(&a.status));
        for (m, a) in all {
            let bg = if self.is_focused(&m.id, a) { Some(HERE_BG) } else { None };
            let target = Some(Target::Agent { machine: m.id.clone(), agent: a.clone() });
            lines.push(Line {
                left: vec![seg(" ", TEXT), self.dot(&a.status), seg(" ", TEXT), bold(a.session.clone(), TEXT), seg(" · ", DIM), seg(a.name.clone(), NAME)],
                bg,
                target: target.clone(),
                ..Default::default()
            });
            let mut second = format!("   {}", a.agent.clone().unwrap_or_else(|| "terminal".into()));
            if m.id != "local" {
                second.push_str(&format!(" · {}", m.label));
            }
            lines.push(Line { left: vec![seg(second, DIM)], bg, target, ..Default::default() });
        }
        if let Some(note) = &self.note {
            lines.push(Line::default());
            lines.push(Line { left: vec![seg(format!(" {note}"), OLIVE)], ..Default::default() });
        }
        lines
    }

    // ------------------------------------------------------------ picker

    fn matches(&self, m: &Machine, a: &Agent) -> bool {
        let q = self.query.to_lowercase();
        q.split_whitespace().all(|w| {
            a.session.to_lowercase().contains(w)
                || a.name.to_lowercase().contains(w)
                || a.status.contains(w)
                || a.agent.as_deref().unwrap_or("").contains(w)
                || m.label.to_lowercase().contains(w)
        })
    }

    /// The rows under the search box; the selectable ones carry a target.
    fn picker_lines(&self) -> Vec<Line> {
        let mut lines = Vec::new();
        let many = self.many();
        for m in &self.machines {
            let mut body = Vec::new();
            for s in &m.sessions {
                let agents: Vec<&Agent> = m.agents.iter().filter(|a| &a.session == s && self.matches(m, a)).collect();
                let session_matches = !self.query.is_empty() && s.to_lowercase().contains(&self.query.to_lowercase());
                if agents.is_empty() && !(self.query.is_empty() || session_matches) {
                    continue;
                }
                let branch = m.agents.iter().filter(|a| &a.session == s).find_map(|a| a.branch.clone()).unwrap_or_default();
                body.push(Line {
                    left: vec![seg(" ", TEXT), bold(s.clone(), TEXT)],
                    right: vec![bold(format!("{branch} "), TEXT)],
                    target: Some(Target::Session { machine: m.id.clone(), name: s.clone() }),
                    ..Default::default()
                });
                for (i, a) in agents.iter().enumerate() {
                    let tree = if i + 1 == agents.len() { " └─ " } else { " ├─ " };
                    body.push(Line {
                        left: vec![seg(tree, DIM), self.dot(&a.status), seg("  ", TEXT), seg(a.name.clone(), TEXT)],
                        right: vec![seg(format!("{:<10}", a.agent.clone().unwrap_or_else(|| "terminal".into())), DIM), seg(format!("{:<9}", a.status), DIM)],
                        target: Some(Target::Agent { machine: m.id.clone(), agent: (*a).clone() }),
                        ..Default::default()
                    });
                }
            }
            if (many && !body.is_empty()) || !m.online {
                let mut left = vec![seg(format!(" {}", m.label), DIM)];
                if !m.online {
                    left.push(seg(format!(" · {}", m.reason.clone().unwrap_or_else(|| "offline".into())), RED));
                }
                lines.push(Line { left, ..Default::default() });
            }
            lines.extend(body);
        }
        lines
    }

    fn picker_targets(&self) -> Vec<Target> {
        self.picker_lines().into_iter().filter_map(|l| l.target).collect()
    }

    fn close_picker(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.renaming = None;
        self.note = None;
        close_self(); // a new one each time: nothing shown can be out of date
    }

    fn picker_key(&mut self, key: KeyWithModifier) -> bool {
        let ctrl = key.key_modifiers.contains(&KeyModifier::Ctrl);
        let alt = key.key_modifiers.contains(&KeyModifier::Alt);
        if let Some((target, text)) = self.renaming.as_mut() {
            match key.bare_key {
                BareKey::Esc => self.renaming = None,
                BareKey::Backspace => {
                    text.pop();
                }
                BareKey::Char(c) if !ctrl && !alt => text.push(c),
                BareKey::Enter => {
                    let name = text.trim().to_string();
                    let target = target.clone();
                    self.renaming = None;
                    if !name.is_empty() {
                        match &target {
                            Target::Agent { machine, agent } => self.zaag_on(machine, &["rename", &agent.id, &name], "act"),
                            Target::Session { machine, name: old } => self.zaag_on(machine, &["rename-session", old, &name], "act"),
                        }
                    }
                }
                _ => {}
            }
            return true;
        }
        let targets = self.picker_targets();
        match key.bare_key {
            BareKey::Esc => self.close_picker(),
            BareKey::Enter => {
                if let Some(t) = targets.get(self.selected).cloned() {
                    self.jump(&t);
                    if self.note.is_none() {
                        self.close_picker();
                    }
                }
            }
            BareKey::Down | BareKey::Tab => self.selected = (self.selected + 1).min(targets.len().saturating_sub(1)),
            BareKey::Up => self.selected = self.selected.saturating_sub(1),
            BareKey::Char('n') if ctrl => self.selected = (self.selected + 1).min(targets.len().saturating_sub(1)),
            BareKey::Char('p') if ctrl => self.selected = self.selected.saturating_sub(1),
            BareKey::Char(c @ ('u' | 'd')) if ctrl => {
                // move the project under the cursor (or the cursor's agent's project) up or down
                if let Some(t) = targets.get(self.selected) {
                    let (machine, session) = match t {
                        Target::Agent { machine, agent } => (machine.clone(), agent.session.clone()),
                        Target::Session { machine, name } => (machine.clone(), name.clone()),
                    };
                    self.zaag_on(&machine, &["move", &session, if c == 'u' { "up" } else { "down" }], "act");
                    self.follow = Some(session);
                    self.follow_from = u32::MAX; // until the move has been done
                }
            }
            BareKey::Char('r') if ctrl => {
                if let Some(t) = targets.get(self.selected).cloned() {
                    let current = match &t {
                        Target::Agent { agent, .. } => agent.name.clone(),
                        Target::Session { name, .. } => name.clone(),
                    };
                    self.renaming = Some((t, current));
                }
            }
            BareKey::Backspace => {
                self.query.pop();
                self.select_first_agent();
            }
            BareKey::Char(c) if !ctrl && !alt => {
                self.query.push(c);
                self.select_first_agent();
                self.note = None;
            }
            _ => return false,
        }
        true
    }

    // ------------------------------------------------------------ drawing

    fn draw(&mut self, lines: Vec<Line>, first_row: usize, rows: usize, cols: usize, selected: Option<usize>) {
        // keep the selection in view
        if let Some(sel) = selected {
            let mut nth = 0;
            for (i, l) in lines.iter().enumerate() {
                if l.target.is_some() {
                    if nth == sel {
                        if i < self.scroll {
                            self.scroll = i.saturating_sub(1);
                        } else if i >= self.scroll + rows {
                            self.scroll = i + 1 - rows;
                        }
                    }
                    nth += 1;
                }
            }
        }
        self.scroll = self.scroll.min(lines.len().saturating_sub(rows));
        let mut nth = 0;
        for (i, line) in lines.into_iter().enumerate() {
            let is_target = line.target.is_some();
            let picked = is_target && selected == Some(nth);
            if is_target {
                nth += 1;
            }
            if i < self.scroll || i >= self.scroll + rows {
                continue;
            }
            let row = first_row + i - self.scroll;
            let bg = if picked { Some(PICK_BG) } else { line.bg };
            let right_w: usize = line.right.iter().map(|s| width(&s.text)).sum();
            let mut room = cols.saturating_sub(right_w + if right_w > 0 { 1 } else { 0 });
            let mut out = format!("\u{1b}[{};1H\u{1b}[0m", row + 1);
            if let Some(b) = bg {
                out.push_str(&format!("\u{1b}[48;5;{b}m"));
            }
            let mut used = 0;
            let paint = |s: &Seg, text: &str| {
                let fg = if picked { PICK_FG } else { s.fg };
                format!("\u{1b}[{};38;5;{fg}m{text}", if s.bold { 1 } else { 22 })
            };
            for s in &line.left {
                let text = clip(&s.text, room);
                room -= width(&text);
                used += width(&text);
                out.push_str(&paint(s, &text));
            }
            out.push_str(&" ".repeat(cols.saturating_sub(used + right_w)));
            for s in &line.right {
                out.push_str(&paint(s, &s.text));
            }
            out.push_str("\u{1b}[0m");
            print!("{out}");
            if row < self.hits.len() {
                self.hits[row] = line.target;
            }
        }
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.mode = if configuration.get("mode").map(String::as_str) == Some("picker") { Mode::Picker } else { Mode::Sidebar };
        self.zaag = configuration.get("zaag").cloned().unwrap_or_else(|| "zaag".to_string());
        self.visible = true;
        ask_permission();
        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::ModeUpdate,
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::Timer,
            EventType::Visible,
            EventType::RunCommandResult,
            EventType::Key,
            EventType::Mouse,
        ]);
        // A session created with nobody attached never answers the permission request: ask again
        // until it does.
        set_timeout(1.0);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(status) => {
                self.granted = status == PermissionStatus::Granted;
                if self.granted {
                    if self.mode == Mode::Sidebar {
                        set_selectable(false);
                    } else {
                        rename_plugin_pane(get_plugin_ids().plugin_id, "agents");
                    }
                    self.poll();
                    self.arm();
                }
                true
            }
            Event::ModeUpdate(info) => {
                let changed = self.session != info.session_name;
                self.session = info.session_name;
                changed
            }
            Event::TabUpdate(tabs) => {
                if let Some(t) = tabs.iter().find(|t| t.active) {
                    self.active_tab = t.position;
                }
                let before = self.focused;
                self.refocus();
                self.mark_seen();
                before != self.focused
            }
            Event::PaneUpdate(manifest) => {
                self.manifest = manifest;
                let before = self.focused;
                self.refocus();
                self.mark_seen();
                before != self.focused
            }
            Event::Visible(v) => {
                self.visible = v;
                if v {
                    // answers and timers that came due while hidden may never have arrived
                    self.pending = 0;
                    self.timer_armed = false;
                    self.poll();
                    self.arm();
                }
                v
            }
            Event::Timer(_) if !self.granted => {
                ask_permission();
                set_timeout(1.0);
                false
            }
            Event::Timer(_) => {
                // two timers can be running after the pane was hidden and shown: keep one
                if self.last_timer.is_some_and(|t| t.elapsed().as_secs_f64() < POLL_SECS * 0.6) {
                    return false;
                }
                self.last_timer = Some(std::time::Instant::now());
                self.timer_armed = false;
                self.poll();
                self.arm();
                self.machines.iter().any(|m| m.agents.iter().any(|a| a.status == "working"))
            }
            Event::RunCommandResult(_, stdout, _, ctx) => match ctx.get("kind").map(String::as_str) {
                Some("list") => {
                    self.pending = 0;
                    if let Ok(Answer { result: Some(listing) }) = serde_json::from_slice::<Answer>(&stdout) {
                        self.machines = listing.machines;
                        self.loaded = true;
                        // the cursor stays on the project that was just moved: only a listing asked
                        // after the move shows its new place
                        let fresh = ctx.get("seq").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0) >= self.follow_from;
                        if fresh {
                            if let Some(name) = self.follow.take() {
                                if let Some(i) = self.picker_targets().iter().position(|t| matches!(t, Target::Session { name: n, .. } if *n == name)) {
                                    self.selected = i;
                                }
                            }
                        }
                        self.mark_seen();
                    }
                    true
                }
                _ => {
                    self.pending = 0;
                    self.follow_from = self.list_seq + 1;
                    self.poll();
                    false
                }
            },
            Event::Key(key) if self.mode == Mode::Picker => self.picker_key(key),
            Event::Mouse(Mouse::LeftClick(line, col)) => {
                if self.mode == Mode::Sidebar && self.buttons_row == Some(line.max(0) as usize) {
                    if let Some(session) = self.session.clone() {
                        if col < self.cols / 2 {
                            self.zaag(&["new", "--session", &session, "--focus"], "act");
                        } else {
                            self.zaag(&["menu", "--session", &session], "act");
                        }
                    }
                    return false;
                }
                if let Some(Some(t)) = self.hits.get(line.max(0) as usize).cloned() {
                    self.jump(&t);
                    if self.mode == Mode::Picker && self.note.is_none() {
                        self.close_picker();
                    }
                    return true;
                }
                false
            }
            Event::Mouse(Mouse::ScrollDown(n)) => {
                self.scroll += n;
                true
            }
            Event::Mouse(Mouse::ScrollUp(n)) => {
                self.scroll = self.scroll.saturating_sub(n);
                true
            }
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.hits = vec![None; rows];
        if !self.granted {
            return; // zellij shows its permission question here
        }
        self.cols = cols;
        self.buttons_row = None;
        if self.mode == Mode::Sidebar {
            // projects on top, then "new / menu", then the agents in the bottom half with their own scroll
            let split = (rows / 2).max(4).min(rows);
            let saved = self.scroll;
            self.scroll = 0;
            let projects = self.project_lines();
            self.draw(projects, 0, split.saturating_sub(2), cols, None);
            self.draw(vec![Line { left: vec![seg(" new", DIM)], right: vec![seg("menu ", DIM)], ..Default::default() }], split.saturating_sub(2), 1, cols, None);
            self.buttons_row = Some(split.saturating_sub(2));
            self.draw(vec![Line { left: vec![seg("─".repeat(cols), RULE)], ..Default::default() }], split - 1, 1, cols, None);
            if split < rows {
                self.draw(vec![Line { left: vec![bold(" agents", DIM)], right: vec![bold("priority ", DIM)], ..Default::default() }], split, 1, cols, None);
            }
            self.scroll = saved;
            let agents = self.agent_lines();
            let first = split + 2;
            let room = rows.saturating_sub(first);
            let total = agents.len();
            self.draw(agents, first, room, cols, None);
            if total > room && room > 0 {
                let thumb = (room * room / total).max(1);
                let at = self.scroll * room / total;
                for r in at..(at + thumb).min(room) {
                    print!("\u{1b}[{};{}H\u{1b}[0;38;5;{RULE}m│\u{1b}[0m", first + r + 1, cols);
                }
            }
            return;
        }
        let count: usize = self.machines.iter().map(|m| m.agents.len()).sum();
        let head = match &self.renaming {
            Some((_, text)) => vec![seg(" rename: ", OLIVE), seg(format!("{text}▏"), TEXT)],
            None if self.query.is_empty() => vec![seg(" / ", DIM), seg("search agents and projects", DIM)],
            None => vec![seg(" / ", DIM), seg(format!("{}▏", self.query), TEXT)],
        };
        let saved = self.scroll;
        self.scroll = 0;
        self.draw(vec![Line { left: head, right: vec![seg(format!("{count} agents "), DIM)], ..Default::default() }, Line { left: vec![seg("─".repeat(cols), 238)], ..Default::default() }], 0, 2, cols, None);
        self.scroll = saved;
        let lines = self.picker_lines();
        let targets = lines.iter().filter(|l| l.target.is_some()).count();
        self.selected = self.selected.min(targets.saturating_sub(1));
        let body = rows.saturating_sub(3);
        self.draw(lines, 2, body, cols, Some(self.selected));
        let hint = match &self.note {
            Some(n) => seg(format!(" {n}"), OLIVE),
            None => seg(" enter: go   ctrl-r: rename   ctrl-u/d: move project   esc: close", DIM),
        };
        let saved = self.scroll;
        self.scroll = 0;
        self.draw(vec![Line { left: vec![hint], ..Default::default() }], rows.saturating_sub(1), 1, cols, None);
        self.scroll = saved;
    }
}
