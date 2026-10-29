//! BigQuery writer — calls Python `vsv2-bq-stage` / `vsv2-bq-merge` / `vsv2-bq-shadow` subprocesses.
//!
//! The Python tooling owns the BQ client credentials and MERGE logic. Rust writes the records to a
//! spool file (JSONL), forks the subprocess with the file as its input, and reads back the JSON
//! report on stdout (`{"rows": N}`).
//!
//! With `VSV2_BIGQUERY_DATASET_STAGING` set, an endpoint's records go to its raw staging table
//! (`vsv2-bq-stage`). Otherwise they are merged into typed bronze tables (`vsv2-bq-merge`).
//! If `VSV2_BIGQUERY_PROJECT` and a dataset are absent the executor skips the write step
//! (dry-run mode — useful for Phase 3 smoke tests).

use std::path::Path;

use tokio::process::Command;
use tracing::debug;
use vsv2_common::config::AppConfig;
use vsv2_common::{Error, Result};

use crate::spool::Spool;

/// Longest a BigQuery write may take. The biggest endpoint (575,000 general-ledger lines) needs
/// about five minutes; an hour only ever ends a write that is stuck.
const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3600);

/// Thin wrapper around the Python BQ CLI tools.
#[derive(Debug, Clone)]
pub struct BqWriter {
    pub project: String,
    pub dataset_bronze: String,
    pub dataset_shadow: String,
    /// Raw staging dataset; when set, `stage` is how records are written.
    pub dataset_staging: Option<String>,
}

/// What `vsv2-bq-stage` reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageOutcome {
    /// Distinct records delivered.
    pub rows: u64,
    /// Rows the MERGE inserted or updated.
    pub changed: u64,
    /// Rows a complete scan marked deleted.
    pub deleted: u64,
    /// Rows a complete scan would have marked deleted but the guard refused.
    pub delete_skipped: u64,
}

impl StageOutcome {
    fn from_json(v: &serde_json::Value) -> Self {
        let n = |key: &str| v[key].as_u64().unwrap_or(0);
        Self {
            rows: n("rows"),
            changed: n("changed"),
            deleted: n("deleted"),
            delete_skipped: n("delete_skipped"),
        }
    }
}

pub struct MergeLineage<'a> {
    pub run_id: &'a str,
    pub endpoint: &'a str,
    pub source: &'a str,
}

impl BqWriter {
    /// The writer the configuration asks for, or `None` when BigQuery is not configured.
    ///
    /// Needs the project and at least one of the staging and bronze datasets. With only a
    /// staging dataset the bronze one (used by the typed merge, never in staging mode) defaults
    /// to it, so nothing is written to a dataset nobody named.
    pub fn from_config(cfg: &AppConfig) -> Option<Self> {
        Self::from_datasets(
            cfg.bq_project.clone(),
            cfg.bq_dataset_bronze.clone(),
            cfg.bq_dataset_staging.clone(),
            cfg.bq_dataset_shadow.clone(),
        )
    }

    fn from_datasets(
        project: Option<String>,
        bronze: Option<String>,
        staging: Option<String>,
        shadow: Option<String>,
    ) -> Option<Self> {
        let project = project?;
        let dataset_bronze = bronze.or_else(|| staging.clone())?;
        let dataset_shadow = shadow.unwrap_or_else(|| format!("{dataset_bronze}_shadow"));
        Some(Self {
            project,
            dataset_bronze,
            dataset_shadow,
            dataset_staging: staging,
        })
    }

    /// Whether syncs land raw JSON in the staging dataset instead of merging typed tables.
    pub fn stages(&self) -> bool {
        self.dataset_staging.is_some()
    }

    /// Where rejected records go: the staging dataset in staging mode, else bronze.
    fn quarantine_dataset(&self) -> &str {
        self.dataset_staging
            .as_deref()
            .unwrap_or(&self.dataset_bronze)
    }

    /// Write `records` to the raw staging table `table` (the endpoint name) via `vsv2-bq-stage`.
    ///
    /// `pk` is the tenant-scoped key (tenant first). An empty batch still creates the table.
    /// `complete` says the batch is everything Visma has for `tenant` and this endpoint: the
    /// tenant's other live rows are marked deleted (see `staging.py` for the guard).
    pub async fn stage(
        &self,
        table: &str,
        pk: &str,
        records: &[serde_json::Value],
        lineage: MergeLineage<'_>,
        tenant: &str,
        complete: bool,
    ) -> Result<StageOutcome> {
        let mut spool = Spool::new()?;
        for record in records {
            spool.push(record)?;
        }
        self.stage_file(table, pk, spool.path()?, lineage, tenant, complete)
            .await
    }

