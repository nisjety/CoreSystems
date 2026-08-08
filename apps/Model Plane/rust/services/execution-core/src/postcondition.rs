//! Deterministic postcondition verifiers (verevon-roadmap.md §3b, P1 item 3).
//!
//! # What this is for
//!
//! The Verified Outcome Foundation's first producer
//! ([`crate::approval_delivery_worker`]) judges an outcome *structurally*:
//! "the boundary handed back an authoritative receipt id, so call it
//! verified". That is a real signal but a weak one — it proves the provider
//! answered, not that the effect exists. A provider can return an id for
//! work it later drops, and a compromised or buggy boundary can return an id
//! for work it never did.
//!
//! A **postcondition** verifier answers the stronger question by going back
//! to the system of record and asking whether the effect is actually there.
//! The distinction is carried on the wire by
//! [`pb::VerificationResult::method`] — `"structural"` vs `"postcondition"` —
//! precisely so a reader can tell today's mechanical check from a real one,
//! and so a future stronger check is never retroactively credited to an
//! older weaker judgment.
//!
//! # The invariant that matters
//!
//! **A verifier may only ever strengthen a claim it actually checked.** When
//! it cannot reach the provider, has no verifier for the action, or gets an
//! ambiguous answer, it returns [`PostconditionOutcome::Inconclusive`] and
//! the caller keeps the existing structural judgment unchanged. An outage
//! must never read as a refutation, and an unreachable provider must never
//! silently upgrade a claim to "independently verified".
//!
//! The one direction that *does* override: a [`PostconditionOutcome::Refuted`]
//! turns a structural success into a verified failure. Catching exactly that
//! case — the boundary said yes, the system of record says no — is the whole
//! reason this layer exists (`Verevon-ai-first.md` §8 risk 7,
//! "false-success risk").

use serde_json::Value;

use crate::shipping_tools::ShippingToolsClient;

/// Booking states that mean shipping-core really holds this shipment. Kept
/// deliberately narrow: anything not named here is not treated as confirmed.
const CONFIRMED_BOOKING_STATUSES: &[&str] = &["booked", "confirmed", "in_transit", "delivered"];

/// Booking states that positively contradict a claimed successful booking.
const REFUTING_BOOKING_STATUSES: &[&str] = &["cancelled", "canceled", "failed", "rejected"];

/// Keys whose *string* value is treated as an object's own identity. Matching
/// only these — never a substring of arbitrary text — is what stops an id
/// quoted inside a message body from confirming that the message exists.
const IDENTIFIER_KEYS: &[&str] = &["ts", "id", "message_id", "messageId", "provider_message_id"];

/// The three-valued judgment a postcondition verifier can reach. Three, not
/// two, because "I could not tell" must be representable — collapsing it into
/// either success or failure is how false claims get made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostconditionOutcome {
    /// The system of record confirms the effect exists as claimed.
    Confirmed { detail: String },
    /// The system of record positively contradicts the claim.
    Refuted { detail: String },
    /// No independent judgment was reached. The caller must fall back to its
    /// existing (structural) judgment rather than treat this as either
    /// outcome.
    Inconclusive { detail: String },
}

impl PostconditionOutcome {
    /// The `method` string for a [`pb::VerificationResult`] built from this
    /// outcome. `Inconclusive` deliberately has none: it never produces a
    /// postcondition-method result at all.
    #[must_use]
    pub fn method(&self) -> Option<&'static str> {
        match self {
            Self::Confirmed { .. } | Self::Refuted { .. } => Some("postcondition"),
            Self::Inconclusive { .. } => None,
        }
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::Confirmed { detail }
            | Self::Refuted { detail }
            | Self::Inconclusive { detail } => detail,
        }
    }
}

/// Verify a claimed shipment booking against shipping-core, the system of
/// record for bookings.
///
/// This is the pilot action of `verevon-roadmap.md` P1 item 3 — the same
/// action whose live `verified_success` the roadmap's §9.4 entry cites. The
/// structural check there only established that a `booking_id` came back;
/// this re-reads that booking from shipping-core and judges its actual
/// state.
///
/// A 404 is a refutation, not an error: `booking_id` came from
/// shipping-core's own create path, so shipping-core not having it is a
/// genuine contradiction rather than a lookup miss. Transport failures stay
/// inconclusive.
pub async fn verify_shipment_booking(
    client: &ShippingToolsClient,
    booking_id: &str,
    org_id: &str,
) -> PostconditionOutcome {
    match client.get_booking(booking_id, org_id).await {
        Ok(Some(record)) => judge_booking_record(booking_id, &record),
        Ok(None) => PostconditionOutcome::Refuted {
            detail: format!(
                "shipping-core has no booking {booking_id} for this organization, \
                 though the booking call returned that id"
            ),
        },
        Err(error) => PostconditionOutcome::Inconclusive {
            detail: format!("could not read booking {booking_id} back: {error}"),
        },
    }
}

/// Judge one shipping-core booking record. Split out from the request so the
/// judgment rules are testable without a live shipping-core.
#[must_use]
pub fn judge_booking_record(booking_id: &str, record: &Value) -> PostconditionOutcome {
    let status = record
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if status.is_empty() {
        return PostconditionOutcome::Inconclusive {
            detail: format!("booking {booking_id} exists but reports no status"),
        };
    }
    if REFUTING_BOOKING_STATUSES.contains(&status.as_str()) {
        return PostconditionOutcome::Refuted {
            detail: format!("shipping-core reports booking {booking_id} as {status}"),
        };
    }
    if !CONFIRMED_BOOKING_STATUSES.contains(&status.as_str()) {
        // An in-between state (e.g. `pending`) or a status this build does
        // not know about. Neither confirms nor contradicts — and guessing
        // would be exactly the overclaim this module exists to prevent.
        return PostconditionOutcome::Inconclusive {
            detail: format!(
                "shipping-core reports booking {booking_id} as {status}, \
                 which is neither a confirmed nor a failed state"
            ),
        };
    }

    let tracking_no = record
        .get("tracking_no")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if tracking_no.is_empty() {
        return PostconditionOutcome::Inconclusive {
            detail: format!(
                "shipping-core reports booking {booking_id} as {status} but carries \
                 no tracking number, so the carrier handoff is unconfirmed"
            ),
        };
    }

    PostconditionOutcome::Confirmed {
        detail: format!(
            "shipping-core independently reports booking {booking_id} as {status} \
             with tracking number {tracking_no}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Provider actions (execute_provider_action)
// ---------------------------------------------------------------------------

/// What a read-back is *capable of* proving. This distinction is the heart of
/// honest verification and is deliberately in the type system rather than in
/// a comment.
///
/// Shipping's `GET /api/bookings/{id}` asks about one specific effect, so its
/// answer is total: absence is a genuine refutation. A provider *listing*
/// (Slack's `conversations.history`, Gmail's message list) is bounded — it
/// returns a window, not the universe. Finding the receipt in that window
/// proves the effect exists; **not** finding it proves nothing at all,
/// because the effect may simply lie past the window. Treating that absence
/// as a refutation would manufacture false failures, which is exactly as
/// harmful as the false successes this layer exists to catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadBackStrength {
    /// A direct lookup of the exact effect by its own id. Absence refutes.
    AuthoritativeById,
    /// A bounded listing. Presence confirms; absence is inconclusive.
    BoundedList,
}

/// One field the write asked to change, and the value it asked for,
/// canonicalized to a string so a JSON number and its textual form compare
/// equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldExpectation {
    pub field: String,
    pub expected: String,
}

/// How a read-back reaches its verdict.
///
/// The split exists because the two cases are not interchangeable, and using
/// the wrong one is the most plausible way to manufacture a false confirmation
/// in this module. A *create* is proven by its object existing. A *mutation*
/// is not: the object existed before the write, so finding it says nothing.
/// Putting that in the type system means a mutation cannot accidentally be
/// wired to the existence check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadBackJudgment {
    /// Confirm by finding the write's receipt id in the read-back.
    IdentifierPresence,
    /// Confirm by comparing the values the write asked for against the values
    /// the system of record now reports.
    FieldValues {
        /// Identifies the mutated object *inside* the read-back — the receipt
        /// id for a listing, the target id for a by-id read. Never a value the
        /// caller supplies independently of the write.
        locator: String,
        /// Non-empty by construction: a field comparison with nothing to
        /// compare would silently degenerate into an existence check, which is
        /// exactly what a mutation must not use.
        expectations: Vec<FieldExpectation>,
    },
}

