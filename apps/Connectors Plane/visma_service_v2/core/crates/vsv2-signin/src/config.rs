//! Sign-in settings, read from the environment (the same `.env` as the rest of the server).
//!
//! The sign-in needs a *web application* registered in Visma's Developer Portal: its client id and
//! secret, and a redirect URL registered exactly as `VISMA_USER_REDIRECT_URI` spells it. The sync
//! service's own client credentials are a different registration and are not used here.

use std::fmt;
use std::path::PathBuf;

use url::Url;

use crate::error::{Result, SignInError};

pub const DEFAULT_ISSUER: &str = "https://connect.visma.com";
pub const DEFAULT_AUTHORIZE_URL: &str = "https://connect.visma.com/connect/authorize";
pub const DEFAULT_TOKEN_URL: &str = "https://connect.visma.com/connect/token";
pub const DEFAULT_REVOCATION_URL: &str = "https://connect.visma.com/connect/revocation";
/// The Finance API host. A signed-in person's calls go to the same host and paths as the service's: Visma's
/// "API base URL" `.../erp/interactive` is the audience of the token, not a route (it answers 404).
pub const DEFAULT_ERP_BASE_URL: &str = "https://api.finance.visma.net";
pub const DEFAULT_USERINFO_URL: &str = "https://connect.visma.com/connect/userinfo";
/// Register this URL for the web application, or set `VISMA_USER_REDIRECT_URI` to the one registered.
pub const DEFAULT_REDIRECT_URI: &str = "http://localhost:38271/callback";
/// Read access to the Finance API as the person, plus what a refresh token and a label need.
pub const DEFAULT_SCOPES: &str =
    "openid profile email offline_access vismanet_erp_interactive_api:read";
/// Added by `login --write`: what the sales-order writes need on the SalesOrders API.
pub const DEFAULT_WRITE_SCOPES: &str =
    "visma.net.erp.salesorder:read visma.net.erp.salesorder:write";
/// Shown every time, so the person always gets the company choice.
pub const DEFAULT_PROMPT: &str = "consent";

#[derive(Clone)]
pub struct SignInConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub issuer: String,
    pub authorize_url: String,
    pub token_url: String,
    pub revocation_url: String,
    pub userinfo_url: String,
    /// Where a company's name is read from after it is connected (`GET /v1/organization`).
    pub erp_base_url: String,
    pub scopes: Vec<String>,
    pub write_scopes: Vec<String>,
    pub prompt: Option<String>,
    pub store_dir: PathBuf,
}

impl fmt::Debug for SignInConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignInConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<hidden>")
            .field("redirect_uri", &self.redirect_uri)
            .field("authorize_url", &self.authorize_url)
            .field("token_url", &self.token_url)
            .field("scopes", &self.scopes)
            .field("write_scopes", &self.write_scopes)
            .field("prompt", &self.prompt)
            .field("store_dir", &self.store_dir)
            .finish_non_exhaustive()
    }
}

