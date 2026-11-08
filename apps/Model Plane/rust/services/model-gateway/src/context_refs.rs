//! Server-side resolution of a send's typed context references (Wave 3 EP-3 S5).
//!
//! The composer attaches references as a list next to the text (`InvokeRequest.context_refs`).
//! The shape and its bounds are `mp_contracts::context_refs`; this module decides, before any run
//! or model call, what each kind does for this turn and when the turn is refused:
//!
//! * `document`: added to the turn's `knowledge_search` filter as `document_ids`. The gateway never
//!   reads the document; the Data Plane applies the viewer's own ACL to the narrowed search.
//! * `space`: must equal the thread's existing Space, or the turn is refused. It never widens a
//!   scope: Space access never widens an owner-plane ACL.
//! * `skill`: merged into the turn's explicit skill picks, which the catalogue resolves as before.
//! * `thread`: admitted only while the thread reference tools (EP-3 S7) are on, so a reference is
//!   never accepted and then silently ignored; the tools read it lazily. A waived turn and a
//!   conversation-only or web-only turn are refused.
//! * `artifact_selection`: admitted only with the selection edit on (`TurnFacts::selection_edit`) and
//!   outside a Space. Its range is checked against the head, and held for the run, by `selection_guard`
//!   once the turn's heads are seeded (EP-3 S6).
//!
//! A Do turn takes `document`, `skill` and `space` only. A ZDR turn with any reference is refused
//! with the catalogue's `context_references` feature (`zdr_features` refuses it too, so the answer
//! is the same whichever check a turn reaches first).
//!
//! Off by default: `MODEL_GATEWAY_CONTEXT_REFS_ENABLED` must be the literal `true`. With it off the
//! field is never read and a non-ZDR turn is the one it was before the field existed.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use mp_contracts::context_refs::{
    validate_context_refs, ArtifactSelection, ContextRef, ContextRefKind,
};
use mp_contracts::dataplane::retrieval_v2::Filters;
use mp_contracts::zdr_refusal::Refusal;
use serde::Deserialize;
use serde_json::{json, Value};

/// The switch. Only the literal `true` turns resolution on.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_CONTEXT_REFS_ENABLED";
/// The ZDR catalogue feature a turn with references is refused with.
pub const ZDR_FEATURE: &str = "context_references";

/// Whether the switch is on, from its raw value.
#[must_use]
pub fn parse_enabled(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// The switch as this process reads it.
#[must_use]
pub fn enabled() -> bool {
    parse_enabled(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The catalogue feature a chat model lists while the switch is on (`features` on `GET /v1/models`).
/// The composer shows its reference chips only where the catalogue says the gateway resolves them.
pub const CATALOGUE_FEATURE: &str = "context_refs";

/// A model's catalogue features, with [`CATALOGUE_FEATURE`] added once to a chat model while the switch
/// is on. Every other model, and every model while the switch is off, keeps its features as they were.
#[must_use]
pub fn catalogue_features(mut features: Vec<String>, modality: &str, enabled: bool) -> Vec<String> {
    if enabled
        && modality == "chat"
        && !features.iter().any(|feature| feature == CATALOGUE_FEATURE)
    {
        features.push(CATALOGUE_FEATURE.to_owned());
    }
    features
}

/// Whether a request carries references at all: anything but an absent field, `null` or `[]`. A
/// malformed value counts, so a ZDR turn that sent one is refused rather than let through.
#[must_use]
pub fn requested(raw: Option<&Value>) -> bool {
    raw.is_some_and(|value| match value {
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        _ => true,
    })
}

/// What the resolver needs to know about the turn, all decided before it runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct TurnFacts<'a> {
    /// The verified token or route says zero retention.
    pub zdr: bool,
    /// The turn runs under a retention waiver (DEC-32).
    pub waived: bool,
    /// The thread reference tools (EP-3 S7) are switched on, so a thread reference can be read.
    pub thread_references: bool,
    /// A Do (`agentic`) turn.
    pub agentic: bool,
    /// Organisation knowledge is searched and skills are matched on this turn (it is neither
    /// conversation-only nor web-only, the two filters under which the turn reads neither).
    pub knowledge: bool,
    /// The selection edit is on for this turn: the switch, with artifact revisions `on` (EP-3 S6).
    pub selection_edit: bool,
    /// The thread's Space, from the Control-signed context the BFF injected; `None` off a Space.
    pub space_id: Option<&'a str>,
}

/// The references of an admitted turn, split by what each does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedContextRefs {
    /// Every reference as validated, labels capped, in the order sent.
    pub refs: Vec<ContextRef>,
    /// The documents the turn's knowledge search is narrowed to.
    pub document_ids: Vec<String>,
    /// The skills added to the turn's explicit picks.
    pub skill_ids: Vec<String>,
}

/// Why a turn's references were refused, before any run or model call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextRefRefusal {
    /// A ZDR turn: the catalogue's `zdr_feature_not_supported` with [`ZDR_FEATURE`].
    Zdr,
    /// A typed refusal for the stream's `error` frame.
    Typed {
        code: &'static str,
        message: &'static str,
        /// A stable reason, for logs and the `details` of an HTTP body.
        reason: &'static str,
    },
}

