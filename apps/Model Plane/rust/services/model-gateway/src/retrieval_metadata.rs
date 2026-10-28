//! `RetrieveResponse.retrieval_metadata` — the advisory side channel Data Plane
//! v2 attaches to every retrieval.
//!
//! Proto field 10, a `google.protobuf.Struct`, carrying two string lists:
//!
//! * `suggested_next_tools` — signal-derived hints naming which of Data Plane
//!   v2's typed retrieval endpoints is likely productive next (low rerank
//!   confidence → graph + wiki; three or more sources → contradictions; nothing
//!   retrieved → wiki + knowledge search). Advisory, never authoritative:
//!   heuristics computed from one response, and an agent stays free to ignore
//!   them. [`hinted_tool_names`] is what turns a hint into something callable.
//! * `zdr_actions_applied` — which Zero Data Retention enforcement actions this
//!   retrieval actually applied. Not a convenience: a caller that cannot
//!   observe enforcement cannot propagate it, and ZDR has to survive every
//!   content-carrying boundary.
//!
//! Data Plane v2 computed both on every request and dropped them at the gRPC
//! boundary until 2026-08-27; the gateway then ignored the field for as long as
//! it existed. Decoding here is deliberately total — a `Struct` that is absent,
//! empty, or the wrong shape yields empty lists and never an error, because a
//! malformed advisory payload must not cost anyone their search results.

use serde::Serialize;

/// Data Plane v2 endpoint path → the gateway tool that actually reaches it.
///
/// Only pairs where the tool EXISTS in [`crate::tool_loop::builtin_tool_defs`]
/// belong here. A hint naming an endpoint the model has no tool for is worse
/// than no hint: it invites a tool call that comes back "unknown tool", which
/// reads to the model as a broken environment rather than a missing capability.
/// `tool_loop`'s `every_hinted_tool_is_actually_advertised` test is what keeps
/// that true as the tool list changes.
const HINT_TO_TOOL: &[(&str, &str)] = &[
    ("/v1/knowledge/search", "knowledge_search"),
    ("/v1/retrieve/graph", "knowledge_graph_search"),
    ("/v1/retrieve/wiki", "knowledge_wiki_search"),
    ("/v1/retrieve/contradictions", "knowledge_contradictions"),
];

/// The Data Plane notice code of a ZDR answer whose dense arm degraded to lexical search (Z-41).
pub const ZDR_DENSE_UNAVAILABLE: &str = "zdr_dense_unavailable";

/// The code of a reduced answer whose reason this gateway has no wording for. An unknown reason is
/// still reported as a reduction: dropping it would tell the user a reduced answer was complete.
pub const RETRIEVAL_REDUCED: &str = "retrieval_reduced";

/// A typed statement that a retrieval answered in a reduced form, for the user to read (Z-41b).
///
/// Only the reason and the key the client files its wording under cross the gateway. Data Plane's
/// English sentence does not: the wording is the client's, in each language it supports, and a
/// sentence about the retention posture must not travel as free text. Every value comes from
/// [`NOTICES`], so a notice can carry nothing but a catalogued code and its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RetrievalNotice {
    /// The stable reason. A client may branch on it.
    pub code: &'static str,
    /// The key the client's wording for this notice is filed under.
    pub wording_key: &'static str,
}

/// The generic notice: a reduced answer whose reason is not one this gateway words.
const REDUCED: RetrievalNotice = RetrievalNotice {
    code: RETRIEVAL_REDUCED,
    wording_key: "retrieval.reduced",
};

/// Every notice the gateway can state. The generic one is the fallback for any other reason.
const NOTICES: &[RetrievalNotice] = &[
    RetrievalNotice {
        code: ZDR_DENSE_UNAVAILABLE,
        wording_key: "retrieval.zdr_dense_unavailable",
    },
    REDUCED,
];

/// The notice for a Data Plane reason: its own entry when the gateway words it, otherwise the generic
/// reduced notice. `None` only for a blank reason, which says nothing at all.
#[must_use]
pub fn notice_for_code(code: &str) -> Option<RetrievalNotice> {
    let code = code.trim();
    if code.is_empty() {
        return None;
    }
    Some(
        NOTICES
            .iter()
            .copied()
            .find(|notice| notice.code == code)
            .unwrap_or(REDUCED),
    )
}