impl SignInConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Like [`from_env`](Self::from_env) over any lookup, so tests do not touch the process environment.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let opt = |key: &str| {
            get(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let need = |key: &str, what: &str| {
            opt(key).ok_or_else(|| {
                SignInError::Config(format!(
                    "{key} is not set: {what} (the web application registered in the Visma Developer Portal)"
                ))
            })
        };
        let endpoint = |key: &str, default: &str| -> Result<String> {
            let raw = opt(key).unwrap_or_else(|| default.to_string());
            validate_endpoint(key, &raw)?;
            Ok(raw)
        };
        let list = |key: &str, default: &str| -> Result<Vec<String>> {
            let scopes: Vec<String> = opt(key)
                .unwrap_or_else(|| default.to_string())
                .split_whitespace()
                .map(str::to_string)
                .collect();
            for scope in &scopes {
                if !scope
                    .chars()
                    .all(|c| c.is_ascii_graphic() && c != '"' && c != '\\')
                {
                    return Err(SignInError::Config(format!(
                        "{key} holds an invalid scope {scope:?}"
                    )));
                }
            }
            Ok(scopes)
        };
        let redirect_uri =
            opt("VISMA_USER_REDIRECT_URI").unwrap_or_else(|| DEFAULT_REDIRECT_URI.to_string());
        validate_redirect_uri(&redirect_uri)?;
        let scopes = list("VISMA_USER_SCOPES", DEFAULT_SCOPES)?;
        if scopes.is_empty() {
            return Err(SignInError::Config(
                "VISMA_USER_SCOPES must name at least one scope".into(),
            ));
        }
        Ok(Self {
            client_id: need(
                "VISMA_USER_CLIENT_ID",
                "the client id of the sign-in application",
            )?,
            client_secret: need(
                "VISMA_USER_CLIENT_SECRET",
                "the client secret of the sign-in application",
            )?,
            redirect_uri,
            issuer: DEFAULT_ISSUER.to_string(),
            authorize_url: endpoint("VISMA_USER_AUTHORIZE_URL", DEFAULT_AUTHORIZE_URL)?,
            // The service credentials use the same token endpoint, so its setting is honoured too.
            token_url: match opt("VISMA_USER_TOKEN_URL").or_else(|| opt("VISMA_TOKEN_URL")) {
                Some(raw) => {
                    validate_endpoint("VISMA_USER_TOKEN_URL", &raw)?;
                    raw
                }
                None => DEFAULT_TOKEN_URL.to_string(),
            },
            revocation_url: endpoint("VISMA_USER_REVOCATION_URL", DEFAULT_REVOCATION_URL)?,
            userinfo_url: endpoint("VISMA_USER_USERINFO_URL", DEFAULT_USERINFO_URL)?,
            erp_base_url: endpoint("VISMA_ERP_REST_BASE_URL", DEFAULT_ERP_BASE_URL)?
                .trim_end_matches('/')
                .to_string(),
            scopes,
            write_scopes: list("VISMA_USER_SCOPES_WRITE", DEFAULT_WRITE_SCOPES)?,
            prompt: match get("VISMA_USER_PROMPT") {
                // Set but empty means "send no prompt parameter".
                Some(value) => Some(value.trim().to_string()).filter(|v| !v.is_empty()),
                None => Some(DEFAULT_PROMPT.to_string()),
            },
            store_dir: store_dir_from_lookup(&get)?,
        })
    }

    /// The scopes to ask for: the base list, and the write list too for `login --write`. No duplicates.
    pub fn requested_scopes(&self, write: bool) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let extra: &[String] = if write { &self.write_scopes } else { &[] };
        for scope in self.scopes.iter().chain(extra) {
            if !out.contains(scope) {
                out.push(scope.clone());
            }
        }
        out
    }
}

/// Where the sign-in is kept on this computer: `VSV2_USER_STORE_DIR` (an absolute path), else `.vsv2/user` in the
/// home directory. Readable without the rest of the sign-in configuration, so a server can tell that somebody is
/// signed in even when the application's client id is missing. A relative path, an unexpanded `~` or a missing
/// home directory is an error: tokens are never written into whatever directory the process happens to be in.
pub fn store_dir_from_env() -> Result<PathBuf> {
    store_dir_from_lookup(|key| std::env::var(key).ok())
}

