//! Site skills for the Do loop: org skills whose `url_patterns` match the page a
//! `browser.observe` just returned (Quarry v2 E4.3, F-t2-043).
//!
//! A skill may name the sites it is for (`UpsertAgentSkillRequest.url_patterns`,
//! stored on `agent_skills` since migration 0116). While `EXECUTION_CORE_SKILL_URL_MATCH`
//! is the literal `true`, a successful `browser.observe` in the Do loop is followed by the
//! guidance of up to [`MAX_URL_SKILLS`] such skills, framed AFTER the already screened and
//! rendered page output, so the skill text is never screened or labelled as page content.
//!
//! # What this is not
//!
//! The text is advisory context. It never widens the tool set, never grants a permission and
//! never changes what the run may do: the purpose-lock, the permission modes and the browser
//! grant decide that, exactly as before. Skills are read through the run's own verified
//! session credential, so what a person may not read is not matched either.
//!
//! # Grammar
//!
//! A pattern is closed and has no regular expression:
//!
//! * a lowercase host (`docs.example.com`), or `*.host` for any subdomain of `host` at any
//!   depth (the bare `host` is not matched by `*.host`; the wildcard host needs two labels
//!   after the `*.`, so `*.com` matches nothing);
//! * optionally followed by a `/path-prefix` (`docs.example.com/api`), which matches that path
//!   and everything below it on a segment boundary;
//! * optionally ending in `*` after a path (`docs.example.com/api*`, `docs.example.com/api/*`),
//!   which turns the path into a plain text prefix.
//!
//! Only `http` and `https` URLs are matched. A query, a fragment, a port and the credentials of
//! an address are ignored. A pattern that breaks the grammar matches nothing.
//!
//! # Per run
//!
//! The org's skills are listed once per run and cached for [`CACHE_TTL`] (256 runs at most), and
//! each (skill id, version) is injected at most once per run, so a model that observes the same
//! site ten times reads the guidance once.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::{self as pb, session_core_client::SessionCoreClient};
use tonic::transport::Channel;

use super::skill_budget::{self, fit_skill_blocks};

/// Deployment switch. Only the literal `true` turns site-skill matching on.
const SKILL_URL_MATCH_ENV: &str = "EXECUTION_CORE_SKILL_URL_MATCH";

/// Most site skills one observation may carry.
pub(crate) const MAX_URL_SKILLS: usize = 2;

/// How long a run's skill list is reused before it is read again.
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Most runs the cache holds; the oldest entry makes room for a new run.
const CACHE_MAX_RUNS: usize = 256;

/// Longest page address read out of an observation, in bytes.
const MAX_PAGE_URL_BYTES: usize = 2048;

/// The label that opens the appended guidance.
const SITE_SKILL_LABEL: &str = "[site skill]";

/// Whether the deployment has switched site-skill matching on.
pub(crate) fn enabled() -> bool {
    parse_enabled(std::env::var(SKILL_URL_MATCH_ENV).ok().as_deref())
}

