//! `vsv2-mcp login`, `logout` and `whoami`: signing in to Visma as a person, so the server acts as that
//! person instead of with the integration's service credentials. See the `vsv2-signin` crate for how.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Context};
use async_trait::async_trait;
use tokio::sync::mpsc;
use vsv2_common::{load_tenants, TenantStatus};
use vsv2_signin::browser::SystemBrowser;
use vsv2_signin::{
    login, logout, snapshot, Console, KnownCompany, LoginOptions, SignInConfig, SignInError, Store,
    UserTokens,
};

use crate::config::McpConfig;

pub const COMMANDS: [&str; 3] = ["login", "logout", "whoami"];

pub const USAGE: &str = "usage:\n  \
    vsv2-mcp login [--write] [--once] [--configured-only] [--paste] [--timeout-secs N]\n  \
    vsv2-mcp logout [--company TENANT_ID]\n  \
    vsv2-mcp whoami [--check]\n\n\
    login      Sign in to Visma as yourself. Visma issues a token for ONE company per approval, so the consent screen\n             \
    comes back after each company, in the same browser tab, until you have connected them all: press Enter here\n             \
    when you are done. --write also asks for the write scopes (VISMA_USER_SCOPES_WRITE); --once connects one\n             \
    company only; --configured-only stops by itself once every company in config/tenants is connected; --paste\n             \
    does not listen for the redirect and asks you to paste the address the browser ends up on.\n  \
    logout     Revoke the sign-in at Visma and forget it here (one company with --company).\n  \
    whoami     Show who is signed in and which companies are connected; --check also asks Visma whether each\n             \
    sign-in is still accepted.\n\n\
    Needs the web application registered in Visma's Developer Portal: VISMA_USER_CLIENT_ID, VISMA_USER_CLIENT_SECRET\n\
    and VISMA_USER_REDIRECT_URI (default http://localhost:38271/callback, registered exactly like that).";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Login {
        write: bool,
        once: bool,
        configured_only: bool,
        paste: bool,
        timeout: Option<u64>,
    },
    Logout {
        company: Option<String>,
    },
    Whoami {
        check: bool,
    },
}

/// `command` is one of [`COMMANDS`]; `args` are the arguments after it. A mistyped flag is an error, not ignored.
pub fn parse(command: &str, args: &[String]) -> anyhow::Result<Command> {
    let mut args = args.iter();
    let (mut write, mut once, mut configured_only, mut paste, mut check) =
        (false, false, false, false, false);
    let (mut timeout, mut company) = (None, None);
    while let Some(arg) = args.next() {
        match (command, arg.as_str()) {
            ("login", "--write") => write = true,
            ("login", "--once") => once = true,
            ("login", "--configured-only") => configured_only = true,
            ("login", "--paste") => paste = true,
            ("login", "--timeout-secs") => {
                let value = args
                    .next()
                    .context("--timeout-secs needs a number of seconds")?;
                timeout = Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|s| (10..=3600).contains(s))
                        .context("--timeout-secs must be 10 to 3600")?,
                );
            }
            ("logout", "--company") => {
                company = Some(args.next().context("--company needs a company id")?.clone())
            }
            ("whoami", "--check") => check = true,
            (_, "-h" | "--help") => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            (_, other) => bail!("unknown argument {other:?} for `{command}`\n\n{USAGE}"),
        }
    }
    match command {
        "login" => Ok(Command::Login {
            write,
            once,
            configured_only,
            paste,
            timeout,
        }),
        "logout" => Ok(Command::Logout { company }),
        "whoami" => Ok(Command::Whoami { check }),
        other => bail!("unknown command {other:?}\n\n{USAGE}"),
    }
}

pub async fn run(command: &str, args: Vec<String>) -> anyhow::Result<()> {
    let command = parse(command, &args)?;
    // Loads `.env` too, so the sign-in application can be configured there like everything else.
    let mcp = McpConfig::from_env().context("invalid configuration")?;
    match command {
        Command::Login {
            write,
            once,
            configured_only,
            paste,
            timeout,
        } => run_login(&mcp, write, once, configured_only, paste, timeout).await,
        Command::Logout { company } => run_logout(company.as_deref()).await,
        Command::Whoami { check } => run_whoami(&mcp, check).await,
    }
}

