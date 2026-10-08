//! Post-mutation verification inside the agentic loop (chat gap closure, B8).
//!
//! # What this closes
//!
//! `crate::postcondition` holds deterministic verifiers that go back to the
//! system of record and ask whether a write's effect is really there. Until
//! now only the approval delivery worker used them, after the run was over, so
//! the model that ran a write never learned whether it held. A model that has
//! just been told "Executed gmail.send" writes "I sent it", and nothing it
//! reads can contradict that.
//!
//! This module runs the verifier **in the loop**, right after a successful
//! write, and appends one line to the tool output. The line is the next
//! round's context and the step record's text, so the model reads the verdict
//! before it answers and the trace shows it.
//!
//! # What it does not change
//!
//! - A write's own outcome. The verdict never turns a success into an error:
//!   the effect may well exist, and an error would invite the model to retry
//!   and duplicate it. A refutation is words, not a failed step.
//! - The invariant of `crate::postcondition`: an outage, a refused read, a
//!   missing verifier and an ambiguous answer are all `Inconclusive`, and say
//!   so. Nothing is upgraded to "verified" without a read that agreed.
//! - The durable receipts. They stay the audit record; this feeds the model
//!   and the trace.
//! - Reads. Only a write is verified.
//!
//! # Off by default
//!
//! [`ENABLED_ENV`] must say so explicitly (`1`, `true`, `on`, `yes`,
//! `enabled`). With it unset every write returns exactly what it did before:
//! no extra request, no extra line. The read-back costs a round trip per
//! write (bounded by [`VERIFY_DEADLINE`]) and a wrong read-back can mislead
//! the model, so an operator opts in once a connector has been seen to answer
//! the read.
//!
//! # Where the read-back's authority comes from
//!
//! The read-back uses the run's own `integration:read` / `shipping:read`
//! credentials, the ones every read in a run already uses, never a write
//! credential. It cannot perform a write even by mistake.

use std::future::Future;
use std::time::Duration;

use serde_json::Value;

use crate::integration_tools::IntegrationActionsClient;
use crate::postcondition::{self, PostconditionOutcome};
use crate::shipping_tools::{BookInput, ShippingToolsClient};
use crate::tool_bridge::ToolExecution;

use super::tool_error;

/// The switch. Read on every write rather than once, so an operator's change
/// needs a restart and nothing else.
pub(crate) const ENABLED_ENV: &str = "EXECUTION_POST_MUTATION_VERIFY_ENABLED";

/// Longest the in-loop read-back may take, token mint included. The write has
/// already happened, so a slow provider must cost the run seconds, not the
/// 25 s a request may otherwise take twice over; a read-back that has not
/// answered by then is `Inconclusive`, never a refutation.
const VERIFY_DEADLINE: Duration = Duration::from_secs(15);

/// The most characters a write's tool output may have once the verification
/// line is on it. The durable step record keeps the first 2,000 characters of
/// an output (`OUTPUT_TRUNCATE` in `runtime_loop::agent`) after a source label
/// and an audit prefix of about 100, so a line at the tail of a long provider
/// echo would reach the model and be cut from the trace. This is the budget
/// that keeps it in both.
const DURABLE_BUDGET_CHARS: usize = 1800;

/// What a cut result says, so a shortened echo is never mistaken for the
/// provider's whole answer.
const SHORTENED_MARKER: &str =
    "\n[…result shortened so the verification line fits the step record]";

/// Whether an explicit env value turns the verification on. Pure so the policy
/// is unit-testable without touching the process environment.
#[must_use]
pub(crate) fn parse_enabled(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "on" | "yes" | "enabled")
    )
}

