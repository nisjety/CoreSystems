//! The retention resolver: the one pure decision of what a turn is under zero data retention.
//!
//! Every plane that receives a user's tokens asks the same question before it serves a model
//! call: is this turn ZDR, persistent, waived for one connection, or refused? The answer comes
//! from the claims the bearers carry and from the route the request names. This module answers
//! that question and nothing else. It does no I/O, reads no environment and parses no token: a
//! plane maps its own verified claims into [`ClaimsView`] and its own route into [`RouteView`],
//! so the rules live here once and cannot drift between model-gateway, inference-core,
//! execution-core and session-core.
//!
//! The rules are the `enforcement_rows` and the `rule` of `zdr-posture-matrix.json`. The
//! integration test `tests/zdr_retention_matrix.rs` runs that golden through [`resolve_turn`]
//! and fails on any row it disagrees with. The decision side (what Auth Core mints) is Auth
//! Core's function and is tested against the same golden.
//!
//! # Codes
//!
//! A refusal carries a code of the Z-05a catalogue ([`crate::zdr_refusal`]). Any other decision
//! carries a [`Reason`]. Both are the matrix's own strings, so a plane logs, branches and
//! records the same words the golden pins.

use crate::agentic_subscription::PROVIDER_ID;
use crate::zdr_refusal::Refusal;

/// The contract version of the `zdr_waiver` claim. A waiver of any other version is refused,
/// never read as if it were this one. The matrix pins the same number as
/// `waiver_contract_version`.
pub const WAIVER_CONTRACT_VERSION: u32 = 1;

/// The providers a waiver may bind. Only the subscription route, `PROVIDER_ID`, has a waiver route
/// in v1, which is the matrix's `policy.waiver_providers`.
pub const WAIVER_PROVIDERS: &[&str] = &[PROVIDER_ID];

/// The `zdr_waiver` claim of one token, as a plane reads it off the verified token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaiverView<'a> {
    /// The waiver contract version the token was minted under.
    pub version: u32,
    /// The canonical provider id the waiver binds, spelled exactly as minted.
    pub provider: &'a str,
    /// The one user connection the waiver binds.
    pub connection: &'a str,
}

/// The claims of one delegated user bearer, as a plane reads them off the verified token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimsView<'a> {
    /// The `zdr` claim. Every token carries it, and a plane refuses a token without it rather
    /// than defaulting it.
    pub zdr: bool,
    /// The `org_zdr` claim: `Some(true)` when the org is ZDR, `None` when the claim is absent
    /// (service tokens).
    pub org_zdr: Option<bool>,
    /// The `zdr_waiver` claim, when the token carries one.
    pub waiver: Option<WaiverView<'a>>,
}

/// The route a request names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteView<'a> {
    /// The provider the request names, as the request spells it. Empty is the default chain.
    /// The subscription route is recognised in any spelling (see `names_subscription_route`); a
    /// waiver binds the canonical spelling only.
    pub provider: &'a str,
    /// The user connection the request names, when it names one.
    pub connection: Option<&'a str>,
    /// Whether the request asks for ZDR itself: Temporary chat, `x-zdr`, or body `zdr: true`.
    pub explicit_zdr: bool,
}

impl RouteView<'_> {
    /// Whether the route runs on one user connection. A subscription route is one even when the
    /// request names no connection, because it cannot be ZDR and must not be treated as if it
    /// could.
    #[must_use]
    pub fn is_connection_route(&self) -> bool {
        self.connection.is_some() || names_subscription_route(self.provider)
    }
}

