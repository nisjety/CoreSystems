use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderValue, AUTHORIZATION, ETAG, IF_MATCH, LOCATION};
use vsv2_auth::{Scope, Tokens};
use vsv2_common::{Error as CommonError, TenantId};
use vsv2_ratelimit::Limiter;

use crate::error::{InteractiveError, Result};
use crate::request::{WriteMethod, WriteRequest, WriteTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteResponse {
    pub status: u16,
    pub body: String,
    pub etag: Option<String>,
    /// The `Location` header: where a created record can be read (a create answers 201 with it).
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadResponse {
    pub status: u16,
    pub body: String,
    pub etag: Option<String>,
}

/// Sends one prepared write. Mockable; the real one is [`HttpTransport`].
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, tenant: &TenantId, req: &WriteRequest) -> Result<WriteResponse>;

    /// Called when a plan is made: can `req` be sent for `tenant` at all, do the credentials carry the scope it
    /// needs? Refusing here means nobody is asked to approve a change that would fail before it left.
    async fn preflight(&self, _tenant: &TenantId, _req: &WriteRequest) -> Result<()> {
        Ok(())
    }
}

/// Reads the current state of a record, with its ETag, for an operation's precondition.
#[async_trait]
pub trait Reader: Send + Sync {
    async fn get(&self, tenant: &TenantId, target: WriteTarget, path: &str)
        -> Result<ReadResponse>;
}

/// Real HTTP implementation. Holds its **own** token source (an `OAuthClient`, or a signed-in person's
/// tokens), so write tokens never share a cache with Layer 2's read client.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    oauth: Tokens,
    limiter: Limiter,
    finance_base: String,
    salesorder_base: String,
    http: reqwest::Client,
}

impl HttpTransport {
    pub fn new(
        oauth: impl Into<Tokens>,
        limiter: Limiter,
        finance_base: impl Into<String>,
        salesorder_base: impl Into<String>,
    ) -> Self {
        // No redirects: a write that is answered with one has not been done, and following it could replay the
        // body (and the bearer token) somewhere else.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client builds");
        Self {
            oauth: oauth.into(),
            limiter,
            finance_base: finance_base.into().trim_end_matches('/').to_string(),
            salesorder_base: salesorder_base.into().trim_end_matches('/').to_string(),
            http,
        }
    }

    fn base(&self, target: WriteTarget) -> &str {
        match target {
            WriteTarget::Finance => &self.finance_base,
            WriteTarget::SalesOrder => &self.salesorder_base,
        }
    }

    const fn write_scope(target: WriteTarget) -> Scope {
        match target {
            WriteTarget::Finance => Scope::ErpWrite,
            WriteTarget::SalesOrder => Scope::SalesOrderWrite,
        }
    }

    const fn read_scope(target: WriteTarget) -> Scope {
        match target {
            WriteTarget::Finance => Scope::Erp,
            WriteTarget::SalesOrder => Scope::SalesOrder,
        }
    }

    async fn bearer(&self, tenant: &TenantId, scope: Scope) -> Result<HeaderValue> {
        let token = self.oauth.token_for(tenant, scope).await?;
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|e| {
            CommonError::Auth(format!("token is not a valid header value: {e}")).into()
        })
    }

    fn http_err(what: &str, e: reqwest::Error) -> InteractiveError {
        CommonError::Http(format!("{what}: {e}")).into()
    }
}

fn header_string(
    h: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(String::from)
}

