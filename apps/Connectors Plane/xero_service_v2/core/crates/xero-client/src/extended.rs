//! Read-only raw fetches for Xero API families outside Accounting.

use chrono::Utc;
use serde_json::Value;
use xero_common::{Error, Result};

use crate::{rate_limit, retry, PaginationOutcome, RawPage, XeroApiClient};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtendedFamily {
    Assets,
    BankFeeds,
    Projects,
    Identity,
    Finance,
}

impl ExtendedFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Assets => "assets",
            Self::BankFeeds => "bankfeeds",
            Self::Projects => "projects",
            Self::Identity => "identity",
            Self::Finance => "finance",
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            Self::Assets => "https://api.xero.com/assets.xro/1.0",
            Self::BankFeeds => "https://api.xero.com/bankfeeds.xro/1.0",
            Self::Projects => "https://api.xero.com/projects.xro/2.0",
            Self::Identity => "https://api.xero.com",
            Self::Finance => "https://api.xero.com/finance.xro/1.0",
        }
    }

    pub fn api_version(self) -> &'static str {
        match self {
            Self::Projects => "2.0",
            Self::Identity => "identity",
            _ => "1.0",
        }
    }

    pub fn read_scope(self) -> Option<&'static str> {
        match self {
            Self::Assets => Some("assets.read"),
            Self::BankFeeds => Some("bankfeeds"),
            Self::Projects => Some("projects.read"),
            Self::Identity | Self::Finance => None,
        }
    }
}

