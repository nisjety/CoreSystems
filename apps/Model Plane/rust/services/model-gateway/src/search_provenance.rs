//! The "Searched via X" line (EP-9 W1): which search providers served a turn's web searches, and the route
//! the turn's entitlement allowed them. Pure: a closed table of Quarry provider ids, the route, and the turn's
//! record. It reads no content, so the frame names providers and counts only, never a query, URL or hit.

use std::sync::Mutex;

use serde::Serialize;

/// The switch that turns the disclosure on for this process. Only the literal `true` enables it (vocabulary C).
pub const SEARCH_PROVENANCE_ENV: &str = "MODEL_GATEWAY_SEARCH_PROVENANCE";

/// Whether the disclosure is switched on in this process.
#[must_use]
pub fn enabled() -> bool {
    parse_enabled(std::env::var(SEARCH_PROVENANCE_ENV).ok().as_deref())
}

fn parse_enabled(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// The providers the line can name: Quarry's id, the closed id the frame carries, and the label it shows.
///
/// Quarry's composite labels name the router or a blend of providers, not one provider, so they show as
/// Verevon's own search rather than as a guess at which part served the hit. A Quarry id missing from this
/// table is never echoed (see [`OTHER`]), so a provider Quarry adds later cannot put a raw string in the line.
const PROVIDERS: &[(&str, &str, &str)] = &[
    ("tantivy_local", "tantivy_local", "Verevon index"),
    ("brave", "brave", "Brave Search"),
    ("serper", "serper", "Serper (Google results)"),
    ("searxng", "searxng", "SearXNG metasearch"),
    ("stract", "stract", "Stract"),
    ("hybrid", "verevon", "Verevon search"),
    ("reranked", "verevon", "Verevon search"),
    ("fallback", "verevon", "Verevon search"),
    ("smart_router", "verevon", "Verevon search"),
];

/// What an id outside [`PROVIDERS`] shows as: the closed id `other` and its generic label.
const OTHER: (&str, &str) = ("other", "other provider");

/// One provider the line names, with the hits it served.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderUse {
    /// The closed id the SPA words its line on. `other` for an id the table does not name.
    pub id: &'static str,
    /// The English label of `id`. The SPA words the Norwegian and English line itself.
    pub label: &'static str,
    /// Hits this provider served across every search of the turn. Zero for a search that returned none.
    pub hits: u32,
}

/// The route a turn's searches were allowed: the free chain, the paid providers, or the free providers only,
/// because zero data retention is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    FreeChain,
    PaidAllowed,
    ZdrFreeOnly,
}

impl Route {
    /// The route of a turn. Zero data retention outranks the paid grant, as it does in
    /// `paid_providers_allowed`, so the two cannot disagree about a ZDR turn.
    #[must_use]
    pub fn for_turn(zdr: bool, paid_allowed: bool) -> Self {
        match (zdr, paid_allowed) {
            (true, _) => Self::ZdrFreeOnly,
            (false, true) => Self::PaidAllowed,
            (false, false) => Self::FreeChain,
        }
    }
}

/// What a turn's searches were served by: each provider once, in the order it first served a hit, and the route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchProvenance {
    pub providers: Vec<ProviderUse>,
    pub route: Route,
}

/// The searches one chat turn has run. Held for the turn and dropped with it, never stored: a turn's searches
/// are disclosed live and then gone. Tool calls of one turn run at once, hence the lock.
#[derive(Debug)]
pub struct SearchProvenanceTurn {
    route: Route,
    state: Mutex<TurnState>,
}

#[derive(Debug, Default)]
struct TurnState {
    providers: Vec<ProviderUse>,
    sent: bool,
}

impl SearchProvenanceTurn {
    /// A record for a turn whose searches are allowed `route`.
    #[must_use]
    pub fn new(route: Route) -> Self {
        Self {
            route,
            state: Mutex::new(TurnState::default()),
        }
    }

    /// Records one answered search. A hit counts for the provider that served it, or for `envelope` (the
    /// provider the edge named for the answer) when the hit names none. A search with no hits still names the
    /// provider it asked, so the line does not lose a search that found nothing.
    pub fn record(&self, envelope: &str, hit_sources: &[&str]) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if hit_sources.is_empty() {
            bump(&mut state.providers, provider_for(envelope), 0);
        }
        for &source in hit_sources {
            let provider = if source.trim().is_empty() {
                envelope
            } else {
                source
            };
            bump(&mut state.providers, provider_for(provider), 1);
        }
    }

    /// The turn's disclosure, once. `None` while no search has run, and again after it was taken, so a turn
    /// sends at most one line however often its stream asks for it.
    pub fn take_provenance(&self) -> Option<SearchProvenance> {
        let mut state = self.state.lock().ok()?;
        if state.sent || state.providers.is_empty() {
            return None;
        }
        state.sent = true;
        Some(SearchProvenance {
            providers: state.providers.clone(),
            route: self.route,
        })
    }
}

/// Adds `hits` to the entry for `(id, label)`, opening the entry on its first hit.
fn bump(providers: &mut Vec<ProviderUse>, (id, label): (&'static str, &'static str), hits: u32) {
    match providers.iter_mut().find(|provider| provider.id == id) {
        Some(provider) => provider.hits = provider.hits.saturating_add(hits),
        None => providers.push(ProviderUse { id, label, hits }),
    }
}