/// Whether this deployment verifies writes in the loop right now.
#[must_use]
pub(crate) fn enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = test_overrides::enabled() {
        return forced;
    }
    parse_enabled(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The integration-corev2 client for a provider action.
///
/// Production builds it from the environment. A test installs a client aimed
/// at fake servers through [`test_overrides::Overrides`], because the loop
/// builds its clients where it runs a tool and a test cannot reach in.
pub(crate) fn integration_client() -> Option<IntegrationActionsClient> {
    #[cfg(test)]
    if let Some(client) = test_overrides::integration() {
        return Some(client);
    }
    IntegrationActionsClient::from_env()
}

/// The shipping-core client for a booking. See [`integration_client`].
pub(crate) fn shipping_client() -> Option<ShippingToolsClient> {
    #[cfg(test)]
    if let Some(client) = test_overrides::shipping() {
        return Some(client);
    }
    ShippingToolsClient::from_env()
}

/// One provider action the loop is about to run, everything the call and its
/// read-back need.
pub(crate) struct ProviderCall<'a> {
    pub org_id: &'a str,
    pub user_id: &'a str,
    pub run_id: &'a str,
    pub step_id: &'a str,
    pub connection_id: &'a str,
    pub operation: &'a str,
    pub params: Value,
    pub body: Value,
    /// The durable approval id a write forwards; `None` for a read.
    pub approval_ref: Option<&'a str>,
    pub session_bearer: Option<&'a str>,
    /// Whether the operation is a write (an unknown operation counts as one).
    pub is_write: bool,
}

/// Run one provider action and, for a write that succeeded, append the
/// verdict of reading it back from the provider.
pub(crate) async fn run_provider_call(
    client: &IntegrationActionsClient,
    call: ProviderCall<'_>,
) -> ToolExecution {
    let verify = call.is_write && enabled();
    // The write consumes its params and body, and the read-back needs exactly
    // what the write asked for (the object it targeted, the values it set).
    // Copied before the call, and without the approval id the call adds to its
    // own copy of the body.
    let asked = verify.then(|| (call.params.clone(), call.body.clone()));

    let outcome = match client
        .execute_action(
            call.org_id,
            call.connection_id,
            call.operation,
            call.params,
            call.body,
            call.user_id,
            call.approval_ref,
            call.session_bearer,
        )
        .await
    {
        Ok(outcome) => outcome,
        Err(error) => return tool_error(error),
    };

    let mut output = outcome.rendered;
    if let Some((params, body)) = asked {
        let verdict = within(
            VERIFY_DEADLINE,
            postcondition::verify_provider_write(
                client,
                call.org_id,
                call.connection_id,
                call.operation,
                &params,
                &body,
                outcome.provider_receipt_id.as_deref(),
                call.session_bearer,
            ),
        )
        .await;
        record(call.run_id, call.step_id, call.operation, &verdict);
        output = with_verdict(output, &verdict);
    }
    ToolExecution {
        output,
        error: None,
    }
}

/// Book a shipment and, when it succeeded, append the verdict of reading the
/// booking back from shipping-core.
pub(crate) async fn run_shipment_booking(
    client: &ShippingToolsClient,
    input: &BookInput,
    org_id: &str,
    approval_id: &str,
    idempotency_key: &str,
    run_id: &str,
    step_id: &str,
) -> ToolExecution {
    let outcome = match client
        .book_shipment(input, org_id, approval_id, idempotency_key)
        .await
    {
        Ok(outcome) => outcome,
        Err(error) => return tool_error(error),
    };
    let mut output = outcome.rendered;
    if enabled() {
        let verdict = within(
            VERIFY_DEADLINE,
            postcondition::verify_shipment_booking(client, &outcome.booking_id, org_id),
        )
        .await;
        record(run_id, step_id, "book_shipment", &verdict);
        output = with_verdict(output, &verdict);
    }
    ToolExecution {
        output,
        error: None,
    }
}

/// Wait for a read-back for at most `deadline`. Running out of time is the
/// provider being slow, which says nothing about whether the write holds.
async fn within<F>(deadline: Duration, read_back: F) -> PostconditionOutcome
where
    F: Future<Output = PostconditionOutcome>,
{
    match tokio::time::timeout(deadline, read_back).await {
        Ok(verdict) => verdict,
        Err(_) => PostconditionOutcome::Inconclusive {
            detail: format!(
                "the read-back did not answer within {} seconds",
                deadline.as_secs().max(1)
            ),
        },
    }
}