/// Whether a retrieval sent with this Data Plane `zdr_mode` may carry its notice to the user. Only a
/// restrictive mode may. A persistent retrieval is never told its answer was reduced under zero
/// retention, because that statement would be false for it.
#[must_use]
pub fn notice_applies_to(zdr_mode: Option<&str>) -> bool {
    matches!(zdr_mode, Some("ephemeral" | "reject"))
}

/// The notice a retrieval sent with this Data Plane `zdr_mode` may state to the user, if any (Z-41b).
///
/// A ZDR retrieval (`ephemeral`) states the typed ZDR notice. A waived retrieval (`reject`) runs persistent
/// for its connection, so the zero-retention wording would be false for it: it is told only that its answer
/// was reduced, under the generic code. A persistent retrieval states nothing.
#[must_use]
pub fn notice_for_mode(notice: RetrievalNotice, zdr_mode: Option<&str>) -> Option<RetrievalNotice> {
    match zdr_mode {
        Some("ephemeral") => Some(notice),
        Some("reject") => Some(REDUCED),
        _ => None,
    }
}

/// The decoded advisory payload. Both lists empty is the overwhelmingly common
/// case (a confident retrieval with ZDR off), and is indistinguishable from an
/// absent field on purpose — Data Plane v2 omits the whole `Struct` then, so
/// the wire stays byte-identical to before it started sending one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalMetadata {
    /// Raw Data Plane endpoint paths, exactly as sent. Kept unmapped so the
    /// hint is still legible when no gateway tool covers it — that is a real
    /// coverage gap worth being able to see, not something to silently drop
    /// before it is recorded.
    pub suggested_next_tools: Vec<String>,
    /// ZDR enforcement actions Data Plane v2 applied to THIS result set (e.g.
    /// `reject_mode_filtered_restricted`, `ephemeral_no_trace_persist`).
    pub zdr_actions_applied: Vec<String>,
    /// The reduced-answer notice, when Data Plane produced this answer in a reduced form (Z-41).
    /// What the caller's turn may show of it is decided by [`notice_for_mode`], not here.
    pub notice: Option<RetrievalNotice>,
}

impl RetrievalMetadata {
    /// True when Data Plane v2 said nothing at all — the normal case.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.suggested_next_tools.is_empty()
            && self.zdr_actions_applied.is_empty()
            && self.notice.is_none()
    }

    /// The subset of [`Self::suggested_next_tools`] the model can actually call,
    /// as gateway tool names. See [`hinted_tool_names`].
    #[must_use]
    pub fn hinted_tools(&self) -> Vec<&'static str> {
        hinted_tool_names(&self.suggested_next_tools)
    }
}

/// Decode the `retrieval_metadata` `Struct` off a `RetrieveResponse`.
///
/// Total by construction: absent field, absent key, a value that is not a list,
/// or a list element that is not a string all reduce to "nothing said". Order is
/// preserved (Data Plane emits these in priority order and de-duplicates them)
/// and blank entries are dropped.
#[must_use]
pub fn from_struct(metadata: Option<&prost_types::Struct>) -> RetrievalMetadata {
    let Some(metadata) = metadata else {
        return RetrievalMetadata::default();
    };
    RetrievalMetadata {
        suggested_next_tools: string_list(metadata, "suggested_next_tools"),
        zdr_actions_applied: string_list(metadata, "zdr_actions_applied"),
        notice: notice_from_struct(metadata),
    }
}

/// The `notice` entry Data Plane writes as a `{code, message}` struct. Only the code is read: the
/// message is Data Plane's English and is never forwarded. A notice with no usable code says nothing.
fn notice_from_struct(metadata: &prost_types::Struct) -> Option<RetrievalNotice> {
    use prost_types::value::Kind;
    let Some(prost_types::Value {
        kind: Some(Kind::StructValue(notice)),
    }) = metadata.fields.get("notice")
    else {
        return None;
    };
    let Some(prost_types::Value {
        kind: Some(Kind::StringValue(code)),
    }) = notice.fields.get("code")
    else {
        return None;
    };
    notice_for_code(code)
}

