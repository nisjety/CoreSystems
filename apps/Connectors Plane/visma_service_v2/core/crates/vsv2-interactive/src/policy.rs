use std::collections::HashSet;

use vsv2_common::TenantId;

use crate::error::{InteractiveError, Result};

/// Allow-list for writes. Default is deny-all; a write needs its tenant *and* its operation listed.
/// There is no wildcard on purpose.
#[derive(Debug, Clone, Default)]
pub struct WritePolicy {
    tenants: HashSet<String>,
    operations: HashSet<String>,
}

impl WritePolicy {
    pub fn deny_all() -> Self {
        Self::default()
    }

    pub fn new<T, O>(tenants: T, operations: O) -> Self
    where
        T: IntoIterator,
        T::Item: Into<String>,
        O: IntoIterator,
        O::Item: Into<String>,
    {
        Self {
            tenants: tenants.into_iter().map(Into::into).collect(),
            operations: operations.into_iter().map(Into::into).collect(),
        }
    }

    /// Parse comma-separated env lists (`VSV2_WRITE_ENABLED_TENANTS`, `VSV2_WRITE_ENABLED_OPERATIONS`).
    pub fn from_lists(tenants: &str, operations: &str) -> Self {
        let split = |s: &str| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty() && *x != "*")
                .map(String::from)
                .collect::<Vec<_>>()
        };
        Self::new(split(tenants), split(operations))
    }

    pub fn from_env() -> Self {
        let get = |k| std::env::var(k).unwrap_or_default();
        Self::from_lists(
            &get("VSV2_WRITE_ENABLED_TENANTS"),
            &get("VSV2_WRITE_ENABLED_OPERATIONS"),
        )
    }

    /// Allow-listed tenant ids, sorted (for reporting; the allow-list itself is not editable).
    pub fn tenants(&self) -> Vec<String> {
        let mut v: Vec<String> = self.tenants.iter().cloned().collect();
        v.sort();
        v
    }

    /// Allow-listed operation names, sorted.
    pub fn operations(&self) -> Vec<String> {
        let mut v: Vec<String> = self.operations.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn check(&self, tenant: &TenantId, operation: &str) -> Result<()> {
        if !self.tenants.contains(tenant.as_str()) {
            return Err(InteractiveError::Denied(format!(
                "writes are not enabled for tenant {tenant}"
            )));
        }
        if !self.operations.contains(operation) {
            return Err(InteractiveError::Denied(format!(
                "operation {operation} is not enabled"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> TenantId {
        TenantId::new(s)
    }

    #[test]
    fn default_denies_everything() {
        assert!(WritePolicy::deny_all().check(&t("a"), "op").is_err());
    }

    #[test]
    fn needs_both_tenant_and_operation() {
        let p = WritePolicy::new(["a"], ["op"]);
        assert!(p.check(&t("a"), "op").is_ok());
        assert!(p.check(&t("b"), "op").is_err());
        assert!(p.check(&t("a"), "other").is_err());
    }

    #[test]
    fn reports_its_allow_lists_sorted() {
        let p = WritePolicy::new(["b", "a"], ["op2", "op1"]);
        assert_eq!(
            (p.tenants(), p.operations()),
            (
                vec!["a".to_string(), "b".into()],
                vec!["op1".to_string(), "op2".into()]
            )
        );
        assert!(WritePolicy::deny_all().tenants().is_empty());
    }

    #[test]
    fn wildcard_is_ignored_not_honoured() {
        let p = WritePolicy::from_lists("*", " * ");
        assert!(p.check(&t("a"), "op").is_err());
        let p = WritePolicy::from_lists("a, b ,", "op1,op2");
        assert!(p.check(&t("b"), "op2").is_ok());
    }
}