impl std::str::FromStr for ExtendedFamily {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "assets" => Ok(Self::Assets),
            "bankfeeds" => Ok(Self::BankFeeds),
            "projects" => Ok(Self::Projects),
            "identity" => Ok(Self::Identity),
            "finance" => Ok(Self::Finance),
            _ => Err(Error::Config(format!("unknown Xero API family: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseShape {
    Items,
    Array,
    Object,
}

#[derive(Debug, Clone)]
pub struct ExtendedRequest {
    pub family: ExtendedFamily,
    pub endpoint: &'static str,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub shape: ResponseShape,
    pub paginated: bool,
    pub request_key: Option<String>,
    #[cfg(any(test, feature = "test-support"))]
    test_base_url: Option<String>,
}

impl ExtendedRequest {
    pub fn new(
        family: ExtendedFamily,
        endpoint: &'static str,
        path: &str,
        shape: ResponseShape,
        paginated: bool,
    ) -> Result<Self> {
        // Routes are from the static catalog; reject malformed paths before
        // passing them to reqwest so a caller cannot alter the Xero host.
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains("..")
            || path.contains(['?', '#', '\\'])
        {
            return Err(Error::Config(format!("invalid extended API path: {path}")));
        }
        Ok(Self {
            family,
            endpoint,
            path: path.to_owned(),
            query: Vec::new(),
            shape,
            paginated,
            request_key: None,
            #[cfg(any(test, feature = "test-support"))]
            test_base_url: None,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn with_test_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.test_base_url = Some(base_url.into().trim_end_matches('/').to_owned());
        self
    }

    pub(crate) fn base_url(&self) -> &str {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(base_url) = self.test_base_url.as_deref() {
            return base_url;
        }
        self.family.base_url()
    }
}

/// Return (record count, whether this page is terminal). Xero's extended APIs
/// use `items` plus a pageCount, unlike Accounting's endpoint-keyed wrappers.
pub(crate) fn response_info(shape: ResponseShape, body: &[u8], page: u32) -> Result<(usize, bool)> {
    let data: Value = serde_json::from_slice(body)
        .map_err(|e| Error::XeroApi(format!("invalid extended API JSON: {e}")))?;
    match shape {
        ResponseShape::Items => {
            let items = data
                .get("items")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::XeroApi("extended API response has no items array".into()))?;
            let page_count = data
                .get("pagination")
                .and_then(|p| p.get("pageCount"))
                .and_then(Value::as_u64);
            Ok((
                items.len(),
                items.is_empty() || page_count.is_some_and(|n| u64::from(page) >= n),
            ))
        }
        ResponseShape::Array => data
            .as_array()
            .map(|a| (a.len(), true))
            .ok_or_else(|| Error::XeroApi("extended API response is not an array".into())),
        ResponseShape::Object => data
            .as_object()
            .map(|_| (1, true))
            .ok_or_else(|| Error::XeroApi("extended API response is not an object".into())),
    }
}

impl XeroApiClient {
    /// Fetch a catalogued extended resource as verbatim pages. A malformed
    /// response fails the entity instead of being mistaken for an empty page.
    pub async fn fetch_extended_raw(
        &self,
        access_token: &str,
        request: &ExtendedRequest,
    ) -> Result<(Vec<RawPage>, PaginationOutcome)> {
        let mut reader = self.extended_page_reader(request);
        let mut pages = Vec::new();
        while let Some(raw) = reader.next_page(access_token).await? {
            pages.push(raw);
        }
        Ok((pages, reader.outcome()))
    }

    pub(crate) async fn fetch_extended_page(
        &self,
        access_token: &str,
        request: &ExtendedRequest,
        url: &str,
        query: &[(String, String)],
        page: u32,
    ) -> Result<RawPage> {
        let limiter = rate_limit::for_tenant(&self.tenant_id);
        let mut attempt = 0;
        loop {
            let permit = limiter.acquire().await?;
            let mut req = self
                .http
                .get(url)
                .bearer_auth(access_token)
                .header("Accept", "application/json")
                .query(query);
            if self.send_tenant_header && request.family != ExtendedFamily::Identity {
                req = req.header("Xero-tenant-id", &self.tenant_id);
            }
            match req.send().await {
                Ok(resp) => {
                    limiter.update_from_headers(resp.headers()).await?;
                    let status = resp.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let wait = retry::retry_after_or_default(resp.headers());
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
                    if !status.is_success() {
                        return Err(Error::XeroApi(format!(
                            "{} {} {}",
                            status.as_u16(),
                            request.endpoint,
                            request.path,
                        )));
                    }
                    let body = crate::raw_pages::read_raw_body(resp).await?;
                    let (record_count, _) = response_info(request.shape, &body, page)?;
                    return Ok(RawPage {
                        page,
                        body,
                        http_status: status.as_u16(),
                        record_count,
                        fetched_at: Utc::now(),
                    });
                }
                Err(e)
                    if retry::is_transient_transport_err(&e) && attempt < retry::MAX_ATTEMPTS =>
                {
                    let wait = retry::exp_backoff(attempt);
                    drop(permit);
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                Err(e) => return Err(Error::XeroApi(e.to_string())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_have_documented_hosts_and_scopes() {
        assert_eq!(
            ExtendedFamily::Assets.base_url(),
            "https://api.xero.com/assets.xro/1.0"
        );
        assert_eq!(ExtendedFamily::Projects.read_scope(), Some("projects.read"));
        assert_eq!(ExtendedFamily::BankFeeds.read_scope(), Some("bankfeeds"));
        assert_eq!(ExtendedFamily::Identity.read_scope(), None);
        assert_eq!(
            ExtendedFamily::Finance.base_url(),
            "https://api.xero.com/finance.xro/1.0"
        );
    }

    #[test]
    fn items_pagination_uses_page_count_not_short_page() {
        let body = br#"{"pagination":{"page":1,"pageCount":2,"pageSize":50,"itemCount":1},"items":[{"projectId":"p1"}]}"#;
        assert_eq!(
            response_info(ResponseShape::Items, body, 1).unwrap(),
            (1, false)
        );
        assert_eq!(
            response_info(ResponseShape::Items, body, 2).unwrap(),
            (1, true)
        );
        assert!(response_info(ResponseShape::Items, br#"{"error":"bad"}"#, 1).is_err());
    }

    #[test]
    fn root_responses_are_validated() {
        assert_eq!(
            response_info(ResponseShape::Array, br#"[{"id":"1"}]"#, 1).unwrap(),
            (1, true)
        );
        assert_eq!(
            response_info(ResponseShape::Object, br#"{"assetId":"1"}"#, 1).unwrap(),
            (1, true)
        );
        assert!(response_info(ResponseShape::Object, b"[]", 1).is_err());
    }

    #[test]
    fn request_paths_cannot_escape_the_documented_host() {
        let ok = ExtendedRequest::new(
            ExtendedFamily::Assets,
            "assets_assets",
            "/Assets",
            ResponseShape::Items,
            true,
        );
        assert!(ok.is_ok());
        let bad = ExtendedRequest::new(
            ExtendedFamily::Assets,
            "assets_assets",
            "//evil.example/x",
            ResponseShape::Items,
            true,
        );
        assert!(bad.is_err());
    }
}