/// A planned read-back: which frozen-surface operation to call, with what
/// params, what its answer is worth, and how to judge it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderReadBack {
    pub operation: &'static str,
    pub params: Value,
    pub strength: ReadBackStrength,
    pub judgment: ReadBackJudgment,
}

/// Canonicalize a JSON scalar for comparison. Returns `None` for objects and
/// arrays: a nested value's textual shape depends on provider normalization,
/// and comparing those would produce refutations that reflect formatting
/// rather than whether the mutation applied.
fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// The scalar fields a write asked to set, taken from its own request body.
///
/// Only top-level scalars: a nested object in the request has no reliable
/// correspondence to how the provider reports it back.
fn scalar_expectations(value: &Value) -> Vec<FieldExpectation> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut fields: Vec<FieldExpectation> = map
        .iter()
        .filter_map(|(field, raw)| {
            let expected = scalar_to_string(raw)?;
            if expected.is_empty() {
                return None;
            }
            Some(FieldExpectation {
                field: field.clone(),
                expected,
            })
        })
        .collect();
    // Deterministic order so a plan is reproducible and testable.
    fields.sort_by(|a, b| a.field.cmp(&b.field));
    fields
}

/// The scalar fields a RestLi partial update asked to set.
///
/// LinkedIn's update body is `{"patch": {"$set": {…}}}`; anything else (a
/// nested per-field patch, a `$delete`) yields nothing, which correctly makes
/// the planner return `None` rather than a comparison with no content.
fn restli_set_expectations(body: &Value) -> Vec<FieldExpectation> {
    body.get("patch")
        .and_then(|patch| patch.get("$set"))
        .map(scalar_expectations)
        .unwrap_or_default()
}

/// Plan the read-back for a completed provider write, or `None` when this
/// build has no verifier for that operation.
///
/// Every operation named here is already in the **frozen** actions-surface
/// contract (`docs/actions-surface-operations.md`); this adds no provider
/// capability, it only re-uses existing reads to check existing writes.
/// Returning `None` is the correct, common answer — it leaves the structural
/// judgment untouched rather than guessing.
///
/// Not verifiable by design, and worth stating rather than leaving to a
/// future reader to rediscover:
/// - Microsoft `mail.send` returns `202 Accepted` with **no id at all**, so
///   the dispatcher never even reaches a `Completed` disposition for it (the
///   receipt heuristic finds nothing and it fails closed as
///   `invalid_continuation`). There is nothing to match a read-back against.
/// - GitHub `issues.create` is the same class for a less obvious reason: it
///   answers with `id` and `number` as JSON **numbers**, and both
///   integration-corev2's `actionProviderMessageIDAtDepth` and its Rust port
///   in `integration_tools` match only string values. No receipt is extracted,
///   so `issues.create` never reaches `Completed` either — a read-back against
///   the `issues` listing would be unreachable code, not a missing verifier.
/// - GitHub `issues.update` inherits exactly the same numeric-id problem as
///   `issues.create`, so it is out of reach for the same reason — not because
///   the `issues` listing could not be compared against.
///
/// Mutations use [`ReadBackJudgment::FieldValues`] instead: the read-back must
/// show the values the write asked for, because the object's mere existence
/// predates the write. Their reachability is gated on something separate from
/// this planner — see [`verify_provider_action`].
#[must_use]
pub fn plan_provider_read_back(
    operation: &str,
    params: &Value,
    body: &Value,
    receipt_id: &str,
) -> Option<ProviderReadBack> {
    let receipt_id = receipt_id.trim();
    let param = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    match operation.trim() {
        // Slack chat.postMessage returns the message `ts`; conversations.history
        // lists a bounded window of the same channel's messages.
        "message.send" | "slack.message.send" => {
            let channel = body.get("channel").and_then(Value::as_str)?.trim();
            if channel.is_empty() {
                return None;
            }
            Some(ProviderReadBack {
                operation: "messages.list",
                params: serde_json::json!({ "channel": channel, "limit": 100 }),
                strength: ReadBackStrength::BoundedList,
                judgment: ReadBackJudgment::IdentifierPresence,
            })
        }
        // Gmail send returns the created message id; the message list returns
        // a bounded window that a just-sent message should fall inside.
        "gmail.send" | "google.gmail.send" => Some(ProviderReadBack {
            operation: "gmail.messages",
            params: serde_json::json!({ "maxResults": 100 }),
            strength: ReadBackStrength::BoundedList,
            judgment: ReadBackJudgment::IdentifierPresence,
        }),
        // LinkedIn creates answer with an empty body plus an `x-restli-id`
        // header, which integration-corev2's `doJSON` materializes as
        // `result.id` — so these do produce a receipt to match against.
        //
        // An event is fetchable by its own id, which makes this the first
        // authoritative provider read-back: a 200 means the provider itself
        // found the event, because a bad id answers non-2xx (and therefore
        // reads as inconclusive, not as a refutation — see below).
        "events.create" | "linkedin.events.create" if !receipt_id.is_empty() => {
            Some(ProviderReadBack {
                operation: "events.get",
                params: serde_json::json!({ "eventId": receipt_id }),
                strength: ReadBackStrength::AuthoritativeById,
                judgment: ReadBackJudgment::IdentifierPresence,
            })
        }
        // Posts have no by-id read on the frozen surface, only an author-scoped
        // listing — bounded, so presence confirms and absence proves nothing.
        // The author URN comes from the write's own body (the Posts API
        // requires it), not from anything the caller could redirect.
        "posts.create" | "linkedin.posts.create" => {
            let author = body.get("author").and_then(Value::as_str)?.trim();
            if author.is_empty() {
                return None;
            }
            Some(ProviderReadBack {
                operation: "posts.list",
                params: serde_json::json!({ "author": author, "count": 100 }),
                strength: ReadBackStrength::BoundedList,
                judgment: ReadBackJudgment::IdentifierPresence,
            })
        }
        // The Meta family returns Graph node ids as JSON strings, and each of
        // these creates has a matching node fetch keyed on exactly that id.
        // None of them need a param from the write — the receipt is the whole
        // lookup key — so a verifier here cannot be pointed anywhere the write
        // did not already go.
        "live.create" | "facebook.live.create" if !receipt_id.is_empty() => {
            Some(ProviderReadBack {
                operation: "live.get",
                params: serde_json::json!({ "liveVideoId": receipt_id }),
                strength: ReadBackStrength::AuthoritativeById,
                judgment: ReadBackJudgment::IdentifierPresence,
            })
        }
        // Confirms the *container* exists, which is what this write creates —
        // publishing it is a separate approved action with its own receipt.
        "instagram.media.create" if !receipt_id.is_empty() => Some(ProviderReadBack {
            operation: "instagram.media.status",
            params: serde_json::json!({ "creationId": receipt_id }),
            strength: ReadBackStrength::AuthoritativeById,
            judgment: ReadBackJudgment::IdentifierPresence,
        }),
        "threads.container.create" if !receipt_id.is_empty() => Some(ProviderReadBack {
            operation: "threads.container.status",
            params: serde_json::json!({ "creationId": receipt_id }),
            strength: ReadBackStrength::AuthoritativeById,
            judgment: ReadBackJudgment::IdentifierPresence,
        }),

        // --- Mutations: judged on values, never on existence ----------------
        //
        // Each of these reads the object back by the id the *write itself*
        // targeted (from its params), so the verifier looks at exactly the
        // object the approval authorized and nowhere else. An update with
        // nothing comparable in its patch plans no read at all rather than
        // degenerating into an existence check.
        "events.update" | "linkedin.events.update" => {
            let event_id = param("eventId")?;
            let expectations = restli_set_expectations(body);
            if expectations.is_empty() {
                return None;
            }
            Some(ProviderReadBack {
                operation: "events.get",
                params: serde_json::json!({ "eventId": event_id }),
                strength: ReadBackStrength::AuthoritativeById,
                judgment: ReadBackJudgment::FieldValues {
                    locator: event_id.to_owned(),
                    expectations,
                },
            })
        }
        "ads.campaign.update" | "linkedin.ads.campaign.update" => {
            let account_id = param("accountId")?;
            let campaign_id = param("campaignId")?;
            let expectations = restli_set_expectations(body);
            if expectations.is_empty() {
                return None;
            }
            Some(ProviderReadBack {
                operation: "ads.campaign",
                params: serde_json::json!({
                    "accountId": account_id,
                    "campaignId": campaign_id,
                }),
                strength: ReadBackStrength::AuthoritativeById,
                judgment: ReadBackJudgment::FieldValues {
                    locator: campaign_id.to_owned(),
                    expectations,
                },
            })
        }
        // The expected value here comes from the *operation*, not the body:
        // these lifecycle calls carry no body at all, and what they assert is
        // named by which endpoint was called. Okta reports user status as an
        // uppercase enum on each element of the (bounded) user listing, so the
        // user is located in that window first and only then compared.
        "user.suspend" | "okta.user.suspend" | "user.activate" | "okta.user.activate" => {
            let user_id = param("userId")?;
            let expected_status = if operation.trim().ends_with("suspend") {
                "SUSPENDED"
            } else {
                "ACTIVE"
            };
            Some(ProviderReadBack {
                operation: "users",
                params: serde_json::json!({ "limit": 200 }),
                strength: ReadBackStrength::BoundedList,
                judgment: ReadBackJudgment::FieldValues {
                    locator: user_id.to_owned(),
                    expectations: vec![FieldExpectation {
                        field: "status".to_owned(),
                        expected: expected_status.to_owned(),
                    }],
                },
            })
        }
        // An upsert either created or changed the product, so existence proves
        // nothing either way; the retailer fields it set are what distinguish
        // the two. Located by receipt inside the catalog's bounded listing.
        "catalog.product.upsert" if !receipt_id.is_empty() => {
            let catalog_id = param("catalogId")?;
            let expectations = scalar_expectations(body);
            if expectations.is_empty() {
                return None;
            }
            Some(ProviderReadBack {
                operation: "catalog.products",
                params: serde_json::json!({ "catalogId": catalog_id, "limit": 100 }),
                strength: ReadBackStrength::BoundedList,
                judgment: ReadBackJudgment::FieldValues {
                    locator: receipt_id.to_owned(),
                    expectations,
                },
            })
        }
        _ => None,
    }
}

