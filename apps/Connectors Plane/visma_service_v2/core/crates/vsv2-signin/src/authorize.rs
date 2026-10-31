//! The authorization request that is sent to the browser, and the check of the answer that comes back.

use url::Url;

use crate::config::SignInConfig;
use crate::error::{Result, SignInError};
use crate::pkce::{random_string, Pkce};
use vsv2_common::text::one_line;

/// One authorization request: the URL for the browser, and what is needed to accept the answer.
#[derive(Debug, Clone)]
pub struct AuthRequest {
    pub url: String,
    pub state: String,
    pub pkce: Pkce,
}

/// Build the request for `scopes`. Every request gets its own `state` and PKCE pair, so an answer can only
/// complete the request it belongs to.
pub fn build_request(
    cfg: &SignInConfig,
    redirect_uri: &str,
    scopes: &[String],
) -> Result<AuthRequest> {
    let mut url = Url::parse(&cfg.authorize_url)
        .map_err(|_| SignInError::Config("the authorize URL is not a URL".into()))?;
    let pkce = Pkce::generate();
    let state = random_string();
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &cfg.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &scopes.join(" "))
            .append_pair("state", &state)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", "S256");
        if let Some(prompt) = &cfg.prompt {
            query.append_pair("prompt", prompt);
        }
    }
    Ok(AuthRequest {
        url: url.to_string(),
        state,
        pkce,
    })
}

/// The `key=value` pairs of what the browser came back with: a full redirect URL or just its query string.
pub fn params_from_pasted(text: &str) -> Result<Vec<(String, String)>> {
    let text = text.trim();
    let query = if text.contains("://") {
        Url::parse(text)
            .map_err(|_| SignInError::NoCode("that is not a URL".into()))?
            .query()
            .unwrap_or("")
            .to_string()
    } else {
        text.trim_start_matches('?').to_string()
    };
    if query.is_empty() {
        return Err(SignInError::NoCode(
            "there is no query in what was pasted".into(),
        ));
    }
    Ok(url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect())
}