impl ContextRefRefusal {
    const fn typed(code: &'static str, message: &'static str, reason: &'static str) -> Self {
        Self::Typed {
            code,
            message,
            reason,
        }
    }

    /// The ZDR catalogue refusal, for [`ContextRefRefusal::Zdr`].
    #[must_use]
    pub fn zdr_refusal() -> Refusal {
        Refusal::feature_not_supported(ZDR_FEATURE)
    }

    /// The typed envelope for an HTTP body: `{error: {code, message, details}}`, details an object.
    #[must_use]
    pub fn json_body(&self) -> Value {
        match self {
            Self::Zdr => Self::zdr_refusal().json_body(),
            Self::Typed {
                code,
                message,
                reason,
            } => json!({"error": {"code": code, "message": message, "details": {"reason": reason}}}),
        }
    }
}

/// The code for a list that breaks a bound or is not a list of references.
pub const INVALID_CODE: &str = "context_refs_invalid";
/// The code for a kind this turn cannot take.
pub const UNSUPPORTED_CODE: &str = "context_ref_unsupported";
/// The code for a Space reference that is not the thread's own Space.
pub const SPACE_MISMATCH_CODE: &str = "context_ref_space_mismatch";

const INVALID_MESSAGE: &str =
    "The attached references could not be read. Remove them and attach them again.";

#[derive(Deserialize)]
struct WireSelection {
    #[serde(alias = "artifactId")]
    artifact_id: String,
    version: u32,
    start: u32,
    end: u32,
    #[serde(alias = "quoteSha256")]
    quote_sha256: String,
}

#[derive(Deserialize)]
struct WireRef {
    kind: String,
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    selection: Option<WireSelection>,
}

/// The references a raw value names, validated. Every failure is the one invalid refusal: the
/// reason says which rule, the message never echoes what was sent.
fn parse(raw: &Value) -> Result<Vec<ContextRef>, ContextRefRefusal> {
    let invalid =
        |reason: &'static str| ContextRefRefusal::typed(INVALID_CODE, INVALID_MESSAGE, reason);
    let Value::Array(items) = raw else {
        return Err(invalid("not_a_list"));
    };
    // Bounded before anything is decoded, so an oversized body costs no per-item work.
    if items.len() > mp_contracts::context_refs::MAX_CONTEXT_REFS {
        return Err(invalid("too_many_refs"));
    }
    let mut refs = Vec::with_capacity(items.len());
    for item in items {
        let wire: WireRef =
            serde_json::from_value(item.clone()).map_err(|_| invalid("malformed_ref"))?;
        let kind = ContextRefKind::from_wire(&wire.kind).ok_or_else(|| invalid("unknown_kind"))?;
        refs.push(ContextRef {
            kind,
            id: wire.id,
            label: wire.label,
            selection: wire.selection.map(|selection| ArtifactSelection {
                artifact_id: selection.artifact_id,
                version: selection.version,
                start: selection.start,
                end: selection.end,
                quote_sha256: selection.quote_sha256,
            }),
        });
    }
    validate_context_refs(&refs).map_err(|error| invalid(error.reason()))
}