/// `rendered` followed by the verdict's line, with `rendered` cut when the two
/// together would not fit the durable step record (see
/// [`DURABLE_BUDGET_CHARS`]). The line is never the part that gives way.
fn with_verdict(rendered: String, verdict: &PostconditionOutcome) -> String {
    let line = verdict.verdict_line();
    let room = DURABLE_BUDGET_CHARS.saturating_sub(line.chars().count() + 1);
    let kept = if rendered.chars().count() > room {
        let marker = SHORTENED_MARKER.chars().count();
        let mut cut: String = rendered.chars().take(room.saturating_sub(marker)).collect();
        cut.push_str(SHORTENED_MARKER);
        cut
    } else {
        rendered
    };
    format!("{kept}\n{line}")
}

/// A metadata-only trace of the verdict. The detail can quote the provider, so
/// it stays out of the log; the verdict kind and the ids are enough to find the
/// step.
fn record(run_id: &str, step_id: &str, operation: &str, verdict: &PostconditionOutcome) {
    match verdict {
        PostconditionOutcome::Refuted { .. } => tracing::warn!(
            run_id,
            step_id,
            operation,
            verdict = "refuted",
            "post-mutation verification: the system of record contradicts the write"
        ),
        PostconditionOutcome::Confirmed { .. } => tracing::info!(
            run_id,
            step_id,
            operation,
            verdict = "confirmed",
            "post-mutation verification"
        ),
        PostconditionOutcome::Inconclusive { .. } => tracing::info!(
            run_id,
            step_id,
            operation,
            verdict = "inconclusive",
            "post-mutation verification"
        ),
    }
}

/// Per-thread stand-ins for the env switch and the two service clients, so a
/// test can drive the loop without mutating the process environment (which
/// every test running in parallel would read). `#[tokio::test]` runs its whole
/// task tree on the test's own thread, so an override reaches everything that
/// test drives.
#[cfg(test)]
pub(crate) mod test_overrides {
    use std::cell::{Cell, RefCell};

    use crate::integration_tools::IntegrationActionsClient;
    use crate::shipping_tools::ShippingToolsClient;

    thread_local! {
        static ENABLED: Cell<Option<bool>> = const { Cell::new(None) };
        static INTEGRATION: RefCell<Option<IntegrationActionsClient>> =
            const { RefCell::new(None) };
        static SHIPPING: RefCell<Option<ShippingToolsClient>> = const { RefCell::new(None) };
    }

    pub(super) fn enabled() -> Option<bool> {
        ENABLED.with(Cell::get)
    }

    pub(super) fn integration() -> Option<IntegrationActionsClient> {
        INTEGRATION.with(|cell| cell.borrow().clone())
    }

    pub(super) fn shipping() -> Option<ShippingToolsClient> {
        SHIPPING.with(|cell| cell.borrow().clone())
    }

    /// Holds the overrides it was given on the current thread until dropped.
    pub(crate) struct Overrides;

    impl Overrides {
        pub(crate) fn new() -> Self {
            Self
        }

        pub(crate) fn enabled(self, value: bool) -> Self {
            ENABLED.with(|cell| cell.set(Some(value)));
            self
        }

        pub(crate) fn integration(self, client: IntegrationActionsClient) -> Self {
            INTEGRATION.with(|cell| *cell.borrow_mut() = Some(client));
            self
        }

        pub(crate) fn shipping(self, client: ShippingToolsClient) -> Self {
            SHIPPING.with(|cell| *cell.borrow_mut() = Some(client));
            self
        }
    }

