//! Whose Visma authority the live tools use: the integration's service credentials, or the person who
//! signed in with `vsv2-mcp login`.
//!
//! A sign-in that exists but cannot be used (its application is not configured, the file cannot be read)
//! turns the live tools OFF with the reason. It never falls back to the service credentials, which can see
//! more than the person can.

use serde_json::{json, Value};
use vsv2_signin::{store_dir_from_lookup, SignInConfig, Store, UserTokens};

use crate::config::IdentityMode;

pub struct SignedIn {
    pub tokens: UserTokens,
    pub config: SignInConfig,
}

pub enum Resolved {
    Service,
    User(Box<SignedIn>),
    /// A sign-in is wanted, or exists, but cannot be used.
    Unusable(String),
}

pub fn resolve(mode: IdentityMode) -> Resolved {
    resolve_with(mode, &|key| std::env::var(key).ok())
}

/// [`resolve`] over any environment lookup, so the decision can be tested without touching the process.
pub fn resolve_with(mode: IdentityMode, get: &dyn Fn(&str) -> Option<String>) -> Resolved {
    if mode == IdentityMode::Service {
        return Resolved::Service;
    }
    let dir = match store_dir_from_lookup(get) {
        Ok(dir) => dir,
        // Where no home directory exists nobody can have signed in; in user mode that is the problem.
        Err(_) if mode == IdentityMode::Auto => return Resolved::Service,
        Err(e) => return Resolved::Unusable(format!("the sign-in store cannot be located: {e}")),
    };
    match Store::new(dir).session() {
        Ok(None) if mode == IdentityMode::Auto => Resolved::Service,
        Ok(None) => Resolved::Unusable("VSV2_MCP_IDENTITY=user but nobody is signed in: run `vsv2-mcp login`".into()),
        Err(e) => Resolved::Unusable(format!("the sign-in on this computer cannot be read: {e}")),
        Ok(Some(_)) => match SignInConfig::from_lookup(get).and_then(|config| Ok((UserTokens::open(&config)?, config))) {
            Ok((tokens, config)) => Resolved::User(Box::new(SignedIn { tokens, config })),
            Err(e) => Resolved::Unusable(format!(
                "someone is signed in on this computer but the sign-in cannot be used ({e}). Fix that, or run `vsv2-mcp logout` to use the service credentials"
            )),
        },
    }
}

impl SignedIn {
    /// The person's own name or e-mail address, else their id.
    pub fn person(&self) -> &str {
        self.tokens.session().display()
    }

    /// Who the audit log names: the person's id, which does not change, and the name Visma gave in brackets.
    pub fn audit_label(&self) -> String {
        let session = self.tokens.session();
        let sub = vsv2_common::text::one_line(&session.sub, 100);
        match &session.label {
            Some(label) => format!("{sub} ({label})"),
            None => sub,
        }
    }

    /// What `visma_init` says about whose authority the tools run with.
    pub fn acts_as(&self) -> String {
        let companies = self.tokens.companies().map(|c| c.len()).unwrap_or(0);
        format!(
            "you, {} (signed in to Visma Connect): live Visma reads and writes use your own sign-in, one token per \
             company ({companies} connected), so Visma applies your own roles and permissions and the tools can see only \
             the companies you connected. The warehouse tools use this computer's BigQuery login and visma_sync_status its \
             database, neither of them your Visma sign-in",
            self.person()
        )
    }

