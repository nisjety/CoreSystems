//! The content security policy of an MCP App frame (Wave 4 W4-D M1b).
//!
//! An app frame keeps the no-network policy it has always had (`PREVIEW_CSP` in the SPA's `sandbox-html.ts`) unless
//! both sides allow an origin: the app declares it in `_meta.ui.csp`, and the org names it in
//! `mcp_apps.network_origins` (`mp_contracts::org_agent_policy`). This module builds the policy from those two lists.
//! It is pure: the caller reads the app and the org policy, and nothing here reaches the network or stores anything.
//!
//! Declared origins are untrusted server text. Each is kept only when it is one canonical https origin
//! ([`is_https_origin`]), so no declared string can add a directive, a source keyword or a wildcard. An origin is
//! written only into the directive the app declared it for, and only when the org allows it. With nothing granted the
//! policy is the frame's own, byte for byte.

use std::collections::BTreeSet;

use mp_contracts::org_agent_policy::is_https_origin;
use serde_json::Value;

const META_UI: &str = "ui";
const META_CSP: &str = "csp";
/// `connectDomains`: fetch, XHR and WebSocket (`connect-src`).
const CONNECT_DOMAINS: &str = "connectDomains";
/// `resourceDomains`: scripts, styles, images, fonts and media.
const RESOURCE_DOMAINS: &str = "resourceDomains";
/// `frameDomains`: nested frames (`frame-src`).
const FRAME_DOMAINS: &str = "frameDomains";
/// `baseUriDomains`: the document's base URI (`base-uri`).
const BASE_URI_DOMAINS: &str = "baseUriDomains";

/// The origins one app declares, by the directive each would be written to. Each list is sorted and unique.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclaredCsp {
    pub connect: Vec<String>,
    pub resource: Vec<String>,
    pub frame: Vec<String>,
    pub base_uri: Vec<String>,
    /// Declared entries that were not canonical https origins. They are dropped and only counted.
    pub dropped: usize,
}

impl DeclaredCsp {
    /// The origins declared in a content's `_meta`. A missing or malformed declaration declares nothing.
    #[must_use]
    pub fn from_meta(meta: Option<&Value>) -> Self {
        let Some(csp) = meta
            .and_then(|meta| meta.get(META_UI))
            .and_then(|ui| ui.get(META_CSP))
        else {
            return Self::default();
        };
        let mut dropped = 0;
        let connect = read_origins(csp, CONNECT_DOMAINS, &mut dropped);
        let resource = read_origins(csp, RESOURCE_DOMAINS, &mut dropped);
        let frame = read_origins(csp, FRAME_DOMAINS, &mut dropped);
        let base_uri = read_origins(csp, BASE_URI_DOMAINS, &mut dropped);
        Self {
            connect,
            resource,
            frame,
            base_uri,
            dropped,
        }
    }
}

fn read_origins(csp: &Value, key: &str, dropped: &mut usize) -> Vec<String> {
    let mut kept = BTreeSet::new();
    for entry in csp.get(key).and_then(Value::as_array).into_iter().flatten() {
        match entry.as_str() {
            Some(origin) if is_https_origin(origin) => {
                kept.insert(origin.to_owned());
            }
            _ => *dropped += 1,
        }
    }
    kept.into_iter().collect()
}

/// The policy an app frame is served with, and the origins it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCsp {
    /// The `Content-Security-Policy` value of the frame.
    pub policy: String,
    /// The origins the policy names, sorted and unique. Empty for the no-network policy.
    pub origins: Vec<String>,
}

