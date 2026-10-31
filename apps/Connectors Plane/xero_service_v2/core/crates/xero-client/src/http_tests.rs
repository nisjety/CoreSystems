use super::*;

use chrono::{TimeZone, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::extended::{ExtendedFamily, ExtendedRequest, ResponseShape};

struct FixtureResponse {
    status: &'static str,
    body: &'static [u8],
    headers: &'static [(&'static str, &'static str)],
}

async fn fixture_server(
    responses: Vec<FixtureResponse>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 16 * 1024];
            let count = stream.read(&mut request).await.unwrap();
            requests.push(String::from_utf8_lossy(&request[..count]).into_owned());

            let mut head = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                response.status,
                response.body.len()
            );
            for (name, value) in response.headers {
                head.push_str(name);
                head.push_str(": ");
                head.push_str(value);
                head.push_str("\r\n");
            }
            head.push_str("\r\n");
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(response.body).await.unwrap();
        }
        requests
    });
    (base_url, task)
}

fn local_client(base_url: &str, tenant: &str) -> XeroApiClient {
    XeroApiClient::new_with_tenant_header(tenant, true)
        .with_test_base_url(format!("{base_url}/api.xro/2.0"))
}

#[tokio::test]
async fn fetch_pages_with_auth_headers_and_modified_window() {
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Invoices":[{"InvoiceID":"keep","UpdatedDateUTC":"2026-04-21T12:00:00Z"}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Invoices":[]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-pages");
    let modified_after = Utc.with_ymd_and_hms(2026, 4, 20, 0, 0, 0).unwrap();

    let records = client
        .fetch(
            "fixture-token",
            &EntityType::Invoices,
            Some(modified_after),
            None,
            2,
        )
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["InvoiceID"], "keep");
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("GET /api.xro/2.0/Invoices?page=1&pageSize=2"));
    assert!(requests[1].contains("GET /api.xro/2.0/Invoices?page=2&pageSize=2"));
    let first = requests[0].to_ascii_lowercase();
    assert!(first.contains("authorization: bearer fixture-token"));
    assert!(first.contains("xero-tenant-id: tenant-pages"));
    assert!(first.contains("if-modified-since: 2026-04-20t00:00:00"));
}

#[tokio::test]
async fn extras_expand_details_and_keep_headers_when_one_detail_fails() {
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Budgets":[{"BudgetID":"expanded"},{"BudgetID":"header-only"}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Budgets":{"BudgetID":"expanded","BudgetLines":[{"Amount":12}]}}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "404 Not Found",
            body: br#"{"Message":"missing"}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-expand");
    let records = client
        .fetch_with_extras(
            "fixture-token",
            &EntityType::Budgets,
            None,
            None,
            &ExtraQuery {
                expand_lines: true,
                extra: vec![("tracking".into(), "true".into())],
                ..ExtraQuery::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["BudgetLines"][0]["Amount"], 12);
    assert_eq!(records[1]["BudgetID"], "header-only");
    let requests = server.await.unwrap();
    assert!(requests[0].contains("GET /api.xro/2.0/Budgets?tracking=true"));
    assert!(requests[1].contains("GET /api.xro/2.0/Budgets/expanded?tracking=true"));
    assert!(requests[2].contains("GET /api.xro/2.0/Budgets/header-only?tracking=true"));
}

#[tokio::test]
async fn reports_support_parsed_and_verbatim_responses() {
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Reports":[{"ReportName":"BalanceSheet","Rows":[]}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Reports":[{"ReportName":"BalanceSheet","Rows":[{"RowType":"Header"}]}]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-reports");
    let extra = ExtraQuery {
        extra: vec![("date".into(), "2026-04-30".into())],
        ..ExtraQuery::default()
    };

    let parsed = client
        .fetch_report("fixture-token", &EntityType::ReportBalanceSheet, &extra)
        .await
        .unwrap();
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0]["_params"]["date"], "2026-04-30");

    let raw = client
        .fetch_report_raw("fixture-token", &EntityType::ReportBalanceSheet, &extra)
        .await
        .unwrap();
    assert_eq!(raw.http_status, 200);
    assert!(raw.body.starts_with(br#"{"Reports"#));
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.contains("GET /api.xro/2.0/Reports/BalanceSheet?date=2026-04-30")));

    assert!(client
        .fetch_report("fixture-token", &EntityType::Invoices, &extra)
        .await
        .is_err());
}