impl HttpTransport {
    /// One HTTP attempt with a (cached) write token.
    async fn attempt(
        &self,
        tenant: &TenantId,
        req: &WriteRequest,
        url: &str,
    ) -> Result<WriteResponse> {
        let scope = Self::write_scope(req.target);
        let method = match req.method {
            WriteMethod::Post => reqwest::Method::POST,
            WriteMethod::Put => reqwest::Method::PUT,
            WriteMethod::Patch => reqwest::Method::PATCH,
        };
        // Everything up to `send` can fail without the request having left; those failures are `NotSent`.
        self.limiter
            .acquire()
            .await
            .map_err(|e| InteractiveError::NotSent(e.to_string()))?;
        let bearer = self
            .bearer(tenant, scope)
            .await
            .map_err(|e| InteractiveError::NotSent(e.to_string()))?;
        let mut rb = self
            .http
            .request(method, url)
            .header(AUTHORIZATION, bearer)
            .json(&req.body);
        if let Some(etag) = &req.if_match {
            rb = rb.header(
                IF_MATCH,
                HeaderValue::from_str(etag)
                    .map_err(|e| InteractiveError::NotSent(format!("bad If-Match value: {e}")))?,
            );
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| Self::http_err("write request", e))?;
        let status = resp.status().as_u16();
        let etag = header_string(resp.headers(), ETAG);
        let location = header_string(resp.headers(), LOCATION);
        let body = resp
            .text()
            .await
            .map_err(|e| Self::http_err("read write response", e))?;
        Ok(WriteResponse {
            status,
            body,
            etag,
            location,
        })
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn send(&self, tenant: &TenantId, req: &WriteRequest) -> Result<WriteResponse> {
        req.validate()
            .map_err(|e| InteractiveError::NotSent(e.to_string()))?;
        let scope = Self::write_scope(req.target);
        debug_assert!(scope.is_write());
        let url = format!("{}{}", self.base(req.target), req.path);

        let first = self.attempt(tenant, req, &url).await?;
        // One retry on 401 only: a 401 means the write was not processed, so replaying it is safe.
        if first.status == 401 {
            self.oauth.forget_token(tenant, scope).await;
            return self.attempt(tenant, req, &url).await;
        }
        Ok(first)
    }

    async fn preflight(&self, tenant: &TenantId, req: &WriteRequest) -> Result<()> {
        req.validate()?;
        // A token for the write scope is what the send needs: get it now, so a missing scope or a refused
        // credential is a refusal of the plan and not a failure after somebody approved it.
        self.bearer(tenant, Self::write_scope(req.target))
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl Reader for HttpTransport {
    async fn get(
        &self,
        tenant: &TenantId,
        target: WriteTarget,
        path: &str,
    ) -> Result<ReadResponse> {
        crate::request::validate_path(path)?;
        let scope = Self::read_scope(target);
        let url = format!("{}{}", self.base(target), path);
        self.limiter.acquire().await?;
        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, self.bearer(tenant, scope).await?)
            .send()
            .await
            .map_err(|e| Self::http_err("precondition read", e))?;
        let status = resp.status().as_u16();
        let etag = header_string(resp.headers(), ETAG);
        let body = resp
            .text()
            .await
            .map_err(|e| Self::http_err("read precondition body", e))?;
        Ok(ReadResponse { status, body, etag })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vsv2_auth::OAuthClient;
    use vsv2_common::Source;

    async fn transport(server: &mockito::ServerGuard) -> HttpTransport {
        let oauth = OAuthClient::new(format!("{}/token", server.url()), "cid", "secret")
            .with_scope(Scope::SalesOrderWrite, "so:write")
            .with_scope(Scope::SalesOrder, "so:read");
        HttpTransport::new(
            oauth,
            Limiter::new(Source::ErpRest, 6000, 100_000),
            server.url(),
            server.url(),
        )
    }

    fn req() -> WriteRequest {
        WriteRequest {
            target: WriteTarget::SalesOrder,
            method: WriteMethod::Put,
            path: "/v3/salesorder/SO1".into(),
            body: json!({"qty": 2}),
            if_match: Some("\"e1\"".into()),
        }
    }

    #[tokio::test]
    async fn sends_write_scope_token_if_match_and_body() {
        let mut s = mockito::Server::new_async().await;
        let tok = s
            .mock("POST", "/token")
            .match_body(mockito::Matcher::Regex("scope=so%3Awrite".into()))
            .with_body(r#"{"access_token":"W","expires_in":3600}"#)
            .create_async()
            .await;
        let put = s
            .mock("PUT", "/v3/salesorder/SO1")
            .match_header("authorization", "Bearer W")
            .match_header("if-match", "\"e1\"")
            .match_body(mockito::Matcher::Json(json!({"qty": 2})))
            .with_status(200)
            .with_header("etag", "\"e2\"")
            .with_body("{}")
            .create_async()
            .await;

        let r = transport(&s)
            .await
            .send(&TenantId::new("t"), &req())
            .await
            .unwrap();
        assert_eq!((r.status, r.etag.as_deref()), (200, Some("\"e2\"")));
        tok.assert_async().await;
        put.assert_async().await;
    }

    #[tokio::test]
    async fn a_401_is_retried_once_with_a_fresh_token_and_a_412_is_returned_not_retried() {
        let mut s = mockito::Server::new_async().await;
        let _tok = s
            .mock("POST", "/token")
            .with_body(r#"{"access_token":"W","expires_in":3600}"#)
            .expect(2)
            .create_async()
            .await;
        let unauth = s
            .mock("PUT", "/v3/salesorder/SO1")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;
        let stale = s
            .mock("PUT", "/v3/salesorder/SO1")
            .with_status(412)
            .with_body("stale")
            .expect(1)
            .create_async()
            .await;

        let r = transport(&s)
            .await
            .send(&TenantId::new("t"), &req())
            .await
            .unwrap();
        assert_eq!((r.status, r.body.as_str()), (412, "stale"));
        unauth.assert_async().await;
        stale.assert_async().await;
    }

    #[tokio::test]
    async fn reader_uses_the_read_scope_and_returns_the_etag() {
        let mut s = mockito::Server::new_async().await;
        let _tok = s
            .mock("POST", "/token")
            .match_body(mockito::Matcher::Regex("scope=so%3Aread".into()))
            .with_body(r#"{"access_token":"R","expires_in":3600}"#)
            .create_async()
            .await;
        let _get = s
            .mock("GET", "/v3/salesorder/SO1")
            .match_header("authorization", "Bearer R")
            .with_header("etag", "\"e9\"")
            .with_body("{}")
            .create_async()
            .await;

        let r = transport(&s)
            .await
            .get(
                &TenantId::new("t"),
                WriteTarget::SalesOrder,
                "/v3/salesorder/SO1",
            )
            .await
            .unwrap();
        assert_eq!(r.etag.as_deref(), Some("\"e9\""));
    }
}