    /// `stage`, for the records already in the spool file at `path`. A run stages this way, so
    /// its records never have to be held in memory as a list.
    pub async fn stage_file(
        &self,
        table: &str,
        pk: &str,
        path: &Path,
        lineage: MergeLineage<'_>,
        tenant: &str,
        complete: bool,
    ) -> Result<StageOutcome> {
        let dataset = self
            .dataset_staging
            .as_deref()
            .ok_or_else(|| Error::Config("VSV2_BIGQUERY_DATASET_STAGING is not set".to_string()))?;
        debug!(table, complete, "bq_writer: stage");
        let mut args = vec![
            "--project",
            &self.project,
            "--dataset",
            dataset,
            "--table",
            table,
            "--pk",
            pk,
            "--run-id",
            lineage.run_id,
            "--endpoint",
            lineage.endpoint,
            "--source",
            lineage.source,
            "--tenant",
            tenant,
        ];
        if complete {
            args.push("--complete");
        }
        let output = self.run_file("vsv2-bq-stage", &args, path).await?;
        Ok(StageOutcome::from_json(&output))
    }

    /// MERGE `records` into `project.dataset_bronze.<table>` via `vsv2-bq-merge`.
    ///
    /// `normalizer` is the `join_normalizer` name from the endpoint config.
    /// When `Some`, the Python loader reshapes DataMart records to bronze schema
    /// before the MERGE.
    pub async fn merge(
        &self,
        table: &str,
        pk: &str,
        records: &[serde_json::Value],
        normalizer: Option<&str>,
        lineage: MergeLineage<'_>,
    ) -> Result<u64> {
        if records.is_empty() {
            return Ok(0);
        }
        let mut spool = Spool::new()?;
        for record in records {
            spool.push(record)?;
        }
        self.merge_file(table, pk, spool.path()?, normalizer, lineage)
            .await
    }

    /// `merge`, for the records already in the spool file at `path`.
    pub async fn merge_file(
        &self,
        table: &str,
        pk: &str,
        path: &Path,
        normalizer: Option<&str>,
        lineage: MergeLineage<'_>,
    ) -> Result<u64> {
        debug!(table, ?normalizer, "bq_writer: merge");
        let mut args = vec![
            "--project",
            &self.project,
            "--dataset",
            &self.dataset_bronze,
            "--table",
            table,
            "--pk",
            pk,
            "--run-id",
            lineage.run_id,
            "--endpoint",
            lineage.endpoint,
            "--source",
            lineage.source,
        ];
        if let Some(n) = normalizer {
            args.extend_from_slice(&["--normalizer", n]);
        }
        let output = self.run_file("vsv2-bq-merge", &args, path).await?;
        Ok(output["rows"].as_u64().unwrap_or(0))
    }

    /// Append `records` to `project.dataset_shadow.<table>_dm_shadow` via `vsv2-bq-shadow`.
    ///
    /// `normalizer` should match the endpoint's `join_normalizer` so the shadow
    /// table is in bronze shape and `shadow-diff` compares apples to apples.
    pub async fn write_shadow(
        &self,
        table: &str,
        records: &[serde_json::Value],
        normalizer: Option<&str>,
    ) -> Result<u64> {
        debug!(
            table,
            rows = records.len(),
            ?normalizer,
            "bq_writer: write_shadow"
        );
        let mut args = vec![
            "--project",
            &self.project,
            "--dataset-shadow",
            &self.dataset_shadow,
            "--table",
            table,
        ];
        if let Some(n) = normalizer {
            args.extend_from_slice(&["--normalizer", n]);
        }
        self.run("vsv2-bq-shadow", &args, records).await
    }

    /// Append rejected records to `project.dataset_bronze.__vsv2_quarantine`.
    pub async fn write_quarantine(&self, records: &[serde_json::Value]) -> Result<u64> {
        debug!(rows = records.len(), "bq_writer: write_quarantine");
        let args = vec![
            "--project",
            &self.project,
            "--dataset",
            self.quarantine_dataset(),
        ];
        self.run("vsv2-bq-quarantine", &args, records).await
    }

    /// `write_quarantine`, for the rejected records already in the spool file at `path`.
    pub async fn write_quarantine_file(&self, path: &Path) -> Result<u64> {
        let args = vec![
            "--project",
            &self.project,
            "--dataset",
            self.quarantine_dataset(),
        ];
        let output = self.run_file("vsv2-bq-quarantine", &args, path).await?;
        Ok(output["rows"].as_u64().unwrap_or(0))
    }