    /// The `identity` capability for `visma_init`: who, which companies, with which scopes.
    pub fn capability(&self, names: &dyn Fn(&str) -> Option<String>) -> Value {
        let companies: Vec<Value> = self
            .tokens
            .companies()
            .unwrap_or_default()
            .into_iter()
            .map(|c| {
                json!({
                    "tenant_id": c.tenant_id,
                    "name": names(&c.tenant_id),
                    "scopes": c.scopes,
                    "access_token_expires_at": c.access_expires_at,
                })
            })
            .collect();
        json!({
            "mode": "user",
            "user": self.person(),
            "user_id": self.tokens.session().sub,
            "companies": companies,
            "note": "to change who this server acts as: `vsv2-mcp login` adds companies, `vsv2-mcp logout` returns to the service credentials; restart the server (a new session) after either",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use chrono::Utc;
    use vsv2_signin::{Connection, Session};

    struct Machine {
        _dir: tempfile::TempDir,
        env: HashMap<String, String>,
        store: Store,
    }

    /// A computer with a store directory and, optionally, the sign-in application configured.
    fn machine(configured: bool) -> Machine {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = dir.path().join("user");
        let mut env = HashMap::from([(
            "VSV2_USER_STORE_DIR".to_string(),
            store_dir.display().to_string(),
        )]);
        if configured {
            env.insert("VISMA_USER_CLIENT_ID".into(), "web-app".into());
            env.insert("VISMA_USER_CLIENT_SECRET".into(), "s3cret".into());
        }
        Machine {
            _dir: dir,
            env,
            store: Store::new(store_dir),
        }
    }

    impl Machine {
        fn sign_in(&self, client_id: &str) {
            self.store
                .save_session(&Session::new(
                    "user-1".into(),
                    Some("ima@example.com".into()),
                    client_id.into(),
                ))
                .unwrap();
            self.store
                .save_connection(&Connection::new(
                    "t-1".into(),
                    "user-1".into(),
                    vec!["vismanet_erp_interactive_api:read".into()],
                    "refresh".into(),
                    "access".into(),
                    Utc::now() + chrono::Duration::hours(1),
                ))
                .unwrap();
        }

        fn resolve(&self, mode: IdentityMode) -> Resolved {
            resolve_with(mode, &|key| self.env.get(key).cloned())
        }
    }

    fn reason(resolved: Resolved) -> String {
        match resolved {
            Resolved::Unusable(reason) => reason,
            Resolved::Service => panic!("expected an unusable sign-in, got the service identity"),
            Resolved::User(_) => panic!("expected an unusable sign-in, got a user"),
        }
    }

    #[test]
    fn the_service_identity_never_looks_at_the_sign_in() {
        let m = machine(true);
        m.sign_in("web-app");
        assert!(matches!(
            m.resolve(IdentityMode::Service),
            Resolved::Service
        ));
    }

    #[test]
    fn nobody_signed_in_is_the_service_in_auto_and_an_error_in_user_mode() {
        let m = machine(true);
        assert!(matches!(m.resolve(IdentityMode::Auto), Resolved::Service));
        assert!(reason(m.resolve(IdentityMode::User)).contains("vsv2-mcp login"));
    }

    #[test]
    fn a_working_sign_in_makes_the_tools_act_as_the_person() {
        let m = machine(true);
        m.sign_in("web-app");
        for mode in [IdentityMode::Auto, IdentityMode::User] {
            let Resolved::User(signed_in) = m.resolve(mode) else {
                panic!("{mode:?} should act as the person")
            };
            assert_eq!(signed_in.person(), "ima@example.com");
            let acts_as = signed_in.acts_as();
            assert!(
                acts_as.contains("ima@example.com") && acts_as.contains("1 connected"),
                "{acts_as}"
            );
            let capability =
                signed_in.capability(&|id| (id == "t-1").then(|| "Aquatiq AS".to_string()));
            assert_eq!(capability["mode"], "user");
            assert_eq!(capability["companies"][0]["name"], "Aquatiq AS");
            assert!(
                !capability.to_string().contains("refresh")
                    && !capability.to_string().contains("s3cret")
            );
        }
    }

    #[test]
    fn a_sign_in_that_cannot_be_used_turns_the_tools_off_instead_of_widening_them() {
        // Signed in, but the application is not configured on this computer.
        let unconfigured = machine(false);
        unconfigured.sign_in("web-app");
        for mode in [IdentityMode::Auto, IdentityMode::User] {
            let why = reason(unconfigured.resolve(mode));
            assert!(
                why.contains("VISMA_USER_CLIENT_ID") && why.contains("vsv2-mcp logout"),
                "{why}"
            );
        }
        // Signed in with another application than the configured one.
        let other_app = machine(true);
        other_app.sign_in("another-app");
        assert!(reason(other_app.resolve(IdentityMode::Auto)).contains("another application"));
        // A session file that is not readable.
        let broken = machine(true);
        broken.sign_in("web-app");
        std::fs::write(broken.store.dir().join("user.json"), b"{ not json").unwrap();
        assert!(reason(broken.resolve(IdentityMode::Auto)).contains("cannot be read"));
    }
}
