//! `EventFilter` domain type + validation.
//!
//! Mirrors the proto `EventFilter` shape but with Rust-native invariants:
//!   - `org_id` is required for any QueryEvents / SubscribeEvents operation
//!     to prevent cross-tenant leak-by-omission.
//!   - `event_types` is a bitmask set; the zero set means "no filter".
//!   - `run_id` without `org_id` is rejected so callers can't drift into a
//!     global scan by skipping `org_id`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::error::{EventLogError, EventLogResult};

/// A set of event-type discriminator ints. We avoid coupling this crate to
/// the proto `EventType` enum; consumers pass the numeric discriminator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTypeMask(BTreeSet<u32>);

impl EventTypeMask {
    /// Empty mask = "accept all types".
    #[must_use]
    pub fn any() -> Self {
        Self(BTreeSet::new())
    }

    /// Build a mask from discriminator values. Caller-side dedup is free.
    pub fn from_iter<I: IntoIterator<Item = u32>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }

    /// Is this mask a wildcard (empty set)?
    #[must_use]
    pub fn is_any(&self) -> bool {
        self.0.is_empty()
    }

    /// Does `ty` match?
    #[must_use]
    pub fn matches(&self, ty: u32) -> bool {
        self.0.is_empty() || self.0.contains(&ty)
    }

    /// Iterate the discriminators in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().copied()
    }

    /// Count of types in the mask (0 = wildcard).
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True iff no types are set (i.e., wildcard).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Filter applied to QueryEvents / SubscribeEvents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventFilter {
    /// Tenant. Always required for validation to pass.
    pub org_id: String,
    /// Run scope. Empty = all runs under `org_id`.
    pub run_id: String,
    /// Event-type discriminator mask. Empty = all types.
    pub event_types: EventTypeMask,
    /// Correlation id to pin to. Empty = any.
    pub correlation_id: String,
    /// Resource reference (e.g. `thread/{id}`). Empty = any.
    pub resource_ref: String,
}

impl EventFilter {
    /// Validate cross-field invariants. Callers MUST invoke this before
    /// accepting a filter from the wire.
    ///
    /// # Errors
    /// - `MissingField("org_id")` when `org_id` is empty.
    /// - `InvalidField` for shape violations (too-long tenant/run/etc).
    pub fn validate(&self) -> EventLogResult<()> {
        if self.org_id.is_empty() {
            return Err(EventLogError::MissingField { field: "org_id" });
        }
        if self.org_id.len() > 128 {
            return Err(EventLogError::InvalidField {
                field: "org_id",
                detail: format!("length {} exceeds 128", self.org_id.len()),
            });
        }
        if self.run_id.len() > 64 {
            return Err(EventLogError::InvalidField {
                field: "run_id",
                detail: format!("length {} exceeds 64", self.run_id.len()),
            });
        }
        if self.correlation_id.len() > 128 {
            return Err(EventLogError::InvalidField {
                field: "correlation_id",
                detail: format!("length {} exceeds 128", self.correlation_id.len()),
            });
        }
        if self.resource_ref.len() > 256 {
            return Err(EventLogError::InvalidField {
                field: "resource_ref",
                detail: format!("length {} exceeds 256", self.resource_ref.len()),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> EventFilter {
        EventFilter {
            org_id: "org_1".into(),
            run_id: String::new(),
            event_types: EventTypeMask::any(),
            correlation_id: String::new(),
            resource_ref: String::new(),
        }
    }

    #[test]
    fn default_mask_accepts_all() {
        let m = EventTypeMask::any();
        assert!(m.is_any());
        assert!(m.matches(1));
        assert!(m.matches(9999));
    }

    #[test]
    fn specific_mask_accepts_only_declared() {
        let m = EventTypeMask::from_iter([10, 20]);
        assert!(!m.is_any());
        assert!(m.matches(10));
        assert!(!m.matches(11));
        assert!(m.matches(20));
    }

    #[test]
    fn mask_roundtrips_json_sorted() {
        let m = EventTypeMask::from_iter([30, 10, 20, 10]);
        let s = serde_json::to_string(&m).unwrap();
        let back: EventTypeMask = serde_json::from_str(&s).unwrap();
        assert_eq!(back.iter().collect::<Vec<_>>(), vec![10, 20, 30]);
    }

    #[test]
    fn validate_requires_org_id() {
        let mut f = base();
        f.org_id.clear();
        assert_eq!(f.validate().unwrap_err().code(), "MISSING_FIELD");
    }

    #[test]
    fn validate_caps_field_lengths() {
        let mut f = base();
        f.run_id = "r".repeat(65);
        let err = f.validate().unwrap_err();
        assert_eq!(err.code(), "INVALID_FIELD");
        assert!(err.to_string().contains("run_id"));
    }

    #[test]
    fn filter_roundtrips_json() {
        let f = EventFilter {
            org_id: "org_1".into(),
            run_id: "run_1".into(),
            event_types: EventTypeMask::from_iter([5, 10, 50]),
            correlation_id: "corr".into(),
            resource_ref: "thread/abc".into(),
        };
        let s = serde_json::to_string(&f).unwrap();
        let back: EventFilter = serde_json::from_str(&s).unwrap();
        assert_eq!(back, f);
        assert!(back.validate().is_ok());
    }
}
