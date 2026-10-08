//! The two closed vocabularies of a usage event: what kind of provider call it covers
//! (`event_type`) and what product feature caused it (`feature`).
//!
//! Both are closed on purpose. `feature` is the dimension cost-core reports spend by and
//! the organization's budget exclusion keys on, so a spelling that drifts between a
//! producer and the verifier is spend that disappears from a report. The verifier in
//! data-orchestrator (`internal/cost/verifier.go`) pins the same pairs per producer, and
//! the cross-language fixture test fails when the two sides disagree.

/// What kind of provider call an event covers. The Data Plane's own classification, older
/// than `feature` (the v1 event carried only this): `embedding` and `rerank` kept their
/// names, and every chat-completion call (contextualization, graph extraction) is `llm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EventType {
    Embedding,
    Llm,
    Rerank,
}

impl EventType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedding => "embedding",
            Self::Llm => "llm",
            Self::Rerank => "rerank",
        }
    }
}

/// The product feature a paid call belongs to.
///
/// Spelled the way cost-core accepts it (a bounded charset: lowercase letters, digits and
/// hyphens), because it is forwarded verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Feature {
    /// Embedding of chunk text (vectors only; the token count is an estimate).
    Embedding,
    /// The sentences that situate a chunk in its document (contextual retrieval).
    ContextualRetrieval,
    /// Entity and relationship extraction for the graph.
    GraphExtraction,
    /// The summary of a graph community.
    GraphCommunitySummary,
    /// Reranking of the Data Plane's own retrieval candidates.
    Rerank,
    /// Reranking of caller-supplied candidates (Quarry's web results).
    WebRerank,
}

impl Feature {
    pub const ALL: [Feature; 6] = [
        Self::Embedding,
        Self::ContextualRetrieval,
        Self::GraphExtraction,
        Self::GraphCommunitySummary,
        Self::Rerank,
        Self::WebRerank,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedding => "embedding",
            Self::ContextualRetrieval => "contextual-retrieval",
            Self::GraphExtraction => "graph-extraction",
            Self::GraphCommunitySummary => "graph-community-summary",
            Self::Rerank => "rerank",
            Self::WebRerank => "web-rerank",
        }
    }

    /// The kind of call this feature makes. One feature, one kind: an event whose pair is
    /// anything else is refused by the verifier.
    pub const fn event_type(self) -> EventType {
        match self {
            Self::Embedding => EventType::Embedding,
            Self::ContextualRetrieval | Self::GraphExtraction | Self::GraphCommunitySummary => {
                EventType::Llm
            }
            Self::Rerank | Self::WebRerank => EventType::Rerank,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|feature| feature.as_str() == value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_feature_round_trips_through_its_name() {
        for feature in Feature::ALL {
            assert_eq!(Feature::parse(feature.as_str()), Some(feature));
        }
        assert_eq!(
            Feature::parse("Embedding"),
            None,
            "the vocabulary is case sensitive"
        );
        assert_eq!(Feature::parse(""), None);
        assert_eq!(Feature::parse("chat"), None);
    }

    #[test]
    fn a_feature_is_spelled_the_way_cost_core_accepts_it() {
        for feature in Feature::ALL {
            let name = feature.as_str();
            assert!(
                !name.is_empty()
                    && name.len() <= 63
                    && name.starts_with(|c: char| c.is_ascii_lowercase())
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{name} is not a bounded lowercase-hyphen label"
            );
        }
    }

    /// The pairs the verifier pins per producer. If one of these changes, the Go side
    /// (`internal/cost/verifier.go`, `producerKinds`) changes in the same commit.
    #[test]
    fn each_feature_makes_exactly_one_kind_of_call() {
        let pairs: Vec<(&str, &str)> = Feature::ALL
            .iter()
            .map(|feature| (feature.as_str(), feature.event_type().as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("embedding", "embedding"),
                ("contextual-retrieval", "llm"),
                ("graph-extraction", "llm"),
                ("graph-community-summary", "llm"),
                ("rerank", "rerank"),
                ("web-rerank", "rerank"),
            ]
        );
    }
}
