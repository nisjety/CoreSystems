//! Startup report for the default-off switches that read only their exact value, and the drift test
//! that keeps the report in step with the readers.
//!
//! Each switch in [`EXACT_SWITCHES`] is read exactly by its reader: the literal `true`, the way the
//! shared `mp_contracts::loop_fold::switch_from` reads it. Before those readers were made exact,
//! several of them also took `1`, `on`, `yes`, `enabled`, or `true` in another case or with
//! surrounding whitespace. A live environment that still sets one of those spellings now reads as
//! off, silently.
//!
//! This module changes no reader and no switch value. A switch set to such a spelling stays off, as
//! it does under its exact reader. [`warn_former_spellings`] logs one warning per such switch at
//! startup, so the mismatch shows in the logs instead of only as a missing feature.
//!
//! The table names each switch through its reader's own constant, so a renamed constant fails to
//! compile. The drift test checks each reader: every place in this crate that reads a listed switch
//! holds the exact read of its literal, so a literal change or a lenient reader fails a test even when
//! another reader of the same switch, in the same file, is still exact.
//!
//! [`DOCUMENTED_EXCEPTIONS`] lists the switches this service reads by another rule, and says why.

use tracing::warn;

/// Each switch this service reads exactly, with the one value that turns it on. Each name is the
/// reader's own constant: keep it in step with the reader, and the drift test checks the reader.
pub const EXACT_SWITCHES: &[(&str, &str)] = &[
    (crate::config::ALLOW_GLOBAL_DEPLOYMENT_ENV, "true"),
    (crate::config::ALLOW_GLOBAL_RESIDENCY_ENV, "true"),
    (crate::config::ALLOW_NON_EU_EMBEDDING_ENV, "true"),
    (crate::provider::speech::NON_EU_TTS_ENV, "true"),
    (crate::config::BYO_GATEWAY_ENV, "true"),
    (crate::config::PROVIDER_BENCHING_ENV, "true"),
    (crate::config::SAME_TIER_FALLBACK_ENV, "true"),
    (crate::config::VERTEX_GEMINI_ENABLED_ENV, "true"),
    (crate::auth::API_KEY_PRINCIPAL_ENV, "true"),
    (crate::http_health::PROMOTION_REQUIRED_ENV, "true"),
    (crate::provider::anthropic::TIERED_SYSTEM_ENV, "true"),
    (crate::provider::anthropic::ADAPTIVE_THINKING_ENV, "true"),
    (
        crate::provider::catalogue_overlay::CATALOGUE_OVERLAY_ENV,
        "true",
    ),
    (crate::provider::catalogue_overlay::ORG_OVERLAY_ENV, "true"),
    (
        crate::provider::codex_subscription::SPEED_TIER_SPLIT_ENV,
        "true",
    ),
    (crate::provider::effort::GEMINI_EFFORT_ENV, "true"),
    (crate::provider::effort::BUDGET_CLAMP_ENV, "true"),
    (crate::provider::effort::CODEX_WIDE_ENV, "true"),
    (crate::provider::fallback::ROUTE_NOTE_ENV, "true"),
    (crate::provider::openai::REASONING_EFFORT_ENV, "true"),
    (crate::provider::openai::PROMPT_CACHE_KEY_ENV, "true"),
    (crate::request_capture::CAPTURE_ENV, "true"),
];

/// The switches this service reads by a rule other than the exact literal in [`EXACT_SWITCHES`]. None
/// of them is listed there, and each one says why it stays as it is.
pub const DOCUMENTED_EXCEPTIONS: &[(&str, &str)] = &[
    (
        "ANTHROPIC_NATIVE_CONTEXT_MANAGEMENT",
        "default-on kill switch: on unless an explicit off word (the shared list), trimmed and in any case; unset or blank is on (provider::anthropic)",
    ),
    (
        "INFERENCE_LOGPROBS_ENABLED",
        "default-on kill switch: off only for 0, false, no or off, trimmed and in any case (provider::logprobs)",
    ),
    (
        "VEREVON_INTENT_ENABLED",
        "default-on kill switch: off only for 0, false, off or no, trimmed and in any case (config)",
    ),
    (
        "AZURE_OPENAI_ZDR_CONFIRMED",
        "legacy attestation boolean: exact true claims ZDR, an off word makes no claim, any other value refuses boot (config legacy_zdr_asserted)",
    ),
    (
        "AZURE_ANTHROPIC_ZDR_CONFIRMED",
        "legacy attestation boolean: exact true claims ZDR, an off word makes no claim, any other value refuses boot (config legacy_zdr_asserted)",
    ),
];

