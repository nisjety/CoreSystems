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

impl LoopGuardMode {
    /// Parses the raw `LOOP_GUARD_MODE` value, if it is set.
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            None | Some("" | "off") => Self::Off,
            Some("enforce") => Self::Enforce,
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
    /// The latest calls as (tool, signature), for the cycle check.
    recent: VecDeque<(String, String)>,
    /// The latest rounds, for the stall check.
    rounds: VecDeque<Round>,
}

#[derive(Debug, Default)]
struct Repeats {
    count: u32,
    /// The highest reminder level already given for this call.
    reminded: u8,
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

    /// Observes one round's calls, in the order the model asked for them, and
    /// returns the verdict for the loop.
    ///
    /// A round with no calls changes nothing and continues. A stop takes
    /// precedence over a reminder in the same round.
    pub fn observe_round(&mut self, calls: &[CallRecord]) -> Verdict {
        if calls.is_empty() {
            return Verdict::Continue;
        }
        let mut reminder: u8 = 0;
        let mut repeat_stop = None;
        for call in calls {
            let repeats = self.repeats.entry(call.signature.clone()).or_default();
            repeats.count += 1;
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
            self.recent
                .push_back((call.tool.clone(), call.signature.clone()));
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
            let signature_at = |index: usize| &self.recent[start + index].1;
            let block: HashSet<&String> = (0..period).map(signature_at).collect();
            let repeats =
                (period..span).all(|index| signature_at(index) == signature_at(index - period));
            if block.len() > 1 && repeats {
                let (tool, signature) = &self.recent[len - 1];
                return Some(LoopStop {
                    reason: StopReason::Cycle,
                    tool: tool.clone(),
                    signature_hash: signature.clone(),
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
        let (tool, signature) = self.recent.back()?;
        let covered: usize = self.rounds.iter().map(|round| round.signatures.len()).sum();
        Some(LoopStop {
            reason: StopReason::NoProgress,
            tool: tool.clone(),
            signature_hash: signature.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn call(tool: &str, arguments: &str, outcome: CallOutcome) -> CallRecord {
        CallRecord {
            tool: tool.to_owned(),
            signature: call_signature(tool, arguments),
            outcome,
        }
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
        for unknown in ["Enforce", "true", "1", "on", "enforce-all"] {
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
}
