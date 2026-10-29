use crate::{
    backend::{Failure, Query, Response},
    catalog,
    config::AccessScope,
};
use base64::Engine;
use rmcp::model::ResourceContents;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

pub const MAX_RESULT_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_MODEL_BYTES: usize = 60 * 1024;
const TTL: Duration = Duration::from_secs(600);
#[derive(Clone)]
struct Stored {
    owner: String,
    tenant: String,
    sensitive: bool,
    response: Response,
    created: Instant,
}
pub struct Results {
    entries: Mutex<HashMap<Uuid, Stored>>,
}
impl Default for Results {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadArgs {
    pub result_id: Uuid,
    #[serde(default)]
    pub pointer: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub fields: Vec<String>,
}
fn default_limit() -> usize {
    50
}

impl Results {
    pub async fn capture(
        &self,
        scope: &AccessScope,
        request: &Query,
        response: Response,
    ) -> Result<Value, Failure> {
        let op = catalog::operation(&request.family, &request.operation_id)?;
        if response.body.len() > MAX_RESULT_BYTES {
            return Err(Failure::new("result_too_large","The response exceeds the 5 MiB interactive limit; narrow the query or use extraction.",false));
        }
        let binary = op.binary();
        let original = if binary || response.http_status == 304 {
            Value::Null
        } else {
            serde_json::from_slice::<Value>(&response.body).map_err(|_| {
                Failure::new("invalid_xero_response", "Xero returned invalid JSON.", true)
            })?
        };
        if !binary && response.http_status != 304 && !original.is_object() && !original.is_array() {
            return Err(Failure::new(
                "invalid_xero_response",
                "Xero returned an unexpected JSON shape.",
                true,
            ));
        }
        let projected = preview(&original, request.limit);
        let (available_paths, available_paths_truncated) = paths(&original);
        let too_large = serde_json::to_vec(&projected)
            .map_err(|_| Failure::invalid("Cannot encode result."))?
            .len()
            > MAX_MODEL_BYTES;
        let truncated = projected != original || too_large;
        let hash = format!("{:x}", Sha256::digest(&response.body));
        let fetched = response.fetched_at.to_rfc3339();
        let bytes = response.body.len();
        let status = response.http_status;
        let mime = response.content_type.clone();
        let id = if !scope.zero_data_retention && response.http_status != 304 {
            let mut entries = self.entries.lock().await;
            entries.retain(|_, v| v.created.elapsed() < TTL);
            if entries.len() >= 32 {
                if let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, v)| v.created)
                    .map(|(id, _)| *id)
                {
                    entries.remove(&oldest);
                }
            }
            let id = Uuid::new_v4();
            entries.insert(
                id,
                Stored {
                    owner: scope.client_id.clone(),
                    tenant: request.tenant_id.clone(),
                    sensitive: op.sensitive(),
                    response,
                    created: Instant::now(),
                },
            );
            Some(id)
        } else {
            None
        };
        if binary && id.is_none() {
            return Err(Failure::new(
                "retention_policy",
                "Document handles are unavailable under zero-data-retention policy.",
                false,
            ));
        }
        Ok(
            json!({"data":if too_large {Value::Null} else {projected},"meta":{
                "tenant_id":request.tenant_id,"family":request.family,"operation_id":request.operation_id,
                "fetched_at":fetched,"http_status":status,"spec_commit":catalog::catalog().spec_commit,
                "coverage":"single_response","complete_collection":false,"truncated":truncated,
                "page":request.query.get("page"),"offset":request.query.get("offset"),
                "result_id":id,"resource_uri":id.map(|id|format!("xero://results/{id}")),
                "expires_in_seconds":id.map(|_|TTL.as_secs()),"bytes":bytes,"sha256":hash,"content_type":mime,
                "binary":binary,"zero_data_retention":scope.zero_data_retention,
                "identity_tenant_filtered":request.family=="identity",
                "available_paths":available_paths,"available_paths_truncated":available_paths_truncated,"source_content_trust":"untrusted_data"
            }}),
        )
    }

    pub async fn read(&self, scope: &AccessScope, args: ReadArgs) -> Result<Value, Failure> {
        if !(1..=100).contains(&args.limit)
            || args.offset > 1_000_000
            || args.pointer.len() > 256
            || args.fields.len() > 50
            || args
                .fields
                .iter()
                .any(|f| f.len() > 100 || f.chars().any(char::is_control))
        {
            return Err(Failure::invalid(
                "Invalid result paging or field projection options.",
            ));
        }
        let stored = {
            let entries = self.entries.lock().await;
            self.find(&entries, scope, args.result_id)?.clone()
        };
        if catalog_binary(&stored.response) {
            return Err(Failure::invalid("Use resources/read for binary documents."));
        }
        let value: Value = serde_json::from_slice(&stored.response.body)
            .map_err(|_| Failure::invalid("Stored result is not JSON."))?;
        let selected = value
            .pointer(&args.pointer)
            .ok_or_else(|| Failure::invalid("JSON pointer does not exist in this result."))?;
        let (data, next) = if let Some(array) = selected.as_array() {
            let rows = array
                .iter()
                .skip(args.offset)
                .take(args.limit)
                .map(|row| project_fields(row, &args.fields))
                .collect::<Vec<_>>();
            let next = (args.offset + rows.len() < array.len()).then_some(args.offset + rows.len());
            (Value::Array(rows), next)
        } else {
            if args.offset != 0 {
                return Err(Failure::invalid("offset applies only to arrays."));
            }
            (project_fields(selected, &args.fields), None)
        };
        if serde_json::to_vec(&data)
            .map_err(|_| Failure::invalid("Cannot encode result."))?
            .len()
            > MAX_MODEL_BYTES
        {
            return Err(Failure::new(
                "result_too_large",
                "Select a deeper JSON pointer, fewer fields, or a smaller limit.",
                false,
            ));
        }
        Ok(
            json!({"data":data,"meta":{"result_id":args.result_id,"tenant_id":stored.tenant,"pointer":args.pointer,"offset":args.offset,"next_offset":next,"fetched_at":stored.response.fetched_at,"coverage":"cached_response_slice","complete_collection":false}}),
        )
    }

    pub async fn resource(
        &self,
        scope: &AccessScope,
        id: Uuid,
        uri: &str,
    ) -> Result<ResourceContents, Failure> {
        let stored = {
            let entries = self.entries.lock().await;
            self.find(&entries, scope, id)?.clone()
        };
        if catalog_binary(&stored.response) {
            Ok(ResourceContents::blob(
                base64::engine::general_purpose::STANDARD.encode(&stored.response.body),
                uri,
            )
            .with_mime_type(stored.response.content_type.clone()))
        } else {
            if stored.response.body.len() > MAX_MODEL_BYTES {
                return Err(Failure::new(
                    "result_too_large",
                    "Use xero_read_result to page this resource by JSON pointer.",
                    false,
                ));
            }
            let text = std::str::from_utf8(&stored.response.body)
                .map_err(|_| Failure::invalid("Result is not UTF-8 JSON."))?;
            Ok(ResourceContents::text(text, uri).with_mime_type("application/json"))
        }
    }

    fn find<'a>(
        &self,
        entries: &'a HashMap<Uuid, Stored>,
        scope: &AccessScope,
        id: Uuid,
    ) -> Result<&'a Stored, Failure> {
        let unavailable = || {
            Failure::new(
                "result_unavailable",
                "This result has expired or is unavailable to the current client.",
                false,
            )
        };
        let stored = entries.get(&id).ok_or_else(unavailable)?;
        if scope.zero_data_retention
            || stored.owner != scope.client_id
            || stored.created.elapsed() >= TTL
        {
            return Err(unavailable());
        }
        scope.authorize(&stored.tenant, stored.sensitive, false)?;
        Ok(stored)
    }
}

