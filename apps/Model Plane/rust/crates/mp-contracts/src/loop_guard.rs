//! Repeat, cycle and stall detection for the tool loops (WS25-S1).
//!
//! A model can ask for the same failing call again and again, bounce between
//! two calls, or keep trying calls that all fail. Each loop has so far been
//! bounded only by its round budget, which the model spends on the loop. This
//! module is the one place that decides a tool phase has stopped making
//! progress. The gateway's inline loop judges by it from WS25-S1, and
//! execution-core's Do loop adopts the same rules in WS25-S2.
//!
//! The guard only observes. [`LoopGuard::observe_round`] never executes, retries
//! or re-dispatches a tool. A verdict asks the loop that owns the guard to remind
//! the model or to end the tool phase. Retry authority stays with the dispatch
//! loops' own retry policies.
//!
//! Its state lives in memory for one turn and is never persisted. A stop is
//! content-free: it names the tool, carries the call's signature (a hash, never
//! the arguments) and counts the calls the pattern covers.
//!
//! # Rules
//!
//! * Repeat: the same signature seen [`REMIND_AT`] times reminds the model, and
//!   [`STOP_AT`] times stops the loop. The count runs over the whole turn.
//! * Cycle: a block of two or three calls with at least two different calls in
//!   it, repeated three times over the latest calls, stops the loop.
//! * No progress: [`STALL_ROUNDS`] rounds in a row with no successful call, and
//!   at least two different calls among them, stops the loop. One call repeated
//!   is a repeat, and the repeat rule owns that case.
//!
//! Result awareness (`LOOP_GUARD_RESULT_AWARE`, off unless set) changes the repeat
//! and cycle rules. A call whose result digest differs from the digest that call
//! last returned is progress: its run restarts, and it is kept out of the cycle
//! check. A call with no digest, or with the digest it had before, is counted as it
//! always was. The no-progress rule does not change.
//!
//! The digest leaves out what a tool stamps on every answer: a timestamp, a request
//! id, a parked result's handle and its expiry, a timing (see [`result_digest`]).
//! Without that, a repeated call that gets the same answer with a fresh stamp would
//! be progress on every call, and no repeat or cycle would ever stop.
//!
//! [`LoopGuardMode`] decides what a verdict does to the loop that asks. This
//! module only parses the switch.

use std::collections::{HashMap, HashSet, VecDeque};

/// The repeat counts at which the model is reminded: the third and the fifth.
pub const REMIND_AT: [u32; 2] = [3, 5];

/// The repeat count at which the loop stops.
pub const STOP_AT: u32 = 8;

/// How many of the latest calls the cycle check looks back over.
const CYCLE_WINDOW: usize = 12;

/// How many rounds in a row with no successful call make a stall.
const STALL_ROUNDS: usize = 4;

/// The reminder for the third repeat of one call. Authored here, never copied
/// from a tool result, so no tool output can word what the model is told.
pub const REMIND_FIRST: &str = "You have made this same tool call three times this turn. Use the results you already have, or change the arguments if you need something different.";

/// The reminder for the fifth repeat of one call.
pub const REMIND_SECOND: &str = "You have made this same tool call five times this turn. Stop repeating it and answer from what you have, saying what could not be checked. If it keeps repeating, the tool phase will end.";

/// Given to the model when a stop ends the tool phase, so the answer says what
/// is missing instead of presenting partial work as complete.
pub const STOP_NOTE: &str = "The tool phase was stopped because the calls kept repeating or failing without progress. Do not call any more tools. Answer from the results above and say plainly which part of the request could not be completed or checked.";

/// What `LOOP_GUARD_MODE` asks for.
///
/// Unset or empty is [`Off`](Self::Off): no guard is built and nothing changes.
/// `off`, `shadow` and `enforce` are themselves. Any other value is
/// [`Shadow`](Self::Shadow): this switch only observes, so a value it cannot read
/// must never enforce. Authority switches read an unknown value as enforce; this
/// one does not (the K3-26 vocabulary split).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopGuardMode {
    Off,
    /// Counts and logs verdicts. The loop runs as it would with the guard off.
    Shadow,
    /// Reminders reach the model, and a stop ends the tool phase.
    Enforce,
}

/// The environment switch of the guard, `LOOP_GUARD_MODE`: read by the gateway's inline loop and by
/// execution-core's Do loop.
pub const ENV: &str = "LOOP_GUARD_MODE";

/// The environment switch that turns on result awareness, `LOOP_GUARD_RESULT_AWARE`. Each consumer reads it
/// once per process through [`result_aware_from_env_value`]. Off unless the value is the literal `true`.
pub const RESULT_AWARE_ENV: &str = "LOOP_GUARD_RESULT_AWARE";

/// Whether a set mode-switch value is one the mode does not read. Such a value falls to the weaker
/// reading without an error: this guard observes, and a strengthening switch stays off. Unset, empty,
/// `off`, and `shadow` or `enforce` in any case with surrounding whitespace are read. `OFF` is not
/// `off`: it is flagged, because it reads as `shadow` here.
#[must_use]
pub fn unreadable_mode_word(value: &str) -> bool {
    let word = value.trim();
    !(word.is_empty()
        || word == "off"
        || word.eq_ignore_ascii_case("shadow")
        || word.eq_ignore_ascii_case("enforce"))
}

/// Whether a set value of the approval step-up switch, `APPROVAL_STEP_UP_ENFORCED`, is one its gate does
/// not take. The gate reads `shadow` and `enforce` trimmed and in any case, and any other value as off.
/// So `off` in any case is a word it reads as off and is not flagged, where [`unreadable_mode_word`]
/// flags `OFF`. Unset and blank values are never flagged.
#[must_use]
pub fn unreadable_step_up_word(value: &str) -> bool {
    let word = value.trim();
    !(word.is_empty()
        || word.eq_ignore_ascii_case("off")
        || word.eq_ignore_ascii_case("shadow")
        || word.eq_ignore_ascii_case("enforce"))
}

impl LoopGuardMode {
    /// Parses the raw `LOOP_GUARD_MODE` value, if it is set. `enforce` and `shadow` are read trimmed and
    /// case-folded, as `SESSION_CORE_SPACE_ZDR` is, so `Enforce` and ` enforce ` enforce. Any other value
    /// observes as `Shadow`; [`unreadable_mode_word`] lets the startup report warn about it.
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            None | Some("" | "off") => Self::Off,
            Some(word) if word.eq_ignore_ascii_case("enforce") => Self::Enforce,
            Some(word) if word.eq_ignore_ascii_case("shadow") => Self::Shadow,
            Some(_) => Self::Shadow,
        }
    }

    /// Whether the guard runs at all.
    #[must_use]
    pub const fn observes(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether a verdict changes what the loop sends.
    #[must_use]
    pub const fn enforces(self) -> bool {
        matches!(self, Self::Enforce)
    }

    /// The closed label used in logs and metrics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Shadow => "shadow",
            Self::Enforce => "enforce",
        }
    }
}