pub fn store_dir_from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<PathBuf> {
    let absolute = |path: PathBuf, what: &str| {
        if path.is_absolute() {
            Ok(path)
        } else {
            Err(SignInError::Config(format!(
                "{what} must be an absolute path, got {:?} (a leading ~ is not expanded)",
                path.display().to_string()
            )))
        }
    };
    let set = |key: &str| {
        get(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if let Some(dir) = set("VSV2_USER_STORE_DIR") {
        return absolute(PathBuf::from(dir), "VSV2_USER_STORE_DIR");
    }
    match set("HOME").or_else(|| set("USERPROFILE")) {
        Some(home) => absolute(PathBuf::from(home).join(".vsv2").join("user"), "the home directory"),
        None => Err(SignInError::Config(
            "neither VSV2_USER_STORE_DIR nor a home directory (HOME) is set: set VSV2_USER_STORE_DIR to an absolute path".into(),
        )),
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// Tokens and the client secret are sent to these URLs, so they must be Visma hosts over TLS. Plain
/// HTTP loopback is accepted only so tests can point at a local mock.
pub fn validate_endpoint(key: &str, raw: &str) -> Result<()> {
    let bad = |why: &str| SignInError::Config(format!("{key} {raw:?} {why}"));
    let url = Url::parse(raw).map_err(|_| bad("is not a URL"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad("must not contain credentials"));
    }
    let host = url.host_str().ok_or_else(|| bad("needs a host"))?;
    let visma = host == "visma.com"
        || host.ends_with(".visma.com")
        || host == "visma.net"
        || host.ends_with(".visma.net");
    match url.scheme() {
        "https" if visma || is_loopback_host(host) => Ok(()),
        "http" if is_loopback_host(host) => Ok(()),
        _ => Err(bad("must be https on a *.visma.com or *.visma.net host")),
    }
}

/// Whether the redirect URL points at this computer, so a listener can catch it.
pub fn redirect_is_loopback(raw: &str) -> bool {
    Url::parse(raw)
        .is_ok_and(|url| url.scheme() == "http" && url.host_str().is_some_and(is_loopback_host))
}

/// A redirect URL is either loopback http (the listener catches it) or an https URL of the operator's own
/// (the person pastes the address they land on).
pub fn validate_redirect_uri(raw: &str) -> Result<()> {
    let bad = |why: &str| SignInError::Config(format!("VISMA_USER_REDIRECT_URI {raw:?} {why}"));
    let url = Url::parse(raw).map_err(|_| bad("is not a URL"))?;
    if url.fragment().is_some() {
        return Err(bad("must not have a fragment"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad("must not contain credentials"));
    }
    let host = url.host_str().ok_or_else(|| bad("needs a host"))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_loopback_host(host) => Ok(()),
        _ => Err(bad("must be http on localhost / 127.0.0.1 or https")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    fn base() -> Vec<(&'static str, &'static str)> {
        vec![
            ("HOME", "/home/test"),
            ("VISMA_USER_CLIENT_ID", "web-app"),
            ("VISMA_USER_CLIENT_SECRET", "s3cret"),
        ]
    }

    #[test]
    fn the_defaults_name_the_finance_api_and_the_documented_endpoints() {
        let cfg = SignInConfig::from_lookup(lookup(&base())).unwrap();
        assert_eq!(cfg.redirect_uri, DEFAULT_REDIRECT_URI);
        assert_eq!(cfg.token_url, DEFAULT_TOKEN_URL);
        assert!(cfg.scopes.contains(&"offline_access".to_string()));
        assert!(cfg
            .scopes
            .contains(&"vismanet_erp_interactive_api:read".to_string()));
        assert_eq!(cfg.prompt.as_deref(), Some("consent"));
    }

    #[test]
    fn the_client_registration_is_required_and_named_when_missing() {
        let e = SignInConfig::from_lookup(lookup(&[]))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("VISMA_USER_CLIENT_ID") && e.contains("Developer Portal"),
            "{e}"
        );
        let e = SignInConfig::from_lookup(lookup(&[("VISMA_USER_CLIENT_ID", "x")]))
            .unwrap_err()
            .to_string();
        assert!(e.contains("VISMA_USER_CLIENT_SECRET"), "{e}");
    }

    #[test]
    fn the_secret_never_shows_in_debug_output() {
        let cfg = SignInConfig::from_lookup(lookup(&base())).unwrap();
        assert!(!format!("{cfg:?}").contains("s3cret"));
    }

    #[test]
    fn write_scopes_are_added_only_on_request_and_never_twice() {
        let mut pairs = base();
        pairs.push(("VISMA_USER_SCOPES_WRITE", "openid x:write"));
        let cfg = SignInConfig::from_lookup(lookup(&pairs)).unwrap();
        assert!(!cfg.requested_scopes(false).contains(&"x:write".to_string()));
        let all = cfg.requested_scopes(true);
        assert!(all.contains(&"x:write".to_string()));
        assert_eq!(all.iter().filter(|s| *s == "openid").count(), 1);
    }

    #[test]
    fn an_empty_prompt_setting_sends_no_prompt_and_an_unset_one_asks_for_consent() {
        let mut pairs = base();
        pairs.push(("VISMA_USER_PROMPT", ""));
        assert_eq!(
            SignInConfig::from_lookup(lookup(&pairs)).unwrap().prompt,
            None
        );
        let mut pairs = base();
        pairs.push(("VISMA_USER_PROMPT", "select_account"));
        assert_eq!(
            SignInConfig::from_lookup(lookup(&pairs))
                .unwrap()
                .prompt
                .as_deref(),
            Some("select_account")
        );
    }

    #[test]
    fn tokens_only_go_to_visma_hosts_over_tls() {
        for bad in [
            "http://connect.visma.com/connect/token",
            "https://evil.example/connect/token",
            "https://connect.visma.com.evil.example/token",
            "https://user:pw@connect.visma.com/token",
            "connect.visma.com/token",
        ] {
            let mut pairs = base();
            pairs.push(("VISMA_USER_TOKEN_URL", bad));
            assert!(SignInConfig::from_lookup(lookup(&pairs)).is_err(), "{bad}");
        }
        let mut pairs = base();
        pairs.push(("VISMA_USER_TOKEN_URL", "http://127.0.0.1:9/token"));
        assert!(SignInConfig::from_lookup(lookup(&pairs)).is_ok());
    }

    #[test]
    fn the_service_token_url_setting_is_honoured_when_the_user_one_is_unset() {
        let mut pairs = base();
        pairs.push(("VISMA_TOKEN_URL", "https://connect.visma.com/connect/token"));
        assert_eq!(
            SignInConfig::from_lookup(lookup(&pairs)).unwrap().token_url,
            DEFAULT_TOKEN_URL
        );
    }

    #[test]
    fn a_redirect_is_loopback_http_or_https_and_nothing_else() {
        for ok in [
            "http://localhost:38271/callback",
            "http://127.0.0.1:1/cb",
            "http://[::1]:5/cb",
            "https://app.example.com/cb",
        ] {
            assert!(validate_redirect_uri(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://app.example.com/cb",
            "https://app.example.com/cb#frag",
            "ftp://localhost/cb",
            "localhost:1/cb",
            "https://u:p@x.example/cb",
        ] {
            assert!(validate_redirect_uri(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_store_directory_is_absolute_and_never_the_working_directory() {
        let dir = |pairs: &[(&str, &str)]| store_dir_from_lookup(lookup(pairs));
        assert_eq!(
            dir(&[("HOME", "/home/ima")]).unwrap(),
            PathBuf::from("/home/ima/.vsv2/user")
        );
        assert_eq!(
            dir(&[
                ("HOME", "/home/ima"),
                ("VSV2_USER_STORE_DIR", "/var/lib/vsv2")
            ])
            .unwrap(),
            PathBuf::from("/var/lib/vsv2")
        );
        for relative in ["~/.vsv2/user", "relative/dir", "./here"] {
            let e = dir(&[("HOME", "/home/ima"), ("VSV2_USER_STORE_DIR", relative)])
                .unwrap_err()
                .to_string();
            assert!(
                e.contains("absolute path") && e.contains("VSV2_USER_STORE_DIR"),
                "{relative}: {e}"
            );
        }
        // No home directory and no setting: an error, not a directory inside whatever the process is in.
        let e = dir(&[]).unwrap_err().to_string();
        assert!(
            e.contains("VSV2_USER_STORE_DIR") && e.contains("HOME"),
            "{e}"
        );
        assert!(dir(&[("HOME", "relative-home")]).is_err());
        assert!(SignInConfig::from_lookup(lookup(&[
            ("VISMA_USER_CLIENT_ID", "x"),
            ("VISMA_USER_CLIENT_SECRET", "y")
        ]))
        .is_err());
    }

    #[test]
    fn only_http_loopback_redirects_can_be_caught_by_a_listener() {
        assert!(redirect_is_loopback("http://localhost:38271/callback"));
        assert!(redirect_is_loopback("http://127.0.0.1:1/cb"));
        assert!(!redirect_is_loopback("https://localhost:1/cb"));
        assert!(!redirect_is_loopback("https://app.example.com/cb"));
        assert!(!redirect_is_loopback("not a url"));
    }

    #[test]
    fn scope_lists_reject_quotes_and_backslashes() {
        let mut pairs = base();
        pairs.push(("VISMA_USER_SCOPES", "openid \"x"));
        assert!(SignInConfig::from_lookup(lookup(&pairs)).is_err());
        let mut pairs = base();
        pairs.push(("VISMA_USER_SCOPES", "   "));
        // blank falls back to the default list
        assert!(SignInConfig::from_lookup(lookup(&pairs)).is_ok());
    }
}
