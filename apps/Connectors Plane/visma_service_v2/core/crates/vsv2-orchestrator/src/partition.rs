use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vsv2_common::{Error, PaginationProfile, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartitioningConfig {
    #[serde(default = "default_partition_mode")]
    pub mode: String,
    pub dimensions: Vec<PartitionDimensionConfig>,
    #[serde(default = "default_max_partitions")]
    pub max_partitions: usize,
    /// Fields copied within each fetched row after the partition fields are added:
    /// new field -> path of an existing field of the same row (`locationId` = `id`).
    #[serde(default)]
    pub row_copies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartitionDimensionConfig {
    /// Name of the request parameter this dimension fills.
    pub query_param: String,
    pub source_endpoint: String,
    pub source_path: String,
    #[serde(default)]
    pub fallback_values: Vec<String>,
    /// The value replaces `{query_param}` in the endpoint's URL instead of being sent as a query
    /// parameter (`/Customers/{customerId}/locations`). Rows do not repeat it, so it is added to
    /// every row under the same name.
    #[serde(default)]
    pub in_path: bool,
    /// Pagination profile for listing `source_endpoint`, when it is not the endpoint's own.
    #[serde(default)]
    pub source_pagination_profile: Option<PaginationProfile>,
    /// Fields of the dimension's source record added to every row fetched for that value:
    /// row field -> path in the source record (`defaultLocationId` = `defaultLocation.id`).
    #[serde(default)]
    pub carry: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartitionPlan {
    pub key: String,
    /// Query parameters of the request.
    pub query: HashMap<String, Value>,
    /// Values that replace `{name}` in the URL.
    pub path: HashMap<String, String>,
    /// Fields added to every fetched row.
    pub inject: HashMap<String, Value>,
}

impl PartitionPlan {
    pub fn default_partition() -> Self {
        Self {
            key: vsv2_state::DEFAULT_PARTITION_KEY.to_string(),
            ..Default::default()
        }
    }

    pub fn is_default(&self) -> bool {
        self.key == vsv2_state::DEFAULT_PARTITION_KEY
    }
}

fn default_partition_mode() -> String {
    "cartesian".to_string()
}

fn default_max_partitions() -> usize {
    10_000
}

pub fn build_partitions_from_dimensions(
    config: &PartitioningConfig,
    dimension_records: &HashMap<String, Vec<Value>>,
) -> Result<Vec<PartitionPlan>> {
    if config.mode != "cartesian" {
        return Err(Error::Config(format!(
            "unsupported partitioning.mode '{}'",
            config.mode
        )));
    }

    let mut values_by_param: Vec<(&PartitionDimensionConfig, Vec<DimensionValue>)> = Vec::new();
    for dim in &config.dimensions {
        let values = extract_dimension_values(dim, dimension_records)?;
        if values.is_empty() {
            // The dimension's listing is walked completely (and checked against the API's own
            // total), so an empty one is an answer: no employees, no timecards to ask for.
            return Ok(Vec::new());
        }
        values_by_param.push((dim, values));
    }

    // One partial plan per combination so far: the chosen value per parameter, and the fields
    // carried from the chosen dimension records.
    type Partial = (BTreeMap<String, String>, HashMap<String, Value>);
    let mut plans: Vec<Partial> = vec![(BTreeMap::new(), HashMap::new())];
    for (dim, values) in &values_by_param {
        let mut next: Vec<Partial> = Vec::new();
        for (existing, carried) in &plans {
            for value in values {
                let mut merged = existing.clone();
                merged.insert(dim.query_param.clone(), value.value.clone());
                let mut fields = carried.clone();
                fields.extend(value.carried.clone());
                if dim.in_path {
                    fields
                        .entry(dim.query_param.clone())
                        .or_insert_with(|| Value::String(value.value.clone()));
                }
                next.push((merged, fields));
                if next.len() > config.max_partitions {
                    return Err(Error::Config(format!(
                        "partition plan exceeds max_partitions={}",
                        config.max_partitions
                    )));
                }
            }
        }
        plans = next;
    }

    let in_path: BTreeSet<&str> = config
        .dimensions
        .iter()
        .filter(|d| d.in_path)
        .map(|d| d.query_param.as_str())
        .collect();
    Ok(plans
        .into_iter()
        .map(|(parts, inject)| {
            let key = parts
                .iter()
                .map(|(param, value)| format!("{param}={value}"))
                .collect::<Vec<_>>()
                .join("|");
            let mut query = HashMap::new();
            let mut path = HashMap::new();
            for (param, value) in parts {
                if in_path.contains(param.as_str()) {
                    path.insert(param, value);
                } else {
                    query.insert(param, Value::String(value));
                }
            }
            PartitionPlan {
                key,
                query,
                path,
                inject,
            }
        })
        .collect())
}

/// One value of a dimension and the fields to carry onto rows fetched for it.
struct DimensionValue {
    value: String,
    carried: HashMap<String, Value>,
}

fn extract_dimension_values(
    dim: &PartitionDimensionConfig,
    dimension_records: &HashMap<String, Vec<Value>>,
) -> Result<Vec<DimensionValue>> {
    // BTreeMap: sorted and de-duplicated by value; the first record seen for a value supplies
    // the carried fields.
    let mut out: BTreeMap<String, HashMap<String, Value>> = BTreeMap::new();
    if let Some(records) = dimension_records.get(&dim.source_endpoint) {
        for record in records {
            if let Some(value) = get_path(record, &dim.source_path).and_then(value_to_string) {
                out.entry(value)
                    .or_insert_with(|| carried_fields(dim, record));
            }
        }
    }
    for fallback in &dim.fallback_values {
        if !fallback.trim().is_empty() {
            out.entry(fallback.clone()).or_default();
        }
    }
    Ok(out
        .into_iter()
        .map(|(value, carried)| DimensionValue { value, carried })
        .collect())
}

fn carried_fields(dim: &PartitionDimensionConfig, record: &Value) -> HashMap<String, Value> {
    dim.carry
        .iter()
        .filter_map(|(field, path)| {
            get_path(record, path)
                .filter(|v| !v.is_null())
                .map(|v| (field.clone(), v.clone()))
        })
        .collect()
}

fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |current, part| current.get(part))
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) if s.trim().is_empty() => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn builds_cartesian_partition_plan_from_tenant_records() {
        let cfg = PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![
                PartitionDimensionConfig {
                    query_param: "ledger".into(),
                    source_endpoint: "/v1/ledger".into(),
                    source_path: "number".into(),
                    fallback_values: vec![],
                    ..Default::default()
                },
                PartitionDimensionConfig {
                    query_param: "branch".into(),
                    source_endpoint: "/v1/branch".into(),
                    source_path: "number".into(),
                    fallback_values: vec![],
                    ..Default::default()
                },
                PartitionDimensionConfig {
                    query_param: "financialYear".into(),
                    source_endpoint: "/v1/financialPeriod".into(),
                    source_path: "year".into(),
                    fallback_values: vec![],
                    ..Default::default()
                },
            ],
            max_partitions: 100,
            ..Default::default()
        };
        let records = HashMap::from([
            (
                "/v1/ledger".into(),
                vec![json!({"number": "ACTUAL"}), json!({"number": "BUDGET"})],
            ),
            (
                "/v1/branch".into(),
                vec![json!({"number": "10"}), json!({"number": "20"})],
            ),
            (
                "/v1/financialPeriod".into(),
                vec![
                    json!({"year": "2025"}),
                    json!({"year": "2025"}),
                    json!({"year": "2026"}),
                ],
            ),
        ]);

        let plans = build_partitions_from_dimensions(&cfg, &records).unwrap();

        assert_eq!(plans.len(), 8);
        assert_eq!(plans[0].key, "branch=10|financialYear=2025|ledger=ACTUAL");
        assert_eq!(plans[0].query["branch"], "10");
        assert_eq!(plans[0].query["financialYear"], "2025");
        assert_eq!(plans[0].query["ledger"], "ACTUAL");
    }

    #[test]
    fn fallback_values_make_dimension_non_empty() {
        let cfg = PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![PartitionDimensionConfig {
                query_param: "ledger".into(),
                source_endpoint: "/v1/ledger".into(),
                source_path: "number".into(),
                fallback_values: vec!["ACTUAL".into()],
                ..Default::default()
            }],
            max_partitions: 10,
            ..Default::default()
        };

        let plans = build_partitions_from_dimensions(&cfg, &HashMap::new()).unwrap();

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].query["ledger"], "ACTUAL");
    }

    #[test]
    fn a_dimension_with_no_values_means_nothing_to_fetch() {
        let cfg = PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![PartitionDimensionConfig {
                query_param: "employeeCd".into(),
                source_endpoint: "/v1/employee".into(),
                source_path: "employeeId".into(),
                ..Default::default()
            }],
            max_partitions: 10,
            ..Default::default()
        };
        let no_employees = HashMap::from([("/v1/employee".to_string(), vec![])]);

        let plans = build_partitions_from_dimensions(&cfg, &no_employees).unwrap();

        assert!(plans.is_empty());
    }

    #[test]
    fn rejects_partition_explosion() {
        let cfg = PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![
                PartitionDimensionConfig {
                    query_param: "branch".into(),
                    source_endpoint: "/v1/branch".into(),
                    source_path: "number".into(),
                    fallback_values: vec![],
                    ..Default::default()
                },
                PartitionDimensionConfig {
                    query_param: "ledger".into(),
                    source_endpoint: "/v1/ledger".into(),
                    source_path: "number".into(),
                    fallback_values: vec![],
                    ..Default::default()
                },
            ],
            max_partitions: 2,
            ..Default::default()
        };
        let records = HashMap::from([
            (
                "/v1/branch".into(),
                vec![json!({"number": "10"}), json!({"number": "20"})],
            ),
            (
                "/v1/ledger".into(),
                vec![json!({"number": "ACTUAL"}), json!({"number": "BUDGET"})],
            ),
        ]);

        let err = build_partitions_from_dimensions(&cfg, &records).unwrap_err();

        assert!(err.to_string().contains("max_partitions"));
    }

    fn customer_locations_config() -> PartitioningConfig {
        serde_json::from_value(json!({
            "dimensions": [{
                "query_param": "customerId",
                "in_path": true,
                "source_endpoint": "https://salesorder.visma.net/api/v3/Customers",
                "source_pagination_profile": "salesorder_page_index",
                "source_path": "id",
                "carry": {"defaultLocationId": "defaultLocation.id"}
            }],
            "row_copies": {"locationId": "id"},
            "max_partitions": 20000
        }))
        .expect("the customer_locations partitioning block must deserialize")
    }

    #[test]
    fn a_path_dimension_fills_the_url_not_the_query_and_marks_every_row() {
        let records = HashMap::from([(
            "https://salesorder.visma.net/api/v3/Customers".to_string(),
            vec![
                json!({"id": "300001", "defaultLocation": {"id": "HOVED"}}),
                json!({"id": "300002", "defaultLocation": {"id": "HS"}}),
            ],
        )]);

        let plans = build_partitions_from_dimensions(&customer_locations_config(), &records)
            .expect("plans");

        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].key, "customerId=300001");
        assert_eq!(plans[0].path["customerId"], "300001");
        assert!(
            plans[0].query.is_empty(),
            "a path value must not be sent as a query parameter"
        );
        assert_eq!(
            plans[0].inject["customerId"], "300001",
            "rows do not repeat the customer"
        );
        assert_eq!(plans[0].inject["defaultLocationId"], "HOVED");
        assert_eq!(plans[1].inject["defaultLocationId"], "HS");
    }

    #[test]
    fn query_dimensions_keep_their_old_behaviour() {
        let cfg = PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![PartitionDimensionConfig {
                query_param: "ledger".into(),
                source_endpoint: "/v1/ledger".into(),
                source_path: "number".into(),
                ..Default::default()
            }],
            max_partitions: 10,
            ..Default::default()
        };
        let records = HashMap::from([("/v1/ledger".to_string(), vec![json!({"number": "1"})])]);

        let plan = build_partitions_from_dimensions(&cfg, &records)
            .expect("plans")
            .remove(0);

        assert_eq!(plan.query["ledger"], "1");
        assert!(plan.path.is_empty());
        assert!(
            plan.inject.is_empty(),
            "query partitions add nothing to the rows"
        );
    }

    #[test]
    fn carried_fields_come_from_the_first_record_of_a_repeated_value() {
        let mut cfg = customer_locations_config();
        cfg.dimensions[0].carry = BTreeMap::from([("group".to_string(), "g".to_string())]);
        let records = HashMap::from([(
            "https://salesorder.visma.net/api/v3/Customers".to_string(),
            vec![
                json!({"id": "1", "g": "first"}),
                json!({"id": "1", "g": "second"}),
            ],
        )]);

        let plans = build_partitions_from_dimensions(&cfg, &records).expect("plans");

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].inject["group"], "first");
    }
}