/// Whether `provider` names the subscription route, in any spelling a request might carry: case,
/// surrounding whitespace and `-` or `_` separators are ignored. A ZDR turn must never be answered
/// `zdr` on a spelling this function does not recognise, since a plane that canonicalises the name
/// later would send the turn to a provider that cannot give zero retention.
fn names_subscription_route(provider: &str) -> bool {
    let fold = |text: &str| -> String {
        text.chars()
            .filter(|c| !matches!(c, '-' | '_'))
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let named = fold(provider.trim());
    std::iter::once(PROVIDER_ID)
        .chain(SUBSCRIPTION_ALIASES.iter().copied())
        .any(|id| fold(id) == named)
}

/// The provider spellings inference-core also accepts for the subscription route (its aliases).
/// They name the same route as the canonical id, so a turn that uses one is the subscription route
/// for every rule here: a ZDR turn is never answered on it. The gateway's own reader of these
/// spellings (`normalize::is_codex_subscription_hint`) must accept exactly this list.
pub const SUBSCRIPTION_ALIASES: &[&str] = &["codex-subscription", "chatgpt-codex"];

/// Why a decision is what it is. The strings are the matrix's `reason_codes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The org is ZDR and the turn asks for nothing more.
    OrgZdr,
    /// The turn asks for ZDR itself.
    ExplicitZdr,
    /// The org is not ZDR, so the turn is persistent.
    OrgZdrOff,
    /// The org is ZDR, the connection named is not, and a valid acknowledgement lets this one
    /// connection run with ZDR off for this run only.
    ConnectionNotZdrCapable,
}

impl Reason {
    /// The matrix's reason code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrgZdr => "org_zdr",
            Self::ExplicitZdr => "explicit_zdr",
            Self::OrgZdrOff => "org_zdr_off",
            Self::ConnectionNotZdrCapable => "connection_not_zdr_capable",
        }
    }
}

/// What a turn is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution<'a> {
    /// Zero retention applies: the content is not persisted.
    Zdr { reason: Reason },
    /// The turn is persistent, as a turn of an org that is not ZDR is.
    Persistent { reason: Reason },
    /// The org is ZDR, and this one connection runs with ZDR off for this run only. Nothing
    /// else is waived.
    Waived {
        provider: &'a str,
        connection: &'a str,
        reason: Reason,
    },
    /// The turn is refused. Nothing was served.
    Refuse(Refusal),
}

impl Resolution<'_> {
    /// The decision word of the matrix: `zdr`, `persistent`, `waived` or `refuse`.
    #[must_use]
    pub fn decision(&self) -> &'static str {
        match self {
            Self::Zdr { .. } => "zdr",
            Self::Persistent { .. } => "persistent",
            Self::Waived { .. } => "waived",
            Self::Refuse(_) => "refuse",
        }
    }

    /// The reason, or the refusal code, as the matrix spells it.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Zdr { reason } | Self::Persistent { reason } | Self::Waived { reason, .. } => {
                reason.as_str()
            }
            Self::Refuse(refusal) => refusal.code(),
        }
    }

    /// The retention class a turn records (`zdr`, `persistent` or `zdr_waived`). A refusal
    /// records no turn, so it has none.
    #[must_use]
    pub fn retention_class(&self) -> Option<&'static str> {
        match self {
            Self::Zdr { .. } => Some("zdr"),
            Self::Persistent { .. } => Some("persistent"),
            Self::Waived { .. } => Some("zdr_waived"),
            Self::Refuse(_) => None,
        }
    }
}

/// Whether `route` is the route `waiver` binds: its provider and its connection exactly, no
/// explicit ZDR, a waiver of the current contract version, and a provider that has a waiver
/// route. Planes that receive a waiver on a route call this; [`resolve_turn`] applies it too.
#[must_use]
pub fn route_allowed_under_waiver(waiver: &WaiverView<'_>, route: &RouteView<'_>) -> bool {
    waiver.version == WAIVER_CONTRACT_VERSION
        && WAIVER_PROVIDERS.contains(&waiver.provider)
        && !waiver.connection.is_empty()
        && route.provider == waiver.provider
        && route.connection == Some(waiver.connection)
        && !route.explicit_zdr
}