    impl Drop for Overrides {
        fn drop(&mut self) {
            ENABLED.with(|cell| cell.set(None));
            INTEGRATION.with(|cell| *cell.borrow_mut() = None);
            SHIPPING.with(|cell| *cell.borrow_mut() = None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::test_overrides::Overrides;
    use super::*;
    use crate::attestation::Signer;

    // --- The switch ----------------------------------------------------------

    #[test]
    fn only_an_explicit_value_turns_verification_on() {
        for on in ["1", "true", "TRUE", " on ", "yes", "Enabled"] {
            assert!(parse_enabled(Some(on)), "{on:?} must turn it on");
        }
        for off in ["", "0", "false", "off", "no", "disabled", "maybe", "2"] {
            assert!(!parse_enabled(Some(off)), "{off:?} must leave it off");
        }
        assert!(!parse_enabled(None), "unset is off");
    }

    // --- The line and its budget ---------------------------------------------

    #[test]
    fn the_verification_line_always_survives_a_long_result() {
        let long = format!("Executed gmail.send. Result:\n{}", "x".repeat(5000));
        let verdict = PostconditionOutcome::Refuted {
            detail: "y".repeat(2000),
        };
        let output = with_verdict(long, &verdict);

        assert!(
            output.chars().count() <= DURABLE_BUDGET_CHARS,
            "the output must fit the step record, got {} chars",
            output.chars().count()
        );
        assert!(
            output.ends_with(&verdict.verdict_line()),
            "the verdict is the last thing the model reads and the part that never gives way"
        );
        assert!(
            output.contains("result shortened"),
            "a cut result must say it was cut"
        );
        assert_eq!(
            output
                .lines()
                .last()
                .map(|line| line.starts_with("NOT confirmed")),
            Some(true)
        );
    }

    #[test]
    fn a_short_result_is_left_exactly_as_it_was() {
        let rendered = "Executed gmail.send. Result:\n{\"id\":\"msg-1\"}".to_owned();
        let verdict = PostconditionOutcome::Confirmed {
            detail: "listed".to_owned(),
        };
        assert_eq!(
            with_verdict(rendered.clone(), &verdict),
            format!("{rendered}\nVerified against the system of record: listed")
        );
    }

    #[tokio::test]
    async fn a_read_back_that_does_not_answer_in_time_is_inconclusive() {
        let verdict = within(
            Duration::from_millis(20),
            std::future::pending::<PostconditionOutcome>(),
        )
        .await;
        assert!(
            matches!(&verdict, PostconditionOutcome::Inconclusive { detail } if detail.contains("did not answer")),
            "an unanswered read-back is not a refutation: {verdict:?}"
        );
    }

    // --- Provider writes, against fake Auth Core and integration-corev2 -------

    const ORG: &str = "org-1";
    const CONNECTION: &str = "conn-1";
    const ACTIONS_PATH: &str = "/api/v1/connections/conn-1/actions";

    struct Fakes {
        auth: MockServer,
        integration: MockServer,
    }

    impl Fakes {
        /// Auth Core mints a token for any scope (and exchanges a session
        /// bearer for one), and integration-corev2 knows `conn-1`'s provider.
        async fn start() -> Self {
            let auth = MockServer::start().await;
            for token_path in [
                "/api/ingestion/internal-token",
                "/api/ingestion/token-exchange",
            ] {
                Mock::given(method("POST"))
                    .and(path(token_path))
                    .respond_with(
                        ResponseTemplate::new(200).set_body_json(json!({"token": "test-token"})),
                    )
                    .mount(&auth)
                    .await;
            }
            let integration = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/api/v1/connections/conn-1"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"data": {"connection": {"providerKey": "google"}}})),
                )
                .mount(&integration)
                .await;
            Self { auth, integration }
        }

        fn client(&self) -> IntegrationActionsClient {
            let signer = Signer::new([7u8; 32], "test-key-1").expect("test signer");
            IntegrationActionsClient::new_for_test(
                &self.auth.uri(),
                &self.integration.uri(),
                Some(Arc::new(signer)),
            )
        }

        /// What integration-corev2 answers when `operation` is called.
        async fn answer(&self, operation: &str, status: u16, result: serde_json::Value) {
            Mock::given(method("POST"))
                .and(path(ACTIONS_PATH))
                .and(body_partial_json(json!({ "operation": operation })))
                .respond_with(ResponseTemplate::new(status).set_body_json(
                    json!({"success": status < 300, "data": {"action": {"result": result}}}),
                ))
                .mount(&self.integration)
                .await;
        }

        /// The operations integration-corev2 was asked to run, in order.
        async fn operations_called(&self) -> Vec<String> {
            self.integration
                .received_requests()
                .await
                .expect("request recording is on")
                .iter()
                .filter(|request| request.url.path() == ACTIONS_PATH)
                .filter_map(|request| {
                    let body: serde_json::Value = serde_json::from_slice(&request.body).ok()?;
                    body["operation"].as_str().map(str::to_owned)
                })
                .collect()
        }
    }

    fn call(
        operation: &str,
        params: serde_json::Value,
        body: serde_json::Value,
        is_write: bool,
    ) -> ProviderCall<'_> {
        ProviderCall {
            org_id: ORG,
            user_id: "user-1",
            run_id: "run-1",
            step_id: "step-1",
            connection_id: CONNECTION,
            operation,
            params,
            body,
            approval_ref: is_write.then_some("appr_run-1_step-1"),
            session_bearer: None,
            is_write,
        }
    }

