//! Pinned OpenAPI catalog for read-only Accounting GET operations.

use std::{collections::HashSet, sync::OnceLock};

use bytes::{Bytes, BytesMut};
use chrono::{DateTime, Utc};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Url,
};
use serde::Deserialize;
use xero_common::{Error, Result};

use crate::{rate_limit, retry, XeroApiClient, BASE_URL};

const MAX_RESPONSE_BYTES: usize = 50 * 1024 * 1024;
const WAREHOUSE_EXCLUDED_OPERATION_IDS: &[&str] =
    &["getPaymentServices", "getBrandingThemePaymentServices"];

fn ensure_warehouse_operation_in_scope(operation_id: &str) -> Result<()> {
    if WAREHOUSE_EXCLUDED_OPERATION_IDS.contains(&operation_id) {
        return Err(Error::Config(format!(
            "Accounting GET operation is out of scope for the Xero warehouse: {operation_id}"
        )));
    }
    Ok(())
}

fn prepared_request_is_out_of_scope(url: &Url) -> bool {
    let path = url.path().trim_end_matches('/');
    path.ends_with("/PaymentServices")
        && (path == "/api.xro/2.0/PaymentServices"
            || (path.starts_with("/api.xro/2.0/BrandingThemes/") && path.matches('/').count() == 5))
}

#[derive(Deserialize)]
pub struct AccountingGetCatalog {
    pub spec_commit: String,
    pub spec_sha256: String,
    pub operations: Vec<AccountingGetOperation>,
}

#[derive(Deserialize)]
pub struct AccountingGetOperation {
    pub operation_id: String,
    pub path: String,
    pub parameters: Vec<AccountingGetParameter>,
    pub response_content_types: Vec<String>,
}

#[derive(Deserialize)]
pub struct AccountingGetParameter {
    pub name: String,
    pub location: String,
    pub required: bool,
    #[serde(rename = "type")]
    pub kind: String,
    pub format: Option<String>,
}

pub struct PreparedAccountingGet {
    pub url: Url,
    pub headers: HeaderMap,
    pub binary: bool,
    pub accept: &'static str,
}

pub struct AccountingGetResponse {
    pub body: Bytes,
    pub content_type: String,
    pub http_status: u16,
    pub fetched_at: DateTime<Utc>,
}

pub fn catalog() -> &'static AccountingGetCatalog {
    static CATALOG: OnceLock<AccountingGetCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../accounting_get_catalog.json"))
            .expect("checked-in Accounting GET catalog must be valid")
    })
}

pub fn operation(id: &str) -> Result<&'static AccountingGetOperation> {
    ensure_warehouse_operation_in_scope(id)?;
    catalog()
        .operations
        .iter()
        .find(|operation| operation.operation_id == id)
        .ok_or_else(|| Error::Config(format!("unknown Accounting GET operation: {id}")))
}

/// Return the catalogued JSON GET operation for an Accounting collection's
/// UUID detail route. Keeping this lookup in the pinned catalog prevents the
/// collection synchronizer from treating a hand-built URL as undocumented.
pub fn uuid_detail_operation(
    entity: &xero_common::EntityType,
) -> Result<&'static AccountingGetOperation> {
    if !entity.has_uuid_detail() {
        return Err(Error::Config(format!(
            "{entity} has no catalogued UUID detail operation"
        )));
    }
    let expected_path = format!("/{}/{{{}}}", entity.xero_path(), entity.id_field());
    catalog()
        .operations
        .iter()
        .find(|candidate| candidate.path == expected_path && !candidate.is_binary())
        .ok_or_else(|| {
            Error::Config(format!(
                "{} has no catalogued JSON UUID detail operation for {expected_path}",
                entity
            ))
        })
}

impl AccountingGetOperation {
    pub fn is_binary(&self) -> bool {
        self.response_content_types.iter().any(|content_type| {
            matches!(
                content_type.as_str(),
                "application/pdf" | "application/octet-stream"
            )
        })
    }