/// The turn's retention, from the claims of every bearer it presents and the route it names.
///
/// The rules are applied in the matrix's order:
///
/// 1. Every bearer agrees on `zdr` and on the waiver. Disagreement is refused as a posture
///    mismatch, and so is a request with no bearer at all: a request with no posture is not
///    served.
/// 2. A waiver is an exception to the org default and nothing more. It is honoured only on a
///    token that is persistent (`zdr` false) for an org that is ZDR (`org_zdr` true on every
///    bearer), at the current contract version, for the provider and connection it binds, with
///    no explicit ZDR. A token that is wrong for a waiver is a posture mismatch; a route that is
///    not the one the waiver binds is a waiver route mismatch.
/// 3. Without a waiver, a token that is persistent for a ZDR org is never served.
/// 4. A ZDR token never reaches a connection route. No connection provider is ZDR-capable yet
///    (the matrix's `policy.zdr_capable_connection_providers` is empty), and a connection that is
///    not ZDR-capable runs only under a waiver. An explicit request on a connection is refused,
///    since a connection cannot give ZDR on request.
/// 5. Otherwise a ZDR token is ZDR, and so is an explicit request. Anything else is persistent.
#[must_use]
pub fn resolve_turn<'a>(bearers: &[ClaimsView<'a>], route: &RouteView<'a>) -> Resolution<'a> {
    let Some(first) = bearers.first().copied() else {
        return Resolution::Refuse(Refusal::POSTURE_MISMATCH);
    };
    if bearers
        .iter()
        .any(|bearer| bearer.zdr != first.zdr || bearer.waiver != first.waiver)
    {
        return Resolution::Refuse(Refusal::POSTURE_MISMATCH);
    }
    if let Some(waiver) = first.waiver {
        return resolve_waived(bearers, first.zdr, &waiver, route);
    }
    if bearers
        .iter()
        .any(|bearer| bearer.org_zdr == Some(true) && !bearer.zdr)
    {
        return Resolution::Refuse(Refusal::POSTURE_MISMATCH);
    }
    let explicit = route.explicit_zdr;
    if route.is_connection_route() && (first.zdr || explicit) {
        return Resolution::Refuse(if explicit {
            Refusal::EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION
        } else {
            Refusal::CONNECTION_NOT_ACKNOWLEDGED
        });
    }
    if first.zdr || explicit {
        let reason = if explicit {
            Reason::ExplicitZdr
        } else {
            Reason::OrgZdr
        };
        return Resolution::Zdr { reason };
    }
    Resolution::Persistent {
        reason: Reason::OrgZdrOff,
    }
}

/// The waived branch of [`resolve_turn`]: a waiver is an exception to the org default, so it
/// is honoured only for a token that is persistent for a ZDR org, and only on the route it binds.
fn resolve_waived<'a>(
    bearers: &[ClaimsView<'a>],
    zdr: bool,
    waiver: &WaiverView<'a>,
    route: &RouteView<'a>,
) -> Resolution<'a> {
    let persistent_for_zdr_org = !zdr && bearers.iter().all(|bearer| bearer.org_zdr == Some(true));
    if waiver.version != WAIVER_CONTRACT_VERSION || !persistent_for_zdr_org {
        return Resolution::Refuse(Refusal::POSTURE_MISMATCH);
    }
    if !route_allowed_under_waiver(waiver, route) {
        return Resolution::Refuse(Refusal::WAIVER_ROUTE_MISMATCH);
    }
    Resolution::Waived {
        provider: waiver.provider,
        connection: waiver.connection,
        reason: Reason::ConnectionNotZdrCapable,
    }
}

/// How a plane treats the Space decision's `zero_data_retention` flag on the legacy owner
/// `create_thread` and `append_message` paths (WS06-S2, `SESSION_CORE_SPACE_ZDR`).
///
/// `Observe` is the default: the flag is counted and the write proceeds, which is the v1
/// behaviour the posture matrix pins (`policy.space_zdr = not_applied_in_v1`). `Enforce` makes a
/// ZDR-flagged Space refuse the write with the typed catalogue refusal. The flag is a rollout
/// switch, never an authorization input: no value of it widens what a decision authorizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceZdrMode {
    Observe,
    Enforce,
}