/// Resolves a turn's references, or refuses the turn.
///
/// `Ok(None)` when the request carries none, and when the switch is off for a non-ZDR turn (the
/// field is then never read). A ZDR turn with references is refused whatever the switch says,
/// exactly as `zdr_features::refusal` refuses it.
///
/// # Errors
///
/// The first refusal the turn's references meet: ZDR, then a malformed list, then the first
/// reference this turn cannot take, in the order sent.
pub fn resolve(
    enabled: bool,
    raw: Option<&Value>,
    facts: &TurnFacts<'_>,
) -> Result<Option<ResolvedContextRefs>, ContextRefRefusal> {
    if !requested(raw) {
        return Ok(None);
    }
    if facts.zdr {
        return Err(ContextRefRefusal::Zdr);
    }
    let Some(raw) = raw.filter(|_| enabled) else {
        return Ok(None);
    };
    let refs = parse(raw)?;
    let mut resolved = ResolvedContextRefs::default();
    for reference in &refs {
        match reference.kind {
            ContextRefKind::Thread | ContextRefKind::ArtifactSelection if facts.agentic => {
                return Err(ContextRefRefusal::typed(
                    UNSUPPORTED_CODE,
                    "Do mode can use attached documents, skills and the current Space only. Remove the other references, or ask in chat.",
                    "kind_not_in_do_mode",
                ));
            }
            // OD-7: a thread is read lazily by the thread reference tools, never injected. A waived turn reads no
            // other conversation (recall excludes waived content, DEC-32), and a conversation-only or web-only turn
            // is offered no tool that could read it.
            ContextRefKind::Thread => {
                if !facts.thread_references {
                    return Err(ContextRefRefusal::typed(
                        UNSUPPORTED_CODE,
                        "References to other conversations are not available yet. Remove them and send again.",
                        "thread_refs_unavailable",
                    ));
                }
                if facts.waived {
                    return Err(ContextRefRefusal::typed(
                        UNSUPPORTED_CODE,
                        "Other conversations cannot be referenced on this connection. Remove them and send again.",
                        "thread_refs_waived",
                    ));
                }
                if !facts.knowledge {
                    return Err(ContextRefRefusal::typed(
                        UNSUPPORTED_CODE,
                        "Referenced conversations cannot be used with a conversation-only or web-only source filter. Switch the source filter back to include knowledge.",
                        "thread_refs_need_knowledge",
                    ));
                }
            }
            // A Space keeps no artifact versions (OD-9), so a selection there is refused before the switch is read.
            ContextRefKind::ArtifactSelection => {
                if facts.space_id.is_some() {
                    return Err(ContextRefRefusal::typed(
                        UNSUPPORTED_CODE,
                        "Editing a selection is not available in a Space conversation. Remove the selection and send again.",
                        "selection_edit_in_space",
                    ));
                }
                if !facts.selection_edit {
                    return Err(ContextRefRefusal::typed(
                        UNSUPPORTED_CODE,
                        "Editing a selection is not available yet. Remove the selection and send again.",
                        "selection_edit_unavailable",
                    ));
                }
            }
            ContextRefKind::Space => {
                if facts.space_id != Some(reference.id.as_str()) {
                    return Err(ContextRefRefusal::typed(
                        SPACE_MISMATCH_CODE,
                        "Only the Space this conversation belongs to can be referenced here.",
                        "space_mismatch",
                    ));
                }
            }
            ContextRefKind::Document => {
                // A document narrows the knowledge search. A turn that searches no organisation
                // knowledge would drop it without a word, so it is refused instead.
                if !facts.knowledge {
                    return Err(ContextRefRefusal::typed(
                        UNSUPPORTED_CODE,
                        "Attached documents need organisation knowledge on for this message. Switch the source filter back to include knowledge.",
                        "documents_need_knowledge",
                    ));
                }
                resolved.document_ids.push(reference.id.clone());
            }
            ContextRefKind::Skill => {
                // Skills are matched only where knowledge is read; elsewhere the pick would vanish.
                if !facts.knowledge {
                    return Err(ContextRefRefusal::typed(
                        UNSUPPORTED_CODE,
                        "Attached skills cannot be used with a conversation-only or web-only source filter. Switch the source filter back to include knowledge.",
                        "skills_need_knowledge",
                    ));
                }
                resolved.skill_ids.push(reference.id.clone());
            }
        }
    }
    resolved.refs = refs;
    Ok(Some(resolved))
}