/// Judge a completed read-back, by whichever rule the plan declared.
///
/// The asymmetry enforced here is the whole point: presence always confirms,
/// but absence only refutes when the read-back was authoritative for that
/// exact id.
#[must_use]
pub fn judge_provider_read_back(
    plan: &ProviderReadBack,
    receipt_id: &str,
    result: &Value,
) -> PostconditionOutcome {
    match &plan.judgment {
        ReadBackJudgment::IdentifierPresence => judge_identifier_presence(plan, receipt_id, result),
        ReadBackJudgment::FieldValues {
            locator,
            expectations,
        } => judge_field_values(plan, locator, expectations, result),
    }
}

fn judge_identifier_presence(
    plan: &ProviderReadBack,
    receipt_id: &str,
    result: &Value,
) -> PostconditionOutcome {
    let receipt_id = receipt_id.trim();
    if receipt_id.is_empty() {
        return PostconditionOutcome::Inconclusive {
            detail: "no provider receipt id to match against".to_owned(),
        };
    }

    if response_contains_identifier(result, receipt_id, 0) {
        return PostconditionOutcome::Confirmed {
            detail: format!(
                "provider's own {} read independently lists {receipt_id}",
                plan.operation
            ),
        };
    }

    match plan.strength {
        ReadBackStrength::AuthoritativeById => PostconditionOutcome::Refuted {
            detail: format!(
                "provider's own {} lookup does not contain {receipt_id}",
                plan.operation
            ),
        },
        ReadBackStrength::BoundedList => PostconditionOutcome::Inconclusive {
            detail: format!(
                "{receipt_id} was not in the bounded {} window, which neither \
                 confirms nor contradicts the write",
                plan.operation
            ),
        },
    }
}

/// Judge a mutation by comparing the values it asked for against the values
/// the provider now reports.
///
/// # Why a mismatch is allowed to refute
///
/// This is the one place the module compares content rather than identity, and
/// content comparison is how false *failures* get manufactured — a provider
/// that trims, re-cases or normalizes a stored value would otherwise read as
/// "the mutation did not apply". Three rules keep that from happening:
/// scalars only (a nested value's shape is the provider's choice), comparison
/// after trimming and case-folding, and a field the read-back does not report
/// at all counts as unknown rather than as a mismatch. What survives all three
/// is a field the provider *did* report, with a value that is genuinely not
/// the one requested — which is a real contradiction of the write's success.
fn judge_field_values(
    plan: &ProviderReadBack,
    locator: &str,
    expectations: &[FieldExpectation],
    result: &Value,
) -> PostconditionOutcome {
    let scope = locate_object(result, locator, 0);
    let scope = match (scope, plan.strength) {
        (Some(object), _) => object,
        // A by-id read *is* the object, so failing to spot the id inside it is
        // far more likely to mean the provider reports ids in a shape this
        // search does not recognize than that it answered about something
        // else. Comparing the fields against the whole response is the safe
        // reading: a genuinely wrong object still refutes, on the mismatch,
        // while an unrecognized id shape costs nothing.
        (None, ReadBackStrength::AuthoritativeById) => result,
        (None, ReadBackStrength::BoundedList) => {
            return PostconditionOutcome::Inconclusive {
                detail: format!(
                    "{locator} was not in the bounded {} window, so its fields \
                     could not be compared",
                    plan.operation
                ),
            };
        }
    };

    let mut matched: Vec<&str> = Vec::new();
    let mut mismatched: Vec<String> = Vec::new();
    let mut unknown: Vec<&str> = Vec::new();
    for expectation in expectations {
        let reported = field_values(scope, &expectation.field, 0);
        if reported.is_empty() {
            unknown.push(&expectation.field);
        } else if reported
            .iter()
            .any(|value| value.eq_ignore_ascii_case(expectation.expected.trim()))
        {
            matched.push(&expectation.field);
        } else {
            mismatched.push(format!(
                "{} is {} but the write set it to {}",
                expectation.field,
                reported.join("/"),
                expectation.expected
            ));
        }
    }

    if !mismatched.is_empty() {
        return PostconditionOutcome::Refuted {
            detail: format!(
                "provider's own {} read contradicts the write: {}",
                plan.operation,
                mismatched.join("; ")
            ),
        };
    }
    if matched.is_empty() {
        return PostconditionOutcome::Inconclusive {
            detail: format!(
                "provider's own {} read reports none of the changed fields ({}), \
                 so the mutation could not be checked",
                plan.operation,
                unknown.join(", ")
            ),
        };
    }
    PostconditionOutcome::Confirmed {
        detail: format!(
            "provider's own {} read independently reports {} as the write set them{}",
            plan.operation,
            matched.join(", "),
            if unknown.is_empty() {
                String::new()
            } else {
                format!(" ({} not reported back)", unknown.join(", "))
            }
        ),
    }
}