impl SpaceZdrMode {
    /// The label value of the `enforced` metric dimension.
    #[must_use]
    pub const fn enforced_label(self) -> &'static str {
        match self {
            Self::Observe => "false",
            Self::Enforce => "true",
        }
    }
}

/// Parse the `SESSION_CORE_SPACE_ZDR` value. Only the exact string `enforce` enables
/// enforcement; unset, empty, `observe`, `Enforce`, `true` and every other value are `Observe`,
/// so a typo can never silently turn the refusal on or change what ships by default.
#[must_use]
pub fn space_zdr_mode(raw: Option<&str>) -> SpaceZdrMode {
    match raw {
        Some("enforce") => SpaceZdrMode::Enforce,
        _ => SpaceZdrMode::Observe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route<'a>(
        provider: &'a str,
        connection: Option<&'a str>,
        explicit_zdr: bool,
    ) -> RouteView<'a> {
        RouteView {
            provider,
            connection,
            explicit_zdr,
        }
    }

    #[test]
    fn the_reason_strings_are_the_matrix_reason_codes() {
        assert_eq!(Reason::OrgZdr.as_str(), "org_zdr");
        assert_eq!(Reason::ExplicitZdr.as_str(), "explicit_zdr");
        assert_eq!(Reason::OrgZdrOff.as_str(), "org_zdr_off");
        assert_eq!(
            Reason::ConnectionNotZdrCapable.as_str(),
            "connection_not_zdr_capable"
        );
    }

    #[test]
    fn the_waiver_contract_version_is_the_matrix_version() {
        assert_eq!(WAIVER_CONTRACT_VERSION, 1);
        assert_eq!(WAIVER_PROVIDERS, &[PROVIDER_ID]);
    }

    #[test]
    fn a_subscription_route_is_a_connection_route_even_without_a_connection() {
        assert!(route(PROVIDER_ID, None, false).is_connection_route());
        assert!(route("azure-openai", Some("conn-1"), false).is_connection_route());
        assert!(!route("", None, false).is_connection_route());
    }

    #[test]
    fn a_waiver_binds_its_provider_connection_and_version_exactly() {
        let waiver = WaiverView {
            version: 1,
            provider: PROVIDER_ID,
            connection: "conn-1",
        };
        assert!(route_allowed_under_waiver(
            &waiver,
            &route(PROVIDER_ID, Some("conn-1"), false)
        ));
        let refused = [
            route(PROVIDER_ID, Some("conn-2"), false),
            route("", None, false),
            route("azure-openai", Some("conn-1"), false),
            route(PROVIDER_ID, None, false),
            route(PROVIDER_ID, Some("conn-1"), true),
        ];
        for other in refused {
            assert!(
                !route_allowed_under_waiver(&waiver, &other),
                "{other:?} is not the route the waiver binds"
            );
        }
        let other_version = WaiverView {
            version: 2,
            ..waiver
        };
        assert!(!route_allowed_under_waiver(
            &other_version,
            &route(PROVIDER_ID, Some("conn-1"), false)
        ));
        let empty_connection = WaiverView {
            connection: "",
            ..waiver
        };
        assert!(!route_allowed_under_waiver(
            &empty_connection,
            &route(PROVIDER_ID, Some(""), false)
        ));
        let not_waivable = WaiverView {
            provider: "azure-openai",
            ..waiver
        };
        assert!(!route_allowed_under_waiver(
            &not_waivable,
            &route("azure-openai", Some("conn-1"), false)
        ));
    }

    #[test]
    fn every_outcome_names_the_matrix_decision_code_and_retention_class() {
        let zdr = Resolution::Zdr {
            reason: Reason::OrgZdr,
        };
        assert_eq!(
            (zdr.decision(), zdr.code(), zdr.retention_class()),
            ("zdr", "org_zdr", Some("zdr"))
        );
        let explicit = Resolution::Zdr {
            reason: Reason::ExplicitZdr,
        };
        assert_eq!(
            (
                explicit.decision(),
                explicit.code(),
                explicit.retention_class()
            ),
            ("zdr", "explicit_zdr", Some("zdr"))
        );
        let persistent = Resolution::Persistent {
            reason: Reason::OrgZdrOff,
        };
        assert_eq!(
            (
                persistent.decision(),
                persistent.code(),
                persistent.retention_class()
            ),
            ("persistent", "org_zdr_off", Some("persistent"))
        );
        let waived = Resolution::Waived {
            provider: PROVIDER_ID,
            connection: "conn-1",
            reason: Reason::ConnectionNotZdrCapable,
        };
        assert_eq!(
            (waived.decision(), waived.code(), waived.retention_class()),
            ("waived", "connection_not_zdr_capable", Some("zdr_waived"))
        );
        let refused = Resolution::Refuse(Refusal::POSTURE_MISMATCH);
        assert_eq!(
            (
                refused.decision(),
                refused.code(),
                refused.retention_class()
            ),
            ("refuse", "zdr_posture_mismatch", None)
        );
    }

    #[test]
    fn a_zdr_token_never_reaches_the_subscription_route_in_any_spelling() {
        let zdr = ClaimsView {
            zdr: true,
            org_zdr: Some(true),
            waiver: None,
        };
        for provider in [
            PROVIDER_ID,
            "OPENAI-CODEX-SUBSCRIPTION",
            " openai-codex-subscription ",
            "openai_codex_subscription",
            "Openai-Codex-Subscription",
        ] {
            for connection in [None, Some("conn-1")] {
                let got = resolve_turn(&[zdr], &route(provider, connection, false));
                assert!(
                    matches!(got, Resolution::Refuse(_)),
                    "{provider:?} with {connection:?} answered {got:?}"
                );
            }
        }
    }

    #[test]
    fn a_zdr_token_never_reaches_a_subscription_alias_either() {
        let zdr = ClaimsView {
            zdr: true,
            org_zdr: Some(true),
            waiver: None,
        };
        for provider in std::iter::once(PROVIDER_ID).chain(SUBSCRIPTION_ALIASES.iter().copied()) {
            for connection in [None, Some("conn-1")] {
                let got = resolve_turn(&[zdr], &route(provider, connection, false));
                assert!(
                    matches!(got, Resolution::Refuse(_)),
                    "{provider:?} with {connection:?} answered {got:?}"
                );
            }
        }
    }

    #[test]
    fn a_zdr_turn_the_request_asks_for_reports_explicit_zdr_even_in_a_zdr_org() {
        let zdr = ClaimsView {
            zdr: true,
            org_zdr: Some(true),
            waiver: None,
        };
        assert_eq!(
            resolve_turn(&[zdr], &route("", None, true)),
            Resolution::Zdr {
                reason: Reason::ExplicitZdr
            }
        );
        assert_eq!(
            resolve_turn(&[zdr], &route("", None, false)),
            Resolution::Zdr {
                reason: Reason::OrgZdr
            }
        );
    }

    #[test]
    fn a_waiver_is_honoured_only_on_the_canonical_spelling_and_a_named_connection() {
        let waiver = WaiverView {
            version: 1,
            provider: PROVIDER_ID,
            connection: "conn-1",
        };
        let persistent = ClaimsView {
            zdr: false,
            org_zdr: Some(true),
            waiver: Some(waiver),
        };
        assert!(matches!(
            resolve_turn(&[persistent], &route(PROVIDER_ID, Some("conn-1"), false)),
            Resolution::Waived { .. }
        ));
        assert_eq!(
            resolve_turn(
                &[persistent],
                &route("OPENAI-CODEX-SUBSCRIPTION", Some("conn-1"), false)
            ),
            Resolution::Refuse(Refusal::WAIVER_ROUTE_MISMATCH)
        );
        let unnamed = ClaimsView {
            waiver: Some(WaiverView {
                connection: "",
                ..waiver
            }),
            ..persistent
        };
        assert_eq!(
            resolve_turn(&[unnamed], &route(PROVIDER_ID, Some(""), false)),
            Resolution::Refuse(Refusal::WAIVER_ROUTE_MISMATCH)
        );
    }
}
