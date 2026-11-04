//! The narrow rule an "Always" on a shell approval may propose (build plan EP-9 E4).
//!
//! A person who approves `git commit -m "Fix ø"` and chooses "Always" must be offered a rule they
//! can read and that says less than the command did: `git commit`, never the message, a path or a
//! host. So the rule is never cut out of the command text. It is a program and, for the programs
//! that have them, a subcommand, both taken from the closed tables below and returned as the
//! table's own `&'static str`; the command only selects a table row.
//!
//! The tokenizer is deliberately smaller than a shell. It answers "no rule" for anything it does not
//! fully understand: pipes, `;`, `&` and `&&`, redirects, command and variable substitution, globs,
//! escapes, comments, line breaks, an unterminated quote, an environment assignment in front of the
//! program, a quoted or path-qualified program, an option before the subcommand and every program or
//! subcommand outside the tables. "No rule" means the approval is offered once, as before.

use std::fmt;

use serde_json::Value;

use crate::loop_fold::switch_from;

/// The switch for shell-rule proposals. Default off; parsed by [`switch_from`] (`true` and nothing
/// else). It only narrows what WS13-S7's proposals carry, so it is refused without that switch.
pub const SHELL_RULE_PROPOSALS_ENV: &str = "APPROVAL_SHELL_RULE_PROPOSALS_ENABLED";
/// WS13-S7's switch for "Always" proposals (model-gateway reads it for the decide route).
pub const RULE_PROPOSALS_ENV: &str = "APPROVAL_RULE_PROPOSALS_ENABLED";
/// The shell tool's name, the only tool a shell rule is proposed for.
pub const SHELL_TOOL: &str = "shell";
/// Longest command the tokenizer reads; a longer one has no rule.
const MAX_COMMAND_CHARS: usize = 2000;
/// Most words a command may have and still get a rule.
const MAX_TOKENS: usize = 64;
/// Punctuation an unquoted word may contain besides letters and digits. Every other character is
/// shell syntax the tokenizer does not model, so it ends in "no rule".
const PLAIN_PUNCTUATION: &str = "-_./:,=+@%";

/// Which words a program's rule names after the program.
enum Subcommands {
    /// The rule is the program alone; every word after it is an argument.
    None,
    /// The second word must be one of these, unquoted and exact; it becomes part of the rule.
    OneOf(&'static [&'static str]),
}

/// Programs that may be named by a rule. A program not listed has no rule, whatever follows it: an
/// interpreter (`sh`, `python`), a wrapper that runs another command (`env`, `xargs`, `sudo`,
/// `find -exec`) or a destructive tool (`rm`) is never proposed. Subcommands that run arbitrary
/// configured code by name (`git config`, `npm run`) are left out for the same reason.
const PROGRAMS: &[(&str, Subcommands)] = &[
    (
        "git",
        Subcommands::OneOf(&[
            "add",
            "blame",
            "branch",
            "commit",
            "diff",
            "fetch",
            "log",
            "ls-files",
            "pull",
            "restore",
            "rev-parse",
            "show",
            "status",
            "switch",
        ]),
    ),
    (
        "cargo",
        Subcommands::OneOf(&[
            "build", "check", "clippy", "doc", "fmt", "metadata", "test", "tree",
        ]),
    ),
    (
        "go",
        Subcommands::OneOf(&["build", "fmt", "list", "test", "vet", "version"]),
    ),
    ("npm", Subcommands::OneOf(&["ci", "ls", "outdated", "test"])),
    (
        "pnpm",
        Subcommands::OneOf(&["install", "list", "outdated", "test"]),
    ),
    (
        "pip",
        Subcommands::OneOf(&["check", "freeze", "list", "show"]),
    ),
    ("cat", Subcommands::None),
    ("date", Subcommands::None),
    ("df", Subcommands::None),
    ("diff", Subcommands::None),
    ("du", Subcommands::None),
    ("echo", Subcommands::None),
    ("file", Subcommands::None),
    ("grep", Subcommands::None),
    ("head", Subcommands::None),
    ("ls", Subcommands::None),
    ("pwd", Subcommands::None),
    ("rg", Subcommands::None),
    ("sort", Subcommands::None),
    ("stat", Subcommands::None),
    ("tail", Subcommands::None),
    ("tree", Subcommands::None),
    ("uname", Subcommands::None),
    ("uniq", Subcommands::None),
    ("wc", Subcommands::None),
    ("which", Subcommands::None),
    ("whoami", Subcommands::None),
];

