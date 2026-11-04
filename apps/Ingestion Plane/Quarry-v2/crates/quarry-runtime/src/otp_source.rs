//! E3.1-d: where a one-time code comes from, and the executor-only helper that
//! puts it into the page.
//!
//! An agent proposes `AgentAction::FillOtpRef { snapshot_id, generation, ref_id,
//! approval_grant_id }`: there is no value on the wire. The executor resolves the
//! target and checks the preconditions (E3.1-c, in quarry-browser), then calls
//! [`deliver_otp`] here. That helper reads the code from an [`OtpSource`] for one
//! mail connection, inserts it through trusted input (an [`OtpFillTarget`]), and
//! marks the target's loader credential-delivered so AX, DOM and screenshot reads
//! of that frame are denied until it navigates (E3.1-e).
//!
//! The code itself:
//! - lives only in an [`OtpCode`], which zeroes its buffer on drop and whose
//!   `Debug` prints `[redacted:otp]`; it has no `Display` and no `Serialize`;
//! - is never logged, never put in an error message and never put in a receipt:
//!   the receipt ([`OtpFillReceipt`]) carries the outcome `otp_filled` only.
//!
//! The real source is the integration-corev2 `mail.otp.read` client (E3.1-h,
//! audience `ingestion`, scope `integration:otp:read`). [`StubOtpSource`] serves
//! tests and the login fixture (`/otp-login` on the `test-site` server).
//!
//! Flag: `QUARRY_EDGE__FILL_OTP`, default off. The edge refuses `fill_otp_ref`
//! while it is off; [`deliver_otp`] also refuses (`Unsupported`) unless the caller
//! passes `enabled = true`, so a library caller cannot reach a source by accident.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::Serialize;
use zeroize::Zeroizing;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

/// The flag that turns one-time-code fills on. Same name and parsing as the edge.
pub const FILL_OTP_FLAG: &str = "QUARRY_EDGE__FILL_OTP";

/// What a receipt, a log line or an observation note shows in place of the code.
pub const OTP_REDACTION: &str = "[redacted:otp]";

/// The only outcome a successful fill records.
pub const OTP_FILLED_OUTCOME: &str = "otp_filled";

/// Shortest and longest code accepted. Real one-time codes are 4 to 10 characters;
/// anything else is refused before it can reach a page.
const MIN_CODE_LEN: usize = 4;
const MAX_CODE_LEN: usize = 10;

/// Whether `QUARRY_EDGE__FILL_OTP` is on in this process (`1`, `true`, `yes`, `on`).
/// Read per call; default off.
pub fn fill_otp_enabled() -> bool {
    std::env::var(FILL_OTP_FLAG)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Which code to read. Opaque ids only: never a mail address, never a subject line.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct OtpRequest {
    /// The org the run belongs to; the source must not read across orgs.
    pub org_id: String,
    /// The approved mail connection (integration-corev2 connection id).
    pub mail_connection_id: String,
    /// The single-use `fill_otp` approval grant this read is spent against.
    pub approval_grant_id: String,
    /// The https origin the code is for (the grant origin, checked by E3.1-c).
    pub origin: String,
}

/// A one-time code. The buffer is zeroed on drop; `Debug` never shows it; there is
/// no `Display`, no `Clone` and no `Serialize`, so it cannot reach a log, a receipt
/// or a response by formatting or serialising.
pub struct OtpCode(Zeroizing<String>);

impl OtpCode {
    /// Accept a code read from a source. Refused when it is not 4 to 10 ASCII letters
    /// or digits; the error never carries the value.
    pub fn new(value: impl Into<String>) -> QuarryResult<Self> {
        let value = Zeroizing::new(value.into());
        let trimmed_len = value.trim().len();
        let shape_ok = trimmed_len == value.len()
            && (MIN_CODE_LEN..=MAX_CODE_LEN).contains(&value.len())
            && value.bytes().all(|b| b.is_ascii_alphanumeric());
        if !shape_ok {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "one-time code from the source has an unexpected shape",
            )
            .with_details(serde_json::json!({ "reason": "otp_code_shape" })));
        }
        Ok(Self(value))
    }

    /// The code, for the trusted-input insert only. Callers must not copy it into
    /// anything that outlives the insert.
    pub fn expose_for_insert(&self) -> &str {
        self.0.as_str()
    }

    /// Length in characters (safe to log; the value is not).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always false for a constructed code (kept for clippy's `len_without_is_empty`).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for OtpCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(OTP_REDACTION)
    }
}

