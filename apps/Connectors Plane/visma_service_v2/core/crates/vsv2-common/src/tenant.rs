use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{Error, Result};

/// Visma.net tenant identifier. Opaque UUID-shaped string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TenantId(pub String);

impl TenantId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Aquatiq AS — pilot tenant, hard-coded for Phase 0–4.
pub const AQUATIQ_AS_TENANT: &str = "33f319b9-4d57-11ee-960c-025417856183";

/// Whether a tenant still produces data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantStatus {
    #[default]
    Active,
    /// Merged into another entity; no new data, credentials still work for history.
    Historical,
}

/// One `config/tenants/<name>.json`: who a tenant is and which DataMart company it maps to.
///
/// `datamart_company_id` is `null` until discovery has run (`vsv2 dm-companies`); such a tenant
/// is listed but not registered. The id may be written as a string or a number.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantConfig {
    pub tenant_id: String,
    pub tenant_name: String,
    #[serde(default, deserialize_with = "text_or_number")]
    pub datamart_company_id: Option<String>,
    #[serde(default)]
    pub status: TenantStatus,
    /// Last day the tenant produced data (`YYYY-MM-DD`); required for a historical tenant.
    #[serde(default)]
    pub valid_to: Option<String>,
    /// Business unit the tenant became on its merge, for enrichment.
    #[serde(default)]
    pub bu_label: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

fn text_or_number<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Text(String),
        Number(u64),
    }
    Ok(match Option::<Value>::deserialize(d)? {
        None => None,
        Some(Value::Text(text)) if text.trim().is_empty() => None,
        Some(Value::Text(text)) => Some(text.trim().to_string()),
        Some(Value::Number(number)) => Some(number.to_string()),
    })
}

/// `8-4-4-4-12` hexadecimal groups.
fn is_uuid_shaped(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn is_date(text: &str) -> bool {
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

/// Reject a tenant list that would register the wrong thing.
///
/// Every tenant has its own DataMart company, so a company id shared by two tenants means one of
/// them was copied from the other and would fetch the other's data.
pub fn validate_tenants(tenants: &[TenantConfig]) -> Result<()> {
    let mut ids = HashSet::new();
    let mut companies = HashSet::new();
    for tenant in tenants {
        let name = &tenant.tenant_name;
        if !is_uuid_shaped(&tenant.tenant_id) {
            return Err(Error::Config(format!(
                "tenant {name}: tenant_id '{}' is not a UUID",
                tenant.tenant_id
            )));
        }
        if name.trim().is_empty() {
            return Err(Error::Config(format!(
                "tenant {}: tenant_name is empty",
                tenant.tenant_id
            )));
        }
        if !ids.insert(tenant.tenant_id.as_str()) {
            return Err(Error::Config(format!(
                "tenant_id {} is listed twice",
                tenant.tenant_id
            )));
        }
        if let Some(company) = &tenant.datamart_company_id {
            if !company.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::Config(format!(
                    "tenant {name}: datamart_company_id '{company}' is not a number"
                )));
            }
            if !companies.insert(company.as_str()) {
                return Err(Error::Config(format!(
                    "datamart_company_id {company} belongs to more than one tenant (second: {name})"
                )));
            }
        }
        if let Some(valid_to) = &tenant.valid_to {
            if !is_date(valid_to) {
                return Err(Error::Config(format!(
                    "tenant {name}: valid_to '{valid_to}' is not a YYYY-MM-DD date"
                )));
            }
        }
        if tenant.status == TenantStatus::Historical && tenant.valid_to.is_none() {
            return Err(Error::Config(format!(
                "tenant {name}: a historical tenant needs valid_to"
            )));
        }
    }
    Ok(())
}

