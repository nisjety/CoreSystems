//! Accounting attachment-list discovery. Binary download is a separate flow.

use bytes::{Bytes, BytesMut};
use chrono::{DateTime, Utc};
use reqwest::{StatusCode, Url};
use xero_common::{EntityType, Error, Result};

#[cfg(test)]
use crate::BASE_URL;
use crate::{rate_limit, retry, XeroApiClient};

const MAX_LIST_BYTES: usize = 2 * 1024 * 1024;

pub struct RawAttachmentList {
    pub body: Bytes,
    pub record_count: usize,
    pub fetched_at: DateTime<Utc>,
}

pub fn supports_attachments(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Accounts
            | EntityType::BankTransactions
            | EntityType::BankTransfers
            | EntityType::Contacts
            | EntityType::CreditNotes
            | EntityType::Invoices
            | EntityType::ManualJournals
            | EntityType::PurchaseOrders
            | EntityType::Quotes
            | EntityType::Receipts
            | EntityType::RepeatingInvoices
    )
}

fn attachment_list_url_at(base_url: &str, entity: &EntityType, parent_id: &str) -> Result<Url> {
    if !supports_attachments(entity) {
        return Err(Error::Config(format!(
            "{} has no Accounting attachment list",
            entity
        )));
    }
    let id = uuid::Uuid::parse_str(parent_id)
        .map_err(|_| Error::Config("attachment parent ID must be a UUID".into()))?;
    let mut url = Url::parse(base_url).map_err(|e| Error::Config(e.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| Error::Config("invalid Accounting API base URL".into()))?
        .extend([entity.xero_path(), &id.to_string(), "Attachments"]);
    Ok(url)
}

#[cfg(test)]
fn attachment_list_url(entity: &EntityType, parent_id: &str) -> Result<Url> {
    attachment_list_url_at(BASE_URL, entity, parent_id)
}

fn attachment_count(body: &[u8]) -> Result<usize> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| Error::XeroApi("invalid attachment-list JSON".into()))?;
    let items = value
        .get("Attachments")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::XeroApi("attachment-list response lacks Attachments array".into()))?;
    if items.iter().any(|item| {
        item.get("AttachmentID")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
    }) {
        return Err(Error::XeroApi(
            "attachment-list item lacks a valid AttachmentID".into(),
        ));
    }
    Ok(items.len())
}

impl XeroApiClient {
    /// Fetch one parent's attachment metadata, preserving the original JSON.
    /// Shares the tenant limiter and retry policy with Accounting page fetches.
    pub async fn fetch_attachment_list_raw(
        &self,
        access_token: &str,
        entity: &EntityType,
        parent_id: &str,
    ) -> Result<RawAttachmentList> {
        let url = attachment_list_url_at(&self.base_url, entity, parent_id)?;
        self.fetch_attachment_list_at_url(access_token, &url).await
    }