/// A rule a shell approval may propose: a program and, where its table row has them, a subcommand.
/// Both are table words, so the rule holds no text of the command it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellRule {
    program: &'static str,
    subcommand: Option<&'static str>,
}

impl ShellRule {
    /// The program the rule names.
    #[must_use]
    pub const fn program(&self) -> &'static str {
        self.program
    }

    /// The subcommand the rule names, for a program that has them.
    #[must_use]
    pub const fn subcommand(&self) -> Option<&'static str> {
        self.subcommand
    }

    /// The rule as the person reads it on the proposal card, for example `git status`.
    #[must_use]
    pub fn text(&self) -> String {
        match self.subcommand {
            Some(subcommand) => format!("{} {subcommand}", self.program),
            None => self.program.to_owned(),
        }
    }

    /// The cause's `action_key` for a shell call the rule covers, for example `shell:git status`.
    #[must_use]
    pub fn action_key(&self) -> String {
        format!("{SHELL_TOOL}:{}", self.text())
    }
}

/// One word of a command. `quoted` is whether any part of it was inside quotes: a program or a
/// subcommand must be written plainly, so a quoted one has no rule.
struct Word {
    text: String,
    quoted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
}

/// Split a command into words, or `None` when it uses any syntax beyond plain words, single quotes
/// and double quotes without expansion.
fn words(command: &str) -> Option<Vec<Word>> {
    if command.chars().count() > MAX_COMMAND_CHARS {
        return None;
    }
    let mut words = Vec::new();
    let mut text = String::new();
    let mut quoted = false;
    let mut quote = Quote::None;
    for c in command.chars() {
        match quote {
            Quote::None => {
                if c == ' ' || c == '\t' {
                    if !text.is_empty() || quoted {
                        words.push(Word {
                            text: std::mem::take(&mut text),
                            quoted,
                        });
                        quoted = false;
                    }
                } else if c == '\'' {
                    quote = Quote::Single;
                    quoted = true;
                } else if c == '"' {
                    quote = Quote::Double;
                    quoted = true;
                } else if c.is_alphanumeric() || PLAIN_PUNCTUATION.contains(c) {
                    text.push(c);
                } else {
                    return None;
                }
            }
            Quote::Single => {
                if c == '\'' {
                    quote = Quote::None;
                } else if c.is_control() {
                    return None;
                } else {
                    text.push(c);
                }
            }
            Quote::Double => {
                if c == '"' {
                    quote = Quote::None;
                } else if c.is_control() || matches!(c, '$' | '`' | '\\' | '!') {
                    return None;
                } else {
                    text.push(c);
                }
            }
        }
        if words.len() > MAX_TOKENS {
            return None;
        }
    }
    if quote != Quote::None {
        return None;
    }
    if !text.is_empty() || quoted {
        words.push(Word { text, quoted });
    }
    (words.len() <= MAX_TOKENS).then_some(words)
}

/// The rule a shell command may propose, or `None` when it has none (see the module docs).
#[must_use]
pub fn rule_for_command(command: &str) -> Option<ShellRule> {
    let words = words(command)?;
    let first = words.first()?;
    // `NAME=value cmd` runs `cmd` under a changed environment, which a rule could not show.
    if first.quoted || first.text.contains('=') {
        return None;
    }
    let (program, subcommands) = PROGRAMS.iter().find(|(name, _)| *name == first.text)?;
    let subcommand = match subcommands {
        Subcommands::None => None,
        Subcommands::OneOf(names) => {
            let second = words.get(1).filter(|word| !word.quoted)?;
            Some(*names.iter().find(|name| **name == second.text)?)
        }
    };
    Some(ShellRule {
        program: *program,
        subcommand,
    })
}

