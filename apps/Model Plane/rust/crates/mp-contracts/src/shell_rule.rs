//! The narrow rule an "Always" on a shell approval may propose (build plan EP-9 E4).
//!
//! A person who approves `git diff --stat src` and chooses "Always" must be offered a rule they can
//! read and that says less than the command did: `git diff`, never the path or a host. So the rule is
//! never cut out of the command text. It is a program and, for the programs that have them, a
//! subcommand, both taken from the closed tables below and returned as the table's own `&'static str`;
//! the command only selects a table row.
//!
//! The table holds read-only programs only (the owner's rule for an "Always", D-f09-6). A program that
//! writes a file or runs project code (a build, a test, an install, a commit, a formatter that rewrites
//! the tree) has no row. A word after a read-only program can still make it write or run a program
//! (`git diff --output=f`, `sort -o f`, `rg --pre cmd`, `git -c core.pager=cmd log`), so the
//! `DENIED_ARGUMENTS` table refuses the rule for those words too. A refused rule never changes the call: the
//! approval is offered once, as before.
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

/// Programs that may be named by a rule: read-only programs only (D-f09-6). A program not listed has no
/// rule, whatever follows it. An interpreter (`sh`, `python`), a wrapper that runs another command (`env`,
/// `xargs`, `sudo`, `find -exec`) or a destructive tool (`rm`) is never proposed. So is a step that runs
/// project code or writes the tree: a build, a test, an install, a commit, a stage, a fetch, a switch, a
/// formatter that rewrites files (`cargo fmt`, `go fmt`), and `uniq`, whose second operand is an output file.
/// Subcommands that run arbitrary configured code by name (`git config`, `npm run`) are left out for the
/// same reason. A read-only row can still be refused by one of its words; see [`DENIED_ARGUMENTS`].
const PROGRAMS: &[(&str, Subcommands)] = &[
    (
        "git",
        Subcommands::OneOf(&[
            "blame",
            "diff",
            "log",
            "ls-files",
            "rev-parse",
            "show",
            "status",
        ]),
    ),
    ("cargo", Subcommands::OneOf(&["metadata", "tree"])),
    ("go", Subcommands::OneOf(&["list", "version"])),
    ("npm", Subcommands::OneOf(&["ls", "outdated"])),
    ("pnpm", Subcommands::OneOf(&["list", "outdated"])),
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
    ("wc", Subcommands::None),
    ("which", Subcommands::None),
    ("whoami", Subcommands::None),
];

