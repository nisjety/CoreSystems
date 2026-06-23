//! Per-resource ownership + sharing for the gateway registries (MCP servers,
//! plugins, commands, hooks).
//!
//! This sits BELOW org/tenant isolation: every registry is already keyed by
//! `(org_id, resource_id)`, so ownership never crosses tenants. Within an org a
//! resource is one of:
//!   - **org-scoped**: created by an admin, visible to ALL org members.
//!   - **user-scoped**: owned by its creator, PRIVATE by default — visible only
//!     to the owner, users it was explicitly shared with, and (once shared) org
//!     admins. Admins see SHARED user resources for governance, but NEVER a
//!     private (unshared) one.
//!
//! A user may share their own resources with specific users but can NEVER make
//! one org-wide; only an admin creates org-scoped resources. The admin GATE on
//! org-scope creation is enforced authoritatively at the BFF (which knows the
//! session role); the gateway defends in depth via [`is_admin_claim`].

use std::sync::Arc;

use dashmap::DashMap;

/// Registry `kind` discriminators (the middle component of the ownership key).
pub const KIND_MCP: &str = "mcp";
pub const KIND_PLUGIN: &str = "plugin";
pub const KIND_COMMAND: &str = "command";
pub const KIND_HOOK: &str = "hook";

/// Resource scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Admin-created, visible to all members of the org.
    Org,
    /// User-owned, private until explicitly shared.
    User,
}

impl Scope {
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        if s.eq_ignore_ascii_case("org") {
            Self::Org
        } else {
            Self::User
        }
    }

    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Org => "org",
            Self::User => "user",
        }
    }
}

/// Ownership record for one resource.
#[derive(Clone, Debug)]
pub struct Ownership {
    pub scope: Scope,
    /// Empty for org-scoped resources.
    pub owner_user_id: String,
    /// Explicit grantees (user ids) for a user-scoped resource.
    pub shared_with: Vec<String>,
}

impl Ownership {
    #[must_use]
    pub fn org() -> Self {
        Self {
            scope: Scope::Org,
            owner_user_id: String::new(),
            shared_with: Vec::new(),
        }
    }

    #[must_use]
    pub fn user(owner_user_id: impl Into<String>) -> Self {
        Self {
            scope: Scope::User,
            owner_user_id: owner_user_id.into(),
            shared_with: Vec::new(),
        }
    }

    /// Whether the resource has been shared with at least one user.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        !self.shared_with.is_empty()
    }

    /// Can `user_id` actually USE this resource (e.g. the agent calling its
    /// tool)? Org resources are usable by everyone in the org; a user resource
    /// is usable only by its owner or an explicit grantee. Admin role does NOT
    /// widen this — an admin's agent must not silently use every shared resource
    /// in the org. Tenant scoping is already enforced by the registry key.
    #[must_use]
    pub fn usable_by(&self, user_id: &str) -> bool {
        match self.scope {
            Scope::Org => true,
            Scope::User => {
                self.owner_user_id == user_id || self.shared_with.iter().any(|u| u == user_id)
            }
        }
    }

    /// Can `(user_id, is_admin)` SEE this resource in a management/listing view?
    /// Everything [`usable_by`] allows, PLUS: an admin additionally sees SHARED
    /// user resources for governance — but never a private (unshared) one.
    #[must_use]
    pub fn visible_to(&self, user_id: &str, is_admin: bool) -> bool {
        self.usable_by(user_id) || (is_admin && self.is_shared())
    }

    /// Who may share/unshare/delete this resource: the owner (user-scoped), or an
    /// admin (org-scoped). A user can never mutate another user's resource, and
    /// an admin cannot touch a private (unshared) user resource — they cannot
    /// even see it.
    #[must_use]
    pub fn can_modify(&self, user_id: &str, is_admin: bool) -> bool {
        match self.scope {
            Scope::Org => is_admin,
            Scope::User => self.owner_user_id == user_id,
        }
    }
}

