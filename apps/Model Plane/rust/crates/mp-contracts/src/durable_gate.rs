//! The gate in front of every durable loop store (build plan WS03 S3 corrections).
//!
//! The durable loop writes content: a fold holds conversation text, an approval
//! continuation holds a retained call. Only a run that **keeps state** may do
//! that. Three postures exist ([`crate::retention`]): a zero-retention run keeps
//! nothing, a persistent run keeps what the plane keeps, and a *waived* run is a
//! zero-retention org running one connection with ZDR off for that run only.
//! A waiver is an exception for inference routing and nothing else: it does not
//! make the run a place to park content, so a waived run is refused here exactly
//! like a zero-retention run. Before this gate existed the check was written as
//! "not ZDR", and a waived run (whose `zdr` flag is false) slipped through it.
//!
//! One vocabulary, not a parallel one: the decision is the resolver's own
//! [`Resolution`], the class strings are the stored `runs.retention_class`
//! spellings ([`Resolution::retention_class`]), and the refusal is the shared
//! typed [`Refusal`] with the catalogue feature names `agent_run` and
//! `approval_continuation`.
//!
//! Two forms of the same rule, so the database predicate and the resolver
//! cannot drift: [`refuse_unless_persistent`] reads a decision made from the
//! caller's credentials, and [`refuse_unless_persistent_class`] reads the class a
//! run stored. Both are `Ok` for `persistent` and for nothing else.

use crate::retention::Resolution;
use crate::zdr_refusal::Refusal;

/// The retention class string that may use a durable store: `runs.retention_class`.
const PERSISTENT_CLASS: &str = "persistent";

/// A durable store of the loop, named so a refusal says which one it guarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableStore {
    /// The loop fold (a run's saved conversation state).
    Fold,
    /// The approval consume (a retained call handed to the loop).
    Consume,
    /// The `/continue` of a run after an approval.
    Continue,
    /// A parked run (a run waiting on a system condition, resumed later).
    Park,
}

impl DurableStore {
    /// The catalogue feature a refusal names (`mp_contracts::zdr_refusal::FEATURES`).
    /// The fold and a park are parts of an agent run; the consume and the continue
    /// are parts of an approval's continuation.
    #[must_use]
    pub const fn feature(self) -> &'static str {
        match self {
            Self::Fold | Self::Park => "agent_run",
            Self::Consume | Self::Continue => "approval_continuation",
        }
    }
}

/// Refuse every decision except a persistent one.
///
/// * `Persistent` is `Ok`.
/// * `Refuse` is returned unchanged: the resolver already said why.
/// * `Zdr` and `Waived` are refused with the store's feature
///   ([`Refusal::feature_not_supported`]). A waived run is persistent for its
///   connection only; nothing durable may be written for it.
///
/// # Errors
/// The typed refusal.
pub fn refuse_unless_persistent(
    store: DurableStore,
    resolution: &Resolution<'_>,
) -> Result<(), Refusal> {
    match resolution {
        Resolution::Persistent { .. } => Ok(()),
        Resolution::Refuse(refusal) => Err(*refusal),
        Resolution::Zdr { .. } | Resolution::Waived { .. } => {
            Err(Refusal::feature_not_supported(store.feature()))
        }
    }
}

/// Refuse every stored retention class except `persistent` (`zdr`, `zdr_waived`
/// and anything unrecognised are refused: an unknown class is never a grant).
///
/// # Errors
/// The typed refusal.
pub fn refuse_unless_persistent_class(
    store: DurableStore,
    retention_class: &str,
) -> Result<(), Refusal> {
    if retention_class == PERSISTENT_CLASS {
        Ok(())
    } else {
        Err(Refusal::feature_not_supported(store.feature()))
    }
}