/// The companies in `config/tenants` that produce data today, as the ones to expect a consent screen for.
fn configured_companies(mcp: &McpConfig) -> Vec<KnownCompany> {
    match load_tenants(&mcp.tenant_dir) {
        Ok(list) => list
            .into_iter()
            .filter(|t| t.status == TenantStatus::Active)
            .map(|t| KnownCompany {
                id: t.tenant_id,
                name: t.tenant_name,
            })
            .collect(),
        Err(e) => {
            println!("(no company names from {}: {e})", mcp.tenant_dir.display());
            Vec::new()
        }
    }
}

fn load_signin() -> anyhow::Result<SignInConfig> {
    SignInConfig::from_env().map_err(|e| anyhow::anyhow!("{e}\n\n{USAGE}"))
}

async fn run_login(
    mcp: &McpConfig,
    write: bool,
    once: bool,
    configured_only: bool,
    paste: bool,
    timeout: Option<u64>,
) -> anyhow::Result<()> {
    let cfg = load_signin()?;
    let store = Store::new(cfg.store_dir.clone());
    let known = configured_companies(mcp);
    print_state(&store, &known)?;
    let mut options = LoginOptions::new(known);
    options.write = write;
    options.once = once;
    options.configured_only = configured_only;
    options.paste = paste;
    if let Some(seconds) = timeout {
        options.round_timeout = Duration::from_secs(seconds);
    }
    println!(
        "Application {} with redirect URL {}: that exact URL must be registered for the application in the Visma Developer Portal.",
        cfg.client_id, cfg.redirect_uri
    );
    println!(
        "Visma asks for one company at a time. Approve each company you want this server to see."
    );
    let terminal = Terminal::new();
    match login(&cfg, &store, &options, &SystemBrowser, &terminal).await {
        Ok(report) => {
            println!("\nDone: {}.", report.finished.summary());
            println!(
                "Signed in as {}; connected or renewed in this run: {}.",
                report.session.display(),
                report.connected.len()
            );
            let total = store.connections().map(|c| c.len()).unwrap_or(0);
            println!(
                "{total} companies are connected in total. Check with `vsv2-mcp whoami --check`."
            );
            println!(
                "A running MCP server does not notice a new sign-in: start a new session. It then acts as you (VSV2_MCP_IDENTITY defaults to `auto`); undo with `vsv2-mcp logout`."
            );
            if !write {
                println!("Changing data in Visma also needs `vsv2-mcp login --write` and VSV2_MCP_ENABLE_WRITES=1.");
            }
            Ok(())
        }
        Err(e) => {
            if let Some(hint) = hint(&e) {
                eprintln!("{hint}");
            }
            Err(e.into())
        }
    }
}

/// What to try, for the errors where the cause is usually a setting rather than a mistake.
fn hint(error: &SignInError) -> Option<&'static str> {
    match error {
        SignInError::Denied { error, .. } if error == "invalid_scope" => Some(
            "Visma refused one of the scopes in VISMA_USER_SCOPES. The application in the Developer Portal must be a web application that may use them, including offline_access.",
        ),
        SignInError::Denied { error, .. } if error == "unauthorized_client" || error == "invalid_request" => Some(
            "Visma did not accept the request. Check VISMA_USER_CLIENT_ID and that VISMA_USER_REDIRECT_URI is registered for the application exactly as written.",
        ),
        SignInError::Token { error, .. } if error == "invalid_client" => {
            Some("Visma did not accept the client id or secret: check VISMA_USER_CLIENT_ID and VISMA_USER_CLIENT_SECRET.")
        }
        SignInError::Token { error, .. } if error == "invalid_grant" => Some(
            "The redirect URI sent must match the one registered. Check VISMA_USER_REDIRECT_URI, or use --paste if the registered one is not on this computer.",
        ),
        // Visma shows an error page of its own, without redirecting back, when it rejects the client or the redirect
        // URL; from here that looks like nobody answering.
        SignInError::Timeout(_) | SignInError::Cancelled => Some(
            "If the browser showed an error from Visma instead of a consent screen, the usual causes are a wrong VISMA_USER_CLIENT_ID, or a redirect URL that is not registered for the application exactly as VISMA_USER_REDIRECT_URI. Check both and run login again.",
        ),
        _ => None,
    }
}

