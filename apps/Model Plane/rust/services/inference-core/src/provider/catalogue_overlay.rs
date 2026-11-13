//! The platform catalogue overlay (F-t3-065 phase 1): an admin's view of the
//! provider catalogue the env-defined providers build. It hides models, orders
//! the listing and gives model ids aliases. It is stored in the routing policy
//! beside the role table, so it reuses that store, hot-swap and admin path.
//!
//! The overlay changes what is listed and what a pinned id resolves to. It never
//! sets a privacy tier, residency or ZDR, and the caller's entitlement still
//! applies after it. It is dark until `INFERENCE_CATALOGUE_OVERLAY` is the literal
//! `true`: off, a stored overlay is ignored and the listing and admission are as
//! they were.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use mp_contracts::model_plane::v1 as pb;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::fallback::is_unspecified_model;
use super::role::ROLE_MODEL_ID;
use super::routing_policy::{accepts_no_reasoning, RolePurpose};
use super::ModelInfo;

/// The most model ids the hidden set may hold.
pub const MAX_HIDDEN: usize = 200;

/// The most model ids the order may hold.
pub const MAX_ORDER: usize = 200;

/// The most aliases the overlay may hold.
pub const MAX_ALIASES: usize = 50;

/// The longest alias id, in bytes.
pub const MAX_ALIAS_LEN: usize = 64;

/// The switch that turns the overlay on. Only the literal `true` enables it.
pub const CATALOGUE_OVERLAY_ENV: &str = "INFERENCE_CATALOGUE_OVERLAY";

/// The admin's catalogue overlay, stored as part of the routing policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueOverlay {
    /// Model ids the listing drops, and admission refuses when a caller pins one.
    #[serde(default)]
    pub hidden: BTreeSet<String>,
    /// Model ids the listing puts first, in this order. A model the order does
    /// not name keeps its place after them. An alias id may be named here.
    #[serde(default)]
    pub order: Vec<String>,
    /// Alias id to the concrete model id it stands for. An alias lists as that
    /// model and serves as that model.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

