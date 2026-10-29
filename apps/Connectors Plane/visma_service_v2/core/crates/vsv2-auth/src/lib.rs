//! `vsv2-auth` — Visma Connect OAuth client.
//!
//! Reuses the same Visma Connect endpoint as ERP (per locked decision).
//! DataMart is requested via a different scope on the same client_credentials
//! grant. Token cache is per `(tenant_id, logical_scope)`.
//!
//! [`TokenSource`] and [`Tokens`] are what the HTTP clients hold, so a signed-in person's tokens
//! (`vsv2-signin`) can stand in for the client credentials.

pub mod oauth;
pub mod scopes;
pub mod token_cache;
pub mod token_source;

pub use oauth::OAuthClient;
pub use scopes::Scope;
pub use token_cache::CachedToken;
pub use token_source::{TokenSource, Tokens};