/// The policy an app frame is served with, when the app's declared origins are intersected with the org's `allowed`
/// ones. An origin is granted only when it is both, and only into the directive it was declared for.
#[must_use]
pub fn build(declared: &DeclaredCsp, allowed: &[String]) -> AppCsp {
    let allowed: BTreeSet<&str> = allowed
        .iter()
        .map(String::as_str)
        .filter(|origin| is_https_origin(origin))
        .collect();
    let connect = granted(&declared.connect, &allowed);
    let resource = granted(&declared.resource, &allowed);
    let frame = granted(&declared.frame, &allowed);
    let base_uri = granted(&declared.base_uri, &allowed);
    let origins: BTreeSet<&str> = connect
        .iter()
        .chain(&resource)
        .chain(&frame)
        .chain(&base_uri)
        .copied()
        .collect();
    let policy = [
        "default-src 'none'".to_owned(),
        directive("base-uri", &[], &base_uri),
        "child-src 'none'".to_owned(),
        directive("connect-src", &[], &connect),
        directive("font-src", &["data:"], &resource),
        "form-action 'none'".to_owned(),
        "frame-ancestors 'none'".to_owned(),
        directive("frame-src", &[], &frame),
        directive("img-src", &["data:", "blob:"], &resource),
        directive("media-src", &["data:", "blob:"], &resource),
        "object-src 'none'".to_owned(),
        directive("script-src", &["'unsafe-inline'"], &resource),
        directive("style-src", &["'unsafe-inline'"], &resource),
        "worker-src 'none'".to_owned(),
    ]
    .join("; ");
    AppCsp {
        policy,
        origins: origins.into_iter().map(str::to_owned).collect(),
    }
}

fn granted<'a>(declared: &'a [String], allowed: &BTreeSet<&str>) -> Vec<&'a str> {
    declared
        .iter()
        .map(String::as_str)
        .filter(|origin| allowed.contains(origin))
        .collect()
}

/// One directive: its keywords, then its granted origins; `'none'` when there are neither.
fn directive(name: &str, keywords: &[&str], origins: &[&str]) -> String {
    let sources: Vec<&str> = keywords.iter().chain(origins).copied().collect();
    if sources.is_empty() {
        format!("{name} 'none'")
    } else {
        format!("{name} {}", sources.join(" "))
    }
}