/// The closed id and label a Quarry provider id shows as.
fn provider_for(raw: &str) -> (&'static str, &'static str) {
    let raw = raw.trim();
    PROVIDERS
        .iter()
        .find(|(quarry, _, _)| *quarry == raw)
        .map_or(OTHER, |&(_, id, label)| (id, label))
}

#[cfg(test)]
mod tests {
    use super::{parse_enabled, provider_for, ProviderUse, Route, SearchProvenanceTurn, OTHER};

    #[test]
    fn the_switch_is_on_only_for_the_literal_true() {
        assert!(parse_enabled(Some("true")));
        assert!(parse_enabled(Some(" true ")));
        for raw in [
            None,
            Some(""),
            Some("1"),
            Some("yes"),
            Some("True"),
            Some("on"),
        ] {
            assert!(!parse_enabled(raw), "{raw:?} must not switch the line on");
        }
    }

    #[test]
    fn every_verified_quarry_provider_has_a_label_of_its_own() {
        for quarry in [
            "tantivy_local",
            "brave",
            "serper",
            "searxng",
            "stract",
            "hybrid",
            "reranked",
            "fallback",
            "smart_router",
        ] {
            assert_ne!(
                provider_for(quarry),
                OTHER,
                "{quarry} is named in the table"
            );
        }
        assert_eq!(provider_for("brave"), ("brave", "Brave Search"));
        assert_eq!(
            provider_for("tantivy_local"),
            ("tantivy_local", "Verevon index")
        );
    }

    #[test]
    fn a_composite_router_shows_as_verevon_search() {
        assert_eq!(provider_for("smart_router"), ("verevon", "Verevon search"));
        assert_eq!(provider_for(" hybrid "), ("verevon", "Verevon search"));
    }

    #[test]
    fn an_unknown_provider_is_never_echoed() {
        let turn = SearchProvenanceTurn::new(Route::FreeChain);
        turn.record("zzz_new_engine", &["zzz_new_engine"]);
        let provenance = turn.take_provenance().expect("a search ran");
        assert_eq!(
            provenance.providers,
            vec![ProviderUse {
                id: "other",
                label: "other provider",
                hits: 1,
            }]
        );
        let wire = serde_json::to_string(&provenance).expect("the line serializes");
        assert!(
            !wire.contains("zzz_new_engine"),
            "the raw id reached the line: {wire}"
        );
    }

    #[test]
    fn two_searches_in_one_turn_add_their_hits_by_provider() {
        let turn = SearchProvenanceTurn::new(Route::PaidAllowed);
        turn.record("smart_router", &["brave", "brave", "tantivy_local"]);
        turn.record("smart_router", &["brave"]);
        let provenance = turn.take_provenance().expect("searches ran");
        assert_eq!(
            provenance.providers,
            vec![
                ProviderUse {
                    id: "brave",
                    label: "Brave Search",
                    hits: 3,
                },
                ProviderUse {
                    id: "tantivy_local",
                    label: "Verevon index",
                    hits: 1,
                },
            ]
        );
        assert_eq!(provenance.route, Route::PaidAllowed);
    }

    #[test]
    fn a_hit_without_a_source_takes_the_envelope_provider() {
        let turn = SearchProvenanceTurn::new(Route::FreeChain);
        turn.record("searxng", &["", "stract"]);
        let provenance = turn.take_provenance().expect("a search ran");
        assert_eq!(
            provenance.providers,
            vec![
                ProviderUse {
                    id: "searxng",
                    label: "SearXNG metasearch",
                    hits: 1,
                },
                ProviderUse {
                    id: "stract",
                    label: "Stract",
                    hits: 1,
                },
            ]
        );
    }

    #[test]
    fn a_search_with_no_hits_still_names_the_provider_it_asked() {
        let turn = SearchProvenanceTurn::new(Route::FreeChain);
        turn.record("brave", &[]);
        assert_eq!(
            turn.take_provenance()
                .map(|provenance| provenance.providers),
            Some(vec![ProviderUse {
                id: "brave",
                label: "Brave Search",
                hits: 0,
            }])
        );
    }

    #[test]
    fn the_route_follows_zdr_before_the_paid_grant() {
        assert_eq!(Route::for_turn(true, true), Route::ZdrFreeOnly);
        assert_eq!(Route::for_turn(true, false), Route::ZdrFreeOnly);
        assert_eq!(Route::for_turn(false, true), Route::PaidAllowed);
        assert_eq!(Route::for_turn(false, false), Route::FreeChain);
        assert_eq!(
            serde_json::to_value(Route::ZdrFreeOnly).expect("the route serializes"),
            "zdr_free_only"
        );
    }

    #[test]
    fn nothing_is_sent_without_a_search_and_the_line_is_sent_once() {
        let turn = SearchProvenanceTurn::new(Route::FreeChain);
        assert!(turn.take_provenance().is_none());
        turn.record("brave", &["brave"]);
        assert!(turn.take_provenance().is_some());
        assert!(
            turn.take_provenance().is_none(),
            "a second take must not send the line again"
        );
    }
}