#[tokio::test]
async fn business_date_and_journal_paths_use_their_respective_pagination_rules() {
    let (base_url, business_server) = fixture_server(vec![
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Invoices":[{"InvoiceID":"inside","Date":"2026-04-21"}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Invoices":[]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-business-date");
    let window = DateWindow::new(
        chrono::NaiveDate::from_ymd_opt(2026, 4, 20).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 4, 22).unwrap(),
    )
    .unwrap();
    let result = client
        .fetch_by_business_date_tracked("fixture-token", &EntityType::Invoices, window, 25)
        .await
        .unwrap();
    assert_eq!(result.records[0]["InvoiceID"], "inside");
    assert_eq!(result.outcome.pages_fetched, 1);
    let requests = business_server.await.unwrap();
    assert!(requests[0].contains("GET /api.xro/2.0/Invoices?where="));
    assert!(requests[0].contains("page=1") && requests[0].contains("pageSize=25"));

    let (base_url, journal_server) = fixture_server(vec![
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Journals":[{"JournalID":"one","JournalNumber":9}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Journals":[]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-journals");
    let journals = client
        .fetch("fixture-token", &EntityType::Journals, None, None, 100)
        .await
        .unwrap();
    assert_eq!(journals[0]["JournalID"], "one");
    let requests = journal_server.await.unwrap();
    assert!(requests[0].contains("GET /api.xro/2.0/Journals?offset=0"));
    assert!(requests[1].contains("GET /api.xro/2.0/Journals?offset=9"));
}

#[tokio::test]
async fn raw_pages_discovery_and_429_retry_run_against_local_http() {
    let (base_url, raw_server) = fixture_server(vec![
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Invoices":[{"InvoiceID":"one"}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Invoices":[]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-raw-pages");
    let (pages, outcome) = client
        .fetch_raw_pages(
            "fixture-token",
            &EntityType::Invoices,
            None,
            &ExtraQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(outcome.termination, TerminationReason::EndedOnEmptyPage);
    assert!(raw_server.await.unwrap()[1].contains("page=2"));

    let (base_url, discovery_server) = fixture_server(vec![FixtureResponse {
        status: "200 OK",
        body: br#"{"Invoices":[{"InvoiceID":"parent"}]}"#,
        headers: &[],
    }])
    .await;
    let client = local_client(&base_url, "tenant-discovery");
    let parents = client
        .fetch_discovery_page("fixture-token", &EntityType::Invoices, 3, 40)
        .await
        .unwrap();
    assert_eq!(parents[0]["InvoiceID"], "parent");
    assert!(discovery_server.await.unwrap()[0].contains("page=3&pageSize=40"));

    let (base_url, retry_server) = fixture_server(vec![
        FixtureResponse {
            status: "429 Too Many Requests",
            body: br#"{"Message":"slow down"}"#,
            headers: &[("Retry-After", "0")],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Accounts":[{"AccountID":"after-retry"}]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-retry");
    let records = client
        .fetch_page("fixture-token", &EntityType::Accounts, None, 1, 100)
        .await
        .unwrap();
    assert_eq!(records[0]["AccountID"], "after-retry");
    assert_eq!(retry_server.await.unwrap().len(), 2);
}

#[tokio::test]
async fn extras_pagination_retries_a_server_error_and_preserves_supported_filters() {
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "500 Internal Server Error",
            body: br#"{"Message":"temporary"}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Invoices":[{"InvoiceID":"one"}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Invoices":[]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-extras-pagination");
    let result = client
        .fetch_with_extras_tracked(
            "fixture-token",
            &EntityType::Invoices,
            None,
            None,
            &ExtraQuery {
                page_size: Some(25),
                where_clause: Some("Status==\"AUTHORISED\"".into()),
                order: Some("Date DESC".into()),
                ids: Some("invoice-one,invoice-two".into()),
                include_archived: true,
                summary_only: true,
                statuses: Some("AUTHORISED,PAID".into()),
                extra: vec![("fixture".into(), "true".into())],
                ..ExtraQuery::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(result.records[0]["InvoiceID"], "one");
    assert_eq!(
        result.outcome.termination,
        TerminationReason::EndedOnEmptyPage
    );
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].contains("page=1&pageSize=25"));
    assert!(requests[1].contains("includeArchived=true"));
    assert!(requests[1].contains("summaryOnly=true"));
    assert!(requests[1].contains("Statuses=AUTHORISED%2CPAID"));
    assert!(requests[2].contains("page=2&pageSize=25"));
}

#[tokio::test]
async fn standard_and_raw_fetches_treat_not_modified_as_an_empty_page() {
    let (base_url, standard_server) = fixture_server(vec![FixtureResponse {
        status: "304 Not Modified",
        body: b"",
        headers: &[],
    }])
    .await;
    let client = local_client(&base_url, "tenant-standard-304");
    assert!(client
        .fetch("fixture-token", &EntityType::Accounts, None, None, 100)
        .await
        .unwrap()
        .is_empty());
    assert!(standard_server.await.unwrap()[0].contains("GET /api.xro/2.0/Accounts"));

    let (base_url, raw_server) = fixture_server(vec![FixtureResponse {
        status: "304 Not Modified",
        body: b"",
        headers: &[],
    }])
    .await;
    let client = local_client(&base_url, "tenant-raw-304");
    let (pages, outcome) = client
        .fetch_raw_pages(
            "fixture-token",
            &EntityType::Invoices,
            None,
            &ExtraQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].http_status, 304);
    assert_eq!(outcome.termination, TerminationReason::EndedOnEmptyPage);
    assert!(raw_server.await.unwrap()[0].contains("GET /api.xro/2.0/Invoices?page=1"));
}

#[tokio::test]
async fn parsed_report_retries_a_server_error_then_preserves_its_snapshot_envelope() {
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "503 Service Unavailable",
            body: br#"{"Message":"temporary"}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Reports":[{"ReportName":"ProfitAndLoss","Rows":[]}]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-report-retry");
    let report = client
        .fetch_report(
            "fixture-token",
            &EntityType::ReportProfitAndLoss,
            &ExtraQuery {
                extra: vec![
                    ("fromDate".into(), "2026-01-01".into()),
                    ("toDate".into(), "2026-01-31".into()),
                ],
                ..ExtraQuery::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(report.records.len(), 1);
    assert_eq!(
        report.records[0]["Report"]["Reports"][0]["ReportName"],
        "ProfitAndLoss"
    );
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].contains("fromDate=2026-01-01&toDate=2026-01-31"));
}

#[tokio::test]
async fn extended_reads_retry_then_follow_documented_page_count() {
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "429 Too Many Requests",
            body: br#"{"Message":"slow down"}"#,
            headers: &[("Retry-After", "0")],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"items":[{"assetId":"one"}],"pagination":{"pageCount":2}}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"items":[],"pagination":{"pageCount":2}}"#,
            headers: &[],
        },
    ])
    .await;
    let request = ExtendedRequest::new(
        ExtendedFamily::Assets,
        "assets_assets",
        "/Assets",
        ResponseShape::Items,
        true,
    )
    .unwrap()
    .with_test_base_url(base_url);
    let client = XeroApiClient::new_with_tenant_header("tenant-extended-assets", true);
    let (pages, outcome) = client
        .fetch_extended_raw("fixture-token", &request)
        .await
        .unwrap();

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].record_count, 1);
    assert_eq!(outcome.termination, TerminationReason::EndedOnEmptyPage);
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].contains("GET /Assets?page=1"));
    assert!(requests[2].contains("GET /Assets?page=2"));
    assert!(requests[1]
        .to_ascii_lowercase()
        .contains("xero-tenant-id: tenant-extended-assets"));
}