fn parse_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// The host and path of an http(s) address, lowercased. `None` for any other scheme, an empty
/// host or an address this module does not read (an IPv6 literal, a space).
fn host_and_path(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    if url.is_empty() || url.len() > MAX_PAGE_URL_BYTES || url.chars().any(char::is_whitespace) {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    // The query and the fragment are not part of what a skill is matched on.
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    // Credentials, then the port.
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    if host_port.starts_with('[') {
        return None;
    }
    let host = host_port.split(':').next().unwrap_or_default();
    let host = host.trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    Some((host.to_owned(), path.to_owned()))
}

/// Whether a host is plain lowercase DNS labels.
fn is_plain_host(host: &str) -> bool {
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// Whether `pattern` matches the http(s) address `url`. See the module docs for the grammar.
/// Anything outside it, and any address that is not http or https, does not match.
pub(crate) fn url_pattern_matches(pattern: &str, url: &str) -> bool {
    let Some((host, path)) = host_and_path(url) else {
        return false;
    };
    let pattern = pattern.trim();
    let (pattern, star) = match pattern.strip_suffix('*') {
        Some(rest) => (rest, true),
        None => (pattern, false),
    };
    let (pattern_host, pattern_path) = match pattern.find('/') {
        Some(at) => (&pattern[..at], &pattern[at..]),
        None => (pattern, ""),
    };
    // A trailing `*` belongs to a path; `host*` is not in the grammar.
    if star && pattern_path.is_empty() {
        return false;
    }
    let host_matches = match pattern_host.strip_prefix("*.") {
        Some(suffix) => {
            // Two labels at least, so `*.com` cannot name a whole registry.
            is_plain_host(suffix)
                && suffix.contains('.')
                && host.len() > suffix.len() + 1
                && host.ends_with(suffix)
                && host[..host.len() - suffix.len()].ends_with('.')
        }
        None => is_plain_host(pattern_host) && host == pattern_host,
    };
    if !host_matches {
        return false;
    }
    if pattern_path.is_empty() {
        return true;
    }
    if pattern_path.contains('*') {
        return false;
    }
    if star {
        return path.starts_with(pattern_path);
    }
    let prefix = pattern_path.trim_end_matches('/');
    prefix.is_empty() || path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// The length of the longest pattern of `skill` that matches `page_url`, or `None`.
fn best_match_len(skill: &pb::AgentSkill, page_url: &str) -> Option<usize> {
    skill
        .url_patterns
        .iter()
        .filter(|pattern| url_pattern_matches(pattern, page_url))
        .map(|pattern| pattern.trim().chars().count())
        .max()
}

/// The skills to apply on `page_url`: enabled, accepted, kind `skill`, with a non-empty
/// `url_patterns` and some content, at most [`MAX_URL_SKILLS`], the longest matching pattern
/// first (ties keep the listing order).
pub(crate) fn select_for_url<'a>(
    skills: &'a [pb::AgentSkill],
    page_url: &str,
) -> Vec<&'a pb::AgentSkill> {
    let mut matched: Vec<(usize, &pb::AgentSkill)> = skills
        .iter()
        .filter(|skill| {
            !skill.url_patterns.is_empty()
                && skill.enabled
                // A learned proposal its owner has not accepted is not usable (WS07-S7).
                && (skill.review_state.is_empty() || skill.review_state == "accepted")
                && (skill.kind.is_empty() || skill.kind == "skill")
                && !skill.content.trim().is_empty()
        })
        .filter_map(|skill| best_match_len(skill, page_url).map(|len| (len, skill)))
        .collect();
    // Stable: equal specificity keeps the order session-core listed them in.
    matched.sort_by(|a, b| b.0.cmp(&a.0));
    matched
        .into_iter()
        .take(MAX_URL_SKILLS)
        .map(|(_, skill)| skill)
        .collect()
}

/// One skill as the block that goes into the budget.
fn site_skill_block(skill: &pb::AgentSkill) -> String {
    format!("## Site skill: {}\n{}", skill.name, skill.content)
}

/// `output` followed by the site-skill guidance, inside the shared 8000-character skill budget
/// (truncate before drop). Blocks are `## Site skill: <name>\n<content>`. With nothing to add,
/// or nothing that fits, `output` comes back unchanged.
pub(crate) fn annotate(output: &str, blocks: &[String]) -> String {
    if blocks.is_empty() {
        return output.to_owned();
    }
    let fitted = fit_skill_blocks(blocks.to_vec());
    if fitted.truncated > 0 || fitted.dropped > 0 {
        tracing::warn!(
            truncated = fitted.truncated,
            dropped = fitted.dropped,
            budget_chars = skill_budget::SKILL_CONTEXT_BUDGET_CHARS,
            "site skill guidance exceeded its context budget; degraded to fit"
        );
    }
    if fitted.blocks.is_empty() {
        return output.to_owned();
    }
    format!(
        "{output}\n\n{SITE_SKILL_LABEL}\n{}",
        fitted.blocks.join("\n\n")
    )
}

/// The address a `browser.observe` result names: the `page_url` of the JSON that the Do loop's
/// observation wraps in its untrusted-content frame. `None` when the result is not that shape.
pub(crate) fn page_url_of_observation(output: &str) -> Option<String> {
    let start = output.find('{')?;
    let end = output.rfind('}')?;
    if end < start {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&output[start..=end]).ok()?;
    let url = value.get("page_url")?.as_str()?.trim();
    if url.is_empty() || url.len() > MAX_PAGE_URL_BYTES {
        return None;
    }
    Some(url.to_owned())
}

/// One run's cached skill list and what it has already been told.
struct RunSkills {
    fetched: Instant,
    /// The org's skills that carry `url_patterns`; nothing else is kept.
    skills: Vec<pb::AgentSkill>,
    /// (skill id, version) of every skill already injected in this run.
    injected: HashSet<(String, i32)>,
}

type RunKey = (String, String);

fn cache() -> &'static Mutex<HashMap<RunKey, RunSkills>> {
    static CACHE: OnceLock<Mutex<HashMap<RunKey, RunSkills>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

/// Whether `key` has a skill list younger than [`CACHE_TTL`].
fn is_fresh(key: &RunKey) -> bool {
    cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(key)
        .is_some_and(|entry| entry.fetched.elapsed() < CACHE_TTL)
}

/// Stores a freshly read list for `key`, keeping what the run was already told. Makes room first:
/// expired entries go, then the oldest, so the map never passes [`CACHE_MAX_RUNS`].
fn store(key: RunKey, skills: Vec<pb::AgentSkill>) {
    let mut map = cache().lock().unwrap_or_else(PoisonError::into_inner);
    let injected = map
        .remove(&key)
        .map(|entry| entry.injected)
        .unwrap_or_default();
    if map.len() >= CACHE_MAX_RUNS {
        map.retain(|_, entry| entry.fetched.elapsed() < CACHE_TTL);
    }
    while map.len() >= CACHE_MAX_RUNS {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, entry)| entry.fetched)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
    map.insert(
        key,
        RunSkills {
            fetched: Instant::now(),
            skills,
            injected,
        },
    );
}