impl CatalogueOverlay {
    /// Whether the overlay changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.order.is_empty() && self.aliases.is_empty()
    }

    /// Whether the overlay hides `id`. Ids match without regard to ASCII case or
    /// surrounding whitespace, on either side, as `retired_successor` and
    /// `is_reserved_id` read theirs.
    #[must_use]
    pub fn is_hidden(&self, id: &str) -> bool {
        let id = id.trim();
        self.hidden
            .iter()
            .any(|hidden| hidden.trim().eq_ignore_ascii_case(id))
    }

    /// The concrete model id an alias stands for, when `id` is one of the aliases.
    #[must_use]
    pub fn alias_target(&self, id: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(id))
            .map(|(_, target)| target.as_str())
    }

    /// Every way the overlay fails against the chat catalogue, in list order. An
    /// empty overlay is valid.
    ///
    /// # Errors
    /// The refusals, each naming its list and, where one applies, its id.
    pub fn validate(&self, catalogue: &[ModelInfo]) -> Result<(), Vec<CatalogueOverlayError>> {
        let mut errors = Vec::new();
        for (list, count, max) in [
            (OverlayList::Hidden, self.hidden.len(), MAX_HIDDEN),
            (OverlayList::Order, self.order.len(), MAX_ORDER),
            (OverlayList::Aliases, self.aliases.len(), MAX_ALIASES),
        ] {
            if count > max {
                errors.push(CatalogueOverlayError::TooMany { list, count, max });
            }
        }
        for id in &self.hidden {
            if id.trim().is_empty() {
                errors.push(CatalogueOverlayError::BlankId {
                    list: OverlayList::Hidden,
                });
            } else if is_reserved_id(id) {
                errors.push(CatalogueOverlayError::ReservedId {
                    list: OverlayList::Hidden,
                    id: id.clone(),
                });
            }
        }
        for id in &self.order {
            if id.trim().is_empty() {
                errors.push(CatalogueOverlayError::BlankId {
                    list: OverlayList::Order,
                });
            }
        }
        for (alias, target) in &self.aliases {
            errors.extend(self.alias_refusal(alias, target, catalogue));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Why one alias is refused, or `None` when it is valid. An alias may not be
    /// a malformed id, a reserved id, or the id of a model the catalogue lists or
    /// the overlay hides, and it must name a listed model that is not hidden.
    fn alias_refusal(
        &self,
        alias: &str,
        target: &str,
        catalogue: &[ModelInfo],
    ) -> Option<CatalogueOverlayError> {
        let listed = |id: &str| {
            catalogue
                .iter()
                .any(|model| model.id.eq_ignore_ascii_case(id))
        };
        if !is_alias_id(alias) {
            Some(CatalogueOverlayError::MalformedAlias {
                alias: alias.to_owned(),
            })
        } else if is_reserved_id(alias) {
            Some(CatalogueOverlayError::ReservedId {
                list: OverlayList::Aliases,
                id: alias.to_owned(),
            })
        } else if listed(alias) || self.is_hidden(alias) {
            Some(CatalogueOverlayError::ShadowsModel {
                alias: alias.to_owned(),
            })
        } else if self.is_hidden(target) {
            Some(CatalogueOverlayError::HiddenTarget {
                alias: alias.to_owned(),
                target: target.to_owned(),
            })
        } else if !listed(target) {
            Some(CatalogueOverlayError::UnknownTarget {
                alias: alias.to_owned(),
                target: target.to_owned(),
            })
        } else {
            None
        }
    }

    /// Apply the overlay to a listing the providers built, after the dedupe.
    /// Hidden models drop out. Each alias lists as a copy of its target under the
    /// alias id, unless that id is already listed or the target is not listed.
    /// The models the order names come first, in that order; the rest keep the
    /// order they had.
    pub fn apply_to_listing(&self, models: &mut Vec<pb::ModelInfo>) {
        if self.is_empty() {
            return;
        }
        models.retain(|model| !self.is_hidden(&model.id));
        let listed: HashSet<String> = models
            .iter()
            .map(|model| model.id.to_ascii_lowercase())
            .collect();
        let aliased: Vec<pb::ModelInfo> = self
            .aliases
            .iter()
            .filter(|(alias, _)| {
                !listed.contains(&alias.to_ascii_lowercase()) && !self.is_hidden(alias.as_str())
            })
            .filter_map(|(alias, target)| {
                models
                    .iter()
                    .find(|model| model.id.eq_ignore_ascii_case(target.as_str()))
                    .map(|model| pb::ModelInfo {
                        id: alias.clone(),
                        ..model.clone()
                    })
            })
            .collect();
        models.extend(aliased);
        let mut rank: HashMap<String, usize> = HashMap::new();
        for (position, id) in self.order.iter().enumerate() {
            rank.entry(id.to_ascii_lowercase()).or_insert(position);
        }
        models.sort_by_key(|model| {
            rank.get(&model.id.to_ascii_lowercase())
                .copied()
                .unwrap_or(usize::MAX)
        });
    }
}

/// Which of the overlay's lists a refusal is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayList {
    /// The hidden set.
    Hidden,
    /// The order.
    Order,
    /// The alias map.
    Aliases,
}

impl fmt::Display for OverlayList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Hidden => "hidden",
            Self::Order => "order",
            Self::Aliases => "aliases",
        })
    }
}

/// An overlay the platform refuses (EP-13 D6, F-t3-065). Serialized as the
/// `details` of an `invalid_catalogue_overlay` refusal, so an admin row can show it.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum CatalogueOverlayError {
    /// A list holds more entries than the overlay allows.
    #[error("the {list} list holds {count} entries, the most is {max}")]
    TooMany {
        list: OverlayList,
        count: usize,
        max: usize,
    },
    /// A list holds an entry that is blank.
    #[error("the {list} list holds a blank model id")]
    BlankId { list: OverlayList },
    /// An entry names something the routing layer answers for itself.
    #[error("`{id}` is answered by the routing layer and cannot appear in the {list} list")]
    ReservedId { list: OverlayList, id: String },
    /// An alias id is not one to 64 characters, each a lower-case letter, a digit, `.`, `_` or `-`.
    #[error("alias `{alias}` is not a valid id: lower-case letters, digits, `.`, `_` and `-`, at most 64")]
    MalformedAlias { alias: String },
    /// An alias is the id of a model the catalogue lists or the overlay hides.
    #[error("alias `{alias}` is the id of a model the catalogue lists or hides")]
    ShadowsModel { alias: String },
    /// An alias names a model the chat catalogue does not list.
    #[error("alias `{alias}` names `{target}`, which the chat catalogue does not list")]
    UnknownTarget { alias: String, target: String },
    /// An alias names a model the overlay hides.
    #[error("alias `{alias}` names `{target}`, which the overlay hides")]
    HiddenTarget { alias: String, target: String },
}

