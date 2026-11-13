//! The owner read `knowledge_list_documents` in the Do loop (WS24-S8): the org's document
//! inventory from Data Plane v2, read on the run's own delegated user bearer.
//!
//! Inventory only. A row carries a document's id, title, source, type and status, never its
//! content, so a question about what a document says still goes through `knowledge_search`. The
//! bearer is the one `knowledge_search` forwards, so Data Plane applies the user's own document
//! visibility; no service credential is minted here.
//!
//! The model is offered the tool only when `MODEL_ACTION_ADMISSION=table` and the run is not
//! retention-bound. A call that reaches dispatch under any other condition is refused before any
//! RPC. The Ask loop answers the same read through the gateway's verified request; the two share a
//! name and a row cap, not code, because the Do loop's authority is the run's own bearer.

use std::time::Duration;

use mp_contracts::dataplane::documents_v2::{
    document_service_client::DocumentServiceClient, Document, ListDocumentsRequest,
    ListDocumentsResponse,
};
use mp_contracts::model_plane::v1::ToolDefinition;
use serde::Deserialize;
use serde_json::{json, Value};
use tonic::{metadata::MetadataValue, transport::Channel, Status};

use crate::tool_bridge::ToolExecution;

/// The Do-loop name of the read. The Ask loop answers the same read under this name.
pub const TOOL_NAME: &str = "knowledge_list_documents";
/// The admission contract row this tool executes.
pub const ACTION_ID: &str = "knowledge.list_documents";

const DESCRIPTION: &str = "List WHICH documents exist in the organization's knowledge base, with each document's title, source, type, status, and the org-wide total. Use this for inventory questions: how many documents there are, which sources are in the knowledge base, whether a report has been ingested yet. Do NOT use it to answer what a document SAYS: it returns no document content, so use knowledge_search for the substance of the material. Do NOT use it for public-web questions (use web_search).";
const PARAMETERS_JSON: &str = r#"{"type":"object","additionalProperties":false,"properties":{"type":{"type":"string","description":"Optional document-type filter as stored by Data Plane (e.g. 'pdf', 'web_page'). Omit to list all types."},"limit":{"type":"integer","description":"Max documents to list, 1-25 (default 25). The org-wide total is always reported regardless of this cap."}}}"#;

/// Documents shown to the model, however many the upstream reports.
const MAX_ROWS: usize = 25;
/// The largest row count a call may ask for; the request never asks for more than a row shown.
const MAX_LIMIT: i32 = 25;
const MAX_TYPE_CHARS: usize = 64;
const MAX_TITLE_CHARS: usize = 180;
const RPC_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether the read is offered to the model, and whether dispatch runs it: the admission switch
/// names the table and the run is not retention-bound. `retention_bound` folds in a waived route,
/// as `offered_tools` passes it.
#[must_use]
pub fn permitted(admission_table: bool, retention_bound: bool) -> bool {
    admission_table && !retention_bound
}

/// Whether this deployment's admission switch names the table. Strict like Capability Core: only
/// the literal `table` admits the owner reads.
#[must_use]
pub fn admission_table() -> bool {
    #[cfg(test)]
    {
        if let Some(on) = test_switches::admission_override() {
            return on;
        }
    }
    crate::control_http_client::env_value("MODEL_ACTION_ADMISSION").as_deref() == Some("table")
}

/// Whether this deployment refuses the listing on a Space-scoped run (`OWNER_LIST_SPACE_REFUSE`).
/// Strict: only the literal `true` switches it on, so the listing keeps its organisation-wide
/// reach everywhere else. Dark by default (D2-769). The value is read raw, not through the
/// trimming `env_value`, so a padded value such as ` true` stays off, as in model-gateway.
#[must_use]
pub fn space_refuse_enabled() -> bool {
    std::env::var("OWNER_LIST_SPACE_REFUSE").ok().as_deref() == Some("true")
}

/// Whether a call on a run in `space_id` is refused because the run is Space-scoped. The Data Plane
/// listing takes no Space argument, so it would answer with the whole organisation's titles; a
/// Space-scoped run therefore gets no listing at all rather than a wider one than its retrieval.
#[must_use]
pub fn space_refused(space_id: &str) -> bool {
    !space_id.trim().is_empty() && space_refuse_enabled()
}

