//! The state SQL: the shape of the statements (always) and the statements themselves against the
//! migrated schema as the runtime role (`#[ignore]`d behind `MIGRATED_TEST_DATABASE_URL`, and run,
//! never skipped, by `tests/scripts/run-postgres-contract-tests.sh`).
//!
//! The scenarios of the pipeline (`pipeline::tests`) already run the claim, the settlement, the
//! markers and the scan end to end on the same schema; what is here is what they do not say:
//! organization isolation of every statement, every settlement the state machine can produce
//! accepted by the table's own constraints, the policy and the spend the gates read, and the
//! erasure.

use std::collections::BTreeSet;

use super::*;
use crate::pipeline::harness::{Control, Database};
use crate::state::{ErrorClass, Outcome, Stamp, Status, ZdrReason, DEFAULT_MAX_ATTEMPTS};

// ── the shape of the statements ──────────────────────────────────────────────

#[test]
fn the_due_rule_is_one_definition_used_by_the_claim_and_the_scan() {
    let in_claim = due_predicate("s", "$4::text");
    let in_scan = due_predicate("s", "$3::text");
    assert!(
        CLAIM_SQL.contains(&in_claim),
        "the claim builds on the rule"
    );
    assert!(
        DUE_DOCUMENTS_SQL.contains(&in_scan),
        "and so does the scan: they cannot disagree about whether a chunk is due"
    );
}

#[test]
fn the_due_rule_picks_up_the_work_statuses_and_a_stale_empty_and_nothing_final() {
    let rule = due_predicate("s", "$4::text");
    for status in WORK_STATUSES {
        assert!(
            rule.contains(&format!("'{}'", status.as_str())),
            "{status:?}"
        );
    }
    assert!(rule.contains("s.status = 'empty' AND s.prompt_version IS DISTINCT FROM $4::text"));
    for final_status in ["'ready'", "'failed_permanent'"] {
        assert!(
            !rule.contains(final_status),
            "{final_status} is never due, whatever its timestamps"
        );
    }
    // A held chunk is not due, whatever its status.
    assert!(rule.starts_with("((s.lease_until IS NULL OR s.lease_until <= now()) AND ("));
    // A NULL wait is due now.
    assert!(rule.contains("s.next_attempt_at IS NULL OR s.next_attempt_at <= now()"));
}

#[test]
fn every_statement_is_bound_to_the_organization_it_serves() {
    for (name, sql) in [
        ("claim", CLAIM_SQL.as_str()),
        ("mark document", MARK_DOCUMENT_SQL.as_str()),
        ("mark organization", MARK_ORG_SQL.as_str()),
        ("mark restricted", MARK_RESTRICTED_SQL.as_str()),
        ("due documents", DUE_DOCUMENTS_SQL.as_str()),
        ("settle", SETTLE_SQL),
        ("standing", STANDING_SQL),
        ("policy", POLICY_SQL),
        ("month tokens", MONTH_TOKENS_SQL),
    ] {
        let bound = ["org_id = $1", "org_id = $2"]
            .iter()
            .any(|predicate| sql.contains(predicate));
        assert!(bound, "{name} does not bind the organization: {sql}");
    }
    // The organization is a parameter, never interpolated: no statement holds a quoted value
    // other than the fixed vocabularies.
    for sql in [
        CLAIM_SQL.as_str(),
        MARK_ORG_SQL.as_str(),
        DUE_DOCUMENTS_SQL.as_str(),
    ] {
        let literals: BTreeSet<&str> = sql
            .split('\'')
            .skip(1)
            .step_by(2)
            .filter(|literal| !literal.trim().is_empty() && *literal != ", ")
            .collect();
        for literal in literals {
            assert!(
                Status::ALL.iter().any(|status| status.as_str() == literal)
                    || EGRESS_ALLOWED_CLASSIFICATIONS.contains(&literal)
                    || matches!(literal, "org" | "pending" | "empty"),
                "an unexpected literal `{literal}` in a statement"
            );
        }
    }
}

