//! Operator command parsing for the bridge CLI. Pure + total — unit-tested.

/// A parsed operator command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Register a new session on the given channel (defaults to "cli").
    NewSession { channel: String },
    /// List the org's sessions.
    ListSessions,
    /// Show one session by id.
    GetSession { id: String },
    /// Send input text to a session.
    Send { id: String, text: String },
    /// Close a session.
    Close { id: String },
    /// Print help.
    Help,
    /// Exit the REPL.
    Quit,
    /// Blank line — no-op.
    Empty,
    /// Unrecognized verb.
    Unknown(String),
}

/// Parse one input line into a [`Command`]. Total: never panics, always returns.
pub fn parse(line: &str) -> Command {
    let line = line.trim();
    if line.is_empty() {
        return Command::Empty;
    }
    let mut head = line.splitn(2, char::is_whitespace);
    let verb = head.next().unwrap_or("");
    let rest = head.next().unwrap_or("").trim();
    match verb {
        "new" => Command::NewSession {
            channel: if rest.is_empty() {
                "cli".to_owned()
            } else {
                rest.to_owned()
            },
        },
        "list" | "ls" => Command::ListSessions,
        "get" => Command::GetSession {
            id: rest.to_owned(),
        },
        "send" => {
            let mut p = rest.splitn(2, char::is_whitespace);
            let id = p.next().unwrap_or("").to_owned();
            let text = p.next().unwrap_or("").trim().to_owned();
            Command::Send { id, text }
        }
        "close" => Command::Close {
            id: rest.to_owned(),
        },
        "help" | "?" => Command::Help,
        "quit" | "exit" | "q" => Command::Quit,
        other => Command::Unknown(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_verbs_and_args() {
        assert_eq!(parse(""), Command::Empty);
        assert_eq!(parse("   "), Command::Empty);
        assert_eq!(
            parse("new slack"),
            Command::NewSession {
                channel: "slack".to_owned()
            }
        );
        // bare `new` defaults the channel.
        assert_eq!(
            parse("new"),
            Command::NewSession {
                channel: "cli".to_owned()
            }
        );
        assert_eq!(parse("list"), Command::ListSessions);
        assert_eq!(parse("ls"), Command::ListSessions);
        assert_eq!(
            parse("get s-1"),
            Command::GetSession {
                id: "s-1".to_owned()
            }
        );
        assert_eq!(
            parse("close s-1"),
            Command::Close {
                id: "s-1".to_owned()
            }
        );
        assert_eq!(parse("help"), Command::Help);
        assert_eq!(parse("quit"), Command::Quit);
        assert_eq!(parse("q"), Command::Quit);
        assert_eq!(
            parse("frobnicate"),
            Command::Unknown("frobnicate".to_owned())
        );
    }

    #[test]
    fn send_splits_id_then_rest_as_text() {
        assert_eq!(
            parse("send s-1 hello there, world"),
            Command::Send {
                id: "s-1".to_owned(),
                text: "hello there, world".to_owned()
            }
        );
        // missing text is allowed (empty) — the server validates.
        assert_eq!(
            parse("send s-1"),
            Command::Send {
                id: "s-1".to_owned(),
                text: String::new()
            }
        );
    }
}
