use xero_gcs::{EntityOutcome, OutcomeClass};

/// Classify a failed Xero request without retaining its diagnostic text.
pub(crate) fn classify_source_failure(endpoint: &str, error: &str) -> (OutcomeClass, Option<u16>) {
    let status = parse_http_status(error);
    let class = match status {
        Some(404)
            if matches!(
                endpoint,
                "accounting_get_getcontactcissettings" | "accounting_get_getonlineinvoice"
            ) =>
        {
            OutcomeClass::NotApplicable
        }
        Some(404)
            if matches!(
                endpoint,
                "accounting_get_getorganisationcissettings"
                    | "accounting_get_getreporttenninetynine"
            ) =>
        {
            OutcomeClass::SourceUnavailable
        }
        Some(401 | 403) => OutcomeClass::AccessDenied,
        Some(404) => OutcomeClass::FailedPermanent,
        Some(408 | 425 | 429) | Some(500..=599) => OutcomeClass::FailedRetryable,
        Some(400 | 409 | 422) => OutcomeClass::FailedPermanent,
        _ => OutcomeClass::FailedUnknown,
    };
    (class, status)
}

/// Decide whether an entity should make the whole CLI run fail.
///
/// A tenant-specific unavailable feature or absent optional parent is still
/// useful, auditable source evidence; transport and connector failures are not.
pub fn is_actionable_failure(outcome: &EntityOutcome) -> bool {
    if outcome.termination == "hit_max_pages_cap" {
        return true;
    }
    match outcome.outcome_class {
        Some(
            OutcomeClass::FailedRetryable
            | OutcomeClass::FailedPermanent
            | OutcomeClass::FailedUnknown,
        ) => true,
        Some(OutcomeClass::NotApplicable | OutcomeClass::SourceUnavailable) => false,
        Some(OutcomeClass::AccessDenied) => true,
        None => {
            outcome.error.is_some()
                || matches!(
                    outcome.termination.as_str(),
                    "error" | "partial_detail_failure"
                )
        }
    }
}

pub(crate) fn sanitized_error(error: &str) -> String {
    match parse_http_status(error) {
        Some(status) => format!("Xero source request failed (HTTP {status})"),
        None => "Xero source request failed".to_string(),
    }
}

fn parse_http_status(error: &str) -> Option<u16> {
    let prefixed = error
        .split_ascii_whitespace()
        .next()
        .and_then(parse_status_token);
    prefixed.or_else(|| {
        error
            .match_indices("HTTP ")
            .find_map(|(index, _)| parse_status_token(&error[index + 5..]))
    })
}

fn parse_status_token(value: &str) -> Option<u16> {
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    let status = digits.parse::<u16>().ok()?;
    (100..=599).contains(&status).then_some(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_documented_xero_absences_without_retaining_error_text() {
        assert_eq!(
            classify_source_failure(
                "accounting_get_getonlineinvoice",
                "Accounting GET failed (HTTP 404 Not Found)"
            ),
            (OutcomeClass::NotApplicable, Some(404))
        );
        assert_eq!(
            classify_source_failure(
                "accounting_get_getreporttenninetynine",
                "Accounting GET failed (HTTP 404 Not Found)"
            ),
            (OutcomeClass::SourceUnavailable, Some(404))
        );
    }

    #[test]
    fn classifies_access_and_retryable_failures() {
        assert_eq!(
            classify_source_failure("projects_projects", "403 forbidden"),
            (OutcomeClass::AccessDenied, Some(403))
        );
        assert_eq!(
            classify_source_failure("invoices", "429 rate limited"),
            (OutcomeClass::FailedRetryable, Some(429))
        );
        assert_eq!(
            classify_source_failure("invoices", "invalid JSON envelope"),
            (OutcomeClass::FailedUnknown, None)
        );
        assert_eq!(
            classify_source_failure("invoices", "Accounting GET failed (HTTP 404 Not Found)"),
            (OutcomeClass::FailedPermanent, Some(404))
        );
    }

    #[test]
    fn persisted_error_summary_never_copies_source_diagnostics() {
        assert_eq!(
            sanitized_error("Accounting GET failed (HTTP 404 Not Found): /Invoices/private-id"),
            "Xero source request failed (HTTP 404)"
        );
        assert_eq!(
            sanitized_error("private upstream message"),
            "Xero source request failed"
        );
    }
}
