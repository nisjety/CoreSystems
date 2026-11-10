//! E3.1-d: the executor's [`OtpFillTarget`] over one resolved browser target.
//!
//! The `fill_otp_ref` arm of the observation executor resolves the snapshot target to
//! its native binding, runs the read-only precheck (`BrowserDriver::otp_fill_precheck`,
//! E3.1-c), and then wraps the result here. [`crate::otp_source::deliver_otp`] calls the
//! three methods in order: the loader id, the trusted insert, and the credential-delivered
//! mark. The driver checks the preconditions again inside the insert, so a page that
//! changed after the precheck is refused there.
//!
//! This adapter holds no code. The only value it passes on is the `&str` the insert
//! receives, borrowed from the [`OtpCode`] that `deliver_otp` owns and drops.
//!
//! Flag: `QUARRY_EDGE__FILL_OTP`, default off. The executor checks it before it builds
//! this adapter.

use async_trait::async_trait;
use quarry_browser::{BrowserDriver, BrowserNativeTarget, BrowserSession, OtpFillPrecheck};
use quarry_core::error::QuarryResult;

use crate::otp_source::{OtpCode, OtpFillTarget};

/// One resolved, precondition-checked input of a `fill_otp_ref` step. It is built for one
/// step and dropped with it.
pub struct BrowserOtpFillTarget<'a> {
    browser: &'a dyn BrowserDriver,
    session: &'a BrowserSession,
    target: &'a BrowserNativeTarget,
    precheck: OtpFillPrecheck,
}

impl<'a> BrowserOtpFillTarget<'a> {
    /// Wrap the target whose preconditions `precheck` describes.
    pub fn new(
        browser: &'a dyn BrowserDriver,
        session: &'a BrowserSession,
        target: &'a BrowserNativeTarget,
        precheck: OtpFillPrecheck,
    ) -> Self {
        Self {
            browser,
            session,
            target,
            precheck,
        }
    }
}

#[async_trait]
impl OtpFillTarget for BrowserOtpFillTarget<'_> {
    fn loader_id(&self) -> &str {
        &self.precheck.loader_id
    }

    async fn insert_trusted(&self, code: &OtpCode) -> QuarryResult<()> {
        self.browser
            .otp_insert_trusted(
                self.session,
                self.target,
                &self.precheck,
                code.expose_for_insert(),
            )
            .await
    }

    async fn mark_credential_delivered(&self, loader_id: &str) -> QuarryResult<()> {
        self.browser
            .mark_otp_credential_delivered(self.session, loader_id)
            .await
    }
}