/// Find the object inside a read-back that carries `locator` as its own
/// identifier — the one element of a listing, or the object a by-id read
/// returned. Depth-bounded like the identifier search.
///
/// Unlike [`response_contains_identifier`], which mirrors the receipt
/// extractor's string-only rule, this accepts a numeric id as well: providers
/// that report ids as JSON numbers (GitHub, and Graph in places) are common,
/// and failing to locate an object on that basis would push a perfectly good
/// mutation toward a verdict it has not earned.
fn locate_object<'a>(value: &'a Value, locator: &str, depth: u8) -> Option<&'a Value> {
    if depth > 6 {
        return None;
    }
    match value {
        Value::Object(map) => {
            for key in IDENTIFIER_KEYS {
                if map
                    .get(*key)
                    .and_then(scalar_to_string)
                    .is_some_and(|candidate| candidate == locator)
                {
                    return Some(value);
                }
            }
            map.values()
                .find_map(|nested| locate_object(nested, locator, depth + 1))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| locate_object(item, locator, depth + 1)),
        _ => None,
    }
}

/// Every scalar value reported at `field` within this object, canonicalized.
///
/// Searches nested objects because a provider may report a field one level
/// down (Snap and Graph both wrap payloads); an array of objects is *not*
/// descended into, since those are sibling records rather than this object's
/// own fields.
fn field_values(value: &Value, field: &str, depth: u8) -> Vec<String> {
    if depth > 4 {
        return Vec::new();
    }
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    if let Some(reported) = map.get(field).and_then(scalar_to_string) {
        found.push(reported);
    }
    for nested in map.values() {
        if nested.is_object() {
            found.extend(field_values(nested, field, depth + 1));
        }
    }
    found
}

/// Does this response carry `identifier` as an actual identifier value?
///
/// Matches only on string values at identifier-shaped keys, never on a
/// substring of arbitrary text — a receipt id appearing inside a message
/// *body* would say nothing about whether the message exists, and matching it
/// would produce a false `Confirmed`.
fn response_contains_identifier(value: &Value, identifier: &str, depth: u8) -> bool {
    if depth > 6 {
        return false;
    }
    match value {
        Value::Object(map) => {
            for key in IDENTIFIER_KEYS {
                if map
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(|candidate| candidate.trim() == identifier)
                {
                    return true;
                }
            }
            map.values()
                .any(|nested| response_contains_identifier(nested, identifier, depth + 1))
        }
        Value::Array(items) => items
            .iter()
            .any(|item| response_contains_identifier(item, identifier, depth + 1)),
        _ => false,
    }
}

/// Verify a claimed provider write by re-reading the provider through an
/// existing frozen-surface read operation.
///
/// Returns `Inconclusive` — leaving the structural judgment intact — when no
/// verifier exists for the operation or the read itself fails.
///
/// Note what a failed read costs an authoritative by-id lookup. integration-corev2
/// surfaces a provider 404 as `502 action_failed` carrying the upstream status
/// inside a message string, so a genuine "it does not exist" is indistinguishable
/// here from an outage without parsing that prose. It is therefore treated as an
/// outage: `AuthoritativeById` confirms in practice and refutes only in the
/// narrow case of a 200 whose body lacks the id. Deciding a refutation by
/// string-matching an error message would be exactly the weak evidence this
/// module exists to refuse.
///
/// The read-back is also subject to the connection's own capabilities: a
/// connection granted `social.post.write` but not `social.post.read` will fail
/// the listing and stay inconclusive. That is the safe direction, but it means
/// a verifier can be silently unexercised in production — the connection needs
/// the *read* capability for verification to be worth anything.
///
/// # The receipt gate, which matters most for mutations
///
/// This is only ever reached from a `Completed` disposition, and a provider
/// action reaches `Completed` only when the receipt heuristic found a **string**
/// id in the write's response. That is a poor fit for mutations: the object's
/// id was known *before* the call, and providers commonly answer a successful
/// update with `204 No Content` — for which integration-corev2's `doJSON`
/// returns `{"ok": true}` and, unlike the empty-body-with-200 path, does not
/// even attach `x-restli-id`. Such a write is currently recorded as
/// `invalid_continuation`, a terminal failure, despite the same code path
/// noting that integration-corev2 durably completed it.
///
/// The mutation verifiers below therefore key on the object id the write
/// *targeted* rather than on a receipt, so they are correct whenever they run —
/// but until the disposition rule stops requiring a receipt for actions whose
/// identity is already known, the ones whose providers answer 204 will not run.
/// That gap is a false *failure*, not a false success, so it is recorded here
/// rather than papered over by loosening what counts as a verified outcome.
pub async fn verify_provider_action(
    client: &crate::integration_tools::IntegrationActionsClient,
    org_id: &str,
    connection_id: &str,
    operation: &str,
    params: &Value,
    body: &Value,
    receipt_id: &str,
) -> PostconditionOutcome {
    if receipt_id.trim().is_empty() {
        return PostconditionOutcome::Inconclusive {
            detail: "no provider receipt id to verify against".to_owned(),
        };
    }
    let Some(plan) = plan_provider_read_back(operation, params, body, receipt_id) else {
        return PostconditionOutcome::Inconclusive {
            detail: format!("no postcondition verifier is defined for {operation}"),
        };
    };
    match client
        .read_action_json(org_id, connection_id, plan.operation, plan.params.clone())
        .await
    {
        Ok(result) => judge_provider_read_back(&plan, receipt_id, &result),
        Err(error) => PostconditionOutcome::Inconclusive {
            detail: format!("could not read {} back: {error}", plan.operation),
        },
    }
}

// ---------------------------------------------------------------------------
// Browser procedures
// ---------------------------------------------------------------------------

