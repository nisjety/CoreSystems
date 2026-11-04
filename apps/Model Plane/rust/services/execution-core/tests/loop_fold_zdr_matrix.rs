//! The shared posture matrix, held against the shared durable gate (build plan WS03 S9, static
//! part).
//!
//! The durable loop writes content: a fold holds conversation text, a consumed approval hands a
//! retained call to the loop, a `/continue` resumes stored work, and a park keeps a run waiting.
//! Only a turn that keeps state may use any of them. `fixtures/zdr-posture-matrix.json` is a
//! byte-identical copy of the canonical matrix (`apps/Model Plane/api/contracts/
//! zdr-posture-matrix.json`), pinned beside it and checked by
//! `scripts/tests/zdr-contract-sync.test.sh`. Its `enforcement_rows` say what a turn is, from the
//! bearers and the route alone.
//!
//! # What this file proves, and what it does not
//!
//! Every row is decided here by the real resolver (`mp_contracts::retention::resolve_turn`, no
//! hand-made decision) and then handed to the shared durable gate (`mp_contracts::durable_gate::
//! refuse_unless_persistent`). The gate must be open for exactly the rows the matrix calls
//! `persistent` and shut for every other one: a ZDR row, a waived row (whose `zdr` claim is false,
//! so a check written as "not ZDR" lets it through) and a refused row.
//!
//! That is a proof about the SHARED GATE and the resolver. It is NOT a proof about any store:
//! no production code calls `refuse_unless_persistent` or `refuse_unless_persistent_class`, so
//! breaking the fold writer, the consume SQL, the `/continue` gate or the pause site leaves every
//! test below green. Each store decides persistence its own way, from a flag its caller computes,
//! and those gates are private to their crates. The shared gate is the specification they must
//! agree with, and the agreement is held from the other side:
//!
//! * `tests/durable_loop_contract.rs` holds the WIRING by reading source (the SQL compares the
//!   class this gate opens for; the writer gate is fed `zdr` and `ctx.route.is_waived()`; the
//!   pause site passes not-ZDR AND not-waived), and its `store_proofs()` table names, per store,
//!   the unit tests that drive that store's own gate with a zero-retention run and a waived run,
//!   failing if one is renamed, removed or commented out. It also fails if production starts
//!   calling the shared gate, so this header cannot stay stale unnoticed;
//! * the one function here that IS called in production is `loop_mode_for_persistent` (the pause
//!   site's mint, through `resume.rs`), so the minting test below is wiring, not specification.
//!
//! A row the gate cannot tell apart from a bug is a vacuous row, so the test also runs the pre-fix
//! rule ("not ZDR") against the same rows and requires it to DISAGREE with the matrix on the
//! waived rows. If the matrix lost its waived rows, that comparison would fail instead of the
//! whole file passing by having nothing to catch.

use mp_contracts::durable_gate::{
    loop_mode_for_persistent, refuse_unless_persistent, refuse_unless_persistent_class,
    DurableStore,
};
use mp_contracts::retention::{
    resolve_turn, ClaimsView, Reason, Resolution, RouteView, WaiverView,
};
use mp_contracts::zdr_refusal::code;
use serde_json::Value;
use std::collections::BTreeSet;

const MATRIX: &str = include_str!("fixtures/zdr-posture-matrix.json");

/// The enforcement rows of the canonical matrix when this test was written, and
/// how they split by decision. A row added or removed without this file being
/// read is a change nobody reviewed against the durable stores.
const ENFORCEMENT_ROWS: usize = 33;
const REFUSE: usize = 18;
const ZDR: usize = 7;
const PERSISTENT: usize = 3;
const WAIVED: usize = 4;

/// The only rows whose decision is `persistent`: the shared gate is open for these and
/// for nothing else.
const PERSISTENT_ROWS: [&str; 3] = [
    "e_persistent_default_route",
    "e_persistent_subscription_route_unchanged",
    "e_service_persistent_without_org_claim",
];