/// Where one-time codes come from. Executor-only: no agent, edge route or Model
/// Plane caller reads through this trait. Implementations must not log the value
/// or put it in an error message.
#[async_trait]
pub trait OtpSource: Send + Sync {
    /// Read the current code for the request's mail connection.
    async fn read_code(&self, request: &OtpRequest) -> QuarryResult<OtpCode>;
}

/// The executor's side of a fill: one resolved, precondition-checked target
/// (E3.1-c: https, frame origin equals top and grant origin, exactly one visible
/// required non-password input, no options and no QR).
#[async_trait]
pub trait OtpFillTarget: Send + Sync {
    /// The loader (document) id of the target's frame; the credential-delivered
    /// mark is keyed on it and cleared by navigation.
    fn loader_id(&self) -> &str;

    /// Insert the code through trusted input (CDP `Input.insertText` on the focused
    /// target), never by setting `value` from script.
    async fn insert_trusted(&self, code: &OtpCode) -> QuarryResult<()>;

    /// Mark the loader credential-delivered: AX, DOM and screenshot reads of that
    /// frame are denied until navigation (E3.1-e).
    async fn mark_credential_delivered(&self, loader_id: &str) -> QuarryResult<()>;
}

/// What a fill leaves behind. No value, no length, no field text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OtpFillReceipt {
    /// Always [`OTP_FILLED_OUTCOME`].
    pub outcome: &'static str,
    pub loader_id: String,
    pub approval_grant_id: String,
}

/// Read a code and put it into the target. Executor-only.
///
/// Order: refuse when `enabled` is false (no read); read the code; insert it through
/// trusted input; mark the loader credential-delivered. If the insert fails the mark
/// is still attempted, since part of the code may already be in the field (fail
/// closed), and the insert's error is returned. The code is dropped (and zeroed)
/// before the mark, and never appears in the receipt or in an error.
pub async fn deliver_otp(
    enabled: bool,
    source: &dyn OtpSource,
    target: &dyn OtpFillTarget,
    request: &OtpRequest,
) -> QuarryResult<OtpFillReceipt> {
    if !enabled {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "fill_otp is not enabled on this deployment (QUARRY_EDGE__FILL_OTP is off)",
        ));
    }
    let loader_id = target.loader_id().to_string();
    let code = source.read_code(request).await?;
    let inserted = target.insert_trusted(&code).await;
    drop(code);
    let marked = target.mark_credential_delivered(&loader_id).await;
    inserted?;
    marked?;
    tracing::info!(
        outcome = OTP_FILLED_OUTCOME,
        loader_id = %loader_id,
        approval_grant_id = %request.approval_grant_id,
        "one-time code delivered"
    );
    Ok(OtpFillReceipt {
        outcome: OTP_FILLED_OUTCOME,
        loader_id,
        approval_grant_id: request.approval_grant_id.clone(),
    })
}

/// A test source: fixed codes per mail connection, with a read counter. A
/// connection with no code is `NotFound`; [`StubOtpSource::failing`] makes every
/// read fail with the given code.
#[derive(Default)]
pub struct StubOtpSource {
    codes: Mutex<HashMap<String, String>>,
    fail_with: Option<ErrorCode>,
    reads: AtomicUsize,
}

