//! The `healthcheck` subcommand: prove auth + (optionally) storage are wired.
//!
//! Gets a token for every configured tenant using the selected auth mode and, when
//! `--check-bucket` is passed, reads GCS bucket metadata to confirm access.
//! No data is fetched or written.

use anyhow::Context;
use clap::Args;

use xero_common::GcsConfig;

use crate::sync::{build_custom_auth, build_pkce_auth};

/// Flags for `xero healthcheck`.
#[derive(Debug, Args)]
pub struct HealthArgs {
    /// Also verify read access to the configured GCS bucket.
    #[arg(long)]
    check_bucket: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum AuthMode {
    Pkce,
    Custom,
}

fn parse_auth_mode(value: &str) -> anyhow::Result<AuthMode> {
    match value.to_ascii_lowercase().as_str() {
        "pkce" => Ok(AuthMode::Pkce),
        "custom" | "cc" | "client_credentials" | "" => Ok(AuthMode::Custom),
        other => anyhow::bail!("unsupported XERO_AUTH_MODE: {other} (use 'pkce' or 'custom')"),
    }
}

/// Execute the `healthcheck` subcommand. Exits non-zero on the first failure.
pub async fn run(args: HealthArgs) -> anyhow::Result<()> {
    let auth_mode = std::env::var("XERO_AUTH_MODE").unwrap_or_else(|_| "custom".into());
    let (auth, tenants, _) = match parse_auth_mode(&auth_mode)? {
        AuthMode::Pkce => build_pkce_auth().await?,
        AuthMode::Custom => build_custom_auth()?,
    };

    let mut failures = 0usize;
    for tenant in &tenants {
        match auth.get_valid_token(tenant).await {
            Ok(_) => println!("✓  token minted for tenant {tenant}"),
            Err(e) => {
                failures += 1;
                eprintln!("✗  token mint failed for tenant {tenant}: {e}");
            }
        }
    }

    if args.check_bucket {
        let gcs = GcsConfig::from_env().context("GCS config error (set GCS_BUCKET)")?;
        match xero_gcs::GcsRawSink::new(gcs.bucket.clone()).await {
            Ok(sink) => match sink.check_bucket().await {
                Ok(()) => println!("✓  GCS bucket reachable: {}", sink.bucket()),
                Err(e) => {
                    failures += 1;
                    eprintln!("✗  GCS bucket check failed for {}: {e}", sink.bucket());
                }
            },
            Err(e) => {
                failures += 1;
                eprintln!("✗  GCS sink build failed for bucket {}: {e}", gcs.bucket);
            }
        }
    }

    if failures > 0 {
        anyhow::bail!("healthcheck failed: {failures} check(s) did not pass");
    }
    println!("\n✓  All systems healthy");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_pkce_and_legacy_custom_connection_modes() {
        assert_eq!(parse_auth_mode("PKCE").unwrap(), AuthMode::Pkce);
        assert_eq!(parse_auth_mode("custom").unwrap(), AuthMode::Custom);
        assert_eq!(parse_auth_mode("cc").unwrap(), AuthMode::Custom);
        assert_eq!(
            parse_auth_mode("client_credentials").unwrap(),
            AuthMode::Custom
        );
        assert!(parse_auth_mode("unknown").is_err());
    }
}
