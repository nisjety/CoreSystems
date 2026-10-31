//! BigQuery backend that shells out to the `bq` CLI (uses whatever gcloud/ADC identity is
//! configured). Arguments are passed as argv, never through a shell.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use super::{DryRunInfo, RawRows, SchemaField, TableRef, Warehouse, WarehouseError};

const CALL_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct BqCli {
    project: String,
    bin: String,
}

impl BqCli {
    pub fn new(project: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            bin: "bq".into(),
        }
    }

    pub fn with_binary(mut self, bin: impl Into<String>) -> Self {
        self.bin = bin.into();
        self
    }

    async fn exec(&self, args: &[String]) -> Result<String, WarehouseError> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(args).kill_on_drop(true);
        let out = tokio::time::timeout(CALL_TIMEOUT, cmd.output())
            .await
            .map_err(|_| {
                WarehouseError::Backend(format!("bq timed out after {}s", CALL_TIMEOUT.as_secs()))
            })?
            .map_err(|e| WarehouseError::Backend(format!("could not run `{}`: {e}", self.bin)))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            // bq reports query errors on stdout as often as on stderr.
            let stderr = String::from_utf8_lossy(&out.stderr);
            let msg = format!("{} {}", stdout.trim(), stderr.trim());
            return Err(WarehouseError::Backend(vsv2_common::text::printable(
                msg.trim(),
                800,
            )));
        }
        Ok(stdout)
    }

    fn base(&self, sub: &str) -> Vec<String> {
        vec![
            sub.into(),
            "--nouse_legacy_sql".into(),
            format!("--project_id={}", self.project),
        ]
    }
}

fn parse_schema(fields: &Value) -> Vec<SchemaField> {
    fields
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| SchemaField {
                    name: f["name"].as_str().unwrap_or_default().to_string(),
                    ty: f["type"].as_str().unwrap_or("STRING").to_ascii_uppercase(),
                    repeated: f["mode"].as_str() == Some("REPEATED"),
                    fields: parse_schema(&f["fields"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_dry_run(stdout: &str) -> Result<DryRunInfo, WarehouseError> {
    let v: Value = serde_json::from_str(stdout)
        .map_err(|e| WarehouseError::Backend(format!("unreadable dry-run output: {e}")))?;
    let q = &v["statistics"]["query"];
    let bytes = q["totalBytesProcessed"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| q["totalBytesProcessed"].as_u64())
        .unwrap_or(0);
    let referenced = q["referencedTables"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| TableRef {
                    project: t["projectId"].as_str().unwrap_or_default().into(),
                    dataset: t["datasetId"].as_str().unwrap_or_default().into(),
                    table: t["tableId"].as_str().unwrap_or_default().into(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(DryRunInfo {
        statement_type: q["statementType"].as_str().unwrap_or("UNKNOWN").to_string(),
        total_bytes: bytes,
        referenced,
        schema: parse_schema(&q["schema"]["fields"]),
    })
}

#[async_trait]
impl Warehouse for BqCli {
    fn project(&self) -> &str {
        &self.project
    }

    async fn dry_run(&self, sql: &str) -> Result<DryRunInfo, WarehouseError> {
        let mut args = self.base("query");
        args.extend([
            "--dry_run".into(),
            "--format=prettyjson".into(),
            "--".into(),
            sql.into(),
        ]);
        parse_dry_run(&self.exec(&args).await?)
    }

    async fn run(
        &self,
        sql: &str,
        max_rows: usize,
        max_bytes: u64,
    ) -> Result<RawRows, WarehouseError> {
        let mut args = self.base("query");
        args.extend([
            "--format=json".into(),
            "--quiet".into(),
            format!("--max_rows={max_rows}"),
            format!("--maximum_bytes_billed={max_bytes}"),
            "--".into(),
            sql.into(),
        ]);
        let stdout = self.exec(&args).await?;
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            return Ok(RawRows { rows: vec![] });
        }
        let v: Value = serde_json::from_str(trimmed)
            .map_err(|e| WarehouseError::Backend(format!("unreadable query output: {e}")))?;
        Ok(RawRows {
            rows: v.as_array().cloned().unwrap_or_default(),
        })
    }

    async fn list_datasets(&self) -> Result<Vec<String>, WarehouseError> {
        let args = vec![
            "ls".into(),
            format!("--project_id={}", self.project),
            "--max_results=1000".into(),
            "--format=json".into(),
        ];
        let stdout = self.exec(&args).await?;
        let v: Value = serde_json::from_str(stdout.trim())
            .map_err(|e| WarehouseError::Backend(format!("unreadable dataset list: {e}")))?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|d| {
                        d["datasetReference"]["datasetId"]
                            .as_str()
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_shaped_dry_run_document() {
        let doc = r#"{"statistics":{"query":{"statementType":"SELECT","totalBytesProcessed":"10485760",
            "referencedTables":[{"projectId":"p","datasetId":"d","tableId":"t"}],
            "schema":{"fields":[{"name":"i","type":"INTEGER","mode":"NULLABLE"},
                {"name":"arr","type":"INTEGER","mode":"REPEATED"},
                {"name":"st","type":"RECORD","fields":[{"name":"k","type":"INTEGER"}]}]}}}}"#;
        let i = parse_dry_run(doc).unwrap();
        assert_eq!(
            (i.statement_type.as_str(), i.total_bytes),
            ("SELECT", 10_485_760)
        );
        assert_eq!(
            i.referenced[0],
            TableRef {
                project: "p".into(),
                dataset: "d".into(),
                table: "t".into()
            }
        );
        assert!(i.schema[1].repeated);
        assert_eq!(i.schema[2].fields[0].name, "k");
    }

    #[test]
    fn missing_statistics_become_unknown_not_select() {
        let i = parse_dry_run("{}").unwrap();
        assert_eq!(i.statement_type, "UNKNOWN"); // SafeWarehouse refuses anything but SELECT
        assert!(parse_dry_run("not json").is_err());
    }
}