    pub fn prepare(
        &self,
        path_params: &[(String, String)],
        query: &[(String, String)],
        headers: &[(String, String)],
    ) -> Result<PreparedAccountingGet> {
        ensure_warehouse_operation_in_scope(&self.operation_id)?;
        self.validate_params("path", path_params)?;
        self.validate_params("query", query)?;
        self.validate_params("header", headers)?;
        let mut url = Url::parse(&format!("{BASE_URL}/"))
            .map_err(|error| Error::Config(error.to_string()))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| Error::Config("invalid Accounting base URL".into()))?;
            segments.pop_if_empty();
            for template in self.path.trim_start_matches('/').split('/') {
                let value = if template.starts_with('{') && template.ends_with('}') {
                    let name = &template[1..template.len() - 1];
                    path_params
                        .iter()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value.as_str())
                        .ok_or_else(|| Error::Config(format!("missing path parameter {name}")))?
                } else {
                    template
                };
                segments.push(value);
            }
        }
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(name, value)| (name, value)));
        }
        let mut header_map = HeaderMap::new();
        for (name, value) in headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::Config(format!("invalid header name {name}")))?;
            let value = HeaderValue::from_str(value)
                .map_err(|_| Error::Config("invalid Accounting header value".into()))?;
            header_map.insert(name, value);
        }
        Ok(PreparedAccountingGet {
            url,
            headers: header_map,
            binary: self.is_binary(),
            accept: if self
                .response_content_types
                .iter()
                .any(|kind| kind == "application/pdf")
            {
                "application/pdf"
            } else if self.is_binary() {
                "*/*"
            } else {
                "application/json"
            },
        })
    }

    fn validate_params(&self, location: &str, values: &[(String, String)]) -> Result<()> {
        let mut provided = HashSet::new();
        for (name, value) in values {
            if !provided.insert(name.as_str()) {
                return Err(Error::Config(format!(
                    "duplicate {location} parameter {name}"
                )));
            }
            if name.eq_ignore_ascii_case("xero-tenant-id")
                || name.eq_ignore_ascii_case("authorization")
                || name.eq_ignore_ascii_case("accept")
            {
                return Err(Error::Config(
                    "authorization headers are system-managed".into(),
                ));
            }
            let parameter = self
                .parameters
                .iter()
                .find(|parameter| parameter.location == location && parameter.name == *name)
                .ok_or_else(|| Error::Config(format!("unsupported {location} parameter {name}")))?;
            validate_value(parameter, value)?;
        }
        for parameter in self.parameters.iter().filter(|parameter| {
            parameter.location == location
                && parameter.required
                && parameter.name != "xero-tenant-id"
        }) {
            if !provided.contains(parameter.name.as_str()) {
                return Err(Error::Config(format!(
                    "missing {location} parameter {}",
                    parameter.name
                )));
            }
        }
        Ok(())
    }
}

fn validate_value(parameter: &AccountingGetParameter, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(Error::Config(format!(
            "invalid value for {}",
            parameter.name
        )));
    }
    if parameter.location == "path" && (value.contains('/') || value.contains('\\')) {
        return Err(Error::Config(format!(
            "invalid path value for {}",
            parameter.name
        )));
    }
    match (parameter.kind.as_str(), parameter.format.as_deref()) {
        (_, Some("uuid")) => {
            uuid::Uuid::parse_str(value)
                .map_err(|_| Error::Config(format!("{} must be a UUID", parameter.name)))?;
        }
        (_, Some("date")) => {
            chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| Error::Config(format!("{} must be a date", parameter.name)))?;
        }
        ("integer", _) => {
            value
                .parse::<i64>()
                .map_err(|_| Error::Config(format!("{} must be an integer", parameter.name)))?;
        }
        ("boolean", _) if value != "true" && value != "false" => {
            return Err(Error::Config(format!(
                "{} must be true or false",
                parameter.name
            )));
        }
        _ => {}
    }
    Ok(())
}

impl XeroApiClient {
    pub async fn fetch_accounting_get(
        &self,
        access_token: &str,
        prepared: &PreparedAccountingGet,
    ) -> Result<AccountingGetResponse> {
        self.fetch_accounting_get_with_limit(access_token, prepared, MAX_RESPONSE_BYTES)
            .await
    }