/// How a call ended, as the guard counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    /// Returned a result.
    Ok,
    /// Failed, or was cut off before it could run.
    Err,
    /// Not run, because an identical call already ran this turn.
    Suppressed,
}

/// One call of a round, as the guard reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    /// The tool's name. A name is not content, so a stop may carry it.
    pub tool: String,
    /// The call's [`call_signature`].
    pub signature: String,
    pub outcome: CallOutcome,
    /// The call's [`result_digest`], when the call ran and the consumer computed one. `None` is an unknown
    /// result, and an unknown result is judged as it always was.
    pub result_digest: Option<String>,
}

/// Why the guard stopped a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Repeat,
    Cycle,
    NoProgress,
}

impl StopReason {
    /// The closed label used in logs and metrics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Repeat => "repeat",
            Self::Cycle => "cycle",
            Self::NoProgress => "no_progress",
        }
    }
}

/// A content-free account of a stop: the tool, a hash of its signature, and how
/// many calls the pattern covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopStop {
    pub reason: StopReason,
    pub tool: String,
    /// The signature of the call the stop names. It is a hash of the tool and its
    /// arguments, never the arguments.
    pub signature_hash: String,
    /// The calls the pattern covers: the repeats of the signature, the calls of
    /// the cycle's window, or the calls of the stalled rounds.
    pub count: u32,
}

/// What the loop should do after a round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    /// Reminds the model. `level` is 1 for the third repeat and 2 for the fifth.
    Remind {
        level: u8,
        text: &'static str,
    },
    /// Ends the loop that owns the guard.
    Stop(LoopStop),
}

impl Verdict {
    /// The closed label used in logs and metrics.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Remind { .. } => "remind",
            Self::Stop(_) => "stop",
        }
    }
}

/// The guard's state for one turn. Build one per loop and feed it every round in
/// order.
#[derive(Debug, Default)]
pub struct LoopGuard {
    repeats: HashMap<String, Repeats>,
    /// The latest calls, for the cycle check.
    recent: VecDeque<WindowCall>,
    /// The latest rounds, for the stall check.
    rounds: VecDeque<Round>,
    /// Whether a call whose result changed is progress (see [`LoopGuard::with_result_awareness`]).
    result_aware: bool,
}

#[derive(Debug, Default)]
struct Repeats {
    count: u32,
    /// The highest reminder level already given for this call.
    reminded: u8,
    /// The digest of the result this call last returned. Only kept with result awareness on.
    last_digest: Option<String>,
}

/// One call in the cycle window.
#[derive(Debug)]
struct WindowCall {
    tool: String,
    /// The call's signature. A stop names it, so it never carries a digest.
    signature: String,
    /// What the cycle check compares: the signature, or `signature#digest` for a call whose result changed.
    /// A cycle through such a call needs the same result to recur, not only the same call.
    key: String,
}

#[derive(Debug)]
struct Round {
    progressed: bool,
    signatures: Vec<String>,
}

impl LoopGuard {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Turns result awareness on or off, as `LOOP_GUARD_RESULT_AWARE` does. On, a call whose digest differs from
    /// the one it last returned is progress: its run restarts, and it stays out of the cycle check. Off, every
    /// call is judged as it was before digests existed.
    #[must_use]
    pub fn with_result_awareness(mut self, on: bool) -> Self {
        self.result_aware = on;
        self
    }

    /// Observes one round's calls, in the order the model asked for them, and
    /// returns the verdict for the loop.
    ///
    /// A round with no calls changes nothing and continues. A stop takes
    /// precedence over a reminder in the same round.
    pub fn observe_round(&mut self, calls: &[CallRecord]) -> Verdict {
        if calls.is_empty() {
            return Verdict::Continue;
        }
        let result_aware = self.result_aware;
        let mut reminder: u8 = 0;
        let mut repeat_stop = None;
        for call in calls {
            // A digest counts only with result awareness on. No digest is unknown, and counts as before.
            let digest = call.result_digest.as_deref().filter(|_| result_aware);
            let repeats = self.repeats.entry(call.signature.clone()).or_default();
            // A result that differs from the one this call last returned is progress, not a repeat: the
            // call's run restarts here.
            let result_changed = digest.is_some_and(|now| {
                repeats
                    .last_digest
                    .as_deref()
                    .is_some_and(|seen| seen != now)
            });
            if result_changed {
                repeats.count = 0;
                repeats.reminded = 0;
            }
            repeats.count += 1;
            if let Some(now) = digest {
                repeats.last_digest = Some(now.to_owned());
            }
            if repeat_stop.is_none() && repeats.count >= STOP_AT {
                repeat_stop = Some(LoopStop {
                    reason: StopReason::Repeat,
                    tool: call.tool.clone(),
                    signature_hash: call.signature.clone(),
                    count: repeats.count,
                });
            }
            let level: u8 = if repeats.count >= REMIND_AT[1] {
                2
            } else {
                u8::from(repeats.count >= REMIND_AT[0])
            };
            if level > repeats.reminded {
                repeats.reminded = level;
                reminder = reminder.max(level);
            }
            let key = match digest {
                Some(now) if result_changed => format!("{}#{now}", call.signature),
                _ => call.signature.clone(),
            };
            self.recent.push_back(WindowCall {
                tool: call.tool.clone(),
                signature: call.signature.clone(),
                key,
            });
            if self.recent.len() > CYCLE_WINDOW {
                self.recent.pop_front();
            }
        }
        self.rounds.push_back(Round {
            progressed: calls.iter().any(|call| call.outcome == CallOutcome::Ok),
            signatures: calls.iter().map(|call| call.signature.clone()).collect(),
        });
        if self.rounds.len() > STALL_ROUNDS {
            self.rounds.pop_front();
        }

        if let Some(stop) = repeat_stop
            .or_else(|| self.cycle_stop())
            .or_else(|| self.stall_stop())
        {
            return Verdict::Stop(stop);
        }
        match reminder {
            1 => Verdict::Remind {
                level: 1,
                text: REMIND_FIRST,
            },
            2 => Verdict::Remind {
                level: 2,
                text: REMIND_SECOND,
            },
            _ => Verdict::Continue,
        }
    }

