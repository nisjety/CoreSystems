use crate::backend::Failure;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::OnceLock};
use xero_client::accounting_get::{
    AccountingGetOperation, AccountingGetParameter, PreparedAccountingGet,
};

pub type Params = BTreeMap<String, Value>;

#[derive(Deserialize, Serialize)]
pub struct Catalog {
    pub spec_commit: String,
    pub sources: Value,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Operation {
    pub family: String,
    pub operation_id: String,
    pub path: String,
    pub base_url: String,
    pub summary: String,
    pub parameters: Vec<Parameter>,
    pub response_content_types: Vec<String>,
    pub documented_scopes: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Parameter {
    pub name: String,
    pub location: String,
    pub required: bool,
    pub schema: Value,
    pub description: String,
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../catalog.json"))
            .expect("tested pinned Xero MCP catalog")
    })
}

const WAREHOUSE_EXCLUDED_OPERATION_IDS: &[&str] =
    &["getPaymentServices", "getBrandingThemePaymentServices"];

pub fn operations() -> &'static [Operation] {
    static OPERATIONS: OnceLock<Vec<Operation>> = OnceLock::new();
    OPERATIONS
        .get_or_init(|| {
            catalog()
                .operations
                .iter()
                .filter(|operation| {
                    !WAREHOUSE_EXCLUDED_OPERATION_IDS.contains(&operation.operation_id.as_str())
                })
                .cloned()
                .collect()
        })
        .as_slice()
}
pub fn operation(family: &str, id: &str) -> Result<&'static Operation, Failure> {
    operations()
        .iter()
        .find(|op| op.family == family && op.operation_id == id)
        .ok_or_else(|| Failure::invalid("Unknown catalogued Xero GET operation."))
}

impl Operation {
    pub fn tool_name(&self) -> String {
        format!("xero_{}_{}", self.family, self.operation_id)
    }
    pub fn binary(&self) -> bool {
        self.response_content_types
            .iter()
            .any(|s| s == "application/pdf" || s == "application/octet-stream")
    }
    pub fn sensitive(&self) -> bool {
        self.binary()
            || matches!(
                self.family.as_str(),
                "finance" | "bankfeeds" | "identity" | "payroll_nz"
            )
    }

    pub fn tool_schema(&self) -> Value {
        let mut properties = serde_json::Map::from_iter([
            (
                "tenant_id".into(),
                json!({"type":"string","format":"uuid","description":"Explicit Xero tenant authorized for this client."}),
            ),
            (
                "limit".into(),
                json!({"type":"integer","minimum":1,"maximum":100,"default":50,"description":"Maximum rows included in the model response; Xero pagination is explicit."}),
            ),
        ]);
        let mut required = vec!["tenant_id"];
        for location in ["path", "query", "header"] {
            let params = self
                .parameters
                .iter()
                .filter(|p| p.location == location)
                .collect::<Vec<_>>();
            if params.is_empty() {
                continue;
            }
            let mut fields = serde_json::Map::new();
            for p in &params {
                let mut schema = p.schema.clone();
                if !schema.is_object() {
                    schema = json!({"type":"string"});
                }
                schema["description"] = json!(p.description);
                if schema.get("type").is_none() {
                    schema["type"] = json!("string");
                }
                if p.name == "pageSize" {
                    schema["minimum"] = json!(1);
                    schema["maximum"] = json!(100);
                }
                if p.name == "page" {
                    schema["minimum"] = json!(1);
                    schema["maximum"] = json!(1_000_000);
                }
                fields.insert(p.name.clone(), schema);
            }
            let required_fields = params
                .iter()
                .filter(|p| p.required)
                .map(|p| &p.name)
                .collect::<Vec<_>>();
            if !required_fields.is_empty() {
                required.push(location);
            }
            properties.insert(location.into(), json!({"type":"object","properties":fields,"required":required_fields,"additionalProperties":false}));
        }
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }

    pub fn prepare(
        &self,
        path: &Params,
        query: &Params,
        headers: &Params,
    ) -> Result<PreparedAccountingGet, Failure> {
        let path = self.pairs("path", path)?;
        let query = self.pairs("query", query)?;
        let headers = self.pairs("header", headers)?;
        let op = AccountingGetOperation {
            operation_id: self.operation_id.clone(),
            path: self.path.clone(),
            parameters: self
                .parameters
                .iter()
                .map(|p| AccountingGetParameter {
                    name: p.name.clone(),
                    location: p.location.clone(),
                    required: p.required,
                    kind: p.schema["type"].as_str().unwrap_or("string").into(),
                    format: p.schema["format"].as_str().map(String::from),
                })
                .collect(),
            response_content_types: self.response_content_types.clone(),
        };
        let mut prepared = op.prepare(&path, &query, &headers).map_err(|_| Failure::invalid("Missing, invalid or unsupported Xero request parameter; inspect this operation's schema."))?;
        if self.family != "accounting" {
            let suffix = prepared
                .url
                .as_str()
                .strip_prefix("https://api.xero.com/api.xro/2.0")
                .ok_or_else(|| Failure::invalid("Invalid catalogued path."))?;
            prepared.url = format!("{}{suffix}", self.base_url)
                .parse()
                .map_err(|_| Failure::invalid("Invalid catalogued URL."))?;
        }
        Ok(prepared)
    }