/// Accept the answer to `request` and return its authorization code. The `state` must be the one that was
/// sent, an `iss` (RFC 9207), when present, must be Visma Connect's, and an `error` is reported as such.
pub fn read_callback(
    params: &[(String, String)],
    request: &AuthRequest,
    issuer: &str,
) -> Result<String> {
    let get = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    if get("state").is_some_and(|state| state != request.state) {
        return Err(SignInError::StateMismatch);
    }
    if let Some(got) = get("iss") {
        if got.trim_end_matches('/') != issuer.trim_end_matches('/') {
            return Err(SignInError::IssuerMismatch {
                expected: issuer.to_string(),
                got: one_line(got, 100),
            });
        }
    }
    if let Some(error) = get("error") {
        return Err(SignInError::Denied {
            error: one_line(error, 60),
            description: one_line(get("error_description").unwrap_or_default(), 300),
        });
    }
    if get("state").is_none() {
        return Err(SignInError::StateMismatch);
    }
    get("code")
        .filter(|code| !code.is_empty())
        .map(str::to_string)
        .ok_or_else(|| SignInError::NoCode("the redirect had no `code`".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SignInConfig {
        crate::testutil::config()
    }

    fn query(url: &str) -> Vec<(String, String)> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }

    fn value<'a>(pairs: &'a [(String, String)], name: &str) -> &'a str {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("no {name}"))
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_request_carries_pkce_state_scopes_the_redirect_and_the_prompt() {
        let request = build_request(
            &cfg(),
            "http://localhost:1/cb",
            &["openid".into(), "a:read".into()],
        )
        .unwrap();
        let q = query(&request.url);
        assert_eq!(value(&q, "response_type"), "code");
        assert_eq!(value(&q, "client_id"), "web-app");
        assert_eq!(value(&q, "redirect_uri"), "http://localhost:1/cb");
        assert_eq!(value(&q, "scope"), "openid a:read");
        assert_eq!(value(&q, "state"), request.state);
        assert_eq!(value(&q, "code_challenge"), request.pkce.challenge);
        assert_eq!(value(&q, "code_challenge_method"), "S256");
        assert_eq!(value(&q, "prompt"), "consent");
        assert!(
            !request.url.contains(&request.pkce.verifier),
            "the verifier never goes to the browser"
        );
        assert!(!request.url.contains("s3cret"));
    }

    #[test]
    fn no_prompt_parameter_when_none_is_configured_and_every_request_is_fresh() {
        let mut config = cfg();
        config.prompt = None;
        let a = build_request(&config, "http://localhost:1/cb", &["openid".into()]).unwrap();
        let b = build_request(&config, "http://localhost:1/cb", &["openid".into()]).unwrap();
        assert!(!query(&a.url).iter().any(|(k, _)| k == "prompt"));
        assert_ne!(a.state, b.state);
        assert_ne!(a.pkce.verifier, b.pkce.verifier);
    }

    #[test]
    fn a_matching_answer_yields_its_code() {
        let request = build_request(&cfg(), "http://localhost:1/cb", &["openid".into()]).unwrap();
        let answer = pairs(&[
            ("code", "abc"),
            ("state", &request.state),
            ("iss", "https://connect.visma.com"),
        ]);
        assert_eq!(
            read_callback(&answer, &request, "https://connect.visma.com").unwrap(),
            "abc"
        );
        let without_iss = pairs(&[("code", "abc"), ("state", &request.state)]);
        assert!(read_callback(&without_iss, &request, "https://connect.visma.com").is_ok());
    }

    #[test]
    fn an_answer_for_another_request_is_refused() {
        let request = build_request(&cfg(), "http://localhost:1/cb", &["openid".into()]).unwrap();
        for answer in [
            pairs(&[("code", "abc"), ("state", "someone-else")]),
            pairs(&[("code", "abc")]),
            pairs(&[
                ("state", &request.state),
                ("code", "abc"),
                ("iss", "https://evil.example"),
            ]),
        ] {
            assert!(
                read_callback(&answer, &request, "https://connect.visma.com").is_err(),
                "{answer:?}"
            );
        }
        let mixed_up = pairs(&[
            ("state", &request.state),
            ("code", "abc"),
            ("iss", "https://evil.example"),
        ]);
        assert!(matches!(
            read_callback(&mixed_up, &request, "https://connect.visma.com"),
            Err(SignInError::IssuerMismatch { .. })
        ));
    }

    #[test]
    fn an_error_from_visma_is_reported_with_its_reason_even_without_a_state() {
        let request = build_request(&cfg(), "http://localhost:1/cb", &["openid".into()]).unwrap();
        let denied = pairs(&[
            ("error", "invalid_scope"),
            ("error_description", "offline_access is not allowed"),
            ("state", &request.state),
        ]);
        match read_callback(&denied, &request, "https://connect.visma.com") {
            Err(SignInError::Denied { error, description }) => {
                assert_eq!(error, "invalid_scope");
                assert!(description.contains("offline_access"));
            }
            other => panic!("{other:?}"),
        }
        let stateless = pairs(&[("error", "access_denied")]);
        assert!(matches!(
            read_callback(&stateless, &request, "https://connect.visma.com"),
            Err(SignInError::Denied { .. })
        ));
        let wrong_state = pairs(&[("error", "access_denied"), ("state", "other")]);
        assert!(matches!(
            read_callback(&wrong_state, &request, "https://connect.visma.com"),
            Err(SignInError::StateMismatch)
        ));
    }

    #[test]
    fn a_pasted_url_or_query_string_is_split_into_pairs() {
        for text in [
            "http://localhost:38271/callback?code=a%20b&state=s",
            "  ?code=a%20b&state=s  ",
            "code=a%20b&state=s",
        ] {
            let p = params_from_pasted(text).unwrap();
            assert_eq!(value(&p, "code"), "a b");
            assert_eq!(value(&p, "state"), "s");
        }
        for bad in ["", "http://localhost:1/callback", "not a url ://"] {
            assert!(params_from_pasted(bad).is_err(), "{bad:?}");
        }
    }
}
