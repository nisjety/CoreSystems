use chrono::NaiveDate;
use xero_auth::TokenProvider;
use xero_client::{raw_pages::RawPageReader, PaginationOutcome, RawPage};
use xero_gcs::{metadata, object_key, EntityOutcome, MetaArgs, ObjectMeta, RawSink};

use crate::outcome::{classify_source_failure, sanitized_error};

#[async_trait::async_trait]
pub(super) trait PageSource: Send {
    async fn next(&mut self) -> Result<Option<RawPage>, String>;
    fn outcome(&self) -> PaginationOutcome;
}

pub(super) struct AuthenticatedPages<'a> {
    pub reader: RawPageReader<'a>,
    pub auth: &'a dyn TokenProvider,
    pub tenant: &'a str,
}

#[async_trait::async_trait]
impl PageSource for AuthenticatedPages<'_> {
    async fn next(&mut self) -> Result<Option<RawPage>, String> {
        if self.reader.finished() {
            return Ok(None);
        }
        let token = self
            .auth
            .get_valid_token(self.tenant)
            .await
            .map_err(|e| e.to_string())?;
        self.reader
            .next_page(&token.access_token)
            .await
            .map_err(|e| e.to_string())
    }

    fn outcome(&self) -> PaginationOutcome {
        self.reader.outcome()
    }
}

pub(super) struct WriterArgs<'a> {
    pub prefix: &'a str,
    pub ts: &'a str,
    pub run_date: NaiveDate,
    pub run_id_short: &'a str,
    pub meta: MetaArgs<'a>,
}

pub(super) struct PageWriter<'a> {
    sink: &'a dyn RawSink,
    args: WriterArgs<'a>,
    pages: u32,
    records: i64,
}

impl<'a> PageWriter<'a> {
    pub fn new(sink: &'a dyn RawSink, args: WriterArgs<'a>) -> Self {
        Self {
            sink,
            args,
            pages: 0,
            records: 0,
        }
    }

    pub async fn write(&mut self, page: &RawPage) -> Result<(), String> {
        self.write_with_metadata(page, None).await
    }

    /// Write a raw page while preserving the ordinary collection metadata and
    /// adding request-specific metadata. Detail GETs use this to keep each
    /// direct request's operation and parent identity next to its raw bytes.
    pub async fn write_with_metadata(
        &mut self,
        page: &RawPage,
        additions: Option<&ObjectMeta>,
    ) -> Result<(), String> {
        if page.http_status == 304 {
            return Ok(());
        }
        let key = object_key(
            self.args.prefix,
            self.args.meta.tenant_id,
            self.args.meta.endpoint,
            self.args.run_date,
            self.args.ts,
            self.args.run_id_short,
            page.page,
        );
        let mut meta = metadata(&MetaArgs {
            page: page.page,
            record_count: page.record_count as i64,
            http_status: page.http_status,
            ..self.args.meta.clone()
        });
        if let Some(additions) = additions {
            meta.0.extend(additions.0.clone());
        }
        self.sink
            .put_raw(&key, &page.body, &meta)
            .await
            .map_err(|e| e.to_string())?;
        self.pages += 1;
        self.records += page.record_count as i64;
        Ok(())
    }

    pub fn args(&self) -> &WriterArgs<'a> {
        &self.args
    }

    pub fn scoped<'b>(&'b self, args: WriterArgs<'b>) -> PageWriter<'b> {
        PageWriter::new(self.sink, args)
    }

    pub fn outcome(&self, termination: &str, error: Option<String>) -> EntityOutcome {
        let (outcome_class, http_status, error) = match error.as_deref() {
            Some(error) => {
                let (outcome_class, http_status) =
                    classify_source_failure(self.args.meta.endpoint, error);
                (
                    Some(outcome_class),
                    http_status,
                    Some(sanitized_error(error)),
                )
            }
            None => (None, None, None),
        };
        EntityOutcome {
            tenant: self.args.meta.tenant_id.into(),
            endpoint: self.args.meta.endpoint.into(),
            pages: self.pages,
            records: self.records,
            termination: termination.into(),
            request_key: None,
            error,
            outcome_class,
            http_status,
        }
    }
}