const STORES: [DurableStore; 4] = [
    DurableStore::Fold,
    DurableStore::Consume,
    DurableStore::Continue,
    DurableStore::Park,
];

fn matrix() -> Value {
    serde_json::from_str(MATRIX).expect("the vendored matrix is JSON")
}

fn rows(matrix: &Value) -> &Vec<Value> {
    matrix["enforcement_rows"]
        .as_array()
        .expect("enforcement_rows is an array")
}

fn id(row: &Value) -> &str {
    row["id"].as_str().expect("a row has an id")
}

fn decision(row: &Value) -> &str {
    row["expect"]["decision"]
        .as_str()
        .expect("a row expects a decision")
}

/// The claims of every bearer of a row, borrowing the strings of the matrix.
fn claims_of(row: &Value) -> Vec<ClaimsView<'_>> {
    row["inputs"]["bearers"]
        .as_array()
        .expect("bearers are an array")
        .iter()
        .map(|bearer| ClaimsView {
            zdr: bearer["zdr"].as_bool().expect("every bearer states zdr"),
            org_zdr: bearer["org_zdr"].as_bool(),
            waiver: (!bearer["waiver"].is_null()).then(|| WaiverView {
                version: u32::try_from(
                    bearer["waiver"]["version"]
                        .as_u64()
                        .expect("a waiver has a version"),
                )
                .expect("the version fits"),
                provider: bearer["waiver"]["provider"].as_str().expect("provider"),
                connection: bearer["waiver"]["connection"].as_str().expect("connection"),
            }),
        })
        .collect()
}

fn route_of(row: &Value) -> RouteView<'_> {
    let route = &row["inputs"]["route"];
    RouteView {
        provider: route["provider"].as_str().expect("a route has a provider"),
        connection: route["connection"].as_str(),
        explicit_zdr: route["explicit_zdr"]
            .as_bool()
            .expect("a route states explicit_zdr"),
    }
}

/// What the real resolver says about a row. No decision is built by hand.
fn resolved(row: &Value) -> Resolution<'_> {
    resolve_turn(&claims_of(row), &route_of(row))
}

/// The pre-fix rule, written as the code used to: a durable store was open to any
/// turn that was "not ZDR". A waived turn's `zdr` flag is false, so it passed.
fn the_old_not_zdr_rule_is_open(resolution: &Resolution<'_>) -> bool {
    !matches!(resolution, Resolution::Zdr { .. } | Resolution::Refuse(_))
}

#[test]
fn the_vendored_matrix_has_the_rows_this_test_was_written_for() {
    let matrix = matrix();
    assert_eq!(matrix["contract"], "zdr.posture-matrix");
    let rows = rows(&matrix);
    assert_eq!(
        rows.len(),
        ENFORCEMENT_ROWS,
        "a changed matrix must be reviewed against the durable gate and every store proof"
    );
    let ids: BTreeSet<&str> = rows.iter().map(id).collect();
    assert_eq!(ids.len(), rows.len(), "enforcement row ids are unique");
    let count = |wanted: &str| rows.iter().filter(|row| decision(row) == wanted).count();
    assert_eq!(count("refuse"), REFUSE);
    assert_eq!(count("zdr"), ZDR);
    assert_eq!(count("persistent"), PERSISTENT);
    assert_eq!(count("waived"), WAIVED);
    let persistent: BTreeSet<&str> = rows
        .iter()
        .filter(|row| decision(row) == "persistent")
        .map(id)
        .collect();
    assert_eq!(
        persistent,
        PERSISTENT_ROWS.into_iter().collect::<BTreeSet<_>>(),
        "the set of persistent rows is the set the shared gate may open for"
    );
}