    pub fn pairs(&self, location: &str, values: &Params) -> Result<Vec<(String, String)>, Failure> {
        let mut pairs = Vec::new();
        for (name, value) in values {
            let p = self
                .parameters
                .iter()
                .find(|p| p.location == location && p.name == *name)
                .ok_or_else(|| Failure::invalid("Unsupported Xero request parameter."))?;
            let schema = &p.schema;
            let kind = schema["type"].as_str().unwrap_or("string");
            let text = match kind {
                "integer" if value.is_i64() || value.is_u64() => value.to_string(),
                "number" if value.is_number() => value.to_string(),
                "boolean" if value.is_boolean() => value.to_string(),
                "array" if value.is_array() => {
                    let items = value.as_array().unwrap();
                    if items.len() > 100 {
                        return Err(Failure::invalid("At most 100 filter values are allowed."));
                    }
                    items
                        .iter()
                        .map(|v| {
                            let value = v.as_str().ok_or_else(|| {
                                Failure::invalid("Filter array values must be strings.")
                            })?;
                            if value.is_empty()
                                || value.contains(',')
                                || value.chars().any(char::is_control)
                            {
                                return Err(Failure::invalid(
                                    "Filter array items must be nonempty, unambiguous values.",
                                ));
                            }
                            if schema["items"]["format"] == "uuid"
                                && uuid::Uuid::parse_str(value).is_err()
                            {
                                return Err(Failure::invalid("Filter array items must be UUIDs."));
                            }
                            Ok(value.to_owned())
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(",")
                }
                "string" if value.is_string() => value.as_str().unwrap().into(),
                _ => {
                    return Err(Failure::invalid(
                        "Request parameter type does not match the catalogued schema.",
                    ))
                }
            };
            if text.is_empty()
                || text.len() > 512
                || text.chars().any(char::is_control)
                || (location == "path" && matches!(text.as_str(), "." | ".."))
            {
                return Err(Failure::invalid("Invalid request parameter value."));
            }
            if let Some(choices) = schema["enum"].as_array() {
                if !choices.contains(value) {
                    return Err(Failure::invalid(
                        "Request parameter is not a supported enum value.",
                    ));
                }
            }
            if name == "pageSize" && !(1..=100).contains(&value.as_u64().unwrap_or(0)) {
                return Err(Failure::invalid("pageSize must be between 1 and 100."));
            }
            if name == "page" && !(1..=1_000_000).contains(&value.as_u64().unwrap_or(0)) {
                return Err(Failure::invalid("page must be between 1 and 1000000."));
            }
            pairs.push((name.clone(), text));
        }
        Ok(pairs)
    }
}

pub fn manifest() -> Value {
    json!({
        "schema_version":"1.0", "provider_key":"xero", "server_name":"xero-mcp",
        "transports":["stdio","streamable-http"], "mcp_path":"/mcp",
        "spec_commit":catalog().spec_commit,
        "families":["accounting","assets","bankfeeds","projects","identity","finance","payroll_nz"],
        "catalog_operation_count":catalog().operations.len(), "operation_count":operations().len(), "source_writes":false,
        "execution_owner":"Model Plane", "connection_owner":"Ingestion Plane integration core (future adapter)",
        "authority":"Bearer service credentials bind client identity and Xero tenant allowlists; tool arguments never grant authority.",
        "data_contract":"data + meta provenance; explicit single-response coverage; resource handles are tenant/client-scoped and expire after 10 minutes",
        "extraction":"Asynchronous local/private-GCS raw extraction; explicit permission, destination and idempotency key; ZDR rejects persistence",
        "tools":operations().iter().map(|op| json!({"name":op.tool_name(),"family":op.family,"operation_id":op.operation_id,"read_only":true,"sensitive":op.sensitive(),"documented_scopes":op.documented_scopes})).collect::<Vec<_>>(),
        "operational_tools":["xero_list_connections","xero_list_operations","xero_list_write_operations","xero_warehouse_overview","xero_warehouse_metric","xero_business_read","xero_financial_report","xero_payroll_nz_read","xero_build_ui_link","xero_query","xero_read_result","xero_extract","xero_get_job","xero_cancel_job"],
        "reference_projects":["https://github.com/XeroAPI/xero-mcp-server","https://github.com/XeroAPI/xero-agent-toolkit","https://github.com/XeroAPI/xero-command-line","https://github.com/XeroAPI/xero-prompt-library"]
    })
}