/// The rule a `shell` tool call may propose, read from its `command` argument. A call whose input
/// is not an object with a string `command` has no rule.
#[must_use]
pub fn rule_for_shell_input(tool_input: &str) -> Option<ShellRule> {
    let input = serde_json::from_str::<Value>(tool_input).ok()?;
    rule_for_command(input.get("command")?.as_str()?)
}

/// The shell-rule switch on without WS13-S7's rule switch: a narrower proposal with no proposal
/// path to carry it. Refused at startup rather than ignored, so the configuration is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellRuleSwitchRefused;

impl fmt::Display for ShellRuleSwitchRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{SHELL_RULE_PROPOSALS_ENV} requires {RULE_PROPOSALS_ENV}"
        )
    }
}

impl std::error::Error for ShellRuleSwitchRefused {}

/// Parse the two switches: the shell-rule switch's value, refused when it is on and the rule switch
/// is not. Both are read by the one strict parser.
///
/// # Errors
/// [`ShellRuleSwitchRefused`] when the shell-rule switch is on and the rule switch is not.
pub fn shell_rule_proposals_from(
    shell_rules: Option<&str>,
    rule_proposals: Option<&str>,
) -> Result<bool, ShellRuleSwitchRefused> {
    let on = switch_from(shell_rules);
    if on && !switch_from(rule_proposals) {
        return Err(ShellRuleSwitchRefused);
    }
    Ok(on)
}

/// The switch as this process has it, for the startup check: an error refuses to start.
///
/// # Errors
/// [`ShellRuleSwitchRefused`], as [`shell_rule_proposals_from`].
pub fn shell_rule_proposals_checked() -> Result<bool, ShellRuleSwitchRefused> {
    shell_rule_proposals_from(
        std::env::var(SHELL_RULE_PROPOSALS_ENV).ok().as_deref(),
        std::env::var(RULE_PROPOSALS_ENV).ok().as_deref(),
    )
}

