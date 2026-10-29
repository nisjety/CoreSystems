//! Tests of the real ports that need no database: the HTTP side against a local server and
//! the key tool against a script that stands in for it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use vsv2_auth::{OAuthClient, Scope};
use vsv2_common::PrimaryKey;
use vsv2_erp_rest::ErpRestClient;
use vsv2_ratelimit::Limiter;

use super::super::fixtures::def;
use super::super::OpenDoc;
use super::*;

#[test]
fn one_document_is_an_object_or_a_list_of_one_object() {
    let doc = json!({"referenceNumber": "1"});

    assert_eq!(single_document(doc.clone(), "/x").unwrap(), doc);
    assert_eq!(single_document(json!([doc.clone()]), "/x").unwrap(), doc);
    assert!(single_document(json!([]), "/x").is_err());
    assert!(single_document(json!([doc.clone(), doc]), "/x").is_err());
    assert!(single_document(json!("text"), "/x").is_err());
    assert!(single_document(json!([1]), "/x").is_err());
}

#[test]
fn the_byte_count_is_read_from_the_summary_line_of_stderr() {
    let stderr = "warning: something\n{\"keys\": 3, \"bytes_processed\": 12345}\n";

    assert_eq!(bytes_processed(stderr), Some(12345));
    assert_eq!(bytes_processed(""), None);
    assert_eq!(bytes_processed("no summary here\n"), None);
    assert_eq!(bytes_processed("{\"keys\": 3}"), None);
}

#[test]
fn a_run_status_follows_the_outcome() {
    assert_eq!(run_status(RunOutcome::Succeeded), RunStatus::Succeeded);
    assert_eq!(run_status(RunOutcome::Partial), RunStatus::PartialSuccess);
    assert_eq!(run_status(RunOutcome::Stalled), RunStatus::PartialSuccess);
    assert_eq!(run_status(RunOutcome::Failed), RunStatus::Failed);
}

#[test]
fn the_tail_of_a_text_is_its_end_made_printable() {
    assert_eq!(tail("short", 100), "short");
    assert_eq!(tail("  padded \n", 100), "padded");
    assert_eq!(tail("abcdefgh", 3), "fgh");
    assert_eq!(tail("æøåæøå", 3), "æøå", "characters, not bytes");
    assert_eq!(tail("a\0b", 10), "a.b");
    assert_eq!(tail("", 10), "");
}

/// Records what the open-set write asks the stage writer for.
#[derive(Default)]
struct RecordedStage {
    calls: Mutex<Vec<StageCall>>,
}

#[derive(Debug, Clone, PartialEq)]
struct StageCall {
    table: String,
    pk: String,
    records: usize,
    run_id: String,
    lineage_endpoint: String,
    lineage_source: String,
    tenant: String,
}

#[async_trait]
impl StageWrite for RecordedStage {
    async fn upsert(
        &self,
        table: &str,
        pk: &str,
        records: &[Value],
        lineage: MergeLineage<'_>,
        tenant: &str,
    ) -> Result<StageOutcome> {
        self.calls.lock().unwrap().push(StageCall {
            table: table.to_string(),
            pk: pk.to_string(),
            records: records.len(),
            run_id: lineage.run_id.to_string(),
            lineage_endpoint: lineage.endpoint.to_string(),
            lineage_source: lineage.source.to_string(),
            tenant: tenant.to_string(),
        });
        Ok(StageOutcome {
            rows: records.len() as u64,
            changed: 2,
            deleted: 0,
            delete_skipped: 0,
        })
    }

    async fn quarantine_rows(&self, records: &[Value]) -> Result<u64> {
        Ok(records.len() as u64)
    }
}

#[tokio::test]
async fn the_write_goes_to_the_endpoints_own_staging_table_keyed_like_a_normal_sync() {
    let stage = RecordedStage::default();
    let bq = BqWriter {
        project: "p".into(),
        dataset_bronze: "s".into(),
        dataset_shadow: "s_shadow".into(),
        dataset_staging: Some("s".into()),
    };
    let staging = BqStaging {
        bq: &bq,
        stager: &stage,
        keys_command: "unused",
    };
    let run_id = Uuid::new_v4();

    let changed = staging
        .write(
            &tenant(),
            &supplier_def(),
            run_id,
            vec![json!({"documentType": "Invoice", "referenceNumber": "1"})],
        )
        .await
        .unwrap();

    assert_eq!(changed, 2, "what the MERGE changed, not what was delivered");
    let calls = stage.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![StageCall {
            table: "invoices".into(),
            pk: "tenant_id,documentType,referenceNumber".into(),
            records: 1,
            run_id: run_id.to_string(),
            // the endpoint's own name: the heartbeat of the write is recorded under it
            lineage_endpoint: "invoices".into(),
            lineage_source: "erp_rest".into(),
            tenant: "33f319b9-4d57-11ee-960c-025417856183".into(),
        }]
    );
}

