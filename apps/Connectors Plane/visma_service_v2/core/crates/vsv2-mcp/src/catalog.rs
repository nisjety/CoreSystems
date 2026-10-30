//! Read-only catalog of the endpoint configs in `config/endpoints/*.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub endpoint_name: String,
    #[serde(default)]
    pub bigquery_table: Option<String>,
    #[serde(default)]
    pub primary_key: Value,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub api_endpoint: Option<String>,
    #[serde(default)]
    pub pagination_profile: Option<String>,
    #[serde(default)]
    pub page_size: Option<u32>,
    #[serde(default)]
    pub tier: Option<u32>,
    #[serde(default)]
    pub modified_field: Option<String>,
    #[serde(default)]
    pub default_query: Value,
    #[serde(default)]
    pub required_query: Vec<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub datamart: Option<Value>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
}

impl Catalog {
    pub fn from_entries(mut entries: Vec<CatalogEntry>) -> Self {
        entries.sort_by(|a, b| a.endpoint_name.cmp(&b.endpoint_name));
        Self { entries }
    }

    /// Load every `*.json` in `dir`. A file that does not parse is an error: a silently missing
    /// endpoint would make the catalog lie.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let mut entries = Vec::new();
        for f in std::fs::read_dir(dir)? {
            let path = f?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // `_schema.json` is the JSON Schema for the configs, not an endpoint.
            if path.extension().and_then(|e| e.to_str()) != Some("json") || name.starts_with('_') {
                continue;
            }
            let raw = std::fs::read_to_string(&path)?;
            let e: CatalogEntry = serde_json::from_str(&raw)
                .map_err(|err| anyhow::anyhow!("{}: {err}", path.display()))?;
            entries.push(e);
        }
        Ok(Self::from_entries(entries))
    }

    pub fn all(&self) -> &[CatalogEntry] {
        &self.entries
    }

    pub fn get(&self, name: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|e| e.endpoint_name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_sorted_entries_and_ignores_non_json_and_errors_on_bad_json() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("b.json"),
            r#"{"endpoint_name":"b","source":"erp_rest"}"#,
        )
        .unwrap();
        std::fs::write(
            d.path().join("a.json"),
            r#"{"endpoint_name":"a","source":"erp_rest","tier":2}"#,
        )
        .unwrap();
        std::fs::write(d.path().join("readme.txt"), "x").unwrap();
        std::fs::write(
            d.path().join("_schema.json"),
            r#"{"$schema":"x","type":"object"}"#,
        )
        .unwrap();
        let c = Catalog::load(d.path()).unwrap();
        assert_eq!(
            c.all()
                .iter()
                .map(|e| e.endpoint_name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(c.get("a").unwrap().tier, Some(2));
        assert!(c.get("zzz").is_none());

        std::fs::write(d.path().join("bad.json"), "{").unwrap();
        assert!(Catalog::load(d.path()).is_err());
    }
}