/// `(org_id, kind, resource_id)` → ownership. `kind` distinguishes registries
/// (`"mcp"`, `"plugin"`, `"command"`, `"hook"`) so one store serves all four.
type OwnershipKey = (String, String, String);

/// Gateway-scoped ownership sidecar. Kept beside the registries (which hold the
/// resource payloads) so the proto resource messages don't each need new fields;
/// the durable record rides along in the capability-core write-through.
#[derive(Clone, Default)]
pub struct OwnershipStore {
    inner: Arc<DashMap<OwnershipKey, Ownership>>,
}

impl OwnershipStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    fn key(org_id: &str, kind: &str, resource_id: &str) -> OwnershipKey {
        (org_id.to_owned(), kind.to_owned(), resource_id.to_owned())
    }

    pub fn set(&self, org_id: &str, kind: &str, resource_id: &str, ownership: Ownership) {
        self.inner
            .insert(Self::key(org_id, kind, resource_id), ownership);
    }

    pub fn remove(&self, org_id: &str, kind: &str, resource_id: &str) {
        self.inner.remove(&Self::key(org_id, kind, resource_id));
    }

    #[must_use]
    pub fn get(&self, org_id: &str, kind: &str, resource_id: &str) -> Option<Ownership> {
        self.inner
            .get(&Self::key(org_id, kind, resource_id))
            .map(|e| e.value().clone())
    }

    /// Can `user_id` USE the resource (agent tool exposure)? A resource with
    /// **no** ownership record is grandfathered as org-usable (pre-ownership
    /// resources are never suddenly hidden — matches the ownership-plan
    /// grandfather rule).
    #[must_use]
    pub fn usable(&self, org_id: &str, kind: &str, resource_id: &str, user_id: &str) -> bool {
        self.get(org_id, kind, resource_id)
            .is_none_or(|o| o.usable_by(user_id))
    }

    /// Is the resource visible to `(user_id, is_admin)` in a management view? A
    /// resource with **no** ownership record is grandfathered as org-visible.
    #[must_use]
    pub fn visible(
        &self,
        org_id: &str,
        kind: &str,
        resource_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> bool {
        self.get(org_id, kind, resource_id)
            .is_none_or(|o| o.visible_to(user_id, is_admin))
    }

    /// May `(user_id, is_admin)` mutate (share/delete/disable) the resource? A
    /// resource with no ownership record is treated as org-managed → admin only.
    #[must_use]
    pub fn can_modify(
        &self,
        org_id: &str,
        kind: &str,
        resource_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> bool {
        self.get(org_id, kind, resource_id)
            .map_or(is_admin, |o| o.can_modify(user_id, is_admin))
    }

    /// Replace the grantee list of a user-scoped resource (owner only).
    ///
    /// # Errors
    /// Returns `Err` if the resource is unknown, is org-scoped, or the caller is
    /// not the owner.
    pub fn set_shares(
        &self,
        org_id: &str,
        kind: &str,
        resource_id: &str,
        caller_user_id: &str,
        grantees: Vec<String>,
    ) -> Result<Ownership, String> {
        let mut entry = self
            .inner
            .get_mut(&Self::key(org_id, kind, resource_id))
            .ok_or_else(|| "resource not found".to_owned())?;
        if entry.scope != Scope::User {
            return Err("only user-owned resources can be shared".to_owned());
        }
        if entry.owner_user_id != caller_user_id {
            return Err("only the owner may share this resource".to_owned());
        }
        // De-dup, drop blanks, and never list the owner as a grantee.
        let mut clean: Vec<String> = Vec::new();
        for g in grantees {
            let g = g.trim().to_owned();
            if g.is_empty() || g == entry.owner_user_id || clean.contains(&g) {
                continue;
            }
            clean.push(g);
        }
        entry.shared_with = clean;
        Ok(entry.clone())
    }
}

/// Derive admin status for a request. Authoritative org-role lives in the
/// Control Plane, so the gateway accepts either an admin marker in the verified
/// token `scopes` (e.g. `org:admin`) or a BFF-forwarded `x-user-role: admin`
/// header (trusted on the internal bus, same as the forwarded `x-user-id`).
#[must_use]
pub fn is_admin_claim(scopes: &[String], role_header: Option<&str>) -> bool {
    let scope_admin = scopes.iter().any(|s| {
        let s = s.as_str();
        s == "admin" || s == "org:admin" || s.ends_with(":admin")
    });
    let header_admin = role_header.is_some_and(|r| r.eq_ignore_ascii_case("admin"));
    scope_admin || header_admin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_scope_visible_to_all_members() {
        let o = Ownership::org();
        assert!(o.visible_to("anyone", false));
        assert!(o.visible_to("other", true));
    }

    #[test]
    fn private_user_resource_owner_only() {
        let o = Ownership::user("alice");
        assert!(o.visible_to("alice", false));
        assert!(!o.visible_to("bob", false));
        // Admin does NOT see a private (unshared) resource.
        assert!(!o.visible_to("admin", true));
    }

    #[test]
    fn shared_resource_visible_to_grantee_and_admin_not_others() {
        let mut o = Ownership::user("alice");
        o.shared_with = vec!["bob".to_owned()];
        assert!(o.visible_to("alice", false)); // owner
        assert!(o.visible_to("bob", false)); // grantee
        assert!(o.visible_to("admin", true)); // any admin sees a SHARED resource
        assert!(!o.visible_to("carol", false)); // unrelated non-admin: no
        // USE semantics are narrower: an admin's agent does NOT get to use a
        // shared resource it wasn't granted; only owner + grantee can use it.
        assert!(o.usable_by("alice"));
        assert!(o.usable_by("bob"));
        assert!(!o.usable_by("carol")); // admin role gives no extra USE access
    }

    #[test]
    fn modify_rules() {
        let user = Ownership::user("alice");
        assert!(user.can_modify("alice", false)); // owner
        assert!(!user.can_modify("bob", false)); // not owner
        assert!(!user.can_modify("admin", true)); // admin can't touch a user resource
        let org = Ownership::org();
        assert!(org.can_modify("admin", true)); // admin manages org
        assert!(!org.can_modify("alice", false)); // member can't
    }

    #[test]
    fn store_grandfathers_unknown_as_org_visible() {
        let store = OwnershipStore::new();
        assert!(store.visible("o", "mcp", "legacy", "anyone", false));
        assert!(store.can_modify("o", "mcp", "legacy", "anyone", true)); // admin
    }

    #[test]
    fn store_share_owner_only_and_dedups() {
        let store = OwnershipStore::new();
        store.set("o", "mcp", "s1", Ownership::user("alice"));
        // wrong owner rejected
        assert!(store
            .set_shares("o", "mcp", "s1", "bob", vec!["x".into()])
            .is_err());
        // owner shares; owner-self + blanks + dups dropped
        let updated = store
            .set_shares(
                "o",
                "mcp",
                "s1",
                "alice",
                vec![
                    "bob".into(),
                    "bob".into(),
                    String::new(),
                    "alice".into(),
                    "carol".into(),
                ],
            )
            .expect("owner can share");
        assert_eq!(updated.shared_with, vec!["bob".to_owned(), "carol".to_owned()]);
        assert!(store.visible("o", "mcp", "s1", "bob", false));
        assert!(store.visible("o", "mcp", "s1", "admin", true));
        assert!(!store.visible("o", "mcp", "s1", "dave", false));
    }

    #[test]
    fn is_admin_claim_reads_scopes_and_header() {
        assert!(is_admin_claim(&["org:admin".to_owned()], None));
        assert!(is_admin_claim(&["admin".to_owned()], None));
        assert!(is_admin_claim(&[], Some("admin")));
        assert!(is_admin_claim(&[], Some("Admin")));
        assert!(!is_admin_claim(&["member".to_owned()], Some("member")));
        assert!(!is_admin_claim(&[], None));
    }
}