/// The typed refusal a Space-scoped run receives in place of the listing.
pub const SPACE_REFUSAL: &str = "knowledge_list_documents is not available in a Space-scoped run";

/// The refusal a call on the listing gets before its arguments are read, or `None` when the call may
/// go on. `space_refused` is the Space refusal for the call's run (see [`space_refused`]). The Space
/// refusal is checked first (D2-774): a Space-scoped run is refused whatever `MODEL_ACTION_ADMISSION`
/// says, because that switch decides only whether the model is offered the listing. The switch and the
/// retention bar come next (WS24-S8).
#[must_use]
pub fn refusal_before_arguments(
    space_refused: bool,
    admission_table: bool,
    retention_bound: bool,
) -> Option<String> {
    if space_refused {
        return Some(SPACE_REFUSAL.to_owned());
    }
    if !permitted(admission_table, retention_bound) {
        return Some(format!(
            "{TOOL_NAME} is not available on this run: the owner document listing needs MODEL_ACTION_ADMISSION=table and a run that is not retention-bound"
        ));
    }
    None
}

/// The definition offered to the model.
#[must_use]
pub fn tool_def() -> ToolDefinition {
    ToolDefinition {
        name: TOOL_NAME.to_owned(),
        description: DESCRIPTION.to_owned(),
        parameters_json: PARAMETERS_JSON.to_owned(),
    }
}

/// The arguments a call may choose: a lowercased type filter (empty lists every type) and a row
/// count in `1..=MAX_LIMIT`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ListArgs {
    doc_type: String,
    limit: i32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ListInput {
    #[serde(rename = "type")]
    doc_type: String,
    limit: Option<i32>,
}

fn parse_input(tool_input: &str) -> Result<ListArgs, String> {
    let text = if tool_input.trim().is_empty() {
        "{}"
    } else {
        tool_input
    };
    let input: ListInput = serde_json::from_str(text)
        .map_err(|error| format!("invalid {TOOL_NAME} input: {error}"))?;
    let doc_type = input.doc_type.trim().to_lowercase();
    if doc_type.chars().count() > MAX_TYPE_CHARS {
        return Err(format!(
            "{TOOL_NAME} type must be at most {MAX_TYPE_CHARS} characters"
        ));
    }
    let limit = input.limit.unwrap_or(MAX_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(format!("{TOOL_NAME} limit must be between 1 and {MAX_LIMIT}"));
    }
    Ok(ListArgs { doc_type, limit })
}

/// Trimmed title, cut after [`MAX_TITLE_CHARS`] characters.
fn trim_title(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.chars().count() <= MAX_TITLE_CHARS {
        return trimmed.to_owned();
    }
    let mut cut: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    cut.push('…');
    cut
}

fn inventory_row(document: &Document) -> Value {
    json!({
        "document_id": document.document_id,
        "title": trim_title(&document.title),
        "source": document.source,
        "type": document.r#type,
        "status": document.status,
    })
}

/// The tool result: the org-wide total, the rows shown and how many were withheld. Each row carries
/// the five inventory fields only, so no document content or metadata reaches the model.
fn listing_output(response: &ListDocumentsResponse) -> String {
    let rows: Vec<Value> = response
        .documents
        .iter()
        .take(MAX_ROWS)
        .map(inventory_row)
        .collect();
    json!({
        "upstream": "data-plane-v2 DocumentService.ListDocuments",
        "total_in_org": response.total,
        "returned": rows.len(),
        "withheld": response.documents.len().saturating_sub(MAX_ROWS),
        "documents": rows,
    })
    .to_string()
}

/// One `ListDocuments` call on the run's bearer, for the run's verified org.
async fn fetch(
    channel: Channel,
    bearer: &str,
    org_id: &str,
    args: &ListArgs,
) -> Result<ListDocumentsResponse, Status> {
    let authorization = MetadataValue::try_from(format!("Bearer {bearer}"))
        .map_err(|_| Status::invalid_argument("credential is malformed"))?;
    let mut request = tonic::Request::new(ListDocumentsRequest {
        org_id: org_id.to_owned(),
        limit: args.limit,
        offset: 0,
        r#type: args.doc_type.clone(),
    });
    request.metadata_mut().insert("authorization", authorization);
    let outcome = tokio::time::timeout(
        RPC_TIMEOUT,
        DocumentServiceClient::new(channel).list_documents(request),
    )
    .await
    .map_err(|_| Status::deadline_exceeded("Data Plane ListDocuments timed out"))?;
    Ok(outcome?.into_inner())
}