/// The document an app frame loads (Wave 4 W4-D M1c): `policy` in a `<meta>` first in the head, then the app's own
/// markup in the body, the shape the SPA gives a frame. A `<meta>` applies every directive except `frame-ancestors`,
/// `report-uri` and `sandbox`, which the frame route sends as response headers. `policy` comes from [`build`], whose
/// origins are canonical https origins, so no quote or markup reaches the attribute.
#[must_use]
pub fn frame_document(policy: &str, html: &str) -> String {
    format!(
        "<!doctype html><html><head>\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{policy}\">\
         </head><body>{}</body></html>",
        html.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The frame's own policy: `PREVIEW_CSP` in `apps/Frontend Plane/verevonv3/src/shared/lib/sandbox-html.ts`.
    const PREVIEW_CSP: &str = "default-src 'none'; base-uri 'none'; child-src 'none'; connect-src 'none'; font-src data:; form-action 'none'; frame-ancestors 'none'; frame-src 'none'; img-src data: blob:; media-src data: blob:; object-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; worker-src 'none'";

    fn owned(list: &[&str]) -> Vec<String> {
        list.iter().map(|origin| (*origin).to_owned()).collect()
    }

    #[test]
    fn with_nothing_granted_the_policy_is_the_frame_policy() {
        let built = build(&DeclaredCsp::default(), &[]);
        assert_eq!(built.policy, PREVIEW_CSP);
        assert!(built.origins.is_empty());
    }

    #[test]
    fn a_declared_origin_the_org_does_not_allow_is_not_granted() {
        let declared = DeclaredCsp::from_meta(Some(
            &json!({"ui": {"csp": {"connectDomains": ["https://api.example.test"]}}}),
        ));
        let built = build(&declared, &[]);
        assert_eq!(built.policy, PREVIEW_CSP);
        assert!(built.origins.is_empty());
    }

    #[test]
    fn an_allowed_origin_the_app_did_not_declare_is_not_granted() {
        let built = build(
            &DeclaredCsp::default(),
            &owned(&["https://api.example.test"]),
        );
        assert_eq!(built.policy, PREVIEW_CSP);
        assert!(built.origins.is_empty());
    }

    #[test]
    fn a_granted_origin_is_written_only_into_the_directive_it_was_declared_for() {
        let declared = DeclaredCsp::from_meta(Some(&json!({"ui": {"csp": {
            "connectDomains": ["https://api.example.test"],
            "resourceDomains": ["https://cdn.example.test"],
            "frameDomains": ["https://embed.example.test"],
            "baseUriDomains": ["https://base.example.test"]
        }}})));
        let allowed = owned(&[
            "https://api.example.test",
            "https://cdn.example.test",
            "https://embed.example.test",
            "https://base.example.test",
        ]);
        let built = build(&declared, &allowed);
        assert_eq!(
            built.policy,
            "default-src 'none'; base-uri https://base.example.test; child-src 'none'; connect-src https://api.example.test; font-src data: https://cdn.example.test; form-action 'none'; frame-ancestors 'none'; frame-src https://embed.example.test; img-src data: blob: https://cdn.example.test; media-src data: blob: https://cdn.example.test; object-src 'none'; script-src 'unsafe-inline' https://cdn.example.test; style-src 'unsafe-inline' https://cdn.example.test; worker-src 'none'"
        );
        assert!(!built.policy.contains('*'), "no wildcard source");
        assert_eq!(
            built.origins,
            [
                "https://api.example.test",
                "https://base.example.test",
                "https://cdn.example.test",
                "https://embed.example.test"
            ]
        );
    }

    #[test]
    fn only_the_overlap_of_declared_and_allowed_is_granted() {
        let declared = DeclaredCsp::from_meta(Some(&json!({"ui": {"csp": {
            "connectDomains": ["https://a.example.test", "https://b.example.test"]
        }}})));
        let built = build(
            &declared,
            &owned(&["https://b.example.test", "https://c.example.test"]),
        );
        assert_eq!(built.origins, ["https://b.example.test"]);
        assert!(built.policy.contains("connect-src https://b.example.test;"));
        assert!(!built.policy.contains("a.example.test"));
        assert!(!built.policy.contains("c.example.test"));
    }

    #[test]
    fn declared_entries_that_are_not_canonical_https_origins_are_dropped() {
        let hostile = [
            "http://api.example.test",
            "https://*.example.test",
            "https://api.example.test/app",
            "https://API.example.test",
            "https://api.example.test; script-src *",
            "'unsafe-eval'",
            "data:",
            "*",
            "https://api.example.test:0443",
            "https://",
            "",
        ];
        let declared = DeclaredCsp::from_meta(Some(&json!({"ui": {"csp": {
            "connectDomains": hostile,
            "resourceDomains": [7, null]
        }}})));
        assert!(declared.connect.is_empty());
        assert!(declared.resource.is_empty());
        assert_eq!(declared.dropped, hostile.len() + 2);
        // Even when the org names them as written, nothing hostile reaches the policy.
        let built = build(&declared, &owned(&hostile));
        assert_eq!(built.policy, PREVIEW_CSP);
        assert!(built.origins.is_empty());
    }

    #[test]
    fn an_allowed_origin_must_itself_be_canonical() {
        let declared = DeclaredCsp::from_meta(Some(
            &json!({"ui": {"csp": {"connectDomains": ["https://api.example.test"]}}}),
        ));
        let built = build(
            &declared,
            &owned(&["https://API.example.test", "https://api.example.test/"]),
        );
        assert_eq!(built.policy, PREVIEW_CSP);
        assert!(built.origins.is_empty());
    }

    #[test]
    fn declared_lists_are_sorted_and_unique() {
        let declared = DeclaredCsp::from_meta(Some(&json!({"ui": {"csp": {
            "resourceDomains": ["https://z.example.test", "https://a.example.test", "https://z.example.test"]
        }}})));
        assert_eq!(
            declared.resource,
            ["https://a.example.test", "https://z.example.test"]
        );
        assert_eq!(declared.dropped, 0);
    }

    #[test]
    fn a_missing_declaration_declares_nothing() {
        assert_eq!(DeclaredCsp::from_meta(None), DeclaredCsp::default());
        assert_eq!(
            DeclaredCsp::from_meta(Some(&json!({"ui": {}}))),
            DeclaredCsp::default()
        );
        assert_eq!(
            DeclaredCsp::from_meta(Some(&json!({"ui": {"csp": "https://x.example.test"}}))),
            DeclaredCsp::default()
        );
    }
}