#[tokio::test]
async fn identity_extended_read_uses_no_tenant_header_and_accepts_a_root_array() {
    let (base_url, server) = fixture_server(vec![FixtureResponse {
        status: "200 OK",
        body: br#"[{"id":"connection-one"}]"#,
        headers: &[],
    }])
    .await;
    let request = ExtendedRequest::new(
        ExtendedFamily::Identity,
        "identity_connections",
        "/connections",
        ResponseShape::Array,
        false,
    )
    .unwrap()
    .with_test_base_url(base_url);
    let client = XeroApiClient::new_with_tenant_header("tenant-extended-identity", true);
    let (pages, outcome) = client
        .fetch_extended_raw("fixture-token", &request)
        .await
        .unwrap();

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].record_count, 1);
    assert_eq!(outcome.termination, TerminationReason::EndedOnEmptyPage);
    let request = server.await.unwrap().pop().unwrap();
    assert!(request.contains("GET /connections"));
    assert!(!request.to_ascii_lowercase().contains("xero-tenant-id"));
}

#[tokio::test]
async fn business_date_supports_purchase_order_ranges_and_local_journal_filtering() {
    let window = DateWindow::new(
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
    )
    .unwrap();
    let (base_url, orders_server) = fixture_server(vec![
        FixtureResponse {
            status: "200 OK",
            body: br#"{"PurchaseOrders":[{"PurchaseOrderID":"order-one","Date":"2026-01-15"}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"PurchaseOrders":[]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-business-purchase-orders");
    let orders = client
        .fetch_by_business_date_tracked("fixture-token", &EntityType::PurchaseOrders, window, 100)
        .await
        .unwrap();
    assert_eq!(orders.records.len(), 1);
    let requests = orders_server.await.unwrap();
    assert!(requests[0].contains("DateFrom=2026-01-01&DateTo=2026-01-31"));
    assert!(requests[0].contains("page=1"));

    let (base_url, journals_server) = fixture_server(vec![
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Journals":[{"JournalID":"in","JournalNumber":1,"JournalDate":"2026-01-15"},{"JournalID":"out","JournalNumber":2,"JournalDate":"2026-02-01"}]}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Journals":[]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-business-journals");
    let journals = client
        .fetch_by_business_date_tracked("fixture-token", &EntityType::Journals, window, 100)
        .await
        .unwrap();
    assert_eq!(journals.records.len(), 1);
    assert_eq!(journals.records[0]["JournalID"], "in");
    let requests = journals_server.await.unwrap();
    assert!(requests[0].contains("GET /api.xro/2.0/Journals?offset=0"));
    assert!(requests[0]
        .to_ascii_lowercase()
        .contains("if-modified-since"));
}

#[tokio::test]
async fn public_raw_detail_retries_a_server_error_and_keeps_the_detail_page_identity() {
    let budget_id = "5f3c9a2e-0000-0000-0000-000000000000";
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "500 Internal Server Error",
            body: br#"{"Message":"temporary"}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Budgets":{"BudgetID":"5f3c9a2e-0000-0000-0000-000000000000","BudgetLines":[]}}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-raw-detail");
    let page = client
        .fetch_raw_detail("fixture-token", &EntityType::Budgets, budget_id, 7)
        .await
        .unwrap();

    assert_eq!(page.page, 7);
    assert_eq!(page.record_count, 1);
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.contains(&format!("GET /api.xro/2.0/Budgets/{budget_id}"))));
}

#[tokio::test]
async fn raw_report_retries_rate_limit_then_returns_verbatim_snapshot_bytes() {
    let body = br#"{"Reports":[{"ReportName":"BalanceSheet","Rows":[]}]}"#;
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "429 Too Many Requests",
            body: br#"{"Message":"slow down"}"#,
            headers: &[("Retry-After", "0")],
        },
        FixtureResponse {
            status: "200 OK",
            body,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-raw-report-retry");
    let page = client
        .fetch_report_raw(
            "fixture-token",
            &EntityType::ReportBalanceSheet,
            &ExtraQuery {
                extra: vec![("date".into(), "2026-01-31".into())],
                ..ExtraQuery::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(page.body.as_ref(), body);
    assert_eq!(page.record_count, 1);
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.contains("GET /api.xro/2.0/Reports/BalanceSheet?date=2026-01-31")));
}

#[tokio::test]
async fn attachment_metadata_retries_rate_limit_through_the_public_client_api() {
    let invoice_id = "5f3c9a2e-0000-0000-0000-000000000000";
    let attachment_id = "aeb4f270-0000-0000-0000-000000000000";
    let (base_url, server) = fixture_server(vec![
        FixtureResponse {
            status: "500 Internal Server Error",
            body: br#"{"Message":"temporary"}"#,
            headers: &[],
        },
        FixtureResponse {
            status: "429 Too Many Requests",
            body: br#"{"Message":"slow down"}"#,
            headers: &[("Retry-After", "0")],
        },
        FixtureResponse {
            status: "200 OK",
            body: br#"{"Attachments":[{"AttachmentID":"aeb4f270-0000-0000-0000-000000000000"}]}"#,
            headers: &[],
        },
    ])
    .await;
    let client = local_client(&base_url, "tenant-attachment-retry");
    let listing = client
        .fetch_attachment_list_raw("fixture-token", &EntityType::Invoices, invoice_id)
        .await
        .unwrap();

    assert_eq!(listing.record_count, 1);
    assert!(listing
        .body
        .windows(attachment_id.len())
        .any(|window| window == attachment_id.as_bytes()));
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| {
        request.contains(&format!(
            "GET /api.xro/2.0/Invoices/{invoice_id}/Attachments"
        ))
    }));
}

#[tokio::test]
async fn attachment_metadata_returns_a_redacted_error_for_a_terminal_client_failure() {
    let (base_url, server) = fixture_server(vec![FixtureResponse {
        status: "404 Not Found",
        body: br#"{"Message":"private fixture diagnostic"}"#,
        headers: &[],
    }])
    .await;
    let client = local_client(&base_url, "tenant-attachment-not-found");
    let result = client
        .fetch_attachment_list_raw(
            "fixture-token",
            &EntityType::Invoices,
            "5f3c9a2e-0000-0000-0000-000000000000",
        )
        .await;
    let error = match result {
        Err(error) => error.to_string(),
        Ok(_) => panic!("404 attachment metadata response must fail"),
    };

    assert!(error.contains("404"));
    assert!(!error.contains("private fixture diagnostic"));
    assert_eq!(server.await.unwrap().len(), 1);
}

#[tokio::test]
async fn attachment_metadata_stops_after_the_bounded_server_error_retry_budget() {
    let (base_url, server) = fixture_server(
        (0..=retry::MAX_ATTEMPTS)
            .map(|_| FixtureResponse {
                status: "500 Internal Server Error",
                body: br#"{"Message":"temporary"}"#,
                headers: &[],
            })
            .collect(),
    )
    .await;
    let client = local_client(&base_url, "tenant-attachment-retry-budget");
    let result = client
        .fetch_attachment_list_raw(
            "fixture-token",
            &EntityType::Invoices,
            "5f3c9a2e-0000-0000-0000-000000000000",
        )
        .await;
    let error = match result {
        Err(error) => error.to_string(),
        Ok(_) => panic!("attachment retry budget must be enforced"),
    };

    assert!(error.contains("after"));
    assert_eq!(
        server.await.unwrap().len(),
        (retry::MAX_ATTEMPTS + 1) as usize
    );
}
