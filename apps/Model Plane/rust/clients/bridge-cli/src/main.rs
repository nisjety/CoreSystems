//! bridge-cli — operator shell for bridge-core (matrix P6, "CLI/TUI shell").
//!
//! Two modes over the same [`client::BridgeClient`] (`/api/v1/sessions`):
//!   * default — a line REPL (new/list/get/send/close).
//!   * `bridge-cli tui` — a richer ratatui terminal UI (session list + output +
//!     input panels), layered on the same client (no duplication).
//!
//! bridge-core owns sessions/channels/voice; this is a thin client — no
//! backend. Config (env): `BRIDGE_CORE_URL` (default <http://localhost:8091>),
//! `MP_ORG_ID` (default "default"), `MP_USER_ID` (default "operator").
//!
//! The command parser, URL/base64 building, and TUI state model are
//! unit-tested; the REPL/TUI loops and live HTTP need a running bridge-core.

mod client;
mod command;
mod tui;

use std::io::{self, BufRead, Write};

use anyhow::Result;

use client::BridgeClient;
use command::{parse, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let base =
        std::env::var("BRIDGE_CORE_URL").unwrap_or_else(|_| "http://localhost:8091".to_owned());
    let org = std::env::var("MP_ORG_ID").unwrap_or_else(|_| "default".to_owned());
    let user = std::env::var("MP_USER_ID").unwrap_or_else(|_| "operator".to_owned());
    let client = BridgeClient::new(&base, &org, &user);

    if std::env::args().nth(1).as_deref() == Some("tui") {
        return tui::run(client).await;
    }
    repl(client).await
}

async fn repl(client: BridgeClient) -> Result<()> {
    println!(
        "bridge-cli (org={}).  Type 'help', or run `bridge-cli tui` for the terminal UI.",
        client.org_id()
    );
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("> ");
        io::stdout().flush().ok();
        let Some(line) = lines.next() else { break };
        match parse(&line?) {
            Command::Quit => break,
            Command::Empty => {}
            Command::Help => print_help(),
            cmd => match dispatch(&client, cmd).await {
                Ok(out) => println!("{out}"),
                Err(e) => eprintln!("error: {e:#}"),
            },
        }
    }
    Ok(())
}

async fn dispatch(client: &BridgeClient, cmd: Command) -> Result<String> {
    match cmd {
        Command::NewSession { channel } => client.new_session(&channel).await,
        Command::ListSessions => client.list_sessions().await,
        Command::GetSession { id } => client.get_session(&id).await,
        Command::Send { id, text } => client.ingest(&id, &text).await,
        Command::Close { id } => client.close_session(&id).await,
        Command::Unknown(v) => Ok(format!("unknown command '{v}' — type 'help'")),
        Command::Help | Command::Quit | Command::Empty => Ok(String::new()),
    }
}

fn print_help() {
    println!(
        "commands:\n  \
         new [channel]      register a session (channel defaults to 'cli')\n  \
         list | ls          list sessions\n  \
         get <id>           show a session\n  \
         send <id> <text>   send input to a session\n  \
         close <id>         close a session\n  \
         help | quit\n\
         (run `bridge-cli tui` for the terminal UI)"
    );
}
