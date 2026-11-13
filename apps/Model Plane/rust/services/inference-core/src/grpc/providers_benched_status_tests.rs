//! The wire form of `providers_benched` (R2-OD-40-b2): every eligible provider is
//! benched, so the caller is told to retry later, never given a posture refusal.

use super::*;

fn trailer<'a>(status: &'a Status, key: &str) -> Option<&'a str> {
    status
        .metadata()
        .get(key)
        .and_then(|value| value.to_str().ok())
}

#[test]
fn providers_benched_is_unavailable_with_its_kind_and_retry_after() {
    let status = provider_error_to_status(provider::ProviderError::ProvidersBenched {
        retry_after_ms: 1_500,
    });
    assert_eq!(status.code(), tonic::Code::Unavailable);
    // The kind and trailer names are a frozen wire vocabulary: spelled as literals.
    assert_eq!(
        trailer(&status, "x-mp-provider-error"),
        Some("providers_benched")
    );
    assert_eq!(trailer(&status, "x-mp-retry-after-ms"), Some("1500"));
    assert_eq!(trailer(&status, "x-mp-resets-at"), None);
    assert!(
        !status.message().contains("zdr") && !status.message().contains("tier"),
        "not a posture refusal: {}",
        status.message()
    );
}

#[test]
fn providers_benched_is_not_a_posture_refusal_code() {
    // Data Plane treats FAILED_PRECONDITION posture kinds as terminal; a bench is transient.
    let status =
        provider_error_to_status(provider::ProviderError::ProvidersBenched { retry_after_ms: 1 });
    assert_ne!(status.code(), tonic::Code::FailedPrecondition);
    assert_eq!(trailer(&status, "x-mp-retry-after-ms"), Some("1"));
}