    /// Bound interactive reads while retaining the raw extraction ceiling.
    pub async fn fetch_accounting_get_with_limit(
        &self,
        access_token: &str,
        prepared: &PreparedAccountingGet,
        max_response_bytes: usize,
    ) -> Result<AccountingGetResponse> {
        if prepared_request_is_out_of_scope(&prepared.url) {
            return Err(Error::Config(
                "Accounting GET operation is out of scope for the Xero warehouse".into(),
            ));
        }
        if max_response_bytes == 0 {
            return Err(Error::Config("Response byte limit must be positive".into()));
        }
        let max_response_bytes = max_response_bytes.min(MAX_RESPONSE_BYTES);
        let limiter = rate_limit::for_tenant(&self.tenant_id);
        let mut attempt = 0;
        loop {
            let permit = limiter.acquire().await?;
            let mut request = self
                .http
                .get(prepared.url.clone())
                .bearer_auth(access_token)
                .header("Accept", prepared.accept)
                .headers(prepared.headers.clone());
            if self.send_tenant_header {
                request = request.header("Xero-tenant-id", &self.tenant_id);
            }
            match request.send().await {
                Ok(mut response) => {
                    limiter.update_from_headers(response.headers()).await?;
                    let status = response.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let wait = retry::retry_after_or_default(response.headers());
                        limiter.defer_for(wait).await?;
                        if attempt < retry::MAX_ATTEMPTS {
                            drop(permit);
                            attempt += 1;
                            continue;
                        }
                    }
                    if status.is_server_error() && attempt < retry::MAX_ATTEMPTS {
                        let wait = retry::exp_backoff(attempt);
                        drop(permit);
                        tokio::time::sleep(wait).await;
                        attempt += 1;
                        continue;
                    }
                    if !status.is_success() && status != reqwest::StatusCode::NOT_MODIFIED {
                        return Err(Error::XeroApi(format!(
                            "Accounting GET failed (HTTP {status})"
                        )));
                    }
                    if response
                        .content_length()
                        .is_some_and(|size| size > max_response_bytes as u64)
                    {
                        return Err(Error::XeroApi(
                            "Xero GET response exceeds the configured byte limit".into(),
                        ));
                    }
                    let content_type = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("application/octet-stream")
                        .to_owned();
                    if status != reqwest::StatusCode::NOT_MODIFIED {
                        let mime = content_type
                            .split(';')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_ascii_lowercase();
                        let expects_pdf = prepared.accept == "application/pdf";
                        if (!prepared.binary && mime != "application/json")
                            || (expects_pdf && mime != "application/pdf")
                            || (prepared.binary && (mime.contains("json") || mime == "text/html"))
                        {
                            return Err(Error::XeroApi(format!(
                                "Accounting GET returned unexpected content type: {content_type}"
                            )));
                        }
                    }
                    let mut body = BytesMut::new();
                    while let Some(chunk) = response.chunk().await.map_err(|error| {
                        Error::XeroApi(format!("Accounting GET body read failed: {error}"))
                    })? {
                        if body.len().saturating_add(chunk.len()) > max_response_bytes {
                            return Err(Error::XeroApi(
                                "Xero GET response exceeds the configured byte limit".into(),
                            ));
                        }
                        body.extend_from_slice(&chunk);
                    }
                    return Ok(AccountingGetResponse {
                        body: body.freeze(),
                        content_type,
                        http_status: status.as_u16(),
                        fetched_at: Utc::now(),
                    });
                }
                Err(error)
                    if retry::is_transient_transport_err(&error)
                        && attempt < retry::MAX_ATTEMPTS =>
                {
                    let wait = retry::exp_backoff(attempt);
                    drop(permit);
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                Err(error) => return Err(Error::XeroApi(error.to_string())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn json_server(status: &str, body: &'static [u8]) -> Url {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/FeedConnections", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let status = status.to_owned();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
        });
        url
    }

    #[tokio::test]
    async fn catalogued_json_success_status_is_preserved() {
        let body = br#"{"feedConnections":[],"pagination":{"page":1}}"#;
        let request = PreparedAccountingGet {
            url: json_server("202 Accepted", body).await,
            headers: HeaderMap::new(),
            binary: false,
            accept: "application/json",
        };
        let client = XeroApiClient::new("status-fixture");
        let response = client
            .fetch_accounting_get("fixture-token", &request)
            .await
            .unwrap();
        assert_eq!(response.http_status, 202);
        assert_eq!(response.body.as_ref(), body);
        let request = PreparedAccountingGet {
            url: json_server("400 Bad Request", b"private diagnostics").await,
            ..request
        };
        let error = client
            .fetch_accounting_get("fixture-token", &request)
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("400"));
        assert!(!error.to_string().contains("private diagnostics"));
    }

    #[tokio::test]
    async fn interactive_limit_does_not_lower_the_extraction_limit() {
        let body = br#"{"Invoices":[{"InvoiceID":"fixture-record"}]}"#;
        let request = PreparedAccountingGet {
            url: json_server("200 OK", body).await,
            headers: HeaderMap::new(),
            binary: false,
            accept: "application/json",
        };
        let client = XeroApiClient::new("size-fixture");
        assert!(client
            .fetch_accounting_get_with_limit("fixture-token", &request, 16)
            .await
            .is_err());
        let request = PreparedAccountingGet {
            url: json_server("200 OK", body).await,
            ..request
        };
        assert_eq!(
            client
                .fetch_accounting_get("fixture-token", &request)
                .await
                .unwrap()
                .body
                .as_ref(),
            body
        );
    }

    #[tokio::test]
    async fn manually_constructed_payment_service_request_is_rejected_before_fetch() {
        let request = PreparedAccountingGet {
            url: Url::parse("https://api.xero.com/api.xro/2.0/PaymentServices").unwrap(),
            headers: HeaderMap::new(),
            binary: false,
            accept: "application/json",
        };
        let result = XeroApiClient::new("scope-fixture")
            .fetch_accounting_get("fixture-token", &request)
            .await;
        match result {
            Err(error) => assert!(error.to_string().contains("out of scope")),
            Ok(_) => panic!("payment service request was not rejected"),
        }
    }

    #[tokio::test]
    async fn interactive_limit_applies_to_chunked_bodies() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/Invoices", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n14\r\n01234567890123456789\r\n0\r\n\r\n").await.unwrap();
        });
        let request = PreparedAccountingGet {
            url,
            headers: HeaderMap::new(),
            binary: false,
            accept: "application/json",
        };
        assert!(XeroApiClient::new("chunked-fixture")
            .fetch_accounting_get_with_limit("fixture-token", &request, 16)
            .await
            .is_err());
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.into(), value.into())
    }

    #[test]
    fn uuid_detail_routes_resolve_through_the_pinned_catalog() {
        use xero_common::EntityType;

        let detail_entities = EntityType::all()
            .iter()
            .filter(|entity| entity.has_uuid_detail())
            .collect::<Vec<_>>();
        assert_eq!(detail_entities.len(), 24);
        for entity in detail_entities {
            let operation = uuid_detail_operation(entity).unwrap_or_else(|error| {
                panic!(
                    "{} is missing a catalogued detail operation: {error}",
                    entity.as_str()
                )
            });
            assert_eq!(
                operation.path,
                format!("/{}/{{{}}}", entity.xero_path(), entity.id_field())
            );
            assert!(!operation.is_binary());
        }
        assert_eq!(
            uuid_detail_operation(&EntityType::Bills)
                .unwrap()
                .operation_id,
            "getInvoice"
        );
        assert!(uuid_detail_operation(&EntityType::Currencies).is_err());
    }

    #[test]
    fn catalog_contains_every_pinned_accounting_get() {
        assert_eq!(catalog().operations.len(), 126);
        assert_eq!(
            catalog().spec_commit,
            "fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f"
        );
        assert!(operation("getInvoice").is_ok());
        assert!(operation("getInvoiceAsPdf").unwrap().is_binary());
        assert!(operation("getInvoiceAttachmentById").unwrap().is_binary());
        assert!(!operation("getInvoiceHistory").unwrap().is_binary());
        assert!(operation("getPaymentServices").is_err());
        assert!(operation("getBrandingThemePaymentServices").is_err());
        for operation_id in ["getPaymentServices", "getBrandingThemePaymentServices"] {
            let operation = catalog()
                .operations
                .iter()
                .find(|operation| operation.operation_id == operation_id)
                .unwrap();
            match operation.prepare(&[], &[], &[]) {
                Err(error) => assert!(error.to_string().contains("out of scope")),
                Ok(_) => panic!("{operation_id} was not rejected"),
            }
        }
        assert!(operation("deleteInvoice").is_err());
    }

    #[test]
    fn path_query_and_required_header_are_validated() {
        let invoice = operation("getInvoice").unwrap();
        let id = "a03ffcd2-5d91-4c7e-b483-318584e9e439";
        let prepared = invoice.prepare(&[pair("InvoiceID", id)], &[], &[]).unwrap();
        assert_eq!(prepared.url.path(), format!("/api.xro/2.0/Invoices/{id}"));
        assert!(invoice
            .prepare(&[pair("InvoiceID", "bad")], &[], &[])
            .is_err());
        assert!(invoice.prepare(&[pair("wrong", id)], &[], &[]).is_err());

        let aged = operation("getReportAgedPayablesByContact").unwrap();
        assert!(aged.prepare(&[], &[], &[]).is_err());
        assert!(aged.prepare(&[], &[pair("contactId", id)], &[]).is_ok());
        assert!(aged.prepare(&[], &[pair("unexpected", "x")], &[]).is_err());

        let attachment = operation("getInvoiceAttachmentById").unwrap();
        let params = [pair("InvoiceID", id), pair("AttachmentID", id)];
        assert!(attachment.prepare(&params, &[], &[]).is_err());
        assert!(attachment
            .prepare(&params, &[], &[pair("contentType", "application/pdf")])
            .is_ok());
        assert!(attachment
            .prepare(&params, &[], &[pair("Authorization", "bad")])
            .is_err());
        assert!(attachment
            .prepare(&params, &[], &[pair("AUTHORIZATION", "bad")])
            .is_err());
        assert!(attachment
            .prepare(&params, &[], &[pair("XERO-TENANT-ID", "bad")])
            .is_err());

        let pdf = operation("getInvoiceAsPdf").unwrap();
        assert_eq!(
            pdf.prepare(&[pair("InvoiceID", id)], &[], &[])
                .unwrap()
                .accept,
            "application/pdf"
        );
    }

    #[test]
    fn pdf_operations_use_the_spec_effective_path() {
        let id = "a03ffcd2-5d91-4c7e-b483-318584e9e439";
        for (operation_id, parameter, collection) in [
            ("getInvoiceAsPdf", "InvoiceID", "Invoices"),
            ("getCreditNoteAsPdf", "CreditNoteID", "CreditNotes"),
            ("getPurchaseOrderAsPdf", "PurchaseOrderID", "PurchaseOrders"),
            ("getQuoteAsPdf", "QuoteID", "Quotes"),
        ] {
            let pdf = operation(operation_id).unwrap();
            let prepared = pdf.prepare(&[pair(parameter, id)], &[], &[]).unwrap();
            assert_eq!(
                prepared.url.path(),
                format!("/api.xro/2.0/{collection}/{id}")
            );
            assert_eq!(prepared.accept, "application/pdf");
        }
    }

    #[test]
    fn every_catalogued_get_can_prepare_with_required_parameters() {
        let mut callable = 0;
        for operation in &catalog().operations {
            let values = |location: &str| {
                operation
                    .parameters
                    .iter()
                    .filter(|parameter| {
                        parameter.location == location
                            && parameter.required
                            && parameter.name != "xero-tenant-id"
                    })
                    .map(|parameter| {
                        let value = match (parameter.kind.as_str(), parameter.format.as_deref()) {
                            (_, Some("uuid")) => "a03ffcd2-5d91-4c7e-b483-318584e9e439",
                            (_, Some("date")) => "2026-10-02",
                            ("integer", _) => "1",
                            ("boolean", _) => "true",
                            _ if parameter.name == "contentType" => "application/pdf",
                            _ => "sample",
                        };
                        (parameter.name.clone(), value.to_owned())
                    })
                    .collect::<Vec<_>>()
            };
            let result = operation.prepare(&values("path"), &values("query"), &values("header"));
            if WAREHOUSE_EXCLUDED_OPERATION_IDS.contains(&operation.operation_id.as_str()) {
                match result {
                    Err(error) => assert!(error.to_string().contains("out of scope")),
                    Ok(_) => panic!("{} was not rejected", operation.operation_id),
                }
                continue;
            }
            let prepared = result.unwrap_or_else(|error| {
                panic!("{} cannot be prepared: {error}", operation.operation_id)
            });
            assert!(prepared.url.as_str().starts_with(BASE_URL));
            callable += 1;
        }
        assert_eq!(callable, 124);
    }
}
