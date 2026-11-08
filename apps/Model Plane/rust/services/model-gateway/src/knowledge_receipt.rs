//! The knowledge receipt of a Space-scoped chat turn (WS18-S3).
//!
//! It says what the Space's knowledge did for the answer: searched under the turn's Space decision, refused
//! for want of that decision, or not asked for at all. It is a mode and a count. It carries no content, and
//! nothing persists it: it rides on the turn's `source_scope_applied` frame and goes no further.

use serde::Serialize;

use crate::normalize::RetrievalScope;
use crate::retrieval::Grounding;

/// What the Space's knowledge did for one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeMode {
    /// The turn searched the Space's knowledge under its Space decision.
    Retrieval,
    /// The Space decision was empty, so the turn held no authority over the Space's knowledge. Nothing was
    /// searched, and the answer is not grounded in it.
    NotEntitled,
    /// The turn asked for no knowledge (a web-only filter, or a conversation-only turn), so nothing was searched.
    NotSearched,
}

/// The `knowledge` block of `source_scope_applied`.
///
/// `in_context` is deliberately absent. It is reserved for knowledge the assembled context carried without a
/// search, and no turn reports it yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeReceipt {
    pub mode: KnowledgeMode,
    /// The sources the search returned. Present only when a search ran this turn, so a missing count never
    /// reads as "searched and found nothing".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<u32>,
}

/// The knowledge receipt of a turn, or `None` for a turn that no Space scopes.
///
/// `space_scoped` is true when a Space decision scoped the turn's retrieval, and `token_empty` when that
/// decision is the empty one the BFF sends for a refusal. `grounding` is what the turn's search returned, and
/// `None` when no search returned a result. A turn no Space scopes gets no receipt, so its frames stay as they
/// were.
#[must_use]
pub fn knowledge_receipt(
    space_scoped: bool,
    token_empty: bool,
    scope: RetrievalScope,
    conversation_only: bool,
    grounding: Option<&Grounding>,
) -> Option<KnowledgeReceipt> {
    if !space_scoped {
        return None;
    }
    let mode = if conversation_only || scope == RetrievalScope::WebOnly {
        KnowledgeMode::NotSearched
    } else if token_empty {
        KnowledgeMode::NotEntitled
    } else {
        KnowledgeMode::Retrieval
    };
    let sources = match mode {
        KnowledgeMode::Retrieval => {
            grounding.and_then(|grounding| u32::try_from(grounding.source_count).ok())
        }
        KnowledgeMode::NotEntitled | KnowledgeMode::NotSearched => None,
    };
    Some(KnowledgeReceipt { mode, sources })
}

#[cfg(test)]
mod tests {
    use mp_contracts::dataplane::retrieval_v2::RetrieveResponse;
    use serde_json::json;

    use super::*;
    use crate::retrieval::build_grounding;

    fn grounding_with_sources(source_count: usize) -> Grounding {
        let mut grounding = build_grounding("policy", &RetrieveResponse::default());
        grounding.source_count = source_count;
        grounding
    }

    #[test]
    fn knowledge_receipt_is_absent_for_a_turn_no_space_scopes() {
        let searched = grounding_with_sources(2);
        for scope in [
            RetrievalScope::Auto,
            RetrievalScope::KnowledgeOnly,
            RetrievalScope::WebOnly,
        ] {
            for token_empty in [false, true] {
                for conversation_only in [false, true] {
                    assert_eq!(
                        knowledge_receipt(false, token_empty, scope, conversation_only, None),
                        None
                    );
                    assert_eq!(
                        knowledge_receipt(
                            false,
                            token_empty,
                            scope,
                            conversation_only,
                            Some(&searched)
                        ),
                        None
                    );
                }
            }
        }
    }

    #[test]
    fn knowledge_receipt_reports_an_empty_decision_as_not_entitled() {
        for scope in [RetrievalScope::Auto, RetrievalScope::KnowledgeOnly] {
            assert_eq!(
                knowledge_receipt(true, true, scope, false, None),
                Some(KnowledgeReceipt {
                    mode: KnowledgeMode::NotEntitled,
                    sources: None
                })
            );
        }
    }

    #[test]
    fn knowledge_receipt_reports_a_web_only_or_conversation_turn_as_not_searched() {
        let not_searched = Some(KnowledgeReceipt {
            mode: KnowledgeMode::NotSearched,
            sources: None,
        });
        for token_empty in [false, true] {
            assert_eq!(
                knowledge_receipt(true, token_empty, RetrievalScope::WebOnly, false, None),
                not_searched
            );
            assert_eq!(
                knowledge_receipt(true, token_empty, RetrievalScope::Auto, true, None),
                not_searched
            );
        }
    }

    #[test]
    fn knowledge_receipt_counts_sources_only_when_a_search_returned_a_result() {
        let retrieval = |sources| {
            Some(KnowledgeReceipt {
                mode: KnowledgeMode::Retrieval,
                sources,
            })
        };
        assert_eq!(
            knowledge_receipt(
                true,
                false,
                RetrievalScope::Auto,
                false,
                Some(&grounding_with_sources(3))
            ),
            retrieval(Some(3))
        );
        assert_eq!(
            knowledge_receipt(
                true,
                false,
                RetrievalScope::KnowledgeOnly,
                false,
                Some(&grounding_with_sources(0))
            ),
            retrieval(Some(0))
        );
        assert_eq!(
            knowledge_receipt(true, false, RetrievalScope::Auto, false, None),
            retrieval(None)
        );
    }

    #[test]
    fn knowledge_receipt_serializes_a_mode_and_a_count_and_nothing_else() {
        let searched = knowledge_receipt(
            true,
            false,
            RetrievalScope::Auto,
            false,
            Some(&grounding_with_sources(2)),
        );
        assert_eq!(
            serde_json::to_value(searched.expect("a Space-scoped searched turn has a receipt"))
                .unwrap(),
            json!({ "mode": "retrieval", "sources": 2 })
        );
        let refused = knowledge_receipt(true, true, RetrievalScope::Auto, false, None);
        assert_eq!(
            serde_json::to_value(refused.expect("a Space-scoped refused turn has a receipt"))
                .unwrap(),
            json!({ "mode": "not_entitled" })
        );
        let not_searched = knowledge_receipt(true, false, RetrievalScope::WebOnly, false, None);
        assert_eq!(
            serde_json::to_value(not_searched.expect("a Space-scoped web-only turn has a receipt"))
                .unwrap(),
            json!({ "mode": "not_searched" })
        );
    }
}
