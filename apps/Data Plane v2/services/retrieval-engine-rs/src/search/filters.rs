use qdrant_client::qdrant::{Condition, FieldCondition, Match};

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
            if !values.is_empty() {
                for val in values {
                    conditions.push(Condition::from(FieldCondition {
                        key: key.to_string(),
                        r#match: Some(Match {
                            match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Keyword(
                                val.clone(),
                            )),
                        }),
                        ..Default::default()
                    }));
                }
            }
        }

        if let Some(ref region) = self.region {
            conditions.push(Condition::from(FieldCondition {
                key: "region".to_string(),
                r#match: Some(Match {
                    match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Keyword(
                        region.clone(),
                    )),
                }),
                ..Default::default()
            }));
        }

        conditions
    }
}