/// Load and validate every `*.json` in `dir` (files starting with `_` are skipped), sorted by name.
pub fn load_tenants(dir: &Path) -> Result<Vec<TenantConfig>> {
    let mut tenants = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.extension().and_then(|e| e.to_str()) != Some("json") || file.starts_with('_') {
            continue;
        }
        let tenant: TenantConfig = serde_json::from_slice(&std::fs::read(&path)?)
            .map_err(|e| Error::Config(format!("invalid tenant config {}: {e}", path.display())))?;
        tenants.push(tenant);
    }
    tenants.sort_by(|a, b| a.tenant_name.cmp(&b.tenant_name));
    validate_tenants(&tenants)?;
    Ok(tenants)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(id: &str, name: &str, company: Option<&str>) -> TenantConfig {
        TenantConfig {
            tenant_id: id.into(),
            tenant_name: name.into(),
            datamart_company_id: company.map(String::from),
            status: TenantStatus::Active,
            valid_to: None,
            bu_label: None,
            notes: None,
        }
    }

    const A: &str = "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd";
    const B: &str = "ffa4a061-8084-11e9-b307-0aa512338dd6";

    #[test]
    fn a_company_id_may_be_text_a_number_or_missing() {
        let parse = |json: &str| -> TenantConfig { serde_json::from_str(json).unwrap() };
        let base = r#""tenant_id": "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd", "tenant_name": "A""#;

        assert_eq!(
            parse(&format!("{{{base}, \"datamart_company_id\": \"4734948\"}}")).datamart_company_id,
            Some("4734948".into())
        );
        assert_eq!(
            parse(&format!("{{{base}, \"datamart_company_id\": 4734948}}")).datamart_company_id,
            Some("4734948".into())
        );
        assert_eq!(
            parse(&format!("{{{base}, \"datamart_company_id\": null}}")).datamart_company_id,
            None
        );
        assert_eq!(parse(&format!("{{{base}}}")).datamart_company_id, None);
    }

    #[test]
    fn an_unknown_field_is_a_typo_not_something_to_ignore() {
        let json = r#"{"tenant_id": "x", "tenant_name": "A", "datamart_company": "1"}"#;
        assert!(serde_json::from_str::<TenantConfig>(json).is_err());
    }

    #[test]
    fn two_tenants_with_one_company_are_rejected() {
        let error = validate_tenants(&[tenant(A, "A", Some("1")), tenant(B, "B", Some("1"))])
            .unwrap_err()
            .to_string();
        assert!(error.contains("more than one tenant"), "{error}");
    }

    #[test]
    fn two_tenants_without_a_company_id_are_fine() {
        assert!(validate_tenants(&[tenant(A, "A", None), tenant(B, "B", None)]).is_ok());
    }

    #[test]
    fn malformed_tenants_are_rejected() {
        assert!(validate_tenants(&[tenant("not-a-uuid", "A", None)]).is_err());
        assert!(validate_tenants(&[tenant(A, "A", Some("47x"))]).is_err());
        assert!(validate_tenants(&[tenant(A, " ", None)]).is_err());
        assert!(validate_tenants(&[tenant(A, "A", None), tenant(A, "B", None)]).is_err());

        let mut dated = tenant(A, "A", None);
        dated.valid_to = Some("2025-13-40".into());
        assert!(validate_tenants(&[dated]).is_err());
    }

    #[test]
    fn a_historical_tenant_needs_the_date_it_stopped() {
        let mut gone = tenant(A, "A", Some("1"));
        gone.status = TenantStatus::Historical;
        assert!(validate_tenants(&[gone.clone()]).is_err());

        gone.valid_to = Some("2025-12-31".into());
        assert!(validate_tenants(&[gone]).is_ok());
    }

    /// The tenant files in the repository are what `vsv2 tenants-register` writes to the state
    /// store: they have to stay valid, and the twelve tenants of v1 have to stay in them.
    #[test]
    fn the_repository_tenant_files_are_valid() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../config/tenants");

        let tenants = load_tenants(&dir).unwrap();

        assert_eq!(tenants.len(), 12);
        assert!(tenants.iter().any(|t| t.tenant_id == AQUATIQ_AS_TENANT));
        let historical = tenants
            .iter()
            .filter(|t| t.status == TenantStatus::Historical)
            .count();
        assert_eq!(historical, 4);
        let with_company = tenants
            .iter()
            .filter(|t| t.datamart_company_id.is_some())
            .count();
        assert_eq!(
            with_company, 11,
            "every tenant but the pilot has its DataMart company"
        );
    }
}