/// Whether `id` is one the routing layer answers for itself rather than a concrete
/// model: an automatic id, a `verevon-*` mode, or the side-task role. The overlay
/// never hides, lists or aliases one.
#[must_use]
pub fn is_reserved_id(id: &str) -> bool {
    is_unspecified_model(id) || id.trim().eq_ignore_ascii_case(ROLE_MODEL_ID)
}

/// Whether `alias` is a valid alias id: one to [`MAX_ALIAS_LEN`] lower-case ASCII
/// letters, digits, `.`, `_` or `-`.
fn is_alias_id(alias: &str) -> bool {
    (1..=MAX_ALIAS_LEN).contains(&alias.len())
        && alias.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// Whether an explicit env value turns the overlay on. Pure, so the reading can be
/// tested without the process environment.
fn catalogue_overlay_enabled_from(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// Whether the overlay is on for this process, read once from
/// `INFERENCE_CATALOGUE_OVERLAY`.
#[must_use]
pub fn catalogue_overlay_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        catalogue_overlay_enabled_from(std::env::var(CATALOGUE_OVERLAY_ENV).ok().as_deref())
    })
}

/// The switch that turns the per-organisation overlay on (W4-BYO-3). Only the literal `true` enables it.
pub const ORG_OVERLAY_ENV: &str = "INFERENCE_ORG_OVERLAY";

/// The most candidates one organisation role may hold, as the platform role table allows.
pub const MAX_ORG_ROLE_CANDIDATES: usize = 4;

/// One organisation's overlay on the platform catalogue and role table (W4-BYO-3, EP-13 D6, F-t3-065).
/// session-core stores it per organisation and inference-core reads it through the routing policy service.
///
/// It only narrows. It hides models, orders the listing and names role candidates, and it has no aliases and
/// nothing that adds a model: a key outside these three is ignored, and a role candidate the platform catalogue
/// does not list is dropped when the roles are read. The caller's entitlement and the ZDR, privacy-tier and
/// residency floors still apply to anything it picks. It is dark until `INFERENCE_ORG_OVERLAY` is the literal
/// `true`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct OrgOverlay {
    /// Model ids the organisation's listing drops.
    #[serde(default)]
    pub hidden: Vec<String>,
    /// Model ids the organisation's listing puts first, in this order.
    #[serde(default)]
    pub order: Vec<String>,
    /// Per side-task purpose (the role table's snake_case names), the candidate model ids tried before the
    /// platform role table's.
    #[serde(default)]
    pub roles: BTreeMap<String, Vec<String>>,
}

impl OrgOverlay {
    /// Reads a stored `config_json`. An empty or `{}` document, and one that does not parse, is the empty
    /// overlay, so a stored overlay that cannot be read changes nothing. The lists are capped at the platform
    /// overlay's limits.
    #[must_use]
    pub fn from_json(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "{}" {
            return Self::default();
        }
        match serde_json::from_str::<Self>(trimmed) {
            Ok(mut overlay) => {
                overlay.hidden.truncate(MAX_HIDDEN);
                overlay.order.truncate(MAX_ORDER);
                overlay
            }
            Err(error) => {
                tracing::warn!(%error, "org routing overlay parse failed; ignoring it");
                Self::default()
            }
        }
    }