#[test]
fn the_cadence_row_is_a_partition_no_fetch_ever_uses() {
    let key = state_key(
        &TenantId::new("t"),
        &def(Some("/x"), PrimaryKey::Single("k".into())),
    );

    assert_eq!(key.partition_key, "_open_set");
    assert_eq!(key.source, Source::ErpRest);
    assert_eq!(key.endpoint, "invoices");
}

// The HTTP side, against a local server ---------------------------------------------------

type Routes = HashMap<&'static str, (u16, String)>;

/// A server that answers the token request and the routes it is given by path, and records
/// every request target (path and query). One connection per request, like the client tests.
async fn serve(routes: Routes) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0_u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let target = request.split_whitespace().nth(1).unwrap_or("").to_string();
            let path = target.split('?').next().unwrap_or("").to_string();
            let (status, body) = if request.starts_with("POST /token ") {
                (200, r#"{"access_token":"t","expires_in":3600}"#.to_string())
            } else {
                recorded.lock().unwrap().push(target);
                routes
                    .get(path.as_str())
                    .cloned()
                    .unwrap_or((404, r#"{"message":"no such route"}"#.to_string()))
            };
            let response = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    (address, seen)
}

fn adapter(address: SocketAddr) -> ErpRestAdapter {
    let oauth = OAuthClient::new(format!("http://{address}/token"), "id", "secret")
        .with_scope(Scope::Erp, "financials:read");
    ErpRestAdapter {
        client: ErpRestClient::new(
            format!("http://{address}"),
            oauth,
            Limiter::new(Source::ErpRest, 6_000, 1_000_000),
        ),
        page_size: 100,
    }
}

fn customer_def() -> EndpointDefinition {
    EndpointDefinition {
        default_query: HashMap::from([
            ("expandApplications".to_string(), json!("true")),
            ("expandAttachments".to_string(), json!("true")),
        ]),
        ..def(
            Some("/v1/customerinvoice"),
            PrimaryKey::Single("referenceNumber".into()),
        )
    }
}

fn supplier_def() -> EndpointDefinition {
    def(
        Some("/v1/supplierInvoice"),
        PrimaryKey::Composite(vec!["documentType".into(), "referenceNumber".into()]),
    )
}

fn open_row(reference: &str, total: u64) -> Value {
    json!({
        "referenceNumber": reference, "documentType": "Invoice", "status": "Open",
        "balance": 100.5,
        "metadata": {"totalCount": total, "maxPageSize": 1000}
    })
}

fn tenant() -> TenantId {
    TenantId::new("33f319b9-4d57-11ee-960c-025417856183")
}

#[tokio::test]
async fn the_open_list_asks_for_open_documents_a_thousand_at_a_time_without_expands() {
    let body = json!([open_row("1", 2), open_row("2", 2)]).to_string();
    let (address, seen) = serve(HashMap::from([("/v1/customerinvoice", (200, body))])).await;

    let list = adapter(address)
        .list_open(&tenant(), &customer_def())
        .await
        .unwrap();

    assert_eq!(
        list.docs,
        vec![
            OpenDoc::new(DocKey::new(["1"]), Some(100.5)),
            OpenDoc::new(DocKey::new(["2"]), Some(100.5))
        ],
        "each row gives its key and its balance"
    );
    assert_eq!(
        list.page_metrics.len(),
        1,
        "totalCount is reached: one request"
    );
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    for wanted in ["status=Open", "pageSize=1000", "pageNumber=1"] {
        assert!(requests[0].contains(wanted), "{wanted} in {}", requests[0]);
    }
    assert!(!requests[0].contains("expand"), "{}", requests[0]);
}

#[tokio::test]
async fn an_open_list_with_a_closed_row_is_refused() {
    let closed = json!([
        open_row("1", 2),
        {"referenceNumber": "2", "documentType": "Invoice", "status": "Closed",
         "metadata": {"totalCount": 2}}
    ]);
    let (address, _) = serve(HashMap::from([(
        "/v1/customerinvoice",
        (200, closed.to_string()),
    )]))
    .await;

    let error = adapter(address)
        .list_open(&tenant(), &customer_def())
        .await
        .unwrap_err();

    assert!(
        error.to_string().contains("did not apply the filter"),
        "{error}"
    );
}

#[tokio::test]
async fn an_empty_open_list_is_an_empty_set() {
    let (address, _) = serve(HashMap::from([(
        "/v1/customerinvoice",
        (200, "[]".to_string()),
    )]))
    .await;

    let list = adapter(address)
        .list_open(&tenant(), &customer_def())
        .await
        .unwrap();

    assert!(list.docs.is_empty());
    assert_eq!(list.page_metrics.len(), 1);
}

#[tokio::test]
async fn a_document_is_fetched_at_its_key_with_the_endpoints_expand_flags() {
    let body = json!({"referenceNumber": "023672", "status": "Closed"}).to_string();
    let (address, seen) = serve(HashMap::from([("/v1/customerinvoice/023672", (200, body))])).await;

    let doc = adapter(address)
        .fetch_by_key(&tenant(), &customer_def(), &DocKey::new(["023672"]))
        .await
        .unwrap()
        .expect("a document");

    assert_eq!(doc["status"], "Closed");
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("/v1/customerinvoice/023672?"),
        "{}",
        requests[0]
    );
    assert!(
        requests[0].contains("expandApplications=true"),
        "{}",
        requests[0]
    );
    assert!(
        requests[0].contains("expandAttachments=true"),
        "{}",
        requests[0]
    );
}

#[tokio::test]
async fn a_supplier_document_is_fetched_by_document_type_and_reference_number() {
    let body = json!({"documentType": "DebitAdj", "referenceNumber": "200089"}).to_string();
    let (address, seen) = serve(HashMap::from([(
        "/v1/supplierInvoice/DebitAdj/200089",
        (200, body),
    )]))
    .await;

    let doc = adapter(address)
        .fetch_by_key(
            &tenant(),
            &supplier_def(),
            &DocKey::new(["DebitAdj", "200089"]),
        )
        .await
        .unwrap();

    assert!(doc.is_some());
    assert_eq!(
        seen.lock().unwrap().clone(),
        vec!["/v1/supplierInvoice/DebitAdj/200089"]
    );
}

#[tokio::test]
async fn a_404_is_none_and_any_other_failure_is_an_error() {
    let (address, _) = serve(HashMap::from([(
        "/v1/customerinvoice/boom",
        (500, r#"{"message":"boom"}"#.to_string()),
    )]))
    .await;
    let adapter = adapter(address);

    let gone = adapter
        .fetch_by_key(&tenant(), &customer_def(), &DocKey::new(["999999"]))
        .await;
    let broken = adapter
        .fetch_by_key(&tenant(), &customer_def(), &DocKey::new(["boom"]))
        .await;

    assert!(matches!(gone, Ok(None)), "{gone:?}");
    assert!(
        matches!(broken, Err(Error::UpstreamStatus { status: 500, .. })),
        "{broken:?}"
    );
}

#[tokio::test]
async fn the_http_adapter_is_what_the_trait_hands_out() {
    use crate::adapter::FetchAdapter;

    let (address, _) = serve(HashMap::new()).await;

    assert!(adapter(address).open_set_source().is_some());
    assert!(crate::noop_adapter::NoopAdapter { source_name: "x" }
        .open_set_source()
        .is_none());
}

// The key tool, a script standing in for vsv2-bq-open-keys -----------------------------------

#[cfg(unix)]
mod key_tool {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    /// One of these tests at a time. A script that is still open for writing in one thread when
    /// another thread starts a process is refused with "text file busy" on Linux.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn writer(staging: Option<&str>) -> BqWriter {
        BqWriter {
            project: "aquatiq-datavarehus".into(),
            dataset_bronze: "dw_staging_visma".into(),
            dataset_shadow: "dw_staging_visma_shadow".into(),
            dataset_staging: staging.map(str::to_string),
        }
    }

    /// An executable that records the arguments it was started with in `args.txt`, then runs
    /// `body`. Returns the script's path and the file of arguments.
    fn fake_tool(dir: &Path, body: &str) -> (String, PathBuf) {
        let args_file = dir.join("args.txt");
        let script = dir.join("vsv2-bq-open-keys");
        let text = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n{body}\n",
            args_file.display()
        );
        std::fs::write(&script, text).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (script.to_string_lossy().into_owned(), args_file)
    }

    async fn open_keys(command: &str, bq: &BqWriter) -> Result<StagedOpen> {
        BqStaging {
            bq,
            stager: bq,
            keys_command: command,
        }
        .open_keys(&tenant(), &supplier_def())
        .await
    }

    #[tokio::test]
    async fn the_tool_gets_project_dataset_table_tenant_and_key_fields_and_its_output_is_read() {
        let _serial = SERIAL.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let (command, args_file) = fake_tool(
            dir.path(),
            r#"echo '{"documentType":"Invoice","referenceNumber":"1","balance":"1450.5"}'
echo '{"documentType":"DebitAdj","referenceNumber":"2","balance":null}'
echo '{"keys": 2, "bytes_processed": 1917072, "bytes_billed": 10485760}' >&2"#,
        );

        let staged = open_keys(&command, &writer(Some("dw_staging_visma")))
            .await
            .unwrap();

        assert_eq!(
            staged.docs,
            vec![
                OpenDoc::new(DocKey::new(["Invoice", "1"]), Some(1450.5)),
                OpenDoc::new(DocKey::new(["DebitAdj", "2"]), None)
            ]
        );
        assert_eq!(staged.bytes_processed, Some(1_917_072));
        let args: Vec<String> = std::fs::read_to_string(args_file)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(
            args,
            [
                "--project",
                "aquatiq-datavarehus",
                "--dataset",
                "dw_staging_visma",
                "--table",
                "invoices",
                "--tenant",
                "33f319b9-4d57-11ee-960c-025417856183",
                "--pk",
                "documentType,referenceNumber",
            ]
        );
    }

    #[tokio::test]
    async fn a_tool_that_prints_nothing_means_no_open_documents() {
        let _serial = SERIAL.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let (command, _) = fake_tool(
            dir.path(),
            r#"echo '{"keys": 0, "bytes_processed": 7}' >&2"#,
        );

        let staged = open_keys(&command, &writer(Some("s"))).await.unwrap();

        assert!(staged.docs.is_empty());
        assert_eq!(staged.bytes_processed, Some(7));
    }

    #[tokio::test]
    async fn a_tool_that_prints_keys_alone_still_reads_with_unknown_balances() {
        let _serial = SERIAL.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let (command, _) = fake_tool(
            dir.path(),
            r#"echo '{"documentType":"Invoice","referenceNumber":"1"}'
echo '{"keys": 1, "bytes_processed": 9}' >&2"#,
        );

        let staged = open_keys(&command, &writer(Some("s"))).await.unwrap();

        assert_eq!(
            staged.docs,
            vec![OpenDoc::new(DocKey::new(["Invoice", "1"]), None)]
        );
    }

    #[tokio::test]
    async fn a_tool_that_fails_gives_its_error_line_and_no_keys() {
        let _serial = SERIAL.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let (command, _) = fake_tool(
            dir.path(),
            "echo 'error: aquatiq-datavarehus.s.invoices was not found' >&2\nexit 2",
        );

        let error = open_keys(&command, &writer(Some("s")))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("exited non-zero"), "{error}");
        assert!(error.contains("was not found"), "{error}");
    }

    #[tokio::test]
    async fn a_crash_is_reported_by_its_last_line_not_by_the_start_of_the_traceback() {
        let _serial = SERIAL.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let (command, _) = fake_tool(
            dir.path(),
            "i=0; while [ $i -lt 40 ]; do echo '  File \"x.py\", line 1, in <module>' >&2; i=$((i+1)); done\n\
             echo 'google.auth.exceptions.DefaultCredentialsError: no credentials' >&2\nexit 1",
        );

        let error = open_keys(&command, &writer(Some("s")))
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("DefaultCredentialsError: no credentials"),
            "{error}"
        );
        assert!(
            error.chars().count() < 800,
            "bounded: {}",
            error.chars().count()
        );
    }

    #[tokio::test]
    async fn output_that_is_not_keys_is_an_error_not_an_empty_set() {
        let _serial = SERIAL.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let (command, _) = fake_tool(dir.path(), "echo 'Traceback (most recent call last):'");

        let error = open_keys(&command, &writer(Some("s")))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("open keys line 1"), "{error}");
    }

    #[tokio::test]
    async fn a_tool_that_is_not_installed_is_reported_by_name() {
        let _serial = SERIAL.lock().await;
        let error = open_keys("/nonexistent/vsv2-bq-open-keys", &writer(Some("s")))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("spawn failed"), "{error}");
        assert!(error.contains("vsv2-bq-open-keys"), "{error}");
    }

    #[tokio::test]
    async fn without_a_staging_dataset_nothing_is_started() {
        let _serial = SERIAL.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let (command, args_file) = fake_tool(dir.path(), "echo '{}'");

        let error = open_keys(&command, &writer(None))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("VSV2_BIGQUERY_DATASET_STAGING"), "{error}");
        assert!(!args_file.exists(), "the tool was never started");
    }

    /// The one environment variable that makes `stage_writer_child` do its work.
    const CHILD_MARKER: &str = "VSV2_TEST_STAGE_WRITER_CHILD";

    /// Does the work of `the_real_stage_writer_upserts_and_never_asks_for_a_complete_scan`, in a
    /// process of its own whose `PATH` leads to a fake `vsv2-bq-stage`: changing the environment of
    /// the test process while other tests run in threads is not safe. Without the marker (a plain
    /// `cargo test`) it does nothing.
    #[tokio::test]
    async fn stage_writer_child() {
        if std::env::var_os(CHILD_MARKER).is_none() {
            return;
        }
        let bq = writer(Some("dw_staging_visma"));
        let lineage = MergeLineage {
            run_id: "run-1",
            endpoint: "invoices",
            source: "erp_rest",
        };
        let records = [json!({"tenant_id": "t", "referenceNumber": "1"})];

        let outcome = StageWrite::upsert(
            &bq,
            "invoices",
            "tenant_id,referenceNumber",
            &records,
            lineage,
            "t",
        )
        .await
        .expect("the fake stage tool answers");

        assert_eq!(outcome.changed, 1);
    }

    /// The line that matters most in the write: `BqWriter::upsert` must call `stage` with
    /// `complete = false`. A complete scan would mark every row of the tenant that the batch lacks
    /// as deleted, and a batch is a handful of documents.
    #[tokio::test]
    async fn the_real_stage_writer_upserts_and_never_asks_for_a_complete_scan() {
        let _serial = SERIAL.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("stage_args.txt");
        let script = dir.path().join("vsv2-bq-stage");
        let text = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n\
             echo '{{\"rows\": 1, \"changed\": 1, \"deleted\": 0, \"delete_skipped\": 0}}'\n",
            args_file.display()
        );
        std::fs::write(&script, text).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!(
            "{}:{}",
            dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );

        let child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "open_set::adapters::tests::key_tool::stage_writer_child",
                "--nocapture",
            ])
            .env("PATH", path)
            .env(CHILD_MARKER, "1")
            .output()
            .await
            .unwrap();

        assert!(
            child.status.success(),
            "the child test failed:\n{}\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        let args: Vec<String> = std::fs::read_to_string(&args_file)
            .expect("the fake vsv2-bq-stage was started")
            .lines()
            .map(str::to_string)
            .collect();
        assert!(!args.iter().any(|arg| arg == "--complete"), "{args:?}");
        for (flag, value) in [
            ("--project", "aquatiq-datavarehus"),
            ("--dataset", "dw_staging_visma"),
            ("--table", "invoices"),
            ("--pk", "tenant_id,referenceNumber"),
            ("--run-id", "run-1"),
            ("--endpoint", "invoices"),
            ("--source", "erp_rest"),
            ("--tenant", "t"),
        ] {
            let at = args
                .iter()
                .position(|arg| arg == flag)
                .unwrap_or_else(|| panic!("{flag} is missing from {args:?}"));
            assert_eq!(args[at + 1], value, "{flag} in {args:?}");
        }
    }
}
