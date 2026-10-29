use std::collections::HashMap;

use serde_json::Value;
use sha2::{Digest, Sha256};
use vsv2_common::PrimaryKey;

use crate::registry::EndpointDefinition;

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationIssue {
    pub index: usize,
    pub key: Option<String>,
    pub reasons: Vec<String>,
    pub record: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationOutcome {
    pub valid_records: Vec<Value>,
    pub rejected: Vec<ValidationIssue>,
    pub duplicates_dropped: usize,
}

const COMPOSITE_PK_SEPARATOR: &str = "\u{1f}";

/// What became of one record.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Kept: the record, to be written.
    Valid(Value),
    /// Identical to a record already kept under its key: dropped.
    Duplicate,
    /// Refused, with the reasons.
    Rejected(ValidationIssue),
}

/// The first 16 bytes of the SHA-256 of a payload. A key keeps this instead of its payload.
type PayloadDigest = [u8; 16];

/// Checks records one at a time, the way a run receives them, page by page.
///
/// For each key it keeps a digest of the one payload it accepted first. A later record with that key
/// is then a duplicate (the same payload) or a conflict (any other payload). The payloads themselves
/// are not kept, so checking an endpoint costs a digest per key, not the endpoint's text.
pub struct RecordChecker {
    key_paths: Vec<String>,
    validation_fields: Vec<String>,
    accepted_digests: HashMap<String, PayloadDigest>,
    duplicates_dropped: usize,
}

impl RecordChecker {
    pub fn new(def: &EndpointDefinition) -> Self {
        Self {
            key_paths: scoped_key_paths(def),
            validation_fields: def.validation_fields.clone().unwrap_or_default(),
            accepted_digests: HashMap::new(),
            duplicates_dropped: 0,
        }
    }

    /// Check `record`, the `index`th record of its run. The index is reported with a rejection.
    pub fn check(&mut self, index: usize, record: Value) -> Verdict {
        let mut reasons = Vec::new();
        let key = extract_key(&record, &self.key_paths, "primary key", &mut reasons);

        for field in &self.validation_fields {
            if !path_has_present_value(&record, field) {
                reasons.push(format!("missing validation field {field}"));
            }
        }
        if !reasons.is_empty() {
            return Verdict::Rejected(ValidationIssue {
                index,
                key,
                reasons,
                record,
            });
        }

        let Some(key) = key else {
            return Verdict::Rejected(ValidationIssue {
                index,
                key: None,
                reasons: vec!["missing primary key".into()],
                record,
            });
        };

        let digest = payload_digest(&record);
        match self.accepted_digests.get(&key) {
            Some(accepted) if *accepted == digest => {
                self.duplicates_dropped += 1;
                Verdict::Duplicate
            }
            Some(_) => Verdict::Rejected(ValidationIssue {
                index,
                key: Some(key),
                reasons: vec!["duplicate primary key with different payload".into()],
                record,
            }),
            None => {
                self.accepted_digests.insert(key, digest);
                Verdict::Valid(record)
            }
        }
    }

    /// Exact duplicates dropped so far.
    pub fn duplicates_dropped(&self) -> usize {
        self.duplicates_dropped
    }
}

/// Check a whole list of records at once.
pub fn validate_records(def: &EndpointDefinition, records: Vec<Value>) -> ValidationOutcome {
    let mut checker = RecordChecker::new(def);
    let mut valid_records = Vec::new();
    let mut rejected = Vec::new();

    for (index, record) in records.into_iter().enumerate() {
        match checker.check(index, record) {
            Verdict::Valid(record) => valid_records.push(record),
            Verdict::Duplicate => {}
            Verdict::Rejected(issue) => rejected.push(issue),
        }
    }

    ValidationOutcome {
        valid_records,
        rejected,
        duplicates_dropped: checker.duplicates_dropped(),
    }
}

fn payload_digest(record: &Value) -> PayloadDigest {
    let payload = serde_json::to_string(record).unwrap_or_else(|_| record.to_string());
    let hash = Sha256::digest(payload.as_bytes());
    let mut digest = [0_u8; 16];
    digest.copy_from_slice(&hash[..16]);
    digest
}

fn scoped_key_paths(def: &EndpointDefinition) -> Vec<String> {
    let mut parts = match &def.primary_key {
        PrimaryKey::Single(path) => vec![path.clone()],
        PrimaryKey::Composite(paths) => paths.clone(),
    };

    if def.partitioning.is_some()
        && !parts
            .iter()
            .any(|part| part.eq_ignore_ascii_case("_partition_key"))
    {
        parts.insert(0, "_partition_key".to_string());
    }

    let has_tenant = parts
        .iter()
        .any(|part| part.eq_ignore_ascii_case("tenant_id"));
    if has_tenant {
        parts
    } else {
        std::iter::once("tenant_id".to_string())
            .chain(parts)
            .collect()
    }
}

fn extract_key(
    record: &Value,
    paths: &[String],
    label: &str,
    reasons: &mut Vec<String>,
) -> Option<String> {
    let mut values = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(value) = get_path(record, path) else {
            reasons.push(format!("missing {label} field {path}"));
            return None;
        };
        if !is_present(value) {
            reasons.push(format!("missing {label} field {path}"));
            return None;
        }
        values.push(stringify_key_value(value));
    }
    Some(values.join(COMPOSITE_PK_SEPARATOR))
}

fn path_has_present_value(record: &Value, path: &str) -> bool {
    get_path(record, path).is_some_and(is_present)
}

fn get_path<'a>(record: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(record, |current, part| current.get(part))
}

fn is_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        Value::Bool(_) | Value::Number(_) => true,
    }
}