fn tool_error(message: String) -> ToolExecution {
    ToolExecution {
        output: String::new(),
        error: Some(message),
    }
}

/// Runs the Do-loop read. The Space refusal comes first (D2-774), then the switch and retention checks,
/// then the arguments, the run's org and the bearer; each refusal is a typed tool error returned before
/// any RPC. `space_id` is the run's Space (empty for a run outside one): with
/// `OWNER_LIST_SPACE_REFUSE=true` a Space-scoped run is refused, fail closed, because the listing
/// cannot be narrowed to the Space.
pub async fn execute(
    tool_input: &str,
    org_id: &str,
    space_id: &str,
    admission_table: bool,
    retention_bound: bool,
    data_plane_bearer: Option<&str>,
) -> ToolExecution {
    if let Some(refusal) =
        refusal_before_arguments(space_refused(space_id), admission_table, retention_bound)
    {
        return tool_error(refusal);
    }
    let args = match parse_input(tool_input) {
        Ok(args) => args,
        Err(error) => return tool_error(error),
    };
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return tool_error(format!("{TOOL_NAME} requires a run org_id (tenant scope)"));
    }
    let Some(bearer) = data_plane_bearer
        .filter(|value| !value.trim().is_empty() && *value == value.trim())
    else {
        return tool_error(format!(
            "{TOOL_NAME} requires the originating verified user credential"
        ));
    };
    let Some(url) = crate::knowledge_tools::retrieval_endpoint_from_env() else {
        return tool_error(crate::knowledge_tools::format_degraded_error(&format!(
            "{TOOL_NAME} unavailable: DATAPLANE_RETRIEVAL_URL not configured"
        )));
    };
    let Ok(endpoint) = Channel::from_shared(url) else {
        return tool_error(crate::knowledge_tools::format_degraded_error(&format!(
            "{TOOL_NAME} unavailable: the Data Plane endpoint is not a valid URL"
        )));
    };
    match fetch(endpoint.connect_lazy(), bearer, org_id, &args).await {
        Ok(response) => ToolExecution {
            output: listing_output(&response),
            error: None,
        },
        Err(status) => tool_error(crate::knowledge_tools::format_degraded_error(&format!(
            "{TOOL_NAME} failed: {}",
            status.message()
        ))),
    }
}

/// The owner listing's switches for the tests that set them. The Space refusal's switch is a process-wide
/// environment variable, so [`test_switches::Switches::set`] holds one lock for as long as it is set. The
/// admission switch is read per thread here: a test sets it for its own thread only, so no other test's
/// offered tools see it, and each test's switches are put back on drop.
#[cfg(test)]
pub(crate) mod test_switches {
    use std::cell::Cell;

    /// The Space refusal's environment variable.
    const SPACE_SWITCH: &str = "OWNER_LIST_SPACE_REFUSE";
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    thread_local! {
        /// This thread's admission switch while a test sets it; `None` reads the environment.
        static ADMISSION: Cell<Option<bool>> = const { Cell::new(None) };
    }

    /// The admission switch a test set for this thread, if any.
    pub(crate) fn admission_override() -> Option<bool> {
        ADMISSION.with(Cell::get)
    }

    /// The owner listing's switches, set for one test.
    pub(crate) struct Switches {
        _lock: tokio::sync::MutexGuard<'static, ()>,
        previous_space: Option<String>,
        previous_admission: Option<bool>,
    }

    impl Switches {
        /// `space_refuse` sets `OWNER_LIST_SPACE_REFUSE`, `None` unsetting it. `admission_table` sets this
        /// thread's admission switch: `Some(true)` offers and admits the listing, `None` reads the environment.
        pub(crate) async fn set(space_refuse: Option<&str>, admission_table: Option<bool>) -> Self {
            let lock = LOCK.lock().await;
            let previous_space = std::env::var(SPACE_SWITCH).ok();
            match space_refuse {
                Some(value) => std::env::set_var(SPACE_SWITCH, value),
                None => std::env::remove_var(SPACE_SWITCH),
            }
            let previous_admission = ADMISSION.with(|cell| cell.replace(admission_table));
            Self {
                _lock: lock,
                previous_space,
                previous_admission,
            }
        }
    }

