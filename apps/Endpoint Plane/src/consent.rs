//! A native Yes/No prompt shown to whoever is physically at this machine,
//! deciding whether a newly-logged-in controller gets input control. Only
//! reached for a TEMPORARY-password login (`Event::LoggedIn::
//! requires_consent`) -- a permanent password is pre-authorized, unattended
//! access and skips this entirely; see `va_protocol::handshake`'s docs for
//! that distinction.
//!
//! `rfd`'s public API is fully safe (confirmed via its actual source, not
//! docs.rs: on Windows, absent the `common-controls-v6` feature this crate
//! doesn't enable, it calls the classic `MessageBoxW` -- every `unsafe`
//! block is internal to `rfd`'s own crate), matching the precedent
//! `windows-capture`/`enigo` already established for this workspace's
//! `unsafe_code = "forbid"` lint. `AsyncMessageDialog::show` already spawns
//! its own OS thread internally (a oneshot channel bridges it back to a
//! `Future`), so this is safe to `.await` directly from a tokio task --
//! no `spawn_blocking` needed here, unlike `DxgiDuplicationSource`'s setup.

use std::time::Duration;

use rfd::{AsyncMessageDialog, MessageButtons, MessageDialogResult, MessageLevel};

/// How long to wait for a real person to answer before assuming nobody's
/// there and failing closed. Matches the ballpark most attended-access
/// remote-support tools use for a consent prompt -- not a value cited from
/// any specific product, just a reasonable default for this pass.
const CONSENT_TIMEOUT: Duration = Duration::from_secs(30);

/// Shows a real native dialog asking whether to allow `device_id` to
/// control this machine's mouse and keyboard, and waits up to
/// `CONSENT_TIMEOUT` for an answer. Returns `true` only on an explicit
/// "Yes" -- a "No", a closed dialog, and a timeout with no answer all deny,
/// a fail-closed default appropriate for something this security-sensitive.
pub async fn ask_for_input_control(device_id: &str) -> bool {
    let dialog = AsyncMessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title("Verevon Support")
        .set_description(format!(
            "\"{device_id}\" is requesting to control this computer's mouse and keyboard.\n\nAllow?"
        ))
        .set_buttons(MessageButtons::YesNo);

    match tokio::time::timeout(CONSENT_TIMEOUT, dialog.show()).await {
        Ok(MessageDialogResult::Yes) => true,
        Ok(_) => false,
        Err(_elapsed) => {
            tracing::warn!(
                timeout = ?CONSENT_TIMEOUT,
                "no response to the input-control consent prompt in time -- denying"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `ask_for_input_control` itself is deliberately NOT unit-tested here:
    // it shows a real native dialog and blocks on a real human, which is
    // exactly what a unit test must never do. Live-tested manually instead
    // (see docs/architecture.md). This test only pins down the constant
    // that governs its fail-closed behavior, so a future edit can't
    // silently make the timeout unreasonably long or short without a test
    // noticing.
    #[test]
    fn consent_timeout_is_a_reasonable_bounded_duration() {
        assert!(CONSENT_TIMEOUT >= Duration::from_secs(10));
        assert!(CONSENT_TIMEOUT <= Duration::from_mins(2));
    }
}