fn stringify_key_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;
    use vsv2_common::{BackgroundMode, PaginationProfile, PrimaryKey, Source};

    use super::*;

    fn def(primary_key: PrimaryKey, validation_fields: Vec<&str>) -> EndpointDefinition {
        EndpointDefinition {
            endpoint_name: "accounts".into(),
            bigquery_table: "visma_accounts".into(),
            primary_key,
            source: Source::ErpRest,
            enabled: true,
            api_method: Some("get_accounts".into()),
            api_endpoint: Some("/v1/account".into()),
            fallback_api_method: None,
            pagination_profile: PaginationProfile::FinancePageNumber,
            page_size: None,
            background_mode: BackgroundMode::Off,
            background_poll_secs: None,
            background_timeout_secs: None,
            default_query: HashMap::new(),
            required_query: vec![],
            modified_field: None,
            merge_type: Some("upsert".into()),
            sync_pattern: Some("incremental_upsert".into()),
            validation_fields: Some(validation_fields.into_iter().map(str::to_string).collect()),
            partitioning: None,
            period_scan: None,
            datamart: None,
            tier: None,
            open_set: false,
        }
    }

    #[test]
    fn rejects_records_missing_key_or_validation_fields() {
        let def = def(PrimaryKey::Single("accountCD".into()), vec!["description"]);
        let outcome = validate_records(
            &def,
            vec![
                json!({"tenant_id": "t1", "accountCD": "1500", "description": "Trade"}),
                json!({"tenant_id": "t1", "description": "Missing key"}),
                json!({"tenant_id": "t1", "accountCD": "1600", "description": ""}),
            ],
        );

        assert_eq!(outcome.valid_records.len(), 1);
        assert_eq!(outcome.rejected.len(), 2);
        assert!(outcome.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("missing primary key field accountCD")));
        assert!(outcome.rejected[1]
            .reasons
            .iter()
            .any(|reason| reason.contains("missing validation field description")));
    }

    #[test]
    fn drops_exact_duplicate_records_without_rejecting_run() {
        let def = def(PrimaryKey::Single("accountCD".into()), vec!["description"]);
        let record = json!({"tenant_id": "t1", "accountCD": "1500", "description": "Trade"});
        let outcome = validate_records(&def, vec![record.clone(), record]);

        assert_eq!(outcome.valid_records.len(), 1);
        assert!(outcome.rejected.is_empty());
        assert_eq!(outcome.duplicates_dropped, 1);
    }

    #[test]
    fn rejects_same_key_with_different_payloads() {
        let def = def(PrimaryKey::Single("accountCD".into()), vec!["description"]);
        let outcome = validate_records(
            &def,
            vec![
                json!({"tenant_id": "t1", "accountCD": "1500", "description": "Trade"}),
                json!({"tenant_id": "t1", "accountCD": "1500", "description": "Changed"}),
            ],
        );

        assert_eq!(outcome.valid_records.len(), 1);
        assert_eq!(outcome.rejected.len(), 1);
        assert!(outcome.rejected[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("duplicate primary key with different payload")));
    }

    #[test]
    fn partitioned_records_include_partition_key_in_identity() {
        let mut def = def(PrimaryKey::Single("accountCD".into()), vec!["description"]);
        def.partitioning = Some(crate::partition::PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![crate::partition::PartitionDimensionConfig {
                query_param: "ledger".into(),
                source_endpoint: "/v1/ledger".into(),
                source_path: "number".into(),
                fallback_values: vec![],
                ..Default::default()
            }],
            max_partitions: 10,
            ..Default::default()
        });
        let outcome = validate_records(
            &def,
            vec![
                json!({
                    "tenant_id": "t1",
                    "_partition_key": "ledger=ACTUAL",
                    "accountCD": "1500",
                    "description": "Trade",
                }),
                json!({
                    "tenant_id": "t1",
                    "_partition_key": "ledger=BUDGET",
                    "accountCD": "1500",
                    "description": "Trade",
                }),
            ],
        );

        assert_eq!(outcome.valid_records.len(), 2);
        assert!(outcome.rejected.is_empty());
    }

    fn trade(description: &str) -> Value {
        json!({"tenant_id": "t1", "accountCD": "1500", "description": description})
    }

    #[test]
    fn a_conflict_is_caught_when_the_first_record_came_in_an_earlier_page() {
        let def = def(PrimaryKey::Single("accountCD".into()), vec!["description"]);
        let mut checker = RecordChecker::new(&def);

        let first_page = checker.check(0, trade("Trade"));
        let later_page = checker.check(1, trade("Changed"));

        assert!(matches!(first_page, Verdict::Valid(_)));
        let Verdict::Rejected(issue) = later_page else {
            panic!("a different payload under a kept key is rejected");
        };
        assert_eq!(issue.index, 1);
        assert!(issue
            .reasons
            .iter()
            .any(|reason| reason.contains("duplicate primary key with different payload")));
    }

    #[test]
    fn an_exact_copy_from_a_later_page_is_dropped_and_counted() {
        let def = def(PrimaryKey::Single("accountCD".into()), vec!["description"]);
        let mut checker = RecordChecker::new(&def);

        assert!(matches!(
            checker.check(0, trade("Trade")),
            Verdict::Valid(_)
        ));
        assert_eq!(checker.check(1, trade("Trade")), Verdict::Duplicate);
        assert_eq!(checker.duplicates_dropped(), 1);
    }

    #[test]
    fn an_accepted_record_is_handed_back_unchanged() {
        let def = def(PrimaryKey::Single("accountCD".into()), vec!["description"]);
        let mut checker = RecordChecker::new(&def);

        assert_eq!(
            checker.check(0, trade("Trade")),
            Verdict::Valid(trade("Trade"))
        );
    }
}
