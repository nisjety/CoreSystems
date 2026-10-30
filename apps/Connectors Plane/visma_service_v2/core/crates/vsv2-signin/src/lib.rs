//! `vsv2-signin`: a person signs in to Visma once and the MCP server then acts as that person.
//!
//! Visma Connect issues an access token for ONE company per approval, so signing in to all of a person's
//! companies means approving each of them. [`login`] does that in one go: it opens the consent screen,
//! stores the result, and sends the same browser tab straight on to the next company until every known
//! company is connected. The tokens are kept per company ([`store`]) and [`UserTokens`] hands them to the
//! HTTP clients, refreshing them when they expire.

pub mod authorize;
pub mod browser;
pub mod config;
pub mod error;
pub mod exchange;
pub mod jwt;
pub mod login;
pub mod logout;
pub mod loopback;
pub mod pkce;
pub mod provider;
pub mod store;

#[cfg(test)]
mod testutil;

pub use config::{store_dir_from_env, store_dir_from_lookup, SignInConfig};
pub use error::{Result, SignInError};
pub use login::{login, Browser, Console, Finished, KnownCompany, LoginOptions, LoginReport};
pub use logout::{logout, LogoutReport};
pub use provider::{snapshot, Company, Snapshot, UserTokens};
pub use store::{Connection, Session, Store};
