//! Browser assistant redirect discovery. Quarry owns network access, org-core
//! answers each hop's site policy, and the broker grants only the final host.

use std::{collections::HashSet, future::Future, time::Duration};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use url::{Host, Url};

use crate::{
    config::AppState,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    public_url::normalize_public_http_url,
};

use super::{
    browser::quarry_call,
    browser_sites::{enabled_from_env as browser_sites_enabled, explain_site_for_member},
};

const MAX_REDIRECTS: usize = 5;
const RESOLUTION_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AccessLevel {
    Deny,
    Ask,
    Read,
    Act,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SiteAccess {
    effective: AccessLevel,
    cap_act: bool,
    default_decision: bool,
    revision: i64,
}

impl SiteAccess {
    fn can_request_access(self) -> bool {
        self.effective == AccessLevel::Ask && self.cap_act
    }

    fn can_act_for_task(self, task_scoped: bool) -> bool {
        self.effective == AccessLevel::Act
            || (task_scoped && self.default_decision && self.can_request_access())
    }
}

#[derive(Deserialize)]
struct SiteExplanation {
    organization_id: String,
    user_id: String,
    revision: i64,
    decision: SiteDecision,
}

#[derive(Deserialize)]
struct SiteDecision {
    host: String,
    effective: AccessLevel,
    cap: String,
    #[serde(default)]
    source: String,
}

#[derive(Debug, PartialEq, Eq)]
enum ResolutionError {
    SourceDenied(String),
    DestinationDenied {
        host: String,
        can_request_access: bool,
    },
    Unverifiable,
}

enum ProbeResult {
    Target(String),
    Final,
    Unavailable,
    Blocked,
}

/// Give the frontend an exact destination for an in-context choice. The next
/// start still needs a fresh broker decision, including after the user consents.
fn denied(redirect: bool, host: Option<&str>, can_request_access: bool) -> Response {
    let code = if redirect {
        "browser_redirect_access_denied"
    } else {
        "browser_grant_denied"
    };
    let message = if redirect {
        "Browser access was not granted for the redirect destination."
    } else {
        "Browser access was not granted for this session."
    };
    let details = json!({
        "host": host.filter(|host| valid_policy_host(host)),
        "canRequestAccess": can_request_access && browser_sites_enabled(),
    });
    (
        StatusCode::FORBIDDEN,
        Json(error_with_details(code, message, details)),
    )
        .into_response()
}

pub(super) fn access_denied(host: Option<&str>) -> Response {
    denied(true, host, false)
}

pub(super) fn grant_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "browser_redirect_access_unavailable",
            "Could not verify browser access for the redirect destination.",
        )),
    )
        .into_response()
}

pub(super) async fn direct_access_denied(
    state: &AppState,
    user: &AuthenticatedUser,
    host: Option<&str>,
    original: Response,
) -> Response {
    let Some(host) = host.filter(|host| valid_policy_host(host)) else {
        return original;
    };
    let access = tokio::time::timeout(
        RESOLUTION_TIMEOUT,
        site_access(state, user, host.to_owned()),
    )
    .await
    .ok()
    .flatten();
    match access {
        Some(access) => denied(false, Some(host), access.can_request_access()),
        None => original,
    }
}

pub(super) async fn resolve_start_url(
    state: &AppState,
    user: &AuthenticatedUser,
    token: Option<&str>,
    target: &str,
    task_scoped: bool,
) -> Result<Option<String>, Response> {
    let Some(initial) = eligible_homepage(target) else {
        return Ok(None);
    };
    let Some(token) = token.filter(|token| !token.trim().is_empty()) else {
        return Ok(None);
    };
    let result = tokio::time::timeout(
        RESOLUTION_TIMEOUT,
        resolve_chain(
            initial,
            task_scoped,
            |host| site_access(state, user, host),
            |url| redirect_hop(state, user, token, url),
        ),
    )
    .await;
    match result {
        Ok(Ok(resolved)) => Ok(resolved),
        // An unavailable discovery leaves the original exact-host session path in charge.
        Err(_) => Ok(None),
        Ok(Err(ResolutionError::SourceDenied(host))) => Err(denied(false, Some(&host), false)),
        Ok(Err(ResolutionError::DestinationDenied {
            host,
            can_request_access,
        })) => Err(denied(true, Some(&host), can_request_access)),
        Ok(Err(ResolutionError::Unverifiable)) => Err((
            StatusCode::BAD_GATEWAY,
            Json(error(
                "browser_redirect_unverified",
                "Could not verify a safe redirect for this address.",
            )),
        )
            .into_response()),
    }
}