fn print_state(store: &Store, known: &[KnownCompany]) -> anyhow::Result<()> {
    match snapshot(store)? {
        None => println!("Not signed in yet."),
        Some(state) => {
            println!(
                "Signed in as {}; {} companies connected so far.",
                state.session.display(),
                state.companies.len()
            );
            let names = names_of(
                known,
                &state
                    .companies
                    .iter()
                    .map(|c| (c.tenant_id.clone(), c.name.clone()))
                    .collect::<Vec<_>>(),
            );
            for company in &state.companies {
                println!("  {}", label(&names, &company.tenant_id));
            }
        }
    }
    Ok(())
}

/// Configured names, then the names Visma gave, by company id.
fn names_of(
    known: &[KnownCompany],
    connected: &[(String, Option<String>)],
) -> HashMap<String, String> {
    let mut names: HashMap<String, String> = connected
        .iter()
        .filter_map(|(id, name)| Some((id.clone(), name.clone()?)))
        .collect();
    names.extend(known.iter().map(|k| (k.id.clone(), k.name.clone())));
    names
}

fn label(names: &HashMap<String, String>, id: &str) -> String {
    match names.get(id) {
        Some(name) => format!("{name} ({id})"),
        None => id.to_string(),
    }
}

async fn run_logout(company: Option<&str>) -> anyhow::Result<()> {
    let store = Store::new(vsv2_signin::store_dir_from_env()?);
    // Revoking needs the application's credentials; without them the tokens are only forgotten here.
    let exchanger = SignInConfig::from_env()
        .ok()
        .map(|cfg| vsv2_signin::exchange::Exchanger::new(&cfg));
    if exchanger.is_none() {
        println!("(the application is not configured, so the tokens are forgotten here but not revoked at Visma)");
    }
    let report = logout(&store, exchanger.as_ref(), company).await?;
    println!(
        "Forgot {} companies ({} revoked at Visma, {} could not be). {}",
        report.removed,
        report.revoked,
        report.revoke_failed,
        if report.signed_out {
            "Nobody is signed in now; the server uses the service credentials again."
        } else {
            "Others stay connected."
        }
    );
    Ok(())
}

async fn run_whoami(mcp: &McpConfig, check: bool) -> anyhow::Result<()> {
    let store = Store::new(vsv2_signin::store_dir_from_env()?);
    let Some(state) = snapshot(&store)? else {
        println!("Nobody is signed in. The server uses the service credentials. Sign in with `vsv2-mcp login`.");
        return Ok(());
    };
    let known = configured_companies(mcp);
    let connected: Vec<(String, Option<String>)> = state
        .companies
        .iter()
        .map(|c| (c.tenant_id.clone(), c.name.clone()))
        .collect();
    let names = names_of(&known, &connected);
    println!(
        "Signed in as {} (user id {}), since {}.",
        state.session.display(),
        state.session.sub,
        state.session.signed_in_at.format("%Y-%m-%d %H:%M UTC")
    );
    println!("Kept in {}.", store.dir().display());
    let tokens = if check {
        Some(UserTokens::open(&load_signin()?)?)
    } else {
        None
    };
    println!("\nConnected companies ({}):", state.companies.len());
    for company in &state.companies {
        let minutes = (company.access_expires_at - chrono::Utc::now()).num_minutes();
        let access = if minutes > 0 {
            format!("access token valid {minutes} min")
        } else {
            "access token expired (renewed on next use)".to_string()
        };
        let verdict = match &tokens {
            Some(tokens) => match tokens.verify(&company.tenant_id).await {
                Ok(()) => " | Visma accepts it",
                Err(e) => {
                    println!("  {} | REJECTED: {e}", label(&names, &company.tenant_id));
                    continue;
                }
            },
            None => "",
        };
        println!(
            "  {} | {access} | scopes: {}{verdict}",
            label(&names, &company.tenant_id),
            company.scopes.join(" ")
        );
    }
    let missing: Vec<&str> = known
        .iter()
        .filter(|k| !connected.iter().any(|(id, _)| *id == k.id))
        .map(|k| k.name.as_str())
        .collect();
    if !missing.is_empty() {
        println!(
            "\nConfigured companies not connected: {}. `vsv2-mcp login` adds them.",
            missing.join(", ")
        );
    }
    Ok(())
}