    /// Run `cmd` on `records`, written to a spool first. Nothing is run for an empty list.
    async fn run(&self, cmd: &str, args: &[&str], records: &[serde_json::Value]) -> Result<u64> {
        if records.is_empty() {
            return Ok(0);
        }
        let mut spool = Spool::new()?;
        for record in records {
            spool.push(record)?;
        }
        let output = self.run_file(cmd, args, spool.path()?).await?;
        Ok(output["rows"].as_u64().unwrap_or(0))
    }

    /// Run `cmd` with `args` and the records in the file at `path` as its `--input`, then read the
    /// JSON report it prints.
    async fn run_file(&self, cmd: &str, args: &[&str], path: &Path) -> Result<serde_json::Value> {
        // A write that never returns would hold the sync and its worker for ever. `kill_on_drop`
        // makes the timeout take the Python process down with it.
        let output = tokio::time::timeout(
            WRITE_TIMEOUT,
            Command::new(cmd)
                .args(args)
                .arg("--input")
                .arg(path)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| {
            Error::Other(format!(
                "{cmd}: no result after {} s, killed",
                WRITE_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| Error::Other(format!("{cmd}: spawn failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Other(format!("{cmd} exited non-zero: {stderr}")));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(stdout.trim())
            .map_err(|e| Error::Other(format!("{cmd}: bad output {e}: {stdout}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(text: &str) -> Option<String> {
        Some(text.to_string())
    }

    #[test]
    fn the_stage_report_is_read_with_missing_numbers_as_zero() {
        let full = serde_json::json!({"rows": 5, "changed": 3, "deleted": 2, "delete_skipped": 7});
        let old = serde_json::json!({"rows": 5});

        assert_eq!(
            StageOutcome::from_json(&full),
            StageOutcome {
                rows: 5,
                changed: 3,
                deleted: 2,
                delete_skipped: 7
            }
        );
        assert_eq!(StageOutcome::from_json(&old).deleted, 0);
    }

    #[test]
    fn nothing_is_written_without_a_project_or_a_dataset() {
        assert!(BqWriter::from_datasets(None, some("b"), some("s"), None).is_none());
        assert!(BqWriter::from_datasets(some("p"), None, None, None).is_none());
    }

    #[test]
    fn a_staging_dataset_alone_is_enough_and_selects_staging_mode() {
        let writer =
            BqWriter::from_datasets(some("p"), None, some("dw_staging_visma"), None).unwrap();

        assert!(writer.stages());
        assert_eq!(writer.quarantine_dataset(), "dw_staging_visma");
        assert_eq!(
            writer.dataset_bronze, "dw_staging_visma",
            "never an unnamed dataset"
        );
    }

    #[test]
    fn a_bronze_dataset_alone_keeps_the_typed_merge() {
        let writer =
            BqWriter::from_datasets(some("p"), some("visma_bronze_v2"), None, None).unwrap();

        assert!(!writer.stages());
        assert_eq!(writer.quarantine_dataset(), "visma_bronze_v2");
        assert_eq!(writer.dataset_shadow, "visma_bronze_v2_shadow");
    }

    #[test]
    fn staging_wins_when_both_are_set_and_quarantine_follows_it() {
        let writer = BqWriter::from_datasets(
            some("p"),
            some("visma_bronze_v2"),
            some("dw_staging_visma"),
            some("shadow"),
        )
        .unwrap();

        assert!(writer.stages());
        assert_eq!(writer.quarantine_dataset(), "dw_staging_visma");
        assert_eq!(writer.dataset_bronze, "visma_bronze_v2");
        assert_eq!(writer.dataset_shadow, "shadow");
    }

    #[tokio::test]
    async fn the_records_file_is_the_commands_input_and_its_report_is_read_back() {
        let mut spool = Spool::new().expect("spool");
        spool.push(&serde_json::json!({"a": 1})).expect("push");
        let path = spool.path().expect("path").to_path_buf();
        let writer = BqWriter::from_datasets(some("p"), some("b"), None, None).unwrap();

        // Stands in for the Python command: it checks that `--input` names the spool file.
        let report = writer
            .run_file(
                "sh",
                &[
                    "-c",
                    r#"test "$1" = --input && test -f "$2" && echo '{"rows": 1}'"#,
                    "sh",
                ],
                &path,
            )
            .await
            .expect("the command runs on the file");

        assert_eq!(report, serde_json::json!({"rows": 1}));
    }
}