fn eligible_homepage(target: &str) -> Option<Url> {
    let url = Url::parse(target).ok()?;
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    redirect_host(&url)?;
    Some(url)
}

fn redirect_host(url: &Url) -> Option<&str> {
    if !matches!(url.scheme(), "http" | "https")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    match url.host() {
        Some(Host::Domain(host)) if valid_policy_host(host) => Some(host),
        _ => None,
    }
}

fn valid_policy_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

async fn site_access(
    state: &AppState,
    user: &AuthenticatedUser,
    host: String,
) -> Option<SiteAccess> {
    let org = &user.authorized_membership.as_ref()?.organization_id;
    let (status, Json(body)) = explain_site_for_member(state, user, org, &host).await;
    if !status.is_success() {
        return None;
    }
    let answer: SiteExplanation = serde_json::from_value(unwrap_data(&body)).ok()?;
    if answer.organization_id != *org
        || answer.user_id != user.user_id
        || answer.decision.host != host
    {
        return None;
    }
    Some(SiteAccess {
        effective: answer.decision.effective,
        cap_act: answer.decision.cap == "act",
        default_decision: answer.decision.source == "default",
        revision: answer.revision,
    })
}

async fn redirect_hop(
    state: &AppState,
    user: &AuthenticatedUser,
    token: &str,
    url: String,
) -> ProbeResult {
    let (status, body) = quarry_call(
        state,
        Method::POST,
        "/v1/browser/redirect-hop",
        Some(json!({ "url": url })),
        Some(token),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return if body
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str)
            == Some("SECURITY_BLOCKED")
        {
            ProbeResult::Blocked
        } else {
            ProbeResult::Unavailable
        };
    }
    match unwrap_data(&body).get("target") {
        Some(serde_json::Value::String(target)) => ProbeResult::Target(target.clone()),
        Some(serde_json::Value::Null) => ProbeResult::Final,
        _ => ProbeResult::Unavailable,
    }
}