    fn last_line(output: &str) -> &str {
        output.lines().last().unwrap_or_default()
    }

    #[tokio::test]
    async fn a_confirmed_write_ends_with_the_verified_line() {
        let _on = Overrides::new().enabled(true);
        let fakes = Fakes::start().await;
        fakes
            .answer("gmail.send", 200, json!({"id": "msg-1"}))
            .await;
        fakes
            .answer(
                "gmail.messages",
                200,
                json!({"messages": [{"id": "msg-0"}, {"id": "msg-1"}]}),
            )
            .await;

        let result = run_provider_call(
            &fakes.client(),
            call(
                "gmail.send",
                json!({}),
                json!({"to": "a@example.org", "subject": "Hi", "bodyText": "Hello"}),
                true,
            ),
        )
        .await;

        assert_eq!(result.error, None);
        assert!(
            result.output.starts_with("Executed gmail.send."),
            "the write's own result is kept first: {}",
            result.output
        );
        assert!(
            last_line(&result.output).starts_with("Verified against the system of record:"),
            "a confirmed write says so: {}",
            result.output
        );
        assert_eq!(
            fakes.operations_called().await,
            ["gmail.send", "gmail.messages"],
            "one write, then one read-back"
        );
    }

    #[tokio::test]
    async fn a_refuted_write_tells_the_model_not_to_report_success() {
        let _on = Overrides::new().enabled(true);
        let fakes = Fakes::start().await;
        fakes
            .answer("linkedin.events.create", 200, json!({"id": "evt-1"}))
            .await;
        // The provider's own lookup answers, with a different event.
        fakes
            .answer("events.get", 200, json!({"id": "evt-OTHER", "name": "x"}))
            .await;

        let result = run_provider_call(
            &fakes.client(),
            call(
                "linkedin.events.create",
                json!({}),
                json!({"name": "Launch"}),
                true,
            ),
        )
        .await;

        let line = last_line(&result.output);
        assert!(
            line.starts_with("NOT confirmed: the system of record contradicts this write:"),
            "{}",
            result.output
        );
        assert!(
            line.ends_with("do not tell the user it succeeded, report the discrepancy"),
            "{line}"
        );
        assert_eq!(
            result.error, None,
            "a refutation is words, never a failed step the model would retry into a duplicate"
        );
        assert!(
            result.output.contains("evt-1"),
            "the write's own result is still shown"
        );
    }

    #[tokio::test]
    async fn a_write_with_no_verifier_says_it_could_not_verify() {
        let _on = Overrides::new().enabled(true);
        let fakes = Fakes::start().await;
        fakes
            .answer("pages.post", 200, json!({"id": "post-1"}))
            .await;

        let result = run_provider_call(
            &fakes.client(),
            call(
                "pages.post",
                json!({"pageId": "p"}),
                json!({"message": "hi"}),
                true,
            ),
        )
        .await;

        assert_eq!(
            last_line(&result.output),
            "Could not verify (no postcondition verifier is defined for pages.post); \
             say so, do not claim it is confirmed"
        );
        assert_eq!(
            fakes.operations_called().await,
            ["pages.post"],
            "with no verifier there is nothing to read back"
        );
    }