/// The real resolver answers every row as the golden says, and its stored class is
/// the golden's own vocabulary (`runs.retention_class` is what the consume reads).
#[test]
fn the_real_resolver_answers_every_enforcement_row_and_names_the_stored_class() {
    let matrix = matrix();
    let vocabulary = &matrix["vocabulary"]["retention_classes"];
    let mut mismatches = Vec::new();
    for row in rows(&matrix) {
        let got = resolved(row);
        let expect_code = row["expect"]["code"].as_str().expect("a row has a code");
        if (got.decision(), got.code()) != (decision(row), expect_code) {
            mismatches.push(format!(
                "{}: the golden expects {} / {}, the resolver answers {} / {}",
                id(row),
                decision(row),
                expect_code,
                got.decision(),
                got.code()
            ));
        }
        let expected_class = (decision(row) != "refuse")
            .then(|| vocabulary[decision(row)].as_str())
            .flatten();
        if got.retention_class() != expected_class {
            mismatches.push(format!(
                "{}: class {:?}, the vocabulary says {:?}",
                id(row),
                got.retention_class(),
                expected_class
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    // The class strings the stores compare against are the vocabulary's.
    assert_eq!(vocabulary["persistent"], "persistent");
    assert_eq!(vocabulary["zdr"], "zdr");
    assert_eq!(vocabulary["waived"], "zdr_waived");
}

/// The shared gate is open for a row exactly when the matrix calls it `persistent`, for every
/// `DurableStore` it knows, and the refusal it gives is the typed one. This is the specification
/// the stores are held to; it does not call any store (see the header).
#[test]
fn the_shared_durable_gate_opens_for_the_persistent_rows_and_for_nothing_else() {
    let matrix = matrix();
    let mut opened = BTreeSet::new();
    for row in rows(&matrix) {
        let resolution = resolved(row);
        let should_open = decision(row) == "persistent";
        for store in STORES {
            let gate = refuse_unless_persistent(store, &resolution);
            assert_eq!(
                gate.is_ok(),
                should_open,
                "{} ({}): the shared gate for {store:?} must be {} for it",
                id(row),
                decision(row),
                if should_open { "open" } else { "shut" }
            );
            if should_open {
                opened.insert(id(row).to_owned());
                continue;
            }
            let refusal = gate.expect_err("a shut store answers a typed refusal");
            match decision(row) {
                // A resolver refusal is passed on unchanged: it already says why.
                "refuse" => assert_eq!(
                    refusal.code(),
                    row["expect"]["code"].as_str().expect("code"),
                    "{}: the resolver's own refusal must not be rewritten",
                    id(row)
                ),
                // A ZDR or waived turn is refused as an unsupported feature of the store.
                _ => {
                    assert_eq!(refusal.code(), code::FEATURE_NOT_SUPPORTED, "{}", id(row));
                    assert_eq!(
                        refusal.feature(),
                        Some(store.feature()),
                        "{}: the refusal names the store's feature",
                        id(row)
                    );
                }
            }
        }
    }
    assert_eq!(
        opened,
        PERSISTENT_ROWS
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        "the shared gate was opened for a row that is not persistent, or not for one that is"
    );
}

/// The class form of the shared gate (what a SQL predicate on `runs.retention_class` mirrors)
/// and the decision form cannot drift: on every row the resolver serves they agree, for every
/// store. The SQL itself is tied to the class form in `durable_loop_contract.rs`.
#[test]
fn the_class_gate_agrees_with_the_decision_gate_on_every_served_row() {
    let matrix = matrix();
    let mut served = 0_usize;
    for row in rows(&matrix) {
        let resolution = resolved(row);
        let Some(class) = resolution.retention_class() else {
            assert_eq!(
                decision(row),
                "refuse",
                "{}: only a refusal has no class",
                id(row)
            );
            continue;
        };
        served += 1;
        for store in STORES {
            assert_eq!(
                refuse_unless_persistent_class(store, class).is_ok(),
                refuse_unless_persistent(store, &resolution).is_ok(),
                "{} ({class}): the stored class and the decision disagree for {store:?}",
                id(row)
            );
        }
    }
    assert_eq!(
        served,
        ZDR + PERSISTENT + WAIVED,
        "every non-refused row was compared"
    );
    // The class gate opens for one spelling. A class the resolver never writes is not a grant.
    for store in STORES {
        assert!(refuse_unless_persistent_class(store, "persistent").is_ok());
        for other in [
            "zdr",
            "zdr_waived",
            "",
            "unknown",
            "Persistent",
            "persistent ",
        ] {
            let refusal = refuse_unless_persistent_class(store, other)
                .expect_err("only the exact class `persistent` opens a store");
            assert_eq!(refusal.feature(), Some(store.feature()), "{other:?}");
        }
    }
}

/// `loop` is minted only for a persistent row, and only with the switch on. Unlike the gates
/// above this function has a production caller (the pause site, through `resume.rs`), so this
/// one is wiring and not only specification.
#[test]
fn a_loop_approval_is_minted_for_the_persistent_rows_with_the_switch_on_and_for_nothing_else() {
    let matrix = matrix();
    for row in rows(&matrix) {
        let persistent = matches!(resolved(row), Resolution::Persistent { .. });
        assert_eq!(persistent, decision(row) == "persistent", "{}", id(row));
        assert_eq!(
            loop_mode_for_persistent(persistent, true),
            if persistent { "loop" } else { "" },
            "{} ({})",
            id(row),
            decision(row)
        );
        assert_eq!(
            loop_mode_for_persistent(persistent, false),
            "",
            "{}: the switch off mints no loop approval, whatever the row",
            id(row)
        );
    }
}

/// Anti-vacuity, part one: a hand-built waived resolution is refused by every store,
/// and a hand-built persistent one is not, so the loops above are not passing by
/// resolving everything to the same answer.
#[test]
fn a_hand_built_waived_decision_is_refused_and_a_persistent_one_is_not() {
    let waived = Resolution::Waived {
        provider: "openai-codex-subscription",
        connection: "conn_1",
        reason: Reason::ConnectionNotZdrCapable,
    };
    assert!(!waived
        .retention_class()
        .is_some_and(|class| class == "persistent"));
    let persistent = Resolution::Persistent {
        reason: Reason::OrgZdrOff,
    };
    for store in STORES {
        let refusal = refuse_unless_persistent(store, &waived)
            .expect_err("a waived run keeps no durable state");
        assert_eq!(refusal.code(), code::FEATURE_NOT_SUPPORTED);
        assert_eq!(refusal.feature(), Some(store.feature()));
        assert_eq!(refuse_unless_persistent(store, &persistent), Ok(()));
        assert!(
            refuse_unless_persistent_class(store, waived.retention_class().expect("class"))
                .is_err(),
            "the stored class of a waived run (`zdr_waived`) is refused"
        );
    }
}

/// Anti-vacuity, part two: the rule these stores used before the fix ("not ZDR") is
/// wrong on exactly the waived rows. The matrix must still contain rows that catch
/// it: if it lost them, this test fails rather than the file passing by having
/// nothing to catch.
#[test]
fn the_old_not_zdr_rule_is_caught_by_the_waived_rows_and_by_nothing_else() {
    let matrix = matrix();
    let mut caught = BTreeSet::new();
    for row in rows(&matrix) {
        let resolution = resolved(row);
        let old = the_old_not_zdr_rule_is_open(&resolution);
        let right = refuse_unless_persistent(DurableStore::Fold, &resolution).is_ok();
        if old != right {
            caught.insert(id(row).to_owned());
            assert_eq!(
                decision(row),
                "waived",
                "{}: the old rule may only be wrong on a waived row",
                id(row)
            );
            assert!(old, "{}: the old rule let it through", id(row));
        }
    }
    assert_eq!(
        caught,
        [
            "e_equal_waived_bearers",
            "e_waived_web_reader_keeps_inline_head",
            "e_waiver_on_bound_route",
        ]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        "the waived rows are what tell `not ZDR` from `persistent`"
    );
}