    /// A block of two or three calls, with at least two different calls in it,
    /// repeated three times over the latest calls.
    fn cycle_stop(&self) -> Option<LoopStop> {
        let len = self.recent.len();
        for period in 2..=3 {
            let span = 3 * period;
            if len < span {
                continue;
            }
            let start = len - span;
            let key_at = |index: usize| &self.recent[start + index].key;
            let block: HashSet<&String> = (0..period).map(key_at).collect();
            let repeats = (period..span).all(|index| key_at(index) == key_at(index - period));
            if block.len() > 1 && repeats {
                let last = &self.recent[len - 1];
                return Some(LoopStop {
                    reason: StopReason::Cycle,
                    tool: last.tool.clone(),
                    signature_hash: last.signature.clone(),
                    count: u32::try_from(span).unwrap_or(u32::MAX),
                });
            }
        }
        None
    }

    /// [`STALL_ROUNDS`] rounds in a row with no successful call, across at least
    /// two different calls.
    fn stall_stop(&self) -> Option<LoopStop> {
        if self.rounds.len() < STALL_ROUNDS || self.rounds.iter().any(|round| round.progressed) {
            return None;
        }
        let distinct: HashSet<&String> = self
            .rounds
            .iter()
            .flat_map(|round| &round.signatures)
            .collect();
        if distinct.len() < 2 {
            return None;
        }
        let last = self.recent.back()?;
        let covered: usize = self.rounds.iter().map(|round| round.signatures.len()).sum();
        Some(LoopStop {
            reason: StopReason::NoProgress,
            tool: last.tool.clone(),
            signature_hash: last.signature.clone(),
            count: u32::try_from(covered).unwrap_or(u32::MAX),
        })
    }
}

/// The signature that makes two calls the same call: the tool's name and its
/// arguments in canonical form, hashed so that no argument content is kept.
///
/// A `query` argument is compared after [`normalise_query`], so case and spacing
/// do not hide a repeated search. Arguments that are not JSON are hashed as the
/// text they are.
#[must_use]
pub fn call_signature(tool: &str, arguments_json: &str) -> String {
    let mut arguments = serde_json::from_str::<serde_json::Value>(arguments_json)
        .unwrap_or_else(|_| serde_json::Value::String(arguments_json.trim().to_owned()));
    if let Some(query) = arguments.get_mut("query") {
        let normalised = query.as_str().map(normalise_query);
        if let Some(normalised) = normalised {
            *query = serde_json::Value::String(normalised);
        }
    }
    // serde_json keeps object keys sorted (its `preserve_order` feature is off),
    // so the order the model wrote the arguments in does not change the signature.
    let canonical = arguments.to_string();
    let mut hasher = blake3::Hasher::new();
    hasher.update(tool.as_bytes());
    hasher.update(b"\n");
    hasher.update(canonical.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// The case- and whitespace-insensitive form of a search query, so "Aquatiq" and
/// "aquatiq " are one repeated query.
#[must_use]
pub fn normalise_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The digest length in hex characters: 64 bits, enough to tell two results of one call apart within a turn.
const RESULT_DIGEST_HEX_LEN: usize = 16;

/// The placeholder a volatile timestamp becomes in the text a digest is taken over.
const TIME_PLACEHOLDER: &str = "<time>";

/// The placeholder a volatile request, trace, span or correlation id value becomes in that text.
const ID_PLACEHOLDER: &str = "<id>";

/// JSON keys that name a volatile field whole, matched on the key folded to lowercase letters and digits, so
/// `checked_at`, `checkedAt` and `Checked-At` are one key: a server's clock, its timings, and a parked result's
/// expiry. `now` and `as_of` are left out: they often wrap or date real data, so a change there can be progress.
const VOLATILE_KEY_NAMES: [&str; 12] = [
    "servertime",
    "fetchedat",
    "retrievedat",
    "generatedat",
    "checkedat",
    "observedat",
    "expiresat",
    "traceparent",
    "elapsedms",
    "durationms",
    "latencyms",
    "tookms",
];

/// Folded JSON key endings that mark a volatile field: any `timestamp`, a request, trace, span or correlation id
/// (`request_id`, `x-request-id`, `trace_id`), and a parked result's `handle_id`.
const VOLATILE_KEY_ENDINGS: [&str; 6] = [
    "timestamp",
    "requestid",
    "traceid",
    "spanid",
    "correlationid",
    "handleid",
];

/// The request, trace, span and correlation id stems a keyed id value follows, lowercase.
const ID_STEMS: [&[u8]; 4] = [b"request", b"trace", b"span", b"correlation"];

/// The digest of one call's result, for result awareness: the BLAKE3 hash of the result's normalised text (see
/// `normalised_result`), in hex and cut to 16 characters. The guard compares digests within one turn only. They
/// are never persisted, logged or exported, and the result itself is never kept.
#[must_use]
pub fn result_digest(output: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(normalised_result(output).as_bytes());
    let hex = hasher.finalize().to_hex().to_string();
    hex[..RESULT_DIGEST_HEX_LEN].to_owned()
}

/// The text a result digest is taken over, with what a tool stamps on every answer left out. A JSON result is
/// judged by its keys: the volatile keys go at every depth, and the rest is written back in canonical form
/// (object keys sorted, no spacing), so the spacing and key order of an answer do not count. Any other result is
/// judged by its text: an RFC 3339 date-time, or a `2026-11-21 10:15:30` stamp, becomes [`TIME_PLACEHOLDER`], and
/// the value after a request, trace, span or correlation id becomes [`ID_PLACEHOLDER`]. A date with no clock, a
/// number and every other value stay, so a change in them is still progress.
fn normalised_result(output: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(output) {
        Ok(mut value) => {
            strip_volatile_keys(&mut value);
            value.to_string()
        }
        Err(_) => scrub_volatile_tokens(output),
    }
}

/// Removes the volatile keys from a JSON value, at every depth.
fn strip_volatile_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|key, _| !is_volatile_key(key));
            map.values_mut().for_each(strip_volatile_keys);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_volatile_keys),
        _ => {}
    }
}

/// Whether a JSON key names a volatile field, by [`VOLATILE_KEY_NAMES`] or [`VOLATILE_KEY_ENDINGS`].
fn is_volatile_key(key: &str) -> bool {
    let folded: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    VOLATILE_KEY_NAMES.contains(&folded.as_str())
        || VOLATILE_KEY_ENDINGS
            .iter()
            .any(|ending| folded.ends_with(*ending))
}