/// The independently-checkable facts about a finished browser procedure.
///
/// Deliberately excludes the loop's `summary` and `status`: those are the
/// agent's own account of what happened, and checking a claim against itself
/// verifies nothing. Only the observation the browser actually produced, and
/// whether durable evidence for it exists, count here.
#[derive(Debug, Clone, Copy)]
pub struct BrowserProcedureEvidence<'a> {
    /// Whether the loop ended in an abort (cancelled, approval denied, …).
    pub aborted: bool,
    /// `status` of the final observation: `success` | `failed` | `timeout` |
    /// `blocked`. `None` when the procedure produced no observation at all.
    pub final_status: Option<&'a str>,
    pub final_page_title: &'a str,
    pub final_page_text: &'a str,
    /// True when a screenshot or DOM snapshot was retained, so a human can
    /// actually inspect the claim.
    pub has_durable_evidence: bool,
    /// Zero-data-retention: Quarry retained no page content, so there is
    /// nothing to inspect and nothing to match against.
    pub zdr: bool,
    /// The substring that made the loop stop.
    pub stop_criteria: &'a str,
    /// A success condition declared independently of `stop_criteria`.
    ///
    /// Carried from `browser_agent::PlanConfig::postcondition`. `None` when the
    /// caller declared none, which leaves that procedure **refutable but never
    /// confirmable** — see [`judge_browser_procedure`].
    pub declared_postcondition: Option<&'a str>,
}

