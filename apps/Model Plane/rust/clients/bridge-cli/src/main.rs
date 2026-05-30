//! bridge-cli — minimal operator shell for bridge-core (matrix P6, "CLI/TUI
//! shell"). A line REPL over bridge-core's `/api/v1/sessions` API
//! (new/list/get/send/close). bridge-core owns sessions/channels/voice; this is
//! a thin client — no backend, no duplication. A richer ratatui TUI can layer
//! on this `client` module later without changing the server contract.
//!
//! Config (env): `BRIDGE_CORE_URL` (default http://localhost:8091),
//! `MP_ORG_ID` (default "default"), `MP_USER_ID` (default "operator").
//!
//! The command parsing + URL/base64 building are unit-tested; the REPL loop and
//! the live HTTP calls need a running bridge-core to exercise end-to-end.

mod client;
mod command;

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

    println!("bridge-cli -> {base}  (org={org}, user={user}).  Type 'help'.");
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("> ");
        io::stdout().flush().ok();
        let Some(line) = lines.next() else { break };
        let cmd = parse(&line?);
        match cmd {
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
        // Handled before dispatch; return empty for exhaustiveness without panic.
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
         help | quit"
    );
}