/// Every statement of the state store runs in an organization-scoped transaction
/// (`begin_org_scoped`: the organization set, then the runtime role), because that is what makes
/// row-level security real for it: the services connect as a superuser, on which RLS is inert.
/// The database test below proves the premise (a scoped transaction IS the runtime role) and that
/// the SQL binds its organization; neither can see a method that quietly runs on the plain pool,
/// which is the one regression that would leave the policy as inert as it was before this table
/// had one (a mutation that did exactly that passed every database test). So the text is
/// checked: every method that sends a statement opens a scoped transaction, except the boot
/// check, which reads the catalogue and no organization's rows.
#[test]
fn every_method_that_sends_a_statement_opens_an_organization_scoped_transaction() {
    let source = include_str!("../state_store.rs");
    let production: String = source
        .split("#[cfg(test)]")
        .next()
        .expect("the production part of the file")
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut sending = Vec::new();
    for body in production.split("async fn ").skip(1) {
        let name = body.split('(').next().expect("a function name").trim();
        if !body.contains("sqlx::query") {
            continue;
        }
        sending.push(name.to_owned());
        assert!(
            name == "verify_state_schema" || body.contains("begin_org_scoped("),
            "{name} sends a statement outside an organization-scoped transaction: row-level security would not apply to it"
        );
    }
    // The test must have found the methods, or it proves nothing.
    for expected in [
        "claim_chunk",
        "settle_chunk",
        "mark_population",
        "document_standing",
        "list_due_documents",
        "org_policy",
        "month_llm_tokens",
        "verify_state_schema",
    ] {
        assert!(
            sending.iter().any(|name| name == expected),
            "the scan did not find {expected}: {sending:?}"
        );
    }
}

#[test]
fn the_classification_the_sql_allows_is_the_one_egress_allowed_allows() {
    let sql = allowed_classification_sql();
    for classification in EGRESS_ALLOWED_CLASSIFICATIONS {
        assert!(sql.contains(&format!("'{classification}'")));
        assert!(crate::state::egress_allowed(Some(classification)));
    }
    assert!(sql.contains("lower(btrim(d.zdr_classification))"));
    // The claim holds the same rule itself: it is the last gate before a chunk is sent.
    assert!(CLAIM_SQL.contains(&sql));
    // The scan and the withheld-marking are complementary: one takes the allowed, the other the rest.
    assert!(DUE_DOCUMENTS_SQL.contains(&sql));
    assert!(MARK_ORG_SQL.contains(&sql));
    assert!(MARK_RESTRICTED_SQL.contains(&format!("NOT ({sql})")));
}

#[test]
fn a_marker_never_rewrites_a_final_or_a_held_row() {
    for sql in [
        MARK_DOCUMENT_SQL.as_str(),
        MARK_ORG_SQL.as_str(),
        MARK_RESTRICTED_SQL.as_str(),
    ] {
        // Both the rows it selects and the rows it may update are the waiting, unleased ones.
        assert_eq!(sql.matches("cur.status IN (").count(), 1);
        assert!(sql.contains("s.status IN ("));
        assert!(sql.contains("cur.lease_until <= now()"));
        assert!(sql.contains("s.lease_until <= now()"));
        for final_status in ["'ready'", "'empty'", "'failed_permanent'"] {
            assert!(!sql.contains(final_status), "{final_status} in a marker");
        }
        // It is bounded.
        assert!(sql.contains("LIMIT $5"));
    }
}

/// The partial index of the migration covers exactly the statuses the rule picks up, or the
/// work queue would scan the table.
#[test]
fn the_work_index_of_the_migration_covers_exactly_the_statuses_the_rule_picks_up() {
    let migration = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../infra/postgres/migrations/20261003150000_graph_extraction_state.sql"
    ))
    .expect("the migration")
    .replace("\r\n", "\n");
    let at = migration
        .find("CREATE INDEX IF NOT EXISTS idx_ges_work")
        .expect("the work index");
    let statement = &migration[at..at + migration[at..].find(';').expect("an end")];
    let in_index: BTreeSet<&str> = statement.split('\'').skip(1).step_by(2).collect();
    let in_rule: BTreeSet<&str> = WORK_STATUSES.iter().map(|status| status.as_str()).collect();
    assert_eq!(in_index, in_rule);
}

#[test]
fn a_database_without_the_migration_is_refused_with_the_name_of_the_migration() {
    assert_eq!(schema_problem(true, true, true, true), None);
    let problem = schema_problem(false, false, true, true).expect("a problem");
    assert!(
        problem.contains("20261003150000_graph_extraction_state") && problem.contains("migrate up"),
        "{problem}"
    );
    let unprotected = schema_problem(true, false, true, true).expect("a problem");
    assert!(unprotected.contains("row-level security"), "{unprotected}");
    let policy = schema_problem(true, true, false, true).expect("a problem");
    assert!(
        policy.contains("20261003130000_context_provenance"),
        "{policy}"
    );
    let ledger = schema_problem(true, true, true, false).expect("a problem");
    assert!(ledger.contains("20261003140000_cost_events_v2"), "{ledger}");
}