    #[tokio::test]
    async fn an_outage_in_the_read_back_is_never_a_refutation() {
        let _on = Overrides::new().enabled(true);
        let fakes = Fakes::start().await;
        fakes
            .answer("gmail.send", 200, json!({"id": "msg-1"}))
            .await;
        fakes
            .answer("gmail.messages", 502, json!({"message": "upstream down"}))
            .await;

        let result = run_provider_call(
            &fakes.client(),
            call(
                "gmail.send",
                json!({}),
                json!({"to": "a@example.org"}),
                true,
            ),
        )
        .await;

        let line = last_line(&result.output);
        assert!(
            line.starts_with("Could not verify (could not read gmail.messages back"),
            "{line}"
        );
        assert!(
            line.ends_with("say so, do not claim it is confirmed"),
            "{line}"
        );
        assert!(
            !result.output.contains("NOT confirmed"),
            "{}",
            result.output
        );
        assert_eq!(result.error, None, "the write itself succeeded");
    }

    #[tokio::test]
    async fn a_receiptless_mutation_is_judged_on_the_values_it_set() {
        let _on = Overrides::new().enabled(true);
        for (stored_name, expect_prefix) in [
            ("Old name", "NOT confirmed"),
            ("New name", "Verified against the system of record"),
        ] {
            let fakes = Fakes::start().await;
            // The update answers with no id, which is how a 204 reads.
            fakes
                .answer("linkedin.events.update", 200, json!({"ok": true}))
                .await;
            fakes
                .answer(
                    "events.get",
                    200,
                    json!({"id": "evt-9", "name": stored_name}),
                )
                .await;

            let result = run_provider_call(
                &fakes.client(),
                call(
                    "linkedin.events.update",
                    json!({"eventId": "evt-9"}),
                    json!({"patch": {"$set": {"name": "New name"}}}),
                    true,
                ),
            )
            .await;

            assert!(
                last_line(&result.output).starts_with(expect_prefix),
                "stored {stored_name:?}: {}",
                result.output
            );
            assert_eq!(
                fakes.operations_called().await,
                ["linkedin.events.update", "events.get"]
            );
        }
    }

    #[tokio::test]
    async fn a_receiptless_create_cannot_be_verified_and_reads_nothing() {
        let _on = Overrides::new().enabled(true);
        let fakes = Fakes::start().await;
        fakes.answer("gmail.send", 200, json!({"ok": true})).await;

        let result = run_provider_call(
            &fakes.client(),
            call(
                "gmail.send",
                json!({}),
                json!({"to": "a@example.org"}),
                true,
            ),
        )
        .await;

        let line = last_line(&result.output);
        assert!(
            line.starts_with(
                "Could not verify (the provider returned no receipt id for gmail.send"
            ),
            "{line}"
        );
        assert_eq!(
            fakes.operations_called().await,
            ["gmail.send"],
            "a create without its receipt has nothing to stand on, so nothing is read"
        );
    }

    #[tokio::test]
    async fn a_read_operation_is_never_verified() {
        let _on = Overrides::new().enabled(true);
        let fakes = Fakes::start().await;
        fakes
            .answer(
                "gmail.messages",
                200,
                json!({"messages": [{"id": "msg-1"}]}),
            )
            .await;

        let result = run_provider_call(
            &fakes.client(),
            call("gmail.messages", json!({"maxResults": 5}), json!({}), false),
        )
        .await;

        assert!(
            !result.output.contains("system of record"),
            "{}",
            result.output
        );
        assert!(
            !result.output.contains("Could not verify"),
            "{}",
            result.output
        );
        assert_eq!(fakes.operations_called().await, ["gmail.messages"]);
    }

    #[tokio::test]
    async fn with_the_switch_off_a_write_is_exactly_what_it_was() {
        let _off = Overrides::new().enabled(false);
        let fakes = Fakes::start().await;
        fakes
            .answer("gmail.send", 200, json!({"id": "msg-1"}))
            .await;

        let result = run_provider_call(
            &fakes.client(),
            call(
                "gmail.send",
                json!({}),
                json!({"to": "a@example.org"}),
                true,
            ),
        )
        .await;

        assert_eq!(result.error, None);
        assert!(
            !result.output.contains("system of record")
                && !result.output.contains("Could not verify"),
            "no verdict line while the switch is off: {}",
            result.output
        );
        assert_eq!(
            fakes.operations_called().await,
            ["gmail.send"],
            "no read-back request while the switch is off"
        );
    }