    impl Drop for Switches {
        fn drop(&mut self) {
            match &self.previous_space {
                Some(value) => std::env::set_var(SPACE_SWITCH, value),
                None => std::env::remove_var(SPACE_SWITCH),
            }
            ADMISSION.with(|cell| cell.set(self.previous_admission));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn document(id: &str, title: &str, content: &str) -> Document {
        Document {
            document_id: id.to_owned(),
            org_id: "org-a".to_owned(),
            source: "upload".to_owned(),
            r#type: "pdf".to_owned(),
            title: title.to_owned(),
            content: content.to_owned(),
            status: "indexed".to_owned(),
            metadata: None,
            created_at: None,
            updated_at: None,
            deleted_at: None,
            zdr_classification: String::new(),
            zdr_reason: None,
        }
    }

    fn keys(value: &Value) -> BTreeSet<&str> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn the_tool_is_named_for_the_contract_row_and_bound_to_the_retrieval_capability() {
        assert_eq!(TOOL_NAME, "knowledge_list_documents");
        assert_eq!(ACTION_ID, "knowledge.list_documents");
        assert_eq!(
            crate::capability_policy::trusted_capability_id(TOOL_NAME).as_deref(),
            Some("cap.retrieval.query")
        );
    }

    #[test]
    fn the_read_is_permitted_only_under_the_table_switch_and_never_under_retention() {
        assert!(permitted(true, false));
        assert!(!permitted(false, false));
        assert!(!permitted(true, true));
        assert!(!permitted(false, true));
    }

    #[test]
    fn the_offered_schema_is_closed_and_names_no_content_field() {
        let definition = tool_def();
        assert_eq!(definition.name, TOOL_NAME);
        assert!(!definition.description.trim().is_empty());
        let schema: Value = serde_json::from_str(&definition.parameters_json).expect("schema parses");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(keys(&schema["properties"]), BTreeSet::from(["limit", "type"]));
    }

    #[test]
    fn input_is_closed_defaults_to_every_type_and_the_full_cap_and_bounds_the_limit() {
        let all = ListArgs {
            doc_type: String::new(),
            limit: 25,
        };
        assert_eq!(parse_input("{}"), Ok(all.clone()));
        assert_eq!(parse_input("  "), Ok(all.clone()));
        assert_eq!(parse_input(r#"{"type":"  PDF "}"#).expect("type").doc_type, "pdf");
        assert_eq!(parse_input(r#"{"limit":5}"#).expect("limit").limit, 5);
        assert!(parse_input(r#"{"limit":26}"#).is_err());
        assert!(parse_input(r#"{"limit":0}"#).is_err());
        assert!(parse_input(r#"{"status":"indexed"}"#).is_err());
        assert!(parse_input("not json").is_err());
        assert!(parse_input(&format!(r#"{{"type":"{}"}}"#, "x".repeat(65))).is_err());
    }

    #[test]
    fn a_listing_carries_five_fields_caps_the_rows_and_never_carries_content() {
        let documents: Vec<Document> = (0..30)
            .map(|index| document(&format!("doc-{index}"), &format!("Title {index}"), "SECRET BODY"))
            .collect();
        let output = listing_output(&ListDocumentsResponse {
            documents,
            total: 30,
        });
        assert!(!output.contains("SECRET BODY"));
        let value: Value = serde_json::from_str(&output).expect("output is JSON");
        assert_eq!(value["total_in_org"], 30);
        assert_eq!(value["returned"], 25);
        assert_eq!(value["withheld"], 5);
        let rows = value["documents"].as_array().expect("rows");
        assert_eq!(rows.len(), 25);
        for row in rows {
            assert_eq!(
                keys(row),
                BTreeSet::from(["document_id", "source", "status", "title", "type"])
            );
        }
    }

    #[test]
    fn a_long_title_is_cut_and_a_padded_one_is_trimmed() {
        let long = format!("  {}  ", "a".repeat(200));
        let trimmed = trim_title(&long);
        assert_eq!(trimmed.chars().count(), MAX_TITLE_CHARS + 1);
        assert!(trimmed.ends_with('…'));
        assert_eq!(trim_title("  Policy  "), "Policy");
    }

    #[tokio::test]
    async fn refusals_are_tool_errors_returned_before_any_rpc() {
        let off = execute("{}", "org-a", "", false, false, Some("token")).await;
        assert!(off
            .error
            .as_deref()
            .is_some_and(|error| error.contains("MODEL_ACTION_ADMISSION=table")));
        assert!(execute("{}", "org-a", "", true, true, Some("token"))
            .await
            .error
            .is_some());
        let no_bearer = execute("{}", "org-a", "", true, false, None).await;
        assert!(no_bearer
            .error
            .as_deref()
            .is_some_and(|error| error.contains("verified user credential")));
        let padded = execute("{}", "org-a", "", true, false, Some(" token")).await;
        assert!(padded
            .error
            .as_deref()
            .is_some_and(|error| error.contains("verified user credential")));
        let no_org = execute("{}", "  ", "", true, false, Some("token")).await;
        assert!(no_org
            .error
            .as_deref()
            .is_some_and(|error| error.contains("run org_id")));
        let bad_input =
            execute(r#"{"limit":99}"#, "org-a", "", true, false, Some("token")).await;
        assert!(bad_input
            .error
            .as_deref()
            .is_some_and(|error| error.contains("between 1 and 25")));
    }

    #[test]
    fn a_space_refusal_comes_before_the_admission_and_retention_refusals() {
        for (admission, retention) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_eq!(
                refusal_before_arguments(true, admission, retention).as_deref(),
                Some(SPACE_REFUSAL),
                "admission {admission}, retention {retention}"
            );
        }
    }

    #[test]
    fn an_unscoped_run_keeps_the_admission_and_retention_refusals_as_they_were() {
        let off = refusal_before_arguments(false, false, false).expect("refused");
        assert!(off.contains("MODEL_ACTION_ADMISSION=table"), "{off}");
        let retained = refusal_before_arguments(false, true, true).expect("refused");
        assert!(retained.contains("not retention-bound"), "{retained}");
        assert_eq!(refusal_before_arguments(false, true, false), None);
    }

    #[tokio::test]
    async fn the_space_switch_is_on_only_for_the_literal_true_and_a_named_space() {
        {
            let _switches = test_switches::Switches::set(Some("true"), None).await;
            assert!(space_refused("space-1"));
            assert!(!space_refused("   "));
            assert!(!space_refused(""));
        }
        {
            let _switches = test_switches::Switches::set(Some("TRUE"), None).await;
            assert!(!space_refused("space-1"));
        }
        let _switches = test_switches::Switches::set(None, None).await;
        assert!(!space_refused("space-1"));
    }

    #[tokio::test]
    async fn a_space_run_is_refused_before_the_admission_switch_and_the_credential_are_read() {
        let _switches = test_switches::Switches::set(Some("true"), None).await;
        let off = execute("{}", "org-a", "space-1", false, false, None).await;
        assert_eq!(off.error.as_deref(), Some(SPACE_REFUSAL));
        let on = execute("{}", "org-a", "space-1", true, false, Some("token")).await;
        assert_eq!(on.error.as_deref(), Some(SPACE_REFUSAL));
    }

    #[tokio::test]
    async fn with_the_space_switch_off_a_space_run_meets_the_checks_it_always_did() {
        let _switches = test_switches::Switches::set(None, None).await;
        let off = execute("{}", "org-a", "space-1", false, false, Some("token")).await;
        assert!(off
            .error
            .as_deref()
            .is_some_and(|error| error.contains("MODEL_ACTION_ADMISSION=table")));
        let no_bearer = execute("{}", "org-a", "space-1", true, false, None).await;
        assert!(no_bearer
            .error
            .as_deref()
            .is_some_and(|error| error.contains("verified user credential")));
    }

    // The tests above prove refusal never reaches the network. This one proves the other half: a
    // permitted listing reaches a real server on the run's bearer, for the run's org, with the
    // type filter and the row count, and the reply is projected to the five fields.
    mod wire_round_trip {
        use std::sync::{Arc, Mutex};

        use mp_contracts::dataplane::documents_v2::{
            document_service_server::{DocumentService, DocumentServiceServer},
            BulkIngestRequest, BulkIngestResponse, CreateDocumentRequest, CreateDocumentResponse,
            DeleteDocumentRequest, DeleteDocumentResponse, GetDocumentIndexStatusRequest,
            GetDocumentIndexStatusResponse, GetDocumentRequest, GetDocumentResponse,
            IngestStatusRequest, IngestStatusResponse,
        };
        use tonic::{Response, Status as TonicStatus};

        use super::*;

        #[derive(Clone, Default)]
        struct RecordingDocumentService {
            last_authorization: Arc<Mutex<Option<String>>>,
            last_request: Arc<Mutex<Option<ListDocumentsRequest>>>,
        }

        #[tonic::async_trait]
        impl DocumentService for RecordingDocumentService {
            async fn get_document(
                &self,
                _request: tonic::Request<GetDocumentRequest>,
            ) -> Result<Response<GetDocumentResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn list_documents(
                &self,
                request: tonic::Request<ListDocumentsRequest>,
            ) -> Result<Response<ListDocumentsResponse>, TonicStatus> {
                *self.last_authorization.lock().unwrap() = request
                    .metadata()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                *self.last_request.lock().unwrap() = Some(request.into_inner());
                Ok(Response::new(ListDocumentsResponse {
                    documents: vec![document("doc-1", "  Policy  ", "SECRET BODY")],
                    total: 1,
                }))
            }
            async fn create_document(
                &self,
                _request: tonic::Request<CreateDocumentRequest>,
            ) -> Result<Response<CreateDocumentResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn delete_document(
                &self,
                _request: tonic::Request<DeleteDocumentRequest>,
            ) -> Result<Response<DeleteDocumentResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn bulk_ingest(
                &self,
                _request: tonic::Request<BulkIngestRequest>,
            ) -> Result<Response<BulkIngestResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn get_document_index_status(
                &self,
                _request: tonic::Request<GetDocumentIndexStatusRequest>,
            ) -> Result<Response<GetDocumentIndexStatusResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn get_ingest_status(
                &self,
                _request: tonic::Request<IngestStatusRequest>,
            ) -> Result<Response<IngestStatusResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
        }

        #[tokio::test]
        async fn the_listing_reaches_a_real_server_on_the_run_bearer_and_is_projected() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind fake Data Plane server");
            let address = listener.local_addr().expect("listener address");
            let service = RecordingDocumentService::default();
            let server_service = service.clone();
            let handle = tokio::spawn(async move {
                let _ = tonic::transport::Server::builder()
                    .add_service(DocumentServiceServer::new(server_service))
                    .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                    .await;
            });
            let channel = Channel::from_shared(format!("http://{address}"))
                .expect("valid endpoint")
                .connect()
                .await
                .expect("connect to fake Data Plane server");

            let args = ListArgs {
                doc_type: "pdf".to_owned(),
                limit: 7,
            };
            let response = fetch(channel, "user-bearer", "org-a", &args)
                .await
                .expect("the listing reaches the server");

            assert_eq!(
                service.last_authorization.lock().unwrap().as_deref(),
                Some("Bearer user-bearer")
            );
            let sent = service
                .last_request
                .lock()
                .unwrap()
                .clone()
                .expect("request recorded");
            assert_eq!(sent.org_id, "org-a");
            assert_eq!(sent.limit, 7);
            assert_eq!(sent.offset, 0);
            assert_eq!(sent.r#type, "pdf");
            let output: Value =
                serde_json::from_str(&listing_output(&response)).expect("output is JSON");
            assert_eq!(output["documents"][0]["title"], "Policy");
            assert!(!listing_output(&response).contains("SECRET BODY"));

            handle.abort();
        }
    }
}

/// Section 3.4: `OWNER_LIST_SPACE_REFUSE` is documented strict: only the literal `true` switches it on.
#[cfg(test)]
mod space_switch_parse {
    use super::space_refuse_enabled;
    use super::test_switches::Switches;

    #[tokio::test]
    async fn only_the_literal_true_switches_the_space_refusal_on() {
        let mut wrong = Vec::new();
        for (raw, on) in [
            (Some("true"), true),
            (Some(" true"), false),
            (Some("true\n"), false),
            (Some("TRUE"), false),
            (Some("1"), false),
            (Some(""), false),
            (None, false),
        ] {
            let _switches = Switches::set(raw, None).await;
            if space_refuse_enabled() != on {
                wrong.push(raw);
            }
        }
        assert!(wrong.is_empty(), "values read wrongly: {wrong:?}");
    }
}