/// Judge a finished browser procedure against its own final observation.
///
/// # Why this cannot simply re-check `stop_criteria`
///
/// `stop_criteria` is the substring that *caused* the loop to stop. Testing
/// the final page for it would always succeed and would prove nothing — it
/// checks the agent against its own stop decision. That circularity is the
/// browser equivalent of trusting a provider's own "success" claim, and it is
/// refused explicitly below rather than quietly producing a confident-looking
/// `Confirmed`.
///
/// # What it can decide today
///
/// - **Refuted** — the final observation is a `failed`/`timeout`/`blocked`
///   result, or the procedure aborted. This is real value: a loop can report
///   `Completed` while the last thing that actually happened was a failure,
///   and that mismatch is exactly the false-success case.
/// - **Refuted** — a distinct postcondition was declared and the final page
///   does not contain it.
/// - **Confirmed** — a distinct postcondition was declared, the final
///   observation succeeded, the page contains it, and durable evidence was
///   retained. Implemented and tested, but unreachable until `PlanConfig`
///   carries a postcondition.
/// - **Inconclusive** — everything else, each with a specific reason.
#[must_use]
pub fn judge_browser_procedure(evidence: &BrowserProcedureEvidence<'_>) -> PostconditionOutcome {
    if evidence.aborted {
        return PostconditionOutcome::Refuted {
            detail: "the browser procedure aborted before reaching its goal".to_owned(),
        };
    }
    let Some(status) = evidence.final_status.map(str::trim) else {
        return PostconditionOutcome::Inconclusive {
            detail: "the browser procedure produced no observation to judge".to_owned(),
        };
    };
    if status != "success" {
        return PostconditionOutcome::Refuted {
            detail: format!("the browser procedure's final observation was {status}"),
        };
    }

    let Some(postcondition) = evidence
        .declared_postcondition
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return PostconditionOutcome::Inconclusive {
            detail: "no postcondition was declared independently of the loop's stop criteria, \
                     so the final page cannot be checked against anything the agent did not \
                     itself decide"
                .to_owned(),
        };
    };
    if postcondition == evidence.stop_criteria.trim() {
        return PostconditionOutcome::Inconclusive {
            detail: "the declared postcondition is the loop's own stop criteria; re-checking it \
                     would verify the agent against its own stop decision"
                .to_owned(),
        };
    }

    if evidence.zdr {
        return PostconditionOutcome::Inconclusive {
            detail: "Zero Data Retention kept no page content, so the postcondition cannot be \
                     checked against the final page"
                .to_owned(),
        };
    }

    let matched = evidence.final_page_title.contains(postcondition)
        || evidence.final_page_text.contains(postcondition);
    if !matched {
        return PostconditionOutcome::Refuted {
            detail: format!(
                "the final page does not contain the declared postcondition {postcondition:?}"
            ),
        };
    }
    if !evidence.has_durable_evidence {
        return PostconditionOutcome::Inconclusive {
            detail: "the final page matched the declared postcondition, but no screenshot or \
                     DOM snapshot was retained, so the claim is not inspectable"
                .to_owned(),
        };
    }

    PostconditionOutcome::Confirmed {
        detail: format!(
            "the final observation succeeded, contains the declared postcondition \
             {postcondition:?}, and retained inspectable evidence"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_booked_shipment_with_tracking_is_confirmed() {
        let outcome = judge_booking_record(
            "bk_1",
            &json!({ "status": "booked", "tracking_no": "LC652849244NO" }),
        );
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
        assert_eq!(outcome.method(), Some("postcondition"));
        assert!(outcome.detail().contains("LC652849244NO"));
    }

    #[test]
    fn a_cancelled_booking_is_refuted() {
        let outcome = judge_booking_record("bk_1", &json!({ "status": "cancelled" }));
        assert!(matches!(outcome, PostconditionOutcome::Refuted { .. }));
        assert_eq!(outcome.method(), Some("postcondition"));
    }

    #[test]
    fn a_pending_booking_is_inconclusive_not_refuted() {
        let outcome = judge_booking_record("bk_1", &json!({ "status": "pending" }));
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "an in-flight state must not be reported as a failure"
        );
        assert_eq!(outcome.method(), None);
    }

    #[test]
    fn an_unknown_status_is_inconclusive_never_confirmed() {
        let outcome = judge_booking_record(
            "bk_1",
            &json!({ "status": "some_future_state", "tracking_no": "X1" }),
        );
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "a status this build does not know must never read as confirmed"
        );
    }

    #[test]
    fn a_booked_shipment_without_tracking_is_inconclusive() {
        let outcome = judge_booking_record("bk_1", &json!({ "status": "booked" }));
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
        assert!(outcome.detail().contains("no tracking number"));
    }

    #[test]
    fn a_record_without_a_status_is_inconclusive() {
        let outcome = judge_booking_record("bk_1", &json!({ "tracking_no": "X1" }));
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
    }

    #[test]
    fn status_matching_is_case_and_whitespace_insensitive() {
        let outcome = judge_booking_record(
            "bk_1",
            &json!({ "status": "  BOOKED ", "tracking_no": "X1" }),
        );
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
    }

    #[test]
    fn only_a_judged_outcome_carries_the_postcondition_method() {
        assert_eq!(
            PostconditionOutcome::Confirmed {
                detail: String::new()
            }
            .method(),
            Some("postcondition")
        );
        assert_eq!(
            PostconditionOutcome::Refuted {
                detail: String::new()
            }
            .method(),
            Some("postcondition")
        );
        assert_eq!(
            PostconditionOutcome::Inconclusive {
                detail: String::new()
            }
            .method(),
            None,
            "an inconclusive check must never claim it performed a postcondition verification"
        );
    }

    // --- Provider-action read-backs ----------------------------------------

    #[test]
    fn slack_send_plans_a_channel_scoped_history_read() {
        let plan = plan_provider_read_back(
            "slack.message.send",
            &json!({}),
            &json!({ "channel": "C123", "text": "hello" }),
            "1700000000.000200",
        )
        .expect("slack message.send has a verifier");
        assert_eq!(plan.operation, "messages.list");
        assert_eq!(plan.params["channel"], json!("C123"));
        assert_eq!(plan.strength, ReadBackStrength::BoundedList);
    }

    #[test]
    fn slack_send_without_a_channel_has_no_plan() {
        assert!(plan_provider_read_back(
            "slack.message.send",
            &json!({}),
            &json!({ "text": "hi" }),
            "ts-1"
        )
        .is_none());
        assert!(plan_provider_read_back(
            "slack.message.send",
            &json!({}),
            &json!({ "channel": "  " }),
            "ts-1"
        )
        .is_none());
    }

    #[test]
    fn an_operation_with_no_verifier_plans_nothing() {
        // Including writes that genuinely cannot be verified: Microsoft
        // mail.send returns no id at all, and GitHub issues.create returns its
        // id as a JSON number, which the receipt extractor (string-only, on
        // both sides of the boundary) never picks up.
        for operation in [
            "mail.send",
            "microsoft.mail.send",
            "issues.create",
            "github.issues.create",
            "issues.comment.create",
            "whatever",
        ] {
            assert!(
                plan_provider_read_back(operation, &json!({}), &json!({}), "receipt-1").is_none(),
                "{operation} must not claim a verifier it does not have"
            );
        }
    }

    #[test]
    fn a_mutation_is_never_paired_with_a_read_that_would_find_it_anyway() {
        // Each of these has a usable read on the frozen surface, and pairing
        // it with the identifier check would be the subtlest false confirmation
        // available here: the object existed before the write, so finding it
        // says nothing about whether the mutation applied. They are verified —
        // but only ever on values.
        let patch = json!({ "patch": { "$set": { "name": "New name" } } });
        for (operation, params, body) in [
            ("events.update", json!({ "eventId": "ev-1" }), patch.clone()),
            (
                "linkedin.events.update",
                json!({ "eventId": "ev-1" }),
                patch.clone(),
            ),
            (
                "ads.campaign.update",
                json!({ "accountId": "acc-1", "campaignId": "cmp-1" }),
                patch.clone(),
            ),
            ("user.suspend", json!({ "userId": "00u1" }), json!({})),
            ("okta.user.activate", json!({ "userId": "00u1" }), json!({})),
            (
                "catalog.product.upsert",
                json!({ "catalogId": "cat-1" }),
                json!({ "retailer_id": "SKU-1" }),
            ),
        ] {
            let plan = plan_provider_read_back(operation, &params, &body, "receipt-1")
                .unwrap_or_else(|| panic!("{operation} has a field-level verifier"));
            assert!(
                matches!(plan.judgment, ReadBackJudgment::FieldValues { .. }),
                "{operation} mutates an existing object, so it must never be judged \
                 on identifier presence"
            );
        }
        // GitHub is excluded for a different reason entirely: its ids come back
        // as JSON numbers, so no receipt is ever extracted and the write never
        // reaches a Completed disposition to verify.
        for operation in ["issues.update", "github.issues.update"] {
            assert!(
                plan_provider_read_back(
                    operation,
                    &json!({ "owner": "o", "repo": "r", "issueNumber": "5" }),
                    &json!({ "state": "closed" }),
                    "receipt-1"
                )
                .is_none(),
                "{operation} produces no receipt, so a verifier would be unreachable"
            );
        }
    }

    #[test]
    fn linkedin_event_create_plans_an_authoritative_by_id_lookup() {
        let plan = plan_provider_read_back(
            "linkedin.events.create",
            &json!({}),
            &json!({}),
            "  7212345  ",
        )
        .expect("events.create has a verifier");
        assert_eq!(plan.operation, "events.get");
        assert_eq!(
            plan.params["eventId"],
            json!("7212345"),
            "the receipt is the whole lookup key, trimmed"
        );
        assert_eq!(plan.strength, ReadBackStrength::AuthoritativeById);
    }

    #[test]
    fn linkedin_post_create_plans_an_author_scoped_listing_from_its_own_body() {
        let plan = plan_provider_read_back(
            "posts.create",
            &json!({}),
            &json!({ "author": "urn:li:organization:42", "commentary": "hi" }),
            "urn:li:share:999",
        )
        .expect("posts.create has a verifier");
        assert_eq!(plan.operation, "posts.list");
        assert_eq!(plan.params["author"], json!("urn:li:organization:42"));
        assert_eq!(
            plan.strength,
            ReadBackStrength::BoundedList,
            "an author listing is a window, so absence must stay inconclusive"
        );
    }

    #[test]
    fn linkedin_post_create_without_an_author_has_no_plan() {
        assert!(
            plan_provider_read_back("posts.create", &json!({}), &json!({}), "urn:li:share:9")
                .is_none()
        );
        assert!(plan_provider_read_back(
            "posts.create",
            &json!({}),
            &json!({ "author": " " }),
            "urn:li:share:9"
        )
        .is_none());
    }

    #[test]
    fn meta_creates_plan_a_node_fetch_keyed_only_on_the_receipt() {
        for (operation, read, key) in [
            ("live.create", "live.get", "liveVideoId"),
            (
                "instagram.media.create",
                "instagram.media.status",
                "creationId",
            ),
            (
                "threads.container.create",
                "threads.container.status",
                "creationId",
            ),
        ] {
            let plan =
                plan_provider_read_back(operation, &json!({}), &json!({}), "17900000000000000")
                    .unwrap_or_else(|| panic!("{operation} has a verifier"));
            assert_eq!(plan.operation, read);
            assert_eq!(plan.params[key], json!("17900000000000000"));
            assert_eq!(plan.strength, ReadBackStrength::AuthoritativeById);
            assert_eq!(
                plan.params.as_object().map(serde_json::Map::len),
                Some(1),
                "{operation}'s read-back must not take any input the write did not produce"
            );
        }
    }

    // --- Mutations: field-level read-backs ---------------------------------

    fn field_plan(strength: ReadBackStrength, expectations: &[(&str, &str)]) -> ProviderReadBack {
        ProviderReadBack {
            operation: "events.get",
            params: json!({}),
            strength,
            judgment: ReadBackJudgment::FieldValues {
                locator: "ev-1".to_owned(),
                expectations: expectations
                    .iter()
                    .map(|(field, expected)| FieldExpectation {
                        field: (*field).to_owned(),
                        expected: (*expected).to_owned(),
                    })
                    .collect(),
            },
        }
    }

    #[test]
    fn an_event_update_is_judged_on_the_values_its_patch_set() {
        let plan = plan_provider_read_back(
            "linkedin.events.update",
            &json!({ "eventId": "7212345" }),
            &json!({ "patch": { "$set": { "name": "Autumn launch", "capacity": 250 } } }),
            "",
        )
        .expect("events.update has a verifier");
        assert_eq!(plan.operation, "events.get");
        assert_eq!(plan.params["eventId"], json!("7212345"));
        let ReadBackJudgment::FieldValues {
            locator,
            expectations,
        } = &plan.judgment
        else {
            panic!("a mutation must never be judged on identifier presence");
        };
        assert_eq!(locator, "7212345", "the locator is the write's own target");
        assert_eq!(
            expectations,
            &vec![
                FieldExpectation {
                    field: "capacity".to_owned(),
                    expected: "250".to_owned()
                },
                FieldExpectation {
                    field: "name".to_owned(),
                    expected: "Autumn launch".to_owned()
                },
            ]
        );
    }

    #[test]
    fn a_mutation_with_nothing_comparable_plans_no_read_at_all() {
        // Rather than a read that would silently degenerate into "the object
        // exists", which is the false confirmation this whole split prevents.
        for body in [
            json!({}),
            json!({ "patch": {} }),
            json!({ "patch": { "$delete": ["name"] } }),
            // Nested-only: no scalar whose reported shape we could trust.
            json!({ "patch": { "$set": { "schedule": { "start": 1 } } } }),
        ] {
            assert!(
                plan_provider_read_back(
                    "events.update",
                    &json!({ "eventId": "7212345" }),
                    &body,
                    ""
                )
                .is_none(),
                "an update with no comparable field must not plan a read"
            );
        }
        assert!(
            plan_provider_read_back(
                "events.update",
                &json!({}),
                &json!({ "patch": { "$set": { "name": "x" } } }),
                ""
            )
            .is_none(),
            "without the target id there is nothing to read back"
        );
    }

    #[test]
    fn okta_lifecycle_expectations_come_from_the_operation_not_the_body() {
        for (operation, expected) in [
            ("user.suspend", "SUSPENDED"),
            ("okta.user.suspend", "SUSPENDED"),
            ("user.activate", "ACTIVE"),
            ("okta.user.activate", "ACTIVE"),
        ] {
            let plan =
                plan_provider_read_back(operation, &json!({ "userId": "00u1" }), &json!({}), "")
                    .unwrap_or_else(|| panic!("{operation} has a verifier"));
            assert_eq!(plan.operation, "users");
            let ReadBackJudgment::FieldValues {
                locator,
                expectations,
            } = &plan.judgment
            else {
                panic!("{operation} must be judged on the resulting status");
            };
            assert_eq!(locator, "00u1");
            assert_eq!(expectations.len(), 1);
            assert_eq!(expectations[0].field, "status");
            assert_eq!(expectations[0].expected, expected);
        }
    }

    #[test]
    fn a_catalog_upsert_is_located_by_receipt_then_compared() {
        let plan = plan_provider_read_back(
            "catalog.product.upsert",
            &json!({ "catalogId": "cat-9" }),
            &json!({ "retailer_id": "SKU-1", "price": "199 NOK" }),
            "prod-77",
        )
        .expect("catalog.product.upsert has a verifier");
        assert_eq!(plan.operation, "catalog.products");
        assert_eq!(plan.params["catalogId"], json!("cat-9"));
        assert_eq!(plan.strength, ReadBackStrength::BoundedList);
        let ReadBackJudgment::FieldValues { locator, .. } = &plan.judgment else {
            panic!("an upsert must be judged on values, not existence");
        };
        assert_eq!(locator, "prod-77");
    }

    #[test]
    fn finding_the_object_is_never_enough_to_confirm_a_mutation() {
        // The single most important property of this split: the locator is
        // present, so the identifier check would have confirmed — and the
        // field judgment must not, because the object predates the write.
        let plan = field_plan(ReadBackStrength::AuthoritativeById, &[("name", "New name")]);
        let outcome =
            judge_provider_read_back(&plan, "ev-1", &json!({ "id": "ev-1", "name": "Old name" }));
        assert!(
            matches!(outcome, PostconditionOutcome::Refuted { .. }),
            "an unchanged field must contradict the write, not confirm it"
        );
        assert!(outcome.detail().contains("Old name"));
    }

    #[test]
    fn every_reported_field_matching_confirms_the_mutation() {
        let plan = field_plan(
            ReadBackStrength::AuthoritativeById,
            &[("name", "New name"), ("capacity", "250")],
        );
        let outcome = judge_provider_read_back(
            &plan,
            "ev-1",
            &json!({ "data": { "id": "ev-1", "name": "New name", "capacity": 250 } }),
        );
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
        assert_eq!(outcome.method(), Some("postcondition"));
    }

    #[test]
    fn provider_normalization_does_not_manufacture_a_refutation() {
        // Trimming and re-casing are the provider's business; refuting on
        // those would report a successful write as a failure.
        let plan = field_plan(
            ReadBackStrength::AuthoritativeById,
            &[("name", " New Name ")],
        );
        let outcome =
            judge_provider_read_back(&plan, "ev-1", &json!({ "id": "ev-1", "name": "new name" }));
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
    }

    #[test]
    fn a_field_the_provider_does_not_report_is_unknown_not_mismatched() {
        let plan = field_plan(
            ReadBackStrength::AuthoritativeById,
            &[("name", "New name"), ("visibility", "PUBLIC")],
        );
        let outcome =
            judge_provider_read_back(&plan, "ev-1", &json!({ "id": "ev-1", "name": "New name" }));
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
        assert!(
            outcome.detail().contains("visibility"),
            "what could not be checked must be stated, not quietly dropped"
        );
    }

    #[test]
    fn a_read_back_reporting_none_of_the_changed_fields_is_inconclusive() {
        let plan = field_plan(ReadBackStrength::AuthoritativeById, &[("name", "New name")]);
        let outcome = judge_provider_read_back(&plan, "ev-1", &json!({ "id": "ev-1" }));
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
        assert_eq!(outcome.method(), None);
    }

    #[test]
    fn a_mutated_object_missing_from_a_bounded_window_is_inconclusive() {
        let plan = field_plan(ReadBackStrength::BoundedList, &[("status", "SUSPENDED")]);
        let outcome = judge_provider_read_back(
            &plan,
            "ev-1",
            &json!({ "users": [{ "id": "other", "status": "ACTIVE" }] }),
        );
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "a window that did not include the object proves nothing about it"
        );
    }

    #[test]
    fn an_authoritative_lookup_answering_about_another_object_is_refuted_on_the_values() {
        let plan = field_plan(ReadBackStrength::AuthoritativeById, &[("name", "New name")]);
        let outcome =
            judge_provider_read_back(&plan, "ev-1", &json!({ "id": "someone-else", "name": "x" }));
        assert!(matches!(outcome, PostconditionOutcome::Refuted { .. }));
    }

    #[test]
    fn an_id_reported_as_a_number_still_locates_the_object() {
        // Refuting because the provider serializes its id as a number rather
        // than a string would be a false failure on a successful mutation.
        let plan = field_plan(ReadBackStrength::BoundedList, &[("state", "closed")]);
        let outcome = judge_provider_read_back(
            &plan,
            "ev-1",
            &json!({ "items": [{ "id": 4242, "state": "open" }] }),
        );
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "an unrelated numeric-id record must not be mistaken for the target"
        );

        let plan = ProviderReadBack {
            operation: "issues",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
            judgment: ReadBackJudgment::FieldValues {
                locator: "4242".to_owned(),
                expectations: vec![FieldExpectation {
                    field: "state".to_owned(),
                    expected: "closed".to_owned(),
                }],
            },
        };
        let outcome = judge_provider_read_back(
            &plan,
            "4242",
            &json!({ "items": [{ "id": 4242, "state": "open" }] }),
        );
        assert!(
            matches!(outcome, PostconditionOutcome::Refuted { .. }),
            "a numeric id must locate its object so the stale value can refute"
        );
    }

    #[test]
    fn the_right_element_of_a_listing_is_the_one_compared() {
        let plan = field_plan(ReadBackStrength::BoundedList, &[("status", "SUSPENDED")]);
        let outcome = judge_provider_read_back(
            &plan,
            "ev-1",
            &json!({ "users": [
                { "id": "other", "status": "ACTIVE" },
                { "id": "ev-1", "status": "SUSPENDED" },
            ]}),
        );
        assert!(
            matches!(outcome, PostconditionOutcome::Confirmed { .. }),
            "a sibling record's field must not decide this object's verdict"
        );
    }

    #[test]
    fn a_by_id_read_back_is_not_planned_without_a_receipt() {
        for operation in [
            "linkedin.events.create",
            "live.create",
            "instagram.media.create",
            "threads.container.create",
        ] {
            assert!(
                plan_provider_read_back(operation, &json!({}), &json!({}), "   ").is_none(),
                "{operation} cannot look anything up without an id to look up"
            );
        }
    }

    #[test]
    fn a_receipt_present_in_the_listing_is_confirmed() {
        let plan = ProviderReadBack {
            operation: "messages.list",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
            judgment: ReadBackJudgment::IdentifierPresence,
        };
        let result = json!({ "data": { "action": { "result": {
            "messages": [{ "ts": "1699999999.000100" }, { "ts": "1700000000.000200" }]
        }}}});
        let outcome = judge_provider_read_back(&plan, "1700000000.000200", &result);
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
        assert_eq!(outcome.method(), Some("postcondition"));
    }

    #[test]
    fn absence_from_a_bounded_list_is_inconclusive_never_refuted() {
        // The asymmetry that keeps this layer honest: a window that does not
        // contain the receipt has not proven the effect is missing.
        let plan = ProviderReadBack {
            operation: "messages.list",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
            judgment: ReadBackJudgment::IdentifierPresence,
        };
        let result = json!({ "messages": [{ "ts": "1699999999.000100" }] });
        let outcome = judge_provider_read_back(&plan, "1700000000.000200", &result);
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "a bounded listing can prove presence but never absence"
        );
        assert_eq!(outcome.method(), None);
    }

    #[test]
    fn absence_from_an_authoritative_lookup_is_refuted() {
        let plan = ProviderReadBack {
            operation: "issue.get",
            params: json!({}),
            strength: ReadBackStrength::AuthoritativeById,
            judgment: ReadBackJudgment::IdentifierPresence,
        };
        let outcome = judge_provider_read_back(&plan, "42", &json!({ "id": "43" }));
        assert!(matches!(outcome, PostconditionOutcome::Refuted { .. }));
    }

    #[test]
    fn a_receipt_appearing_only_inside_message_text_is_not_a_match() {
        // Someone quoting an id in a message body must never confirm that the
        // id's own message exists.
        let plan = ProviderReadBack {
            operation: "messages.list",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
            judgment: ReadBackJudgment::IdentifierPresence,
        };
        let result = json!({ "messages": [
            { "ts": "1699999999.000100", "text": "see 1700000000.000200 for details" }
        ]});
        let outcome = judge_provider_read_back(&plan, "1700000000.000200", &result);
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "matching must be on identifier fields, not arbitrary text"
        );
    }

    #[test]
    fn an_empty_receipt_is_inconclusive() {
        let plan = ProviderReadBack {
            operation: "messages.list",
            params: json!({}),
            strength: ReadBackStrength::BoundedList,
            judgment: ReadBackJudgment::IdentifierPresence,
        };
        let outcome = judge_provider_read_back(&plan, "   ", &json!({ "messages": [] }));
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
    }

    #[test]
    fn gmail_send_plans_a_bounded_message_list() {
        let plan =
            plan_provider_read_back("gmail.send", &json!({}), &json!({ "raw": "…" }), "msg-1")
                .expect("gmail.send has a verifier");
        assert_eq!(plan.operation, "gmail.messages");
        assert_eq!(plan.strength, ReadBackStrength::BoundedList);
    }

    // --- Browser procedures -------------------------------------------------

    fn browser_evidence() -> BrowserProcedureEvidence<'static> {
        BrowserProcedureEvidence {
            aborted: false,
            final_status: Some("success"),
            final_page_title: "Order confirmed",
            final_page_text: "Your order 12345 is confirmed.",
            has_durable_evidence: true,
            zdr: false,
            stop_criteria: "confirmed",
            declared_postcondition: None,
        }
    }

    #[test]
    fn a_failed_final_observation_refutes_a_completed_loop() {
        // The whole point: the loop may report Completed while the last thing
        // that actually happened was a failure.
        for status in ["failed", "timeout", "blocked"] {
            let evidence = BrowserProcedureEvidence {
                final_status: Some(status),
                ..browser_evidence()
            };
            let outcome = judge_browser_procedure(&evidence);
            assert!(
                matches!(outcome, PostconditionOutcome::Refuted { .. }),
                "final observation {status} must refute"
            );
        }
    }

    #[test]
    fn an_aborted_procedure_is_refuted() {
        let evidence = BrowserProcedureEvidence {
            aborted: true,
            ..browser_evidence()
        };
        assert!(matches!(
            judge_browser_procedure(&evidence),
            PostconditionOutcome::Refuted { .. }
        ));
    }

    #[test]
    fn no_observation_is_inconclusive_not_refuted() {
        let evidence = BrowserProcedureEvidence {
            final_status: None,
            ..browser_evidence()
        };
        assert!(matches!(
            judge_browser_procedure(&evidence),
            PostconditionOutcome::Inconclusive { .. }
        ));
    }

    #[test]
    fn without_a_declared_postcondition_a_browser_procedure_is_never_confirmed() {
        // Today's real state of the world: PlanConfig carries no postcondition,
        // so success can only ever be inconclusive. If this test starts
        // failing, someone added a confirmation path — make sure it is not
        // circular.
        let outcome = judge_browser_procedure(&browser_evidence());
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
        assert_eq!(outcome.method(), None);
        assert!(outcome.detail().contains("stop criteria"));
    }

    #[test]
    fn reusing_the_stop_criteria_as_the_postcondition_is_refused_as_circular() {
        let evidence = BrowserProcedureEvidence {
            stop_criteria: "confirmed",
            declared_postcondition: Some("confirmed"),
            ..browser_evidence()
        };
        let outcome = judge_browser_procedure(&evidence);
        assert!(
            matches!(outcome, PostconditionOutcome::Inconclusive { .. }),
            "checking the agent against its own stop decision proves nothing"
        );
        assert!(outcome.detail().contains("stop decision"));
    }

    #[test]
    fn a_distinct_postcondition_present_with_evidence_is_confirmed() {
        let evidence = BrowserProcedureEvidence {
            declared_postcondition: Some("order 12345"),
            ..browser_evidence()
        };
        let outcome = judge_browser_procedure(&evidence);
        assert!(matches!(outcome, PostconditionOutcome::Confirmed { .. }));
        assert_eq!(outcome.method(), Some("postcondition"));
    }

    #[test]
    fn a_distinct_postcondition_absent_from_the_final_page_is_refuted() {
        let evidence = BrowserProcedureEvidence {
            declared_postcondition: Some("order 99999"),
            ..browser_evidence()
        };
        assert!(matches!(
            judge_browser_procedure(&evidence),
            PostconditionOutcome::Refuted { .. }
        ));
    }

    #[test]
    fn a_match_without_retained_evidence_is_inconclusive() {
        let evidence = BrowserProcedureEvidence {
            declared_postcondition: Some("order 12345"),
            has_durable_evidence: false,
            ..browser_evidence()
        };
        let outcome = judge_browser_procedure(&evidence);
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
        assert!(outcome.detail().contains("not inspectable"));
    }

    #[test]
    fn a_zdr_run_cannot_be_confirmed_because_no_page_content_was_kept() {
        let evidence = BrowserProcedureEvidence {
            declared_postcondition: Some("order 12345"),
            zdr: true,
            ..browser_evidence()
        };
        let outcome = judge_browser_procedure(&evidence);
        assert!(matches!(outcome, PostconditionOutcome::Inconclusive { .. }));
        assert!(outcome.detail().contains("Zero Data Retention"));
    }
}
