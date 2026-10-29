use crate::{config::AppState, public_url::normalize_public_http_url};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) struct PreviewReceipt {
    pub snippet: Value,
    pub elements: usize,
}
#[derive(Deserialize)]
struct Page {
    final_url: String,
    status: u16,
    run_id: String,
    metadata: Metadata,
    headings: Vec<Heading>,
}
#[derive(Deserialize)]
struct Metadata {
    title: Option<String>,
    description: Option<String>,
}
#[derive(Deserialize)]
struct Heading {
    text: String,
}

pub(super) async fn capture_preview(
    state: &AppState,
    token: &str,
    url: &str,
) -> Result<PreviewReceipt> {
    let mut response = state
        .client
        .post(format!("{}/v1/audit/page", state.quarry_edge_url))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(15))
        .json(&json!({"url":url}))
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 512 * 1024 {
            bail!("preview response exceeds limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    preview_receipt(serde_json::from_slice(&bytes).context("invalid public preview")?)
}

fn preview_receipt(page: Page) -> Result<PreviewReceipt> {
    if !(200..300).contains(&page.status)
        || page.headings.len() > 200
        || page.run_id.is_empty()
        || page.run_id.len() > 128
    {
        bail!("public preview did not capture a page");
    }
    let url = normalize_public_http_url(&page.final_url)
        .map_err(|_| anyhow::anyhow!("invalid preview URL"))?;
    let title = page.metadata.title.filter(|s| !s.trim().is_empty());
    let description = page.metadata.description.filter(|s| !s.trim().is_empty());
    if title.as_ref().is_some_and(|s| s.len() > 4096)
        || description.as_ref().is_some_and(|s| s.len() > 16384)
        || page.headings.iter().any(|h| h.text.len() > 4096)
    {
        bail!("public preview fields exceed limit");
    }
    let excerpt = description.unwrap_or_else(|| {
        page.headings
            .iter()
            .take(4)
            .map(|h| h.text.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    });
    if title.is_none() && excerpt.trim().is_empty() {
        bail!("public preview contains no text evidence");
    }
    Ok(PreviewReceipt {
        elements: page.headings.len(),
        snippet: json!({"id":page.run_id,"kind":"text","title":title.unwrap_or_else(|| url.clone()),
            "excerpt":excerpt,"url":url,"source":"seed","contentType":"text/html","elementCount":page.headings.len()}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    fn page(status: u16) -> Value {
        json!({"final_url":"https://www.triodelab.no/","status":status,"run_id":"run_test","metadata":{"title":"Triodelab","description":null},"headings":[{"text":"Digitale tjenester"}]})
    }
    #[tokio::test]
    async fn preview_uses_only_bounded_quarry_capture_and_verified_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audit/page"))
            .and(header("authorization", "Bearer test-preview-token"))
            .and(body_json(json!({"url":"https://triodelab.no/"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(200)))
            .expect(1)
            .mount(&server)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = server.uri();
        let receipt = capture_preview(&state, "test-preview-token", "https://triodelab.no/")
            .await
            .unwrap();
        assert_eq!(receipt.snippet["url"], "https://www.triodelab.no/");
        assert_eq!(receipt.snippet["excerpt"], "Digitale tjenester");
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
    #[tokio::test]
    async fn upstream_denial_is_not_a_successful_empty_preview() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = server.uri();
        assert!(
            capture_preview(&state, "test-preview-token", "https://triodelab.no/")
                .await
                .is_err()
        );
    }
    #[test]
    fn unsuccessful_missing_or_private_evidence_is_rejected() {
        assert!(preview_receipt(serde_json::from_value(page(404)).unwrap()).is_err());
        let mut empty = page(200);
        empty["metadata"]["title"] = Value::Null;
        empty["headings"] = json!([]);
        assert!(preview_receipt(serde_json::from_value(empty).unwrap()).is_err());
        let mut private = page(200);
        private["final_url"] = json!("http://127.0.0.1/");
        assert!(preview_receipt(serde_json::from_value(private).unwrap()).is_err());
    }
}
