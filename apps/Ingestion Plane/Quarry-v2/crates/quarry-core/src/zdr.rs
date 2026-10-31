//! Zero Data Retention policy enforcement.
//!
//! When `zdr=true`, Quarry must not persist any durable artifacts, cache
//! entries, or event history. This module provides the guard that every
//! I/O-bearing path must consult before writing.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, QuarryError, QuarryResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZdrMode {
    #[default]
    Off,
    On,
}

/// The span field the process log reads to know a request runs under ZDR.
/// A span that wants to carry it declares `zdr = tracing::field::Empty`.
pub const ZDR_SPAN_FIELD: &str = "zdr";

impl ZdrMode {
    pub fn is_active(self) -> bool {
        self == ZdrMode::On
    }

    /// Record `zdr = true` on the current span when this mode is active, so the
    /// log filter can drop events that carry a URL, query or error text from
    /// here on (and from every child span). A no-op when ZDR is off, when there
    /// is no current span, or when the span did not declare the field.
    ///
    /// The mark is set by the earliest code that learns the posture and is
    /// never lowered: a later `record(zdr, false)` does not clear it.
    pub fn mark_current_span(self) {
        if self.is_active() {
            tracing::Span::current().record(ZDR_SPAN_FIELD, true);
        }
    }
}

impl From<bool> for ZdrMode {
    fn from(v: bool) -> Self {
        if v {
            ZdrMode::On
        } else {
            ZdrMode::Off
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    Artifact,
    Cache,
    Event,
    Ingest,
    Index,
}

impl WriteKind {
    fn label(self) -> &'static str {
        match self {
            WriteKind::Artifact => "artifact",
            WriteKind::Cache => "cache",
            WriteKind::Event => "event",
            WriteKind::Ingest => "ingest",
            WriteKind::Index => "index",
        }
    }
}

pub fn guard(zdr: ZdrMode, kind: WriteKind) -> QuarryResult<()> {
    if zdr.is_active() {
        Err(QuarryError::new(
            ErrorCode::Forbidden,
            format!("ZDR active: {} write denied", kind.label()),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zdr_off_allows_all_writes() {
        for kind in [
            WriteKind::Artifact,
            WriteKind::Cache,
            WriteKind::Event,
            WriteKind::Ingest,
            WriteKind::Index,
        ] {
            assert!(guard(ZdrMode::Off, kind).is_ok());
        }
    }

    #[test]
    fn zdr_on_blocks_all_writes() {
        for kind in [
            WriteKind::Artifact,
            WriteKind::Cache,
            WriteKind::Event,
            WriteKind::Ingest,
            WriteKind::Index,
        ] {
            let err = guard(ZdrMode::On, kind).unwrap_err();
            assert_eq!(err.code, ErrorCode::Forbidden);
        }
    }

    #[test]
    fn from_bool() {
        assert_eq!(ZdrMode::from(true), ZdrMode::On);
        assert_eq!(ZdrMode::from(false), ZdrMode::Off);
    }
}
