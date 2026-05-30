//! bridge-tui — a richer ratatui terminal UI for bridge-core, layered on the
//! same [`crate::client::BridgeClient`] the REPL uses (no duplication). Panels:
//! a selectable session list, an output log, and an input box; keys drive the
//! same `/api/v1/sessions` operations (new/list/get/send/close).
//!
//! The state model ([`App`] selection, input editing, log capping) is pure and
//! unit-tested; the terminal init/draw/event loop and the live HTTP calls are
//! glue that needs a TTY + a running bridge-core to exercise.

use std::time::Duration;

use anyhow::Result;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout},
    style::{Style, Stylize},
    widgets::{Block, List, ListItem, Paragraph},
    DefaultTerminal, Frame,
};

use crate::client::{BridgeClient, Session};

/// Max output lines retained (cheap memory bound; the panel scrolls to bottom).
const MAX_OUTPUT_LINES: usize = 500;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Input,
}

struct App {
    client: BridgeClient,
    sessions: Vec<Session>,
    selected: usize,
    output: Vec<String>,
    input: String,
    mode: Mode,
    should_quit: bool,
}

impl App {
    fn new(client: BridgeClient) -> Self {
        Self {
            client,
            sessions: Vec::new(),
            selected: 0,
            output: vec!["press 'r' to refresh, '?' for help".to_owned()],
            input: String::new(),
            mode: Mode::Normal,
            should_quit: false,
        }
    }

    // --- pure state (unit-tested) ------------------------------------------

    fn select_next(&mut self) {
        if !self.sessions.is_empty() && self.selected + 1 < self.sessions.len() {
            self.selected += 1;
        }
    }

    fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn selected_id(&self) -> Option<String> {
        self.sessions.get(self.selected).map(|s| s.id.clone())
    }

    /// Append a log line, clamping the selection and bounding the buffer.
    fn log(&mut self, msg: impl Into<String>) {
        self.output.push(msg.into());
        if self.output.len() > MAX_OUTPUT_LINES {
            let drop = self.output.len() - MAX_OUTPUT_LINES;
            self.output.drain(0..drop);
        }
    }

    /// Keep `selected` in range after the session list changes.
    fn clamp_selection(&mut self) {
        if self.selected >= self.sessions.len() {
            self.selected = self.sessions.len().saturating_sub(1);
        }
    }

    // --- async actions (glue over the client) ------------------------------

    async fn refresh(&mut self) {
        match self.client.list_sessions_typed().await {
            Ok(s) => {
                self.sessions = s;
                self.clamp_selection();
                self.log(format!("refreshed: {} session(s)", self.sessions.len()));
            }
            Err(e) => self.log(format!("refresh failed: {e}")),
        }
    }

    async fn new_session(&mut self) {
        match self.client.new_session("cli").await {
            Ok(out) => self.log(format!("new session: {out}")),
            Err(e) => self.log(format!("new session failed: {e}")),
        }
        self.refresh().await;
    }

    async fn get_selected(&mut self) {
        let Some(id) = self.selected_id() else {
            self.log("no session selected");
            return;
        };
        match self.client.get_session(&id).await {
            Ok(out) => self.log(format!("{id}: {out}")),
            Err(e) => self.log(format!("get {id} failed: {e}")),
        }
    }

    async fn close_selected(&mut self) {
        let Some(id) = self.selected_id() else {
            self.log("no session selected");
            return;
        };
        match self.client.close_session(&id).await {
            Ok(_) => self.log(format!("closed {id}")),
            Err(e) => self.log(format!("close {id} failed: {e}")),
        }
        self.refresh().await;
    }

    async fn send_input(&mut self) {
        let text = std::mem::take(&mut self.input);
        if text.is_empty() {
            return;
        }
        let Some(id) = self.selected_id() else {
            self.log("no session selected — cannot send");
            return;
        };
        match self.client.ingest(&id, &text).await {
            Ok(_) => self.log(format!("→ {id}: {text}")),
            Err(e) => self.log(format!("send to {id} failed: {e}")),
        }
    }
}

/// Run the TUI: take over the terminal, load sessions, run the event loop, and
/// always restore the terminal on exit.
pub async fn run(client: BridgeClient) -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new(client);
    app.refresh().await;
    let result = event_loop(&mut terminal, &mut app).await;
    ratatui::restore();
    result
}

async fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        // Short poll keeps the loop responsive without busy-spinning.
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code).await;
                }
            }
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