/// Replaces the volatile tokens of a text: each timestamp becomes [`TIME_PLACEHOLDER`] and each keyed id value
/// becomes [`ID_PLACEHOLDER`]. Both are looked for at every byte. A match is ASCII, so each cut lands on a char
/// boundary, and every other byte, multi-byte characters included, is copied as it is.
fn scrub_volatile_tokens(text: &str) -> String {
    let bytes = text.as_bytes();
    let lower_text = text.to_ascii_lowercase();
    let lower = lower_text.as_bytes();
    let mut scrubbed = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        let token = timestamp_end(bytes, at)
            .map(|end| (end, TIME_PLACEHOLDER))
            .or_else(|| keyed_id_end(lower, at).map(|end| (end, ID_PLACEHOLDER)));
        if let Some((end, placeholder)) = token {
            scrubbed.push_str(&text[copied..at]);
            scrubbed.push_str(placeholder);
            copied = end;
            at = end;
        } else {
            at += 1;
        }
    }
    scrubbed.push_str(&text[copied..]);
    scrubbed
}

/// The end of an RFC 3339 date-time (`2026-11-21T10:15:30.5+01:00`) or of a `2026-11-21 10:15:30` stamp that starts
/// at `start`. A time needs a `T` or a space before its clock, and the zone, fraction and seconds are optional; a
/// space needs the seconds. A bare date is content and is not matched.
fn timestamp_end(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && bytes[start - 1].is_ascii_digit() {
        return None;
    }
    let mut scan = Scan { bytes, at: start };
    scan.digits(4)?;
    scan.one_of(b"-")?;
    scan.digits(2)?;
    scan.one_of(b"-")?;
    scan.digits(2)?;
    let separator = scan.one_of(b"Tt ")?;
    scan.digits(2)?;
    scan.one_of(b":")?;
    scan.digits(2)?;
    let before_seconds = scan.at;
    scan.optional(|s| {
        s.one_of(b":")?;
        s.digits(2)
    });
    if separator == b' ' && scan.at == before_seconds {
        return None;
    }
    scan.optional(|s| {
        s.one_of(b".,")?;
        s.digit_run()
    });
    scan.optional(|s| s.one_of(b"Zz").map(|_| ()));
    scan.optional(|s| {
        s.one_of(b"+-")?;
        s.digits(2)?;
        s.optional(|z| {
            let _ = z.one_of(b":");
            z.digits(2)
        });
        Some(())
    });
    if scan.peek().is_some_and(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(scan.at)
}

/// The end of a request, trace, span or correlation id key and its value, when one starts at `start`. The key is
/// a stem, one optional `-`, `_` or space, and `id` with no letter after it, so `request_id`, `x-request-id` and
/// `Request ID` all match. The value follows any `:`, `=`, spaces and quotes, and is the run of letters, digits,
/// `-` and `_`.
fn keyed_id_end(lower: &[u8], start: usize) -> Option<usize> {
    if start > 0 && lower[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    let stem = ID_STEMS
        .iter()
        .find(|stem| lower[start..].starts_with(stem))?;
    let mut at = start + stem.len();
    if matches!(lower.get(at).copied(), Some(b'-' | b'_' | b' ')) {
        at += 1;
    }
    if !lower[at..].starts_with(b"id") {
        return None;
    }
    at += 2;
    if lower.get(at).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    while matches!(
        lower.get(at).copied(),
        Some(b':' | b'=' | b' ' | b'"' | b'\'')
    ) {
        at += 1;
    }
    let value_start = at;
    while lower
        .get(at)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_'))
    {
        at += 1;
    }
    (at > value_start).then_some(at)
}

/// A forward cursor over a result's bytes, for the volatile-token scanner.
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Scan<'_> {
    /// Takes exactly `count` ASCII digits.
    fn digits(&mut self, count: usize) -> Option<()> {
        let run = self.bytes.get(self.at..self.at + count)?;
        if !run.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.at += count;
        Some(())
    }

    /// Takes one or more ASCII digits.
    fn digit_run(&mut self) -> Option<()> {
        let count = self.bytes[self.at..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if count == 0 {
            return None;
        }
        self.at += count;
        Some(())
    }

    /// Takes one byte of `options` and returns it.
    fn one_of(&mut self, options: &[u8]) -> Option<u8> {
        let byte = *self.bytes.get(self.at)?;
        if !options.contains(&byte) {
            return None;
        }
        self.at += 1;
        Some(byte)
    }

    /// Runs `part`. When it fails the cursor goes back, so the part is simply absent.
    fn optional(&mut self, part: impl FnOnce(&mut Self) -> Option<()>) {
        let before = self.at;
        if part(self).is_none() {
            self.at = before;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
}

/// Whether `LOOP_GUARD_RESULT_AWARE` turns result awareness on: the exact literal `true`, read through the
/// shared strict parser. A padded or cased value, `1`, and an unset variable are off, as they are for every
/// literal switch.
#[must_use]
pub fn result_aware_from_env_value(value: Option<&str>) -> bool {
    crate::loop_fold::switch_from(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(tool: &str, arguments: &str, outcome: CallOutcome) -> CallRecord {
        CallRecord {
            tool: tool.to_owned(),
            signature: call_signature(tool, arguments),
            outcome,
            result_digest: None,
        }
    }

    /// A successful call of `tool` with `arguments`, whose result digests to `digest`.
    fn answered(tool: &str, arguments: &str, digest: &str) -> CallRecord {
        CallRecord {
            result_digest: Some(digest.to_owned()),
            ..call(tool, arguments, CallOutcome::Ok)
        }
    }

    /// A guard with result awareness on.
    fn aware() -> LoopGuard {
        LoopGuard::new().with_result_awareness(true)
    }

    /// The verdict of each of `count` identical successful calls, the call `index` carrying the digest `digest(index)`.
    fn verdicts_for(
        guard: &mut LoopGuard,
        arguments: &str,
        count: u32,
        digest: impl Fn(u32) -> Option<String>,
    ) -> Vec<Verdict> {
        (0..count)
            .map(|index| {
                let record = CallRecord {
                    result_digest: digest(index),
                    ..call("fetch_url", arguments, CallOutcome::Ok)
                };
                guard.observe_round(std::slice::from_ref(&record))
            })
            .collect()
    }

    fn failing_fetch(url: &str) -> CallRecord {
        call(
            "fetch_url",
            &format!(r#"{{"url":"{url}"}}"#),
            CallOutcome::Err,
        )
    }

    /// Feeds one call per round and returns each round's verdict.
    fn one_call_per_round(guard: &mut LoopGuard, calls: &[CallRecord]) -> Vec<Verdict> {
        calls
            .iter()
            .map(|record| guard.observe_round(std::slice::from_ref(record)))
            .collect()
    }

    #[test]
    fn loop_guard_mode_is_off_unless_asked_and_an_unknown_value_only_observes() {
        assert_eq!(LoopGuardMode::from_env_value(None), LoopGuardMode::Off);
        assert_eq!(LoopGuardMode::from_env_value(Some("")), LoopGuardMode::Off);
        assert_eq!(
            LoopGuardMode::from_env_value(Some(" off ")),
            LoopGuardMode::Off
        );
        assert_eq!(
            LoopGuardMode::from_env_value(Some("shadow")),
            LoopGuardMode::Shadow
        );
        assert_eq!(
            LoopGuardMode::from_env_value(Some("enforce")),
            LoopGuardMode::Enforce
        );
        for word in ["Enforce", "ENFORCE", " enforce ", "\tEnforce\n"] {
            assert_eq!(
                LoopGuardMode::from_env_value(Some(word)),
                LoopGuardMode::Enforce,
                "{word:?} is read trimmed and in any case"
            );
        }
        for unknown in ["true", "1", "on", "enforce-all", "enforced", "OFF"] {
            assert_eq!(
                LoopGuardMode::from_env_value(Some(unknown)),
                LoopGuardMode::Shadow,
                "{unknown} must observe and never enforce"
            );
        }
        assert!(!LoopGuardMode::Off.observes());
        assert!(LoopGuardMode::Shadow.observes() && !LoopGuardMode::Shadow.enforces());
        assert!(LoopGuardMode::Enforce.observes() && LoopGuardMode::Enforce.enforces());
    }

    #[test]
    fn unreadable_mode_word_flags_only_a_value_the_mode_does_not_read() {
        for word in ["", "   ", "off", "shadow", "Shadow", " enforce ", "ENFORCE"] {
            assert!(!unreadable_mode_word(word), "{word:?} is read");
        }
        for word in ["enforced", "true", "1", "on", "OFF", "Off", "enforce-all"] {
            assert!(unreadable_mode_word(word), "{word:?} is not read");
        }
    }

    #[test]
    fn the_step_up_word_rule_reads_off_in_any_case_where_the_loop_guard_does_not() {
        // The loop guard reads `OFF` as shadow, so it flags `OFF` and `Off`. The step-up gate reads
        // every case of `off` as off, so it does not.
        assert_eq!(
            LoopGuardMode::from_env_value(Some("OFF")),
            LoopGuardMode::Shadow
        );
        assert!(unreadable_mode_word("OFF") && unreadable_mode_word("Off"));
        for word in ["off", "OFF", "Off", " off "] {
            assert!(
                !unreadable_step_up_word(word),
                "{word:?} reads as off at the step-up gate"
            );
        }
        for word in ["", "   ", "shadow", "Shadow", " enforce ", "ENFORCE"] {
            assert!(!unreadable_step_up_word(word), "{word:?} is read");
        }
        for word in ["enforced", "true", "1", "on", "enforce-all", "y"] {
            assert!(
                unreadable_step_up_word(word),
                "{word:?} is not read by the step-up gate"
            );
        }
    }

    #[test]
    fn loop_guard_repeated_failing_call_reminds_at_three_and_five_then_stops_at_eight() {
        let mut guard = LoopGuard::new();
        let calls: Vec<CallRecord> = (0..8)
            .map(|_| failing_fetch("https://down.example/page"))
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert_eq!(verdicts[0], Verdict::Continue);
        assert_eq!(verdicts[1], Verdict::Continue);
        assert_eq!(
            verdicts[2],
            Verdict::Remind {
                level: 1,
                text: REMIND_FIRST
            }
        );
        assert_eq!(verdicts[3], Verdict::Continue);
        assert_eq!(
            verdicts[4],
            Verdict::Remind {
                level: 2,
                text: REMIND_SECOND
            }
        );
        assert_eq!(verdicts[5], Verdict::Continue);
        assert_eq!(verdicts[6], Verdict::Continue);
        assert_eq!(
            verdicts[7],
            Verdict::Stop(LoopStop {
                reason: StopReason::Repeat,
                tool: "fetch_url".to_owned(),
                signature_hash: calls[7].signature.clone(),
                count: 8,
            })
        );
    }

    #[test]
    fn loop_guard_a_round_repeating_one_call_three_times_reminds_once() {
        let mut guard = LoopGuard::new();
        let record = failing_fetch("https://down.example/page");
        let round = vec![record.clone(), record.clone(), record.clone()];
        assert_eq!(
            guard.observe_round(&round),
            Verdict::Remind {
                level: 1,
                text: REMIND_FIRST
            }
        );
        assert_eq!(
            guard.observe_round(std::slice::from_ref(&record)),
            Verdict::Continue,
            "the fourth repeat does not remind again"
        );
    }

    #[test]
    fn loop_guard_suppressed_repeats_count_toward_the_repeat_rules() {
        let mut guard = LoopGuard::new();
        let suppressed = call(
            "knowledge_search",
            r#"{"query":"Aquatiq"}"#,
            CallOutcome::Suppressed,
        );
        let verdicts: Vec<Verdict> = (0..3)
            .map(|_| guard.observe_round(std::slice::from_ref(&suppressed)))
            .collect();
        assert_eq!(
            verdicts[2],
            Verdict::Remind {
                level: 1,
                text: REMIND_FIRST
            }
        );
    }

    #[test]
    fn loop_guard_two_calls_alternating_three_times_stop_as_a_cycle() {
        let mut guard = LoopGuard::new();
        let a = call("web_search", r#"{"query":"alpha"}"#, CallOutcome::Ok);
        let b = call("web_search", r#"{"query":"beta"}"#, CallOutcome::Ok);
        let calls = [
            a.clone(),
            b.clone(),
            a.clone(),
            b.clone(),
            a.clone(),
            b.clone(),
        ];
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert!(verdicts[..5]
            .iter()
            .all(|verdict| !matches!(verdict, Verdict::Stop(_))));
        assert_eq!(
            verdicts[5],
            Verdict::Stop(LoopStop {
                reason: StopReason::Cycle,
                tool: "web_search".to_owned(),
                signature_hash: b.signature,
                count: 6,
            })
        );
    }

    #[test]
    fn loop_guard_three_calls_repeated_three_times_stop_as_a_cycle_of_nine() {
        let mut guard = LoopGuard::new();
        let calls: Vec<CallRecord> = ["alpha", "beta", "gamma"]
            .iter()
            .cycle()
            .take(9)
            .map(|query| {
                call(
                    "web_search",
                    &format!(r#"{{"query":"{query}"}}"#),
                    CallOutcome::Ok,
                )
            })
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert_eq!(
            verdicts[6],
            Verdict::Remind {
                level: 1,
                text: REMIND_FIRST
            },
            "the first call of the cycle has its third repeat at call seven"
        );
        assert_eq!(
            verdicts[8],
            Verdict::Stop(LoopStop {
                reason: StopReason::Cycle,
                tool: "web_search".to_owned(),
                signature_hash: calls[8].signature.clone(),
                count: 9,
            })
        );
    }

    #[test]
    fn loop_guard_four_rounds_of_failing_different_calls_stop_as_no_progress() {
        let mut guard = LoopGuard::new();
        let rounds = [
            vec![call(
                "fetch_url",
                r#"{"url":"https://a.example"}"#,
                CallOutcome::Err,
            )],
            vec![call("web_search", r#"{"query":"b"}"#, CallOutcome::Err)],
            vec![call(
                "knowledge_search",
                r#"{"query":"c"}"#,
                CallOutcome::Suppressed,
            )],
            vec![call(
                "fetch_url",
                r#"{"url":"https://d.example"}"#,
                CallOutcome::Err,
            )],
        ];
        let verdicts: Vec<Verdict> = rounds
            .iter()
            .map(|round| guard.observe_round(round))
            .collect();
        assert!(verdicts[..3]
            .iter()
            .all(|verdict| *verdict == Verdict::Continue));
        let Verdict::Stop(stop) = &verdicts[3] else {
            panic!("expected a stall stop, got {:?}", verdicts[3]);
        };
        assert_eq!(stop.reason, StopReason::NoProgress);
        assert_eq!(stop.tool, "fetch_url");
        assert_eq!(stop.count, 4);
    }

    #[test]
    fn loop_guard_a_successful_call_in_the_window_breaks_the_stall() {
        let mut guard = LoopGuard::new();
        let rounds = [
            vec![call(
                "fetch_url",
                r#"{"url":"https://a.example"}"#,
                CallOutcome::Err,
            )],
            vec![call("web_search", r#"{"query":"b"}"#, CallOutcome::Ok)],
            vec![call(
                "knowledge_search",
                r#"{"query":"c"}"#,
                CallOutcome::Err,
            )],
            vec![call(
                "fetch_url",
                r#"{"url":"https://d.example"}"#,
                CallOutcome::Err,
            )],
        ];
        let verdicts: Vec<Verdict> = rounds
            .iter()
            .map(|round| guard.observe_round(round))
            .collect();
        assert_eq!(verdicts[3], Verdict::Continue);
    }

    #[test]
    fn loop_guard_one_failing_call_repeated_is_a_repeat_and_never_a_stall() {
        let mut guard = LoopGuard::new();
        let calls: Vec<CallRecord> = (0..4)
            .map(|_| failing_fetch("https://down.example/page"))
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert!(verdicts.iter().all(|verdict| !matches!(
            verdict,
            Verdict::Stop(stop) if stop.reason == StopReason::NoProgress
        )));
    }

    #[test]
    fn loop_guard_an_empty_round_changes_nothing() {
        let mut guard = LoopGuard::new();
        for _ in 0..10 {
            assert_eq!(guard.observe_round(&[]), Verdict::Continue);
        }
        assert_eq!(
            guard.observe_round(std::slice::from_ref(&failing_fetch("https://x.example"))),
            Verdict::Continue,
            "ten empty rounds did not make the next call a repeat"
        );
    }

    #[test]
    fn loop_guard_stop_is_content_free() {
        let mut guard = LoopGuard::new();
        let calls: Vec<CallRecord> = (0..8)
            .map(|_| failing_fetch("https://intern.example/case?token=abc123"))
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        let Verdict::Stop(stop) = &verdicts[7] else {
            panic!("expected a stop at eight, got {:?}", verdicts[7]);
        };
        let rendered = format!("{stop:?}");
        assert!(!rendered.contains("abc123"), "{rendered}");
        assert!(!rendered.contains("intern.example"), "{rendered}");
        assert_eq!(stop.signature_hash.len(), 64);
        assert!(stop
            .signature_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));
        assert!(!REMIND_FIRST.contains("http") && !REMIND_SECOND.contains("http"));
    }

    #[test]
    fn loop_guard_signature_ignores_key_order_and_query_case_and_spacing() {
        let first = call_signature("web_search", r#"{"query":"  Coresystem   AS ","top_k":5}"#);
        let reordered = call_signature("web_search", r#"{"top_k":5,"query":"coresystem as"}"#);
        assert_eq!(first, reordered);
        assert_ne!(
            first,
            call_signature("knowledge_search", r#"{"query":"coresystem as","top_k":5}"#),
            "a different tool is a different call"
        );
        assert_ne!(
            first,
            call_signature("web_search", r#"{"query":"coresystem as","top_k":6}"#),
            "different arguments are a different call"
        );
    }

    #[test]
    fn loop_guard_signature_hashes_arguments_that_are_not_json_as_their_text() {
        let first = call_signature("fetch_url", "not json");
        assert_eq!(first, call_signature("fetch_url", "  not json "));
        assert_ne!(first, call_signature("fetch_url", "other text"));
    }

    #[test]
    fn loop_guard_normalise_query_collapses_case_and_whitespace() {
        assert_eq!(normalise_query("  Aquatiq \t  AS "), "aquatiq as");
        assert_eq!(normalise_query("   "), "");
    }

    #[test]
    fn loop_guard_result_awareness_is_off_unless_asked_and_the_digests_change_nothing() {
        let arguments = r#"{"url":"https://flip.example/page"}"#;
        let changing = |index: u32| Some(format!("digest-{index}"));
        let steady = |_: u32| Some("steady".to_owned());
        let unknown = |_: u32| -> Option<String> { None };
        let off_changing = verdicts_for(&mut LoopGuard::new(), arguments, 10, changing);
        let off_steady = verdicts_for(&mut LoopGuard::new(), arguments, 10, steady);
        let off_unknown = verdicts_for(&mut LoopGuard::new(), arguments, 10, unknown);
        let on_unknown = verdicts_for(&mut aware(), arguments, 10, unknown);
        assert_eq!(
            off_changing, off_unknown,
            "with the switch off, a digest is ignored"
        );
        assert_eq!(
            off_steady, off_unknown,
            "with the switch off, a digest is ignored"
        );
        assert_eq!(
            on_unknown, off_unknown,
            "a call with no digest is judged as before, even with the switch on"
        );
        assert!(matches!(
            &off_unknown[7],
            Verdict::Stop(stop) if stop.reason == StopReason::Repeat && stop.count == 8
        ));
    }

    #[test]
    fn loop_guard_a_call_whose_result_keeps_changing_never_reaches_stop_at() {
        let mut guard = aware();
        let calls: Vec<CallRecord> = (0..3 * STOP_AT)
            .map(|index| answered("job_status", r#"{"job":"42"}"#, &format!("state-{index}")))
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert!(
            verdicts.iter().all(|verdict| *verdict == Verdict::Continue),
            "a result that moves on restarts the run, so it is neither reminded nor stopped: {verdicts:?}"
        );
    }

    #[test]
    fn loop_guard_the_same_result_repeated_eight_times_still_stops_at_eight() {
        let arguments = r#"{"url":"https://same.example/page"}"#;
        let mut guard = aware();
        let calls: Vec<CallRecord> = (0..STOP_AT)
            .map(|_| answered("fetch_url", arguments, "same-digest"))
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert_eq!(
            verdicts[2],
            Verdict::Remind {
                level: 1,
                text: REMIND_FIRST
            }
        );
        assert_eq!(
            verdicts[4],
            Verdict::Remind {
                level: 2,
                text: REMIND_SECOND
            }
        );
        let Verdict::Stop(stop) = &verdicts[7] else {
            panic!(
                "the eighth identical result must stop the loop, got {:?}",
                verdicts[7]
            );
        };
        assert_eq!(stop.reason, StopReason::Repeat);
        assert_eq!(stop.count, STOP_AT);
        assert_eq!(stop.signature_hash, call_signature("fetch_url", arguments));
    }

    #[test]
    fn loop_guard_a_changed_result_restarts_the_reminder_ladder() {
        let arguments = r#"{"url":"https://poll.example/page"}"#;
        let digest = |index: u32| {
            let state = if index < 5 { "first" } else { "second" };
            Some(state.to_owned())
        };
        let verdicts = verdicts_for(&mut aware(), arguments, 8, digest);
        assert_eq!(
            verdicts[2],
            Verdict::Remind {
                level: 1,
                text: REMIND_FIRST
            },
            "the third identical result reminds"
        );
        assert_eq!(
            verdicts[4],
            Verdict::Remind {
                level: 2,
                text: REMIND_SECOND
            },
            "the fifth identical result reminds again"
        );
        assert_eq!(
            verdicts[5],
            Verdict::Continue,
            "the result changed: a new run starts"
        );
        assert_eq!(verdicts[6], Verdict::Continue);
        assert_eq!(
            verdicts[7],
            Verdict::Remind {
                level: 1,
                text: REMIND_FIRST
            },
            "the third call of the new run reminds again, at the first level"
        );
    }

    #[test]
    fn loop_guard_a_polling_pair_whose_results_change_is_not_a_cycle() {
        let mut guard = aware();
        let mut calls = Vec::new();
        for round in 0..6 {
            calls.push(answered(
                "job_status",
                r#"{"job":"42"}"#,
                &format!("status-{round}"),
            ));
            calls.push(answered(
                "job_progress",
                r#"{"job":"43"}"#,
                &format!("progress-{round}"),
            ));
        }
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert!(
            verdicts.iter().all(|verdict| *verdict == Verdict::Continue),
            "a pair polling changing results is progress, not a cycle: {verdicts:?}"
        );
    }

    #[test]
    fn loop_guard_a_pair_repeating_the_same_results_is_still_a_cycle() {
        let mut guard = aware();
        let a = answered("web_search", r#"{"query":"alpha"}"#, "result-a");
        let b = answered("web_search", r#"{"query":"beta"}"#, "result-b");
        let calls = [
            a.clone(),
            b.clone(),
            a.clone(),
            b.clone(),
            a.clone(),
            b.clone(),
        ];
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert_eq!(
            verdicts[5],
            Verdict::Stop(LoopStop {
                reason: StopReason::Cycle,
                tool: "web_search".to_owned(),
                signature_hash: b.signature,
                count: 6,
            })
        );
    }

    #[test]
    fn loop_guard_one_call_cycling_through_the_same_results_stops_by_its_bare_signature() {
        let arguments = r#"{"url":"https://flip.example/state"}"#;
        let mut guard = aware();
        let calls: Vec<CallRecord> = (0..8)
            .map(|index| {
                let digest = if index % 2 == 0 {
                    "state-on"
                } else {
                    "state-off"
                };
                answered("fetch_url", arguments, digest)
            })
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert!(verdicts[..6]
            .iter()
            .all(|verdict| !matches!(verdict, Verdict::Stop(_))));
        let Verdict::Stop(stop) = &verdicts[6] else {
            panic!(
                "the results recur in the same order, which is a cycle: {:?}",
                verdicts[6]
            );
        };
        assert_eq!(stop.reason, StopReason::Cycle);
        assert_eq!(stop.tool, "fetch_url");
        assert_eq!(
            stop.signature_hash,
            call_signature("fetch_url", arguments),
            "the stop names the call's signature, never a digest"
        );
    }

    #[test]
    fn loop_guard_the_no_progress_rule_is_unchanged_with_result_awareness_on() {
        let mut guard = aware();
        let rounds = [
            vec![call(
                "fetch_url",
                r#"{"url":"https://a.example"}"#,
                CallOutcome::Err,
            )],
            vec![call("web_search", r#"{"query":"b"}"#, CallOutcome::Err)],
            vec![call(
                "knowledge_search",
                r#"{"query":"c"}"#,
                CallOutcome::Suppressed,
            )],
            vec![call(
                "fetch_url",
                r#"{"url":"https://d.example"}"#,
                CallOutcome::Err,
            )],
        ];
        let verdicts: Vec<Verdict> = rounds
            .iter()
            .map(|round| guard.observe_round(round))
            .collect();
        let Verdict::Stop(stop) = &verdicts[3] else {
            panic!(
                "four stalled rounds must stop the loop, got {:?}",
                verdicts[3]
            );
        };
        assert_eq!(stop.reason, StopReason::NoProgress);
        assert_eq!(stop.count, 4);
    }

    #[test]
    fn loop_guard_result_digest_is_a_short_hash_of_the_result_and_never_the_result() {
        let digest = result_digest("Bergen: 11 degrees, clear");
        assert_eq!(digest.len(), 16);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(
            digest,
            result_digest("Bergen: 11 degrees, clear"),
            "the same result digests the same"
        );
        assert_ne!(digest, result_digest("Bergen: 12 degrees, clear"));
        assert!(!digest.contains("Bergen"), "{digest}");
        assert_eq!(
            result_digest("x"),
            &blake3::hash(b"x").to_hex()[..16],
            "the digest is the hash call_signature uses, cut to 16 characters"
        );
    }

    #[test]
    fn loop_guard_result_awareness_reads_only_the_literal_true() {
        assert!(result_aware_from_env_value(Some("true")));
        for value in [
            None,
            Some(""),
            Some("TRUE"),
            Some("True"),
            Some("1"),
            Some("on"),
            Some("yes"),
            Some("false"),
            Some(" true "),
        ] {
            assert!(
                !result_aware_from_env_value(value),
                "{value:?} must read as off, as every literal switch reads it"
            );
        }
    }

    /// The answer of a status poll: the same status, with the server's clock, a request id and a timing stamped
    /// on it. Only the stamps differ between two polls.
    fn stamped_status(index: u32, status: &str) -> String {
        format!(
            r#"{{"status":"{status}","checked_at":"2026-11-21T10:00:{index:02}Z","request_id":"req-fixture-{index:04}","elapsed_ms":{}}}"#,
            100 + index
        )
    }

    #[test]
    fn loop_guard_a_repeat_stamped_on_every_answer_still_reminds_and_stops() {
        let arguments = r#"{"job":"42"}"#;
        let outputs: Vec<String> = (0..STOP_AT)
            .map(|index| stamped_status(index, "pending"))
            .collect();
        assert!(
            outputs.windows(2).all(|pair| pair[0] != pair[1]),
            "every poll is answered with new stamps"
        );
        let calls: Vec<CallRecord> = outputs
            .iter()
            .map(|output| answered("job_status", arguments, &result_digest(output)))
            .collect();
        let mut guard = aware();
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert_eq!(
            verdicts[2],
            Verdict::Remind {
                level: 1,
                text: REMIND_FIRST
            },
            "the third identical answer reminds"
        );
        assert_eq!(
            verdicts[4],
            Verdict::Remind {
                level: 2,
                text: REMIND_SECOND
            },
            "the fifth identical answer reminds again"
        );
        assert_eq!(
            verdicts[7],
            Verdict::Stop(LoopStop {
                reason: StopReason::Repeat,
                tool: "job_status".to_owned(),
                signature_hash: calls[7].signature.clone(),
                count: STOP_AT,
            }),
            "the eighth identical answer stops the loop, though every poll carried new stamps"
        );
    }

    #[test]
    fn loop_guard_stamped_answers_are_judged_as_bare_ones_with_result_awareness_off() {
        let arguments = r#"{"job":"42"}"#;
        let stamped: Vec<CallRecord> = (0..STOP_AT)
            .map(|index| {
                let digest = result_digest(&stamped_status(index, "pending"));
                answered("job_status", arguments, &digest)
            })
            .collect();
        let bare: Vec<CallRecord> = (0..STOP_AT)
            .map(|_| call("job_status", arguments, CallOutcome::Ok))
            .collect();
        assert_eq!(
            one_call_per_round(&mut LoopGuard::new(), &stamped),
            one_call_per_round(&mut LoopGuard::new(), &bare),
            "with the switch off a digest is ignored, stamps and all"
        );
    }

    #[test]
    fn loop_guard_a_cycle_stamped_on_every_answer_still_stops() {
        let mut guard = aware();
        let calls: Vec<CallRecord> = (0..6)
            .map(|index| {
                let arguments = if index % 2 == 0 {
                    r#"{"query":"alpha"}"#
                } else {
                    r#"{"query":"beta"}"#
                };
                let output = format!(
                    r#"{{"hits":2,"fetched_at":"2026-11-21T10:00:{index:02}Z","request_id":"req-fixture-{index:04}"}}"#
                );
                answered("web_search", arguments, &result_digest(&output))
            })
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        assert!(verdicts[..5]
            .iter()
            .all(|verdict| !matches!(verdict, Verdict::Stop(_))));
        assert_eq!(
            verdicts[5],
            Verdict::Stop(LoopStop {
                reason: StopReason::Cycle,
                tool: "web_search".to_owned(),
                signature_hash: calls[5].signature.clone(),
                count: 6,
            }),
            "the same two answers recur in order, so the pair is a cycle"
        );
    }

    #[test]
    fn loop_guard_a_parked_result_read_again_under_a_new_handle_is_a_repeat() {
        let arguments = r#"{"url":"https://big.example/report"}"#;
        let mut guard = aware();
        let calls: Vec<CallRecord> = (0..STOP_AT)
            .map(|index| {
                let envelope = format!(
                    r#"{{"handle_id":"res_fixture-{index:04}","capability_id":"mcp__reports__fetch","size_bytes":52000,"summary":"JSON array of 300 rows","row_count":300,"expires_at":"2026-11-21T10:{index:02}:00.123456789+00:00","kind":"json","note":"The COMPLETE result is held under this handle."}}"#
                );
                answered("fetch_url", arguments, &result_digest(&envelope))
            })
            .collect();
        let verdicts = one_call_per_round(&mut guard, &calls);
        let Verdict::Stop(stop) = &verdicts[7] else {
            panic!(
                "the eighth read of one result must stop the loop, got {:?}",
                verdicts[7]
            );
        };
        assert_eq!(stop.reason, StopReason::Repeat);
        assert_eq!(stop.count, STOP_AT);
    }

    #[test]
    fn loop_guard_result_digest_leaves_out_the_stamps_and_keeps_the_answer() {
        let first = r#"{"status":"pending","checked_at":"2026-11-21T10:15:30Z","request_id":"req-fixture-0001","elapsed_ms":120}"#;
        let later = r#"{"elapsed_ms":4800,"request_id":"req-fixture-0002","checked_at":"2026-11-21T10:16:02.5+01:00","status":"pending"}"#;
        assert_eq!(
            result_digest(first),
            result_digest(later),
            "key order, stamps and timings are not the answer"
        );
        assert_ne!(
            result_digest(first),
            result_digest(&first.replace("pending", "done")),
            "a changed status is progress"
        );
        assert_ne!(
            result_digest(r#"{"count":3}"#),
            result_digest(r#"{"count":4}"#),
            "a number is content"
        );
        assert_ne!(
            result_digest(r#"{"start":"2026-11-21T10:00:00+01:00"}"#),
            result_digest(r#"{"start":"2026-11-21T11:00:00+01:00"}"#),
            "a time under a key that is not a stamp is content"
        );
        assert_ne!(
            result_digest(r#"{"now":{"temp":11}}"#),
            result_digest(r#"{"now":{"temp":12}}"#),
            "a 'now' block that carries data is content"
        );
        assert_eq!(
            result_digest("Fetched 2026-11-21T10:15:30Z: Bergen 11 degrees"),
            result_digest("Fetched 2026-11-21 10:16:02: Bergen 11 degrees"),
            "a stamp in text is left out, whatever its separator"
        );
        assert_eq!(
            result_digest("Oslo ☀ 2026-11-21T10:15:30Z é"),
            result_digest("Oslo ☀ 2026-11-21T10:16:02Z é"),
            "multi-byte text is kept whole around a stamp"
        );
        assert_eq!(
            result_digest("Request ID: 4bf92f3577b3 Bergen 11 degrees"),
            result_digest("Request ID: 7c21ef0a9d Bergen 11 degrees"),
            "a request id in text is left out"
        );
        assert_ne!(
            result_digest("due 2026-11-21"),
            result_digest("due 2026-11-22"),
            "a date with no clock is content"
        );
        assert_ne!(
            result_digest("Bergen 11 degrees"),
            result_digest("Bergen 12 degrees"),
            "text that is not a stamp is content"
        );
    }
}