fn string_list(metadata: &prost_types::Struct, key: &str) -> Vec<String> {
    use prost_types::value::Kind;
    let Some(prost_types::Value {
        kind: Some(Kind::ListValue(list)),
    }) = metadata.fields.get(key)
    else {
        return Vec::new();
    };
    list.values
        .iter()
        .filter_map(|value| match &value.kind {
            Some(Kind::StringValue(text)) => Some(text.trim()).filter(|t| !t.is_empty()),
            _ => None,
        })
        .map(str::to_owned)
        .collect()
}

/// Map Data Plane endpoint hints onto gateway tool names, dropping anything the
/// gateway cannot serve and de-duplicating while preserving order.
///
/// Advisory in, advisory out: this decides only what is *offerable*, never what
/// the model must do next.
#[must_use]
pub fn hinted_tool_names(suggested: &[String]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for hint in suggested {
        let hint = hint.trim();
        if let Some((_, tool)) = HINT_TO_TOOL.iter().find(|(path, _)| *path == hint) {
            if !out.contains(tool) {
                out.push(tool);
            }
        }
    }
    out
}

/// Every gateway tool a Data Plane hint can name. Used by `tool_loop`'s
/// coverage test to prove each one is really advertised to the model.
#[must_use]
pub fn every_hintable_tool() -> Vec<&'static str> {
    HINT_TO_TOOL.iter().map(|(_, tool)| *tool).collect()
}

/// True when Data Plane v2 applied a ZDR enforcement action to content this
/// turn is about to retain durably.
///
/// Expected to be unreachable today, and written as a check rather than an
/// assumption on purpose. The gateway only ever asks Data Plane for `ephemeral`
/// when `effective_zdr` is already true, and a ZDR turn takes the
/// persistence-free path — so the two postures cannot currently disagree. That
/// is a property of two call sites lining up, not an invariant anything
/// enforces: the HTTP retrieval proxy already accepts a caller-supplied
/// `zdr_mode: "reject"` on a non-ZDR credential, and any future re-route of the
/// ZDR branch would break it silently. Same reasoning as the `!effective_zdr`
/// re-check that guards durable thread titles.
#[must_use]
pub fn retention_posture_conflict(
    turn_retains_durably: bool,
    zdr_actions_applied: &[String],
) -> bool {
    turn_retains_durably && !zdr_actions_applied.is_empty()
}

#[cfg(test)]
mod tests {
    use super::{
        every_hintable_tool, from_struct, hinted_tool_names, notice_applies_to,
        notice_for_mode, retention_posture_conflict, RetrievalMetadata, RetrievalNotice,
    };
    use prost_types::{ListValue, Struct, Value};