// ── against the migrated schema, as the runtime role ─────────────────────────

#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_migrated_schema_is_the_one_this_service_boots_against() {
    let rig = Database::new().await;
    rig.store()
        .verify_state_schema()
        .await
        .expect("the migrated schema passes the boot-time check");
}

#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn every_statement_runs_as_the_runtime_role_and_one_organization_cannot_touch_anothers() {
    let rig = Database::new().await;
    let (a, b) = (rig.org("iso-a"), rig.org("iso-b"));
    let a_chunks = rig
        .add_document(&a, "d", "org", "internal", &["one", "two"])
        .await;
    rig.add_document(&b, "d", "org", "internal", &["three"])
        .await;
    let store = rig.store();

    // The premise: the scoped transaction really is the runtime role, which is subject to RLS.
    let pool = rig.pool().clone();
    let mut tx = pg_org_scope::begin_org_scoped(&pool, &a)
        .await
        .expect("a scoped transaction");
    let (user,): (String,) = sqlx::query_as("SELECT current_user::text")
        .fetch_one(&mut *tx)
        .await
        .expect("current_user");
    tx.commit().await.expect("commit");
    assert_eq!(user, "dataplane_app");

    // Organization B can claim, settle, mark and scan nothing of A's, even naming A's ids.
    let lease = std::time::Duration::from_secs(60);
    assert_eq!(
        store
            .claim_chunk(&b, &a_chunks[0], lease)
            .await
            .expect("claim"),
        None,
        "a chunk of another organization is not claimable"
    );
    assert!(
        store
            .claim_chunk(&a, &a_chunks[0], lease)
            .await
            .expect("claim")
            .is_some(),
        "and it is claimable by its own"
    );
    // The claim is the last gate before a chunk is sent, so it holds the document's standing
    // itself: a private chunk and a restricted one are not claimable even when named (the callers
    // check as well; this is the statement that takes the chunk).
    let withheld = [
        rig.add_document(&a, "private", "private", "internal", &["a private chunk"])
            .await,
        rig.add_document(
            &a,
            "restricted",
            "org",
            "restricted",
            &["a restricted chunk"],
        )
        .await,
    ];
    for chunk in withheld.iter().flatten() {
        assert_eq!(
            store.claim_chunk(&a, chunk, lease).await.expect("claim"),
            None,
            "{chunk} must not be claimable"
        );
        assert!(
            rig.row(&a, chunk).await.is_none(),
            "and no state is created for {chunk}"
        );
    }
    let settlement = Outcome::SkippedPolicy.settle(0, DEFAULT_MAX_ATTEMPTS);
    assert!(
        !store
            .settle_chunk(&b, &a_chunks[0], &settlement)
            .await
            .expect("settle"),
        "another organization's settlement matches nothing"
    );
    assert_eq!(
        store
            .mark_document(&b, &format!("{a}-d"), &settlement, 100)
            .await
            .expect("mark"),
        0
    );
    assert_eq!(
        rig.row(&a, &a_chunks[0]).await.expect("state").status,
        "pending",
        "A's row is exactly as A left it (claimed, unsettled)"
    );
    let due_b = store.list_due_documents(&b, 10).await.expect("scan");
    assert_eq!(
        due_b,
        vec![format!("{b}-d")],
        "B sees its own document and not A's"
    );
    assert!(store
        .document_standing(&b, &format!("{a}-d"))
        .await
        .expect("standing")
        .is_none());
}