async fn resolve_chain<Access, AccessFuture, Probe, ProbeFuture>(
    initial: Url,
    task_scoped: bool,
    lookup: Access,
    probe: Probe,
) -> Result<Option<String>, ResolutionError>
where
    Access: Fn(String) -> AccessFuture,
    AccessFuture: Future<Output = Option<SiteAccess>>,
    Probe: Fn(String) -> ProbeFuture,
    ProbeFuture: Future<Output = ProbeResult>,
{
    let mut visited = HashSet::new();
    let mut url = initial;
    let mut crossed_host = false;
    let mut policy_revision = None;

    for hop in 0..=MAX_REDIRECTS {
        let host = redirect_host(&url)
            .ok_or(ResolutionError::Unverifiable)?
            .to_owned();
        if !visited.insert(url.clone()) {
            return Err(ResolutionError::Unverifiable);
        }
        let Some(access) = lookup(host.clone()).await else {
            return Ok(None);
        };
        if access.effective == AccessLevel::Deny || (task_scoped && !access.can_act_for_task(true)) {
            return Err(if hop == 0 {
                ResolutionError::SourceDenied(host)
            } else {
                ResolutionError::DestinationDenied {
                    host,
                    can_request_access: false,
                }
            });
        }
        if policy_revision.is_some_and(|revision| revision != access.revision) {
            return Ok(None);
        }
        policy_revision = Some(access.revision);

        match probe(url.as_str().to_owned()).await {
            ProbeResult::Unavailable => return Ok(None),
            ProbeResult::Blocked => return Err(ResolutionError::Unverifiable),
            ProbeResult::Final if !crossed_host => return Ok(None),
            ProbeResult::Final if access.can_act_for_task(task_scoped) => {
                return Ok(Some(url.into()));
            }
            ProbeResult::Final => {
                return Err(ResolutionError::DestinationDenied {
                    host,
                    can_request_access: access.can_request_access(),
                });
            }
            ProbeResult::Target(target) if hop < MAX_REDIRECTS => {
                let mut next = url
                    .join(&target)
                    .map_err(|_| ResolutionError::Unverifiable)?;
                let normalized = normalize_public_http_url(next.as_str())
                    .map_err(|_| ResolutionError::Unverifiable)?;
                next = Url::parse(&normalized).map_err(|_| ResolutionError::Unverifiable)?;
                next.set_fragment(None);
                let next_host = redirect_host(&next).ok_or(ResolutionError::Unverifiable)?;
                if url.scheme() == "https" && next.scheme() != "https" {
                    return Err(ResolutionError::Unverifiable);
                }
                crossed_host |= next_host != host;
                url = next;
            }
            ProbeResult::Target(_) => return Err(ResolutionError::Unverifiable),
        }
    }
    Err(ResolutionError::Unverifiable)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, future::ready};

    use super::*;

    fn access(effective: AccessLevel) -> SiteAccess {
        SiteAccess {
            effective,
            cap_act: true,
            default_decision: true,
            revision: 1,
        }
    }

    #[test]
    fn only_public_homepage_shape_enables_discovery() {
        assert!(eligible_homepage("https://example.org/").is_some());
        assert!(eligible_homepage("http://example.org").is_some());
        for url in [
            "https://example.org/path",
            "https://example.org/?q=secret",
            "https://example.org/#anchor",
            "https://example.org:8443/",
            "https://user:password@example.org/",
            "https://1.1.1.1/",
        ] {
            assert!(eligible_homepage(url).is_none(), "{url}");
        }
    }

    #[tokio::test]
    async fn follows_the_observed_chain_and_checks_access_before_each_probe() {
        let events = RefCell::new(Vec::new());
        let result = resolve_chain(
            Url::parse("http://example.org/").unwrap(),
            false,
            |host| {
                events.borrow_mut().push(format!("check {host}"));
                ready(Some(access(if host == "docs.example.com" {
                    AccessLevel::Act
                } else {
                    AccessLevel::Ask
                })))
            },
            |url| {
                events.borrow_mut().push(format!("probe {url}"));
                ready(match url.as_str() {
                    "http://example.org/" => ProbeResult::Target("https://example.org/".into()),
                    "https://example.org/" => {
                        ProbeResult::Target("https://docs.example.com/help".into())
                    }
                    "https://docs.example.com/help" => ProbeResult::Target("/en/help".into()),
                    "https://docs.example.com/en/help" => ProbeResult::Final,
                    _ => panic!("unexpected probe"),
                })
            },
        )
        .await;
        assert_eq!(result, Ok(Some("https://docs.example.com/en/help".into())));
        assert_eq!(
            events.into_inner(),
            [
                "check example.org",
                "probe http://example.org/",
                "check example.org",
                "probe https://example.org/",
                "check docs.example.com",
                "probe https://docs.example.com/help",
                "check docs.example.com",
                "probe https://docs.example.com/en/help",
            ]
        );
    }

    #[tokio::test]
    async fn respects_source_and_intermediate_blocks_before_network_access() {
        let source = resolve_chain(
            Url::parse("https://example.org/").unwrap(),
            false,
            |_| ready(Some(access(AccessLevel::Deny))),
            |_| -> std::future::Ready<ProbeResult> { panic!("blocked source must not be probed") },
        )
        .await;
        assert_eq!(
            source,
            Err(ResolutionError::SourceDenied("example.org".into()))
        );

        let destination = resolve_chain(
            Url::parse("https://example.org/").unwrap(),
            false,
            |host| {
                ready(Some(access(if host == "example.org" {
                    AccessLevel::Ask
                } else {
                    AccessLevel::Deny
                })))
            },
            |url| {
                assert_eq!(url, "https://example.org/");
                ready(ProbeResult::Target("https://blocked.example.com/".into()))
            },
        )
        .await;
        assert_eq!(
            destination,
            Err(ResolutionError::DestinationDenied {
                host: "blocked.example.com".into(),
                can_request_access: false,
            })
        );
    }

    #[tokio::test]
    async fn destination_requires_act_and_only_an_ask_can_offer_access() {
        for (level, can_request_access) in [(AccessLevel::Ask, true), (AccessLevel::Read, false)] {
            let result = resolve_chain(
                Url::parse("https://example.org/").unwrap(),
                false,
                |host| {
                    ready(Some(access(if host == "example.org" {
                        AccessLevel::Act
                    } else {
                        level
                    })))
                },
                |url| {
                    ready(if url == "https://example.org/" {
                        ProbeResult::Target("https://docs.example.com/".into())
                    } else {
                        ProbeResult::Final
                    })
                },
            )
            .await;
            assert_eq!(
                result,
                Err(ResolutionError::DestinationDenied {
                    host: "docs.example.com".into(),
                    can_request_access,
                })
            );
        }
    }

    #[tokio::test]
    async fn task_follows_public_aliases_without_a_saved_site_choice() {
        let result = resolve_chain(
            Url::parse("https://example.org/").unwrap(),
            true,
            |_| ready(Some(access(AccessLevel::Ask))),
            |url| ready(if url == "https://example.org/" {
                ProbeResult::Target("https://www.example.org/en".into())
            } else {
                ProbeResult::Final
            }),
        )
        .await;
        assert_eq!(result, Ok(Some("https://www.example.org/en".into())));
    }

    #[tokio::test]
    async fn task_does_not_probe_a_destination_with_a_read_ceiling() {
        let result = resolve_chain(
            Url::parse("https://example.org/").unwrap(),
            true,
            |host| ready(Some(SiteAccess {
                cap_act: host == "example.org",
                ..access(AccessLevel::Ask)
            })),
            |url| {
                assert_eq!(url, "https://example.org/");
                ready(ProbeResult::Target("https://limited.example.org/".into()))
            },
        )
        .await;
        assert_eq!(result, Err(ResolutionError::DestinationDenied {
            host: "limited.example.org".into(),
            can_request_access: false,
        }));
    }

    #[tokio::test]
    async fn unsafe_downgraded_or_looping_redirects_never_resolve() {
        for target in [
            "https://example.org/",
            "http://example.org/",
            "https://127.0.0.1/",
            "https://example.net:8080/",
            "https://user:password@example.net/",
            "file:///secret",
        ] {
            let result = resolve_chain(
                Url::parse("https://example.org/").unwrap(),
                false,
                |host| {
                    assert_eq!(host, "example.org");
                    ready(Some(access(AccessLevel::Act)))
                },
                |_| ready(ProbeResult::Target(target.into())),
            )
            .await;
            assert_eq!(result, Err(ResolutionError::Unverifiable), "{target}");
        }
    }

    #[tokio::test]
    async fn discovery_failure_and_same_host_redirects_keep_the_original_scope() {
        let unavailable = resolve_chain(
            Url::parse("https://example.org/").unwrap(),
            false,
            |_| ready(Some(access(AccessLevel::Act))),
            |_| ready(ProbeResult::Unavailable),
        )
        .await;
        assert_eq!(unavailable, Ok(None));

        let same_host = resolve_chain(
            Url::parse("https://example.org/").unwrap(),
            false,
            |_| ready(Some(access(AccessLevel::Act))),
            |url| {
                ready(if url == "https://example.org/" {
                    ProbeResult::Target("/welcome".into())
                } else {
                    ProbeResult::Final
                })
            },
        )
        .await;
        assert_eq!(same_host, Ok(None));
    }
}
