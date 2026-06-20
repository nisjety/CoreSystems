//! Ownable-vs-team resource taxonomy — Rust mirror of the canonical
//! `resource_taxonomy.json` owned by user-core. `scripts/lint-resource-taxonomy.sh`
//! asserts these lists stay byte-identical to the JSON so a resource type can
//! never be ownable in one plane and team-shared in another (a split-brain that
//! could ship a false privacy guarantee).

/// Resource types that may be user-owned / private and support per-user grants.
pub const OWNABLE: &[&str] = &["document"];

/// Resource types that are always org-scoped; private visibility and per-user
/// grants are rejected for them. Read by `scripts/lint-resource-taxonomy.sh`
/// (cross-language parity), not yet referenced in Rust code.
#[allow(dead_code)]
pub const TEAM_SHARED: &[&str] = &[
    "audit_log",
    "billing",
    "capability_registry",
    "conversation",
    "inbox",
    "org_settings",
    "quarry_run",
    "quarry_source",
    "ticket",
];

/// True when the resource type may be user-owned (so ownership filtering applies).
#[allow(dead_code)] // consumed as the taxonomy expands beyond documents
pub fn is_ownable(resource_type: &str) -> bool {
    OWNABLE.contains(&resource_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_disjoint_and_nonempty() {
        assert!(!OWNABLE.is_empty());
        assert!(!TEAM_SHARED.is_empty());
        for o in OWNABLE {
            assert!(!TEAM_SHARED.contains(o), "{o} is in BOTH sets");
        }
    }

    #[test]
    fn document_is_ownable_inbox_is_not() {
        assert!(is_ownable("document"));
        assert!(!is_ownable("inbox"));
    }
}
