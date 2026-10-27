//! Space authorization seam for message appends (build plan P-SC1, X01).
//!
//! `AppendAs` says on whose authority `append_message_in_tx_as` writes. The
//! owner path is the one that existed before this seam. The member path is
//! reserved for WS01-S3 (`authorize_append`, `member_append_denial`), which adds
//! its gate here; until it lands the append core refuses `Member` outright, so a
//! member can never reach the owner path by default.

/// The authority an append is made under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppendAs {
    /// The thread's owner, or a service acting for the owner: every caller of
    /// `append_message_in_tx` today.
    Owner,
    /// A Space member who does not own the thread. Not yet reachable: refused
    /// by the append core until WS01-S3 lands the member gate.
    #[allow(dead_code)]
    Member,
}