/// The skill ids the catalogue resolves for this turn: the explicit picks, then the referenced
/// skills not already picked. Nothing referenced leaves the picks exactly as sent.
#[must_use]
pub fn merged_skill_ids(requested: &[String], resolved: Option<&ResolvedContextRefs>) -> Vec<String> {
    let mut ids = requested.to_vec();
    for id in resolved.map_or(&[][..], |resolved| resolved.skill_ids.as_slice()) {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// The `{kind, id, label}` of each reference, for the user message's `metadata.context_refs`, so the
/// chips render after a reload. No selection range, quote or hash and no content is kept.
#[must_use]
pub fn metadata_value(refs: &[ContextRef]) -> Value {
    Value::Array(
        refs.iter()
            .map(|reference| {
                json!({
                    "kind": reference.kind.as_str(),
                    "id": reference.id,
                    "label": reference.label,
                })
            })
            .collect(),
    )
}

/// The documents each live run's knowledge search is narrowed to, by run id. An entry exists only
/// while its [`TurnDocuments`] guard is alive, so it is dropped with the turn, never persisted, and a
/// run that named no document has none (its search is the one it was).
static TURN_DOCUMENTS: LazyLock<Mutex<HashMap<String, Vec<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Holds a run's document narrowing for as long as the turn runs.
#[derive(Debug)]
#[must_use = "the narrowing is removed when the guard is dropped"]
pub struct TurnDocuments {
    run_id: Option<String>,
}

impl Drop for TurnDocuments {
    fn drop(&mut self) {
        if let Some(run_id) = self.run_id.take() {
            TURN_DOCUMENTS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&run_id);
        }
    }
}

/// Records the turn's referenced documents for its run. Nothing is recorded when the turn has none.
pub fn register_turn_documents(run_id: &str, resolved: Option<&ResolvedContextRefs>) -> TurnDocuments {
    let documents = resolved.map(|resolved| resolved.document_ids.clone()).unwrap_or_default();
    if run_id.is_empty() || documents.is_empty() {
        return TurnDocuments { run_id: None };
    }
    TURN_DOCUMENTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(run_id.to_owned(), documents);
    TurnDocuments {
        run_id: Some(run_id.to_owned()),
    }
}

/// The filter a `knowledge_search` of this run sends: `document_ids` when the turn referenced
/// documents, otherwise `None`, the request as it was.
#[must_use]
pub fn knowledge_filters(run_id: &str) -> Option<Filters> {
    let documents = TURN_DOCUMENTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(run_id)
        .cloned()?;
    Some(Filters {
        document_ids: documents,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> TurnFacts<'static> {
        TurnFacts {
            knowledge: true,
            ..TurnFacts::default()
        }
    }

    fn one(kind: &str, id: &str) -> Value {
        json!([{"kind": kind, "id": id, "label": format!("label {id}")}])
    }

    fn refused_code(result: Result<Option<ResolvedContextRefs>, ContextRefRefusal>) -> &'static str {
        match result {
            Err(ContextRefRefusal::Typed { code, .. }) => code,
            other => panic!("expected a typed refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_switch_reads_only_the_literal_true() {
        assert!(parse_enabled(Some("true")));
        assert!(parse_enabled(Some(" true ")));
        for value in [None, Some(""), Some("1"), Some("TRUE"), Some("yes"), Some("on")] {
            assert!(!parse_enabled(value), "{value:?}");
        }
    }

    #[test]
    fn a_chat_model_lists_context_refs_only_while_the_switch_is_on() {
        let listed = || vec!["tools".to_owned(), "reasoning".to_owned()];
        assert_eq!(catalogue_features(listed(), "chat", false), listed());
        assert_eq!(
            catalogue_features(listed(), "chat", true),
            vec!["tools".to_owned(), "reasoning".to_owned(), "context_refs".to_owned()]
        );
        assert_eq!(catalogue_features(listed(), "image", true), listed());
        let once = vec!["context_refs".to_owned()];
        assert_eq!(catalogue_features(once.clone(), "chat", true), once, "listed once");
    }

    #[test]
    fn flag_off_leaves_non_empty_refs_unread() {
        // The design's `context_refs_disabled` refusal is not built: with the switch off the field is
        // never read, so a non-ZDR turn is byte-identical to one sent before the field existed.
        let raw = one("document", "doc-1");
        assert_eq!(resolve(false, Some(&raw), &facts()), Ok(None));
        // A ZDR turn is refused whatever the switch says.
        let zdr = TurnFacts { zdr: true, ..facts() };
        assert_eq!(resolve(false, Some(&raw), &zdr), Err(ContextRefRefusal::Zdr));
    }

    #[test]
    fn an_absent_null_or_empty_list_is_no_request() {
        for raw in [None, Some(Value::Null), Some(json!([]))] {
            assert!(!requested(raw.as_ref()));
            let zdr = TurnFacts { zdr: true, ..facts() };
            assert_eq!(resolve(true, raw.as_ref(), &zdr), Ok(None));
        }
        assert!(requested(Some(&json!("doc-1"))), "a malformed value is still a request");
    }

    #[test]
    fn zdr_turn_with_refs_is_refused_with_context_references() {
        let zdr = TurnFacts { zdr: true, ..facts() };
        let result = resolve(true, Some(&one("document", "doc-1")), &zdr);
        assert_eq!(result, Err(ContextRefRefusal::Zdr));
        let refusal = ContextRefRefusal::zdr_refusal();
        assert_eq!(refusal.code(), mp_contracts::zdr_refusal::code::FEATURE_NOT_SUPPORTED);
        assert_eq!(refusal.feature(), Some(ZDR_FEATURE));
        assert!(mp_contracts::zdr_refusal::feature(ZDR_FEATURE).is_some());
    }

    #[test]
    fn document_ref_narrows_retrieval_filter() {
        let raw = json!([
            {"kind": "document", "id": "doc-1", "label": "Budsjett"},
            {"kind": "document", "id": "doc-2", "label": "Plan"},
        ]);
        let resolved = resolve(true, Some(&raw), &facts())
            .expect("admitted")
            .expect("references");
        assert_eq!(resolved.document_ids, vec!["doc-1", "doc-2"]);

        let run_id = "run-document-ref-narrows";
        assert_eq!(knowledge_filters(run_id), None, "no narrowing before the turn registers");
        {
            let _guard = register_turn_documents(run_id, Some(&resolved));
            let filters = knowledge_filters(run_id).expect("narrowed");
            assert_eq!(filters.document_ids, vec!["doc-1", "doc-2"]);
            assert!(filters.workspace_ids.is_empty() && filters.sources.is_empty());
        }
        assert_eq!(knowledge_filters(run_id), None, "dropped with the turn");
    }

    #[test]
    fn a_turn_without_documents_registers_nothing() {
        let run_id = "run-without-documents";
        let _guard = register_turn_documents(run_id, None);
        assert_eq!(knowledge_filters(run_id), None);
        let skills_only = ResolvedContextRefs {
            skill_ids: vec!["skill.a".to_owned()],
            ..ResolvedContextRefs::default()
        };
        let _guard = register_turn_documents(run_id, Some(&skills_only));
        assert_eq!(knowledge_filters(run_id), None);
    }

    #[test]
    fn document_ref_is_refused_when_the_turn_searches_no_knowledge() {
        let no_knowledge = TurnFacts { knowledge: false, ..facts() };
        let result = resolve(true, Some(&one("document", "doc-1")), &no_knowledge);
        assert_eq!(refused_code(result), UNSUPPORTED_CODE);
        let result = resolve(true, Some(&one("skill", "skill.a")), &no_knowledge);
        assert_eq!(refused_code(result), UNSUPPORTED_CODE);
    }

    #[test]
    fn space_ref_cannot_widen_scope() {
        let raw = one("space", "space-a");
        // Off a Space, a Space reference cannot add one.
        assert_eq!(refused_code(resolve(true, Some(&raw), &facts())), SPACE_MISMATCH_CODE);
        // In another Space it is refused too.
        let other = TurnFacts { space_id: Some("space-b"), ..facts() };
        assert_eq!(refused_code(resolve(true, Some(&raw), &other)), SPACE_MISMATCH_CODE);
        // The thread's own Space is admitted and changes nothing.
        let own = TurnFacts { space_id: Some("space-a"), ..facts() };
        let resolved = resolve(true, Some(&raw), &own).expect("admitted").expect("references");
        assert!(resolved.document_ids.is_empty() && resolved.skill_ids.is_empty());
        assert_eq!(resolved.refs.len(), 1);
    }

    #[test]
    fn skill_ref_merges_into_skill_ids() {
        let raw = json!([
            {"kind": "skill", "id": "skill.a"},
            {"kind": "skill", "id": "skill.b"},
        ]);
        let resolved = resolve(true, Some(&raw), &facts()).expect("admitted");
        let picked = vec!["skill.b".to_owned(), "skill.c".to_owned()];
        assert_eq!(
            merged_skill_ids(&picked, resolved.as_ref()),
            vec!["skill.b", "skill.c", "skill.a"]
        );
        assert_eq!(merged_skill_ids(&picked, None), picked, "no references, picks as sent");
    }

    #[test]
    fn thread_ref_refused_while_thread_tools_off() {
        let result = resolve(true, Some(&one("thread", "thread-1")), &facts());
        assert_eq!(refused_code(result), UNSUPPORTED_CODE);
        let waived = TurnFacts { waived: true, ..facts() };
        let result = resolve(true, Some(&one("thread", "thread-1")), &waived);
        assert_eq!(refused_code(result), UNSUPPORTED_CODE);
    }

    fn refused_reason(result: Result<Option<ResolvedContextRefs>, ContextRefRefusal>) -> &'static str {
        match result {
            Err(ContextRefRefusal::Typed { reason, .. }) => reason,
            other => panic!("expected a typed refusal, got {other:?}"),
        }
    }

    #[test]
    fn thread_ref_admitted_with_thread_tools_on_except_waived() {
        let on = TurnFacts { thread_references: true, ..facts() };
        let resolved = resolve(true, Some(&one("thread", "thread-1")), &on).expect("admitted").expect("references");
        assert_eq!(resolved.refs.len(), 1);
        let waived = TurnFacts { waived: true, ..on };
        assert_eq!(refused_reason(resolve(true, Some(&one("thread", "thread-1")), &waived)), "thread_refs_waived");
        let no_knowledge = TurnFacts { knowledge: false, ..on };
        assert_eq!(
            refused_reason(resolve(true, Some(&one("thread", "thread-1")), &no_knowledge)),
            "thread_refs_need_knowledge"
        );
    }

    #[test]
    fn selection_ref_refused_while_selection_edit_off() {
        let raw = json!([{
            "kind": "artifact_selection", "id": "art-1", "label": "Avsnitt",
            "selection": {"artifactId": "art-1", "version": 2, "start": 0, "end": 5, "quoteSha256": "b".repeat(64)}
        }]);
        assert_eq!(refused_code(resolve(true, Some(&raw), &facts())), UNSUPPORTED_CODE);
    }

    #[test]
    fn selection_ref_admitted_when_selection_edit_is_on() {
        let raw = json!([{
            "kind": "artifact_selection", "id": "art-1", "label": "Avsnitt",
            "selection": {"artifactId": "art-1", "version": 2, "start": 0, "end": 5, "quoteSha256": "b".repeat(64)}
        }]);
        let on = TurnFacts { selection_edit: true, ..facts() };
        let resolved = resolve(true, Some(&raw), &on).expect("admitted").expect("references");
        assert_eq!(resolved.refs.len(), 1);
        let selection = resolved.refs[0].selection.as_ref().expect("the range travels with the reference");
        assert_eq!((selection.artifact_id.as_str(), selection.version, selection.start, selection.end), ("art-1", 2, 0, 5));
        assert!(resolved.document_ids.is_empty() && resolved.skill_ids.is_empty());
    }

    #[test]
    fn selection_ref_is_refused_in_a_space_and_on_a_do_turn() {
        let raw = json!([{
            "kind": "artifact_selection", "id": "art-1", "label": "Avsnitt",
            "selection": {"artifactId": "art-1", "version": 2, "start": 0, "end": 5, "quoteSha256": "b".repeat(64)}
        }]);
        let space = TurnFacts { selection_edit: true, space_id: Some("space-a"), ..facts() };
        match resolve(true, Some(&raw), &space) {
            Err(ContextRefRefusal::Typed { code, reason, .. }) => {
                assert_eq!(code, UNSUPPORTED_CODE);
                assert_eq!(reason, "selection_edit_in_space");
            }
            other => panic!("expected the Space refusal, got {other:?}"),
        }
        let agentic = TurnFacts { selection_edit: true, agentic: true, ..facts() };
        match resolve(true, Some(&raw), &agentic) {
            Err(ContextRefRefusal::Typed { code, reason, .. }) => {
                assert_eq!(code, UNSUPPORTED_CODE);
                assert_eq!(reason, "kind_not_in_do_mode");
            }
            other => panic!("expected the Do-mode refusal, got {other:?}"),
        }
    }

    #[test]
    fn do_turn_refuses_thread_ref() {
        let agentic = TurnFacts { agentic: true, ..facts() };
        let result = resolve(true, Some(&one("thread", "thread-1")), &agentic);
        match result {
            Err(ContextRefRefusal::Typed { code, reason, .. }) => {
                assert_eq!(code, UNSUPPORTED_CODE);
                assert_eq!(reason, "kind_not_in_do_mode");
            }
            other => panic!("expected the Do-mode refusal, got {other:?}"),
        }
        // Documents, skills and the turn's own Space are taken on a Do turn.
        let own = TurnFacts { space_id: Some("space-a"), ..agentic };
        let raw = json!([
            {"kind": "document", "id": "doc-1"},
            {"kind": "skill", "id": "skill.a"},
            {"kind": "space", "id": "space-a"},
        ]);
        assert!(resolve(true, Some(&raw), &own).expect("admitted").is_some());
    }

    #[test]
    fn a_malformed_list_is_refused_typed_and_never_echoed() {
        let cases = [
            json!("doc-1"),
            json!([{"kind": "mention", "id": "x"}]),
            json!([{"kind": "document"}]),
            json!([{"kind": "document", "id": "a b"}]),
            json!([{"kind": "document", "id": "doc-1"}, {"kind": "document", "id": "doc-1"}]),
            Value::Array((0..21).map(|n| json!({"kind": "document", "id": format!("d{n}")})).collect()),
        ];
        for raw in cases {
            let result = resolve(true, Some(&raw), &facts());
            assert_eq!(refused_code(result), INVALID_CODE, "{raw}");
        }
    }

    #[test]
    fn the_http_body_is_the_typed_envelope_with_object_details() {
        let refusal = ContextRefRefusal::typed(SPACE_MISMATCH_CODE, "m", "space_mismatch");
        let body = refusal.json_body();
        assert_eq!(body["error"]["code"], SPACE_MISMATCH_CODE);
        assert!(body["error"]["details"].is_object());
        assert_eq!(ContextRefRefusal::Zdr.json_body()["error"]["feature"], ZDR_FEATURE);
    }

    #[test]
    fn metadata_stores_kind_id_label_only() {
        let raw = json!([
            {"kind": "document", "id": "doc-1", "label": format!("  {}", "x".repeat(300))},
            {"kind": "skill", "id": "skill.a", "label": "Skriv"},
        ]);
        let resolved = resolve(true, Some(&raw), &facts()).expect("admitted").expect("references");
        let metadata = metadata_value(&resolved.refs);
        let items = metadata.as_array().expect("a list");
        assert_eq!(items.len(), 2);
        for item in items {
            let mut keys: Vec<&str> = item.as_object().expect("an object").keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(keys, vec!["id", "kind", "label"]);
        }
        assert_eq!(items[0]["kind"], "document");
        assert_eq!(items[0]["label"].as_str().map(|label| label.chars().count()), Some(120));
        assert_eq!(items[1]["id"], "skill.a");
    }
}
