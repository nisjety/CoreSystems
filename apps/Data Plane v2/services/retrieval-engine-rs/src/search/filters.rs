use qdrant_client::qdrant::{Condition, FieldCondition, Match, RepeatedStrings};

pub struct RetrievalFilters {
    pub document_types: Vec<String>,
    pub departments: Vec<String>,
    pub languages: Vec<String>,
    pub document_ids: Vec<String>,
    pub sources: Vec<String>,
    pub region: Option<String>,
    pub workspaces: Vec<String>,
    pub collections: Vec<String>,
    pub acl_tags: Vec<String>,
}

impl RetrievalFilters {
    /// Every condition returned here lands in the dense arm's `must` list
    /// (`search::dense::vector_search`), so it is ANDed with the others.
    ///
    /// That is why a multi-value axis MUST become one any-of condition rather
    /// than one condition per value: two `document_id = …` conditions in a
    /// `must` mean "this chunk's document_id equals both", which no chunk
    /// satisfies. A filter list means "any of these" on every other axis in
    /// this codebase — `AuthContext::intersect_filter` builds exactly that —
    /// and a filter that silently matches nothing is worse than one that
    /// matches too much, because an empty result reads as "nothing found".
    pub fn to_qdrant_conditions(&self) -> Vec<Condition> {
        let mut conditions = Vec::new();

        for (key, values) in [
            ("type", &self.document_types),
            ("department", &self.departments),
            ("language", &self.languages),
            ("document_id", &self.document_ids),
            ("source", &self.sources),
            ("workspace_id", &self.workspaces),
            ("collection_id", &self.collections),
            ("acl_tag", &self.acl_tags),
        ] {
            if let Some(condition) = any_of_condition(key, values) {
                conditions.push(condition);
            }
        }

        if let Some(ref region) = self.region {
            if let Some(condition) = any_of_condition("region", std::slice::from_ref(region)) {
                conditions.push(condition);
            }
        }

        conditions
    }
}

/// One condition matching any of `values`, or `None` when the axis is
/// unconstrained. Blank entries are dropped: an empty keyword would narrow the
/// axis to chunks whose field is the empty string.
fn any_of_condition(key: &str, values: &[String]) -> Option<Condition> {
    let values: Vec<String> = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    let match_value = match values.len() {
        0 => return None,
        1 => qdrant_client::qdrant::r#match::MatchValue::Keyword(
            values.into_iter().next().expect("checked one value"),
        ),
        _ => qdrant_client::qdrant::r#match::MatchValue::Keywords(RepeatedStrings {
            strings: values,
        }),
    };
    Some(Condition::from(FieldCondition {
        key: key.to_owned(),
        r#match: Some(Match {
            match_value: Some(match_value),
        }),
        ..Default::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::RetrievalFilters;
    use qdrant_client::qdrant::{condition::ConditionOneOf, r#match::MatchValue};

    fn empty() -> RetrievalFilters {
        RetrievalFilters {
            document_types: vec![],
            departments: vec![],
            languages: vec![],
            document_ids: vec![],
            sources: vec![],
            region: None,
            workspaces: vec![],
            collections: vec![],
            acl_tags: vec![],
        }
    }

    fn field_matches(filters: &RetrievalFilters, key: &str) -> Option<MatchValue> {
        filters
            .to_qdrant_conditions()
            .into_iter()
            .filter_map(|condition| match condition.condition_one_of {
                Some(ConditionOneOf::Field(field)) if field.key == key => {
                    field.r#match.and_then(|m| m.match_value)
                }
                _ => None,
            })
            .next()
    }

    #[test]
    fn a_single_value_axis_is_an_exact_keyword() {
        let filters = RetrievalFilters {
            workspaces: vec!["workspace_1".into()],
            ..empty()
        };
        assert!(matches!(
            field_matches(&filters, "workspace_id"),
            Some(MatchValue::Keyword(value)) if value == "workspace_1"
        ));
    }

    // The regression this file exists to prevent: two conditions on one key in
    // a `must` list can never both hold, so a two-document filter used to
    // return nothing at all rather than those two documents.
    #[test]
    fn a_multi_value_axis_is_one_any_of_condition_not_two_ands() {
        let filters = RetrievalFilters {
            document_ids: vec!["doc_1".into(), "doc_2".into()],
            ..empty()
        };
        let conditions = filters.to_qdrant_conditions();
        assert_eq!(conditions.len(), 1, "one axis must yield one condition");
        match field_matches(&filters, "document_id") {
            Some(MatchValue::Keywords(values)) => {
                assert_eq!(values.strings, ["doc_1", "doc_2"]);
            }
            other => panic!("expected an any-of keyword match, got {other:?}"),
        }
    }

    #[test]
    fn a_blank_entry_never_narrows_an_axis_to_the_empty_string() {
        let filters = RetrievalFilters {
            sources: vec!["  ".into()],
            acl_tags: vec!["tag_1".into(), "   ".into()],
            ..empty()
        };
        assert!(field_matches(&filters, "source").is_none());
        assert!(matches!(
            field_matches(&filters, "acl_tag"),
            Some(MatchValue::Keyword(value)) if value == "tag_1"
        ));
    }

    #[test]
    fn axes_stay_independent_so_they_and_across_and_or_within() {
        let filters = RetrievalFilters {
            document_ids: vec!["doc_1".into(), "doc_2".into()],
            workspaces: vec!["workspace_1".into()],
            region: Some("swedencentral".into()),
            ..empty()
        };
        assert_eq!(filters.to_qdrant_conditions().len(), 3);
    }
}