/// The switches in [`EXACT_SWITCHES`] whose value, as `lookup` returns it, is a spelling the exact
/// reader does not take. Each is returned with its exact value, in table order.
#[must_use]
pub fn former_spellings(
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(&'static str, &'static str)> {
    EXACT_SWITCHES
        .iter()
        .copied()
        .filter(|&(name, literal)| {
            mp_contracts::loop_fold::former_spelling(literal, lookup(name).as_deref())
        })
        .collect()
}

/// Logs one warning for each switch that is set to a former spelling. Called once, at startup,
/// before either listener opens. It changes no switch: each one stays off.
pub fn warn_former_spellings() {
    for (name, exact_value) in former_spellings(|name| std::env::var(name).ok()) {
        warn!(
            switch = name,
            exact_value,
            "switch is set to a value its exact reader does not take, so it reads as off; \
             set it to exactly this value to turn it on"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};

    /// The names reported for the given `(name, value)` environment.
    fn names(env: &[(&'static str, &'static str)]) -> Vec<&'static str> {
        former_spellings(|name| {
            env.iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| (*value).to_owned())
        })
        .into_iter()
        .map(|(name, _)| name)
        .collect()
    }

    #[test]
    fn every_listed_switch_is_listed_once_and_the_known_ones_are_present() {
        let mut seen = BTreeSet::new();
        for (name, _) in EXACT_SWITCHES {
            assert!(seen.insert(*name), "{name} is listed twice");
        }
        for required in [
            "MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT",
            "MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY",
            "MODEL_PLANE_ALLOW_NON_EU_EMBEDDING",
            "MODEL_PLANE_ALLOW_NON_EU_TTS",
            "INFERENCE_API_KEY_PRINCIPAL",
            "INFERENCE_ROUTE_NOTE",
            "INFERENCE_CATALOGUE_OVERLAY",
            "INFERENCE_REQUEST_CAPTURE",
            "INFERENCE_ANTHROPIC_TIERED_SYSTEM",
            "VERTEX_GEMINI_ENABLED",
        ] {
            assert!(seen.contains(required), "{required} is not listed");
        }
    }

    #[test]
    fn a_former_spelling_is_reported_for_every_listed_switch() {
        // `1`, `on`, `yes`, `enabled`, `TRUE` and padded `true` are the spellings an older reader took
        // as on. Each one is reported against each switch, except its own exact value.
        for (name, exact_value) in EXACT_SWITCHES {
            for former in ["1", "on", "yes", "enabled", "true", "TRUE", " true "] {
                if former == *exact_value {
                    continue;
                }
                assert_eq!(names(&[(*name, former)]), vec![*name], "{name}={former:?}");
            }
        }
    }

    #[test]
    fn the_exact_value_and_the_off_values_are_not_reported() {
        for (name, exact_value) in EXACT_SWITCHES {
            assert!(
                names(&[(*name, *exact_value)]).is_empty(),
                "{name}={exact_value:?}"
            );
            for off in ["", "0", "false", "off", "no"] {
                assert!(names(&[(*name, off)]).is_empty(), "{name}={off:?}");
            }
        }
        assert!(
            names(&[]).is_empty(),
            "an unset environment reports nothing"
        );
    }

    #[test]
    fn an_unrecognised_value_is_not_reported_as_a_former_spelling() {
        assert!(names(&[("INFERENCE_ROUTE_NOTE", "maybe")]).is_empty());
    }

    /// Every listed switch is read in production code, and each of its reads holds the exact read of
    /// its literal. The check runs per read, not per file.
    #[test]
    fn every_reader_of_a_listed_switch_reads_its_exact_literal() {
        let sources = production_sources();
        let functions: Vec<Vec<(usize, usize, String)>> = sources
            .iter()
            .map(|source| functions_in(&source.lines))
            .collect();
        let mut bodies: Bodies = HashMap::new();
        for (s, source) in sources.iter().enumerate() {
            for (start, end, name) in &functions[s] {
                bodies
                    .entry(name.clone())
                    .or_default()
                    .push((s, source.lines[*start..*end].join("\n")));
            }
        }
        let postings = postings(&sources);
        let defs = const_definitions(&sources);
        let mut problems = Vec::new();
        for &(name, literal) in EXACT_SWITCHES {
            let sites = reader_sites(name, &sources, &postings, &defs);
            if sites.is_empty() {
                problems.push(format!("{name} is listed, but no production code reads it"));
            }
            for (s, l) in sites {
                let (function, first, second) = read_text(&sources, &functions, (s, l));
                if !read_is_exact(&bodies, s, &first, literal)
                    && !read_is_exact(&bodies, s, &second, literal)
                {
                    problems.push(format!(
                        "{name}: {} production line {} (in {function}) does not read exactly {literal:?}",
                        sources[s].file,
                        sources[s].numbers[l]
                    ));
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// The crate's production source, one entry per file. Its lines are the ones outside
    /// `#[cfg(test)]` items and outside comments. This file is left out, since it names every switch.
    struct Source {
        /// The file name, such as `sse.rs`.
        file: String,
        /// The module names that declare this file (see `module_names_of`): a `module::NAME` path through
        /// one of them is a read of this file's constants.
        modules: Vec<String>,
        /// Whether each production line belongs to a `use` statement, which may span lines (`in_use_lines`).
        in_use: Vec<bool>,
        /// The production lines, and the 1-based line of each one in the file, for the report.
        lines: Vec<String>,
        numbers: Vec<usize>,
    }

    fn production_sources() -> Vec<Source> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).expect("the crate's source directory is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    walk(&path, out);
                    continue;
                }
                let file = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let is_rust = path.extension().and_then(|ext| ext.to_str()) == Some("rs");
                if !is_rust || file == "switch_spellings.rs" {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("a source file is readable");
                out.push((file, text.replace("\r\n", "\n")));
            }
        }
        let mut raw = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut raw,
        );
        // A file that a `#[cfg(test)]` module loads is test code, not production.
        let test_only = test_only_files(&raw);
        let mut out: Vec<Source> = raw
            .into_iter()
            .filter(|(file, _)| !test_only.contains(file))
            .map(|(file, text)| {
                let (numbers, lines): (Vec<usize>, Vec<String>) =
                    production_lines(&text).into_iter().unzip();
                Source {
                    file,
                    modules: Vec::new(),
                    in_use: in_use_lines(&lines),
                    lines,
                    numbers,
                }
            })
            .collect();
        assert!(
            out.len() > 30,
            "the scan saw the crate's sources ({} files)",
            out.len()
        );
        let modules: Vec<Vec<String>> = (0..out.len())
            .map(|owner| module_names_of(&out, owner))
            .collect();
        for (source, names) in out.iter_mut().zip(modules) {
            source.modules = names;
        }
        out
    }

    /// The module name, if `line` is a `mod NAME;` declaration that loads a file.
    fn declared_module(line: &str) -> Option<&str> {
        let rest = line.trim_start();
        let rest = rest
            .strip_prefix("pub(crate) ")
            .or_else(|| rest.strip_prefix("pub "))
            .unwrap_or(rest);
        let name = rest.strip_prefix("mod ")?.strip_suffix(';')?;
        (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')).then_some(name)
    }

    /// The file a `#[path = "..."]` attribute names, without its directories, if `line` is one.
    fn path_attribute(line: &str) -> Option<String> {
        let rest = line.trim_start().strip_prefix("#[path")?;
        let start = rest.find('"')? + 1;
        let end = start + rest[start..].find('"')?;
        rest[start..end]
            .rsplit(|c| c == '/' || c == '\\')
            .next()
            .map(str::to_owned)
    }

    /// The file the `mod NAME;` at `lines[l]` loads: the file a `#[path]` attribute above it names, or
    /// else `NAME.rs`. Also whether a `#[cfg(test)]` attribute above it gates the declaration.
    fn declared_file<S: AsRef<str>>(lines: &[S], l: usize, name: &str) -> (String, bool) {
        let mut file = format!("{name}.rs");
        let mut gated = false;
        let mut above = l;
        while above > 0 && lines[above - 1].as_ref().trim_start().starts_with("#[") {
            above -= 1;
            gated |= lines[above].as_ref().trim() == "#[cfg(test)]";
            if let Some(path) = path_attribute(lines[above].as_ref()) {
                file = path;
            }
        }
        (file, gated)
    }

    /// The files that a `#[cfg(test)]` module declaration loads. Those are test code, not production.
    fn test_only_files(raw: &[(String, String)]) -> Vec<String> {
        let mut out = Vec::new();
        for (_, text) in raw {
            let lines: Vec<&str> = text.lines().collect();
            for (l, line) in lines.iter().enumerate() {
                let Some(name) = declared_module(line) else {
                    continue;
                };
                let (file, gated) = declared_file(&lines, l, name);
                if gated {
                    out.push(file);
                }
            }
        }
        out
    }

    /// The module names that declare `sources[owner]`: each `mod NAME;` whose file is that one. So
    /// `pub mod parts;` under `#[path = "vision_parts.rs"]` gives `parts`, and not the file stem.
    fn module_names_of(sources: &[Source], owner: usize) -> Vec<String> {
        let file = sources[owner].file.as_str();
        let mut names = Vec::new();
        for source in sources {
            for (l, line) in source.lines.iter().enumerate() {
                let Some(name) = declared_module(line) else {
                    continue;
                };
                if declared_file(&source.lines, l, name).0 == file {
                    names.push(name.to_owned());
                }
            }
        }
        names
    }

    /// `text` without its `#[cfg(test)]` items and its comment lines, each with its 1-based line number
    /// in `text`. A `#[cfg(test)]` on a one-line item drops that line; on any other item it drops the
    /// lines through the closing brace at the attribute's own indentation.
    fn production_lines(text: &str) -> Vec<(usize, String)> {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim_start();
            if trimmed.starts_with("#[cfg(test)]") {
                let indent = line.len() - trimmed.len();
                // The item is the first line after any further attributes, such as `#[path = "..."]`.
                let item = (i + 1..lines.len())
                    .find(|&next| !lines[next].trim_start().starts_with("#["))
                    .unwrap_or(lines.len());
                let next = lines.get(item).map_or("", |next| next.trim());
                let one_line = (next.ends_with(';') && !next.contains('{'))
                    || (next.contains('{') && next.ends_with('}'));
                if one_line {
                    i = item + 1;
                    continue;
                }
                let mut end = item;
                while end < lines.len() {
                    let body = lines[end];
                    let body_trimmed = body.trim_start();
                    if body.len() - body_trimmed.len() == indent && body_trimmed.starts_with('}') {
                        break;
                    }
                    end += 1;
                }
                i = end + 1;
                continue;
            }
            if !trimmed.starts_with("//") {
                out.push((i + 1, line.to_owned()));
            }
            i += 1;
        }
        out
    }

    /// The name of the function whose header is `line`, when it is one.
    fn fn_name_of(line: &str) -> Option<&str> {
        let trimmed = line.trim_start();
        let at = trimmed.find("fn ")?;
        let qualifiers_only = trimmed[..at].split_whitespace().all(|word| {
            matches!(
                word,
                "pub" | "pub(crate)" | "pub(super)" | "async" | "const" | "unsafe"
            )
        });
        if !qualifiers_only {
            return None;
        }
        let name = trimmed[at + 3..]
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()?;
        (!name.is_empty()).then_some(name)
    }

    /// The `[start, end)` line range of every function in `lines`, with its name. A body ends at the
    /// closing brace at the header's own indentation, which rustfmt keeps.
    fn functions_in(lines: &[String]) -> Vec<(usize, usize, String)> {
        let mut out = Vec::new();
        for (start, line) in lines.iter().enumerate() {
            let Some(name) = fn_name_of(line) else {
                continue;
            };
            let header = line.trim_end();
            let indent = line.len() - line.trim_start().len();
            let one_line = header.ends_with(';')
                || (header.contains('{')
                    && header.matches('{').count() == header.matches('}').count());
            let end = if one_line {
                start + 1
            } else {
                lines[start + 1..]
                    .iter()
                    .position(|later| {
                        let body = later.trim_start();
                        later.len() - body.len() == indent && body.starts_with('}')
                    })
                    .map_or(lines.len(), |offset| start + offset + 2)
            };
            out.push((start, end, name.to_owned()));
        }
        out
    }

    /// Where each identifier and each quoted literal of the crate occurs, as `(source, line)` pairs.
    /// A quoted literal is keyed with its quotes, so the key `"NAME"` finds the string `NAME`.
    fn postings(sources: &[Source]) -> HashMap<String, Vec<(usize, usize)>> {
        let mut map: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        for (s, source) in sources.iter().enumerate() {
            for (l, line) in source.lines.iter().enumerate() {
                let mut seen = BTreeSet::new();
                let mut ident = String::new();
                let mut quoted: Option<String> = None;
                for c in line.chars() {
                    if let Some(text) = quoted.as_mut() {
                        if c == '"' {
                            seen.insert(format!("\"{text}\""));
                            quoted = None;
                        } else {
                            text.push(c);
                        }
                        continue;
                    }
                    if c == '"' {
                        quoted = Some(String::new());
                    } else if c.is_alphanumeric() || c == '_' {
                        ident.push(c);
                    } else if !ident.is_empty() {
                        seen.insert(std::mem::take(&mut ident));
                    }
                }
                if !ident.is_empty() {
                    seen.insert(ident);
                }
                for key in seen {
                    map.entry(key).or_default().push((s, l));
                }
            }
        }
        map
    }

    /// `(source, identifier)` for each `const IDENT: &str = "VALUE";` in the crate, keyed by VALUE.
    fn const_definitions(sources: &[Source]) -> HashMap<String, Vec<(usize, String)>> {
        let mut out: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        for (s, source) in sources.iter().enumerate() {
            for line in &source.lines {
                let Some(at) = line.find("const ") else {
                    continue;
                };
                let rest = &line[at + "const ".len()..];
                let Some(colon) = rest.find(':') else {
                    continue;
                };
                let ident = rest[..colon].trim();
                let shaped = !ident.is_empty()
                    && ident
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
                let Some(eq) = rest.find("= \"") else {
                    continue;
                };
                let value_start = eq + "= \"".len();
                let Some(close) = rest[value_start..].find('"') else {
                    continue;
                };
                let after = rest[value_start + close + 1..].trim_start();
                if !shaped || !after.starts_with(';') {
                    continue;
                }
                let value = &rest[value_start..value_start + close];
                out.entry(value.to_owned())
                    .or_default()
                    .push((s, ident.to_owned()));
            }
        }
        out
    }

    /// Whether each production line of `lines` belongs to a `use` statement, which may span lines.
    fn in_use_lines(lines: &[String]) -> Vec<bool> {
        let mut flags = Vec::with_capacity(lines.len());
        let mut inside = false;
        for line in lines {
            let trimmed = line.trim_start();
            if !inside
                && ["use ", "pub use ", "pub(crate) use "]
                    .iter()
                    .any(|start| trimmed.starts_with(start))
            {
                inside = true;
            }
            flags.push(inside);
            if inside && line.trim_end().ends_with(';') {
                inside = false;
            }
        }
        flags
    }

    /// Whether `text` holds `segment` as a whole path segment followed by `::`: `parts::` but not
    /// `vision_parts::`.
    fn names_segment(text: &str, segment: &str) -> bool {
        let path = format!("{segment}::");
        text.match_indices(&path).any(|(at, _)| {
            !text[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    }

    /// The names `source` binds to the constant `ident` of one of `modules` through a `use` statement:
    /// `ident` itself, or the alias of `ident as ALIAS`. A statement may span lines.
    fn names_bound_to(source: &Source, modules: &[String], ident: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut statement = String::new();
        for (l, line) in source.lines.iter().enumerate() {
            if !source.in_use[l] {
                continue;
            }
            statement.push_str(line);
            statement.push(' ');
            if !line.trim_end().ends_with(';') {
                continue;
            }
            let text = std::mem::take(&mut statement);
            if !(modules.iter().any(|module| names_segment(&text, module))
                && mentions(&text, ident))
            {
                continue;
            }
            let alias = text
                .split_once(format!("{ident} as ").as_str())
                .map(|(_, rest)| {
                    rest.chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<String>()
                })
                .filter(|name| !name.is_empty());
            names.push(alias.unwrap_or_else(|| ident.to_owned()));
        }
        names
    }

    /// The reads of the switch `name` in production code, as `(source, line)` pairs: each line that
    /// holds its quoted name, and each line that holds an identifier bound to it. An identifier is
    /// read in its own file, or as a path through a module that declares its file (`module::IDENT`),
    /// or, in a file that imports it, under the name the import binds (`IDENT`, or the alias of
    /// `IDENT as ALIAS`). A definition and a `use` statement are not reads.
    fn reader_sites(
        name: &str,
        sources: &[Source],
        postings: &HashMap<String, Vec<(usize, usize)>>,
        defs: &HashMap<String, Vec<(usize, String)>>,
    ) -> Vec<(usize, usize)> {
        let quoted = format!("\"{name}\"");
        // Each key with its owner and, when set, the one file that binds it with a `use` statement. A key
        // of the owner is read where its module path names it; a bound key is read only in its file.
        let mut keys: Vec<(String, Option<usize>, Option<usize>)> =
            vec![(quoted.clone(), None, None)];
        for (owner, ident) in defs.get(name).map(Vec::as_slice).unwrap_or_default() {
            keys.push((ident.clone(), Some(*owner), None));
            for (file, source) in sources.iter().enumerate() {
                if file == *owner {
                    continue;
                }
                for bound in names_bound_to(source, &sources[*owner].modules, ident) {
                    keys.push((bound, Some(*owner), Some(file)));
                }
            }
        }
        let mut sites = BTreeSet::new();
        for (key, owner, file) in keys {
            for &(s, l) in postings.get(&key).map(Vec::as_slice).unwrap_or_default() {
                let line = sources[s].lines[l].as_str();
                let is_definition =
                    line.contains("const ") && line.contains(&format!("= {quoted};"));
                if sources[s].in_use[l] || is_definition {
                    continue;
                }
                let read = match (owner, file) {
                    (None, _) => true,
                    (Some(_), Some(file)) => s == file,
                    (Some(owner), None) => {
                        s == owner
                            || sources[owner]
                                .modules
                                .iter()
                                .any(|module| mentions(line, &format!("{module}::{key}")))
                    }
                };
                if read {
                    sites.insert((s, l));
                }
            }
        }
        sites.into_iter().collect()
    }

    /// The statement lines per side of a read: a statement ends at a line that ends with `;`, `,`, `{`
    /// or `}`, so a wrapped call, a struct field and a `let` each keep their own lines. Bounded, so a
    /// read is never judged by a long run of neighbours.
    const MAX_STATEMENT_LINES: usize = 12;

    /// The indices of the lines of the statement that holds line `l`: the lines above it back to the
    /// one that ends a statement or a block, and the lines below it down to the first that does.
    fn statement_lines(lines: &[String], l: usize) -> Vec<usize> {
        let ends = |line: &str| {
            let text = line.trim_end();
            [";", ",", "{", "}"].iter().any(|end| text.ends_with(end))
        };
        let mut first = l;
        while first > 0 && l - first < MAX_STATEMENT_LINES && !ends(&lines[first - 1]) {
            first -= 1;
        }
        let mut last = l;
        while last + 1 < lines.len() && last - l < MAX_STATEMENT_LINES && !ends(&lines[last]) {
            last += 1;
        }
        (first..=last).collect()
    }

    /// The name a `let` statement binds, if `line` starts one: `raw` for `let raw = ...` or `let mut raw`.
    fn bound_name(line: &str) -> Option<String> {
        let rest = line.trim_start().strip_prefix("let ")?;
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        (!name.is_empty()).then_some(name)
    }

    /// The line that opens the call a line sits in, when that call wraps the line: the nearest line above
    /// that leaves a parenthesis open, within the bound. An argument of a call (`std::env::var(X)...,` under
    /// `if !enabled(`) is judged with the line that opens the call.
    fn opener_line(lines: &[String], l: usize) -> Option<usize> {
        let mut owed: i64 = 0;
        for j in (l.saturating_sub(MAX_STATEMENT_LINES)..l).rev() {
            let text = &lines[j];
            owed += text.matches(')').count() as i64 - text.matches('(').count() as i64;
            if owed < 0 {
                return Some(j);
            }
        }
        None
    }

    /// The text of the given lines, in order.
    fn text_of(lines: &[String], indices: &BTreeSet<usize>) -> String {
        indices
            .iter()
            .map(|&index| lines[index].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The two texts one read of a switch is checked by. The first is its own statement with the line that
    /// opens the call it sits in. The second adds the statement that first uses the variable the read binds
    /// (`let raw = std::env::var(X).ok();`), through the end of the block. The second is used only when the
    /// first does not vouch for the read, so a later use of the variable cannot widen a read that is exact.
    fn read_texts(lines: &[String], l: usize) -> (String, String) {
        let own = statement_lines(lines, l);
        let mut first_step: BTreeSet<usize> = own.iter().copied().collect();
        if let Some(opener) = opener_line(lines, l) {
            // Only the opening line: its statement would reach the sibling arguments after it.
            first_step.insert(opener);
        }
        let mut second_step = first_step.clone();
        if let (Some(&first), Some(&last)) = (own.first(), own.last()) {
            if let Some(name) = bound_name(&lines[first]) {
                for j in last + 1..lines.len().min(last + 1 + MAX_STATEMENT_LINES) {
                    let text = lines[j].trim();
                    if text == "}" || text.starts_with("fn ") {
                        break;
                    }
                    if mentions(&lines[j], &name) {
                        second_step.extend(statement_lines(lines, j));
                        break;
                    }
                }
            }
        }
        (text_of(lines, &first_step), text_of(lines, &second_step))
    }

    /// The texts one read of a switch is checked by (see `read_texts`), and the name of the function that
    /// holds it (for the report).
    fn read_text(
        sources: &[Source],
        functions: &[Vec<(usize, usize, String)>],
        (s, l): (usize, usize),
    ) -> (String, String, String) {
        let (first, second) = read_texts(&sources[s].lines, l);
        let function = functions[s]
            .iter()
            .filter(|(start, end, _)| *start <= l && l < *end)
            .max_by_key(|(start, _, _)| *start)
            .map_or_else(
                || String::from("(no function)"),
                |(_, _, name)| name.clone(),
            );
        (function, first, second)
    }

    /// Reads that fold, trim or otherwise widen the value: a switch reader may use none of them.
    const LENIENT_READS: &[&str] = &[
        ".trim()",
        "str::trim",
        "to_ascii_lowercase",
        "to_lowercase",
        "to_ascii_uppercase",
        "to_uppercase",
        "eq_ignore_ascii_case",
        "switch_from_any_case",
        "is_explicit_off",
    ];

    /// Whether `text` folds or trims the value, or accepts another spelling of the switch.
    fn widens(text: &str, literal: &str) -> bool {
        LENIENT_READS.iter().any(|read| text.contains(read))
            || ["1", "on", "yes", "enabled", "true", "TRUE", "True", "ON"]
                .into_iter()
                .filter(|word| *word != literal)
                .any(|word| text.contains(&format!("\"{word}\"")))
    }

    /// Whether `text` holds an exact read of `literal`: a comparison with it, a match arm on it, or,
    /// for `true`, the shared exact reader: `switch_from`, or its alias `switch_value_is_on`.
    fn has_exact_read(text: &str, literal: &str) -> bool {
        let mut markers = vec![
            format!("Some(\"{literal}\")"),
            format!("Ok(\"{literal}\")"),
            format!("== \"{literal}\""),
            format!("\"{literal}\" =>"),
        ];
        if literal == "true" {
            markers.push("switch_from(".to_owned());
            markers.push("switch_value_is_on".to_owned());
        }
        markers.iter().any(|marker| text.contains(marker.as_str()))
    }

    /// The names a text calls or names as a path: identifiers that are followed by `(`, and the last
    /// segment of a `::` path, as in `and_then(Self::recognise)`, which passes a function as a value.
    fn callees(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut at = 0;
        while at < chars.len() {
            if !(chars[at].is_alphanumeric() || chars[at] == '_') {
                at += 1;
                continue;
            }
            let start = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                at += 1;
            }
            let after_path = start >= 2 && chars[start - 1] == ':' && chars[start - 2] == ':';
            if after_path || chars.get(at) == Some(&'(') {
                out.push(chars[start..at].iter().collect());
            }
        }
        out
    }

    /// The function bodies of the crate, by name: each with the index of the file that holds it.
    type Bodies = HashMap<String, Vec<(usize, String)>>;

    /// The bodies a name resolves to from `file`: the functions of that name in the same file or, when
    /// the file has none, every function of that name in the crate. A read is exact only when every one
    /// of them is, so a same-named lenient helper elsewhere cannot be passed off by an exact one.
    fn resolve<'a>(bodies: &'a Bodies, file: usize, name: &str) -> Vec<(usize, &'a str)> {
        let Some(all) = bodies.get(name) else {
            return Vec::new();
        };
        let same: Vec<(usize, &str)> = all
            .iter()
            .filter(|(owner, _)| *owner == file)
            .map(|(owner, body)| (*owner, body.as_str()))
            .collect();
        if same.is_empty() {
            all.iter()
                .map(|(owner, body)| (*owner, body.as_str()))
                .collect()
        } else {
            same
        }
    }

    /// Whether some callee of `text` reads `literal` exactly: it resolves to bodies that all do, followed
    /// `depth` levels deep.
    fn callee_reads_exactly(
        bodies: &Bodies,
        file: usize,
        text: &str,
        literal: &str,
        depth: u8,
    ) -> bool {
        callees(text).iter().any(|callee| {
            let targets = resolve(bodies, file, callee);
            !targets.is_empty()
                && targets
                    .iter()
                    .all(|&(owner, body)| helper_reads_exactly(bodies, owner, body, literal, depth))
        })
    }

    /// A helper that a read calls, followed `depth` levels deep: it reads the value exactly, or it calls
    /// one that does.
    fn helper_reads_exactly(
        bodies: &Bodies,
        file: usize,
        body: &str,
        literal: &str,
        depth: u8,
    ) -> bool {
        if widens(body, literal) {
            return false;
        }
        if has_exact_read(body, literal) {
            return true;
        }
        depth > 0 && callee_reads_exactly(bodies, file, body, literal, depth - 1)
    }

    /// Whether one read of a switch is exact. Its statement may not widen the value, and it must hold the
    /// exact read, unless a helper that the statement calls does, followed one more level down. The
    /// enclosing function is not a fallback: another read in it would vouch for this one. A read that
    /// folds, trims or takes another spelling fails.
    fn read_is_exact(bodies: &Bodies, file: usize, statement: &str, literal: &str) -> bool {
        if widens(statement, literal) {
            return false;
        }
        has_exact_read(statement, literal)
            || callee_reads_exactly(bodies, file, statement, literal, 1)
    }

    /// The scan itself: an exact read passes, and a read that folds, trims, takes another spelling or
    /// accepts any value fails, with or without a helper in between.
    #[test]
    fn the_reader_check_tells_an_exact_read_from_a_lenient_or_wider_one() {
        let none: Bodies = HashMap::new();
        assert!(read_is_exact(
            &none,
            0,
            "switch_from(std::env::var(X).ok().as_deref())",
            "true"
        ));
        assert!(read_is_exact(
            &none,
            0,
            "std::env::var(X).as_deref() == Ok(\"1\")",
            "1"
        ));
        assert!(!read_is_exact(
            &none,
            0,
            "std::env::var(X).ok().as_deref() == Some(\"1\")",
            "true"
        ));
        assert!(!read_is_exact(
            &none,
            0,
            "std::env::var(X).ok().map(|v| v.trim().to_owned()).as_deref() == Some(\"true\")",
            "true"
        ));
        assert!(!read_is_exact(
            &none,
            0,
            "switch_from_any_case(std::env::var(X).ok().as_deref())",
            "true"
        ));
        assert!(!read_is_exact(&none, 0, "std::env::var(X).is_ok()", "true"));
        let mut helpers: Bodies = HashMap::new();
        helpers.insert(
            "parse_enabled".to_owned(),
            vec![(
                0,
                "fn parse_enabled(raw: Option<&str>) -> bool { switch_from(raw) }".to_owned(),
            )],
        );
        assert!(read_is_exact(
            &helpers,
            0,
            "parse_enabled(std::env::var(X).ok().as_deref())",
            "true"
        ));
        helpers.insert(
            "parse_enabled".to_owned(),
            vec![(
                0,
                "fn parse_enabled(raw: Option<&str>) -> bool { raw.is_some() }".to_owned(),
            )],
        );
        assert!(!read_is_exact(
            &helpers,
            0,
            "parse_enabled(std::env::var(X).ok().as_deref())",
            "true"
        ));
        // A lenient statement fails, whatever else holds the exact read.
        assert!(!read_is_exact(&none, 0, "let v = raw.trim();", "true"));
    }

    /// A same-named helper is resolved in the file that calls it. A read is exact only when every helper
    /// it can reach is, so a lenient `parse_enabled` in one file is not passed by an exact one in another.
    #[test]
    fn a_helper_is_resolved_in_its_own_file_and_every_candidate_must_be_exact() {
        let exact = "fn parse_enabled(raw: Option<&str>) -> bool { switch_from(raw) }";
        let lenient = "fn parse_enabled(raw: Option<&str>) -> bool { raw.is_some() }";
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "parse_enabled".to_owned(),
            vec![(0, exact.to_owned()), (1, lenient.to_owned())],
        );
        let call = "parse_enabled(std::env::var(X).ok().as_deref())";
        assert!(
            read_is_exact(&bodies, 0, call, "true"),
            "file 0 has its own exact helper"
        );
        assert!(
            !read_is_exact(&bodies, 1, call, "true"),
            "file 1 has the lenient helper"
        );
        assert!(
            !read_is_exact(&bodies, 2, call, "true"),
            "a file with none of its own needs every helper of that name to be exact"
        );
    }

    /// A function passed as a value (`and_then(Self::recognise)`) is followed to its body, as a call is.
    #[test]
    fn a_path_passed_as_a_value_is_followed_to_its_body() {
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "recognise".to_owned(),
            vec![(
                0,
                "fn recognise(value: &str) -> Option<Self> { match value { \"on\" => Some(Self::On), _ => None } }"
                    .to_owned(),
            )],
        );
        assert!(read_is_exact(
            &bodies,
            0,
            "value.and_then(Self::recognise)",
            "on"
        ));
    }

    /// A statement is the lines from the one after a line that ends a statement or block, through the
    /// first line that ends one. A wrapped call is one statement, and a struct field is its own.
    #[test]
    fn statement_lines_keep_a_wrapped_call_whole_and_a_field_on_its_own() {
        let lines: Vec<String> = [
            "Self {",
            "    sandbox: switch_from(get(A).as_deref()),",
            "    vision: switch_from(",
            "        get(B)",
            "            .as_deref(),",
            "    ),",
            "    other: get(C).is_some(),",
            "}",
            "let on = switch_value_is_on;",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        assert_eq!(statement_lines(&lines, 1), vec![1]);
        assert_eq!(statement_lines(&lines, 3), vec![2, 3, 4]);
        assert_eq!(statement_lines(&lines, 6), vec![6]);
        assert_eq!(statement_lines(&lines, 8), vec![8]);
    }

    /// A lenient read is judged by its own statement. An exact read on the line before it, or elsewhere
    /// in its function, does not vouch for it: the scan must fail the lenient read.
    #[test]
    fn an_exact_read_next_to_a_lenient_one_does_not_vouch_for_it() {
        let lines: Vec<String> = [
            "    let a = switch_from(std::env::var(A).ok().as_deref());",
            "    let b = std::env::var(B).is_ok();",
            "    a && b",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let none: Bodies = HashMap::new();
        let statement = |l: usize| {
            statement_lines(&lines, l)
                .into_iter()
                .map(|index| lines[index].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(read_is_exact(&none, 0, &statement(0), "true"));
        assert!(
            !read_is_exact(&none, 0, &statement(1), "true"),
            "the exact read of A on the line before vouched for B"
        );
    }

    /// A read bound to a variable is judged by the first statement that uses the variable, and only when
    /// its own statement does not vouch for it. A later use of the variable cannot widen it.
    #[test]
    fn a_bound_read_is_judged_with_the_first_statement_that_uses_the_variable() {
        let lines: Vec<String> = [
            "    let raw = std::env::var(A).ok();",
            "    let flag = switch_from(raw.as_deref());",
            "    let shown = raw.as_deref().map(str::trim);",
            "    let other = std::env::var(B).is_ok();",
            "    let lenient = std::env::var(C).ok();",
            "    let shown_c = lenient.is_some();",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let none: Bodies = HashMap::new();
        let (own, bound) = read_texts(&lines, 0);
        assert!(!read_is_exact(&none, 0, &own, "true"));
        assert!(read_is_exact(&none, 0, &bound, "true"));
        let (own, bound) = read_texts(&lines, 3);
        assert!(!read_is_exact(&none, 0, &own, "true"));
        assert!(!read_is_exact(&none, 0, &bound, "true"));
        let (own, bound) = read_texts(&lines, 4);
        assert!(!read_is_exact(&none, 0, &own, "true"));
        assert!(!read_is_exact(&none, 0, &bound, "true"));
    }

    /// An argument of a call that wraps it is judged with the line that opens the call, when the argument's
    /// own statement ends at the line above it.
    #[test]
    fn an_argument_of_a_wrapping_call_is_judged_with_the_call() {
        let lines: Vec<String> = [
            "    Self::from_values(",
            "        std::env::var(A).ok().as_deref(),",
            "        std::env::var(B).ok().as_deref(),",
            "    )",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "from_values".to_owned(),
            vec![(
                0,
                "fn from_values(a: Option<&str>, b: Option<&str>) -> bool { let on = switch_value_is_on; on(a) && on(b) }"
                    .to_owned(),
            )],
        );
        assert_eq!(opener_line(&lines, 2), Some(0));
        let (first, _) = read_texts(&lines, 2);
        assert!(read_is_exact(&bodies, 0, &first, "true"));
        assert!(!read_is_exact(
            &bodies,
            0,
            "std::env::var(B).ok().as_deref(),",
            "true"
        ));
    }

    /// A sibling argument's exact read does not vouch for a lenient argument of the same call.
    #[test]
    fn a_sibling_argument_does_not_vouch_for_a_lenient_one() {
        let lines: Vec<String> = [
            "    Self::combine(",
            "        switch_from(std::env::var(A).ok().as_deref()),",
            "        std::env::var(B).is_some(),",
            "    )",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let none: Bodies = HashMap::new();
        let (first, second) = read_texts(&lines, 2);
        assert!(!read_is_exact(&none, 0, &first, "true"));
        assert!(!read_is_exact(&none, 0, &second, "true"));
    }

    /// The scan keys a path on the module name that declares a file, not on the file's stem: a
    /// `parts::ENABLED_VAR` read from another file is seen, and `vision_parts::ENABLED_VAR` is not. A
    /// file that imports the constant, under its name or an alias, reads it through that name; a file
    /// that does not import it does not read a bare name.
    #[test]
    fn a_read_through_a_module_path_or_an_import_is_seen_and_an_unimported_name_is_not() {
        let source = |file: &str, lines: &[&str]| {
            let lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
            Source {
                file: file.to_owned(),
                modules: Vec::new(),
                in_use: in_use_lines(&lines),
                numbers: (1..=lines.len()).collect(),
                lines,
            }
        };
        let mut sources = vec![
            source(
                "vision_parts.rs",
                &["pub const ENABLED_VAR: &str = \"CHAT_VISION_AS_PARTS_ENABLED\";"],
            ),
            source(
                "vision.rs",
                &["#[path = \"vision_parts.rs\"]", "pub mod parts;"],
            ),
            source(
                "reader.rs",
                &[
                    "fn on() -> bool { switch_from(std::env::var(parts::ENABLED_VAR).ok().as_deref()) }",
                    "fn stem() -> bool { switch_from(std::env::var(vision_parts::ENABLED_VAR).ok().as_deref()) }",
                ],
            ),
            source(
                "alias.rs",
                &[
                    "use crate::vision::{parts::ENABLED_VAR as VISION_AS_PARTS_VAR, Other};",
                    "fn aliased() -> bool { switch_from(get(VISION_AS_PARTS_VAR).as_deref()) }",
                    "fn bare_unimported() -> bool { switch_from(std::env::var(ENABLED_VAR).ok().as_deref()) }",
                ],
            ),
        ];
        let modules: Vec<Vec<String>> = (0..sources.len())
            .map(|owner| module_names_of(&sources, owner))
            .collect();
        for (source, names) in sources.iter_mut().zip(modules) {
            source.modules = names;
        }
        assert_eq!(sources[0].modules, vec!["parts".to_owned()]);
        let postings = postings(&sources);
        let defs = const_definitions(&sources);
        assert_eq!(
            reader_sites("CHAT_VISION_AS_PARTS_ENABLED", &sources, &postings, &defs),
            vec![(2, 0), (3, 1)]
        );
    }

    /// A `#[cfg(test)]` module's file is test code, and a `#[cfg(test)]` item followed by another
    /// attribute still drops only its own item, so the production lines after it are kept.
    #[test]
    fn a_test_only_file_is_not_production_and_a_gated_item_keeps_the_lines_after_it() {
        let raw = vec![
            (
                "owner.rs".to_owned(),
                "#[cfg(test)]\n#[path = \"owner_tests.rs\"]\nmod owner_tests;\nfn kept() {}\n"
                    .to_owned(),
            ),
            ("owner_tests.rs".to_owned(), "fn t() {}".to_owned()),
        ];
        assert_eq!(test_only_files(&raw), vec!["owner_tests.rs".to_owned()]);
        let kept: Vec<String> = production_lines(&raw[0].1)
            .into_iter()
            .map(|(_, line)| line)
            .collect();
        assert_eq!(kept, vec!["fn kept() {}".to_owned()]);
    }

    /// Each documented exception is named by production code and is not also an exact switch.
    #[test]
    fn every_documented_exception_is_read_by_production_code_and_is_not_also_listed() {
        let sources = production_sources();
        for (name, reason) in DOCUMENTED_EXCEPTIONS {
            assert!(!reason.is_empty(), "{name} has no reason");
            assert!(
                EXACT_SWITCHES
                    .iter()
                    .copied()
                    .all(|(listed, _)| listed != *name),
                "{name} is listed as exact and as an exception"
            );
            assert!(
                sources
                    .iter()
                    .any(|source| source.lines.iter().any(|line| mentions(line, name))),
                "{name} is not read by production code"
            );
        }
    }

    /// Whether `line` names `word` as a whole identifier, or as a quoted literal.
    fn mentions(line: &str, word: &str) -> bool {
        let is_ident = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        line.match_indices(word).any(|(at, _)| {
            !is_ident(line[..at].chars().next_back())
                && !is_ident(line[at + word.len()..].chars().next())
        })
    }

    /// The exact literal of every `true` entry is the one `switch_from` takes, and nothing else is.
    #[test]
    fn the_true_entries_are_the_literal_the_shared_reader_takes() {
        for (name, literal) in EXACT_SWITCHES.iter().copied() {
            if literal == "true" {
                assert!(
                    mp_contracts::loop_fold::switch_from(Some(literal)),
                    "{name} is listed as true, and switch_from does not take it"
                );
            }
        }
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
    }
}