/// Whether shell-rule proposals are on in this process. A refused combination reads as off, so a
/// process that skipped the startup check still proposes nothing new.
#[must_use]
pub fn shell_rule_proposals_enabled() -> bool {
    shell_rule_proposals_checked().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(command: &str) -> Option<String> {
        rule_for_command(command).map(|rule| rule.text())
    }

    #[test]
    fn shell_rule_table() {
        let cases: &[(&str, Option<&str>)] = &[
            // Program and subcommand, arguments dropped.
            ("git status", Some("git status")),
            ("  git   status  ", Some("git status")),
            ("git\tstatus", Some("git status")),
            ("git status --short", Some("git status")),
            ("git log --oneline -n 5", Some("git log")),
            (
                "git commit -m \"Fiks ærlig talt små feil\"",
                Some("git commit"),
            ),
            ("git commit -m 'Rettet øl og ål'", Some("git commit")),
            ("git add src/bøker/ærfugl.rs", Some("git add")),
            ("git diff --stat=80", Some("git diff")),
            (
                "cargo test -p mp-contracts --lib shell_rule",
                Some("cargo test"),
            ),
            ("go test ./...", Some("go test")),
            ("pnpm install", Some("pnpm install")),
            ("pip list", Some("pip list")),
            // Programs whose rule is the program alone.
            ("ls", Some("ls")),
            ("ls -la Påskeferie/", Some("ls")),
            ("cat 'Møtereferat 2026.txt'", Some("cat")),
            ("echo \"Hei på deg, Åse\"", Some("echo")),
            ("grep -rn TODO src", Some("grep")),
            ("wc -l a.rs b.rs", Some("wc")),
            // Compound commands, redirects and substitution.
            ("git status | head", None),
            ("git status; rm -rf /", None),
            ("git status && git push", None),
            ("git status & sleep 1", None),
            ("git status > out.txt", None),
            ("cat < in.txt", None),
            ("git log $(whoami)", None),
            ("git log `whoami`", None),
            ("echo $HOME", None),
            ("echo \"$HOME\"", None),
            ("echo \"`id`\"", None),
            ("echo \"a\\\"b\"", None),
            ("ls *.rs", None),
            ("ls ~", None),
            ("ls \\ ", None),
            ("git status # comment", None),
            ("git status\nrm -rf /", None),
            ("(git status)", None),
            ("{ git status; }", None),
            // Environment assignments and wrappers.
            ("GIT_DIR=/tmp git status", None),
            ("env git status", None),
            ("sudo git status", None),
            ("xargs git status", None),
            ("sh -c 'git status'", None),
            // Programs and subcommands outside the tables, or not written plainly.
            ("rm -rf build", None),
            ("python script.py", None),
            ("/usr/bin/git status", None),
            ("./git status", None),
            ("'git' status", None),
            ("git 'status'", None),
            ("GIT status", None),
            ("git -C repo status", None),
            ("git config core.pager cat", None),
            ("git push", None),
            ("git", None),
            ("npm run build", None),
            ("git status 'unterminated", None),
            ("", None),
            ("   ", None),
        ];
        assert!(cases.len() >= 30);
        for (command, expected) in cases {
            assert_eq!(rule(command).as_deref(), *expected, "command: {command:?}");
        }
    }

    #[test]
    fn shell_rule_never_contains_an_argument_token() {
        let commands = [
            "git commit -m 'hemmelig passord ø'",
            "git add secrets.env",
            "cat /etc/passwd",
            "ls Påskeferie",
            "cargo test secret_test_name",
            "echo token-123",
        ];
        for command in commands {
            let rule = rule_for_command(command).expect("a rule");
            let text = rule.text();
            let named = text.split(' ').count();
            for argument in command
                .split([' ', '\''])
                .filter(|word| !word.is_empty())
                .skip(named)
            {
                assert!(!text.contains(argument), "{command:?} proposed {text:?}");
            }
            assert_eq!(rule.action_key(), format!("shell:{text}"));
        }
    }

    #[test]
    fn shell_rule_has_no_rule_for_compound_commands() {
        for joiner in [
            " | ", "; ", " && ", " || ", " & ", " > ", " >> ", " < ", "\n",
        ] {
            let command = format!("git status{joiner}ls");
            assert_eq!(rule(&command), None, "joiner {joiner:?}");
        }
    }

    #[test]
    fn shell_rule_reads_the_command_argument_only() {
        assert_eq!(
            rule_for_shell_input(r#"{"command":"git status","cwd":"/w"}"#)
                .map(|rule| rule.action_key()),
            Some("shell:git status".to_owned())
        );
        assert_eq!(
            rule_for_shell_input(r#"{"command":["git","status"]}"#),
            None
        );
        assert_eq!(rule_for_shell_input(r#"{"cmd":"git status"}"#), None);
        assert_eq!(rule_for_shell_input("git status"), None);
    }

    #[test]
    fn shell_rule_refuses_an_overlong_command() {
        let long = format!("ls {}", "a".repeat(MAX_COMMAND_CHARS));
        assert_eq!(rule(&long), None);
        let many = format!("ls{}", " a".repeat(MAX_TOKENS));
        assert_eq!(rule(&many), None);
        let enough = format!("ls{}", " a".repeat(MAX_TOKENS - 1));
        assert_eq!(rule(&enough).as_deref(), Some("ls"));
    }

    #[test]
    fn shell_rule_switch_is_strict_and_needs_the_rule_switch() {
        assert_eq!(shell_rule_proposals_from(None, None), Ok(false));
        assert_eq!(
            shell_rule_proposals_from(Some("1"), Some("true")),
            Ok(false)
        );
        assert_eq!(shell_rule_proposals_from(Some("yes"), None), Ok(false));
        assert_eq!(shell_rule_proposals_from(None, Some("true")), Ok(false));
        assert_eq!(
            shell_rule_proposals_from(Some(" TRUE "), Some("true")),
            Ok(true)
        );
        assert_eq!(
            shell_rule_proposals_from(Some("true"), None),
            Err(ShellRuleSwitchRefused)
        );
        assert_eq!(
            shell_rule_proposals_from(Some("true"), Some("1")),
            Err(ShellRuleSwitchRefused)
        );
    }
}
