//! Pull-based raw pagination. A caller must consume a page before fetching more.

use chrono::{DateTime, Utc};
use xero_common::{EntityType, Error, Result};

use crate::{
    extended::{response_info, ExtendedRequest},
    extra_query_pairs, max_journal_number_from_bytes, max_pages_per_entity,
    supports_page_pagination, supports_page_size, ExtraQuery, PaginationOutcome, RawPage,
    TerminationReason, XeroApiClient,
};

pub const MAX_RAW_PAGE_BYTES: usize = 50 * 1024 * 1024;

enum Request<'a> {
    Accounting(&'a EntityType),
    Extended(&'a ExtendedRequest),
}

pub struct RawPageReader<'a> {
    api: &'a XeroApiClient,
    request: Request<'a>,
    url: String,
    modified_after: Option<DateTime<Utc>>,
    extra: Vec<(String, String)>,
    page_size: u32,
    page: u32,
    offset: i64,
    cap: u32,
    done: bool,
    outcome: PaginationOutcome,
}

impl XeroApiClient {
    pub fn raw_page_reader<'a>(
        &'a self,
        entity: &'a EntityType,
        modified_after: Option<DateTime<Utc>>,
        extras: &ExtraQuery,
    ) -> RawPageReader<'a> {
        RawPageReader::new(
            self,
            Request::Accounting(entity),
            modified_after,
            extra_query_pairs(extras, entity),
            extras.page_size.unwrap_or(100),
        )
    }

    pub fn extended_page_reader<'a>(&'a self, request: &'a ExtendedRequest) -> RawPageReader<'a> {
        RawPageReader::new(
            self,
            Request::Extended(request),
            None,
            request.query.clone(),
            100,
        )
    }
}