/// Reads the org's enabled skills through the run's verified session credential and keeps those
/// with `url_patterns`. `None` on any failure: matching is advisory and never fails a step.
async fn fetch_url_skills(
    org_id: &str,
    channel: &Channel,
    bearer: &str,
) -> Option<Vec<pb::AgentSkill>> {
    let request = super::authenticated_session_request(
        pb::ListAgentSkillsRequest {
            org_id: org_id.to_owned(),
            enabled_only: true,
            for_user_id: String::new(),
            kinds: Vec::new(),
            thread_id: String::new(),
        },
        bearer,
    )
    .ok()?;
    let response = match SessionCoreClient::new(channel.clone())
        .list_agent_skills(request)
        .await
    {
        Ok(response) => response.into_inner(),
        Err(error) => {
            tracing::warn!(code = ?error.code(), %org_id, "site skills: skill read failed");
            return None;
        }
    };
    Some(
        response
            .skills
            .into_iter()
            .filter(|skill| !skill.url_patterns.is_empty())
            .collect(),
    )
}

/// The blocks to add after an observation of `page_url`: the matching skills this run has not
/// been given yet, marked as given. Empty when none match.
fn take_new_blocks(key: &RunKey, page_url: &str) -> Vec<String> {
    let mut map = cache().lock().unwrap_or_else(PoisonError::into_inner);
    let Some(entry) = map.get_mut(key) else {
        return Vec::new();
    };
    let fresh: Vec<&pb::AgentSkill> = select_for_url(&entry.skills, page_url)
        .into_iter()
        .filter(|skill| !entry.injected.contains(&(skill.id.clone(), skill.version)))
        .collect();
    if fresh.is_empty() {
        return Vec::new();
    }
    let blocks: Vec<String> = fresh.iter().map(|skill| site_skill_block(skill)).collect();
    // `annotate` fits the blocks the same way; mark the ones that will survive the fit.
    let kept = fit_skill_blocks(blocks.clone()).blocks.len();
    let marked: Vec<(String, i32)> = fresh
        .iter()
        .take(kept)
        .map(|skill| (skill.id.clone(), skill.version))
        .collect();
    entry.injected.extend(marked);
    blocks
}

/// The site-skill blocks for an observation of `page_url` in `run_id`, or none. Every failure
/// (no session channel or credential, a failed read, no match) yields none.
pub(crate) async fn blocks_for_page(
    org_id: &str,
    run_id: &str,
    page_url: &str,
    zdr: bool,
    session_channel: Option<&Channel>,
    session_bearer: Option<&str>,
) -> Vec<String> {
    // The Do loop's browser tools never run under zero retention; this is the defensive line.
    if zdr || org_id.trim().is_empty() || run_id.trim().is_empty() {
        return Vec::new();
    }
    let key: RunKey = (org_id.to_owned(), run_id.to_owned());
    if !is_fresh(&key) {
        let (Some(channel), Some(bearer)) = (
            session_channel,
            session_bearer.filter(|b| !b.trim().is_empty()),
        ) else {
            return Vec::new();
        };
        let Some(skills) = fetch_url_skills(org_id, channel, bearer).await else {
            return Vec::new();
        };
        store(key.clone(), skills);
    }
    take_new_blocks(&key, page_url)
}

/// `rendered` with the site-skill guidance for `page_url` after it, or `rendered` unchanged.
/// The caller has checked the switch and that the step was a successful `browser.observe`.
pub(crate) async fn annotate_observation(
    rendered: String,
    org_id: &str,
    run_id: &str,
    page_url: &str,
    zdr: bool,
    session_channel: Option<&Channel>,
    session_bearer: Option<&str>,
) -> String {
    let blocks = blocks_for_page(
        org_id,
        run_id,
        page_url,
        zdr,
        session_channel,
        session_bearer,
    )
    .await;
    if blocks.is_empty() {
        return rendered;
    }
    annotate(&rendered, &blocks)
}