    async fn fetch_attachment_list_at_url(
        &self,
        access_token: &str,
        url: &Url,
    ) -> Result<RawAttachmentList> {
        let limiter = rate_limit::for_tenant(&self.tenant_id);
        let mut attempt = 0;
        'request: loop {
            let permit = limiter.acquire().await?;
            let mut request = self
                .http
                .get(url.clone())
                .bearer_auth(access_token)
                .header("Accept", "application/json");
            if self.send_tenant_header {
                request = request.header("Xero-tenant-id", &self.tenant_id);
            }
            match request.send().await {
                Ok(mut response) => {
                    limiter.update_from_headers(response.headers()).await?;
                    let status = response.status();
                    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        if status == StatusCode::TOO_MANY_REQUESTS {
                            limiter
                                .defer_for(retry::retry_after_or_default(response.headers()))
                                .await?;
                        }
                        if attempt >= retry::MAX_ATTEMPTS {
                            return Err(Error::XeroApi(format!(
                                "attachment list failed (HTTP {status}) after {} attempts",
                                retry::MAX_ATTEMPTS
                            )));
                        }
                        let wait = if status == StatusCode::TOO_MANY_REQUESTS {
                            None
                        } else {
                            Some(retry::exp_backoff(attempt))
                        };
                        drop(permit);
                        if let Some(wait) = wait {
                            tokio::time::sleep(wait).await;
                        }
                        attempt += 1;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(Error::XeroApi(format!(
                            "attachment list failed (HTTP {status})"
                        )));
                    }
                    let mut body = BytesMut::new();
                    while let Some(chunk) = match response.chunk().await {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            if attempt >= retry::MAX_ATTEMPTS {
                                return Err(Error::XeroApi(format!(
                                    "attachment list body failed after {} attempts: {error}",
                                    retry::MAX_ATTEMPTS
                                )));
                            }
                            let wait = retry::exp_backoff(attempt);
                            drop(permit);
                            tokio::time::sleep(wait).await;
                            attempt += 1;
                            continue 'request;
                        }
                    } {
                        if body.len() + chunk.len() > MAX_LIST_BYTES {
                            return Err(Error::XeroApi("attachment list exceeds 2 MiB".into()));
                        }
                        body.extend_from_slice(&chunk);
                    }
                    let body = body.freeze();
                    let record_count = attachment_count(&body)?;
                    return Ok(RawAttachmentList {
                        body,
                        record_count,
                        fetched_at: Utc::now(),
                    });
                }
                Err(error) => {
                    if retry::is_transient_transport_err(&error) && attempt < retry::MAX_ATTEMPTS {
                        let wait = retry::exp_backoff(attempt);
                        drop(permit);
                        tokio::time::sleep(wait).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::XeroApi(error.to_string()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xero_common::EntityType;

    #[test]
    fn list_url_has_validated_parent_and_id() {
        let id = "5f3c9a2e-0000-0000-0000-000000000000";
        let url = attachment_list_url(&EntityType::Invoices, id).unwrap();
        assert_eq!(
            url.as_str(),
            format!("https://api.xero.com/api.xro/2.0/Invoices/{id}/Attachments")
        );
        assert!(attachment_list_url(&EntityType::Invoices, "../x").is_err());
        assert!(attachment_list_url(&EntityType::ReportProfitAndLoss, id).is_err());
    }

    #[test]
    fn list_body_requires_an_attachments_array() {
        assert_eq!(
            attachment_count(
                br#"{"Attachments":[{"AttachmentID":"5f3c9a2e-0000-0000-0000-000000000000"}]}"#
            )
            .unwrap(),
            1
        );
        assert!(attachment_count(br#"{"Attachments":{}}"#).is_err());
        assert!(attachment_count(br#"{}"#).is_err());
        assert!(attachment_count(br#"{"Attachments":[{"FileName":"a.pdf"}]}"#).is_err());
    }

    #[tokio::test]
    async fn local_http_list_preserves_bytes_and_observes_daily_quota() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body =
            br#"{ "Attachments" : [{"AttachmentID":"5f3c9a2e-0000-0000-0000-000000000000"}] }"#;
        let expected = body.to_vec();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]).to_ascii_lowercase();
            assert!(request.contains("xero-tenant-id: attachment-http-test"));
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nx-daylimit-remaining: 0\r\n\r\n",
                expected.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(&expected).await.unwrap();
        });
        let client = XeroApiClient::new("attachment-http-test");
        let url = Url::parse(&format!("http://{address}/attachment-list")).unwrap();
        let listing = client
            .fetch_attachment_list_at_url("token", &url)
            .await
            .unwrap();
        assert_eq!(listing.body.as_ref(), body);
        assert_eq!(listing.record_count, 1);
        assert!(client
            .fetch_attachment_list_at_url("token", &url)
            .await
            .is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn truncated_response_is_retried() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = br#"{"Attachments":[]}"#;
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                if attempt == 0 {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort")
                        .await
                        .unwrap();
                } else {
                    let header =
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                    stream.write_all(header.as_bytes()).await.unwrap();
                    stream.write_all(body).await.unwrap();
                }
            }
        });
        let client = XeroApiClient::new("attachment-body-retry-test");
        let url = Url::parse(&format!("http://{address}/attachment-list")).unwrap();
        let listing = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.fetch_attachment_list_at_url("token", &url),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(listing.record_count, 0);
        server.await.unwrap();
    }
}