    #[tokio::test]
    async fn a_failed_write_is_an_error_and_is_not_verified() {
        let _on = Overrides::new().enabled(true);
        let fakes = Fakes::start().await;
        fakes
            .answer("gmail.send", 502, json!({"message": "provider refused"}))
            .await;

        let result = run_provider_call(
            &fakes.client(),
            call(
                "gmail.send",
                json!({}),
                json!({"to": "a@example.org"}),
                true,
            ),
        )
        .await;

        assert!(
            result
                .error
                .as_deref()
                .is_some_and(|e| e.contains("provider action returned")),
            "{result:?}"
        );
        assert!(result.output.is_empty());
        assert_eq!(
            fakes.operations_called().await,
            ["gmail.send"],
            "a write that failed has no effect to read back"
        );
    }

    #[tokio::test]
    async fn a_long_provider_echo_does_not_push_the_verdict_out_of_the_record() {
        let _on = Overrides::new().enabled(true);
        let fakes = Fakes::start().await;
        fakes
            .answer(
                "gmail.send",
                200,
                json!({"id": "msg-1", "echo": "z".repeat(6000)}),
            )
            .await;
        fakes
            .answer(
                "gmail.messages",
                200,
                json!({"messages": [{"id": "msg-1"}]}),
            )
            .await;

        let result = run_provider_call(
            &fakes.client(),
            call(
                "gmail.send",
                json!({}),
                json!({"to": "a@example.org"}),
                true,
            ),
        )
        .await;

        assert!(result.output.chars().count() <= DURABLE_BUDGET_CHARS);
        assert!(last_line(&result.output).starts_with("Verified against the system of record:"));
    }

    // --- Shipment bookings, against fake Auth Core and shipping-core ----------

    fn booking_input() -> BookInput {
        let address = |name: &str| {
            json!({
                "name": name, "street": "Gata 1", "postal_code": "0001", "city": "Oslo",
                "country": "NO", "is_business": true, "phone": "+4712345678", "email": null
            })
        };
        serde_json::from_value(json!({
            "quote_ref": "quote-1", "carrier_code": "mock-bring", "service_name": "Parcel",
            "price_amount_cents": 1000, "price_currency": "NOK",
            "from": address("Sender"), "to": address("Recipient"),
            "weight_kg": 1.0, "length_cm": 10.0, "width_cm": 10.0, "height_cm": 10.0,
            "booked_by": "user-1"
        }))
        .expect("a valid booking input")
    }

