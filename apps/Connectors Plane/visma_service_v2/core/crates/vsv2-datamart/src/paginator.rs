use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::debug;

use crate::client::DatamartClient;
use vsv2_common::{Error, PageFetchMetric, PageSink, Result, TenantId};

/// Cursor pagination state for DataMart queries.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CursorPaginator {
    pub page_size: u32,
    pub last_id: Option<String>,
    pub updated_since: Option<DateTime<Utc>>,
    pub max_pages: Option<u32>,
}

impl CursorPaginator {
    pub fn new(page_size: u32) -> Self {
        Self {
            page_size,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaginatedRecords {
    pub records: Vec<Value>,
    pub page_metrics: Vec<PageFetchMetric>,
}

/// Pull every page until the response has fewer items than `page_size`.
///
/// `query` MUST accept `pageSize` and `lastId` variables and should accept an
/// `updatedInDataMart` or legacy `updatedSince` variable when incremental sync
/// is required. Some schemas may also accept `companyId`.
/// Returns a single root field `[<entity>]` whose elements have an `id` field
/// used as the next `lastId` cursor.
pub async fn paginate_all(
    client: &DatamartClient,
    tenant: &TenantId,
    company_id: Option<&str>,
    query: &str,
    root_field: &str,
    paginator: &mut CursorPaginator,
) -> Result<Vec<Value>> {
    Ok(
        paginate_all_with_metrics(client, tenant, company_id, query, root_field, paginator)
            .await?
            .records,
    )
}

/// What a cursor walk leaves behind once its rows have gone to the sink.
#[derive(Debug, Clone, Default)]
pub struct CursorWalk {
    pub page_metrics: Vec<PageFetchMetric>,
    /// Rows handed to the sink.
    pub records: usize,
}

pub async fn paginate_all_with_metrics(
    client: &DatamartClient,
    tenant: &TenantId,
    company_id: Option<&str>,
    query: &str,
    root_field: &str,
    paginator: &mut CursorPaginator,
) -> Result<PaginatedRecords> {
    let mut records: Vec<Value> = Vec::new();
    let walk = paginate_all_stream(
        client,
        tenant,
        company_id,
        query,
        root_field,
        paginator,
        &mut records,
    )
    .await?;
    Ok(PaginatedRecords {
        records,
        page_metrics: walk.page_metrics,
    })
}

/// Walk a DataMart query page by page, handing each page to `sink` as it arrives. The cursor in
/// `paginator` moves on with every page, so the walk can be resumed from where it stopped.
pub async fn paginate_all_stream(
    client: &DatamartClient,
    tenant: &TenantId,
    company_id: Option<&str>,
    query: &str,
    root_field: &str,
    paginator: &mut CursorPaginator,
    sink: &mut dyn PageSink,
) -> Result<CursorWalk> {
    let mut page_metrics = Vec::new();
    let mut pages: u32 = 0;
    let mut records: usize = 0;

    loop {
        let mut vars = json!({
            "pageSize":           paginator.page_size,
            "lastId":             paginator.last_id,
            "updatedSince":       paginator.updated_since,
            "updatedInDataMart":  paginator.updated_since,
        });
        if let Value::Object(ref mut m) = vars {
            // GraphQL DateTime nullable — leave as null when None.
            if paginator.updated_since.is_none() {
                m.insert("updatedSince".into(), Value::Null);
                m.insert("updatedInDataMart".into(), Value::Null);
            }
            if let Some(cid) = company_id {
                m.insert("companyId".into(), Value::String(cid.to_string()));
            }
        }

        let resp = client.execute_with_meta(tenant, query, vars).await?;

        let items = resp
            .body
            .get(root_field)
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::GraphQl(format!("expected array at data.{root_field}")))?
            .clone();

        debug!(page = pages + 1, n = items.len(), "datamart: page");
        page_metrics.push(PageFetchMetric {
            upstream_path: root_field.to_string(),
            page_param: Some("lastId".into()),
            page_value: None,
            page_size: Some(paginator.page_size),
            status: resp.status,
            elapsed_ms: resp.elapsed_ms,
            records: items.len() as i64,
            total_count: None,
            has_next: Some((items.len() as u32) >= paginator.page_size),
            rate_limit: resp.rate_limit,
            background_mode: None,
        });

        if items.is_empty() {
            break;
        }

        // Advance cursor before yielding records — last `id` field wins.
        if let Some(last) = items.last() {
            if let Some(id) = last.get("id").and_then(|v| v.as_str()) {
                paginator.last_id = Some(id.to_string());
            }
        }

        let n = items.len();
        records += n;
        sink.accept(items)?;
        pages += 1;

        if let Some(cap) = paginator.max_pages {
            if pages >= cap {
                break;
            }
        }
        if (n as u32) < paginator.page_size {
            break;
        }
    }

    Ok(CursorWalk {
        page_metrics,
        records,
    })
}

#[cfg(test)]
mod tests {
    use mockito::Matcher;
    use vsv2_auth::{OAuthClient, Scope};
    use vsv2_common::{PageSink, Source};
    use vsv2_ratelimit::Limiter;

    use super::*;

    /// Keeps each page as it arrives.
    #[derive(Default)]
    struct Probe {
        pages: Vec<Vec<Value>>,
    }

    impl PageSink for Probe {
        fn accept(&mut self, page: Vec<Value>) -> Result<()> {
            self.pages.push(page);
            Ok(())
        }
    }

    fn client(server: &mockito::ServerGuard) -> DatamartClient {
        let oauth = OAuthClient::new(format!("{}/token", server.url()), "cid", "secret")
            .with_scope(Scope::Datamart, "datamart:read");
        DatamartClient::new(
            format!("{}/graphql", server.url()),
            oauth,
            Limiter::new(Source::DatamartGraphql, 6_000, 100_000),
        )
    }

    #[tokio::test]
    async fn each_page_reaches_the_sink_and_the_cursor_moves_on_to_the_next() {
        let mut server = mockito::Server::new_async().await;
        let _token = server
            .mock("POST", "/token")
            .with_body(r#"{"access_token":"t","expires_in":3600}"#)
            .create_async()
            .await;
        let first_page = server
            .mock("POST", "/graphql")
            .match_body(Matcher::Regex(r#""lastId":null"#.into()))
            .with_body(r#"{"data":{"items":[{"id":"a"},{"id":"b"}]}}"#)
            .expect(1)
            .create_async()
            .await;
        let second_page = server
            .mock("POST", "/graphql")
            .match_body(Matcher::Regex(r#""lastId":"b""#.into()))
            .with_body(r#"{"data":{"items":[{"id":"c"}]}}"#)
            .expect(1)
            .create_async()
            .await;
        let mut paginator = CursorPaginator::new(2);
        let mut probe = Probe::default();

        let walk = paginate_all_stream(
            &client(&server),
            &TenantId::new("t1"),
            None,
            "query { items }",
            "items",
            &mut paginator,
            &mut probe,
        )
        .await
        .expect("walk");

        assert_eq!(
            probe.pages.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![2, 1],
            "the short last page ends the walk"
        );
        assert_eq!(walk.records, 3);
        assert_eq!(walk.page_metrics.len(), 2);
        first_page.assert_async().await;
        second_page.assert_async().await;
    }
}