/// The terminal. Lines typed are collected by one thread for the whole run, so a pending "press Enter to
/// stop" never swallows a line meant for a later question.
struct Terminal {
    lines: tokio::sync::Mutex<mpsc::UnboundedReceiver<String>>,
}

impl Terminal {
    fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                match stdin.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if sender.send(line.trim().to_string()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Self {
            lines: tokio::sync::Mutex::new(receiver),
        }
    }
}

#[async_trait]
impl Console for Terminal {
    fn say(&self, line: &str) {
        println!("{line}");
    }

    async fn ask(&self, prompt: &str) -> Option<String> {
        println!("{prompt}");
        self.lines.lock().await.recv().await
    }

    async fn wait_for_enter(&self) {
        // End of input (no terminal to press Enter in) means "never", not "stop".
        if self.lines.lock().await.recv().await.is_none() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn login_flags_are_parsed_and_mistyped_ones_are_refused() {
        assert_eq!(
            parse("login", &args(&[])).unwrap(),
            Command::Login {
                write: false,
                once: false,
                configured_only: false,
                paste: false,
                timeout: None
            }
        );
        assert_eq!(
            parse(
                "login",
                &args(&[
                    "--write",
                    "--once",
                    "--configured-only",
                    "--paste",
                    "--timeout-secs",
                    "90"
                ])
            )
            .unwrap(),
            Command::Login {
                write: true,
                once: true,
                configured_only: true,
                paste: true,
                timeout: Some(90)
            }
        );
        for bad in [
            &["--wirte"][..],
            &["--timeout-secs"],
            &["--timeout-secs", "5"],
            &["--timeout-secs", "x"],
            &["--check"],
        ] {
            assert!(parse("login", &args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn logout_and_whoami_take_only_their_own_flags() {
        assert_eq!(
            parse("logout", &args(&["--company", "t-1"])).unwrap(),
            Command::Logout {
                company: Some("t-1".into())
            }
        );
        assert_eq!(
            parse("logout", &args(&[])).unwrap(),
            Command::Logout { company: None }
        );
        assert_eq!(
            parse("whoami", &args(&["--check"])).unwrap(),
            Command::Whoami { check: true }
        );
        assert!(parse("logout", &args(&["--company"])).is_err());
        assert!(parse("whoami", &args(&["--write"])).is_err());
        assert!(parse("login", &args(&["--company", "t-1"])).is_err());
    }

    #[test]
    fn names_prefer_the_configured_name_over_the_one_visma_gave() {
        let known = vec![KnownCompany {
            id: "t-1".into(),
            name: "Aquatiq AS".into(),
        }];
        let names = names_of(
            &known,
            &[
                ("t-1".into(), Some("AQUATIQ AS (Visma)".into())),
                ("t-2".into(), Some("Aqua Pharma Inc".into())),
                ("t-3".into(), None),
            ],
        );
        assert_eq!(label(&names, "t-1"), "Aquatiq AS (t-1)");
        assert_eq!(label(&names, "t-2"), "Aqua Pharma Inc (t-2)");
        assert_eq!(label(&names, "t-3"), "t-3");
    }

    #[test]
    fn the_usual_setting_mistakes_have_a_hint_and_other_errors_do_not() {
        let denied = |error: &str| SignInError::Denied {
            error: error.into(),
            description: String::new(),
        };
        assert!(hint(&denied("invalid_scope"))
            .unwrap()
            .contains("VISMA_USER_SCOPES"));
        assert!(hint(&denied("unauthorized_client"))
            .unwrap()
            .contains("registered"));
        assert!(hint(&SignInError::Token {
            status: 401,
            error: "invalid_client".into(),
            description: String::new()
        })
        .unwrap()
        .contains("CLIENT_SECRET"));
        assert!(hint(&SignInError::Token {
            status: 400,
            error: "invalid_grant".into(),
            description: String::new()
        })
        .unwrap()
        .contains("--paste"));
        assert!(hint(&denied("access_denied")).is_none());
        for silent in [SignInError::Timeout(60), SignInError::Cancelled] {
            assert!(
                hint(&silent)
                    .unwrap()
                    .contains("redirect URL that is not registered"),
                "{silent}"
            );
        }
        assert!(hint(&SignInError::NoRefreshToken).is_none());
    }
}