    /// shipping-core that books `booking-1` and then answers the read-back with
    /// `read_back`.
    async fn shipping_fakes(read_back: ResponseTemplate) -> (MockServer, MockServer) {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "t"})))
            .mount(&auth)
            .await;
        let shipping = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/bookings"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "booking_id": "booking-1", "confirmation_token": "confirm-1"
            })))
            .mount(&shipping)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/bookings/booking-1/confirm"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "carrier_name": "Bring", "booking_ref": "ref-1",
                "tracking_no": "TRK-1", "has_label": false
            })))
            .mount(&shipping)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/bookings/booking-1"))
            .respond_with(read_back)
            .mount(&shipping)
            .await;
        (auth, shipping)
    }

    async fn book(auth: &MockServer, shipping: &MockServer) -> ToolExecution {
        let client = ShippingToolsClient::new_for_test(
            shipping.uri(),
            auth.uri(),
            "model-execution",
            "test-only-service-key",
        );
        run_shipment_booking(
            &client,
            &booking_input(),
            ORG,
            "appr_run-1_step-1",
            "run-1:step-1",
            "run-1",
            "step-1",
        )
        .await
    }

    async fn reads_of_the_booking(shipping: &MockServer) -> usize {
        shipping
            .received_requests()
            .await
            .expect("request recording is on")
            .iter()
            .filter(|request| request.method.as_str() == "GET")
            .count()
    }

    #[tokio::test]
    async fn a_booking_shipping_core_holds_is_confirmed() {
        let _on = Overrides::new().enabled(true);
        let (auth, shipping) =
            shipping_fakes(ResponseTemplate::new(200).set_body_json(
                json!({"id": "booking-1", "status": "booked", "tracking_no": "TRK-1"}),
            ))
            .await;

        let result = book(&auth, &shipping).await;

        assert_eq!(result.error, None);
        assert!(
            result.output.starts_with("Shipment BOOKED with Bring."),
            "{}",
            result.output
        );
        assert_eq!(
            last_line(&result.output),
            "Verified against the system of record: shipping-core independently reports \
             booking booking-1 as booked with tracking number TRK-1"
        );
    }

    #[tokio::test]
    async fn a_booking_shipping_core_does_not_have_is_refuted() {
        let _on = Overrides::new().enabled(true);
        let (auth, shipping) =
            shipping_fakes(ResponseTemplate::new(404).set_body_json(json!({"error": "not found"})))
                .await;

        let result = book(&auth, &shipping).await;

        let line = last_line(&result.output);
        assert!(
            line.starts_with("NOT confirmed: the system of record contradicts this write: shipping-core has no booking booking-1"),
            "{line}"
        );
        assert_eq!(
            result.error, None,
            "a refutation is words, not a failed step"
        );
    }

    #[tokio::test]
    async fn a_cancelled_booking_is_refuted() {
        let _on = Overrides::new().enabled(true);
        let (auth, shipping) = shipping_fakes(
            ResponseTemplate::new(200).set_body_json(json!({"status": "cancelled"})),
        )
        .await;

        let result = book(&auth, &shipping).await;

        assert!(
            last_line(&result.output)
                .contains("shipping-core reports booking booking-1 as cancelled"),
            "{}",
            result.output
        );
    }

    #[tokio::test]
    async fn a_shipping_core_outage_is_inconclusive_not_a_refutation() {
        let _on = Overrides::new().enabled(true);
        let (auth, shipping) = shipping_fakes(
            ResponseTemplate::new(500).set_body_json(json!({"error": "database unavailable"})),
        )
        .await;

        let result = book(&auth, &shipping).await;

        let line = last_line(&result.output);
        assert!(
            line.starts_with("Could not verify (could not read booking booking-1 back"),
            "{line}"
        );
        assert!(
            !result.output.contains("NOT confirmed"),
            "{}",
            result.output
        );
        assert!(result.output.starts_with("Shipment BOOKED with Bring."));
    }

    #[tokio::test]
    async fn a_pending_booking_is_not_called_confirmed() {
        let _on = Overrides::new().enabled(true);
        let (auth, shipping) =
            shipping_fakes(ResponseTemplate::new(200).set_body_json(json!({"status": "pending"})))
                .await;

        let result = book(&auth, &shipping).await;

        assert!(
            last_line(&result.output).starts_with(
                "Could not verify (shipping-core reports booking booking-1 as pending"
            ),
            "{}",
            result.output
        );
    }

    #[tokio::test]
    async fn with_the_switch_off_a_booking_reads_nothing_back() {
        let _off = Overrides::new().enabled(false);
        let (auth, shipping) = shipping_fakes(
            ResponseTemplate::new(200)
                .set_body_json(json!({"status": "booked", "tracking_no": "TRK-1"})),
        )
        .await;

        let result = book(&auth, &shipping).await;

        assert!(
            !result.output.contains("system of record"),
            "{}",
            result.output
        );
        assert_eq!(reads_of_the_booking(&shipping).await, 0);
    }

    #[tokio::test]
    async fn a_booking_that_failed_is_an_error_and_is_not_verified() {
        let _on = Overrides::new().enabled(true);
        let (auth, shipping) = shipping_fakes(ResponseTemplate::new(200)).await;
        // The create step is refused, so there is no booking to read back.
        shipping.reset().await;
        Mock::given(method("POST"))
            .and(path("/api/bookings"))
            .respond_with(
                ResponseTemplate::new(502).set_body_json(json!({"error": "carrier down"})),
            )
            .mount(&shipping)
            .await;

        let result = book(&auth, &shipping).await;

        assert!(
            result
                .error
                .as_deref()
                .is_some_and(|e| e.contains("booking create failed")),
            "{result:?}"
        );
        assert_eq!(reads_of_the_booking(&shipping).await, 0);
    }
}
