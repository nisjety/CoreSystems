use serde_json::{json, Value};
use vsv2_common::{TenantConfig, TenantId};

use crate::tool::ToolError;

/// The tenants this server may touch. A tenant id that is not configured is refused, so the OAuth
/// client can never be pointed at an arbitrary tenant.
#[derive(Debug, Clone)]
pub struct Tenants {
    all: Vec<TenantConfig>,
    default_id: String,
    /// True for a signed-in person: what is listed is all this server may show, so a question that names no
    /// company is answered for these companies and no others.
    restricted: bool,
}

impl Tenants {
    pub fn new(mut all: Vec<TenantConfig>, default_id: impl Into<String>) -> Self {
        all.sort_by(|a, b| a.tenant_name.cmp(&b.tenant_name));
        Self {
            all,
            default_id: default_id.into(),
            restricted: false,
        }
    }

    /// The companies a signed-in person has connected. A configured company the person has not connected is
    /// left out, since a call for it would be refused; a connected company that is not configured is added,
    /// under the name Visma gave it (or its id). There is no default company unless the configured default
    /// is among them, so a call without `tenant` is refused rather than aimed at an arbitrary company.
    pub fn for_sign_in(
        configured: Vec<TenantConfig>,
        connected: &[(String, Option<String>)],
        default_id: &str,
    ) -> Self {
        let mut all: Vec<TenantConfig> = configured
            .into_iter()
            .filter(|t| connected.iter().any(|(id, _)| *id == t.tenant_id))
            .collect();
        for (id, name) in connected {
            if all.iter().any(|t| &t.tenant_id == id) {
                continue;
            }
            let shown = name.clone().unwrap_or_else(|| id.clone());
            if let Ok(tenant) =
                serde_json::from_value(json!({"tenant_id": id, "tenant_name": shown}))
            {
                all.push(tenant);
            }
        }
        let default = if all.iter().any(|t| t.tenant_id == default_id) {
            default_id
        } else {
            ""
        };
        let mut tenants = Self::new(all, default);
        tenants.restricted = true;
        tenants
    }

    /// Whether a question that names no company must be limited to the companies listed here.
    pub fn is_restricted(&self) -> bool {
        self.restricted
    }

    /// Every company's id.
    pub fn ids(&self) -> Vec<String> {
        self.all.iter().map(|t| t.tenant_id.clone()).collect()
    }

    /// The name a company is shown under, by id.
    pub fn name_of(&self, tenant_id: &str) -> Option<String> {
        self.all
            .iter()
            .find(|t| t.tenant_id == tenant_id)
            .map(|t| t.tenant_name.clone())
    }

    pub fn default_id(&self) -> &str {
        &self.default_id
    }

    pub fn describe(&self) -> Vec<Value> {
        self.all
            .iter()
            .map(|t| {
                json!({
                    "tenant_id": t.tenant_id,
                    "name": t.tenant_name,
                    "status": format!("{:?}", t.status).to_lowercase(),
                    "datamart_company_id": t.datamart_company_id,
                    "valid_to": t.valid_to,
                    "is_default": t.tenant_id == self.default_id,
                })
            })
            .collect()
    }

    /// Accepts a tenant id or a (case-insensitive) tenant name; `None` means the default tenant.
    pub fn resolve(&self, wanted: Option<&str>) -> Result<TenantId, ToolError> {
        let w = wanted
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.default_id);
        if w.is_empty() {
            return Err(ToolError::InvalidArgs(format!(
                "there is no default company: pass `tenant` (one of: {})",
                self.all
                    .iter()
                    .map(|t| t.tenant_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let hit = self
            .all
            .iter()
            .find(|t| t.tenant_id == w || t.tenant_name.eq_ignore_ascii_case(w));
        match hit {
            Some(t) => Ok(TenantId::new(t.tenant_id.clone())),
            None => Err(ToolError::InvalidArgs(format!(
                "unknown tenant {w:?}; use visma_list_tenants (known: {})",
                self.all
                    .iter()
                    .map(|t| t.tenant_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    pub fn company_id(&self, tenant: &TenantId) -> Option<&str> {
        self.all
            .iter()
            .find(|t| t.tenant_id == tenant.as_str())
            .and_then(|t| t.datamart_company_id.as_deref())
    }
}

#[cfg(test)]
pub(crate) fn test_tenants() -> Tenants {
    let mk = |id: &str, name: &str, company: Option<&str>| -> TenantConfig {
        serde_json::from_value(json!({
            "tenant_id": id, "tenant_name": name, "datamart_company_id": company
        }))
        .unwrap()
    };
    Tenants::new(
        vec![
            mk("t-aq", "Aquatiq AS", Some("4839725")),
            mk("t-ab", "Aquatiq AB", None),
        ],
        "t-aq",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_default_id_name_and_rejects_unknown() {
        let t = test_tenants();
        assert_eq!(t.resolve(None).unwrap().as_str(), "t-aq");
        assert_eq!(t.resolve(Some("  ")).unwrap().as_str(), "t-aq");
        assert_eq!(t.resolve(Some("t-ab")).unwrap().as_str(), "t-ab");
        assert_eq!(t.resolve(Some("aquatiq ab")).unwrap().as_str(), "t-ab");
        let e = t.resolve(Some("evil-tenant")).unwrap_err().to_string();
        assert!(
            e.contains("unknown tenant") && e.contains("Aquatiq AS"),
            "{e}"
        );
    }

    fn configured(id: &str, name: &str) -> TenantConfig {
        serde_json::from_value(
            json!({"tenant_id": id, "tenant_name": name, "datamart_company_id": "1"}),
        )
        .unwrap()
    }

    #[test]
    fn a_sign_in_sees_only_the_companies_it_connected_and_names_the_unconfigured_ones() {
        let t = Tenants::for_sign_in(
            vec![
                configured("t-aq", "Aquatiq AS"),
                configured("t-ab", "Aquatiq AB"),
            ],
            &[
                ("t-aq".into(), None),
                ("t-xx".into(), Some("Aqua Pharma Inc".into())),
                ("t-yy".into(), None),
            ],
            "t-aq",
        );
        let names: Vec<String> = t
            .describe()
            .iter()
            .map(|d| d["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["Aqua Pharma Inc", "Aquatiq AS", "t-yy"]);
        assert!(
            t.resolve(Some("Aquatiq AB")).is_err(),
            "a company that was not connected is refused"
        );
        assert_eq!(t.resolve(None).unwrap().as_str(), "t-aq");
        assert_eq!(t.resolve(Some("aqua pharma inc")).unwrap().as_str(), "t-xx");
    }

    #[test]
    fn a_sign_in_without_the_default_company_has_no_default() {
        let t = Tenants::for_sign_in(
            vec![configured("t-aq", "Aquatiq AS")],
            &[("t-xx".into(), Some("Aqua Pharma Inc".into()))],
            "t-aq",
        );
        let e = t.resolve(None).unwrap_err().to_string();
        assert!(
            e.contains("no default company") && e.contains("Aqua Pharma Inc"),
            "{e}"
        );
        assert!(t.resolve(Some("t-xx")).is_ok());
        assert!(t.describe().iter().all(|d| d["is_default"] == false));
    }

    #[test]
    fn describes_tenants_sorted_and_marks_the_default() {
        let d = test_tenants().describe();
        assert_eq!(d[0]["name"], "Aquatiq AB");
        assert_eq!(d[1]["is_default"], true);
        assert_eq!(d[1]["datamart_company_id"], "4839725");
        assert_eq!(
            test_tenants().company_id(&TenantId::new("t-aq")),
            Some("4839725")
        );
    }
}