    /// Whether the overlay changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.order.is_empty() && self.roles.is_empty()
    }

    /// Whether the overlay hides `id`, matching ids without regard to ASCII case or surrounding whitespace, as
    /// [`CatalogueOverlay::is_hidden`] does.
    #[must_use]
    pub fn is_hidden(&self, id: &str) -> bool {
        let id = id.trim();
        self.hidden
            .iter()
            .any(|hidden| hidden.trim().eq_ignore_ascii_case(id))
    }

    /// Apply the overlay to a listing, after the platform overlay. Hidden models drop out and the models the
    /// order names come first, in that order; the rest keep the order they had. It never adds, renames or
    /// changes a model, so the listing can only get shorter or be reordered.
    pub fn apply_to_listing(&self, models: &mut Vec<pb::ModelInfo>) {
        if self.is_empty() {
            return;
        }
        models.retain(|model| !self.is_hidden(&model.id));
        let mut rank: HashMap<String, usize> = HashMap::new();
        for (position, id) in self.order.iter().enumerate() {
            rank.entry(id.to_ascii_lowercase()).or_insert(position);
        }
        models.sort_by_key(|model| {
            rank.get(&model.id.to_ascii_lowercase())
                .copied()
                .unwrap_or(usize::MAX)
        });
    }

    /// The organisation's role candidates for `purpose`, in the organisation's order, limited to ids the platform
    /// `catalogue` lists. An id the catalogue does not list, a reserved routing id, a model the organisation
    /// hides, and for a small-budget purpose a model that cannot take a no-reasoning call, are dropped, as the
    /// platform role table refuses them. The ids come back spelled as the catalogue spells them. The caller
    /// still asks the call's entitlement and ZDR, privacy-tier and residency gates of each one.
    #[must_use]
    pub fn role_candidates(&self, purpose: RolePurpose, catalogue: &[ModelInfo]) -> Vec<String> {
        let Some(candidates) = self.roles.get(purpose.as_str()) else {
            return Vec::new();
        };
        let mut picked: Vec<String> = Vec::new();
        for candidate in candidates {
            if picked.len() >= MAX_ORG_ROLE_CANDIDATES {
                break;
            }
            if is_reserved_id(candidate) || self.is_hidden(candidate) {
                continue;
            }
            let Some(model) = catalogue
                .iter()
                .find(|model| model.id.eq_ignore_ascii_case(candidate))
            else {
                continue;
            };
            if purpose.is_small_budget() && !accepts_no_reasoning(model) {
                continue;
            }
            if !picked
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&model.id))
            {
                picked.push(model.id.clone());
            }
        }
        picked
    }
}

/// Whether an explicit env value turns the organisation overlay on. Pure, so the reading can be tested without
/// the process environment.
fn org_overlay_enabled_from(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// Whether the organisation overlay is on for this process, read once from `INFERENCE_ORG_OVERLAY`.
#[must_use]
pub fn org_overlay_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED
        .get_or_init(|| org_overlay_enabled_from(std::env::var(ORG_OVERLAY_ENV).ok().as_deref()))
}

/// Switch parsers and the switch-off path (verification plan 3.4).
#[cfg(test)]
#[path = "catalogue_overlay_switch_tests.rs"]
mod switch_tests;

#[cfg(test)]
mod match_tests {
    use super::*;

    /// Surrounding whitespace is not part of an id, on either side: a padded hidden entry hides the model it names,
    /// and a padded id asked about is the same model.
    #[test]
    fn a_padded_id_on_either_side_names_the_same_model() {
        let platform = CatalogueOverlay {
            hidden: BTreeSet::from([" gpt-5-mini ".to_owned()]),
            ..CatalogueOverlay::default()
        };
        assert!(platform.is_hidden("gpt-5-mini"));
        assert!(platform.is_hidden(" GPT-5-MINI\n"));
        assert!(!platform.is_hidden("gpt-5"));
        assert!(!platform.is_hidden("gpt-5-nano"));

        let organisation = OrgOverlay {
            hidden: vec!["\tgpt-4o-mini".to_owned()],
            ..OrgOverlay::default()
        };
        assert!(organisation.is_hidden("gpt-4o-mini "));
        assert!(organisation.is_hidden("GPT-4o-Mini"));
        assert!(!organisation.is_hidden("gpt-4o"));
    }
}
