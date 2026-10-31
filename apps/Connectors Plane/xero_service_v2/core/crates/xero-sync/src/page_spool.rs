//! Private, bounded disk staging for all-or-collection detail fallback.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use xero_client::{raw_pages::MAX_RAW_PAGE_BYTES, PaginationOutcome, RawPage, TerminationReason};

use crate::streaming::PageSource;

const DEFAULT_CAP: usize = 512 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    page: u32,
    http_status: u16,
    record_count: usize,
    fetched_at: DateTime<Utc>,
}

pub(super) struct PageSpool {
    root: tempfile::TempDir,
    cap: usize,
    bytes: usize,
    len: u32,
    next: u32,
    outcome: PaginationOutcome,
}

impl PageSpool {
    pub fn from_env() -> Result<Self, String> {
        let cap = match std::env::var("XERO_DETAIL_SPOOL_MAX_BYTES") {
            Ok(value) => value
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or("XERO_DETAIL_SPOOL_MAX_BYTES must be positive")?,
            Err(std::env::VarError::NotPresent) => DEFAULT_CAP,
            Err(_) => return Err("Invalid detail spool configuration".into()),
        };
        let parent = std::env::var_os("XERO_DETAIL_SPOOL_DIR");
        Self::new(parent.as_deref().map(Path::new), cap)
    }

    pub fn new(parent: Option<&Path>, cap: usize) -> Result<Self, String> {
        let mut builder = tempfile::Builder::new();
        builder.prefix("xero-detail-");
        let root = match parent {
            Some(path) => builder.tempdir_in(path),
            None => builder.tempdir(),
        }
        .map_err(|_| "Cannot create private detail staging directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "Cannot restrict private detail staging directory")?;
        }
        Ok(Self {
            root,
            cap,
            bytes: 0,
            len: 0,
            next: 1,
            outcome: PaginationOutcome {
                pages_fetched: 0,
                termination: TerminationReason::EndedOnEmptyPage,
            },
        })
    }

    pub fn remaining(&self) -> usize {
        self.cap.saturating_sub(self.bytes)
    }

    pub fn set_outcome(&mut self, outcome: PaginationOutcome) {
        self.outcome = outcome;
    }

    pub async fn append(&mut self, page: &RawPage) -> Result<(), String> {
        let header = serde_json::to_vec(&Header {
            page: page.page,
            http_status: page.http_status,
            record_count: page.record_count,
            fetched_at: page.fetched_at,
        })
        .map_err(|_| "Cannot encode detail staging metadata")?;
        let size = header.len().saturating_add(page.body.len());
        if page.body.len() > MAX_RAW_PAGE_BYTES || size > self.remaining() {
            return Err("Detail staging byte limit exceeded; collection fallback remains incomplete until its run succeeds".into());
        }
        let slot = self.len.checked_add(1).ok_or("Too many staged pages")?;
        self.write_private(&format!("{slot}.body"), &page.body)
            .await?;
        self.write_private(&format!("{slot}.meta"), &header).await?;
        self.len = slot;
        self.bytes += size;
        Ok(())
    }

    async fn write_private(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options
            .open(self.root.path().join(name))
            .await
            .map_err(|_| "Cannot write private detail staging file")?;
        file.write_all(bytes)
            .await
            .map_err(|_| "Cannot write private detail staging file")?;
        file.flush()
            .await
            .map_err(|_| "Cannot flush private detail staging file")?;
        Ok(())
    }

    async fn read_bounded(&self, name: &str, limit: usize) -> Result<Vec<u8>, String> {
        let file = tokio::fs::File::open(self.root.path().join(name))
            .await
            .map_err(|_| "Cannot read private detail staging file")?;
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "Cannot read private detail staging file")?;
        if bytes.len() > limit {
            return Err("Invalid oversized detail staging file".into());
        }
        Ok(bytes)
    }
}

#[async_trait::async_trait]
impl PageSource for PageSpool {
    async fn next(&mut self) -> Result<Option<RawPage>, String> {
        if self.next > self.len {
            return Ok(None);
        }
        let header: Header = serde_json::from_slice(
            &self
                .read_bounded(&format!("{}.meta", self.next), 1024)
                .await?,
        )
        .map_err(|_| "Invalid detail staging metadata")?;
        let body = self
            .read_bounded(&format!("{}.body", self.next), MAX_RAW_PAGE_BYTES)
            .await?;
        self.next += 1;
        Ok(Some(RawPage {
            page: header.page,
            body: body.into(),
            http_status: header.http_status,
            record_count: header.record_count,
            fetched_at: header.fetched_at,
        }))
    }

    fn outcome(&self) -> PaginationOutcome {
        self.outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::PageSource;

    fn page() -> RawPage {
        RawPage {
            page: 7,
            body: b"{\"Budgets\":[]}".as_slice().into(),
            http_status: 200,
            record_count: 0,
            fetched_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn spool_preserves_raw_bytes_and_removes_private_files_on_drop() {
        let parent = tempfile::tempdir().unwrap();
        let mut spool = PageSpool::new(Some(parent.path()), 2048).unwrap();
        let root = spool.root.path().to_owned();
        let original = page();
        spool.append(&original).await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(root.join("1.body"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let reread = spool.next().await.unwrap().unwrap();
        assert_eq!(reread.body, original.body);
        assert_eq!(reread.page, 7);
        assert_eq!(reread.fetched_at, original.fetched_at);
        assert!(spool.next().await.unwrap().is_none());
        drop(spool);
        assert!(!root.exists());
    }

    #[tokio::test]
    async fn byte_cap_and_corrupt_metadata_do_not_return_success() {
        let mut spool = PageSpool::new(None, 1).unwrap();
        assert!(spool.append(&page()).await.is_err());
        assert_eq!(spool.len, 0);
        let mut spool = PageSpool::new(None, 2048).unwrap();
        spool.append(&page()).await.unwrap();
        tokio::fs::write(spool.root.path().join("1.meta"), b"not-json")
            .await
            .unwrap();
        assert!(spool.next().await.is_err());
    }

    #[tokio::test]
    async fn cancellation_removes_staged_raw_data() {
        let mut spool = PageSpool::new(None, 2048).unwrap();
        let root = spool.root.path().to_owned();
        let ready = std::sync::Arc::new(tokio::sync::Notify::new());
        let signal = ready.clone();
        let task = tokio::spawn(async move {
            spool.append(&page()).await.unwrap();
            signal.notify_one();
            std::future::pending::<()>().await;
            drop(spool);
        });
        ready.notified().await;
        assert!(root.exists());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!root.exists());
    }
}
