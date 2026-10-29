/// Logical scope categories. Resolved to wire scope strings via config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Erp,
    Datamart,
    Expense,
    SalesOrder,
    /// Layer 3 only: Finance interactive API create/update. Never requested by read syncs.
    ErpWrite,
    /// Layer 3 only: SalesOrder V3 create/update.
    SalesOrderWrite,
}

impl Scope {
    pub const fn as_key(&self) -> &'static str {
        match self {
            Scope::Erp => "erp",
            Scope::Datamart => "datamart",
            Scope::Expense => "expense",
            Scope::SalesOrder => "salesorder",
            Scope::ErpWrite => "erp_write",
            Scope::SalesOrderWrite => "salesorder_write",
        }
    }
}

impl Scope {
    /// Scopes that may change data. Layer 2 (read) code must never request these.
    pub const fn is_write(&self) -> bool {
        matches!(self, Scope::ErpWrite | Scope::SalesOrderWrite)
    }
}

#[cfg(test)]
mod tests {
    use super::Scope;

    #[test]
    fn only_the_layer3_scopes_are_write_scopes() {
        for read in [
            Scope::Erp,
            Scope::Datamart,
            Scope::Expense,
            Scope::SalesOrder,
        ] {
            assert!(!read.is_write(), "{read:?}");
        }
        assert!(Scope::ErpWrite.is_write());
        assert!(Scope::SalesOrderWrite.is_write());
    }

    #[test]
    fn scope_keys_are_unique() {
        let all = [
            Scope::Erp,
            Scope::Datamart,
            Scope::Expense,
            Scope::SalesOrder,
            Scope::ErpWrite,
            Scope::SalesOrderWrite,
        ];
        let keys: std::collections::HashSet<_> = all.iter().map(|s| s.as_key()).collect();
        assert_eq!(keys.len(), all.len());
    }
}
