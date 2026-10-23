//! Boot checks for profile envelope encryption (WS17.E1).
//!
//! The settings are read once at boot and refused when they are inconsistent
//! or weak. A refusal names the setting, never its value. Nothing is silently
//! defaulted: a missing or short master key stops the edge.
//!
//! Settings:
//!   * `QUARRY_EDGE__PROFILE_ENCRYPTION` = `off` (default) | `compat` | `on`
//!   * `QUARRY_EDGE__PROFILE_ENCRYPTION_REQUIRED` = `1` refuses any mode below `on`
//!   * `QUARRY_EDGE__PROFILE_KEY_PROVIDER` = `env` (default). `org_core` is WS17.E2.
//!   * `QUARRY_PROFILE_MASTER_KEYS` = JSON object `{"<id>": "<base64 of 32 bytes>"}`
//!   * `QUARRY_PROFILE_MASTER_KEY_ACTIVE` = the id used for new wraps

use std::sync::Arc;

use anyhow::{bail, Result};

use quarry_runtime::envelope_profile_store::EncryptionMode;
use quarry_runtime::profile_keys::{EnvKeyProvider, ProfileKeyProvider};

/// Env var holding the master key JSON. Read by name, not through the config
/// deserializer, so a bad value is refused instead of falling back silently.
pub const MASTER_KEYS_ENV: &str = "QUARRY_PROFILE_MASTER_KEYS";

/// Env var naming the active master key id.
pub const MASTER_KEY_ACTIVE_ENV: &str = "QUARRY_PROFILE_MASTER_KEY_ACTIVE";

/// What the boot decided. `keys` is `Some` for `compat` and `on`.
pub struct EncryptionPlan {
    pub mode: EncryptionMode,
    pub keys: Option<Arc<dyn ProfileKeyProvider>>,
}

/// Validate the settings and build the key provider. Pure apart from the
/// warning log, so it can be unit-tested with plain values.
pub fn plan_from_settings(
    mode_raw: &str,
    required: bool,
    provider_raw: &str,
    master_keys_json: Option<&str>,
    active: Option<&str>,
) -> Result<EncryptionPlan> {
    let Some(mode) = EncryptionMode::parse(mode_raw) else {
        bail!("QUARRY_EDGE__PROFILE_ENCRYPTION must be off, compat or on");
    };
    match provider_raw.trim() {
        "" | "env" => {}
        "org_core" => bail!(
            "QUARRY_EDGE__PROFILE_KEY_PROVIDER=org_core is not built yet (WS17.E2); use env"
        ),
        _ => bail!("QUARRY_EDGE__PROFILE_KEY_PROVIDER must be env"),
    }
    if required && mode != EncryptionMode::On {
        bail!(
            "QUARRY_EDGE__PROFILE_ENCRYPTION_REQUIRED=1 refuses profile_encryption={}; set on first",
            mode.as_str()
        );
    }
    if mode == EncryptionMode::Off {
        return Ok(EncryptionPlan { mode, keys: None });
    }
    let Some(json) = master_keys_json.filter(|s| !s.trim().is_empty()) else {
        bail!("{MASTER_KEYS_ENV} is required when profile encryption is compat or on");
    };
    let Some(active) = active.filter(|s| !s.trim().is_empty()) else {
        bail!("{MASTER_KEY_ACTIVE_ENV} is required when profile encryption is compat or on");
    };
    let provider = EnvKeyProvider::from_config(json, active.trim()).map_err(|e| {
        anyhow::anyhow!(
            "profile master keys refused at boot ({}); check {MASTER_KEYS_ENV} and {MASTER_KEY_ACTIVE_ENV}",
            e.class()
        )
    })?;
    if mode == EncryptionMode::On {
        tracing::warn!(
            "profile encryption on with the env key provider: no crypto-shredding until WS17.E2 (losing or copying the master key exposes every profile it wraps)"
        );
    }
    Ok(EncryptionPlan {
        mode,
        keys: Some(Arc::new(provider)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine as _;

    fn json() -> String {
        format!(r#"{{"v1":"{}"}}"#, B64.encode([5u8; 32]))
    }

    #[test]
    fn off_needs_no_keys_and_required_refuses_it() {
        let p = plan_from_settings("off", false, "", None, None).unwrap();
        assert_eq!(p.mode, EncryptionMode::Off);
        assert!(p.keys.is_none());
        assert!(plan_from_settings("off", true, "", None, None).is_err());
    }

    #[test]
    fn required_refuses_compat_and_accepts_on() {
        assert!(plan_from_settings("compat", true, "env", Some(&json()), Some("v1")).is_err());
        assert!(plan_from_settings("on", true, "env", Some(&json()), Some("v1")).is_ok());
    }

    #[test]
    fn compat_and_on_need_master_keys_and_an_active_id() {
        assert!(plan_from_settings("on", false, "env", None, Some("v1")).is_err());
        assert!(plan_from_settings("on", false, "env", Some(&json()), None).is_err());
        assert!(plan_from_settings("on", false, "env", Some("{}"), Some("v1")).is_err());
        let ok = plan_from_settings("compat", false, "env", Some(&json()), Some("v1")).unwrap();
        assert!(ok.keys.is_some());
    }

    #[test]
    fn org_core_and_unknown_values_are_refused() {
        assert!(plan_from_settings("on", false, "org_core", Some(&json()), Some("v1")).is_err());
        assert!(plan_from_settings("maybe", false, "env", None, None).is_err());
        assert!(plan_from_settings("off", false, "vault", None, None).is_err());
    }
}