async fn handle_key(app: &mut App, code: KeyCode) {
    match app.mode {
        Mode::Normal => match code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => app.select_next(),
            KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
            KeyCode::Char('r') => app.refresh().await,
            KeyCode::Char('n') => app.new_session().await,
            KeyCode::Char('g') => app.get_selected().await,
            KeyCode::Char('x' | 'd') => app.close_selected().await,
            KeyCode::Char('i') => app.mode = Mode::Input,
            KeyCode::Char('?') => {
                app.log("keys: j/k move · n new · i input · g get · x close · r refresh · q quit");
            }
            _ => {}
        },
        Mode::Input => match code {
            KeyCode::Enter => {
                app.send_input().await;
                app.mode = Mode::Normal;
            }
            KeyCode::Esc => {
                app.input.clear();
                app.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Char(c) => app.input.push(c),
            _ => {}
        },
    }
}

fn ui(f: &mut Frame, app: &App) {
    let rows = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Min(3),    // body
        Constraint::Length(3), // input
        Constraint::Length(1), // help
    ])
    .split(f.area());

    f.render_widget(
        Paragraph::new(format!(
            "bridge-tui  org={}  sessions={}",
            app.client.org_id(),
            app.sessions.len()
        ))
        .style(Style::new().bold()),
        rows[0],
    );

    let body =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(rows[1]);

    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let marker = if i == app.selected { "▶ " } else { "  " };
            let line = format!("{marker}{} [{}] {}", s.id, s.channel, s.status);
            let style = if i == app.selected {
                Style::new().reversed()
            } else {
                Style::new()
            };
            ListItem::new(line).style(style)
        })
        .collect();
    f.render_widget(
        List::new(items).block(Block::bordered().title("Sessions")),
        body[0],
    );

    // Output panel scrolled to the bottom (newest visible).
    let inner_h = body[1].height.saturating_sub(2) as usize;
    let scroll = u16::try_from(app.output.len().saturating_sub(inner_h)).unwrap_or(0);
    f.render_widget(
        Paragraph::new(app.output.join("\n"))
            .block(Block::bordered().title("Output"))
            .scroll((scroll, 0)),
        body[1],
    );

    let (title, content) = match app.mode {
        Mode::Input => ("Input — Enter to send, Esc to cancel", app.input.as_str()),
        Mode::Normal => ("Input — press 'i' to edit", app.input.as_str()),
    };
    f.render_widget(
        Paragraph::new(content).block(Block::bordered().title(title)),
        rows[2],
    );

    f.render_widget(
        Paragraph::new("j/k move · n new · i input · g get · x close · r refresh · q quit")
            .style(Style::new().dim()),
        rows[3],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(n: usize) -> App {
        let mut app = App::new(BridgeClient::new("http://localhost:0", "o", "u"));
        app.sessions = (0..n)
            .map(|i| Session {
                id: format!("s-{i}"),
                channel: "cli".to_owned(),
                status: "active".to_owned(),
            })
            .collect();
        app
    }

    #[test]
    fn selection_clamps_at_both_ends() {
        let mut app = app_with(3);
        assert_eq!(app.selected, 0);
        app.select_prev(); // already at top
        assert_eq!(app.selected, 0);
        app.select_next();
        app.select_next();
        assert_eq!(app.selected, 2);
        app.select_next(); // already at bottom
        assert_eq!(app.selected, 2);
        assert_eq!(app.selected_id().as_deref(), Some("s-2"));
    }

    #[test]
    fn selection_handles_empty_list() {
        let mut app = app_with(0);
        app.select_next();
        app.select_prev();
        assert_eq!(app.selected, 0);
        assert!(app.selected_id().is_none());
    }

    #[test]
    fn clamp_after_shrink() {
        let mut app = app_with(3);
        app.select_next();
        app.select_next(); // selected = 2
        app.sessions.truncate(1); // list shrank to 1
        app.clamp_selection();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn log_is_bounded() {
        let mut app = app_with(0);
        for i in 0..(MAX_OUTPUT_LINES + 50) {
            app.log(format!("line {i}"));
        }
        assert!(app.output.len() <= MAX_OUTPUT_LINES);
        // Newest line retained.
        assert!(app
            .output
            .last()
            .unwrap()
            .contains(&format!("line {}", MAX_OUTPUT_LINES + 49)));
    }
}