    /// The `retrieval_metadata` a Data Plane answer carries when it degraded under ZDR: a `notice`
    /// struct of `{code, message}`, as `with_notice` writes it.
    fn notice_struct(code: &str, message: &str) -> Struct {
        let body = Struct {
            fields: [
                ("code".to_owned(), string_value(code)),
                ("message".to_owned(), string_value(message)),
            ]
            .into_iter()
            .collect(),
        };
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "notice".to_owned(),
            Value {
                kind: Some(prost_types::value::Kind::StructValue(body)),
            },
        );
        Struct { fields }
    }

    /// A ZDR answer that degraded to lexical search decodes to its typed code and wording key.
    #[test]
    fn a_degraded_answers_notice_decodes_to_its_code_and_wording_key() {
        let decoded = from_struct(Some(&notice_struct(
            "zdr_dense_unavailable",
            "Semantic search was not used for this answer.",
        )));
        assert_eq!(
            decoded.notice,
            Some(RetrievalNotice {
                code: "zdr_dense_unavailable",
                wording_key: "retrieval.zdr_dense_unavailable",
            })
        );
        assert!(!decoded.is_empty(), "a notice alone is something to say");
    }

    /// Data Plane's sentence is its own English and is never the client's text. What crosses the
    /// gateway is the code and the key, and nothing else a message could carry.
    #[test]
    fn the_data_plane_sentence_never_crosses_the_gateway() {
        let message = "Semantic search was not used: zero data retention is on. SECRET-DOC-TEXT";
        let decoded = from_struct(Some(&notice_struct("zdr_dense_unavailable", message)))
            .notice
            .expect("the notice decodes");
        let wire = serde_json::to_string(&decoded).expect("the notice serializes");
        assert_eq!(
            wire,
            r#"{"code":"zdr_dense_unavailable","wording_key":"retrieval.zdr_dense_unavailable"}"#
        );
        assert!(!wire.contains("SECRET"), "{wire}");
    }

    /// A reason this gateway does not word is reported as a reduction, never dropped.
    #[test]
    fn an_unknown_reason_is_still_reported_as_a_reduction() {
        let decoded = from_struct(Some(&notice_struct("some_future_reason", "text")));
        assert_eq!(
            decoded
                .notice
                .map(|notice| (notice.code, notice.wording_key)),
            Some(("retrieval_reduced", "retrieval.reduced"))
        );
    }

    /// A notice with no usable code says nothing, and neither does a `notice` that is not a struct.
    #[test]
    fn a_notice_without_a_usable_code_says_nothing() {
        assert!(from_struct(Some(&notice_struct("   ", "text")))
            .notice
            .is_none());
        assert!(from_struct(Some(&notice_struct("", "text")))
            .notice
            .is_none());
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("notice".to_owned(), string_value("zdr_dense_unavailable"));
        assert!(from_struct(Some(&Struct { fields })).notice.is_none());
    }

    /// An ordinary answer, and a ZDR answer that did not degrade, carries no notice.
    #[test]
    fn an_ordinary_answer_carries_no_notice() {
        assert!(from_struct(None).notice.is_none());
        let decoded = from_struct(Some(&list_field(
            "zdr_actions_applied",
            &["ephemeral_no_trace_persist"],
        )));
        assert!(decoded.notice.is_none());
        assert!(
            !decoded.is_empty(),
            "the actions still count as something said"
        );
    }

    /// Only a restrictive retrieval may carry its notice. A persistent one never does.
    #[test]
    fn only_a_restrictive_retrieval_may_carry_a_notice() {
        assert!(notice_applies_to(Some("ephemeral")));
        assert!(notice_applies_to(Some("reject")));
        assert!(!notice_applies_to(None));
        assert!(!notice_applies_to(Some("disabled")));
        assert!(!notice_applies_to(Some("unknown")));
    }

    /// What a notice may say depends on its mode. A ZDR retrieval states its own notice. A waived one runs
    /// persistent for its connection, so it is told only that its answer was reduced, under the generic code.
    /// A persistent one is told nothing.
    #[test]
    fn a_notice_states_only_what_its_mode_may_say() {
        let zdr_notice = RetrievalNotice {
            code: "zdr_dense_unavailable",
            wording_key: "retrieval.zdr_dense_unavailable",
        };
        assert_eq!(
            notice_for_mode(zdr_notice, Some("ephemeral")),
            Some(zdr_notice)
        );
        assert_eq!(
            notice_for_mode(zdr_notice, Some("reject")),
            Some(RetrievalNotice {
                code: "retrieval_reduced",
                wording_key: "retrieval.reduced",
            })
        );
        assert_eq!(notice_for_mode(zdr_notice, None), None);
        assert_eq!(notice_for_mode(zdr_notice, Some("disabled")), None);
    }

    fn string_value(text: &str) -> Value {
        Value {
            kind: Some(prost_types::value::Kind::StringValue(text.to_owned())),
        }
    }

    fn list_field(key: &str, items: &[&str]) -> Struct {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            key.to_owned(),
            Value {
                kind: Some(prost_types::value::Kind::ListValue(ListValue {
                    values: items.iter().map(|item| string_value(item)).collect(),
                })),
            },
        );
        Struct { fields }
    }

    /// The shape observed live on 2026-08-27 after the Data Plane fix:
    /// `{"suggested_next_tools": ["/v1/retrieve/contradictions"]}`.
    #[test]
    fn the_live_response_shape_decodes_to_a_callable_tool() {
        let decoded = from_struct(Some(&list_field(
            "suggested_next_tools",
            &["/v1/retrieve/contradictions"],
        )));
        assert_eq!(
            decoded.suggested_next_tools,
            vec!["/v1/retrieve/contradictions".to_owned()]
        );
        assert_eq!(decoded.hinted_tools(), vec!["knowledge_contradictions"]);
        assert!(decoded.zdr_actions_applied.is_empty());
        assert!(!decoded.is_empty());
    }

    /// An absent `Struct` is the normal case for a confident retrieval with ZDR
    /// off, and must be silent rather than an error.
    #[test]
    fn an_absent_struct_says_nothing() {
        assert_eq!(from_struct(None), RetrievalMetadata::default());
        assert!(from_struct(None).is_empty());
    }

    /// Advisory data must never be able to break retrieval. Every malformed
    /// shape reduces to "nothing said".
    #[test]
    fn a_malformed_struct_degrades_to_silence_instead_of_failing() {
        let mut fields = std::collections::BTreeMap::new();
        // Right key, wrong kind: a bare string where a list belongs.
        fields.insert(
            "suggested_next_tools".to_owned(),
            string_value("/v1/retrieve/graph"),
        );
        // Right kind, junk elements: numbers, nulls and blanks.
        fields.insert(
            "zdr_actions_applied".to_owned(),
            Value {
                kind: Some(prost_types::value::Kind::ListValue(ListValue {
                    values: vec![
                        Value {
                            kind: Some(prost_types::value::Kind::NumberValue(7.0)),
                        },
                        Value { kind: None },
                        string_value("   "),
                        string_value("  ephemeral_no_trace_persist  "),
                    ],
                })),
            },
        );
        let decoded = from_struct(Some(&Struct { fields }));
        assert!(decoded.suggested_next_tools.is_empty());
        assert_eq!(
            decoded.zdr_actions_applied,
            vec!["ephemeral_no_trace_persist".to_owned()],
            "usable entries survive; junk is dropped, not fatal"
        );
    }

    /// A hint the gateway has no tool for is dropped rather than passed through
    /// as a tool name the model would fail to call. It stays visible in
    /// `suggested_next_tools`, which is where a coverage gap should show up.
    #[test]
    fn an_unmapped_hint_is_dropped_not_invented() {
        let hints = [
            "/v1/retrieve/timeline".to_owned(),
            "/v1/retrieve/graph".to_owned(),
            "/v1/retrieve/freshness".to_owned(),
        ];
        assert_eq!(hinted_tool_names(&hints), vec!["knowledge_graph_search"]);
        assert!(hinted_tool_names(&["".to_owned(), "  ".to_owned()]).is_empty());
    }

    /// Data Plane already de-duplicates, but order and uniqueness of the mapped
    /// output are this function's contract, not a downstream assumption: two
    /// endpoints could map to one tool.
    #[test]
    fn mapped_hints_keep_first_occurrence_order_and_stay_unique() {
        let hints = [
            "/v1/retrieve/wiki".to_owned(),
            "/v1/retrieve/graph".to_owned(),
            "/v1/retrieve/wiki".to_owned(),
        ];
        assert_eq!(
            hinted_tool_names(&hints),
            vec!["knowledge_wiki_search", "knowledge_graph_search"]
        );
    }

    #[test]
    fn every_hintable_tool_is_listed_once() {
        let tools = every_hintable_tool();
        let unique: std::collections::BTreeSet<_> = tools.iter().collect();
        assert_eq!(tools.len(), unique.len(), "duplicate tool in HINT_TO_TOOL");
        assert!(tools.contains(&"knowledge_search"));
    }

    /// The compliance check: enforcement reported on a durable turn is a
    /// divergence. Everything else is not.
    #[test]
    fn only_enforcement_on_a_durable_turn_counts_as_a_conflict() {
        let applied = ["ephemeral_no_trace_persist".to_owned()];
        assert!(retention_posture_conflict(true, &applied));
        assert!(
            !retention_posture_conflict(false, &applied),
            "a ZDR turn taking the persistence-free path is the posture working"
        );
        assert!(!retention_posture_conflict(true, &[]));
        assert!(!retention_posture_conflict(false, &[]));
    }
}
