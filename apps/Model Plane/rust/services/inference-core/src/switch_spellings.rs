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

    /// The reads the scan accepts without judging the calls around them, each with its reason: `(file, needle,
    /// reason)`, where the needle is a text of the reading line. Each read's verdict is taken by an outer call after
    /// an exact reader or comparison on the same line, or it reaches an exact reader through a helper that only
    /// drops an empty value, so a helper the scan finds around it is not what judges it.
    const PASS_THROUGH_READS: &[(&str, &str, &str)] = &[(
        "provider/speech.rs",
        "non_eu_tts_opted_in(env_nonempty(NON_EU_TTS_ENV)",
        "env_nonempty drops an empty value and passes the rest on unchanged to the exact comparison",
    )];

    /// Whether the read at `lines[l]` of `source` is one of `PASS_THROUGH_READS`.
    fn pass_through_read(source: &Source, l: usize) -> bool {
        PASS_THROUGH_READS
            .iter()
            .any(|(file, needle, _)| source.file == *file && source.lines[l].contains(needle))
    }

    /// Each pass-through read still occurs in production code, and says why it is allowed.
    #[test]
    fn every_pass_through_read_still_occurs_in_production_code() {
        let sources = production_sources();
        for (file, needle, reason) in PASS_THROUGH_READS {
            assert!(!reason.is_empty(), "{file}: {needle} has no reason");
            assert!(
                sources.iter().any(|source| source.file == *file
                    && source.lines.iter().any(|line| line.contains(needle))),
                "{needle} is no longer in production code of {file}"
            );
        }
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
                bodies.entry(name.clone()).or_default().push((
                    s,
                    is_method(&source.lines, *start),
                    source.lines[*start..*end].join("\n"),
                ));
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
            let ids: Vec<String> = switch_keys(name, &sources, &defs)
                .into_iter()
                .map(|(key, _, _)| key)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            for (s, l) in sites {
                if pass_through_read(&sources[s], l) {
                    continue;
                }
                let (function, first, second) = read_text(&sources, &functions, (s, l));
                let scope = enclosing_scope(&sources[s], &functions[s], l);
                let bound: Vec<String> = bound_variable(&sources[s].lines, l).into_iter().collect();
                if !read_is_exact_in(&bodies, s, &first, literal, &scope, &ids)
                    && !read_is_exact_in(&bodies, s, &second, literal, &scope, &bound)
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
        /// The file's path under `src`, with `/` between directories, such as `sse.rs` or `provider/mod.rs`.
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
        fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).expect("the crate's source directory is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    walk(root, &path, out);
                    continue;
                }
                let file = source_key(root, &path);
                let is_rust = path.extension().and_then(|ext| ext.to_str()) == Some("rs");
                if !is_rust || file == "switch_spellings.rs" {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("a source file is readable");
                out.push((file, text.replace("\r\n", "\n")));
            }
        }
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut raw = Vec::new();
        walk(&src, &src, &mut raw);
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
        let modules = modules_of_all(&out);
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

    /// A source file's key: its path under `src`, with `/` between the directories (`provider/mod.rs`).
    fn source_key(root: &std::path::Path, path: &std::path::Path) -> String {
        path.strip_prefix(root)
            .expect("a source file under src")
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// The path a `#[path = "..."]` attribute names, as written (with `/` between directories), if `line` is one.
    fn path_attribute(line: &str) -> Option<String> {
        let rest = line.trim_start().strip_prefix("#[path")?;
        let start = rest.find('"')? + 1;
        let end = start + rest[start..].find('"')?;
        Some(rest[start..end].replace('\\', "/"))
    }

    /// `base` and `part` joined with `/`, with `.` and `..` resolved, as rustc resolves a `#[path]`.
    fn join_key(base: &str, part: &str) -> String {
        let mut parts: Vec<&str> = base.split('/').filter(|step| !step.is_empty()).collect();
        for step in part.split('/') {
            match step {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        parts.join("/")
    }

    /// The name of the inline module (`mod NAME {`, with any visibility) that `trimmed` opens, if it does.
    fn inline_module_name(trimmed: &str) -> Option<String> {
        let rest = trimmed.strip_suffix('{')?.trim_end();
        let rest = rest
            .strip_prefix("pub(crate) ")
            .or_else(|| rest.strip_prefix("pub "))
            .unwrap_or(rest);
        let name = rest.strip_prefix("mod ")?.trim();
        (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
            .then(|| name.to_owned())
    }

    /// The names of the inline modules that enclose `lines[l]`, outermost first. A block ends at the closing
    /// brace at its opening line's own indentation, which rustfmt keeps.
    fn inline_modules_at<S: AsRef<str>>(lines: &[S], l: usize) -> Vec<String> {
        let mut open: Vec<(usize, String)> = Vec::new();
        for line in lines.iter().take(l) {
            let line = line.as_ref();
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if trimmed.starts_with('}') && open.last().is_some_and(|(at, _)| *at == indent) {
                open.pop();
            } else if let Some(name) = inline_module_name(trimmed) {
                open.push((indent, name));
            }
        }
        open.into_iter().map(|(_, name)| name).collect()
    }

    /// The keys of the files that the `mod NAME;` at `lines[l]` of the source `owner` may load, and whether a
    /// `#[cfg(test)]` attribute above it gates the declaration. Resolved as rustc resolves it. A `#[path]` on a
    /// module outside any inline module is relative to the directory of the declaring file. Otherwise the module's
    /// directory is the declaring file's, with the file's own name as a further directory unless the file is a
    /// `mod.rs` or a crate root, and each inline module enclosing the declaration is a further directory. A module
    /// without a `#[path]` loads `NAME.rs` or `NAME/mod.rs` there.
    fn declared_files<S: AsRef<str>>(
        owner: &str,
        lines: &[S],
        l: usize,
        name: &str,
    ) -> (Vec<String>, bool) {
        let mut path = None;
        let mut gated = false;
        let mut above = l;
        while above > 0 && lines[above - 1].as_ref().trim_start().starts_with("#[") {
            above -= 1;
            gated |= lines[above].as_ref().trim() == "#[cfg(test)]";
            if let Some(written) = path_attribute(lines[above].as_ref()) {
                path = Some(written);
            }
        }
        let inline = inline_modules_at(lines, l);
        let (dir, file) = owner.rsplit_once('/').unwrap_or(("", owner));
        let mod_rs = matches!(file, "mod.rs" | "lib.rs" | "main.rs");
        let base = if inline.is_empty() && path.is_some() {
            dir.to_owned()
        } else {
            let own = if mod_rs {
                dir.to_owned()
            } else {
                join_key(dir, file.strip_suffix(".rs").unwrap_or(file))
            };
            inline
                .iter()
                .fold(own, |base, module| join_key(&base, module))
        };
        let files = match path {
            Some(written) => vec![join_key(&base, &written)],
            None => vec![
                join_key(&base, &format!("{name}.rs")),
                join_key(&base, &format!("{name}/mod.rs")),
            ],
        };
        (files, gated)
    }

    /// The keys of the files that a `#[cfg(test)]` module declaration loads. Those are test code, not production.
    fn test_only_files(raw: &[(String, String)]) -> Vec<String> {
        let mut out = Vec::new();
        for (owner, text) in raw {
            let lines: Vec<&str> = text.lines().collect();
            for (l, line) in lines.iter().enumerate() {
                let Some(name) = declared_module(line) else {
                    continue;
                };
                let (files, gated) = declared_files(owner, &lines, l, name);
                if gated {
                    out.extend(files);
                }
            }
        }
        out
    }

    /// The module names of every source at once, one list per source: each `mod NAME;` adds its name to the
    /// file or files it may load (see `declared_files`), so a file's list is the names that declare it.
    fn modules_of_all(sources: &[Source]) -> Vec<Vec<String>> {
        let index: HashMap<&str, usize> = sources
            .iter()
            .enumerate()
            .map(|(s, source)| (source.file.as_str(), s))
            .collect();
        let mut out = vec![Vec::new(); sources.len()];
        for source in sources {
            for (l, line) in source.lines.iter().enumerate() {
                let Some(name) = declared_module(line) else {
                    continue;
                };
                let (files, _) = declared_files(&source.file, &source.lines, l, name);
                for file in files {
                    if let Some(&owner) = index.get(file.as_str()) {
                        out[owner].push(name.to_owned());
                    }
                }
            }
        }
        out
    }

    /// The module names that declare `sources[owner]`: each `mod NAME;` that loads that file. So `pub mod
    /// parts;` under `#[path = "vision_parts.rs"]` gives `parts`, and not the file stem.
    fn module_names_of(sources: &[Source], owner: usize) -> Vec<String> {
        let file = sources[owner].file.as_str();
        let mut names = Vec::new();
        for source in sources {
            for (l, line) in source.lines.iter().enumerate() {
                let Some(name) = declared_module(line) else {
                    continue;
                };
                let (files, _) = declared_files(&source.file, &source.lines, l, name);
                if files.iter().any(|candidate| candidate == file) {
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

    /// The names a read of the switch `name` goes by: its quoted name, each constant bound to it, with the
    /// file that owns the constant, and each name a `use` statement binds to such a constant, with the one file
    /// that binds it. A key of the owner is read where its module path names it; a bound key is read only in its
    /// file. A read is judged by the clauses that hold one of these names (see `vouches_for`).
    fn switch_keys(
        name: &str,
        sources: &[Source],
        defs: &HashMap<String, Vec<(usize, String)>>,
    ) -> Vec<(String, Option<usize>, Option<usize>)> {
        let mut keys: Vec<(String, Option<usize>, Option<usize>)> =
            vec![(format!("\"{name}\""), None, None)];
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
        keys
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
        let mut sites = BTreeSet::new();
        for (key, owner, file) in switch_keys(name, sources, defs) {
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

    /// The variable that the statement holding `lines[l]` binds with `let`, if it binds one. The second text of
    /// `read_texts` is judged by the uses of this name, not by the switch's own names, so an exact read of another
    /// value in the statement that first uses the variable vouches for nothing.
    fn bound_variable(lines: &[String], l: usize) -> Option<String> {
        statement_lines(lines, l)
            .first()
            .and_then(|&first| bound_name(&lines[first]))
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

    /// Reads that fold, trim or otherwise widen the value: a switch reader may use none of them. It is a list, not a
    /// rule, so a widening it does not name passes: `switch_from(std::env::var(X).ok().map(|v| v.replace(' ', "")).as_deref())`
    /// strips spaces and is accepted. A new kind of widening needs its own entry here.
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

    /// The names a text puts a value through: the names it calls with at least one argument, and the last segment of
    /// a `::` path it passes on as a value (`and_then(Self::recognise)`). A zero-argument call (`enabled()`) is left
    /// out, since it cannot receive the value of a read; a read inside it is judged on its own line. The name that a
    /// `fn` declares is not a call, so a helper's own header does not make it call itself.
    fn callees_on_value_path(text: &str) -> Vec<String> {
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
            let called = chars.get(at) == Some(&'(');
            let no_argument =
                called && chars[at + 1..].iter().find(|c| !c.is_whitespace()) == Some(&')');
            if (after_path || called) && !no_argument && !follows_fn_keyword(&chars, start) {
                out.push(chars[start..at].iter().collect());
            }
        }
        out
    }

    /// Whether the identifier that starts at `chars[start]` is the name a `fn` declares (`fn NAME`).
    fn follows_fn_keyword(chars: &[char], start: usize) -> bool {
        let mut before = start;
        while before > 0 && chars[before - 1].is_whitespace() {
            before -= 1;
        }
        before >= 2
            && chars[before - 2] == 'f'
            && chars[before - 1] == 'n'
            && (before == 2 || !(chars[before - 3].is_alphanumeric() || chars[before - 3] == '_'))
    }

    /// The function bodies of the crate, by name: each with the index of the file that holds it, whether it is a
    /// method (see `is_method`), and its text.
    type Bodies = HashMap<String, Vec<(usize, bool, String)>>;

    /// Whether the function whose header is `lines[start]` is a method, declared inside an `impl` block. Its enclosing
    /// block is the nearest line above it with a smaller indentation that is not a wrapped header's continuation (a
    /// `{` or a `where` line), and that line opens the `impl`. A free function of an inline module is no method, and
    /// neither is a top-level one.
    fn is_method(lines: &[String], start: usize) -> bool {
        let indent = indentation(&lines[start]);
        (0..start)
            .rev()
            .filter(|&above| !lines[above].trim().is_empty() && indentation(&lines[above]) < indent)
            .find(|&above| {
                let header = lines[above].trim_start();
                !(header.starts_with('{') || header == "where" || header.starts_with("where "))
            })
            .is_some_and(|above| {
                let header = lines[above].trim_start();
                header.starts_with("impl ")
                    || header.starts_with("impl<")
                    || header.starts_with("unsafe impl")
            })
    }

    /// The width of a line's leading whitespace.
    fn indentation(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// The bodies a name resolves to from `file`: the functions of that name in the same file, methods included, or,
    /// when the file has none, every free function of that name in the crate, including one in an inline module of
    /// another file. A method of that name elsewhere (a `get` on some registry) is not what a bare call names, so a
    /// closure's `get(X)` is no helper. A read is exact only when every resolved body is, so a same-named lenient
    /// helper cannot be passed off by an exact one.
    fn resolve<'a>(bodies: &'a Bodies, file: usize, name: &str) -> Vec<(usize, &'a str)> {
        let Some(all) = bodies.get(name) else {
            return Vec::new();
        };
        let same: Vec<(usize, &str)> = all
            .iter()
            .filter(|(owner, _, _)| *owner == file)
            .map(|(owner, _, body)| (*owner, body.as_str()))
            .collect();
        if same.is_empty() {
            all.iter()
                .filter(|(_, method, _)| !method)
                .map(|(owner, _, body)| (*owner, body.as_str()))
                .collect()
        } else {
            same
        }
    }

    /// The crate functions `text` calls, each as the bodies its name resolves to from `file`. A call that names
    /// no crate function (a std accessor such as `var` or `ok`, or the exact reader `switch_from`) is not a helper,
    /// and neither is a call to a name in `scope`: a parameter or a local closure of the function, such as the
    /// `lookup(X)` of a `lookup: &dyn Fn(&str) -> ...` parameter, whatever crate function shares its name.
    fn crate_helpers<'a>(
        bodies: &'a Bodies,
        file: usize,
        names: &[String],
        scope: &[String],
    ) -> Vec<Vec<(usize, &'a str)>> {
        names
            .iter()
            .filter(|name| !scope.contains(name))
            .map(|name| resolve(bodies, file, name))
            .filter(|targets| !targets.is_empty())
            .collect()
    }

    /// Whether every crate helper that `text` puts the value through reads `literal` exactly: each name resolves to
    /// bodies that all do, followed `depth` levels deep (see `callees_on_value_path`). One helper that folds the value
    /// fails the text, even beside a helper that reads it exactly, since the value passes through both.
    fn callee_reads_exactly(
        bodies: &Bodies,
        file: usize,
        text: &str,
        literal: &str,
        depth: u8,
        scope: &[String],
    ) -> bool {
        crate_helpers(bodies, file, &callees_on_value_path(text), scope)
            .iter()
            .all(|targets| {
                targets
                    .iter()
                    .all(|&(owner, body)| helper_reads_exactly(bodies, owner, body, literal, depth))
            })
    }

    /// A helper that a read puts the value through reads `literal` exactly: its body does not widen the value, every
    /// place its body names a parameter is vouched for (see `params_vouched`), and every crate helper its body puts
    /// the value through reads it exactly too, `depth` levels down (`all`, as for the read itself). A helper that
    /// calls no crate helper must hold the exact read itself. A helper's own exact read does not excuse its other
    /// callees: `parse_both(raw)` that reads `switch_from(raw) || lenient(raw)` fails on `lenient`. Nor does it excuse
    /// a presence check on its own value parameter: `switch_from(raw) || raw.is_some()` fails on `raw.is_some()`.
    /// At depth zero a helper that still puts the value through a crate helper fails, since that helper is not read.
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
        let scope = scope_names(body);
        if !params_vouched(bodies, file, body, literal, &scope) {
            return false;
        }
        if crate_helpers(bodies, file, &callees_on_value_path(body), &scope).is_empty() {
            return has_exact_read(body, literal);
        }
        depth > 0 && callee_reads_exactly(bodies, file, body, literal, depth - 1, &scope)
    }

    /// Whether every place a helper's body names one of its parameters is vouched for (see `vouches_for`), and the
    /// body names one at all, so a helper that never reads its value fails. The places are those in the block after
    /// the body's opening brace, so the parameter list is none of them. A `_` parameter cannot be named, so it is
    /// not checked.
    fn params_vouched(
        bodies: &Bodies,
        file: usize,
        body: &str,
        literal: &str,
        scope: &[String],
    ) -> bool {
        let block = body.split_once('{').map_or("", |(_, block)| block);
        let params: Vec<String> = param_names(body)
            .into_iter()
            .filter(|name| name != "_")
            .collect();
        vouches_for(bodies, file, block, literal, scope, &params)
    }

    /// Whether one read of a switch is exact, judged within the function it sits in (`scope`, see `scope_names`).
    /// The statement may not widen the value, and every helper it puts the value through must read it exactly, so an
    /// exact read beside a lenient getter vouches for nothing: `switch_from(lenient(X))`, and a `guard(raw, exact(X))`
    /// whose `guard` does not read the value, both fail. The read itself must be vouched for in its own clause (see
    /// `vouches_for`): each place the statement names it (`ids`: the switch's names, or on the bound path the
    /// variable the read binds) sits in an exact read of that value, or is an argument of a crate helper that reads
    /// exactly. An exact read of another value in the same statement vouches for nothing. A zero-argument call is no
    /// helper on the value's path (see `callees_on_value_path`), and a name the scope binds (a parameter or a local
    /// closure) is no crate function. The enclosing function is never a fallback: another read in it would vouch for
    /// this one.
    fn read_is_exact_in(
        bodies: &Bodies,
        file: usize,
        statement: &str,
        literal: &str,
        scope: &[String],
        ids: &[String],
    ) -> bool {
        let direct = crate_helpers(bodies, file, &callees_on_value_path(statement), scope);
        if widens(statement, literal)
            || direct
                .iter()
                .flatten()
                .any(|&(_, body)| widens(body, literal))
        {
            return false;
        }
        callee_reads_exactly(bodies, file, statement, literal, 1, scope)
            && vouches_for(bodies, file, statement, literal, scope, ids)
    }

    /// Whether one read of the switch `X`, the name the unit tests give it, is exact, with no enclosing function to
    /// judge it in (see `read_is_exact_in`).
    fn read_is_exact(bodies: &Bodies, file: usize, statement: &str, literal: &str) -> bool {
        read_of_is_exact(bodies, file, statement, literal, "X")
    }

    /// Whether the read of `id` in `statement` is exact, with no enclosing function (see `read_is_exact_in`).
    fn read_of_is_exact(
        bodies: &Bodies,
        file: usize,
        statement: &str,
        literal: &str,
        id: &str,
    ) -> bool {
        read_is_exact_in(bodies, file, statement, literal, &[], &[id.to_owned()])
    }

    /// Whether every place `text` names one of `ids` is vouched for, and there is at least one. A place is vouched
    /// for when its clause (see `clause_bounds`) holds an exact read bound to it (see `binds_exact_read`), or when it
    /// is an argument of a crate helper, or passed along its own chain to one (`.and_then(Self::recognise)`), or an
    /// argument of a local alias of the reader (see `reader_aliases`). That the helpers read exactly is
    /// `callee_reads_exactly`'s part. A struct field label names no place (see `field_label`). A place that a presence
    /// check follows in its own chain (see `presence_on_place`) is never vouched for, whatever it sits beside or is
    /// passed to: `exact(raw.is_some().then_some("true"))` reads `raw` by presence. Any other place's chain stops at
    /// the first `==` or `!=` (see `operand_end`), and an alias vouches only for the calls it is an argument of.
    fn vouches_for(
        bodies: &Bodies,
        file: usize,
        text: &str,
        literal: &str,
        scope: &[String],
        ids: &[String],
    ) -> bool {
        let (chars, mask) = masked(text);
        let aliases = reader_aliases(text, literal);
        let places: Vec<(usize, usize)> = ids
            .iter()
            .flat_map(|id| occurrences(&chars, &mask, id))
            .filter(|&(_, end)| !field_label(&chars, &mask, end))
            .collect();
        !places.is_empty()
            && places.iter().all(|&(start, end)| {
                if presence_on_place(&chars, &mask, end) {
                    return false;
                }
                let enclosing = enclosing_callees(&chars, &mask, start);
                let (lo, hi, _) = clause_bounds(&chars, &mask, start, end);
                if binds_exact_read(&chars, &mask, (lo, hi), (start, end), literal) {
                    return true;
                }
                let stop = operand_end(&chars, &mask, end, hi);
                let mut names = enclosing.clone();
                names.extend(callees_on_value_path(
                    &chars[end..stop].iter().collect::<String>(),
                ));
                !crate_helpers(bodies, file, &names, scope).is_empty()
                    || enclosing.iter().any(|name| aliases.contains(name))
            })
    }

    /// Whether the identifier that ends at `chars[end]` is a struct field label (`name: value`), not a use of it: an
    /// optional run of whitespace, then one `:` that is not the first half of a `::`.
    fn field_label(chars: &[char], mask: &[bool], end: usize) -> bool {
        let mut at = end;
        while at < chars.len() && chars[at].is_whitespace() {
            at += 1;
        }
        chars.get(at) == Some(&':') && chars.get(at + 1) != Some(&':') && !mask[at]
    }

    /// Whether the place is followed, in its own operand chain, by a presence check (`.is_some()` and kin). The chain
    /// may run over spaces, masked text and line breaks, as rustfmt lays out a long one, and through the calls in it:
    /// `raw.map(read).is_some()` is a presence check on the place too.
    fn presence_on_place(chars: &[char], mask: &[bool], end: usize) -> bool {
        let mut at = end;
        loop {
            while at < chars.len() && (mask[at] || chars[at].is_whitespace()) {
                at += 1;
            }
            if chars.get(at) != Some(&'.') {
                return false;
            }
            let start = at + 1;
            let mut stop = start;
            while stop < chars.len() && (chars[stop].is_alphanumeric() || chars[stop] == '_') {
                stop += 1;
            }
            let name: String = chars[start..stop].iter().collect();
            if matches!(name.as_str(), "is_some" | "is_none" | "is_ok" | "is_err") {
                return true;
            }
            at = if chars.get(stop) == Some(&'(') {
                closing_paren(chars, mask, stop) + 1
            } else {
                stop
            };
        }
    }

    /// The index where the operand that `chars[from..hi]` runs on ends: the first `==` or `!=` outside masked text, or
    /// `hi`. The other side of a comparison is another operand, so `raw.is_some() == on(raw)` leaves `on` out of the
    /// chain of its place.
    fn operand_end(chars: &[char], mask: &[bool], from: usize, hi: usize) -> usize {
        (from..hi)
            .find(|&at| {
                !mask[at] && matches!(chars[at], '=' | '!') && chars.get(at + 1) == Some(&'=')
            })
            .unwrap_or(hi)
    }

    /// The names `text` binds to the shared exact reader (`let on = switch_value_is_on;`), so that a call of one of
    /// them reads exactly, as the reader does. Only for `true`, the literal the shared reader takes. A binding of the
    /// reader's result (`let flag = switch_from(raw);`) is no alias of it.
    fn reader_aliases(text: &str, literal: &str) -> Vec<String> {
        if literal != "true" {
            return Vec::new();
        }
        text.split("let ")
            .skip(1)
            .filter_map(|piece| {
                let piece = piece.trim_start();
                let piece = piece.strip_prefix("mut ").unwrap_or(piece).trim_start();
                let name: String = piece
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let value = piece[name.len()..]
                    .trim_start()
                    .strip_prefix('=')?
                    .trim_start();
                let path: String = value
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
                    .collect();
                let last = path.rsplit("::").next()?;
                let ends = value[path.len()..].trim_start();
                let is_reader = matches!(last, "switch_from" | "switch_value_is_on");
                (!name.is_empty() && is_reader && (ends.is_empty() || ends.starts_with(';')))
                    .then_some(name)
            })
            .collect()
    }

    /// The characters of `text`, with a mask of those inside a string or char literal or a trailing comment. The
    /// clause scan passes over masked characters, so a parenthesis, a comma or a `||` in a message does not cut or
    /// join a clause.
    fn masked(text: &str) -> (Vec<char>, Vec<bool>) {
        let chars: Vec<char> = text.chars().collect();
        let mut mask = vec![false; chars.len()];
        let mut at = 0;
        let mut in_string = false;
        while at < chars.len() {
            let c = chars[at];
            if in_string {
                mask[at] = true;
                if c == '\\' && at + 1 < chars.len() {
                    mask[at + 1] = true;
                    at += 2;
                    continue;
                }
                in_string = c != '"';
            } else if c == '"' {
                mask[at] = true;
                in_string = true;
            } else if c == '\'' && chars.get(at + 2) == Some(&'\'') {
                mask[at..at + 3].fill(true);
                at += 3;
                continue;
            } else if c == '\''
                && chars.get(at + 1) == Some(&'\\')
                && chars.get(at + 3) == Some(&'\'')
            {
                mask[at..at + 4].fill(true);
                at += 4;
                continue;
            } else if c == '/' && chars.get(at + 1) == Some(&'/') {
                while at < chars.len() && chars[at] != '\n' {
                    mask[at] = true;
                    at += 1;
                }
                continue;
            }
            at += 1;
        }
        (chars, mask)
    }

    /// Where `id` occurs in `chars`, as `[start, end)` ranges: as a whole identifier outside masked text, or, for a
    /// quoted key (`"NAME"`), as that literal. The name a `let` binds (`raw` in `let raw = ...`) is no use of it.
    fn occurrences(chars: &[char], mask: &[bool], id: &str) -> Vec<(usize, usize)> {
        let word: Vec<char> = id.chars().collect();
        let quoted = word.first() == Some(&'"');
        let is_ident = |c: &char| c.is_alphanumeric() || *c == '_';
        if word.is_empty() || word.len() > chars.len() {
            return Vec::new();
        }
        (0..=chars.len() - word.len())
            .map(|start| (start, start + word.len()))
            .filter(|&(start, end)| {
                chars[start..end] == word[..]
                    && (quoted
                        || (!mask[start]
                            && !start
                                .checked_sub(1)
                                .is_some_and(|before| is_ident(&chars[before]))
                            && !chars.get(end).is_some_and(is_ident)
                            && !declared_by_let(chars, start)))
            })
            .collect()
    }

    /// Whether the identifier at `chars[start]` is the name a `let` or `let mut` binds.
    fn declared_by_let(chars: &[char], start: usize) -> bool {
        let before: String = chars[..start].iter().collect();
        let words: Vec<&str> = before.split_whitespace().rev().take(2).collect();
        match words.as_slice() {
            ["let", ..] => true,
            ["mut", "let"] => true,
            _ => false,
        }
    }

    /// The clause of the read at `chars[start..end]`: the expression around it, out through every call it is an
    /// argument of, up to the `&&`, `||`, `,` or `;` at its own level, or the block it sits in. A sibling argument and
    /// the other side of a boolean operator are other clauses. A `matches!` keeps its pattern with its value. The
    /// right side leaves no more parentheses than the left side crossed, so the call a sibling argument sits in is
    /// not part of its clause. Returns the bounds and the open parentheses crossed on the left, innermost first.
    fn clause_bounds(
        chars: &[char],
        mask: &[bool],
        start: usize,
        end: usize,
    ) -> (usize, usize, Vec<usize>) {
        let mut lo = start;
        let mut depth = 0usize;
        let mut crossed = Vec::new();
        while lo > 0 {
            let at = lo - 1;
            if !mask[at] {
                match chars[at] {
                    ')' | ']' | '}' => depth += 1,
                    '(' | '[' if depth == 0 => crossed.push(at),
                    '(' | '[' | '{' if depth > 0 => depth -= 1,
                    '{' => break,
                    ';' | ',' if depth == 0 => break,
                    c @ ('&' | '|') if depth == 0 && at > 0 && chars[at - 1] == c => break,
                    _ => {}
                }
            }
            lo = at;
        }
        let mut hi = end;
        let mut depth = 0usize;
        let mut level = 0usize;
        while hi < chars.len() {
            if !mask[hi] {
                match chars[hi] {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' if depth > 0 => depth -= 1,
                    ')' | ']' if level < crossed.len() => level += 1,
                    ')' | ']' | '}' => break,
                    ';' if depth == 0 => break,
                    ',' if depth == 0 => {
                        if !crossed
                            .get(level)
                            .is_some_and(|&open| opens_matches(chars, open))
                        {
                            break;
                        }
                    }
                    c @ ('&' | '|') if depth == 0 && chars.get(hi + 1) == Some(&c) => break,
                    _ => {}
                }
            }
            hi += 1;
        }
        (lo, hi, crossed)
    }

    /// Whether the parenthesis at `chars[open]` opens a `matches!` call.
    fn opens_matches(chars: &[char], open: usize) -> bool {
        let name = "matches!";
        open >= name.len()
            && chars[open - name.len()..open]
                .iter()
                .copied()
                .eq(name.chars())
            && !(open > name.len()
                && (chars[open - name.len() - 1].is_alphanumeric()
                    || chars[open - name.len() - 1] == '_'))
    }

    /// The names of the calls the read at `chars[start]` is an argument of, innermost first, out to the block or
    /// statement it sits in. A sibling argument of the read is passed over, so the call that takes both is found.
    fn enclosing_callees(chars: &[char], mask: &[bool], start: usize) -> Vec<String> {
        let mut names = Vec::new();
        let mut depth = 0usize;
        let mut at = start;
        while at > 0 {
            at -= 1;
            if mask[at] {
                continue;
            }
            match chars[at] {
                ')' | ']' | '}' => depth += 1,
                '(' | '[' | '{' if depth > 0 => depth -= 1,
                '(' => {
                    let name: String = chars[..at]
                        .iter()
                        .rev()
                        .take_while(|c| c.is_alphanumeric() || **c == '_')
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    if !name.is_empty() {
                        names.push(name);
                    }
                }
                '{' => break,
                ';' if depth == 0 => break,
                _ => {}
            }
        }
        names
    }

    /// Whether the clause `chars[lo..hi]` of the read at `chars[at..until]` holds an exact read of that value. An
    /// exact value (see `exact_values`) vouches for the place only when it is the other whole side of the place's own
    /// `==` or `!=` (see `own_comparison`), parentheses aside, in either order, or an arm pattern of a `match` or
    /// `matches!` that the place is the scrutinee of (see `scrutinee_of_exact_pattern`). One elsewhere in the clause
    /// vouches for nothing: `(raw).is_some() == (raw == Some("true"))` reads its first `raw` by presence. The place's
    /// own chain may also hand its value to an exact closure (see `chain_reads_by_closure`: `.is_some_and(|value|
    /// value == "true")`). For `true`, a `switch_from` or `switch_value_is_on` call vouches for a place its argument
    /// holds in value position (see `in_value_position`), so `switch_from(if raw.map_or(false, |_| true) {
    /// Some("true") } else { None })` does not vouch for `raw`, and the reader vouches for a place whose own chain
    /// passes its value to it (see `chain_passes_to_reader`: `.map(switch_value_is_on)`). The clause holds only the
    /// read's own operand chain (see `clause_bounds`), so `switch_from(a) || b.is_ok()` does not vouch for `b`.
    fn binds_exact_read(
        chars: &[char],
        mask: &[bool],
        (lo, hi): (usize, usize),
        (at, until): (usize, usize),
        literal: &str,
    ) -> bool {
        let exact = exact_values(literal);
        if own_comparison(chars, mask, (lo, hi), at)
            .is_some_and(|other| is_exact_value(chars, mask, other, &exact))
            || scrutinee_of_exact_pattern(chars, mask, (lo, hi), at, &exact)
            || chain_reads_by_closure(chars, mask, (at, until), literal)
        {
            return true;
        }
        literal == "true"
            && ["switch_from", "switch_value_is_on"].iter().any(|reader| {
                occurrences(chars, mask, reader)
                    .into_iter()
                    .filter(|&(start, _)| lo <= start && start < hi)
                    .any(|(start, end)| {
                        if chars.get(end) == Some(&'(') {
                            end < at
                                && at < closing_paren(chars, mask, end)
                                && in_value_position(chars, mask, end + 1, at)
                        } else {
                            start > at && chain_passes_to_reader(chars, mask, (at, until), start)
                        }
                    })
            })
    }

    /// The exact values a comparison or a pattern may hold for `literal`: `Some("true")`, `Ok("true")` and `"true"`.
    fn exact_values(literal: &str) -> [String; 3] {
        [
            format!("Some(\"{literal}\")"),
            format!("Ok(\"{literal}\")"),
            format!("\"{literal}\""),
        ]
    }

    /// Whether `chars[a..b]`, without its surrounding whitespace and parentheses, is one of the exact values.
    fn is_exact_value(
        chars: &[char],
        mask: &[bool],
        (a, b): (usize, usize),
        exact: &[String],
    ) -> bool {
        let (a, b) = without_group(chars, mask, (a, b));
        let text: String = chars[a..b].iter().collect();
        exact.iter().any(|value| *value == text)
    }

    /// The bounds of `chars[a..b]` without its surrounding whitespace and the parentheses that wrap all of it.
    fn without_group(
        chars: &[char],
        mask: &[bool],
        (mut a, mut b): (usize, usize),
    ) -> (usize, usize) {
        loop {
            while a < b && chars[a].is_whitespace() {
                a += 1;
            }
            while b > a && chars[b - 1].is_whitespace() {
                b -= 1;
            }
            if b > a + 1 && chars[a] == '(' && !mask[a] && closing_paren(chars, mask, a) == b - 1 {
                a += 1;
                b -= 1;
            } else {
                return (a, b);
            }
        }
    }

    /// Whether `chars[at]` opens the `==` or `!=` of a comparison.
    fn is_comparison(chars: &[char], mask: &[bool], at: usize) -> bool {
        !mask[at]
            && matches!(chars[at], '=' | '!')
            && chars.get(at + 1) == Some(&'=')
            && chars.get(at + 2) != Some(&'=')
            && !(at > 0 && matches!(chars[at - 1], '=' | '!' | '<' | '>'))
    }

    /// The other side of the innermost comparison in the clause `chars[lo..hi]` that has the place at `chars[at]` in
    /// one of its sides, or none when no comparison holds it. A side runs from the operator out to the bracket, `,`,
    /// `;`, `|`, `&&`, `=` or `{` at its own level (see `side_start` and `side_end`).
    fn own_comparison(
        chars: &[char],
        mask: &[bool],
        (lo, hi): (usize, usize),
        at: usize,
    ) -> Option<(usize, usize)> {
        let mut best: Option<(usize, (usize, usize))> = None;
        for op in lo..hi.saturating_sub(1) {
            if !is_comparison(chars, mask, op) {
                continue;
            }
            let left = (side_start(chars, mask, lo, op), op);
            let right = (op + 2, side_end(chars, mask, op + 2, hi));
            let (own, other) = if left.0 <= at && at < left.1 {
                (left, right)
            } else if right.0 <= at && at < right.1 {
                (right, left)
            } else {
                continue;
            };
            let width = own.1 - own.0;
            if best.map_or(true, |(narrowest, _)| width < narrowest) {
                best = Some((width, other));
            }
        }
        best.map(|(_, other)| other)
    }

    /// Where the left side of the comparison at `chars[op]` starts, no earlier than `lo`, past a leading keyword
    /// (`if`, `return` and kin).
    fn side_start(chars: &[char], mask: &[bool], lo: usize, op: usize) -> usize {
        let mut depth = 0usize;
        let mut from = op;
        while from > lo {
            let at = from - 1;
            if !mask[at] {
                match chars[at] {
                    ')' | ']' | '}' => depth += 1,
                    '(' | '[' | '{' if depth == 0 => break,
                    '(' | '[' | '{' => depth -= 1,
                    ',' | ';' | '|' | '=' if depth == 0 => break,
                    '&' if depth == 0 && at > 0 && chars[at - 1] == '&' => break,
                    '>' if depth == 0 && at > 0 && chars[at - 1] == '=' => break,
                    _ => {}
                }
            }
            from = at;
        }
        loop {
            while from < op && chars[from].is_whitespace() {
                from += 1;
            }
            let word: String = chars[from..op]
                .iter()
                .take_while(|c| c.is_alphanumeric() || **c == '_')
                .collect();
            let keyword = matches!(
                word.as_str(),
                "if" | "while" | "return" | "match" | "else" | "in" | "break"
            );
            if keyword
                && chars
                    .get(from + word.len())
                    .is_some_and(|c| c.is_whitespace())
            {
                from += word.len();
            } else {
                return from;
            }
        }
    }

    /// Where the right side of a comparison that starts at `chars[from]` ends, no later than `hi`.
    fn side_end(chars: &[char], mask: &[bool], from: usize, hi: usize) -> usize {
        let mut depth = 0usize;
        let mut at = from;
        while at < hi {
            if !mask[at] {
                match chars[at] {
                    '(' | '[' => depth += 1,
                    '{' if depth == 0 => break,
                    '{' => depth += 1,
                    ')' | ']' | '}' if depth == 0 => break,
                    ')' | ']' | '}' => depth -= 1,
                    ',' | ';' | '|' | '=' if depth == 0 => break,
                    '&' if depth == 0 && chars.get(at + 1) == Some(&'&') => break,
                    _ => {}
                }
            }
            at += 1;
        }
        at
    }

    /// Whether the place at `chars[at]` is the scrutinee of a `match` in the clause `chars[lo..hi]` that has an exact
    /// value as an arm pattern, or of a `matches!` whose pattern is one.
    fn scrutinee_of_exact_pattern(
        chars: &[char],
        mask: &[bool],
        (lo, hi): (usize, usize),
        at: usize,
        exact: &[String],
    ) -> bool {
        (lo..hi).any(|m| {
            if keyword_at(chars, mask, m, "match") {
                let Some(open) = block_after(chars, mask, m + "match".len(), hi) else {
                    return false;
                };
                m < at
                    && at < open
                    && arm_patterns(chars, mask, open)
                        .into_iter()
                        .any(|pattern| is_exact_value(chars, mask, pattern, exact))
            } else if chars[m] == '(' && !mask[m] && opens_matches(chars, m) {
                let close = closing_paren(chars, mask, m);
                let Some(comma) = (m + 1..close).find(|&c| {
                    !mask[c] && chars[c] == ',' && depth_between(chars, mask, m + 1, c) == 0
                }) else {
                    return false;
                };
                let (a, mut b) = without_group(chars, mask, (comma + 1, close));
                if b > a && chars[b - 1] == ',' {
                    b -= 1;
                }
                m < at && at < comma && is_exact_value(chars, mask, (a, b), exact)
            } else {
                false
            }
        })
    }

    /// The bracket depth that `chars[from..to]` leaves open.
    fn depth_between(chars: &[char], mask: &[bool], from: usize, to: usize) -> usize {
        let mut depth = 0usize;
        for at in from..to {
            if mask[at] {
                continue;
            }
            match chars[at] {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        depth
    }

    /// Whether the keyword `word` starts at `chars[at]`, as a whole word outside masked text.
    fn keyword_at(chars: &[char], mask: &[bool], at: usize, word: &str) -> bool {
        let is_ident = |c: &char| c.is_alphanumeric() || *c == '_';
        let len = word.chars().count();
        !mask[at]
            && at + len <= chars.len()
            && chars[at..at + len].iter().copied().eq(word.chars())
            && !at
                .checked_sub(1)
                .is_some_and(|before| is_ident(&chars[before]))
            && !chars.get(at + len).is_some_and(is_ident)
    }

    /// The `{` that opens the block of a `match` whose scrutinee starts at `chars[from]`: the first one at the
    /// scrutinee's own level, before `hi`.
    fn block_after(chars: &[char], mask: &[bool], from: usize, hi: usize) -> Option<usize> {
        let mut depth = 0usize;
        for at in from..hi {
            if mask[at] {
                continue;
            }
            match chars[at] {
                '(' | '[' => depth += 1,
                '{' if depth == 0 => return Some(at),
                '{' => depth += 1,
                ')' | ']' | '}' if depth == 0 => return None,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        None
    }

    /// The arm patterns of the `match` block that opens at `chars[open]`: the text before each `=>` at the block's
    /// own level, back to the `,`, `{` or `}` that ends the arm before it.
    fn arm_patterns(chars: &[char], mask: &[bool], open: usize) -> Vec<(usize, usize)> {
        let mut patterns = Vec::new();
        let mut depth = 0usize;
        let mut at = open + 1;
        while at < chars.len() {
            if !mask[at] {
                match chars[at] {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' if depth == 0 => break,
                    ')' | ']' | '}' => depth -= 1,
                    '=' if depth == 0 && chars.get(at + 1) == Some(&'>') => {
                        let mut start = at;
                        let mut inner = 0usize;
                        while start > open + 1 {
                            let before = start - 1;
                            if !mask[before] {
                                match chars[before] {
                                    ')' | ']' => inner += 1,
                                    '(' | '[' if inner > 0 => inner -= 1,
                                    '(' | '[' | ',' | '{' | '}' if inner == 0 => break,
                                    _ => {}
                                }
                            }
                            start = before;
                        }
                        patterns.push((start, at));
                    }
                    _ => {}
                }
            }
            at += 1;
        }
        patterns
    }

    /// Whether the place at `chars[to]` sits in value position in the reader argument that starts at `chars[from]`:
    /// no `{`, `if` or `match` comes between them, so the reader takes the place's value and not a value chosen on it.
    fn in_value_position(chars: &[char], mask: &[bool], from: usize, to: usize) -> bool {
        (from..to).all(|at| {
            mask[at]
                || !(chars[at] == '{'
                    || keyword_at(chars, mask, at, "if")
                    || keyword_at(chars, mask, at, "match"))
        })
    }

    /// The links a place's own chain may pass its value through on its way to a reader or an exact closure: each
    /// keeps the value as it is, so none of them turns presence into a value. Each is a call with no argument.
    const ADAPTERS: &[&str] = &[
        "ok",
        "as_deref",
        "as_ref",
        "as_str",
        "cloned",
        "copied",
        "to_owned",
        "to_string",
    ];

    /// The method links of the place's own chain at `chars[start..end]`, in order: each link's name, and the bounds of
    /// its argument list, or none for a link without one (a field, `.await`). The chain leaves a call whose whole
    /// argument it is (`std::env::var(X)`, `get(X)`) and no other bracket, so `matches!(raw, Some(_))` and `(raw)`
    /// end it.
    fn chain_links(
        chars: &[char],
        mask: &[bool],
        (start, end): (usize, usize),
    ) -> Vec<(String, Option<(usize, usize)>)> {
        let is_ident = |c: char| c.is_alphanumeric() || c == '_';
        let mut links = Vec::new();
        let (mut from, mut to) = (start, end);
        loop {
            let mut at = to;
            while at < chars.len() && (mask[at] || chars[at].is_whitespace()) {
                at += 1;
            }
            match chars.get(at) {
                Some('.') => {
                    let mut stop = at + 1;
                    while stop < chars.len() && is_ident(chars[stop]) {
                        stop += 1;
                    }
                    if stop == at + 1 {
                        return links;
                    }
                    let name: String = chars[at + 1..stop].iter().collect();
                    if chars.get(stop) == Some(&'(') {
                        let close = closing_paren(chars, mask, stop);
                        if close >= chars.len() {
                            return links;
                        }
                        links.push((name, Some((stop + 1, close))));
                        to = close + 1;
                    } else {
                        links.push((name, None));
                        to = stop;
                    }
                }
                Some(')') => {
                    let mut open = from;
                    while open > 0 && chars[open - 1].is_whitespace() {
                        open -= 1;
                    }
                    if open == 0 || chars[open - 1] != '(' || mask[open - 1] {
                        return links;
                    }
                    let open = open - 1;
                    let mut callee = open;
                    while callee > 0 && (is_ident(chars[callee - 1]) || chars[callee - 1] == ':') {
                        callee -= 1;
                    }
                    if callee == open || closing_paren(chars, mask, open) != at {
                        return links;
                    }
                    from = callee;
                    to = at + 1;
                }
                _ => return links,
            }
        }
    }

    /// Whether the place's own chain (see `chain_links`) passes its value, through adapters only (see `ADAPTERS`), to
    /// a `map`, `and_then`, `is_some_and` or `is_ok_and` whose whole argument is the reader at `chars[reader]`:
    /// `raw.map(switch_value_is_on)` does, and `matches!(raw, Some(_)).then_some("true").map(switch_value_is_on)`
    /// does not.
    fn chain_passes_to_reader(
        chars: &[char],
        mask: &[bool],
        place: (usize, usize),
        reader: usize,
    ) -> bool {
        for (name, args) in chain_links(chars, mask, place) {
            let Some(args) = args else {
                return false;
            };
            let (a, b) = without_group(chars, mask, args);
            if args.0 <= reader && reader < args.1 {
                return matches!(
                    name.as_str(),
                    "map" | "and_then" | "is_some_and" | "is_ok_and"
                ) && chars[a..b]
                    .iter()
                    .all(|c| c.is_alphanumeric() || *c == '_' || *c == ':');
            }
            if !(ADAPTERS.contains(&name.as_str()) && a == b) {
                return false;
            }
        }
        false
    }

    /// Whether the place's own chain (see `chain_links`) passes its value, through adapters only (see `ADAPTERS`), to
    /// an `is_some_and` or `is_ok_and` whose whole argument is a closure that compares its parameter with `literal`:
    /// `std::env::var(X).is_ok_and(|value| value == "true")`. Any other closure body is no exact read.
    fn chain_reads_by_closure(
        chars: &[char],
        mask: &[bool],
        place: (usize, usize),
        literal: &str,
    ) -> bool {
        for (name, args) in chain_links(chars, mask, place) {
            let Some(args) = args else {
                return false;
            };
            let (a, b) = without_group(chars, mask, args);
            if matches!(name.as_str(), "is_some_and" | "is_ok_and") {
                return exact_closure(&chars[a..b].iter().collect::<String>(), literal);
            }
            if !(ADAPTERS.contains(&name.as_str()) && a == b) {
                return false;
            }
        }
        false
    }

    /// Whether `text` is a closure that compares its one parameter with `literal` and does nothing else:
    /// `|value| value == "true"`, either way round.
    fn exact_closure(text: &str, literal: &str) -> bool {
        let Some((param, body)) = text.strip_prefix('|').and_then(|rest| rest.split_once('|'))
        else {
            return false;
        };
        let param = param.trim();
        let Some((left, right)) = body.split_once("==") else {
            return false;
        };
        let value = format!("\"{literal}\"");
        let (left, right) = (left.trim(), right.trim());
        !param.is_empty()
            && param.chars().all(|c| c.is_alphanumeric() || c == '_')
            && ((left == param && right == value) || (left == value && right == param))
    }

    /// The index of the `)` that closes the `(` at `chars[open]`, or the end of the text when it is not closed there.
    fn closing_paren(chars: &[char], mask: &[bool], open: usize) -> usize {
        let mut depth = 0usize;
        for at in open..chars.len() {
            if mask[at] {
                continue;
            }
            match chars[at] {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return at;
                    }
                }
                _ => {}
            }
        }
        chars.len()
    }

    /// The names a function binds that a call could take for a crate function: its parameters and the local
    /// closures it binds (`let read = |name| ...`). A call to one of them is a call to that closure.
    fn scope_names(function: &str) -> Vec<String> {
        let mut names = param_names(function);
        for piece in function.split("let ").skip(1) {
            let piece = piece.trim_start();
            let piece = piece.strip_prefix("mut ").unwrap_or(piece).trim_start();
            let name: String = piece
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let rest = piece[name.len()..].trim_start();
            if !name.is_empty() && (rest.starts_with("= |") || rest.starts_with("= move |")) {
                names.push(name);
            }
        }
        names
    }

    /// The parameter names of a function text: each identifier at the top level of its parameter list that a `:`
    /// follows (`raw` and `lookup` in `fn f(raw: Option<&str>, lookup: &dyn Fn(&str) -> bool)`).
    fn param_names(function: &str) -> Vec<String> {
        let header = function.split('{').next().unwrap_or(function);
        // The parameter list follows the function's name, so a `(` before it (`pub(crate)`) is not the list.
        let from = header.find("fn ").unwrap_or(0);
        let Some(open) = header[from..].find('(').map(|at| from + at) else {
            return Vec::new();
        };
        let chars: Vec<char> = header[open + 1..].chars().collect();
        let mut names = Vec::new();
        let mut depth = 0usize;
        let mut at = 0;
        while at < chars.len() {
            let c = chars[at];
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            } else if depth == 0 && (c.is_alphabetic() || c == '_') {
                let start = at;
                while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                    at += 1;
                }
                let name: String = chars[start..at].iter().collect();
                let mut next = at;
                while next < chars.len() && chars[next] == ' ' {
                    next += 1;
                }
                if chars.get(next) == Some(&':') && chars.get(next + 1) != Some(&':') {
                    names.push(name);
                }
                continue;
            }
            at += 1;
        }
        names
    }

    /// The names bound by the function that a line sits in (see `scope_names`); none outside a function.
    fn enclosing_scope(
        source: &Source,
        functions: &[(usize, usize, String)],
        l: usize,
    ) -> Vec<String> {
        functions
            .iter()
            .filter(|(start, end, _)| *start <= l && l < *end)
            .max_by_key(|(start, _, _)| *start)
            .map_or_else(Vec::new, |(start, end, _)| {
                scope_names(&source.lines[*start..*end].join("\n"))
            })
    }

    /// A file is keyed by its path under `src`, so two files with one basename in different directories are two
    /// files: each module name belongs to the file its own declaration loads.
    #[test]
    fn same_basenames_in_different_directories_are_different_files() {
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
            source("a.rs", &["#[path = \"a/shared.rs\"]", "pub mod alpha;"]),
            source("b.rs", &["#[path = \"b/shared.rs\"]", "pub mod beta;"]),
            source(
                "a/shared.rs",
                &["pub const ALPHA_VAR: &str = \"ALPHA_SWITCH\";"],
            ),
            source(
                "b/shared.rs",
                &["pub const BETA_VAR: &str = \"BETA_SWITCH\";"],
            ),
        ];
        let modules: Vec<Vec<String>> = (0..sources.len())
            .map(|owner| module_names_of(&sources, owner))
            .collect();
        for (source, names) in sources.iter_mut().zip(modules) {
            source.modules = names;
        }
        assert_eq!(sources[2].modules, vec!["alpha".to_owned()]);
        assert_eq!(sources[3].modules, vec!["beta".to_owned()]);
    }

    /// A `#[path]` inside an inline module is relative to the directory that the inline module opens, as rustc
    /// resolves it. A module without one loads its own file name there. A `mod.rs` or a crate root has no
    /// directory named for its file; any other file has one. A `#[path]` outside an inline module is relative to
    /// the declaring file's own directory.
    #[test]
    fn a_path_inside_an_inline_module_is_relative_to_its_directory() {
        let inline = [
            "pub mod inner {",
            "    #[path = \"deep.rs\"]",
            "    pub mod leaf;",
            "}",
        ];
        assert_eq!(
            declared_files("gate.rs", &inline, 2, "leaf"),
            (vec!["gate/inner/deep.rs".to_owned()], false)
        );
        assert_eq!(
            declared_files("lib.rs", &inline, 2, "leaf"),
            (vec!["inner/deep.rs".to_owned()], false)
        );
        let plain = ["pub mod inner {", "    pub mod plain;", "}"];
        assert_eq!(
            declared_files("gate.rs", &plain, 1, "plain"),
            (
                vec![
                    "gate/inner/plain.rs".to_owned(),
                    "gate/inner/plain/mod.rs".to_owned()
                ],
                false
            )
        );
        assert_eq!(
            declared_files(
                "http_routes/x.rs",
                &["#[path = \"y.rs\"]", "mod z;"],
                1,
                "z"
            ),
            (vec!["http_routes/y.rs".to_owned()], false)
        );
    }

    /// A read that calls a lenient helper fails, even beside an exact reader: every helper the statement calls
    /// must read the value exactly, and a getter that folds it is not one.
    #[test]
    fn every_helper_a_read_calls_must_read_exactly() {
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "lenient_getter".to_owned(),
            vec![(
                0,
                false,
                "fn lenient_getter(name: &str) -> Option<String> { std::env::var(name).ok().map(|value| value.trim().to_owned()) }"
                    .to_owned(),
            )],
        );
        bodies.insert(
            "exact_reader".to_owned(),
            vec![(
                0,
                false,
                "fn exact_reader(raw: Option<&str>) -> bool { raw == Some(\"true\") }".to_owned(),
            )],
        );
        assert!(!read_is_exact(
            &bodies,
            0,
            "switch_from(lenient_getter(X))",
            "true"
        ));
        assert!(!read_is_exact(
            &bodies,
            0,
            "switch_from(exact_reader(lenient_getter(X)))",
            "true"
        ));
        assert!(read_is_exact(
            &bodies,
            0,
            "switch_from(std::env::var(X).ok().as_deref())",
            "true"
        ));
        assert!(read_is_exact(
            &bodies,
            0,
            "exact_reader(std::env::var(X).ok().as_deref())",
            "true"
        ));
    }

    /// A read that does not vouch for itself is not vouched for by the function that holds it: the exact read
    /// of another switch in the same function passes nothing to a read of `X` that has none of its own.
    #[test]
    fn a_read_is_not_vouched_for_by_another_exact_read_in_its_function() {
        let lines: Vec<String> = [
            "fn reads() -> bool {",
            "    let other = std::env::var(B).is_ok();",
            "    let flag = switch_from(std::env::var(A).ok().as_deref());",
            "    other && flag",
            "}",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let none: Bodies = HashMap::new();
        let (own, bound) = read_texts(&lines, 1);
        assert!(!read_of_is_exact(&none, 0, &own, "true", "B"));
        assert!(!read_of_is_exact(&none, 0, &bound, "true", "other"));
        let (own, _) = read_texts(&lines, 2);
        assert!(read_of_is_exact(&none, 0, &own, "true", "A"));
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
                false,
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
                false,
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
            vec![(0, false, exact.to_owned()), (1, false, lenient.to_owned())],
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

    /// Every helper a read puts the value through must read it exactly, not just one of them: an exact read beside a
    /// helper that folds the value vouches for nothing. A helper that folds it one level down fails the read as well.
    #[test]
    fn a_folding_helper_beside_an_exact_read_fails_the_read() {
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "trim_getter".to_owned(),
            vec![(
                0,
                false,
                "fn trim_getter(name: &str) -> Option<String> { std::env::var(name).ok().map(|value| value.trim().to_owned()) }"
                    .to_owned(),
            )],
        );
        bodies.insert(
            "wrapper".to_owned(),
            vec![(
                0,
                false,
                "fn wrapper(name: &str) -> Option<String> { trim_getter(name) }".to_owned(),
            )],
        );
        bodies.insert(
            "guard".to_owned(),
            vec![(
                0,
                false,
                "fn guard(raw: Option<String>, on: bool) -> bool { on && raw.is_some() }"
                    .to_owned(),
            )],
        );
        bodies.insert(
            "exact_getter".to_owned(),
            vec![(
                0,
                false,
                "fn exact_getter(name: &str) -> bool { std::env::var(name).as_deref() == Ok(\"true\") }"
                    .to_owned(),
            )],
        );
        // E1: the reader is exact, but the value goes through `wrapper`, which folds it in `trim_getter`.
        assert!(!read_is_exact(
            &bodies,
            0,
            "switch_from(wrapper(X))",
            "true"
        ));
        // E2: the exact read is an argument of `guard`, which does not read the value at all.
        assert!(!read_is_exact(
            &bodies,
            0,
            "guard(std::env::var(X).ok(), exact_getter(X))",
            "true"
        ));
        assert!(read_is_exact(&bodies, 0, "exact_getter(X)", "true"));
    }

    /// The cross-file lookup follows a free function of an inline module in another file, and not a method: a method
    /// of that name elsewhere is no target of a bare call (see `is_method`).
    #[test]
    fn a_free_function_of_another_files_inline_module_is_followed_and_a_method_is_not() {
        let lines: Vec<String> = [
            "pub mod helper {",
            "    pub fn lenient(name: &str) -> String { name.trim().to_owned() }",
            "}",
            "pub struct Registry;",
            "impl Registry {",
            "    pub fn lenient(&self) -> bool { true }",
            "}",
            "pub struct Bridge<T>(T);",
            "impl<T> Bridge<T>",
            "where",
            "    T: Clone,",
            "{",
            "    pub fn lenient(&self) -> bool { true }",
            "}",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        assert!(!is_method(&lines, 1), "a free function of an inline module");
        assert!(is_method(&lines, 5), "a method of an impl block");
        assert!(is_method(&lines, 12), "a method of a wrapped impl header");
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "lenient".to_owned(),
            vec![
                (1, is_method(&lines, 1), lines[1].clone()),
                (1, is_method(&lines, 5), lines[5].clone()),
                (1, is_method(&lines, 12), lines[12].clone()),
            ],
        );
        let found = resolve(&bodies, 0, "lenient");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].1.contains(".trim()"), "{found:?}");
        assert_eq!(
            resolve(&bodies, 1, "lenient").len(),
            3,
            "a file with a name of its own sees all of its candidates"
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
                false,
                "fn recognise(value: &str) -> Option<Self> { match value { \"on\" => Some(Self::On), _ => None } }"
                    .to_owned(),
            )],
        );
        assert!(read_of_is_exact(
            &bodies,
            0,
            "value.and_then(Self::recognise)",
            "on",
            "value"
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
        assert!(read_of_is_exact(&none, 0, &statement(0), "true", "A"));
        assert!(
            !read_of_is_exact(&none, 0, &statement(1), "true", "B"),
            "the exact read of A on the line before vouched for B"
        );
    }

    /// Probe B: an exact read of another switch in the same statement does not vouch for a presence check, since an
    /// exact read vouches only for the value it takes: `switch_from(A) || B.is_ok()` reads A exactly, and not B.
    #[test]
    fn an_exact_read_of_another_switch_in_the_same_statement_does_not_vouch() {
        let none: Bodies = HashMap::new();
        let statement =
            "    let on = switch_from(std::env::var(A).ok().as_deref()) || std::env::var(B).is_ok();";
        assert!(read_of_is_exact(&none, 0, statement, "true", "A"));
        assert!(!read_of_is_exact(&none, 0, statement, "true", "B"));
        // A comparison vouches only for its own operand as well.
        let compared =
            "    let on = std::env::var(A).as_deref() == Ok(\"true\") && std::env::var(B).is_ok();";
        assert!(read_of_is_exact(&none, 0, compared, "true", "A"));
        assert!(!read_of_is_exact(&none, 0, compared, "true", "B"));
        // And a helper vouches only for the value it takes.
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "exact_reader".to_owned(),
            vec![(
                0,
                false,
                "fn exact_reader(raw: Option<&str>) -> bool { raw == Some(\"true\") }".to_owned(),
            )],
        );
        let helped =
            "    let on = exact_reader(std::env::var(A).ok().as_deref()) || std::env::var(B).is_ok();";
        assert!(read_of_is_exact(&bodies, 0, helped, "true", "A"));
        assert!(!read_of_is_exact(&bodies, 0, helped, "true", "B"));
        // `matches!` keeps its pattern with its value.
        let matched = "    let on = matches!(std::env::var(A).ok().as_deref(), Some(\"true\"));";
        assert!(read_of_is_exact(&none, 0, matched, "true", "A"));
    }

    /// Probes A and C: on the bound path a read is judged by the uses of the variable it binds. An exact read of
    /// another value in the statement that first uses the variable does not vouch for it.
    #[test]
    fn a_bound_read_is_not_vouched_for_by_another_values_exact_read() {
        let none: Bodies = HashMap::new();
        let judged = |lines: &[&str]| {
            let lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
            let (own, bound) = read_texts(&lines, 0);
            let name = bound_variable(&lines, 0).expect("the read binds a variable");
            (
                read_of_is_exact(&none, 0, &own, "true", "B"),
                read_of_is_exact(&none, 0, &bound, "true", &name),
            )
        };
        // A: the presence of B, joined to an exact read of A.
        assert_eq!(
            judged(&[
                "    let present = std::env::var(B).is_ok();",
                "    let flag = switch_from(std::env::var(A).ok().as_deref()) && present;",
            ]),
            (false, false)
        );
        // C: B's value is taken as present, beside an exact read of another value.
        assert_eq!(
            judged(&[
                "    let raw = std::env::var(B).ok();",
                "    let on = switch_from(other_value) || raw.is_some();",
            ]),
            (false, false)
        );
        // The exact reader that takes the variable still vouches for it.
        assert_eq!(
            judged(&[
                "    let raw = std::env::var(B).ok();",
                "    let on = switch_from(raw.as_deref());",
            ]),
            (false, true)
        );
    }

    /// Probe D: a helper's own exact read does not excuse another crate helper it puts the value through. Every
    /// helper on the value's path must read exactly, at every level, and a helper that still calls one at the depth
    /// limit fails.
    #[test]
    fn a_helpers_own_exact_read_does_not_excuse_its_lenient_callee() {
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "parse_both".to_owned(),
            vec![(
                0,
                false,
                "fn parse_both(raw: Option<&str>) -> bool { switch_from(raw) || lenient_check(raw) }"
                    .to_owned(),
            )],
        );
        bodies.insert(
            "lenient_check".to_owned(),
            vec![(
                0,
                false,
                "fn lenient_check(raw: Option<&str>) -> bool { raw.map(|v| v.trim()).is_some() }"
                    .to_owned(),
            )],
        );
        assert!(!read_is_exact(
            &bodies,
            0,
            "switch_from(parse_both(std::env::var(X).ok().as_deref()))",
            "true"
        ));
        // Without the trim, a presence check is still no exact read.
        bodies.insert(
            "lenient_check".to_owned(),
            vec![(
                0,
                false,
                "fn lenient_check(raw: Option<&str>) -> bool { raw.is_some() }".to_owned(),
            )],
        );
        assert!(!read_is_exact(
            &bodies,
            0,
            "switch_from(parse_both(std::env::var(X).ok().as_deref()))",
            "true"
        ));
        // Exact all the way down, it passes.
        bodies.insert(
            "lenient_check".to_owned(),
            vec![(
                0,
                false,
                "fn lenient_check(raw: Option<&str>) -> bool { raw == Some(\"true\") }".to_owned(),
            )],
        );
        assert!(read_is_exact(
            &bodies,
            0,
            "parse_both(std::env::var(X).ok().as_deref())",
            "true"
        ));
        // At the depth limit, a helper that still calls a crate helper is not read, so it fails.
        bodies.insert(
            "outer".to_owned(),
            vec![(
                0,
                false,
                "fn outer(raw: Option<&str>) -> bool { switch_from(raw) && parse_both(raw) }"
                    .to_owned(),
            )],
        );
        assert!(!read_is_exact(
            &bodies,
            0,
            "outer(std::env::var(X).ok().as_deref())",
            "true"
        ));
    }

    /// A presence check on a helper's own value parameter fails the helper, even beside its exact read of that
    /// parameter (probes p12 and p13). Each place the helper names its value by must be an exact read of it, an
    /// argument of a crate helper that reads exactly, or a call of a local alias of the reader. A presence check that
    /// an exact read is joined to by `==`, or that shares its clause with one, is no exact read either (probes a to
    /// d), and no crate helper it is passed to vouches for it. An exact value vouches only as the other whole side of
    /// the place's own comparison or as a pattern the place is matched on, and a reader only for a place in value
    /// position, so a presence check spelled `(raw).is_some()`, `Option::is_some(&raw)`, `map_or`, `is_some_and` or
    /// `matches!` fails beside its exact read too. A closure that only compares its parameter with the literal, on the
    /// place's own chain, still reads it exactly.
    #[test]
    fn a_presence_check_on_a_helpers_value_parameter_fails_beside_its_exact_read() {
        let mut bodies: Bodies = HashMap::new();
        let mut helper = |name: &str, text: &str| {
            bodies.insert(name.to_owned(), vec![(0, false, text.to_owned())]);
        };
        helper(
            "exact",
            "fn exact(raw: Option<&str>) -> bool { switch_from(raw) }",
        );
        // p12: the exact read is beside a presence check on the value parameter.
        helper(
            "either",
            "fn either(raw: Option<&str>) -> bool { switch_from(raw) || raw.is_some() }",
        );
        // p13: the presence check on the value parameter is beside an exact helper that reads it.
        helper(
            "both",
            "fn both(raw: Option<&str>) -> bool { raw.is_some() && exact(raw) }",
        );
        helper(
            "compared",
            "fn compared(raw: Option<&str>) -> bool { raw.is_some() && raw == Some(\"true\") }",
        );
        helper(
            "aliased",
            "fn aliased(raw: Option<&str>) -> bool { let on = switch_value_is_on; on(raw) }",
        );
        helper(
            "aliased_lenient",
            "fn aliased_lenient(raw: Option<&str>) -> bool { let on = switch_value_is_on; on(raw) || raw.is_some() }",
        );
        helper(
            "unread",
            "fn unread(raw: Option<&str>) -> bool { switch_from(None) }",
        );
        // Probes a to d: a presence check joined to an exact read by `==`, or sharing its clause with one.
        helper(
            "probe_a",
            "fn probe_a(raw: Option<&str>) -> bool { let on = switch_value_is_on; raw.is_some() == on(raw) }",
        );
        helper(
            "probe_b",
            "fn probe_b(raw: Option<&str>) -> bool { raw.is_some() == exact(raw) }",
        );
        helper(
            "probe_c",
            "fn probe_c(raw: Option<&str>) -> bool { raw.is_some() == (raw == Some(\"true\")) }",
        );
        helper(
            "probe_d",
            "fn probe_d(raw: Option<&str>) -> bool { switch_from(if raw.is_some() { Some(\"true\") } else { None }) }",
        );
        // The presence check on the other side of a line break, beside a marker in the same clause.
        helper(
            "split_c",
            "fn split_c(raw: Option<&str>) -> bool {\n    raw\n        .is_some() == (raw == Some(\"true\"))\n}",
        );
        // `other` reads exactly, but a comparison with its result does not pass the value through it.
        helper(
            "other",
            "fn other(raw: Option<&str>) -> Option<&str> { if raw == Some(\"true\") { Some(\"true\") } else { None } }",
        );
        helper(
            "compared_to_other",
            "fn compared_to_other(raw: Option<&str>) -> bool { raw == other(None) }",
        );
        // The shapes that stay exact: a literal comparison, and a struct literal built from the reader's alias.
        helper(
            "compared_exact",
            "fn compared_exact(raw: Option<&str>) -> bool { raw == Some(\"true\") }",
        );
        helper(
            "struct_literal",
            "fn struct_literal(live: Option<&str>) -> Self { let on = switch_value_is_on; Self { live: on(live) } }",
        );
        helper(
            "room_like",
            "fn room_like(view: Option<&str>, text: Option<&str>) -> Self {\n    let on = switch_value_is_on;\n    Self {\n        view: on(view),\n        text: on(view) && on(text),\n    }\n}",
        );
        helper(
            "compared_reversed",
            "fn compared_reversed(raw: Option<&str>) -> bool { Some(\"true\") == raw }",
        );
        helper(
            "matched_exact",
            "fn matched_exact(raw: Option<&str>) -> bool { match raw { Some(\"true\") => true, _ => false } }",
        );
        helper(
            "matches_exact",
            "fn matches_exact(raw: Option<&str>) -> bool { matches!(raw, Some(\"true\")) }",
        );
        helper(
            "mapped_exact",
            "fn mapped_exact(raw: Option<&str>) -> bool { raw.map(switch_value_is_on).unwrap_or(false) }",
        );
        helper(
            "closure_exact",
            "fn closure_exact(raw: Option<&str>) -> bool { raw.is_some_and(|value| value == \"true\") }",
        );
        // A closure that does more than compare its parameter, or a chain that transforms the value first, is none.
        helper(
            "closure_lenient",
            "fn closure_lenient(raw: Option<&str>) -> bool { raw.is_some_and(|value| value == \"true\" || value.is_empty()) }",
        );
        helper(
            "closure_mapped",
            "fn closure_mapped(raw: Option<&str>) -> bool { raw.map(|_| \"true\").is_some_and(|value| value == \"true\") }",
        );
        // An exact value elsewhere in the clause is not the other side of the presence check's own comparison.
        helper(
            "grouped_presence",
            "fn grouped_presence(raw: Option<&str>) -> bool { (raw).is_some() == (raw == Some(\"true\")) }",
        );
        helper(
            "path_presence",
            "fn path_presence(raw: Option<&str>) -> bool { Option::is_some(&raw) == (raw == Some(\"true\")) }",
        );
        helper(
            "map_or_presence",
            "fn map_or_presence(raw: Option<&str>) -> bool { raw.map_or(false, |_| true) == (raw == Some(\"true\")) }",
        );
        helper(
            "is_some_and_presence",
            "fn is_some_and_presence(raw: Option<&str>) -> bool { raw.is_some_and(|_| true) == (raw == Some(\"true\")) }",
        );
        helper(
            "matches_presence",
            "fn matches_presence(raw: Option<&str>) -> bool { matches!(raw, Some(_)) == (raw == Some(\"true\")) }",
        );
        // A reader argument that chooses its value on the place does not read the place.
        helper(
            "chosen_presence",
            "fn chosen_presence(raw: Option<&str>) -> bool { switch_from(if raw.map_or(false, |_| true) { Some(\"true\") } else { None }) }",
        );
        helper(
            "mapped_presence",
            "fn mapped_presence(raw: Option<&str>) -> bool { matches!(raw, Some(_)).then_some(\"true\").map(switch_value_is_on).unwrap_or(false) }",
        );
        // A presence check is vouched for by no crate helper it is passed to, even an exact one.
        helper(
            "exact_param",
            "fn exact_param(v: Option<&str>) -> bool { switch_from(v) }",
        );
        helper(
            "presence_through",
            "fn presence_through(raw: Option<&str>) -> bool { exact_param(raw.is_some().then_some(\"true\")) }",
        );
        let call = |name: &str| format!("{name}(std::env::var(X).ok().as_deref())");
        assert!(read_is_exact(&bodies, 0, &call("exact"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("either"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("both"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("compared"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("probe_a"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("probe_b"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("probe_c"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("probe_d"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("split_c"), "true"));
        assert!(!read_is_exact(
            &bodies,
            0,
            &call("compared_to_other"),
            "true"
        ));
        assert!(read_is_exact(&bodies, 0, &call("aliased"), "true"));
        assert!(!read_is_exact(&bodies, 0, &call("aliased_lenient"), "true"));
        assert!(read_is_exact(&bodies, 0, &call("compared_exact"), "true"));
        assert!(read_is_exact(&bodies, 0, &call("struct_literal"), "true"));
        for exact in [
            "room_like",
            "compared_reversed",
            "matched_exact",
            "matches_exact",
            "mapped_exact",
            "closure_exact",
            "exact_param",
        ] {
            assert!(read_is_exact(&bodies, 0, &call(exact), "true"), "{exact}");
        }
        for lenient in [
            "grouped_presence",
            "path_presence",
            "map_or_presence",
            "is_some_and_presence",
            "matches_presence",
            "chosen_presence",
            "mapped_presence",
            "presence_through",
            "closure_lenient",
            "closure_mapped",
        ] {
            assert!(
                !read_is_exact(&bodies, 0, &call(lenient), "true"),
                "{lenient}"
            );
        }
        // The same shapes on the statement path, with no helper in between.
        let none: Bodies = HashMap::new();
        for lenient in [
            "std::env::var(X).is_ok() == (std::env::var(X).ok().as_deref() == Some(\"true\"))",
            "switch_from(if std::env::var(X).map_or(false, |_| true) { Some(\"true\") } else { None })",
        ] {
            assert!(!read_is_exact(&none, 0, lenient, "true"), "{lenient}");
        }
        for exact in [
            "Some(\"true\") == std::env::var(X).ok().as_deref()",
            "std::env::var(X).is_ok_and(|value| value == \"true\")",
            "get(X).is_some_and(|value| value == \"true\")",
            "std::env::var(X).ok().as_deref().map(switch_value_is_on)",
        ] {
            assert!(read_is_exact(&none, 0, exact, "true"), "{exact}");
        }
        assert!(
            !read_is_exact(&bodies, 0, &call("unread"), "true"),
            "a helper that never names its value does not read it"
        );
        // An alias binds the reader only when it is bound to the reader itself, and only for `true`.
        assert_eq!(
            reader_aliases("let on = switch_value_is_on; on(raw)", "true"),
            vec!["on".to_owned()]
        );
        assert!(reader_aliases("let flag = switch_from(raw);", "true").is_empty());
        assert!(reader_aliases("let on = switch_from;", "1").is_empty());
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
        assert_eq!(bound_variable(&lines, 0).as_deref(), Some("raw"));
        assert!(!read_of_is_exact(&none, 0, &own, "true", "A"));
        assert!(read_of_is_exact(&none, 0, &bound, "true", "raw"));
        let (own, bound) = read_texts(&lines, 3);
        assert!(!read_of_is_exact(&none, 0, &own, "true", "B"));
        assert!(!read_of_is_exact(&none, 0, &bound, "true", "other"));
        let (own, bound) = read_texts(&lines, 4);
        assert!(!read_of_is_exact(&none, 0, &own, "true", "C"));
        assert!(!read_of_is_exact(&none, 0, &bound, "true", "lenient"));
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
                false,
                "fn from_values(a: Option<&str>, b: Option<&str>) -> bool { let on = switch_value_is_on; on(a) && on(b) }"
                    .to_owned(),
            )],
        );
        assert_eq!(opener_line(&lines, 2), Some(0));
        let (first, _) = read_texts(&lines, 2);
        assert!(read_of_is_exact(&bodies, 0, &first, "true", "B"));
        assert!(!read_of_is_exact(
            &bodies,
            0,
            "std::env::var(B).ok().as_deref(),",
            "true",
            "B"
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
        assert!(!read_of_is_exact(&none, 0, &first, "true", "B"));
        assert!(!read_of_is_exact(&none, 0, &second, "true", "B"));
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