fn catalog_binary(response: &Response) -> bool {
    !response
        .content_type
        .split(';')
        .next()
        .unwrap_or("")
        .eq_ignore_ascii_case("application/json")
}
fn preview(value: &Value, limit: usize) -> Value {
    match value {
        Value::Array(rows) => Value::Array(rows.iter().take(limit).cloned().collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, v)| {
                    (
                        key.clone(),
                        if let Value::Array(rows) = v {
                            Value::Array(rows.iter().take(limit).cloned().collect())
                        } else {
                            v.clone()
                        },
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}
fn project_fields(value: &Value, fields: &[String]) -> Value {
    if fields.is_empty() {
        return value.clone();
    }
    if let Some(object) = value.as_object() {
        Value::Object(
            object
                .iter()
                .filter(|(key, _)| fields.contains(key))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    } else {
        value.clone()
    }
}
fn paths(value: &Value) -> (Vec<String>, bool) {
    match value {
        Value::Object(object) => {
            let mut paths = Vec::new();
            let mut bytes = 2;
            for key in object.keys() {
                if paths.len() == 64 {
                    break;
                }
                if key.len() > 512 {
                    continue;
                }
                let path = format!("/{}", key.replace('~', "~0").replace('/', "~1"));
                let encoded = serde_json::to_vec(&path)
                    .expect("JSON string serialization")
                    .len()
                    + 1;
                if bytes + encoded > 2048 {
                    continue;
                }
                bytes += encoded;
                paths.push(path);
            }
            let truncated = paths.len() != object.len();
            (paths, truncated)
        }
        Value::Array(_) => (vec![String::new()], false),
        _ => (Vec::new(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wide_response_metadata_is_bounded_and_handle_stays_usable() {
        let results = Results::default();
        let request: Query = serde_json::from_value(
            json!({"tenant_id":TENANT,"family":"accounting","operation_id":"getInvoices"}),
        )
        .unwrap();
        let object = (0..2000)
            .map(|index| (format!("{index:04}{}", "x".repeat(60)), json!([index])))
            .collect::<serde_json::Map<_, _>>();
        let response = Response {
            body: serde_json::to_vec(&object).unwrap().into(),
            content_type: "application/json".into(),
            http_status: 200,
            fetched_at: chrono::Utc::now(),
        };
        let result = results.capture(&scope(), &request, response).await.unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() < 64 * 1024);
        assert_eq!(result["meta"]["available_paths_truncated"], true);
        let id = serde_json::from_value(result["meta"]["result_id"].clone()).unwrap();
        let slice = results
            .read(
                &scope(),
                ReadArgs {
                    result_id: id,
                    pointer: format!("/0000{}", "x".repeat(60)),
                    offset: 0,
                    limit: 1,
                    fields: vec![],
                },
            )
            .await
            .unwrap();
        assert_eq!(slice["data"], json!([0]));
    }
    const TENANT: &str = "5f3c9a2e-0000-0000-0000-000000000000";
    fn scope() -> AccessScope {
        AccessScope::new("fixture", vec![TENANT.into()], false, true, false).unwrap()
    }
    fn request(operation: &str) -> Query {
        serde_json::from_value(
            json!({"tenant_id":TENANT,"family":"accounting","operation_id":operation,"limit":1}),
        )
        .unwrap()
    }
    fn response(body: &[u8], content_type: &str, http_status: u16) -> Response {
        Response {
            body: body.to_vec().into(),
            content_type: content_type.into(),
            http_status,
            fetched_at: chrono::Utc::now(),
        }
    }
    fn read_args(id: Uuid) -> ReadArgs {
        ReadArgs {
            result_id: id,
            pointer: String::new(),
            offset: 0,
            limit: 1,
            fields: vec![],
        }
    }

    #[tokio::test]
    async fn malformed_oversized_and_not_modified_responses_are_not_misrepresented() {
        let results = Results::default();
        for body in [b"not-json".as_slice(), b"42".as_slice()] {
            assert_eq!(
                results
                    .capture(
                        &scope(),
                        &request("getInvoices"),
                        response(body, "application/json", 200)
                    )
                    .await
                    .unwrap_err()
                    .code,
                "invalid_xero_response"
            );
        }
        assert_eq!(
            results
                .capture(
                    &scope(),
                    &request("getInvoices"),
                    response(&vec![b' '; MAX_RESULT_BYTES + 1], "application/json", 200)
                )
                .await
                .unwrap_err()
                .code,
            "result_too_large"
        );
        let unchanged = results
            .capture(
                &scope(),
                &request("getInvoices"),
                response(b"", "application/json", 304),
            )
            .await
            .unwrap();
        assert_eq!(unchanged["meta"]["http_status"], 304);
        assert_eq!(unchanged["meta"]["result_id"], Value::Null);
        let large = serde_json::to_vec(
            &json!({"Invoices":[{"InvoiceID":"id","Description":"x".repeat(MAX_MODEL_BYTES)}]}),
        )
        .unwrap();
        let captured = results
            .capture(
                &scope(),
                &request("getInvoices"),
                response(&large, "application/json", 200),
            )
            .await
            .unwrap();
        assert_eq!(captured["data"], Value::Null);
        let id: Uuid = serde_json::from_value(captured["meta"]["result_id"].clone()).unwrap();
        assert!(results
            .resource(&scope(), id, "xero://results/fixture")
            .await
            .is_err());
        assert!(results.read(&scope(), read_args(id)).await.is_err());
        let args = ReadArgs {
            pointer: "/Invoices".into(),
            fields: vec!["InvoiceID".into()],
            ..read_args(id)
        };
        assert_eq!(
            results.read(&scope(), args).await.unwrap()["data"][0]["InvoiceID"],
            "id"
        );
    }
    #[tokio::test]
    async fn binary_resources_require_current_sensitive_permission_and_non_zdr() {
        let results = Results::default();
        let captured = results
            .capture(
                &scope(),
                &request("getInvoiceAsPdf"),
                response(b"%PDF-fixture", "application/pdf", 200),
            )
            .await
            .unwrap();
        let id: Uuid = serde_json::from_value(captured["meta"]["result_id"].clone()).unwrap();
        assert_eq!(captured["meta"]["binary"], true);
        assert!(results.read(&scope(), read_args(id)).await.is_err());
        let resource = serde_json::to_value(
            results
                .resource(&scope(), id, "xero://results/fixture")
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(resource["mimeType"], "application/pdf");
        assert!(resource["blob"].is_string());
        assert!(results
            .resource(
                &AccessScope {
                    allow_sensitive: false,
                    ..scope()
                },
                id,
                "xero://results/fixture"
            )
            .await
            .is_err());
        assert!(results
            .capture(
                &AccessScope {
                    zero_data_retention: true,
                    ..scope()
                },
                &request("getInvoiceAsPdf"),
                response(b"%PDF", "application/pdf", 200)
            )
            .await
            .is_err());
    }
    #[tokio::test]
    async fn result_cache_expires_evicts_and_validates_projection_parameters() {
        let results = Results::default();
        let captured = results
            .capture(
                &scope(),
                &request("getInvoices"),
                response(
                    br#"{"Invoices":[{"InvoiceID":"one"},{"InvoiceID":"two"}]}"#,
                    "application/json",
                    200,
                ),
            )
            .await
            .unwrap();
        let id: Uuid = serde_json::from_value(captured["meta"]["result_id"].clone()).unwrap();
        for args in [
            ReadArgs {
                limit: 0,
                ..read_args(id)
            },
            ReadArgs {
                offset: 1_000_001,
                ..read_args(id)
            },
            ReadArgs {
                pointer: "x".repeat(257),
                ..read_args(id)
            },
            ReadArgs {
                fields: vec!["\n".into()],
                ..read_args(id)
            },
            ReadArgs {
                pointer: "/missing".into(),
                ..read_args(id)
            },
            ReadArgs {
                offset: 1,
                ..read_args(id)
            },
        ] {
            assert!(results.read(&scope(), args).await.is_err());
        }
        let args = ReadArgs {
            pointer: "/Invoices".into(),
            fields: vec!["InvoiceID".into()],
            ..read_args(id)
        };
        assert_eq!(
            results.read(&scope(), args).await.unwrap()["meta"]["next_offset"],
            1
        );
        results.entries.lock().await.get_mut(&id).unwrap().created = Instant::now() - TTL;
        assert!(results.read(&scope(), read_args(id)).await.is_err());
        for _ in 0..33 {
            results
                .capture(
                    &scope(),
                    &request("getInvoices"),
                    response(b"[]", "application/json", 200),
                )
                .await
                .unwrap();
        }
        assert_eq!(results.entries.lock().await.len(), 32);
        assert_eq!(paths(&json!({"a/b~c":1})), (vec!["/a~1b~0c".into()], false));
        assert_eq!(project_fields(&json!(1), &["id".into()]), json!(1));
    }
    #[tokio::test]
    async fn bounded_preview_and_handles_preserve_client_and_tenant_authority() {
        let tenant = "5f3c9a2e-0000-0000-0000-000000000000";
        let scope = AccessScope::new("first", vec![tenant.into()], false, false, false).unwrap();
        let other = AccessScope::new("second", vec![tenant.into()], false, false, false).unwrap();
        let results = Results::default();
        let request:Query=serde_json::from_value(json!({"tenant_id":tenant,"family":"accounting","operation_id":"getInvoices","limit":1})).unwrap();
        let captured = results
            .capture(
                &scope,
                &request,
                Response {
                    body: serde_json::to_vec(
                        &json!({"Invoices":[{"InvoiceID":"a"},{"InvoiceID":"b"}]}),
                    )
                    .unwrap()
                    .into(),
                    content_type: "application/json".into(),
                    http_status: 200,
                    fetched_at: chrono::Utc::now(),
                },
            )
            .await
            .unwrap();
        assert_eq!(captured["data"]["Invoices"].as_array().unwrap().len(), 1);
        assert_eq!(captured["meta"]["truncated"], true);
        let id = serde_json::from_value(captured["meta"]["result_id"].clone()).unwrap();
        let args = || ReadArgs {
            result_id: id,
            pointer: "/Invoices".into(),
            offset: 1,
            limit: 1,
            fields: vec![],
        };
        assert!(results.read(&other, args()).await.is_err());
        assert_eq!(
            results.read(&scope, args()).await.unwrap()["data"][0]["InvoiceID"],
            "b"
        );
        let zdr = AccessScope {
            zero_data_retention: true,
            ..scope
        };
        assert!(results.read(&zdr, args()).await.is_err());
    }
}