impl StubOtpSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serve `code` for `mail_connection_id`.
    pub fn with_code(self, mail_connection_id: impl Into<String>, code: impl Into<String>) -> Self {
        self.codes
            .lock()
            .expect("stub otp codes")
            .insert(mail_connection_id.into(), code.into());
        self
    }

    /// Every read fails with `code`.
    pub fn failing(code: ErrorCode) -> Self {
        Self {
            fail_with: Some(code),
            ..Self::default()
        }
    }

    /// How many reads were made.
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl OtpSource for StubOtpSource {
    async fn read_code(&self, request: &OtpRequest) -> QuarryResult<OtpCode> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if let Some(code) = self.fail_with {
            return Err(QuarryError::new(code, "stub otp source refused the read"));
        }
        let value = self
            .codes
            .lock()
            .expect("stub otp codes")
            .get(&request.mail_connection_id)
            .cloned();
        match value {
            Some(v) => OtpCode::new(v),
            None => Err(QuarryError::new(
                ErrorCode::NotFound,
                "no one-time code for this mail connection",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture code: the same value the `/otp-login` page accepts.
    const CODE: &str = "482915";

    #[derive(Default)]
    struct RecordingTarget {
        inserted: Mutex<Vec<String>>,
        marked: Mutex<Vec<String>>,
        fail_insert: bool,
    }

    #[async_trait]
    impl OtpFillTarget for RecordingTarget {
        fn loader_id(&self) -> &str {
            "loader-1"
        }
        async fn insert_trusted(&self, code: &OtpCode) -> QuarryResult<()> {
            self.inserted
                .lock()
                .unwrap()
                .push(code.expose_for_insert().to_string());
            if self.fail_insert {
                return Err(QuarryError::new(ErrorCode::DriverFailed, "insert failed"));
            }
            Ok(())
        }
        async fn mark_credential_delivered(&self, loader_id: &str) -> QuarryResult<()> {
            self.marked.lock().unwrap().push(loader_id.to_string());
            Ok(())
        }
    }

    fn request() -> OtpRequest {
        OtpRequest {
            org_id: "org-1".into(),
            mail_connection_id: "mail-1".into(),
            approval_grant_id: "grant-1".into(),
            origin: "https://login.example.test".into(),
        }
    }

    #[tokio::test]
    async fn delivers_through_trusted_input_and_marks_the_loader() {
        let source = StubOtpSource::new().with_code("mail-1", CODE);
        let target = RecordingTarget::default();
        let receipt = deliver_otp(true, &source, &target, &request()).await.unwrap();
        assert_eq!(receipt.outcome, OTP_FILLED_OUTCOME);
        assert_eq!(receipt.loader_id, "loader-1");
        assert_eq!(receipt.approval_grant_id, "grant-1");
        assert_eq!(*target.inserted.lock().unwrap(), vec![CODE.to_string()]);
        assert_eq!(*target.marked.lock().unwrap(), vec!["loader-1".to_string()]);
        assert_eq!(source.reads(), 1);
    }

    #[tokio::test]
    async fn the_receipt_never_carries_the_code() {
        let source = StubOtpSource::new().with_code("mail-1", CODE);
        let target = RecordingTarget::default();
        let receipt = deliver_otp(true, &source, &target, &request()).await.unwrap();
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(!json.contains(CODE), "{json}");
        assert!(!format!("{receipt:?}").contains(CODE));
        assert!(json.contains(OTP_FILLED_OUTCOME));
    }

    #[tokio::test]
    async fn flag_off_refuses_before_any_read() {
        let source = StubOtpSource::new().with_code("mail-1", CODE);
        let target = RecordingTarget::default();
        let err = deliver_otp(false, &source, &target, &request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Unsupported);
        assert_eq!(source.reads(), 0);
        assert!(target.inserted.lock().unwrap().is_empty());
        assert!(target.marked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failed_insert_still_marks_the_loader_and_hides_the_code() {
        let source = StubOtpSource::new().with_code("mail-1", CODE);
        let target = RecordingTarget {
            fail_insert: true,
            ..RecordingTarget::default()
        };
        let err = deliver_otp(true, &source, &target, &request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
        assert_eq!(*target.marked.lock().unwrap(), vec!["loader-1".to_string()]);
        assert!(!format!("{err:?}").contains(CODE));
    }

    #[tokio::test]
    async fn a_source_error_inserts_and_marks_nothing() {
        let source = StubOtpSource::failing(ErrorCode::Forbidden);
        let target = RecordingTarget::default();
        let err = deliver_otp(true, &source, &target, &request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(target.inserted.lock().unwrap().is_empty());
        assert!(target.marked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_connection_is_not_found() {
        let source = StubOtpSource::new().with_code("other", CODE);
        let err = source.read_code(&request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn debug_of_a_code_is_redacted() {
        let code = OtpCode::new(CODE).unwrap();
        assert_eq!(format!("{code:?}"), OTP_REDACTION);
        assert_eq!(code.len(), CODE.len());
        assert!(!code.is_empty());
    }

    #[test]
    fn badly_shaped_codes_are_refused_without_echoing_them() {
        for bad in ["", "12", "12345678901", " 4829", "48-29", "4829\n"] {
            let err = OtpCode::new(bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest);
            if !bad.trim().is_empty() {
                assert!(!err.message.contains(bad.trim()), "{bad:?}");
            }
        }
    }
}