/// Every settlement the state machine can produce is accepted by the table's constraints
/// (`status`, `error_class`, the failure-needs-a-class rule, the stamp and the yield rules): the
/// pure test in `state` applies the CHECKs to the settlements; this applies the database.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn every_settlement_the_state_machine_produces_is_accepted_by_the_table() {
    let rig = Database::new().await;
    let org = rig.org("accept");
    let stamp = |entities, relationships, claims| Stamp {
        prompt_version: PROMPT_VERSION.to_owned(),
        model: "gpt-4o-mini-2024-07-18".to_owned(),
        provider: "azure-openai".to_owned(),
        entities,
        relationships,
        claims,
    };
    let outcomes = vec![
        Outcome::Extracted(stamp(2, 1, 1)),
        Outcome::Extracted(stamp(0, 0, 0)),
        Outcome::SkippedZdr(ZdrReason::Org),
        Outcome::SkippedZdr(ZdrReason::Document),
        Outcome::SkippedZdr(ZdrReason::NoAttestedRoute),
        Outcome::SkippedPolicy,
        Outcome::SkippedVisibility,
        Outcome::SkippedBudget,
        Outcome::PostureUnavailable {
            retry_after: std::time::Duration::from_secs(300),
        },
        Outcome::ResidencyUnavailable,
        Outcome::Transient {
            class: ErrorClass::Auth,
            retry_after: None,
        },
        Outcome::Transient {
            class: ErrorClass::RateLimited,
            retry_after: Some(std::time::Duration::from_secs(7_200)),
        },
        Outcome::Transient {
            class: ErrorClass::InvalidOutput,
            retry_after: None,
        },
        Outcome::Permanent {
            class: ErrorClass::TooLong,
        },
    ];
    let texts: Vec<String> = (0..outcomes.len()).map(|n| format!("chunk {n}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let chunks = rig.add_document(&org, "d", "org", "internal", &refs).await;
    let lease = std::time::Duration::from_secs(60);

    for (outcome, chunk) in outcomes.iter().zip(&chunks) {
        let claim = rig
            .store()
            .claim_chunk(&org, chunk, lease)
            .await
            .expect("claim")
            .expect("a new chunk is claimable");
        let settlement = outcome.settle(claim.attempts, DEFAULT_MAX_ATTEMPTS);
        assert!(
            rig.store()
                .settle_chunk(&org, chunk, &settlement)
                .await
                .unwrap_or_else(|error| panic!("{outcome:?} was refused by the table: {error}")),
            "{outcome:?}"
        );
        let row = rig.row(&org, chunk).await.expect("state");
        assert_eq!(row.status, settlement.status.as_str(), "{outcome:?}");
        assert_eq!(
            row.error_class.as_deref(),
            settlement.error_class.map(ErrorClass::as_str),
            "{outcome:?}"
        );
        assert!(!row.leased, "settling releases the lease: {outcome:?}");
        assert_eq!(
            row.attempts,
            if settlement.refund_attempt { 0 } else { 1 },
            "{outcome:?}"
        );
        assert_eq!(
            row.retry_in_secs.is_some(),
            settlement.retry_in.is_some(),
            "{outcome:?}"
        );
    }
    let ready = rig.row(&org, &chunks[0]).await.expect("state");
    assert_eq!(
        (
            ready.prompt_version.as_deref(),
            ready.model.as_deref(),
            ready.provider.as_deref()
        ),
        (
            Some(PROMPT_VERSION),
            Some("gpt-4o-mini-2024-07-18"),
            Some("azure-openai")
        )
    );
    assert_eq!(
        (ready.entities, ready.relationships, ready.claims),
        (Some(2), Some(1), Some(1))
    );
}

#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_scan_is_ordered_bounded_and_skips_what_is_not_due() {
    let rig = Database::new().await;
    let org = rig.org("scan");
    let held = rig
        .add_document(&org, "a-held", "org", "internal", &["x"])
        .await;
    rig.add_document(&org, "b-due", "org", "internal", &["x"])
        .await;
    rig.add_document(&org, "c-due", "org", "internal", &["x"])
        .await;
    rig.add_document(&org, "d-due", "org", "internal", &["x"])
        .await;
    rig.add_document(&org, "e-private", "private", "internal", &["x"])
        .await;
    rig.add_document(&org, "f-restricted", "org", "restricted", &["x"])
        .await;
    let store = rig.store();
    store
        .claim_chunk(&org, &held[0], std::time::Duration::from_secs(60))
        .await
        .expect("claim")
        .expect("claimed");

    assert_eq!(
        store.list_due_documents(&org, 2).await.expect("scan"),
        vec![format!("{org}-b-due"), format!("{org}-c-due")],
        "ordered by id, bounded, the held document, the private one and the restricted one left out"
    );
    assert_eq!(
        store
            .list_due_documents(&org, 10)
            .await
            .expect("scan")
            .len(),
        3
    );
}