pub(super) async fn write_pages(
    source: &mut dyn PageSource,
    writer: &mut PageWriter<'_>,
) -> EntityOutcome {
    loop {
        match source.next().await {
            Ok(Some(page)) => {
                if let Err(error) = writer.write(&page).await {
                    return writer.outcome("error", Some(error));
                }
            }
            Ok(None) => return writer.outcome(source.outcome().termination.as_str(), None),
            Err(error) => return writer.outcome("error", Some(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use xero_client::{PaginationOutcome, RawPage, TerminationReason};
    use xero_gcs::{ObjectMeta, RawSink};

    struct Source {
        next: u32,
        writes: Arc<AtomicUsize>,
        fail_at: Option<u32>,
    }
    #[async_trait::async_trait]
    impl PageSource for Source {
        async fn next(&mut self) -> Result<Option<RawPage>, String> {
            if self.next > 2 {
                return Ok(None);
            }
            assert_eq!(self.writes.load(Ordering::SeqCst), (self.next - 1) as usize);
            if self.fail_at == Some(self.next) {
                return Err("fixture late fetch failure".into());
            }
            let page = RawPage {
                page: self.next,
                body: format!("{{\"Invoices\":[{{\"page\":{}}}]}}", self.next).into(),
                http_status: 200,
                record_count: 1,
                fetched_at: chrono::Utc::now(),
            };
            self.next += 1;
            Ok(Some(page))
        }
        fn outcome(&self) -> PaginationOutcome {
            PaginationOutcome {
                pages_fetched: self.next - 1,
                termination: TerminationReason::EndedOnEmptyPage,
            }
        }
    }
    struct Sink {
        writes: Arc<AtomicUsize>,
        fail: bool,
    }
    #[async_trait::async_trait]
    impl RawSink for Sink {
        async fn put_raw(&self, _: &str, body: &[u8], meta: &ObjectMeta) -> xero_gcs::Result<()> {
            if self.fail {
                return Err(xero_gcs::GcsError::Config("fixture sink failure".into()));
            }
            assert_eq!(meta.0["x-record-count"], "1");
            assert!(body.starts_with(b"{\"Invoices\""));
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn fixture_args() -> WriterArgs<'static> {
        WriterArgs {
            prefix: "raw/xero",
            ts: "20261003T170000Z",
            run_date: NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
            run_id_short: "abcdef",
            meta: MetaArgs {
                tenant_id: "tenant-fixture",
                endpoint: "invoices",
                run_id: "abcdef00-0000-0000-0000-000000000000",
                ..MetaArgs::default()
            },
        }
    }

    #[tokio::test]
    async fn upload_precedes_next_fetch_and_late_errors_preserve_written_counts() {
        for failure in [None, Some(2)] {
            let writes = Arc::new(AtomicUsize::new(0));
            let sink = Sink {
                writes: writes.clone(),
                fail: false,
            };
            let mut source = Source {
                next: 1,
                writes: writes.clone(),
                fail_at: failure,
            };
            let args = fixture_args();
            let mut writer = PageWriter::new(&sink, args);
            let outcome = write_pages(&mut source, &mut writer).await;
            assert_eq!(outcome.pages, if failure.is_some() { 1 } else { 2 });
            assert_eq!(outcome.records, i64::from(outcome.pages));
            assert_eq!(outcome.error.is_some(), failure.is_some());
            if failure.is_some() {
                assert_eq!(outcome.termination, "error");
            }
        }
    }

    #[tokio::test]
    async fn sink_failure_does_not_fetch_or_count_another_page() {
        let writes = Arc::new(AtomicUsize::new(0));
        let sink = Sink {
            writes: writes.clone(),
            fail: true,
        };
        let mut source = Source {
            next: 1,
            writes,
            fail_at: None,
        };
        let mut writer = PageWriter::new(&sink, fixture_args());
        let outcome = write_pages(&mut source, &mut writer).await;
        assert_eq!(source.next, 2);
        assert_eq!(outcome.pages, 0);
        assert_eq!(outcome.records, 0);
        assert!(outcome.error.is_some());
    }

    struct ExpiredAuth;
    #[async_trait::async_trait]
    impl TokenProvider for ExpiredAuth {
        async fn get_valid_token(&self, _: &str) -> xero_common::Result<xero_auth::TokenData> {
            Err(xero_common::Error::Auth(
                "fixture refresh unavailable".into(),
            ))
        }
    }

    #[tokio::test]
    async fn token_refresh_failure_retains_previously_committed_pages() {
        let writes = Arc::new(AtomicUsize::new(0));
        let sink = Sink {
            writes: writes.clone(),
            fail: false,
        };
        let mut writer = PageWriter::new(&sink, fixture_args());
        writer
            .write(&RawPage {
                page: 1,
                body: br#"{"Invoices":[{}]}"#.as_slice().into(),
                http_status: 200,
                record_count: 1,
                fetched_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
        let api = xero_client::XeroApiClient::new("fixture-auth");
        let mut source = AuthenticatedPages {
            reader: api.raw_page_reader(
                &xero_common::EntityType::Invoices,
                None,
                &xero_client::ExtraQuery::default(),
            ),
            auth: &ExpiredAuth,
            tenant: "fixture-auth",
        };
        let outcome = write_pages(&mut source, &mut writer).await;
        assert_eq!(outcome.pages, 1);
        assert_eq!(outcome.records, 1);
        assert_eq!(outcome.termination, "error");
        assert_eq!(outcome.error.as_deref(), Some("Xero source request failed"));
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(source.outcome().pages_fetched, 0);
    }

    #[tokio::test]
    async fn not_modified_page_is_not_uploaded_or_counted() {
        let writes = Arc::new(AtomicUsize::new(0));
        let sink = Sink {
            writes: writes.clone(),
            fail: true,
        };
        let mut writer = PageWriter::new(&sink, fixture_args());
        writer
            .write(&RawPage {
                page: 1,
                body: Default::default(),
                http_status: 304,
                record_count: 0,
                fetched_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
        assert_eq!(writer.outcome("ended_on_empty_page", None).pages, 0);
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }
}