/// Words that refuse a rule when they come after the program: each one makes the program write a file or
/// run a program the rule would not show. A row names the program it applies to (`"*"` for every program),
/// the word, and why the word is refused. The table is matched on the word with any `=value` removed, and
/// case-sensitively:
///   - a long word (`--name`) refuses the flag itself, a longer flag of the same family (`--name-...`), and
///     an unambiguous abbreviation of the flag, which git and the GNU tools accept (`--outp` for `--output`);
///   - a single-letter word (`-o`) refuses any short cluster that holds the letter (`-ro`);
///   - a single-dash long word (`-toolexec`) refuses the word itself, its family, and the same word with a
///     second leading dash (`--toolexec`), which the go tool reads as the same flag.
/// A value after `=` that reads as a path refuses the rule for any flag (see [`argument_refused`]).
const DENIED_ARGUMENTS: &[(&str, &str)] = &[
    // Writes the output to a named file: git diff, log and show, and sort.
    ("*", "--output"),
    // Runs a named program: an external diff, a remote helper, a pager, a compressor, a preprocessor.
    ("*", "--exec"),
    // rg runs the named program to convert each file it reads. `--pretty` is a different word.
    ("*", "--pre"),
    // rg runs its preprocessor only on the files the glob matches.
    ("*", "--pre-glob"),
    // rg runs the named program to find the host name it puts in a hyperlink format. Not in the spec's list; it
    // runs a program, the same class as --pre.
    ("rg", "--hostname-bin"),
    // git runs an external diff program.
    ("*", "--ext-diff"),
    // git runs a configured text conversion program.
    ("*", "--textconv"),
    // git runs a named program on the remote side of a fetch.
    ("*", "--upload-pack"),
    // git runs a named program on the remote side of a push.
    ("*", "--receive-pack"),
    // git runs a named program as its pager.
    ("*", "--open-files-in-pager"),
    // sort runs a named program on its temporary files.
    ("*", "--compress-program"),
    // git loads its helper programs from a named directory.
    ("*", "--exec-path"),
    // Sets configuration for one command; git's `--config-env` can name a variable that sets a program.
    ("*", "--config"),
    // git's configuration override: a `core.pager`, alias or diff driver runs a program.
    ("git", "-c"),
    // git runs in another directory, so the rule would describe another repository.
    ("git", "-C"),
    // Names an order file: a path the rule does not show.
    ("git", "-O"),
    // Points git at another repository.
    ("git", "--git-dir"),
    // Points git at another working tree.
    ("git", "--work-tree"),
    // Writes the sorted output to a named file.
    ("sort", "-o"),
    // Writes its temporary files into the named directory (not in the spec's list; it writes a directory).
    ("sort", "-T"),
    // Writes its temporary files into the named directory, long form (not in the spec's list).
    ("sort", "--temporary-directory"),
    // Writes the listing to a named file.
    ("tree", "-o"),
    // With -H or -T, writes a 00Tree.html file into each directory (not in the spec's list; a writer).
    ("tree", "-R"),
    // Sets the system clock.
    ("date", "-s"),
    // Sets the system clock, long form.
    ("date", "--set"),
    // Writes a compiled magic file into the directory.
    ("file", "-C"),
    // Writes a compiled magic file, long form.
    ("file", "--compile"),
    // A build flag that names a program the go tool runs in place of its own compile and link steps. A
    // `go list` that builds reaches it (not in the spec's list; it runs a program).
    ("go", "-toolexec"),
    // `-mod=mod` lets go list update go.mod and go.sum (not in the spec's list; it writes).
    ("go", "-mod"),
    // Names another go.mod that `-mod=mod` may update (not in the spec's list; it writes).
    ("go", "-modfile"),
    // `go list -export` compiles each package to report its export data (not in the spec's list; it builds).
    ("go", "-export"),
    // `go list -compiled` runs cgo on each package to report the files the compiler sees (not in the spec's list).
    ("go", "-compiled"),
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
    // A word after the program may still make it write or run a program, whatever its row says.
    if argument_refused(program, &words[1..]) {
        return None;
    }
    Some(ShellRule {
        program: *program,
        subcommand,
    })
}

/// Whether a word after the program refuses the rule: a denied word in any form (see
/// [`DENIED_ARGUMENTS`]), or a flag whose value reads as a path, which can name an output or an input the
/// rule does not show.
fn argument_refused(program: &str, rest: &[Word]) -> bool {
    rest.iter().any(|word| {
        let (name, value) = match word.text.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (word.text.as_str(), None),
        };
        let path_value = name.starts_with('-') && value.is_some_and(looks_like_path);
        path_value
            || DENIED_ARGUMENTS
                .iter()
                .any(|&(scope, flag)| (scope == "*" || scope == program) && denies(name, flag))
    })
}

/// Whether a flag's value reads as a path: it has a separator, or starts with `.` or `~`.
fn looks_like_path(value: &str) -> bool {
    value.contains(['/', '\\']) || value.starts_with(['.', '~'])
}

/// Whether `word` (its `=value` already removed) is the denied `flag`. A single-letter flag is refused by any
/// short cluster that holds the letter. Any other flag is refused by the word itself, a longer flag of the
/// same family, or an abbreviation of the flag. A single-dash long flag is refused in its double-dash form too
/// (`--toolexec`): the go tool reads `--flag` as `-flag`.
fn denies(word: &str, flag: &str) -> bool {
    let single_letter = flag.len() == 2 && flag.starts_with('-') && !flag.starts_with("--");
    if single_letter {
        let letter = flag.as_bytes()[1];
        return word.len() > 1
            && word.starts_with('-')
            && !word.starts_with("--")
            && word.as_bytes()[1..].contains(&letter);
    }
    let single_dash_long = flag.starts_with('-') && !flag.starts_with("--");
    let word = if single_dash_long {
        single_dash(word)
    } else {
        word
    };
    word.len() > 2
        && (word == flag
            || word
                .strip_prefix(flag)
                .is_some_and(|rest| rest.starts_with('-'))
            || flag.starts_with(word))
}