#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_policy_is_read_as_the_table_has_it_and_an_absent_row_is_the_default() {
    let rig = Database::new().await;
    let org = rig.org("policy-read");
    assert_eq!(
        rig.store().org_policy(&org).await.expect("policy"),
        OrgPolicy::DEFAULT,
        "no row: graph extraction on, everything inherited"
    );
    rig.set_policy(&org, "off", "stop", Some(25.5)).await;
    assert_eq!(
        rig.store().org_policy(&org).await.expect("policy"),
        OrgPolicy {
            graph_extraction: Switch::Off,
            budget_mode: Some(BudgetMode::Stop),
            monthly_budget_usd: Some(25.5),
        }
    );
    rig.set_policy(&org, "on", "inherit", None).await;
    assert_eq!(
        rig.store().org_policy(&org).await.expect("policy"),
        OrgPolicy {
            graph_extraction: Switch::On,
            budget_mode: None,
            monthly_budget_usd: None,
        }
    );
}

#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_months_spend_counts_this_organizations_llm_events_of_this_month_and_nothing_else() {
    let rig = Database::new().await;
    let (a, b) = (rig.org("spend-a"), rig.org("spend-b"));
    let insert = |org: String,
                  event_type: &'static str,
                  feature: &'static str,
                  input: i64,
                  output: i64,
                  age: &'static str| {
        let pool = rig.pool().clone();
        async move {
            sqlx::query(
                "INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key, \
                     schema_version, feature, provider_used, input_tokens, output_tokens, cache_read_tokens, \
                     cost_basis, estimated, zdr, request_id, created_at) \
                 VALUES ($1, 'gpt-4o-mini', $2, 1, 0, $3, 2, $4, 'azure-openai', $5, $6, 0, 'metered', false, false, $3, \
                     now() - $7::interval)",
            )
            .bind(event_type)
            .bind(org)
            .bind(format!("spend-{}", uuid::Uuid::new_v4()))
            .bind(feature)
            .bind(input)
            .bind(output)
            .bind(age)
            .execute(&pool)
            .await
            .expect("a usage event");
        }
    };
    insert(
        a.clone(),
        "llm",
        "graph-extraction",
        1_000,
        200,
        "0 seconds",
    )
    .await;
    insert(
        a.clone(),
        "llm",
        "contextual-retrieval",
        500,
        50,
        "0 seconds",
    )
    .await;
    insert(a.clone(), "rerank", "rerank", 9_999, 0, "0 seconds").await;
    insert(
        a.clone(),
        "llm",
        "graph-extraction",
        7_777,
        7_777,
        "70 days",
    )
    .await;
    insert(
        b.clone(),
        "llm",
        "graph-extraction",
        4_000,
        400,
        "0 seconds",
    )
    .await;

    assert_eq!(
        rig.store().month_llm_tokens(&a).await.expect("spend"),
        (1_500, 250),
        "this organization's background LLM events of this month: not a rerank, not last quarter, not another organization's"
    );
    assert_eq!(
        rig.store().month_llm_tokens(&b).await.expect("spend"),
        (4_000, 400)
    );
    assert_eq!(
        rig.store()
            .month_llm_tokens(&rig.org("spend-none"))
            .await
            .expect("spend"),
        (0, 0)
    );
}

#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn erasing_an_organization_erases_what_was_done_to_its_chunks() {
    let rig = Database::new().await;
    let (a, b) = (rig.org("erase-a"), rig.org("erase-b"));
    let a_chunks = rig
        .add_document(&a, "d", "org", "internal", &["one", "two"])
        .await;
    let b_chunks = rig
        .add_document(&b, "d", "org", "internal", &["three"])
        .await;
    let settlement = Outcome::SkippedPolicy.settle(0, DEFAULT_MAX_ATTEMPTS);
    for org in [&a, &b] {
        rig.store()
            .mark_document(org, &format!("{org}-d"), &settlement, 100)
            .await
            .expect("mark");
    }
    assert!(rig.row(&a, &a_chunks[0]).await.is_some());

    let summary = rig
        .store()
        .purge_organization_data(&a)
        .await
        .expect("purge");

    assert_eq!(summary.graph_extraction_state, 2);
    assert!(rig.row(&a, &a_chunks[0]).await.is_none());
    assert!(
        rig.row(&b, &b_chunks[0]).await.is_some(),
        "another organization's state is untouched"
    );
    assert_eq!(
        rig.store()
            .purge_organization_data(&a)
            .await
            .expect("a replay")
            .graph_extraction_state,
        0,
        "a redelivered erasure event matches nothing and is not an error"
    );
}