/// The `continuation_mode` a new approval carries: `loop` when the run keeps
/// state (`persistent`: not zero-retention and not waived) and the switch is on,
/// otherwise empty, which is the cold delivery worker exactly as before.
///
/// `persistent` is the caller's decision, never `!zdr` alone: a waived run's
/// `zdr` flag is false, and it must still get the cold mode.
#[must_use]
pub const fn continuation_mode_for(persistent: bool, enabled: bool) -> &'static str {
    if enabled && persistent {
        "loop"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retention::Reason;

    const STORES: [DurableStore; 4] = [
        DurableStore::Fold,
        DurableStore::Consume,
        DurableStore::Continue,
        DurableStore::Park,
    ];

    fn waived() -> Resolution<'static> {
        Resolution::Waived {
            provider: "openai-codex-subscription",
            connection: "conn_1",
            reason: Reason::ConnectionNotZdrCapable,
        }
    }

    #[test]
    fn only_a_persistent_decision_may_use_a_durable_store() {
        let persistent = Resolution::Persistent {
            reason: Reason::OrgZdrOff,
        };
        let zdr = Resolution::Zdr {
            reason: Reason::OrgZdr,
        };
        for store in STORES {
            assert_eq!(refuse_unless_persistent(store, &persistent), Ok(()));
            for refused in [zdr, waived()] {
                let refusal = refuse_unless_persistent(store, &refused)
                    .expect_err("a zdr or waived run keeps no durable state");
                assert_eq!(refusal.code(), "zdr_feature_not_supported");
                assert_eq!(refusal.feature(), Some(store.feature()));
            }
        }
    }

    #[test]
    fn a_resolver_refusal_is_returned_unchanged() {
        let refused = Resolution::Refuse(Refusal::POSTURE_MISMATCH);
        for store in STORES {
            assert_eq!(
                refuse_unless_persistent(store, &refused),
                Err(Refusal::POSTURE_MISMATCH)
            );
        }
    }

    #[test]
    fn the_class_form_is_ok_for_persistent_only_and_agrees_with_the_decision_form() {
        let decisions = [
            Resolution::Persistent {
                reason: Reason::OrgZdrOff,
            },
            Resolution::Zdr {
                reason: Reason::OrgZdr,
            },
            Resolution::Zdr {
                reason: Reason::ExplicitZdr,
            },
            waived(),
        ];
        for store in STORES {
            for decision in decisions {
                let class = decision.retention_class().expect("a served decision has a class");
                assert_eq!(
                    refuse_unless_persistent_class(store, class).is_ok(),
                    refuse_unless_persistent(store, &decision).is_ok(),
                    "{class}: the database predicate and the resolver must agree"
                );
            }
            for unknown in ["", "Persistent", "persistent ", "unknown", "zdr_waived", "zdr"] {
                assert!(
                    refuse_unless_persistent_class(store, unknown).is_err(),
                    "{unknown:?} is never a grant"
                );
            }
            assert!(refuse_unless_persistent_class(store, "persistent").is_ok());
        }
    }

    #[test]
    fn the_stores_name_the_catalogue_features_they_belong_to() {
        assert_eq!(DurableStore::Fold.feature(), "agent_run");
        assert_eq!(DurableStore::Park.feature(), "agent_run");
        assert_eq!(DurableStore::Consume.feature(), "approval_continuation");
        assert_eq!(DurableStore::Continue.feature(), "approval_continuation");
        for store in STORES {
            assert!(
                crate::zdr_refusal::feature(store.feature()).is_some(),
                "{store:?} names a feature outside the vocabulary"
            );
        }
    }

    #[test]
    fn a_loop_mode_needs_both_a_run_that_keeps_state_and_the_switch() {
        assert_eq!(continuation_mode_for(true, true), "loop");
        assert_eq!(continuation_mode_for(true, false), "");
        assert_eq!(
            continuation_mode_for(false, true),
            "",
            "a zdr or waived run keeps no fold"
        );
        assert_eq!(continuation_mode_for(false, false), "");
    }
}
