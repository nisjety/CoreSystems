//! Static fetch driver. reqwest + rustls. Browser driver added in Phase 2.

use async_trait::async_trait;
use std::time::Duration;
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;

use crate::driver::Driver;

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub status: u16,
    pub final_url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub duration_ms: u64,
}

pub struct StaticDriver {
    client: reqwest::Client,
}

impl StaticDriver {
    pub fn new(timeout: Duration, user_agent: &str) -> QuarryResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {e}")))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Driver for StaticDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        let start = std::time::Instant::now();
        let resp = self.client.get(url.clone()).send().await.map_err(|e| {
            let code = if e.is_timeout() {
                ErrorCode::Timeout
            } else if e.is_connect() {
                ErrorCode::UpstreamBlocked
            } else {
                ErrorCode::DriverFailed
            };
            QuarryError::new(code, format!("static fetch: {e}"))
        })?;
        let status = resp.status().as_u16();
        let final_url = resp.url().clone();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let body = resp
            .bytes()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("read body: {e}")))?
            .to_vec();
        Ok(FetchResponse {
            status,
            final_url,
            headers,
            body,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}