impl<'a> RawPageReader<'a> {
    fn new(
        api: &'a XeroApiClient,
        request: Request<'a>,
        modified_after: Option<DateTime<Utc>>,
        extra: Vec<(String, String)>,
        page_size: u32,
    ) -> Self {
        let url = match request {
            Request::Accounting(entity) => api.accounting_url(entity),
            Request::Extended(request) => format!("{}{}", request.base_url(), request.path),
        };
        Self {
            api,
            request,
            url,
            modified_after,
            extra,
            page_size,
            page: 1,
            offset: 0,
            cap: max_pages_per_entity(),
            done: false,
            outcome: PaginationOutcome {
                pages_fetched: 0,
                termination: TerminationReason::HitMaxPagesCap,
            },
        }
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    pub fn outcome(&self) -> PaginationOutcome {
        self.outcome
    }

    /// The token is supplied per page so ingestion can refresh between uploads.
    pub async fn next_page(&mut self, access_token: &str) -> Result<Option<RawPage>> {
        if self.done {
            return Ok(None);
        }
        let mut query = self.extra.clone();
        let raw = match self.request {
            Request::Accounting(entity) => {
                if matches!(entity, EntityType::Journals) {
                    query = vec![("offset".into(), self.offset.to_string())];
                } else if supports_page_pagination(entity) {
                    query.push(("page".into(), self.page.to_string()));
                    if supports_page_size(entity) {
                        query.push(("pageSize".into(), self.page_size.to_string()));
                    }
                }
                self.api
                    .fetch_raw_at_url(
                        access_token,
                        entity,
                        &self.url,
                        self.modified_after,
                        &query,
                        self.page,
                    )
                    .await?
            }
            Request::Extended(request) => {
                if request.paginated {
                    query.push(("page".into(), self.page.to_string()));
                    query.push(("pageSize".into(), self.page_size.to_string()));
                }
                self.api
                    .fetch_extended_page(access_token, request, &self.url, &query, self.page)
                    .await?
            }
        };
        self.advance(&raw)?;
        Ok(Some(raw))
    }

    fn advance(&mut self, raw: &RawPage) -> Result<()> {
        self.outcome.pages_fetched += 1;
        let mut termination = None;
        match self.request {
            Request::Accounting(entity) => {
                if raw.record_count == 0
                    || !supports_page_pagination(entity) && !matches!(entity, EntityType::Journals)
                {
                    termination = Some(TerminationReason::EndedOnEmptyPage);
                } else if matches!(entity, EntityType::Journals) {
                    let next =
                        max_journal_number_from_bytes(entity, &raw.body).unwrap_or(self.offset);
                    if next <= self.offset {
                        termination = Some(TerminationReason::OffsetNotAdvancing);
                    }
                    self.offset = next;
                }
            }
            Request::Extended(request) => {
                let (_, terminal) = response_info(request.shape, &raw.body, self.page)?;
                if terminal || !request.paginated {
                    termination = Some(TerminationReason::EndedOnEmptyPage);
                }
            }
        }
        if termination.is_none() && self.page >= self.cap {
            termination = Some(TerminationReason::HitMaxPagesCap);
        }
        if let Some(reason) = termination {
            self.outcome.termination = reason;
            self.done = true;
        } else {
            self.page += 1;
        }
        Ok(())
    }
}

pub(crate) async fn read_raw_body(response: reqwest::Response) -> Result<bytes::Bytes> {
    read_body_with_limit(response, MAX_RAW_PAGE_BYTES).await
}

async fn read_body_with_limit(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<bytes::Bytes> {
    let oversized =
        || Error::XeroApi("Xero raw response exceeds the configured page byte limit".into());
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(oversized());
    }
    let mut body = bytes::BytesMut::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Error::XeroApi("Xero raw body read failed".into()))?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(oversized());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExtraQuery, RawPage, XeroApiClient};
    use chrono::Utc;
    use xero_common::EntityType;

    fn page(number: u32, records: usize, body: &'static [u8]) -> RawPage {
        RawPage {
            page: number,
            body: body.into(),
            http_status: 200,
            record_count: records,
            fetched_at: Utc::now(),
        }
    }

    #[test]
    fn accounting_cursor_keeps_empty_page_and_real_request_count() {
        let api = XeroApiClient::new("cursor-fixture");
        let mut reader = api.raw_page_reader(&EntityType::Invoices, None, &ExtraQuery::default());
        reader.advance(&page(1, 1, b"{}")).unwrap();
        assert!(!reader.finished());
        assert_eq!(reader.page, 2);
        reader.advance(&page(2, 0, b"{}")).unwrap();
        assert!(reader.finished());
        assert_eq!(reader.outcome().pages_fetched, 2);
        assert_eq!(
            reader.outcome().termination,
            crate::TerminationReason::EndedOnEmptyPage
        );
    }

    #[test]
    fn journal_cursor_preserves_offset_and_detects_non_advancement() {
        let api = XeroApiClient::new("journal-cursor-fixture");
        let mut reader = api.raw_page_reader(&EntityType::Journals, None, &ExtraQuery::default());
        reader
            .advance(&page(1, 1, br#"{"Journals":[{"JournalNumber":7}]}"#))
            .unwrap();
        assert_eq!(reader.offset, 7);
        assert!(!reader.finished());
        reader
            .advance(&page(2, 1, br#"{"Journals":[{"JournalNumber":7}]}"#))
            .unwrap();
        assert!(reader.finished());
        assert_eq!(
            reader.outcome().termination,
            crate::TerminationReason::OffsetNotAdvancing
        );
    }

    #[test]
    fn cap_and_single_response_do_not_request_another_page() {
        let api = XeroApiClient::new("cap-cursor-fixture");
        let mut reader = api.raw_page_reader(&EntityType::Invoices, None, &ExtraQuery::default());
        reader.cap = 1;
        reader.advance(&page(1, 1, b"{}")).unwrap();
        assert!(reader.finished());
        assert_eq!(
            reader.outcome().termination,
            crate::TerminationReason::HitMaxPagesCap
        );
        let mut reader = api.raw_page_reader(&EntityType::Accounts, None, &ExtraQuery::default());
        reader.advance(&page(1, 2, b"{}")).unwrap();
        assert!(reader.finished());
        assert_eq!(reader.outcome().pages_fetched, 1);
    }

    #[tokio::test]
    async fn extended_cursor_follows_page_count_and_completed_read_is_noop() {
        use crate::extended::{ExtendedFamily, ExtendedRequest, ResponseShape};
        let api = XeroApiClient::new("extended-cursor-fixture");
        let request = ExtendedRequest::new(
            ExtendedFamily::Projects,
            "projects_projects",
            "/projects",
            ResponseShape::Items,
            true,
        )
        .unwrap();
        let mut reader = api.extended_page_reader(&request);
        reader
            .advance(&page(
                1,
                1,
                br#"{"items":[{}],"pagination":{"pageCount":2}}"#,
            ))
            .unwrap();
        assert!(!reader.finished());
        reader
            .advance(&page(
                2,
                1,
                br#"{"items":[{}],"pagination":{"pageCount":2}}"#,
            ))
            .unwrap();
        assert!(reader.finished());
        assert!(reader
            .next_page("must-not-be-used")
            .await
            .unwrap()
            .is_none());
        assert_eq!(reader.outcome().pages_fetched, 2);
    }

    #[tokio::test]
    async fn declared_and_chunked_raw_bodies_are_bounded_without_changing_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (response, limit, expected) in [
            (b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n01234567890123456789".as_slice(), 10, None),
            (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n14\r\n01234567890123456789\r\n0\r\n\r\n".as_slice(), 10, None),
            (b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nraw!".as_slice(), 10, Some(b"raw!".as_slice())),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/raw", listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0u8;1024];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                stream.write_all(response).await.unwrap();
            });
            let response = reqwest::Client::new().get(url).send().await.unwrap();
            let result = read_body_with_limit(response, limit).await;
            if let Some(bytes) = expected { assert_eq!(result.unwrap().as_ref(),bytes); }
            else { assert!(result.unwrap_err().to_string().contains("byte limit")); }
            task.await.unwrap();
        }
    }

    async fn fixture_server(
        responses: Vec<&'static [u8]>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/pages", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for body in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0u8; 4096];
                let len = stream.read(&mut request).await.unwrap();
                requests.push(String::from_utf8_lossy(&request[..len]).into_owned());
                let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                stream.write_all(header.as_bytes()).await.unwrap();
                stream.write_all(body).await.unwrap();
            }
            requests
        });
        (url, task)
    }

    #[tokio::test]
    async fn pull_reader_real_http_preserves_pagination_and_rotated_tokens() {
        let (url, task) = fixture_server(vec![
            br#"{"Invoices":[{"InvoiceID":"one"}]}"#,
            br#"{"Invoices":[]}"#,
        ])
        .await;
        let api = XeroApiClient::new("raw-pull-http-fixture");
        let mut reader = api.raw_page_reader(
            &EntityType::Invoices,
            None,
            &ExtraQuery {
                page_size: Some(2),
                ..ExtraQuery::default()
            },
        );
        reader.url = url;
        assert_eq!(
            reader
                .next_page("first-fixture-token")
                .await
                .unwrap()
                .unwrap()
                .record_count,
            1
        );
        assert_eq!(
            reader
                .next_page("rotated-fixture-token")
                .await
                .unwrap()
                .unwrap()
                .record_count,
            0
        );
        assert!(reader.next_page("unused").await.unwrap().is_none());
        let requests = task.await.unwrap();
        assert!(requests[0].contains("page=1") && requests[0].contains("pageSize=2"));
        assert!(requests[1].contains("page=2"));
        assert!(requests[0].contains("Bearer first-fixture-token"));
        assert!(requests[1].contains("Bearer rotated-fixture-token"));
        assert_eq!(reader.outcome().pages_fetched, 2);
    }

    #[tokio::test]
    async fn pull_reader_handles_journal_offsets_and_extended_page_counts_over_http() {
        let (url, task) = fixture_server(vec![
            br#"{"Journals":[{"JournalNumber":7}]}"#,
            br#"{"Journals":[]}"#,
        ])
        .await;
        let api = XeroApiClient::new("journal-pull-http-fixture");
        let mut reader = api.raw_page_reader(&EntityType::Journals, None, &ExtraQuery::default());
        reader.url = url;
        while reader.next_page("fixture").await.unwrap().is_some() {}
        let requests = task.await.unwrap();
        assert!(requests[0].contains("offset=0"));
        assert!(requests[1].contains("offset=7"));
        use crate::extended::{ExtendedFamily, ExtendedRequest, ResponseShape};
        let request = ExtendedRequest::new(
            ExtendedFamily::Projects,
            "projects_projects",
            "/projects",
            ResponseShape::Items,
            true,
        )
        .unwrap();
        let (url, task) = fixture_server(vec![
            br#"{"items":[{}],"pagination":{"pageCount":2}}"#,
            br#"{"items":[{}],"pagination":{"pageCount":2}}"#,
        ])
        .await;
        let mut reader = api.extended_page_reader(&request);
        reader.url = url;
        while reader.next_page("fixture").await.unwrap().is_some() {}
        assert_eq!(reader.outcome().pages_fetched, 2);
        assert!(task.await.unwrap()[1].contains("page=2"));
    }
}