/// The word with its leading dashes cut to one, so `--toolexec` and `---toolexec` both read as `-toolexec`.
/// A word with no leading dash is returned unchanged.
fn single_dash(word: &str) -> &str {
    let name = word.trim_start_matches('-');
    match word.len() - name.len() {
        0 => word,
        dashes => &word[dashes - 1..],
    }
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
            ("git diff --stat=80", Some("git diff")),
            ("git blame src/lib.rs", Some("git blame")),
            ("git ls-files", Some("git ls-files")),
            ("git rev-parse --show-toplevel", Some("git rev-parse")),
            ("git show HEAD:README.md", Some("git show")),
            ("cargo metadata --format-version 1", Some("cargo metadata")),
            ("cargo tree -p mp-contracts", Some("cargo tree")),
            ("go list ./...", Some("go list")),
            ("go version", Some("go version")),
            ("npm ls --depth=0", Some("npm ls")),
            ("npm outdated", Some("npm outdated")),
            ("pnpm list --depth 0", Some("pnpm list")),
            ("pip list", Some("pip list")),
            ("pip freeze", Some("pip freeze")),
            // Programs and subcommands that write or run project code: no rule (D-f09-6).
            ("git commit -m \"Fiks ærlig talt små feil\"", None),
            ("git commit -m 'Rettet øl og ål'", None),
            ("git add src/bøker/ærfugl.rs", None),
            ("git branch -D old", None),
            ("git fetch origin", None),
            ("git pull", None),
            ("git restore src/lib.rs", None),
            ("git switch main", None),
            ("cargo build", None),
            ("cargo check", None),
            ("cargo clippy", None),
            ("cargo doc", None),
            ("cargo fmt", None),
            ("cargo test -p mp-contracts --lib shell_rule", None),
            ("go build ./...", None),
            ("go fmt ./...", None),
            ("go test ./...", None),
            ("go vet ./...", None),
            ("npm ci", None),
            ("npm test", None),
            ("pnpm install", None),
            ("pnpm test", None),
            ("uniq a.txt b.txt", None),
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
            // Words that write a file or run a program after a read-only program (DENIED_ARGUMENTS).
            ("git diff --output=x", None),
            ("git diff --outp=x", None),
            ("git diff --ext-diff", None),
            ("git show --textconv HEAD", None),
            ("git log -O order.txt", None),
            ("git -c core.pager=x log", None),
            ("git blame -C src/lib.rs", None),
            ("git log --git-dir=/elsewhere", None),
            ("sort -o f", None),
            ("sort -ro out.txt in.txt", None),
            ("sort --compress-program=gzip f", None),
            ("sort -T /tmp f", None),
            ("sort --temporary-directory=/tmp f", None),
            ("tree -o out.txt", None),
            ("tree -RH .", None),
            ("rg --pre x pat", None),
            ("rg --pre-glob '*.gz' pat", None),
            ("rg --hostname-bin=x pat", None),
            ("date -s 2020-01-01", None),
            ("date --set=2020", None),
            ("file -C magic", None),
            ("file --compile magic", None),
            ("go list -toolexec=x ./...", None),
            ("go list -mod=mod ./...", None),
            ("go list -modfile=x.mod ./...", None),
            ("go list -export ./...", None),
            ("go list -compiled ./...", None),
            // The go tool reads `--flag` as `-flag`: the double-dash form of a go row is refused too.
            ("go list --toolexec=x ./...", None),
            ("go list --mod=mod ./...", None),
            ("go list --modfile=x.mod ./...", None),
            ("go list --export ./...", None),
            ("go list --compiled ./...", None),
            // A read-only form of a row keeps its rule, and a flag that only shares a prefix is not refused.
            ("git diff --stat src", Some("git diff")),
            ("git log -n 5", Some("git log")),
            ("git log --pretty=oneline", Some("git log")),
            ("git log --contains HEAD", Some("git log")),
            ("ls --color=auto", Some("ls")),
            ("grep --include='*.rs' -rn TODO src", Some("grep")),
            ("go list -m ./...", Some("go list")),
            // A flag whose value reads as a path refuses the rule, whichever flag it is.
            ("grep --exclude-dir=target/x -rn TODO src", None),
            ("ls --hide='~/x'", None),
            ("git log --format=%h/%s", None),
        ];
        assert!(cases.len() >= 30);
        for (command, expected) in cases {
            assert_eq!(rule(command).as_deref(), *expected, "command: {command:?}");
        }
    }

    #[test]
    fn shell_rule_never_contains_an_argument_token() {
        let commands = [
            "git log --format=hemmelig-passord",
            "git blame secrets.env",
            "cat /etc/passwd",
            "ls Påskeferie",
            "cargo tree secret_crate",
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
        // Exact: a padded or cased `true` leaves the shell-rule switch off, so nothing is refused.
        assert_eq!(
            shell_rule_proposals_from(Some(" TRUE "), Some("true")),
            Ok(false)
        );
        assert_eq!(
            shell_rule_proposals_from(Some("true"), Some("true")),
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

    #[test]
    fn the_program_table_is_read_only_only() {
        let rows: Vec<(&str, Vec<&str>)> = PROGRAMS
            .iter()
            .map(|(program, subcommands)| {
                let names = match subcommands {
                    Subcommands::None => Vec::new(),
                    Subcommands::OneOf(names) => names.to_vec(),
                };
                (*program, names)
            })
            .collect();
        let expected: Vec<(&str, Vec<&str>)> = vec![
            (
                "git",
                vec![
                    "blame",
                    "diff",
                    "log",
                    "ls-files",
                    "rev-parse",
                    "show",
                    "status",
                ],
            ),
            ("cargo", vec!["metadata", "tree"]),
            ("go", vec!["list", "version"]),
            ("npm", vec!["ls", "outdated"]),
            ("pnpm", vec!["list", "outdated"]),
            ("pip", vec!["check", "freeze", "list", "show"]),
            ("cat", vec![]),
            ("date", vec![]),
            ("df", vec![]),
            ("diff", vec![]),
            ("du", vec![]),
            ("echo", vec![]),
            ("file", vec![]),
            ("grep", vec![]),
            ("head", vec![]),
            ("ls", vec![]),
            ("pwd", vec![]),
            ("rg", vec![]),
            ("sort", vec![]),
            ("stat", vec![]),
            ("tail", vec![]),
            ("tree", vec![]),
            ("uname", vec![]),
            ("wc", vec![]),
            ("which", vec![]),
            ("whoami", vec![]),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn every_denied_word_names_every_program_or_a_program_in_the_table() {
        let mut seen = std::collections::BTreeSet::new();
        for &(scope, flag) in DENIED_ARGUMENTS {
            assert!(
                scope == "*" || PROGRAMS.iter().any(|(program, _)| *program == scope),
                "{scope} {flag}: no such program in the table"
            );
            assert!(flag.starts_with('-'), "{flag}");
            assert!(seen.insert((scope, flag)), "{scope} {flag} is listed twice");
        }
    }

    #[test]
    fn a_denied_word_refuses_the_rule_in_every_form_the_tools_accept() {
        for command in [
            "git diff --output-indicator-new=x",
            "git diff --outp x",
            "git log --ext=x",
            "git diff --exec-path=/x",
            "sort --output=f",
            "sort -ro out",
            "date -sR",
            "tree -oH .",
            "rg --pre=cmd pat",
            "git log -cx=y",
            "file -bC magic",
        ] {
            assert_eq!(rule(command), None, "{command}");
        }
    }

    #[test]
    fn a_read_only_form_of_a_row_keeps_its_rule() {
        assert_eq!(rule("git diff --stat src").as_deref(), Some("git diff"));
        assert_eq!(rule("git log -n 5").as_deref(), Some("git log"));
        assert_eq!(rule("sort -k 2 -u names.txt").as_deref(), Some("sort"));
        assert_eq!(rule("date --utc").as_deref(), Some("date"));
    }

    #[test]
    fn a_single_dash_long_go_flag_also_refuses_the_double_dash_form() {
        // The go tool reads `--flag` as `-flag`, so a single-dash long row refuses the double-dash word too:
        // with a value, as a separate word, as an abbreviation, and with a third dash (which go refuses itself).
        for command in [
            "go list --toolexec=prog ./...",
            "go list --toolexec prog ./...",
            "go list --tool=prog ./...",
            "go list ---toolexec=prog ./...",
            "go list --export ./...",
            "go list --compiled ./...",
            "go list --mod=mod ./...",
            "go list --modfile=x.mod ./...",
        ] {
            assert_eq!(rule(command), None, "{command}");
        }
        // Read-only double-dash flags of go list keep the rule.
        assert_eq!(rule("go list --json ./...").as_deref(), Some("go list"));
        assert_eq!(
            rule("go list --deps --json ./...").as_deref(),
            Some("go list")
        );
    }
}
